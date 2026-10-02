//! The web half of the platform, see the parent module.

use iced::time::Instant;
use iced::{Subscription, window};

mod page;

pub(crate) use page::{guard, leaving};

use super::TICK;
use crate::Message;

/// Ticks every second with the time, as [`Message::AutoSaveTick`]. The web
/// has no threads, but iced's executor there has a timer, on the browser's
/// `setTimeout`.
pub(crate) fn auto_save_ticks() -> Subscription<Message> {
    iced::time::every(TICK).map(|_| Message::AutoSaveTick(Instant::now()))
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
