use glam::{DVec2, Vec3};
use varde_document::{Command, Document, Editor, OriginPlane, Plane, Sketch};
use varde_sketch::{CIRCLE_SEGMENTS, Constraint, Curve};

use super::*;

fn regenerate(editor: &Editor, exclude: Option<FeatureId>) -> Request {
    Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude,
    }
}

/// The example cube and a sketch on the XZ plane holding a line from
/// (0, 0) to (2, 1), a construction circle, and a quarter arc, and the
/// sketch's id.
pub(crate) fn sketched() -> (Editor, FeatureId) {
    let mut editor = Editor::new(Document::example());
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XZ)))
        .unwrap();
    let feature = editor.document().features()[0].id;
    let mut sketch = Sketch::default();
    let start = sketch.add_point(DVec2::ZERO).unwrap();
    let end = sketch.add_point(DVec2::new(2.0, 1.0)).unwrap();
    sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    let center = sketch.add_point(DVec2::new(5.0, 5.0)).unwrap();
    let radius = 1.0;
    sketch
        .add_curve(Curve::Circle { center, radius }, true)
        .unwrap();
    let arc_start = sketch.add_point(DVec2::new(6.0, 5.0)).unwrap();
    let arc_end = sketch.add_point(DVec2::new(5.0, 6.0)).unwrap();
    sketch
        .add_curve(
            Curve::Arc {
                center,
                start: arc_start,
                end: arc_end,
            },
            false,
        )
        .unwrap();
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    (editor, feature)
}

#[test]
fn regenerate_tessellates_the_snapshot() {
    let editor = Editor::new(Document::example());
    let Response::Regenerated {
        generation,
        exclude,
        mesh,
        sketches,
        unsolved,
    } = handle(regenerate(&editor, None))
    else {
        panic!("regeneration failed");
    };
    assert_eq!(generation, editor.generation());
    assert_eq!(exclude, None);
    assert_eq!(*mesh, crate::tessellate(editor.document()).unwrap());
    assert_eq!(mesh.triangle_count(), 12);
    assert_eq!(*sketches, RenderLines::default());
    assert!(unsolved.is_empty());
}

/// Adds a sketch on XY that doesn't solve, as a file could hold: a line
/// between two fixed points held horizontal, though they're at different
/// heights. Its id.
pub(crate) fn unsolvable(editor: &mut Editor) -> FeatureId {
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XY)))
        .unwrap();
    let feature = editor.document().features().last().unwrap().id;
    let mut sketch = Sketch::default();
    let start = sketch.add_point(DVec2::ZERO).unwrap();
    let end = sketch.add_point(DVec2::new(4.0, 1.0)).unwrap();
    sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    for constraint in [
        Constraint::Fix(start),
        Constraint::Fix(end),
        Constraint::HorizontalPoints(start, end),
    ] {
        sketch.add_constraint(constraint).unwrap();
    }
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    feature
}

#[test]
fn sketches_that_do_not_solve_are_named() {
    let (mut editor, solved) = sketched();
    let unsolved = unsolvable(&mut editor);
    let Response::Regenerated {
        unsolved: named, ..
    } = handle(regenerate(&editor, None))
    else {
        panic!("regeneration failed");
    };
    assert_eq!(named, [unsolved]);
    assert_ne!(solved, unsolved);
}

#[test]
fn regenerate_flattens_the_sketches() {
    let (editor, feature) = sketched();
    let Response::Regenerated { mesh, sketches, .. } = handle(regenerate(&editor, None)) else {
        panic!("regeneration failed");
    };
    assert_eq!(mesh.triangle_count(), 12);
    assert_eq!(
        *sketches,
        flatten_sketches(editor.document(), None).unwrap()
    );
    assert_eq!(sketches.ends().len(), 2);

    // The sketch being edited is left out, and the answer says so.
    let Response::Regenerated {
        exclude, sketches, ..
    } = handle(regenerate(&editor, Some(feature)))
    else {
        panic!("regeneration failed");
    };
    assert_eq!(exclude, Some(feature));
    assert_eq!(*sketches, RenderLines::default());
}

#[test]
fn sketches_are_placed_on_their_planes() {
    let (editor, _) = sketched();
    let lines = flatten_sketches(editor.document(), None).unwrap();
    // The construction circle is left out.
    let polylines: Vec<_> = lines.polylines().collect();
    assert_eq!(polylines.len(), 2);
    // On XZ, the sketch's y is the world's Z.
    assert_eq!(polylines[0], [[0.0; 3], [2.0, 0.0, 1.0]]);
    let arc = polylines[1];
    assert_eq!(arc.len(), CIRCLE_SEGMENTS / 4 + 1);
    assert_eq!(arc[0], [6.0, 0.0, 5.0]);
    assert_eq!(arc[arc.len() - 1], [5.0, 0.0, 6.0]);
    for point in arc {
        let offset = Vec3::from(*point) - Vec3::new(5.0, 0.0, 5.0);
        assert!((offset.length() - 1.0).abs() < 1e-6, "{point:?}");
        assert_eq!(offset.y, 0.0);
    }
}

#[test]
fn hidden_and_excluded_sketches_are_left_out() {
    let (mut editor, feature) = sketched();
    // A second sketch, on XY, with a line.
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XY)))
        .unwrap();
    let second = editor.document().features()[1].id;
    let mut sketch = Sketch::default();
    let start = sketch.add_point(DVec2::new(1.0, 2.0)).unwrap();
    let end = sketch.add_point(DVec2::new(3.0, 4.0)).unwrap();
    sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    editor
        .apply(Command::SetSketch {
            feature: second,
            sketch: Box::new(sketch),
        })
        .unwrap();
    let only_second = [[[1.0, 2.0, 0.0], [3.0, 4.0, 0.0]]];

    let lines = flatten_sketches(editor.document(), Some(feature)).unwrap();
    assert_eq!(lines.polylines().collect::<Vec<_>>(), only_second);
    assert_eq!(
        flatten_sketches(editor.document(), None)
            .unwrap()
            .ends()
            .len(),
        3
    );

    editor
        .apply(Command::SetFeatureVisible(feature, false))
        .unwrap();
    let lines = flatten_sketches(editor.document(), None).unwrap();
    assert_eq!(lines.polylines().collect::<Vec<_>>(), only_second);
    let lines = flatten_sketches(editor.document(), Some(second)).unwrap();
    assert_eq!(lines, RenderLines::default());
}

#[test]
fn sketches_far_out_stay_within_the_lines_bound() {
    // The largest arc a checked sketch can hold, from one corner of the
    // coordinate limit round the far side of another.
    let max = f64::from(varde_document::MAX_COORD);
    let mut editor = Editor::new(Document::default());
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::YZ)))
        .unwrap();
    let feature = editor.document().features()[0].id;
    let mut sketch = Sketch::default();
    let center = sketch.add_point(DVec2::new(max, max)).unwrap();
    let start = sketch.add_point(DVec2::new(-max, -max)).unwrap();
    let end = sketch.add_point(DVec2::new(-max, -max + 1.0)).unwrap();
    sketch
        .add_curve(Curve::Arc { center, start, end }, false)
        .unwrap();
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    let lines = flatten_sketches(editor.document(), None).unwrap();
    assert_eq!(lines.points().len(), CIRCLE_SEGMENTS + 1);
}

#[test]
fn lines_are_drawn_without_the_ends_a_chamfer_cuts_off() {
    let mut editor = Editor::new(Document::default());
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XY)))
        .unwrap();
    let feature = editor.document().features()[0].id;
    let mut sketch = Sketch::default();
    let corner = sketch.add_point(DVec2::ZERO).unwrap();
    let along = sketch.add_point(DVec2::new(20.0, 0.0)).unwrap();
    let up = sketch.add_point(DVec2::new(0.0, 10.0)).unwrap();
    let a = sketch
        .add_curve(
            Curve::Line {
                start: corner,
                end: along,
            },
            false,
        )
        .unwrap();
    let b = sketch
        .add_curve(
            Curve::Line {
                start: up,
                end: corner,
            },
            false,
        )
        .unwrap();
    // A chamfer cutting 1 off each.
    let start = sketch.add_point(DVec2::new(1.0, 0.0)).unwrap();
    let end = sketch.add_point(DVec2::new(0.0, 1.0)).unwrap();
    let chamfer = sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    sketch.curve_mut(chamfer).unwrap().corner = Some(varde_sketch::Corner {
        a,
        b,
        at: corner,
        equal: true,
    });
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    let lines = flatten_sketches(editor.document(), None).unwrap();
    assert_eq!(
        lines.polylines().collect::<Vec<_>>(),
        [
            [[1.0, 0.0, 0.0], [20.0, 0.0, 0.0]],
            [[0.0, 10.0, 0.0], [0.0, 1.0, 0.0]],
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        ]
    );
}
