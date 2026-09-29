//! The controls over the viewport's top-right corner: the projection
//! toggle, Home and the view cube.

use iced::widget::{button, container, mouse_area, row, text};
use iced::{Alignment, Element, Length, Padding, mouse};
use varde_render::{Camera, Projection};

use crate::chrome::icon_button;
use crate::icons::Icon;
use crate::theme::{self, Tone};
use crate::{Look, Message, view_cube};

/// Blocks the viewport under a control, so clicks and drags on it don't
/// orbit the camera.
fn block_viewport_drag<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    mouse_area(content)
        .interaction(mouse::Interaction::Idle)
        .into()
}

/// The projection toggle, Home and the view cube, for the top-right corner
/// of the viewport.
pub fn view_controls(camera: &Camera) -> Element<'_, Message> {
    let projection = |label, projection| {
        button(
            text(label)
                .size(12)
                .height(Length::Fill)
                .align_y(Alignment::Center),
        )
        .height(22)
        .padding([0, 8])
        .style(theme::flat_button(camera.projection() == projection))
        .on_press(Message::Look(Look::SetProjection(projection)))
    };

    let home = icon_button(
        Icon::Home,
        Tone::Muted,
        Some(Message::Look(Look::ResetCamera)),
    )
    .style(theme::float_button);

    let buttons = block_viewport_drag(
        row![
            container(
                row![
                    projection("Perspective", Projection::Perspective),
                    projection("Orthographic", Projection::Orthographic),
                ]
                .spacing(2)
            )
            .padding(2)
            .style(theme::float_panel),
            home,
        ]
        .spacing(6),
    );

    // The cube is not blocked: dragging from around it still orbits.
    row![
        container(buttons).padding(Padding::ZERO.top(6)),
        view_cube::view_cube(camera),
    ]
    .spacing(2)
    .into()
}
