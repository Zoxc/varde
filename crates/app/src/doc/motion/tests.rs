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
        session
            .fields
            .iter()
            .all(|field| field.value.as_ref().unwrap().value == 0.0)
    );
}
