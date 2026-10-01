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

/// Adds a join of the example's regions that takes its only body out,
/// which fails. Its id.
pub(crate) fn add_failing(editor: &mut Editor) -> FeatureId {
    let body = editor.document().bodies()[0].id;
    let extrude = Extrude {
        operation: Operation::Join(Targets {
            excluded: vec![body],
        }),
        ..example_extrude(editor.document())
    };
    editor
        .apply(editor.document().add_extrude(extrude))
        .unwrap();
    editor.document().features().last().unwrap().id
}

/// Adds a sketch on XY drawn by `draw`, and an extrude of all its
/// regions over `extent` with `operation`. The extrude's id.
pub(crate) fn add_extrude(
    editor: &mut Editor,
    draw: impl FnOnce(&mut Sketch),
    extent: Extent,
    operation: Operation,
) -> FeatureId {
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XY)))
        .unwrap();
    let feature = editor.document().features().last().unwrap().id;
    let mut sketch = Sketch::default();
    draw(&mut sketch);
    let profiles = sketch.profiles().unwrap();
    let regions = (0..profiles.regions.len())
        .map(|index| profiles.reference(index).unwrap())
        .collect();
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    let extrude = Extrude {
        sketch: feature,
        regions,
        extent,
        flip: false,
        operation,
    };
    editor
        .apply(editor.document().add_extrude(extrude))
        .unwrap();
    editor.document().features().last().unwrap().id
}

/// Draws the rectangle from `min` to `max`.
pub(crate) fn rectangle(min: (f64, f64), max: (f64, f64)) -> impl FnOnce(&mut Sketch) {
    move |sketch| {
        let corners = [
            (min.0, min.1),
            (max.0, min.1),
            (max.0, max.1),
            (min.0, max.1),
        ]
        .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
        for (k, &start) in corners.iter().enumerate() {
            let end = corners[(k + 1) % corners.len()];
            sketch.add_curve(Curve::Line { start, end }, false).unwrap();
        }
    }
}

/// Draws the circle about `center` of `radius`.
pub(crate) fn disc(center: (f64, f64), radius: f64) -> impl FnOnce(&mut Sketch) {
    move |sketch| {
        let center = sketch.add_point(DVec2::new(center.0, center.1)).unwrap();
        sketch
            .add_curve(Curve::Circle { center, radius }, false)
            .unwrap();
    }
}

/// `a` and `b` together, as the extent of two sides.
pub(crate) fn two_sides(document: &Document, a: &str, b: &str) -> Extent {
    Extent::TwoSides(length(document, a), length(document, b))
}

/// Adds a pocket 13 × 20 mm and 4 mm deep cut up into the example
/// plate's bottom, clear of its hole, the tool from 1 mm below it. The
/// cut's id.
pub(crate) fn add_pocket(editor: &mut Editor) -> FeatureId {
    let extent = two_sides(editor.document(), "4", "1");
    add_extrude(
        editor,
        rectangle((-25.0, -10.0), (-12.0, 10.0)),
        extent,
        Operation::Cut(Targets::default()),
    )
}

/// The volume the pocket takes away.
pub(crate) const POCKET: f64 = 13.0 * 20.0 * 4.0;

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
    let failing = add_failing(&mut editor);
    // A flipped extrude after it, making a second body below the plate.
    let below = plate_below(&mut editor);
    let mut cache = Cache::default();
    let evaluation = evaluate(editor.document(), &mut cache);
    assert_eq!(
        evaluation.failed,
        [(
            failing,
            "it doesn't touch any body not taken out of it".to_owned()
        )]
    );
    // The sketch's profiles and the three solids: whether the join
    // touches the body it takes out isn't asked.
    assert_eq!(cache.counts().1, 4);
    let bodies: Vec<BodyId> = editor.document().bodies().iter().map(|b| b.id).collect();
    let made: Vec<BodyId> = evaluation.bodies.iter().map(|made| made.body).collect();
    assert_eq!(made, bodies);
    assert_eq!(bodies[1], below);
    assert_near(evaluation.bodies[0].solid.volume(), plate(8.0, 10.0));
    let bounds = evaluation.bodies[1].solid.bounds3().unwrap();
    assert_eq!((bounds.min.z, bounds.max.z), (-3.0, 0.0));
    assert_near(evaluation.bodies[1].solid.volume(), plate(8.0, 3.0));
    assert_eq!(evaluation.touched, [(failing, vec![])]);
}

/// Adds a second plate, 3 mm thick, below the example's: its body.
fn plate_below(editor: &mut Editor) -> BodyId {
    let extrude = Extrude {
        flip: true,
        extent: Extent::OneSide(length(editor.document(), "3")),
        ..example_extrude(editor.document())
    };
    editor
        .apply(editor.document().add_extrude(extrude))
        .unwrap();
    editor.document().bodies().last().unwrap().id
}

/// The only body's solid, after checking nothing failed.
fn only_body(evaluation: &Evaluation) -> &Solid {
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let [made] = &evaluation.bodies[..] else {
        panic!("one body");
    };
    &made.solid
}

#[test]
fn a_pocket_is_cut_into_the_plate() {
    let mut editor = Editor::new(Document::example());
    let cut = add_pocket(&mut editor);
    let evaluation = evaluated(editor.document());
    let solid = only_body(&evaluation);
    assert_near(solid.volume(), plate(8.0, 10.0) - POCKET);
    let bounds = solid.bounds3().unwrap();
    assert_eq!((bounds.min.z, bounds.max.z), (0.0, 10.0));
    let body = editor.document().bodies()[0].id;
    assert_eq!(evaluation.touched, [(cut, vec![body])]);
    // The pocket's faces carry the cut's id.
    let cut = cut.get();
    assert!(
        solid
            .mesh()
            .faces()
            .iter()
            .any(|face| face.name.feature == cut)
    );
}

#[test]
fn the_plate_joined_again_taller_is_one_body() {
    // The example's plate region extruded again as a join over a longer
    // span ("make it taller"): the two walls of every curve (the hole's
    // too) lie on each other past the first's caps. Refused before as
    // leaving no clean solid.
    for (text, height) in [("two sides", 15.0), ("one side", 12.0), ("symmetric", 30.0)] {
        let mut editor = Editor::new(Document::example());
        let d = editor.document();
        let extent = match text {
            "two sides" => Extent::TwoSides(length(d, "12"), length(d, "3")),
            "one side" => Extent::OneSide(length(d, "12")),
            _ => Extent::Symmetric(length(d, "30")),
        };
        let extrude = Extrude {
            extent,
            flip: false,
            operation: Operation::Join(Targets::default()),
            ..example_extrude(d)
        };
        editor
            .apply(editor.document().add_extrude(extrude))
            .unwrap();
        let evaluation = evaluated(editor.document());
        let solid = only_body(&evaluation);
        assert_near(solid.volume(), plate(8.0, height));
    }
}

#[test]
fn a_boss_is_joined_to_the_plate() {
    let mut editor = Editor::new(Document::example());
    let extent = Extent::OneSide(length(editor.document(), "15"));
    add_extrude(
        &mut editor,
        disc((20.0, 0.0), 5.0),
        extent,
        Operation::Join(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    let solid = only_body(&evaluation);
    assert_near(solid.volume(), plate(8.0, 10.0) + PI * 25.0 * 5.0);
    let bounds = solid.bounds3().unwrap();
    assert_eq!((bounds.min.z, bounds.max.z), (0.0, 15.0));
}

#[test]
fn the_plate_is_intersected() {
    let mut editor = Editor::new(Document::example());
    let extent = two_sides(editor.document(), "20", "20");
    add_extrude(
        &mut editor,
        rectangle((0.0, -30.0), (40.0, 30.0)),
        extent,
        Operation::Intersect(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    let solid = only_body(&evaluation);
    // The plate's right half, with half the hole.
    assert_near(solid.volume(), (30.0 * 40.0 - PI * 64.0 / 2.0) * 10.0);
    let bounds = solid.bounds3().unwrap();
    assert_eq!((bounds.min.x, bounds.max.x), (0.0, 30.0));
}

#[test]
fn through_all_cuts_through_the_bodies() {
    let mut editor = Editor::new(Document::example());
    add_extrude(
        &mut editor,
        disc((-20.0, 10.0), 3.0),
        Extent::ThroughAll,
        Operation::Cut(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    let solid = only_body(&evaluation);
    assert_near(solid.volume(), plate(8.0, 10.0) - PI * 9.0 * 10.0);
}

#[test]
fn a_tall_plate_is_drilled_through_all() {
    // The example plate a metre tall: the plate's cap edges cross the
    // drill's wall, 1 m tall and a few mm wide, and the search for those
    // crossings ran out of pieces splitting the wall across its width.
    for center in [(-20.0, 10.0), (-17.27, 5.21), (13.61, 1.26)] {
        let mut editor = Editor::new(Document::example());
        let feature = editor.document().features()[1].id;
        let extrude = Extrude {
            extent: Extent::OneSide(length(editor.document(), "1000")),
            ..example_extrude(editor.document())
        };
        editor
            .apply(Command::SetExtrude {
                feature,
                extrude: Box::new(extrude),
            })
            .unwrap();
        add_extrude(
            &mut editor,
            disc(center, 3.0),
            Extent::ThroughAll,
            Operation::Cut(Targets::default()),
        );
        let evaluation = evaluated(editor.document());
        assert!(
            evaluation.failed.is_empty(),
            "{center:?}: {:?}",
            evaluation.failed
        );
        let solid = only_body(&evaluation);
        assert_near(solid.volume(), plate(8.0, 1000.0) - PI * 9.0 * 1000.0);
    }
}

#[test]
fn bodies_taken_out_are_left_as_they_are() {
    let mut editor = Editor::new(Document::example());
    let top = editor.document().bodies()[0].id;
    let below = plate_below(&mut editor);
    let cut = add_extrude(
        &mut editor,
        disc((-20.0, 10.0), 3.0),
        Extent::ThroughAll,
        Operation::Cut(Targets {
            excluded: vec![top],
        }),
    );
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_eq!(evaluation.touched, [(cut, vec![below])]);
    let volumes: Vec<f64> = (evaluation.bodies.iter())
        .map(|made| made.solid.volume())
        .collect();
    assert_near(volumes[0], plate(8.0, 10.0));
    assert_near(volumes[1], plate(8.0, 3.0) - PI * 9.0 * 3.0);
}

#[test]
fn a_join_touching_two_bodies_is_added_to_each() {
    let mut editor = Editor::new(Document::example());
    let top = editor.document().bodies()[0].id;
    let below = plate_below(&mut editor);
    let extent = two_sides(editor.document(), "15", "5");
    let join = add_extrude(
        &mut editor,
        disc((20.0, 0.0), 5.0),
        extent,
        Operation::Join(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_eq!(evaluation.touched, [(join, vec![top, below])]);
    // Bodies never merge: both hold the boss, so they overlap.
    let boss = PI * 25.0 * 20.0;
    assert_near(
        evaluation.bodies[0].solid.volume(),
        plate(8.0, 10.0) + boss - PI * 25.0 * 10.0,
    );
    assert_near(
        evaluation.bodies[1].solid.volume(),
        plate(8.0, 3.0) + boss - PI * 25.0 * 3.0,
    );
}

/// A cut that would take the whole plate fails and leaves it as it was,
/// so a join after it still finds the plate.
#[test]
fn a_cut_leaving_nothing_of_a_body_fails() {
    let mut editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let cut = add_extrude(
        &mut editor,
        rectangle((-40.0, -30.0), (40.0, 30.0)),
        Extent::ThroughAll,
        Operation::Cut(Targets::default()),
    );
    let extent = Extent::OneSide(length(editor.document(), "15"));
    let join = add_extrude(
        &mut editor,
        disc((20.0, 0.0), 5.0),
        extent,
        Operation::Join(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(
        evaluation.failed,
        [(cut, message::emptied(Doing::Cutting, "Body 1"))]
    );
    assert!(evaluation.failed[0].1.starts_with("cutting it from Body 1"));
    // Listed, so the panel offers to untick it.
    assert_eq!(evaluation.touched, [(cut, vec![body]), (join, vec![body])]);
    assert_near(
        evaluation.bodies[0].solid.volume(),
        plate(8.0, 10.0) + PI * 25.0 * 5.0,
    );
}

/// A disc below the plate, flush on its bottom face only: the
/// intersection is empty, which fails.
#[test]
fn an_intersect_only_flush_with_a_body_fails() {
    let mut editor = Editor::new(Document::example());
    let extent = Extent::OneSide(length(editor.document(), "5"));
    let intersect = add_extrude(
        &mut editor,
        disc((20.0, 0.0), 5.0),
        extent,
        Operation::Intersect(Targets::default()),
    );
    set_extrude(&mut editor, intersect, |extrude| extrude.flip = true);
    // The kernel gives the empty solid, not `Invalid`, which would read
    // "leaves no clean solid".
    let evaluation = evaluated(editor.document());
    assert_eq!(
        evaluation.failed,
        [(intersect, message::emptied(Doing::Intersecting, "Body 1"))]
    );
    assert_near(evaluation.bodies[0].solid.volume(), plate(8.0, 10.0));
}

/// A cut that would empty one body and cut another changes neither;
/// with the first unticked, it cuts the other.
#[test]
fn a_feature_leaving_one_target_empty_changes_none() {
    let mut editor = Editor::new(Document::example());
    let top = editor.document().bodies()[0].id;
    let below = plate_below(&mut editor);
    let extent = two_sides(editor.document(), "10", "1");
    let cut = add_extrude(
        &mut editor,
        rectangle((-40.0, -30.0), (40.0, 30.0)),
        extent,
        Operation::Cut(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(
        evaluation.failed,
        [(cut, message::emptied(Doing::Cutting, "Body 1"))]
    );
    assert_eq!(evaluation.touched, [(cut, vec![top, below])]);
    assert_near(evaluation.bodies[0].solid.volume(), plate(8.0, 10.0));
    assert_near(evaluation.bodies[1].solid.volume(), plate(8.0, 3.0));

    set_extrude(&mut editor, cut, |extrude| {
        extrude.operation = Operation::Cut(Targets {
            excluded: vec![top],
        });
    });
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_near(evaluation.bodies[0].solid.volume(), plate(8.0, 10.0));
    assert_near(
        evaluation.bodies[1].solid.volume(),
        plate(8.0, 3.0) * 2.0 / 3.0,
    );
}

/// The parametric case: the plate made smaller than a cut after it, the
/// cut goes from cutting the plate to failing, and the plate is kept.
#[test]
fn a_cut_failing_after_an_upstream_edit_empties_nothing() {
    let mut editor = Editor::new(Document::example());
    let cut = add_extrude(
        &mut editor,
        disc((0.0, 0.0), 15.0),
        Extent::ThroughAll,
        Operation::Cut(Targets::default()),
    );
    let mut cache = Cache::default();
    let before = evaluate(editor.document(), &mut cache);
    assert!(before.failed.is_empty(), "{:?}", before.failed);
    assert_near(
        before.bodies[0].solid.volume(),
        (60.0 * 40.0 - PI * 225.0) * 10.0,
    );
    // The plate's rectangle 20 × 20, inside the disc.
    edit_sketch(&mut editor, |sketch| {
        for point in sketch.points.iter_mut().filter(|p| p.at.x.abs() == 30.0) {
            point.at = point.at.signum() * 10.0;
        }
    });
    let after = evaluate(editor.document(), &mut cache);
    assert_eq!(
        after.failed,
        [(cut, message::emptied(Doing::Cutting, "Body 1"))]
    );
    assert_near(after.bodies[0].solid.volume(), (400.0 - PI * 64.0) * 10.0);
}

/// The plate made big enough for a cut that empties it, then the edit
/// undone and redone: the cut fails, cuts and fails again, the plate
/// kept whole each time it fails, also with the results found in the
/// cache.
#[test]
fn undoing_the_edit_that_saved_a_body_empties_nothing() {
    let mut editor = Editor::new(Document::example());
    let cut = add_extrude(
        &mut editor,
        rectangle((-40.0, -30.0), (40.0, 30.0)),
        Extent::ThroughAll,
        Operation::Cut(Targets::default()),
    );
    let failing = [(cut, message::emptied(Doing::Cutting, "Body 1"))];
    let mut cache = Cache::default();
    let check = |cache: &mut Cache, editor: &Editor, fails: bool| {
        cache.begin();
        let evaluation = evaluate(editor.document(), cache);
        if fails {
            assert_eq!(evaluation.failed, failing);
            assert_near(evaluation.bodies[0].solid.volume(), plate(8.0, 10.0));
        } else {
            // A frame 100 × 80 round the cut's 80 × 60.
            assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
            assert_near(
                evaluation.bodies[0].solid.volume(),
                (100.0 * 80.0 - 80.0 * 60.0) * 10.0,
            );
        }
    };
    check(&mut cache, &editor, true);
    edit_sketch(&mut editor, |sketch| {
        for point in sketch.points.iter_mut().filter(|p| p.at.x.abs() == 30.0) {
            point.at = DVec2::new(point.at.x.signum() * 50.0, point.at.y.signum() * 40.0);
        }
    });
    check(&mut cache, &editor, false);
    for _ in 0..2 {
        editor.undo();
        check(&mut cache, &editor, true);
        editor.redo();
        check(&mut cache, &editor, false);
    }
}

/// An intersect with every body taken out of it fails as touching none
/// left in, not as leaving one empty, and changes none.
#[test]
fn an_intersect_with_every_body_taken_out_changes_none() {
    let mut editor = Editor::new(Document::example());
    let top = editor.document().bodies()[0].id;
    let below = plate_below(&mut editor);
    let extent = two_sides(editor.document(), "5", "1");
    let intersect = add_extrude(
        &mut editor,
        disc((20.0, 0.0), 5.0),
        extent,
        Operation::Intersect(Targets {
            excluded: vec![top, below],
        }),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(
        evaluation.failed,
        [(
            intersect,
            "it doesn't touch any body not taken out of it".to_owned()
        )]
    );
    assert_eq!(evaluation.touched, [(intersect, vec![])]);
    assert_near(evaluation.bodies[0].solid.volume(), plate(8.0, 10.0));
    assert_near(evaluation.bodies[1].solid.volume(), plate(8.0, 3.0));

    // Only the lower plate put back: the disc 1 mm into it leaves a
    // disc 1 mm thick.
    set_extrude(&mut editor, intersect, |extrude| {
        extrude.operation = Operation::Intersect(Targets {
            excluded: vec![top],
        });
    });
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_near(evaluation.bodies[0].solid.volume(), plate(8.0, 10.0));
    assert_near(evaluation.bodies[1].solid.volume(), PI * 25.0);
}

#[test]
fn a_join_touching_no_body_fails() {
    let mut editor = Editor::new(Document::example());
    let extent = Extent::OneSide(length(editor.document(), "5"));
    let join = add_extrude(
        &mut editor,
        disc((100.0, 100.0), 2.0),
        extent,
        Operation::Join(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(
        evaluation.failed,
        [(join, "it doesn't touch any body".to_owned())]
    );
    assert_eq!(evaluation.touched, [(join, vec![])]);
    assert_near(evaluation.bodies[0].solid.volume(), plate(8.0, 10.0));
}

#[test]
fn editing_the_plate_regenerates_the_cut() {
    let mut editor = Editor::new(Document::example());
    add_pocket(&mut editor);
    let mut cache = Cache::default();
    cache.begin();
    let before = evaluate(editor.document(), &mut cache);
    assert_near(only_body(&before).volume(), plate(8.0, 10.0) - POCKET);
    let (_, worked) = cache.counts();

    let feature = editor.document().features()[1].id;
    let extrude = Extrude {
        extent: Extent::OneSide(length(editor.document(), "12")),
        ..example_extrude(editor.document())
    };
    editor
        .apply(Command::SetExtrude {
            feature,
            extrude: Box::new(extrude),
        })
        .unwrap();
    cache.begin();
    let after = evaluate(editor.document(), &mut cache);
    let solid = only_body(&after);
    assert_near(solid.volume(), plate(8.0, 12.0) - POCKET);
    assert_eq!(solid.bounds3().unwrap().max.z, 12.0);
    // The plate, whether the pocket's tool touches it, and the cut ran
    // again; both sketches and the pocket's tool were found.
    assert_eq!(cache.counts().1, worked + 3);
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
    // Without a budget, so only what the request before used is kept
    // (the default budget keeps the rest too: see the regenerator's
    // tests).
    let mut cache = Cache::with_budget(0);
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

/// The extrude `feature` of `editor`'s document, changed by `change`,
/// as one edit.
pub(crate) fn set_extrude(
    editor: &mut Editor,
    feature: FeatureId,
    change: impl FnOnce(&mut Extrude),
) {
    let Some(FeatureKind::Extrude(extrude)) = editor.document().feature(feature).map(|f| &f.kind)
    else {
        panic!("{feature:?} is an extrude");
    };
    let mut extrude = extrude.clone();
    change(&mut extrude);
    editor
        .apply(Command::SetExtrude {
            feature,
            extrude: Box::new(extrude),
        })
        .unwrap();
}

/// A body `touches` can't tell fails the feature and is listed, so the
/// panel offers to take it out; taken out, it isn't asked about.
#[test]
fn a_body_that_cant_be_told_is_passed_over_or_listed() {
    let mut editor = Editor::new(Document::example());
    let top = editor.document().bodies()[0].id;
    let below = plate_below(&mut editor);
    let extent = two_sides(editor.document(), "20", "20");
    let cut = add_extrude(
        &mut editor,
        disc((-20.0, 10.0), 3.0),
        extent,
        Operation::Cut(Targets::default()),
    );
    let all = editor.document().clone();
    set_extrude(&mut editor, cut, |extrude| {
        extrude.operation = Operation::Cut(Targets {
            excluded: vec![top],
        });
    });
    let taken_out = editor.document().clone();
    // The same with a thicker plate on top, so what the plate below
    // needs is kept from it, and the top plate's `touches`, if asked,
    // runs again, with no budget, and can't tell.
    let first = editor.document().features()[1].id;
    set_extrude(&mut editor, first, |extrude| {
        extrude.extent = Extent::OneSide(length(&all, "12"));
    });
    let mut cache = Cache::default();
    cache.begin();
    let warm = evaluate(editor.document(), &mut cache);
    assert!(warm.failed.is_empty(), "{:?}", warm.failed);

    cache.begin();
    let evaluation = evaluate_within(&taken_out, &mut cache, Budget::new(0));
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_eq!(evaluation.touched, [(cut, vec![below])]);
    assert_near(evaluation.bodies[0].solid.volume(), plate(8.0, 10.0));
    assert_near(
        evaluation.bodies[1].solid.volume(),
        plate(8.0, 3.0) - PI * 9.0 * 3.0,
    );

    cache.begin();
    let evaluation = evaluate_within(&all, &mut cache, Budget::new(0));
    let [(failed, error)] = &evaluation.failed[..] else {
        panic!("{:?}", evaluation.failed);
    };
    assert_eq!(*failed, cut);
    assert!(
        error.starts_with("finding where it meets Body 1"),
        "{error}"
    );
    assert_eq!(evaluation.touched, [(cut, vec![top])]);
    assert_near(evaluation.bodies[1].solid.volume(), plate(8.0, 3.0));
}

#[test]
fn too_thin_at_the_finest_tolerance_suggests_none_finer() {
    // A strip 1e-7 mm wide: 10 resolutions at the finest tolerance, too
    // thin for its walls and caps, and there is no finer one to try.
    let mut editor = Editor::new(Document::default());
    let finest = Tolerance::new(Tolerance::MIN_FIT).unwrap();
    editor.apply(Command::SetTolerance(finest)).unwrap();
    let extent = Extent::OneSide(length(editor.document(), "1"));
    let strip = add_extrude(
        &mut editor,
        rectangle((0.0, 0.0), (10.0, 1e-7)),
        extent,
        Operation::NewBody(BodyId::NEW),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(
        evaluation.failed,
        [(
            strip,
            "its regions have parts too thin or too close together to extrude, even at \
             the finest tolerance"
                .to_owned()
        )]
    );
}

/// A round body 4 mm tall about the origin at `fit`, and an extrude
/// with `operation` of a disc of the same radius tangent to it along a
/// line, at `angle` from the sketch's x axis, 1 mm tall from the body's
/// middle. The body's id and the extrude's.
fn tangent_discs(fit: f64, angle: f64, operation: Operation) -> (Editor, BodyId, FeatureId) {
    let mut editor = Editor::new(Document::default());
    let tolerance = Tolerance::new(fit).unwrap();
    editor.apply(Command::SetTolerance(tolerance)).unwrap();
    let extent = two_sides(editor.document(), "2", "2");
    add_extrude(
        &mut editor,
        disc((0.0, 0.0), 1.0),
        extent,
        Operation::NewBody(BodyId::NEW),
    );
    let body = editor.document().bodies()[0].id;
    let extent = Extent::OneSide(length(editor.document(), "1"));
    let center = (2.0 * angle.cos(), 2.0 * angle.sin());
    let feature = add_extrude(&mut editor, disc(center, 1.0), extent, operation);
    (editor, body, feature)
}

/// A boss tangent to a body only along a line touches it, so the join
/// fails naming the body (the union refuses the line contact, or runs
/// out working it out at the default tolerance), never as touching no
/// body; the body is listed and kept as it was. At the default
/// tolerance `touches` finds it within less than the old refinement
/// took.
#[test]
fn a_join_tangent_to_a_body_along_a_line_names_it() {
    let (editor, body, join) =
        tangent_discs(Tolerance::MAX_FIT, 0.7, Operation::Join(Targets::default()));
    let evaluation = evaluated(editor.document());
    let [(failed, error)] = &evaluation.failed[..] else {
        panic!("{:?}", evaluation.failed);
    };
    assert_eq!(*failed, join);
    assert!(
        error.starts_with("joining it to Body 1 leaves no clean solid"),
        "{error}"
    );
    assert_eq!(evaluation.touched, [(join, vec![body])]);
    assert_near(evaluation.bodies[0].solid.volume(), PI * 4.0);

    let (editor, body, join) = tangent_discs(1e-3, 0.7, Operation::Join(Targets::default()));
    let evaluation = evaluate_within(
        editor.document(),
        &mut Cache::default(),
        Budget::new(200_000),
    );
    let [(failed, error)] = &evaluation.failed[..] else {
        panic!("{:?}", evaluation.failed);
    };
    assert_eq!(*failed, join);
    assert!(
        error.starts_with("joining it to Body 1 is too complex"),
        "{error}"
    );
    assert_eq!(evaluation.touched, [(join, vec![body])]);
    assert_near(evaluation.bodies[0].solid.volume(), PI * 4.0);
}

/// A cut tangent to a body along a line targets it: with the line on
/// the circles' seam the difference is the body unchanged, a no-op;
/// off it the kernel can't tell the tangent faces apart and the cut
/// fails naming the body, changing nothing.
#[test]
fn a_cut_tangent_to_a_body_along_a_line_is_a_no_op_or_names_it() {
    let (editor, body, cut) =
        tangent_discs(Tolerance::MAX_FIT, 0.0, Operation::Cut(Targets::default()));
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_eq!(evaluation.touched, [(cut, vec![body])]);
    assert_near(evaluation.bodies[0].solid.volume(), PI * 4.0);

    let (editor, body, cut) =
        tangent_discs(Tolerance::MAX_FIT, 0.7, Operation::Cut(Targets::default()));
    let evaluation = evaluated(editor.document());
    let [(failed, error)] = &evaluation.failed[..] else {
        panic!("{:?}", evaluation.failed);
    };
    assert_eq!(*failed, cut);
    assert!(
        error.starts_with("cutting it from Body 1 can't be worked out"),
        "{error}"
    );
    assert_eq!(evaluation.touched, [(cut, vec![body])]);
    assert_near(evaluation.bodies[0].solid.volume(), PI * 4.0);
}
