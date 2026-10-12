//! A widget passing only the mouse and touch to what it wraps: the body's
//! Opacity slider is changed by dragging alone, which ends in a release
//! that commits it, never by the keys or the wheel, which would change it
//! with no release to follow.

use iced::advanced::widget::Tree;
use iced::advanced::{Clipboard, Layout, Shell, Widget, mouse, renderer};
use iced::{Element, Event, Rectangle};

/// `content`, given every event but the keys (so it sees no modifiers
/// held either) and the wheel.
pub(crate) struct MouseOnly<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
}

impl<'a, Message, Theme, Renderer> MouseOnly<'a, Message, Theme, Renderer> {
    pub(crate) fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
        }
    }
}

/// Whether `content` is given `event`.
fn passed(event: &Event) -> bool {
    match event {
        Event::Mouse(event) => !matches!(event, mouse::Event::WheelScrolled { .. }),
        Event::Keyboard(_) => false,
        Event::Touch(_) | Event::Window(_) | Event::InputMethod(_) => true,
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for MouseOnly<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    crate::wrap::pass_to!(
        content, Theme, Renderer;
        size, size_hint, children, diff, layout, operate, mouse_interaction, draw
    );

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if !passed(event) {
            return;
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }
}

impl<'a, Message, Theme, Renderer> From<MouseOnly<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(widget: MouseOnly<'a, Message, Theme, Renderer>) -> Self {
        Self::new(widget)
    }
}
