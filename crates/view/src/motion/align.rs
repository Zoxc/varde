//! An align being set up: its own parts of the move's panel, built in the
//! style of the UI mock's Move panel (the mock has no Align panel, only
//! the tool in its Transform group): the Body, then From and To, each a
//! Point, a Direction and a Second direction, then Flip, and Offset, a
//! distance along the target's direction and an angle about it.

use glam::DVec3;
use iced::Element;
use iced::widget::column;

use super::{AlignRole, AlignSide, AlignSlot, MotionField, MotionLook, MotionPick, MotionState};
use varde_document::{AxisRef, DirRef, Document, PointRef};

use crate::icons::Icon;
use crate::operation_panel::{PanelHover, field, pick_field, picked_row, toggle};
use crate::pick::{Pick, PickIndex};
use crate::plane_pick::face_name;
use crate::{Look, Message};

/// What the panel and the viewport show of an align being set up, beside
/// what every move's session has.
#[derive(Debug, Clone)]
pub struct AlignView<'a> {
    /// Each side's point, direction and second direction as named, if
    /// picked: the moved side's first ([`AlignSide::index`], then
    /// [`AlignRole::index`]).
    pub names: [[Option<String>; 3]; 2],
    /// What the status bar says of it once it's whole: "Body 2 to
    /// Body 1".
    pub info: Option<String>,
    /// Where each side's point and directions are, as far as that's
    /// known: where the preview found them, or while they're picked, the
    /// points where they were picked on the model shown. Drawn in the
    /// viewport, the moved side's in the accent, the target's in the
    /// second colour.
    pub marks: [AlignMark; 2],
    /// While a point is picked, the model shown and what of it the
    /// cursor is over: its snap points are drawn as the measure tool's.
    pub snaps: Option<(&'a PickIndex, Pick)>,
}

/// One side of an align as the viewport draws it: its point and
/// directions, where they're known.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AlignMark {
    pub point: Option<DVec3>,
    /// Its direction and second direction, as found (not unit).
    pub directions: [Option<DVec3>; 2],
}

/// An align's point as the panel names it: "Corner of Body 1", "Middle
/// of an edge of Body 1", "Centre of an edge of Body 2", "Origin".
pub fn point_name(document: &Document, point: &PointRef) -> String {
    let body = |body| {
        document
            .body(body)
            .map_or("a body", |body| body.name.as_str())
    };
    match point {
        PointRef::Origin => "Origin".to_owned(),
        PointRef::Corner { body: on, .. } => format!("Corner of {}", body(*on)),
        PointRef::Middle(edge) => format!("Middle of an edge of {}", body(edge.body)),
        PointRef::Centre(edge) => format!("Centre of an edge of {}", body(edge.body)),
    }
}

/// An align's direction as the panel names it: "Normal of Extrude 1's
/// end", "Axis of Extrude 2's side", "Edge of Body 1", "Z axis".
pub fn direction_name(document: &Document, direction: &DirRef) -> String {
    match direction {
        DirRef::Origin(axis) => super::axis_name(document, &AxisRef::Origin(*axis)),
        DirRef::Normal(face) => format!("Normal of {}", face_name(document, face)),
        DirRef::Axis(AxisRef::Face(face)) => format!("Axis of {}", face_name(document, face)),
        DirRef::Axis(axis) => super::axis_name(document, axis),
    }
}

/// What a side's field asks for while it's empty or picked.
fn place(slot: AlignSlot) -> &'static str {
    match (slot.side, slot.role) {
        (AlignSide::Moved, AlignRole::Point) => "Click a corner, middle or centre",
        (AlignSide::Target, AlignRole::Point) => "Click a corner, middle, centre or origin",
        (_, AlignRole::Primary) => "Click a face or edge",
        (_, AlignRole::Secondary) => "Optional: click a face or edge",
    }
}

/// The panel's body for an align, after its Body field `body`: From and
/// To, each side's point, direction and second direction, then Flip and
/// Offset (Distance and Angle).
pub(super) fn body<'a>(
    state: &MotionState<'a>,
    body: Element<'a, Message>,
    value: impl Fn(MotionField, &'a str) -> Element<'a, Message>,
) -> Element<'a, Message> {
    let editable = state.editable;
    let send = |look: MotionLook| editable.then_some(Message::Look(Look::Motion(look)));
    let Some(align) = &state.align else {
        return body;
    };
    let side = |side: AlignSide| {
        let fields = AlignRole::ALL.map(|role| {
            let slot = AlignSlot::new(side, role);
            let on = state.picking == MotionPick::Align(slot);
            let pick = send(MotionLook::Picking(MotionPick::Align(slot)));
            let icon = match role {
                AlignRole::Point => Icon::Point,
                AlignRole::Primary | AlignRole::Secondary => Icon::SeAxis,
            };
            let name = align.names[side.index()][role.index()].clone();
            let row = name.map(|name| {
                picked_row(
                    icon,
                    name,
                    None,
                    send(MotionLook::Clear(slot)),
                    pick.clone(),
                    PanelHover::Axis,
                    state.hover,
                )
            });
            let place = (row.is_none() || on).then(|| place(slot).to_owned());
            field(
                role.label(),
                pick_field(row.into_iter().collect(), place, on, pick),
            )
        });
        column(fields).spacing(8)
    };
    let flip = toggle(
        Icon::TkFlip,
        "Flip",
        state.flip,
        send(MotionLook::Flip),
        Some("The directions meet the other way round"),
    );
    column![
        body,
        super::section("From"),
        side(AlignSide::Moved),
        super::section("To"),
        side(AlignSide::Target),
        flip,
        super::section("Offset"),
        value(MotionField::Distance, MotionField::Distance.label()),
        value(MotionField::Angle, MotionField::Angle.label()),
    ]
    .spacing(10)
    .into()
}
