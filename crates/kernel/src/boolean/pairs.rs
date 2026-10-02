//! Each pair of faces' cut: which of its ends join up, decided once for
//! the pair and read by both faces.
//!
//! The ends of a pair's cut are the crossings of its faces' edges
//! through the other face: an edge of `A`'s face through `B`'s, or of
//! `B`'s through `A`'s. The counting leaves as many going in as out.
//! Two flat triangles meet in one segment at most, so their two ends (or
//! none) join. Curved patches can meet in several arcs, and in closed
//! loops no edge crossing shows, so a pair is decided from its ends only
//! with a **certificate** that no loop hides in it: the two patches'
//! normal cones apart (the surfaces are never parallel there, and a loop
//! needs somewhere they are), both planar (planes meet in a line), or,
//! with no ends, their control hulls apart, or walls along one direction
//! that come near each other (they meet only in lines along it, which
//! run out of the pair through ends). Then no ends is no cut, and
//! two ends are one arc; the ends of two planar patches, on the line
//! their planes meet in, join in order along it. On walls along one
//! direction, ends join line by line where the walls cross clearly
//! (`along_generators`).
//!
//! Any other pair is **refined**: both patches are split exactly
//! (red–green, so the neighbours across split edges are split too, see
//! [`Refiner`]) and everything is counted again, every new vertex and
//! edge getting primitives of its own. Below a size floor
//! ([`MIN_SPLIT`] resolutions) a pair is decided by fixed rules instead:
//! no certificate means no loop, and the ends, taken round the pair in
//! order, join each going in to the next coming out (as parentheses
//! match), so the arcs don't cross.

use std::cmp::Ordering;

use glam::{DVec2, DVec3};

use super::BooleanError;
use super::count::{self, Counts};
use super::curved::Curved;
use super::input::Input;
use crate::budget::Work;
use crate::mesh::{MIN_SPLIT, Mesh, Node, Refiner, Surface, apart, samples};
use crate::par::par_map;
use crate::patch::NormalCone;
use crate::{KernelError, MAX_PATCHES, MAX_REFINE_DEPTH, Tolerance};

/// A cut arc of a pair of faces (triangle `tris[0]` of `A`, `tris[1]` of
/// `B`) between two of its ends, by vertex id: `plus`, the end whose
/// sign seen from `A` is +1, and `minus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Arc {
    pub(super) tris: [u32; 2],
    pub(super) plus: u32,
    pub(super) minus: u32,
}

/// The ids new vertices start from: after both operands' vertices come
/// the crossings of `A`'s edges (`x12`, in order), then `B`'s.
pub(super) fn first_ids(a: &Input, b: &Input, counts: &Counts) -> [u32; 2] {
    let first12 = (a.mesh.verts().len() + b.mesh.verts().len()) as u32;
    [first12, first12 + counts.x12.len() as u32]
}

/// Every pair of faces' ends, sorted by pair: (the pair, the end's vertex
/// id, its sign seen from `A`). A crossing of an edge of `A` through a
/// face of `B` is an end of the pairs of both of the edge's triangles with
/// that face, with its sign as each triangle runs the edge; one of an
/// edge of `B` likewise, with the sign turned over.
fn ends(a: &Input, b: &Input, counts: &Counts) -> Vec<([u32; 2], u32, i8)> {
    let [first12, first21] = first_ids(a, b, counts);
    let mut ends = Vec::with_capacity(2 * (counts.x12.len() + counts.x21.len()));
    for (i, c) in counts.x12.iter().enumerate() {
        let [forward, backward] = a.edge_tris[c.edge as usize];
        let id = first12 + i as u32;
        ends.push(([forward, c.face], id, c.x));
        ends.push(([backward, c.face], id, -c.x));
    }
    for (i, c) in counts.x21.iter().enumerate() {
        let [forward, backward] = b.edge_tris[c.edge as usize];
        let id = first21 + i as u32;
        ends.push(([c.face, forward], id, -c.x));
        ends.push(([c.face, backward], id, c.x));
    }
    ends.sort_unstable();
    ends
}

/// The arcs of flat operands: every pair's ends are two, one of each
/// sign, joined.
pub(super) fn flat(a: &Input, b: &Input, counts: &Counts) -> Result<Vec<Arc>, KernelError> {
    let ends = ends(a, b, counts);
    let mut arcs = Vec::with_capacity(ends.len() / 2);
    for pair in ends.chunk_by(|x, y| x.0 == y.0) {
        let [(tris, u, su), (_, v, sv)] = pair else {
            return Err(KernelError::Boolean(BooleanError::Inconsistent));
        };
        if su + sv != 0 {
            return Err(KernelError::Boolean(BooleanError::Inconsistent));
        }
        let (plus, minus) = if *su > 0 { (*u, *v) } else { (*v, *u) };
        arcs.push(Arc {
            tris: *tris,
            plus,
            minus,
        });
    }
    Ok(arcs)
}

/// What the pair decisions leave once every pair is decided: the operands
/// as refined for it (the same surfaces, split; their vertices first the
/// operands' own, then those refinement made), the tree of each
/// operand's red splits and each refined triangle's leaf in it, their
/// counts, and every pair of faces' arcs, sorted. What the tracing,
/// fitting and assembly of the curved cuts start from.
#[derive(Debug)]
pub(super) struct Refined {
    pub(super) a: Mesh,
    pub(super) b: Mesh,
    pub(super) tree: [Vec<Node>; 2],
    pub(super) leaf: [Vec<u32>; 2],
    pub(super) counts: Counts,
    pub(super) arcs: Vec<Arc>,
}

/// Counts `a` against `b` (whose meshes pass `check`, one of them with
/// curved patches) and decides every pair of faces, refining both until
/// it can: see the [module](self) docs. `grow` is whether `A` grows (a
/// union) or shrinks, for ties.
pub(super) fn refined(
    a: &Mesh,
    b: &Mesh,
    grow: bool,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<Refined, KernelError> {
    let floor = MIN_SPLIT * tol.resolution();
    // The refiners split leaves down to an eighth of the floor, for the
    // neighbours red–green splits along with a piece at the floor.
    let resolution = tol.resolution();
    let mut refiners = [
        Refiner::new(a, resolution, floor / 8.0),
        Refiner::new(b, resolution, floor / 8.0),
    ];
    let mut meshes = [a.clone(), b.clone()];
    // Each triangle's leaf in its refiner.
    let mut leaves: [Vec<u32>; 2] = [
        (0..a.tris().len() as u32).collect(),
        (0..b.tris().len() as u32).collect(),
    ];

    // A pair is split once a round, and at most `MAX_REFINE_DEPTH` times.
    for _ in 0..=2 * MAX_REFINE_DEPTH {
        let split = {
            let ia = Input::new(&meshes[0], tol);
            let ib = Input::new(&meshes[1], tol);
            let counts = counted(&ia, &ib, grow, tol, work)?;
            match decide(&ia, &ib, &counts, floor, tol.resolution(), work)? {
                Decision::Arcs(arcs) => Err((counts, arcs)),
                Decision::Split(split) => Ok(split),
            }
        };
        let split = match split {
            Ok(split) => split,
            Err((counts, arcs)) => {
                let [a, b] = meshes;
                let [ra, rb] = &refiners;
                return Ok(Refined {
                    a,
                    b,
                    tree: [ra.nodes().to_vec(), rb.nodes().to_vec()],
                    leaf: leaves,
                    counts,
                    arcs,
                });
            }
        };
        for k in 0..2 {
            if split[k].is_empty() {
                continue;
            }
            let wanted: Vec<u32> = split[k].iter().map(|&t| leaves[k][t as usize]).collect();
            refiners[k].split(&wanted, work)?;
            let pieces = refiners[k].pieces()?;
            if pieces.len() > MAX_PATCHES {
                return Err(KernelError::TooComplex);
            }
            work.spend(pieces.len())?;
            leaves[k] = pieces.iter().map(|p| p.leaf).collect();
            meshes[k] = refiners[k].mesh(&pieces);
        }
    }
    Err(KernelError::TooComplex)
}

/// One round's counting of operands with curved patches: [`refined`]
/// counts so every round, and [`touches`](super::touches) once, so its
/// counts are `refined`'s first round's, bit for bit.
pub(super) fn counted(
    a: &Input,
    b: &Input,
    grow: bool,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<Counts, KernelError> {
    count::count(a, b, &Curved::new(a, b, grow, tol), tol, work)
}

/// What a round of decisions comes to: every pair's arcs, or the
/// triangles of each operand to split first.
enum Decision {
    Arcs(Vec<Arc>),
    Split([Vec<u32>; 2]),
}

/// What one pair comes to.
enum PairDecision {
    Arcs(Vec<Arc>),
    Split { a: bool, b: bool },
}

/// One end of a pair's cut: its vertex id, its sign seen from `A`, and
/// where it is.
#[derive(Debug, Clone, Copy)]
struct End {
    id: u32,
    sign: i8,
    at: DVec3,
}

/// Decides every pair of faces that may meet: see the [module](self)
/// docs. Pieces no larger than `floor` across aren't split.
fn decide(
    a: &Input,
    b: &Input,
    counts: &Counts,
    floor: f64,
    resolution: f64,
    work: &mut Work,
) -> Result<Decision, KernelError> {
    // Where each crossing is.
    let at12: Vec<DVec3> = counts
        .x12
        .iter()
        .map(|c| a.conic(c.edge).eval(c.t))
        .collect();
    let at21: Vec<DVec3> = counts
        .x21
        .iter()
        .map(|c| b.conic(c.edge).eval(c.t))
        .collect();
    let [first12, first21] = first_ids(a, b, counts);
    let place = |id: u32| {
        if id < first21 {
            at12[(id - first12) as usize]
        } else {
            at21[(id - first21) as usize]
        }
    };
    let ends = ends(a, b, counts);
    // Every pair that may meet: those whose boxes do, and any with ends.
    let mut pairs: Vec<[u32; 2]> = counts.pairs.clone();
    pairs.extend(ends.iter().map(|e| e.0));
    pairs.sort_unstable();
    pairs.dedup();
    work.spend(pairs.len())?;
    let jobs: Vec<([u32; 2], Vec<End>)> = {
        let mut k = 0;
        pairs
            .iter()
            .map(|&pair| {
                let mut here = Vec::new();
                while k < ends.len() && ends[k].0 < pair {
                    k += 1;
                }
                while k < ends.len() && ends[k].0 == pair {
                    let (_, id, sign) = ends[k];
                    here.push(End {
                        id,
                        sign,
                        at: place(id),
                    });
                    k += 1;
                }
                (pair, here)
            })
            .collect()
    };
    let cones = |input: &Input, side: usize| {
        let mut used: Vec<u32> = pairs.iter().map(|p| p[side]).collect();
        used.sort_unstable();
        used.dedup();
        let found = par_map(&used, |&t| input.patches[t as usize].normal_cone());
        let mut all = vec![NormalCone::ALL; input.tris.len()];
        for (t, cone) in used.into_iter().zip(found) {
            all[t as usize] = cone;
        }
        all
    };
    let (cones_a, cones_b) = (cones(a, 0), cones(b, 1));
    let decided = par_map(&jobs, |(pair, ends)| {
        pair_decision(a, b, *pair, ends, [&cones_a, &cones_b], floor, resolution)
    });
    let mut arcs = Vec::new();
    let mut split = [Vec::new(), Vec::new()];
    for (&(pair, _), d) in jobs.iter().zip(decided) {
        match d.map_err(KernelError::Boolean)? {
            PairDecision::Arcs(mut here) => arcs.append(&mut here),
            PairDecision::Split { a, b } => {
                if a {
                    split[0].push(pair[0]);
                }
                if b {
                    split[1].push(pair[1]);
                }
            }
        }
    }
    if split.iter().any(|s| !s.is_empty()) {
        for s in &mut split {
            s.sort_unstable();
            s.dedup();
        }
        return Ok(Decision::Split(split));
    }
    Ok(Decision::Arcs(arcs))
}

/// Decides the pair of triangle `p` of `A` and `q` of `B` with `ends`.
fn pair_decision(
    a: &Input,
    b: &Input,
    [p, q]: [u32; 2],
    ends: &[End],
    cones: [&[NormalCone]; 2],
    floor: f64,
    resolution: f64,
) -> Result<PairDecision, BooleanError> {
    let (pa, pb) = (&a.patches[p as usize], &b.patches[q as usize]);
    let planar = a.planar[p as usize] && b.planar[q as usize];
    let arcs = |pairs: Vec<(End, End)>| {
        PairDecision::Arcs(
            pairs
                .into_iter()
                .map(|(x, y)| {
                    let (plus, minus) = if x.sign > 0 { (x, y) } else { (y, x) };
                    Arc {
                        tris: [p, q],
                        plus: plus.id,
                        minus: minus.id,
                    }
                })
                .collect(),
        )
    };
    if one_surface(a, p, b, q, resolution) {
        // The perturbation moves `A` off the surface both lie on, so
        // they don't meet: no loop, and no ends either.
        return if ends.is_empty() {
            Ok(PairDecision::Arcs(Vec::new()))
        } else {
            Err(BooleanError::Inconsistent)
        };
    }
    let certified = planar
        || cones[0][p as usize].apart(&cones[1][q as usize])
        || plane_and_cylinder(a, p, b, q, cones)
        || (ends.is_empty() && apart(&pa.hull(), &pb.hull(), 0.0))
        || (ends.is_empty() && parallel_walls(a, p, b, q, resolution));
    if certified {
        match ends {
            [] => return Ok(PairDecision::Arcs(Vec::new())),
            [x, y] => {
                if x.sign + y.sign != 0 {
                    return Err(BooleanError::Inconsistent);
                }
                return Ok(arcs(vec![(*x, *y)]));
            }
            _ if planar => {
                if let Some(joined) = along_line(a, b, p, q, ends) {
                    return Ok(arcs(joined));
                }
            }
            _ => {}
        }
    }
    if !ends.is_empty()
        && let Some(d) = parallel_generators(a, p, b, q, resolution)
        && let Some(joined) = along_generators(a, p, b, q, ends, d, resolution)
    {
        return Ok(arcs(joined));
    }
    let size = |b: crate::patch::Bounds3| (b.max - b.min).max_element();
    let (split_a, split_b) = (size(pa.bounds()) > floor, size(pb.bounds()) > floor);
    if split_a || split_b {
        return Ok(PairDecision::Split {
            a: split_a,
            b: split_b,
        });
    }
    // At the floor: no certificate means no loop, and the ends join in
    // order round the pair.
    Ok(arcs(round_order(pa.p, ends)?))
}

/// Whether triangle `p` of `A` and `q` of `B` lie on one curved surface:
/// their faces claim quadrics, and points sampled on each patch lie on
/// the other's within `resolution`. Such patches are coincident, as a
/// pin in a hole cut by the same circle, or stacked cylinders of one
/// radius: `A`'s perturbation takes it off the surface to one side
/// whole, and they don't meet. (Planar patches are certified as such.)
fn one_surface(a: &Input, p: u32, b: &Input, q: u32, resolution: f64) -> bool {
    let surface = |input: &Input, t: u32| input.mesh.faces()[input.face(t) as usize].surface;
    let (sa, sb) = (surface(a, p), surface(b, q));
    if !(matches!(sa, Surface::Quadric(_)) && matches!(sb, Surface::Quadric(_))) {
        return false;
    }
    let on = |patch: &crate::patch::Patch, other: &Surface| {
        samples().all(|u| other.distance(patch.eval(u)) <= resolution)
    };
    on(&a.patches[p as usize], &sb) && on(&b.patches[q as usize], &sa)
}

/// Whether one of triangle `p` of `A` and `q` of `B` is planar and the
/// other lies on a cylinder (its face's quadric, the same all along a
/// direction) with its normals within a half-space: then they meet in no
/// closed loop. A plane cuts a cylinder in lines along it or in a conic
/// round it, whose normals turn right round; a patch whose normals don't
/// holds no such conic, and lines run out of it. So flush and tangent
/// planes on walls (a box's side along a round boss) are certified as
/// planes meeting planes are, where the normal cones meet.
fn plane_and_cylinder(a: &Input, p: u32, b: &Input, q: u32, cones: [&[NormalCone]; 2]) -> bool {
    let cylinder = |input: &Input, t: u32, cone: &NormalCone| {
        let Surface::Quadric(quadric) = input.mesh.faces()[input.face(t) as usize].surface else {
            return false;
        };
        cone.cos > 0.0 && along(&quadric).is_some()
    };
    (a.planar[p as usize] && cylinder(b, q, &cones[1][q as usize]))
        || (b.planar[q as usize] && cylinder(a, p, &cones[0][p as usize]))
}

/// How near two walls along one direction must come in a pair, in
/// resolutions, for [`parallel_walls`] to certify it.
const PARALLEL_NEAR: f64 = 64.0;

/// Whether triangle `p` of `A` and `q` of `B` lie on walls along one
/// direction ([`parallel_generators`]) that come within [`PARALLEL_NEAR`]
/// resolutions of each other there: some point sampled on either patch
/// is that near the other's surface. With no ends such a pair has no cut.
///
/// Walls further apart are refined as any other pair, until their hulls
/// part, though they don't meet either: later stages rely on it. Where a
/// cap's flat ring lies between two such walls (a cylinder inside a
/// larger one sharing its top, cylinders of radii 1 and 1.001 stacked, a
/// coaxial one a few thousandths larger over part of the other's span),
/// the ring is cut along both walls' rims and triangulated from their
/// pieces: from unrefined walls, whose quarter arcs bulge across a ring
/// narrower than their sag, its triangles fail the hull rules against
/// the walls' (or, on a rim through a cap vertex in line with two of the
/// rim's, fold). Refined until their hulls part, the walls' pieces bulge
/// less than the ring is wide. Walls within the resolution of each other
/// (a tangency) never part: refined until their pieces were flat, a line
/// contact of a few millimetres ran out of the budget, the pairs along it
/// doubling every round. Between the two the certificate takes walls
/// that come nearer than 64 resolutions, which leaves a ring narrower
/// than that between them to fail, and stops a tangency's refinement once
/// a sample of its pieces comes that near the other wall: pieces about
/// 0.1 across for walls of radius 1 at the default tolerance, where
/// flat ones are about 0.003.
fn parallel_walls(a: &Input, p: u32, b: &Input, q: u32, resolution: f64) -> bool {
    if parallel_generators(a, p, b, q, resolution).is_none() {
        return false;
    }
    let within = PARALLEL_NEAR * resolution;
    let surface = |input: &Input, t: u32| input.mesh.faces()[input.face(t) as usize].surface;
    let near = |patch: &crate::patch::Patch, other: &Surface| {
        samples().any(|u| other.distance(patch.eval(u)) <= within)
    };
    near(&a.patches[p as usize], &surface(b, q)) || near(&b.patches[q as usize], &surface(a, p))
}

/// The direction triangle `p` of `A` and `q` of `B` both don't change
/// along, if their faces claim quadrics that are cylinders ([`along`]) with
/// directions parallel to within `resolution` over the pair's extent (its
/// boxes' diagonal): then two lines, one along each, drift apart by less
/// than the resolution across the pair. Such walls meet only in lines
/// along it (the cross-sections' common points, swept along it), and a
/// line runs out of both patches through their edges, where the counting
/// gives it ends: with none, they don't meet.
fn parallel_generators(a: &Input, p: u32, b: &Input, q: u32, resolution: f64) -> Option<DVec3> {
    let dir = |input: &Input, t: u32| match input.mesh.faces()[input.face(t) as usize].surface {
        Surface::Quadric(quadric) => along(&quadric),
        _ => None,
    };
    let (x, y) = (dir(a, p)?, dir(b, q)?);
    let bounds = a.patches[p as usize]
        .bounds()
        .union(b.patches[q as usize].bounds());
    let extent = (bounds.max - bounds.min).length();
    // A non-finite extent gives `None`.
    (x.cross(y).length() * extent <= resolution).then_some(x)
}

/// How thick, in resolutions, the sliver beside a line along walls'
/// common direction must be at least for [`along_generators`] to join
/// the line's ends: a quarter, 16 tie distances.
const LENS: f64 = 0.25;

/// The ends of triangle `p` of `A` and `q` of `B`, on walls along `d`
/// ([`parallel_generators`]), joined one arc per line along `d`, if
/// every line is clear of any other.
///
/// Such walls meet in lines along `d`, and each line's stretch inside
/// both patches runs between two ends, where it leaves one patch or the
/// other: at the same point of the cross-section (`at − d·(at·d)`), with
/// opposite signs, apart along `d`. So the ends are grouped by where they
/// are in the cross-section, and each group of exactly two is a line's
/// arc. Anything else (a group of one, three or four, as two lines a hair
/// apart or a line leaving and coming back give; two ends at one place
/// along `d`) gives `None`, and the pair is refined as before.
///
/// A line is clear where the walls cross at an angle `θ` with
/// `θ² ≥ 2·LENS·resolution·κ`, `κ` the sum of their cross-sections'
/// curvatures there (`sin θ` stands in for `θ`, which only asks more).
/// Two curves crossing so bound a sliver that closes no sooner than
/// `2θ/κ` away and is at least `θ²/2κ` thick: [`LENS`] resolutions. A
/// thinner one (walls overlapping by a fraction of the resolution, down
/// to crossings only the counting's ties make) is left to refinement,
/// whose later counts may drop its ends: joined in an early round, they
/// fold. For walls of one radius `R` side by side this joins lines at
/// least `√(R·resolution)` apart. Ends are grouped within `θ/κ` (the
/// smallest angle and the largest `κ` of the pair's ends): half the
/// distance to another line.
fn along_generators(
    a: &Input,
    p: u32,
    b: &Input,
    q: u32,
    ends: &[End],
    d: DVec3,
    resolution: f64,
) -> Option<Vec<(End, End)>> {
    let quadric = |input: &Input, t: u32| match input.mesh.faces()[input.face(t) as usize].surface {
        Surface::Quadric(quadric) => Some(quadric),
        _ => None,
    };
    let (qa, qb) = (quadric(a, p)?, quadric(b, q)?);
    if !ends.len().is_multiple_of(2) {
        return None;
    }
    // The curvature of a wall's cross-section at `x`: `tᵀ·H·t / |∇F|`,
    // `t` the unit tangent there square to `d`.
    let curvature = |quadric: &crate::mesh::Quadric, x: DVec3| {
        let gradient = quadric.gradient(x);
        let t = d.cross(gradient).try_normalize()?;
        let hessian = quadric.a + quadric.a.transpose();
        Some((t.dot(hessian * t).abs() / gradient.length(), gradient))
    };
    let (mut bend, mut least) = (0.0f64, f64::INFINITY);
    for e in ends {
        let (ka, ga) = curvature(&qa, e.at)?;
        let (kb, gb) = curvature(&qb, e.at)?;
        let kappa = ka + kb;
        let sine = ga.normalize().cross(gb.normalize()).length();
        // Not clear also where any of it isn't finite.
        let clear = (sine * sine).partial_cmp(&(2.0 * LENS * resolution * kappa));
        if !matches!(clear, Some(Ordering::Greater | Ordering::Equal)) {
            return None;
        }
        bend = bend.max(kappa);
        least = least.min(sine);
    }
    let group = least / bend;
    let across: Vec<DVec3> = ends.iter().map(|e| e.at - d * e.at.dot(d)).collect();
    // Grouped by single linkage: each end's group is the smallest index
    // linked to it.
    let n = ends.len();
    let mut label: Vec<usize> = (0..n).collect();
    for i in 0..n {
        for j in 0..i {
            if across[i].distance(across[j]) <= group {
                let (from, to) = (label[i].max(label[j]), label[i].min(label[j]));
                for l in &mut label {
                    if *l == from {
                        *l = to;
                    }
                }
            }
        }
    }
    let mut joined = Vec::with_capacity(n / 2);
    for i in 0..n {
        if label[i] != i {
            continue;
        }
        let members: Vec<usize> = (0..n).filter(|&k| label[k] == i).collect();
        let [x, y] = members[..] else {
            return None;
        };
        let (x, y) = (ends[x], ends[y]);
        let apart = (x.at - y.at).dot(d).abs().partial_cmp(&resolution);
        if x.sign + y.sign != 0 || apart != Some(Ordering::Greater) {
            return None;
        }
        joined.push((x, y));
    }
    Some(joined)
}

/// The direction a quadric doesn't change along, if it is a cylinder: a
/// null direction of its (symmetric) matrix along which its linear part
/// vanishes too, to rounding. For a matrix of rank 2 (circles, ellipses,
/// hyperbolas swept) that is its one null direction; for rank 1 (a
/// parabola swept: the matrix is `g·gᵀ`, its null directions a plane)
/// the one in that plane square to the linear part.
pub(super) fn along(q: &crate::mesh::Quadric) -> Option<DVec3> {
    let m = (q.a + q.a.transpose()) * 0.5;
    let rows = [m.row(0), m.row(1), m.row(2)];
    let size = rows.iter().map(|r| r.length()).fold(0.0, f64::max);
    if !(size > 0.0 && size.is_finite()) {
        return None;
    }
    let longest = |v: [DVec3; 3]| {
        v.into_iter()
            .max_by(|x, y| x.length_squared().total_cmp(&y.length_squared()))
    };
    let fits = |d: DVec3| {
        (m * d).length() <= 1e-9 * size && q.b.dot(d).abs() <= 1e-9 * (size + q.b.length())
    };
    // Rank 2: the rows' largest cross product.
    let crossed = longest([
        rows[0].cross(rows[1]),
        rows[1].cross(rows[2]),
        rows[2].cross(rows[0]),
    ])?;
    if let Some(d) = crossed.try_normalize().filter(|&d| fits(d)) {
        return Some(d);
    }
    // Rank 1: square to the largest row and to the linear part.
    let row = longest(rows)?;
    row.cross(q.b).try_normalize().filter(|&d| fits(d))
}

/// The ends of a pair of planar patches, on the line their planes meet
/// in, joined in order along it, if they alternate in sign there.
fn along_line(a: &Input, b: &Input, p: u32, q: u32, ends: &[End]) -> Option<Vec<(End, End)>> {
    let normal = |corners: [DVec3; 3]| (corners[1] - corners[0]).cross(corners[2] - corners[0]);
    let (na, nb) = (normal(a.corners(p)), normal(b.corners(q)));
    let d = na.cross(nb);
    // Planes too near parallel (or not finite) give no line.
    if d.length().partial_cmp(&(1e-9 * na.length() * nb.length())) != Some(Ordering::Greater) {
        return None;
    }
    // An odd number can't pair up (the counting balances every pair's
    // ends, so it doesn't happen; left to the rules that report it).
    if !ends.len().is_multiple_of(2) {
        return None;
    }
    let mut sorted = ends.to_vec();
    sorted.sort_by(|x, y| x.at.dot(d).total_cmp(&y.at.dot(d)).then(x.id.cmp(&y.id)));
    let joined: Vec<(End, End)> = sorted.chunks(2).map(|c| (c[0], c[1])).collect();
    joined
        .iter()
        .all(|(x, y)| x.sign + y.sign == 0)
        .then_some(joined)
}

/// The ends of a pair taken round it in order (by angle about their
/// middle, in the plane of `corners`), each going in joined to the next
/// coming out, as parentheses match: from the end after which the
/// running sum of signs is lowest, so every `−` finds a `+` before it.
fn round_order(corners: [DVec3; 3], ends: &[End]) -> Result<Vec<(End, End)>, BooleanError> {
    if ends.iter().map(|e| i32::from(e.sign)).sum::<i32>() != 0 {
        return Err(BooleanError::Inconsistent);
    }
    if ends.is_empty() {
        return Ok(Vec::new());
    }
    let x = (corners[1] - corners[0]).normalize_or_zero();
    let n = x.cross(corners[2] - corners[0]);
    let y = n.cross(x).normalize_or_zero();
    let flat = |p: DVec3| DVec2::new((p - corners[0]).dot(x), (p - corners[0]).dot(y));
    let middle = ends.iter().map(|e| flat(e.at)).sum::<DVec2>() / ends.len() as f64;
    let mut sorted: Vec<(f64, End)> = ends
        .iter()
        .map(|e| (pseudo_angle(flat(e.at) - middle), *e))
        .collect();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
    // Start after the lowest running sum.
    let (mut sum, mut low, mut start) = (0i32, 0i32, 0usize);
    for (k, (_, e)) in sorted.iter().enumerate() {
        sum += i32::from(e.sign);
        if sum < low {
            (low, start) = (sum, k + 1);
        }
    }
    let n = sorted.len();
    let mut open: Vec<End> = Vec::new();
    let mut out = Vec::with_capacity(n / 2);
    for k in 0..n {
        let e = sorted[(start + k) % n].1;
        if e.sign > 0 {
            open.push(e);
        } else {
            let plus = open.pop().ok_or(BooleanError::Inconsistent)?;
            out.push((plus, e));
        }
    }
    Ok(out)
}

/// A number that grows with the angle of `d` from `+x` counter-clockwise,
/// in `[0, 4)`, with `+ − ÷` only (so the same bits everywhere).
fn pseudo_angle(d: DVec2) -> f64 {
    let r = d.x.abs() + d.y.abs();
    if r == 0.0 {
        return 0.0;
    }
    let c = d.x / r;
    if d.y >= 0.0 { 1.0 - c } else { 3.0 + c }
}

#[cfg(test)]
pub(crate) mod tests;
