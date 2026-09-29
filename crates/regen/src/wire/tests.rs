use std::sync::Arc;

use glam::Vec3;
use varde_document::{Command, Document, Editor};
use varde_kernel::{MeshPart, Shape};

use super::*;
use crate::handle;
use crate::tests::sketched;

fn two_cubes() -> Editor {
    let mut editor = Editor::new(Document::example());
    editor
        .apply(Command::AddBody {
            name: "Cube 2".to_owned(),
            shape: Shape::cuboid(Vec3::splat(1.0)),
            position: Vec3::X * 3.0,
        })
        .unwrap();
    editor
}

fn regenerate(editor: &Editor) -> Request {
    Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude: None,
    }
}

/// The head of a regenerated `generation`, whose model is in its parts.
fn regenerated(generation: u64) -> Head {
    Head::Regenerated {
        generation: generation.into(),
        exclude: None,
        unsolved: Vec::new(),
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
    let editor = two_cubes();
    let bytes = encode_request(&regenerate(&editor));
    let Request::Regenerate {
        generation,
        document,
        exclude,
    } = decode_request(&bytes).unwrap();
    assert_eq!(u64::from(generation), 1);
    assert_eq!(*document, *editor.document());
    assert_eq!(exclude, None);
}

#[test]
fn request_leaving_out_a_sketch_round_trips() {
    let (editor, feature) = sketched();
    let request = Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude: Some(feature),
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
fn regenerated_round_trips() {
    let (mut editor, _) = sketched();
    editor
        .apply(Command::AddBody {
            name: "Cube 2".to_owned(),
            shape: Shape::cuboid(Vec3::splat(1.0)),
            position: Vec3::X * 3.0,
        })
        .unwrap();
    let unsolved = crate::tests::unsolvable(&mut editor);
    let bytes = encode_request(&regenerate(&editor));
    let response = handle(decode_request(&bytes).unwrap());
    let Response::Regenerated {
        generation,
        exclude,
        mesh,
        sketches,
        unsolved: marked,
    } = round_trip(&response)
    else {
        panic!("regeneration failed");
    };
    assert_eq!(generation, editor.generation());
    assert_eq!(exclude, None);
    assert_eq!(marked, [unsolved]);
    assert_eq!(*mesh, crate::tessellate(editor.document()).unwrap());
    assert_eq!(mesh.triangle_count(), 24);
    assert_eq!(
        *sketches,
        crate::flatten_sketches(editor.document(), None).unwrap()
    );
    assert_eq!(sketches.ends().len(), 3);
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
        error: "the kernel gave up".to_owned(),
    };
    assert!(encode_reply(&response).1.is_none());
    let Response::Failed {
        generation,
        exclude,
        error,
    } = round_trip(&response)
    else {
        panic!("not a failure");
    };
    assert_eq!(u64::from(generation), u64::MAX);
    assert_eq!(exclude, Some(feature));
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
    assert_eq!(error, Error::Parts(5).to_string());
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
    let request = encode_request(&regenerate(&two_cubes()));
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
    let mut request = encode_request(&regenerate(&two_cubes()));
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
    let response = Response::Regenerated {
        generation: Generation::from(0),
        exclude: None,
        mesh: Arc::new(mesh),
        sketches: Arc::new(lines),
        unsolved: Vec::new(),
    };
    let (_, mesh) = encode_reply(&response);
    mesh.unwrap().map(<[u8]>::to_vec).to_vec()
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
    assert_eq!(decode(&parts), Err(Error::Parts(5)));
    assert_eq!(decode(&[]), Err(Error::Parts(0)));
    parts.extend([Vec::new(), Vec::new()]);
    assert_eq!(decode(&parts), Err(Error::Parts(7)));
}

#[test]
fn partial_elements_are_an_error() {
    for (part, name) in [
        (0, Part::RenderMesh(MeshPart::Positions)),
        (2, Part::RenderMesh(MeshPart::Indices)),
        (3, Part::RenderMesh(MeshPart::Edges)),
        (4, Part::RenderLines(LinesPart::Points)),
        (5, Part::RenderLines(LinesPart::Ends)),
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
    let huge = Huge(len);
    let parts: [&dyn Buffer; 6] = [&huge, &huge, &indices, &edges, &points, &ends];
    assert_eq!(
        decode_model(&parts).map(|_| ()),
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
    let len = RenderLines::MAX_POINTS * size_of::<[f32; 3]>() + 1;
    let huge = Huge(len);
    let parts: Vec<&dyn Buffer> = mesh
        .iter()
        .copied()
        .chain([&huge as &dyn Buffer, &ends])
        .collect();
    assert_eq!(
        decode_model(&parts).map(|_| ()),
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
        .collect();
    assert_eq!(
        decode_model(&parts).map(|_| ()),
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
    assert_eq!(error.to_string(), "model in 0 parts instead of 6");
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
    let request = encode_request(&regenerate(&two_cubes()));
    let (head, _) = encode_reply(&handle(decode_request(&request).unwrap()));
    let parts = triangle();
    let failed = Head::Failed {
        generation: Generation::from(3),
        exclude: None,
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
        error: String::new(),
    }
    .encode();
    head.pop();
    head.extend(huge);
    assert!(matches!(Head::decode(&head), Err(Error::Head(_))));
}
