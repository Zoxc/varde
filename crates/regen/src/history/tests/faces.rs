//! Sketches on faces in the history: placed on the face as the features
//! before them leave it, following it when those change, through a join
//! that consumes its body; failing when the face is gone, isn't flat or
//! its body is gone; tilted faces end to end; and what's cached.

use glam::DVec3;
use varde_document::{AxisLine, FaceRef, Placement, Revolve, Turn};
use varde_kernel::mesh::{FaceKey, Form, PartKey};

use super::*;
use crate::Draft;
use crate::picking::region_form;
use crate::tests::{answered, regenerate_with};

/// Adds a sketch on `plane` drawn by `draw`: its id.
fn add_sketch(editor: &mut Editor, plane: Plane, draw: impl FnOnce(&mut Sketch)) -> FeatureId {
    editor.apply(editor.document().add_sketch(plane)).unwrap();
    let feature = editor.document().features().last().unwrap().id;
    let mut sketch = Sketch::default();
    draw(&mut sketch);
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    feature
}

/// Adds an extrude of all the regions of `sketch` over `extent` with
/// `operation`: its id.
fn add_extrude_of(
    editor: &mut Editor,
    sketch: FeatureId,
    extent: Extent,
    operation: Operation,
) -> FeatureId {
    let FeatureKind::Sketch { sketch: drawn, .. } =
        &editor.document().feature(sketch).unwrap().kind
    else {
        panic!("{sketch:?} is a sketch");
    };
    let profiles = drawn.profiles().unwrap();
    let regions = (0..profiles.regions.len())
        .map(|index| profiles.reference(index).unwrap())
        .collect();
    let extrude = Extrude {
        sketch,
        regions,
        extent,
        flip: false,
        operation,
    };
    editor
        .apply(editor.document().add_feature(extrude.into()))
        .unwrap();
    editor.document().features().last().unwrap().id
}

/// One side, `text` in millimetres (every test's design's unit).
fn one_side(text: &str) -> Extent {
    Extent::OneSide(length(&Document::default(), text))
}

fn join() -> Operation {
    Operation::Join(Targets::default())
}

fn cut() -> Operation {
    Operation::Cut(Targets::default())
}

/// The example plate's top face (its extrude's end cap), picked at
/// `(20, 0, 10)`.
fn top(document: &Document) -> FaceRef {
    FaceRef {
        body: document.bodies()[0].id,
        key: FaceKey {
            feature: document.features()[1].id.get(),
            part: PartKey::EndCap,
            instance: 0,
        },
        near: DVec3::new(20.0, 0.0, 10.0),
    }
}

/// The key of the one region of `solid` whose form `pick` takes, as a
/// user picking it would get.
fn key_where(solid: &Solid, pick: impl Fn(&Form) -> bool) -> FaceKey {
    let topology = solid.topology();
    let picked: Vec<FaceKey> = (topology.regions().iter())
        .filter(|region| pick(region_form(solid, region)))
        .map(|region| region.key)
        .collect();
    let [key] = picked[..] else {
        panic!("one region is picked: {picked:?}");
    };
    key
}

/// Whether `form` is a plane whose normal is `n`'s direction, within
/// rounding.
fn plane_along(form: &Form, n: DVec3) -> bool {
    matches!(*form, Form::Plane { n: m, .. } if m.normalize().dot(n.normalize()) > 1.0 - 1e-12)
}

/// The sketch coordinates of the world point `at` on `placement`.
fn local(placement: &Placement, at: DVec3) -> (f64, f64) {
    let offset = at - placement.origin;
    (offset.dot(placement.x), offset.dot(placement.y))
}

/// The bits of a placement, so equality is to the bit.
fn bits(placement: &Placement) -> [[u64; 3]; 4] {
    [placement.origin, placement.x, placement.y, placement.normal]
        .map(|v| v.to_array().map(f64::to_bits))
}

/// Where `evaluation` placed the sketch `feature`.
fn placed(evaluation: &Evaluation, feature: FeatureId) -> Placement {
    let found = (evaluation.placements.iter()).find(|(id, _)| *id == feature);
    found.unwrap_or_else(|| panic!("{feature:?} is placed")).1
}

/// The volumes agree within the rounding of a tilted frame.
fn assert_close(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-7 * b.abs().max(1.0), "{a} vs {b}");
}

/// The example plate with a sketch on its top: a disc of radius 3 at
/// `(20, 0)` joined 5 up, and a disc of radius 3 at `(-20, 0)` cut 3
/// down (from 1 above). The sketches' and the extrudes' ids.
fn boss_and_pocket_on_top() -> (Editor, [FeatureId; 4]) {
    let mut editor = Editor::new(Document::example());
    let plane = Plane::Face(top(editor.document()));
    let boss = add_sketch(&mut editor, plane, disc((20.0, 0.0), 3.0));
    let joined = add_extrude_of(&mut editor, boss, one_side("5"), join());
    let pocket = add_sketch(&mut editor, plane, disc((-20.0, 0.0), 3.0));
    let extent = two_sides(editor.document(), "1", "3");
    let cut = add_extrude_of(&mut editor, pocket, extent, cut());
    (editor, [boss, joined, pocket, cut])
}

/// A sketch on the plate's top is placed there, as XY is but 10 up, to
/// the bit; a boss joined from it and a pocket cut from it are where
/// the face is, and the sketch is drawn there.
#[test]
fn a_sketch_on_the_top_face_is_placed_on_it() {
    let (mut editor, [boss, _, pocket, _]) = boss_and_pocket_on_top();
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let up = Placement {
        origin: DVec3::new(0.0, 0.0, 10.0),
        ..OriginPlane::XY.placement()
    };
    assert_eq!(bits(&placed(&evaluation, boss)), bits(&up));
    assert_eq!(
        evaluation
            .placements
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>(),
        [boss, pocket]
    );
    let solid = only_body(&evaluation);
    assert_near(
        solid.volume(),
        plate(8.0, 10.0) + PI * 9.0 * 5.0 - PI * 9.0 * 3.0,
    );
    let bounds = solid.bounds3().unwrap();
    assert!((bounds.max.z - 15.0).abs() < 1e-9, "{bounds:?}");

    // Drawn on the face: every point of the boss's circle is 10 up.
    editor
        .apply(Command::SetFeatureVisible(boss, true))
        .unwrap();
    let lines = crate::flatten_sketches(editor.document(), &evaluation.placements, None).unwrap();
    assert!(!lines.points().is_empty());
    for point in lines.points() {
        assert_eq!(point[2], 10.0);
        assert!(
            (DVec2::new(point[0].into(), point[1].into()) - DVec2::new(20.0, 0.0)).length()
                < 3.0 + 1e-5
        );
    }
    // Not placed, it isn't drawn.
    assert_eq!(
        crate::flatten_sketches(editor.document(), &[], None),
        Ok(varde_kernel::RenderLines::default())
    );
}

/// Making the plate thicker moves its top, and the sketch on it with
/// what it made: the boss stands on the new top and the pocket goes 3
/// into it.
#[test]
fn a_sketch_on_a_face_follows_it_when_an_earlier_feature_changes() {
    let (mut editor, [boss, ..]) = boss_and_pocket_on_top();
    let feature = editor.document().features()[1].id;
    let extrude = Extrude {
        extent: one_side("12"),
        ..example_extrude(editor.document())
    };
    editor
        .apply(Command::SetFeature {
            feature,
            kind: Box::new(extrude.into()),
        })
        .unwrap();
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_eq!(placed(&evaluation, boss).origin, DVec3::new(0.0, 0.0, 12.0));
    let solid = only_body(&evaluation);
    assert_near(
        solid.volume(),
        plate(8.0, 12.0) + PI * 9.0 * 5.0 - PI * 9.0 * 3.0,
    );
    assert!((solid.bounds3().unwrap().max.z - 17.0).abs() < 1e-9);
}

/// A draft making the plate thicker is answered with the sketch on its
/// top following, drawn on the new top: drafts run the history in order
/// as commits do.
#[test]
fn a_draft_moves_the_sketches_on_its_faces() {
    let (mut editor, [boss, ..]) = boss_and_pocket_on_top();
    editor
        .apply(Command::SetFeatureVisible(boss, true))
        .unwrap();
    let feature = editor.document().features()[1].id;
    let extrude = Extrude {
        extent: one_side("12"),
        ..example_extrude(editor.document())
    };
    let draft = Draft {
        revision: 1,
        feature: Some(feature),
        kind: extrude.into(),
    };
    let answer = answered(crate::handle(regenerate_with(&editor, Some(draft))));
    assert_eq!(answer.draft.unwrap().error, None);
    let [(sketch, placement), _] = answer.placements[..] else {
        panic!("both sketches placed: {:?}", answer.placements);
    };
    assert_eq!(sketch, boss);
    assert_eq!(placement.origin.z, 12.0);
    assert!(!answer.sketches.points().is_empty());
    assert!(
        answer
            .sketches
            .points()
            .iter()
            .all(|point| point[2] == 12.0)
    );

    // Without it, the committed top.
    let answer = answered(crate::handle(regenerate_with(&editor, None)));
    assert_eq!(answer.placements[0].1.origin.z, 10.0);
    assert!(
        answer
            .sketches
            .points()
            .iter()
            .all(|point| point[2] == 10.0)
    );
}

/// The plate's right half cut down to a slope from `(10, 10)` to `(30,
/// 5)` in x and z, across its depth: the sloped face's outward normal
/// is `(1, 0, 4)` (unnormalized), and 2,000 mm³ are taken away. The
/// cut's id and the sloped face's reference, picked at its middle.
fn sloped() -> (Editor, FeatureId, FaceRef) {
    let mut editor = Editor::new(Document::example());
    let wedge = add_sketch(&mut editor, Plane::Origin(OriginPlane::XZ), |sketch| {
        let corners = [(5.0, 11.25), (35.0, 3.75), (35.0, 20.0), (5.0, 20.0)]
            .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
        for (k, &start) in corners.iter().enumerate() {
            let end = corners[(k + 1) % corners.len()];
            sketch.add_curve(Curve::Line { start, end }, false).unwrap();
        }
    });
    let extent = Extent::Symmetric(length(editor.document(), "50"));
    let cut = add_extrude_of(&mut editor, wedge, extent, cut());
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let solid = only_body(&evaluation);
    assert_near(solid.volume(), plate(8.0, 10.0) - 2000.0);
    let normal = DVec3::new(1.0, 0.0, 4.0);
    let key = key_where(solid, |form| plane_along(form, normal));
    assert_eq!(key.feature, cut.get());
    let face = FaceRef {
        body: editor.document().bodies()[0].id,
        key,
        near: DVec3::new(20.0, 0.0, 7.5),
    };
    (editor, cut, face)
}

/// End to end on a tilted face: a sketch on the slope is placed with
/// `y` up the slope and `x` along +Y; a boss joined square to the slope
/// and a hole cut into it have their analytic volumes, a pin from a
/// sketch on the hole's floor joined flush fills it again, and a pocket
/// from a sketch on the pin's end goes in square to the slope too.
#[test]
fn sketches_on_a_tilted_face_build_square_to_it() {
    let (mut editor, _, slope) = sloped();
    let sketch = add_sketch(&mut editor, Plane::Face(slope), |_| {});
    let evaluation = evaluated(editor.document());
    let placement = placed(&evaluation, sketch);
    let n = DVec3::new(1.0, 0.0, 4.0) / 17f64.sqrt();
    assert!((placement.normal - n).length() < 1e-15, "{placement:?}");
    assert!((placement.x - DVec3::Y).length() < 1e-15, "{placement:?}");
    let up = DVec3::new(-4.0, 0.0, 1.0) / 17f64.sqrt();
    assert!((placement.y - up).length() < 1e-15, "{placement:?}");
    // On the plane through (10, 0, 10), nearest the origin.
    assert!((placement.origin - n * n.dot(DVec3::new(10.0, 0.0, 10.0))).length() < 1e-12);
    assert!(placement.valid());

    // A boss of radius 3 at (25, −10) on the slope, 5 out; a hole of
    // radius 2 at (15, 10), 3 in (from 1 out).
    let boss_at = local(&placement, DVec3::new(25.0, -10.0, 6.25));
    let hole_at = local(&placement, DVec3::new(15.0, 10.0, 8.75));
    let mut drawn = Sketch::default();
    disc(boss_at, 3.0)(&mut drawn);
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn),
        })
        .unwrap();
    add_extrude_of(&mut editor, sketch, one_side("5"), join());
    let hole = add_sketch(&mut editor, Plane::Face(slope), disc(hole_at, 2.0));
    let extent = two_sides(editor.document(), "1", "3");
    let drilled = add_extrude_of(&mut editor, hole, extent, cut());
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let base = plate(8.0, 10.0) - 2000.0;
    assert_close(
        only_body(&evaluation).volume(),
        base + PI * 9.0 * 5.0 - PI * 4.0 * 3.0,
    );
    // Both placed alike, to the bit: one face, one rule.
    assert_eq!(bits(&placed(&evaluation, hole)), bits(&placement));

    // The hole's floor: the drill's cap left in the body, facing out.
    let solid = only_body(&evaluation);
    let floor_d = n.dot(placement.origin) - 3.0;
    let floor = key_where(solid, |form| {
        plane_along(form, n) && matches!(*form, Form::Plane { d, .. } if (d - floor_d).abs() < 1e-6)
    });
    assert_eq!(floor.feature, drilled.get());
    let floor_at = placement.to_world(DVec2::new(hole_at.0, hole_at.1)) - n * 3.0;
    let floor = FaceRef {
        body: slope.body,
        key: floor,
        near: floor_at,
    };
    let on_floor = add_sketch(&mut editor, Plane::Face(floor), |_| {});
    let floor_placement = placed(&evaluated(editor.document()), on_floor);
    assert!((floor_placement.normal - n).length() < 1e-15);
    let pin_at = local(&floor_placement, floor_at);
    let mut drawn = Sketch::default();
    disc(pin_at, 2.0)(&mut drawn);
    editor
        .apply(Command::SetSketch {
            feature: on_floor,
            sketch: Box::new(drawn),
        })
        .unwrap();
    let pin = add_extrude_of(&mut editor, on_floor, one_side("3"), join());
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_close(only_body(&evaluation).volume(), base + PI * 9.0 * 5.0);

    // A pocket of radius 1, 2 deep, from a sketch on the pin's end, which
    // is flush with the slope.
    let end = FaceRef {
        body: slope.body,
        key: FaceKey {
            feature: pin.get(),
            part: PartKey::EndCap,
            instance: 0,
        },
        near: floor_at + n * 3.0,
    };
    let on_end = add_sketch(&mut editor, Plane::Face(end), |_| {});
    let end_placement = placed(&evaluated(editor.document()), on_end);
    assert!((end_placement.normal - n).length() < 1e-15);
    assert!((end_placement.origin - placement.origin).length() < 1e-9);
    let pocket_at = local(&end_placement, floor_at + n * 3.0);
    let mut drawn = Sketch::default();
    disc(pocket_at, 1.0)(&mut drawn);
    editor
        .apply(Command::SetSketch {
            feature: on_end,
            sketch: Box::new(drawn),
        })
        .unwrap();
    let extent = two_sides(editor.document(), "1", "2");
    add_extrude_of(&mut editor, on_end, extent, cut());
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_close(
        only_body(&evaluation).volume(),
        base + PI * 9.0 * 5.0 - PI * 2.0,
    );
}

/// The app places a sketch on a face it picks itself, before any answer,
/// from the face's summary in the picking tables, as the page gets them
/// from the worker: [`Placement::on_plane`] on those bits gives the bits
/// regenerating then places the sketch at, on the tilted slope as on the
/// plate's top.
#[test]
fn a_picked_face_s_summary_places_a_sketch_as_regenerating_does() {
    let (mut editor, _, slope) = sloped();
    let top = top(editor.document());
    let on_slope = add_sketch(&mut editor, Plane::Face(slope), |_| {});
    let on_top = add_sketch(&mut editor, Plane::Face(top), |_| {});
    let response = crate::handle(regenerate_with(&editor, None));
    let (head, parts) = crate::wire::encode_reply(&response);
    let parts: Vec<&[u8]> = parts.iter().map(|part| &**part).collect();
    let crate::Response::Regenerated {
        mesh,
        picking,
        placements,
        ..
    } = crate::wire::decode_reply(&head[..], &parts).unwrap()
    else {
        panic!("regeneration failed");
    };
    for (sketch, face) in [(on_slope, slope), (on_top, top)] {
        let (_, picked) = (picking.faces().iter().enumerate())
            .find(|&(i, picked)| {
                picking.face_body(&mesh, i as u32) == Some(face.body) && picked.key == face.key
            })
            .unwrap_or_else(|| panic!("{face:?} is drawn"));
        let crate::picking::Summary::Plane { n, d } = picked.summary else {
            panic!("{face:?} is flat");
        };
        let from_pick = Placement::on_plane(n.into(), d).unwrap();
        let answered = (placements.iter()).find(|(id, _)| *id == sketch).unwrap();
        assert_eq!(bits(&from_pick), bits(&answered.1));
    }
}

/// A sketch on the top end of a revolved cylinder (the face its
/// rectangle's top line turns into) is placed there, and a boss joined
/// from it stands on it.
#[test]
fn a_sketch_on_a_revolve_s_flat_end_is_placed_on_it() {
    let mut editor = Editor::new(Document::default());
    let section = add_sketch(
        &mut editor,
        Plane::Origin(OriginPlane::XZ),
        rectangle((0.0, 0.0), (5.0, 10.0)),
    );
    let FeatureKind::Sketch { sketch: drawn, .. } =
        &editor.document().feature(section).unwrap().kind
    else {
        unreachable!()
    };
    let profiles = drawn.profiles().unwrap();
    let revolve = Revolve {
        sketch: section,
        regions: vec![profiles.reference(0).unwrap()],
        axis: AxisLine::SketchY,
        extent: Turn::Full,
        flip: false,
        operation: Operation::NewBody(BodyId::NEW),
    };
    editor
        .apply(editor.document().add_feature(revolve.into()))
        .unwrap();
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let key = key_where(only_body(&evaluation), |form| plane_along(form, DVec3::Z));
    let end = FaceRef {
        body: editor.document().bodies()[0].id,
        key,
        near: DVec3::new(1.0, 1.0, 10.0),
    };
    let sketch = add_sketch(&mut editor, Plane::Face(end), disc((1.0, 1.0), 2.0));
    add_extrude_of(&mut editor, sketch, one_side("3"), join());
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let up = Placement {
        origin: DVec3::new(0.0, 0.0, 10.0),
        ..OriginPlane::XY.placement()
    };
    assert_eq!(bits(&placed(&evaluation, sketch)), bits(&up));
    assert_close(
        only_body(&evaluation).volume(),
        PI * 25.0 * 10.0 + PI * 4.0 * 3.0,
    );
}

/// A sketch on a face of a body a join merged into another is placed on
/// the holder's solid, where the face lives on.
#[test]
fn a_sketch_on_a_consumed_body_s_face_follows_it_into_its_holder() {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let block = add_body(&mut editor, rectangle((40.0, -10.0), (60.0, 10.0)), "6");
    let block_maker = editor.document().features().last().unwrap().id;
    // A bridge from the plate to the block merges the block into it.
    add_extrude(
        &mut editor,
        rectangle((25.0, -5.0), (45.0, 5.0)),
        one_side("4"),
        join(),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(evaluation.merged, [(block, plate)]);
    // The block's top, 6 up.
    let face = FaceRef {
        body: block,
        key: FaceKey {
            feature: block_maker.get(),
            part: PartKey::EndCap,
            instance: 0,
        },
        near: DVec3::new(55.0, 0.0, 6.0),
    };
    let sketch = add_sketch(&mut editor, Plane::Face(face), disc((55.0, 5.0), 2.0));
    add_extrude_of(&mut editor, sketch, one_side("3"), join());
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_eq!(
        placed(&evaluation, sketch).origin,
        DVec3::new(0.0, 0.0, 6.0)
    );
    let merged = super::plate(8.0, 10.0) + 20.0 * 20.0 * 6.0 + 10.0 * 10.0 * 4.0;
    assert_close(
        solid_of(&evaluation, plate).volume(),
        merged + PI * 4.0 * 3.0,
    );
}

/// A sketch whose face a cut took away fails, "its face wasn't found",
/// and so does its extrude; it isn't drawn. Removing the face's body's
/// maker leaves it failing too, its body gone.
#[test]
fn a_sketch_whose_face_is_gone_fails() {
    let mut editor = Editor::new(Document::example());
    let face = top(editor.document());
    // The top 2 taken off the whole plate.
    let extent = two_sides(editor.document(), "5", "2");
    let skim = add_sketch(
        &mut editor,
        Plane::Face(face),
        rectangle((-40.0, -30.0), (40.0, 30.0)),
    );
    // Placed on the top before the skim cuts it away.
    add_extrude_of(&mut editor, skim, extent, cut());
    let sketch = add_sketch(&mut editor, Plane::Face(face), disc((20.0, 0.0), 3.0));
    let boss = add_extrude_of(&mut editor, sketch, one_side("5"), join());
    editor
        .apply(Command::SetFeatureVisible(sketch, true))
        .unwrap();
    let evaluation = evaluated(editor.document());
    assert_eq!(
        evaluation.failed,
        [
            (sketch, "its face wasn't found".to_owned()),
            (boss, "its sketch isn't placed".to_owned()),
        ]
    );
    assert_eq!(evaluation.placements.len(), 1);
    assert_near(evaluation.bodies[0].solid.volume(), plate(8.0, 8.0));
    let lines = crate::flatten_sketches(editor.document(), &evaluation.placements, None).unwrap();
    assert_eq!(lines, varde_kernel::RenderLines::default());

    // The plate's extrude removed: the body is gone, the sketches stay.
    let maker = editor.document().features()[1].id;
    editor.apply(Command::RemoveFeature(maker)).unwrap();
    let evaluation = evaluated(editor.document());
    assert!(evaluation.bodies.is_empty());
    assert_eq!(
        evaluation.failed,
        [
            (skim, "its face's body is gone".to_owned()),
            (
                editor.document().features()[2].id,
                "its sketch isn't placed".to_owned()
            ),
            (sketch, "its face's body is gone".to_owned()),
            (boss, "its sketch isn't placed".to_owned()),
        ]
    );
}

/// A sketch on the hole's wall, a cylinder, fails: "its face isn't
/// flat". Its profiles are still worked out.
#[test]
fn a_sketch_on_a_curved_face_fails() {
    let mut editor = Editor::new(Document::example());
    let evaluation = evaluated(editor.document());
    let key = key_where(only_body(&evaluation), |form| {
        matches!(form, Form::Cylinder { .. })
    });
    let wall = FaceRef {
        body: editor.document().bodies()[0].id,
        key,
        near: DVec3::new(8.0, 0.0, 5.0),
    };
    let sketch = add_sketch(&mut editor, Plane::Face(wall), disc((0.0, 0.0), 1.0));
    let mut cache = Cache::default();
    let evaluation = evaluate(editor.document(), &mut cache);
    assert_eq!(
        evaluation.failed,
        [(sketch, "its face isn't flat".to_owned())]
    );
    assert!(evaluation.placements.is_empty());
    // Both sketches' profiles, the plate, and the placement tried.
    assert_eq!(cache.counts(), (0, 4));
}

/// A placement is found again while the face's solid and the reference
/// don't change, whatever the sketch draws; the profiles are filed by the
/// sketch alone, so the same drawing on another plane finds them; and an
/// edit that moves the face works the placement out again, and the tool.
#[test]
fn placements_are_cached_by_the_solid_and_profiles_by_the_sketch() {
    let mut editor = Editor::new(Document::example());
    let face = top(editor.document());
    let sketch = add_sketch(&mut editor, Plane::Face(face), disc((20.0, 0.0), 3.0));
    add_extrude_of(&mut editor, sketch, one_side("5"), join());
    let mut cache = Cache::with_budget(0);
    cache.begin();
    let first = evaluate(editor.document(), &mut cache);
    assert!(first.failed.is_empty(), "{:?}", first.failed);
    // Two sketches' profiles, the plate, the placement, the boss's tool,
    // whether it touches the plate, and the union.
    assert_eq!(cache.counts(), (0, 7));
    cache.begin();
    let again = evaluate(editor.document(), &mut cache);
    assert_eq!(cache.counts(), (7, 7));
    assert_eq!(bits(&placed(&again, sketch)), bits(&placed(&first, sketch)));

    // Another drawing: its profiles, the tool, the touch and the union
    // again; the placement found.
    let mut drawn = Sketch::default();
    disc((20.0, 0.0), 4.0)(&mut drawn);
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn),
        })
        .unwrap();
    cache.begin();
    evaluate(editor.document(), &mut cache);
    assert_eq!(cache.counts(), (7 + 3, 7 + 4));

    // On XY, 10 lower: its profiles are found, the tool isn't (its frame
    // changed), and nothing is placed.
    editor
        .apply(Command::SetSketchPlane {
            feature: sketch,
            plane: Plane::Origin(OriginPlane::XY),
        })
        .unwrap();
    cache.begin();
    let on_xy = evaluate(editor.document(), &mut cache);
    assert!(on_xy.placements.is_empty());
    assert_eq!(cache.counts(), (10 + 3, 11 + 3));

    // Back on the top, the plate made thicker: the plate, the placement,
    // the tool, the touch and the union are worked out again.
    editor
        .apply(Command::SetSketchPlane {
            feature: sketch,
            plane: Plane::Face(top(editor.document())),
        })
        .unwrap();
    let feature = editor.document().features()[1].id;
    let extrude = Extrude {
        extent: one_side("12"),
        ..example_extrude(editor.document())
    };
    editor
        .apply(Command::SetFeature {
            feature,
            kind: Box::new(extrude.into()),
        })
        .unwrap();
    cache.begin();
    let thicker = evaluate(editor.document(), &mut cache);
    assert_eq!(placed(&thicker, sketch).origin.z, 12.0);
    assert_eq!(cache.counts(), (13 + 2, 14 + 5));
}

/// A sketch on a boss's top still resolves once the boss is made flush
/// with the plate: the boss's top merges into the plate's, which keeps
/// its key as an alias.
#[test]
fn a_sketch_on_a_boss_s_top_resolves_once_the_boss_is_flush() {
    let mut editor = Editor::new(Document::example());
    let boss = add_extrude(
        &mut editor,
        rectangle((15.0, -5.0), (25.0, 5.0)),
        one_side("15"),
        join(),
    );
    let face = FaceRef {
        body: editor.document().bodies()[0].id,
        key: FaceKey {
            feature: boss.get(),
            part: PartKey::EndCap,
            instance: 0,
        },
        near: DVec3::new(20.0, 0.0, 15.0),
    };
    let sketch = add_sketch(&mut editor, Plane::Face(face), disc((20.0, 0.0), 2.0));
    add_extrude_of(&mut editor, sketch, one_side("3"), join());
    let evaluation = evaluated(editor.document());
    assert_eq!(placed(&evaluation, sketch).origin.z, 15.0);
    assert_near(
        only_body(&evaluation).volume(),
        plate(8.0, 10.0) + 100.0 * 5.0 + PI * 4.0 * 3.0,
    );

    let FeatureKind::Extrude(extrude) = &editor.document().feature(boss).unwrap().kind else {
        unreachable!()
    };
    let flush = Extrude {
        extent: one_side("10"),
        ..extrude.clone()
    };
    editor
        .apply(Command::SetFeature {
            feature: boss,
            kind: Box::new(flush.into()),
        })
        .unwrap();
    let evaluation = evaluated(editor.document());
    let up = Placement {
        origin: DVec3::new(0.0, 0.0, 10.0),
        ..OriginPlane::XY.placement()
    };
    assert_eq!(bits(&placed(&evaluation, sketch)), bits(&up));
    assert_near(
        only_body(&evaluation).volume(),
        plate(8.0, 10.0) + PI * 4.0 * 3.0,
    );
}

/// A sketch on a wall of a hexagonal prism, facing 30° round from X:
/// `y` straight up, `x` along the wall, and a boss joined square to it.
#[test]
fn a_sketch_on_a_hexagonal_prism_s_wall_builds_square_to_it() {
    let mut editor = Editor::new(Document::default());
    let hexagon = add_sketch(&mut editor, Plane::Origin(OriginPlane::XY), |sketch| {
        let corners: Vec<_> = (0..6)
            .map(|k| {
                let angle = f64::from(k) * PI / 3.0;
                let at = DVec2::new(angle.cos(), angle.sin()) * 20.0;
                sketch.add_point(at).unwrap()
            })
            .collect();
        for (k, &start) in corners.iter().enumerate() {
            let end = corners[(k + 1) % corners.len()];
            sketch.add_curve(Curve::Line { start, end }, false).unwrap();
        }
    });
    let new_body = Operation::NewBody(BodyId::NEW);
    add_extrude_of(&mut editor, hexagon, one_side("30"), new_body);
    let evaluation = evaluated(editor.document());
    let n = DVec3::new(3f64.sqrt() / 2.0, 0.5, 0.0);
    let key = key_where(only_body(&evaluation), |form| plane_along(form, n));
    let middle = n * (20.0 * 3f64.sqrt() / 2.0) + DVec3::new(0.0, 0.0, 15.0);
    let wall = FaceRef {
        body: editor.document().bodies()[0].id,
        key,
        near: middle,
    };
    let sketch = add_sketch(&mut editor, Plane::Face(wall), |_| {});
    let placement = placed(&evaluated(editor.document()), sketch);
    assert_eq!(placement.y, DVec3::Z);
    assert!((placement.normal - n).length() < 1e-15, "{placement:?}");
    assert!((placement.x - DVec3::new(-0.5, 3f64.sqrt() / 2.0, 0.0)).length() < 1e-15);
    let mut drawn = Sketch::default();
    disc(local(&placement, middle), 3.0)(&mut drawn);
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn),
        })
        .unwrap();
    add_extrude_of(&mut editor, sketch, one_side("5"), join());
    let evaluation = evaluated(editor.document());
    let hexagon = 3.0 * 3f64.sqrt() / 2.0 * 400.0 * 30.0;
    assert_close(only_body(&evaluation).volume(), hexagon + PI * 9.0 * 5.0);
}

/// A revolve from a sketch on the plate's top turns about the sketch's
/// axis where the face is: a rectangle from `(0, 0)` to `(3, 2)` about
/// the sketch's y makes a cylinder of radius 3 about the line along Y
/// 10 up, standing in the plate's hole.
#[test]
fn a_revolve_from_a_sketch_on_a_face_turns_where_the_face_is() {
    let mut editor = Editor::new(Document::example());
    let face = top(editor.document());
    let sketch = add_sketch(
        &mut editor,
        Plane::Face(face),
        rectangle((0.0, 0.0), (3.0, 2.0)),
    );
    let FeatureKind::Sketch { sketch: drawn, .. } =
        &editor.document().feature(sketch).unwrap().kind
    else {
        unreachable!()
    };
    let revolve = Revolve {
        sketch,
        regions: vec![drawn.profiles().unwrap().reference(0).unwrap()],
        axis: AxisLine::SketchY,
        extent: Turn::Full,
        flip: false,
        operation: Operation::NewBody(BodyId::NEW),
    };
    editor
        .apply(editor.document().add_feature(revolve.into()))
        .unwrap();
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let turned = &evaluation.bodies[1].solid;
    assert_close(turned.volume(), PI * 9.0 * 2.0);
    let bounds = turned.bounds3().unwrap();
    assert!((bounds.min.z - 7.0).abs() < 1e-6 && (bounds.max.z - 13.0).abs() < 1e-6);
    assert!(bounds.min.y.abs() < 1e-9 && (bounds.max.y - 2.0).abs() < 1e-9);
}

/// Draws the closed polygon through `corners`.
fn polygon(corners: Vec<(f64, f64)>) -> impl FnOnce(&mut Sketch) {
    move |sketch| {
        let ids: Vec<_> = (corners.iter())
            .map(|&(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap())
            .collect();
        for (k, &start) in ids.iter().enumerate() {
            let end = ids[(k + 1) % ids.len()];
            sketch.add_curve(Curve::Line { start, end }, false).unwrap();
        }
    }
}

/// A block far out whose wall facing `(1, 0.3, 0)` is within the
/// coordinate limit, but whose plane's point nearest the origin isn't:
/// the sketch on it fails, "its face is too far out to sketch on", and so
/// does its extrude. Put on the block's top, it's placed and builds.
#[test]
fn a_sketch_on_a_face_whose_plane_passes_the_limit_fails() {
    let mut editor = Editor::new(Document::default());
    let n = DVec2::new(1.0, 0.3).normalize();
    let along = n.perp();
    let at = DVec2::new(9.9e5, 9.9e5);
    let corner = |a: f64, b: f64| {
        let p = at + along * a - n * b;
        (p.x, p.y)
    };
    // A 10 × 10 block with a wall along `along` through `at`.
    let block = polygon(vec![
        corner(-5.0, 0.0),
        corner(5.0, 0.0),
        corner(5.0, 10.0),
        corner(-5.0, 10.0),
    ]);
    add_body(&mut editor, block, "10");
    let evaluation = evaluated(editor.document());
    let solid = only_body(&evaluation);
    let wall = FaceRef {
        body: editor.document().bodies()[0].id,
        key: key_where(solid, |form| plane_along(form, n.extend(0.0))),
        near: at.extend(5.0),
    };
    let sketch = add_sketch(&mut editor, Plane::Face(wall), disc((0.0, 0.0), 1.0));
    let boss = add_extrude_of(&mut editor, sketch, one_side("2"), join());
    let evaluation = evaluated(editor.document());
    assert_eq!(
        evaluation.failed,
        [
            (sketch, "its face is too far out to sketch on".to_owned()),
            (boss, "its sketch isn't placed".to_owned()),
        ]
    );
    assert!(evaluation.placements.is_empty());
    // The rule's origin, n̂ (n̂·p), is past the limit along x.
    let origin = n * n.dot(at);
    assert!(origin.x > f64::from(varde_kernel::MAX_COORD), "{origin}");

    // Put on the block's top instead, it's placed and builds.
    let top = FaceRef {
        key: key_where(only_body(&evaluated(&prefix(editor.document()))), |form| {
            plane_along(form, DVec3::Z)
        }),
        near: at.extend(10.0),
        ..wall
    };
    editor
        .apply(Command::SetSketchPlane {
            feature: sketch,
            plane: Plane::Face(top),
        })
        .unwrap();
    let mut drawn = Sketch::default();
    disc((at.x - n.x * 5.0, at.y - n.y * 5.0), 1.0)(&mut drawn);
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn),
        })
        .unwrap();
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_close(only_body(&evaluation).volume(), 100.0 * 10.0 + PI * 2.0);
}

/// The document with only its first two features (the block).
fn prefix(document: &Document) -> Document {
    let mut editor = Editor::new(document.clone());
    while editor.document().features().len() > 2 {
        let last = editor.document().features().last().unwrap().id;
        editor.apply(Command::RemoveFeature(last)).unwrap();
    }
    editor.document().clone()
}

/// A chain of sketches on faces, far out on a wall tilted in XZ: a boss
/// joined square to the wall and a hole cut into the boss's end from a
/// sketch on it. Widening and lengthening the prism moves the wall, and
/// the boss and hole with it; undoing and redoing moves them back and
/// forth; the volumes stay analytic and a warm cache answers as a cold
/// one does, to the bit.
#[test]
fn a_chain_of_sketches_on_a_far_tilted_wall_follows_its_edits() {
    let centre = DVec2::new(-2.0e5, 3.0e5);
    let turn = 10f64.to_radians();
    let hexagon = |radius: f64| {
        polygon(
            (0..6)
                .map(|k| {
                    let angle = turn + f64::from(k) * PI / 3.0;
                    let at = centre + DVec2::new(angle.cos(), angle.sin()) * radius;
                    (at.x, at.y)
                })
                .collect(),
        )
    };
    let mut editor = Editor::new(Document::default());
    let base = add_sketch(&mut editor, Plane::Origin(OriginPlane::XZ), hexagon(20.0));
    let new_body = Operation::NewBody(BodyId::NEW);
    let prism = add_extrude_of(&mut editor, base, one_side("30"), new_body);
    let body = editor.document().bodies()[0].id;
    // The wall between the first two corners faces 40° round from X in
    // XZ; the prism runs along −Y.
    let facing = (turn + PI / 6.0).sin_cos();
    let n = DVec3::new(facing.1, 0.0, facing.0);
    let middle = |radius: f64| {
        let apothem = radius * (PI / 6.0).cos();
        DVec3::new(centre.x, -15.0, centre.y) + n * apothem
    };
    let solid = only_body(&evaluated(editor.document())).clone();
    let wall = FaceRef {
        body,
        key: key_where(&solid, |form| plane_along(form, n)),
        near: middle(20.0),
    };
    let on_wall = add_sketch(&mut editor, Plane::Face(wall), |_| {});
    let placement = placed(&evaluated(editor.document()), on_wall);
    let at = local(&placement, middle(20.0));
    let mut drawn = Sketch::default();
    disc(at, 3.0)(&mut drawn);
    editor
        .apply(Command::SetSketch {
            feature: on_wall,
            sketch: Box::new(drawn),
        })
        .unwrap();
    let boss = add_extrude_of(&mut editor, on_wall, one_side("5"), join());
    let end = FaceRef {
        body,
        key: FaceKey {
            feature: boss.get(),
            part: PartKey::EndCap,
            instance: 0,
        },
        near: middle(20.0) + n * 5.0,
    };
    let on_end = add_sketch(&mut editor, Plane::Face(end), disc(at, 1.5));
    let hole = add_extrude_of(&mut editor, on_end, one_side("2"), cut());
    let mut flipped = match &editor.document().feature(hole).unwrap().kind {
        FeatureKind::Extrude(extrude) => extrude.clone(),
        _ => unreachable!(),
    };
    flipped.flip = true;
    editor
        .apply(Command::SetFeature {
            feature: hole,
            kind: Box::new(flipped.into()),
        })
        .unwrap();

    let mut cache = Cache::default();
    let mut check = |editor: &Editor, radius: f64, height: f64| {
        let warm = evaluate(editor.document(), &mut cache);
        let cold = evaluated(editor.document());
        assert_eq!(warm.failed, cold.failed);
        assert_eq!(
            warm.placements
                .iter()
                .map(|(id, p)| (*id, bits(p)))
                .collect::<Vec<_>>(),
            cold.placements
                .iter()
                .map(|(id, p)| (*id, bits(p)))
                .collect::<Vec<_>>(),
        );
        let volume = only_body(&warm).volume();
        assert_eq!(volume.to_bits(), only_body(&cold).volume().to_bits());
        let hexagon = 3.0 * 3f64.sqrt() / 2.0 * radius * radius * height;
        let expected = hexagon + PI * 9.0 * 5.0 - PI * 2.25 * 2.0;
        assert!(
            (volume - expected).abs() < 1e-6 * expected,
            "{volume} vs {expected}"
        );
        // The wall's sketch on the wall's plane, the end's 5 out from it,
        // both square to it.
        let wall = placed(&warm, on_wall);
        let end = placed(&warm, on_end);
        let offset = |p: &Placement, d: f64| (middle(radius) + n * d - p.origin).dot(p.normal);
        // Within the rounding of corners 3e5 out.
        assert!(offset(&wall, 0.0).abs() < 1e-9, "{wall:?}");
        assert!(offset(&end, 5.0).abs() < 1e-9, "{end:?}");
        for p in [wall, end] {
            assert!((p.normal - n).length() < 1e-10, "{p:?}");
            assert!(p.valid());
        }
    };
    check(&editor, 20.0, 30.0);

    // Wider (its corners moved, its curves kept) and longer.
    let FeatureKind::Sketch { sketch: drawn, .. } = &editor.document().feature(base).unwrap().kind
    else {
        unreachable!()
    };
    let mut wider = drawn.clone();
    let mut fresh = Sketch::default();
    hexagon(24.0)(&mut fresh);
    for (point, moved) in wider.points.iter_mut().zip(&fresh.points) {
        point.at = moved.at;
    }
    editor
        .apply(Command::SetSketch {
            feature: base,
            sketch: Box::new(wider),
        })
        .unwrap();
    let extrude = match &editor.document().feature(prism).unwrap().kind {
        FeatureKind::Extrude(extrude) => Extrude {
            extent: one_side("36"),
            ..extrude.clone()
        },
        _ => unreachable!(),
    };
    editor
        .apply(Command::SetFeature {
            feature: prism,
            kind: Box::new(extrude.into()),
        })
        .unwrap();
    // The wall's middle stays 15 along the prism's length; the prism
    // is longer, so the boss is off its middle but on it.
    check(&editor, 24.0, 36.0);
    editor.undo();
    editor.undo();
    check(&editor, 20.0, 30.0);
    editor.redo();
    editor.redo();
    check(&editor, 24.0, 36.0);
}
