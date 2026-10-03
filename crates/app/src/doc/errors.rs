//! The failures' geometry the viewport shows, and framing the camera on
//! it: the draft's while an operation is set up and its preview fails,
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
use varde_view::{ShownError, ShownErrors};

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
    ///   which is where the edit is now;
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
            .filter(|_| draft.is_none());
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
    /// curves the sketch being edited marks as failing again too.
    pub(super) fn refresh_errors(&mut self) {
        let wanted = self.wanted_errors();
        if !self.errors.shows(&wanted) {
            self.errors = Arc::new(ShownErrors::new(wanted));
        }
        let failing = self.failing_curves();
        if let Some(session) = &mut self.sketch {
            session.failing = failing;
        }
    }

    /// Whether the draft's failure, as the panel shows it, has geometry
    /// with a box: a Show button beside the panel's error frames it.
    pub(crate) fn draft_framed(&self) -> bool {
        (self.feed.draft_geometry()).is_some_and(|geometry| geometry.bounds().is_some())
    }

    /// Frames the camera on the box of `feature`'s failure, or with
    /// `None` the draft's, if it has geometry with one, as the model
    /// shown found it. The camera turns to it as Home does, keeping its
    /// direction, and orbits its middle from then on.
    pub(super) fn show_failure(&mut self, feature: Option<FeatureId>) {
        let geometry = match feature {
            Some(feature) => self.failure_geometry(feature),
            None => self.feed.draft_geometry(),
        };
        if let Some(bounds) = geometry.and_then(|geometry| geometry.bounds()) {
            self.frame(bounds);
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
mod tests;
