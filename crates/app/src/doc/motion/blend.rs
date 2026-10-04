//! The edges a blend picks, in the move's session: a chamfer's
//! ([`MotionKind::Chamfer`]), and a fillet's once there's one, which
//! takes the same session with its own size. They're picked as `refs`
//! has it (a click picks or takes out, named as of the feature, all on
//! one body, sorted as [`EdgeRef::order`] keeps them, lit as selected),
//! each with its tangent chain while the Tangent chain tick takes them
//! in. The session's bodies are the edges' body, never picked
//! themselves, so a body merged into another before the feature takes
//! the edges with it on to the holder.

use glam::DVec3;
use varde_document::{Document, EdgeRef};
use varde_expr::Unit;
use varde_view::{BlendEdge, BlendEdges, MotionKind, Pick, PickIndex, Picked};

use super::refs::Refs;
use super::{Doc, MotionSession, edge_radius};

/// A blend's edges as picked, and its Tangent chain tick.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BlendSetup {
    /// The edges, all on one body, in [`EdgeRef::order`].
    pub(crate) edges: Refs<EdgeRef>,
    /// The Tangent chain tick: on to begin with, as the plan has it.
    pub(crate) chains: bool,
}

impl Default for BlendSetup {
    fn default() -> Self {
        Self {
            edges: Refs::default(),
            chains: true,
        }
    }
}

impl BlendSetup {
    /// The edges `edges` (a stored feature's), tick `chains`, picked on
    /// no model shown.
    pub(super) fn of(edges: &[EdgeRef], chains: bool) -> Self {
        Self {
            edges: Refs::of(edges),
            chains,
        }
    }
}

impl MotionSession {
    /// Keeps the bodies the edges' body, which the session's own rules
    /// (gone, merged) follow.
    pub(super) fn blend_body(&mut self) {
        self.bodies = self.blend.edges.body().into_iter().collect();
    }

    /// What's still to be done before it can be committed, the words for
    /// the status bar: the edges.
    pub(super) fn blend_need(&self) -> Option<&'static str> {
        let words = match self.kind {
            MotionKind::Chamfer => "pick the edges to chamfer",
            _ => "pick the edges",
        };
        self.blend.edges.refs.is_empty().then_some(words)
    }

    /// The words for its edges being gone, if they are, the UI mock's.
    pub(super) fn blend_gone(&self) -> Option<&'static str> {
        let edges = &self.blend.edges;
        (edges.gone && !edges.refs.is_empty()).then_some("A picked edge is gone")
    }

    /// Notes whether `document` no longer takes the edges at feature
    /// `index`, or no longer holds their body.
    pub(super) fn prune_blend(&mut self, document: &Document, index: usize) {
        let edges = &mut self.blend.edges;
        edges.gone = !edges.refs.is_empty()
            && (edges
                .body()
                .is_some_and(|body| document.body(body).is_none())
                || document.check_blend_edges(index, &edges.refs).is_err());
    }
}

impl Doc {
    /// What lights under the cursor for `pick` while a blend's edges are
    /// picked: its edge, with its tangent chain while they take them in.
    pub(super) fn blend_hover(&self, pick: Pick) -> Vec<Picked> {
        let chains = self
            .motion
            .as_ref()
            .is_some_and(|session| session.blend.chains);
        match pick.target {
            Picked::Edge(edge) => (chain_of(self.feed.pick_index(), edge, chains).into_iter())
                .map(Picked::Edge)
                .collect(),
            target => vec![target],
        }
    }

    /// The edges of the blend being set up to light in the model shown.
    pub(super) fn blend_lit(&self) -> Vec<Picked> {
        match &self.motion {
            Some(session) if session.kind.blends() => self.refs_lit::<EdgeRef>(),
            _ => Vec::new(),
        }
    }

    /// The edges of the blend being set up as the panel lists them:
    /// "Edge 2" by its place, with what the model shown measures of it
    /// where it's found there (a straight edge's length, a circle's
    /// diameter, an arc's radius).
    pub(super) fn blend_edges(&self, session: &MotionSession) -> BlendEdges {
        let index = self.feed.pick_index();
        let length = Some(Unit::Length(self.editor.document().units()));
        let edges = (session.blend.edges.found(index.model()))
            .enumerate()
            .map(|(at, (edge, found))| {
                let (meta, round) =
                    found.map_or((None, false), |found| measured(index, found, &edge, length));
                BlendEdge {
                    edge,
                    name: format!("Edge {}", at + 1),
                    meta,
                    round,
                }
            })
            .collect();
        BlendEdges {
            edges,
            chains: session.blend.chains,
        }
    }
}

/// `edge` of `index`'s model, with the rest of its tangent chain if
/// `chains`.
pub(super) fn chain_of(index: &PickIndex, edge: u32, chains: bool) -> Vec<u32> {
    let chain = index.tangent_chain(edge);
    if chains && !chain.is_empty() {
        chain.to_vec()
    } else {
        vec![edge]
    }
}

/// What `index`'s model measures of `edge`, the edge `reference` names,
/// written with `unit`: a straight one's length, a closed round one's
/// diameter ("Ø16 mm", and whether it's such a rim) or an arc's radius
/// ("R8 mm"); nothing for another curve.
fn measured(
    index: &PickIndex,
    edge: u32,
    reference: &EdgeRef,
    unit: Option<Unit>,
) -> (Option<String>, bool) {
    if let Some([from, to]) = index.edge_ends(edge, &reference.faces) {
        return (
            Some(varde_expr::format(DVec3::distance(from, to), unit)),
            false,
        );
    }
    let Some(radius) = edge_radius(index, edge) else {
        return (None, false);
    };
    let closed = (index.picking().closed().get(edge as usize)).is_some_and(|&closed| closed);
    if closed {
        let diameter = varde_expr::format(2.0 * radius, unit);
        (Some(format!("Ø{diameter}")), true)
    } else {
        (
            Some(format!("R{}", varde_expr::format(radius, unit))),
            false,
        )
    }
}
