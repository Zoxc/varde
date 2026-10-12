//! The measure tool in the viewport. The cursor picks the model as it
//! does outside the sessions (`ModelPicking`), with the snap points of
//! what it's over taken within `SNAP_REACH` (`PickIndex::snap`); the app
//! highlights what's hovered and picked. Drawn here, on top of the model
//! so a point or a distance inside it still shows: the hovered face's or
//! edge's snap points as dots, the one the cursor takes bigger; the
//! points picked, in A's and B's colours; the minimum distance's segment
//! between its two points, and its label, a widget anchored at its
//! middle.

use std::sync::Arc;

use glam::DVec2;
use iced::Element;
use iced::widget::{container, text};
use varde_document::Placement;
use varde_render::{Camera, Colors, SketchLayer};

use super::style::{line, mark_dot};
use crate::Message;
use crate::anchors::Anchors;
use crate::measure::{MeasureSlot, MeasureState};
use crate::pick::{Pick, PickIndex};
use crate::theme::opaque;
use crate::theme::{self, SketchColors};

/// How wide a snap dot is to its rim's outside, and the one the cursor
/// takes, in pixels.
const SNAP_RADIUS: f32 = 3.5;
const SNAPPED_RADIUS: f32 = 5.0;
/// How wide a picked point's dot is, and the ends of the distance's.
const POINT_RADIUS: f32 = 5.0;
const END_RADIUS: f32 = 3.0;
/// How wide the distance's segment is, dashed.
const SEGMENT_WIDTH: f32 = 2.0;

/// The measure tool, as the viewport shows it.
#[derive(Debug, Clone)]
pub(crate) struct Measuring<'a> {
    state: MeasureState<'a>,
}

impl<'a> Measuring<'a> {
    pub(crate) fn new(state: MeasureState<'a>) -> Self {
        Self { state }
    }

    /// What it draws, in `colors` and the sketch's `sketch` colours: no
    /// base layer, as it all changes with the cursor or the answer, and a
    /// live one with the dots and the distance's segment, in the world.
    pub(crate) fn layers(
        &self,
        colors: &Colors,
        sketch: SketchColors,
    ) -> (Arc<SketchLayer>, SketchLayer) {
        (super::NO_LAYER.clone(), self.live(colors, sketch))
    }

    fn live(&self, colors: &Colors, sketch: SketchColors) -> SketchLayer {
        let mut layer = SketchLayer::default();
        let colour = |slot: MeasureSlot| match slot {
            MeasureSlot::A => opaque(colors.selected),
            MeasureSlot::B => opaque(colors.second),
        };
        // Picks that touch have no segment to draw.
        if let Some(gap) = self.state.gap().filter(|gap| gap.distance > 0.0) {
            let [a, b] = gap.points.map(|p| glam::DVec3::from(p).as_vec3());
            let style = line(colour(MeasureSlot::A), SEGMENT_WIDTH, true);
            layer.world_polyline(&[a, b], style);
            layer.world_point(
                a,
                mark_dot(END_RADIUS, colour(MeasureSlot::A), sketch, true),
            );
            layer.world_point(
                b,
                mark_dot(END_RADIUS, colour(MeasureSlot::B), sketch, true),
            );
        }
        for slot in [MeasureSlot::A, MeasureSlot::B] {
            if let Some(point) = self.state.points[slot.index()] {
                layer.world_point(
                    point.as_vec3(),
                    mark_dot(POINT_RADIUS, colour(slot), sketch, true),
                );
            }
        }
        if let Some(hover) = self.state.hover {
            snap_dots(&mut layer, self.state.index, hover, sketch);
        }
        layer
    }

    /// The distance's label, anchored at the middle of its segment, seen
    /// by `camera`: none without a distance, or for picks that touch.
    pub(crate) fn label(&self, camera: &Camera) -> Option<Element<'a, Message>> {
        let gap = self.state.gap()?;
        if gap.distance <= 0.0 {
            return None;
        }
        let [a, b] = gap.points.map(glam::DVec3::from);
        let placement = Placement {
            origin: (a + b) / 2.0,
            x: glam::DVec3::X,
            y: glam::DVec3::Y,
            normal: glam::DVec3::Z,
        };
        let shown =
            varde_expr::Shown::current().format(gap.distance, Some(self.state.units.into()));
        let chip = container(text(shown).size(12).style(theme::text_in(|p| p.text)))
            .padding([1, 4])
            .style(|theme| theme::glyph(theme, false));
        let anchored = Anchors::new(*camera, placement, [(DVec2::ZERO, chip.into())]);
        Some(anchored.into())
    }
}

/// The snap points of what `hover` is over in `index`'s model, if it's
/// that model's, as dots in `sketch`'s hovered colour, the one the
/// cursor takes bigger: the measure tool's, and an align's while its
/// point is picked.
pub(super) fn snap_dots(
    layer: &mut SketchLayer,
    index: &PickIndex,
    hover: Pick,
    sketch: SketchColors,
) {
    if hover.model != index.model() {
        return;
    }
    for (snapped, point) in index.snaps(hover.target) {
        let (radius, fixed) = if hover.snap == Some(snapped) {
            (SNAPPED_RADIUS, true)
        } else {
            (SNAP_RADIUS, false)
        };
        let style = mark_dot(radius, sketch.hovered, sketch, fixed);
        layer.world_point(point.as_vec3(), style);
    }
}
