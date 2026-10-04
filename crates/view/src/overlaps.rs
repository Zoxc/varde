//! The list of what overlaps where the left button was held still, to
//! choose one from: in a sketch its points and curves there, in the model
//! its vertices, edges and faces there. Hovering a row highlights its
//! item, clicking it selects that one as a click on it would, and a click
//! anywhere else closes it, the selection as it was.

use glam::DVec2;
use std::collections::BTreeSet;

use iced::widget::{Space, button, checkbox, column, container, mouse_area, pin, row, stack, text};
use iced::{Alignment, Element, Length};
use varde_document::Document;
use varde_sketch::{Id, Sketch};

use crate::pick::{Pick, Picked};
use crate::select::Selection;
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
const WIDTH: f32 = 180.0;
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
    Sketch(Vec<Id>),
    /// Of the model shown.
    Model(Vec<Pick>),
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

impl OverlapItems {
    pub fn len(&self) -> usize {
        match self {
            OverlapItems::Sketch(ids) => ids.len(),
            OverlapItems::Model(picks) => picks.len(),
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
/// or the model's `model_selection`, over a layer filling the viewport
/// that closes it on a press anywhere else. A row clicked selects its item
/// alone (with `Ctrl`, `Cmd` on macOS, adds it or takes it out, the list
/// kept open); its tick adds it or takes it out alone.
pub(crate) fn view<'a>(
    overlaps: &Overlaps,
    sketch: Option<(&Sketch, &BTreeSet<Id>)>,
    model_selection: &Selection,
    document: &Document,
) -> Element<'a, Message> {
    let checked: Vec<bool> = match &overlaps.items {
        OverlapItems::Sketch(ids) => (ids.iter())
            .map(|id| sketch.is_some_and(|(_, selection)| selection.contains(id)))
            .collect(),
        OverlapItems::Model(picks) => {
            let targets: Vec<Picked> = model_selection.targets().collect();
            (picks.iter())
                .map(|pick| {
                    model_selection.model() == Some(pick.model) && targets.contains(&pick.target)
                })
                .collect()
        }
    };
    let sketch = sketch.map(|(sketch, _)| sketch);
    let names: Vec<String> = match &overlaps.items {
        OverlapItems::Sketch(ids) => (ids.iter())
            .map(|&id| {
                sketch
                    .and_then(|sketch| sketch.name(id))
                    .unwrap_or_default()
            })
            .collect(),
        OverlapItems::Model(picks) => (picks.iter())
            .map(|pick| {
                let body = document.body(pick.body).map(|body| body.name.as_str());
                model_name(pick.target, body)
            })
            .collect(),
    };
    let rows = names.into_iter().enumerate().map(|(index, name)| {
        let tick = checkbox(checked.get(index).copied().unwrap_or(false))
            .size(15)
            .style(theme::tick)
            .on_toggle(move |_| Message::Look(Look::ToggleOverlap(index)));
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
/// 1", by its body's name if that's known.
fn model_name(target: Picked, body: Option<&str>) -> String {
    let kind = match target {
        Picked::Face(_) => "Face",
        Picked::Edge(_) => "Edge",
        Picked::Vertex(_) => "Vertex",
    };
    match body {
        Some(body) => format!("{kind} of {body}"),
        None => kind.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_goes_beside_the_cursor_or_back_inside_the_viewport() {
        let items = OverlapItems::Sketch(vec![Id::ORIGIN, Id::X_AXIS]);
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
            model_name(Picked::Edge(3), Some("Body 2")),
            "Edge of Body 2"
        );
        assert_eq!(model_name(Picked::Vertex(0), None), "Vertex");
    }
}
