//! A scale being set up: its own parts of the move's panel, built in the
//! style of the UI mock's Move panel (the mock has no Scale panel, only
//! the tool in its Modify group): the Bodies, the Point it scales about,
//! then how it scales, as tiles (one factor, one per axis, or to an
//! edge's length), and that mode's fields: the factor, the factors along
//! X, Y and Z, or the edge (its length now beside it), the length it's
//! to have and, for a straight edge along a world axis, "Along its axis
//! only".

use glam::DVec3;
use iced::Element;
use iced::widget::column;

use super::{MotionField, MotionLook, MotionPick, MotionState};
use varde_document::Axis3;

use crate::icons::Icon;
use crate::operation_panel::{PanelHover, field, pick_field, picked_row, tile, tiles, toggle};
use crate::pick::{Pick, PickIndex};
use crate::{Look, Message};

/// How a scale scales, the panel's choices: by one factor along every
/// axis, by one per world axis, or by the factor that gives an edge a
/// typed length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScaleMode {
    #[default]
    Uniform,
    PerAxis,
    EdgeLength,
}

impl ScaleMode {
    /// The three in the panel's order.
    pub const ALL: [ScaleMode; 3] = [
        ScaleMode::Uniform,
        ScaleMode::PerAxis,
        ScaleMode::EdgeLength,
    ];

    /// Its tile's label: "Uniform", "Per axis", "Edge length".
    pub fn label(self) -> &'static str {
        match self {
            ScaleMode::Uniform => "Uniform",
            ScaleMode::PerAxis => "Per axis",
            ScaleMode::EdgeLength => "Edge length",
        }
    }

    /// Its tile's icon (not in the mock, drawn as the patterns' modes).
    fn icon(self) -> Icon {
        match self {
            ScaleMode::Uniform => Icon::ScUniform,
            ScaleMode::PerAxis => Icon::ScAxes,
            ScaleMode::EdgeLength => Icon::ScEdge,
        }
    }
}

/// What the panel and the viewport show of a scale being set up, beside
/// what every move's session has.
#[derive(Debug, Clone)]
pub struct ScaleView<'a> {
    pub mode: ScaleMode,
    /// The point it scales about, as named: "Origin", "Corner of Body 1".
    pub point: String,
    /// Where that point is, if known: where the preview found it, or
    /// where it was picked on the model shown. Drawn as a dot.
    pub at: Option<DVec3>,
    /// The edge an edge length names, as named ("Edge of Body 1"), if
    /// one is picked.
    pub edge: Option<String>,
    /// That edge's length now, as the features before the scale leave
    /// it, written in the design's units, if it's known.
    pub length: Option<String>,
    /// Whether "Along its axis only" is offered: the edge is straight and
    /// along a world axis, or the option is on (so it can be turned off).
    pub offered: bool,
    pub axis_only: bool,
    /// What the status bar says of it once it's whole: "Body 1 ×2".
    pub info: Option<String>,
    /// While the point is picked, the model shown and what of it the
    /// cursor is over: its snap points are drawn as the measure tool's.
    pub snaps: Option<(&'a PickIndex, Pick)>,
}

/// The panel's body for a scale, after its Bodies field `body`: the Point,
/// then Scale's tiles and the fields of the mode picked.
pub(super) fn body<'a>(
    state: &MotionState<'a>,
    body: Element<'a, Message>,
    value: impl Fn(MotionField, &'a str) -> Element<'a, Message>,
) -> Element<'a, Message> {
    let editable = state.editable;
    let send = |look: MotionLook| editable.then_some(Message::Look(Look::Motion(look)));
    let Some(scale) = &state.scale else {
        return body;
    };
    let picked = |pick: MotionPick, icon: Icon, name: Option<String>, meta, place: &str| {
        let on = state.picking == pick;
        let press = send(MotionLook::Picking(pick));
        // No cross: picking another replaces it, as a move's axis.
        let row = name.map(|name| {
            picked_row(
                icon,
                name,
                meta,
                None,
                press.clone(),
                PanelHover::Axis,
                state.hover,
            )
        });
        let place = (row.is_none() || on).then(|| place.to_owned());
        pick_field(row.into_iter().collect(), place, on, press)
    };
    let point = field(
        "Point",
        picked(
            MotionPick::Point,
            Icon::Point,
            Some(scale.point.clone()),
            None,
            "Click a corner, middle, centre or origin",
        ),
    );
    let modes = ScaleMode::ALL.iter().map(|&mode| {
        tile(
            mode.icon(),
            mode.label(),
            scale.mode == mode,
            send(MotionLook::ScaleMode(mode)),
        )
    });
    let fields: Element<'a, Message> = match scale.mode {
        ScaleMode::Uniform => value(MotionField::Factor, "Factor"),
        ScaleMode::PerAxis => {
            let axes = Axis3::ALL.map(|axis| value(MotionField::AxisFactor(axis), axis.name()));
            column(axes).spacing(8).into()
        }
        ScaleMode::EdgeLength => {
            let edge = field(
                "Edge",
                picked(
                    MotionPick::Edge,
                    Icon::SeAxis,
                    scale.edge.clone(),
                    scale.length.clone(),
                    "Click an edge of a body scaled",
                ),
            );
            let axis_only = scale.offered.then(|| {
                toggle(
                    Icon::SeAxis,
                    "Along its axis only",
                    scale.axis_only,
                    send(MotionLook::AxisOnly),
                    Some("Stretch the bodies along the axis the edge runs along only"),
                )
            });
            column![edge, value(MotionField::Length, "Length"), axis_only]
                .spacing(10)
                .into()
        }
    };
    column![body, point, super::section("Scale"), tiles(modes), fields]
        .spacing(10)
        .into()
}
