//! The web app, built by trunk from `index.html`. Does nothing natively,
//! where the app is `crates/binary`.

#[cfg(target_arch = "wasm32")]
fn main() -> varde_app::Result {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Warn).ok();
    varde_app::run()
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {}
