use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use varde_expr::LengthUnit;
use varde_kernel::Tolerance;
use varde_sketch::{LinkKind, Sketch};

use crate::{
    Body, BodyId, CheckError, Copies, Document, EditError, FeatureId, FeatureKind, Id, LinkSource,
    MAX_PATTERN_BODIES, Move, Opacity, Operation, OutsideRef, Pattern, Plane, Removable, Snapshot,
    Turn, sketch_face,
};

/// An edit to a [`Document`]. [`Editor::apply`] refuses one that would
/// leave the document failing [`Document::check`].
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Removes a body with the feature that makes it, and so with all
    /// [`Document::removal`] lists for it.
    RemoveBody(BodyId),
    SetVisible(BodyId, bool),
    SetOpacity(BodyId, Opacity),
    /// Adds a feature holding an empty sketch on `plane`, an origin plane
    /// or a face of a body ([`Plane::Face`]).
    AddSketch {
        name: String,
        plane: Plane,
    },
    /// Replaces a sketch feature's sketch whole: how every edit inside a
    /// sketch is committed, as one undoable change. The sources of links
    /// it no longer has go with them; a link it has that the feature has
    /// no source for is refused (a new one comes with
    /// [`Command::AddLink`]).
    SetSketch {
        feature: FeatureId,
        sketch: Box<Sketch>,
    },
    /// Replaces a sketch feature's sketch whole, as
    /// [`Command::SetSketch`] does, giving the link `source.link` of the
    /// new sketch, one the feature has no source for, what it comes
    /// from: how a link the Project or Intersect tool adds is committed.
    AddLink {
        feature: FeatureId,
        sketch: Box<Sketch>,
        source: LinkSource,
    },
    /// Puts a sketch feature on another plane, an origin plane or a face,
    /// keeping its drawing as it is in its own coordinates. Not a sketch,
    /// or the plane it's on already, changes nothing.
    SetSketchPlane {
        feature: FeatureId,
        plane: Plane,
    },
    /// Adds a feature of any kind but a sketch's ([`Command::AddSketch`]
    /// adds those), hiding the sketches whose regions it takes
    /// ([`FeatureKind::profile_sketches`]: a loft's sections' sketches). One making a new body adds the body,
    /// "Body N" one past the bodies so named, and gives it its id whatever
    /// [`Operation::NewBody`] held ([`BodyId::NEW`]); a split keeping
    /// both sides makes one too, whatever its `new_body` held, and one
    /// keeping a side makes none ([`Split::new_body`]); nothing added
    /// holds an id ([`FeatureKind::held_body`]). A pattern whose
    /// copies are bodies of their own ([`Copies::Separate`]) adds them,
    /// one per copy, named so in turn, whatever its list held. A
    /// revolve's axis must be a line of its sketch
    /// ([`Revolve::check_axis`]), a split's line's curves curves of its
    /// sketch ([`Split::check_curves`]), a sweep's path's curves curves
    /// of theirs ([`Sweep::check_curves`]), and a loft's start points,
    /// points and rail curves those of theirs ([`Loft::check_names`]).
    ///
    /// [`Loft::check_names`]: crate::Loft::check_names
    /// [`Revolve::check_axis`]: crate::Revolve::check_axis
    /// [`Operation::NewBody`]: crate::Operation::NewBody
    /// [`Split::new_body`]: crate::Split::new_body
    /// [`Split::check_curves`]: crate::Split::check_curves
    /// [`Sweep::check_curves`]: crate::Sweep::check_curves
    AddFeature {
        name: String,
        kind: Box<FeatureKind>,
    },
    /// Replaces a feature's kind, as edited, keeping its id, name and
    /// visibility. Neither may be a sketch's: [`Command::SetSketch`] sets
    /// those. The caller passes the regions as
    /// references made again from the sketch as it is now
    /// (`varde_sketch::Profiles::reference`), not the old ones. One that
    /// made a new body and still does keeps the body; one that stops
    /// making it removes it, dropping it from the other features'
    /// excluded lists (but refused while a combine names it, as a target
    /// or a tool: removing it would leave the combine naming a body that
    /// isn't there), holding its id ([`FeatureKind::held_body`]); one
    /// that starts making one gets back the body whose id it held, else
    /// adds one as [`Command::AddFeature`] does. The id is filled in
    /// whatever the command held. A split's new body goes the same
    /// way, as its `keep` has both sides or one. A pattern's copy bodies go the same
    /// way: each copy (by its original and its `k`) the feature made a
    /// body of keeps it, the others get new ones, and those it no longer
    /// makes (fewer copies, a body taken out, joined to the original
    /// again, another kind) are removed, refused while a later feature
    /// names one. A revolve's axis must be a line of its sketch, as for
    /// [`Command::AddFeature`].
    SetFeature {
        feature: FeatureId,
        kind: Box<FeatureKind>,
    },
    /// Removes a feature with every later feature that uses it, directly
    /// or through others, and the bodies they all make, dropping those
    /// bodies from the other features' excluded lists: what
    /// [`Document::removal`] lists.
    RemoveFeature(FeatureId),
    SetFeatureVisible(FeatureId, bool),
    /// Changes the design's units. Every dimension's expression first has
    /// the old units written in after its bare numbers
    /// ([`Sketch::pin_units`]), so it means what it did, and no value or
    /// geometry changes.
    SetUnits(LengthUnit),
    /// Changes the design's tolerance, see [`Document::tolerance`].
    SetTolerance(Tolerance),
    /// Replaces the whole document, e.g. with unsaved changes recovered
    /// after a crash.
    Replace(Box<Document>),
}

impl Document {
    /// The command adding a new sketch on `plane`, named one past the
    /// highest "Sketch N" in the document, so that the numbering holds
    /// across sessions and undo.
    pub fn add_sketch(&self, plane: Plane) -> Command {
        let names = self.features.iter().map(|feature| feature.name.as_str());
        Command::AddSketch {
            name: format!("Sketch {}", next_number(names, "Sketch")),
            plane,
        }
    }

    /// The command giving the sketch feature `feature` `sketch` whole:
    /// [`Command::SetSketch`], or, as `sketch` lacks the sketch face
    /// ([`sketch_face`]) the feature's plane has it hold, [`Command::AddLink`]
    /// with one added to it (last, making nothing until relinked), so
    /// drawing a sketch afresh keeps it.
    pub fn set_sketch_whole(&self, feature: FeatureId, mut sketch: Sketch) -> Command {
        let face = (self.feature(feature)).and_then(|found| match &found.kind {
            FeatureKind::Sketch { plane, .. } => plane.face().copied(),
            _ => None,
        });
        let held = (self.feature_index(feature)).and_then(|index| self.sketch_face_of(index));
        let lacks = held
            .is_none_or(|id| (sketch.link(id)).is_none_or(|link| link.kind != LinkKind::Project));
        if let Some(face) = face.filter(|_| lacks)
            && let Ok(link) = sketch.add_link(LinkKind::Project)
        {
            return Command::AddLink {
                feature,
                sketch: Box::new(sketch),
                source: LinkSource {
                    link,
                    source: OutsideRef::Face(face),
                },
            };
        }
        Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        }
    }

    /// The command adding a feature of `kind`, named one past the highest
    /// of its kind's names in the document ("Extrude N", "Revolve N"),
    /// like [`Document::add_sketch`].
    pub fn add_feature(&self, kind: FeatureKind) -> Command {
        let names = self.features.iter().map(|feature| feature.name.as_str());
        let noun = kind.noun();
        Command::AddFeature {
            name: format!("{noun} {}", next_number(names, noun)),
            kind: Box::new(kind),
        }
    }

    /// A copy with body `id` as `change` leaves it, or `None` if there's
    /// no such body or `change` leaves it as it was.
    fn with_body(&self, id: BodyId, change: impl FnOnce(&mut Body)) -> Option<Document> {
        let index = self.body_index(id)?;
        let mut body = self.bodies[index].clone();
        change(&mut body);
        (body != self.bodies[index]).then(|| {
            let mut next = Document::clone(self);
            next.bodies[index] = body;
            next
        })
    }

    /// Gives the sketch feature at `index` `sketch`, keeping the sources
    /// of the links it still has, and adding `source` for its link, in
    /// its place by link (one there for the link already is replaced).
    /// The check is the caller's.
    fn set_sketch(&mut self, index: usize, sketch: Sketch, source: Option<LinkSource>) {
        if let FeatureKind::Sketch {
            sketch: old,
            sources,
            ..
        } = &mut self.features[index].kind
        {
            sources.retain(|kept| sketch.link(kept.link).is_some());
            if let Some(source) = source {
                sources.retain(|kept| kept.link != source.link);
                let at = sources.partition_point(|kept| kept.link < source.link);
                sources.insert(at, source);
            }
            *old = sketch;
        }
    }

    /// The sketch face ([`sketch_face`]) of feature `index`, if it's a
    /// sketch on a face with one.
    pub(crate) fn sketch_face_of(&self, index: usize) -> Option<Id> {
        match &self.features.get(index)?.kind {
            FeatureKind::Sketch {
                plane,
                sketch,
                sources,
            } => sketch_face(plane, sketch, sources),
            _ => None,
        }
    }

    /// Refuses a sketch set at `index` that leaves it without the sketch
    /// face it had in `before`, as nothing removes it but leaving the face.
    fn keeps_sketch_face(&self, before: &Document, index: usize) -> Result<(), EditError> {
        match before.sketch_face_of(index) {
            Some(_) if self.sketch_face_of(index).is_none() => {
                Err(EditError::SketchFace(self.features[index].id))
            }
            _ => Ok(()),
        }
    }

    /// Gives the sketch feature at `index` its sketch face
    /// ([`sketch_face`]) as its plane has it, `was` being the link that
    /// was its sketch face before its plane changed: on a face, the one
    /// there is kept, else `was` follows the new face, else a new link is
    /// added (making nothing until it's relinked: construction, out of
    /// profiles); a sketch face that's no longer one, as the sketch left
    /// its face or a link of the new face was there already, is deleted.
    /// The check is the caller's.
    fn follow_sketch_face(&mut self, index: usize, was: Option<Id>) -> Result<(), EditError> {
        let feature = self.features[index].id;
        let FeatureKind::Sketch {
            plane,
            sketch,
            sources,
        } = &mut self.features[index].kind
        else {
            return Ok(());
        };
        let now = sketch_face(plane, sketch, sources);
        let drop = |sketch: &mut Sketch, sources: &mut Vec<LinkSource>, link: Id| {
            sketch.delete(&[link]);
            sources.retain(|from| from.link != link);
        };
        match (plane.face().copied(), now, was) {
            (Some(_), Some(now), Some(was)) if now != was => drop(sketch, sources, was),
            (Some(_), Some(_), _) | (None, _, None) => {}
            (Some(face), None, Some(was)) => {
                if let Some(from) = sources.iter_mut().find(|from| from.link == was) {
                    from.source = OutsideRef::Face(face);
                }
            }
            (Some(face), None, None) => {
                let link = (sketch.add_link(LinkKind::Project))
                    .map_err(|why| EditError::Sketch(feature, why.into()))?;
                sources.push(LinkSource {
                    link,
                    source: OutsideRef::Face(face),
                });
            }
            (None, _, Some(was)) => drop(sketch, sources, was),
        }
        Ok(())
    }

    /// The document with every sketch on a face given its sketch face
    /// ([`sketch_face`]) where it lacks one, as a document read is: one
    /// whose sketch has no id left stays without. Passes the check as it did.
    pub(crate) fn with_sketch_faces(mut self) -> Document {
        for index in 0..self.features.len() {
            // With no link before, the only change is the link added,
            // which leaves the sketch as it was when it fails.
            let _ = self.follow_sketch_face(index, None);
        }
        debug_assert_eq!(self.check(), Ok(()));
        self
    }

    /// Adds a visible, opaque body made by `feature` with a new id, "Body
    /// N" one past the bodies so named, see [`Document::add_sketch`]. New
    /// ids are the highest, so it goes last.
    fn add_body(&mut self, feature: FeatureId) -> Result<BodyId, EditError> {
        let names = self.bodies.iter().map(|body| body.name.as_str());
        let number = next_number(names, "Body");
        self.add_numbered_body(feature, number)
    }

    /// Adds a body made by `feature` with the id `id` it held (see
    /// [`FeatureKind::held_body`]), in its place in id order, named as
    /// [`Document::add_body`] names a new one. The id is one the
    /// document's check held for it: below the next id and no body's.
    fn restore_body(&mut self, feature: FeatureId, id: BodyId) {
        let names = self.bodies.iter().map(|body| body.name.as_str());
        let name = format!("Body {}", next_number(names, "Body"));
        let at = self.bodies.partition_point(|body| body.id < id);
        self.bodies.insert(
            at,
            Body {
                id,
                name,
                visible: true,
                opacity: Opacity::default(),
                created_by: feature,
            },
        );
    }

    /// Adds a body as [`Document::add_body`] does, named "Body `number`".
    fn add_numbered_body(&mut self, feature: FeatureId, number: u64) -> Result<BodyId, EditError> {
        let name = format!("Body {number}");
        let id = BodyId(self.new_id()?);
        self.bodies.push(Body {
            id,
            name,
            visible: true,
            opacity: Opacity::default(),
            created_by: feature,
        });
        Ok(id)
    }
}

/// The copy bodies `pattern` lists once added or set in place of `old`
/// (see [`Copies::Separate`]): in its order, the body `old`, if it's a
/// pattern, made of the same copy (its original and `k`), or `None` for
/// one to add. `None` whole for a pattern joined to its originals, or
/// with a count or a number of bodies the document refuses, whose list
/// is then left as given, for the check to refuse.
fn planned_copies(old: Option<&FeatureKind>, pattern: &Pattern) -> Option<Vec<Option<BodyId>>> {
    if pattern.joins() {
        return None;
    }
    let count = (pattern.separate_count()).filter(|&count| count <= MAX_PATTERN_BODIES)?;
    let made: BTreeMap<(BodyId, u32), BodyId> = match old {
        Some(FeatureKind::Pattern(old)) => (old.copy_bodies())
            .map(|(source, k, body)| ((source, k), body))
            .collect(),
        _ => BTreeMap::new(),
    };
    let n = pattern.bodies.len();
    if n == 0 {
        return None;
    }
    (0..count)
        .map(|at| {
            let k = u32::try_from(at / n).ok()?.checked_add(1)?;
            Some(made.get(&(pattern.bodies[at % n], k)).copied())
        })
        .collect::<Option<Vec<_>>>()
}

/// Fills in the body `kind` makes or holds, as [`Command::AddFeature`]
/// (with no `old`) and [`Command::SetFeature`] (in place of `old`) take
/// it, whatever the command held: the body `old` makes or holds
/// ([`FeatureKind::held_body`]), if any, else [`BodyId::NEW`] for a body
/// it makes, for the command to give a new id; a panel needn't keep them
/// in step. A split's new body and an operation's (a new body's, or a
/// join's, cut's or intersect's held id, [`Targets::held`]); other kinds
/// hold none, and drop what `old` held.
///
/// [`Targets::held`]: crate::Targets::held
fn planned_new_body(old: Option<&FeatureKind>, kind: &mut FeatureKind) {
    let held = old.and_then(|old| old.new_body().or(old.held_body()));
    if let FeatureKind::Split(split) = kind {
        split.new_body = held.or(split.keeps_both().then_some(BodyId::NEW));
        return;
    }
    match kind.operation_mut() {
        Some(Operation::NewBody(body)) => *body = held.unwrap_or(BodyId::NEW),
        Some(
            Operation::Join(targets) | Operation::Cut(targets) | Operation::Intersect(targets),
        ) => {
            targets.held = held;
        }
        None => {}
    }
}

/// The bodies `old`, a pattern, makes of its copies ([`Copies::Separate`]).
fn copy_bodies_of(old: &FeatureKind) -> Vec<BodyId> {
    match old {
        FeatureKind::Pattern(Pattern {
            copies: Copies::Separate(made),
            ..
        }) => made.clone(),
        _ => Vec::new(),
    }
}

/// One past the highest `N` of the `names` that are "`kind` N", or 1. A
/// file could hold the highest number there is, which is then reused: a
/// repeated name is harmless.
fn next_number<'a>(names: impl Iterator<Item = &'a str>, kind: &str) -> u64 {
    names
        .filter_map(|name| {
            name.strip_prefix(kind)?
                .strip_prefix(' ')?
                .parse::<u64>()
                .ok()
        })
        .max()
        .map_or(1, |highest| highest.saturating_add(1))
}

impl Document {
    /// Has `feature` make `body` as its new body.
    fn set_new_body(&mut self, feature: FeatureId, body: BodyId) {
        if let Some(index) = self.feature_index(feature)
            && let Some(new) = self.features[index].kind.new_body_mut()
        {
            *new = body;
        }
    }

    /// The copy bodies of the pattern feature `id` that setting it to
    /// `kind` ([`Command::SetFeature`]) would remove, sorted: those it
    /// makes of copies `kind` doesn't make bodies of. For a panel to say
    /// which a later feature names, which refuses it.
    pub fn copies_dropped(&self, id: FeatureId, kind: &FeatureKind) -> Vec<BodyId> {
        let Some(old) = self.feature(id).map(|feature| &feature.kind) else {
            return Vec::new();
        };
        let kept: Vec<BodyId> = match kind {
            FeatureKind::Pattern(pattern) => (planned_copies(Some(old), pattern).into_iter())
                .flatten()
                .flatten()
                .collect(),
            _ => Vec::new(),
        };
        let mut dropped: Vec<BodyId> = (copy_bodies_of(old).into_iter())
            .filter(|body| !kept.contains(body))
            .collect();
        dropped.sort_unstable();
        dropped
    }

    /// Gives the pattern feature `id` the copy bodies `planned`
    /// ([`planned_copies`]): a new body for each `None`, numbered on
    /// from the bodies named "Body N".
    fn make_copies(
        &mut self,
        id: FeatureId,
        planned: Vec<Option<BodyId>>,
    ) -> Result<(), EditError> {
        let names = self.bodies.iter().map(|body| body.name.as_str());
        let mut number = next_number(names, "Body");
        let mut made = Vec::with_capacity(planned.len());
        for body in planned {
            made.push(match body {
                Some(body) => body,
                None => {
                    let body = self.add_numbered_body(id, number)?;
                    number = number.saturating_add(1);
                    body
                }
            });
        }
        if let Some(index) = self.feature_index(id)
            && let FeatureKind::Pattern(pattern) = &mut self.features[index].kind
        {
            pattern.copies = Copies::Separate(made);
        }
        Ok(())
    }

    /// Removes `bodies`, sorted, dropping them from the other features'
    /// excluded lists.
    fn remove_bodies(&mut self, bodies: &[BodyId]) {
        if bodies.is_empty() {
            return;
        }
        self.bodies
            .retain(|body| bodies.binary_search(&body.id).is_err());
        self.drop_excluded(bodies);
    }

    /// Checks what [`Command::AddFeature`] and [`Command::SetFeature`]
    /// require of `kind`, feature `index` of this document, beyond
    /// [`Document::check`]: a revolve's axis is a line of its sketch, a
    /// split's line's and a sweep's path's curves are curves of their
    /// sketches, and a loft's points and rail curves are of its sketches.
    fn check_new(&self, index: usize, kind: &FeatureKind) -> Result<(), EditError> {
        if let FeatureKind::Sweep(sweep) = kind {
            let id = self.features[index].id;
            sweep
                .check_curves(|sketch| self.sketch_before(index, sketch))
                .map_err(|why| EditError::Invalid(CheckError::Sweep(id, why)))?;
        }
        if let FeatureKind::Loft(loft) = kind {
            let id = self.features[index].id;
            loft.check_names(|sketch| self.sketch_before(index, sketch))
                .map_err(|why| EditError::Invalid(CheckError::Loft(id, why)))?;
        }
        if let FeatureKind::Split(split) = kind
            && let Some(sketch) = (split.tool.sketch()).and_then(|id| self.sketch_before(index, id))
        {
            let id = self.features[index].id;
            split
                .check_curves(sketch)
                .map_err(|why| EditError::Invalid(CheckError::Split(id, why)))?;
        }
        if let FeatureKind::Revolve(revolve) = kind
            && let Some(sketch) = self.sketch_before(index, revolve.sketch)
        {
            let id = self.features[index].id;
            revolve
                .check_axis(sketch)
                .map_err(|why| EditError::Invalid(CheckError::Revolve(id, why)))?;
        }
        Ok(())
    }
}

/// Names a state of an [`Editor`]'s document, see [`Editor::revision`].
/// Only equality means anything: a newer state may have an older
/// revision, so there's no ordering them. Serializable as its number, for
/// the IO lane's requests, which answer with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Revision(u64);

/// An editor's own come from [`Editor::revision`]; this is for tests,
/// which name one by its number.
impl From<u64> for Revision {
    fn from(number: u64) -> Self {
        Self(number)
    }
}

impl From<Revision> for u64 {
    fn from(revision: Revision) -> Self {
        revision.0
    }
}

/// Orders the changes of an [`Editor`]'s document, see
/// [`Editor::generation`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Generation(u64);

/// An editor's own come from [`Editor::generation`]; this is for tests,
/// which name one by its number.
impl From<u64> for Generation {
    fn from(number: u64) -> Self {
        Self(number)
    }
}

impl From<Generation> for u64 {
    fn from(generation: Generation) -> Self {
        generation.0
    }
}

/// How many edits can be undone. Older ones are forgotten, so history holds
/// at most this many copies of the document.
const MAX_UNDO: usize = 200;

/// Owns a [`Document`] and its edit history.
///
/// History is snapshot based for simplicity, and capped at `MAX_UNDO`
/// steps. Once documents get large this should switch to storing inverse
/// commands or a persistent data structure.
///
/// The document and its history are behind `Arc`s, so a [`snapshot`] is a
/// refcount bump and undo/redo only swap pointers. An edit clones the
/// document once, and one that would change nothing isn't cloned at all.
///
/// [`snapshot`]: Editor::snapshot
#[derive(Debug, Clone)]
pub struct Editor {
    current: State,
    undo: VecDeque<State>,
    redo: Vec<State>,
    /// The revision the next edit gets.
    next_revision: Revision,
    generation: Generation,
}

/// A document in an editor's history, with the revision naming it, which
/// stays with it through undo and redo, and its lineage, see
/// [`Editor::lineage`].
#[derive(Debug, Clone)]
struct State {
    document: Arc<Document>,
    revision: Revision,
    lineage: Revision,
}

impl Editor {
    /// Starts editing `document`, which passes [`Document::check`] as
    /// every [`Document`] does (see there).
    pub fn new(document: Document) -> Self {
        debug_assert_eq!(document.check(), Ok(()));
        Self {
            current: State {
                document: Arc::new(document),
                revision: Revision(0),
                lineage: Revision(0),
            },
            undo: VecDeque::new(),
            redo: Vec::new(),
            next_revision: Revision(1),
            generation: Generation(0),
        }
    }

    pub fn document(&self) -> &Document {
        &self.current.document
    }

    /// The current document, shared rather than copied. Later edits don't
    /// change it.
    pub fn snapshot(&self) -> Snapshot {
        self.current.document.clone()
    }

    /// Names the document's state, for asking whether it's one seen before,
    /// e.g. the one saved. Each edit makes a new one, and undo and redo
    /// give a state back its own.
    pub fn revision(&self) -> Revision {
        self.current.revision
    }

    /// Names the line of edits the document comes from: the revision of
    /// the document it began with, the first or one that replaced it
    /// whole ([`Command::Replace`]). It changes only when an edit, undo or
    /// redo crosses such a replacement, when ids in the document, which
    /// otherwise name the same things from one state to the next, may name
    /// other things than before.
    pub fn lineage(&self) -> Revision {
        self.current.lineage
    }

    /// Grows on every change, undo and redo included: for ordering work
    /// about the document, such as regenerating it, where the newest wins.
    pub fn generation(&self) -> Generation {
        self.generation
    }

    /// Records a change: a new generation.
    fn changed(&mut self) {
        self.generation = Generation(self.generation.0 + 1);
    }

    /// Applies `command` as one undoable change. Refuses one that runs out
    /// of ids or would leave the document failing [`Document::check`],
    /// leaving the document, its history, revision and generation as they
    /// were. One that would change nothing leaves them as they were too.
    pub fn apply(&mut self, command: Command) -> Result<(), EditError> {
        self.change(command, true)
    }

    /// Applies `command` as [`Editor::apply`] does, but folded into the
    /// change before it rather than as one of its own: undo takes both
    /// back at once, and the redo history stays. For what follows from a
    /// change rather than being one: a sketch's links found again on the
    /// model the change made. The document gets a new revision and
    /// generation as with any change. Refused as [`Editor::apply`]
    /// refuses, leaving everything as it was.
    pub fn amend(&mut self, command: Command) -> Result<(), EditError> {
        self.change(command, false)
    }

    /// [`Editor::apply`], or, unless `own_step`, [`Editor::amend`].
    fn change(&mut self, command: Command, own_step: bool) -> Result<(), EditError> {
        // Each edit works on a private copy, so a refused one leaves `self`
        // as it was, and one that would change nothing returns before
        // copying.
        let document = &self.current.document;
        let replacing = matches!(command, Command::Replace(_));
        let next = match command {
            Command::RemoveBody(id) => {
                let removal = document.removal(Removable::Body(id));
                if removal.is_empty() {
                    return Ok(());
                }
                let mut next = Document::clone(document);
                next.remove(&removal);
                next
            }
            Command::SetVisible(id, visible) => {
                let Some(next) = document.with_body(id, |body| body.visible = visible) else {
                    return Ok(());
                };
                next
            }
            Command::SetOpacity(id, opacity) => {
                let Some(next) = document.with_body(id, |body| body.opacity = opacity) else {
                    return Ok(());
                };
                next
            }
            Command::AddSketch { name, plane } => {
                let mut next = Document::clone(document);
                let sketch = Sketch::default();
                next.push_feature(
                    name,
                    FeatureKind::Sketch {
                        plane,
                        sketch,
                        sources: Vec::new(),
                    },
                )?;
                next.follow_sketch_face(next.features.len() - 1, None)?;
                next
            }
            Command::SetSketch { feature, sketch } => {
                let Some(index) = document.feature_index(feature) else {
                    return Ok(());
                };
                match &document.features[index].kind {
                    FeatureKind::Sketch { sketch: old, .. } if *old != *sketch => {}
                    _ => return Ok(()),
                }
                let mut next = Document::clone(document);
                next.set_sketch(index, *sketch, None);
                next.keeps_sketch_face(document, index)?;
                next
            }
            Command::AddLink {
                feature,
                sketch,
                source,
            } => {
                let Some(index) = document.feature_index(feature) else {
                    return Ok(());
                };
                if !matches!(document.features[index].kind, FeatureKind::Sketch { .. }) {
                    return Ok(());
                }
                let mut next = Document::clone(document);
                next.set_sketch(index, *sketch, Some(source));
                next.keeps_sketch_face(document, index)?;
                next
            }
            Command::SetSketchPlane { feature, plane } => {
                let Some(index) = document.feature_index(feature) else {
                    return Ok(());
                };
                match &document.features[index].kind {
                    FeatureKind::Sketch { plane: old, .. } if *old != plane => {}
                    _ => return Ok(()),
                }
                let mut next = Document::clone(document);
                let was = next.sketch_face_of(index);
                if let FeatureKind::Sketch { plane: old, .. } = &mut next.features[index].kind {
                    *old = plane;
                }
                next.follow_sketch_face(index, was)?;
                next
            }
            Command::AddFeature { name, kind } => {
                if matches!(*kind, FeatureKind::Sketch { .. }) {
                    return Err(EditError::SketchKind);
                }
                let mut kind = kind;
                planned_new_body(None, &mut kind);
                let mut next = Document::clone(document);
                let sketches = kind.profile_sketches();
                let makes_body = kind.new_body().is_some();
                let copies = match &*kind {
                    FeatureKind::Pattern(pattern) => planned_copies(None, pattern),
                    _ => None,
                };
                let id = next.push_feature(name, *kind)?;
                if makes_body {
                    let body = next.add_body(id)?;
                    next.set_new_body(id, body);
                }
                if let Some(planned) = copies {
                    next.make_copies(id, planned)?;
                }
                for sketch in sketches {
                    if let Some(index) = next.feature_index(sketch) {
                        next.features[index].visible = false;
                    }
                }
                let index = next.features.len() - 1;
                next.check_new(index, &next.features[index].kind)?;
                next
            }
            Command::SetFeature { feature, mut kind } => {
                let Some((index, old)) = document
                    .feature_index(feature)
                    .map(|index| (index, &document.features[index].kind))
                else {
                    return Ok(());
                };
                if matches!(old, FeatureKind::Sketch { .. })
                    || matches!(*kind, FeatureKind::Sketch { .. })
                {
                    return Err(EditError::SketchKind);
                }
                planned_new_body(Some(old), &mut kind);
                let kept = old.new_body();
                let copies = match &mut *kind {
                    FeatureKind::Pattern(pattern) => {
                        let planned = planned_copies(Some(old), pattern);
                        // Every copy kept: the list as it'll be, for
                        // telling an edit that changes nothing.
                        if let Some(made) = (planned.as_ref())
                            .and_then(|planned| planned.iter().copied().collect::<Option<Vec<_>>>())
                        {
                            pattern.copies = Copies::Separate(made);
                        }
                        planned
                    }
                    _ => None,
                };
                if *old == *kind {
                    return Ok(());
                }
                let mut next = Document::clone(document);
                let makes_body = kind.new_body().is_some();
                let dropped = document.copies_dropped(feature, &kind);
                next.features[index].kind = *kind;
                next.remove_bodies(&dropped);
                if let Some(planned) = copies {
                    next.make_copies(feature, planned)?;
                }
                match (kept, makes_body) {
                    (Some(body), false) => {
                        if let Some(at) = next.body_index(body) {
                            next.bodies.remove(at);
                        }
                        next.drop_excluded(&[body]);
                    }
                    (None, true) => match next.features[index].kind.new_body() {
                        // Made again with the id it held.
                        Some(held) if held != BodyId::NEW => next.restore_body(feature, held),
                        _ => {
                            let body = next.add_body(feature)?;
                            next.set_new_body(feature, body);
                        }
                    },
                    _ => {}
                }
                next.check_new(index, &next.features[index].kind)?;
                next
            }
            Command::RemoveFeature(id) => {
                let removal = document.removal(Removable::Feature(id));
                if removal.is_empty() {
                    return Ok(());
                }
                let mut next = Document::clone(document);
                next.remove(&removal);
                next
            }
            Command::SetFeatureVisible(id, visible) => {
                let Some(index) = document
                    .feature_index(id)
                    .filter(|&index| document.features[index].visible != visible)
                else {
                    return Ok(());
                };
                let mut next = Document::clone(document);
                next.features[index].visible = visible;
                next
            }
            Command::SetUnits(units) => {
                if units == document.units {
                    return Ok(());
                }
                let mut next = Document::clone(document);
                let before = document.design();
                let angle = Turn::ask(&before);
                let offset = Move::offset_ask(&before);
                let angle_ask = Move::angle_ask(&before);
                for feature in &mut next.features {
                    match &mut feature.kind {
                        FeatureKind::Sketch { sketch, .. } => sketch.pin_units(&before),
                        FeatureKind::Extrude(extrude) => {
                            for (value, ask) in extrude.values_mut(&before) {
                                value.pin_units(&ask);
                            }
                        }
                        FeatureKind::Revolve(revolve) => {
                            for value in revolve.extent.values_mut() {
                                value.pin_units(&angle);
                            }
                        }
                        FeatureKind::Move(moved) => {
                            let (offsets, angle) = moved.values_mut();
                            for value in offsets {
                                value.pin_units(&offset);
                            }
                            if let Some(value) = angle {
                                value.pin_units(&angle_ask);
                            }
                        }
                        FeatureKind::Pattern(pattern) => {
                            for (value, ask) in pattern.values_mut(&before) {
                                value.pin_units(&ask);
                            }
                        }
                        FeatureKind::Align(align) => {
                            if let Some(value) = &mut align.offset {
                                value.pin_units(&offset);
                            }
                            if let Some(value) = &mut align.turn {
                                value.pin_units(&angle_ask);
                            }
                        }
                        // Factors have no unit, but are pinned as a
                        // pattern's count is: a bare number added to a
                        // length inside one took the units.
                        FeatureKind::Scale(scale) => {
                            for (value, ask) in scale.values_mut(&before) {
                                value.pin_units(&ask);
                            }
                        }
                        FeatureKind::Chamfer(chamfer) => {
                            for (value, ask) in chamfer.values_mut(&before) {
                                value.pin_units(&ask);
                            }
                        }
                        FeatureKind::Shell(shell) => {
                            for (value, ask) in shell.values_mut(&before) {
                                value.pin_units(&ask);
                            }
                        }
                        FeatureKind::Fillet(fillet) => {
                            for (value, ask) in fillet.values_mut(&before) {
                                value.pin_units(&ask);
                            }
                        }
                        FeatureKind::OffsetFace(offset) => {
                            for (value, ask) in offset.values_mut(&before) {
                                value.pin_units(&ask);
                            }
                        }
                        FeatureKind::FaceDraft(draft) => {
                            for (value, ask) in draft.values_mut(&before) {
                                value.pin_units(&ask);
                            }
                        }
                        // Turns have no unit, but are pinned as a
                        // scale's factors are.
                        FeatureKind::Sweep(sweep) => {
                            for (value, ask) in sweep.values_mut(&before) {
                                value.pin_units(&ask);
                            }
                        }
                        // No values.
                        FeatureKind::Combine(_)
                        | FeatureKind::Mirror(_)
                        | FeatureKind::Split(_)
                        | FeatureKind::Loft(_) => {}
                    }
                }
                next.units = units;
                next
            }
            Command::SetTolerance(tolerance) => {
                if tolerance == document.tolerance() {
                    return Ok(());
                }
                let mut next = Document::clone(document);
                next.tolerance = tolerance.fit();
                next
            }
            Command::Replace(replacement) => {
                if *replacement == **document {
                    return Ok(());
                }
                *replacement
            }
        };
        next.check().map_err(EditError::Invalid)?;

        let revision = self.next_revision;
        // One per edit: a u64 won't run out.
        self.next_revision = Revision(revision.0 + 1);
        let lineage = if replacing {
            revision
        } else {
            self.current.lineage
        };
        let before = std::mem::replace(
            &mut self.current,
            State {
                document: Arc::new(next),
                revision,
                lineage,
            },
        );
        // Amended, the state replaced is dropped: undo goes back past it.
        if own_step {
            self.push_undo(before);
            self.redo.clear();
        }
        self.changed();
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some(previous) = self.undo.pop_back() {
            self.redo
                .push(std::mem::replace(&mut self.current, previous));
            self.changed();
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            let current = std::mem::replace(&mut self.current, next);
            self.push_undo(current);
            self.changed();
        }
    }

    fn push_undo(&mut self, state: State) {
        self.undo.push_back(state);
        if self.undo.len() > MAX_UNDO {
            self.undo.pop_front();
        }
    }
}

#[cfg(test)]
mod tests;
