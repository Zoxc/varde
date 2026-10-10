//! The settings page, as `notes/Settings.md` and the UI mock have it: in
//! the viewport's place in a document, in the recent designs' on the
//! welcome screen. A head with Reset to defaults and the close button,
//! over a tab per section, the open one a card of rows in a column: what
//! each setting is and does on the left, its control on the right. Reset
//! to defaults asks first, in a popup listing what it changes.

use iced::widget::{Space, button, column, container, row, space, stack, text, toggler};
use iced::{Alignment, Element, Font, Length, Padding};
use varde_expr::LengthUnit;
use varde_render::{Projection, Shading};

use crate::chrome::{self, Edge, dialog_button, dialog_of_width, edged, icon_button, scrolled};
use crate::icons::{self, Icon};
use crate::shortcut::{KeyName, Shortcut};
use crate::theme::{self, Emphasis, TabLook, Tone};
use crate::{Edges, Message, ThemeChoice, ViewOptions};

/// The settings page's state, as the app keeps it.
#[derive(Debug, Clone, Copy)]
pub struct SettingsState<'a> {
    /// The settings as they are.
    pub options: ViewOptions,
    /// The section showing.
    pub tab: SettingsTab,
    /// Whether Reset to defaults is asking first.
    pub resetting: bool,
    /// Where the settings are kept, for the foot.
    pub kept_in: KeptIn<'a>,
}

/// Where the settings are kept, as the page's foot says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeptIn<'a> {
    /// In the file at this path, as shown.
    File(&'a str),
    /// In `settings.toml` in the browser's storage, on the web.
    Browser,
    /// Nowhere: there's no config directory to keep them in.
    Nowhere,
}

/// A section of the settings page, a tab each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    Appearance,
    View,
    Units,
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 3] = [
        SettingsTab::Appearance,
        SettingsTab::View,
        SettingsTab::Units,
    ];

    fn label(self) -> &'static str {
        match self {
            SettingsTab::Appearance => "Appearance",
            SettingsTab::View => "View",
            SettingsTab::Units => "Units",
        }
    }
}

/// What's done on the settings page itself; the settings it sets have
/// messages of their own, the view options menu's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
    /// Shows the section.
    Tab(SettingsTab),
    /// Chooses the units new designs start in.
    SetLengthUnit(LengthUnit),
    /// Reset to defaults: asks first.
    AskReset,
    /// Leaves the settings as they are, the popup closed.
    CancelReset,
    /// Puts every setting back to its default, the popup closed.
    Reset,
}

/// A setting Reset to defaults changes: its name, and what it is and
/// will be, as the popup says them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingChange {
    pub name: &'static str,
    pub from: &'static str,
    pub to: &'static str,
}

/// The settings of `options` that aren't their defaults, in the page's
/// order, each from what it is to the default.
pub fn changed_settings(options: ViewOptions) -> Vec<SettingChange> {
    let defaults = ViewOptions::default();
    let said = |options: ViewOptions| {
        [
            ("Theme", theme_said(options.theme)),
            ("Projection", projection_said(options.projection)),
            ("Shading", shading_said(options.shading)),
            ("Edges", edges_said(options.edges)),
            ("Hidden edges", on_off(options.hidden_edges)),
            ("Names", on_off(options.names)),
            ("Mouse hints", on_off(options.mouse_hints)),
            ("Length unit", options.length_unit.symbol()),
        ]
    };
    (said(options).into_iter())
        .zip(said(defaults))
        .filter(|((_, from), (_, to))| from != to)
        .map(|((name, from), (_, to))| SettingChange { name, from, to })
        .collect()
}

fn on_off(on: bool) -> &'static str {
    if on { "On" } else { "Off" }
}

fn theme_said(theme: ThemeChoice) -> &'static str {
    match theme {
        ThemeChoice::Auto => "System",
        ThemeChoice::Light => "Light",
        ThemeChoice::Dark => "Dark",
    }
}

fn theme_icon(theme: ThemeChoice) -> Icon {
    match theme {
        ThemeChoice::Auto => Icon::Contrast,
        ThemeChoice::Light => Icon::Sun,
        ThemeChoice::Dark => Icon::Moon,
    }
}

fn projection_said(projection: Projection) -> &'static str {
    match projection {
        Projection::Orthographic => "Orthographic",
        Projection::Perspective => "Perspective",
    }
}

fn projection_icon(projection: Projection) -> Icon {
    match projection {
        Projection::Orthographic => Icon::Ortho,
        Projection::Perspective => Icon::Persp,
    }
}

const SHADINGS: [Shading; 4] = [
    Shading::Regular,
    Shading::Flat,
    Shading::Metal,
    Shading::FlatMetal,
];

fn shading_said(shading: Shading) -> &'static str {
    match shading {
        Shading::Regular => "Regular",
        Shading::Flat => "Flat",
        Shading::Metal => "Metal",
        Shading::FlatMetal => "Flat metal",
    }
}

const EDGES: [Edges; 3] = [Edges::Default, Edges::Wireframe, Edges::Tessellation];

fn edges_said(edges: Edges) -> &'static str {
    match edges {
        Edges::Default => "Features",
        Edges::Wireframe => "Patches",
        Edges::Tessellation => "Triangles",
    }
}

/// The settings gear, for the top right of a document's toolbar: on
/// while the page is `open`, and closing it then.
pub(crate) fn settings_button<'a>(open: bool) -> Element<'a, Message> {
    let button = icon_button(Icon::Gear, Tone::Muted, Some(Message::ToggleSettings))
        .style(theme::flat_button(open, Tone::Muted));
    chrome::tip(
        button,
        text(format!(
            "Settings  {}",
            KeyName::from(Shortcut::SETTINGS).label()
        )),
    )
}

/// The widest the page's column gets.
const COLUMN_WIDTH: f32 = 620.0;

/// The settings page, filling the room it's given, with Reset to
/// defaults' popup over it while it asks.
pub fn page<'a>(state: SettingsState<'a>) -> Element<'a, Message> {
    let changed = changed_settings(state.options);
    let reset = chrome::small_button("Reset to defaults", Emphasis::Secondary)
        .on_press_maybe((!changed.is_empty()).then_some(Message::Settings(SettingsPage::AskReset)));
    let close = chrome::tip(
        icon_button(Icon::Close, Tone::Muted, Some(Message::ToggleSettings)),
        text(format!(
            "Close  {}",
            KeyName::from(Shortcut::ESCAPE).label()
        )),
    );
    let head = row![
        icons::tinted(Icon::Gear, icons::INLINE, |p| p.muted),
        text("Settings").size(14).font(theme::SEMIBOLD),
        space::horizontal(),
        reset,
        Space::new().width(6),
        close,
    ]
    .spacing(8)
    .height(Length::Fill)
    .align_y(Alignment::Center);
    let head = edged(
        container(head)
            .width(Length::Fill)
            .padding(Padding::from([0, 10]).left(24))
            .style(theme::toolbar),
        Edge::Bottom,
        44.0,
    );

    let (title, note, rows) = section(state.tab, state.options);
    let mut section = column![
        container(
            text(title.to_uppercase())
                .size(11)
                .font(theme::BOLD)
                .style(theme::faint_text)
        )
        .padding(Padding::ZERO.left(2)),
    ]
    .spacing(6);
    if let Some(note) = note {
        section = section.push(
            container(text(note).size(12).style(theme::muted_text)).padding(Padding::ZERO.left(2)),
        );
    }
    let mut card = column![];
    for (index, row) in rows.into_iter().enumerate() {
        if index > 0 {
            card = card.push(chrome::hrule());
        }
        card = card.push(row);
    }
    section = section.push(container(card).style(theme::settings_card));
    let foot = text(match state.kept_in {
        KeptIn::File(path) => format!("Kept in {path}."),
        KeptIn::Browser => "Kept in settings.toml, in browser storage.".to_owned(),
        KeptIn::Nowhere => "Not kept: there's no config directory to keep them in.".to_owned(),
    })
    .size(11.5)
    .style(theme::faint_text);
    let column = column![section, container(foot).padding(Padding::ZERO.left(2))]
        .spacing(22)
        .max_width(COLUMN_WIDTH);
    let body = scrolled(
        container(column)
            .center_x(Length::Fill)
            .padding(Padding::from([22, 24]).bottom(40)),
        2.0,
    )
    .height(Length::Fill);

    let page = container(column![head, tabs(state.tab), body])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::welcome);
    if state.resetting && !changed.is_empty() {
        stack![page, reset_popup(changed)].into()
    } else {
        page.into()
    }
}

/// A tab per section, as the side panel's, in the middle of their strip.
fn tabs<'a>(open: SettingsTab) -> Element<'a, Message> {
    let tab = |tab: SettingsTab| {
        let look = if tab == open {
            TabLook::Raised
        } else {
            TabLook::Flat
        };
        let label = text(tab.label()).font(if look == TabLook::Flat {
            Font::DEFAULT
        } else {
            theme::SEMIBOLD
        });
        let face = container(label)
            .height(Length::Fill)
            .align_y(Alignment::Center)
            .padding(Padding::from([0, 14]).bottom(theme::TAB_LINE))
            .style(theme::tab_face(look));
        button(face)
            .height(theme::TAB_HEIGHT)
            .padding(Padding::ZERO.top(theme::TAB_LINE))
            .style(theme::tab(look))
            .on_press(Message::Settings(SettingsPage::Tab(tab)))
            .into()
    };
    let tabs: Vec<Element<'a, Message>> = SettingsTab::ALL.into_iter().map(tab).collect();
    // The tabs hang 1 px over the strip's bottom border, as the side
    // panel's do.
    stack![
        edged(
            container(space::vertical())
                .width(Length::Fill)
                .style(theme::tab_strip),
            Edge::Bottom,
            8.0 + theme::TAB_HEIGHT,
        ),
        container(row(tabs).spacing(2))
            .center_x(Length::Fill)
            .padding(Padding::from(8).bottom(0)),
    ]
    .into()
}

/// The open section's title, the note under it, if it has one, and its
/// rows.
fn section<'a>(
    tab: SettingsTab,
    options: ViewOptions,
) -> (
    &'static str,
    Option<&'static str>,
    Vec<Element<'a, Message>>,
) {
    match tab {
        SettingsTab::Appearance => (
            "Appearance",
            None,
            vec![setting(
                "Theme",
                "Light or dark, or as the system is set",
                segmented(ThemeChoice::ALL.map(|theme| {
                    (
                        Some(theme_icon(theme)),
                        theme_said(theme),
                        theme == options.theme,
                        Message::SetTheme(theme),
                    )
                })),
            )],
        ),
        SettingsTab::View => (
            "View",
            Some(
                "How the viewport draws the model. The status bar's menu changes the same settings.",
            ),
            vec![
                setting(
                    "Projection",
                    "Perspective shrinks what is farther away",
                    segmented([Projection::Orthographic, Projection::Perspective].map(
                        |projection| {
                            (
                                Some(projection_icon(projection)),
                                projection_said(projection),
                                projection == options.projection,
                                Message::SetProjection(projection),
                            )
                        },
                    )),
                ),
                setting(
                    "Shading",
                    "How the faces are lit",
                    segmented(SHADINGS.map(|shading| {
                        (
                            Some(crate::status::shading_icon(shading)),
                            shading_said(shading),
                            shading == options.shading,
                            Message::SetShading(shading),
                        )
                    })),
                ),
                setting(
                    "Edges",
                    "Which edges show besides the feature edges",
                    segmented(EDGES.map(|edges| {
                        (
                            Some(crate::status::edges_icon(edges)),
                            edges_said(edges),
                            edges == options.edges,
                            Message::SetEdges(edges),
                        )
                    })),
                ),
                setting(
                    "Hidden edges",
                    "Edges behind the model, dashed",
                    switch(options.hidden_edges, Message::ToggleHiddenEdges),
                ),
                setting(
                    "Names",
                    "The names of bodies, sketches and planes over the view",
                    switch(options.names, Message::ToggleNames),
                ),
                setting(
                    "Mouse hints",
                    "What the mouse buttons do, in the status bar",
                    switch(options.mouse_hints, Message::ToggleMouseHints),
                ),
            ],
        ),
        SettingsTab::Units => (
            "Units",
            Some("For new designs. A design opened keeps its own units."),
            vec![setting(
                "Length",
                "For sizes typed without a unit, and shown",
                segmented(LengthUnit::ALL.map(|unit| {
                    (
                        None,
                        unit.symbol(),
                        unit == options.length_unit,
                        Message::Settings(SettingsPage::SetLengthUnit(unit)),
                    )
                })),
            )],
        ),
    }
}

/// A row of a section's card: the setting's `name` over a line saying
/// what it does, `control` right of them.
fn setting<'a>(
    name: &'a str,
    does: &'a str,
    control: Element<'a, Message>,
) -> Element<'a, Message> {
    container(
        row![
            column![
                text(name).font(theme::SEMIBOLD),
                text(does).size(12).style(theme::muted_text),
            ]
            .spacing(2)
            .width(Length::Fill),
            control,
        ]
        .spacing(16)
        .align_y(Alignment::Center),
    )
    .padding([9, 14])
    .into()
}

/// A choice among a few, as one bar of buttons, each its icon, if it has
/// one, its label, whether it's the one chosen and what it sends.
fn segmented<'a, const N: usize>(
    choices: [(Option<Icon>, &'static str, bool, Message); N],
) -> Element<'a, Message> {
    let buttons = choices.into_iter().map(|(icon, label, on, message)| {
        let label = text(label)
            .size(12.5)
            .font(if on { theme::SEMIBOLD } else { Font::DEFAULT })
            .wrapping(text::Wrapping::None);
        let content = row![
            icon.map(|icon| icons::tinted(icon, 14.0, move |p| if on { p.text } else { p.muted })),
            label
        ]
        .spacing(6)
        .height(Length::Fill)
        .align_y(Alignment::Center);
        button(content)
            .height(26)
            .padding([0, 10])
            .style(theme::segment(on))
            .on_press(message)
            .into()
    });
    container(row(buttons).spacing(2))
        .padding(2)
        .style(theme::segments)
        .into()
}

/// On or off, sending `message` to turn it the other way.
fn switch<'a>(on: bool, message: Message) -> Element<'a, Message> {
    toggler(on)
        .on_toggle(move |_| message.clone())
        .size(20)
        .style(theme::switch)
        .into()
}

/// Reset to defaults' popup: what it changes, from what it is to the
/// default. Enter resets, Escape cancels (the app's keys).
fn reset_popup<'a>(changed: Vec<SettingChange>) -> Element<'a, Message> {
    let title = match changed.len() {
        1 => "Reset a setting to its default?".to_owned(),
        count => format!("Reset {count} settings to defaults?"),
    };
    let rows = changed.into_iter().map(|change| {
        container(
            row![
                text(change.name).width(Length::Fill).style(|theme| {
                    iced::widget::text::Style {
                        color: Some(theme::palette(theme).text),
                    }
                }),
                text(change.from).style(theme::muted_text),
                text("→"),
                text(change.to).font(theme::SEMIBOLD).style(|theme| {
                    iced::widget::text::Style {
                        color: Some(theme::palette(theme).text),
                    }
                }),
            ]
            .spacing(8),
        )
        .padding([4, 8])
        .width(Length::Fill)
        .style(theme::reset_row)
        .into()
    });
    let cancel = dialog_button(
        "Cancel",
        Emphasis::Secondary.button_style(),
        Some(Message::Settings(SettingsPage::CancelReset)),
    );
    let reset = dialog_button(
        "Reset",
        Emphasis::Primary.button_style(),
        Some(Message::Settings(SettingsPage::Reset)),
    );
    dialog_of_width(
        column![
            text(title).size(14).font(theme::SEMIBOLD),
            column(rows).spacing(2),
            Space::new().height(4),
            row![space::horizontal(), cancel, reset].spacing(8),
        ]
        .spacing(10),
        400.0,
    )
}

#[cfg(test)]
mod tests;
