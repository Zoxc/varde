//! Setting up an extrude: its session, started by the Extrude tool or by
//! editing an extrude, picking regions, its distances typed or dragged
//! with the handle, the preview through the regeneration lane's drafts,
//! and committing it as one undo step or cancelling it, which leaves no
//! trace.

use std::collections::BTreeSet;
use std::sync::Arc;

use varde_document::{
    BodyId, Command, Design, Document, Extent, Extrude, ExtrudeError, FeatureId, FeatureKind,
    MAX_EXTRUDE_REGIONS, Operation, RegionRef, Sketch, Targets,
};
use varde_expr::{Unit, Value};
use varde_sketch::Profiles;
use varde_view::{
    Candidate, Distance, DistanceField, ExtentKind, ExtrudeLook, ExtrudeState, ExtrudeTarget,
    OperationKind,
};

use super::{Doc, Focus};

/// The extrude being set up, while one is: [`Doc::extrude`].
#[derive(Debug)]
pub(crate) struct ExtrudeSession {
    /// The extrude edited, or `None` for a new one.
    pub(crate) feature: Option<FeatureId>,
    /// The sketch the extrude takes regions of, once there is one.
    pub(crate) source: Option<FeatureId>,
    /// Whether the source stays when no region is picked: one the
    /// session started from, selected or edited, rather than picked.
    fixed: bool,
    /// The profiles of the source, or before there is one of the visible
    /// sketches, those with regions.
    found: Vec<Found>,
    /// The regions picked, by index into the source's profiles.
    pub(crate) picked: BTreeSet<usize>,
    /// The references to them, in the same order, made as they're picked
    /// (and again when the sketch changes). Kept while the source's
    /// regions can't be found, which leaves none picked, to find them
    /// again once they can.
    references: Vec<RegionRef>,
    /// How many of the edited extrude's regions weren't found.
    pub(crate) missing: usize,
    pub(crate) extent: ExtentKind,
    /// The first distance's field, and two sides' second.
    pub(crate) fields: [DistanceText; 2],
    pub(crate) flip: bool,
    pub(crate) operation: OperationKind,
    /// The bodies a join, cut or intersect leaves out, sorted: the
    /// edited extrude's to start with.
    pub(crate) excluded: Vec<BodyId>,
    /// The handle's knob being dragged, if one is.
    pub(crate) grabbed: Option<Distance>,
    /// The design as the fields' texts were last read, whose units bare
    /// numbers in them are in: see [`ExtrudeSession::follow_units`].
    design: Design,
}

/// A sketch's profiles, and the sketch they're of.
#[derive(Debug)]
struct Found {
    feature: FeatureId,
    sketch: Sketch,
    profiles: Arc<Profiles>,
}

/// A distance's field: the text as typed, the value it last gave, and why
/// the text is refused, if it is.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DistanceText {
    pub(crate) text: String,
    pub(crate) value: Option<Value>,
    pub(crate) error: Option<varde_expr::Error>,
}

/// The distance a new extrude starts with, in millimetres.
const DEFAULT_DISTANCE: f64 = 10.0;

impl DistanceText {
    /// The field holding `value`.
    fn of(value: &Value) -> Self {
        Self {
            text: value.text.clone(),
            value: Some(value.clone()),
            error: None,
        }
    }

    /// The field as the view shows it.
    fn field(&self) -> DistanceField<'_> {
        DistanceField {
            text: &self.text,
            error: self.error.as_ref(),
            value: self.value.as_ref().map(|value| value.value),
        }
    }

    /// Reads `text` as a distance of `document`'s: the value it gives,
    /// or why it's refused, keeping the last value.
    fn input(&mut self, text: String, document: &Document) {
        match Value::new(&text, &Extent::ask(&document.design())) {
            Ok(value) => {
                self.value = Some(value);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
        self.text = text;
    }
}

impl ExtrudeSession {
    /// A session setting up a new extrude, in `document`'s units, taking
    /// the regions of `source` if given, else of the one the first region
    /// picked is in.
    fn new(document: &Document, source: Option<FeatureId>) -> Self {
        let mut distance = DistanceText {
            text: String::new(),
            value: None,
            error: None,
        };
        let text = varde_expr::format(DEFAULT_DISTANCE, Some(Unit::Length(document.units())));
        distance.input(text, document);
        Self {
            feature: None,
            source,
            fixed: source.is_some(),
            found: Vec::new(),
            picked: BTreeSet::new(),
            references: Vec::new(),
            missing: 0,
            extent: ExtentKind::OneSide,
            fields: [distance.clone(), distance],
            flip: false,
            operation: OperationKind::NewBody,
            excluded: Vec::new(),
            grabbed: None,
            design: document.design(),
        }
    }

    /// A session editing the extrude `feature` of `document`, with its
    /// values and the regions of its sketch its references find.
    fn editing(document: &Document, feature: FeatureId, extrude: &Extrude) -> Self {
        let mut session = Self::new(document, Some(extrude.sketch));
        session.feature = Some(feature);
        session.refresh(document);
        let found = session
            .found(extrude.sketch)
            .map(|found| found.profiles.resolve(&extrude.regions));
        let resolved = found.unwrap_or_else(|| vec![None; extrude.regions.len()]);
        session.missing = resolved.iter().filter(|index| index.is_none()).count();
        let picked = resolved.into_iter().flatten().collect();
        session.pick(picked);
        let (extent, [first, second]) = match &extrude.extent {
            Extent::OneSide(d) => (ExtentKind::OneSide, [Some(d), None]),
            Extent::Symmetric(d) => (ExtentKind::Symmetric, [Some(d), None]),
            Extent::TwoSides(a, b) => (ExtentKind::TwoSides, [Some(a), Some(b)]),
            Extent::ThroughAll => (ExtentKind::ThroughAll, [None, None]),
        };
        session.extent = extent;
        let first = first.map(DistanceText::of);
        if let Some(first) = &first {
            session.fields = [first.clone(), first.clone()];
        }
        if let Some(second) = second {
            session.fields[1] = DistanceText::of(second);
        }
        session.flip = extrude.flip;
        session.operation = match &extrude.operation {
            Operation::NewBody(_) => OperationKind::NewBody,
            Operation::Join(_) => OperationKind::Join,
            Operation::Cut(_) => OperationKind::Cut,
            Operation::Intersect(_) => OperationKind::Intersect,
        };
        session.excluded = extrude.operation.excluded().to_vec();
        session
    }

    /// The profiles found of `feature`, if it's a candidate.
    fn found(&self, feature: FeatureId) -> Option<&Found> {
        self.found.iter().find(|found| found.feature == feature)
    }

    /// Picks the regions `picked` of the source, making their references.
    /// Those that can't be referenced, too thin for a point inside to be
    /// found, are left out.
    fn pick(&mut self, picked: BTreeSet<usize>) {
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
            .take(MAX_EXTRUDE_REGIONS)
            .collect();
        self.picked = referenced.iter().map(|(index, _)| *index).collect();
        self.references = referenced
            .into_iter()
            .map(|(_, reference)| reference)
            .collect();
    }

    /// Keeps each distance's length where the design's units changed
    /// since its field was read, as the document does its own: the units
    /// the text was read in are written after its bare numbers. A text
    /// that's refused stays as typed, to be read in the new units.
    fn follow_units(&mut self, document: &Document) {
        let design = document.design();
        if design == self.design {
            return;
        }
        let ask = Extent::ask(&self.design);
        for field in &mut self.fields {
            if let Some(value) = &mut field.value {
                value.pin_units(&ask);
                if field.error.is_none() {
                    field.text = value.text.clone();
                }
            }
        }
        self.design = design;
    }

    /// Finds the profiles of the sketches of `document` whose regions can
    /// be picked again where their sketch changed, and those picked in
    /// the source again by their references. False if the source is gone.
    fn refresh(&mut self, document: &Document) -> bool {
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
            match kept {
                Some(at) if old[at].sketch == *sketch => self.found.push(old.swap_remove(at)),
                _ => {
                    remap |= Some(id) == self.source;
                    // A sketch too complex for its regions to be found has
                    // none to pick.
                    if let Ok(profiles) = sketch.profiles()
                        && (Some(id) == self.source || !profiles.regions.is_empty())
                    {
                        self.found.push(Found {
                            feature: id,
                            sketch: sketch.clone(),
                            profiles: Arc::new(profiles),
                        });
                    }
                }
            }
        }
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

    /// The extrude as set up, if it's whole: a source, regions picked, and
    /// the distances its extent takes, as they last read.
    fn extrude(&self) -> Option<Extrude> {
        let sketch = self.source?;
        if self.picked.is_empty() {
            return None;
        }
        let value = |distance: Distance| self.fields[distance.index()].value.clone();
        let extent = match self.extent {
            ExtentKind::OneSide => Extent::OneSide(value(Distance::First)?),
            ExtentKind::Symmetric => Extent::Symmetric(value(Distance::First)?),
            ExtentKind::TwoSides => {
                Extent::TwoSides(value(Distance::First)?, value(Distance::Second)?)
            }
            ExtentKind::ThroughAll => Extent::ThroughAll,
        };
        let targets = Targets {
            excluded: self.excluded.clone(),
        };
        let operation = match self.operation {
            OperationKind::NewBody => Operation::NewBody(BodyId::NEW),
            OperationKind::Join => Operation::Join(targets),
            OperationKind::Cut => Operation::Cut(targets),
            OperationKind::Intersect => Operation::Intersect(targets),
        };
        Some(Extrude {
            sketch,
            regions: self.references.clone(),
            extent,
            flip: self.flip,
            operation,
        })
    }

    /// Why the extrude as set up can't be committed to a document of
    /// `design`, if its own check refuses it: two sides over
    /// [`MAX_COORD`](varde_document::MAX_COORD) together, say. None
    /// while it isn't whole.
    fn refused(&self, design: &Design) -> Option<ExtrudeError> {
        self.extrude()?.check_own(design).err()
    }

    /// Whether it can be committed to a document of `design`: it's whole,
    /// none of the distances its extent takes is refused, and it passes
    /// its own check ([`ExtrudeSession::refused`]). What's left to the
    /// document are the references to other features and bodies, which
    /// the session keeps valid.
    fn ready(&self, design: &Design) -> bool {
        let typed = self
            .extent
            .distances()
            .iter()
            .all(|distance| self.fields[distance.index()].error.is_none());
        typed
            && self
                .extrude()
                .is_some_and(|extrude| extrude.check_own(design).is_ok())
    }

    /// Takes `body` out of the join, cut or intersect, or puts it back,
    /// if it's one of `document`'s made before the extrude edited.
    fn toggle_target(&mut self, body: BodyId, document: &Document) {
        match self.excluded.binary_search(&body) {
            Ok(at) => {
                self.excluded.remove(at);
            }
            Err(at) => {
                let made_before = document.body(body).is_some_and(|made| {
                    let maker = document
                        .features()
                        .iter()
                        .position(|f| f.id == made.created_by);
                    let edited = self.feature.and_then(|feature| {
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

    /// Moves the knob of `distance` to `to`, in millimetres along the
    /// sketch plane's normal: one side's goes past the plane by flipping.
    /// A knob on the plane changes nothing, and so does one where the
    /// field refuses the distance, or where the extrude's own check would
    /// refuse it and didn't before (two sides together over the limit):
    /// the knob stops there. Already over the limit, it only moves back
    /// towards it.
    fn drag(&mut self, distance: Distance, to: f64, document: &Document) {
        let (length, flip) = match (self.extent, distance) {
            (ExtentKind::OneSide, Distance::First) => (to.abs(), Some(to < 0.0)),
            (ExtentKind::Symmetric, Distance::First) => (2.0 * to.abs(), None),
            (ExtentKind::TwoSides, _) => (to.abs(), None),
            _ => return,
        };
        if !(length > 0.0 && length.is_finite()) {
            return;
        }
        let text = varde_expr::format(length, Some(Unit::Length(document.units())));
        let mut field = self.fields[distance.index()].clone();
        field.input(text, document);
        if field.error.is_some() {
            return;
        }
        let design = document.design();
        let before = self.refused(&design);
        let old = std::mem::replace(&mut self.fields[distance.index()], field);
        let old_flip = self.flip;
        if let Some(flip) = flip {
            self.flip = flip;
        }
        let worse = match (before, self.refused(&design)) {
            (_, None) => false,
            (None, Some(_)) => true,
            // Already over the limit (typed so), the knob only goes back
            // towards it.
            (Some(_), Some(after)) => {
                after == ExtrudeError::Length
                    && old.value.as_ref().is_none_or(|old| length > old.value)
            }
        };
        if worse {
            self.fields[distance.index()] = old;
            self.flip = old_flip;
        }
    }
}

impl Doc {
    /// Starts setting up a new extrude, in a document that can be changed
    /// and outside a sketch, taking the regions of the sketch selected in
    /// the Timeline if one is; or cancels the one being set up. The first
    /// distance's field takes the focus.
    pub(crate) fn start_extrude(&mut self) {
        if self.extrude.take().is_some() || !self.editable() || self.sketch.is_some() {
            return;
        }
        self.picking_plane = false;
        let document = self.editor.document();
        let selected = self.selected_feature.filter(|&id| is_sketch(document, id));
        let mut session = ExtrudeSession::new(document, selected);
        session.refresh(document);
        self.extrude = Some(session);
        self.focus = Some(Focus::All);
    }

    /// Edits the extrude feature `id`, if the document holds it, in a
    /// session with its values, outside a sketch, in a document that can
    /// be changed: a read-only one has no session.
    pub(crate) fn edit_extrude(&mut self, id: FeatureId) {
        let document = self.editor.document();
        let Some(FeatureKind::Extrude(extrude)) = document.feature(id).map(|f| &f.kind) else {
            return;
        };
        if self.sketch.is_some() || !self.editable() {
            return;
        }
        self.picking_plane = false;
        self.selected_feature = Some(id);
        self.extrude = Some(ExtrudeSession::editing(document, id, extrude));
        self.focus = Some(Focus::All);
    }

    /// Takes `message`, changing the extrude being set up.
    pub(crate) fn extrude_look(&mut self, message: ExtrudeLook) {
        let editable = self.editable();
        let document = self.editor.document();
        let Some(session) = &mut self.extrude else {
            return;
        };
        match message {
            ExtrudeLook::Cancel => self.extrude = None,
            _ if !editable => {}
            ExtrudeLook::PickRegion { sketch, region } => {
                if session.source.is_some_and(|source| source != sketch) {
                    return;
                }
                let Some(found) = session.found(sketch) else {
                    return;
                };
                if region >= found.profiles.regions.len() {
                    return;
                }
                session.source = Some(sketch);
                let mut picked = session.picked.clone();
                if !picked.remove(&region) {
                    picked.insert(region);
                }
                session.pick(picked);
                // All taken out again, another sketch's may be picked,
                // unless the session started from this one.
                if session.picked.is_empty() && !session.fixed {
                    session.source = None;
                }
                session.refresh(document);
            }
            ExtrudeLook::Extent(kind) => {
                // Only a cut goes through all.
                if kind != ExtentKind::ThroughAll || session.operation == OperationKind::Cut {
                    session.extent = kind;
                }
            }
            ExtrudeLook::Input { distance, text } => {
                session.fields[distance.index()].input(text, document);
            }
            ExtrudeLook::Flip => session.flip = !session.flip,
            ExtrudeLook::Operation(kind) => {
                session.operation = kind;
                if kind != OperationKind::Cut && session.extent == ExtentKind::ThroughAll {
                    session.extent = ExtentKind::OneSide;
                }
            }
            ExtrudeLook::Target(body) => session.toggle_target(body, document),
            ExtrudeLook::GrabHandle(distance) => session.grabbed = Some(distance),
            ExtrudeLook::DragHandle { distance, to } => {
                if session.grabbed == Some(distance) {
                    session.drag(distance, to, document);
                }
            }
            ExtrudeLook::DropHandle => session.grabbed = None,
        }
    }

    /// Whether the extrude being set up can be committed: the document
    /// can be changed, no sketch edits wait on the solver, and the session
    /// is ready ([`ExtrudeSession::ready`]). Edits left with the solver
    /// are committed after it answers, and may change the regions picked,
    /// which are found again then: until they are, the preview isn't of
    /// what would be committed, and the extrude would come before them in
    /// the undo history.
    pub(crate) fn extrude_ready(&self) -> bool {
        self.extrude.as_ref().is_some_and(|session| {
            self.editable() && !self.proposing() && session.ready(&self.editor.document().design())
        })
    }

    /// Adds the extrude being set up, or changes the one edited, as one
    /// undo step, and ends the session: if it's ready
    /// ([`Doc::extrude_ready`]: the edits left with the solver answered),
    /// and the document takes it. Refused, the session stays, and why
    /// shows.
    pub(crate) fn commit_extrude(&mut self) {
        if !self.extrude_ready() {
            return;
        }
        let Some(session) = &self.extrude else {
            return;
        };
        let Some(extrude) = session.extrude() else {
            return;
        };
        let feature = session.feature;
        let command = match feature {
            Some(feature) => Command::SetExtrude {
                feature,
                extrude: Box::new(extrude),
            },
            None => self.editor.document().add_extrude(extrude),
        };
        let before = self.editor.revision();
        self.apply(command);
        if self.edit_error.is_some() {
            return;
        }
        let added = feature.is_none() && self.editor.revision() != before;
        if added {
            // New features get the highest id, so it's the last.
            self.selected_feature = self.editor.document().features().last().map(|f| f.id);
        }
        self.extrude = None;
    }

    /// Ends the extrude session if what it's about is gone, e.g. by undo,
    /// the document can't be changed any more, or the document was
    /// replaced whole, `replaced` (restoring recovered changes, or undoing
    /// or redoing that), when its ids may name other things; and finds its sketches'
    /// regions again if they changed.
    ///
    /// Unlike the sketch session, which only resets across a replacement
    /// and reads the sketch its id names now, an extrude session holds
    /// values read before it, which OK would write over whatever extrude
    /// the id names now.
    pub(crate) fn prune_extrude(&mut self, replaced: bool) {
        let editable = self.editable();
        let document = self.editor.document();
        let Some(session) = &mut self.extrude else {
            return;
        };
        let edited = session.feature.is_none_or(|feature| {
            matches!(
                document.feature(feature).map(|f| &f.kind),
                Some(FeatureKind::Extrude(_))
            )
        });
        if !(editable && !replaced && edited && session.refresh(document)) {
            self.extrude = None;
            return;
        }
        session.follow_units(document);
        // Bodies gone (by undo, say) can't be taken out.
        session
            .excluded
            .retain(|&body| document.body(body).is_some());
    }

    /// The extrude being set up as the regeneration lane previews it, and
    /// the extrude it edits, if it's whole.
    pub(crate) fn draft(&self) -> Option<(Option<FeatureId>, Extrude)> {
        let session = self.extrude.as_ref()?;
        Some((session.feature, session.extrude()?))
    }

    /// Whether there's a sketch to extrude regions of: a visible one, or
    /// the one selected in the Timeline. Whether it has regions is found
    /// once the session starts.
    pub(crate) fn extrudable(&self) -> bool {
        let document = self.editor.document();
        document.features().iter().any(|feature| {
            matches!(feature.kind, FeatureKind::Sketch { .. })
                && (feature.visible || self.selected_feature == Some(feature.id))
        })
    }

    /// The extrude being set up, for the view.
    pub(crate) fn extrude_state(&self) -> Option<ExtrudeState<'_>> {
        let session = self.extrude.as_ref()?;
        let document = self.editor.document();
        let candidates = session
            .found
            .iter()
            .filter_map(|found| {
                let FeatureKind::Sketch { plane, .. } = &document.feature(found.feature)?.kind
                else {
                    return None;
                };
                Some(Candidate {
                    feature: found.feature,
                    plane: *plane,
                    profiles: &found.profiles,
                })
            })
            .collect();
        let editing = session
            .feature
            .and_then(|feature| document.feature(feature))
            .map(|feature| feature.name.as_str());
        Some(ExtrudeState {
            editing,
            candidates,
            source: session.source,
            picked: &session.picked,
            missing: session.missing,
            extent: session.extent,
            fields: [session.fields[0].field(), session.fields[1].field()],
            flip: session.flip,
            operation: session.operation,
            targets: self.extrude_targets(session),
            grabbed: session.grabbed,
            error: self.feed.draft_error(),
            refused: session.refused(&document.design()),
            checking: self.proposals.slow(),
            ready: self.extrude_ready(),
            editable: self.editable(),
            units: document.units(),
        })
    }
}

impl Doc {
    /// The bodies the session's join, cut or intersect lists: those its
    /// preview touches and those taken out, in the order they were made.
    fn extrude_targets(&self, session: &ExtrudeSession) -> Vec<ExtrudeTarget<'_>> {
        if !session.operation.has_targets() {
            return Vec::new();
        }
        let touched = self.feed.draft_touched();
        (self.editor.document().bodies().iter())
            .filter(|body| touched.contains(&body.id) || session.excluded.contains(&body.id))
            .map(|body| ExtrudeTarget {
                body: body.id,
                name: &body.name,
                included: session.excluded.binary_search(&body.id).is_err(),
            })
            .collect()
    }
}

/// Whether `id` is a sketch feature of `document`.
pub(super) fn is_sketch(document: &Document, id: FeatureId) -> bool {
    matches!(
        document.feature(id).map(|feature| &feature.kind),
        Some(FeatureKind::Sketch { .. })
    )
}

#[cfg(test)]
mod tests;
