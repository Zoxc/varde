//! The keyboard shortcuts and held modifiers: what the app matches key
//! presses against and the labels the view shows for them, so the two
//! can't disagree, and which message each shortcut sends on which screen.

use std::borrow::Cow;

use iced::keyboard::Modifiers;

use crate::{File, Message, Welcome};

/// A key pressed on its own, or with the platform's command modifier
/// (`Ctrl`, or `Cmd` on macOS) and maybe Shift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    /// The key's character, an ASCII lower case letter.
    key: char,
    shift: bool,
    command: bool,
}

impl Shortcut {
    pub const NEW: Self = Self::plain('n');
    pub const OPEN: Self = Self::plain('o');
    pub const SAVE: Self = Self::command('s');
    pub const SAVE_AS: Self = Self {
        shift: true,
        ..Self::SAVE
    };

    const fn plain(key: char) -> Self {
        assert!(key.is_ascii_lowercase());
        Self {
            key,
            shift: false,
            command: false,
        }
    }

    const fn command(key: char) -> Self {
        Self {
            command: true,
            ..Self::plain(key)
        }
    }

    /// The shortcut as shown on a key chip or in a menu: `Ctrl Shift S`,
    /// or `Cmd Shift S` on macOS, matching [`Modifiers::command`].
    pub fn label(self) -> String {
        let command = if cfg!(target_os = "macos") {
            "Cmd "
        } else {
            "Ctrl "
        };
        let command = if self.command { command } else { "" };
        let shift = if self.shift { "Shift " } else { "" };
        format!("{command}{shift}{}", self.key.to_ascii_uppercase())
    }

    /// Whether pressing `key` with `modifiers` is this shortcut. Alt, and
    /// any modifier the shortcut doesn't ask for, rule it out.
    pub fn matches(self, key: &str, modifiers: Modifiers) -> bool {
        let command = if self.command {
            modifiers.command()
        } else {
            !modifiers.control() && !modifiers.logo()
        };
        let mut chars = key.chars();
        let is_key = chars
            .next()
            .is_some_and(|c| c.eq_ignore_ascii_case(&self.key))
            && chars.next().is_none();
        is_key && command && modifiers.shift() == self.shift && !modifiers.alt()
    }
}

/// A shortcut and the message it sends. The view shows the shortcut on the
/// control sending the message, and the app matches key presses against
/// it, so the key does what the control does: nothing while the control
/// is disabled.
#[derive(Debug, Clone)]
pub struct Binding {
    pub(crate) shortcut: Shortcut,
    message: Message,
    enabled: bool,
}

impl Binding {
    fn new(shortcut: Shortcut, message: Message, enabled: bool) -> Self {
        Self {
            shortcut,
            message,
            enabled,
        }
    }

    /// The message it sends, unless it's disabled.
    pub fn sends(&self) -> Option<Message> {
        self.enabled.then(|| self.message.clone())
    }
}

/// The welcome screen's shortcuts: New design and Open, in that order.
pub fn welcome_bindings() -> [Binding; 2] {
    [
        Binding::new(Shortcut::NEW, Message::Welcome(Welcome::NewDesign), true),
        Binding::new(Shortcut::OPEN, Message::Welcome(Welcome::Open), true),
    ]
}

/// The document screen's shortcuts: Save and Save As, in that order. Save
/// is disabled unless the document is `editable`.
pub fn document_bindings(editable: bool) -> [Binding; 2] {
    [
        Binding::new(Shortcut::SAVE, Message::File(File::Save), editable),
        Binding::new(Shortcut::SAVE_AS, Message::File(File::SaveAs), true),
    ]
}

/// The message of the first enabled binding of `bindings` that pressing
/// `key` with `modifiers` is, if any.
pub fn pressed(
    bindings: impl IntoIterator<Item = Binding>,
    key: &str,
    modifiers: Modifiers,
) -> Option<Message> {
    bindings
        .into_iter()
        .find(|binding| binding.enabled && binding.shortcut.matches(key, modifiers))
        .map(|binding| binding.message)
}

/// A modifier held down, rather than a key pressed.
#[derive(Debug, Clone, Copy)]
pub struct Held {
    is_held: fn(Modifiers) -> bool,
    label: &'static str,
    /// The label on macOS.
    mac_label: &'static str,
}

impl Held {
    /// Held to peek at the other side panel tab.
    pub const PEEK: Self = Self {
        is_held: Modifiers::alt,
        label: "Alt",
        mac_label: "Option",
    };

    /// Whether it's held among `modifiers`.
    pub fn is_held(self, modifiers: Modifiers) -> bool {
        (self.is_held)(modifiers)
    }

    /// The modifier as shown on a key chip: `Alt`, or `Option` on macOS.
    pub fn label(self) -> &'static str {
        if cfg!(target_os = "macos") {
            self.mac_label
        } else {
            self.label
        }
    }
}

/// A key as the view shows it, on a key chip or in a menu: a shortcut or a
/// held modifier, so the view can only show keys the app matches.
#[derive(Debug, Clone, Copy)]
pub enum KeyName {
    Press(Shortcut),
    Held(Held),
}

impl From<Shortcut> for KeyName {
    fn from(shortcut: Shortcut) -> Self {
        KeyName::Press(shortcut)
    }
}

impl From<Held> for KeyName {
    fn from(held: Held) -> Self {
        KeyName::Held(held)
    }
}

impl KeyName {
    pub fn label(self) -> Cow<'static, str> {
        match self {
            KeyName::Press(shortcut) => shortcut.label().into(),
            KeyName::Held(held) => held.label().into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(Shortcut::NEW.label(), "N");
        let command = if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Ctrl"
        };
        assert_eq!(Shortcut::SAVE.label(), format!("{command} S"));
        assert_eq!(Shortcut::SAVE_AS.label(), format!("{command} Shift S"));
    }

    #[test]
    fn matches() {
        let none = Modifiers::empty();
        assert!(Shortcut::NEW.matches("n", none));
        assert!(!Shortcut::NEW.matches("n", Modifiers::CTRL));
        assert!(!Shortcut::NEW.matches("N", Modifiers::SHIFT));
        assert!(!Shortcut::NEW.matches("o", none));
        assert!(!Shortcut::NEW.matches("nn", none));
        assert!(!Shortcut::NEW.matches("", none));

        let command = Modifiers::COMMAND;
        assert!(Shortcut::SAVE.matches("s", command));
        assert!(!Shortcut::SAVE.matches("s", none));
        assert!(!Shortcut::SAVE.matches("s", command | Modifiers::ALT));
        assert!(!Shortcut::SAVE.matches("S", command | Modifiers::SHIFT));
        assert!(Shortcut::SAVE_AS.matches("S", command | Modifiers::SHIFT));
        assert!(!Shortcut::SAVE_AS.matches("s", command));
    }

    #[test]
    fn peek() {
        assert!(Held::PEEK.is_held(Modifiers::ALT));
        assert!(Held::PEEK.is_held(Modifiers::ALT | Modifiers::SHIFT));
        assert!(!Held::PEEK.is_held(Modifiers::CTRL));
    }

    #[test]
    fn a_disabled_binding_is_a_dead_key() {
        let command = Modifiers::COMMAND;
        let shift = command | Modifiers::SHIFT;
        let save = |editable| pressed(document_bindings(editable), "s", command);
        assert!(matches!(save(true), Some(Message::File(File::Save))));
        assert!(save(false).is_none());
        let save_as = pressed(document_bindings(false), "S", shift);
        assert!(matches!(save_as, Some(Message::File(File::SaveAs))));
    }
}
