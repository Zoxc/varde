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
