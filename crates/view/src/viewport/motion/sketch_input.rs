//! What a split's and a sweep's tools pick in their sketches: a region
//! or a curve, under the cursor.

use glam::DVec2;
use iced::Rectangle;
use varde_document::FeatureId;
use varde_render::Camera;
use varde_sketch::Id;

use crate::motion::SketchLines;
use crate::viewport::regions::{self, Regions};
use crate::viewport::sketch_pick::{SketchHit, curve_under};

/// What the viewport keeps of a tool picking in its sketches: the region
/// under the cursor and the base layer of the source's regions, and the
/// curve under the cursor, with its sketch.
#[derive(Default)]
pub(super) struct SketchInput {
    pub(super) regions: regions::Input,
    pub(super) curve: Option<(FeatureId, Id)>,
}

impl SketchInput {
    /// Whether a region or curve is under the cursor.
    pub(super) fn over(&self) -> bool {
        self.regions.hover.is_some() || self.curve.is_some()
    }

    /// Lets go of the region or curve under the cursor: whether there
    /// was one.
    pub(super) fn leave(&mut self) -> bool {
        self.regions.hover.take().is_some() | self.curve.take().is_some()
    }
}

impl std::fmt::Debug for SketchInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SketchInput")
            .field("region", &self.regions.hover)
            .field("curve", &self.curve)
            .finish()
    }
}

/// A region or a curve of a sketch, as picked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum SketchPick {
    Region { sketch: FeatureId, region: usize },
    Curve { sketch: FeatureId, curve: Id },
}

/// The regions and the curves a tool picks in.
pub(super) struct SketchPicker<'s, 'a> {
    pub(super) regions: Regions<'s, 'a>,
    pub(super) lines: &'s [SketchLines<'a>],
}

impl SketchPicker<'_, '_> {
    /// The region (picking `regions`) or else the curve under the screen
    /// position `at`.
    pub(super) fn under(
        &self,
        regions: bool,
        at: DVec2,
        camera: &Camera,
        bounds: Rectangle,
    ) -> Option<SketchPick> {
        if regions {
            let (sketch, region) = self.regions.region_under(at, camera, bounds)?;
            Some(SketchPick::Region { sketch, region })
        } else {
            let (sketch, curve) =
                curve_under(self.lines, at, camera, bounds, None).map(SketchHit::of)?;
            Some(SketchPick::Curve { sketch, curve })
        }
    }

    /// Works out what's [`under`](Self::under) the screen position `at`
    /// (nothing off the viewport) into `input`: whether it changed.
    pub(super) fn hover(
        &self,
        regions: bool,
        input: &mut SketchInput,
        at: Option<DVec2>,
        camera: &Camera,
        bounds: Rectangle,
    ) -> bool {
        let (region, curve) = match at.and_then(|at| self.under(regions, at, camera, bounds)) {
            Some(SketchPick::Region { sketch, region }) => (Some((sketch, region)), None),
            Some(SketchPick::Curve { sketch, curve }) => (None, Some((sketch, curve))),
            None => (None, None),
        };
        std::mem::replace(&mut input.regions.hover, region) != region
            || std::mem::replace(&mut input.curve, curve) != curve
    }
}
