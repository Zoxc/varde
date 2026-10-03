//! The failures' geometry the viewport shows, and framing the camera on
//! the draft's and going back: the draft's while an operation is set up and its preview fails,
//! a failed feature's while its Timeline row is hovered or selected or
//! its panel is open, nothing otherwise, so a model with an old failure
//! isn't covered in red. While a sketch is edited, the curves the
//! failures of the features using it name are marked in it instead, and
//! their points shown.

use std::collections::BTreeSet;
use std::sync::Arc;

use varde_document::{FeatureId, Id};
use varde_kernel::Aabb;
use varde_regen::ErrorGeometry;
use varde_view::{Framing, ShownError, ShownErrors};

use super::Doc;
use super::camera::FRAME_MARGIN;

/// The least height, in millimetres, the view is framed to: a failure at
/// one point, or a tiny one, is shown with what's around it rather than
/// zoomed into nothing.
const MIN_FRAME_HEIGHT: f32 = 1.0;

impl Doc {
    /// The failures' geometry the viewport draws, rebuilt only when what
    /// it shows changes (see [`Doc::refresh_errors`]).
    pub(crate) fn shown_errors(&self) -> &Arc<ShownErrors> {
        &self.errors
    }

    /// The geometry of the failures to show, all of the model shown
    /// (its answer brought them), in order, none twice:
    ///
    /// - the draft's, while an operation is set up and the newest answer
    ///   for its draft as it is fails with geometry, as the panel says;
    /// - each failed feature's whose Timeline row is hovered (outside a
    ///   sketch, where the Timeline doesn't show) or selected, or whose
    ///   panel is open: the edited feature's only when the draft has none,
    ///   which is where the edit is now, and only until a draft of it is
    ///   shown, whose failure the model shown has for it then: the
    ///   draft's, above, or while a changed draft is on its way the one
    ///   before's, which the panel doesn't show either;
    /// - while a sketch is edited, each failed feature's using it whose
    ///   failure names its curves, as those curves are marked in it.
    ///
    /// A failure naming the edited sketch's curves is drawn without its
    /// curves, which the sketch marks itself in its plane (see
    /// [`Doc::failing_curves`]): the 3D copy would draw them again, and
    /// late while a drag moves them. Its points (where a profile touches
    /// itself, an open gap's ends) and patches still are, and one with
    /// nothing else isn't shown.
    fn wanted_errors(&self) -> Vec<ShownError> {
        let draft = self
            .operating()
            .then(|| self.feed.draft_geometry())
            .flatten();
        let edited = (self.extrude.as_ref().and_then(|session| session.feature))
            .or_else(|| self.revolve.as_ref().and_then(|session| session.feature))
            .or_else(|| self.combine.as_ref().and_then(|session| session.feature))
            .or_else(|| self.motion.as_ref().and_then(|session| session.feature));
        // Once a draft of it is shown, the edited feature's failure in the
        // model shown is a draft's: the newest one's, as the draft's
        // above, or one before it, whose panel error is gone too.
        let redrafted = edited.filter(|_| self.feed.draft_shown());
        let edited = edited.filter(|_| draft.is_none());
        let hovered = self.hovered_feature.filter(|_| self.sketch.is_none());
        let sketched = (self.feed.failed_features().iter())
            .map(|failed| failed.feature)
            .filter(|&feature| self.uses_edited_sketch(feature));
        let mut wanted: Vec<ShownError> =
            draft.cloned().map(ShownError::whole).into_iter().collect();
        let features = [hovered, self.selected_feature, edited]
            .into_iter()
            .flatten();
        for feature in features.chain(sketched) {
            if Some(feature) == redrafted {
                continue;
            }
            let Some(geometry) = self.failure_geometry(feature) else {
                continue;
            };
            if wanted
                .iter()
                .any(|shown| Arc::ptr_eq(&shown.geometry, geometry))
            {
                continue;
            }
            let lines = !(self.uses_edited_sketch(feature) && !geometry.sketch_curves().is_empty());
            let rest = !geometry.points().is_empty() || !geometry.mesh().indices().is_empty();
            if lines || rest {
                wanted.push(ShownError {
                    geometry: geometry.clone(),
                    lines,
                });
            }
        }
        wanted
    }

    /// The geometry of `feature`'s failure, as the model shown found it,
    /// if it failed and has some.
    fn failure_geometry(&self, feature: FeatureId) -> Option<&Arc<ErrorGeometry>> {
        (self.feed.failed_features().iter())
            .find(|failed| failed.feature == feature)?
            .geometry
            .as_ref()
    }

    /// Whether `feature` takes its regions from the sketch being edited.
    fn uses_edited_sketch(&self, feature: FeatureId) -> bool {
        let edited = self.sketch.as_ref().map(|session| session.feature);
        let document = self.editor.document();
        edited.is_some()
            && document
                .feature(feature)
                .and_then(|feature| feature.kind.sketch())
                == edited
    }

    /// The curves of the sketch being edited that the failures of the
    /// features using it name, as the model shown found them: every
    /// failed one, not only one hovered or selected, since the sketch is
    /// where they're mended. Only curves the sketch as shown holds: one
    /// deleted since isn't found. No draft fails meanwhile: editing a
    /// sketch ends the operation being set up.
    fn failing_curves(&self) -> BTreeSet<Id> {
        let named: BTreeSet<u64> = (self.feed.failed_features().iter())
            .filter(|failed| self.uses_edited_sketch(failed.feature))
            .filter_map(|failed| failed.geometry.as_ref())
            .flat_map(|geometry| geometry.sketch_curves().iter().copied())
            .collect();
        if named.is_empty() {
            return BTreeSet::new();
        }
        let curves = self
            .shown_sketch()
            .into_iter()
            .flat_map(|sketch| &sketch.curves);
        curves
            .map(|entry| entry.id)
            .filter(|id| named.contains(&u64::from(id.get())))
            .collect()
    }

    /// Picks the failures shown again, making what the viewport draws
    /// of them anew only if they're others than before: the same `Arc`s,
    /// as the regeneration side hands an unchanged failure back, keep
    /// what's drawn, so the renderer uploads nothing again. Picks the
    /// curves the sketch being edited marks as failing again too, and
    /// forgets the view Go back would turn to once the draft has no
    /// failure to frame.
    pub(super) fn refresh_errors(&mut self) {
        if !self.operating() || self.draft_bounds().is_none() {
            self.before_show = None;
        }
        let wanted = self.wanted_errors();
        if !self.errors.shows(&wanted) {
            self.errors = Arc::new(ShownErrors::new(wanted));
        }
        let failing = self.failing_curves();
        if let Some(session) = &mut self.sketch {
            session.failing = failing;
        }
    }

    /// The box of the geometry of the draft's failure, as the panel shows
    /// it, if it has one.
    fn draft_bounds(&self) -> Option<Aabb> {
        (self.feed.draft_geometry()).and_then(|geometry| geometry.bounds())
    }

    /// The button beside the panel's error, if the draft's failure has
    /// geometry with a box: Show, framing it, or Go back once it has.
    pub(crate) fn draft_framed(&self) -> Option<Framing> {
        self.draft_bounds().map(|_| match self.before_show {
            Some(_) => Framing::GoBack,
            None => Framing::Show,
        })
    }

    /// Frames the camera on the box of the draft's failure, if it has
    /// geometry with one, as the model shown found it, remembering the
    /// view for Go back. The camera turns to it as Home does, keeping its
    /// direction, and orbits its middle from then on.
    pub(super) fn show_failure(&mut self) {
        let Some(bounds) = self.draft_bounds() else {
            return;
        };
        if self.before_show.is_none() {
            let camera = (self.animation.as_ref()).map_or(self.camera, |animation| animation.to);
            self.before_show = Some((camera, self.pivot));
        }
        self.frame(bounds);
    }

    /// Turns the camera back to the view it had before Show, orbiting
    /// what it did then.
    pub(super) fn back_from_failure(&mut self) {
        if let Some((camera, pivot)) = self.before_show.take() {
            self.pivot = pivot;
            self.animate_camera(camera);
        }
    }

    /// Turns the camera to show `bounds` whole in the middle of the view,
    /// from where it looks.
    fn frame(&mut self, bounds: Aabb) {
        let mut to = (self.animation.as_ref()).map_or(self.camera, |animation| animation.to);
        to.set_target(bounds.center());
        let diagonal = (bounds.max - bounds.min).length();
        to.set_view_height((diagonal * FRAME_MARGIN).max(MIN_FRAME_HEIGHT));
        self.pivot = None;
        self.animate_camera(to);
    }
}

#[cfg(test)]
pub(crate) mod tests;
