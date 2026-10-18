//! Cluster records on hand-made feet and orders: the z-order key from an
//! end of `e`, of `g` (negated) or the crossing itself, the end chosen by
//! position whatever the ids and stored directions; a shared corner's
//! point–vertex key; feet that miss the cluster's piece; `s11` from clear
//! and read z-orders; the end a parallel pair's tie came from; a
//! shared-curve pair's clusters from its ends' ties alone; cut points'
//! stored positions.

use std::collections::BTreeMap;

use super::*;
use crate::order::{EdgeKey, VertKey};
use crate::records::feet::{EndKind, FootEnd};
use crate::records::height::HeightReads;
use crate::records::ray::{ChainCrossing, ChainRec, Crossing};
use crate::records::shadow::{RayRec, VvRec};
use smallvec::smallvec;

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
    let at = |span, t: [f64; 2]| XPart { span, t }.meets(&f, 3, true);
    assert!(at(0, [0.0, 0.125]));
    assert!(at(2, [0.875, 1.0]));
    assert!(!at(1, [0.0, 1.0]));
    assert!(!at(0, [0.5, 1.0]));
}

/// On a closed edge a foot reaching the vertex from one side only (not
/// through it) touches a piece at the vertex from the other side; on an
/// open edge the chain's two ends are apart.
#[test]
fn a_foot_at_a_closed_edges_vertex_touches_the_other_side() {
    let end = |span, t: [f64; 2], kind| FootEnd { span, t, kind };
    let f = |lo, hi| Foot {
        lo,
        hi,
        wraps: false,
        hull: false,
    };
    let from_start = f(
        end(0, [0.0, 0.0], EndKind::Vertex),
        end(0, [0.25, 0.25], EndKind::Crossing),
    );
    let to_end = f(
        end(2, [0.75, 0.75], EndKind::Crossing),
        end(2, [1.0, 1.0], EndKind::Vertex),
    );
    let at = |foot: &Foot, span, t: [f64; 2], closed| XPart { span, t }.meets(foot, 3, closed);
    assert!(at(&from_start, 2, [0.875, 1.0], true));
    assert!(!at(&from_start, 2, [0.875, 1.0], false));
    assert!(!at(&from_start, 2, [0.5, 0.875], true));
    assert!(at(&to_end, 0, [0.0, 0.125], true));
    assert!(!at(&to_end, 0, [0.0, 0.125], false));
    assert!(!at(&to_end, 0, [0.125, 0.25], true));
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
    let at = |foot: &Foot, span, t: [f64; 2]| XPart { span, t }.meets(foot, 3, false);
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

fn line_rays(k: i8) -> EdgeRays<'static> {
    EdgeRays::Line(RayRec {
        crossing: match k {
            0 => None,
            1 => Some(Sign::Pos),
            _ => Some(Sign::Neg),
        },
    })
}

/// `e` of `A` from `(0,0,0)` to `(4,0,0)` and `g` of `B` from `(2,0,0)`
/// to `(6,0,0)`, on one line: `b` lies on `g` at its middle.
fn shared_pair() -> (Reads, XKey, [VertId; 4]) {
    let mut r = Reads::default();
    let a = r.vert(A, 0, [0.0, 0.0, 0.0]);
    let b = r.vert(A, 1, [4.0, 0.0, 0.0]);
    let g0 = r.vert(B, 2, [2.0, 0.0, 0.0]);
    let g1 = r.vert(B, 3, [6.0, 0.0, 0.0]);
    let e = r.edge(A, 1, a, b);
    let g = r.edge(B, 2, g0, g1);
    r.no_feet(e, g);
    (r, XKey { e, g }, [a, b, g0, g1])
}

fn shared_lines<'a>(terms: [i8; 4]) -> XPair<'a> {
    XPair {
        e: XCurve::Line([0.0, 0.0, 0.0], [4.0, 0.0, 0.0]),
        g: XCurve::Line([2.0, 0.0, 0.0], [6.0, 0.0, 0.0]),
        terms: XTerms {
            a_g: line_rays(terms[0]),
            b_g: line_rays(terms[1]),
            g0_e: line_rays(terms[2]),
            g1_e: line_rays(terms[3]),
        },
        shared: true,
    }
}

/// A tie at `b` (on `g`, its ray crossing `g` there) is one cluster at
/// `b`, reading the point–curve row there, its pieces `b` itself and the
/// crossing's place on `g`; a tie at `g0` (on `e`) likewise, with the
/// identity's sign.
#[test]
fn a_shared_curves_crossings_are_its_ends_ties() {
    let (mut r, x, [_, b, g0, _]) = shared_pair();
    r.foot(Point::vert(A, b), x.g, foot(0.5, 0.5));
    r.foot(Point::vert(B, g0), x.e, foot(0.5, 0.5));
    let rec = shared_clusters(&r, x, &shared_lines([0, 1, 0, 0])).expect("formed");
    assert_eq!(rec.clusters.len(), 1);
    let cl = rec.clusters[0];
    assert_eq!(cl.degree, 1);
    assert_eq!(cl.row, ZRow::PointCurve(Point::vert(A, b)));
    assert_eq!(cl.clear, None);
    assert_eq!(cl.e.t, [1.0, 1.0]);
    assert!(cl.g.t[0] <= 0.5 && 0.5 <= cl.g.t[1], "{:?}", cl.g);
    // Both ties, of opposite signs: two clusters, `X = 0`.
    let rec = shared_clusters(&r, x, &shared_lines([0, 1, 1, 0])).expect("formed");
    assert_eq!(rec.degree(), 2);
    let rec = shared_clusters(&r, x, &shared_lines([0, -1, 1, 0])).expect("formed");
    assert_eq!(rec.degree(), 0);
    assert_eq!(
        rec.clusters
            .iter()
            .map(|c| (c.degree, c.row))
            .collect::<Vec<_>>(),
        vec![
            (-1, ZRow::PointCurve(Point::vert(A, b))),
            (1, ZRow::PointCurve(Point::vert(B, g0)))
        ]
    );
    assert_eq!(rec.clusters[1].g.t, [0.0, 0.0]);
}

/// A crossing outside every end's feet isn't a tie: refused, never
/// searched for.
#[test]
fn a_shared_curves_crossing_away_from_the_ends_is_refused() {
    let (r, x, _) = shared_pair();
    assert_eq!(
        shared_clusters(&r, x, &shared_lines([0, 1, 0, 0])),
        Err(SharedError::Crossed(1))
    );
    assert_eq!(
        shared_clusters(&r, x, &shared_lines([0, 0, 0, 0])),
        Ok(XRec::default())
    );
    let mut r = Reads::default();
    let (a, b) = (r.vert(A, 0, [0.0; 3]), r.vert(A, 1, [4.0, 0.0, 0.0]));
    let (g0, g1) = (r.vert(B, 2, [2.0, 0.0, 0.0]), r.vert(B, 3, [6.0, 0.0, 0.0]));
    let x = XKey {
        e: r.edge(A, 1, a, b),
        g: r.edge(B, 2, g0, g1),
    };
    assert_eq!(
        shared_clusters(&r, x, &shared_lines([0, 1, 0, 0])),
        Err(SharedError::Missing(RecordKey::Ray(RayKey {
            q: Point::vert(A, a),
            g: x.g
        })))
    );
}

/// A curved edge with no spans doesn't fit: refused, never a panic.
#[test]
fn a_shared_chain_without_spans_is_refused() {
    let (mut r, x, [a, b, _, _]) = shared_pair();
    r.foot(Point::vert(A, a), x.g, foot(0.0, 0.0));
    r.foot(Point::vert(A, b), x.g, foot(0.5, 0.5));
    let none = ChainRec::default();
    for e_empty in [true, false] {
        let empty: [Span; 0] = [];
        let line = XCurve::Line([2.0, 0.0, 0.0], [6.0, 0.0, 0.0]);
        let (e, g) = if e_empty {
            (XCurve::Spans(&empty), line)
        } else {
            (
                XCurve::Line([0.0; 3], [4.0, 0.0, 0.0]),
                XCurve::Spans(&empty),
            )
        };
        let rays = |c: &XCurve<'_>| match c {
            XCurve::Line(..) => line_rays(0),
            XCurve::Spans(_) => EdgeRays::Chain(&none),
        };
        let pair = XPair {
            e,
            g,
            terms: XTerms {
                a_g: rays(&g),
                b_g: rays(&g),
                g0_e: rays(&e),
                g1_e: rays(&e),
            },
            shared: true,
        };
        assert_eq!(shared_clusters(&r, x, &pair), Err(SharedError::Shape));
    }
}

/// A curved `g`: of `a`'s two crossings, the one in `a`'s foot is a
/// cluster, the other not a tie; a closed `e`'s two terms cancel.
#[test]
fn shared_chains_place_their_ties_and_closed_edges_cancel() {
    let mut r = Reads::default();
    let a = r.vert(A, 0, [0.0; 3]);
    let b = r.vert(A, 1, [4.0, 0.0, 0.0]);
    let g0 = r.vert(B, 2, [0.0, 0.0, 0.0]);
    let g1 = r.vert(B, 3, [4.0, 1.0, 0.0]);
    let x = XKey {
        e: r.edge(A, 1, a, b),
        g: r.edge(B, 2, g0, g1),
    };
    r.no_feet(x.e, x.g);
    r.foot(Point::vert(A, a), x.g, foot(0.0, 0.25));
    let spans = [[
        [0.0, 0.0, 0.0, 1.0],
        [2.0, 2.0, 0.0, 1.0],
        [4.0, 1.0, 0.0, 1.0],
    ]];
    let chain = |t: [f64; 2], sign| ChainCrossing {
        span: 0,
        crossing: Crossing {
            t,
            sign,
            single: true,
        },
    };
    let one = ChainRec {
        crossings: smallvec![chain([0.0, 0.0], Sign::Pos)],
        ..ChainRec::default()
    };
    let two = ChainRec {
        crossings: smallvec![chain([0.0, 0.0], Sign::Pos), chain([0.5, 0.75], Sign::Neg)],
        ..ChainRec::default()
    };
    let none = ChainRec::default();
    let pair = |rays| XPair {
        e: XCurve::Line([0.0; 3], [4.0, 0.0, 0.0]),
        g: XCurve::Spans(&spans),
        terms: XTerms {
            a_g: rays,
            b_g: EdgeRays::Chain(&none),
            g0_e: line_rays(0),
            g1_e: line_rays(0),
        },
        shared: true,
    };
    let rec = shared_clusters(&r, x, &pair(EdgeRays::Chain(&one))).expect("formed");
    assert_eq!(rec.degree(), -1);
    assert_eq!(rec.clusters[0].row, ZRow::PointCurve(Point::vert(A, a)));
    assert_eq!(rec.clusters[0].e.t, [0.0, 0.0]);
    assert_eq!(
        shared_clusters(&r, x, &pair(EdgeRays::Chain(&two))),
        Err(SharedError::Crossed(1))
    );

    // `e` closed at `a`: `RayRec(b, g) − RayRec(a, g)` is one record less
    // itself.
    let mut r = Reads::default();
    let a = r.vert(A, 0, [0.0; 3]);
    let (g0, g1) = (r.vert(B, 2, [0.0; 3]), r.vert(B, 3, [4.0, 1.0, 0.0]));
    let x = XKey {
        e: r.edge(A, 1, a, a),
        g: r.edge(B, 2, g0, g1),
    };
    let loop_spans = [
        [
            [0.0, 0.0, 0.0, 1.0],
            [2.0, 2.0, 0.0, 1.0],
            [4.0, 0.0, 0.0, 1.0],
        ],
        [
            [4.0, 0.0, 0.0, 1.0],
            [2.0, -2.0, 0.0, 1.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    ];
    for v in [g0, g1] {
        r.feet.insert((Point::vert(B, v), x.e), Feet::default());
    }
    let closed = XPair {
        e: XCurve::Spans(&loop_spans),
        g: XCurve::Spans(&spans),
        terms: XTerms {
            a_g: EdgeRays::Chain(&one),
            b_g: EdgeRays::Chain(&one),
            g0_e: EdgeRays::Chain(&ChainRec::default()),
            g1_e: EdgeRays::Chain(&ChainRec::default()),
        },
        shared: true,
    };
    assert_eq!(shared_clusters(&r, x, &closed), Ok(XRec::default()));
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
