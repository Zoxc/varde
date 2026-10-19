//! Winding numbers: each vertex's winding number `w(v)` in the other
//! operand, `1` inside it and `0` outside, from one seed per connected
//! component of the operand's edges, carried along every edge by its
//! crossings.
//!
//! **Seeds** ([`seeds`]). Per component, the first vertex in geometric
//! order ([`EntityOrder::vert_key`], ids only on exact ties) whose
//! certified 3D clearance from the other operand exceeds `ε_tie`; if none
//! has clearance, the first vertex, marked uncleared. The caller counts the seed's winding
//! `w(v0) = Σ_f s02(v0, f)` (`s20` for a vertex of `B`) over every face
//! of the other operand, from the same records and the same decisions as
//! the crossings ([`crate::count::s02`]), so one ray suffices: a tie at
//! the seed is decided once and read by both.
//!
//! **Clearance** ([`clear_of_pieces`]). A vertex is tested against the
//! other operand's faces near it, each from its whole tiles (the leaves a
//! broad phase returns): a tile's hull box clear of the vertex by more
//! than `ε_tie` (padded by the face's widest band) clears the whole tile;
//! one not clear refines to its pieces, the face's dyadic boxes
//! ([`crate::count::boxes`]). A box certified outside the trim is no part
//! of the face; a core box's hull, or a mixed box's padded by the bands of
//! the trims it meets (which its ribbons stay within), clear of the vertex
//! clears it; any other box splits. So a vertex near a tile but clear of
//! the face's surface is cleared. A box whose hull lies within `ε_tie` of
//! the vertex, or is itself within a quarter of `ε_tie` across, ends the
//! test uncleared, as does anything the level can't certify (a hull, a
//! classification, a split past the boxes' depth) and a test past
//! [`MAX_CLEAR_BOXES`] boxes: an uncertified test is no clearance, never
//! an error.
//!
//! **Propagation** ([`windings`]). Along an edge `e = a→b` the winding
//! changes by its crossings with every face of the other operand,
//! `w(b) − w(a) = Σ_f x12(e, f)` ([`crate::count::cross`]), an identity of
//! the records whenever the other operand is closed, so the change sums
//! to 0 round every closed walk and the windings don't depend on the path
//! that carries them. From each seed the windings are carried
//! breadth-first, each vertex's edges in geometric order
//! ([`EntityOrder::edge_key`]); then **every** edge is checked to agree,
//! not only those of the walk: a set of records that doesn't fit shows on
//! some edge as `w(b) − w(a) ≠ Σ_f x12(e, f)`. The operands are solids,
//! so a winding outside `{0, 1}` is records that don't fit too.
//!
//! **Signals.** A disagreeing edge ([`WindingError::Disagree`]) and a
//! winding outside `{0, 1}` ([`WindingError::Outside`]) are the
//! counting's ladder signals: the decisions don't fit, and the next rung
//! re-decides. Each is keyed by its entity, and of several failures of
//! one kind the one reported is the first in geometric order, so the key
//! doesn't depend on ids. The edges disagree first: a wrong crossing
//! makes some winding wrong, and the edge is nearer the cause. The
//! windings are carried in `i128`, where no sum of fewer than `2³²`
//! changes in `i64` overflows, so a winding however large is only
//! outside `{0, 1}`, and never hides a disagreement found later.
//!
//! **Lobes** ([`windings_lobed`]). Inside an edge's join zone the
//! counting surfaces of its two faces can overshoot each other by a seam
//! so that their union isn't embedded there (a **lobe**): the exact column
//! hits then give a point a winding of −1 or 2, which no closed solid
//! has, and no exact record set fits. On a winding outside `{0, 1}` at a
//! point `q` lying within the near radius of an edge `g` of the other
//! operand (its feet on `g` not empty; every edge is taken as without a
//! certificate that its join zone has no lobe), `q`'s hits near `g` on
//! `g`'s faces are re-read by the **edge rule** (the edge's key at `q`'s
//! foot, [`crate::records::height::edge_rule_key`]), only `q`'s records
//! change, and the windings are carried again. Each `(q, g)` falls back
//! once, the edges in the caller's geometric order; a winding the
//! fallbacks don't bring into `{0, 1}` stays the signal. The fallbacks
//! are counted ([`Lobes::count`]): the edge rule is a convention at the
//! scale of the seam, not a truth of the moved configuration.
//!
//! **Budget.** Each clearance test is given the share the search has
//! left and costs what it reports: [`clear_of_pieces`] charges each box
//! tested (a hull, a classification, a split, at the count's units per
//! piece, [`Charge`]) and stops once past its share. Past the search's
//! share the search stops, [`SeedError::OutOfBudget`] keyed by the vertex
//! being tested. Propagation and its checks are integer sums, one per
//! edge.

use std::collections::{BTreeSet, VecDeque};

use smallvec::SmallVec;

use crate::bezier::{HomPatch, HullBox};
use crate::count::boxes::{
    BoxKey, Class, FaceUv, SpanRef, Trim, box_hull, classify, classify_among, split, within_band,
    within_of,
};
use crate::count::s02::{Charge, Exhausted};
use crate::ids::{EdgeId, VertId};
use crate::num::bounded::{Bounded, add_up, down, up};
use crate::order::{self, EntityOrder};
use crate::records::keys::{Operand, Point};

/// An operand's vertices and edges, as the windings walk them.
#[derive(Clone, Copy, Debug)]
pub struct Graph<'a> {
    /// The operand.
    pub operand: Operand,
    /// The number of vertices: ids `0..verts`.
    pub verts: u32,
    /// Per edge, by id, its ends, stored start first. A closed edge's
    /// ends are one vertex.
    pub edges: &'a [[VertId; 2]],
}

/// Why the graph can't be walked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GraphError {
    /// An edge's end isn't one of the vertices.
    End(EdgeId),
    /// More edges than ids.
    SizeLimit,
}

impl Graph<'_> {
    /// The edges, checked: ids fit, ends are vertices.
    fn checked(&self) -> Result<(), GraphError> {
        if u32::try_from(self.edges.len()).map_or(true, |n| n == u32::MAX) {
            return Err(GraphError::SizeLimit);
        }
        for (e, ends) in (0u32..).zip(self.edges) {
            if ends.iter().any(|v| v.0 >= self.verts) {
                return Err(GraphError::End(EdgeId(e)));
            }
        }
        Ok(())
    }

    /// The vertices in geometric order, ids breaking exact ties.
    fn verts_in_order<O: EntityOrder>(&self, order: &O) -> Vec<VertId> {
        let mut keyed: Vec<_> = (0..self.verts)
            .map(|v| (order.vert_key(self.operand, VertId(v)), VertId(v)))
            .collect();
        order::sort(&mut keyed);
        keyed.into_iter().map(|(_, v)| v).collect()
    }

    /// The edges in geometric order, ids breaking exact ties.
    fn edges_in_order<O: EntityOrder>(&self, order: &O) -> Vec<EdgeId> {
        let mut keyed: Vec<_> = (0u32..)
            .zip(self.edges)
            .map(|(e, _)| (order.edge_key(self.operand, EdgeId(e)), EdgeId(e)))
            .collect();
        order::sort(&mut keyed);
        keyed.into_iter().map(|(_, e)| e).collect()
    }

    /// Per vertex its edges in the order of `edges` (a closed edge once).
    fn adjacency(&self, edges: &[EdgeId]) -> Vec<Vec<EdgeId>> {
        let mut out = vec![Vec::new(); self.verts as usize];
        for &e in edges {
            let [a, b] = self.edges[e.index()];
            out[a.index()].push(e);
            if b != a {
                out[b.index()].push(e);
            }
        }
        out
    }

    /// Each vertex's component, numbered by the first vertex of `verts`
    /// (the geometric order) in it.
    fn components(&self, verts: &[VertId]) -> Vec<u32> {
        let ids: Vec<EdgeId> = (0u32..).zip(self.edges).map(|(e, _)| EdgeId(e)).collect();
        let adj = self.adjacency(&ids);
        let mut comp = vec![u32::MAX; self.verts as usize];
        let mut n = 0u32;
        let mut queue = VecDeque::new();
        for &v in verts {
            if comp[v.index()] != u32::MAX {
                continue;
            }
            comp[v.index()] = n;
            queue.push_back(v);
            while let Some(u) = queue.pop_front() {
                for &e in &adj[u.index()] {
                    let o = self.other_end(e, u);
                    if comp[o.index()] == u32::MAX {
                        comp[o.index()] = n;
                        queue.push_back(o);
                    }
                }
            }
            n += 1;
        }
        comp
    }

    /// The end of `e` other than `v` (`v` itself on a closed edge).
    fn other_end(&self, e: EdgeId, v: VertId) -> VertId {
        let [a, b] = self.edges[e.index()];
        if a == v { b } else { a }
    }
}

/// A component's seed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Seed {
    /// The vertex.
    pub vert: VertId,
    /// Whether its clearance is certified (an audit ray may start only
    /// from a cleared seed).
    pub cleared: bool,
}

/// Why the seeds aren't found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SeedError<E> {
    /// The graph can't be walked.
    Graph(GraphError),
    /// A clearance test failed (a record missing, say), at this vertex.
    Clearance {
        /// The vertex tested.
        point: Point,
        /// The test's error.
        error: E,
    },
    /// The search ran past its share at this vertex's test.
    OutOfBudget {
        /// What ran out: [`Exhausted::Work`].
        cause: Exhausted,
        /// The vertex tested.
        point: Point,
    },
}

/// The seeds of `graph`'s components (module docs), one per component,
/// in the geometric order of their first vertices. `clear(v, left)`
/// certifies vertex `v`'s clearance from the other operand (`Ok(true)`
/// certified, `Ok(false)` not, [`clear_of_pieces`]) given the `left` units
/// of `share` not yet spent, with its cost in units, each charged against
/// `share`. With the seeds, or the error, the units spent.
pub fn seeds<O: EntityOrder, E>(
    order: &O,
    graph: &Graph<'_>,
    share: u64,
    mut clear: impl FnMut(VertId, u64) -> (Result<bool, E>, u64),
) -> (Result<Vec<Seed>, SeedError<E>>, u64) {
    let mut spent = 0u64;
    if let Err(e) = graph.checked() {
        return (Err(SeedError::Graph(e)), spent);
    }
    let verts = graph.verts_in_order(order);
    let comp = graph.components(&verts);
    // Per component its vertices in geometric order; the components
    // are numbered in the order of their first vertices.
    let n = comp.iter().map(|&c| c as usize + 1).max().unwrap_or(0);
    let mut members: Vec<Vec<VertId>> = vec![Vec::new(); n];
    for &v in &verts {
        members[comp[v.index()] as usize].push(v);
    }
    let mut out = Vec::with_capacity(n);
    for m in &members {
        let mut seed = Seed {
            vert: m[0],
            cleared: false,
        };
        for &v in m {
            let point = Point::vert(graph.operand, v);
            let (r, cost) = clear(v, share - spent);
            spent = spent.saturating_add(cost);
            if spent > share {
                return (
                    Err(SeedError::OutOfBudget {
                        cause: Exhausted::Work,
                        point,
                    }),
                    spent,
                );
            }
            match r {
                Ok(true) => {
                    seed = Seed {
                        vert: v,
                        cleared: true,
                    };
                    break;
                }
                Ok(false) => {}
                Err(error) => return (Err(SeedError::Clearance { point, error }), spent),
            }
        }
        out.push(seed);
    }
    (Ok(out), spent)
}

/// Whether point `p`'s distance from every box of `boxes` is certified
/// greater than `eps`: the squared distance, bounded below in
/// round-to-nearest ([`down`]), against `eps²` bounded above ([`up`]),
/// or one axis's gap, bounded below, alone past `eps`.
/// The boxes are the other operand's near `p` (a BVH's hits within
/// `eps`): boxes farther away are clear of it by their own query. Never
/// clear for a point or `eps` not finite, or a negative `eps`. A seed's
/// test refines boxes not clear to the face's pieces
/// ([`clear_of_pieces`]).
pub fn clear_of<'b>(p: [f64; 3], eps: f64, boxes: impl IntoIterator<Item = &'b HullBox>) -> bool {
    if !p.iter().all(|x| x.is_finite()) || !eps.is_finite() || eps < 0.0 {
        return false;
    }
    let limit = up(eps * eps);
    boxes.into_iter().all(|b| {
        let mut d2 = 0.0f64;
        for k in 0..3 {
            // The gap along axis `k`, bounded below: a NaN side (none
            // certified) compares false and leaves it 0.
            let below = b.lo[k] - p[k];
            let above = p[k] - b.hi[k];
            let gap = if below > 0.0 {
                down(below)
            } else if above > 0.0 {
                down(above)
            } else {
                0.0
            };
            // One axis's gap alone, where the squares underflow.
            if gap > eps {
                return true;
            }
            if gap > 0.0 {
                d2 = down(d2 + down(gap * gap));
            }
        }
        d2 > limit
    })
}

/// The most boxes one clearance test ([`clear_of_pieces`]) tests, over
/// all its faces; past them the point is not cleared. A point clear of a
/// face by much more than `ε_tie` takes a few boxes per halving of the
/// tile down to its distance, one nearer a few dozen more.
pub const MAX_CLEAR_BOXES: u64 = 1 << 12;

/// A face of the other operand as a clearance test reads it: its UV
/// domain and its tiles' patches at the first level, one per tile in
/// [`FaceUv::tile_boxes`]' order.
#[derive(Clone, Copy, Debug)]
pub struct ClearFace<'a, 'f> {
    /// The face.
    pub face: &'a FaceUv<'f>,
    /// Its tiles' patches.
    pub patches: &'a [HomPatch<Bounded<f64>>],
}

/// Whether point `p`'s distance from each of `faces` is certified greater
/// than `eps` (module docs, "Clearance"): each face's whole tiles first,
/// any not clear refined to its pieces' boxes. Each box tested is charged
/// as a count's piece at the first level ([`Charge`]): its hull, and its
/// classification's and split's pieces. With the answer, or
/// [`Exhausted::Work`] once past `charge.share`, the units spent. Never
/// cleared for a point or `eps` not finite, or a negative `eps`, nor where
/// the level certifies no answer.
pub fn clear_of_pieces(
    p: [f64; 3],
    eps: f64,
    faces: &[ClearFace<'_, '_>],
    charge: &Charge,
) -> (Result<bool, Exhausted>, u64) {
    let mut t = Clearance {
        p,
        eps,
        charge,
        pieces: 0,
        boxes: 0,
    };
    let r = t.all(faces);
    (r, t.spent())
}

/// One clearance test's state ([`clear_of_pieces`]).
struct Clearance<'c> {
    p: [f64; 3],
    eps: f64,
    charge: &'c Charge,
    /// The pieces tested so far.
    pieces: u64,
    /// The boxes tested so far, against [`MAX_CLEAR_BOXES`].
    boxes: u64,
}

impl Clearance<'_> {
    fn spent(&self) -> u64 {
        self.pieces.saturating_mul(self.charge.units[0])
    }

    /// Charges `n` pieces; past the share, out of budget.
    fn take(&mut self, n: u64) -> Result<(), Exhausted> {
        self.pieces = self.pieces.saturating_add(n);
        if self.spent() > self.charge.share {
            return Err(Exhausted::Work);
        }
        Ok(())
    }

    /// Whether `h`, padded by `pad`, is certified clear of the point.
    fn clear(&self, h: &HullBox, pad: f64) -> bool {
        clear_of(self.p, add_up(self.eps, pad), [h])
    }

    fn all(&mut self, faces: &[ClearFace<'_, '_>]) -> Result<bool, Exhausted> {
        if !self.p.iter().all(|x| x.is_finite()) || !self.eps.is_finite() || self.eps < 0.0 {
            return Ok(false);
        }
        for f in faces {
            if !self.face(f)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Whether the point is cleared of face `f`, its tiles in key order,
    /// each box's halves lower first.
    fn face(&mut self, f: &ClearFace<'_, '_>) -> Result<bool, Exhausted> {
        let like = Bounded::exact(0.0);
        let trims = f.face.trims();
        let face_band = widest(trims.iter());
        let mut stack: Vec<(BoxKey, Option<SmallVec<[SpanRef; 8]>>)> =
            f.face.tile_boxes().map(|b| (b, None)).collect();
        stack.reverse();
        while let Some((b, among)) = stack.pop() {
            self.boxes += 1;
            if self.boxes > MAX_CLEAR_BOXES {
                return Ok(false);
            }
            let Some(patch) = f.face.patch_of(f.patches, b.tile()) else {
                return Ok(false);
            };
            self.take(1)?;
            let Some(hull) = box_hull(patch, &b) else {
                return Ok(false);
            };
            // Clear with any ribbon of the face: no classification.
            if self.clear(&hull, face_band) {
                continue;
            }
            let c = match &among {
                None => classify(f.face, &b, &like),
                Some(m) => classify_among(f.face, &b, &like, m),
            };
            let Ok(c) = c else {
                return Ok(false);
            };
            self.take(c.pieces)?;
            let pad = match &c.class {
                Class::Outside => continue,
                Class::Core => 0.0,
                Class::Mixed(cs) => widest(trims.iter().filter(|t| cs.contains(&t.coedge))),
            };
            if pad < face_band && self.clear(&hull, pad) {
                continue;
            }
            // Within `eps` whole, or too small to separate further.
            if within_of(&hull, self.p, self.eps) || within_band(&hull, self.eps / 4.0) {
                return Ok(false);
            }
            let Ok(s) = split(f.face, &b, patch, None) else {
                return Ok(false);
            };
            self.take(s.pieces)?;
            let [lo, hi] = s.halves;
            stack.push((hi, Some(c.met.clone())));
            stack.push((lo, Some(c.met)));
        }
        Ok(true)
    }
}

/// The widest band of `trims`, 0 for none (bands are finite and `≥ 0`,
/// [`FaceUv::new`]).
fn widest<'t>(trims: impl Iterator<Item = &'t Trim<'t>>) -> f64 {
    trims.fold(0.0, |m, t| if t.band > m { t.band } else { m })
}

/// Why the windings aren't settled. [`WindingError::is_signal`] tells
/// the ladder signals from the caller's errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindingError<E> {
    /// The graph can't be walked.
    Graph(GraphError),
    /// Reading an edge's crossings failed (an `s02` or `s11` not
    /// settled, a shared curve's crossing refused): the first such edge
    /// in geometric order.
    Read {
        /// The operand.
        operand: Operand,
        /// The edge.
        edge: EdgeId,
        /// The read's error.
        error: E,
    },
    /// A seed isn't a vertex, or its component already has a seed.
    Seed(Point),
    /// No seed reaches this vertex (the first such in geometric order).
    Unseeded(Point),
    /// Ladder signal: the windings at the edge's ends differ by other
    /// than its crossings (the first such edge in geometric order).
    Disagree {
        /// The operand.
        operand: Operand,
        /// The edge.
        edge: EdgeId,
        /// The windings at its stored start and end.
        w: [i128; 2],
        /// Its crossings summed, `Σ_f x12(e, f)`.
        change: i64,
    },
    /// Ladder signal: a winding outside `{0, 1}` (the first such vertex
    /// in geometric order).
    Outside {
        /// The vertex.
        point: Point,
        /// Its winding.
        w: i128,
    },
    /// Counting the changes or seeds again after a lobe fallback failed
    /// ([`windings_lobed`]).
    Recount(E),
}

impl<E> WindingError<E> {
    /// Whether this is a ladder signal (module docs): the decisions don't
    /// fit, the next rung re-decides.
    pub fn is_signal(&self) -> bool {
        matches!(
            self,
            WindingError::Disagree { .. } | WindingError::Outside { .. }
        )
    }
}

/// Each vertex's winding number in the other operand (module docs), by
/// id: from `seeds` (vertex, its winding `Σ_f s02(v0, f)`), one per
/// component, carried along the edges by `change(e) = Σ_f x12(e, f)`
/// (`x21` for `B`'s edges), read once per edge in geometric order; then
/// every edge checked to agree and every winding to be 0 or 1.
pub fn windings<O: EntityOrder, E>(
    order: &O,
    graph: &Graph<'_>,
    seeds: &[(VertId, i64)],
    mut change: impl FnMut(EdgeId) -> Result<i64, E>,
) -> Result<Vec<i64>, WindingError<E>> {
    graph.checked().map_err(WindingError::Graph)?;
    let operand = graph.operand;
    let edges = graph.edges_in_order(order);
    let mut dw = vec![0i64; graph.edges.len()];
    for &e in &edges {
        dw[e.index()] = change(e).map_err(|error| WindingError::Read {
            operand,
            edge: e,
            error,
        })?;
    }
    let adj = graph.adjacency(&edges);
    let nv = graph.verts as usize;
    // In `i128`: fewer than `2³²` edges of changes in `i64` can't
    // overflow it (module docs).
    let mut w: Vec<Option<i128>> = vec![None; nv];
    let mut queue = VecDeque::new();
    for &(s, ws) in seeds {
        let point = Point::vert(operand, s);
        if s.0 >= graph.verts || w[s.index()].is_some() {
            return Err(WindingError::Seed(point));
        }
        w[s.index()] = Some(i128::from(ws));
        queue.push_back(s);
        while let Some(v) = queue.pop_front() {
            let wv = w[v.index()].expect("queued with a winding");
            for &e in &adj[v.index()] {
                let [a, _] = graph.edges[e.index()];
                let o = graph.other_end(e, v);
                if w[o.index()].is_some() {
                    continue;
                }
                let d = i128::from(dw[e.index()]);
                w[o.index()] = Some(if a == v { wv + d } else { wv - d });
                queue.push_back(o);
            }
        }
    }
    let verts = graph.verts_in_order(order);
    if let Some(&v) = verts.iter().find(|v| w[v.index()].is_none()) {
        return Err(WindingError::Unseeded(Point::vert(operand, v)));
    }
    let w: Vec<i128> = w.into_iter().map(|x| x.unwrap_or(0)).collect();
    for &e in &edges {
        let [a, b] = graph.edges[e.index()];
        let ends = [w[a.index()], w[b.index()]];
        if ends[1] - ends[0] != i128::from(dw[e.index()]) {
            return Err(WindingError::Disagree {
                operand,
                edge: e,
                w: ends,
                change: dw[e.index()],
            });
        }
    }
    if let Some(&v) = verts.iter().find(|v| !(0..=1).contains(&w[v.index()])) {
        return Err(WindingError::Outside {
            point: Point::vert(operand, v),
            w: w[v.index()],
        });
    }
    Ok(w.into_iter()
        .map(|w| i64::try_from(w).expect("0 or 1"))
        .collect())
}

/// The lobe fallbacks of one operand's windings (module docs, "Lobes"):
/// which `(q, g)` fell back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lobes {
    taken: BTreeSet<(Point, EdgeId)>,
}

impl Lobes {
    /// None taken.
    pub fn new() -> Lobes {
        Lobes::default()
    }

    /// The edges point `q` fell back at, by id.
    pub fn at(&self, q: Point) -> Vec<EdgeId> {
        self.taken
            .iter()
            .filter(|(p, _)| *p == q)
            .map(|&(_, g)| g)
            .collect()
    }

    /// The points that fell back, each once, by key.
    pub fn points(&self) -> Vec<Point> {
        let mut out: Vec<Point> = self.taken.iter().map(|&(p, _)| p).collect();
        out.dedup();
        out
    }

    /// The fallbacks taken.
    pub fn count(&self) -> usize {
        self.taken.len()
    }

    /// The first of `near` (edges in geometric order) `q` hasn't fallen
    /// back at, taken.
    fn take(&mut self, q: Point, near: &[EdgeId]) -> Option<EdgeId> {
        let g = *near.iter().find(|&&g| !self.taken.contains(&(q, g)))?;
        self.taken.insert((q, g));
        Some(g)
    }
}

/// The seeds' windings and each edge's change, by id, as
/// [`windings_lobed`]'s `counts` gives them.
pub type Counted = (Vec<(VertId, i64)>, Vec<i64>);

/// [`windings`] with the lobe fallback (module docs, "Lobes"): `counts`
/// gives the seeds' windings and each edge's change (one per edge, by
/// id) with the fallbacks `lobes` holds; `join_zone(q)` the edges of the other
/// operand within whose near radius `q` lies, in geometric order. On a
/// winding outside `{0, 1}` at `q` the first of them `q` hasn't fallen
/// back at is taken and everything counted and carried again; where none
/// is left, the signal stands.
pub fn windings_lobed<O: EntityOrder, E>(
    order: &O,
    graph: &Graph<'_>,
    lobes: &mut Lobes,
    mut counts: impl FnMut(&Lobes) -> Result<Counted, E>,
    mut join_zone: impl FnMut(Point) -> Result<Vec<EdgeId>, E>,
) -> Result<Vec<i64>, WindingError<E>> {
    loop {
        let (seeds, dw) = counts(lobes).map_err(WindingError::Recount)?;
        if dw.len() != graph.edges.len() {
            return Err(WindingError::Graph(GraphError::SizeLimit));
        }
        let r = windings(order, graph, &seeds, |e| {
            Ok::<i64, std::convert::Infallible>(dw[e.index()])
        });
        match r {
            Ok(w) => return Ok(w),
            Err(WindingError::Outside { point, w }) => {
                let near = join_zone(point).map_err(WindingError::Recount)?;
                if lobes.take(point, &near).is_none() {
                    return Err(WindingError::Outside { point, w });
                }
            }
            Err(e) => return Err(widen(e)),
        }
    }
}

/// A winding error whose reads can't fail, as one whose can.
fn widen<E>(e: WindingError<std::convert::Infallible>) -> WindingError<E> {
    match e {
        WindingError::Graph(g) => WindingError::Graph(g),
        WindingError::Read { error, .. } | WindingError::Recount(error) => match error {},
        WindingError::Seed(p) => WindingError::Seed(p),
        WindingError::Unseeded(p) => WindingError::Unseeded(p),
        WindingError::Disagree {
            operand,
            edge,
            w,
            change,
        } => WindingError::Disagree {
            operand,
            edge,
            w,
            change,
        },
        WindingError::Outside { point, w } => WindingError::Outside { point, w },
    }
}

#[cfg(test)]
mod tests;
