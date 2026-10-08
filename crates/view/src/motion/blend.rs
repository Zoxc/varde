//! The edges a blend picks, shared by the edge sessions (a chamfer's
//! and a fillet's): what the app hands the panel of
//! them, their field (each edge a row, "Edge 2" by its place in the
//! list, as regeneration's messages count them, with its length or
//! diameter beside it and a cross taking it out) and the Tangent chain
//! tick, which takes in the edges running on smoothly from each.

use iced::Element;
use varde_document::EdgeRef;

use super::{MotionLook, MotionPick, MotionState, PickedRow, picks_field};
use crate::icons::Icon;
use crate::operation_panel::{PanelHover, toggle};
use crate::{Look, Message};

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
}

/// The edges picked, sorted as the feature keeps them, and whether each
/// takes in its tangent chain.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BlendEdges {
    pub edges: Vec<BlendEdge>,
    pub chains: bool,
}

/// The panel's Edges field: a row per edge, and while picking, or with
/// none, where to click (`place`).
pub(super) fn edges_field<'a>(
    state: &MotionState<'a>,
    edges: &BlendEdges,
    place: &str,
) -> Element<'a, Message> {
    let rows = (edges.edges.iter().enumerate()).map(|(at, edge)| PickedRow {
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
    picks_field(state, MotionPick::Edges, "Edges", place, rows)
}

/// The Tangent chain tick, on to begin with: the model mock has it on
/// the fillet's panel only, the plan on both.
pub(super) fn chain_toggle<'a>(
    state: &MotionState<'a>,
    edges: &BlendEdges,
) -> Element<'a, Message> {
    let send = state
        .editable
        .then_some(Message::Look(Look::Motion(MotionLook::Chain)));
    toggle(
        Icon::TkChain,
        "Tangent chain",
        edges.chains,
        send,
        Some("Take in edges that run on smoothly"),
    )
}
