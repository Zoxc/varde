//! The MCP server on the document screen (see `agents/mcp.md`): the
//! status bar's chip while a client is connected, the toolbar's toggle
//! while the server is on, and the panel on the right listing the
//! client's calls. The app gives none of it on the web, where a page
//! can't listen.

use std::borrow::Cow;

use iced::widget::{Space, column, container, hover, row, text};
use iced::{Alignment, Element, Length, Padding, Theme, border};

use crate::chrome::{self, hrule, icon_button, small_button, vrule};
use crate::icons::{self, Icon};
use crate::settings::McpStatus;
use crate::theme::{self, Emphasis, SEMIBOLD, TAB_HEIGHT, Tone};
use crate::{Message, SettingsPage};

/// How wide the panel is, its rule included, in pixels: the mock's.
pub const MCP_PANEL_WIDTH: f32 = 460.0;

/// What the panel, its toggle and the chip show of the MCP server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpShown<'a> {
    /// Whether the settings' *MCP* switch is on: the feature, its panel
    /// and toggle (not the server, which `status` says).
    pub on: bool,
    /// Whether the panel is open.
    pub open: bool,
    pub status: McpStatus<'a>,
    /// The port set, which the stopped panel says it would serve on.
    pub port: u16,
    /// The client connected, with the name it gave, if one is.
    pub client: Option<Option<&'a str>>,
    /// The client whose session ended, with the name it gave, while its
    /// calls are still listed: a note under them says it's gone.
    pub gone: Option<Option<&'a str>>,
    /// The client's calls, oldest first.
    pub rows: Vec<McpRow<'a>>,
    /// How many calls before `rows` the log let go.
    pub dropped: usize,
    /// The popup asking the user to finish what they have open before
    /// the client's edit, if it's up.
    pub ask: Option<McpAsk<'a>>,
    /// The dialog asking before a revert that undoes later changes, if
    /// it's up.
    pub revert: Option<McpRevertAsk<'a>>,
}

/// The dialog asking before a revert from the panel that would undo
/// later changes (anyone's), listing them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpRevertAsk<'a> {
    /// What the revert goes back to: "before Extruded Sketch 1".
    pub to: &'a str,
    /// The differences from the design now, a line each: "removed
    /// Fillet 1".
    pub differences: &'a [String],
}

/// The popup asking the user to finish the sketch or panel they have
/// open, with something unfinished, before a client's edit applies; or,
/// once that call ended, saying it was cancelled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpAsk<'a> {
    /// The client's name, as it gave it.
    pub client: Option<&'a str>,
    /// What the user is doing, after "you're": "editing Sketch 2".
    pub editing: &'a str,
    /// The client's edit: its tool and its arguments as typed.
    pub tool: McpTool,
    pub args: &'a str,
    /// Why the user's work wasn't applied when Apply yours was tried.
    pub refused: Option<&'a str>,
    /// Whether the call ended unapplied (30 s, cancelled, its session
    /// gone): only Close is offered.
    pub cancelled: bool,
}

impl McpShown<'_> {
    /// The chip the status bar shows, with the client's name, while one
    /// is connected.
    fn chip(&self) -> Option<&str> {
        let client = self.client.filter(|_| self.on)?;
        Some(client_name(client))
    }

    /// Whether the server is listening.
    fn serving(&self) -> bool {
        matches!(self.status, McpStatus::Listening(_))
    }
}

/// What the panel sends: its toggle, the head's buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpPanel {
    /// Opens the panel or hides it: the toolbar's toggle, the chip, ✕.
    Toggle,
    /// Starts the server, or tries the port again after it couldn't be
    /// bound; the settings untouched.
    Start,
    /// Stops the server, the feature staying on.
    Stop,
    /// The popup's Apply yours: finishes what the user has open as its OK
    /// would, then lets the client's edit apply.
    ApplyYours,
    /// The popup's Keep editing, or `Esc`: refuses the client's edit.
    /// Also its Close, once the edit was cancelled.
    KeepEditing,
    /// Opens the context menu of the edit row of this serial.
    OpenMenu(u64),
    /// Closes the row's context menu.
    CloseMenu,
    /// Selects the feature the edit row of this serial made or changed
    /// in the Timeline, scrolled to it.
    ShowInTimeline(u64),
    /// Reverts the design to as it was after the edit of this serial.
    RevertToHere(u64),
    /// Reverts the design to as it was before the edit of this serial.
    RevertBefore(u64),
    /// The revert dialog's Revert.
    ConfirmRevert,
    /// The revert dialog's Cancel, or `Esc`.
    CancelRevert,
}

/// A row of the panel: a client's call, or one of the user's own edits
/// between them ("renamed Hole 2 to Bolt hole", shown after "You:").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpRow<'a> {
    Call(McpCall<'a>),
    User {
        said: Cow<'a, str>,
        /// Whether it's since been taken back: greyed, marked *Reverted*.
        reverted: bool,
    },
    /// A revert made from the panel.
    Revert(McpRevert<'a>),
}

/// A revert made from the panel, as its row shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpRevert<'a> {
    /// What it went back to: "before Extruded Sketch 1".
    pub to: Cow<'a, str>,
    /// Its tooltip: "You: reverted to ..." and what it changed, a line
    /// each.
    pub tip: Cow<'a, str>,
    /// Whether it's been taken back since (undone, or reverted past).
    pub reverted: bool,
}

/// What an edit's row offers and shows beyond a call's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McpEdit {
    /// The row's serial, which its menu's messages name it by.
    pub serial: u64,
    /// Whether what it did has been taken back: greyed, marked
    /// *Reverted*.
    pub reverted: bool,
    /// Whether its reply reached no one: *Applied (client not told)*.
    pub not_told: bool,
    /// Whether its context menu is open.
    pub menu: bool,
    /// Whether Show in Timeline has a feature to show.
    pub timeline: bool,
    /// Whether Revert to here is offered: its after is kept, and isn't
    /// the design now.
    pub here: bool,
    /// Whether Revert to before it is offered: its before is kept.
    pub before: bool,
    /// Whether it's the client's undo of its own edits: its menu has
    /// Show in Timeline only.
    pub undo: bool,
}

/// A client's call, as its row shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpCall<'a> {
    pub tool: McpTool,
    /// Its arguments, as typed text: "Extrude 2", "Body 1, faces facing +z".
    pub args: Cow<'a, str>,
    /// How long ago it came: "Just now", "5 min ago".
    pub ago: String,
    pub state: McpCallState,
    /// What came of it, for its tooltip: a summary of the answer, or why
    /// it failed.
    pub result: Option<&'a str>,
    /// The user's changes its result told the client of, if it told any:
    /// "Told: since its last call, you renamed ...", a line under it.
    pub told: Option<Cow<'a, str>>,
    /// An applied edit's row: its menu and whether it's taken back.
    pub edit: Option<McpEdit>,
}

/// The tools a call can be of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpTool {
    DesignSummary,
    Feature,
    BodyTopology,
    Sketch,
    /// Measuring one item, or between two.
    Measure,
    /// A picture of the model.
    Render,
    /// An edit: renaming a feature or a body, as are those below.
    Rename,
    SetVisible,
    SetColor,
    SetOpacity,
    RemoveFeature,
    RemoveBody,
    AddParameter,
    SetParameter,
    RenameParameter,
    RemoveParameter,
    SetUnits,
    SetTolerance,
    SetRollback,
    AddSketch,
    SetSketchPlane,
    AddLink,
    /// Drawing, constraining and dimensioning in a sketch.
    EditSketch,
    /// Changing a feature's settings by a patch.
    SetFeature,
    Extrude,
    Revolve,
    Combine,
    MoveBody,
    Mirror,
    Pattern,
    Align,
    Scale,
    Split,
    Shell,
    OffsetFace,
    Draft,
    Chamfer,
    Fillet,
    Sweep,
    Loft,
    /// Taking back the client's own edits: its row "Undid: renamed
    /// feature Extrude 1 to Base", or "Undid: 3 edits".
    Undo,
}

impl McpTool {
    /// What a call of it with `args` did, as its row says it: "Renamed
    /// feature Extrude 1 to Base".
    pub fn with(self, args: &str) -> String {
        format!("{} {args}", self.said()).trim_end().to_owned()
    }

    /// What the call did, in the user's words, before its arguments.
    pub fn said(self) -> &'static str {
        match self {
            McpTool::DesignSummary => "Read the design summary",
            McpTool::Feature => "Looked at",
            McpTool::BodyTopology => "Listed the faces and edges of",
            McpTool::Sketch => "Read the sketch",
            McpTool::Measure => "Measured",
            McpTool::Render => "Rendered",
            McpTool::Rename => "Renamed",
            McpTool::SetVisible => "Showed or hid",
            McpTool::SetColor => "Coloured",
            McpTool::SetOpacity => "Set the opacity of",
            McpTool::RemoveFeature => "Removed the feature",
            McpTool::RemoveBody => "Removed the body",
            McpTool::AddParameter => "Added the parameter",
            McpTool::SetParameter => "Set the parameter",
            McpTool::RenameParameter => "Renamed the parameter",
            McpTool::RemoveParameter => "Removed the parameter",
            McpTool::SetUnits => "Set the units to",
            McpTool::SetTolerance => "Set the tolerance to",
            McpTool::SetRollback => "Moved the rollback marker",
            McpTool::AddSketch => "Added a sketch on",
            McpTool::SetSketchPlane => "Changed the plane of",
            McpTool::AddLink => "Linked into",
            McpTool::EditSketch => "Drew in",
            McpTool::SetFeature => "Changed",
            McpTool::Extrude => "Extruded",
            McpTool::Revolve => "Revolved",
            McpTool::Combine => "Combined",
            McpTool::MoveBody => "Moved",
            McpTool::Mirror => "Mirrored",
            McpTool::Pattern => "Patterned",
            McpTool::Align => "Aligned",
            McpTool::Scale => "Scaled",
            McpTool::Split => "Split",
            McpTool::Shell => "Shelled",
            McpTool::OffsetFace => "Offset faces",
            McpTool::Draft => "Drafted",
            McpTool::Chamfer => "Chamfered",
            McpTool::Fillet => "Filleted",
            McpTool::Sweep => "Swept",
            McpTool::Loft => "Lofted",
            McpTool::Undo => "Undid:",
        }
    }

    /// The tool's own icon.
    pub(crate) fn icon(self) -> Icon {
        match self {
            McpTool::DesignSummary => Icon::QSummary,
            McpTool::Feature => Icon::QFeature,
            McpTool::BodyTopology => Icon::QTopology,
            McpTool::Sketch => Icon::QSketch,
            // The measure tool's own: it measures as the tool does.
            McpTool::Measure => Icon::Measure,
            McpTool::Render => Icon::Persp,
            McpTool::Rename | McpTool::RenameParameter => Icon::Rename,
            McpTool::SetVisible => Icon::Eye,
            McpTool::SetColor => Icon::Body,
            McpTool::SetOpacity => Icon::Contrast,
            McpTool::RemoveFeature | McpTool::RemoveBody | McpTool::RemoveParameter => Icon::Trash,
            McpTool::AddParameter | McpTool::SetParameter => Icon::Params,
            McpTool::SetUnits | McpTool::SetTolerance => Icon::Gear,
            McpTool::SetRollback => Icon::Rollback,
            McpTool::AddSketch => Icon::Sketch,
            McpTool::SetSketchPlane => Icon::Plane,
            McpTool::AddLink => Icon::Project,
            McpTool::EditSketch => Icon::Polygon,
            McpTool::SetFeature => Icon::QFeature,
            McpTool::Extrude => Icon::Extrude,
            McpTool::Revolve => Icon::Revolve,
            McpTool::Combine => Icon::Combine,
            McpTool::MoveBody => Icon::Move,
            McpTool::Mirror => Icon::BMirror,
            McpTool::Pattern => Icon::LPattern,
            McpTool::Align => Icon::Align,
            McpTool::Scale => Icon::Scale,
            McpTool::Split => Icon::Split,
            McpTool::Shell => Icon::Shell,
            McpTool::OffsetFace => Icon::OffsetFace,
            McpTool::Draft => Icon::Draft,
            McpTool::Chamfer => Icon::BChamfer,
            McpTool::Fillet => Icon::BFillet,
            McpTool::Sweep => Icon::Sweep,
            McpTool::Loft => Icon::Loft,
            McpTool::Undo => Icon::Undo,
        }
    }
}

/// How a call stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpCallState {
    /// Not answered yet.
    Waiting,
    Answered,
    /// Answered with an error.
    Failed,
    /// An edit applied to the design (failing to regenerate or not: its
    /// tooltip says).
    Applied,
    /// An edit refused or ended before it applied: the design unchanged.
    NotApplied,
}

impl McpCallState {
    fn said(self) -> &'static str {
        match self {
            McpCallState::Waiting => "Waiting",
            McpCallState::Answered => "Answered",
            McpCallState::Failed => "Failed",
            McpCallState::Applied => "Applied",
            McpCallState::NotApplied => "Not applied",
        }
    }
}

/// The popup an MCP client's edit raises while the user has something
/// unfinished open (see [`McpAsk`]), or else the one asking before a
/// revert ([`McpRevertAsk`]), if either is up: over the whole screen.
pub(crate) fn ask_popup<'a>(mcp: &McpShown<'a>) -> Option<Element<'a, Message>> {
    let Some(ask) = mcp.ask.as_ref() else {
        return mcp.revert.as_ref().map(revert_popup);
    };
    let client = client_name(ask.client);
    let edit = ask.tool.with(ask.args);
    // An undo of its own edits says what it takes back: "its last edit".
    let undo = matches!(ask.tool, McpTool::Undo);
    let (title, said) = match ask.cancelled {
        true if undo => (
            format!("{client}'s undo was cancelled"),
            format!(
                "It waited while you were {}, and didn't undo {}. Tell it to try again if \
                 you want it.",
                ask.editing, ask.args
            ),
        ),
        false if undo => (
            format!(
                "{client} wants to undo {} while you're {}.",
                ask.args, ask.editing
            ),
            "Apply yours to finish what you have open and let it go ahead, or keep editing \
             and it's told you are."
                .to_owned(),
        ),
        true => (
            format!("{client}'s edit was cancelled"),
            format!(
                "It waited while you were {}, and wasn't applied: {edit}. Tell it to try \
                 again if you want it.",
                ask.editing
            ),
        ),
        false => (
            format!(
                "{client} wants to change the design while you're {}.",
                ask.editing
            ),
            format!(
                "Its edit: {edit}. Apply yours to finish what you have open and let it \
                 go ahead, or keep editing and it's told you are."
            ),
        ),
    };
    let refused = ask
        .refused
        .map(|why| text(why.to_owned()).style(theme::warning_text));
    let buttons = match ask.cancelled {
        true => row![
            Space::new().width(Length::Fill),
            panel_button("Close", theme::primary_button, McpPanel::KeepEditing),
        ],
        false => row![
            Space::new().width(Length::Fill),
            panel_button(
                "Keep editing",
                theme::secondary_button,
                McpPanel::KeepEditing
            ),
            panel_button("Apply yours", theme::primary_button, McpPanel::ApplyYours),
        ],
    };
    Some(chrome::dialog(
        column![
            text(title).size(14).font(SEMIBOLD),
            text(said).size(12).style(theme::muted_text),
            refused,
            Space::new().height(4),
            buttons.spacing(8),
        ]
        .spacing(8),
    ))
}

/// A dialog's button, `style`d, sending the panel `message`.
fn panel_button<'a>(
    label: &'a str,
    style: fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style,
    message: McpPanel,
) -> iced::widget::Button<'a, Message> {
    chrome::dialog_button(label, style, Some(Message::McpPanel(message)))
}

/// The most differences the revert dialog lists; the rest counted.
const DIFFERENCES: usize = 12;

/// The dialog asking before a revert that undoes later changes.
fn revert_popup<'a>(ask: &McpRevertAsk<'a>) -> Element<'a, Message> {
    let more = ask.differences.len().saturating_sub(DIFFERENCES);
    let lines = (ask.differences.iter().take(DIFFERENCES))
        .map(|line| text(format!("• {line}")).size(12).into())
        .chain((more > 0).then(|| {
            text(format!("and {more} more"))
                .size(12)
                .style(theme::faint_text)
                .into()
        }));
    chrome::dialog(
        column![
            text(format!("Revert to {}?", ask.to))
                .size(14)
                .font(SEMIBOLD),
            text("This undoes later changes too. Undo takes the revert back.")
                .size(12)
                .style(theme::muted_text),
            column(lines).spacing(2),
            Space::new().height(4),
            row![
                Space::new().width(Length::Fill),
                panel_button("Cancel", theme::secondary_button, McpPanel::CancelRevert),
                panel_button("Revert", theme::primary_button, McpPanel::ConfirmRevert),
            ]
            .spacing(8),
        ]
        .spacing(8),
    )
}

/// A client's name as shown: the one it gave, or "A client".
fn client_name(name: Option<&str>) -> &str {
    name.unwrap_or("A client")
}

/// The toolbar's toggle of the panel, on while it's open, if it shows.
pub(crate) fn toggle<'a>(mcp: Option<&McpShown<'_>>) -> Option<Element<'a, Message>> {
    let open = mcp.filter(|mcp| mcp.on)?.open;
    let button = icon_button(
        Icon::Agent,
        Tone::Muted,
        Some(Message::McpPanel(McpPanel::Toggle)),
    )
    .style(theme::flat_button(open, Tone::Muted));
    Some(chrome::tip(button, text("MCP requests")))
}

/// The status bar's chip, while a client is connected: a click opens the
/// panel.
pub(crate) fn status_chip<'a>(mcp: Option<&McpShown<'a>>) -> Option<Element<'a, Message>> {
    let name = mcp?.chip()?;
    let chip = iced::widget::button(chip_content(format!("MCP · {name}"), true))
        .padding(0)
        .style(theme::chip_button)
        .on_press(Message::McpPanel(McpPanel::Toggle));
    Some(chrome::tip(
        chip,
        text(format!(
            "{name} is connected over MCP: edits may come from it. Opens its requests"
        )),
    ))
}

const CHIP_HEIGHT: f32 = 20.0;

/// A chip's dot, green while `live`, and `label`.
fn chip_content<'a>(label: String, live: bool) -> Element<'a, Message> {
    let dot = container(Space::new().width(7).height(7)).style(move |theme: &Theme| {
        let p = theme::palette(theme);
        container::Style {
            background: Some((if live { p.ok } else { p.faint }).into()),
            border: border::rounded(3.5),
            ..Default::default()
        }
    });
    container(
        row![dot, text(label).size(11).font(SEMIBOLD)]
            .spacing(5)
            .align_y(Alignment::Center),
    )
    .padding([0, 7])
    .height(CHIP_HEIGHT)
    .align_y(Alignment::Center)
    .into()
}

/// The MCP chip of the panel's head, as a tag rather than a button.
fn head_chip<'a>(live: bool) -> Element<'a, Message> {
    container(chip_content("MCP".to_owned(), live))
        .style(theme::count_chip)
        .into()
}

/// The panel on the right of the viewport, if it's open: a head with the
/// MCP chip, the client, Disconnect, Start or Stop and ✕ (which hides
/// it, the server going on), over the client's calls, newest last; or,
/// stopped, saying so.
pub(crate) fn panel<'a>(mcp: &McpShown<'a>) -> Option<Element<'a, Message>> {
    if !(mcp.on && mcp.open) {
        return None;
    }
    let serving = mcp.serving();
    let client = mcp.client.filter(|_| serving);
    let who: Element<'a, Message> = match client {
        Some(name) => row![
            text(client_name(name)).font(SEMIBOLD),
            small_button("Disconnect", Emphasis::Secondary)
                .on_press(Message::Settings(SettingsPage::DisconnectMcp)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into(),
        None => text(match mcp.status {
            McpStatus::Listening(_) => "No client",
            McpStatus::Starting => "Starting…",
            McpStatus::Off | McpStatus::BindFailed(_) => "Stopped",
        })
        .style(theme::muted_text)
        .into(),
    };
    // Started but not bound, Start tries again.
    let run = match matches!(mcp.status, McpStatus::Listening(_) | McpStatus::Starting) {
        true => chrome::tip(
            labelled(Icon::Stop, "Stop", Emphasis::Secondary)
                .on_press(Message::McpPanel(McpPanel::Stop)),
            text("Stop the server; the client disconnects"),
        ),
        false => chrome::tip(
            labelled(Icon::Plug, "Start", Emphasis::Primary)
                .on_press(Message::McpPanel(McpPanel::Start)),
            text(format!("Serve on 127.0.0.1:{}", mcp.port)),
        ),
    };
    let close = chrome::tip(
        icon_button(
            Icon::Close,
            Tone::Muted,
            Some(Message::McpPanel(McpPanel::Toggle)),
        )
        .style(theme::flat_button(false, Tone::Muted)),
        text("Hide; the server goes on"),
    );
    let head = container(
        row![
            head_chip(client.is_some()),
            who,
            Space::new().width(Length::Fill),
            run,
            close
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([0, 8])
    .height(TAB_HEIGHT)
    .width(Length::Fill)
    .align_y(Alignment::Center)
    .style(theme::tab_strip);

    let body: Element<'a, Message> = match mcp.status {
        McpStatus::Listening(_) if !mcp.rows.is_empty() => {
            let dropped = (mcp.dropped > 0).then(|| {
                note(match mcp.dropped {
                    1 => "1 earlier call isn't kept".to_owned(),
                    n => format!("{n} earlier calls aren't kept"),
                })
            });
            let gone = mcp
                .gone
                .map(|name| note(format!("{} disconnected", client_name(name))));
            let rows = mcp.rows.iter().map(|row| match row {
                McpRow::Call(call) => call_row(call),
                McpRow::User { said, reverted } => you_row(None, said, *reverted),
                McpRow::Revert(revert) => revert_row(revert),
            });
            chrome::scrolled(
                container(
                    column![dropped, column(rows).spacing(2), gone]
                        .spacing(4)
                        .width(Length::Fill),
                )
                .padding([6, 8]),
                2.0,
            )
            .height(Length::Fill)
            .into()
        }
        McpStatus::Listening(_) => empty(
            "A client connected over MCP reads the open design; its calls show here.".into(),
            None,
        ),
        McpStatus::Starting => empty("Starting…".into(), None),
        McpStatus::BindFailed(why) => empty(
            stopped(mcp.port),
            Some(format!(
                "Port {} can't be used: {why}. Choose another in the settings",
                mcp.port
            )),
        ),
        McpStatus::Off => empty(stopped(mcp.port), None),
    };
    let panel = container(column![head, hrule(), body])
        .width(MCP_PANEL_WIDTH - 1.0)
        .height(Length::Fill)
        .style(theme::side_panel);
    Some(row![vrule(), panel].into())
}

/// What the panel says while the server is stopped.
fn stopped(port: u16) -> String {
    format!(
        "The MCP server is stopped. Start it to let clients such as Claude Code or Codex \
         connect at 127.0.0.1:{port}/mcp."
    )
}

/// A button of the panel's head: `icon` and `label`.
fn labelled<'a>(
    icon: Icon,
    label: &'a str,
    emphasis: Emphasis,
) -> iced::widget::Button<'a, Message> {
    iced::widget::button(
        row![icons::icon(icon, 14.0), text(label).size(12)]
            .spacing(5)
            .align_y(Alignment::Center),
    )
    .padding([3, 9])
    .style(emphasis.button_style())
}

/// The body's note in place of rows, and a failure under it.
fn empty<'a>(said: String, bad: Option<String>) -> Element<'a, Message> {
    let bad = bad.map(|bad| text(bad).style(theme::danger_text).center());
    container(
        column![text(said).style(theme::muted_text).center(), bad]
            .spacing(6)
            .align_x(Alignment::Center),
    )
    .padding([0, 24])
    .center(Length::Fill)
    .into()
}

/// A faint note between the rows.
fn note<'a>(said: String) -> Element<'a, Message> {
    container(text(said).size(12).style(theme::faint_text))
        .center_x(Length::Fill)
        .padding([2, 0])
        .into()
}

/// A call's row, flat as the Timeline's: the tool's icon, what it did
/// and its arguments, how long ago it came and its state; what came of
/// it in its tooltip.
fn call_row<'a>(call: &McpCall<'a>) -> Element<'a, Message> {
    let shown = |hovered: bool| -> Element<'a, Message> {
        container(call_line(call))
            .padding(Padding::from([0, 8]))
            .style(theme::list_row(false, hovered))
            .into()
    };
    let tip = call.result.unwrap_or(call.state.said());
    let row = chrome::tip(hover(shown(false), shown(true)), text(tip.to_owned()));
    let row = match call.edit {
        Some(edit) => edit_menu(row, edit),
        None => row,
    };
    match &call.told {
        Some(told) => column![
            row,
            container(small_text(told.clone(), theme::faint_text))
                .padding(Padding::from([0, 8]))
                .width(Length::Fill)
                .clip(true)
        ]
        .into(),
        None => row,
    }
}

/// An edit's row with its context menu: Show in Timeline, Revert to
/// here, Revert to before it.
fn edit_menu<'a>(row: Element<'a, Message>, edit: McpEdit) -> Element<'a, Message> {
    use crate::menu::{menu_item, menu_separator};
    let serial = edit.serial;
    let message = |offered: bool, message| offered.then_some(Message::McpPanel(message));
    let menu = edit.menu.then(|| {
        let show: Element<'a, Message> = menu_item(
            Icon::QFeature,
            "Show in Timeline".into(),
            None,
            message(edit.timeline, McpPanel::ShowInTimeline(serial)),
        )
        .into();
        if edit.undo {
            return crate::panels::row_menu(vec![show]);
        }
        crate::panels::row_menu(vec![
            show,
            menu_separator().into(),
            menu_item(
                Icon::Rollback,
                "Revert to here".into(),
                None,
                message(edit.here, McpPanel::RevertToHere(serial)),
            )
            .into(),
            menu_item(
                Icon::Undo,
                "Revert to before it".into(),
                None,
                message(edit.before, McpPanel::RevertBefore(serial)),
            )
            .into(),
        ])
    });
    crate::context_menu::ContextMenu::new(
        row,
        menu,
        Message::McpPanel(McpPanel::OpenMenu(serial)),
        Message::McpPanel(McpPanel::CloseMenu),
    )
    .into()
}

/// A revert's row: "You: reverted to before Extruded Sketch 1", greyed
/// once taken back.
fn revert_row<'a>(revert: &McpRevert<'a>) -> Element<'a, Message> {
    let said = format!("reverted to {}", revert.to).into();
    chrome::tip(
        you_row(Some(Icon::Undo), &said, revert.reverted),
        text(revert.tip.clone()),
    )
}

/// A row of something the user did, after `icon` if any: "You: renamed
/// Hole 2 to Bolt hole", greyed and marked *Reverted* once taken back.
fn you_row<'a>(icon: Option<Icon>, said: &Cow<'a, str>, reverted: bool) -> Element<'a, Message> {
    container(
        row![
            icon.map(|icon| icons::icon(icon, icons::INLINE)),
            text("You:").font(SEMIBOLD).wrapping(text::Wrapping::None),
            container(
                text(said.clone())
                    .style(faded(reverted))
                    .wrapping(text::Wrapping::None)
            )
            .width(Length::Fill)
            .clip(true),
            reverted.then(reverted_tag),
        ]
        .spacing(8)
        .height(crate::panels::ROW_HEIGHT)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([0, 8]))
    .into()
}

/// A row's secondary text's style: faint once taken back, else muted.
fn faded(reverted: bool) -> fn(&iced::Theme) -> text::Style {
    match reverted {
        true => theme::faint_text,
        false => theme::muted_text,
    }
}

/// The *Reverted* a row taken back ends with.
fn reverted_tag<'a>() -> iced::widget::Text<'a> {
    small_text("Reverted", theme::faint_text)
}

/// How big a row's small text is: its state, what the client was told.
const SMALL_SIZE: f32 = 11.5;

/// A row's small text `said`, styled by `style`, on one line.
fn small_text<'a>(
    said: impl text::IntoFragment<'a>,
    style: fn(&iced::Theme) -> text::Style,
) -> iced::widget::Text<'a> {
    text(said)
        .size(SMALL_SIZE)
        .style(style)
        .wrapping(text::Wrapping::None)
}

/// A call's row's line.
fn call_line<'a>(call: &McpCall<'a>) -> Element<'a, Message> {
    let reverted = call.edit.is_some_and(|edit| edit.reverted);
    let not_told = call.edit.is_some_and(|edit| edit.not_told);
    let state: Element<'a, Message> = match call.state {
        // Taken back since: greyed, saying so.
        McpCallState::Applied if reverted => reverted_tag().into(),
        McpCallState::Applied if not_told => {
            small_text("Applied (client not told)", theme::ok_text).into()
        }
        McpCallState::Waiting => icons::tinted(Icon::More, icons::INLINE, |p| p.accent).into(),
        McpCallState::Answered => icons::tinted(Icon::Check, icons::INLINE, |p| p.ok).into(),
        McpCallState::Failed => icons::tinted(Icon::Close, icons::INLINE, |p| p.danger).into(),
        // An edit says how it went in words: whether the design changed.
        McpCallState::Applied => small_text(call.state.said(), theme::ok_text).into(),
        McpCallState::NotApplied => small_text(call.state.said(), theme::danger_text).into(),
    };
    let said = text(call.tool.said()).wrapping(text::Wrapping::None);
    let said = match reverted {
        true => said.style(theme::faint_text),
        false => said.font(SEMIBOLD),
    };
    row![
        icons::icon(call.tool.icon(), icons::INLINE),
        said,
        container(
            text(call.args.clone())
                .style(faded(reverted))
                .wrapping(text::Wrapping::None)
        )
        .width(Length::Fill)
        .clip(true),
        small_text(call.ago.clone(), theme::faint_text),
        state,
    ]
    .spacing(8)
    .height(crate::panels::ROW_HEIGHT)
    .align_y(Alignment::Center)
    .into()
}

#[cfg(test)]
mod tests;
