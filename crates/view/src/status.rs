//! The status bar, floating over a screen's bottom right: the selection in
//! a box of its own with the key that clears it, then a bar with what's
//! going on, the hints for the keys and the mouse, and on the document
//! screen the button of the view options menu, which opens above it.

use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::widget::{Button, container, mouse_area, opaque, row};
use iced::{Alignment, Element, Event, Length, Padding, Rectangle, Size, Vector};
use varde_render::{Projection, Shading};

use crate::chrome::{Hint, icon_button, key_hint};
use crate::icons::Icon;
use crate::shortcut::Shortcut;
use crate::theme::{self, Tone};
use crate::toolbar::{
    MENU_ITEM_HEIGHT, choice_item, menu_item, menu_separator, submenu_item, ticked,
};
use crate::{Edges, Look, Message, ViewOptions, ViewSubmenu};

/// How tall the status bar is, its border included, in pixels.
pub const STATUS_BAR_HEIGHT: f32 = 30.0;

/// How far the status bar floats in from the screen's right and bottom,
/// in pixels.
const RIGHT: f32 = 12.0;
const BOTTOM: f32 = 10.0;

/// How much of a screen's height, from its bottom, the status bar takes
/// with the margin under it: what stays clear of it.
pub const STATUS_BAR_ROOM: f32 = STATUS_BAR_HEIGHT + BOTTOM;

/// The gap between the selection's box and the bar, and between the
/// groups in each.
const GAP: f32 = 8.0;
const SPACING: f32 = 12.0;
/// The padding at a box's ends, less beside a button, which has its own.
const PADDING: f32 = 10.0;
const BUTTON_PADDING: f32 = 2.0;

/// What the status bar shows.
pub struct Status<'a> {
    /// What's selected, in a box of its own with how to clear it.
    pub selection: Option<Element<'a, Message>>,
    /// What's going on, on one line, cut short where it doesn't fit.
    pub info: Option<Element<'a, Message>>,
    pub hints: Vec<Hint<'a>>,
    /// Whether the hints of the mouse show: the step the tool or
    /// operation open asks a click for always, the others only while
    /// there's no selection nor what's going on to show, for which they
    /// make room.
    pub mouse_hints: bool,
    /// On the document screen, whether the view options menu is open: its
    /// button ends the bar.
    pub view_menu: Option<bool>,
}

/// The status bar's layer: `status` floating at the bottom right of the
/// screen, or the viewport, it's put over. The layer takes only what's
/// over the bar and lets the rest through; the bar takes what's over it,
/// so a drag on it doesn't orbit the camera.
///
/// The hints and the menu's button show whole: where the bar doesn't fit,
/// what's going on is cut short first, then the selection. The mouse's
/// hints, but for the step a click takes in the tool or operation open
/// ([`crate::chrome::step_hint`]), show only with neither.
pub fn status_bar(status: Status<'_>) -> Element<'_, Message> {
    let Status {
        selection,
        info,
        hints,
        mouse_hints,
        view_menu,
    } = status;

    let room = selection.is_none() && info.is_none();
    let hints: Vec<_> = hints
        .into_iter()
        .filter(|hint| !hint.mouse || (mouse_hints && (room || hint.step)))
        .map(|hint| hint.element)
        .collect();
    let hints = (!hints.is_empty()).then(|| {
        Part::new(
            row(hints).spacing(14).align_y(Alignment::Center),
            KEPT,
            true,
        )
    });
    let menu = view_menu.map(|open| {
        let button = icon_button(
            Icon::More,
            Tone::Muted,
            Some(Message::Look(Look::ToggleViewMenu)),
        )
        .style(theme::flat_button(open, theme::Tone::Text));
        Part::new(button, KEPT, true)
    });
    let menu_only = info.is_none() && hints.is_none();
    let info = info.map(|info| Part::new(clipped(info), STATUS, false));
    let bar = Boxed {
        left: if menu_only { BUTTON_PADDING } else { PADDING },
        right: if view_menu.is_some() {
            BUTTON_PADDING
        } else {
            PADDING
        },
        parts: [info, hints, menu].into_iter().flatten().collect(),
    };
    let selection = selection.map(|selection| Boxed {
        left: PADDING,
        right: PADDING,
        parts: vec![
            Part::new(clipped(selection), SELECTION, false),
            Part::new(key_hint(Shortcut::SPACE, "Clear").element, KEPT, true),
        ],
    });
    let boxes: Vec<_> = selection
        .into_iter()
        .chain((!bar.parts.is_empty()).then_some(bar))
        .collect();

    container(opaque(Bar::new(boxes)))
        .align_right(Length::Fill)
        .align_bottom(Length::Fill)
        .padding(Padding::ZERO.left(RIGHT).right(RIGHT).bottom(BOTTOM))
        .into()
}

/// How soon a part of the bar gives way where it doesn't fit: the lowest
/// first.
const STATUS: u8 = 0;
const SELECTION: u8 = 1;
const KEPT: u8 = 2;

/// `content` cut where it doesn't fit, on one line.
fn clipped<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    container(content).clip(true).into()
}

/// The view options menu, open above the status bar's button at the
/// screen's bottom right: the Shading and Edges submenus, each item
/// showing the icon of the choice made and the `submenu` open to its left,
/// lined up with it; the projection, `projection` ticked; and the toggles
/// of the `options`. Hovering a submenu's item opens it, and another item
/// closes it. A press anywhere off the menus closes them.
pub fn view_menu<'a>(
    projection: Projection,
    options: ViewOptions,
    submenu: Option<ViewSubmenu>,
) -> Element<'a, Message> {
    let open = |which: Option<ViewSubmenu>| Message::Look(Look::ViewSubmenu(which));
    // Hovering an item closes the submenu open, if one is.
    let closing = |item: Button<'static, Message>| -> Element<'a, Message> {
        match submenu {
            Some(_) => mouse_area(item).on_enter(open(None)).into(),
            None => item.into(),
        }
    };
    let opening = |which, icon, label| -> Element<'a, Message> {
        let item = submenu_item(icon, label, submenu == Some(which), open(Some(which)));
        mouse_area(item).on_enter(open(Some(which))).into()
    };
    let projection_item = |label, choice| {
        closing(menu_item(
            ticked(projection == choice),
            label,
            None,
            Some(Message::Look(Look::SetProjection(choice))),
        ))
    };
    let toggle = |on, label: &'static str, message| {
        closing(menu_item(ticked(on), label.into(), None, Some(message)))
    };
    let menu = container(
        iced::widget::column![
            opening(
                ViewSubmenu::Shading,
                shading_icon(options.shading),
                "Shading"
            ),
            opening(ViewSubmenu::Edges, edges_icon(options.edges), "Edges"),
            menu_separator(),
            projection_item("Orthographic".into(), Projection::Orthographic),
            projection_item("Perspective".into(), Projection::Perspective),
            menu_separator(),
            toggle(
                options.mouse_hints,
                "Mouse hints",
                Message::ToggleMouseHints
            ),
            toggle(
                options.hidden_edges,
                "Hidden edges",
                Message::ToggleHiddenEdges
            ),
        ]
        .width(180),
    )
    .padding(4)
    .style(theme::menu);

    // The submenu's first item beside the item opening it: the items are
    // `MENU_ITEM_HEIGHT` tall, from the top of both menus' padding.
    let submenu = submenu.map(|which| {
        let (items, at) = match which {
            ViewSubmenu::Shading => (shading_choices(options.shading), 0),
            ViewSubmenu::Edges => (edges_choices(options.edges), 1),
        };
        let list = container(iced::widget::column(items).width(180))
            .padding(4)
            .style(theme::menu);
        container(opaque(list)).padding(Padding::ZERO.top(at as f32 * MENU_ITEM_HEIGHT))
    });

    mouse_area(
        container(
            row![submenu, opaque(menu)]
                .spacing(2)
                .align_y(Alignment::Start),
        )
        .align_right(Length::Fill)
        .align_bottom(Length::Fill)
        .padding(Padding::ZERO.right(RIGHT).bottom(STATUS_BAR_ROOM + 4.0)),
    )
    .interaction(mouse::Interaction::Idle)
    .on_press(Message::Look(Look::CloseViewMenu))
    .on_right_press(Message::Look(Look::CloseViewMenu))
    .into()
}

/// The shadings, in the Shading submenu, `chosen` ticked.
fn shading_choices<'a>(chosen: Shading) -> Vec<Element<'a, Message>> {
    [
        (Shading::Regular, "Shaded"),
        (Shading::Flat, "Flat shaded"),
        (Shading::Metal, "Metal"),
        (Shading::FlatMetal, "Flat metal"),
    ]
    .into_iter()
    .map(|(shading, label)| {
        let message = Message::SetShading(shading);
        choice_item(shading_icon(shading), label, shading == chosen, message).into()
    })
    .collect()
}

/// The edges drawn, in the Edges submenu, `chosen` ticked.
fn edges_choices<'a>(chosen: Edges) -> Vec<Element<'a, Message>> {
    [
        (Edges::Default, "Feature edges"),
        (Edges::Wireframe, "Wireframe"),
        (Edges::Tessellation, "Tessellation"),
    ]
    .into_iter()
    .map(|(edges, label)| {
        let message = Message::SetEdges(edges);
        choice_item(edges_icon(edges), label, edges == chosen, message).into()
    })
    .collect()
}

fn shading_icon(shading: Shading) -> Icon {
    match shading {
        Shading::Regular => Icon::ShadeRegular,
        Shading::Flat => Icon::ShadeFlat,
        Shading::Metal => Icon::ShadeMetal,
        Shading::FlatMetal => Icon::ShadeFlatMetal,
    }
}

fn edges_icon(edges: Edges) -> Icon {
    match edges {
        Edges::Default => Icon::EdgesDefault,
        Edges::Wireframe => Icon::EdgesWireframe,
        Edges::Tessellation => Icon::EdgesTessellation,
    }
}

/// A part of one of the bar's boxes.
struct Part<'a> {
    element: Element<'a, Message>,
    /// How soon it gives way: see [`STATUS`].
    rank: u8,
    /// Whether a line divides it from the part before it, if one shows.
    divided: bool,
}

impl<'a> Part<'a> {
    fn new(element: impl Into<Element<'a, Message>>, rank: u8, divided: bool) -> Self {
        Self {
            element: element.into(),
            rank,
            divided,
        }
    }
}

/// One of the bar's floating boxes: its parts, left to right, `left` and
/// `right` in from its ends.
struct Boxed<'a> {
    parts: Vec<Part<'a>>,
    left: f32,
    right: f32,
}

/// The status bar's boxes, [`GAP`] apart, each [`STATUS_BAR_HEIGHT`] tall
/// with its parts [`SPACING`] apart, or twice that with a line between.
/// Each part is as wide as it is, unless the boxes don't fit: then the
/// parts that give way first ([`Part::rank`]) get what the others leave,
/// down to nothing, and one squeezed to nothing isn't shown.
///
/// Rows can't do this: they lay their children out in order, so the last
/// would get only what those before it leave.
struct Bar<'a> {
    boxes: Vec<Boxed<'a>>,
    /// As last laid out, each box's bounds and where the lines between its
    /// parts are, across, from the bar's top left.
    drawn: Vec<(Rectangle, Vec<f32>)>,
}

/// How tall the line between two parts is, in pixels.
const DIVIDER: f32 = 16.0;

impl<'a> Bar<'a> {
    fn new(boxes: Vec<Boxed<'a>>) -> Self {
        Self {
            boxes,
            drawn: Vec::new(),
        }
    }

    fn parts(&self) -> impl Iterator<Item = &Part<'a>> {
        self.boxes.iter().flat_map(|boxed| &boxed.parts)
    }

    fn parts_mut(&mut self) -> impl Iterator<Item = &mut Part<'a>> {
        self.boxes.iter_mut().flat_map(|boxed| &mut boxed.parts)
    }
}

impl<'a> From<Bar<'a>> for Element<'a, Message> {
    fn from(bar: Bar<'a>) -> Self {
        Element::new(bar)
    }
}

impl Widget<Message, iced::Theme, iced::Renderer> for Bar<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Fixed(STATUS_BAR_HEIGHT))
    }

    fn children(&self) -> Vec<Tree> {
        self.parts().map(|part| Tree::new(&part.element)).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        let elements: Vec<_> = self.parts().map(|part| &part.element).collect();
        tree.diff_children(&elements);
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let max = limits.max();
        // What isn't the parts: the boxes' ends, the gaps between them and
        // between their parts.
        let fixed: f32 = self
            .boxes
            .iter()
            .map(|boxed| {
                let between = boxed.parts.iter().skip(1).map(|part| {
                    if part.divided {
                        2.0 * SPACING + 1.0
                    } else {
                        SPACING
                    }
                });
                boxed.left + boxed.right + between.sum::<f32>()
            })
            .sum::<f32>()
            + GAP * self.boxes.len().saturating_sub(1) as f32;
        // The parts kept longest first, each given what's left.
        let mut order: Vec<_> = self.parts().map(|part| part.rank).enumerate().collect();
        order.sort_by_key(|&(_, rank)| std::cmp::Reverse(rank));
        let mut left = (max.width - fixed).max(0.0);
        let mut nodes: Vec<_> = vec![layout::Node::default(); order.len()];
        let mut parts: Vec<_> = self.parts_mut().zip(&mut tree.children).collect();
        for (at, _) in order {
            let (part, tree) = &mut parts[at];
            let within = layout::Limits::new(Size::ZERO, Size::new(left, STATUS_BAR_HEIGHT));
            let node = part.element.as_widget_mut().layout(tree, renderer, &within);
            left = (left - node.size().width).max(0.0);
            nodes[at] = node;
        }
        drop(parts);

        let mut x = 0.0;
        let mut at = 0;
        let mut placed = Vec::with_capacity(nodes.len());
        self.drawn.clear();
        for boxed in &self.boxes {
            let start = x;
            x += boxed.left;
            let mut dividers = Vec::new();
            let mut shown = false;
            for part in &boxed.parts {
                let node = std::mem::take(&mut nodes[at]);
                at += 1;
                let size = node.size();
                if size.width > 0.0 && shown {
                    x += SPACING;
                    if part.divided {
                        dividers.push(x - start);
                        x += 1.0 + SPACING;
                    }
                }
                placed.push(node.move_to((x, (STATUS_BAR_HEIGHT - size.height) / 2.0)));
                if size.width > 0.0 {
                    x += size.width;
                    shown = true;
                }
            }
            x += boxed.right;
            let bounds = Rectangle::new(
                iced::Point::new(start, 0.0),
                Size::new(x - start, STATUS_BAR_HEIGHT),
            );
            self.drawn.push((bounds, dividers));
            x += GAP;
        }
        let width = (x - GAP).max(0.0);
        let size = limits.resolve(
            Length::Shrink,
            Length::Fixed(STATUS_BAR_HEIGHT),
            Size::new(width, STATUS_BAR_HEIGHT),
        );
        layout::Node::with_children(size, placed)
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
                .parts_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
            {
                part.element
                    .as_widget_mut()
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
            .parts_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            part.element.as_widget_mut().update(
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
        self.parts()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((part, tree), layout)| {
                part.element
                    .as_widget()
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
        use iced::advanced::Renderer as _;

        let origin = layout.bounds().position();
        let panel = theme::float_panel(theme);
        let line = theme::palette(theme).line;
        for (bounds, dividers) in &self.drawn {
            let bounds = *bounds + Vector::new(origin.x, origin.y);
            container::draw_background(renderer, &panel, bounds);
            for &divider in dividers {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(
                            iced::Point::new(
                                origin.x + divider,
                                bounds.y + (STATUS_BAR_HEIGHT - DIVIDER) / 2.0,
                            ),
                            Size::new(1.0, DIVIDER),
                        ),
                        ..renderer::Quad::default()
                    },
                    line,
                );
            }
        }
        let style = renderer::Style {
            text_color: panel.text_color.unwrap_or(style.text_color),
        };
        for ((part, tree), layout) in self.parts().zip(&tree.children).zip(layout.children()) {
            // A part squeezed to nothing isn't drawn.
            if layout.bounds().width > 0.0 {
                part.element
                    .as_widget()
                    .draw(tree, renderer, theme, &style, layout, cursor, viewport);
            }
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
        let children = self
            .boxes
            .iter_mut()
            .flat_map(|boxed| &mut boxed.parts)
            .zip(&mut tree.children)
            .zip(layout.children())
            .filter_map(|((part, tree), layout)| {
                part.element
                    .as_widget_mut()
                    .overlay(tree, layout, renderer, viewport, translation)
            })
            .collect::<Vec<_>>();
        (!children.is_empty()).then(|| overlay::Group::with_children(children).overlay())
    }
}

#[cfg(test)]
mod tests;
