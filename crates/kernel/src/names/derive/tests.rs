//! Derived names on polygon bodies: a tetrahedron's edges between its
//! faces' keys and its corners meeting three, the same names whatever
//! order its rows were added in; one key on both sides of an edge; a
//! pole's tag and its one-face edge.

use std::collections::BTreeMap;

use varde_decide::num::storage::OpStart;

use super::*;
use crate::geom::curve::Curve3D;
use crate::geom::form::PlaneKey;
use crate::geom::lattice::Lattice;
use crate::geom::pcurve::{PCurve, Span2};
use crate::geom::surface::Surface;
use crate::infra::ops::{FeatureId, InstanceId, OpCtx, Status, Tolerance};
use crate::names::keys::{FacePart, PoleEnd};
use crate::topo::brep::{BodyKind, EdgeKind, ExactRows};
use crate::topo::build::{Builder, Use};

fn status() -> Status {
    let start = OpStart {
        max_coord: 1.0,
        rho_op: 0.0,
    };
    Status::new(Tolerance::new(1e-3).expect("in range"), &start)
}

fn key(part: u32) -> FaceKey {
    FaceKey::new(
        OpCtx {
            feature: FeatureId(5),
            instance: InstanceId(2),
        },
        FacePart::Primitive(part),
    )
}

fn line(a: [f64; 3], b: [f64; 3]) -> Curve3D {
    let m = [0, 1, 2].map(|k| (a[k] + b[k]) * 0.5);
    let pt = |p: [f64; 3]| [p[0], p[1], p[2], 1.0];
    Curve3D::new(vec![[pt(a), pt(m), pt(b)]], vec![0.0, 1.0]).expect("a line")
}

fn uv_line(a: [f64; 2], b: [f64; 2]) -> PCurve {
    let m = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    let span: Span2 = [[a[0], a[1], 1.0], [m[0], m[1], 1.0], [b[0], b[1], 1.0]];
    PCurve::new(vec![span], vec![0.0, 1.0]).expect("a UV line")
}

fn plane() -> Surface {
    let lat = Lattice::new(PlaneKey::new([0.0, 0.0, 1.0], [0.0; 3]).unwrap().into()).unwrap();
    Surface::new(lat.clone(), lat.half(0).unwrap()).unwrap()
}

const TET: [[f64; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
];

/// Each edge used once each way.
const TET_FACES: [[u32; 3]; 4] = [[0, 2, 1], [0, 1, 3], [1, 2, 3], [0, 3, 2]];

/// The tetrahedron (topology only, every face on one plane), its corners
/// added in the order `corners`, its faces in the order `faces`, face
/// `f` of [`TET_FACES`] keyed `keys[f]`.
fn tet(corners: [usize; 4], faces: [usize; 4], keys: [FaceKey; 4]) -> Brep {
    let mut b = Builder::new(BodyKind::Solid, status());
    let s = b.add_surface(plane(), None, ExactRows::default()).unwrap();
    let mut verts = [VertId::NONE; 4];
    for k in corners {
        verts[k] = b.add_vert(TET[k]).unwrap();
    }
    let mut edges = BTreeMap::new();
    for f in faces {
        let corners = TET_FACES[f];
        let face = b.add_face(s, false, keys[f]).unwrap();
        let uses: Vec<Use> = (0..3)
            .map(|k| {
                let (from, to) = (
                    verts[corners[k] as usize],
                    verts[corners[(k + 1) % 3] as usize],
                );
                let (lo, hi) = (from.min(to), from.max(to));
                let (a, z) = (b.brep().pos(lo), b.brep().pos(hi));
                let e = *edges.entry([lo, hi]).or_insert_with(|| {
                    let c = b.add_curve(line(a, z)).unwrap();
                    b.add_edge(c, [0.0, 1.0], [lo, hi]).unwrap()
                });
                Use {
                    edge: e,
                    reversed: from > to,
                    pcurve: b.add_pcurve(uv_line([a[0], a[1]], [z[0], z[1]])).unwrap(),
                    seam_bound: 0.0,
                }
            })
            .collect();
        b.add_loop(face, true, &uses).unwrap();
    }
    b.finish().unwrap().0
}

/// The body's edge names by their ends' positions (lesser first) and its
/// vertex names by position: what names are, read without ids.
type Named = (
    BTreeMap<[[u64; 3]; 2], EdgeName>,
    BTreeMap<[u64; 3], VertName>,
);

fn named(b: &Brep) -> Named {
    let at = |v: VertId| b.pos(v).map(f64::to_bits);
    let edges = edge_names(b)
        .into_iter()
        .enumerate()
        .map(|(e, n)| {
            let [p, q] = b.ends(EdgeId(e as u32)).map(at);
            ([p.min(q), p.max(q)], n.expect("every edge is used"))
        })
        .collect();
    let verts = vert_names(b)
        .into_iter()
        .enumerate()
        .map(|(v, n)| (at(VertId(v as u32)), n))
        .collect();
    (edges, verts)
}

#[test]
fn a_tetrahedrons_edges_lie_between_two_keys_and_its_corners_meet_three() {
    let keys = [0, 1, 2, 3].map(key);
    let b = tet([0, 1, 2, 3], [0, 1, 2, 3], keys);
    for e in 0..b.edge_count() {
        let e = EdgeId(e as u32);
        let n = edge_name(&b, e).unwrap();
        assert_eq!(n.tag, None);
        let [p, q] = n.between.keys();
        assert!(p < q, "two keys, lesser first");
        // The keys are those of the faces of the edge's two coedges.
        let c = b.edge_coedge(e);
        let faces = [b.face(c), b.face(b.pair(c))].map(|f| *b.key(f));
        assert!(faces.iter().all(|k| n.between.has(k)));
    }
    for (v, n) in vert_names(&b).iter().enumerate() {
        assert_eq!(n.meet.keys().len(), 3);
        assert!(n.meet.keys().is_sorted());
        // A corner of the tetrahedron meets every face but the opposite.
        let opposite = TET_FACES
            .iter()
            .position(|f| !f.contains(&(v as u32)))
            .unwrap();
        assert!(!n.meet.has(&keys[opposite]));
        // The names of the edges ending there read only its keys.
        for e in 0..b.edge_count() {
            let e = EdgeId(e as u32);
            if b.ends(e).contains(&VertId(v as u32)) {
                let between = edge_name(&b, e).unwrap().between;
                assert!(between.keys().iter().all(|k| n.meet.has(k)));
            }
        }
    }
}

#[test]
fn names_do_not_depend_on_the_order_rows_were_added() {
    let keys = [0, 1, 2, 3].map(key);
    let first = named(&tet([0, 1, 2, 3], [0, 1, 2, 3], keys));
    for (corners, faces) in [
        ([3, 2, 1, 0], [3, 2, 1, 0]),
        ([1, 3, 0, 2], [2, 0, 3, 1]),
        ([2, 0, 3, 1], [1, 3, 0, 2]),
    ] {
        let b = tet(corners, faces, keys);
        assert_eq!(named(&b), first, "corners {corners:?}, faces {faces:?}");
    }
}

#[test]
fn faces_of_one_key_name_an_edge_between_it_twice_and_meet_it_once() {
    // Faces 0 and 1 share a key, as the halves of one constructed part.
    let keys = [key(0), key(0), key(2), key(3)];
    let b = tet([0, 1, 2, 3], [0, 1, 2, 3], keys);
    let (edges, verts) = named(&b);
    // Faces 0 and 1 share the edge from corner 0 to corner 1.
    let at = |k: usize| TET[k].map(f64::to_bits);
    let shared = edges[&[at(0), at(1)]];
    assert!(shared.between.is_one_key());
    assert_eq!(shared.between, Between::new(key(0), key(0)));
    // Corner 0 lies on faces 0, 1 and 3: two keys.
    assert_eq!(verts[&at(0)].meet, Meet::new([key(0), key(3)]));
    assert_eq!(verts[&at(0)].meet.keys(), [key(0), key(3)]);
    // Corner 2 on faces 0, 2 and 3: three.
    assert_eq!(verts[&at(2)].meet.keys(), [key(0), key(2), key(3)]);
}

#[test]
fn a_poles_one_face_edge_is_between_its_key_twice_and_the_pole_keeps_its_tag() {
    let mut b = Builder::new(BodyKind::Sheet, status());
    let s = b.add_surface(plane(), None, ExactRows::default()).unwrap();
    let pole = b
        .add_pole([0.0, 0.0, 1.0], VertTag::Pole(PoleEnd::S))
        .unwrap();
    let p = [0.0, 0.0, 1.0];
    let c = b.add_curve(line(p, p)).unwrap();
    let e = b.add_edge(c, [0.0, 1.0], [pole, pole]).unwrap();
    b.set_edge_kind(e, EdgeKind::Singular);
    let lone = b.add_curve(line(p, p)).unwrap();
    let unused = b.add_edge(lone, [0.0, 1.0], [pole, pole]).unwrap();
    let f = b.add_face(s, false, key(7)).unwrap();
    let u = Use {
        edge: e,
        reversed: false,
        pcurve: b.add_pcurve(uv_line([0.0, 1.0], [1.0, 1.0])).unwrap(),
        seam_bound: 0.0,
    };
    b.add_loop(f, true, &[u]).unwrap();
    let b = b.brep();
    let n = edge_name(b, e).unwrap();
    assert_eq!(n.between.keys(), [key(7); 2]);
    assert_eq!(edge_name(b, unused), None);
    assert_eq!(
        vert_names(b),
        [VertName {
            meet: Meet::new([key(7)]),
            tag: Some(VertTag::Pole(PoleEnd::S)),
        }]
    );
}

#[test]
fn an_edges_end_on_a_face_meets_the_three_keys() {
    let [a, b, c] = [key(1), key(2), key(3)];
    assert_eq!(Meet::edge_on_face(Between::new(b, a), c).keys(), [a, b, c]);
    // A split old edge's end on one of its own faces meets two.
    assert_eq!(Meet::edge_on_face(Between::new(a, b), a).keys(), [a, b]);
    // Names order by their keys: an edge's by its lesser key first.
    let mut names = [
        Name::Edge(EdgeName {
            between: Between::new(c, a),
            tag: None,
        }),
        Name::Face(b),
        Name::Edge(EdgeName {
            between: Between::new(b, a),
            tag: Some(EdgeTag::Seam),
        }),
    ];
    names.sort();
    assert_eq!(names[0], Name::Face(b));
    assert_eq!(
        names[1],
        Name::Edge(EdgeName {
            between: Between::new(a, b),
            tag: Some(EdgeTag::Seam),
        })
    );
}
