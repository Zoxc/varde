//! The settings page's styles.

use super::*;

/// The bar along the left of a settings row whose setting isn't its
/// default.
pub fn changed_bar(theme: &Theme) -> container::Style {
    let p = palette(theme);
    filled(p.accent, p.text)
}

/// The settings page's card of rows, on the panel's colour.
pub fn settings_card(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: outline(p.line, 8.0),
        ..filled(p.panel, p.text)
    }
}

/// A segmented bar on the settings page: its buttons ([`segment`]) on the
/// chips' colour.
pub fn segments(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: border::rounded(7),
        ..filled(p.chip, p.text)
    }
}

/// A button of a [`segments`] bar: muted on the bar, the text colour on
/// hover, and while `on` raised on the panel's colour with a thin line.
pub fn segment(on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        if on {
            button::Style {
                background: Some(Background::Color(p.panel)),
                text_color: p.text,
                border: outline(p.line, 5.0),
                ..button::Style::default()
            }
        } else {
            button::Style {
                text_color: if is_hovered(status) { p.text } else { p.muted },
                border: border::rounded(5),
                ..button::Style::default()
            }
        }
    }
}

/// A switch on the settings page: the fill's colour while on, a grey
/// mixed from the text's off, its knob white.
pub fn switch(theme: &Theme, status: toggler::Status) -> toggler::Style {
    let p = palette(theme);
    let on = match status {
        toggler::Status::Active { is_toggled }
        | toggler::Status::Hovered { is_toggled }
        | toggler::Status::Disabled { is_toggled } => is_toggled,
    };
    let background = if on {
        p.fill
    } else {
        mix(p.text, p.panel, 0.22)
    };
    toggler::Style {
        background: Background::Color(background),
        background_border_width: 0.0,
        background_border_color: Color::TRANSPARENT,
        foreground: Background::Color(Color::WHITE),
        foreground_border_width: 0.0,
        foreground_border_color: Color::TRANSPARENT,
        text_color: None,
        border_radius: None,
        padding_ratio: 0.1,
    }
}

/// A row of Reset to defaults' list, on the chips' colour.
pub fn reset_row(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: border::rounded(CONTROL_RADIUS),
        ..filled(p.chip, p.faint)
    }
}
