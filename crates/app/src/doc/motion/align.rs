//! An align in the move's session ([`MotionKind::Align`]): its body
//! picked as a move's (one, replaced by a click on another), then each
//! side's point, direction and second direction, picked on the model in
//! that order (the moved body's point, the target's, the moved body's
//! direction, the target's; a second direction only from its field), the
//! points with the measure tool's snap dots (a corner, a straight edge's
//! middle, a round edge's centre), the directions on faces and edges
//! (a flat face's normal, a round face's axis, a straight edge's
//! direction, a round edge's axis), each named as of the feature
//! ([`Naming`]). A round edge's centre picked for a point also gives
//! that side its direction, the rim's axis, while it has none: a pin's
//! rim and a hole's then align the pin into the hole in two clicks. The
//! target side may be the origin and its axes, from the toolbar.

use std::borrow::Cow;

use glam::DVec3;
use varde_document::{Align, AlignRefs, AxisRef, BodyId, DirRef, Document, PointRef};
use varde_regen::{AlignDatums, Summary};
use varde_view::{
    AlignMark, AlignRole, AlignSide, AlignSlot, AlignView, MotionField, MotionKind, MotionPick,
    Naming, Pick, PickIndex, Picked, Snapped, Unnamed, align_info, direction_name, point_name,
};

use super::{Doc, MotionSession, OUT_OF_DATE};

/// Why a pick can't be an align's point or direction, in the words the
/// status bar shows.
pub(super) const NOT_A_POINT: &str =
    "Only a corner, a straight edge's middle or a round edge's centre can be the point";
pub(super) const NOT_A_DIRECTION: &str =
    "Only a flat or round face, or a straight or round edge, can give the direction";

/// An align's references as picked so far, and where they were picked.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct AlignSetup {
    /// The moved side's, then the target's ([`AlignSide::index`]).
    pub(crate) sides: [Side; 2],
    /// Where each reference was picked on the model shown, by side and
    /// role: drawn and lit while that model is shown.
    marks: [[Option<Mark>; 3]; 2],
    /// The references the document no longer takes at the feature's place
    /// (an undo took their body or a face's maker away), or on a body it
    /// no longer holds: kept, said to be gone, until picked again or back.
    gone: Vec<AlignSlot>,
}

/// One side of an align as picked: its point, direction and second
/// direction, each if picked.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Side {
    pub(crate) point: Option<PointRef>,
    pub(crate) primary: Option<DirRef>,
    pub(crate) secondary: Option<DirRef>,
}

/// A reference picked for one of an align's slots.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Taken {
    Point(PointRef),
    Direction(DirRef),
}

impl Taken {
    /// The body it's on, if it isn't the origin's.
    fn body(&self) -> Option<BodyId> {
        match self {
            Taken::Point(point) => point.body(),
            Taken::Direction(direction) => direction.body(),
        }
    }
}

/// Where a reference was picked: on the model `model` shows, `target`
/// (lit, for a direction) and the point (drawn, for a point).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Mark {
    model: u64,
    target: Option<Picked>,
    at: DVec3,
}

impl Side {
    /// Its references as an align stores them, if it has a point.
    fn refs(&self) -> Option<AlignRefs> {
        Some(AlignRefs {
            point: self.point?,
            primary: self.primary,
            secondary: self.secondary,
        })
    }

    /// Whether `role` is picked.
    fn has(&self, role: AlignRole) -> bool {
        match role {
            AlignRole::Point => self.point.is_some(),
            AlignRole::Primary => self.primary.is_some(),
            AlignRole::Secondary => self.secondary.is_some(),
        }
    }

    /// Takes `role` out.
    fn clear(&mut self, role: AlignRole) {
        match role {
            AlignRole::Point => self.point = None,
            AlignRole::Primary => self.primary = None,
            AlignRole::Secondary => self.secondary = None,
        }
    }

    /// Each reference picked, by role, as the document checks one on its
    /// own: a point alone, or a direction with the origin as the point.
    fn each(&self) -> Vec<(AlignRole, AlignRefs)> {
        let alone = |direction: Option<DirRef>| {
            direction.map(|direction| AlignRefs {
                point: PointRef::Origin,
                primary: Some(direction),
                secondary: None,
            })
        };
        let point = self.point.map(|point| AlignRefs {
            point,
            primary: None,
            secondary: None,
        });
        [
            (AlignRole::Point, point),
            (AlignRole::Primary, alone(self.primary)),
            (AlignRole::Secondary, alone(self.secondary)),
        ]
        .into_iter()
        .filter_map(|(role, refs)| Some((role, refs?)))
        .collect()
    }
}

impl AlignSetup {
    /// The references of `align`, picked on no model shown.
    pub(super) fn of(align: &Align) -> Self {
        let side = |refs: &AlignRefs| Side {
            point: Some(refs.point),
            primary: refs.primary,
            secondary: refs.secondary,
        };
        Self {
            sides: [side(&align.from), side(&align.to)],
            ..Self::default()
        }
    }

    fn side(&self, side: AlignSide) -> &Side {
        &self.sides[side.index()]
    }

    /// Picks `taken` for `slot`, picked on `mark`.
    fn set(&mut self, slot: AlignSlot, taken: Taken, mark: Option<Mark>) {
        let side = &mut self.sides[slot.side.index()];
        match (slot.role, taken) {
            (AlignRole::Point, Taken::Point(point)) => side.point = Some(point),
            (AlignRole::Primary, Taken::Direction(direction)) => side.primary = Some(direction),
            (AlignRole::Secondary, Taken::Direction(direction)) => {
                side.secondary = Some(direction);
            }
            _ => return,
        }
        self.marks[slot.side.index()][slot.role.index()] = mark;
        self.gone.retain(|&gone| gone != slot);
    }

    /// Takes `slot` out.
    pub(super) fn clear(&mut self, slot: AlignSlot) {
        self.sides[slot.side.index()].clear(slot.role);
        self.marks[slot.side.index()][slot.role.index()] = None;
        self.gone.retain(|&gone| gone != slot);
    }

    /// Takes out what's picked on the moved side on another body than
    /// `body`, and on the target side on `body`: picking another body to
    /// move leaves neither side naming the wrong one.
    fn moved_to(&mut self, body: BodyId) {
        for role in AlignRole::ALL {
            let moved = AlignSlot::new(AlignSide::Moved, role);
            if self
                .taken(moved)
                .is_some_and(|taken| taken.body() != Some(body))
            {
                self.clear(moved);
            }
            let target = AlignSlot::new(AlignSide::Target, role);
            if self
                .taken(target)
                .is_some_and(|taken| taken.body() == Some(body))
            {
                self.clear(target);
            }
        }
    }

    /// What's picked for `slot`, if anything.
    fn taken(&self, slot: AlignSlot) -> Option<Taken> {
        let side = self.side(slot.side);
        match slot.role {
            AlignRole::Point => side.point.map(Taken::Point),
            AlignRole::Primary => side.primary.map(Taken::Direction),
            AlignRole::Secondary => side.secondary.map(Taken::Direction),
        }
    }

    /// What a click picks next once one is picked: the first of the
    /// moved body's point, the target's, the moved body's direction and
    /// the target's still to pick, then a second direction where the
    /// other side has one; else nothing (another field clicked picks
    /// again).
    pub(super) fn next(&self) -> MotionPick {
        use AlignRole::{Point, Primary, Secondary};
        use AlignSide::{Moved, Target};
        let order = [
            (Moved, Point),
            (Target, Point),
            (Moved, Primary),
            (Target, Primary),
        ];
        if let Some(&(side, role)) = order
            .iter()
            .find(|&&(side, role)| !self.side(side).has(role))
        {
            return MotionPick::Align(AlignSlot::new(side, role));
        }
        let [moved, target] = [Moved, Target].map(|side| self.side(side).has(Secondary));
        match (moved, target) {
            (true, false) => MotionPick::Align(AlignSlot::new(Target, Secondary)),
            (false, true) => MotionPick::Align(AlignSlot::new(Moved, Secondary)),
            _ => MotionPick::Nothing,
        }
    }

    /// What's still to be picked before it's whole, the words for the
    /// status bar, if anything: the points, and directions paired.
    fn need(&self) -> Option<&'static str> {
        let [moved, target] = &self.sides;
        if moved.point.is_none() {
            return Some("pick a point on the body: a corner, an edge's middle or a rim's centre");
        }
        if target.point.is_none() {
            return Some("pick the point to align it to");
        }
        match (moved.primary.is_some(), target.primary.is_some()) {
            (true, false) => return Some("pick the direction to align it to"),
            (false, true) => return Some("pick a direction on the body"),
            _ => {}
        }
        match (moved.secondary.is_some(), target.secondary.is_some()) {
            (true, false) => Some("pick the second direction to align it to"),
            (false, true) => Some("pick a second direction on the body"),
            (true, true) if moved.primary.is_none() => {
                Some("pick a direction on each side before a second one")
            }
            _ => None,
        }
    }

    /// Notes the references `document` no longer takes at feature
    /// `index`, or whose body it no longer holds.
    fn prune(&mut self, document: &Document, index: usize) {
        let mut gone = Vec::new();
        for side in [AlignSide::Moved, AlignSide::Target] {
            for (role, refs) in self.side(side).each() {
                let held = (refs.point.body().into_iter())
                    .chain(refs.directions().filter_map(DirRef::body))
                    .all(|body| document.body(body).is_some());
                if !held || document.check_align_refs(index, &refs).is_err() {
                    gone.push(AlignSlot::new(side, role));
                }
            }
        }
        self.gone = gone;
    }

    /// The first reference picked that's gone, the UI mock's words for it.
    fn gone(&self) -> Option<&'static str> {
        let slot = self.gone.first()?;
        Some(match (slot.side, slot.role) {
            (AlignSide::Moved, AlignRole::Point) => "The point on the body is gone: pick another",
            (AlignSide::Moved, AlignRole::Primary) => {
                "The direction on the body is gone: pick another"
            }
            (AlignSide::Moved, AlignRole::Secondary) => {
                "The second direction on the body is gone: pick another"
            }
            (AlignSide::Target, AlignRole::Point) => {
                "The point it's aligned to is gone: pick another"
            }
            (AlignSide::Target, AlignRole::Primary) => {
                "The direction it's aligned to is gone: pick another"
            }
            (AlignSide::Target, AlignRole::Secondary) => {
                "The second direction it's aligned to is gone: pick another"
            }
        })
    }

    /// What of the picks on `model` to light: the faces and edges picked
    /// for directions, the moved side's then the target's.
    fn lit(&self, model: u64) -> [Vec<Picked>; 2] {
        self.marks.each_ref().map(|marks| {
            (marks[1..].iter().flatten())
                .filter(|mark| mark.model == model)
                .filter_map(|mark| mark.target)
                .collect()
        })
    }
}

impl MotionSession {
    /// The align as set up, if it's whole: its body, both points, its
    /// directions paired, and the distance and angle as they last read,
    /// stored only where they aren't zero and there are directions to
    /// offset along (else it isn't whole: [`MotionSession::align_need`]
    /// says so); its flip only with directions.
    pub(super) fn align(&self) -> Option<Align> {
        let &[body] = &self.bodies[..] else {
            return None;
        };
        let [moved, target] = &self.align.sides;
        let (from, to) = (moved.refs()?, target.refs()?);
        if self.align.need().is_some() {
            return None;
        }
        let primaries = from.primary.is_some();
        let value = |field: MotionField| {
            let value = self.field(field).value.clone()?;
            Some((value.value != 0.0).then_some(value))
        };
        let (offset, turn) = (value(MotionField::Distance)?, value(MotionField::Angle)?);
        if !primaries && (offset.is_some() || turn.is_some()) {
            return None;
        }
        Some(Align {
            body,
            from,
            to,
            flip: primaries && self.flip,
            offset,
            turn,
        })
    }

    /// What's still to be done before it can be committed, the words for
    /// the status bar: its body, then its references (see
    /// [`AlignSetup::need`]); a distance or angle without directions to
    /// go along.
    pub(super) fn align_need(&self) -> Option<&'static str> {
        let &[body] = &self.bodies[..] else {
            return Some("pick the body to align");
        };
        let moved = &self.align.sides[0];
        let on_body = [AlignRole::Point, AlignRole::Primary, AlignRole::Secondary]
            .into_iter()
            .filter_map(|role| self.align.taken(AlignSlot::new(AlignSide::Moved, role)))
            .all(|taken| taken.body() == Some(body));
        if !on_body {
            return Some("pick the points and directions on the body again");
        }
        if let Some(need) = self.align.need() {
            return Some(need);
        }
        if moved.primary.is_none() {
            let typed = |field: MotionField| {
                (self.field(field).value.as_ref()).is_some_and(|value| value.value != 0.0)
            };
            if typed(MotionField::Distance) || typed(MotionField::Angle) {
                return Some("pick a direction on each side to offset or turn along");
            }
        }
        None
    }

    /// The UI mock's words for what it names that's gone, if anything.
    pub(super) fn align_gone(&self) -> Option<&'static str> {
        self.align.gone()
    }

    /// Notes what `document` no longer takes at feature `index`.
    pub(super) fn prune_align(&mut self, document: &Document, index: usize) {
        self.align.prune(document, index);
    }
}

impl Doc {
    /// Picks `body` as the one the align being set up moves, if the
    /// feature can name it: what's picked on the moved side on another
    /// body, and on the target's on it, is taken out, and clicks go on to
    /// the next reference to pick.
    pub(super) fn align_body(&mut self, body: BodyId) {
        let Some(session) = &mut self.motion else {
            return;
        };
        if session.bodies == [body] {
            return;
        }
        session.bodies = vec![body];
        session.align.moved_to(body);
        session.picking = session.align.next();
    }

    /// Takes `pick` as `slot` of the align being set up
    /// ([`Doc::align_reference`]): a round edge's centre also as its
    /// side's direction while it has none; the body it's on as the one
    /// moved if none is yet. Clicks go on to the next reference to pick.
    pub(super) fn align_pick(
        &mut self,
        slot: AlignSlot,
        pick: Pick,
    ) -> Result<(), Cow<'static, str>> {
        if !self.feed.answers_request() || self.feed.predates_replacement() {
            return Err(OUT_OF_DATE.into());
        }
        let taken = self.align_reference(slot, pick)?;
        let index = self.feed.pick_index();
        let model = index.model();
        let at = match (pick.snap, pick.target) {
            (Some(snapped), _) => index.snap_point(snapped).unwrap_or(pick.at),
            (None, Picked::Edge(edge)) if slot.role == AlignRole::Point => {
                (index.snap_point(Snapped::EdgePoint(edge))).unwrap_or(pick.at)
            }
            _ => pick.at,
        };
        let lit = match pick.target {
            Picked::Face(_) | Picked::Edge(_) if slot.role != AlignRole::Point => Some(pick.target),
            _ => None,
        };
        let mark = Mark {
            model,
            target: lit,
            at,
        };
        let rim = match (taken, pick.target, pick.snap) {
            (Taken::Point(PointRef::Centre(edge)), Picked::Edge(shown), _)
            | (Taken::Point(PointRef::Centre(edge)), _, Some(Snapped::EdgePoint(shown))) => {
                Some((edge, shown))
            }
            _ => None,
        };
        let Some(session) = &mut self.motion else {
            return Ok(());
        };
        if slot.side == AlignSide::Moved
            && session.bodies.is_empty()
            && let Some(body) = taken.body()
        {
            session.bodies = vec![body];
            session.align.moved_to(body);
        }
        session.align.set(slot, taken, Some(mark));
        let primary = AlignSlot::new(slot.side, AlignRole::Primary);
        if let Some((edge, shown)) = rim
            && session.align.taken(primary).is_none()
        {
            let mark = Mark {
                model,
                target: Some(Picked::Edge(shown)),
                at,
            };
            let axis = Taken::Direction(DirRef::Axis(AxisRef::Edge(edge)));
            session.align.set(primary, axis, Some(mark));
        }
        session.picking = session.align.next();
        Ok(())
    }

    /// Takes the origin, or the origin axis `axis`, as the target's point
    /// or direction being picked.
    pub(super) fn align_origin(&mut self, axis: Option<varde_document::Axis3>) {
        let Some(session) = &mut self.motion else {
            return;
        };
        let MotionPick::Align(slot) = session.picking else {
            return;
        };
        if slot.side != AlignSide::Target {
            return;
        }
        let taken = match (slot.role, axis) {
            (AlignRole::Point, None) => Taken::Point(PointRef::Origin),
            (AlignRole::Primary | AlignRole::Secondary, Some(axis)) => {
                Taken::Direction(DirRef::Origin(axis))
            }
            _ => return,
        };
        session.align.set(slot, taken, None);
        session.picking = session.align.next();
    }

    /// `pick` of the model shown as `slot` of the align being set up,
    /// named as the feature stores it ([`Naming`], the history stopped at
    /// the feature): for a point, a corner, a straight edge's middle or a
    /// round edge's centre (its snap point, or the edge clicked); for a
    /// direction, a flat face's normal, a round face's axis, or a
    /// straight or round edge. The moved side's must be on the body moved
    /// (if there's one yet; one a join before the feature merged into
    /// another can't be), the target's on another. Refused, why, if not.
    pub(super) fn align_reference(
        &self,
        slot: AlignSlot,
        pick: Pick,
    ) -> Result<Taken, Cow<'static, str>> {
        let session = self.motion.as_ref().ok_or("Nothing is set up")?;
        let index = self.feed.pick_index();
        let document = self.editor.document();
        let features = document.features();
        let before = (session.feature)
            .and_then(|id| features.iter().position(|feature| feature.id == id))
            .unwrap_or(features.len());
        let naming = Naming::before(document, before, self.shown());
        let refused = |why: Unnamed, what: &str| -> Cow<'static, str> {
            match why {
                Unnamed::Missing => format!("That {what} isn't in the model").into(),
                Unnamed::Later => {
                    let article = if what.starts_with('e') { "an" } else { "a" };
                    format!("Only {article} {what} made before the align can be picked").into()
                }
                Unnamed::Unclear => {
                    format!("Which body that {what} is on at the align can't be told: pick another")
                        .into()
                }
            }
        };
        let taken = match slot.role {
            AlignRole::Point => Taken::Point(point_of(index, &naming, pick, &refused)?),
            AlignRole::Primary | AlignRole::Secondary => {
                Taken::Direction(direction_of(index, &naming, pick, &refused)?)
            }
        };
        let Some(body) = taken.body() else {
            return Ok(taken);
        };
        let merged = self.feed.merged_before(document, session.feature);
        let held = merged.holder(body).unwrap_or(body);
        let moved = session.bodies.first().copied();
        let name = |body: BodyId| (document.body(body)).map_or("the body", |body| &body.name);
        match slot.side {
            AlignSide::Moved => {
                if merged.holder(body).is_some() {
                    return Err(format!(
                        "{} is merged into {} before the align: pick on a body it's on",
                        name(body),
                        name(held)
                    )
                    .into());
                }
                if let Some(moved) = moved.filter(|&moved| moved != body) {
                    return Err(format!("Pick it on {}, the body aligned", name(moved)).into());
                }
                if !super::super::combine::pickable(document, body, session.feature) {
                    return Err(refused(Unnamed::Later, "body"));
                }
            }
            AlignSide::Target => {
                if moved.is_some_and(|moved| held == moved) {
                    return Err(
                        "Pick what it's aligned to on another body than the one aligned".into(),
                    );
                }
            }
        }
        Ok(taken)
    }

    /// What's drawn of the align being set up, and named in its panel.
    pub(super) fn align_view(&self, session: &MotionSession) -> AlignView<'_> {
        let document = self.editor.document();
        let names = session.align.sides.each_ref().map(|side| {
            [
                side.point.map(|point| point_name(document, &point)),
                side.primary.map(|d| direction_name(document, &d)),
                side.secondary.map(|d| direction_name(document, &d)),
            ]
        });
        let info = session.align().map(|align| align_info(document, &align));
        let model = self.feed.model();
        let picking = matches!(session.picking, MotionPick::Align(_));
        let marks = match self.feed.draft_datums().filter(|_| !picking) {
            Some(datums) => marks_of(&datums),
            None => session.align.marks.each_ref().map(|marks| AlignMark {
                point: marks[0]
                    .filter(|mark| mark.model == model)
                    .map(|mark| mark.at),
                directions: [None, None],
            }),
        };
        let snaps = match session.picking {
            MotionPick::Align(slot) if slot.role == AlignRole::Point => self
                .pick
                .hover()
                .filter(|pick| pick.model == model)
                .map(|pick| (self.feed.pick_index(), pick)),
            _ => None,
        };
        AlignView {
            names,
            info,
            marks,
            snaps,
        }
    }

    /// The faces and edges picked for the align's directions to light in
    /// the model shown, the moved side's and the target's.
    pub(super) fn align_lit(&self) -> [Vec<Picked>; 2] {
        match &self.motion {
            Some(session) if session.kind == MotionKind::Align => {
                session.align.lit(self.feed.model())
            }
            _ => [Vec::new(), Vec::new()],
        }
    }
}

/// The marks of what regenerating found: each side's point and
/// directions.
fn marks_of(datums: &AlignDatums) -> [AlignMark; 2] {
    [datums.moved, datums.target].map(|side| match side {
        Some(side) => AlignMark {
            point: Some(DVec3::from(side.point)),
            directions: [side.primary, side.secondary].map(|d| d.map(DVec3::from)),
        },
        None => AlignMark::default(),
    })
}

/// The point `pick` names, see [`Doc::align_reference`].
fn point_of(
    index: &PickIndex,
    naming: &Naming,
    pick: Pick,
    refused: &impl Fn(Unnamed, &str) -> Cow<'static, str>,
) -> Result<PointRef, Cow<'static, str>> {
    let edge = match (pick.snap, pick.target) {
        (Some(Snapped::Corner(corner)), _) => {
            return naming
                .corner_ref(index, corner)
                .map_err(|why| refused(why, "corner"));
        }
        (Some(Snapped::EdgePoint(edge)), _) | (None, Picked::Edge(edge)) => edge,
        _ => return Err(NOT_A_POINT.into()),
    };
    let keys = index.chain_keys(edge).ok_or(NOT_A_POINT)?;
    let straight = index.edge_ends(edge, &keys).is_some();
    let round = !straight && index.snap_point(Snapped::EdgePoint(edge)).is_some();
    if !(straight || round) {
        return Err(NOT_A_POINT.into());
    }
    let named = naming
        .edge_ref(index, edge, pick.at)
        .map_err(|why| refused(why, "edge"))?;
    Ok(if straight {
        PointRef::Middle(named)
    } else {
        PointRef::Centre(named)
    })
}

/// The direction `pick` names, see [`Doc::align_reference`].
fn direction_of(
    index: &PickIndex,
    naming: &Naming,
    pick: Pick,
    refused: &impl Fn(Unnamed, &str) -> Cow<'static, str>,
) -> Result<DirRef, Cow<'static, str>> {
    match pick.target {
        Picked::Face(face) => {
            let summary = (index.picking().faces().get(face as usize)).map(|face| face.summary);
            let flat = matches!(summary, Some(Summary::Plane { .. }));
            let round = matches!(
                summary,
                Some(
                    Summary::Cylinder { .. }
                        | Summary::Cone { .. }
                        | Summary::Torus { .. }
                        | Summary::Revolved { .. }
                )
            );
            if !(flat || round) {
                return Err(NOT_A_DIRECTION.into());
            }
            let named = naming
                .checked_face_ref(index, face, pick.at)
                .map_err(|why| refused(why, "face"))?;
            Ok(if flat {
                DirRef::Normal(named)
            } else {
                DirRef::Axis(AxisRef::Face(named))
            })
        }
        Picked::Edge(edge) => {
            let keys = index.chain_keys(edge).ok_or(NOT_A_DIRECTION)?;
            let straight = index.edge_ends(edge, &keys).is_some();
            let round = index.snap_point(Snapped::EdgePoint(edge)).is_some();
            if !(straight || round) {
                return Err(NOT_A_DIRECTION.into());
            }
            let named = naming
                .edge_ref(index, edge, pick.at)
                .map_err(|why| refused(why, "edge"))?;
            Ok(DirRef::Axis(AxisRef::Edge(named)))
        }
        Picked::Vertex(_) => Err(NOT_A_DIRECTION.into()),
    }
}
