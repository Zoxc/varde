//! The lane on the web: a Web Worker for the app, handling requests in
//! order.
//!
//! The same API as the native lane, so the app doesn't tell them apart. The
//! worker is a second wasm instance built from `src/bin/varde-io-worker.rs`
//! by trunk (see `crates/web/index.html`), next to the regeneration worker
//! of `varde-regen`. It shares no memory with the page: messages cross as
//! bytes, see [`wire`].
//!
//! The code here is split by where it runs. `page` is the page's side: the
//! lane's, which never waits (how its requests reach the worker is
//! [`varde_lane::page`]'s), and the pickers and downloads of files of the
//! user's (see `src/pick.rs`). `worker` is the worker's: its serve loop,
//! what it does with each request (`src/web/worker/files.rs`), the Origin
//! Private File System it keeps designs and auto-saves in, whose
//! synchronous access handles only exist in workers (see `src/opfs.rs`),
//! and reading and writing the files the user picked. `js` is used by
//! both.
//!
//! A request using a file the user picked (see `src/pick.rs`) is posted
//! along with what the browser handed over for it, a handle or a file,
//! which is cloned to the worker.
//!
//! Every request is posted as it comes, numbered; the worker queues them
//! and replaces waiting saves like the native lane does (see the crate
//! docs), handling one at a time. Requests posted before the worker is
//! ready would be lost, so they wait on the page until it says it is.
//!
//! A worker that stops (a panic traps its wasm instance, or its script or
//! wasm didn't load) takes its open files with it, and the browser lets go
//! of their handles, keeping what was auto-saved for the next session to
//! recover. The requests it hadn't answered, and every one after, are
//! answered with the error. No new worker is started: it would hand out
//! [`FileId`](crate::FileId)s the app still holds for files of the old one.

use js_sys::Uint8Array;
use varde_lane::bytes;

use crate::wire;

pub(crate) mod js;
pub(crate) mod page;
pub(crate) mod worker;

pub use page::lane::spawn;
pub use worker::serve::serve;

/// The bytes of the message `buffer`, checked before they're copied: the
/// length is the other side's to say.
fn message_bytes(buffer: &Uint8Array) -> Result<Vec<u8>, wire::Error> {
    bytes::copy(buffer, wire::MAX_MESSAGE_BYTES).map_err(|e| wire::Error::TooLarge(e.len))
}
