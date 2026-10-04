//! The extrude being set up: what the app hands the view of it, the
//! messages changing it, where its handle goes, and the floating panel
//! holding its options over the right of the viewport. The viewport's
//! side, picking regions and dragging the handle, is in
//! `viewport/extrude.rs`.

use std::collections::BTreeSet;

use glam::{DVec2, DVec3};
use iced::Element;
use iced::widget::{column, text};
use varde_document::{BodyId, ExtrudeError, FeatureId, Placement};
use varde_expr::LengthUnit;
use varde_sketch::{Region, angle};

use crate::chrome::tip;
use crate::icons::Icon;
use crate::operation_panel::{
    BodyTarget, Candidate, Footer, Framing, OperationKind, PanelHover, Parts, TypedField, bodies,
    field, footer_message, message_text, operation_panel, pick_field, picked_row, tile, tiles,
    toggle, value_field,
};
use crate::theme;
use crate::{Edit, Look, Message, VALUE_FIELD};

/// The field of an extrude's second distance, for two sides. The first
/// is [`VALUE_FIELD`], which takes the focus as the session opens.
const SECOND_FIELD: iced::widget::Id = iced::widget::Id::new("extrude-second");

/// The field of an extrude's taper.
const TAPER_FIELD: iced::widget::Id = iced::widget::Id::new("extrude-taper");

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

    /// Its choice's icon: the profile as a slab, and where it goes.
    fn icon(self) -> Icon {
        match self {
            ExtentKind::OneSide => Icon::ExOne,
            ExtentKind::Symmetric => Icon::ExSym,
            ExtentKind::TwoSides => Icon::ExTwo,
            ExtentKind::ThroughAll => Icon::ExThru,
        }
    }

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
    /// The text in the taper's field, as typed: an angle the walls lean
    /// by, 0° for none.
    Taper(String),
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
    pub fields: [TypedField<'a>; 2],
    pub flip: bool,
    /// The taper's field: an angle, 0° for none.
    pub taper: TypedField<'a>,
    pub operation: OperationKind,
    /// For a join, cut or intersect, the bodies its preview touches and
    /// those taken out of it, in the order they were made.
    pub targets: Vec<BodyTarget<'a>>,
    /// The knob grabbed, if one is.
    pub grabbed: Option<Distance>,
    /// Why the preview failed, if it did.
    pub error: Option<&'a str>,
    /// The button framing the camera on the geometry of why the preview
    /// failed, or going back from it, if that geometry has a box: beside
    /// [`ExtrudeState::error`]'s Add anyway.
    pub show_error: Option<Framing>,
    /// Why the extrude as set up can't be committed, if its own check
    /// refuses it (two sides over the limit together, say): shown in
    /// place of [`ExtrudeState::error`].
    pub refused: Option<ExtrudeError>,
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
    /// ([`ExtrudeState::error`]), and the operation could be committed otherwise.
    pub accept: bool,
    /// Whether the document can be changed.
    pub editable: bool,
    /// The design's units, which snapped distances are typed in.
    pub units: LengthUnit,
    /// The row of the panel the cursor is over, if any: the viewport
    /// lights it up too.
    pub hover: Option<PanelHover>,
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
        let placement = source.placement;
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
/// that isn't finite and above zero, or a step past the largest double.
///
/// The step is saved with the extrude it snaps, so it's made the same
/// natively and on the web: its decade by libm's `log10`, and the step
/// the nearest double to its decimal, read from its text (exact and
/// portable, where `pow(10, k)` can be an ulp off for negative `k`).
pub fn snap_step(pixel: f64, units: LengthUnit) -> Option<f64> {
    let least = pixel * SNAP_PIXELS / units.mm();
    if !(least > 0.0 && least.is_finite()) {
        return None;
    }
    // Within ±324 for any positive finite `least`. Should the logarithm
    // round down across a power of ten, the 10 still finds that power.
    let decade = angle::log10(least).floor() as i32;
    let step = [(1, 0), (2, 0), (5, 0), (1, 1)]
        .into_iter()
        .filter_map(|(m, up)| format!("{m}e{}", decade + up).parse::<f64>().ok())
        .find(|&step| step >= least)?;
    Some(step * units.mm()).filter(|step| step.is_finite())
}

/// The floating panel of the extrude being set up.
pub(crate) fn panel<'a>(state: &ExtrudeState<'a>) -> Element<'a, Message> {
    let editable = state.editable;
    let send = |look: ExtrudeLook| editable.then_some(Message::Look(Look::Extrude(look)));

    // The regions picked, each with a cross taking it out; clicks always
    // pick regions, so the field is always the one picking.
    let rows = (state.source.into_iter())
        .flat_map(|sketch| {
            state.picked.iter().map(move |&region| {
                picked_row(
                    Icon::SeRegion,
                    region_name(region),
                    None,
                    send(ExtrudeLook::PickRegion { sketch, region }),
                    None,
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
    let place = if state.picked.is_empty() && empty {
        "No closed regions to extrude"
    } else if state.picked.is_empty() {
        "Click regions to extrude"
    } else {
        "Click regions"
    };
    let profile = field(
        "Profile",
        pick_field(rows, Some(place.to_owned()), true, None),
    );
    let missing = (state.missing > 0).then(|| {
        let note = match state.missing {
            1 => "1 region wasn't found".to_owned(),
            n => format!("{n} regions weren't found"),
        };
        text(note).size(12).style(theme::danger_text)
    });

    let extents = ExtentKind::ALL.map(|kind| {
        // Through all only cuts.
        let through = kind == ExtentKind::ThroughAll && state.operation != OperationKind::Cut;
        let message = send(ExtrudeLook::Extent(kind)).filter(|_| !through);
        let tile = tile(kind.icon(), kind.label(), state.extent == kind, message);
        if through {
            tip(tile, text("Only a cut goes through all"))
        } else {
            tile
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
    let flip = state.extent.flips().then(|| {
        toggle(
            Icon::TkFlip,
            "Flip",
            state.flip,
            send(ExtrudeLook::Flip),
            None,
        )
    });
    let taper = value_field(
        "Taper",
        TAPER_FIELD,
        state.taper,
        editable.then_some(|text| Message::Look(Look::Extrude(ExtrudeLook::Taper(text)))),
        Message::Edit(Edit::CommitExtrude),
        // The distance's field sends it, where there is one.
        (state.extent.distances().is_empty())
            .then_some(Message::Look(Look::Extrude(ExtrudeLook::Cancel))),
    );
    let operations = OperationKind::ALL.map(|kind| {
        tile(
            kind.icon(),
            kind.label(),
            state.operation == kind,
            send(ExtrudeLook::Operation(kind)),
        )
    });
    let targets = bodies(
        state.operation,
        &state.targets,
        |body| send(ExtrudeLook::Target(body)),
        state.hover,
    );
    // Why OK can't be pressed, or the preview failed, or that OK waits
    // on the solver.
    let refused = (state.refused.map(|refused| refused.to_string())).or_else(|| state.held.clone());
    let message = footer_message(
        "Extrude",
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
        missing,
        field("Extent", tiles(extents)),
        column(fields).spacing(8),
        flip,
        taper,
        field("Operation", tiles(operations)),
        targets,
    ]
    .spacing(10);
    operation_panel(Parts {
        icon: Icon::Extrude,
        title: state.editing.unwrap_or("New extrude"),
        body: body.into(),
        message,
        ok: state.ready.then_some(Message::Edit(Edit::CommitExtrude)),
        cancel: Message::Look(Look::Extrude(ExtrudeLook::Cancel)),
        close: false,
    })
}

/// What a region picked is called in the panel: "Region 1", by its index
/// in its sketch's profiles.
pub(crate) fn region_name(region: usize) -> String {
    format!("Region {}", region.saturating_add(1))
}

/// The field of `distance`, named `label`, showing why its text is
/// refused under it. `Enter` in it is OK, `Esc` Cancel.
fn distance_field<'a>(
    label: &'a str,
    distance: Distance,
    field: TypedField<'a>,
    editable: bool,
) -> Element<'a, Message> {
    let id = match distance {
        Distance::First => VALUE_FIELD,
        Distance::Second => SECOND_FIELD,
    };
    let input = editable
        .then_some(move |text| Message::Look(Look::Extrude(ExtrudeLook::Input { distance, text })));
    value_field(
        label,
        id,
        field,
        input,
        Message::Edit(Edit::CommitExtrude),
        // One field sends it, so `Esc` cancels once.
        (distance == Distance::First).then_some(Message::Look(Look::Extrude(ExtrudeLook::Cancel))),
    )
}

#[cfg(test)]
mod tests;
