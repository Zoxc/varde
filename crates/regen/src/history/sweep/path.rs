//! A sweep's path as built from its parts: model edge chains ordered
//! end to end, parts joined and checked for corners, the end its twist
//! knob stands at, and the numbers that key it.

use glam::DVec3;
use varde_kernel_compat::measure::{EdgeShape, edge_shape};
use varde_kernel_compat::patch::Conic3;
use varde_kernel_compat::sweep::{self as path_sweep, Path, Piece};
use varde_kernel_compat::{Evidence, Frame, Solid, Tolerance, Topology};

use super::super::Failed;
use crate::SweepFound;
use crate::cache::Keyer;
use crate::why::{self, SweepRefusal};

/// How far apart two tangents at a joint may be, or the path's start
/// from square to the profile's plane, as a sine: the kernel's own rule,
/// which its sweep guards with.
const TANGENT_SINE: f64 = path_sweep::JOINT_SINE;

/// One part of a path as built: its pieces end to end, and whether they
/// close on themselves.
pub(in crate::history) struct Part {
    pub(in crate::history) pieces: Vec<Piece>,
    pub(in crate::history) closed: bool,
}

/// How many points each conic of a curved piece is walked by, carrying
/// the profile's x along an open chain to its end.
const END_STEPS: usize = 16;

/// Where an open chain of `pieces` ends, for the sweep's twist knob
/// ([`SweepFound::End`]): its end, its tangent there, and the x of the
/// profile's `frame` carried there, by rotation-minimizing frames
/// (double reflection over the pieces' points) to follow the path, or
/// taken square to the end's tangent to keep it. `None` for a chain
/// with no length at its start or end.
pub(super) fn chain_end(
    pieces: &[Piece],
    frame: &Frame,
    orientation: path_sweep::Orientation,
) -> Option<SweepFound> {
    // Points along the chain with their tangents (not unit).
    let mut walk: Vec<(DVec3, DVec3)> = Vec::new();
    for piece in pieces {
        match piece {
            Piece::Line { from, to } => {
                walk.push((*from, *to - *from));
                walk.push((*to, *to - *from));
            }
            Piece::Arc { conics, .. } | Piece::Curve { conics, .. } => {
                for conic in conics {
                    walk.extend(
                        (0..=END_STEPS).map(|k| conic.eval_deriv(k as f64 / END_STEPS as f64)),
                    );
                }
            }
        }
    }
    let unit = |(at, way): (DVec3, DVec3)| way.try_normalize().map(|way| (at, way));
    let walk: Vec<(DVec3, DVec3)> = walk.into_iter().filter_map(unit).collect();
    let (&(_, first), &(at, tangent)) = (walk.first()?, walk.last()?);
    let square = |x: DVec3, t: DVec3| (x - t * t.dot(x)).try_normalize();
    let zero = match orientation {
        path_sweep::Orientation::Keep => {
            square(frame.x, tangent).or_else(|| square(frame.y, tangent))?
        }
        path_sweep::Orientation::Follow => {
            let mut x = square(frame.x, first)?;
            for pair in walk.windows(2) {
                let [(p0, t0), (p1, t1)] = [pair[0], pair[1]];
                let v1 = p1 - p0;
                let c1 = v1.dot(v1);
                if c1 <= 0.0 {
                    continue;
                }
                let x_l = x - v1 * (2.0 * v1.dot(x) / c1);
                let t_l = t0 - v1 * (2.0 * v1.dot(t0) / c1);
                let v2 = t1 - t_l;
                let c2 = v2.dot(v2);
                x = if c2 > 0.0 {
                    x_l - v2 * (2.0 * v2.dot(x_l) / c2)
                } else {
                    x_l
                };
            }
            square(x, tangent)?
        }
    };
    Some(SweepFound::End {
        at: at.to_array(),
        tangent: tangent.to_array(),
        zero: zero.to_array(),
    })
}

/// "Its path has a corner", the corner at `at` drawn.
pub(super) fn corner(at: DVec3, tolerance: &Tolerance) -> Failed {
    let mut evidence = Evidence::default();
    evidence.add_points([at]);
    Failed::with_evidence(
        why::sweep_refused(SweepRefusal::Corner),
        &evidence,
        tolerance,
    )
}

/// `chains` of `topology` (sorted, each once) joined end to end by their
/// vertices: each chain and whether it runs backwards along the walk,
/// and whether they close. A chain closed on itself must be alone; three
/// ends at a vertex, or chains in several pieces, are refused.
pub(super) fn order_chains(
    solid: &Solid,
    topology: &Topology,
    chains: &[u32],
) -> Result<(Vec<(u32, bool)>, bool), Failed> {
    let mesh = solid.mesh();
    let all = topology.chains();
    let ends: Vec<[u32; 2]> = (chains.iter())
        .map(|&c| {
            let halfedges = &all[c as usize].halfedges;
            let (first, last) = (halfedges[0], halfedges[halfedges.len() - 1]);
            [mesh.halfedge(first).start, mesh.end(last)]
        })
        .collect();
    if let Some(closed) = chains.iter().position(|&c| all[c as usize].closed) {
        return match chains.len() {
            1 => Ok((vec![(chains[closed], false)], true)),
            _ => Err(why::PATH_CLOSED_NOT_ALONE.into()),
        };
    }
    // The chain ends at each vertex, found by binary search: a long
    // tangent chain is ordered in `n log n`.
    let mut by_vertex: Vec<(u32, usize, usize)> = (0..ends.len())
        .flat_map(|i| (0..2).map(move |e| (i, e)))
        .map(|(i, e)| (ends[i][e], i, e))
        .collect();
    by_vertex.sort_unstable();
    let at = |vertex: u32| -> &[(u32, usize, usize)] {
        let from = by_vertex.partition_point(|&(v, ..)| v < vertex);
        let to = by_vertex.partition_point(|&(v, ..)| v <= vertex);
        &by_vertex[from..to]
    };
    let branch = || Failed::from(why::PATH_EDGES_BRANCH);
    let mut start = None;
    for (i, pair) in ends.iter().enumerate() {
        for (e, &vertex) in pair.iter().enumerate() {
            let meeting = at(vertex).len();
            if meeting > 2 {
                return Err(branch());
            }
            if meeting == 1 && start.is_none() {
                start = Some((i, e));
            }
        }
    }
    let (loops, (mut i, mut e)) = match start {
        Some(start) => (false, start),
        None => (true, (0, 0)),
    };
    let mut walk = Vec::with_capacity(ends.len());
    let mut used = vec![false; ends.len()];
    loop {
        used[i] = true;
        walk.push((chains[i], e == 1));
        let vertex = ends[i][1 - e];
        let next = (at(vertex).iter()).find(|&&(_, j, _)| j != i && !used[j]);
        match next {
            Some(&(_, j, f)) => (i, e) = (j, f),
            None => break,
        }
    }
    if walk.len() != ends.len() {
        return Err(branch());
    }
    Ok((walk, loops))
}

/// The piece chain `chain` of `topology` makes on `solid`, run backwards
/// with `backwards`.
pub(super) fn chain_piece(
    solid: &Solid,
    topology: &Topology,
    chain: u32,
    backwards: bool,
) -> Piece {
    let chain = &topology.chains()[chain as usize];
    let mesh = solid.mesh();
    let mut conics: Vec<Conic3> = chain.halfedges.iter().map(|&h| mesh.curve(h)).collect();
    if backwards {
        reverse_conics(&mut conics);
    }
    match edge_shape(solid, chain) {
        EdgeShape::Line { .. } => Piece::Line {
            from: conics[0].p0,
            to: conics[conics.len() - 1].p1,
        },
        EdgeShape::Circle { centre, axis, .. } => Piece::Arc {
            conics,
            centre,
            axis: if backwards { -axis } else { axis },
        },
        EdgeShape::Ellipse { .. } | EdgeShape::Other => Piece::Curve {
            conics,
            normal: None,
        },
    }
}

/// The path `parts` make, joined as the module's docs say, for a profile
/// on the plane through `start.0` square to the unit `start.1`, whose
/// box's middle `middle` gives (worked out only where several ends are
/// on the plane: it takes the profile).
pub(in crate::history) fn join(
    mut parts: Vec<Part>,
    (origin, normal): (DVec3, DVec3),
    middle: impl FnOnce() -> DVec3,
    tolerance: &Tolerance,
) -> Result<Path, Failed> {
    let resolution = tolerance.resolution();
    if parts.iter().any(|part| part.closed) {
        if parts.len() > 1 {
            return Err(why::PATH_CLOSED_NOT_ALONE.into());
        }
        let pieces = parts.remove(0).pieces;
        check_joints(&pieces, true, tolerance)?;
        return Ok(Path::Chain {
            pieces,
            closed: true,
        });
    }
    let ends = |part: &Part| -> Option<[DVec3; 2]> {
        Some([part.pieces.first()?.start()?, part.pieces.last()?.end()?])
    };
    // The ends on the profile's plane; of several, the nearest its
    // middle (the first of equals).
    let mut on_plane: Vec<(usize, usize, DVec3)> = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        let Some(points) = ends(part) else {
            return Err(why::PATH_NOT_FOUND.into());
        };
        for (e, point) in points.into_iter().enumerate() {
            if (point - origin).dot(normal).abs() <= resolution {
                on_plane.push((i, e, point));
            }
        }
    }
    let (i, e) = match on_plane.as_slice() {
        [] => return Err(why::PATH_OFF_START.into()),
        &[(i, e, _)] => (i, e),
        several => {
            let middle = middle();
            let mut first = (several[0].0, several[0].1);
            let mut nearest = several[0].2.distance_squared(middle);
            for &(i, e, point) in &several[1..] {
                let distance = point.distance_squared(middle);
                if distance < nearest {
                    (first, nearest) = ((i, e), distance);
                }
            }
            first
        }
    };
    let mut chain = Vec::new();
    let mut part = parts.remove(i);
    if e == 1 {
        reverse(&mut part.pieces);
    }
    chain.append(&mut part.pieces);
    while !parts.is_empty() {
        let at = (chain.last())
            .and_then(Piece::end)
            .ok_or(why::PATH_NOT_FOUND)?;
        let mut found = None;
        for (i, part) in parts.iter().enumerate() {
            let points = ends(part).ok_or(why::PATH_NOT_FOUND)?;
            for (e, point) in points.into_iter().enumerate() {
                if point.distance(at) <= resolution {
                    if found.is_some() {
                        return Err(why::PATH_PARTS_APART.into());
                    }
                    found = Some((i, e));
                }
            }
        }
        let (i, e) = found.ok_or(why::PATH_PARTS_APART)?;
        let mut part = parts.remove(i);
        if e == 1 {
            reverse(&mut part.pieces);
        }
        chain.append(&mut part.pieces);
    }
    check_joints(&chain, false, tolerance)?;
    let start = chain
        .first()
        .and_then(|piece| tangents(piece).map(|t| t[0]));
    if start.is_none_or(|t| t.cross(normal).length() > TANGENT_SINE) {
        return Err(why::PATH_NOT_SQUARE.into());
    }
    Ok(Path::Chain {
        pieces: chain,
        closed: false,
    })
}

/// `pieces` run the other way.
fn reverse(pieces: &mut [Piece]) {
    pieces.reverse();
    for piece in pieces {
        match piece {
            Piece::Line { from, to } => std::mem::swap(from, to),
            Piece::Arc { conics, axis, .. } => {
                reverse_conics(conics);
                *axis = -*axis;
            }
            Piece::Curve { conics, .. } => reverse_conics(conics),
        }
    }
}

fn reverse_conics(conics: &mut [Conic3]) {
    conics.reverse();
    for conic in conics {
        *conic = conic.reversed();
    }
}

/// The unit tangents `piece` starts and ends along, the way it runs;
/// `None` where one has no direction.
fn tangents(piece: &Piece) -> Option<[DVec3; 2]> {
    match piece {
        Piece::Line { from, to } => {
            let t = (*to - *from).try_normalize()?;
            Some([t, t])
        }
        Piece::Arc { conics, .. } | Piece::Curve { conics, .. } => {
            let (first, last) = (conics.first()?, conics.last()?);
            let leaving = (first.c - first.p0)
                .try_normalize()
                .or_else(|| (first.p1 - first.p0).try_normalize())?;
            let arriving = (last.p1 - last.c)
                .try_normalize()
                .or_else(|| (last.p1 - last.p0).try_normalize())?;
            Some([leaving, arriving])
        }
    }
}

/// Checks that each of `pieces` runs on from the one before (and the
/// first from the last, if `closed`) along the same tangent, within a
/// sine of [`TANGENT_SINE`]; a corner fails with its joint drawn. A
/// piece closed on itself (a circle, a closed spline, a rim) has no
/// joint: where its last conic meets its first is inside the piece, as
/// its other conics' joints are, which a traced chain's fit leaves
/// tangent only to the tolerance.
fn check_joints(pieces: &[Piece], closed: bool, tolerance: &Tolerance) -> Result<(), Failed> {
    let count = pieces.len();
    let joints = if closed && count > 1 {
        count
    } else {
        count.saturating_sub(1)
    };
    for k in 0..joints {
        let (a, b) = (&pieces[k], &pieces[(k + 1) % count]);
        let at = b.start().unwrap_or(DVec3::ZERO);
        let (Some([_, out]), Some([into, _])) = (tangents(a), tangents(b)) else {
            return Err(corner(at, tolerance));
        };
        if out.dot(into) <= 0.0 || out.cross(into).length() > TANGENT_SINE {
            return Err(corner(at, tolerance));
        }
    }
    Ok(())
}

/// Every number of `path` into `keyer`, so the tool is filed by the path
/// as built.
pub(super) fn path_bits(keyer: &mut Keyer, path: &Path) {
    let conics = |keyer: &mut Keyer, conics: &[Conic3]| {
        keyer.number(conics.len() as u64);
        for conic in conics {
            (keyer.point(conic.p0).point(conic.c))
                .number(conic.w.to_bits())
                .point(conic.p1);
        }
    };
    match path {
        Path::Chain { pieces, closed } => {
            keyer.bytes(b"chain").number(u64::from(*closed));
            keyer.number(pieces.len() as u64);
            for piece in pieces {
                match piece {
                    Piece::Line { from, to } => {
                        keyer.bytes(b"line");
                        keyer.point(*from);
                        keyer.point(*to);
                    }
                    Piece::Arc {
                        conics: arc,
                        centre,
                        axis,
                    } => {
                        keyer.bytes(b"arc");
                        conics(keyer, arc);
                        keyer.point(*centre);
                        keyer.point(*axis);
                    }
                    Piece::Curve {
                        conics: curve,
                        normal,
                    } => {
                        keyer.bytes(b"curve");
                        conics(keyer, curve);
                        match normal {
                            Some(normal) => {
                                keyer.number(1);
                                keyer.point(*normal);
                            }
                            None => {
                                keyer.number(0);
                            }
                        }
                    }
                }
            }
        }
        Path::Helix(helix) => {
            keyer.bytes(b"helix");
            keyer.point(helix.point);
            keyer.point(helix.axis);
            keyer
                .number(helix.pitch.to_bits())
                .number(helix.turns.to_bits())
                .number(u64::from(helix.left));
        }
    }
}
