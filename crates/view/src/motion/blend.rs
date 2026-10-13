//! The edges a blend picks, shared by the edge sessions (a chamfer's
//! and a fillet's): what the app hands the panel of
//! them, their field (each face it names a row, "Face 2", then each edge,
//! "Edge 2" by its place in the list, as regeneration's messages count
//! them, with its length or diameter beside it and a cross taking it
//! out; an edge around a face named, which the blend leaves out, in the
//! exclusions' purple) and the Tangent chain tick, which takes in the
//! edges running on smoothly from each.

use iced::Element;
use varde_document::{EdgeRef, FaceRef};

use super::{MotionLook, MotionPick, MotionState, PickedRow, picks_field};
use crate::Message;
use crate::icons::Icon;
use crate::operation_panel::{PanelHover, RowTone, toggle};

/// An edge picked, as its row shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct BlendEdge {
    /// The edge as the feature stores it, which its cross takes out.
    pub edge: EdgeRef,
    /// Its name: "Edge 2", its place in the list.
    pub name: String,
    /// Beside it, what the model shown measures of it, if it's found
    /// there: a straight edge's length, "40 mm", or a circle's diameter,
    /// "Ø16 mm".
    pub meta: Option<String>,
    /// Whether it's a round edge, a hole's or a boss's rim: its row has
    /// the rim's icon.
    pub round: bool,
    /// Whether it's around a face the blend names, so left out: its row
    /// in the exclusions' purple.
    pub excluded: bool,
}

/// A face a blend names, standing for the edges around it, as its row
/// shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct BlendFace {
    /// The face as the feature stores it, which its cross takes out.
    pub face: FaceRef,
    /// Its name: "Face 2", its place in the list.
    pub name: String,
}

/// The faces and edges picked, each sorted as the feature keeps them, and
/// whether each edge takes in its tangent chain.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BlendEdges {
    pub faces: Vec<BlendFace>,
    pub edges: Vec<BlendEdge>,
    pub chains: bool,
}

impl BlendEdge {
    /// The edge's row in a picks field, `at` its place in the list:
    /// shared by the blends' Edges field and the sweep's Path.
    pub(super) fn row(&self, at: usize) -> PickedRow {
        PickedRow {
            icon: if self.round {
                Icon::SeRim
            } else {
                Icon::SeEdge
            },
            name: self.name.clone(),
            meta: self.meta.clone(),
            drop: MotionLook::DropEdge(self.edge),
            hover: PanelHover::Edge(at),
            tone: if self.excluded {
                RowTone::Excluded
            } else {
                RowTone::Plain
            },
        }
    }
}

/// The panel's Edges field: a row per face, then per edge, and while
/// picking, or with none, where to click (`place`).
pub(super) fn edges_field<'a>(
    state: &MotionState<'a>,
    edges: &BlendEdges,
    place: &str,
) -> Element<'a, Message> {
    let faces = (edges.faces.iter().enumerate()).map(|(at, face)| PickedRow {
        icon: Icon::SeFace,
        name: face.name.clone(),
        meta: None,
        drop: MotionLook::DropFace(face.face),
        hover: PanelHover::Face(at),
        tone: RowTone::Plain,
    });
    let rows = (edges.edges.iter().enumerate()).map(|(at, edge)| edge.row(at));
    picks_field(
        state,
        MotionPick::Edges,
        "Edges",
        place,
        faces.chain(rows),
        0,
    )
}

/// The Tangent chain tick, on to begin with: the model mock has it on
/// the fillet's panel only, the plan on both.
pub(super) fn chain_toggle<'a>(
    state: &MotionState<'a>,
    edges: &BlendEdges,
) -> Element<'a, Message> {
    toggle(
        Icon::TkChain,
        "Tangent chain",
        edges.chains,
        state.send(MotionLook::Chain),
        Some("Take in edges that run on smoothly"),
    )
}
