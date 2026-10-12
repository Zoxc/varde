//! Colours: sRGB, body tints, the pattern style and the frame's palette.

use super::*;

/// An opaque sRGB-encoded colour, like CSS and UI colours. The renderer
/// converts it for its target.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Srgb(pub [f32; 3]);

/// A colour of a body's own: a hue and a saturation, its lightness the
/// colour it's applied to's ([`Srgb::tinted`]).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BodyTint {
    /// In degrees round the colour wheel.
    pub hue: f32,
    /// From 0, grey, to 1.
    pub saturation: f32,
}

impl Srgb {
    /// This colour with `tint`'s hue and saturation, keeping its HSL
    /// lightness: how a body's own colour is drawn on the theme's model
    /// colour, lit as it is. The saturation sets the chroma
    /// (`TINT_CHROMA` at 1) rather than HSL's share of what the
    /// lightness allows, so a colour is as vivid on a pale model as on a
    /// dark one, up to what the lightness allows. A hue not finite is 0, a
    /// saturation out of range clamped to it (NaN is grey).
    pub fn tinted(self, tint: BodyTint) -> Srgb {
        let [r, g, b] = self
            .0
            .map(|c| if c.is_nan() { 0.0 } else { c.clamp(0.0, 1.0) });
        let lightness = (r.max(g).max(b) + r.min(g).min(b)) / 2.0;
        let hue = if tint.hue.is_finite() {
            tint.hue.rem_euclid(360.0)
        } else {
            0.0
        };
        let saturation = if tint.saturation.is_nan() {
            0.0
        } else {
            tint.saturation.clamp(0.0, 1.0)
        };
        let chroma = (TINT_CHROMA * saturation).min(1.0 - (2.0 * lightness - 1.0).abs());
        // Each channel's distance round the wheel from its peak, in sixths.
        let channel = |n: f32| {
            let k = (n + hue / 30.0).rem_euclid(12.0);
            lightness - chroma / 2.0 * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
        };
        Srgb([channel(0.0), channel(8.0), channel(4.0)].map(|c| c.clamp(0.0, 1.0)))
    }
}

/// The chroma of a [`BodyTint`] of saturation 1, where the lightness
/// allows it: tints are drawn at this share of it, whatever the model
/// colour's lightness ([`Srgb::tinted`]).
pub const TINT_CHROMA: f32 = 0.6;

/// How what the model hides of the hovered and selected faces is drawn: a
/// wash with diagonal stripes, within an edge where it ends, in the stripes'
/// colour. Out of range values are clamped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PatternStyle {
    /// How wide the edge is, in logical pixels, from 0 to 16.
    pub edge_width: f32,
    /// How opaque the wash is, from 0 to 1.
    pub wash: f32,
    /// How opaque the stripes and the edge are, from 0 to 1.
    pub stripes: f32,
}

impl PatternStyle {
    pub const DEFAULT: PatternStyle = PatternStyle {
        edge_width: 3.0,
        wash: 0.2,
        stripes: 0.5,
    };
}

/// Scene colours, from the UI theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    /// Background gradient, top to bottom.
    pub background_top: Srgb,
    pub background_bottom: Srgb,
    /// Base colour of model faces, before lighting.
    pub model: Srgb,
    /// How far the light on the faces is spread from its middle, as a
    /// share of the regular shading's own (1): higher has faces turned to
    /// the light brighter and the rest darker, 0 lights them all alike.
    /// Regular and flat [`Shading`] only, not metal. Out of range (0 to 4)
    /// or NaN is 1.
    pub contrast: f32,
    /// Feature edges, drawn over the faces [`EDGE_WIDTH`] wide.
    pub edge: Srgb,
    /// Grid lines.
    pub grid: Srgb,
    /// The X, Y and Z axes, for the grid's axis lines.
    pub axes: [Srgb; 3],
    /// The rims of the origin marker's ring and dot, and the pivot's.
    pub origin_outline: Srgb,
    /// The core of the pivot's ring and dot.
    pub pivot: Srgb,
    /// Finished sketches' curves.
    pub sketch: Srgb,
    /// How opaque the model's faces and edges are when faded ([`Frame::fade`] at 1),
    /// from 0 to 1. Only its nearest faces are drawn, and they still hide
    /// what's behind them from the grid and the sketches.
    pub faded_alpha: f32,
    /// How opaque the edges the model hides are drawn, from 0 to 1, of
    /// [`Self::edge`]: see [`Frame::hidden_edges`].
    pub hidden_edge_alpha: f32,
    /// The hovered faces' colour, before lighting: brighter than
    /// [`Self::model`]. See [`Frame::hovered_faces`].
    pub hover_face: Srgb,
    /// The rim around the hovered edges and vertex, for contrast with
    /// them and what's behind them: see [`HOVER_RIM`]. The selected
    /// edges' and vertices' rim is in it too, translucent: see
    /// [`SELECTED_RIM`].
    pub hover_outline: Srgb,
    /// The accent: selected faces are tinted with it, by
    /// [`Self::selected_tint`], selected edges and vertices drawn in it,
    /// shaded by [`Self::selected_edge_shade`].
    pub selected: Srgb,
    /// How far a selected face is tinted towards [`Self::selected`], from
    /// 0 to 1. Out of range or NaN is 0.6.
    pub selected_tint: f32,
    /// How far the selected edges' and vertices' colour is from
    /// [`Self::selected`], so they stand out on a selected face: from -1,
    /// black, through 0, the accent itself, to 1, white, of the way in
    /// linear light. Out of range or NaN is 0.
    pub selected_edge_shade: f32,
    /// How what the model hides of the hovered and selected faces is
    /// drawn: see [`PatternStyle`].
    pub pattern: PatternStyle,
    /// The second colour, for the second of two picks (the measure
    /// tool's B), where the first is in [`Self::selected`]: faces tinted
    /// with it as the selected are with theirs, and edges drawn in it,
    /// unshaded. See
    /// [`Frame::second_faces`] and [`Highlights::second_edges`].
    pub second: Srgb,
    /// Error geometry ([`Frame::errors`]): its patches (lit), curves and
    /// points.
    pub error: Srgb,
    /// The halo around error geometry, translucent: drawn once over the
    /// frame wherever any of it reaches, so overlaps don't darken. Alpha
    /// out of range or NaN is 0.3.
    pub error_halo: Srgba,
}

impl Colors {
    /// These colours with every out of range knob at the fallback its
    /// docs give, so the shader only sees values it can use.
    pub(super) fn sanitized(&self) -> Colors {
        let [r, g, b, a] = self.error_halo.0;
        Colors {
            contrast: within(self.contrast, 0.0..=4.0, 1.0),
            // Out of range, the model is drawn opaque rather than not at
            // all.
            faded_alpha: within(self.faded_alpha, 0.0..=1.0, 1.0),
            hidden_edge_alpha: within(self.hidden_edge_alpha, 0.0..=1.0, 0.0),
            // Out of range, as faint as can be rather than solid.
            selected_tint: within(self.selected_tint, 0.0..=1.0, 0.6),
            selected_edge_shade: within(self.selected_edge_shade, -1.0..=1.0, 0.0),
            pattern: PatternStyle {
                edge_width: clamped(self.pattern.edge_width, 16.0),
                wash: clamped(self.pattern.wash, 1.0),
                stripes: clamped(self.pattern.stripes, 1.0),
            },
            // Out of range, as faint as the theme's.
            error_halo: Srgba([r, g, b, within(a, 0.0..=1.0, 0.3)]),
            ..*self
        }
    }
}

/// `value` if it's in `range`, else (or NaN) `fallback`.
fn within(value: f32, range: std::ops::RangeInclusive<f32>, fallback: f32) -> f32 {
    if range.contains(&value) {
        value
    } else {
        fallback
    }
}

/// `value` clamped to 0 through `most`, NaN 0.
fn clamped(value: f32, most: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, most)
    }
}

impl Srgb {
    /// This colour in linear, which the shader works in, opaque.
    pub(super) fn linear(self) -> [f32; 4] {
        let [r, g, b] = self.0.map(srgb_to_linear);
        [r, g, b, 1.0]
    }
}

/// An sRGB encoded value, from 0 to 1, made linear.
pub(crate) fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// A linear value, from 0 to 1, sRGB encoded: [`srgb_to_linear`]'s
/// inverse, clamped.
pub(crate) fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}
