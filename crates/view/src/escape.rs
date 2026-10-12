//! A widget taking `Esc` before what it wraps: a focused text field
//! would take it only to let go of the focus, leaving the field open, so
//! the value field wraps itself in one to close on the first `Esc`.

use iced::advanced::widget::Tree;
use iced::advanced::{Clipboard, Layout, Shell, Widget, mouse, renderer};
use iced::keyboard::{self, key::Named};
use iced::{Element, Event, Rectangle};

/// `content`, sending `on_escape` when `Esc` is pressed, whatever has the
/// focus, and keeping the key from `content` and the app. Where a widget
/// before it took the key (another `OnEscape`, a focused field), it
/// sends nothing.
pub(crate) struct OnEscape<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    on_escape: Message,
}

impl<'a, Message, Theme, Renderer> OnEscape<'a, Message, Theme, Renderer> {
    pub(crate) fn new(
        content: impl Into<Element<'a, Message, Theme, Renderer>>,
        on_escape: Message,
    ) -> Self {
        Self {
            content: content.into(),
            on_escape,
        }
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for OnEscape<'_, Message, Theme, Renderer>
where
    Message: Clone,
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
        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(Named::Escape),
            ..
        }) = event
        {
            // A panel's every field sends its cancel: the first takes the
            // key, and the others see it taken, so it's sent once.
            if !shell.is_event_captured() {
                shell.publish(self.on_escape.clone());
                shell.capture_event();
            }
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

impl<'a, Message, Theme, Renderer> From<OnEscape<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(widget: OnEscape<'a, Message, Theme, Renderer>) -> Self {
        Self::new(widget)
    }
}
