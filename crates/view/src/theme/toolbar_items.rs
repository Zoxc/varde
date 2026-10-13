//! The toolbar's file cell and pills.

use super::*;

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

/// The grey pill a design with no name shows in its name's place in the
/// toolbar's file cell.
pub fn name_pill(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: border::rounded(PILL),
        ..filled(p.chip, p.muted)
    }
}

/// The colour the bar under the file cell says where a design is kept
/// in, on the web: the accent for browser storage, green for the
/// `computer`, 70% into the text's colour.
pub fn location_ink(p: &Palette, computer: bool) -> Color {
    mix(if computer { p.ok } else { p.accent }, p.text, 0.7)
}

/// The dot after an edited document's name in the toolbar's file cell.
pub fn dirty_dot(theme: &Theme) -> container::Style {
    container::Style {
        border: border::rounded(3),
        ..container::background(palette(theme).muted)
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
