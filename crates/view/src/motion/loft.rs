//! A loft being set up: its own parts of the move's panel. The UI mock
//! has no loft panel (its toolbar's Create group has only the tool,
//! "Loft", and its icon), so it's built in the style of the mock's
//! nearest ones, the revolve's and the sweep's: the Sections in order,
//! each a row with an up and a down chevron and a cross (picked in the
//! viewport: a region of a sketch, or a sketch point first or last);
//! Smooth or Ruled as tiles; Closed; the Rails (sketch curve chains,
//! picked as a sweep's path's), not while it's closed; then Operation and
//! Bodies as an extrude's. The viewport's side, the sections and their
//! start dots and the rails' curves picked, is in `viewport/motion.rs`.

use glam::DVec2;
use iced::Element;
use iced::widget::column;
use varde_document::{FeatureId, LoftMode, Placement};
use varde_sketch::{Id, Region};

use super::sweep::SweepPart;
use super::{MotionLook, MotionPick, MotionState, PickedRow, SketchLines, picks_field};
use crate::icons::Icon;
use crate::operation_panel::{
    BodyTarget, Candidate, OperationKind, PanelHover, bodies, field, ordered_row, pick_field, tile,
    tiles, toggle,
};
use crate::{Look, Message};

/// A loft's section as the panel lists it and the viewport draws it.
#[derive(Debug, Clone)]
pub struct LoftSection<'a> {
    /// Its sketch's name: "Sketch 2".
    pub name: String,
    /// Whether what it names is gone (its sketch, region or point).
    pub gone: bool,
    pub sketch: FeatureId,
    /// Where its sketch is, if it's placed.
    pub placement: Option<Placement>,
    pub shape: LoftShape<'a>,
}

/// What a loft's section is, as the viewport draws it.
#[derive(Debug, Clone)]
pub enum LoftShape<'a> {
    /// A region, if it's found; its corners that are sketch points, each
    /// with where it is in the sketch, which a click moves its start to;
    /// and the start, if it has one.
    Region {
        region: Option<&'a Region>,
        corners: Vec<(Id, DVec2)>,
        start: Option<Id>,
    },
    /// A sketch point, where it is if it's there.
    Point(Option<DVec2>),
}

impl LoftShape<'_> {
    /// Where its start dot is in its sketch: a region's start corner, or
    /// the point.
    pub fn dot(&self) -> Option<DVec2> {
        match self {
            LoftShape::Region { corners, start, .. } => {
                let start = (*start)?;
                (corners.iter()).find_map(|&(id, at)| (id == start).then_some(at))
            }
            LoftShape::Point(at) => *at,
        }
    }
}

/// What the panel and the viewport show of a loft being set up, beside
/// what every move's session has.
#[derive(Debug, Clone)]
pub struct LoftView<'a> {
    /// The sketches whose regions show: the visible ones before the loft,
    /// and its sections' own.
    pub candidates: Vec<Candidate<'a>>,
    /// The sketches whose points (for point sections), and while the
    /// rails are picked curves, show: the visible ones before the loft,
    /// and its sections' and rails' own.
    pub lines: Vec<SketchLines<'a>>,
    /// Its sections, in order.
    pub sections: Vec<LoftSection<'a>>,
    /// Its rails, as their rows show them.
    pub rails: Vec<SweepPart>,
    /// Each rail's sketch and curves, by the rails' order: drawn in the
    /// selected colour.
    pub chains: Vec<(FeatureId, &'a [Id])>,
    pub mode: LoftMode,
    pub closed: bool,
    pub operation: OperationKind,
    /// The bodies a join, cut or intersect lists.
    pub targets: Vec<BodyTarget<'a>>,
    /// What the status bar says of it once it's whole: "3 sections ·
    /// Smooth · New body".
    pub info: Option<String>,
}

/// No regions picked: a loft's sections are drawn apart.
static NONE_PICKED: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();

impl LoftView<'_> {
    /// No regions picked, for the candidates' shading.
    pub fn none_picked() -> &'static std::collections::BTreeSet<usize> {
        &NONE_PICKED
    }
}

/// The two modes in the panel's order, with their tiles' icons and
/// labels.
const MODES: [(LoftMode, Icon, &str); 2] = [
    (LoftMode::Smooth, Icon::Smooth, "Smooth"),
    (LoftMode::Ruled, Icon::Line, "Ruled"),
];

/// The panel's body for a loft: Sections, the mode's tiles, Closed,
/// Rails (not while closed), Operation and Bodies.
pub(super) fn body<'a>(state: &MotionState<'a>) -> Element<'a, Message> {
    let Some(loft) = &state.loft else {
        return column![].into();
    };
    let editable = state.editable;
    let send = |look: MotionLook| editable.then_some(Message::Look(Look::Motion(look)));

    let picking = state.picking == MotionPick::Regions;
    let press = send(MotionLook::Picking(MotionPick::Regions));
    let last = loft.sections.len().saturating_sub(1);
    let rows: Vec<_> = (loft.sections.iter().enumerate())
        .map(|(at, section)| {
            let icon = match section.shape {
                LoftShape::Region { .. } => Icon::SeRegion,
                LoftShape::Point(_) => Icon::Point,
            };
            let meta = if section.gone {
                "gone".to_owned()
            } else {
                section.name.clone()
            };
            let up = (at > 0).then(|| send(MotionLook::SectionUp(at))).flatten();
            let down = (at < last)
                .then(|| send(MotionLook::SectionUp(at + 1)))
                .flatten();
            ordered_row(
                icon,
                format!("Section {}", at + 1),
                Some(meta),
                Some([up, down]),
                send(MotionLook::DropSection(at)),
                press.clone(),
                PanelHover::Section(at),
                state.hover,
            )
        })
        .collect();
    let place = (rows.is_empty() || picking).then(|| "Click regions or sketch points".to_owned());
    let sections = field("Sections", pick_field(rows, place, picking, press));

    let modes = MODES.map(|(mode, icon, label)| {
        tile(
            icon,
            label,
            loft.mode == mode,
            send(MotionLook::LoftMode(mode)),
        )
    });
    let closed = toggle(
        Icon::CpFull,
        "Closed",
        loft.closed,
        send(MotionLook::Closed),
        Some("Loft the last section back to the first"),
    );
    let rails = (!loft.closed).then(|| {
        let rows = (loft.rails.iter().enumerate()).map(|(at, rail)| PickedRow {
            icon: Icon::Line,
            name: rail.name.clone(),
            meta: rail.meta.clone(),
            drop: MotionLook::DropRail(at),
            hover: PanelHover::Part(at),
            failed: false,
            excluded: false,
        });
        picks_field(
            state,
            MotionPick::Path,
            "Rails",
            "Click sketch curves",
            rows,
        )
    });
    let operations = OperationKind::ALL.map(|kind| {
        tile(
            kind.icon(),
            kind.label(),
            loft.operation == kind,
            send(MotionLook::Operation(kind)),
        )
    });
    let targets = bodies(
        loft.operation,
        &loft.targets,
        |body| send(MotionLook::Target(body)),
        state.hover,
    );
    column![
        sections,
        field("Between sections", tiles(modes)),
        closed,
        rails,
        field("Operation", tiles(operations)),
        targets,
    ]
    .spacing(10)
    .into()
}
