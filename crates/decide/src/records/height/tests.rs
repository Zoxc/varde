//! Height keys on hand-made records: each target's key (a vertex, a cut
//! point's own crossing, a foot at or away from an end, a column hit with
//! and without an exact incidence), read from either operand; the edge
//! rule's key (the lobe fallback's) at the first near end in geometric
//! order whatever the ids and the edges' stored directions; and the
//! errors for records and feet that aren't there.

use std::collections::BTreeMap;

use super::*;
use crate::ids::{CoedgeId, FaceId};
use crate::order::{EdgeKey, VertKey};
use crate::records::feet::{EndKind, Foot, FootEnd};
use crate::records::keys::{HitIx, Query, Sheet, XClusterIx};

use Operand::{A, B};

/// Records made by hand: positions give the order (no face keys), `near`
/// and feet are as stored.
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
}

impl HeightReads for Reads {
    fn vv(&self, q: Point, w: VertId) -> Option<VvRec> {
        self.near.get(&(q, w)).map(|&near| VvRec {
            place: crate::records::shadow::SideAhead {
                side: crate::num::decide::Sign::Pos,
                ahead: crate::num::decide::Sign::Pos,
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

fn end(t: f64, kind: EndKind) -> FootEnd {
    FootEnd {
        span: 0,
        t: [t, t],
        kind,
    }
}

/// A foot over `[lo, hi]` of a straight edge, holding an end where it
/// reaches `0` or `1`.
fn foot(lo: f64, hi: f64) -> Foot {
    let kind = |t: f64| {
        if t == 0.0 || t == 1.0 {
            EndKind::Vertex
        } else {
            EndKind::Crossing
        }
    };
    Foot {
        lo: end(lo, kind(lo)),
        hi: end(hi, kind(hi)),
        wraps: false,
        hull: false,
    }
}

fn qa() -> Point {
    Point::vert(A, VertId(100))
}

fn vv(q: Point, w: VertId) -> Oriented<HKey> {
    HKey::vv(q, w)
}

fn qa_at(r: &mut Reads, at: [f64; 3]) -> Point {
    r.vert(A, 100, at);
    qa()
}

#[test]
fn a_vertex_target_is_the_pair_whichever_point_asks() {
    let r = Reads::default();
    let a = Point::vert(A, VertId(1));
    let b = Point::vert(B, VertId(2));
    let ka = height_key(&r, a, Target::Vert(VertId(2))).unwrap();
    let kb = height_key(&r, b, Target::Vert(VertId(1))).unwrap();
    assert_eq!(ka.key, kb.key);
    assert!(!ka.swapped && kb.swapped);
}

#[test]
fn a_cut_point_against_its_own_edge_reads_the_crossing() {
    let r = Reads::default();
    let c = XClusterIx(3);
    let qa = Point {
        operand: A,
        q: Query::Cut(EdgeId(5), EdgeId(7), c),
    };
    let qb = Point {
        operand: B,
        q: Query::Cut(EdgeId(7), EdgeId(5), c),
    };
    let x = HKey::X {
        e: EdgeId(5),
        g: EdgeId(7),
        c,
    };
    // No feet asked for: the crossing is the key.
    let ka = height_key(&r, qa, Target::Edge(EdgeId(7), FootId(0))).unwrap();
    let kb = height_key(&r, qb, Target::Edge(EdgeId(5), FootId(0))).unwrap();
    assert_eq!((ka.key, ka.swapped), (x, false));
    assert_eq!((kb.key, kb.swapped), (x, true));
    // The edge rule reads it too.
    let kr = edge_rule_key(&r, qa, EdgeId(7), FootId(0)).unwrap();
    assert_eq!((kr.key, kr.swapped), (x, false));
}

/// An edge `g` of `B` from `v0` (id 1) to `v1` (id 2), its feet for `qa`.
fn edge_reads(lesser_first: bool, feet: &[Foot]) -> (Reads, EdgeId, [VertId; 2]) {
    let mut r = Reads::default();
    let (p0, p1) = ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    let (p0, p1) = if lesser_first { (p0, p1) } else { (p1, p0) };
    let v0 = r.vert(B, 1, p0);
    let v1 = r.vert(B, 2, p1);
    let g = r.edge(B, 9, v0, v1);
    r.feet.insert(
        (qa(), g),
        Feet {
            feet: feet.iter().copied().collect(),
        },
    );
    r.vert(A, 100, [0.25, 1e-9, 0.0]);
    (r, g, [v0, v1])
}

#[test]
fn an_edge_reads_a_vertex_only_where_the_point_is_exactly_at_it() {
    for lesser_first in [true, false] {
        let (mut r, g, [v0, v1]) = edge_reads(lesser_first, &[foot(0.0, 1.0)]);
        // Both ends near the point, neither at it: the foot's own key.
        r.near.insert((qa(), v0), true);
        r.near.insert((qa(), v1), true);
        let k = height_key(&r, qa(), Target::Edge(g, FootId(0))).unwrap();
        let own = HKey::Curve {
            q: qa(),
            g,
            foot: FootId(0),
        };
        assert_eq!((k.key, k.swapped), (own, false));
        // The point exactly at an end: that end's key.
        let at = if lesser_first {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 0.0, 0.0]
        };
        qa_at(&mut r, at);
        let k = height_key(&r, qa(), Target::Edge(g, FootId(0))).unwrap();
        assert_eq!(k, vv(qa(), v1), "lesser first: {lesser_first}");
        // `−0` is `+0`.
        qa_at(&mut r, at.map(|x| if x == 0.0 { -0.0 } else { x }));
        let k = height_key(&r, qa(), Target::Edge(g, FootId(0))).unwrap();
        assert_eq!(k, vv(qa(), v1));
    }
}

#[test]
fn the_edge_rule_shares_the_height_of_its_first_near_end() {
    // One foot over the whole edge: both ends in it.
    for lesser_first in [true, false] {
        let (mut r, g, [v0, v1]) = edge_reads(lesser_first, &[foot(0.0, 1.0)]);
        r.near.insert((qa(), v0), true);
        r.near.insert((qa(), v1), true);
        let k = edge_rule_key(&r, qa(), g, FootId(0)).unwrap();
        // The geometrically lesser end, never the lower id.
        let first = if lesser_first { v0 } else { v1 };
        assert_eq!(k, vv(qa(), first), "lesser first: {lesser_first}");
        // Only the other end near: that one.
        let other = if lesser_first { v1 } else { v0 };
        r.near.insert((qa(), first), false);
        let k = edge_rule_key(&r, qa(), g, FootId(0)).unwrap();
        assert_eq!(k, vv(qa(), other));
        // Neither near: the foot's own point–curve height.
        r.near.insert((qa(), other), false);
        let k = edge_rule_key(&r, qa(), g, FootId(0)).unwrap();
        let own = HKey::Curve {
            q: qa(),
            g,
            foot: FootId(0),
        };
        assert_eq!((k.key, k.swapped), (own, false));
    }
}

#[test]
fn the_edge_rule_at_an_inner_foot_reads_no_vertex() {
    let (mut r, g, [v0, v1]) = edge_reads(true, &[foot(0.0, 0.2), foot(0.4, 0.6)]);
    r.near.insert((qa(), v0), true);
    r.near.insert((qa(), v1), true);
    let k = edge_rule_key(&r, qa(), g, FootId(1)).unwrap();
    assert_eq!(
        k.key,
        HKey::Curve {
            q: qa(),
            g,
            foot: FootId(1)
        }
    );
    let k = edge_rule_key(&r, qa(), g, FootId(0)).unwrap();
    assert_eq!(k, vv(qa(), v0));
}

/// A hit reads its own key whatever lies near it: no foot or near vertex
/// keys a height away from exact incidence; at an incidence the vertex's
/// or edge's key.
#[test]
fn a_hit_reads_its_own_key_but_at_an_exact_incidence() {
    let (mut r, g, [v0, v1]) = edge_reads(true, &[foot(0.0, 1.0)]);
    r.near.insert((qa(), v0), true);
    r.near.insert((qa(), v1), true);
    let f = FaceId(4);
    let sheets = [Sheet::Tile([0, 0]), Sheet::Ribbon(CoedgeId(3), 1)];
    for sheet in sheets {
        let i = HitIx(1);
        let t = |inc| Target::Hit { f, sheet, i, inc };
        let k = height_key(&r, qa(), t(None)).unwrap();
        assert_eq!(
            (k.key, k.swapped),
            (
                HKey::Hit {
                    q: qa(),
                    f,
                    sheet,
                    i
                },
                false
            )
        );
        // On the edge's curve, not at its ends: the point–curve key.
        let k = height_key(&r, qa(), t(Some(Incidence::Edge(g, FootId(0))))).unwrap();
        assert_eq!(
            k.key,
            HKey::Curve {
                q: qa(),
                g,
                foot: FootId(0)
            }
        );
        // At a vertex: the pair's key.
        let k = height_key(&r, qa(), t(Some(Incidence::Vert(v1)))).unwrap();
        assert_eq!(k, vv(qa(), v1));
    }
}

#[test]
fn missing_records_and_feet_are_errors() {
    let (r, g, [v0, _]) = edge_reads(true, &[foot(0.0, 0.5)]);
    assert_eq!(
        edge_rule_key(&r, qa(), g, FootId(0)),
        Err(KeyError::Missing(RecordKey::Vv(VvKey::of(qa(), v0).key)))
    );
    for k in [
        height_key(&r, qa(), Target::Edge(g, FootId(1))),
        edge_rule_key(&r, qa(), g, FootId(1)),
    ] {
        assert_eq!(
            k,
            Err(KeyError::NoFoot {
                q: qa(),
                g,
                foot: FootId(1)
            })
        );
    }
    let h = EdgeId(77);
    assert_eq!(
        height_key(&r, qa(), Target::Edge(h, FootId(0))),
        Err(KeyError::Missing(RecordKey::Ray(RayKey { q: qa(), g: h })))
    );
}

mod records;
