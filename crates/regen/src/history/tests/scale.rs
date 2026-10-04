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

use super::combine::fuzz::truncated;
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
    // An edge not found shows nothing.
    let evaluation = evaluated(editor.document());
    assert!(failure(&evaluation, id).unwrap().geometry.is_none());
    let top = flat_edge(
        &solid,
        a,
        (DVec3::X, 10.0),
        (DVec3::Z, 10.0),
        DVec3::new(10.0, 5.0, 10.0),
    );
    let too_far =
        "the length is too far from the edge's: more than a thousand times longer or shorter";
    for text in ["10001", "0.009"] {
        set(&mut editor, id, to_length(&mm(), &[a], top, text, false));
        unmoved(&editor, id, too_far);
        // The edge found is drawn where it fails: the 10 long top edge
        // along Y at x 10.
        let evaluation = evaluated(editor.document());
        let failed = failure(&evaluation, id).unwrap();
        let geometry = failed.geometry.as_ref().expect("the edge drawn");
        let points = geometry.lines().points();
        assert!(!points.is_empty());
        assert!(
            (points.iter()).all(|p| p[0] == 10.0 && p[2] == 10.0 && (0.0..=10.0).contains(&p[1])),
            "{points:?}"
        );
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
    assert!(failure(&evaluation, id).unwrap().geometry.is_some());
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
        Some("the length is too far from the edge's: more than a thousand times longer or shorter")
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

/// Draws the half disc of radius `r` about the origin right of the
/// sketch's Y axis, which a whole turn about that axis makes a sphere.
pub(super) fn half_disc(r: f64) -> impl FnOnce(&mut varde_sketch::Sketch) {
    move |sketch| {
        let point = |sketch: &mut varde_sketch::Sketch, x: f64, y: f64| {
            sketch.add_point(glam::DVec2::new(x, y)).unwrap()
        };
        let center = point(sketch, 0.0, 0.0);
        let start = point(sketch, 0.0, -r);
        let end = point(sketch, 0.0, r);
        sketch
            .add_curve(varde_sketch::Curve::Arc { center, start, end }, false)
            .unwrap();
        sketch
            .add_curve(
                varde_sketch::Curve::Line {
                    start: end,
                    end: start,
                },
                false,
            )
            .unwrap();
    }
}

/// The volume of the slab `|z| <= h` of the ellipsoid of semi-axes `a`,
/// `b` across it and `c` along Z: `π a b (2h − 2h³ / 3c²)`.
fn ellipsoid_slab(a: f64, b: f64, c: f64, h: f64) -> f64 {
    PI * a * b * (2.0 * h - 2.0 * h.powi(3) / (3.0 * c * c))
}

/// The ellipsoid a sphere of radius 10 scaled × 2, 1, 1/2 is, with a
/// feature after it: its volume `4/3 π abc` (the sphere's own, its pole
/// caps fitted, times the factors' product to rounding); the half
/// `x <= 0` a cut leaves, and its slabs `|x|`, `|y|`, `|z| <= 1` each
/// alone, are the analytic volumes to within the fit times the area of
/// the faces cut (plane cuts through a quadric are fitted), or the
/// feature is refused: never a wrong one.
#[test]
fn a_sphere_scaled_per_axis_cuts_as_an_ellipsoid() {
    let (a, b, c) = (20.0, 10.0, 5.0);
    let fit = Tolerance::default().fit();
    let ellipsoid = |editor: &mut Editor| {
        let ball = revolved(editor, half_disc(10.0));
        let scale = Scale {
            bodies: vec![ball],
            about: PointRef::Origin,
            factor: ScaleFactor::PerAxis(["2", "1", "1/2"].map(|t| factor(&mm(), t))),
        };
        add(editor, scale);
        ball
    };
    let mut editor = Editor::new(Document::default());
    let ball = ellipsoid(&mut editor);
    let sphere = solid_of(&evaluated(&truncated(editor.document(), 2)), ball).clone();
    // Its pole caps are fitted: within 1e-8 of the sphere's volume.
    assert!(close(sphere.volume(), 4.0 / 3.0 * PI * 1000.0, 1e-8));
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let solid = solid_of(&evaluation, ball);
    assert!(
        close(solid.volume(), sphere.volume(), 1e-12),
        "{}",
        solid.volume()
    );
    assert!(near_box(solid, [-a, -b, -c], [a, b, c]));
    let quadrics = (solid.mesh().faces().iter())
        .filter(|face| matches!(face.form, Form::Quadric { .. }))
        .count();
    assert!(quadrics > 0, "per axis, a sphere is a quadric");
    // Within the fit of `wanted` over cut faces of `area`, or refused
    // changing nothing.
    let held = |editor: &Editor, feature: FeatureId, wanted: f64, area: f64| {
        let evaluation = evaluated(editor.document());
        let v = solid_of(&evaluation, ball).volume();
        match failure(&evaluation, feature) {
            None => assert!((v - wanted).abs() <= fit * area, "{v} {wanted}"),
            Some(_) => assert!(close(v, sphere.volume(), 1e-12), "{v}"),
        }
    };
    let extent = two_sides(editor.document(), "10", "10");
    let cut = add_extrude(
        &mut editor,
        rectangle((0.0, -30.0), (30.0, 30.0)),
        extent,
        Operation::Cut(Targets::default()),
    );
    held(&editor, cut, 2.0 / 3.0 * PI * a * b * c, PI * b * c);
    // The slabs, each square to an axis: semi-axes across it, along it.
    for (plane, [p, q, r]) in [
        (OriginPlane::XY, [a, b, c]),
        (OriginPlane::XZ, [a, c, b]),
        (OriginPlane::YZ, [b, c, a]),
    ] {
        let mut editor = Editor::new(Document::default());
        ellipsoid(&mut editor);
        let extent = two_sides(editor.document(), "1", "1");
        let slab = super::super::tests::add_extrude_on(
            &mut editor,
            plane,
            rectangle((-30.0, -30.0), (30.0, 30.0)),
            extent,
            Operation::Intersect(Targets::default()),
        );
        let faces = 2.0 * PI * p * q * (1.0 - 1.0 / (r * r));
        held(&editor, slab, ellipsoid_slab(p, q, r, 1.0), faces);
    }
}

/// A cylinder of radius 5 scaled × 3 along X is an elliptic cylinder
/// (semi-axes 15 and 5); a block cut through it square to Y at y 2 and
/// a block joined across it each give the analytic volume.
#[test]
fn an_elliptic_cylinder_cuts_and_joins() {
    let mut editor = Editor::new(Document::default());
    let pin = add_body(&mut editor, disc((0.0, 0.0), 5.0), "10");
    let scale = Scale {
        bodies: vec![pin],
        about: PointRef::Origin,
        factor: ScaleFactor::PerAxis(["3", "1", "1"].map(|t| factor(&mm(), t))),
    };
    add(&mut editor, scale);
    let (a, b, h) = (15.0, 5.0, 10.0);
    let evaluation = evaluated(editor.document());
    assert!(close(
        solid_of(&evaluation, pin).volume(),
        PI * a * b * h,
        1e-12
    ));
    // Everything above y = 2 cut away: the segment left is the ellipse's
    // area below the chord y = 2 (in units of the unit circle scaled).
    let extent = two_sides(editor.document(), "20", "20");
    let cut = add_extrude(
        &mut editor,
        rectangle((-30.0, 2.0), (30.0, 30.0)),
        extent,
        Operation::Cut(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    // The unit circle's area below y = t: π/2 + t√(1−t²) + asin t.
    let t: f64 = 2.0 / b;
    let below = (PI / 2.0 + t * (1.0 - t * t).sqrt() + t.asin()) * a * b;
    assert!(
        failure(&evaluation, cut).is_none(),
        "{:?}",
        evaluation.failed
    );
    let v = solid_of(&evaluation, pin).volume();
    assert!(close(v, below * h, 1e-9), "{v} {}", below * h);
    // A block joined across its end at x 10 to 20, y -10 to 0.
    let extent = Extent::OneSide(length(editor.document(), "10"));
    let join = add_extrude(
        &mut editor,
        rectangle((10.0, -10.0), (20.0, 0.0)),
        extent,
        Operation::Join(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    assert!(
        failure(&evaluation, join).is_none(),
        "{:?}",
        evaluation.failed
    );
    // The ellipse's part in x >= 10, y <= 0 is a quarter of what's
    // beyond x = 10: (π/2 − s√(1−s²) − asin s)·ab/2 for s = 10/15.
    let s: f64 = 10.0 / a;
    let beyond = (PI / 2.0 - s * (1.0 - s * s).sqrt() - s.asin()) * a * b / 2.0;
    let wanted = (below + 100.0 - beyond) * h;
    let v = solid_of(&evaluation, pin).volume();
    assert!(close(v, wanted, 1e-9), "{v} {wanted}");
}

/// Factors at the bounds on curved and fitted bodies: a sphere and a
/// torus × 1000 and × 1/1000, and per axis 1000, 1, 1/1000. Each is
/// kept with its volume the product of the factors times the old one
/// (fitted faces' slack × the largest factor above 1), or refused as
/// too small or out of range, changing nothing.
#[test]
fn factors_at_the_bounds_on_curved_and_fitted_bodies() {
    for (draw, name) in [
        (
            Box::new(half_disc(10.0)) as Box<dyn FnOnce(&mut varde_sketch::Sketch)>,
            "sphere",
        ),
        (Box::new(disc((20.0, 0.0), 5.0)), "torus"),
    ] {
        let mut editor = Editor::new(Document::default());
        let body = revolved(&mut editor, draw);
        let before = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
        let id = add(&mut editor, uniform(&mm(), &[body], PointRef::Origin, "1"));
        for factors in [
            ["1000"; 3],
            ["1/1000"; 3],
            ["1000", "1", "1/1000"],
            ["1", "1000", "1000"],
        ] {
            let scale = Scale {
                bodies: vec![body],
                about: PointRef::Origin,
                factor: ScaleFactor::PerAxis(factors.map(|t| factor(&mm(), t))),
            };
            set(&mut editor, id, scale);
            let evaluation = evaluated(editor.document());
            let what = format!("{name} {factors:?}");
            let solid = solid_of(&evaluation, body);
            if let Some(failed) = failure(&evaluation, id) {
                eprintln!("{what}: {}", failed.message);
                assert_eq!(solid, &*before, "{what}");
                continue;
            }
            let f = noted(&evaluation, id).factors.unwrap();
            let product = f[0] * f[1] * f[2];
            let wanted = before.volume() * product;
            eprintln!("{what}: {} {}", solid.volume(), wanted);
            assert!(
                close(solid.volume(), wanted, 1e-9),
                "{what}: {} {wanted}",
                solid.volume()
            );
            let largest = f.into_iter().fold(1.0, f64::max);
            for (was, now) in before.mesh().faces().iter().zip(solid.mesh().faces()) {
                if matches!(now.surface, Surface::Free) {
                    assert!(
                        now.slack >= was.slack * largest,
                        "{what}: {} {}",
                        now.slack,
                        was.slack
                    );
                }
            }
        }
    }
}

/// A quad whose front edge runs from the origin to `(10, rise)`.
fn tilted(rise: f64) -> impl FnOnce(&mut varde_sketch::Sketch) {
    move |sketch| {
        let ids = [(0.0, 0.0), (10.0, rise), (10.0, 10.0), (0.0, 10.0)]
            .map(|(x, y)| sketch.add_point(glam::DVec2::new(x, y)).unwrap());
        for k in 0..4 {
            let (start, end) = (ids[k], ids[(k + 1) % 4]);
            sketch
                .add_curve(varde_sketch::Curve::Line { start, end }, false)
                .unwrap();
        }
    }
}

/// Along its axis only at the sine bound: a front edge rising 0.9e-8 over
/// 10 (a sine of 0.9e-9) scales along X alone and comes out 50 long to
/// `1e-12`; rising 1.1e-8 it's refused as off the axis, changing
/// nothing; uniform, it scales either way.
#[test]
fn along_its_axis_at_the_sine_bound() {
    for (rise, along) in [(0.9e-8, true), (1.1e-8, false), (-0.9e-8, true)] {
        let mut editor = Editor::new(Document::default());
        let quad = add_body(&mut editor, tilted(rise), "5");
        let solid = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
        let front = key_where(
            &solid,
            |form| matches!(*form, Form::Plane { n, .. } if n.y < -0.5),
        );
        let front = edge(
            quad,
            key_on(&solid, DVec3::Z, 5.0),
            front,
            DVec3::new(5.0, rise / 2.0, 5.0),
        );
        let was = measured(&solid, &front).length;
        let id = add(&mut editor, to_length(&mm(), &[quad], front, "50", true));
        let evaluation = evaluated(editor.document());
        let what = format!("{rise}");
        if !along {
            assert_eq!(
                failure(&evaluation, id).map(|f| f.message.as_str()),
                Some("its edge isn't along an axis any more, so it can't scale along it only"),
                "{what}"
            );
            assert_eq!(solid_of(&evaluation, quad), &*solid, "{what}");
            set(
                &mut editor,
                id,
                to_length(&mm(), &[quad], front, "50", false),
            );
            let evaluation = evaluated(editor.document());
            assert!(failure(&evaluation, id).is_none(), "{what}");
            let now = measured(solid_of(&evaluation, quad), &front).length;
            assert!(close(now, 50.0, 1e-12), "{what}: {now}");
            continue;
        }
        assert!(
            failure(&evaluation, id).is_none(),
            "{what}: {:?}",
            evaluation.failed
        );
        let found = noted(&evaluation, id);
        assert_eq!(found.factors, Some([50.0 / was, 1.0, 1.0]), "{what}");
        let scaled = solid_of(&evaluation, quad);
        let now = measured(scaled, &front).length;
        assert!(close(now, 50.0, 1e-12), "{what}: {now}");
        assert!(
            close(scaled.volume(), solid.volume() * 50.0 / was, 1e-12),
            "{what}"
        );
    }
}

/// A point on a body scaled and then merged into another: a later scale
/// about the scaled block's top corner, which a union put in the other
/// block, finds it on the union where the first scale left it, and
/// scales the union about it.
#[test]
fn a_point_on_a_scaled_body_after_a_merge() {
    let mut editor = Editor::new(Document::default());
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let solid = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
    add(&mut editor, uniform(&mm(), &[a], PointRef::Origin, "2"));
    let b = block(&mut editor, 20.0, 0.0, 30.0, 10.0, "10");
    add(
        &mut editor,
        Combine {
            target: b,
            tools: vec![a],
            op: BodyOp::Union,
            keep_tools: false,
        },
    );
    let mut faces = [DVec3::X, DVec3::Y, DVec3::Z].map(|n| key_on(&solid, n, 10.0));
    faces.sort();
    let about = PointRef::Corner {
        body: a,
        faces,
        near: DVec3::splat(20.0),
    };
    let id = add(&mut editor, uniform(&mm(), &[b], about, "1/2"));
    let evaluation = evaluated(editor.document());
    assert!(
        failure(&evaluation, id).is_none(),
        "{:?}",
        evaluation.failed
    );
    let found = (evaluation.scaled.iter()).find(|(feature, _)| *feature == id);
    assert_eq!(found.unwrap().1.centre, Some([20.0; 3]));
    let union = solid_of(&evaluation, b);
    assert_near(union.volume(), (8000.0 + 1000.0) / 8.0);
    assert!(boxed(union, [10.0; 3], [25.0, 20.0, 20.0]));
}

/// The scale's edge through upstream edits: the block it's on made a
/// join that merges it into another block is said to be in that one;
/// the block made taller changes the edge, the scale keeping its typed
/// length; a block cut down so the upright edge is gone isn't found.
#[test]
fn an_edge_through_upstream_edits_that_merge_or_change_it() {
    let mut editor = Editor::new(Document::default());
    let other = block(&mut editor, -10.0, 0.0, 0.0, 10.0, "10");
    let a = block(&mut editor, 0.0, 0.0, 10.0, 10.0, "10");
    let maker = editor.document().features()[3].id;
    let solid = Arc::clone(&evaluated(editor.document()).bodies[1].solid);
    let upright = flat_edge(
        &solid,
        a,
        (DVec3::X, 10.0),
        (DVec3::Y, 10.0),
        DVec3::new(10.0, 10.0, 5.0),
    );
    let id = add(&mut editor, to_length(&mm(), &[a], upright, "30", true));
    let evaluation = evaluated(editor.document());
    assert!(
        failure(&evaluation, id).is_none(),
        "{:?}",
        evaluation.failed
    );
    assert!(near_box(
        solid_of(&evaluation, a),
        [0.0; 3],
        [10.0, 10.0, 30.0]
    ));
    // Taller: the factor follows.
    let twenty = length(editor.document(), "20");
    set_extrude(&mut editor, maker, |extrude| {
        extrude.extent = Extent::OneSide(twenty);
    });
    let evaluation = evaluated(editor.document());
    assert!(
        failure(&evaluation, id).is_none(),
        "{:?}",
        evaluation.failed
    );
    assert_eq!(noted(&evaluation, id).factors, Some([1.0, 1.0, 1.5]));
    assert!(near_box(
        solid_of(&evaluation, a),
        [0.0; 3],
        [10.0, 10.0, 30.0]
    ));
    editor.undo();
    // Made a join, the block isn't made any more: the edit is refused
    // or takes the scale with it, never leaves it naming nothing.
    let FeatureKind::Extrude(mut join) = editor.document().feature(maker).unwrap().kind.clone()
    else {
        unreachable!()
    };
    join.operation = Operation::Join(Targets::default());
    let set = Command::SetFeature {
        feature: maker,
        kind: Box::new(join.into()),
    };
    let before = editor.document().clone();
    match editor.apply(set) {
        Ok(()) => {
            let document = editor.document();
            document.check().unwrap();
            let evaluation = evaluated(document);
            if document.feature(id).is_some() {
                let failed = failure(&evaluation, id).expect("refused");
                assert!(failed.message.contains("is in"), "{}", failed.message);
            }
            assert!(boxed(
                solid_of(&evaluation, other),
                [-10.0, 0.0, 0.0],
                [10.0, 10.0, 10.0]
            ));
            editor.undo();
        }
        Err(_) => assert_eq!(*editor.document(), before),
    }
    // A cut taking the upright edge's corner away, all the way up: the
    // edge isn't found.
    let extent = two_sides(editor.document(), "40", "40");
    add_extrude(
        &mut editor,
        rectangle((8.0, 8.0), (12.0, 12.0)),
        extent,
        Operation::Cut(Targets::default()),
    );
    // The scale is now before the cut: moved after it by editing the cut
    // into the history before the scale isn't possible, so a fresh scale
    // after the cut names the edge.
    let after = add(&mut editor, to_length(&mm(), &[a], upright, "30", true));
    let evaluation = evaluated(editor.document());
    let failed = failure(&evaluation, after).map(|f| f.message.as_str());
    assert_eq!(failed, Some("its edge wasn't found"));
}

/// The axis test at its bound, the small components kept apart from the
/// large one's square: a sine of 1.1e-9 is off the axis at any length,
/// 0.9e-9 on it.
#[test]
fn along_axis_tells_the_sine_at_any_length() {
    use crate::history::scale::along_axis;
    for length in [1e-3, 1.0, 10.0, 1e6] {
        for (axis, d) in [
            (0, DVec3::new(length, 0.9e-9 * length, 0.0)),
            (1, DVec3::new(0.6e-9 * length, -length, 0.6e-9 * length)),
            (2, DVec3::new(0.0, 0.0, -length)),
        ] {
            assert_eq!(along_axis(d), Some(axis), "{d}");
        }
        for d in [
            DVec3::new(length, 1.1e-9 * length, 0.0),
            DVec3::new(0.8e-9 * length, length, 0.8e-9 * length),
            DVec3::new(length, length, 0.0),
        ] {
            assert_eq!(along_axis(d), None, "{d}");
        }
    }
    assert_eq!(along_axis(DVec3::ZERO), None);
}

/// A cone turned from the triangle (0, 0), (5, 0), (0, 10) about the
/// sketch's Y axis scaled × 2 along X is an elliptic cone (a quadric):
/// its volume twice the cone's; the tip above y = 5 a cut leaves is 1/8
/// of that, to within the fit over the ellipse cut, or the cut is
/// refused changing nothing.
#[test]
fn a_cone_scaled_per_axis_cuts_as_an_elliptic_cone() {
    let mut editor = Editor::new(Document::default());
    let cone = revolved(&mut editor, polygon(&[(0.0, 0.0), (5.0, 0.0), (0.0, 10.0)]));
    let before = Arc::clone(&evaluated(editor.document()).bodies[0].solid);
    let scale = Scale {
        bodies: vec![cone],
        about: PointRef::Origin,
        factor: ScaleFactor::PerAxis(["2", "1", "1"].map(|t| factor(&mm(), t))),
    };
    add(&mut editor, scale);
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let scaled = solid_of(&evaluation, cone);
    let whole = PI * 10.0 * 5.0 * 10.0 / 3.0;
    assert!(close(scaled.volume(), 2.0 * before.volume(), 1e-12));
    assert!(close(scaled.volume(), whole, 1e-8), "{}", scaled.volume());
    assert!(
        (scaled.mesh().faces().iter()).any(|face| matches!(face.form, Form::Quadric { .. })),
        "a cone per axis is a quadric"
    );
    // Everything below y = 5 cut away, by a block on XZ (whose normal is
    // −Y: 20 down, 5 up).
    let extent = two_sides(editor.document(), "20", "5");
    let cut = super::super::tests::add_extrude_on(
        &mut editor,
        OriginPlane::XZ,
        rectangle((-30.0, -30.0), (30.0, 30.0)),
        extent,
        Operation::Cut(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    let v = solid_of(&evaluation, cone).volume();
    match failure(&evaluation, cut) {
        None => {
            let area = PI * 5.0 * 2.5;
            let fit = Tolerance::default().fit();
            assert!((v - whole / 8.0).abs() <= fit * area, "{v}");
        }
        Some(_) => assert!(close(v, whole, 1e-8), "{v}"),
    }
}

/// `∫ √(r² − x²) dx` from 0 to `x`.
fn under_circle(r: f64, x: f64) -> f64 {
    (x * (r * r - x * x).sqrt() + r * r * (x / r).asin()) / 2.0
}

/// The example plate scaled × 2 along X: its hole an ellipse of
/// semi-axes 16 and 8. A disc of radius 8 joined in it touches the
/// ellipse at two points (tangent): kept with the analytic volume, or
/// refused changing nothing; one of radius 10 crossing it is kept with
/// the analytic volume (the hole's area outside the disc left open).
#[test]
fn a_disc_in_an_elliptic_hole() {
    // The area the ellipse x²/256 + y²/64 <= 1 shares with a disc of
    // radius `r` about its centre: the ellipse's within x² <= c², the
    // disc's beyond (they cross at x² = 4(r² − 64)/3, or the disc is
    // inside).
    let shared = |r: f64| {
        let c2 = 4.0 * (r * r - 64.0) / 3.0;
        if c2 <= 0.0 {
            return PI * r * r;
        }
        let c = c2.sqrt();
        2.0 * under_circle(16.0, c) + 4.0 * (under_circle(r, r) - under_circle(r, c))
    };
    for (radius, crossing) in [(8.0, false), (10.0, true)] {
        let mut editor = Editor::new(Document::example());
        let plate = editor.document().bodies()[0].id;
        let scale = Scale {
            bodies: vec![plate],
            about: PointRef::Origin,
            factor: ScaleFactor::PerAxis(["2", "1", "1"].map(|t| factor(&mm(), t))),
        };
        add(&mut editor, scale);
        let ellipse = PI * 16.0 * 8.0;
        let holed = (120.0 * 40.0 - ellipse) * 10.0;
        let evaluation = evaluated(editor.document());
        assert!(close(solid_of(&evaluation, plate).volume(), holed, 1e-12));
        let extent = Extent::OneSide(length(editor.document(), "10"));
        let join = add_extrude(
            &mut editor,
            disc((0.0, 0.0), radius),
            extent,
            Operation::Join(Targets::default()),
        );
        let evaluation = evaluated(editor.document());
        let v = solid_of(&evaluation, plate).volume();
        let wanted = holed + shared(radius) * 10.0;
        match failure(&evaluation, join) {
            None => assert!(close(v, wanted, 1e-9), "{radius}: {v} {wanted}"),
            Some(failed) => {
                assert!(!crossing, "{radius}: {}", failed.message);
                assert!(close(v, holed, 1e-12), "{radius}: {v}");
            }
        }
    }
}
