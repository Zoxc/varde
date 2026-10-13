//! The sketch being edited, in the viewport: the left button's work on it
//! (selecting, box selecting, dragging geometry and drawing with the
//! tools), `Esc` while the button is held, the cursor, and the layers the
//! renderer draws of it, and the constraints' glyphs over it: the left
//! button in `input`, the layers in `draw`, the widgets over it in
//! `chips`. See `agents/sketch/viewport.md`.

mod chips;
mod dimensions;
mod draw;
mod glyphs;
mod input;

use draw::Drawn;

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::Arc;

use glam::DVec2;
use iced::keyboard::Modifiers;
use iced::time::Instant;
use iced::{Color, Rectangle, Vector};
use varde_document::Placement;
use varde_expr::LengthUnit;
use varde_kernel_compat::RenderLines;
use varde_render::{Camera, GridPlane, SketchLayer};
use varde_sketch::{Id, NearMiss, Profiles, Selectable, Sketch};

use crate::hit::ScreenBox;
use crate::overlaps;
use crate::projection::Projector;
use crate::shortcut::Held;
use crate::snap::Snap;
use crate::theme::SketchColors;
use crate::{ActiveTool, SketchState, ValueField};

/// How far the cursor moves with the button held before it's a drag
/// rather than a click, in pixels.
const DRAG_DISTANCE: f64 = 4.0;
const CURVE_WIDTH: f32 = 1.8;
/// A touch wider than a finished sketch's
/// ([`super::sketch_pick::SELECTED_WIDTH`]).
const SELECTED_WIDTH: f32 = 2.6;
const POINT_RADIUS: f32 = 3.6;
const HOVERED_POINT_RADIUS: f32 = 5.1;
/// How wide the outline of the box dragged to select is, in pixels.
const BOX_LINE_WIDTH: f32 = 1.0;
/// How opaque what waits on the solver is, against what's committed.
const PENDING_ALPHA: f32 = 0.45;
/// Where a glyph is centred from where its anchor shows: up and to the
/// right, beside what it's on rather than over it. In pixels.
pub(crate) const GLYPH_OFFSET: Vector = Vector::new(13.0, -13.0);
/// How wide the origin's axes are, and the guides of a snap, in pixels.
const AXIS_WIDTH: f32 = 1.0;
const GUIDE_WIDTH: f32 = 1.0;
/// Where the glyph of a snap is centred from the cursor: down and to the
/// right, clear of it. In pixels.
pub(crate) const SNAP_OFFSET: Vector = Vector::new(16.0, 16.0);
/// Where a drawing tool's fields are from the cursor: their top left
/// corner, down and to the right, below the snap's glyph. In pixels.
pub(crate) const FIELDS_OFFSET: Vector = Vector::new(12.0, 28.0);
/// How near open ends have to be to be marked as almost meeting, in
/// pixels at the target: a gap too small to see.
const NEAR_MISS_GAP: f64 = 6.0;
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
    /// Whether a click on a spline adds a point to it (see
    /// [`SketchState::inserting`]).
    inserting: bool,
    /// Whether tools snap without [`Held::FREE`] held, as the settings
    /// say: if not, only with it.
    snaps: bool,
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

impl States {
    /// Whether `id` is drawn red: in a conflict or named by a failing
    /// feature.
    fn red(&self, id: Id) -> bool {
        self.conflicts.contains(&id) || self.failing.contains(&id)
    }

    /// The colour of the curve or point `id` by its state alone: red,
    /// else fixed, else free.
    fn color(&self, id: Id, colors: SketchColors) -> Color {
        if self.red(id) {
            colors.conflict
        } else if self.fixed.contains(&id) {
            colors.fixed
        } else {
            colors.curve
        }
    }

    /// `color`, faded if `id` waits on the solver.
    fn faded(&self, id: Id, color: Color) -> Color {
        if self.pending.contains(&id) {
            fade(color)
        } else {
            color
        }
    }
}

/// `color` faded, as what waits on the solver is.
fn fade(color: Color) -> Color {
    color.scale_alpha(PENDING_ALPHA)
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
    /// The snap last sent to the app ([`Look::Snap`](crate::Look::Snap)), so it's sent again
    /// only when it changes.
    snap: Option<Snap>,
    /// The near misses last found, and for what, so they're only paired
    /// again when the profiles or the zoom change.
    near: RefCell<Option<Near>>,
    /// The base layer last built, and what it was built from, so it's
    /// only built, and uploaded, again when that changes.
    base: RefCell<Option<Base>>,
}

impl<'a> Sketching<'a> {
    /// `sketch` as the viewport shows it. Its tool and dragging work only
    /// if it's `editable`; its tools snap without [`Held::FREE`] held if
    /// `snaps`, else only with it.
    pub(crate) fn new(sketch: SketchState<'a>, editable: bool, snaps: bool) -> Self {
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
            inserting: sketch.inserting && editable && sketch.tool.is_none(),
            snaps,
        }
    }

    pub(crate) fn placement(&self) -> Placement {
        self.placement
    }

    /// The projector onto the sketch's plane for `camera` in `bounds`.
    fn projector(&self, camera: &Camera, bounds: Rectangle) -> Option<Projector> {
        Projector::new(camera, self.placement, bounds.width, bounds.height)
    }

    /// Whether tools snap with `modifiers` held: unless [`Held::FREE`]
    /// is among them, or only if it is where the settings turned snapping
    /// off.
    fn snaps(&self, modifiers: Modifiers) -> bool {
        self.snaps != Held::FREE.is_held(modifiers)
    }

    /// Where the grid and the sketch are drawn: on the sketch's plane.
    /// Every origin plane makes one; one that didn't would leave them on
    /// XY.
    pub(crate) fn grid(&self) -> GridPlane {
        super::regions::sketch_grid(&self.placement())
    }
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
    /// snaps ([`snap::snap_drag`](crate::snap::snap_drag)).
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

/// The open ends of `profiles` within `gap` of each other.
struct Near {
    profiles: Arc<Profiles>,
    gap: f64,
    misses: Vec<NearMiss>,
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

/// A dimension's arrowheads, in sketch coordinates (see
/// [`dimensions::Lines::arrows`]), and their colour.
#[derive(Debug, Clone, PartialEq)]
struct Arrows {
    id: Id,
    arrows: Vec<[DVec2; 2]>,
    color: Color,
}

#[cfg(test)]
mod tests;
