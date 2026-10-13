//! The operation panel's styles: its head, tiles, toggles, picks, fields
//! and failures.

use super::*;

/// A square icon button in the operation panel's head, Cancel or (the
/// `primary`) OK: a thin border, the hover background on hover; OK in the
/// fill colour, faded while disabled.
pub fn head_button(primary: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        if primary {
            return button::Style {
                border: border::rounded(CONTROL_RADIUS),
                ..filled_button(p, p.fill, status)
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
                background: Some(Background::Color(on_fill(p, false))),
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
            (true, false) => (on_fill(p, false), p.accent),
            (true, true) => (on_fill(p, true), p.accent),
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
        ..filled(danger_wash(p), p.text)
    }
}

/// A button in a [`fail_box`], Add anyway and Show (or Go back): a thin
/// border on the panel's colour, its words in the strong danger colour
/// if `danger`, else the text's.
pub fn fail_button(danger: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        let style = button::Style {
            background: Some(Background::Color(if is_hovered(status) {
                p.hl
            } else {
                p.panel
            })),
            text_color: if danger { p.danger_strong } else { p.text },
            border: outline(p.line, CONTROL_RADIUS),
            ..button::Style::default()
        };
        fade_if_disabled(status, style)
    }
}

/// The swatch of a body's colour in its context menu: the model's colour
/// as the viewport has it, tinted by `tint` if it has one, framed by a
/// line.
pub fn body_swatch(tint: Option<BodyTint>) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = palette(theme);
        container::Style {
            background: Some(Background::Color(model_color(p, tint))),
            border: border::rounded(3.0).color(p.line).width(1.0),
            ..container::Style::default()
        }
    }
}
