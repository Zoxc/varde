use iced::advanced::clipboard;
use iced::widget::{Space, button};
use varde_document::OriginPlane;

use super::*;
use crate::projection::top_camera;

/// Through [`top_camera`], 20 sketch units across the layer's height, the
/// origin in its middle, a unit 10 pixels.
const SIZE: Size = Size::new(300.0, 200.0);

type Layer<'a> = Anchors<'a, u8, iced::Theme, ()>;

/// A widget `size` pixels square.
fn square(size: f32) -> Element<'static, u8, iced::Theme, ()> {
    Space::new().width(size).height(size).into()
}

/// Lays `layer` out over [`SIZE`], and returns its tree and layout.
fn lay_out(layer: &mut Layer<'_>) -> (Tree, layout::Node) {
    let mut tree = Tree::new(&*layer as &dyn Widget<u8, iced::Theme, ()>);
    let node = layer.layout(&mut tree, &(), &layout::Limits::new(Size::ZERO, SIZE));
    (tree, node)
}

#[test]
fn widgets_are_centred_where_their_anchors_show() {
    let anchored = [
        (DVec2::new(0.0, 0.0), square(10.0)),
        (DVec2::new(-5.0, 3.0), square(4.0)),
        // Off the layer: past its right edge, and far below.
        (DVec2::new(20.0, 0.0), square(10.0)),
        (DVec2::new(0.0, -1e5), square(10.0)),
    ];
    let mut layer = Layer::new(top_camera(), OriginPlane::XY.placement(), anchored);
    let (_, node) = lay_out(&mut layer);
    assert_eq!(node.size(), SIZE);
    let bounds: Vec<_> = node.children().iter().map(layout::Node::bounds).collect();
    let near = |bounds: Rectangle, x: f32, y: f32, size: f32| {
        let corner = Point::new(bounds.x, bounds.y);
        corner.distance(Point::new(x, y)) < 1e-3 && bounds.size() == Size::new(size, size)
    };
    assert!(near(bounds[0], 145.0, 95.0, 10.0), "{:?}", bounds[0]);
    assert!(near(bounds[1], 98.0, 68.0, 4.0), "{:?}", bounds[1]);
    // Only those inside are laid out.
    assert_eq!(layer.shown, [true, true, false, false]);
    assert_eq!(bounds[2], Rectangle::default());
}

#[test]
fn a_point_is_placed_only_if_it_shows_inside() {
    let projector = Projector::new(&top_camera(), OriginPlane::XY.placement(), 300.0, 200.0);
    let projector = projector.unwrap();
    let place = |x: f64, y: f64| place(&projector, SIZE, DVec2::new(x, y));
    let near = |placed: Option<Point>, x: f32, y: f32| {
        placed.is_some_and(|p| p.distance(Point::new(x, y)) < 1e-3)
    };
    assert!(near(place(0.0, 0.0), 150.0, 100.0));
    assert!(near(place(-14.99, 9.99), 0.1, 0.1));
    assert!(near(place(14.99, -9.99), 299.9, 199.9));
    assert_eq!(place(-15.1, 0.0), None);
    assert_eq!(place(0.0, -10.1), None);
}

#[test]
fn the_layer_takes_only_what_is_over_its_widgets() {
    let anchored = [(
        DVec2::ZERO,
        button(Space::new().width(20.0).height(20.0))
            .on_press(1)
            .into(),
    )];
    let mut layer: Layer<'_> = Anchors::new(top_camera(), OriginPlane::XY.placement(), anchored);
    let (mut tree, node) = lay_out(&mut layer);
    let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    let viewport = Rectangle::with_size(SIZE);
    let mut taken = |at: Point| {
        let cursor = mouse::Cursor::Available(at);
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        layer.update(
            &mut tree,
            &press,
            Layout::new(&node),
            cursor,
            &(),
            &mut clipboard::Null,
            &mut shell,
            &viewport,
        );
        let captured = shell.is_event_captured();
        let interaction =
            layer.mouse_interaction(&tree, Layout::new(&node), cursor, &viewport, &());
        (captured, interaction)
    };
    // On the button at the middle, and off it.
    assert_eq!(
        taken(Point::new(150.0, 100.0)),
        (true, mouse::Interaction::Pointer)
    );
    assert_eq!(
        taken(Point::new(20.0, 30.0)),
        (false, mouse::Interaction::None)
    );
}

#[test]
fn nudged_widgets_sit_beside_their_anchors_and_apart() {
    let anchored = [
        (DVec2::ZERO, square(10.0)),
        // At the same place: nudged right past the first.
        (DVec2::ZERO, square(10.0)),
        (DVec2::ZERO, square(10.0)),
        // Far enough away to stay where it is.
        (DVec2::new(-5.0, 3.0), square(10.0)),
    ];
    let layer = Layer::new(top_camera(), OriginPlane::XY.placement(), anchored);
    let mut layer = layer.nudged(Vector::new(8.0, -8.0));
    let (_, node) = lay_out(&mut layer);
    let corners: Vec<_> = node
        .children()
        .iter()
        .map(|child| child.bounds().position())
        .collect();
    let near = |at: Point, x: f32, y: f32| at.distance(Point::new(x, y)) < 1e-3;
    assert!(near(corners[0], 153.0, 87.0), "{:?}", corners[0]);
    assert!(near(corners[1], 165.0, 87.0), "{:?}", corners[1]);
    assert!(near(corners[2], 177.0, 87.0), "{:?}", corners[2]);
    assert!(near(corners[3], 103.0, 57.0), "{:?}", corners[3]);
}
