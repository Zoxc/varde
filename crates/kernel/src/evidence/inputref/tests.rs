//! Input entities named on the cube: by the same names the body derives,
//! with their points; through the leaves a record key's operands are;
//! and guarded on links naming nothing.

use varde_decide::records::keys::{Point, RayKey, RecordKey};

use super::*;
use crate::evidence::tests::{CUBE, cube, key};
use crate::names::{edge_name as derived_edge_name, vert_names};

#[test]
fn entities_are_named_as_the_body_names_them() {
    let b = cube();
    let leaf = Leaf::new(3, &b);
    for f in 0..b.face_count() as u32 {
        let r = InputRef::of(&leaf, Entity::Face(FaceId(f)));
        assert_eq!(r.name, Some(Name::Face(key(f))));
        assert_eq!((r.leaf, r.instance), (3, InstanceId::ORIGINAL));
        assert!(r.point.is_some());
    }
    let names = vert_names(&b);
    for v in 0..b.vert_count() as u32 {
        let r = InputRef::of(&leaf, Entity::Vert(VertId(v)));
        assert_eq!(r.name, Some(Name::Vert(names[v as usize].clone())));
        assert_eq!(r.point, Some(CUBE[v as usize]));
        // Three faces meet at each corner.
        assert_eq!(names[v as usize].meet.keys().len(), 3);
    }
    for e in 0..b.edge_count() as u32 {
        let e = EdgeId(e);
        let r = InputRef::of(&leaf, Entity::Edge(e));
        assert_eq!(r.name, derived_edge_name(&b, e).map(Name::Edge));
        let [s, t] = b.ends(e).map(|v| b.pos(v));
        let mid = [0, 1, 2].map(|k| (s[k] + t[k]) * 0.5);
        assert_eq!(r.point, Some(mid));
    }
    // A coedge is named by its edge, a loop by its face.
    let c = CoedgeId(5);
    let r = InputRef::of(&leaf, Entity::Coedge(c));
    assert_eq!(r.name, derived_edge_name(&b, b.edge(c)).map(Name::Edge));
    let l = b.first_loop(FaceId(2));
    let r = InputRef::of(&leaf, Entity::Loop(l));
    assert_eq!(r.name, Some(Name::Face(key(2))));
    assert_eq!(
        r.point,
        Some(b.pos(b.coedge_vert(b.first_coedge(l), false)))
    );
    // Geometry has no name.
    let r = InputRef::of(&leaf, Entity::Surf(b.face_surface_id(FaceId(0))));
    assert_eq!(r.name, None);
}

#[test]
fn ids_naming_nothing_are_named_as_far_as_they_go() {
    let b = cube();
    let leaf = Leaf::new(0, &b);
    for id in [
        Entity::Face(FaceId(99)),
        Entity::Vert(VertId(99)),
        Entity::Edge(EdgeId::NONE),
        Entity::Coedge(CoedgeId(1 << 20)),
    ] {
        let r = InputRef::of(&leaf, id);
        assert_eq!((r.name, r.point, r.id), (None, None, id));
    }
    // A leaf not among the operation's: the id alone.
    let r = InputRef::among(&[leaf], 1, Entity::Face(FaceId(0)));
    assert_eq!((r.leaf, r.name, r.point), (1, None, None));
}

#[test]
fn a_record_keys_entities_are_named_through_the_leaves() {
    let (a, b) = (cube(), cube());
    let leaves = [Leaf::new(0, &a), Leaf::new(1, &b)];
    let ray = RayKey {
        q: Point::vert(Operand::B, VertId(7)),
        g: EdgeId(2),
    };
    let name = ItemName {
        record: Some(RecordKey::Ray(ray)),
        world: None,
    };
    let refs = name.inputs(&leaves);
    let named = name.entities();
    assert_eq!(refs.len(), named.len());
    for (r, n) in refs.iter().zip(&named) {
        assert_eq!(r.leaf, Leaf::of_operand(n.operand));
        assert_eq!(r.id, n.entity);
        assert!(r.name.is_some());
    }
    assert!(ItemName::default().inputs(&leaves).is_empty());
}

/// A sub-edge names its parent edge with its parameter range (sorted)
/// and the curve's point at the range's middle; a face's window names
/// the face with its UV box (sorted) and the surface's point at its
/// centre.
#[test]
fn parts_name_their_parents_with_windows() {
    let b = cube();
    let leaf = Leaf::new(1, &b);
    let e = EdgeId(2);
    let [t0, t1] = b.t_range(e);
    let mid = t0 * 0.5 + t1 * 0.5;
    let r = InputRef::sub_edge(&leaf, e, [mid, t0]);
    assert_eq!(r.name, InputRef::of(&leaf, Entity::Edge(e)).name);
    assert_eq!(r.window, Some(Window::Edge { t: [t0, mid] }));
    assert_eq!(r.point, b.edge_curve(e).point_at(t0 * 0.5 + mid * 0.5));
    let f = FaceId(3);
    let s = b.face_surface(f);
    let rect = s.rect();
    let (u0, v0) = (rect.u.start as f64, rect.v.start as f64);
    let uv = [[u0 + 0.75, v0 + 0.5], [u0 + 0.25, v0 + 0.0]];
    let r = InputRef::face_window(&leaf, f, uv);
    assert_eq!(r.name, Some(Name::Face(key(3))));
    let (lo, hi) = ([u0 + 0.25, v0], [u0 + 0.75, v0 + 0.5]);
    assert_eq!(r.window, Some(Window::Face { uv: [lo, hi] }));
    let centre = s.eval([u0 + 0.5, v0 + 0.25]).map(|p| p.map(|x| x.v));
    assert_eq!(r.point, centre);
}

/// An edge pair names both edges through their operands' leaves with
/// their boxes; an operand without a leaf keeps its box, unnamed.
#[test]
fn an_edge_pair_names_both_edges() {
    let b = cube();
    let leaves = [Leaf::new(0, &b), Leaf::new(1, &b)];
    let [x, y] = edge_pair(
        &leaves,
        [
            (Operand::A, EdgeId(1), [0.0, 0.5]),
            (Operand::B, EdgeId(4), [0.25, 0.75]),
        ],
    );
    assert_eq!((x.leaf, x.id), (0, Entity::Edge(EdgeId(1))));
    assert_eq!((y.leaf, y.id), (1, Entity::Edge(EdgeId(4))));
    assert_eq!(y.window, Some(Window::Edge { t: [0.25, 0.75] }));
    let [_, y] = edge_pair(
        &leaves[..1],
        [
            (Operand::A, EdgeId(1), [0.0, 0.5]),
            (Operand::B, EdgeId(4), [0.25, 0.75]),
        ],
    );
    assert_eq!((y.name, y.point), (None, None));
    assert_eq!(y.window, Some(Window::Edge { t: [0.25, 0.75] }));
}

/// A snap's entities map to their operand parents with the part they
/// come from: a parameter or range of an edge, a point, segment or region
/// of a face's UV as its box; a vertex to its parent whole.
#[test]
fn snapped_entities_map_through_their_pre_images() {
    let b = cube();
    let leaves = [Leaf::new(0, &b), Leaf::new(1, &b)];
    let q = |x: f64| Rational::try_from(x).unwrap();
    let pre = |parent, at| Pre { parent, at };
    let e = EdgeId(0);
    let r = through_snap(
        &leaves,
        &pre((Operand::B, Entity::Edge(e)), PreImage::Param(q(0.5))),
    );
    assert_eq!(
        (r.leaf, r.window),
        (1, Some(Window::Edge { t: [0.5, 0.5] }))
    );
    let r = through_snap(
        &leaves,
        &pre(
            (Operand::A, Entity::Edge(e)),
            PreImage::Range([q(0.75), q(0.25)]),
        ),
    );
    assert_eq!(r.window, Some(Window::Edge { t: [0.25, 0.75] }));
    let f = FaceId(1);
    let rect = b.face_surface(f).rect();
    let (u, v) = (rect.u.start as f64, rect.v.start as f64);
    let p = |a: f64, c: f64| [q(u + a), q(v + c)];
    let r = through_snap(
        &leaves,
        &pre((Operand::A, Entity::Face(f)), PreImage::Point(p(0.5, 0.5))),
    );
    let at = [u + 0.5, v + 0.5];
    assert_eq!(r.window, Some(Window::Face { uv: [at, at] }));
    let r = through_snap(
        &leaves,
        &pre(
            (Operand::A, Entity::Face(f)),
            PreImage::Segment([p(0.75, 0.25), p(0.25, 0.5)]),
        ),
    );
    assert_eq!(
        r.window,
        Some(Window::Face {
            uv: [[u + 0.25, v + 0.25], [u + 0.75, v + 0.5]]
        })
    );
    let region = PreImage::Region(vec![
        vec![p(0.0, 0.0), p(0.5, 0.0), p(0.5, 1.0)],
        vec![p(0.25, 0.75)],
    ]);
    let r = through_snap(&leaves, &pre((Operand::A, Entity::Face(f)), region));
    assert_eq!(
        r.window,
        Some(Window::Face {
            uv: [[u, v], [u + 0.5, v + 1.0]]
        })
    );
    let r = through_snap(
        &leaves,
        &pre((Operand::B, Entity::Vert(VertId(3))), PreImage::Whole),
    );
    assert_eq!(r, InputRef::of(&leaves[1], Entity::Vert(VertId(3))));
    let mut map = SnapMap::default();
    map.faces.insert(
        crate::boolean::snap::rewrite::SnapFace::Kept { face: f, part: 0 },
        pre((Operand::A, Entity::Face(f)), PreImage::Point(p(0.5, 0.5))),
    );
    assert_eq!(snap_inputs(&leaves, &map).len(), 1);
}
