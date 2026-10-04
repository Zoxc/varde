use std::cell::RefCell;
use std::rc::Rc;

use glam::DVec3;
use iced::keyboard::{self, key};
use varde_document::{Axis3, Document, Editor, Mirror, Operation, PlaneRef};
use varde_regen::{Request, Summary};
use varde_view::{Edit, Look, MotionField, MotionLook, MotionPick, Picked};

use super::*;
use crate::tests::{add_disc, answer, holding, key_in, press_in, screen_texts, two_sides};

type Requests = Rc<RefCell<Vec<Request>>>;

/// The example's plate, "Body 1" (60 × 40 × 10 about the origin, from z 0
/// to 10), and two discs of radius 5 made as new bodies, 15 mm up and 5
/// mm down through it: "Body 2" about (20, 0) and "Body 3" about
/// (-20, 0).
struct Plates {
    doc: Doc,
    requests: Requests,
    bodies: [BodyId; 3],
}

fn plates() -> Plates {
    let mut editor = Editor::new(Document::example());
    let new = || Operation::NewBody(BodyId::NEW);
    for center in [(20.0, 0.0), (-20.0, 0.0)] {
        let extent = two_sides(editor.document(), "15", "5");
        add_disc(&mut editor, center, extent, new());
    }
    let bodies = editor.document().bodies();
    let bodies = [bodies[0].id, bodies[1].id, bodies[2].id];
    let (doc, requests) = holding(editor.document().clone());
    Plates {
        doc,
        requests,
        bodies,
    }
}

impl Plates {
    fn motion(&mut self, message: MotionLook) {
        self.doc.look(Look::Motion(message));
    }

    fn input(&mut self, field: MotionField, text: &str) {
        self.motion(MotionLook::Input {
            field,
            text: text.to_owned(),
        });
    }

    fn answer(&mut self) {
        answer(&mut self.doc, &self.requests);
    }

    /// A click in the viewport on `target` of `body` of the model shown,
    /// at `at`.
    fn click_at(&mut self, body: BodyId, target: Picked, at: DVec3) {
        let pick = varde_view::Pick {
            model: self.doc.feed.pick_index().model(),
            target,
            body,
            at,
            snap: None,
        };
        self.doc.look(Look::ClickModel {
            pick: Some(pick),
            add: false,
            double: false,
        });
    }

    /// A click in the viewport on a face of `body`.
    fn click(&mut self, body: BodyId) {
        let face = (self.doc.feed.pick_index().body_faces(body).next()).expect("a face");
        self.click_at(body, Picked::Face(face), DVec3::ZERO);
    }

    /// The face of `body` in the model shown whose summary `wanted`
    /// takes.
    fn face(&self, body: BodyId, wanted: impl Fn(&Summary) -> bool) -> u32 {
        let index = self.doc.feed.pick_index();
        (index.body_faces(body))
            .find(|&face| wanted(&index.picking().faces()[face as usize].summary))
            .expect("such a face")
    }

    /// The box of `body` in the model shown.
    fn bounds(&self, body: BodyId) -> [DVec3; 2] {
        (self.doc.feed.pick_index().bodies_bounds(&[body])).expect("the body shows")
    }

    /// The draft the last request carries, if any, and the feature it
    /// edits.
    fn last_draft(&self) -> Option<(Option<FeatureId>, FeatureKind)> {
        let requests = self.requests.borrow();
        let Request::Regenerate { draft, .. } = requests.last()? else {
            return None;
        };
        let draft = draft.as_ref()?;
        Some((draft.feature, draft.kind.clone()))
    }

    /// The document's last feature, its id and kind.
    fn last_feature(&self) -> (FeatureId, FeatureKind) {
        let feature = self.doc.editor.document().features().last().unwrap();
        (feature.id, feature.kind.clone())
    }
}

fn character(c: &str) -> keyboard::Key {
    keyboard::Key::Character(c.into())
}

fn enter() -> keyboard::Key {
    keyboard::Key::Named(key::Named::Enter)
}

/// Whether `a` and `b` are within a micrometre of each other.
fn near(a: DVec3, b: DVec3) -> bool {
    a.distance(b) < 1e-3
}

#[test]
fn m_starts_a_move_typing_offsets_moves_the_preview_and_enter_commits_one_undo_step() {
    let mut plates = plates();
    let [_, right, _] = plates.bodies;
    let before = plates.doc.editor.document().clone();
    let [low, high] = plates.bounds(right);
    // The body selected is the one moved.
    plates.click(right);
    key_in(&mut plates.doc, character("m"));
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(session.kind, MotionKind::Move);
    assert_eq!(session.bodies, [right]);
    assert_eq!(session.axis, Some(AxisRef::Origin(Axis3::Z)));
    assert!(plates.doc.picks());
    // Nothing moves yet: not ready, and the status bar asks for it.
    assert!(!plates.doc.motion_ready());
    let shown = screen_texts(&plates.doc);
    for text in [
        "New move",
        "Bodies",
        "Translate",
        "Rotate",
        "Z axis",
        "Angle",
    ] {
        assert!(shown.contains(&text.to_owned()), "{text}: {shown:?}");
    }
    assert!(
        shown.contains(&"· enter a distance or an angle".to_owned()),
        "{shown:?}"
    );
    assert!(plates.last_draft().is_none(), "nothing to preview yet");

    // Typed offsets move the preview.
    plates.input(MotionField::Offset(Axis3::X), "12");
    plates.input(MotionField::Offset(Axis3::Z), "-5");
    assert!(plates.doc.motion_ready());
    let (feature, draft) = plates.last_draft().expect("a draft");
    assert_eq!(feature, None);
    let FeatureKind::Move(moved) = &draft else {
        panic!("a move's draft");
    };
    assert_eq!(moved.offset_vector(), DVec3::new(12.0, 0.0, -5.0));
    assert_eq!(moved.turn, None, "an angle of nothing turns about nothing");
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    let offset = DVec3::new(12.0, 0.0, -5.0);
    let [moved_low, moved_high] = plates.bounds(right);
    assert!(near(moved_low, low + offset) && near(moved_high, high + offset));

    // Enter commits it, one undo step, selected and named.
    key_in(&mut plates.doc, enter());
    assert!(plates.doc.motion.is_none());
    let (id, kind) = plates.last_feature();
    assert_eq!(kind, draft);
    assert_eq!(plates.doc.selected_feature, Some(id));
    assert_eq!(
        plates.doc.editor.document().feature(id).unwrap().name,
        "Move 1"
    );
    plates.answer();
    plates
        .doc
        .look(Look::SelectPanel(varde_view::Panel::Timeline));
    let shown = screen_texts(&plates.doc);
    // The Timeline's note: how far in all.
    assert!(shown.contains(&"13 mm".to_owned()), "{shown:?}");
    // Selected, the status bar says it as the mock's info does.
    assert!(
        shown.contains(&"Body 2 by 12, 0, -5 mm".to_owned()),
        "{shown:?}"
    );
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

#[test]
fn a_move_turns_about_a_round_face_picked_whose_axis_regeneration_finds() {
    let mut plates = plates();
    let [plate, right, _] = plates.bodies;
    plates.doc.look(Look::StartMove);
    plates.click(right);
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [right]);
    // The Axis field picks the axis: the toolbar offers the origin axes.
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    let shown = screen_texts(&plates.doc);
    for axis in ["X axis", "Y axis", "Z axis"] {
        assert!(shown.contains(&axis.to_owned()), "{shown:?}");
    }
    plates.answer();
    // A face that isn't round is refused, saying why.
    let top = plates.face(
        plate,
        |summary| matches!(summary, Summary::Plane { n, .. } if n[2] > 0.5),
    );
    plates.click_at(plate, Picked::Face(top), DVec3::new(0.0, 15.0, 10.0));
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("Only a straight or round edge, or a round face, can be the axis")
    );
    let shown = screen_texts(&plates.doc);
    assert!(
        shown.iter().any(|text| text.contains("can be the axis")),
        "{shown:?}"
    );
    // The disc's wall is: its axis, along Z through (20, 0).
    let wall = plates.face(right, |summary| matches!(summary, Summary::Cylinder { .. }));
    plates.click_at(right, Picked::Face(wall), DVec3::new(25.0, 0.0, 0.0));
    let session = plates.doc.motion.as_ref().unwrap();
    let Some(AxisRef::Face(face)) = session.axis else {
        panic!("a face axis: {:?}", session.axis);
    };
    assert_eq!(face.body, right);
    assert_eq!(session.picking, MotionPick::Bodies);
    // Turning the plate about the disc's axis by a quarter: the preview
    // finds the axis, which the viewport draws.
    plates.click(right);
    plates.click(plate);
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [plate]);
    plates.input(MotionField::Angle, "90");
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    let state = plates.doc.motion_state().unwrap();
    let [point, along] = state.line.expect("the axis found");
    assert!(
        (point.x - 20.0).abs() < 1e-9 && point.y.abs() < 1e-9,
        "{point}"
    );
    assert!(along.normalize().z.abs() > 1.0 - 1e-12, "{along}");
    // The plate, 60 × 40 about the origin, turned a quarter about
    // (20, 0): (x, y) to (20 − y, x − 20), 40 × 60 about (20, −20).
    let [low, high] = plates.bounds(plate);
    assert!(near(low, DVec3::new(0.0, -50.0, 0.0)), "{low}");
    assert!(near(high, DVec3::new(40.0, 10.0, 10.0)), "{high}");
    plates.doc.update(Edit::CommitMotion);
    let (_, kind) = plates.last_feature();
    let FeatureKind::Move(moved) = kind else {
        panic!("a move");
    };
    assert!(matches!(moved.turn, Some((AxisRef::Face(_), _))));
}

#[test]
fn a_mirror_across_a_picked_face_keeps_the_original_and_a_curved_face_is_refused() {
    let mut plates = plates();
    let [plate, right, _] = plates.bodies;
    let before = plates.doc.editor.document().clone();
    plates.doc.look(Look::StartMirror);
    let session = plates.doc.motion.as_ref().unwrap();
    assert_eq!(session.kind, MotionKind::Mirror);
    assert_eq!(session.picking, MotionPick::Bodies);
    assert!(session.keep_original, "Create copy is on to begin with");
    // M doesn't start a move over it: it's not the mirror's key, it
    // swaps it for a move.
    assert!(matches!(
        press_in(&plates.doc, character("m")),
        Some(crate::Message::Ui(varde_view::Message::Look(
            Look::StartMove
        )))
    ));
    plates.click(right);
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    // A new mirror isn't previewed while its plane is picked.
    assert!(plates.last_draft().is_none());
    plates.answer();
    let shown = screen_texts(&plates.doc);
    for text in [
        "New mirror",
        "Plane",
        "Click a plane or face",
        "Create copy",
        "XY plane",
    ] {
        assert!(shown.contains(&text.to_owned()), "{text}: {shown:?}");
    }
    // The disc's wall isn't flat.
    let wall = plates.face(right, |summary| matches!(summary, Summary::Cylinder { .. }));
    plates.click_at(right, Picked::Face(wall), DVec3::new(25.0, 0.0, 0.0));
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("Only a flat face can be the mirror plane")
    );
    assert_eq!(plates.doc.motion.as_ref().unwrap().plane, None);
    // The plate's side at x = 30 is.
    let side = plates.face(plate, |summary| {
        matches!(summary, Summary::Plane { n, d } if n[0] > 0.5 && (d - 30.0).abs() < 1e-9)
    });
    plates.click_at(plate, Picked::Face(side), DVec3::new(30.0, 0.0, 5.0));
    let session = plates.doc.motion.as_ref().unwrap();
    let Some(PlaneRef::Face(face)) = session.plane else {
        panic!("a face plane: {:?}", session.plane);
    };
    assert_eq!(face.body, plate);
    assert!(plates.doc.motion_ready());
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    // The disc and its image across x = 30, side by side: x 15 to 45.
    let [low, high] = plates.bounds(right);
    assert!((low.x - 15.0).abs() < 1e-3 && (high.x - 45.0).abs() < 1e-3);
    // The plane found, drawn through x = 30.
    let [point, normal] = plates.doc.motion_state().unwrap().line.expect("the plane");
    assert!((point.x - 30.0).abs() < 1e-9 && normal.normalize().x > 1.0 - 1e-12);

    plates.doc.update(Edit::CommitMotion);
    let (id, kind) = plates.last_feature();
    assert_eq!(
        kind,
        FeatureKind::Mirror(Mirror {
            bodies: vec![right],
            plane: PlaneRef::Face(face),
            keep_original: true,
        })
    );
    plates.answer();
    plates
        .doc
        .look(Look::SelectPanel(varde_view::Panel::Timeline));
    let shown = screen_texts(&plates.doc);
    // The Timeline's note, the mock's short form, and the status bar's.
    assert!(shown.contains(&"Extrude 1's side".to_owned()), "{shown:?}");
    assert!(
        shown.contains(&"Body 2 across Extrude 1's side · copy".to_owned()),
        "{shown:?}"
    );
    assert!(shown.contains(&"Mirror 1".to_owned()), "{shown:?}");
    assert_eq!(plates.doc.selected_feature, Some(id));
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

#[test]
fn editing_a_move_from_the_timeline_changes_it_as_one_undo_step() {
    let mut plates = plates();
    let [_, right, _] = plates.bodies;
    plates.doc.look(Look::StartMove);
    plates.doc.look(Look::ClickBody {
        body: right,
        add: false,
    });
    plates.input(MotionField::Offset(Axis3::Y), "10");
    plates.doc.update(Edit::CommitMotion);
    plates.answer();
    let (id, first) = plates.last_feature();
    let committed = plates.doc.editor.document().clone();
    let [low, _] = plates.bounds(right);
    assert!((low.y - 5.0).abs() < 1e-3, "{low}");

    // A double-click on its row, or Enter, edits it with its values.
    plates.doc.look(Look::SelectFeature(id));
    key_in(&mut plates.doc, enter());
    let session = plates.doc.motion.as_ref().expect("editing it");
    assert_eq!(session.feature, Some(id));
    assert_eq!(session.bodies, [right]);
    let shown = screen_texts(&plates.doc);
    assert!(shown.contains(&"Move 1".to_owned()), "{shown:?}");
    assert_eq!(session.fields[1].text, "10 mm");

    // Picking the axis shows the bodies where the move finds them: the
    // edited move's preview leaves them be.
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    let (feature, draft) = plates.last_draft().expect("a draft");
    assert_eq!(feature, Some(id));
    let FeatureKind::Move(neutral) = draft else {
        panic!("a move");
    };
    assert_eq!(neutral.offset_vector(), DVec3::ZERO);
    plates.answer();
    let [low, _] = plates.bounds(right);
    assert!((low.y + 5.0).abs() < 1e-3, "{low}");
    // An origin axis from the toolbar hands the clicks back.
    plates.motion(MotionLook::OriginAxis(Axis3::X));
    assert_eq!(
        plates.doc.motion.as_ref().unwrap().picking,
        MotionPick::Bodies
    );

    plates.input(MotionField::Offset(Axis3::Y), "20");
    plates.answer();
    let [low, _] = plates.bounds(right);
    assert!((low.y - 15.0).abs() < 1e-3, "{low}");
    plates.doc.update(Edit::CommitMotion);
    assert!(plates.doc.motion.is_none());
    let (same, changed) = plates.last_feature();
    assert_eq!(same, id);
    assert_ne!(changed, first);
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), committed);

    // Esc leaves an edit without a trace.
    plates.doc.look(Look::EditFeature(id));
    plates.input(MotionField::Offset(Axis3::Y), "30");
    plates.doc.look(Look::Escape);
    assert!(plates.doc.motion.is_none());
    assert_eq!(*plates.doc.editor.document(), committed);
}

#[test]
fn refusals_show_in_the_panel_and_ok_waits() {
    let mut plates = plates();
    let [_, right, _] = plates.bodies;
    plates.doc.look(Look::StartMove);
    plates.click(right);
    // A field's text out of range is refused under it.
    plates.input(MotionField::Offset(Axis3::X), "2000000");
    assert!(!plates.doc.motion_ready());
    let state = plates.doc.motion_state().unwrap();
    assert!(state.fields[0].error.is_some());
    // Within range, but taking the disc out of range: the preview fails,
    // saying so, and OK waits for Add anyway.
    plates.input(MotionField::Offset(Axis3::X), "999990");
    assert!(plates.doc.motion_ready());
    plates.answer();
    let error = plates.doc.feed.draft_error().expect("the preview fails");
    assert!(error.contains("out of range"), "{error}");
    let shown = screen_texts(&plates.doc);
    assert!(shown.contains(&"Move fails".to_owned()), "{shown:?}");
    assert!(shown.contains(&"Add anyway".to_owned()), "{shown:?}");
    let state = plates.doc.motion_state().unwrap();
    assert!(!state.ready && state.accept);
    let features = plates.doc.editor.document().features().len();
    key_in(&mut plates.doc, enter());
    assert_eq!(plates.doc.editor.document().features().len(), features);
    // Add anyway keeps it, failing.
    plates.doc.update(Edit::AcceptError);
    assert_eq!(plates.doc.editor.document().features().len(), features + 1);
    plates.answer();
    let (id, _) = plates.last_feature();
    assert!(
        (plates.doc.feed.failed_features().iter()).any(|failed| failed.feature == id),
        "the move fails"
    );
}

#[test]
fn moves_and_mirrors_need_a_body_and_an_editable_document_outside_sketches() {
    let (mut doc, _requests) = holding(Document::default());
    // No body: M does nothing.
    assert!(press_in(&doc, character("m")).is_none());
    doc.look(Look::StartMove);
    // Look::StartMove still opens it (the binding is what's disabled).
    assert!(doc.motion.is_some());
    doc.look(Look::StartMove);
    assert!(doc.motion.is_none(), "again backs out of it");

    let mut plates = plates();
    plates.doc.read_only = Some("read-only".to_owned());
    plates.doc.look(Look::StartMove);
    assert!(plates.doc.motion.is_none());
}

#[test]
fn a_straight_edge_is_an_axis_and_a_face_made_after_the_move_is_refused() {
    let mut plates = plates();
    let [plate, right, _] = plates.bodies;
    plates.doc.look(Look::StartMove);
    plates.click(right);
    plates.input(MotionField::Offset(Axis3::Y), "10");
    plates.doc.update(Edit::CommitMotion);
    plates.answer();
    let (id, _) = plates.last_feature();
    // A disc made after the move.
    let extent = two_sides(plates.doc.editor.document(), "5", "1");
    add_disc(
        &mut plates.doc.editor,
        (0.0, 30.0),
        extent,
        Operation::NewBody(BodyId::NEW),
    );
    plates.doc.sync();
    plates.answer();
    let later = plates.doc.editor.document().bodies().last().unwrap().id;

    plates.doc.look(Look::EditFeature(id));
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    plates.answer();
    let wall = plates.face(later, |summary| matches!(summary, Summary::Cylinder { .. }));
    plates.click_at(later, Picked::Face(wall), DVec3::new(5.0, 30.0, 2.0));
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("Only a face made before the move can be picked")
    );
    assert_eq!(
        plates.doc.motion.as_ref().unwrap().axis,
        Some(AxisRef::Origin(Axis3::Z))
    );

    // A round edge, the later disc's rim, is refused for being later;
    // the right disc's rim is taken, as a round edge.
    let index = plates.doc.feed.pick_index();
    let rim = |body: BodyId| {
        (0..index.mesh().edge_count() as u32)
            .filter(|&edge| index.body(Picked::Edge(edge)) == Some(body))
            .find(|&edge| super::round_edge(index, edge))
            .expect("a rim")
    };
    let (later_rim, right_rim) = (rim(later), rim(right));
    plates.click_at(later, Picked::Edge(later_rim), DVec3::new(5.0, 30.0, 5.0));
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("Only an edge made before the move can be picked")
    );
    plates.click_at(right, Picked::Edge(right_rim), DVec3::new(25.0, 10.0, 15.0));
    let session = plates.doc.motion.as_ref().unwrap();
    assert!(
        matches!(session.axis, Some(AxisRef::Edge(found)) if found.body == right),
        "{:?}",
        session.axis
    );
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    plates.answer();

    // A straight edge of the plate: the line through it.
    let index = plates.doc.feed.pick_index();
    let (edge, ends) = (0..index.mesh().edge_count() as u32)
        .filter(|&edge| index.body(Picked::Edge(edge)) == Some(plate))
        .find_map(|edge| {
            let keys = index.chain_keys(edge)?;
            Some((edge, index.edge_ends(edge, &keys)?))
        })
        .expect("a straight edge of the plate");
    let middle = (ends[0] + ends[1]) / 2.0;
    plates.click_at(plate, Picked::Edge(edge), middle);
    let session = plates.doc.motion.as_ref().unwrap();
    assert!(
        matches!(session.axis, Some(AxisRef::Edge(found)) if found.body == plate),
        "{:?}",
        session.axis
    );
    let shown = screen_texts(&plates.doc);
    assert!(shown.contains(&"Edge of Body 1".to_owned()), "{shown:?}");
    plates.input(MotionField::Angle, "90");
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    let [point, along] = plates.doc.motion_state().unwrap().line.expect("the axis");
    let edge = ends[1] - ends[0];
    assert!(
        along.normalize().dot(edge.normalize()) > 1.0 - 1e-6,
        "{along} {edge}"
    );
    assert!((point - ends[0]).cross(edge).length() < 1e-6 * edge.length_squared());
}

#[test]
fn the_handles_set_the_offsets_and_a_ring_the_turn_committed_as_one_undo_step() {
    let mut plates = plates();
    let [_, right, _] = plates.bodies;
    let before = plates.doc.editor.document().clone();
    plates.click(right);
    key_in(&mut plates.doc, character("m"));
    // The handles stand at the body's box centre, about world axes.
    let state = plates.doc.motion_state().expect("a move");
    assert_eq!(state.bounds, Some(plates.bounds(right)));
    assert_eq!(state.origin_axis, Some(Axis3::Z));
    assert_eq!(state.units, varde_expr::LengthUnit::Mm);

    // The Z arrow dragged: the viewport types the snapped offset in.
    plates.motion(MotionLook::Input {
        field: MotionField::Offset(Axis3::Z),
        text: "6 mm".to_owned(),
    });
    let (_, draft) = plates.last_draft().expect("a draft");
    let FeatureKind::Move(moved) = &draft else {
        panic!("a move's draft");
    };
    assert_eq!(moved.offset_vector(), DVec3::new(0.0, 0.0, 6.0));
    plates.answer();
    let [low, high] = plates.bounds(right);
    let centre = (low + high) / 2.0;
    let state = plates.doc.motion_state().expect("a move");
    assert_eq!(state.bounds, Some([low, high]));

    // The Y ring dragged a quarter turn back: turned about the Y axis,
    // and shifted so the centre stays, as the viewport works it out.
    let shift = DVec3::new(centre.x + centre.z - 6.0, 0.0, centre.z - centre.x);
    let units = Some(varde_expr::Unit::Length(varde_expr::LengthUnit::Mm));
    plates.motion(MotionLook::Turn {
        axis: Axis3::Y,
        angle: "-90°".to_owned(),
        offset: [shift.x, shift.y, shift.z].map(|v| varde_expr::format(v, units)),
    });
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(session.axis, Some(AxisRef::Origin(Axis3::Y)));
    let (_, draft) = plates.last_draft().expect("a draft");
    let FeatureKind::Move(moved) = &draft else {
        panic!("a move's draft");
    };
    let (axis, angle) = moved.turn.as_ref().expect("a turn");
    assert_eq!(*axis, AxisRef::Origin(Axis3::Y));
    assert!((angle.value + std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert!(near(moved.offset_vector(), shift));
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    let [low, high] = plates.bounds(right);
    assert!(near((low + high) / 2.0, centre), "turned about its centre");
    // The disc's axis now runs along X.
    assert!(high.x - low.x > 19.0 && high.z - low.z < 11.0);

    // OK commits it, one undo step.
    assert!(plates.doc.motion_ready());
    plates.doc.update(Edit::CommitMotion);
    assert!(plates.doc.motion.is_none());
    assert_eq!(plates.last_feature().1, draft);
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

#[test]
fn a_ring_s_turn_does_nothing_to_a_mirror() {
    let mut plates = plates();
    let [_, right, _] = plates.bodies;
    plates.click(right);
    plates.doc.start_motion(MotionKind::Mirror);
    let turn = MotionLook::Turn {
        axis: Axis3::X,
        angle: "90°".to_owned(),
        offset: ["1 mm", "2 mm", "3 mm"].map(str::to_owned),
    };
    plates.motion(turn);
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(session.axis, None);
    assert!(
        session.fields[..4]
            .iter()
            .all(|field| field.value.as_ref().unwrap().value == 0.0)
    );
}

/// A ring's turn is taken only as the handles offer it: from a move
/// turning by nothing, or about that world axis already, while its
/// bodies are picked; the offsets it brings are worked out from such a
/// turn, and would be wrong from any other.
#[test]
fn a_ring_s_turn_is_taken_only_as_the_handles_offer_it() {
    let mut plates = plates();
    let [_, right, _] = plates.bodies;
    plates.click(right);
    key_in(&mut plates.doc, character("m"));
    let turn = |axis: Axis3, angle: &str, x: &str| MotionLook::Turn {
        axis,
        angle: angle.to_owned(),
        offset: [x, "0 mm", "0 mm"].map(str::to_owned),
    };
    let state = |plates: &Plates| {
        let session = plates.doc.motion.as_ref().expect("a session");
        let angle = session.fields[MotionField::Angle.index()].value.clone();
        let x = session.fields[MotionField::Offset(Axis3::X).index()]
            .value
            .clone();
        (session.axis, angle.unwrap().value, x.unwrap().value)
    };
    // From no turn, about Y.
    plates.motion(turn(Axis3::Y, "-90°", "1 mm"));
    let (axis, angle, x) = state(&plates);
    assert_eq!(axis, Some(AxisRef::Origin(Axis3::Y)));
    assert!((angle + std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert_eq!(x, 1.0);
    // About another axis while turning about Y: dropped.
    plates.motion(turn(Axis3::Z, "45°", "2 mm"));
    assert_eq!(state(&plates).0, Some(AxisRef::Origin(Axis3::Y)));
    assert_eq!(state(&plates).2, 1.0);
    // On about Y: taken.
    plates.motion(turn(Axis3::Y, "-45°", "3 mm"));
    assert_eq!(state(&plates).2, 3.0);
    // While the axis is picked, none: the handles aren't there.
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    plates.motion(turn(Axis3::Y, "-30°", "4 mm"));
    assert_eq!(state(&plates).2, 3.0);
}

/// [`plates`] with a fourth body, a disc of radius 5 about (20, 30), 2
/// mm up from XY: the right disc and it together aren't symmetric about
/// their box's centre, which a turn other than a quarter moves.
fn lopsided() -> (Plates, [BodyId; 2]) {
    let mut editor = Editor::new(Document::example());
    let new = || Operation::NewBody(BodyId::NEW);
    for center in [(20.0, 0.0), (-20.0, 0.0)] {
        let extent = two_sides(editor.document(), "15", "5");
        add_disc(&mut editor, center, extent, new());
    }
    let extent = varde_document::Extent::OneSide(crate::tests::length(editor.document(), "2"));
    add_disc(&mut editor, (20.0, 30.0), extent, new());
    let all = editor.document().bodies();
    let bodies = [all[0].id, all[1].id, all[2].id];
    let low = all[3].id;
    let (doc, requests) = holding(editor.document().clone());
    let plates = Plates {
        doc,
        requests,
        bodies,
    };
    let right = plates.bodies[1];
    (plates, [right, low])
}

/// Where the viewport stands the move's handles while none is dragged.
fn handles_at(doc: &Doc) -> DVec3 {
    let state = doc.motion_state().expect("a move");
    let [low, high] = state.bounds.expect("bodies shown");
    state.centre.unwrap_or((low + high) / 2.0)
}

/// What a ring of the handles at `centre` sends turning about `axis`
/// to `angle` degrees from `from` degrees, the offsets `offset`.
fn ring(axis: Axis3, centre: DVec3, offset: DVec3, from: f64, angle: f64) -> MotionLook {
    let shift = varde_kernel::Motion::turn(centre, axis.direction(), angle - from)
        .unwrap()
        .point(offset);
    let units = Some(varde_expr::Unit::Length(varde_expr::LengthUnit::Mm));
    MotionLook::Turn {
        axis,
        angle: format!("{angle}°"),
        offset: [shift.x, shift.y, shift.z].map(|v| varde_expr::format(v, units)),
    }
}

/// The offsets of the move being set up, as they read.
fn offsets(doc: &Doc) -> DVec3 {
    let session = doc.motion.as_ref().expect("a session");
    let [x, y, z] = Axis3::ALL.map(|axis| {
        (session.fields[MotionField::Offset(axis).index()].value)
            .as_ref()
            .unwrap()
            .value
    });
    DVec3::new(x, y, z)
}

/// The handles stay where a ring turned the bodies about once it's let
/// go and the preview comes, though the bodies' box centre moves; a
/// second ring turned about them back to no turn brings the offsets back
/// to nothing, and an arrow then shifts the handles with the bodies.
#[test]
fn the_handles_stay_where_a_ring_turned_them_and_turning_back_undoes_it() {
    let (mut plates, [right, low]) = lopsided();
    plates.doc.look(Look::StartMove);
    plates.click(right);
    plates.click(low);
    plates.answer();
    let start = handles_at(&plates.doc);
    let [a, b] = plates
        .doc
        .feed
        .pick_index()
        .bodies_bounds(&[right, low])
        .unwrap();
    assert!(near(start, (a + b) / 2.0), "{start}");

    plates.motion(ring(Axis3::X, start, DVec3::ZERO, 0.0, 30.0));
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    let [a, b] = plates
        .doc
        .feed
        .pick_index()
        .bodies_bounds(&[right, low])
        .unwrap();
    assert!(!near((a + b) / 2.0, start), "the box's centre moved");
    let after = handles_at(&plates.doc);
    assert!(near(after, start), "the handles jumped: {start} to {after}");

    // Back to no turn about where they stand: no offset left.
    let offset = offsets(&plates.doc);
    plates.motion(ring(Axis3::X, after, offset, 30.0, 0.0));
    assert_eq!(offsets(&plates.doc), DVec3::ZERO);
    plates.answer();
    let [c, d] = plates
        .doc
        .feed
        .pick_index()
        .bodies_bounds(&[right, low])
        .unwrap();
    assert!(near((c + d) / 2.0, start));
    assert!(near(handles_at(&plates.doc), start));

    // Turned again, then an arrow: the handles move with the offset at
    // once, before the preview comes.
    plates.motion(ring(Axis3::X, start, DVec3::ZERO, 0.0, 45.0));
    plates.answer();
    let turned = handles_at(&plates.doc);
    assert!(near(turned, start));
    let y = offsets(&plates.doc).y;
    plates.input(MotionField::Offset(Axis3::Y), &format!("{} mm", y + 10.0));
    let shifted = handles_at(&plates.doc);
    assert!(
        near(shifted, start + DVec3::new(0.0, 10.0, 0.0)),
        "{shifted}"
    );
    plates.answer();
    assert!(near(handles_at(&plates.doc), shifted));
}

/// A move turned about a model edge, its angle typed back to 0, then a
/// ring dragged: the ring's world axis takes over from the edge, the
/// bodies turn about the handles, which stay where they were.
#[test]
fn a_ring_after_a_turn_about_an_edge_typed_back_to_nothing() {
    let (mut plates, [right, low]) = lopsided();
    plates.doc.look(Look::StartMove);
    plates.click(right);
    plates.click(low);
    plates.answer();
    let start = handles_at(&plates.doc);
    // About the right disc's rim, a third of a turn.
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    plates.answer();
    let index = plates.doc.feed.pick_index();
    let rim = (0..index.mesh().edge_count() as u32)
        .filter(|&edge| index.body(Picked::Edge(edge)) == Some(right))
        .find(|&edge| super::round_edge(index, edge))
        .expect("a rim");
    plates.click_at(right, Picked::Edge(rim), DVec3::new(25.0, 0.0, 15.0));
    assert!(matches!(
        plates.doc.motion.as_ref().unwrap().axis,
        Some(AxisRef::Edge(_))
    ));
    plates.input(MotionField::Angle, "120");
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    let [point, along] = plates
        .doc
        .motion_state()
        .unwrap()
        .line
        .expect("the rim's axis");
    let about = varde_kernel::Motion::turn(point, along, 120.0).unwrap();
    assert!(near(handles_at(&plates.doc), about.point(start)));

    // Typed back to nothing, the bodies are back, and the handles.
    plates.input(MotionField::Angle, "0");
    assert!(near(handles_at(&plates.doc), start));
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    assert!(near(handles_at(&plates.doc), start));
    let state = plates.doc.motion_state().unwrap();
    assert_eq!(state.line, None, "turning by nothing, no axis drawn");

    // A ring: about its world axis now, turning about the handles.
    plates.motion(ring(Axis3::X, start, DVec3::ZERO, 0.0, 30.0));
    let session = plates.doc.motion.as_ref().unwrap();
    assert_eq!(session.axis, Some(AxisRef::Origin(Axis3::X)));
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    assert!(near(handles_at(&plates.doc), start));
    assert!(plates.doc.motion_ready());
}

/// With nothing selected, a new move or mirror of a model of one body
/// picks that body, as the UI mock's: a mirror goes on to its plane. Of
/// several, none.
#[test]
fn the_only_body_is_picked_for_a_new_move_or_mirror() {
    let (mut doc, _requests) = holding(Document::example());
    let plate = doc.editor.document().bodies()[0].id;
    doc.start_motion(MotionKind::Move);
    let session = doc.motion.as_ref().expect("a move");
    assert_eq!(session.bodies, [plate]);
    assert_eq!(session.picking, MotionPick::Bodies);
    doc.start_motion(MotionKind::Mirror);
    let session = doc.motion.as_ref().expect("a mirror");
    assert_eq!(session.bodies, [plate]);
    assert_eq!(session.picking, MotionPick::Reference);

    let mut plates = plates();
    plates.doc.start_motion(MotionKind::Move);
    assert!(plates.doc.motion.as_ref().unwrap().bodies.is_empty());
}

/// Adds a disc of radius 5 about (0, 30), 5 mm up and 1 down, as a new
/// body, as its own undo step: the body.
fn later_disc(plates: &mut Plates) -> BodyId {
    let extent = two_sides(plates.doc.editor.document(), "5", "1");
    add_disc(
        &mut plates.doc.editor,
        (0.0, 30.0),
        extent,
        Operation::NewBody(BodyId::NEW),
    );
    plates.doc.sync();
    plates.answer();
    plates.doc.editor.document().bodies().last().unwrap().id
}

/// A picked body an undo takes away stays listed, as the mock's
/// "Missing body", the panel saying it's gone, nothing previewed or
/// committed; the redo brings it back. Taken out, the move goes on.
#[test]
fn a_picked_body_an_undo_takes_away_is_said_to_be_gone() {
    let mut plates = plates();
    let [_, right, _] = plates.bodies;
    let later = later_disc(&mut plates);
    plates.doc.look(Look::StartMove);
    plates.click(right);
    plates.click(later);
    plates.input(MotionField::Offset(Axis3::X), "5");
    plates.answer();
    assert!(plates.doc.motion_ready());

    plates.doc.update(Edit::Undo);
    assert!(plates.last_draft().is_none(), "nothing previewed");
    plates.answer();
    let session = plates.doc.motion.as_ref().expect("still set up");
    assert_eq!(session.bodies, [right, later]);
    assert!(!plates.doc.motion_ready());
    let shown = screen_texts(&plates.doc);
    assert!(shown.contains(&"Missing body".to_owned()), "{shown:?}");
    assert!(
        shown
            .iter()
            .any(|text| text.contains("A picked body is gone")),
        "{shown:?}"
    );
    plates.doc.update(Edit::CommitMotion);
    assert!(plates.doc.motion.is_some(), "not committed");

    // Back with a redo.
    plates.doc.update(Edit::Redo);
    assert!(plates.last_draft().is_some());
    plates.answer();
    assert!(plates.doc.motion_ready());
    // Gone again, and taken out: the rest moves.
    plates.doc.update(Edit::Undo);
    plates.motion(MotionLook::Drop(later));
    assert!(plates.doc.motion_ready());
    plates.doc.update(Edit::CommitMotion);
    let (_, kind) = plates.last_feature();
    let FeatureKind::Move(moved) = kind else {
        panic!("a move");
    };
    assert_eq!(moved.bodies, [right]);
}

/// A move's axis on a body an undo takes away is said to be gone while
/// the move turns, as the mock's "The axis is gone: pick another", and
/// another axis picked clears it; a mirror's plane likewise.
#[test]
fn an_axis_or_plane_an_undo_takes_away_is_said_to_be_gone() {
    let mut plates = plates();
    let [plate, right, _] = plates.bodies;
    let later = later_disc(&mut plates);
    plates.doc.look(Look::StartMove);
    plates.click(right);
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    plates.answer();
    let wall = plates.face(later, |summary| matches!(summary, Summary::Cylinder { .. }));
    plates.click_at(later, Picked::Face(wall), DVec3::new(5.0, 30.0, 2.0));
    let axis = plates.doc.motion.as_ref().unwrap().axis;
    assert!(matches!(axis, Some(AxisRef::Face(face)) if face.body == later));
    plates.input(MotionField::Angle, "30");
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    assert!(plates.doc.motion_ready());

    plates.doc.update(Edit::Undo);
    assert!(plates.last_draft().is_none(), "nothing previewed");
    plates.answer();
    let session = plates.doc.motion.as_ref().expect("still set up");
    assert_eq!(session.axis, axis, "kept, for a redo");
    assert!(!plates.doc.motion_ready());
    let shown = screen_texts(&plates.doc);
    assert!(
        shown
            .iter()
            .any(|text| text.contains("The axis is gone: pick another")),
        "{shown:?}"
    );
    // Turning by nothing, the axis isn't needed.
    plates.input(MotionField::Angle, "0");
    plates.input(MotionField::Offset(Axis3::X), "5");
    assert!(plates.doc.motion_ready());
    plates.input(MotionField::Angle, "30");
    assert!(!plates.doc.motion_ready());
    // Another axis picked: it goes on.
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    plates.motion(MotionLook::OriginAxis(Axis3::Y));
    assert!(plates.doc.motion_ready());
    assert!(plates.last_draft().is_some());
    plates.motion(MotionLook::Cancel);

    // A mirror's plane, a face of the disc, likewise.
    plates.doc.update(Edit::Redo);
    plates.answer();
    plates.click(plate);
    plates.doc.start_motion(MotionKind::Mirror);
    plates.answer();
    let top = plates.face(
        later,
        |summary| matches!(summary, Summary::Plane { n, .. } if n[2] > 0.5),
    );
    plates.click_at(later, Picked::Face(top), DVec3::new(0.0, 30.0, 5.0));
    assert!(matches!(
        plates.doc.motion.as_ref().unwrap().plane,
        Some(PlaneRef::Face(_))
    ));
    assert!(plates.doc.motion_ready());
    plates.doc.update(Edit::Undo);
    plates.answer();
    assert!(!plates.doc.motion_ready());
    let shown = screen_texts(&plates.doc);
    assert!(
        shown
            .iter()
            .any(|text| text.contains("The plane is gone: pick another")),
        "{shown:?}"
    );
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    plates.motion(MotionLook::OriginPlane(varde_document::OriginPlane::XY));
    assert!(plates.doc.motion_ready());
}

/// A move set up with two bodies that a combine, brought back by a redo,
/// merges one into the other: the move follows on to the body holding
/// it, as a click on it would pick, its handles at that body's centre,
/// and moves it whole.
#[test]
fn a_move_follows_a_body_a_redone_combine_merges() {
    let mut plates = plates();
    let [plate, right, _] = plates.bodies;
    let combine = varde_document::Combine {
        target: plate,
        tools: vec![right],
        op: varde_document::BodyOp::Union,
        keep_tools: false,
    };
    let add = plates.doc.editor.document().add_feature(combine.into());
    plates.doc.apply(add);
    plates.doc.sync();
    plates.answer();
    let [low, high] = plates.bounds(plate);
    plates.doc.update(Edit::Undo);
    plates.answer();

    plates.doc.look(Look::StartMove);
    plates.click(plate);
    plates.click(right);
    plates.input(MotionField::Offset(Axis3::Z), "20");
    plates.answer();
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [plate, right]);

    plates.doc.update(Edit::Redo);
    plates.answer();
    let session = plates.doc.motion.as_ref().expect("still set up");
    assert_eq!(
        session.bodies,
        [plate],
        "the tool followed on to the target"
    );
    // A click on what was the tool picks the target, which it's in.
    plates.click(plate);
    assert!(plates.doc.motion.as_ref().unwrap().bodies.is_empty());
    plates.click_at(
        plate,
        Picked::Face(plates.face(plate, |summary| matches!(summary, Summary::Cylinder { .. }))),
        DVec3::new(25.0, 0.0, 12.0),
    );
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [plate]);
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    let [moved_low, moved_high] = plates.bounds(plate);
    let up = DVec3::new(0.0, 0.0, 20.0);
    assert!(near(moved_low, low + up) && near(moved_high, high + up));
    assert!(near(
        handles_at(&plates.doc),
        (moved_low + moved_high) / 2.0
    ));
    assert!(plates.doc.motion_ready());
    plates.doc.update(Edit::CommitMotion);
    let (_, kind) = plates.last_feature();
    let FeatureKind::Move(moved) = kind else {
        panic!("a move");
    };
    assert_eq!(moved.bodies, [plate]);
}

mod align;
mod chamfer;
mod face_draft;
mod face_session;
mod fillet;
mod knobs;
mod loft;
mod offset_face;
mod overlaps;
mod pattern;
mod scale;
mod shell;
mod split;
mod sweep;
mod unchanged;

/// `Esc` cancels a move once, though its four fields each send the
/// cancel: the first takes the key.
#[test]
fn escape_cancels_a_move_once() {
    use crate::tests::{pressed, typing};
    use varde_view::Message as Ui;
    let (mut doc, _requests) = holding(Document::example());
    doc.start_motion(MotionKind::Move);
    let escape = keyboard::Key::Named(key::Named::Escape);
    for focused in [true, false] {
        let (sent, shortcuts) = pressed(&doc, &[typing(escape.clone(), None)], focused);
        assert!(
            matches!(sent[..], [Ui::Look(Look::Motion(MotionLook::Cancel))]),
            "{sent:?}"
        );
        assert!(shortcuts.is_empty(), "{shortcuts:?}");
    }
}
