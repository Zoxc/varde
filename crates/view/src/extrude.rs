//! The extrude being set up: what the app hands the view of it, the
//! messages changing it, where its handle goes, and the floating panel
//! holding its options over the right of the viewport. The viewport's
//! side, picking regions and dragging the handle, is in
//! `viewport/extrude.rs`.

use std::collections::BTreeSet;
use std::sync::Arc;

use glam::{DVec2, DVec3};
use iced::widget::{
    Space, button, checkbox, column, container, opaque, row, space, text, text_input,
};
use iced::{Alignment, Element, Length};
use varde_document::{BodyId, ExtrudeError, FeatureId, Placement, Plane};
use varde_expr::LengthUnit;
use varde_sketch::{Profiles, Region};

use crate::chrome::{hrule, small_button, tip};
use crate::escape::OnEscape;
use crate::theme::{self, Emphasis, SEMIBOLD};
use crate::{Edit, Look, Message, VALUE_FIELD};

/// The field of an extrude's second distance, for two sides. The first
/// is [`VALUE_FIELD`], which takes the focus as the session opens.
const SECOND_FIELD: iced::widget::Id = iced::widget::Id::new("extrude-second");

/// How wide the panel is, in pixels.
const PANEL_WIDTH: f32 = 264.0;

/// How far below the viewport's top the panel starts, clear of the
/// camera controls and the view cube, in pixels.
pub(crate) const PANEL_TOP: f32 = 150.0;

/// How far the handle's snapping steps are apart at least, in pixels at
/// the target: a step is the roundest length in the design's units at
/// least this many pixels long.
const SNAP_PIXELS: f64 = 6.0;

/// How an extrude's extent is given, see `varde_document::Extent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExtentKind {
    #[default]
    OneSide,
    Symmetric,
    TwoSides,
    ThroughAll,
}

impl ExtentKind {
    pub const ALL: [ExtentKind; 4] = [
        ExtentKind::OneSide,
        ExtentKind::Symmetric,
        ExtentKind::TwoSides,
        ExtentKind::ThroughAll,
    ];

    fn label(self) -> &'static str {
        match self {
            ExtentKind::OneSide => "One side",
            ExtentKind::Symmetric => "Symmetric",
            ExtentKind::TwoSides => "Two sides",
            ExtentKind::ThroughAll => "Through all",
        }
    }

    /// The distances it's given by.
    pub fn distances(self) -> &'static [Distance] {
        match self {
            ExtentKind::OneSide | ExtentKind::Symmetric => &[Distance::First],
            ExtentKind::TwoSides => &[Distance::First, Distance::Second],
            ExtentKind::ThroughAll => &[],
        }
    }

    /// Whether Flip changes it.
    pub fn flips(self) -> bool {
        matches!(self, ExtentKind::OneSide | ExtentKind::TwoSides)
    }
}

/// What an extrude does with its solid, see `varde_document::Operation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OperationKind {
    #[default]
    NewBody,
    Join,
    Cut,
    Intersect,
}

impl OperationKind {
    pub const ALL: [OperationKind; 4] = [
        OperationKind::NewBody,
        OperationKind::Join,
        OperationKind::Cut,
        OperationKind::Intersect,
    ];

    fn label(self) -> &'static str {
        match self {
            OperationKind::NewBody => "New body",
            OperationKind::Join => "Join",
            OperationKind::Cut => "Cut",
            OperationKind::Intersect => "Intersect",
        }
    }

    /// Whether it works on bodies already there, which the panel then
    /// lists.
    pub fn has_targets(self) -> bool {
        self != OperationKind::NewBody
    }
}

/// One of an extrude's distances: the first, which one side and
/// symmetric have too, or the second of two sides, against the normal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Distance {
    First,
    Second,
}

impl Distance {
    /// Where it's kept in an array of both.
    pub fn index(self) -> usize {
        match self {
            Distance::First => 0,
            Distance::Second => 1,
        }
    }
}

/// A change to the extrude being set up, see [`Look::Extrude`].
#[derive(Debug, Clone, PartialEq)]
pub enum ExtrudeLook {
    /// A region of the sketch `sketch` clicked: added, or taken out if
    /// it's picked. The first sets the sketch the extrude takes regions
    /// of, if none is yet.
    PickRegion {
        sketch: FeatureId,
        region: usize,
    },
    Extent(ExtentKind),
    /// The text in a distance's field, as typed.
    Input {
        distance: Distance,
        text: String,
    },
    /// Turns the direction round, for one side and two sides.
    Flip,
    /// Chooses the operation. Leaving a cut, through all goes back to
    /// one side.
    Operation(OperationKind),
    /// Takes a body out of a join, cut or intersect, or puts it back.
    Target(BodyId),
    /// A knob of the handle pressed: the viewport follows the cursor
    /// until it's let go of.
    GrabHandle(Distance),
    /// The knob of `distance` dragged to `to`, in millimetres along the
    /// sketch plane's normal from the picked regions' centre, snapped:
    /// where the knob goes, on either side.
    DragHandle {
        distance: Distance,
        to: f64,
    },
    /// The knob grabbed let go of.
    DropHandle,
    /// Drops the extrude being set up, changing nothing: Cancel, or `Esc`.
    Cancel,
}

/// A sketch whose regions can be picked, and where they are.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    pub feature: FeatureId,
    pub plane: Plane,
    pub profiles: &'a Arc<Profiles>,
}

/// A distance's field: its text, and why it's refused, if it is.
#[derive(Debug, Clone, Copy)]
pub struct DistanceField<'a> {
    pub text: &'a str,
    pub error: Option<&'a varde_expr::Error>,
    /// The last value it gave, in millimetres, which the preview and the
    /// handle show while the text is refused.
    pub value: Option<f64>,
}

/// A body a join, cut or intersect touches, or one taken out of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtrudeTarget<'a> {
    pub body: BodyId,
    pub name: &'a str,
    /// Whether it's worked on: not taken out.
    pub included: bool,
}

/// The extrude being set up, and how it's shown.
#[derive(Debug, Clone)]
pub struct ExtrudeState<'a> {
    /// The name of the extrude edited, or none for a new one.
    pub editing: Option<&'a str>,
    /// The sketches whose regions show and can be picked: the extrude's
    /// sketch once there is one, else every visible sketch with regions.
    pub candidates: Vec<Candidate<'a>>,
    /// The sketch the extrude takes regions of, once there is one: the
    /// only candidate then.
    pub source: Option<FeatureId>,
    /// The regions picked, by their index in the source's profiles.
    pub picked: &'a BTreeSet<usize>,
    /// How many of the edited extrude's regions weren't found.
    pub missing: usize,
    pub extent: ExtentKind,
    /// The first distance's field, and two sides' second.
    pub fields: [DistanceField<'a>; 2],
    pub flip: bool,
    pub operation: OperationKind,
    /// For a join, cut or intersect, the bodies its preview touches and
    /// those taken out of it, in the order they were made.
    pub targets: Vec<ExtrudeTarget<'a>>,
    /// The knob grabbed, if one is.
    pub grabbed: Option<Distance>,
    /// Why the preview failed, if it did.
    pub error: Option<&'a str>,
    /// Why the extrude as set up can't be committed, if its own check
    /// refuses it (two sides over the limit together, say): shown in
    /// place of [`ExtrudeState::error`].
    pub refused: Option<ExtrudeError>,
    /// Whether sketch edits have waited on the solver long enough to say
    /// so: OK waits for them, and the panel says why.
    pub checking: bool,
    /// Whether OK can be pressed.
    pub ready: bool,
    /// Whether the document can be changed.
    pub editable: bool,
    /// The design's units, which snapped distances are typed in.
    pub units: LengthUnit,
}

/// The handle: an arrow from the picked regions' centre along the sketch
/// plane's normal, with a knob at each distance.
#[derive(Debug, Clone, PartialEq)]
pub struct Handle {
    /// The picked regions' centre, in the world.
    pub origin: DVec3,
    /// The sketch plane's normal.
    pub normal: DVec3,
    /// Each knob, and how far along the normal it is, in millimetres.
    pub knobs: Vec<(Distance, f64)>,
}

impl Handle {
    /// A placement whose x axis is the handle's: the sketch point `(t, 0)`
    /// is `t` along it, for anchoring and projecting.
    pub(crate) fn placement(&self) -> Placement {
        let x = self.normal;
        let y = x.any_orthonormal_vector();
        Placement {
            origin: self.origin,
            x,
            y,
            normal: x.cross(y),
        }
    }
}

impl ExtrudeState<'_> {
    /// The source sketch, if it's chosen.
    pub(crate) fn source(&self) -> Option<&Candidate<'_>> {
        let source = self.source?;
        self.candidates.iter().find(|c| c.feature == source)
    }

    /// The handle, once regions are picked and the extent has distances.
    pub fn handle(&self) -> Option<Handle> {
        let source = self.source()?;
        let regions = self
            .picked
            .iter()
            .filter_map(|&index| source.profiles.regions.get(index));
        let centre = centroid(regions)?;
        let placement = source.plane.placement();
        let value = |distance: Distance| self.fields[distance.index()].value;
        let sign = if self.flip && self.extent.flips() {
            -1.0
        } else {
            1.0
        };
        let knobs: Vec<(Distance, f64)> = match self.extent {
            ExtentKind::OneSide => value(Distance::First)
                .map(|d| (Distance::First, sign * d))
                .into_iter()
                .collect(),
            ExtentKind::Symmetric => value(Distance::First)
                .map(|d| (Distance::First, d / 2.0))
                .into_iter()
                .collect(),
            ExtentKind::TwoSides => [
                value(Distance::First).map(|a| (Distance::First, sign * a)),
                value(Distance::Second).map(|b| (Distance::Second, -sign * b)),
            ]
            .into_iter()
            .flatten()
            .collect(),
            ExtentKind::ThroughAll => Vec::new(),
        };
        let handle = Handle {
            origin: placement.to_world(centre),
            normal: placement.normal,
            knobs,
        };
        (handle.origin.is_finite() && !handle.knobs.is_empty()).then_some(handle)
    }
}

/// The centre of `regions` together, weighing each loop of their outlines
/// by its area (holes taking theirs away), or of their boxes if that
/// comes to nothing.
fn centroid<'r>(regions: impl Iterator<Item = &'r Region>) -> Option<DVec2> {
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
fn loop_moment(polyline: &[DVec2]) -> (f64, DVec2) {
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

/// The step, in millimetres, a length dragged at `pixel` millimetres a
/// pixel snaps to in `units`: the roundest 1, 2 or 5 times a power of ten
/// of the units at least a few pixels long. `None` for a pixel
/// that isn't finite and above zero.
pub fn snap_step(pixel: f64, units: LengthUnit) -> Option<f64> {
    let least = pixel * SNAP_PIXELS / units.mm();
    if !(least > 0.0 && least.is_finite()) {
        return None;
    }
    let power = 10f64.powi(least.log10().floor() as i32);
    let step = [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * power)
        .find(|&step| step >= least)?;
    Some(step * units.mm())
}

/// The floating panel of the extrude being set up.
pub(crate) fn panel<'a>(state: &ExtrudeState<'a>) -> Element<'a, Message> {
    let title = text(state.editing.unwrap_or("New extrude"))
        .size(13)
        .font(SEMIBOLD);
    let regions = match state.picked.len() {
        0 if state
            .candidates
            .iter()
            .all(|candidate| candidate.profiles.regions.is_empty()) =>
        {
            "No closed regions to extrude".to_owned()
        }
        0 => "Click regions to extrude".to_owned(),
        1 => "1 region".to_owned(),
        n => format!("{n} regions"),
    };
    let missing = (state.missing > 0).then(|| {
        let note = match state.missing {
            1 => "1 region wasn't found".to_owned(),
            n => format!("{n} regions weren't found"),
        };
        text(note).size(12).style(theme::danger_text)
    });
    let editable = state.editable;
    let send = |look: ExtrudeLook| editable.then_some(Message::Look(Look::Extrude(look)));

    let extents = ExtentKind::ALL.map(|kind| {
        // Through all only cuts.
        let through = kind == ExtentKind::ThroughAll && state.operation != OperationKind::Cut;
        let message = send(ExtrudeLook::Extent(kind)).filter(|_| !through);
        let choice = choice(kind.label(), state.extent == kind, message);
        if through {
            tip(choice, text("Only a cut goes through all"))
        } else {
            choice
        }
    });
    let fields = state.extent.distances().iter().map(|&distance| {
        let label = match (state.extent, distance) {
            (ExtentKind::TwoSides, Distance::First) => "Side 1",
            (ExtentKind::TwoSides, Distance::Second) => "Side 2",
            _ => "Distance",
        };
        distance_field(label, distance, state.fields[distance.index()], editable)
    });
    let flip = state
        .extent
        .flips()
        .then(|| choice("Flip", state.flip, send(ExtrudeLook::Flip)));
    let operations = OperationKind::ALL.map(|kind| {
        choice(
            kind.label(),
            state.operation == kind,
            send(ExtrudeLook::Operation(kind)),
        )
    });
    let targets = (state.operation.has_targets() && !state.targets.is_empty()).then(|| {
        let rows = state.targets.iter().map(|&target| {
            checkbox(target.included)
                .label(target.name)
                .size(14)
                .text_size(12)
                .on_toggle_maybe(editable.then_some(move |_| {
                    Message::Look(Look::Extrude(ExtrudeLook::Target(target.body)))
                }))
                .into()
        });
        column![heading("Bodies"), column(rows).spacing(4)].spacing(6)
    });
    // Why OK can't be pressed, or the preview failed, or that OK waits
    // on the solver.
    let error = match (state.refused, state.error) {
        (Some(refused), _) => Some(text(refused.to_string()).size(12).style(theme::danger_text)),
        (None, Some(error)) => Some(text(error).size(12).style(theme::danger_text)),
        (None, None) => state.checking.then(|| {
            text("Checking the sketch…")
                .size(12)
                .style(theme::muted_text)
        }),
    };
    let buttons = row![
        space::horizontal(),
        small_button("Cancel", Emphasis::Secondary)
            .on_press(Message::Look(Look::Extrude(ExtrudeLook::Cancel))),
        small_button("OK", Emphasis::Primary)
            .on_press_maybe(state.ready.then_some(Message::Edit(Edit::CommitExtrude))),
    ]
    .spacing(6);

    let content = column![
        row![
            title,
            space::horizontal(),
            text(regions).size(12).style(theme::muted_text)
        ]
        .align_y(Alignment::Center),
        missing,
        hrule(),
        heading("Extent"),
        grid(extents),
        column(fields).spacing(4),
        flip,
        hrule(),
        heading("Operation"),
        grid(operations),
        targets,
        error,
        Space::new().height(2),
        buttons,
    ]
    .spacing(6)
    .width(PANEL_WIDTH);
    opaque(container(content).padding(10).style(theme::float_panel))
}

/// A small heading in the panel.
fn heading<'a>(label: &'a str) -> Element<'a, Message> {
    text(label)
        .size(11.5)
        .font(SEMIBOLD)
        .style(theme::muted_text)
        .into()
}

/// Four choices in two rows of two.
fn grid<'a>(choices: [Element<'a, Message>; 4]) -> Element<'a, Message> {
    let [a, b, c, d] = choices;
    column![row![a, b].spacing(4), row![c, d].spacing(4)]
        .spacing(4)
        .into()
}

/// A choice of the panel's, highlighted while `on`, sending `message`, or
/// disabled without one.
fn choice<'a>(label: &'a str, on: bool, message: Option<Message>) -> Element<'a, Message> {
    button(
        text(label)
            .size(12)
            .width(Length::Fill)
            .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([3, 6])
    .style(theme::flat_button(on))
    .on_press_maybe(message)
    .into()
}

/// The field of `distance`, named `label`, showing why its text is
/// refused under it. `Enter` in it is OK, `Esc` Cancel.
fn distance_field<'a>(
    label: &'a str,
    distance: Distance,
    field: DistanceField<'a>,
    editable: bool,
) -> Element<'a, Message> {
    let id = match distance {
        Distance::First => VALUE_FIELD,
        Distance::Second => SECOND_FIELD,
    };
    let input = text_input("Distance", field.text)
        .id(id)
        .size(12)
        .padding([2, 4])
        .width(Length::Fill);
    let input = if editable {
        input
            .on_input(move |text| {
                Message::Look(Look::Extrude(ExtrudeLook::Input { distance, text }))
            })
            .on_submit(Message::Edit(Edit::CommitExtrude))
    } else {
        input
    };
    let input = OnEscape::new(input, Message::Look(Look::Extrude(ExtrudeLook::Cancel)));
    let error = field
        .error
        .map(|error| text(error.to_string()).size(11.5).style(theme::danger_text));
    column![
        row![text(label).size(12).width(64), input].align_y(Alignment::Center),
        error,
    ]
    .spacing(2)
    .into()
}

#[cfg(test)]
mod tests;
