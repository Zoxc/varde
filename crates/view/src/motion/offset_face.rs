//! An offset face being set up: its own parts of the move's panel. The
//! model mock has no offset face panel, so it's built in the style of
//! its shell panel: the Faces to move (a face session's rows), the
//! Distance, and the Inward and Tangent faces ticks. The faces are
//! picked and lit in the viewport as the app's highlight; its handle,
//! the extrude's arrow and knob along the first face's normal, is drawn
//! and dragged in `viewport/motion.rs`.

use glam::DVec3;
use iced::Element;
use iced::widget::column;

use super::faces::faces_field;
use super::{MotionField, MotionLook, MotionState, PickedFaces};
use crate::icons::Icon;
use crate::operation_panel::toggle;
use crate::{Look, Message};

/// An offset face's handle: an arrow from the first face's point along
/// its outward normal, as the face is before the offset, with a knob at
/// the distance the faces move (negative inward), which dragging sets,
/// through zero to the other side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceHandle {
    /// The first face's point, on the face as the offset finds it.
    pub origin: DVec3,
    /// The face's outward normal there, unit.
    pub normal: DVec3,
    /// How far along the normal the knob is, in millimetres: the
    /// distance, negative inward; zero while the distance doesn't read.
    pub at: f64,
}

/// What the panel shows of an offset face being set up, beside what
/// every move's session has.
#[derive(Debug, Clone, PartialEq)]
pub struct OffsetFaceView {
    /// The faces to move.
    pub faces: PickedFaces,
    /// Whether they move into the body.
    pub inward: bool,
    /// Whether faces running on smoothly from them are taken in.
    pub tangent: bool,
    /// Its handle, once the first face is found on the model shown.
    pub handle: Option<FaceHandle>,
    /// What the status bar says of it once it's whole: "2 faces · 2 mm
    /// outward".
    pub info: Option<String>,
}

/// The panel's body for an offset face: Faces, Distance, Inward and
/// Tangent faces.
pub(super) fn body<'a>(
    state: &MotionState<'a>,
    value: impl Fn(MotionField, &'a str) -> Element<'a, Message>,
) -> Element<'a, Message> {
    let Some(offset) = &state.offset_face else {
        return column![].into();
    };
    let send = |look: MotionLook| state.editable.then_some(Message::Look(Look::Motion(look)));
    let faces = faces_field(state, &offset.faces, "Faces", "Click faces");
    let inward = toggle(
        Icon::TkFlip,
        "Inward",
        offset.inward,
        send(MotionLook::Flip),
        Some("Into the body, shrinking it"),
    );
    let tangent = toggle(
        Icon::TkChain,
        "Tangent faces",
        offset.tangent,
        send(MotionLook::TangentFaces),
        Some("Take in faces that run on smoothly"),
    );
    column![
        faces,
        value(MotionField::Distance, "Distance"),
        inward,
        tangent,
    ]
    .spacing(10)
    .into()
}
