//! The parameters popup's styles.

use super::*;

/// A parameter's name in the parameters' popup: bare until hovered (the
/// hover background) or focused (the accent's border on the panel's
/// colour), as the mock's `.pname`.
pub fn param_name_input(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let p = palette(theme);
    let (background, edge) = match status {
        text_input::Status::Focused { .. } => (Some(p.panel), p.accent),
        text_input::Status::Hovered => (Some(p.hl), Color::TRANSPARENT),
        text_input::Status::Active | text_input::Status::Disabled => (None, Color::TRANSPARENT),
    };
    text_input::Style {
        background: Background::Color(background.unwrap_or(Color::TRANSPARENT)),
        border: outline(edge, CONTROL_RADIUS),
        icon: p.muted,
        placeholder: p.faint,
        value: p.text,
        selection: p.accent_soft,
    }
}

/// A row of the parameters' popup: every other one (`banded`) a little
/// darker, the text's 3% into the panel's, as the mock's `.tb-row`.
pub fn param_row(banded: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = palette(theme);
        container::Style {
            background: Some(Background::Color(if banded {
                mix(p.text, p.panel, 0.03)
            } else {
                p.panel
            })),
            ..container::Style::default()
        }
    }
}

/// The header row of the parameters' popup: the text's 6% into the
/// panel's, muted words, as the mock's `.tb-head`.
pub fn param_head(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(well(p))),
        text_color: Some(p.muted),
        ..container::Style::default()
    }
}

/// The mark down a parameter's row's left edge: the accent's for one the
/// feature selected uses, the danger colour's for one in error (`bad`).
pub fn param_mark(bad: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = palette(theme);
        container::Style {
            background: Some(Background::Color(if bad { p.danger } else { p.accent })),
            ..container::Style::default()
        }
    }
}

/// A parameter's delete button, shown over its row while hovered: bare,
/// faint, the danger's words on the hover background when hovered.
pub fn param_delete(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let hovered = is_hovered(status);
    button::Style {
        background: hovered.then_some(Background::Color(p.hl)),
        text_color: if hovered { p.danger } else { p.faint },
        border: border::rounded(5.0),
        ..button::Style::default()
    }
}

/// The parameters' popup's add row: the accent's words on the panel's
/// colour, the soft accent on hover, as the mock's `.padd`.
pub fn param_add(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    button::Style {
        background: Some(Background::Color(if is_hovered(status) {
            p.accent_soft
        } else {
            p.panel
        })),
        text_color: p.accent,
        ..button::Style::default()
    }
}

/// A parameter name offered under a typed value's field: the first,
/// which Tab puts in, lit as hovered ones are.
pub fn suggestion_row(first: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        button::Style {
            background: (first || is_hovered(status)).then_some(Background::Color(p.accent_soft)),
            text_color: p.text,
            border: border::rounded(5.0),
            ..button::Style::default()
        }
    }
}
