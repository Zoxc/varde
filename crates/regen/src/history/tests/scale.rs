//! Scales in the history: a block scaled about the origin and about its
//! corner (to the bit), a cylinder per axis (its rim an ellipse), a join
//! flush with a scaled block, a torus scaled up (kept, its fitted faces
//! counted), a block scaled so an edge has a length (uniform and along
//! its axis only), a hole's rim scaled to a circumference, the length
//! kept through an upstream edit and undo, each refusal, and the cache
//! keyed by the motion.

use glam::DVec3;
use varde_document::{BodyOp, Combine, EdgeRef, PointRef, Scale, ScaleFactor};
use varde_kernel::measure::{EdgeShape, Measured, Pick, Target, measure};
use varde_kernel::mesh::{FaceKey, Form, Surface};
use varde_kernel::{Budget, Tolerance};

use super::motion::{
    add, block, boxed, failure, key_on, key_where, near_box, polygon, revolved, set,
};
use super::*;

/// A document in millimetres, for values typed in them.
fn mm() -> Document {
    Document::default()
}

fn factor(document: &Document, text: &str) -> Value {
    Value::new(text, &Scale::factor_ask(&document.design())).unwrap()
}

/// `bodies` scaled by `text` about `about`.
fn uniform(document: &Document, bodies: &[BodyId], about: PointRef, text: &str) -> Scale {
    Scale {
        bodies: bodies.to_vec(),
        about,
        factor: ScaleFactor::Uniform(factor(document, text)),
    }
}

/// `bodies` scaled about the origin so `edge` is `text` long, along its
/// axis only if `axis_only`.
fn to_length(
    document: &Document,
    bodies: &[BodyId],
    edge: EdgeRef,
    text: &str,
    axis_only: bool,
) -> Scale {
    Scale {
        bodies: bodies.to_vec(),
        about: PointRef::Origin,
        factor: ScaleFactor::EdgeLength {
            edge,
            length: length(document, text),
            axis_only,
        },
    }
}

/// The edge of `body` between the faces of the keys `a` and `b`.
fn edge(body: BodyId, a: FaceKey, b: FaceKey, near: DVec3) -> EdgeRef {
    EdgeRef {
        body,
        faces: [a.min(b), a.max(b)],
        near,
    }
}

/// The edge of `body` (whose solid is `solid`) between its flat faces on
/// the planes `a` and `b`, `n·x = d`.
fn flat_edge(
    solid: &Solid,
    body: BodyId,
    a: (DVec3, f64),
    b: (DVec3, f64),
    near: DVec3,
) -> EdgeRef {
    edge(body, key_on(solid, a.0, a.1), key_on(solid, b.0, b.1), near)
}

/// What the measure tool says of `edge` on `solid`.
fn measured(solid: &Solid, edge: &EdgeRef) -> varde_kernel::measure::EdgeMeasure {
    let topology = solid.topology();
    let chain = topology.edge(solid, edge.faces, edge.near).unwrap();
    let target = Target {
        solid,
        topology: &topology,
        pick: Pick::Edge(chain),
    };
    match measure(&target, &Tolerance::default(), &Budget::DEFAULT).unwrap() {
        Measured::Edge(edge) => edge,
        other => panic!("{other:?}"),
    }
}

fn close(a: f64, b: f64, relative: f64) -> bool {
    (a - b).abs() <= relative * b.abs()
}

/// What `evaluation` noted of the one scale in it.
fn noted(evaluation: &Evaluation, feature: FeatureId) -> crate::ScaleFound {
    let [(found, scaled)] = evaluation.scaled[..] else {
        panic!("{:?}", evaluation.scaled);
    };
    assert_eq!(found, feature);
    scaled
}

/// × 2 about the origin doubles every coordinate, to the bit; about the
/// block's top corner the corner stays put; × 0.5 about it halves the
/// block towards it. Volumes are the analytic ones.
#[test]
fn a_block_scaled_about_the_origin_and_about_a_corner() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let solid = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
    let id = add(&mut editor, uniform(&mm(), &[a], PointRef::Origin, "2"));
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let scaled = solid_of(&evaluation, a);
    assert!(boxed(scaled, [0.0; 3], [20.0; 3]));
    assert_near(scaled.volume(), 8000.0);
    let found = noted(&evaluation, id);
    assert_eq!(found.centre, Some([0.0; 3]));
    assert_eq!(found.factors, Some([2.0; 3]));
    assert_eq!((found.length, found.fitted), (None, 0));
    // About the top corner at (10, 10, 10).
    let mut faces = [DVec3::X, DVec3::Y, DVec3::Z].map(|n| key_on(&solid, n, 10.0));
    faces.sort();
    let corner = PointRef::Corner {
        body: a,
        faces,
        near: DVec3::splat(10.0),
    };
    set(&mut editor, id, uniform(&mm(), &[a], corner, "2"));
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert!(boxed(solid_of(&evaluation, a), [-10.0; 3], [10.0; 3]));
    assert_near(solid_of(&evaluation, a).volume(), 8000.0);
    assert_eq!(noted(&evaluation, id).centre, Some([10.0; 3]));
    set(&mut editor, id, uniform(&mm(), &[a], corner, "1/2"));
    let evaluation = evaluated(editor.document());
    assert!(boxed(solid_of(&evaluation, a), [5.0; 3], [10.0; 3]));
    assert_near(solid_of(&evaluation, a).volume(), 125.0);
    // Undone, the block is as it was.
    editor.undo();
    editor.undo();
    editor.undo();
    let evaluation = evaluated(editor.document());
    assert!(boxed(solid_of(&evaluation, a), [0.0; 3], [10.0; 3]));
    assert!(evaluation.scaled.is_empty());
}

/// A cylinder scaled × 2 along Y only: twice the volume, its walls a
/// conic cylinder, its rim an ellipse of semi-axes 10 and 5, whose
/// length the measure tool gives.
#[test]
fn a_cylinder_scaled_per_axis_has_elliptic_rims() {
    let mut editor = Editor::new(Document::default());
    let pin = add_body(&mut editor, disc((0.0, 0.0), 5.0), "10");
    let scale = Scale {
        bodies: vec![pin],
        about: PointRef::Origin,
        factor: ScaleFactor::PerAxis(["1", "2", "1"].map(|t| factor(&mm(), t))),
    };
    let id = add(&mut editor, scale);
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let solid = solid_of(&evaluation, pin);
    assert_near(solid.volume(), 2.0 * PI * 25.0 * 10.0);
    assert!(near_box(solid, [-5.0, -10.0, 0.0], [5.0, 10.0, 10.0]));
    assert_eq!(noted(&evaluation, id).factors, Some([1.0, 2.0, 1.0]));
    let wall = key_where(solid, |form| matches!(form, Form::ConicCylinder { .. }));
    let rim = edge(
        pin,
        key_on(solid, DVec3::Z, 10.0),
        wall,
        DVec3::new(0.0, 10.0, 10.0),
    );
    let rim = measured(solid, &rim);
    let EdgeShape::Ellipse { major, minor, .. } = rim.shape else {
        panic!("{:?}", rim.shape);
    };
    assert!(
        close(major, 10.0, 1e-12) && close(minor, 5.0, 1e-12),
        "{major} {minor}"
    );
    // The perimeter of an ellipse of semi-axes 10 and 5.
    assert!(
        close(rim.length, 48.442_241_102_739_24, 1e-12),
        "{}",
        rim.length
    );
}

/// A block scaled × 2 about the origin is flush with a block made after
/// it beside its new place, and a union of the two is one block.
#[test]
fn a_join_after_a_scale_is_flush_with_the_scaled_block() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    add(&mut editor, uniform(&mm(), &[a], PointRef::Origin, "2"));
    let b = block(&mut editor, 20.0, 0.0, 30.0, 20.0, "20");
    let combine = Combine {
        target: a,
        tools: vec![b],
        op: BodyOp::Union,
        keep_tools: false,
    };
    add(&mut editor, combine);
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_eq!(evaluation.merged, [(b, a)]);
    let joined = solid_of(&evaluation, a);
    assert_near(joined.volume(), 8000.0 + 4000.0);
    assert!(boxed(joined, [0.0; 3], [30.0, 20.0, 20.0]));
}

/// A revolved torus (fitted faces) scaled × 25.4 is kept: its volume
/// the unscaled one's × 25.4³, its fitted faces' slack × 25.4, and the
/// note counts them.
#[test]
fn a_torus_scaled_up_is_kept_with_its_fitted_faces_counted() {
    let mut editor = Editor::new(Document::default());
    let ring = revolved(&mut editor, disc((20.0, 0.0), 5.0));
    let before = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
    let free = |solid: &Solid| {
        (solid.mesh().faces().iter())
            .filter(|face| matches!(face.surface, Surface::Free))
            .map(|face| face.slack)
            .collect::<Vec<_>>()
    };
    assert!(!free(&before).is_empty(), "a torus is fitted");
    let id = add(
        &mut editor,
        uniform(&mm(), &[ring], PointRef::Origin, "25.4"),
    );
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let scaled = solid_of(&evaluation, ring);
    let f: f64 = 25.4;
    assert!(close(scaled.volume(), before.volume() * f.powi(3), 1e-9));
    assert!(close(before.volume(), 2.0 * PI * PI * 20.0 * 25.0, 1e-3));
    let slack = free(scaled);
    assert!(slack.iter().all(|&s| s >= f), "{slack:?}");
    let found = noted(&evaluation, id);
    assert!(found.fitted > 0);
    assert_eq!(found.factors, Some([f; 3]));
}

/// A 10 × 20 × 5 block scaled so its 20 edge along Y is 50: uniform,
/// every length × 2.5; along its axis only, Y alone. An X edge to 50
/// stretches X alone.
#[test]
fn a_block_scaled_so_an_edge_has_a_length() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 20.0, "5");
    let solid = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
    let along_y = flat_edge(
        &solid,
        a,
        (DVec3::X, 10.0),
        (DVec3::Z, 5.0),
        DVec3::new(10.0, 10.0, 5.0),
    );
    let id = add(&mut editor, to_length(&mm(), &[a], along_y, "50", false));
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let scaled = solid_of(&evaluation, a);
    assert!(near_box(scaled, [0.0; 3], [25.0, 50.0, 12.5]));
    assert_near(scaled.volume(), 1000.0 * 2.5f64.powi(3));
    let found = noted(&evaluation, id);
    assert_eq!(found.length, Some(20.0));
    assert_eq!(found.factors, Some([2.5; 3]));
    assert!(close(measured(scaled, &along_y).length, 50.0, 1e-12));
    // Along Y only.
    set(&mut editor, id, to_length(&mm(), &[a], along_y, "50", true));
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let scaled = solid_of(&evaluation, a);
    assert!(boxed(scaled, [0.0; 3], [10.0, 50.0, 5.0]));
    assert_near(scaled.volume(), 2500.0);
    assert_eq!(noted(&evaluation, id).factors, Some([1.0, 2.5, 1.0]));
    // An X edge: X alone.
    let along_x = flat_edge(
        &solid,
        a,
        (-DVec3::Y, 0.0),
        (DVec3::Z, 5.0),
        DVec3::new(5.0, 0.0, 5.0),
    );
    set(&mut editor, id, to_length(&mm(), &[a], along_x, "50", true));
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let scaled = solid_of(&evaluation, a);
    assert!(boxed(scaled, [0.0; 3], [50.0, 20.0, 5.0]));
    assert_eq!(measured(scaled, &along_x).length, 50.0);
}

/// The plate scaled so its hole's rim, a whole circle of radius 8, is
/// 100 round: the rim measured after is 100 to `1e-12` relative.
#[test]
fn a_hole_s_rim_is_scaled_to_a_circumference() {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let solid = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
    let wall = key_where(&solid, |form| matches!(form, Form::Cylinder { .. }));
    let rim = edge(
        plate,
        key_on(&solid, DVec3::Z, 10.0),
        wall,
        DVec3::new(8.0, 0.0, 10.0),
    );
    let id = add(&mut editor, to_length(&mm(), &[plate], rim, "100", false));
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let found = noted(&evaluation, id);
    assert!(close(found.length.unwrap(), 16.0 * PI, 1e-13));
    let scaled = solid_of(&evaluation, plate);
    let after = measured(scaled, &rim);
    assert!(close(after.length, 100.0, 1e-12), "{}", after.length);
    let f = 100.0 / (16.0 * PI);
    assert!(close(
        scaled.volume(),
        plate_volume(8.0, 10.0) * f.powi(3),
        1e-9
    ));
    // Along its axis only: a rim isn't straight.
    set(
        &mut editor,
        id,
        to_length(&mm(), &[plate], rim, "100", true),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        "its edge isn't straight, so it can't scale along its axis only"
    );
    assert!(boxed(
        solid_of(&evaluation, plate),
        [-30.0, -20.0, 0.0],
        [30.0, 20.0, 10.0]
    ));
}

/// The plate scaled along Z so its upright edge is 50 keeps that height
/// when the plate is extruded twice as far upstream, and on undo.
#[test]
fn an_upstream_edit_keeps_the_typed_length() {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let extrude = editor.document().features()[1].id;
    let solid = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
    let upright = flat_edge(
        &solid,
        plate,
        (DVec3::X, 30.0),
        (DVec3::Y, 20.0),
        DVec3::new(30.0, 20.0, 5.0),
    );
    let id = add(&mut editor, to_length(&mm(), &[plate], upright, "50", true));
    let tall = |editor: &Editor, made: f64| {
        let evaluation = evaluated(editor.document());
        assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
        let scaled = solid_of(&evaluation, plate);
        assert!(near_box(scaled, [-30.0, -20.0, 0.0], [30.0, 20.0, 50.0]));
        assert_near(scaled.volume(), plate_volume(8.0, 50.0));
        let found = noted(&evaluation, id);
        assert_eq!(found.length, Some(made));
        assert_eq!(found.factors, Some([1.0, 1.0, 50.0 / made]));
    };
    tall(&editor, 10.0);
    let twenty = length(editor.document(), "20");
    set_extrude(&mut editor, extrude, |extrude| {
        extrude.extent = Extent::OneSide(twenty);
    });
    tall(&editor, 20.0);
    editor.undo();
    tall(&editor, 10.0);
}

/// The volume of the example plate, `height` thick with a hole of
/// `radius`.
fn plate_volume(radius: f64, height: f64) -> f64 {
    (60.0 * 40.0 - PI * radius * radius) * height
}

/// Each refusal, changing no body: an edge whose faces don't meet, a
/// length more than a thousand times the edge's (either way), along its
/// axis only for a slanted edge, out of range, and a scale down taking
/// a thin plate under the resolution.
#[test]
fn refusals() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let far = block(&mut editor, 1000.0, 0.0, 1010.0, 10.0, "10");
    let slanted: &'static [(f64, f64)] = &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 5.0)];
    let wedge = add_body(&mut editor, polygon(slanted), "5");
    let solid = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
    let unmoved = |editor: &Editor, id: FeatureId, message: &str| {
        let evaluation = evaluated(editor.document());
        assert_eq!(failure(&evaluation, id).unwrap().message, message);
        assert!(boxed(solid_of(&evaluation, a), [0.0; 3], [10.0; 3]));
    };
    // The top and bottom don't meet.
    let apart = flat_edge(
        &solid,
        a,
        (DVec3::Z, 10.0),
        (-DVec3::Z, 0.0),
        DVec3::new(5.0, 5.0, 5.0),
    );
    let id = add(&mut editor, to_length(&mm(), &[a], apart, "20", false));
    unmoved(&editor, id, "its edge wasn't found");
    let top = flat_edge(
        &solid,
        a,
        (DVec3::X, 10.0),
        (DVec3::Z, 10.0),
        DVec3::new(10.0, 5.0, 10.0),
    );
    let too_far =
        "the length is too far from the edge's: it would scale by more than a thousand times";
    for text in ["10001", "0.009"] {
        set(&mut editor, id, to_length(&mm(), &[a], top, text, false));
        unmoved(&editor, id, too_far);
    }
    // The bounds themselves scale.
    for (text, f) in [("10000", 1e3), ("0.01", 1e-3)] {
        set(&mut editor, id, to_length(&mm(), &[a], top, text, false));
        let evaluation = evaluated(editor.document());
        assert!(failure(&evaluation, id).is_none(), "{text}");
        assert_eq!(noted(&evaluation, id).factors, Some([f; 3]));
    }
    // Out of range: × 1000 about a point 1000 out takes the block past
    // the limit.
    let mut faces = [DVec3::X, DVec3::Y, DVec3::Z].map(|n| key_on(&solid, n, 10.0));
    faces.sort();
    set(
        &mut editor,
        id,
        uniform(
            &mm(),
            &[a],
            PointRef::Corner {
                body: a,
                faces,
                near: DVec3::splat(10.0),
            },
            "1000",
        ),
    );
    let evaluation = evaluated(editor.document());
    assert!(
        failure(&evaluation, id).is_none(),
        "{:?}",
        evaluation.failed
    );
    set(
        &mut editor,
        id,
        uniform(&mm(), &[a, far], PointRef::Origin, "1000"),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        "scaling Body 2 takes it out of range: every part must stay within 1000000 mm of the \
         origin"
    );
    assert!(boxed(solid_of(&evaluation, a), [0.0; 3], [10.0; 3]));
    // A slanted edge, along its axis only.
    let evaluation = evaluated(editor.document());
    let wedge_solid = solid_of(&evaluation, wedge);
    let side = key_where(
        wedge_solid,
        |form| matches!(*form, Form::Plane { n, .. } if n.x != 0.0 && n.y != 0.0),
    );
    let slope = edge(
        wedge,
        key_on(wedge_solid, DVec3::Z, 5.0),
        side,
        DVec3::new(5.0, 7.5, 5.0),
    );
    set(
        &mut editor,
        id,
        to_length(&mm(), &[wedge], slope, "20", true),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        "its edge isn't along an axis any more, so it can't scale along it only"
    );
    // Uniform, it scales by 20 over its length.
    set(
        &mut editor,
        id,
        to_length(&mm(), &[wedge], slope, "20", false),
    );
    let evaluation = evaluated(editor.document());
    assert!(
        failure(&evaluation, id).is_none(),
        "{:?}",
        evaluation.failed
    );
    let f = 20.0 / 125f64.sqrt();
    assert!(close(noted(&evaluation, id).factors.unwrap()[0], f, 1e-15));
}

/// A plate a micrometre thick scaled by a thousandth is thinner than the
/// resolution: refused as too small for the tolerance.
#[test]
fn a_scale_down_under_the_resolution_is_refused() {
    let mut editor = Editor::new(Document::default());
    let thin = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "0.001");
    let id = add(
        &mut editor,
        uniform(&mm(), &[thin], PointRef::Origin, "0.001"),
    );
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, id).map(|failed| failed.message.as_str());
    assert_eq!(
        failed,
        Some(
            "scaling Body 1 leaves no clean solid: parts of it come too close together, or get \
             too small, for the tolerance; try a finer tolerance"
        )
    );
    assert!(boxed(
        solid_of(&evaluation, thin),
        [0.0; 3],
        [10.0, 10.0, 0.001]
    ));
}

/// An edge length giving the factor a typed one gave finds the scaled
/// body in the cache; another factor scales it again.
#[test]
fn an_unchanged_factor_is_found_in_the_cache() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let solid = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
    let id = add(&mut editor, uniform(&mm(), &[a], PointRef::Origin, "2"));
    let mut cache = Cache::default();
    cache.begin();
    let first = evaluate(editor.document(), &mut cache);
    let top = flat_edge(
        &solid,
        a,
        (DVec3::X, 10.0),
        (DVec3::Z, 10.0),
        DVec3::new(10.0, 5.0, 10.0),
    );
    set(&mut editor, id, to_length(&mm(), &[a], top, "20", false));
    cache.begin();
    let same = evaluate(editor.document(), &mut cache);
    assert!(Arc::ptr_eq(&first.bodies[0].solid, &same.bodies[0].solid));
    set(&mut editor, id, to_length(&mm(), &[a], top, "30", false));
    cache.begin();
    let other = evaluate(editor.document(), &mut cache);
    assert!(!Arc::ptr_eq(&first.bodies[0].solid, &other.bodies[0].solid));
    assert_near(other.bodies[0].solid.volume(), 27000.0);
}

/// A scale's draft answers what it found, its edge's length too where
/// the length typed is too far from it, and the point it scales about;
/// a point on a body merged into another is found on that one, and one
/// whose faces don't meet isn't found.
#[test]
fn a_draft_answers_what_the_scale_found() {
    use crate::Draft;
    use crate::tests::{answered, regenerate_with};
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let solid = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
    let top = flat_edge(
        &solid,
        a,
        (DVec3::X, 10.0),
        (DVec3::Z, 10.0),
        DVec3::new(10.0, 5.0, 10.0),
    );
    let draft = |scale: Scale| Draft {
        revision: 3,
        feature: None,
        kind: scale.into(),
    };
    let far = draft(to_length(&mm(), &[a], top, "20000", false));
    let answer = answered(crate::handle(regenerate_with(&editor, Some(far))));
    let drafted = answer.draft.unwrap();
    assert_eq!(
        drafted.error.as_deref(),
        Some("the length is too far from the edge's: it would scale by more than a thousand times")
    );
    let found = *drafted.scale.unwrap();
    assert_eq!(found.centre, Some([0.0; 3]));
    assert_eq!(found.length, Some(10.0));
    assert_eq!(found.factors, None);
    let good = draft(to_length(&mm(), &[a], top, "25", false));
    let answer = answered(crate::handle(regenerate_with(&editor, Some(good))));
    let drafted = answer.draft.unwrap();
    assert_eq!(drafted.error, None);
    assert_eq!(drafted.scale.unwrap().factors, Some([2.5; 3]));
    // About the corner of a body a combine consumed: found on the target.
    let b = block(&mut editor, 20.0, 0.0, 30.0, 10.0, "10");
    let mut faces = [DVec3::X, DVec3::Y, DVec3::Z].map(|n| key_on(&solid, n, 10.0));
    faces.sort();
    add(
        &mut editor,
        Combine {
            target: b,
            tools: vec![a],
            op: BodyOp::Union,
            keep_tools: false,
        },
    );
    let about = PointRef::Corner {
        body: a,
        faces,
        near: DVec3::splat(10.0),
    };
    let id = add(&mut editor, uniform(&mm(), &[b], about, "2"));
    let evaluation = evaluated(editor.document());
    assert!(
        failure(&evaluation, id).is_none(),
        "{:?}",
        evaluation.failed
    );
    assert_eq!(noted(&evaluation, id).centre, Some([10.0; 3]));
    let mut apart = [
        key_on(&solid, DVec3::X, 10.0),
        key_on(&solid, -DVec3::X, 0.0),
        key_on(&solid, DVec3::Z, 10.0),
    ];
    apart.sort();
    let about = PointRef::Corner {
        body: a,
        faces: apart,
        near: DVec3::splat(10.0),
    };
    set(&mut editor, id, uniform(&mm(), &[b], about, "2"));
    let evaluation = evaluated(editor.document());
    assert_eq!(
        failure(&evaluation, id).unwrap().message,
        "its point wasn't found"
    );
    let found = noted(&evaluation, id);
    assert_eq!((found.centre, found.factors), (None, Some([2.0; 3])));
}
