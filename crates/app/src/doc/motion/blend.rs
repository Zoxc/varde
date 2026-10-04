//! The edges a blend picks, in the move's session: a chamfer's
//! ([`MotionKind::Chamfer`]), and a
//! fillet's once there's one, which takes the same session with its own
//! size. Each click on an edge of
//! the model shown picks it or, picked already, takes it out; the
//! edges are named as of the feature ([`Naming`](varde_view::Naming)),
//! on the body holding them there, all on one body (once one is
//! picked, the edges of other bodies don't light under the cursor and
//! are refused), kept sorted as the feature stores them
//! ([`EdgeRef::order`]), and lit as selected, found again by their names
//! on each model shown. The session's bodies are the edges' body, never
//! picked themselves, so a body merged into another before the feature
//! takes the edges with it on to the holder.

use std::borrow::Cow;

use glam::DVec3;
use varde_document::{BodyId, Document, EdgeRef, MAX_BLEND_EDGES};
use varde_expr::Unit;
use varde_view::{BlendEdge, BlendEdges, MotionKind, Pick, PickIndex, Picked, Unnamed};

use super::{Doc, MotionSession, OUT_OF_DATE, edge_radius, unnamed};
use crate::doc::feed::Merges;

/// A blend's edges as picked, and where they are on a model shown.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BlendSetup {
    /// At most [`MAX_BLEND_EDGES`], all on one body, in
    /// [`EdgeRef::order`] without repeats, as the feature stores them.
    pub(crate) edges: Vec<EdgeRef>,
    /// Each edge's place on a model shown, by `edges`' order: where it
    /// was picked, or found again by its names on one shown since
    /// ([`BlendSetup::follow`]).
    marks: Vec<Mark>,
    /// The Tangent chain tick: on to begin with, as the plan has it.
    pub(crate) chains: bool,
    /// Whether the edges are gone: the document no longer takes them at
    /// the feature's place (an undo took their body or a face's maker
    /// away), or no longer holds their body. Kept, said to be gone, until
    /// they're taken out or back.
    gone: bool,
}

impl Default for BlendSetup {
    fn default() -> Self {
        Self {
            edges: Vec::new(),
            marks: Vec::new(),
            chains: true,
            gone: false,
        }
    }
}

/// Where an edge is on the model `model`: the mesh's edge, if it's
/// there.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Mark {
    model: u64,
    edge: Option<u32>,
}

impl BlendSetup {
    /// The edges `edges` (a stored feature's), tick `chains`, picked on
    /// no model shown.
    pub(super) fn of(edges: &[EdgeRef], chains: bool) -> Self {
        Self {
            edges: edges.to_vec(),
            marks: vec![
                Mark {
                    model: 0,
                    edge: None
                };
                edges.len()
            ],
            chains,
            gone: false,
        }
    }

    /// The body its edges are on, if it has any.
    pub(super) fn body(&self) -> Option<BodyId> {
        self.edges.first().map(|edge| edge.body)
    }

    /// The edge picked that is one of `edges` of the model `model`, if
    /// one is: where a click on them takes it out.
    fn picked(&self, model: u64, edges: &[u32]) -> Option<usize> {
        (self.marks.iter()).position(|mark| {
            mark.model == model && mark.edge.is_some_and(|edge| edges.contains(&edge))
        })
    }

    /// Adds `edge`, picked as `target` of the model `model`, in its
    /// place; not past the limit, nor again.
    fn add(
        &mut self,
        kind: MotionKind,
        edge: EdgeRef,
        model: u64,
        target: u32,
    ) -> Result<(), Cow<'static, str>> {
        let at = match self.edges.binary_search_by(|picked| picked.order(&edge)) {
            Ok(_) => return Ok(()),
            Err(at) => at,
        };
        if self.edges.len() >= MAX_BLEND_EDGES {
            let noun = noun(kind);
            return Err(format!("A {noun} takes at most {MAX_BLEND_EDGES} edges").into());
        }
        self.edges.insert(at, edge);
        let mark = Mark {
            model,
            edge: Some(target),
        };
        self.marks.insert(at, mark);
        Ok(())
    }

    /// Takes out the edge at `at`.
    fn remove(&mut self, at: usize) {
        self.edges.remove(at);
        self.marks.remove(at);
        if self.edges.is_empty() {
            self.gone = false;
        }
    }

    /// Takes `edge` out, if it's picked.
    pub(super) fn drop_edge(&mut self, edge: &EdgeRef) {
        if let Some(at) = self.edges.iter().position(|picked| picked == edge) {
            self.remove(at);
        }
    }

    /// Finds the edges again on `index`'s model where they aren't marked
    /// on it yet, by their names on the body `shown` says draws theirs
    /// there.
    fn follow(&mut self, index: &PickIndex, shown: impl Fn(BodyId) -> BodyId) {
        let model = index.model();
        for (edge, mark) in self.edges.iter().zip(&mut self.marks) {
            if mark.model != model {
                *mark = Mark {
                    model,
                    edge: index.find_edge(shown(edge.body), edge.faces, edge.near),
                };
            }
        }
    }

    /// The edges picked lit on `index`'s model, those marked there, with
    /// their tangent chains while they take them in.
    fn lit(&self, index: &PickIndex) -> Vec<Picked> {
        let model = index.model();
        let mut lit: Vec<u32> = (self.marks.iter())
            .filter(|mark| mark.model == model)
            .filter_map(|mark| mark.edge)
            .flat_map(|edge| chain_of(index, edge, self.chains))
            .collect();
        lit.sort_unstable();
        lit.dedup();
        lit.into_iter().map(Picked::Edge).collect()
    }

    /// Moves the edges on bodies `merges` has merged into others on to
    /// the bodies holding them, keeping their order: whether any moved.
    fn follow_merges(&mut self, merges: &Merges) -> bool {
        let mut moved = false;
        for edge in &mut self.edges {
            if let Some(holder) = merges.holder(edge.body) {
                edge.body = holder;
                moved = true;
            }
        }
        if moved {
            let mut pairs: Vec<(EdgeRef, Mark)> = (self.edges.iter().copied())
                .zip(self.marks.iter().copied())
                .collect();
            pairs.sort_by(|a, b| a.0.order(&b.0));
            pairs.dedup_by(|a, b| a.0.order(&b.0).is_eq());
            (self.edges, self.marks) = pairs.into_iter().unzip();
        }
        moved
    }
}

impl MotionSession {
    /// Keeps the bodies the edges' body, which the session's own rules
    /// (gone, merged) follow.
    pub(super) fn blend_body(&mut self) {
        self.bodies = self.blend.body().into_iter().collect();
    }

    /// What's still to be done before it can be committed, the words for
    /// the status bar: the edges.
    pub(super) fn blend_need(&self) -> Option<&'static str> {
        let words = match self.kind {
            MotionKind::Chamfer => "pick the edges to chamfer",
            _ => "pick the edges",
        };
        self.blend.edges.is_empty().then_some(words)
    }

    /// The words for its edges being gone, if they are, the UI mock's.
    pub(super) fn blend_gone(&self) -> Option<&'static str> {
        (self.blend.gone && !self.blend.edges.is_empty()).then_some("A picked edge is gone")
    }

    /// Notes whether `document` no longer takes the edges at feature
    /// `index`, or no longer holds their body.
    pub(super) fn prune_blend(&mut self, document: &Document, index: usize) {
        let edges = &self.blend.edges;
        self.blend.gone = !edges.is_empty()
            && (self
                .blend
                .body()
                .is_some_and(|body| document.body(body).is_none())
                || document.check_chamfer_edges(index, edges).is_err());
    }

    /// Moves the edges, and so the body, on to the bodies holding theirs
    /// where `merges` (the merges before the feature) has them merged:
    /// whether they moved.
    pub(super) fn follow_blend(&mut self, merges: &Merges) -> bool {
        let moved = self.blend.follow_merges(merges);
        self.blend_body();
        moved
    }
}

/// What a blend of `kind` is called in words: "chamfer".
fn noun(kind: MotionKind) -> String {
    kind.noun().to_lowercase()
}

impl Doc {
    /// Whether the model shown names edges as the session asks for them:
    /// of the document as it is, not of one replaced whole since, and
    /// with a draft of this session's run (which of its drafts doesn't
    /// change the edges before the feature), or for a new feature none
    /// (added at the end, it changes nothing before it): not with the
    /// draft of another session just ended, which may have moved or cut
    /// them. Picks wait for nothing else, so edges can be clicked one
    /// after another while the preview of the last comes. The edges the
    /// feature itself makes (on its preview, or as stored) are refused
    /// by their names ([`Naming::before`](varde_view::Naming::before)).
    /// A shell's faces are picked the same way (see `faces`).
    pub(super) fn blend_model_current(&self) -> bool {
        let new = self.motion.as_ref().is_some_and(|s| s.feature.is_none());
        self.feed.generation() == Some(self.editor.generation())
            && !self.feed.predates_replacement()
            && (self.feed.shows_draft_of_run() || (new && !self.feed.shows_draft()))
    }

    /// `pick` of the model shown as an edge of the blend being set up:
    /// any edge, named as of the feature on the body holding it there,
    /// which is made before it, and the edges' body once one is picked.
    /// Refused, why, if not.
    pub(super) fn blend_edge_of(&self, pick: Pick) -> Result<EdgeRef, Cow<'static, str>> {
        let session = self.motion.as_ref().ok_or("Nothing is set up")?;
        let kind = session.kind;
        let Picked::Edge(edge) = pick.target else {
            return Err(format!("Only an edge can be {}ed", noun(kind)).into());
        };
        let naming = self.motion_naming().ok_or("Nothing is set up")?;
        let index = self.feed.pick_index();
        let named = naming
            .edge_ref(index, edge, pick.at)
            .map_err(|why| unnamed(why, "edge", kind))?;
        let document = self.editor.document();
        let merged = self.feed.merged_before(document, session.feature);
        let held = merged.holder(named.body).unwrap_or(named.body);
        if let Some(body) = session.blend.body()
            && body != held
        {
            let name = (document.body(body)).map_or("one body", |body| body.name.as_str());
            return Err(format!(
                "A {}'s edges are all on one body: pick edges of {name}",
                noun(kind)
            )
            .into());
        }
        if !super::super::combine::pickable(document, held, session.feature) {
            return Err(unnamed(Unnamed::Later, "body", kind));
        }
        Ok(EdgeRef {
            body: held,
            ..named
        })
    }

    /// Whether a click on `pick` takes an edge out: it's one picked, or
    /// in the tangent chain of one while they take them in, which is lit
    /// with it.
    fn blend_picked(&self, pick: Pick) -> Option<usize> {
        let session = self.motion.as_ref()?;
        let Picked::Edge(edge) = pick.target else {
            return None;
        };
        let index = self.feed.pick_index();
        if pick.model != index.model() {
            return None;
        }
        let chain = chain_of(index, edge, session.blend.chains);
        session.blend.picked(pick.model, &chain)
    }

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

    /// Whether `pick` lights as the cursor's over it: an edge a click
    /// would pick or take out.
    pub(super) fn blend_takes(&self, pick: Pick) -> bool {
        self.blend_model_current()
            && (self.blend_picked(pick).is_some() || self.blend_edge_of(pick).is_ok())
    }

    /// Takes a click on `pick` while a blend's edges are picked: an edge
    /// picked already is taken out, another picked, or why it can't be.
    pub(super) fn blend_click(&mut self, pick: Pick) -> Result<(), Cow<'static, str>> {
        if !self.blend_model_current() {
            return Err(OUT_OF_DATE.into());
        }
        if let Some(at) = self.blend_picked(pick) {
            if let Some(session) = &mut self.motion {
                session.blend.remove(at);
                session.blend_body();
            }
            return Ok(());
        }
        let edge = self.blend_edge_of(pick)?;
        let Picked::Edge(target) = pick.target else {
            return Ok(());
        };
        let Some(session) = &mut self.motion else {
            return Ok(());
        };
        session.blend.add(session.kind, edge, pick.model, target)?;
        session.blend_body();
        Ok(())
    }

    /// Picks the edges selected in the model shown, those a click would
    /// pick, for the blend just started: as the UI mock's chamfer takes
    /// a hole's rims selected.
    pub(super) fn blend_selected(&mut self) {
        let index = self.feed.pick_index();
        let model = index.model();
        if self.pick.selection.model() != Some(model) {
            return;
        }
        let picks: Vec<Pick> = (self.pick.selection.items())
            .filter_map(|item| match *item {
                varde_view::Selected::Edge { body, faces, near } => {
                    let edge = index.find_edge(body, faces, near)?;
                    Some(Pick {
                        model,
                        target: Picked::Edge(edge),
                        body,
                        at: near,
                        snap: None,
                    })
                }
                _ => None,
            })
            .collect();
        for pick in picks {
            // One refused (on another body, made later) is left out.
            let _ = self.blend_click(pick);
        }
    }

    /// Finds the edges of the blend being set up again on the model
    /// shown, where they aren't marked yet, each on the body drawing its
    /// body there.
    pub(super) fn follow_blend_marks(&mut self) {
        let Some(session) = &mut self.motion else {
            return;
        };
        if !session.kind.blends() {
            return;
        }
        let merged = self.feed.merged_bodies();
        let shown = |body: BodyId| {
            (merged.iter())
                .find(|(consumed, _)| *consumed == body)
                .map_or(body, |&(_, holder)| holder)
        };
        session.blend.follow(self.feed.pick_index(), shown);
    }

    /// The edges of the blend being set up to light in the model shown.
    pub(super) fn blend_lit(&self) -> Vec<Picked> {
        match &self.motion {
            Some(session) if session.kind.blends() => session.blend.lit(self.feed.pick_index()),
            _ => Vec::new(),
        }
    }

    /// The edge whose row in the panel is hovered, if it's marked on the
    /// model shown.
    pub(super) fn blend_hovered(&self, at: usize) -> Option<Picked> {
        let session = self.motion.as_ref()?;
        let mark = session.blend.marks.get(at)?;
        (mark.model == self.feed.model())
            .then_some(mark.edge)
            .flatten()
            .map(Picked::Edge)
    }

    /// The edges of the blend being set up as the panel lists them:
    /// "Edge 2" by its place, with what the model shown measures of it
    /// where it's found there (a straight edge's length, a circle's
    /// diameter, an arc's radius).
    pub(super) fn blend_edges(&self, session: &MotionSession) -> BlendEdges {
        let index = self.feed.pick_index();
        let model = index.model();
        let length = Some(Unit::Length(self.editor.document().units()));
        let edges = (session.blend.edges.iter().zip(&session.blend.marks))
            .enumerate()
            .map(|(at, (&edge, mark))| {
                let found = (mark.model == model).then_some(mark.edge).flatten();
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
fn chain_of(index: &PickIndex, edge: u32, chains: bool) -> Vec<u32> {
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
