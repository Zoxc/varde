//! Colours and widget styles, with a light and a dark palette.
//!
//! Style functions take the iced [`Theme`] and look up the matching
//! [`Palette`] with [`palette`], so the same function serves both modes.

use std::sync::LazyLock;

use iced::theme::palette::Extended;
use iced::widget::slider::{self as slide, HandleShape};
use iced::widget::{button, checkbox, container, rule, scrollable, text, text_input};
use iced::{Background, Border, Color, Font, Shadow, Theme, Vector, border, color, font};
use varde_render::{Colors, Srgb, Srgba};

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
}

/// The theme the user chose: a [`Mode`], or the one the system prefers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    #[default]
    Auto,
    Light,
    Dark,
}

impl ThemeChoice {
    /// The next choice, as the theme button goes through them.
    pub fn cycled(self) -> Self {
        match self {
            ThemeChoice::Auto => ThemeChoice::Light,
            ThemeChoice::Light => ThemeChoice::Dark,
            ThemeChoice::Dark => ThemeChoice::Auto,
        }
    }

    /// The mode chosen, `system`'s if it's [`ThemeChoice::Auto`]: light
    /// when the system doesn't say.
    pub fn mode(self, system: Option<Mode>) -> Mode {
        match self {
            ThemeChoice::Auto => system.unwrap_or_default(),
            ThemeChoice::Light => Mode::Light,
            ThemeChoice::Dark => Mode::Dark,
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
    /// Danger text, like an error.
    pub danger: Color,
    /// The fill of a [`danger_button`], under white text.
    pub danger_fill: Color,
    /// Darker danger text, for words that must read as failed on the
    /// panel: a failed feature's name, Add anyway (the mock's
    /// `--danger-text`).
    pub danger_strong: Color,
    /// Warning text, for what may go wrong but isn't wrong yet: the
    /// mock's construction colour, as its panels' warnings.
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
    /// The tools' icons, by category.
    pub icons: IconColors,
    /// View cube faces, turned away from the light and facing it.
    pub cube_shade: Color,
    pub cube_lit: Color,
    /// The sketch being edited, as the viewport draws it.
    pub sketching: SketchColors,
}

/// The colours of the sketch being edited, by the state of what's drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SketchColors {
    /// Curves and the rims of points, while the constraints leave them
    /// free to move.
    pub curve: Color,
    /// Curves and points the constraints fix: darker.
    pub fixed: Color,
    /// What's in a conflict, or a refused edit ran into, and glyphs of
    /// constraints in one.
    pub conflict: Color,
    /// Construction curves, drawn dashed.
    pub construction: Color,
    /// What's selected.
    pub selected: Color,
    /// What's under the cursor.
    pub hovered: Color,
    /// Inside the points' rims: the mock's halo.
    pub point_fill: Color,
    /// The shape a tool is drawing, before it's placed.
    pub preview: Color,
    /// The box dragged to select, filled and outlined.
    pub box_fill: Color,
    pub box_line: Color,
    /// The sketch's origin and axes.
    pub axis: Color,
    /// Where a drawing tool snaps: its glyph, and its guides, dashed.
    pub guide: Color,
    /// The regions the curves enclose, shaded lightly under them.
    pub region: Color,
    /// The region under the cursor, over its shading.
    pub region_hovered: Color,
    /// The rings marking open ends that almost meet.
    pub near_miss: Color,
}

// Scene colours the same in both palettes.
const AXES: [Srgb; 3] = [
    Srgb([0.76, 0.28, 0.25]),
    Srgb([0.35, 0.63, 0.31]),
    Srgb([0.25, 0.45, 0.77]),
];
const FEATURE_EDGE: Srgb = Srgb([0.12, 0.13, 0.15]);
const GRID: Srgb = Srgb([0.45, 0.49, 0.54]);
const ORIGIN_OUTLINE: Srgb = Srgb([0.2, 0.22, 0.25]);
/// The model behind a sketch being edited: the mock's ghosted model.
const FADED_ALPHA: f32 = 0.3;
/// The edges the model hides, dashed over it.
const HIDDEN_EDGE_ALPHA: f32 = 0.45;
/// Error geometry, solid: the app's red, as a sketch's conflict and the
/// danger button are.
const ERROR: Color = color!(0xe0564b);
/// The halo around error geometry: the same red, translucent.
const ERROR_HALO: Srgba = Srgba([ERROR.r, ERROR.g, ERROR.b, 0.3]);

/// `color` for the renderer, which takes no alpha.
const fn srgb(color: Color) -> Srgb {
    Srgb([color.r, color.g, color.b])
}

/// `color` with the opacity `a`.
const fn alpha(color: Color, a: f32) -> Color {
    Color { a, ..color }
}

/// What an icon stands for, which picks its colours: the categories of
/// `notes/ui-mock-icons.html`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconCategory {
    /// Sketches and drawing their curves.
    Sketch,
    /// Changing curves: trim, extend, offset, mirror.
    Modify,
    /// Constraints.
    Constraint,
    Dimension,
    /// Solids and bodies.
    Solid,
    /// Construction geometry: planes.
    Construction,
    /// Looking into the model: measuring.
    Inspect,
    /// The design's file: opening and saving it.
    File,
}

/// The colours of one category's icons.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IconTone {
    /// The tool's own geometry.
    pub line: Color,
    /// What it does to it: arrows, picked points, the relation it makes.
    pub accent: Color,
    /// The geometry it works from or against, faint.
    pub reference: Color,
}

/// The icons' colours by [`IconCategory`]: the icon mock's defaults, its
/// "Distinct, purple constraints" colours with the softer accent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IconColors {
    pub sketch: IconTone,
    pub modify: IconTone,
    pub constraint: IconTone,
    pub dimension: IconTone,
    pub solid: IconTone,
    pub construction: IconTone,
    pub inspect: IconTone,
    pub file: IconTone,
}

impl IconColors {
    pub fn tone(&self, category: IconCategory) -> IconTone {
        match category {
            IconCategory::Sketch => self.sketch,
            IconCategory::Modify => self.modify,
            IconCategory::Constraint => self.constraint,
            IconCategory::Dimension => self.dimension,
            IconCategory::Solid => self.solid,
            IconCategory::Construction => self.construction,
            IconCategory::Inspect => self.inspect,
            IconCategory::File => self.file,
        }
    }
}

/// `a` mixed with `share` of it into `b`, opaque, as CSS `color-mix` in
/// sRGB does.
const fn mix(a: Color, b: Color, share: f32) -> Color {
    Color {
        r: a.r * share + b.r * (1.0 - share),
        g: a.g * share + b.g * (1.0 - share),
        b: a.b * share + b.b * (1.0 - share),
        a: 1.0,
    }
}

/// A category's icon colours from its colour, as the icon mock derives
/// them: the line is the colour with `darken` of black mixed in, the
/// accent is the category's `accent` mixed 60% into the line (the softer
/// accent), and reference geometry is a grey halfway from `panel` to
/// `text`, mixed 92% into the line.
const fn icon_tone(
    color: Color,
    darken: f32,
    accent: Color,
    text: Color,
    panel: Color,
) -> IconTone {
    let line = mix(color, Color::BLACK, 1.0 - darken);
    IconTone {
        line,
        accent: mix(accent, line, 0.6),
        reference: mix(mix(text, panel, 0.52), line, 0.92),
    }
}

// Each category's accent stands out from its colour: orange, unless the
// category is itself red to yellow (then blue) or pink to magenta (then
// teal). The file's is the text's, as orange looked odd on Save.
const LIGHT_ORANGE: Color = color!(0xe88a00);
const LIGHT_BLUE: Color = color!(0x2f6fd8);
const LIGHT_TEAL: Color = color!(0x0d9a88);
const DARK_ORANGE: Color = color!(0xffad33);
const DARK_BLUE: Color = color!(0x78aaff);
const DARK_TEAL: Color = color!(0x3fd3bf);

const LIGHT_TEXT: Color = color!(0x2b3036);
// Also the web page's background until the first frame, crates/web/index.html.
const LIGHT_PANEL: Color = color!(0xfafafb);
const DARK_TEXT: Color = color!(0xe4e7ea);
const DARK_PANEL: Color = color!(0x212228);

const LIGHT_ICONS: IconColors = {
    const fn tone(color: Color, darken: f32, accent: Color) -> IconTone {
        icon_tone(color, darken, accent, LIGHT_TEXT, LIGHT_PANEL)
    }
    IconColors {
        sketch: tone(color!(0x0a95ad), 0.0, LIGHT_ORANGE),
        modify: tone(color!(0xcf3f30), 0.1, LIGHT_BLUE),
        constraint: tone(color!(0x8a3fd0), 0.12, LIGHT_ORANGE),
        dimension: tone(color!(0x5f6b80), 0.15, LIGHT_ORANGE),
        solid: tone(color!(0xc0409a), 0.0, LIGHT_TEAL),
        construction: tone(color!(0xc39000), 0.08, LIGHT_BLUE),
        inspect: tone(color!(0x2f9a47), 0.0, LIGHT_ORANGE),
        file: tone(LIGHT_BLUE, 0.0, LIGHT_TEXT),
    }
};

const DARK_ICONS: IconColors = {
    const fn tone(color: Color, darken: f32, accent: Color) -> IconTone {
        icon_tone(color, darken, accent, DARK_TEXT, DARK_PANEL)
    }
    IconColors {
        sketch: tone(color!(0x3cc3d9), 0.0, DARK_ORANGE),
        modify: tone(color!(0xff7a66), 0.1, DARK_BLUE),
        constraint: tone(color!(0xb48cff), 0.12, DARK_ORANGE),
        dimension: tone(color!(0xa6b2c6), 0.15, DARK_ORANGE),
        solid: tone(color!(0xef77c2), 0.0, DARK_TEAL),
        construction: tone(color!(0xf0c43c), 0.08, DARK_BLUE),
        inspect: tone(color!(0x62cf7c), 0.0, DARK_ORANGE),
        file: tone(DARK_BLUE, 0.0, DARK_TEXT),
    }
};

// The mock's colours for sketches and for construction, used by the
// renderer and the sketch being edited alike.
const LIGHT_SKETCH: Color = color!(0x0a95ad);
const LIGHT_CONSTRUCTION: Color = color!(0xe0861a);
const DARK_SKETCH: Color = color!(0x39b9cf);
const DARK_CONSTRUCTION: Color = color!(0xf0a24a);

const LIGHT: Palette = Palette {
    accent: color!(0x0a95ad),
    accent_soft: color!(0x0a95ad, 0.13),
    hl: color!(0xe3f5da),
    hl_line: color!(0x9dd488),
    text: LIGHT_TEXT,
    muted: color!(0x6c747d),
    faint: color!(0x9aa2ab),
    line: color!(0x141e28, 0.11),
    edge: color!(0x34303f),
    chip: color!(0x141e28, 0.07),
    ok: color!(0x3d9b35),
    danger: color!(0xe0564b),
    danger_fill: color!(0xe0564b),
    danger_strong: color!(0xad2a19),
    warning: LIGHT_CONSTRUCTION,
    scrim: color!(0x000000, 0.25),

    title_bg: color!(0xe9ebef),
    panel: LIGHT_PANEL,
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
        // The accent.
        pivot: srgb(color!(0x0a95ad)),
        sketch: srgb(LIGHT_SKETCH),
        faded_alpha: FADED_ALPHA,
        hidden_edge_alpha: HIDDEN_EDGE_ALPHA,
        // Neutral, lighter than the model.
        hover_face: srgb(color!(0xf4f4f4)),
        // White: brighter than the lit faces, and around the dark edge
        // it shows on the light background too.
        hover_outline: srgb(color!(0xffffff)),
        // The accent.
        selected: srgb(color!(0x0a95ad)),
        // A light tint, so the selected edges, the accent a little
        // darker, show on it and apart from the near black edges.
        selected_tint: 0.3,
        selected_edge_shade: -0.3,
        // The construction colour: the measure tool's B, apart from A in
        // the accent.
        second: srgb(LIGHT_CONSTRUCTION),
        error: srgb(ERROR),
        error_halo: ERROR_HALO,
    },
    icons: LIGHT_ICONS,
    // hsl(258 10% 80%) to hsl(258 10% 96%).
    cube_shade: color!(0xcac7d1),
    cube_lit: color!(0xf4f4f6),
    sketching: SketchColors {
        curve: LIGHT_SKETCH,
        fixed: color!(0x0b5566),
        conflict: ERROR,
        construction: LIGHT_CONSTRUCTION,
        selected: color!(0x2f5fd8),
        hovered: color!(0x3d9b35),
        point_fill: color!(0xf4f4f7),
        preview: alpha(LIGHT_SKETCH, 0.75),
        box_fill: alpha(LIGHT_SKETCH, 0.08),
        box_line: alpha(LIGHT_SKETCH, 0.7),
        axis: color!(0x8a939e),
        guide: color!(0xc2701d),
        region: alpha(LIGHT_SKETCH, 0.1),
        region_hovered: color!(0x3d9b35, 0.18),
        near_miss: color!(0xe0564b),
    },
};

const DARK: Palette = Palette {
    accent: color!(0x39b9cf),
    accent_soft: color!(0x39b9cf, 0.16),
    hl: color!(0x76cc60, 0.15),
    hl_line: color!(0x76cc60, 0.5),
    text: DARK_TEXT,
    muted: color!(0x9aa3ac),
    faint: color!(0x69727c),
    line: color!(0xffffff, 0.08),
    edge: color!(0x0e0d12),
    chip: color!(0xffffff, 0.08),
    ok: color!(0x5cc052),
    // Lighter than the light palette's, to read on the dark panel.
    danger: color!(0xf07563),
    danger_fill: color!(0xe0564b),
    danger_strong: color!(0xf58a7a),
    warning: DARK_CONSTRUCTION,
    scrim: color!(0x000000, 0.45),

    title_bg: color!(0x17181d),
    panel: DARK_PANEL,
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
        // The accent.
        pivot: srgb(color!(0x39b9cf)),
        sketch: srgb(DARK_SKETCH),
        faded_alpha: FADED_ALPHA,
        hidden_edge_alpha: HIDDEN_EDGE_ALPHA,
        // Neutral, lighter than the model.
        hover_face: srgb(color!(0xbababa)),
        hover_outline: srgb(color!(0xffffff)),
        // The accent.
        selected: srgb(color!(0x39b9cf)),
        selected_tint: 0.6,
        // Lighter than a selected face's tint, to show on it.
        selected_edge_shade: 0.85,
        second: srgb(DARK_CONSTRUCTION),
        error: srgb(ERROR),
        error_halo: ERROR_HALO,
    },
    icons: DARK_ICONS,
    // hsl(258 8% 30%) to hsl(258 8% 50%).
    cube_shade: color!(0x4a4653),
    cube_lit: color!(0x7b758a),
    sketching: SketchColors {
        curve: DARK_SKETCH,
        fixed: color!(0x1f8394),
        conflict: ERROR,
        construction: DARK_CONSTRUCTION,
        selected: color!(0x7ea2ff),
        hovered: color!(0x76cc60),
        point_fill: color!(0x24252b),
        preview: alpha(DARK_SKETCH, 0.75),
        box_fill: alpha(DARK_SKETCH, 0.1),
        box_line: alpha(DARK_SKETCH, 0.7),
        axis: color!(0x6b737d),
        guide: color!(0xf0a24a),
        region: alpha(DARK_SKETCH, 0.12),
        region_hovered: color!(0x76cc60, 0.2),
        near_miss: color!(0xe0564b),
    },
};

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

/// A folder tab in the [`tab_strip`], [`TAB_HEIGHT`] tall, around a
/// [`tab_face`] with [`TAB_LINE`] of padding at the top.
///
/// A raised tab has the panel colour, so it joins the panel below, and a
/// 2px accent line along its top: the button is the accent, and its face
/// the panel colour below the line. A gradient would do it in one quad,
/// but iced draws none on the web.
pub fn tab(look: TabLook) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        let background = match look {
            TabLook::Raised | TabLook::Peek => Some(p.accent),
            TabLook::Flat if is_hovered(status) => Some(p.hl),
            TabLook::Flat => None,
        };
        let text_color = match look {
            TabLook::Flat if is_hovered(status) => p.text,
            _ => look.content(p),
        };
        button::Style {
            background: background.map(Background::Color),
            text_color,
            border: border::rounded(border::top(TAB_RADIUS)),
            ..button::Style::default()
        }
    }
}

/// How far a [`tab`]'s face is below its top: the raised tab's accent line.
pub const TAB_LINE: f32 = 2.0;

/// The face of a [`tab`] below its accent line: the panel colour when
/// raised. Its top corners are rounded less than the tab's, so they stay
/// inside the tab's: they meet its sides where its corners end.
pub fn tab_face(look: TabLook) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = palette(theme);
        container::Style {
            background: (look != TabLook::Flat).then_some(Background::Color(p.panel)),
            border: border::rounded(border::top(TAB_RADIUS - TAB_LINE)),
            ..container::Style::default()
        }
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
    filled_button(p, p.accent, status)
}

/// A [`primary_button`] closing the right end of a [`pill`]: square on
/// its left, where it joins the pill.
pub fn pill_end_button(theme: &Theme, status: button::Status) -> button::Style {
    let style = primary_button(theme, status);
    button::Style {
        border: Border {
            radius: border::Radius::new(0)
                .top_right(CONTROL_RADIUS)
                .bottom_right(CONTROL_RADIUS),
            ..style.border
        },
        ..style
    }
}

/// The main call to action where it destroys something: danger fill with
/// white text.
pub fn danger_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    filled_button(p, p.danger_fill, status)
}

/// How opaque a disabled button is, all of it: the mock's `opacity`.
const DISABLED_OPACITY: f32 = 0.45;

/// `style` faded as a whole, as a disabled button is: its fill, text and
/// border at [`DISABLED_OPACITY`]. Fading the fill alone would leave white
/// text on a dark panel looking pressable.
fn faded(style: button::Style) -> button::Style {
    let fade = |color: Color| color.scale_alpha(DISABLED_OPACITY);
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
    if status == button::Status::Disabled {
        faded(style)
    } else {
        style
    }
}

/// A button beside a [`primary_button`]: panel fill with a thin border.
pub fn secondary_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let hovered = is_hovered(status);
    let style = button::Style {
        background: Some(Background::Color(if hovered { p.hl } else { p.panel })),
        text_color: Emphasis::Secondary.content(p),
        border: outline(if hovered { p.hl_line } else { p.line }, BUTTON_RADIUS),
        ..button::Style::default()
    };
    if status == button::Status::Disabled {
        faded(style)
    } else {
        style
    }
}

/// The colour of the operation panel's recessed well, which its fields
/// sit in: the text's 6% into the panel's, as the tool rail's strip.
pub fn well(p: &Palette) -> Color {
    mix(p.text, p.panel, 0.06)
}

/// The colour of the operation panel's card behind its head, and of its
/// controls on the well.
fn control_fill(p: &Palette, enabled: bool) -> Color {
    if enabled {
        p.panel
    } else {
        p.panel.scale_alpha(DISABLED_OPACITY)
    }
}

/// A square icon button in the operation panel's head, Cancel or (the
/// `primary`) OK: a thin border, the hover background on hover; OK filled
/// with the accent, faded while disabled.
pub fn head_button(primary: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        if primary {
            return button::Style {
                border: border::rounded(CONTROL_RADIUS),
                ..filled_button(p, p.accent, status)
            };
        }
        let hovered = is_hovered(status);
        button::Style {
            background: hovered.then_some(Background::Color(p.hl)),
            text_color: if hovered { p.text } else { p.muted },
            border: outline(p.line, CONTROL_RADIUS),
            ..button::Style::default()
        }
    }
}

/// One of a few choices in the operation panel, like an extrude's
/// extent, as a tile (its icon over its label): a thin border on the
/// panel's colour, the hover background on hover, and while `on` a 1 px
/// accent border on the soft accent, its label still the text's. Faint
/// while disabled.
pub fn tile(on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        let enabled = status != button::Status::Disabled;
        let base = button::Style {
            background: Some(Background::Color(control_fill(p, enabled))),
            text_color: flat_content(p, Tone::Text, enabled, false),
            border: outline(p.line, CONTROL_RADIUS),
            ..button::Style::default()
        };
        if !enabled {
            base
        } else if on {
            button::Style {
                background: Some(Background::Color(mix(p.accent, p.panel, 0.13))),
                border: outline(p.accent, CONTROL_RADIUS),
                ..base
            }
        } else if is_hovered(status) {
            button::Style {
                background: Some(Background::Color(p.hl)),
                ..base
            }
        } else {
            base
        }
    }
}

/// An option's toggle in the operation panel, its icon's button beside
/// its name: the row takes the hover background on hover.
pub fn toggle_row(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let enabled = status != button::Status::Disabled;
    button::Style {
        background: (enabled && is_hovered(status)).then_some(Background::Color(p.hl)),
        text_color: flat_content(p, Tone::Text, enabled, false),
        border: border::rounded(CONTROL_RADIUS + 1.0),
        ..button::Style::default()
    }
}

/// The square holding an option's icon in its [`toggle_row`]: a thin
/// border on the panel's colour, the accent's border while `hovered`, and
/// while `on` the accent's border on the soft accent.
pub fn toggle_square(on: bool, hovered: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = palette(theme);
        let (fill, edge) = match (on, hovered) {
            (true, false) => (mix(p.accent, p.panel, 0.13), p.accent),
            (true, true) => (mix(p.accent, p.panel, 0.26), p.accent),
            (false, true) => (p.panel, p.accent),
            (false, false) => (p.panel, p.line),
        };
        container::Style {
            border: outline(edge, CONTROL_RADIUS),
            ..filled(fill, p.text)
        }
    }
}

/// A field of the operation panel picked into by clicks in the viewport:
/// a thin border on the panel's colour, the accent's while it's the one
/// picking (`on`).
pub fn pick_box(on: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = palette(theme);
        container::Style {
            border: outline(if on { p.accent } else { p.line }, CONTROL_RADIUS),
            ..filled(p.panel, p.text)
        }
    }
}

/// A row of what's picked in a [`pick_box`], as a Timeline row: the hover
/// background on hover, or while `hovered` (the app knows it is, which a
/// row with nothing to press can't tell from its status).
pub fn picked_row(hovered: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        button::Style {
            background: (hovered || is_hovered(status)).then_some(Background::Color(p.hl)),
            text_color: p.text,
            border: border::rounded(CONTROL_RADIUS),
            ..button::Style::default()
        }
    }
}

/// A row of an operation's Bodies list, in a [`pick_box`] as the picked
/// rows are: the hover background while `hovered` (it, or its body in
/// the viewport, as the app knows).
pub fn body_row(hovered: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = palette(theme);
        container::Style {
            background: hovered.then_some(Background::Color(p.hl)),
            border: border::rounded(CONTROL_RADIUS),
            ..container::Style::default()
        }
    }
}

/// The cross taking something picked out of its field: bare, a grey tile
/// on hover.
pub fn remove_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    button::Style {
        background: is_hovered(status).then_some(Background::Color(p.text.scale_alpha(0.1))),
        text_color: p.text,
        border: border::rounded(CONTROL_RADIUS - 1.0),
        ..button::Style::default()
    }
}

/// Where a [`pick_box`] is clicked for more, its foot under a rule, or
/// alone its one row (`alone`): the hover background on hover, its
/// corners the box's inside's.
pub fn place_line(alone: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        let inner = CONTROL_RADIUS - 1.0;
        let radius = if alone {
            border::Radius::new(inner)
        } else {
            border::Radius::new(0).bottom(inner)
        };
        button::Style {
            background: is_hovered(status).then_some(Background::Color(p.hl)),
            text_color: p.muted,
            border: Border {
                radius,
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}

/// A typed value's field in the operation panel: the panel's colour on
/// the well, a thin border, the accent's while focused or hovered, the
/// danger colour's while its text is refused (`bad`).
pub fn field_input(bad: bool) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |theme, status| {
        let p = palette(theme);
        let edge = match status {
            _ if bad => p.danger,
            text_input::Status::Focused { .. } => p.accent,
            text_input::Status::Hovered => p.hl_line,
            text_input::Status::Active | text_input::Status::Disabled => p.line,
        };
        let disabled = status == text_input::Status::Disabled;
        text_input::Style {
            background: Background::Color(control_fill(p, !disabled)),
            border: outline(edge, CONTROL_RADIUS),
            icon: p.muted,
            placeholder: p.faint,
            value: if disabled { p.muted } else { p.text },
            selection: p.accent_soft,
        }
    }
}

/// The box of a draft that fails, at the operation panel's foot: the
/// panel's colour under muted words.
pub fn fail_box(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: border::rounded(CONTROL_RADIUS),
        ..filled(p.panel, p.muted)
    }
}

/// The title of a [`fail_box`]: the danger colour's wash, its words the
/// text's.
pub fn fail_title(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: border::rounded(border::Radius::new(0).top(CONTROL_RADIUS)),
        ..filled(mix(p.danger, p.panel, 0.14), p.text)
    }
}

/// Add anyway, in a [`fail_box`]: a thin border on the panel's colour,
/// its words in the strong danger colour.
pub fn add_anyway(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let style = button::Style {
        background: Some(Background::Color(if is_hovered(status) {
            p.hl
        } else {
            p.panel
        })),
        text_color: p.danger_strong,
        border: outline(p.line, CONTROL_RADIUS),
        ..button::Style::default()
    };
    if status == button::Status::Disabled {
        faded(style)
    } else {
        style
    }
}

/// Corner radius of a [`tick`]'s box.
const TICK_RADIUS: f32 = 4.0;

/// A checkbox: a faint box whose border turns accent on hover, filled
/// with the accent and a white check while ticked; its label in the
/// text colour, whatever the colour around it.
pub fn tick(theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    let p = palette(theme);
    let (checked, hovered, enabled) = match status {
        checkbox::Status::Active { is_checked } => (is_checked, false, true),
        checkbox::Status::Hovered { is_checked } => (is_checked, true, true),
        checkbox::Status::Disabled { is_checked } => (is_checked, false, false),
    };
    let edge = if checked || hovered {
        p.accent
    } else {
        p.faint
    };
    let style = checkbox::Style {
        background: Background::Color(if checked {
            p.accent
        } else {
            Color::TRANSPARENT
        }),
        icon_color: Color::WHITE,
        border: outline(edge, TICK_RADIUS),
        text_color: Some(p.text),
    };
    if enabled {
        style
    } else {
        let fade = |color: Color| color.scale_alpha(DISABLED_OPACITY);
        checkbox::Style {
            background: Background::Color(if checked {
                fade(p.accent)
            } else {
                Color::TRANSPARENT
            }),
            border: outline(fade(edge), TICK_RADIUS),
            text_color: Some(fade(p.text)),
            ..style
        }
    }
}

/// The radius of a [`slider`]'s handle.
pub const SLIDER_HANDLE_RADIUS: f32 = 6.0;

/// A slider: a thin rail, accent up to the handle and the line colour
/// after it, and a round handle in the panel's colour ringed in the
/// accent, filled with the hover colour while hovered or dragged; faded
/// as a disabled button is unless `enabled`.
pub fn slider(enabled: bool) -> impl Fn(&Theme, slide::Status) -> slide::Style {
    move |theme, status| {
        let p = palette(theme);
        let fade = |color: Color| {
            if enabled {
                color
            } else {
                color.scale_alpha(DISABLED_OPACITY)
            }
        };
        let fill = match status {
            slide::Status::Hovered | slide::Status::Dragged if enabled => {
                mix(p.accent, p.panel, 0.2)
            }
            _ => p.panel,
        };
        slide::Style {
            rail: slide::Rail {
                backgrounds: (
                    Background::Color(fade(p.accent)),
                    Background::Color(fade(p.line)),
                ),
                width: 4.0,
                border: border::rounded(2.0),
            },
            handle: slide::Handle {
                shape: HandleShape::Circle {
                    radius: SLIDER_HANDLE_RADIUS,
                },
                background: Background::Color(fill),
                border_width: 2.0,
                border_color: fade(p.accent),
            },
        }
    }
}

/// Width of a scrollbar's scroller, in pixels. It floats over the
/// content's edge or in a padding left for it.
pub const SCROLLBAR_WIDTH: f32 = 4.0;

/// Every scrollable's scrollbar: a thin faint scroller on no rail, as
/// the mock's thin scrollbars.
pub fn scrollbar(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let p = palette(theme);
    let rail = scrollable::Rail {
        background: None,
        border: Border::default(),
        scroller: scrollable::Scroller {
            background: Background::Color(p.faint),
            border: border::rounded(SCROLLBAR_WIDTH / 2.0),
        },
    };
    scrollable::Style {
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        ..scrollable::default(theme, status)
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
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.08),
            offset: Vector::new(0.0, 3.0),
            blur_radius: 10.0,
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

/// The shadow under the operation panel's card, as the mock's.
pub const CARD_SHADOW: Shadow = Shadow {
    color: Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.1,
    },
    offset: Vector { x: 0.0, y: 6.0 },
    blur_radius: 18.0,
};

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

/// Corner radius of a card of the tool rail ([`rail_card`]); the parts
/// inside its border are one less.
pub const RAIL_CARD_RADIUS: f32 = 8.0;

/// A card of the tool rail over the viewport: one tool set.
pub fn rail_card(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: outline(p.line, RAIL_CARD_RADIUS),
        ..filled(p.panel, p.text)
    }
}

/// The head of a card of the tool rail, its whole width: highlighted on
/// hover and while its set's list is `open`, which it opens on hover, so
/// the two look the same. Its top corners follow the card's, and its
/// bottom ones too without a strip under it (`over_strip`).
pub fn rail_head(open: bool, over_strip: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        let inner = RAIL_CARD_RADIUS - 1.0;
        let radius = if over_strip {
            border::Radius::new(0).top(inner)
        } else {
            border::Radius::new(inner)
        };
        button::Style {
            background: (open || is_hovered(status)).then_some(Background::Color(p.hl)),
            text_color: p.text,
            border: Border {
                radius,
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}

/// The recessed strip of a tool rail card's first tools, along its
/// bottom: rounded at its top, and at its bottom as the card's inside.
pub fn rail_strip(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: Border {
            radius: border::Radius::new(RAIL_CARD_RADIUS - 1.0).top(CONTROL_RADIUS),
            ..Border::default()
        },
        ..filled(well(p), p.text)
    }
}

/// What's behind a tool rail card's strip: the head's highlight while its
/// set is `open`, showing in the strip's rounded top corners.
pub fn rail_strip_backing(open: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| container::Style {
        background: open.then_some(Background::Color(palette(theme).hl)),
        border: border::rounded(border::Radius::new(0).bottom(RAIL_CARD_RADIUS - 1.0)),
        ..container::Style::default()
    }
}

/// A row of a tool rail's list: a [`flat_button`], highlighted as on
/// hover while the keys are on it (`focused`), disabled or not, unless
/// it's `on`.
pub fn rail_row(on: bool, focused: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let style = flat_button(on)(theme, status);
        if focused && !(on && status != button::Status::Disabled) {
            button::Style {
                background: Some(Background::Color(palette(theme).hl)),
                ..style
            }
        } else {
            style
        }
    }
}

/// Corner radius of the list of a tool rail's set ([`rail_list_band`]).
const RAIL_LIST_RADIUS: f32 = 10.0;

/// How tall the band in the set's colour along a rail list's top is.
pub const RAIL_LIST_BAND: f32 = 3.0;

/// The list of a tool rail's set, beside its card, inside its
/// [`rail_list_band`].
pub fn rail_list(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: Border {
            radius: border::Radius::new(RAIL_LIST_RADIUS).top(RAIL_LIST_RADIUS - RAIL_LIST_BAND),
            ..Border::default()
        },
        ..filled(p.panel, p.text)
    }
}

/// What a tool rail's list sits in: the colour of its set's `category`,
/// showing as a band along its top, and a menu's shadow.
pub fn rail_list_band(category: IconCategory) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = palette(theme);
        container::Style {
            border: border::rounded(RAIL_LIST_RADIUS),
            shadow: menu(theme).shadow,
            ..filled(p.icons.tone(category).line, p.text)
        }
    }
}

/// A knob of the extrude handle over the viewport: an accent dot with a
/// rim in the panel's colour, to show on any face.
pub fn knob(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: Border {
            color: p.panel,
            width: 2.0,
            radius: 999.0.into(),
        },
        ..container::background(p.accent)
    }
}

/// The chip a constraint's glyph sits on over the viewport, outlined in
/// the danger colour while the constraint is in a conflict.
pub fn glyph(theme: &Theme, conflict: bool) -> container::Style {
    let p = palette(theme);
    let edge = if conflict { p.danger } else { p.line };
    container::Style {
        border: outline(edge, GLYPH_RADIUS),
        ..filled(alpha(p.panel, 0.9), p.text)
    }
}

/// Corner radius of a constraint's [`glyph`].
const GLYPH_RADIUS: f32 = 4.0;

/// A list row under the cursor, like a body in the side panel.
pub fn hovered_row(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: border::rounded(CONTROL_RADIUS),
        ..filled(p.hl, p.text)
    }
}

/// A selected list row, like a feature in the Timeline.
pub fn selected_row(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: border::rounded(CONTROL_RADIUS),
        ..filled(p.accent_soft, p.text)
    }
}

/// A list row that can be selected: a [`selected_row`] if `selected`,
/// else a [`hovered_row`] while `hovered`, else bare.
pub fn list_row(selected: bool, hovered: bool) -> fn(&Theme) -> container::Style {
    if selected {
        selected_row
    } else if hovered {
        hovered_row
    } else {
        |_| container::Style::default()
    }
}

/// The accent colour, for what's asked of the user, like picking a plane.
pub fn accent_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(palette(theme).accent),
    }
}

/// The toolbar's context while editing a sketch: its name on the soft
/// accent, ended by a [`pill_end_button`].
pub fn pill(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: border::rounded(CONTROL_RADIUS),
        ..filled(p.accent_soft, p.text)
    }
}

/// A tag after the toolbar's context, saying what's going on in it:
/// accent text on the soft accent.
pub fn tag(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: border::rounded(5),
        ..filled(p.accent_soft, p.accent)
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

/// The strong danger colour, for what failed: a failed feature's name.
pub fn failed_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(palette(theme).danger_strong),
    }
}

/// The warning colour, for what may go wrong but isn't wrong yet.
pub fn warning_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(palette(theme).warning),
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

/// The tag naming one of the measure tool's two picks, "A" or "B" (the
/// `second`): in the colour its highlight is drawn in, the scene's
/// selection or second colour, with white text where it reads (3:1, as
/// for bold text: the light palette's accent), else dark (the dark
/// palette's accent and both construction colours are too light for
/// white).
pub fn pick_tag(second: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let scene = palette(theme).scene;
        let [r, g, b] = if second { scene.second } else { scene.selected }.0;
        let fill = Color::from_rgb(r, g, b);
        container::Style {
            border: border::rounded(4),
            ..filled(fill, tag_text(fill))
        }
    }
}

/// White text on `fill` if it has a contrast of 3:1 or more there, else
/// dark.
fn tag_text(fill: Color) -> Color {
    let white = (1.05) / (relative_luminance(fill) + 0.05);
    if white >= 3.0 {
        Color::WHITE
    } else {
        LIGHT_TEXT
    }
}

/// The WCAG relative luminance of an opaque sRGB colour.
fn relative_luminance(color: Color) -> f32 {
    let linear = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
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
            assert_eq!(on.background, Background::Color(p.accent));
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
