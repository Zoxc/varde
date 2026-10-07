//! A chamfer in the move's session ([`MotionKind::Chamfer`]): its edges
//! picked as a blend's (see `blend`), how it's sized (the model mock's
//! Type tiles: Equal, Two distances, Distance and angle), its fields,
//! Flip sides and Tangent chain.

use varde_document::{Chamfer, ChamferSize, Design, FeatureKind};
use varde_expr::Value;
use varde_render::Camera;
use varde_view::{ChamferType, ChamferView, MotionField, MotionKind};

use super::blend::BlendSetup;
use super::{BLEND_SHARE, Doc, MotionSession, length_field};
use crate::doc::camera::fitting_length;
use crate::doc::regions::TypedText;

/// What `field` of a chamfer is read with in `design`, if it's one of
/// its own: a distance as an extrude's, the angle above 0 and under 90°.
pub(super) fn chamfer_ask<'p>(
    field: MotionField,
    design: &Design<'p>,
) -> Option<varde_expr::Ask<'p>> {
    match field {
        MotionField::ChamferDistance | MotionField::ChamferSecond => {
            Some(Chamfer::distance_ask(design))
        }
        MotionField::ChamferAngle => Some(Chamfer::angle_ask(design)),
        _ => None,
    }
}

/// The chamfer's fields as a new one opens them, seen by `camera`: a
/// distance of half [`BLEND_SHARE`] of the view's height made nice
/// ([`fitting_length`]) and twice it, in the design's units with their
/// symbol, and 45°.
pub(super) fn chamfer_fields(design: &Design, camera: &Camera) -> [TypedText; 3] {
    let distance = Chamfer::distance_ask(design);
    let angle = Chamfer::angle_ask(design);
    let first = fitting_length(camera, design.units, BLEND_SHARE / 2.0);
    [
        length_field(first, &distance, design),
        length_field(2.0 * first, &distance, design),
        TypedText::read("45°".to_owned(), &angle),
    ]
}

impl MotionSession {
    /// The chamfer as set up, if it's whole: its edges, and its type's
    /// values as they last read. Equal is the same either way round, so
    /// it's stored unflipped.
    pub(super) fn chamfer(&self) -> Option<Chamfer> {
        if self.blend.edges.refs.is_empty() {
            return None;
        }
        let value = |field: MotionField| self.field(field).value.clone();
        let distance = value(MotionField::ChamferDistance)?;
        let distances = match self.chamfer_type {
            ChamferType::Equal => ChamferSize::Equal(distance),
            ChamferType::Two => ChamferSize::Two(distance, value(MotionField::ChamferSecond)?),
            ChamferType::Angle => ChamferSize::Angle(distance, value(MotionField::ChamferAngle)?),
        };
        Some(Chamfer {
            edges: self.blend.edges.refs.clone(),
            distances,
            chains: self.blend.chains,
            flip: self.flip && self.chamfer_type != ChamferType::Equal,
        })
    }

    /// Opens the chamfer `chamfer` in this session: its edges, Tangent
    /// chain, type, values and Flip sides.
    pub(super) fn open_chamfer(&mut self, chamfer: &Chamfer) {
        let design = &self.read_in.design();
        let distance = Chamfer::distance_ask(design);
        let mut set = |field: MotionField, value: &Value, ask: &varde_expr::Ask| {
            self.fields[field.index()] = TypedText::of(value, ask);
        };
        let kind = match &chamfer.distances {
            ChamferSize::Equal(d) => {
                set(MotionField::ChamferDistance, d, &distance);
                ChamferType::Equal
            }
            ChamferSize::Two(a, b) => {
                set(MotionField::ChamferDistance, a, &distance);
                set(MotionField::ChamferSecond, b, &distance);
                ChamferType::Two
            }
            ChamferSize::Angle(d, a) => {
                set(MotionField::ChamferDistance, d, &distance);
                set(MotionField::ChamferAngle, a, &Chamfer::angle_ask(design));
                ChamferType::Angle
            }
        };
        self.chamfer_type = kind;
        self.flip = chamfer.flip;
        self.blend = BlendSetup::of(&chamfer.edges, chamfer.chains);
        self.blend_body();
    }
}

impl Doc {
    /// What the panel shows of the chamfer being set up.
    pub(super) fn chamfer_view(&self, session: &MotionSession) -> ChamferView {
        let units = self.editor.document().units();
        ChamferView {
            edges: self.blend_edges(session),
            kind: session.chamfer_type,
            info: (session.chamfer())
                .filter(|_| session.kind == MotionKind::Chamfer)
                .map(|chamfer| varde_view::chamfer_info(&chamfer, units)),
        }
    }
}

/// The feature a chamfer session makes.
pub(super) fn chamfer_kind(session: &MotionSession) -> Option<FeatureKind> {
    session.chamfer().map(FeatureKind::Chamfer)
}
