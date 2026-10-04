//! Revolves in the history: new bodies, the turning direction on every
//! plane and axis, joins, cuts and intersects, regions found again,
//! the axis lost, and drafts answered from the cache.

use glam::DVec3;
use varde_document::{AxisLine, Revolve, Turn};
use varde_sketch::Id;

use super::*;
use crate::Draft;
use crate::tests::{answered, regenerate_with};

/// An angle of `text` (bare numbers in degrees) in `document`.
fn angle(document: &Document, text: &str) -> Value {
    Value::new(text, &Turn::ask(&document.design())).unwrap()
}

/// A revolve of all the regions of `sketch` about `axis`.
fn revolve_of(
    editor: &Editor,
    sketch: FeatureId,
    axis: AxisLine,
    extent: Turn,
    operation: Operation,
) -> Revolve {
    let FeatureKind::Sketch { sketch: drawn, .. } =
        &editor.document().feature(sketch).unwrap().kind
    else {
        panic!("{sketch:?} is a sketch");
    };
    let profiles = drawn.profiles().unwrap();
    let regions = (0..profiles.regions.len())
        .map(|index| profiles.reference(index).unwrap())
        .collect();
    Revolve {
        sketch,
        regions,
        axis,
        extent,
        flip: false,
        operation,
    }
}

/// Adds a sketch on `plane` drawn by `draw`, which gives the axis, and a
/// revolve of all its regions about it, `flip`ped or not. The revolve's
/// id.
fn add_revolve(
    editor: &mut Editor,
    plane: OriginPlane,
    draw: impl FnOnce(&mut Sketch) -> AxisLine,
    extent: Turn,
    flip: bool,
    operation: Operation,
) -> FeatureId {
    editor
        .apply(editor.document().add_sketch(Plane::Origin(plane)))
        .unwrap();
    let feature = editor.document().features().last().unwrap().id;
    let mut sketch = Sketch::default();
    let axis = draw(&mut sketch);
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    let revolve = Revolve {
        flip,
        ..revolve_of(editor, feature, axis, extent, operation)
    };
    editor
        .apply(editor.document().add_feature(revolve.into()))
        .unwrap();
    editor.document().features().last().unwrap().id
}

/// Draws the rectangle from `min` to `max`, and gives `axis`.
fn rectangle_about(
    min: (f64, f64),
    max: (f64, f64),
    axis: AxisLine,
) -> impl FnOnce(&mut Sketch) -> AxisLine {
    move |sketch| {
        rectangle(min, max)(sketch);
        axis
    }
}

/// Draws the rectangle from `min` to `max` and a construction line from
/// `start` to `end`, the axis.
fn rectangle_and_line(
    min: (f64, f64),
    max: (f64, f64),
    start: (f64, f64),
    end: (f64, f64),
) -> impl FnOnce(&mut Sketch) -> AxisLine {
    move |sketch| {
        rectangle(min, max)(sketch);
        let [start, end] = [start, end].map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
        AxisLine::Curve(sketch.add_curve(Curve::Line { start, end }, true).unwrap())
    }
}

/// The volume of the ring from radius `inner` to `outer`, `height` tall,
/// turned `turn` of a whole turn.
fn ring(inner: f64, outer: f64, height: f64, turn: f64) -> f64 {
    PI * (outer * outer - inner * inner) * height * turn
}

/// The volumes agree within the rounding of exact faces.
fn assert_close(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-9 * b.abs().max(1.0), "{a} vs {b}");
}

#[test]
fn a_full_turn_makes_a_new_body() {
    let mut editor = Editor::new(Document::default());
    add_revolve(
        &mut editor,
        OriginPlane::XY,
        rectangle_about((5.0, 0.0), (10.0, 4.0), AxisLine::SketchY),
        Turn::Full,
        false,
        Operation::NewBody(BodyId::NEW),
    );
    let evaluation = evaluated(editor.document());
    let solid = only_body(&evaluation);
    assert_eq!(evaluation.bodies[0].body, editor.document().bodies()[0].id);
    assert_close(solid.volume(), ring(5.0, 10.0, 4.0, 1.0));
    let bounds = solid.bounds3().unwrap();
    assert_eq!(bounds.min, glam::DVec3::new(-10.0, 0.0, -10.0));
    assert_eq!(bounds.max, glam::DVec3::new(10.0, 4.0, 10.0));
}

/// A quarter turn of a ring 5 to 10 from its axis and 4 tall turns
/// right-handed about the axis's direction from the sketch plane, on
/// every plane, on either side of the axis, about lines either way, and
/// back when flipped.
#[test]
fn part_turns_go_right_handed_about_the_axis() {
    type Draw = Box<dyn FnOnce(&mut Sketch) -> AxisLine>;
    type Case = (OriginPlane, Draw, bool, [f64; 3], [f64; 3]);
    // The plane, the drawing, whether flipped, and the world box the
    // quarter must lie in (lower and upper corners, from the turn's
    // direction).
    let cases: Vec<Case> = vec![
        // About +y: +x turns toward −z.
        (
            OriginPlane::XY,
            Box::new(rectangle_about((5.0, 0.0), (10.0, 4.0), AxisLine::SketchY)),
            false,
            [0.0, 0.0, -10.0],
            [10.0, 4.0, 0.0],
        ),
        // Flipped, toward +z.
        (
            OriginPlane::XY,
            Box::new(rectangle_about((5.0, 0.0), (10.0, 4.0), AxisLine::SketchY)),
            true,
            [0.0, 0.0, 0.0],
            [10.0, 4.0, 10.0],
        ),
        // The other side of the axis: −x turns toward +z.
        (
            OriginPlane::XY,
            Box::new(rectangle_about(
                (-10.0, 0.0),
                (-5.0, 4.0),
                AxisLine::SketchY,
            )),
            false,
            [-10.0, 0.0, 0.0],
            [0.0, 4.0, 10.0],
        ),
        // About a line pointing along −y: +x turns toward +z.
        (
            OriginPlane::XY,
            Box::new(rectangle_and_line(
                (5.0, 0.0),
                (10.0, 4.0),
                (0.0, 7.0),
                (0.0, -3.0),
            )),
            false,
            [0.0, 0.0, 0.0],
            [10.0, 4.0, 10.0],
        ),
        // About the sketch's x on XY: +y turns toward +z.
        (
            OriginPlane::XY,
            Box::new(rectangle_about((0.0, 5.0), (4.0, 10.0), AxisLine::SketchX)),
            false,
            [0.0, 0.0, 0.0],
            [4.0, 10.0, 10.0],
        ),
        // On XZ (normal −y), about its x, the world's +x: the world's +z
        // turns toward −y.
        (
            OriginPlane::XZ,
            Box::new(rectangle_about((0.0, 5.0), (4.0, 10.0), AxisLine::SketchX)),
            false,
            [0.0, -10.0, 0.0],
            [4.0, 0.0, 10.0],
        ),
        // On YZ, about its y, the world's +z: the world's +y turns
        // toward −x.
        (
            OriginPlane::YZ,
            Box::new(rectangle_about((5.0, 0.0), (10.0, 4.0), AxisLine::SketchY)),
            false,
            [-10.0, 0.0, 0.0],
            [0.0, 10.0, 4.0],
        ),
    ];
    for (k, (plane, draw, flip, min, max)) in cases.into_iter().enumerate() {
        let mut editor = Editor::new(Document::default());
        let quarter = Turn::OneSide(angle(editor.document(), "90"));
        add_revolve(
            &mut editor,
            plane,
            draw,
            quarter,
            flip,
            Operation::NewBody(BodyId::NEW),
        );
        let evaluation = evaluated(editor.document());
        let solid = only_body(&evaluation);
        assert_close(solid.volume(), ring(5.0, 10.0, 4.0, 0.25));
        let bounds = solid.bounds3().unwrap();
        for axis in 0..3 {
            // Within rounding of where the quarter turn ends.
            let near = |a: f64, b: f64| (a - b).abs() <= 1e-9;
            assert!(near(bounds.min[axis], min[axis]), "case {k}: {bounds:?}");
            assert!(near(bounds.max[axis], max[axis]), "case {k}: {bounds:?}");
        }
    }
}

/// `at` turned by `angle` (a quarter turn either way) right-handed about
/// the line through `point` along the unit `along`, by Rodrigues'
/// formula: worked out on its own, not by the frame regen builds.
fn turned(at: DVec3, point: DVec3, along: DVec3, angle: f64) -> DVec3 {
    let v = at - point;
    let parallel = along * along.dot(v);
    point + parallel + (v - parallel) * angle.cos() + along.cross(v) * angle.sin()
}

/// A quarter turn of a square off a slanted line, on every plane, on
/// either side of the line, about the line either way and flipped or
/// not, has the square's corners where they started and where Rodrigues'
/// formula turns them, and none where turning the other way would.
#[test]
fn a_quarter_turn_about_a_slanted_line_lands_where_rodrigues_says() {
    let (a, b) = ((1.0, 0.0), (3.0, 1.0));
    // Left of the line from a to b, and right of it.
    let squares = [((0.0, 4.0), (2.0, 6.0)), ((4.0, -4.0), (6.0, -2.0))];
    for plane in OriginPlane::ALL {
        let placement = plane.placement();
        let world = |(x, y): (f64, f64)| placement.to_world(DVec2::new(x, y));
        for (min, max) in squares {
            for (start, end) in [(a, b), (b, a)] {
                for flip in [false, true] {
                    let mut editor = Editor::new(Document::default());
                    let quarter = Turn::OneSide(angle(editor.document(), "90"));
                    add_revolve(
                        &mut editor,
                        plane,
                        rectangle_and_line(min, max, start, end),
                        quarter,
                        flip,
                        Operation::NewBody(BodyId::NEW),
                    );
                    let evaluation = evaluated(editor.document());
                    let solid = only_body(&evaluation);
                    let case = format!("{plane:?} {min:?} {start:?} flip {flip}");
                    // Pappus: the square's area times the length of the
                    // quarter circle its middle runs along.
                    let middle = DVec2::new((min.0 + max.0) / 2.0, (min.1 + max.1) / 2.0);
                    let (s, e) = (DVec2::new(start.0, start.1), DVec2::new(end.0, end.1));
                    let radius = (middle - s).perp_dot(e - s).abs() / (e - s).length();
                    assert_close(solid.volume(), 4.0 * radius * PI / 2.0);
                    let point = world(start);
                    let along = (world(end) - point).normalize();
                    let angle = if flip { -PI / 2.0 } else { PI / 2.0 };
                    let corners = [min, (max.0, min.1), max, (min.0, max.1)].map(world);
                    let near = |p: DVec3| {
                        let verts = solid.mesh().verts();
                        verts.iter().any(|v| v.distance(p) <= 1e-9)
                    };
                    for corner in corners {
                        assert!(near(corner), "{case}: {corner} at the start");
                        let to = turned(corner, point, along, angle);
                        assert!(near(to), "{case}: {corner} turned to {to}");
                        let away = turned(corner, point, along, -angle);
                        assert!(!near(away), "{case}: {corner} turned the other way");
                    }
                }
            }
        }
    }
}

/// An edge of the region as the axis: its segment makes no face, and the
/// region turns about the line's direction.
#[test]
fn an_edge_of_the_region_can_be_the_axis() {
    let mut editor = Editor::new(Document::default());
    let quarter = Turn::OneSide(angle(editor.document(), "90"));
    add_revolve(
        &mut editor,
        OriginPlane::XY,
        |sketch| {
            rectangle((0.0, 0.0), (5.0, 4.0))(sketch);
            // The rectangle's left edge, from (0, 4) down to (0, 0).
            let left = sketch.curves[3].id;
            AxisLine::Curve(left)
        },
        quarter,
        false,
        Operation::NewBody(BodyId::NEW),
    );
    let evaluation = evaluated(editor.document());
    let solid = only_body(&evaluation);
    assert_close(solid.volume(), PI * 25.0 * 4.0 / 4.0);
    // About −y, +x turns toward +z.
    let bounds = solid.bounds3().unwrap();
    assert!(bounds.min.z.abs() <= 1e-9 && (bounds.max.z - 5.0).abs() <= 1e-9);
    assert!(bounds.min.x.abs() <= 1e-9 && (bounds.max.x - 5.0).abs() <= 1e-9);
}

#[test]
fn symmetric_and_two_sided_turns_take_their_angles() {
    for (texts, flip, turn, [low, high]) in [
        (["90", ""], false, 0.25, [-1.0, 1.0]),
        (["90", "45"], false, 3.0 / 8.0, [-1.0, 1.0]),
        (["90", "45"], true, 3.0 / 8.0, [-1.0, 1.0]),
        (["270", ""], false, 0.75, [-9.0, 9.0]),
    ] {
        let mut editor = Editor::new(Document::default());
        let extent = match texts {
            [a, ""] => Turn::Symmetric(angle(editor.document(), a)),
            [a, b] => Turn::TwoSides(angle(editor.document(), a), angle(editor.document(), b)),
        };
        add_revolve(
            &mut editor,
            OriginPlane::XY,
            rectangle_about((5.0, 0.0), (10.0, 4.0), AxisLine::SketchY),
            extent,
            flip,
            Operation::NewBody(BodyId::NEW),
        );
        let evaluation = evaluated(editor.document());
        let solid = only_body(&evaluation);
        assert_close(solid.volume(), ring(5.0, 10.0, 4.0, turn));
        // Both sides of the sketch plane.
        let bounds = solid.bounds3().unwrap();
        assert!(bounds.min.z < low && bounds.max.z > high, "{bounds:?}");
    }
    // Two sides: 90° about +y from +x is toward −z, 45° back toward +z.
    let mut editor = Editor::new(Document::default());
    let extent = Turn::TwoSides(
        angle(editor.document(), "90"),
        angle(editor.document(), "45"),
    );
    add_revolve(
        &mut editor,
        OriginPlane::XY,
        rectangle_about((5.0, 0.0), (10.0, 4.0), AxisLine::SketchY),
        extent,
        false,
        Operation::NewBody(BodyId::NEW),
    );
    let bounds = only_body(&evaluated(editor.document())).bounds3().unwrap();
    assert!((bounds.min.z + 10.0).abs() <= 1e-9, "{bounds:?}");
    assert!(
        (bounds.max.z - 10.0 * (PI / 4.0).sin()).abs() <= 1e-9,
        "{bounds:?}"
    );
}

/// Two sides typed in degrees adding up to 360 make the whole turn,
/// though in radians they come a rounding over it (0.5 and 359.5, which
/// the document took for over a turn) or under it (1.1 and 358.9, which
/// the kernel refused as a part turn so nearly full its ends touch).
#[test]
fn two_sides_adding_up_to_a_turn_make_the_whole_turn() {
    let whole = |extent: Turn| {
        let mut editor = Editor::new(Document::default());
        add_revolve(
            &mut editor,
            OriginPlane::XY,
            rectangle_about((5.0, 0.0), (10.0, 4.0), AxisLine::SketchY),
            extent,
            false,
            Operation::NewBody(BodyId::NEW),
        );
        let evaluation = evaluated(editor.document());
        assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
        only_body(&evaluation).volume()
    };
    let full = whole(Turn::Full);
    let document = Document::default();
    for [a, b] in [["0.5", "359.5"], ["1.1", "358.9"]] {
        let extent = Turn::TwoSides(angle(&document, a), angle(&document, b));
        assert_eq!(whole(extent).to_bits(), full.to_bits(), "{a} + {b}");
    }
}

/// A plate 40 × 40 and 5 thick on XY, from z = 0. Its body.
fn add_plate(editor: &mut Editor) -> BodyId {
    let extent = Extent::OneSide(length(editor.document(), "5"));
    add_extrude(
        editor,
        rectangle((-20.0, -20.0), (20.0, 20.0)),
        extent,
        Operation::NewBody(BodyId::NEW),
    );
    editor.document().bodies().last().unwrap().id
}

/// A full turn about the world's z of the rectangle from `(0, z0)` to
/// `(radius, z1)` on XZ, with `operation`. Its id.
fn add_turned(
    editor: &mut Editor,
    radius: f64,
    (z0, z1): (f64, f64),
    operation: Operation,
) -> FeatureId {
    add_revolve(
        editor,
        OriginPlane::XZ,
        rectangle_about((0.0, z0), (radius, z1), AxisLine::SketchY),
        Turn::Full,
        false,
        operation,
    )
}

#[test]
fn revolves_join_cut_and_intersect_as_extrudes_do() {
    let plate = 40.0 * 40.0 * 5.0;
    let cases = [
        // A boss on the plate.
        (
            Operation::Join(Targets::default()),
            5.0,
            (5.0, 10.0),
            plate + PI * 25.0 * 5.0,
        ),
        // A blind hole 3 deep.
        (
            Operation::Cut(Targets::default()),
            5.0,
            (2.0, 10.0),
            plate - PI * 25.0 * 3.0,
        ),
        // A disc holding the whole plate's width, 3 of its thickness.
        (
            Operation::Intersect(Targets::default()),
            30.0,
            (-1.0, 3.0),
            40.0 * 40.0 * 3.0,
        ),
    ];
    for (operation, radius, span, volume) in cases {
        let mut editor = Editor::new(Document::default());
        let body = add_plate(&mut editor);
        let revolve = add_turned(&mut editor, radius, span, operation);
        let evaluation = evaluated(editor.document());
        let solid = only_body(&evaluation);
        assert_close(solid.volume(), volume);
        assert_eq!(evaluation.touched, [(revolve, vec![body])]);
    }
}

#[test]
fn a_revolve_touching_nothing_fails_and_one_emptying_a_body_fails() {
    let mut editor = Editor::new(Document::default());
    add_plate(&mut editor);
    let away = add_turned(
        &mut editor,
        5.0,
        (8.0, 10.0),
        Operation::Join(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(
        evaluation.failed,
        [(away, "it doesn't touch any body".to_owned())]
    );
    assert_close(evaluation.bodies[0].solid.volume(), 8000.0);

    let mut editor = Editor::new(Document::default());
    add_plate(&mut editor);
    let all = add_turned(
        &mut editor,
        40.0,
        (-1.0, 6.0),
        Operation::Cut(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    let [
        crate::FeatureFailure {
            feature: failed,
            message: error,
            ..
        },
    ] = &evaluation.failed[..]
    else {
        panic!("{:?}", evaluation.failed);
    };
    assert_eq!(*failed, all);
    assert!(error.contains("would leave nothing of it"), "{error}");
    assert_close(evaluation.bodies[0].solid.volume(), 8000.0);
}

/// A revolve's join touching two bodies merges them, as an extrude's.
#[test]
fn a_revolve_joining_two_bodies_merges_them() {
    let mut editor = Editor::new(Document::default());
    let extent = Extent::OneSide(length(editor.document(), "5"));
    let new = || Operation::NewBody(BodyId::NEW);
    add_extrude(
        &mut editor,
        rectangle((-20.0, -5.0), (-10.0, 5.0)),
        extent.clone(),
        new(),
    );
    add_extrude(
        &mut editor,
        rectangle((10.0, -5.0), (20.0, 5.0)),
        extent,
        new(),
    );
    let [first, second] = [0, 1].map(|k| editor.document().bodies()[k].id);
    // A ring from 8 to 15 round the z axis, 2 tall, through both.
    let join = add_revolve(
        &mut editor,
        OriginPlane::XZ,
        rectangle_about((8.0, 0.0), (15.0, 2.0), AxisLine::SketchY),
        Turn::Full,
        false,
        Operation::Join(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    let solid = only_body(&evaluation);
    assert_eq!(evaluation.bodies[0].body, first);
    assert_eq!(evaluation.merged, [(second, first)]);
    assert_eq!(evaluation.touched, [(join, vec![first, second])]);
    // The blocks, and the ring less what's in them: inside a block the
    // ring runs from x = 10 to its outer rim, between y = ±5.
    let blocks = 2.0 * 10.0 * 10.0 * 5.0;
    let ring = ring(8.0, 15.0, 2.0, 1.0);
    // In each block, the ring's part is where x ≥ 10, |y| ≤ 5 and
    // x² + y² ≤ 15²: |y| ≤ 5 up to x = √200, the circle after.
    let knee = 200.0_f64.sqrt();
    let under = |x: f64| (x * (225.0 - x * x).max(0.0).sqrt() + 225.0 * (x / 15.0).asin()) / 2.0;
    let area = 2.0 * (5.0 * (knee - 10.0) + under(15.0) - under(knee));
    let overlap = 2.0 * area * 2.0;
    let volume = blocks + ring - overlap;
    assert!(
        (solid.volume() - volume).abs() <= 1e-9 * volume,
        "{} vs {volume}",
        solid.volume()
    );
}

/// The sketch `sketch` changed by `change`, as one edit.
fn edit(editor: &mut Editor, sketch: FeatureId, change: impl FnOnce(&mut Sketch)) {
    let FeatureKind::Sketch { sketch: drawn, .. } =
        &editor.document().feature(sketch).unwrap().kind
    else {
        panic!("{sketch:?} is a sketch");
    };
    let mut drawn = drawn.clone();
    change(&mut drawn);
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn),
        })
        .unwrap();
}

/// The id of the point of `sketch` at `at`.
fn point_at(sketch: &Sketch, at: (f64, f64)) -> Id {
    sketch
        .points
        .iter()
        .find(|point| point.at == DVec2::new(at.0, at.1))
        .unwrap()
        .id
}

#[test]
fn the_region_is_found_again_after_its_sketch_is_edited() {
    let mut editor = Editor::new(Document::default());
    add_revolve(
        &mut editor,
        OriginPlane::XY,
        rectangle_and_line((5.0, 0.0), (10.0, 4.0), (0.0, 0.0), (0.0, 1.0)),
        Turn::Full,
        false,
        Operation::NewBody(BodyId::NEW),
    );
    let sketch = editor.document().features()[0].id;
    let mut cache = Cache::default();
    let volume = |editor: &Editor, cache: &mut Cache| {
        let evaluation = evaluate(editor.document(), cache);
        only_body(&evaluation).volume()
    };
    assert_close(volume(&editor, &mut cache), ring(5.0, 10.0, 4.0, 1.0));
    edit(&mut editor, sketch, |drawn| {
        // A line elsewhere, the outer side moved out to 12 and the axis
        // line moved along itself.
        let [a, b] =
            [(30.0, 30.0), (40.0, 30.0)].map(|(x, y)| drawn.add_point(DVec2::new(x, y)).unwrap());
        drawn
            .add_curve(Curve::Line { start: a, end: b }, false)
            .unwrap();
        for corner in [(10.0, 0.0), (10.0, 4.0)] {
            let id = point_at(drawn, corner);
            drawn
                .points
                .iter_mut()
                .find(|point| point.id == id)
                .unwrap()
                .at
                .x = 12.0;
        }
        let start = point_at(drawn, (0.0, 0.0));
        drawn
            .points
            .iter_mut()
            .find(|point| point.id == start)
            .unwrap()
            .at
            .y = -3.0;
    });
    assert_close(volume(&editor, &mut cache), ring(5.0, 12.0, 4.0, 1.0));
}

#[test]
fn a_deleted_axis_line_is_not_found() {
    let mut editor = Editor::new(Document::default());
    let revolve = add_revolve(
        &mut editor,
        OriginPlane::XY,
        rectangle_and_line((5.0, 0.0), (10.0, 4.0), (0.0, 0.0), (0.0, 1.0)),
        Turn::Full,
        false,
        Operation::NewBody(BodyId::NEW),
    );
    let sketch = editor.document().features()[0].id;
    let FeatureKind::Revolve(made) = &editor.document().feature(revolve).unwrap().kind else {
        panic!("a revolve");
    };
    let AxisLine::Curve(line) = made.axis else {
        panic!("about a line");
    };
    edit(&mut editor, sketch, |drawn| drawn.delete(&[line]));
    let evaluation = evaluated(editor.document());
    assert_eq!(evaluation.failed, [(revolve, "axis not found".to_owned())]);
    assert!(evaluation.bodies.is_empty());
    // A line that's gone is nowhere to show.
    assert!(evaluation.failed[0].geometry.is_none());
}

/// An axis line whose ends are at one place fails, showing that place
/// and marking the line in its sketch.
#[test]
fn an_axis_line_of_no_length_shows_where_it_is() {
    let mut editor = Editor::new(Document::default());
    let revolve = add_revolve(
        &mut editor,
        OriginPlane::XZ,
        rectangle_and_line((5.0, 0.0), (10.0, 4.0), (0.0, 2.0), (0.0, 2.0)),
        Turn::Full,
        false,
        Operation::NewBody(BodyId::NEW),
    );
    let FeatureKind::Revolve(made) = &editor.document().feature(revolve).unwrap().kind else {
        panic!("a revolve");
    };
    let AxisLine::Curve(line) = made.axis else {
        panic!("about a line");
    };
    let crate::Response::Regenerated { failed, .. } = crate::handle(regenerate_with(&editor, None))
    else {
        panic!("regeneration failed");
    };
    let [failure] = &failed[..] else {
        panic!("{failed:?}");
    };
    assert_eq!(
        (failure.feature, failure.message.as_str()),
        (revolve, "its axis line has no length")
    );
    let geometry = failure.geometry.as_ref().expect("the line's place");
    let at = OriginPlane::XZ.placement().to_world(DVec2::new(0.0, 2.0));
    assert_eq!(geometry.points(), [at.as_vec3().to_array()]);
    assert_eq!(geometry.sketch_curves(), [u64::from(line.get())]);
    assert_eq!(geometry.mesh().triangle_count(), 0);
    assert!(geometry.lines().points().is_empty() && geometry.faces().is_empty());
}

#[test]
fn an_outline_across_the_axis_fails() {
    // The example's plate, centred on the origin, about the sketch's y.
    let mut editor = Editor::new(Document::example());
    let extrude = example_extrude(editor.document());
    let revolve = Revolve {
        sketch: extrude.sketch,
        regions: extrude.regions.clone(),
        axis: AxisLine::SketchY,
        extent: Turn::Full,
        flip: false,
        operation: Operation::NewBody(BodyId::NEW),
    };
    editor
        .apply(editor.document().add_feature(revolve.into()))
        .unwrap();
    let revolve = editor.document().features().last().unwrap().id;
    let below = plate_below(&mut editor);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        evaluation.failed,
        [(revolve, "its outline crosses the axis".to_owned())]
    );
    // The bodies before and after it are made.
    let bodies = editor.document().bodies();
    let made: Vec<BodyId> = evaluation.bodies.iter().map(|made| made.body).collect();
    assert_eq!(made, [bodies[0].id, below]);
    assert_near(evaluation.bodies[0].solid.volume(), plate(8.0, 10.0));
}

/// A cut being set up as a draft lists the plate it touches, is answered
/// from the cache when asked again, and dragging its angle works out
/// only its tool, touch, cut and mesh again.
#[test]
fn a_revolve_draft_is_answered_from_the_cache() {
    let mut editor = Editor::new(Document::default());
    let body = add_plate(&mut editor);
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XZ)))
        .unwrap();
    let sketch = editor.document().features().last().unwrap().id;
    edit(&mut editor, sketch, rectangle((0.0, 2.0), (5.0, 10.0)));
    let mut regenerator = crate::Regenerator::default();
    let committed = answered(regenerator.handle(regenerate_with(&editor, None)));
    let (_, before) = regenerator.cache().counts();

    let quarter = Turn::OneSide(angle(editor.document(), "90"));
    let cut = revolve_of(
        &editor,
        sketch,
        AxisLine::SketchY,
        quarter,
        Operation::Cut(Targets::default()),
    );
    let mut draft = Draft {
        revision: 1,
        feature: None,
        kind: cut.into(),
    };
    let first = answered(regenerator.handle(regenerate_with(&editor, Some(draft.clone()))));
    assert_eq!(
        first.draft,
        Some(crate::Drafted {
            revision: 1,
            geometry: None,
            error: None,
            touched: Some(vec![body]),
            uncut: Vec::new(),
            reference: None,
            datums: None,
            scale: None,
        })
    );
    assert_ne!(first.mesh, committed.mesh);
    // The tool, whether it touches the plate, the cut and its mesh.
    let (_, worked) = regenerator.cache().counts();
    assert_eq!(worked, before + 4);

    // Asked again: all found.
    draft.revision = 2;
    let again = answered(regenerator.handle(regenerate_with(&editor, Some(draft.clone()))));
    assert_eq!(again.draft.unwrap().error, None);
    assert!(Arc::ptr_eq(&again.mesh, &first.mesh));
    assert_eq!(regenerator.cache().counts().1, worked);

    // Turned further: only its tool, touching, cut and mesh.
    draft.revision = 3;
    let FeatureKind::Revolve(revolve) = &mut draft.kind else {
        unreachable!()
    };
    revolve.extent = Turn::OneSide(angle(editor.document(), "180"));
    let further = answered(regenerator.handle(regenerate_with(&editor, Some(draft.clone()))));
    assert_eq!(further.draft.unwrap().error, None);
    assert_eq!(regenerator.cache().counts().1, worked + 4);

    // Committed, its result is the draft's.
    editor
        .apply(editor.document().add_feature(draft.kind.clone()))
        .unwrap();
    let evaluation = evaluated(editor.document());
    assert_close(
        only_body(&evaluation).volume(),
        8000.0 - PI * 25.0 * 3.0 / 2.0,
    );
}

/// A draft editing a revolve into an extrude, and back, as `SetFeature`
/// would.
#[test]
fn a_draft_may_change_the_kind_of_its_feature() {
    let mut editor = Editor::new(Document::default());
    let revolve = add_revolve(
        &mut editor,
        OriginPlane::XY,
        rectangle_about((5.0, 0.0), (10.0, 4.0), AxisLine::SketchY),
        Turn::Full,
        false,
        Operation::NewBody(BodyId::NEW),
    );
    let FeatureKind::Revolve(made) = editor.document().feature(revolve).unwrap().kind.clone()
    else {
        unreachable!()
    };
    let extrude = Extrude {
        sketch: made.sketch,
        regions: made.regions.clone(),
        extent: Extent::OneSide(length(editor.document(), "3")),
        flip: false,
        operation: made.operation.clone(),
    };
    let draft = Draft {
        revision: 1,
        feature: Some(revolve),
        kind: extrude.clone().into(),
    };
    let answer = answered(crate::handle(regenerate_with(&editor, Some(draft))));
    assert_eq!(answer.draft.unwrap().error, None);
    let [(_, bounds)] = answer.bodies[..] else {
        panic!("one body");
    };
    assert_eq!(
        (bounds.min, bounds.max),
        (
            glam::Vec3::new(5.0, 0.0, 0.0),
            glam::Vec3::new(10.0, 4.0, 3.0)
        )
    );

    // Back: the extrude committed, a draft making it a revolve again.
    editor
        .apply(Command::SetFeature {
            feature: revolve,
            kind: Box::new(extrude.into()),
        })
        .unwrap();
    let draft = Draft {
        revision: 2,
        feature: Some(revolve),
        kind: made.into(),
    };
    let answer = answered(crate::handle(regenerate_with(&editor, Some(draft))));
    assert_eq!(answer.draft.unwrap().error, None);
    let [(_, bounds)] = answer.bodies[..] else {
        panic!("one body");
    };
    assert_eq!(
        (bounds.min, bounds.max),
        (
            glam::Vec3::new(-10.0, 0.0, -10.0),
            glam::Vec3::new(10.0, 4.0, 10.0)
        )
    );
}

/// Regions moved into the axis's frame past the coordinate limit fail
/// before the kernel is asked.
#[test]
fn regions_too_far_from_the_axis_fail() {
    let mut editor = Editor::new(Document::default());
    let revolve = add_revolve(
        &mut editor,
        OriginPlane::XY,
        rectangle_and_line(
            (900_000.0, 0.0),
            (950_000.0, 4.0),
            (-900_000.0, 0.0),
            (-900_000.0, 1.0),
        ),
        Turn::Full,
        false,
        Operation::NewBody(BodyId::NEW),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(
        evaluation.failed,
        [(
            revolve,
            "its regions are too far from the axis to revolve".to_owned()
        )]
    );
}

/// Moving a profile into the axis's frame puts the ends of the axis
/// line's own segments, and the ends of the others there, at `x = 0`
/// exactly, and moves every other point rigidly, even one on the line
/// past its end (the kernel's to put on the axis, not regen's).
#[test]
fn only_the_axis_line_s_own_points_are_put_on_the_axis() {
    let p = |x: f64, y: f64| DVec2::new(x, y);
    let (start, end) = (p(0.3, 0.1), p(2.9, 1.7));
    // On the line, past its end, to rounding.
    let past = start + (end - start) * 1.5;
    // The axis, a line of a sketch, and other curves' ids.
    let mut sketch = Sketch::default();
    let [a, b] = [start, end].map(|at| sketch.add_point(at).unwrap());
    let line = (sketch.add_curve(Curve::Line { start: a, end: b }, true)).unwrap();
    let id = u64::from(line.get());
    let corners = [
        (start, id),
        (end, id + 1),
        (past, id + 2),
        (p(2.0, 5.0), id + 3),
    ];
    let segments = (0..corners.len())
        .map(|k| {
            let (a, curve) = corners[k];
            let b = corners[(k + 1) % corners.len()].0;
            Segment {
                conic: Conic2::line(a, b).unwrap(),
                curve,
            }
        })
        .collect();
    let profile = Profile {
        loops: vec![Loop { segments }],
    };
    let axis = Axis {
        at: start,
        along: end - start,
        curve: Some(line),
    };
    let placement = OriginPlane::XY.placement();
    let (moved, frame, _) = axis_frame(&profile, &axis, &placement).unwrap();
    let (x, y) = (frame.x.truncate(), frame.y.truncate());
    let map = |q: DVec2| p((q - start).dot(x), (q - start).dot(y));
    let moved = &moved.loops[0].segments;
    for (k, segment) in moved.iter().enumerate() {
        let (from, to) = (corners[k].0, corners[(k + 1) % corners.len()].0);
        for (got, was) in [(segment.conic.p0, from), (segment.conic.p1, to)] {
            if was == start || was == end {
                assert_eq!(got, p(0.0, map(was).y), "{was}");
            } else {
                assert_eq!(got, map(was), "{was}");
            }
        }
    }
    // The axis segment's control point too; the next one's is moved.
    assert_eq!(moved[0].conic.c.x, 0.0);
    assert_eq!(moved[1].conic.c, map((end + past) * 0.5));
}

/// A slanted edge of a triangle as the axis, whose ends a float doesn't
/// put on the line through them exactly: a whole turn makes the two
/// cones, as Pappus says, not a refusal for touching or crossing the
/// axis.
#[test]
fn a_slanted_edge_of_the_region_can_be_the_axis() {
    let corners = [(0.3, 0.1), (2.9, 1.7), (1.0, 3.0)];
    let mut editor = Editor::new(Document::default());
    add_revolve(
        &mut editor,
        OriginPlane::XZ,
        |sketch| {
            let ids = corners.map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
            let lines: Vec<Id> = (0..3)
                .map(|k| {
                    let (start, end) = (ids[k], ids[(k + 1) % 3]);
                    sketch.add_curve(Curve::Line { start, end }, false).unwrap()
                })
                .collect();
            AxisLine::Curve(lines[0])
        },
        Turn::Full,
        false,
        Operation::NewBody(BodyId::NEW),
    );
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let [a, b, c] = corners.map(|(x, y)| DVec2::new(x, y));
    let area = (b - a).perp_dot(c - a).abs() / 2.0;
    let middle = (a + b + c) / 3.0;
    let distance = (b - a).perp_dot(middle - a).abs() / (b - a).length();
    // Its apexes are fitted caps: within half the fit tolerance over
    // its area, as the kernel's own revolves are.
    let solid = only_body(&evaluation);
    let slack = 0.5 * editor.document().tolerance().fit() * solid.area();
    let volume = area * 2.0 * PI * distance;
    assert!(
        (solid.volume() - volume).abs() <= slack,
        "{}",
        solid.volume()
    );
}

/// A half disc whose flat side runs along the axis some way off it,
/// turned all the way round and nearly so: a half torus whose two faces
/// meet at creases leaving their rings into one quadrant of the
/// meridian plane (the flat side's cylinder and the round's top or
/// bottom). Repair used to split those rings until straight to the
/// resolution, and gave up at the default tolerance.
#[test]
fn a_half_torus_revolves_at_the_default_tolerance() {
    let (radius, height) = (5.0, 17.5);
    for (turn, fraction) in [("360", 1.0), ("358.8", 358.8 / 360.0)] {
        let mut editor = Editor::new(Document::default());
        let extent = Turn::OneSide(angle(editor.document(), turn));
        add_revolve(
            &mut editor,
            OriginPlane::XY,
            |sketch| {
                let [center, start, end] = [
                    (1.0, height),
                    (1.0 + radius, height),
                    (1.0 - radius, height),
                ]
                .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
                sketch
                    .add_curve(Curve::Arc { center, start, end }, false)
                    .unwrap();
                sketch
                    .add_curve(
                        Curve::Line {
                            start: end,
                            end: start,
                        },
                        false,
                    )
                    .unwrap();
                AxisLine::SketchX
            },
            extent,
            false,
            Operation::NewBody(BodyId::NEW),
        );
        let evaluation = evaluated(editor.document());
        let solid = only_body(&evaluation);
        // Pappus: the half disc's area times the turn of its centroid.
        let centroid = height + 4.0 * radius / (3.0 * PI);
        let volume = PI * radius * radius / 2.0 * 2.0 * PI * centroid * fraction;
        let slack = 0.5 * editor.document().tolerance().fit() * solid.area();
        assert!(
            (solid.volume() - volume).abs() <= slack,
            "{turn}: {} for {volume}",
            solid.volume()
        );
        let patches = solid.mesh().tris().len();
        assert!(patches <= 1_000, "{turn}: {patches} patches");
    }
}

/// An axis line whose ends differ by less than a float can give a
/// direction to (its length squared underflows) passes [`axis_line`]
/// but not [`axis_frame`]: the same words as a line of no length, and
/// the same showing, its start and the line.
#[test]
fn an_axis_line_too_short_for_a_direction_shows_where_it_is() {
    let tiny = 1e-310;
    let mut editor = Editor::new(Document::default());
    let revolve = add_revolve(
        &mut editor,
        OriginPlane::XY,
        rectangle_and_line((5.0, 0.0), (10.0, 4.0), (0.0, 2.0), (tiny, 2.0)),
        Turn::Full,
        false,
        Operation::NewBody(BodyId::NEW),
    );
    let FeatureKind::Revolve(made) = &editor.document().feature(revolve).unwrap().kind else {
        panic!("a revolve");
    };
    let AxisLine::Curve(line) = made.axis else {
        panic!("about a line");
    };
    let FeatureKind::Sketch { sketch, .. } = &editor.document().feature(made.sketch).unwrap().kind
    else {
        panic!("a sketch");
    };
    let axis = axis_line(sketch, made.axis).expect("a line of some length");
    assert_eq!(axis.along, DVec2::new(tiny, 0.0));
    let crate::Response::Regenerated { failed, .. } = crate::handle(regenerate_with(&editor, None))
    else {
        panic!("regeneration failed");
    };
    let [failure] = &failed[..] else {
        panic!("{failed:?}");
    };
    assert_eq!(
        (failure.feature, failure.message.as_str()),
        (revolve, "its axis line has no length")
    );
    let geometry = failure.geometry.as_ref().expect("the line's place");
    assert_eq!(geometry.points(), [[0.0, 2.0, 0.0]]);
    assert_eq!(geometry.sketch_curves(), [u64::from(line.get())]);
}

/// [`axis_frame`] refuses an axis with no direction, naming where it is
/// and its line; the words are those before the errors were typed.
#[test]
fn axis_errors_keep_their_words() {
    let axis = Axis {
        at: DVec2::new(1.0, 2.0),
        along: DVec2::new(0.0, -1e-200),
        curve: Some(Id::ORIGIN),
    };
    let profile = Profile { loops: Vec::new() };
    let refused = axis_frame(&profile, &axis, &OriginPlane::XY.placement()).unwrap_err();
    assert_eq!(
        refused,
        AxisError::NoLength {
            at: DVec2::new(1.0, 2.0),
            curve: Some(Id::ORIGIN),
        }
    );
    assert_eq!(refused.message(), "its axis line has no length");
    assert_eq!(AxisError::NotFound.message(), "axis not found");
    assert_eq!(
        AxisError::TooFar.message(),
        "its regions are too far from the axis to revolve"
    );
    // A line the axis names that isn't a line any more isn't found.
    let mut sketch = Sketch::default();
    let center = sketch.add_point(DVec2::ZERO).unwrap();
    let circle = (sketch.add_curve(
        Curve::Circle {
            center,
            radius: 1.0,
        },
        false,
    ))
    .unwrap();
    assert_eq!(
        axis_line(&sketch, AxisLine::Curve(circle)),
        Err(AxisError::NotFound)
    );
    assert_eq!(
        axis_line(&sketch, AxisLine::Curve(Id::ORIGIN)),
        Err(AxisError::NotFound)
    );
}
