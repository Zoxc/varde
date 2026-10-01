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
use iced::widget::text::Wrapping;
use iced::widget::{column, container, opaque, row, space, text};
use iced::{Alignment, Element, Event, Length, Rectangle, Size, Vector};

use crate::Message;
use crate::chrome::{hrule, scrolled, small_button};
use crate::controls::CONTROLS_HEIGHT;
use crate::status::STATUS_BAR_ROOM;
use crate::theme::{self, Emphasis, SEMIBOLD};
use crate::viewport::CONTROLS_TOP;

/// How wide the panel is, in pixels.
pub(crate) const PANEL_WIDTH: f32 = 264.0;

/// How far below the viewport's top the panel starts, clear of the
/// camera controls (the view cube and Home), in pixels.
pub(crate) const PANEL_TOP: f32 = CONTROLS_TOP + CONTROLS_HEIGHT + PANEL_MARGIN;

/// How far from the viewport's right the panel stays at least, and from
/// its top where it rises over the controls, in pixels.
pub(crate) const PANEL_MARGIN: f32 = 12.0;

/// How far from the viewport's bottom the panel stays at least: clear of
/// the floating status bar, in pixels.
pub(crate) const PANEL_BOTTOM: f32 = STATUS_BAR_ROOM + PANEL_MARGIN;

/// How tall the panel may get below its top before it rises above
/// [`PANEL_TOP`], in pixels: its header and footer and a few rows of its
/// body. A shorter viewport lifts the panel over the camera controls
/// rather than squeeze its body to nothing or its buttons away.
const PANEL_ROOM: f32 = 200.0;

/// The scrollable holding the panel's body.
pub const PANEL_BODY: iced::widget::Id = iced::widget::Id::new("operation-panel-body");

/// How tall the footer's message gets at most, in pixels: about five
/// lines. A longer one scrolls, so it can't push OK off the panel and
/// none of it is lost.
const MESSAGE_HEIGHT: f32 = 80.0;

/// The horizontal padding of the panel's sections, in pixels. The
/// scrollbars of the body and the message float in its right padding,
/// clear of the text.
const SIDE: f32 = 10.0;

/// What an operation's panel shows.
pub(crate) struct Parts<'a> {
    /// The operation's name, or the feature edited: one line, clipped.
    pub title: &'a str,
    /// A short note right of the title, such as what's picked.
    pub summary: Option<Element<'a, Message>>,
    /// The options, scrolled when they don't fit.
    pub body: Element<'a, Message>,
    /// Above the buttons: why OK can't be pressed, say. It scrolls past
    /// about five lines.
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
    // Both scrollbars float in the right padding, clear of the text.
    let margin = (SIDE - theme::SCROLLBAR_WIDTH) / 2.0;
    let body = scrolled(
        container(body)
            .width(Length::Fill)
            .padding(iced::Padding::from([8.0, SIDE]).bottom(10.0)),
        margin,
    )
    .id(PANEL_BODY)
    .width(Length::Fill);
    let message = message.map(|message| {
        container(
            scrolled(
                container(message).width(Length::Fill).padding([0.0, SIDE]),
                margin,
            )
            .width(Length::Fill),
        )
        .max_height(MESSAGE_HEIGHT)
    });
    let buttons = row![
        space::horizontal(),
        small_button("Cancel", Emphasis::Secondary).on_press(cancel),
        small_button("OK", Emphasis::Primary).on_press_maybe(ok),
    ]
    .spacing(6);
    let footer = column![
        hrule(),
        column![message, container(buttons).padding([0.0, SIDE])]
            .spacing(6)
            .padding(iced::Padding::from([8.0, 0.0]).bottom(10.0)),
    ];
    let sections = Sections {
        width: PANEL_WIDTH,
        parts: [header.into(), body.into(), footer.into()],
    };
    opaque(container(sections).style(theme::operation_panel).clip(true))
}

/// `panel` placed over a viewport: at its right, [`PANEL_MARGIN`] in from
/// its right and [`PANEL_BOTTOM`] up from its bottom, and [`PANEL_TOP`]
/// down from its top, or higher, down to [`PANEL_MARGIN`], where the
/// viewport is too short to leave it [`PANEL_ROOM`] below that. The layer takes only what's over the panel
/// and lets the rest through.
pub(crate) fn placed(panel: Element<'_, Message>) -> Element<'_, Message> {
    Element::new(Placed { panel })
}

/// See [`placed`].
struct Placed<'a> {
    panel: Element<'a, Message>,
}

impl Placed<'_> {
    /// How far below the top of a viewport `height` tall the panel starts.
    fn top(height: f32) -> f32 {
        if !height.is_finite() {
            return PANEL_TOP;
        }
        (height - PANEL_BOTTOM - PANEL_ROOM).clamp(PANEL_MARGIN, PANEL_TOP)
    }
}

impl Widget<Message, iced::Theme, iced::Renderer> for Placed<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.panel)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.panel));
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = limits.resolve(Length::Fill, Length::Fill, Size::ZERO);
        let top = Self::top(size.height);
        let room = Size::new(
            (size.width - 2.0 * PANEL_MARGIN).max(0.0),
            (size.height - top - PANEL_BOTTOM).max(0.0),
        );
        let panel = self.panel.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            &layout::Limits::new(Size::ZERO, room),
        );
        let x = (size.width - PANEL_MARGIN - panel.size().width).max(0.0);
        layout::Node::with_children(size, vec![panel.move_to((x, top))])
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        let panel = layout.children().next().expect("the panel's layout");
        self.panel
            .as_widget_mut()
            .operate(&mut tree.children[0], panel, renderer, operation);
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
        let panel = layout.children().next().expect("the panel's layout");
        self.panel.as_widget_mut().update(
            &mut tree.children[0],
            event,
            panel,
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
        let panel = layout.children().next().expect("the panel's layout");
        self.panel.as_widget().mouse_interaction(
            &tree.children[0],
            panel,
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
        let panel = layout.children().next().expect("the panel's layout");
        self.panel.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            panel,
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
        let panel = layout.children().next().expect("the panel's layout");
        self.panel.as_widget_mut().overlay(
            &mut tree.children[0],
            panel,
            renderer,
            viewport,
            translation,
        )
    }
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
        // The footer first: where even the header and footer don't fit,
        // the buttons keep their height and the title gives way.
        let footer = footer
            .as_widget_mut()
            .layout(footer_tree, renderer, &within(max.height));
        let left = (max.height - footer.size().height).max(0.0);
        let header = header
            .as_widget_mut()
            .layout(header_tree, renderer, &within(left));
        let left = (left - header.size().height).max(0.0);
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
