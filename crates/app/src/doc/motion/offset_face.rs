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
use varde_document::{Design, FaceRef, FeatureKind, Generation, OffsetFace};
use varde_render::Camera;
use varde_view::{FaceHandle, MotionField, MotionKind, OffsetFaceView};

use crate::doc::camera::fitting_length;

use super::length_field;
use super::refs::Refs;
use super::{Doc, MotionSession};
use crate::doc::regions::TypedText;

/// Where an offset face's handle stands: `at`, on the first face `face`
/// as it is before the offset, and the face's outward `normal` there,
/// found on the model of the document at `generation`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Anchor {
    face: FaceRef,
    generation: Generation,
    at: DVec3,
    normal: DVec3,
}

/// The distance field as a new one opens it, seen by `camera`:
/// [`DISTANCE_SHARE`] of the view's height made nice
/// ([`fitting_length`]), in the design's units with their symbol.
pub(super) fn distance_field(design: &Design, camera: &Camera) -> TypedText {
    let length = fitting_length(camera, design.units, DISTANCE_SHARE);
    length_field(length, &OffsetFace::distance_ask(design), design)
}

/// The part of the view's height a new offset face's distance starts at,
/// at most.
const DISTANCE_SHARE: f64 = super::BLEND_SHARE / 2.0;

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
        let ask = OffsetFace::distance_ask(&self.design);
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

impl Doc {
    /// What the panel shows of the offset face being set up.
    pub(super) fn offset_face_view(&self, session: &MotionSession) -> OffsetFaceView {
        let units = self.editor.document().units();
        let generation = self.editor.generation();
        let handle = (session.anchor.as_ref())
            .filter(|anchor| {
                anchor.generation == generation && session.faces.refs.first() == Some(&anchor.face)
            })
            .map(|anchor| FaceHandle {
                origin: anchor.at,
                normal: anchor.normal,
                at: session.signed_distance(),
            });
        OffsetFaceView {
            faces: self.picked_faces(session),
            inward: session.flip,
            tangent: session.tangent,
            handle,
            info: (session.offset_face())
                .filter(|_| session.kind == MotionKind::OffsetFace)
                .map(|offset| varde_view::offset_info(&offset, units)),
        }
    }

    /// Finds where the handle of the offset face being set up stands
    /// ([`Anchor`]) if it isn't known for its first face and the
    /// document as they are: once the model shown answers what was asked
    /// last, on the first face found there, taken back by the distance
    /// the preview moved it where the model shows a working preview of
    /// this session's offset.
    pub(super) fn follow_offset_anchor(&mut self) {
        let generation = self.editor.generation();
        let Some(session) = &self.motion else {
            return;
        };
        let Some(&first) = session.faces.refs.first() else {
            return;
        };
        let current = |anchor: &Anchor| anchor.face == first && anchor.generation == generation;
        if session.kind != MotionKind::OffsetFace
            || session.anchor.as_ref().is_some_and(current)
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
        if let Some(session) = &mut self.motion {
            session.anchor = Some(Anchor {
                face: first,
                generation,
                at,
                normal,
            });
        }
    }
}

/// The feature an offset face session makes.
pub(super) fn offset_face_kind(session: &MotionSession) -> Option<FeatureKind> {
    session.offset_face().map(FeatureKind::OffsetFace)
}
