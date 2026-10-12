//! What the features picked on sketches (a split's regions and line, a
//! sweep's profile and path, a loft's sections and rails) share: which
//! sketches the feature at its place takes, the regions offered, the
//! chains of curves picked, and how they're named.

use varde_document::{BodyId, CurveChain, Document, FeatureId};
use varde_sketch::{Id, Sketch};
use varde_view::Candidate;

use super::{Doc, MotionSession};
use crate::doc::OperationSlot;
use crate::doc::regions::{BodyTargets, RegionPick};
use crate::doc::sketch_of;

impl MotionSession {
    /// The feature's place in `document`: the edited one's, or where a
    /// new one goes.
    pub(super) fn index_in(&self, document: &Document) -> usize {
        (self.feature)
            .and_then(|id| document.feature_index(id))
            .unwrap_or(document.insert_at())
    }

    /// Whether `sketch` is a sketch of `document` the feature at its
    /// place (a split's, a sweep's, a loft's) can take: before the
    /// feature edited.
    pub(super) fn takes_sketch(&self, document: &Document, sketch: FeatureId) -> bool {
        let index = self.index_in(document);
        (document.features()[..index].iter())
            .any(|feature| feature.id == sketch && sketch_of(document, feature.id).is_some())
    }
}

impl Doc {
    /// The sketches whose regions `regions` offers for the feature
    /// `session` sets up: those placed, before it.
    pub(super) fn region_candidates<'s>(
        &'s self,
        session: &MotionSession,
        regions: &'s RegionPick,
    ) -> Vec<Candidate<'s>> {
        let document = self.editor.document();
        (regions.candidates(|id| self.placement(id)).into_iter())
            .filter(|candidate| session.takes_sketch(document, candidate.feature))
            .collect()
    }

    /// Takes the body `body` out of what the feature being set up joins,
    /// cuts or intersects, its targets as `targets` picks them from the
    /// session, or puts it back.
    pub(super) fn toggle_target(
        &mut self,
        targets: impl FnOnce(&mut MotionSession) -> &mut BodyTargets,
        body: BodyId,
    ) {
        let revision = self.feed.revision();
        let document = self.editor.document();
        let Some(session) = self.mode.motion_mut() else {
            return;
        };
        let feature = session.feature;
        targets(session).toggle(body, feature, document, revision);
    }
}

/// The name of the sketch `sketch`, as a panel shows it.
pub(super) fn sketch_name(document: &Document, sketch: FeatureId) -> String {
    (document.feature(sketch)).map_or_else(|| "A sketch".to_owned(), |f| f.name.clone())
}

/// Where in `chains` the one of `sketch`'s holding `curve` is, if one
/// does: a click on it takes it out.
pub(super) fn chain_at(chains: &[CurveChain], sketch: FeatureId, curve: Id) -> Option<usize> {
    (chains.iter())
        .position(|chain| chain.sketch == sketch && chain.curves.binary_search(&curve).is_ok())
}

/// The chain of curves `curve` is in, of `drawn`, the sketch `sketch`'s
/// ([`varde_sketch::Sketch::chain_of`]), sorted: a sweep's path's part or
/// a loft's rail.
pub(super) fn chain_through(drawn: &Sketch, sketch: FeatureId, curve: Id) -> CurveChain {
    let mut curves = drawn.chain_of(curve);
    curves.sort_unstable();
    curves.dedup();
    CurveChain { sketch, curves }
}

/// Whether a chain of `chains` names what `document` no longer takes at
/// feature `index`: a sketch not before it, or a curve its sketch hasn't.
pub(super) fn chains_gone(document: &Document, index: usize, chains: &[CurveChain]) -> bool {
    let features = document.features();
    let before = |sketch: FeatureId| {
        (features[..index.min(features.len())].iter()).any(|feature| feature.id == sketch)
    };
    (chains.iter()).any(|chain| {
        !before(chain.sketch)
            || sketch_of(document, chain.sketch).is_none_or(|sketch| {
                (chain.curves.iter()).any(|&curve| sketch.curve(curve).is_none())
            })
    })
}

/// "1 curve", "3 curves": `n` of `one`, or of `many` if not one.
pub(super) fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}
