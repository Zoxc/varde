//! The menus' building blocks: their items, the choices and submenus
//! among them, and the lines between their groups.

use std::borrow::Cow;

use iced::widget::{Button, Container, button, container, row, text};
use iced::{Alignment, Element, Length};

use crate::Message;
use crate::chrome::{hrule, key_label};
use crate::icons::{self, Icon};
use crate::shortcut::Shortcut;
use crate::theme::{self, Tone};

/// A menu's item: `icon`, `label` and the `key` that does the same if
/// any, sending `message`; disabled without one.
pub(crate) fn menu_item(
    icon: Icon,
    label: Cow<'static, str>,
    key: Option<Shortcut>,
    message: Option<Message>,
) -> Button<'static, Message> {
    let key = key.map(|key| key_label(key).into());
    menu_row(icon, label, key, message)
}

/// A menu's item that's one of a set of choices with icons of their own:
/// `icon` and `label`, sending `message`, with a tick at its right while
/// `chosen`.
pub(crate) fn choice_item(
    icon: Icon,
    label: &'static str,
    chosen: bool,
    message: Message,
) -> Button<'static, Message> {
    let tick = chosen.then(|| {
        icons::tinted(Icon::Check, icons::INLINE, |p| {
            theme::flat_content(p, Tone::Text, true, false)
        })
        .into()
    });
    menu_row(icon, label.into(), tick, Some(message))
}

/// A menu's item opening a submenu to its left: `icon` and `label`, with a
/// chevron at its right pointing to where it opens, sending `message`;
/// lit while `open`.
pub(crate) fn submenu_item(
    icon: Icon,
    label: &'static str,
    open: bool,
    message: Message,
) -> Button<'static, Message> {
    let chevron = icons::tinted(Icon::ChevLeft, icons::INLINE, |p| {
        theme::flat_content(p, Tone::Muted, true, false)
    })
    .into();
    menu_row(icon, label.into(), Some(chevron), Some(message))
        .style(theme::flat_button(open, theme::Tone::Text))
}

/// A menu's item: `icon`, `label` and what's at its `right` if anything,
/// sending `message`; disabled without one.
fn menu_row(
    icon: Icon,
    label: Cow<'static, str>,
    right: Option<Element<'static, Message>>,
    message: Option<Message>,
) -> Button<'static, Message> {
    let enabled = message.is_some();
    let key = right.map(|right| container(right).align_right(Length::Fill));
    button(
        row![
            // In its own colours, or text-toned, so hovering doesn't change
            // it; faint while disabled.
            if enabled && icon.category().is_some() {
                icons::icon(icon, icons::INLINE)
            } else {
                icons::tinted(icon, icons::INLINE, move |p| {
                    theme::flat_content(p, Tone::Text, enabled, false)
                })
                .into()
            },
            text(label),
            key,
        ]
        .spacing(MENU_ITEM_SPACING)
        .height(Length::Fill)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(MENU_ITEM_HEIGHT)
    .padding([0, 8])
    .style(theme::flat_button(false, theme::Tone::Text))
    .on_press_maybe(message)
}

/// How tall a menu's item is, in pixels.
pub(crate) const MENU_ITEM_HEIGHT: f32 = 28.0;

/// The space between a menu item's icon and its label.
pub(crate) const MENU_ITEM_SPACING: f32 = 10.0;

/// A line between a menu's groups of items.
pub(crate) fn menu_separator<'a>() -> Container<'a, Message> {
    container(hrule()).padding([4, 2])
}

/// The icon left of a menu item that's a choice: a tick while `on`.
pub(crate) fn ticked(on: bool) -> Icon {
    if on { Icon::Check } else { Icon::Blank }
}
