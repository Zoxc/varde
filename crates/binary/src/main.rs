//! The native app, and `varde mcp`, the stdio relay to its MCP server
//! (`src/mcp.rs`), chosen by the first argument before anything of the
//! app starts.
//!
//! On Windows a release build is a GUI program, so no console window opens
//! with it; a debug build keeps the console, where the `log` output goes.
//! `varde mcp` works in either: a client starting it hands it its pipes,
//! which a GUI program inherits as any other does. Run by hand from a
//! console, a release build has none, so it reads nothing and exits, and
//! its usage errors go nowhere (`cargo run -- mcp`, a debug build, has the
//! console).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::process::ExitCode;

use log::{LevelFilter, Log, Metadata, Record};

#[cfg(not(target_arch = "wasm32"))]
mod mcp;

fn main() -> ExitCode {
    // Only fails if a logger is already set, which it isn't.
    let _ = log::set_logger(&Stderr);
    log::set_max_level(LevelFilter::Warn);
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(code) = mcp::main(std::env::args_os().skip(1)) {
        return code;
    }
    match varde_app::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:?}");
            ExitCode::FAILURE
        }
    }
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
