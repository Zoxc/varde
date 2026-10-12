//! The settings page, as `notes/Settings.md` and the UI mock have it: in
//! the viewport's place in a document, in the recent designs' on the
//! welcome screen. A head with Reset to defaults and the close button,
//! over a tab per section, the open one a card of rows in a column: what
//! each setting is and does on the left, its control on the right. Reset
//! to defaults asks first, in a popup listing what it changes.

use std::borrow::Cow;

use iced::widget::{
    Space, button, column, container, row, slider, space, stack, text, text_input, toggler,
};
use iced::{Alignment, Element, Font, Length, Padding};
use varde_expr::{AngleUnit, LengthUnit, Shown};
use varde_render::{Projection, Rotation, Shading};

use crate::chrome::{self, Edge, dialog_button, dialog_of_width, edged, icon_button, scrolled};
use crate::icons::{self, Icon};
use crate::mouse::{DragAction, PivotClick, RotationPoint};
use crate::mouse_only::MouseOnly;
use crate::shortcut::{KeyName, Shortcut};
use crate::theme::{self, Emphasis, TabLook, Tone};
use crate::{
    AUTO_SAVE_MINUTES, Edges, INTERFACE_SCALE_MAX, INTERFACE_SCALE_MIN, INTERFACE_SCALE_STEP,
    MCP_PORT_DEFAULT, MCP_PORT_MAX, MCP_PORT_MIN, Message, RECENT_DESIGNS, ThemeChoice,
    ViewOptions,
};

/// The settings page's state, as the app keeps it.
#[derive(Debug, Clone)]
pub struct SettingsState<'a> {
    /// The settings as they are.
    pub options: ViewOptions,
    /// The section showing.
    pub tab: SettingsTab,
    /// Whether Reset to defaults is asking first.
    pub resetting: bool,
    /// The interface scale the slider is dragged to, not yet let go:
    /// shown, but not set till then.
    pub scale_dragged: Option<u16>,
    /// Where the settings are kept, for the foot.
    pub kept_in: KeptIn<'a>,
    /// The MCP server, for its tab: natively only.
    pub mcp: McpState<'a>,
}

/// What the MCP tab shows of the server besides its settings. Its
/// `Debug` doesn't show the token.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct McpState<'a> {
    pub status: McpStatus<'a>,
    /// The client connected, if one is.
    pub client: Option<McpClient<'a>>,
    /// The bearer token's text, once the IO lane has read or made it.
    pub token: Option<String>,
    /// Whether the token shows, rather than dots.
    pub token_shown: bool,
    /// The port as typed in its field, while that isn't the port set:
    /// a number out of range, or one not yet set.
    pub port_typed: Option<&'a str>,
    /// The running program's path, for the lines starting it as
    /// `varde mcp`, the stdio relay: none if it can't be told, or isn't
    /// text.
    pub exe: Option<&'a str>,
}

impl std::fmt::Debug for McpState<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpState")
            .field("status", &self.status)
            .field("client", &self.client)
            .field("token", &self.token.as_ref().map(|_| TOKEN_HIDDEN))
            .field("token_shown", &self.token_shown)
            .field("port_typed", &self.port_typed)
            .field("exe", &self.exe)
            .finish()
    }
}

/// How the MCP server is, as the tab's Server row and the panel say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum McpStatus<'a> {
    /// Stopped: not started from the panel, or the feature off.
    #[default]
    Off,
    /// Started, waiting for the token or the port.
    Starting,
    /// Listening on the port.
    Listening(u16),
    /// The port couldn't be bound: why.
    BindFailed(&'a str),
}

/// The client connected to the MCP server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpClient<'a> {
    /// The name it gave, if any.
    pub name: Option<&'a str>,
    /// When it connected, as said: "just now", "5 min ago".
    pub since: String,
}

/// The port typed into the MCP tab's field, if it's one it takes: digits
/// alone, from [`MCP_PORT_MIN`] to [`MCP_PORT_MAX`].
pub fn mcp_port_of(typed: &str) -> Option<u16> {
    let typed = typed.trim();
    if typed.is_empty() || typed.len() > 5 || !typed.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    typed
        .parse::<u16>()
        .ok()
        .filter(|port| (MCP_PORT_MIN..=MCP_PORT_MAX).contains(port))
}

/// The lines adding the server at `port` to a client, carrying `token`:
/// Claude Code's command, Codex's `~/.codex/config.toml` table (its
/// `mcp add` takes no header) and opencode's `opencode.json` entry,
/// under `"mcp"` (no OAuth, which it would otherwise try).
pub fn mcp_client_lines(port: u16, token: &str) -> [String; 3] {
    let url = format!("http://127.0.0.1:{port}/mcp");
    [
        format!(
            "claude mcp add --transport http varde {url} --header \"Authorization: Bearer {token}\""
        ),
        format!(
            "[mcp_servers.varde]\nurl = \"{url}\"\nhttp_headers = {{ Authorization = \"Bearer {token}\" }}"
        ),
        format!(
            "\"varde\": {{ \"type\": \"remote\", \"url\": \"{url}\", \"oauth\": false, \"headers\": {{ \"Authorization\": \"Bearer {token}\" }} }}"
        ),
    ]
}

/// The lines adding the server to a client through `varde mcp`, the
/// stdio relay, run from `exe`: Claude Code's command, Codex's
/// `~/.codex/config.toml` table and opencode's `opencode.json` entry,
/// as [`mcp_client_lines`] has them, but with no token: the relay reads
/// it. The path quoted for the shell the command is pasted into (on
/// Windows double quotes, which cmd and PowerShell both read, else
/// POSIX's single). `None` if the path can't be quoted safely: a control
/// character in it (a newline would run half the command when pasted),
/// or on Windows something cmd or PowerShell expands or ends a string at
/// even in double quotes (`%`, `$`, `` ` ``, `!`, `"`, `“`, `”`, `„`).
pub fn mcp_stdio_lines(exe: &str) -> Option<[String; 3]> {
    mcp_stdio_lines_for(exe, cfg!(windows))
}

fn mcp_stdio_lines_for(exe: &str, windows: bool) -> Option<[String; 3]> {
    if exe.chars().any(char::is_control) {
        return None;
    }
    let shell = if windows {
        windows_quoted(exe)?
    } else {
        posix_quoted(exe)
    };
    let string = json_string(exe);
    Some([
        format!("claude mcp add varde -- {shell} mcp"),
        format!("[mcp_servers.varde]\ncommand = {string}\nargs = [\"mcp\"]"),
        format!("\"varde\": {{ \"type\": \"local\", \"command\": [{string}, \"mcp\"] }}"),
    ])
}

/// `word` as one POSIX shell word: as it is if nothing in it is special,
/// else in single quotes, each `'` closed, escaped and reopened.
fn posix_quoted(word: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "/._-+:@%,=".contains(c);
    if !word.is_empty() && word.chars().all(plain) {
        return word.to_owned();
    }
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// `word` as one word for cmd and PowerShell: as it is if nothing in it
/// is special to either (PowerShell splits at `,`, splats at `@`), else
/// in double quotes; `None` if it holds what either expands or ends a
/// string at inside them.
fn windows_quoted(word: &str) -> Option<String> {
    if word
        .chars()
        .any(|c| "%$`!\"\u{201C}\u{201D}\u{201E}".contains(c))
    {
        return None;
    }
    let plain = |c: char| c.is_ascii_alphanumeric() || "/\\._-+:".contains(c);
    if !word.is_empty() && word.chars().all(plain) {
        return Some(word.to_owned());
    }
    Some(format!("\"{word}\""))
}

/// `text` as a string both JSON and TOML read: double quotes, with `"`,
/// `\\` and control characters escaped.
fn json_string(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            c if c.is_control() => quoted.push_str(&format!("\\u{:04x}", c as u32)),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

/// A token hidden: dots in its place.
const TOKEN_HIDDEN: &str = "••••••••••••••••";

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
    /// Natively only: a page can't listen.
    Mcp,
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 6] = [
        SettingsTab::Appearance,
        SettingsTab::View,
        SettingsTab::Units,
        SettingsTab::Mouse,
        SettingsTab::Files,
        SettingsTab::Mcp,
    ];

    /// The tabs shown: on the web, all but Files and MCP.
    pub fn shown() -> impl Iterator<Item = SettingsTab> {
        Self::ALL.into_iter().filter(|&tab| {
            !(cfg!(target_arch = "wasm32") && matches!(tab, SettingsTab::Files | SettingsTab::Mcp))
        })
    }

    fn label(self) -> &'static str {
        match self {
            SettingsTab::Appearance => "Appearance",
            SettingsTab::View => "View",
            SettingsTab::Units => "Units",
            SettingsTab::Mouse => "Mouse",
            SettingsTab::Files => "Files",
            SettingsTab::Mcp => "MCP",
        }
    }
}

/// What's done on the settings page itself; the settings it sets have
/// messages of their own, the view options menu's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsPage {
    /// Shows the section.
    Tab(SettingsTab),
    /// Chooses the units new designs start in.
    SetLengthUnit(LengthUnit),
    /// Sets one of the settings only the page sets.
    Set(Setting),
    /// The interface scale's slider is dragged to this, in percent: only
    /// its label shows it, so the page doesn't rescale under the pointer.
    DragScale(u16),
    /// The interface scale's slider is let go: the scale it was dragged
    /// to is set.
    ReleaseScale,
    /// Reset to defaults: asks first.
    AskReset,
    /// Leaves the settings as they are, the popup closed.
    CancelReset,
    /// Puts every setting back to its default, the popup closed.
    Reset,
    /// Makes a new MCP token in place of the old, ending the session:
    /// natively only.
    RegenerateMcpToken,
    /// The MCP port's field holds this, as typed: set once it's a port
    /// the field takes ([`mcp_port_of`]). Natively only.
    TypeMcpPort(String),
    /// Shows the MCP token, or hides it. Natively only.
    ShowMcpToken(bool),
    /// Ends the MCP client's session. Natively only.
    DisconnectMcp,
}

/// A setting only the settings page sets, to a value. Values the page
/// doesn't offer are ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// In percent, see [`crate::interface_scale_offered`].
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
    /// Set rather than toggled, for a row's reset: a toggle sent with the
    /// setting changed under it, as by the stored settings arriving or a
    /// second click before the page redraws, would flip it the wrong way.
    HiddenEdges(bool),
    Names(bool),
    MouseHints(bool),
    /// In minutes, one of [`AUTO_SAVE_MINUTES`], 0 for off.
    AutoSave(u16),
    /// One of [`RECENT_DESIGNS`].
    RecentDesigns(u16),
    /// Whether the MCP feature (its panel and toggle) is on: natively only.
    Mcp(bool),
    /// From [`crate::MCP_PORT_MIN`] to [`crate::MCP_PORT_MAX`]: natively
    /// only.
    McpPort(u16),
}

/// A setting Reset to defaults changes: its name, and what it is and
/// will be, as the popup says them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingChange {
    pub name: &'static str,
    pub from: Cow<'static, str>,
    pub to: Cow<'static, str>,
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
            ("MCP", on_off(options.mcp)),
        ]
        .map(|(name, said)| (name, Cow::Borrowed(said)))
        .into_iter()
        .chain([("MCP port", Cow::Owned(options.mcp_port.to_string()))])
    };
    // The web has no Files or MCP tab: what they would set isn't listed.
    let native = |name: &str| matches!(name, "Auto-save" | "Recent designs" | "MCP" | "MCP port");
    said(options)
        .zip(said(defaults))
        .filter(|((_, from), (_, to))| from != to)
        .filter(|((name, _), _)| !(cfg!(target_arch = "wasm32") && native(name)))
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

/// Each interface scale offered, as shown: from [`INTERFACE_SCALE_MIN`]
/// in steps of [`INTERFACE_SCALE_STEP`].
const SCALE_LABELS: [&str; 26] = [
    "75%", "80%", "85%", "90%", "95%", "100%", "105%", "110%", "115%", "120%", "125%", "130%",
    "135%", "140%", "145%", "150%", "155%", "160%", "165%", "170%", "175%", "180%", "185%", "190%",
    "195%", "200%",
];

fn scale_said(percent: u16) -> &'static str {
    if !crate::interface_scale_offered(percent) {
        return "Other";
    }
    // Offered, so at least the least and a whole step from it.
    let index = usize::from((percent - INTERFACE_SCALE_MIN) / INTERFACE_SCALE_STEP);
    SCALE_LABELS.get(index).copied().unwrap_or("Other")
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

fn auto_save_said(minutes: u16) -> &'static str {
    match minutes {
        0 => "Off",
        1 => "1 min",
        5 => "5 min",
        10 => "10 min",
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

pub(crate) fn edges_said(edges: Edges) -> &'static str {
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

    let (title, note, rows) = section(state.tab, state.options, state.scale_dragged, &state.mcp);
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
        card = card.push(setting_row(row));
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
    scale_dragged: Option<u16>,
    mcp: &McpState<'a>,
) -> (&'static str, Option<String>, Vec<SettingRow<'a>>) {
    let set = |setting| Message::Settings(SettingsPage::Set(setting));
    let d = ViewOptions::default();
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
                    reset(options.theme, d.theme, theme_said, Message::SetTheme),
                ),
                setting(
                    "Interface scale",
                    "The size of everything but the model",
                    scale_slider(scale_dragged.unwrap_or(options.interface_scale)),
                    reset(options.interface_scale, d.interface_scale, scale_said, |percent| {
                        set(Setting::InterfaceScale(percent))
                    }),
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
                    reset(
                        options.projection,
                        d.projection,
                        projection_said,
                        Message::SetProjection,
                    ),
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
                    reset(options.shading, d.shading, shading_said, Message::SetShading),
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
                    reset(options.edges, d.edges, edges_said, Message::SetEdges),
                ),
                setting(
                    "Hidden edges",
                    "Edges behind the model, dashed",
                    switch(options.hidden_edges, Message::ToggleHiddenEdges),
                    reset(options.hidden_edges, d.hidden_edges, on_off, |on| set(Setting::HiddenEdges(on))),
                ),
                setting(
                    "Names",
                    "The names of bodies, sketches and planes over the view",
                    switch(options.names, Message::ToggleNames),
                    reset(options.names, d.names, on_off, |on| set(Setting::Names(on))),
                ),
                setting(
                    "Mouse hints",
                    "What the mouse buttons do, in the status bar",
                    switch(options.mouse_hints, Message::ToggleMouseHints),
                    reset(options.mouse_hints, d.mouse_hints, on_off, |on| set(Setting::MouseHints(on))),
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
                    reset(options.length_unit, d.length_unit, LengthUnit::symbol, |unit| {
                        Message::Settings(SettingsPage::SetLengthUnit(unit))
                    }),
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
                    reset(options.angle_unit, d.angle_unit, angle_said, |unit| {
                        set(Setting::AngleUnit(unit))
                    }),
                ),
                setting(
                    "Decimal places",
                    "At most, after the point; trailing zeros are left out",
                    decimals_stepper(options),
                    reset(options.decimals, d.decimals, decimals_said, |decimals| {
                        set(Setting::Decimals(decimals))
                    }),
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
                    "Changes go to the recovery file this long after the first",
                    segmented(AUTO_SAVE_MINUTES.map(|minutes| {
                        (
                            None,
                            auto_save_said(minutes),
                            minutes == options.auto_save,
                            set(Setting::AutoSave(minutes)),
                        )
                    })),
                    reset(options.auto_save, d.auto_save, auto_save_said, |minutes| {
                        set(Setting::AutoSave(minutes))
                    }),
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
                    reset(
                        options.recent_designs,
                        d.recent_designs,
                        recent_said,
                        |count| set(Setting::RecentDesigns(count)),
                    ),
                ),
            ],
        ),
        SettingsTab::Mcp => (
            "MCP",
            Some(
                "Lets an outside agent (Claude Code, Codex, an IDE) read and edit the open design; its edits are undone with Undo, as yours are. Only this computer can connect."
                    .to_owned(),
            ),
            mcp_rows(options, mcp),
        ),
    }
}

/// The MCP section's rows: the switch, the port, the client while
/// serving, the token and the lines adding the server to a client.
fn mcp_rows<'a>(options: ViewOptions, mcp: &McpState<'a>) -> Vec<SettingRow<'a>> {
    let set = |setting| Message::Settings(SettingsPage::Set(setting));
    let page = Message::Settings;
    let d = ViewOptions::default();
    let port = options.mcp_port;
    let serving = match mcp.status {
        McpStatus::Listening(bound) if options.mcp => Some(bound),
        _ => None,
    };
    let status: Cow<'static, str> = match mcp.status {
        McpStatus::Off => "Stopped: Start it in the MCP panel".into(),
        McpStatus::Listening(bound) => {
            format!("Listening at 127.0.0.1:{bound}, one client at a time").into()
        }
        McpStatus::BindFailed(_) => "Not started: see the port".into(),
        McpStatus::Starting => "Starting".into(),
    };
    let typed_bad = mcp
        .port_typed
        .is_some_and(|typed| mcp_port_of(typed).is_none());
    let (port_said, port_bad): (Cow<'static, str>, bool) = match mcp.status {
        _ if typed_bad => (
            format!("A number from {MCP_PORT_MIN} to {MCP_PORT_MAX}").into(),
            true,
        ),
        McpStatus::BindFailed(why) if options.mcp => (
            format!("Port {port} can't be used: {why}. Choose another").into(),
            true,
        ),
        _ => ("Where it listens, on 127.0.0.1 only".into(), false),
    };
    let port_text = match mcp.port_typed {
        Some(typed) => typed.to_owned(),
        None => port.to_string(),
    };
    let port_field = text_input(&MCP_PORT_DEFAULT.to_string(), &port_text)
        .on_input(|typed| Message::Settings(SettingsPage::TypeMcpPort(typed)))
        .padding([3, 6])
        .size(12.5)
        .width(70)
        .style(theme::field_input(port_bad));
    let mut rows = vec![
        setting(
            "MCP",
            "Shows the MCP panel, where the server is started",
            switch(options.mcp, set(Setting::Mcp(!options.mcp))),
            reset(options.mcp, d.mcp, on_off, |on| set(Setting::Mcp(on))),
        ),
        SettingRow {
            bad: port_bad,
            ..setting(
                "Port",
                port_said,
                port_field.into(),
                reset(
                    port,
                    MCP_PORT_DEFAULT,
                    |port| port.to_string(),
                    |port| set(Setting::McpPort(port)),
                ),
            )
        },
    ];
    if options.mcp {
        rows.insert(1, setting("Server", status, Space::new().into(), None));
    }
    if serving.is_some() {
        let client: Element<'a, Message> = match &mcp.client {
            Some(client) => row![
                text(client.name.unwrap_or("A client").to_owned()).font(theme::SEMIBOLD),
                text(format!("since {}", client.since))
                    .size(12)
                    .style(theme::muted_text),
                chrome::small_button("Disconnect", Emphasis::Secondary)
                    .on_press(page(SettingsPage::DisconnectMcp)),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
            None => text("None").size(12.5).style(theme::muted_text).into(),
        };
        rows.push(setting(
            "Client",
            "Connected now; Disconnect drops it",
            client,
            None,
        ));
    }
    let copy = |what: String| tipped_button(Icon::Copy, "Copy", Message::Copy(what));
    let token: Element<'a, Message> = match &mcp.token {
        Some(token) => {
            let shown = mcp.token_shown;
            row![
                text(if shown {
                    token.clone()
                } else {
                    TOKEN_HIDDEN.to_owned()
                })
                .size(12)
                .font(Font::MONOSPACE),
                tipped_button(
                    if shown { Icon::EyeOff } else { Icon::Eye },
                    if shown { "Hide" } else { "Show" },
                    page(SettingsPage::ShowMcpToken(!shown)),
                ),
                copy(token.clone()),
                chrome::small_button("Regenerate", Emphasis::Secondary)
                    .on_press(page(SettingsPage::RegenerateMcpToken)),
            ]
            .spacing(4)
            .align_y(Alignment::Center)
            .into()
        }
        None => text("Not read yet")
            .size(12.5)
            .style(theme::muted_text)
            .into(),
    };
    rows.push(setting(
        "Token",
        "Clients send it with each request; a new one drops the client",
        token,
        None,
    ));
    // The stdio relay's lines first, holding no token; then, once the
    // token is known, the HTTP ones, shown with it as the Token row shows
    // it and copied with it.
    let stdio = mcp
        .exe
        .and_then(mcp_stdio_lines)
        .map(|lines| lines.clone().into_iter().zip(lines));
    let http = mcp.token.as_ref().map(|token| {
        let shown = if mcp.token_shown {
            token.as_str()
        } else {
            TOKEN_HIDDEN
        };
        mcp_client_lines(port, shown)
            .into_iter()
            .zip(mcp_client_lines(port, token))
    });
    if stdio.is_some() || http.is_some() {
        let carried = match (stdio.is_some(), http.is_some()) {
            (true, true) => {
                "The first three run this Varde as a relay and hold no token; the last three carry it"
            }
            (true, false) => "Each runs this Varde as a relay, which reads the token",
            _ => "Each carries the token",
        };
        // A path the lines can't hold safely is said, not offered.
        let refused = if mcp.exe.is_some() && stdio.is_none() {
            ". This Varde's path has characters a terminal would read differently, so there are no lines running it as a relay"
        } else {
            ""
        };
        let said = format!(
            "Claude Code: paste into a terminal. Codex: add to ~/.codex/config.toml. opencode: add to opencode.json under \"mcp\". {carried}{refused}"
        );
        let lines = (stdio.into_iter().flatten())
            .chain(http.into_iter().flatten())
            .map(|(shown, copied)| {
                row![
                    container(text(shown).size(11.5).font(Font::MONOSPACE))
                        .width(Length::Fill)
                        .padding([4, 8])
                        .style(theme::reset_row),
                    copy(copied),
                ]
                .spacing(6)
                .align_y(Alignment::Center)
                .into()
            });
        rows.push(SettingRow {
            below: Some(column(lines).spacing(6).into()),
            ..setting("Add to a client", said, Space::new().into(), None)
        });
    }
    rows
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
fn mouse_rows<'a>(options: ViewOptions) -> Vec<SettingRow<'a>> {
    let set = |setting| Message::Settings(SettingsPage::Set(setting));
    let mouse = options.mouse;
    let d = crate::MouseOptions::default();
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
            reset(mouse.left, d.left, drag_said, |action| {
                set(Setting::LeftDrag(action))
            }),
        ),
        setting(
            "Middle drag",
            "Turning, panning or zooming the view",
            drag(mouse.middle, Setting::MiddleDrag),
            reset(mouse.middle, d.middle, drag_said, |action| {
                set(Setting::MiddleDrag(action))
            }),
        ),
        setting(
            "Right drag",
            "With Shift held, it orbits",
            drag(mouse.right, Setting::RightDrag),
            reset(mouse.right, d.right, drag_said, |action| {
                set(Setting::RightDrag(action))
            }),
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
            reset(mouse.rotation, d.rotation, rotation_said, |rotation| {
                set(Setting::Rotation(rotation))
            }),
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
            reset(
                mouse.rotation_point,
                d.rotation_point,
                rotation_point_said,
                |point| set(Setting::RotationPoint(point)),
            ),
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
            reset(
                mouse.pivot_click,
                d.pivot_click,
                pivot_click_said,
                |click| set(Setting::PivotClick(click)),
            ),
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
            reset(
                mouse.wheel_down_zooms_in,
                d.wheel_down_zooms_in,
                zoom_said,
                |down| set(Setting::WheelDownZoomsIn(down)),
            ),
        ),
        setting(
            "Zoom to pointer",
            "Zoom about the point under the pointer, else the middle",
            switch(
                mouse.zoom_to_pointer,
                set(Setting::ZoomToPointer(!mouse.zoom_to_pointer)),
            ),
            reset(mouse.zoom_to_pointer, d.zoom_to_pointer, on_off, |on| {
                set(Setting::ZoomToPointer(on))
            }),
        ),
        setting(
            "Snapping",
            "Sketch tools snap to points, curves and directions unless Shift is held; off, only while it is",
            switch(
                mouse.sketch_snap,
                set(Setting::SketchSnap(!mouse.sketch_snap)),
            ),
            reset(mouse.sketch_snap, d.sketch_snap, on_off, |on| {
                set(Setting::SketchSnap(on))
            }),
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

/// A row of a section's card, see [`setting_row`].
struct SettingRow<'a> {
    name: &'static str,
    does: Cow<'static, str>,
    /// Whether what `does` says is an error, shown as one.
    bad: bool,
    control: Element<'a, Message>,
    /// With the setting not its default: the default, as said, and what
    /// sets it back, see [`reset`].
    reset: Option<(Cow<'static, str>, Message)>,
    /// What goes under the name and control, across the row.
    below: Option<Element<'a, Message>>,
}

/// The row of setting `name`, saying what it `does`, with its `control`,
/// and with `reset`, if it's changed from its default.
fn setting<'a>(
    name: &'static str,
    does: impl Into<Cow<'static, str>>,
    control: Element<'a, Message>,
    reset: Option<(Cow<'static, str>, Message)>,
) -> SettingRow<'a> {
    SettingRow {
        name,
        does: does.into(),
        bad: false,
        control,
        reset,
        below: None,
    }
}

/// With `value` not `default`: the default as `said`, and what sets it
/// back, as choosing it on the page does.
fn reset<T: Copy + PartialEq, S: Into<Cow<'static, str>>>(
    value: T,
    default: T,
    said: impl Fn(T) -> S,
    message: impl FnOnce(T) -> Message,
) -> Option<(Cow<'static, str>, Message)> {
    (value != default).then(|| (said(default).into(), message(default)))
}

/// A muted icon button with a `tip` saying what it does.
fn tipped_button(
    icon: Icon,
    tip: impl iced::widget::text::IntoFragment<'static>,
    message: Message,
) -> Element<'static, Message> {
    chrome::tip(
        icon_button(icon, Tone::Muted, Some(message)).style(theme::flat_button(false, Tone::Muted)),
        text(tip),
    )
}

/// A row of a section's card: the setting's name over a line saying what
/// it does, its control right of them. Changed from its default, it has
/// an accent bar along its left and a reset arrow before the control.
fn setting_row(row: SettingRow<'_>) -> Element<'_, Message> {
    let SettingRow {
        name,
        does,
        bad,
        control,
        reset,
        below,
    } = row;
    let changed = reset.is_some();
    let reset = reset.map(|(default, message)| {
        tipped_button(Icon::Undo, format!("Reset to {default}"), message)
    });
    let line = row![
        column![
            text(name).font(theme::SEMIBOLD),
            text(does).size(12).style(if bad {
                theme::danger_text
            } else {
                theme::muted_text
            }),
        ]
        .spacing(2)
        .width(Length::Fill),
        row![reset, control].spacing(6).align_y(Alignment::Center),
    ]
    .spacing(16)
    .align_y(Alignment::Center);
    let content = container(column![line].push(below).spacing(8)).padding([9, 14]);
    if changed {
        // iced has no one-sided borders: the bar is a strip of its own,
        // the row's padding the narrower by its width.
        row![
            container(Space::new())
                .width(CHANGED_BAR)
                .height(Length::Fill)
                .style(theme::changed_bar),
            content.padding(Padding::from([9, 14]).left(14.0 - CHANGED_BAR)),
        ]
        .into()
    } else {
        content.into()
    }
}

/// The width of a changed setting's bar.
const CHANGED_BAR: f32 = 3.0;

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

/// The interface scale's slider at `percent`, its value beside it: a
/// drag only moves it and its label, the scale set once let go. Only the
/// mouse moves it ([`MouseOnly`]): the keys and the wheel would, with no
/// release to set it.
fn scale_slider<'a>(percent: u16) -> Element<'a, Message> {
    let percent = percent.clamp(INTERFACE_SCALE_MIN, INTERFACE_SCALE_MAX);
    row![
        MouseOnly::new(
            slider(
                INTERFACE_SCALE_MIN..=INTERFACE_SCALE_MAX,
                percent,
                |percent| { Message::Settings(SettingsPage::DragScale(percent)) }
            )
            .step(INTERFACE_SCALE_STEP)
            .on_release(Message::Settings(SettingsPage::ReleaseScale))
            .width(180)
            .style(theme::slider(true))
        ),
        text(scale_said(percent))
            .size(12.5)
            .width(40)
            .align_x(Alignment::End)
            .wrapping(text::Wrapping::None),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
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
