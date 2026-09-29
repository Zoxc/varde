//! The IO Web Worker, built by trunk next to the web app (see
//! `crates/web/index.html`). A binary of its own so the worker's wasm holds
//! the IO side only, not iced and wgpu. Does nothing natively, where the
//! lane is a thread.

fn main() {
    #[cfg(target_arch = "wasm32")]
    varde_io::serve_worker();
}
