//! A chamfer being set up: its own parts of the move's panel, the model
//! mock's chamfer panel: the Edges picked, its Type as tiles (Equal, Two
//! distances, Distance and angle), the fields of the type, Flip sides
//! (not for Equal) and the Tangent chain tick, which the mock has on the
//! fillet's panel only. The edges are picked and lit in the viewport as
//! the app's highlight; nothing else is drawn.

use iced::Element;
use iced::widget::column;

use super::blend::{chain_toggle, edges_field};
use super::{BlendEdges, MotionField, MotionLook, MotionState, section};
use crate::icons::Icon;
use crate::operation_panel::{tile, tiles, toggle};
use crate::{Look, Message};

/// How a chamfer is sized, the mock's Type tiles: one distance along
/// both faces, two distances, or a distance and the cut's angle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ChamferType {
    #[default]
    Equal,
    Two,
    Angle,
}

impl ChamferType {
    /// The three in the panel's order.
    pub const ALL: [ChamferType; 3] = [ChamferType::Equal, ChamferType::Two, ChamferType::Angle];

    /// Its tile's label, the mock's: "Equal", "Two distances",
    /// "Distance and angle".
    pub fn label(self) -> &'static str {
        match self {
            ChamferType::Equal => "Equal",
            ChamferType::Two => "Two distances",
            ChamferType::Angle => "Distance and angle",
        }
    }

    /// Its tile's icon, the mock's `ch-equal`, `ch-two` and `ch-angle`.
    fn icon(self) -> Icon {
        match self {
            ChamferType::Equal => Icon::ChEqual,
            ChamferType::Two => Icon::ChTwo,
            ChamferType::Angle => Icon::ChAngle,
        }
    }

    /// The fields it reads, in the panel's order.
    pub fn fields(self) -> &'static [MotionField] {
        match self {
            ChamferType::Equal => &[MotionField::ChamferDistance],
            ChamferType::Two => &[MotionField::ChamferDistance, MotionField::ChamferSecond],
            ChamferType::Angle => &[MotionField::ChamferDistance, MotionField::ChamferAngle],
        }
    }
}

/// What the panel shows of a chamfer being set up, beside what every
/// move's session has (its Flip sides is the session's flip).
#[derive(Debug, Clone, PartialEq)]
pub struct ChamferView {
    pub edges: BlendEdges,
    pub kind: ChamferType,
    /// What the status bar says of it once it's whole: "2 edges · Equal ·
    /// 1 mm · Tangent chain".
    pub info: Option<String>,
}

/// The panel's body for a chamfer: Edges, Type's tiles, the type's
/// fields, Flip sides and Tangent chain.
pub(super) fn body<'a>(
    state: &MotionState<'a>,
    value: impl Fn(MotionField, &'a str) -> Element<'a, Message>,
) -> Element<'a, Message> {
    let Some(chamfer) = &state.chamfer else {
        return column![].into();
    };
    let send = |look: MotionLook| state.editable.then_some(Message::Look(Look::Motion(look)));
    let edges = edges_field(state, &chamfer.edges, "Click edges");
    let types = ChamferType::ALL.iter().map(|&kind| {
        tile(
            kind.icon(),
            kind.label(),
            chamfer.kind == kind,
            send(MotionLook::ChamferType(kind)),
        )
    });
    let first = match chamfer.kind {
        ChamferType::Two => "Distance 1",
        _ => "Distance",
    };
    let second = match chamfer.kind {
        ChamferType::Equal => None,
        ChamferType::Two => Some(value(MotionField::ChamferSecond, "Distance 2")),
        ChamferType::Angle => Some(value(MotionField::ChamferAngle, "Angle")),
    };
    // Equal is the same either way round: the mock offers no flip.
    let flip = (chamfer.kind != ChamferType::Equal).then(|| {
        toggle(
            Icon::TkFlip,
            "Flip sides",
            state.flip,
            send(MotionLook::Flip),
            None,
        )
    });
    column![
        edges,
        section("Type"),
        tiles(types),
        value(MotionField::ChamferDistance, first),
        second,
        flip,
        chain_toggle(state, &chamfer.edges),
    ]
    .spacing(10)
    .into()
}
