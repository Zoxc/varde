//! The floating panel an operation is set up in, over the right of the
//! viewport: a header with its title and a summary, a body with its
//! options, and a footer with its message and Cancel and OK. The header
//! and footer always show; the body scrolls when the panel would run past
//! the room it has, so OK and Cancel stay on screen however many options
//! there are or however short the window is.
//!
//! The extrude is its only user yet (`extrude::panel`); the other
//! operations are to set themselves up in it too.

use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::text::Wrapping;
use iced::widget::{column, container, opaque, row, scrollable, space, text};
use iced::{Alignment, Element, Event, Length, Rectangle, Size, Vector};

use crate::Message;
use crate::chrome::{hrule, small_button};
use crate::theme::{self, Emphasis, SEMIBOLD};

/// How wide the panel is, in pixels.
pub(crate) const PANEL_WIDTH: f32 = 264.0;

/// How far below the viewport's top the panel starts, clear of the
/// camera controls and the view cube, in pixels.
pub(crate) const PANEL_TOP: f32 = 150.0;

/// How far from the viewport's right and bottom the panel stays at least,
/// in pixels.
pub(crate) const PANEL_MARGIN: f32 = 12.0;

/// The scrollable holding the panel's body.
pub const PANEL_BODY: iced::widget::Id = iced::widget::Id::new("operation-panel-body");

/// How tall the footer's message gets at most, in pixels: about five
/// lines, clipped past that, so a long one can't push OK off the panel.
const MESSAGE_HEIGHT: f32 = 80.0;

/// The horizontal padding of the panel's sections, in pixels. The body's
/// scrollbar floats in its right padding, clear of the options.
const SIDE: f32 = 10.0;

/// What an operation's panel shows.
pub(crate) struct Parts<'a> {
    /// The operation's name, or the feature edited: one line, clipped.
    pub title: &'a str,
    /// A short note right of the title, such as what's picked.
    pub summary: Option<Element<'a, Message>>,
    /// The options, scrolled when they don't fit.
    pub body: Element<'a, Message>,
    /// Above the buttons: why OK can't be pressed, say.
    pub message: Option<Element<'a, Message>>,
    /// What OK sends, or nothing while it can't be pressed.
    pub ok: Option<Message>,
    /// What Cancel sends.
    pub cancel: Message,
}

/// The panel showing `parts`. It's `opaque`: clicks and the wheel over it
/// don't reach the scene under it.
pub(crate) fn operation_panel(parts: Parts<'_>) -> Element<'_, Message> {
    let Parts {
        title,
        summary,
        body,
        message,
        ok,
        cancel,
    } = parts;
    let title = container(text(title).size(13).font(SEMIBOLD).wrapping(Wrapping::None))
        .width(Length::Fill)
        .clip(true);
    let header = column![
        container(row![title, summary].spacing(8).align_y(Alignment::Center)).padding([9.0, SIDE]),
        hrule(),
    ];
    let scrollbar = Scrollbar::new()
        .width(4)
        .scroller_width(4)
        .margin((SIDE - 4.0) / 2.0);
    let body = scrollable(
        container(body)
            .width(Length::Fill)
            .padding(iced::Padding::from([8.0, SIDE]).bottom(10.0)),
    )
    .id(PANEL_BODY)
    .direction(Direction::Vertical(scrollbar))
    .width(Length::Fill);
    let message = message.map(|message| {
        container(message)
            .width(Length::Fill)
            .max_height(MESSAGE_HEIGHT)
            .clip(true)
    });
    let buttons = row![
        space::horizontal(),
        small_button("Cancel", Emphasis::Secondary).on_press(cancel),
        small_button("OK", Emphasis::Primary).on_press_maybe(ok),
    ]
    .spacing(6);
    let footer = column![
        hrule(),
        container(column![message, buttons].spacing(6))
            .padding(iced::Padding::from([8.0, SIDE]).bottom(10.0)),
    ];
    let sections = Sections {
        width: PANEL_WIDTH,
        parts: [header.into(), body.into(), footer.into()],
    };
    opaque(container(sections).style(theme::float_panel).clip(true))
}

/// The text of a message in a panel's footer, in `style`, broken within
/// words where they don't fit, as a message may quote a name.
pub(crate) fn message_text<'a>(
    message: impl text::IntoFragment<'a>,
    style: fn(&iced::Theme) -> text::Style,
) -> Element<'a, Message> {
    text(message)
        .size(12)
        .wrapping(Wrapping::WordOrGlyph)
        .style(style)
        .into()
}

/// A header, a body and a footer, one above the other, `width` wide: the
/// header and footer as tall as they are, the body in what's left of the
/// height the panel may take, at most as tall as it is.
///
/// A column can't do this: it lays its children out in order, so the
/// footer would get only what the body leaves, and a body filling the
/// rest would make the panel always as tall as it may be.
struct Sections<'a> {
    width: f32,
    parts: [Element<'a, Message>; 3],
}

impl Widget<Message, iced::Theme, iced::Renderer> for Sections<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.width), Length::Shrink)
    }

    fn children(&self) -> Vec<Tree> {
        self.parts.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.parts);
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits.width(self.width).height(Length::Shrink);
        let max = limits.max();
        let [header, body, footer] = &mut self.parts;
        let [header_tree, body_tree, footer_tree] = &mut tree.children[..] else {
            unreachable!("three parts have three trees");
        };
        let within = |height: f32| layout::Limits::new(Size::ZERO, Size::new(max.width, height));
        let header = header
            .as_widget_mut()
            .layout(header_tree, renderer, &within(max.height));
        let left = (max.height - header.size().height).max(0.0);
        let footer = footer
            .as_widget_mut()
            .layout(footer_tree, renderer, &within(left));
        let left = (left - footer.size().height).max(0.0);
        let body = body
            .as_widget_mut()
            .layout(body_tree, renderer, &within(left));
        let body_top = header.size().height;
        let footer_top = body_top + body.size().height;
        let height = footer_top + footer.size().height;
        let size = limits.resolve(
            Length::Fixed(self.width),
            Length::Shrink,
            Size::new(self.width, height),
        );
        layout::Node::with_children(
            size,
            vec![
                header,
                body.move_to((0.0, body_top)),
                footer.move_to((0.0, footer_top)),
            ],
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            for ((part, tree), layout) in self
                .parts
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
            {
                part.as_widget_mut()
                    .operate(tree, layout, renderer, operation);
            }
        });
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
        for ((part, tree), layout) in self
            .parts
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            part.as_widget_mut().update(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
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
        self.parts
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((part, tree), layout)| {
                part.as_widget()
                    .mouse_interaction(tree, layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default()
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
        for ((part, tree), layout) in self.parts.iter().zip(&tree.children).zip(layout.children()) {
            part.as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        overlay::from_children(
            &mut self.parts,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a> From<Sections<'a>> for Element<'a, Message> {
    fn from(sections: Sections<'a>) -> Self {
        Element::new(sections)
    }
}

#[cfg(test)]
mod tests;
