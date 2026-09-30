//! Merging over-refined patches back: the pair decisions split both
//! operands wherever they couldn't decide a pair yet, red–green, and
//! pieces far enough from any cut come through whole. A patch of that
//! refinement (an operand's patch, or a red split of one) all of whose
//! pieces came through whole is itself again.
//!
//! The candidates are the largest such patches. One is restored unless a
//! vertex inside it (a midpoint the refinement made) is still a corner of
//! a triangle outside it, such as a neighbour across one of its edges
//! still split finer: then its children are the candidates instead, and
//! so on until that holds. A restored patch is one face's, never across
//! faces, with the corners and edges it had in the refinement: exactly
//! the operand's surface.

use std::collections::BTreeSet;

use super::super::input::Side;
use super::{Curves, Refinement, key};
use crate::KernelError;
use crate::budget::Work;
use crate::mesh::Edge;

/// Replaces the kept whole pieces (`whole`: which refined triangle of
/// which operand each triangle is) of every refinement patch that can be
/// restored by that patch: see the [module](self) docs. `offsets` are
/// where each operand's vertices start.
#[allow(clippy::too_many_arguments)]
pub(super) fn merge(
    tris: &mut Vec<[u32; 3]>,
    faces: &mut Vec<u32>,
    whole: &mut Vec<Option<(Side, u32)>>,
    off: &mut Vec<bool>,
    curves: &mut Curves,
    refinement: &Refinement,
    offsets: [u32; 2],
    work: &mut Work,
) -> Result<(), KernelError> {
    for side in [Side::A, Side::B] {
        let k = side as usize;
        let (nodes, leaf) = (refinement.tree[k], refinement.leaf[k]);
        let n = nodes.len();
        if n == 0 {
            continue;
        }
        // Each node's children (red splits make four), and how many
        // pieces each leaf is and how many of them came through whole.
        let mut children: Vec<Vec<u32>> = vec![Vec::new(); n];
        for (id, node) in nodes.iter().enumerate() {
            if node.parent != u32::MAX {
                children[node.parent as usize].push(id as u32);
            }
        }
        let mut pieces = vec![0u32; n];
        for &l in leaf {
            pieces[l as usize] += 1;
        }
        let mut kept = vec![0u32; n];
        for &(s, t) in whole.iter().flatten() {
            if s == side {
                kept[leaf[t as usize] as usize] += 1;
            }
        }
        // Complete: every piece below came through whole. Children come
        // after their parents, so from the last node back.
        let mut complete = vec![false; n];
        for id in (0..n).rev() {
            complete[id] = if children[id].is_empty() {
                pieces[id] > 0 && kept[id] == pieces[id]
            } else {
                children[id].iter().all(|&c| complete[c as usize])
            };
        }
        // Worth restoring: split (four children), or a leaf that is two
        // green halves.
        let worth = |id: usize| !children[id].is_empty() || pieces[id] == 2;
        let mut candidate: Vec<bool> = (0..n)
            .map(|id| {
                let parent = nodes[id].parent;
                complete[id] && worth(id) && (parent == u32::MAX || !complete[parent as usize])
            })
            .collect();
        if !candidate.contains(&true) {
            continue;
        }
        let depth_limit = 2 * crate::MAX_REFINE_DEPTH as usize + 2;
        // The candidate each triangle is a piece of, if any.
        let under = |i: usize, candidate: &[bool]| -> Option<u32> {
            let (s, t) = whole[i]?;
            if s != side {
                return None;
            }
            let mut id = leaf[t as usize];
            for _ in 0..depth_limit {
                if candidate[id as usize] {
                    return Some(id);
                }
                id = nodes[id as usize].parent;
                if id == u32::MAX {
                    return None;
                }
            }
            None
        };
        // Vertex ids, for flags by vertex.
        let verts = tris
            .iter()
            .flatten()
            .copied()
            .chain(
                nodes
                    .iter()
                    .flat_map(|node| node.corners.map(|v| v + offsets[k])),
            )
            .max()
            .map_or(0, |v| v as usize + 1);
        let mut corner = vec![false; verts];
        // Each round demotes the candidates in the way a level down (each
        // round at least one, so it ends), a unit of work per triangle.
        loop {
            work.spend(tris.len())?;
            let covering: Vec<Option<u32>> =
                (0..tris.len()).map(|i| under(i, &candidate)).collect();
            // Corners of the triangles there will be.
            corner.fill(false);
            for (tri, c) in tris.iter().zip(&covering) {
                if c.is_none() {
                    for &v in tri {
                        corner[v as usize] = true;
                    }
                }
            }
            for (id, node) in nodes.iter().enumerate() {
                if candidate[id] {
                    for v in node.corners {
                        corner[(v + offsets[k]) as usize] = true;
                    }
                }
            }
            // A candidate with a vertex inside it that stays a corner
            // outside it gives way to its children.
            let mut demoted = BTreeSet::new();
            for (tri, c) in tris.iter().zip(&covering) {
                let Some(c) = c else { continue };
                let own = nodes[*c as usize].corners.map(|v| v + offsets[k]);
                let inside = tri.iter().any(|v| !own.contains(v) && corner[*v as usize]);
                if inside {
                    demoted.insert(*c);
                }
            }
            if demoted.is_empty() {
                break;
            }
            for c in demoted {
                candidate[c as usize] = false;
                for &ch in &children[c as usize] {
                    if worth(ch as usize) {
                        candidate[ch as usize] = true;
                    }
                }
            }
        }
        if !candidate.contains(&true) {
            continue;
        }
        // Replace the pieces by their candidate, in the candidates' order.
        let covering: Vec<Option<u32>> = (0..tris.len()).map(|i| under(i, &candidate)).collect();
        let mut face_of: Vec<(u32, u32)> = covering
            .iter()
            .zip(faces.iter())
            .filter_map(|(c, &f)| c.map(|c| (c, f)))
            .collect();
        face_of.sort_unstable();
        face_of.dedup_by_key(|x| x.0);
        let keep: Vec<bool> = covering.iter().map(Option::is_none).collect();
        retain(tris, &keep);
        retain(faces, &keep);
        retain(whole, &keep);
        retain(off, &keep);
        for (c, face) in face_of {
            let node = &nodes[c as usize];
            let corners = node.corners.map(|v| v + offsets[k]);
            for i in 0..3 {
                curves.insert(
                    key(corners[i], corners[(i + 1) % 3]),
                    Edge {
                        ctrl: node.patch.c[i],
                        weight: node.patch.w[i],
                    },
                );
            }
            tris.push(corners);
            faces.push(face);
            whole.push(None);
            off.push(false);
        }
    }
    Ok(())
}

/// Keeps the items of `list` whose flag in `keep` is set.
fn retain<T>(list: &mut Vec<T>, keep: &[bool]) {
    let mut i = 0;
    list.retain(|_| {
        i += 1;
        keep[i - 1]
    });
}
