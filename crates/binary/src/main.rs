//! The native app. On Windows a release build is a GUI program, so no
//! console window opens with it; a debug build keeps the console, where
//! the `log` output goes.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use log::{LevelFilter, Log, Metadata, Record};

fn main() -> varde_app::Result {
    // Only fails if a logger is already set, which it isn't.
    let _ = log::set_logger(&Stderr);
    log::set_max_level(LevelFilter::Warn);
    varde_app::run()
}

/// Writes `log` output, the app's errors and iced's and wgpu's warnings, to
/// stderr, as the web app writes it to the browser console.
struct Stderr;

impl Log for Stderr {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            eprintln!("{} {}: {}", record.level(), record.target(), record.args());
        }
    }

    fn flush(&self) {}
}
