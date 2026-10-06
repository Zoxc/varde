//! Shells in the history. The kernel's shell isn't built yet (it fails
//! as too complex), so: a shell reaching the kernel fails with the
//! too-complex message, changing no body, and the rest of the history
//! goes on; regen's own refusal comes first (an open face not found,
//! after the feature that made it is removed), and the open faces are
//! found again after an upstream dimension changes. With the kernel's
//! shell replaced by a stand-in hollowing boxes by another box, what
//! regeneration hands it and does with the result: closed, open and
//! outward shells by their volumes, a face named twice handed over once,
//! too thick refused, other refusals worded and drawn, the cache. The
//! analytic volumes of the kernel's shell (a box open or closed, a
//! rounded plate, a boss on a plate, outward, too thick, the same bits
//! twice) are written out, ignored until the kernel's shell is built.

use std::cell::RefCell;

use glam::DVec3;
use varde_document::{FaceRef, Shell};
use varde_kernel::mesh::{FaceKey, Form};
use varde_kernel::{ShellError, Topology};

use super::motion::{add, block, failure, key_on, key_where, set};
use super::*;

/// Shells on this test's thread by the box stand-in.
fn with_boxes() {
    super::super::shell::shell_by_boxes();
}

/// A thickness of `text` for a shell in `document`.
fn thickness(document: &Document, text: &str) -> Value {
    Value::new(text, &Shell::thickness_ask(&document.design())).unwrap()
}

/// The face of `body` keyed `key`, picked at `near`.
fn face(body: BodyId, key: FaceKey, near: [f64; 3]) -> FaceRef {
    FaceRef {
        body,
        key,
        near: DVec3::from(near),
    }
}

/// A shell of `body` `size` thick inward, opening `open`.
fn shell(document: &Document, body: BodyId, mut open: Vec<FaceRef>, size: &str) -> Shell {
    open.sort_by(FaceRef::order);
    Shell {
        body,
        open,
        thickness: thickness(document, size),
        outward: false,
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

/// The cube's front face, at y = 0.
fn front(solid: &Solid, body: BodyId) -> FaceRef {
    face(body, key_on(solid, -DVec3::Y, 0.0), [5.0, 0.0, 5.0])
}

/// The message of the kernel's shell on `body`, which isn't built
/// yet.
fn not_built(body: &str) -> String {
    format!("shelling {body} isn't supported yet")
}

fn assert_near(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-9 * b.abs().max(1.0), "{a} vs {b}");
}

/// The volume of `body` in `editor`'s document regenerated, the shell
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

/// A shell the kernel can't do yet fails as too complex, leaving the
/// body as it was; the history goes on: a later join still works on the
/// body.
#[test]
fn a_shell_the_kernel_cant_do_fails_as_too_complex() {
    let (mut editor, body, solid) = cube();
    let kind = shell(editor.document(), body, vec![top(&solid, body)], "1");
    let id = add(&mut editor, kind);
    let extent = two_sides(editor.document(), "12", "1");
    let join = add_extrude(
        &mut editor,
        rectangle((8.0, 8.0), (12.0, 12.0)),
        extent,
        Operation::Join(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the shell fails");
    assert_eq!(failed.message, not_built("Body 1"));
    assert!(failure(&evaluation, join).is_none());
    assert_eq!(evaluation.failed.len(), 1);
    // The cube and the join's block, 4 × 4 × 13 less the 2 × 2 × 10
    // shared.
    assert_near(
        solid_of(&evaluation, body).volume(),
        1000.0 + 16.0 * 13.0 - 40.0,
    );
    // A closed one too.
    let closed = shell(editor.document(), body, Vec::new(), "1");
    set(&mut editor, id, closed);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        not_built("Body 1")
    );
}

/// An open face whose maker is removed (the cut that made a notch's
/// wall; the shell stays, as it names its body only) isn't found: the
/// shell fails before the kernel, the body left as it was; among
/// others, it's named by its place.
#[test]
fn an_open_face_that_is_gone_is_not_found() {
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
    // The notch's wall at x = 3, facing −x.
    let wall = key_where(
        notched,
        |form| matches!(*form, Form::Plane { n, d } if n.x < -0.5 && (d + 3.0).abs() < 1e-9),
    );
    let at = face(body, wall, [3.0, 1.5, 5.0]);
    let kind = shell(editor.document(), body, vec![at], "1");
    let id = add(&mut editor, kind);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        not_built("Body 1")
    );
    editor.apply(Command::RemoveFeature(notch)).unwrap();
    assert!(editor.document().feature(id).is_some());
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the shell fails");
    assert_eq!(failed.message, "its open face wasn't found");
    assert!(failed.geometry.is_none());
    assert_near(solid_of(&evaluation, body).volume(), 1000.0);
    let kind = shell(editor.document(), body, vec![at, top(&solid, body)], "1");
    let place = kind.open.iter().position(|f| *f == at).unwrap();
    set(&mut editor, id, kind);
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        format!("its open face {} of 2 wasn't found", place + 1)
    );
}

/// An upstream dimension change moves the open face; it's found again
/// by its name, and the shell opened through it where it went.
#[test]
fn an_open_face_is_found_again_after_an_upstream_change() {
    let (mut editor, body, solid) = cube();
    let kind = shell(editor.document(), body, vec![top(&solid, body)], "1");
    let id = add(&mut editor, kind);
    let maker = editor.document().body(body).unwrap().created_by;
    let twenty = Extent::OneSide(length(editor.document(), "20"));
    set_extrude(&mut editor, maker, |extrude| extrude.extent = twenty);
    // The kernel's: found, then too complex.
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        not_built("Body 1")
    );
    // The stand-in's: open where the top now is, 2000 less 8 × 8 × 19.
    with_boxes();
    let shelled = volume(&editor, id, body);
    assert_near(shelled, 2000.0 - 64.0 * 19.0);
    let evaluation = evaluated(editor.document());
    let bounds = solid_of(&evaluation, body).bounds3().unwrap();
    assert_near(bounds.max.z, 20.0);
}

/// Through the stand-in: a closed shell keeps a void inside, an open one
/// is opened through its faces, an outward one grows walls outside.
#[test]
fn shells_hollow_by_their_volumes() {
    with_boxes();
    let (mut editor, body, solid) = cube();
    let mm = editor.document().clone();
    // Closed: the void a second shell of the mesh, inside the first.
    let id = add(&mut editor, shell(&mm, body, Vec::new(), "1"));
    assert_near(volume(&editor, id, body), 1000.0 - 512.0);
    set(
        &mut editor,
        id,
        shell(&mm, body, vec![top(&solid, body)], "1"),
    );
    assert_near(volume(&editor, id, body), 1000.0 - 8.0 * 8.0 * 9.0);
    let two = vec![top(&solid, body), front(&solid, body)];
    set(&mut editor, id, shell(&mm, body, two, "1"));
    assert_near(volume(&editor, id, body), 1000.0 - 8.0 * 9.0 * 9.0);
    let outward = Shell {
        outward: true,
        ..shell(&mm, body, vec![top(&solid, body)], "1")
    };
    set(&mut editor, id, outward);
    // 12 × 12 × 11 less the cube.
    assert_near(volume(&editor, id, body), 12.0 * 12.0 * 11.0 - 1000.0);
}

thread_local! {
    /// What the recording stand-in was handed: the regions open, the
    /// thickness and the direction.
    static HANDED: RefCell<Vec<(Vec<u32>, f64, bool)>> = const { RefCell::new(Vec::new()) };
}

/// A stand-in that records what it's handed, and fails.
#[allow(clippy::too_many_arguments)]
fn recording(
    _: &Solid,
    _: &Topology,
    open: &[u32],
    thickness: f64,
    outward: bool,
    _: u64,
    _: &Tolerance,
    _: &Budget,
) -> Result<Solid, ShellError> {
    HANDED.with_borrow_mut(|handed| handed.push((open.to_vec(), thickness, outward)));
    Err(ShellError::TooThick)
}

/// Two references to one face (picked at two points) hand the kernel
/// that face once; the thickness in model units and the direction as
/// stored.
#[test]
fn a_face_named_twice_is_opened_once() {
    super::super::shell::SHELLER.set(Some(recording));
    let (mut editor, body, solid) = cube();
    let mm = editor.document().clone();
    let mut twice = top(&solid, body);
    twice.near = DVec3::new(1.0, 1.0, 10.0);
    let kind = Shell {
        outward: true,
        ..shell(
            &mm,
            body,
            vec![top(&solid, body), twice, front(&solid, body)],
            "2.5",
        )
    };
    add(&mut editor, kind);
    evaluated(editor.document());
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
    assert_eq!(handed, [(wanted, 2.5, true)]);
}

/// The stand-in's walls meeting inside: refused as too thick, worded
/// for the Timeline, the body left whole.
#[test]
fn too_thick_is_refused() {
    with_boxes();
    let (mut editor, body, solid) = cube();
    let mm = editor.document().clone();
    let id = add(&mut editor, shell(&mm, body, vec![top(&solid, body)], "5"));
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the shell fails");
    assert_eq!(
        failed.message,
        "the shell is too thick for Body 1: its walls would run into each other"
    );
    assert_near(solid_of(&evaluation, body).volume(), 1000.0);
    set(
        &mut editor,
        id,
        shell(&mm, body, vec![top(&solid, body)], "4.9"),
    );
    assert_near(volume(&editor, id, body), 1000.0 - 0.2 * 0.2 * 5.1);
}

/// A stand-in refusing as its region 0 too small a round.
#[allow(clippy::too_many_arguments)]
fn round_too_small(
    _: &Solid,
    _: &Topology,
    _: &[u32],
    _: f64,
    _: bool,
    _: u64,
    _: &Tolerance,
    _: &Budget,
) -> Result<Solid, ShellError> {
    Err(ShellError::RoundTooSmall { region: 0 })
}

/// A stand-in refusing at the solid's vertex 0.
#[allow(clippy::too_many_arguments)]
fn corner(
    _: &Solid,
    _: &Topology,
    _: &[u32],
    _: f64,
    _: bool,
    _: u64,
    _: &Tolerance,
    _: &Budget,
) -> Result<Solid, ShellError> {
    Err(ShellError::Corner { vertex: 0 })
}

/// The kernel's refusals are worded for the Timeline, with the face or
/// corner they're about drawn.
#[test]
fn refusals_are_worded_and_drawn() {
    let (mut editor, body, solid) = cube();
    let mm = editor.document().clone();
    let id = add(&mut editor, shell(&mm, body, vec![top(&solid, body)], "1"));
    super::super::shell::SHELLER.set(Some(round_too_small));
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the shell fails");
    assert_eq!(
        failed.message,
        "the shell is thicker than the smallest round of Body 1: try a thinner wall"
    );
    assert!(failed.geometry.is_some());
    super::super::shell::SHELLER.set(Some(corner));
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the shell fails");
    assert_eq!(
        failed.message,
        "faces of Body 1 meeting at a corner can't be offset together: try another thickness"
    );
    assert!(failed.geometry.is_some());
}

/// A shell that worked is found in the cache the next time.
#[test]
fn a_shell_that_worked_is_cached() {
    with_boxes();
    let (mut editor, body, solid) = cube();
    let kind = shell(editor.document(), body, vec![top(&solid, body)], "1");
    add(&mut editor, kind);
    let mut cache = Cache::default();
    let first = evaluate(editor.document(), &mut cache);
    let again = evaluate(editor.document(), &mut cache);
    assert!(Arc::ptr_eq(&first.bodies[0].solid, &again.bodies[0].solid));
    assert_near(first.bodies[0].solid.volume(), 424.0);
}

/// A shell of a body a combine took into another fails, naming where
/// it went, as every feature naming a consumed body does.
#[test]
fn a_consumed_body_fails_a_shell() {
    let (mut editor, body, _) = cube();
    let other = block(&mut editor, 5.0, 5.0, 15.0, 15.0, "10");
    let combine = varde_document::Combine {
        target: body,
        tools: vec![other],
        op: varde_document::BodyOp::Union,
        keep_tools: false,
    };
    add(&mut editor, combine);
    let mm = editor.document().clone();
    let id = add(&mut editor, shell(&mm, other, Vec::new(), "1"));
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).expect("the shell fails");
    assert_eq!(
        failed.message,
        "Body 2 is in Body 1 now: a feature before this one merged it in"
    );
}

// The kernel's shell, analytically: ignored until it's built.

/// A box open at its top, open at two faces, closed (a void), and
/// outward.
#[test]
#[ignore = "kernel shell not built"]
fn the_kernel_shells_a_box() {
    let (mut editor, body, solid) = cube();
    let mm = editor.document().clone();
    let id = add(&mut editor, shell(&mm, body, vec![top(&solid, body)], "1"));
    assert_near(volume(&editor, id, body), 1000.0 - 8.0 * 8.0 * 9.0);
    let two = vec![top(&solid, body), front(&solid, body)];
    set(&mut editor, id, shell(&mm, body, two, "1"));
    assert_near(volume(&editor, id, body), 1000.0 - 8.0 * 9.0 * 9.0);
    set(&mut editor, id, shell(&mm, body, Vec::new(), "1"));
    assert_near(volume(&editor, id, body), 1000.0 - 512.0);
    let outward = Shell {
        outward: true,
        ..shell(&mm, body, vec![top(&solid, body)], "1")
    };
    set(&mut editor, id, outward);
    assert_near(volume(&editor, id, body), 12.0 * 12.0 * 11.0 - 1000.0);
}

/// A slot-shaped plate 10 mm high (two lines 10 mm long, 6 mm apart,
/// joined by half circles) shelled 1 mm open at its top: its round ends
/// offset exactly to radius 2, `(60 + 9π)·10 − (40 + 4π)·9`.
#[test]
#[ignore = "kernel shell not built"]
fn the_kernel_shells_a_rounded_plate() {
    let mut editor = Editor::new(Document::default());
    let body = add_body(&mut editor, slot, "10");
    let evaluation = evaluated(editor.document());
    let solid = solid_of(&evaluation, body).clone();
    let top = face(body, key_on(&solid, DVec3::Z, 10.0), [5.0, 0.0, 10.0]);
    let mm = editor.document().clone();
    let id = add(&mut editor, shell(&mm, body, vec![top], "1"));
    let wanted = (60.0 + 9.0 * PI) * 10.0 - (40.0 + 4.0 * PI) * 9.0;
    let got = volume(&editor, id, body);
    assert!((got - wanted).abs() < 1e-6 * wanted, "{got} vs {wanted}");
}

/// Draws a slot: two lines 10 mm long, 6 mm apart, joined by half
/// circles at each end.
fn slot(sketch: &mut Sketch) {
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

/// A 20 mm square plate 5 mm thick with a round boss of radius 3 on it
/// 5 mm high, shelled 1 mm open at the plate's bottom: the hollow is
/// the plate's inside up to 4 mm and the boss's, radius 2, from there
/// up to 9 mm, so `2000 + 45π − 18·18·4 − 4π·5`.
#[test]
#[ignore = "kernel shell not built"]
fn the_kernel_shells_a_boss_on_a_plate() {
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
    let bottom = face(body, key_on(&solid, -DVec3::Z, 0.0), [10.0, 10.0, 0.0]);
    let mm = editor.document().clone();
    let id = add(&mut editor, shell(&mm, body, vec![bottom], "1"));
    let wanted = 2000.0 + 45.0 * PI - 18.0 * 18.0 * 4.0 - 20.0 * PI;
    let got = volume(&editor, id, body);
    assert!((got - wanted).abs() < 1e-6 * wanted, "{got} vs {wanted}");
}

/// Walls meeting inside are refused as too thick, closed or open.
#[test]
#[ignore = "kernel shell not built"]
fn the_kernel_refuses_too_thick() {
    let (mut editor, body, solid) = cube();
    let mm = editor.document().clone();
    for open in [Vec::new(), vec![top(&solid, body)]] {
        let id = add(&mut editor, shell(&mm, body, open, "5"));
        let evaluation = evaluated(editor.document());
        assert_eq!(
            failure(&evaluation, id).unwrap().message,
            "the shell is too thick for Body 1: its walls would run into each other"
        );
        editor.undo();
    }
}

/// The same shell twice, in fresh caches: the same bits.
#[test]
#[ignore = "kernel shell not built"]
fn the_kernel_shells_alike_every_time() {
    let (mut editor, body, solid) = cube();
    let mm = editor.document().clone();
    add(&mut editor, shell(&mm, body, vec![top(&solid, body)], "1"));
    let a = evaluated(editor.document());
    let b = evaluated(editor.document());
    let (a, b) = (solid_of(&a, body), solid_of(&b, body));
    assert_eq!(a.volume().to_bits(), b.volume().to_bits());
    assert_eq!(a.mesh().tris().len(), b.mesh().tris().len());
}

/// The stand-in against the volumes it should give, on a 10 × 6 × 4
/// box: every set of faces opened (opposite ones among them), inward
/// and outward, at thicknesses about half a side and past a side. Each
/// shell that works has its volume; one that can't (walls meeting, a
/// floor as thick as the body, nothing left) fails, never leaving the
/// body as it was. Only within a hair of meeting may either be.
#[test]
fn the_stand_in_shells_boxes_by_their_volumes() {
    with_boxes();
    let mut editor = Editor::new(Document::default());
    let body = block(&mut editor, 0.0, 0.0, 10.0, 6.0, "4");
    let evaluation = evaluated(editor.document());
    let solid = solid_of(&evaluation, body).clone();
    let size = [10.0, 6.0, 4.0];
    // Each face: its axis and end, and a point on it.
    let mut sides = Vec::new();
    for axis in 0..3 {
        for end in 0..2 {
            let mut n = DVec3::ZERO;
            n[axis] = if end == 0 { -1.0 } else { 1.0 };
            let d = if end == 0 { 0.0 } else { size[axis] };
            let mut near = DVec3::from(size) / 2.0;
            near[axis] = if end == 0 { 0.0 } else { size[axis] };
            sides.push((axis, face(body, key_on(&solid, n, d), near.into())));
        }
    }
    let mm = editor.document().clone();
    let id = add(&mut editor, shell(&mm, body, Vec::new(), "1"));
    // One cache, so the block is made once.
    let mut cache = Cache::default();
    let whole: f64 = size.iter().product();
    for mask in box_face_masks() {
        let open: Vec<FaceRef> = (0..6)
            .filter(|k| mask & (1 << k) != 0)
            .map(|k| sides[k].1)
            .collect();
        let mut closed = [2.0f64; 3];
        for k in (0..6).filter(|k| mask & (1 << k) != 0) {
            closed[sides[k].0] -= 1.0;
        }
        for text in [
            "0.5",
            "1.9999999",
            "2",
            "2.9999999",
            "3.0000001",
            "4",
            "4.9999999",
            "6",
            "10",
        ] {
            for outward in [false, true] {
                let t: f64 = text.parse().unwrap();
                let kind = Shell {
                    outward,
                    ..shell(&mm, body, open.clone(), text)
                };
                set(&mut editor, id, kind);
                cache.begin();
                let evaluation = evaluate(editor.document(), &mut cache);
                let (wanted, slack) = if outward {
                    let grown: f64 = (0..3).map(|a| size[a] + t * closed[a]).product();
                    (grown - whole, f64::INFINITY)
                } else {
                    let inner: Vec<f64> = (0..3).map(|a| size[a] - t * closed[a]).collect();
                    let slack = inner.iter().copied().fold(f64::INFINITY, f64::min);
                    let hollow = if slack > 0.0 {
                        inner.iter().product()
                    } else {
                        0.0
                    };
                    (whole - hollow, slack)
                };
                let what = format!("open {mask:06b} {text} outward {outward}");
                match failure(&evaluation, id) {
                    None => {
                        let got = solid_of(&evaluation, body).volume();
                        assert!(slack > 0.0, "{what}: worked, giving {got}");
                        assert!(
                            (got - wanted).abs() <= 1e-6 * whole,
                            "{what}: {got} vs {wanted}"
                        );
                        assert!(got < whole || outward, "{what}: nothing taken");
                    }
                    Some(failed) => {
                        let can = slack <= 1e-6 || wanted <= 1e-6;
                        assert!(can, "{what}: {}", failed.message);
                    }
                }
            }
        }
    }
}

mod fuzz;
