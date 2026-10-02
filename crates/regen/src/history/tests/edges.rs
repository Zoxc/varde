//! Revolves about model edges in the history: about an edge of the face
//! their sketch is on (volumes by Pappus, turning the way the edge's
//! reference runs), following the edge when an earlier feature moves
//! it, and through a join that consumes its body; failing when the edge
//! is off the sketch's plane, isn't straight, or is gone; and what's
//! cached. Edges are directed with their first face on their left from
//! outside, on mirrored solids too.

use glam::DVec3;
use varde_document::{AxisLine, EdgeRef, Placement, Revolve, Turn};
use varde_kernel::mesh::{FaceKey, Form, PartKey};
use varde_kernel::topology::Region;
use varde_kernel::{Budget, Motion};

use super::*;
use crate::Draft;
use crate::picking::region_form;
use crate::tests::{answered, regenerate_with};

/// Adds a sketch on `plane`, empty: its id.
fn add_sketch(editor: &mut Editor, plane: Plane) -> FeatureId {
    editor.apply(editor.document().add_sketch(plane)).unwrap();
    editor.document().features().last().unwrap().id
}

/// Sets the drawing of `sketch` to the rectangle with the world corners
/// `min` and `max`, both on its plane, placed at `placement`.
fn draw_rectangle(
    editor: &mut Editor,
    sketch: FeatureId,
    placement: &Placement,
    min: DVec3,
    max: DVec3,
) {
    let local = |at: DVec3| {
        let offset = at - placement.origin;
        assert!(
            offset.dot(placement.normal).abs() < 1e-12,
            "{at} is on the plane"
        );
        (offset.dot(placement.x), offset.dot(placement.y))
    };
    let (a, b) = (local(min), local(max));
    let mut drawn = Sketch::default();
    rectangle((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)))(&mut drawn);
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn),
        })
        .unwrap();
}

/// A revolve of all the regions of `sketch` about `axis`, a whole turn,
/// making a new body.
fn revolve_about(editor: &Editor, sketch: FeatureId, axis: AxisLine) -> Revolve {
    let FeatureKind::Sketch { sketch: drawn, .. } =
        &editor.document().feature(sketch).unwrap().kind
    else {
        panic!("{sketch:?} is a sketch");
    };
    let profiles = drawn.profiles().unwrap();
    let regions = (0..profiles.regions.len())
        .map(|index| profiles.reference(index).unwrap())
        .collect();
    Revolve {
        sketch,
        regions,
        axis,
        extent: Turn::Full,
        flip: false,
        operation: Operation::NewBody(BodyId::NEW),
    }
}

/// Adds [`revolve_about`] `sketch`'s `axis`: its id.
fn add_about(editor: &mut Editor, sketch: FeatureId, axis: AxisLine) -> FeatureId {
    let revolve = revolve_about(editor, sketch, axis);
    editor
        .apply(editor.document().add_feature(revolve.into()))
        .unwrap();
    editor.document().features().last().unwrap().id
}

/// Sets the revolve `feature` to `revolve`.
fn set_revolve(editor: &mut Editor, feature: FeatureId, revolve: Revolve) {
    editor
        .apply(Command::SetFeature {
            feature,
            kind: Box::new(revolve.into()),
        })
        .unwrap();
}

/// The key of the one region of `solid` on the plane `n·x = d`.
fn key_on(solid: &Solid, n: DVec3, d: f64) -> FaceKey {
    let topology = solid.topology();
    let on = |form: &Form| {
        matches!(*form, Form::Plane { n: m, d: e }
            if m.abs_diff_eq(n, 1e-12) && (e - d).abs() < 1e-9)
    };
    let found: Vec<FaceKey> = (topology.regions().iter())
        .filter(|region| on(region_form(solid, region)))
        .map(|region| region.key)
        .collect();
    let [key] = found[..] else {
        panic!("one region on {n}·x = {d}: {found:?}");
    };
    key
}

/// The edge of `body` between the faces `a` and `b`, picked at `near`.
fn edge(body: BodyId, a: FaceKey, b: FaceKey, near: DVec3) -> EdgeRef {
    EdgeRef {
        body,
        faces: [a.min(b), a.max(b)],
        near,
    }
}

/// The example plate's top (its end cap) and front (`y = −20`) faces'
/// keys.
fn top_and_front(document: &Document) -> (FaceKey, FaceKey) {
    let evaluation = evaluated(document);
    let solid = solid_of(&evaluation, document.bodies()[0].id);
    let top = FaceKey {
        feature: document.features()[1].id.get(),
        part: PartKey::EndCap,
        instance: 0,
    };
    assert_eq!(key_on(solid, DVec3::Z, 10.0), top);
    (top, key_on(solid, -DVec3::Y, 20.0))
}

/// Where `evaluation` placed the sketch `feature`.
fn placed(evaluation: &Evaluation, feature: FeatureId) -> Placement {
    let found = (evaluation.placements.iter()).find(|(id, _)| *id == feature);
    found.unwrap_or_else(|| panic!("{feature:?} is placed")).1
}

/// The volume of the ring from radius `inner` to `outer`, `length`
/// long, a whole turn.
fn ring(inner: f64, outer: f64, length: f64) -> f64 {
    PI * (outer * outer - inner * inner) * length
}

/// The volumes agree within the rounding of exact faces.
fn assert_close(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-9 * b.abs().max(1.0), "{a} vs {b}");
}

/// The example plate with a sketch on its front face (`y = −20`) holding
/// the rectangle from `x = −10` to `10` and `z = low` to `high`, and the
/// reference to the plate's top front edge (`z = 10`): the editor, the
/// sketch's id and the edge.
fn on_the_front(low: f64, high: f64) -> (Editor, FeatureId, EdgeRef) {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let (top, front) = top_and_front(editor.document());
    let face = varde_document::FaceRef {
        body: plate,
        key: front,
        near: DVec3::new(0.0, -20.0, 5.0),
    };
    let sketch = add_sketch(&mut editor, Plane::Face(face));
    let placement = placed(&evaluated(editor.document()), sketch);
    draw_rectangle(
        &mut editor,
        sketch,
        &placement,
        DVec3::new(-10.0, -20.0, low),
        DVec3::new(10.0, -20.0, high),
    );
    let edge = edge(plate, top, front, DVec3::new(0.0, -20.0, 10.0));
    (editor, sketch, edge)
}

/// The solid of the body the last feature made.
fn last_body(editor: &Editor, evaluation: &Evaluation) -> Arc<Solid> {
    let body = editor.document().bodies().last().unwrap().id;
    let made = evaluation.bodies.iter().find(|made| made.body == body);
    Arc::clone(&made.unwrap().solid)
}

/// A rectangle on the plate's front face turned a whole turn about the
/// plate's top front edge is a tube about it, its volume by Pappus; a
/// quarter turn turns right-handed about the edge's direction: along the
/// way it runs with its first face on its left, seen from outside.
#[test]
fn a_sketch_on_the_plate_s_side_revolves_about_its_edge() {
    let (mut editor, sketch, edge) = on_the_front(12.0, 15.0);
    let revolve = add_about(&mut editor, sketch, AxisLine::Edge(edge));
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    let tube = last_body(&editor, &evaluation);
    assert_close(tube.volume(), ring(2.0, 5.0, 20.0));
    let bounds = tube.bounds3().unwrap();
    assert!(
        (bounds.min - DVec3::new(-10.0, -25.0, 5.0))
            .abs()
            .max_element()
            < 1e-9
    );
    assert!(
        (bounds.max - DVec3::new(10.0, -15.0, 15.0))
            .abs()
            .max_element()
            < 1e-9
    );

    // The top's boundary runs round it anticlockwise seen from above, so
    // along +x at the front; the front's, seen from the front, along −x
    // at its top.
    let top = FaceKey {
        feature: editor.document().features()[1].id.get(),
        part: PartKey::EndCap,
        instance: 0,
    };
    let along_x = edge.faces[0] == top;
    let quarter = Value::new("90", &Turn::ask(&editor.document().design())).unwrap();
    for flip in [false, true] {
        let turned = Revolve {
            extent: Turn::OneSide(quarter.clone()),
            flip,
            ..revolve_about(&editor, sketch, AxisLine::Edge(edge))
        };
        set_revolve(&mut editor, revolve, turned);
        let evaluation = evaluated(editor.document());
        assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
        let solid = last_body(&editor, &evaluation);
        assert_close(solid.volume(), ring(2.0, 5.0, 20.0) / 4.0);
        // Right-handed about +x, up turns towards −y.
        let towards_minus_y = along_x != flip;
        let bounds = solid.bounds3().unwrap();
        let (low, high) = if towards_minus_y {
            (-25.0, -20.0)
        } else {
            (-20.0, -15.0)
        };
        assert!((bounds.min.y - low).abs() < 1e-9, "{flip}: {bounds:?}");
        assert!((bounds.max.y - high).abs() < 1e-9, "{flip}: {bounds:?}");
    }
}

/// The plate made thicker moves its top edge up: the revolve follows it,
/// its sketch staying where it was on the front face.
#[test]
fn a_revolve_follows_its_edge_when_an_earlier_feature_moves_it() {
    let (mut editor, sketch, edge) = on_the_front(20.0, 23.0);
    add_about(&mut editor, sketch, AxisLine::Edge(edge));
    let mut cache = Cache::default();
    let evaluation = evaluate(editor.document(), &mut cache);
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_close(
        last_body(&editor, &evaluation).volume(),
        ring(10.0, 13.0, 20.0),
    );
    let plate = editor.document().features()[1].id;
    set_extrude(&mut editor, plate, |extrude| {
        extrude.extent = Extent::OneSide(length(&Document::default(), "14"));
    });
    let evaluation = evaluate(editor.document(), &mut cache);
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_close(
        last_body(&editor, &evaluation).volume(),
        ring(6.0, 9.0, 20.0),
    );
}

/// An edge off the sketch's plane fails, "isn't in the sketch's plane":
/// the plate's top front edge for a sketch on XZ, and its top back edge
/// for a sketch on its front.
#[test]
fn an_edge_off_the_sketch_s_plane_fails() {
    let (mut editor, sketch, front_edge) = on_the_front(12.0, 15.0);
    let plate = front_edge.body;
    let evaluation = evaluated(editor.document());
    let solid = solid_of(&evaluation, plate);
    let back = key_on(solid, DVec3::Y, 20.0);
    let (top, _) = top_and_front(editor.document());
    let back_edge = edge(plate, top, back, DVec3::new(0.0, 20.0, 10.0));
    let revolve = add_about(&mut editor, sketch, AxisLine::Edge(back_edge));
    let off = "its axis edge isn't in the sketch's plane".to_owned();
    let failed = evaluated(editor.document()).failed;
    assert_eq!(failed, [(revolve, off.clone())]);
    // It shows the edge and its two ends.
    let geometry = failed[0].geometry.as_deref().expect("the edge shown");
    assert_eq!(geometry.points().len(), 2);
    assert!(!geometry.lines().points().is_empty());

    let xz = add_sketch(&mut editor, Plane::Origin(OriginPlane::XZ));
    draw_rectangle(
        &mut editor,
        xz,
        &OriginPlane::XZ.placement(),
        DVec3::new(-10.0, 0.0, 12.0),
        DVec3::new(10.0, 0.0, 15.0),
    );
    let on_xz = add_about(&mut editor, xz, AxisLine::Edge(front_edge));
    assert_eq!(
        evaluated(editor.document()).failed,
        [(revolve, off.clone()), (on_xz, off)]
    );
}

/// The hole's top rim, a circle on the plate's top, isn't straight: a
/// revolve about it fails, "isn't straight". So does a corner's pair of
/// faces meeting nowhere: "wasn't found".
#[test]
fn a_round_edge_fails() {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let (top, _) = top_and_front(editor.document());
    let evaluation = evaluated(editor.document());
    let topology = solid_of(&evaluation, plate).topology();
    let wall = (topology.regions().iter())
        .map(|region| region.key)
        .find(|key| {
            matches!(key.part, PartKey::Side { .. }) && {
                let solid = solid_of(&evaluation, plate);
                let region = topology.regions().iter().find(|r| r.key == *key).unwrap();
                matches!(region_form(solid, region), Form::Cylinder { .. })
            }
        })
        .unwrap();
    let face = varde_document::FaceRef {
        body: plate,
        key: top,
        near: DVec3::new(20.0, 0.0, 10.0),
    };
    let sketch = add_sketch(&mut editor, Plane::Face(face));
    let placement = placed(&evaluated(editor.document()), sketch);
    draw_rectangle(
        &mut editor,
        sketch,
        &placement,
        DVec3::new(15.0, -5.0, 10.0),
        DVec3::new(25.0, 5.0, 10.0),
    );
    let rim = edge(plate, top, wall, DVec3::new(8.0, 0.0, 10.0));
    let revolve = add_about(&mut editor, sketch, AxisLine::Edge(rim));
    let failed = evaluated(editor.document()).failed;
    assert_eq!(
        failed,
        [(revolve, "its axis edge isn't straight".to_owned())]
    );
    // It shows the rim.
    let geometry = failed[0].geometry.as_deref().expect("the rim shown");
    assert!(!geometry.lines().points().is_empty());
    // The top and the bottom don't meet.
    let bottom = FaceKey {
        part: PartKey::StartCap,
        ..top
    };
    let nowhere = edge(plate, top, bottom, DVec3::new(30.0, 0.0, 5.0));
    {
        let changed = revolve_about(&editor, sketch, AxisLine::Edge(nowhere));
        set_revolve(&mut editor, revolve, changed)
    };
    assert_eq!(
        evaluated(editor.document()).failed,
        [(revolve, "its axis edge wasn't found".to_owned())]
    );
}

/// An earlier cut widened under the whole plate takes its bottom front
/// edge away: the revolve about it fails, "wasn't found", and works
/// again once the cut is narrowed back. Its body's maker removed, its
/// sketch on the plate's front isn't placed, which it says.
#[test]
fn a_revolve_whose_edge_is_gone_fails() {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let (top, front) = top_and_front(editor.document());
    let bottom = FaceKey {
        part: PartKey::StartCap,
        ..top
    };
    // The bottom 5 taken off a patch clear of the front edge.
    let extent = two_sides(editor.document(), "5", "2");
    add_extrude(
        &mut editor,
        rectangle((15.0, -5.0), (25.0, 5.0)),
        extent,
        Operation::Cut(Targets::default()),
    );
    let skim_sketch = editor.document().features()[2].id;
    let face = varde_document::FaceRef {
        body: plate,
        key: front,
        near: DVec3::new(0.0, -20.0, 5.0),
    };
    let sketch = add_sketch(&mut editor, Plane::Face(face));
    let placement = placed(&evaluated(editor.document()), sketch);
    draw_rectangle(
        &mut editor,
        sketch,
        &placement,
        DVec3::new(-10.0, -20.0, -5.0),
        DVec3::new(10.0, -20.0, -2.0),
    );
    let bottom_front = edge(plate, bottom, front, DVec3::new(0.0, -20.0, 0.0));
    let revolve = add_about(&mut editor, sketch, AxisLine::Edge(bottom_front));
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_close(
        last_body(&editor, &evaluation).volume(),
        ring(2.0, 5.0, 20.0),
    );

    let widen = |editor: &mut Editor, min: (f64, f64), max: (f64, f64)| {
        let mut drawn = Sketch::default();
        rectangle(min, max)(&mut drawn);
        editor
            .apply(Command::SetSketch {
                feature: skim_sketch,
                sketch: Box::new(drawn),
            })
            .unwrap();
    };
    widen(&mut editor, (-40.0, -30.0), (40.0, 30.0));
    let evaluation = evaluated(editor.document());
    assert_eq!(
        evaluation.failed,
        [(revolve, "its axis edge wasn't found".to_owned())]
    );
    widen(&mut editor, (15.0, -5.0), (25.0, 5.0));
    assert!(evaluated(editor.document()).failed.is_empty());

    let maker = editor.document().features()[1].id;
    editor.apply(Command::RemoveFeature(maker)).unwrap();
    let evaluation = evaluated(editor.document());
    let gone = (evaluation.failed.iter()).find(|failed| failed.feature == revolve);
    // The sketch on the plate's face isn't placed either, which the
    // revolve says first.
    assert_eq!(gone.unwrap().message, "its sketch isn't placed");
}

/// The revolve's sketch is on XZ, so its edge's body may go while the
/// sketch stays placed: then it fails "body is gone".
#[test]
fn a_revolve_whose_edge_s_body_is_gone_fails() {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    // The plate's front bottom edge, on a plate moved to y = 0..40 by
    // drawing it there: it lies in XZ.
    let maker = editor.document().features()[1].id;
    let sketch_1 = editor.document().features()[0].id;
    let mut drawn = Sketch::default();
    rectangle((-30.0, 0.0), (30.0, 40.0))(&mut drawn);
    editor
        .apply(Command::SetSketch {
            feature: sketch_1,
            sketch: Box::new(drawn),
        })
        .unwrap();
    let profiles = match &editor.document().features()[0].kind {
        FeatureKind::Sketch { sketch, .. } => sketch.profiles().unwrap(),
        _ => unreachable!(),
    };
    set_extrude(&mut editor, maker, |extrude| {
        extrude.regions = vec![profiles.reference(0).unwrap()];
    });
    let evaluation = evaluated(editor.document());
    let solid = solid_of(&evaluation, plate);
    let (front, bottom) = (key_on(solid, -DVec3::Y, 0.0), key_on(solid, -DVec3::Z, 0.0));
    let bottom_front = edge(plate, front, bottom, DVec3::new(0.0, 0.0, 0.0));
    let xz = add_sketch(&mut editor, Plane::Origin(OriginPlane::XZ));
    draw_rectangle(
        &mut editor,
        xz,
        &OriginPlane::XZ.placement(),
        DVec3::new(-10.0, 0.0, -5.0),
        DVec3::new(10.0, 0.0, -2.0),
    );
    let revolve = add_about(&mut editor, xz, AxisLine::Edge(bottom_front));
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_close(
        last_body(&editor, &evaluation).volume(),
        ring(2.0, 5.0, 20.0),
    );
    editor.apply(Command::RemoveFeature(maker)).unwrap();
    assert_eq!(
        evaluated(editor.document()).failed,
        [(revolve, "its axis edge's body is gone".to_owned())]
    );
}

/// An edge of a body a join merged into another is found on the holder's
/// solid, where it lives on.
#[test]
fn a_revolve_about_a_consumed_body_s_edge_follows_it_into_its_holder() {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let block = add_body(&mut editor, rectangle((40.0, -10.0), (60.0, 10.0)), "6");
    let block_maker = editor.document().features().last().unwrap().id;
    // A bridge from the plate to the block merges the block into it.
    add_extrude(
        &mut editor,
        rectangle((25.0, -5.0), (45.0, 5.0)),
        Extent::OneSide(length(&Document::default(), "4")),
        Operation::Join(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(evaluation.merged, [(block, plate)]);
    let solid = solid_of(&evaluation, plate);
    let block_top = FaceKey {
        feature: block_maker.get(),
        part: PartKey::EndCap,
        instance: 0,
    };
    let block_front = key_on(solid, -DVec3::Y, 10.0);
    assert_eq!(block_front.feature, block_maker.get());
    // On XZ moved to the block's front: a sketch on the block's front
    // face, named by the block.
    let face = varde_document::FaceRef {
        body: block,
        key: block_front,
        near: DVec3::new(50.0, -10.0, 3.0),
    };
    let sketch = add_sketch(&mut editor, Plane::Face(face));
    let placement = placed(&evaluated(editor.document()), sketch);
    draw_rectangle(
        &mut editor,
        sketch,
        &placement,
        DVec3::new(45.0, -10.0, 8.0),
        DVec3::new(55.0, -10.0, 10.0),
    );
    let top_front = edge(block, block_top, block_front, DVec3::new(50.0, -10.0, 6.0));
    add_about(&mut editor, sketch, AxisLine::Edge(top_front));
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_close(
        last_body(&editor, &evaluation).volume(),
        ring(2.0, 4.0, 10.0),
    );
}

/// The edge's ends are cached with its body's solid, and the tool by
/// where the axis lies in the sketch: the edge picked again elsewhere
/// along it finds the same tool, and a draft about it is answered from
/// the cache.
#[test]
fn a_revolve_about_an_edge_is_found_in_the_cache() {
    let (mut editor, sketch, edge) = on_the_front(12.0, 15.0);
    let revolve = add_about(&mut editor, sketch, AxisLine::Edge(edge));
    let mut cache = Cache::default();
    let first = evaluate(editor.document(), &mut cache);
    let elsewhere = EdgeRef {
        near: DVec3::new(25.0, -20.0, 10.0),
        ..edge
    };
    {
        let changed = revolve_about(&editor, sketch, AxisLine::Edge(elsewhere));
        set_revolve(&mut editor, revolve, changed)
    };
    let again = evaluate(editor.document(), &mut cache);
    assert!(Arc::ptr_eq(
        &last_body(&editor, &first),
        &last_body(&editor, &again)
    ));

    // Drafts about the edge: one as the revolve answered as it is, one
    // about an edge off the plane failing, the model answered without
    // it.
    let draft = |edge: EdgeRef| Draft {
        revision: 1,
        feature: Some(revolve),
        kind: revolve_about(&editor, sketch, AxisLine::Edge(edge)).into(),
    };
    let answer = answered(crate::handle(regenerate_with(&editor, Some(draft(edge)))));
    assert!(answer.failed.is_empty(), "{:?}", answer.failed);
    assert_eq!(answer.draft.unwrap().error, None);
    let lower = EdgeRef {
        near: DVec3::new(0.0, -20.0, 0.0),
        faces: {
            let bottom = FaceKey {
                part: PartKey::StartCap,
                ..edge
                    .faces
                    .into_iter()
                    .find(|key| key.part == PartKey::EndCap)
                    .unwrap()
            };
            let side = edge
                .faces
                .into_iter()
                .find(|key| key.part != PartKey::EndCap)
                .unwrap();
            [bottom.min(side), bottom.max(side)]
        },
        ..edge
    };
    let answer = answered(crate::handle(regenerate_with(&editor, Some(draft(lower)))));
    assert!(answer.failed.is_empty(), "{:?}", answer.failed);
    // The bottom front edge is in the front's plane too, but the
    // rectangle is on its far side from the top edge: still a tube,
    // further out.
    assert_eq!(answer.draft.unwrap().error, None);
}

/// A rectangle standing on the plate's top front edge, its side along
/// it, turns into a solid cylinder about the edge: the side on the axis
/// makes no face.
#[test]
fn a_profile_along_its_edge_turns_into_a_cylinder() {
    let (mut editor, sketch, edge) = on_the_front(10.0, 15.0);
    add_about(&mut editor, sketch, AxisLine::Edge(edge));
    let evaluation = evaluated(editor.document());
    assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
    assert_close(
        last_body(&editor, &evaluation).volume(),
        ring(0.0, 5.0, 20.0),
    );
}

/// Every straight edge between flat faces of the example plate, and of
/// its image in an oblique mirror, runs with its first key's face on its
/// left seen from outside: that face's outward normal (its triangles run
/// round it anticlockwise from outside) crossed with the edge's
/// direction points into the face. The mirror reverses the triangles, so
/// the rule holds on the copy as on the source.
#[test]
fn edges_run_with_their_first_face_on_their_left_mirrored_too() {
    let document = Document::example();
    let evaluation = evaluated(&document);
    let plate = solid_of(&evaluation, document.bodies()[0].id);
    let mirror = Motion::mirror(DVec3::new(3.0, 0.0, 0.0), DVec3::new(1.0, 2.0, 0.5)).unwrap();
    let image = (plate.transformed(&mirror, None, &Tolerance::DEFAULT, &Budget::DEFAULT)).unwrap();
    for solid in [plate, &image] {
        let topology = solid.topology();
        let mesh = solid.mesh();
        let corners = |tri: u32| [0, 1, 2].map(|k| mesh.curve(3 * tri + k).p0);
        let mut checked = 0;
        for chain in topology.chains() {
            let regions = chain.regions.map(|r| &topology.regions()[r as usize]);
            let flat = |region: &&Region| matches!(region_form(solid, region), Form::Plane { .. });
            if !regions.iter().all(flat)
                || !matches!(edge_shape(solid, chain), EdgeShape::Line { .. })
            {
                continue;
            }
            let [a, b] = regions.map(|region| region.key);
            let first = mesh.curve(chain.halfedges[0]);
            let reference = edge(document.bodies()[0].id, a, b, (first.p0 + first.p1) / 2.0);
            let [from, to] = edge_ends(solid, &reference, &Tolerance::DEFAULT).unwrap();
            let left = regions
                .iter()
                .find(|r| r.key == reference.faces[0])
                .unwrap();
            let [p, q, r] = corners(left.tris[0]);
            let outward = (q - p).cross(r - p);
            let points: Vec<DVec3> = left.tris.iter().flat_map(|&t| corners(t)).collect();
            let inside = points.iter().sum::<DVec3>() / points.len() as f64;
            let into = outward.cross(to - from);
            assert!(into.dot(inside - (from + to) / 2.0) > 0.0, "{reference:?}");
            checked += 1;
        }
        // The plate's twelve box edges.
        assert_eq!(checked, 12);
    }
}
