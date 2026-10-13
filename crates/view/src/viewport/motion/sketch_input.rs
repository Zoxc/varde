//! What a split's and a sweep's tools pick in their sketches: a region
//! or a curve, under the cursor; and the curves a sweep's path, a loft's
//! rails and a split's line are picked from, as drawn.

use glam::DVec2;
use iced::widget::shader::Action;
use iced::{Rectangle, mouse};
use varde_document::FeatureId;
use varde_render::{Camera, SketchLayer, Space as LayerSpace};
use varde_sketch::Id;

use super::{CURVE_WIDTH, Moving};
use crate::motion::{MotionLook, MotionPick, SketchLines};
use crate::theme::SketchColors;
use crate::viewport::local_cursor;
use crate::viewport::regions::{self, Regions, grid_plane};
use crate::viewport::sketch_pick::{HOVERED_WIDTH, SELECTED_WIDTH, SketchHit, curve_under};
use crate::viewport::style::line;
use crate::{Look, Message};

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

impl Moving<'_> {
    /// What a sweep or a loft picks in its sketches, while the panel is
    /// editable: regions or a path's curves.
    pub(super) fn regions_or_path(&self) -> Option<MotionPick> {
        let picking = self.state.picking;
        (self.state.editable && matches!(picking, MotionPick::Regions | MotionPick::Path))
            .then_some(picking)
    }
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

    /// Takes a press of the left button with the `cursor` over `bounds`:
    /// publishes `look` of what's [`under`](Self::under) it, if anything.
    pub(super) fn press(
        &self,
        regions: bool,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        camera: &Camera,
        look: impl FnOnce(SketchPick) -> MotionLook,
    ) -> Option<Action<Message>> {
        let at = local_cursor(bounds, cursor)?;
        let look = look(self.under(regions, at, camera, bounds)?);
        Some(Action::publish(Message::Look(Look::Motion(look))).and_capture())
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

/// Draws a sweep's path (or a loft's rails, or a split's line) on
/// `live`: while it's picked (`picking`), the curves of the sketches of
/// `lines` (construction ones dashed, the chain of the one `hovered`
/// stronger), and each part's curves of `chains` in the selected colour,
/// those of the part whose row is hovered (`part`) in the hovered one.
pub(super) fn chain_lines(
    live: &mut SketchLayer,
    lines: &[SketchLines<'_>],
    chains: &[(FeatureId, &[Id])],
    hovered: Option<(FeatureId, Id)>,
    part: Option<(FeatureId, &[Id])>,
    picking: bool,
    colors: SketchColors,
) {
    let picked = |feature: FeatureId, id: Id| {
        (chains.iter())
            .any(|&(sketch, curves)| sketch == feature && curves.binary_search(&id).is_ok())
    };
    let lit = |feature: FeatureId, id: Id| {
        part.is_some_and(|(sketch, curves)| sketch == feature && curves.binary_search(&id).is_ok())
    };
    for candidate in lines {
        let Some(plane) = grid_plane(candidate.placement) else {
            continue;
        };
        let space = LayerSpace::On(plane);
        let sketch = candidate.sketch;
        let chain = match hovered {
            Some((feature, curve)) if feature == candidate.feature => sketch.chain_of(curve),
            _ => Vec::new(),
        };
        for entry in &sketch.curves {
            let is_picked = picked(candidate.feature, entry.id);
            let is_hovered = chain.contains(&entry.id) || lit(candidate.feature, entry.id);
            if !(picking || is_picked || is_hovered) {
                continue;
            }
            let Some(points) = sketch.flatten(&entry.curve) else {
                continue;
            };
            let (color, width) = if is_hovered {
                (colors.hovered, HOVERED_WIDTH)
            } else if is_picked {
                (colors.selected, SELECTED_WIDTH)
            } else if entry.construction {
                (colors.construction, CURVE_WIDTH)
            } else {
                (colors.curve, CURVE_WIDTH)
            };
            let dashed = entry.construction && !is_picked && !is_hovered;
            live.polyline(space, &points, line(color, width, dashed));
        }
    }
}
