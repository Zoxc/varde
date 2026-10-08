//! A split being set up: its own parts of the move's panel. The UI mock
//! has no Split panel (the icon mock has only the tool, "Split body" in
//! its Modify group), so it's built in the style of the mock's nearest
//! ones, Combine's and Mirror's: the Body, then "Split with" as tiles (a
//! plane or face, another body, a sketch's regions or a line of its
//! curves) over the field the tool is picked into, "Keeps Body 1" (which
//! piece keeps the body's id) and Keep (both pieces, or one as a trim).
//! The viewport's side, the sketches' regions and curves picked as the
//! tool and the pieces' labels, is in `viewport/motion.rs`.

use std::collections::BTreeSet;

use glam::DVec3;
use iced::Element;
use iced::widget::text::Wrapping;
use iced::widget::{column, text};
use varde_document::{FeatureId, Keep, Placement, Side};
use varde_sketch::{Id, Sketch};

use super::{MotionLook, MotionPick, MotionState};
use crate::chrome::sentence;
use crate::icons::Icon;
use crate::operation_panel::{
    Candidate, PanelHover, missing_rows, pick_field, picked_row, tile, tiles,
};
use crate::theme::{self, BOLD};
use crate::{Look, Message};

/// What a split splits with, its "Split with" tiles: a plane or a face
/// (an origin plane from the toolbar, or a face of the model, flat or
/// curved), another body, regions of a sketch, or a line of a sketch's
/// curves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SplitMode {
    #[default]
    Face,
    Body,
    Regions,
    Line,
}

impl SplitMode {
    /// The four in the panel's order.
    pub const ALL: [SplitMode; 4] = [
        SplitMode::Face,
        SplitMode::Body,
        SplitMode::Regions,
        SplitMode::Line,
    ];

    /// Its tile's label: "Face", "Body", "Region", "Line".
    pub fn label(self) -> &'static str {
        match self {
            SplitMode::Face => "Face",
            SplitMode::Body => "Body",
            SplitMode::Regions => "Region",
            SplitMode::Line => "Line",
        }
    }

    /// Its tile's icon, and its tool's row's: the mock's plane, body,
    /// region and line.
    pub fn icon(self) -> Icon {
        match self {
            SplitMode::Face => Icon::SePlane,
            SplitMode::Body => Icon::Body,
            SplitMode::Regions => Icon::SeRegion,
            SplitMode::Line => Icon::Line,
        }
    }

    /// Where to click for its tool, the field's words while it's empty
    /// or picking.
    fn place(self) -> &'static str {
        match self {
            SplitMode::Face => "Click a plane or face",
            SplitMode::Body => "Click a body",
            SplitMode::Regions => "Click regions of a sketch",
            SplitMode::Line => "Click the curves of a line",
        }
    }

    /// What the status bar's hint says while its tool is picked.
    pub fn hint(self) -> &'static str {
        match self {
            SplitMode::Face => "Pick the plane or face",
            SplitMode::Body => "Pick the tool body",
            SplitMode::Regions => "Pick regions",
            SplitMode::Line => "Pick curves",
        }
    }
}

/// A sketch whose curves a split's line is picked from, and where it is.
#[derive(Debug, Clone, Copy)]
pub struct SketchLines<'a> {
    pub feature: FeatureId,
    pub placement: Placement,
    pub sketch: &'a Sketch,
}

/// A piece of the split as the preview shows it, labelled in the
/// viewport at `at` (the middle of its box) with `name`: the body's for
/// the piece keeping its id, the new body's (or "New body") for the
/// other.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitPiece {
    pub at: DVec3,
    pub name: String,
    /// Whether it's the piece keeping the body's id.
    pub keeps: bool,
}

/// No regions picked.
static NONE_PICKED: BTreeSet<usize> = BTreeSet::new();

/// What the panel and the viewport show of a split being set up, beside
/// what every move's session has.
#[derive(Debug, Clone)]
pub struct SplitView<'a> {
    pub mode: SplitMode,
    /// The tool picked for `mode`, as named ("XY plane", "Extrude 1's
    /// end", "Body 2", "Sketch 1"), and what's beside its name ("2
    /// regions", "3 curves"), if one is.
    pub tool: Option<(String, Option<String>)>,
    /// The body split's name, if one is picked and there.
    pub body: Option<&'a str>,
    pub original: Side,
    pub keep: Keep,
    /// The later features naming the body, and the piece they'll get,
    /// the panel's warning: "2 later features use Body 1: they'll get the
    /// back piece".
    pub later: Option<String>,
    /// What the status bar says of it once it's whole: "Body 1 by XY".
    pub info: Option<String>,
    /// While regions are picked: the sketches whose regions show (the
    /// source once there's one), the source, and the source's regions
    /// picked.
    pub candidates: Vec<Candidate<'a>>,
    pub source: Option<FeatureId>,
    pub picked: &'a BTreeSet<usize>,
    /// While regions are picked, how many of the edited split's weren't
    /// found: listed under the tool, each with a cross.
    pub missing: usize,
    /// While a line is picked: the sketches whose curves show (the
    /// line's once there's one).
    pub lines: Vec<SketchLines<'a>>,
    /// The line's sketch and its curves picked, sorted.
    pub chain: Option<(FeatureId, &'a [Id])>,
    /// The pieces the preview shows, labelled; none while it doesn't
    /// split.
    pub pieces: Vec<SplitPiece>,
}

impl SplitView<'_> {
    /// No regions picked, for a view without them.
    pub fn none_picked() -> &'static BTreeSet<usize> {
        &NONE_PICKED
    }

    /// The piece keeping the body's id: the one kept, or with both
    /// kept, the original.
    pub fn kept(&self) -> Side {
        self.keep.kept(self.original)
    }
}

/// The tile icon of `side`, a piece: the one kept drawn, the other
/// dashed.
fn side_icon(side: Side) -> Icon {
    match side {
        Side::Front => Icon::SpFront,
        Side::Back => Icon::SpBack,
    }
}

/// The tile of `keep`: its icon and label.
fn keep_tile(keep: Keep) -> (Icon, &'static str) {
    match keep {
        Keep::Both => (Icon::SpBoth, "Both"),
        Keep::Front => (Icon::SpFront, "Front"),
        Keep::Back => (Icon::SpBack, "Back"),
    }
}

/// The panel's body for a split, after its Body field `body`: "Split
/// with" and its tool's field, "Keeps Body 1" (disabled for a trim,
/// whose kept piece keeps the id), the later features' warning, and
/// Keep.
pub(super) fn body<'a>(
    state: &MotionState<'a>,
    body: Element<'a, Message>,
) -> Element<'a, Message> {
    let editable = state.editable;
    let send = |look: MotionLook| editable.then_some(Message::Look(Look::Motion(look)));
    let Some(split) = &state.split else {
        return body;
    };
    let modes = SplitMode::ALL.iter().map(|&mode| {
        tile(
            mode.icon(),
            mode.label(),
            split.mode == mode,
            send(MotionLook::SplitWith(mode)),
        )
    });
    let on = state.picking == MotionPick::Tool;
    let press = send(MotionLook::Picking(MotionPick::Tool));
    // No cross: picking another replaces it, as a mirror's plane; a
    // region or curve is taken out by clicking it again.
    let row = (split.tool.clone()).map(|(name, meta)| {
        picked_row(
            split.mode.icon(),
            name,
            meta,
            None,
            press.clone(),
            PanelHover::Axis,
            state.hover,
        )
    });
    let place = (row.is_none() || on).then(|| split.mode.place().to_owned());
    let missing = missing_rows(
        split.missing,
        |index| send(MotionLook::DropMissingRegion(index)),
        press.clone(),
        state.hover,
    );
    let rows = row.into_iter().chain(missing).collect();
    let tool = pick_field(rows, place, on, press);
    let trim = split.keep != Keep::Both;
    let sides = [Side::Front, Side::Back].map(|side| {
        let message = send(MotionLook::Original(side)).filter(|_| !trim);
        tile(side_icon(side), side.name(), split.kept() == side, message)
    });
    let keeps = text(format!("Keeps {}", split.body.unwrap_or("the body")))
        .size(11)
        .font(BOLD)
        .wrapping(Wrapping::WordOrGlyph)
        .style(theme::faint_text);
    let later = (split.later.clone()).map(|note| {
        text(sentence(&note).into_owned())
            .size(11.5)
            .wrapping(Wrapping::WordOrGlyph)
            .style(theme::warning_text)
    });
    let keep = [Keep::Both, Keep::Front, Keep::Back].map(|keep| {
        let (icon, label) = keep_tile(keep);
        tile(
            icon,
            label,
            split.keep == keep,
            send(MotionLook::Keep(keep)),
        )
    });
    column![
        body,
        super::section("Split with"),
        tiles(modes),
        tool,
        column![keeps, tiles(sides), later].spacing(3),
        super::section("Keep"),
        tiles(keep),
    ]
    .spacing(10)
    .into()
}
