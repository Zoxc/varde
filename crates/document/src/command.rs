//! The edits to a [`Document`]: [`Command`], and [`Document::after`], the
//! document each makes, with the body and pattern planning they share.

use std::collections::BTreeMap;
use std::fmt;

use varde_expr::{LengthUnit, exact};
use varde_kernel_compat::Tolerance;
use varde_sketch::{Budget, LinkKind, Sketch, Unrevalued};

use crate::{
    Body, BodyId, CheckError, Copies, Document, FeatureId, FeatureKind, Id, LinkSource,
    MAX_PATTERN_BODIES, Named, Opacity, Operation, OutsideRef, Param, Pattern, Plane, Removable,
    Removal, Tint, param, sketch_face,
};

/// An edit to a [`Document`]. [`Editor::apply`](crate::Editor::apply) refuses one that would
/// leave the document failing [`Document::check`].
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Removes a body with the feature that makes it, and so with all
    /// [`Document::removal`] lists for it.
    RemoveBody(BodyId),
    SetVisible(BodyId, bool),
    SetOpacity(BodyId, Opacity),
    /// Gives a body a colour of its own, or with `None` the theme's back.
    SetColor(BodyId, Option<Tint>),
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
    /// excluded lists (a later feature naming it, as a combine's target
    /// or tool, keeps naming it, and fails), holding its id ([`FeatureKind::held_body`]); one
    /// that starts making one gets back the body whose id it held, else
    /// adds one as [`Command::AddFeature`] does. The id is filled in
    /// whatever the command held. A split's new body goes the same
    /// way, as its `keep` has both sides or one. A pattern's copy bodies go the same
    /// way: each copy (by its original and its `k`) the feature made a
    /// body of keeps it, the others get new ones, and those it no longer
    /// makes (fewer copies, a body taken out, joined to the original
    /// again, another kind) are removed, likewise. A revolve's axis must be a line of its sketch, as for
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
    /// Removes the features, with all [`Document::removal_of`] lists for
    /// them, as one edit.
    RemoveFeatures(Vec<FeatureId>),
    /// Removes the features and the bodies they make, keeping the later
    /// features that use them, which then name what isn't there: what
    /// [`Document::breaking_removal`] lists.
    RemoveOnly(Vec<FeatureId>),
    SetFeatureVisible(FeatureId, bool),
    /// Rolls the Timeline back to before the feature, or forward to its
    /// end with `None`, see [`Document::rollback`]. A feature that isn't
    /// there changes nothing.
    SetRollback(Option<FeatureId>),
    /// Gives a feature or a body `name`, as it is: [`Document::rename`]
    /// makes the command with a name no other has. The name it has
    /// already, or one that isn't there, changes nothing.
    Rename {
        target: Named,
        name: String,
    },
    /// Changes the design's units. Every dimension's, feature value's and
    /// parameter's expression first has the old units written in after
    /// its bare numbers ([`Sketch::pin_units`],
    /// [`Value::pin_units`](varde_expr::Value::pin_units),
    /// [`Params::pin_units`](varde_expr::Params::pin_units)), so it means
    /// what it did, and no value or geometry changes. One the units would
    /// take too long is replaced by its value ([`varde_expr::exact`]); a
    /// parameter's in error, which has none, stays as it is.
    SetUnits(LengthUnit),
    /// Adds a parameter, `name = text`, after the others. Its name must
    /// be one [`varde_expr::check_name`] takes, no other parameter's; its
    /// text may be in error, as nothing uses it yet.
    AddParam {
        name: String,
        text: String,
    },
    /// Sets the expression of the parameter at `index`, without the
    /// whitespace around it, and evaluates every feature value and sketch
    /// dimension naming a parameter again, the geometry following: a
    /// sketch whose driving dimensions change is solved again here, within
    /// the solver's iteration bound ([`varde_sketch::revalue`]), in the
    /// same undo step. Refused, as [`EditError::Value`], where a value
    /// that uses it, directly or through other parameters, would be in
    /// error, and as [`EditError::Unsolved`] where a sketch wouldn't
    /// solve. One that isn't there changes nothing.
    SetParam {
        index: usize,
        text: String,
    },
    /// Renames the parameter at `index`, rewriting every use of it in
    /// feature values, sketch dimensions and other parameters
    /// ([`varde_expr::rename`]).
    /// Refused as [`EditError::Value`] where that takes an expression
    /// over [`varde_expr::MAX_LEN`]. One that isn't there changes nothing.
    RenameParam {
        index: usize,
        name: String,
    },
    /// Removes the parameter at `index`: refused, as
    /// [`EditError::ParamUsed`], while a feature value, a sketch
    /// dimension or another parameter uses it. One that isn't there changes nothing.
    RemoveParam(usize),
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
        Command::AddSketch {
            name: format!("Sketch {}", self.next_number("Sketch")),
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
        let noun = kind.noun();
        Command::AddFeature {
            name: format!("{noun} {}", self.next_number(noun)),
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
        let number = self.next_number("Body");
        self.add_numbered_body(feature, number)
    }

    /// Adds a body made by `feature` with the id `id` it held (see
    /// [`FeatureKind::held_body`]), in its place in id order, named as
    /// [`Document::add_body`] names a new one. The id is one the
    /// document's check held for it: below the next id and no body's.
    fn restore_body(&mut self, feature: FeatureId, id: BodyId) {
        let body = new_body(id, self.next_number("Body"), feature);
        let at = self.bodies.partition_point(|body| body.id < id);
        self.bodies.insert(at, body);
    }

    /// Adds a body as [`Document::add_body`] does, named "Body `number`".
    fn add_numbered_body(&mut self, feature: FeatureId, number: u64) -> Result<BodyId, EditError> {
        let id = BodyId(self.new_id()?);
        self.bodies.push(new_body(id, number, feature));
        Ok(id)
    }
}

/// A new body: visible, opaque, uncoloured, "Body `number`", made by
/// `created_by`.
fn new_body(id: BodyId, number: u64, created_by: FeatureId) -> Body {
    Body {
        id,
        name: format!("Body {number}"),
        visible: true,
        opacity: Opacity::default(),
        color: None,
        created_by,
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

impl Document {
    /// One past the highest `N` of the feature and body names that are
    /// "`kind` N", or 1: both sorts, as a name is either's, so a default
    /// never takes one the other sort has ("Body 2" a feature's). A file
    /// could hold the highest number there is, which is then reused: a
    /// repeated name is harmless.
    fn next_number(&self, kind: &str) -> u64 {
        let features = self.features.iter().map(|feature| feature.name.as_str());
        let bodies = self.bodies.iter().map(|body| body.name.as_str());
        next_number(features.chain(bodies), kind)
    }
}

/// One past the highest `N` of the `names` that are "`kind` N", or 1.
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
        let mut number = self.next_number("Body");
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

    /// The document with feature `feature`'s kind set to `kind`, as
    /// [`Command::SetFeature`] does; `None` if that changes nothing.
    fn with_kind(
        &self,
        feature: FeatureId,
        mut kind: Box<FeatureKind>,
    ) -> Result<Option<Document>, EditError> {
        let Some((index, old)) = self
            .feature_index(feature)
            .map(|index| (index, &self.features[index].kind))
        else {
            return Ok(None);
        };
        if matches!(old, FeatureKind::Sketch { .. }) || matches!(*kind, FeatureKind::Sketch { .. })
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
            return Ok(None);
        }
        let mut next = self.clone();
        let makes_body = kind.new_body().is_some();
        let dropped = self.copies_dropped(feature, &kind);
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
        Ok(Some(next))
    }

    /// The document with `params` for its parameters, resolved, and every
    /// feature value and sketch dimension naming a parameter evaluated
    /// again, as the parameter commands do, each sketch whose dimensions
    /// change solved again ([`varde_sketch::revalue`]) within `budget`; with `renamed`, the old name and the new, each use of
    /// the old in feature values (and in `params`, which the caller
    /// renames) written as the new first. `None` if nothing changes.
    fn with_params(
        &self,
        params: Vec<Param>,
        renamed: Option<(&str, &str)>,
        budget: &Budget,
    ) -> Result<Option<Document>, EditError> {
        if params == self.params {
            return Ok(None);
        }
        param::check_params(&params).map_err(EditError::Invalid)?;
        let mut next = self.clone();
        next.resolved = param::resolve(&params, self.units);
        next.params = params;
        let design = next.design();
        let mut changed = Vec::new();
        let mut sketches = Vec::new();
        for (index, feature) in next.features.iter().enumerate() {
            if let FeatureKind::Sketch { sketch, .. } = &feature.kind {
                let error = |why| EditError::Value(ValueOf::Feature(feature.id), why);
                let mut renamed_sketch = None;
                if let Some((old, new)) = renamed {
                    let mut copy = sketch.clone();
                    for entry in &mut copy.dimensions {
                        let value = &mut entry.dimension.value;
                        value.text = varde_expr::rename(&value.text, old, new).map_err(error)?;
                    }
                    renamed_sketch = Some(copy);
                }
                let base = renamed_sketch.as_ref().unwrap_or(sketch);
                // Solved within the caller's bound, as a proposal is, but
                // here and now: the change is one undo step.
                let solved = match varde_sketch::revalue(base, &design, budget) {
                    Ok(solved) => solved.or(renamed_sketch),
                    Err(Unrevalued::Value(_, why)) => return Err(error(why)),
                    Err(Unrevalued::Rejected(why)) => {
                        return Err(EditError::Unsolved(feature.id, why));
                    }
                };
                if let Some(solved) = solved.filter(|solved| solved != sketch) {
                    sketches.push((index, solved));
                }
                continue;
            }
            let mut kind = feature.kind.clone();
            for (value, ask) in kind.values_mut(&design) {
                let error = |why| EditError::Value(ValueOf::Feature(feature.id), why);
                if let Some((old, new)) = renamed {
                    value.text = varde_expr::rename(&value.text, old, new).map_err(error)?;
                }
                if !varde_expr::names(&value.text).is_empty() {
                    value.value = varde_expr::evaluate(&value.text, &ask).map_err(error)?;
                }
            }
            if kind != feature.kind {
                changed.push((feature.id, Box::new(kind)));
            }
        }
        for (index, sketch) in sketches {
            next.set_sketch(index, sketch, None);
        }
        for (feature, kind) in changed {
            if let Some(set) = next.with_kind(feature, kind)? {
                next = set;
            }
        }
        Ok(Some(next))
    }

    /// Checks what [`Command::AddFeature`] and [`Command::SetFeature`]
    /// require of `kind`, feature `index` of this document, beyond
    /// [`Document::check`]: a revolve's axis is a line of its sketch, a
    /// split's line's and a sweep's path's curves are curves of their
    /// sketches, and a loft's points and rail curves are of its sketches.
    fn check_new(&self, index: usize, kind: &FeatureKind) -> Result<(), EditError> {
        let id = self.features[index].id;
        let checked = match kind {
            FeatureKind::Sweep(sweep) => sweep
                .check_curves(|sketch| self.sketch_before(index, sketch))
                .map_err(|why| CheckError::Sweep(id, why)),
            FeatureKind::Loft(loft) => loft
                .check_names(|sketch| self.sketch_before(index, sketch))
                .map_err(|why| CheckError::Loft(id, why)),
            FeatureKind::Split(split) => match split.tool.sketch() {
                Some(sketch) => self.sketch_before(index, sketch).map_or(Ok(()), |sketch| {
                    split
                        .check_curves(sketch)
                        .map_err(|why| CheckError::Split(id, why))
                }),
                None => Ok(()),
            },
            FeatureKind::Revolve(revolve) => {
                self.sketch_before(index, revolve.sketch)
                    .map_or(Ok(()), |sketch| {
                        revolve
                            .check_axis(sketch)
                            .map_err(|why| CheckError::Revolve(id, why))
                    })
            }
            _ => Ok(()),
        };
        checked.map_err(EditError::Invalid)
    }
}

impl Document {
    /// The document `command` makes of this one, or `None` if it changes
    /// nothing; sketches solved within `budget`. Each edit works on a
    /// copy, so one that would change nothing returns before copying.
    pub(crate) fn after(
        &self,
        command: Command,
        budget: &Budget,
    ) -> Result<Option<Document>, EditError> {
        let next = match command {
            Command::RemoveBody(id) => {
                return Ok(self.with_removal(self.removal(Removable::Body(id))));
            }
            Command::SetVisible(id, visible) => {
                return Ok(self.with_body(id, |body| body.visible = visible));
            }
            Command::SetOpacity(id, opacity) => {
                return Ok(self.with_body(id, |body| body.opacity = opacity));
            }
            Command::SetColor(id, color) => {
                return Ok(self.with_body(id, |body| body.color = color));
            }
            Command::AddSketch { name, plane } => {
                let mut next = self.clone();
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
                let Some(index) = self.feature_index(feature) else {
                    return Ok(None);
                };
                match &self.features[index].kind {
                    FeatureKind::Sketch { sketch: old, .. } if *old != *sketch => {}
                    _ => return Ok(None),
                }
                let mut next = self.clone();
                next.set_sketch(index, *sketch, None);
                next.keeps_sketch_face(self, index)?;
                next
            }
            Command::AddLink {
                feature,
                sketch,
                source,
            } => {
                let Some(index) = self.feature_index(feature) else {
                    return Ok(None);
                };
                if !matches!(self.features[index].kind, FeatureKind::Sketch { .. }) {
                    return Ok(None);
                }
                let mut next = self.clone();
                next.set_sketch(index, *sketch, Some(source));
                next.keeps_sketch_face(self, index)?;
                next
            }
            Command::SetSketchPlane { feature, plane } => {
                let Some(index) = self.feature_index(feature) else {
                    return Ok(None);
                };
                match &self.features[index].kind {
                    FeatureKind::Sketch { plane: old, .. } if *old != plane => {}
                    _ => return Ok(None),
                }
                let mut next = self.clone();
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
                let mut next = self.clone();
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
            Command::SetFeature { feature, kind } => return self.with_kind(feature, kind),
            Command::RemoveFeature(id) => {
                return Ok(self.with_removal(self.removal(Removable::Feature(id))));
            }
            Command::RemoveFeatures(ids) => {
                let targets: Vec<_> = ids.into_iter().map(Removable::Feature).collect();
                return Ok(self.with_removal(self.removal_of(&targets)));
            }
            Command::RemoveOnly(ids) => {
                let targets: Vec<_> = ids.into_iter().map(Removable::Feature).collect();
                return Ok(self.with_removal(self.breaking_removal(&targets)));
            }
            Command::SetFeatureVisible(id, visible) => {
                let Some(index) = self
                    .feature_index(id)
                    .filter(|&index| self.features[index].visible != visible)
                else {
                    return Ok(None);
                };
                let mut next = self.clone();
                next.features[index].visible = visible;
                next
            }
            Command::SetRollback(rollback) => {
                if self.rollback == rollback
                    || rollback.is_some_and(|id| self.feature(id).is_none())
                {
                    return Ok(None);
                }
                let mut next = self.clone();
                next.rollback = rollback;
                next
            }
            Command::Rename { target, name } => {
                if self.name_of(target).is_none_or(|old| old == name) {
                    return Ok(None);
                }
                let mut next = self.clone();
                if let Some(slot) = next.name_mut(target) {
                    *slot = name;
                }
                next
            }
            Command::SetUnits(units) => {
                if units == self.units {
                    return Ok(None);
                }
                let mut next = self.clone();
                let before = self.design();
                for feature in &mut next.features {
                    if let FeatureKind::Sketch { sketch, .. } = &mut feature.kind {
                        sketch.pin_units(&before);
                    }
                    // Counts, factors and turns have no unit, but are
                    // pinned all the same: a bare number added to a length
                    // inside one took the units.
                    for (value, ask) in feature.kind.values_mut(&before) {
                        value.pin_units(&ask);
                    }
                }
                for (param, result) in next.params.iter_mut().zip(self.resolved.iter()) {
                    if let (_, Ok(resolved)) = result {
                        param.text = (self.resolved)
                            .pin_units(&param.text, self.units)
                            .unwrap_or_else(|_| exact(resolved.value, resolved.quantity));
                    }
                }
                next.units = units;
                next.resolved = param::resolve(&next.params, units);
                next
            }
            Command::AddParam { name, text } => {
                let mut params = self.params.clone();
                params.push(Param {
                    name,
                    text: text.trim().to_owned(),
                });
                return self.with_params(params, None, budget);
            }
            Command::SetParam { index, text } => {
                let mut params = self.params.clone();
                let Some(param) = params.get_mut(index) else {
                    return Ok(None);
                };
                param.text = text.trim().to_owned();
                return self.with_params(params, None, budget);
            }
            Command::RenameParam { index, name } => {
                let Some(old) = self.params.get(index).map(|param| param.name.as_str()) else {
                    return Ok(None);
                };
                if old == name {
                    return Ok(None);
                }
                let mut params = self.params.clone();
                for (at, param) in params.iter_mut().enumerate() {
                    param.text = varde_expr::rename(&param.text, old, &name)
                        .map_err(|why| EditError::Value(ValueOf::Param(at), why))?;
                }
                params[index].name = name.clone();
                return self.with_params(params, Some((old, &name)), budget);
            }
            Command::RemoveParam(index) => {
                let Some(param) = self.params.get(index) else {
                    return Ok(None);
                };
                if self.param_used(&param.name) {
                    return Err(EditError::ParamUsed(param.name.clone()));
                }
                let mut params = self.params.clone();
                params.remove(index);
                return self.with_params(params, None, budget);
            }
            Command::SetTolerance(tolerance) => {
                if tolerance == self.tolerance() {
                    return Ok(None);
                }
                let mut next = self.clone();
                next.tolerance = tolerance.fit();
                next
            }
            Command::Replace(replacement) => {
                if *replacement == *self {
                    return Ok(None);
                }
                *replacement
            }
        };
        Ok(Some(next))
    }

    /// A copy with `removal` removed, or `None` if it removes nothing.
    fn with_removal(&self, removal: Removal) -> Option<Document> {
        if removal.is_empty() {
            return None;
        }
        let mut next = self.clone();
        next.remove(&removal);
        Some(next)
    }
}

/// Why an edit was refused. The document is left as it was.
#[derive(Debug, Clone, PartialEq)]
pub enum EditError {
    /// Every id for a body or a feature has been used.
    OutOfIds,
    /// The edit would leave the document failing [`Document::check`], for
    /// the reason given.
    Invalid(CheckError),
    /// An edit of the sketch of this feature can't be made, see
    /// [`SketchEdit::apply`](varde_sketch::SketchEdit::apply).
    Sketch(FeatureId, varde_sketch::EditError),
    /// [`Command::AddFeature`] or [`Command::SetFeature`] was given a
    /// sketch, or asked to set one: sketches are added by
    /// [`Command::AddSketch`] and set by [`Command::SetSketch`].
    SketchKind,
    /// [`Command::SetSketch`] or [`Command::AddLink`] would take away the
    /// sketch face ([`sketch_face`]) of this sketch feature, which stays
    /// while the sketch is on its face.
    SketchFace(FeatureId),
    /// A parameter command would leave this value in error: a feature's
    /// that uses the parameter (directly or through others) or, renaming
    /// one, a parameter's text the new name takes over
    /// [`MAX_LEN`](varde_expr::MAX_LEN).
    Value(ValueOf, varde_expr::Error),
    /// [`Command::RemoveParam`] was asked to remove this parameter, which
    /// a feature value or another parameter uses.
    ParamUsed(String),
    /// A parameter command changes dimensions of this sketch feature's
    /// sketch, which then doesn't solve, see
    /// [`varde_sketch::revalue`].
    Unsolved(FeatureId, varde_sketch::Rejected),
}

/// Whose value an [`EditError::Value`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueOf {
    /// One of this feature's values.
    Feature(FeatureId),
    /// The text of the parameter at this index.
    Param(usize),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::OutOfIds => f.write_str("the document has no ids left"),
            EditError::Invalid(why) => why.fmt(f),
            EditError::Sketch(id, why) => write!(f, "feature {}: {why}", id.0),
            EditError::SketchKind => {
                f.write_str("sketches are added and set by their own commands")
            }
            EditError::SketchFace(_) => f.write_str("the sketch face can't be removed"),
            EditError::Value(ValueOf::Feature(id), why) => {
                write!(f, "feature {}: a value would be in error: {why}", id.0)
            }
            EditError::Value(ValueOf::Param(index), why) => {
                write!(f, "parameter {}: {why}", index + 1)
            }
            EditError::ParamUsed(name) => write!(f, "the parameter '{name}' is in use"),
            EditError::Unsolved(id, why) => {
                write!(f, "feature {}: the sketch wouldn't solve: {why}", id.0)
            }
        }
    }
}

impl std::error::Error for EditError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EditError::OutOfIds
            | EditError::SketchKind
            | EditError::SketchFace(_)
            | EditError::ParamUsed(_) => None,
            EditError::Invalid(why) => Some(why),
            EditError::Value(_, why) => Some(why),
            EditError::Sketch(_, why) => Some(why),
            EditError::Unsolved(_, why) => Some(why),
        }
    }
}
