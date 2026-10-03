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
//! # The title
//!
//! The window's title is iced's natively; on the web the page's
//! (`document.title`), which [`show_title`] keeps in step, so the tab and
//! the browser's history name the design.
//!
//! # Files dropped
//!
//! On the web a `.vrdp` file dropped on the welcome screen's page opens as
//! one picked with Open… would: [`drops`] yields [`Message::FileDragged`]
//! as one comes over the page and goes off it, and
//! [`Message::FileDropped`] as it's dropped. Where the File System Access
//! API is (Chromium), the browser hands over a handle to it
//! (`DataTransferItem.getAsFileSystemHandle`), and the design goes on from
//! the file; elsewhere only a `File`, which is copied into browser
//! storage. It's subscribed to
//! whatever shows, so that the browser never takes a file dropped, which
//! would leave the page for it; with a document open, or over a dialog,
//! one is let go of unopened. Natively nothing is dropped: the welcome
//! screen has no drop zone there.
//!
//! [`Message::CloseRequested`]: crate::Message::CloseRequested
//! [`Message::FileDragged`]: crate::Message::FileDragged
//! [`Message::FileDropped`]: crate::Message::FileDropped
//! [`Message::PageLeaving`]: crate::Message::PageLeaving

use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::{auto_save_ticks, copy, drops, guard, leaving, show_title, window_icon};

#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
pub(crate) use web::{auto_save_ticks, copy, drops, guard, leaving, show_title, window_icon};

/// How often [`auto_save_ticks`] ticks.
const TICK: Duration = Duration::from_secs(1);
