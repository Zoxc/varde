//! Two parts of a panel, one above the other, sharing its height by a
//! divider dragged between them.

use iced::widget::canvas::{self, Action, Frame, Geometry};
use iced::widget::{column, container, responsive};
use iced::{Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, mouse};

use crate::Message;
use crate::theme;

/// The divider's height: its line and the room around it to grab it by.
const DIVIDER: f32 = 7.0;

/// The least share of the height either part keeps.
const MIN_SHARE: f32 = 0.1;

/// `above` over `below`, `above` taking the `share` of the height the
/// divider leaves, within [`MIN_SHARE`] of either end. Dragging the
/// divider sends `on_resize` with the share it's dragged to. Each part is
/// built for its height, so a list can lay out only the rows it shows.
pub fn vertical_split<'a>(
    share: f32,
    above: impl Fn(f32) -> Element<'a, Message> + 'a,
    below: impl Fn(f32) -> Element<'a, Message> + 'a,
    on_resize: fn(f32) -> Message,
) -> Element<'a, Message> {
    responsive(move |size| {
        let room = (size.height - DIVIDER).max(0.0);
        let top = room * clamp_share(share);
        column![
            container(above(top)).height(top),
            canvas::Canvas::new(Divider {
                above: top,
                room,
                on_resize,
            })
            .width(Length::Fill)
            .height(DIVIDER),
            container(below(room - top)).height(Length::Fill),
        ]
        .into()
    })
    .into()
}

/// `share` within [`MIN_SHARE`] of either end; the middle if it isn't a
/// number.
fn clamp_share(share: f32) -> f32 {
    if share.is_nan() {
        0.5
    } else {
        share.clamp(MIN_SHARE, 1.0 - MIN_SHARE)
    }
}

/// The divider between the parts: knows how tall the part above it is and
/// how much room the two share, to turn the cursor into a share.
struct Divider {
    above: f32,
    room: f32,
    on_resize: fn(f32) -> Message,
}

impl Divider {
    /// The share of the room above the cursor at `y`, the divider centred
    /// on it, for a divider at `bounds`.
    fn share_at(&self, bounds: Rectangle, y: f32) -> Option<f32> {
        let top = bounds.y - self.above;
        (self.room >= 1.0).then(|| clamp_share((y - top - DIVIDER / 2.0) / self.room))
    }
}

impl canvas::Program<Message> for Divider {
    /// Whether it's being dragged.
    type State = bool;

    fn update(
        &self,
        dragging: &mut bool,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let Event::Mouse(event) = event else {
            return None;
        };
        match *event {
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                cursor.position_over(bounds)?;
                *dragging = true;
                Some(Action::capture())
            }
            // The raw position, so the drag goes on outside the divider.
            mouse::Event::CursorMoved {
                position: Point { y, .. },
            } if *dragging => {
                let share = self.share_at(bounds, y)?;
                Some(Action::publish((self.on_resize)(share)).and_capture())
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) if *dragging => {
                *dragging = false;
                Some(Action::capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _dragging: &bool,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill_rectangle(
            Point::new(0.0, (DIVIDER / 2.0).floor()),
            Size::new(bounds.width, 1.0),
            theme::palette(theme).line,
        );
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        dragging: &bool,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if *dragging || cursor.is_over(bounds) {
            mouse::Interaction::ResizingVertically
        } else {
            mouse::Interaction::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_share_follows_the_cursor_within_bounds() {
        let divider = Divider {
            above: 100.0,
            room: 400.0,
            on_resize: |_| Message::ToggleTheme,
        };
        // The divider sits at y 150, so the split starts at 50.
        let bounds = Rectangle::new(Point::new(0.0, 150.0), Size::new(200.0, DIVIDER));
        let at = |y| divider.share_at(bounds, y + DIVIDER / 2.0).unwrap();
        assert_eq!(at(50.0 + 200.0), 0.5);
        assert_eq!(at(50.0 + 100.0), 0.25);
        assert_eq!(at(0.0), MIN_SHARE);
        assert_eq!(at(1000.0), 1.0 - MIN_SHARE);
        assert_eq!(clamp_share(f32::NAN), 0.5);

        let squashed = Divider {
            room: 0.0,
            ..divider
        };
        assert_eq!(squashed.share_at(bounds, 10.0), None);
    }
}
