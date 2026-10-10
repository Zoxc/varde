//! The settings page, as `notes/Settings.md` and the UI mock have it: in
//! the viewport's place in a document, in the recent designs' on the
//! welcome screen. A head with Reset to defaults and the close button,
//! over a tab per section, the open one a card of rows in a column: what
//! each setting is and does on the left, its control on the right. Reset
//! to defaults asks first, in a popup listing what it changes.

use iced::widget::{Space, button, column, container, row, space, stack, text, toggler};
use iced::{Alignment, Element, Font, Length, Padding};
use varde_expr::{AngleUnit, LengthUnit, Shown};
use varde_render::{Projection, Rotation, Shading};

use crate::chrome::{self, Edge, dialog_button, dialog_of_width, edged, icon_button, scrolled};
use crate::icons::{self, Icon};
use crate::mouse::{DragAction, PivotClick, RotationPoint};
use crate::shortcut::{KeyName, Shortcut};
use crate::theme::{self, Emphasis, TabLook, Tone};
use crate::{
    AUTO_SAVE_PAUSES, Edges, INTERFACE_SCALES, Message, RECENT_DESIGNS, ThemeChoice, ViewOptions,
};

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
    Mouse,
    /// Natively only: the web has no tab for files.
    Files,
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 5] = [
        SettingsTab::Appearance,
        SettingsTab::View,
        SettingsTab::Units,
        SettingsTab::Mouse,
        SettingsTab::Files,
    ];

    /// The tabs shown: on the web, all but Files.
    pub fn shown() -> impl Iterator<Item = SettingsTab> {
        Self::ALL
            .into_iter()
            .filter(|&tab| !(cfg!(target_arch = "wasm32") && tab == SettingsTab::Files))
    }

    fn label(self) -> &'static str {
        match self {
            SettingsTab::Appearance => "Appearance",
            SettingsTab::View => "View",
            SettingsTab::Units => "Units",
            SettingsTab::Mouse => "Mouse",
            SettingsTab::Files => "Files",
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
    /// Sets one of the settings only the page sets.
    Set(Setting),
    /// Reset to defaults: asks first.
    AskReset,
    /// Leaves the settings as they are, the popup closed.
    CancelReset,
    /// Puts every setting back to its default, the popup closed.
    Reset,
}

/// A setting only the settings page sets, to a value. Values the page
/// doesn't offer are ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// In percent, one of [`INTERFACE_SCALES`].
    InterfaceScale(u16),
    AngleUnit(AngleUnit),
    /// `None` for each unit's own, else at most [`Shown::MAX_DECIMALS`].
    Decimals(Option<u8>),
    LeftDrag(DragAction),
    MiddleDrag(DragAction),
    RightDrag(DragAction),
    Rotation(Rotation),
    RotationPoint(RotationPoint),
    PivotClick(PivotClick),
    /// Whether turning the wheel down zooms in.
    WheelDownZoomsIn(bool),
    ZoomToPointer(bool),
    SketchSnap(bool),
    /// In seconds, one of [`AUTO_SAVE_PAUSES`], 0 for off.
    AutoSave(u16),
    /// One of [`RECENT_DESIGNS`].
    RecentDesigns(u16),
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
            ("Interface scale", scale_said(options.interface_scale)),
            ("Length unit", options.length_unit.symbol()),
            ("Angle unit", angle_said(options.angle_unit)),
            ("Decimal places", decimals_said(options.decimals)),
            ("Left drag", drag_said(options.mouse.left)),
            ("Middle drag", drag_said(options.mouse.middle)),
            ("Right drag", drag_said(options.mouse.right)),
            ("Rotation", rotation_said(options.mouse.rotation)),
            (
                "Rotation point",
                rotation_point_said(options.mouse.rotation_point),
            ),
            (
                "Set rotation point with",
                pivot_click_said(options.mouse.pivot_click),
            ),
            (
                "Zoom direction",
                zoom_said(options.mouse.wheel_down_zooms_in),
            ),
            ("Zoom to pointer", on_off(options.mouse.zoom_to_pointer)),
            ("Snapping", on_off(options.mouse.sketch_snap)),
            ("Auto-save", auto_save_said(options.auto_save)),
            ("Recent designs", recent_said(options.recent_designs)),
        ]
    };
    // The web has no Files tab: what it would set isn't listed.
    let files = |name: &str| matches!(name, "Auto-save" | "Recent designs");
    (said(options).into_iter())
        .zip(said(defaults))
        .filter(|((_, from), (_, to))| from != to)
        .filter(|((name, _), _)| !(cfg!(target_arch = "wasm32") && files(name)))
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

fn scale_said(percent: u16) -> &'static str {
    match percent {
        90 => "90%",
        100 => "100%",
        125 => "125%",
        150 => "150%",
        _ => "Other",
    }
}

fn angle_said(unit: AngleUnit) -> &'static str {
    match unit {
        AngleUnit::Deg => "Degrees",
        AngleUnit::Rad => "Radians",
    }
}

fn decimals_said(decimals: Option<u8>) -> &'static str {
    const COUNTS: [&str; 7] = ["0", "1", "2", "3", "4", "5", "6"];
    match decimals {
        None => "Auto",
        Some(count) => COUNTS
            .get(usize::from(count.min(Shown::MAX_DECIMALS)))
            .copied()
            .unwrap_or("6"),
    }
}

fn drag_said(action: DragAction) -> &'static str {
    match action {
        DragAction::Orbit => "Orbit",
        DragAction::Pan => "Pan",
        DragAction::Zoom => "Zoom",
        DragAction::Nothing => "Nothing",
    }
}

fn rotation_said(rotation: Rotation) -> &'static str {
    match rotation {
        Rotation::Turntable => "Turntable",
        Rotation::OverTheTop => "Turntable, over the top",
        Rotation::Trackball => "Trackball",
    }
}

fn rotation_point_said(point: RotationPoint) -> &'static str {
    match point {
        RotationPoint::Clicked => "Clicked",
        RotationPoint::Pointer => "Under the pointer",
        RotationPoint::Centre => "View centre",
    }
}

fn pivot_click_said(click: PivotClick) -> &'static str {
    match click {
        PivotClick::Middle => "Middle click",
        PivotClick::MiddleDouble => "Middle double-click",
        PivotClick::ShiftMiddle => "Shift + middle click",
    }
}

fn zoom_said(wheel_down_zooms_in: bool) -> &'static str {
    if wheel_down_zooms_in {
        "Wheel down"
    } else {
        "Wheel up"
    }
}

fn auto_save_said(seconds: u16) -> &'static str {
    match seconds {
        0 => "Off",
        3 => "3 s",
        10 => "10 s",
        30 => "30 s",
        _ => "Other",
    }
}

fn recent_said(count: u16) -> &'static str {
    match count {
        10 => "10",
        25 => "25",
        50 => "50",
        100 => "100",
        _ => "Other",
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
    let tabs: Vec<Element<'a, Message>> = SettingsTab::shown().map(tab).collect();
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
) -> (&'static str, Option<String>, Vec<Element<'a, Message>>) {
    let set = |setting| Message::Settings(SettingsPage::Set(setting));
    match tab {
        SettingsTab::Appearance => (
            "Appearance",
            None,
            vec![
                setting(
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
                ),
                setting(
                    "Interface scale",
                    "The size of everything but the model",
                    segmented(INTERFACE_SCALES.map(|percent| {
                        (
                            None,
                            scale_said(percent),
                            percent == options.interface_scale,
                            set(Setting::InterfaceScale(percent)),
                        )
                    })),
                ),
            ],
        ),
        SettingsTab::View => (
            "View",
            Some(
                "How the viewport draws the model. The status bar's menu changes the same settings."
                    .to_owned(),
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
            Some(units_note(options)),
            vec![
                setting(
                    "Length",
                    "For new designs: sizes typed without a unit, and shown",
                    segmented(LengthUnit::ALL.map(|unit| {
                        (
                            None,
                            unit.symbol(),
                            unit == options.length_unit,
                            Message::Settings(SettingsPage::SetLengthUnit(unit)),
                        )
                    })),
                ),
                setting(
                    "Angle",
                    "How angles are shown; typed without a unit, they're degrees",
                    segmented([AngleUnit::Deg, AngleUnit::Rad].map(|unit| {
                        (
                            None,
                            angle_said(unit),
                            unit == options.angle_unit,
                            set(Setting::AngleUnit(unit)),
                        )
                    })),
                ),
                setting(
                    "Decimal places",
                    "At most, after the point; trailing zeros are left out",
                    decimals_stepper(options),
                ),
            ],
        ),
        SettingsTab::Mouse => ("Mouse", None, mouse_rows(options)),
        SettingsTab::Files => (
            "Files",
            None,
            vec![
                setting(
                    "Auto-save",
                    "Changes go to the recovery file after a pause in editing",
                    segmented(AUTO_SAVE_PAUSES.map(|seconds| {
                        (
                            None,
                            auto_save_said(seconds),
                            seconds == options.auto_save,
                            set(Setting::AutoSave(seconds)),
                        )
                    })),
                ),
                setting(
                    "Recent designs",
                    "How many the welcome screen keeps",
                    segmented(RECENT_DESIGNS.map(|count| {
                        (
                            None,
                            recent_said(count),
                            count == options.recent_designs,
                            set(Setting::RecentDesigns(count)),
                        )
                    })),
                ),
            ],
        ),
    }
}

/// The Units section's note: what its settings are for, and a length and
/// an angle as they're shown.
fn units_note(options: ViewOptions) -> String {
    let shown = options.shown();
    let length = shown.format(12.345_67, Some(options.length_unit.into()));
    let angle = shown.format(std::f64::consts::PI / 8.0, Some(AngleUnit::Deg.into()));
    format!(
        "Length for new designs; a design opened keeps its own. Angles and decimals for every design. Shown as {length} · {angle}."
    )
}

/// The Mouse section's rows: a drag per button, how the view turns and
/// about what, the wheel's zoom and the sketch tools' snapping.
fn mouse_rows<'a>(options: ViewOptions) -> Vec<Element<'a, Message>> {
    let set = |setting| Message::Settings(SettingsPage::Set(setting));
    let mouse = options.mouse;
    let drag = |on: DragAction, setting: fn(DragAction) -> Setting| {
        segmented(DragAction::ALL.map(|action| {
            (
                None,
                drag_said(action),
                action == on,
                Message::Settings(SettingsPage::Set(setting(action))),
            )
        }))
    };
    let mut rows = vec![
        setting(
            "Left drag",
            "Outside a sketch; a click always selects",
            drag(mouse.left, Setting::LeftDrag),
        ),
        setting(
            "Middle drag",
            "Turning, panning or zooming the view",
            drag(mouse.middle, Setting::MiddleDrag),
        ),
        setting(
            "Right drag",
            "With Shift held, it orbits",
            drag(mouse.right, Setting::RightDrag),
        ),
        setting(
            "Rotation",
            "How a drag turns the view",
            segmented(
                [
                    Rotation::Turntable,
                    Rotation::OverTheTop,
                    Rotation::Trackball,
                ]
                .map(|rotation| {
                    (
                        None,
                        match rotation {
                            Rotation::Turntable => "Turntable",
                            Rotation::OverTheTop => "Over the top",
                            Rotation::Trackball => "Trackball",
                        },
                        rotation == mouse.rotation,
                        set(Setting::Rotation(rotation)),
                    )
                }),
            ),
        ),
        setting(
            "Rotation point",
            "What the view orbits about",
            segmented(RotationPoint::ALL.map(|point| {
                (
                    None,
                    rotation_point_said(point),
                    point == mouse.rotation_point,
                    set(Setting::RotationPoint(point)),
                )
            })),
        ),
    ];
    if mouse.rotation_point == RotationPoint::Clicked {
        rows.push(setting(
            "Set rotation point with",
            "On the model or grid; elsewhere, the centre again",
            segmented(PivotClick::ALL.map(|click| {
                (
                    None,
                    pivot_click_said(click),
                    click == mouse.pivot_click,
                    set(Setting::PivotClick(click)),
                )
            })),
        ));
    }
    rows.extend([
        setting(
            "Zoom direction",
            "Which way the wheel zooms in",
            segmented([false, true].map(|down| {
                (
                    None,
                    zoom_said(down),
                    down == mouse.wheel_down_zooms_in,
                    set(Setting::WheelDownZoomsIn(down)),
                )
            })),
        ),
        setting(
            "Zoom to pointer",
            "Zoom about the point under the pointer, else the middle",
            switch(
                mouse.zoom_to_pointer,
                set(Setting::ZoomToPointer(!mouse.zoom_to_pointer)),
            ),
        ),
        setting(
            "Snapping",
            "Sketch tools snap to points, curves and directions unless Shift is held; off, only while it is",
            switch(
                mouse.sketch_snap,
                set(Setting::SketchSnap(!mouse.sketch_snap)),
            ),
        ),
    ]);
    rows
}

/// The most decimal places shown: fewer or more, from each unit's own
/// count (the length unit's) while that's what's shown, or back to it.
fn decimals_stepper<'a>(options: ViewOptions) -> Element<'a, Message> {
    let set = |decimals| Message::Settings(SettingsPage::Set(Setting::Decimals(decimals)));
    let count = options.decimals.map_or_else(
        || u8::try_from(options.length_unit.decimals()).unwrap_or(Shown::MAX_DECIMALS),
        |count| count.min(Shown::MAX_DECIMALS),
    );
    let step = |label: &'static str, to: Option<u8>| {
        button(text(label).size(12.5).width(14).align_x(Alignment::Center))
            .height(26)
            .padding([0, 8])
            .style(theme::segment(false))
            .on_press_maybe(to.map(|to| set(Some(to))))
    };
    let fewer = step("−", count.checked_sub(1));
    let more = step(
        "+",
        count
            .checked_add(1)
            .filter(|&more| more <= Shown::MAX_DECIMALS),
    );
    let shown = container(
        text(decimals_said(options.decimals))
            .size(12.5)
            .font(theme::SEMIBOLD),
    )
    .width(40)
    .height(26)
    .center_x(40)
    .align_y(Alignment::Center);
    let auto = options.decimals.is_none();
    let auto =
        button(
            text("Auto")
                .size(12.5)
                .font(if auto { theme::SEMIBOLD } else { Font::DEFAULT }),
        )
        .height(26)
        .padding([0, 10])
        .style(theme::segment(auto))
        .on_press_maybe((!auto).then(|| set(None)));
    container(
        row![fewer, shown, more, auto]
            .spacing(2)
            .align_y(Alignment::Center),
    )
    .padding(2)
    .style(theme::segments)
    .into()
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
