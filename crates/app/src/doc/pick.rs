//! Picking the model shown with the cursor, outside sketches and
//! sessions: what's hovered, what's selected, and the highlight drawn for
//! them. The pick index is the feed's, built the first time it's needed
//! for a model (see [`MeshFeed::pick_index`](super::feed::MeshFeed::pick_index)).
//! The selection keeps faces and edges by name and finds them again in
//! each new model (see [`Selection`]); Objects shows the bodies selected
//! and selects them too.

use std::sync::Arc;

use varde_document::BodyId;
use varde_view::{ModelHighlight, ModelPicking, PanelHover, Pick, Picked, Picks, Selection};

use super::Doc;

/// What's hovered and selected in the model shown, and its highlight.
#[derive(Debug, Default)]
pub(crate) struct ModelPick {
    /// Of the model [`Pick::model`] names.
    hover: Option<Pick>,
    pub(crate) selection: Selection,
    /// Built when what it's of changes, so the renderer uploads it only
    /// then.
    highlight: Arc<ModelHighlight>,
    /// What `highlight` was built of: the model, the target hovered and
    /// the selection.
    built: Option<(u64, Option<Picked>, Selection)>,
    /// While an extrude or revolve is set up, the body whose row in its
    /// panel is hovered, lit in the model shown (its preview).
    panel_highlight: Arc<ModelHighlight>,
    /// What `panel_highlight` was built of: the model and the body.
    panel_built: Option<(u64, BodyId)>,
}

impl ModelPick {
    /// The face, edge or vertex hovered, if any.
    pub(crate) fn hover(&self) -> Option<Pick> {
        self.hover
    }
}

impl Doc {
    /// Whether the cursor picks the model: outside sketches and the
    /// extrude or revolve being set up, which pick what they need themselves, and
    /// not while a draft's preview is still shown after it, where what's
    /// selected would be looked for in a model that isn't the document's,
    /// nor while the model shown is of a document since replaced whole,
    /// whose bodies' ids may name others now. While a combine is set up
    /// it picks bodies, its preview's too: a combine's draft makes no
    /// body, so the bodies its model has are the document's.
    pub(crate) fn picks(&self) -> bool {
        let combining = self.combine.is_some();
        self.sketch.is_none()
            && (combining || (!self.operating() && !self.feed.shows_draft()))
            && !self.feed.predates_replacement()
    }

    /// The row of the operation's panel the cursor is over, if one is set
    /// up and the row is still there: a region still picked, a combine's
    /// body still named.
    pub(crate) fn panel_hover(&self) -> Option<PanelHover> {
        let region = |regions: &super::regions::RegionPick, hover: PanelHover| match hover {
            PanelHover::Region { sketch, region } => {
                regions.source == Some(sketch) && regions.picked.contains(&region)
            }
            _ => true,
        };
        if let Some(session) = &self.extrude {
            return session
                .hover
                .filter(|&hover| region(&session.regions, hover));
        }
        if let Some(session) = &self.revolve {
            return session
                .hover
                .filter(|&hover| region(&session.regions, hover));
        }
        let session = self.combine.as_ref()?;
        session.hover.filter(|&hover| match hover {
            PanelHover::Body(body) => session.target == Some(body) || session.tools.contains(&body),
            _ => false,
        })
    }

    /// Hovers the row `hover` of the operation's panel, or none: the
    /// viewport lights it up too.
    pub(crate) fn hover_panel(&mut self, hover: Option<PanelHover>) {
        if let Some(session) = &mut self.extrude {
            session.hover = hover;
        }
        if let Some(session) = &mut self.revolve {
            session.hover = hover;
        }
        if let Some(session) = &mut self.combine {
            session.hover = hover;
        }
        self.refresh_highlight();
    }

    /// Takes the cursor leaving the panel's row `left`: nothing's hovered,
    /// unless another row already is, entered before this one was left.
    pub(crate) fn leave_panel(&mut self, left: PanelHover) {
        let hovered = (self.extrude.as_ref().map(|session| session.hover))
            .or_else(|| self.revolve.as_ref().map(|session| session.hover))
            .or_else(|| self.combine.as_ref().map(|session| session.hover))
            .flatten();
        if hovered == Some(left) {
            self.hover_panel(None);
        }
    }

    /// Hovers `pick`, from the viewport as the cursor moves, or nothing:
    /// only the highlight changes. A pick of another model than the one
    /// shown, or while the cursor doesn't pick, is dropped.
    pub(crate) fn hover(&mut self, pick: Option<Pick>) {
        let pick = pick.filter(|pick| pick.model == self.feed.model() && self.picks());
        self.pick.hover = pick;
        self.refresh_highlight();
    }

    /// Takes a click on the model, on `pick` or on nothing, see
    /// [`Selection::click`]. Selecting in the model lets go of the
    /// feature selected in the Timeline.
    pub(crate) fn click_model(&mut self, pick: Option<Pick>, add: bool, double: bool) {
        // Picking a plane, a click on a flat face asks for a sketch on it
        // instead (`Edit::FacePicked`), and one elsewhere does nothing.
        if !self.picks()
            || self.picking_plane.is_some()
            || pick.is_some_and(|pick| pick.model != self.feed.model())
        {
            return;
        }
        let index = self.feed.pick_index();
        self.pick.selection.click(index, pick, add, double);
        if !add || !self.pick.selection.is_empty() {
            self.selected_feature = None;
        }
        self.refresh_highlight();
    }

    /// Takes a click on `body`'s row in Objects, see
    /// [`Selection::click_body`], letting go of the feature selected in
    /// the Timeline. Not in a sketch, where Objects' rows don't select.
    /// A body a join merged into another is drawn as that one, so
    /// selects it.
    pub(crate) fn click_body(&mut self, body: BodyId, add: bool) {
        if self.sketch.is_some() || self.editor.document().body(body).is_none() {
            return;
        }
        let body = (self.feed.merged_bodies().iter())
            .find(|(merged, _)| *merged == body)
            .map_or(body, |&(_, holder)| holder);
        self.pick.selection.click_body(body, add);
        self.selected_feature = None;
        self.refresh_highlight();
    }

    /// Selects nothing in the model.
    pub(crate) fn clear_model_selection(&mut self) {
        self.pick.selection.clear();
        self.refresh_highlight();
    }

    /// Forgets what's hovered and selected in the model: the document was
    /// replaced whole, and the bodies they name by id may be others now.
    pub(crate) fn forget_picks(&mut self) {
        self.pick.hover = None;
        self.pick.selection.clear();
        self.refresh_highlight();
    }

    /// Drops what's hovered once the model it's of isn't shown, or the
    /// cursor doesn't pick, finds what's selected again in a new model
    /// while the cursor picks (what isn't there, bodies the document
    /// doesn't hold or a join merged into another, stops being selected
    /// but is looked for in later models; a face or edge of a merged body
    /// in the body holding it), and rebuilds the highlight if what it's
    /// of changed.
    pub(crate) fn prune_picks(&mut self) {
        let stale = |pick: Pick| pick.model != self.feed.model() || !self.picks();
        if self.pick.hover.is_some_and(stale) {
            self.pick.hover = None;
        }
        // Not in a combine's preview, which isn't the document's model.
        if self.picks() && self.combine.is_none() && !self.pick.selection.holds_nothing() {
            let document = self.editor.document();
            let merged = self.feed.merged_bodies();
            let drawn = |body| {
                document.body(body)?;
                let holder = merged.iter().find(|(merged, _)| *merged == body);
                Some(holder.map_or(body, |&(_, holder)| holder))
            };
            self.pick.selection.resolve(self.feed.pick_index(), drawn);
        }
        self.refresh_highlight();
    }

    /// Rebuilds the highlight if the model, the target hovered or the
    /// selection changed since it was built. Nothing is drawn while the
    /// cursor doesn't pick, so it isn't built then.
    fn refresh_highlight(&mut self) {
        self.refresh_panel_highlight();
        if !self.picks() {
            return;
        }
        // The measure tool's own, leaving the selection's as it was, and
        // likewise the combine's.
        if self.measure.is_some() {
            self.refresh_measure_highlight();
            return;
        }
        if self.combine.is_some() {
            self.refresh_combine_highlight();
            return;
        }
        let hover = self.shown_hover();
        let key = (
            self.feed.model(),
            hover.map(|pick| pick.target),
            self.pick.selection.clone(),
        );
        if self.pick.built.as_ref() == Some(&key) {
            return;
        }
        let highlight = if hover.is_none() && self.pick.selection.is_empty() {
            ModelHighlight::default()
        } else {
            let index = self.feed.pick_index();
            self.pick.selection.highlight(index, hover)
        };
        self.pick.highlight = Arc::new(highlight);
        self.pick.built = Some(key);
    }

    /// Rebuilds the extrude's or revolve's highlight, the body whose row in
    /// its panel is hovered lit, if the model or the body changed since it
    /// was built. The preview keeps the bodies' ids, so the body is found
    /// in the model shown, preview or not.
    fn refresh_panel_highlight(&mut self) {
        let operating = self.extrude.is_some() || self.revolve.is_some();
        let body = (self.panel_hover())
            .filter(|_| operating)
            .and_then(PanelHover::body);
        let key = body.map(|body| (self.feed.model(), body));
        if self.pick.panel_built == key {
            return;
        }
        self.pick.panel_highlight = Arc::new(match body {
            Some(body) => {
                let index = self.feed.pick_index();
                let faces: Vec<Picked> = index.body_faces(body).map(Picked::Face).collect();
                index.highlight_with(&faces, &[], &[])
            }
            None => ModelHighlight::default(),
        });
        self.pick.panel_built = key;
    }

    /// What's hovered as it's highlighted: while picking a plane, only a
    /// face that can take the sketch, which a click puts it on (the
    /// status bar says why another isn't): flat, and for a sketch whose
    /// plane is changed, of a body made before it.
    fn shown_hover(&self) -> Option<Pick> {
        let picking = self.model_picking()?;
        (self.pick.hover).filter(|pick| picking.takes(pick.target))
    }

    /// Picking for the viewport, if the cursor picks the model: faces
    /// and edges and the snap points of what it's over while measuring,
    /// only faces while picking a plane or a combine's bodies (a click
    /// anywhere on a body picks it, whatever the selection's mode), else
    /// what the selection's mode takes.
    pub(crate) fn model_picking(&self) -> Option<ModelPicking<'_>> {
        let measuring = self.measure.is_some();
        self.picks().then(|| ModelPicking {
            index: self.feed.pick_index(),
            hovered: self.pick.hover().map(|pick| pick.target),
            hovered_snap: self.pick.hover().and_then(|pick| pick.snap),
            picks: if measuring {
                Picks::All
            } else if self.picking_plane.is_some() || self.combine.is_some() {
                Picks::Faces
            } else {
                self.pick.selection.mode().picks()
            },
            snaps: measuring,
            planes: (self.picking_plane.as_ref())
                .filter(|_| !measuring)
                .map(|picking| &picking.pick),
        })
    }

    /// What the viewport draws over the model, if anything: nothing while
    /// the cursor doesn't pick it.
    pub(crate) fn highlight(&self) -> Option<&Arc<ModelHighlight>> {
        // While measuring, the measure tool's, and not the selection's.
        if self.measure.is_some() {
            return self.measure_highlight().filter(|_| self.picks());
        }
        if self.combine.is_some() {
            return self.combine_highlight().filter(|_| self.picks());
        }
        // An extrude's or revolve's: the body hovered in its panel.
        if self.extrude.is_some() || self.revolve.is_some() {
            let current =
                (self.pick.panel_built).is_some_and(|(model, _)| model == self.feed.model());
            return current.then_some(&self.pick.panel_highlight);
        }
        // Only of the model shown: one built for an earlier model would be
        // drawn over another.
        let current = (self.pick.built.as_ref()).is_some_and(|built| built.0 == self.feed.model());
        Some(&self.pick.highlight)
            .filter(|highlight| self.picks() && current && !highlight.is_empty())
    }
}

#[cfg(test)]
mod tests;
