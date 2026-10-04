//! The faces a face session picks, in the move's session: a shell's
//! ([`MotionKind::Shell`]), and an offset face's and a draft's once there
//! are those, which take the same session with their own values (each
//! kind adds itself to [`MotionKind::picks_faces`], [`limit`], [`verb`],
//! [`MotionSession::faces_need`] and [`MotionSession::prune_faces`]).
//! They're picked as `refs` has it: a click on a face of the model shown
//! picks it or, picked already, takes it out; named as of the feature,
//! all on one body, sorted as [`FaceRef::order`] keeps them, lit as
//! selected. On a preview, what's left of a face a shell removes (the
//! rim of its walls) keeps the face's name: it's lit as the face, and a
//! click on it takes the face out.
//!
//! The session's body is the faces' body; a shell's, while it has none,
//! is the body picked (in Objects, or the one selected or the model's
//! only one as it starts), which it hollows closed ([`takes_body`]). A
//! body merged into another before the feature takes the faces with it
//! on to the holder.

use varde_document::{Document, FaceRef, MAX_SHELL_FACES};
use varde_view::{MotionKind, Picked, PickedFace, PickedFaces};

use super::{Doc, MotionSession};
use crate::doc::feed::Merges;

/// The most faces a feature of `kind` takes.
pub(super) fn limit(kind: MotionKind) -> usize {
    match kind {
        MotionKind::Shell => MAX_SHELL_FACES,
        _ => 0,
    }
}

/// What a face session of `kind` does to its faces, in words: "removed".
pub(super) fn verb(kind: MotionKind) -> &'static str {
    match kind {
        MotionKind::Shell => "removed",
        _ => "picked",
    }
}

/// Whether a face session of `kind` takes a body with no faces picked: a
/// shell's, hollowed closed. Others' body is only their faces'.
pub(super) fn takes_body(kind: MotionKind) -> bool {
    kind == MotionKind::Shell
}

impl MotionSession {
    /// Keeps the body the faces' body, while it has faces: with none, a
    /// shell's body picked stays, and others have none.
    pub(super) fn faces_body(&mut self) {
        match self.faces.body() {
            Some(body) => self.bodies = vec![body],
            None if !takes_body(self.kind) => self.bodies.clear(),
            None => {}
        }
    }

    /// What's still to be done before it can be committed, the words for
    /// the status bar: a shell's body, from a face or picked (it needs no
    /// faces: with none it's closed); others' faces.
    pub(super) fn faces_need(&self) -> Option<&'static str> {
        match self.kind {
            MotionKind::Shell => {
                (self.bodies.is_empty()).then_some("pick faces to remove, or the body to hollow")
            }
            _ => self.faces.refs.is_empty().then_some("pick the faces"),
        }
    }

    /// The words for what it names being gone, if it is, the UI mock's:
    /// its faces, or with none, a shell's body picked.
    pub(super) fn faces_gone(&self) -> Option<&'static str> {
        let body = (self.bodies.iter()).any(|body| self.gone_bodies.contains(body));
        if self.faces.refs.is_empty() {
            body.then_some("A picked body is gone")
        } else {
            (body || self.faces.gone).then_some("A picked face is gone")
        }
    }

    /// Notes whether `document` no longer takes the faces at feature
    /// `index`, or no longer holds their body.
    pub(super) fn prune_faces(&mut self, document: &Document, index: usize) {
        let faces = &self.faces.refs;
        let Some(body) = self.faces.body() else {
            self.faces.gone = false;
            return;
        };
        let taken = match self.kind {
            MotionKind::Shell => document.check_face_set(index, body, faces).is_ok(),
            _ => true,
        };
        self.faces.gone = document.body(body).is_none() || !taken;
    }

    /// Moves the faces, and so the body, on to the bodies holding theirs
    /// where `merges` (the merges before the feature) has them merged,
    /// and a shell's body picked while it has none: whether they moved.
    pub(super) fn follow_faces(&mut self, merges: &Merges) -> bool {
        let mut body = false;
        for picked in &mut self.bodies {
            if let Some(holder) = merges.holder(*picked) {
                *picked = holder;
                body = true;
            }
        }
        self.follow_refs::<FaceRef>(merges) || body
    }
}

impl Doc {
    /// The faces of the face session to light in the model shown.
    pub(super) fn faces_lit(&self) -> Vec<Picked> {
        match &self.motion {
            Some(session) if session.kind.picks_faces() => self.refs_lit::<FaceRef>(),
            _ => Vec::new(),
        }
    }

    /// The faces of the face session as the panel lists them: "Face 2"
    /// by its place, with what kind of face it is where it's found on the
    /// model shown ("Planar face").
    pub(super) fn picked_faces(&self, session: &MotionSession) -> PickedFaces {
        let index = self.feed.pick_index();
        let faces = (session.faces.found(index.model()))
            .enumerate()
            .map(|(at, (face, found))| {
                let meta = found
                    .and_then(|found| index.picking().faces().get(found as usize))
                    .map(|found| varde_view::face_kind(&found.summary).to_owned());
                PickedFace {
                    face,
                    name: format!("Face {}", at + 1),
                    meta,
                }
            })
            .collect();
        PickedFaces { faces }
    }
}
