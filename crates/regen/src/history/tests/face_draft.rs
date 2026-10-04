//! Drafts in the history. The kernel's draft isn't built yet (it fails
//! as too complex), so: a draft reaching the kernel fails with the
//! too-complex message, changing no body, and the rest of the history
//! goes on; regen's own refusals come first (a face not found, the
//! neutral face not found or not flat). With the kernel's draft replaced
//! by a stand-in turning a box's sides about their hinges (their corners
//! moved, every face keeping its name): a draft surviving an upstream
//! dimension change, a sketch on a drafted face following it, flip and
//! the neutral plane by their volumes, a face named twice handed over
//! once, sides meeting refused, other refusals worded and drawn, the
//! cache. The analytic volumes of the kernel's draft are written out,
//! ignored until it's built.

use std::cell::RefCell;

use glam::DVec3;
use varde_document::{FaceDraft, FaceRef, PlaneRef, Scale, ScaleFactor};
use varde_kernel::mesh::{FaceKey, Form};
use varde_kernel::{DraftError, Topology};

use super::chamfer::slot;
use super::motion::{add, block, cylinder, failure, key_on, key_where, revolved, set};
use super::scale::half_disc;
use super::*;

/// Drafts faces on this test's thread by the box stand-in.
fn with_boxes() {
    super::super::face_draft::draft_by_boxes();
}

/// An angle of `text` for a draft in `document`.
fn angle(document: &Document, text: &str) -> Value {
    Value::new(text, &FaceDraft::angle_ask(&document.design())).unwrap()
}

/// The tangent of `degrees`.
fn tan(degrees: f64) -> f64 {
    degrees.to_radians().tan()
}

/// The face of `body` keyed `key`, picked at `near`.
fn face(body: BodyId, key: FaceKey, near: [f64; 3]) -> FaceRef {
    FaceRef {
        body,
        key,
        near: DVec3::from(near),
    }
}

/// The XY plane, its normal +Z.
const XY: PlaneRef = PlaneRef::Origin(OriginPlane::XY);

/// `faces` drafted by `degrees` from `neutral`, flipped or not, tangent
/// faces taken in.
fn draft(
    document: &Document,
    mut faces: Vec<FaceRef>,
    neutral: PlaneRef,
    degrees: &str,
    flip: bool,
) -> FaceDraft {
    faces.sort_by(FaceRef::order);
    FaceDraft {
        faces,
        neutral,
        angle: angle(document, degrees),
        flip,
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

/// The 10 mm cube's faces, by the outward normal: top, bottom, front
/// (y = 0), back, left (x = 0), right.
fn top(solid: &Solid, body: BodyId) -> FaceRef {
    face(body, key_on(solid, DVec3::Z, 10.0), [5.0, 5.0, 10.0])
}

fn bottom(solid: &Solid, body: BodyId) -> FaceRef {
    face(body, key_on(solid, -DVec3::Z, 0.0), [5.0, 5.0, 0.0])
}

fn front(solid: &Solid, body: BodyId) -> FaceRef {
    face(body, key_on(solid, -DVec3::Y, 0.0), [5.0, 0.0, 5.0])
}

fn back(solid: &Solid, body: BodyId) -> FaceRef {
    face(body, key_on(solid, DVec3::Y, 10.0), [5.0, 10.0, 5.0])
}

fn left(solid: &Solid, body: BodyId) -> FaceRef {
    face(body, key_on(solid, -DVec3::X, 0.0), [0.0, 5.0, 5.0])
}

fn right(solid: &Solid, body: BodyId) -> FaceRef {
    face(body, key_on(solid, DVec3::X, 10.0), [10.0, 5.0, 5.0])
}

/// The cube's four sides.
fn sides(solid: &Solid, body: BodyId) -> Vec<FaceRef> {
    vec![
        front(solid, body),
        back(solid, body),
        left(solid, body),
        right(solid, body),
    ]
}

/// What the kernel's draft says of `body` today.
fn too_complex(body: &str) -> String {
    format!("drafting faces of {body} is too complex to work out")
}

fn assert_near(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-9 * b.abs().max(1.0), "{a} vs {b}");
}

/// The volumes agree within the kernel's fitting of curved faces.
fn assert_close(a: f64, b: f64, within: f64) {
    assert!((a - b).abs() <= within * b.abs().max(1.0), "{a} vs {b}");
}

/// The volume of `body` in `editor`'s document regenerated, the draft
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

/// `∫ area(z) dz` from `a` to `b`, `area` a polynomial of degree at most
/// three: Simpson's rule, exact for it.
fn integral(a: f64, b: f64, area: impl Fn(f64) -> f64) -> f64 {
    (b - a) / 6.0 * (area(a) + 4.0 * area((a + b) / 2.0) + area(b))
}

/// A draft the kernel can't do yet fails as too complex, leaving the
/// body as it was; the history goes on: a later join still works on the
/// body.
#[test]
fn a_draft_the_kernel_cant_do_fails_as_too_complex() {
    let (mut editor, body, solid) = cube();
    let kind = draft(editor.document(), sides(&solid, body), XY, "3", false);
    let id = add(&mut editor, kind);
    let extent = two_sides(editor.document(), "12", "1");
    let join = add_extrude(
        &mut editor,
        rectangle((8.0, 8.0), (12.0, 12.0)),
        extent,
        Operation::Join(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the draft fails");
    assert_eq!(failed.message, too_complex("Body 1"));
    assert!(failure(&evaluation, join).is_none());
    assert_eq!(evaluation.failed.len(), 1);
    assert_near(
        solid_of(&evaluation, body).volume(),
        1000.0 + 16.0 * 13.0 - 40.0,
    );
    // Flipped, from a face, too.
    let flipped = draft(
        editor.document(),
        vec![front(&solid, body)],
        PlaneRef::Face(top(&solid, body)),
        "3",
        true,
    );
    set(&mut editor, id, flipped);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        too_complex("Body 1")
    );
}

/// The cube with a notch cut through its corner, from (−1, −1) to
/// (3, 3): the notch's id and its wall at x = 3 (facing −X).
fn notched(editor: &mut Editor, body: BodyId) -> (FeatureId, FaceRef) {
    let extent = two_sides(editor.document(), "11", "1");
    let notch = add_extrude(
        editor,
        rectangle((-1.0, -1.0), (3.0, 3.0)),
        extent,
        Operation::Cut(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    let solid = solid_of(&evaluation, body);
    let wall = key_where(
        solid,
        |form| matches!(*form, Form::Plane { n, d } if n.x < -0.5 && (d + 3.0).abs() < 1e-9),
    );
    (notch, face(body, wall, [3.0, 1.5, 5.0]))
}

/// A face whose maker is removed (the cut that made a notch's wall; the
/// draft stays, as it names its body only) isn't found: the draft fails
/// before the kernel, the body left as it was, its neutral plane still
/// noted for the session to draw; among others, it's named by its
/// place.
#[test]
fn a_face_that_is_gone_is_not_found() {
    let (mut editor, body, solid) = cube();
    let (notch, wall) = notched(&mut editor, body);
    let kind = draft(editor.document(), vec![wall], XY, "3", false);
    let id = add(&mut editor, kind);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        too_complex("Body 1")
    );
    editor.apply(Command::RemoveFeature(notch)).unwrap();
    assert!(editor.document().feature(id).is_some());
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the draft fails");
    assert_eq!(failed.message, "its face wasn't found");
    assert!(failed.geometry.is_none());
    assert_near(solid_of(&evaluation, body).volume(), 1000.0);
    // The neutral plane is still noted, for the session to draw.
    let noted = (evaluation.references.iter()).find(|(feature, _)| *feature == id);
    assert_eq!(
        noted.map(|(_, plane)| *plane),
        Some([DVec3::ZERO, DVec3::Z])
    );
    let kind = draft(
        editor.document(),
        vec![wall, front(&solid, body)],
        XY,
        "3",
        false,
    );
    let place = kind.faces.iter().position(|f| *f == wall).unwrap();
    set(&mut editor, id, kind);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        format!("its face {} of 2 wasn't found", place + 1)
    );
}

/// The neutral face whose maker is removed isn't found ("its neutral
/// face wasn't found"), the body left as it was; a round neutral face
/// isn't flat, and is drawn.
#[test]
fn the_neutral_face_gone_is_not_found() {
    with_boxes();
    let (mut editor, body, solid) = cube();
    let (notch, wall) = notched(&mut editor, body);
    let kind = draft(
        editor.document(),
        vec![top(&solid, body)],
        PlaneRef::Face(wall),
        "3",
        false,
    );
    let id = add(&mut editor, kind);
    // Found, but the notched cube isn't a box for the stand-in.
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        too_complex("Body 1")
    );
    editor.apply(Command::RemoveFeature(notch)).unwrap();
    assert!(editor.document().feature(id).is_some());
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the draft fails");
    assert_eq!(failed.message, "its neutral face wasn't found");
    assert!(failed.geometry.is_none());
    assert_near(solid_of(&evaluation, body).volume(), 1000.0);

    // A pin beside the cube: its wall isn't flat.
    let pin = add_body(&mut editor, disc((20.0, 5.0), 2.0), "10");
    let evaluation = evaluated(editor.document());
    let wall = face(pin, cylinder(solid_of(&evaluation, pin)), [22.0, 5.0, 5.0]);
    let kind = draft(
        editor.document(),
        vec![front(&solid, body)],
        PlaneRef::Face(wall),
        "3",
        false,
    );
    let id = add(&mut editor, kind);
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the draft fails");
    assert_eq!(failed.message, "its neutral face isn't flat");
    assert!(failed.geometry.is_some());
}

/// An upstream dimension change moves the faces; they're found again by
/// their names, and drafted where they went: the cube made 20 high, its
/// front drafted from its foot.
#[test]
fn a_draft_survives_an_upstream_change() {
    let (mut editor, body, solid) = cube();
    let kind = draft(editor.document(), vec![front(&solid, body)], XY, "5", false);
    let id = add(&mut editor, kind);
    let maker = editor.document().body(body).unwrap().created_by;
    let twenty = Extent::OneSide(length(editor.document(), "20"));
    set_extrude(&mut editor, maker, |extrude| extrude.extent = twenty);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        too_complex("Body 1")
    );
    // The stand-in's: the front leaning in by tan 5° as it rises 20.
    with_boxes();
    assert_near(
        volume(&editor, id, body),
        2000.0 - 10.0 * tan(5.0) * 400.0 / 2.0,
    );
    let evaluation = evaluated(editor.document());
    let bounds = solid_of(&evaluation, body).bounds3().unwrap();
    assert_near(bounds.max.z, 20.0);
    assert_near(bounds.min.y, 0.0);
}

/// Flip reverses the pull: the cube's front drafted 5° from its foot
/// leans in as it rises; flipped, it leans out. From the top face (its
/// normal up), the front leans out below it; flipped, in.
#[test]
fn flip_reverses_the_pull() {
    with_boxes();
    let (mut editor, body, solid) = cube();
    let mm = editor.document().clone();
    let wedge = 10.0 * tan(5.0) * 100.0 / 2.0;
    let id = add(
        &mut editor,
        draft(&mm, vec![front(&solid, body)], XY, "5", false),
    );
    assert_near(volume(&editor, id, body), 1000.0 - wedge);
    set(
        &mut editor,
        id,
        draft(&mm, vec![front(&solid, body)], XY, "5", true),
    );
    assert_near(volume(&editor, id, body), 1000.0 + wedge);
    let from_top = PlaneRef::Face(top(&solid, body));
    set(
        &mut editor,
        id,
        draft(&mm, vec![front(&solid, body)], from_top, "5", false),
    );
    assert_near(volume(&editor, id, body), 1000.0 + wedge);
    let evaluation = evaluated(editor.document());
    let bounds = solid_of(&evaluation, body).bounds3().unwrap();
    assert_near(bounds.min.y, -10.0 * tan(5.0));
    // The neutral plane noted for the session to draw, as found.
    let noted = (evaluation.references.iter()).find(|(feature, _)| *feature == id);
    let [point, normal] = noted.expect("the plane noted").1;
    assert!(point.abs_diff_eq(DVec3::new(0.0, 0.0, 10.0), 1e-9) && normal == DVec3::Z);
    set(
        &mut editor,
        id,
        draft(&mm, vec![front(&solid, body)], from_top, "5", true),
    );
    assert_near(volume(&editor, id, body), 1000.0 - wedge);
    // Undone, the draft from the top is back, then none.
    editor.undo();
    assert_near(volume(&editor, id, body), 1000.0 + wedge);
    editor.undo();
    editor.undo();
    editor.undo();
    let evaluation = evaluated(editor.document());
    assert!(editor.document().feature(id).is_none());
    assert_near(solid_of(&evaluation, body).volume(), 1000.0);
}

/// Through the stand-in: the four sides from the foot (a frustum of a
/// pyramid), from a plane through the middle (turned about hinges
/// inside the faces), two adjacent sides; the neutral plane a face of
/// another body.
#[test]
fn drafts_turn_faces_by_their_volumes() {
    with_boxes();
    let (mut editor, body, solid) = cube();
    let mm = editor.document().clone();
    let t = tan(10.0);
    let id = add(
        &mut editor,
        draft(&mm, sides(&solid, body), XY, "10", false),
    );
    let frustum = integral(0.0, 10.0, |z| (10.0 - 2.0 * t * z).powi(2));
    assert_near(volume(&editor, id, body), frustum);
    let two = vec![front(&solid, body), right(&solid, body)];
    set(&mut editor, id, draft(&mm, two, XY, "10", false));
    assert_near(
        volume(&editor, id, body),
        integral(0.0, 10.0, |z| (10.0 - t * z).powi(2)),
    );
    // The neutral plane the top of a slab beside it, 4 high, made
    // before the draft.
    let (mut editor, body, solid) = cube();
    let slab = block(&mut editor, 20.0, 0.0, 30.0, 10.0, "4");
    let evaluation = evaluated(editor.document());
    let slab_top = face(
        slab,
        key_on(solid_of(&evaluation, slab), DVec3::Z, 4.0),
        [25.0, 5.0, 4.0],
    );
    let kind = draft(
        editor.document(),
        sides(&solid, body),
        PlaneRef::Face(slab_top),
        "10",
        false,
    );
    let id = add(&mut editor, kind);
    let wanted = integral(0.0, 10.0, |z| (10.0 - 2.0 * t * (z - 4.0)).powi(2));
    assert_near(volume(&editor, id, body), wanted);
    assert_eq!(
        editor.document().feature(id).unwrap().kind.bodies(),
        [body, slab]
    );

    // A cube from z = −5 to 5, drafted about XY through its middle.
    let mut editor = Editor::new(Document::default());
    let extent = two_sides(editor.document(), "5", "5");
    add_extrude(
        &mut editor,
        rectangle((0.0, 0.0), (10.0, 10.0)),
        extent,
        Operation::NewBody(BodyId::NEW),
    );
    let middle = editor.document().bodies()[0].id;
    let evaluation = evaluated(editor.document());
    let solid = solid_of(&evaluation, middle).clone();
    let four = vec![
        face(middle, key_on(&solid, -DVec3::Y, 0.0), [5.0, 0.0, 1.0]),
        face(middle, key_on(&solid, DVec3::Y, 10.0), [5.0, 10.0, 1.0]),
        face(middle, key_on(&solid, -DVec3::X, 0.0), [0.0, 5.0, 1.0]),
        face(middle, key_on(&solid, DVec3::X, 10.0), [10.0, 5.0, 1.0]),
    ];
    let mm = editor.document().clone();
    let id = add(&mut editor, draft(&mm, four, XY, "10", false));
    let wanted = integral(-5.0, 5.0, |z| (10.0 - 2.0 * t * z).powi(2));
    assert_near(volume(&editor, id, middle), wanted);
    assert!(wanted > 1000.0);
}

/// The world corner `at`, on `placement`'s plane, within rounding.
fn on_plane(placement: &Placement, at: DVec3) -> bool {
    (at - placement.origin).dot(placement.normal).abs() < 1e-9
}

/// A sketch on the face a draft turns, after it, is placed on the face
/// where the draft left it, and follows it when the angle or an
/// upstream dimension changes: drafted from the top, the front's plane
/// goes through the top's front edge whatever the cube's height.
#[test]
fn a_sketch_on_a_drafted_face_follows_it() {
    with_boxes();
    let (mut editor, body, solid) = cube();
    let from_top = PlaneRef::Face(top(&solid, body));
    let kind = draft(
        editor.document(),
        vec![front(&solid, body)],
        from_top,
        "10",
        false,
    );
    let id = add(&mut editor, kind);
    editor
        .apply(
            editor
                .document()
                .add_sketch(Plane::Face(front(&solid, body))),
        )
        .unwrap();
    let sketch = editor.document().features().last().unwrap().id;
    let placed = |editor: &Editor| {
        let evaluation = evaluated(editor.document());
        assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
        let found = (evaluation.placements.iter()).find(|(feature, _)| *feature == sketch);
        found.expect("the sketch is placed").1
    };
    let leaning = |degrees: f64| {
        let r = degrees.to_radians();
        DVec3::new(0.0, -r.cos(), r.sin())
    };
    let placement = placed(&editor);
    assert!(placement.normal.abs_diff_eq(leaning(10.0), 1e-12));
    // The hinge, the top's front edge, and the foot pushed out.
    assert!(on_plane(&placement, DVec3::new(3.0, 0.0, 10.0)));
    assert!(on_plane(
        &placement,
        DVec3::new(3.0, -10.0 * tan(10.0), 0.0)
    ));
    // Another angle.
    let mm = editor.document().clone();
    set(
        &mut editor,
        id,
        draft(&mm, vec![front(&solid, body)], from_top, "4", false),
    );
    let placement = placed(&editor);
    assert!(placement.normal.abs_diff_eq(leaning(4.0), 1e-12));
    assert!(on_plane(&placement, DVec3::new(3.0, 0.0, 10.0)));
    // The cube made 20 high: the hinge goes up with the top.
    let maker = editor.document().body(body).unwrap().created_by;
    let twenty = Extent::OneSide(length(editor.document(), "20"));
    set_extrude(&mut editor, maker, |extrude| extrude.extent = twenty);
    let placement = placed(&editor);
    assert!(placement.normal.abs_diff_eq(leaning(4.0), 1e-12));
    assert!(on_plane(&placement, DVec3::new(3.0, 0.0, 20.0)));
    assert!(!on_plane(&placement, DVec3::new(3.0, 0.0, 10.0)));
}

/// Sides drafted until they meet the one opposite at the box's top are
/// refused (the top would shrink to nothing), the face drawn, the body
/// left whole; a top facing the pull is refused too.
#[test]
fn sides_meeting_and_faces_facing_the_pull_are_refused() {
    with_boxes();
    let (mut editor, body, solid) = cube();
    let mm = editor.document().clone();
    let opposite = vec![front(&solid, body), back(&solid, body)];
    // tan α · 10 on each side reaches 5 at α = atan(1/2), 26.57°.
    let id = add(&mut editor, draft(&mm, opposite.clone(), XY, "27", false));
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the draft fails");
    assert_eq!(
        failed.message,
        "the face turns past a neighbouring face of Body 1: try a smaller angle"
    );
    assert!(failed.geometry.is_some());
    assert_near(solid_of(&evaluation, body).volume(), 1000.0);
    set(&mut editor, id, draft(&mm, opposite, XY, "26", false));
    let t = tan(26.0);
    assert_near(
        volume(&editor, id, body),
        10.0 * integral(0.0, 10.0, |z| 10.0 - 2.0 * t * z),
    );
    let facing = vec![top(&solid, body), front(&solid, body)];
    set(&mut editor, id, draft(&mm, facing, XY, "3", false));
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the draft fails");
    assert_eq!(
        failed.message,
        "a face of Body 1 faces the pull direction: nothing to draft"
    );
    assert!(failed.geometry.is_some());
    set(
        &mut editor,
        id,
        draft(&mm, vec![bottom(&solid, body)], XY, "3", true),
    );
    let evaluation = evaluated(editor.document());
    assert!(failure(&evaluation, id).is_some());
}

/// What the recording stand-in was handed: the regions, the neutral
/// plane's point, the pull, the angle and whether tangent faces are
/// taken in.
type Handed = (Vec<u32>, DVec3, DVec3, f64, bool);

thread_local! {
    /// What [`recording`] was handed.
    static HANDED: RefCell<Vec<Handed>> = const { RefCell::new(Vec::new()) };
}

/// A stand-in that records what it's handed, and fails.
#[allow(clippy::too_many_arguments)]
fn recording(
    _: &Solid,
    _: &Topology,
    faces: &[u32],
    neutral: DVec3,
    pull: DVec3,
    angle: f64,
    tangent: bool,
    _: u64,
    _: &Tolerance,
    _: &Budget,
) -> Result<Solid, DraftError> {
    HANDED.with_borrow_mut(|handed| handed.push((faces.to_vec(), neutral, pull, angle, tangent)));
    Err(DraftError::IntoBody)
}

/// Two references to one face (picked at two points) hand the kernel
/// that face once; the neutral plane as a point and the pull (the top's
/// normal, flipped down), the angle in radians and the tangent flag as
/// stored.
#[test]
fn a_face_named_twice_is_drafted_once() {
    super::super::face_draft::DRAFTER.set(Some(recording));
    let (mut editor, body, solid) = cube();
    let mm = editor.document().clone();
    let mut twice = front(&solid, body);
    twice.near = DVec3::new(1.0, 0.0, 1.0);
    let kind = FaceDraft {
        tangent: false,
        ..draft(
            &mm,
            vec![front(&solid, body), twice, left(&solid, body)],
            PlaneRef::Face(top(&solid, body)),
            "2.5",
            true,
        )
    };
    let id = add(&mut editor, kind);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        "the face runs into another part of Body 1: try a smaller angle"
    );
    let topology = solid.topology();
    let region = |key: FaceKey| {
        (topology.regions().iter())
            .position(|region| region.key == key)
            .unwrap() as u32
    };
    let mut wanted = vec![
        region(front(&solid, body).key),
        region(left(&solid, body).key),
    ];
    wanted.sort();
    let handed = HANDED.with_borrow(Clone::clone);
    let [(faces, neutral, pull, angle, tangent)] = &handed[..] else {
        panic!("{handed:?}");
    };
    assert_eq!(*faces, wanted);
    assert_near(neutral.z, 10.0);
    assert_eq!(*pull, -DVec3::Z);
    assert_near(*angle, 2.5f64.to_radians());
    assert!(!tangent);
}

/// A stand-in failing with what the thread's [`REFUSAL`] says.
#[allow(clippy::too_many_arguments)]
fn refusing(
    _: &Solid,
    _: &Topology,
    _: &[u32],
    _: DVec3,
    _: DVec3,
    _: f64,
    _: bool,
    _: u64,
    _: &Tolerance,
    _: &Budget,
) -> Result<Solid, DraftError> {
    Err(REFUSAL.with_borrow(Clone::clone))
}

thread_local! {
    /// What [`refusing`] fails with.
    static REFUSAL: RefCell<DraftError> = const { RefCell::new(DraftError::IntoBody) };
}

/// The kernel's refusals are worded for the Timeline, with the face or
/// corner they're about drawn.
#[test]
fn refusals_are_worded_and_drawn() {
    let (mut editor, body, solid) = cube();
    let mm = editor.document().clone();
    let id = add(
        &mut editor,
        draft(&mm, vec![front(&solid, body)], XY, "3", false),
    );
    super::super::face_draft::DRAFTER.set(Some(refusing));
    for (error, words, drawn) in [
        (
            DraftError::FacingPull { region: 0 },
            "a face of Body 1 faces the pull direction: nothing to draft",
            true,
        ),
        (
            DraftError::CannotDraft { region: 1 },
            "a face of Body 1 can't be drafted: only flat faces and walls along the pull can",
            true,
        ),
        (
            DraftError::PastNeighbour { region: 0 },
            "the face turns past a neighbouring face of Body 1: try a smaller angle",
            true,
        ),
        (
            DraftError::IntoBody,
            "the face runs into another part of Body 1: try a smaller angle",
            false,
        ),
        (
            DraftError::RoundTooSmall { region: 0 },
            "a round face of Body 1 narrows to nothing: try a smaller angle",
            true,
        ),
        (
            DraftError::NoSurface { region: 1 },
            "a face of Body 1 next to it has no surface to extend",
            true,
        ),
        (
            DraftError::TangentNeighbour { region: 2 },
            "it is tangent to a face of Body 1 that isn't picked: pick it too, or turn on Tangent faces",
            true,
        ),
        (
            DraftError::Corner { vertex: 0 },
            "faces of Body 1 meeting at a corner can't be drafted together: try another angle",
            true,
        ),
        (
            DraftError::OutOfRange,
            "it moves Body 1 out of range",
            false,
        ),
    ] {
        REFUSAL.set(error);
        let evaluation = evaluated(editor.document());
        let failed = failure(&evaluation, id).expect("the draft fails");
        assert_eq!(failed.message, words);
        assert_eq!(failed.geometry.is_some(), drawn, "{words}");
        assert_near(solid_of(&evaluation, body).volume(), 1000.0);
    }
}

/// A draft that worked is found in the cache the next time; flipped it
/// isn't.
#[test]
fn a_draft_that_worked_is_cached() {
    with_boxes();
    let (mut editor, body, solid) = cube();
    let kind = draft(editor.document(), vec![front(&solid, body)], XY, "5", false);
    let id = add(&mut editor, kind);
    let mut cache = Cache::default();
    let first = evaluate(editor.document(), &mut cache);
    let again = evaluate(editor.document(), &mut cache);
    assert!(Arc::ptr_eq(&first.bodies[0].solid, &again.bodies[0].solid));
    let wedge = 10.0 * tan(5.0) * 100.0 / 2.0;
    assert_near(first.bodies[0].solid.volume(), 1000.0 - wedge);
    let flipped = draft(editor.document(), vec![front(&solid, body)], XY, "5", true);
    set(&mut editor, id, flipped);
    let other = evaluate(editor.document(), &mut cache);
    assert_near(other.bodies[0].solid.volume(), 1000.0 + wedge);
}

/// A draft of a body a combine took into another fails, naming where it
/// went, as every feature naming a consumed body does.
#[test]
fn a_consumed_body_fails_a_draft() {
    let (mut editor, body, _) = cube();
    let other = block(&mut editor, 5.0, 5.0, 15.0, 15.0, "10");
    let evaluation = evaluated(editor.document());
    let other_side = face(
        other,
        key_on(solid_of(&evaluation, other), DVec3::X, 15.0),
        [15.0, 10.0, 5.0],
    );
    let combine = varde_document::Combine {
        target: body,
        tools: vec![other],
        op: varde_document::BodyOp::Union,
        keep_tools: false,
    };
    add(&mut editor, combine);
    let mm = editor.document().clone();
    let id = add(&mut editor, draft(&mm, vec![other_side], XY, "3", false));
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the draft fails");
    assert_eq!(
        failed.message,
        "Body 2 is in Body 1 now: a feature before this one merged it in"
    );
}

/// The stand-in against the volumes it should give, on a 10 × 6 × 4
/// box: every set of its faces drafted, from XY and from its top,
/// flipped or not, at angles small to steep. Each that works has the
/// volume its cross-sections give and keeps every face's name; a face
/// facing the pull, or sides meeting the ones opposite, fail, never
/// leaving the body as it was.
#[test]
fn the_stand_in_drafts_boxes_by_their_volumes() {
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
    let top = PlaneRef::Face(sides[5].1);
    let mm = editor.document().clone();
    let id = add(&mut editor, draft(&mm, vec![sides[0].1], XY, "1", false));
    for mask in 1..64u32 {
        let picked: Vec<usize> = (0..6).filter(|k| mask & (1 << k) != 0).collect();
        let faces: Vec<FaceRef> = picked.iter().map(|&k| sides[k].1).collect();
        let mut drafted = [0.0f64; 2];
        for &k in &picked {
            if sides[k].0 < 2 {
                drafted[sides[k].0] += 1.0;
            }
        }
        let facing = picked.iter().any(|&k| sides[k].0 == 2);
        for text in ["1", "10", "30", "45", "56.3", "56.4", "75"] {
            let t = tan(text.parse().unwrap());
            for (neutral, q) in [(XY, 0.0), (top, 4.0)] {
                for flip in [false, true] {
                    let kind = draft(&mm, faces.clone(), neutral, text, flip);
                    set(&mut editor, id, kind);
                    let evaluation = evaluated(editor.document());
                    let sign = if flip { -1.0 } else { 1.0 };
                    let width =
                        |axis: usize, z: f64| size[axis] - drafted[axis] * t * sign * (z - q);
                    let least = [0.0, 4.0]
                        .into_iter()
                        .flat_map(|z| [width(0, z), width(1, z)])
                        .fold(f64::INFINITY, f64::min);
                    let what = format!("faces {mask:06b} {text}° from {q} flip {flip}");
                    match failure(&evaluation, id) {
                        None => {
                            assert!(!facing && least > 0.0, "{what}: worked");
                            let got = solid_of(&evaluation, body);
                            let wanted = integral(0.0, 4.0, |z| width(0, z) * width(1, z));
                            assert!(
                                (got.volume() - wanted).abs() <= 1e-9 * wanted,
                                "{what}: {} vs {wanted}",
                                got.volume()
                            );
                            let now: Vec<FaceKey> = (got.topology().regions().iter())
                                .map(|region| region.key)
                                .collect();
                            assert_eq!(now, keys, "{what}: names kept");
                        }
                        Some(failed) => {
                            assert!(facing || least <= 1e-6, "{what}: {}", failed.message);
                            assert_near(solid_of(&evaluation, body).volume(), 240.0);
                        }
                    }
                }
            }
        }
    }
}

// The kernel's draft, analytically: ignored until it's built.

/// The cube's four sides drafted 5° from its foot (a frustum of a
/// pyramid) and from its top (widening down), names kept; the same bits
/// twice.
#[test]
#[ignore = "kernel draft not built"]
fn the_kernel_drafts_a_box_s_sides() {
    let (mut editor, body, solid) = cube();
    let mm = editor.document().clone();
    let t = tan(5.0);
    let id = add(&mut editor, draft(&mm, sides(&solid, body), XY, "5", false));
    let frustum = integral(0.0, 10.0, |z| (10.0 - 2.0 * t * z).powi(2));
    assert_near(volume(&editor, id, body), frustum);
    let a = evaluated(editor.document());
    let b = evaluated(editor.document());
    let (a, b) = (solid_of(&a, body), solid_of(&b, body));
    assert_eq!(a.volume().to_bits(), b.volume().to_bits());
    assert_eq!(a.mesh().tris().len(), b.mesh().tris().len());
    let mut before: Vec<FaceKey> = (solid.topology().regions().iter())
        .map(|region| region.key)
        .collect();
    let mut after: Vec<FaceKey> = (a.topology().regions().iter())
        .map(|region| region.key)
        .collect();
    before.sort();
    after.sort();
    assert_eq!(before, after);
    let from_top = PlaneRef::Face(top(&solid, body));
    set(
        &mut editor,
        id,
        draft(&mm, sides(&solid, body), from_top, "5", false),
    );
    let widening = integral(0.0, 10.0, |z| (10.0 + 2.0 * t * (10.0 - z)).powi(2));
    assert_near(volume(&editor, id, body), widening);
}

/// The cube from z = −5 to 5, its sides drafted about XY through its
/// middle: each face turned about a hinge inside it.
#[test]
#[ignore = "kernel draft not built"]
fn the_kernel_drafts_about_a_hinge_inside_the_faces() {
    let mut editor = Editor::new(Document::default());
    let extent = two_sides(editor.document(), "5", "5");
    add_extrude(
        &mut editor,
        rectangle((0.0, 0.0), (10.0, 10.0)),
        extent,
        Operation::NewBody(BodyId::NEW),
    );
    let body = editor.document().bodies()[0].id;
    let evaluation = evaluated(editor.document());
    let solid = solid_of(&evaluation, body).clone();
    let four = vec![
        face(body, key_on(&solid, -DVec3::Y, 0.0), [5.0, 0.0, 1.0]),
        face(body, key_on(&solid, DVec3::Y, 10.0), [5.0, 10.0, 1.0]),
        face(body, key_on(&solid, -DVec3::X, 0.0), [0.0, 5.0, 1.0]),
        face(body, key_on(&solid, DVec3::X, 10.0), [10.0, 5.0, 1.0]),
    ];
    let mm = editor.document().clone();
    let id = add(&mut editor, draft(&mm, four, XY, "5", false));
    let t = tan(5.0);
    let wanted = integral(-5.0, 5.0, |z| (10.0 - 2.0 * t * z).powi(2));
    assert_near(volume(&editor, id, body), wanted);
}

/// A 20 mm square plate 5 thick with a boss of radius 3 on it, 5 high,
/// its wall drafted 5° from the plate's top: an exact cone frustum (the
/// form's half-angle to 1e-12); a plate with a hole of radius 3 through
/// it, the hole's wall drafted from its foot, widening up.
#[test]
#[ignore = "kernel draft not built"]
fn the_kernel_drafts_a_boss_and_a_hole_into_cones() {
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
    let plate_top = face(body, key_on(&solid, DVec3::Z, 5.0), [2.0, 2.0, 5.0]);
    let wall = face(body, cylinder(&solid), [13.0, 10.0, 7.0]);
    let mm = editor.document().clone();
    let id = add(
        &mut editor,
        draft(&mm, vec![wall], PlaneRef::Face(plate_top), "5", false),
    );
    let t = tan(5.0);
    let boss = PI * integral(0.0, 5.0, |u| (3.0 - t * u).powi(2));
    assert_close(volume(&editor, id, body), 2000.0 + boss, 1e-9);
    let evaluation = evaluated(editor.document());
    let drafted = solid_of(&evaluation, body);
    let cone = key_where(drafted, |form| matches!(form, Form::Cone { .. }));
    assert_eq!(cone, wall.key);
    let topology = drafted.topology();
    let region = (topology.regions().iter())
        .find(|region| region.key == cone)
        .unwrap();
    let Form::Cone { sin, cos, axis, .. } = *crate::picking::region_form(drafted, region) else {
        unreachable!()
    };
    let r = 5f64.to_radians();
    assert!((sin - r.sin()).abs() < 1e-12 && (cos - r.cos()).abs() < 1e-12);
    assert!(axis.abs_diff_eq(-DVec3::Z, 1e-12) || axis.abs_diff_eq(DVec3::Z, 1e-12));

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
    let mm = editor.document().clone();
    let id = add(&mut editor, draft(&mm, vec![wall], XY, "5", false));
    let hole = PI * integral(0.0, 5.0, |z| (3.0 + t * z).powi(2));
    assert_close(volume(&editor, id, body), 2000.0 - hole, 1e-9);
}

/// A slot-shaped plate 10 high (sides 10 long, 6 apart, round ends):
/// one flat side drafted 5° from its foot with tangent faces grown takes
/// the whole wall, planes and cones still tangent (the stadium offset
/// in by `tan 5° · z`); with them left out it's refused.
#[test]
#[ignore = "kernel draft not built"]
fn the_kernel_drafts_a_rounded_slot_across_tangent_faces() {
    let mut editor = Editor::new(Document::default());
    let extent = Extent::OneSide(length(editor.document(), "10"));
    add_extrude(&mut editor, slot, extent, Operation::NewBody(BodyId::NEW));
    let body = editor.document().bodies()[0].id;
    let evaluation = evaluated(editor.document());
    let solid = solid_of(&evaluation, body).clone();
    let side = face(body, key_on(&solid, -DVec3::Y, 3.0), [5.0, -3.0, 5.0]);
    let mm = editor.document().clone();
    let id = add(&mut editor, draft(&mm, vec![side], XY, "5", false));
    let t = tan(5.0);
    let stadium = |d: f64| 10.0 * (6.0 - 2.0 * d) + PI * (3.0 - d).powi(2);
    assert_close(
        volume(&editor, id, body),
        integral(0.0, 10.0, |z| stadium(t * z)),
        1e-9,
    );
    let alone = FaceDraft {
        tangent: false,
        ..draft(&mm, vec![side], XY, "5", false)
    };
    set(&mut editor, id, alone);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        "it is tangent to a face of Body 1 that isn't picked: pick it too, or turn on Tangent faces"
    );
}

/// A plate 20 square and 5 thick with two holes of radius 2 through it,
/// drafted as a whole 3° from its foot: its sides a frustum, its holes
/// widening cones.
#[test]
#[ignore = "kernel draft not built"]
fn the_kernel_drafts_a_plate_with_holes_as_a_whole() {
    let mut editor = Editor::new(Document::default());
    let body = block(&mut editor, 0.0, 0.0, 20.0, 20.0, "5");
    for x in [6.0, 14.0] {
        let extent = two_sides(editor.document(), "6", "1");
        add_extrude(
            &mut editor,
            disc((x, 10.0), 2.0),
            extent,
            Operation::Cut(Targets::default()),
        );
    }
    let evaluation = evaluated(editor.document());
    let solid = solid_of(&evaluation, body).clone();
    let topology = solid.topology();
    let walls: Vec<FaceRef> = (topology.regions().iter())
        .filter_map(
            |region| match *crate::picking::region_form(&solid, region) {
                Form::Plane { n, .. } if n.z.abs() > 0.5 => None,
                Form::Plane { n, d } => Some(face(body, region.key, (n * d + DVec3::Z).into())),
                Form::Cylinder {
                    point,
                    axis,
                    radius,
                } => {
                    let out = axis.any_orthonormal_vector() * radius;
                    let mut near = point + out;
                    near.z = 2.0;
                    Some(face(body, region.key, near.into()))
                }
                _ => panic!("a plane or a cylinder"),
            },
        )
        .collect();
    assert_eq!(walls.len(), 6);
    let mm = editor.document().clone();
    let id = add(&mut editor, draft(&mm, walls, XY, "3", false));
    let t = tan(3.0);
    let outer = integral(0.0, 5.0, |z| (20.0 - 2.0 * t * z).powi(2));
    let hole = PI * integral(0.0, 5.0, |z| (2.0 + t * z).powi(2));
    assert_close(volume(&editor, id, body), outer - 2.0 * hole, 1e-9);
}

/// A cylinder of radius 5 scaled × 2 along Y (an elliptic wall of
/// semi-axes 5 and 10), 10 high, its wall drafted 3° from its foot:
/// the fitted constant-slope surface, its sections the ellipse's
/// offsets (area `A − P·d + π d²` at `d = tan 3° · z`), within the fit.
#[test]
#[ignore = "kernel draft not built"]
fn the_kernel_drafts_an_elliptic_wall_fitted() {
    let mut editor = Editor::new(Document::default());
    let pin = add_body(&mut editor, disc((0.0, 0.0), 5.0), "10");
    let factor = |text: &str| Value::new(text, &Scale::factor_ask(&Document::default().design()));
    let scale = Scale {
        bodies: vec![pin],
        about: varde_document::PointRef::Origin,
        factor: ScaleFactor::PerAxis(["1", "2", "1"].map(|t| factor(t).unwrap())),
    };
    add(&mut editor, scale);
    let evaluation = evaluated(editor.document());
    let solid = solid_of(&evaluation, pin).clone();
    let wall = key_where(&solid, |form| matches!(form, Form::ConicCylinder { .. }));
    let mm = editor.document().clone();
    let id = add(
        &mut editor,
        draft(&mm, vec![face(pin, wall, [5.0, 0.0, 5.0])], XY, "3", false),
    );
    let (area, perimeter) = (PI * 50.0, 48.442_241_102_739_24);
    let t = tan(3.0);
    let wanted = integral(0.0, 10.0, |z| {
        let d = t * z;
        area - perimeter * d + PI * d * d
    });
    assert_close(volume(&editor, id, pin), wanted, 1e-4);
}

/// A sphere can't be drafted; a narrow slot (2 wide) cut through a
/// plate, its walls drafted 10° so it narrows up, closes: refused.
#[test]
#[ignore = "kernel draft not built"]
fn the_kernel_refuses_a_sphere_and_a_slot_closing() {
    let mut editor = Editor::new(Document::default());
    let ball = revolved(&mut editor, half_disc(5.0));
    let evaluation = evaluated(editor.document());
    let round = key_where(solid_of(&evaluation, ball), |form| {
        matches!(form, Form::Sphere { .. })
    });
    let mm = editor.document().clone();
    let id = add(
        &mut editor,
        draft(
            &mm,
            vec![face(ball, round, [5.0, 0.0, 0.0])],
            XY,
            "3",
            false,
        ),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        "a face of Body 1 can't be drafted: only flat faces and walls along the pull can"
    );

    let mut editor = Editor::new(Document::default());
    let body = block(&mut editor, 0.0, 0.0, 20.0, 20.0, "10");
    let extent = two_sides(editor.document(), "11", "1");
    add_extrude(
        &mut editor,
        rectangle((9.0, 2.0), (11.0, 18.0)),
        extent,
        Operation::Cut(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    let solid = solid_of(&evaluation, body).clone();
    let walls = vec![
        face(body, key_on(&solid, DVec3::X, 9.0), [9.0, 10.0, 5.0]),
        face(body, key_on(&solid, -DVec3::X, -11.0), [11.0, 10.0, 5.0]),
    ];
    let mm = editor.document().clone();
    let id = add(&mut editor, draft(&mm, walls, XY, "10", true));
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        "the face turns past a neighbouring face of Body 1: try a smaller angle"
    );
}

mod fuzz;
