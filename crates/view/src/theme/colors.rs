//! The palettes: the modes, the colours each holds and the colour maths
//! they're built with.

use super::*;

/// Whether the UI is light or dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Light,
    Dark,
}

impl Mode {
    pub const fn palette(self) -> &'static Palette {
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
    pub const ALL: [ThemeChoice; 3] = [ThemeChoice::Auto, ThemeChoice::Light, ThemeChoice::Dark];

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
    /// A fill under white text, a [`primary_button`] or a ticked [`tick`]:
    /// the accent, darker in the dark palette, where the accent is too
    /// bright for it (the mock's `--fill`).
    pub fill: Color,
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
    /// Behind a design's thumbnail on its card, which is drawn in the
    /// theme's own colours.
    pub thumbnail: Color,
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
    /// Driving dimensions, their lines, arrows and labels: the Dimension
    /// tools' icon colour.
    pub dimension: Color,
    /// What marks a driven size, one the constraints set though no
    /// driving dimension gives it: the wash of its pill and the equals
    /// sign in it, in the Geometry list and on a reference dimension's
    /// label.
    pub driven: Color,
    /// What's selected.
    pub selected: Color,
    /// What's under the cursor.
    pub hovered: Color,
    /// Inside the points' rims: the mock's halo.
    pub point_fill: Color,
    /// The rims of points the constraints leave free: the accent the
    /// drawing tools' icons draw their points in.
    pub point: Color,
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
    /// Profiles picked for a feature (an extrude's, a revolve's regions, a
    /// loft's sections), filled and outlined: magenta, apart from
    /// [`selected`](Self::selected)'s blue on picked curves.
    pub profile: Color,
    /// The rings marking open ends that almost meet: the failed text's
    /// colour ([`Palette::danger_strong`]), as a failing feature's name in
    /// the Timeline.
    pub near_miss: Color,
    /// A spline's handles: their lines and the rims of their tips, set
    /// apart from the curves and points they shape.
    pub spline_handle: Color,
    /// An extrude's handle: its shaft
    /// and pucks in the Create tools' icon colour, the arrows out of them
    /// in its accent, as the Extrude icon's slab and arrow; both lighter
    /// while hovered or dragged.
    pub handle: Color,
    pub handle_accent: Color,
    pub handle_hovered: Color,
    pub handle_accent_hovered: Color,
    /// The handles of the Modify tools (a chamfer's, a shell's, ...): in
    /// their icon colour and accent as the extrude's are in Create's.
    pub modify_handle: Color,
    pub modify_handle_accent: Color,
    pub modify_handle_hovered: Color,
    pub modify_handle_accent_hovered: Color,
    /// A pattern's count's knob, beside its spacing's or span's in the
    /// handles' colour: teal, to read apart.
    pub count_handle: Color,
    pub count_handle_hovered: Color,
    /// The rail along a handle's axis while it's hovered or dragged: ink,
    /// the text mixed 80% into the panel.
    pub rail: Color,
    /// What a link made (projected or intersected geometry): purple, apart
    /// from the sketch's own curves and construction.
    pub link: Color,
}

// Scene colours the same in both palettes.
pub(super) const AXES: [Srgb; 3] = [
    Srgb([0.76, 0.28, 0.25]),
    Srgb([0.35, 0.63, 0.31]),
    Srgb([0.25, 0.45, 0.77]),
];
pub(super) const GRID: Srgb = Srgb([0.45, 0.49, 0.54]);
pub(super) const ORIGIN_OUTLINE: Srgb = Srgb([0.2, 0.22, 0.25]);
/// The model behind a sketch being edited: the mock's ghosted model.
pub(super) const FADED_ALPHA: f32 = 0.3;
/// The edges the model hides, dashed over it.
pub(super) const HIDDEN_EDGE_ALPHA: f32 = 0.45;
/// Error geometry, solid: the app's red, as a sketch's conflict and the
/// danger button are.
pub(super) const ERROR: Color = color!(0xe0564b);
/// The halo around error geometry: the same red, translucent.
pub(super) const ERROR_HALO: Srgba = Srgba([ERROR.r, ERROR.g, ERROR.b, 0.3]);

/// `color` for the renderer, which takes no alpha.
pub(super) const fn srgb(color: Color) -> Srgb {
    Srgb([color.r, color.g, color.b])
}

/// `color` from the renderer, opaque, as iced's.
pub(crate) const fn opaque(Srgb([r, g, b]): Srgb) -> Color {
    Color { r, g, b, a: 1.0 }
}

/// The model's colour as the viewport draws it, tinted by `tint` if it
/// has one.
pub(super) fn model_color(p: &Palette, tint: Option<BodyTint>) -> Color {
    let model = p.scene.model;
    opaque(tint.map_or(model, |tint| model.tinted(tint)))
}

/// `color` with the opacity `a`.
pub(super) const fn alpha(color: Color, a: f32) -> Color {
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
pub(crate) const fn mix(a: Color, b: Color, share: f32) -> Color {
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
pub(super) const fn icon_tone(
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
pub(super) const LIGHT_ORANGE: Color = color!(0xe88a00);
pub(super) const LIGHT_BLUE: Color = color!(0x2f6fd8);
pub(super) const LIGHT_TEAL: Color = color!(0x0d9a88);
pub(super) const DARK_ORANGE: Color = color!(0xffad33);
pub(super) const DARK_BLUE: Color = color!(0x78aaff);
pub(super) const DARK_TEAL: Color = color!(0x3fd3bf);

pub(super) const LIGHT_TEXT: Color = color!(0x2b3036);
// Also the web page's background until the first frame, crates/web/index.html.
pub(super) const LIGHT_PANEL: Color = color!(0xfafafb);
pub(super) const DARK_TEXT: Color = color!(0xe4e7ea);
pub(super) const DARK_PANEL: Color = color!(0x212228);
pub(super) const LIGHT_ACCENT: Color = color!(0x0a95ad);
pub(super) const DARK_ACCENT: Color = color!(0x39b9cf);
/// The light palette's green: what went well, and what's hovered.
pub(super) const LIGHT_OK: Color = color!(0x3d9b35);
/// The dark palette's highlight green, also what's hovered.
pub(super) const DARK_HIGHLIGHT: Color = color!(0x76cc60);
/// The strong red: a failed step's text, and a near miss.
pub(super) const LIGHT_DANGER_STRONG: Color = color!(0xad2a19);
pub(super) const DARK_DANGER_STRONG: Color = color!(0xf58a7a);

pub(super) const LIGHT_ICONS: IconColors = {
    const fn tone(color: Color, darken: f32, accent: Color) -> IconTone {
        icon_tone(color, darken, accent, LIGHT_TEXT, LIGHT_PANEL)
    }
    IconColors {
        sketch: tone(LIGHT_ACCENT, 0.0, LIGHT_ORANGE),
        modify: tone(color!(0xcf3f30), 0.1, LIGHT_BLUE),
        constraint: tone(color!(0x8a3fd0), 0.12, LIGHT_ORANGE),
        dimension: tone(color!(0x5f6b80), 0.15, LIGHT_ORANGE),
        solid: tone(color!(0xc0409a), 0.0, LIGHT_TEAL),
        construction: tone(color!(0xc39000), 0.08, LIGHT_BLUE),
        inspect: tone(color!(0x2f9a47), 0.0, LIGHT_ORANGE),
        file: tone(LIGHT_BLUE, 0.0, LIGHT_TEXT),
    }
};

pub(super) const DARK_ICONS: IconColors = {
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

/// How much of a handle's colours is kept, mixed into white, while it's
/// hovered or dragged.
pub(super) const HANDLE_HOVERED: f32 = 0.72;

/// A handle's colour `color` as it shows while hovered or dragged.
pub(crate) const fn hovered(color: Color) -> Color {
    mix(color, Color::WHITE, HANDLE_HOVERED)
}
/// A pattern's count's knob's teal, beside the handles' magenta.
pub(super) const LIGHT_COUNT: Color = color!(0x0b8a91);
pub(super) const DARK_COUNT: Color = color!(0x3fc9d3);

// The mock's colours for sketches and for construction, used by the
// renderer and the sketch being edited alike.
pub(super) const LIGHT_SKETCH: Color = LIGHT_ACCENT;
pub(super) const LIGHT_CONSTRUCTION: Color = color!(0xe0861a);
pub(super) const DARK_SKETCH: Color = DARK_ACCENT;
pub(super) const DARK_CONSTRUCTION: Color = color!(0xf0a24a);

pub(super) const LIGHT: Palette = Palette {
    accent: LIGHT_ACCENT,
    accent_soft: alpha(LIGHT_ACCENT, 0.13),
    fill: LIGHT_ACCENT,
    hl: color!(0xe3f5da),
    hl_line: color!(0x9dd488),
    text: LIGHT_TEXT,
    muted: color!(0x6c747d),
    faint: color!(0x9aa2ab),
    line: color!(0x141e28, 0.11),
    edge: color!(0x1f2126),
    thumbnail: color!(0xeef0f3),
    chip: color!(0x141e28, 0.07),
    ok: LIGHT_OK,
    danger: ERROR,
    danger_fill: ERROR,
    danger_strong: LIGHT_DANGER_STRONG,
    warning: LIGHT_CONSTRUCTION,
    scrim: color!(0x000000, 0.25),

    title_bg: color!(0xe9ebef),
    panel: LIGHT_PANEL,
    tabstrip: color!(0xeef0f3),
    scene: Colors {
        background_top: srgb(color!(0xf7f7f9)),
        background_bottom: srgb(color!(0xdfe1e6)),
        // hsl(258 16% 84%), the mock's.
        model: srgb(color!(0xd4d0dd)),
        contrast: 1.2,
        edge: Srgb([0.12, 0.13, 0.15]),
        grid: GRID,
        axes: AXES,
        origin_outline: ORIGIN_OUTLINE,
        // The accent.
        pivot: srgb(LIGHT_ACCENT),
        sketch: srgb(LIGHT_SKETCH),
        faded_alpha: FADED_ALPHA,
        hidden_edge_alpha: HIDDEN_EDGE_ALPHA,
        // Neutral, lighter than the model.
        hover_face: srgb(color!(0xf4f4f4)),
        // White: brighter than the lit faces, and around the dark edge
        // it shows on the light background too.
        hover_outline: srgb(color!(0xffffff)),
        // The accent.
        selected: srgb(LIGHT_ACCENT),
        // A light tint, so the selected edges, the accent a little
        // darker, show on it and apart from the near black edges.
        selected_tint: 0.3,
        selected_edge_shade: -0.3,
        pattern: varde_render::PatternStyle::DEFAULT,
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
        dimension: LIGHT_ICONS.dimension.line,
        driven: color!(0x3a9a5b),
        selected: color!(0x2f5fd8),
        hovered: LIGHT_OK,
        point_fill: color!(0xf4f4f7),
        point: LIGHT_ORANGE,
        preview: alpha(LIGHT_SKETCH, 0.75),
        box_fill: alpha(LIGHT_SKETCH, 0.08),
        box_line: alpha(LIGHT_SKETCH, 0.7),
        axis: color!(0x8a939e),
        guide: color!(0xc2701d),
        region: alpha(LIGHT_SKETCH, 0.1),
        region_hovered: alpha(LIGHT_OK, 0.18),
        profile: color!(0xc0368c),
        near_miss: LIGHT_DANGER_STRONG,
        spline_handle: color!(0x8a6d3b),
        handle: LIGHT_ICONS.solid.line,
        handle_accent: LIGHT_ICONS.solid.accent,
        handle_hovered: hovered(LIGHT_ICONS.solid.line),
        handle_accent_hovered: hovered(LIGHT_ICONS.solid.accent),
        modify_handle: LIGHT_ICONS.modify.line,
        modify_handle_accent: LIGHT_ICONS.modify.accent,
        modify_handle_hovered: hovered(LIGHT_ICONS.modify.line),
        modify_handle_accent_hovered: hovered(LIGHT_ICONS.modify.accent),
        count_handle: LIGHT_COUNT,
        count_handle_hovered: hovered(LIGHT_COUNT),
        rail: mix(LIGHT_TEXT, LIGHT_PANEL, 0.8),
        link: color!(0x8e44c9),
    },
};

pub(super) const DARK: Palette = Palette {
    accent: DARK_ACCENT,
    accent_soft: alpha(DARK_ACCENT, 0.16),
    fill: color!(0x116d7e),
    hl: alpha(DARK_HIGHLIGHT, 0.15),
    hl_line: alpha(DARK_HIGHLIGHT, 0.5),
    text: DARK_TEXT,
    muted: color!(0x9aa3ac),
    faint: color!(0x69727c),
    line: color!(0xffffff, 0.08),
    edge: color!(0xb5b2bd),
    thumbnail: color!(0x45474f),
    chip: color!(0xffffff, 0.08),
    ok: color!(0x5cc052),
    // Lighter than the light palette's, to read on the dark panel.
    danger: color!(0xf07563),
    danger_fill: ERROR,
    danger_strong: DARK_DANGER_STRONG,
    warning: DARK_CONSTRUCTION,
    scrim: color!(0x000000, 0.45),

    title_bg: color!(0x17181d),
    panel: DARK_PANEL,
    tabstrip: color!(0x1b1c21),
    scene: Colors {
        background_top: srgb(color!(0x2b2c32)),
        background_bottom: srgb(color!(0x1a1b1f)),
        // hsl(258 12% 44%), the mock's.
        model: srgb(color!(0x6b637e)),
        // More than the light theme's, for the darker faces.
        contrast: 1.6,
        // Light, to show on the dark background and apart from the faces.
        edge: srgb(color!(0xb5b2bd)),
        grid: GRID,
        axes: AXES,
        origin_outline: ORIGIN_OUTLINE,
        // The accent.
        pivot: srgb(DARK_ACCENT),
        sketch: srgb(DARK_SKETCH),
        faded_alpha: FADED_ALPHA,
        hidden_edge_alpha: HIDDEN_EDGE_ALPHA,
        // The model 8% of the way to white, in linear light: a faint lift.
        hover_face: srgb(color!(0x807a8e)),
        // Near black: around the light edges, as white is around the
        // light palette's dark ones.
        hover_outline: srgb(color!(0x0e0d12)),
        // The accent.
        selected: srgb(DARK_ACCENT),
        selected_tint: 0.6,
        // A deep teal, darker than a selected face's tint to show on it,
        // and apart from the light edges.
        selected_edge_shade: -0.6,
        pattern: varde_render::PatternStyle::DEFAULT,
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
        dimension: DARK_ICONS.dimension.line,
        driven: color!(0x4fb06e),
        selected: color!(0x7ea2ff),
        hovered: DARK_HIGHLIGHT,
        point_fill: color!(0x24252b),
        point: DARK_ORANGE,
        preview: alpha(DARK_SKETCH, 0.75),
        box_fill: alpha(DARK_SKETCH, 0.1),
        box_line: alpha(DARK_SKETCH, 0.7),
        axis: color!(0x6b737d),
        guide: DARK_CONSTRUCTION,
        region: alpha(DARK_SKETCH, 0.12),
        region_hovered: alpha(DARK_HIGHLIGHT, 0.2),
        profile: color!(0xef7ac4),
        near_miss: DARK_DANGER_STRONG,
        spline_handle: color!(0xc9a66b),
        handle: DARK_ICONS.solid.line,
        handle_accent: DARK_ICONS.solid.accent,
        handle_hovered: hovered(DARK_ICONS.solid.line),
        handle_accent_hovered: hovered(DARK_ICONS.solid.accent),
        modify_handle: DARK_ICONS.modify.line,
        modify_handle_accent: DARK_ICONS.modify.accent,
        modify_handle_hovered: hovered(DARK_ICONS.modify.line),
        modify_handle_accent_hovered: hovered(DARK_ICONS.modify.accent),
        count_handle: DARK_COUNT,
        count_handle_hovered: hovered(DARK_COUNT),
        rail: mix(DARK_TEXT, DARK_PANEL, 0.8),
        link: color!(0xc08cf2),
    },
};

/// Multiplies each channel, like CSS `filter: brightness()`.
pub(super) fn brighten(color: Color, factor: f32) -> Color {
    Color {
        r: (color.r * factor).min(1.0),
        g: (color.g * factor).min(1.0),
        b: (color.b * factor).min(1.0),
        ..color
    }
}

/// The WCAG relative luminance of an opaque sRGB colour.
pub(super) fn relative_luminance(color: Color) -> f32 {
    let linear = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
}
