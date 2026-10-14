//! Planning the bodies a feature makes: a new body's id, the one it held,
//! and a pattern's copy bodies.

use std::collections::BTreeMap;

use super::EditError;
use crate::{
    Body, BodyId, CheckError, Copies, Document, FeatureId, FeatureKind, MAX_PATTERN_BODIES,
    Opacity, Operation, Pattern,
};

impl Document {
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

/// Setting a feature's kind, as [`Document::plan_kind`] plans it for
/// [`Document::set_kind`].
pub(super) struct KindPlan {
    feature: FeatureId,
    index: usize,
    kind: Box<FeatureKind>,
    /// The pattern's copy bodies, see [`planned_copies`].
    copies: Option<Vec<Option<BodyId>>>,
    /// The copy bodies it no longer makes, see [`Document::copies_dropped`].
    dropped: Vec<BodyId>,
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

/// Fills in the body `kind` makes or holds, as [`Command::AddFeature`](crate::Command::AddFeature)
/// (with no `old`) and [`Command::SetFeature`](crate::Command::SetFeature) (in place of `old`) take
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
    /// Has `feature` make `body` as its new body.
    fn set_new_body(&mut self, feature: FeatureId, body: BodyId) {
        if let Some(index) = self.feature_index(feature)
            && let Some(new) = self.features[index].kind.new_body_mut()
        {
            *new = body;
        }
    }

    /// The copy bodies of the pattern feature `id` that setting it to
    /// `kind` ([`Command::SetFeature`](crate::Command::SetFeature)) would remove, sorted: those it
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
    /// [`Command::SetFeature`](crate::Command::SetFeature) does; `None` if that changes nothing.
    pub(super) fn with_kind(
        &self,
        feature: FeatureId,
        kind: Box<FeatureKind>,
    ) -> Result<Option<Document>, EditError> {
        let Some(plan) = self.plan_kind(feature, kind)? else {
            return Ok(None);
        };
        let mut next = self.clone();
        next.set_kind(plan)?;
        Ok(Some(next))
    }

    /// Plans setting feature `feature`'s kind to `kind`, as
    /// [`Command::SetFeature`](crate::Command::SetFeature) does, without
    /// changing anything: `kind` with the body it makes or holds filled
    /// in, and a pattern's copies; `None` if that changes nothing.
    pub(super) fn plan_kind(
        &self,
        feature: FeatureId,
        mut kind: Box<FeatureKind>,
    ) -> Result<Option<KindPlan>, EditError> {
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
        let dropped = self.copies_dropped(feature, &kind);
        Ok(Some(KindPlan {
            feature,
            index,
            kind,
            copies,
            dropped,
        }))
    }

    /// Sets a feature's kind in place as `plan`
    /// ([`Document::plan_kind`]) has it: its bodies removed, restored or
    /// added, then [`Document::check_new`] on it.
    pub(super) fn set_kind(&mut self, plan: KindPlan) -> Result<(), EditError> {
        let KindPlan {
            feature,
            index,
            kind,
            copies,
            dropped,
        } = plan;
        let kept = self.features[index].kind.new_body();
        let makes_body = kind.new_body().is_some();
        self.features[index].kind = *kind;
        self.remove_bodies(&dropped);
        if let Some(planned) = copies {
            self.make_copies(feature, planned)?;
        }
        match (kept, makes_body) {
            (Some(body), false) => {
                if let Some(at) = self.body_index(body) {
                    self.bodies.remove(at);
                }
                self.drop_excluded(&[body]);
            }
            (None, true) => match self.features[index].kind.new_body() {
                // Made again with the id it held.
                Some(held) if held != BodyId::NEW => self.restore_body(feature, held),
                _ => {
                    let body = self.add_body(feature)?;
                    self.set_new_body(feature, body);
                }
            },
            _ => {}
        }
        self.check_new(index, &self.features[index].kind)
    }

    /// The document with a feature of `kind` added, as
    /// [`Command::AddFeature`](crate::Command::AddFeature) does.
    pub(super) fn with_new_feature(
        &self,
        name: String,
        mut kind: Box<FeatureKind>,
    ) -> Result<Document, EditError> {
        if matches!(*kind, FeatureKind::Sketch { .. }) {
            return Err(EditError::SketchKind);
        }
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
        let index = next.feature_index(id).expect("just added");
        next.check_new(index, &next.features[index].kind)?;
        Ok(next)
    }

    /// Checks what [`Command::AddFeature`](crate::Command::AddFeature) and [`Command::SetFeature`](crate::Command::SetFeature)
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
