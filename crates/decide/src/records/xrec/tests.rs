//! Cluster records on hand-made feet and orders: the z-order key, the
//! crossing itself whatever lies near it; `s11` from clear and read
//! z-orders; the end a parallel pair's tie came from; cut points'
//! stored positions.

use std::collections::BTreeMap;

use super::*;
use crate::ids::VertId;
use crate::order::{EdgeKey, VertKey};
use crate::records::feet::{EndKind, Feet, Foot, FootEnd};
use crate::records::height::HeightReads;
use crate::records::shadow::{RayRec, VvRec};

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
            place: crate::records::shadow::SideAhead {
                side: Sign::Pos,
                ahead: Sign::Pos,
            },
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

/// Every cluster reads its own crossing key, whatever lies near it: both
/// ends of `e` within `r_g` of the cluster's piece of `g`, the ends of `g`
/// near `e`, a shared corner's point–vertex record near, feet missing.
#[test]
fn a_cluster_reads_its_crossing_key_whatever_lies_near() {
    for ids in [[10, 11, 20, 21], [11, 10, 21, 20]] {
        let (mut r, x, [a, b, g0, g1]) = pair(ids);
        for w in [a, b] {
            let lo = stored(&r, B, x.g, g0, 0.4);
            let hi = stored(&r, B, x.g, g0, 0.6);
            r.foot(Point::vert(A, w), x.g, between(lo, hi));
        }
        for w in [g0, g1] {
            r.foot(Point::vert(B, w), x.e, foot(0.0, 1.0));
        }
        r.near.insert((Point::vert(A, a), g0), true);
        for c in [0, 3] {
            let z = zorder_key(x, XClusterIx(c));
            let want = HKey::X {
                e: x.e,
                g: x.g,
                c: XClusterIx(c),
            };
            assert_eq!(z.key.key, want, "ids {ids:?}");
            assert!(!z.key.swapped);
            assert_eq!(z.height_key(x.g).anchor, Anchor::Edge(B, x.g));
            let rec = HeightRec {
                above: Sign::Pos,
                by_orders: false,
                fold: false,
            };
            assert_eq!(z.above(&rec), Sign::Pos);
        }
    }
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
        level: 0,
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

/// A cut point is its piece's middle on the cut edge, exactly a span's
/// end where the piece is that end; `None` past the spans.
#[test]
fn cut_points_are_their_pieces_middles() {
    let line = XCurve::Line([0.0, 0.0, 0.0], [4.0, 2.0, -8.0]);
    let part = |span, t| XPart { span, t };
    assert_eq!(
        cut_point(&line, &part(0, [0.25, 0.75])),
        Some([2.0, 1.0, -4.0])
    );
    assert_eq!(
        cut_point(&line, &part(0, [1.0, 1.0])),
        Some([4.0, 2.0, -8.0])
    );
    assert_eq!(cut_point(&line, &part(1, [0.0, 0.5])), None);
    // A quarter circle, weight `√2/2` at its corner: its middle is
    // `(√2/2, √2/2)`, rounded.
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let spans = [
        [
            [1.0, 0.0, 0.0, 1.0],
            [1.0, 1.0, 0.0, w],
            [0.0, 1.0, 0.0, 1.0],
        ],
        [
            [0.0, 1.0, 0.0, 1.0],
            [-1.0, 1.0, 0.0, w],
            [-1.0, 0.0, 0.0, 1.0],
        ],
    ];
    let arc = XCurve::Spans(&spans);
    let m = cut_point(&arc, &part(0, [0.0, 1.0])).expect("a point");
    assert!(
        (m[0] - w).abs() < 1e-15 && (m[1] - w).abs() < 1e-15,
        "{m:?}"
    );
    assert_eq!(cut_point(&arc, &part(1, [0.0, 0.0])), Some([0.0, 1.0, 0.0]));
    assert_eq!(
        cut_point(&arc, &part(1, [1.0, 1.0])),
        Some([-1.0, 0.0, 0.0])
    );
    assert_eq!(cut_point(&arc, &part(2, [0.0, 1.0])), None);
    // The same for every piece of one cluster read from either side.
    let cl = XCluster {
        e: part(1, [0.5, 0.5]),
        g: part(0, [0.0, 1.0]),
        degree: 1,
        clear: None,
        row: ZRow::EdgeEdge,
    };
    assert_eq!(cl.part(A), &cl.e);
    assert_eq!(cl.part(B), &cl.g);
    let p = cut_point(&arc, cl.part(A)).expect("a point");
    assert!(
        (p[0] + w).abs() < 1e-15 && (p[1] - w).abs() < 1e-15,
        "{p:?}"
    );
}
