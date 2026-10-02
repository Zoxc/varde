use glam::{DVec2, DVec3};
use varde_document::{
    BodyId, Command, Document, Editor, Extent, Extrude, Operation, OriginPlane, Plane,
};
use varde_expr::Value;
use varde_kernel::mesh::PartKey;
use varde_kernel::{MeshParts, RenderMesh};
use varde_regen::{Cache, PickFace, Picking, Summary, evaluate, tessellate_picking};
use varde_render::{Projection, View};
use varde_sketch::{Curve, Sketch};

use super::*;
use crate::pick::tests::{BOTTOM, FRONT, SIZE, camera, plane, plate, shown};

/// `document` regenerated and made ready for picking as model `model`.
fn index_of(document: &Document, model: u64) -> PickIndex {
    let mut cache = Cache::default();
    let evaluation = evaluate(document, &mut cache);
    let (mesh, picking) = tessellate_picking(document, &evaluation, &mut cache).unwrap();
    PickIndex::new(mesh, picking, model)
}

/// A slot 10 mm up from XY: straight sides from -10 to 10 along x at y
/// ±4, rounded at the ends with radius 4. Its top's rim runs on smoothly
/// all round.
fn slot() -> PickIndex {
    let mut editor = Editor::new(Document::default());
    let plane = Plane::Origin(OriginPlane::XY);
    editor.apply(editor.document().add_sketch(plane)).unwrap();
    let feature = editor.document().features()[0].id;
    let mut sketch = Sketch::default();
    let mut point = |x: f64, y: f64| sketch.add_point(DVec2::new(x, y)).unwrap();
    let [a, b, c, d] =
        [(-10.0, -4.0), (10.0, -4.0), (10.0, 4.0), (-10.0, 4.0)].map(|(x, y)| point(x, y));
    let [e, f] = [(10.0, 0.0), (-10.0, 0.0)].map(|(x, y)| point(x, y));
    for curve in [
        Curve::Line { start: a, end: b },
        Curve::Arc {
            center: e,
            start: b,
            end: c,
        },
        Curve::Line { start: c, end: d },
        Curve::Arc {
            center: f,
            start: d,
            end: a,
        },
    ] {
        sketch.add_curve(curve, false).unwrap();
    }
    let profiles = sketch.profiles().unwrap();
    assert_eq!(profiles.regions.len(), 1);
    let region = profiles.reference(0).unwrap();
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    let design = editor.document().design();
    let distance = Value::new("10", &Extent::ask(&design)).unwrap();
    let extrude = Extrude {
        sketch: feature,
        regions: vec![region],
        extent: Extent::OneSide(distance),
        flip: false,
        operation: Operation::NewBody(BodyId::NEW),
    };
    editor
        .apply(editor.document().add_feature(extrude.into()))
        .unwrap();
    index_of(editor.document(), 3)
}

/// What the cursor at the world point `at`, seen from the top, picks of
/// `index`, as `picks` take.
fn from_top(index: &PickIndex, at: DVec3, picks: Picks) -> Pick {
    let top = camera(View::Top, Projection::Orthographic);
    index.pick(&top, SIZE, shown(&top, at), picks).unwrap()
}

/// The plate's top, picked at its middle-ish.
fn top_of(index: &PickIndex) -> Pick {
    from_top(index, DVec3::new(20.0, 5.0, 10.0), Picks::All)
}

/// The plate's top front edge.
fn front_edge(index: &PickIndex) -> Pick {
    let pick = from_top(index, DVec3::new(10.0, -20.0, 10.0), Picks::Edges);
    assert!(matches!(pick.target, Picked::Edge(_)), "{pick:?}");
    pick
}

/// The plate's bottom, picked from below.
fn bottom_of(index: &PickIndex) -> Pick {
    let below = camera(View::Bottom, Projection::Orthographic);
    let at = shown(&below, DVec3::new(20.0, 5.0, 0.0));
    let pick = index.pick(&below, SIZE, at, Picks::Faces).unwrap();
    assert_eq!(plane(index, pick.target), BOTTOM);
    pick
}

fn targets(selection: &Selection) -> Vec<Picked> {
    selection.targets().collect()
}

#[test]
fn a_click_selects_and_with_add_toggles() {
    let index = plate();
    let (top, bottom) = (top_of(&index), bottom_of(&index));
    let mut selection = Selection::default();
    assert!(selection.click(&index, Some(top), false, false));
    assert_eq!(targets(&selection), [top.target]);
    let [Selected::Face { body, key, near }] = selection.items().copied().collect::<Vec<_>>()[..]
    else {
        panic!("{selection:?}");
    };
    assert_eq!((body, near), (top.body, top.at));
    let Picked::Face(face) = top.target else {
        unreachable!()
    };
    assert_eq!(key, index.picking().faces()[face as usize].key);
    // Again: nothing changes.
    assert!(!selection.click(&index, Some(top), false, false));
    // Another alone replaces it.
    assert!(selection.click(&index, Some(bottom), false, false));
    assert_eq!(targets(&selection), [bottom.target]);
    // With add, it's added, then taken out.
    assert!(selection.click(&index, Some(top), true, false));
    assert_eq!(targets(&selection), [bottom.target, top.target]);
    let edge = front_edge(&index);
    assert!(selection.click(&index, Some(edge), true, false));
    assert_eq!(
        targets(&selection),
        [bottom.target, top.target, edge.target]
    );
    assert!(selection.click(&index, Some(top), true, false));
    assert_eq!(targets(&selection), [bottom.target, edge.target]);
    // Nothing clicked with add keeps it; alone, clears it.
    assert!(!selection.click(&index, None, true, false));
    assert_eq!(targets(&selection).len(), 2);
    assert!(selection.click(&index, None, false, false));
    assert!(selection.is_empty());
    assert!(!selection.click(&index, None, false, false));
    // A pick of another model is ignored.
    let other = Pick { model: 8, ..top };
    assert!(!selection.click(&index, Some(other), false, false));
    assert!(selection.is_empty());
}

#[test]
fn a_double_click_selects_the_body() {
    let index = plate();
    let top = top_of(&index);
    let mut selection = Selection::default();
    selection.click(&index, Some(top), false, false);
    selection.click(&index, Some(top), false, true);
    assert_eq!(
        selection.items().copied().collect::<Vec<_>>(),
        [Selected::Body(top.body)]
    );
    assert_eq!(selection.bodies().collect::<Vec<_>>(), [top.body]);
    assert_eq!(targets(&selection), []);
    // With add, the first click's face goes again as the body comes, and
    // the body goes as it's double-clicked again.
    let edge = front_edge(&index);
    let mut selection = Selection::default();
    selection.click(&index, Some(edge), false, false);
    selection.click(&index, Some(top), true, false);
    selection.click(&index, Some(top), true, true);
    assert_eq!(targets(&selection), [edge.target]);
    assert_eq!(selection.bodies().collect::<Vec<_>>(), [top.body]);
    selection.click(&index, Some(top), true, false);
    selection.click(&index, Some(top), true, true);
    assert_eq!(targets(&selection), [edge.target]);
    assert_eq!(selection.bodies().count(), 0);
    // A body's faces show selected.
    let mut selection = Selection::default();
    selection.click_body(top.body, false);
    let faces: Vec<_> = index.body_faces(top.body).collect();
    assert!(faces.len() >= 7, "{faces:?}");
    let all: Vec<Picked> = faces.iter().map(|&f| Picked::Face(f)).collect();
    assert_eq!(
        selection.highlight(&index, None),
        index.highlight(&[], &all)
    );
}

#[test]
fn modes_select_faces_edges_or_bodies() {
    let index = plate();
    let (top, edge) = (top_of(&index), front_edge(&index));
    // Faces only: an edge clicked selects nothing, and doesn't clear.
    let mut faces = Selection::new(SelectionMode::Faces);
    assert_eq!(faces.mode().picks(), Picks::Faces);
    faces.click(&index, Some(top), false, false);
    assert!(!faces.click(&index, Some(edge), false, false));
    assert_eq!(targets(&faces), [top.target]);
    // A double-click is a click there.
    faces.click(&index, Some(top), false, true);
    assert_eq!(targets(&faces), [top.target]);
    assert!(!faces.click_body(top.body, false));
    // Edges only.
    let mut edges = Selection::new(SelectionMode::Edges { tangent: false });
    assert_eq!(edges.mode().picks(), Picks::Edges);
    assert!(!edges.click(&index, Some(top), false, false));
    edges.click(&index, Some(edge), false, false);
    assert_eq!(targets(&edges), [edge.target]);
    assert_eq!(edges.hovered(&index, top), []);
    // Bodies: whatever's clicked gives its body, and its faces hover.
    let mut bodies = Selection::new(SelectionMode::Bodies);
    bodies.click(&index, Some(edge), false, false);
    assert_eq!(bodies.bodies().collect::<Vec<_>>(), [edge.body]);
    let faces: Vec<_> = index.body_faces(edge.body).map(Picked::Face).collect();
    assert_eq!(bodies.hovered(&index, top), faces);
    // Toggled out from Objects.
    assert!(bodies.click_body(edge.body, true));
    assert!(bodies.is_empty());
}

#[test]
fn a_tangent_chain_is_selected_and_toggled_as_one() {
    let index = slot();
    let rim = from_top(&index, DVec3::new(0.0, -4.0, 10.0), Picks::Edges);
    let Picked::Edge(chain) = rim.target else {
        panic!("{rim:?}");
    };
    // The top's rim: two straight edges and two round ones, one tangent
    // chain; the upright edges where the walls meet aren't in it.
    let members = index.tangent_chain(chain);
    assert_eq!(members.len(), 4, "{members:?}");
    assert!(members.contains(&chain));
    let top_key = |c: u32| index.chain_keys(c).unwrap();
    let cap = |c: u32| {
        let keys = top_key(c);
        keys.iter().any(|key| key.part == PartKey::EndCap)
    };
    assert!(members.iter().all(|&c| cap(c)));
    let chains = (0..index.mesh().edge_count() as u32).filter(|&c| index.edge_faces(c).is_some());
    for c in chains {
        assert_eq!(members.contains(&c), cap(c), "{c}");
    }
    let mut selection = Selection::new(SelectionMode::Edges { tangent: true });
    let hovered = selection.hovered(&index, rim);
    assert_eq!(
        hovered,
        members.iter().map(|&c| Picked::Edge(c)).collect::<Vec<_>>()
    );
    selection.click(&index, Some(rim), false, false);
    let mut selected = targets(&selection);
    selected.sort();
    assert_eq!(selected, hovered);
    // Each by its own keys, at a point on it.
    for item in selection.items() {
        let Selected::Edge { faces, near, .. } = *item else {
            panic!("{item:?}");
        };
        let found = index.find_edge(rim.body, faces, near).unwrap();
        assert!(members.contains(&found));
    }
    // Clicked again with add, all of it goes.
    selection.click(&index, Some(rim), true, false);
    assert!(selection.is_empty());
    // One of it selected, a click with add takes the rest in.
    let mut single = Selection::new(SelectionMode::Edges { tangent: false });
    single.click(&index, Some(rim), false, false);
    assert_eq!(targets(&single), [rim.target]);
    // Without tangent chains, an edge alone.
    assert_eq!(single.hovered(&index, rim), [rim.target]);
}

/// The example's plate made `thickness` mm thick, made ready for picking
/// as model `model`.
fn plate_thick(thickness: &str, model: u64) -> PickIndex {
    let mut editor = Editor::new(Document::example());
    let feature = editor.document().features()[1].id;
    let Some(varde_document::FeatureKind::Extrude(extrude)) =
        editor.document().feature(feature).map(|f| f.kind.clone())
    else {
        panic!("the example's second feature is its extrude");
    };
    let design = editor.document().design();
    let distance = Value::new(thickness, &Extent::ask(&design)).unwrap();
    let extrude = Extrude {
        extent: Extent::OneSide(distance),
        ..extrude
    };
    editor
        .apply(Command::SetFeature {
            feature,
            kind: Box::new(extrude.into()),
        })
        .unwrap();
    index_of(editor.document(), model)
}

#[test]
fn a_vertex_is_selected_by_the_faces_meeting_there_and_found_again() {
    let index = plate();
    let corner = DVec3::new(-30.0, -20.0, 10.0);
    let pick = from_top(&index, corner + DVec3::new(0.6, 0.6, 0.0), Picks::All);
    let Picked::Vertex(vertex) = pick.target else {
        panic!("{pick:?}");
    };
    let mut selection = Selection::default();
    assert!(selection.click(&index, Some(pick), false, false));
    let [Selected::Vertex { body, faces, near }] =
        selection.items().copied().collect::<Vec<_>>()[..]
    else {
        panic!("{selection:?}");
    };
    assert_eq!((body, near), (pick.body, pick.at));
    assert_eq!(Some(faces), index.vertex_keys(vertex));
    assert_eq!(selection.hovered(&index, pick), [pick.target]);
    // Thicker, the corner on top is found again, where it is now.
    let thicker = plate_thick("12", 8);
    assert!(selection.resolve(&thicker, Some));
    let [Picked::Vertex(found)] = targets(&selection)[..] else {
        panic!("{selection:?}");
    };
    let at = thicker.corner_point(found).unwrap();
    assert!(at.distance(DVec3::new(-30.0, -20.0, 12.0)) < 1e-4, "{at}");
    // Sessions taking faces, edges or bodies take no vertex.
    for mode in [
        SelectionMode::Faces,
        SelectionMode::Edges { tangent: false },
    ] {
        let mut selection = Selection::new(mode);
        assert!(!selection.click(&index, Some(pick), false, false));
        assert!(selection.hovered(&index, pick).is_empty());
    }
    let mut bodies = Selection::new(SelectionMode::Bodies);
    bodies.click(&index, Some(pick), false, false);
    assert_eq!(bodies.bodies().collect::<Vec<_>>(), [pick.body]);
}

#[test]
fn selection_is_found_again_in_a_new_model_or_dropped() {
    let index = plate();
    let (top, edge) = (top_of(&index), front_edge(&index));
    let mut selection = Selection::default();
    selection.click(&index, Some(top), false, false);
    selection.click(&index, Some(edge), true, false);
    selection.click_body(top.body, true);
    assert_eq!(selection.model(), Some(7));
    // The plate made 12 mm thick: another mesh, the same faces by name.
    let coarse = plate_thick("12", 8);
    assert_ne!(coarse.mesh().positions(), index.mesh().positions());
    assert!(selection.resolve(&coarse, Some));
    assert_eq!(selection.model(), Some(8));
    let found = targets(&selection);
    assert_eq!(found.len(), 2);
    const THICKER: ([f64; 3], f64) = ([0.0, 0.0, 1.0], 12.0);
    assert_eq!(plane(&coarse, found[0]), THICKER);
    let Picked::Edge(chain) = found[1] else {
        panic!("{found:?}");
    };
    let [a, b] = coarse.edge_faces(chain).unwrap();
    let mut sides = [a, b].map(|f| plane(&coarse, Picked::Face(f)));
    sides.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(sides, [FRONT, THICKER]);
    assert_eq!(selection.items().count(), 3);
    // Found again in the same model, nothing changes.
    assert!(!selection.resolve(&coarse, Some));
    // A body the document doesn't hold any more goes.
    let gone = |body| (body != top.body).then_some(body);
    assert!(selection.resolve(&coarse, gone));
    assert_eq!(selection.bodies().count(), 0);
    // A model without the body: its faces and edges go.
    let empty = PickIndex::new(Default::default(), Default::default(), 9);
    assert!(selection.resolve(&empty, gone));
    assert!(selection.is_empty());
    // But they're looked for again, and found where they are, as after
    // an undo, until the selection changes: the body too, held again.
    assert!(!selection.holds_nothing());
    assert!(selection.resolve(&coarse, Some));
    assert_eq!(targets(&selection), found);
    assert_eq!(selection.bodies().collect::<Vec<_>>(), [top.body]);
    assert!(selection.resolve(&empty, Some));
    let pick = top_of(&index);
    selection.click(&index, Some(pick), false, false);
    selection.click(&index, None, false, false);
    assert!(selection.holds_nothing());
    selection.resolve(&coarse, Some);
    assert!(selection.is_empty());
}

/// Three triangles far apart: face 0 named `a`, face 1 named `b` with `a`
/// merged into it, and face 2 of `b` too, of one body; an edge between
/// faces 0 and 1 along the first's side, one between faces 1 and 2 along
/// the second's, and a crease of face 2 along the third's.
fn aliased() -> PickIndex {
    let positions = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [10.0, 0.0, 0.0],
        [11.0, 0.0, 0.0],
        [10.0, 1.0, 0.0],
        [20.0, 0.0, 0.0],
        [21.0, 0.0, 0.0],
        [20.0, 1.0, 0.0],
    ];
    let corners = [0, 1, 3, 4, 6, 7].map(|v| positions[v]).to_vec();
    let mesh = RenderMesh::from_parts(MeshParts {
        positions,
        normals: vec![[0.0, 0.0, 1.0]; 9],
        indices: vec![0, 1, 2, 3, 4, 5, 6, 7, 8],
        face_ends: vec![3, 6, 9],
        edge_vertices: vec![0, 1, 3, 4, 6, 7],
        edge_ends: vec![2, 4, 6],
        edge_faces: vec![[0, 1], [1, 2], [2, 2]],
        corners,
        edge_corners: vec![[0, 1], [2, 3], [4, 5]],
        part_ends: vec![[3, 3, 6]],
    })
    .unwrap();
    let face = |part: PartKey, aliases: Vec<FaceKey>| PickFace {
        key: FaceKey {
            feature: 1,
            part,
            instance: 0,
        },
        aliases,
        summary: Summary::Plane {
            n: [0.0, 0.0, 1.0],
            d: 0.0,
        },
    };
    let a = face(PartKey::StartCap, Vec::new());
    let b = face(PartKey::EndCap, vec![a.key]);
    let b2 = face(PartKey::EndCap, Vec::new());
    let picking = Picking::from_parts(
        vec![BodyId::NEW],
        vec![a, b, b2],
        vec![false; 3],
        vec![0, 1, 2],
        vec![None; 3],
        Vec::new(),
        &mesh,
    )
    .unwrap();
    PickIndex::new(std::sync::Arc::new(mesh), std::sync::Arc::new(picking), 1)
}

#[test]
fn names_find_faces_and_edges_by_alias_and_the_nearest() {
    let index = aliased();
    let body = BodyId::NEW;
    let key = |f: usize| index.picking().faces()[f].key;
    let at = |x: f64| DVec3::new(x, 0.2, 0.0);
    // `a` names faces 0 and 1 (by alias): the nearest.
    assert_eq!(index.find_face(body, &key(0), at(0.2)), Some(0));
    assert_eq!(index.find_face(body, &key(0), at(10.2)), Some(1));
    assert_eq!(index.find_face(body, &key(0), at(30.0)), Some(1));
    // `b` names faces 1 and 2.
    assert_eq!(index.find_face(body, &key(1), at(0.0)), Some(1));
    assert_eq!(index.find_face(body, &key(1), at(20.5)), Some(2));
    // No point to go by: the lowest.
    assert_eq!(index.find_face(body, &key(1), DVec3::NAN), Some(1));
    // Of another body, or another name: none.
    let other = FaceKey {
        feature: 2,
        ..key(0)
    };
    assert_eq!(index.find_face(body, &other, at(0.0)), None);
    // Edges: between `a` and `b` either way round, and by alias.
    let [a, b] = [key(0), key(1)];
    assert_eq!(index.find_edge(body, [b, a], at(0.0)), Some(0));
    // `a`–`b` names edge 1 too, faces 1 (by alias) and 2.
    assert_eq!(index.find_edge(body, [a, b], at(10.5)), Some(1));
    assert_eq!(
        index.find_edge(body, [a, b], DVec3::new(20.5, 0.0, 0.0)),
        Some(1)
    );
    assert_eq!(index.find_edge(body, [b, b], at(0.0)), Some(1));
    assert_eq!(index.find_edge(body, [a, other], at(0.0)), None);
    // Each edge its own tangent chain, and a crease in none.
    assert_eq!(index.tangent_chain(0), [0]);
    assert_eq!(index.tangent_chain(1), [1]);
    assert_eq!(index.tangent_chain(2), []);
}

#[test]
fn a_hover_is_drawn_with_what_s_selected() {
    let index = plate();
    let (top, edge) = (top_of(&index), front_edge(&index));
    let mut selection = Selection::default();
    selection.click(&index, Some(top), false, false);
    let selected = [top.target];
    assert_eq!(
        selection.highlight(&index, Some(top)),
        index.highlight(&[top.target], &selected)
    );
    assert_eq!(
        selection.highlight(&index, Some(edge)),
        index.highlight(&[edge.target], &selected)
    );
    // A hover of another model shows nothing.
    let other = Pick { model: 8, ..edge };
    assert_eq!(
        selection.highlight(&index, Some(other)),
        index.highlight(&[], &selected)
    );
    // Nor what was found in another model.
    let coarse = index_of(&Document::example(), 8);
    assert!(selection.highlight(&coarse, None).is_empty());
}
