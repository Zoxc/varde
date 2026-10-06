//! The web app, built by trunk from `index.html`: one wasm module, which
//! runs the app on the page and the lanes' Web Workers, each an instance
//! of its own (see `varde_lane::page::Host::start`). Does nothing
//! natively, where the app is `crates/binary`.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// Runs the app, on the page. wasm-bindgen calls it as every instance
/// starts, so in a worker it does nothing: the loader calls
/// [`serve_worker`] there.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsError> {
    if !js_sys::global().is_instance_of::<web_sys::Window>() {
        return Ok(());
    }
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Warn).ok();
    load_fonts();
    varde_app::run().map_err(|e| JsError::new(&e.to_string()))
}

/// Runs the worker's side of the lane `role` names, in a Web Worker,
/// called by `worker_loader.js` with the role the page posted.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn serve_worker(role: &str) -> Result<(), JsError> {
    match role {
        varde_regen::WORKER_ROLE => varde_regen::serve_worker(),
        varde_solve::WORKER_ROLE => varde_solve::serve_worker(),
        varde_io::WORKER_ROLE => varde_io::serve_worker(),
        _ => return Err(JsError::new(&format!("no worker has the role {role:?}"))),
    }
    Ok(())
}

/// A browser has no system fonts to give: iced brings Fira Sans Regular
/// (its `fira-sans` feature) and these add the semibold and a monospace,
/// under `fonts/` with their license. The generic families `Font::DEFAULT`
/// and `Font::MONOSPACE` name would otherwise find nothing and draw no
/// text at all. Bold, the view cube's letters, falls back to semibold.
#[cfg(target_arch = "wasm32")]
fn load_fonts() {
    use std::borrow::Cow;

    let mut fonts = iced::advanced::graphics::text::font_system()
        .write()
        .expect("font system lock poisoned");
    fonts.load_font(Cow::Borrowed(include_bytes!(
        "../fonts/FiraSans-SemiBold.ttf"
    )));
    fonts.load_font(Cow::Borrowed(include_bytes!(
        "../fonts/FiraMono-Regular.ttf"
    )));
    let db = fonts.raw().db_mut();
    db.set_sans_serif_family("Fira Sans");
    db.set_monospace_family("Fira Mono");
}
