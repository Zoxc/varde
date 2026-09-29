//! What the app does differently natively and on the web, beyond files
//! (`varde_io::pick`): each target's half sits in a module of its own,
//! `src/platform/native.rs` and `src/platform/web.rs`, behind one API.
//!
//! # The page going away
//!
//! On the web, closing or reloading the tab never asks to close the
//! window, so [`Message::CloseRequested`] never comes. The browser asks the
//! user before leaving while the app subscribes to [`guard`], which it does
//! while there are changes to lose, as it asks natively before closing.
//! Whether they leave isn't told, so [`Message::PageLeaving`], from
//! [`leaving`], asks for an auto-save as the page may be going away, or is
//! hidden, which is the last the page is sure to hear of before it's
//! discarded.
//!
//! Natively there's no page: closing the window asks instead, see
//! `Varde::leave`, so neither [`guard`] nor [`leaving`] ever yields.
//!
//! [`Message::CloseRequested`]: crate::Message::CloseRequested
//! [`Message::PageLeaving`]: crate::Message::PageLeaving

use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::{auto_save_ticks, guard, leaving, window_icon};

#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
pub(crate) use web::{auto_save_ticks, guard, leaving, window_icon};

/// How often [`auto_save_ticks`] ticks.
const TICK: Duration = Duration::from_secs(1);
