//! Picking the model shown with the cursor, outside sketches and
//! sessions: what's hovered, and the highlight drawn for it. The pick
//! index is the feed's, built the first time it's needed for a model
//! (see [`MeshFeed::pick_index`](super::feed::MeshFeed::pick_index)).

use std::sync::Arc;

use varde_render::{Emphasis, Highlight};
use varde_view::{ModelPicking, Pick};

use super::Doc;

/// What's hovered in the model shown, and its highlight.
#[derive(Debug, Default)]
pub(crate) struct Hover {
    /// Of the model [`Pick::model`] names.
    pick: Option<Pick>,
    /// Built when `pick` changes, so the renderer uploads it only then.
    highlight: Arc<Highlight>,
}

impl Hover {
    /// The face or edge hovered, if any.
    pub(crate) fn pick(&self) -> Option<Pick> {
        self.pick
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
        if pick.map(|pick| pick.target) == self.hover.pick.map(|pick| pick.target) {
            self.hover.pick = pick;
            return;
        }
        let highlight = match pick {
            Some(pick) => (self.feed.pick_index()).highlight([(pick.target, Emphasis::Hovered)]),
            None => Highlight::default(),
        };
        self.hover = Hover {
            pick,
            highlight: Arc::new(highlight),
        };
    }

    /// Drops what's hovered once the model it's of isn't shown, or the
    /// cursor doesn't pick.
    pub(crate) fn prune_hover(&mut self) {
        let stale = |pick: Pick| pick.model != self.feed.model() || !self.picks();
        if self.hover.pick.is_some_and(stale) {
            self.hover = Hover::default();
        }
    }

    /// Picking for the viewport, if the cursor picks the model.
    pub(crate) fn model_picking(&self) -> Option<ModelPicking<'_>> {
        self.picks().then(|| ModelPicking {
            index: self.feed.pick_index(),
            hovered: self.hover.pick().map(|pick| pick.target),
        })
    }

    /// What the viewport draws over the model, if anything.
    pub(crate) fn highlight(&self) -> Option<&Arc<Highlight>> {
        Some(&self.hover.highlight).filter(|highlight| !highlight.is_empty())
    }
}

#[cfg(test)]
mod tests;
