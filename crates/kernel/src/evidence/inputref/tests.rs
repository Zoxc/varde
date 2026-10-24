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
