//! A draft being set up: its own parts of the move's panel. The model
//! mock has no draft panel, so it's built in the style of its shell
//! panel: the Faces to draft (a face session's rows), the Neutral plane
//! (a mirror's plane row: an origin plane from the toolbar while it
//! picks, or a flat face clicked), the Angle, and the Flip and Tangent
//! faces ticks. The faces are picked and lit in the viewport as the
//! app's highlight; the neutral plane is drawn as a mirror's, with the
//! pull's arrow through it, in `viewport/motion.rs`.

use iced::Element;
use iced::widget::column;

use super::faces::faces_field;
use super::{MotionField, MotionLook, MotionState, PickedFaces};
use crate::icons::Icon;
use crate::operation_panel::toggle;
use crate::{Look, Message};

/// What the panel shows of a draft being set up, beside what every
/// move's session has.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftView {
    /// The faces to draft.
    pub faces: PickedFaces,
    /// Whether the pull runs against the neutral plane's normal.
    pub flip: bool,
    /// Whether faces running on smoothly from them are taken in.
    pub tangent: bool,
    /// What the status bar says of it once it's whole: "4 faces · 3° from
    /// XY".
    pub info: Option<String>,
}

/// The panel's body for a draft: Faces, Neutral plane (`neutral`, the
/// panel's reference field), Angle, Flip and Tangent faces.
pub(super) fn body<'a>(
    state: &MotionState<'a>,
    neutral: Element<'a, Message>,
    value: impl Fn(MotionField, &'a str) -> Element<'a, Message>,
) -> Element<'a, Message> {
    let Some(draft) = &state.draft else {
        return column![].into();
    };
    let send = |look: MotionLook| state.editable.then_some(Message::Look(Look::Motion(look)));
    let faces = faces_field(state, &draft.faces, "Faces", "Click faces");
    let flip = toggle(
        Icon::TkFlip,
        "Flip",
        draft.flip,
        send(MotionLook::Flip),
        Some("Pull against the plane's normal"),
    );
    let tangent = toggle(
        Icon::TkChain,
        "Tangent faces",
        draft.tangent,
        send(MotionLook::TangentFaces),
        Some("Take in faces that run on smoothly"),
    );
    column![
        faces,
        neutral,
        value(MotionField::Angle, "Angle"),
        flip,
        tangent,
    ]
    .spacing(10)
    .into()
}
