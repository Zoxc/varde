//! Height keys and layers on hand-made records: each target's key (a
//! vertex, a cut point's own crossing, a foot with and without a near end,
//! a layer's feature or none), read from either operand; the feature in
//! geometric order whatever the ids, the edges' stored directions and the
//! order the pieces come in; one layer, id and key however its pieces are
//! cut and joined; and the errors for records, feet and layers that
//! aren't there.

use std::collections::BTreeMap;

use smallvec::smallvec;

use super::*;
use crate::order::{EdgeKey, VertKey};
use crate::records::feet::{EndKind, Foot, FootEnd};
use crate::records::keys::{Query, XClusterIx};

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

#[test]
fn a_vertex_target_is_the_pair_whichever_point_asks() {
    let r = Reads::default();
    let t = LayerTable::new();
    let a = Point::vert(A, VertId(1));
    let b = Point::vert(B, VertId(2));
    let ka = height_key(&r, &t, a, Target::Vert(VertId(2))).unwrap();
    let kb = height_key(&r, &t, b, Target::Vert(VertId(1))).unwrap();
    assert_eq!(ka.key, kb.key);
    assert!(!ka.swapped && kb.swapped);
}

#[test]
fn a_cut_point_against_its_own_edge_reads_the_crossing() {
    let r = Reads::default();
    let t = LayerTable::new();
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
    let ka = height_key(&r, &t, qa, Target::Edge(EdgeId(7), FootId(0))).unwrap();
    let kb = height_key(&r, &t, qb, Target::Edge(EdgeId(5), FootId(0))).unwrap();
    assert_eq!((ka.key, ka.swapped), (x, false));
    assert_eq!((kb.key, kb.swapped), (x, true));
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
    (r, g, [v0, v1])
}

#[test]
fn a_foot_shares_the_height_of_its_first_near_end() {
    let t = LayerTable::new();
    // One foot over the whole edge: both ends in it.
    for lesser_first in [true, false] {
        let (mut r, g, [v0, v1]) = edge_reads(lesser_first, &[foot(0.0, 1.0)]);
        r.near.insert((qa(), v0), true);
        r.near.insert((qa(), v1), true);
        let k = height_key(&r, &t, qa(), Target::Edge(g, FootId(0))).unwrap();
        // The geometrically lesser end, never the lower id.
        let first = if lesser_first { v0 } else { v1 };
        assert_eq!(k, vv(qa(), first), "lesser first: {lesser_first}");
        // Only the other end near: that one.
        let other = if lesser_first { v1 } else { v0 };
        r.near.insert((qa(), first), false);
        let k = height_key(&r, &t, qa(), Target::Edge(g, FootId(0))).unwrap();
        assert_eq!(k, vv(qa(), other));
        // Neither near: the foot's own point–curve height.
        r.near.insert((qa(), other), false);
        let k = height_key(&r, &t, qa(), Target::Edge(g, FootId(0))).unwrap();
        let own = HKey::Curve {
            q: qa(),
            g,
            foot: FootId(0),
        };
        assert_eq!((k.key, k.swapped), (own, false));
    }
}

#[test]
fn an_inner_foot_reads_no_vertex() {
    let t = LayerTable::new();
    let (mut r, g, [v0, v1]) = edge_reads(true, &[foot(0.0, 0.2), foot(0.4, 0.6)]);
    r.near.insert((qa(), v0), true);
    r.near.insert((qa(), v1), true);
    let k = height_key(&r, &t, qa(), Target::Edge(g, FootId(1))).unwrap();
    assert_eq!(
        k.key,
        HKey::Curve {
            q: qa(),
            g,
            foot: FootId(1)
        }
    );
    let k = height_key(&r, &t, qa(), Target::Edge(g, FootId(0))).unwrap();
    assert_eq!(k, vv(qa(), v0));
}

#[test]
fn missing_records_feet_and_layers_are_errors() {
    let t = LayerTable::new();
    let (r, g, [v0, _]) = edge_reads(true, &[foot(0.0, 0.5)]);
    assert_eq!(
        height_key(&r, &t, qa(), Target::Edge(g, FootId(0))),
        Err(KeyError::Missing(RecordKey::Vv(VvKey::of(qa(), v0).key)))
    );
    assert_eq!(
        height_key(&r, &t, qa(), Target::Edge(g, FootId(1))),
        Err(KeyError::NoFoot {
            q: qa(),
            g,
            foot: FootId(1)
        })
    );
    let h = EdgeId(77);
    assert_eq!(
        height_key(&r, &t, qa(), Target::Edge(h, FootId(0))),
        Err(KeyError::Missing(RecordKey::Ray(RayKey { q: qa(), g: h })))
    );
    let f = FaceId(4);
    assert_eq!(
        height_key(&r, &t, qa(), Target::Layer(f, LayerId(0))),
        Err(KeyError::NoLayer {
            q: qa(),
            f,
            layer: LayerId(0)
        })
    );
}

/// A face of `B` at `qa`: a square `v1 … v4` at heights, with its four
/// edges (ids chosen against the geometry), each foot over the whole edge
/// or an inner stretch.
struct Square {
    r: Reads,
    v: [VertId; 4],
    e: [EdgeId; 4],
}

/// The square with vertex ids `ids` at its corners in counter-clockwise
/// order from `(0, 0)`, and edge ids `eids`.
fn square(ids: [u32; 4], eids: [u32; 4]) -> Square {
    let mut r = Reads::default();
    let at = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let v: [VertId; 4] = std::array::from_fn(|i| r.vert(B, ids[i], at[i]));
    let e: [EdgeId; 4] = std::array::from_fn(|i| r.edge(B, eids[i], v[i], v[(i + 1) % 4]));
    Square { r, v, e }
}

#[test]
fn the_feature_is_the_first_near_vertex_then_the_first_foot() {
    let f = FaceId(1);
    for (ids, eids) in [
        ([1, 2, 3, 4], [10, 11, 12, 13]),
        ([4, 3, 2, 1], [13, 12, 11, 10]),
        ([2, 4, 1, 3], [12, 10, 13, 11]),
    ] {
        let Square { mut r, v, e } = square(ids, eids);
        // An inner foot on every edge, middle third.
        for g in e {
            r.feet.insert(
                (qa(), g),
                Feet {
                    feet: smallvec![foot(0.3, 0.6)],
                },
            );
        }
        for w in v {
            r.near.insert((qa(), w), true);
        }
        let all: Vec<Feature> = v
            .iter()
            .map(|&w| Feature::Vert(w))
            .chain(e.iter().map(|&g| Feature::Foot(g, FootId(0))))
            .collect();
        // The first vertex in the order: at (0, 0), whatever its id.
        let mut p = LayerPieces::new();
        p.piece(all.iter().rev().copied(), None);
        let ls = p.layers(&r, qa(), f).unwrap();
        assert_eq!(ls.len(), 1);
        assert_eq!(
            ls.get(LayerId(0)).unwrap().feature,
            Some(Feature::Vert(v[0]))
        );
        // Not near: the next one, at (0, 1) (lexicographic `(x, y, z)`).
        r.near.insert((qa(), v[0]), false);
        let ls = p.layers(&r, qa(), f).unwrap();
        assert_eq!(
            ls.get(LayerId(0)).unwrap().feature,
            Some(Feature::Vert(v[3]))
        );
        // No vertex near: the first edge's foot, the edge from (0, 0) to
        // (0, 1) (`e[3]`, its ends the least pair).
        for w in v {
            r.near.insert((qa(), w), false);
        }
        let ls = p.layers(&r, qa(), f).unwrap();
        let x = Some(Feature::Foot(e[3], FootId(0)));
        assert_eq!(ls.get(LayerId(0)).unwrap().feature, x, "ids {ids:?}");
        // The key: the foot's own, its edge's ends being far.
        let mut t = LayerTable::new();
        t.insert(qa(), f, ls);
        let k = height_key(&r, &t, qa(), Target::Layer(f, LayerId(0))).unwrap();
        assert_eq!(
            k.key,
            HKey::Curve {
                q: qa(),
                g: e[3],
                foot: FootId(0)
            }
        );
    }
}

#[test]
fn feet_on_one_edge_go_from_its_lesser_end() {
    let f = FaceId(1);
    for lesser_first in [true, false] {
        let (r, g, _) = edge_reads(lesser_first, &[foot(0.1, 0.2), foot(0.7, 0.8)]);
        let mut p = LayerPieces::new();
        p.piece(
            [Feature::Foot(g, FootId(0)), Feature::Foot(g, FootId(1))],
            None,
        );
        let ls = p.layers(&r, qa(), f).unwrap();
        // From the lesser end: the foot nearer it.
        let first = if lesser_first { FootId(0) } else { FootId(1) };
        assert_eq!(
            ls.get(LayerId(0)).unwrap().feature,
            Some(Feature::Foot(g, first))
        );
    }
}

#[test]
fn a_layer_whose_feature_is_a_vertex_shares_its_vertex_key() {
    let f = FaceId(1);
    let Square { mut r, v, e } = square([1, 2, 3, 4], [10, 11, 12, 13]);
    r.near.insert((qa(), v[2]), true);
    r.feet.insert(
        (qa(), e[1]),
        Feet {
            feet: smallvec![foot(0.5, 1.0)],
        },
    );
    let mut p = LayerPieces::new();
    p.piece([Feature::Foot(e[1], FootId(0)), Feature::Vert(v[2])], None);
    let mut t = LayerTable::new();
    t.insert(qa(), f, p.layers(&r, qa(), f).unwrap());
    let layer = height_key(&r, &t, qa(), Target::Layer(f, LayerId(0))).unwrap();
    let vert = height_key(&r, &t, qa(), Target::Vert(v[2])).unwrap();
    let edge = height_key(&r, &t, qa(), Target::Edge(e[1], FootId(0))).unwrap();
    assert_eq!(layer, vert);
    assert_eq!(edge, vert);
}

/// One face with three layers at `qa`: one holding a near vertex, one a
/// foot only, one core; given as pieces cut and ordered in different
/// ways, they come out the same.
#[test]
fn layers_are_partition_free() {
    let f = FaceId(1);
    let Square { mut r, v, e } = square([1, 2, 3, 4], [10, 11, 12, 13]);
    r.near.insert((qa(), v[1]), true);
    r.near.insert((qa(), v[3]), false);
    r.feet.insert(
        (qa(), e[2]),
        Feet {
            feet: smallvec![foot(0.4, 0.5)],
        },
    );
    let hit = Pos::new([0.5, 0.5, 3.0]).unwrap();
    // Layer contents: [v1 and v3] (v3 not near), [the foot on e2], [core].
    let layer_a = [Feature::Vert(v[3]), Feature::Vert(v[1])];
    let layer_b = [Feature::Foot(e[2], FootId(0))];
    let want = {
        let mut p = LayerPieces::new();
        p.piece(layer_a, None);
        p.piece(layer_b, None);
        p.piece([], Some(hit));
        p.layers(&r, qa(), f).unwrap()
    };
    assert_eq!(want.len(), 3);
    assert_eq!(
        want.get(LayerId(0)).unwrap().feature,
        Some(Feature::Vert(v[1]))
    );
    assert_eq!(
        want.get(LayerId(1)).unwrap().feature,
        Some(Feature::Foot(e[2], FootId(0)))
    );
    assert_eq!(want.get(LayerId(2)).unwrap().core, Some(hit));
    // Cut finer, in other orders, joined by meetings or shared features.
    for seed in 0..32u64 {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let mut p = LayerPieces::new();
        let mut ids: Vec<(usize, usize)> = Vec::new();
        let mut parts: Vec<(usize, Vec<Feature>)> = vec![
            (0, vec![layer_a[0]]),
            (0, vec![layer_a[1]]),
            (0, vec![]),
            (1, vec![layer_b[0]]),
            (1, vec![layer_b[0]]),
            (1, vec![]),
            (2, vec![]),
            (2, vec![]),
        ];
        for i in (1..parts.len()).rev() {
            let j = (rng.next_u64() % (i as u64 + 1)) as usize;
            parts.swap(i, j);
        }
        for (layer, xs) in parts {
            let core = (layer == 2).then_some(hit);
            ids.push((layer, p.piece(xs, core)));
        }
        // Join each piece to the previous one of its layer, unless they
        // share a feature, which joins them anyway.
        for layer in 0..3 {
            let of: Vec<usize> = ids.iter().filter(|x| x.0 == layer).map(|x| x.1).collect();
            for w in of.windows(2) {
                p.join(w[1], w[0]);
            }
        }
        assert_eq!(p.layers(&r, qa(), f).unwrap(), want, "seed {seed}");
    }
}

#[test]
fn core_layers_go_by_their_key() {
    let f = FaceId(1);
    let r = Reads::default();
    let lo = Pos::new([0.0, 0.0, 1.0]).unwrap();
    let hi = Pos::new([0.0, 0.0, 2.0]).unwrap();
    for order in [[lo, hi], [hi, lo]] {
        let mut p = LayerPieces::new();
        for c in order {
            p.piece([], Some(c));
        }
        let ls = p.layers(&r, qa(), f).unwrap();
        assert_eq!(ls.get(LayerId(0)).unwrap().core, Some(lo));
        assert_eq!(ls.get(LayerId(1)).unwrap().core, Some(hi));
        let mut t = LayerTable::new();
        t.insert(qa(), f, ls);
        let k = height_key(&r, &t, qa(), Target::Layer(f, LayerId(1))).unwrap();
        assert_eq!(
            (k.key, k.swapped),
            (
                HKey::Face {
                    q: qa(),
                    f,
                    layer: LayerId(1)
                },
                false
            )
        );
    }
    // Two core layers with one key can't be told apart.
    let mut p = LayerPieces::new();
    p.piece([], Some(lo));
    p.piece([], Some(lo));
    assert_eq!(p.layers(&r, qa(), f), Err(KeyError::CoreTie { q: qa(), f }));
}

#[test]
fn layers_find_the_one_holding_a_feature() {
    let f = FaceId(1);
    let Square { mut r, v, .. } = square([1, 2, 3, 4], [10, 11, 12, 13]);
    for w in v {
        r.near.insert((qa(), w), true);
    }
    let mut p = LayerPieces::new();
    p.piece([Feature::Vert(v[2])], None);
    p.piece([Feature::Vert(v[0])], None);
    let ls = p.layers(&r, qa(), f).unwrap();
    assert_eq!(ls.holding(Feature::Vert(v[0])), Some(LayerId(0)));
    assert_eq!(ls.holding(Feature::Vert(v[2])), Some(LayerId(1)));
    assert_eq!(ls.holding(Feature::Vert(v[1])), None);
    assert_eq!(ls.iter().count(), 2);
    assert!(!ls.is_empty());
}

/// Two layers whose features are vertices at one place (bitwise-equal)
/// are numbered alike whichever piece comes first.
#[test]
fn layers_tied_in_place_go_by_their_feature_not_the_pieces_order() {
    let f = FaceId(1);
    let mut r = Reads::default();
    let v = r.vert(B, 7, [1.0, 2.0, 3.0]);
    let w = r.vert(B, 5, [1.0, 2.0, 3.0]);
    r.near.insert((qa(), v), true);
    r.near.insert((qa(), w), true);
    let mut ids = Vec::new();
    for order in [[v, w], [w, v]] {
        let mut p = LayerPieces::new();
        for x in order {
            p.piece([Feature::Vert(x)], None);
        }
        let ls = p.layers(&r, qa(), f).unwrap();
        ids.push(ls.holding(Feature::Vert(v)));
    }
    assert_eq!(ids[0], ids[1]);
    assert_eq!(ids[0], Some(LayerId(1)));
}

/// A core layer's key is a point: `−0` and `+0` are one coordinate, so
/// two core layers at hits differing only in a zero's sign tie, and the
/// key a layer keeps doesn't depend on which of its pieces held a `−0`.
#[test]
fn core_keys_read_minus_zero_as_zero() {
    let f = FaceId(1);
    let r = Reads::default();
    let neg = Pos::new([-0.0, 0.0, 1.0]).unwrap();
    let pos = Pos::new([0.0, 0.0, 1.0]).unwrap();
    let mut p = LayerPieces::new();
    p.piece([], Some(neg));
    p.piece([], Some(pos));
    assert_eq!(p.layers(&r, qa(), f), Err(KeyError::CoreTie { q: qa(), f }));
    for cores in [[neg, pos], [pos, pos], [neg, neg]] {
        let mut p = LayerPieces::new();
        let a = p.piece([], Some(cores[0]));
        let b = p.piece([], Some(cores[1]));
        p.join(a, b);
        let ls = p.layers(&r, qa(), f).unwrap();
        let core = ls.get(LayerId(0)).unwrap().core.unwrap();
        assert_eq!(core.get().map(f64::to_bits), pos.get().map(f64::to_bits));
    }
}

mod records;
