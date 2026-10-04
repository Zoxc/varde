//! A shell being set up: its own parts of the move's panel, the model
//! mock's shell panel: the faces to Remove (none for a closed hollow
//! body), the Thickness, and its Direction as tiles (Inward, Outward).
//! The faces are picked and lit in the viewport as the app's highlight;
//! nothing else is drawn.

use iced::Element;
use iced::widget::column;

use super::faces::faces_field;
use super::{MotionField, MotionLook, MotionState, PickedFaces, section};
use crate::icons::Icon;
use crate::operation_panel::{tile, tiles};
use crate::{Look, Message};

/// Which way a shell's walls grow from the body's faces, the mock's
/// Direction tiles: inside them, the body hollowed, or outside them, the
/// body becoming the hollow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ShellDirection {
    #[default]
    Inward,
    Outward,
}

impl ShellDirection {
    /// The two in the panel's order.
    pub const ALL: [ShellDirection; 2] = [ShellDirection::Inward, ShellDirection::Outward];

    /// The direction a shell stores as `outward`.
    pub fn of(outward: bool) -> Self {
        if outward {
            ShellDirection::Outward
        } else {
            ShellDirection::Inward
        }
    }

    /// Whether the walls grow outside the faces, as a shell stores it.
    pub fn outward(self) -> bool {
        self == ShellDirection::Outward
    }

    /// Its tile's label, the mock's: "Inward", "Outward".
    pub fn label(self) -> &'static str {
        match self {
            ShellDirection::Inward => "Inward",
            ShellDirection::Outward => "Outward",
        }
    }

    /// Its tile's icon, the mock's `sh-in` and `sh-out`.
    fn icon(self) -> Icon {
        match self {
            ShellDirection::Inward => Icon::ShIn,
            ShellDirection::Outward => Icon::ShOut,
        }
    }
}

/// What the panel shows of a shell being set up, beside what every
/// move's session has.
#[derive(Debug, Clone, PartialEq)]
pub struct ShellView {
    /// The faces to remove.
    pub faces: PickedFaces,
    pub direction: ShellDirection,
    /// What the status bar says of it once it's whole: "2 faces removed
    /// · 2 mm inward", "Closed · 1 mm outward".
    pub info: Option<String>,
}

/// The panel's body for a shell: Remove, Thickness and Direction's
/// tiles.
pub(super) fn body<'a>(
    state: &MotionState<'a>,
    value: impl Fn(MotionField, &'a str) -> Element<'a, Message>,
) -> Element<'a, Message> {
    let Some(shell) = &state.shell else {
        return column![].into();
    };
    let send = |look: MotionLook| state.editable.then_some(Message::Look(Look::Motion(look)));
    let faces = faces_field(state, &shell.faces, "Remove", "Click faces");
    let directions = ShellDirection::ALL.iter().map(|&direction| {
        tile(
            direction.icon(),
            direction.label(),
            shell.direction == direction,
            send(MotionLook::ShellDirection(direction)),
        )
    });
    column![
        faces,
        value(MotionField::Thickness, "Thickness"),
        section("Direction"),
        tiles(directions),
    ]
    .spacing(10)
    .into()
}
