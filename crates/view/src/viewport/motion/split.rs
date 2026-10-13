//! A split's tool picked in its sketches: their regions or curves.

use glam::DVec2;
use iced::widget::shader::Action;
use iced::{Rectangle, mouse};
use varde_render::Camera;

use super::Moving;
use super::sketch_input::{SketchInput, SketchPick, SketchPicker};
use crate::motion::{MotionLook, MotionPick, SplitMode, SplitView};
use crate::viewport::local_cursor;
use crate::viewport::regions::Regions;
use crate::{Look, Message};

/// A split's sketches whose regions show, and those picked. The panel's
/// region rows don't light their region here.
pub(super) fn split_regions<'s, 'a>(split: &'s SplitView<'a>) -> Regions<'s, 'a> {
    Regions::new(&split.candidates, split.source, split.picked, None)
}

/// A split's regions and curves, as its tool picks them.
fn split_picker<'s, 'a>(split: &'s SplitView<'a>) -> SketchPicker<'s, 'a> {
    SketchPicker {
        regions: split_regions(split),
        lines: &split.lines,
    }
}

impl<'a> Moving<'a> {
    /// The kind of a split's tool picked in its sketches, while one is:
    /// regions or a line, which then take the left button.
    pub(super) fn split_picking(&self) -> Option<SplitMode> {
        let split = self.state.split()?;
        let picking = self.state.picking == MotionPick::Tool && self.state.editable;
        (picking && matches!(split.mode, SplitMode::Regions | SplitMode::Line))
            .then_some(split.mode)
    }

    /// Works out the region or curve of a split picking in `mode` under
    /// the screen position `at` (none off the viewport) into `input`:
    /// whether it changed.
    pub(super) fn split_hover(
        &self,
        mode: SplitMode,
        input: &mut SketchInput,
        at: Option<DVec2>,
        camera: &Camera,
        bounds: Rectangle,
    ) -> bool {
        self.state.split().is_some_and(|split| {
            split_picker(split).hover(mode == SplitMode::Regions, input, at, camera, bounds)
        })
    }

    /// Takes the mouse `event` while a split's tool is picked in its
    /// sketches ([`Moving::split_picking`]): hovering and clicking the
    /// region or curve under the cursor. `None` for what's left to the
    /// camera: the left button pressed off them orbits.
    pub(super) fn split_mouse(
        &self,
        mode: SplitMode,
        input: &mut SketchInput,
        event: mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        camera: &Camera,
    ) -> Option<Action<Message>> {
        let split = self.state.split()?;
        match event {
            mouse::Event::CursorMoved { .. } => {
                let over = local_cursor(bounds, cursor);
                self.split_hover(mode, input, over, camera, bounds)
                    .then(Action::request_redraw)
            }
            mouse::Event::CursorLeft => input.leave().then(Action::request_redraw),
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let at = local_cursor(bounds, cursor)?;
                let regions = mode == SplitMode::Regions;
                let look = match split_picker(split).under(regions, at, camera, bounds)? {
                    SketchPick::Region { sketch, region } => {
                        MotionLook::SplitRegion { sketch, region }
                    }
                    SketchPick::Curve { sketch, curve } => MotionLook::SplitCurve { sketch, curve },
                };
                Some(Action::publish(Message::Look(Look::Motion(look))).and_capture())
            }
            _ => None,
        }
    }
}
