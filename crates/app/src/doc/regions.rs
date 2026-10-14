//! What the operation sessions share, the extrude's and the revolve's:
//! picking regions of a sketch ([`RegionPick`]), the bodies a join, cut or
//! intersect takes out ([`BodyTargets`]), the fields values are typed in
//! ([`TypedText`]), and committing what's set up
//! ([`Doc::commit_feature`]).

use std::collections::BTreeSet;
use std::sync::Arc;

use varde_document::{
    BodyId, Command, Design, Document, FeatureId, FeatureKind, LengthUnit, MAX_COORD, Operation,
    Params, Placement, RegionRef, Sketch, Targets,
};
use varde_expr::{Ask, Unit, Value};
use varde_sketch::{MAX_WORK, Profiles};
use varde_view::{
    BodyTarget, Candidate, OperationKind, ParamsIn, Side, Sides, TypedField, Verdict,
};

use super::{Doc, is_sketch};

/// Whether an operation of `document` edited from `edited` (a new one,
/// where new features go, if `None`) can name `body`: it's there, made by
/// a feature before the operation, as the document's check wants.
pub(crate) fn pickable(document: &Document, body: BodyId, edited: Option<FeatureId>) -> bool {
    let at = edited.map_or(Some(document.insert_at()), |id| document.feature_index(id));
    document.body(body).is_some_and(|made| {
        let maker = document.feature_index(made.created_by);
        maker.is_some_and(|maker| at.is_none_or(|at| maker < at))
    })
}

/// The most work finding the profiles of the visible sketches may take
/// in all, in the unit of [`MAX_WORK`], on the UI thread: a file can hold
/// any number of sketches, each as complex as [`MAX_WORK`] allows.
/// Those past it have no regions to pick.
pub(crate) fn refresh_work() -> usize {
    2 * work_unit()
}

/// The most work finding the profiles of [`RegionPick::also`]'s sketches
/// may take in all, apart from [`refresh_work`]: a loft's up to 64
/// sections may each be of a sketch of its own, hidden. Those past it
/// are worked out on a later change, as the visible ones past theirs.
pub(crate) fn also_work() -> usize {
    4 * work_unit()
}

/// The unit of [`refresh_work`] and [`also_work`]: [`MAX_WORK`], or in
/// tests what `with_work_unit` sets on this thread, so that tests of
/// the budgets needn't spend the whole of them in a debug build.
fn work_unit() -> usize {
    #[cfg(test)]
    if let Some(unit) = WORK_UNIT.get() {
        return unit;
    }
    MAX_WORK
}

#[cfg(test)]
thread_local! {
    static WORK_UNIT: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Runs `f` with [`work_unit`] `unit` (at most [`MAX_WORK`]) on this
/// thread.
#[cfg(test)]
pub(crate) fn with_work_unit<R>(unit: usize, f: impl FnOnce() -> R) -> R {
    assert!(unit <= MAX_WORK);
    let old = WORK_UNIT.replace(Some(unit));
    let result = f();
    WORK_UNIT.set(old);
    result
}

/// The regions an operation takes, of the sketch it takes them of: the
/// source, the one the session started from (selected, or the edited
/// feature's), or else the one the first region (or a revolve's axis) is
/// picked in, which un-picking every region lets go of again.
#[derive(Debug, Clone)]
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
    /// Those past what's left of [`refresh_work`] aren't: they're tried
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
    /// The edited feature's references that find no region, listed as
    /// missing and named by the feature until taken out, so it fails and
    /// can be added anyway; found again as the sketch changes.
    lost: Vec<RegionRef>,
    /// How many of the edited feature's regions weren't found.
    pub(crate) missing: usize,
    /// The most regions that can be picked: the feature's limit.
    most: usize,
    /// Sketches whose profiles are found too while there's no source,
    /// whether shown or not, within [`also_work`] in all (not
    /// [`refresh_work`]), kept even with no regions: a loft's sections'
    /// sketches, which adding it hid.
    pub(crate) also: Vec<FeatureId>,
}

/// The regions picked of a sketch that's gone (an undo took it away),
/// put by while others are offered, and picked again if it comes back
/// before others are picked: a split's and a sweep's.
#[derive(Debug, Clone, Default)]
pub(crate) struct GoneRegions(Option<(FeatureId, Vec<RegionRef>)>);

impl GoneRegions {
    /// Puts `pick`'s regions by where its source is no longer a sketch of
    /// `document`, leaving none picked, or picks those put by again where
    /// none are and their sketch is back.
    pub(crate) fn follow(&mut self, pick: &mut RegionPick, document: &Document) {
        if let Some(source) = pick.source
            && !is_sketch(document, source)
        {
            self.0 = Some((source, pick.references()));
            *pick = RegionPick::new(None, pick.most);
        } else if pick.source.is_none()
            && let Some((source, regions)) =
                self.0.take_if(|(source, _)| is_sketch(document, *source))
        {
            *pick = RegionPick::editing(document, source, &regions, pick.most);
        }
    }

    /// Picks the region `region` of `sketch` in `pick`, or takes it out
    /// ([`RegionPick::toggle`]), letting go of those put by once `pick`
    /// has a source again: other regions picked.
    pub(crate) fn toggle(
        &mut self,
        pick: &mut RegionPick,
        sketch: FeatureId,
        region: usize,
        document: &Document,
    ) {
        pick.toggle(sketch, region, false, document);
        if pick.source.is_some() {
            self.0 = None;
        }
    }

    /// Whether regions are put by.
    pub(crate) fn is_some(&self) -> bool {
        self.0.is_some()
    }
}

/// A sketch's profiles, and the sketch they're of.
#[derive(Debug, Clone)]
pub(crate) struct Found {
    pub(crate) feature: FeatureId,
    pub(crate) sketch: Sketch,
    pub(crate) profiles: Arc<Profiles>,
}

/// The regions `references` resolved to, `resolved`, split into those
/// found and the references that weren't.
fn split_resolved(
    references: &[RegionRef],
    resolved: Vec<Option<usize>>,
) -> (BTreeSet<usize>, Vec<RegionRef>) {
    let mut found = BTreeSet::new();
    let mut lost = Vec::new();
    for (reference, index) in references.iter().zip(resolved) {
        match index {
            Some(index) => {
                found.insert(index);
            }
            None => lost.push(reference.clone()),
        }
    }
    (found, lost)
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
            lost: Vec::new(),
            missing: 0,
            most,
            also: Vec::new(),
        }
    }

    /// The regions of `sketch` of `document` that `regions`, an edited
    /// feature's references, find, counting those they don't. With the
    /// sketch gone (deleted, the feature kept), none are found, and
    /// another sketch's may be picked, which lets go of them.
    pub(crate) fn editing(
        document: &Document,
        sketch: FeatureId,
        regions: &[RegionRef],
        most: usize,
    ) -> Self {
        let gone = document.feature(sketch).is_none();
        let mut pick = Self::new((!gone).then_some(sketch), most);
        pick.refresh(document);
        let found = pick
            .found(sketch)
            .map(|found| found.profiles.resolve(regions));
        let resolved = found.unwrap_or_else(|| vec![None; regions.len()]);
        let (picked, lost) = split_resolved(regions, resolved);
        pick.pick_within(picked, most);
        pick.lost = lost;
        pick.missing = pick.lost.len();
        pick
    }

    /// The most regions that can be picked.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn most(&self) -> usize {
        self.most
    }

    /// The profiles found of `feature`, if it's a candidate.
    pub(crate) fn found(&self, feature: FeatureId) -> Option<&Found> {
        self.found.iter().find(|found| found.feature == feature)
    }

    /// The references to the regions picked, as the feature stores them,
    /// then the edited feature's that weren't found, listed as missing
    /// until taken out ([`RegionPick::drop_missing`]): a feature naming
    /// one fails, and Add anyway keeps it so.
    pub(crate) fn references(&self) -> Vec<RegionRef> {
        (self.references.iter())
            .chain(&self.lost)
            .cloned()
            .collect()
    }

    /// Takes out the edited feature's `index`th region that wasn't
    /// found, if there's one.
    pub(crate) fn drop_missing(&mut self, index: usize) {
        if index < self.lost.len() {
            self.lost.remove(index);
            self.missing = self.lost.len();
        }
    }

    /// Picks the regions `picked` of the source, making their references.
    /// Those that can't be referenced, too thin for a point inside to be
    /// found, are left out.
    pub(crate) fn pick(&mut self, picked: BTreeSet<usize>) {
        self.pick_within(picked, self.most.saturating_sub(self.lost.len()));
    }

    /// [`RegionPick::pick`], at most `most` of them.
    fn pick_within(&mut self, picked: BTreeSet<usize>, most: usize) {
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
            .take(most)
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
        if self.source.is_none() {
            // An edited feature's regions that weren't found were of a
            // sketch that's gone, and mean nothing in this one.
            self.lost.clear();
            self.missing = 0;
        }
        self.source = Some(sketch);
        true
    }

    /// Takes the curves `curves` of `sketch`, selected: the sketch as the
    /// source and the regions they bound alone (every piece of the outer
    /// loop on one of them) picked. False, nothing changed, if they bound
    /// none, or the sketch can't be the source.
    pub(crate) fn take_curves(
        &mut self,
        sketch: FeatureId,
        curves: &[varde_sketch::Id],
        document: &Document,
    ) -> bool {
        let Some(found) = self.found(sketch) else {
            return false;
        };
        let bounded: BTreeSet<usize> = (found.profiles.regions.iter().enumerate())
            .filter(|(_, region)| (region.outer.iter()).all(|piece| curves.contains(&piece.curve)))
            .map(|(index, _)| index)
            .take(self.most)
            .collect();
        if bounded.is_empty() || !self.choose(sketch) {
            return false;
        }
        self.pick(bounded);
        self.refresh(document);
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
    /// visible sketches' within [`refresh_work`] in all, and
    /// [`RegionPick::also`]'s within [`also_work`].
    pub(crate) fn refresh(&mut self, document: &Document) -> bool {
        let wanted = self.wanted(document);
        let mut old = std::mem::take(&mut self.found);
        let mut old_skipped = std::mem::take(&mut self.skipped);
        let mut left = refresh_work();
        let mut also_left = also_work();
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
            let also = !is_source && self.source.is_none() && self.also.contains(&id);
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
            let left = if also { &mut also_left } else { &mut left };
            if !is_source && *left == 0 {
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
            let whole = is_source || *left >= MAX_WORK;
            let found = if is_source {
                sketch.profiles()
            } else {
                sketch.profiles_spending(left)
            };
            match found {
                Ok(profiles) if is_source || also || !profiles.regions.is_empty() => {
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
            self.remap();
        }
        true
    }

    /// The sketches whose profiles are wanted: the source, or before
    /// there is one, the visible sketches and [`RegionPick::also`]'s.
    fn wanted(&self, document: &Document) -> Vec<FeatureId> {
        match self.source {
            Some(source) => vec![source],
            None => (document.features().iter())
                .filter(|feature| feature.visible || self.also.contains(&feature.id))
                .map(|feature| feature.id)
                .collect(),
        }
    }

    /// Picks the regions of the source, its sketch changed, again by
    /// their references, and those lost too where they're found again.
    /// Its regions not found for now, the references wait for the
    /// sketch to have them again.
    fn remap(&mut self) {
        let source = self.source.and_then(|source| self.found(source));
        let Some(profiles) = source.map(|found| found.profiles.clone()) else {
            self.picked.clear();
            return;
        };
        let (mut picked, _) = split_resolved(&self.references, profiles.resolve(&self.references));
        let (found_again, lost) = split_resolved(&self.lost, profiles.resolve(&self.lost));
        picked.extend(found_again);
        // Those still lost don't count against the most here: they were
        // kept beside the picked before, within it.
        self.pick_within(picked, self.most);
        self.lost = lost;
        self.missing = self.lost.len();
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
#[derive(Debug, Clone, Default)]
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
    /// before the feature `edited` (where a new one goes, for `None`). `revision` is the
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
                if pickable(document, body, edited) {
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
            held: None,
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

/// The design a session's typed fields were last read in: its units,
/// which bare numbers in them took, and its parameters, which names in
/// them are, shared with the document they're from.
#[derive(Debug, Clone)]
pub(crate) struct ReadIn {
    units: LengthUnit,
    params: Arc<Params>,
}

impl ReadIn {
    pub(crate) fn of(document: &Document) -> ReadIn {
        ReadIn {
            units: document.units(),
            params: document.params_shared(),
        }
    }

    /// What fields are read with: as [`Document::design`] gives it.
    pub(crate) fn design(&self) -> Design<'_> {
        Design {
            max: f64::from(MAX_COORD),
            units: self.units,
            params: &self.params,
        }
    }

    /// Takes `document`'s units and parameters: what it read in before,
    /// if either changed, for the fields to follow them
    /// ([`TypedText::follow`]).
    pub(crate) fn follow(&mut self, document: &Document) -> Option<ReadIn> {
        let before = std::mem::replace(self, ReadIn::of(document));
        let same = before.units == self.units
            && (Arc::ptr_eq(&before.params, &self.params) || before.params == self.params);
        (!same).then_some(before)
    }
}

/// A typed value's field: the text as typed, the value it last gave, and
/// why the text is refused, if it is. Empty by default.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TypedText {
    pub(crate) text: String,
    pub(crate) value: Option<Value>,
    pub(crate) error: Option<varde_expr::Error>,
}

impl TypedText {
    /// The field with `text` read for `ask`.
    pub(crate) fn read(text: String, ask: &Ask) -> Self {
        let mut field = Self::default();
        field.input(text, ask);
        field
    }

    /// The field a new feature opens with `text` in, a default: shown as
    /// [`TypedText::of`] shows the value it reads to, or as typed where
    /// `ask` refuses it.
    pub(crate) fn opened(text: &str, ask: &Ask) -> Self {
        Value::new(text, ask).map_or_else(
            |_| Self::read(text.to_owned(), ask),
            |value| Self::of(&value, ask),
        )
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

    /// The field as the view shows it, offering `params`' names.
    pub(crate) fn field<'a>(&'a self, params: ParamsIn<'a>) -> TypedField<'a> {
        TypedField {
            text: &self.text,
            error: self.error.as_ref(),
            value: self.value.as_ref().map(|value| value.value),
            params,
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

    /// Follows the design it was read in for `before` changing to the
    /// one `now` asks in: the units as [`TypedText::follow_units`] does,
    /// and a text naming parameters is read again where they changed, so
    /// `height / 2` gives half the new height, or is refused with why.
    pub(crate) fn follow(&mut self, before: &Ask, now: &Ask) {
        if before.units != now.units {
            self.follow_units(before);
        }
        if before.params != now.params && !varde_expr::names(&self.text).is_empty() {
            let text = std::mem::take(&mut self.text);
            self.input(text, now);
        }
    }
}

/// The fields of a two-sided extent, an extrude's distances or a
/// revolve's angles, and its flip: what the two sessions share of it.
#[derive(Debug, Clone)]
pub(crate) struct SideFields {
    /// The first value's field, and two sides' second.
    pub(crate) fields: [TypedText; 2],
    /// Whether one side goes the other way, or two sides swap: kept
    /// while the extent ignores it (see [`SideFields::stored_flip`]), for
    /// when it's switched back.
    pub(crate) flip: bool,
    /// The flip the feature edited was stored with while its extent
    /// ignores it, as old files may have it: written back as it was
    /// while the extent set up ignores it too, so an edit changing
    /// nothing writes nothing. False otherwise.
    ignored_flip: bool,
}

impl SideFields {
    /// The fields `fields`, not flipped.
    pub(crate) fn new(fields: [TypedText; 2]) -> Self {
        Self {
            fields,
            flip: false,
            ignored_flip: false,
        }
    }

    /// Takes an edited feature's: its `values` (each side's it has),
    /// stored values asked for by `ask`, and `flip`, its extent taking
    /// it if `flips`.
    pub(crate) fn open(&mut self, values: [Option<&Value>; 2], flip: bool, flips: bool, ask: &Ask) {
        for (field, value) in self.fields.iter_mut().zip(values) {
            if let Some(value) = value {
                *field = TypedText::of(value, ask);
            }
        }
        self.flip = flip;
        self.ignored_flip = flip && !flips;
    }

    /// The flip to store: the one set up where the extent takes it
    /// (`flips`), else the one stored before if that extent ignored it
    /// too ([`SideFields::ignored_flip`]), else none, so a flip left set
    /// from another extent isn't stored where it changes nothing.
    pub(crate) fn stored_flip(&self, flips: bool) -> bool {
        if flips { self.flip } else { self.ignored_flip }
    }

    /// The value `side`'s field last gave.
    pub(crate) fn value(&self, side: Side) -> Option<Value> {
        self.fields[side.index()].value.clone()
    }

    /// Reads `text` into `side`'s field, for `ask`.
    pub(crate) fn input(&mut self, side: Side, text: String, ask: &Ask) {
        self.fields[side.index()].input(text, ask);
    }

    /// Whether none of the fields of `sides` is refused.
    pub(crate) fn typed(&self, sides: Sides) -> bool {
        (sides.fields().iter()).all(|side| self.fields[side.index()].error.is_none())
    }

    /// Follows the design changing from the one `before` asks in to the
    /// one `now` does ([`TypedText::follow`]).
    pub(crate) fn follow(&mut self, before: &Ask, now: &Ask) {
        for field in &mut self.fields {
            field.follow(before, now);
        }
    }
}

/// Moves the knob of `side`, of an extent spending its sides as `sides`,
/// to `to` from the sketch plane (millimetres along its normal, or
/// radians about the axis), typing it into the field of `session`'s
/// [`SideFields`] (`fields`) asked for by `ask`, in `unit`. One side's
/// value is how far either way, past the plane flipping it; symmetric's
/// twice it; two sides' each how far its own way. A knob on the plane
/// changes nothing; one past what the field takes stands at its least or
/// most. Nothing changes where the field still refuses the value, or
/// where the feature's own check (`refused`) would refuse it and didn't
/// before (two sides together over the limit): the knob stops there.
/// Already over the limit, refused `too_far`, it only moves back towards
/// it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drag_side<S, E: PartialEq>(
    session: &mut S,
    fields: fn(&mut S) -> &mut SideFields,
    sides: Sides,
    side: Side,
    to: f64,
    (ask, unit): (&Ask, Unit),
    refused: impl Fn(&S) -> Option<E>,
    too_far: E,
) {
    let (value, flip) = match (sides, side) {
        (Sides::One, Side::First) => (to.abs(), Some(to < 0.0)),
        (Sides::Symmetric, Side::First) => (2.0 * to.abs(), None),
        (Sides::Two, _) => (to.abs(), None),
        _ => return,
    };
    if !(value > 0.0 && value.is_finite()) {
        return;
    }
    // Past what the field takes, as near as it goes.
    let value = value.min(ask.max).max(ask.min.unwrap_or(0.0));
    let text = varde_expr::format(value, Some(unit));
    let mut field = fields(session).fields[side.index()].clone();
    field.input(text, ask);
    if field.error.is_some() {
        return;
    }
    let before = refused(session);
    let own = fields(session);
    let old = std::mem::replace(&mut own.fields[side.index()], field);
    let old_flip = own.flip;
    if let Some(flip) = flip {
        own.flip = flip;
    }
    let worse = match (before, refused(session)) {
        (_, None) => false,
        (None, Some(_)) => true,
        // Already over the limit (typed so), the knob only goes back
        // towards it.
        (Some(_), Some(after)) => {
            after == too_far && old.value.as_ref().is_none_or(|old| value > old.value)
        }
    };
    if worse {
        let own = fields(session);
        own.fields[side.index()] = old;
        own.flip = old_flip;
    }
}

/// Whether a session setting up a feature from `regions` (an extrude or
/// a revolve), editing `feature` if it's `Some`, stays: what it's about
/// is still there. It ends where the feature edited isn't one `is_kind`
/// takes any more (undone away, say), the document can't be changed
/// (`editable` false), it was replaced whole (`replaced`), or the
/// regions' sketches can't be found again ([`RegionPick::refresh`],
/// which finds them again if they changed).
pub(crate) fn regions_session_stays(
    document: &Document,
    feature: Option<FeatureId>,
    regions: &mut RegionPick,
    editable: bool,
    replaced: bool,
    is_kind: fn(&FeatureKind) -> bool,
) -> bool {
    let edited = feature.is_none_or(|feature| {
        (document.feature(feature)).is_some_and(|found| is_kind(&found.kind))
    });
    editable && !replaced && edited && regions.refresh(document)
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
        let at = self.editor.document().insert_at();
        self.apply(command);
        if self.edit_error.is_some() {
            return false;
        }
        if edited.is_none() && self.editor.revision() != before {
            self.selected_feature = self.editor.document().features().get(at).map(|f| f.id);
        }
        true
    }

    /// Commits `built`, the operation being set up as a feature and the
    /// feature it edits, if `ready` and [`Doc::commit_by`] lets it,
    /// ending the session once the document takes it: whether it did.
    pub(crate) fn commit_built(
        &mut self,
        ready: bool,
        accept: bool,
        built: Option<(Option<FeatureId>, FeatureKind)>,
    ) -> bool {
        if !self.commit_by(ready, accept) {
            return false;
        }
        let Some((edited, kind)) = built else {
            return false;
        };
        let done = self.commit_feature(edited, kind);
        if done {
            self.mode.take_operation();
        }
        done
    }

    /// Whether a session ready to be committed (`ready`) is to be by OK
    /// and `Enter` (`accept` false), which wait while the preview failed,
    /// or by Accept error (`accept`), which takes only a failed one, to
    /// be fixed later. The error is the newest draft's, none while it's
    /// unanswered: OK doesn't wait on the preview, but for one hidden
    /// ([`Doc::operation_before`]).
    pub(crate) fn commit_by(&self, ready: bool, accept: bool) -> bool {
        // With the preview hidden a changed draft isn't answered till
        // it shows again, and a blend's body before it is another draft,
        // its own error unknown: until then, neither.
        let unanswered =
            (self.operation_before() && !self.feed.draft_answered()) || self.blend_before();
        ready && !unanswered && self.feed.draft_error().is_some() == accept
    }

    /// Whether the panel offers Add anyway: as [`Doc::commit_by`] takes
    /// it, but with the error the panel shows, held a moment while a
    /// changed draft is on its way, so the button doesn't blink with it.
    /// Pressed before the answer, it does nothing.
    pub(crate) fn offers_accept(&self, ready: bool) -> bool {
        ready && self.feed.shown_draft_error().is_some()
    }

    /// How the operation set up editing `feature` (none for a new one)
    /// stands, for its panel: ready to be committed by its own measure
    /// (`ready`), refused by its own check (`refused`), and warning of
    /// `warning` where nothing else is said.
    pub(crate) fn verdict(
        &self,
        feature: Option<FeatureId>,
        ready: bool,
        refused: Option<String>,
        warning: Option<String>,
    ) -> Verdict<'_> {
        Verdict {
            editing: (feature.and_then(|feature| self.editor.document().feature(feature)))
                .map(|feature| feature.name.as_str()),
            refused,
            error: self.feed.shown_draft_error(),
            show_error: self.draft_framed(),
            warning,
            checking: self.proposals.slow(),
            ready: self.commit_by(ready, false),
            accept: self.offers_accept(ready),
        }
    }

    /// The note saying which bodies the cut set up takes nothing from
    /// (it only touches them, face to face, say), as the UI mock's
    /// "Body 2: nothing to cut", once its preview says so.
    pub(crate) fn uncut_note(&self) -> Option<String> {
        let document = self.editor.document();
        let names: Vec<&str> = (self.feed.draft_uncut().iter())
            .filter_map(|&body| document.body(body))
            .map(|body| body.name.as_str())
            .collect();
        (!names.is_empty()).then(|| format!("{}: nothing to cut", names.join(", ")))
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
