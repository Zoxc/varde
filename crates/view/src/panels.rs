//! The side panel: the Timeline and Objects tabs, and in a sketch the
//! Sketch tab in place of the Timeline.

use iced::widget::{button, column, container, mouse_area, responsive, row, space, stack, text};
use iced::{Alignment, Element, Font, Length, Padding};
use varde_document::{FeatureId, OriginPlane};

use crate::chrome::{self, ChipSize, Edge, edged, key_chip};
use crate::icons::{self, Icon};
use crate::rollback::{GHOST_HEIGHT, MARKER_HEIGHT};
use crate::shortcut::Held;
use crate::theme::{self, SEMIBOLD, SIDE_PANEL_WIDTH, TAB_HEIGHT, TAB_LINE, TabLook};
use crate::{DocumentState, Look, Message};
use objects::{ObjectsView, objects};
use sketch_tab::sketch_tab;
use timeline::{TimelineView, timeline};

pub(crate) const ROW_HEIGHT: f32 = 28.0;

mod body_look;
mod objects;
mod rows;
mod sketch_tab;
mod timeline;

pub(crate) use rows::row_menu;
pub(crate) use sketch_tab::dimension_value_field;

/// The docked panel left of the viewport. Shows the selected tab, or the
/// other one while peeking with the peek key held. Bodies and features can
/// only be hidden, shown and removed if the document is editable. A click
/// on what's empty of it, the tab strip right of the tabs or below a
/// list, clears the selection, as `Space` does.
pub fn side_panel<'a>(state: &DocumentState<'a>) -> Element<'a, Message> {
    let selected = state.panel;
    let peek = state.peek;
    let sketching = state.sketch.is_some();
    let shown = if peek {
        selected.other(sketching)
    } else {
        selected
    };

    let tab = |panel: Panel, icon: Icon| {
        let look = if panel != shown {
            TabLook::Flat
        } else if peek {
            TabLook::Peek
        } else {
            TabLook::Raised
        };
        let alt = (panel != selected && !peek).then(|| key_chip(Held::PEEK, ChipSize::Small));
        let label = row![
            icons::tinted(icon, icons::INLINE, move |p| look.content(p)),
            text(panel.label()).font(if look == TabLook::Flat {
                Font::DEFAULT
            } else {
                SEMIBOLD
            }),
            alt,
        ]
        .spacing(6)
        .height(Length::Fill)
        .align_y(Alignment::Center);
        // The face sits `TAB_LINE` down; as much padding below it keeps the
        // label in the tab's middle.
        let face = container(label)
            .height(Length::Fill)
            .padding(Padding::from([0, 12]).bottom(TAB_LINE))
            .style(theme::tab_face(look));
        button(face)
            .height(TAB_HEIGHT)
            .padding(Padding::ZERO.top(TAB_LINE))
            .style(theme::tab(look))
            .on_press(Message::Look(Look::SelectPanel(panel)))
    };

    let features = if sketching {
        tab(Panel::Sketch, Icon::Sketch)
    } else {
        tab(Panel::Timeline, Icon::Rollback)
    };
    // The tabs hang 1 px over the strip's bottom border, so the raised tab
    // joins the panel below.
    let strip = mouse_area(stack![
        edged(
            container(space::vertical())
                .width(Length::Fill)
                .style(theme::tab_strip),
            Edge::Bottom,
            8.0 + TAB_HEIGHT,
        ),
        row![features, tab(Panel::Objects, Icon::Body)]
            .spacing(2)
            .padding(Padding::from(8).bottom(0)),
    ])
    .on_press(CLEAR_SELECTION);

    let document = state.editor.document();
    let editable = state.editable();
    let content = match (shown, state.sketch) {
        (Panel::Sketch, Some(sketch)) => {
            // The sketch edited stays selected in the Timeline.
            let change = (state.selected_feature)
                .filter(|_| editable)
                .map(|id| Message::Look(Look::ChangePlane(id)));
            sketch_tab(document, sketch, change)
        }
        (Panel::Objects, _) => scrolled(objects(ObjectsView::of(state))),
        _ => {
            let rollback = state.rollback;
            let view = TimelineView::of(state);
            let list = move || timeline(view);
            // Rolled back short of the end, and not while the marker is
            // dragged (even over where it was), the ghost marker after the list rolls to the end,
            // sticking to the panel's foot while the list scrolls. Always
            // in the same widgets, so a drag survives it showing.
            let rolled = !state.rolling
                && rollback.before.is_some_and(|until| {
                    (document.features().iter()).any(|feature| feature.id == until)
                });
            let ghost = rolled && editable && !rollback.editing;
            let rows = document.features().len() as f32 * ROW_HEIGHT + MARKER_HEIGHT;
            responsive(move |size| {
                let room = if ghost {
                    size.height - GHOST_HEIGHT
                } else {
                    size.height
                };
                let overflows = rows + LIST_PADDING.y() > room;
                let list = container(scroller(list()).id(TIMELINE_LIST))
                    .height(Length::Shrink)
                    .max_height(room.max(0.0));
                let ghost = ghost.then(|| crate::rollback::ghost(overflows));
                column![list, ghost].into()
            })
            .into()
        }
    };

    // The rows, buttons and fields take their own presses: the rest
    // reach this.
    let content = mouse_area(content).on_press(CLEAR_SELECTION);

    // On the web, under the file cell, where the design is kept.
    let location = state.file.location.map(crate::toolbar::location_bar);
    edged(
        container(column![location, strip, content])
            .height(Length::Fill)
            .style(theme::side_panel),
        Edge::Right,
        SIDE_PANEL_WIDTH,
    )
}

/// The Timeline's scrolled list: the MCP panel's Show in Timeline
/// scrolls it to the feature.
pub const TIMELINE_LIST: iced::widget::Id = iced::widget::Id::new("timeline-list");

/// What a click on an empty part of the panel or the toolbar sends.
pub(crate) const CLEAR_SELECTION: Message = Message::Look(Look::ClearSelection);

/// The padding round a list in the panel.
const LIST_PADDING: Padding = Padding {
    top: 6.0,
    right: 8.0,
    bottom: 6.0,
    left: 8.0,
};

/// `list` scrolled within the rest of the panel.
fn scrolled<'a>(list: Element<'a, Message>) -> Element<'a, Message> {
    scroller(list).height(Length::Fill).into()
}

/// `list` scrolled, as tall as it is.
fn scroller<'a>(list: Element<'a, Message>) -> iced::widget::Scrollable<'a, Message> {
    // The scroller floats in the right padding.
    chrome::scrolled(container(list).padding(LIST_PADDING), 2.0)
}

/// A group of the Sketch tab's Geometry list, which can be folded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GeometryGroup {
    /// The sketch's own points and curves.
    Own,
    /// Its sketch face: the outline of the face it's on, projected.
    SketchFace,
    /// Its links projecting outside geometry.
    Projected,
    /// Its links intersecting outside geometry with its plane.
    Intersected,
}

/// A group of the Objects tab, which can be folded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObjectGroup {
    Origin,
    Bodies,
    Sketches,
}

/// A row of Objects that's selected in the list itself, not in the
/// model: an origin object or a sketch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectRow {
    Origin(OriginObject),
    Sketch(FeatureId),
}

/// One of the world's origin objects, listed in Objects' Origin group,
/// which can be shown and hidden but not deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginObject {
    /// The origin's marker.
    Point,
    Axis(varde_document::Axis3),
    Plane(OriginPlane),
}

impl OriginObject {
    /// Each, in the order Objects lists them.
    pub const ALL: [OriginObject; 7] = [
        OriginObject::Point,
        OriginObject::Axis(varde_document::Axis3::X),
        OriginObject::Axis(varde_document::Axis3::Y),
        OriginObject::Axis(varde_document::Axis3::Z),
        OriginObject::Plane(OriginPlane::XY),
        OriginObject::Plane(OriginPlane::XZ),
        OriginObject::Plane(OriginPlane::YZ),
    ];

    /// It as the renderer has it.
    pub fn part(self) -> varde_render::OriginPart {
        match self {
            OriginObject::Point => varde_render::OriginPart::Marker,
            OriginObject::Axis(axis) => varde_render::OriginPart::Axis(axis as usize),
            OriginObject::Plane(plane) => varde_render::OriginPart::Plane(plane as usize),
        }
    }

    /// Its entry in `origin`, whether it's shown.
    pub fn shown(self, origin: &mut varde_render::OriginShown) -> &mut bool {
        match self {
            OriginObject::Point => &mut origin.marker,
            OriginObject::Axis(axis) => &mut origin.axes[axis as usize],
            OriginObject::Plane(plane) => &mut origin.planes[plane as usize],
        }
    }
}

impl GeometryGroup {
    /// The group of the links of `kind`.
    pub fn of_links(kind: varde_sketch::LinkKind) -> Self {
        match kind {
            varde_sketch::LinkKind::Project => GeometryGroup::Projected,
            varde_sketch::LinkKind::Intersect => GeometryGroup::Intersected,
        }
    }
}

/// A tab of the side panel. Two show at a time: Timeline and Objects, or
/// in a sketch Sketch and Objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Panel {
    #[default]
    Timeline,
    /// The sketch being edited: its geometry and constraints.
    Sketch,
    Objects,
}

impl Panel {
    /// The tab listing the features, or in a sketch what it holds, which
    /// shows with Objects.
    fn features(sketching: bool) -> Self {
        if sketching {
            Panel::Sketch
        } else {
            Panel::Timeline
        }
    }

    /// The other of the two tabs showing, given whether a sketch is being
    /// edited.
    pub fn other(self, sketching: bool) -> Self {
        match self {
            Panel::Timeline | Panel::Sketch => Panel::Objects,
            Panel::Objects => Panel::features(sketching),
        }
    }

    /// The tab to show on entering (`sketching`) or leaving a sketch:
    /// Sketch and Timeline trade places, and Objects stays.
    pub fn for_sketching(self, sketching: bool) -> Self {
        match self {
            Panel::Timeline | Panel::Sketch => Panel::features(sketching),
            Panel::Objects => Panel::Objects,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Panel::Timeline => "Timeline",
            Panel::Sketch => "Sketch",
            Panel::Objects => "Objects",
        }
    }
}

#[cfg(test)]
mod tests;
