//! What the sessions picking several edges or faces of one body share: a
//! blend's edges (see `blend`) and a face session's faces (see `faces`).
//! Each click on an edge or face of the model shown picks it or, picked
//! already, takes it out; they're named as of the feature
//! ([`Naming`]), on the body holding them there, all on one body (once
//! one is picked, those of other bodies don't light under the cursor and
//! are refused), kept sorted as the feature stores them, and lit as
//! selected, found again by their names on each model shown. A body
//! merged into another before the feature takes them with it on to the
//! holder.

use std::borrow::Cow;
use std::cmp::Ordering;

use glam::DVec3;
use varde_document::{BodyId, EdgeRef, FaceRef};
use varde_view::{MotionKind, MotionPick, Naming, Pick, PickIndex, Picked, Selected, Unnamed};

use super::{Doc, MotionSession, OUT_OF_DATE, unnamed};
use crate::doc::feed::Merges;

/// An edge or face reference a session picks several of.
pub(crate) trait Ref: Copy + PartialEq {
    /// What it is in words: "edge", "face".
    const NOUN: &'static str;
    /// The order lists of them are kept in ([`EdgeRef::order`],
    /// [`FaceRef::order`]).
    fn order(&self, other: &Self) -> Ordering;
    fn body(&self) -> BodyId;
    fn body_mut(&mut self) -> &mut BodyId;
    /// Where it was picked.
    fn near(&self) -> DVec3;
    /// The mesh's edge or face `picked` is, if it's one of these.
    fn target(picked: Picked) -> Option<u32>;
    /// The mesh's edge or face `target` as picked.
    fn picked(target: u32) -> Picked;
    /// It on `index`'s model, drawn on `body`.
    fn find(&self, index: &PickIndex, body: BodyId) -> Option<u32>;
    /// The mesh's `target` picked at `at` as a feature stores it.
    fn named(naming: &Naming, index: &PickIndex, target: u32, at: DVec3) -> Result<Self, Unnamed>;
    /// What's selected as one of these, if it is: the body it's on, the
    /// reference.
    fn selected(item: &Selected) -> Option<Self>;
    /// The status bar's words for a click on another kind of thing in a
    /// session of `kind`: "Only an edge can be chamfered".
    fn only(kind: MotionKind) -> String;
    /// The most a feature of `kind` takes.
    fn limit(kind: MotionKind) -> usize;
    /// The session's picks of these.
    fn refs(session: &MotionSession) -> &Refs<Self>;
    fn refs_mut(session: &mut MotionSession) -> &mut Refs<Self>;
    /// The mesh's ones a click on `target` lights and matches: a blend's
    /// edge with its tangent chain while it takes them in; on a shell's
    /// own preview, a face with the other pieces of the face it's part
    /// of. `doc` is the doc showing `index`.
    fn grown(doc: &Doc, session: &MotionSession, index: &PickIndex, target: u32) -> Vec<u32>;
}

/// Several references picked, all on one body, and where they are on a
/// model shown.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Refs<R> {
    /// At most the kind's limit ([`Ref::limit`]), all on one body, in
    /// [`Ref::order`] without repeats, as the feature stores them.
    pub(crate) refs: Vec<R>,
    /// Each one's place on a model shown, by `refs`' order: where it was
    /// picked, or found again by its names on one shown since
    /// ([`Refs::follow`]).
    marks: Vec<Mark>,
    /// Whether they're gone: the document no longer takes them at the
    /// feature's place (an undo took their body or a face's maker away),
    /// or no longer holds their body. Kept, said to be gone, until
    /// they're taken out or back.
    pub(super) gone: bool,
}

impl<R> Default for Refs<R> {
    fn default() -> Self {
        Self {
            refs: Vec::new(),
            marks: Vec::new(),
            gone: false,
        }
    }
}

/// Where a reference is on the model `model`: the mesh's edge or face,
/// if it's there.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Mark {
    model: u64,
    at: Option<u32>,
}

impl<R: Ref> Refs<R> {
    /// The references `refs` (a stored feature's), picked on no model
    /// shown.
    pub(super) fn of(refs: &[R]) -> Self {
        let mark = Mark { model: 0, at: None };
        Self {
            refs: refs.to_vec(),
            marks: vec![mark; refs.len()],
            gone: false,
        }
    }

    /// The body they're on, if there are any.
    pub(super) fn body(&self) -> Option<BodyId> {
        self.refs.first().map(R::body)
    }

    /// The one picked that is one of `targets` of the model `model`, if
    /// one is: where a click on them takes it out.
    fn picked(&self, model: u64, targets: &[u32]) -> Option<usize> {
        (self.marks.iter())
            .position(|mark| mark.model == model && mark.at.is_some_and(|at| targets.contains(&at)))
    }

    /// Adds `found`, picked as `target` of the model `model`, in its
    /// place; not past `kind`'s limit, nor again.
    fn add(
        &mut self,
        kind: MotionKind,
        found: R,
        model: u64,
        target: u32,
    ) -> Result<(), Cow<'static, str>> {
        let at = match self.refs.binary_search_by(|picked| picked.order(&found)) {
            Ok(_) => return Ok(()),
            Err(at) => at,
        };
        let limit = R::limit(kind);
        if self.refs.len() >= limit {
            let (noun, what) = (noun(kind), R::NOUN);
            let a = article(&noun);
            return Err(format!("{a} {noun} takes at most {limit} {what}s").into());
        }
        self.refs.insert(at, found);
        let mark = Mark {
            model,
            at: Some(target),
        };
        self.marks.insert(at, mark);
        Ok(())
    }

    /// Takes out every one that is one of `targets` of the model
    /// `model`: all a click on a tangent chain takes out, where more than
    /// one of its edges were picked (apart, with Tangent chain off).
    fn remove_all(&mut self, model: u64, targets: &[u32]) {
        while let Some(at) = self.picked(model, targets) {
            self.remove(at);
        }
    }

    /// Takes out the one at `at`.
    fn remove(&mut self, at: usize) {
        self.refs.remove(at);
        self.marks.remove(at);
        if self.refs.is_empty() {
            self.gone = false;
        }
    }

    /// Takes `picked` out, if it's picked.
    pub(super) fn drop_ref(&mut self, picked: &R) {
        if let Some(at) = self.refs.iter().position(|r| r == picked) {
            self.remove(at);
        }
    }

    /// Finds them again on `index`'s model where they aren't marked on
    /// it yet, by their names on the body `shown` says draws theirs
    /// there.
    fn follow(&mut self, index: &PickIndex, shown: impl Fn(BodyId) -> BodyId) {
        let model = index.model();
        for (found, mark) in self.refs.iter().zip(&mut self.marks) {
            if mark.model != model {
                *mark = Mark {
                    model,
                    at: found.find(index, shown(found.body())),
                };
            }
        }
    }

    /// Where each is on the model `model`, by their order, if it's
    /// marked there.
    pub(super) fn found(&self, model: u64) -> impl Iterator<Item = (R, Option<u32>)> + '_ {
        (self.refs.iter().copied())
            .zip(&self.marks)
            .map(move |(r, mark)| (r, (mark.model == model).then_some(mark.at).flatten()))
    }

    /// Moves those on bodies `merges` has merged into others on to the
    /// bodies holding them, keeping their order: whether any moved.
    fn follow_merges(&mut self, merges: &Merges) -> bool {
        let mut moved = false;
        for found in &mut self.refs {
            if let Some(holder) = merges.holder(found.body()) {
                *found.body_mut() = holder;
                moved = true;
            }
        }
        if moved {
            let mut pairs: Vec<(R, Mark)> = (self.refs.iter().copied())
                .zip(self.marks.iter().copied())
                .collect();
            pairs.sort_by(|a, b| a.0.order(&b.0));
            pairs.dedup_by(|a, b| a.0.order(&b.0).is_eq());
            (self.refs, self.marks) = pairs.into_iter().unzip();
        }
        moved
    }
}

/// What a session of `kind` is called in words: "chamfer", "shell".
pub(super) fn noun(kind: MotionKind) -> String {
    kind.noun().to_lowercase()
}

/// The article starting a sentence about a `noun`: "A shell", "An
/// offset".
pub(super) fn article(noun: &str) -> &'static str {
    if noun.starts_with(['a', 'e', 'i', 'o', 'u']) {
        "An"
    } else {
        "A"
    }
}

impl MotionSession {
    /// Keeps its bodies those its references are on (a blend's, a face
    /// session's) as the session's own rules have it.
    pub(super) fn refs_body(&mut self) {
        if self.kind.blends() {
            self.blend_body();
        } else if self.kind.picks_faces() {
            self.faces_body();
        }
    }

    /// Moves the references of type `R`, and so the body, on to the
    /// bodies holding theirs where `merges` (the merges before the
    /// feature) has them merged: whether they moved.
    pub(super) fn follow_refs<R: Ref>(&mut self, merges: &Merges) -> bool {
        let moved = R::refs_mut(self).follow_merges(merges);
        self.refs_body();
        moved
    }
}

impl Doc {
    /// Whether the model shown names edges and faces as a session picking
    /// several asks for them: of the document as it is, not of one
    /// replaced whole since, and with a draft of this session's run (which
    /// of its drafts doesn't change those before the feature), or for a
    /// new feature none (added at the end, it changes nothing before it):
    /// not with the draft of another session just ended, which may have
    /// moved or cut them. Picks wait for nothing else, so they can be
    /// clicked one after another while the preview of the last comes.
    /// Those the feature itself makes (on its preview, or as stored) are
    /// refused by their names ([`Naming::before`]).
    pub(super) fn refs_model_current(&self) -> bool {
        let new = self.motion.as_ref().is_some_and(|s| s.feature.is_none());
        self.feed.generation() == Some(self.editor.generation())
            && !self.feed.predates_replacement()
            && (self.feed.shows_draft_of_run() || (new && !self.feed.shows_draft()))
    }

    /// `pick` of the model shown as a reference of type `R` for the
    /// session: named as of the feature on the body holding it there,
    /// which is made before it, and the body of those picked once one
    /// is. Refused, why, if not.
    fn ref_of<R: Ref>(&self, pick: Pick) -> Result<R, Cow<'static, str>> {
        let session = self.motion.as_ref().ok_or("Nothing is set up")?;
        let kind = session.kind;
        let target = R::target(pick.target).ok_or_else(|| R::only(kind))?;
        let naming = self.motion_naming().ok_or("Nothing is set up")?;
        let index = self.feed.pick_index();
        let mut named =
            R::named(&naming, index, target, pick.at).map_err(|why| unnamed(why, R::NOUN, kind))?;
        let document = self.editor.document();
        let merged = self.feed.merged_before(document, session.feature);
        let held = merged.holder(named.body()).unwrap_or(named.body());
        if let Some(body) = R::refs(session).body()
            && body != held
        {
            let name = (document.body(body)).map_or("one body", |body| body.name.as_str());
            let what = R::NOUN;
            return Err(format!(
                "{} {}'s {what}s are all on one body: pick {what}s of {name}",
                article(&noun(kind)),
                noun(kind)
            )
            .into());
        }
        if !super::super::combine::pickable(document, held, session.feature) {
            return Err(unnamed(Unnamed::Later, "body", kind));
        }
        *named.body_mut() = held;
        Ok(named)
    }

    /// Whether a click on `pick` takes a reference of type `R` out: it's
    /// one picked, or one it grows into ([`Ref::grown`]).
    fn ref_picked<R: Ref>(&self, pick: Pick) -> Option<usize> {
        let session = self.motion.as_ref()?;
        let target = R::target(pick.target)?;
        let index = self.feed.pick_index();
        if pick.model != index.model() {
            return None;
        }
        let grown = R::grown(self, session, index, target);
        R::refs(session).picked(pick.model, &grown)
    }

    /// Whether the session being set up has `pick` among the edges or
    /// faces it picks, so a click would take it out: `None` for one that
    /// picks neither (or no session), or a draft picking its neutral
    /// plane, where a click takes the face as the plane.
    pub(crate) fn motion_has(&self, pick: Pick) -> Option<bool> {
        let session = self.motion.as_ref()?;
        if session.picking == MotionPick::Reference {
            return None;
        }
        let kind = session.kind;
        if kind.blends() || kind == MotionKind::Sweep {
            Some(self.ref_picked::<EdgeRef>(pick).is_some())
        } else if kind.picks_faces() {
            Some(self.ref_picked::<FaceRef>(pick).is_some())
        } else {
            None
        }
    }

    /// Whether `pick` lights as the cursor's over it: one of type `R` a
    /// click would pick or take out.
    pub(super) fn refs_take<R: Ref>(&self, pick: Pick) -> bool {
        self.refs_model_current()
            && (self.ref_picked::<R>(pick).is_some() || self.ref_of::<R>(pick).is_ok())
    }

    /// Takes a click on `pick` while references of type `R` are picked:
    /// one picked already is taken out, another picked, or why it can't
    /// be.
    pub(super) fn refs_click<R: Ref>(&mut self, pick: Pick) -> Result<(), Cow<'static, str>> {
        if !self.refs_model_current() {
            return Err(OUT_OF_DATE.into());
        }
        if self.ref_picked::<R>(pick).is_some() {
            // Every one picked a click on it lights takes out: a tangent
            // chain's edges picked apart all go with it.
            let grown = R::target(pick.target).map_or_else(Vec::new, |target| {
                let session = self.motion.as_ref().expect("a session picked it");
                R::grown(self, session, self.feed.pick_index(), target)
            });
            if let Some(session) = &mut self.motion {
                R::refs_mut(session).remove_all(pick.model, &grown);
                session.refs_body();
            }
            return Ok(());
        }
        let found = self.ref_of::<R>(pick)?;
        let Some(target) = R::target(pick.target) else {
            return Ok(());
        };
        let Some(session) = &mut self.motion else {
            return Ok(());
        };
        let kind = session.kind;
        R::refs_mut(session).add(kind, found, pick.model, target)?;
        session.refs_body();
        Ok(())
    }

    /// Picks those of type `R` selected in the model shown, those a click
    /// would pick, for the session just started. The first one's body
    /// decides, over the body the session started with.
    pub(super) fn refs_selected<R: Ref>(&mut self) {
        let index = self.feed.pick_index();
        let model = index.model();
        if self.pick.selection.model() != Some(model) {
            return;
        }
        let picks: Vec<Pick> = (self.pick.selection.items())
            .filter_map(|item| {
                let found = R::selected(item)?;
                let target = found.find(index, found.body())?;
                Some(Pick {
                    model,
                    target: R::picked(target),
                    body: found.body(),
                    at: found.near(),
                    snap: None,
                })
            })
            .collect();
        for pick in picks {
            // One refused (on another body, made later) is left out.
            let _ = self.refs_click::<R>(pick);
        }
    }

    /// Finds those of type `R` the session picked again on the model
    /// shown, where they aren't marked yet, each on the body drawing its
    /// body there.
    pub(super) fn follow_ref_marks<R: Ref>(&mut self) {
        let Some(session) = &mut self.motion else {
            return;
        };
        let merged = self.feed.merged_bodies();
        let shown = |body: BodyId| {
            (merged.iter())
                .find(|(consumed, _)| *consumed == body)
                .map_or(body, |&(_, holder)| holder)
        };
        R::refs_mut(session).follow(self.feed.pick_index(), shown);
    }

    /// Those of type `R` the session picked to light in the model shown,
    /// each grown as a click on it lights ([`Ref::grown`]).
    pub(super) fn refs_lit<R: Ref>(&self) -> Vec<Picked> {
        let Some(session) = &self.motion else {
            return Vec::new();
        };
        let index = self.feed.pick_index();
        let mut lit: Vec<u32> = (R::refs(session).found(index.model()))
            .filter_map(|(_, at)| at)
            .flat_map(|at| R::grown(self, session, index, at))
            .collect();
        lit.sort_unstable();
        lit.dedup();
        lit.into_iter().map(R::picked).collect()
    }

    /// The one of type `R` whose row in the panel is hovered, if it's
    /// marked on the model shown.
    pub(super) fn ref_hovered<R: Ref>(&self, at: usize) -> Option<Picked> {
        let session = self.motion.as_ref()?;
        let (_, found) = R::refs(session).found(self.feed.model()).nth(at)?;
        found.map(R::picked)
    }
}

impl Ref for EdgeRef {
    const NOUN: &'static str = "edge";

    fn order(&self, other: &Self) -> Ordering {
        EdgeRef::order(self, other)
    }

    fn body(&self) -> BodyId {
        self.body
    }

    fn body_mut(&mut self) -> &mut BodyId {
        &mut self.body
    }

    fn near(&self) -> DVec3 {
        self.near
    }

    fn target(picked: Picked) -> Option<u32> {
        match picked {
            Picked::Edge(edge) => Some(edge),
            _ => None,
        }
    }

    fn picked(target: u32) -> Picked {
        Picked::Edge(target)
    }

    fn find(&self, index: &PickIndex, body: BodyId) -> Option<u32> {
        index.find_edge(body, self.faces, self.near)
    }

    fn named(naming: &Naming, index: &PickIndex, target: u32, at: DVec3) -> Result<Self, Unnamed> {
        naming.edge_ref(index, target, at)
    }

    fn selected(item: &Selected) -> Option<Self> {
        match *item {
            Selected::Edge { body, faces, near } => Some(EdgeRef { body, faces, near }),
            _ => None,
        }
    }

    fn only(kind: MotionKind) -> String {
        if kind == MotionKind::Sweep {
            return "Only an edge or a sketch's curve can be on the path".to_owned();
        }
        format!("Only an edge can be {}ed", noun(kind))
    }

    fn limit(_: MotionKind) -> usize {
        varde_document::MAX_BLEND_EDGES
    }

    fn refs(session: &MotionSession) -> &Refs<Self> {
        &session.blend.edges
    }

    fn refs_mut(session: &mut MotionSession) -> &mut Refs<Self> {
        &mut session.blend.edges
    }

    fn grown(_: &Doc, session: &MotionSession, index: &PickIndex, target: u32) -> Vec<u32> {
        super::blend::chain_of(index, target, session.blend.chains)
    }
}

impl Ref for FaceRef {
    const NOUN: &'static str = "face";

    fn order(&self, other: &Self) -> Ordering {
        FaceRef::order(self, other)
    }

    fn body(&self) -> BodyId {
        self.body
    }

    fn body_mut(&mut self) -> &mut BodyId {
        &mut self.body
    }

    fn near(&self) -> DVec3 {
        self.near
    }

    fn target(picked: Picked) -> Option<u32> {
        match picked {
            Picked::Face(face) => Some(face),
            _ => None,
        }
    }

    fn picked(target: u32) -> Picked {
        Picked::Face(target)
    }

    fn find(&self, index: &PickIndex, body: BodyId) -> Option<u32> {
        index.find_face(body, &self.key, self.near)
    }

    fn named(naming: &Naming, index: &PickIndex, target: u32, at: DVec3) -> Result<Self, Unnamed> {
        naming.checked_face_ref(index, target, at)
    }

    fn selected(item: &Selected) -> Option<Self> {
        match *item {
            Selected::Face { body, key, near } => Some(FaceRef { body, key, near }),
            _ => None,
        }
    }

    fn only(kind: MotionKind) -> String {
        format!("Only a face can be {}", super::faces::verb(kind))
    }

    fn limit(kind: MotionKind) -> usize {
        super::faces::limit(kind)
    }

    fn refs(session: &MotionSession) -> &Refs<Self> {
        &session.faces
    }

    fn refs_mut(session: &mut MotionSession) -> &mut Refs<Self> {
        &mut session.faces
    }

    /// On a shell's own preview, what's left of a face it removes may be
    /// in several pieces (its top, opened with two opposite sides, is
    /// two strips), each keyed as the face: they light, and a click on
    /// any takes the face out, as one. (A face of the body already in
    /// pieces before the shell, one of them removed, lights with the
    /// others there too.)
    fn grown(doc: &Doc, session: &MotionSession, index: &PickIndex, target: u32) -> Vec<u32> {
        if session.kind == MotionKind::Shell && doc.feed.shows_draft_of_run() {
            index.faces_keyed_as(target)
        } else {
            vec![target]
        }
    }
}
