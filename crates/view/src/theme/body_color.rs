//! The body colour rail's styles.

use super::*;

/// A [`slider`] laid over a [`color_rail`]: its own rail left out, its
/// handle filled with the model colour tinted as `handle` says (the
/// theme's own if none), so it shows the colour picked.
pub fn color_slider(
    enabled: bool,
    handle: Option<BodyTint>,
) -> impl Fn(&Theme, slide::Status) -> slide::Style {
    let plain = slider(enabled);
    move |theme, status| {
        let handle = model_color(palette(theme), handle);
        let mut style = plain(theme, status);
        style.rail.backgrounds = (
            Background::Color(Color::TRANSPARENT),
            Background::Color(Color::TRANSPARENT),
        );
        style.handle.background = Background::Color(handle);
        style
    }
}

/// The rail under a [`color_slider`]: the model colour tinted by each of
/// `stops` in turn, evenly spread from left to right (at most 8, iced's
/// limit for a gradient), none for the theme's own. Faded unless
/// `enabled`.
///
/// iced draws no gradients on the web, so there the rail is
/// [`COLOR_RAIL_STRIPS`] of [`color_rail_strip`] instead, under this
/// style with its gradient left out: the border alone.
pub fn color_rail(
    stops: Vec<Option<BodyTint>>,
    enabled: bool,
) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = palette(theme);
        let last = stops.len().saturating_sub(1).max(1) as f32;
        let gradient = (stops.iter().enumerate()).fold(
            iced::gradient::Linear::new(iced::Degrees(90.0)),
            |gradient, (i, tint)| gradient.add_stop(i as f32 / last, rail_color(p, *tint, enabled)),
        );
        let background = if cfg!(target_arch = "wasm32") {
            None
        } else {
            Some(Background::Gradient(gradient.into()))
        };
        container::Style {
            background,
            border: border::rounded(COLOR_RAIL_HEIGHT / 2.0)
                .color(p.line)
                .width(1.0),
            ..container::Style::default()
        }
    }
}

/// How many solid strips make a [`color_rail`] where iced draws no
/// gradients.
pub const COLOR_RAIL_STRIPS: usize = 48;

/// Strip `index` of [`COLOR_RAIL_STRIPS`] standing in for a
/// [`color_rail`]'s gradient: the rail's colour at the strip's middle,
/// the end strips rounded as the rail is.
pub fn color_rail_strip(
    stops: Vec<Option<BodyTint>>,
    index: usize,
    enabled: bool,
) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = palette(theme);
        let at = (index as f32 + 0.5) / COLOR_RAIL_STRIPS as f32;
        let span = stops.len().saturating_sub(1);
        let color = match stops.as_slice() {
            [] => rail_color(p, None, enabled),
            [only] => rail_color(p, *only, enabled),
            _ => {
                let scaled = at * span as f32;
                let i = (scaled.floor() as usize).min(span - 1);
                let (a, b) = (
                    rail_color(p, stops[i], enabled),
                    rail_color(p, stops[i + 1], enabled),
                );
                let t = scaled - i as f32;
                let mix = |a: f32, b: f32| a + (b - a) * t;
                Color {
                    r: mix(a.r, b.r),
                    g: mix(a.g, b.g),
                    b: mix(a.b, b.b),
                    a: mix(a.a, b.a),
                }
            }
        };
        let radius = COLOR_RAIL_HEIGHT / 2.0;
        let first = if index == 0 { radius } else { 0.0 };
        let last = if index + 1 == COLOR_RAIL_STRIPS {
            radius
        } else {
            0.0
        };
        container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                radius: border::Radius {
                    top_left: first,
                    bottom_left: first,
                    top_right: last,
                    bottom_right: last,
                },
                ..Border::default()
            },
            ..container::Style::default()
        }
    }
}

/// The model colour tinted by `tint` (none for the theme's own), faded
/// unless `enabled`: one stop of a [`color_rail`].
pub(super) fn rail_color(p: &Palette, tint: Option<BodyTint>, enabled: bool) -> Color {
    fade_unless(enabled, model_color(p, tint))
}

/// The height of a [`color_rail`].
pub const COLOR_RAIL_HEIGHT: f32 = 8.0;
