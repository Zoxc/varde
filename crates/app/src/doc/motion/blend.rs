//! The edges a blend picks, in the move's session: a chamfer's
//! ([`MotionKind::Chamfer`]) and a fillet's ([`MotionKind::Fillet`]),
//! each with its own size. They're picked as `refs`
//! has it (a click picks or takes out, named as of the feature, all on
//! one body, sorted as [`EdgeRef::order`] keeps them, lit as selected),
//! a click taking in the edge's tangent chain, each of its edges picked,
//! while the Tangent chain tick is on (edges selected as the session
//! starts are taken as they are). Its faces (the session's, from the faces selected as it starts, or
//! the feature's) stand for the edges around them: an edge picked around
//! one is left out, lit and listed in the exclusions' purple. The
//! session's bodies are the edges' and faces' body, never picked
//! themselves, so a body merged into another before the feature takes
//! them with it on to the holder.

use glam::DVec3;
use varde_document::{Document, EdgeRef, FaceRef, blend_body, blend_excludes};
use varde_expr::Unit;
use varde_view::{
    BlendEdge, BlendEdges, BlendFace, MotionKind, MotionPick, Pick, PickIndex, Picked,
};

use super::refs::Refs;
use super::{Doc, MotionSession, edge_radius};

/// A blend's edges as picked, and its Tangent chain tick.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BlendSetup {
    /// The edges, all on one body, in [`EdgeRef::order`].
    pub(crate) edges: Refs<EdgeRef>,
    /// The Tangent chain tick: on to begin with, as the plan has it. A
    /// blend's says what a click picks; a sweep's is stored.
    pub(crate) chains: bool,
    /// A blend's stored `chains`, each edge taking in its tangent chain
    /// as it's regenerated: kept for a feature that had it, never set for
    /// a new one, whose clicks pick the chains' edges themselves.
    pub(crate) stored: bool,
}

impl Default for BlendSetup {
    fn default() -> Self {
        Self {
            edges: Refs::default(),
            chains: true,
            stored: false,
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
            stored: false,
        }
    }

    /// A blend's edges `edges` and stored `chains`, picked on no model
    /// shown, with the tick on.
    pub(super) fn blend_of(edges: &[EdgeRef], stored: bool) -> Self {
        Self {
            edges: Refs::of(edges),
            chains: true,
            stored,
        }
    }
}

impl Doc {
    /// Whether the model shown is the blend's body as of the feature
    /// rather than its preview: while the command modifier is held in a
    /// fillet or chamfer picking its edges, so those it blends away can
    /// be clicked (a face's edge to leave it out, an edge to take it out).
    /// A new one shows the document as it is, an edited one a move of
    /// nothing of its body ([`MotionSession::unmoved`]).
    pub(crate) fn blend_before(&self) -> bool {
        self.command_held
            && self.motion.as_ref().is_some_and(|session| {
                session.kind.blends() && session.picking == MotionPick::Edges
            })
    }

    /// Notes the command modifier `held` or let go, showing a blend's
    /// body before it or its preview again.
    pub(crate) fn hold_command(&mut self, held: bool) {
        if held != self.command_held {
            self.command_held = held;
            self.request_model();
        }
    }
}

impl MotionSession {
    /// Keeps the bodies the edges' and faces' body, which the session's
    /// own rules (gone, merged) follow.
    pub(super) fn blend_body(&mut self) {
        self.bodies = blend_body(&self.blend.edges.refs, &self.faces.refs)
            .into_iter()
            .collect();
    }

    /// What's still to be done before it can be committed, the words for
    /// the status bar: the edges.
    pub(super) fn blend_need(&self) -> Option<&'static str> {
        let words = match self.kind {
            MotionKind::Chamfer => "pick the edges to chamfer",
            MotionKind::Fillet => "pick the edges to fillet",
            _ => "pick the edges",
        };
        (self.blend.edges.refs.is_empty() && self.faces.refs.is_empty()).then_some(words)
    }

    /// The words for its edges or faces being gone, if they are, the UI
    /// mock's.
    pub(super) fn blend_gone(&self) -> Option<&'static str> {
        let edges = &self.blend.edges;
        let any = !edges.refs.is_empty() || !self.faces.refs.is_empty();
        (edges.gone && any).then_some("A picked edge is gone")
    }

    /// Notes whether `document` no longer takes the edges and faces at
    /// feature `index`, or no longer holds their body.
    pub(super) fn prune_blend(&mut self, document: &Document, index: usize) {
        let (edges, faces) = (&self.blend.edges.refs, &self.faces.refs);
        let body = blend_body(edges, faces);
        self.blend.edges.gone = body.is_some()
            && (body.is_some_and(|body| document.body(body).is_none())
                || document.check_blend_edges(index, edges, faces).is_err());
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

    /// The faces and edges of the blend being set up to light in the
    /// model shown: those it takes as selected, each edge with its
    /// tangent chain where it's stored taking them in, and the edges it leaves out
    /// (around a face it names) alone, in the exclusions' purple.
    pub(super) fn blend_lit_parts(&self) -> [Vec<Picked>; 2] {
        let Some(session) = self.motion.as_ref().filter(|s| s.kind.blends()) else {
            return [Vec::new(), Vec::new()];
        };
        let index = self.feed.pick_index();
        let model = index.model();
        let faces = &session.faces.refs;
        let mut taken: Vec<Picked> = (session.faces.found(model))
            .filter_map(|(_, at)| at.map(Picked::Face))
            .collect();
        let mut out: Vec<Picked> = Vec::new();
        for (edge, at) in session.blend.edges.found(model) {
            let Some(at) = at else { continue };
            if blend_excludes(&edge, faces) {
                out.push(Picked::Edge(at));
            } else {
                let chain = chain_of(index, at, session.blend.stored);
                taken.extend(chain.into_iter().map(Picked::Edge));
            }
        }
        for lit in [&mut taken, &mut out] {
            lit.sort_unstable();
            lit.dedup();
        }
        [taken, out]
    }

    /// What of the blend being set up lights as selected
    /// ([`Doc::blend_lit_parts`]'s first).
    #[cfg(test)]
    pub(super) fn blend_lit(&self) -> Vec<Picked> {
        let [taken, _] = self.blend_lit_parts();
        taken
    }

    /// The faces and edges of the blend being set up as the panel lists
    /// them: "Face 2" and "Edge 2" by their places, each edge with what
    /// the model shown measures of it where it's found there (a straight
    /// edge's length, a circle's diameter, an arc's radius) and whether
    /// it's left out. A sweep's path has no faces.
    pub(super) fn blend_edges(&self, session: &MotionSession) -> BlendEdges {
        let index = self.feed.pick_index();
        let length = Some(Unit::Length(self.editor.document().units()));
        let face_refs: &[FaceRef] = if session.kind.blends() {
            &session.faces.refs
        } else {
            &[]
        };
        let faces = (face_refs.iter().enumerate())
            .map(|(at, &face)| BlendFace {
                face,
                name: format!("Face {}", at + 1),
            })
            .collect();
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
                    excluded: blend_excludes(&edge, face_refs),
                }
            })
            .collect();
        BlendEdges {
            faces,
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
