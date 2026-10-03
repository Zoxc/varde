//! Window chrome shared by every screen: the floating status bar over it
//! (`crate::status`), the theme and help buttons, and the small widgets the
//! screens share: icon buttons, key chips and the status bar's hints.
//!
//! The OS draws the title bar; the app sets its text in `Varde::title`.

use std::borrow::Cow;

use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{
    Button, Column, Container, Rule, Scrollable, Text, button, column, container, opaque, row,
    rule, scrollable, stack, text, tooltip,
};
use iced::{Alignment, Element, Font, Length};

use crate::Message;
use crate::icons::{self, Icon, MouseButton};
use crate::shortcut::KeyName;
use crate::theme::{self, Emphasis, ThemeChoice, Tone};

/// `content` telling `tip` below it while hovered, in a small box no
/// wider than about one and a half side panels.
pub(crate) fn tip<'a>(
    content: impl Into<Element<'a, Message>>,
    tip: Text<'a>,
) -> Element<'a, Message> {
    tooltip(
        content,
        container(tip.size(12))
            .padding([3, 6])
            .max_width(theme::SIDE_PANEL_WIDTH * 1.5)
            .style(theme::menu),
        tooltip::Position::Bottom,
    )
    .into()
}

/// `content` telling `label` and its `key` at once while hovered, right
/// of it, kept within the window: for a tool on the rail.
pub(crate) fn side_tip<'a>(
    content: impl Into<Element<'a, Message>>,
    label: &'a str,
    key: impl Into<KeyName>,
) -> Element<'a, Message> {
    let tip = row![text(label).size(12), key_chip(key, ChipSize::Small)]
        .spacing(6)
        .align_y(Alignment::Center);
    tooltip(
        content,
        container(tip).padding([3, 6]).style(theme::menu),
        tooltip::Position::Right,
    )
    .gap(6)
    .snap_within_viewport(true)
    .into()
}

/// `content` with `status`, the floating status bar's layer
/// ([`crate::status::status_bar`]), over its bottom right.
pub fn window<'a>(
    content: impl Into<Element<'a, Message>>,
    status: Element<'a, Message>,
) -> Element<'a, Message> {
    stack![
        container(content).width(Length::Fill).height(Length::Fill),
        status
    ]
    .into()
}

/// A 1 px horizontal separator.
pub fn hrule<'a>() -> Rule<'a> {
    rule::horizontal(1).style(theme::separator)
}

/// A small heading in a panel or menu, or of a group in a list.
pub(crate) fn heading<'a>(label: &'a str) -> Text<'a> {
    text(label)
        .size(11.5)
        .font(theme::SEMIBOLD)
        .style(theme::muted_text)
}

/// `content` scrolled up and down with the app's one scrollbar: a thin
/// faint scroller on no rail ([`theme::scrollbar`]), `margin` in from the
/// right edge. It floats over the content, so leave it room in a padding
/// (or embed it with `spacing`).
pub fn scrolled<'a>(
    content: impl Into<Element<'a, Message>>,
    margin: f32,
) -> Scrollable<'a, Message> {
    let scrollbar = Scrollbar::new()
        .width(theme::SCROLLBAR_WIDTH)
        .scroller_width(theme::SCROLLBAR_WIDTH)
        .margin(margin);
    scrollable(content)
        .direction(Direction::Vertical(scrollbar))
        .style(theme::scrollbar)
}

/// A 1 px vertical separator.
pub fn vrule<'a>() -> Rule<'a> {
    rule::vertical(1).style(theme::separator)
}

/// The side of a bar or panel that [`edged`] puts a separator along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Bottom,
    Right,
}

/// `content` with a 1 px separator along its `edge`, `total` tall (or wide,
/// for [`Edge::Right`]) with the separator: iced has no one-sided borders.
pub fn edged<'a>(content: Container<'a, Message>, edge: Edge, total: f32) -> Element<'a, Message> {
    let inner = total - 1.0;
    match edge {
        Edge::Bottom => column![content.height(inner), hrule()].into(),
        Edge::Right => row![content.width(inner), vrule()].into(),
    }
}

/// The theme button, showing the `theme` chosen and going on to the next,
/// and the help button, for the top-right of a screen.
pub fn app_buttons<'a>(theme: ThemeChoice) -> Element<'a, Message> {
    let (icon, label) = match theme {
        ThemeChoice::Auto => (Icon::Contrast, "Theme: as the system's"),
        ThemeChoice::Light => (Icon::Sun, "Theme: light"),
        ThemeChoice::Dark => (Icon::Moon, "Theme: dark"),
    };
    let theme_toggle = tip(
        icon_button(icon, Tone::Muted, Some(Message::CycleTheme)),
        text(label),
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

/// What the button framing the camera on a failure's geometry says.
pub(crate) const SHOW_FAILURE: &str = "Show";

/// A small button, for a banner or beside a card.
pub fn small_button(label: &str, emphasis: Emphasis) -> Button<'_, Message> {
    button(text(label).size(12))
        .padding([2, 10])
        .style(emphasis.button_style())
}

/// A status bar hint, and whether it's of the mouse, which the status bar
/// leaves out with its mouse hints turned off.
pub struct Hint<'a> {
    pub(crate) element: Element<'a, Message>,
    pub(crate) mouse: bool,
}

/// A status bar hint: `key` does `label`.
pub fn key_hint<'a>(key: impl Into<KeyName>, label: &'a str) -> Hint<'a> {
    hint(key_chip(key, ChipSize::Normal), label, false)
}

/// A status bar hint: using the mouse `button` does `label`.
pub fn mouse_hint<'a>(button: MouseButton, label: &'a str) -> Hint<'a> {
    hint(icons::mouse(button), label, true)
}

/// A status bar hint: double-clicking the mouse `button` does `label`.
pub fn double_hint<'a>(button: MouseButton, label: &'a str) -> Hint<'a> {
    hint(
        row![icons::mouse(button), icons::mouse(button)].spacing(1),
        label,
        true,
    )
}

/// A status bar hint: using the mouse `button` with `key` held does `label`.
pub fn chord_hint<'a>(key: impl Into<KeyName>, button: MouseButton, label: &'a str) -> Hint<'a> {
    hint(
        row![key_chip(key, ChipSize::Normal), icons::mouse(button)]
            .spacing(3)
            .align_y(Alignment::Center),
        label,
        true,
    )
}

fn hint<'a>(input: impl Into<Element<'a, Message>>, label: &'a str, mouse: bool) -> Hint<'a> {
    Hint {
        element: row![input.into(), text(label).size(12)]
            .spacing(5)
            .align_y(Alignment::Center)
            .into(),
        mouse,
    }
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

/// `message` as a sentence on its own: its first letter capitalised.
/// Error messages start in lower case, to follow a colon ("Couldn't
/// regenerate: its regions are too complex"); where one stands alone,
/// in a panel or a tooltip, it's shown through this. A letter whose
/// capital is two ("ß", "ﬁ") gets the first in upper case and the rest
/// in lower, as title case has it ("Ss", "Fi").
pub fn sentence(message: &str) -> Cow<'_, str> {
    let mut chars = message.chars();
    match chars.next() {
        Some(first) if first.is_lowercase() => {
            let mut upper = first.to_uppercase();
            let mut sentence: String = upper.next().into_iter().collect();
            sentence.extend(upper.flat_map(char::to_lowercase));
            sentence.push_str(chars.as_str());
            Cow::Owned(sentence)
        }
        _ => Cow::Borrowed(message),
    }
}

/// `content` as a dialog over the whole screen, which dims the rest and
/// keeps it from being clicked.
pub(crate) fn dialog<'a>(content: Column<'a, Message>) -> Element<'a, Message> {
    let dialog = container(content.width(380)).padding(18).style(theme::menu);
    opaque(
        container(opaque(dialog))
            .center(Length::Fill)
            .style(theme::scrim),
    )
}

/// A dialog's button, labelled `label`, in `style`, sending `message`,
/// or disabled without one.
pub(crate) fn dialog_button<'a>(
    label: &'a str,
    style: fn(&iced::Theme, button::Status) -> button::Style,
    message: Option<Message>,
) -> Button<'a, Message> {
    button(text(label).font(theme::SEMIBOLD))
        .padding([6, 14])
        .style(style)
        .on_press_maybe(message)
}
