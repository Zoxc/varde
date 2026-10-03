//! Setting up a move or a mirror: its session, started by `Look::StartMove`
//! (`M`, the toolbar, the rail's Transform set) or `Look::StartMirror`
//! (the toolbar, the rail), or by editing one, picking its bodies as a
//! combine's (the body of what a click in the viewport is on, or a row
//! in Objects), a move's axis or a mirror's plane (an origin one from
//! the toolbar, or a model edge or face clicked, named as of the
//! feature), a move's offsets and angle typed, the preview through the
//! regeneration lane's drafts, and committing it as one undo step or
//! cancelling it, which leaves no trace.
//!
//! While the axis or plane is picked, the model shown is the history as
//! of the feature: a new one isn't previewed then, and an edited one is
//! previewed as a move that leaves its bodies where they are, so the
//! edges and faces clicked are where the feature finds them. A click is
//! taken only on the model answering what was asked last.

use std::borrow::Cow;
use std::sync::Arc;

use glam::DVec3;
use varde_document::{
    Axis3, AxisRef, BodyId, Design, Document, FeatureId, FeatureKind, MAX_FEATURE_BODIES, Mirror,
    Move, OriginPlane, PlaneRef,
};
use varde_expr::{AngleUnit, Unit, Value};
use varde_regen::Summary;
use varde_view::{
    CombineBody, ModelHighlight, MotionField, MotionKind, MotionLook, MotionPick, MotionState,
    Naming, PanelHover, Pick, Picked, Unnamed, axis_name, plane_name,
};

use super::combine::pickable;
use super::feed::Merges;
use super::regions::TypedText;
use super::{Doc, Focus};

/// The move or mirror being set up, while one is: [`Doc::motion`].
#[derive(Debug)]
pub(crate) struct MotionSession {
    pub(crate) kind: MotionKind,
    /// The feature edited, or `None` for a new one.
    pub(crate) feature: Option<FeatureId>,
    /// Sorted without repeats, as the document keeps them; at most
    /// [`MAX_FEATURE_BODIES`].
    pub(crate) bodies: Vec<BodyId>,
    /// What a click picks.
    pub(crate) picking: MotionPick,
    /// A move's fields: the offsets along X, Y and Z, then the angle.
    pub(crate) fields: [TypedText; 4],
    /// A move's axis: the Z axis to begin with, as the UI mock's. Only
    /// stored with an angle other than zero.
    pub(crate) axis: Option<AxisRef>,
    /// A mirror's plane, once picked.
    pub(crate) plane: Option<PlaneRef>,
    /// A mirror's Create copy: on to begin with, as the UI mock's.
    pub(crate) keep_original: bool,
    /// The panel's row the cursor is over, if any.
    pub(crate) hover: Option<PanelHover>,
    /// The bodies of the feature edited, which its preview leaves where
    /// they are while the axis or plane is picked with none picked.
    edited_bodies: Vec<BodyId>,
    /// The design as the fields' texts were last read, whose units bare
    /// lengths in them are in.
    design: Design,
    /// Built when what it's of changes, so the renderer uploads it only
    /// then.
    highlight: Arc<ModelHighlight>,
    /// What `highlight` was built of.
    built: Option<Built>,
}

/// What a move's or mirror's highlight is built of: the model, what's
/// hovered and whether its row in the panel is, what clicks pick, and
/// the bodies.
type Built = (u64, Option<(Picked, bool)>, MotionPick, Vec<BodyId>);

/// A value read for `ask` from `text`: one the asks take, as the fields
/// start with.
fn read(text: &str, ask: &varde_expr::Ask) -> TypedText {
    TypedText::read(text.to_owned(), ask)
}

impl MotionSession {
    /// A session setting up a new `kind` of `document`, of `bodies`: a
    /// move by nothing yet about the Z axis, or a mirror keeping the
    /// original with its plane to pick. Clicks pick bodies, or a
    /// mirror's plane once it has bodies.
    fn new(kind: MotionKind, document: &Document, mut bodies: Vec<BodyId>) -> Self {
        bodies.sort_unstable();
        bodies.dedup();
        bodies.truncate(MAX_FEATURE_BODIES);
        let design = document.design();
        let offset = Move::offset_ask(&design);
        let angle = Move::angle_ask(&design);
        let length = |design: &Design| varde_expr::format(0.0, Some(Unit::Length(design.units)));
        let zero = length(&design);
        let picking = match kind {
            MotionKind::Mirror if !bodies.is_empty() => MotionPick::Reference,
            _ => MotionPick::Bodies,
        };
        Self {
            kind,
            feature: None,
            bodies,
            picking,
            fields: [
                read(&zero, &offset),
                read(&zero, &offset),
                read(&zero, &offset),
                read(
                    &varde_expr::format(0.0, Some(Unit::Angle(AngleUnit::Deg))),
                    &angle,
                ),
            ],
            axis: (kind == MotionKind::Move).then_some(AxisRef::Origin(Axis3::Z)),
            plane: None,
            keep_original: true,
            hover: None,
            edited_bodies: Vec::new(),
            design,
            highlight: Arc::default(),
            built: None,
        }
    }

    /// A session editing the move or mirror `feature` of `document`, of
    /// `kind`, with its values; `None` if it's neither.
    fn editing(document: &Document, feature: FeatureId) -> Option<Self> {
        let design = document.design();
        let mut session = match &document.feature(feature)?.kind {
            FeatureKind::Move(moved) => {
                let mut session = Self::new(MotionKind::Move, document, moved.bodies.clone());
                let offset = Move::offset_ask(&design);
                for (field, value) in session.fields.iter_mut().zip(&moved.offset) {
                    *field = TypedText::of(value, &offset);
                }
                if let Some((axis, angle)) = &moved.turn {
                    session.axis = Some(*axis);
                    session.fields[MotionField::Angle.index()] =
                        TypedText::of(angle, &Move::angle_ask(&design));
                }
                session
            }
            FeatureKind::Mirror(mirror) => {
                let mut session = Self::new(MotionKind::Mirror, document, mirror.bodies.clone());
                session.plane = Some(mirror.plane);
                session.keep_original = mirror.keep_original;
                session.picking = MotionPick::Bodies;
                session
            }
            _ => return None,
        };
        session.feature = Some(feature);
        session.edited_bodies = session.bodies.clone();
        Some(session)
    }

    /// The field `field`'s text as last read.
    fn field(&self, field: MotionField) -> &TypedText {
        &self.fields[field.index()]
    }

    /// The angle as it last read, in radians: zero until one is typed.
    fn angle(&self) -> Option<f64> {
        let value = self.field(MotionField::Angle).value.as_ref()?;
        Some(value.value)
    }

    /// The feature as set up, if it's whole: bodies, and a move's offsets
    /// and angle as they last read with an axis if the angle isn't zero
    /// (none stored if it is), or a mirror's plane.
    fn kind(&self) -> Option<FeatureKind> {
        if self.bodies.is_empty() {
            return None;
        }
        match self.kind {
            MotionKind::Move => {
                let offset = |axis: Axis3| (self.field(MotionField::Offset(axis)).value).clone();
                let angle = self.field(MotionField::Angle).value.clone()?;
                let turn = if angle.value == 0.0 {
                    None
                } else {
                    Some((self.axis?, angle))
                };
                Some(FeatureKind::Move(Move {
                    bodies: self.bodies.clone(),
                    offset: [offset(Axis3::X)?, offset(Axis3::Y)?, offset(Axis3::Z)?],
                    turn,
                }))
            }
            MotionKind::Mirror => Some(FeatureKind::Mirror(Mirror {
                bodies: self.bodies.clone(),
                plane: self.plane?,
                keep_original: self.keep_original,
            })),
        }
    }

    /// What's still to be done before it can be committed, the UI mock's
    /// words, if anything.
    fn need(&self) -> Option<&'static str> {
        if self.bodies.is_empty() {
            return Some(match self.kind {
                MotionKind::Move => "pick the bodies to move",
                MotionKind::Mirror => "pick the bodies to mirror",
            });
        }
        match self.kind {
            MotionKind::Mirror if self.plane.is_none() => {
                Some("pick a plane: an origin plane or a planar face")
            }
            MotionKind::Mirror => None,
            MotionKind::Move => {
                let angle = self.angle().unwrap_or(0.0);
                if angle != 0.0 && self.axis.is_none() {
                    return Some("pick an axis to rotate about");
                }
                let offsets = [Axis3::X, Axis3::Y, Axis3::Z].map(|axis| {
                    (self.field(MotionField::Offset(axis)).value.as_ref())
                        .map_or(0.0, |value| value.value)
                });
                (angle == 0.0 && offsets.iter().all(|&offset| offset == 0.0))
                    .then_some("enter a distance or an angle")
            }
        }
    }

    /// Why it can't be committed to a document of `design` as set up, if
    /// its own check refuses it. None while it isn't whole.
    fn refused(&self, design: &Design) -> Option<String> {
        let refused = match self.kind()? {
            FeatureKind::Move(moved) => moved.check_own(design).err(),
            FeatureKind::Mirror(mirror) => mirror.check_own().err(),
            _ => None,
        };
        refused.map(|why| format!("it {why}"))
    }

    /// Whether it can be committed to a document of `design`: whole,
    /// with nothing still to do, no field refused, and passing its own
    /// check. What's left to the document, the bodies and the reference,
    /// the session keeps valid.
    fn ready(&self, design: &Design) -> bool {
        let typed = match self.kind {
            MotionKind::Move => self.fields.iter().all(|field| field.error.is_none()),
            MotionKind::Mirror => true,
        };
        typed && self.need().is_none() && self.kind().is_some() && self.refused(design).is_none()
    }

    /// Picks `body`, or takes it out if it's picked. None are added past
    /// the limit.
    fn toggle(&mut self, body: BodyId) {
        match self.bodies.binary_search(&body) {
            Ok(at) => {
                self.bodies.remove(at);
            }
            Err(at) if self.bodies.len() < MAX_FEATURE_BODIES => self.bodies.insert(at, body),
            Err(_) => {}
        }
    }

    /// Lets go of the bodies `document` no longer holds or that aren't
    /// made before the feature edited any more, and of an axis or plane
    /// on a body or of a face's maker the document no longer takes there
    /// (an undo took them away), so what's set up never names what the
    /// document can't hold: a move's axis goes back to the Z axis, a
    /// mirror's plane is picked again. An edited feature's own reference,
    /// whose body or maker is gone with ids below the next, the document
    /// holds, failing, and it stays.
    fn prune(&mut self, document: &Document) {
        let edited = self.feature;
        self.bodies.retain(|&body| pickable(document, body, edited));
        self.edited_bodies
            .retain(|&body| pickable(document, body, edited));
        let features = document.features();
        let Some(index) = (match edited {
            Some(id) => features.iter().position(|feature| feature.id == id),
            None => Some(features.len()),
        }) else {
            return;
        };
        if let Some(axis) = &self.axis
            && document.check_axis_ref(index, axis).is_err()
        {
            self.axis = Some(AxisRef::Origin(Axis3::Z));
        }
        if let Some(plane) = &self.plane
            && document.check_plane_ref(index, plane).is_err()
        {
            self.plane = None;
            self.picking = MotionPick::Reference;
        }
    }

    /// Moves the bodies `merges` (the merges before the feature) have
    /// merged into others on to the bodies holding them, as a click on
    /// one picks, as a combine's ([`super::CombineSession`]): whether any
    /// moved.
    fn follow(&mut self, merges: &Merges) -> bool {
        let held = |body: BodyId| merges.holder(body).unwrap_or(body);
        let mut bodies: Vec<BodyId> = self.bodies.iter().map(|&body| held(body)).collect();
        bodies.sort_unstable();
        bodies.dedup();
        let moved = bodies != self.bodies;
        self.bodies = bodies;
        moved
    }

    /// Keeps each value where the design's units changed since its field
    /// was read, as the document does its own.
    fn follow_units(&mut self, document: &Document) {
        let design = document.design();
        if design == self.design {
            return;
        }
        let asks = [
            Move::offset_ask(&self.design),
            Move::offset_ask(&self.design),
            Move::offset_ask(&self.design),
            Move::angle_ask(&self.design),
        ];
        for (field, ask) in self.fields.iter_mut().zip(&asks) {
            field.follow_units(ask);
        }
        self.design = design;
    }

    /// The draft previewing it while the axis or plane isn't picked: the
    /// feature as set up, if it's whole (a new one only once it does
    /// something: [`MotionSession::need`]). While it is, an edited feature
    /// is previewed as a move leaving its bodies where they are (turning
    /// them by nothing about a move's axis, so where it is shows), and a
    /// new one not at all: the model shown is the history as of the
    /// feature, which the edges and faces clicked are named as.
    fn draft(&self, design: &Design) -> Option<(Option<FeatureId>, FeatureKind)> {
        if self.picking == MotionPick::Bodies {
            // A new one that does nothing yet shows as the document does.
            if self.feature.is_none() && self.need().is_some() {
                return None;
            }
            return Some((self.feature, self.kind()?));
        }
        let feature = self.feature?;
        let bodies = if self.bodies.is_empty() {
            self.edited_bodies.clone()
        } else {
            self.bodies.clone()
        };
        if bodies.is_empty() {
            return None;
        }
        let zero = || Value::new("0", &Move::offset_ask(design)).ok();
        let turn = match (self.kind, self.axis) {
            (MotionKind::Move, Some(axis)) => {
                Some((axis, Value::new("0", &Move::angle_ask(design)).ok()?))
            }
            _ => None,
        };
        Some((
            Some(feature),
            FeatureKind::Move(Move {
                bodies,
                offset: [zero()?, zero()?, zero()?],
                turn,
            }),
        ))
    }
}

/// Why a model edge or face can't be a move's axis or a mirror's plane,
/// in the words the status bar shows.
const NOT_AN_AXIS: &str = "Only a straight or round edge, or a round face, can be the axis";
const NOT_A_PLANE: &str = "Only a flat face can be the mirror plane";
const OUT_OF_DATE: &str = "The model shown is out of date: try again once it's regenerated";

impl Doc {
    /// Starts setting up a new move or mirror (`kind`), in a document
    /// that can be changed and outside a sketch, or cancels the one being
    /// set up (one of the other kind is replaced). What's selected in the
    /// model gives it its bodies. Another operation being set up is
    /// dropped. A move's first offset field takes the focus.
    pub(crate) fn start_motion(&mut self, kind: MotionKind) {
        if (self.motion.take()).is_some_and(|session| session.kind == kind)
            || !self.editable()
            || self.sketch.is_some()
        {
            return;
        }
        self.picking_plane = None;
        self.extrude = None;
        self.revolve = None;
        self.combine = None;
        let document = self.editor.document();
        let merged = self.feed.merged_before(document, None);
        let mut bodies: Vec<BodyId> = Vec::new();
        for item in self.pick.selection.items() {
            let body = item.body();
            let body = merged.holder(body).unwrap_or(body);
            if pickable(document, body, None) && !bodies.contains(&body) {
                bodies.push(body);
            }
        }
        self.motion = Some(MotionSession::new(kind, document, bodies));
        if kind == MotionKind::Move {
            self.focus = Some(Focus::All);
        }
    }

    /// Edits the move or mirror feature `id`, if the document holds it,
    /// in a session with its values, outside a sketch, in a document that
    /// can be changed. Another operation being set up is dropped.
    pub(crate) fn edit_motion(&mut self, id: FeatureId) {
        if self.sketch.is_some() || !self.editable() {
            return;
        }
        let Some(session) = MotionSession::editing(self.editor.document(), id) else {
            return;
        };
        self.picking_plane = None;
        self.extrude = None;
        self.revolve = None;
        self.combine = None;
        self.selected_feature = Some(id);
        if session.kind == MotionKind::Move {
            self.focus = Some(Focus::All);
        }
        self.motion = Some(session);
    }

    /// Takes `message`, changing the move or mirror being set up.
    pub(crate) fn motion_look(&mut self, message: MotionLook) {
        let editable = self.editable();
        let document = self.editor.document();
        let Some(session) = &mut self.motion else {
            return;
        };
        match message {
            MotionLook::Cancel => self.motion = None,
            _ if !editable => {}
            MotionLook::Picking(picking) => session.picking = picking,
            MotionLook::Drop(body) => {
                session.bodies.retain(|&picked| picked != body);
            }
            MotionLook::Input { field, text } => {
                let design = document.design();
                let ask = match field {
                    MotionField::Offset(_) => Move::offset_ask(&design),
                    MotionField::Angle => Move::angle_ask(&design),
                };
                session.fields[field.index()].input(text, &ask);
            }
            MotionLook::Turn {
                axis,
                angle,
                offset,
            } if session.kind == MotionKind::Move => {
                let design = document.design();
                session.axis = Some(AxisRef::Origin(axis));
                (session.fields[MotionField::Angle.index()])
                    .input(angle, &Move::angle_ask(&design));
                let ask = Move::offset_ask(&design);
                for (axis, text) in Axis3::ALL.into_iter().zip(offset) {
                    session.fields[MotionField::Offset(axis).index()].input(text, &ask);
                }
            }
            MotionLook::Turn { .. } => {}
            MotionLook::OriginAxis(axis) if session.kind == MotionKind::Move => {
                session.axis = Some(AxisRef::Origin(axis));
                session.picking = MotionPick::Bodies;
            }
            MotionLook::OriginPlane(plane) if session.kind == MotionKind::Mirror => {
                session.plane = Some(PlaneRef::Origin(plane));
                session.picking = MotionPick::Bodies;
            }
            MotionLook::OriginAxis(_) | MotionLook::OriginPlane(_) => {}
            MotionLook::Copy => session.keep_original = !session.keep_original,
        }
    }

    /// Takes a click on the model while a move or mirror is set up: the
    /// body of what it's on while bodies are picked
    /// ([`Doc::motion_body`]), else the edge or face as the axis or plane
    /// ([`Doc::motion_reference`]). A click on nothing, or on a model no
    /// longer shown, does nothing.
    pub(crate) fn motion_click(&mut self, pick: Option<Pick>) {
        let Some(pick) = pick.filter(|pick| pick.model == self.feed.model()) else {
            return;
        };
        let Some(session) = &self.motion else {
            return;
        };
        if !self.picks() || !self.editable() {
            return;
        }
        match session.picking {
            MotionPick::Bodies => self.motion_body(pick.body),
            MotionPick::Reference => match self.motion_reference(pick) {
                Ok(()) => {}
                Err(why) => self.notice = Some(why.into_owned()),
            },
        }
    }

    /// Picks `body` for the move or mirror being set up, or takes it out:
    /// a body merged into another before the feature as the one holding
    /// it, which the model draws it as. Only bodies the feature can name,
    /// in a document that can be changed.
    pub(crate) fn motion_body(&mut self, body: BodyId) {
        if !self.editable() {
            return;
        }
        let document = self.editor.document();
        let Some(session) = &mut self.motion else {
            return;
        };
        let merged = self.feed.merged_before(document, session.feature);
        let body = merged.holder(body).unwrap_or(body);
        if pickable(document, body, session.feature) {
            session.toggle(body);
        }
    }

    /// Takes `pick` as the axis or plane of the move or mirror being set
    /// up ([`Doc::reference_of`]), handing the clicks back to bodies, or
    /// says why it can't be.
    fn motion_reference(&mut self, pick: Pick) -> Result<(), Cow<'static, str>> {
        if !self.feed.answers_request() || self.feed.predates_replacement() {
            return Err(OUT_OF_DATE.into());
        }
        let reference = self.reference_of(pick.target, pick.at)?;
        let Some(session) = &mut self.motion else {
            return Ok(());
        };
        match reference {
            Reference::Axis(axis) => session.axis = Some(axis),
            Reference::Plane(plane) => session.plane = Some(plane),
        }
        session.picking = MotionPick::Bodies;
        Ok(())
    }

    /// `target` of the model shown, picked at `at`, as the axis or plane
    /// of the move or mirror being set up, named as the feature stores it
    /// ([`Naming`], the history stopped at the feature): for a move a
    /// straight or round edge, or a round face (a cylinder's, cone's,
    /// torus's or other surface of revolution's); for a mirror a flat
    /// face. Refused, why, if it's none of those, made by the feature or
    /// a later one, or on a body which can't be told there.
    fn reference_of(&self, target: Picked, at: DVec3) -> Result<Reference, Cow<'static, str>> {
        let session = self.motion.as_ref().ok_or("Nothing is set up")?;
        let index = self.feed.pick_index();
        let document = self.editor.document();
        let features = document.features();
        let before = (session.feature)
            .and_then(|id| features.iter().position(|feature| feature.id == id))
            .unwrap_or(features.len());
        let naming = Naming::before(document, before, self.shown());
        let noun = session.kind.noun().to_lowercase();
        let refused = |why: Unnamed, what: &str| -> Cow<'static, str> {
            match why {
                Unnamed::Missing => format!("That {what} isn't in the model").into(),
                Unnamed::Later => {
                    let article = if what.starts_with('e') { "an" } else { "a" };
                    format!("Only {article} {what} made before the {noun} can be picked").into()
                }
                Unnamed::Unclear => format!(
                    "Which body that {what} is on at the {noun} can't be told: pick another"
                )
                .into(),
            }
        };
        let summary =
            |face: u32| (index.picking().faces().get(face as usize)).map(|face| face.summary);
        match (session.kind, target) {
            (MotionKind::Move, Picked::Edge(edge)) => {
                let keys = index.chain_keys(edge).ok_or(NOT_AN_AXIS)?;
                let straight = index.edge_ends(edge, &keys).is_some();
                let round = round_edge(index, edge);
                if !(straight || round) {
                    return Err(NOT_AN_AXIS.into());
                }
                let edge = naming
                    .edge_ref(index, edge, at)
                    .map_err(|why| refused(why, "edge"))?;
                Ok(Reference::Axis(AxisRef::Edge(edge)))
            }
            (MotionKind::Move, Picked::Face(face)) => {
                let round = matches!(
                    summary(face),
                    Some(
                        Summary::Cylinder { .. }
                            | Summary::Cone { .. }
                            | Summary::Torus { .. }
                            | Summary::Revolved { .. }
                    )
                );
                if !round {
                    return Err(NOT_AN_AXIS.into());
                }
                let face = naming
                    .checked_face_ref(index, face, at)
                    .map_err(|why| refused(why, "face"))?;
                Ok(Reference::Axis(AxisRef::Face(face)))
            }
            (MotionKind::Mirror, Picked::Face(face)) => {
                if !matches!(summary(face), Some(Summary::Plane { .. })) {
                    return Err(NOT_A_PLANE.into());
                }
                let face = naming
                    .checked_face_ref(index, face, at)
                    .map_err(|why| refused(why, "face"))?;
                Ok(Reference::Plane(PlaneRef::Face(face)))
            }
            (MotionKind::Move, _) => Err(NOT_AN_AXIS.into()),
            (MotionKind::Mirror, _) => Err(NOT_A_PLANE.into()),
        }
    }

    /// Whether the move or mirror being set up can be committed: the
    /// document can be changed, no sketch edits wait on the solver, and
    /// the session is ready ([`MotionSession::ready`]).
    pub(crate) fn motion_ready(&self) -> bool {
        let design = self.editor.document().design();
        self.motion
            .as_ref()
            .is_some_and(|session| self.editable() && !self.proposing() && session.ready(&design))
    }

    /// Adds the move or mirror being set up, or changes the one edited,
    /// as one undo step, and ends the session: if it's ready
    /// ([`Doc::motion_ready`]), its preview failed only if `accept`
    /// ([`Doc::commit_by`]), and the document takes it. Refused, the
    /// session stays, and why shows.
    pub(crate) fn commit_motion(&mut self, accept: bool) {
        if !self.commit_by(self.motion_ready(), accept) {
            return;
        }
        let Some(session) = &self.motion else {
            return;
        };
        let Some(kind) = session.kind() else {
            return;
        };
        if self.commit_feature(session.feature, kind) {
            self.motion = None;
        }
    }

    /// Ends the move or mirror session if the feature edited is gone, the
    /// document can't be changed any more, or it was replaced whole
    /// (`replaced`); lets go of what it can't name any more, keeps its
    /// values where the units changed, and follows bodies merged since.
    pub(crate) fn prune_motion(&mut self, replaced: bool) {
        let editable = self.editable();
        let document = self.editor.document();
        let Some(session) = &mut self.motion else {
            return;
        };
        let edited = session.feature.is_none_or(|feature| {
            matches!(
                (session.kind, document.feature(feature).map(|f| &f.kind)),
                (MotionKind::Move, Some(FeatureKind::Move(_)))
                    | (MotionKind::Mirror, Some(FeatureKind::Mirror(_)))
            )
        });
        if !(editable && !replaced && edited) {
            self.motion = None;
            return;
        }
        session.prune(document);
        session.follow_units(document);
        self.follow_motion_merges();
    }

    /// Moves the bodies of the move or mirror being set up on to the
    /// bodies holding them where the model shown has them merged before
    /// it ([`MotionSession::follow`]): whether they moved.
    pub(crate) fn follow_motion_merges(&mut self) -> bool {
        let Some(session) = &self.motion else {
            return false;
        };
        let merges = (self.feed).merged_before(self.editor.document(), session.feature);
        (self.motion.as_mut()).is_some_and(|session| session.follow(&merges))
    }

    /// The move or mirror being set up as the regeneration lane previews
    /// it, see [`MotionSession::draft`].
    pub(crate) fn motion_draft(&self) -> Option<(Option<FeatureId>, FeatureKind)> {
        let session = self.motion.as_ref()?;
        session.draft(&self.editor.document().design())
    }

    /// The edge or face hovered, if it's one a click takes as the axis or
    /// plane: lit as the cursor's over it.
    fn takes_reference(&self, pick: Pick) -> bool {
        (self.feed.answers_request() && !self.feed.predates_replacement())
            && self.reference_of(pick.target, pick.at).is_ok()
    }

    /// Rebuilds the move's or mirror's highlight if what it's of changed:
    /// its bodies' faces as selected; while bodies are picked the body
    /// hovered, and while the axis or plane is the edge or face hovered
    /// if a click takes it, as hovered; a body whose row in the panel is
    /// hovered lit, even one picked, which in the view keeps its colour
    /// under the cursor.
    pub(crate) fn refresh_motion_highlight(&mut self) {
        if !self.picks() {
            return;
        }
        let Some(session) = &self.motion else {
            return;
        };
        let panel = self.panel_hover().and_then(PanelHover::body);
        let hovered: Option<(Picked, bool)> = match (panel, self.pick.hover()) {
            (Some(body), _) => {
                let index = self.feed.pick_index();
                index
                    .body_faces(body)
                    .next()
                    .map(|face| (Picked::Face(face), true))
            }
            (None, Some(pick)) => match session.picking {
                MotionPick::Bodies => {
                    (!session.bodies.contains(&pick.body)).then_some((pick.target, false))
                }
                MotionPick::Reference => self.takes_reference(pick).then_some((pick.target, false)),
            },
            (None, None) => None,
        };
        let key = (
            self.feed.model(),
            hovered,
            session.picking,
            session.bodies.clone(),
        );
        if session.built.as_ref() == Some(&key) {
            return;
        }
        let index = self.feed.pick_index();
        let faces = |body: BodyId| index.body_faces(body).map(Picked::Face);
        let lit = hovered
            .filter(|&(_, panel)| panel)
            .and_then(|(target, _)| index.body(target));
        let picked: Vec<Picked> = (session.bodies.iter())
            .filter(|&&body| Some(body) != lit)
            .flat_map(|&body| faces(body))
            .collect();
        let hover: Vec<Picked> = match (hovered, session.picking) {
            (Some((target, true)), _) | (Some((target, false)), MotionPick::Bodies) => index
                .body(target)
                .map(|body| faces(body).collect())
                .unwrap_or_default(),
            (Some((target, false)), MotionPick::Reference) => vec![target],
            (None, _) => Vec::new(),
        };
        let highlight = Arc::new(index.highlight_with(&hover, &picked, &[]));
        if let Some(session) = &mut self.motion {
            session.highlight = highlight;
            session.built = Some(key);
        }
    }

    /// The move's or mirror's highlight, if one is set up and it's built
    /// for the model shown.
    pub(crate) fn motion_highlight(&self) -> Option<&Arc<ModelHighlight>> {
        let session = self.motion.as_ref()?;
        let current = (session.built.as_ref()).is_some_and(|built| built.0 == self.feed.model());
        Some(&session.highlight).filter(|highlight| current && !highlight.is_empty())
    }

    /// The move or mirror being set up, for the view.
    pub(crate) fn motion_state(&self) -> Option<MotionState<'_>> {
        let session = self.motion.as_ref()?;
        let document = self.editor.document();
        let design = document.design();
        let named = |body: BodyId| {
            Some(CombineBody {
                body,
                name: document.body(body)?.name.as_str(),
            })
        };
        let editing = session
            .feature
            .and_then(|feature| document.feature(feature))
            .map(|feature| feature.name.as_str());
        let (reference, origin) = match session.kind {
            MotionKind::Move => {
                let axis = session.axis.as_ref();
                let origin = axis.and_then(|axis| match axis {
                    AxisRef::Origin(axis) => Some([DVec3::ZERO, axis.direction()]),
                    _ => None,
                });
                (axis.map(|axis| axis_name(document, axis)), origin)
            }
            MotionKind::Mirror => {
                let plane = session.plane.as_ref();
                let origin = plane.and_then(|plane| match plane {
                    PlaneRef::Origin(plane) => Some([DVec3::ZERO, origin_normal(*plane)]),
                    _ => None,
                });
                (plane.map(|plane| plane_name(document, plane)), origin)
            }
        };
        // A move turning by nothing doesn't show its axis unless it's
        // being picked.
        let turning = session.kind == MotionKind::Mirror
            || session.picking == MotionPick::Reference
            || session.angle().is_some_and(|angle| angle != 0.0);
        let line = origin
            .or_else(|| self.feed.draft_reference())
            .filter(|_| turning && reference.is_some());
        // The bodies where the model shown has them: a merged one in its
        // holder.
        let merged = self.feed.merged_bodies();
        let shown: Vec<BodyId> = (session.bodies.iter())
            .map(|&body| {
                (merged.iter())
                    .find(|(consumed, _)| *consumed == body)
                    .map_or(body, |&(_, holder)| holder)
            })
            .collect();
        let bounds = self.feed.pick_index().bodies_bounds(&shown);
        let fields = MotionField::ALL.map(|field| session.field(field).field());
        Some(MotionState {
            kind: session.kind,
            editing,
            bodies: session
                .bodies
                .iter()
                .filter_map(|&body| named(body))
                .collect(),
            picking: session.picking,
            fields,
            reference,
            line,
            bounds,
            origin_axis: match session.axis {
                Some(AxisRef::Origin(axis)) => Some(axis),
                _ => None,
            },
            units: document.units(),
            keep_original: session.keep_original,
            need: session.need(),
            refused: session.refused(&design),
            error: self.feed.draft_error(),
            show_error: self.draft_framed(),
            checking: self.proposals.slow(),
            ready: self.commit_by(self.motion_ready(), false),
            accept: self.commit_by(self.motion_ready(), true),
            editable: self.editable(),
            hover: self.panel_hover(),
        })
    }
}

/// Whether `edge` of `index`'s model is round as the mesh draws it: a
/// circle or an arc of one, every point of its polyline as far from its
/// snap point (a round edge's centre) within the rounding of the mesh's
/// `f32` points and a millionth of its radius; not an ellipse, which
/// regenerating refuses as an axis. Regenerating decides from the exact
/// curves.
fn round_edge(index: &varde_view::PickIndex, edge: u32) -> bool {
    let Some(Some(centre)) = index.picking().snaps().get(edge as usize) else {
        return false;
    };
    let centre = DVec3::from(*centre);
    let mesh = index.mesh();
    let Some(polyline) = mesh.polyline(edge as usize) else {
        return false;
    };
    let points: Vec<DVec3> = (polyline.iter())
        .filter_map(|&i| mesh.positions().get(i as usize))
        .map(|&p| glam::Vec3::from(p).as_dvec3())
        .collect();
    let radii = points.iter().map(|p| p.distance(centre));
    let (low, high) = radii.fold((f64::INFINITY, 0.0_f64), |(low, high), r| {
        (low.min(r), high.max(r))
    });
    let scale = (points.iter())
        .map(|p| p.abs().max_element())
        .fold(centre.abs().max_element(), f64::max);
    let slack = 1e-6 * high + 8.0 * f64::from(f32::EPSILON) * scale;
    points.len() >= 2 && low > slack && high - low <= slack
}

/// The normal of the origin plane `plane`: the world axis square to it.
fn origin_normal(plane: OriginPlane) -> DVec3 {
    plane.placement().normal
}

/// An edge or face taken as a move's axis or a mirror's plane.
enum Reference {
    Axis(AxisRef),
    Plane(PlaneRef),
}

#[cfg(test)]
mod tests;
