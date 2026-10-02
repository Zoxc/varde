//! The IO worker's side of the web build, see the parent module: run by the
//! worker's `main` (`src/bin/varde-io-worker.rs`), never on the page.

mod disk;
mod files;
mod opfs;
pub(crate) mod serve;
mod settings;
