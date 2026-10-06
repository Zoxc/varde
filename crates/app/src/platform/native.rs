//! The native half of the platform, see the parent module.

use iced::time::Instant;
use iced::{Subscription, window};

use std::time::Duration;

use super::{TOAST_TICK, TICK};
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

/// Natively iced shows the title as the window's.
pub(crate) fn show_title(_title: &str) {}

/// Natively the welcome screen takes no files dropped.
pub(crate) fn drops() -> Subscription<Message> {
    Subscription::none()
}

/// Ticks every second with the time, as [`Message::AutoSaveTick`], see
/// [`ticks`].
pub(crate) fn auto_save_ticks() -> Subscription<Message> {
    ticks("auto-save timer", TICK, Message::AutoSaveTick)
}

/// Ticks every [`TOAST_TICK`] with the time, as [`Message::ToastTick`],
/// see [`ticks`].
pub(crate) fn toast_ticks() -> Subscription<Message> {
    ticks("toast timer", TOAST_TICK, Message::ToastTick)
}

/// Ticks every `period` with the time, as `message`, from a thread of its
/// own named `name`: iced's thread pool executor has no timer. The thread
/// ends at the tick after the subscription is dropped. If the thread
/// can't start, nothing ticks, which is logged.
fn ticks(
    name: &'static str,
    period: Duration,
    message: fn(Instant) -> Message,
) -> Subscription<Message> {
    use iced::futures::StreamExt;
    use iced::futures::channel::mpsc;

    type Timer = (&'static str, Duration, fn(Instant) -> Message);

    fn start(
        &(name, period, message): &Timer,
    ) -> impl iced::futures::Stream<Item = Message> + use<> {
        let (sender, ticks) = mpsc::unbounded();
        let timer = std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                while sender.unbounded_send(Instant::now()).is_ok() {
                    std::thread::sleep(period);
                }
            });
        if let Err(error) = timer {
            log::error!("Couldn't start the {name}: {error}");
        }
        ticks.map(message)
    }

    Subscription::run_with((name, period, message), start)
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

/// Puts `text` on the clipboard, through iced's.
pub(crate) fn copy(text: String) -> iced::Task<Message> {
    iced::clipboard::write(text)
}
