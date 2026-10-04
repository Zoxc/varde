//! The faces a face session picks, in the move's session: a shell's
//! ([`MotionKind::Shell`]), and an offset face's and a draft's once there
//! are those, which take the same session with their own values. Each
//! click on a face of the model shown picks it or, picked already, takes
//! it out; the faces are named as of the feature
//! ([`Naming`](varde_view::Naming)), on the body holding them there, all
//! on one body (once one is picked, the faces of other bodies don't light
//! under the cursor and are refused), kept sorted as the feature stores
//! them ([`FaceRef::order`]), and lit as selected, found again by their
//! names on each model shown. The session's body is the faces' body, or,
//! while it has none, the body picked (in Objects, or the one selected or
//! the model's only one as it starts), which a shell with no faces
//! hollows closed; a body merged into another before the feature takes
//! the faces with it on to the holder.

use std::borrow::Cow;

use varde_document::{BodyId, Document, FaceRef, MAX_SHELL_FACES};
use varde_view::{MotionKind, Pick, PickIndex, Picked, PickedFace, PickedFaces, Unnamed};

use super::{Doc, MotionSession, OUT_OF_DATE, unnamed};
use crate::doc::feed::Merges;

/// A face session's faces as picked, and where they are on a model
/// shown.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct FaceSetup {
    /// At most [`FaceSetup::limit`], all on one body, in
    /// [`FaceRef::order`] without repeats, as the feature stores them.
    pub(crate) faces: Vec<FaceRef>,
    /// Each face's place on a model shown, by `faces`' order: where it
    /// was picked, or found again by its names on one shown since
    /// ([`FaceSetup::follow`]).
    marks: Vec<Mark>,
    /// Whether the faces are gone: the document no longer takes them at
    /// the feature's place (an undo took a face's maker away). Kept, said
    /// to be gone, until they're taken out or back.
    gone: bool,
}

/// Where a face is on the model `model`: the mesh's face, if it's there.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Mark {
    model: u64,
    face: Option<u32>,
}

impl FaceSetup {
    /// The faces `faces` (a stored feature's), picked on no model shown.
    pub(super) fn of(faces: &[FaceRef]) -> Self {
        Self {
            faces: faces.to_vec(),
            marks: vec![
                Mark {
                    model: 0,
                    face: None
                };
                faces.len()
            ],
            gone: false,
        }
    }

    /// The body its faces are on, if it has any.
    fn body(&self) -> Option<BodyId> {
        self.faces.first().map(|face| face.body)
    }

    /// The most faces a feature of `kind` takes.
    fn limit(kind: MotionKind) -> usize {
        match kind {
            MotionKind::Shell => MAX_SHELL_FACES,
            _ => 0,
        }
    }

    /// The face picked that is `face` of the model `model`, if one is:
    /// where a click on it takes it out.
    fn picked(&self, model: u64, face: u32) -> Option<usize> {
        (self.marks.iter()).position(|mark| mark.model == model && mark.face == Some(face))
    }

    /// Adds `face`, picked as `target` of the model `model`, in its
    /// place; not past the limit, nor again.
    fn add(
        &mut self,
        kind: MotionKind,
        face: FaceRef,
        model: u64,
        target: u32,
    ) -> Result<(), Cow<'static, str>> {
        let at = match self.faces.binary_search_by(|picked| picked.order(&face)) {
            Ok(_) => return Ok(()),
            Err(at) => at,
        };
        let limit = Self::limit(kind);
        if self.faces.len() >= limit {
            let noun = noun(kind);
            return Err(format!("A {noun} takes at most {limit} faces").into());
        }
        self.faces.insert(at, face);
        let mark = Mark {
            model,
            face: Some(target),
        };
        self.marks.insert(at, mark);
        Ok(())
    }

    /// Takes out the face at `at`.
    fn remove(&mut self, at: usize) {
        self.faces.remove(at);
        self.marks.remove(at);
        if self.faces.is_empty() {
            self.gone = false;
        }
    }

    /// Takes `face` out, if it's picked.
    pub(super) fn drop_face(&mut self, face: &FaceRef) {
        if let Some(at) = self.faces.iter().position(|picked| picked == face) {
            self.remove(at);
        }
    }

    /// Finds the faces again on `index`'s model where they aren't marked
    /// on it yet, by their names on the body `shown` says draws theirs
    /// there.
    fn follow(&mut self, index: &PickIndex, shown: impl Fn(BodyId) -> BodyId) {
        let model = index.model();
        for (face, mark) in self.faces.iter().zip(&mut self.marks) {
            if mark.model != model {
                *mark = Mark {
                    model,
                    face: index.find_face(shown(face.body), &face.key, face.near),
                };
            }
        }
    }

    /// The faces picked lit on `index`'s model, those marked there.
    fn lit(&self, index: &PickIndex) -> Vec<Picked> {
        let model = index.model();
        let mut lit: Vec<u32> = (self.marks.iter())
            .filter(|mark| mark.model == model)
            .filter_map(|mark| mark.face)
            .collect();
        lit.sort_unstable();
        lit.dedup();
        lit.into_iter().map(Picked::Face).collect()
    }

    /// Moves the faces on bodies `merges` has merged into others on to
    /// the bodies holding them, keeping their order: whether any moved.
    fn follow_merges(&mut self, merges: &Merges) -> bool {
        let mut moved = false;
        for face in &mut self.faces {
            if let Some(holder) = merges.holder(face.body) {
                face.body = holder;
                moved = true;
            }
        }
        if moved {
            let mut pairs: Vec<(FaceRef, Mark)> = (self.faces.iter().copied())
                .zip(self.marks.iter().copied())
                .collect();
            pairs.sort_by(|a, b| a.0.order(&b.0));
            pairs.dedup_by(|a, b| a.0.order(&b.0).is_eq());
            (self.faces, self.marks) = pairs.into_iter().unzip();
        }
        moved
    }
}

impl MotionSession {
    /// Keeps the body the faces' body, while it has faces: with none, the
    /// body picked stays.
    pub(super) fn faces_body(&mut self) {
        if let Some(body) = self.faces.body() {
            self.bodies = vec![body];
        }
    }

    /// What's still to be done before it can be committed, the words for
    /// the status bar: the body, from a face or picked. Faces themselves
    /// aren't needed: a shell with none is closed.
    pub(super) fn faces_need(&self) -> Option<&'static str> {
        let words = match self.kind {
            MotionKind::Shell => "pick faces to remove, or the body to hollow",
            _ => "pick the faces",
        };
        self.bodies.is_empty().then_some(words)
    }

    /// The words for its faces being gone, if they are, the UI mock's.
    pub(super) fn faces_gone(&self) -> Option<&'static str> {
        (self.faces.gone && !self.faces.faces.is_empty()).then_some("A picked face is gone")
    }

    /// Notes whether `document` no longer takes the faces at feature
    /// `index`, or no longer holds their body.
    pub(super) fn prune_faces(&mut self, document: &Document, index: usize) {
        let faces = &self.faces.faces;
        let Some(body) = self.faces.body() else {
            self.faces.gone = false;
            return;
        };
        let taken = match self.kind {
            MotionKind::Shell => document.check_shell_faces(index, body, faces).is_ok(),
            _ => true,
        };
        self.faces.gone = document.body(body).is_none() || !taken;
    }

    /// Moves the faces, and so the body, on to the bodies holding theirs
    /// where `merges` (the merges before the feature) has them merged,
    /// and the body picked while it has none: whether they moved.
    pub(super) fn follow_faces(&mut self, merges: &Merges) -> bool {
        let moved = self.faces.follow_merges(merges);
        let mut body = false;
        for picked in &mut self.bodies {
            if let Some(holder) = merges.holder(*picked) {
                *picked = holder;
                body = true;
            }
        }
        self.faces_body();
        moved || body
    }
}

/// What a face session of `kind` is called in words: "shell".
fn noun(kind: MotionKind) -> String {
    kind.noun().to_lowercase()
}

/// What a face session of `kind` does to its faces, in words: "removed".
fn verb(kind: MotionKind) -> &'static str {
    match kind {
        MotionKind::Shell => "removed",
        _ => "picked",
    }
}

impl Doc {
    /// `pick` of the model shown as a face of the face session: any face,
    /// named as of the feature on the body holding it there, which is
    /// made before it, and the faces' body once one is picked. Refused,
    /// why, if not.
    pub(super) fn face_of(&self, pick: Pick) -> Result<FaceRef, Cow<'static, str>> {
        let session = self.motion.as_ref().ok_or("Nothing is set up")?;
        let kind = session.kind;
        let Picked::Face(face) = pick.target else {
            return Err(format!("Only a face can be {}", verb(kind)).into());
        };
        let naming = self.motion_naming().ok_or("Nothing is set up")?;
        let index = self.feed.pick_index();
        let named = naming
            .checked_face_ref(index, face, pick.at)
            .map_err(|why| unnamed(why, "face", kind))?;
        let document = self.editor.document();
        let merged = self.feed.merged_before(document, session.feature);
        let held = merged.holder(named.body).unwrap_or(named.body);
        if let Some(body) = session.faces.body()
            && body != held
        {
            let name = (document.body(body)).map_or("one body", |body| body.name.as_str());
            return Err(format!(
                "A {}'s faces are all on one body: pick faces of {name}",
                noun(kind)
            )
            .into());
        }
        if !super::super::combine::pickable(document, held, session.feature) {
            return Err(unnamed(Unnamed::Later, "body", kind));
        }
        Ok(FaceRef {
            body: held,
            ..named
        })
    }

    /// Whether a click on `pick` takes a face out: it's one picked.
    fn face_picked(&self, pick: Pick) -> Option<usize> {
        let session = self.motion.as_ref()?;
        let Picked::Face(face) = pick.target else {
            return None;
        };
        if pick.model != self.feed.pick_index().model() {
            return None;
        }
        session.faces.picked(pick.model, face)
    }

    /// Whether `pick` lights as the cursor's over it: a face a click
    /// would pick or take out.
    pub(super) fn faces_takes(&self, pick: Pick) -> bool {
        self.blend_model_current()
            && (self.face_picked(pick).is_some() || self.face_of(pick).is_ok())
    }

    /// Takes a click on `pick` while a face session's faces are picked: a
    /// face picked already is taken out, another picked, or why it can't
    /// be.
    pub(super) fn faces_click(&mut self, pick: Pick) -> Result<(), Cow<'static, str>> {
        if !self.blend_model_current() {
            return Err(OUT_OF_DATE.into());
        }
        if let Some(at) = self.face_picked(pick) {
            if let Some(session) = &mut self.motion {
                session.faces.remove(at);
                session.faces_body();
            }
            return Ok(());
        }
        let face = self.face_of(pick)?;
        let Picked::Face(target) = pick.target else {
            return Ok(());
        };
        let Some(session) = &mut self.motion else {
            return Ok(());
        };
        session.faces.add(session.kind, face, pick.model, target)?;
        session.faces_body();
        Ok(())
    }

    /// Picks the faces selected in the model shown, those a click would
    /// pick, for the face session just started: as the UI mock's shell
    /// takes the face selected. The first one's body decides, over the
    /// body the session started with.
    pub(super) fn faces_selected(&mut self) {
        let index = self.feed.pick_index();
        let model = index.model();
        if self.pick.selection.model() != Some(model) {
            return;
        }
        let picks: Vec<Pick> = (self.pick.selection.items())
            .filter_map(|item| match *item {
                varde_view::Selected::Face { body, key, near } => {
                    let face = index.find_face(body, &key, near)?;
                    Some(Pick {
                        model,
                        target: Picked::Face(face),
                        body,
                        at: near,
                        snap: None,
                    })
                }
                _ => None,
            })
            .collect();
        for pick in picks {
            // One refused (on another body, made later) is left out.
            let _ = self.faces_click(pick);
        }
    }

    /// Finds the faces of the face session again on the model shown,
    /// where they aren't marked yet, each on the body drawing its body
    /// there.
    pub(super) fn follow_face_marks(&mut self) {
        let Some(session) = &mut self.motion else {
            return;
        };
        if !session.kind.picks_faces() {
            return;
        }
        let merged = self.feed.merged_bodies();
        let shown = |body: BodyId| {
            (merged.iter())
                .find(|(consumed, _)| *consumed == body)
                .map_or(body, |&(_, holder)| holder)
        };
        session.faces.follow(self.feed.pick_index(), shown);
    }

    /// The faces of the face session to light in the model shown.
    pub(super) fn faces_lit(&self) -> Vec<Picked> {
        match &self.motion {
            Some(session) if session.kind.picks_faces() => {
                session.faces.lit(self.feed.pick_index())
            }
            _ => Vec::new(),
        }
    }

    /// The face whose row in the panel is hovered, if it's marked on the
    /// model shown.
    pub(super) fn face_hovered(&self, at: usize) -> Option<Picked> {
        let session = self.motion.as_ref()?;
        let mark = session.faces.marks.get(at)?;
        (mark.model == self.feed.model())
            .then_some(mark.face)
            .flatten()
            .map(Picked::Face)
    }

    /// The faces of the face session as the panel lists them: "Face 2"
    /// by its place, with what kind of face it is where it's found on the
    /// model shown ("Planar face").
    pub(super) fn picked_faces(&self, session: &MotionSession) -> PickedFaces {
        let index = self.feed.pick_index();
        let model = index.model();
        let faces = (session.faces.faces.iter().zip(&session.faces.marks))
            .enumerate()
            .map(|(at, (&face, mark))| {
                let found = (mark.model == model).then_some(mark.face).flatten();
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
