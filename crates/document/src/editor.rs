use std::{collections::VecDeque, sync::Arc};

use serde::{Deserialize, Serialize};
use varde_expr::LengthUnit;
use varde_kernel::Tolerance;
use varde_sketch::Sketch;

use crate::{
    Body, BodyId, CheckError, Document, EditError, Extent, FeatureId, FeatureKind, Move, Opacity,
    Operation, Plane, Removable, Snapshot, Turn,
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
    /// sketch is committed, as one undoable change.
    SetSketch {
        feature: FeatureId,
        sketch: Box<Sketch>,
    },
    /// Puts a sketch feature on another plane, an origin plane or a face,
    /// keeping its drawing as it is in its own coordinates. Not a sketch,
    /// or the plane it's on already, changes nothing.
    SetSketchPlane {
        feature: FeatureId,
        plane: Plane,
    },
    /// Adds a feature of any kind but a sketch's ([`Command::AddSketch`]
    /// adds those), hiding the sketch whose regions it takes
    /// ([`FeatureKind::sketch`]). One making a new body adds the body,
    /// "Body N" one past the bodies so named, and gives it its id whatever
    /// [`Operation::NewBody`] held ([`BodyId::NEW`]). A revolve's axis
    /// must be a line of its sketch ([`Revolve::check_axis`]).
    ///
    /// [`Revolve::check_axis`]: crate::Revolve::check_axis
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
    /// isn't there); one that starts making one adds it, as
    /// [`Command::AddFeature`] does. A revolve's axis must be a line of
    /// its sketch, as for [`Command::AddFeature`].
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

    /// Adds a visible, opaque body made by `feature` with a new id, "Body
    /// N" one past the bodies so named, see [`Document::add_sketch`]. New
    /// ids are the highest, so it goes last.
    fn add_body(&mut self, feature: FeatureId) -> Result<BodyId, EditError> {
        let names = self.bodies.iter().map(|body| body.name.as_str());
        let name = format!("Body {}", next_number(names, "Body"));
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
            && let Some(operation) = self.features[index].kind.operation_mut()
        {
            *operation = Operation::NewBody(body);
        }
    }

    /// Checks what [`Command::AddFeature`] and [`Command::SetFeature`]
    /// require of `kind`, feature `index` of this document, beyond
    /// [`Document::check`]: a revolve's axis is a line of its sketch.
    fn check_new(&self, index: usize, kind: &FeatureKind) -> Result<(), EditError> {
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
                next.push_feature(name, FeatureKind::Sketch { plane, sketch })?;
                next
            }
            Command::SetSketch { feature, sketch } => {
                let Some(index) = document
                    .feature_index(feature)
                    .filter(|&index| match &document.features[index].kind {
                        FeatureKind::Sketch { sketch: old, .. } => *old != *sketch,
                        _ => false,
                    })
                else {
                    return Ok(());
                };
                let mut next = Document::clone(document);
                if let FeatureKind::Sketch { sketch: old, .. } = &mut next.features[index].kind {
                    *old = *sketch;
                }
                next
            }
            Command::SetSketchPlane { feature, plane } => {
                let Some(index) = document
                    .feature_index(feature)
                    .filter(|&index| match &document.features[index].kind {
                        FeatureKind::Sketch { plane: old, .. } => *old != plane,
                        _ => false,
                    })
                else {
                    return Ok(());
                };
                let mut next = Document::clone(document);
                if let FeatureKind::Sketch { plane: old, .. } = &mut next.features[index].kind {
                    *old = plane;
                }
                next
            }
            Command::AddFeature { name, kind } => {
                if matches!(*kind, FeatureKind::Sketch { .. }) {
                    return Err(EditError::SketchKind);
                }
                let mut next = Document::clone(document);
                let sketch = kind.sketch();
                let makes_body = kind.new_body().is_some();
                let id = next.push_feature(name, *kind)?;
                if makes_body {
                    let body = next.add_body(id)?;
                    next.set_new_body(id, body);
                }
                if let Some(index) = sketch.and_then(|sketch| next.feature_index(sketch)) {
                    next.features[index].visible = false;
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
                let kept = old.new_body();
                if let (Some(body), Some(Operation::NewBody(new))) = (kept, kind.operation_mut()) {
                    *new = body;
                }
                if *old == *kind {
                    return Ok(());
                }
                let mut next = Document::clone(document);
                let makes_body = kind.new_body().is_some();
                next.features[index].kind = *kind;
                match (kept, makes_body) {
                    (Some(body), false) => {
                        if let Some(at) = next.body_index(body) {
                            next.bodies.remove(at);
                        }
                        next.drop_excluded(&[body]);
                    }
                    (None, true) => {
                        let body = next.add_body(feature)?;
                        next.set_new_body(feature, body);
                    }
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
                let length = Extent::ask(&before);
                let angle = Turn::ask(&before);
                let offset = Move::offset_ask(&before);
                let angle_ask = Move::angle_ask(&before);
                for feature in &mut next.features {
                    match &mut feature.kind {
                        FeatureKind::Sketch { sketch, .. } => sketch.pin_units(&before),
                        FeatureKind::Extrude(extrude) => {
                            for value in extrude.extent.values_mut() {
                                value.pin_units(&length);
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
                        // No values.
                        FeatureKind::Combine(_) | FeatureKind::Mirror(_) => {}
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
        self.push_undo(before);
        self.redo.clear();
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
