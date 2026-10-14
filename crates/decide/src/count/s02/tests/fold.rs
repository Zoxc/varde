//! The fold rule: a core layer on a silhouette inside its face, `q` on
//! it, so the face's height and its facing are certified at no level.
//! Its record is the fold's, the upper sheet's facing from how the face
//! bends, and the layer contributes that facing where the column enters
//! it (some piece of non-zero degree), the same for `s02` and `s20`;
//! where it doesn't, the layer reads no record and counts nothing.

use super::*;
use crate::num::decide::Band;
use crate::num::exact::ORDERS;
use crate::perturb::{Curved, Moving, Rate, Seg};
use crate::records::height::model::{FaceAt, HeightModel, ModelHeights};
use crate::records::keys::{FootId, HKey};
use crate::records::shadow::ShadowGeometry;

/// A curved face whose normal at the hit is horizontal (`r̂`, exactly
/// `⊥ UP`), bending by `bend` along `ê`; the point on it.
struct Silhouette {
    bend: f64,
    /// Whether the column's hit is formed at all.
    hit: bool,
}

impl ShadowGeometry for Silhouette {
    fn point(&self, _: Point) -> Moving {
        Moving::fixed(DVec3::new(1.0, 2.0, 3.0))
    }

    fn near_radius(&self, _: Point) -> f64 {
        0.0
    }

    fn ends(&self, _: Operand, _: EdgeId) -> [VertId; 2] {
        unreachable!("no edges")
    }

    fn line(&self, _: Point, _: EdgeId) -> Seg {
        unreachable!("no edges")
    }
}

impl HeightModel for Silhouette {
    fn plane(&self, _: Operand, _: FaceId) -> Option<(DVec3, f64)> {
        None
    }

    fn plane_rates(&self, _: Point, _: FaceId, _: DVec3) -> [Rate; ORDERS] {
        unreachable!("no plane")
    }

    fn foot_point(&self, _: Point, _: EdgeId, _: FootId) -> Option<Moving> {
        None
    }

    fn curved_target<S: Curved>(&self, _: HKey, like: &S) -> Option<[S; 3]> {
        self.hit
            .then(|| [1.0, 2.0, 3.0].map(|x| S::from_f64(x, like)))
    }

    fn face_at<S: Curved>(&self, _: Point, _: FaceId, _: &[S; 3], like: &S) -> Option<FaceAt<S>> {
        let lit = |v: DVec3| [v.x, v.y, v.z].map(|x| S::from_f64(x, like));
        Some(FaceAt {
            normal: lit(R_HAT),
            rates: std::array::from_fn(|_| lit(DVec3::ZERO)),
            bend: S::from_f64(self.bend, like),
        })
    }
}

/// A face's pieces at `q`: two near pieces of the given degrees, joined,
/// with no feature (one core layer).
fn pieces(q: Point, degrees: [i64; 2]) -> FacePieces {
    let key = Pos::new([1.0, 2.0, 3.0]).unwrap();
    FacePieces {
        point: q,
        face: FaceId(5),
        above: 0,
        below: 0,
        near: degrees
            .into_iter()
            .map(|degree| NearPiece {
                degree,
                features: SmallVec::new(),
                core: Some(key),
                boxes: Vec::new(),
            })
            .collect(),
        joins: vec![(0, 1)],
        pieces: 0,
    }
}

/// The count of `p` with its records filled on `geom`.
fn count(p: &FacePieces, geom: &Silhouette) -> (i64, usize) {
    let order = Order::default();
    let (sh, ft, o) = no_reads(&order);
    let reads = Records {
        shadows: &sh,
        feet: &ft,
        order: o,
    };
    let mut layers = LayerTable::new();
    layers.insert(p.point, p.face, p.layers(&reads).unwrap());
    let wanted = p.wanted(&reads, &layers).unwrap();
    let mut heights = Heights::new();
    let band = Band::curved(0.0, 1.0 / 1_073_741_824.0).unwrap();
    heights
        .fill(
            wanted.clone(),
            &ModelHeights { model: geom },
            band,
            &mut Seq,
        )
        .unwrap();
    for k in &wanted {
        let r = heights.table().get(k).expect("decided");
        assert!(r.fold && r.by_orders, "{r:?}");
    }
    (p.count(&reads, &layers, &heights).unwrap(), wanted.len())
}

#[test]
fn the_fold_contributes_its_upper_sheet_s_facing() {
    // Bending away from the normal going up: the upper sheet faces up.
    // Bending towards it: down. For both operands' points alike.
    for (bend, facing) in [(-1.0, 1), (1.0, -1)] {
        for op in [Operand::A, Operand::B] {
            let q = Point::vert(op, VertId(9));
            let (n, read) = count(&pieces(q, [1, -1]), &Silhouette { bend, hit: true });
            assert_eq!((n, read), (facing, 1), "bend {bend}, {op:?}");
        }
    }
}

#[test]
fn a_layer_the_column_misses_reads_nothing() {
    let q = Point::vert(Operand::A, VertId(9));
    let (n, read) = count(
        &pieces(q, [0, 0]),
        &Silhouette {
            bend: -1.0,
            hit: true,
        },
    );
    assert_eq!((n, read), (0, 0));
}

#[test]
fn layers_of_another_refinement_are_refused() {
    // The same layer found from other boxes keys its core elsewhere: its
    // stored layers aren't these pieces', and no record is read by them.
    let q = Point::vert(Operand::A, VertId(9));
    let order = Order::default();
    let (sh, ft, o) = no_reads(&order);
    let reads = Records {
        shadows: &sh,
        feet: &ft,
        order: o,
    };
    let p = pieces(q, [1, -1]);
    let mut other = p.clone();
    for n in &mut other.near {
        n.core = Some(Pos::new([1.0, 2.0, 4.0]).unwrap());
    }
    let mut layers = LayerTable::new();
    layers.insert(q, p.face, other.layers(&reads).unwrap());
    let stale = KeyError::NoLayer {
        q,
        f: p.face,
        layer: LayerId(0),
    };
    assert_eq!(p.wanted(&reads, &layers), Err(stale));
    assert_eq!(
        p.count(&reads, &layers, &Heights::new()),
        Err(CountError::Key(stale))
    );
}

#[test]
fn a_hit_formed_at_no_level_is_not_a_tie() {
    // The face's facing would be uncertified too, but the hit itself
    // isn't formed: the value escalates and no tie is stored for the key
    // (a later fill, with the hit formed, decides it from its value).
    let q = Point::vert(Operand::A, VertId(9));
    let p = pieces(q, [1, -1]);
    let order = Order::default();
    let (sh, ft, o) = no_reads(&order);
    let reads = Records {
        shadows: &sh,
        feet: &ft,
        order: o,
    };
    let mut layers = LayerTable::new();
    layers.insert(q, p.face, p.layers(&reads).unwrap());
    let wanted = p.wanted(&reads, &layers).unwrap();
    let geom = Silhouette {
        bend: -1.0,
        hit: false,
    };
    let mut heights = Heights::new();
    let band = Band::curved(0.0, 1.0 / 1_073_741_824.0).unwrap();
    let r = heights.fill(
        wanted.clone(),
        &ModelHeights { model: &geom },
        band,
        &mut Seq,
    );
    assert!(r.is_err(), "{r:?}");
    for k in &wanted {
        assert_eq!(heights.key(k.h), None, "no tie stored for {k:?}");
        assert_eq!(heights.table().get(k), None);
    }
}
