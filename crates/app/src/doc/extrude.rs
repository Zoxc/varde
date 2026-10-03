//! Setting up an extrude: its session, started by the Extrude tool or by
//! editing an extrude, picking regions, its distances typed or dragged
//! with the handle, the preview through the regeneration lane's drafts,
//! and committing it as one undo step or cancelling it, which leaves no
//! trace.

use varde_document::{
    Design, Document, Extent, Extrude, ExtrudeError, FeatureId, FeatureKind, MAX_EXTRUDE_REGIONS,
};
use varde_expr::Unit;
use varde_view::{Distance, ExtentKind, ExtrudeLook, ExtrudeState, OperationKind, PanelHover};

use super::regions::{BodyTargets, RegionPick, TypedText};
use super::{Doc, Focus};

/// The extrude being set up, while one is: [`Doc::extrude`].
#[derive(Debug)]
pub(crate) struct ExtrudeSession {
    /// The extrude edited, or `None` for a new one.
    pub(crate) feature: Option<FeatureId>,
    /// The regions picked, and the sketch they're of.
    pub(crate) regions: RegionPick,
    pub(crate) extent: ExtentKind,
    /// The first distance's field, and two sides' second.
    pub(crate) fields: [TypedText; 2],
    pub(crate) flip: bool,
    pub(crate) operation: OperationKind,
    /// The bodies a join, cut or intersect leaves out: the edited
    /// extrude's to start with.
    pub(crate) targets: BodyTargets,
    /// The handle's knob being dragged, if one is.
    pub(crate) grabbed: Option<Distance>,
    /// The panel's row the cursor is over, if any.
    pub(crate) hover: Option<PanelHover>,
    /// The design as the fields' texts were last read, whose units bare
    /// numbers in them are in: see [`ExtrudeSession::follow_units`].
    design: Design,
}

/// The distance a new extrude starts with, in millimetres.
const DEFAULT_DISTANCE: f64 = 10.0;

impl ExtrudeSession {
    /// A session setting up a new extrude, in `document`'s units, taking
    /// the regions of `source` if given, else of the one the first region
    /// picked is in.
    fn new(document: &Document, source: Option<FeatureId>) -> Self {
        let text = varde_expr::format(DEFAULT_DISTANCE, Some(Unit::Length(document.units())));
        let distance = TypedText::read(text, &Extent::ask(&document.design()));
        Self {
            feature: None,
            regions: RegionPick::new(source, MAX_EXTRUDE_REGIONS),
            extent: ExtentKind::OneSide,
            fields: [distance.clone(), distance],
            flip: false,
            operation: OperationKind::NewBody,
            targets: BodyTargets::default(),
            grabbed: None,
            hover: None,
            design: document.design(),
        }
    }

    /// A session editing the extrude `feature` of `document`, with its
    /// values and the regions of its sketch its references find.
    fn editing(document: &Document, feature: FeatureId, extrude: &Extrude) -> Self {
        let mut session = Self::new(document, Some(extrude.sketch));
        session.feature = Some(feature);
        session.regions = RegionPick::editing(
            document,
            extrude.sketch,
            &extrude.regions,
            MAX_EXTRUDE_REGIONS,
        );
        let (extent, [first, second]) = match &extrude.extent {
            Extent::OneSide(d) => (ExtentKind::OneSide, [Some(d), None]),
            Extent::Symmetric(d) => (ExtentKind::Symmetric, [Some(d), None]),
            Extent::TwoSides(a, b) => (ExtentKind::TwoSides, [Some(a), Some(b)]),
            Extent::ThroughAll => (ExtentKind::ThroughAll, [None, None]),
        };
        session.extent = extent;
        let ask = Extent::ask(&document.design());
        let first = first.map(|first| TypedText::of(first, &ask));
        if let Some(first) = &first {
            session.fields = [first.clone(), first.clone()];
        }
        if let Some(second) = second {
            session.fields[1] = TypedText::of(second, &ask);
        }
        session.flip = extrude.flip;
        session.operation = OperationKind::of(&extrude.operation);
        session.targets = BodyTargets::new(extrude.operation.excluded());
        session
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
            field.follow_units(&ask);
        }
        self.design = design;
    }

    /// The extrude as set up, if it's whole: a source, regions picked, and
    /// the distances its extent takes, as they last read.
    fn extrude(&self) -> Option<Extrude> {
        let sketch = self.regions.source?;
        if self.regions.picked.is_empty() {
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
        Some(Extrude {
            sketch,
            regions: self.regions.references().to_vec(),
            extent,
            flip: self.flip,
            operation: self.targets.operation(self.operation),
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
        field.input(text, &Extent::ask(&document.design()));
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
    /// the Timeline if one is; or cancels the one being set up. A revolve
    /// or combine being set up is dropped. The first
    /// distance's field takes the focus.
    pub(crate) fn start_extrude(&mut self) {
        if self.extrude.take().is_some() || !self.editable() || self.sketch.is_some() {
            return;
        }
        self.picking_plane = None;
        self.revolve = None;
        self.combine = None;
        self.motion = None;
        let document = self.editor.document();
        let selected = self.selected_feature.filter(|&id| is_sketch(document, id));
        let mut session = ExtrudeSession::new(document, selected);
        session.regions.refresh(document);
        self.extrude = Some(session);
        self.focus = Some(Focus::All);
    }

    /// Edits the extrude feature `id`, if the document holds it, in a
    /// session with its values, outside a sketch, in a document that can
    /// be changed: a read-only one has no session. A revolve or combine
    /// being set up is dropped.
    pub(crate) fn edit_extrude(&mut self, id: FeatureId) {
        let document = self.editor.document();
        let Some(FeatureKind::Extrude(extrude)) = document.feature(id).map(|f| &f.kind) else {
            return;
        };
        if self.sketch.is_some() || !self.editable() {
            return;
        }
        self.picking_plane = None;
        self.revolve = None;
        self.combine = None;
        self.motion = None;
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
                session.regions.toggle(sketch, region, false, document);
            }
            ExtrudeLook::Extent(kind) => {
                // Only a cut goes through all.
                if kind != ExtentKind::ThroughAll || session.operation == OperationKind::Cut {
                    session.extent = kind;
                }
            }
            ExtrudeLook::Input { distance, text } => {
                let ask = Extent::ask(&document.design());
                session.fields[distance.index()].input(text, &ask);
            }
            ExtrudeLook::Flip => session.flip = !session.flip,
            ExtrudeLook::Operation(kind) => {
                session.operation = kind;
                if kind != OperationKind::Cut && session.extent == ExtentKind::ThroughAll {
                    session.extent = ExtentKind::OneSide;
                }
            }
            ExtrudeLook::Target(body) => {
                let revision = self.feed.revision();
                (session.targets).toggle(body, session.feature, document, revision);
            }
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
            self.editable()
                && !self.proposing()
                && session.ready(&self.editor.document().design())
                && self.held(session.feature, session.operation).is_none()
        })
    }

    /// Adds the extrude being set up, or changes the one edited, as one
    /// undo step, and ends the session: if it's ready
    /// ([`Doc::extrude_ready`]: the edits left with the solver answered),
    /// its preview failed only if `accept` ([`Doc::commit_by`]), and the
    /// document takes it. Refused, the session stays, and why
    /// shows.
    pub(crate) fn commit_extrude(&mut self, accept: bool) {
        if !self.commit_by(self.extrude_ready(), accept) {
            return;
        }
        let Some(session) = &self.extrude else {
            return;
        };
        let Some(extrude) = session.extrude() else {
            return;
        };
        if self.commit_feature(session.feature, extrude.into()) {
            self.extrude = None;
        }
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
        if !(editable && !replaced && edited && session.regions.refresh(document)) {
            self.extrude = None;
            return;
        }
        session.follow_units(document);
        session.targets.prune(document);
    }

    /// The extrude being set up as the regeneration lane previews it, and
    /// the extrude it edits, if it's whole.
    pub(crate) fn extrude_draft(&self) -> Option<(Option<FeatureId>, FeatureKind)> {
        let session = self.extrude.as_ref()?;
        Some((session.feature, session.extrude()?.into()))
    }

    /// The extrude being set up, for the view.
    pub(crate) fn extrude_state(&self) -> Option<ExtrudeState<'_>> {
        let session = self.extrude.as_ref()?;
        let document = self.editor.document();
        let candidates = session.regions.candidates(|id| self.placement(id));
        let editing = session
            .feature
            .and_then(|feature| document.feature(feature))
            .map(|feature| feature.name.as_str());
        Some(ExtrudeState {
            editing,
            candidates,
            source: session.regions.source,
            picked: &session.regions.picked,
            missing: session.regions.missing,
            extent: session.extent,
            fields: [session.fields[0].field(), session.fields[1].field()],
            flip: session.flip,
            operation: session.operation,
            targets: self.body_targets(session.operation, session.feature, &session.targets),
            grabbed: session.grabbed,
            hover: self.panel_hover(),
            error: self.feed.draft_error(),
            show_error: self.draft_framed(),
            refused: session.refused(&document.design()),
            held: self.held(session.feature, session.operation),
            checking: self.proposals.slow(),
            ready: self.commit_by(self.extrude_ready(), false),
            accept: self.commit_by(self.extrude_ready(), true),
            editable: self.editable(),
            units: document.units(),
        })
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
