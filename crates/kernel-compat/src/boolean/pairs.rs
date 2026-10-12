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
//! run out of the pair through ends), or cylinders and cones on one axis
//! (they meet in one curve round it, see `coaxial::walls`, whose ends
//! join in turn round the axis). Then no ends is no cut, and
//! two ends are one arc; the ends of two planar patches, on the line
//! their planes meet in, join in order along it. On walls along one
//! direction, ends join line by line where the walls cross clearly
//! (`along_generators`); in a union, where they don't but touch facing
//! opposite ways (solids touching along a line from either side), the
//! operation fails at once as [`BooleanError::NotManifold`]
//! (`pinched_line`).
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
use super::coaxial;
use super::count::{self, Counts};
use super::curved::{Curved, SeamRules};
use super::evidence::Gather;
use super::input::{Input, Side};
use crate::budget::Work;
use crate::mesh::{MIN_SPLIT, Mesh, Node, Refiner, Surface, apart, samples};
use crate::par::par_map;
use crate::patch::NormalCone;
use crate::{Failure, KernelError, MAX_PATCHES, MAX_REFINE_DEPTH, Tolerance};

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

/// The crossing that is new vertex `id` (`first` from [`first_ids`]): of
/// an edge of `A` (its index in `x12`) or of `B` (in `x21`).
pub(super) fn crossing_of(first: [u32; 2], id: u32) -> (Side, usize) {
    if id < first[1] {
        (Side::A, (id - first[0]) as usize)
    } else {
        (Side::B, (id - first[1]) as usize)
    }
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

/// Where the crossing that is vertex `id` (a new one: see [`first_ids`])
/// lies, along its edge.
fn crossing_at(a: &Input, b: &Input, counts: &Counts, id: u32) -> DVec3 {
    let (input, c) = match crossing_of(first_ids(a, b, counts), id) {
        (Side::A, k) => (a, &counts.x12[k]),
        (Side::B, k) => (b, &counts.x21[k]),
    };
    input.conic(c.edge).eval(c.t)
}

/// The pair of faces `pair` (a triangle of `A`, one of `B`) whose ends,
/// at `ends`, don't join up, as [`BooleanError::Inconsistent`]: both
/// patches, the faces they lie on, and the ends as points.
fn pair_failure(
    a: &Input,
    b: &Input,
    pair: [u32; 2],
    ends: impl Iterator<Item = DVec3>,
) -> Failure {
    let mut gather = Gather::new();
    gather.pair(a, b, pair);
    for at in ends {
        if gather.truncated() {
            break;
        }
        gather.point(at);
    }
    gather.failure(BooleanError::Inconsistent)
}

/// The arcs of flat operands: every pair's ends are two, one of each
/// sign, joined. A pair whose ends aren't fails as
/// [`BooleanError::Inconsistent`], with its patches and ends.
pub(super) fn flat(a: &Input, b: &Input, counts: &Counts) -> Result<Vec<Arc>, Failure> {
    let ends = ends(a, b, counts);
    let mut arcs = Vec::with_capacity(ends.len() / 2);
    for pair in ends.chunk_by(|x, y| x.0 == y.0) {
        let refused = || {
            let at = pair.iter().map(|&(_, id, _)| crossing_at(a, b, counts, id));
            pair_failure(a, b, pair[0].0, at)
        };
        let [(tris, u, su), (_, v, sv)] = pair else {
            return Err(refused());
        };
        if su + sv != 0 {
            return Err(refused());
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
/// The shortcuts a try at the decisions may take, each certifying pairs
/// that refinement would split, so the pieces beside their cuts stay as
/// large as they were: some results refinement gets right fail the hull,
/// neighbour or fold rules from them, and the operation is tried again
/// without (see `boolean_within`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Shortcuts {
    /// Ends joined along walls' common direction (`along_generators`).
    pub(super) along: bool,
    /// Cylinders and cones on one axis certified (`coaxial::walls`), and
    /// quadrics of revolution on one axis cut in their parallels (see
    /// `chain::parallel`) rather than traced.
    pub(super) coaxial: bool,
}

impl Shortcuts {
    /// Every shortcut.
    pub(super) const ALL: Shortcuts = Shortcuts {
        along: true,
        coaxial: true,
    };
    /// None: refinement as it always was.
    pub(super) const NONE: Shortcuts = Shortcuts {
        along: false,
        coaxial: false,
    };

    /// Whether any is taken.
    pub(super) fn any(self) -> bool {
        self.along || self.coaxial
    }
}

#[derive(Debug)]
pub(super) struct Refined {
    pub(super) a: Mesh,
    pub(super) b: Mesh,
    pub(super) tree: [Vec<Node>; 2],
    pub(super) leaf: [Vec<u32>; 2],
    pub(super) counts: Counts,
    pub(super) arcs: Vec<Arc>,
    /// The shortcuts some pair's decision took.
    pub(super) used: Shortcuts,
}

/// [`refined_with`], taking every shortcut, failing with the error alone.
#[cfg(test)]
pub(super) fn refined(
    a: &Mesh,
    b: &Mesh,
    grow: bool,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<Refined, KernelError> {
    let rules = SeamRules::new(true);
    refined_with(
        a,
        b,
        grow,
        Shortcuts::ALL,
        super::tie(tol),
        &rules,
        tol,
        work,
    )
    .map_err(|f| f.error)
}

/// Counts `a` against `b` (whose meshes pass `check`, one of them with
/// curved patches) and decides every pair of faces, refining both until
/// it can: see the [module](self) docs. `grow` is whether `A` grows (a
/// union) or shrinks, for ties; only the `shortcuts` given are taken
/// (else such pairs are split as any other), and near ties within `tie`
/// taken as ties (0: none, see [`Curved::new`]), first orders of
/// rounding told by `rules`. A
/// union failing as [`BooleanError::NotManifold`] from the decisions
/// comes with the pairs showing it, an `Inconsistent` with what doesn't
/// fit (see [`decide`] and the counting's [`count::count`]).
#[allow(clippy::too_many_arguments)]
pub(super) fn refined_with(
    a: &Mesh,
    b: &Mesh,
    grow: bool,
    shortcuts: Shortcuts,
    tie: f64,
    rules: &SeamRules,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<Refined, Failure> {
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
            let counts = counted(&ia, &ib, grow, tie, rules, tol, work)?;
            match decide(
                &ia,
                &ib,
                &counts,
                floor,
                tol.resolution(),
                shortcuts,
                grow,
                work,
            )? {
                Decision::Arcs(arcs, used) => Err((counts, arcs, used)),
                Decision::Split(split) => Ok(split),
            }
        };
        let split = match split {
            Ok(split) => split,
            Err((counts, arcs, used)) => {
                let [a, b] = meshes;
                let [ra, rb] = &refiners;
                return Ok(Refined {
                    a,
                    b,
                    tree: [ra.nodes().to_vec(), rb.nodes().to_vec()],
                    leaf: leaves,
                    counts,
                    arcs,
                    used,
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
                return Err(KernelError::TooComplex.into());
            }
            work.spend(pieces.len())?;
            leaves[k] = pieces.iter().map(|p| p.leaf).collect();
            meshes[k] = refiners[k].mesh(&pieces);
        }
    }
    Err(KernelError::TooComplex.into())
}

/// One round's counting of operands with curved patches: [`refined_with`]
/// counts so every round, and [`touches`](super::touches) once, so its
/// counts are its first round's, bit for bit.
pub(super) fn counted(
    a: &Input,
    b: &Input,
    grow: bool,
    tie: f64,
    rules: &SeamRules,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<Counts, Failure> {
    count::count(a, b, &Curved::new(a, b, grow, tie, rules, tol), tol, work)
}

/// What a round of decisions comes to: every pair's arcs (and the
/// shortcuts some took), or the triangles of each operand to split first.
enum Decision {
    Arcs(Vec<Arc>, Shortcuts),
    Split([Vec<u32>; 2]),
}

/// What one pair comes to: its arcs (with the shortcut that decided
/// them, if one did), or a split.
enum PairDecision {
    Arcs(Vec<Arc>),
    Joined(Vec<Arc>),
    Coaxial(Vec<Arc>),
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
/// docs. Pieces no larger than `floor` across aren't split; only the
/// `shortcuts` given are taken. `grow` is whether
/// `A` grows (a union): then walls touching along a line from either
/// side fail the operation as [`BooleanError::NotManifold`] (see
/// [`pinched_line`]), with the pairs that show it ([`pinch_failure`]).
/// The error is the first pair's, in order, that fails; an
/// `Inconsistent` comes with that pair's patches and ends.
#[allow(clippy::too_many_arguments)]
fn decide(
    a: &Input,
    b: &Input,
    counts: &Counts,
    floor: f64,
    resolution: f64,
    shortcuts: Shortcuts,
    grow: bool,
    work: &mut Work,
) -> Result<Decision, Failure> {
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
    let first = first_ids(a, b, counts);
    let place = |id: u32| match crossing_of(first, id) {
        (Side::A, k) => at12[k],
        (Side::B, k) => at21[k],
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
        pair_decision(
            a,
            b,
            *pair,
            ends,
            [&cones_a, &cones_b],
            floor,
            resolution,
            shortcuts,
            grow,
        )
    });
    if let Some((k, &Err(error))) = decided.iter().enumerate().find(|(_, d)| d.is_err()) {
        let (pair, ends) = &jobs[k];
        return Err(match error {
            BooleanError::Inconsistent => pair_failure(a, b, *pair, ends.iter().map(|e| e.at)),
            BooleanError::NotManifold => {
                let pinched = (jobs.iter().zip(&decided))
                    .filter(|(_, d)| matches!(d, Err(BooleanError::NotManifold)))
                    .map(|(job, _)| job.0);
                pinch_failure(a, b, pinched)
            }
            error => KernelError::Boolean(error).into(),
        });
    }
    let mut arcs = Vec::new();
    let mut used = Shortcuts::NONE;
    let mut split = [Vec::new(), Vec::new()];
    for (&(pair, _), d) in jobs.iter().zip(decided) {
        match d.map_err(KernelError::Boolean)? {
            PairDecision::Arcs(mut here) => arcs.append(&mut here),
            PairDecision::Joined(mut here) => {
                used.along = true;
                arcs.append(&mut here);
            }
            PairDecision::Coaxial(mut here) => {
                used.coaxial = true;
                arcs.append(&mut here);
            }
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
    Ok(Decision::Arcs(arcs, used))
}

/// Walls touching along a line ([`pinched_line`]), united, as
/// [`BooleanError::NotManifold`]: each pair of faces refused so, in
/// order, as its two patches (`A`'s, then `B`'s: pieces of the
/// operands' triangles, as refined), and the names of the operands'
/// faces they lie on, each once; whole pairs, up to the allowance and
/// the caps.
fn pinch_failure(a: &Input, b: &Input, pairs: impl Iterator<Item = [u32; 2]>) -> Failure {
    let mut gather = Gather::new();
    for pair in pairs {
        if gather.truncated() {
            break;
        }
        gather.pair(a, b, pair);
    }
    gather.failure(BooleanError::NotManifold)
}

/// Decides the pair of triangle `p` of `A` and `q` of `B` with `ends`.
#[allow(clippy::too_many_arguments)]
fn pair_decision(
    a: &Input,
    b: &Input,
    [p, q]: [u32; 2],
    ends: &[End],
    cones: [&[NormalCone]; 2],
    floor: f64,
    resolution: f64,
    shortcuts: Shortcuts,
    grow: bool,
) -> Result<PairDecision, BooleanError> {
    let (pa, pb) = (&a.patches[p as usize], &b.patches[q as usize]);
    let planar = a.planar[p as usize] && b.planar[q as usize];
    let joined = |pairs: Vec<(End, End)>| -> Vec<Arc> {
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
            .collect()
    };
    let arcs = |pairs: Vec<(End, End)>| PairDecision::Arcs(joined(pairs));
    if one_surface(a, p, b, q, resolution) {
        // The perturbation moves `A` off the surface both lie on, so
        // they don't meet: no loop, and no ends either.
        return if ends.is_empty() {
            Ok(PairDecision::Arcs(Vec::new()))
        } else {
            Err(BooleanError::Inconsistent)
        };
    }
    let others = planar
        || cones[0][p as usize].apart(&cones[1][q as usize])
        || plane_and_cylinder(a, p, b, q, cones)
        || (ends.is_empty() && apart(&pa.hull(), &pb.hull(), 0.0))
        || (ends.is_empty() && parallel_walls(a, p, b, q, resolution));
    // Cylinders and cones on one axis, a shortcut: certified rather than
    // refined, their pieces stay as large as they were (a cut circle a
    // hair from another, a ring's corner a hundredth off a cone's wall
    // fail the hull rules then). A pair it alone decides says so, and a
    // failed result is tried again without it.
    let coaxial = shortcuts
        .coaxial
        .then(|| coaxial::walls([&a.form(p), &b.form(q)], [pa, pb], resolution))
        .flatten();
    let certified = others || coaxial.is_some();
    // What the pair's arcs come to, marked where only the coaxial
    // certificate decided them.
    let decided = |arcs: Vec<Arc>| {
        if coaxial.is_some() && !others {
            PairDecision::Coaxial(arcs)
        } else {
            PairDecision::Arcs(arcs)
        }
    };
    if certified {
        match ends {
            [] => return Ok(decided(Vec::new())),
            [x, y] => {
                if x.sign + y.sign != 0 {
                    return Err(BooleanError::Inconsistent);
                }
                return Ok(decided(joined(vec![(*x, *y)])));
            }
            _ if planar => {
                if let Some(joined) = along_line(a, b, p, q, ends) {
                    return Ok(arcs(joined));
                }
            }
            _ => {
                let at: Vec<(DVec3, i8)> = ends.iter().map(|e| (e.at, e.sign)).collect();
                // Joined round the axis: by the coaxial certificate alone.
                if let Some(pairs) = coaxial.and_then(|walls| coaxial::along(&walls, &at)) {
                    return Ok(PairDecision::Coaxial(joined(
                        pairs.into_iter().map(|(x, y)| (ends[x], ends[y])).collect(),
                    )));
                }
            }
        }
    }
    if shortcuts.along
        && !ends.is_empty()
        && let Some(d) = parallel_generators(a, p, b, q, resolution)
    {
        if let Some(lines) = along_generators(a, p, b, q, ends, d, resolution) {
            return Ok(PairDecision::Joined(joined(lines)));
        }
        if grow && pinched_line(a, p, b, q, ends, d, resolution) {
            return Err(BooleanError::NotManifold);
        }
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
/// curvatures' sizes there, however they bend (more than the gap between
/// them bends, which only asks more, as `sin θ` standing in for `θ`
/// does).
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
    let (qa, qb) = (quadric(a, p)?, quadric(b, q)?);
    if !ends.len().is_multiple_of(2) {
        return None;
    }
    let (mut bend, mut least) = (0.0f64, f64::INFINITY);
    for e in ends {
        let (ka, ga) = section(&qa, e.at, d)?;
        let (kb, gb) = section(&qb, e.at, d)?;
        let kappa = ka.abs() + kb.abs();
        let sine = ga.normalize().cross(gb.normalize()).length();
        // Not clear also where any of it isn't finite.
        if !clear(sine, kappa, resolution) {
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

/// The quadric face `t` of `input` claims, if it claims one.
fn quadric(input: &Input, t: u32) -> Option<crate::mesh::Quadric> {
    match input.mesh.faces()[input.face(t) as usize].surface {
        Surface::Quadric(quadric) => Some(quadric),
        _ => None,
    }
}

/// The curvature of a wall's cross-section square to `d` at `x`,
/// `tᵀ·H·t / |∇F|` with `t` the unit tangent there square to `d`
/// (positive where the wall bends away from the way its gradient points,
/// as a cylinder's from its outward normal), and the quadric's gradient
/// there.
fn section(quadric: &crate::mesh::Quadric, x: DVec3, d: DVec3) -> Option<(f64, DVec3)> {
    let gradient = quadric.gradient(x);
    let t = d.cross(gradient).try_normalize()?;
    let hessian = quadric.a + quadric.a.transpose();
    Some((t.dot(hessian * t) / gradient.length(), gradient))
}

/// Whether walls crossing at an angle of sine `sine`, their
/// cross-sections' curvatures' sizes adding up to `kappa`, bound a sliver
/// at least [`LENS`] resolutions thick (see [`along_generators`]); not
/// where any of it isn't finite.
fn clear(sine: f64, kappa: f64, resolution: f64) -> bool {
    let clear = (sine * sine).partial_cmp(&(2.0 * LENS * resolution * kappa));
    matches!(clear, Some(Ordering::Greater | Ordering::Equal))
}

/// How deep, in resolutions, a slit between walls bending into each other
/// must be for [`pinched_line`] to take their ends: four tie distances.
const SLIT: f64 = 1.0 / 16.0;

/// Whether triangle `p` of `A` and `q` of `B`, on walls along `d`
/// ([`parallel_generators`]), touch along a line from either side, for a
/// union: it has ends, and at every one the walls face opposite ways
/// (each wall's normal there, the quadric's gradient, turned the way its
/// patch faces at its middle) and aren't clear of each other (see
/// [`along_generators`]: tangent, or crossing at so small an angle `θ`
/// that `θ² < 2·LENS·resolution·κ`, `κ` the sizes of their
/// cross-sections' curvatures added); and where they bend into each
/// other (those curvatures, each signed by the way its wall faces, add
/// up to less than zero), the slit they leave is at least [`SLIT`]
/// resolutions deep (`θ²/2|κ'|`, `κ'` that sum).
///
/// Walls tangent within the tie distance, `A` grown by the perturbation,
/// cross in two lines infinitely close, whose ends (two or four to a
/// pair, as the patches' edges fall) no join takes, and refinement split
/// the pairs along the line until the budget ran out (seconds). The exact
/// union of solids touching along a line from either side, bending away
/// from each other there, isn't a manifold. Walls crossing at such an
/// angle unite into a crease, or leave a slit, whose two sides stay
/// within the resolution of each other for `resolution/θ` beside it,
/// more than `√(2·resolution/κ)`: half the width of a piece bent by `κ`
/// that is flat to the resolution (for solids side by side, an overlap
/// under [`LENS`] resolutions). Either way the result touches itself at
/// the kernel's resolution, so the union fails as such at once. Others
/// are refined as before: walls facing the same way (one solid inside
/// the other, touching its skin from inside: the outer one there, a
/// manifold); and walls bending into each other with no slit, a pin
/// against the wall of a smaller hole tangent to it inside (it plugs the
/// hole: a manifold, but the counting's ties give the pairs along the
/// line ends where the walls come within a tie of each other).
fn pinched_line(
    a: &Input,
    p: u32,
    b: &Input,
    q: u32,
    ends: &[End],
    d: DVec3,
    resolution: f64,
) -> bool {
    let (Some(qa), Some(qb)) = (quadric(a, p), quadric(b, q)) else {
        return false;
    };
    if ends.is_empty() {
        return false;
    }
    let middle = DVec3::splat(1.0 / 3.0);
    let (na, nb) = (
        a.patches[p as usize].normal(middle),
        b.patches[q as usize].normal(middle),
    );
    // 1 where the gradient `g` points the way `n` faces, −1 where it
    // points against it; `None` where the two are within 60° of square
    // (a patch so bent that its middle tells nothing), or not finite.
    let facing = |g: DVec3, n: DVec3| {
        let along = g.dot(n);
        let half = 0.5 * g.length() * n.length();
        if along > half {
            Some(1.0)
        } else if -along > half {
            Some(-1.0)
        } else {
            None
        }
    };
    ends.iter().all(|e| {
        let (Some((ka, ga)), Some((kb, gb))) = (section(&qa, e.at, d), section(&qb, e.at, d))
        else {
            return false;
        };
        let (Some(sa), Some(sb)) = (facing(ga, na), facing(gb, nb)) else {
            return false;
        };
        let (ua, ub) = ((ga * sa).normalize(), (gb * sb).normalize());
        let sine = ua.cross(ub).length();
        // Each wall bends away from its outward normal by its signed
        // curvature; facing opposite ways, the gap between them opens by
        // their sum.
        let gap = sa * ka + sb * kb;
        // Bending into each other, they overlap all round the line but
        // for a slit, which only a crossing that far from tangent shows:
        // the counting's ties give ends where walls overlapping all round
        // come within a tie of each other.
        let slit = gap > 0.0 || sine * sine > 2.0 * SLIT * resolution * -gap;
        let kappa = ka.abs() + kb.abs();
        let finite = sine.is_finite() && kappa.is_finite();
        ua.dot(ub) < 0.0 && slit && finite && !clear(sine, kappa, resolution)
    })
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
