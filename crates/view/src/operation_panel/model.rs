//! What the operation sessions hand the panel and the viewport: the
//! sketches picked in, the sides, the kinds, the row hovered, the
//! bodies joined into and the verdict.

use std::collections::BTreeSet;
use std::sync::Arc;

use glam::{DVec2, DVec3};
use iced::Element;
use varde_document::{BodyId, BodyOp, FeatureId, Placement};
use varde_sketch::{Profiles, Region, Sketch};

use super::controls::{toggle, value_field};
use crate::icons::Icon;
use crate::{Look, Message};

/// No regions picked, for a view without them.
pub static NO_REGIONS: BTreeSet<usize> = BTreeSet::new();

/// A sketch whose regions can be picked, and where they are.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    pub feature: FeatureId,
    /// Where its plane is: a sketch that isn't placed isn't a candidate.
    pub placement: Placement,
    /// The sketch, whose lines a revolve's axis is picked from.
    pub sketch: &'a Sketch,
    pub profiles: &'a Arc<Profiles>,
}

impl<'a> Candidate<'a> {
    /// The candidate of `candidates` that's the sketch `source`, if it's
    /// chosen and among them.
    pub(crate) fn find<'c>(
        candidates: &'c [Candidate<'a>],
        source: Option<FeatureId>,
    ) -> Option<&'c Candidate<'a>> {
        let source = source?;
        candidates.iter().find(|c| c.feature == source)
    }

    /// The centre of its regions `picked`, in the world ([`centroid`]).
    pub(crate) fn centre_of(&self, picked: &BTreeSet<usize>) -> Option<DVec3> {
        let regions = (picked.iter()).filter_map(|&index| self.profiles.regions.get(index));
        Some(self.placement.to_world(centroid(regions)?))
    }

    /// Whether none of `candidates` has a closed region to pick.
    pub(crate) fn none_closed(candidates: &[Candidate<'_>]) -> bool {
        (candidates.iter()).all(|candidate| candidate.profiles.regions.is_empty())
    }
}

/// The centre of `regions` together, weighing each loop of their outlines
/// by its area (holes taking theirs away), or of their boxes if that
/// comes to nothing.
pub(crate) fn centroid<'r>(regions: impl Iterator<Item = &'r Region>) -> Option<DVec2> {
    let mut area = 0.0;
    let mut moment = DVec2::ZERO;
    let mut boxes: Option<(DVec2, DVec2)> = None;
    for region in regions {
        let (min, max) = region.bounds;
        boxes = Some(boxes.map_or((min, max), |(lo, hi)| (lo.min(min), hi.max(max))));
        for (k, polyline) in region.outline.iter().enumerate() {
            let (a, m) = loop_moment(polyline);
            // The outer loop adds, holes take away, whichever way round.
            let sign = if k == 0 { a.signum() } else { -a.signum() };
            area += sign * a;
            moment += m * sign;
        }
    }
    let centre = moment / area;
    if area > 0.0 && centre.is_finite() {
        Some(centre)
    } else {
        boxes.map(|(lo, hi)| (lo + hi) / 2.0)
    }
}

/// The signed area of the closed `polyline` and its first moment (its
/// centroid times its area), by the shoelace formula.
pub(super) fn loop_moment(polyline: &[DVec2]) -> (f64, DVec2) {
    let Some(&last) = polyline.last() else {
        return (0.0, DVec2::ZERO);
    };
    let mut area = 0.0;
    let mut moment = DVec2::ZERO;
    let mut before = last;
    for &at in polyline {
        let cross = before.perp_dot(at);
        area += cross;
        moment += (before + at) * cross;
        before = at;
    }
    (area / 2.0, moment / 6.0)
}

/// One of the values of a two-sided extent, an extrude's distances or a
/// revolve's angles: the first, which one side and symmetric have too,
/// or the second of two sides, going the other way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Side {
    First,
    Second,
}

impl Side {
    /// Where it's kept in an array of both.
    pub fn index(self) -> usize {
        match self {
            Side::First => 0,
            Side::Second => 1,
        }
    }
}

/// How an extent spends its [`Side`]s: an extrude's
/// [`ExtentKind`](crate::ExtentKind) or a revolve's
/// [`TurnKind`](crate::TurnKind), but for what each goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sides {
    /// None: through all, or a full turn.
    None,
    /// The first, one way, or the other way flipped.
    One,
    /// The first, half of it either way.
    Symmetric,
    /// Both, the first one way and the second the other, swapped flipped.
    Two,
}

impl Sides {
    /// The values it's given by.
    pub fn fields(self) -> &'static [Side] {
        match self {
            Sides::None => &[],
            Sides::One | Sides::Symmetric => &[Side::First],
            Sides::Two => &[Side::First, Side::Second],
        }
    }

    /// Whether Flip changes it.
    pub fn flips(self) -> bool {
        matches!(self, Sides::One | Sides::Two)
    }

    /// The handle's sign: -1 where `flip` is on and Flip changes it.
    pub(crate) fn sign(self, flip: bool) -> f64 {
        if flip && self.flips() { -1.0 } else { 1.0 }
    }

    /// The panel's Flip toggle, lit while `flip`, sending `message`, for
    /// what Flip changes.
    pub(crate) fn flip_toggle<'a>(
        self,
        flip: bool,
        message: Option<Message>,
    ) -> Option<Element<'a, Message>> {
        self.flips()
            .then(|| toggle(Icon::TkFlip, "Flip", flip, message, None))
    }

    /// Where the handle's knobs go, each [`Side`] given in `values`: how
    /// far along, `sign` -1 where flipped. One side's at its value,
    /// symmetric's at half of it, two sides' each its own way.
    pub(crate) fn knobs(self, values: [Option<f64>; 2], sign: f64) -> Vec<(Side, f64)> {
        let [first, second] = values;
        match self {
            Sides::None => Vec::new(),
            Sides::One => first.map(|a| (Side::First, sign * a)).into_iter().collect(),
            Sides::Symmetric => first.map(|a| (Side::First, a / 2.0)).into_iter().collect(),
            Sides::Two => [
                first.map(|a| (Side::First, sign * a)),
                second.map(|b| (Side::Second, -sign * b)),
            ]
            .into_iter()
            .flatten()
            .collect(),
        }
    }

    /// The fields of the values it's given by, of `fields`: "Side 1" and
    /// "Side 2" for two sides, else `single` (the first is
    /// [`VALUE_FIELD`](crate::VALUE_FIELD), the second `second`), each
    /// sending `input` as it's typed in, if it can be. `Enter` in one
    /// is `commit`, `Esc` `cancel`.
    pub(crate) fn value_fields<'a>(
        self,
        single: &'a str,
        second: iced::widget::Id,
        fields: [TypedField<'a>; 2],
        input: Option<impl Fn(Side, String) -> Message + Copy + 'a>,
        commit: Message,
        cancel: Message,
    ) -> Vec<Element<'a, Message>> {
        (self.fields().iter())
            .map(|&side| {
                let (label, id) = match (self, side) {
                    (Sides::Two, Side::First) => ("Side 1", crate::VALUE_FIELD),
                    (Sides::Two, Side::Second) => ("Side 2", second.clone()),
                    _ => (single, crate::VALUE_FIELD),
                };
                let input = input.map(|input| move |text| input(side, text));
                value_field(
                    label,
                    id,
                    fields[side.index()],
                    input,
                    commit.clone(),
                    cancel.clone(),
                )
            })
            .collect()
    }
}

/// A typed value's field, a distance or an angle: its text, and why it's
/// refused, if it is.
#[derive(Debug, Clone, Copy)]
pub struct TypedField<'a> {
    pub text: &'a str,
    pub error: Option<&'a varde_expr::Error>,
    /// The last value it gave, in model units (millimetres or radians),
    /// which the preview and an extrude's handle show while the text is
    /// refused.
    pub value: Option<f64>,
    /// The design's parameters, whose names are offered as they're typed
    /// (see the `suggest` module).
    pub params: crate::ParamsIn<'a>,
}

/// What an extrude or a revolve does with its solid, see
/// `varde_document::Operation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OperationKind {
    #[default]
    NewBody,
    Join,
    Cut,
    Intersect,
}

/// The operation a combine's `BodyOp` matches, for its icon: the
/// booleans as circles.
impl From<BodyOp> for OperationKind {
    fn from(op: BodyOp) -> Self {
        match op {
            BodyOp::Union => OperationKind::Join,
            BodyOp::Subtract => OperationKind::Cut,
            BodyOp::Intersect => OperationKind::Intersect,
        }
    }
}

impl OperationKind {
    pub const ALL: [OperationKind; 4] = [
        OperationKind::NewBody,
        OperationKind::Join,
        OperationKind::Cut,
        OperationKind::Intersect,
    ];

    /// The kind of `operation`.
    pub fn of(operation: &varde_document::Operation) -> Self {
        use varde_document::Operation;
        match operation {
            Operation::NewBody(_) => OperationKind::NewBody,
            Operation::Join(_) => OperationKind::Join,
            Operation::Cut(_) => OperationKind::Cut,
            Operation::Intersect(_) => OperationKind::Intersect,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            OperationKind::NewBody => "New body",
            OperationKind::Join => "Join",
            OperationKind::Cut => "Cut",
            OperationKind::Intersect => "Intersect",
        }
    }

    /// Its choice's icon: the booleans as circles.
    pub(crate) fn icon(self) -> Icon {
        match self {
            OperationKind::NewBody => Icon::BoNew,
            OperationKind::Join => Icon::BoJoin,
            OperationKind::Cut => Icon::BoCut,
            OperationKind::Intersect => Icon::BoInt,
        }
    }

    /// Whether it works on bodies already there, which the panel then
    /// lists.
    pub fn has_targets(self) -> bool {
        self != OperationKind::NewBody
    }
}

/// What the cursor is over in an operation's panel, for the viewport to
/// light up too: a picked region's row, a revolve's axis's, or a body's (a
/// combine's target or tool, or one a join, cut or intersect touches).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelHover {
    Region {
        sketch: FeatureId,
        region: usize,
    },
    Axis,
    Body(BodyId),
    /// A chamfer's edge, by its place in the list.
    Edge(usize),
    /// A shell's face, by its place in the list.
    Face(usize),
    /// A sweep's path part of a sketch's curves, by its place among
    /// them; a loft's rail, by its place among them.
    Part(usize),
    /// A loft's section, by its place among them.
    Section(usize),
    /// A region an edited feature names that wasn't found, by its place
    /// among them: nothing to light.
    Missing(usize),
}

impl PanelHover {
    /// The region, and its sketch, if it's one.
    pub fn region(self) -> Option<(FeatureId, usize)> {
        match self {
            PanelHover::Region { sketch, region } => Some((sketch, region)),
            PanelHover::Axis
            | PanelHover::Body(_)
            | PanelHover::Edge(_)
            | PanelHover::Face(_)
            | PanelHover::Part(_)
            | PanelHover::Section(_)
            | PanelHover::Missing(_) => None,
        }
    }

    /// The body, if it's one.
    pub fn body(self) -> Option<BodyId> {
        match self {
            PanelHover::Body(body) => Some(body),
            PanelHover::Region { .. }
            | PanelHover::Axis
            | PanelHover::Edge(_)
            | PanelHover::Face(_)
            | PanelHover::Part(_)
            | PanelHover::Section(_)
            | PanelHover::Missing(_) => None,
        }
    }
}

/// The messages telling the app `hover` is entered, and left.
pub(super) fn hovering(hover: PanelHover) -> (Message, Message) {
    (
        Message::Look(Look::HoverPanel(Some(hover))),
        Message::Look(Look::LeavePanel(hover)),
    )
}

/// A body a join, cut or intersect touches, or one taken out of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyTarget<'a> {
    pub body: BodyId,
    pub name: &'a str,
    /// Whether it's worked on: not taken out.
    pub included: bool,
    /// The name of the body an earlier join merged it into, if one did:
    /// it's listed only while it's taken out (or just put back), which
    /// does nothing then, so that can be seen and undone.
    pub holder: Option<&'a str>,
}

/// The button left of Add anyway framing the camera on where a draft
/// fails: Show, which turns into Go back once pressed, turning the camera
/// back to where it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    /// Show, sending [`Look::ShowFailure`].
    Show,
    /// Go back, sending [`Look::BackFromFailure`].
    GoBack,
}

impl Framing {
    /// The button's icon, words and what it sends.
    pub(super) fn button(self) -> (Icon, &'static str, Message) {
        match self {
            Framing::Show => (Icon::Locate, "Show", Message::Look(Look::ShowFailure)),
            Framing::GoBack => (Icon::Back, "Go back", Message::Look(Look::BackFromFailure)),
        }
    }
}

/// The body a join merges the bodies it's ticked for into, if it's
/// ticked for two or more of `targets`: the first made of them, which
/// then holds them all. A body merged away before isn't one of them.
pub(crate) fn joined_into<'a>(
    operation: OperationKind,
    targets: &[BodyTarget<'a>],
) -> Option<&'a str> {
    if operation != OperationKind::Join {
        return None;
    }
    let mut included = (targets.iter()).filter(|target| target.included && target.holder.is_none());
    let first = included.next()?;
    included.next().map(|_| first.name)
}
