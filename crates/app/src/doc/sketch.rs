//! Editing a sketch: making one, entering and leaving it, and what's
//! selected in it. Drawing and changing its geometry is in [`edit`], and
//! proposing the changes to the solver in [`propose`].

mod dimension;
mod edit;
mod links;
mod outside;
mod propose;
mod shape;
mod spline;
mod typed;

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::Arc;

use glam::{DVec2, Vec3};
use varde_document::{
    Command, FaceRef, Feature, FeatureId, FeatureKind, Generation, LinkSource, Placement, Plane,
    Revision, Sketch,
};
use varde_expr::Value;
use varde_render::{Camera, Projection};
use varde_sketch::{Analysis, Id, Profiles, Rejected, Role, Selectable, SketchEdit, TooComplex};
use varde_view::typed::{DEFAULT_SIDES, Field};
use varde_view::{
    ActiveTool, CURVED_FACE, GeometryGroup, LinkRow, Naming, Panel, PlanePick, RowMenu, Shown,
    SketchLines, SketchState, Snap, Target, Tool, ToolClick, ValueField, ValueTarget,
};

use super::camera::FRAME_MARGIN;
use super::extrude::is_sketch;
use super::{Change, Doc, HOME_TARGET, OUT_OF_DATE, home_camera};
pub(crate) use dimension::Focus;
pub(crate) use outside::OutsideClick;
use propose::sketch_of;
pub(crate) use propose::{Analyses, Proposals};

/// The sketch being edited, while one is: [`Doc::sketch`].
#[derive(Debug)]
pub(crate) struct SketchSession {
    /// The sketch feature edited.
    pub(crate) feature: FeatureId,
    /// Where it is: [`Doc::placement`] as it was last known, kept while
    /// that knows none (a model of a document replaced whole shown, its
    /// face lost upstream by an undo in the sketch), so the session goes
    /// on where it was.
    pub(crate) placement: Placement,
    /// The tool drawing in the sketch, if one is in use.
    pub(crate) tool: Option<Drawing>,
    /// The items selected, which the viewport and the Geometry list show.
    /// Only ever items the sketch holds, see [`Doc::prune`].
    pub(crate) selection: BTreeSet<Selectable>,
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
    /// The Geometry list's groups folded.
    pub(crate) folded: BTreeSet<GeometryGroup>,
    /// The curves whose Geometry rows are unfolded, listing their points.
    /// Ids gone drop out, see [`Doc::prune`].
    pub(crate) expanded: BTreeSet<Id>,
    /// Whether the curvature comb of the splines selected shows.
    pub(crate) comb: bool,
    /// Whether clicks on splines add points to them: the toolbar's Add
    /// point, put down with the tools.
    pub(crate) inserting: bool,
    /// The item hovered in a list or by its glyph, if any.
    pub(crate) hovered: Option<Selectable>,
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
    /// The curves the failures of the features using the sketch name,
    /// as the model shown found them, marked in red within the errors'
    /// halo: see [`Doc::refresh_errors`].
    pub(crate) failing: BTreeSet<Id>,
    /// The sketch's links as its Sketch tab lists them: see
    /// [`Doc::refresh_links`].
    pub(crate) links: Vec<LinkRow>,
    /// The link whose row is hovered, if one's is: what it comes from is
    /// lit in the model.
    pub(crate) link_hover: Option<Id>,
    /// The sketch as it was entered, to tell whether it's been changed.
    pub(crate) entered: Sketch,
}

impl SketchSession {
    fn new(feature: FeatureId, placement: Placement, entered: Sketch) -> Self {
        Self {
            feature,
            placement,
            entered,
            tool: None,
            selection: BTreeSet::new(),
            listed_on: BTreeSet::new(),
            drag: None,
            scroll: 0.0,
            constraint_scroll: 0.0,
            constraining: false,
            glyphs: true,
            folded: BTreeSet::new(),
            expanded: BTreeSet::new(),
            comb: false,
            inserting: false,
            hovered: None,
            waiting: None,
            analyses: Analyses::default(),
            refusal: None,
            value: None,
            label: None,
            snap: None,
            aim: None,
            profiles: None,
            failing: BTreeSet::new(),
            links: Vec::new(),
            link_hover: None,
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
    /// The links the edits add, by their ids in `sketch`, and what each
    /// comes from: committed with it.
    pub(crate) sources: Vec<(Id, varde_document::OutsideRef)>,
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
    pub(crate) picked: Vec<Selectable>,
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
            chained: self.chain.map_or(0, |chain| chain.lines),
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
    /// What the point dragged snapped to, which dropping it ties it to.
    pub(crate) target: Option<varde_view::Target>,
    /// The last step the solver converged on, shown; none until the
    /// first.
    pub(crate) solution: Option<Arc<Sketch>>,
}

/// Where a new sketch on a face was placed when its face was picked, kept
/// until a model at least as new as the sketch shows, which places it
/// itself: see [`Doc::placement`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct Placed {
    pub(crate) feature: FeatureId,
    /// The plane it was put on: if an undo takes it off, the placement
    /// is no longer its.
    pub(crate) plane: Plane,
    pub(crate) placement: Placement,
    /// The editor's generation once the sketch was added.
    pub(crate) generation: Generation,
}

/// A plane being picked: what for, and whether the sketch whose plane is
/// changed is edited once that's done or backed out of (it was left for
/// it, or entered while it wasn't placed).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PickingPlane {
    pub(crate) pick: PlanePick,
    pub(crate) enter: bool,
}

/// The share of the Sketch tab the Geometry list takes until the divider
/// is dragged.
pub(crate) const GEOMETRY_SHARE: f32 = 0.6;

/// Below what cosine between its old and new normals the sketch being
/// edited has turned to another plane, which the camera turns to face:
/// far above what rounding a tilted face's normal anew changes.
const TURNED: f64 = 1.0 - 1e-9;

impl Doc {
    /// Starts picking the plane for a new sketch, or backs out of
    /// picking a plane. Only outside a sketch, in a document that can be
    /// edited; an operation being set up is dropped.
    pub(crate) fn pick_plane(&mut self) {
        if self.picking_plane.is_some() {
            self.stop_picking_plane();
        } else if self.editable() && self.sketch.is_none() {
            self.extrude = None;
            self.revolve = None;
            self.combine = None;
            self.motion = None;
            self.picking_plane = Some(PickingPlane {
                pick: self.new_sketch_pick(),
                enter: false,
            });
            // An origin plane selected alone in Objects is taken at once,
            // and let go of.
            if let Some(plane) = self.selected_origin_plane() {
                self.objects_selected.clear();
                self.plane_picked(Plane::Origin(plane));
            }
        }
    }

    /// Starts picking another plane for the sketch feature `id`, in a
    /// document that can be edited, outside an operation: from its
    /// Timeline row, or while it's edited, which is left for it and
    /// entered again once a plane is picked or picking is backed out of.
    /// Not while another sketch is edited.
    pub(crate) fn change_plane(&mut self, id: FeatureId) {
        if !self.editable()
            || self.operating()
            || (self.sketch.as_ref()).is_some_and(|session| session.feature != id)
        {
            return;
        }
        let Some(pick) = self.change_pick(id, None) else {
            return;
        };
        let enter = self.sketch.is_some();
        self.finish_sketch();
        self.selected_feature = Some(id);
        self.picking_plane = Some(PickingPlane { pick, enter });
    }

    /// Keeps the plane being picked in step with the document and the
    /// model shown, after each edit and answer: which faces take the
    /// sketch may change with an undo, and picking for a sketch goes with
    /// the sketch, or across a replacement, where its id may name
    /// another.
    pub(crate) fn prune_plane_pick(&mut self, replaced: bool) {
        let Some(picking) = &self.picking_plane else {
            return;
        };
        // Saved where it can't be written, the document can't take it:
        // backed out of, as `Esc` does.
        if !self.editable() {
            self.stop_picking_plane();
            return;
        }
        let pick = match &picking.pick.sketch {
            None => Some(self.new_sketch_pick()),
            Some(_) if replaced => None,
            Some((id, _)) => {
                // Asked for because it failed: why, as the model shown
                // has it, until it's placed.
                let failed = (picking.pick.failed.as_ref()).and_then(|_| self.failure(*id));
                self.change_pick(*id, failed)
            }
        };
        match (pick, &mut self.picking_plane) {
            (Some(pick), Some(picking)) => picking.pick = pick,
            _ => self.picking_plane = None,
        }
    }

    /// Backs out of picking a plane, leaving the document as it was: the
    /// sketch left for it is edited again, if it's placed.
    fn stop_picking_plane(&mut self) {
        let Some(picking) = self.picking_plane.take() else {
            return;
        };
        if let (true, Some((id, _))) = (picking.enter, picking.pick.sketch)
            && self.placement(id).is_some()
        {
            self.enter_sketch(id);
        }
    }

    /// Takes the plane picked: a new sketch on it, or the sketch whose
    /// plane is changed put on it, one undo step. A face is found in the
    /// model shown, and must take the sketch there (see
    /// [`PlanePick::refusal`]): if it doesn't, picking goes on and the
    /// status bar says why. An origin plane is taken for a new sketch
    /// without picking (the toolbar's buttons); a face only while picking:
    /// a click sent before picking ended may come after.
    pub(crate) fn plane_picked(&mut self, plane: Plane) {
        let Some(picking) = self.picking_plane.take() else {
            if let Plane::Origin(_) = plane {
                self.change(Change::NewSketch(plane));
            }
            return;
        };
        let (plane, placed) = match plane {
            Plane::Origin(_) => (plane, None),
            Plane::Face(face) => match self.face_placement(&face, &picking.pick) {
                Ok((face, placement)) => (Plane::Face(face), Some(placement)),
                Err(why) => {
                    self.notice = Some(why.into_owned());
                    self.picking_plane = Some(picking);
                    return;
                }
            },
        };
        let Some((feature, _)) = picking.pick.sketch.clone() else {
            self.change(Change::NewSketch(plane));
            return;
        };
        self.change(Change::SetPlane {
            feature,
            plane,
            placed,
            enter: picking.enter,
        });
    }

    /// Puts the sketch `feature` on `plane`, one undo step, its drawing
    /// as it is in its own coordinates, `placed` there if that's a face
    /// (see [`Doc::placement`]); then edits it if `enter`.
    pub(crate) fn set_sketch_plane(
        &mut self,
        feature: FeatureId,
        plane: Plane,
        placed: Option<Placement>,
        enter: bool,
    ) {
        let before = self.editor.revision();
        self.apply(Command::SetSketchPlane { feature, plane });
        if self.editor.revision() != before
            && let Some(placement) = placed
        {
            self.placed = Some(Placed {
                feature,
                plane,
                placement,
                generation: self.editor.generation(),
            });
        }
        if enter {
            self.enter_sketch(feature);
        }
    }

    /// Adds a sketch on `plane` and edits it, unless that's refused. A
    /// face is found in the model shown, and must be flat there: its
    /// placement is worked out from it at once, as regenerating will
    /// (see [`Doc::placement`]), so the session starts facing it.
    pub(crate) fn new_sketch(&mut self, plane: Plane) {
        self.picking_plane = None;
        let (plane, placed) = match plane {
            Plane::Origin(_) => (plane, None),
            Plane::Face(face) => match self.face_placement(&face, &self.new_sketch_pick()) {
                Ok((face, placement)) => (Plane::Face(face), Some(placement)),
                Err(why) => {
                    self.notice = Some(why.into_owned());
                    return;
                }
            },
        };
        let before = self.editor.revision();
        self.apply(self.editor.document().add_sketch(plane));
        if self.editor.revision() != before {
            // New features get the highest id, so it's the last.
            if let Some(feature) = self.editor.document().features().last() {
                let feature = feature.id;
                if let Some(placement) = placed {
                    self.placed = Some(Placed {
                        feature,
                        plane,
                        placement,
                        generation: self.editor.generation(),
                    });
                }
                self.enter_sketch(feature);
            }
        }
    }

    /// The face selected in the model, if a face alone is and the cursor
    /// picks the model, outside picking a plane: what `S` puts a new
    /// sketch on.
    pub(crate) fn selected_face(&self) -> Option<FaceRef> {
        (self.picks() && self.sketch.is_none() && self.picking_plane.is_none())
            .then(|| self.pick.selection.single_face())
            .flatten()
    }

    /// Why the model shown failed the feature `id`, if it did.
    fn failure(&self, id: FeatureId) -> Option<String> {
        (self.feed.failed_features().iter())
            .find(|failed| failed.feature == id)
            .map(|failed| failed.message.clone())
    }

    /// Picking the plane for a new sketch, as the document and the model
    /// shown have it.
    fn new_sketch_pick(&self) -> PlanePick {
        PlanePick::new_sketch(self.editor.document(), self.shown())
    }

    /// Picking another plane for the sketch `id`, which `failed` to be
    /// placed if it says why, as the document and the model shown have
    /// it: none if it isn't a sketch.
    fn change_pick(&self, id: FeatureId, failed: Option<String>) -> Option<PlanePick> {
        PlanePick::change(self.editor.document(), id, failed, self.shown())
    }

    /// What the model shown found that naming its faces needs.
    pub(crate) fn shown(&self) -> Shown<'_> {
        Shown {
            merged: self.feed.merged_bodies(),
            touched: self.feed.touched_features(),
            failed: self.feed.failed_features(),
        }
    }

    /// The naming of picks as of `feature` ([`Naming::before`]): the
    /// history stopped there, all of it for a new feature (`None`).
    pub(crate) fn naming_at(&self, feature: Option<FeatureId>) -> Naming {
        let document = self.editor.document();
        let before = (feature.and_then(|id| document.feature_index(id)))
            .unwrap_or(document.features().len());
        Naming::before(document, before, self.shown())
    }

    /// The face a sketch on `face` goes on and where, as the model shown
    /// has the face (a merged body's on the body holding it): the face as
    /// `pick` names it ([`PlanePick::face_ref`]), and
    /// [`Placement::on_plane`] of its plane, the bits regenerating will
    /// place it by. Refused, why, if the face isn't found there (the
    /// cursor doesn't pick that model), isn't flat, can't take the sketch
    /// `pick` is for, or is too far out to sketch on.
    fn face_placement(
        &self,
        face: &FaceRef,
        pick: &PlanePick,
    ) -> Result<(FaceRef, Placement), Cow<'static, str>> {
        if !self.picks() {
            return Err(OUT_OF_DATE.into());
        }
        let index = self.feed.pick_index();
        let found = index
            .find_face(self.feed.shown_body(face.body), &face.key, face.near)
            .ok_or("That face isn't in the model shown")?;
        if let Some(why) = pick.refusal(index, found) {
            return Err(why);
        }
        let placement = index.face_placement(found).ok_or(CURVED_FACE)?;
        let face =
            (pick.face_ref(index, found, face.near)).ok_or("That face isn't in the model shown")?;
        if placement.valid() {
            Ok((face, placement))
        } else {
            Err("That face is too far out to sketch on".into())
        }
    }

    /// Where the sketch feature `id` is, if it's a sketch and placed: an
    /// origin plane's placement, or a sketch on a face's as the model
    /// shown placed it ([`MeshFeed::placement`](super::feed::MeshFeed::placement)),
    /// or as worked out where its face was picked until a model at least
    /// as new as that shows, so nothing waits or jumps. None for a sketch
    /// on a face that failed to be placed, or before any model places it.
    pub(crate) fn placement(&self, id: FeatureId) -> Option<Placement> {
        let FeatureKind::Sketch { plane, .. } = &self.editor.document().feature(id)?.kind else {
            return None;
        };
        match plane {
            Plane::Origin(plane) => Some(plane.placement()),
            Plane::Face(_) => {
                let pending = self.placed.filter(|placed| {
                    placed.feature == id
                        && placed.plane == *plane
                        && (self.feed.generation()).is_none_or(|shown| shown < placed.generation)
                });
                match pending {
                    Some(placed) => Some(placed.placement),
                    None => self.feed.placement(id, plane),
                }
            }
        }
    }

    /// The sketches of the features `wanted` takes that are placed
    /// ([`Doc::placement`]), for the viewport to pick their curves and
    /// points where they are.
    pub(crate) fn placed_sketches(
        &self,
        wanted: impl Fn(&Feature) -> bool,
    ) -> Vec<SketchLines<'_>> {
        (self.editor.document().features().iter())
            .filter(|feature| wanted(feature))
            .filter_map(|feature| {
                let FeatureKind::Sketch { sketch, .. } = &feature.kind else {
                    return None;
                };
                Some(SketchLines {
                    feature: feature.id,
                    placement: self.placement(feature.id)?,
                    sketch,
                })
            })
            .collect()
    }

    /// Keeps the placement of the sketch being edited in step with
    /// [`Doc::placement`], where that knows one: a new model may move its
    /// face, and an undo or redo in the sketch may put it on another
    /// plane, which the camera then turns to face. Lets go of the
    /// placement worked out at a pick once a model as new as it shows.
    pub(crate) fn follow_placement(&mut self) {
        let shown = self.feed.generation();
        (self.placed).take_if(|placed| shown.is_some_and(|shown| shown >= placed.generation));
        let Some(feature) = self.sketch.as_ref().map(|session| session.feature) else {
            return;
        };
        let Some(placement) = self.placement(feature) else {
            return;
        };
        let Some(session) = &mut self.sketch else {
            return;
        };
        // A face moved along its normal, or its normal rounded anew,
        // leaves the view as it is.
        let turned = session.placement.normal.dot(placement.normal) < TURNED;
        session.placement = placement;
        if turned && let Some(to) = self.turn_to_sketch() {
            self.animate_camera(to);
        }
    }

    /// Edits the sketch feature `id`, if the document holds it and it's
    /// placed: the camera turns to face it, the model fades behind it,
    /// and the Sketch tab takes the Timeline's place and shows. It stays selected
    /// in the Timeline for after, and leaving it turns the camera back to
    /// the view before. A sketch on a face that failed to be placed (its
    /// face is gone, or isn't flat) is put on another plane first, in a
    /// document that can be edited: picking it starts, the status bar
    /// saying why, and the sketch is entered once one is picked (`Esc`
    /// leaves it as it is). Otherwise one that isn't placed isn't edited,
    /// and the status bar says why.
    pub(crate) fn enter_sketch(&mut self, id: FeatureId) {
        let Some(feature) = self.editor.document().feature(id) else {
            return;
        };
        if !matches!(feature.kind, FeatureKind::Sketch { .. }) {
            return;
        }
        let Some(placement) = self.placement(id) else {
            let failed = self.failure(id);
            if let Some(why) = &failed
                && self.editable()
            {
                let pick = self.change_pick(id, Some(why.clone()));
                self.finish_sketch();
                self.extrude = None;
                self.revolve = None;
                self.combine = None;
                self.motion = None;
                self.selected_feature = Some(id);
                self.picking_plane = pick.map(|pick| PickingPlane { pick, enter: true });
                return;
            }
            let why = failed.as_deref().unwrap_or("it isn't placed yet");
            self.notice = Some(format!("Can't edit {}: {why}", feature.name));
            return;
        };
        self.picking_plane = None;
        self.extrude = None;
        self.revolve = None;
        self.combine = None;
        self.motion = None;
        self.selected_feature = Some(id);
        // The Timeline's rows go, with no exit: none is hovered once the
        // sketch is left, until the cursor moves onto one.
        self.hovered_feature = None;
        // Back in the sketch, the edit refused after it was left is seen
        // to be missing.
        if self
            .refused_edit
            .as_ref()
            .is_some_and(|&(edited, _)| edited == id)
        {
            self.refused_edit = None;
        }
        // The view to come back to, the one being turned to if turning:
        // from one sketch to another, the one before the first.
        if self.sketch.is_none() {
            self.before_sketch = Some(self.animation.as_ref().map_or(self.camera, |a| a.to));
            self.panel_before_sketch = Some(self.panel);
        }
        let entered = match &feature.kind {
            FeatureKind::Sketch { sketch, .. } => sketch.clone(),
            _ => Sketch::default(),
        };
        self.sketch = Some(SketchSession::new(id, placement, entered));
        self.fade.start(1.0, &placement, iced::time::Instant::now());
        self.panel = Panel::Sketch;
        // The rail's sets are the sketch's now.
        self.rail.close();
        if let Some(to) = self.turn_to_sketch() {
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
    /// [`Doc::sync`] to, turning the camera back to the view before it in
    /// the projection it has now.
    fn end_session(&mut self) {
        if let Some(mut to) = self.before_sketch.take() {
            to.set_projection(self.camera.projection());
            self.animate_camera(to);
        }
        if let Some(session) = self.sketch.take() {
            (self.fade).start(0.0, &session.placement, iced::time::Instant::now());
        }
        // The tab shown before, unless Objects was picked in the sketch.
        let before = self.panel_before_sketch.take().unwrap_or(self.panel);
        self.panel = match self.panel {
            Panel::Sketch => before,
            panel => panel,
        }
        .for_sketching(false);
        self.rail.close();
    }

    /// Backs out of whatever is open, the innermost first: the delete
    /// prompt, a drag of a body's Opacity or Colour slider with its context menu, the
    /// rail's list, a row's context menu, the file menu, the view options
    /// menu, picking a plane, the operation being set up, the
    /// value field, a label grabbed, the drag of geometry, the shape the
    /// tool is drawing (or what the Dimension or Mirror tool has picked),
    /// the tool, the sketch, the feature selected and what's selected in
    /// the model.
    pub(crate) fn escape(&mut self) {
        if self.deleting.is_some() {
            self.deleting = None;
        } else if self.sliding() {
            self.opacity_preview = None;
            self.color_preview = None;
            self.row_menu = None;
        } else if self.rail.open.is_some() {
            self.rail.close();
        } else if self.row_menu.take().is_some() {
        } else if self.file_menu {
            self.file_menu = false;
        } else if self.view_menu {
            self.view_menu = false;
        } else if self.picking_plane.is_some() {
            self.stop_picking_plane();
        } else if self.extrude.is_some() {
            self.extrude = None;
        } else if self.revolve.is_some() {
            self.revolve = None;
        } else if self.combine.is_some() {
            self.combine = None;
        } else if self.motion.is_some() {
            self.motion = None;
        } else if self.measure.is_some() {
            self.measure = None;
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
                None if session.inserting => session.inserting = false,
                None => self.finish_sketch(),
            }
        } else {
            self.selected_feature = None;
            self.clear_model_selection();
        }
    }

    /// Takes a click in the sketch without a tool, on `hit` if anything:
    /// selects it alone, or nothing, or with `add` adds it to the
    /// selection or takes it out.
    pub(crate) fn click_geometry(&mut self, hit: Option<Selectable>, add: bool) {
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
    pub(crate) fn select_box(&mut self, ids: Vec<Selectable>, add: bool) {
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
    pub(crate) fn holds(&self, id: Selectable) -> bool {
        self.working_sketch()
            .is_some_and(|sketch| sketch.selectable(id))
    }

    /// Lets go of the item `id` hovered, if it still is.
    pub(crate) fn leave_item(&mut self, id: Selectable) {
        if let Some(session) = &mut self.sketch {
            session.hovered.take_if(|&mut hovered| hovered == id);
        }
    }

    /// Hovers the item `id` of a list or glyph, or none, if the sketch
    /// holds it.
    pub(crate) fn hover_item(&mut self, id: Option<Selectable>) {
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
            session.inserting = false;
        }
    }

    /// Puts down the tool in use, the Constrain tool too, and the drag
    /// with it.
    pub(crate) fn put_down_tool(&mut self) {
        if let Some(session) = &mut self.sketch {
            session.drag = None;
            session.tool = None;
            session.constraining = false;
            session.inserting = false;
        }
    }

    /// Folds a group of the Geometry list, or unfolds it.
    pub(crate) fn toggle_group(&mut self, group: GeometryGroup) {
        if let Some(session) = &mut self.sketch
            && !session.folded.remove(&group)
        {
            session.folded.insert(group);
        }
    }

    /// Unfolds a curve's row of the Geometry list to show its points
    /// under it, or folds it.
    pub(crate) fn toggle_expanded(&mut self, id: Id) {
        let held = self.holds(id.into());
        if let Some(session) = &mut self.sketch
            && !session.expanded.remove(&id)
            && held
        {
            session.expanded.insert(id);
        }
    }

    /// Shows the constraints' glyphs, or hides them.
    pub(crate) fn toggle_glyphs(&mut self) {
        if let Some(session) = &mut self.sketch {
            session.glyphs = !session.glyphs;
        }
    }

    /// Clears the selection: the sketch's in a sketch, else the
    /// Timeline's and the model's.
    pub(crate) fn clear_selection(&mut self) {
        match &mut self.sketch {
            Some(session) => session.selection.clear(),
            None => {
                self.selected_feature = None;
                self.clear_model_selection();
            }
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
            let selected: Vec<Selectable> = selection.copied().collect();
            let item_ids: Vec<Id> = selected.iter().filter_map(|target| target.item()).collect();
            let items = |picked: Vec<Id>| picked.into_iter().map(Selectable::Item).collect();
            match (tool, selected.as_slice()) {
                (Tool::Mirror, _) => items(shape::mirrorable(sketch, &item_ids)),
                (Tool::Offset, _) => items(shape::offsettable(sketch, &item_ids)),
                (Tool::Fillet | Tool::Chamfer, _) => items(shape::cornered(sketch, &item_ids)),
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
        session.inserting = false;
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
        // A row removed goes without an exit; one coming back (an undo)
        // isn't under the cursor for it.
        self.hovered_feature =
            (self.hovered_feature).filter(|&id| !replaced && document.feature(id).is_some());
        // A row's menu goes with what the row lists, a feature of the
        // Timeline's with its selection.
        self.row_menu = self.row_menu.filter(|menu| match *menu {
            RowMenu::Feature(id) => self.selected_feature == Some(id),
            RowMenu::Sketch(id) => !replaced && document.feature(id).is_some(),
            RowMenu::Body(id) => !replaced && document.body(id).is_some(),
            RowMenu::Item(id) => {
                !replaced && (self.edited_sketch()).is_some_and(|(_, sketch)| sketch.selectable(id))
            }
            RowMenu::Link(id) => {
                !replaced
                    && (self.edited_sketch()).is_some_and(|(_, sketch)| sketch.link(id).is_some())
            }
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
            session.inserting = false;
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
                session.selection.retain(|&id| sketch.selectable(id));
                session.expanded.retain(|&id| sketch.curve(id).is_some());
                follow_selection(&session.selection, &mut session.listed_on, sketch);
                session.hovered = session.hovered.filter(|&id| sketch.selectable(id));
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
                    && (drawing.picked.iter().any(|&id| !sketch.selectable(id))
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
                        if target
                            .and_then(|target| target.item())
                            .is_some_and(|item| sketch.kind(item).is_none())
                        {
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
        let FeatureKind::Sketch { plane, sketch, .. } = &feature.kind else {
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
            modified: *sketch != session.entered,
            plane: *plane,
            placement: session.placement,
            sketch: self.shown_sketch()?,
            // Faded only once they've waited `feed::SLOW`, as the status
            // bar says so: most are answered well within it.
            pending: match waiting {
                Some(waiting) if self.proposals.slow() => &waiting.added,
                _ => &NONE,
            },
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
                params: self.params_in(),
            }),
            label_drag: session.label.map(|label| (label.id, label.by)),
            snap: session.snap,
            aim: session.aim.map(|click| click.at),
            profiles: session.profiles.as_ref().map(|found| &found.profiles),
            comb: session.comb,
            inserting: session.inserting,
            failing: &session.failing,
            links: &session.links,
            link_menu: match self.row_menu {
                Some(RowMenu::Link(link)) => Some(link),
                _ => None,
            },
            item_menu: match self.row_menu {
                Some(RowMenu::Item(item)) => Some(item),
                _ => None,
            },
            folded: &session.folded,
            expanded: &session.expanded,
            editable: self.editable(),
        })
    }

    /// The sketch being edited as it's shown: as the solver last solved it
    /// while it's dragged, else with the edits waiting on the solver
    /// applied, else as committed.
    pub(super) fn shown_sketch(&self) -> Option<&Sketch> {
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
        let FeatureKind::Sketch { plane, sketch, .. } = &feature.kind else {
            return None;
        };
        Some((*plane, sketch))
    }

    /// The sketch being edited as the document holds it, if one is, and
    /// what its links come from, one for each in their order.
    pub(crate) fn edited_links(&self) -> Option<(&Sketch, &[LinkSource])> {
        let session = self.sketch.as_ref()?;
        let feature = self.editor.document().feature(session.feature)?;
        let FeatureKind::Sketch {
            sketch, sources, ..
        } = &feature.kind
        else {
            return None;
        };
        Some((sketch, sources))
    }

    /// The camera looking straight at the sketch being edited, if one is,
    /// framing it, from its normal with its `y` up: what Home turns to in
    /// a sketch.
    pub(crate) fn sketch_camera(&self) -> Option<Camera> {
        let (_, sketch) = self.edited_sketch()?;
        let placement = self.sketch.as_ref()?.placement;
        Some(facing(self.camera.projection(), None, placement, sketch))
    }

    /// The camera turning to the sketch being edited, if one is, as
    /// entering it or its plane moving does: as [`Self::sketch_camera`],
    /// but from the side of the plane and turned as near as it can be to
    /// the view now (or where it's turning to), so the view changes least.
    pub(crate) fn turn_to_sketch(&self) -> Option<Camera> {
        let (_, sketch) = self.edited_sketch()?;
        let placement = self.sketch.as_ref()?.placement;
        let now = self.animation.as_ref().map_or(self.camera, |a| a.to);
        Some(facing(now.projection(), Some(&now), placement, sketch))
    }
}

/// The camera in `projection` looking straight at `sketch` on `placement`,
/// framing what's drawn of it, its points and its curves, or if it has
/// none, where `now` (else Home) looks brought onto the plane, at `now`'s
/// zoom. With a view `now`, it
/// looks from the side of the plane `now` looks from, and keeps up on
/// screen as near as it can to `now`'s (see [`facing_turn`]); else from
/// the placement's normal with its `y` up.
fn facing(
    projection: Projection,
    now: Option<&Camera>,
    placement: Placement,
    sketch: &Sketch,
) -> Camera {
    let mut camera = home_camera(projection);
    let (normal, up) = match now {
        Some(now) => facing_turn(now, placement),
        None => (placement.normal.as_vec3(), placement.y.as_vec3()),
    };
    camera.face(normal, up);
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
            // No closer than the default camera sees, for a lone point.
            let size = (high - low).max_element() as f32 * FRAME_MARGIN;
            camera.set_view_height(size.max(Camera::default().view_height()));
        }
        // Nothing drawn: where the view looks (Home's without one)
        // brought onto the plane, at the view's zoom, so a new sketch
        // doesn't jump away from what was being looked at.
        None => {
            let target = now.map_or(HOME_TARGET, |now| now.target());
            let normal = placement.normal.as_vec3();
            let above = normal.dot(target - placement.origin.as_vec3());
            camera.set_target(target - normal * above);
            if let Some(now) = now {
                camera.set_view_height(now.view_height());
            }
        }
    }
    camera
}

/// Which way to face a sketch on `placement` from `now`: the normal to
/// look from, the placement's own or its reverse, whichever side `now`
/// looks from (the placement's for a view along the plane), and which of
/// the sketch's axes, `±x` or `±y`, to put up on screen, the one nearest
/// `now`'s up. Steps of a quarter turn keep the sketch's axes square on
/// screen. The camera keeps world Z up unless it looks straight down or
/// up, so only then does the axis chosen turn it.
fn facing_turn(now: &Camera, placement: Placement) -> (Vec3, Vec3) {
    let normal = placement.normal.as_vec3();
    let normal = if now.backward().dot(normal) < 0.0 {
        -normal
    } else {
        normal
    };
    let up = now.up();
    let (x, y) = (placement.x.as_vec3(), placement.y.as_vec3());
    let axes = [y, x, -y, -x];
    // The first wins ties, so a view square to the plane keeps `y` up.
    let best = axes
        .into_iter()
        .fold((y, f32::NEG_INFINITY), |best, axis| {
            let score = axis.dot(up);
            if score > best.1 { (axis, score) } else { best }
        })
        .0;
    (normal, best)
}

/// Keeps `listed_on`, the points and curves the Constraints list lists
/// the constraints and dimensions on, in step with the `selection` of
/// `sketch`: the points and curves selected, or none, listing all, when
/// nothing is. While only constraints and dimensions are selected it
/// stays as it was, less what's gone, so one picked from the list, or a
/// second click on it, finds it where it was.
fn follow_selection(
    selection: &BTreeSet<Selectable>,
    listed_on: &mut BTreeSet<Id>,
    sketch: &Sketch,
) {
    let geometry = |id: &Id| {
        sketch
            .kind(*id)
            .is_some_and(|kind| Role::Geometry.admits(kind))
    };
    // A handle as a line, by its tip, which its constraints name.
    let selected: BTreeSet<Id> = selection
        .iter()
        .map(|target| target.id())
        .filter(geometry)
        .collect();
    if selected.is_empty() && !selection.is_empty() {
        listed_on.retain(geometry);
    } else {
        *listed_on = selected;
    }
}

#[cfg(test)]
mod tests;
