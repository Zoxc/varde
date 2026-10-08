//! The list of what overlaps where the left button was held still, to
//! choose one from: in a sketch its points and curves there, in the model
//! its vertices, edges and faces there, with the curves and points of the
//! finished sketches picked with it (or by Project and Intersect). Hovering a row highlights its
//! item, clicking it selects that one as a click on it would, and a click
//! anywhere else closes it, the selection as it was.

use glam::DVec2;
use std::collections::BTreeSet;

use iced::widget::{Space, button, checkbox, column, container, mouse_area, pin, row, stack, text};
use iced::{Alignment, Element, Length};
use varde_document::{Document, OriginPlane};
use varde_sketch::{Selectable, Sketch};

use crate::pick::{Pick, Picked};
use crate::select::{Selection, SketchItem};
use crate::theme;
use crate::{Look, Message};

/// How long the left button is held still, in a sketch or on the model,
/// before what's there is listed.
pub(crate) const HOLD_DELAY: iced::time::Duration = iced::time::Duration::from_millis(500);
/// How near the cursor what's listed shows, in pixels.
pub(crate) const OVERLAP_REACH: f64 = 8.0;
/// The most items listed: a corner seen straight down has its vertex,
/// five edges and five faces about it, hidden ones included.
pub(crate) const MAX_OVERLAPS: usize = 16;
/// How wide the list is, and how tall a row, in pixels.
pub(crate) const WIDTH: f32 = 220.0;
const ROW_HEIGHT: f32 = crate::toolbar::MENU_ITEM_HEIGHT;
const PADDING: f32 = 4.0;
/// How far from where the button was held the list's corner is, in
/// pixels: clear of the cursor, so letting go of the button isn't on a
/// row.
const OFFSET: f32 = 10.0;

/// What overlaps where the button was held, listed there.
#[derive(Debug, Clone, PartialEq)]
pub struct Overlaps {
    /// Where the button was held, in the viewport's pixels: ringed
    /// `OVERLAP_REACH` round, how far it picks.
    pub held: DVec2,
    /// The list's top left corner, in the viewport's pixels.
    pub at: DVec2,
    pub items: OverlapItems,
}

/// The items listed, nearest first.
#[derive(Debug, Clone, PartialEq)]
pub enum OverlapItems {
    /// Of the sketch being edited.
    Sketch(Vec<Selectable>),
    /// Of the model shown.
    Model(Vec<Pick>),
    /// Of the model shown and of the finished sketches picked with it
    /// ([`ModelPicking::sketches`](crate::ModelPicking::sketches)): listed
    /// so only where one of those sketches has something there.
    Mixed(Vec<OverlapItem>),
}

/// A row of a list of the model's overlaps with sketches' items.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OverlapItem {
    /// A face, edge or vertex of the model shown.
    Model(Pick),
    /// A finished sketch's curve or point.
    Sketch(SketchItem),
    /// An origin plane, listed while one can be picked.
    Origin(OriginPlane),
}

impl Overlaps {
    /// The list of `items` for the button held at `held`, in a viewport
    /// `size` big: beside it, and to its left or above it where it would
    /// run off the viewport's right or bottom.
    pub(crate) fn new(held: DVec2, size: [f32; 2], items: OverlapItems) -> Self {
        let [width, height] = size.map(f64::from);
        let (list_width, list_height) = (f64::from(WIDTH), list_height(items.len()));
        let offset = f64::from(OFFSET);
        let place = |from: f64, size: f64, room: f64| {
            if from + offset + size <= room {
                from + offset
            } else {
                (from - offset - size).max(0.0)
            }
        };
        Overlaps {
            held,
            at: DVec2::new(
                place(held.x, list_width, width),
                place(held.y, list_height, height),
            ),
            items,
        }
    }
}

/// How a row of the list of the model's overlaps shows while a session
/// picks the model for itself ([`DocumentState::overlap_ticks`](crate::DocumentState::overlap_ticks)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OverlapTick {
    /// Whether the session has the row's item: a click on it would leave
    /// it as it is or take it out.
    pub ticked: bool,
    pub note: OverlapNote,
    /// Whether a click on the row picks its item alone, never adding it
    /// to others (picking a plane, an origin plane): it has no tick, and
    /// `Ctrl` does nothing.
    pub only: bool,
}

/// What a row's item is to the session, where that's more than the item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlapNote {
    #[default]
    None,
    /// An edge of a tangent chain the session picked: a click on it
    /// takes the chain's edges out. "Chain of Body 1".
    Chain,
    /// The session's own edge or face that its preview has taken away
    /// (rounded off, cut out): listed still, to take out. "Removed edge
    /// of Body 1".
    Removed,
}

impl OverlapItems {
    pub fn len(&self) -> usize {
        match self {
            OverlapItems::Sketch(ids) => ids.len(),
            OverlapItems::Model(picks) => picks.len(),
            OverlapItems::Mixed(items) => items.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// How tall a list of `rows` is, in pixels.
fn list_height(rows: usize) -> f64 {
    f64::from(ROW_HEIGHT) * rows as f64 + f64::from(PADDING) * 2.0
}

/// The list as shown, a menu's rows naming the items of `sketch` or
/// `document`'s bodies, each ticked if it's in the sketch's `selection`
/// or the model's `model_selection` (as `ticks` has it, where it's
/// given: a session's own picks, named as they are to it), over a layer filling the viewport
/// that closes it on a press anywhere else. A row clicked selects its item
/// alone (with `Ctrl`, `Cmd` on macOS, adds it or takes it out, the list
/// kept open); its tick adds it or takes it out alone. A row picking its
/// item only alone ([`OverlapTick::only`]) has no tick.
pub(crate) fn view<'a>(
    overlaps: &Overlaps,
    sketch: Option<(&Sketch, &BTreeSet<Selectable>)>,
    model_selection: &Selection,
    ticks: Option<&[OverlapTick]>,
    document: &Document,
) -> Element<'a, Message> {
    let selected = |pick: &Pick, targets: &[Picked]| {
        model_selection.model() == Some(pick.model) && targets.contains(&pick.target)
    };
    let checked: Vec<bool> = match (&overlaps.items, ticks) {
        (OverlapItems::Model(_) | OverlapItems::Mixed(_), Some(ticks)) => {
            ticks.iter().map(|tick| tick.ticked).collect()
        }
        (OverlapItems::Sketch(ids), _) => (ids.iter())
            .map(|id| sketch.is_some_and(|(_, selection)| selection.contains(id)))
            .collect(),
        (OverlapItems::Model(picks), None) => {
            let targets: Vec<Picked> = model_selection.targets().collect();
            (picks.iter())
                .map(|pick| selected(pick, &targets))
                .collect()
        }
        (OverlapItems::Mixed(items), None) => {
            let targets: Vec<Picked> = model_selection.targets().collect();
            let items_selected: Vec<SketchItem> = model_selection.sketch_items().collect();
            (items.iter())
                .map(|item| match item {
                    OverlapItem::Model(pick) => selected(pick, &targets),
                    OverlapItem::Sketch(item) => items_selected.contains(item),
                    OverlapItem::Origin(_) => false,
                })
                .collect()
        }
    };
    let sketch = sketch.map(|(sketch, _)| sketch);
    let names: Vec<String> = match &overlaps.items {
        OverlapItems::Sketch(ids) => (ids.iter())
            .map(|&id| {
                sketch
                    .and_then(|sketch| sketch.selectable_name(id))
                    .unwrap_or_default()
            })
            .collect(),
        OverlapItems::Model(picks) => (picks.iter().enumerate())
            .map(|(row, pick)| {
                let body = document.body(pick.body).map(|body| body.name.as_str());
                let tick = ticks.and_then(|ticks| ticks.get(row));
                let note = tick.map_or(OverlapNote::None, |tick| tick.note);
                model_name(pick.target, body, note)
            })
            .collect(),
        OverlapItems::Mixed(items) => (items.iter())
            .map(|item| match *item {
                OverlapItem::Model(pick) => {
                    let body = document.body(pick.body).map(|body| body.name.as_str());
                    model_name(pick.target, body, OverlapNote::None)
                }
                OverlapItem::Sketch(item) => sketch_item_name(document, item),
                OverlapItem::Origin(plane) => format!("{} plane", plane.name()),
            })
            .collect(),
    };
    let rows = names.into_iter().enumerate().map(|(index, name)| {
        let only = (ticks.and_then(|ticks| ticks.get(index))).is_some_and(|tick| tick.only);
        let tick = (!only).then(|| {
            checkbox(checked.get(index).copied().unwrap_or(false))
                .size(15)
                .style(theme::tick)
                .on_toggle(move |_| Message::Look(Look::ToggleOverlap(index)))
        });
        let label = row![tick, text(name)]
            .spacing(8)
            .height(Length::Fill)
            .align_y(Alignment::Center);
        let row = button(label)
            .width(Length::Fill)
            .height(ROW_HEIGHT)
            .padding([0, 8])
            .style(theme::flat_button(false, theme::Tone::Text))
            .on_press(Message::Look(Look::ChooseOverlap { index, add: false }));
        mouse_area(row)
            .on_enter(Message::Look(Look::HoverOverlap(Some(index))))
            .on_exit(Message::Look(Look::LeaveOverlap(index)))
            .into()
    });
    let list = container(column(rows).width(WIDTH))
        .padding(PADDING)
        .style(theme::menu);
    let away = mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
        .on_press(Message::Look(Look::CloseOverlaps))
        .on_right_press(Message::Look(Look::CloseOverlaps))
        .on_middle_press(Message::Look(Look::CloseOverlaps));
    let list = pin(list).x(overlaps.at.x as f32).y(overlaps.at.y as f32);
    let reach = OVERLAP_REACH as f32;
    let ring = container(Space::new())
        .width(2.0 * reach)
        .height(2.0 * reach)
        .style(theme::pick_ring);
    let ring = pin(ring)
        .x(overlaps.held.x as f32 - reach)
        .y(overlaps.held.y as f32 - reach);
    stack![away, ring, list].into()
}

/// A face, edge or vertex of the model as a row names it: "Face of Body
/// 1", by its body's name if that's known, as `note` says it is to the
/// session: "Chain of Body 1", "Removed edge of Body 1".
fn model_name(target: Picked, body: Option<&str>, note: OverlapNote) -> String {
    let kind = match (target, note) {
        (Picked::Edge(_), OverlapNote::Chain) => "Chain",
        (Picked::Face(_), OverlapNote::Removed) => "Removed face",
        (Picked::Edge(_), OverlapNote::Removed) => "Removed edge",
        (Picked::Vertex(_), OverlapNote::Removed) => "Removed vertex",
        (Picked::Face(_), _) => "Face",
        (Picked::Edge(_), _) => "Edge",
        (Picked::Vertex(_), _) => "Vertex",
    };
    match body {
        Some(body) => format!("{kind} of {body}"),
        None => kind.to_owned(),
    }
}

/// A finished sketch's curve or point as a row names it: "Line 3 of
/// Sketch 2", as the status bar names the one selected.
fn sketch_item_name(document: &Document, item: SketchItem) -> String {
    let Some(feature) = document.feature(item.sketch) else {
        return String::new();
    };
    let name = match &feature.kind {
        varde_document::FeatureKind::Sketch { sketch, .. } => sketch.name(item.item),
        _ => None,
    };
    match name {
        Some(name) => format!("{name} of {}", feature.name),
        None => feature.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sketch_s_item_is_named_with_its_sketch() {
        let document = Document::example();
        let feature = &document.features()[0];
        let varde_document::FeatureKind::Sketch { sketch, .. } = &feature.kind else {
            panic!("the example's first feature is its sketch");
        };
        let curve = sketch.curves[0].id;
        let item = SketchItem {
            sketch: feature.id,
            item: curve,
        };
        let wanted = format!("{} of {}", sketch.name(curve).unwrap(), feature.name);
        assert_eq!(sketch_item_name(&document, item), wanted);
    }

    #[test]
    fn the_list_goes_beside_the_cursor_or_back_inside_the_viewport() {
        let items = OverlapItems::Sketch(vec![
            varde_sketch::Id::ORIGIN.into(),
            varde_sketch::Id::X_AXIS.into(),
        ]);
        let beside = Overlaps::new(DVec2::new(20.0, 30.0), [800.0, 600.0], items.clone());
        assert_eq!(beside.at, DVec2::new(30.0, 40.0));
        assert_eq!(beside.held, DVec2::new(20.0, 30.0));
        let corner = Overlaps::new(DVec2::new(790.0, 590.0), [800.0, 600.0], items);
        let height = list_height(2);
        assert_eq!(
            corner.at,
            DVec2::new(790.0 - 10.0 - f64::from(WIDTH), 590.0 - 10.0 - height)
        );
    }

    #[test]
    fn a_model_item_is_named_by_its_kind_and_body() {
        assert_eq!(
            model_name(Picked::Edge(3), Some("Body 2"), OverlapNote::None),
            "Edge of Body 2"
        );
        assert_eq!(
            model_name(Picked::Vertex(0), None, OverlapNote::None),
            "Vertex"
        );
        // As a session has it: an edge of a picked chain, a face its
        // preview took away.
        assert_eq!(
            model_name(Picked::Edge(3), Some("Body 2"), OverlapNote::Chain),
            "Chain of Body 2"
        );
        assert_eq!(
            model_name(Picked::Face(1), Some("Body 2"), OverlapNote::Removed),
            "Removed face of Body 2"
        );
    }
}
