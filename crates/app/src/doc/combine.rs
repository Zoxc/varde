//! Setting up a combine: its session, started by `Look::StartCombine`
//! (`B`, the toolbar, the rail's Modify set) or by editing a combine,
//! picking its target and tools in the viewport (the body of what a click
//! is on) or in Objects, its operation and Keep tools, the preview
//! through the regeneration lane's drafts, and committing it as one undo
//! step or cancelling it, which leaves no trace.
//!
//! While it's set up the cursor picks the model as outside the sessions,
//! the preview included (only bodies are picked, by id, and a combine
//! draft makes none), and the highlight is its own: the target in the
//! selection's colour, the tools in the second colour, the body under
//! the cursor hovered. A body an earlier join or combine merged into
//! another is drawn as that one, so a click picks that one, as its row in
//! Objects does: the combine would fail naming the merged body ("Body 3
//! is in Body 2 now").

use std::sync::Arc;

use varde_document::{BodyId, BodyOp, Combine, Document, FeatureId, FeatureKind};
use varde_view::{
    CombineBody, CombineLook, CombinePick, CombineState, ModelHighlight, PanelHover, Pick, Picked,
};

use super::Doc;
use super::feed::Merges;

/// The combine being set up, while one is: [`Doc::combine`].
#[derive(Debug)]
pub(crate) struct CombineSession {
    /// The combine edited, or `None` for a new one.
    pub(crate) feature: Option<FeatureId>,
    pub(crate) target: Option<BodyId>,
    /// Sorted without repeats, as the document keeps them, the target
    /// not among them; at most [`varde_document::MAX_FEATURE_BODIES`].
    pub(crate) tools: Vec<BodyId>,
    /// What a click on a body picks.
    pub(crate) picking: CombinePick,
    pub(crate) op: BodyOp,
    pub(crate) keep_tools: bool,
    /// Built when what it's of changes, so the renderer uploads it only
    /// then.
    highlight: Arc<ModelHighlight>,
    /// What `highlight` was built of.
    built: Option<Built>,
    /// The panel's row the cursor is over, if any.
    pub(crate) hover: Option<PanelHover>,
}

/// What a combine's highlight is built of: the model, the body hovered and
/// whether its row in the panel is, the target and the tools.
type Built = (u64, Option<(BodyId, bool)>, Option<BodyId>, Vec<BodyId>);

impl CombineSession {
    /// A session setting up a new combine of `target` with `tools`, if
    /// given: a union using the tools up. Clicks pick the tools once
    /// there's a target.
    fn new(target: Option<BodyId>, mut tools: Vec<BodyId>) -> Self {
        tools.retain(|&tool| Some(tool) != target);
        tools.sort_unstable();
        tools.dedup();
        tools.truncate(varde_document::MAX_FEATURE_BODIES);
        Self {
            feature: None,
            picking: if target.is_some() {
                CombinePick::Tools
            } else {
                CombinePick::Target
            },
            target,
            tools,
            op: BodyOp::Union,
            keep_tools: false,
            highlight: Arc::default(),
            built: None,
            hover: None,
        }
    }

    /// A session editing the combine `feature`, with its values.
    fn editing(feature: FeatureId, combine: &Combine) -> Self {
        Self {
            feature: Some(feature),
            op: combine.op,
            keep_tools: combine.keep_tools,
            ..Self::new(Some(combine.target), combine.tools.clone())
        }
    }

    /// The combine as set up, if it's whole: a target and a tool.
    fn combine(&self) -> Option<Combine> {
        let target = self.target?;
        (!self.tools.is_empty()).then(|| Combine {
            target,
            tools: self.tools.clone(),
            op: self.op,
            keep_tools: self.keep_tools,
        })
    }

    /// Picks `body` as what clicks pick: the target (taking it out of the
    /// tools if it's one, and handing the clicks to the tools), or a tool,
    /// added, or taken out if it's one. The target isn't a tool; nor are
    /// tools added past the limit.
    fn pick(&mut self, body: BodyId) {
        match self.picking {
            CombinePick::Target => {
                self.tools.retain(|&tool| tool != body);
                self.target = Some(body);
                self.picking = CombinePick::Tools;
            }
            CombinePick::Tools if self.target == Some(body) => {}
            CombinePick::Tools => match self.tools.binary_search(&body) {
                Ok(at) => {
                    self.tools.remove(at);
                }
                Err(at) if self.tools.len() < varde_document::MAX_FEATURE_BODIES => {
                    self.tools.insert(at, body);
                }
                Err(_) => {}
            },
        }
    }

    /// Takes `body` out, the target or a tool. Without a target, clicks
    /// pick it next.
    fn drop(&mut self, body: BodyId) {
        if self.target == Some(body) {
            self.target = None;
            self.picking = CombinePick::Target;
        }
        self.tools.retain(|&tool| tool != body);
    }

    /// Lets go of the bodies `document` no longer holds, or that aren't
    /// made before the combine edited any more (undone, say): a combine
    /// can't name them.
    fn prune(&mut self, document: &Document) {
        let edited = self.feature;
        let keep = |body: BodyId| pickable(document, body, edited);
        if self.target.is_some_and(|target| !keep(target)) {
            self.target = None;
            self.picking = CombinePick::Target;
        }
        self.tools.retain(|&tool| keep(tool));
    }

    /// Moves the bodies `merges` (the merges before the combine) have
    /// merged into others on to the bodies holding them, as a click on
    /// one picks: one picked on a model that didn't show the merge yet
    /// (a join committed and not answered) would fail the combine ("Body
    /// 2 is in Body 1 now"). A tool landing on the target, or on another
    /// tool, is taken once. Whether anything moved.
    fn follow(&mut self, merges: &Merges) -> bool {
        let held = |body: BodyId| merges.holder(body).unwrap_or(body);
        let target = self.target.map(held);
        let mut tools: Vec<BodyId> = self.tools.iter().map(|&tool| held(tool)).collect();
        tools.retain(|&tool| Some(tool) != target);
        tools.sort_unstable();
        tools.dedup();
        let moved = target != self.target || tools != self.tools;
        self.target = target;
        self.tools = tools;
        moved
    }
}

/// Whether a combine of `document` edited from `edited` (a new one, where
/// new features go, if `None`) can name `body`: it's there, made by a
/// feature before the combine, as the document's check wants.
pub(crate) fn pickable(document: &Document, body: BodyId, edited: Option<FeatureId>) -> bool {
    let at = edited.map_or(Some(document.insert_at()), |id| document.feature_index(id));
    document.body(body).is_some_and(|made| {
        let maker = document.feature_index(made.created_by);
        maker.is_some_and(|maker| at.is_none_or(|at| maker < at))
    })
}

impl Doc {
    /// Starts setting up a new combine, in a document that can be changed
    /// and outside a sketch and the other operations, or cancels the one
    /// being set up. What's selected in the model gives it its bodies: the
    /// first selected's body is the target, the others' the tools.
    pub(crate) fn start_combine(&mut self) {
        if self.combine.take().is_some()
            || !self.editable()
            || self.sketch.is_some()
            || self.extrude.is_some()
            || self.revolve.is_some()
            || self.motion.is_some()
        {
            return;
        }
        self.picking_plane = None;
        let bodies = self.selected_bodies();
        let target = bodies.first().copied();
        let tools = bodies.get(1..).unwrap_or_default().to_vec();
        self.combine = Some(CombineSession::new(target, tools));
    }

    /// The bodies of what's selected in the model, in the order selected,
    /// each once, that a new combine, move or mirror can name: a body
    /// merged into another as the one holding it, which the model draws
    /// it as.
    pub(crate) fn selected_bodies(&self) -> Vec<BodyId> {
        let document = self.editor.document();
        let merged = self.feed.merged_before(document, None);
        let mut bodies: Vec<BodyId> = Vec::new();
        for item in self.pick.selection.items() {
            // A sketch's curve or point names no body.
            let Some(body) = item.body() else {
                continue;
            };
            let body = merged.holder(body).unwrap_or(body);
            if pickable(document, body, None) && !bodies.contains(&body) {
                bodies.push(body);
            }
        }
        bodies
    }

    /// Edits the combine feature `id`, if the document holds it, in a
    /// session with its values, outside a sketch, in a document that can
    /// be changed: a read-only one has no session. An extrude or revolve
    /// being set up is dropped.
    pub(crate) fn edit_combine(&mut self, id: FeatureId) {
        let document = self.editor.document();
        let Some(FeatureKind::Combine(combine)) = document.feature(id).map(|f| &f.kind) else {
            return;
        };
        if self.sketch.is_some() || !self.editable() {
            return;
        }
        self.picking_plane = None;
        self.extrude = None;
        self.revolve = None;
        self.motion = None;
        self.selected_feature = Some(id);
        self.combine = Some(CombineSession::editing(id, combine));
    }

    /// Takes `message`, changing the combine being set up.
    pub(crate) fn combine_look(&mut self, message: CombineLook) {
        let editable = self.editable();
        let Some(session) = &mut self.combine else {
            return;
        };
        match message {
            CombineLook::Cancel => self.combine = None,
            _ if !editable => {}
            CombineLook::Picking(picking) => session.picking = picking,
            CombineLook::Drop(body) => session.drop(body),
            CombineLook::Operation(op) => session.op = op,
            CombineLook::KeepTools => session.keep_tools = !session.keep_tools,
        }
    }

    /// Takes a click on the model while a combine is set up: picks the
    /// body of what it's on, see [`Doc::combine_body`]. A click on
    /// nothing, or on a model no longer shown, does nothing.
    pub(crate) fn combine_click(&mut self, pick: Option<Pick>) {
        let Some(pick) = pick.filter(|pick| pick.model == self.feed.model()) else {
            return;
        };
        if self.picks() {
            self.combine_body(pick.body);
        }
    }

    /// Whether the combine being set up has the body of `pick` in the
    /// role a click on it gives it, for the list of the model's overlaps:
    /// its target, or a tool while tools are picked (a click on a tool
    /// while the target is picked makes it the target).
    pub(crate) fn combine_has(&self, pick: Pick) -> bool {
        let Some(session) = &self.combine else {
            return false;
        };
        let merged = self
            .feed
            .merged_before(self.editor.document(), session.feature);
        let body = merged.holder(pick.body).unwrap_or(pick.body);
        session.target == Some(body)
            || (session.picking == CombinePick::Tools && session.tools.contains(&body))
    }

    /// Picks `body` for the combine being set up, from the viewport or its
    /// row in Objects, as the session says ([`CombineSession::pick`]): a
    /// body merged into another before the combine as the one holding it,
    /// which the model draws it as. Only bodies the combine can name, in
    /// a document that can be changed.
    pub(crate) fn combine_body(&mut self, body: BodyId) {
        if !self.editable() {
            return;
        }
        let document = self.editor.document();
        let Some(session) = &mut self.combine else {
            return;
        };
        let merged = self.feed.merged_before(document, session.feature);
        let body = merged.holder(body).unwrap_or(body);
        if pickable(document, body, session.feature) {
            session.pick(body);
        }
    }

    /// Whether the document has bodies to combine: two or more.
    pub(crate) fn combinable(&self) -> bool {
        self.editor.document().bodies().len() >= 2
    }

    /// Whether the combine being set up can be committed: the document
    /// can be changed, no sketch edits wait on the solver (they come
    /// first in the undo history), and it's whole and passes its own
    /// check; the session keeps its bodies ones the document has, made
    /// before it.
    pub(crate) fn combine_ready(&self) -> bool {
        self.combine.as_ref().is_some_and(|session| {
            self.editable()
                && !self.proposing()
                && session
                    .combine()
                    .is_some_and(|combine| combine.check_own().is_ok())
        })
    }

    /// Adds the combine being set up, or changes the one edited, as one
    /// undo step, and ends the session: if it's ready
    /// ([`Doc::combine_ready`]), its preview failed only if `accept`
    /// ([`Doc::commit_by`]), and the document takes it. Refused, the
    /// session stays, and why shows.
    pub(crate) fn commit_combine(&mut self, accept: bool) {
        if !self.commit_by(self.combine_ready(), accept) {
            return;
        }
        let Some(session) = &self.combine else {
            return;
        };
        let Some(combine) = session.combine() else {
            return;
        };
        if self.commit_feature(session.feature, combine.into()) {
            self.combine = None;
        }
    }

    /// Ends the combine session if the combine edited is gone, the
    /// document can't be changed any more, or it was replaced whole
    /// (`replaced`), as [`Doc::prune_extrude`] does the extrude's; and
    /// lets go of the bodies it can't name any more.
    pub(crate) fn prune_combine(&mut self, replaced: bool) {
        let editable = self.editable();
        let document = self.editor.document();
        let Some(session) = &mut self.combine else {
            return;
        };
        let edited = session.feature.is_none_or(|feature| {
            matches!(
                document.feature(feature).map(|f| &f.kind),
                Some(FeatureKind::Combine(_))
            )
        });
        if !(editable && !replaced && edited) {
            self.combine = None;
            return;
        }
        session.prune(document);
        // `sync` asks for the preview after.
        self.follow_merges();
    }

    /// Moves the combine's bodies on to the bodies holding them where
    /// the model shown has them merged before it
    /// ([`CombineSession::follow`]), after an edit and with each answer:
    /// whether they moved, and the preview is to be asked for again.
    pub(crate) fn follow_merges(&mut self) -> bool {
        let Some(session) = &self.combine else {
            return false;
        };
        let merges = (self.feed).merged_before(self.editor.document(), session.feature);
        (self.combine.as_mut()).is_some_and(|session| session.follow(&merges))
    }

    /// The combine being set up as the regeneration lane previews it, and
    /// the combine it edits, if it's whole.
    pub(crate) fn combine_draft(&self) -> Option<(Option<FeatureId>, FeatureKind)> {
        let session = self.combine.as_ref()?;
        Some((session.feature, session.combine()?.into()))
    }

    /// Rebuilds the combine's highlight if the model, the body hovered,
    /// the target or the tools changed since it was built: the target's
    /// faces as selected, the tools' in the second colour (a tool the
    /// preview uses up has none), the hovered body's hovered. A body whose
    /// row in the panel is hovered is lit so even if it's the target or a
    /// tool, which in the view keep their colour under the cursor.
    pub(crate) fn refresh_combine_highlight(&mut self) {
        if !self.picks() {
            return;
        }
        let panel = self.panel_hover().and_then(PanelHover::body);
        let hovered = match panel {
            Some(body) => Some((body, true)),
            None => self.pick.hover().map(|pick| (pick.body, false)),
        };
        let Some(session) = &mut self.combine else {
            return;
        };
        let key = (
            self.feed.model(),
            hovered,
            session.target,
            session.tools.clone(),
        );
        if session.built.as_ref() == Some(&key) {
            return;
        }
        let index = self.feed.pick_index();
        let faces = |body: Option<BodyId>| -> Vec<Picked> {
            body.map(|body| index.body_faces(body).map(Picked::Face).collect())
                .unwrap_or_default()
        };
        let lit = hovered.filter(|&(_, panel)| panel).map(|(body, _)| body);
        let target = faces(session.target.filter(|&target| Some(target) != lit));
        let tools: Vec<Picked> = (session.tools.iter())
            .filter(|&&tool| Some(tool) != lit)
            .flat_map(|&tool| faces(Some(tool)))
            .collect();
        // What's picked keeps its colour under the cursor in the view.
        let hover = match hovered {
            Some((body, true)) => faces(Some(body)),
            Some((body, false))
                if Some(body) != session.target && !session.tools.contains(&body) =>
            {
                faces(Some(body))
            }
            _ => Vec::new(),
        };
        session.highlight = Arc::new(index.highlight_with(&hover, &target, &tools));
        session.built = Some(key);
    }

    /// The combine's highlight, if one is set up and it's built for the
    /// model shown.
    pub(crate) fn combine_highlight(&self) -> Option<&Arc<ModelHighlight>> {
        let session = self.combine.as_ref()?;
        let current = (session.built.as_ref()).is_some_and(|built| built.0 == self.feed.model());
        Some(&session.highlight).filter(|highlight| current && !highlight.is_empty())
    }

    /// The combine being set up, for the view.
    pub(crate) fn combine_state(&self) -> Option<CombineState<'_>> {
        let session = self.combine.as_ref()?;
        let document = self.editor.document();
        let named = |body: BodyId| {
            Some(CombineBody {
                body,
                name: document.body(body)?.name.as_str(),
            })
        };
        let editing = session
            .feature
            .and_then(|feature| document.feature(feature))
            .map(|feature| feature.name.as_str());
        Some(CombineState {
            editing,
            target: session.target.and_then(named),
            tools: session
                .tools
                .iter()
                .filter_map(|&tool| named(tool))
                .collect(),
            picking: session.picking,
            op: session.op,
            keep_tools: session.keep_tools,
            enough: self.combinable(),
            error: self.feed.shown_draft_error(),
            show_error: self.draft_framed(),
            checking: self.proposals.slow(),
            ready: self.commit_by(self.combine_ready(), false),
            accept: self.offers_accept(self.combine_ready()),
            editable: self.editable(),
            hover: self.panel_hover(),
        })
    }
}

#[cfg(test)]
mod tests;
