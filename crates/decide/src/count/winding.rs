//! Winding numbers: each vertex's winding number `w(v)` in the other
//! operand, `1` inside it and `0` outside, from one seed per connected
//! component of the operand's edges, carried along every edge by its
//! crossings.
//!
//! **Seeds** ([`seeds`]). Per component, the first vertex in geometric
//! order ([`EntityOrder::vert_key`], ids only on exact ties) whose
//! certified 3D clearance from the other operand exceeds `ε_tie`
//! ([`clear_of`] over the boxes near it); if none has clearance, the
//! first vertex, marked uncleared. The caller counts the seed's winding
//! `w(v0) = Σ_f s02(v0, f)` (`s20` for a vertex of `B`) over every face
//! of the other operand, from the same records and the same decisions as
//! the crossings ([`crate::count::s02`]), so one ray suffices: a tie at
//! the seed is decided once and read by both.
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
//! **Budget.** Each clearance test costs what its certificate reports,
//! charged against the share the search is given; past it the search
//! stops, [`SeedError::OutOfBudget`] keyed by the vertex being tested.
//! Propagation and its checks are integer sums, one per edge.

use std::collections::VecDeque;

use crate::bezier::HullBox;
use crate::count::s02::Exhausted;
use crate::ids::{EdgeId, VertId};
use crate::num::bounded::{down, up};
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
/// in the geometric order of their first vertices. `clear(v)` certifies
/// vertex `v`'s clearance from the other operand (`Ok(true)` certified,
/// `Ok(false)` not) with its cost in units, each charged against `share`.
/// With the seeds, or the error, the units spent.
pub fn seeds<O: EntityOrder, E>(
    order: &O,
    graph: &Graph<'_>,
    share: u64,
    mut clear: impl FnMut(VertId) -> (Result<bool, E>, u64),
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
            let (r, cost) = clear(v);
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
/// clear for a point or `eps` not finite, or a negative `eps`.
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

#[cfg(test)]
mod tests;
