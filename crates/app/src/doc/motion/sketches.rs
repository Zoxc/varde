//! What the features picked on sketches (a split's regions and line, a
//! sweep's profile and path, a loft's sections and rails) share: which
//! sketches the feature at its place takes, the regions offered, the
//! chains of curves picked, and how they're named.

use varde_document::{BodyId, CurveChain, Document, Feature, FeatureId};
use varde_sketch::{Id, Sketch};
use varde_view::{Candidate, MotionKind, MotionPick, SketchLines, SplitMode};

use super::{Doc, MotionSession};
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
        sketch_before(document, self.index_in(document), sketch)
    }

    /// Whether a split's tool is picked in the viewport in its sketches, where
    /// the model isn't picked: regions or a line.
    /// A sweep's profile's regions likewise, and a loft's sections and
    /// rails.
    pub(crate) fn picks_sketches(&self) -> bool {
        let split = self.kind == MotionKind::Split
            && self.picking == MotionPick::Tool
            && matches!(self.split.mode, SplitMode::Regions | SplitMode::Line);
        let loft = self.kind == MotionKind::Loft
            && matches!(self.picking, MotionPick::Regions | MotionPick::Path);
        split || loft || (self.kind == MotionKind::Sweep && self.picking == MotionPick::Regions)
    }
}

impl Doc {
    /// The sketches of the features `wanted` takes whose curves or points
    /// the feature being set up in `session` (a split, a sweep, a loft)
    /// picks, where they're placed: each a sketch it can take at its
    /// place ([`MotionSession::takes_sketch`]) that's placed.
    pub(super) fn sketch_lines<'s>(
        &'s self,
        session: &MotionSession,
        wanted: impl Fn(&Feature) -> bool,
    ) -> Vec<SketchLines<'s>> {
        let document = self.editor.document();
        self.placed_sketches(|feature| {
            wanted(feature) && session.takes_sketch(document, feature.id)
        })
    }

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

/// Whether `sketch` is a sketch of `document` before feature `index`.
pub(super) fn sketch_before(document: &Document, index: usize, sketch: FeatureId) -> bool {
    let features = document.features();
    (features[..index.min(features.len())].iter()).any(|feature| feature.id == sketch)
        && sketch_of(document, sketch).is_some()
}

/// Whether `sketch` is a sketch of `document` that still has every
/// curve of `curves`.
pub(super) fn curves_held(document: &Document, sketch: FeatureId, curves: &[Id]) -> bool {
    sketch_of(document, sketch)
        .is_some_and(|drawn| curves.iter().all(|&curve| drawn.curve(curve).is_some()))
}

/// Whether a chain of `chains` names what `document` no longer takes at
/// feature `index`: a sketch not before it, or a curve its sketch hasn't.
pub(super) fn chains_gone(document: &Document, index: usize, chains: &[CurveChain]) -> bool {
    (chains.iter()).any(|chain| {
        !sketch_before(document, index, chain.sketch)
            || !curves_held(document, chain.sketch, &chain.curves)
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
