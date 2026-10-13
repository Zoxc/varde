//! Where a design in browser storage stands against its downloads, as the
//! welcome screen's cards and the file menu say it.

use iced::widget::{Container, Space, container};

use crate::Message;
use crate::theme::{self, Palette};

/// Where a design in browser storage stands against its downloads, each
/// with when it was downloaded, like "2 h ago", if that's known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Downloads {
    /// It never was: what's in the browser is all there is.
    Never,
    /// It was, and has changed since.
    Changed(Option<String>),
    /// What's in the browser is what was last downloaded.
    Latest(Option<String>),
}

/// What a card, or the file menu, says of where a design stands against
/// its `downloads`, and the colour of the dot before that.
pub(crate) fn downloads_said(downloads: &Downloads) -> (String, fn(&Palette) -> iced::Color) {
    let at = |what: &str, when: &Option<String>| match when {
        Some(when) => format!("{what} {when}"),
        None => what.to_owned(),
    };
    match downloads {
        Downloads::Never => ("Never downloaded".to_owned(), |_| theme::NEVER_DOWNLOADED),
        Downloads::Changed(when) => (at("Changed since downloaded", when), |p| p.accent),
        Downloads::Latest(when) => (at("Latest downloaded", when), |p| p.ok),
    }
}

/// The dot before what's said of where a design stands, in `color`.
pub(crate) fn dot<'a>(color: fn(&Palette) -> iced::Color) -> Container<'a, Message> {
    container(Space::new().width(7).height(7)).style(theme::dot(color))
}
