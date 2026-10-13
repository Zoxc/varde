//! Picking the plane a sketch goes on: a new sketch's, or another for a
//! sketch, from the origin planes or the model's flat faces, and where a
//! sketch on a face is placed ([`Doc::placement`]).

use std::borrow::Cow;

use glam::DVec3;
use varde_document::{
    Command, FaceRef, Feature, FeatureId, FeatureKind, Generation, Placement, Plane,
};
use varde_view::{CURVED_FACE, PickIndex, PlanePick, SketchLines};

use super::{Change, Doc, DocMode, OUT_OF_DATE};

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

impl Doc {
    /// Starts picking the plane for a new sketch, or backs out of
    /// picking a plane. Only outside a sketch, in a document that can be
    /// edited; an operation being set up is dropped.
    pub(crate) fn pick_plane(&mut self) {
        if self.mode.picking_plane().is_some() {
            self.stop_picking_plane();
        } else if self.editable() && self.mode.sketch().is_none() {
            self.mode = DocMode::PickingPlane(PickingPlane {
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
            || (self.mode.sketch()).is_some_and(|session| session.feature != id)
        {
            return;
        }
        let Some(pick) = self.change_pick(id, None) else {
            return;
        };
        let enter = self.mode.sketch().is_some();
        self.finish_sketch();
        self.selected_feature = Some(id);
        self.mode = DocMode::PickingPlane(PickingPlane { pick, enter });
    }

    /// Keeps the plane being picked in step with the document and the
    /// model shown, after each edit and answer: which faces take the
    /// sketch may change with an undo, and picking for a sketch goes with
    /// the sketch, or across a replacement, where its id may name
    /// another.
    pub(crate) fn prune_plane_pick(&mut self, replaced: bool) {
        let Some(picking) = self.mode.picking_plane() else {
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
        match (pick, self.mode.picking_plane_mut()) {
            (Some(pick), Some(picking)) => picking.pick = pick,
            _ => {
                self.mode.take_picking_plane();
            }
        }
    }

    /// Backs out of picking a plane, leaving the document as it was: the
    /// sketch left for it is edited again, if it's placed.
    pub(super) fn stop_picking_plane(&mut self) {
        let Some(picking) = self.mode.take_picking_plane() else {
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
        let Some(picking) = self.mode.take_picking_plane() else {
            if let Plane::Origin(_) = plane {
                self.change(Change::NewSketch(plane));
            }
            return;
        };
        let (plane, placed) = match self.resolve_plane(plane, &picking.pick) {
            Ok(resolved) => resolved,
            Err(why) => {
                self.notice = Some(why.into_owned());
                self.mode = DocMode::PickingPlane(picking);
                return;
            }
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
        if self.editor.revision() != before {
            self.remember_placed(feature, plane, placed);
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
        self.mode.take_picking_plane();
        let (plane, placed) = match self.resolve_plane(plane, &self.new_sketch_pick()) {
            Ok(resolved) => resolved,
            Err(why) => {
                self.notice = Some(why.into_owned());
                return;
            }
        };
        let before = self.editor.revision();
        let at = self.editor.document().insert_at();
        self.apply(self.editor.document().add_sketch(plane));
        if self.editor.revision() != before
            && let Some(feature) = self.editor.document().features().get(at)
        {
            let feature = feature.id;
            self.remember_placed(feature, plane, placed);
            self.enter_sketch(feature);
        }
    }

    /// The plane a sketch picked on `plane` goes on, and where if that's
    /// a face: an origin plane as it is, a face as [`Doc::face_placement`]
    /// places it for `pick`, or refused, why.
    fn resolve_plane(
        &self,
        plane: Plane,
        pick: &PlanePick,
    ) -> Result<(Plane, Option<Placement>), Cow<'static, str>> {
        match plane {
            Plane::Origin(_) => Ok((plane, None)),
            Plane::Face(face) => {
                let (face, placement) = self.face_placement(&face, pick)?;
                Ok((Plane::Face(face), Some(placement)))
            }
        }
    }

    /// Notes that the sketch `feature`, just put on `plane`, is `placed`
    /// there as of this generation, if it's on a face, for
    /// [`Doc::placement`] to use until a model places it.
    fn remember_placed(&mut self, feature: FeatureId, plane: Plane, placed: Option<Placement>) {
        if let Some(placement) = placed {
            self.placed = Some(Placed {
                feature,
                plane,
                placement,
                generation: self.editor.generation(),
            });
        }
    }

    /// The face selected in the model, if a face alone is and the cursor
    /// picks the model, outside picking a plane: what `S` puts a new
    /// sketch on.
    pub(crate) fn selected_face(&self) -> Option<FaceRef> {
        (self.picks() && self.mode.sketch().is_none() && self.mode.picking_plane().is_none())
            .then(|| self.pick.selection.single_face())
            .flatten()
    }

    /// Why the model shown failed the feature `id`, if it did.
    pub(super) fn failure(&self, id: FeatureId) -> Option<String> {
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
    pub(super) fn change_pick(&self, id: FeatureId, failed: Option<String>) -> Option<PlanePick> {
        PlanePick::change(self.editor.document(), id, failed, self.shown())
    }

    /// The face a sketch on `face` goes on and where, as the model shown
    /// has the face (a merged body's on the body holding it): see
    /// [`face_placement_in`]. Refused, why, if the face isn't found there
    /// (the cursor doesn't pick that model), or as that refuses it.
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
        face_placement_in(index, found, face.near, pick)
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
}

/// The face a sketch on face `found` of `index`'s model, picked at
/// `near`, goes on and where: the face as `pick` names it
/// ([`PlanePick::face_ref`]), and [`Placement::on_plane`] of its plane,
/// the bits regenerating will place it by. Refused, why, if the face
/// isn't flat, can't take the sketch `pick` is for, or is too far out to
/// sketch on. A click's on the model shown, and the MCP client's on the
/// committed model, its key found there.
pub(crate) fn face_placement_in(
    index: &PickIndex,
    found: u32,
    near: DVec3,
    pick: &PlanePick,
) -> Result<(FaceRef, Placement), Cow<'static, str>> {
    if let Some(why) = pick.refusal(index, found) {
        return Err(why);
    }
    let placement = index.face_placement(found).ok_or(CURVED_FACE)?;
    let face = (pick.face_ref(index, found, near)).ok_or("That face isn't in the model shown")?;
    if placement.valid() {
        Ok((face, placement))
    } else {
        Err("That face is too far out to sketch on".into())
    }
}
