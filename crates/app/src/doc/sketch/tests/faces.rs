//! Sketches on faces: picking a flat face for a new sketch, editing it in
//! the face's frame, extruding from it, its placement following the
//! history before it, and one whose placement failed.

use std::cell::RefCell;
use std::rc::Rc;

use glam::{DVec2, DVec3, Vec3};
use iced::mouse::{Button, Cursor, Event};
use iced::time::Instant;
use varde_document::{
    Command, Document, Editor, Extent, Extrude, FaceRef, FeatureId, FeatureKind, Operation,
    OriginPlane, Placement, Plane,
};
use varde_expr::Value;
use varde_regen::{Request, Summary};
use varde_sketch::Curve;
use varde_view::{Edit, ExtrudeLook, Look, Message as Ui, Mode, OperationKind, Picked, Tool};

use super::{Answered, click};
use crate::doc::{CAMERA_ANIMATION, Doc};
use crate::tests::{SolveLane, answer, deferred, example, key_in, press_in, shown, texts};

type Requests = Rc<RefCell<Vec<Request>>>;

/// The screen the tests click in.
const SIZE: iced::Size = iced::Size::new(1280.0, 800.0);

/// Over the middle of the viewport, the example plate's top shows from
/// Home.
const OVER_TOP: iced::Point = iced::Point::new(780.0, 450.0);

fn letter(c: &str) -> iced::keyboard::Key {
    iced::keyboard::Key::Character(c.into())
}

/// What a left click at `at` over `doc`'s screen sends.
fn click_screen(doc: &Doc, at: iced::Point) -> Vec<Ui> {
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(doc.view_in(Mode::Light), SIZE, &mut renderer);
    let mut sent = Vec::new();
    for event in [
        iced::Event::Mouse(Event::CursorMoved { position: at }),
        iced::Event::Mouse(Event::ButtonPressed(Button::Left)),
        iced::Event::Mouse(Event::ButtonReleased(Button::Left)),
    ] {
        let _ = ui.update(
            &[event],
            Cursor::Available(at),
            &mut renderer,
            &mut iced::advanced::clipboard::Null,
            &mut sent,
        );
    }
    sent
}

/// Takes `sent` as the app would.
fn take(doc: &mut Doc, sent: Vec<Ui>) {
    for message in sent {
        match message {
            Ui::Look(look) => doc.look(look),
            Ui::Edit(edit) => doc.update(edit),
            _ => {}
        }
    }
}

/// The status bar's texts, left to right.
fn status_bar(doc: &Doc) -> Vec<String> {
    let top = SIZE.height - varde_view::STATUS_BAR_ROOM;
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(doc.view_in(Mode::Light), SIZE, &mut renderer);
    let mut bar: Vec<_> = texts(&mut ui, &renderer)
        .into_iter()
        .filter(|text| text.bounds.y >= top && text.bounds.x >= varde_view::SIDE_PANEL_WIDTH)
        .collect();
    bar.sort_by(|a, b| a.bounds.x.total_cmp(&b.bounds.x));
    bar.into_iter().map(|text| text.text).collect()
}

/// The faces of the model shown whose summary `wanted` takes, with a
/// point inside each (its first triangle's middle).
fn faces(doc: &Doc, wanted: impl Fn(&Summary) -> bool) -> Vec<(u32, DVec3)> {
    let index = doc.feed.pick_index();
    let mesh = index.mesh();
    (index.picking().faces().iter().enumerate())
        .filter(|(_, face)| wanted(&face.summary))
        .map(|(face, _)| {
            let first = mesh.face_indices(face).unwrap().start;
            let face = face as u32;
            let corners = &mesh.indices()[first..first + 3];
            let middle = corners
                .iter()
                .map(|&i| Vec3::from(mesh.positions()[i as usize]).as_dvec3())
                .sum::<DVec3>()
                / 3.0;
            (face, middle)
        })
        .collect()
}

/// The reference to the face of the model shown on the plane `n·x = d`.
fn face_on(doc: &Doc, n: DVec3, d: f64) -> FaceRef {
    let on = |summary: &Summary| {
        matches!(*summary, Summary::Plane { n: m, d: e }
            if DVec3::from(m).abs_diff_eq(n, 1e-12) && (e - d).abs() < 1e-9)
    };
    let [(face, near)] = faces(doc, on)[..] else {
        panic!("one face on {n}·x = {d}");
    };
    doc.feed.pick_index().face_ref(face, near).unwrap()
}

/// The sketch feature `id`'s plane.
fn plane(doc: &Doc, id: FeatureId) -> Plane {
    match &doc.editor.document().feature(id).unwrap().kind {
        FeatureKind::Sketch { plane, .. } => *plane,
        _ => panic!("not a sketch"),
    }
}

/// The sketch being edited.
fn edited(doc: &Doc) -> Option<FeatureId> {
    doc.sketch.as_ref().map(|session| session.feature)
}

fn settle_camera(doc: &mut Doc) {
    doc.animation_frame(Instant::now() + 2 * CAMERA_ANIMATION);
}

/// The highest point of the model shown.
fn top_of_mesh(doc: &Doc) -> f32 {
    (doc.feed.mesh().positions().iter())
        .map(|p| p[2])
        .fold(f32::MIN, f32::max)
}

/// Puts a circle of `radius` about `center` in the sketch `id`, as one
/// edit, and answers the model.
fn circle_in(doc: &mut Doc, requests: &Requests, id: FeatureId, center: DVec2, radius: f64) {
    let mut drawn = varde_sketch::Sketch::default();
    let center = drawn.add_point(center).unwrap();
    drawn
        .add_curve(Curve::Circle { center, radius }, false)
        .unwrap();
    doc.apply(Command::SetSketch {
        feature: id,
        sketch: Box::new(drawn),
    });
    doc.sync();
    answer(doc, requests);
}

/// The example, and a sketch on its plate's top, picked at its middle,
/// entered and answered: its id.
fn on_the_top() -> (Doc, FeatureId, Requests) {
    let (mut doc, requests) = example();
    doc.look(Look::PickPlane);
    let sent = click_screen(&doc, OVER_TOP);
    take(&mut doc, sent);
    let id = edited(&doc).expect("a sketch on the top is entered");
    answer(&mut doc, &requests);
    (doc, id, requests)
}

#[test]
fn s_then_a_click_on_the_plates_top_starts_a_sketch_there() {
    let (mut doc, requests) = example();
    let body = doc.editor.document().bodies()[0].id;
    let revision = doc.editor.revision();
    key_in(&mut doc, letter("s"));
    assert!(doc.picking_plane.is_some());
    let bar = status_bar(&doc);
    assert!(bar.iter().any(|t| t.contains("flat face")), "{bar:?}");

    let sent = click_screen(&doc, OVER_TOP);
    let edits: Vec<_> = (sent.iter())
        .filter_map(|message| match message {
            Ui::Edit(edit) => Some(edit),
            _ => None,
        })
        .collect();
    let [Edit::FacePicked(face)] = edits[..] else {
        panic!("{sent:?}");
    };
    assert!(
        !sent
            .iter()
            .any(|message| matches!(message, Ui::Look(Look::ClickModel { .. }))),
        "{sent:?}"
    );
    assert_eq!(face.body, body);
    assert_eq!(face.near.z, 10.0);
    take(&mut doc, sent);
    assert!(doc.picking_plane.is_none());
    let id = edited(&doc).expect("the sketch is entered");
    assert!(matches!(plane(&doc, id), Plane::Face(f) if f.body == body));

    // Placed at once, at the top, its axes the XY plane's: as the answer
    // will place it, to the bit.
    let placement = doc.sketch_state().unwrap().placement;
    let expected = Placement {
        origin: DVec3::new(0.0, 0.0, 10.0),
        ..OriginPlane::XY.placement()
    };
    assert_eq!(placement, expected);
    assert_eq!(doc.placement(id), Some(expected));
    assert_eq!(doc.feed.placement(id, &plane(&doc, id)), None);
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.placement(id, &plane(&doc, id)), Some(expected));
    assert_eq!(doc.placement(id), Some(expected));

    // The camera faces it, its y up on screen.
    settle_camera(&mut doc);
    assert!(doc.camera.backward().abs_diff_eq(Vec3::Z, 1e-5));
    assert!(doc.camera.up().abs_diff_eq(Vec3::Y, 1e-5));
    assert!((doc.camera.target().z - 10.0).abs() < 1e-4);

    // One undo step.
    doc.update(Edit::Undo);
    assert_eq!(edited(&doc), None);
    assert_eq!(doc.editor.revision(), revision);
}

#[test]
fn esc_while_picking_leaves_no_trace() {
    let (mut doc, _requests) = example();
    let revision = doc.editor.revision();
    doc.look(Look::PickPlane);
    doc.look(Look::Escape);
    assert!(doc.picking_plane.is_none());
    assert_eq!(doc.editor.revision(), revision);
    assert_eq!(doc.notice, None);
    // A click once picking ended selects, as before.
    let sent = click_screen(&doc, OVER_TOP);
    assert!(
        (sent.iter()).any(|m| matches!(m, Ui::Look(Look::ClickModel { pick: Some(_), .. }))),
        "{sent:?}"
    );
    // A face click that was on its way when picking ended does nothing.
    let face = face_on(&doc, DVec3::Z, 10.0);
    doc.update(Edit::FacePicked(face));
    assert_eq!(doc.editor.revision(), revision);
}

#[test]
fn a_curved_face_isnt_highlighted_or_sketched_on() {
    let (mut doc, _requests) = example();
    let revision = doc.editor.revision();
    let [(wall, near)] = faces(&doc, |s| matches!(s, Summary::Cylinder { .. }))[..] else {
        panic!("the plate has one hole");
    };
    let body = doc.editor.document().bodies()[0].id;
    doc.look(Look::PickPlane);
    let pick = varde_view::Pick {
        model: doc.feed.model(),
        target: Picked::Face(wall),
        body,
        at: near,
        snap: None,
    };
    doc.look(Look::Hover(Some(pick)));
    assert_eq!(doc.highlight(), None, "a curved face isn't highlighted");
    let bar = status_bar(&doc);
    assert!(bar.iter().any(|t| t == varde_view::CURVED_FACE), "{bar:?}");
    let picking = doc.model_picking().unwrap();
    assert!(picking.planes.is_some() && !picking.takes(pick.target));

    // Asked for anyway, it's refused, and says why.
    let face = doc.feed.pick_index().face_ref(wall, near).unwrap();
    doc.update(Edit::FacePicked(face));
    assert_eq!(doc.editor.revision(), revision);
    assert_eq!(edited(&doc), None);
    assert_eq!(doc.notice.as_deref(), Some(varde_view::CURVED_FACE));
    assert!(doc.picking_plane.is_some(), "picking goes on");
    doc.look(Look::Escape);

    // Selected, `S` says so too.
    doc.look(Look::ClickModel {
        pick: Some(pick),
        add: false,
        double: false,
    });
    assert!(doc.keys().unwrap().face_selected);
    key_in(&mut doc, letter("s"));
    assert_eq!(doc.editor.revision(), revision);
    assert_eq!(doc.notice.as_deref(), Some(varde_view::CURVED_FACE));
    // Anything else asked hides it.
    doc.look(Look::Escape);
    assert_eq!(doc.notice, None);
}

#[test]
fn s_with_a_flat_face_selected_sketches_on_it() {
    let (mut doc, requests) = example();
    let sent = click_screen(&doc, OVER_TOP);
    take(&mut doc, sent);
    assert!(doc.pick.selection.single_face().is_some());
    assert!(matches!(
        press_in(&doc, letter("s")),
        Some(crate::Message::Ui(Ui::Edit(Edit::SketchOnSelection)))
    ));
    // The toolbar and the rail name it.
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(doc.view_in(Mode::Light), SIZE, &mut renderer);
    let labels: Vec<_> = texts(&mut ui, &renderer)
        .into_iter()
        .map(|t| t.text)
        .collect();
    assert!(labels.iter().any(|t| t == "Sketch on face"), "{labels:?}");
    drop(ui);

    key_in(&mut doc, letter("s"));
    let id = edited(&doc).expect("the sketch is entered");
    assert!(matches!(plane(&doc, id), Plane::Face(_)));
    assert_eq!(doc.sketch_state().unwrap().placement.origin.z, 10.0);
    answer(&mut doc, &requests);
    assert_eq!(doc.sketch_state().unwrap().placement.origin.z, 10.0);
}

#[test]
fn a_line_drawn_on_the_top_lands_on_it_in_the_world() {
    let (mut doc, id, requests) = on_the_top();
    let lane = SolveLane::connect(&mut doc);
    let mut doc = Answered { doc, lane };
    doc.look(Look::SelectTool(Tool::Line));
    click(&mut doc, -5.0, 3.0);
    click(&mut doc, 7.0, 3.0);
    doc.look(Look::Escape);
    doc.look(Look::FinishSketch);
    answer(&mut doc, &requests);
    let sketch = edited_lines(&doc, id);
    assert_eq!(sketch, 1, "one line drawn");
    // Drawn at the top, where its sketch coordinates put it.
    let points = doc.feed.sketches().points();
    assert!(!points.is_empty());
    for p in points {
        assert_eq!(p[2], 10.0, "{p:?}");
        assert_eq!(p[1], 3.0, "{p:?}");
        assert!((-5.0..=7.0).contains(&p[0]), "{p:?}");
    }
}

/// How many lines the sketch `id` holds.
fn edited_lines(doc: &Doc, id: FeatureId) -> usize {
    let FeatureKind::Sketch { sketch, .. } = &doc.editor.document().feature(id).unwrap().kind
    else {
        panic!("not a sketch");
    };
    (sketch.curves.iter())
        .filter(|entry| matches!(entry.curve, Curve::Line { .. }))
        .count()
}

#[test]
fn an_extrude_from_the_top_previews_joined_and_follows_the_plate() {
    let (mut doc, id, requests) = on_the_top();
    doc.look(Look::FinishSketch);
    circle_in(&mut doc, &requests, id, DVec2::new(20.0, 10.0), 4.0);
    assert!(doc.extrudable());

    doc.look(Look::SelectFeature(id));
    doc.look(Look::StartExtrude);
    let state = doc.extrude_state().unwrap();
    let [candidate] = &state.candidates[..] else {
        panic!("the sketch on the top is the one candidate");
    };
    assert_eq!(candidate.feature, id);
    assert_eq!(candidate.placement.origin.z, 10.0);
    doc.look(Look::Extrude(ExtrudeLook::PickRegion {
        sketch: id,
        region: 0,
    }));
    doc.look(Look::Extrude(ExtrudeLook::Operation(OperationKind::Join)));
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.draft_error(), None);
    // Joined: the plate's one body, 10 mm higher where the boss is.
    assert_eq!(doc.feed.parts().len(), 1);
    assert_eq!(top_of_mesh(&doc), 20.0);
    key_in(
        &mut doc,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
    );
    assert!(doc.extrude.is_none());
    answer(&mut doc, &requests);
    assert!(doc.feed.failed_features().is_empty());
    assert_eq!(top_of_mesh(&doc), 20.0);

    // The plate made 12 mm thick: the sketch and its boss follow its top.
    let extrude = doc.editor.document().features()[1].id;
    let Some(FeatureKind::Extrude(plate)) = doc
        .editor
        .document()
        .feature(extrude)
        .map(|f| f.kind.clone())
    else {
        panic!("the example's second feature is its extrude");
    };
    let ask = Extent::ask(&doc.editor.document().design());
    let thicker = Extrude {
        extent: Extent::OneSide(Value::new("12", &ask).unwrap()),
        ..plate
    };
    doc.apply(Command::SetFeature {
        feature: extrude,
        kind: Box::new(thicker.into()),
    });
    doc.sync();
    answer(&mut doc, &requests);
    assert_eq!(doc.placement(id).unwrap().origin.z, 12.0);
    assert_eq!(top_of_mesh(&doc), 22.0);
    doc.look(Look::EditFeature(id));
    assert_eq!(edited(&doc), Some(id));
    assert_eq!(doc.sketch_state().unwrap().placement.origin.z, 12.0);
}

/// A prism along Y, 20 mm long about XZ, whose end is the triangle
/// (−10, 0), (10, 0), (0, 10) in XZ: two walls slanted at 45°.
fn prism() -> (Doc, Requests) {
    let mut editor = Editor::new(Document::default());
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XZ)))
        .unwrap();
    let sketch = editor.document().features()[0].id;
    let mut end = varde_sketch::Sketch::default();
    let corners = [(-10.0, 0.0), (10.0, 0.0), (0.0, 10.0)]
        .map(|(x, y)| end.add_point(DVec2::new(x, y)).unwrap());
    for (k, &start) in corners.iter().enumerate() {
        let end_point = corners[(k + 1) % 3];
        end.add_curve(
            Curve::Line {
                start,
                end: end_point,
            },
            false,
        )
        .unwrap();
    }
    let region = end.profiles().unwrap().reference(0).unwrap();
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(end),
        })
        .unwrap();
    let ask = Extent::ask(&editor.document().design());
    let extrude = Extrude {
        sketch,
        regions: vec![region],
        extent: Extent::Symmetric(Value::new("20", &ask).unwrap()),
        flip: false,
        operation: Operation::NewBody(varde_document::BodyId::NEW),
    };
    editor
        .apply(editor.document().add_feature(extrude.into()))
        .unwrap();
    let (mut doc, requests) = deferred();
    doc.apply(Command::Replace(Box::new(editor.document().clone())));
    doc.sync();
    answer(&mut doc, &requests);
    (doc, requests)
}

#[test]
fn a_sketch_on_a_tilted_face_is_placed_up_and_extruded() {
    let (mut doc, requests) = prism();
    // The wall facing +X and up: n = (1, 0, 1)/√2.
    let tilted = |summary: &Summary| matches!(*summary, Summary::Plane { n, .. } if n[0] > 0.5 && n[2] > 0.5);
    let [(wall, near)] = faces(&doc, tilted)[..] else {
        panic!("one wall faces +X and up");
    };
    let Summary::Plane { n, d } = doc.feed.pick_index().picking().faces()[wall as usize].summary
    else {
        unreachable!();
    };
    let face = doc.feed.pick_index().face_ref(wall, near).unwrap();
    doc.look(Look::PickPlane);
    doc.update(Edit::FacePicked(face));
    let id = edited(&doc).expect("the sketch is entered");
    let placement = doc.sketch_state().unwrap().placement;
    assert_eq!(placement, Placement::on_plane(DVec3::from(n), d).unwrap());
    assert!(placement.valid());
    // Up stays up: its y climbs the wall, its x runs along it.
    assert!(placement.y.z > 0.7 && placement.y.x < -0.7, "{placement:?}");
    assert!(placement.x.abs_diff_eq(DVec3::Y, 1e-12), "{placement:?}");
    answer(&mut doc, &requests);
    assert_eq!(
        doc.feed.placement(id, &plane(&doc, id)),
        Some(placement),
        "the same bits"
    );
    settle_camera(&mut doc);
    assert!(
        (doc.camera.backward().as_dvec3()).abs_diff_eq(placement.normal, 1e-5),
        "the camera faces the wall"
    );

    // A boss on the wall, joined: the prism grows out of the wall.
    doc.look(Look::FinishSketch);
    circle_in(&mut doc, &requests, id, DVec2::ZERO, 2.0);
    let before = top_of_mesh(&doc);
    doc.look(Look::SelectFeature(id));
    doc.look(Look::StartExtrude);
    doc.look(Look::Extrude(ExtrudeLook::PickRegion {
        sketch: id,
        region: 0,
    }));
    doc.look(Look::Extrude(ExtrudeLook::Operation(OperationKind::Join)));
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.draft_error(), None);
    assert_eq!(doc.feed.parts().len(), 1);
    // The circle's middle is at the wall's middle point nearest the
    // origin, (2.5, 0, 2.5)·2 = (5, 0, 5); 10 mm out along the normal and
    // 2 mm up its y the boss reaches z = 5 + 10/√2 + 2/√2.
    let expected = 5.0 + 12.0 / 2f64.sqrt();
    assert!(
        (f64::from(top_of_mesh(&doc)) - expected).abs() < 0.05,
        "{} {expected} (was {before})",
        top_of_mesh(&doc)
    );
}

#[test]
fn a_sketch_whose_face_is_gone_fails_and_entering_it_asks_for_a_plane() {
    let (mut doc, id, requests) = on_the_top();
    doc.look(Look::FinishSketch);
    circle_in(&mut doc, &requests, id, DVec2::new(20.0, 10.0), 4.0);
    let drawn = sketch_of(&doc, id);
    // Its face's body goes with the plate's extrude; the sketch stays.
    let extrude = doc.editor.document().features()[1].id;
    doc.apply(Command::RemoveFeature(extrude));
    doc.sync();
    assert!(doc.editor.document().feature(id).is_some());
    answer(&mut doc, &requests);
    assert_eq!(doc.placement(id), None);
    let failed = doc.feed.failed_features();
    let (_, why) = failed.iter().find(|(f, _)| *f == id).expect("it fails");
    let why = why.clone();
    // Not drawn, and nothing to extrude.
    assert!(doc.feed.sketches().points().is_empty());
    assert!(!doc.extrudable());

    // Listed in the Timeline, failing, its note naming the face as it
    // can: its maker and body are gone.
    let state = doc.state(false, Mode::Light, varde_view::ViewOptions::default());
    assert!(state.failed.iter().any(|(f, _)| *f == id));
    drop(state);
    doc.look(Look::SelectPanel(varde_view::Panel::Timeline));
    let shown = all_texts(&doc);
    assert!(shown.iter().any(|t| t == "on a face"), "{shown:?}");

    // Entering it asks for a plane first, saying why.
    let revision = doc.editor.revision();
    doc.look(Look::EditFeature(id));
    assert_eq!(edited(&doc), None);
    assert!(doc.picking_plane.is_some());
    let bar = status_bar(&doc);
    assert!(
        bar.iter()
            .any(|t| t.contains(&why) && t.contains("Sketch 2") && t.contains("Pick a plane")),
        "{bar:?}"
    );
    // `Esc` leaves it as it is.
    doc.look(Look::Escape);
    assert!(doc.picking_plane.is_none());
    assert_eq!(edited(&doc), None);
    assert_eq!(doc.editor.revision(), revision);

    // A plane picked puts it there, one undo step, its drawing kept, and
    // it's entered.
    doc.look(Look::EditFeature(id));
    doc.update(Edit::PlanePicked(OriginPlane::XY));
    assert_eq!(plane(&doc, id), Plane::Origin(OriginPlane::XY));
    assert_eq!(sketch_of(&doc, id), drawn);
    assert_eq!(edited(&doc), Some(id));
    assert_eq!(
        doc.sketch_state().unwrap().placement,
        OriginPlane::XY.placement()
    );
    doc.look(Look::FinishSketch);
    answer(&mut doc, &requests);
    assert!(doc.feed.failed_features().iter().all(|(f, _)| *f != id));
    doc.update(Edit::Undo);
    assert_eq!(doc.editor.revision(), revision);
    assert!(matches!(plane(&doc, id), Plane::Face(_)));

    // Asking for a plane, the plate's extrude undone back: once its
    // model shows the sketch placed, the reason goes.
    answer(&mut doc, &requests);
    doc.look(Look::EditFeature(id));
    assert!(
        doc.picking_plane
            .as_ref()
            .is_some_and(|p| p.pick.failed.is_some())
    );
    doc.update(Edit::Undo);
    assert!(doc.editor.document().feature(extrude).is_some());
    answer(&mut doc, &requests);
    let picking = doc.picking_plane.as_ref().expect("still picking");
    assert_eq!(picking.pick.failed, None);
    let bar = status_bar(&doc);
    assert!(
        bar.iter()
            .any(|t| t == "Pick a plane or a flat face for Sketch 2"),
        "{bar:?}"
    );
}

/// Entering a sketch whose face is gone while a combine is set up ends
/// the combine before asking for a plane: picking a plane never runs
/// beside an operation, whose clicks would take the faces.
#[test]
fn entering_a_sketch_whose_face_is_gone_ends_a_combine() {
    let (mut doc, id, requests) = on_the_top();
    doc.look(Look::FinishSketch);
    circle_in(&mut doc, &requests, id, DVec2::new(20.0, 10.0), 4.0);
    let extrude = doc.editor.document().features()[1].id;
    doc.apply(Command::RemoveFeature(extrude));
    doc.sync();
    answer(&mut doc, &requests);
    assert_eq!(doc.placement(id), None);
    doc.look(Look::StartCombine);
    assert!(doc.combine.is_some());
    doc.look(Look::EditFeature(id));
    assert!(doc.picking_plane.is_some());
    assert!(doc.combine.is_none());
}

/// The sketch feature `id`'s drawing.
fn sketch_of(doc: &Doc, id: FeatureId) -> varde_sketch::Sketch {
    match &doc.editor.document().feature(id).unwrap().kind {
        FeatureKind::Sketch { sketch, .. } => sketch.clone(),
        _ => panic!("not a sketch"),
    }
}

/// Every text on `doc`'s screen.
fn all_texts(doc: &Doc) -> Vec<String> {
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(doc.view_in(Mode::Light), SIZE, &mut renderer);
    (texts(&mut ui, &renderer).into_iter())
        .map(|text| text.text)
        .collect()
}

/// Where the middle of the first text reading `label` is on `doc`'s
/// screen.
fn text_at(doc: &Doc, label: &str) -> iced::Point {
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(doc.view_in(Mode::Light), SIZE, &mut renderer);
    let found = (texts(&mut ui, &renderer).into_iter())
        .find(|text| text.text == label)
        .unwrap_or_else(|| panic!("no {label:?} on the screen"));
    found.bounds.center()
}

mod change;
mod fuzz;

#[test]
fn a_face_of_a_feature_removed_since_the_model_shown_takes_no_sketch() {
    let (mut doc, _requests) = example();
    let top = face_on(&doc, DVec3::Z, 10.0);
    // The plate's extrude removed, its answer not in yet: the plate still
    // shows, but its faces aren't the document's.
    let extrude = doc.editor.document().features()[1].id;
    doc.apply(Command::RemoveFeature(extrude));
    doc.sync();
    let revision = doc.editor.revision();
    key_in(&mut doc, letter("s"));
    assert!(doc.picking_plane.is_some());
    let sent = click_screen(&doc, OVER_TOP);
    assert!(
        !(sent.iter()).any(|m| matches!(m, Ui::Edit(Edit::FacePicked(_)))),
        "{sent:?}"
    );
    doc.update(Edit::FacePicked(top));
    assert_eq!(doc.editor.revision(), revision);
    assert_eq!(doc.notice.as_deref(), Some("That face isn't in the model"));
    assert!(doc.picking_plane.is_some(), "picking goes on");
}
