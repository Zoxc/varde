//! The extrude being set up: what the app hands the view of it, the
//! messages changing it, where its handle goes, and the floating panel
//! holding its options over the right of the viewport. The viewport's
//! side, picking regions and dragging the handle, is in
//! `viewport/extrude.rs`.

use std::collections::BTreeSet;

use glam::DVec3;
use iced::Element;
use iced::widget::{column, text};
use varde_document::{BodyId, FeatureId, Placement};
use varde_expr::LengthUnit;

use crate::chrome::tip;
use crate::icons::Icon;
use crate::operation_panel::{
    BodyTarget, Candidate, OperationKind, PanelHover, Parts, Side, Sides, TypedField, Verdict,
    field, operation_panel, operation_section, pick_field, region_rows, tile, tiles, value_field,
};
use crate::{Edit, Look, Message};

/// The field of an extrude's second distance, for two sides. The first
/// is [`VALUE_FIELD`](crate::VALUE_FIELD), which takes the focus as the session opens.
const SECOND_FIELD: iced::widget::Id = iced::widget::Id::new("extrude-second");

/// The field of an extrude's taper.
const TAPER_FIELD: iced::widget::Id = iced::widget::Id::new("extrude-taper");

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
        self.sides().fields()
    }

    /// Whether Flip changes it.
    pub fn flips(self) -> bool {
        self.sides().flips()
    }

    /// How it spends its distances.
    pub fn sides(self) -> Sides {
        match self {
            ExtentKind::OneSide => Sides::One,
            ExtentKind::Symmetric => Sides::Symmetric,
            ExtentKind::TwoSides => Sides::Two,
            ExtentKind::ThroughAll => Sides::None,
        }
    }
}

/// One of an extrude's distances: the first, which one side and
/// symmetric have too, or the second of two sides, against the normal.
pub type Distance = Side;

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
    /// Takes out the edited extrude's `n`th region that wasn't found:
    /// its row's cross.
    DropMissing(usize),
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
    /// Its title, OK and foot.
    pub verdict: Verdict<'a>,
    /// The sketches whose regions show and can be picked: the extrude's
    /// sketch once there is one, else every visible sketch with regions.
    pub candidates: Vec<Candidate<'a>>,
    /// The sketch the extrude takes regions of, once there is one: the
    /// only candidate then.
    pub source: Option<FeatureId>,
    /// The regions picked, by their index in the source's profiles.
    pub picked: &'a BTreeSet<usize>,
    /// How many of the edited extrude's regions weren't found: listed
    /// after those picked, each with a cross.
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

impl<'a> ExtrudeState<'a> {
    /// The panel's title: the feature edited's name, or "New extrude".
    pub(crate) fn title(&self) -> &'a str {
        self.verdict.title("New extrude")
    }

    /// What the status bar says of it: the regions picked.
    pub(crate) fn status_info(&self) -> String {
        crate::words::regions_picked(self.picked.len(), "extrude")
    }
}

impl ExtrudeState<'_> {
    /// The source sketch, if it's chosen.
    pub(crate) fn source(&self) -> Option<&Candidate<'_>> {
        Candidate::find(&self.candidates, self.source)
    }

    /// The handle, once regions are picked and the extent has distances.
    pub fn handle(&self) -> Option<Handle> {
        let source = self.source()?;
        let centre = source.centre_of(self.picked)?;
        let placement = source.placement;
        let sign = self.extent.sides().sign(self.flip);
        let knobs = (self.extent.sides()).knobs(self.fields.map(|field| field.value), sign);
        let handle = Handle {
            origin: centre,
            normal: placement.normal,
            knobs,
        };
        (handle.origin.is_finite() && !handle.knobs.is_empty()).then_some(handle)
    }
}

/// The floating panel of the extrude being set up.
pub(crate) fn panel<'a>(state: &ExtrudeState<'a>) -> Element<'a, Message> {
    let editable = state.editable;
    let send = |look: ExtrudeLook| editable.then_some(Message::Look(Look::Extrude(look)));

    // The regions picked, each with a cross taking it out; clicks always
    // pick regions, so the field is always the one picking.
    let rows = region_rows(
        state.source,
        state.picked,
        state.missing,
        |sketch, region| send(ExtrudeLook::PickRegion { sketch, region }),
        |index| send(ExtrudeLook::DropMissing(index)),
        None,
        state.hover,
    );
    let empty = Candidate::none_closed(&state.candidates);
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
    let fields = state.extent.sides().value_fields(
        "Distance",
        SECOND_FIELD,
        state.fields,
        editable.then_some(|distance, text| {
            Message::Look(Look::Extrude(ExtrudeLook::Input { distance, text }))
        }),
        Message::Edit(Edit::CommitExtrude),
        Message::Look(Look::Extrude(ExtrudeLook::Cancel)),
    );
    let flip = (state.extent.sides()).flip_toggle(state.flip, send(ExtrudeLook::Flip));
    let taper = value_field(
        "Taper",
        TAPER_FIELD,
        state.taper,
        editable.then_some(|text| Message::Look(Look::Extrude(ExtrudeLook::Taper(text)))),
        Message::Edit(Edit::CommitExtrude),
        Message::Look(Look::Extrude(ExtrudeLook::Cancel)),
    );
    let operation = operation_section(
        state.operation,
        &state.targets,
        |kind| send(ExtrudeLook::Operation(kind)),
        |body| send(ExtrudeLook::Target(body)),
        state.hover,
    );
    // Why OK can't be pressed, or the preview failed, or that OK waits
    // on the solver.
    let message = state.verdict.footer("Extrude");

    let body = column![
        profile,
        field("Extent", tiles(extents)),
        column(fields).spacing(8),
        flip,
        taper,
        operation,
    ]
    .spacing(10);
    operation_panel(Parts {
        icon: Icon::Extrude,
        title: state.title(),
        body: body.into(),
        message,
        ok: state.verdict.ok(Message::Edit(Edit::CommitExtrude)),
        cancel: Message::Look(Look::Extrude(ExtrudeLook::Cancel)),
        close: false,
    })
}

#[cfg(test)]
mod tests;
