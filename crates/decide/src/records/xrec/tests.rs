//! Cluster records on hand-made feet and orders: the z-order key from an
//! end of `e`, of `g` (negated) or the crossing itself, the end chosen by
//! position whatever the ids and stored directions; a shared corner's
//! point–vertex key; feet that miss the cluster's piece; `s11` from clear
//! and read z-orders; the end a parallel pair's tie came from.

use std::collections::BTreeMap;

use super::*;
use crate::order::{EdgeKey, VertKey};
use crate::records::feet::{EndKind, FootEnd};
use crate::records::height::HeightReads;
use crate::records::shadow::VvRec;

use Operand::{A, B};

/// Records made by hand: positions give the order, `near` and feet as
/// stored.
#[derive(Default)]
struct Reads {
    pos: BTreeMap<(Operand, VertId), [f64; 3]>,
    edges: BTreeMap<(Operand, EdgeId), [VertId; 2]>,
    near: BTreeMap<(Point, VertId), bool>,
    feet: BTreeMap<(Point, EdgeId), Feet>,
}

impl Reads {
    fn vert(&mut self, op: Operand, id: u32, at: [f64; 3]) -> VertId {
        self.pos.insert((op, VertId(id)), at);
        VertId(id)
    }

    fn edge(&mut self, op: Operand, id: u32, v: VertId, w: VertId) -> EdgeId {
        let ends = if v < w { [v, w] } else { [w, v] };
        self.edges.insert((op, EdgeId(id)), ends);
        EdgeId(id)
    }

    fn foot(&mut self, q: Point, g: EdgeId, f: Foot) {
        self.feet.entry((q, g)).or_default().feet.push(f);
    }

    /// Every end of `e` and `g` with no feet on the other edge.
    fn no_feet(&mut self, e: EdgeId, g: EdgeId) {
        for (op, x, y) in [(A, e, g), (B, g, e)] {
            for v in self.edges[&(op, x)] {
                self.feet.entry((Point::vert(op, v), y)).or_default();
            }
        }
    }
}

impl HeightReads for Reads {
    fn vv(&self, q: Point, w: VertId) -> Option<VvRec> {
        self.near.get(&(q, w)).map(|&near| VvRec {
            side: Sign::Pos,
            ahead: Sign::Pos,
            near,
        })
    }

    fn feet(&self, q: Point, g: EdgeId) -> Option<&Feet> {
        self.feet.get(&(q, g))
    }
}

impl EntityOrder for Reads {
    type Key = ();

    fn ends(&self, operand: Operand, g: EdgeId) -> [VertId; 2] {
        self.edges[&(operand, g)]
    }

    fn vert_key(&self, operand: Operand, v: VertId) -> VertKey<()> {
        VertKey::new(self.pos[&(operand, v)], []).expect("no NaN")
    }

    fn edge_key(&self, operand: Operand, g: EdgeId) -> EdgeKey<()> {
        let [s, e] = self.edges[&(operand, g)];
        let [a, b] = [s, e].map(|v| self.pos[&(operand, v)]);
        let mid = [0, 1, 2].map(|k| 0.5 * (a[k] + b[k]));
        EdgeKey::new([s, e].map(|v| self.vert_key(operand, v)), mid, [(), ()]).expect("no NaN")
    }
}

/// A foot over `[lo, hi]` of span 0, holding an end where it reaches `0`
/// or `1`.
fn foot(lo: f64, hi: f64) -> Foot {
    let end = |t: f64| FootEnd {
        span: 0,
        t: [t, t],
        kind: if t == 0.0 || t == 1.0 {
            EndKind::Vertex
        } else {
            EndKind::Crossing
        },
    };
    Foot {
        lo: end(lo),
        hi: end(hi),
        wraps: false,
        hull: false,
    }
}

/// A foot between `a` and `b`, in either order.
fn between(a: f64, b: f64) -> Foot {
    if a <= b { foot(a, b) } else { foot(b, a) }
}

fn cluster(e: [f64; 2], g: [f64; 2]) -> XCluster {
    XCluster {
        e: XPart { span: 0, t: e },
        g: XPart { span: 0, t: g },
        degree: 1,
        clear: None,
        row: ZRow::EdgeEdge,
    }
}

/// `e` of `A` from `(0,0,0)` to `(4,0,0)` and `g` of `B` from `(2,−2,1)`
/// to `(2,2,1)`, the ends' ids as given (each edge stored from its
/// lower-id end).
fn pair(ids: [u32; 4]) -> (Reads, XKey, [VertId; 4]) {
    let mut r = Reads::default();
    let a = r.vert(A, ids[0], [0.0, 0.0, 0.0]);
    let b = r.vert(A, ids[1], [4.0, 0.0, 0.0]);
    let g0 = r.vert(B, ids[2], [2.0, -2.0, 1.0]);
    let g1 = r.vert(B, ids[3], [2.0, 2.0, 1.0]);
    let e = r.edge(A, 1, a, b);
    let g = r.edge(B, 2, g0, g1);
    r.no_feet(e, g);
    (r, XKey { e, g }, [a, b, g0, g1])
}

/// Stored parameter on edge `g` of `op` of the point at `t` from the
/// vertex `from`.
fn stored(r: &Reads, op: Operand, g: EdgeId, from: VertId, t: f64) -> f64 {
    if r.edges[&(op, g)][0] == from {
        t
    } else {
        1.0 - t
    }
}

#[test]
fn both_ends_of_e_near_choose_the_lesser_by_position_whatever_the_ids() {
    for ids in [[10, 11, 20, 21], [11, 10, 21, 20]] {
        let (mut r, x, [a, b, g0, _]) = pair(ids);
        // Both ends of `e` lie within `r_g` of `g`'s middle stretch.
        for w in [a, b] {
            let lo = stored(&r, B, x.g, g0, 0.4);
            let hi = stored(&r, B, x.g, g0, 0.6);
            r.foot(Point::vert(A, w), x.g, between(lo, hi));
        }
        let mid = stored(&r, B, x.g, g0, 0.5);
        let cl = cluster([0.25, 0.75], [mid - 0.125, mid + 0.125]);
        let z = zorder_key(&r, x, XClusterIx(0), &cl, [1, 1]).expect("a key");
        // `a` at the origin is the lesser by position, whichever id it has.
        let want = HKey::Curve {
            q: Point::vert(A, a),
            g: x.g,
            foot: FootId(0),
        };
        assert_eq!(z.key.key, want, "ids {ids:?}");
        assert!(!z.negated);
        assert_eq!(z.height_key(x.g).anchor, Anchor::Edge(B, x.g));
    }
}

#[test]
fn an_end_of_g_near_e_reads_negated() {
    for ids in [[10, 11, 20, 21], [11, 10, 21, 20]] {
        let (mut r, x, [a, _, g0, g1]) = pair(ids);
        let lo = stored(&r, A, x.e, a, 0.45);
        let hi = stored(&r, A, x.e, a, 0.55);
        for w in [g0, g1] {
            r.foot(Point::vert(B, w), x.e, between(lo, hi));
        }
        let cl = cluster([0.25, 0.75], [0.25, 0.75]);
        let z = zorder_key(&r, x, XClusterIx(3), &cl, [1, 1]).expect("a key");
        // `g0` at `y = −2` is the lesser.
        assert_eq!(
            z.key,
            Oriented {
                key: HKey::Curve {
                    q: Point::vert(B, g0),
                    g: x.e,
                    foot: FootId(0)
                },
                swapped: false
            },
            "ids {ids:?}"
        );
        assert!(z.negated);
        let rec = HeightRec {
            above: Sign::Pos,
            by_orders: false,
            fold: false,
        };
        // `e` above `g0`: `g` below `e`.
        assert_eq!(z.above(&rec), Sign::Neg);
    }
}

#[test]
fn feet_missing_the_piece_give_the_crossing_key() {
    let (mut r, x, [a, b, g0, g1]) = pair([10, 11, 20, 21]);
    // Feet on the far stretches only.
    r.foot(Point::vert(A, a), x.g, foot(0.0, 0.1));
    r.foot(Point::vert(A, b), x.g, foot(0.9, 1.0));
    r.foot(Point::vert(B, g0), x.e, foot(0.0, 0.2));
    r.foot(Point::vert(B, g1), x.e, foot(0.8, 1.0));
    let cl = cluster([0.375, 0.625], [0.375, 0.625]);
    let z = zorder_key(&r, x, XClusterIx(2), &cl, [1, 1]).expect("a key");
    assert_eq!(
        z.key.key,
        HKey::X {
            e: x.e,
            g: x.g,
            c: XClusterIx(2)
        }
    );
    assert!(!z.negated && !z.key.swapped);
    // A foot whose bracket touches the piece meets it.
    r.feet
        .get_mut(&(Point::vert(A, b), x.g))
        .expect("feet")
        .feet[0] = foot(0.625, 1.0);
    // It holds `g1`, not near `b`.
    r.near.insert((Point::vert(A, b), g1), false);
    let z = zorder_key(&r, x, XClusterIx(2), &cl, [1, 1]).expect("a key");
    assert_eq!(
        z.key.key,
        HKey::Curve {
            q: Point::vert(A, b),
            g: x.g,
            foot: FootId(0)
        }
    );
}

#[test]
fn a_shared_corner_reads_its_point_vertex_key() {
    let (mut r, x, [a, _, g0, _]) = pair([10, 11, 20, 21]);
    // `a`'s foot on `g` holds `g0`, and the two are near.
    r.foot(Point::vert(A, a), x.g, foot(0.0, 0.2));
    r.near.insert((Point::vert(A, a), g0), true);
    let cl = cluster([0.0, 0.125], [0.0, 0.125]);
    let z = zorder_key(&r, x, XClusterIx(0), &cl, [1, 1]).expect("a key");
    assert_eq!(z.key, HKey::vv(Point::vert(A, a), g0));
}

#[test]
fn a_foot_through_a_closed_edges_vertex_meets_both_ends() {
    let f = Foot {
        lo: FootEnd {
            span: 2,
            t: [0.75, 0.75],
            kind: EndKind::Crossing,
        },
        hi: FootEnd {
            span: 0,
            t: [0.25, 0.25],
            kind: EndKind::Crossing,
        },
        wraps: true,
        hull: false,
    };
    let at = |span, t: [f64; 2]| XPart { span, t }.meets(&f, 3);
    assert!(at(0, [0.0, 0.125]));
    assert!(at(2, [0.875, 1.0]));
    assert!(!at(1, [0.0, 1.0]));
    assert!(!at(0, [0.5, 1.0]));
}

/// A foot ending at a knot touches a piece starting there, whichever
/// span's parameter names the knot.
#[test]
fn a_foot_ending_at_a_knot_meets_the_next_span() {
    let end = |span, t: [f64; 2]| FootEnd {
        span,
        t,
        kind: EndKind::Crossing,
    };
    let f = |lo, hi| Foot {
        lo,
        hi,
        wraps: false,
        hull: false,
    };
    let to_knot = f(end(0, [0.25, 0.25]), end(0, [0.875, 1.0]));
    let from_knot = f(end(2, [0.0, 0.0]), end(2, [0.5, 0.5]));
    let at = |foot: &Foot, span, t: [f64; 2]| XPart { span, t }.meets(foot, 3);
    assert!(at(&to_knot, 1, [0.0, 0.5]));
    assert!(!at(&to_knot, 1, [0.25, 0.5]));
    assert!(at(&from_knot, 1, [0.5, 1.0]));
    assert!(!at(&from_knot, 1, [0.5, 0.75]));
    assert!(!at(&to_knot, 2, [0.0, 0.5]));
}

#[test]
fn missing_feet_are_reported() {
    let (mut r, x, [a, ..]) = pair([10, 11, 20, 21]);
    r.feet.remove(&(Point::vert(A, a), x.g));
    let cl = cluster([0.0, 1.0], [0.0, 1.0]);
    assert_eq!(
        zorder_key(&r, x, XClusterIx(0), &cl, [1, 1]),
        Err(KeyError::Missing(RecordKey::Ray(RayKey {
            q: Point::vert(A, a),
            g: x.g
        })))
    );
}

#[test]
fn s11_sums_the_clusters_with_g_above() {
    let mut c = [cluster([0.0, 0.5], [0.0, 0.5]); 3];
    c[0].clear = Some(Sign::Pos);
    c[0].degree = 1;
    c[1].clear = Some(Sign::Neg);
    c[1].degree = -1;
    c[2].degree = -1;
    let rec = XRec {
        clusters: c.to_vec(),
    };
    assert_eq!(rec.degree(), -1);
    let read = |above: Sign| {
        rec.s11(|ix, _| {
            assert_eq!(ix, XClusterIx(2), "only the unclear cluster reads");
            Ok::<_, ()>(above)
        })
    };
    assert_eq!(read(Sign::Pos), Ok(0));
    assert_eq!(read(Sign::Neg), Ok(1));
    assert_eq!(rec.s11(|_, _| Err::<Sign, _>(7)), Err(7));
}

#[test]
fn a_parallel_pairs_tie_comes_from_a_counting_end() {
    let (r, x, [a, b, g0, _]) = pair([10, 11, 20, 21]);
    let rays = |k: i8| {
        EdgeRays::Line(RayRec {
            crossing: match k {
                0 => None,
                1 => Some(Sign::Pos),
                _ => Some(Sign::Neg),
            },
        })
    };
    let terms = |ag, bg, g0e, g1e| XTerms {
        a_g: rays(ag),
        b_g: rays(bg),
        g0_e: rays(g0e),
        g1_e: rays(g1e),
    };
    assert_eq!(tie_point(&r, x, &terms(0, 0, 0, 0)), None);
    assert_eq!(
        tie_point(&r, x, &terms(0, 1, 0, 0)),
        Some(Point::vert(A, b))
    );
    // `−RayRec(a, g)` and `−RayRec(g1, e)` count `+1`: `a` at the origin
    // is the lesser.
    assert_eq!(
        tie_point(&r, x, &terms(-1, 0, 0, -1)),
        Some(Point::vert(A, a))
    );
    // `X = +1` from `b` and `g0` (`+1`) against `g1` (`−1`): the lesser of
    // `b` and `g0`, `g0` at `x = 2`.
    assert_eq!(
        tie_point(&r, x, &terms(0, 1, 1, 1)),
        Some(Point::vert(B, g0))
    );
}
