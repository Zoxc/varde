//! The welcome page's styles: its cards, chips, delete buttons and drop
//! zone.

use super::*;

/// The welcome page behind its content, the colour of the viewport's top.
pub fn welcome(theme: &Theme) -> container::Style {
    let p = palette(theme);
    filled(opaque(p.scene.background_top), p.text)
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

/// A card that isn't clicked, like the welcome page's note of a panic.
pub fn note_card(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: outline(p.line, CARD_RADIUS),
        ..filled(p.panel, p.text)
    }
}

/// The thumbnail at the top of a [`card`], inside its 1 px border.
pub fn card_thumbnail(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: border::rounded(border::top(CARD_RADIUS - 1.0)),
        ..filled(p.thumbnail, p.text)
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

/// The count beside a welcome page heading, like Recent's, or the MCP
/// panel's tag: muted text on a key chip's fill, rounded to a pill.
pub fn count_chip(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: border::rounded(PILL),
        ..filled(p.chip, p.muted)
    }
}

/// A [`count_chip`] that's a button, like the status bar's MCP chip:
/// highlighted, its text strong, on hover.
pub fn chip_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let hovered = is_hovered(status);
    button::Style {
        background: Some(Background::Color(if hovered { p.hl } else { p.chip })),
        text_color: if hovered { p.text } else { p.muted },
        border: border::rounded(PILL),
        ..button::Style::default()
    }
}

/// The corner radius that rounds a chip up to 20 pixels high to a pill.
pub(super) const PILL: f32 = 10.0;

/// The delete button at the foot of a design's card on the welcome page,
/// over the card's own highlight: a wash of the danger colour on hover,
/// so it reads as a button of its own. Its icon is [`delete_icon`].
pub fn delete_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    button::Style {
        background: is_hovered(status).then_some(Background::Color(danger_wash(p))),
        text_color: p.muted,
        border: border::rounded(CONTROL_RADIUS),
        ..button::Style::default()
    }
}

/// The colour of a [`delete_button`]'s icon: the strong danger colour
/// while `hovered`.
pub fn delete_icon(p: &Palette, hovered: bool) -> Color {
    if hovered { p.danger_strong } else { p.muted }
}

/// The welcome page's drop zone on the web: no fill but the hover
/// background, or the soft accent while a file is dragged over the page
/// (`lit`), its text the text colour either way. Its dashed outline is drawn over
/// it, see [`drop_zone_line`].
pub fn drop_zone(lit: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        let background = if lit {
            Some(p.accent_soft)
        } else {
            is_hovered(status).then_some(p.hl)
        };
        button::Style {
            background: background.map(Background::Color),
            text_color: p.text,
            border: border::rounded(CARD_RADIUS),
            ..button::Style::default()
        }
    }
}

/// The colour of a [`drop_zone`]'s dashed outline: the accent while `lit`,
/// the hover border while `hovered`, else the text 18% into the welcome
/// page's background. Opaque, as an SVG's tint takes no alpha.
pub fn drop_zone_line(p: &Palette, lit: bool, hovered: bool) -> Color {
    let background = opaque(p.scene.background_top);
    if lit {
        p.accent
    } else if hovered {
        mix(p.hl_line, background, p.hl_line.a)
    } else {
        mix(p.text, background, 0.18)
    }
}

/// A dot `color` picks from the palette, 7 px across, like the one before
/// a design's downloads on the welcome page.
pub fn dot(color: fn(&Palette) -> Color) -> impl Fn(&Theme) -> container::Style {
    move |theme| container::Style {
        border: border::rounded(3.5),
        ..container::background(color(palette(theme)))
    }
}

/// The dot of a design in browser storage never downloaded: the mock's
/// amber, the same in both palettes.
pub const NEVER_DOWNLOADED: Color = color!(0xe0a01a);

/// A recessed box of text in a dialog, like a panic's report.
pub fn text_well(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: outline(p.line, CONTROL_RADIUS),
        ..filled(well(p), p.text)
    }
}
