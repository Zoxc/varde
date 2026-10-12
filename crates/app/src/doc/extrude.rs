//! Setting up an extrude: its session, started by the Extrude tool or by
//! editing an extrude, picking regions, its distances typed or dragged
//! with the handle, the preview through the regeneration lane's drafts,
//! and committing it as one undo step or cancelling it, which leaves no
//! trace.

use crate::doc::{DocMode, OperationSession, OperationSlot};
use varde_document::{
    Design, Document, Extent, Extrude, ExtrudeError, FeatureId, FeatureKind, MAX_EXTRUDE_REGIONS,
};
use varde_expr::{AngleUnit, LengthUnit, Unit};
use varde_render::Camera;
use varde_view::{Distance, ExtentKind, ExtrudeLook, ExtrudeState, OperationKind, PanelHover};

use super::regions::{
    BodyTargets, ReadIn, RegionPick, SideFields, TypedText, drag_side, regions_session_stays,
};
use super::{Doc, Focus};

/// The extrude being set up, while one is: [`Doc::mode`].
#[derive(Debug)]
pub(crate) struct ExtrudeSession {
    /// The extrude edited, or `None` for a new one.
    pub(crate) feature: Option<FeatureId>,
    /// The regions picked, and the sketch they're of.
    pub(crate) regions: RegionPick,
    pub(crate) extent: ExtentKind,
    /// The distances' fields, and the flip.
    pub(crate) sides: SideFields,
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
    /// numbers in them are in: see [`ExtrudeSession::follow_design`].
    read_in: ReadIn,
}

/// The part of the view's height a new extrude's distance starts at, at
/// most: see [`default_distance`].
const DEFAULT_SHARE: f64 = 0.25;

/// The distance a new extrude starts with, in millimetres, seen by
/// `camera` in a design of `units`: [`DEFAULT_SHARE`] of the view's
/// height, made nice by [`fitting_length`](super::camera::fitting_length).
fn default_distance(camera: &Camera, units: LengthUnit) -> f64 {
    super::camera::fitting_length(camera, units, DEFAULT_SHARE)
}

/// The distance a new extrude the MCP client adds starts with, in
/// millimetres: the panel's follows the view, which the client doesn't
/// see.
#[cfg(not(target_arch = "wasm32"))]
const CLIENT_DISTANCE: f64 = 10.0;

impl ExtrudeSession {
    /// A session setting up a new extrude, in `document`'s units, its
    /// distance one that fits the view of `camera`, taking the regions of
    /// `source` if given, else of the one the first region picked is in.
    fn new(document: &Document, camera: &Camera, source: Option<FeatureId>) -> Self {
        let distance = default_distance(camera, document.units());
        Self::with_distance(document, distance, source)
    }

    /// A session setting up a new extrude of regions of `source` for the
    /// MCP client, as the panel's but its distances [`CLIENT_DISTANCE`].
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn for_client(document: &Document, source: FeatureId) -> Self {
        Self::with_distance(document, CLIENT_DISTANCE, Some(source))
    }

    /// [`ExtrudeSession::new`], its distances `distance` millimetres.
    fn with_distance(document: &Document, distance: f64, source: Option<FeatureId>) -> Self {
        let units = document.units();
        let text = varde_expr::format(distance, Some(Unit::Length(units)));
        let distance = TypedText::read(text, &Extent::ask(&document.design()));
        let none = varde_expr::format(0.0, Some(Unit::Angle(AngleUnit::Deg)));
        let taper = TypedText::read(none, &Extrude::taper_ask(&document.design()));
        Self {
            feature: None,
            regions: RegionPick::new(source, MAX_EXTRUDE_REGIONS),
            extent: ExtentKind::OneSide,
            sides: SideFields::new([distance.clone(), distance]),
            operation: OperationKind::NewBody,
            taper,
            targets: BodyTargets::default(),
            grabbed: None,
            hover: None,
            read_in: ReadIn::of(document),
        }
    }

    /// A session editing the extrude `feature` of `document`, with its
    /// values and the regions of its sketch its references find; a
    /// distance it has no value for, one that fits the view of `camera`.
    pub(crate) fn editing(
        document: &Document,
        camera: &Camera,
        feature: FeatureId,
        extrude: &Extrude,
    ) -> Self {
        let session = Self::new(document, camera, Some(extrude.sketch));
        session.opened(document, feature, extrude)
    }

    /// [`ExtrudeSession::editing`] for the MCP client: a distance the
    /// feature has no value for (one through all) is [`CLIENT_DISTANCE`],
    /// as a new one's, not one following the user's view.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn editing_for_client(
        document: &Document,
        feature: FeatureId,
        extrude: &Extrude,
    ) -> Self {
        let session = Self::with_distance(document, CLIENT_DISTANCE, Some(extrude.sketch));
        session.opened(document, feature, extrude)
    }

    /// This fresh session opened on the extrude `feature` of `document`,
    /// with its values: see [`ExtrudeSession::editing`].
    fn opened(self, document: &Document, feature: FeatureId, extrude: &Extrude) -> Self {
        let mut session = self;
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
        // Two sides switched to from one start with both its distance.
        let flips = session.extent.flips();
        (session.sides).open([first, second.or(first)], extrude.flip, flips, &ask);
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
    /// that's refused stays as typed, to be read in the new units. One
    /// naming parameters is read again where they changed
    /// ([`TypedText::follow`]).
    fn follow_design(&mut self, document: &Document) {
        let Some(before) = self.read_in.follow(document) else {
            return;
        };
        let (before, now) = (before.design(), self.read_in.design());
        (self.sides).follow(&Extent::ask(&before), &Extent::ask(&now));
        (self.taper).follow(&Extrude::taper_ask(&before), &Extrude::taper_ask(&now));
    }

    /// The extrude as set up, if it's whole: a source, regions picked, and
    /// the distances its extent takes and the taper, as they last read (a
    /// taper of nothing is none).
    pub(crate) fn extrude(&self) -> Option<Extrude> {
        let sketch = self.regions.source?;
        if self.regions.references().is_empty() {
            return None;
        }
        let value = |distance: Distance| self.sides.value(distance);
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
            regions: self.regions.references(),
            extent,
            flip: self.sides.stored_flip(self.extent.flips()),
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

    /// Takes `kind` as the extent, unless it's through all and the
    /// operation isn't a cut, which alone goes through all: whether it
    /// was taken.
    pub(crate) fn set_extent(&mut self, kind: ExtentKind) -> bool {
        let taken = kind != ExtentKind::ThroughAll || self.operation == OperationKind::Cut;
        if taken {
            self.extent = kind;
        }
        taken
    }

    /// Takes `kind` as the operation, through all becoming one side
    /// unless it's a cut.
    pub(crate) fn set_operation(&mut self, kind: OperationKind) {
        self.operation = kind;
        if kind != OperationKind::Cut && self.extent == ExtentKind::ThroughAll {
            self.extent = ExtentKind::OneSide;
        }
    }

    /// Reads `text` into the field of `distance`, in `document`'s design.
    pub(crate) fn input(&mut self, distance: Distance, text: String, document: &Document) {
        (self.sides).input(distance, text, &Extent::ask(&document.design()));
    }

    /// Reads `text` into the taper's field, in `document`'s design.
    pub(crate) fn input_taper(&mut self, text: String, document: &Document) {
        self.taper
            .input(text, &Extrude::taper_ask(&document.design()));
    }

    /// Whether it can be committed to a document of `design`: it's whole,
    /// none of the distances its extent takes nor the taper is refused,
    /// and it passes
    /// its own check ([`ExtrudeSession::refused`]). What's left to the
    /// document are the references to other features and bodies, which
    /// the session keeps valid.
    fn ready(&self, design: &Design) -> bool {
        let typed = self.sides.typed(self.extent.sides()) && self.taper.error.is_none();
        typed
            && self
                .extrude()
                .is_some_and(|extrude| extrude.check_own(design).is_ok())
    }

    /// Moves the knob of `distance` to `to`, in millimetres along the
    /// sketch plane's normal ([`drag_side`]): two sides together can't
    /// go over the limit by it.
    fn drag(&mut self, distance: Distance, to: f64, document: &Document) {
        let design = document.design();
        drag_side(
            self,
            |session| &mut session.sides,
            self.extent.sides(),
            distance,
            to,
            (&Extent::ask(&design), Unit::Length(document.units())),
            |session| session.refused(&design),
            ExtrudeError::Length,
        );
    }
}

impl Doc {
    /// Starts setting up a new extrude, in a document that can be changed
    /// and outside a sketch, taking the regions of the sketch selected in
    /// the Timeline if one is, or else the first one selected in Objects; or cancels the one being set up. A revolve
    /// or combine being set up is dropped. The first
    /// distance's field takes the focus.
    pub(crate) fn start_extrude(&mut self) {
        if self.mode.take_extrude().is_some() || !self.editable() || self.mode.sketch().is_some() {
            return;
        }
        // Leaving whatever else was open.
        self.mode = DocMode::Idle;
        let document = self.editor.document();
        // Or the first selected in Objects.
        let selected = (self.selected_feature.filter(|&id| is_sketch(document, id)))
            .or_else(|| self.selected_sketches().next());
        let curves = self.selected_curves();
        let document = self.editor.document();
        let mut session = ExtrudeSession::new(document, &self.camera, selected);
        session.regions.refresh(document);
        // Sketch curves selected: the regions they bound.
        if let Some((sketch, curves)) = curves {
            session.regions.take_curves(sketch, &curves, document);
        }
        self.mode = DocMode::Operation(OperationSession::Extrude(session));
        self.focus = Some(Focus::All);
        self.opened_operation();
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
        if self.mode.sketch().is_some() || !self.editable() {
            return;
        }
        self.selected_feature = Some(id);
        self.mode = DocMode::Operation(OperationSession::Extrude(ExtrudeSession::editing(
            document,
            &self.camera,
            id,
            extrude,
        )));
        self.focus = Some(Focus::All);
        self.opened_operation();
    }

    /// Takes `message`, changing the extrude being set up.
    pub(crate) fn extrude_look(&mut self, message: ExtrudeLook) {
        let editable = self.editable();
        let document = self.editor.document();
        let Some(session) = self.mode.extrude_mut() else {
            return;
        };
        match message {
            ExtrudeLook::Cancel => {
                self.mode.take_operation();
            }
            _ if !editable => {}
            ExtrudeLook::PickRegion { sketch, region } => {
                session.regions.toggle(sketch, region, false, document);
            }
            ExtrudeLook::DropMissing(index) => session.regions.drop_missing(index),
            ExtrudeLook::Extent(kind) => {
                session.set_extent(kind);
            }
            ExtrudeLook::Input { distance, text } => session.input(distance, text, document),
            ExtrudeLook::Flip => session.sides.flip = !session.sides.flip,
            ExtrudeLook::Taper(text) => session.input_taper(text, document),
            ExtrudeLook::Operation(kind) => session.set_operation(kind),
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
    /// can be changed, none of the user's sketch edits wait on the solver
    /// ([`Doc::users_proposing`]), and the session is ready
    /// ([`ExtrudeSession::ready`]). Edits left with the solver
    /// are committed after it answers, and may change the regions picked,
    /// which are found again then: until they are, the preview isn't of
    /// what would be committed, and the extrude would come before them in
    /// the undo history.
    pub(crate) fn extrude_ready(&self) -> bool {
        self.mode.extrude().is_some_and(|session| {
            self.editable()
                && !self.users_proposing()
                && session.ready(&self.editor.document().design())
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
        let Some(session) = self.mode.extrude() else {
            return;
        };
        let Some(extrude) = session.extrude() else {
            return;
        };
        if self.commit_feature(session.feature, extrude.into()) {
            self.mode.take_operation();
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
        let Some(session) = self.mode.extrude_mut() else {
            return;
        };
        let is_kind = |kind: &FeatureKind| matches!(kind, FeatureKind::Extrude(_));
        let (feature, regions) = (session.feature, &mut session.regions);
        if !regions_session_stays(document, feature, regions, editable, replaced, is_kind) {
            self.mode.take_operation();
            return;
        }
        session.follow_design(document);
        session.targets.prune(document);
    }

    /// The extrude being set up as the regeneration lane previews it, and
    /// the extrude it edits, if it's whole.
    pub(crate) fn extrude_draft(&self) -> Option<(Option<FeatureId>, FeatureKind)> {
        let session = self.mode.extrude()?;
        Some((session.feature, session.extrude()?.into()))
    }

    /// The extrude being set up, for the view.
    pub(crate) fn extrude_state(&self) -> Option<ExtrudeState<'_>> {
        let session = self.mode.extrude()?;
        let document = self.editor.document();
        let candidates = session.regions.candidates(|id| self.placement(id));
        let uncut = (session.operation == OperationKind::Cut)
            .then(|| self.uncut_note())
            .flatten();
        Some(ExtrudeState {
            verdict: self.verdict(
                session.feature,
                self.extrude_ready(),
                session
                    .refused(&document.design())
                    .map(|refused| refused.to_string()),
                uncut,
            ),
            candidates,
            source: session.regions.source,
            picked: &session.regions.picked,
            missing: session.regions.missing,
            extent: session.extent,
            fields: (session.sides.fields.each_ref()).map(|field| field.field(self.params_in())),
            flip: session.sides.flip,
            taper: session.taper.field(self.params_in()),
            operation: session.operation,
            targets: self.body_targets(session.operation, session.feature, &session.targets),
            grabbed: session.grabbed,
            hover: self.panel_hover(),
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
