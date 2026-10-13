//! An offset face being set up: its own parts of the move's panel. The
//! model mock has no offset face panel, so it's built in the style of
//! its shell panel: the Faces to move (a face session's rows), the
//! Distance, and the Inward and Tangent faces ticks. The faces are
//! picked and lit in the viewport as the app's highlight; its handle,
//! an arrow and knob along the first face's normal, is an operation knob
//! (`viewport/knobs.rs`).

use iced::Element;
use iced::widget::column;

use super::faces::faces_field;
use super::{MotionField, MotionLook, MotionState, PickedFaces};
use crate::Message;
use crate::icons::Icon;
use crate::operation_panel::toggle;

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
    /// What the status bar says of it once it's whole: "2 faces · 2 mm
    /// outward".
    pub info: Option<String>,
}

/// The panel's body for an offset face: Faces, Distance, Inward and
/// Tangent faces.
pub(super) fn body<'a>(
    state: &MotionState<'a>,
    value: impl Fn(MotionField) -> Element<'a, Message>,
) -> Element<'a, Message> {
    let Some(offset) = state.offset_face() else {
        return column![].into();
    };
    let faces = faces_field(state, &offset.faces, "Faces", "Click faces");
    let inward = toggle(
        Icon::TkFlip,
        "Inward",
        offset.inward,
        state.send(MotionLook::Flip),
        Some("Into the body, shrinking it"),
    );
    let tangent = toggle(
        Icon::TkChain,
        "Tangent faces",
        offset.tangent,
        state.send(MotionLook::TangentFaces),
        Some("Take in faces that run on smoothly"),
    );
    column![faces, value(MotionField::Distance), inward, tangent,]
        .spacing(10)
        .into()
}
