//! The sketch being edited, in the viewport: the left button's work on it
//! (selecting, box selecting, dragging geometry and drawing with the
//! tools), `Esc` while the button is held, the cursor, and the layers the
//! renderer draws of it, and the constraints' glyphs over it. See
//! `agents/sketch.md`.

mod dimensions;
mod glyphs;

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::Arc;

use glam::DVec2;
use iced::keyboard::{self, Modifiers, key::Named};
use iced::mouse;
use iced::time::{Duration, Instant};
use iced::widget::shader::Action;
use iced::widget::{MouseArea, column, container, mouse_area, row, text};
use iced::{Alignment, Color, Element, Event, Padding, Point, Rectangle, Vector};
use varde_document::{MAX_COORD, Placement};
use varde_expr::LengthUnit;
use varde_kernel::RenderLines;
use varde_render::{Camera, GridPlane, LineStyle, PointStyle, SketchLayer, Space, Srgba};
use varde_sketch::{
    Curve, DimensionEntry, Id, Kind, Measure, NearMiss, Profiles, Region, Selectable, Side, Sketch,
    SplineKind, cut_line, foot,
};

use crate::document::tied_items;
use crate::hit::{self, BoxMode, ScreenBox};
use crate::overlaps::{self, OverlapItems, Overlaps};
use crate::projection::{Cursor, Projector};
use crate::shortcut::Held;
use crate::snap::{self, Snap};
use crate::theme::{self, SketchColors};
use crate::typed::{self, Field};
use crate::{
    ActiveTool, ConstraintKind, Edit, Look, Message, SketchState, Tool, ToolClick, ValueField,
    ValueTarget, dimension, icons, panels,
};

/// How near the cursor an item has to be to be under it, in pixels.
const HIT_TOLERANCE: f64 = 6.0;
/// How near the cursor a corner has to be for Fillet and Chamfer to take
/// it, in pixels: a point, but all they look for.
const CORNER_TOLERANCE: f64 = 12.0;
/// How far the cursor moves with the button held before it's a drag
/// rather than a click, in pixels.
const DRAG_DISTANCE: f64 = 4.0;
const CURVE_WIDTH: f32 = 1.8;
const SELECTED_WIDTH: f32 = 2.6;
const HOVERED_WIDTH: f32 = 3.0;
const POINT_RADIUS: f32 = 3.6;
const HOVERED_POINT_RADIUS: f32 = 5.1;
const POINT_RIM: f32 = 1.2;
/// The dashes of construction geometry, and of a box touching what it
/// selects: on and off, in pixels.
const DASH: [f32; 2] = [6.0, 4.0];
/// How wide the outline of the box dragged to select is, in pixels.
const BOX_LINE_WIDTH: f32 = 1.0;
/// How soon a second press of a tool has to follow the first, near where
/// it was, to make a double-click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// How opaque what waits on the solver is, against what's committed.
const PENDING_ALPHA: f32 = 0.45;
/// The side of a constraint's glyph's icon, and the room around it on its
/// chip, in pixels.
const GLYPH_ICON: f32 = 16.0;
const GLYPH_PADDING: f32 = 2.0;
/// The side of a snap's glyph's icon, in pixels.
const SNAP_ICON: f32 = 12.0;
/// Where a glyph is centred from where its anchor shows: up and to the
/// right, beside what it's on rather than over it. In pixels.
pub(crate) const GLYPH_OFFSET: Vector = Vector::new(13.0, -13.0);
/// How wide dimensions' lines are, in pixels.
const DIMENSION_WIDTH: f32 = 1.0;
/// How long a dimension's arrowhead is, and how wide either side of its
/// line, in pixels.
const ARROW_LENGTH: f64 = 8.0;
const ARROW_HALF_WIDTH: f64 = 2.8;
/// How wide the origin's axes are, and the guides of a snap, in pixels.
const AXIS_WIDTH: f32 = 1.0;
const GUIDE_WIDTH: f32 = 1.0;
/// Where the glyph of a snap is centred from the cursor: down and to the
/// right, clear of it. In pixels.
pub(crate) const SNAP_OFFSET: Vector = Vector::new(16.0, 16.0);
/// The size of a dimension's label's text, and the room around it on its
/// chip, in pixels.
const LABEL_SIZE: f32 = 12.0;
const LABEL_PADDING: [u16; 2] = [1, 4];
/// Where a drawing tool's fields are from the cursor: their top left
/// corner, down and to the right, below the snap's glyph. In pixels.
pub(crate) const FIELDS_OFFSET: Vector = Vector::new(12.0, 28.0);
/// How wide a field's name is beside it, in pixels.
const FIELD_NAME_WIDTH: f32 = 56.0;
/// How near open ends have to be to be marked as almost meeting, in
/// pixels at the target: a gap too small to see.
const NEAR_MISS_GAP: f64 = 6.0;
/// The ring marking open ends that almost meet: its radius to the
/// outside and how wide it is, in pixels.
const NEAR_MISS_RADIUS: f32 = 7.0;
const NEAR_MISS_WIDTH: f32 = 1.5;
/// The disc marking where a point snaps ([`Snap::snapped`]), drawing or
/// dragged: its radius, in pixels, wide enough to show round the cursor
/// over it, and how opaque it is, of the points' colour.
const SNAP_RADIUS: f32 = 15.0;
const SNAP_WASH: f32 = 0.15;
/// How wide a spline's handles, its control polygon and its curvature
/// comb are drawn, in pixels.
const HANDLE_WIDTH: f32 = 1.0;
const COMB_WIDTH: f32 = 1.0;
/// How long the longest tooth of a curvature comb is, in pixels at the
/// target: the rest in proportion to their curvature.
const COMB_LENGTH: f64 = 48.0;

/// The sketch being edited, as the viewport shows it.
#[derive(Debug, Clone)]
pub(crate) struct Sketching<'a> {
    sketch: &'a Sketch,
    placement: Placement,
    selection: &'a BTreeSet<Selectable>,
    /// The tool in use, if the sketch is editable and one is.
    tool: Option<ActiveTool<'a>>,
    /// Whether geometry can be dragged.
    editable: bool,
    /// How items are drawn by their state.
    states: States,
    /// The item hovered in a list or by its glyph, highlighted: a
    /// constraint by what it ties together.
    hovered: Option<Selectable>,
    /// Whether the constraints' glyphs are shown.
    glyphs: bool,
    /// The design's units, which dimensions show in.
    units: LengthUnit,
    /// The value field, if it's open.
    value: Option<ValueField<'a>>,
    /// The dimension whose label is grabbed, if one is, and how far it's
    /// been dragged.
    label_drag: Option<(Id, DVec2)>,
    /// Where the drawing tool's next click snaps, as the app last heard,
    /// for its glyph.
    snap: Option<Snap>,
    /// Where the drawing tool's next click goes, as the app last heard,
    /// for its fields.
    aim: Option<DVec2>,
    /// The regions the sketch encloses, if the app has found them.
    profiles: Option<&'a Arc<Profiles>>,
    /// Whether the curvature comb of the splines selected shows.
    comb: bool,
}

/// The items drawn otherwise than free, by why.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct States {
    /// Points and curves the constraints fix, drawn darker, points filled.
    fixed: BTreeSet<Id>,
    /// Constraints in conflict and what they tie together, in red.
    conflicts: BTreeSet<Id>,
    /// Curves a failing feature using the sketch names, in red within
    /// the halo the failures' geometry has (see [`Base::failing`]).
    failing: BTreeSet<Id>,
    /// Items of edits waiting on the solver, faded.
    pending: BTreeSet<Id>,
}

/// What the viewport keeps of the sketch between events and frames.
#[derive(Default)]
pub(crate) struct Input {
    /// Where the cursor is over the viewport, if it is, in its pixels.
    cursor: Option<DVec2>,
    /// The item under the cursor.
    hover: Option<Selectable>,
    /// The left button held down, if it is.
    press: Option<Press>,
    /// When and where a tool was last pressed, unless that ended a
    /// double-click, to tell the next press is one.
    last_click: Option<(Instant, DVec2)>,
    /// When and where a click without a tool last selected something,
    /// unless that ended a double-click: a second on a spline adds a
    /// point to it.
    last_select: Option<(Instant, DVec2)>,
    /// Where the cursor last was in the viewport's pixels, over it or not:
    /// where a label is when it's grabbed, as its own widget takes the
    /// press.
    pointer: Option<DVec2>,
    /// The label grabbed, as the viewport follows it, once the cursor has
    /// moved.
    label: Option<LabelPress>,
    /// Whether the left button is held down with a tool that clicks where
    /// it's let go, dragged or not: Offset placing its copy.
    releasing: bool,
    /// The snap last sent to the app ([`Look::Snap`]), so it's sent again
    /// only when it changes.
    snap: Option<Snap>,
    /// The region under the cursor as it last moved, so it's drawn again
    /// when that changes.
    region: Option<usize>,
    /// The near misses last found, and for what, so they're only paired
    /// again when the profiles or the zoom change.
    near: RefCell<Option<Near>>,
    /// The base layer last built, and what it was built from, so it's
    /// only built, and uploaded, again when that changes.
    base: RefCell<Option<Base>>,
}

/// The base layer as built, with what it was built from and what the
/// live layer needs of it.
struct Base {
    drawn: Drawn,
    layer: Arc<SketchLayer>,
    /// The dimensions' arrowheads, made each frame where they show from
    /// these, so that their lines aren't worked out again every frame.
    arrows: Vec<Arrows>,
    /// The curvature combs of the splines selected, while they show, for
    /// the live layer to draw scaled to the view (see
    /// [`Sketch::curvature_comb`]).
    combs: Vec<Vec<[DVec2; 2]>>,
    /// The failing curves ([`States::failing`]) placed in the world, if
    /// any: drawn as failures' geometry's halo alone, under the base
    /// layer's red curves at their own width. A new `Arc` with each base
    /// layer, so the renderer uploads them again only then.
    failing: Option<Arc<RenderLines>>,
}

/// The open ends of `profiles` within `gap` of each other.
struct Near {
    profiles: Arc<Profiles>,
    gap: f64,
    misses: Vec<NearMiss>,
}

/// A dimension's arrowheads, in sketch coordinates (see
/// [`dimensions::Lines::arrows`]), and their colour.
#[derive(Debug, Clone, PartialEq)]
struct Arrows {
    id: Id,
    arrows: Vec<[DVec2; 2]>,
    color: Color,
}

/// The left button held down, without a tool.
#[derive(Debug, Clone, Copy)]
struct Press {
    /// Where it went down, and where the cursor is now, in pixels.
    from: DVec2,
    to: DVec2,
    /// Where it went down on the sketch, if over it.
    at: Option<DVec2>,
    /// What it went down on.
    hit: Option<Selectable>,
    /// Where the point it went down on was then, if a point: dragged, it
    /// snaps ([`snap::snap_drag`]).
    point: Option<DVec2>,
    /// Whether it went farther than [`DRAG_DISTANCE`]: a drag rather than
    /// a click, of `hit` if it can be dragged, else a box.
    moved: bool,
    /// Whether it drags `hit`.
    grab: bool,
    /// When it went down.
    when: Instant,
    /// Whether it was held still long enough to list what overlaps
    /// where it went down, listed or not.
    held: bool,
}

/// A dimension's label grabbed, followed by the viewport.
#[derive(Debug, Clone, Copy)]
struct LabelPress {
    id: Id,
    /// Where it was grabbed, in pixels.
    from: DVec2,
    /// Whether it went farther than [`DRAG_DISTANCE`]: a drag rather than
    /// a click.
    moved: bool,
}

impl Press {
    /// The box it drags, while it drags one.
    fn area(&self) -> Option<ScreenBox> {
        (self.moved && !self.grab).then(|| ScreenBox::new(self.from, self.to))
    }

    /// Whether it drags geometry.
    fn drags_geometry(&self) -> bool {
        self.moved && self.grab
    }

    /// When what overlaps where it went down is listed if it's held still
    /// till then, unless it has moved or that's been looked at.
    fn lists_at(&self) -> Option<Instant> {
        (!self.moved && !self.held).then(|| self.when + overlaps::HOLD_DELAY)
    }
}

/// Everything the base layer depends on. Not the camera: the renderer
/// projects the layer.
#[derive(Debug, Clone)]
struct Drawn {
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
}

impl<'a> Sketching<'a> {
    /// `sketch` as the viewport shows it. Its tool and dragging work only
    /// if it's `editable`.
    pub(crate) fn new(sketch: SketchState<'a>, editable: bool) -> Self {
        let states = States {
            fixed: sketch
                .analysis
                .map(|analysis| analysis.fixed.clone())
                .unwrap_or_default(),
            conflicts: sketch.conflicting_items(),
            // Only curves, those the sketch holds.
            failing: (sketch.failing.iter())
                .copied()
                .filter(|&id| sketch.sketch.curve(id).is_some())
                .collect(),
            pending: sketch.pending.clone(),
        };
        Self {
            sketch: sketch.sketch,
            placement: sketch.placement,
            selection: sketch.selection,
            tool: sketch.tool.filter(|_| editable),
            editable,
            states,
            hovered: sketch.hovered,
            glyphs: sketch.glyphs,
            units: sketch.units,
            value: sketch.value,
            label_drag: sketch.label_drag,
            snap: sketch.snap,
            aim: sketch.aim,
            profiles: sketch.profiles.and_then(|found| found.as_ref().ok()),
            comb: sketch.comb,
        }
    }

    pub(crate) fn placement(&self) -> Placement {
        self.placement
    }

    /// Where the grid and the sketch are drawn: on the sketch's plane.
    /// Every origin plane makes one; one that didn't would leave them on
    /// XY.
    pub(crate) fn grid(&self) -> GridPlane {
        let placement = self.placement();
        GridPlane::new(
            placement.origin.as_vec3(),
            placement.x.as_vec3(),
            placement.y.as_vec3(),
        )
        .unwrap_or(GridPlane::XY)
    }

    /// Takes `event`, with the `cursor` over the viewport's `bounds` seen
    /// by `camera` and `modifiers` held. `None` for what isn't the
    /// sketch's: buttons other than the left, the wheel, and keys but
    /// `Esc` while the left button is held.
    pub(crate) fn update(
        &self,
        input: &mut Input,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        camera: &Camera,
        modifiers: Modifiers,
    ) -> Option<Action<Message>> {
        match event {
            // Lets go of the button held down, and the key does nothing
            // more. A drag of geometry is the app's to put back, so it's
            // told to, whether or not the drag has moved anything yet: left
            // to the app, the key would back out of the sketch then.
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(Named::Escape),
                ..
            }) => {
                // An Offset dragged goes nowhere.
                if std::mem::take(&mut input.releasing) {
                    return Some(Action::request_redraw().and_capture());
                }
                let press = input.press.take()?;
                Some(if press.grab && press.moved {
                    Action::publish(Message::Look(Look::CancelDrag)).and_capture()
                } else {
                    Action::request_redraw().and_capture()
                })
            }
            Event::Mouse(event) => {
                let projector =
                    Projector::new(camera, self.placement(), bounds.width, bounds.height)?;
                self.mouse(input, *event, bounds, cursor, &projector, modifiers)
            }
            Event::Window(iced::window::Event::RedrawRequested(now)) => {
                self.hold(input, *now, bounds, camera)
            }
            _ => None,
        }
    }

    /// Takes a frame drawn at `now` while the button may be held: held
    /// still for [`overlaps::HOLD_DELAY`] over more than one item, it lets
    /// go and lists them ([`Look::OpenOverlaps`]); over one or none it goes
    /// on as it was.
    fn hold(
        &self,
        input: &mut Input,
        now: Instant,
        bounds: Rectangle,
        camera: &Camera,
    ) -> Option<Action<Message>> {
        let press = input.press.as_mut()?;
        let due = press.lists_at()?;
        if now < due {
            // Asked again: a redraw asked for sooner lets go of it.
            return Some(Action::request_redraw_at(due));
        }
        press.held = true;
        let projector = Projector::new(camera, self.placement(), bounds.width, bounds.height)?;
        let cursor = projector.cursor(press.from)?;
        let tolerance = overlaps::OVERLAP_REACH * cursor.pixel;
        let mut ids = hit::overlaps(self.sketch, cursor.at, tolerance);
        if ids.len() < 2 {
            return None;
        }
        ids.truncate(overlaps::MAX_OVERLAPS);
        let size = [bounds.width, bounds.height];
        let list = Overlaps::new(press.from, size, OverlapItems::Sketch(ids));
        input.press = None;
        Some(Action::publish(Message::Look(Look::OpenOverlaps(list))))
    }

    /// Takes the mouse `event`, with the `cursor` over `bounds`.
    fn mouse(
        &self,
        input: &mut Input,
        event: mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        projector: &Projector,
        modifiers: Modifiers,
    ) -> Option<Action<Message>> {
        let local = |p: Point| DVec2::new((p.x - bounds.x).into(), (p.y - bounds.y).into());
        match event {
            mouse::Event::CursorMoved { position } => {
                let over = cursor.position_over(bounds).map(local);
                input.cursor = over;
                let previous = input.pointer.replace(local(position));
                if let Some((id, _)) = self.label_drag {
                    return self.drag_label(input, id, previous, local(position), projector);
                }
                if let Some(press) = &mut input.press {
                    // The raw position, so a drag goes on over the rest of
                    // the window.
                    return self.drag(press, local(position), projector, modifiers);
                }
                let under = over.and_then(|at| projector.cursor(at));
                let hover = under.and_then(|cursor| self.hit(cursor));
                let changed = std::mem::replace(&mut input.hover, hover) != hover;
                let region = under.and_then(|cursor| self.region(hover, cursor));
                let changed = changed || std::mem::replace(&mut input.region, region) != region;
                if let Some(pointed) = self.point_to(input, under, modifiers) {
                    return Some(pointed);
                }
                (changed || self.tool.is_some()).then(Action::request_redraw)
            }
            mouse::Event::CursorLeft => {
                input.cursor = None;
                let hovered = input.hover.take().is_some() | input.region.take().is_some();
                if let Some(snapped) = self.point_to(input, None, modifiers) {
                    return Some(snapped);
                }
                (hovered || self.tool.is_some()).then(Action::request_redraw)
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let position = cursor.position_over(bounds)?;
                let from = local(position);
                let under = projector.cursor(from);
                if self.releases() {
                    input.releasing = true;
                    return Some(Action::capture());
                }
                // The model's picking's: see [`Sketching::picks_outside`].
                if self.picks_outside() {
                    return None;
                }
                if self.tool.is_some() {
                    let double = double_click(&mut input.last_click, from);
                    // The app lets go of the snap it shows as it takes the
                    // click, so the next is sent afresh.
                    input.snap = None;
                    let message = under.map(|cursor| {
                        let click = ToolClick {
                            double,
                            ..self.click(cursor, modifiers)
                        };
                        Message::Edit(Edit::ToolClick(click))
                    });
                    return Some(capture(message));
                }
                let hit = under.and_then(|cursor| self.hit(cursor));
                input.press = Some(Press {
                    from,
                    to: from,
                    at: under.map(|cursor| cursor.at),
                    hit,
                    point: (hit.and_then(Selectable::item))
                        .and_then(|id| self.sketch.point(id))
                        .map(|point| point.at),
                    moved: false,
                    // The origin and axes stay where they are, and so does
                    // what a link made.
                    grab: self.editable
                        && hit.is_some_and(|hit| {
                            let id = hit.id();
                            !id.is_builtin() && !self.sketch.is_linked(id)
                        }),
                    when: Instant::now(),
                    held: false,
                });
                let due = input.press.as_ref().and_then(Press::lists_at)?;
                Some(Action::request_redraw_at(due).and_capture())
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                if std::mem::take(&mut input.releasing) {
                    let under = cursor
                        .position_over(bounds)
                        .and_then(|at| projector.cursor(local(at)));
                    let message = under.map(|cursor| {
                        Message::Edit(Edit::ToolClick(self.click(cursor, modifiers)))
                    });
                    return Some(capture(message));
                }
                // A label grabbed is let go of, moved or not.
                if input.label.take().is_some() || self.label_drag.is_some() {
                    return Some(capture(Some(Message::Edit(Edit::DropLabel))));
                }
                let press = input.press.take()?;
                let add = modifiers.command();
                // A second click on a spline, soon after the first and
                // near it, adds a point to it.
                let double = !press.moved && double_click(&mut input.last_select, press.from);
                let spline = press
                    .hit
                    .and_then(Selectable::item)
                    .filter(|&id| self.editable && self.sketch.kind(id) == Some(Kind::Spline));
                let message = match press.area() {
                    _ if double && let (Some(spline), Some(at)) = (spline, press.at) => {
                        Message::Edit(Edit::InsertSplinePoint { spline, at })
                    }
                    _ if !press.moved => Message::Look(Look::ClickGeometry {
                        hit: press.hit,
                        add,
                    }),
                    None => Message::Edit(Edit::DropGeometry),
                    Some(area) => {
                        let mode = BoxMode::dragged(press.from, press.to);
                        let ids = hit::in_box(self.sketch, projector, area, mode);
                        Message::Look(Look::SelectBox { ids, add })
                    }
                };
                Some(capture(Some(message)))
            }
            _ => None,
        }
    }

    /// Moves the button held down, `press`, to `to`: past
    /// [`DRAG_DISTANCE`] it drags what it went down on, or a box. A point
    /// dragged snaps unless [`Held::FREE`] is among `modifiers`.
    fn drag(
        &self,
        press: &mut Press,
        to: DVec2,
        projector: &Projector,
        modifiers: Modifiers,
    ) -> Option<Action<Message>> {
        press.to = to;
        if !press.moved && press.from.distance(to) < DRAG_DISTANCE {
            return Some(Action::capture());
        }
        press.moved = true;
        if !press.grab {
            return Some(Action::request_redraw().and_capture());
        }
        // Off the sketch, the geometry stays where it was dragged last.
        let message =
            press
                .hit
                .zip(press.at)
                .zip(projector.cursor(to))
                .map(|((id, from), cursor)| {
                    let snapped = self
                        .drag_snap(press, cursor, modifiers)
                        .filter(|(_, snap)| snap.snapped());
                    // Snapped, the point goes where it snapped rather than
                    // keep the offset it was grabbed at (a rim's radius
                    // takes only where the cursor is).
                    let (from, to, target) = match snapped {
                        Some((grabbed, snap)) => (grabbed, snap.at, snap.target),
                        None => (from, cursor.at, None),
                    };
                    Message::Look(Look::DragGeometry {
                        id,
                        from,
                        to,
                        target,
                    })
                });
        Some(capture(message))
    }

    /// Where what `press` drags snaps with the cursor at `cursor`, with
    /// where it was grabbed: a point as [`snap::snap_drag`] has it, a
    /// circle's rim as [`snap::snap_rim`]. `None` for anything else, or
    /// with [`Held::FREE`] among `modifiers`.
    fn drag_snap(
        &self,
        press: &Press,
        cursor: Cursor,
        modifiers: Modifiers,
    ) -> Option<(DVec2, Snap)> {
        if Held::FREE.is_held(modifiers) {
            return None;
        }
        let id = press.hit?.item()?;
        if let Some(point) = press.point {
            return Some((
                point,
                snap::snap_drag(self.sketch, id, cursor.at, cursor.pixel),
            ));
        }
        let rim = self
            .sketch
            .curve(id)
            .filter(|entry| entry.curve.kind() == Kind::Circle)?;
        Some((
            press.at?,
            snap::snap_rim(self.sketch, rim.id, cursor.at, cursor.pixel),
        ))
    }

    /// Follows the cursor moved to `to` from `previous` with the label of
    /// the dimension `id` grabbed: past [`DRAG_DISTANCE`] from where it
    /// was grabbed, it drags the label there.
    fn drag_label(
        &self,
        input: &mut Input,
        id: Id,
        previous: Option<DVec2>,
        to: DVec2,
        projector: &Projector,
    ) -> Option<Action<Message>> {
        let grabbed = match input.label {
            Some(label) if label.id == id => label,
            // The label took the press, where the cursor was last.
            _ => LabelPress {
                id,
                from: previous.unwrap_or(to),
                moved: false,
            },
        };
        let moved = grabbed.moved || grabbed.from.distance(to) >= DRAG_DISTANCE;
        input.label = Some(LabelPress { moved, ..grabbed });
        if !moved {
            return Some(Action::capture());
        }
        let message = projector
            .cursor(grabbed.from)
            .zip(projector.cursor(to))
            .map(|(from, to)| {
                Message::Look(Look::DragLabel {
                    id,
                    from: from.at,
                    to: to.at,
                })
            });
        Some(capture(message))
    }

    /// Whether its tool picks outside the sketch, in the model and other
    /// sketches ([`Tool::picks_outside`]): then the left button is the
    /// model's picking's, and nothing of the sketch is hit.
    pub(crate) fn picks_outside(&self) -> bool {
        self.tool.is_some_and(|tool| tool.tool.picks_outside())
    }

    /// The item under `cursor`, see [`hit::hit`]: with Trim or Extend,
    /// or Offset picking its chain, the curve ([`hit::hit_curve`]), with
    /// Mirror choosing its line the line ([`hit::hit_line`]), with Fillet
    /// or Chamfer picking their corner the point where lines make one, a
    /// little farther off ([`hit::hit_corner`]), and nothing with Offset
    /// placing its copy, or Fillet or Chamfer theirs, which go where the
    /// cursor is, nor with Project or Intersect, which pick outside the
    /// sketch.
    fn hit(&self, cursor: Cursor) -> Option<Selectable> {
        let tolerance = HIT_TOLERANCE * cursor.pixel;
        let item = match self.tool {
            _ if self.releases() || self.picks_outside() => None,
            Some(tool) if tool.tool.corners() => {
                hit::hit_corner(self.sketch, cursor.at, CORNER_TOLERANCE * cursor.pixel)
            }
            Some(tool) if matches!(tool.tool, Tool::Trim | Tool::Extend | Tool::Offset) => {
                hit::hit_curve(self.sketch, cursor.at, tolerance)
            }
            Some(tool) if tool.tool == Tool::Mirror && tool.about => {
                hit::hit_line(self.sketch, cursor.at, tolerance)
            }
            _ => return hit::hit(self.sketch, cursor.at, tolerance),
        };
        item.map(Selectable::Item)
    }

    /// The region under `cursor`, highlighted, see
    /// [`Profiles::region_at`]: only without a tool, and with no item
    /// `hover`ed, which is highlighted instead.
    fn region(&self, hover: Option<Selectable>, cursor: Cursor) -> Option<usize> {
        if hover.is_some() || self.tool.is_some() {
            return None;
        }
        self.profiles?.region_at(cursor.at)
    }

    /// The near misses of the sketch's profiles seen through `projector`:
    /// open ends within [`NEAR_MISS_GAP`] pixels at the target, paired
    /// again only when the profiles or the zoom have changed since
    /// `input` last kept them.
    fn near_misses(&self, input: &Input, projector: &Projector) -> Vec<NearMiss> {
        let Some(profiles) = self.profiles else {
            return Vec::new();
        };
        let gap = NEAR_MISS_GAP * projector.pixel();
        let mut near = input.near.borrow_mut();
        match &*near {
            Some(near) if Arc::ptr_eq(&near.profiles, profiles) && near.gap == gap => {
                near.misses.clone()
            }
            _ => {
                let misses = profiles.near_misses(gap);
                *near = Some(Near {
                    profiles: profiles.clone(),
                    gap,
                    misses: misses.clone(),
                });
                misses
            }
        }
    }

    /// Where the drawing tool's next click snaps to with the cursor at
    /// `cursor`, see [`snap::snap`], if it snaps to anything: not without
    /// a drawing tool, nor while [`Held::FREE`] is held among
    /// `modifiers`.
    fn snap(&self, cursor: Cursor, modifiers: Modifiers) -> Option<Snap> {
        let tool = self.tool.filter(|tool| tool.tool.draws())?;
        if Held::FREE.is_held(modifiers) {
            return None;
        }
        Some(snap::snap(self.sketch, &tool, cursor.at, cursor.pixel)).filter(Snap::snapped)
    }

    /// The drawing tool's click with the cursor at `cursor` and
    /// `modifiers` held, where it snaps, if it does: not a double-click.
    fn click(&self, cursor: Cursor, modifiers: Modifiers) -> ToolClick {
        let click = ToolClick {
            at: cursor.at,
            target: None,
            inference: None,
            hit: self.hit(cursor),
            pixel: cursor.pixel,
            double: false,
            reference: Held::REFERENCE.is_held(modifiers),
        };
        match self.snap(cursor, modifiers) {
            Some(snap) => click.snapped(snap),
            None => click,
        }
    }

    /// Whether the tool clicks where the left button is let go rather
    /// than pressed, so the click can be dragged there: Offset placing its
    /// copy, or Fillet or Chamfer theirs, which the preview shows through
    /// the cursor meanwhile.
    fn releases(&self) -> bool {
        self.tool
            .is_some_and(|tool| tool.tool.places() && !tool.picked.is_empty())
    }

    /// Whether the drawing tool's shape has fields, so the app hears where
    /// its click would go as the cursor moves ([`Look::Aim`]).
    fn aiming(&self) -> bool {
        self.tool.is_some_and(|tool| !tool.fields().is_empty())
    }

    /// Tells the app where the drawing tool's click would go with the
    /// cursor at `under`, if it's over the sketch, and `modifiers` held:
    /// while its shape has fields, the click itself, else where it snaps,
    /// if that's changed. With the cursor off the sketch, a shape with
    /// fields stays aimed where it was, snapped as it was, as `Enter`
    /// places it there.
    fn point_to(
        &self,
        input: &mut Input,
        under: Option<Cursor>,
        modifiers: Modifiers,
    ) -> Option<Action<Message>> {
        if self.aiming() {
            let cursor = under?;
            let click = self.click(cursor, modifiers);
            // The app takes where it snaps from it.
            input.snap = Some(click.snap()).filter(Snap::snapped);
            return Some(Action::publish(Message::Look(Look::Aim(click))));
        }
        let snap = under.and_then(|cursor| self.snap(cursor, modifiers));
        self.send_snap(input, snap)
    }

    /// Tells the app `snap` is where the next click snaps to, if that's
    /// not what it was told last: its glyph shows by the cursor.
    fn send_snap(&self, input: &mut Input, snap: Option<Snap>) -> Option<Action<Message>> {
        if input.snap == snap {
            return None;
        }
        input.snap = snap;
        Some(Action::publish(Message::Look(Look::Snap(snap))))
    }

    /// Takes `modifiers` changing to what they are, with the `cursor` over
    /// the viewport's `bounds` seen by `camera`: [`Held::FREE`] lets go of
    /// the snap, or takes it again.
    pub(crate) fn modifiers_changed(
        &self,
        input: &mut Input,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        camera: &Camera,
        modifiers: Modifiers,
    ) -> Option<Action<Message>> {
        self.tool?;
        let projector = Projector::new(camera, self.placement(), bounds.width, bounds.height)?;
        let at = cursor.position_over(bounds)?;
        let at = DVec2::new((at.x - bounds.x).into(), (at.y - bounds.y).into());
        Some(
            self.point_to(input, projector.cursor(at), modifiers)
                .unwrap_or_else(Action::request_redraw),
        )
    }

    /// The cursor over the viewport's `bounds`, if the sketch sets it:
    /// grabbing while geometry is dragged, else over the viewport a
    /// crosshair with a tool, or a pointer over an item to select.
    pub(crate) fn mouse_interaction(
        &self,
        input: &Input,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<mouse::Interaction> {
        let label = input.label.is_some_and(|label| label.moved) && self.label_drag.is_some();
        if label || input.press.is_some_and(|press| press.drags_geometry()) {
            return Some(mouse::Interaction::Grabbing);
        }
        cursor.position_over(bounds)?;
        if self.tool.is_some() {
            Some(mouse::Interaction::Crosshair)
        } else if input.press.is_none() && input.hover.is_some() {
            Some(mouse::Interaction::Pointer)
        } else {
            None
        }
    }

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
        let mut base = input.base.borrow_mut();
        let current = base.as_ref().is_some_and(|base| {
            let drawn = &base.drawn;
            drawn.sketch == *self.sketch
                && drawn.selection == *self.selection
                && drawn.states == self.states
                && drawn.colors == colors
                && drawn.label_drag == self.label_drag
                && drawn.placement == self.placement
                && drawn.profiles.as_ref().map(Arc::as_ptr) == self.profiles.map(Arc::as_ptr)
                && drawn.comb == self.comb
        });
        let base = match &mut *base {
            Some(base) if current => base,
            base => {
                let (layer, arrows) = self.base_layer(colors);
                let drawn = Drawn {
                    sketch: self.sketch.clone(),
                    selection: self.selection.clone(),
                    states: self.states.clone(),
                    colors,
                    label_drag: self.label_drag,
                    placement: self.placement,
                    profiles: self.profiles.cloned(),
                    comb: self.comb,
                };
                base.insert(Base {
                    drawn,
                    layer: Arc::new(layer),
                    arrows,
                    combs: self.combs(),
                    failing: self.failing_lines(),
                })
            }
        };
        let projector = Projector::new(camera, self.placement(), bounds.width, bounds.height);
        let mut live = self.live_layer(input, projector.as_ref(), colors, &base.arrows, modifiers);
        if let Some(projector) = &projector {
            combs(&mut live, &base.combs, projector.pixel(), colors.guide);
        }
        (base.layer.clone(), live)
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
    fn selected(&self, id: Id) -> bool {
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
        let states = &self.states;
        let faded = |id: Id, color: Color| {
            if states.pending.contains(&id) {
                Color {
                    a: color.a * PENDING_ALPHA,
                    ..color
                }
            } else {
                color
            }
        };
        let red = |id: Id| states.conflicts.contains(&id) || states.failing.contains(&id);
        let state_color = |id: Id| {
            if red(id) {
                colors.conflict
            } else if states.fixed.contains(&id) {
                colors.fixed
            } else {
                colors.curve
            }
        };
        // The origin and axes: selected, in a conflict, or else their own
        // colour.
        let builtin_color = |id: Id| {
            if self.selected(id) {
                colors.selected
            } else if states.conflicts.contains(&id) {
                colors.conflict
            } else {
                colors.axis
            }
        };
        // The axes under the rest, and the origin under the points.
        for id in [Id::X_AXIS, Id::Y_AXIS] {
            for half in axis(id).into_iter().flatten() {
                let style = line(builtin_color(id), AXIS_WIDTH, false);
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
                let (color, width) = match (selected, entry.construction && !red(entry.id)) {
                    (true, _) => (colors.selected, SELECTED_WIDTH),
                    _ if linked.contains(&entry.id) && !red(entry.id) => (colors.link, CURVE_WIDTH),
                    (false, true) => (colors.construction, CURVE_WIDTH),
                    (false, false) => (state_color(entry.id), CURVE_WIDTH),
                };
                let color = faded(entry.id, color);
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
        let mut style = dot(POINT_RADIUS, colors.point_fill, builtin_color(Id::ORIGIN));
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
                let fill = if selected {
                    colors.selected
                } else {
                    colors.point_fill
                };
                let rim = if linked.contains(&point.id) && !red(point.id) {
                    colors.link
                } else {
                    if red(point.id) || states.fixed.contains(&point.id) {
                        state_color(point.id)
                    } else if tips.contains(&point.id) {
                        colors.spline_handle
                    } else {
                        colors.point
                    }
                };
                let mut style = dot(POINT_RADIUS, faded(point.id, fill), faded(point.id, rim));
                style.fixed = !selected && states.fixed.contains(&point.id);
                layer.point(point.at, style);
            }
        }
        (layer, arrows)
    }

    /// What shows how splines are shaped: each handle as a line from its
    /// end through its fit point to its tip, in the handles' colour, or
    /// the selection's with its spline or itself
    /// ([`Selectable::HandleLine`]); and
    /// of a spline selected by control points, its control polygon, dashed
    /// in the handles' colour, as its points are. Fit points without a
    /// handle show none.
    fn spline_aids(&self, layer: &mut SketchLayer, colors: SketchColors) {
        let sketch = self.sketch;
        let at = |id: Id| sketch.point(id).map(|point| point.at);
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
            if !selected {
                continue;
            }
            if spline.kind == SplineKind::Control {
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

    /// The glyphs of the sketch's constraints, if they're shown, each
    /// where [`glyphs::anchors`] puts it: hovering one highlights what it
    /// ties together, clicking selects it. In red while in a conflict, in
    /// the selection's colour while selected, faded while waiting on the
    /// solver.
    pub(crate) fn glyphs(&self) -> Vec<(DVec2, Element<'a, Message>)> {
        if !self.glyphs {
            return Vec::new();
        }
        let mut glyphs = Vec::new();
        for entry in &self.sketch.constraints {
            let id = entry.id;
            let kind = ConstraintKind::of(&entry.constraint);
            let look = self.look(id);
            for at in glyphs::anchors(self.sketch, &entry.constraint) {
                glyphs.push((at, glyph(id, kind, look)));
            }
        }
        glyphs
    }

    /// The glyph of where the drawing tool's next click snaps to, if it
    /// does, by the cursor: what the point is on, then how the shape
    /// runs, as their constraints' icons, as far as the values typed leave
    /// them (see [`typed::hold`]).
    pub(crate) fn snap_glyph(&self) -> Option<(DVec2, Element<'a, Message>)> {
        let tool = self.tool.filter(|tool| tool.tool.draws())?;
        let snap = self.snap?;
        // What the values typed leave of it, where it snapped.
        let snap = Snap {
            at: snap.at,
            ..typed::hold(&tool, snap)?
        };
        if !snap.snapped() {
            return None;
        }
        let icons = snap.kinds().into_iter().map(|kind| {
            icons::tinted(kind.icon(), SNAP_ICON, |palette| palette.sketching.guide).into()
        });
        let chip = container(iced::widget::Row::with_children(icons).spacing(2))
            .padding(GLYPH_PADDING)
            .style(|theme| theme::glyph(theme, false));
        Some((snap.at, chip.into()))
    }

    /// How the constraint or dimension `id` looks, by its state.
    fn look(&self, id: Id) -> GlyphLook {
        if self.states.conflicts.contains(&id) {
            GlyphLook::Conflict
        } else if self.selected(id) {
            GlyphLook::Selected
        } else if self.states.pending.contains(&id) {
            GlyphLook::Pending
        } else {
            GlyphLook::Free
        }
    }

    /// The colour of the dimension `entry`, by its state and whether it's
    /// a reference.
    fn dimension_color(&self, entry: &DimensionEntry, colors: SketchColors) -> Color {
        self.look(entry.id)
            .dimension_color(colors, entry.dimension.driving)
    }

    /// Where the label of the dimension `entry` is, in sketch
    /// coordinates, as far as it's dragged.
    fn label_at(&self, entry: &DimensionEntry) -> Option<DVec2> {
        let dimension = &entry.dimension;
        let dragged = self
            .label_drag
            .filter(|&(id, _)| id == entry.id)
            .map_or(DVec2::ZERO, |(_, by)| by);
        Some(self.sketch.anchor(&dimension.measure)? + dimension.label + dragged)
    }

    /// How the dimension `entry` is drawn, see [`dimensions::lines`].
    fn dimension_lines(&self, entry: &DimensionEntry) -> Option<dimensions::Lines> {
        let dimension = &entry.dimension;
        let label = self.label_at(entry)?;
        dimensions::lines(self.sketch, &dimension.measure, dimension.side, label)
    }

    /// The labels of the sketch's dimensions, each at its place: its
    /// value, a reference's in brackets, coloured by its state. Pressing
    /// one selects it and grabs it to drag, double-clicking a driving one
    /// opens the value field in its place, hovering it highlights what it
    /// measures. The one the field is open on over the viewport is left
    /// out: the field takes its place.
    pub(crate) fn labels(&self) -> Vec<(DVec2, Element<'a, Message>)> {
        let edited =
            self.value
                .filter(|field| !field.in_list)
                .and_then(|field| match field.target {
                    ValueTarget::Dimension(id) => Some(*id),
                    ValueTarget::New { .. } | ValueTarget::Field(_) => None,
                });
        let shown = self
            .sketch
            .dimensions
            .iter()
            .filter(|entry| Some(entry.id) != edited);
        shown
            .filter_map(|entry| {
                let at = self.label_at(entry)?;
                let dimension = &entry.dimension;
                let value = dimension::label(self.sketch, dimension, self.units);
                let chip = label_chip(
                    entry.id,
                    value,
                    self.look(entry.id),
                    dimension.driving,
                    self.editable,
                );
                Some((at, chip))
            })
            .collect()
    }

    /// The value field over the viewport, if it's open there: at the label
    /// of the dimension it edits, or of the one it places, with why the
    /// value typed was refused under it if it was.
    pub(crate) fn field(&self) -> Option<(DVec2, Element<'a, Message>)> {
        let field = self.value.filter(|field| !field.in_list)?;
        let at = match field.target {
            ValueTarget::New { measure, label, .. } => self.sketch.anchor(measure)? + *label,
            ValueTarget::Dimension(id) => self.label_at(self.sketch.dimension(*id)?)?,
            // By the cursor, with the tool's other fields.
            ValueTarget::Field(_) => return None,
        };
        let content = column![panels::value_field("", field.text), field_error(field)].spacing(2);
        let chip = container(content)
            .padding(2)
            .style(|theme| theme::glyph(theme, false));
        Some((at, chip.into()))
    }

    /// The drawing tool's fields, while its shape has any, by where its
    /// click would go (see [`typed`]): each named, showing the value typed
    /// in it, or what it measures of the shape until one is, and the one
    /// the value field is open on taking it, with why the value typed was
    /// refused under it if it was.
    pub(crate) fn fields(&self) -> Option<(DVec2, Element<'a, Message>)> {
        let tool = self.tool?;
        let fields = tool.fields();
        let at = self.aim.filter(|_| !fields.is_empty())?;
        let outline =
            typed::outline(&tool, at).or_else(|| typed::corner_outline(&tool, self.sketch, at));
        let open = self
            .value
            .filter(|field| matches!(field.target, ValueTarget::Field(_)));
        let rows = fields.iter().map(|&field| {
            let typed = tool.value_in(field);
            let measured = match field {
                Field::Sides => Some(f64::from(tool.sides)),
                Field::Distance if tool.tool == Tool::Offset => self
                    .sketch
                    .offset_side(&tool_items(&tool), at)
                    .map(|(reach, _)| reach),
                _ => outline.as_ref().and_then(|outline| outline.value(field)),
            };
            let unit = field.quantity().unit(self.units);
            let shown = match typed {
                Some(value) => varde_expr::format(value.value, unit),
                None => measured.map_or_else(String::new, |value| varde_expr::format(value, unit)),
            };
            let value: Element<'a, Message> = match open {
                Some(open) if *open.target == ValueTarget::Field(field) => {
                    column![panels::value_field(&shown, open.text), field_error(open)]
                        .spacing(2)
                        .into()
                }
                _ => text(shown)
                    .size(LABEL_SIZE)
                    .style(if typed.is_some() {
                        theme::accent_text
                    } else {
                        theme::muted_text
                    })
                    .into(),
            };
            row![text(field.label()).size(11).width(FIELD_NAME_WIDTH), value]
                .spacing(4)
                .align_y(Alignment::Center)
                .into()
        });
        let chip = container(iced::widget::Column::with_children(rows).spacing(2))
            .padding(LABEL_PADDING)
            .style(|theme| theme::glyph(theme, false));
        Some((at, chip.into()))
    }

    /// What changes as the cursor moves: the box being dragged, what's
    /// under the cursor (with a drawing tool, what it snaps to; else an
    /// item, or failing one the region), the dimensions' arrowheads
    /// (`dimension_arrows`, a hovered dimension's in the hover colour), the
    /// rings marking near misses, which depend on the zoom, and the shape
    /// the tool is drawing to where the cursor snaps, with `modifiers`
    /// held, and the snap's guide, seen through `projector` if the
    /// viewport shows anything.
    fn live_layer(
        &self,
        input: &Input,
        projector: Option<&Projector>,
        colors: SketchColors,
        dimension_arrows: &[Arrows],
        modifiers: Modifiers,
    ) -> SketchLayer {
        let mut layer = SketchLayer::default();
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
        // A drawing tool shows what it snaps to.
        let drawing = self.tool.is_some_and(|tool| tool.tool.draws());
        // Not while the button held down drags.
        let still = input.press.is_none_or(|press| !press.moved);
        let hover = if drawing {
            snap.and_then(|snap| snap.highlighted())
                .map(Selectable::Item)
        } else {
            input.hover.filter(|_| still)
        };
        // What a list's row, a glyph or a label hovered stands for: a
        // constraint or a dimension by what it ties together, and a
        // dimension itself.
        let listed = self.hovered.into_iter().flat_map(|target| match target {
            Selectable::Item(id) => tied_items(self.sketch, id)
                .into_iter()
                .map(Selectable::Item)
                .collect(),
            _ => vec![target],
        });
        for id in hover.into_iter().chain(listed) {
            self.highlight(&mut layer, id, colors.hovered);
        }
        // Where a drawing tool's click, or a point or rim dragged, snaps is
        // marked round the cursor, whatever it snapped to.
        let dragged = (input.press)
            .filter(|press| press.moved && press.grab && !Held::FREE.is_held(modifiers))
            .zip(cursor)
            .and_then(|(press, cursor)| self.drag_snap(&press, cursor, modifiers))
            .map(|(_, snap)| snap);
        let spot = snap.filter(|_| drawing).or(dragged).filter(Snap::snapped);
        if let Some(spot) = spot {
            layer.point(spot.at, snap_disc(colors.point));
        }
        let region = cursor
            .filter(|_| still)
            .and_then(|cursor| Some((self.profiles?, self.region(input.hover, cursor)?)));
        if let Some((profiles, region)) = region
            && let Some(region) = profiles.regions.get(region)
        {
            fill_region(&mut layer, region, colors.region_hovered);
        }
        let hovered =
            (self.hovered.and_then(Selectable::item)).and_then(|id| self.sketch.dimension(id));
        if let Some(lines) = hovered.and_then(|entry| self.dimension_lines(entry)) {
            draw_lines(&mut layer, &lines, colors.hovered);
        }
        if let Some(projector) = projector {
            for dimension in dimension_arrows {
                let color = if self.hovered == Some(dimension.id.into()) {
                    colors.hovered
                } else {
                    dimension.color
                };
                arrows(&mut layer, projector, &dimension.arrows, color);
            }
            let ring = ring(colors.near_miss);
            for miss in self.near_misses(input, projector) {
                layer.point((miss.a + miss.b) / 2.0, ring);
            }
        }
        let under = (input.hover.and_then(Selectable::item))
            .filter(|_| still)
            .zip(cursor);
        match self.tool {
            Some(tool) if tool.tool == Tool::Dimension => {
                for &id in tool.picked {
                    self.highlight(&mut layer, id, colors.selected);
                }
            }
            Some(tool) if !tool.tool.draws() => {
                self.shape_preview(&mut layer, &tool, under, at, colors);
            }
            Some(tool) => {
                if let Some((snap, cursor)) = snap.zip(cursor)
                    && let Some(guide) = snap.guide(self.sketch, &tool, cursor.pixel)
                {
                    layer.polyline(Space::Sketch, &guide, line(colors.guide, GUIDE_WIDTH, true));
                }
                let at = snap.map(|snap| snap.at).or(at);
                preview(&mut layer, &tool, at, colors);
            }
            None => {}
        }
        if let Some((measure, side, label)) = self.placing(at) {
            let lines = dimensions::lines(self.sketch, &measure, side, label);
            if let Some(lines) = lines {
                draw_lines(&mut layer, &lines, colors.preview);
                if let Some(projector) = projector {
                    arrows(&mut layer, projector, &lines.arrows, colors.preview);
                }
            }
        }
        layer
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
        tool: &ActiveTool<'_>,
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
                for &id in tool.picked {
                    self.highlight(layer, id, colors.selected);
                }
                let style = line(colors.preview, CURVE_WIDTH, false);
                for polyline in at.map(|at| placed(tool, sketch, at)).unwrap_or_default() {
                    layer.polyline(Space::Sketch, &polyline, style);
                }
            }
            Tool::Mirror => {
                for &id in tool.picked {
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
        let (measure, side) = dimension::measure(self.sketch, tool.picked, at, tool.switched)?;
        Some((measure, side, at))
    }

    /// Draws `target` highlighted in `color`, as under the cursor, into
    /// `layer`: a point or a curve, the origin and axes too, a spline's
    /// handle as a line.
    fn highlight(&self, layer: &mut SketchLayer, target: Selectable, color: Color) {
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

/// The items `tool` has picked, by their ids: what Offset and Mirror
/// work on.
fn tool_items(tool: &ActiveTool<'_>) -> Vec<Id> {
    tool.picked.iter().map(|target| target.id()).collect()
}

/// The handle whose tip is `tip` as drawn: from its end through its fit
/// point to its tip. `None` if `tip` is no handle's tip.
pub(crate) fn handle_arms(sketch: &Sketch, tip: Id) -> Option<[DVec2; 3]> {
    let (_, handle) = sketch.handle(tip)?;
    let at = |id| sketch.point(id).map(|point| point.at);
    Some([at(handle.end)?, at(handle.at)?, at(handle.tip)?])
}

/// The axis `id` names, if it names one, as drawn: as far as a sketch
/// reaches either way, as two segments out from the origin. The renderer
/// cuts a segment to the near plane and the viewport by mixing its ends,
/// which is only as exact as its start is near the part that shows: one
/// starting that far away lands pixels off the grid's axis line, more as
/// it's zoomed in and in perspective.
fn axis(id: Id) -> Option<[[DVec2; 2]; 2]> {
    let reach = f64::from(MAX_COORD);
    let along = match id {
        Id::X_AXIS => DVec2::X,
        Id::Y_AXIS => DVec2::Y,
        _ => return None,
    };
    Some([[DVec2::ZERO, -along * reach], [DVec2::ZERO, along * reach]])
}

/// Whether a click at `at`, in the viewport's pixels, ends a double-click:
/// soon after the click `last` holds and near it. `last` then holds this
/// click, unless it ended one.
pub(super) fn double_click(last: &mut Option<(Instant, DVec2)>, at: DVec2) -> bool {
    let now = Instant::now();
    let double = last.take().is_some_and(|(time, from)| {
        now.saturating_duration_since(time) < DOUBLE_CLICK && from.distance(at) < DRAG_DISTANCE
    });
    if !double {
        *last = Some((now, at));
    }
    double
}

/// Draws the curvature combs `combs` (see [`Sketch::curvature_comb`]) in
/// `color` into `layer`, a pixel at the target being `pixel` sketch units:
/// each tooth from its place, away from where the spline turns, the
/// longest of a comb [`COMB_LENGTH`] pixels and the rest in proportion,
/// and a line through their tips. A spline that doesn't turn has no
/// teeth.
fn combs(layer: &mut SketchLayer, combs: &[Vec<[DVec2; 2]>], pixel: f64, color: Color) {
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
fn draw_lines(layer: &mut SketchLayer, lines: &dimensions::Lines, color: Color) {
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
fn arrows(layer: &mut SketchLayer, projector: &Projector, arrows: &[[DVec2; 2]], color: Color) {
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

/// How a constraint's glyph, or a dimension, looks, by its state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GlyphLook {
    Free,
    Selected,
    Conflict,
    Pending,
}

impl GlyphLook {
    /// Its colour among `colors`: free in the sketch colour, or a
    /// `reference` dimension in the construction colour, and that faded
    /// while waiting on the solver.
    fn color(self, colors: SketchColors, reference: bool) -> Color {
        let free = if reference {
            colors.construction
        } else {
            colors.curve
        };
        self.over(free, colors)
    }

    /// Its colour as a dimension's among `colors`: free in the Dimension
    /// tools' colour if `driving`, else in the construction colour, and
    /// that faded while waiting on the solver.
    fn dimension_color(self, colors: SketchColors, driving: bool) -> Color {
        let free = if driving {
            colors.dimension
        } else {
            colors.construction
        };
        self.over(free, colors)
    }

    /// Its colour among `colors`, free in `free`.
    fn over(self, free: Color, colors: SketchColors) -> Color {
        match self {
            GlyphLook::Free => free,
            GlyphLook::Selected => colors.selected,
            GlyphLook::Conflict => colors.conflict,
            GlyphLook::Pending => Color {
                a: free.a * PENDING_ALPHA,
                ..free
            },
        }
    }
}

/// The glyph of the constraint `id`, of `kind`, looking `look`: its icon on
/// a chip, in its category's colours while free, else in one colour by
/// its look, which selects it when clicked and highlights what it ties
/// together while hovered.
fn glyph<'a>(id: Id, kind: ConstraintKind, look: GlyphLook) -> Element<'a, Message> {
    let icon: Element<'a, Message> = if look == GlyphLook::Free {
        icons::icon(kind.icon(), GLYPH_ICON)
    } else {
        icons::tinted(kind.icon(), GLYPH_ICON, move |palette| {
            look.color(palette.sketching, false)
        })
        .into()
    };
    chip(id, icon, GLYPH_PADDING, look)
        .on_press(Message::Look(Look::ClickRow(id.into())))
        .interaction(mouse::Interaction::Pointer)
        .into()
}

/// The label of the dimension `id`, showing `value`, looking `look`, in
/// the construction colour unless `driving`: pressing it selects it and,
/// if `editable`, grabs it to drag, double-clicking a driving one opens the
/// value field on it, and hovering it highlights what it measures.
fn label_chip<'a>(
    id: Id,
    value: String,
    look: GlyphLook,
    driving: bool,
    editable: bool,
) -> Element<'a, Message> {
    let value = text(value)
        .size(LABEL_SIZE)
        .style(move |theme| text::Style {
            color: Some(look.dimension_color(theme::palette(theme).sketching, driving)),
        });
    let area = chip(id, value, LABEL_PADDING, look)
        .on_press(Message::Look(Look::PressLabel { id, add: false }))
        .interaction(if editable {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::Pointer
        });
    if driving && editable {
        area.on_double_click(Message::Look(Look::EditDimension { id, in_list: false }))
            .into()
    } else {
        area.into()
    }
}

/// `content` on a chip over the viewport, as a glyph or a label of the
/// constraint or dimension `id` looking `look`, highlighting what `id`
/// ties together while hovered.
fn chip<'a>(
    id: Id,
    content: impl Into<Element<'a, Message>>,
    padding: impl Into<Padding>,
    look: GlyphLook,
) -> MouseArea<'a, Message> {
    let chip = container(content)
        .padding(padding)
        .style(move |theme| match look {
            GlyphLook::Selected => theme::selected_glyph(theme),
            _ => theme::glyph(theme, look == GlyphLook::Conflict),
        });
    mouse_area(chip)
        .on_enter(Message::Look(Look::HoverItem(Some(id.into()))))
        .on_exit(Message::Look(Look::LeaveItem(id.into())))
}

/// The shape `tool` is drawing, with the cursor `at` its next point if
/// it's over the sketch, and the points placed so far.
fn preview(
    layer: &mut SketchLayer,
    tool: &ActiveTool<'_>,
    at: Option<DVec2>,
    colors: SketchColors,
) {
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
                if let &[start, end] = tool.placed {
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
    for &placed in tool.placed {
        layer.point(placed, dot(POINT_RADIUS, colors.point_fill, color));
    }
}

/// What `tool`, one that [`places`](Tool::places) and has picked, would
/// make in `sketch` with the cursor `at` a place, as polylines: Offset's
/// copy through the cursor, or as far off as the distance typed on the
/// cursor's side; Fillet's or Chamfer's through the cursor, or as the
/// values typed make it (see [`typed::corner_outline`]). Empty where it
/// makes nothing.
fn placed(tool: &ActiveTool<'_>, sketch: &Sketch, at: DVec2) -> Vec<Vec<DVec2>> {
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

/// Why the value typed in `field` was refused, under it, if it was.
fn field_error<'a>(field: ValueField<'_>) -> Option<Element<'a, Message>> {
    field.error.map(|error| {
        text(crate::chrome::sentence(&error.to_string()).into_owned())
            .size(11)
            .style(theme::danger_text)
            .into()
    })
}

/// A line `width` pixels wide in `color`, dashed if `dashed`.
pub(super) fn line(color: Color, width: f32, dashed: bool) -> LineStyle {
    LineStyle {
        color: srgba(color),
        width,
        dash: dashed.then_some(DASH),
    }
}

/// A point `radius` pixels across, `fill` inside a `rim`, not filled as
/// fixed.
fn dot(radius: f32, fill: Color, rim: Color) -> PointStyle {
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
fn ring(color: Color) -> PointStyle {
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
fn snap_disc(color: Color) -> PointStyle {
    let wash = srgba(color.scale_alpha(SNAP_WASH));
    PointStyle {
        radius: SNAP_RADIUS,
        rim_width: 0.0,
        rim: wash,
        fill: wash,
        fixed: false,
    }
}

/// Fills `region` on `layer` in `color`, by its outline, holes left out.
pub(super) fn fill_region(layer: &mut SketchLayer, region: &Region, color: Color) {
    fill_region_in(layer, Space::Sketch, region, color);
}

/// Like [`fill_region`], with the region's coordinates in `space`.
pub(super) fn fill_region_in(layer: &mut SketchLayer, space: Space, region: &Region, color: Color) {
    let outline = region.outline.iter().map(Vec::as_slice);
    layer.fill(space, outline, srgba(color));
}

/// `color` for the renderer.
pub(super) fn srgba(color: Color) -> Srgba {
    Srgba([color.r, color.g, color.b, color.a])
}

/// `message`, if there is one, with the event captured.
fn capture(message: Option<Message>) -> Action<Message> {
    match message {
        Some(message) => Action::publish(message).and_capture(),
        None => Action::capture(),
    }
}

#[cfg(test)]
mod tests;
