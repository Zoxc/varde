//! A context menu: content that asks for its menu when it's right-clicked,
//! and shows it where it was clicked while the app has it open.

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector};

use crate::Message;

/// `content`, sending `on_open` when it's right-clicked. While `menu` is
/// given it shows over everything, where `content` was last right-clicked,
/// kept within the window; a press off it sends `on_close`.
pub(crate) struct ContextMenu<'a> {
    content: Element<'a, Message>,
    menu: Option<Element<'a, Message>>,
    on_open: Message,
    on_close: Message,
}

impl<'a> ContextMenu<'a> {
    pub(crate) fn new(
        content: impl Into<Element<'a, Message>>,
        menu: Option<Element<'a, Message>>,
        on_open: Message,
        on_close: Message,
    ) -> Self {
        Self {
            content: content.into(),
            menu,
            on_open,
            on_close,
        }
    }
}

/// Where the content was last right-clicked, in its own coordinates
/// (those of a scrolled list's content, within one).
#[derive(Default)]
struct State {
    at: Option<Point>,
}

impl Widget<Message, iced::Theme, iced::Renderer> for ContextMenu<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        std::iter::once(&self.content)
            .chain(&self.menu)
            .map(Tree::new)
            .collect()
    }

    fn diff(&self, tree: &mut Tree) {
        let elements: Vec<_> = std::iter::once(&self.content).chain(&self.menu).collect();
        tree.diff_children(&elements);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        (self.content.as_widget_mut()).layout(&mut tree.children[0], renderer, limits)
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
        if shell.is_event_captured() {
            return;
        }
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) = event
            && let Some(at) = cursor.position_over(layout.bounds())
        {
            tree.state.downcast_mut::<State>().at = Some(at);
            shell.publish(self.on_open.clone());
            shell.capture_event();
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        (self.content.as_widget_mut()).operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        // Not right-clicked since the tree was made: at its top left.
        let at = tree.state.downcast_ref::<State>().at;
        let at = at.unwrap_or(layout.position()) + translation;
        let mut children = tree.children.iter_mut();
        let content = self.content.as_widget_mut().overlay(
            children.next()?,
            layout,
            renderer,
            viewport,
            translation,
        );
        let menu = (self.menu.as_mut())
            .zip(children.next())
            .map(|(menu, tree)| {
                overlay::Element::new(Box::new(Menu {
                    at,
                    menu,
                    tree,
                    on_close: &self.on_close,
                }))
            });
        let overlays: Vec<_> = content.into_iter().chain(menu).collect();
        (!overlays.is_empty()).then(|| overlay::Group::with_children(overlays).overlay())
    }
}

impl<'a> From<ContextMenu<'a>> for Element<'a, Message> {
    fn from(menu: ContextMenu<'a>) -> Self {
        Element::new(menu)
    }
}

/// The open menu, over the whole window: its top left `at`, unless that
/// would put it partly outside.
struct Menu<'a, 'b> {
    at: Point,
    menu: &'b mut Element<'a, Message>,
    tree: &'b mut Tree,
    on_close: &'b Message,
}

impl Menu<'_, '_> {
    /// Where the menu itself is laid out within the window's `layout`.
    fn menu_layout<'l>(layout: Layout<'l>) -> Layout<'l> {
        layout.children().next().expect("the menu's layout")
    }
}

impl overlay::Overlay<Message, iced::Theme, iced::Renderer> for Menu<'_, '_> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let limits = layout::Limits::new(Size::ZERO, bounds);
        let menu = self
            .menu
            .as_widget_mut()
            .layout(self.tree, renderer, &limits);
        let size = menu.size();
        let x = self.at.x.min(bounds.width - size.width).max(0.0);
        let y = self.at.y.min(bounds.height - size.height).max(0.0);
        layout::Node::with_children(bounds, vec![menu.move_to(Point::new(x, y))])
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.menu.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            Self::menu_layout(layout),
            cursor,
            &layout.bounds(),
        );
    }

    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        let menu = Self::menu_layout(layout);
        (self.menu.as_widget_mut()).operate(self.tree, menu, renderer, operation);
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let menu = Self::menu_layout(layout);
        self.menu.as_widget_mut().update(
            self.tree,
            event,
            menu,
            cursor,
            renderer,
            clipboard,
            shell,
            &layout.bounds(),
        );
        // Nothing under the menu takes a press, and one off it closes it.
        if let Event::Mouse(mouse::Event::ButtonPressed(_)) = event {
            if !cursor.is_over(menu.bounds()) {
                shell.publish(self.on_close.clone());
            }
            shell.capture_event();
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        // Idle off the menu, so nothing under it shows hovered.
        let menu = Self::menu_layout(layout);
        let over = self.menu.as_widget().mouse_interaction(
            self.tree,
            menu,
            cursor,
            &layout.bounds(),
            renderer,
        );
        if over == mouse::Interaction::None {
            mouse::Interaction::Idle
        } else {
            over
        }
    }
}

#[cfg(test)]
mod tests;
