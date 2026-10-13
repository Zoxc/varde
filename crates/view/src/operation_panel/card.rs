//! The floating cards the operation panel and the parameters popup are
//! drawn as: placed over the viewport's right, and laid out in sections.

use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::widget::{button, container, text};
use iced::{Border, Color, Element, Event, Length, Rectangle, Size, Vector};

use crate::Message;
use crate::chrome::tip;
use crate::controls::CONTROLS_HEIGHT;
use crate::icons::{self, Icon};
use crate::status::STATUS_BAR_ROOM;
use crate::theme;
use crate::viewport::CONTROLS_TOP;

/// How wide the panel is, in pixels.
pub(crate) const PANEL_WIDTH: f32 = 288.0;

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
/// [`PANEL_TOP`], in pixels: its head and foot and a few rows of its
/// body. A shorter viewport lifts the panel over the camera controls
/// rather than squeeze its body to nothing or its buttons away.
pub(super) const PANEL_ROOM: f32 = 200.0;

/// The horizontal padding of the body and the foot, in pixels. The
/// scrollbars of the body and the message float in its right padding,
/// clear of the text.
pub(super) const SIDE: f32 = 9.0;

/// How thick the accent line along the card's top is, in pixels.
pub(super) const ACCENT_LINE: f32 = 3.0;

/// The corner radius of the card, and of the well's top corners.
pub(super) const CARD_RADIUS: f32 = 8.0;
pub(super) const WELL_RADIUS: f32 = 6.0;

/// A square icon button of the head, `icon` in it, with the id `id`
/// naming it, sending `message`, or disabled without one; `primary` is
/// OK's look. Hovering it tells `tip`.
pub(crate) fn head_button<'a>(
    icon: Icon,
    id: iced::widget::Id,
    primary: bool,
    message: Option<Message>,
    tip_text: &'a str,
) -> Element<'a, Message> {
    let enabled = message.is_some();
    // 16 px in the 26 px square sits on whole pixels: at 15 the centring
    // puts it half a pixel off, which blurs the cross's middle wide.
    let glyph: Element<'a, Message> = Element::from(icons::tinted(icon, 16.0, move |p| {
        match (primary, enabled) {
            (true, _) => theme::Emphasis::Primary.content(p),
            (false, true) => p.muted,
            (false, false) => p.faint,
        }
    }));
    let button = button(container(glyph).center(Length::Fill))
        .width(26)
        .height(26)
        .padding(0)
        .style(theme::head_button(primary))
        .on_press_maybe(message);
    container(tip(button, text(tip_text))).id(id).into()
}

/// `panel` placed over a viewport: at its right, [`PANEL_MARGIN`] in from
/// its right and [`PANEL_BOTTOM`] up from its bottom, and [`PANEL_TOP`]
/// down from its top, or higher, down to [`PANEL_MARGIN`], where the
/// viewport is too short to leave it [`PANEL_ROOM`] below that. The layer takes only what's over the panel
/// and lets the rest through.
pub(crate) fn placed(panel: Element<'_, Message>) -> Element<'_, Message> {
    placed_from(panel, PANEL_MARGIN)
}

/// `panel` placed as [`placed`] places one, but `right` in from the
/// viewport's right: the parameters' popup, left of an operation's
/// panel. It keeps [`PANEL_MARGIN`] from the viewport's left, and gets
/// narrower where it has to.
pub(crate) fn placed_from(panel: Element<'_, Message>, right: f32) -> Element<'_, Message> {
    Element::new(Placed { panel, right })
}

/// See [`placed`].
pub(super) struct Placed<'a> {
    panel: Element<'a, Message>,
    /// How far in from the viewport's right the panel's right edge is.
    right: f32,
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
            (size.width - self.right - PANEL_MARGIN).max(0.0),
            (size.height - top - PANEL_BOTTOM).max(0.0),
        );
        let panel = self.panel.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            &layout::Limits::new(Size::ZERO, room),
        );
        let x = (size.width - self.right - panel.size().width).max(0.0);
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

/// The card: a header, a body and a footer, one above the other, `width`
/// wide, under the accent line, the body and the footer in the recessed
/// well: the header and footer as tall as they are, the body in what's
/// left of the height the panel may take, at most as tall as it is.
///
/// A column can't do this: it lays its children out in order, so the
/// footer would get only what the body leaves, and a body filling the
/// rest would make the panel always as tall as it may be.
pub(crate) struct Sections<'a> {
    pub(crate) width: f32,
    pub(crate) parts: [Element<'a, Message>; 3],
    /// Whether the body sits in the recessed well; without it, the
    /// panel's colour goes on under the head, as the parameters' popup
    /// has it.
    pub(crate) well: bool,
}

/// How far in from the card's sides and bottom its parts are: inside its
/// 1 px border.
pub(super) const BORDER: f32 = 1.0;

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
        let inner = (max.width - 2.0 * BORDER).max(0.0);
        let within = |height: f32| layout::Limits::new(Size::ZERO, Size::new(inner, height));
        // The header first: where even the header and footer don't fit,
        // its buttons keep their height and the message gives way.
        let room = (max.height - ACCENT_LINE - BORDER).max(0.0);
        let header = header
            .as_widget_mut()
            .layout(header_tree, renderer, &within(room));
        let left = (room - header.size().height).max(0.0);
        let footer = footer
            .as_widget_mut()
            .layout(footer_tree, renderer, &within(left));
        let left = (left - footer.size().height).max(0.0);
        let body = body
            .as_widget_mut()
            .layout(body_tree, renderer, &within(left));
        let body_top = ACCENT_LINE + header.size().height;
        let footer_top = body_top + body.size().height;
        let height = footer_top + footer.size().height + BORDER;
        let size = limits.resolve(
            Length::Fixed(self.width),
            Length::Shrink,
            Size::new(self.width, height),
        );
        layout::Node::with_children(
            size,
            vec![
                header.move_to((BORDER, ACCENT_LINE)),
                body.move_to((BORDER, body_top)),
                footer.move_to((BORDER, footer_top)),
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
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;

        let p = theme::palette(theme);
        let card = layout.bounds();
        let radius = |top: f32, bottom: f32| iced::border::Radius::new(top).bottom(bottom);
        let quad = |bounds: Rectangle, radius, border: Border| renderer::Quad {
            bounds,
            border: Border { radius, ..border },
            ..renderer::Quad::default()
        };
        // Its shadow, then the accent line along its top, round at the
        // card's corners, the panel's colour over the rest of it.
        renderer.fill_quad(
            renderer::Quad {
                shadow: theme::CARD_SHADOW,
                ..quad(card, CARD_RADIUS.into(), Border::default())
            },
            Color::TRANSPARENT,
        );
        let accent = Rectangle {
            height: (ACCENT_LINE + CARD_RADIUS).min(card.height),
            ..card
        };
        renderer.fill_quad(
            quad(accent, radius(CARD_RADIUS, 0.0), Border::default()),
            p.accent,
        );
        let below = Rectangle {
            y: card.y + ACCENT_LINE,
            height: (card.height - ACCENT_LINE).max(0.0),
            ..card
        };
        renderer.fill_quad(
            quad(
                below,
                radius(CARD_RADIUS - ACCENT_LINE, CARD_RADIUS),
                Border {
                    color: p.line,
                    width: BORDER,
                    ..Border::default()
                },
            ),
            p.panel,
        );
        // The well, from the body's top to the card's border.
        if let Some(body) = layout.children().nth(1).filter(|_| self.well) {
            let top = body.bounds().y;
            let well = Rectangle {
                x: card.x + BORDER,
                y: top,
                width: (card.width - 2.0 * BORDER).max(0.0),
                height: (card.y + card.height - BORDER - top).max(0.0),
            };
            renderer.fill_quad(
                quad(
                    well,
                    radius(WELL_RADIUS, CARD_RADIUS - BORDER),
                    Border::default(),
                ),
                theme::well(p),
            );
        }
        let style = renderer::Style { text_color: p.text };
        for ((part, tree), layout) in self.parts.iter().zip(&tree.children).zip(layout.children()) {
            part.as_widget()
                .draw(tree, renderer, theme, &style, layout, cursor, viewport);
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
