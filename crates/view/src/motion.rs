//! The move or mirror being set up: what the app hands the view of it,
//! the messages changing it, and its floating panel over the right of the
//! viewport. Its bodies are picked as a combine's (in the viewport, where
//! a click picks the body of what it's on, or in Objects); a move's axis
//! and a mirror's plane are picked in the viewport too (a model edge or
//! face) or, while they're the ones picking, from the toolbar's origin
//! axes or planes. The viewport's side, drawing the axis or plane and a
//! move's handles, which set its offsets and turn, is in
//! `viewport/motion.rs`.

use glam::DVec3;
use iced::Element;
use iced::widget::column;
use varde_document::{Axis3, AxisRef, BodyId, Document, OriginPlane, PlaneRef};
use varde_expr::{AngleUnit, LengthUnit, Unit};

/// Angles are shown in degrees.
const DEGREES: Unit = Unit::Angle(AngleUnit::Deg);

use crate::icons::Icon;
use crate::operation_panel::{
    Framing, PanelHover, Parts, TypedField, field, footer_message, label, operation_panel,
    pick_field, picked_row, toggle, value_field,
};
use crate::plane_pick::face_name;
use crate::revolve::edge_axis_name;
use crate::{CombineBody, Edit, Look, Message, VALUE_FIELD};

/// Which of the two is set up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MotionKind {
    Move,
    Mirror,
}

impl MotionKind {
    /// Its name as a feature's noun: "Move".
    pub fn noun(self) -> &'static str {
        match self {
            MotionKind::Move => "Move",
            MotionKind::Mirror => "Mirror",
        }
    }

    /// Its icon, the UI mock's `move` and `bmirror`.
    pub fn icon(self) -> Icon {
        match self {
            MotionKind::Move => Icon::Move,
            MotionKind::Mirror => Icon::BMirror,
        }
    }

    /// The panel's title for a new one: "New move".
    pub fn new_title(self) -> &'static str {
        match self {
            MotionKind::Move => "New move",
            MotionKind::Mirror => "New mirror",
        }
    }
}

/// What a click in the viewport picks: bodies, or the reference (a
/// move's axis, a mirror's plane). A click on one of the panel's fields
/// makes it the one picking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MotionPick {
    #[default]
    Bodies,
    Reference,
}

/// A typed field of a move: an offset along a world axis, or the angle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MotionField {
    Offset(Axis3),
    Angle,
}

impl MotionField {
    /// The four, in the panel's order.
    pub const ALL: [MotionField; 4] = [
        MotionField::Offset(Axis3::X),
        MotionField::Offset(Axis3::Y),
        MotionField::Offset(Axis3::Z),
        MotionField::Angle,
    ];

    /// Where it's kept in an array of the four.
    pub fn index(self) -> usize {
        match self {
            MotionField::Offset(Axis3::X) => 0,
            MotionField::Offset(Axis3::Y) => 1,
            MotionField::Offset(Axis3::Z) => 2,
            MotionField::Angle => 3,
        }
    }

    /// Its text field's id: the first offset's is [`VALUE_FIELD`], which
    /// takes the focus as the session opens.
    fn id(self) -> iced::widget::Id {
        match self {
            MotionField::Offset(Axis3::X) => VALUE_FIELD,
            MotionField::Offset(Axis3::Y) => iced::widget::Id::new("move-y"),
            MotionField::Offset(Axis3::Z) => iced::widget::Id::new("move-z"),
            MotionField::Angle => iced::widget::Id::new("move-angle"),
        }
    }

    /// Its label, the mock's: "X", "Y", "Z", "Angle".
    fn label(self) -> &'static str {
        match self {
            MotionField::Offset(axis) => axis.name(),
            MotionField::Angle => "Angle",
        }
    }
}

/// A change to the move or mirror being set up, see [`Look::Motion`].
#[derive(Debug, Clone, PartialEq)]
pub enum MotionLook {
    /// Whether clicks pick bodies or the reference: the panel's fields
    /// clicked.
    Picking(MotionPick),
    /// Takes `body` out: its row's cross.
    Drop(BodyId),
    /// The text in a move's field, as typed.
    Input { field: MotionField, text: String },
    /// An origin axis as a move's axis: the toolbar's, while the axis is
    /// picked.
    OriginAxis(Axis3),
    /// An origin plane as a mirror's plane: the toolbar's, while the
    /// plane is picked.
    OriginPlane(OriginPlane),
    /// A ring of a move's handles dragged: turns the bodies about the
    /// world axis `axis` by `angle` and shifts them by `offset`, so they
    /// turn about the handles' centre (the move turns about an axis
    /// through the origin, then shifts), the texts as typed in the
    /// angle's and the offsets' fields.
    Turn {
        axis: Axis3,
        angle: String,
        offset: [String; 3],
    },
    /// A mirror's Create copy: keeps the original, or not.
    Copy,
    /// Drops the move or mirror being set up, changing nothing: Cancel,
    /// or `Esc`.
    Cancel,
}

/// The move or mirror being set up, and how it's shown.
#[derive(Debug, Clone)]
pub struct MotionState<'a> {
    pub kind: MotionKind,
    /// The name of the feature edited, or none for a new one.
    pub editing: Option<&'a str>,
    /// The bodies picked, sorted by id as the document keeps them.
    pub bodies: Vec<CombineBody<'a>>,
    /// What a click picks.
    pub picking: MotionPick,
    /// A move's fields: the offsets along X, Y and Z, then the angle.
    pub fields: [TypedField<'a>; 4],
    /// The reference's name, "Z axis", "Edge of Body 1", "XY plane",
    /// "Extrude 1's end", if there's one.
    pub reference: Option<String>,
    /// Where the reference is, as a point on it and its direction (a
    /// plane's normal), not unit, if that's known: an origin axis or
    /// plane, or where regenerating the preview found an edge or face.
    pub line: Option<[DVec3; 2]>,
    /// Where the bodies are, the corners of their box in the model shown,
    /// if it shows them: the axis and plane are drawn across it, and a
    /// move's handles at its centre.
    pub bounds: Option<[DVec3; 2]>,
    /// A move's axis, if it's a world axis: while the move turns, only
    /// that axis's ring of the handles turns it further.
    pub origin_axis: Option<Axis3>,
    /// The design's units, which the handles' snapped offsets are typed
    /// in.
    pub units: LengthUnit,
    /// A mirror's Create copy.
    pub keep_original: bool,
    /// What's still to be done before it can be committed, if anything:
    /// "pick the bodies to move", for the status bar.
    pub need: Option<&'static str>,
    /// Why it can't be committed as set up, if its own check refuses it:
    /// shown in place of [`MotionState::error`].
    pub refused: Option<String>,
    /// Why the preview failed, if it did.
    pub error: Option<&'a str>,
    /// The button framing the camera on the geometry of why the preview
    /// failed, or going back from it, if that geometry has a box.
    pub show_error: Option<Framing>,
    /// Whether sketch edits have waited on the solver long enough to say
    /// so: OK waits for them, and the panel says why.
    pub checking: bool,
    /// Whether OK (and `Enter`) can be pressed: not while the preview
    /// failed.
    pub ready: bool,
    /// Whether Add anyway can be pressed: the preview failed, and it
    /// could be committed otherwise.
    pub accept: bool,
    /// Whether the document can be changed.
    pub editable: bool,
    /// The row of the panel the cursor is over, if any: the viewport
    /// lights it up too.
    pub hover: Option<PanelHover>,
}

impl<'a> MotionState<'a> {
    /// The panel's title: the feature edited's name, or "New move".
    pub fn title(&self) -> &'a str {
        self.editing.unwrap_or(self.kind.new_title())
    }
}

/// A move's axis as the panel and notes name it: "Z axis", "Edge of
/// Body 1", "Extrude 1's side".
pub fn axis_name(document: &Document, axis: &AxisRef) -> String {
    match axis {
        AxisRef::Origin(axis) => format!("{} axis", axis.name()),
        AxisRef::Edge(edge) => edge_axis_name(document, edge),
        AxisRef::Face(face) => face_name(document, face),
    }
}

/// A mirror's plane as the panel and notes name it: "XY plane",
/// "Extrude 1's end".
pub fn plane_name(document: &Document, plane: &PlaneRef) -> String {
    match plane {
        PlaneRef::Origin(plane) => format!("{} plane", plane.name()),
        PlaneRef::Face(face) => face_name(document, face),
    }
}

/// A mirror's plane as its Timeline row notes it, the mock's short form:
/// "XY", "Extrude 1's end".
pub(crate) fn plane_short(document: &Document, plane: &PlaneRef) -> String {
    match plane {
        PlaneRef::Origin(plane) => plane.name().to_owned(),
        PlaneRef::Face(face) => face_name(document, face),
    }
}

/// A move's Timeline note, the mock's: how far it shifts in all and its
/// turn, "82.5 mm 30°", either left out when it's none; "0 mm" for a
/// move that does nothing.
pub(crate) fn move_note(moved: &varde_document::Move, units: LengthUnit) -> String {
    let shift = moved.offset_vector().length();
    let angle = moved.turn.as_ref().map(|(_, angle)| angle.value);
    let mut parts = Vec::new();
    if shift != 0.0 || angle.is_none_or(|angle| angle == 0.0) {
        parts.push(varde_expr::format(shift, Some(units.into())));
    }
    if let Some(angle) = angle.filter(|&angle| angle != 0.0) {
        parts.push(varde_expr::format(angle, Some(DEGREES)));
    }
    parts.join(" ")
}

/// What the status bar says of a selected move, the mock's: "Body 1 by
/// 10, 0, -5 mm, 30° about Z axis".
pub(crate) fn move_info(document: &Document, moved: &varde_document::Move) -> String {
    let units = document.units();
    let bodies = body_names(document, &moved.bodies);
    let mut parts = Vec::new();
    if moved.offset.iter().any(|value| value.value != 0.0) {
        let symbol = format!(" {}", Unit::from(units).symbol());
        let [x, y, z] = moved.offset.each_ref().map(|value| {
            let shown = varde_expr::format(value.value, Some(units.into()));
            shown
                .strip_suffix(&symbol)
                .map(str::to_owned)
                .unwrap_or(shown)
        });
        parts.push(format!("by {x}, {y}, {z}{symbol}"));
    }
    if let Some((axis, angle)) = &moved.turn
        && angle.value != 0.0
    {
        let angle = varde_expr::format(angle.value, Some(DEGREES));
        parts.push(format!("{angle} about {}", axis_name(document, axis)));
    }
    if parts.is_empty() {
        format!("{bodies} not moved")
    } else {
        format!("{bodies} {}", parts.join(", "))
    }
}

/// What the status bar says of a selected mirror, the mock's: "Body 1
/// across XY plane · copy".
pub(crate) fn mirror_info(document: &Document, mirror: &varde_document::Mirror) -> String {
    let bodies = body_names(document, &mirror.bodies);
    let copy = if mirror.keep_original { " · copy" } else { "" };
    format!(
        "{bodies} across {}{copy}",
        plane_name(document, &mirror.plane)
    )
}

/// The names of `bodies` of `document`, joined: "Body 1, Body 2".
pub(crate) fn body_names(document: &Document, bodies: &[BodyId]) -> String {
    let names: Vec<&str> = (bodies.iter())
        .map(|&body| {
            document
                .body(body)
                .map_or("a body", |body| body.name.as_str())
        })
        .collect();
    names.join(", ")
}

/// What the status bar says of the move or mirror being set up: "Body 1
/// · 10 mm, 0 mm, 0 mm", or what's still to do.
pub(crate) fn status_info(state: &MotionState<'_>) -> String {
    if let Some(need) = state.need {
        return need.to_owned();
    }
    let names: Vec<&str> = state.bodies.iter().map(|body| body.name).collect();
    let bodies = names.join(", ");
    match (state.kind, &state.reference) {
        (MotionKind::Mirror, Some(plane)) => format!("{bodies} across {plane}"),
        (MotionKind::Move, Some(axis)) => {
            let angle = state.fields[MotionField::Angle.index()]
                .value
                .filter(|&angle| angle != 0.0);
            match angle {
                Some(angle) => format!(
                    "{bodies} · {} about {axis}",
                    varde_expr::format(angle, Some(DEGREES))
                ),
                None => bodies,
            }
        }
        _ => bodies,
    }
}

/// The floating panel of the move or mirror being set up, the mock's:
/// Bodies; a move's Translate (X, Y, Z) and Rotate (Axis, Angle); a
/// mirror's Plane and Create copy.
pub(crate) fn panel<'a>(state: &MotionState<'a>) -> Element<'a, Message> {
    let editable = state.editable;
    let send = |look: MotionLook| editable.then_some(Message::Look(Look::Motion(look)));

    let picking_bodies = state.picking == MotionPick::Bodies;
    let pick_bodies = send(MotionLook::Picking(MotionPick::Bodies));
    let rows: Vec<_> = (state.bodies.iter())
        .map(|body| {
            picked_row(
                Icon::Body,
                body.name,
                None,
                send(MotionLook::Drop(body.body)),
                pick_bodies.clone(),
                PanelHover::Body(body.body),
                state.hover,
            )
        })
        .collect();
    let place = (rows.is_empty() || picking_bodies).then(|| "Click bodies".to_owned());
    let bodies = field(
        "Bodies",
        pick_field(rows, place, picking_bodies, pick_bodies),
    );

    let picking_reference = state.picking == MotionPick::Reference;
    let pick_reference = send(MotionLook::Picking(MotionPick::Reference));
    let (reference_label, reference_icon, reference_place) = match state.kind {
        MotionKind::Move => ("Axis", Icon::SeAxis, "Click an axis or edge"),
        MotionKind::Mirror => ("Plane", Icon::SePlane, "Click a plane or face"),
    };
    let reference_row = state.reference.clone().map(|name| {
        picked_row(
            reference_icon,
            name,
            None,
            None,
            pick_reference.clone(),
            PanelHover::Axis,
            state.hover,
        )
    });
    let reference_place =
        (reference_row.is_none() || picking_reference).then(|| reference_place.to_owned());
    let reference = field(
        reference_label,
        pick_field(
            reference_row.into_iter().collect(),
            reference_place,
            picking_reference,
            pick_reference,
        ),
    );

    let field_of = |which: MotionField| {
        let input = editable.then_some(move |text| {
            Message::Look(Look::Motion(MotionLook::Input { field: which, text }))
        });
        value_field(
            which.label(),
            which.id(),
            state.fields[which.index()],
            input,
            Message::Edit(Edit::CommitMotion),
            Message::Look(Look::Motion(MotionLook::Cancel)),
        )
    };
    let body: Element<'a, Message> = match state.kind {
        MotionKind::Move => {
            let translate = MotionField::ALL[..3].iter().map(|&which| field_of(which));
            column![
                bodies,
                section("Translate"),
                column(translate).spacing(8),
                section("Rotate"),
                reference,
                field_of(MotionField::Angle),
            ]
            .spacing(10)
            .into()
        }
        MotionKind::Mirror => {
            let copy = toggle(
                Icon::TkCopy,
                "Create copy",
                state.keep_original,
                send(MotionLook::Copy),
                Some("Keep the original too"),
            );
            column![bodies, reference, copy].spacing(10).into()
        }
    };
    let message = footer_message(
        state.kind.noun(),
        state.refused.clone(),
        state.error,
        state.show_error,
        state.accept.then_some(Message::Edit(Edit::AcceptError)),
        state.checking,
    );
    operation_panel(Parts {
        icon: state.kind.icon(),
        title: state.title(),
        body,
        message,
        ok: state.ready.then_some(Message::Edit(Edit::CommitMotion)),
        cancel: Message::Look(Look::Motion(MotionLook::Cancel)),
        close: false,
    })
}

/// A section's heading in the panel, the mock's `h4`: as a field's
/// label.
fn section(name: &str) -> iced::widget::Text<'_> {
    label(name)
}

#[cfg(test)]
mod tests;
