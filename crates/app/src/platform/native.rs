//! The native half of the platform, see the parent module.

use iced::time::Instant;
use iced::{Subscription, window};

use super::TICK;
use crate::Message;

/// Natively the app asks before the window closes instead, see
/// `Varde::leave`.
pub(crate) fn guard() -> Subscription<Message> {
    Subscription::none()
}

/// Natively [`Message::CloseRequested`] comes instead.
pub(crate) fn leaving() -> Subscription<Message> {
    Subscription::none()
}

/// Ticks every second with the time, as [`Message::AutoSaveTick`], from a
/// thread of its own: iced's thread pool executor has no timer. The thread
/// ends at the tick after the subscription is dropped.
pub(crate) fn auto_save_ticks() -> Subscription<Message> {
    use iced::futures::StreamExt;
    use iced::futures::channel::mpsc;

    fn start() -> impl iced::futures::Stream<Item = Message> {
        let (sender, ticks) = mpsc::unbounded();
        let timer = std::thread::Builder::new()
            .name("auto-save timer".to_owned())
            .spawn(move || {
                while sender.unbounded_send(Instant::now()).is_ok() {
                    std::thread::sleep(TICK);
                }
            });
        // Nothing is auto-saved then, but nothing else is lost.
        if let Err(error) = timer {
            log::error!("Couldn't start auto-saving: {error}");
        }
        ticks.map(Message::AutoSaveTick)
    }

    Subscription::run(start)
}

/// The logo rasterized for the window icon. Uses the resvg that iced's SVG
/// support already builds, so no image decoder or pre-rendered bitmap is
/// needed.
pub(crate) fn window_icon() -> Option<window::Icon> {
    use resvg::{tiny_skia, usvg};

    const SIZE: u32 = 64;
    let tree = usvg::Tree::from_str(varde_view::LOGO_SVG, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(SIZE, SIZE)?;
    let scale = SIZE as f32 / tree.size().width().max(tree.size().height());
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // tiny-skia stores premultiplied alpha; the icon wants it straight.
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let c = pixel.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    window::icon::from_rgba(rgba, SIZE, SIZE).ok()
}
