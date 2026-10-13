//! The tool rail's styles.

use super::*;

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
///
/// While its set is `held` open, by a click or its key, it shows the
/// selection's colour rather than the hover highlight, deeper on hover.
pub fn rail_head(
    open: bool,
    held: bool,
    over_strip: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        let inner = RAIL_CARD_RADIUS - 1.0;
        let radius = if over_strip {
            border::Radius::new(0).top(inner)
        } else {
            border::Radius::new(inner)
        };
        button::Style {
            background: if held {
                Some(Background::Color(selected_tint(p, is_hovered(status))))
            } else {
                (open || is_hovered(status)).then_some(Background::Color(p.hl))
            },
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
/// set is `open` (the selection's colour while `held`), showing in the
/// strip's rounded top corners.
pub fn rail_strip_backing(open: bool, held: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| container::Style {
        background: if held {
            Some(Background::Color(palette(theme).accent_soft))
        } else {
            open.then_some(Background::Color(palette(theme).hl))
        },
        border: border::rounded(border::Radius::new(0).bottom(RAIL_CARD_RADIUS - 1.0)),
        ..container::Style::default()
    }
}

/// A row of a tool rail's list: a [`flat_button`], highlighted as on
/// hover while the keys are on it (`focused`), disabled or not, unless
/// it's `on`.
pub fn rail_row(on: bool, focused: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let style = flat_button(on, Tone::Text)(theme, status);
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
pub(super) const RAIL_LIST_RADIUS: f32 = 10.0;

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
