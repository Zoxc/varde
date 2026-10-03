//! The first panic of a session, kept for the welcome screen to show
//! next time.
//!
//! [`install`] sets the panic hook. The first panic of the process is
//! recorded and the user told of it at once, in a dialog (natively the
//! platform's, through `rfd`; on the web the browser's `alert`). Later
//! ones are left to the hook before it, which logs them: they're mostly
//! the first one's aftermath, as the app comes down.
//!
//! The hook writes the record itself rather than through the IO lane: the
//! lane may be what panicked, and the process may be going down. Natively
//! it's `panic.toml` in the platform data directory, e.g.
//! `~/.local/share/varde-cad/panic.toml`, replaced whole
//! (`src/native/panicked.rs`). On the web the page can't reach the Origin
//! Private File System without waiting, so it's kept in the page's
//! `localStorage` instead (`src/web/page/panicked.rs`), and a Web Worker's
//! panic is recorded by the page as it hears of it, the worker having no
//! `localStorage`. Loading and discarding it are the lane's
//! [`Request::LoadPanic`](crate::Request::LoadPanic) and
//! [`Request::DiscardPanic`](crate::Request::DiscardPanic), which the web
//! answers on the page.
//!
//! The record is a table of plain keys:
//!
//! ```toml
//! time = 1759500000
//! version = "0.1.0"
//! thread = "main"
//! message = "index out of bounds: the len is 0 but the index is 0"
//! location = "crates/app/src/lib.rs:10:5"
//! backtrace = """..."""
//! ```
//!
//! It's user data, edited by hand perhaps: each key is read on its own,
//! and one gone bad is left out. One without a message isn't a panic.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::UnixSeconds;

/// The most bytes of the record read: a larger one is taken as gone bad.
pub(crate) const MAX_BYTES: u64 = 256 * 1024;
/// The most bytes of a message kept.
const MAX_MESSAGE: usize = 16 * 1024;
/// The most bytes of a backtrace kept.
const MAX_BACKTRACE: usize = 192 * 1024;
/// The most bytes of the other keys kept.
const MAX_SHORT: usize = 1024;

/// A panic, as recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Panic {
    /// When it happened, by the system clock: only for showing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<UnixSeconds>,
    /// The app's version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The thread that panicked, or on the web the worker, e.g. "the file
    /// worker".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    pub message: String,
    /// Where in the source, as `file:line:column`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backtrace: Option<String>,
}

impl Panic {
    /// The panic `info` describes, on this thread, now.
    fn of(info: &std::panic::PanicHookInfo<'_>) -> Self {
        let payload = info.payload();
        let message = (payload.downcast_ref::<&str>().copied())
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("unknown error");
        #[cfg(not(target_arch = "wasm32"))]
        let thread = std::thread::current().name().map(str::to_owned);
        #[cfg(target_arch = "wasm32")]
        let thread = Some("main".to_owned());
        Self::new(
            thread,
            message,
            info.location().map(ToString::to_string),
            backtrace(),
        )
    }

    /// A panic on `thread` with `message`, now, each key bounded.
    pub(crate) fn new(
        thread: Option<String>,
        message: &str,
        location: Option<String>,
        backtrace: Option<String>,
    ) -> Self {
        Self {
            time: Some(UnixSeconds::now()),
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            thread: thread.map(|thread| bounded(thread, MAX_SHORT)),
            message: bounded(message.to_owned(), MAX_MESSAGE),
            location: location.map(|location| bounded(location, MAX_SHORT)),
            backtrace: backtrace.map(|backtrace| bounded(backtrace, MAX_BACKTRACE)),
        }
    }

    /// The panic `toml` holds, if it holds one: a table with a message.
    /// The other keys are left out where they're missing or gone bad.
    pub(crate) fn parse(toml: &str) -> Option<Self> {
        let table = toml.parse::<toml::Table>().ok()?;
        let text = |name: &str, max| {
            table
                .get(name)
                .and_then(toml::Value::as_str)
                .map(|text| bounded(text.to_owned(), max))
        };
        Some(Self {
            time: (table.get("time"))
                .and_then(toml::Value::as_integer)
                .map(UnixSeconds),
            version: text("version", MAX_SHORT),
            thread: text("thread", MAX_SHORT),
            message: text("message", MAX_MESSAGE)?,
            location: text("location", MAX_SHORT),
            backtrace: text("backtrace", MAX_BACKTRACE),
        })
    }

    pub(crate) fn serialize(&self) -> String {
        toml::to_string(self).expect("plain keys serialize")
    }

    /// The whole of it, as the welcome screen shows it and copies it, e.g.
    /// for a bug report.
    pub fn report(&self) -> String {
        let app = varde_document::APP_NAME;
        let mut report = match &self.version {
            Some(version) => format!("{app} {version} panicked"),
            None => format!("{app} panicked"),
        };
        if let Some(thread) = &self.thread {
            report += &format!(" on thread '{thread}'");
        }
        if let Some(location) = &self.location {
            report += &format!(" at {location}");
        }
        report += &format!(":\n{}\n", self.message);
        if let Some(backtrace) = &self.backtrace {
            report += &format!("\nBacktrace:\n{backtrace}\n");
        }
        report
    }
}

/// `text`, cut to at most `max` bytes, at a character boundary.
fn bounded(mut text: String, max: usize) -> String {
    if text.len() > max {
        let end = (0..=max)
            .rev()
            .find(|&end| text.is_char_boundary(end))
            .unwrap_or(0);
        text.truncate(end);
    }
    text
}

/// The backtrace of the panic in progress: natively Rust's, symbolized,
/// on the web the JS stack, which names the wasm functions.
fn backtrace() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Some(std::backtrace::Backtrace::force_capture().to_string())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let error = js_sys::Error::new("");
        js_sys::Reflect::get(&error, &"stack".into())
            .ok()?
            .as_string()
    }
}

/// Where the panic is recorded natively, if there is a data directory to
/// put it in.
#[cfg(not(target_arch = "wasm32"))]
pub fn store() -> Option<PathBuf> {
    crate::native::project_dirs().map(|dirs| dirs.data_dir().join("panic.toml"))
}

/// On the web the panic is in `localStorage`, see the module docs.
#[cfg(target_arch = "wasm32")]
pub fn store() -> Option<PathBuf> {
    None
}

/// Whether the process has panicked, and so recorded the panic.
static PANICKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Sets the panic hook, see the module docs, keeping the one before for
/// every panic. Called once, by `varde_app::run`, after the frontend
/// set up its logging. On the web it also hears of the workers' panics, see
/// `varde_lane::page::on_worker_panic`.
pub fn install() {
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        before(info);
        if first() {
            panicked(&Panic::of(info));
        }
    }));
    #[cfg(target_arch = "wasm32")]
    varde_lane::page::on_worker_panic(|worker, message| {
        if first() {
            panicked(&Panic::new(Some(worker.to_owned()), message, None, None));
        }
    });
}

/// Whether this is the process's first panic.
fn first() -> bool {
    !PANICKED.swap(true, std::sync::atomic::Ordering::SeqCst)
}

/// Records `panic` and tells the user, which waits for them.
fn panicked(panic: &Panic) {
    #[cfg(not(target_arch = "wasm32"))]
    crate::native::panicked::record(panic);
    #[cfg(target_arch = "wasm32")]
    crate::web::page::panicked::record(panic);
    tell(panic);
}

/// What the dialog telling of `panic` says.
fn told(panic: &Panic) -> String {
    let app = varde_document::APP_NAME;
    format!(
        "{app} ran into an internal error and may not work as it should:\n\n{}\n\n\
         The details are kept, to show the next time {app} starts.",
        bounded(panic.message.clone(), 600)
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn tell(panic: &Panic) {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("Internal error")
        .set_description(told(panic))
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

#[cfg(target_arch = "wasm32")]
fn tell(panic: &Panic) {
    if let Some(window) = web_sys::window() {
        let _ = window.alert_with_message(&told(panic));
    }
}

#[cfg(test)]
mod tests;
