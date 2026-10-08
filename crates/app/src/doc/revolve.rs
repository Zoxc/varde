//! Setting up a revolve: its session, started by `Look::StartRevolve` or
//! by editing a revolve, picking regions and the axis (a line of the
//! source sketch or one of its axes, or a straight model edge in its
//! plane), its angles typed, the preview
//! through the regeneration lane's drafts, and committing it as one undo
//! step or cancelling it, which leaves no trace. Picking regions, the
//! Bodies list and the typed fields are the extrude's
//! ([`super::regions`]).

use std::borrow::Cow;
use std::f64::consts::PI;

use glam::DVec3;
use varde_document::{
    AxisLine, Document, EdgeRef, FeatureId, FeatureKind, MAX_REVOLVE_REGIONS, Revolve,
    RevolveError, Turn,
};
use varde_expr::{AngleUnit, Unit};
use varde_view::{
    Angle, OperationKind, PanelHover, RevolveLook, RevolvePick, RevolveState, Selected, TurnKind,
    Unnamed, axis_edge,
};

use super::extrude::is_sketch;
use super::regions::{BodyTargets, ReadIn, RegionPick, TypedText};
use super::{Doc, Focus, OUT_OF_DATE};

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
    /// Where the axis is, if it's a model edge found in the model shown
    /// when it was picked, or when the session started: its ends, in the
    /// order it runs. For the arrow only: regenerating finds the edge
    /// again.
    pub(crate) edge_ends: Option<[DVec3; 2]>,
    /// The straight edge selected when the session started, its axis
    /// once the profile is picked (which the edge is checked against).
    axis_selected: Option<EdgeRef>,
    /// What a click picks first.
    pub(crate) picking: RevolvePick,
    pub(crate) extent: TurnKind,
    /// The first angle's field, and two sides' second.
    pub(crate) fields: [TypedText; 2],
    /// Whether one side goes the other way, or two sides swap: kept
    /// while the extent ignores it (see [`RevolveSession::stored_flip`]), for when
    /// it's switched back.
    pub(crate) flip: bool,
    /// The flip the revolve edited was stored with while its extent ignores
    /// it, as old files may have it: written back as it was while the
    /// extent set up ignores it too, so an edit changing nothing writes
    /// nothing. False otherwise.
    ignored_flip: bool,
    pub(crate) operation: OperationKind,
    /// The bodies a join, cut or intersect leaves out: the edited
    /// revolve's to start with.
    pub(crate) targets: BodyTargets,
    /// The design as the fields' texts were last read, whose units bare
    /// lengths in them are in: see [`RevolveSession::follow_design`].
    /// The panel's row the cursor is over, if any.
    pub(crate) hover: Option<PanelHover>,
    /// The knob being dragged in the viewport, if one is.
    pub(crate) grabbed: Option<Angle>,
    read_in: ReadIn,
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
            grabbed: None,
            axis: None,
            axis_missing: false,
            edge_ends: None,
            axis_selected: None,
            picking: RevolvePick::Regions,
            extent: TurnKind::Full,
            fields: DEFAULT_ANGLES.map(field),
            flip: false,
            ignored_flip: false,
            operation: OperationKind::NewBody,
            targets: BodyTargets::default(),
            read_in: ReadIn::of(document),
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
        // A model edge isn't the sketch's: regenerating says if it's gone.
        let found = matches!(revolve.axis, AxisLine::Edge(_))
            || sketch_of(document, revolve.sketch)
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
        session.ignored_flip = revolve.flip && !session.extent.flips();
        session.operation = OperationKind::of(&revolve.operation);
        session.targets = BodyTargets::new(revolve.operation.excluded());
        session
    }

    /// The axis picked, if the source has it as `document` is: a line of
    /// it, or one of its axes; or a model edge, which the document
    /// checks.
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
        self.edge_ends = None;
        self.picking = RevolvePick::Regions;
        self.regions.refresh(document);
    }

    /// Takes the curves `curves` of `sketch`, selected: the regions they
    /// bound, and as the axis the one straight line among them on none of
    /// those regions' outer loops, if one alone is.
    fn take_curves(&mut self, sketch: FeatureId, curves: &[varde_sketch::Id], document: &Document) {
        let bounded = self.regions.take_curves(sketch, curves, document);
        let Some(found) = self.regions.found(sketch) else {
            return;
        };
        let on_loops = |curve: varde_sketch::Id| {
            bounded
                && (self.regions.picked.iter()).any(|&region| {
                    (found.profiles.regions.get(region))
                        .is_some_and(|region| region.outer.iter().any(|piece| piece.curve == curve))
                })
        };
        let mut lines = (curves.iter().copied()).filter(|&curve| {
            matches!(
                found.sketch.curve(curve).map(|entry| &entry.curve),
                Some(varde_sketch::Curve::Line { .. })
            ) && !on_loops(curve)
        });
        if let (Some(line), None) = (lines.next(), lines.next()) {
            self.pick_axis(sketch, AxisLine::Curve(line), document);
        }
    }

    /// Drops the axis if it's a model edge `document` no longer takes
    /// there (an undo took its body or a face's maker away, or the
    /// revolve edited moved after them), so what's set up never names
    /// what the document can't hold; it's picked again. An edited
    /// revolve's own edge, whose body or maker is gone, the document
    /// holds, failing, and it stays.
    fn prune_edge(&mut self, document: &Document) {
        let Some(AxisLine::Edge(edge)) = &self.axis else {
            return;
        };
        let index = match self.feature {
            Some(id) => document.feature_index(id),
            None => Some(document.insert_at()),
        };
        if index.is_some_and(|index| document.check_edge(index, edge).is_ok()) {
            return;
        }
        self.axis = None;
        self.edge_ends = None;
        self.picking = RevolvePick::Axis;
    }

    /// Keeps each angle where the design's units changed since its field
    /// was read, as the document does its own: only lengths inside an
    /// angle's expression take the units. One naming parameters is read
    /// again where they changed ([`TypedText::follow`]).
    fn follow_design(&mut self, document: &Document) {
        let Some(before) = self.read_in.follow(document) else {
            return;
        };
        let ask = Turn::ask(&before.design());
        let now = Turn::ask(&self.read_in.design());
        for field in &mut self.fields {
            field.follow(&ask, &now);
        }
    }

    /// The flip to store: the one set up where the extent takes it,
    /// else the one stored before if that extent ignored it too
    /// ([`RevolveSession::ignored_flip`]), else none, so a flip left set
    /// from another extent isn't stored where it changes nothing.
    fn stored_flip(&self) -> bool {
        if self.extent.flips() {
            self.flip
        } else {
            self.ignored_flip
        }
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
            flip: self.stored_flip(),
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

    /// Types into `angle`'s field where its knob is dragged to: `to`
    /// radians about the axis from the sketch plane. One side's angle is
    /// how far round either way, a turn back past the plane flipping it;
    /// symmetric's twice it; two sides' each how far its own way. Nothing
    /// changes for an angle the field doesn't take (none, or past a turn),
    /// nor for a drag that takes two sides further over a turn than they
    /// were.
    fn drag(&mut self, angle: Angle, to: f64, document: &Document) {
        let (turn, flip) = match (self.extent, angle) {
            (TurnKind::OneSide, Angle::First) => (to.abs(), Some(to < 0.0)),
            (TurnKind::Symmetric, Angle::First) => (2.0 * to.abs(), None),
            (TurnKind::TwoSides, _) => (to.abs(), None),
            _ => return,
        };
        if !(turn > 0.0 && turn.is_finite()) {
            return;
        }
        let text = varde_expr::format(turn, Some(Unit::Angle(AngleUnit::Deg)));
        let mut field = self.fields[angle.index()].clone();
        field.input(text, &Turn::ask(&document.design()));
        if field.error.is_some() {
            return;
        }
        let before = self.refused(document);
        let old = std::mem::replace(&mut self.fields[angle.index()], field);
        let old_flip = self.flip;
        if let Some(flip) = flip {
            self.flip = flip;
        }
        let worse = match (before, self.refused(document)) {
            (_, None) => false,
            (None, Some(_)) => true,
            // Already over a turn (typed so), the knob only goes back.
            (Some(_), Some(after)) => {
                after == RevolveError::Turn && old.value.as_ref().is_none_or(|old| turn > old.value)
            }
        };
        if worse {
            self.fields[angle.index()] = old;
            self.flip = old_flip;
        }
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
pub(super) fn sketch_of(document: &Document, id: FeatureId) -> Option<&varde_document::Sketch> {
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
        AxisLine::SketchX | AxisLine::SketchY | AxisLine::Edge(_) => true,
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
    /// the Timeline if one is, or else the first one selected in Objects;
    /// or cancels the one being set up. An extrude
    /// or combine being set up is dropped. The first angle's field, if the extent has one, takes the
    /// focus.
    pub(crate) fn start_revolve(&mut self) {
        if self.revolve.take().is_some() || !self.editable() || self.sketch.is_some() {
            return;
        }
        self.picking_plane = None;
        self.extrude = None;
        self.combine = None;
        self.motion = None;
        let document = self.editor.document();
        // Or the first selected in Objects.
        let selected = (self.selected_feature.filter(|&id| is_sketch(document, id)))
            .or_else(|| self.selected_sketches().next());
        let curves = self.selected_curves();
        let document = self.editor.document();
        let mut session = RevolveSession::new(document, selected);
        session.regions.refresh(document);
        // Sketch curves selected: the regions they bound, and the line
        // among them bounding none, or a line alone, as the axis.
        if let Some((sketch, curves)) = curves {
            session.take_curves(sketch, &curves, document);
        }
        // An edge alone selected is the axis to be.
        let mut items = self.pick.selection.items();
        session.axis_selected = match (items.next(), items.next()) {
            (Some(&Selected::Edge { body, faces, near }), None) => {
                Some(EdgeRef { body, faces, near })
            }
            _ => None,
        };
        drop(items);
        self.revolve = Some(session);
        self.focus = Some(Focus::All);
        self.axis_from_selection();
    }

    /// Picks the edge selected when the revolve started as its axis, once
    /// it has a profile and no axis, saying why if it can't be: tried
    /// once.
    fn axis_from_selection(&mut self) {
        let Some(session) = &mut self.revolve else {
            return;
        };
        if session.axis.is_some() || session.regions.source.is_none() {
            return;
        }
        let Some(edge) = session.axis_selected.take() else {
            return;
        };
        let index = self.feed.pick_index();
        let body = self.feed.shown_body(edge.body);
        if let Some(found) = index.find_edge(body, edge.faces, edge.near) {
            self.pick_axis_edge(index.model(), found, edge.near);
        }
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
        self.motion = None;
        self.selected_feature = Some(id);
        let mut session = RevolveSession::editing(document, id, revolve);
        if let AxisLine::Edge(edge) = &revolve.axis {
            session.edge_ends = self.shown_edge(edge);
        }
        self.revolve = Some(session);
        self.focus = Some(Focus::All);
    }

    /// Moves the arrow of the revolve being set up, if its axis is a model
    /// edge, to where the model just shown has the edge: an undo or an
    /// edit upstream may have moved it. Kept where it was if the model
    /// doesn't show it (the revolve's own join swallowing it, say):
    /// regenerating says if it's gone.
    pub(crate) fn follow_edge_axis(&mut self) {
        let Some(RevolveSession {
            axis: Some(AxisLine::Edge(edge)),
            ..
        }) = &self.revolve
        else {
            return;
        };
        let Some(ends) = self.shown_edge(edge) else {
            return;
        };
        if let Some(session) = &mut self.revolve {
            session.edge_ends = Some(ends);
        }
    }

    /// Where the model shown has the edge `edge` names, if it has it and
    /// it's straight: its ends, in the order the reference runs. It's
    /// looked for on the body holding `edge`'s body at the end of the
    /// history, which the model shows.
    fn shown_edge(&self, edge: &EdgeRef) -> Option<[DVec3; 2]> {
        let index = self.feed.pick_index();
        let found = index.find_edge(self.feed.shown_body(edge.body), edge.faces, edge.near)?;
        index.edge_ends(found, &edge.faces)
    }

    /// Picks edge `edge` of the model shown, of the index `model` names,
    /// clicked at `at`, as the axis of the revolve being set up, or says
    /// why it can't be ([`Doc::axis_edge`]).
    fn pick_axis_edge(&mut self, model: u64, edge: u32, at: DVec3) {
        let Some(session) = &self.revolve else {
            return;
        };
        match self.axis_edge(session, model, edge, at) {
            Ok((reference, ends)) => {
                let document = self.editor.document();
                let Some(session) = &mut self.revolve else {
                    return;
                };
                session.axis = Some(AxisLine::Edge(reference));
                session.axis_missing = false;
                session.edge_ends = Some(ends);
                session.picking = RevolvePick::Regions;
                session.regions.refresh(document);
            }
            Err(why) => self.notice = Some(why.into_owned()),
        }
    }

    /// Edge `edge` of the model shown, of the index `model` names, clicked
    /// at `at`, as the axis of the revolve `session` sets up: the
    /// reference the revolve stores
    /// ([`Naming::edge_ref`](varde_view::Naming::edge_ref), with the
    /// history stopped at the revolve) and the edge's ends. Refused, why,
    /// if the model shown isn't the one clicked or the cursor doesn't pick
    /// it, there's no source to take the plane of yet, or the edge isn't
    /// straight or in the source's plane ([`axis_edge`]), isn't made
    /// before the revolve, or which body it's on there can't be told.
    fn axis_edge(
        &self,
        session: &RevolveSession,
        model: u64,
        edge: u32,
        at: DVec3,
    ) -> Result<(EdgeRef, [DVec3; 2]), Cow<'static, str>> {
        if model != self.feed.model() || self.feed.predates_replacement() {
            return Err(OUT_OF_DATE.into());
        }
        let source = (session.regions.source).ok_or("Pick the profile first, then its axis")?;
        let placement = self
            .placement(source)
            .ok_or("The profile's sketch isn't placed")?;
        let index = self.feed.pick_index();
        let document = self.editor.document();
        let resolution = document.tolerance().resolution();
        let ends = axis_edge(index, edge, &placement, resolution)?;
        let naming = self.naming_at(session.feature);
        let reference = naming.edge_ref(index, edge, at).map_err(|why| match why {
            Unnamed::Missing => "That edge isn't in the model",
            Unnamed::Later => "Only an edge made before the revolve can be its axis",
            Unnamed::Unclear => {
                "Which body that edge is on at the revolve can't be told: pick another"
            }
        })?;
        Ok((reference, ends))
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
            RevolveLook::PickEdge { model, edge, at } => self.pick_axis_edge(model, edge, at),
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
            RevolveLook::GrabHandle(angle) => session.grabbed = Some(angle),
            RevolveLook::DragHandle { angle, to } => {
                if session.grabbed == Some(angle) {
                    session.drag(angle, to, document);
                }
            }
            RevolveLook::DropHandle => session.grabbed = None,
        }
        self.axis_from_selection();
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
        session.follow_design(document);
        session.targets.prune(document);
        session.prune_edge(document);
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
            edge_ends: session
                .edge_ends
                .filter(|_| matches!(session.axis, Some(AxisLine::Edge(_)))),
            edge_body: match &session.axis {
                Some(AxisLine::Edge(edge)) => document.body(edge.body).map(|b| b.name.as_str()),
                _ => None,
            },
            index: self.feed.pick_index(),
            resolution: document.tolerance().resolution(),
            picking: session.picking,
            extent: session.extent,
            fields: [
                session.fields[0].field(self.params_in()),
                session.fields[1].field(self.params_in()),
            ],
            flip: session.flip,
            operation: session.operation,
            targets: self.body_targets(session.operation, session.feature, &session.targets),
            error: self.feed.shown_draft_error(),
            show_error: self.draft_framed(),
            refused: session.refused(document),
            held: self.held(session.feature, session.operation),
            uncut: (session.operation == OperationKind::Cut)
                .then(|| self.uncut_note())
                .flatten(),
            checking: self.proposals.slow(),
            ready: self.commit_by(self.revolve_ready(), false),
            accept: self.offers_accept(self.revolve_ready()),
            editable: self.editable(),
            hover: self.panel_hover(),
            grabbed: session.grabbed,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests;
