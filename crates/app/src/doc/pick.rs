//! Picking the model shown with the cursor, outside sketches and
//! sessions: what's hovered, what's selected, and the highlight drawn for
//! them. The pick index is the feed's, built the first time it's needed
//! for a model (see [`MeshFeed::pick_index`](super::feed::MeshFeed::pick_index)).
//! The selection keeps faces and edges by name and finds them again in
//! each new model (see [`Selection`]); Objects shows the bodies selected
//! and selects them too.

use std::sync::Arc;

use varde_document::BodyId;
use varde_render::Highlight;
use varde_view::{ModelPicking, Pick, Picked, Selection};

use super::Doc;

/// What's hovered and selected in the model shown, and its highlight.
#[derive(Debug, Default)]
pub(crate) struct ModelPick {
    /// Of the model [`Pick::model`] names.
    hover: Option<Pick>,
    pub(crate) selection: Selection,
    /// Built when what it's of changes, so the renderer uploads it only
    /// then.
    highlight: Arc<Highlight>,
    /// What `highlight` was built of: the model, the target hovered and
    /// the selection.
    built: Option<(u64, Option<Picked>, Selection)>,
}

impl ModelPick {
    /// The face or edge hovered, if any.
    pub(crate) fn hover(&self) -> Option<Pick> {
        self.hover
    }
}

impl Doc {
    /// Whether the cursor picks the model: outside sketches and the
    /// extrude being set up, which pick what they need themselves.
    pub(crate) fn picks(&self) -> bool {
        self.sketch.is_none() && self.extrude.is_none()
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
        if !self.picks() || pick.is_some_and(|pick| pick.model != self.feed.model()) {
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
    pub(crate) fn click_body(&mut self, body: BodyId, add: bool) {
        if self.sketch.is_some() || self.editor.document().body(body).is_none() {
            return;
        }
        self.pick.selection.click_body(body, add);
        self.selected_feature = None;
        self.refresh_highlight();
    }

    /// Selects nothing in the model.
    pub(crate) fn clear_model_selection(&mut self) {
        self.pick.selection.clear();
        self.refresh_highlight();
    }

    /// Drops what's hovered once the model it's of isn't shown, or the
    /// cursor doesn't pick, finds what's selected again in a new model
    /// (dropping what isn't there and bodies the document doesn't hold)
    /// while the cursor picks, and rebuilds the highlight if what it's
    /// of changed.
    pub(crate) fn prune_picks(&mut self) {
        let stale = |pick: Pick| pick.model != self.feed.model() || !self.picks();
        if self.pick.hover.is_some_and(stale) {
            self.pick.hover = None;
        }
        if self.picks() && !self.pick.selection.is_empty() {
            let document = self.editor.document();
            let exists = |body| document.body(body).is_some();
            self.pick.selection.resolve(self.feed.pick_index(), exists);
        }
        self.refresh_highlight();
    }

    /// Rebuilds the highlight if the model, the target hovered or the
    /// selection changed since it was built. Nothing is drawn while the
    /// cursor doesn't pick, so it isn't built then.
    fn refresh_highlight(&mut self) {
        if !self.picks() {
            return;
        }
        let key = (
            self.feed.model(),
            self.pick.hover.map(|pick| pick.target),
            self.pick.selection.clone(),
        );
        if self.pick.built.as_ref() == Some(&key) {
            return;
        }
        let highlight = if self.pick.hover.is_none() && self.pick.selection.is_empty() {
            Highlight::default()
        } else {
            let index = self.feed.pick_index();
            self.pick.selection.highlight(index, self.pick.hover)
        };
        self.pick.highlight = Arc::new(highlight);
        self.pick.built = Some(key);
    }

    /// Picking for the viewport, if the cursor picks the model.
    pub(crate) fn model_picking(&self) -> Option<ModelPicking<'_>> {
        self.picks().then(|| ModelPicking {
            index: self.feed.pick_index(),
            hovered: self.pick.hover().map(|pick| pick.target),
            picks: self.pick.selection.mode().picks(),
        })
    }

    /// What the viewport draws over the model, if anything: nothing while
    /// the cursor doesn't pick it.
    pub(crate) fn highlight(&self) -> Option<&Arc<Highlight>> {
        Some(&self.pick.highlight).filter(|highlight| self.picks() && !highlight.is_empty())
    }
}

#[cfg(test)]
mod tests;
