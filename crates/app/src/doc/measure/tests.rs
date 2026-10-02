use std::cell::RefCell;
use std::f64::consts::PI;

use glam::DVec3;
use iced::keyboard;
use varde_regen::{Request, Summary};
use varde_view::{Edit, Look, Message as Ui, Mode, RailLook};

use super::*;
use crate::tests::{answer, example, key_in, screen_texts, shown, texts};

/// The face of `doc`'s model shown whose summary `is` takes, as a pick
/// of it at `at`.
fn face(doc: &Doc, at: DVec3, is: impl Fn(&Summary) -> bool) -> Pick {
    let index = doc.feed.pick_index();
    let faces = index.picking().faces();
    let face = faces.iter().position(|face| is(&face.summary)).unwrap();
    Pick {
        model: index.model(),
        target: Picked::Face(face as u32),
        body: index.face_body(face as u32).unwrap(),
        at,
        snap: None,
    }
}

/// The example's plate's top, at z 10.
fn top(doc: &Doc) -> Pick {
    face(
        doc,
        DVec3::new(20.0, 5.0, 10.0),
        |summary| matches!(summary, Summary::Plane { n, .. } if n[2] > 0.5),
    )
}

/// The example's plate's bottom, at z 0.
fn bottom(doc: &Doc) -> Pick {
    face(
        doc,
        DVec3::new(20.0, 5.0, 0.0),
        |summary| matches!(summary, Summary::Plane { n, .. } if n[2] < -0.5),
    )
}

/// The rim of the example's hole on the top, as a pick of it.
fn rim(doc: &Doc) -> Pick {
    let index = doc.feed.pick_index();
    let picking = index.picking();
    let is =
        |face: u32, test: &dyn Fn(&Summary) -> bool| test(&picking.faces()[face as usize].summary);
    let chain = (0..index.mesh().edge_count() as u32)
        .find(|&edge| {
            let wall = |s: &Summary| matches!(s, Summary::Cylinder { .. });
            let top = |s: &Summary| matches!(s, Summary::Plane { n, .. } if n[2] > 0.5);
            let Some([a, b]) = index.edge_faces(edge) else {
                return false;
            };
            (is(a, &wall) && is(b, &top)) || (is(b, &wall) && is(a, &top))
        })
        .unwrap();
    Pick {
        model: index.model(),
        target: Picked::Edge(chain),
        body: index.body(Picked::Edge(chain)).unwrap(),
        at: index.chain_point(chain).unwrap(),
        snap: None,
    }
}

/// A click on what `pick` picks of `doc`, with Shift if `add`.
fn click(doc: &mut Doc, pick: impl FnOnce(&Doc) -> Pick, add: bool) {
    let pick = pick(doc);
    doc.look(Look::ClickModel {
        pick: Some(pick),
        add,
        double: false,
    });
}

/// The panel's value labelled `label` and its text, the first of them.
fn value_after(texts: &[String], label: &str) -> String {
    let at = texts.iter().position(|text| text == label);
    let at = at.unwrap_or_else(|| panic!("no {label} in {texts:?}"));
    texts[at + 1].clone()
}

/// The measure the request asked last carries, if it was a regeneration.
fn last_inspect(requests: &RefCell<Vec<Request>>) -> Option<varde_regen::Inspect> {
    match requests.borrow().last()? {
        Request::Regenerate { inspect, .. } => inspect.as_deref().cloned(),
        Request::Export { .. } => None,
    }
}

#[test]
fn i_starts_and_leaves_the_measure_tool_and_so_does_the_rail() {
    let (mut doc, _requests) = example();
    key_in(&mut doc, keyboard::Key::Character("i".into()));
    assert!(doc.measure.is_some());
    key_in(&mut doc, keyboard::Key::Character("i".into()));
    assert!(doc.measure.is_none());
    // The rail's Inspect set: its list, then Enter on Measure.
    doc.look(Look::Rail(RailLook::Open(1)));
    key_in(&mut doc, keyboard::Key::Named(keyboard::key::Named::Enter));
    assert!(doc.measure.is_some());
    assert_eq!(doc.rail.open, None);
    let texts = screen_texts(&doc);
    assert!(texts.contains(&"Measure".to_owned()), "{texts:?}");
    assert!(
        texts.contains(&"Click a face, edge, point or body".to_owned()),
        "{texts:?}"
    );
    // Close leaves it.
    doc.look(Look::Measure(MeasureLook::Close));
    assert!(doc.measure.is_none());

    // Not in a sketch, nor while an operation is set up; starting one
    // leaves it. A read-only document is measured too.
    doc.look(Look::StartMeasure);
    doc.look(Look::StartExtrude);
    assert!(doc.measure.is_none());
    assert!(doc.extrude.is_some());
    doc.look(Look::StartMeasure);
    assert!(doc.measure.is_none());
    doc.look(Look::Escape);
    let sketch = doc.editor.document().features()[0].id;
    doc.look(Look::EditFeature(sketch));
    doc.look(Look::StartMeasure);
    assert!(doc.measure.is_none());
    doc.look(Look::FinishSketch);
    doc.read_only = Some("in use".to_owned());
    key_in(&mut doc, keyboard::Key::Character("i".into()));
    assert!(doc.measure.is_some());
}

#[test]
fn two_parallel_faces_give_the_plate_s_thickness_in_mm_and_in() {
    let (mut doc, requests) = example();
    let generation = doc.editor.generation();
    let edited = doc.edited();
    doc.look(Look::StartMeasure);
    click(&mut doc, top, false);
    click(&mut doc, bottom, false);
    answer(&mut doc, &requests);
    // Measuring wrote nothing.
    assert_eq!(doc.editor.generation(), generation);
    assert_eq!(doc.edited(), edited);
    let between = doc.feed.inspected().unwrap().between.clone().unwrap();
    assert_eq!(between.distance.as_ref().unwrap().distance, 10.0);
    assert_eq!(between.angle, Some(PI));
    let texts = screen_texts(&doc);
    assert_eq!(value_after(&texts, "Distance"), "10 mm");
    assert_eq!(value_after(&texts, "Angle"), "180°");
    for name in ["A  Planar face of Body 1", "B  Planar face of Body 1"] {
        assert!(
            texts.iter().any(|text| text.contains(&name[3..])),
            "{name}: {texts:?}"
        );
    }
    // In inches, as the design's units say: 4 decimals.
    doc.update(Edit::SetUnits(varde_document::LengthUnit::In));
    answer(&mut doc, &requests);
    let texts = screen_texts(&doc);
    assert_eq!(value_after(&texts, "Distance"), "0.3937 in");
}

#[test]
fn a_hole_s_rim_gives_its_radius_and_its_centre_point_snaps() {
    let (mut doc, requests) = example();
    doc.look(Look::StartMeasure);
    let rim = rim(&doc);
    click(&mut doc, move |_| rim, false);
    answer(&mut doc, &requests);
    let texts = screen_texts(&doc);
    assert_eq!(value_after(&texts, "Radius"), "8 mm");
    assert_eq!(value_after(&texts, "Diameter"), "16 mm");
    assert_eq!(value_after(&texts, "Length"), "50.265 mm");
    assert!(
        texts.contains(&"Circular edge of Body 1".to_owned()),
        "{texts:?}"
    );
    // Its highlight is the rim, in the accent: from where the answer
    // found it.
    assert!(!doc.highlight().unwrap().is_empty());

    // The rim's centre, a snap point: a point.
    let Picked::Edge(chain) = rim.target else {
        unreachable!()
    };
    let centre = Pick {
        snap: Some(Snapped::EdgePoint(chain)),
        ..rim
    };
    click(&mut doc, move |_| centre, false);
    answer(&mut doc, &requests);
    let state = doc.measure_state().unwrap();
    let point = state.points[1].unwrap();
    assert!(point.distance(DVec3::new(0.0, 0.0, 10.0)) < 1e-9, "{point}");
    let texts = screen_texts(&doc);
    // From the rim to its centre: the radius.
    assert_eq!(value_after(&texts, "Distance"), "8 mm");
}

#[test]
fn a_corner_is_a_point_with_its_coordinates() {
    let (mut doc, requests) = example();
    doc.look(Look::StartMeasure);
    let top = top(&doc);
    let index = doc.feed.pick_index();
    let (snapped, at) = (index.snaps(top.target).into_iter())
        .find(|(_, at)| *at == DVec3::new(30.0, 20.0, 10.0))
        .unwrap();
    let corner = Pick {
        snap: Some(snapped),
        at: at - DVec3::X,
        ..top
    };
    click(&mut doc, move |_| corner, false);
    let Some(varde_regen::Inspect { first, .. }) = last_inspect(&requests) else {
        panic!("no measure asked");
    };
    assert!(matches!(first.entity, Entity::Corner(_)));
    assert_eq!(first.near, [30.0, 20.0, 10.0]);
    answer(&mut doc, &requests);
    let texts = screen_texts(&doc);
    assert_eq!(value_after(&texts, "X"), "30 mm");
    assert_eq!(value_after(&texts, "Y"), "20 mm");
    assert_eq!(value_after(&texts, "Z"), "10 mm");
    // Drawn as a dot, not highlighted as a face or an edge.
    assert!(doc.measure_state().unwrap().points[0].is_some());
    assert_eq!(doc.highlight(), None);
}

#[test]
fn a_double_click_picks_the_body_its_volume_and_centre() {
    let (mut doc, requests) = example();
    doc.look(Look::StartMeasure);
    let top = top(&doc);
    click(&mut doc, move |_| top, false);
    doc.look(Look::ClickModel {
        pick: Some(top),
        add: false,
        double: true,
    });
    let session = doc.measure.as_ref().unwrap();
    assert_eq!(session.picks[0].unwrap().entity, Entity::Body);
    assert_eq!(session.picks[1], None);
    answer(&mut doc, &requests);
    let Some(Ok(probed)) = doc.probed()[0] else {
        panic!("{:?}", doc.feed.inspected());
    };
    let Ok(Measure::Body { volume, centre, .. }) = probed.measure else {
        panic!("{probed:?}");
    };
    let analytic = 60.0 * 40.0 * 10.0 - PI * 64.0 * 10.0;
    assert!((volume - analytic).abs() < 1e-9 * analytic, "{volume}");
    let centre = DVec3::from(centre.unwrap());
    assert!(
        centre.distance(DVec3::new(0.0, 0.0, 5.0)) < 1e-9,
        "{centre}"
    );
    let texts = screen_texts(&doc);
    assert_eq!(value_after(&texts, "Volume"), "21989.381 mm³");
    assert_eq!(value_after(&texts, "Centre"), "0, 0, 5 mm");
    // All its faces highlighted.
    let index = doc.feed.pick_index();
    let faces = index.body_faces(top.body).count();
    assert!(faces > 2);
    assert!(!doc.highlight().unwrap().is_empty());
    // Its row in Objects picks it too: B with Shift.
    doc.look(Look::ClickBody {
        body: top.body,
        add: true,
    });
    let session = doc.measure.as_ref().unwrap();
    assert_eq!(session.picks[1].unwrap().entity, Entity::Body);
}

#[test]
fn clicks_pick_a_then_b_then_a_again_and_shift_replaces_b() {
    let (mut doc, requests) = example();
    doc.look(Look::StartMeasure);
    let (top, bottom, rim) = (top(&doc), bottom(&doc), rim(&doc));
    let entity =
        |doc: &Doc, slot: usize| doc.measure.as_ref().unwrap().picks[slot].map(|p| p.entity);
    click(&mut doc, move |_| top, false);
    assert!(matches!(entity(&doc, 0), Some(Entity::Face(_))));
    assert_eq!(entity(&doc, 1), None);
    click(&mut doc, move |_| rim, false);
    assert!(matches!(entity(&doc, 1), Some(Entity::Edge(_))));
    // Shift replaces B.
    click(&mut doc, move |_| bottom, true);
    let a = entity(&doc, 0);
    assert!(matches!(entity(&doc, 1), Some(Entity::Face(_))));
    assert_ne!(entity(&doc, 1), a);
    // A third click starts again from A.
    click(&mut doc, move |_| rim, false);
    assert!(matches!(entity(&doc, 0), Some(Entity::Edge(_))));
    assert_eq!(entity(&doc, 1), None);
    // A click off the model lets go of both.
    doc.look(Look::ClickModel {
        pick: None,
        add: false,
        double: false,
    });
    assert_eq!(entity(&doc, 0), None);
    answer(&mut doc, &requests);
    assert_eq!(last_inspect(&requests), None);
}

#[test]
fn new_picks_ask_again_and_only_the_newest_answer_is_taken() {
    let (mut doc, requests) = example();
    doc.look(Look::StartMeasure);
    assert!(
        requests.borrow().is_empty(),
        "nothing picked, nothing asked"
    );
    click(&mut doc, top, false);
    let first = requests.take();
    assert_eq!(first.len(), 1);
    let revision = first[0].inspect().unwrap();
    // The same picks, asked again by any look, ask nothing more.
    doc.look(Look::Hover(None));
    assert!(requests.borrow().is_empty());
    click(&mut doc, bottom, false);
    let second = requests.take();
    assert_eq!(second[0].inspect(), Some(revision + 1));

    // The newer answer first, then the older: the older is dropped.
    let handle = varde_regen::handle;
    doc.computed(handle(second[0].clone()));
    assert!(doc.feed.inspected().unwrap().between.is_some());
    doc.computed(handle(first[0].clone()));
    let inspected = doc.feed.inspected().unwrap();
    assert_eq!(inspected.revision, revision + 1);
    assert!(inspected.between.is_some());

    // The older first: nothing shows for the picks until theirs comes.
    click(&mut doc, rim, false);
    let third = requests.take();
    click(&mut doc, top, true);
    let fourth = requests.take();
    doc.computed(handle(third[0].clone()));
    assert_eq!(doc.feed.inspected(), None);
    assert_eq!(doc.highlight(), None);
    let texts = screen_texts(&doc);
    assert!(texts.contains(&"Measuring…".to_owned()), "{texts:?}");
    doc.computed(handle(fourth[0].clone()));
    assert_eq!(
        doc.feed.inspected().unwrap().revision,
        fourth[0].inspect().unwrap()
    );
}

#[test]
fn picks_are_found_again_after_an_edit_and_one_gone_is_noted_and_kept() {
    let (mut doc, requests) = example();
    doc.look(Look::StartMeasure);
    click(&mut doc, top, false);
    click(&mut doc, bottom, false);
    answer(&mut doc, &requests);
    // A thicker plate: the same picks measure it.
    let feature = doc.editor.document().features()[1].id;
    let Some(varde_document::FeatureKind::Extrude(plate)) = doc
        .editor
        .document()
        .feature(feature)
        .map(|f| f.kind.clone())
    else {
        panic!("the example's second feature is its extrude");
    };
    let ask = varde_document::Extent::ask(&doc.editor.document().design());
    let thicker = varde_document::Extrude {
        extent: varde_document::Extent::OneSide(varde_expr::Value::new("12", &ask).unwrap()),
        ..plate
    };
    doc.apply(varde_document::Command::SetFeature {
        feature,
        kind: Box::new(thicker.into()),
    });
    doc.sync();
    // Until the new model shows, the old one's answer does.
    assert_eq!(value_after(&screen_texts(&doc), "Distance"), "10 mm");
    answer(&mut doc, &requests);
    assert_eq!(value_after(&screen_texts(&doc), "Distance"), "12 mm");

    // A hole cut through the plate, its wall picked as B, and the cut
    // undone: its face is gone, noted, and found again on redo.
    let up_and_down = crate::tests::two_sides(doc.editor.document(), "15", "5");
    let cut = varde_document::Operation::Cut(varde_document::Targets::default());
    crate::tests::add_disc(&mut doc.editor, (20.0, 0.0), up_and_down, cut);
    doc.sync();
    answer(&mut doc, &requests);
    let wall = face(
        &doc,
        DVec3::new(25.0, 0.0, 5.0),
        |summary| matches!(summary, Summary::Cylinder { radius, .. } if *radius == 5.0),
    );
    click(&mut doc, move |_| wall, true);
    answer(&mut doc, &requests);
    // B's own values, unfolded.
    doc.look(Look::Measure(MeasureLook::Fold(MeasureSlot::B)));
    assert_eq!(value_after(&screen_texts(&doc), "Radius"), "5 mm");
    doc.update(Edit::Undo);
    answer(&mut doc, &requests);
    let texts = screen_texts(&doc);
    assert!(texts.contains(&"Face not found".to_owned()), "{texts:?}");
    assert!(doc.measure.as_ref().unwrap().picks[1].is_some(), "kept");
    doc.update(Edit::Redo);
    answer(&mut doc, &requests);
    let texts = screen_texts(&doc);
    assert!(!texts.contains(&"Face not found".to_owned()), "{texts:?}");
}

#[test]
fn esc_leaves_no_trace() {
    let (mut doc, requests) = example();
    // Something selected before, which stays selected.
    doc.look(Look::ClickModel {
        pick: Some(top(&doc)),
        add: false,
        double: false,
    });
    let selection = doc.pick.selection.clone();
    let highlight = doc.highlight().cloned();
    let generation = doc.editor.generation();
    let (edited, undo) = (doc.edited(), doc.editor.can_undo());
    doc.look(Look::StartMeasure);
    // The selection isn't drawn while measuring.
    assert_eq!(doc.highlight(), None);
    click(&mut doc, bottom, false);
    click(&mut doc, rim, false);
    answer(&mut doc, &requests);
    assert!(doc.highlight().is_some());
    doc.look(Look::Escape);
    assert!(doc.measure.is_none());
    assert_eq!(doc.pick.selection, selection);
    assert_eq!(doc.highlight().cloned(), highlight);
    assert_eq!(doc.editor.generation(), generation);
    assert_eq!((doc.edited(), doc.editor.can_undo()), (edited, undo));
    // The model is asked for again without the measure.
    assert_eq!(requests.borrow().len(), 1);
    assert_eq!(last_inspect(&requests), None);
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.inspected(), None);
    let texts = screen_texts(&doc);
    assert!(!texts.contains(&"Close".to_owned()), "{texts:?}");
    assert!(!texts.contains(&"Pick A".to_owned()), "{texts:?}");
}

#[test]
fn the_copy_button_copies_the_value_with_its_unit_at_full_precision() {
    let (mut doc, requests) = example();
    doc.update(Edit::SetUnits(varde_document::LengthUnit::In));
    answer(&mut doc, &requests);
    doc.look(Look::StartMeasure);
    click(&mut doc, top, false);
    click(&mut doc, bottom, false);
    answer(&mut doc, &requests);
    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(
        doc.view(false, Mode::Light, varde_view::ViewOptions::default()),
        size,
        &mut renderer,
    );
    let found = texts(&mut ui, &renderer);
    let distance = found.iter().find(|text| text.text == "0.3937 in").unwrap();
    // The copy button is at the row's right, in the panel's padding.
    let y = distance.bounds.center_y();
    let mut x = distance.bounds.x;
    let sent = loop {
        assert!(x < size.width, "no copy button right of the value");
        let sent = crate::tests::clicked(&mut ui, &mut renderer, iced::Point::new(x, y));
        if sent.iter().any(|message| matches!(message, Ui::Copy(_))) {
            break sent;
        }
        x += 4.0;
    };
    let copied = sent.iter().find_map(|message| match message {
        Ui::Copy(text) => Some(text.clone()),
        _ => None,
    });
    assert_eq!(copied.as_deref(), Some("0.3937007874015748 in"));
}

/// The viewport picks vertices too: a vertex's snap point is its corner,
/// measured as the corner; one picked without it picks nothing.
#[test]
fn a_vertex_snaps_to_its_corner() {
    let (mut doc, requests) = example();
    doc.look(Look::StartMeasure);
    let index = doc.feed.pick_index();
    let at = glam::Vec3::new(30.0, 20.0, 10.0);
    let corner = (index.mesh().corners().iter())
        .position(|&c| glam::Vec3::from(c) == at)
        .unwrap() as u32;
    let target = Picked::Vertex(corner);
    let snaps = index.snaps(target);
    assert_eq!(snaps.len(), 1, "{snaps:?}");
    let (snapped, point) = snaps[0];
    assert_eq!(point, at.as_dvec3());
    let pick = Pick {
        model: index.model(),
        target,
        body: index.body(target).unwrap(),
        at: point,
        snap: None,
    };
    click(&mut doc, move |_| pick, false);
    assert_eq!(doc.measure.as_ref().unwrap().picks[0], None);
    let snapped = Pick {
        snap: Some(snapped),
        ..pick
    };
    click(&mut doc, move |_| snapped, false);
    answer(&mut doc, &requests);
    let Some(Ok(probed)) = doc.probed()[0] else {
        panic!("{:?}", doc.feed.inspected());
    };
    assert_eq!(probed.measure, Ok(Measure::Point([30.0, 20.0, 10.0])));
}

/// The example with a second plate below it, its body.
pub(super) fn two_plates() -> (Doc, std::rc::Rc<RefCell<Vec<Request>>>, BodyId) {
    let (mut doc, requests) = example();
    let document = doc.editor.document();
    let Some(varde_document::FeatureKind::Extrude(plate)) = document
        .features()
        .get(1)
        .map(|feature| feature.kind.clone())
    else {
        panic!("the example's second feature is its extrude");
    };
    let ask = varde_document::Extent::ask(&document.design());
    let below = varde_document::Extrude {
        flip: true,
        extent: varde_document::Extent::OneSide(varde_expr::Value::new("3", &ask).unwrap()),
        ..plate
    };
    doc.apply(document.add_feature(below.into()));
    doc.sync();
    answer(&mut doc, &requests);
    let below = doc.editor.document().bodies().last().unwrap().id;
    (doc, requests, below)
}

/// A body a join merged into another, picked by its row in Objects, is
/// measured and highlighted as the body holding it; and a face of it
/// picked before the join is found on the holder.
#[test]
fn picks_of_a_merged_body_are_of_its_holder() {
    let (mut doc, requests, below) = two_plates();
    let plate = doc.editor.document().bodies()[0].id;
    doc.look(Look::StartMeasure);
    let under = face(
        &doc,
        DVec3::new(-20.0, 15.0, -3.0),
        |summary| matches!(summary, Summary::Plane { n, d } if n[2] < -0.5 && *d == 3.0),
    );
    assert_eq!(under.body, below);
    click(&mut doc, move |_| under, false);
    doc.look(Look::ClickBody {
        body: below,
        add: true,
    });
    let extent = crate::tests::two_sides(doc.editor.document(), "15", "5");
    let join = varde_document::Operation::Join(varde_document::Targets::default());
    crate::tests::add_disc(&mut doc.editor, (20.0, 0.0), extent, join);
    doc.sync();
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.merged_bodies(), [(below, plate)]);
    let [Some(Ok(face)), Some(Ok(body))] = doc.probed() else {
        panic!("{:?}", doc.feed.inspected());
    };
    assert!(matches!(face.at, Some(At::Face(_))), "{face:?}");
    assert!(matches!(body.measure, Ok(Measure::Body { .. })), "{body:?}");
    let index = doc.feed.pick_index();
    let mut faces: Vec<u32> = index.body_faces(plate).collect();
    let highlight = doc.highlight().unwrap();
    let mut second = highlight.second_faces.clone();
    faces.sort_unstable();
    second.sort_unstable();
    assert_eq!(second, faces);
    assert!(!highlight.selected_faces.is_empty());
}

mod fuzz;
