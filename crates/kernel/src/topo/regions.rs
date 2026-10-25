//! A solid's regions, derived from its faces, edges and contact records:
//! the **side graph** and its components, the side-shells; each shell's
//! kind by its signed volume; the regions the shells bound, with their
//! islands placed and the lumps' containers. The constructions fill a
//! body's shells and regions from it ([`derive()`]); the check's invariant
//! 8 rebuilds them and compares.
//!
//! **The side graph.** Every face has two sides, `Out` towards its
//! normal and `In` in the material. Across an ordinary edge (two
//! coedges, positive length, in no contact record) the faces' `Out` sides
//! are glued, and their `In` sides; a pole's side (one coedge) and a
//! zero-length edge (its curve a point) glue nothing. Each glue is
//! **balanced**: the two coedges run the edge opposite ways, so along
//! the edge the glued sides' incidences cancel. A contact record's sides
//! are glued by its orientation class, which the store doesn't hold yet:
//! a body with a contact record has no side graph here
//! ([`RegionError::Contact`]). A component of the graph is a side-shell;
//! its sides are listed in face order, `Out` before `In`, and the shells
//! in order of their first side.
//!
//! **Kinds.** A shell's signed volume (its sides oriented: an `Out` side
//! as its face, an `In` side reversed; [`signed_volume`]) is negative on
//! the **outer** shell of the region it faces, positive on an **island**.
//! A shell's sides all face one way: `In` sides bound material, `Out`
//! sides empty space. A sign the volume's bounds don't certify is the
//! perturbed ray's to decide, not built ([`RegionError::Unsigned`]).
//!
//! **Regions.** Each outer shell bounds one region: a lump (its sides
//! `In`) or a void (`Out`). The unbounded region holds the empty islands
//! no void encloses. An island lies in the region of the innermost outer
//! shell of its kind enclosing it ([`place`]): only certified box
//! exclusions are built so far, so an island is placed where they leave
//! one choice (a material island one lump whose box holds it; an empty
//! island no void whose box holds it, so the unbounded region), and is
//! otherwise left to the nesting query ([`RegionError::Unplaced`]). A
//! material island no lump's box holds is enclosed by none: a defect
//! ([`RegionError::Unenclosed`]). A lump's **container** is the region
//! of its empty islands (the shells of the `Out` sides of its outer
//! shell's faces): a void, or none for the unbounded region; a void's is
//! none.
//!
//! **Order.** The unbounded region first (where any shell lies in it),
//! then a region per outer shell in the shells' order. A region is named
//! by the least key of its shells' faces ([`Brep::region_key`]).
//!
//! **Work.** The side graph one unit per edge and per side, in one item;
//! each shell's volume as the measure charges its faces' fluxes, one unit
//! more per side; each island's placement one unit, and each box it
//! reads as the measure's box search charges it.

use varde_decide::num::bounded::Bounded;

use super::brep::{Brep, EdgeId, FaceId, FaceKey, RegionId, RegionKind, ShellId, ShellKind, Side};
use crate::infra::budget::{Halt, Ledger, Pass, Step, Stop, Work};
use crate::measure::bounds::{BoundsError, faces_box};
use crate::measure::mass::{Integrand, face_integrals};

#[cfg(test)]
pub(crate) mod tests;

/// Why a body's regions can't be derived.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionError {
    /// The body has a contact record: its sides are glued by its class,
    /// not built yet.
    Contact,
    /// Edge `.0`'s two coedges run it the same way: the glue across it
    /// isn't balanced.
    Balance(EdgeId),
    /// A shell's signed volume isn't certified of a sign: the perturbed
    /// ray's to decide, not built yet.
    Unsigned,
    /// An island the box exclusions leave more than one region for: the
    /// nesting query's to place, not built yet.
    Unplaced,
    /// A material island no lump encloses.
    Unenclosed,
    /// A face can't be measured or bounded: its trims don't close or
    /// leave its surface's tiles (the check's earlier invariants).
    Broken,
}

/// A side as a node of the side graph.
fn node(f: FaceId, side: Side) -> usize {
    f.index() * 2 + side.index()
}

/// The roots of a union of nodes, the least node of each set its root,
/// so the sets don't depend on the order of the joins.
struct Union(Vec<usize>);

impl Union {
    fn root(&mut self, mut a: usize) -> usize {
        while self.0[a] != a {
            self.0[a] = self.0[self.0[a]];
            a = self.0[a];
        }
        a
    }

    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.root(a), self.root(b));
        if a < b {
            self.0[b] = a;
        } else {
            self.0[a] = b;
        }
    }
}

/// Whether edge `e`'s curve is a point: all its control points one, bit
/// for bit.
fn is_point(b: &Brep, e: EdgeId) -> bool {
    let mut points = b.edge_spans(e).iter().flat_map(|s| s.iter());
    let Some(first) = points.next() else {
        return true;
    };
    points.all(|p| (0..3).all(|k| p[k].to_bits() == first[k].to_bits()))
}

/// The side graph's components, the side-shells (module docs), on
/// `ledger`. A coedge, face or edge id naming nothing is passed over (the
/// check's invariant 1 refuses it).
pub(crate) fn side_graph(
    b: &Brep,
    ledger: &mut Ledger<'_>,
) -> Result<Vec<Vec<(FaceId, Side)>>, Stop<RegionError>> {
    if !b.contacts().is_empty() {
        return Err(Stop::Failed(RegionError::Contact));
    }
    let nf = b.face_count();
    let nc = b.coedge_count();
    let mut union = Union((0..2 * nf).collect());
    for e in 0..b.edge_count() {
        ledger.charge(Step::Work, 1)?;
        let e = EdgeId(e as u32);
        let c = b.edge_coedge(e);
        if c.index() >= nc {
            continue;
        }
        let p = b.pair(c);
        if p.index() >= nc || b.curve_count() <= b.edge_curve_id(e).index() || is_point(b, e) {
            continue;
        }
        let (f, g) = (b.face(c), b.face(p));
        if f.index() >= nf || g.index() >= nf {
            continue;
        }
        if b.reversed(c) == b.reversed(p) {
            return Err(Stop::Failed(RegionError::Balance(e)));
        }
        for side in [Side::Out, Side::In] {
            union.join(node(f, side), node(g, side));
        }
    }
    ledger.charge(Step::Work, u64::try_from(2 * nf).unwrap_or(u64::MAX))?;
    let mut shells: Vec<Vec<(FaceId, Side)>> = Vec::new();
    // Per root, its shell's place, filled in node order: the shells come
    // in order of their first side.
    let mut at = vec![usize::MAX; 2 * nf];
    for f in 0..nf {
        let f = FaceId(f as u32);
        for side in [Side::Out, Side::In] {
            let r = union.root(node(f, side));
            if at[r] == usize::MAX {
                at[r] = shells.len();
                shells.push(Vec::new());
            }
            shells[at[r]].push((f, side));
        }
    }
    Ok(shells)
}

/// Per face, the edges of its coedges, sorted, each once.
pub(crate) fn face_edges(b: &Brep) -> Vec<Vec<EdgeId>> {
    (0..b.face_count())
        .map(|f| {
            let mut es: Vec<EdgeId> = b
                .loops(FaceId(f as u32))
                .flat_map(|l| b.loop_coedges(l))
                .map(|c| b.edge(c))
                .collect();
            es.sort_unstable();
            es.dedup();
            es
        })
        .collect()
}

/// Three times the signed volume of the shell of `sides` (module docs),
/// certified, the faces summed in order of their keys (then their sides
/// and ids, breaking exact ties), so the sum's rounding doesn't depend on
/// the order the shell lists them in; `None` where a face can't be
/// measured (a broken store). The origin is the centre of the box of the
/// control points of the faces' edges (`face_edges[f]` those of face
/// `f`): the flux doesn't depend on it, its bounds do. One unit per side,
/// each face's flux as the measure charges it.
pub(crate) fn signed_volume<E>(
    b: &Brep,
    sides: &[(FaceId, Side)],
    face_edges: &[Vec<EdgeId>],
    ledger: &mut Ledger<'_>,
) -> Result<Option<Bounded<f64>>, Stop<E>> {
    ledger.charge(Step::Work, u64::try_from(sides.len()).unwrap_or(u64::MAX))?;
    let mut sides: Vec<(FaceId, Side)> = sides
        .iter()
        .copied()
        .filter(|(f, _)| f.index() < b.face_count())
        .collect();
    sides.sort_unstable_by(|&(f, a), &(g, c)| (b.key(f), a, f).cmp(&(b.key(g), c, g)));
    let o = centre(b, &sides, face_edges);
    let mut sum = Bounded::exact(0.0);
    for &(f, side) in &sides {
        let flux = match face_integrals(b, f, o, &[Integrand::Volume], ledger) {
            Ok(v) => v[0],
            Err(Stop::Failed(_)) => return Ok(None),
            Err(Stop::Exhausted(x)) => return Err(Stop::Exhausted(x)),
            Err(Stop::Cap(cap)) => return Err(Stop::Cap(cap)),
        };
        sum = match side {
            Side::Out => sum + flux,
            Side::In => sum + -flux,
        };
    }
    Ok(Some(sum))
}

/// The centre of the box of the control points of the faces' edges; the
/// world origin where they have none.
fn centre(b: &Brep, sides: &[(FaceId, Side)], face_edges: &[Vec<EdgeId>]) -> [f64; 3] {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for &(f, _) in sides {
        for &e in face_edges.get(f.index()).map_or(&[][..], Vec::as_slice) {
            if e.index() >= b.edge_count() {
                continue;
            }
            for p in b.edge_spans(e).iter().flat_map(|span| span.iter()) {
                for k in 0..3 {
                    // Compare and select: no `min`/`max`, a NaN left out.
                    if p[k] < lo[k] {
                        lo[k] = p[k];
                    }
                    if p[k] > hi[k] {
                        hi[k] = p[k];
                    }
                }
            }
        }
    }
    std::array::from_fn(|k| {
        if lo[k] <= hi[k] {
            lo[k] * 0.5 + hi[k] * 0.5
        } else {
            0.0
        }
    })
}

/// Where an island lies (module docs, [`place`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    /// In the region of candidate `.0`.
    In(usize),
    /// In the unbounded region (an empty island only).
    Unbounded,
    /// More than one region left: the nesting query's to decide.
    Unsettled,
    /// A material island no candidate's box holds: enclosed by none.
    Unenclosed,
}

/// The vertices of the faces of `sides` (`face_edges[f]` the edges of
/// face `f`): points the shell certainly passes through.
fn shell_points(b: &Brep, sides: &[(FaceId, Side)], face_edges: &[Vec<EdgeId>]) -> Vec<[f64; 3]> {
    let mut vs: Vec<_> = sides
        .iter()
        .filter_map(|(f, _)| face_edges.get(f.index()))
        .flatten()
        .filter(|e| e.index() < b.edge_count())
        .flat_map(|&e| b.ends(e))
        .filter(|v| v.index() < b.vert_count())
        .collect();
    vs.sort_unstable();
    vs.dedup();
    vs.into_iter().map(|v| b.pos(v)).collect()
}

/// A certified box holding the faces of `sides`, refined to a 64th of
/// the extent of their vertices.
fn shell_box(
    b: &Brep,
    sides: &[(FaceId, Side)],
    face_edges: &[Vec<EdgeId>],
    ledger: &mut Ledger<'_>,
) -> Result<[[f64; 3]; 2], Stop<BoundsError>> {
    let points = shell_points(b, sides, face_edges);
    let mut extent = 0.0f64;
    for k in 0..3 {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in &points {
            if p[k] < lo {
                lo = p[k];
            }
            if p[k] > hi {
                hi = p[k];
            }
        }
        let d = hi - lo;
        if d > extent {
            extent = d;
        }
    }
    let res = if extent > 0.0 { extent / 64.0 } else { 1.0 };
    let faces: Vec<FaceId> = sides.iter().map(|&(f, _)| f).collect();
    faces_box(b, &faces, face_edges, res, ledger)
}

/// Whether every point lies in the closed box `[lo, hi]`.
fn holds(bx: &[[f64; 3]; 2], points: &[[f64; 3]]) -> bool {
    points
        .iter()
        .all(|p| (0..3).all(|k| bx[0][k] <= p[k] && p[k] <= bx[1][k]))
}

/// Whether the box of the faces of `outer` certainly leaves out a point
/// of the island `island`: then `outer` doesn't enclose it.
pub(crate) fn excludes(
    b: &Brep,
    outer: &[(FaceId, Side)],
    island: &[(FaceId, Side)],
    face_edges: &[Vec<EdgeId>],
    ledger: &mut Ledger<'_>,
) -> Result<bool, Stop<BoundsError>> {
    let points = shell_points(b, island, face_edges);
    let bx = shell_box(b, outer, face_edges, ledger)?;
    Ok(!holds(&bx, &points))
}

/// Where the island of sides `island` lies among the outer shells of its
/// kind `candidates` (module docs): the one whose box holds it, if one
/// (for a material island) or none (an empty island: the unbounded
/// region). One unit, and each candidate's box.
pub(crate) fn place(
    b: &Brep,
    island: &[(FaceId, Side)],
    material: bool,
    candidates: &[&[(FaceId, Side)]],
    face_edges: &[Vec<EdgeId>],
    ledger: &mut Ledger<'_>,
) -> Result<Place, Stop<BoundsError>> {
    ledger.charge(Step::Work, 1)?;
    let mut held = Vec::new();
    for (k, outer) in candidates.iter().enumerate() {
        if !excludes(b, outer, island, face_edges, ledger)? {
            held.push(k);
        }
    }
    Ok(match (material, held.as_slice()) {
        (true, []) => Place::Unenclosed,
        (true, &[k]) => Place::In(k),
        (false, []) => Place::Unbounded,
        _ => Place::Unsettled,
    })
}

/// A derived region (module docs): its kind, its outer shell (`None`
/// for the unbounded region), its islands and its container, by their
/// places in [`Derived`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DerivedRegion {
    pub(crate) kind: RegionKind,
    pub(crate) outer: Option<usize>,
    pub(crate) islands: Vec<usize>,
    pub(crate) container: Option<usize>,
}

/// A solid's shells and regions as derived (module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Derived {
    /// The side-shells and their kinds.
    pub(crate) shells: Vec<(Vec<(FaceId, Side)>, ShellKind)>,
    /// The regions.
    pub(crate) regions: Vec<DerivedRegion>,
}

impl From<BoundsError> for RegionError {
    fn from(_: BoundsError) -> Self {
        RegionError::Broken
    }
}

fn bounds_stop(s: Stop<BoundsError>) -> Stop<RegionError> {
    match s {
        Stop::Exhausted(x) => Stop::Exhausted(x),
        Stop::Cap(c) => Stop::Cap(c),
        Stop::Failed(e) => Stop::Failed(e.into()),
    }
}

/// Whether the shell of `sides` bounds material: its sides `In`.
fn is_material(sides: &[(FaceId, Side)]) -> bool {
    sides.first().is_some_and(|&(_, s)| s == Side::In)
}

/// The shells and regions of solid `b` (module docs), in passes of
/// `work`: the side graph, the shells' volumes, the islands' places.
pub(crate) fn derive(b: &Brep, work: &mut Work) -> Result<Derived, Halt<RegionError>> {
    let pass = |stage| Pass { stage, rung: 0 };
    let graph = work
        .pass(pass("regions graph"), &[()], |(), ledger| {
            side_graph(b, ledger)
        })?
        .pop()
        .unwrap_or_default();
    let edges = face_edges(b);
    let kinds = work.pass(
        pass("regions volume"),
        &graph,
        |sides, ledger| match signed_volume(b, sides, &edges, ledger)? {
            None => Err(Stop::Failed(RegionError::Broken)),
            Some(v) if v.v.abs() > v.err() => Ok(if v.v < 0.0 {
                ShellKind::Outer
            } else {
                ShellKind::Island
            }),
            Some(_) => Err(Stop::Failed(RegionError::Unsigned)),
        },
    )?;
    let outers: Vec<usize> = (0..graph.len())
        .filter(|&s| kinds[s] == ShellKind::Outer)
        .collect();
    let islands: Vec<usize> = (0..graph.len())
        .filter(|&s| kinds[s] == ShellKind::Island)
        .collect();
    let places = work.pass(pass("regions place"), &islands, |&s, ledger| {
        let material = is_material(&graph[s]);
        let cands: Vec<usize> = outers
            .iter()
            .copied()
            .filter(|&o| is_material(&graph[o]) == material)
            .collect();
        let sides: Vec<&[(FaceId, Side)]> = cands.iter().map(|&o| graph[o].as_slice()).collect();
        match place(b, &graph[s], material, &sides, &edges, ledger).map_err(bounds_stop)? {
            Place::In(k) => Ok(Some(cands[k])),
            Place::Unbounded => Ok(None),
            Place::Unsettled => Err(Stop::Failed(RegionError::Unplaced)),
            Place::Unenclosed => Err(Stop::Failed(RegionError::Unenclosed)),
        }
    })?;
    let unbounded = places.iter().any(Option::is_none);
    let mut regions = Vec::new();
    if unbounded {
        regions.push(DerivedRegion {
            kind: RegionKind::Empty,
            outer: None,
            islands: Vec::new(),
            container: None,
        });
    }
    // Per outer shell, its region's place.
    let mut region_of = vec![usize::MAX; graph.len()];
    for &o in &outers {
        region_of[o] = regions.len();
        regions.push(DerivedRegion {
            kind: if is_material(&graph[o]) {
                RegionKind::Material
            } else {
                RegionKind::Empty
            },
            outer: Some(o),
            islands: Vec::new(),
            container: None,
        });
    }
    let mut island_region = vec![usize::MAX; graph.len()];
    for (&s, place) in islands.iter().zip(&places) {
        let r = place.map_or(0, |o| region_of[o]);
        island_region[s] = r;
        regions[r].islands.push(s);
    }
    // A lump's container: the region of its faces' `Out` sides' shells.
    let mut shell_of = vec![usize::MAX; 2 * b.face_count()];
    for (s, sides) in graph.iter().enumerate() {
        for &(f, side) in sides {
            shell_of[node(f, side)] = s;
        }
    }
    for r in &mut regions {
        let (Some(o), RegionKind::Material) = (r.outer, r.kind) else {
            continue;
        };
        let mut around: Vec<usize> = graph[o]
            .iter()
            .map(|&(f, _)| island_region[shell_of[node(f, Side::Out)]])
            .collect();
        around.sort_unstable();
        around.dedup();
        match around.as_slice() {
            &[e] if e != usize::MAX => {
                r.container = regions_outer(e, unbounded);
            }
            _ => {
                return Err(Halt::Failed {
                    item: 0,
                    error: RegionError::Unenclosed,
                });
            }
        }
    }
    Ok(Derived {
        shells: graph.into_iter().zip(kinds).collect(),
        regions,
    })
}

/// The container of a lump whose empty islands lie in region `e`: none
/// for the unbounded region (the first, where there is one).
fn regions_outer(e: usize, unbounded: bool) -> Option<usize> {
    (!(unbounded && e == 0)).then_some(e)
}

impl Brep {
    /// The name of region `r`: the least key of its shells' faces; `None`
    /// for a region without faces.
    pub fn region_key(&self, r: RegionId) -> Option<&FaceKey> {
        let shells = std::iter::once(self.outer_shell(r))
            .filter(|s| !s.is_none())
            .chain(self.island_shells(r).iter().copied());
        shells
            .flat_map(|s: ShellId| self.shell_sides(s).iter().map(|&(f, _)| self.key(f)))
            .min()
    }
}
