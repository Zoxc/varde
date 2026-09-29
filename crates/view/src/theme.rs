//! Colours and widget styles, with a light and a dark palette.
//!
//! Style functions take the iced [`Theme`] and look up the matching
//! [`Palette`] with [`palette`], so the same function serves both modes.

use std::sync::LazyLock;

use iced::theme::palette::Extended;
use iced::widget::{button, container, rule, text};
use iced::{
    Background, Border, Color, Font, Gradient, Radians, Shadow, Theme, Vector, border, color, font,
};
use varde_render::{Colors, Srgb};

/// Whether the UI is light or dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Light,
    Dark,
}

impl Mode {
    pub fn palette(self) -> &'static Palette {
        match self {
            Mode::Light => &LIGHT,
            Mode::Dark => &DARK,
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            Mode::Light => Mode::Dark,
            Mode::Dark => Mode::Light,
        }
    }
}

/// The design tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub accent: Color,
    /// Background of selected and toggled-on items.
    pub accent_soft: Color,
    /// Hover background.
    pub hl: Color,
    /// Hover border.
    pub hl_line: Color,
    pub text: Color,
    pub muted: Color,
    pub faint: Color,
    /// Borders and separators.
    pub line: Color,
    /// Outlines drawn over the viewport, like the view cube's.
    pub edge: Color,
    /// Key chip background.
    pub chip: Color,
    pub ok: Color,
    pub danger: Color,
    pub warning: Color,
    /// Dims the screen behind a dialog.
    pub scrim: Color,

    /// Title and status bar.
    pub title_bg: Color,
    /// Toolbar and side panel.
    pub panel: Color,
    pub tabstrip: Color,
    /// The viewport's scene, as the renderer draws it.
    pub scene: Colors,
    /// Icons for solids and bodies.
    pub solid: Color,
    /// View cube faces, turned away from the light and facing it.
    pub cube_shade: Color,
    pub cube_lit: Color,
}

// Scene colours the same in both palettes.
const AXES: [Srgb; 3] = [
    Srgb([0.85, 0.25, 0.22]),
    Srgb([0.30, 0.65, 0.25]),
    Srgb([0.20, 0.45, 0.85]),
];
const FEATURE_EDGE: Srgb = Srgb([0.12, 0.13, 0.15]);
const GRID: Srgb = Srgb([0.45, 0.49, 0.54]);
const ORIGIN_OUTLINE: Srgb = Srgb([0.2, 0.22, 0.25]);

/// `color` for the renderer, which takes no alpha.
const fn srgb(color: Color) -> Srgb {
    Srgb([color.r, color.g, color.b])
}

const LIGHT: Palette = Palette {
    accent: color!(0x0a95ad),
    accent_soft: color!(0x0a95ad, 0.13),
    hl: color!(0xe3f5da),
    hl_line: color!(0x9dd488),
    text: color!(0x2b3036),
    muted: color!(0x6c747d),
    faint: color!(0x9aa2ab),
    line: color!(0x141e28, 0.11),
    edge: color!(0x34303f),
    chip: color!(0x141e28, 0.07),
    ok: color!(0x3d9b35),
    danger: color!(0xe0564b),
    warning: color!(0xe8a317),
    scrim: color!(0x000000, 0.25),

    title_bg: color!(0xe9ebef),
    // Also the web page's background until the first frame, crates/web/index.html.
    panel: color!(0xfafafb),
    tabstrip: color!(0xeef0f3),
    scene: Colors {
        background_top: srgb(color!(0xf7f7f9)),
        background_bottom: srgb(color!(0xdfe1e6)),
        // hsl(258 16% 84%): the viewport's lighting spans roughly the
        // mock's 54-87% lightness with this base.
        model: srgb(color!(0xd4d0dd)),
        edge: FEATURE_EDGE,
        grid: GRID,
        axes: AXES,
        origin_outline: ORIGIN_OUTLINE,
    },
    solid: color!(0x8a6fc4),
    // hsl(258 10% 80%) to hsl(258 10% 96%).
    cube_shade: color!(0xcac7d1),
    cube_lit: color!(0xf4f4f6),
};

const DARK: Palette = Palette {
    accent: color!(0x39b9cf),
    accent_soft: color!(0x39b9cf, 0.16),
    hl: color!(0x76cc60, 0.15),
    hl_line: color!(0x76cc60, 0.5),
    text: color!(0xe4e7ea),
    muted: color!(0x9aa3ac),
    faint: color!(0x69727c),
    line: color!(0xffffff, 0.08),
    edge: color!(0x0e0d12),
    chip: color!(0xffffff, 0.08),
    ok: color!(0x5cc052),
    danger: color!(0xe0564b),
    warning: color!(0xe8a317),
    scrim: color!(0x000000, 0.45),

    title_bg: color!(0x17181d),
    panel: color!(0x212228),
    tabstrip: color!(0x1b1c21),
    scene: Colors {
        background_top: srgb(color!(0x2b2c32)),
        background_bottom: srgb(color!(0x1a1b1f)),
        // hsl(258 12% 59%), for the mock's 30-62% lightness.
        model: srgb(color!(0x918aa3)),
        edge: FEATURE_EDGE,
        grid: GRID,
        axes: AXES,
        origin_outline: ORIGIN_OUTLINE,
    },
    solid: color!(0xa896d6),
    // hsl(258 8% 30%) to hsl(258 8% 50%).
    cube_shade: color!(0x4a4653),
    cube_lit: color!(0x7b758a),
};

/// For names and headings.
pub const SEMIBOLD: Font = Font {
    weight: font::Weight::Semibold,
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
const CARD_RADIUS: f32 = 10.0;

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

/// The document toolbar.
pub fn toolbar(theme: &Theme) -> container::Style {
    let p = palette(theme);
    filled(p.panel, p.text)
}

/// The docked panel left of the viewport.
pub fn side_panel(theme: &Theme) -> container::Style {
    toolbar(theme)
}

/// A strip under the toolbar telling something about the whole document,
/// like it being read-only.
pub fn banner(theme: &Theme) -> container::Style {
    let p = palette(theme);
    filled(p.accent_soft, p.text)
}

/// The status bar along the bottom of the window.
pub fn status_bar(theme: &Theme) -> container::Style {
    let p = palette(theme);
    filled(p.title_bg, p.muted)
}

/// The strip holding the side panel's [`tab`]s.
pub fn tab_strip(theme: &Theme) -> container::Style {
    let p = palette(theme);
    filled(p.tabstrip, p.muted)
}

/// How a side panel [`tab`] shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabLook {
    /// On the strip, highlighted on hover.
    Flat,
    /// The selected tab, joining the panel below.
    Raised,
    /// The tab shown while the peek key is held: raised, with accent text.
    Peek,
}

impl TabLook {
    /// The colour of the tab's label at rest, and of its icon.
    pub fn content(self, p: &Palette) -> Color {
        match self {
            TabLook::Flat => p.muted,
            TabLook::Raised => p.text,
            TabLook::Peek => p.accent,
        }
    }
}

/// A folder tab in the [`tab_strip`], [`TAB_HEIGHT`] tall.
///
/// A raised tab has the panel colour, so it joins the panel below, and a
/// 2px accent line along its top.
pub fn tab(look: TabLook) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        match look {
            TabLook::Raised | TabLook::Peek => raised_tab(p, look.content(p)),
            TabLook::Flat if is_hovered(status) => button::Style {
                background: Some(Background::Color(p.hl)),
                text_color: p.text,
                ..flat_tab(p)
            },
            TabLook::Flat => flat_tab(p),
        }
    }
}

fn flat_tab(p: &Palette) -> button::Style {
    button::Style {
        background: None,
        text_color: TabLook::Flat.content(p),
        border: border::rounded(border::top(TAB_RADIUS)),
        ..button::Style::default()
    }
}

fn raised_tab(p: &Palette, text: Color) -> button::Style {
    // A hard-edged gradient, since iced has no inset shadow or one-sided
    // border. Starts at the top (angle PI).
    let line = 2.0 / TAB_HEIGHT;
    let gradient = iced::gradient::Linear::new(Radians::PI)
        .add_stop(0.0, p.accent)
        .add_stop(line, p.accent)
        .add_stop(line + 0.5 / TAB_HEIGHT, p.panel)
        .add_stop(1.0, p.panel);
    button::Style {
        background: Some(Background::Gradient(Gradient::Linear(gradient))),
        text_color: text,
        ..flat_tab(p)
    }
}

/// The colour of an enabled [`flat_button`]'s content at rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Text,
    Muted,
    Faint,
}

/// The colour of a [`flat_button`]'s content, for an icon in it, which
/// doesn't take the button's text colour: `tone` at rest, the text colour
/// while `hovered`, and faint unless `enabled`.
pub fn flat_content(p: &Palette, tone: Tone, enabled: bool, hovered: bool) -> Color {
    match (enabled, hovered, tone) {
        (false, _, _) | (true, false, Tone::Faint) => p.faint,
        (true, false, Tone::Muted) => p.muted,
        (true, true, _) | (true, false, Tone::Text) => p.text,
    }
}

/// A borderless button that only shows a background on hover, or when `on`.
/// Its text is [`Tone::Text`], or the accent colour when `on`.
pub fn flat_button(on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        let enabled = status != button::Status::Disabled;
        let hovered = is_hovered(status);
        let base = button::Style {
            background: None,
            text_color: flat_content(p, Tone::Text, enabled, hovered),
            border: border::rounded(CONTROL_RADIUS),
            ..button::Style::default()
        };

        if !enabled {
            base
        } else if on {
            button::Style {
                background: Some(Background::Color(p.accent_soft)),
                text_color: p.accent,
                ..base
            }
        } else if hovered {
            button::Style {
                background: Some(Background::Color(p.hl)),
                ..base
            }
        } else {
            base
        }
    }
}

/// Which of the two filled buttons a button is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    /// A [`primary_button`].
    Primary,
    /// A [`secondary_button`].
    Secondary,
}

impl Emphasis {
    pub fn button_style(self) -> fn(&Theme, button::Status) -> button::Style {
        match self {
            Emphasis::Primary => primary_button,
            Emphasis::Secondary => secondary_button,
        }
    }

    /// The style of a key chip on the button.
    pub fn key_chip(self) -> fn(&Theme) -> container::Style {
        match self {
            Emphasis::Primary => accent_key_chip,
            Emphasis::Secondary => key_chip,
        }
    }

    /// The colour of the button's content, for an icon in it, which doesn't
    /// take the button's text colour.
    pub fn content(self, p: &Palette) -> Color {
        match self {
            Emphasis::Primary => Color::WHITE,
            Emphasis::Secondary => p.text,
        }
    }
}

/// The main call to action: accent fill with white text.
pub fn primary_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let background = match status {
        button::Status::Disabled => p.accent.scale_alpha(0.5),
        _ if is_hovered(status) => brighten(p.accent, 1.08),
        _ => p.accent,
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: Emphasis::Primary.content(p),
        border: border::rounded(BUTTON_RADIUS),
        ..button::Style::default()
    }
}

/// A button beside a [`primary_button`]: panel fill with a thin border.
pub fn secondary_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let hovered = is_hovered(status);
    button::Style {
        background: Some(Background::Color(if hovered { p.hl } else { p.panel })),
        text_color: Emphasis::Secondary.content(p),
        border: outline(if hovered { p.hl_line } else { p.line }, BUTTON_RADIUS),
        ..button::Style::default()
    }
}

/// The welcome page behind its content, the colour of the viewport's top.
pub fn welcome(theme: &Theme) -> container::Style {
    let p = palette(theme);
    let Srgb([r, g, b]) = p.scene.background_top;
    filled(Color::from_rgb(r, g, b), p.text)
}

/// A clickable card, like a recent file on the welcome page.
pub fn card(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let hovered = is_hovered(status);
    button::Style {
        background: Some(Background::Color(p.panel)),
        text_color: p.text,
        border: outline(if hovered { p.hl_line } else { p.line }, CARD_RADIUS),
        ..button::Style::default()
    }
}

/// The thumbnail at the top of a [`card`], inside its 1 px border.
pub fn card_thumbnail(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: border::rounded(border::top(CARD_RADIUS - 1.0)),
        ..filled(p.tabstrip, p.text)
    }
}

/// The text under a [`card_thumbnail`], highlighted while the card is
/// `hovered`.
pub fn card_meta(theme: &Theme, hovered: bool) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: hovered.then_some(Background::Color(p.hl)),
        border: border::rounded(border::bottom(CARD_RADIUS - 1.0)),
        ..container::Style::default()
    }
}

/// A key label, like `Alt` in a hint. Use a monospace font inside.
pub fn key_chip(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: outline(p.line, CHIP_RADIUS),
        ..filled(p.chip, p.muted)
    }
}

/// A [`key_chip`] on a [`primary_button`].
pub fn accent_key_chip(_theme: &Theme) -> container::Style {
    container::Style {
        border: outline(Color::from_rgba(1.0, 1.0, 1.0, 0.25), CHIP_RADIUS),
        ..filled(Color::from_rgba(1.0, 1.0, 1.0, 0.18), Color::WHITE)
    }
}

/// The file cell at the left of the toolbar, which opens the file menu.
/// Square, so it fills its toolbar cell; highlighted while the menu is open.
pub fn file_cell(open: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        let hovered = is_hovered(status);
        button::Style {
            background: (open || hovered).then_some(Background::Color(p.hl)),
            text_color: p.text,
            ..button::Style::default()
        }
    }
}

/// The dot after an edited document's name in the toolbar's file cell.
pub fn dirty_dot(theme: &Theme) -> container::Style {
    container::Style {
        border: border::rounded(3),
        ..container::background(palette(theme).muted)
    }
}

/// A drop-down menu, floating over everything else.
pub fn menu(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: outline(p.line, MENU_RADIUS),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.18),
            offset: Vector::new(0.0, 8.0),
            blur_radius: 30.0,
        },
        ..filled(p.panel, p.text)
    }
}

/// Dims the screen behind a dialog.
pub fn scrim(theme: &Theme) -> container::Style {
    container::background(palette(theme).scrim)
}

/// A group of controls floating over the viewport.
pub fn float_panel(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: outline(p.line, FLOAT_RADIUS),
        ..filled(p.panel, p.muted)
    }
}

/// A single button floating over the viewport, styled like a
/// [`float_panel`].
pub fn float_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let hovered = is_hovered(status);
    button::Style {
        background: Some(Background::Color(if hovered { p.hl } else { p.panel })),
        text_color: if hovered { p.text } else { p.muted },
        border: outline(p.line, FLOAT_RADIUS),
        ..button::Style::default()
    }
}

/// A list row under the cursor, like a body in the side panel.
pub fn hovered_row(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: border::rounded(CONTROL_RADIUS),
        ..filled(p.hl, p.text)
    }
}

pub fn muted_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(palette(theme).muted),
    }
}

pub fn faint_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(palette(theme).faint),
    }
}

pub fn danger_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(palette(theme).danger),
    }
}

pub fn separator(theme: &Theme) -> rule::Style {
    rule::Style {
        color: palette(theme).line,
        ..rule::default(theme)
    }
}

/// Multiplies each channel, like CSS `filter: brightness()`.
fn brighten(color: Color, factor: f32) -> Color {
    Color {
        r: (color.r * factor).min(1.0),
        g: (color.g * factor).min(1.0),
        b: (color.b * factor).min(1.0),
        ..color
    }
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
}
