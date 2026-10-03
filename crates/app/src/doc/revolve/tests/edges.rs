//! Revolving about a model edge: a straight edge of the model shown, in
//! the plane of the sketch the regions are of, picked as the axis,
//! previewed, committed and edited; the edges that can't be refused,
//! saying why.

use glam::{DVec2, DVec3};
use varde_document::{
    AxisLine, Command, EdgeRef, FaceKey, FeatureId, FeatureKind, PartKey, Placement, Plane,
    Revolve, Sketch,
};
use varde_regen::{Request, Summary};
use varde_view::{Edit, Look, RevolveLook, RevolvePick, TurnKind};

use crate::doc::Doc;
use crate::tests::{answer, example};

type Requests = std::rc::Rc<std::cell::RefCell<Vec<Request>>>;

/// The example plate with a sketch on its front face (`y = −20`) holding
/// a rectangle from `x = −10` to `10` and 2 to 5 above the plate's top
/// (`z = 12` to `15`), answered: the document, the requests waiting and
/// the sketch's id.
fn on_the_front() -> (Doc, Requests, FeatureId) {
    let (mut doc, requests) = example();
    let index = doc.feed.pick_index();
    let mesh = index.mesh();
    let front = (index.picking().faces().iter().enumerate())
        .find(|(_, face)| {
            matches!(face.summary, Summary::Plane { n, d }
                if DVec3::from(n).abs_diff_eq(-DVec3::Y, 1e-12) && (d - 20.0).abs() < 1e-9)
        })
        .map(|(face, _)| face as u32)
        .expect("the plate's front");
    assert!(mesh.face_count() > 0);
    let face = index.face_ref(front, DVec3::new(0.0, -20.0, 5.0)).unwrap();
    let placement = Placement::on_plane(-DVec3::Y, 20.0).unwrap();
    doc.apply(doc.editor.document().add_sketch(Plane::Face(face)));
    let sketch = doc.editor.document().features().last().unwrap().id;
    let local = |x: f64, z: f64| {
        let offset = DVec3::new(x, -20.0, z) - placement.origin;
        DVec2::new(offset.dot(placement.x), offset.dot(placement.y))
    };
    let mut drawn = Sketch::default();
    let corners = [(-10.0, 12.0), (10.0, 12.0), (10.0, 15.0), (-10.0, 15.0)]
        .map(|(x, z)| drawn.add_point(local(x, z)).unwrap());
    for k in 0..4 {
        let line = varde_sketch::Curve::Line {
            start: corners[k],
            end: corners[(k + 1) % 4],
        };
        drawn.add_curve(line, false).unwrap();
    }
    doc.apply(Command::SetSketch {
        feature: sketch,
        sketch: Box::new(drawn),
    });
    doc.sync();
    answer(&mut doc, &requests);
    (doc, requests, sketch)
}

/// The plate's top (its extrude's end cap) and the face of the model
/// shown on `n·x = d`: their keys.
fn top_and(doc: &Doc, n: DVec3, d: f64) -> (FaceKey, FaceKey) {
    let document = doc.editor.document();
    let top = FaceKey {
        feature: document.features()[1].id.get(),
        part: PartKey::EndCap,
        instance: 0,
    };
    let index = doc.feed.pick_index();
    let face = (index.picking().faces().iter())
        .find(|face| {
            matches!(face.summary, Summary::Plane { n: m, d: e }
                if DVec3::from(m).abs_diff_eq(n, 1e-12) && (e - d).abs() < 1e-9)
        })
        .expect("a face there");
    (top, face.key)
}

/// The edge of the model shown between the plate's top and its face on
/// `n·x = d`, nearest `near`.
fn edge_of(doc: &Doc, n: DVec3, d: f64, near: DVec3) -> u32 {
    let (top, other) = top_and(doc, n, d);
    let plate = doc.editor.document().bodies()[0].id;
    let index = doc.feed.pick_index();
    index
        .find_edge(plate, [top.min(other), top.max(other)], near)
        .expect("the edge")
}

/// Starts a revolve and picks the sketch's rectangle.
fn start(doc: &mut Doc, sketch: FeatureId) {
    doc.look(Look::StartRevolve);
    doc.look(Look::Revolve(RevolveLook::PickRegion { sketch, region: 0 }));
}

/// Clicks edge `edge` of the model shown at `at`.
fn pick_edge(doc: &mut Doc, edge: u32, at: DVec3) {
    let model = doc.feed.pick_index().model();
    doc.look(Look::Revolve(RevolveLook::PickEdge { model, edge, at }));
}

/// The revolves of `doc`'s document.
fn revolves(doc: &Doc) -> Vec<(FeatureId, Revolve)> {
    (doc.editor.document().features().iter())
        .filter_map(|feature| match &feature.kind {
            FeatureKind::Revolve(revolve) => Some((feature.id, revolve.clone())),
            _ => None,
        })
        .collect()
}

/// The plate's top front edge picked is the axis: stored by the plate,
/// the two faces' keys and the point clicked, its arrow along it,
/// previewed, and committed as one undo step; the tube it makes is
/// regenerated.
#[test]
fn a_model_edge_is_picked_as_the_axis_and_committed() {
    let (mut doc, requests, sketch) = on_the_front();
    let before = doc.editor.document().clone();
    start(&mut doc, sketch);
    assert_eq!(doc.revolve.as_ref().unwrap().picking, RevolvePick::Axis);
    let edge = edge_of(&doc, -DVec3::Y, 20.0, DVec3::new(0.0, -20.0, 10.0));
    let at = DVec3::new(3.0, -20.0, 10.0);
    pick_edge(&mut doc, edge, at);
    assert_eq!(doc.notice, None);
    let (top, front) = top_and(&doc, -DVec3::Y, 20.0);
    let plate = doc.editor.document().bodies()[0].id;
    let expected = EdgeRef {
        body: plate,
        faces: [top.min(front), top.max(front)],
        near: at,
    };
    let session = doc.revolve.as_ref().unwrap();
    assert_eq!(session.axis, Some(AxisLine::Edge(expected)));
    assert_eq!(session.picking, RevolvePick::Regions);
    let state = doc.revolve_state().unwrap();
    assert_eq!(state.axis_name().as_deref(), Some("Edge of Body 1"));
    assert!(state.ready);
    // Its ends along x, at the plate's top front.
    let [a, b] = state.edge_ends.unwrap();
    assert_eq!((a.y, a.z, b.y, b.z), (-20.0, 10.0, -20.0, 10.0));
    assert_eq!((a.x.abs(), b.x.abs()), (30.0, 30.0));
    assert!(a.x != b.x);

    // The preview takes it.
    let Some(Request::Regenerate {
        draft: Some(draft), ..
    }) = requests.borrow().last().cloned()
    else {
        panic!("a draft");
    };
    let FeatureKind::Revolve(drafted) = &draft.kind else {
        panic!("a revolve");
    };
    assert_eq!(drafted.axis, AxisLine::Edge(expected));
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.draft_error(), None);

    doc.update(Edit::CommitRevolve);
    assert!(doc.revolve.is_none());
    let [(_, revolve)] = &revolves(&doc)[..] else {
        panic!("one revolve");
    };
    assert_eq!(revolve.axis, AxisLine::Edge(expected));
    answer(&mut doc, &requests);
    assert!(
        doc.feed.failed_features().is_empty(),
        "{:?}",
        doc.feed.failed_features()
    );
    assert_eq!(doc.editor.document().bodies().len(), 2);
    doc.update(Edit::Undo);
    assert_eq!(*doc.editor.document(), before);
}

/// Edges that can't be the axis are refused, saying why, and leave the
/// axis as it was: one picked before the profile, a round one, one off
/// the sketch's plane, and one of a model that isn't shown any more.
#[test]
fn edges_that_cant_be_the_axis_are_refused() {
    let (mut doc, _requests, sketch) = on_the_front();
    let top_front = edge_of(&doc, -DVec3::Y, 20.0, DVec3::new(0.0, -20.0, 10.0));
    doc.look(Look::StartRevolve);
    doc.look(Look::Revolve(RevolveLook::Picking(RevolvePick::Axis)));
    pick_edge(&mut doc, top_front, DVec3::new(0.0, -20.0, 10.0));
    assert_eq!(
        doc.notice.as_deref(),
        Some("Pick the profile first, then its axis")
    );
    assert_eq!(doc.revolve.as_ref().unwrap().axis, None);

    doc.look(Look::Revolve(RevolveLook::PickRegion { sketch, region: 0 }));
    assert_eq!(doc.notice, None);
    // The hole's top rim.
    let index = doc.feed.pick_index();
    let rim = (0..index.mesh().edge_count() as u32)
        .find(|&edge| {
            index.chain_keys(edge).is_some_and(|keys| {
                keys.iter().any(|key| key.part == PartKey::EndCap)
                    && index.edge_ends(edge, &keys).is_none()
            })
        })
        .expect("a round edge on the top");
    pick_edge(&mut doc, rim, DVec3::new(8.0, 0.0, 10.0));
    assert_eq!(doc.notice.as_deref(), Some(varde_view::EDGE_NOT_STRAIGHT));
    assert_eq!(doc.revolve.as_ref().unwrap().axis, None);

    // The top back edge.
    let back = edge_of(&doc, DVec3::Y, 20.0, DVec3::new(0.0, 20.0, 10.0));
    pick_edge(&mut doc, back, DVec3::new(0.0, 20.0, 10.0));
    assert_eq!(doc.notice.as_deref(), Some(varde_view::EDGE_OFF_PLANE));
    assert_eq!(doc.revolve.as_ref().unwrap().axis, None);

    // A pick of another model.
    let model = doc.feed.pick_index().model() + 1;
    let at = DVec3::new(0.0, -20.0, 10.0);
    doc.look(Look::Revolve(RevolveLook::PickEdge {
        model,
        edge: top_front,
        at,
    }));
    assert!(doc.notice.as_deref().unwrap().contains("out of date"));
    assert_eq!(doc.revolve.as_ref().unwrap().axis, None);
    // The right one still is.
    pick_edge(&mut doc, top_front, at);
    assert!(matches!(
        doc.revolve.as_ref().unwrap().axis,
        Some(AxisLine::Edge(_))
    ));
}

/// A revolve about an edge edited keeps its axis, drawn where the model
/// shows the edge, and is set again in place. An edge its own solid
/// made can't be its axis: its start cap's edges lie in the plane.
#[test]
fn editing_a_revolve_about_an_edge_keeps_it() {
    let (mut doc, requests, sketch) = on_the_front();
    start(&mut doc, sketch);
    let edge = edge_of(&doc, -DVec3::Y, 20.0, DVec3::new(0.0, -20.0, 10.0));
    pick_edge(&mut doc, edge, DVec3::new(0.0, -20.0, 10.0));
    doc.look(Look::Revolve(RevolveLook::Extent(TurnKind::OneSide)));
    doc.look(Look::Revolve(RevolveLook::Input {
        angle: varde_view::Angle::First,
        text: "90".to_owned(),
    }));
    answer(&mut doc, &requests);
    doc.update(Edit::CommitRevolve);
    answer(&mut doc, &requests);
    assert!(
        doc.feed.failed_features().is_empty(),
        "{:?}",
        doc.feed.failed_features()
    );
    let [(feature, committed)] = &revolves(&doc)[..] else {
        panic!("one revolve");
    };
    let (feature, committed) = (*feature, committed.clone());

    doc.look(Look::EditFeature(feature));
    let session = doc.revolve.as_ref().expect("editing it");
    assert_eq!(session.axis, Some(committed.axis));
    assert!(!session.axis_missing);
    assert!(session.edge_ends.is_some());
    let state = doc.revolve_state().unwrap();
    assert!(state.ready);
    assert_eq!(state.axis_name().as_deref(), Some("Edge of Body 1"));

    // The quarter tube's own start cap, on the front, has straight edges
    // in the plane: made by the revolve, they're refused.
    answer(&mut doc, &requests);
    doc.look(Look::Revolve(RevolveLook::Picking(RevolvePick::Axis)));
    let index = doc.feed.pick_index();
    let own = (0..index.mesh().edge_count() as u32)
        .find(|&edge| {
            index.chain_keys(edge).is_some_and(|keys| {
                keys.iter().all(|key| key.feature == feature.get())
                    && index
                        .edge_ends(edge, &keys)
                        .is_some_and(|ends| ends.iter().all(|p| (p.y + 20.0).abs() < 1e-9))
            })
        })
        .expect("a straight edge of the tube on the front");
    pick_edge(&mut doc, own, DVec3::new(0.0, -20.0, 12.0));
    assert_eq!(
        doc.notice.as_deref(),
        Some("Only an edge made before the revolve can be its axis")
    );
    assert_eq!(doc.revolve.as_ref().unwrap().axis, Some(committed.axis));

    doc.look(Look::Revolve(RevolveLook::Extent(TurnKind::Full)));
    doc.update(Edit::CommitRevolve);
    let [(same, revolve)] = &revolves(&doc)[..] else {
        panic!("one revolve");
    };
    assert_eq!(*same, feature);
    assert_eq!(revolve.axis, committed.axis);
    assert_eq!(revolve.extent, varde_document::Turn::Full);
    doc.update(Edit::Undo);
    assert_eq!(revolves(&doc)[0].1, committed);
}

/// Adds a sketch on `plane` holding the rectangle from `a` to `b` (world
/// points on the plane): its id.
fn add_rectangle(
    editor: &mut varde_document::Editor,
    plane: varde_document::OriginPlane,
    a: DVec3,
    b: DVec3,
) -> FeatureId {
    let placement = plane.placement();
    editor
        .apply(editor.document().add_sketch(Plane::Origin(plane)))
        .unwrap();
    let feature = editor.document().features().last().unwrap().id;
    let local = |p: DVec3| {
        let offset = p - placement.origin;
        DVec2::new(offset.dot(placement.x), offset.dot(placement.y))
    };
    let (a, b) = (local(a), local(b));
    let mut drawn = Sketch::default();
    let corners = [(a.x, a.y), (b.x, a.y), (b.x, b.y), (a.x, b.y)]
        .map(|(x, y)| drawn.add_point(DVec2::new(x, y)).unwrap());
    for k in 0..4 {
        let line = varde_sketch::Curve::Line {
            start: corners[k],
            end: corners[(k + 1) % 4],
        };
        drawn.add_curve(line, false).unwrap();
    }
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(drawn),
        })
        .unwrap();
    feature
}

/// Extrudes all of sketch `sketch`'s regions `distance` up as a new body:
/// the extrude's id.
fn add_box(editor: &mut varde_document::Editor, sketch: FeatureId, distance: &str) -> FeatureId {
    let FeatureKind::Sketch { sketch: drawn, .. } =
        &editor.document().feature(sketch).unwrap().kind
    else {
        panic!("a sketch");
    };
    let profiles = drawn.profiles().unwrap();
    let regions = (0..profiles.regions.len())
        .map(|index| profiles.reference(index).unwrap())
        .collect();
    let extrude = varde_document::Extrude {
        sketch,
        regions,
        extent: varde_document::Extent::OneSide(crate::tests::length(editor.document(), distance)),
        flip: false,
        operation: varde_document::Operation::NewBody(varde_document::BodyId::NEW),
    };
    editor
        .apply(editor.document().add_feature(extrude.into()))
        .unwrap();
    editor.document().features().last().unwrap().id
}

/// The example plate and a box from (20, 0, 0) to (40, 10, 20), "Body 2",
/// combined into it as a union before a sketch on XZ holding a rectangle
/// from x = 22 to 28 and z = 12 to 15: the box's face on y = 0 meets the
/// plate's top along x = 20 to 30 at z = 10, in XZ. The document, the
/// requests waiting, the sketch and the box's extrude.
fn combined() -> (Doc, Requests, FeatureId, FeatureId) {
    use varde_document::OriginPlane;
    let mut editor = varde_document::Editor::new(varde_document::Document::example());
    let base = add_rectangle(
        &mut editor,
        OriginPlane::XY,
        DVec3::new(20.0, 0.0, 0.0),
        DVec3::new(40.0, 10.0, 0.0),
    );
    let maker = add_box(&mut editor, base, "20");
    let bodies = editor.document().bodies();
    let combine = varde_document::Combine {
        target: bodies[0].id,
        tools: vec![bodies[1].id],
        op: varde_document::BodyOp::Union,
        keep_tools: false,
    };
    editor
        .apply(editor.document().add_feature(combine.into()))
        .unwrap();
    let sketch = add_rectangle(
        &mut editor,
        OriginPlane::XZ,
        DVec3::new(22.0, 0.0, 12.0),
        DVec3::new(28.0, 0.0, 15.0),
    );
    let (doc, requests) = crate::tests::holding(editor.document().clone());
    assert!(
        doc.feed.failed_features().is_empty(),
        "{:?}",
        doc.feed.failed_features()
    );
    (doc, requests, sketch, maker)
}

/// An edge between a face of the target and one of a tool a combine
/// before the revolve used up is on the target there: picked, it's the
/// axis, named by the target, and the revolve regenerates.
#[test]
fn an_edge_between_bodies_a_combine_merged_before_is_the_axis() {
    let (mut doc, requests, sketch, maker) = combined();
    start(&mut doc, sketch);
    let document = doc.editor.document();
    let plate = document.bodies()[0].id;
    let top = FaceKey {
        feature: document.features()[1].id.get(),
        part: PartKey::EndCap,
        instance: 0,
    };
    let index = doc.feed.pick_index();
    let wall = (index.picking().faces().iter())
        .find(|face| {
            face.key.feature == maker.get()
                && matches!(face.summary, Summary::Plane { n, d }
                    if DVec3::from(n).abs_diff_eq(-DVec3::Y, 1e-12) && d.abs() < 1e-9)
        })
        .expect("the box's face on y = 0")
        .key;
    let near = DVec3::new(25.0, 0.0, 10.0);
    let edge = index
        .find_edge(plate, [top.min(wall), top.max(wall)], near)
        .expect("the edge");
    pick_edge(&mut doc, edge, near);
    assert_eq!(doc.notice, None);
    let expected = EdgeRef {
        body: plate,
        faces: [top.min(wall), top.max(wall)],
        near,
    };
    assert_eq!(
        doc.revolve.as_ref().unwrap().axis,
        Some(AxisLine::Edge(expected))
    );
    doc.update(Edit::CommitRevolve);
    answer(&mut doc, &requests);
    assert!(
        doc.feed.failed_features().is_empty(),
        "{:?}",
        doc.feed.failed_features()
    );
}

/// An undo taking away the body of the edge picked as the axis of a new
/// revolve drops the axis, which is picked again: what's set up never
/// names a body the document doesn't have.
#[test]
fn an_undo_taking_the_edge_s_body_away_drops_the_axis() {
    let (mut doc, requests, sketch) = on_the_front();
    // A box from (35, -20, 0) to (45, -10, 10), "Body 2", its front on
    // the sketch's plane.
    let mut editor = doc.editor.clone();
    let base = add_rectangle(
        &mut editor,
        varde_document::OriginPlane::XY,
        DVec3::new(35.0, -20.0, 0.0),
        DVec3::new(45.0, -10.0, 0.0),
    );
    let maker = add_box(&mut editor, base, "10");
    let box_feature = editor.document().feature(maker).unwrap().clone();
    doc.apply(
        editor
            .document()
            .add_sketch(Plane::Origin(varde_document::OriginPlane::XY)),
    );
    let FeatureKind::Sketch { sketch: drawn, .. } = &editor.document().feature(base).unwrap().kind
    else {
        panic!("a sketch");
    };
    let base_id = doc.editor.document().features().last().unwrap().id;
    doc.apply(Command::SetSketch {
        feature: base_id,
        sketch: Box::new(drawn.clone()),
    });
    let FeatureKind::Extrude(extrude) = box_feature.kind else {
        panic!("an extrude");
    };
    let extrude = varde_document::Extrude {
        sketch: base_id,
        ..extrude
    };
    doc.apply(doc.editor.document().add_feature(extrude.into()));
    doc.sync();
    answer(&mut doc, &requests);
    let body = doc.editor.document().bodies()[1].id;
    let maker = doc.editor.document().features().last().unwrap().id;

    start(&mut doc, sketch);
    let index = doc.feed.pick_index();
    let keys: Vec<FaceKey> = (index.picking().faces().iter())
        .filter(|face| face.key.feature == maker.get())
        .filter(|face| match face.summary {
            Summary::Plane { n, d } => {
                let n = DVec3::from(n);
                (n.abs_diff_eq(-DVec3::Y, 1e-12) && (d - 20.0).abs() < 1e-9)
                    || (n.abs_diff_eq(DVec3::Z, 1e-12) && (d - 10.0).abs() < 1e-9)
            }
            _ => false,
        })
        .map(|face| face.key)
        .collect();
    let [a, b] = keys[..] else {
        panic!("the box's front and top: {keys:?}");
    };
    let near = DVec3::new(40.0, -20.0, 10.0);
    let edge = index
        .find_edge(body, [a.min(b), a.max(b)], near)
        .expect("the box's top front edge");
    pick_edge(&mut doc, edge, near);
    assert_eq!(doc.notice, None);
    assert!(matches!(
        doc.revolve.as_ref().unwrap().axis,
        Some(AxisLine::Edge(edge)) if edge.body == body
    ));
    doc.update(Edit::Undo);
    answer(&mut doc, &requests);
    assert!(doc.editor.document().body(body).is_none());
    let session = doc.revolve.as_ref().expect("still setting up");
    assert_eq!(session.axis, None);
    assert_eq!(session.edge_ends, None);
    assert_eq!(session.picking, RevolvePick::Axis);
    assert!(doc.revolve_draft().is_none());
    // Redone, the body is back, but the axis is picked again.
    doc.update(Edit::Redo);
    answer(&mut doc, &requests);
    assert_eq!(doc.revolve.as_ref().unwrap().axis, None);
}

/// The arrow on an edge picked as the axis follows the edge when an undo
/// moves it: drawn where the model shown has it.
#[test]
fn the_arrow_follows_its_edge_through_an_undo() {
    let (mut doc, requests, sketch) = on_the_front();
    // The plate made 11 thick, its top front edge at z = 11.
    let document = doc.editor.document();
    let maker = document.features()[1].id;
    let FeatureKind::Extrude(plate) = &document.features()[1].kind else {
        panic!("the plate's extrude");
    };
    let thicker = varde_document::Extrude {
        extent: varde_document::Extent::OneSide(crate::tests::length(document, "11")),
        ..plate.clone()
    };
    doc.apply(Command::SetFeature {
        feature: maker,
        kind: Box::new(thicker.into()),
    });
    doc.sync();
    answer(&mut doc, &requests);
    start(&mut doc, sketch);
    let edge = edge_of(&doc, -DVec3::Y, 20.0, DVec3::new(0.0, -20.0, 11.0));
    pick_edge(&mut doc, edge, DVec3::new(0.0, -20.0, 11.0));
    let [a, b] = doc.revolve.as_ref().unwrap().edge_ends.unwrap();
    assert_eq!((a.z, b.z), (11.0, 11.0));
    doc.update(Edit::Undo);
    answer(&mut doc, &requests);
    let session = doc.revolve.as_ref().expect("still setting up");
    assert!(matches!(session.axis, Some(AxisLine::Edge(_))));
    let [a, b] = session.edge_ends.expect("the edge shown");
    assert_eq!((a.z, b.z), (10.0, 10.0));
}
