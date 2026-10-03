//! Setting up a revolve: its session, started by `Look::StartRevolve` or
//! by editing a revolve, picking regions and the axis (a line of the
//! source sketch or one of its axes), its angles typed, the preview
//! through the regeneration lane's drafts, and committing it as one undo
//! step or cancelling it, which leaves no trace. Picking regions, the
//! Bodies list and the typed fields are the extrude's
//! ([`super::regions`]).

use std::f64::consts::PI;

use varde_document::{
    AxisLine, Design, Document, FeatureId, FeatureKind, MAX_REVOLVE_REGIONS, Revolve, RevolveError,
    Turn,
};
use varde_expr::{AngleUnit, Unit};
use varde_view::{
    Angle, OperationKind, PanelHover, RevolveLook, RevolvePick, RevolveState, TurnKind,
};

use super::extrude::is_sketch;
use super::regions::{BodyTargets, RegionPick, TypedText};
use super::{Doc, Focus};

/// The revolve being set up, while one is: [`Doc::revolve`].
#[derive(Debug)]
pub(crate) struct RevolveSession {
    /// The revolve edited, or `None` for a new one.
    pub(crate) feature: Option<FeatureId>,
    /// The regions picked, and the sketch they're of.
    pub(crate) regions: RegionPick,
    /// The axis picked, of the source. Kept while the source doesn't
    /// have it (its line undone away, say), which leaves the revolve not
    /// whole, to be found again once it does.
    pub(crate) axis: Option<AxisLine>,
    /// Whether the edited revolve's axis line wasn't found: until another
    /// is picked.
    pub(crate) axis_missing: bool,
    /// What a click picks first.
    pub(crate) picking: RevolvePick,
    pub(crate) extent: TurnKind,
    /// The first angle's field, and two sides' second.
    pub(crate) fields: [TypedText; 2],
    pub(crate) flip: bool,
    pub(crate) operation: OperationKind,
    /// The bodies a join, cut or intersect leaves out: the edited
    /// revolve's to start with.
    pub(crate) targets: BodyTargets,
    /// The design as the fields' texts were last read, whose units bare
    /// lengths in them are in: see [`RevolveSession::follow_units`].
    /// The panel's row the cursor is over, if any.
    pub(crate) hover: Option<PanelHover>,
    design: Design,
}

/// The angles a new revolve's fields start with, in radians: half a turn
/// for one side and symmetric, and a quarter for two sides' second.
const DEFAULT_ANGLES: [f64; 2] = [PI, PI / 2.0];

impl RevolveSession {
    /// A session setting up a new revolve in `document`, taking the
    /// regions of `source` if given, else of the one the first region (or
    /// the axis) is picked in.
    fn new(document: &Document, source: Option<FeatureId>) -> Self {
        let ask = Turn::ask(&document.design());
        let field = |angle: f64| {
            TypedText::read(
                varde_expr::format(angle, Some(Unit::Angle(AngleUnit::Deg))),
                &ask,
            )
        };
        Self {
            feature: None,
            regions: RegionPick::new(source, MAX_REVOLVE_REGIONS),
            hover: None,
            axis: None,
            axis_missing: false,
            picking: RevolvePick::Regions,
            extent: TurnKind::Full,
            fields: DEFAULT_ANGLES.map(field),
            flip: false,
            operation: OperationKind::NewBody,
            targets: BodyTargets::default(),
            design: document.design(),
        }
    }

    /// A session editing the revolve `feature` of `document`, with its
    /// values, the regions of its sketch its references find, and its
    /// axis if its sketch has it.
    fn editing(document: &Document, feature: FeatureId, revolve: &Revolve) -> Self {
        let mut session = Self::new(document, Some(revolve.sketch));
        session.feature = Some(feature);
        session.regions = RegionPick::editing(
            document,
            revolve.sketch,
            &revolve.regions,
            MAX_REVOLVE_REGIONS,
        );
        let found = sketch_of(document, revolve.sketch)
            .is_some_and(|sketch| revolve.check_axis(sketch).is_ok());
        session.axis = found.then_some(revolve.axis);
        session.axis_missing = !found;
        let ask = Turn::ask(&document.design());
        let (extent, [first, second]) = match &revolve.extent {
            Turn::Full => (TurnKind::Full, [None, None]),
            Turn::OneSide(a) => (TurnKind::OneSide, [Some(a), None]),
            Turn::Symmetric(a) => (TurnKind::Symmetric, [Some(a), None]),
            Turn::TwoSides(a, b) => (TurnKind::TwoSides, [Some(a), Some(b)]),
        };
        session.extent = extent;
        if let Some(first) = first {
            session.fields[0] = TypedText::of(first, &ask);
        }
        if let Some(second) = second {
            session.fields[1] = TypedText::of(second, &ask);
        }
        session.flip = revolve.flip;
        session.operation = OperationKind::of(&revolve.operation);
        session.targets = BodyTargets::new(revolve.operation.excluded());
        session
    }

    /// The axis picked, if the source has it as `document` is: a line of
    /// it, or one of its axes.
    fn axis(&self, document: &Document) -> Option<AxisLine> {
        let axis = self.axis?;
        let sketch = sketch_of(document, self.regions.source?)?;
        axis_holds(sketch, axis).then_some(axis)
    }

    /// Picks `axis` of the sketch `sketch`, if it's the source or a
    /// candidate and there's none yet, which it becomes, and `axis` is a
    /// line of it or one of its axes. Clicks pick regions again then.
    fn pick_axis(&mut self, sketch: FeatureId, axis: AxisLine, document: &Document) {
        let holds = sketch_of(document, sketch).is_some_and(|drawn| axis_holds(drawn, axis));
        if !holds || !self.regions.choose(sketch) {
            return;
        }
        self.axis = Some(axis);
        self.axis_missing = false;
        self.picking = RevolvePick::Regions;
        self.regions.refresh(document);
    }

    /// Keeps each angle where the design's units changed since its field
    /// was read, as the document does its own: only lengths inside an
    /// angle's expression take the units.
    fn follow_units(&mut self, document: &Document) {
        let design = document.design();
        if design == self.design {
            return;
        }
        let ask = Turn::ask(&self.design);
        for field in &mut self.fields {
            field.follow_units(&ask);
        }
        self.design = design;
    }

    /// The revolve as set up of `document`, if it's whole: a source,
    /// regions picked, an axis the source has, and the angles its extent
    /// takes, as they last read.
    fn revolve(&self, document: &Document) -> Option<Revolve> {
        let sketch = self.regions.source?;
        if self.regions.picked.is_empty() {
            return None;
        }
        let axis = self.axis(document)?;
        let value = |angle: Angle| self.fields[angle.index()].value.clone();
        let extent = match self.extent {
            TurnKind::Full => Turn::Full,
            TurnKind::OneSide => Turn::OneSide(value(Angle::First)?),
            TurnKind::Symmetric => Turn::Symmetric(value(Angle::First)?),
            TurnKind::TwoSides => Turn::TwoSides(value(Angle::First)?, value(Angle::Second)?),
        };
        Some(Revolve {
            sketch,
            regions: self.regions.references().to_vec(),
            axis,
            extent,
            flip: self.flip,
            operation: self.targets.operation(self.operation),
        })
    }

    /// Why the revolve as set up can't be committed to `document`, if its
    /// own check refuses it: two sides over a turn together, say. None
    /// while it isn't whole.
    fn refused(&self, document: &Document) -> Option<RevolveError> {
        let design = document.design();
        self.revolve(document)?.check_own(&design).err()
    }

    /// Whether it can be committed to `document`: it's whole, none of the
    /// angles its extent takes is refused, and it passes its own check
    /// ([`RevolveSession::refused`]). The axis is the source's
    /// ([`RevolveSession::axis`]); what's left to the document are the
    /// references to other features and bodies, which the session keeps
    /// valid.
    fn ready(&self, document: &Document) -> bool {
        let typed =
            (self.extent.angles().iter()).all(|angle| self.fields[angle.index()].error.is_none());
        let design = document.design();
        typed
            && self
                .revolve(document)
                .is_some_and(|revolve| revolve.check_own(&design).is_ok())
    }
}

/// The sketch of the sketch feature `id` of `document`, if it's one.
fn sketch_of(document: &Document, id: FeatureId) -> Option<&varde_document::Sketch> {
    match &document.feature(id)?.kind {
        FeatureKind::Sketch { sketch, .. } => Some(sketch),
        _ => None,
    }
}

/// Whether `sketch` has `axis`: a line of it with its ends apart, or one
/// of its axes. What adding or setting a revolve requires of it
/// ([`Revolve::check_axis`]), and that the line has a length, which
/// regeneration requires ("its axis line has no length"): a line whose
/// ends are at one point is no axis to pick, nor to commit.
fn axis_holds(sketch: &varde_document::Sketch, axis: AxisLine) -> bool {
    match axis {
        AxisLine::SketchX | AxisLine::SketchY => true,
        AxisLine::Curve(id) => match sketch.curve(id).map(|entry| &entry.curve) {
            Some(&varde_sketch::Curve::Line { start, end }) => {
                let at = |point| sketch.point(point).map(|point| point.at);
                at(start)
                    .zip(at(end))
                    .is_some_and(|(start, end)| start != end)
            }
            _ => false,
        },
    }
}

impl Doc {
    /// Starts setting up a new revolve, in a document that can be changed
    /// and outside a sketch, taking the regions of the sketch selected in
    /// the Timeline if one is; or cancels the one being set up. An extrude
    /// or combine being set up is dropped. The first angle's field, if the extent has one, takes the
    /// focus.
    pub(crate) fn start_revolve(&mut self) {
        if self.revolve.take().is_some() || !self.editable() || self.sketch.is_some() {
            return;
        }
        self.picking_plane = None;
        self.extrude = None;
        self.combine = None;
        let document = self.editor.document();
        let selected = self.selected_feature.filter(|&id| is_sketch(document, id));
        let mut session = RevolveSession::new(document, selected);
        session.regions.refresh(document);
        self.revolve = Some(session);
        self.focus = Some(Focus::All);
    }

    /// Edits the revolve feature `id`, if the document holds it, in a
    /// session with its values, outside a sketch, in a document that can
    /// be changed: a read-only one has no session. An extrude or combine
    /// being set up is dropped.
    pub(crate) fn edit_revolve(&mut self, id: FeatureId) {
        let document = self.editor.document();
        let Some(FeatureKind::Revolve(revolve)) = document.feature(id).map(|f| &f.kind) else {
            return;
        };
        if self.sketch.is_some() || !self.editable() {
            return;
        }
        self.picking_plane = None;
        self.extrude = None;
        self.combine = None;
        self.selected_feature = Some(id);
        self.revolve = Some(RevolveSession::editing(document, id, revolve));
        self.focus = Some(Focus::All);
    }

    /// Takes `message`, changing the revolve being set up.
    pub(crate) fn revolve_look(&mut self, message: RevolveLook) {
        let editable = self.editable();
        let document = self.editor.document();
        let Some(session) = &mut self.revolve else {
            return;
        };
        match message {
            RevolveLook::Cancel => self.revolve = None,
            _ if !editable => {}
            RevolveLook::PickRegion { sketch, region } => {
                // The axis is of the source: it stays while one is picked.
                let keep = session.axis.is_some();
                session.regions.toggle(sketch, region, keep, document);
                // Once a region is picked, the axis is next.
                if session.axis.is_none() && !session.regions.picked.is_empty() {
                    session.picking = RevolvePick::Axis;
                }
            }
            RevolveLook::PickAxis { sketch, axis } => session.pick_axis(sketch, axis, document),
            RevolveLook::Picking(picking) => session.picking = picking,
            RevolveLook::Extent(kind) => session.extent = kind,
            RevolveLook::Input { angle, text } => {
                let ask = Turn::ask(&document.design());
                session.fields[angle.index()].input(text, &ask);
            }
            RevolveLook::Flip => session.flip = !session.flip,
            RevolveLook::Operation(kind) => session.operation = kind,
            RevolveLook::Target(body) => {
                let revision = self.feed.revision();
                (session.targets).toggle(body, session.feature, document, revision);
            }
        }
    }

    /// Whether the revolve being set up can be committed: the document
    /// can be changed, no sketch edits wait on the solver, and the session
    /// is ready ([`RevolveSession::ready`]), as for an extrude
    /// ([`Doc::extrude_ready`]).
    pub(crate) fn revolve_ready(&self) -> bool {
        self.revolve.as_ref().is_some_and(|session| {
            self.editable()
                && !self.proposing()
                && session.ready(self.editor.document())
                && self.held(session.feature, session.operation).is_none()
        })
    }

    /// Adds the revolve being set up, or changes the one edited, as one
    /// undo step, and ends the session: if it's ready
    /// ([`Doc::revolve_ready`]), its preview failed only if `accept`
    /// ([`Doc::commit_by`]), and the document takes it. Refused, the
    /// session stays, and why shows.
    pub(crate) fn commit_revolve(&mut self, accept: bool) {
        if !self.commit_by(self.revolve_ready(), accept) {
            return;
        }
        let Some(session) = &self.revolve else {
            return;
        };
        let Some(revolve) = session.revolve(self.editor.document()) else {
            return;
        };
        if self.commit_feature(session.feature, revolve.into()) {
            self.revolve = None;
        }
    }

    /// Ends the revolve session if what it's about is gone, the document
    /// can't be changed any more, or the document was replaced whole
    /// (`replaced`), as [`Doc::prune_extrude`] does the extrude's; and
    /// finds its sketches' regions again if they changed.
    pub(crate) fn prune_revolve(&mut self, replaced: bool) {
        let editable = self.editable();
        let document = self.editor.document();
        let Some(session) = &mut self.revolve else {
            return;
        };
        let edited = session.feature.is_none_or(|feature| {
            matches!(
                document.feature(feature).map(|f| &f.kind),
                Some(FeatureKind::Revolve(_))
            )
        });
        if !(editable && !replaced && edited && session.regions.refresh(document)) {
            self.revolve = None;
            return;
        }
        session.follow_units(document);
        session.targets.prune(document);
    }

    /// The revolve being set up as the regeneration lane previews it, and
    /// the revolve it edits, if it's whole.
    pub(crate) fn revolve_draft(&self) -> Option<(Option<FeatureId>, FeatureKind)> {
        let session = self.revolve.as_ref()?;
        let revolve = session.revolve(self.editor.document())?;
        Some((session.feature, revolve.into()))
    }

    /// The revolve being set up, for the view.
    pub(crate) fn revolve_state(&self) -> Option<RevolveState<'_>> {
        let session = self.revolve.as_ref()?;
        let document = self.editor.document();
        let editing = session
            .feature
            .and_then(|feature| document.feature(feature))
            .map(|feature| feature.name.as_str());
        Some(RevolveState {
            editing,
            candidates: session.regions.candidates(|id| self.placement(id)),
            source: session.regions.source,
            picked: &session.regions.picked,
            missing: session.regions.missing,
            axis: session.axis(document),
            axis_missing: session.axis_missing,
            picking: session.picking,
            extent: session.extent,
            fields: [session.fields[0].field(), session.fields[1].field()],
            flip: session.flip,
            operation: session.operation,
            targets: self.body_targets(session.operation, session.feature, &session.targets),
            error: self.feed.draft_error(),
            show_error: self.draft_framed(),
            refused: session.refused(document),
            held: self.held(session.feature, session.operation),
            checking: self.proposals.slow(),
            ready: self.commit_by(self.revolve_ready(), false),
            accept: self.commit_by(self.revolve_ready(), true),
            editable: self.editable(),
            hover: self.panel_hover(),
        })
    }
}

#[cfg(test)]
pub(crate) mod tests;
