//! A sweep being set up: its own parts of the move's panel. The UI mock
//! has no sweep panel (the icon mock has only the tool, "Sweep" in its
//! Create group), so it's built in the style of the mock's nearest one,
//! the revolve's: the Profile (regions picked as an extrude's), then what
//! the path is, as tiles (Path or Helix): a path's parts (each click on a
//! sketch's curve adding the chain it's in, each model edge picked as a
//! blend's), Tangent chain, Keep orientation and Twist; or a helix's
//! Axis, Pitch, Turns, Left-handed and Flip; then Operation and Bodies as
//! an extrude's. The viewport's side, the profile's regions and the path
//! sketches' curves picked, is in `viewport/motion.rs`.

use std::collections::BTreeSet;

use iced::Element;
use iced::widget::column;
use varde_document::FeatureId;
use varde_sketch::Id;

use super::blend::chain_toggle;
use super::{
    BlendEdges, MotionField, MotionLook, MotionPick, MotionState, PickedRow, SketchLines,
    picks_field,
};
use crate::extrude::region_name;
use crate::icons::Icon;
use crate::operation_panel::{
    BodyTarget, Candidate, OperationKind, PanelHover, bodies, field, tile, tiles, toggle,
};
use crate::{Look, Message};

/// What a sweep's path is, its Path and Helix tiles: parts joined end
/// to end (sketch chains and model edges), or a helix about an axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SweepPath {
    #[default]
    Path,
    Helix,
}

impl SweepPath {
    /// The two in the panel's order.
    pub const ALL: [SweepPath; 2] = [SweepPath::Path, SweepPath::Helix];

    /// Its tile's label: "Path", "Helix".
    pub fn label(self) -> &'static str {
        match self {
            SweepPath::Path => "Path",
            SweepPath::Helix => "Helix",
        }
    }

    /// Its tile's icon: the sweep's own, or a coil.
    fn icon(self) -> Icon {
        match self {
            SweepPath::Path => Icon::Sweep,
            SweepPath::Helix => Icon::SwHelix,
        }
    }
}

/// A part of a sweep's path of a sketch's curves, as its row shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepPart {
    /// The sketch's name: "Sketch 2".
    pub name: String,
    /// How many curves: "3 curves".
    pub meta: Option<String>,
}

/// No regions picked.
static NONE_PICKED: BTreeSet<usize> = BTreeSet::new();

/// What the panel and the viewport show of a sweep being set up, beside
/// what every move's session has.
#[derive(Debug, Clone)]
pub struct SweepView<'a> {
    pub path: SweepPath,
    /// The sketches whose regions show (the profile's once there's one),
    /// the profile's sketch, and its regions picked.
    pub candidates: Vec<Candidate<'a>>,
    pub source: Option<FeatureId>,
    pub picked: &'a BTreeSet<usize>,
    /// How many of the edited sweep's regions weren't found: listed
    /// after those picked, each with a cross.
    pub missing: usize,
    /// The path's parts of sketch curves, in the order they were picked.
    pub parts: Vec<SweepPart>,
    /// The path's model edges, picked as a blend's, and its Tangent
    /// chain tick.
    pub edges: BlendEdges,
    /// While the path is picked: the sketches whose curves show, every
    /// visible one before the sweep but the profile's.
    pub lines: Vec<SketchLines<'a>>,
    /// Each path part's sketch and curves, by the parts' order: drawn
    /// in the selected colour.
    pub chains: Vec<(FeatureId, &'a [Id])>,
    pub keep_orientation: bool,
    pub left_handed: bool,
    pub operation: OperationKind,
    /// The bodies a join, cut or intersect lists.
    pub targets: Vec<BodyTarget<'a>>,
    /// What the status bar says of it once it's whole: "Along Sketch 2 ·
    /// Follow path · New body".
    pub info: Option<String>,
}

impl SweepView<'_> {
    /// No regions picked, for a view without them.
    pub fn none_picked() -> &'static BTreeSet<usize> {
        &NONE_PICKED
    }
}

/// The panel's body for a sweep: Profile, the path's tiles and its
/// fields (a path's parts and options, or a helix's axis `reference`,
/// pitch, turns and ticks), Operation and Bodies.
pub(super) fn body<'a>(
    state: &MotionState<'a>,
    reference: Element<'a, Message>,
    value: impl Fn(MotionField, &'a str) -> Element<'a, Message>,
) -> Element<'a, Message> {
    let Some(sweep) = &state.sweep else {
        return column![].into();
    };
    let editable = state.editable;
    let send = |look: MotionLook| editable.then_some(Message::Look(Look::Motion(look)));

    let regions = (sweep.source.into_iter()).flat_map(|sketch| {
        sweep.picked.iter().map(move |&region| PickedRow {
            icon: Icon::SeRegion,
            name: region_name(region),
            meta: None,
            drop: MotionLook::SweepRegion { sketch, region },
            hover: PanelHover::Region { sketch, region },
            failed: false,
        })
    });
    let regions = regions.chain((0..sweep.missing).map(|index| PickedRow {
        icon: Icon::SeRegion,
        name: "Missing region".to_owned(),
        meta: None,
        drop: MotionLook::DropMissingRegion(index),
        hover: PanelHover::Missing(index),
        failed: true,
    }));
    let empty = (sweep.candidates.iter()).all(|candidate| candidate.profiles.regions.is_empty());
    let place = if sweep.picked.is_empty() && empty {
        "No closed regions to sweep"
    } else {
        "Click regions"
    };
    let profile = picks_field(state, MotionPick::Regions, "Profile", place, regions);

    let paths = SweepPath::ALL.iter().map(|&path| {
        tile(
            path.icon(),
            path.label(),
            sweep.path == path,
            send(MotionLook::SweepPath(path)),
        )
    });
    let path: Element<'a, Message> = match sweep.path {
        SweepPath::Path => {
            let parts = (sweep.parts.iter().enumerate()).map(|(at, part)| PickedRow {
                icon: Icon::Line,
                name: part.name.clone(),
                meta: part.meta.clone(),
                drop: MotionLook::DropPart(at),
                hover: PanelHover::Part(at),
                failed: false,
            });
            let edges = (sweep.edges.edges.iter().enumerate()).map(|(at, edge)| PickedRow {
                icon: if edge.round {
                    Icon::SeRim
                } else {
                    Icon::SeEdge
                },
                name: edge.name.clone(),
                meta: edge.meta.clone(),
                drop: MotionLook::DropEdge(edge.edge),
                hover: PanelHover::Edge(at),
                failed: false,
            });
            let rows = parts.chain(edges);
            let field = picks_field(
                state,
                MotionPick::Path,
                "Path",
                "Click sketch curves or edges",
                rows,
            );
            let keep = toggle(
                Icon::Move,
                "Keep orientation",
                sweep.keep_orientation,
                send(MotionLook::KeepOrientation),
                Some("Move the profile along without turning it"),
            );
            column![
                field,
                chain_toggle(state, &sweep.edges),
                keep,
                value(MotionField::Twist, "Twist"),
            ]
            .spacing(10)
            .into()
        }
        SweepPath::Helix => {
            let left = toggle(
                Icon::Revolve,
                "Left-handed",
                sweep.left_handed,
                send(MotionLook::LeftHanded),
                Some("Turn clockwise seen from the axis's tip"),
            );
            let flip = toggle(
                Icon::TkFlip,
                "Flip",
                state.flip,
                send(MotionLook::Flip),
                Some("Climb the other way along the axis"),
            );
            column![
                reference,
                value(MotionField::Pitch, "Pitch"),
                value(MotionField::Turns, "Turns"),
                left,
                flip,
            ]
            .spacing(10)
            .into()
        }
    };
    let operations = OperationKind::ALL.map(|kind| {
        tile(
            kind.icon(),
            kind.label(),
            sweep.operation == kind,
            send(MotionLook::Operation(kind)),
        )
    });
    let targets = bodies(
        sweep.operation,
        &sweep.targets,
        |body| send(MotionLook::Target(body)),
        state.hover,
    );
    column![
        profile,
        field("Along", tiles(paths)),
        path,
        field("Operation", tiles(operations)),
        targets,
    ]
    .spacing(10)
    .into()
}
