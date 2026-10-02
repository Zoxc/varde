//! The web app, built by trunk from `index.html`. Does nothing natively,
//! where the app is `crates/binary`.

#[cfg(target_arch = "wasm32")]
fn main() -> varde_app::Result {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Warn).ok();
    load_fonts();
    varde_app::run()
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
    fonts.load_font(Cow::Borrowed(include_bytes!("../fonts/FiraSans-SemiBold.ttf")));
    fonts.load_font(Cow::Borrowed(include_bytes!("../fonts/FiraMono-Regular.ttf")));
    let db = fonts.raw().db_mut();
    db.set_sans_serif_family("Fira Sans");
    db.set_monospace_family("Fira Mono");
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {}
