//! The web half of the platform, see the parent module.

use iced::time::Instant;
use iced::{Subscription, window};

mod page;

pub(crate) use page::{drops, guard, leaving};

use super::TICK;
use crate::Message;

/// Ticks every second with the time, as [`Message::AutoSaveTick`]. The web
/// has no threads, but iced's executor there has a timer, on the browser's
/// `setTimeout`.
pub(crate) fn auto_save_ticks() -> Subscription<Message> {
    iced::time::every(TICK).map(|_| Message::AutoSaveTick(Instant::now()))
}

/// Shows `title` as the page's, if it isn't already: the tab and the
/// browser's history name the design by it.
pub(crate) fn show_title(title: &str) {
    if let Some(document) = web_sys::window().and_then(|window| window.document())
        && document.title() != title
    {
        document.set_title(title);
    }
}

/// Browsers have no window icon.
pub(crate) fn window_icon() -> Option<window::Icon> {
    None
}

/// Puts `text` on the clipboard through the browser's Clipboard API, at
/// once, while the click on the copy button still counts as the user's
/// (iced's clipboard does nothing on the web). Where it's refused (no
/// secure context, no permission), the value isn't copied and the
/// browser's console says why: the promise isn't waited on.
pub(crate) fn copy(text: String) -> iced::Task<Message> {
    if let Some(window) = web_sys::window() {
        let _ = window.navigator().clipboard().write_text(&text);
    }
    iced::Task::none()
}
