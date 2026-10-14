//! Height records: a key's value decided once for every anchor reading
//! it, so the faces at a shared vertex read one `VV(v, w)` and agree
//! whenever it decides; at a tie each anchor's own tangent plane decides;
//! the band class is the key's, geometric only when every anchor is; a
//! curved key's level is the one its anchors' orders start at; and the
//! records replay.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU32, Ordering as AtOrd};

use glam::DVec3;

use super::*;
use crate::num::exact::Num;
use crate::perturb::{Moving, PlaneHeight, PointHeight};
use crate::records::shadow::LevelWork;

/// A pass deciding in key order on one thread, counting items and work.
#[derive(Default)]
struct Seq {
    decided: usize,
    work: Cost,
}

impl Pass for Seq {
    type Error = Fail;

    fn run<K: Sync, V: Send>(
        &mut self,
        keys: &[Fresh<K>],
        decide: &(dyn Fn(&Fresh<K>) -> Decided<V> + Sync),
    ) -> Result<Vec<V>, Fail> {
        keys.iter()
            .map(|k| {
                self.decided += 1;
                let (v, w) = decide(k);
                self.work = self.work.and(w);
                v
            })
            .collect()
    }
}

/// The flat predicates the test geometry gives.
#[derive(Clone, Debug)]
enum Flat {
    Point(PointHeight),
    Plane(PlaneHeight),
}

impl Pred for Flat {
    fn eval<N: Num>(&self) -> N {
        match self {
            Flat::Point(p) => p.eval(),
            Flat::Plane(p) => p.eval(),
        }
    }
}

/// Points of both operands (`A`'s moving by `d`), planes of `B`'s faces
/// through the origin, curved point–face keys with a value formed from
/// the `fail`-th level on, and the anchors that are geometric.
#[derive(Default)]
struct Geo {
    at: BTreeMap<Point, Moving>,
    planes: BTreeMap<FaceId, DVec3>,
    curved: Option<(f64, u32)>,
    rows: Option<[f64; ORDERS]>,
    geometric: BTreeSet<Anchor>,
    calls: AtomicU32,
}

impl HeightGeometry for Geo {
    type Flat = Flat;

    fn value(&self, h: HKey) -> KeyValue<Flat> {
        match h {
            HKey::VV { a, b } => KeyValue::Flat(Flat::Point(PointHeight {
                q: self.at[&Point { operand: A, q: a }].clone(),
                w: self.at[&Point { operand: B, q: b }].clone(),
            })),
            _ => KeyValue::Curved,
        }
    }

    fn curved_value<S: Curved>(&self, _: HKey, like: &S) -> Option<S> {
        let (v, from) = self.curved.expect("a curved key");
        let n = self.calls.fetch_add(1, AtOrd::Relaxed);
        (n >= from).then(|| S::from_f64(v, like))
    }

    fn facing_only<S: Curved>(&self, _: HKey, _: &S) -> bool {
        // A curved value never formed stands for an uncertified facing.
        true
    }

    fn row(&self, k: HeightKey) -> AnchorRow<Flat> {
        match (k.h, k.anchor) {
            (HKey::VV { a, .. }, Anchor::Face(B, f)) => {
                let normal = self.planes[&f];
                AnchorRow::Flat(Flat::Plane(PlaneHeight {
                    normal,
                    offset: 0.0,
                    rates: [crate::perturb::Rate::ZERO; ORDERS],
                    q: self.at[&Point { operand: A, q: a }].clone(),
                }))
            }
            _ => AnchorRow::Curved,
        }
    }

    fn curved_row<S: Curved>(&self, _: HeightKey, like: &S) -> Option<[S; ORDERS]> {
        self.rows.map(|r| r.map(|x| S::from_f64(x, like)))
    }

    fn geometric(&self, k: HeightKey) -> bool {
        self.geometric.contains(&k.anchor)
    }
}

fn band() -> Band {
    Band::curved(0.0, 1.0 / 1_073_741_824.0).expect("a band")
}

const V: VertId = VertId(7);
const W: VertId = VertId(3);
const F1: FaceId = FaceId(1);
const F2: FaceId = FaceId(2);

fn pv() -> Point {
    Point::vert(A, V)
}

/// Vertex `v` of `A` at `v_at` moving by `d`, vertex `w` of `B` at the
/// origin with faces `f1`, `f2` of `B` through it, tangent planes of
/// normals `(1, 0, 1)` and `(−1, 0, 1)`: a roof ridge along `y`.
fn ridge(v_at: DVec3, d: DVec3) -> Geo {
    let mut g = Geo::default();
    g.at.insert(pv(), Moving::of_a(v_at, Sign::Pos, d));
    g.at.insert(Point::vert(B, W), Moving::fixed(DVec3::ZERO));
    g.planes.insert(F1, DVec3::new(1.0, 0.0, 1.0));
    g.planes.insert(F2, DVec3::new(-1.0, 0.0, 1.0));
    g
}

/// `v`'s keys against the layers of `f1` and `f2` that hold `w`: one
/// layer each, its feature `w` (near `v`), so each reads `VV(v, w)`.
fn ridge_keys() -> [Oriented<HKey>; 2] {
    let mut r = Reads::default();
    r.vert(B, W.0, [0.0, 0.0, 0.0]);
    r.near.insert((pv(), W), true);
    let mut t = LayerTable::new();
    for f in [F1, F2] {
        let mut p = LayerPieces::new();
        p.piece([Feature::Vert(W)], None);
        t.insert(pv(), f, p.layers(&r, pv(), f).unwrap());
    }
    [F1, F2].map(|f| height_key(&r, &t, pv(), Target::Layer(f, LayerId(0))).unwrap())
}

fn at(h: HKey, f: FaceId) -> HeightKey {
    HeightKey {
        h,
        anchor: Anchor::Face(B, f),
    }
}

#[test]
fn faces_at_a_shared_vertex_read_one_value() {
    let [k1, k2] = ridge_keys();
    assert_eq!(k1, k2);
    assert_eq!(k1, HKey::vv(pv(), W));
    // `v` below `w`: the value decides, once, for both faces.
    let g = ridge(DVec3::new(0.0, 0.0, -1.0), DVec3::X);
    let mut hs = Heights::new();
    let mut pass = Seq::default();
    hs.fill([at(k1.key, F1), at(k1.key, F2)], &g, band(), &mut pass)
        .unwrap();
    assert_eq!(pass.decided, 3, "one key, two anchors");
    assert_eq!(hs.key_table().len(), 1);
    let rec = HeightRec {
        above: Sign::Pos,
        by_orders: false,
        fold: false,
    };
    for f in [F1, F2] {
        assert_eq!(hs.get(k1, Anchor::Face(B, f)), Some(rec));
    }
    assert_eq!(
        hs.key(k1.key),
        Some(KeyRec {
            value: Some(Sign::Pos),
            kind: BandKind::Flat,
            level: 0
        })
    );
    // Asked again, nothing is decided.
    hs.fill([at(k1.key, F2)], &g, band(), &mut pass).unwrap();
    assert_eq!(pass.decided, 3);
    hs.replay(&g, band()).unwrap();
}

#[test]
fn at_a_tie_each_face_s_tangent_plane_decides() {
    let [k, _] = ridge_keys();
    // `v` on `w`, moving along `+x`: under the `f2` slope, over `f1`'s
    // (knife wedge): the anchors differ, the key's tie read once.
    let g = ridge(DVec3::ZERO, DVec3::X);
    let mut hs = Heights::new();
    let mut pass = Seq::default();
    hs.fill([at(k.key, F1), at(k.key, F2)], &g, band(), &mut pass)
        .unwrap();
    assert_eq!(pass.decided, 3);
    assert_eq!(hs.key(k.key).unwrap().value, None);
    let above = |f| hs.get(k, Anchor::Face(B, f)).unwrap();
    assert_eq!(
        above(F1),
        HeightRec {
            above: Sign::Neg,
            by_orders: true,
            fold: false,
        }
    );
    assert_eq!(above(F2).above, Sign::Pos);
    // Moving up, `v` is above both.
    let g = ridge(DVec3::ZERO, DVec3::Z);
    let mut hs = Heights::new();
    hs.fill([at(k.key, F1), at(k.key, F2)], &g, band(), &mut pass)
        .unwrap();
    for f in [F1, F2] {
        assert_eq!(hs.get(k, Anchor::Face(B, f)).unwrap().above, Sign::Neg);
    }
    hs.replay(&g, band()).unwrap();
}

#[test]
fn the_b_point_reads_the_stored_value_negated() {
    let fa = FaceId(4);
    let ka = HKey::vv(pv(), W);
    let kb = HKey::vv(Point::vert(B, W), V);
    assert_eq!(ka.key, kb.key);
    assert!(kb.swapped);
    // `w` above `v`.
    let g = ridge(DVec3::new(0.0, 0.0, -1.0), DVec3::X);
    let mut hs = Heights::new();
    let anchor = Anchor::Face(A, fa);
    hs.fill(
        [HeightKey { h: kb.key, anchor }],
        &g,
        band(),
        &mut Seq::default(),
    )
    .unwrap();
    // For `w` asking, `A`'s face at `v` is below.
    assert_eq!(hs.get(kb, anchor).unwrap().above, Sign::Neg);
    assert_eq!(hs.get(ka, anchor).unwrap().above, Sign::Pos);
}

fn face_key() -> HKey {
    HKey::Face {
        q: pv(),
        f: F1,
        layer: LayerId(0),
    }
}

#[test]
fn a_curved_key_is_decided_at_one_level_its_anchors_start_at() {
    let h = face_key();
    // The value forms from the third level on: a tie there.
    let g = Geo {
        curved: Some((0.0, 2)),
        rows: Some([-0.25, 0.0, 0.0, 0.0]),
        ..Geo::default()
    };
    let mut hs = Heights::new();
    let mut pass = Seq::default();
    hs.fill([at(h, F1)], &g, band(), &mut pass).unwrap();
    assert_eq!(
        hs.key(h),
        Some(KeyRec {
            value: None,
            kind: BandKind::Curved,
            level: 2
        })
    );
    let ev = |w: &LevelWork| w.evals;
    assert_eq!(pass.work.levels.map(|w| ev(&w))[..3], [1, 1, 2]);
    let own = Oriented {
        key: h,
        swapped: false,
    };
    assert_eq!(
        hs.get(own, Anchor::Face(B, F1)),
        Some(HeightRec {
            above: Sign::Neg,
            by_orders: true,
            fold: false,
        })
    );
    // Beyond the band, the value decides.
    let g = Geo {
        curved: Some((0.5, 0)),
        ..Geo::default()
    };
    let mut hs = Heights::new();
    hs.fill([at(h, F1)], &g, band(), &mut Seq::default())
        .unwrap();
    assert_eq!(hs.get(own, Anchor::Face(B, F1)).unwrap().above, Sign::Pos);
    // Never formed: a point–face value is a tie at the last level, so
    // the anchor's orders decide; with none either (and no fold rule
    // here), the row's escalation stands past the last level.
    let g = Geo {
        curved: Some((0.0, u32::MAX)),
        ..Geo::default()
    };
    let mut hs = Heights::new();
    let got = hs.fill([at(h, F1)], &g, band(), &mut Seq::default());
    assert_eq!(got, Err(HeightError::Pass(Fail::Escalate { order: 1 })));
    // With orders, they decide.
    let g = Geo {
        curved: Some((0.0, u32::MAX)),
        rows: Some([0.25, 0.0, 0.0, 0.0]),
        ..Geo::default()
    };
    let mut hs = Heights::new();
    hs.fill([at(h, F1)], &g, band(), &mut Seq::default())
        .unwrap();
    let last = u8::try_from(crate::records::shadow::LEVELS - 1).unwrap();
    assert_eq!(hs.key(h).map(|r| (r.value, r.level)), Some((None, last)));
    assert_eq!(hs.get(own, Anchor::Face(B, F1)).unwrap().above, Sign::Pos);
}

#[test]
fn a_key_is_geometric_only_when_every_anchor_is() {
    let k = HKey::vv(pv(), W).key;
    let mut g = ridge(DVec3::new(0.0, 0.0, -1.0), DVec3::X);
    g.geometric.insert(Anchor::Face(B, F1));
    let mut hs = Heights::new();
    hs.fill([at(k, F1), at(k, F2)], &g, band(), &mut Seq::default())
        .unwrap();
    assert_eq!(hs.key(k).unwrap().kind, BandKind::Flat);
    // Alone, the geometric anchor makes it geometric; an anchor that
    // isn't, asked later, would change its band.
    let mut hs = Heights::new();
    hs.fill([at(k, F1)], &g, band(), &mut Seq::default())
        .unwrap();
    assert_eq!(hs.key(k).unwrap().kind, BandKind::Geometric);
    let got = hs.fill([at(k, F2)], &g, band(), &mut Seq::default());
    assert_eq!(got, Err(HeightError::Band(k)));
    // A geometric value is never a tie.
    let mut g = ridge(DVec3::ZERO, DVec3::X);
    g.geometric.insert(Anchor::Face(B, F1));
    let got = Heights::new().fill([at(k, F1)], &g, band(), &mut Seq::default());
    assert_eq!(got, Err(HeightError::Pass(Fail::Escalate { order: 0 })));
}

#[test]
fn a_face_key_read_at_another_face_is_refused() {
    let g = Geo {
        curved: Some((0.0, 0)),
        rows: Some([1.0, 0.0, 0.0, 0.0]),
        ..Geo::default()
    };
    let got = Heights::new().fill([at(face_key(), F2)], &g, band(), &mut Seq::default());
    assert!(
        matches!(got, Err(HeightError::Pass(Fail::Internal { .. }))),
        "{got:?}"
    );
}

#[test]
fn a_lone_core_layer_needs_its_key() {
    let r = Reads::default();
    let mut p = LayerPieces::new();
    p.piece([], None);
    assert_eq!(
        p.layers(&r, pv(), F1),
        Err(KeyError::NoCore { q: pv(), f: F1 })
    );
}
