//! Triangulating a profile's region for the caps.
//!
//! The caps are the constrained Delaunay triangulation of the polygon of
//! the segments' chords, with each chord's triangle taking the segment's
//! curve as its edge. The chain is already separated, so the chords don't
//! cross and each bulge lies clear of everything but its own triangle's
//! side. What is left is to keep every cap patch's corners open, measured
//! between the curves' tangents rather than the chords:
//!
//! - A triangle with two of the loop's segments meeting at a corner (an
//!   ear) whose tangents turn by about 180° or more, as two arcs in a row
//!   of a circle do, gets a Steiner point at its centroid.
//! - A corner between a curved segment and an inner edge that is too
//!   narrow or too wide gets that segment halved: its tangent turns
//!   towards its chord. For a segment bulging into the region, that keeps
//!   the bulge inside its triangle.
//! - A triangle whose corners are all open but whose patch still fails
//!   the fold check (a bulge of weight above 1 into the triangle can) gets
//!   its curved segments halved: their weights go towards 1.
//!
//! A Steiner point inside the hull of a segment bulging into the region
//! could end up outside the region once that segment is halved, so that
//! segment is halved instead. Each round triangulates afresh; the rounds
//! stop when nothing changes, or fail after [`MAX_ROUNDS`].

use std::collections::VecDeque;

use glam::DVec2;
use spade::handles::FixedVertexHandle;
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};

use super::chain::{Chain, SIN_MIN, Seg, chord, next_in_loop};
use crate::budget::Work;
use crate::mesh::{Bvh, apart};
use crate::patch::{Bounds3, Patch};
use crate::profile::ProfileError;
use crate::{KernelError, MAX_PATCHES};

/// The most rounds of triangulating and mending.
const MAX_ROUNDS: usize = 32;

/// How many times a segment may have been halved, all told, before the
/// caps give up halving it: mending that doesn't converge would otherwise
/// double the segments it can't mend every round.
const MAX_CAP_DEPTH: u8 = 16;

/// Work units per vertex for one triangulation: inserting a vertex walks
/// and flips a few edges.
const TRIANGULATION_WORK: usize = 8;

/// The caps' triangles.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Cap {
    /// Points added inside the region. Vertex ids run through the chain's
    /// vertices, then these.
    pub steiner: Vec<DVec2>,
    /// Counter-clockwise in the profile's plane.
    pub tris: Vec<[u32; 3]>,
}

/// The region of `chain` (separated) as triangles, halving segments of
/// `chain` where a cap patch needs it.
pub(super) fn triangulate(
    chain: &mut Chain,
    margin: f64,
    work: &mut Work,
) -> Result<Cap, KernelError> {
    let mut steiner: Vec<DVec2> = Vec::new();
    for _ in 0..MAX_ROUNDS {
        let (segs, starts) = chain.flat();
        let n = segs.len();
        if n.saturating_add(steiner.len()) > MAX_PATCHES / 4 {
            return Err(KernelError::TooComplex);
        }
        let tris = region(&segs, &starts, &steiner)?;
        work.spend((n + steiner.len()).saturating_mul(TRIANGULATION_WORK) + tris.len())?;
        let at = |v: u32| {
            let v = v as usize;
            if v < n {
                segs[v].conic.p0
            } else {
                steiner[v - n]
            }
        };
        let (mut split, ears) = mend(&segs, &starts, &at, &tris);
        let added = place(&segs, &at, &tris, &ears, margin, &mut split);
        if split.is_empty() && added.is_empty() {
            return Ok(Cap { steiner, tris });
        }
        split.sort_unstable();
        split.dedup();
        chain.split(&split, MAX_CAP_DEPTH, KernelError::TooComplex)?;
        steiner.extend(added);
    }
    Err(KernelError::TooComplex)
}

/// A spade point, with coordinates too small for it (below about
/// `1e-43`, and not zero) taken as zero. The triangulation only decides
/// which triangles there are; the chain's separation keeps every choice
/// it makes far above such a difference.
fn point(p: DVec2) -> Point2<f64> {
    let flush = |x: f64| if x.abs() < 1e-30 { 0.0 } else { x };
    Point2::new(flush(p.x), flush(p.y))
}

/// The triangles of the region: the constrained Delaunay triangulation of
/// the chords and the Steiner points, the triangles where the loops wind
/// once. Loops that wind anywhere other than 0 or 1 times don't nest.
fn region(segs: &[Seg], starts: &[u32], steiner: &[DVec2]) -> Result<Vec<[u32; 3]>, KernelError> {
    let triangulation = || KernelError::Profile(ProfileError::Triangulation);
    let mut cdt = ConstrainedDelaunayTriangulation::<Point2<f64>>::new();
    let points = segs
        .iter()
        .map(|s| s.conic.p0)
        .chain(steiner.iter().copied());
    for (i, p) in points.enumerate() {
        let handle = cdt.insert(point(p)).map_err(|_| triangulation())?;
        // A repeated point comes back as the vertex already there.
        if handle.index() != i {
            return Err(triangulation());
        }
    }
    let next = next_in_loop(starts);
    let v = FixedVertexHandle::from_index;
    for i in 0..segs.len() as u32 {
        if cdt
            .try_add_constraint(v(i as usize), v(next(i) as usize))
            .is_empty()
        {
            return Err(triangulation());
        }
    }
    // A point exactly on a chord would have split it.
    for i in 0..segs.len() as u32 {
        if !cdt.exists_constraint(v(i as usize), v(next(i) as usize)) {
            return Err(triangulation());
        }
    }

    // Each inner face's corners and, per side, the face across it and
    // the side's ends, counter-clockwise.
    let outer = cdt.outer_face().fix().index();
    let nf = cdt.num_all_faces();
    let mut corners = vec![None; nf];
    let mut across = vec![[(0usize, 0u32, 0u32); 3]; nf];
    for face in cdt.inner_faces() {
        let f = face.fix().index();
        corners[f] = Some(face.vertices().map(|v| v.fix().index() as u32));
        for (k, e) in face.adjacent_edges().into_iter().enumerate() {
            let (a, b) = (e.from().fix().index() as u32, e.to().fix().index() as u32);
            across[f][k] = (e.rev().face().fix().index(), a, b);
        }
    }

    // How the winding number changes from the right of the side `a → b`
    // to its left: by one across a chord run forwards, back by one across
    // a chord run backwards.
    let step = |a: u32, b: u32| -> i32 {
        match chord(starts, a, b) {
            Some((_, true)) => 1,
            Some((_, false)) => -1,
            None => 0,
        }
    };
    let nesting = || KernelError::Profile(ProfileError::Nesting);
    let mut winding: Vec<Option<i32>> = vec![None; nf];
    let mut queue = VecDeque::new();
    let set = |winding: &mut [Option<i32>], queue: &mut VecDeque<usize>, f: usize, value| {
        match winding[f] {
            None => {
                winding[f] = Some(value);
                queue.push_back(f);
                Ok(())
            }
            Some(w) if w == value => Ok(()),
            Some(_) => Err(nesting()),
        }
    };
    // The outer face, around the hull, winds 0 times.
    for f in 0..nf {
        if corners[f].is_some() {
            for &(g, a, b) in &across[f] {
                if g == outer {
                    set(&mut winding, &mut queue, f, step(a, b))?;
                }
            }
        }
    }
    while let Some(f) = queue.pop_front() {
        let w = winding[f].expect("queued faces have a winding");
        for &(g, a, b) in &across[f] {
            if g != outer {
                set(&mut winding, &mut queue, g, w - step(a, b))?;
            }
        }
    }
    let mut tris = Vec::new();
    for (f, corners) in corners.iter().enumerate() {
        if let Some(corners) = corners {
            match winding[f] {
                Some(0) => {}
                Some(1) => tris.push(*corners),
                _ => return Err(nesting()),
            }
        }
    }
    if tris.is_empty() {
        return Err(nesting());
    }
    Ok(tris)
}

/// What the triangles need mended: the segments to halve, and the
/// triangles (by index) to put a Steiner point in.
fn mend(
    segs: &[Seg],
    starts: &[u32],
    at: &impl Fn(u32) -> DVec2,
    tris: &[[u32; 3]],
) -> (Vec<u32>, Vec<usize>) {
    let curved = |e: Option<(u32, bool)>| e.is_some_and(|(s, _)| segs[s as usize].curved);
    let mut split = Vec::new();
    let mut ears = Vec::new();
    for (t, tri) in tris.iter().enumerate() {
        let sides = [0, 1, 2].map(|k| chord(starts, tri[k], tri[(k + 1) % 3]));
        if !sides.iter().any(|&e| curved(e)) {
            continue;
        }
        let (mut ear, mut narrow) = (false, false);
        for k in 0..3 {
            let (v, a, b) = (tri[k], tri[(k + 1) % 3], tri[(k + 2) % 3]);
            // The side into `v` is stored from `b`: flip which end is `v`.
            let (ea, eb) = (sides[k], sides[(k + 2) % 3].map(|(s, start)| (s, !start)));
            if !curved(ea) && !curved(eb) {
                continue;
            }
            // Along the side from `v` to `w`: its curve's tangent there.
            let direction = |e: Option<(u32, bool)>, w: u32| match e {
                None => at(w) - at(v),
                Some((s, start)) => segs[s as usize].tangent(start),
            };
            let (da, db) = (direction(ea, a), direction(eb, b));
            if da.perp_dot(db) > SIN_MIN * da.length() * db.length() {
                continue;
            }
            if ea.is_some() && eb.is_some() {
                ear = true;
            } else {
                narrow = true;
                let (s, _) = if curved(ea) { ea } else { eb }.expect("a curved chord");
                split.push(s);
            }
        }
        if ear {
            ears.push(t);
        } else if !narrow && patch(segs, at, tri, &sides).fold_direction().is_none() {
            // Open corners but a fold inside (a bulge of weight above 1
            // into the triangle can): halve its curves.
            split.extend(
                sides
                    .iter()
                    .filter(|&&e| curved(e))
                    .map(|e| e.expect("a curved chord").0),
            );
        }
    }
    (split, ears)
}

/// The cap patch on triangle `tri`, in the plane `z = 0`, with its
/// `sides` (the segment along each, if any) as its edges.
fn patch(
    segs: &[Seg],
    at: &impl Fn(u32) -> DVec2,
    tri: &[u32; 3],
    sides: &[Option<(u32, bool)>; 3],
) -> Patch {
    let p = tri.map(|v| at(v).extend(0.0));
    let mut c = [0, 1, 2].map(|k| (p[k] + p[(k + 1) % 3]) * 0.5);
    let mut w = [1.0; 3];
    for k in 0..3 {
        if let Some((s, _)) = sides[k] {
            let seg = &segs[s as usize];
            if seg.curved {
                c[k] = seg.conic.c.extend(0.0);
                w[k] = seg.conic.w;
            }
        }
    }
    Patch { p, c, w }
}

/// The Steiner points for `ears`, their triangles' centroids, except
/// where one lies within `margin` of the hull of a segment bulging into
/// the region: that segment goes into `split` instead, to be halved.
fn place(
    segs: &[Seg],
    at: &impl Fn(u32) -> DVec2,
    tris: &[[u32; 3]],
    ears: &[usize],
    margin: f64,
    split: &mut Vec<u32>,
) -> Vec<DVec2> {
    let concave: Vec<u32> = (0..segs.len() as u32)
        .filter(|&s| segs[s as usize].concave())
        .collect();
    let bvh = Bvh::new(
        concave
            .iter()
            .map(|&s| Bounds3::around(&segs[s as usize].hull()).expect("three points"))
            .collect(),
    );
    let mut added = Vec::new();
    let mut near = Vec::new();
    for &t in ears {
        let [a, b, c] = tris[t];
        let centroid = (at(a) + at(b) + at(c)) / 3.0;
        let p = centroid.extend(0.0);
        near.clear();
        bvh.query(&Bounds3::point(p), margin, &mut near);
        let inside = near
            .iter()
            .map(|&i| concave[i as usize])
            .find(|&s| !apart(&segs[s as usize].hull(), &[p], margin));
        match inside {
            Some(s) => split.push(s),
            None => added.push(centroid),
        }
    }
    added
}
