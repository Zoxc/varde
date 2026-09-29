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
    Eye => r#"<path d="M2.5 12s3.5-6 9.5-6 9.5 6 9.5 6-3.5 6-9.5 6-9.5-6-9.5-6z"/><circle cx="12" cy="12" r="2.5"/>"#,
    EyeOff => r#"<path d="M3 3l18 18"/><path d="M10.6 6.1A9 9 0 0 1 12 6c6 0 9.5 6 9.5 6a16 16 0 0 1-2.7 3.3M6.6 7.6C4 9.3 2.5 12 2.5 12s3.5 6 9.5 6a9 9 0 0 0 3.4-.7"/>"#,
    Trash => r#"<path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/>"#,
    Rollback => r#"<path d="M4 4v16"/><path d="M20 12H8M12 8l-4 4 4 4"/>"#,
    Save => r#"<path d="M5 4h11l3 3v13H5z"/><path d="M8 4v5h7V4M8 20v-6h8v6"/>"#,
    Sketch => r#"<path d="M4 20h16"/><path d="M14.5 4.5l5 5L9 20H4v-5z"/>"#,
    Check => r#"<path d="M5 12.5l4.5 4.5L19 7"/>"#,
    Plane => r#"<path d="M3 16l5-8h13l-5 8z"/>"#,
    Line => r#"<path d="M6 18L18 6"/><circle cx="5" cy="19" r="1.6"/><circle cx="19" cy="5" r="1.6"/>"#,
    Circle => r##"<circle cx="12" cy="12" r="8"/><circle cx="12" cy="12" r=".8" fill="#000"/>"##,
    Arc => r#"<path d="M4 18A9 9 0 0 1 20 18"/>"#,
    Rectangle => r#"<rect x="4" y="6" width="16" height="12"/>"#,
    Polygon => r#"<path d="M8 4.5h8l4 7.5-4 7.5H8L4 12z"/>"#,
    Spline => r#"<path d="M3 17c3-9 6-9 9-5s6 4 9-5"/><circle cx="3" cy="17" r="1.3"/><circle cx="21" cy="7" r="1.3"/>"#,
    Point => r##"<circle cx="12" cy="12" r="2.5" fill="#000"/><path d="M12 3v4M12 17v4M3 12h4M17 12h4"/>"##,
    Constrain => r#"<rect x="5" y="11" width="14" height="9" rx="1.5"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/>"#,
    Coincident => r##"<path d="M4 20L20 4M4 4l16 16"/><circle cx="12" cy="12" r="2.5" fill="#000"/>"##,
    Horizontal => r#"<path d="M4 12h16M4 8v8M20 8v8"/>"#,
    Vertical => r#"<path d="M12 4v16M8 4h8M8 20h8"/>"#,
    Parallel => r#"<path d="M8 20L14 4M12 20l6-16"/>"#,
    Perpendicular => r#"<path d="M4 20h16M12 20V5"/>"#,
    Tangent => r#"<circle cx="10" cy="13" r="6"/><path d="M3 7h18"/>"#,
    Smooth => r##"<path d="M3 17h7c5 0 7-3 11-11"/><circle cx="10" cy="17" r="1.6" fill="#000"/>"##,
    Equal => r#"<path d="M5 9h14M5 15h14"/>"#,
    Concentric => r#"<circle cx="12" cy="12" r="8"/><circle cx="12" cy="12" r="3.5"/>"#,
    Midpoint => r##"<path d="M4 18L20 6"/><circle cx="12" cy="12" r="2.5" fill="#000"/>"##,
    Symmetric => r#"<path d="M12 3v18" stroke-dasharray="2 2.2"/><circle cx="6" cy="12" r="2.2"/><circle cx="18" cy="12" r="2.2"/>"#,
    Fix => r#"<path d="M9 4h6l-1 6 3 3H7l3-3z"/><path d="M12 13v7"/>"#,
    Dimension => r#"<path d="M4 6v12M20 6v12M4 12h16M7.5 9L4 12l3.5 3M16.5 9l3.5 3-3.5 3"/>"#,
    Trim => r#"<circle cx="6" cy="7" r="2.5"/><circle cx="6" cy="17" r="2.5"/><path d="M8.2 8.4L20 17M8.2 15.6L20 7"/>"#,
    Extend => r#"<path d="M3 18L13 8"/><path d="M13 8l7-7" stroke-dasharray="2 2.2"/><path d="M15 21V11"/>"#,
    Mirror => r#"<path d="M12 3v18" stroke-dasharray="2 2"/><path d="M9 7L3 17h6zM15 7l6 10h-6z"/>"#,
    Offset => r#"<rect x="3" y="3" width="18" height="18" rx="2"/><rect x="8" y="8" width="8" height="8" rx="1"/>"#,
    Fillet => r#"<path d="M5 20V11a6 6 0 0 1 6-6h9"/>"#,
    Chamfer => r#"<path d="M5 20V10l5-5h10"/>"#,
    // Not in the mock: arrows each way, a handle on a curve, a comb's
    // teeth over one.
    Convert => r#"<path d="M4 8h14l-3-3M20 16H6l3 3"/>"#,
    Handles => r#"<path d="M3 19c3-8 8-12 18-13"/><path d="M4.5 9.5l10 4"/><circle cx="4.5" cy="9.5" r="1.5"/><circle cx="14.5" cy="13.5" r="1.5"/>"#,
    Comb => r#"<path d="M3 18c4-9 14-9 18 0"/><path d="M6.5 13.4L5 9.5M12 11.3V6.5M17.5 13.4L19 9.5"/>"#,
    // Nothing: room for an icon, beside items that have one.
    Blank => "",
}

impl Icon {
    /// The icon's colour, by what it stands for as in the mock: the solid
    /// colour for solids and bodies, the sketch colour for sketches and
    /// their curves, the construction colour for planes and points, and
    /// otherwise the text colour.
    pub fn tint(self, palette: &Palette) -> Color {
        match self {
            Icon::Body => palette.solid,
            Icon::Sketch
            | Icon::Line
            | Icon::Circle
            | Icon::Arc
            | Icon::Rectangle
            | Icon::Polygon
            | Icon::Spline
            | Icon::Convert
            | Icon::Handles
            | Icon::Comb
            | Icon::Constrain
            | Icon::Coincident
            | Icon::Horizontal
            | Icon::Vertical
            | Icon::Parallel
            | Icon::Perpendicular
            | Icon::Tangent
            | Icon::Smooth
            | Icon::Equal
            | Icon::Concentric
            | Icon::Midpoint
            | Icon::Symmetric
            | Icon::Fix
            | Icon::Dimension
            | Icon::Trim
            | Icon::Extend
            | Icon::Mirror
            | Icon::Offset
            | Icon::Fillet
            | Icon::Chamfer => palette.sketch,
            Icon::Plane | Icon::Point => palette.construction,
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
