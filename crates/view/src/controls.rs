//! The controls over the viewport's top-right corner: the view cube, and
//! Home under it at its right.

use iced::widget::{column, mouse_area};
use iced::{Alignment, Element};
use varde_render::Camera;

use crate::chrome::{icon_button, over_viewport};
use crate::icons::Icon;
use crate::theme::{self, Tone};
use crate::{Look, Message, view_cube};

/// The gap between the view cube and Home, in pixels.
const GAP: f32 = 2.0;

/// How tall the controls are, in pixels: the cube, the gap and Home.
pub(crate) const CONTROLS_HEIGHT: f32 = view_cube::SIZE + GAP + crate::icons::BUTTON_SIZE;

/// The view cube and Home, for the top-right corner of the viewport.
pub fn view_controls(camera: &Camera) -> Element<'_, Message> {
    let home = icon_button(
        Icon::Home,
        Tone::Muted,
        Some(Message::Look(Look::ResetCamera)),
    )
    .style(theme::float_button);

    // The cube is not blocked: dragging from around it still orbits. Over
    // it, the point the camera orbits is marked.
    let cube = mouse_area(view_cube::view_cube(camera))
        .on_enter(Message::Look(Look::HoverCube(true)))
        .on_exit(Message::Look(Look::HoverCube(false)));
    column![cube, over_viewport(home)]
        .spacing(GAP)
        .align_x(Alignment::End)
        .into()
}
