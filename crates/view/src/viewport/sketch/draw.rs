//! The layers the renderer draws of the sketch being edited: the base
//! layer, built again only when what it shows changes, and the live layer
//! over it each frame (hover, previews, snaps, combs, arrowheads).

use std::collections::BTreeSet;
use std::sync::Arc;

use glam::DVec2;
use iced::keyboard::Modifiers;
use iced::{Color, Rectangle};
use varde_document::{MAX_COORD, Placement};
use varde_kernel_compat::RenderLines;
use varde_render::{Camera, PointStyle, SketchLayer, Space, Srgba};
use varde_sketch::{
    Curve, CurveEntry, DimensionEntry, Id, Measure, Profiles, Selectable, Side, Sketch, SplineKind,
    cut_line, foot,
};

use super::{
    AXIS_WIDTH, Arrows, BOX_LINE_WIDTH, Base, COMB_LENGTH, COMB_WIDTH, CURVE_WIDTH, GUIDE_WIDTH,
    HANDLE_WIDTH, HOVERED_POINT_RADIUS, Input, POINT_RADIUS, SELECTED_WIDTH, Sketching, States,
    dimensions,
};
use crate::document::tied_items;
use crate::hit::BoxMode;
use crate::projection::{Cursor, Projector};
use crate::shortcut::Held;
use crate::snap::{Snap, Target};
use crate::theme::SketchColors;
use crate::typed::{self, Field};
use crate::viewport::sketch_pick::HOVERED_WIDTH;
use crate::viewport::style::{fill_region, line, srgba};
use crate::{Drawing, Tool, ValueTarget, dimension};

const POINT_RIM: f32 = 1.2;
/// How wide dimensions' lines are, in pixels.
const DIMENSION_WIDTH: f32 = 1.0;
/// How long a dimension's arrowhead is, and how wide either side of its
/// line, in pixels.
const ARROW_LENGTH: f64 = 8.0;
const ARROW_HALF_WIDTH: f64 = 2.8;
/// The ring marking open ends that almost meet: its radius to the
/// outside and how wide it is, in pixels.
const NEAR_MISS_RADIUS: f32 = 7.0;
const NEAR_MISS_WIDTH: f32 = 1.5;
/// The disc marking where a point snaps ([`Snap::snapped`]), drawing or
/// dragged: its radius, in pixels, wide enough to show round the cursor
/// over it, and how opaque it is, of the points' colour.
const SNAP_RADIUS: f32 = 15.0;
const SNAP_WASH: f32 = 0.15;

impl<'a> Sketching<'a> {
    /// What the renderer draws of the sketch, with `input` as it is, in a
    /// viewport of `bounds` seen by `camera`: the base layer, built again
    /// only when what it's drawn from ([`Drawn`]) changes, and the live
    /// layer.
    pub(crate) fn layers(
        &self,
        input: &Input,
        camera: &Camera,
        bounds: Rectangle,
        colors: SketchColors,
        modifiers: Modifiers,
    ) -> (Arc<SketchLayer>, SketchLayer) {
        let projector = self.projector(camera, bounds);
        let dragged = self.dragged(input, projector.as_ref(), modifiers);
        let closing = self.drag_closing(input, dragged);
        let mut base = input.base.borrow_mut();
        let current = (base.as_ref()).is_some_and(|base| base.drawn.matches(self, colors, closing));
        let base = match &mut *base {
            Some(base) if current => base,
            base => {
                // Dropped on the other end of its curve, the sketch as the
                // drop leaves it: the curve closed, the point gone, the
                // region filled.
                let closed = closing.and_then(|closing| self.closed(closing));
                let profiles = closed
                    .as_ref()
                    .map(|closed| closed.profiles().ok().map(Arc::new));
                let (layer, arrows) = match (&closed, &profiles) {
                    (Some(closed), Some(profiles)) => Sketching {
                        sketch: closed,
                        profiles: profiles.as_ref(),
                        ..self.clone()
                    }
                    .base_layer(colors),
                    _ => self.base_layer(colors),
                };
                let drawn = Drawn::of(self, colors, closing);
                base.insert(Base {
                    drawn,
                    layer: Arc::new(layer),
                    arrows,
                    combs: self.combs(),
                    failing: self.failing_lines(),
                })
            }
        };
        let mut live = self.live_layer(
            input,
            projector.as_ref(),
            colors,
            &base.arrows,
            modifiers,
            dragged,
        );
        if let Some(projector) = &projector {
            combs(&mut live, &base.combs, projector.pixel(), colors.guide);
        }
        (base.layer.clone(), live)
    }

    /// Where what the button held down drags snaps with the cursor where
    /// it is, if it drags and snaps ([`Sketching::drag_snap`]).
    fn dragged(
        &self,
        input: &Input,
        projector: Option<&Projector>,
        modifiers: Modifiers,
    ) -> Option<Snap> {
        let press = input.press.filter(|press| press.moved && press.grab)?;
        let cursor = projector?.cursor(input.cursor?)?;
        self.drag_snap(&press, cursor, modifiers)
            .map(|(_, snap)| snap)
    }

    /// A point dragged onto the other end of its arc or spline (see
    /// [`crate::spline::close_on_drop`]), snapping as `dragged`: the point
    /// and the one it's dropped on. Cheap, for [`Drawn`]'s key: the sketch
    /// the drop leaves ([`Sketching::closed`]) is built only with the
    /// layer.
    fn drag_closing(&self, input: &Input, dragged: Option<Snap>) -> Option<(Id, Id)> {
        let point = input.press?.hit?.item()?;
        let target = dragged?.target;
        let Some(Target::Point(other)) = target else {
            return None;
        };
        crate::spline::close_on_drop(self.sketch, point, target)?;
        Some((point, other))
    }

    /// The sketch as dropping `point` on `other` leaves it, closing its
    /// curve (see [`crate::spline::closed_by_drop`]), drawn in place of
    /// the one shown.
    fn closed(&self, (point, other): (Id, Id)) -> Option<Sketch> {
        crate::spline::closed_by_drop(self.sketch, point, Some(Target::Point(other)))
            .map(|(_, closed)| closed)
    }

    /// The failing curves of the base layer last built, placed in the
    /// world, if there are any: for [`Frame::errors`], whose halo goes
    /// around them as around the failures' geometry in the model. Call
    /// after [`Sketching::layers`].
    ///
    /// [`Frame::errors`]: varde_render::Frame::errors
    pub(crate) fn failing(&self, input: &Input) -> Option<Arc<RenderLines>> {
        input.base.borrow().as_ref()?.failing.clone()
    }

    /// The failing curves the sketch holds, flattened and placed in the
    /// world, if there are any. A curve that can't be placed within
    /// [`RenderLines::MAX_POSITION`] is left out.
    fn failing_lines(&self) -> Option<Arc<RenderLines>> {
        let mut lines = RenderLines::default();
        let curves =
            (self.sketch.curves.iter()).filter(|entry| self.states.failing.contains(&entry.id));
        for entry in curves {
            let Some(polyline) = self.sketch.flatten(&entry.curve) else {
                continue;
            };
            let placed = polyline
                .iter()
                .map(|&at| self.placement.to_world(at).as_vec3());
            // Refused, it isn't drawn.
            let _ = lines.push(placed);
        }
        (!lines.points().is_empty()).then(|| Arc::new(lines))
    }

    /// Whether the item `id` is selected.
    pub(super) fn selected(&self, id: Id) -> bool {
        self.selection.contains(&Selectable::Item(id))
    }

    /// The curvature combs of the splines selected, if they show.
    fn combs(&self) -> Vec<Vec<[DVec2; 2]>> {
        if !self.comb {
            return Vec::new();
        }
        let selected = self.selection.iter();
        selected
            .filter_map(|&target| self.sketch.curvature_comb(target.item()?))
            .collect()
    }

    /// The sketch: the regions its curves enclose shaded lightly, its
    /// curves, construction ones and the ends of lines fillets and
    /// chamfers cut off dashed, then its points, what's selected
    /// over the rest. Each is coloured by its state: free, fixed (darker,
    /// a point filled), in a conflict or named by a failing feature
    /// (red), and faded while it waits on
    /// the solver. With the dimensions' arrowheads, for the live layer to
    /// draw where they show.
    fn base_layer(&self, colors: SketchColors) -> (SketchLayer, Vec<Arrows>) {
        let mut layer = SketchLayer::default();
        // Each on its own, as regions share their edges: a hole's inside
        // is a region too, and even-odd over them all would leave it out.
        for region in self.profiles.iter().flat_map(|profiles| &profiles.regions) {
            fill_region(&mut layer, region, colors.region);
        }
        let sketch = self.sketch;
        // The axes under the rest, and the origin under the points.
        for id in [Id::X_AXIS, Id::Y_AXIS] {
            for half in axis(id).into_iter().flatten() {
                let style = line(self.builtin_color(id, colors), AXIS_WIDTH, false);
                layer.axis_polyline(Space::Sketch, &half, style);
            }
        }
        // What links made is in their own tone, a link's curves dashed
        // unless they count for profiles, as construction curves are.
        let linked = sketch.linked();
        // The ends of lines fillets and chamfers cut off are dashed.
        let cut_back = sketch.cut_back();
        for selected in [false, true] {
            let curves = sketch
                .curves
                .iter()
                .filter(|entry| self.selected(entry.id) == selected);
            for entry in curves {
                let Some(polyline) = sketch.flatten(&entry.curve) else {
                    continue;
                };
                let linked = linked.contains(&entry.id);
                let (color, width) = self.curve_style(entry, selected, linked, colors);
                let (kept, cut) = match (cut_back.get(&entry.id), &polyline[..]) {
                    (Some(&kept), &[start, end]) => {
                        let (kept, cut) = cut_line(start, end, kept);
                        (kept.map(|kept| kept.to_vec()), cut)
                    }
                    _ => (Some(polyline), Vec::new()),
                };
                if let Some(polyline) = kept {
                    let style = line(color, width, entry.construction);
                    layer.polyline(Space::Sketch, &polyline, style);
                }
                for polyline in cut {
                    layer.polyline(Space::Sketch, &polyline, line(color, width, true));
                }
            }
        }
        let mut arrows = Vec::with_capacity(sketch.dimensions.len());
        for entry in &sketch.dimensions {
            let Some(lines) = self.dimension_lines(entry) else {
                continue;
            };
            let color = self.dimension_color(entry, colors);
            draw_lines(&mut layer, &lines, color);
            arrows.push(Arrows {
                id: entry.id,
                arrows: lines.arrows,
                color,
            });
        }
        let mut style = dot(
            POINT_RADIUS,
            colors.point_fill,
            self.builtin_color(Id::ORIGIN, colors),
        );
        style.fixed = true;
        layer.point(DVec2::ZERO, style);
        self.spline_aids(&mut layer, colors);
        // Handles' tips and ends and control points, in the handles'
        // colour.
        let mut tips = sketch.tips();
        for (_, spline) in sketch.splines() {
            tips.extend(spline.handles.iter().map(|handle| handle.end));
            if spline.kind == SplineKind::Control {
                tips.extend(spline.points.iter().copied());
            }
        }
        for selected in [false, true] {
            let points = sketch
                .points
                .iter()
                .filter(|point| self.selected(point.id) == selected);
            for point in points {
                let linked = linked.contains(&point.id);
                let tip = tips.contains(&point.id);
                let style = self.point_style(point.id, selected, linked, tip, colors);
                layer.point(point.at, style);
            }
        }
        (layer, arrows)
    }

    /// The colour of the origin or an axis `id`: selected, in a conflict,
    /// or else their own colour.
    fn builtin_color(&self, id: Id, colors: SketchColors) -> Color {
        if self.selected(id) {
            colors.selected
        } else if self.states.conflicts.contains(&id) {
            colors.conflict
        } else {
            colors.axis
        }
    }

    /// The colour and width of the curve `entry`, `selected` or not and
    /// made by a link (`linked`) or not. The order of its states: selected
    /// over red (in a conflict or failing; unlike a glyph, whose conflict
    /// shows over its selection), red over linked, linked over
    /// construction, construction over fixed; and that faded while it
    /// waits on the solver.
    fn curve_style(
        &self,
        entry: &CurveEntry,
        selected: bool,
        linked: bool,
        colors: SketchColors,
    ) -> (Color, f32) {
        let id = entry.id;
        let red = self.states.red(id);
        let (color, width) = if selected {
            (colors.selected, SELECTED_WIDTH)
        } else if linked && !red {
            (colors.link, CURVE_WIDTH)
        } else if entry.construction && !red {
            (colors.construction, CURVE_WIDTH)
        } else {
            (self.states.color(id, colors), CURVE_WIDTH)
        };
        (self.states.faded(id, color), width)
    }

    /// How the point `id` is drawn, `selected` or not, made by a link
    /// (`linked`) or not and a handle's tip or end or a control point
    /// (`tip`) or not: filled in the selection's colour if selected; its
    /// rim linked unless red, else red or fixed, else a tip's, else
    /// plain; filled as fixed when fixed and not selected; and faded while
    /// it waits on the solver.
    fn point_style(
        &self,
        id: Id,
        selected: bool,
        linked: bool,
        tip: bool,
        colors: SketchColors,
    ) -> PointStyle {
        let states = &self.states;
        let fill = if selected {
            colors.selected
        } else {
            colors.point_fill
        };
        let rim = if linked && !states.red(id) {
            colors.link
        } else if states.red(id) || states.fixed.contains(&id) {
            states.color(id, colors)
        } else if tip {
            colors.spline_handle
        } else {
            colors.point
        };
        let mut style = dot(POINT_RADIUS, states.faded(id, fill), states.faded(id, rim));
        style.fixed = !selected && states.fixed.contains(&id);
        style
    }

    /// What shows how splines are shaped: each handle as a line from its
    /// end through its fit point to its tip, in the handles' colour, or
    /// the selection's with its spline or itself
    /// ([`Selectable::HandleLine`]); and
    /// of a spline by control points selected, or with any of its control
    /// points selected, its control polygon, dashed in the handles'
    /// colour, as its points are. Fit points without a handle show none.
    fn spline_aids(&self, layer: &mut SketchLayer, colors: SketchColors) {
        let sketch = self.sketch;
        let at = |id: Id| sketch.place(id);
        for entry in &sketch.curves {
            let Curve::Spline(spline) = &entry.curve else {
                continue;
            };
            let selected = self.selected(entry.id);
            let color = if selected {
                colors.selected
            } else {
                colors.spline_handle
            };
            for handle in &spline.handles {
                if let Some(arms) = handle_arms(sketch, handle.tip) {
                    let line_selected =
                        self.selection.contains(&Selectable::HandleLine(handle.tip));
                    let color = if line_selected {
                        colors.selected
                    } else {
                        color
                    };
                    let style = line(color, HANDLE_WIDTH, false);
                    layer.polyline(Space::Sketch, &arms, style);
                }
            }
            if spline.kind == SplineKind::Control
                && (selected || spline.points.iter().any(|&id| self.selected(id)))
            {
                let mut polygon: Vec<DVec2> =
                    spline.points.iter().filter_map(|&id| at(id)).collect();
                if spline.closed
                    && let Some(&first) = polygon.first()
                {
                    polygon.push(first);
                }
                let style = line(colors.spline_handle, HANDLE_WIDTH, true);
                layer.polyline(Space::Sketch, &polygon, style);
            }
        }
    }

    /// The colour of the dimension `entry`, by its state and whether it's
    /// a reference.
    pub(super) fn dimension_color(&self, entry: &DimensionEntry, colors: SketchColors) -> Color {
        self.look(entry.id)
            .dimension_color(colors, entry.dimension.driving)
    }

    /// Where the label of the dimension `entry` is, in sketch
    /// coordinates, as far as it's dragged.
    pub(super) fn label_at(&self, entry: &DimensionEntry) -> Option<DVec2> {
        let dimension = &entry.dimension;
        let dragged = self
            .label_drag
            .filter(|&(id, _)| id == entry.id)
            .map_or(DVec2::ZERO, |(_, by)| by);
        Some(self.sketch.anchor(&dimension.measure)? + dimension.label + dragged)
    }

    /// How the dimension `entry` is drawn, see [`dimensions::lines`].
    pub(super) fn dimension_lines(&self, entry: &DimensionEntry) -> Option<dimensions::Lines> {
        let dimension = &entry.dimension;
        let label = self.label_at(entry)?;
        dimensions::lines(self.sketch, &dimension.measure, dimension.side, label)
    }

    /// What changes as the cursor moves, seen through `projector` if the
    /// viewport shows anything, with `modifiers` held, and where what's
    /// dragged snaps, `dragged` ([`Sketching::dragged`]): each overlay
    /// below, in order, the later over the earlier.
    fn live_layer(
        &self,
        input: &Input,
        projector: Option<&Projector>,
        colors: SketchColors,
        dimension_arrows: &[Arrows],
        modifiers: Modifiers,
        dragged: Option<Snap>,
    ) -> SketchLayer {
        let mut layer = SketchLayer::default();
        draw_box(&mut layer, input, colors);
        let aim = self.aim_of(input, projector, modifiers);
        self.draw_hover(&mut layer, &aim, colors);
        let drawing = self.tool.is_some_and(|tool| tool.tool.draws());
        // Where a drawing tool's click, or a point or rim dragged, snaps is
        // marked round the cursor, whatever it snapped to.
        let spot = aim
            .snap
            .filter(|_| drawing)
            .or(dragged)
            .filter(Snap::snapped);
        if let Some(spot) = spot {
            layer.point(spot.at, snap_disc(colors.point));
        }
        self.draw_dimension_marks(&mut layer, input, projector, colors, dimension_arrows);
        self.draw_tool(&mut layer, input, &aim, colors);
        self.draw_placing(&mut layer, aim.at, projector, colors, modifiers);
        layer
    }

    /// Where the cursor is and what it's on, see [`Aim`].
    fn aim_of(&self, input: &Input, projector: Option<&Projector>, modifiers: Modifiers) -> Aim {
        let cursor = input
            .cursor
            .zip(projector)
            .and_then(|(pixel, projector)| projector.cursor(pixel));
        // Where the cursor snaps, or with it off the sketch, a shape with
        // fields where it was last aimed, which `Enter` places.
        let (snap, at) = match cursor {
            Some(cursor) => (self.snap(cursor, modifiers), Some(cursor.at)),
            None if self.aiming() => (self.snap, self.aim),
            None => (None, None),
        };
        // Not while the button held down drags.
        let still = input.press.is_none_or(|press| !press.moved);
        // A drawing tool shows what it snaps to.
        let hover = if self.tool.is_some_and(|tool| tool.tool.draws()) {
            snap.and_then(|snap| snap.highlighted())
                .map(Selectable::Item)
        } else {
            input.hover.filter(|_| still)
        };
        Aim {
            cursor,
            snap,
            at,
            still,
            hover,
        }
    }

    /// What's hovered, in the hover colour: under the cursor, and what a
    /// list's row, a glyph or a label hovered stands for; and where a
    /// point added to the spline hovered would go.
    fn draw_hover(&self, layer: &mut SketchLayer, aim: &Aim, colors: SketchColors) {
        // A constraint or a dimension by what it ties together, and a
        // dimension itself.
        let listed = self.hovered.into_iter().flat_map(|target| match target {
            Selectable::Item(id) => tied_items(self.sketch, id)
                .into_iter()
                .map(Selectable::Item)
                .collect(),
            _ => vec![target],
        });
        for id in aim.hover.into_iter().chain(listed) {
            self.highlight(layer, id, colors.hovered);
        }
        // Without a tool: a double-click, or a click while adding points.
        let ghost = (self.tool.is_none())
            .then(|| self.spline_hovered(aim.hover))
            .flatten()
            .zip(aim.cursor)
            .and_then(|(spline, cursor)| self.sketch.nearest_on(spline, cursor.at));
        if let Some(ghost) = ghost {
            layer.point(ghost, ring(colors.hovered));
        }
    }

    /// The hovered dimension's lines, the dimensions' arrowheads
    /// (`dimension_arrows`, a hovered dimension's in the hover colour) and
    /// the rings marking near misses, the last two depending on the zoom.
    fn draw_dimension_marks(
        &self,
        layer: &mut SketchLayer,
        input: &Input,
        projector: Option<&Projector>,
        colors: SketchColors,
        dimension_arrows: &[Arrows],
    ) {
        let hovered =
            (self.hovered.and_then(Selectable::item)).and_then(|id| self.sketch.dimension(id));
        if let Some(lines) = hovered.and_then(|entry| self.dimension_lines(entry)) {
            draw_lines(layer, &lines, colors.hovered);
        }
        if let Some(projector) = projector {
            for dimension in dimension_arrows {
                let color = if self.hovered == Some(dimension.id.into()) {
                    colors.hovered
                } else {
                    dimension.color
                };
                arrows(layer, projector, &dimension.arrows, color);
            }
            let ring = ring(colors.near_miss);
            for miss in self.near_misses(input, projector) {
                layer.point((miss.a + miss.b) / 2.0, ring);
            }
        }
    }

    /// The tool's overlay: Dimension's picks, a shape tool's preview, or a
    /// drawing tool's shape to where the cursor snaps, and the snap's
    /// guide.
    fn draw_tool(&self, layer: &mut SketchLayer, input: &Input, aim: &Aim, colors: SketchColors) {
        let under = (input.hover.and_then(Selectable::item))
            .filter(|_| aim.still)
            .zip(aim.cursor);
        match self.tool {
            Some(tool) if tool.tool == Tool::Dimension => {
                for &id in &tool.picked {
                    self.highlight(layer, id, colors.selected);
                }
            }
            Some(tool) if !tool.tool.draws() => {
                self.shape_preview(layer, tool, under, aim.at, colors);
            }
            Some(tool) => {
                if let Some((snap, cursor)) = aim.snap.zip(aim.cursor)
                    && let Some(guide) = snap.guide(self.sketch, tool, cursor.pixel)
                {
                    layer.polyline(Space::Sketch, &guide, line(colors.guide, GUIDE_WIDTH, true));
                }
                let at = aim.snap.map(|snap| snap.at).or(aim.at);
                preview(layer, tool, at, colors);
            }
            None => {}
        }
    }

    /// The dimension being placed, with the cursor `at` a place, in the
    /// driven colour while the reference modifier is held.
    fn draw_placing(
        &self,
        layer: &mut SketchLayer,
        at: Option<DVec2>,
        projector: Option<&Projector>,
        colors: SketchColors,
        modifiers: Modifiers,
    ) {
        let Some((measure, side, label)) = self.placing(at) else {
            return;
        };
        // With the reference modifier held, a click places a reference:
        // it shows in their colour until then.
        let reference = self.value.is_none() && Held::REFERENCE.is_held(modifiers);
        let color = if reference {
            colors.driven
        } else {
            colors.preview
        };
        if let Some(lines) = dimensions::lines(self.sketch, &measure, side, label) {
            draw_lines(layer, &lines, color);
            if let Some(projector) = projector {
                arrows(layer, projector, &lines.arrows, color);
            }
        }
    }

    /// What a shape tool, `tool`, would do with the item `under` the
    /// cursor, if there is one, or the cursor `at` a place: Trim's piece to
    /// take away, over the curve highlighted, in the conflict colour;
    /// Extend's extension from the end nearer the cursor, in the preview
    /// colour; Offset's chain under the cursor highlighted, then once
    /// picked, as selected, with its copy through the cursor (or as far
    /// off as the distance typed, on the cursor's side) in the preview
    /// colour; the Mirror tool's picks as selected and, once it's choosing
    /// the line, their mirror images in the line under the cursor; Fillet's
    /// and Chamfer's corner under the cursor, its lines highlighted, then
    /// once picked, as selected, with the fillet or chamfer through the
    /// cursor (or as the values typed make it) in the preview colour.
    fn shape_preview(
        &self,
        layer: &mut SketchLayer,
        tool: &Drawing,
        under: Option<(Id, Cursor)>,
        at: Option<DVec2>,
        colors: SketchColors,
    ) {
        let sketch = self.sketch;
        match tool.tool {
            Tool::Trim => {
                if let Some((id, cursor)) = under
                    && let Some(piece) = sketch.trim_piece(id, cursor.at)
                {
                    let style = line(colors.conflict, HOVERED_WIDTH, false);
                    layer.polyline(Space::Sketch, &piece, style);
                }
            }
            Tool::Extend => {
                let extension = under.and_then(|(id, cursor)| {
                    let end = sketch.nearer_end(id, cursor.at)?;
                    sketch.extension(id, end, 4.0 * f64::from(MAX_COORD))
                });
                if let Some(extension) = extension {
                    let style = line(colors.preview, CURVE_WIDTH, false);
                    layer.polyline(Space::Sketch, &extension, style);
                }
            }
            placing if placing.places() && tool.picked.is_empty() => {
                // The rest of what a click would pick: the chain the curve
                // hovered is in, the lines of the corner hovered.
                if let Some((hovered, cursor)) = under {
                    for id in placing.pick(sketch, hovered, cursor.at) {
                        if id != hovered {
                            self.highlight(layer, id.into(), colors.hovered);
                        }
                    }
                }
            }
            placing if placing.places() => {
                for &id in &tool.picked {
                    self.highlight(layer, id, colors.selected);
                }
                let style = line(colors.preview, CURVE_WIDTH, false);
                for polyline in at.map(|at| placed(tool, sketch, at)).unwrap_or_default() {
                    layer.polyline(Space::Sketch, &polyline, style);
                }
            }
            Tool::Mirror => {
                for &id in &tool.picked {
                    self.highlight(layer, id, colors.selected);
                }
                let about = under
                    .filter(|_| tool.about)
                    .and_then(|(id, _)| sketch.line(id));
                if let Some((a, b)) = about.filter(|(a, b)| a != b) {
                    let reflect = |p: DVec2| 2.0 * foot(p, a, b) - p;
                    let style = line(colors.preview, CURVE_WIDTH, false);
                    for id in tool_items(tool) {
                        if let Some(entry) = sketch.curve(id)
                            && let Some(polyline) = sketch.flatten(&entry.curve)
                        {
                            let image: Vec<_> = polyline.into_iter().map(reflect).collect();
                            layer.polyline(Space::Sketch, &image, style);
                        } else if let Some(point) = sketch.point(id) {
                            let color = colors.preview;
                            layer.point(reflect(point.at), dot(POINT_RADIUS, color, color));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// The dimension being placed, if one is, and where its label is: the
    /// one the value field is open on, or with the cursor `at` a place on
    /// the sketch, what the Dimension tool measures of what's picked.
    fn placing(&self, at: Option<DVec2>) -> Option<(Measure, Side, DVec2)> {
        if let Some(field) = self.value {
            return match field.target {
                ValueTarget::New {
                    measure,
                    side,
                    label,
                } => Some((
                    measure.clone(),
                    *side,
                    self.sketch.anchor(measure)? + *label,
                )),
                ValueTarget::Dimension(_) | ValueTarget::Field(_) => None,
            };
        }
        let tool = self.tool.filter(|tool| tool.tool == Tool::Dimension)?;
        let at = at?;
        let (measure, side) = dimension::measure(self.sketch, &tool.picked, at, tool.switched)?;
        Some((measure, side, at))
    }

    /// Draws `target` highlighted in `color`, as under the cursor, into
    /// `layer`: a point or a curve, the origin and axes too, a spline's
    /// handle as a line.
    pub(super) fn highlight(&self, layer: &mut SketchLayer, target: Selectable, color: Color) {
        let id = match target {
            Selectable::Item(id) => id,
            Selectable::HandleLine(tip) => {
                if let Some(arms) = handle_arms(self.sketch, tip) {
                    layer.polyline(Space::Sketch, &arms, line(color, HOVERED_WIDTH, false));
                }
                return;
            }
        };
        if let Some(axis) = axis(id) {
            for half in axis {
                layer.axis_polyline(Space::Sketch, &half, line(color, HOVERED_WIDTH, false));
            }
        } else if let Some(point) = self.sketch.point(id) {
            let style = dot(HOVERED_POINT_RADIUS, color, color);
            layer.point(point.at, style);
        } else if let Some(entry) = self.sketch.curve(id)
            && let Some(polyline) = self.sketch.flatten(&entry.curve)
        {
            let style = line(color, HOVERED_WIDTH, entry.construction);
            layer.polyline(Space::Sketch, &polyline, style);
        }
    }
}

/// Where the cursor is and what it's on, for [`Sketching::live_layer`].
struct Aim {
    /// The cursor, on the sketch's plane.
    cursor: Option<Cursor>,
    /// Where it snaps, or with it off the sketch, where it was last aimed.
    snap: Option<Snap>,
    /// Where it is, or was last aimed.
    at: Option<DVec2>,
    /// Whether the button held down, if any, hasn't dragged.
    still: bool,
    /// What's under it: with a drawing tool, what it snaps to.
    hover: Option<Selectable>,
}

/// The box being dragged, if one is.
fn draw_box(layer: &mut SketchLayer, input: &Input, colors: SketchColors) {
    if let Some(press) = input.press
        && let Some(area) = press.area()
    {
        let touching = BoxMode::dragged(press.from, press.to) == BoxMode::Touching;
        let corners = area.corners();
        layer.fill(Space::Screen, [&corners[..]], srgba(colors.box_fill));
        let outline = [corners.as_slice(), &corners[..1]].concat();
        let style = line(colors.box_line, BOX_LINE_WIDTH, touching);
        layer.polyline(Space::Screen, &outline, style);
    }
}

/// Everything the base layer depends on. Not the camera: the renderer
/// projects the layer.
#[derive(Debug, Clone)]
pub(super) struct Drawn {
    sketch: Sketch,
    selection: BTreeSet<Selectable>,
    states: States,
    colors: SketchColors,
    label_drag: Option<(Id, DVec2)>,
    /// Where the failing curves are placed in the world.
    placement: Placement,
    /// Compared by pointer: the app finds them again only when the sketch
    /// changes.
    profiles: Option<Arc<Profiles>>,
    comb: bool,
    /// The point dropped on the other end of its curve, and that end, see
    /// [`Sketching::drag_closing`]: the sketch drawn is as the drop leaves
    /// it.
    closing: Option<(Id, Id)>,
}

impl Drawn {
    /// What `sketching` draws in `colors`, `closing` a curve or not.
    fn of(sketching: &Sketching, colors: SketchColors, closing: Option<(Id, Id)>) -> Self {
        Drawn {
            sketch: sketching.sketch.clone(),
            selection: sketching.selection.clone(),
            states: sketching.states.clone(),
            colors,
            label_drag: sketching.label_drag,
            placement: sketching.placement,
            profiles: sketching.profiles.cloned(),
            comb: sketching.comb,
            closing,
        }
    }

    /// Whether it's what `sketching` draws in `colors`, `closing` a curve
    /// or not: whether the layer built from it is still current.
    fn matches(
        &self,
        sketching: &Sketching,
        colors: SketchColors,
        closing: Option<(Id, Id)>,
    ) -> bool {
        // Destructured, so a field added fails to compile until it's
        // compared.
        let Drawn {
            sketch,
            selection,
            states,
            colors: drawn_colors,
            label_drag,
            placement,
            profiles,
            comb,
            closing: drawn_closing,
        } = self;
        sketch == sketching.sketch
            && selection == sketching.selection
            && *states == sketching.states
            && *drawn_colors == colors
            && *label_drag == sketching.label_drag
            && *placement == sketching.placement
            && profiles.as_ref().map(Arc::as_ptr) == sketching.profiles.map(Arc::as_ptr)
            && *comb == sketching.comb
            && *drawn_closing == closing
    }
}

/// The items `tool` has picked, by their ids: what Offset and Mirror
/// work on.
pub(super) fn tool_items(tool: &Drawing) -> Vec<Id> {
    tool.picked.iter().map(|target| target.id()).collect()
}

/// The handle whose tip is `tip` as drawn: from its end through its fit
/// point to its tip. `None` if `tip` is no handle's tip.
pub(crate) fn handle_arms(sketch: &Sketch, tip: Id) -> Option<[DVec2; 3]> {
    let (_, handle) = sketch.handle(tip)?;
    let at = |id| sketch.place(id);
    Some([at(handle.end)?, at(handle.at)?, at(handle.tip)?])
}

/// The axis `id` names, if it names one, as drawn: as far as a sketch
/// reaches either way, as two segments out from the origin. The renderer
/// cuts a segment to the near plane and the viewport by mixing its ends,
/// which is only as exact as its start is near the part that shows: one
/// starting that far away lands pixels off the grid's axis line, more as
/// it's zoomed in and in perspective.
pub(super) fn axis(id: Id) -> Option<[[DVec2; 2]; 2]> {
    let reach = f64::from(MAX_COORD);
    let along = match id {
        Id::X_AXIS => DVec2::X,
        Id::Y_AXIS => DVec2::Y,
        _ => return None,
    };
    Some([[DVec2::ZERO, -along * reach], [DVec2::ZERO, along * reach]])
}

/// Draws the curvature combs `combs` (see [`Sketch::curvature_comb`]) in
/// `color` into `layer`, a pixel at the target being `pixel` sketch units:
/// each tooth from its place, away from where the spline turns, the
/// longest of a comb [`COMB_LENGTH`] pixels and the rest in proportion,
/// and a line through their tips. A spline that doesn't turn has no
/// teeth.
pub(super) fn combs(layer: &mut SketchLayer, combs: &[Vec<[DVec2; 2]>], pixel: f64, color: Color) {
    let style = line(color, COMB_WIDTH, false);
    for teeth in combs {
        let most = teeth
            .iter()
            .map(|[_, curving]| curving.length())
            .fold(0.0, f64::max);
        if !(most > 0.0 && most.is_finite()) {
            continue;
        }
        let scale = COMB_LENGTH * pixel / most;
        let tips: Vec<DVec2> = teeth
            .iter()
            .map(|&[place, curving]| place - curving * scale)
            .collect();
        for (&[place, _], &tip) in teeth.iter().zip(&tips) {
            layer.polyline(Space::Sketch, &[place, tip], style);
        }
        layer.polyline(Space::Sketch, &tips, style);
    }
}

/// Draws the lines of `lines`, a dimension's, in `color` into `layer`.
pub(super) fn draw_lines(layer: &mut SketchLayer, lines: &dimensions::Lines, color: Color) {
    let style = line(color, DIMENSION_WIDTH, false);
    for polyline in &lines.lines {
        layer.polyline(Space::Sketch, polyline, style);
    }
}

/// Draws the arrowheads `arrows`, a dimension's (see
/// [`dimensions::Lines::arrows`]), in `color` into `layer`: each a fixed
/// size on the screen, seen through `projector`, pointing from its tail to
/// its tip where they show. One whose tip and tail show at one place, or
/// behind the eye, is left out.
pub(super) fn arrows(
    layer: &mut SketchLayer,
    projector: &Projector,
    arrows: &[[DVec2; 2]],
    color: Color,
) {
    for &[tip, from] in arrows {
        let (Some(tip), Some(from)) = (projector.project(tip), projector.project(from)) else {
            continue;
        };
        let Some(along) = (tip - from).try_normalize() else {
            continue;
        };
        let base = tip - along * ARROW_LENGTH;
        let across = along.perp() * ARROW_HALF_WIDTH;
        layer.triangle(
            Space::Screen,
            [tip, base + across, base - across],
            srgba(color),
        );
    }
}

/// The shape `tool` is drawing, with the cursor `at` its next point if
/// it's over the sketch, and the points placed so far.
fn preview(layer: &mut SketchLayer, tool: &Drawing, at: Option<DVec2>, colors: SketchColors) {
    let color = if tool.construction {
        colors.construction
    } else {
        colors.preview
    };
    if let Some(at) = at {
        let outline = typed::outline(tool, at);
        let style = line(color, CURVE_WIDTH, tool.construction);
        match &outline {
            Some(outline) => {
                if let Some(construction) = outline.construction() {
                    let style = line(colors.construction, CURVE_WIDTH, true);
                    layer.polyline(Space::Sketch, &construction, style);
                }
                for polyline in outline.polylines() {
                    layer.polyline(Space::Sketch, &polyline, style);
                }
            }
            // Its third point on the line through its ends.
            None if tool.tool == Tool::Arc && tool.typed.is_empty() => {
                if let &[start, end] = tool.placed.as_slice() {
                    layer.polyline(Space::Sketch, &[start, end], style);
                }
            }
            None => {}
        }
        // Where the click places its point, as the values typed hold it.
        let placed = outline
            .as_ref()
            .filter(|_| !tool.typed.is_empty())
            .map_or(at, |outline| outline.aimed(at));
        layer.point(placed, dot(POINT_RADIUS, color, color));
    }
    for &placed in &tool.placed {
        layer.point(placed, dot(POINT_RADIUS, colors.point_fill, color));
    }
}

/// What `tool`, one that [`places`](Tool::places) and has picked, would
/// make in `sketch` with the cursor `at` a place, as polylines: Offset's
/// copy through the cursor, or as far off as the distance typed on the
/// cursor's side; Fillet's or Chamfer's through the cursor, or as the
/// values typed make it (see [`typed::corner_outline`]). Empty where it
/// makes nothing.
pub(super) fn placed(tool: &Drawing, sketch: &Sketch, at: DVec2) -> Vec<Vec<DVec2>> {
    if tool.tool == Tool::Offset {
        let chain = tool_items(tool);
        let copy = sketch.offset_side(&chain, at).and_then(|(reach, side)| {
            let typed = tool.value_in(Field::Distance).map(|value| value.value);
            let distance = typed.unwrap_or(reach);
            sketch.offset_preview(&chain, distance, side).ok()
        });
        return copy.unwrap_or_default();
    }
    typed::corner_outline(tool, sketch, at).map_or_else(Vec::new, |made| made.polylines())
}

/// A point `radius` pixels across, `fill` inside a `rim`, not filled as
/// fixed.
pub(super) fn dot(radius: f32, fill: Color, rim: Color) -> PointStyle {
    PointStyle {
        radius,
        rim_width: POINT_RIM,
        rim: srgba(rim),
        fill: srgba(fill),
        fixed: false,
    }
}

/// The ring marking a near miss, in `color`: a point with nothing inside
/// its rim.
pub(super) fn ring(color: Color) -> PointStyle {
    PointStyle {
        radius: NEAR_MISS_RADIUS,
        rim_width: NEAR_MISS_WIDTH,
        rim: srgba(color),
        fill: Srgba::default(),
        fixed: false,
    }
}

/// The disc marking a spot snapped to, a faint wash of `color` with no
/// rim.
pub(super) fn snap_disc(color: Color) -> PointStyle {
    let wash = srgba(color.scale_alpha(SNAP_WASH));
    PointStyle {
        radius: SNAP_RADIUS,
        rim_width: 0.0,
        rim: wash,
        fill: wash,
        fixed: false,
    }
}
