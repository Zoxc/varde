//! A toast: a short message floating at the bottom middle of the
//! screen, over the status bar, for a few seconds, such as a name being
//! taken. The app keeps it and its time; the view only shows it.

use iced::widget::text::Wrapping;
use iced::widget::{container, text};
use iced::{Element, Length, Padding};

use crate::Message;
use crate::status::STATUS_BAR_ROOM;
use crate::theme;

/// How far over the status bar the toast floats, in pixels.
const GAP: f32 = 8.0;

/// The toast's layer: `message` in a card centred at the bottom of the
/// screen it's put over, clear of the status bar. It takes nothing, so
/// what's under it works as usual.
pub fn toast(message: &str) -> Element<'_, Message> {
    let card = container(text(message).wrapping(Wrapping::WordOrGlyph))
        .padding(Padding::from([8, 14]))
        .max_width(480)
        .style(theme::menu);
    container(card)
        .center_x(Length::Fill)
        .align_bottom(Length::Fill)
        .padding(Padding::ZERO.bottom(STATUS_BAR_ROOM + GAP))
        .into()
}
