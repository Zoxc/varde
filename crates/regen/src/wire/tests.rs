use std::sync::Arc;

use varde_document::{Command, Document, Editor, FeatureKind};
use varde_kernel::MeshPart;
use varde_kernel::mesh::{FaceKey, PartKey};

use super::*;
use crate::tests::sketched;
use crate::{Draft, handle};

fn regenerate(editor: &Editor) -> Request {
    Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude: None,
        draft: None,
        inspect: None,
    }
}

/// The head of a regenerated `generation` whose model is [`triangle`]:
/// one part, of the one body it lists, one face and one crease.
fn regenerated(generation: u64) -> Head {
    Head::Regenerated {
        generation: generation.into(),
        exclude: None,
        draft: None,
        unsolved: Vec::new(),
        failed: Vec::new(),
        touched: Vec::new(),
        merged: Vec::new(),
        placements: Vec::new(),
        bodies: vec![(BodyId::NEW, [[0.0; 3], [1.0; 3]])],
        parts: vec![BodyId::NEW],
        faces: vec![face()],
        closed: vec![false],
        tangents: vec![0],
        snaps: vec![None],
        corners: Vec::new(),
        inspected: None,
    }
}

/// The face of [`triangle`]'s one triangle: a plane's.
fn face() -> PickFace {
    PickFace {
        key: FaceKey {
            feature: 1,
            part: PartKey::StartCap,
            instance: 0,
        },
        aliases: Vec::new(),
        summary: crate::Summary::Plane {
            n: [0.0, 0.0, 1.0],
            d: 0.0,
        },
    }
}

/// The parts following a head without a model.
const NO_PARTS: &[&[u8]] = &[];

fn round_trip(response: &Response) -> Response {
    let (head, parts) = encode_reply(response);
    let parts: Vec<&[u8]> = parts.iter().map(|part| &**part).collect();
    decode_reply(&head[..], &parts).unwrap()
}

#[test]
fn request_round_trips() {
    let (editor, _) = sketched();
    let bytes = encode_request(&regenerate(&editor));
    let Request::Regenerate {
        generation,
        document,
        exclude,
        draft,
        inspect: None,
    } = decode_request(&bytes).unwrap()
    else {
        panic!("not a regeneration");
    };
    assert_eq!(generation, editor.generation());
    assert_eq!(*document, *editor.document());
    assert_eq!(exclude, None);
    assert_eq!(draft, None);
}

/// A document holding a revolve and a revolve's draft cross and come
/// back as they went, and the answer says what the draft touches.
#[test]
fn a_revolve_and_its_draft_round_trip() {
    use varde_document::{AxisLine, Operation, Revolve, Targets, Turn};

    let mut editor = Editor::new(Document::example());
    let extrude = crate::history::tests::example_extrude(editor.document());
    // The plate's regions about a line clear of it, on its −x side.
    let revolve = Revolve {
        sketch: extrude.sketch,
        regions: extrude.regions.clone(),
        axis: AxisLine::SketchY,
        extent: Turn::Full,
        flip: false,
        operation: Operation::Cut(Targets::default()),
    };
    editor
        .apply(editor.document().add_feature(revolve.clone().into()))
        .unwrap();
    let turn = varde_expr::Value::new("30", &Turn::ask(&editor.document().design())).unwrap();
    let draft = Draft {
        revision: 3,
        feature: None,
        kind: Revolve {
            axis: AxisLine::SketchX,
            extent: Turn::TwoSides(turn.clone(), turn),
            flip: true,
            operation: Operation::Join(Targets::default()),
            ..revolve
        }
        .into(),
    };
    let request = Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude: None,
        draft: Some(Box::new(draft.clone())),
        inspect: None,
    };
    let Request::Regenerate {
        document,
        draft: back,
        ..
    } = decode_request(&encode_request(&request)).unwrap()
    else {
        panic!("not a regeneration");
    };
    assert_eq!(*document, *editor.document());
    assert_eq!(back, Some(Box::new(draft)));

    // Both cross the plate's axes, so both fail, the draft with the
    // model without it, and say why.
    let Response::Regenerated { draft, failed, .. } = round_trip(&handle(request)) else {
        panic!("regeneration failed");
    };
    let crosses = "its outline crosses the axis".to_owned();
    assert_eq!(
        draft,
        Some(Drafted {
            revision: 3,
            error: Some(crosses.clone()),
            touched: None,
        })
    );
    let revolve = editor.document().features().last().unwrap().id;
    assert_eq!(failed, [(revolve, crosses)]);
}

/// A revolve that works crosses the wire in its reply: a boss turned
/// onto the example's plate joins it, and a quarter ring drafted as a
/// new body goes, the bodies' boxes, mesh and picking tables with them.
#[test]
fn a_revolve_that_works_crosses_in_the_reply() {
    use varde_document::{AxisLine, Operation, OriginPlane, Plane, Revolve, Targets, Turn};
    use varde_sketch::{Curve, Sketch};

    // A sketch on XZ (the world's x and z) of two rectangles.
    let mut editor = Editor::new(Document::example());
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XZ)))
        .unwrap();
    let sketch = editor.document().features().last().unwrap().id;
    let mut drawn = Sketch::default();
    for (min, max) in [((0.0, 10.0), (12.0, 14.0)), ((35.0, 0.0), (40.0, 5.0))] {
        let corners = [min, (max.0, min.1), max, (min.0, max.1)]
            .map(|(x, y)| drawn.add_point(glam::DVec2::new(x, y)).unwrap());
        for k in 0..4 {
            let (start, end) = (corners[k], corners[(k + 1) % 4]);
            drawn.add_curve(Curve::Line { start, end }, false).unwrap();
        }
    }
    let profiles = drawn.profiles().unwrap();
    let region = |inside: (f64, f64)| {
        let at = glam::DVec2::new(inside.0, inside.1);
        profiles.reference(profiles.region_at(at).unwrap()).unwrap()
    };
    let (boss, ring) = (region((6.0, 12.0)), region((37.0, 2.0)));
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn),
        })
        .unwrap();
    // A disc of radius 12 from z = 10 to 14 about the world's z, on the
    // plate's top.
    let join = Revolve {
        sketch,
        regions: vec![boss],
        axis: AxisLine::SketchY,
        extent: Turn::Full,
        flip: false,
        operation: Operation::Join(Targets::default()),
    };
    editor
        .apply(editor.document().add_feature(join.clone().into()))
        .unwrap();
    let revolve = editor.document().features().last().unwrap().id;
    let plate = editor.document().bodies()[0].id;
    // A quarter turn about the world's +z: +x toward +y.
    let quarter = varde_expr::Value::new("90", &Turn::ask(&editor.document().design())).unwrap();
    let draft = Draft {
        revision: 5,
        feature: None,
        kind: Revolve {
            regions: vec![ring],
            extent: Turn::OneSide(quarter),
            operation: Operation::NewBody(BodyId::NEW),
            ..join
        }
        .into(),
    };
    let request = Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude: None,
        draft: Some(Box::new(draft.clone())),
        inspect: None,
    };
    let request = decode_request(&encode_request(&request)).unwrap();
    let response = handle(request);
    let Response::Regenerated {
        draft,
        failed,
        touched,
        merged,
        bodies,
        mesh,
        picking,
        ..
    } = round_trip(&response)
    else {
        panic!("regeneration failed");
    };
    assert_eq!(
        draft,
        Some(Drafted {
            revision: 5,
            error: None,
            touched: None,
        })
    );
    assert_eq!(failed, []);
    assert_eq!(touched, [(revolve, vec![plate])]);
    assert_eq!(merged, []);
    let boxes: Vec<[[f32; 3]; 2]> = bodies
        .iter()
        .map(|(_, aabb)| [aabb.min.to_array(), aabb.max.to_array()])
        .collect();
    assert_eq!(
        boxes,
        [
            [[-30.0, -20.0, 0.0], [30.0, 20.0, 14.0]],
            [[0.0, 0.0, 0.0], [40.0, 40.0, 5.0]],
        ]
    );
    assert_eq!(bodies[0].0, plate);
    // The joined plate's faces and the quarter ring's, all drawn.
    assert!(mesh.triangle_count() > 0);
    let Response::Regenerated {
        mesh: sent,
        picking: sent_picking,
        ..
    } = &response
    else {
        unreachable!()
    };
    assert_eq!(*mesh, **sent);
    assert_eq!(*picking, **sent_picking);
    let feature = |face: &PickFace| face.key.feature;
    assert!(
        picking
            .faces()
            .iter()
            .any(|face| feature(face) == revolve.get())
    );
}

#[test]
fn request_with_a_draft_round_trips() {
    let editor = Editor::new(Document::example());
    let FeatureKind::Extrude(extrude) = &editor.document().features()[1].kind else {
        panic!("the example's second feature is its extrude");
    };
    let draft = Draft {
        revision: 7,
        feature: Some(editor.document().features()[1].id),
        kind: extrude.clone().into(),
    };
    let request = Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude: None,
        draft: Some(Box::new(draft.clone())),
        inspect: None,
    };
    let decoded = decode_request(&encode_request(&request)).unwrap();
    assert_eq!(decoded.draft(), Some(7));
    let Request::Regenerate { draft: back, .. } = decoded else {
        panic!("not a regeneration");
    };
    assert_eq!(back, Some(Box::new(draft)));

    // The answer says which draft it had.
    let Response::Regenerated { draft, .. } = round_trip(&handle(request)) else {
        panic!("regeneration failed");
    };
    assert_eq!(
        draft,
        Some(Drafted {
            revision: 7,
            error: None,
            touched: None,
        })
    );

    // A join says what it touches.
    let body = editor.document().bodies()[0].id;
    let mut join = extrude.clone();
    join.operation = varde_document::Operation::Join(varde_document::Targets::default());
    let request = Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude: None,
        draft: Some(Box::new(Draft {
            revision: 8,
            feature: None,
            kind: join.into(),
        })),
        inspect: None,
    };
    let Response::Regenerated { draft, .. } = round_trip(&handle(request)) else {
        panic!("regeneration failed");
    };
    let draft = draft.unwrap();
    assert_eq!((draft.revision, draft.touched), (8, Some(vec![body])));
}

#[test]
fn request_leaving_out_a_sketch_round_trips() {
    let (editor, feature) = sketched();
    let request = Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude: Some(feature),
        draft: None,
        inspect: None,
    };
    let Request::Regenerate {
        document, exclude, ..
    } = decode_request(&encode_request(&request)).unwrap()
    else {
        panic!("not a regeneration");
    };
    assert_eq!(*document, *editor.document());
    assert_eq!(exclude, Some(feature));

    // The answer says which sketch it left out.
    let response = handle(request);
    let Response::Regenerated {
        exclude, sketches, ..
    } = round_trip(&response)
    else {
        panic!("regeneration failed");
    };
    assert_eq!(exclude, Some(feature));
    assert_eq!(*sketches, RenderLines::default());
}

#[test]
fn untested_and_touching_nothing_stay_apart() {
    for touched in [None, Some(Vec::new()), Some(vec![BodyId::NEW])] {
        let mut head = regenerated(1);
        if let Head::Regenerated { draft, .. } = &mut head {
            *draft = Some(Drafted {
                revision: 3,
                error: None,
                touched: touched.clone(),
            });
        }
        let Head::Regenerated { draft, .. } = Head::decode(&head.encode()).unwrap() else {
            panic!("a regenerated head");
        };
        assert_eq!(draft.unwrap().touched, touched);
    }
}

#[test]
fn regenerated_round_trips() {
    // The example plate, its sketch shown, a sketch that doesn't solve,
    // and an extrude that fails.
    let mut editor = Editor::new(Document::example());
    let unsolved = crate::tests::unsolvable(&mut editor);
    let cut = crate::history::tests::add_failing(&mut editor);
    let sketch = editor.document().features()[0].id;
    editor
        .apply(Command::SetFeatureVisible(sketch, true))
        .unwrap();
    let bytes = encode_request(&regenerate(&editor));
    let response = handle(decode_request(&bytes).unwrap());
    let Response::Regenerated {
        mesh: sent,
        picking: picked,
        bodies: boxes,
        ..
    } = &response
    else {
        panic!("regeneration failed");
    };
    let Response::Regenerated {
        generation,
        exclude,
        draft,
        mesh,
        picking,
        sketches,
        unsolved: marked,
        failed,
        touched,
        merged,
        placements,
        bodies,
        inspected,
    } = round_trip(&response)
    else {
        panic!("regeneration failed");
    };
    assert_eq!(inspected, None);
    assert_eq!(generation, editor.generation());
    assert_eq!(exclude, None);
    assert_eq!(draft, None);
    assert_eq!(marked, [unsolved]);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].0, cut);
    // The join takes out the one body there is, so touches none.
    assert_eq!(touched, [(cut, Vec::new())]);
    assert!(merged.is_empty());
    assert!(placements.is_empty());
    assert_eq!(mesh, *sent);
    assert!(mesh.triangle_count() > 0);
    assert!(mesh.edge_count() > 0);
    assert_eq!(picking, *picked);
    // The plate with a hole: top, bottom, four sides and the hole's wall.
    assert_eq!(picking.faces().len(), 7);
    assert_eq!(mesh.face_count(), 7);
    assert_eq!(bodies, *boxes);
    assert_eq!(bodies.len(), 1);
    assert_eq!(picking.bodies(), [bodies[0].0]);
    assert_eq!(
        *sketches,
        crate::flatten_sketches(editor.document(), &[], None).unwrap()
    );
    assert_eq!(sketches.ends().len(), 6);
}

#[test]
fn bodies_boxes_must_be_boxes() {
    let body = Document::example().bodies()[0].id;
    for bad in [
        [[1.0; 3], [0.0; 3]],
        [[f32::NAN, 0.0, 0.0], [1.0; 3]],
        [[0.0; 3], [f32::INFINITY, 1.0, 1.0]],
    ] {
        let mut head = regenerated(4);
        if let Head::Regenerated { bodies, .. } = &mut head {
            bodies.extend([(body, [[0.0; 3], [1.0; 3]]), (body, bad)]);
        }
        let Response::Failed {
            generation, error, ..
        } = decode_reply(&head.encode()[..], &slices(&triangle())).unwrap()
        else {
            panic!("a bad box was taken");
        };
        assert_eq!(u64::from(generation), 4);
        assert_eq!(error, Error::Bounds.to_string());
    }
}

#[test]
fn merged_bodies_round_trip() {
    use crate::history::tests::{add_extrude, disc, plate_below, two_sides};
    use varde_document::{Operation, Targets};
    // A disc joined through the example plate and a plate under it.
    let mut editor = Editor::new(Document::example());
    let top = editor.document().bodies()[0].id;
    let below = plate_below(&mut editor);
    let extent = two_sides(editor.document(), "15", "5");
    let join = Operation::Join(Targets::default());
    add_extrude(&mut editor, disc((20.0, 0.0), 5.0), extent, join);
    let bytes = encode_request(&regenerate(&editor));
    let Response::Regenerated { merged, bodies, .. } =
        round_trip(&handle(decode_request(&bytes).unwrap()))
    else {
        panic!("regeneration failed");
    };
    assert_eq!(merged, [(below, top)]);
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0].0, top);
}

/// A combine and a combine drafted cross the wire as they went, and the
/// tools it consumes come back merged into its target.
#[test]
fn a_combine_and_its_draft_round_trip() {
    use varde_document::{BodyOp, Combine};
    let mut editor = Editor::new(Document::example());
    let top = editor.document().bodies()[0].id;
    let below = crate::history::tests::plate_below(&mut editor);
    let combine = Combine {
        target: top,
        tools: vec![below],
        op: BodyOp::Union,
        keep_tools: false,
    };
    editor
        .apply(editor.document().add_feature(combine.clone().into()))
        .unwrap();
    let draft = Draft {
        revision: 2,
        feature: editor.document().features().last().map(|f| f.id),
        kind: Combine {
            op: BodyOp::Subtract,
            keep_tools: true,
            ..combine
        }
        .into(),
    };
    let request = Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude: None,
        draft: Some(Box::new(draft.clone())),
        inspect: None,
    };
    let decoded = decode_request(&encode_request(&request)).unwrap();
    let Request::Regenerate {
        document,
        draft: back,
        ..
    } = &decoded
    else {
        panic!("not a regeneration");
    };
    assert_eq!(**document, *editor.document());
    assert_eq!(back, &Some(Box::new(draft)));
    // The draft keeps its tool, so nothing is merged.
    let Response::Regenerated {
        draft,
        merged,
        bodies,
        ..
    } = round_trip(&handle(decoded))
    else {
        panic!("regeneration failed");
    };
    assert_eq!(draft.unwrap().error, None);
    assert!(merged.is_empty());
    assert_eq!(bodies.len(), 2);
    // Committed, the union consumes it.
    let Response::Regenerated { merged, bodies, .. } = round_trip(&handle(regenerate(&editor)))
    else {
        panic!("regeneration failed");
    };
    assert_eq!(merged, [(below, top)]);
    assert_eq!(bodies.len(), 1);
}

#[test]
fn merged_bodies_are_each_consumed_once_and_hold_none() {
    let [a, b, c] = ids();
    for bad in [vec![(a, b), (a, c)], vec![(a, b), (b, c)], vec![(a, a)]] {
        let mut head = regenerated(5);
        if let Head::Regenerated { merged, .. } = &mut head {
            *merged = bad;
        }
        let Response::Failed {
            generation, error, ..
        } = decode_reply(&head.encode()[..], &slices(&triangle())).unwrap()
        else {
            panic!("bad merged bodies were taken");
        };
        assert_eq!(u64::from(generation), 5);
        assert_eq!(error, Error::Merged.to_string());
    }
    // Two consumed into one holder is fine.
    let mut head = regenerated(6);
    if let Head::Regenerated { merged, .. } = &mut head {
        *merged = vec![(b, a), (c, a)];
    }
    let reply = decode_reply(&head.encode()[..], &slices(&triangle())).unwrap();
    let Response::Regenerated { merged, .. } = reply else {
        panic!("good merged bodies were refused");
    };
    assert_eq!(merged, [(b, a), (c, a)]);
}

/// Three bodies' ids, from a document holding three.
fn ids() -> [BodyId; 3] {
    use crate::history::tests::plate_below;
    let mut editor = Editor::new(Document::example());
    let a = editor.document().bodies()[0].id;
    let b = plate_below(&mut editor);
    let c = plate_below(&mut editor);
    [a, b, c]
}

#[test]
fn empty_model_round_trips() {
    let bytes = encode_request(&regenerate(&Editor::new(Document::default())));
    let response = handle(decode_request(&bytes).unwrap());
    let Response::Regenerated { mesh, sketches, .. } = round_trip(&response) else {
        panic!("regeneration failed");
    };
    assert_eq!(*mesh, RenderMesh::default());
    assert_eq!(*sketches, RenderLines::default());
}

#[test]
fn failed_round_trips() {
    let (_, feature) = sketched();
    let response = Response::Failed {
        generation: Generation::from(u64::MAX),
        exclude: Some(feature),
        draft: Some(2),
        inspect: Some(5),
        error: "the kernel gave up".to_owned(),
    };
    assert!(encode_reply(&response).1.is_empty());
    let Response::Failed {
        generation,
        exclude,
        draft,
        inspect,
        error,
    } = round_trip(&response)
    else {
        panic!("not a failure");
    };
    assert_eq!(u64::from(generation), u64::MAX);
    assert_eq!(exclude, Some(feature));
    assert_eq!(draft, Some(2));
    assert_eq!(inspect, Some(5));
    assert_eq!(error, "the kernel gave up");
}

#[test]
fn malformed_model_fails_its_generation() {
    let mut parts = triangle();
    parts.pop();
    let head = regenerated(5).encode();
    let Response::Failed {
        generation, error, ..
    } = decode_reply(&head[..], &slices(&parts)).unwrap()
    else {
        panic!("decoded a model without line ends");
    };
    assert_eq!(u64::from(generation), 5);
    assert_eq!(error, Error::Parts(MODEL_PARTS - 1).to_string());
}

#[test]
fn unbounded_line_points_fail_their_generation() {
    for bad in [f32::INFINITY, -1e38] {
        let mut parts = triangle();
        parts[12][8..12].copy_from_slice(&bad.to_ne_bytes());
        let head = regenerated(5).encode();
        let Response::Failed {
            generation, error, ..
        } = decode_reply(&head[..], &slices(&parts)).unwrap()
        else {
            panic!("decoded a line point of {bad}");
        };
        assert_eq!(u64::from(generation), 5);
        assert_eq!(error, Error::RenderLines(LinesError::Values).to_string());
    }
}

#[test]
fn unbounded_positions_fail_their_generation() {
    for bad in [f32::NAN, 1e38] {
        let mut parts = triangle();
        parts[0][4..8].copy_from_slice(&bad.to_ne_bytes());
        let head = regenerated(5).encode();
        let Response::Failed {
            generation, error, ..
        } = decode_reply(&head[..], &slices(&parts)).unwrap()
        else {
            panic!("decoded a position of {bad}");
        };
        assert_eq!(u64::from(generation), 5);
        assert_eq!(
            error,
            Error::RenderMesh(MeshError::Values(MeshPart::Positions)).to_string()
        );
    }
}

#[test]
fn malformed_request_is_an_error() {
    let request = encode_request(&regenerate(&sketched().0));
    let mut garbage = request[..2].to_vec();
    garbage.extend([64, 0xff]);
    garbage.extend([0xff; 63]);
    for bytes in [
        &[][..],
        &request[..1],
        &request[..request.len() - 3],
        &garbage,
    ] {
        assert!(
            matches!(decode_request(bytes), Err(Error::Request(_))),
            "{bytes:?}"
        );
    }
}

#[test]
fn bytes_after_the_request_are_an_error() {
    let mut request = encode_request(&regenerate(&sketched().0));
    request.push(0);
    assert_eq!(
        decode_request(&request).unwrap_err().to_string(),
        "couldn't decode the request: 1 bytes after the end"
    );
    let error = decode_request(&request).unwrap_err();
    let source = std::error::Error::source(&error).expect("the decode error");
    assert_eq!(source.to_string(), "1 bytes after the end");
}

#[test]
fn malformed_head_is_an_error() {
    for head in [
        &[][..],
        &[9],
        &[0],
        &[1, 1],
        &[1, 1, 0xff, 0xff, 0xff, 0xff, 0x0f],
    ] {
        assert!(
            matches!(Head::decode(head), Err(Error::Head(_))),
            "{head:?}"
        );
    }
}

#[test]
fn bytes_after_the_head_are_an_error() {
    let mut head = regenerated(1).encode();
    head.push(0);
    assert_eq!(
        Head::decode(&head).unwrap_err().to_string(),
        "couldn't decode the reply: 1 bytes after the end"
    );
}

/// A mesh of one part: one triangle, its one face, its outline as one
/// edge closed on a corner at the first vertex, and a wire along a side.
fn triangle_mesh() -> RenderMesh {
    RenderMesh::from_parts(MeshParts {
        positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        normals: vec![[0.0, 0.0, 1.0]; 3],
        indices: vec![0, 1, 2],
        face_ends: vec![3],
        edge_vertices: vec![0, 1, 2, 0],
        edge_ends: vec![4],
        edge_faces: vec![[0, 0]],
        corners: vec![[0.0; 3]],
        edge_corners: vec![[0, 0]],
        wire_vertices: vec![1, 2],
        wire_ends: vec![2],
        part_ends: vec![[1, 1, 1, 1]],
    })
    .unwrap()
}

/// A regenerated answer with `mesh`, its parts of `parts`, each face
/// [`face`] and no edge closed, and a polyline of two segments around the
/// triangle.
fn answer(mesh: RenderMesh, parts: Vec<BodyId>) -> Response {
    let lines = RenderLines::from_parts(
        vec![[0.0, -1.0, 0.0], [2.0, -1.0, 0.0], [0.0, 2.0, 0.0]],
        vec![3],
    )
    .unwrap();
    let mut bodies = parts.clone();
    bodies.sort_unstable();
    bodies.dedup();
    let boxes = (bodies.into_iter())
        .map(|body| {
            let aabb = Aabb {
                min: Vec3::ZERO,
                max: Vec3::ONE,
            };
            (body, aabb)
        })
        .collect();
    let faces = vec![face(); mesh.face_count()];
    let closed = vec![false; mesh.edge_count()];
    let tangents = (0..mesh.edge_count() as u32).collect();
    let snaps = vec![None; mesh.edge_count()];
    let picking =
        Picking::from_parts(parts, faces, closed, tangents, snaps, Vec::new(), &mesh).unwrap();
    Response::Regenerated {
        generation: Generation::from(0),
        exclude: None,
        draft: None,
        mesh: Arc::new(mesh),
        picking: Arc::new(picking),
        sketches: Arc::new(lines),
        unsolved: Vec::new(),
        failed: Vec::new(),
        touched: Vec::new(),
        merged: Vec::new(),
        placements: Vec::new(),
        bodies: boxes,
        inspected: None,
    }
}

/// The parts of a model of [`triangle_mesh`] and a polyline of two
/// segments around it, as owned bytes to break.
fn triangle() -> Vec<Vec<u8>> {
    let parts = model_parts(&answer(triangle_mesh(), vec![BodyId::NEW]));
    assert_eq!(parts.len(), MODEL_PARTS);
    parts
}

/// The parts following `response`'s head, as owned bytes to break.
fn model_parts(response: &Response) -> Vec<Vec<u8>> {
    let (_, parts) = encode_reply(response);
    parts.into_iter().map(Cow::into_owned).collect()
}

/// A mesh of several parts, each with faces, edges and corners, and an
/// empty one between, round trips with the body of each part.
#[test]
fn a_mesh_of_parts_round_trips_with_their_bodies() {
    let [a, b, c] = ids();
    let mut mesh = triangle_mesh();
    mesh.append(
        &RenderMesh::from_parts(MeshParts {
            part_ends: vec![[0, 0, 0, 0]],
            ..MeshParts::default()
        })
        .unwrap(),
    )
    .unwrap();
    mesh.append_at(&triangle_mesh(), glam::Vec3::X).unwrap();
    assert_eq!(mesh.part_ends(), [[1, 1, 1, 1], [1, 1, 1, 1], [2, 2, 2, 2]]);
    let response = answer(mesh.clone(), vec![a, b, c]);
    let Response::Regenerated {
        mesh: back,
        picking,
        ..
    } = round_trip(&response)
    else {
        panic!("regeneration failed");
    };
    assert_eq!(*back, mesh);
    assert_eq!(picking.bodies(), [a, b, c]);
    assert_eq!(picking.face_body(&mesh, 0), Some(a));
    assert_eq!(picking.face_body(&mesh, 1), Some(c));
}

fn slices(parts: &[Vec<u8>]) -> Vec<&[u8]> {
    parts.iter().map(Vec::as_slice).collect()
}

fn decode(parts: &[Vec<u8>]) -> Result<RenderMesh, Error> {
    decode_model(&slices(parts)).map(|(mesh, _)| mesh)
}

fn decode_lines(parts: &[Vec<u8>]) -> Result<RenderLines, Error> {
    decode_model(&slices(parts)).map(|(_, lines)| lines)
}

#[test]
fn triangle_decodes() {
    let (mesh, lines) = decode_model(&slices(&triangle())).unwrap();
    assert_eq!(mesh.triangle_count(), 1);
    assert_eq!(lines.segment_count(), 2);
}

#[test]
fn wrong_number_of_parts_is_an_error() {
    let mut parts = triangle();
    parts.pop();
    assert_eq!(decode(&parts), Err(Error::Parts(MODEL_PARTS - 1)));
    assert_eq!(decode(&[]), Err(Error::Parts(0)));
    assert_eq!(decode(&parts[..1]), Err(Error::Parts(1)));
    parts.extend([Vec::new(), Vec::new()]);
    assert_eq!(decode(&parts), Err(Error::Parts(MODEL_PARTS + 1)));
}

#[test]
fn partial_elements_are_an_error() {
    for (part, name) in [
        (0, Part::RenderMesh(MeshPart::Positions)),
        (2, Part::RenderMesh(MeshPart::Indices)),
        (3, Part::RenderMesh(MeshPart::FaceEnds)),
        (4, Part::RenderMesh(MeshPart::EdgeVertices)),
        (5, Part::RenderMesh(MeshPart::EdgeEnds)),
        (6, Part::RenderMesh(MeshPart::EdgeFaces)),
        (7, Part::RenderMesh(MeshPart::Corners)),
        (8, Part::RenderMesh(MeshPart::EdgeCorners)),
        (9, Part::RenderMesh(MeshPart::WireVertices)),
        (10, Part::RenderMesh(MeshPart::WireEnds)),
        (11, Part::RenderMesh(MeshPart::PartEnds)),
        (12, Part::RenderLines(LinesPart::Points)),
        (13, Part::RenderLines(LinesPart::Ends)),
    ] {
        let mut parts = triangle();
        parts[part].pop();
        if part == 0 {
            parts[1].pop();
        }
        let len = parts[part].len();
        assert_eq!(
            decode_model(&slices(&parts)).map(|_| ()),
            Err(Error::Partial { part: name, len })
        );
    }
}

#[test]
fn line_ends_must_make_polylines() {
    // Ends past the points, and polylines of one point.
    for ends in [&[4u32][..], &[1, 3], &[]] {
        let mut parts = triangle();
        parts[13] = bytemuck::cast_slice(ends).to_vec();
        assert_eq!(
            decode_lines(&parts),
            Err(Error::RenderLines(LinesError::Ends)),
            "{ends:?}"
        );
    }
}

#[test]
fn normals_must_match_positions() {
    let mut parts = triangle();
    parts[1].truncate(12);
    assert_eq!(
        decode(&parts),
        Err(Error::RenderMesh(MeshError::Normals {
            positions: 3,
            normals: 1
        }))
    );
}

#[test]
fn indices_must_make_triangles() {
    let mut parts = triangle();
    parts[2].truncate(8);
    assert_eq!(
        decode(&parts),
        Err(Error::RenderMesh(MeshError::Triangles(2)))
    );
}

#[test]
fn indices_and_edges_must_refer_to_vertices() {
    let mut parts = triangle();
    parts[2][4..8].copy_from_slice(&3u32.to_ne_bytes());
    assert_eq!(
        decode(&parts),
        Err(Error::RenderMesh(MeshError::OutOfRange {
            part: MeshPart::Indices,
            index: 3,
            vertices: 3
        }))
    );

    let mut parts = triangle();
    parts[4][8..12].copy_from_slice(&u32::MAX.to_ne_bytes());
    assert_eq!(
        decode(&parts),
        Err(Error::RenderMesh(MeshError::OutOfRange {
            part: MeshPart::EdgeVertices,
            index: u32::MAX,
            vertices: 3
        }))
    );

    let mut parts = triangle();
    parts[9][4..8].copy_from_slice(&3u32.to_ne_bytes());
    assert_eq!(
        decode(&parts),
        Err(Error::RenderMesh(MeshError::OutOfRange {
            part: MeshPart::WireVertices,
            index: 3,
            vertices: 3
        }))
    );
}

/// Claims a length without the bytes, to test the bound without
/// allocating it.
struct Huge(usize);

impl Buffer for Huge {
    fn byte_len(&self) -> usize {
        self.0
    }

    fn copy_into(&self, _: &mut [u8]) {
        unreachable!("too large to copy");
    }
}

/// Each part one element past its bound is refused before it's copied.
#[test]
fn oversized_parts_are_an_error() {
    use RenderMesh as M;
    let mesh = |part| Part::RenderMesh(part);
    for (at, part, bytes) in [
        (0, mesh(MeshPart::Positions), M::MAX_VERTICES * 12),
        (1, mesh(MeshPart::Normals), M::MAX_VERTICES * 12),
        (2, mesh(MeshPart::Indices), M::MAX_INDICES * 4),
        (3, mesh(MeshPart::FaceEnds), M::MAX_FACES * 4),
        (4, mesh(MeshPart::EdgeVertices), M::MAX_EDGE_POINTS * 4),
        (5, mesh(MeshPart::EdgeEnds), M::MAX_EDGE_POLYLINES * 4),
        (6, mesh(MeshPart::EdgeFaces), M::MAX_EDGE_POLYLINES * 8),
        (7, mesh(MeshPart::Corners), M::MAX_CORNERS * 12),
        (8, mesh(MeshPart::EdgeCorners), M::MAX_EDGE_POLYLINES * 8),
        (9, mesh(MeshPart::WireVertices), M::MAX_EDGE_POINTS * 4),
        (10, mesh(MeshPart::WireEnds), M::MAX_EDGE_POLYLINES * 4),
        (11, mesh(MeshPart::PartEnds), M::MAX_PARTS * 16),
        (
            12,
            Part::RenderLines(LinesPart::Points),
            RenderLines::MAX_POINTS * 12,
        ),
        (
            13,
            Part::RenderLines(LinesPart::Ends),
            RenderLines::MAX_POLYLINES * 4,
        ),
    ] {
        let len = bytes + 1;
        let huge = Huge(len);
        let parts = triangle();
        let slices = slices(&parts);
        let mut parts: Vec<&dyn Buffer> = slices.iter().map(|p| p as &dyn Buffer).collect();
        parts[at] = &huge;
        assert_eq!(
            decode_model(&parts).map(|_| ()),
            Err(Error::TooLarge { part, len }),
            "{part}"
        );
    }
}

#[test]
fn oversized_head_is_an_error() {
    assert_eq!(
        decode_reply(&Huge(MAX_HEAD_BYTES + 1), NO_PARTS).unwrap_err(),
        Error::TooLarge {
            part: Part::Head,
            len: MAX_HEAD_BYTES + 1
        }
    );
}

#[test]
fn errors_display() {
    let error = decode(&[]).unwrap_err();
    assert_eq!(error.to_string(), "model in 0 parts instead of 14");
}

/// A small deterministic generator for the fuzz tests below (xorshift64).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A number below `n`.
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn bytes(&mut self, max_len: usize) -> Vec<u8> {
        let len = self.below(max_len + 1);
        (0..len).map(|_| self.next() as u8).collect()
    }

    /// `bytes` with a few bytes changed, or cut short.
    fn mutate(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut bytes = bytes.to_vec();
        if bytes.is_empty() || self.below(4) == 0 {
            bytes.truncate(self.below(bytes.len() + 1));
        } else {
            for _ in 0..=self.below(3) {
                let at = self.below(bytes.len());
                bytes[at] = self.next() as u8;
            }
        }
        bytes
    }
}

/// Decodes `head` and `parts` as a reply and checks that an accepted
/// model holds together, which the renderer and picking rely on.
fn decode_any(head: &[u8], parts: &[Vec<u8>]) {
    if let Ok(Response::Regenerated {
        mesh,
        sketches,
        picking,
        bodies,
        ..
    }) = decode_reply(head, &slices(parts))
    {
        let mut start = 0;
        for &end in sketches.ends() {
            assert!(end >= start + 2);
            start = end;
        }
        assert_eq!(start as usize, sketches.points().len());
        assert_eq!(mesh.positions().len(), mesh.normals().len());
        assert!(mesh.indices().len().is_multiple_of(3));
        let vertices = mesh.positions().len();
        for &index in mesh.indices().iter().chain(mesh.edge_vertices()) {
            assert!((index as usize) < vertices);
        }
        assert_eq!(
            mesh.parts().last().map_or(0, |part| part.indices.end),
            mesh.indices().len()
        );
        assert_eq!(picking.bodies().len(), mesh.part_ends().len());
        assert_eq!(picking.faces().len(), mesh.face_count());
        assert_eq!(picking.closed().len(), mesh.edge_count());
        for face in 0..mesh.face_count() as u32 {
            picking.face_body(&mesh, face);
        }
        for edge in 0..mesh.edge_count() as u32 {
            picking.edge_keys(&mesh, edge);
        }
        for (e, &closed) in picking.closed().iter().enumerate() {
            if closed {
                let [a, b] = mesh.edge_faces()[e];
                let [start, end] = mesh.edge_corners()[e];
                assert!(a != b && start == end);
            }
        }
        for face in picking.faces() {
            assert!(face.summary.valid(), "{face:?}");
            assert!(face.aliases.windows(2).all(|w| w[0] < w[1]));
            assert!(!face.aliases.contains(&face.key));
        }
        for body in picking.bodies() {
            assert!(bodies.iter().any(|(listed, _)| listed == body));
        }
    }
}

#[test]
fn random_bytes_never_panic() {
    let mut rng = Rng(0x5eed);
    for _ in 0..5000 {
        let bytes = rng.bytes(64);
        let _ = Head::decode(&bytes);
        let _ = decode_request(&bytes);
        let parts: Vec<_> = (0..rng.below(14)).map(|_| rng.bytes(48)).collect();
        decode_any(&bytes, &parts);
        decode_any(&regenerated(1).encode(), &parts);
    }
}

#[test]
fn damaged_encodings_never_panic() {
    let mut rng = Rng(0xdecade);
    let request = encode_request(&regenerate(&sketched().0));
    let (head, _) = encode_reply(&handle(decode_request(&request).unwrap()));
    let parts = triangle();
    let failed = Head::Failed {
        generation: Generation::from(3),
        exclude: None,
        draft: Some(1),
        inspect: Some(2),
        error: "no".to_owned(),
    }
    .encode();
    for _ in 0..5000 {
        let _ = decode_request(&rng.mutate(&request));
        let _ = Head::decode(&rng.mutate(&failed));
        let mut damaged = parts.clone();
        let part = rng.below(damaged.len());
        damaged[part] = rng.mutate(&damaged[part]);
        decode_any(&rng.mutate(&head), &damaged);
        decode_any(&head, &damaged);
    }
}

#[test]
fn huge_lengths_are_refused_without_allocating_them() {
    // A varint claiming close to `u64::MAX` elements, with nothing after.
    let huge = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01];

    // A document that long, and one with so many bodies in it.
    let mut long = vec![0, 9];
    long.extend(huge);
    let mut bodies = vec![0, 9, huge.len() as u8];
    bodies.extend(huge);
    for request in [long, bodies] {
        assert!(matches!(decode_request(&request), Err(Error::Request(_))));
    }

    // An error message that long.
    let mut head = Head::Failed {
        generation: Generation::from(3),
        exclude: None,
        draft: None,
        inspect: None,
        error: String::new(),
    }
    .encode();
    head.pop();
    head.extend(huge);
    assert!(matches!(Head::decode(&head), Err(Error::Head(_))));
}

/// The error a reply of `head` and `parts` fails its generation with.
fn refused(head: &Head, parts: &[Vec<u8>]) -> String {
    match decode_reply(&head.encode()[..], &slices(parts)).unwrap() {
        Response::Failed { error, .. } => error,
        Response::Regenerated { .. } | Response::Exported { .. } => {
            panic!("a hostile reply was taken")
        }
    }
}

/// `regenerated(1)` with its faces and closed flags changed by `change`.
fn tables(change: impl FnOnce(&mut Vec<PickFace>, &mut Vec<bool>)) -> Head {
    let mut head = regenerated(1);
    if let Head::Regenerated { faces, closed, .. } = &mut head {
        change(faces, closed);
    }
    head
}

#[test]
fn picking_must_have_one_entry_per_part_face_and_edge() {
    let lengths = Error::Picking(PickingError::Lengths).to_string();
    let mut heads = vec![
        tables(|faces, _| faces.push(face())),
        tables(|faces, _| faces.clear()),
        tables(|_, closed| closed.push(false)),
        tables(|_, closed| closed.clear()),
    ];
    for change in [
        (|parts: &mut Vec<BodyId>, _: &mut Vec<u32>| parts.clear())
            as fn(&mut Vec<BodyId>, &mut Vec<u32>),
        |parts, _| parts.push(BodyId::NEW),
        |_, tangents| tangents.push(0),
        |_, tangents| tangents.clear(),
    ] {
        let mut head = regenerated(1);
        if let Head::Regenerated {
            parts, tangents, ..
        } = &mut head
        {
            change(parts, tangents);
        }
        heads.push(head);
    }
    for head in heads {
        assert_eq!(refused(&head, &triangle()), lengths);
    }
}

/// A mesh of two triangles side by side, a face each, with one edge
/// between them: along their shared side if not `closed`, else round the
/// first triangle, closing on its first vertex. As [`triangle`]'s parts,
/// for a head of two faces ([`two_faces`]).
fn two_triangles(closed: bool) -> Vec<Vec<u8>> {
    let (edge_vertices, corners, edge_corners) = if closed {
        (vec![0, 1, 2, 0], vec![[0.0; 3]], vec![[0, 0]])
    } else {
        (
            vec![1, 2],
            vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![[0, 1]],
        )
    };
    let mesh = RenderMesh::from_parts(MeshParts {
        positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]],
        normals: vec![[0.0, 0.0, 1.0]; 4],
        indices: vec![0, 1, 2, 1, 3, 2],
        face_ends: vec![3, 6],
        edge_ends: vec![edge_vertices.len() as u32],
        edge_vertices,
        edge_faces: vec![[0, 1]],
        corners,
        edge_corners,
        part_ends: vec![[2, 1, if closed { 1 } else { 2 }, 0]],
        ..MeshParts::default()
    })
    .unwrap();
    model_parts(&answer(mesh, vec![BodyId::NEW]))
}

/// [`tables`] of two faces, the second the first's end cap, and the
/// one edge closed or not.
fn two_faces(closed: bool) -> Head {
    tables(|faces, flags| {
        let mut second = face();
        second.key.part = PartKey::EndCap;
        faces.push(second);
        flags[0] = closed;
    })
}

#[test]
fn closed_edges_must_close_between_two_faces() {
    // As made, they're taken, and the edge's keys are its faces'.
    for closed in [false, true] {
        let reply = decode_reply(
            &two_faces(closed).encode()[..],
            &slices(&two_triangles(closed)),
        )
        .unwrap();
        let Response::Regenerated { mesh, picking, .. } = reply else {
            panic!("a good edge was refused");
        };
        assert_eq!(picking.closed(), [closed]);
        let keys = picking.edge_keys(&mesh, 0).unwrap();
        assert_eq!(
            keys.map(|key| key.part),
            [PartKey::StartCap, PartKey::EndCap]
        );
    }
    // An open edge said to close, and a crease.
    let bad = Error::Picking(PickingError::Closed).to_string();
    assert_eq!(refused(&two_faces(true), &two_triangles(false)), bad);
    assert_eq!(
        refused(&tables(|_, closed| closed[0] = true), &triangle()),
        bad
    );
    // A crease has no keys.
    let reply = decode_reply(&regenerated(1).encode()[..], &slices(&triangle())).unwrap();
    let Response::Regenerated { mesh, picking, .. } = reply else {
        panic!("the triangle was refused");
    };
    assert_eq!(picking.edge_keys(&mesh, 0), None);
    // Nor has an edge the mesh hasn't.
    assert_eq!(picking.edge_keys(&mesh, 1), None);
    assert_eq!(picking.edge_keys(&mesh, u32::MAX), None);
}

#[test]
fn picked_faces_must_hold_sound_summaries_and_aliases() {
    use crate::Summary;
    let bad = Error::Picking(PickingError::Face).to_string();
    let summaries = [
        Summary::Plane {
            n: [0.0, 0.0, f64::NAN],
            d: 0.0,
        },
        Summary::Plane {
            n: [0.0, 0.0, 2.0],
            d: 0.0,
        },
        Summary::Plane {
            n: [0.0, 0.0, 1.0],
            d: f64::INFINITY,
        },
        Summary::Plane {
            n: [0.0, 0.0, 1.0],
            d: 1e300,
        },
        Summary::Cylinder {
            point: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            radius: -1.0,
        },
        Summary::Cylinder {
            point: [f64::NAN; 3],
            axis: [0.0, 0.0, 1.0],
            radius: 1.0,
        },
        Summary::Cone {
            apex: [0.0; 3],
            axis: [1.0, 0.0, 0.0],
            cos: 0.5,
            sin: 2.0,
        },
        Summary::Sphere {
            centre: [0.0; 3],
            radius: 0.0,
        },
        Summary::Torus {
            centre: [0.0; 3],
            axis: [0.0; 3],
            major: 2.0,
            minor: 1.0,
        },
        Summary::ConicCylinder {
            along: [0.0, 0.5, 0.0],
        },
        Summary::Revolved {
            origin: [f64::NAN; 3],
            axis: [0.0, 0.0, 1.0],
        },
    ];
    for summary in summaries {
        let head = tables(|faces, _| faces[0].summary = summary);
        assert_eq!(refused(&head, &triangle()), bad, "{summary:?}");
    }
    let alias = |part| FaceKey {
        feature: 2,
        part,
        instance: 0,
    };
    for aliases in [
        vec![alias(PartKey::EndCap), alias(PartKey::StartCap)],
        vec![alias(PartKey::StartCap), alias(PartKey::StartCap)],
        vec![face().key],
    ] {
        let head = tables(|faces, _| faces[0].aliases = aliases.clone());
        assert_eq!(refused(&head, &triangle()), bad, "{aliases:?}");
    }
    // Sorted, apart from the key: taken.
    let head = tables(|faces, _| {
        faces[0].aliases = vec![alias(PartKey::StartCap), alias(PartKey::EndCap)];
    });
    assert!(matches!(
        decode_reply(&head.encode()[..], &slices(&triangle())),
        Ok(Response::Regenerated { .. })
    ));
}

#[test]
fn parts_must_be_of_listed_bodies() {
    let mut head = regenerated(1);
    if let Head::Regenerated { bodies, .. } = &mut head {
        bodies.clear();
    }
    assert_eq!(
        refused(&head, &triangle()),
        Error::Picking(PickingError::Body).to_string()
    );
}

/// The reply to two plates, one above the other: real tables of two
/// bodies, fourteen faces and their summaries.
fn two_plates() -> (Vec<u8>, Vec<Vec<u8>>) {
    let mut editor = Editor::new(Document::example());
    crate::history::tests::plate_below(&mut editor);
    let response = handle(decode_request(&encode_request(&regenerate(&editor))).unwrap());
    let (head, parts) = encode_reply(&response);
    let parts: Vec<Vec<u8>> = parts.iter().map(|part| part.to_vec()).collect();
    // It round-trips as it is.
    let Response::Regenerated { picking, .. } = decode_reply(&head[..], &slices(&parts)).unwrap()
    else {
        panic!("the plates were refused");
    };
    assert_eq!(picking.faces().len(), 14);
    assert_eq!(picking.bodies().len(), 2);
    (head, parts)
}

#[test]
fn damaged_picking_tables_never_panic() {
    let (head, parts) = two_plates();
    let mut rng = Rng(0xfeed);
    for _ in 0..3000 {
        let damaged_head = rng.mutate(&head);
        let mut damaged = parts.clone();
        // Mostly the parts the tables go by: the faces', edges' and
        // parts' ends, the edges' faces and corners.
        let part = if rng.below(2) == 0 {
            [3, 5, 6, 8, 9][rng.below(5)]
        } else {
            rng.below(damaged.len())
        };
        damaged[part] = rng.mutate(&damaged[part]);
        for (head, parts) in [
            (&damaged_head, &parts),
            (&head, &damaged),
            (&damaged_head, &damaged),
        ] {
            decode_any(head, parts);
        }
    }
}

/// The parts the tables go by, cut short or longer, are refused: the
/// tables no longer match the mesh, or the mesh isn't one.
#[test]
fn cut_short_parts_the_tables_go_by_are_refused() {
    let (head, parts) = two_plates();
    for part in [3, 5, 9] {
        for cut in [4, 8, parts[part].len()] {
            let mut short = parts.clone();
            let len = short[part].len() - cut;
            short[part].truncate(len);
            match decode_reply(&head[..], &slices(&short)).unwrap() {
                Response::Failed { .. } => {}
                Response::Regenerated { .. } | Response::Exported { .. } => {
                    panic!("part {part} cut by {cut} was taken")
                }
            }
        }
        // Or with more in it.
        let mut long = parts.clone();
        let last = long[part][long[part].len() - 4..].to_vec();
        long[part].extend(last);
        assert!(matches!(
            decode_reply(&head[..], &slices(&long)).unwrap(),
            Response::Failed { .. }
        ));
    }
}

/// Sequences decoded within bounds take at most so many elements, and
/// at most so much of their weight together, and are refused past
/// either.
#[test]
fn bounded_sequences_are_refused_past_their_bounds() {
    use crate::picking::bounded::seq;
    let decode = |value: &Vec<Vec<u8>>, max: usize, budget: usize| {
        let bytes = postcard::to_stdvec(value).unwrap();
        let mut de = postcard::Deserializer::from_bytes(&bytes);
        seq(&mut de, max, |v: &Vec<u8>| v.len(), budget)
    };
    let three = vec![vec![1], vec![2, 3], vec![]];
    assert_eq!(decode(&three, 3, 3).unwrap(), three);
    assert!(decode(&three, 2, 3).is_err());
    assert!(decode(&three, 3, 2).is_err());
    assert_eq!(decode(&Vec::new(), 0, 0).unwrap(), Vec::<Vec<u8>>::new());
}

/// A head claiming more parts, faces, aliases, closed flags or tangent
/// chains than a reply may have is refused as it's decoded, before any of
/// them is built: each costs the page far more memory than its bytes in
/// the head.
#[test]
fn too_many_parts_faces_aliases_or_flags_are_refused_as_the_head_is_decoded() {
    // One part, one face with one alias, one flag and one tangent chain,
    // then each count claimed larger: the bytes for the claim, at the
    // count's place.
    let head = tables(|faces, _| {
        faces[0].aliases.push(FaceKey {
            feature: 2,
            part: PartKey::StartCap,
            instance: 0,
        });
    })
    .encode();
    let Head::Regenerated { faces, parts, .. } = Head::decode(&head).unwrap() else {
        unreachable!()
    };
    let body = postcard::to_stdvec(&parts[0]).unwrap();
    let face = postcard::to_stdvec(&faces[0]).unwrap();
    let summary = postcard::to_stdvec(&faces[0].summary).unwrap();
    let alias = postcard::to_stdvec(&faces[0].aliases[0]).unwrap();
    // [.. parts: 1, body, faces: 1, face [.., aliases: 1, alias,
    // summary], closed: 1, false, tangents: 1, 0, snaps: 1, None,
    // corners: 0, inspected: None]
    let snaps_at = head.len() - 4;
    let tangents_at = snaps_at - 2;
    let closed_at = tangents_at - 2;
    let faces_at = closed_at - face.len() - 1;
    let aliases_at = closed_at - summary.len() - alias.len() - 1;
    let parts_at = faces_at - body.len() - 1;
    for (at, max) in [
        (parts_at, RenderMesh::MAX_PARTS),
        (faces_at, MAX_FACES),
        (aliases_at, Picking::MAX_ALIASES),
        (closed_at, RenderMesh::MAX_EDGE_POLYLINES),
        (tangents_at, RenderMesh::MAX_EDGE_POLYLINES),
        (snaps_at, RenderMesh::MAX_EDGE_POLYLINES),
    ] {
        assert_eq!(head[at], 1);
        for claim in [max as u64 + 1, u64::MAX] {
            let mut claimed = head[..at].to_vec();
            claimed.extend(postcard::to_stdvec(&claim).unwrap());
            claimed.extend(&head[at + 1..]);
            assert!(Head::decode(&claimed).is_err(), "{claim} at {at}");
        }
    }
    // All faces' aliases together: two faces of half as many and one
    // more.
    let aliased = |aliases: usize| {
        tables(|faces, _| {
            faces[0].aliases = (0..aliases as u64)
                .map(|instance| FaceKey {
                    feature: 2,
                    part: PartKey::StartCap,
                    instance,
                })
                .collect();
            faces.push(faces[0].clone());
        })
        .encode()
    };
    assert!(Head::decode(&aliased(Picking::MAX_ALIASES / 2)).is_ok());
    assert!(Head::decode(&aliased(Picking::MAX_ALIASES / 2 + 1)).is_err());
}

/// A model with more faces than a reply may carry is answered as failed
/// for its generation, not sent for the page to refuse.
#[test]
fn a_model_with_too_many_faces_is_answered_as_failed() {
    let response = |faces: usize| {
        // As many triangles, all on the one vertex triple, a face each.
        let mesh = RenderMesh::from_parts(MeshParts {
            positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            indices: [0, 1, 2].repeat(faces),
            face_ends: (1..=faces as u32).map(|f| 3 * f).collect(),
            part_ends: vec![[faces as u32, 0, 0, 0]],
            ..MeshParts::default()
        })
        .unwrap();
        let picking = Picking::from_parts(
            vec![BodyId::NEW],
            vec![face(); faces],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            &mesh,
        )
        .unwrap();
        Response::Regenerated {
            generation: 4.into(),
            exclude: None,
            draft: None,
            mesh: Arc::new(mesh),
            picking: Arc::new(picking),
            sketches: Arc::new(RenderLines::default()),
            unsolved: Vec::new(),
            failed: Vec::new(),
            touched: Vec::new(),
            merged: Vec::new(),
            placements: Vec::new(),
            bodies: vec![(
                BodyId::NEW,
                Aabb {
                    min: Vec3::ZERO,
                    max: Vec3::ONE,
                },
            )],
            inspected: None,
        }
    };
    assert!(matches!(
        round_trip(&response(3)),
        Response::Regenerated { .. }
    ));
    match round_trip(&response(MAX_FACES + 1)) {
        Response::Failed { generation, .. } => assert_eq!(generation, 4.into()),
        Response::Regenerated { .. } | Response::Exported { .. } => {
            panic!("too many faces were sent")
        }
    }
}

#[test]
fn an_export_round_trips_with_its_meshes_checked() {
    let editor = Editor::new(Document::example());
    let request = Request::Export {
        export: 3,
        document: editor.snapshot(),
    };
    let decoded = decode_request(&encode_request(&request)).unwrap();
    assert!(
        matches!(&decoded, Request::Export { export: 3, document } if **document == *editor.document())
    );
    let response = handle(decoded);
    let Response::Exported {
        result: Ok(bodies), ..
    } = &response
    else {
        panic!("the plate wasn't exported: {response:?}");
    };
    let (head, parts) = encode_reply(&response);
    assert_eq!(parts.len(), 1);
    let back = round_trip(&response);
    assert!(matches!(&back, Response::Exported { export: 3, result: Ok(back) } if back == bodies));

    // An error crosses with no parts.
    let failed = Response::Exported {
        export: 4,
        result: Err("Body 1 can't be exported: it's too large".to_owned()),
    };
    assert!(encode_reply(&failed).1.is_empty());
    assert!(matches!(
        round_trip(&failed),
        Response::Exported { export: 4, result: Err(error) } if error.starts_with("Body 1")
    ));

    // Bodies whose mesh isn't a manifold, or missing, or cut short, or
    // with bytes after them, answer the export with an error.
    let mut bodies = bodies.clone();
    let mesh = &bodies[0].mesh;
    let mut triangles = mesh.triangles().to_vec();
    triangles.swap_remove(0);
    #[derive(serde::Serialize)]
    struct Unchecked<'a> {
        origin: [f64; 3],
        positions: &'a [[f64; 3]],
        triangles: &'a [[u32; 3]],
    }
    #[derive(serde::Serialize)]
    struct Body<'a> {
        body: varde_document::BodyId,
        name: &'a str,
        mesh: Unchecked<'a>,
    }
    let open = postcard::to_stdvec(&[Body {
        body: bodies[0].body,
        name: &bodies[0].name,
        mesh: Unchecked {
            origin: mesh.origin(),
            positions: mesh.positions(),
            triangles: &triangles,
        },
    }])
    .unwrap();
    let whole = postcard::to_stdvec(&bodies).unwrap();
    let mut longer = whole.clone();
    longer.push(0);
    for parts in [
        vec![&open[..]],
        vec![],
        vec![&whole[..whole.len() - 1]],
        vec![&longer[..]],
        vec![&whole[..], &whole[..]],
    ] {
        let reply = decode_reply(&head[..], &parts).unwrap();
        assert!(
            matches!(
                reply,
                Response::Exported {
                    export: 3,
                    result: Err(_)
                }
            ),
            "{reply:?}"
        );
    }
    bodies.clear();
    assert!(
        decode_export(&[&postcard::to_stdvec(&bodies).unwrap()[..]])
            .unwrap()
            .is_empty()
    );
}

/// An edge's tangent chain's first is a first, of the same part, no
/// later than its members, between two faces; a crease's is itself:
/// anything else is refused.
#[test]
fn tangent_chains_must_hang_together() {
    // Four triangles, a face each, and two edges: between faces 0 and 1,
    // and between faces 2 and 3 (or a crease of face 2), in one part or
    // two.
    let mesh = |parts: usize, crease: bool| {
        let p = |i: usize| [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]][i];
        RenderMesh::from_parts(MeshParts {
            positions: vec![p(0), p(1), p(2)],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            indices: [0, 1, 2].repeat(4),
            face_ends: vec![3, 6, 9, 12],
            edge_vertices: vec![1, 2, 0, 1],
            edge_ends: vec![2, 4],
            edge_faces: vec![[0, 1], if crease { [2, 2] } else { [2, 3] }],
            corners: vec![p(1), p(2), p(0), p(1)],
            edge_corners: vec![[0, 1], [2, 3]],
            part_ends: if parts == 1 {
                vec![[4, 2, 4, 0]]
            } else {
                vec![[2, 1, 2, 0], [4, 2, 4, 0]]
            },
            ..MeshParts::default()
        })
        .unwrap()
    };
    let [a, b, _] = ids();
    let tables = |tangents: [u32; 2], parts: usize, crease: bool| {
        let mesh = mesh(parts, crease);
        let faces = [PartKey::StartCap, PartKey::EndCap].repeat(2);
        let faces = (faces.into_iter())
            .map(|part| {
                let mut face = face();
                face.key.part = part;
                face
            })
            .collect();
        let bodies = [a, b][..parts].to_vec();
        Picking::from_parts(
            bodies,
            faces,
            vec![false; 2],
            tangents.to_vec(),
            vec![None; 2],
            Vec::new(),
            &mesh,
        )
    };
    assert!(tables([0, 1], 1, false).is_ok());
    assert!(tables([0, 0], 1, false).is_ok());
    for tangents in [[1, 1], [1, 0], [0, 2], [2, 2]] {
        assert_eq!(
            tables(tangents, 1, false),
            Err(PickingError::Tangent),
            "{tangents:?}"
        );
    }
    assert!(tables([0, 1], 2, false).is_ok());
    // A tangent chain across two parts.
    assert_eq!(tables([0, 0], 2, false), Err(PickingError::Tangent));
    // A crease is its own, and no chain's.
    assert!(tables([0, 1], 1, true).is_ok());
    assert_eq!(tables([0, 0], 1, true), Err(PickingError::Tangent));
}

/// A sketch on the example plate's top, shown: its placement crosses
/// with the model, and its lines drawn there.
#[test]
fn placements_round_trip() {
    use varde_document::{FaceRef, Plane};
    let mut editor = Editor::new(Document::example());
    let top = FaceRef {
        body: editor.document().bodies()[0].id,
        key: FaceKey {
            feature: editor.document().features()[1].id.get(),
            part: PartKey::EndCap,
            instance: 0,
        },
        near: glam::DVec3::new(20.0, 0.0, 10.0),
    };
    editor
        .apply(editor.document().add_sketch(Plane::Face(top)))
        .unwrap();
    let sketch = editor.document().features().last().unwrap().id;
    let FeatureKind::Sketch { sketch: drawn, .. } = &editor.document().features()[0].kind else {
        unreachable!()
    };
    let drawn = Box::new(drawn.clone());
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: drawn,
        })
        .unwrap();
    editor
        .apply(Command::SetFeatureVisible(sketch, true))
        .unwrap();
    let response = handle(decode_request(&encode_request(&regenerate(&editor))).unwrap());
    let Response::Regenerated {
        placements: sent, ..
    } = &response
    else {
        panic!("regeneration failed");
    };
    let Response::Regenerated {
        placements,
        sketches,
        failed,
        ..
    } = round_trip(&response)
    else {
        panic!("regeneration failed");
    };
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(placements, *sent);
    let [(id, placement)] = placements[..] else {
        panic!("one placement: {placements:?}");
    };
    assert_eq!(id, sketch);
    assert_eq!(placement.origin, glam::DVec3::new(0.0, 0.0, 10.0));
    assert_eq!(sketches.ends().len(), 5);
    assert!(sketches.points().iter().all(|point| point[2] == 10.0));
}

/// A placement that isn't one, or a sketch placed twice, answers the
/// generation as failed.
#[test]
fn hostile_placements_are_refused() {
    let document = Document::example();
    let [a, b] = [0, 1].map(|i| document.features()[i].id);
    let good = [
        [0.0, 0.0, 10.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];
    let with = |change: fn(&mut [[f64; 3]; 4])| {
        let mut placement = good;
        change(&mut placement);
        vec![(a, placement)]
    };
    let max = f64::from(varde_document::MAX_COORD);
    for bad in [
        with(|p| p[0][0] = f64::NAN),
        with(|p| p[0][2] = f64::INFINITY),
        with(|p| p[0][1] = 2.0 * f64::from(varde_document::MAX_COORD)),
        // Not unit.
        with(|p| p[1][0] = 1.0 + 1e-6),
        // Not square.
        with(|p| p[2] = [1e-6, 1.0, 0.0]),
        // The normal isn't x × y.
        with(|p| p[3] = [0.0, 0.0, -1.0]),
        with(|p| p[3] = [0.0, 0.0, 0.0]),
        vec![(a, good), (b, good), (a, good)],
    ] {
        let mut head = regenerated(7);
        if let Head::Regenerated { placements, .. } = &mut head {
            *placements = bad.clone();
        }
        let Response::Failed {
            generation, error, ..
        } = decode_reply(&head.encode()[..], &slices(&triangle())).unwrap()
        else {
            panic!("bad placements were taken: {bad:?}");
        };
        assert_eq!(u64::from(generation), 7);
        assert_eq!(error, Error::Placement.to_string());
    }
    // At the bound, and two sketches, are fine.
    let mut head = regenerated(8);
    let mut far = good;
    far[0] = [max, -max, max];
    if let Head::Regenerated { placements, .. } = &mut head {
        *placements = vec![(a, far), (b, good)];
    }
    let reply = decode_reply(&head.encode()[..], &slices(&triangle())).unwrap();
    let Response::Regenerated { placements, .. } = reply else {
        panic!("good placements were refused");
    };
    assert_eq!(placements.len(), 2);
    assert_eq!(placements[0].1.origin, glam::DVec3::new(max, -max, max));
}

/// A request measuring two faces of the example round trips, and so does
/// its answer, measures and all.
#[test]
fn a_measure_round_trips() {
    let editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let Response::Regenerated { picking, .. } = handle(regenerate(&editor)) else {
        panic!("regeneration failed");
    };
    assert_eq!(picking.corners().len(), 8);
    let pick = |f: usize, near: [f64; 3]| crate::InspectPick {
        body,
        entity: crate::Entity::Face(picking.faces()[f].key),
        near,
    };
    let plane = |d: f64| {
        let n = [0.0, 0.0, if d > 0.0 { 1.0 } else { -1.0 }];
        (picking.faces().iter())
            .position(|f| f.summary == crate::Summary::Plane { n, d: d.abs() })
            .unwrap()
    };
    let inspect = crate::Inspect {
        revision: 6,
        first: pick(plane(10.0), [20.0, 10.0, 10.0]),
        second: Some(crate::InspectPick {
            entity: crate::Entity::Corner(picking.corner_keys(0)),
            near: picking.corners()[0].point,
            ..pick(0, [0.0; 3])
        }),
    };
    let mut request = regenerate(&editor);
    if let Request::Regenerate { inspect: asked, .. } = &mut request {
        *asked = Some(Box::new(inspect.clone()));
    }
    let decoded = decode_request(&encode_request(&request)).unwrap();
    let Request::Regenerate { inspect: back, .. } = &decoded else {
        panic!("not a regeneration");
    };
    assert_eq!(back.as_ref(), Some(&Box::new(inspect.clone())));
    let response = handle(decoded);
    let Response::Regenerated {
        inspected: Some(sent),
        ..
    } = &response
    else {
        panic!("regeneration failed");
    };
    assert!(sent.first.is_ok() && matches!(sent.second, Some(Ok(_))));
    assert!(sent.between.as_ref().unwrap().distance.is_ok());
    let Response::Regenerated {
        inspected,
        picking: back,
        ..
    } = round_trip(&response)
    else {
        panic!("the reply was refused");
    };
    assert_eq!(inspected.as_ref(), Some(sent));
    assert_eq!(*back, *picking);
}

/// A broken measure in a reply is answered as an error, the model with
/// it taken as usual.
#[test]
fn a_broken_measure_is_answered_as_an_error() {
    let measure = |measure| crate::Inspected {
        revision: 2,
        first: Ok(crate::Probed {
            at: Some(crate::At::Face(0)),
            measure: Ok(measure),
        }),
        second: None,
        between: None,
    };
    for (good, sent) in [
        (true, measure(face_measure(1.0))),
        (false, measure(face_measure(f64::NAN))),
        // A point isn't what a face measures.
        (false, measure(crate::Measure::Point([1.0, 2.0, 3.0]))),
        (
            false,
            crate::Inspected {
                first: Ok(crate::Probed {
                    at: Some(crate::At::Edge(0)),
                    measure: Err("too complex to measure".to_owned()),
                }),
                ..measure(crate::Measure::Point([0.0; 3]))
            },
        ),
        (
            false,
            crate::Inspected {
                first: Ok(crate::Probed {
                    at: Some(crate::At::Corner(0)),
                    measure: Err("too complex to measure".to_owned()),
                }),
                ..measure(crate::Measure::Point([0.0; 3]))
            },
        ),
    ] {
        let mut head = regenerated(1);
        if let Head::Regenerated { inspected, .. } = &mut head {
            *inspected = Some(Box::new(sent.clone()));
        }
        let reply = decode_reply(&head.encode()[..], &slices(&triangle())).unwrap();
        assert_eq!(reply.inspect(), Some(2));
        let Response::Regenerated {
            inspected: Some(inspected),
            ..
        } = reply
        else {
            panic!("the model was refused");
        };
        if good {
            assert_eq!(*inspected, sent);
        } else {
            assert!(inspected.first.is_err(), "{inspected:?}");
        }
    }
}

/// What [`face`] measures, of `area`.
fn face_measure(area: f64) -> crate::Measure {
    crate::Measure::Face {
        area,
        summary: face().summary,
        half_angle: None,
    }
}

/// A model refused fails its generation with the measure's revision.
#[test]
fn a_refused_model_keeps_its_measure_s_revision() {
    let mut head = regenerated(1);
    if let Head::Regenerated { inspected, .. } = &mut head {
        *inspected = Some(Box::new(crate::Inspected {
            revision: 8,
            first: Err("face not found".to_owned()),
            second: None,
            between: None,
        }));
    }
    let reply = decode_reply(&head.encode()[..], &slices(&triangle()[..7])).unwrap();
    assert!(matches!(reply, Response::Failed { .. }));
    assert_eq!(reply.inspect(), Some(8));
}

/// Three triangles round a vertex, a face each, in one part or (if
/// `split`) the first two in one and the third in another, of another
/// body, and their head, with a corner where they meet.
fn fan(split: bool) -> (Head, Vec<Vec<u8>>) {
    let mesh = RenderMesh::from_parts(MeshParts {
        positions: vec![
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [-1.0, -1.0, 0.0],
        ],
        normals: vec![[0.0, 0.0, 1.0]; 4],
        indices: vec![0, 1, 2, 0, 2, 3, 0, 3, 1],
        face_ends: vec![3, 6, 9],
        part_ends: if split {
            vec![[2, 0, 0, 0], [3, 0, 0, 0]]
        } else {
            vec![[3, 0, 0, 0]]
        },
        ..MeshParts::default()
    })
    .unwrap();
    let other = ids()[1];
    let bodies = if split {
        vec![BodyId::NEW, other]
    } else {
        vec![BodyId::NEW]
    };
    let parts = model_parts(&answer(mesh, bodies.clone()));
    let mut head = regenerated(1);
    if let Head::Regenerated {
        faces,
        corners,
        closed,
        tangents,
        snaps,
        parts,
        bodies: boxes,
        ..
    } = &mut head
    {
        for part in [PartKey::EndCap, PartKey::Side { curve: 0 }] {
            let mut next = face();
            next.key.part = part;
            faces.push(next);
        }
        corners.push(PickCorner {
            faces: [0, 1, 2],
            point: [0.0; 3],
        });
        closed.clear();
        tangents.clear();
        snaps.clear();
        *parts = bodies;
        boxes.push((other, [[0.0; 3], [1.0; 3]]));
    }
    (head, parts)
}

#[test]
fn picked_corners_are_three_faces_of_one_part() {
    let (head, parts) = fan(false);
    let reply = decode_reply(&head.encode()[..], &slices(&parts)).unwrap();
    let Response::Regenerated { picking, .. } = reply else {
        panic!("a good corner was refused");
    };
    assert_eq!(picking.corners().len(), 1);
    let keys = picking.corner_keys(0);
    assert!(keys.windows(2).all(|pair| pair[0] < pair[1]));

    let corner = Error::Picking(PickingError::Corner).to_string();
    type Change = fn(&mut Vec<PickCorner>);
    let changes: [Change; 6] = [
        |corners| corners[0].faces = [0, 2, 1],
        |corners| corners[0].faces = [0, 0, 1],
        |corners| corners[0].faces = [0, 1, 3],
        |corners| corners[0].faces = [0, 1, u32::MAX],
        |corners| corners[0].point = [f64::NAN, 0.0, 0.0],
        |corners| corners[0].point = [0.0, 0.0, 1e9],
    ];
    for change in changes {
        let mut head = head.clone();
        if let Head::Regenerated { corners, .. } = &mut head {
            change(corners);
        }
        assert_eq!(refused(&head, &parts), corner);
    }
    // Faces of two parts.
    let (split, split_parts) = fan(true);
    assert_eq!(refused(&split, &split_parts), corner);
    // More corners than vertices.
    let mut more = head.clone();
    if let Head::Regenerated { corners, .. } = &mut more {
        let first = corners[0];
        corners.extend([first; 4]);
    }
    assert_eq!(refused(&more, &parts), corner);
}

#[test]
fn snap_points_are_within_bounds_and_only_on_chains() {
    let snap = |at: [f64; 3]| {
        let mut head = two_faces(false);
        if let Head::Regenerated { snaps, .. } = &mut head {
            snaps[0] = Some(at);
        }
        head
    };
    let reply = decode_reply(
        &snap([0.5, 0.5, 0.0]).encode()[..],
        &slices(&two_triangles(false)),
    )
    .unwrap();
    let Response::Regenerated { picking, .. } = reply else {
        panic!("a good snap point was refused");
    };
    assert_eq!(picking.snaps(), [Some([0.5, 0.5, 0.0])]);
    let bad = Error::Picking(PickingError::Snap).to_string();
    assert_eq!(
        refused(&snap([0.0, f64::INFINITY, 0.0]), &two_triangles(false)),
        bad
    );
    assert_eq!(refused(&snap([0.0, 2e8, 0.0]), &two_triangles(false)), bad);
    // A crease has none.
    let mut crease = regenerated(1);
    if let Head::Regenerated { snaps, .. } = &mut crease {
        snaps[0] = Some([0.0; 3]);
    }
    assert_eq!(refused(&crease, &triangle()), bad);
}

#[test]
fn too_many_corners_are_refused_as_the_head_is_decoded() {
    let (head, _) = fan(false);
    let head = head.encode();
    assert!(Head::decode(&head).is_ok());
    let corner = postcard::to_stdvec(&PickCorner {
        faces: [0, 1, 2],
        point: [0.0; 3],
    })
    .unwrap();
    // [.., corners: 1, corner, inspected: None]
    let at = head.len() - corner.len() - 1 - 1;
    assert_eq!(head[at], 1);
    for claim in [MAX_CORNERS as u64 + 1, u64::MAX] {
        let mut claimed = head[..at].to_vec();
        claimed.extend(postcard::to_stdvec(&claim).unwrap());
        claimed.extend(&head[at + 1..]);
        assert!(Head::decode(&claimed).is_err(), "{claim}");
    }
}

/// Checks, apart from [`Inspected::checked`], what an accepted measure
/// promises the panel and the viewport: numbers finite and in range,
/// places within the tables and of the kind measured, a distance only
/// between two found picks and its points'.
fn assert_sound_measure(inspected: &Inspected, mesh: &RenderMesh, picking: &Picking) {
    let value = |x: f64| assert!(x.is_finite() && x.abs() <= Picking::MAX_VALUE, "{x}");
    let size = |x: f64| {
        value(x);
        assert!(x >= 0.0, "{x}");
    };
    let point = |p: [f64; 3]| p.into_iter().for_each(value);
    let unit = |v: [f64; 3]| {
        let length = glam::DVec3::from(v).length();
        assert!((length - 1.0).abs() <= Picking::UNIT, "{v:?}");
    };
    let probed = |probed: &crate::Probed| {
        match probed.at {
            None => {}
            Some(crate::At::Face(f)) => assert!((f as usize) < picking.faces().len()),
            Some(crate::At::Edge(e)) => {
                let [a, b] = mesh.edge_faces()[e as usize];
                assert_ne!(a, b, "a chain");
            }
            Some(crate::At::Corner(c)) => assert!((c as usize) < picking.corners().len()),
        }
        let Ok(measure) = &probed.measure else {
            return;
        };
        match (*measure, probed.at) {
            (
                crate::Measure::Body {
                    volume,
                    area,
                    centre,
                    bounds,
                },
                None,
            ) => {
                size(volume);
                size(area);
                centre.into_iter().for_each(point);
                if let Some([min, max]) = bounds {
                    point(min);
                    point(max);
                    assert!((0..3).all(|i| min[i] <= max[i]));
                }
            }
            (
                crate::Measure::Face {
                    area,
                    summary,
                    half_angle,
                },
                None | Some(crate::At::Face(_)),
            ) => {
                size(area);
                assert!(summary.valid());
                if let Some(a) = half_angle {
                    assert!((0.0..=std::f64::consts::FRAC_PI_2).contains(&a));
                }
            }
            (crate::Measure::Edge { length, shape, .. }, None | Some(crate::At::Edge(_))) => {
                size(length);
                match shape {
                    crate::EdgeForm::Line { from, to } => {
                        point(from);
                        point(to);
                    }
                    crate::EdgeForm::Circle {
                        centre,
                        axis,
                        radius,
                    } => {
                        point(centre);
                        unit(axis);
                        size(radius);
                        assert!(radius > 0.0);
                    }
                    crate::EdgeForm::Ellipse {
                        centre,
                        axis,
                        major,
                        minor,
                    } => {
                        point(centre);
                        unit(axis);
                        size(major);
                        assert!(minor > 0.0 && minor <= major);
                    }
                    crate::EdgeForm::Other => {}
                }
            }
            (crate::Measure::Point(p), None | Some(crate::At::Edge(_) | crate::At::Corner(_))) => {
                point(p)
            }
            (measure, at) => panic!("{measure:?} at {at:?}"),
        }
    };
    if let Ok(first) = &inspected.first {
        probed(first);
    }
    if let Some(Ok(second)) = &inspected.second {
        probed(second);
    }
    if let Some(between) = &inspected.between {
        assert!(inspected.first.is_ok() && matches!(inspected.second, Some(Ok(_))));
        if let Some(angle) = between.angle {
            assert!((0.0..=std::f64::consts::PI).contains(&angle));
        }
        if let Ok(gap) = &between.distance {
            size(gap.distance);
            gap.points.into_iter().for_each(point);
            let [p, q] = gap.points.map(glam::DVec3::from);
            assert!((p.distance(q) - gap.distance).abs() <= 1e-6);
        }
    }
}

/// The example's reply to measures of every kind: a rim and the body, a
/// corner and the top, the rim's centre and the hole's wall.
fn measured_replies() -> Vec<(Vec<u8>, Vec<Vec<u8>>)> {
    let editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let Response::Regenerated { mesh, picking, .. } = handle(regenerate(&editor)) else {
        panic!("regeneration failed");
    };
    let faces = picking.faces();
    let face = |f: usize| crate::InspectPick {
        body,
        entity: crate::Entity::Face(faces[f].key),
        near: [0.0, 8.0, 5.0],
    };
    let wall = (faces.iter())
        .position(|f| matches!(f.summary, crate::Summary::Cylinder { .. }))
        .unwrap();
    let top = (faces.iter())
        .position(|f| matches!(f.summary, crate::Summary::Plane { n, .. } if n[2] == 1.0))
        .unwrap();
    let rim = picking.closed().iter().position(|&c| c).unwrap() as u32;
    let edge = crate::InspectPick {
        body,
        entity: crate::Entity::Edge(picking.edge_keys(&mesh, rim).unwrap()),
        near: [8.0, 0.0, 10.0],
    };
    let centre = crate::InspectPick {
        entity: crate::Entity::EdgePoint(picking.edge_keys(&mesh, rim).unwrap()),
        ..edge
    };
    let corner = crate::InspectPick {
        body,
        entity: crate::Entity::Corner(picking.corner_keys(0)),
        near: picking.corners()[0].point,
    };
    let whole = crate::InspectPick {
        body,
        entity: crate::Entity::Body,
        near: [0.0; 3],
    };
    let mut near_top = face(top);
    near_top.near = [20.0, 10.0, 10.0];
    [(edge, whole), (corner, near_top), (centre, face(wall))]
        .into_iter()
        .enumerate()
        .map(|(k, (first, second))| {
            let mut request = regenerate(&editor);
            if let Request::Regenerate { inspect, .. } = &mut request {
                *inspect = Some(Box::new(crate::Inspect {
                    revision: k as u64,
                    first,
                    second: Some(second),
                }));
            }
            let response = handle(request);
            let Response::Regenerated {
                inspected: Some(inspected),
                ..
            } = &response
            else {
                panic!("regeneration failed");
            };
            assert!(inspected.first.as_ref().is_ok_and(|p| p.measure.is_ok()));
            let second = inspected.second.as_ref().unwrap();
            assert!(second.as_ref().is_ok_and(|p| p.measure.is_ok()));
            assert!(inspected.between.as_ref().unwrap().distance.is_ok());
            let (head, parts) = encode_reply(&response);
            (head, parts.iter().map(|part| part.to_vec()).collect())
        })
        .collect()
}

/// Hostile bytes where a measure's answer is: in the head of a real reply
/// and as an [`Inspected`] of its own. Never a panic, and whatever is
/// taken holds.
#[test]
fn damaged_measures_never_panic_and_what_is_taken_holds() {
    let mut rng = Rng(0x1235);
    let mut taken = 0;
    for (head, parts) in measured_replies() {
        let Ok(Response::Regenerated {
            inspected: Some(sent),
            mesh,
            picking,
            ..
        }) = decode_reply(&head[..], &slices(&parts))
        else {
            panic!("the reply was refused");
        };
        assert_sound_measure(&sent, &mesh, &picking);
        let bytes = postcard::to_stdvec(&*sent).unwrap();
        // The measure is at the end of the head.
        assert!(head.ends_with(&bytes));
        let at = head.len() - bytes.len();
        for _ in 0..3000 {
            // Damage only the measure's bytes, so most heads still
            // decode: the measure's checks are what's tried.
            let mut damaged = head[..at].to_vec();
            damaged.extend(rng.mutate(&bytes));
            if let Ok(Response::Regenerated {
                inspected: Some(inspected),
                mesh,
                picking,
                ..
            }) = decode_reply(&damaged[..], &slices(&parts))
            {
                assert_sound_measure(&inspected, &mesh, &picking);
                taken += 1;
            }
            decode_any(&rng.mutate(&head), &parts);
            if let Ok(inspected) = postcard::from_bytes::<Inspected>(&rng.mutate(&bytes)) {
                assert_sound_measure(&inspected.checked(&mesh, &picking), &mesh, &picking);
            }
            if let Ok(inspected) = postcard::from_bytes::<Inspected>(&rng.bytes(96)) {
                assert_sound_measure(&inspected.checked(&mesh, &picking), &mesh, &picking);
            }
        }
    }
    assert!(taken > 1000, "{taken}");
}
