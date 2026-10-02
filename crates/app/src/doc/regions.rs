//! What the operation sessions share, the extrude's and the revolve's:
//! picking regions of a sketch ([`RegionPick`]), the bodies a join, cut or
//! intersect takes out ([`BodyTargets`]), the fields values are typed in
//! ([`TypedText`]), and committing what's set up
//! ([`Doc::commit_feature`]).

use std::collections::BTreeSet;
use std::sync::Arc;

use varde_document::{
    BodyId, Command, Document, FeatureId, FeatureKind, Operation, Placement, RegionRef, Sketch,
    Targets,
};
use varde_expr::{Ask, Value};
use varde_sketch::{MAX_WORK, Profiles};
use varde_view::{BodyTarget, Candidate, OperationKind, TypedField};

use super::Doc;
use super::extrude::is_sketch;

/// The most work finding the profiles of the visible sketches may take
/// in all, in the unit of [`MAX_WORK`], on the UI thread: a file can hold
/// any number of sketches, each as complex as [`MAX_WORK`] allows.
/// Those past it have no regions to pick.
pub(crate) const REFRESH_WORK: usize = 2 * MAX_WORK;

/// The regions an operation takes, of the sketch it takes them of: the
/// source, the one the session started from (selected, or the edited
/// feature's), or else the one the first region (or a revolve's axis) is
/// picked in, which un-picking every region lets go of again.
#[derive(Debug)]
pub(crate) struct RegionPick {
    /// The sketch the regions are of, once there is one.
    pub(crate) source: Option<FeatureId>,
    /// Whether the source stays when no region is picked: one the
    /// session started from, selected or edited, rather than picked.
    fixed: bool,
    /// The profiles of the source, or before there is one of the visible
    /// sketches, those with regions.
    pub(crate) found: Vec<Found>,
    /// The sketches found to have no regions to pick: too complex within
    /// the whole of [`MAX_WORK`], or but for the source with none. Kept,
    /// as those found are, and also while not wanted, so as not to work
    /// them out again on every change to the document until they change.
    /// Those past what's left of [`REFRESH_WORK`] aren't: they're tried
    /// again on the next change.
    pub(crate) skipped: Vec<(FeatureId, Sketch)>,
    /// How many times profiles were worked out, for tests.
    #[cfg(test)]
    pub(crate) worked_out: usize,
    /// The regions picked, by index into the source's profiles.
    pub(crate) picked: BTreeSet<usize>,
    /// The references to them, in the same order, made as they're picked
    /// (and again when the sketch changes). Kept while the source's
    /// regions can't be found, which leaves none picked, to find them
    /// again once they can.
    references: Vec<RegionRef>,
    /// How many of the edited feature's regions weren't found.
    pub(crate) missing: usize,
    /// The most regions that can be picked: the feature's limit.
    most: usize,
}

/// A sketch's profiles, and the sketch they're of.
#[derive(Debug)]
pub(crate) struct Found {
    pub(crate) feature: FeatureId,
    pub(crate) sketch: Sketch,
    pub(crate) profiles: Arc<Profiles>,
}

impl RegionPick {
    /// None picked yet, of `source` if given, else of the sketch the
    /// first is picked in; at most `most`. Its profiles are found by
    /// [`RegionPick::refresh`].
    pub(crate) fn new(source: Option<FeatureId>, most: usize) -> Self {
        Self {
            source,
            fixed: source.is_some(),
            found: Vec::new(),
            skipped: Vec::new(),
            #[cfg(test)]
            worked_out: 0,
            picked: BTreeSet::new(),
            references: Vec::new(),
            missing: 0,
            most,
        }
    }

    /// The regions of `sketch` of `document` that `regions`, an edited
    /// feature's references, find, counting those they don't.
    pub(crate) fn editing(
        document: &Document,
        sketch: FeatureId,
        regions: &[RegionRef],
        most: usize,
    ) -> Self {
        let mut pick = Self::new(Some(sketch), most);
        pick.refresh(document);
        let found = pick
            .found(sketch)
            .map(|found| found.profiles.resolve(regions));
        let resolved = found.unwrap_or_else(|| vec![None; regions.len()]);
        pick.missing = resolved.iter().filter(|index| index.is_none()).count();
        let picked = resolved.into_iter().flatten().collect();
        pick.pick(picked);
        pick
    }

    /// The profiles found of `feature`, if it's a candidate.
    pub(crate) fn found(&self, feature: FeatureId) -> Option<&Found> {
        self.found.iter().find(|found| found.feature == feature)
    }

    /// The references to the regions picked, as the feature stores them.
    pub(crate) fn references(&self) -> &[RegionRef] {
        &self.references
    }

    /// Picks the regions `picked` of the source, making their references.
    /// Those that can't be referenced, too thin for a point inside to be
    /// found, are left out.
    pub(crate) fn pick(&mut self, picked: BTreeSet<usize>) {
        let profiles = self
            .source
            .and_then(|source| self.found(source))
            .map(|found| found.profiles.clone());
        let Some(profiles) = profiles else {
            self.picked.clear();
            self.references.clear();
            return;
        };
        let referenced: Vec<(usize, RegionRef)> = picked
            .into_iter()
            .filter_map(|index| Some((index, profiles.reference(index)?)))
            .take(self.most)
            .collect();
        self.picked = referenced.iter().map(|(index, _)| *index).collect();
        self.references = referenced
            .into_iter()
            .map(|(_, reference)| reference)
            .collect();
    }

    /// Takes the sketch `sketch` as the source, if it's a candidate and
    /// there's no other source: false if not.
    pub(crate) fn choose(&mut self, sketch: FeatureId) -> bool {
        if self.source.is_some_and(|source| source != sketch) || self.found(sketch).is_none() {
            return false;
        }
        self.source = Some(sketch);
        true
    }

    /// Picks the region `region` of `sketch`, or takes it out if it's
    /// picked: only of the source, or the first of any candidate, which
    /// becomes the source. All taken out, another sketch's may be picked,
    /// unless the source is fixed, or `keep` (a revolve's axis is on it).
    pub(crate) fn toggle(
        &mut self,
        sketch: FeatureId,
        region: usize,
        keep: bool,
        document: &Document,
    ) {
        let in_range = self
            .found(sketch)
            .is_some_and(|found| region < found.profiles.regions.len());
        if !in_range || !self.choose(sketch) {
            return;
        }
        let mut picked = self.picked.clone();
        if !picked.remove(&region) {
            picked.insert(region);
        }
        self.pick(picked);
        if self.picked.is_empty() && !self.fixed && !keep {
            self.source = None;
        }
        self.refresh(document);
    }

    /// Finds the profiles of the sketches of `document` whose regions can
    /// be picked again where their sketch changed, and those picked in
    /// the source again by their references. False if the source is gone.
    /// The source's within [`MAX_WORK`], or before there is one the
    /// visible sketches' within [`REFRESH_WORK`] in all.
    pub(crate) fn refresh(&mut self, document: &Document) -> bool {
        let wanted: Vec<FeatureId> = match self.source {
            Some(source) => vec![source],
            None => document
                .features()
                .iter()
                .filter(|feature| feature.visible)
                .map(|feature| feature.id)
                .collect(),
        };
        let mut old = std::mem::take(&mut self.found);
        let mut old_skipped = std::mem::take(&mut self.skipped);
        let mut left = REFRESH_WORK;
        let mut remap = false;
        for id in wanted {
            let Some(FeatureKind::Sketch { sketch, .. }) = document.feature(id).map(|f| &f.kind)
            else {
                if Some(id) == self.source {
                    return false;
                }
                continue;
            };
            let kept = old.iter().position(|found| found.feature == id);
            let is_source = Some(id) == self.source;
            if let Some(at) = kept.filter(|&at| old[at].sketch == *sketch) {
                self.found.push(old.swap_remove(at));
                continue;
            }
            let skipped = old_skipped.iter().position(|(feature, _)| *feature == id);
            if let Some(at) = skipped {
                let skipped = old_skipped.swap_remove(at);
                if skipped.1 == *sketch {
                    self.skipped.push(skipped);
                    continue;
                }
            }
            remap |= is_source;
            // Past the budget, not even its splines' shapes are worked
            // out: a file may hold any number of sketches. Nor kept as
            // skipped, as it's no more complex than those that took the
            // work: tried again on a later change, when those found are
            // kept and spend none.
            if !is_source && left == 0 {
                continue;
            }
            #[cfg(test)]
            {
                self.worked_out += 1;
            }
            // A sketch too complex for its regions to be found has none
            // to pick; too complex for less than the whole of
            // [`MAX_WORK`], as with that left of the budget, it may not
            // be, and it's tried again on a later change.
            let whole = is_source || left >= MAX_WORK;
            let found = if is_source {
                sketch.profiles()
            } else {
                sketch.profiles_spending(&mut left)
            };
            match found {
                Ok(profiles) if is_source || !profiles.regions.is_empty() => {
                    self.found.push(Found {
                        feature: id,
                        sketch: sketch.clone(),
                        profiles: Arc::new(profiles),
                    });
                }
                Err(_) if !whole => {}
                _ => self.skipped.push((id, sketch.clone())),
            }
        }
        // Those not wanted now, while there's a source, may be again
        // once there isn't.
        let others = old_skipped.into_iter();
        (self.skipped).extend(others.filter(|(id, _)| is_sketch(document, *id)));
        if remap {
            let source = self.source.and_then(|source| self.found(source));
            match source.map(|found| found.profiles.resolve(&self.references)) {
                Some(resolved) => self.pick(resolved.into_iter().flatten().collect()),
                // Its regions can't be found for now: the references wait
                // for the sketch to have them again.
                None => self.picked.clear(),
            }
        }
        true
    }

    /// The sketches whose regions show, for the view: each found's that
    /// `placement` places ([`Doc::placement`]), there. A sketch on a face
    /// that isn't placed has no regions to pick.
    pub(crate) fn candidates(
        &self,
        placement: impl Fn(FeatureId) -> Option<Placement>,
    ) -> Vec<Candidate<'_>> {
        (self.found.iter())
            .filter_map(|found| {
                Some(Candidate {
                    feature: found.feature,
                    placement: placement(found.feature)?,
                    sketch: &found.sketch,
                    profiles: &found.profiles,
                })
            })
            .collect()
    }
}

/// The bodies a join, cut or intersect takes out, and those just put
/// back.
#[derive(Debug, Default)]
pub(crate) struct BodyTargets {
    /// The bodies left out, sorted: the edited feature's to start with.
    pub(crate) excluded: Vec<BodyId>,
    /// The bodies put back after being taken out, each with the newest
    /// draft revision before it was: listed until a touch test of a later
    /// draft answers, which they were part of, so a body ticked again
    /// doesn't drop out of the list while that answer is on its way.
    /// One entry per body, so no longer than the document's bodies.
    pub(crate) reticked: Vec<(BodyId, u64)>,
}

impl BodyTargets {
    /// Leaving out `excluded`.
    pub(crate) fn new(excluded: &[BodyId]) -> Self {
        Self {
            excluded: excluded.to_vec(),
            reticked: Vec::new(),
        }
    }

    /// Takes `body` out, or puts it back, if it's one of `document`'s made
    /// before the feature `edited` (any, for a new one). `revision` is the
    /// newest draft revision given out: a body put back is listed until a
    /// touch test of a later one answers.
    pub(crate) fn toggle(
        &mut self,
        body: BodyId,
        edited: Option<FeatureId>,
        document: &Document,
        revision: u64,
    ) {
        match self.excluded.binary_search(&body) {
            Ok(at) => {
                self.excluded.remove(at);
                self.reticked.retain(|(reticked, _)| *reticked != body);
                self.reticked.push((body, revision));
            }
            Err(at) => {
                let made_before = document.body(body).is_some_and(|made| {
                    let maker = document
                        .features()
                        .iter()
                        .position(|f| f.id == made.created_by);
                    let edited = edited.and_then(|feature| {
                        document.features().iter().position(|f| f.id == feature)
                    });
                    maker.is_some_and(|maker| edited.is_none_or(|edited| maker < edited))
                });
                if made_before {
                    self.excluded.insert(at, body);
                }
            }
        }
    }

    /// What the feature does with its solid as `kind`, leaving these
    /// bodies out of a join, cut or intersect.
    pub(crate) fn operation(&self, kind: OperationKind) -> Operation {
        let targets = Targets {
            excluded: self.excluded.clone(),
        };
        match kind {
            OperationKind::NewBody => Operation::NewBody(BodyId::NEW),
            OperationKind::Join => Operation::Join(targets),
            OperationKind::Cut => Operation::Cut(targets),
            OperationKind::Intersect => Operation::Intersect(targets),
        }
    }

    /// Lets go of the bodies `document` no longer holds (undone, say),
    /// which can't be taken out, nor put back.
    pub(crate) fn prune(&mut self, document: &Document) {
        self.excluded.retain(|&body| document.body(body).is_some());
        self.reticked
            .retain(|&(body, _)| document.body(body).is_some());
    }
}

/// A typed value's field: the text as typed, the value it last gave, and
/// why the text is refused, if it is.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TypedText {
    pub(crate) text: String,
    pub(crate) value: Option<Value>,
    pub(crate) error: Option<varde_expr::Error>,
}

impl TypedText {
    /// The field with `text` read for `ask`.
    pub(crate) fn read(text: String, ask: &Ask) -> Self {
        let mut field = Self {
            text: String::new(),
            value: None,
            error: None,
        };
        field.input(text, ask);
        field
    }

    /// The field holding `value`, a stored value asked for by `ask`. It
    /// shows the text with the design's unit written after its bare
    /// numbers ("10 mm" for "10"; an angle's bare numbers are degrees
    /// whatever the units, so only lengths inside it change), as a new
    /// one's does; the value stays as it was typed, so OK with nothing
    /// changed writes nothing.
    pub(crate) fn of(value: &Value, ask: &Ask) -> Self {
        let mut shown = value.clone();
        shown.pin_units(ask);
        Self {
            text: shown.text,
            value: Some(value.clone()),
            error: None,
        }
    }

    /// The field as the view shows it.
    pub(crate) fn field(&self) -> TypedField<'_> {
        TypedField {
            text: &self.text,
            error: self.error.as_ref(),
            value: self.value.as_ref().map(|value| value.value),
        }
    }

    /// Reads `text` for `ask`: the value it gives, or why it's refused,
    /// keeping the last value.
    pub(crate) fn input(&mut self, text: String, ask: &Ask) {
        match Value::new(&text, ask) {
            Ok(value) => {
                self.value = Some(value);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
        self.text = text;
    }

    /// Keeps its value where the design's units changed since it was
    /// read for `before`, as the document does its own: the units the
    /// text was read in are written after its bare numbers. A text that's
    /// refused stays as typed, to be read in the new units.
    pub(crate) fn follow_units(&mut self, before: &Ask) {
        if let Some(value) = &mut self.value {
            value.pin_units(before);
            if self.error.is_none() {
                self.text = value.text.clone();
            }
        }
    }
}

impl Doc {
    /// Adds a feature of `kind` set up in a session, or sets the feature
    /// `edited` to it, as one undo step: false if the document refuses
    /// it, which then shows why, and the session stays. A feature added
    /// is selected; one set with nothing changed writes nothing.
    pub(crate) fn commit_feature(&mut self, edited: Option<FeatureId>, kind: FeatureKind) -> bool {
        let command = match edited {
            Some(feature) => Command::SetFeature {
                feature,
                kind: Box::new(kind),
            },
            None => self.editor.document().add_feature(kind),
        };
        let before = self.editor.revision();
        self.apply(command);
        if self.edit_error.is_some() {
            return false;
        }
        if edited.is_none() && self.editor.revision() != before {
            // New features get the highest id, so it's the last.
            self.selected_feature = self.editor.document().features().last().map(|f| f.id);
        }
        true
    }

    /// The bodies a session's `operation` lists, of the feature `edited`
    /// (or a new one) with `targets`: those its preview touches, those
    /// taken out, and those put back since the touch test last answered,
    /// in the order they were made. A body an earlier join merged into
    /// another (as the model shown found) is never touched, so it's
    /// listed only while it's taken out, which does nothing then, or just
    /// put back: with the body holding it, so that it can be seen and put
    /// back.
    pub(crate) fn body_targets<'a>(
        &'a self,
        operation: OperationKind,
        edited: Option<FeatureId>,
        targets: &BodyTargets,
    ) -> Vec<BodyTarget<'a>> {
        if !operation.has_targets() {
            return Vec::new();
        }
        let document = self.editor.document();
        let merged = self.feed.merged_before(document, edited);
        let touched = self.feed.draft_touched();
        let answered = self.feed.draft_touched_revision();
        let reticked = |body: BodyId| {
            targets.reticked.iter().any(|&(reticked, since)| {
                reticked == body && answered.is_none_or(|answered| answered <= since)
            })
        };
        (document.bodies().iter())
            .filter(|body| {
                touched.contains(&body.id)
                    || targets.excluded.contains(&body.id)
                    || reticked(body.id)
            })
            .map(|body| BodyTarget {
                body: body.id,
                name: &body.name,
                included: targets.excluded.binary_search(&body.id).is_err(),
                holder: (merged.holder(body.id))
                    .filter(|_| !touched.contains(&body.id))
                    .and_then(|holder| document.body(holder))
                    .map(|holder| holder.name.as_str()),
            })
            .collect()
    }
}
