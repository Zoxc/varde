//! An offset face in the move's session ([`MotionKind::OffsetFace`]): its
//! faces picked as a face session's (see `faces`), its distance typed or
//! dragged by its handle, Inward (the session's flip) and Tangent faces.
//!
//! The handle stands at the first face's point along its outward
//! normal, as the face is before the offset ([`Anchor`]): found once on
//! the model shown, taken back by the distance the preview moved it if
//! the model shows a working preview of this session, and kept while the
//! first face and the document stay as they were, so the handle doesn't
//! move with the face it moves (its knob does).

use glam::DVec3;
use varde_document::{FeatureKind, OffsetFace};
use varde_view::{MotionField, MotionKind, OffsetFaceView};

use super::refs::Refs;
use super::{Anchor, Doc, Found, MotionSession};
use crate::doc::regions::TypedText;

impl MotionSession {
    /// The offset face as set up, if it's whole: its faces (at least
    /// one), its distance as it last read, Inward and Tangent faces.
    pub(super) fn offset_face(&self) -> Option<OffsetFace> {
        if self.faces.refs.is_empty() {
            return None;
        }
        Some(OffsetFace {
            faces: self.faces.refs.clone(),
            distance: self.field(MotionField::Distance).value.clone()?,
            inward: self.flip,
            tangent: self.tangent,
        })
    }

    /// Opens the offset face `offset` in this session: its faces,
    /// distance, side and Tangent faces.
    pub(super) fn open_offset_face(&mut self, offset: &OffsetFace) {
        let ask = OffsetFace::distance_ask(&self.read_in.design());
        self.fields[MotionField::Distance.index()] = TypedText::of(&offset.distance, &ask);
        self.flip = offset.inward;
        self.tangent = offset.tangent;
        self.faces = Refs::of(&offset.faces);
        self.faces_body();
    }

    /// The handle's knob: the distance as it last read, negative
    /// inward, zero while it doesn't read.
    fn signed_distance(&self) -> f64 {
        let value = (self.field(MotionField::Distance).value.as_ref()).map_or(0.0, |v| v.value);
        if self.flip { -value } else { value }
    }
}

/// An offset face's handle: an arrow from the first face's point along
/// its outward normal, as the face is before the offset, with a knob at
/// the distance the faces move (negative inward), which dragging sets,
/// through zero to the other side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct FaceHandle {
    /// The first face's point, on the face as the offset finds it.
    pub(super) origin: DVec3,
    /// The face's outward normal there, unit.
    pub(super) normal: DVec3,
    /// How far along the normal the knob is, in millimetres: the
    /// distance, negative inward; zero while the distance doesn't read.
    pub(super) at: f64,
}

impl Doc {
    /// What the panel shows of the offset face being set up.
    pub(super) fn offset_face_view(&self, session: &MotionSession) -> OffsetFaceView {
        let units = self.editor.document().units();
        OffsetFaceView {
            faces: self.picked_faces(session),
            inward: session.flip,
            tangent: session.tangent,
            info: (session.offset_face()).map(|offset| varde_view::offset_info(&offset, units)),
        }
    }

    /// The offset face's handle, once its first face is found on the
    /// model shown.
    pub(super) fn offset_face_handle(&self, session: &MotionSession) -> Option<FaceHandle> {
        let generation = self.editor.generation();
        (session.face_anchor(generation)).map(|(origin, normal)| FaceHandle {
            origin,
            normal,
            at: session.signed_distance(),
        })
    }

    /// Finds where the handle of the offset face being set up stands
    /// ([`Anchor::Face`]) if it isn't known for its first face and the
    /// document as they are: once the model shown answers what was asked
    /// last, on the first face found there, taken back by the distance
    /// the preview moved it where the model shows a working preview of
    /// this session's offset.
    pub(super) fn follow_offset_anchor(&mut self) {
        let generation = self.editor.generation();
        let Some(session) = self.mode.motion() else {
            return;
        };
        let Some(&first) = session.faces.refs.first() else {
            return;
        };
        if session.kind != MotionKind::OffsetFace
            || session.face_anchor(generation).is_some()
            || !self.feed.answers_request()
            || self.feed.predates_replacement()
        {
            return;
        }
        let index = self.feed.pick_index();
        let Some((_, Some(found))) = session.faces.found(index.model()).next() else {
            return;
        };
        let Some((at, normal)) = index.face_point(found, first.near) else {
            return;
        };
        let moved = match self.motion_draft() {
            Some((_, FeatureKind::OffsetFace(offset)))
                if self.feed.draft_error().is_none() && self.feed.shows_draft_of_run() =>
            {
                offset.signed_distance()
            }
            _ => 0.0,
        };
        let at = at - normal * moved;
        if !at.is_finite() {
            return;
        }
        if let Some(session) = self.mode.motion_mut() {
            session.anchor = Some(Anchor::Face(Found {
                key: first,
                generation,
                value: (at, normal),
            }));
        }
    }
}
