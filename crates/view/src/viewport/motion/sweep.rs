//! A sweep's profile's regions and path curves picked in its sketches.

use std::sync::Arc;

use glam::DVec2;
use iced::widget::shader::Action;
use iced::{Rectangle, mouse};
use varde_document::FeatureId;
use varde_render::{Camera, SketchLayer, Space as LayerSpace};
use varde_sketch::Id;

use super::sketch_input::{SketchInput, SketchPick, SketchPicker};
use super::{CURVE_WIDTH, Input, Moving};
use crate::motion::{MotionLook, MotionPick, SketchLines, SweepView};
use crate::operation_panel::PanelHover;
use crate::theme::SketchColors;
use crate::viewport::local_cursor;
use crate::viewport::regions::{Regions, grid_plane};
use crate::viewport::sketch_pick::{HOVERED_WIDTH, SELECTED_WIDTH};
use crate::viewport::style::line;
use crate::{Look, Message};

/// A sweep's sketches whose regions show, those picked, and the one
/// hovered in the panel (`hover`).
pub(super) fn sweep_regions<'s, 'a>(
    sweep: &'s SweepView<'a>,
    hover: Option<PanelHover>,
) -> Regions<'s, 'a> {
    Regions::new(&sweep.candidates, sweep.source, sweep.picked, hover)
}

/// A sweep's profile regions and path curves, as it picks them.
fn sweep_picker<'s, 'a>(sweep: &'s SweepView<'a>) -> SketchPicker<'s, 'a> {
    SketchPicker {
        regions: sweep_regions(sweep, None),
        lines: &sweep.lines,
    }
}

/// Draws a sweep's path (or a loft's rails, or a split's line) on
/// `live`: while it's picked (`picking`), the curves of the sketches of
/// `lines` (construction ones dashed, the chain of the one `hovered`
/// stronger), and each part's curves of `chains` in the selected colour,
/// those of the part whose row is hovered (`part`) in the hovered one.
pub(super) fn sweep_lines(
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

impl<'a> Moving<'a> {
    /// What a sweep picks in its sketches, while it does: its profile's
    /// regions, or its path's curves (the model's edges are picked as the
    /// model is, where no curve is under the cursor).
    pub(super) fn sweep_picking(&self) -> Option<MotionPick> {
        self.state.sweep()?;
        let picking = self.state.picking;
        (self.state.editable && matches!(picking, MotionPick::Regions | MotionPick::Path))
            .then_some(picking)
    }

    /// Takes the mouse `event` while a sweep picks in its sketches
    /// ([`Moving::sweep_picking`]): hovering and clicking the region of
    /// its profile, or the curve of a path sketch, under the cursor.
    /// `None` for what's left to picking the model and the camera: off
    /// the path's curves, the model's edges.
    pub(super) fn sweep_mouse(
        &self,
        picking: MotionPick,
        input: &mut Input,
        event: mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        camera: &Camera,
    ) -> Option<Action<Message>> {
        let sweep = self.state.sweep()?;
        let (redraw, input) = (&mut input.redraw, &mut input.sweep);
        match event {
            mouse::Event::CursorMoved { .. } => {
                let over = local_cursor(bounds, cursor);
                let changed = self.sweep_hover(picking, input, over, camera, bounds);
                // Over a curve, the model's edge under it isn't hovered.
                match input.curve {
                    Some(_) if changed => {
                        Some(Action::publish(Message::Look(Look::Hover(None))).and_capture())
                    }
                    Some(_) => Some(Action::capture()),
                    // Off a path curve, the model's picking has the move
                    // too, so an edge there is hovered at once; the frame
                    // drawing the curve away is asked for after it.
                    None if changed && picking == MotionPick::Path => {
                        *redraw = true;
                        None
                    }
                    None if changed => Some(Action::request_redraw()),
                    None => None,
                }
            }
            mouse::Event::CursorLeft => input.leave().then(Action::request_redraw),
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let at = local_cursor(bounds, cursor)?;
                let regions = picking == MotionPick::Regions;
                let look = match sweep_picker(sweep).under(regions, at, camera, bounds)? {
                    SketchPick::Region { sketch, region } => {
                        MotionLook::SweepRegion { sketch, region }
                    }
                    SketchPick::Curve { sketch, curve } => MotionLook::SweepCurve { sketch, curve },
                };
                Some(Action::publish(Message::Look(Look::Motion(look))).and_capture())
            }
            _ => None,
        }
    }

    /// [`Moving::layers`] for a sweep, adding to `live`: while its
    /// profile is picked, its sketches' regions (the one under the cursor
    /// filled stronger); the profile region hovered in the panel; its
    /// path sketches' curves, its path's lit; the base layer of the
    /// profile's regions if it has a source.
    pub(super) fn sweep_layers(
        &self,
        mut live: SketchLayer,
        sweep: &SweepView<'_>,
        input: &Input,
        colors: SketchColors,
        (camera, bounds): (&Camera, Rectangle),
    ) -> (Arc<SketchLayer>, SketchLayer) {
        let picking = self.sweep_picking();
        let regions = sweep_regions(sweep, self.state.hover);
        if picking == Some(MotionPick::Regions) {
            regions.live(input.sweep.regions.hover, colors, &mut live);
        }
        regions.panel_region(camera, bounds, colors, &mut live);
        let hovered = input
            .sweep
            .curve
            .filter(|_| picking == Some(MotionPick::Path));
        let part = match self.state.hover {
            Some(PanelHover::Part(at)) => sweep.chains.get(at).copied(),
            _ => None,
        };
        sweep_lines(
            &mut live,
            &sweep.lines,
            &sweep.chains,
            hovered,
            part,
            picking.is_some(),
            colors,
        );
        let base = if regions.source().is_some() {
            regions.base_layer(&input.sweep.regions, colors)
        } else {
            crate::viewport::NO_LAYER.clone()
        };
        (base, live)
    }

    /// [`Moving::redraw`] for a sweep picking `picking` in its sketches:
    /// works out its region or path curve under the screen position `at`
    /// (none off the viewport), the model let go of under a curve, if
    /// `hovered`.
    pub(super) fn sweep_redraw(
        &self,
        picking: MotionPick,
        input: &mut Input,
        at: Option<DVec2>,
        camera: &Camera,
        bounds: Rectangle,
        hovered: bool,
    ) -> Option<Action<Message>> {
        self.state.sweep()?;
        let changed = self.sweep_hover(picking, &mut input.sweep, at, camera, bounds);
        match input.sweep.curve {
            // The model isn't hovered under a path curve.
            Some(_) if hovered => Some(Action::publish(Message::Look(Look::Hover(None)))),
            Some(_) if changed => Some(Action::request_redraw()),
            // Off a path curve, the model under the cursor is picked
            // again; the frame drawing the curve away is asked for after.
            None if changed => {
                input.redraw = true;
                None
            }
            _ => None,
        }
    }

    /// Works out what of a sweep's sketches is under the cursor at `at`
    /// as it picks them (`picking`): a region of its profile or a curve
    /// of its path. Whether that changed.
    fn sweep_hover(
        &self,
        picking: MotionPick,
        input: &mut SketchInput,
        at: Option<DVec2>,
        camera: &Camera,
        bounds: Rectangle,
    ) -> bool {
        self.state.sweep().is_some_and(|sweep| {
            sweep_picker(sweep).hover(picking == MotionPick::Regions, input, at, camera, bounds)
        })
    }
}
