//! Files of the user's: picking them, and on the web downloading designs.
//! Run on the UI thread natively and on the page on the web, never in the
//! lane, which is handed what they picked.
//!
//! Natively the platform's file dialogs pick them, see `src/native/pick.rs`,
//! handing over a path, which saves write to: nothing is downloaded, and
//! [`writable`] has nothing to ask. The rest of this is about the web,
//! where designs are kept in browser storage by name (see `src/browser.rs`),
//! and the user's own files are what the browser hands over.
//!
//! A browser only hands the page files the user picked. Where the File
//! System Access API is there (Chromium: `showOpenFilePicker` in
//! `window`, see [`file_system_access`]), Open, and Save As to "A file on
//! your computer…", show its pickers, which hand over a
//! `FileSystemFileHandle`: the design goes on from that file like natively,
//! Save writing it back. Elsewhere (Firefox, Safari) Open is a file input,
//! whose file can only be read: the lane copies it into browser storage
//! under its name, made unique, and the design opens from there.
//!
//! Pickers need the user's activation, so the page shows them, from the
//! app's `update` right after the click or key press: within the browser's
//! few seconds of transient activation. What they hand over is kept on the
//! page and the app holds a [`Picked`](crate::Picked) for it, which a request posts to the
//! IO worker along with it (see `src/web.rs`): handles and files are
//! structured-cloneable. The worker reads it whole (`getFile()`, or the
//! file itself) and writes a handle whole (`createWritable()`): a file of
//! the user's has no sync access handle, which only the Origin Private File
//! System has, so it's replaced with a `.vrdp` holding the design as its
//! one record, which the format allows. Before writing, the worker checks
//! the file still holds the record it last read or wrote, and refuses the
//! save as a conflict otherwise, like natively; nothing keeps another
//! program from writing between that check and the write. Nor can the
//! file be locked: two tabs can have it open for editing, and only the
//! conflict check keeps one from saving over the other.
//!
//! Chromium asks the user before a page first writes to a file it opened,
//! and asks only a page, during an activation: the worker can't. So the
//! app asks, with [`writable`], as the user saves, before sending the save.
//!
//! Auto-saves of designs from the user's files go to the Origin Private
//! File System like those of new designs, each to an entry of its own, see
//! `src/opfs.rs`, along with the file's name, so one left behind by a tab
//! that closed is listed on the welcome screen by its name. It opens as a
//! copy: the handle is gone with the tab. Should the entry not be made,
//! say with the site's data blocked, the file still opens and saves, only
//! without auto-saves.
//!
//! A file dropped on the page (the app's welcome screen takes them, see
//! `dropped`) is kept like one the Open picker handed over: where the File
//! System Access API is, its handle (`getAsFileSystemHandle`), else the
//! `File`, copied into browser storage as one from the file input is. It's
//! kept as it's dropped, before the app decides whether to open it: one it
//! doesn't is let go of with [`forget`].
//!
//! Downloading a design is a command of its own, never a save: the page
//! hands the design over through the [`downloader`], which a design in
//! browser storage records (see `src/downloads.rs`). Exporting a 3MF file
//! goes through [`pick_export`]: a save picker suggesting `name.3mf`,
//! natively or with the File System Access API, and otherwise a download
//! of `name.3mf`.
//!
//! On the web the pickers and downloads are the page's, see
//! `src/web/page/pick.rs`, and reading and writing what they handed over
//! is the IO worker's, see `src/web/worker/disk.rs`. How a design's file
//! is named is [`varde_document::name`]'s.

use varde_document::APP_NAME;

#[cfg(not(target_arch = "wasm32"))]
pub use crate::native::pick::{
    downloader, file_system_access, forget, pick_export, pick_open, pick_save, writable,
};
#[cfg(target_arch = "wasm32")]
pub use crate::web::page::pick::{
    Dropped, downloader, dropped, file_system_access, forget, pick_export, pick_open, pick_save,
    writable,
};

/// Hands a design over as a download, by its file name and bytes, see
/// [`downloader`].
pub type Download = fn(&str, &[u8]) -> Result<(), String>;

/// What the file pickers call a design.
pub(crate) fn filter() -> String {
    format!("{APP_NAME} design")
}

/// What the export picker calls a 3MF file.
pub(crate) fn export_filter() -> String {
    "3MF model".to_owned()
}
