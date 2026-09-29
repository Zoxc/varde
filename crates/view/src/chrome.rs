//! Window chrome shared by every screen: the status bar, the theme and help
//! buttons, and the small widgets the screens share: icon buttons and key
//! chips.
//!
//! The OS draws the title bar; the app sets its text in `Varde::title`.

use iced::widget::{
    Button, Container, Rule, Text, button, column, container, row, rule, space, text,
};
use iced::{Alignment, Element, Font, Length};

use crate::Message;
use crate::icons::{self, Icon, MouseButton};
use crate::shortcut::KeyName;
use crate::theme::{self, Emphasis, Mode, Tone};

/// Includes the 1 px border.
const STATUS_BAR_HEIGHT: f32 = 28.0;

/// Puts a status bar under a screen's `content`, showing `info` on the left
/// and `hints` on the right.
pub fn window<'a>(
    content: impl Into<Element<'a, Message>>,
    info: impl Into<Element<'a, Message>>,
    hints: impl IntoIterator<Item = Element<'a, Message>>,
) -> Element<'a, Message> {
    column![
        container(content).width(Length::Fill).height(Length::Fill),
        status_bar(info.into(), hints),
    ]
    .into()
}

fn status_bar<'a>(
    info: Element<'a, Message>,
    hints: impl IntoIterator<Item = Element<'a, Message>>,
) -> Element<'a, Message> {
    let bar = row![
        info,
        space::horizontal(),
        row(hints).spacing(14).align_y(Alignment::Center),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    edged(
        container(bar)
            .width(Length::Fill)
            .padding([0, 12])
            .align_y(Alignment::Center)
            .style(theme::status_bar),
        Edge::Top,
        STATUS_BAR_HEIGHT,
    )
}

/// A 1 px horizontal separator.
pub fn hrule<'a>() -> Rule<'a> {
    rule::horizontal(1).style(theme::separator)
}

/// A 1 px vertical separator.
pub fn vrule<'a>() -> Rule<'a> {
    rule::vertical(1).style(theme::separator)
}

/// The side of a bar or panel that [`edged`] puts a separator along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Right,
}

/// `content` with a 1 px separator along its `edge`, `total` tall (or wide,
/// for [`Edge::Right`]) with the separator: iced has no one-sided borders.
pub fn edged<'a>(content: Container<'a, Message>, edge: Edge, total: f32) -> Element<'a, Message> {
    let inner = total - 1.0;
    match edge {
        Edge::Top => column![hrule(), content.height(inner)].into(),
        Edge::Bottom => column![content.height(inner), hrule()].into(),
        Edge::Right => row![content.width(inner), vrule()].into(),
    }
}

/// The theme toggle and the help button, for the top-right of a screen.
pub fn app_buttons<'a>(mode: Mode) -> Element<'a, Message> {
    let theme_toggle = icon_button(
        match mode {
            Mode::Light => Icon::Moon,
            Mode::Dark => Icon::Sun,
        },
        Tone::Muted,
        Some(Message::ToggleTheme),
    );
    // TODO: open the shortcut sheet.
    let help = icon_button(Icon::Help, Tone::Muted, None);

    row![theme_toggle, help]
        .spacing(4)
        .align_y(Alignment::Center)
        .into()
}

/// An icon-only button. Enabled, it is `tone` at rest and turns to the text
/// colour on hover; without a `message` it is inert and faint.
pub fn icon_button(icon: Icon, tone: Tone, message: Option<Message>) -> Button<'static, Message> {
    let enabled = message.is_some();
    button(icons::button_icon(icon, move |p, hovered| {
        theme::flat_content(p, tone, enabled, hovered)
    }))
    .padding(0)
    .style(theme::flat_button(false))
    .on_press_maybe(message)
}

/// A small button, for a banner or beside a card.
pub fn small_button(label: &str, emphasis: Emphasis) -> Button<'_, Message> {
    button(text(label).size(12))
        .padding([2, 10])
        .style(emphasis.button_style())
}

/// A status bar hint: `key` does `label`.
pub fn key_hint<'a>(key: impl Into<KeyName>, label: &'a str) -> Element<'a, Message> {
    hint(key_chip(key, ChipSize::Normal), label)
}

/// A status bar hint: using the mouse `button` does `label`.
pub fn mouse_hint<'a>(button: MouseButton, label: &'a str) -> Element<'a, Message> {
    hint(icons::mouse(button), label)
}

fn hint<'a>(input: impl Into<Element<'a, Message>>, label: &'a str) -> Element<'a, Message> {
    row![input.into(), text(label).size(12)]
        .spacing(5)
        .align_y(Alignment::Center)
        .into()
}

/// The size of a [`key_chip`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipSize {
    /// For inside a side panel tab.
    Small,
    Normal,
}

/// A key name in a small bordered box. Restyle it with
/// [`Container::style`] to put it on another background.
pub fn key_chip<'a>(key: impl Into<KeyName>, size: ChipSize) -> Container<'a, Message> {
    let (font_size, padding) = match size {
        ChipSize::Small => (9.5, [1, 4]),
        ChipSize::Normal => (10.5, [2, 5]),
    };
    container(
        text(key.into().label())
            .size(font_size)
            .font(Font::MONOSPACE),
    )
    .padding(padding)
    .style(theme::key_chip)
}

/// A menu item's shortcut, as plain faint text.
pub fn key_label<'a>(key: impl Into<KeyName>) -> Text<'a> {
    text(key.into().label()).size(11).style(theme::faint_text)
}
