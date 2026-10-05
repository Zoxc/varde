//! The handles of the operations set up in the move's session that have
//! a value to drag ([`OpKnob`], drawn and dragged by the viewport): where
//! each knob stands and how it drags, worked out from the model shown, and
//! a knob dragged ([`MotionLook::DragKnob`](varde_view::MotionLook::DragKnob))
//! typed into its field.
//!
//! - An offset face's: along the first face's outward normal from where
//!   it is before the offset ([`super::offset_face::Anchor`]), at the
//!   distance, negative inward.
//! - A shell's: along the first face removed, from it into the body (out
//!   of it for Outward walls), at the thickness.
//! - A draft's: on the first face, turning about its hinge (where it
//!   meets the neutral plane) by the angle, its outward normal leaning
//!   towards the pull.
//! - A chamfer's: from the first edge along the bisector of its faces
//!   for Equal, so the knob is on the chamfer's middle; along each face
//!   for Two distances (one knob each); along the first face for Distance
//!   and angle.
//! - A fillet's: from the first edge along the bisector, the knob on the
//!   round's middle.
//! - A scale's: a slider from the point it scales about, towards the
//!   bodies' box centre for Uniform (100 pixels a factor of 1), along each
//!   world axis for Per axis; none for Edge length.
//! - An align's: along the target's primary direction from its point, at
//!   the offset, and on a ring about it there, at the turn.
//! - A linear pattern's: the spacing's on its axis from the original's
//!   start to the first copy's (the total's to the last's), the count's
//!   on a rail above the copies, in its own colour.
//! - A circular pattern's: the span's on the arc through the copies at
//!   the last copy (none for Full 360°), the count's on a slider running
//!   on along the arc's tangent past it; for a new one only, as it finds
//!   the original on the document's model.
//!
//! A shell's, draft's, chamfer's and fillet's knobs stand where their
//! first face or edge is before the feature changes it, which the model
//! shown tells only before it shows a preview: their [`KnobAnchor`] is
//! found then, for a new feature, and kept while the first face or edge
//! and the document stay. An edited one has no knob.

use glam::DVec3;
use varde_document::{EdgeRef, FaceRef, Generation, PlaneRef};
use varde_expr::{AngleUnit, Unit};
use varde_view::{
    ChamferType, KnobPath, KnobRadius, KnobScale, KnobSnap, KnobTone, MotionField, MotionKind,
    OpKnob, PatternMode, ScaleMode, ShellDirection,
};

use super::{Doc, MotionSession, field_ask};

/// How many pixels a scale's slider is for a factor of 1.
const SCALE_PIXELS: f64 = 100.0;
/// How far out of the target's point an align's ring is, in pixels.
const TURN_PIXELS: f64 = 60.0;
/// How many pixels a circular pattern's count's slider runs a copy.
const COUNT_PIXELS: f64 = 8.0;

/// Where a shell's, draft's, chamfer's or fillet's knobs stand, as the
/// first face or edge is before the feature: found on the model shown at
/// `generation`, the document's alone.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum KnobAnchor {
    /// A point of the first face, and its outward normal there.
    Face {
        face: FaceRef,
        generation: Generation,
        at: DVec3,
        normal: DVec3,
    },
    /// A point of the first edge, and its faces' outward normals there:
    /// the first's (as a chamfer's sizes take them, flipped or not) and
    /// the second's.
    Edge {
        edge: EdgeRef,
        generation: Generation,
        at: DVec3,
        normals: [DVec3; 2],
    },
}

impl Doc {
    /// The knobs of the handle of the operation set up in `session`, with
    /// the bodies' box in the model shown `bounds`: none in a document
    /// that can't be changed, or while where they stand isn't known.
    pub(super) fn motion_knobs(
        &self,
        session: &MotionSession,
        bounds: Option<[DVec3; 2]>,
    ) -> Vec<OpKnob> {
        if !self.editable() {
            return Vec::new();
        }
        let knobs = match session.kind {
            MotionKind::OffsetFace => self.offset_face_knob(session).into_iter().collect(),
            MotionKind::Shell => self.shell_knob(session).into_iter().collect(),
            MotionKind::Draft => self.draft_knob(session).into_iter().collect(),
            MotionKind::Chamfer => self.chamfer_knobs(session),
            MotionKind::Fillet => self.fillet_knob(session).into_iter().collect(),
            MotionKind::Scale => self.scale_knobs(session, bounds),
            MotionKind::Align => self.align_knobs(session),
            MotionKind::LinearPattern => self.linear_knobs(session, bounds),
            MotionKind::CircularPattern => self.circular_knobs(session),
            _ => Vec::new(),
        };
        let finite = |knob: &OpKnob| {
            let path = match knob.path {
                KnobPath::Line { origin, along } => origin.is_finite() && along.is_finite(),
                KnobPath::Arc {
                    centre,
                    axis,
                    radial,
                    ..
                } => centre.is_finite() && axis.is_finite() && radial.is_finite(),
            };
            path && knob.value.is_finite() && knob.out.is_finite()
        };
        knobs.into_iter().filter(finite).collect()
    }

    /// Types where knob `index` of the handle was dragged to, `value` in
    /// its field's own units, into its field, as the knob says: a value
    /// the field refuses changes nothing. An offset face's sign is its
    /// side; others' values must be above zero (an align's offset and
    /// turn may be anything), a draft's angle under a quarter turn.
    pub(super) fn drag_knob(&mut self, index: usize, value: f64) {
        let Some(session) = &self.motion else {
            return;
        };
        let bounds = self.motion_bounds(session);
        let Some(knob) = self.motion_knobs(session, bounds).get(index).copied() else {
            return;
        };
        let kind = session.kind;
        let refused = match kind {
            // Its sign is its side: through zero, never at it.
            MotionKind::OffsetFace => value == 0.0,
            MotionKind::Align => false,
            // A whole count, of two or more (the field's ask says how
            // many at most).
            _ if knob.snap == KnobSnap::Count => value < 2.0 || value.fract() != 0.0,
            _ => value <= 0.0,
        };
        if !value.is_finite() || refused {
            return;
        }
        if kind == MotionKind::Draft && value >= std::f64::consts::FRAC_PI_2 {
            return;
        }
        let document = self.editor.document();
        let units = match knob.snap {
            KnobSnap::Length => Some(Unit::Length(document.units())),
            KnobSnap::Angle => Some(Unit::Angle(AngleUnit::Deg)),
            KnobSnap::Factor | KnobSnap::Count => None,
        };
        let shown = if kind == MotionKind::OffsetFace {
            value.abs()
        } else {
            value
        };
        let text = varde_expr::format(shown, units);
        // Shown as nothing in the design's units (zoomed far in, the snap
        // is finer than they show), it would read as zero.
        if kind == MotionKind::OffsetFace && text == varde_expr::format(0.0, units) {
            return;
        }
        let ask = field_ask(kind, knob.field, &document.design());
        let Some(session) = &mut self.motion else {
            return;
        };
        let mut field = session.fields[knob.field.index()].clone();
        field.input(text, &ask);
        if field.error.is_some() {
            return;
        }
        session.fields[knob.field.index()] = field;
        if kind == MotionKind::OffsetFace {
            session.flip = value < 0.0;
        }
    }

    /// The bodies' box in the model shown, a merged one in its holder.
    pub(super) fn motion_bounds(&self, session: &MotionSession) -> Option<[DVec3; 2]> {
        let shown = self.shown_bodies(&session.bodies);
        self.feed.pick_index().bodies_bounds(&shown)
    }

    /// Finds where a shell's, draft's, chamfer's or fillet's knobs stand
    /// ([`KnobAnchor`]) if it isn't known for its first face or edge and
    /// the document as they are: only while the model shown is the
    /// document's alone, of a new feature (an edited one's is changed by
    /// the feature itself).
    pub(super) fn follow_knob_anchor(&mut self) {
        let generation = self.editor.generation();
        let Some(session) = &self.motion else {
            return;
        };
        let current = |anchor: &KnobAnchor| match anchor {
            KnobAnchor::Face {
                face,
                generation: at,
                ..
            } => session.faces.refs.first() == Some(face) && *at == generation,
            KnobAnchor::Edge {
                edge,
                generation: at,
                ..
            } => session.blend.edges.refs.first() == Some(edge) && *at == generation,
        };
        let faces = matches!(session.kind, MotionKind::Shell | MotionKind::Draft);
        let edges = matches!(session.kind, MotionKind::Chamfer | MotionKind::Fillet);
        if !(faces || edges)
            || session.feature.is_some()
            || session.knob_anchor.as_ref().is_some_and(current)
            || self.feed.generation() != Some(generation)
            || self.feed.predates_replacement()
            || self.feed.shows_draft()
        {
            return;
        }
        let index = self.feed.pick_index();
        let anchor = if faces {
            let Some(face) = session.faces.refs.first().cloned() else {
                return;
            };
            let Some(found) = index.find_face(face.body, &face.key, face.near) else {
                return;
            };
            let Some((at, normal)) = index.face_point(found, face.near) else {
                return;
            };
            KnobAnchor::Face {
                face,
                generation,
                at,
                normal,
            }
        } else {
            let Some(edge) = session.blend.edges.refs.first().cloned() else {
                return;
            };
            // The first face as the chamfer's sizes take it.
            let flip = session.kind == MotionKind::Chamfer && session.flip;
            let keys = if flip {
                [edge.faces[1], edge.faces[0]]
            } else {
                edge.faces
            };
            let found = keys.map(|key| index.find_face(edge.body, &key, edge.near));
            let [Some(first), Some(second)] = found else {
                return;
            };
            // The picked point taken onto both faces, so onto the edge
            // between flat ones.
            let mut at = edge.near;
            let mut normals = [DVec3::ZERO; 2];
            for _ in 0..4 {
                for (k, face) in [first, second].into_iter().enumerate() {
                    let Some((point, normal)) = index.face_point(face, at) else {
                        return;
                    };
                    at = point;
                    normals[k] = normal;
                }
            }
            KnobAnchor::Edge {
                edge,
                generation,
                at,
                normals,
            }
        };
        if let Some(session) = &mut self.motion {
            session.knob_anchor = Some(anchor);
        }
    }

    /// An offset face's knob, once its anchor is known.
    fn offset_face_knob(&self, session: &MotionSession) -> Option<OpKnob> {
        let handle = self.offset_face_view(session).handle?;
        Some(OpKnob {
            field: MotionField::Distance,
            path: KnobPath::Line {
                origin: handle.origin,
                along: handle.normal,
            },
            value: handle.at,
            scale: KnobScale::Times(1.0),
            snap: KnobSnap::Length,
            out: handle.normal,
            shaft: Some(0.0),
            tone: KnobTone::Modify,
        })
    }

    /// The face anchor of a shell's or draft's first face, if it's known
    /// for it and the document as they are.
    fn face_anchor(&self, session: &MotionSession) -> Option<(DVec3, DVec3)> {
        match &session.knob_anchor {
            Some(KnobAnchor::Face {
                face,
                generation,
                at,
                normal,
            }) if session.faces.refs.first() == Some(face)
                && *generation == self.editor.generation() =>
            {
                Some((*at, *normal))
            }
            _ => None,
        }
    }

    /// The edge anchor of a chamfer's or fillet's first edge, if it's
    /// known for it and the document as they are: its point, and the
    /// ways into its first face and its second, square to it.
    fn edge_anchor(&self, session: &MotionSession) -> Option<(DVec3, [DVec3; 2])> {
        let (at, [first, second]) = match &session.knob_anchor {
            Some(KnobAnchor::Edge {
                edge,
                generation,
                at,
                normals,
            }) if session.blend.edges.refs.first() == Some(edge)
                && *generation == self.editor.generation() =>
            {
                (*at, *normals)
            }
            _ => return None,
        };
        // Into each face, away from the edge: against the other's normal,
        // taken into its plane.
        let into = |own: DVec3, other: DVec3| (own * own.dot(other) - other).try_normalize();
        Some((at, [into(first, second)?, into(second, first)?]))
    }

    /// The value field `field` last read, in its own units, if it reads.
    fn knob_value(session: &MotionSession, field: MotionField) -> Option<f64> {
        session.field(field).value.as_ref().map(|value| value.value)
    }

    /// A shell's knob: along its first face removed, into the body (out
    /// of it for Outward walls), at the thickness.
    fn shell_knob(&self, session: &MotionSession) -> Option<OpKnob> {
        let (at, normal) = self.face_anchor(session)?;
        let along = match session.direction {
            ShellDirection::Inward => -normal,
            ShellDirection::Outward => normal,
        };
        Some(OpKnob {
            field: MotionField::Thickness,
            path: KnobPath::Line { origin: at, along },
            value: Self::knob_value(session, MotionField::Thickness)?,
            scale: KnobScale::Times(1.0),
            snap: KnobSnap::Length,
            out: along,
            shaft: Some(0.0),
            tone: KnobTone::Modify,
        })
    }

    /// A draft's knob: on its first face, turning about the face's hinge
    /// on the neutral plane by the angle, so its outward normal leans
    /// towards the pull (the plane's normal, against it flipped). None for
    /// a face square to the pull, or one whose point is on its hinge.
    fn draft_knob(&self, session: &MotionSession) -> Option<OpKnob> {
        let (at, normal) = self.face_anchor(session)?;
        let [point, plane_normal] = match session.plane.as_ref()? {
            PlaneRef::Origin(plane) => [DVec3::ZERO, plane.placement().normal],
            PlaneRef::Face(_) => self.feed.draft_reference()?,
        };
        let pull = if session.flip {
            -plane_normal
        } else {
            plane_normal
        };
        // Up the face, towards the pull.
        let up = (pull - normal * normal.dot(pull)).try_normalize()?;
        let rise = up.dot(pull);
        if rise.abs() < 1e-9 {
            return None;
        }
        let hinge = at - up * ((at - point).dot(pull) / rise);
        let radial = at - hinge;
        let radius = radial.length();
        let radial = radial.try_normalize()?;
        // Turned about `axis`, a point up the face moves into the body
        // and the normal leans up: `axis × up` is `−normal`.
        let axis = normal.cross(up).try_normalize()?;
        let angle = Self::knob_value(session, MotionField::Angle)?;
        let (sin, cos) = angle.sin_cos();
        let tangent = axis.cross(radial) * cos - radial * sin;
        Some(OpKnob {
            field: MotionField::Angle,
            path: KnobPath::Arc {
                centre: hinge,
                axis,
                radial,
                radius: KnobRadius::World(radius),
            },
            value: angle,
            scale: KnobScale::Times(1.0),
            snap: KnobSnap::Angle,
            out: tangent,
            shaft: Some(0.0),
            tone: KnobTone::Modify,
        })
    }

    /// A chamfer's knobs: along the bisector of its first edge's faces
    /// for Equal, the knob on the chamfer's middle; along each face for
    /// Two distances; along the first face for Distance and angle.
    fn chamfer_knobs(&self, session: &MotionSession) -> Vec<OpKnob> {
        let Some((at, [first, second])) = self.edge_anchor(session) else {
            return Vec::new();
        };
        let knob = |field: MotionField, along: DVec3, times: f64| {
            Some(OpKnob {
                field,
                path: KnobPath::Line { origin: at, along },
                value: Self::knob_value(session, field)?,
                scale: KnobScale::Times(times),
                snap: KnobSnap::Length,
                out: along,
                shaft: Some(0.0),
                tone: KnobTone::Modify,
            })
        };
        let knobs = match session.chamfer_type {
            ChamferType::Equal => {
                let both = first + second;
                let Some(along) = both.try_normalize() else {
                    return Vec::new();
                };
                vec![knob(
                    MotionField::ChamferDistance,
                    along,
                    both.length() / 2.0,
                )]
            }
            ChamferType::Two => vec![
                knob(MotionField::ChamferDistance, first, 1.0),
                knob(MotionField::ChamferSecond, second, 1.0),
            ],
            ChamferType::Angle => vec![knob(MotionField::ChamferDistance, first, 1.0)],
        };
        knobs.into_iter().flatten().collect()
    }

    /// A fillet's knob: along the bisector of its first edge's faces, on
    /// the round's middle, `r (1 / sin(φ / 2) − 1)` from the edge for an
    /// angle `φ` between the faces.
    fn fillet_knob(&self, session: &MotionSession) -> Option<OpKnob> {
        let (at, [first, second]) = self.edge_anchor(session)?;
        let along = (first + second).try_normalize()?;
        let half = first.dot(second).clamp(-1.0, 1.0).acos() / 2.0;
        let times = 1.0 / half.sin() - 1.0;
        if !(times > 1e-6 && times.is_finite()) {
            return None;
        }
        Some(OpKnob {
            field: MotionField::Radius,
            path: KnobPath::Line { origin: at, along },
            value: Self::knob_value(session, MotionField::Radius)?,
            scale: KnobScale::Times(times),
            snap: KnobSnap::Length,
            out: along,
            shaft: Some(0.0),
            tone: KnobTone::Modify,
        })
    }

    /// A scale's knobs: sliders from its point, [`SCALE_PIXELS`] a factor
    /// of 1, towards the bodies' box centre (`bounds`) for Uniform, along
    /// each world axis towards it for Per axis; none for Edge length.
    fn scale_knobs(&self, session: &MotionSession, bounds: Option<[DVec3; 2]>) -> Vec<OpKnob> {
        let Some(point) = self.scale_view(session).at else {
            return Vec::new();
        };
        let towards = bounds.map_or(DVec3::ONE, |[low, high]| (low + high) / 2.0 - point);
        let knob = |field: MotionField, along: DVec3| {
            Some(OpKnob {
                field,
                path: KnobPath::Line {
                    origin: point,
                    along,
                },
                value: Self::knob_value(session, field)?,
                scale: KnobScale::Pixels(SCALE_PIXELS),
                snap: KnobSnap::Factor,
                out: along,
                shaft: Some(0.0),
                tone: KnobTone::Modify,
            })
        };
        match session.scale.mode {
            ScaleMode::Uniform => {
                let along = towards.try_normalize().unwrap_or(DVec3::ONE.normalize());
                knob(MotionField::Factor, along).into_iter().collect()
            }
            ScaleMode::PerAxis => varde_document::Axis3::ALL
                .into_iter()
                .filter_map(|axis| {
                    let way = axis.direction();
                    let along = if towards.dot(way) < 0.0 { -way } else { way };
                    knob(MotionField::AxisFactor(axis), along)
                })
                .collect(),
            ScaleMode::EdgeLength => Vec::new(),
        }
    }

    /// An align's knobs, once its target's point and primary direction
    /// are known: along the primary from the point, at the offset; and on
    /// a ring about it there, [`TURN_PIXELS`] out, at the turn, from the
    /// target's second direction (or any square to the primary).
    fn align_knobs(&self, session: &MotionSession) -> Vec<OpKnob> {
        let view = self.align_view(session);
        let target = view.marks[1];
        let (Some(point), Some(primary)) = (target.point, target.directions[0]) else {
            return Vec::new();
        };
        let Some(primary) = primary.try_normalize() else {
            return Vec::new();
        };
        let offset = Self::knob_value(session, MotionField::Distance).unwrap_or(0.0);
        let turn = Self::knob_value(session, MotionField::Angle).unwrap_or(0.0);
        let radial = (target.directions[1])
            .and_then(|second| (second - primary * primary.dot(second)).try_normalize())
            .unwrap_or_else(|| primary.any_orthonormal_vector());
        let (sin, cos) = turn.sin_cos();
        let tangent = primary.cross(radial) * cos - radial * sin;
        vec![
            OpKnob {
                field: MotionField::Distance,
                path: KnobPath::Line {
                    origin: point,
                    along: primary,
                },
                value: offset,
                scale: KnobScale::Times(1.0),
                snap: KnobSnap::Length,
                out: primary,
                shaft: Some(0.0),
                tone: KnobTone::Create,
            },
            OpKnob {
                field: MotionField::Angle,
                path: KnobPath::Arc {
                    centre: point + primary * offset,
                    axis: primary,
                    radial,
                    radius: KnobRadius::Pixels(TURN_PIXELS),
                },
                value: turn,
                scale: KnobScale::Times(1.0),
                snap: KnobSnap::Angle,
                out: tangent,
                shaft: Some(0.0),
                tone: KnobTone::Create,
            },
        ]
    }

    /// A linear pattern's knobs, once its axis and the bodies' box are
    /// known: the spacing's (the total's in Total) on the axis through the
    /// box's middle, from the original's start (the box's end the copies
    /// go away from, which the box has whether it holds the copies or
    /// not) to the first copy's (the last's); and the count's on a rail
    /// above the copies, at the last copy, a spacing a copy, its shaft
    /// from the original.
    fn linear_knobs(&self, session: &MotionSession, bounds: Option<[DVec3; 2]>) -> Vec<OpKnob> {
        let (Some([low, high]), (_, Some([_, along]))) = (bounds, self.reference_line(session))
        else {
            return Vec::new();
        };
        let Some(along) = along.try_normalize() else {
            return Vec::new();
        };
        let count = Self::knob_value(session, MotionField::Count);
        let spread = Self::knob_value(session, MotionField::Spread);
        let corners = box_corners(low, high);
        let middle = (low + high) / 2.0;
        let start = (corners.iter().map(|&corner| corner.dot(along))).fold(f64::INFINITY, f64::min);
        let origin = middle + along * (start - middle.dot(along));
        let up = (DVec3::Z - along * along.dot(DVec3::Z))
            .try_normalize()
            .unwrap_or(DVec3::Y);
        let top = (corners.iter().map(|&corner| corner.dot(up))).fold(f64::NEG_INFINITY, f64::max);
        let rail = origin + up * (top - origin.dot(up) + 0.25 * (high - low).length());
        let total = session.mode == PatternMode::Total;
        let mut knobs = Vec::new();
        if let Some(spread) = spread {
            knobs.push(OpKnob {
                field: MotionField::Spread,
                path: KnobPath::Line { origin, along },
                value: spread,
                scale: KnobScale::Times(1.0),
                snap: KnobSnap::Length,
                out: along,
                shaft: Some(0.0),
                tone: KnobTone::Create,
            });
        }
        // A copy's step along the rail.
        let step = match (spread, count) {
            (Some(spread), Some(count)) if total && count > 1.0 => spread / (count - 1.0),
            (Some(spread), _) if !total => spread,
            _ => return knobs,
        };
        if let Some(count) = count.filter(|_| step > 0.0 && step.is_finite()) {
            knobs.push(OpKnob {
                field: MotionField::Count,
                path: KnobPath::Line {
                    origin: rail - along * step,
                    along,
                },
                value: count,
                scale: KnobScale::Times(step),
                snap: KnobSnap::Count,
                out: along,
                shaft: Some(1.0),
                tone: KnobTone::Count,
            });
        }
        knobs
    }

    /// A circular pattern's knobs, for a new one once its axis and the
    /// bodies' box in the document's model (without the pattern) are
    /// known: the span's on the arc through the copies (the bodies' box
    /// centre turned about the axis), at the last copy (the step's in
    /// Spacing, a step a copy less one; none for Full 360°); and the
    /// count's on a slider running on along the arc's tangent past its
    /// end, 8 pixels a copy.
    fn circular_knobs(&self, session: &MotionSession) -> Vec<OpKnob> {
        if session.feature.is_some() {
            return Vec::new();
        }
        let (Some(centre), (_, Some([point, axis]))) =
            (self.committed_centre(session), self.reference_line(session))
        else {
            return Vec::new();
        };
        let Some(axis) = axis.try_normalize() else {
            return Vec::new();
        };
        let foot = point + axis * (centre - point).dot(axis);
        let radial = centre - foot;
        let radius = radial.length();
        let Some(radial) = radial.try_normalize() else {
            return Vec::new();
        };
        let count = Self::knob_value(session, MotionField::Count);
        let spread = Self::knob_value(session, MotionField::Spread);
        let (span, times) = match (session.mode, spread, count) {
            (PatternMode::Full, ..) => (std::f64::consts::TAU, None),
            (PatternMode::Total, Some(spread), _) => (spread, Some(1.0)),
            (PatternMode::Spacing, Some(step), Some(count)) => {
                (step * (count - 1.0), Some(count - 1.0))
            }
            _ => return Vec::new(),
        };
        let (sin, cos) = span.sin_cos();
        let end = foot + (radial * cos + axis.cross(radial) * sin) * radius;
        let tangent = axis.cross(radial) * cos - radial * sin;
        let mut knobs = Vec::new();
        if let (Some(times), Some(spread)) = (times, spread)
            && times > 0.0
        {
            knobs.push(OpKnob {
                field: MotionField::Spread,
                path: KnobPath::Arc {
                    centre: foot,
                    axis,
                    radial,
                    radius: KnobRadius::World(radius),
                },
                value: spread,
                scale: KnobScale::Times(times),
                snap: KnobSnap::Angle,
                out: tangent,
                shaft: Some(0.0),
                tone: KnobTone::Create,
            });
        }
        if let Some(count) = count {
            knobs.push(OpKnob {
                field: MotionField::Count,
                path: KnobPath::Line {
                    origin: end,
                    along: tangent,
                },
                value: count,
                scale: KnobScale::Pixels(COUNT_PIXELS),
                snap: KnobSnap::Count,
                out: tangent,
                shaft: Some(0.0),
                tone: KnobTone::Count,
            });
        }
        knobs
    }

    /// The centre of the session's bodies' box in the document's model,
    /// the last the committed document regenerated to (a merged body in
    /// its holder).
    fn committed_centre(&self, session: &MotionSession) -> Option<DVec3> {
        let (mesh, parts) = self.feed.committed()?;
        let shown = self.shown_bodies(&session.bodies);
        let positions = mesh.positions();
        let indices = mesh.indices();
        let mut bounds: Option<[DVec3; 2]> = None;
        for (part, body) in mesh.parts().zip(parts) {
            if !shown.contains(body) {
                continue;
            }
            for &index in indices.get(part.indices.clone())? {
                let at = glam::Vec3::from(*positions.get(index as usize)?).as_dvec3();
                bounds = Some(match bounds {
                    Some([low, high]) => [low.min(at), high.max(at)],
                    None => [at, at],
                });
            }
        }
        let [low, high] = bounds?;
        Some((low + high) / 2.0)
    }
}

/// The eight corners of the box from `low` to `high`.
fn box_corners(low: DVec3, high: DVec3) -> [DVec3; 8] {
    std::array::from_fn(|k| {
        DVec3::new(
            if k & 1 == 0 { low.x } else { high.x },
            if k & 2 == 0 { low.y } else { high.y },
            if k & 4 == 0 { low.z } else { high.z },
        )
    })
}
