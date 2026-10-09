//! A fillet being set up: its own parts of the move's panel, the model
//! mock's fillet panel: the Edges picked, the Radius and the Tangent
//! chain tick. The edges are picked and lit in the viewport as the app's
//! highlight; nothing else is drawn.

use iced::Element;
use iced::widget::column;

use super::blend::{chain_toggle, edges_field};
use super::{BlendEdges, MotionField, MotionState};
use crate::Message;

/// What the panel shows of a fillet being set up, beside what every
/// move's session has.
#[derive(Debug, Clone, PartialEq)]
pub struct FilletView {
    pub edges: BlendEdges,
    /// What the status bar says of it once it's whole: "2 edges · R2 mm
    /// · Tangent chain".
    pub info: Option<String>,
}

/// The panel's body for a fillet: Edges, Radius and Tangent chain.
pub(super) fn body<'a>(
    state: &MotionState<'a>,
    value: impl Fn(MotionField, &'a str) -> Element<'a, Message>,
) -> Element<'a, Message> {
    let Some(fillet) = &state.fillet else {
        return column![].into();
    };
    column![
        edges_field(state, &fillet.edges, "Click edges or faces"),
        value(MotionField::Radius, "Radius"),
        chain_toggle(state, &fillet.edges),
    ]
    .spacing(10)
    .into()
}
