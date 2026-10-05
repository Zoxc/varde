//! The revolve being set up: what the app hands the view of it, the
//! messages changing it, and the floating panel holding its options over
//! the right of the viewport. The viewport's side, picking regions and
//! the axis (a line or axis of the sketch, or a straight model edge in
//! its plane), is in `viewport/revolve.rs`.

use std::collections::BTreeSet;

use glam::{DVec2, DVec3};
use iced::Element;
use iced::widget::text::Wrapping;
use iced::widget::{column, text};
use varde_document::{AxisLine, BodyId, Document, EdgeRef, FeatureId, Placement, RevolveError};
use varde_sketch::{Curve, Id, Sketch, angle};

use crate::extrude::{centroid, region_name};
use crate::icons::Icon;
use crate::operation_panel::{
    BodyTarget, Candidate, Footer, Framing, OperationKind, PanelHover, Parts, TypedField, bodies,
    field, footer_message, message_text, operation_panel, pick_field, picked_row, tile, tiles,
    toggle, value_field,
};
use crate::pick::{PickIndex, Snapped};
use crate::theme;
use crate::{Edit, Look, Message, VALUE_FIELD};

/// The field of a revolve's second angle, for two sides. The first is
/// [`VALUE_FIELD`], which takes the focus as the session opens.
const SECOND_FIELD: iced::widget::Id = iced::widget::Id::new("revolve-second");

/// How far a sketch's axes reach at least, either side of its origin,
/// while a revolve's axis is picked, in millimetres.
const AXIS_REACH: f64 = 10.0;

/// How a revolve's turn is given, see `varde_document::Turn`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TurnKind {
    #[default]
    Full,
    OneSide,
    Symmetric,
    TwoSides,
}

impl TurnKind {
    pub const ALL: [TurnKind; 4] = [
        TurnKind::Full,
        TurnKind::OneSide,
        TurnKind::Symmetric,
        TurnKind::TwoSides,
    ];

    /// Its choice's icon, seen down the axis.
    fn icon(self) -> Icon {
        match self {
            TurnKind::Full => Icon::RvFull,
            TurnKind::OneSide => Icon::RvOne,
            TurnKind::Symmetric => Icon::RvSym,
            TurnKind::TwoSides => Icon::RvTwo,
        }
    }

    fn label(self) -> &'static str {
        match self {
            TurnKind::Full => "Full 360°",
            TurnKind::OneSide => "One side",
            TurnKind::Symmetric => "Symmetric",
            TurnKind::TwoSides => "Two sides",
        }
    }

    /// The angles it's given by.
    pub fn angles(self) -> &'static [Angle] {
        match self {
            TurnKind::Full => &[],
            TurnKind::OneSide | TurnKind::Symmetric => &[Angle::First],
            TurnKind::TwoSides => &[Angle::First, Angle::Second],
        }
    }

    /// Whether Flip changes it.
    pub fn flips(self) -> bool {
        matches!(self, TurnKind::OneSide | TurnKind::TwoSides)
    }
}

/// One of a revolve's angles: the first, which one side and symmetric
/// have too, or the second of two sides, turning the other way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Angle {
    First,
    Second,
}

impl Angle {
    /// Where it's kept in an array of both.
    pub fn index(self) -> usize {
        match self {
            Angle::First => 0,
            Angle::Second => 1,
        }
    }
}

/// What a click in the viewport picks first: regions, or the axis. A
/// click on a region picks it either way; one on a line or one of the
/// sketch's axes picks the axis only while the axis is being picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RevolvePick {
    #[default]
    Regions,
    Axis,
}

/// A change to the revolve being set up, see [`Look::Revolve`].
#[derive(Debug, Clone, PartialEq)]
pub enum RevolveLook {
    /// A region of the sketch `sketch` clicked: added, or taken out if
    /// it's picked. The first sets the sketch the revolve takes regions
    /// of, if none is yet.
    PickRegion {
        sketch: FeatureId,
        region: usize,
    },
    /// A line of the sketch `sketch`, or one of its axes, clicked while
    /// the axis is picked: the axis, from then on. It sets the sketch the
    /// revolve takes regions of, if none is yet.
    PickAxis {
        sketch: FeatureId,
        axis: AxisLine,
    },
    /// An edge of the model shown clicked while the axis is picked, at
    /// `at`: the axis from then on if it's a straight edge in the source
    /// sketch's plane made before the revolve, else refused, saying why.
    /// `model` is the [`PickIndex::model`] of the index whose `edge` it
    /// is.
    PickEdge {
        model: u64,
        edge: u32,
        at: DVec3,
    },
    /// Whether clicks pick regions or the axis: the panel's Profile and
    /// Axis rows clicked.
    Picking(RevolvePick),
    Extent(TurnKind),
    /// The text in an angle's field, as typed.
    Input {
        angle: Angle,
        text: String,
    },
    /// Turns the direction round, for one side and two sides.
    Flip,
    Operation(OperationKind),
    /// Takes a body out of a join, cut or intersect, or puts it back.
    Target(BodyId),
    /// The knob of `Angle` pressed in the viewport: the cursor drags it
    /// until [`RevolveLook::DropHandle`].
    GrabHandle(Angle),
    /// The knob grabbed dragged to `to` radians about the axis, from the
    /// sketch plane, positive right-handed about the axis's direction.
    DragHandle {
        angle: Angle,
        to: f64,
    },
    /// The knob grabbed let go of.
    DropHandle,
    /// Drops the revolve being set up, changing nothing: Cancel, or `Esc`.
    Cancel,
}

/// The revolve being set up, and how it's shown.
#[derive(Debug, Clone)]
pub struct RevolveState<'a> {
    /// The name of the revolve edited, or none for a new one.
    pub editing: Option<&'a str>,
    /// The sketches whose regions show and can be picked: the revolve's
    /// sketch once there is one, else every visible sketch with regions.
    pub candidates: Vec<Candidate<'a>>,
    /// The sketch the revolve takes regions of, once there is one: the
    /// only candidate then.
    pub source: Option<FeatureId>,
    /// The regions picked, by their index in the source's profiles.
    pub picked: &'a BTreeSet<usize>,
    /// How many of the edited revolve's regions weren't found.
    pub missing: usize,
    /// The axis picked, a line of the source or one of its axes or a
    /// model edge, if one is.
    pub axis: Option<AxisLine>,
    /// Where the axis picked is, if it's a model edge found in the model
    /// shown: its ends, in the order it runs ([`EdgeRef`]).
    pub edge_ends: Option<[DVec3; 2]>,
    /// The name of the body of the axis picked, if it's a model edge.
    pub edge_body: Option<&'a str>,
    /// The model shown, whose straight edges in the source's plane a
    /// click picks as the axis while it's picked.
    pub index: &'a PickIndex,
    /// The resolution of the document's tolerance, within which a model
    /// edge must be in the source's plane to be the axis ([`axis_edge`]).
    pub resolution: f64,
    /// Whether the edited revolve's axis line wasn't found.
    pub axis_missing: bool,
    /// What a click picks first.
    pub picking: RevolvePick,
    pub extent: TurnKind,
    /// The first angle's field, and two sides' second.
    pub fields: [TypedField<'a>; 2],
    pub flip: bool,
    pub operation: OperationKind,
    /// For a join, cut or intersect, the bodies its preview touches and
    /// those taken out of it, in the order they were made.
    pub targets: Vec<BodyTarget<'a>>,
    /// Why the preview failed, if it did.
    pub error: Option<&'a str>,
    /// The button framing the camera on the geometry of why the preview
    /// failed, or going back from it, if that geometry has a box: beside
    /// [`RevolveState::error`]'s Add anyway.
    pub show_error: Option<Framing>,
    /// Why the revolve as set up can't be committed, if its own check
    /// refuses it (two sides over a turn together, say): shown in place
    /// of [`RevolveState::error`].
    pub refused: Option<RevolveError>,
    /// Why the edited feature can't stop making a new body, if a join,
    /// cut or intersect is picked while a combine names its body: shown
    /// in place of the preview's error, and OK waits.
    pub held: Option<String>,
    /// For a cut whose preview works, which bodies it takes nothing from
    /// (it only touches them), as a note: "Body 2: nothing to cut".
    pub uncut: Option<String>,
    /// Whether sketch edits have waited on the solver long enough to say
    /// so: OK waits for them, and the panel says why.
    pub checking: bool,
    /// Whether OK (and `Enter`) can be pressed: not while the preview
    /// failed.
    pub ready: bool,
    /// Whether Accept error can be pressed: the preview failed
    /// ([`RevolveState::error`]), and the operation could be committed otherwise.
    pub accept: bool,
    /// Whether the document can be changed.
    pub editable: bool,
    /// The row of the panel the cursor is over, if any: the viewport
    /// lights it up too.
    pub hover: Option<PanelHover>,
    /// The knob grabbed, if one is.
    pub grabbed: Option<Angle>,
}

impl<'a> RevolveState<'a> {
    /// The source sketch, if it's chosen.
    pub(crate) fn source(&self) -> Option<&Candidate<'a>> {
        let source = self.source?;
        self.candidates.iter().find(|c| c.feature == source)
    }

    /// The axis's name, "Line 3", "X axis" or "Edge of Body 1", if one
    /// is picked and its source shows.
    pub fn axis_name(&self) -> Option<String> {
        let sketch = self.source()?.sketch;
        match self.axis? {
            AxisLine::Edge(_) => Some(edge_name(self.edge_body)),
            axis => sketch.name(axis_id(axis)?),
        }
    }

    /// Whether the turn goes against the axis's direction: flipped, for
    /// one side and two sides. The viewport's arrow on the axis points
    /// the way positive angles turn right-handed about.
    pub(crate) fn reversed(&self) -> bool {
        self.flip && self.extent.flips()
    }
}

/// A revolve's handle: a knob for each angle its extent has, at the
/// picked regions' centre turned about the axis to that angle.
#[derive(Debug, Clone, PartialEq)]
pub struct RevolveHandle {
    /// The point of the axis nearest the regions' centre.
    pub origin: DVec3,
    /// The axis's direction, of unit length: positive angles turn
    /// right-handed about it.
    pub axis: DVec3,
    /// From `origin` to the regions' centre, square to the axis: where
    /// angle 0, the sketch plane, is.
    pub radial: DVec3,
    /// Each knob, and its angle in radians.
    pub knobs: Vec<(Angle, f64)>,
}

impl RevolveHandle {
    /// The regions' centre turned `angle` radians about the axis.
    pub(crate) fn at(&self, angle: f64) -> DVec3 {
        let (sin, cos) = (angle::sin(angle), angle::cos(angle));
        self.origin + self.radial * cos + self.axis.cross(self.radial) * sin
    }

    /// Which way the regions' centre goes turned `angle` radians, of unit
    /// length: positive angles' way.
    pub(crate) fn tangent(&self, angle: f64) -> DVec3 {
        let (sin, cos) = (angle::sin(angle), angle::cos(angle));
        (self.axis.cross(self.radial) * cos - self.radial * sin).normalize_or_zero()
    }

    /// How far the regions' centre is from the axis.
    pub(crate) fn radius(&self) -> f64 {
        self.radial.length()
    }
}

impl RevolveState<'_> {
    /// The axis picked in the world, as a point of it and its direction of
    /// unit length, if its source shows it.
    pub(crate) fn axis_in_world(&self) -> Option<(DVec3, DVec3)> {
        let source = self.source()?;
        let placement = source.placement;
        let (at, along) = match self.axis? {
            AxisLine::Edge(_) => {
                let [start, end] = self.edge_ends?;
                (start, end - start)
            }
            axis => {
                let (at, along) = axis_line(source.sketch, axis)?;
                (
                    placement.to_world(at),
                    placement.x * along.x + placement.y * along.y,
                )
            }
        };
        Some((at, along.try_normalize()?))
    }

    /// The handle, once regions and the axis are picked, the extent has
    /// angles, and the regions' centre is off the axis.
    pub fn handle(&self) -> Option<RevolveHandle> {
        let source = self.source()?;
        let regions = (self.picked.iter()).filter_map(|&index| source.profiles.regions.get(index));
        let centre = source.placement.to_world(centroid(regions)?);
        let (at, axis) = self.axis_in_world()?;
        let origin = at + axis * (centre - at).dot(axis);
        let radial = centre - origin;
        let scale = centre.abs().max_element().max(1.0);
        if !(radial.length() > 1e-9 * scale && radial.is_finite()) {
            return None;
        }
        let value = |angle: Angle| self.fields[angle.index()].value;
        let sign = if self.reversed() { -1.0 } else { 1.0 };
        let knobs: Vec<(Angle, f64)> = match self.extent {
            TurnKind::Full => Vec::new(),
            TurnKind::OneSide => value(Angle::First)
                .map(|a| (Angle::First, sign * a))
                .into_iter()
                .collect(),
            TurnKind::Symmetric => value(Angle::First)
                .map(|a| (Angle::First, a / 2.0))
                .into_iter()
                .collect(),
            TurnKind::TwoSides => [
                value(Angle::First).map(|a| (Angle::First, sign * a)),
                value(Angle::Second).map(|b| (Angle::Second, -sign * b)),
            ]
            .into_iter()
            .flatten()
            .collect(),
        };
        Some(RevolveHandle {
            origin,
            axis,
            radial,
            knobs,
        })
    }
}

/// The id the sketch knows `axis` by: the line's, or its built-in axis's;
/// none for a model edge, which isn't the sketch's.
pub(crate) fn axis_id(axis: AxisLine) -> Option<Id> {
    match axis {
        AxisLine::Curve(id) => Some(id),
        AxisLine::SketchX => Some(Id::X_AXIS),
        AxisLine::SketchY => Some(Id::Y_AXIS),
        AxisLine::Edge(_) => None,
    }
}

/// A model edge as the axis's name: "Edge of Body 1", by the name of
/// its body if that's known.
pub(crate) fn edge_name(body: Option<&str>) -> String {
    match body {
        Some(body) => format!("Edge of {body}"),
        None => "Model edge".to_owned(),
    }
}

/// The name of the axis of a revolve of `document` about `edge`: see
/// [`edge_name`].
pub(crate) fn edge_axis_name(document: &Document, edge: &EdgeRef) -> String {
    edge_name(document.body(edge.body).map(|body| body.name.as_str()))
}

/// Why a model edge can't be a revolve's axis: it isn't straight.
pub const EDGE_NOT_STRAIGHT: &str = "Only a straight edge can be the axis";
/// Why a model edge can't be a revolve's axis: it isn't in the plane of
/// the sketch the revolve takes regions of.
pub const EDGE_OFF_PLANE: &str = "That edge isn't in the sketch's plane";

/// Edge `edge` of `index`'s model as a revolve's axis on a sketch placed
/// at `placement`: its ends in the order its reference runs
/// ([`PickIndex::edge_ends`], its keys sorted), if it's straight and in
/// the sketch's plane as regenerating tells it, within `resolution` (the
/// document's tolerance's); else why not. Regenerating requires the
/// curves' ends within the resolution of the plane: here the middle of
/// those ends (the edge's snap point, from the exact curves) must be,
/// and the mesh's ends, `f32` points, within it and their rounding. So
/// an edge regenerating takes is taken, and one it refuses is refused
/// unless it's tilted across the plane by less than that rounding.
pub fn axis_edge(
    index: &PickIndex,
    edge: u32,
    placement: &Placement,
    resolution: f64,
) -> Result<[DVec3; 2], &'static str> {
    let keys = index.chain_keys(edge).ok_or(EDGE_NOT_STRAIGHT)?;
    let ends = index.edge_ends(edge, &keys).ok_or(EDGE_NOT_STRAIGHT)?;
    let middle = (index.snap_point(Snapped::EdgePoint(edge))).ok_or(EDGE_NOT_STRAIGHT)?;
    let scale = (ends.iter())
        .map(|p| p.abs().max_element())
        .fold(placement.origin.abs().max_element(), f64::max)
        .max(1.0);
    let height = |p: DVec3| ((p - placement.origin).dot(placement.normal)).abs();
    let rounded = resolution + 8.0 * f64::from(f32::EPSILON) * scale;
    let exact = resolution + 16.0 * f64::EPSILON * scale;
    if height(middle) <= exact && ends.iter().all(|&p| height(p) <= rounded) {
        Ok(ends)
    } else {
        Err(EDGE_OFF_PLANE)
    }
}

/// The axis line the sketch's id `id` names, if it names a line of
/// `sketch` or one of its axes: what a click on it picks.
pub(crate) fn axis_of(sketch: &Sketch, id: Id) -> Option<AxisLine> {
    match id {
        Id::X_AXIS => Some(AxisLine::SketchX),
        Id::Y_AXIS => Some(AxisLine::SketchY),
        _ => match sketch.curve(id)?.curve {
            Curve::Line { .. } => Some(AxisLine::Curve(id)),
            _ => None,
        },
    }
}

/// `axis` of `sketch` as a point on it and its direction, in the sketch's
/// coordinates, as regeneration takes it: an axis from the origin along
/// +x or +y, a line from its start to its end. `None` for a line that's
/// gone or isn't one, or has no length, and for a model edge, which isn't
/// the sketch's.
pub(crate) fn axis_line(sketch: &Sketch, axis: AxisLine) -> Option<(DVec2, DVec2)> {
    let (at, along) = match axis {
        AxisLine::SketchX => (DVec2::ZERO, DVec2::X),
        AxisLine::SketchY => (DVec2::ZERO, DVec2::Y),
        AxisLine::Curve(id) => {
            let Curve::Line { start, end } = sketch.curve(id)?.curve else {
                return None;
            };
            let start = sketch.point(start)?.at;
            (start, sketch.point(end)?.at - start)
        }
        AxisLine::Edge(_) => return None,
    };
    (along != DVec2::ZERO && along.is_finite() && at.is_finite()).then_some((at, along))
}

/// How far either side of its origin `sketch`'s axes are drawn, and
/// picked, while a revolve's axis is: a quarter past its farthest point
/// along either, and at least [`AXIS_REACH`].
pub(crate) fn axis_reach(sketch: &Sketch) -> f64 {
    let far = (sketch.points.iter())
        .map(|point| point.at.abs().max_element())
        .filter(|far| far.is_finite())
        .fold(0.0, f64::max);
    (far * 1.25).max(AXIS_REACH)
}

/// The floating panel of the revolve being set up.
pub(crate) fn panel<'a>(state: &RevolveState<'a>) -> Element<'a, Message> {
    let editable = state.editable;
    let send = |look: RevolveLook| editable.then_some(Message::Look(Look::Revolve(look)));

    let picking_regions = state.picking == RevolvePick::Regions;
    let pick_regions = send(RevolveLook::Picking(RevolvePick::Regions));
    let rows: Vec<_> = (state.source.into_iter())
        .flat_map(|sketch| {
            let pick_regions = pick_regions.clone();
            state.picked.iter().map(move |&region| {
                picked_row(
                    Icon::SeRegion,
                    region_name(region),
                    None,
                    send(RevolveLook::PickRegion { sketch, region }),
                    pick_regions.clone(),
                    PanelHover::Region { sketch, region },
                    state.hover,
                )
            })
        })
        .collect();
    let empty = state
        .candidates
        .iter()
        .all(|candidate| candidate.profiles.regions.is_empty());
    let place = (rows.is_empty() || picking_regions).then(|| {
        if state.picked.is_empty() && empty {
            "No closed regions to revolve".to_owned()
        } else {
            "Click regions".to_owned()
        }
    });
    let profile = field(
        "Profile",
        pick_field(rows, place, picking_regions, pick_regions),
    );
    let picking_axis = state.picking == RevolvePick::Axis;
    let pick_axis = send(RevolveLook::Picking(RevolvePick::Axis));
    let axis_row = state.axis_name().map(|name| {
        picked_row(
            Icon::SeAxis,
            name,
            None,
            None,
            pick_axis.clone(),
            PanelHover::Axis,
            state.hover,
        )
    });
    let axis_place = axis_row
        .is_none()
        .then(|| "Click a line, axis or edge".to_owned());
    let axis = field(
        "Axis",
        pick_field(
            axis_row.into_iter().collect(),
            axis_place,
            picking_axis,
            pick_axis,
        ),
    );
    let missing = {
        let regions = match state.missing {
            0 => None,
            1 => Some("1 region wasn't found".to_owned()),
            n => Some(format!("{n} regions weren't found")),
        };
        let axis = state.axis_missing.then(|| {
            match state.axis {
                Some(AxisLine::Edge(_)) => "The axis edge wasn't found",
                _ => "The axis line wasn't found",
            }
            .to_owned()
        });
        let notes = regions.into_iter().chain(axis).map(|note| {
            text(note)
                .size(12)
                .wrapping(Wrapping::WordOrGlyph)
                .style(theme::danger_text)
                .into()
        });
        column(notes).spacing(2)
    };

    let extents = TurnKind::ALL.map(|kind| {
        tile(
            kind.icon(),
            kind.label(),
            state.extent == kind,
            send(RevolveLook::Extent(kind)),
        )
    });
    let fields = state.extent.angles().iter().map(|&angle| {
        let label = match (state.extent, angle) {
            (TurnKind::TwoSides, Angle::First) => "Side 1",
            (TurnKind::TwoSides, Angle::Second) => "Side 2",
            _ => "Angle",
        };
        angle_field(label, angle, state.fields[angle.index()], editable)
    });
    let flip = state.extent.flips().then(|| {
        toggle(
            Icon::TkFlip,
            "Flip",
            state.flip,
            send(RevolveLook::Flip),
            None,
        )
    });
    let operations = OperationKind::ALL.map(|kind| {
        tile(
            kind.icon(),
            kind.label(),
            state.operation == kind,
            send(RevolveLook::Operation(kind)),
        )
    });
    let targets = bodies(
        state.operation,
        &state.targets,
        |body| send(RevolveLook::Target(body)),
        state.hover,
    );
    let refused = (state.refused.map(|refused| refused.to_string())).or_else(|| state.held.clone());
    let message = footer_message(
        "Revolve",
        refused,
        state.error,
        state.show_error,
        state.accept.then_some(Message::Edit(Edit::AcceptError)),
        state.checking,
    )
    // A cut that works but takes nothing from a body says so.
    .or_else(|| {
        (state.uncut.clone()).map(|note| Footer::Text(message_text(note, theme::warning_text)))
    });

    let body = column![
        profile,
        axis,
        missing,
        field("Extent", tiles(extents)),
        column(fields).spacing(8),
        flip,
        field("Operation", tiles(operations)),
        targets,
    ]
    .spacing(10);
    operation_panel(Parts {
        icon: Icon::Revolve,
        title: state.editing.unwrap_or("New revolve"),
        body: body.into(),
        message,
        ok: state.ready.then_some(Message::Edit(Edit::CommitRevolve)),
        cancel: Message::Look(Look::Revolve(RevolveLook::Cancel)),
        close: false,
    })
}

/// The field of `angle`, named `label`, showing why its text is refused
/// under it. `Enter` in it is OK, `Esc` Cancel.
fn angle_field<'a>(
    label: &'a str,
    angle: Angle,
    field: TypedField<'a>,
    editable: bool,
) -> Element<'a, Message> {
    let id = match angle {
        Angle::First => VALUE_FIELD,
        Angle::Second => SECOND_FIELD,
    };
    let input = editable
        .then_some(move |text| Message::Look(Look::Revolve(RevolveLook::Input { angle, text })));
    value_field(
        label,
        id,
        field,
        input,
        Message::Edit(Edit::CommitRevolve),
        Message::Look(Look::Revolve(RevolveLook::Cancel)),
    )
}

#[cfg(test)]
mod tests;
