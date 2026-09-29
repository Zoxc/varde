//! Stroke icons, and the logo for the window icon, from the `I`, `CAT` and `LOGO` tables in
//! `notes/ui-mock.html`.
//!
//! Icons are 24×24 single-colour stroke drawings, tinted through
//! [`svg::Style`].

use std::sync::LazyLock;

use iced::widget::{Svg, stack, svg};
use iced::{Color, Element, Theme};

use crate::theme::{self, Palette};

/// The logo: a folded "V" with a sketch point above it.
///
/// `assets/logo.svg` is the single source for the window icon and the
/// Windows `.ico`.
pub const LOGO_SVG: &str = include_str!("../../../assets/logo.svg");

/// Declares [`Icon`] and its SVG paths from one list, so each icon is named
/// once and [`Icon::ALL`] follows the declaration order.
macro_rules! icons {
    ($($name:ident => $paths:expr,)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Icon {
            $($name,)*
        }

        impl Icon {
            /// Every icon, in declaration order, so `ALL[icon as usize] == icon`.
            const ALL: [Icon; [$(Icon::$name),*].len()] = [$(Icon::$name),*];

            /// The SVG elements, drawn in black.
            fn paths(self) -> &'static str {
                match self {
                    $(Icon::$name => $paths,)*
                }
            }
        }
    };
}

icons! {
    Plus => r#"<path d="M12 5v14M5 12h14"/>"#,
    Folder => r#"<path d="M3 7a1 1 0 0 1 1-1h5l2 2h9a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z"/>"#,
    Chev => r#"<path d="M7 10l5 5 5-5"/>"#,
    Home => r#"<path d="M4 11l8-7 8 7"/><path d="M6 9.5V20h12V9.5"/>"#,
    Undo => r#"<path d="M9 14L4 9l5-5"/><path d="M4 9h10a6 6 0 0 1 0 12h-3"/>"#,
    Redo => r#"<path d="M15 14l5-5-5-5"/><path d="M20 9H10a6 6 0 0 0 0 12h3"/>"#,
    Search => r#"<circle cx="11" cy="11" r="6"/><path d="M20 20l-4.5-4.5"/>"#,
    Sun => r#"<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>"#,
    Moon => r#"<path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z"/>"#,
    Help => r##"<circle cx="12" cy="12" r="9"/><path d="M9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .8-1 1.5V14"/><circle cx="12" cy="17" r=".6" fill="#000"/>"##,
    Close => r#"<path d="M6 6l12 12M18 6L6 18"/>"#,
    Body => r#"<path d="M12 3l8 4.5v9L12 21l-8-4.5v-9z"/><path d="M4 7.5l8 4.5 8-4.5M12 12v9"/>"#,
    Box => r#"<path d="M4 8l8-4 8 4v8l-8 4-8-4z"/><path d="M4 8l8 4 8-4M12 12v8"/>"#,
    Eye => r#"<path d="M2.5 12s3.5-6 9.5-6 9.5 6 9.5 6-3.5 6-9.5 6-9.5-6-9.5-6z"/><circle cx="12" cy="12" r="2.5"/>"#,
    EyeOff => r#"<path d="M3 3l18 18"/><path d="M10.6 6.1A9 9 0 0 1 12 6c6 0 9.5 6 9.5 6a16 16 0 0 1-2.7 3.3M6.6 7.6C4 9.3 2.5 12 2.5 12s3.5 6 9.5 6a9 9 0 0 0 3.4-.7"/>"#,
    Trash => r#"<path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/>"#,
    Rollback => r#"<path d="M4 4v16"/><path d="M20 12H8M12 8l-4 4 4 4"/>"#,
    Save => r#"<path d="M5 4h11l3 3v13H5z"/><path d="M8 4v5h7V4M8 20v-6h8v6"/>"#,
}

impl Icon {
    /// The icon's colour: the solid colour for solids and bodies, otherwise
    /// the text colour.
    pub fn tint(self, palette: &Palette) -> Color {
        match self {
            Icon::Body | Icon::Box => palette.solid,
            _ => palette.text,
        }
    }

    fn handle(self, frame: Frame) -> svg::Handle {
        static HANDLES: LazyLock<[[svg::Handle; Icon::ALL.len()]; 2]> =
            LazyLock::new(|| [handles(0.0), handles(BUTTON_MARGIN)]);
        let frame = match frame {
            Frame::None => 0,
            Frame::Button => 1,
        };
        HANDLES[frame][self as usize].clone()
    }
}

/// Every icon's handle, in [`Icon::ALL`] order, drawn with `margin` icon
/// units of space around it.
fn handles(margin: f32) -> [svg::Handle; Icon::ALL.len()] {
    let side = 24.0 + 2.0 * margin;
    Icon::ALL.map(|icon| {
        svg::Handle::from_memory(
            format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{} {} {side} {side}" fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">{}</svg>"##,
                -margin,
                -margin,
                icon.paths()
            )
            .into_bytes(),
        )
    })
}

/// Empty space drawn around an icon, so the [`Svg`] can fill a button and
/// its hover status matches the button's.
#[derive(Debug, Clone, Copy)]
enum Frame {
    None,
    /// [`INLINE`] in [`BUTTON_SIZE`].
    Button,
}

/// Side of an icon-only toolbar button.
pub const BUTTON_SIZE: f32 = 26.0;

/// Side of an icon beside text, and inside a [`BUTTON_SIZE`] button.
pub const INLINE: f32 = 16.0;

/// The margin in icon units (24 per icon).
const BUTTON_MARGIN: f32 = (BUTTON_SIZE - INLINE) / 2.0 * 24.0 / INLINE;

/// An icon `size` pixels square, in its [`Icon::tint`].
pub fn icon(icon: Icon, size: f32) -> Svg<'static> {
    tinted(icon, size, move |palette| icon.tint(palette))
}

/// An icon `size` pixels square, in the colour `color` picks from the palette.
pub fn tinted(icon: Icon, size: f32, color: impl Fn(&Palette) -> Color + 'static) -> Svg<'static> {
    framed(icon, Frame::None, size, move |palette, _| color(palette))
}

/// The content of an icon-only button [`BUTTON_SIZE`] square, with no
/// padding. `color` gets whether the button is hovered.
pub fn button_icon(icon: Icon, color: impl Fn(&Palette, bool) -> Color + 'static) -> Svg<'static> {
    framed(icon, Frame::Button, BUTTON_SIZE, color)
}

fn framed(
    icon: Icon,
    frame: Frame,
    size: f32,
    color: impl Fn(&Palette, bool) -> Color + 'static,
) -> Svg<'static> {
    Svg::new(icon.handle(frame))
        .width(size)
        .height(size)
        .style(move |theme: &Theme, status| svg::Style {
            color: Some(color(theme::palette(theme), status == svg::Status::Hovered)),
        })
}

/// A mouse button, for [`mouse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    /// The wheel, or the middle button.
    Wheel,
}

impl MouseButton {
    /// The highlight drawn over the mouse outline for this button.
    fn handle(self) -> &'static svg::Handle {
        static LEFT: LazyLock<svg::Handle> =
            LazyLock::new(|| mouse_handle(r#"<path d="M5 .5A4.5 4.5 0 0 0 .5 5v1H5z"/>"#));
        static RIGHT: LazyLock<svg::Handle> =
            LazyLock::new(|| mouse_handle(r#"<path d="M5 .5A4.5 4.5 0 0 1 9.5 5v1H5z"/>"#));
        static WHEEL: LazyLock<svg::Handle> = LazyLock::new(|| {
            mouse_handle(r#"<rect x="4.1" y="2.2" width="1.8" height="3" rx=".9"/>"#)
        });
        match self {
            MouseButton::Left => &LEFT,
            MouseButton::Right => &RIGHT,
            MouseButton::Wheel => &WHEEL,
        }
    }
}

/// A small mouse with `button` highlighted, for input hints.
pub fn mouse<'a, Message: 'a>(button: MouseButton) -> Element<'a, Message> {
    static OUTLINE: LazyLock<svg::Handle> = LazyLock::new(|| {
        mouse_handle(
            r##"<rect x=".5" y=".5" width="9" height="13" rx="4.5" fill="none" stroke="#000"/>"##,
        )
    });

    let part = |handle: &svg::Handle, color: fn(&Palette) -> Color| {
        Svg::new(handle.clone())
            .width(11)
            .height(15)
            .style(move |theme: &Theme, _| svg::Style {
                color: Some(color(theme::palette(theme))),
            })
    };
    stack![
        part(&OUTLINE, |p| p.muted),
        part(button.handle(), |p| p.accent),
    ]
    .into()
}

fn mouse_handle(content: &str) -> svg::Handle {
    svg::Handle::from_memory(
        format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 14">{content}</svg>"#)
            .into_bytes(),
    )
}
