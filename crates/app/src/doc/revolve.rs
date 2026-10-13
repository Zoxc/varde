//! Setting up a revolve: its session, started by `Look::StartRevolve` or
//! by editing a revolve, picking regions and the axis (a line of the
//! source sketch or one of its axes, or a straight model edge in its
//! plane), its angles typed, the preview
//! through the regeneration lane's drafts, and committing it as one undo
//! step or cancelling it, which leaves no trace. Picking regions, the
//! Bodies list and the typed fields are the extrude's
//! ([`super::regions`]).

use crate::doc::{DocMode, OperationSession};
use std::borrow::Cow;
use std::f64::consts::PI;

use glam::DVec3;
use varde_document::{
    AxisLine, Document, EdgeRef, FeatureId, FeatureKind, MAX_REVOLVE_REGIONS, Revolve,
    RevolveError, Turn,
};
use varde_expr::{AngleUnit, Unit};
use varde_view::{
    Angle, NamingPicks, OperationKind, PanelHover, PickIndex, RevolveLook, RevolvePick,
    RevolveState, Selected, TurnKind, Unnamed, axis_edge,
};

use super::regions::{
    BodyTargets, ReadIn, RegionPick, SideFields, TypedText, drag_side, regions_session_stays,
};
use super::{Doc, OUT_OF_DATE, sketch_of};

/// The revolve being set up, while one is: [`Doc::mode`].
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
    /// The angles' fields, and the flip.
    pub(crate) sides: SideFields,
    pub(crate) operation: OperationKind,
    /// The bodies a join, cut or intersect leaves out: the edited
    /// revolve's to start with.
    pub(crate) targets: BodyTargets,
    /// The panel's row the cursor is over, if any.
    pub(crate) hover: Option<PanelHover>,
    /// The knob being dragged in the viewport, if one is.
    pub(crate) grabbed: Option<Angle>,
    /// The design as the fields' texts were last read, whose units bare
    /// lengths in them are in: see [`RevolveSession::follow_design`].
    read_in: ReadIn,
}

/// The angles a new revolve's fields start with, in radians: half a turn
/// for one side and symmetric, and a quarter for two sides' second.
const DEFAULT_ANGLES: [f64; 2] = [PI, PI / 2.0];

impl RevolveSession {
    /// A session setting up a new revolve in `document`, taking the
    /// regions of `source` if given, else of the one the first region (or
    /// the axis) is picked in.
    pub(crate) fn new(document: &Document, source: Option<FeatureId>) -> Self {
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
            sides: SideFields::new(DEFAULT_ANGLES.map(field)),
            operation: OperationKind::NewBody,
            targets: BodyTargets::default(),
            read_in: ReadIn::of(document),
        }
    }

    /// A session editing the revolve `feature` of `document`, with its
    /// values, the regions of its sketch its references find, and its
    /// axis if its sketch has it.
    pub(crate) fn editing(document: &Document, feature: FeatureId, revolve: &Revolve) -> Self {
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
        let flips = session.extent.flips();
        // Unlike an extrude's, two sides switched to from one keep the
        // second's default angle rather than taking the first's.
        (session.sides).open([first, second], revolve.flip, flips, &ask);
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
    /// Whether it was picked.
    pub(crate) fn pick_axis(
        &mut self,
        sketch: FeatureId,
        axis: AxisLine,
        document: &Document,
    ) -> bool {
        let holds = sketch_of(document, sketch).is_some_and(|drawn| axis_holds(drawn, axis));
        if !holds || !self.regions.choose(sketch) {
            return false;
        }
        self.axis = Some(axis);
        self.axis_missing = false;
        self.edge_ends = None;
        self.picking = RevolvePick::Regions;
        self.regions.refresh(document);
        true
    }

    /// Picks the model edge `reference`, its ends `ends`, as the axis
    /// ([`Doc::edge_axis`] checks it). Clicks pick regions again then.
    pub(crate) fn pick_edge_axis(
        &mut self,
        reference: EdgeRef,
        ends: [DVec3; 2],
        document: &Document,
    ) {
        self.axis = Some(AxisLine::Edge(reference));
        self.axis_missing = false;
        self.edge_ends = Some(ends);
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
        let now = Turn::ask(&self.read_in.design());
        self.sides.follow(&Turn::ask(&before.design()), &now);
    }

    /// The revolve as set up of `document`, if it's whole: a source,
    /// regions picked, an axis the source has, and the angles its extent
    /// takes, as they last read.
    pub(crate) fn revolve(&self, document: &Document) -> Option<Revolve> {
        let sketch = self.regions.source?;
        if self.regions.references().is_empty() {
            return None;
        }
        let axis = self.axis(document)?;
        let value = |angle: Angle| self.sides.value(angle);
        let extent = match self.extent {
            TurnKind::Full => Turn::Full,
            TurnKind::OneSide => Turn::OneSide(value(Angle::First)?),
            TurnKind::Symmetric => Turn::Symmetric(value(Angle::First)?),
            TurnKind::TwoSides => Turn::TwoSides(value(Angle::First)?, value(Angle::Second)?),
        };
        Some(Revolve {
            sketch,
            regions: self.regions.references(),
            axis,
            extent,
            flip: self.sides.stored_flip(self.extent.flips()),
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
    /// radians about the axis from the sketch plane ([`drag_side`]), a
    /// whole turn at most: two sides together can't go over a turn by
    /// it.
    fn drag(&mut self, angle: Angle, to: f64, document: &Document) {
        drag_side(
            self,
            |session| &mut session.sides,
            self.extent.sides(),
            angle,
            to,
            (&Turn::ask(&document.design()), Unit::Angle(AngleUnit::Deg)),
            |session| session.refused(document),
            RevolveError::Turn,
        );
    }

    /// Reads `text` into the field of `angle`, in `document`'s design.
    pub(crate) fn input(&mut self, angle: Angle, text: String, document: &Document) {
        (self.sides).input(angle, text, &Turn::ask(&document.design()));
    }

    /// Whether it can be committed to `document`: it's whole, none of the
    /// angles its extent takes is refused, and it passes its own check
    /// ([`RevolveSession::refused`]). The axis is the source's
    /// ([`RevolveSession::axis`]); what's left to the document are the
    /// references to other features and bodies, which the session keeps
    /// valid.
    fn ready(&self, document: &Document) -> bool {
        let typed = self.sides.typed(self.extent.sides());
        let design = document.design();
        typed
            && self
                .revolve(document)
                .is_some_and(|revolve| revolve.check_own(&design).is_ok())
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
        if self.mode.take_revolve().is_some() || !self.can_open_operation() {
            return;
        }
        // Leaving whatever else was open.
        self.mode = DocMode::Idle;
        let selected = self.region_source();
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
        session.axis_selected = {
            let mut items = self.pick.selection.items();
            match (items.next(), items.next()) {
                (Some(&Selected::Edge { body, faces, near }), None) => {
                    Some(EdgeRef { body, faces, near })
                }
                _ => None,
            }
        };
        self.open_operation(OperationSession::Revolve(session), None, true);
        // Picking the selected edge is part of opening: the baseline is
        // taken again with the axis it gives.
        self.axis_from_selection();
        self.opened_operation();
    }

    /// Picks the edge selected when the revolve started as its axis, once
    /// it has a profile and no axis, saying why if it can't be: tried
    /// once.
    fn axis_from_selection(&mut self) {
        let Some(session) = self.mode.revolve_mut() else {
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
        if !self.can_open_operation() {
            return;
        }
        let mut session = RevolveSession::editing(document, id, revolve);
        if let AxisLine::Edge(edge) = &revolve.axis {
            session.edge_ends = self.shown_edge(edge);
        }
        self.open_operation(OperationSession::Revolve(session), Some(id), true);
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
        }) = self.mode.revolve()
        else {
            return;
        };
        let Some(ends) = self.shown_edge(edge) else {
            return;
        };
        if let Some(session) = self.mode.revolve_mut() {
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
        let Some(session) = self.mode.revolve() else {
            return;
        };
        match self.axis_edge(session, model, edge, at) {
            Ok((reference, ends)) => {
                let document = self.editor.document();
                if let Some(session) = self.mode.revolve_mut() {
                    session.pick_edge_axis(reference, ends, document);
                }
            }
            Err(why) => self.notice = Some(why.into_owned()),
        }
    }

    /// Edge `edge` of the model shown, of the index `model` names, clicked
    /// at `at`, as the axis of the revolve `session` sets up
    /// ([`Doc::edge_axis`]). Refused, why, also if the model shown isn't
    /// the one clicked or the cursor doesn't pick it, or there's no
    /// source to take the plane of yet.
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
        self.edge_axis(self.feed.pick_index(), source, session.feature, edge, at)
    }

    /// Edge `edge` of `index`, at `at`, as the axis of a revolve of the
    /// sketch `source` edited from `edited` (a new one if `None`): the
    /// reference the revolve stores
    /// ([`Naming::edge_ref`](varde_view::Naming::edge_ref), with the
    /// history stopped at the revolve) and the edge's ends. Refused, why,
    /// if the sketch isn't placed, the edge isn't straight or in its
    /// plane ([`axis_edge`]), isn't made before the revolve, or which
    /// body it's on there can't be told.
    pub(crate) fn edge_axis(
        &self,
        index: &PickIndex,
        source: FeatureId,
        edited: Option<FeatureId>,
        edge: u32,
        at: DVec3,
    ) -> Result<(EdgeRef, [DVec3; 2]), Cow<'static, str>> {
        let placement = self
            .placement(source)
            .ok_or("The profile's sketch isn't placed")?;
        let resolution = self.editor.document().tolerance().resolution();
        let ends = axis_edge(index, edge, &placement, resolution)?;
        let naming = self.naming_at(edited);
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
        let Some(session) = self.mode.revolve_mut() else {
            return;
        };
        match message {
            RevolveLook::Cancel => {
                self.mode.take_operation();
            }
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
            RevolveLook::DropMissing(index) => session.regions.drop_missing(index),
            RevolveLook::PickAxis { sketch, axis } => {
                session.pick_axis(sketch, axis, document);
            }
            RevolveLook::PickEdge { model, edge, at } => self.pick_axis_edge(model, edge, at),
            RevolveLook::Picking(picking) => session.picking = picking,
            RevolveLook::Extent(kind) => session.extent = kind,
            RevolveLook::Input { angle, text } => session.input(angle, text, document),
            RevolveLook::Flip => session.sides.flip = !session.sides.flip,
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
    /// can be changed, none of the user's sketch edits wait on the solver, and the session
    /// is ready ([`RevolveSession::ready`]), as for an extrude
    /// ([`Doc::extrude_ready`]).
    pub(crate) fn revolve_ready(&self) -> bool {
        self.mode.revolve().is_some_and(|session| {
            self.editable() && !self.users_proposing() && session.ready(self.editor.document())
        })
    }

    /// Adds the revolve being set up, or changes the one edited, as one
    /// undo step, and ends the session: if it's ready
    /// ([`Doc::revolve_ready`]), its preview failed only if `accept`
    /// ([`Doc::commit_by`]), and the document takes it. Refused, the
    /// session stays, and why shows.
    pub(crate) fn commit_revolve(&mut self, accept: bool) {
        self.commit_built(self.revolve_ready(), accept, self.revolve_draft());
    }

    /// Ends the revolve session if what it's about is gone, the document
    /// can't be changed any more, or the document was replaced whole
    /// (`replaced`), as [`Doc::prune_extrude`] does the extrude's; and
    /// finds its sketches' regions again if they changed.
    pub(crate) fn prune_revolve(&mut self, replaced: bool) {
        let editable = self.editable();
        let document = self.editor.document();
        let Some(session) = self.mode.revolve_mut() else {
            return;
        };
        let is_kind = |kind: &FeatureKind| matches!(kind, FeatureKind::Revolve(_));
        let (feature, regions) = (session.feature, &mut session.regions);
        if !regions_session_stays(document, feature, regions, editable, replaced, is_kind) {
            self.mode.take_operation();
            return;
        }
        session.follow_design(document);
        session.targets.prune(document);
        session.prune_edge(document);
    }

    /// The revolve being set up as the regeneration lane previews it, and
    /// the revolve it edits, if it's whole.
    pub(crate) fn revolve_draft(&self) -> Option<(Option<FeatureId>, FeatureKind)> {
        let session = self.mode.revolve()?;
        let revolve = session.revolve(self.editor.document())?;
        Some((session.feature, revolve.into()))
    }

    /// The revolve being set up, for the view.
    pub(crate) fn revolve_state(&self) -> Option<RevolveState<'_>> {
        let session = self.mode.revolve()?;
        let document = self.editor.document();
        let uncut = (session.operation == OperationKind::Cut)
            .then(|| self.uncut_note())
            .flatten();
        Some(RevolveState {
            verdict: self.verdict(
                session.feature,
                self.revolve_ready(),
                session.refused(document).map(|refused| refused.to_string()),
                uncut,
            ),
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
            fields: (session.sides.fields.each_ref()).map(|field| field.field(self.params_in())),
            flip: session.sides.flip,
            operation: session.operation,
            targets: self.body_targets(session.operation, session.feature, &session.targets),
            editable: self.editable(),
            hover: self.panel_hover(),
            grabbed: session.grabbed,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests;
