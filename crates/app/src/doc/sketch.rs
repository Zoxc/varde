//! Editing a sketch: making one, entering and leaving it, and what's
//! selected in it. Drawing and changing its geometry is in [`edit`], and
//! proposing the changes to the solver in [`propose`].

mod dimension;
mod edit;
mod propose;
mod shape;
mod spline;
mod typed;

use std::collections::BTreeSet;
use std::sync::Arc;

use glam::DVec2;
use varde_document::{
    Feature, FeatureId, FeatureKind, Generation, OriginPlane, Placement, Plane, Revision, Sketch,
};
use varde_expr::Value;
use varde_render::{Camera, Projection};
use varde_sketch::{Analysis, Id, Profiles, Rejected, Role, SketchEdit, TooComplex};
use varde_view::typed::{DEFAULT_SIDES, Field};
use varde_view::{
    ActiveTool, RowMenu, SketchState, Snap, Target, Tool, ToolClick, ValueField, ValueTarget,
};

use super::extrude::is_sketch;
use super::{Doc, HOME_TARGET, home_camera};
pub(crate) use dimension::Focus;
#[cfg(test)]
pub(crate) use propose::CHECKING;
use propose::sketch_of;
pub(crate) use propose::{Analyses, Proposals};

/// The sketch being edited, while one is: [`Doc::sketch`].
#[derive(Debug)]
pub(crate) struct SketchSession {
    /// The sketch feature edited.
    pub(crate) feature: FeatureId,
    /// The tool drawing in the sketch, if one is in use.
    pub(crate) tool: Option<Drawing>,
    /// The items selected, which the viewport and the Geometry list show.
    /// Only ever items the sketch holds, see [`Doc::prune`].
    pub(crate) selection: BTreeSet<Id>,
    /// The points and curves the Constraints list lists what's on, all
    /// when none: see [`follow_selection`].
    pub(crate) listed_on: BTreeSet<Id>,
    /// Geometry being dragged, if it is.
    pub(crate) drag: Option<Drag>,
    /// How far the Geometry list is scrolled, in pixels, as the list last
    /// said.
    pub(crate) scroll: f32,
    /// How far the Constraints list is scrolled, in pixels.
    pub(crate) constraint_scroll: f32,
    /// Whether the Constrain tool is in use, offering the constraints that
    /// fit the selection. Never with a drawing tool.
    pub(crate) constraining: bool,
    /// Whether the constraints' glyphs are shown in the viewport.
    pub(crate) glyphs: bool,
    /// Whether the curvature comb of the splines selected shows.
    pub(crate) comb: bool,
    /// The item hovered in a list or by its glyph, if any.
    pub(crate) hovered: Option<Id>,
    /// The sketch with the edits waiting on the solver applied, while
    /// there are any: what's shown, and what tools draw on.
    pub(crate) waiting: Option<Waiting>,
    /// What the constraints do to the sketch, by the revisions analysed.
    pub(crate) analyses: Analyses,
    /// Why the solver refused the last edit, shown with what it ran into
    /// until the next action.
    pub(crate) refusal: Option<Refusal>,
    /// The value field, while it's open on a dimension.
    pub(crate) value: Option<ValueEdit>,
    /// A dimension's label grabbed, while it is.
    pub(crate) label: Option<LabelDrag>,
    /// Where the drawing tool's next click snaps to, as the viewport last
    /// said, for the glyph by the cursor.
    pub(crate) snap: Option<Snap>,
    /// The click the drawing tool would take with the cursor where it last
    /// was, as the viewport or the last click said: where its fields show,
    /// and where `Enter` places its shape.
    pub(crate) aim: Option<ToolClick>,
    /// The profiles of the sketch as it's shown, see
    /// [`Doc::refresh_profiles`].
    pub(crate) profiles: Option<Profiled>,
}

impl SketchSession {
    fn new(feature: FeatureId) -> Self {
        Self {
            feature,
            tool: None,
            selection: BTreeSet::new(),
            listed_on: BTreeSet::new(),
            drag: None,
            scroll: 0.0,
            constraint_scroll: 0.0,
            constraining: false,
            glyphs: true,
            comb: false,
            hovered: None,
            waiting: None,
            analyses: Analyses::default(),
            refusal: None,
            value: None,
            label: None,
            snap: None,
            aim: None,
            profiles: None,
        }
    }

    /// The analysis of the sketch at `revision`, if the solver has made it.
    pub(crate) fn analysis(&self, revision: Revision) -> Option<&Arc<Analysis>> {
        self.analyses.get(revision)
    }
}

/// The regions a sketch encloses, or that it's too complex to find them,
/// and the sketch they're of: see [`Doc::refresh_profiles`].
#[derive(Debug)]
pub(crate) struct Profiled {
    sketch: Sketch,
    pub(crate) profiles: Result<Arc<Profiles>, TooComplex>,
}

/// The sketch being edited with the edits waiting on the solver applied,
/// see [`SketchSession::waiting`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Waiting {
    pub(crate) sketch: Sketch,
    /// The items the edits add, which the sketch committed doesn't hold.
    pub(crate) added: BTreeSet<Id>,
}

/// The value field open on a dimension, or on a field of the drawing
/// tool's, see [`ValueField`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ValueEdit {
    pub(crate) target: ValueTarget,
    /// The text as typed.
    pub(crate) text: String,
    /// Why the text submitted was refused, until it's typed in again.
    pub(crate) error: Option<varde_expr::Error>,
    /// Whether it's in the Constraints list, else at the label.
    pub(crate) in_list: bool,
}

/// A dimension's label grabbed to drag, and how far it's been dragged in
/// sketch units: none until it moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LabelDrag {
    pub(crate) id: Id,
    pub(crate) by: DVec2,
}

/// Why the solver refused an edit of the sketch being edited.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Refusal {
    /// It doesn't hold together, see [`Rejected`].
    Rejected(Rejected),
    /// The solver failed to answer: it panicked, or its worker stopped.
    Failed(String),
}

/// A tool in use, and the shape it's drawing.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Drawing {
    pub(crate) tool: Tool,
    /// The points of the shape placed so far, in sketch coordinates: see
    /// [`ActiveTool::placed`].
    pub(crate) placed: Vec<DVec2>,
    /// What each point placed snapped to, if anything: see
    /// [`ActiveTool::targets`].
    pub(crate) targets: Vec<Option<Target>>,
    /// The lines the Line tool has drawn since it started the chain, if it
    /// has.
    pub(crate) chain: Option<Chain>,
    /// Whether it draws construction geometry.
    pub(crate) construction: bool,
    /// The Dimension tool's items picked so far, to measure, the Mirror
    /// tool's to mirror, the chain the Offset tool offsets, or the corner
    /// Fillet or Chamfer is on, as its point and its lines (see
    /// [`Tool::pick`](varde_view::Tool::pick)).
    pub(crate) picked: Vec<Id>,
    /// Whether the Mirror tool has what it mirrors and waits for the line
    /// to mirror about.
    pub(crate) about: bool,
    /// Whether the Dimension tool measures a circle's radius, or an arc's
    /// diameter, rather than the other: switched for what's picked.
    pub(crate) switched: bool,
    /// The values typed in the tool's fields for the shape being drawn,
    /// each fixing what its field measures and added with the shape as
    /// its driving dimension: see [`varde_view::typed`].
    pub(crate) typed: Vec<(Field, Value)>,
    /// How many sides the Polygon tool draws, from [`MIN_SIDES`] to
    /// [`MAX_SIDES`]: kept from one polygon to the next.
    ///
    /// [`MIN_SIDES`]: varde_view::typed::MIN_SIDES
    /// [`MAX_SIDES`]: varde_view::typed::MAX_SIDES
    pub(crate) sides: u32,
    /// Whether the Rectangle tool draws from the centre, rather than from
    /// a corner.
    pub(crate) centered: bool,
    /// Whether the Spline tool draws by control points, rather than
    /// through fit points: kept from one spline to the next.
    pub(crate) control: bool,
}

impl Drawing {
    fn new(tool: Tool) -> Self {
        Self {
            tool,
            placed: Vec::new(),
            targets: Vec::new(),
            chain: None,
            construction: false,
            picked: Vec::new(),
            about: false,
            switched: false,
            typed: Vec::new(),
            sides: DEFAULT_SIDES,
            centered: false,
            control: false,
        }
    }

    /// The tool as the view has it.
    pub(crate) fn active(&self) -> ActiveTool<'_> {
        ActiveTool {
            tool: self.tool,
            placed: &self.placed,
            targets: &self.targets,
            construction: self.construction,
            picked: &self.picked,
            about: self.about,
            switched: self.switched,
            typed: &self.typed,
            sides: self.sides,
            centered: self.centered,
            control: self.control,
        }
    }

    /// The fields its shape has now, see [`ActiveTool::fields`].
    pub(crate) fn fields(&self) -> &'static [Field] {
        self.active().fields()
    }

    /// Whether it has anything of a shape placed, or picked to measure.
    fn started(&self) -> bool {
        !(self.placed.is_empty() && self.picked.is_empty())
    }

    /// Starts the shape again, keeping the tool.
    fn restart(&mut self) {
        self.placed.clear();
        self.targets.clear();
        self.chain = None;
        self.picked.clear();
        self.about = false;
        self.switched = false;
        self.typed.clear();
    }
}

/// Connected lines the Line tool is drawing: the next starts at `last`,
/// and clicking `first` closes the loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Chain {
    pub(crate) first: Id,
    pub(crate) last: Id,
    /// How many lines it has: a loop takes at least three.
    pub(crate) lines: usize,
}

/// Geometry being dragged, a drag session of the solver's: each step goes
/// to the solver, and what it converges on is shown until the drag is
/// dropped, and proposed, or put back.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Drag {
    /// The document's generation the drag started from: a change to the
    /// document since, such as an undo, puts the drag back.
    pub(crate) generation: Generation,
    /// The revision it started from, which the solution is of.
    pub(crate) revision: Revision,
    /// Names the drag to the solver, see [`varde_solve::Request::Drag`].
    pub(crate) session: u64,
    /// The sketch as committed when it started.
    pub(crate) start: Arc<Sketch>,
    /// The move to where the cursor is, which dropping it proposes.
    pub(crate) edit: SketchEdit,
    /// The last step the solver converged on, shown; none until the
    /// first.
    pub(crate) solution: Option<Arc<Sketch>>,
}

/// The share of the Sketch tab the Geometry list takes until the divider
/// is dragged.
pub(crate) const GEOMETRY_SHARE: f32 = 0.6;

/// How much room a sketch framed on entering it leaves around its points:
/// the view is this many times as tall as they are.
const FRAME_MARGIN: f32 = 1.5;

impl Doc {
    /// Starts picking the plane for a new sketch, or backs out of it.
    /// Only outside a sketch and an extrude, in a document that can be
    /// edited.
    pub(crate) fn pick_plane(&mut self) {
        self.picking_plane = !self.picking_plane
            && self.editable()
            && self.sketch.is_none()
            && self.extrude.is_none();
    }

    /// Adds a sketch on `plane` and edits it, unless that's refused.
    pub(crate) fn new_sketch(&mut self, plane: OriginPlane) {
        self.picking_plane = false;
        let before = self.editor.revision();
        self.apply(self.editor.document().add_sketch(Plane::Origin(plane)));
        if self.editor.revision() != before {
            // New features get the highest id, so it's the last.
            if let Some(feature) = self.editor.document().features().last() {
                self.enter_sketch(feature.id);
            }
        }
    }

    /// Edits the sketch feature `id`, if the document holds it: the camera
    /// turns to face it, the model fades behind it, and the Sketch tab
    /// takes the Timeline's place. It stays selected in the Timeline for
    /// after.
    pub(crate) fn enter_sketch(&mut self, id: FeatureId) {
        if !is_sketch(self.editor.document(), id) {
            return;
        }
        self.picking_plane = false;
        self.extrude = None;
        self.selected_feature = Some(id);
        // Back in the sketch, the edit refused after it was left is seen
        // to be missing.
        if self
            .refused_edit
            .as_ref()
            .is_some_and(|&(edited, _)| edited == id)
        {
            self.refused_edit = None;
        }
        self.sketch = Some(SketchSession::new(id));
        self.panel = self.panel.for_sketching(true);
        // The rail's sets are the sketch's now.
        self.rail.close();
        if let Some(to) = self.sketch_camera() {
            self.animate_camera(to);
        }
        self.sync();
    }

    /// Leaves the sketch being edited, if one is. Everything done in it is
    /// in the document already.
    pub(crate) fn finish_sketch(&mut self) {
        if self.sketch.is_some() {
            self.end_session();
            self.sync();
        }
    }

    /// Leaves the sketch without asking for the model again, for
    /// [`Doc::sync`] to.
    fn end_session(&mut self) {
        self.sketch = None;
        self.panel = self.panel.for_sketching(false);
        self.rail.close();
    }

    /// Backs out of whatever is open, the innermost first: the delete
    /// prompt, the rail's list, the feature's context menu, the file menu, the view options menu, picking a plane, the
    /// extrude being set up, the value field, a label grabbed, the drag of
    /// geometry, the shape the tool is drawing (or what the Dimension or
    /// Mirror tool has picked), the tool, the sketch, the feature selected.
    pub(crate) fn escape(&mut self) {
        if self.deleting.is_some() {
            self.deleting = None;
        } else if self.rail.open.is_some() {
            self.rail.close();
        } else if self.row_menu.take().is_some() {
        } else if self.file_menu {
            self.file_menu = false;
        } else if self.view_menu {
            self.view_menu = false;
        } else if self.picking_plane {
            self.picking_plane = false;
        } else if self.extrude.is_some() {
            self.extrude = None;
        } else if let Some(session) = &mut self.sketch {
            if session.value.take().is_some()
                || session.label.take().is_some()
                || session.drag.take().is_some()
            {
                return;
            }
            match &mut session.tool {
                Some(drawing) if drawing.started() => drawing.restart(),
                Some(_) => session.tool = None,
                None if session.constraining => session.constraining = false,
                None => self.finish_sketch(),
            }
        } else {
            self.selected_feature = None;
        }
    }

    /// Takes a click in the sketch without a tool, on `hit` if anything:
    /// selects it alone, or nothing, or with `add` adds it to the
    /// selection or takes it out.
    pub(crate) fn click_geometry(&mut self, hit: Option<Id>, add: bool) {
        let hit = hit.filter(|&id| self.holds(id));
        let Some(session) = &mut self.sketch else {
            return;
        };
        match (hit, add) {
            (Some(id), true) => {
                if !session.selection.remove(&id) {
                    session.selection.insert(id);
                }
            }
            (Some(id), false) => session.selection = BTreeSet::from([id]),
            (None, true) => {}
            (None, false) => session.selection.clear(),
        }
    }

    /// Selects the items `ids` a box was dragged over, those the sketch
    /// holds: alone, or with `add` as well as what's selected.
    pub(crate) fn select_box(&mut self, ids: Vec<Id>, add: bool) {
        let held: Vec<_> = ids.into_iter().filter(|&id| self.holds(id)).collect();
        let Some(session) = &mut self.sketch else {
            return;
        };
        if !add {
            session.selection.clear();
        }
        session.selection.extend(held);
    }

    /// Whether the sketch being edited, as it's worked on, holds `id`.
    fn holds(&self, id: Id) -> bool {
        self.working_sketch()
            .is_some_and(|sketch| sketch.kind(id).is_some())
    }

    /// Hovers the item `id` of a list or glyph, or none, if the sketch
    /// holds it.
    pub(crate) fn hover_item(&mut self, id: Option<Id>) {
        let id = id.filter(|&id| self.holds(id));
        if let Some(session) = &mut self.sketch {
            session.hovered = id;
        }
    }

    /// Takes up the Constrain tool in a sketch that can be changed, putting
    /// down the drawing tool, or puts it down.
    pub(crate) fn toggle_constrain(&mut self) {
        let editable = self.editable();
        if let Some(session) = &mut self.sketch {
            session.drag = None;
            session.tool = None;
            session.constraining = !session.constraining && editable;
        }
    }

    /// Shows the constraints' glyphs, or hides them.
    pub(crate) fn toggle_glyphs(&mut self) {
        if let Some(session) = &mut self.sketch {
            session.glyphs = !session.glyphs;
        }
    }

    /// Clears the selection: the sketch's in a sketch, else the
    /// Timeline's.
    pub(crate) fn clear_selection(&mut self) {
        match &mut self.sketch {
            Some(session) => session.selection.clear(),
            None => self.selected_feature = None,
        }
    }

    /// Takes up `tool` in a sketch that can be changed, or puts it down if
    /// it's the one in use. The Dimension tool starts from what's selected,
    /// if that's something to measure, or its start; the Mirror tool from
    /// the points and curves selected, if any, going on to the line to
    /// mirror about; the Offset tool from the chain selected, or the one
    /// the curve selected is in, going on to where to put the copy; Fillet
    /// and Chamfer from the two lines selected, if they make a corner.
    pub(crate) fn select_tool(&mut self, tool: Tool) {
        let editable = self.editable();
        let picked = self.working_sketch().map(|sketch| {
            use varde_view::dimension::{joins, pickable};

            let selection = self.sketch.iter().flat_map(|session| &session.selection);
            let selected: Vec<_> = selection.copied().collect();
            match (tool, selected.as_slice()) {
                (Tool::Mirror, _) => shape::mirrorable(sketch, &selected),
                (Tool::Offset, _) => shape::offsettable(sketch, &selected),
                (Tool::Fillet | Tool::Chamfer, _) => shape::cornered(sketch, &selected),
                (Tool::Dimension, &[one]) if pickable(sketch, one) => selected,
                (Tool::Dimension, &[first, second]) if joins(sketch, &[first], second) => selected,
                _ => Vec::new(),
            }
        });
        let Some(session) = &mut self.sketch else {
            return;
        };
        session.drag = None;
        session.constraining = false;
        session.snap = None;
        session.aim = None;
        session.tool = match &session.tool {
            Some(drawing) if drawing.tool == tool => None,
            _ => editable.then(|| {
                let picked = picked.unwrap_or_default();
                Drawing {
                    about: tool == Tool::Mirror && !picked.is_empty(),
                    picked,
                    ..Drawing::new(tool)
                }
            }),
        };
    }

    /// Lets go of what the document no longer holds after it changed, e.g.
    /// by undo: the feature selected, the sketch being edited, which is
    /// left, and the items selected in it. A document that can't be edited
    /// (any more, after a Save As) has no tool in use and nothing dragged.
    /// Across a replacement of the whole document, `replaced` (restoring
    /// recovered changes, or undoing or redoing that), ids may name other
    /// things: the feature selected is let go of, unless it's the sketch
    /// being edited and its id still names a sketch, which the session
    /// goes on with, and so is an edit refused after its sketch was left
    /// (as it is once its sketch is gone); and in it the selection, the item hovered, why the
    /// solver refused the last edit and the tool's shape are let go of.
    pub(crate) fn prune(&mut self, replaced: bool) {
        let editable = self.editable();
        let document = self.editor.document();
        if replaced {
            let edited = self.sketch.as_ref().map(|session| session.feature);
            let edited = edited.filter(|&id| is_sketch(document, id));
            self.selected_feature = self.selected_feature.filter(|&id| Some(id) == edited);
        }
        self.selected_feature = self
            .selected_feature
            .filter(|&id| document.feature(id).is_some());
        // A row's menu goes with what the row lists, a feature of the
        // Timeline's with its selection.
        self.row_menu = self.row_menu.filter(|menu| match *menu {
            RowMenu::Feature(id) => self.selected_feature == Some(id),
            RowMenu::Sketch(id) => !replaced && document.feature(id).is_some(),
            RowMenu::Body(id) => !replaced && document.body(id).is_some(),
        });
        // An edit refused after its sketch was left goes with the sketch,
        // and across a replacement its id may name another.
        if replaced || (self.refused_edit.as_ref()).is_some_and(|&(id, _)| !is_sketch(document, id))
        {
            self.refused_edit = None;
        }
        let Some(session) = &mut self.sketch else {
            return;
        };
        if !editable {
            session.tool = None;
            session.drag = None;
            session.constraining = false;
            session.value = None;
            session.label = None;
        }
        if replaced {
            session.selection.clear();
            session.hovered = None;
            // What the solver ran into are items of the sketch replaced.
            session.refusal = None;
            session.drag = None;
            session.value = None;
            session.label = None;
            if let Some(drawing) = &mut session.tool {
                drawing.restart();
            }
            // What the next click snapped to may be another item now.
            let unsnapped = |click: ToolClick| click.snapped(Snap::free(click.at));
            session.snap = None;
            session.aim = session.aim.map(unsnapped);
        }
        match document.feature(session.feature) {
            Some(Feature {
                kind: FeatureKind::Sketch { sketch, .. },
                ..
            }) => {
                // What's selected and drawn on may be waiting on the
                // solver.
                let waiting = session.waiting.take();
                let sketch = waiting.as_ref().map_or(sketch, |waiting| &waiting.sketch);
                session.selection.retain(|&id| sketch.kind(id).is_some());
                follow_selection(&session.selection, &mut session.listed_on, sketch);
                session.hovered = session.hovered.filter(|&id| sketch.kind(id).is_some());
                // What's measured or edited may be gone, by undo, say.
                let dimension = |id| sketch.dimension(id).is_some();
                if session.label.is_some_and(|label| !dimension(label.id)) {
                    session.label = None;
                }
                let target = session.value.as_ref().map(|field| &field.target);
                let placed = match target {
                    Some(ValueTarget::Dimension(id)) => dimension(*id),
                    Some(ValueTarget::New { measure, .. }) => {
                        measure.items().all(|(id, _)| sketch.kind(id).is_some())
                    }
                    // See below.
                    Some(ValueTarget::Field(_)) | None => true,
                };
                if !placed {
                    session.value = None;
                }
                if let Some(drawing) = &mut session.tool
                    && (drawing.picked.iter().any(|&id| sketch.kind(id).is_none())
                        || !shape::still_picked(sketch, drawing))
                {
                    drawing.restart();
                }
                if session
                    .drag
                    .as_ref()
                    .is_some_and(|drag| drag.generation != self.editor.generation())
                {
                    session.drag = None;
                }
                // A chain whose lines are gone, by undo, say, is over, and a
                // point placed isn't on what's gone.
                if let Some(drawing) = &mut session.tool {
                    for target in &mut drawing.targets {
                        if target.is_some_and(|target| sketch.kind(target.item()).is_none()) {
                            *target = None;
                        }
                    }
                    if let Some(chain) = drawing.chain {
                        match sketch.point(chain.last) {
                            Some(last) if sketch.point(chain.first).is_some() => {
                                drawing.placed = vec![last.at];
                                drawing.targets = vec![Some(Target::Point(chain.last))];
                            }
                            _ => drawing.restart(),
                        }
                    }
                }
                // Where the next click snaps isn't on what's gone: `Enter`
                // places at the aim.
                let has = |id: Id| sketch.kind(id).is_some();
                session.snap = session.snap.map(|snap| snap.within(has));
                session.snap = session.snap.filter(Snap::snapped);
                session.aim = session
                    .aim
                    .map(|click| click.snapped(click.snap().within(has)));
                // A drawing tool's field goes with the shape having it.
                if let Some(ValueTarget::Field(field)) = session.value.as_ref().map(|f| &f.target)
                    && !session
                        .tool
                        .as_ref()
                        .is_some_and(|drawing| drawing.fields().contains(field))
                {
                    session.value = None;
                }
                session.waiting = waiting;
            }
            _ => self.end_session(),
        }
    }

    /// The sketch being edited as it's shown, for the view: as the solver
    /// last solved it while it's dragged, else with the edits waiting on
    /// the solver applied.
    pub(crate) fn sketch_state(&self) -> Option<SketchState<'_>> {
        static NONE: BTreeSet<Id> = BTreeSet::new();
        let session = self.sketch.as_ref()?;
        let feature = self.editor.document().feature(session.feature)?;
        let FeatureKind::Sketch { plane, .. } = &feature.kind else {
            return None;
        };
        let waiting = session.waiting.as_ref();
        let (refusal, solver_error) = match &session.refusal {
            Some(Refusal::Rejected(why)) => (Some(why), None),
            Some(Refusal::Failed(error)) => (None, Some(error.as_str())),
            None => (None, None),
        };
        Some(SketchState {
            name: &feature.name,
            plane: *plane,
            sketch: self.shown_sketch()?,
            pending: waiting.map_or(&NONE, |waiting| &waiting.added),
            selection: &session.selection,
            listed_on: &session.listed_on,
            tool: session.tool.as_ref().map(Drawing::active),
            constraining: session.constraining,
            split: self.sketch_split,
            scroll: session.scroll,
            constraint_scroll: session.constraint_scroll,
            analysis: session.analysis(self.editor.revision()).map(|a| &**a),
            refusal,
            solver_error,
            hovered: session.hovered,
            glyphs: session.glyphs,
            unsolved: self.feed.unsolved().contains(&session.feature),
            checking: self.proposals.slow(),
            units: self.editor.document().units(),
            value: session.value.as_ref().map(|field| ValueField {
                target: &field.target,
                text: &field.text,
                error: field.error.as_ref(),
                in_list: field.in_list,
            }),
            label_drag: session.label.map(|label| (label.id, label.by)),
            snap: session.snap,
            aim: session.aim.map(|click| click.at),
            profiles: session.profiles.as_ref().map(|found| &found.profiles),
            comb: session.comb,
        })
    }

    /// The sketch being edited as it's shown: as the solver last solved it
    /// while it's dragged, else with the edits waiting on the solver
    /// applied, else as committed.
    fn shown_sketch(&self) -> Option<&Sketch> {
        let session = self.sketch.as_ref()?;
        let dragged = session
            .drag
            .as_ref()
            .and_then(|drag| drag.solution.as_deref());
        dragged.or_else(|| self.working_sketch())
    }

    /// Finds the profiles of the sketch being edited as it's shown, unless
    /// they're of that sketch already: once per sketch, whether committed,
    /// with edits waiting or a drag's step, and not as the cursor moves or
    /// the camera does. A millisecond or two for a sketch people draw
    /// (see "Built so far (step 5a)" in `notes/SketchImpl.md`); past the
    /// bounds, [`TooComplex`] in tens of them.
    pub(crate) fn refresh_profiles(&mut self) {
        let Some(shown) = self.shown_sketch() else {
            return;
        };
        let found = self
            .sketch
            .as_ref()
            .and_then(|session| session.profiles.as_ref());
        if found.is_some_and(|found| found.sketch == *shown) {
            return;
        }
        let found = Profiled {
            sketch: shown.clone(),
            profiles: shown.profiles().map(Arc::new),
        };
        if let Some(session) = &mut self.sketch {
            session.profiles = Some(found);
        }
    }

    /// Keeps what the Constraints list lists in step with the selection,
    /// see [`follow_selection`].
    pub(crate) fn list_selection(&mut self) {
        let Some(session) = &mut self.sketch else {
            return;
        };
        let committed = sketch_of(self.editor.document(), session.feature);
        let working = session.waiting.as_ref().map(|waiting| &waiting.sketch);
        if let Some(sketch) = working.or(committed) {
            follow_selection(&session.selection, &mut session.listed_on, sketch);
        }
    }

    /// Whether the Dimension tool is in use, where the peek key places
    /// references instead of peeking.
    pub(crate) fn dimensioning(&self) -> bool {
        self.sketch
            .as_ref()
            .and_then(|session| session.tool.as_ref())
            .is_some_and(|drawing| drawing.tool == Tool::Dimension)
    }

    /// The sketch being edited as it's worked on: with the edits waiting
    /// on the solver applied, if there are any, else as committed.
    pub(crate) fn working_sketch(&self) -> Option<&Sketch> {
        let session = self.sketch.as_ref()?;
        match &session.waiting {
            Some(waiting) => Some(&waiting.sketch),
            None => self.edited_sketch().map(|(_, sketch)| sketch),
        }
    }

    /// The sketch being edited as the document holds it, if one is, and
    /// the plane it's on.
    pub(crate) fn edited_sketch(&self) -> Option<(Plane, &Sketch)> {
        let session = self.sketch.as_ref()?;
        let feature = self.editor.document().feature(session.feature)?;
        let FeatureKind::Sketch { plane, sketch } = &feature.kind else {
            return None;
        };
        Some((*plane, sketch))
    }

    /// The camera looking straight at the sketch being edited, if one is,
    /// framing it: what Home turns to in a sketch.
    pub(crate) fn sketch_camera(&self) -> Option<Camera> {
        let (plane, sketch) = self.edited_sketch()?;
        Some(facing(self.camera.projection(), plane.placement(), sketch))
    }
}

/// The camera in `projection` looking straight at `sketch` on `placement`,
/// framing what's drawn of it, its points and its curves, or where Home
/// looks brought onto the plane if it has none.
fn facing(projection: Projection, placement: Placement, sketch: &Sketch) -> Camera {
    let mut camera = home_camera(projection);
    camera.face(placement.normal.as_vec3(), placement.y.as_vec3());
    // A circle's only point is its centre, so the curves count too.
    let curves = sketch
        .curves
        .iter()
        .filter_map(|entry| sketch.flatten(&entry.curve))
        .flatten();
    let drawn = sketch.points.iter().map(|point| point.at).chain(curves);
    let bounds = drawn.fold(None, |bounds, at| {
        let (low, high): (DVec2, DVec2) = bounds.unwrap_or((at, at));
        Some((low.min(at), high.max(at)))
    });
    match bounds {
        // A checked sketch's points are within `MAX_COORD`, and its radii
        // too, so these are finite.
        Some((low, high)) => {
            camera.set_target(placement.to_world((low + high) / 2.0).as_vec3());
            let size = (high - low).max_element() as f32 * FRAME_MARGIN;
            if size > camera.view_height() {
                camera.zoom(size / camera.view_height());
            }
        }
        None => {
            let normal = placement.normal.as_vec3();
            let above = normal.dot(HOME_TARGET - placement.origin.as_vec3());
            camera.set_target(HOME_TARGET - normal * above);
        }
    }
    camera
}

/// Keeps `listed_on`, the points and curves the Constraints list lists
/// the constraints and dimensions on, in step with the `selection` of
/// `sketch`: the points and curves selected, or none, listing all, when
/// nothing is. While only constraints and dimensions are selected it
/// stays as it was, less what's gone, so one picked from the list, or a
/// second click on it, finds it where it was.
fn follow_selection(selection: &BTreeSet<Id>, listed_on: &mut BTreeSet<Id>, sketch: &Sketch) {
    let geometry = |id: &Id| {
        sketch
            .kind(*id)
            .is_some_and(|kind| Role::Geometry.admits(kind))
    };
    let selected: BTreeSet<Id> = selection.iter().copied().filter(geometry).collect();
    if selected.is_empty() && !selection.is_empty() {
        listed_on.retain(geometry);
    } else {
        *listed_on = selected;
    }
}

#[cfg(test)]
mod tests;
