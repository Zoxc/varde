use std::f64::consts::PI;

use varde_document::{Command, Document, Editor};

use super::*;
use crate::history::tests::{length, plate_below, set_extrude};
use crate::{Regenerator, Request, Response};

/// The example plate: 60 × 40 × 10 from (−30, −20, 0), with a hole of
/// radius 8 through it around the z axis.
const TOP_AREA: f64 = 60.0 * 40.0 - PI * 64.0;
const VOLUME: f64 = TOP_AREA * 10.0;

fn assert_near(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-9 * b.abs().max(1.0), "{a} vs {b}");
}

fn assert_point(a: [f64; 3], b: [f64; 3]) {
    for i in 0..3 {
        assert_near(a[i], b[i]);
    }
}

/// A model's mesh and picking tables, the tables what it derefs to.
#[derive(Debug, PartialEq)]
struct Tables {
    mesh: Arc<RenderMesh>,
    picking: Arc<Picking>,
}

impl std::ops::Deref for Tables {
    type Target = Picking;

    fn deref(&self) -> &Picking {
        &self.picking
    }
}

impl Tables {
    /// The body face `f` is of.
    fn body(&self, f: u32) -> BodyId {
        self.picking
            .face_body(&self.mesh, f)
            .expect("a face of a part")
    }
}

/// `editor`'s model with `inspect` measured, answered by `regenerator`:
/// the tables and the answer.
fn ask(regenerator: &mut Regenerator, editor: &Editor, inspect: Inspect) -> (Tables, Inspected) {
    let response = regenerator.handle(Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude: None,
        draft: None,
        inspect: Some(Box::new(inspect)),
    });
    assert_eq!(response.inspect(), Some(inspect_revision(&response)));
    let Response::Regenerated {
        mesh,
        picking,
        inspected,
        ..
    } = response
    else {
        panic!("regeneration failed: {response:?}");
    };
    (
        Tables { mesh, picking },
        *inspected.expect("a measure was asked for"),
    )
}

fn inspect_revision(response: &Response) -> u64 {
    match response {
        Response::Regenerated { inspected, .. } => inspected.as_ref().unwrap().revision,
        _ => panic!("not a model"),
    }
}

/// The example's tables, without a measure.
fn tables(editor: &Editor) -> Tables {
    let response = crate::handle(Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude: None,
        draft: None,
        inspect: None,
    });
    let Response::Regenerated {
        mesh,
        picking,
        inspected,
        ..
    } = response
    else {
        panic!("regeneration failed");
    };
    assert_eq!(inspected, None);
    Tables { mesh, picking }
}

/// The face of `body` whose summary is `summary`.
fn face(picking: &Tables, body: BodyId, summary: Summary) -> u32 {
    let found = (0..picking.faces().len() as u32)
        .find(|&f| picking.body(f) == body && picking.faces()[f as usize].summary == summary);
    found.expect("the face is there")
}

fn top(picking: &Tables, body: BodyId) -> u32 {
    let n = [0.0, 0.0, 1.0];
    face(picking, body, Summary::Plane { n, d: 10.0 })
}

fn bottom(picking: &Tables, body: BodyId) -> u32 {
    let n = [0.0, 0.0, -1.0];
    face(picking, body, Summary::Plane { n, d: 0.0 })
}

fn wall(picking: &Tables, body: BodyId) -> u32 {
    let found = (0..picking.faces().len() as u32).find(|&f| {
        picking.body(f) == body
            && matches!(picking.faces()[f as usize].summary, Summary::Cylinder { radius, .. } if (radius - 8.0).abs() < 1e-9)
    });
    found.expect("the hole's wall")
}

/// The edge between faces `a` and `b`.
fn chain(picking: &Tables, a: u32, b: u32) -> u32 {
    let found = (picking.mesh.edge_faces().iter()).position(|&faces| {
        let mut faces = faces;
        faces.sort_unstable();
        faces == [a.min(b), a.max(b)]
    });
    found.expect("the edge is there") as u32
}

/// The corner at `point`.
fn corner(picking: &Tables, point: [f64; 3]) -> u32 {
    let found = (picking.corners().iter()).position(|c| c.point == point);
    found.expect("the corner is there") as u32
}

fn face_pick(picking: &Tables, f: u32, near: [f64; 3]) -> InspectPick {
    let face = &picking.faces()[f as usize];
    InspectPick {
        body: picking.body(f),
        entity: Entity::Face(face.key),
        near,
    }
}

fn edge_pick(picking: &Tables, c: u32, near: [f64; 3]) -> InspectPick {
    let body = picking.body(picking.mesh.edge_faces()[c as usize][0]);
    InspectPick {
        body,
        entity: Entity::Edge(picking.edge_keys(&picking.mesh, c).unwrap()),
        near,
    }
}

fn corner_pick(picking: &Tables, c: u32) -> InspectPick {
    let corner = picking.corners()[c as usize];
    InspectPick {
        body: picking.body(corner.faces[0]),
        entity: Entity::Corner(picking.corner_keys(c)),
        near: corner.point,
    }
}

fn body_pick(body: BodyId) -> InspectPick {
    InspectPick {
        body,
        entity: Entity::Body,
        near: [0.0; 3],
    }
}

fn one(revision: u64, first: InspectPick) -> Inspect {
    Inspect {
        revision,
        first,
        second: None,
    }
}

fn two(revision: u64, first: InspectPick, second: InspectPick) -> Inspect {
    Inspect {
        revision,
        first,
        second: Some(second),
    }
}

fn probed(result: &Result<Probed, String>) -> &Probed {
    result.as_ref().expect("the pick is found")
}

fn measured(result: &Result<Probed, String>) -> Measure {
    probed(result)
        .measure
        .clone()
        .expect("the pick is measured")
}

fn gap(inspected: &Inspected) -> Gap {
    let between = inspected.between.as_ref().expect("both found");
    between.distance.clone().expect("the distance is measured")
}

#[test]
fn a_face_is_measured_and_found_in_the_tables() {
    let editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let picking = tables(&editor);
    let top = top(&picking, body);
    let pick = face_pick(&picking, top, [20.0, 10.0, 10.0]);
    let (answered, inspected) = ask(&mut Regenerator::default(), &editor, one(3, pick));
    assert_eq!(answered, picking);
    assert_eq!(inspected.revision, 3);
    assert_eq!(probed(&inspected.first).at, Some(At::Face(top)));
    let Measure::Face {
        area,
        summary,
        half_angle,
        rectangle,
    } = measured(&inspected.first)
    else {
        panic!("a face's measure");
    };
    assert_near(area, TOP_AREA);
    assert_eq!(summary, picking.faces()[top as usize].summary);
    assert_eq!(half_angle, None);
    assert_eq!(rectangle, None);
    assert_eq!((inspected.second, inspected.between), (None, None));
}

#[test]
fn two_parallel_faces_give_the_thickness_and_their_angle() {
    let editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let picking = tables(&editor);
    let (top, bottom) = (top(&picking, body), bottom(&picking, body));
    let inspect = two(
        1,
        face_pick(&picking, top, [20.0, 10.0, 10.0]),
        face_pick(&picking, bottom, [-20.0, 0.0, 0.0]),
    );
    let (_, inspected) = ask(&mut Regenerator::default(), &editor, inspect);
    let gap = gap(&inspected);
    assert_near(gap.distance, 10.0);
    assert_near(gap.points[0][2], 10.0);
    assert_near(gap.points[1][2], 0.0);
    // Outward normals, opposite: half a turn.
    let angle = inspected.between.unwrap().angle.unwrap();
    assert_near(angle, PI);
    assert_eq!(
        probed(&inspected.second.unwrap()).at,
        Some(At::Face(bottom))
    );
}

#[test]
fn a_hole_s_rim_gives_its_radius_and_centre() {
    let editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let picking = tables(&editor);
    let rim = chain(&picking, top(&picking, body), wall(&picking, body));
    assert_point(picking.snaps()[rim as usize].unwrap(), [0.0, 0.0, 10.0]);
    let edge = edge_pick(&picking, rim, [8.0, 0.0, 10.0]);
    let InspectPick {
        entity: Entity::Edge(keys),
        ..
    } = edge
    else {
        unreachable!()
    };
    let point = InspectPick {
        entity: Entity::EdgePoint(keys),
        ..edge
    };
    let bottom = face_pick(&picking, bottom(&picking, body), [0.0, 15.0, 0.0]);
    let mut regenerator = Regenerator::default();
    let (_, inspected) = ask(&mut regenerator, &editor, two(1, edge, point));
    assert_eq!(probed(&inspected.first).at, Some(At::Edge(rim)));
    let Measure::Edge {
        length,
        closed,
        shape,
    } = measured(&inspected.first)
    else {
        panic!("an edge's measure");
    };
    assert_near(length, 16.0 * PI);
    assert!(closed);
    let EdgeForm::Circle {
        centre,
        axis,
        radius,
    } = shape
    else {
        panic!("a circle: {shape:?}");
    };
    assert_near(radius, 8.0);
    assert_point(centre, [0.0, 0.0, 10.0]);
    assert_near(axis[2].abs(), 1.0);
    // Its centre is a point, 8 from the rim; a point has no direction.
    assert_eq!(
        probed(&inspected.second.clone().unwrap()).at,
        Some(At::Edge(rim))
    );
    let Measure::Point(p) = measured(&inspected.second.clone().unwrap()) else {
        panic!("a point");
    };
    assert_point(p, [0.0, 0.0, 10.0]);
    assert_near(gap(&inspected).distance, 8.0);
    assert_eq!(inspected.between.unwrap().angle, None);

    // The centre to the bottom, which the hole goes through: to its rim.
    let (_, inspected) = ask(&mut regenerator, &editor, two(2, point, bottom));
    assert_near(gap(&inspected).distance, 164.0f64.sqrt());
    assert_eq!(inspected.between.unwrap().angle, None);
}

#[test]
fn a_straight_edge_s_point_is_its_middle() {
    let editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let picking = tables(&editor);
    let x = [1.0, 0.0, 0.0];
    let side = face(&picking, body, Summary::Plane { n: x, d: 30.0 });
    let c = chain(&picking, top(&picking, body), side);
    assert_eq!(picking.snaps()[c as usize], Some([30.0, 0.0, 10.0]));
    let mut pick = edge_pick(&picking, c, [30.0, 5.0, 10.0]);
    let Entity::Edge(keys) = pick.entity else {
        unreachable!()
    };
    pick.entity = Entity::EdgePoint(keys);
    let (_, inspected) = ask(&mut Regenerator::default(), &editor, one(1, pick));
    assert_eq!(
        measured(&inspected.first),
        Measure::Point([30.0, 0.0, 10.0])
    );
}

#[test]
fn corners_are_their_vertices_exactly() {
    let editor = Editor::new(Document::example());
    let picking = tables(&editor);
    let near = corner(&picking, [30.0, 20.0, 10.0]);
    let far = corner(&picking, [-30.0, -20.0, 0.0]);
    let inspect = two(1, corner_pick(&picking, near), corner_pick(&picking, far));
    let (_, inspected) = ask(&mut Regenerator::default(), &editor, inspect);
    assert_eq!(probed(&inspected.first).at, Some(At::Corner(near)));
    assert_eq!(
        measured(&inspected.first),
        Measure::Point([30.0, 20.0, 10.0])
    );
    assert_eq!(
        measured(&inspected.second.clone().unwrap()),
        Measure::Point([-30.0, -20.0, 0.0])
    );
    let gap = gap(&inspected);
    assert_near(
        gap.distance,
        (60.0f64 * 60.0 + 40.0 * 40.0 + 10.0 * 10.0).sqrt(),
    );
    assert_eq!(gap.points, [[30.0, 20.0, 10.0], [-30.0, -20.0, 0.0]]);
}

#[test]
fn a_body_gives_its_volume_area_box_and_centre() {
    let editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let (_, inspected) = ask(
        &mut Regenerator::default(),
        &editor,
        one(1, body_pick(body)),
    );
    assert_eq!(probed(&inspected.first).at, None);
    let Measure::Body {
        volume,
        area,
        centre,
        bounds,
    } = measured(&inspected.first)
    else {
        panic!("a body's measure");
    };
    assert_near(volume, VOLUME);
    let sides = 2.0 * (60.0 + 40.0) * 10.0;
    assert_near(area, 2.0 * TOP_AREA + sides + 16.0 * PI * 10.0);
    assert_point(centre.unwrap(), [0.0, 0.0, 5.0]);
    assert_eq!(bounds, Some([[-30.0, -20.0, 0.0], [30.0, 20.0, 10.0]]));
}

#[test]
fn picks_of_a_second_body_are_found_in_its_part_of_the_tables() {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let below = plate_below(&mut editor);
    let picking = tables(&editor);
    // The lower plate's top is at z = 0, its bottom at z = −3.
    let n = [0.0, 0.0, -1.0];
    let under = face(&picking, below, Summary::Plane { n, d: 3.0 });
    let c = (picking.corners().iter())
        .position(|c| c.point == [30.0, 20.0, -3.0])
        .unwrap() as u32;
    assert_eq!(picking.body(picking.corners()[c as usize].faces[0]), below);
    let inspect = two(
        1,
        face_pick(&picking, under, [0.0, 15.0, -3.0]),
        corner_pick(&picking, c),
    );
    let (_, inspected) = ask(&mut Regenerator::default(), &editor, inspect);
    assert_eq!(probed(&inspected.first).at, Some(At::Face(under)));
    assert_eq!(
        probed(&inspected.second.clone().unwrap()).at,
        Some(At::Corner(c))
    );
    assert_near(gap(&inspected).distance, 0.0);

    // The two bodies' bottoms, 3 apart; the plates touch, so 0 between
    // the bodies.
    let bottom = face_pick(&picking, bottom(&picking, plate), [0.0, 15.0, 0.0]);
    let inspect = two(2, bottom, face_pick(&picking, under, [0.0, 15.0, -3.0]));
    let (_, inspected) = ask(&mut Regenerator::default(), &editor, inspect);
    assert_near(gap(&inspected).distance, 3.0);
    let inspect = two(3, body_pick(plate), body_pick(below));
    let (_, inspected) = ask(&mut Regenerator::default(), &editor, inspect);
    assert_near(gap(&inspected).distance, 0.0);
}

#[test]
fn after_an_edit_the_same_picks_measure_what_they_name_now() {
    let mut editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let picking = tables(&editor);
    let inspect = two(
        1,
        face_pick(&picking, top(&picking, body), [20.0, 10.0, 10.0]),
        face_pick(&picking, bottom(&picking, body), [-20.0, 0.0, 0.0]),
    );
    let mut regenerator = Regenerator::default();
    let (_, before) = ask(&mut regenerator, &editor, inspect.clone());
    assert_near(gap(&before).distance, 10.0);

    let plate = editor.document().features()[1].id;
    let extent = varde_document::Extent::OneSide(length(editor.document(), "6"));
    set_extrude(&mut editor, plate, |extrude| extrude.extent = extent);
    let (picking, after) = ask(&mut regenerator, &editor, inspect);
    assert_near(gap(&after).distance, 6.0);
    let At::Face(top_now) = probed(&after.first).at.unwrap() else {
        panic!("a face");
    };
    let n = [0.0, 0.0, 1.0];
    assert_eq!(
        picking.faces()[top_now as usize].summary,
        Summary::Plane { n, d: 6.0 }
    );
}

#[test]
fn picks_naming_nothing_are_answered_not_found() {
    let editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let picking = tables(&editor);
    let top = face_pick(&picking, top(&picking, body), [20.0, 10.0, 10.0]);
    let mut gone = top;
    if let Entity::Face(key) = &mut gone.entity {
        key.feature = 999;
    }
    let (_, inspected) = ask(&mut Regenerator::default(), &editor, two(1, top, gone));
    assert!(inspected.first.is_ok());
    assert_eq!(inspected.second, Some(Err("face not found".to_owned())));
    assert_eq!(inspected.between, None);

    let nobody = body_pick(BodyId::NEW);
    let (_, inspected) = ask(&mut Regenerator::default(), &editor, one(2, nobody));
    assert_eq!(inspected.first, Err("body not found".to_owned()));

    // A face's key twice as an edge's: no edge has the top on both
    // sides.
    let Entity::Face(key) = top.entity else {
        unreachable!()
    };
    let pick = InspectPick {
        entity: Entity::Edge([key; 2]),
        ..top
    };
    let (_, inspected) = ask(&mut Regenerator::default(), &editor, one(3, pick));
    assert_eq!(inspected.first, Err("edge not found".to_owned()));
}

/// A slot cut through the plate at `15 ≤ x ≤ 17` leaves its top in two
/// pieces under the one key: the point picked says which, and its place
/// in the tables is that piece's.
#[test]
fn of_several_faces_of_the_same_key_the_nearest_is_measured() {
    let mut editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let before = tables(&editor);
    let top_key = before.faces()[top(&before, body) as usize].key;
    let extent = crate::history::tests::two_sides(editor.document(), "20", "20");
    crate::history::tests::add_extrude(
        &mut editor,
        crate::history::tests::rectangle((15.0, -30.0), (17.0, 30.0)),
        extent,
        varde_document::Operation::Cut(varde_document::Targets::default()),
    );
    let picking = tables(&editor);
    let tops: Vec<u32> = (0..picking.faces().len() as u32)
        .filter(|&f| picking.faces()[f as usize].key == top_key)
        .collect();
    assert_eq!(tops.len(), 2, "the top is in two pieces");
    let pick = |near: [f64; 3]| InspectPick {
        body,
        entity: Entity::Face(top_key),
        near,
    };
    let (_, inspected) = ask(
        &mut Regenerator::default(),
        &editor,
        two(1, pick([25.0, 0.0, 10.0]), pick([-20.0, 0.0, 10.0])),
    );
    let area = |probed: &Result<Probed, String>| match measured(probed) {
        Measure::Face { area, .. } => area,
        other => panic!("a face: {other:?}"),
    };
    assert_near(area(&inspected.first), 13.0 * 40.0);
    assert_near(
        area(&inspected.second.clone().unwrap()),
        45.0 * 40.0 - PI * 64.0,
    );
    let place = |probed: &Result<Probed, String>| match probed.as_ref().unwrap().at {
        Some(At::Face(f)) => f,
        other => panic!("a face's place: {other:?}"),
    };
    let (right, left) = (
        place(&inspected.first),
        place(inspected.second.as_ref().unwrap()),
    );
    assert_ne!(right, left);
    assert!(tops.contains(&right) && tops.contains(&left));
    // The right piece's place is the one drawn at the right: one of its
    // triangles has a vertex past the slot.
    let Response::Regenerated { mesh, .. } = crate::handle(Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude: None,
        draft: None,
        inspect: None,
    }) else {
        panic!("regeneration failed");
    };
    let positions = mesh.positions();
    let indices = mesh.face_indices(right as usize).unwrap();
    let xs = mesh.indices()[indices]
        .iter()
        .map(|&v| positions[v as usize][0]);
    assert!(xs.into_iter().all(|x| x >= 17.0));
    assert_near(gap(&inspected).distance, 2.0);
}

/// Keys that name nothing for sure are refused, not resolved to some
/// entity: a corner by a face's key twice, by a key and its own face's
/// again, and any pick without a finite point.
#[test]
fn picks_naming_nothing_for_sure_are_refused() {
    let editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let picking = tables(&editor);
    let c = corner(&picking, [30.0, 20.0, 10.0]);
    let good = corner_pick(&picking, c);
    let Entity::Corner([a, b, _]) = good.entity else {
        unreachable!()
    };
    let mut answers = Vec::new();
    for keys in [[a, a, b], [a, b, b], [a; 3]] {
        let pick = InspectPick {
            entity: Entity::Corner(keys),
            ..good
        };
        let (_, inspected) = ask(&mut Regenerator::default(), &editor, one(1, pick));
        answers.push(inspected.first);
    }
    assert!(
        answers
            .iter()
            .all(|a| a == &Err("corner not found".to_owned())),
        "{answers:?}"
    );
    let top = face_pick(&picking, top(&picking, body), [20.0, 10.0, 10.0]);
    for near in [f64::NAN, f64::INFINITY] {
        for pick in [
            InspectPick {
                near: [near, 0.0, 0.0],
                ..top
            },
            InspectPick {
                near: [0.0, 0.0, near],
                ..good
            },
        ] {
            let (_, inspected) = ask(&mut Regenerator::default(), &editor, one(2, pick));
            assert_eq!(inspected.first, Err("the pick has no point".to_owned()));
        }
    }
    // A body needs no point.
    let pick = InspectPick {
        near: [f64::NAN; 3],
        ..body_pick(body)
    };
    let (_, inspected) = ask(&mut Regenerator::default(), &editor, one(3, pick));
    assert!(inspected.first.is_ok());
}

#[test]
fn a_hidden_body_is_measured_but_not_in_the_tables() {
    let mut editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let picking = tables(&editor);
    let top = face_pick(&picking, top(&picking, body), [20.0, 10.0, 10.0]);
    editor.apply(Command::SetVisible(body, false)).unwrap();
    let (picking, inspected) = ask(&mut Regenerator::default(), &editor, one(1, top));
    assert!(picking.faces().is_empty());
    assert_eq!(probed(&inspected.first).at, None);
    let Measure::Face { area, .. } = measured(&inspected.first) else {
        panic!("a face");
    };
    assert_near(area, TOP_AREA);
}

#[test]
fn measures_asked_again_are_answered_from_the_cache() {
    let editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let picking = tables(&editor);
    let inspect = two(
        1,
        face_pick(&picking, top(&picking, body), [20.0, 10.0, 10.0]),
        body_pick(body),
    );
    let mut regenerator = Regenerator::default();
    let (_, first) = ask(&mut regenerator, &editor, inspect.clone());
    let held = regenerator.cache().len();
    let counts = regenerator.cache().counts();
    let (_, again) = ask(
        &mut regenerator,
        &editor,
        Inspect {
            revision: 2,
            ..inspect
        },
    );
    assert_eq!(regenerator.cache().len(), held);
    assert_eq!(
        Inspected {
            revision: 1,
            ..again
        },
        first
    );
    // Measures aren't features' results.
    let (hits, misses) = regenerator.cache().counts();
    assert_eq!(misses, counts.1);
    assert!(hits > counts.0);
}

#[test]
fn a_measure_on_a_draft_is_of_the_drafted_model() {
    let editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let picking = tables(&editor);
    let plate = editor.document().features()[1].id;
    let mut extrude = extrude_of(editor.document(), plate);
    extrude.extent = varde_document::Extent::OneSide(length(editor.document(), "4"));
    let inspect = two(
        1,
        face_pick(&picking, top(&picking, body), [20.0, 10.0, 10.0]),
        face_pick(&picking, bottom(&picking, body), [-20.0, 0.0, 0.0]),
    );
    let response = crate::handle(Request::Regenerate {
        generation: editor.generation(),
        document: editor.snapshot(),
        exclude: None,
        draft: Some(Box::new(crate::Draft {
            revision: 1,
            feature: Some(plate),
            kind: varde_document::FeatureKind::Extrude(extrude),
        })),
        inspect: Some(Box::new(inspect)),
    });
    let Response::Regenerated {
        inspected: Some(inspected),
        ..
    } = response
    else {
        panic!("regeneration failed");
    };
    assert_near(gap(&inspected).distance, 4.0);
}

fn extrude_of(document: &Document, feature: varde_document::FeatureId) -> varde_document::Extrude {
    match &document.feature(feature).unwrap().kind {
        varde_document::FeatureKind::Extrude(extrude) => extrude.clone(),
        _ => panic!("an extrude"),
    }
}

#[test]
fn a_failed_regeneration_says_which_measure_it_had() {
    let request = Request::Regenerate {
        generation: 4.into(),
        document: Arc::new(Document::default()),
        exclude: None,
        draft: None,
        inspect: Some(Box::new(one(9, body_pick(BodyId::NEW)))),
    };
    assert_eq!(request.inspect(), Some(9));
    let failed = request.failure()("panicked".to_owned());
    assert_eq!(failed.inspect(), Some(9));
}

/// An answer that is right as it stands, of the example's tables.
fn good(picking: &Tables) -> Inspected {
    Inspected {
        revision: 1,
        first: Ok(Probed {
            at: Some(At::Edge(0)),
            measure: Ok(Measure::Edge {
                length: 1.0,
                closed: false,
                shape: EdgeForm::Circle {
                    centre: [0.0; 3],
                    axis: [0.0, 0.0, 1.0],
                    radius: 1.0,
                },
            }),
        }),
        second: Some(Ok(Probed {
            at: Some(At::Corner(picking.corners().len() as u32 - 1)),
            measure: Ok(Measure::Point([1.0, 2.0, 3.0])),
        })),
        between: Some(Between {
            distance: Ok(Gap {
                distance: 3f64.sqrt(),
                points: [[0.0; 3], [1.0; 3]],
            }),
            angle: Some(1.0),
        }),
    }
}

#[test]
fn broken_answers_are_checked_into_errors() {
    let editor = Editor::new(Document::example());
    let picking = tables(&editor);
    let good = good(&picking);
    assert_eq!(good.clone().checked(&picking.mesh, &picking.picking), good);
    let breaks: [fn(&mut Inspected); 17] = [
        |i| i.revision = i.revision.wrapping_add(0), // unchanged: stays good
        |i| i.first.as_mut().unwrap().at = Some(At::Face(1000)),
        |i| i.first.as_mut().unwrap().at = Some(At::Edge(1000)),
        |i| i.second.as_mut().unwrap().as_mut().unwrap().at = Some(At::Corner(1000)),
        |i| {
            i.second.as_mut().unwrap().as_mut().unwrap().measure =
                Ok(Measure::Point([f64::NAN, 0.0, 0.0]))
        },
        |i| {
            i.second.as_mut().unwrap().as_mut().unwrap().measure =
                Ok(Measure::Point([2e8, 0.0, 0.0]))
        },
        |i| {
            i.first.as_mut().unwrap().at = Some(At::Face(0));
            i.first.as_mut().unwrap().measure = Ok(Measure::Face {
                area: -1.0,
                summary: Summary::Other,
                half_angle: None,
                rectangle: None,
            })
        },
        |i| {
            i.first.as_mut().unwrap().measure = Ok(Measure::Edge {
                length: 1.0,
                closed: true,
                shape: EdgeForm::Circle {
                    centre: [0.0; 3],
                    axis: [0.0, 0.0, 2.0],
                    radius: 1.0,
                },
            })
        },
        |i| {
            i.first.as_mut().unwrap().at = None;
            i.first.as_mut().unwrap().measure = Ok(Measure::Body {
                volume: 1.0,
                area: f64::INFINITY,
                centre: None,
                bounds: None,
            })
        },
        |i| {
            i.first.as_mut().unwrap().at = None;
            i.first.as_mut().unwrap().measure = Ok(Measure::Body {
                volume: 1.0,
                area: 1.0,
                centre: None,
                bounds: Some([[1.0; 3], [0.0; 3]]),
            })
        },
        |i| i.between.as_mut().unwrap().angle = Some(4.0),
        |i| i.second = None,
        // Places of another kind than their measures.
        |i| i.first.as_mut().unwrap().at = Some(At::Face(0)),
        |i| i.first.as_mut().unwrap().at = Some(At::Corner(0)),
        |i| i.second.as_mut().unwrap().as_mut().unwrap().at = Some(At::Face(0)),
        |i| {
            i.first.as_mut().unwrap().measure = Ok(Measure::Body {
                volume: 1.0,
                area: 1.0,
                centre: None,
                bounds: None,
            })
        },
        // A distance that isn't its points'.
        |i| {
            let gap = i.between.as_mut().unwrap().distance.as_mut().unwrap();
            gap.distance = 1.0;
        },
    ];
    for (k, change) in breaks.iter().enumerate() {
        let mut broken = good.clone();
        change(&mut broken);
        let checked = broken.clone().checked(&picking.mesh, &picking.picking);
        if k == 0 {
            assert_eq!(checked, broken);
            continue;
        }
        assert_eq!(checked.revision, 1, "{k}");
        assert_eq!(checked.first, Err(BROKEN.to_owned()), "{k}");
        assert_eq!(checked.between, None, "{k}");
    }
    let mut broken = good.clone();
    broken.between.as_mut().unwrap().distance = Ok(Gap {
        distance: -1.0,
        points: [[0.0; 3]; 2],
    });
    assert_eq!(
        broken.checked(&picking.mesh, &picking.picking).first,
        Err(BROKEN.to_owned())
    );
}

/// The example's corners are its box's eight, each between three
/// planes through it; its twelve straight edges snap to their middles and
/// its rims to the hole's centres.
#[test]
fn the_corners_table_holds_the_example_s_corners() {
    let editor = Editor::new(Document::example());
    let picking = tables(&editor);
    let mut points: Vec<[f64; 3]> = picking.corners().iter().map(|c| c.point).collect();
    points.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut box_corners = Vec::new();
    for x in [-30.0, 30.0] {
        for y in [-20.0, 20.0] {
            for z in [0.0, 10.0] {
                box_corners.push([x, y, z]);
            }
        }
    }
    assert_eq!(points, box_corners);
    for (c, corner) in picking.corners().iter().enumerate() {
        assert!(corner.faces.windows(2).all(|pair| pair[0] < pair[1]));
        for f in corner.faces {
            let Summary::Plane { n, d } = picking.faces()[f as usize].summary else {
                panic!("a box's corner is between planes");
            };
            let on = (0..3).map(|i| n[i] * corner.point[i]).sum::<f64>();
            assert_near(on, d);
        }
        let keys = picking.corner_keys(c as u32);
        assert!(keys.windows(2).all(|pair| pair[0] <= pair[1]));
    }
    for (snap, closed) in picking.snaps().iter().zip(picking.closed()) {
        let snap = snap.expect("every edge has a point");
        if *closed {
            assert_point(snap, [0.0, 0.0, snap[2]]);
        } else {
            // The middle of a box edge: two coordinates on the box's
            // sides, the third halfway.
            let ends = snap
                .iter()
                .zip([30.0, 20.0, 10.0])
                .filter(|(x, side)| x.abs() == *side || (*side == 10.0 && **x == 0.0));
            assert_eq!(ends.count(), 2, "{snap:?}");
        }
    }
}

/// Places are checked against what the tables hold there: tables of
/// another model than the topology's (the plate before a slot cut it,
/// the slotted plate's topology) give a place only where the entry is
/// the same region, chain or corner, never another's.
#[test]
fn places_in_tables_that_don_t_line_up_are_none() {
    let mut editor = Editor::new(Document::example());
    let body = editor.document().bodies()[0].id;
    let before = tables(&editor);
    let extent = crate::history::tests::two_sides(editor.document(), "20", "20");
    crate::history::tests::add_extrude(
        &mut editor,
        crate::history::tests::rectangle((15.0, -30.0), (17.0, 30.0)),
        extent,
        varde_document::Operation::Cut(varde_document::Targets::default()),
    );
    let mut cache = Cache::default();
    let evaluation = crate::evaluate(editor.document(), &mut cache);
    let made = &evaluation.bodies[0];
    let topology = topology(made, &mut cache);
    let places = Places::of(&before.mesh, &before, body, &topology, &made.solid);
    let mut placed = 0;
    for r in 0..topology.regions().len() as u32 {
        if let Some(At::Face(f)) = places.face(r) {
            assert_eq!(
                before.faces()[f as usize].key,
                topology.regions()[r as usize].key
            );
            placed += 1;
        }
    }
    assert!(placed < topology.regions().len());
    // The slotted plate's own tables place every region, chain and
    // corner.
    let own = tables(&editor);
    let places = Places::of(&own.mesh, &own, body, &topology, &made.solid);
    assert!((0..topology.regions().len() as u32).all(|r| places.face(r).is_some()));
    assert!((0..topology.chains().len() as u32).all(|c| places.chain(c).is_some()));
    assert!((0..topology.corners().len() as u32).all(|c| places.corner(c).is_some()));
    let places = Places::of(&before.mesh, &before, body, &topology, &made.solid);
    for c in 0..topology.chains().len() as u32 {
        if let Some(At::Edge(i)) = places.chain(c) {
            let keys = topology.chains()[c as usize]
                .regions
                .map(|r| topology.regions()[r as usize].key);
            let mut keys = keys;
            keys.sort_unstable();
            assert_eq!(before.edge_keys(&before.mesh, i), Some(keys));
        }
    }
    for c in 0..topology.corners().len() as u32 {
        if let Some(At::Corner(i)) = places.corner(c) {
            let vertex = topology.corners()[c as usize].vertex;
            let point = made.solid.mesh().verts()[vertex as usize].to_array();
            assert_eq!(before.corners()[i as usize].point, point);
        }
    }
}

/// Picks of a body a later join merges into another are measured on the
/// body holding it: a face by its key, and the body whole as the holder;
/// their places are the holder's in the tables.
#[test]
fn picks_of_a_merged_body_are_measured_on_its_holder() {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let below = plate_below(&mut editor);
    let before = tables(&editor);
    let n = [0.0, 0.0, -1.0];
    let under = face(&before, below, Summary::Plane { n, d: 3.0 });
    let under = face_pick(&before, under, [-20.0, 15.0, -3.0]);
    assert_eq!(under.body, below);
    let extent = crate::history::tests::two_sides(editor.document(), "15", "5");
    crate::history::tests::add_extrude(
        &mut editor,
        crate::history::tests::disc((20.0, 0.0), 5.0),
        extent,
        varde_document::Operation::Join(varde_document::Targets::default()),
    );
    let inspect = two(1, under, body_pick(below));
    let (after, inspected) = ask(&mut Regenerator::default(), &editor, inspect);
    assert_eq!(after.bodies(), [plate]);
    let Some(At::Face(f)) = probed(&inspected.first).at else {
        panic!("{inspected:?}");
    };
    assert_eq!(after.body(f), plate);
    let Entity::Face(key) = under.entity else {
        unreachable!()
    };
    assert_eq!(after.faces()[f as usize].key, key);
    let Measure::Face { area, .. } = measured(&inspected.first) else {
        panic!("a face");
    };
    // The lower plate's bottom, its hole and the boss's foot cut out.
    assert_near(area, TOP_AREA - PI * 25.0);
    let Measure::Body { volume, .. } = measured(&inspected.second.clone().unwrap()) else {
        panic!("a body");
    };
    let joined = VOLUME + TOP_AREA * 3.0 + PI * 25.0 * (20.0 - 13.0);
    assert_near(volume, joined);
    // The face is on the body, so 0 between.
    assert_near(gap(&inspected).distance, 0.0);
}
