//! The fold rule on a face known only at the precision levels (a model
//! of a curved face, no stored nets): a hit on a silhouette inside it,
//! `q` on it, so the hit's height and the face's facing are certified at
//! no level. Its record is the fold's, the upper sheet's facing from how
//! the face bends, the same for either operand's point; a piece of
//! non-zero degree reading it contributes that facing, for `s02` and
//! `s20` alike; a hit formed at no level stores no tie. (On stored nets
//! the fold rule is exact: [`crate::records::height::hit`].)

use super::*;
use crate::num::decide::Band;
use crate::num::exact::ORDERS;
use crate::perturb::{Curved, Moving, Rate, Seg};
use crate::records::height::model::{FaceAt, HeightModel, ModelHeights};
use crate::records::keys::{FootId, HKey, HitIx, Sheet};
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

const FACE: FaceId = FaceId(5);

/// The hit key of `q` on the face's one tile.
fn key(q: Point) -> HeightKey {
    HeightKey {
        h: HKey::Hit {
            q,
            f: FACE,
            sheet: Sheet::Tile([0, 0]),
            i: HitIx(0),
        },
        anchor: Anchor::Face(q.operand.other(), FACE),
    }
}

/// The hit's record filled on `geom`.
fn fill(q: Point, geom: &Silhouette) -> (Heights, Result<(), String>) {
    let mut heights = Heights::new();
    let band = Band::curved(0.0, 1.0 / 1_073_741_824.0).unwrap();
    let r = heights
        .fill([key(q)], &ModelHeights { model: geom }, band, &mut Seq)
        .map_err(|e| format!("{e:?}"));
    (heights, r)
}

/// A face's pieces at `q` as a plane face's would be: one near piece of
/// degree `degree` holding the one hit.
fn pieces(q: Point, degree: i64) -> FacePieces {
    FacePieces {
        point: q,
        face: FACE,
        above: 0,
        below: 0,
        near: vec![NearPiece {
            degree,
            hits: smallvec::smallvec![0],
            each: false,
            germ: None,
        }],
        hits: vec![HitTarget {
            target: Target::Hit {
                f: FACE,
                sheet: Sheet::Tile([0, 0]),
                i: HitIx(0),
                inc: None,
            },
            height: Ordering::Equal,
            facing: Some(Sign::Pos),
            near: SmallVec::new(),
        }],
        unformed: false,
        pieces: 0,
    }
}

#[test]
fn the_fold_contributes_its_upper_sheet_s_facing() {
    // Bending away from the normal going up: the upper sheet faces up.
    // Bending towards it: down. For both operands' points alike.
    let order = Order::default();
    let (sh, ft, o) = no_reads(&order);
    let reads = Records {
        shadows: &sh,
        feet: &ft,
        order: o,
    };
    for (bend, facing) in [(-1.0, 1), (1.0, -1)] {
        for op in [Operand::A, Operand::B] {
            let q = Point::vert(op, VertId(9));
            let (heights, r) = fill(q, &Silhouette { bend, hit: true });
            r.unwrap();
            let rec = heights.table().get(&key(q)).expect("decided");
            assert!(rec.fold && rec.by_orders, "{rec:?}");
            let p = pieces(q, 1);
            assert_eq!(p.wanted(&reads, &[]), Ok(vec![key(q)]));
            assert_eq!(
                p.count(&reads, &heights, &[]),
                Ok(facing),
                "bend {bend}, {op:?}"
            );
            // A piece of degree 0 reads nothing and counts nothing.
            let p = pieces(q, 0);
            assert_eq!(p.wanted(&reads, &[]), Ok(Vec::new()));
            assert_eq!(p.count(&reads, &Heights::new(), &[]), Ok(0));
        }
    }
}

#[test]
fn a_hit_formed_at_no_level_is_not_a_tie() {
    // The face's facing would be uncertified too, but the hit itself
    // isn't formed: the value escalates and no tie is stored for the key
    // (a later fill, with the hit formed, decides it from its value).
    let q = Point::vert(Operand::A, VertId(9));
    let geom = Silhouette {
        bend: -1.0,
        hit: false,
    };
    let (heights, r) = fill(q, &geom);
    assert!(r.is_err(), "{r:?}");
    let k = key(q);
    assert_eq!(heights.key(k.h), None, "no tie stored for {k:?}");
    assert_eq!(heights.table().get(&k), None);
}

#[test]
fn a_piece_of_a_face_without_its_hits_holding_the_column_is_refused() {
    // The face is counted without its nets, so its hit isn't formed:
    // the count says so, never a guess.
    let q = Point::vert(Operand::A, VertId(9));
    let order = Order::default();
    let (sh, ft, o) = no_reads(&order);
    let reads = Records {
        shadows: &sh,
        feet: &ft,
        order: o,
    };
    let mut p = pieces(q, 1);
    p.unformed = true;
    p.hits.clear();
    p.near[0].hits.clear();
    let refused = CountError::HitsUnformed {
        point: q,
        face: FACE,
    };
    assert_eq!(p.wanted(&reads, &[]), Err(refused.clone()));
    assert_eq!(p.count(&reads, &Heights::new(), &[]), Err(refused));
    // A plane face's piece of non-zero degree with no hit: a mismatch of
    // the hits and the boxes, refused too.
    p.unformed = false;
    assert_eq!(
        p.count(&reads, &Heights::new(), &[]),
        Err(CountError::NoHit {
            point: q,
            face: FACE
        })
    );
}

#[test]
fn a_fold_forming_no_hit_on_stored_nets_runs_out_of_precision() {
    // A column tangent to a tile at `q`'s height that forms no hit (the
    // tile not bending along `ê` there, or the fold on a trim at `q`):
    // the face's count ends as a piece no level certifies does, never as
    // a hit refused.
    let mut order = Order::default();
    let data = polygon(&[[0.25, 0.25], [1.75, 0.25], [1.0, 1.5]], 1e-3, &mut order);
    let face = data.hit_face(&order).expect("nets");
    let q = Point::vert(Operand::A, VertId(9));
    let motion = motion_of(Operand::A);
    let flat = |e: HitError| {
        let cell = HitCell::default();
        let _ = cell.hits.set(Err(e));
        let f = Flat {
            face: face.clone(),
            point: q,
            at: [1.0, 0.5, 0.0],
            motion: &motion,
            cell: &cell,
        };
        f.hits().map(<[Hit]>::len)
    };
    assert_eq!(
        flat(HitError::Fold([0, 1])),
        Err(CountError::OutOfBudget {
            cause: Exhausted::Precision,
            point: q,
            face: data.id,
            at: None,
        })
    );
    assert_eq!(
        flat(HitError::Tile([0, 1])),
        Err(CountError::Hits(HitError::Tile([0, 1])))
    );
}
