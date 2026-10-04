//! Offset faces in the history. The kernel's offset face isn't built
//! yet (it fails as too complex), so: an offset reaching the kernel
//! fails with the too-complex message, changing no body, and the rest
//! of the history goes on; regen's own refusal comes first (a face not
//! found, after the feature that made it is removed). With the kernel's
//! offset replaced by a stand-in moving a box's faces (a scale along
//! each axis and a move, every face keeping its name): faces found again
//! after an upstream dimension change, a sketch on a moved face
//! following it, out and in by their volumes, a face named twice handed
//! over once, past the opposite face refused, other refusals worded and
//! drawn, the cache. The analytic volumes of the kernel's offset face
//! are written out, ignored until it's built.

use std::cell::RefCell;

use glam::DVec3;
use varde_document::{FaceRef, OffsetFace, Placement};
use varde_kernel::mesh::{FaceKey, Form};
use varde_kernel::{OffsetError, Topology};

use super::motion::{add, block, cylinder, failure, key_on, key_where, near_box, polygon, set};
use super::*;

/// Offsets faces on this test's thread by the box stand-in.
fn with_boxes() {
    super::super::offset_face::offset_by_boxes();
}

/// A distance of `text` for an offset in `document`.
fn distance(document: &Document, text: &str) -> Value {
    Value::new(text, &OffsetFace::distance_ask(&document.design())).unwrap()
}

/// The face of `body` keyed `key`, picked at `near`.
fn face(body: BodyId, key: FaceKey, near: [f64; 3]) -> FaceRef {
    FaceRef {
        body,
        key,
        near: DVec3::from(near),
    }
}

/// `faces` moved `size` out (`inward`: in), tangent faces taken in.
fn offset(document: &Document, mut faces: Vec<FaceRef>, size: &str, inward: bool) -> OffsetFace {
    faces.sort_by(FaceRef::order);
    OffsetFace {
        faces,
        distance: distance(document, size),
        inward,
        tangent: true,
    }
}

fn evaluated(document: &Document) -> Evaluation {
    evaluate(document, &mut Cache::default())
}

/// The 10 mm cube from the origin: the editor, its body and the solid
/// regenerated.
fn cube() -> (Editor, BodyId, Arc<Solid>) {
    let mut editor = Editor::new(Document::default());
    let body = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let made = evaluated(editor.document());
    let solid = Arc::clone(&made.bodies[0].solid);
    (editor, body, solid)
}

/// The cube's top face.
fn top(solid: &Solid, body: BodyId) -> FaceRef {
    face(body, key_on(solid, DVec3::Z, 10.0), [5.0, 5.0, 10.0])
}

/// The cube's bottom face, at z = 0.
fn bottom(solid: &Solid, body: BodyId) -> FaceRef {
    face(body, key_on(solid, -DVec3::Z, 0.0), [5.0, 5.0, 0.0])
}

/// The cube's front face, at y = 0.
fn front(solid: &Solid, body: BodyId) -> FaceRef {
    face(body, key_on(solid, -DVec3::Y, 0.0), [5.0, 0.0, 5.0])
}

/// What the kernel's offset face says of `body` today.
fn too_complex(body: &str) -> String {
    format!("offsetting faces of {body} is too complex to work out")
}

fn assert_near(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-9 * b.abs().max(1.0), "{a} vs {b}");
}

/// The volumes agree within the kernel's fitting of curved faces.
fn assert_close(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-6 * b.abs().max(1.0), "{a} vs {b}");
}

/// The volume of `body` in `editor`'s document regenerated, the offset
/// `id` not failing.
fn volume(editor: &Editor, id: FeatureId, body: BodyId) -> f64 {
    let evaluation = evaluated(editor.document());
    assert!(
        failure(&evaluation, id).is_none(),
        "{:?}",
        evaluation.failed
    );
    solid_of(&evaluation, body).volume()
}

/// An offset the kernel can't do yet fails as too complex, leaving the
/// body as it was; the history goes on: a later join still works on the
/// body.
#[test]
fn an_offset_the_kernel_cant_do_fails_as_too_complex() {
    let (mut editor, body, solid) = cube();
    let kind = offset(editor.document(), vec![top(&solid, body)], "1", false);
    let id = add(&mut editor, kind);
    let extent = two_sides(editor.document(), "12", "1");
    let join = add_extrude(
        &mut editor,
        rectangle((8.0, 8.0), (12.0, 12.0)),
        extent,
        Operation::Join(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the offset fails");
    assert_eq!(failed.message, too_complex("Body 1"));
    assert!(failure(&evaluation, join).is_none());
    assert_eq!(evaluation.failed.len(), 1);
    assert_near(
        solid_of(&evaluation, body).volume(),
        1000.0 + 16.0 * 13.0 - 40.0,
    );
    // Inward too.
    let inward = offset(editor.document(), vec![top(&solid, body)], "1", true);
    set(&mut editor, id, inward);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        too_complex("Body 1")
    );
}

/// A face whose maker is removed (the cut that made a notch's wall; the
/// offset stays, as it names its body only) isn't found: the offset
/// fails before the kernel, the body left as it was; among others, it's
/// named by its place.
#[test]
fn a_face_that_is_gone_is_not_found() {
    let (mut editor, body, solid) = cube();
    let extent = two_sides(editor.document(), "11", "1");
    let notch = add_extrude(
        &mut editor,
        rectangle((-1.0, -1.0), (3.0, 3.0)),
        extent,
        Operation::Cut(Targets::default()),
    );
    let notched = evaluated(editor.document());
    let notched = solid_of(&notched, body);
    let wall = key_where(
        notched,
        |form| matches!(*form, Form::Plane { n, d } if n.x < -0.5 && (d + 3.0).abs() < 1e-9),
    );
    let at = face(body, wall, [3.0, 1.5, 5.0]);
    let kind = offset(editor.document(), vec![at], "1", false);
    let id = add(&mut editor, kind);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        too_complex("Body 1")
    );
    editor.apply(Command::RemoveFeature(notch)).unwrap();
    assert!(editor.document().feature(id).is_some());
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the offset fails");
    assert_eq!(failed.message, "its face wasn't found");
    assert!(failed.geometry.is_none());
    assert_near(solid_of(&evaluation, body).volume(), 1000.0);
    let kind = offset(editor.document(), vec![at, top(&solid, body)], "1", false);
    let place = kind.faces.iter().position(|f| *f == at).unwrap();
    set(&mut editor, id, kind);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        format!("its face {} of 2 wasn't found", place + 1)
    );
}

/// An upstream dimension change moves the face; it's found again by its
/// name, and moved from where it went.
#[test]
fn an_offset_face_is_found_again_after_an_upstream_change() {
    let (mut editor, body, solid) = cube();
    let kind = offset(editor.document(), vec![top(&solid, body)], "2", false);
    let id = add(&mut editor, kind);
    let maker = editor.document().body(body).unwrap().created_by;
    let twenty = Extent::OneSide(length(editor.document(), "20"));
    set_extrude(&mut editor, maker, |extrude| extrude.extent = twenty);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        too_complex("Body 1")
    );
    // The stand-in's: the top, now at 20, moved to 22.
    with_boxes();
    assert_near(volume(&editor, id, body), 100.0 * 22.0);
    let evaluation = evaluated(editor.document());
    let bounds = solid_of(&evaluation, body).bounds3().unwrap();
    assert_near(bounds.max.z, 22.0);
}

/// The world corner `at`, on `placement`'s plane, in its sketch's
/// coordinates.
fn local(placement: &Placement, at: DVec3) -> (f64, f64) {
    let offset = at - placement.origin;
    assert!(
        offset.dot(placement.normal).abs() < 1e-9,
        "{at} on the plane"
    );
    (offset.dot(placement.x), offset.dot(placement.y))
}

/// A sketch on the face an offset moves, after it, is placed on the face
/// where the offset left it, and follows it when the distance or an
/// upstream dimension changes: a block joined on it stands on the moved
/// top.
#[test]
fn a_sketch_on_a_moved_face_follows_it() {
    with_boxes();
    let (mut editor, body, solid) = cube();
    let kind = offset(editor.document(), vec![top(&solid, body)], "2", false);
    let id = add(&mut editor, kind);
    editor
        .apply(editor.document().add_sketch(Plane::Face(top(&solid, body))))
        .unwrap();
    let sketch = editor.document().features().last().unwrap().id;
    let placed = |editor: &Editor| {
        let evaluation = evaluated(editor.document());
        let found = (evaluation.placements.iter()).find(|(feature, _)| *feature == sketch);
        found.expect("the sketch is placed").1
    };
    let placement = placed(&editor);
    assert_near(placement.origin.z, 12.0);
    assert!(placement.normal.abs_diff_eq(DVec3::Z, 1e-12));
    // A 2 × 2 block on it, 3 up.
    let (a, b) = (
        local(&placement, DVec3::new(4.0, 4.0, 12.0)),
        local(&placement, DVec3::new(6.0, 6.0, 12.0)),
    );
    let mut drawn = Sketch::default();
    rectangle((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)))(&mut drawn);
    let profiles = drawn.profiles().unwrap();
    let regions = vec![profiles.reference(0).unwrap()];
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn),
        })
        .unwrap();
    let join = Extrude {
        sketch,
        regions,
        extent: Extent::OneSide(length(editor.document(), "3")),
        flip: false,
        operation: Operation::Join(Targets::default()),
    };
    add(&mut editor, join);
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_near(solid_of(&evaluation, body).volume(), 1200.0 + 12.0);
    // Inward instead: the sketch goes down with the top.
    let inward = offset(editor.document(), vec![top(&solid, body)], "2", true);
    set(&mut editor, id, inward);
    assert_near(placed(&editor).origin.z, 8.0);
    assert_near(volume(&editor, id, body), 800.0 + 12.0);
    // The cube made 20 high: the top at 18.
    let maker = editor.document().body(body).unwrap().created_by;
    let twenty = Extent::OneSide(length(editor.document(), "20"));
    set_extrude(&mut editor, maker, |extrude| extrude.extent = twenty);
    assert_near(placed(&editor).origin.z, 18.0);
    assert_near(volume(&editor, id, body), 1800.0 + 12.0);
}

/// Through the stand-in: out and in by their volumes, two faces, two
/// opposite faces, every face.
#[test]
fn offsets_move_faces_by_their_volumes() {
    with_boxes();
    let (mut editor, body, solid) = cube();
    let mm = editor.document().clone();
    let id = add(
        &mut editor,
        offset(&mm, vec![top(&solid, body)], "2", false),
    );
    assert_near(volume(&editor, id, body), 1200.0);
    set(
        &mut editor,
        id,
        offset(&mm, vec![top(&solid, body)], "2", true),
    );
    assert_near(volume(&editor, id, body), 800.0);
    let two = vec![top(&solid, body), front(&solid, body)];
    set(&mut editor, id, offset(&mm, two, "1", false));
    assert_near(volume(&editor, id, body), 10.0 * 11.0 * 11.0);
    let opposite = vec![top(&solid, body), bottom(&solid, body)];
    set(&mut editor, id, offset(&mm, opposite, "1", true));
    assert_near(volume(&editor, id, body), 800.0);
    let evaluation = evaluated(editor.document());
    assert!(near_box(
        solid_of(&evaluation, body),
        [0.0, 0.0, 1.0],
        [10.0, 10.0, 9.0]
    ));
    // Every face: a uniform offset.
    let topology = solid.topology();
    let every: Vec<FaceRef> = (topology.regions().iter())
        .map(|region| {
            let near = solid.mesh().verts()[0];
            face(body, region.key, near.into())
        })
        .collect();
    set(&mut editor, id, offset(&mm, every, "1", false));
    assert_near(volume(&editor, id, body), 12.0 * 12.0 * 12.0);
}

/// A face moved onto or past the face opposite it is refused (the walls
/// between would shrink to nothing), the face drawn, the body left
/// whole.
#[test]
fn past_the_opposite_face_is_refused() {
    with_boxes();
    let (mut editor, body, solid) = cube();
    let mm = editor.document().clone();
    let id = add(
        &mut editor,
        offset(&mm, vec![top(&solid, body)], "10", true),
    );
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the offset fails");
    assert_eq!(
        failed.message,
        "the face moves past a neighbouring face of Body 1: try a smaller distance"
    );
    assert!(failed.geometry.is_some());
    assert_near(solid_of(&evaluation, body).volume(), 1000.0);
    set(
        &mut editor,
        id,
        offset(&mm, vec![top(&solid, body)], "9.5", true),
    );
    assert_near(volume(&editor, id, body), 50.0);
    // Two opposite faces meeting in the middle.
    let opposite = vec![top(&solid, body), bottom(&solid, body)];
    set(&mut editor, id, offset(&mm, opposite, "5", true));
    let evaluation = evaluated(editor.document());
    assert!(failure(&evaluation, id).is_some());
}

thread_local! {
    /// What the recording stand-in was handed: the regions, the
    /// distance and whether tangent faces are taken in.
    static HANDED: RefCell<Vec<(Vec<u32>, f64, bool)>> = const { RefCell::new(Vec::new()) };
}

/// A stand-in that records what it's handed, and fails.
#[allow(clippy::too_many_arguments)]
fn recording(
    _: &Solid,
    _: &Topology,
    faces: &[u32],
    distance: f64,
    tangent: bool,
    _: u64,
    _: &Tolerance,
    _: &Budget,
) -> Result<Solid, OffsetError> {
    HANDED.with_borrow_mut(|handed| handed.push((faces.to_vec(), distance, tangent)));
    Err(OffsetError::IntoBody)
}

/// Two references to one face (picked at two points) hand the kernel
/// that face once; the distance in model units, negative inward, and
/// the tangent flag as stored.
#[test]
fn a_face_named_twice_is_moved_once() {
    super::super::offset_face::OFFSETTER.set(Some(recording));
    let (mut editor, body, solid) = cube();
    let mm = editor.document().clone();
    let mut twice = top(&solid, body);
    twice.near = DVec3::new(1.0, 1.0, 10.0);
    let kind = OffsetFace {
        tangent: false,
        ..offset(
            &mm,
            vec![top(&solid, body), twice, front(&solid, body)],
            "2.5",
            true,
        )
    };
    let id = add(&mut editor, kind);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        "the face runs into another part of Body 1: try a smaller distance"
    );
    let topology = solid.topology();
    let region = |key: FaceKey| {
        (topology.regions().iter())
            .position(|region| region.key == key)
            .unwrap() as u32
    };
    let mut wanted = vec![
        region(top(&solid, body).key),
        region(front(&solid, body).key),
    ];
    wanted.sort();
    let handed = HANDED.with_borrow(Clone::clone);
    assert_eq!(handed, [(wanted, -2.5, false)]);
}

/// A stand-in failing with what the thread's [`REFUSAL`] says.
#[allow(clippy::too_many_arguments)]
fn refusing(
    _: &Solid,
    _: &Topology,
    _: &[u32],
    _: f64,
    _: bool,
    _: u64,
    _: &Tolerance,
    _: &Budget,
) -> Result<Solid, OffsetError> {
    Err(REFUSAL.with_borrow(Clone::clone))
}

thread_local! {
    /// What [`refusing`] fails with.
    static REFUSAL: RefCell<OffsetError> = const { RefCell::new(OffsetError::IntoBody) };
}

/// The kernel's refusals are worded for the Timeline, with the face or
/// corner they're about drawn.
#[test]
fn refusals_are_worded_and_drawn() {
    let (mut editor, body, solid) = cube();
    let mm = editor.document().clone();
    let id = add(
        &mut editor,
        offset(&mm, vec![top(&solid, body)], "1", false),
    );
    super::super::offset_face::OFFSETTER.set(Some(refusing));
    for (error, words, drawn) in [
        (
            OffsetError::PastNeighbour { region: 0 },
            "the face moves past a neighbouring face of Body 1: try a smaller distance",
            true,
        ),
        (
            OffsetError::IntoBody,
            "the face runs into another part of Body 1: try a smaller distance",
            false,
        ),
        (
            OffsetError::RoundTooSmall { region: 0 },
            "a round face of Body 1 shrinks to nothing: try a smaller distance",
            true,
        ),
        (
            OffsetError::NoSurface { region: 1 },
            "a face of Body 1 next to it has no surface to extend",
            true,
        ),
        (
            OffsetError::TangentNeighbour { region: 2 },
            "it is tangent to a face of Body 1 that isn't picked: pick it too, or turn on Tangent faces",
            true,
        ),
        (
            OffsetError::Corner { vertex: 0 },
            "faces of Body 1 meeting at a corner can't be offset together: try another distance",
            true,
        ),
        (
            OffsetError::OutOfRange,
            "it moves Body 1 out of range",
            false,
        ),
    ] {
        REFUSAL.set(error);
        let evaluation = evaluated(editor.document());
        let failed = failure(&evaluation, id).expect("the offset fails");
        assert_eq!(failed.message, words);
        assert_eq!(failed.geometry.is_some(), drawn, "{words}");
        assert_near(solid_of(&evaluation, body).volume(), 1000.0);
    }
}

/// An offset that worked is found in the cache the next time; another
/// distance isn't.
#[test]
fn an_offset_that_worked_is_cached() {
    with_boxes();
    let (mut editor, body, solid) = cube();
    let kind = offset(editor.document(), vec![top(&solid, body)], "1", false);
    let id = add(&mut editor, kind);
    let mut cache = Cache::default();
    let first = evaluate(editor.document(), &mut cache);
    let again = evaluate(editor.document(), &mut cache);
    assert!(Arc::ptr_eq(&first.bodies[0].solid, &again.bodies[0].solid));
    assert_near(first.bodies[0].solid.volume(), 1100.0);
    let inward = offset(editor.document(), vec![top(&solid, body)], "1", true);
    set(&mut editor, id, inward);
    let other = evaluate(editor.document(), &mut cache);
    assert_near(other.bodies[0].solid.volume(), 900.0);
}

/// An offset of a body a combine took into another fails, naming where
/// it went, as every feature naming a consumed body does.
#[test]
fn a_consumed_body_fails_an_offset() {
    let (mut editor, body, _) = cube();
    let other = block(&mut editor, 5.0, 5.0, 15.0, 15.0, "10");
    let evaluation = evaluated(editor.document());
    let other_top = face(
        other,
        key_on(solid_of(&evaluation, other), DVec3::Z, 10.0),
        [10.0, 10.0, 10.0],
    );
    let combine = varde_document::Combine {
        target: body,
        tools: vec![other],
        op: varde_document::BodyOp::Union,
        keep_tools: false,
    };
    add(&mut editor, combine);
    let mm = editor.document().clone();
    let id = add(&mut editor, offset(&mm, vec![other_top], "1", false));
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the offset fails");
    assert_eq!(
        failed.message,
        "Body 2 is in Body 1 now: a feature before this one merged it in"
    );
}

/// The stand-in against the volumes it should give, on a 10 × 6 × 4
/// box: every set of faces moved, out and in, at distances about half a
/// side and past one. Each that works has its volume and keeps every
/// face's name; one whose walls would shrink to nothing fails, never
/// leaving the body as it was.
#[test]
fn the_stand_in_moves_boxes_by_their_volumes() {
    with_boxes();
    let mut editor = Editor::new(Document::default());
    let body = block(&mut editor, 0.0, 0.0, 10.0, 6.0, "4");
    let evaluation = evaluated(editor.document());
    let solid = solid_of(&evaluation, body).clone();
    let keys: Vec<FaceKey> = (solid.topology().regions().iter())
        .map(|region| region.key)
        .collect();
    let size = [10.0, 6.0, 4.0];
    let mut sides = Vec::new();
    for axis in 0..3 {
        for end in 0..2 {
            let mut n = DVec3::ZERO;
            n[axis] = if end == 0 { -1.0 } else { 1.0 };
            let d = if end == 0 { 0.0 } else { size[axis] };
            let mut near = DVec3::from(size) / 2.0;
            near[axis] = d;
            sides.push((axis, face(body, key_on(&solid, n, d), near.into())));
        }
    }
    let mm = editor.document().clone();
    let id = add(&mut editor, offset(&mm, vec![sides[0].1], "1", false));
    for mask in 1..64u32 {
        let picked: Vec<FaceRef> = (0..6)
            .filter(|k| mask & (1 << k) != 0)
            .map(|k| sides[k].1)
            .collect();
        let mut moved = [0.0f64; 3];
        for k in (0..6).filter(|k| mask & (1 << k) != 0) {
            moved[sides[k].0] += 1.0;
        }
        for text in ["0.5", "1.9999999", "2", "2.0000001", "3", "4", "6", "10"] {
            for inward in [false, true] {
                let d: f64 = text.parse().unwrap();
                let signed = if inward { -d } else { d };
                set(&mut editor, id, offset(&mm, picked.clone(), text, inward));
                let evaluation = evaluated(editor.document());
                let sizes: Vec<f64> = (0..3).map(|a| size[a] + signed * moved[a]).collect();
                let slack = sizes.iter().copied().fold(f64::INFINITY, f64::min);
                let what = format!("faces {mask:06b} {text} inward {inward}");
                match failure(&evaluation, id) {
                    None => {
                        let got = solid_of(&evaluation, body);
                        assert!(slack > 0.0, "{what}: worked");
                        let wanted: f64 = sizes.iter().product();
                        assert!(
                            (got.volume() - wanted).abs() <= 1e-9 * wanted.max(1.0),
                            "{what}: {} vs {wanted}",
                            got.volume()
                        );
                        let now: Vec<FaceKey> = (got.topology().regions().iter())
                            .map(|region| region.key)
                            .collect();
                        assert_eq!(now, keys, "{what}: names kept");
                    }
                    Some(failed) => {
                        assert!(slack <= 1e-6, "{what}: {}", failed.message);
                        assert_near(solid_of(&evaluation, body).volume(), 240.0);
                    }
                }
            }
        }
    }
}

// The kernel's offset face, analytically: ignored until it's built.

/// A box's top out and in; every face out (a uniform offset, sharp
/// corners); names kept.
#[test]
#[ignore = "kernel offset face not built"]
fn the_kernel_moves_a_box_s_faces() {
    let (mut editor, body, solid) = cube();
    let mm = editor.document().clone();
    let id = add(
        &mut editor,
        offset(&mm, vec![top(&solid, body)], "2", false),
    );
    assert_near(volume(&editor, id, body), 1200.0);
    set(
        &mut editor,
        id,
        offset(&mm, vec![top(&solid, body)], "2", true),
    );
    assert_near(volume(&editor, id, body), 800.0);
    let every: Vec<FaceRef> = (solid.topology().regions().iter())
        .map(|region| face(body, region.key, solid.mesh().verts()[0].into()))
        .collect();
    set(&mut editor, id, offset(&mm, every, "1", false));
    assert_near(volume(&editor, id, body), 12.0 * 12.0 * 12.0);
    let evaluation = evaluated(editor.document());
    let mut before: Vec<FaceKey> = (solid.topology().regions().iter())
        .map(|region| region.key)
        .collect();
    let mut after: Vec<FaceKey> = (solid_of(&evaluation, body).topology().regions().iter())
        .map(|region| region.key)
        .collect();
    before.sort();
    after.sort();
    assert_eq!(before, after);
}

/// A regular hexagonal prism (side 10, 10 high) with one wall pushed out
/// by 2: its neighbours, at 120° to it, extended, so the strip added is
/// a trapezoid `2·(10 + 2/√3)` across, 10 high.
#[test]
#[ignore = "kernel offset face not built"]
fn the_kernel_moves_a_prism_s_slanted_wall() {
    let h = 5.0 * 3f64.sqrt();
    static HEX: std::sync::OnceLock<[(f64, f64); 6]> = std::sync::OnceLock::new();
    let points = HEX.get_or_init(|| {
        let h = 5.0 * 3f64.sqrt();
        [
            (-5.0, -h),
            (5.0, -h),
            (10.0, 0.0),
            (5.0, h),
            (-5.0, h),
            (-10.0, 0.0),
        ]
    });
    let mut editor = Editor::new(Document::default());
    let extent = Extent::OneSide(length(editor.document(), "10"));
    add_extrude(
        &mut editor,
        polygon(points),
        extent,
        Operation::NewBody(BodyId::NEW),
    );
    let body = editor.document().bodies()[0].id;
    let evaluation = evaluated(editor.document());
    let solid = solid_of(&evaluation, body).clone();
    let wall = face(body, key_on(&solid, -DVec3::Y, h), [0.0, -h, 5.0]);
    let mm = editor.document().clone();
    let id = add(&mut editor, offset(&mm, vec![wall], "2", false));
    let hexagon = 1.5 * 3f64.sqrt() * 100.0;
    let strip = 2.0 * (10.0 + 2.0 / 3f64.sqrt());
    assert_close(volume(&editor, id, body), (hexagon + strip) * 10.0);
}

/// A 20 mm square plate 5 thick with a boss of radius 3, 5 high: the
/// boss's top down and up by 2 (its wall extended or trimmed), then its
/// wall out by 1 (an exact cylinder of radius 4).
#[test]
#[ignore = "kernel offset face not built"]
fn the_kernel_moves_a_boss_s_top_and_wall() {
    let mut editor = Editor::new(Document::default());
    let body = block(&mut editor, 0.0, 0.0, 20.0, 20.0, "5");
    let extent = two_sides(editor.document(), "10", "1");
    add_extrude(
        &mut editor,
        disc((10.0, 10.0), 3.0),
        extent,
        Operation::Join(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    let solid = solid_of(&evaluation, body).clone();
    let boss_top = face(body, key_on(&solid, DVec3::Z, 10.0), [10.0, 10.0, 10.0]);
    let wall = face(body, cylinder(&solid), [13.0, 10.0, 7.0]);
    let mm = editor.document().clone();
    let plate = 2000.0;
    let id = add(&mut editor, offset(&mm, vec![boss_top], "2", false));
    assert_close(volume(&editor, id, body), plate + 9.0 * PI * 7.0);
    set(&mut editor, id, offset(&mm, vec![boss_top], "2", true));
    assert_close(volume(&editor, id, body), plate + 9.0 * PI * 3.0);
    set(&mut editor, id, offset(&mm, vec![wall], "1", false));
    assert_close(volume(&editor, id, body), plate + 16.0 * PI * 5.0);
}

/// A plate 20 square and 5 thick with a hole of radius 3 through it:
/// the hole's wall moved out of the body by 1 (into the hole: radius 2),
/// then the plate's top up by 1 with the hole's wall extended; the top
/// pulled in until the hole would close isn't the refusal here, but a
/// hole's wall moved by its radius is: the round shrinks to nothing.
#[test]
#[ignore = "kernel offset face not built"]
fn the_kernel_moves_a_hole_s_wall_and_a_plate_s_top() {
    let mut editor = Editor::new(Document::default());
    let body = block(&mut editor, 0.0, 0.0, 20.0, 20.0, "5");
    let extent = two_sides(editor.document(), "6", "1");
    add_extrude(
        &mut editor,
        disc((10.0, 10.0), 3.0),
        extent,
        Operation::Cut(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    let solid = solid_of(&evaluation, body).clone();
    let wall = face(body, cylinder(&solid), [13.0, 10.0, 2.5]);
    let plate_top = face(body, key_on(&solid, DVec3::Z, 5.0), [2.0, 2.0, 5.0]);
    let mm = editor.document().clone();
    let id = add(&mut editor, offset(&mm, vec![wall], "1", false));
    assert_close(volume(&editor, id, body), (400.0 - 4.0 * PI) * 5.0);
    set(&mut editor, id, offset(&mm, vec![plate_top], "1", false));
    assert_close(volume(&editor, id, body), (400.0 - 9.0 * PI) * 6.0);
    set(&mut editor, id, offset(&mm, vec![wall], "3", false));
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        "a round face of Body 1 shrinks to nothing: try a smaller distance"
    );
}

/// A square with a corner chamfered (from `(10, 8)` to `(8, 10)`): its
/// right wall pushed in by 3 would squeeze the chamfer to nothing, past
/// a neighbouring face; a plate's side pulled into a hole through it
/// runs into another part of the body.
#[test]
#[ignore = "kernel offset face not built"]
fn the_kernel_refuses_past_a_neighbour_and_into_the_body() {
    static CUT: [(f64, f64); 5] = [
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 8.0),
        (8.0, 10.0),
        (0.0, 10.0),
    ];
    let mut editor = Editor::new(Document::default());
    let extent = Extent::OneSide(length(editor.document(), "10"));
    add_extrude(
        &mut editor,
        polygon(&CUT),
        extent,
        Operation::NewBody(BodyId::NEW),
    );
    let body = editor.document().bodies()[0].id;
    let evaluation = evaluated(editor.document());
    let solid = solid_of(&evaluation, body).clone();
    let right = face(body, key_on(&solid, DVec3::X, 10.0), [10.0, 4.0, 5.0]);
    let mm = editor.document().clone();
    let id = add(&mut editor, offset(&mm, vec![right], "1", true));
    // Pushed in by 1: still a chamfer, from (9, 9) to (8, 10).
    assert_close(volume(&editor, id, body), (90.0 - 0.5) * 10.0);
    set(&mut editor, id, offset(&mm, vec![right], "3", true));
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        "the face moves past a neighbouring face of Body 1: try a smaller distance"
    );

    let mut editor = Editor::new(Document::default());
    let body = block(&mut editor, 0.0, 0.0, 20.0, 20.0, "5");
    let extent = two_sides(editor.document(), "6", "1");
    add_extrude(
        &mut editor,
        disc((15.0, 10.0), 2.0),
        extent,
        Operation::Cut(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    let solid = solid_of(&evaluation, body).clone();
    let side = face(body, key_on(&solid, DVec3::X, 20.0), [20.0, 10.0, 2.5]);
    let mm = editor.document().clone();
    let id = add(&mut editor, offset(&mm, vec![side], "4", true));
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        "the face runs into another part of Body 1: try a smaller distance"
    );
}

/// Draws a slot: two lines 10 mm long, 6 mm apart, joined by half
/// circles at each end, tangent to them.
pub(super) fn slot(sketch: &mut Sketch) {
    let at = |sketch: &mut Sketch, x: f64, y: f64| sketch.add_point(DVec2::new(x, y)).unwrap();
    let [a, b, c, d] =
        [(0.0, -3.0), (10.0, -3.0), (10.0, 3.0), (0.0, 3.0)].map(|(x, y)| at(sketch, x, y));
    let [left, right] = [(0.0, 0.0), (10.0, 0.0)].map(|(x, y)| at(sketch, x, y));
    sketch
        .add_curve(Curve::Line { start: a, end: b }, false)
        .unwrap();
    let arc = Curve::Arc {
        center: right,
        start: b,
        end: c,
    };
    sketch.add_curve(arc, false).unwrap();
    sketch
        .add_curve(Curve::Line { start: c, end: d }, false)
        .unwrap();
    let arc = Curve::Arc {
        center: left,
        start: d,
        end: a,
    };
    sketch.add_curve(arc, false).unwrap();
}

/// A slot-shaped plate 10 high: its flat side moved out by 1 with
/// tangent faces taken in grows the whole rounded wall (radius 4, the
/// other side too), `(80 + 16π)·10`; with them left out it's refused,
/// tangent to faces not picked. The same offset twice gives the same
/// bits.
#[test]
#[ignore = "kernel offset face not built"]
fn the_kernel_grows_across_tangent_faces() {
    let mut editor = Editor::new(Document::default());
    let extent = Extent::OneSide(length(editor.document(), "10"));
    add_extrude(&mut editor, slot, extent, Operation::NewBody(BodyId::NEW));
    let body = editor.document().bodies()[0].id;
    let evaluation = evaluated(editor.document());
    let solid = solid_of(&evaluation, body).clone();
    let side = face(body, key_on(&solid, -DVec3::Y, 3.0), [5.0, -3.0, 5.0]);
    let mm = editor.document().clone();
    let id = add(&mut editor, offset(&mm, vec![side], "1", false));
    assert_close(volume(&editor, id, body), (80.0 + 16.0 * PI) * 10.0);
    let a = evaluated(editor.document());
    let b = evaluated(editor.document());
    let (a, b) = (solid_of(&a, body), solid_of(&b, body));
    assert_eq!(a.volume().to_bits(), b.volume().to_bits());
    assert_eq!(a.mesh().tris().len(), b.mesh().tris().len());
    let alone = OffsetFace {
        tangent: false,
        ..offset(&mm, vec![side], "1", false)
    };
    set(&mut editor, id, alone);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        "it is tangent to a face of Body 1 that isn't picked: pick it too, or turn on Tangent faces"
    );
}

mod fuzz;
