//! Colours and widget styles, with a light and a dark palette.
//!
//! Style functions take the iced [`Theme`] and look up the matching
//! [`Palette`] with [`palette`], so the same function serves both modes.

use std::sync::LazyLock;

use iced::theme::palette::Extended;
use iced::widget::slider::{self as slide, HandleShape};
use iced::widget::{
    button, checkbox, container, progress_bar, rule, scrollable, text, text_input, toggler,
};
use iced::{Background, Border, Color, Font, Shadow, Theme, Vector, border, color, font};
use varde_render::{BodyTint, Colors, Srgb, Srgba};

mod body_color;
mod colors;
mod controls;
mod operation_panel;
mod params;
mod rail;
mod settings;
mod sketch;
mod toolbar_items;
mod welcome_page;

pub use body_color::*;
pub use colors::*;
pub use controls::*;
pub use operation_panel::*;
pub use params::*;
pub use rail::*;
pub use settings::*;
pub use sketch::*;
pub use toolbar_items::*;
pub use welcome_page::*;

/// For names and headings.
pub const SEMIBOLD: Font = Font {
    weight: font::Weight::Semibold,
    ..Font::DEFAULT
};

/// For the operation panel's labels and headings.
pub const BOLD: Font = Font {
    weight: font::Weight::Bold,
    ..Font::DEFAULT
};

/// Height of a side panel [`tab`]; its accent line is sized for it.
pub const TAB_HEIGHT: f32 = 30.0;

/// Width of the side panel, including its 1 px border. The toolbar's file
/// cell has the same width, so the two line up.
pub const SIDE_PANEL_WIDTH: f32 = 256.0;

/// Width of the side panel without its 1 px border on the right, and of
/// the toolbar's file cell above it, before the separator right of that.
pub const SIDE_PANEL_INNER_WIDTH: f32 = SIDE_PANEL_WIDTH - 1.0;

/// Corner radius of a [`flat_button`] and a [`hovered_row`].
const CONTROL_RADIUS: f32 = 6.0;

/// Corner radius of a [`primary_button`] and a [`secondary_button`].
const BUTTON_RADIUS: f32 = 8.0;

/// Corner radius of a [`key_chip`].
const CHIP_RADIUS: f32 = 4.0;

/// Corner radius of a [`menu`].
const MENU_RADIUS: f32 = 9.0;

/// Corner radius of a [`tab`]'s top.
const TAB_RADIUS: f32 = 7.0;

/// Corner radius of a [`card`]; the parts inside its border are one less.
pub(crate) const CARD_RADIUS: f32 = 10.0;

/// Corner radius shared by [`float_panel`] and [`float_button`].
const FLOAT_RADIUS: f32 = 7.0;

/// The iced theme for `mode`. Built once, so this is cheap to call per frame.
pub fn theme(mode: Mode) -> Theme {
    static THEMES: LazyLock<[Theme; 2]> = LazyLock::new(|| [build(Mode::Light), build(Mode::Dark)]);
    let i = match mode {
        Mode::Light => 0,
        Mode::Dark => 1,
    };
    THEMES[i].clone()
}

fn build(mode: Mode) -> Theme {
    let p = mode.palette();
    let name = match mode {
        Mode::Light => "Varde Light",
        Mode::Dark => "Varde Dark",
    };
    let base = iced::theme::Palette {
        background: p.panel,
        text: p.text,
        primary: p.accent,
        success: p.ok,
        warning: p.warning,
        danger: p.danger,
    };
    // iced would guess `is_dark` from the background's luminance; `palette`
    // reads the mode back from it, so it's set from the mode instead.
    Theme::custom_with_fn(name, base, move |base| Extended {
        is_dark: mode == Mode::Dark,
        ..Extended::generate(base)
    })
}

/// The palette a [`theme`] was built from.
pub fn palette(theme: &Theme) -> &'static Palette {
    let mode = if theme.extended_palette().is_dark {
        Mode::Dark
    } else {
        Mode::Light
    };
    mode.palette()
}

/// A 1 px border.
fn outline(color: Color, radius: f32) -> Border {
    Border {
        color,
        width: 1.0,
        radius: radius.into(),
    }
}

/// Whether a button shows as hovered: under the cursor or held down.
fn is_hovered(status: button::Status) -> bool {
    matches!(status, button::Status::Hovered | button::Status::Pressed)
}

fn filled(background: Color, text: Color) -> container::Style {
    container::Style {
        background: Some(Background::Color(background)),
        text_color: Some(text),
        ..container::Style::default()
    }
}

/// How opaque a disabled button is, all of it: the mock's `opacity`.
const DISABLED_OPACITY: f32 = 0.45;

/// `color` as a disabled control shows it, at [`DISABLED_OPACITY`],
/// unless `enabled`.
fn fade_unless(enabled: bool, color: Color) -> Color {
    if enabled {
        color
    } else {
        color.scale_alpha(DISABLED_OPACITY)
    }
}

/// `style` faded as a whole, as a disabled button is: its fill, text and
/// border at [`DISABLED_OPACITY`]. Fading the fill alone would leave white
/// text on a dark panel looking pressable.
fn faded(style: button::Style) -> button::Style {
    let fade = |color: Color| fade_unless(false, color);
    button::Style {
        background: style.background.map(|background| match background {
            Background::Color(color) => Background::Color(fade(color)),
            gradient => gradient,
        }),
        text_color: fade(style.text_color),
        border: Border {
            color: fade(style.border.color),
            ..style.border
        },
        ..style
    }
}

/// `style`, [`faded`] if `status` is disabled.
fn fade_if_disabled(status: button::Status, style: button::Style) -> button::Style {
    if status == button::Status::Disabled {
        faded(style)
    } else {
        style
    }
}

/// A button filled with `fill`, with white text.
fn filled_button(p: &Palette, fill: Color, status: button::Status) -> button::Style {
    let background = if is_hovered(status) {
        brighten(fill, 1.08)
    } else {
        fill
    };
    let style = button::Style {
        background: Some(Background::Color(background)),
        text_color: Emphasis::Primary.content(p),
        border: border::rounded(BUTTON_RADIUS),
        ..button::Style::default()
    };
    fade_if_disabled(status, style)
}

/// The colour of the operation panel's recessed well, which its fields
/// sit in: the text's 6% into the panel's, as the tool rail's strip.
pub fn well(p: &Palette) -> Color {
    mix(p.text, p.panel, 0.06)
}

/// The colour of the operation panel's card behind its head, and of its
/// controls on the well.
fn control_fill(p: &Palette, enabled: bool) -> Color {
    fade_unless(enabled, p.panel)
}

/// The soft accent fill of a [`tile`] or [`toggle_square`] that's on,
/// deeper while `hovered`.
fn on_fill(p: &Palette, hovered: bool) -> Color {
    mix(p.accent, p.panel, if hovered { 0.26 } else { 0.13 })
}

/// A wash of the danger colour on the panel, behind what failed or would
/// delete: a [`fail_title`], a hovered [`delete_button`].
fn danger_wash(p: &Palette) -> Color {
    mix(p.danger, p.panel, 0.14)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_reads_back_the_mode() {
        for mode in [Mode::Light, Mode::Dark] {
            assert_eq!(palette(&theme(mode)), mode.palette());
        }
    }

    fn luminance(color: Color) -> f32 {
        relative_luminance(color)
    }

    #[test]
    fn pick_tags_read_in_both_modes() {
        // WCAG AA for bold text, 3:1, for A and B on light and dark; the
        // dark palette's A, its light accent, with dark text.
        for mode in [Mode::Light, Mode::Dark] {
            let theme = theme(mode);
            for second in [false, true] {
                let style = pick_tag(second)(&theme);
                let Some(Background::Color(fill)) = style.background else {
                    panic!("a filled tag");
                };
                let text = style.text_color.unwrap();
                let (a, b) = (luminance(text), luminance(fill));
                let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                assert!(ratio >= 3.0, "{mode:?} {second}: {ratio}");
                if mode == Mode::Dark {
                    assert_eq!(text, LIGHT_TEXT);
                }
            }
        }
    }

    #[test]
    fn dark_danger_text_reads_on_the_dark_panel() {
        // WCAG AA for body text: 4.5:1.
        let p = Mode::Dark.palette();
        let ratio = (luminance(p.danger) + 0.05) / (luminance(p.panel) + 0.05);
        assert!(ratio >= 4.5, "{ratio}");
        assert_eq!(p.danger, color!(0xf07563));
    }

    #[test]
    fn a_disabled_button_fades_as_a_whole() {
        for mode in [Mode::Light, Mode::Dark] {
            let theme = theme(mode);
            for style in [primary_button, danger_button, secondary_button] {
                let enabled = style(&theme, button::Status::Active);
                let disabled = style(&theme, button::Status::Disabled);
                let faded = |color: Color| Color {
                    a: color.a * DISABLED_OPACITY,
                    ..color
                };
                assert_eq!(disabled.text_color, faded(enabled.text_color));
                let Some(Background::Color(fill)) = enabled.background else {
                    panic!("a filled button");
                };
                assert_eq!(disabled.background, Some(Background::Color(faded(fill))));
                assert_eq!(disabled.border.color, faded(enabled.border.color));
            }
        }
    }

    #[test]
    fn a_tick_has_normal_text_and_an_accent_box_on_hover() {
        use iced::widget::checkbox::Status;
        for mode in [Mode::Light, Mode::Dark] {
            let (theme, p) = (theme(mode), mode.palette());
            for is_checked in [false, true] {
                for status in [
                    Status::Active { is_checked },
                    Status::Hovered { is_checked },
                ] {
                    assert_eq!(tick(&theme, status).text_color, Some(p.text));
                }
            }
            let rest = tick(&theme, Status::Active { is_checked: false });
            assert_eq!(rest.border.color, p.faint);
            assert_eq!(rest.border.width, 1.0);
            let hovered = tick(&theme, Status::Hovered { is_checked: false });
            assert_eq!(hovered.border.color, p.accent);
            let on = tick(&theme, Status::Active { is_checked: true });
            assert_eq!(on.background, Background::Color(p.fill));
            assert_eq!(on.icon_color, Color::WHITE);
        }
    }

    #[test]
    fn a_tile_has_a_thin_border_and_an_accent_one_when_on() {
        for mode in [Mode::Light, Mode::Dark] {
            let (theme, p) = (theme(mode), mode.palette());
            let off = tile(false)(&theme, button::Status::Active);
            assert_eq!((off.border.width, off.border.color), (1.0, p.line));
            assert_eq!(off.background, Some(Background::Color(p.panel)));
            let hovered = tile(false)(&theme, button::Status::Hovered);
            assert_eq!(hovered.background, Some(Background::Color(p.hl)));
            let on = tile(true)(&theme, button::Status::Active);
            assert_eq!((on.border.width, on.border.color), (1.0, p.accent));
            // Its label stays the text's, as the mock's.
            assert_eq!(on.text_color, p.text);
        }
    }

    #[test]
    fn a_scrollbar_is_a_faint_scroller_on_no_rail() {
        use iced::widget::scrollable::Status;
        for mode in [Mode::Light, Mode::Dark] {
            let (theme, p) = (theme(mode), mode.palette());
            for status in [
                Status::Active {
                    is_horizontal_scrollbar_disabled: false,
                    is_vertical_scrollbar_disabled: false,
                },
                Status::Hovered {
                    is_horizontal_scrollbar_hovered: false,
                    is_vertical_scrollbar_hovered: true,
                    is_horizontal_scrollbar_disabled: false,
                    is_vertical_scrollbar_disabled: false,
                },
                Status::Dragged {
                    is_horizontal_scrollbar_dragged: false,
                    is_vertical_scrollbar_dragged: true,
                    is_horizontal_scrollbar_disabled: false,
                    is_vertical_scrollbar_disabled: false,
                },
            ] {
                let style = scrollbar(&theme, status);
                for rail in [style.vertical_rail, style.horizontal_rail] {
                    assert_eq!(rail.background, None);
                    assert_eq!(rail.scroller.background, Background::Color(p.faint));
                }
            }
        }
    }
}
