//! Setting up an extrude: its session, started by the Extrude tool or by
//! editing an extrude, picking regions, its distances typed or dragged
//! with the handle, the preview through the regeneration lane's drafts,
//! and committing it as one undo step or cancelling it, which leaves no
//! trace.

use varde_document::{
    Design, Document, Extent, Extrude, ExtrudeError, FeatureId, FeatureKind, MAX_EXTRUDE_REGIONS,
};
use varde_expr::{AngleUnit, LengthUnit, Unit};
use varde_render::Camera;
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
    /// Whether one side goes the other way, or two sides swap: kept
    /// while the extent ignores it (see [`ExtrudeSession::stored_flip`]), for when
    /// it's switched back.
    pub(crate) flip: bool,
    /// The flip the extrude edited was stored with while its extent ignores
    /// it, as old files may have it: written back as it was while the
    /// extent set up ignores it too, so an edit changing nothing writes
    /// nothing. False otherwise.
    ignored_flip: bool,
    pub(crate) operation: OperationKind,
    /// The taper's field: an angle, 0° (stored as none) to begin with
    /// or the edited extrude's.
    pub(crate) taper: TypedText,
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

/// The part of the view's height a new extrude's distance starts at, at
/// most: see [`default_distance`].
const DEFAULT_SHARE: f64 = 0.25;

/// The distance a new extrude starts with, in millimetres, seen by
/// `camera` in a design of `units`: [`DEFAULT_SHARE`] of the view's
/// height at the target, rounded down to 1, 2 or 5 times a power of ten
/// of `units`, and no less than a thousandth of one. Made as
/// [`snap_step`](varde_view::snap_step) makes its step, so it's the same
/// natively and on the web.
fn default_distance(camera: &Camera, units: LengthUnit) -> f64 {
    let most = f64::from(camera.view_height()) * DEFAULT_SHARE / units.mm();
    // The view's height is positive and within the camera's extent, so
    // the decade is within ±324. Should the logarithm round down across
    // a power of ten, the 10 still finds that power.
    let decade = (varde_sketch::angle::log10(most).floor() as i32).max(-3);
    let distance = [(1, 1), (5, 0), (2, 0), (1, 0)]
        .into_iter()
        .filter_map(|(m, up)| format!("{m}e{}", decade + up).parse::<f64>().ok())
        .find(|&distance| distance <= most)
        .unwrap_or(0.001);
    distance * units.mm()
}

impl ExtrudeSession {
    /// A session setting up a new extrude, in `document`'s units, its
    /// distance one that fits the view of `camera`, taking the regions of
    /// `source` if given, else of the one the first region picked is in.
    fn new(document: &Document, camera: &Camera, source: Option<FeatureId>) -> Self {
        let units = document.units();
        let text = varde_expr::format(default_distance(camera, units), Some(Unit::Length(units)));
        let distance = TypedText::read(text, &Extent::ask(&document.design()));
        let none = varde_expr::format(0.0, Some(Unit::Angle(AngleUnit::Deg)));
        let taper = TypedText::read(none, &Extrude::taper_ask(&document.design()));
        Self {
            feature: None,
            regions: RegionPick::new(source, MAX_EXTRUDE_REGIONS),
            extent: ExtentKind::OneSide,
            fields: [distance.clone(), distance],
            flip: false,
            ignored_flip: false,
            operation: OperationKind::NewBody,
            taper,
            targets: BodyTargets::default(),
            grabbed: None,
            hover: None,
            design: document.design(),
        }
    }

    /// A session editing the extrude `feature` of `document`, with its
    /// values and the regions of its sketch its references find; a
    /// distance it has no value for, one that fits the view of `camera`.
    fn editing(
        document: &Document,
        camera: &Camera,
        feature: FeatureId,
        extrude: &Extrude,
    ) -> Self {
        let mut session = Self::new(document, camera, Some(extrude.sketch));
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
        session.ignored_flip = extrude.flip && !session.extent.flips();
        session.operation = OperationKind::of(&extrude.operation);
        if let Some(taper) = &extrude.taper {
            session.taper = TypedText::of(taper, &Extrude::taper_ask(&document.design()));
        }
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
        self.taper.follow_units(&Extrude::taper_ask(&self.design));
        self.design = design;
    }

    /// The flip to store: the one set up where the extent takes it,
    /// else the one stored before if that extent ignored it too
    /// ([`ExtrudeSession::ignored_flip`]), else none, so a flip left set
    /// from another extent isn't stored where it changes nothing.
    fn stored_flip(&self) -> bool {
        if self.extent.flips() {
            self.flip
        } else {
            self.ignored_flip
        }
    }

    /// The extrude as set up, if it's whole: a source, regions picked, and
    /// the distances its extent takes and the taper, as they last read (a
    /// taper of nothing is none).
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
        let taper = self.taper.value.clone()?;
        Some(Extrude {
            sketch,
            regions: self.regions.references().to_vec(),
            extent,
            flip: self.stored_flip(),
            taper: (taper.value != 0.0).then_some(taper),
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
    /// none of the distances its extent takes nor the taper is refused,
    /// and it passes
    /// its own check ([`ExtrudeSession::refused`]). What's left to the
    /// document are the references to other features and bodies, which
    /// the session keeps valid.
    fn ready(&self, design: &Design) -> bool {
        let typed = self
            .extent
            .distances()
            .iter()
            .all(|distance| self.fields[distance.index()].error.is_none())
            && self.taper.error.is_none();
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
        let mut session = ExtrudeSession::new(document, &self.camera, selected);
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
        self.extrude = Some(ExtrudeSession::editing(document, &self.camera, id, extrude));
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
            ExtrudeLook::Taper(text) => {
                session
                    .taper
                    .input(text, &Extrude::taper_ask(&document.design()));
            }
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
            taper: session.taper.field(),
            operation: session.operation,
            targets: self.body_targets(session.operation, session.feature, &session.targets),
            grabbed: session.grabbed,
            hover: self.panel_hover(),
            error: self.feed.shown_draft_error(),
            show_error: self.draft_framed(),
            refused: session.refused(&document.design()),
            held: self.held(session.feature, session.operation),
            uncut: (session.operation == OperationKind::Cut)
                .then(|| self.uncut_note())
                .flatten(),
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
