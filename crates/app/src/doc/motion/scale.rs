//! A scale in the move's session ([`MotionKind::Scale`]): its bodies
//! picked as a move's, the point it scales about (the origin to begin
//! with, from the toolbar, or picked on the model as an align's points
//! are: a corner, a straight edge's middle or a round edge's centre, at
//! the measure tool's snap dots), and how it scales: one factor, one per
//! world axis, or the factor that gives an edge of a scaled body a typed
//! length (the edge picked on the model, its length now shown beside it,
//! measured by the regeneration lane, and "Along its axis only" offered
//! for a straight edge along a world axis). Each is named as of the
//! feature ([`Naming`](varde_view::Naming)); what's picked is lit and
//! drawn on the model shown, found again by its names when that changes.

use std::borrow::Cow;

use glam::DVec3;
use varde_document::{AxisRef, BodyId, Document, EdgeRef, PointRef, Scale, ScaleFactor};
use varde_expr::{Unit, Value};
use varde_regen::{EdgeForm, Entity, InspectPick, Measure};
use varde_view::{
    MotionField, MotionKind, MotionPick, Pick, PickIndex, Picked, ScaleMode, ScaleView, Unnamed,
    axis_name, point_name, scale_info,
};

use super::align::{Mark, Taken, point_at, point_of};
use super::{Doc, MotionSession, OUT_OF_DATE, unnamed};
use crate::doc::regions::TypedText;

/// A scale's point and edge as picked, and where they were picked.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ScaleSetup {
    pub(crate) mode: ScaleMode,
    /// The point it scales about: the origin to begin with.
    pub(crate) about: PointRef,
    /// The edge an edge length scales to, once picked: on one of the
    /// bodies, named on the body holding it at the scale.
    pub(crate) edge: Option<EdgeRef>,
    /// An edge length's "Along its axis only".
    pub(crate) axis_only: bool,
    /// Where the point and the edge are on a model shown: where they
    /// were picked, or found again by their names on one shown since
    /// ([`ScaleSetup::follow`]).
    marks: [Option<Mark>; 2],
    /// Whether the point, and the edge, are gone: the document no longer
    /// takes them at the feature's place (an undo took their body or a
    /// face's maker away), or no longer holds their body. Kept, said to
    /// be gone, until picked again or back.
    gone: [bool; 2],
}

impl Default for ScaleSetup {
    fn default() -> Self {
        Self {
            mode: ScaleMode::Uniform,
            about: PointRef::Origin,
            edge: None,
            axis_only: false,
            marks: [None, None],
            gone: [false; 2],
        }
    }
}

impl ScaleSetup {
    /// The point and edge of `scale`, picked on no model shown.
    pub(super) fn of(scale: &Scale) -> Self {
        let (mode, edge, axis_only) = match &scale.factor {
            ScaleFactor::Uniform(_) => (ScaleMode::Uniform, None, false),
            ScaleFactor::PerAxis(_) => (ScaleMode::PerAxis, None, false),
            ScaleFactor::EdgeLength {
                edge, axis_only, ..
            } => (ScaleMode::EdgeLength, Some(*edge), *axis_only),
        };
        Self {
            mode,
            about: scale.about,
            edge,
            axis_only,
            ..Self::default()
        }
    }

    /// Finds the point and the edge again on `index`'s model where they
    /// aren't marked on it yet, by their names on the body `shown` says
    /// draws theirs there: so they stay lit and drawn as the model shown
    /// changes.
    fn follow(&mut self, index: &PickIndex, shown: impl Fn(BodyId) -> BodyId) {
        let model = index.model();
        let current = |mark: &Option<Mark>| mark.is_some_and(|mark| mark.model == model);
        if !current(&self.marks[0]) {
            self.marks[0] = Some(Taken::Point(self.about).found(index, &shown));
        }
        if !current(&self.marks[1]) {
            self.marks[1] = self.edge.map(|edge| Mark {
                model,
                target: (index.find_edge(shown(edge.body), edge.faces, edge.near))
                    .map(Picked::Edge),
                at: None,
            });
        }
    }

    /// The edge picked lit on `model`, if it's marked there.
    fn lit(&self, model: u64) -> Vec<Picked> {
        (self.marks[1].iter())
            .filter(|mark| mark.model == model && self.mode == ScaleMode::EdgeLength)
            .filter_map(|mark| mark.target)
            .collect()
    }

    /// Notes whether the point and the edge are gone from `document` at
    /// feature `index` (see [`ScaleSetup::gone`]).
    fn prune(&mut self, document: &Document, index: usize, bodies: &[BodyId]) {
        let design = document.design();
        let Ok(one) = Value::new("1", &Scale::factor_ask(&design)) else {
            return;
        };
        let held = |body: Option<BodyId>| body.is_none_or(|body| document.body(body).is_some());
        let point = Scale {
            bodies: bodies.to_vec(),
            about: self.about,
            factor: ScaleFactor::Uniform(one.clone()),
        };
        self.gone[0] =
            !held(self.about.body()) || document.check_scale_refs(index, &point).is_err();
        self.gone[1] = self.edge.is_some_and(|edge| {
            let named = Scale {
                bodies: bodies.to_vec(),
                about: PointRef::Origin,
                factor: ScaleFactor::EdgeLength {
                    edge,
                    length: one.clone(),
                    axis_only: false,
                },
            };
            !held(Some(edge.body)) || document.check_scale_refs(index, &named).is_err()
        });
    }
}

impl MotionSession {
    /// The scale as set up, if it's whole: its bodies, its point, and its
    /// factor, factors or edge length as they last read.
    pub(super) fn scale(&self) -> Option<Scale> {
        if self.bodies.is_empty() {
            return None;
        }
        let value = |field: MotionField| self.field(field).value.clone();
        let setup = &self.scale;
        let factor = match setup.mode {
            ScaleMode::Uniform => ScaleFactor::Uniform(value(MotionField::Factor)?),
            ScaleMode::PerAxis => ScaleFactor::PerAxis([
                value(MotionField::AxisFactor(varde_document::Axis3::X))?,
                value(MotionField::AxisFactor(varde_document::Axis3::Y))?,
                value(MotionField::AxisFactor(varde_document::Axis3::Z))?,
            ]),
            ScaleMode::EdgeLength => ScaleFactor::EdgeLength {
                edge: setup.edge?,
                length: value(MotionField::Length)?,
                axis_only: setup.axis_only,
            },
        };
        Some(Scale {
            bodies: self.bodies.clone(),
            about: setup.about,
            factor,
        })
    }

    /// The fields its mode reads.
    pub(super) fn scale_fields(&self) -> &'static [MotionField] {
        use varde_document::Axis3::{X, Y, Z};
        match self.scale.mode {
            ScaleMode::Uniform => &[MotionField::Factor],
            ScaleMode::PerAxis => &[
                MotionField::AxisFactor(X),
                MotionField::AxisFactor(Y),
                MotionField::AxisFactor(Z),
            ],
            ScaleMode::EdgeLength => &[MotionField::Length],
        }
    }

    /// What's still to be done before it can be committed, the words for
    /// the status bar: its bodies, a factor that scales, the edge on one
    /// of them and its length.
    pub(super) fn scale_need(&self) -> Option<&'static str> {
        if self.bodies.is_empty() {
            return Some("pick the bodies to scale");
        }
        let one = |field: &MotionField| {
            (self.field(*field).value.as_ref()).is_none_or(|value| value.value == 1.0)
        };
        match self.scale.mode {
            ScaleMode::Uniform | ScaleMode::PerAxis => {
                (self.scale_fields().iter().all(one)).then_some("enter a factor other than 1")
            }
            ScaleMode::EdgeLength => match self.scale.edge {
                None => Some("pick the edge to give a length"),
                Some(edge) if self.bodies.binary_search(&edge.body).is_err() => {
                    Some("pick an edge of a body it scales")
                }
                Some(_) if self.field(MotionField::Length).value.is_none() => {
                    Some("enter the length the edge is to have")
                }
                Some(_) => None,
            },
        }
    }

    /// The words for its point or edge being gone, if one is.
    pub(super) fn scale_gone(&self) -> Option<&'static str> {
        if self.scale.gone[0] {
            return Some("The point is gone: pick another");
        }
        (self.scale.gone[1] && self.scale.mode == ScaleMode::EdgeLength)
            .then_some("The edge is gone: pick another")
    }

    /// Notes what `document` no longer takes at feature `index`.
    pub(super) fn prune_scale(&mut self, document: &Document, index: usize) {
        let bodies = self.bodies.clone();
        self.scale.prune(document, index, &bodies);
    }

    /// Sets how it scales: an edge length with no edge picks one next.
    pub(super) fn scale_mode(&mut self, mode: ScaleMode) {
        self.scale.mode = mode;
        if mode == ScaleMode::EdgeLength && self.scale.edge.is_none() {
            self.picking = MotionPick::Edge;
        } else if self.picking == MotionPick::Edge {
            self.picking = MotionPick::Bodies;
        }
    }

    /// Takes the origin as its point, while the point is picked.
    pub(super) fn scale_origin(&mut self) {
        if self.picking != MotionPick::Point {
            return;
        }
        self.scale.about = PointRef::Origin;
        self.scale.marks[0] = None;
        self.scale.gone[0] = false;
        self.picking = MotionPick::Bodies;
    }
}

/// Why a pick can't be a scale's edge.
const NOT_AN_EDGE: &str = "Only an edge can be scaled to a length";

impl Doc {
    /// `pick` of the model shown as the point of the scale being set up,
    /// named as the feature stores it, as an align's point is: a corner,
    /// a straight edge's middle or a round edge's centre, on any body
    /// made before the scale. Refused, why, if not.
    pub(super) fn scale_point_of(&self, pick: Pick) -> Result<PointRef, Cow<'static, str>> {
        let naming = self.motion_naming().ok_or("Nothing is set up")?;
        let refused = |why: Unnamed, what: &str| unnamed(why, what, MotionKind::Scale);
        point_of(self.feed.pick_index(), &naming, pick, &refused)
    }

    /// `pick` of the model shown as the edge of the scale being set up:
    /// any edge, named as of the feature on the body holding it there,
    /// which must be one it scales (picked to scale if it scales none
    /// yet). Refused, why, if not.
    pub(super) fn scale_edge_of(&self, pick: Pick) -> Result<EdgeRef, Cow<'static, str>> {
        let session = self.motion.as_ref().ok_or("Nothing is set up")?;
        let Picked::Edge(edge) = pick.target else {
            return Err(NOT_AN_EDGE.into());
        };
        let naming = self.motion_naming().ok_or("Nothing is set up")?;
        let index = self.feed.pick_index();
        let named = naming
            .edge_ref(index, edge, pick.at)
            .map_err(|why| unnamed(why, "edge", MotionKind::Scale))?;
        let document = self.editor.document();
        let merged = self.feed.merged_before(document, session.feature);
        let held = merged.holder(named.body).unwrap_or(named.body);
        if !session.bodies.is_empty() && session.bodies.binary_search(&held).is_err() {
            return Err("Pick an edge of a body it scales".into());
        }
        if !super::super::combine::pickable(document, held, session.feature) {
            return Err(unnamed(Unnamed::Later, "body", MotionKind::Scale));
        }
        Ok(EdgeRef {
            body: held,
            ..named
        })
    }

    /// Takes `pick` as the point of the scale being set up, handing the
    /// clicks back to the bodies, or says why it can't be.
    pub(super) fn scale_point(&mut self, pick: Pick) -> Result<(), Cow<'static, str>> {
        if !self.feed.answers_request() || self.feed.predates_replacement() {
            return Err(OUT_OF_DATE.into());
        }
        let point = self.scale_point_of(pick)?;
        let index = self.feed.pick_index();
        let mark = Mark {
            model: index.model(),
            target: None,
            at: point_at(index, pick),
        };
        let Some(session) = &mut self.motion else {
            return Ok(());
        };
        session.scale.about = point;
        session.scale.marks[0] = Some(mark);
        session.scale.gone[0] = false;
        session.picking = MotionPick::Bodies;
        Ok(())
    }

    /// Takes `pick` as the edge of the scale being set up (its body as
    /// the one scaled if none is yet), handing the clicks back to the
    /// bodies, or says why it can't be.
    pub(super) fn scale_edge(&mut self, pick: Pick) -> Result<(), Cow<'static, str>> {
        if !self.feed.answers_request() || self.feed.predates_replacement() {
            return Err(OUT_OF_DATE.into());
        }
        let edge = self.scale_edge_of(pick)?;
        let model = self.feed.pick_index().model();
        let Some(session) = &mut self.motion else {
            return Ok(());
        };
        if session.bodies.is_empty() {
            session.bodies = vec![edge.body];
        }
        session.scale.edge = Some(edge);
        session.scale.marks[1] = Some(Mark {
            model,
            target: Some(pick.target),
            at: None,
        });
        session.scale.gone[1] = false;
        session.picking = MotionPick::Bodies;
        Ok(())
    }

    /// Finds the point and edge of the scale being set up again on the
    /// model shown, where they aren't marked yet, each on the body
    /// drawing its body there.
    pub(super) fn follow_scale(&mut self) {
        let Some(session) = &mut self.motion else {
            return;
        };
        if session.kind != MotionKind::Scale {
            return;
        }
        let shown = |body| self.feed.shown_body(body);
        session.scale.follow(self.feed.pick_index(), shown);
    }

    /// The edge of the scale being set up to light in the model shown.
    pub(super) fn scale_lit(&self) -> Vec<Picked> {
        match &self.motion {
            Some(session) if session.kind == MotionKind::Scale => {
                session.scale.lit(self.feed.model())
            }
            _ => Vec::new(),
        }
    }

    /// What the regeneration lane is asked to measure for the scale being
    /// set up: its edge, while it scales to one's length, on the body
    /// drawing its body in the model shown. Its length shows in the
    /// panel, and whether it's straight along a world axis offers "Along
    /// its axis only".
    pub(crate) fn scale_inspect(&self) -> Option<(InspectPick, Option<InspectPick>)> {
        let session = self.motion.as_ref()?;
        if session.kind != MotionKind::Scale || session.scale.mode != ScaleMode::EdgeLength {
            return None;
        }
        let edge = session.scale.edge?;
        let body = *self.shown_bodies(&[edge.body]).first()?;
        let pick = InspectPick {
            body,
            entity: Entity::Edge(edge.faces),
            near: edge.near.to_array(),
        };
        Some((pick, None))
    }

    /// The edge measured for the scale being set up, if the lane has
    /// answered for it (not for another edge picked before, or what's
    /// selected): its length and, for a straight one, its ends.
    fn scale_measured(&self) -> Option<(f64, Option<[DVec3; 2]>)> {
        let asked = self.scale_inspect()?;
        let probed = self.feed.inspected_of(asked)?.first.as_ref().ok()?;
        match probed.measure.as_ref().ok()? {
            Measure::Edge { length, shape, .. } => {
                let line = match shape {
                    EdgeForm::Line { from, to } => Some([DVec3::from(*from), DVec3::from(*to)]),
                    _ => None,
                };
                Some((*length, line))
            }
            _ => None,
        }
    }

    /// The length of the scale's edge now, as the features before it
    /// leave it: what the preview found, else (none previewed, or the
    /// preview failed and the model shows the edge unscaled) the edge as
    /// measured on the model shown.
    fn scale_length(&self) -> Option<f64> {
        let found = self.feed.draft_scale();
        let unscaled = found.is_none() || self.feed.draft_error().is_some();
        (found.and_then(|found| found.length)).or_else(|| {
            unscaled
                .then(|| self.scale_measured())
                .flatten()
                .map(|m| m.0)
        })
    }

    /// The panel's note of the scale being set up, if the preview scales
    /// fitted faces up: "3 fitted faces: their error grows × 25.4".
    pub(super) fn scale_note(&self, session: &MotionSession) -> Option<String> {
        if session.kind != MotionKind::Scale || session.need().is_some() {
            return None;
        }
        let found = self.feed.draft_scale()?;
        let largest = found.factors?.into_iter().fold(0.0, f64::max);
        if found.fitted == 0 || largest <= 1.0 {
            return None;
        }
        let grows = varde_expr::format(largest, None);
        Some(match found.fitted {
            1 => format!("1 fitted face: its error grows × {grows}"),
            n => format!("{n} fitted faces: their error grows × {grows}"),
        })
    }

    /// What's drawn of the scale being set up, and named in its panel.
    pub(super) fn scale_view(&self, session: &MotionSession) -> ScaleView<'_> {
        let document = self.editor.document();
        let setup = &session.scale;
        let model = self.feed.model();
        let picking = session.picking == MotionPick::Point;
        let found = (self.feed.draft_scale())
            .filter(|_| !picking)
            .and_then(|found| found.centre)
            .map(DVec3::from);
        let marked = (setup.marks[0])
            .filter(|mark| mark.model == model)
            .and_then(|mark| mark.at);
        let origin = match setup.about {
            PointRef::Origin => Some(DVec3::ZERO),
            _ => None,
        };
        let at = found.or(marked).or(origin);
        let units = document.units();
        let length = (setup.edge.is_some())
            .then(|| self.scale_length())
            .flatten()
            .map(|length| varde_expr::format(length, Some(Unit::Length(units))));
        let straight = (self.scale_measured())
            .and_then(|(_, line)| line)
            .is_some_and(|[from, to]| varde_regen::along_axis(to - from).is_some());
        let snaps = (self.pick.hover())
            .filter(|pick| picking && pick.model == model)
            .map(|pick| (self.feed.pick_index(), pick));
        ScaleView {
            mode: setup.mode,
            point: point_name(document, &setup.about),
            at,
            edge: (setup.edge).map(|edge| axis_name(document, &AxisRef::Edge(edge))),
            length,
            offered: setup.axis_only || (setup.edge.is_some() && straight),
            axis_only: setup.axis_only,
            info: session
                .scale()
                .map(|scale| scale_info(document, &scale, units)),
            snaps,
        }
    }
}

/// The length field as a new scale opens it: empty, asking for nothing
/// yet (not refused).
pub(super) fn empty_length() -> TypedText {
    TypedText {
        text: String::new(),
        value: None,
        error: None,
    }
}
