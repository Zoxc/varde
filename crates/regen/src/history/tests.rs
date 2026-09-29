use std::f64::consts::PI;
use std::sync::Arc;

use glam::DVec2;
use varde_document::{
    BodyId, Command, Document, Editor, Extent, Extrude, FeatureId, FeatureKind, Operation,
    OriginPlane, Plane, Targets,
};
use varde_expr::Value;
use varde_kernel::{Frame, Solid, Tolerance};
use varde_sketch::{Curve, Sketch};

use super::*;

/// The example's extrude.
pub(crate) fn example_extrude(document: &Document) -> Extrude {
    match &document.features()[1].kind {
        FeatureKind::Extrude(extrude) => extrude.clone(),
        FeatureKind::Sketch { .. } => panic!("the example's second feature is its extrude"),
    }
}

/// A length of `text` in `document`'s units.
pub(crate) fn length(document: &Document, text: &str) -> Value {
    Value::new(text, &Extent::ask(&document.design())).unwrap()
}

/// Adds a cut through all of the example's regions, which isn't
/// available yet. Its id.
pub(crate) fn add_cut(editor: &mut Editor) -> FeatureId {
    let extrude = Extrude {
        extent: Extent::ThroughAll,
        operation: Operation::Cut(Targets::default()),
        ..example_extrude(editor.document())
    };
    editor
        .apply(editor.document().add_extrude(extrude))
        .unwrap();
    editor.document().features().last().unwrap().id
}

fn evaluated(document: &Document) -> Evaluation {
    evaluate(document, &mut Cache::default())
}

/// The volume of the example plate with a hole of `radius`, `height`
/// thick.
fn plate(radius: f64, height: f64) -> f64 {
    (60.0 * 40.0 - PI * radius * radius) * height
}

fn assert_near(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-9 * b.abs().max(1.0), "{a} vs {b}");
}

#[test]
fn the_example_plate_is_extruded() {
    let document = Document::example();
    let evaluation = evaluated(&document);
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let [made] = &evaluation.bodies[..] else {
        panic!("one body");
    };
    assert_eq!(made.body, document.bodies()[0].id);
    assert_near(made.solid.volume(), plate(8.0, 10.0));
    let bounds = made.solid.bounds3().unwrap();
    assert_eq!(bounds.min, glam::DVec3::new(-30.0, -20.0, 0.0));
    assert_eq!(bounds.max, glam::DVec3::new(30.0, 20.0, 10.0));
}

/// The example's sketch, changed by `change`, as one edit.
fn edit_sketch(editor: &mut Editor, change: impl FnOnce(&mut Sketch)) {
    let feature = editor.document().features()[0].id;
    let FeatureKind::Sketch { sketch, .. } = &editor.document().features()[0].kind else {
        panic!("the example's first feature is its sketch");
    };
    let mut sketch = sketch.clone();
    change(&mut sketch);
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
}

/// The id of the example's circle.
fn circle(sketch: &Sketch) -> varde_sketch::Id {
    sketch
        .curves
        .iter()
        .find(|entry| matches!(entry.curve, Curve::Circle { .. }))
        .unwrap()
        .id
}

#[test]
fn the_region_is_found_again_after_its_sketch_is_edited() {
    let mut editor = Editor::new(Document::example());
    edit_sketch(&mut editor, |sketch| {
        // A line elsewhere, and the hole's radius changed.
        let start = sketch.add_point(DVec2::new(100.0, 100.0)).unwrap();
        let end = sketch.add_point(DVec2::new(120.0, 100.0)).unwrap();
        sketch.add_curve(Curve::Line { start, end }, false).unwrap();
        let hole = circle(sketch);
        let Curve::Circle { radius, .. } = &mut sketch.curve_mut(hole).unwrap().curve else {
            unreachable!()
        };
        *radius = 5.0;
    });
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_near(evaluation.bodies[0].solid.volume(), plate(5.0, 10.0));

    // A second hole cut into the region: its curves differ now, so it's
    // found by the point inside it.
    edit_sketch(&mut editor, |sketch| {
        let center = sketch.add_point(DVec2::new(20.0, 0.0)).unwrap();
        sketch
            .add_curve(
                Curve::Circle {
                    center,
                    radius: 2.0,
                },
                false,
            )
            .unwrap();
    });
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let two_holes = plate(5.0, 10.0) - PI * 4.0 * 10.0;
    assert_near(evaluation.bodies[0].solid.volume(), two_holes);
}

#[test]
fn a_region_that_is_gone_fails_its_extrude() {
    let mut editor = Editor::new(Document::example());
    // Only the hole's circle is left.
    edit_sketch(&mut editor, |sketch| {
        let hole = circle(sketch);
        sketch.curves.retain(|entry| entry.id == hole);
    });
    let evaluation = evaluated(editor.document());
    let extrude = editor.document().features()[1].id;
    assert_eq!(
        evaluation.failed,
        [(extrude, "region not found".to_owned())]
    );
    assert!(evaluation.bodies.is_empty());
}

#[test]
fn several_regions_are_merged() {
    let mut editor = Editor::new(Document::example());
    // The hole's region too: the plate without a hole.
    let FeatureKind::Sketch { sketch, .. } = &editor.document().features()[0].kind else {
        unreachable!()
    };
    let profiles = sketch.profiles().unwrap();
    let regions = (0..profiles.regions.len())
        .map(|index| profiles.reference(index).unwrap())
        .collect();
    let feature = editor.document().features()[1].id;
    let extrude = Extrude {
        regions,
        extent: Extent::Symmetric(length(editor.document(), "4")),
        ..example_extrude(editor.document())
    };
    editor
        .apply(Command::SetExtrude {
            feature,
            extrude: Box::new(extrude),
        })
        .unwrap();
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let solid = &evaluation.bodies[0].solid;
    assert_near(solid.volume(), plate(0.0, 4.0));
    let bounds = solid.bounds3().unwrap();
    assert_eq!((bounds.min.z, bounds.max.z), (-2.0, 2.0));
}

#[test]
fn a_failing_feature_changes_no_body_and_later_ones_still_run() {
    let mut editor = Editor::new(Document::example());
    let cut = add_cut(&mut editor);
    // A flipped extrude after it, making a second body below the plate.
    let extrude = Extrude {
        flip: true,
        extent: Extent::OneSide(length(editor.document(), "3")),
        ..example_extrude(editor.document())
    };
    editor
        .apply(editor.document().add_extrude(extrude))
        .unwrap();
    let evaluation = evaluated(editor.document());
    assert_eq!(
        evaluation.failed,
        [(cut, "cutting isn't available yet".to_owned())]
    );
    let bodies: Vec<BodyId> = editor.document().bodies().iter().map(|b| b.id).collect();
    let made: Vec<BodyId> = evaluation.bodies.iter().map(|made| made.body).collect();
    assert_eq!(made, bodies);
    let below = evaluation.bodies[1].solid.bounds3().unwrap();
    assert_eq!((below.min.z, below.max.z), (-3.0, 0.0));
    assert_near(evaluation.bodies[1].solid.volume(), plate(8.0, 3.0));
}

#[test]
fn join_and_intersect_are_not_available_yet() {
    for (operation, error) in [
        (Operation::Join(Targets::default()), "joining"),
        (Operation::Intersect(Targets::default()), "intersecting"),
    ] {
        let mut editor = Editor::new(Document::example());
        let extrude = Extrude {
            operation,
            ..example_extrude(editor.document())
        };
        editor
            .apply(editor.document().add_extrude(extrude))
            .unwrap();
        let evaluation = evaluated(editor.document());
        assert_eq!(evaluation.failed.len(), 1);
        assert_eq!(
            evaluation.failed[0].1,
            format!("{error} isn't available yet")
        );
        assert_eq!(evaluation.bodies.len(), 1);
    }
}

#[test]
fn faces_are_named_by_the_extrude_and_its_curves() {
    let document = Document::example();
    let evaluation = evaluated(&document);
    let feature = document.features()[1].id.get();
    let FeatureKind::Sketch { sketch, .. } = &document.features()[0].kind else {
        unreachable!()
    };
    let curves: Vec<u64> = sketch
        .curves
        .iter()
        .map(|entry| u64::from(entry.id.get()))
        .collect();
    for face in evaluation.bodies[0].solid.mesh().faces() {
        assert_eq!(face.name.feature, feature);
        if let varde_kernel::mesh::FacePart::Side { curve, .. } = face.name.part {
            assert!(curves.contains(&curve), "{curve}");
        }
    }
}

#[test]
fn an_unchanged_feature_is_taken_from_the_cache() {
    let mut editor = Editor::new(Document::example());
    // An unrelated sketch after the extrude.
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XZ)))
        .unwrap();
    let mut cache = Cache::default();
    cache.begin();
    let first = evaluate(editor.document(), &mut cache);
    let (_, worked) = cache.counts();
    assert_eq!(worked, 3);

    // Editing the other sketch reruns only it.
    let other = editor.document().features()[2].id;
    let mut sketch = Sketch::default();
    let start = sketch.add_point(DVec2::ZERO).unwrap();
    let end = sketch.add_point(DVec2::ONE).unwrap();
    sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    editor
        .apply(Command::SetSketch {
            feature: other,
            sketch: Box::new(sketch),
        })
        .unwrap();
    cache.begin();
    let second = evaluate(editor.document(), &mut cache);
    assert_eq!(cache.counts(), (2, 4));
    assert!(Arc::ptr_eq(&first.bodies[0].solid, &second.bodies[0].solid));

    // The extrude's distance changed: it runs again, the rest found from
    // the request before.
    let feature = editor.document().features()[1].id;
    let set = |editor: &mut Editor, text: &str| {
        let extrude = Extrude {
            extent: Extent::OneSide(length(editor.document(), text)),
            ..example_extrude(editor.document())
        };
        editor
            .apply(Command::SetExtrude {
                feature,
                extrude: Box::new(extrude),
            })
            .unwrap();
    };
    set(&mut editor, "12");
    cache.begin();
    let thicker = evaluate(editor.document(), &mut cache);
    assert_eq!(cache.counts(), (4, 5));
    assert_near(thicker.bodies[0].solid.volume(), plate(8.0, 12.0));
    // What the request before didn't use is gone.
    set(&mut editor, "10");
    cache.begin();
    let back = evaluate(editor.document(), &mut cache);
    assert_eq!(cache.counts(), (6, 6));
    assert!(!Arc::ptr_eq(&first.bodies[0].solid, &back.bodies[0].solid));
    cache.begin();
    cache.begin();
    evaluate(editor.document(), &mut cache);
    assert_eq!(cache.counts(), (6, 9));
}

#[test]
fn a_new_tolerance_runs_everything_again() {
    let mut editor = Editor::new(Document::example());
    let mut cache = Cache::default();
    cache.begin();
    let fine = evaluate(editor.document(), &mut cache);
    let coarse = Tolerance::new(Tolerance::MAX_FIT).unwrap();
    editor.apply(Command::SetTolerance(coarse)).unwrap();
    cache.begin();
    let evaluation = evaluate(editor.document(), &mut cache);
    // The sketch's profiles don't depend on it.
    assert_eq!(cache.counts(), (1, 3));
    assert!(!Arc::ptr_eq(
        &fine.bodies[0].solid,
        &evaluation.bodies[0].solid
    ));
    assert_near(evaluation.bodies[0].solid.volume(), plate(8.0, 10.0));
}

#[test]
fn through_all_spans_the_bodies_with_a_margin() {
    let tol = Tolerance::DEFAULT;
    let a = Solid::cuboid(glam::DVec3::ZERO, glam::DVec3::splat(10.0), 1, &tol).unwrap();
    let b = Solid::cuboid(glam::DVec3::splat(-5.0), glam::DVec3::splat(2.0), 2, &tol).unwrap();
    let (from, to) = through_all(&Frame::XY, [&a, &b]).unwrap();
    // From -5 to 10, 15 across, so 0.15 + 1 each side.
    assert_near(from, -6.15);
    assert_near(to, 11.15);
    // Along another normal.
    let frame = Frame {
        origin: glam::DVec3::new(0.0, 3.0, 0.0),
        x: glam::DVec3::Z,
        y: glam::DVec3::X,
    };
    let (from, to) = through_all(&frame, [&a]).unwrap();
    assert_near(from, -3.0 - 1.1);
    assert_near(to, 7.0 + 1.1);
    assert_eq!(through_all(&Frame::XY, []), None);
    assert_eq!(through_all(&Frame::XY, [&Solid::empty()]), None);
}

#[test]
fn a_spline_is_extruded_within_the_tolerance() {
    let mut editor = Editor::new(Document::default());
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::YZ)))
        .unwrap();
    let feature = editor.document().features()[0].id;
    let mut sketch = Sketch::default();
    let points = [
        (5.0, 0.0),
        (2.0, 4.0),
        (-4.0, 3.0),
        (-3.0, -3.0),
        (2.0, -5.0),
    ]
    .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
    let curve = sketch
        .add_curve(
            Curve::Spline(varde_sketch::Spline::through(points.to_vec(), true)),
            false,
        )
        .unwrap();
    let profiles = sketch.profiles().unwrap();
    let region = profiles.reference(0).unwrap();
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch.clone()),
        })
        .unwrap();
    let extrude = Extrude {
        sketch: feature,
        regions: vec![region],
        extent: Extent::OneSide(length(editor.document(), "2")),
        flip: false,
        operation: Operation::NewBody(BodyId::NEW),
    };
    editor
        .apply(editor.document().add_extrude(extrude))
        .unwrap();
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let solid = &evaluation.bodies[0].solid;
    // The spline's area, finely sampled.
    let shape = sketch.spline_shape(sketch.spline(curve).unwrap()).unwrap();
    let n = 100_000;
    let area: f64 = (0..n)
        .map(|i| {
            let (a, b) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
            shape.point(a).perp_dot(shape.point(b)) / 2.0
        })
        .sum();
    let fit = Tolerance::DEFAULT.fit();
    assert!(
        (solid.volume() - area * 2.0).abs() <= fit * 40.0 * 2.0,
        "{} vs {}",
        solid.volume(),
        area * 2.0
    );
    // On YZ, extruded along X.
    let bounds = solid.bounds3().unwrap();
    assert_eq!((bounds.min.x, bounds.max.x), (0.0, 2.0));
}
