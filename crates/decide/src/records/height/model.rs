//! The height geometry of the operands ([`ModelHeights`]): which value a
//! height key is decided from and which row an anchor's orders are, read
//! from the operands' points, planes, edges and curved faces
//! ([`HeightModel`]).
//!
//! **Values** (order 0, oriented as the key's stored answer):
//!
//! - a point–vertex key `VV(a, b)`: `UP·(b − a)`, exact on the stored
//!   points ([`PointHeight`]): the height of `B`'s point above `A`'s;
//! - a point–curve key `Curve(q, g, k)`: on a straight edge `UP·(x − q)`,
//!   `x` the edge's point the model reads the foot at
//!   ([`HeightModel::foot_point`]), exact; on a curved edge the same as a
//!   length (`UP` normalised) at the level, `x` the model's
//!   ([`HeightModel::curved_target`]);
//! - a hit key `Hit(q, f, sheet, i)` on a face whose hits are exact
//!   ([`HeightModel::flat_face`], a plane face): the hit's height by its
//!   sign conditions at the roots of the hit's polynomials on the stored
//!   tile and ribbon ([`super::hit`]); on a curved face
//!   `n_f·(x_h − q)·sign(n_f·ê)` ([`face_height`]) at the hit `x_h`, which
//!   the model finds, and the normal there;
//! - an edge–edge key `X(e, g, c)` of two straight edges: the edge–edge
//!   height of their lines, exact ([`EdgeEdge`]); of curved edges the
//!   curved decider's (not here).
//!
//! **Rows** (an anchor's orders): at a face, the tangent plane of the
//! face at the key's **target point** (the vertex of a point–vertex key,
//! the foot's point, the hit), against the key's moving point: on a plane
//! the plane itself moving at its rates there ([`PlaneHeight`]'s orders),
//! at a hit the stored tile's or the ribbon's tangent plane there, exactly
//! ([`super::hit::Hit::orders`]), on a curved
//! face [`tangent_plane_row`] with the model's normal and rates. A
//! point–vertex key read at a face of `A` (whose point the key holds as
//! `a`) is the plane's height above `b`, negated: the key's answer is `b`
//! above `a`. At an edge (an edge–edge key's anchor) the edge–edge row.
//!
//! **The fold rule.** Where a curved anchor's facing `sign(n_f·ê)` is
//! certified at no level, its target point is on a silhouette inside the
//! face: there `ê` lies in the tangent plane and the face folds over in
//! the shadow. Near the fold the face is two sheets, joined at the
//! silhouette, both on the side of it the face bends away from its
//! normal; the upper sheet faces up exactly when the face bends away
//! from its normal going up, `n_f·S_êê < 0` (the normal curvature along
//! `ê`). So the fold's upper sheet's facing is the sign of
//! `−n_f·S_êê` ([`FaceAt::bend`]), whatever side the point asks from.

use glam::DVec3;
use malachite::Rational;

use crate::ids::{EdgeId, FaceId};
use crate::num::exact::{ExactSign, NotFinite, Num, ORDERS, Pred};
use crate::perturb::{
    Curved, EdgeEdge, Moving, MovingAt, PlaneHeight, PointHeight, Rate, Seg, UP, face_height,
    rate_rational, tangent_plane_row,
};
use crate::records::height::hit::{FlatFace, hit_on};
use crate::records::height::{AnchorRow, HeightGeometry, KeyValue};
use crate::records::keys::{Anchor, FootId, HKey, HeightKey, Operand, Point, Query};
use crate::records::shadow::ShadowGeometry;

/// A curved face at a point, as its rows read it.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceAt<S> {
    /// The normal, oriented as the face, any positive multiple.
    pub normal: [S; 3],
    /// The face's rates there for the asking point's record (the
    /// direction field on `A`, zero on `B`).
    pub rates: [[S; 3]; ORDERS],
    /// `n·S_êê` up to a positive factor: the normal curvature along `ê`
    /// where `ê` lies in the tangent plane (module docs, "The fold
    /// rule").
    pub bend: S,
}

/// The operands' geometry the height records read, beyond the shadow
/// records'. Faces are of the operand other than the asking point's.
pub trait HeightModel: ShadowGeometry {
    /// Face `f` of `operand`'s plane `n·x = c`, its normal `n` oriented as
    /// the face; `None` for a curved face.
    fn plane(&self, operand: Operand, f: FaceId) -> Option<(DVec3, f64)>;

    /// Planar face `f`'s rates at its point `x`, for `q`'s record: the
    /// direction field there on `A`, zero on `B`. Also the rates of its
    /// ribbons' points there.
    fn plane_rates(&self, q: Point, f: FaceId, x: DVec3) -> [Rate; ORDERS];

    /// Face `f` of `operand` as its column hits read it, where they are
    /// exact (a plane face, [`super::hit`]); `None` for a curved face.
    fn flat_face(&self, operand: Operand, f: FaceId) -> Option<FlatFace<'_>> {
        let _ = (operand, f);
        None
    }

    /// Straight edge `g` of `operand` as a segment along its stored
    /// direction with its rates at a crossing; `None` for a curved edge.
    fn edge_seg(&self, operand: Operand, g: EdgeId) -> Option<Seg> {
        let _ = (operand, g);
        None
    }

    /// The point of straight edge `g` at foot `foot` of `q` the height is
    /// read at, with its rates; `None` for a curved edge.
    fn foot_point(&self, q: Point, g: EdgeId, foot: FootId) -> Option<Moving>;

    /// The target point of a curved point–curve or hit key at the level
    /// of `like` (the foot's point, the hit of `q`'s column on the
    /// face); `None` where the level can't certify it.
    fn curved_target<S: Curved>(&self, h: HKey, like: &S) -> Option<[S; 3]>;

    /// Curved face `f` (of the operand other than `q`'s) at its point
    /// `at`; `None` where the level can't form it.
    fn face_at<S: Curved>(&self, q: Point, f: FaceId, at: &[S; 3], like: &S) -> Option<FaceAt<S>>;

    /// Whether anchor `k`'s record is geometric (a certified truth).
    fn geometric(&self, k: HeightKey) -> bool {
        let _ = k;
        false
    }
}

/// The flat predicates [`ModelHeights`] gives.
#[derive(Clone, Debug, PartialEq)]
pub enum FlatHeight {
    /// A point's height above another.
    Point(PointHeight),
    /// A plane's height above a point.
    Plane(PlaneHeight),
    /// A point's height above a plane: [`FlatHeight::Plane`] negated.
    Below(PlaneHeight),
    /// The height of the edge of `B` above the edge of `A` at their
    /// shadows' crossing.
    EdgeEdge(EdgeEdge),
}

impl Pred for FlatHeight {
    fn eval<N: Num>(&self) -> N {
        match self {
            FlatHeight::Point(p) => p.eval(),
            FlatHeight::Plane(p) => p.eval(),
            FlatHeight::Below(p) => p.eval::<N>().neg(),
            FlatHeight::EdgeEdge(p) => p.eval(),
        }
    }
}

/// The height geometry of a model (module docs).
#[derive(Clone, Copy, Debug)]
pub struct ModelHeights<'a, M> {
    /// The model.
    pub model: &'a M,
}

/// A query of `operand`.
fn point_of(operand: Operand, q: Query) -> Point {
    Point { operand, q }
}

impl<M: HeightModel> ModelHeights<'_, M> {
    fn point(&self, p: Point) -> Moving {
        self.model.point(p)
    }

    /// The key's moving point and, for a face anchor, whether the row is
    /// negated (a point–vertex key read at a face of `A`).
    fn asker(&self, k: HeightKey) -> Option<(Point, bool)> {
        let Anchor::Face(op, _) = k.anchor else {
            return None;
        };
        match k.h {
            HKey::VV { a, b } => Some(match op {
                Operand::B => (point_of(Operand::A, a), false),
                Operand::A => (point_of(Operand::B, b), true),
            }),
            HKey::Curve { q, .. } | HKey::Hit { q, .. } => Some((q, false)),
            HKey::X { .. } => None,
        }
    }

    /// A point–curve key's target point, flat: a straight edge's foot
    /// point.
    fn flat_target(&self, h: HKey) -> Option<DVec3> {
        match h {
            HKey::Curve { q, g, foot } => self.model.foot_point(q, g, foot).map(|m| m.at),
            HKey::VV { .. } | HKey::Hit { .. } | HKey::X { .. } => None,
        }
    }

    /// The vertex of a point–vertex key on the anchor's face.
    fn vertex_on(&self, h: HKey, op: Operand) -> Option<DVec3> {
        match h {
            HKey::VV { a, b } => Some(match op {
                Operand::A => self.point(point_of(Operand::A, a)).at,
                Operand::B => self.point(point_of(Operand::B, b)).at,
            }),
            _ => self.flat_target(h),
        }
    }

    /// A curved key's or anchor's target point at a level.
    fn target_at<S: Curved>(&self, h: HKey, op: Operand, like: &S) -> Option<[S; 3]> {
        match h {
            HKey::VV { .. } => {
                let x = self.vertex_on(h, op)?;
                Some([x.x, x.y, x.z].map(|c| S::from_f64(c, like)))
            }
            HKey::Curve { q, g, foot } if self.model.foot_point(q, g, foot).is_some() => {
                let x = self.flat_target(h)?;
                Some([x.x, x.y, x.z].map(|c| S::from_f64(c, like)))
            }
            HKey::Curve { .. } | HKey::Hit { .. } => self.model.curved_target(h, like),
            HKey::X { .. } => None,
        }
    }

    /// The anchor face's curved data at the key's target, with the asking
    /// point at the level.
    fn curved_anchor<S: Curved>(
        &self,
        k: HeightKey,
        like: &S,
    ) -> Option<(FaceAt<S>, MovingAt<S>, bool)> {
        let Anchor::Face(op, f) = k.anchor else {
            return None;
        };
        let (p, neg) = self.asker(k)?;
        let at = self.target_at(k.h, op, like)?;
        let face = self.model.face_at(p, f, &at, like)?;
        let q = MovingAt::from_flat(&self.point(p), like);
        Some((face, q, neg))
    }

    /// The two straight edges of an edge–edge key, each at its rates.
    fn edge_pair(&self, h: HKey) -> Option<EdgeEdge> {
        let HKey::X { e, g, .. } = h else {
            return None;
        };
        Some(EdgeEdge {
            e: self.model.edge_seg(Operand::A, e)?,
            g: self.model.edge_seg(Operand::B, g)?,
        })
    }
}

/// An exact sign that can't be formed: the key names no hit there, a
/// caller's bug the decider refuses.
const NO_HIT: ExactSign = Err(NotFinite);

impl<M: HeightModel> HeightGeometry for ModelHeights<'_, M> {
    type Flat = FlatHeight;

    fn value(&self, h: HKey) -> KeyValue<FlatHeight> {
        match h {
            HKey::VV { a, b } => KeyValue::Flat(FlatHeight::Point(PointHeight {
                q: self.point(point_of(Operand::A, a)),
                w: self.point(point_of(Operand::B, b)),
            })),
            HKey::Curve { q, g, foot } => match self.model.foot_point(q, g, foot) {
                Some(w) => KeyValue::Flat(FlatHeight::Point(PointHeight {
                    q: self.point(q),
                    w,
                })),
                None => KeyValue::Curved,
            },
            HKey::Hit { q, f, sheet, i } => {
                let Some(face) = self.model.flat_face(q.operand.other(), f) else {
                    return KeyValue::Curved;
                };
                let at = self.point(q).at.to_array();
                KeyValue::Exact(match hit_on(&face, sheet, i, at) {
                    Ok(Some(hit)) => Ok(hit.height),
                    _ => NO_HIT,
                })
            }
            HKey::X { .. } => match self.edge_pair(h) {
                Some(p) => KeyValue::Flat(FlatHeight::EdgeEdge(p)),
                None => KeyValue::Curved,
            },
        }
    }

    fn curved_value<S: Curved>(&self, h: HKey, like: &S) -> Option<S> {
        match h {
            HKey::Curve { q, .. } => {
                let x = self.model.curved_target(h, like)?;
                let at = MovingAt::from_flat(&self.point(q), like).at;
                let up = [UP.x, UP.y, UP.z].map(|c| S::from_f64(c, like));
                let len = up
                    .iter()
                    .fold(S::int(0, like), |s, c| s.add(&c.mul(c)))
                    .sqrt();
                let d = (0..3).fold(S::int(0, like), |s, i| s.add(&up[i].mul(&x[i].sub(&at[i]))));
                Some(d.div(&len))
            }
            HKey::Hit { q, f, .. } => {
                let x = self.model.curved_target(h, like)?;
                let face = self.model.face_at(q, f, &x, like)?;
                let at = MovingAt::from_flat(&self.point(q), like).at;
                face_height(&face.normal, &x, &at)
            }
            HKey::VV { .. } | HKey::X { .. } => None,
        }
    }

    fn facing_only<S: Curved>(&self, h: HKey, like: &S) -> bool {
        let HKey::Hit { q, f, .. } = h else {
            return false;
        };
        self.model.flat_face(q.operand.other(), f).is_none()
            && self
                .model
                .curved_target(h, like)
                .is_some_and(|x| self.model.face_at(q, f, &x, like).is_some())
    }

    fn row(&self, k: HeightKey) -> AnchorRow<FlatHeight> {
        let (op, f) = match k.anchor {
            Anchor::Face(op, f) => (op, f),
            Anchor::Edge(..) => {
                return match self.edge_pair(k.h) {
                    Some(p) => AnchorRow::Flat(FlatHeight::EdgeEdge(p)),
                    None => AnchorRow::Curved,
                };
            }
        };
        if let HKey::Hit { q, sheet, i, .. } = k.h {
            let Some(face) = self.model.flat_face(op, f) else {
                return AnchorRow::Curved;
            };
            let at = self.point(q).at.to_array();
            let Ok(Some(hit)) = hit_on(&face, sheet, i, at) else {
                return AnchorRow::Exact([NO_HIT; ORDERS]);
            };
            // The sheet's rates at the hit (the field's value there) less
            // the point's.
            let rates = self.model.plane_rates(q, f, DVec3::from_array(at));
            let qr = self.point(q).rates;
            let deltas: [[Rational; 3]; ORDERS] = std::array::from_fn(|n| {
                let (r, d) = (rate_rational(&rates[n]), rate_rational(&qr[n]));
                std::array::from_fn(|c| &r[c] - &d[c])
            });
            return AnchorRow::Exact(hit.orders(&face, at, &deltas).unwrap_or([NO_HIT; ORDERS]));
        }
        let (Some((p, neg)), Some((normal, offset))) = (self.asker(k), self.model.plane(op, f))
        else {
            return AnchorRow::Curved;
        };
        let Some(x) = self.vertex_on(k.h, op) else {
            return AnchorRow::Curved;
        };
        let plane = PlaneHeight {
            normal,
            offset,
            rates: self.model.plane_rates(p, f, x),
            q: self.point(p),
        };
        AnchorRow::Flat(if neg {
            FlatHeight::Below(plane)
        } else {
            FlatHeight::Plane(plane)
        })
    }

    fn curved_row<S: Curved>(&self, k: HeightKey, like: &S) -> Option<[S; ORDERS]> {
        let (face, q, neg) = self.curved_anchor(k, like)?;
        let ord = tangent_plane_row(&face.normal, &face.rates, &q)?;
        Some(if neg { ord.map(|x| x.neg()) } else { ord })
    }

    fn curved_fold<S: Curved>(&self, k: HeightKey, like: &S) -> Option<S> {
        let (face, _, _) = self.curved_anchor(k, like)?;
        Some(face.bend.neg())
    }

    fn geometric(&self, k: HeightKey) -> bool {
        self.model.geometric(k)
    }
}
