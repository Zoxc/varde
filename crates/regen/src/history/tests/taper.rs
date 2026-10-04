//! Tapered extrudes in the history. The kernel's tapered extrude isn't
//! built yet (any taper fails as too complex), so: a tapered extrude
//! fails with the too-complex message, making no body and changing none,
//! and the rest of the history goes on. With the kernel's replaced by a
//! stand-in tapering rectangles on one side of their plane into frustums
//! of pyramids: the tool's volume, a tapered pocket cut into the plate,
//! a negative taper widening, the taper in the cache key, its refusal
//! worded. The analytic volumes of the feature with the kernel's taper
//! (a tapered round boss joined to the plate, a tapered cut through all,
//! two sides) are written out, ignored until it's built.

use varde_document::{Extrude, OriginPlane};
use varde_expr::Value;

use super::motion::{failure, the_plate};
use super::*;

/// Tapers on this test's thread by the frustum stand-in.
fn with_frustums() {
    super::super::taper::taper_by_frustum();
}

fn evaluated(document: &Document) -> Evaluation {
    evaluate(document, &mut Cache::default())
}

/// What the kernel's tapered extrude says today.
const TOO_COMPLEX: &str = "tapering its walls is too complex to work out";

/// A taper of `text` in `document`'s design.
fn taper(document: &Document, text: &str) -> Value {
    Value::new(text, &Extrude::taper_ask(&document.design())).unwrap()
}

/// The tangent of `degrees`, as the stand-in and the kernel take it.
fn tan(degrees: f64) -> f64 {
    let (sin, cos) = varde_kernel::trig::sin_cos(degrees.to_radians());
    sin / cos
}

/// The volume of a rectangle `a` × `b` tapered by `t = tan α` over `h`
/// away from its plane: a frustum of a pyramid.
fn frustum(a: f64, b: f64, t: f64, h: f64) -> f64 {
    let (top_a, top_b) = (a - 2.0 * h * t, b - 2.0 * h * t);
    // Prismatoid: (h / 6)(A0 + 4 Am + A1).
    let (mid_a, mid_b) = (a - h * t, b - h * t);
    h / 6.0 * (a * b + 4.0 * mid_a * mid_b + top_a * top_b)
}

fn assert_close(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-9 * b.abs().max(1.0), "{a} vs {b}");
}

/// Adds a new body of the rectangle `min`–`max` on XY over `extent`
/// tapered by `degrees`: its id.
fn add_tapered(
    editor: &mut Editor,
    min: (f64, f64),
    max: (f64, f64),
    extent: Extent,
    degrees: &str,
    operation: Operation,
) -> FeatureId {
    let feature = add_extrude(editor, rectangle(min, max), extent, operation);
    let taper = taper(editor.document(), degrees);
    set_extrude(editor, feature, |extrude| extrude.taper = Some(taper));
    feature
}

/// The body made by `feature`'s new body.
fn made_by(editor: &Editor, feature: FeatureId) -> BodyId {
    let FeatureKind::Extrude(extrude) = &editor.document().feature(feature).unwrap().kind else {
        panic!("an extrude");
    };
    extrude.operation.new_body().expect("a new body")
}

#[test]
fn the_kernel_s_taper_fails_as_too_complex_and_the_history_goes_on() {
    let mut editor = Editor::new(Document::example());
    let extent = Extent::OneSide(length(editor.document(), "5"));
    let boss = add_tapered(
        &mut editor,
        (35.0, -5.0),
        (45.0, 5.0),
        extent,
        "3",
        Operation::NewBody(BodyId::NEW),
    );
    let body = made_by(&editor, boss);
    let pocket = add_pocket(&mut editor);
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, boss).expect("the taper fails");
    assert_eq!(failed.message, TOO_COMPLEX);
    assert!(evaluation.bodies.iter().all(|made| made.body != body));
    assert!(failure(&evaluation, pocket).is_none());
    assert_close(evaluation.bodies[0].solid.volume(), the_plate() - POCKET);
}

#[test]
fn a_tapered_cut_fails_and_changes_no_body() {
    let mut editor = Editor::new(Document::example());
    let extent = Extent::OneSide(length(editor.document(), "4"));
    let cut = add_tapered(
        &mut editor,
        (-25.0, -10.0),
        (-12.0, 10.0),
        extent,
        "5",
        Operation::Cut(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(failure(&evaluation, cut).unwrap().message, TOO_COMPLEX);
    assert_close(evaluation.bodies[0].solid.volume(), the_plate());
}

#[test]
fn a_tapered_box_is_a_frustum_with_its_names() {
    with_frustums();
    let mut editor = Editor::new(Document::example());
    let extent = Extent::OneSide(length(editor.document(), "5"));
    let boss = add_tapered(
        &mut editor,
        (35.0, -5.0),
        (45.0, 5.0),
        extent,
        "10",
        Operation::NewBody(BodyId::NEW),
    );
    let body = made_by(&editor, boss);
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let made = (evaluation.bodies.iter())
        .find(|made| made.body == body)
        .unwrap();
    assert_close(made.solid.volume(), frustum(10.0, 10.0, tan(10.0), 5.0));
    // Its top is 10 − 10 tan 10° wide, about its middle.
    let bounds = made.solid.bounds3().unwrap();
    assert_close(bounds.max.z, 5.0);
    // Every face is still named by the extrude.
    assert!(
        (made.solid.mesh().faces().iter()).all(|face| face.name.feature == boss.get()),
        "named by the extrude"
    );
}

#[test]
fn a_negative_taper_widens() {
    with_frustums();
    let mut editor = Editor::new(Document::example());
    let extent = Extent::OneSide(length(editor.document(), "5"));
    let boss = add_tapered(
        &mut editor,
        (35.0, -5.0),
        (45.0, 5.0),
        extent,
        "-10",
        Operation::NewBody(BodyId::NEW),
    );
    let body = made_by(&editor, boss);
    let evaluation = evaluated(editor.document());
    let made = (evaluation.bodies.iter())
        .find(|made| made.body == body)
        .unwrap();
    let want = frustum(10.0, 10.0, tan(-10.0), 5.0);
    assert!(want > 500.0);
    assert_close(made.solid.volume(), want);
}

#[test]
fn a_tapered_pocket_narrows_into_the_plate() {
    with_frustums();
    let mut editor = Editor::new(Document::example());
    // Up into the plate's bottom from its plane, narrowing as it goes.
    let extent = Extent::OneSide(length(editor.document(), "4"));
    let cut = add_tapered(
        &mut editor,
        (-25.0, -10.0),
        (-12.0, 10.0),
        extent,
        "10",
        Operation::Cut(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let body = editor.document().bodies()[0].id;
    assert_eq!(evaluation.touched, [(cut, vec![body])]);
    let pocket = frustum(13.0, 20.0, tan(10.0), 4.0);
    assert!(pocket < POCKET);
    assert_close(evaluation.bodies[0].solid.volume(), the_plate() - pocket);
}

#[test]
fn the_taper_is_in_the_tool_s_key() {
    with_frustums();
    let mut editor = Editor::new(Document::example());
    let extent = Extent::OneSide(length(editor.document(), "5"));
    let boss = add_tapered(
        &mut editor,
        (35.0, -5.0),
        (45.0, 5.0),
        extent,
        "10",
        Operation::NewBody(BodyId::NEW),
    );
    let body = made_by(&editor, boss);
    let mut cache = Cache::default();
    let volume = |evaluation: &Evaluation| {
        let made = (evaluation.bodies.iter()).find(|made| made.body == body);
        made.unwrap().solid.volume()
    };
    let first = evaluate(editor.document(), &mut cache);
    assert_close(volume(&first), frustum(10.0, 10.0, tan(10.0), 5.0));
    let steeper = taper(editor.document(), "20");
    set_extrude(&mut editor, boss, |extrude| extrude.taper = Some(steeper));
    let second = evaluate(editor.document(), &mut cache);
    assert_close(volume(&second), frustum(10.0, 10.0, tan(20.0), 5.0));
    // No taper: the straight box, the untapered extrude's.
    set_extrude(&mut editor, boss, |extrude| extrude.taper = None);
    let straight = evaluate(editor.document(), &mut cache);
    assert_close(volume(&straight), 500.0);
    // Undone back to 20°, found in the cache.
    editor.undo();
    let (_, misses) = cache.counts();
    let again = evaluate(editor.document(), &mut cache);
    assert_eq!(cache.counts().1, misses, "nothing worked out again");
    assert_close(volume(&again), frustum(10.0, 10.0, tan(20.0), 5.0));
}

#[test]
fn a_taper_closing_the_profile_is_refused_in_words() {
    with_frustums();
    let mut editor = Editor::new(Document::example());
    // 2 wide: closed 1 / tan 20° ≈ 2.7 up, short of 5.
    let extent = Extent::OneSide(length(editor.document(), "5"));
    let slot = add_tapered(
        &mut editor,
        (35.0, -10.0),
        (37.0, 10.0),
        extent,
        "20",
        Operation::NewBody(BodyId::NEW),
    );
    let pocket = add_pocket(&mut editor);
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, slot).expect("it closes");
    assert!(
        failed
            .message
            .starts_with("the taper closes the profile before its end"),
        "{}",
        failed.message
    );
    assert!(failure(&evaluation, pocket).is_none());
}

// With the kernel's taper.

/// A sketch on `plane` drawn by `draw`, and an extrude of its regions:
/// its id.
fn add_on(
    editor: &mut Editor,
    plane: OriginPlane,
    draw: impl FnOnce(&mut varde_sketch::Sketch),
    extent: Extent,
    degrees: &str,
    operation: Operation,
) -> FeatureId {
    let feature = add_extrude_on(editor, plane, draw, extent, operation);
    let taper = taper(editor.document(), degrees);
    set_extrude(editor, feature, |extrude| extrude.taper = Some(taper));
    feature
}

#[test]
#[ignore = "kernel taper not built"]
fn a_tapered_round_boss_joined_to_the_plate_is_a_cone_frustum() {
    let mut editor = Editor::new(Document::example());
    // From the plate's bottom up 15: 10 inside the plate, 5 above.
    let extent = Extent::OneSide(length(editor.document(), "15"));
    let boss = add_on(
        &mut editor,
        OriginPlane::XY,
        disc((20.0, 0.0), 5.0),
        extent,
        "5",
        Operation::Join(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    assert!(failure(&evaluation, boss).is_none());
    let t = tan(5.0);
    let (r10, r15) = (5.0 - 10.0 * t, 5.0 - 15.0 * t);
    let above = PI * 5.0 / 3.0 * (r10 * r10 + r10 * r15 + r15 * r15);
    assert_close(evaluation.bodies[0].solid.volume(), the_plate() + above);
}

#[test]
#[ignore = "kernel taper not built"]
fn a_tapered_cut_through_all_narrows_through_the_plate() {
    let mut editor = Editor::new(Document::example());
    let cut = add_on(
        &mut editor,
        OriginPlane::XY,
        rectangle((-25.0, -10.0), (-12.0, 10.0)),
        Extent::ThroughAll,
        "3",
        Operation::Cut(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    assert!(failure(&evaluation, cut).is_none());
    // Narrowing up from the plate's bottom, on the sketch's plane.
    let hole = frustum(13.0, 20.0, tan(3.0), 10.0);
    assert_close(evaluation.bodies[0].solid.volume(), the_plate() - hole);
}

#[test]
#[ignore = "kernel taper not built"]
fn two_tapered_sides_narrow_both_ways() {
    let mut editor = Editor::new(Document::default());
    let extent = two_sides(editor.document(), "5", "3");
    let boss = add_on(
        &mut editor,
        OriginPlane::XY,
        rectangle((0.0, 0.0), (10.0, 10.0)),
        extent,
        "4",
        Operation::NewBody(BodyId::NEW),
    );
    let evaluation = evaluated(editor.document());
    assert!(failure(&evaluation, boss).is_none());
    let t = tan(4.0);
    let want = frustum(10.0, 10.0, t, 5.0) + frustum(10.0, 10.0, t, 3.0);
    assert_close(evaluation.bodies[0].solid.volume(), want);
}

#[test]
fn a_two_sided_taper_is_too_complex_with_the_kernel_stub() {
    let mut editor = Editor::new(Document::default());
    let extent = two_sides(editor.document(), "5", "3");
    let boss = add_on(
        &mut editor,
        OriginPlane::XY,
        rectangle((0.0, 0.0), (10.0, 10.0)),
        extent,
        "4",
        Operation::NewBody(BodyId::NEW),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(failure(&evaluation, boss).unwrap().message, TOO_COMPLEX);
    assert!(evaluation.bodies.is_empty());
}

/// A taper of zero (either sign), as a file may hold, is the untapered
/// extrude exactly: the same key, so found in the cache, and the same
/// solid, with the kernel's stand-in, which would refuse any other.
#[test]
fn a_zero_taper_is_the_untapered_extrude() {
    let mut editor = Editor::new(Document::example());
    let feature = editor.document().features()[1].id;
    let mut cache = Cache::default();
    let plain = evaluate(editor.document(), &mut cache);
    assert!(plain.failed.is_empty(), "{:?}", plain.failed);
    for text in ["0", "-0", "3 - 3"] {
        let zero = taper(editor.document(), text);
        assert_eq!(zero.value, 0.0);
        set_extrude(&mut editor, feature, |extrude| extrude.taper = Some(zero));
        let misses = cache.counts().1;
        let again = evaluate(editor.document(), &mut cache);
        assert!(again.failed.is_empty(), "{text}: {:?}", again.failed);
        assert_eq!(cache.counts().1, misses, "{text}: nothing worked out again");
        assert_eq!(again.bodies.len(), plain.bodies.len());
        for (a, b) in again.bodies.iter().zip(&plain.bodies) {
            assert_eq!(a.key, b.key);
            assert!(Arc::ptr_eq(&a.solid, &b.solid), "{text}: the same solid");
        }
        // Cold, the same solid bit for bit.
        let cold = evaluated(editor.document());
        assert_eq!(cold.bodies[0].solid.mesh(), plain.bodies[0].solid.mesh());
    }
}

// Bugs: the taper through the history.

/// A body 10 to 30 up from XY, away from the sketch's plane: a box
/// moved up by a move.
fn raised_block(editor: &mut Editor) -> BodyId {
    let body = super::motion::block(editor, -30.0, -20.0, 30.0, 20.0, "20");
    let ask = varde_document::Move::offset_ask(&editor.document().design());
    let offset = |text: &str| Value::new(text, &ask).unwrap();
    let moved = varde_document::Move {
        bodies: vec![body],
        offset: [offset("0"), offset("0"), offset("10")],
        turn: None,
    };
    super::motion::add(editor, moved);
    body
}

/// Through all, the walls still turn about the sketch's plane though
/// the body (and so the span) doesn't reach it: the hole through a body
/// 10 to 30 up is the frustum's slice from 10 to 30, not one starting
/// its narrowing at the body.
#[test]
fn a_tapered_cut_through_all_hinges_at_the_sketch_s_plane() {
    with_frustums();
    let mut editor = Editor::new(Document::default());
    let body = raised_block(&mut editor);
    let cut = add_tapered(
        &mut editor,
        (-10.0, -5.0),
        (10.0, 5.0),
        Extent::ThroughAll,
        "5",
        Operation::Cut(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_eq!(evaluation.touched, [(cut, vec![body])]);
    let t = tan(5.0);
    let hole = frustum(20.0, 10.0, t, 30.0) - frustum(20.0, 10.0, t, 10.0);
    let block = 60.0 * 40.0 * 20.0;
    assert_close(evaluation.bodies[0].solid.volume(), block - hole);

    // Steep enough to close past the body's top, within the span's
    // margin: refused, the body left whole.
    let steep = taper(editor.document(), "9.4");
    // 10 / (2 tan 9.4°) ≈ 30.2, short of the span's end, 31.2.
    assert!(10.0 - 2.0 * 30.2 * tan(9.4) < 0.1);
    set_extrude(&mut editor, cut, |extrude| extrude.taper = Some(steep));
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, cut).expect("it closes");
    assert!(
        failed.message.starts_with("the taper closes"),
        "{}",
        failed.message
    );
    assert_close(evaluation.bodies[0].solid.volume(), block);
}

/// Flipped, one side runs down from the plane, and the taper narrows
/// it downwards just the same.
#[test]
fn a_flipped_taper_narrows_away_from_the_plane() {
    with_frustums();
    let mut editor = Editor::new(Document::default());
    let extent = Extent::OneSide(length(editor.document(), "5"));
    let boss = add_tapered(
        &mut editor,
        (0.0, 0.0),
        (10.0, 8.0),
        extent,
        "10",
        Operation::NewBody(BodyId::NEW),
    );
    set_extrude(&mut editor, boss, |extrude| extrude.flip = true);
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let solid = &evaluation.bodies[0].solid;
    assert_close(solid.volume(), frustum(10.0, 8.0, tan(10.0), 5.0));
    let bounds = solid.bounds3().unwrap();
    assert_close(bounds.min.z, -5.0);
    assert_close(bounds.max.z, 0.0);
}

/// Symmetric and two sides span the sketch's plane, whose walls only
/// the kernel's taper splits there: both too complex with the stand-in
/// and the kernel's stub alike, making nothing; the rest goes on.
#[test]
fn tapers_across_the_plane_fail_and_change_nothing() {
    for frustums in [false, true] {
        if frustums {
            with_frustums();
        }
        let editor = Editor::new(Document::example());
        let extents = [
            Extent::Symmetric(length(editor.document(), "6")),
            two_sides(editor.document(), "4", "1"),
        ];
        for extent in extents {
            let mut editor = editor.clone();
            let cut = add_tapered(
                &mut editor,
                (-25.0, -10.0),
                (-12.0, 10.0),
                extent,
                "3",
                Operation::Cut(Targets::default()),
            );
            let pocket = add_pocket(&mut editor);
            let evaluation = evaluated(editor.document());
            assert_eq!(failure(&evaluation, cut).unwrap().message, TOO_COMPLEX);
            assert!(failure(&evaluation, pocket).is_none());
            assert_close(evaluation.bodies[0].solid.volume(), the_plate() - POCKET);
        }
    }
}

/// Steep tapers either way: narrowing near a right angle closes the
/// profile, widening near one takes it out of range, each worded and
/// making nothing; a hair of a taper is a frustum a hair off the box.
#[test]
fn steep_and_slight_tapers() {
    with_frustums();
    let mut editor = Editor::new(Document::example());
    let extent = Extent::OneSide(length(editor.document(), "5"));
    let boss = add_tapered(
        &mut editor,
        (35.0, -5.0),
        (45.0, 5.0),
        extent,
        "1",
        Operation::NewBody(BodyId::NEW),
    );
    let cases = [
        (
            "89.9999",
            Some("the taper closes the profile before its end"),
        ),
        ("-89.9999999", Some("the taper widens it out of range")),
        ("26.5", None),
        ("-89", None),
        ("1e-9", None),
        ("-1e-9", None),
    ];
    for (text, refused) in cases {
        let value = taper(editor.document(), text);
        let t = tan(text.parse().unwrap());
        set_extrude(&mut editor, boss, |extrude| extrude.taper = Some(value));
        let evaluation = evaluated(editor.document());
        assert_close(evaluation.bodies[0].solid.volume(), the_plate());
        match refused {
            Some(message) => {
                let failed = failure(&evaluation, boss).expect(text);
                assert!(
                    failed.message.starts_with(message),
                    "{text}: {}",
                    failed.message
                );
                assert_eq!(evaluation.bodies.len(), 1, "{text}");
            }
            None => {
                assert!(failure(&evaluation, boss).is_none(), "{text}");
                let volume = evaluation.bodies[1].solid.volume();
                assert_close(volume, frustum(10.0, 10.0, t, 5.0));
            }
        }
    }
}

/// Warm and cold agree: evaluating through a cache as the taper changes
/// gives the solids a fresh evaluation does, bit for bit.
#[test]
fn a_taper_s_history_warm_is_as_cold() {
    with_frustums();
    let mut editor = Editor::new(Document::example());
    let extent = Extent::OneSide(length(editor.document(), "4"));
    let cut = add_tapered(
        &mut editor,
        (-25.0, -10.0),
        (-12.0, 10.0),
        extent,
        "10",
        Operation::Cut(Targets::default()),
    );
    let mut cache = Cache::default();
    for text in ["10", "-5", "20", "10", "0", "45"] {
        let value = taper(editor.document(), text);
        set_extrude(&mut editor, cut, |extrude| extrude.taper = Some(value));
        let warm = evaluate(editor.document(), &mut cache);
        let cold = evaluated(editor.document());
        assert_eq!(warm.failed.len(), cold.failed.len(), "{text}");
        for (a, b) in warm.bodies.iter().zip(&cold.bodies) {
            assert_eq!(a.key, b.key, "{text}");
            assert_eq!(a.solid.mesh(), b.solid.mesh(), "{text}");
        }
    }
    // Undone all the way, each step back as it was.
    for _ in 0..8 {
        editor.undo();
        let warm = evaluate(editor.document(), &mut cache);
        let cold = evaluated(editor.document());
        for (a, b) in warm.bodies.iter().zip(&cold.bodies) {
            assert_eq!(a.solid.mesh(), b.solid.mesh());
        }
    }
}
