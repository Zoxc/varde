//! The screens' shortcuts, matched against the view's bindings, and the
//! filters of the other keys and window events the app listens for.

use iced::keyboard::{self, key};
use iced::window;
use varde_view::{File, Held, Look, Message as Ui, Unsaved};

use crate::Message;
use crate::doc::Dialog;

/// The welcome screen's shortcuts, see [`varde_view::welcome_bindings`],
/// and the settings page's, if they're `on`: not while a prompt shows
/// over it.
pub(crate) fn welcome_key((on, event): (bool, keyboard::Event)) -> Option<Message> {
    let bindings =
        (varde_view::welcome_bindings().into_iter()).chain([varde_view::settings_binding()]);
    on.then(|| pressed(event, bindings)).flatten()
}

/// The document screen's shortcuts, given what they depend on, see
/// [`varde_view::document_bindings`]: none without it, see [`Doc::keys`].
///
/// [`Doc::keys`]: crate::doc::Doc::keys
pub(crate) fn document_key(
    (keys, event): (Option<varde_view::DocumentKeys>, keyboard::Event),
) -> Option<Message> {
    pressed(event, varde_view::document_bindings(keys?))
}

/// The message of the first enabled binding of `bindings` that `event`
/// presses, if any.
fn pressed(
    event: keyboard::Event,
    bindings: impl IntoIterator<Item = varde_view::Binding>,
) -> Option<Message> {
    let keyboard::Event::KeyPressed {
        key,
        modifiers,
        repeat: false,
        ..
    } = event
    else {
        return None;
    };
    varde_view::pressed(bindings, &key, modifiers).map(Message::Ui)
}

/// Holding the peek key peeks at the other side panel tab.
pub(crate) fn peek_key(event: keyboard::Event) -> Option<Message> {
    match event {
        keyboard::Event::ModifiersChanged(modifiers) => {
            Some(Message::PeekPanel(Held::PEEK.is_held(modifiers)))
        }
        _ => None,
    }
}

/// Tells whether the command modifier is held, see [`Varde::command`](crate::Varde::command).
pub(crate) fn command_key(event: keyboard::Event) -> Option<Message> {
    match event {
        keyboard::Event::ModifiersChanged(modifiers) => {
            Some(Message::CommandHeld(modifiers.command()))
        }
        _ => None,
    }
}

/// Escape, or Tab where no shortcut takes it (`tab_taken`), given the
/// prompt the user is being asked, if any: cancels it (staying, or
/// deleting nothing), and otherwise backs out of what's open, see
/// [`Look::Escape`].
pub(crate) fn escape_key(
    ((dialog, tab_taken, page), event): (
        (Option<Dialog>, bool, Option<PageEscape>),
        keyboard::Event,
    ),
) -> Option<Message> {
    // Escape with any modifiers, unlike a shortcut.
    let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
        return None;
    };
    let tab = key == keyboard::Key::Named(key::Named::Tab);
    if !varde_view::escapes(&key, modifiers) || (tab && tab_taken) {
        return None;
    }
    // The settings page, over what's open in the document, takes it
    // before that does, but not before a prompt over it: Escape cancels
    // Reset's or closes the page, and Tab does nothing there.
    if let (None, Some(page)) = (dialog, page) {
        let message = match page {
            PageEscape::CancelReset => Ui::Settings(varde_view::SettingsPage::CancelReset),
            PageEscape::Close => Ui::ToggleSettings,
        };
        return (!tab).then_some(Message::Ui(message));
    }
    Some(Message::Ui(match dialog {
        Some(Dialog::Unsaved) => Ui::File(File::Unsaved(Unsaved::Cancel)),
        Some(Dialog::Delete) => Ui::Look(Look::CancelDelete),
        Some(Dialog::McpAsk) => Ui::McpPanel(varde_view::McpPanel::KeepEditing),
        Some(Dialog::McpRevert) => Ui::McpPanel(varde_view::McpPanel::CancelRevert),
        Some(Dialog::Damaged) => Ui::Welcome(varde_view::Welcome::CancelDamaged),
        Some(Dialog::Panic) => Ui::Welcome(varde_view::Welcome::ClosePanic),
        Some(Dialog::Naming) => Ui::File(File::CancelName),
        Some(Dialog::DeleteFromBrowser) => Ui::Welcome(varde_view::Welcome::CancelDelete),
        None => Ui::Look(Look::Escape),
    }))
}

/// What `Esc` does on the settings page, see `Varde::page_escape`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PageEscape {
    CancelReset,
    Close,
}

/// `Enter`, as Reset to defaults asks: resets.
pub(crate) fn reset_key(event: keyboard::Event) -> Option<Message> {
    match event {
        keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(key::Named::Enter),
            repeat: false,
            ..
        } => Some(Message::Ui(Ui::Settings(varde_view::SettingsPage::Reset))),
        _ => None,
    }
}

/// Stops peeking once the window loses focus.
pub(crate) fn unfocused((_, event): (window::Id, window::Event)) -> Option<Message> {
    matches!(event, window::Event::Unfocused).then_some(Message::PeekPanel(false))
}
