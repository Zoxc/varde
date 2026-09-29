//! A layer of widgets over the viewport, each centred where a point of the
//! sketch being edited shows, or a little off it and nudged apart where
//! they'd overlap: where constraint glyphs, dimensions' labels and the
//! value field go. Being widgets, they take their own hover and
//! clicks; the layer itself takes nothing, so everything off its widgets
//! goes through to the scene below.

use std::collections::HashMap;

use glam::DVec2;
use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector};
use varde_document::Placement;
use varde_render::Camera;

use crate::projection::Projector;

/// The layer: widgets anchored at sketch points, seen by a camera. Only
/// those whose anchor shows inside the layer are laid out, drawn and given
/// events.
pub(crate) struct Anchors<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    camera: Camera,
    placement: Placement,
    /// Each widget's anchor, in sketch coordinates.
    anchors: Vec<DVec2>,
    children: Vec<Element<'a, Message, Theme, Renderer>>,
    /// Where each widget goes from where its anchor shows.
    place: Place,
    /// Which children the last layout placed.
    shown: Vec<bool>,
}

impl<'a, Message, Theme, Renderer> Anchors<'a, Message, Theme, Renderer> {
    /// The layer over a viewport seen by `camera` of a sketch on
    /// `placement`, with each of `anchored` at its sketch point.
    pub(crate) fn new(
        camera: Camera,
        placement: Placement,
        anchored: impl IntoIterator<Item = (DVec2, Element<'a, Message, Theme, Renderer>)>,
    ) -> Self {
        let (anchors, children): (Vec<_>, Vec<_>) = anchored.into_iter().unzip();
        Self {
            camera,
            placement,
            shown: vec![false; children.len()],
            anchors,
            children,
            place: Place::Centred,
        }
    }

    /// Centres each widget `offset` from where its anchor shows, so it's
    /// beside what it's anchored to rather than over it, and nudges one
    /// that would overlap a widget laid out before it rightwards until it
    /// doesn't.
    pub(crate) fn nudged(self, offset: Vector) -> Self {
        Self {
            place: Place::Nudged(offset),
            ..self
        }
    }

    /// Puts each widget's top left corner `offset` from where its anchor
    /// shows, beside it whatever its size.
    pub(crate) fn beside(self, offset: Vector) -> Self {
        Self {
            place: Place::Beside(offset),
            ..self
        }
    }

    /// The children the last layout placed, with their trees and layouts.
    fn placed<'b, T>(
        &'b self,
        trees: &'b [T],
        layout: Layout<'b>,
    ) -> impl Iterator<Item = (&'b Element<'a, Message, Theme, Renderer>, &'b T, Layout<'b>)> {
        self.children
            .iter()
            .zip(trees)
            .zip(layout.children())
            .zip(&self.shown)
            .filter(|&(_, &shown)| shown)
            .map(|(((child, tree), layout), _)| (child, tree, layout))
    }
}

/// Where an [`Anchors`] layer puts each widget from where its anchor
/// shows.
#[derive(Debug, Clone, Copy)]
enum Place {
    /// Centred there.
    Centred,
    /// Centred this far off, and nudged apart, see [`Anchors::nudged`].
    Nudged(Vector),
    /// Its top left corner this far off, see [`Anchors::beside`].
    Beside(Vector),
}

/// The widgets nudged apart so far, by the cells of a grid they touch, so
/// a widget is only tested against those near it.
#[derive(Default)]
struct Placed {
    cells: HashMap<(i64, i64), Vec<Rectangle>>,
}

impl Placed {
    /// The side of a cell of the grid, in pixels: about a glyph's.
    const CELL: f32 = 24.0;
    /// The space left between widgets nudged apart, in pixels.
    const GAP: f32 = 2.0;
    /// The most times a widget is nudged: past it, it overlaps. Keeps a
    /// pile of widgets at one place from costing without bound.
    const MOST: usize = 16;

    /// Where the widget wanting `bounds` goes: there, or moved right past
    /// the widgets placed before it that it would overlap. Recorded.
    fn nudge(&mut self, mut bounds: Rectangle) -> Point {
        for _ in 0..Self::MOST {
            let Some(right) = self
                .near(bounds)
                .filter(|other| other.intersects(&bounds))
                .map(|other| other.x + other.width)
                .reduce(f32::max)
            else {
                break;
            };
            bounds.x = right + Self::GAP;
        }
        for cell in Self::cells(bounds) {
            self.cells.entry(cell).or_default().push(bounds);
        }
        bounds.position()
    }

    /// The widgets placed in the cells `bounds` touches.
    fn near(&self, bounds: Rectangle) -> impl Iterator<Item = &Rectangle> {
        Self::cells(bounds)
            .filter_map(|cell| self.cells.get(&cell))
            .flatten()
    }

    /// The cells `bounds` touches. Bounds are within a layer, and widgets
    /// are small, so there are few.
    fn cells(bounds: Rectangle) -> impl Iterator<Item = (i64, i64)> {
        let cell = |at: f32| (at / Self::CELL).floor() as i64;
        let (x0, x1) = (cell(bounds.x), cell(bounds.x + bounds.width));
        let (y0, y1) = (cell(bounds.y), cell(bounds.y + bounds.height));
        (x0..=x1).flat_map(move |x| (y0..=y1).map(move |y| (x, y)))
    }
}

/// Where the widget anchored at the sketch point `at` is centred in a
/// layer of `size` seen through `projector`: where `at` shows, if that's
/// inside the layer.
pub(crate) fn place(projector: &Projector, size: Size, at: DVec2) -> Option<Point> {
    let shown = projector.project(at)?;
    let inside = (0.0..=f64::from(size.width)).contains(&shown.x)
        && (0.0..=f64::from(size.height)).contains(&shown.y);
    inside.then(|| Point::new(shown.x as f32, shown.y as f32))
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Anchors<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.children);
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = limits.max();
        let projector = Projector::new(&self.camera, self.placement, size.width, size.height);
        let loose = layout::Limits::new(Size::ZERO, size);
        let placing = self.place;
        let mut placed = Placed::default();
        let nodes = self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(&self.anchors)
            .zip(&mut self.shown)
            .map(|(((child, tree), &at), shown)| {
                let center = projector
                    .as_ref()
                    .and_then(|projector| place(projector, size, at));
                *shown = center.is_some();
                let Some(center) = center else {
                    return layout::Node::default();
                };
                let node = child.as_widget_mut().layout(tree, renderer, &loose);
                let half = node.size();
                let corner = center - Vector::new(half.width / 2.0, half.height / 2.0);
                match placing {
                    Place::Centred => node.move_to(corner),
                    Place::Nudged(offset) => {
                        let bounds = Rectangle::new(corner + offset, node.size());
                        node.move_to(placed.nudge(bounds))
                    }
                    Place::Beside(offset) => node.move_to(center + offset),
                }
            })
            .collect();
        layout::Node::with_children(size, nodes)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let children = self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
            .zip(&self.shown);
        for (((child, tree), layout), &shown) in children {
            if shown {
                child
                    .as_widget_mut()
                    .operate(tree, layout, renderer, operation);
            }
        }
    }

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
        let children = self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
            .zip(&self.shown);
        for (((child, tree), layout), &shown) in children {
            if !shown {
                continue;
            }
            child.as_widget_mut().update(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
            if shell.is_event_captured() {
                return;
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.placed(&tree.children, layout)
            .map(|(child, tree, layout)| {
                child
                    .as_widget()
                    .mouse_interaction(tree, layout, cursor, viewport, renderer)
            })
            .find(|&interaction| interaction != mouse::Interaction::None)
            .unwrap_or_default()
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        for (child, tree, layout) in self.placed(&tree.children, layout) {
            child
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
    }
}

impl<'a, Message, Theme, Renderer> From<Anchors<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(anchors: Anchors<'a, Message, Theme, Renderer>) -> Self {
        Self::new(anchors)
    }
}

#[cfg(test)]
mod tests;
