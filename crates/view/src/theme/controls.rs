//! Styles shared across the app: bars, tabs, buttons, ticks, sliders,
//! scrollbars, menus, list rows and text.

use super::*;

/// The document toolbar.
pub fn toolbar(theme: &Theme) -> container::Style {
    let p = palette(theme);
    filled(p.panel, p.text)
}

/// The docked panel left of the viewport.
pub fn side_panel(theme: &Theme) -> container::Style {
    toolbar(theme)
}

/// A strip under the toolbar telling something about the whole document,
/// like it being read-only.
pub fn banner(theme: &Theme) -> container::Style {
    let p = palette(theme);
    filled(p.accent_soft, p.text)
}

/// The strip holding the side panel's [`tab`]s.
pub fn tab_strip(theme: &Theme) -> container::Style {
    let p = palette(theme);
    filled(p.tabstrip, p.muted)
}

/// How a side panel [`tab`] shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabLook {
    /// On the strip, highlighted on hover.
    Flat,
    /// The selected tab, joining the panel below.
    Raised,
    /// The tab shown while the peek key is held: raised, with accent text.
    Peek,
}

impl TabLook {
    /// The colour of the tab's label at rest, and of its icon.
    pub fn content(self, p: &Palette) -> Color {
        match self {
            TabLook::Flat => p.muted,
            TabLook::Raised => p.text,
            TabLook::Peek => p.accent,
        }
    }
}

/// A folder tab in the [`tab_strip`], [`TAB_HEIGHT`] tall, around a
/// [`tab_face`] with [`TAB_LINE`] of padding at the top.
///
/// A raised tab has the panel colour, so it joins the panel below, and a
/// 2px accent line along its top: the button is the accent, and its face
/// the panel colour below the line. A gradient would do it in one quad,
/// but iced draws none on the web.
pub fn tab(look: TabLook) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        let background = match look {
            TabLook::Raised | TabLook::Peek => Some(p.accent),
            TabLook::Flat if is_hovered(status) => Some(p.hl),
            TabLook::Flat => None,
        };
        let text_color = match look {
            TabLook::Flat if is_hovered(status) => p.text,
            _ => look.content(p),
        };
        button::Style {
            background: background.map(Background::Color),
            text_color,
            border: border::rounded(border::top(TAB_RADIUS)),
            ..button::Style::default()
        }
    }
}

/// How far a [`tab`]'s face is below its top: the raised tab's accent line.
pub const TAB_LINE: f32 = 2.0;

/// The face of a [`tab`] below its accent line: the panel colour when
/// raised. Its top corners are rounded less than the tab's, so they stay
/// inside the tab's: they meet its sides where its corners end.
pub fn tab_face(look: TabLook) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = palette(theme);
        container::Style {
            background: (look != TabLook::Flat).then_some(Background::Color(p.panel)),
            border: border::rounded(border::top(TAB_RADIUS - TAB_LINE)),
            ..container::Style::default()
        }
    }
}

/// The colour of an enabled [`flat_button`]'s content at rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Text,
    Muted,
    Faint,
}

/// The colour of a [`flat_button`]'s content, for an icon in it, which
/// doesn't take the button's text colour: `tone` at rest, the text colour
/// while `hovered`, and faint unless `enabled`.
pub fn flat_content(p: &Palette, tone: Tone, enabled: bool, hovered: bool) -> Color {
    match (enabled, hovered, tone) {
        (false, _, _) | (true, false, Tone::Faint) => p.faint,
        (true, false, Tone::Muted) => p.muted,
        (true, true, _) | (true, false, Tone::Text) => p.text,
    }
}

/// A borderless button that only shows a background on hover, or when `on`.
/// Its text is `tone`, as [`flat_content`] has it, or the accent colour
/// when `on`: [`Tone::Muted`] for a quiet one, like the welcome screen's
/// help at its foot.
pub fn flat_button(on: bool, tone: Tone) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        let enabled = status != button::Status::Disabled;
        let hovered = is_hovered(status);
        let base = button::Style {
            background: None,
            text_color: flat_content(p, tone, enabled, hovered),
            border: border::rounded(CONTROL_RADIUS),
            ..button::Style::default()
        };

        if !enabled {
            base
        } else if on {
            button::Style {
                background: Some(Background::Color(p.accent_soft)),
                text_color: p.accent,
                ..base
            }
        } else if hovered {
            button::Style {
                background: Some(Background::Color(p.hl)),
                ..base
            }
        } else {
            base
        }
    }
}

/// Which of the two filled buttons a button is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    /// A [`primary_button`].
    Primary,
    /// A [`secondary_button`].
    Secondary,
}

impl Emphasis {
    pub fn button_style(self) -> fn(&Theme, button::Status) -> button::Style {
        match self {
            Emphasis::Primary => primary_button,
            Emphasis::Secondary => secondary_button,
        }
    }

    /// The style of a key chip on the button.
    pub fn key_chip(self) -> fn(&Theme) -> container::Style {
        match self {
            Emphasis::Primary => accent_key_chip,
            Emphasis::Secondary => key_chip,
        }
    }

    /// The colour of the button's content, for an icon in it, which doesn't
    /// take the button's text colour.
    pub fn content(self, p: &Palette) -> Color {
        match self {
            Emphasis::Primary => Color::WHITE,
            Emphasis::Secondary => p.text,
        }
    }
}

/// The main call to action: the fill colour with white text.
pub fn primary_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    filled_button(p, p.fill, status)
}

/// A [`primary_button`] closing the right end of a [`pill`]: square on
/// its left, where it joins the pill.
pub fn pill_end_button(theme: &Theme, status: button::Status) -> button::Style {
    let style = primary_button(theme, status);
    button::Style {
        border: Border {
            radius: border::Radius::new(0)
                .top_right(CONTROL_RADIUS)
                .bottom_right(CONTROL_RADIUS),
            ..style.border
        },
        ..style
    }
}

/// The main call to action where it destroys something: danger fill with
/// white text.
pub fn danger_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    filled_button(p, p.danger_fill, status)
}

/// A button beside a [`primary_button`]: panel fill with a thin border.
pub fn secondary_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let hovered = is_hovered(status);
    let style = button::Style {
        background: Some(Background::Color(if hovered { p.hl } else { p.panel })),
        text_color: Emphasis::Secondary.content(p),
        border: outline(if hovered { p.hl_line } else { p.line }, BUTTON_RADIUS),
        ..button::Style::default()
    };
    fade_if_disabled(status, style)
}

/// Corner radius of a [`tick`]'s box.
pub(super) const TICK_RADIUS: f32 = 4.0;

/// A checkbox: a faint box whose border turns accent on hover, in the
/// fill colour with a white check while ticked; its label in the
/// text colour, whatever the colour around it.
pub fn tick(theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    let p = palette(theme);
    let (checked, hovered, enabled) = match status {
        checkbox::Status::Active { is_checked } => (is_checked, false, true),
        checkbox::Status::Hovered { is_checked } => (is_checked, true, true),
        checkbox::Status::Disabled { is_checked } => (is_checked, false, false),
    };
    let edge = if checked {
        p.fill
    } else if hovered {
        p.accent
    } else {
        p.faint
    };
    let fade = |color: Color| fade_unless(enabled, color);
    checkbox::Style {
        background: Background::Color(if checked {
            fade(p.fill)
        } else {
            Color::TRANSPARENT
        }),
        icon_color: Color::WHITE,
        border: outline(fade(edge), TICK_RADIUS),
        text_color: Some(fade(p.text)),
    }
}

/// The radius of a [`slider`]'s handle.
pub const SLIDER_HANDLE_RADIUS: f32 = 6.0;

/// A slider: a thin rail, accent up to the handle and the line colour
/// after it, and a round handle in the panel's colour ringed in the
/// accent, filled with the hover colour while hovered or dragged; faded
/// as a disabled button is unless `enabled`.
pub fn slider(enabled: bool) -> impl Fn(&Theme, slide::Status) -> slide::Style {
    move |theme, status| {
        let p = palette(theme);
        let fade = |color: Color| fade_unless(enabled, color);
        let fill = match status {
            slide::Status::Hovered | slide::Status::Dragged if enabled => {
                mix(p.accent, p.panel, 0.2)
            }
            _ => p.panel,
        };
        slide::Style {
            rail: slide::Rail {
                backgrounds: (
                    Background::Color(fade(p.accent)),
                    Background::Color(fade(p.line)),
                ),
                width: 4.0,
                border: border::rounded(2.0),
            },
            handle: slide::Handle {
                shape: HandleShape::Circle {
                    radius: SLIDER_HANDLE_RADIUS,
                },
                background: Background::Color(fill),
                border_width: 2.0,
                border_color: fade(p.accent),
            },
        }
    }
}

/// Width of a scrollbar's scroller, in pixels. It floats over the
/// content's edge or in a padding left for it.
pub const SCROLLBAR_WIDTH: f32 = 4.0;

/// Every scrollable's scrollbar: a thin faint scroller on no rail, as
/// the mock's thin scrollbars.
pub fn scrollbar(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let p = palette(theme);
    let rail = scrollable::Rail {
        background: None,
        border: Border::default(),
        scroller: scrollable::Scroller {
            background: Background::Color(p.faint),
            border: border::rounded(SCROLLBAR_WIDTH / 2.0),
        },
    };
    scrollable::Style {
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        ..scrollable::default(theme, status)
    }
}

/// A key label, like `Alt` in a hint. Use a monospace font inside.
pub fn key_chip(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: outline(p.line, CHIP_RADIUS),
        ..filled(p.chip, p.muted)
    }
}

/// A [`key_chip`] on a [`primary_button`].
pub fn accent_key_chip(_theme: &Theme) -> container::Style {
    container::Style {
        border: outline(Color::from_rgba(1.0, 1.0, 1.0, 0.25), CHIP_RADIUS),
        ..filled(Color::from_rgba(1.0, 1.0, 1.0, 0.18), Color::WHITE)
    }
}

/// A drop-down menu, floating over everything else.
pub fn menu(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: outline(p.line, MENU_RADIUS),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.08),
            offset: Vector::new(0.0, 3.0),
            blur_radius: 10.0,
        },
        ..filled(p.panel, p.text)
    }
}

/// The ring around where the button was held while what overlaps there
/// is listed: how far it picks, in the accent, faintly filled.
pub fn pick_ring(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.accent.scale_alpha(0.12))),
        border: Border {
            color: p.accent,
            width: 1.5,
            radius: f32::INFINITY.into(),
        },
        ..container::Style::default()
    }
}

/// Dims the screen behind a dialog.
pub fn scrim(theme: &Theme) -> container::Style {
    container::background(palette(theme).scrim)
}

/// A group of controls floating over the viewport.
pub fn float_panel(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: outline(p.line, FLOAT_RADIUS),
        ..filled(p.panel, p.muted)
    }
}

/// The card floating under the toolbar while a regeneration is slow: a
/// [`float_panel`] with the operation panel's card's shadow.
pub fn regenerating_card(theme: &Theme) -> container::Style {
    container::Style {
        shadow: CARD_SHADOW,
        ..float_panel(theme)
    }
}

/// The bar in a [`regenerating_card`]: the share of the steps done in the
/// accent, on the well.
pub fn regenerating_bar(theme: &Theme) -> progress_bar::Style {
    let p = palette(theme);
    progress_bar::Style {
        background: Background::Color(well(p)),
        bar: Background::Color(p.accent),
        border: border::rounded(2),
    }
}

/// The shadow under the operation panel's card, as the mock's.
pub const CARD_SHADOW: Shadow = Shadow {
    color: Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.1,
    },
    offset: Vector { x: 0.0, y: 6.0 },
    blur_radius: 18.0,
};

/// A single button floating over the viewport, styled like a
/// [`float_panel`].
pub fn float_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let hovered = is_hovered(status);
    button::Style {
        background: Some(Background::Color(if hovered { p.hl } else { p.panel })),
        text_color: if hovered { p.text } else { p.muted },
        border: outline(p.line, FLOAT_RADIUS),
        ..button::Style::default()
    }
}

/// A list row under the cursor, like a body in the side panel.
pub fn hovered_row(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: border::rounded(CONTROL_RADIUS),
        ..filled(p.hl, p.text)
    }
}

/// How many times deeper a selected row's tint (the soft accent's alpha)
/// is while hovered.
pub(super) const SELECTED_HOVER_DEEPEN: f32 = 1.75;

/// A selected row's tint, the soft accent, deepened if `hovered`.
pub(super) fn selected_tint(p: &Palette, hovered: bool) -> Color {
    let soft = p.accent_soft;
    if hovered {
        Color {
            a: (soft.a * SELECTED_HOVER_DEEPEN).min(1.0),
            ..soft
        }
    } else {
        soft
    }
}

/// Halfway between the accent and a hovered selected row's tint (the soft
/// accent deepened) made opaque over the panel, for marks that stand for
/// a selection in a list.
pub fn selection_mark(p: &Palette) -> Color {
    let deep = selected_tint(p, true);
    let hovered = mix(Color { a: 1.0, ..deep }, p.panel, deep.a);
    mix(p.accent, hovered, 0.5)
}

/// A selected list row, like a feature in the Timeline.
pub fn selected_row(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: border::rounded(CONTROL_RADIUS),
        ..filled(p.accent_soft, p.text)
    }
}

/// A list row that can be selected: a [`selected_row`] if `selected`,
/// else a [`hovered_row`] while `hovered`, else bare.
pub fn list_row(selected: bool, hovered: bool) -> fn(&Theme) -> container::Style {
    if selected {
        selected_row
    } else if hovered {
        hovered_row
    } else {
        |_| container::Style::default()
    }
}

/// Text in the colour `pick` takes from the palette.
pub fn text_in(pick: impl Fn(&Palette) -> Color) -> impl Fn(&Theme) -> text::Style {
    move |theme| text::Style {
        color: Some(pick(palette(theme))),
    }
}

/// The accent colour, for what's asked of the user, like picking a plane.
pub fn accent_text(theme: &Theme) -> text::Style {
    text_in(|p| p.accent)(theme)
}

pub fn muted_text(theme: &Theme) -> text::Style {
    text_in(|p| p.muted)(theme)
}

pub fn faint_text(theme: &Theme) -> text::Style {
    text_in(|p| p.faint)(theme)
}

/// The success colour, for what went through: an MCP edit applied.
pub fn ok_text(theme: &Theme) -> text::Style {
    text_in(|p| p.ok)(theme)
}

pub fn danger_text(theme: &Theme) -> text::Style {
    text_in(|p| p.danger)(theme)
}

/// The strong danger colour, for what failed: a failed feature's name.
pub fn failed_text(theme: &Theme) -> text::Style {
    text_in(|p| p.danger_strong)(theme)
}

/// The exclusions' purple (the Create tools' handles' colour), for what a blend
/// names to leave out: an edge around a face it names.
pub fn excluded_text(theme: &Theme) -> text::Style {
    text_in(|p| p.sketching.handle)(theme)
}

/// The warning colour, for what may go wrong but isn't wrong yet.
pub fn warning_text(theme: &Theme) -> text::Style {
    text_in(|p| p.warning)(theme)
}

pub fn separator(theme: &Theme) -> rule::Style {
    rule::Style {
        color: palette(theme).line,
        ..rule::default(theme)
    }
}
