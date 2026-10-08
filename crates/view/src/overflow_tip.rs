//! A widget showing a tooltip only while what it wraps is cut short: a
//! list row's name too long for its room tells it whole while hovered.

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Vector};

/// `content`, cut to its room, with `tip` (`content` already wrapped in a
/// tooltip) shown only while `natural`, `content` laid out with all the
/// width it wants, is wider than that room.
pub(crate) struct OverflowTip<'a, Message> {
    tip: Element<'a, Message>,
    natural: Element<'a, Message>,
}

impl<'a, Message> OverflowTip<'a, Message> {
    pub(crate) fn new(
        tip: impl Into<Element<'a, Message>>,
        natural: impl Into<Element<'a, Message>>,
    ) -> Self {
        Self {
            tip: tip.into(),
            natural: natural.into(),
        }
    }
}

/// Whether the content was cut at the last layout.
#[derive(Default)]
struct Cut(bool);

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for OverflowTip<'_, Message> {
    fn size(&self) -> Size<Length> {
        self.tip.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.tip.as_widget().size_hint()
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Cut>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Cut::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.tip), Tree::new(&self.natural)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.tip, &self.natural]);
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let node = self
            .tip
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        let unbounded =
            layout::Limits::new(Size::ZERO, Size::new(f32::INFINITY, limits.max().height));
        let natural =
            self.natural
                .as_widget_mut()
                .layout(&mut tree.children[1], renderer, &unbounded);
        // Half a pixel spares a name that just fits rounding.
        tree.state.downcast_mut::<Cut>().0 = natural.size().width > node.size().width + 0.5;
        node
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.tip
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.tip.as_widget_mut().update(
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

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.tip.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.tip.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        if !tree.state.downcast_ref::<Cut>().0 {
            return None;
        }
        self.tip.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message: 'a> From<OverflowTip<'a, Message>> for Element<'a, Message> {
    fn from(widget: OverflowTip<'a, Message>) -> Self {
        Self::new(widget)
    }
}
