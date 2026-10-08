//! The Timeline's rollback marker: a diamond and a line between the rows,
//! the features after it left out of the model. Dragged up or down the
//! list, it rolls the model to where it's dropped.

use std::sync::LazyLock;

use iced::widget::canvas::{self, Action, Geometry};
use iced::widget::{column, container, hover, mouse_area, row, rule, space, stack, svg, text};
use iced::{
    Alignment, Color, Element, Event, Length, Padding, Point, Rectangle, Renderer, Theme, mouse,
};
use varde_document::FeatureId;

use crate::panels::ROW_HEIGHT;
use crate::theme::{self, SEMIBOLD};
use crate::{Edit, Look, Message};

/// The marker's height between the rows.
pub(crate) const MARKER_HEIGHT: f32 = 16.0;

/// The marker's diamond, as the mock's: a square with rounded corners,
/// turned 45°, filled with the colour its style gives.
const DIAMOND_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="-6 -6 12 12"><rect x="-3.5" y="-3.5" width="7" height="7" rx="1.5" transform="rotate(45)" fill="black"/></svg>"#;

/// The [`DIAMOND_SVG`] loaded.
static DIAMOND_HANDLE: LazyLock<svg::Handle> =
    LazyLock::new(|| svg::Handle::from_memory(DIAMOND_SVG));

/// The diamond's size on screen, its box's.
const DIAMOND: f32 = 12.0;

/// The marker, labelled `label` if there is one, as while a feature is
/// edited, and then in the Create tools' colour rather than the accent.
/// Plain widgets, not canvases, for wgpu's GL backend, which resolves
/// canvas meshes only inside the last one's scissor rect and left stale
/// lines down the list (see `notes/upstream/wgpu.md`).
pub(crate) fn marker<'a>(label: Option<&'a str>) -> Element<'a, Message> {
    let moved = label.is_some();
    let color = move |theme: &Theme| {
        let palette = theme::palette(theme);
        if moved {
            palette.icons.solid.line
        } else {
            palette.accent
        }
    };
    let diamond = svg(DIAMOND_HANDLE.clone())
        .width(DIAMOND)
        .height(DIAMOND)
        .style(move |theme: &Theme, _| svg::Style {
            color: Some(color(theme)),
        });
    let label = label.map(|label| {
        text(label)
            .size(10.5)
            .font(SEMIBOLD)
            .style(move |theme: &Theme| text::Style {
                color: Some(color(theme)),
            })
    });
    let line = container(space::horizontal())
        .width(Length::Fill)
        .height(2)
        .style(move |theme: &Theme| container::Style {
            background: Some(faded(color(theme), 0.6).into()),
            border: iced::border::rounded(1),
            ..container::Style::default()
        });
    row![diamond, label, line]
        .spacing(6)
        .padding(Padding::from([0, 8]).left(6))
        .height(MARKER_HEIGHT)
        .align_y(Alignment::Center)
        .into()
}

/// The [`ghost`]'s height, under the list.
pub(crate) const GHOST_HEIGHT: f32 = ROW_HEIGHT;

/// The ghost marker's diamond: the marker's, outlined with dashes.
const GHOST_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="-6 -6 12 12"><rect x="-3.5" y="-3.5" width="7" height="7" rx="1.5" transform="rotate(45)" fill="none" stroke="black" stroke-width="1.3" stroke-dasharray="2 1.5"/></svg>"#;

/// Under the Timeline rolled back, where the marker goes at the end: a
/// faint, outlined marker labelled "Roll to end", the marker itself when
/// hovered, that rolls the model to the end when clicked. On the panel's
/// colour with a rule above it if the list `overflows` and it sits
/// under it, at the panel's foot.
pub(crate) fn ghost<'a>(overflows: bool) -> Element<'a, Message> {
    static HANDLE: LazyLock<svg::Handle> = LazyLock::new(|| svg::Handle::from_memory(GHOST_SVG));
    let look = |solid: bool| -> Element<'a, Message> {
        let diamond = if solid {
            svg(DIAMOND_HANDLE.clone())
        } else {
            svg(HANDLE.clone())
        };
        let diamond = diamond
            .width(DIAMOND)
            .height(DIAMOND)
            .style(move |theme: &Theme, _| svg::Style {
                color: Some(accent(theme, if solid { 1.0 } else { 0.7 })),
            });
        let label = text("Roll to end")
            .size(10.5)
            .font(SEMIBOLD)
            .style(move |theme: &Theme| text::Style {
                color: Some(accent(theme, if solid { 1.0 } else { 0.8 })),
            });
        let line = container(space::horizontal())
            .width(Length::Fill)
            .height(2)
            .style(move |theme: &Theme| container::Style {
                background: Some(accent(theme, if solid { 0.6 } else { 0.25 }).into()),
                border: iced::border::rounded(1),
                ..container::Style::default()
            });
        container(
            row![diamond, label, line]
                .spacing(6)
                .padding(Padding::from([0, 16]).left(14))
                .height(GHOST_HEIGHT)
                .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(theme::palette(theme).panel.into()),
            ..container::Style::default()
        })
        .into()
    };
    let ghost = mouse_area(hover(look(false), look(true)))
        .on_press(Message::Edit(Edit::SetRollback(None)))
        .interaction(mouse::Interaction::Pointer);
    if overflows {
        column![rule::horizontal(1).style(theme::separator), ghost].into()
    } else {
        ghost.into()
    }
}

/// `list`, the Timeline's rows of `features` with the [`marker`] after
/// the first `above`, each row [`ROW_HEIGHT`] tall, with what drags the
/// marker over it: if `draggable`, a press on the marker drags it to
/// the gap between rows nearest the cursor ([`Look::DragRollback`]),
/// and the release drops it ([`Edit::DropRollback`]), wherever the
/// cursor is then. Over the whole list rather than in the marker, so it
/// stays the same widget, holding the drag, as the marker moves.
pub(crate) fn draggable<'a>(
    list: Element<'a, Message>,
    above: usize,
    features: Vec<FeatureId>,
    draggable: bool,
) -> Element<'a, Message> {
    let grip = canvas::Canvas::new(Grip {
        above,
        features,
        draggable,
    })
    .width(Length::Fill)
    .height(Length::Fill);
    stack![list, grip].into()
}

/// The accent the marker is drawn in, a share of it for the line.
fn accent(theme: &Theme, share: f32) -> Color {
    faded(theme::palette(theme).accent, share)
}

/// `color` with `share` of its opacity.
fn faded(color: Color, share: f32) -> Color {
    Color {
        a: color.a * share,
        ..color
    }
}

/// What takes the presses on the marker, to drag it.
struct Grip {
    above: usize,
    features: Vec<FeatureId>,
    draggable: bool,
}

/// While the marker is dragged, how many rows it was last dragged under.
type Dragged = Option<usize>;

impl Grip {
    /// Where the marker is in a list at `bounds`.
    fn marker(&self, bounds: Rectangle) -> Rectangle {
        Rectangle {
            y: bounds.y + ROW_HEIGHT * self.above as f32,
            height: MARKER_HEIGHT,
            ..bounds
        }
    }

    /// How many rows the marker in a list at `bounds` goes under,
    /// dragged to `y`: the gap between rows nearest it.
    fn slot(&self, bounds: Rectangle, y: f32) -> usize {
        let marker = self.marker(bounds);
        if y >= marker.y && y <= marker.y + MARKER_HEIGHT {
            return self.above;
        }
        let y = if y > marker.y { y - MARKER_HEIGHT } else { y };
        let rows = ((y - bounds.y) / ROW_HEIGHT).round();
        if rows.is_nan() || rows <= 0.0 {
            0
        } else {
            // In range: the cast saturates and `min` bounds it.
            (rows as usize).min(self.features.len())
        }
    }

    /// The feature the marker under `slot` rows is before, `None` past
    /// the last.
    fn until(&self, slot: usize) -> Option<FeatureId> {
        self.features.get(slot).copied()
    }
}

impl canvas::Program<Message> for Grip {
    type State = Dragged;

    fn update(
        &self,
        dragged: &mut Dragged,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let Event::Mouse(event) = event else {
            return None;
        };
        match *event {
            mouse::Event::ButtonPressed(mouse::Button::Left) if self.draggable => {
                cursor.position_over(self.marker(bounds))?;
                *dragged = Some(self.above);
                Some(Action::capture())
            }
            // The raw position, so the drag goes on off the marker.
            mouse::Event::CursorMoved {
                position: Point { y, .. },
            } if dragged.is_some() => {
                let slot = self.slot(bounds, y);
                if *dragged == Some(slot) {
                    return Some(Action::capture());
                }
                *dragged = Some(slot);
                let drag = Message::Look(Look::DragRollback(self.until(slot)));
                Some(Action::publish(drag).and_capture())
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) if dragged.is_some() => {
                *dragged = None;
                Some(Action::publish(Message::Edit(Edit::DropRollback)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _dragged: &Dragged,
        _renderer: &Renderer,
        _theme: &Theme,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        Vec::new()
    }

    fn mouse_interaction(
        &self,
        dragged: &Dragged,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if dragged.is_some() {
            mouse::Interaction::Grabbing
        } else if self.draggable && cursor.is_over(self.marker(bounds)) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The features of a document of `count` sketches.
    fn ids(count: usize) -> Vec<FeatureId> {
        use varde_document::{Document, Editor, OriginPlane, Plane};
        let mut editor = Editor::new(Document::default());
        for _ in 0..count {
            let sketch = (editor.document()).add_sketch(Plane::Origin(OriginPlane::XY));
            editor.apply(sketch).unwrap();
        }
        (editor.document().features().iter())
            .map(|f| f.id)
            .collect()
    }

    fn grip(above: usize, count: usize) -> Grip {
        Grip {
            above,
            features: ids(count),
            draggable: true,
        }
    }

    /// The list's bounds, at the top.
    fn bounds() -> Rectangle {
        Rectangle::new(Point::ORIGIN, iced::Size::new(200.0, 500.0))
    }

    #[test]
    fn dragged_to_the_nearest_gap() {
        let grip = grip(2, 4);
        let at = bounds();
        let marker = grip.marker(at);
        assert_eq!(marker.y, 2.0 * ROW_HEIGHT);
        // Over the marker itself: where it is.
        assert_eq!(grip.slot(at, marker.y + 8.0), 2);
        // Up past the first row's middle, and above the list.
        assert_eq!(grip.slot(at, 13.0), 0);
        assert_eq!(grip.slot(at, -50.0), 0);
        assert_eq!(grip.slot(at, 15.0), 1);
        // Below it, the marker's height left out: past the third row's
        // middle, and past the end.
        let under = marker.y + MARKER_HEIGHT;
        assert_eq!(grip.slot(at, under + 13.0), 2);
        assert_eq!(grip.slot(at, under + 15.0), 3);
        assert_eq!(grip.slot(at, under + 500.0), 4);
        assert_eq!(grip.until(4), None);
        assert_eq!(grip.until(1), Some(ids(4)[1]));
    }
}
