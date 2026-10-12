//! A split's tool picked in its sketches: their regions or curves.

use glam::DVec2;
use iced::widget::shader::Action;
use iced::{Point, Rectangle, mouse};
use varde_document::FeatureId;
use varde_render::{Camera, SketchLayer, Space as LayerSpace};
use varde_sketch::Id;

use super::{CURVE_WIDTH, Moving};
use crate::motion::{MotionLook, MotionPick, SplitMode, SplitView};
use crate::theme::SketchColors;
use crate::viewport::regions::{self, Regions, grid_plane};
use crate::viewport::sketch::line;
use crate::viewport::sketch_pick::{HOVERED_WIDTH, SELECTED_WIDTH, SketchHit, curve_under};
use crate::{Look, Message};

/// What the viewport keeps of a split's tool picked in its sketches: the
/// region under the cursor and the base layer of the source's regions,
/// and the curve under the cursor, with its sketch.
#[derive(Default)]
pub(super) struct SplitInput {
    pub(super) regions: regions::Input,
    pub(super) curve: Option<(FeatureId, Id)>,
}

impl SplitInput {
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

impl std::fmt::Debug for SplitInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SplitInput")
            .field("region", &self.regions.hover)
            .field("curve", &self.curve)
            .finish()
    }
}

/// A split's sketches whose regions show, and those picked.
pub(super) fn split_regions<'s, 'a>(split: &'s SplitView<'a>) -> Regions<'s, 'a> {
    Regions {
        candidates: &split.candidates,
        source: split.source,
        picked: split.picked,
        panel: None,
    }
}

/// Draws a split's line on `live`: while it's picked (`picking`), the
/// curves of the sketches it may be of (construction ones dashed, the
/// one `hovered` stronger), and the curves picked in the selected colour.
pub(super) fn split_lines(
    live: &mut SketchLayer,
    split: &SplitView<'_>,
    hovered: Option<(FeatureId, Id)>,
    picking: bool,
    colors: SketchColors,
) {
    let picked = |feature: FeatureId, id: Id| {
        split
            .chain
            .is_some_and(|(sketch, curves)| sketch == feature && curves.binary_search(&id).is_ok())
    };
    for candidate in &split.lines {
        let Some(plane) = grid_plane(candidate.placement) else {
            continue;
        };
        let space = LayerSpace::On(plane);
        let sketch = candidate.sketch;
        for entry in &sketch.curves {
            let is_picked = picked(candidate.feature, entry.id);
            let is_hovered = hovered == Some((candidate.feature, entry.id));
            if !(picking || is_picked) {
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

impl<'a> Moving<'a> {
    /// The kind of a split's tool picked in its sketches, while one is:
    /// regions or a line, which then take the left button.
    pub(super) fn split_picking(&self) -> Option<SplitMode> {
        let split = self.state.split.as_ref()?;
        let picking = self.state.picking == MotionPick::Tool && self.state.editable;
        (picking && matches!(split.mode, SplitMode::Regions | SplitMode::Line))
            .then_some(split.mode)
    }

    /// The curve of a split's sketches under the screen position `at`, and
    /// its sketch: see [`curve_under`].
    fn curve_under(
        &self,
        split: &SplitView<'_>,
        at: DVec2,
        camera: &Camera,
        bounds: Rectangle,
    ) -> Option<(FeatureId, Id)> {
        curve_under(&split.lines, at, camera, bounds, None).map(SketchHit::of)
    }

    /// Works out the region or curve of a split picking in `mode` under
    /// the screen position `at` (none off the viewport) into `input`:
    /// whether it changed.
    pub(super) fn split_hover(
        &self,
        mode: SplitMode,
        input: &mut SplitInput,
        at: Option<DVec2>,
        camera: &Camera,
        bounds: Rectangle,
    ) -> bool {
        let Some(split) = self.state.split.as_ref() else {
            return false;
        };
        let (region, curve) = match mode {
            SplitMode::Regions => (
                at.and_then(|at| split_regions(split).region_under(at, camera, bounds)),
                None,
            ),
            _ => (
                None,
                at.and_then(|at| self.curve_under(split, at, camera, bounds)),
            ),
        };
        std::mem::replace(&mut input.regions.hover, region) != region
            || std::mem::replace(&mut input.curve, curve) != curve
    }

    /// Takes the mouse `event` while a split's tool is picked in its
    /// sketches ([`Moving::split_picking`]): hovering and clicking the
    /// region or curve under the cursor. `None` for what's left to the
    /// camera: the left button pressed off them orbits.
    pub(super) fn split_mouse(
        &self,
        mode: SplitMode,
        input: &mut SplitInput,
        event: mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        camera: &Camera,
    ) -> Option<Action<Message>> {
        let split = self.state.split.as_ref()?;
        let local = |p: Point| DVec2::new((p.x - bounds.x).into(), (p.y - bounds.y).into());
        let regions = split_regions(split);
        match event {
            mouse::Event::CursorMoved { .. } => {
                let over = cursor.position_over(bounds).map(local);
                self.split_hover(mode, input, over, camera, bounds)
                    .then(Action::request_redraw)
            }
            mouse::Event::CursorLeft => input.leave().then(Action::request_redraw),
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let at = local(cursor.position_over(bounds)?);
                let look = match mode {
                    SplitMode::Regions => {
                        let (sketch, region) = regions.region_under(at, camera, bounds)?;
                        MotionLook::SplitRegion { sketch, region }
                    }
                    _ => {
                        let (sketch, curve) = self.curve_under(split, at, camera, bounds)?;
                        MotionLook::SplitCurve { sketch, curve }
                    }
                };
                Some(Action::publish(Message::Look(Look::Motion(look))).and_capture())
            }
            _ => None,
        }
    }
}
