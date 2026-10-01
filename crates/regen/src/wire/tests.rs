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
    }
}

/// The head of a regenerated `generation`, whose model is in its parts
/// ([`triangle`]): one body, whose one face is the triangle's.
fn regenerated(generation: u64) -> Head {
    Head::Regenerated {
        generation: generation.into(),
        exclude: None,
        draft: None,
        unsolved: Vec::new(),
        failed: Vec::new(),
        touched: Vec::new(),
        merged: Vec::new(),
        bodies: vec![(BodyId::NEW, [[0.0; 3], [1.0; 3]])],
        faces: vec![face()],
        chains: Vec::new(),
    }
}

/// The face of [`triangle`]'s one triangle: a plane's, of the body
/// [`regenerated`] lists.
fn face() -> PickFace {
    PickFace {
        body: BodyId::NEW,
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
    let (head, mesh) = encode_reply(response);
    decode_reply(&head[..], mesh.as_ref().map_or(&[][..], |mesh| &mesh[..])).unwrap()
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
    } = decode_request(&bytes).unwrap();
    assert_eq!(generation, editor.generation());
    assert_eq!(*document, *editor.document());
    assert_eq!(exclude, None);
    assert_eq!(draft, None);
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
        extrude: extrude.clone(),
    };
    let request = Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude: None,
        draft: Some(draft.clone()),
    };
    let decoded = decode_request(&encode_request(&request)).unwrap();
    assert_eq!(decoded.draft(), Some(7));
    let Request::Regenerate { draft: back, .. } = decoded;
    assert_eq!(back, Some(draft));

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
        draft: Some(Draft {
            revision: 8,
            feature: None,
            extrude: join,
        }),
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
    };
    let Request::Regenerate {
        document, exclude, ..
    } = decode_request(&encode_request(&request)).unwrap();
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
        bodies,
    } = round_trip(&response)
    else {
        panic!("regeneration failed");
    };
    assert_eq!(generation, editor.generation());
    assert_eq!(exclude, None);
    assert_eq!(draft, None);
    assert_eq!(marked, [unsolved]);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].0, cut);
    // The join takes out the one body there is, so touches none.
    assert_eq!(touched, [(cut, Vec::new())]);
    assert!(merged.is_empty());
    assert_eq!(mesh, *sent);
    assert_eq!(picking, *picked);
    // The plate with a hole: top, bottom, four sides and the hole's wall.
    assert_eq!(picking.faces().len(), 7);
    assert_eq!(picking.triangles().len(), mesh.triangle_count());
    assert!(mesh.triangle_count() > 0);
    assert!(!mesh.edges().is_empty());
    assert_eq!(bodies, *boxes);
    assert_eq!(bodies.len(), 1);
    assert_eq!(
        *sketches,
        crate::flatten_sketches(editor.document(), None).unwrap()
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
        error: "the kernel gave up".to_owned(),
    };
    assert!(encode_reply(&response).1.is_none());
    let Response::Failed {
        generation,
        exclude,
        draft,
        error,
    } = round_trip(&response)
    else {
        panic!("not a failure");
    };
    assert_eq!(u64::from(generation), u64::MAX);
    assert_eq!(exclude, Some(feature));
    assert_eq!(draft, Some(2));
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
    assert_eq!(error, Error::Parts(7).to_string());
}

#[test]
fn unbounded_line_points_fail_their_generation() {
    for bad in [f32::INFINITY, -1e38] {
        let mut parts = triangle();
        parts[4][8..12].copy_from_slice(&bad.to_ne_bytes());
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

/// The parts of a model of one triangle and a polyline of two segments
/// around it, as owned bytes to break.
fn triangle() -> Vec<Vec<u8>> {
    let mesh = RenderMesh::from_parts(
        vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        vec![[0.0, 0.0, 1.0]; 3],
        vec![0, 1, 2],
        vec![[0, 1], [1, 2], [2, 0]],
    )
    .unwrap();
    let lines = RenderLines::from_parts(
        vec![[0.0, -1.0, 0.0], [2.0, -1.0, 0.0], [0.0, 2.0, 0.0]],
        vec![3],
    )
    .unwrap();
    let picking = Picking::from_parts(
        vec![face()],
        Vec::new(),
        vec![0],
        vec![Picking::NONE; 3],
        &mesh,
    )
    .unwrap();
    let response = Response::Regenerated {
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
        bodies: Vec::new(),
    };
    let (_, mesh) = encode_reply(&response);
    mesh.unwrap().map(<[u8]>::to_vec).to_vec()
}

fn slices(parts: &[Vec<u8>]) -> Vec<&[u8]> {
    parts.iter().map(Vec::as_slice).collect()
}

/// [`decode_model`] with the tables of [`regenerated`].
fn decode_all(parts: &[impl Buffer]) -> Result<(RenderMesh, RenderLines, Picking), Error> {
    decode_model(parts, vec![face()], Vec::new())
}

fn decode(parts: &[Vec<u8>]) -> Result<RenderMesh, Error> {
    decode_all(&slices(parts)).map(|(mesh, _, _)| mesh)
}

fn decode_lines(parts: &[Vec<u8>]) -> Result<RenderLines, Error> {
    decode_all(&slices(parts)).map(|(_, lines, _)| lines)
}

#[test]
fn triangle_decodes() {
    let (mesh, lines, picking) = decode_all(&slices(&triangle())).unwrap();
    assert_eq!(mesh.triangle_count(), 1);
    assert_eq!(lines.segment_count(), 2);
    assert_eq!(picking.triangles(), [0]);
    assert_eq!(picking.edges(), [Picking::NONE; 3]);
}

#[test]
fn wrong_number_of_parts_is_an_error() {
    let mut parts = triangle();
    parts.pop();
    assert_eq!(decode(&parts), Err(Error::Parts(7)));
    assert_eq!(decode(&[]), Err(Error::Parts(0)));
    parts.extend([Vec::new(), Vec::new()]);
    assert_eq!(decode(&parts), Err(Error::Parts(9)));
}

#[test]
fn partial_elements_are_an_error() {
    for (part, name) in [
        (0, Part::RenderMesh(MeshPart::Positions)),
        (2, Part::RenderMesh(MeshPart::Indices)),
        (3, Part::RenderMesh(MeshPart::Edges)),
        (4, Part::RenderLines(LinesPart::Points)),
        (5, Part::RenderLines(LinesPart::Ends)),
        (6, Part::Picking(PickingPart::Triangles)),
        (7, Part::Picking(PickingPart::Edges)),
    ] {
        let mut parts = triangle();
        parts[part].pop();
        if part == 0 {
            parts[1].pop();
        }
        let len = parts[part].len();
        assert_eq!(
            decode_all(&slices(&parts)).map(|_| ()),
            Err(Error::Partial { part: name, len })
        );
    }
}

#[test]
fn line_ends_must_make_polylines() {
    // Ends past the points, and polylines of one point.
    for ends in [&[4u32][..], &[1, 3], &[]] {
        let mut parts = triangle();
        parts[5] = bytemuck::cast_slice(ends).to_vec();
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
    parts[3][20..24].copy_from_slice(&u32::MAX.to_ne_bytes());
    assert_eq!(
        decode(&parts),
        Err(Error::RenderMesh(MeshError::OutOfRange {
            part: MeshPart::Edges,
            index: u32::MAX,
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

#[test]
fn oversized_part_is_an_error() {
    let parts = triangle();
    let (indices, edges) = (&parts[2][..], &parts[3][..]);
    let len = RenderMesh::MAX_VERTICES * size_of::<[f32; 3]>() + 1;
    let (points, ends) = (&parts[4][..], &parts[5][..]);
    let (faces, chains) = (&parts[6][..], &parts[7][..]);
    let huge = Huge(len);
    let parts: [&dyn Buffer; 8] = [
        &huge, &huge, &indices, &edges, &points, &ends, &faces, &chains,
    ];
    assert_eq!(
        decode_all(&parts).map(|_| ()),
        Err(Error::TooLarge {
            part: Part::RenderMesh(MeshPart::Positions),
            len
        })
    );
}

#[test]
fn oversized_line_parts_are_an_error() {
    let parts = triangle();
    let slices = slices(&parts);
    let mesh: Vec<&dyn Buffer> = slices[..4].iter().map(|p| p as &dyn Buffer).collect();
    let (points, ends) = (slices[4], slices[5]);
    let picked: Vec<&dyn Buffer> = slices[6..].iter().map(|p| p as &dyn Buffer).collect();
    let len = RenderLines::MAX_POINTS * size_of::<[f32; 3]>() + 1;
    let huge = Huge(len);
    let parts: Vec<&dyn Buffer> = mesh
        .iter()
        .copied()
        .chain([&huge as &dyn Buffer, &ends])
        .chain(picked.iter().copied())
        .collect();
    assert_eq!(
        decode_all(&parts).map(|_| ()),
        Err(Error::TooLarge {
            part: Part::RenderLines(LinesPart::Points),
            len
        })
    );
    let len = RenderLines::MAX_POLYLINES * size_of::<u32>() + 1;
    let huge = Huge(len);
    let parts: Vec<&dyn Buffer> = mesh
        .iter()
        .copied()
        .chain([&points as &dyn Buffer, &huge])
        .chain(picked.iter().copied())
        .collect();
    assert_eq!(
        decode_all(&parts).map(|_| ()),
        Err(Error::TooLarge {
            part: Part::RenderLines(LinesPart::Ends),
            len
        })
    );
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
    assert_eq!(error.to_string(), "model in 0 parts instead of 8");
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
/// model holds together, which the renderer relies on.
fn decode_any(head: &[u8], parts: &[Vec<u8>]) {
    if let Ok(Response::Regenerated { mesh, sketches, .. }) = decode_reply(head, &slices(parts)) {
        let mut start = 0;
        for &end in sketches.ends() {
            assert!(end >= start + 2);
            start = end;
        }
        assert_eq!(start as usize, sketches.points().len());
        assert_eq!(mesh.positions().len(), mesh.normals().len());
        assert!(mesh.indices().len().is_multiple_of(3));
        let vertices = mesh.positions().len();
        for &index in mesh.indices().iter().chain(mesh.edges().as_flattened()) {
            assert!((index as usize) < vertices);
        }
    }
    if let Ok(Response::Regenerated { mesh, picking, .. }) = decode_reply(head, &slices(parts)) {
        assert_eq!(picking.triangles().len(), mesh.triangle_count());
        assert_eq!(picking.edges().len(), mesh.edges().len());
        let faces = picking.faces().len();
        assert!(picking.triangles().iter().all(|&f| (f as usize) < faces));
        let chains = picking.chains().len();
        assert!((picking.edges().iter()).all(|&c| c == Picking::NONE || (c as usize) < chains));
        for chain in picking.chains() {
            assert!(chain.faces.iter().all(|&f| (f as usize) < faces));
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
        let parts: Vec<_> = (0..rng.below(8)).map(|_| rng.bytes(48)).collect();
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
        Response::Regenerated { .. } => panic!("a hostile reply was taken"),
    }
}

/// `regenerated(1)` with its faces and chains changed by `change`.
fn tables(change: impl FnOnce(&mut Vec<PickFace>, &mut Vec<PickChain>)) -> Head {
    let mut head = regenerated(1);
    if let Head::Regenerated { faces, chains, .. } = &mut head {
        change(faces, chains);
    }
    head
}

#[test]
fn picking_indices_must_be_within_their_tables() {
    let index = Error::Picking(PickingError::Index).to_string();
    // A triangle's face past the table.
    let mut parts = triangle();
    parts[6] = bytemuck::cast_slice(&[1u32]).to_vec();
    assert_eq!(refused(&regenerated(1), &parts), index);
    // An edge's chain where there are none, and not `NONE`.
    for bad in [0u32, u32::MAX - 1] {
        let mut parts = triangle();
        parts[7][4..8].copy_from_slice(&bad.to_ne_bytes());
        assert_eq!(refused(&regenerated(1), &parts), index);
    }
}

#[test]
fn picking_must_have_one_entry_per_triangle_and_edge() {
    let lengths = Error::Picking(PickingError::Lengths).to_string();
    for (part, keep) in [(6, 0), (7, 8), (7, 0)] {
        let mut parts = triangle();
        parts[part].truncate(keep);
        assert_eq!(refused(&regenerated(1), &parts), lengths);
    }
    let mut parts = triangle();
    parts[6].extend(0u32.to_ne_bytes());
    assert_eq!(refused(&regenerated(1), &parts), lengths);
    // More faces than triangles.
    let head = tables(|faces, _| faces.push(face()));
    assert_eq!(
        refused(&head, &triangle()),
        Error::Picking(PickingError::Tables).to_string()
    );
}

#[test]
fn oversized_picking_parts_are_an_error() {
    let parts = triangle();
    let slices = slices(&parts);
    let before: Vec<&dyn Buffer> = slices[..6].iter().map(|p| p as &dyn Buffer).collect();
    let len = RenderMesh::MAX_INDICES / 3 * size_of::<u32>() + 1;
    let huge = Huge(len);
    let parts: Vec<&dyn Buffer> = (before.iter().copied())
        .chain([&huge as &dyn Buffer, &slices[7]])
        .collect();
    assert_eq!(
        decode_all(&parts).map(|_| ()),
        Err(Error::TooLarge {
            part: Part::Picking(PickingPart::Triangles),
            len
        })
    );
    let len = RenderMesh::MAX_EDGES * size_of::<u32>() + 1;
    let huge = Huge(len);
    let parts: Vec<&dyn Buffer> = (before.iter().copied())
        .chain([&slices[6] as &dyn Buffer, &huge])
        .collect();
    assert_eq!(
        decode_all(&parts).map(|_| ()),
        Err(Error::TooLarge {
            part: Part::Picking(PickingPart::Edges),
            len
        })
    );
}

/// A mesh of two triangles side by side, their faces each, and one chain
/// between them along their shared side.
fn two_triangles() -> Vec<Vec<u8>> {
    let mesh = RenderMesh::from_parts(
        vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]],
        vec![[0.0, 0.0, 1.0]; 4],
        vec![0, 1, 2, 1, 3, 2],
        vec![[1, 2]],
    )
    .unwrap();
    let mut parts = triangle();
    parts[0] = bytemuck::cast_slice(mesh.positions()).to_vec();
    parts[1] = bytemuck::cast_slice(mesh.normals()).to_vec();
    parts[2] = bytemuck::cast_slice(mesh.indices()).to_vec();
    parts[3] = bytemuck::cast_slice(mesh.edges()).to_vec();
    parts[6] = bytemuck::cast_slice(&[0u32, 1]).to_vec();
    parts[7] = bytemuck::cast_slice(&[0u32]).to_vec();
    parts
}

#[test]
fn picked_chains_join_two_faces_of_one_body() {
    let other = ids()[0];
    let two = |change: fn(&mut Vec<PickFace>, &mut Vec<PickChain>)| {
        let mut head = tables(|faces, chains| {
            let mut second = face();
            second.key.part = PartKey::EndCap;
            faces.push(second);
            chains.push(PickChain {
                faces: [0, 1],
                closed: false,
            });
            change(faces, chains);
        });
        if let Head::Regenerated { bodies, .. } = &mut head {
            bodies.push((other, [[0.0; 3], [1.0; 3]]));
        }
        head
    };
    // As made, it's taken.
    let reply = decode_reply(&two(|_, _| ()).encode()[..], &slices(&two_triangles())).unwrap();
    let Response::Regenerated { picking, .. } = reply else {
        panic!("a good chain was refused");
    };
    assert_eq!(picking.chain_keys(0)[0].part, PartKey::StartCap);
    let chain = Error::Picking(PickingError::Chain).to_string();
    for change in [
        (|_: &mut Vec<PickFace>, chains: &mut Vec<PickChain>| chains[0].faces = [0, 0])
            as fn(&mut Vec<PickFace>, &mut Vec<PickChain>),
        |_, chains| chains[0].faces = [0, 2],
        |_, chains| chains[0].faces = [u32::MAX, 1],
    ] {
        assert_eq!(refused(&two(change), &two_triangles()), chain);
    }
    // Faces of two bodies.
    let head = {
        let mut head = two(|_, _| ());
        if let Head::Regenerated { faces, .. } = &mut head {
            faces[1].body = other;
        }
        head
    };
    assert_eq!(refused(&head, &two_triangles()), chain);
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
fn picked_faces_must_be_of_listed_bodies() {
    let mut head = regenerated(1);
    if let Head::Regenerated { bodies, .. } = &mut head {
        bodies.clear();
    }
    assert_eq!(
        refused(&head, &triangle()),
        Error::Picking(PickingError::Face).to_string()
    );
}
