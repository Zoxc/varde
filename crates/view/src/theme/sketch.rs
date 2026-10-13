//! The sketch overlays' styles: driven dimensions, glyphs and pick tags.

use super::*;

/// The pill a driven size sits in, after an equals sign: washed in
/// [`SketchColors::driven`], the text in the text colour. Over the
/// viewport (a reference dimension's label) on the panel's colour, so it
/// reads over the model too.
pub fn driven_pill(theme: &Theme, over_viewport: bool) -> container::Style {
    let p = palette(theme);
    let background = if over_viewport {
        alpha(mix(p.sketching.driven, p.panel, 0.15), 0.9)
    } else {
        alpha(p.sketching.driven, 0.15)
    };
    container::Style {
        border: border::rounded(DRIVEN_PILL_RADIUS),
        ..filled(background, p.text)
    }
}

/// The equals sign before a driven size, in [`SketchColors::driven`].
pub fn driven_mark(theme: &Theme) -> text::Style {
    text_in(|p| p.sketching.driven)(theme)
}

/// Corner radius of a [`driven_pill`]: round at its ends.
pub(super) const DRIVEN_PILL_RADIUS: f32 = 8.0;

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

/// The chip of a selected constraint's glyph, or a dimension's label,
/// outlined in the selection colour.
pub fn selected_glyph(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: outline(p.sketching.selected, GLYPH_RADIUS),
        ..glyph(theme, false)
    }
}

/// Corner radius of a constraint's [`glyph`].
pub(super) const GLYPH_RADIUS: f32 = 4.0;

/// The tag naming one of the measure tool's two picks, "A" or "B" (the
/// `second`): in the colour its highlight is drawn in, the scene's
/// selection or second colour, with white text where it reads (3:1, as
/// for bold text: the light palette's accent), else dark (the dark
/// palette's accent and both construction colours are too light for
/// white).
pub fn pick_tag(second: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let scene = palette(theme).scene;
        let fill = opaque(if second { scene.second } else { scene.selected });
        container::Style {
            border: border::rounded(4),
            ..filled(fill, tag_text(fill))
        }
    }
}

/// White text on `fill` if it has a contrast of 3:1 or more there, else
/// dark.
pub(super) fn tag_text(fill: Color) -> Color {
    let white = (1.05) / (relative_luminance(fill) + 0.05);
    if white >= 3.0 {
        Color::WHITE
    } else {
        LIGHT_TEXT
    }
}
