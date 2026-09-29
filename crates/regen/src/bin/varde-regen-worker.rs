//! The regeneration Web Worker, built by trunk next to the web app (see
//! `crates/web/index.html`). A binary of its own so the worker's wasm holds
//! the regeneration side only, not iced and wgpu. Does nothing natively, where
//! lanes are threads.

fn main() {
    #[cfg(target_arch = "wasm32")]
    varde_regen::serve_worker();
}
