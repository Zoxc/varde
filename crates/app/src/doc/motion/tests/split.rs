//! The split session: the rail's Split body on the example plate, by XY
//! from the toolbar, failing as too complex (the kernel's split isn't
//! built) and kept by Add anyway; a flat face as its plane and a curved
//! one as its surface; another body as the tool, with regeneration
//! splitting by two booleans ([`varde_regen::testing`]): the pieces
//! tinted apart and labelled, which keeps the id, a trim; regions of a
//! sketch and an open line; editing from the Timeline, Cancel and undo;
//! the later features' warning and a new body a later feature names
//! held; a tool an undo takes away said to be gone; faces of an edited
//! split's new body named on the body split.

use glam::DVec3;
use varde_document::{
    BodyId, Command, Document, Editor, FeatureId, FeatureKind, Keep, Move, OriginPlane, Plane,
    PlaneRef, Side, Split, SplitTool,
};
use varde_regen::Summary;
use varde_sketch::{Curve, Id};
use varde_view::{Edit, Look, MotionKind, MotionLook, MotionPick, Picked, SplitMode};

use super::{Plates, enter, later_disc, near, plates};
use crate::tests::{holding, key_in, screen_texts};

fn shows(plates: &Plates, wanted: &str) -> bool {
    screen_texts(&plates.doc)
        .iter()
        .any(|text| text.contains(wanted))
}

fn picking(plates: &Plates) -> MotionPick {
    plates.doc.motion.as_ref().expect("a session").picking
}

/// The split the last request previews, if it previews one.
fn drafted(plates: &Plates) -> Option<Split> {
    match plates.last_draft()?.1 {
        FeatureKind::Split(split) => Some(split),
        _ => None,
    }
}

/// The tool of the split being set up.
fn tool(plates: &Plates) -> Option<SplitTool> {
    plates.doc.motion.as_ref()?.split().map(|split| split.tool)
}

/// Whether the box of `body` in the model shown is `low` to `high`.
fn boxed(plates: &Plates, body: BodyId, low: [f64; 3], high: [f64; 3]) -> bool {
    let [l, h] = plates.bounds(body);
    near(l, DVec3::from(low)) && near(h, DVec3::from(high))
}

/// The example's plate alone, "Body 1": 60 × 40 × 10 about the Z axis
/// from z 0 up, a hole of radius 8 through it about the Z axis.
fn plate() -> (Plates, BodyId) {
    let document = Document::example();
    let plate = document.bodies()[0].id;
    let (doc, requests) = holding(document);
    let plates = Plates {
        doc,
        requests,
        bodies: [plate; 3],
    };
    (plates, plate)
}

/// The plate's face whose summary `wanted` takes, clicked at `at`.
fn click_face(plates: &mut Plates, body: BodyId, wanted: impl Fn(&Summary) -> bool, at: DVec3) {
    let face = plates.face(body, wanted);
    plates.click_at(body, Picked::Face(face), at);
}

/// Whether `summary` is a plane facing up.
fn top(summary: &Summary) -> bool {
    matches!(summary, Summary::Plane { n, .. } if n[2] > 0.5)
}

/// A split of `body` by `tool`, both kept, the front keeping the id.
fn split_of(body: BodyId, tool: SplitTool) -> Split {
    Split {
        body,
        tool,
        original: Side::Front,
        keep: Keep::Both,
        new_body: Some(BodyId::NEW),
    }
}

/// [`plates`]'s document with Body 1 split by Body 2, both kept: the
/// editor, the split and its new body.
fn split_plates() -> (Editor, FeatureId, BodyId) {
    let plates = plates();
    let [plate, tool, _] = plates.bodies;
    let mut editor = Editor::new(plates.doc.editor.document().clone());
    let split = split_of(plate, SplitTool::Body(tool));
    editor
        .apply(editor.document().add_feature(split.into()))
        .unwrap();
    let feature = editor.document().features().last().unwrap();
    let FeatureKind::Split(Split {
        new_body: Some(new),
        ..
    }) = feature.kind
    else {
        panic!("a split keeping both");
    };
    let id = feature.id;
    (editor, id, new)
}

/// Holds `editor`'s document as [`plates`] does, with its first three
/// bodies (the first again where it has fewer).
fn held(editor: &Editor) -> Plates {
    let bodies = editor.document().bodies();
    let body = |at: usize| bodies.get(at).unwrap_or(&bodies[0]).id;
    let bodies = [body(0), body(1), body(2)];
    let (doc, requests) = holding(editor.document().clone());
    Plates {
        doc,
        requests,
        bodies,
    }
}

/// The rail's Split body with the example's only body: picks the tool
/// next, a plane or face to begin with, the toolbar offering the origin
/// planes; XY from it previews, which fails as too complex (the kernel's
/// split isn't built), shown in the panel; OK waits, and Add anyway keeps
/// it, "Split 1" making "Body 2", as one undo step.
#[test]
fn the_rail_s_split_body_by_xy_fails_too_complex_and_add_anyway_keeps_it() {
    let (mut plates, plate) = plate();
    let before = plates.doc.editor.document().clone();
    plates.doc.look(Look::StartSplit);
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(session.kind, MotionKind::Split);
    assert_eq!(session.bodies, [plate]);
    assert_eq!(session.split.mode, SplitMode::Face);
    assert_eq!(picking(&plates), MotionPick::Tool);
    for text in [
        "New split",
        "Body 1",
        "Split with",
        "Face",
        "Region",
        "Line",
        "Click a plane or face",
        "Keeps Body 1",
        "Front",
        "Back",
        "Keep",
        "Both",
        "XY plane",
        "pick a plane or a face to split with",
    ] {
        assert!(shows(&plates, text), "{text}");
    }
    assert!(!plates.doc.motion_ready());
    assert!(plates.last_draft().is_none(), "nothing to split with yet");

    plates.motion(MotionLook::OriginPlane(OriginPlane::XY));
    assert_eq!(picking(&plates), MotionPick::Nothing);
    let split = drafted(&plates).expect("a split's draft");
    assert_eq!(
        split,
        split_of(plate, SplitTool::Plane(PlaneRef::Origin(OriginPlane::XY)))
    );
    assert!(plates.doc.motion_ready());
    assert!(shows(&plates, "Body 1 by XY"));
    // The plane is drawn as a mirror's.
    let state = plates.doc.motion_state().unwrap();
    assert_eq!(state.line, Some([DVec3::ZERO, DVec3::Z]));
    plates.answer();
    let error = plates.doc.feed.draft_error().expect("the stand-in fails");
    assert!(error.contains("too complex"), "{error}");
    assert!(shows(&plates, "Split fails"));
    assert!(shows(&plates, "Add anyway"));
    let state = plates.doc.motion_state().unwrap();
    assert!(!state.ready && state.accept);
    // No pieces to label: the body is shown whole.
    assert!(state.split.as_ref().unwrap().pieces.is_empty());
    let features = plates.doc.editor.document().features().len();
    key_in(&mut plates.doc, enter());
    assert_eq!(plates.doc.editor.document().features().len(), features);

    plates.doc.update(Edit::AcceptError);
    assert!(plates.doc.motion.is_none());
    let (id, kind) = plates.last_feature();
    let FeatureKind::Split(stored) = &kind else {
        panic!("a split");
    };
    let new = stored.new_body.expect("a new body");
    let document = plates.doc.editor.document();
    assert_eq!(document.feature(id).unwrap().name, "Split 1");
    assert_eq!(document.body(new).unwrap().name, "Body 2");
    assert_eq!(plates.doc.selected_feature, Some(id));
    plates.answer();
    assert!(
        (plates.doc.feed.failed_features().iter()).any(|failed| failed.feature == id),
        "the split fails"
    );
    plates
        .doc
        .look(Look::SelectPanel(varde_view::Panel::Timeline));
    assert!(shows(&plates, "by XY"));
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

/// A flat face clicked is the tool's plane, named as the feature stores
/// it; the tool's field clicked again picks another, the model shown
/// unpreviewed meanwhile, and a curved face (the hole's wall) is the
/// tool as its surface.
#[test]
fn a_flat_face_splits_by_its_plane_and_a_curved_one_by_its_surface() {
    let (mut plates, plate) = plate();
    plates.doc.look(Look::StartSplit);
    click_face(&mut plates, plate, top, DVec3::new(20.0, 10.0, 10.0));
    let Some(SplitTool::Plane(PlaneRef::Face(face))) = tool(&plates) else {
        panic!("a plane face: {:?}", tool(&plates));
    };
    assert_eq!(face.body, plate);
    assert_eq!(picking(&plates), MotionPick::Nothing);
    assert!(shows(&plates, "Extrude 1's end"));
    assert!(drafted(&plates).is_some());

    plates.motion(MotionLook::Picking(MotionPick::Tool));
    assert_eq!(picking(&plates), MotionPick::Tool);
    assert!(plates.last_draft().is_none(), "the model as of the split");
    plates.answer();
    let hole = |summary: &Summary| matches!(summary, Summary::Cylinder { .. });
    click_face(&mut plates, plate, hole, DVec3::new(8.0, 0.0, 5.0));
    let Some(SplitTool::Face(face)) = tool(&plates) else {
        panic!("a face: {:?}", tool(&plates));
    };
    assert_eq!(face.body, plate);
    let split = drafted(&plates).expect("previewed again");
    assert_eq!(split.tool, SplitTool::Face(face));
    plates.answer();
    let error = plates.doc.feed.draft_error().expect("the stand-in fails");
    assert!(error.contains("too complex"), "{error}");
}

/// Another body as the tool, regeneration splitting by two booleans: the
/// body to split picked first, itself refused as its tool; the pieces
/// tinted apart and labelled, the front (the plate in the disc) keeping
/// the id; Back gives it the rest; keeping the front alone is a trim,
/// committed as one undo step making no body.
#[test]
fn a_body_splits_another_its_pieces_tinted_and_labelled() {
    varde_regen::testing::split_by_booleans();
    let mut plates = plates();
    let [plate, disc, _] = plates.bodies;
    let before = plates.doc.editor.document().clone();
    plates.doc.look(Look::StartSplit);
    // Three bodies and none selected: the body first.
    assert_eq!(picking(&plates), MotionPick::Bodies);
    assert!(shows(&plates, "pick the body to split"));
    plates.click(plate);
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [plate]);
    assert_eq!(picking(&plates), MotionPick::Tool);
    plates.motion(MotionLook::SplitWith(SplitMode::Body));
    assert_eq!(picking(&plates), MotionPick::Tool);
    assert!(shows(&plates, "pick a body to split with"));
    plates.click(plate);
    assert_eq!(tool(&plates), None);
    let notice = plates.doc.notice.clone().unwrap_or_default();
    assert!(notice.contains("body being split"), "{notice}");
    plates.click(disc);
    assert_eq!(tool(&plates), Some(SplitTool::Body(disc)));
    assert_eq!(picking(&plates), MotionPick::Nothing);
    assert!(shows(&plates, "Body 1 by Body 2"));

    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    // The front, the plate in the disc, keeps the id.
    assert!(boxed(&plates, plate, [15.0, -5.0, 0.0], [25.0, 5.0, 10.0]));
    let state = plates.doc.motion_state().unwrap();
    let pieces = &state.split.as_ref().unwrap().pieces;
    assert_eq!(pieces.len(), 2, "{pieces:?}");
    assert_eq!((pieces[0].name.as_str(), pieces[0].keeps), ("Body 1", true));
    assert!(near(pieces[0].at, DVec3::new(20.0, 0.0, 5.0)));
    assert_eq!(
        (pieces[1].name.as_str(), pieces[1].keeps),
        ("New body", false)
    );
    assert!(near(pieces[1].at, DVec3::new(0.0, 0.0, 5.0)));
    // Tinted apart: the body's faces as selected, the other's second.
    plates.doc.refresh_motion_highlight();
    let highlight = plates.doc.motion_highlight().expect("a highlight");
    let index = plates.doc.feed.pick_index();
    let faces = |body: BodyId| index.body_faces(body).collect::<Vec<_>>();
    assert!(
        faces(plate)
            .iter()
            .all(|f| highlight.selected_faces.contains(f))
    );
    assert!(!highlight.second_faces.is_empty());
    assert!(
        (highlight.second_faces.iter()).all(|&f| index.face_body(f) != Some(plate)),
        "the other piece's"
    );

    // Back keeps the id: the plate less the disc.
    plates.motion(MotionLook::Original(Side::Back));
    plates.answer();
    assert!(boxed(
        &plates,
        plate,
        [-30.0, -20.0, 0.0],
        [30.0, 20.0, 10.0]
    ));
    assert!(shows(&plates, "Keeps Body 1"));

    // The front alone: a trim, the front keeping the id.
    plates.motion(MotionLook::Keep(Keep::Front));
    let split = drafted(&plates).expect("a trim's draft");
    assert_eq!((split.keep, split.new_body), (Keep::Front, None));
    plates.answer();
    assert!(boxed(&plates, plate, [15.0, -5.0, 0.0], [25.0, 5.0, 10.0]));
    let state = plates.doc.motion_state().unwrap();
    let pieces = &state.split.as_ref().unwrap().pieces;
    assert_eq!(pieces.len(), 1);
    assert!(shows(&plates, "Body 1 by Body 2 · front only"));
    let bodies = plates.doc.editor.document().bodies().len();
    key_in(&mut plates.doc, enter());
    assert!(plates.doc.motion.is_none());
    let (_, kind) = plates.last_feature();
    assert_eq!(kind, FeatureKind::Split(split));
    assert_eq!(plates.doc.editor.document().bodies().len(), bodies);
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

/// Adds a sketch on XY to `editor` holding a disc of radius 5 about
/// (20, 0) and a line from (-40, 12) to (40, 12), not used by any
/// feature: the sketch and the line's id.
fn add_tools_sketch(editor: &mut Editor) -> (FeatureId, Id) {
    let plane = Plane::Origin(OriginPlane::XY);
    editor.apply(editor.document().add_sketch(plane)).unwrap();
    let feature = editor.document().features().last().unwrap().id;
    let mut sketch = varde_sketch::Sketch::default();
    let center = sketch.add_point(glam::DVec2::new(20.0, 0.0)).unwrap();
    sketch
        .add_curve(
            Curve::Circle {
                center,
                radius: 5.0,
            },
            false,
        )
        .unwrap();
    let start = sketch.add_point(glam::DVec2::new(-40.0, 12.0)).unwrap();
    let end = sketch.add_point(glam::DVec2::new(40.0, 12.0)).unwrap();
    let line = sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    (feature, line)
}

/// A sketch's region as the tool, picked in the viewport (the model
/// isn't picked meanwhile), splits the plate through all; picked again
/// it's taken out. A line of the sketch's curves is the tool too, which
/// the stand-in fails as too complex.
#[test]
fn regions_of_a_sketch_and_an_open_line_split_a_body() {
    varde_regen::testing::split_by_booleans();
    let (plates, plate) = plate();
    let mut editor = Editor::new(plates.doc.editor.document().clone());
    let (sketch, line) = add_tools_sketch(&mut editor);
    let mut plates = held(&editor);
    plates.doc.look(Look::StartSplit);
    plates.motion(MotionLook::SplitWith(SplitMode::Regions));
    assert_eq!(picking(&plates), MotionPick::Tool);
    assert!(
        plates.doc.model_picking().is_none(),
        "the sketch's, not the model"
    );
    let state = plates.doc.motion_state().unwrap();
    let view = state.split.as_ref().unwrap();
    assert!(
        (view.candidates.iter()).any(|candidate| candidate.feature == sketch),
        "the sketch's regions show"
    );
    assert!(shows(&plates, "Click regions of a sketch"));
    plates.motion(MotionLook::SplitRegion { sketch, region: 0 });
    let Some(SplitTool::Regions {
        sketch: of,
        regions,
    }) = tool(&plates)
    else {
        panic!("regions: {:?}", tool(&plates));
    };
    assert_eq!((of, regions.len()), (sketch, 1));
    assert!(shows(&plates, "1 region"));
    // Previewed while regions are picked.
    assert!(drafted(&plates).is_some());
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    assert!(boxed(&plates, plate, [15.0, -5.0, 0.0], [25.0, 5.0, 10.0]));
    plates.motion(MotionLook::SplitRegion { sketch, region: 0 });
    assert_eq!(tool(&plates), None);

    plates.motion(MotionLook::SplitWith(SplitMode::Line));
    assert_eq!(picking(&plates), MotionPick::Tool);
    let state = plates.doc.motion_state().unwrap();
    let view = state.split.as_ref().unwrap();
    assert!((view.lines.iter()).any(|lines| lines.feature == sketch));
    plates.motion(MotionLook::SplitCurve {
        sketch,
        curve: line,
    });
    assert_eq!(
        tool(&plates),
        Some(SplitTool::Chain {
            sketch,
            curves: vec![line]
        })
    );
    assert!(shows(&plates, "1 curve"));
    plates.answer();
    let error = plates.doc.feed.draft_error().expect("the stand-in fails");
    assert!(error.contains("too complex"), "{error}");
    // The regions' tile keeps its own, none now.
    plates.motion(MotionLook::SplitWith(SplitMode::Regions));
    assert_eq!(tool(&plates), None);
}

/// Editing a split from the Timeline opens it with its tool and options;
/// Cancel leaves no trace; Back and OK sets it as one undo step.
#[test]
fn editing_a_split_from_the_timeline_cancel_and_undo() {
    varde_regen::testing::split_by_booleans();
    let (editor, id, new) = split_plates();
    let mut plates = held(&editor);
    let [plate, disc, _] = plates.bodies;
    let before = plates.doc.editor.document().clone();
    // Split by booleans: the front keeps the id, the rest is the new body.
    assert!(boxed(&plates, plate, [15.0, -5.0, 0.0], [25.0, 5.0, 10.0]));
    assert!(boxed(&plates, new, [-30.0, -20.0, 0.0], [30.0, 20.0, 10.0]));

    plates.doc.look(Look::EditFeature(id));
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(session.kind, MotionKind::Split);
    assert_eq!(session.feature, Some(id));
    assert_eq!(session.split.mode, SplitMode::Body);
    assert_eq!(tool(&plates), Some(SplitTool::Body(disc)));
    assert_eq!(picking(&plates), MotionPick::Nothing);
    assert!(shows(&plates, "Split 1"));
    plates.motion(MotionLook::Original(Side::Back));
    plates.motion(MotionLook::Cancel);
    assert!(plates.doc.motion.is_none());
    assert_eq!(*plates.doc.editor.document(), before);

    plates.doc.look(Look::EditFeature(id));
    plates.motion(MotionLook::Original(Side::Back));
    let (feature, _) = plates.last_draft().expect("a draft");
    assert_eq!(feature, Some(id));
    plates.answer();
    // Its pieces swap: the body keeps the rest, the new one the front.
    assert!(boxed(
        &plates,
        plate,
        [-30.0, -20.0, 0.0],
        [30.0, 20.0, 10.0]
    ));
    let state = plates.doc.motion_state().unwrap();
    let pieces = &state.split.as_ref().unwrap().pieces;
    assert_eq!(pieces.len(), 2);
    assert_eq!(pieces[1].name, "Body 4", "the new body as named");
    key_in(&mut plates.doc, enter());
    assert!(plates.doc.motion.is_none());
    let FeatureKind::Split(set) = &plates.doc.editor.document().feature(id).unwrap().kind else {
        panic!("a split");
    };
    assert_eq!((set.original, set.new_body), (Side::Back, Some(new)));
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

/// Later features naming the body are warned of with the piece they'll
/// get; keeping one side while a later feature names the new body is
/// refused at once, as the document would, and not previewed.
#[test]
fn later_features_are_warned_of_and_a_named_new_body_held() {
    let (mut editor, id, new) = split_plates();
    let plate = editor.document().bodies()[0].id;
    for body in [plate, new] {
        let document = editor.document();
        let ask = Move::offset_ask(&document.design());
        let value = |text: &str| varde_expr::Value::new(text, &ask).unwrap();
        let moved = Move {
            bodies: vec![body],
            offset: [value("5"), value("0"), value("0")],
            turn: None,
        };
        editor.apply(document.add_feature(moved.into())).unwrap();
    }
    let mut plates = held(&editor);
    plates.doc.look(Look::EditFeature(id));
    let warned = |plates: &Plates| {
        let state = plates.doc.motion_state().unwrap();
        state.split.as_ref().unwrap().later.clone()
    };
    assert_eq!(
        warned(&plates).as_deref(),
        Some("1 later feature uses Body 1: it'll get the front piece")
    );
    plates.motion(MotionLook::Original(Side::Back));
    assert_eq!(
        warned(&plates).as_deref(),
        Some("1 later feature uses Body 1: it'll get the back piece")
    );
    assert!(shows(&plates, "it'll get the back piece"));
    plates.motion(MotionLook::Keep(Keep::Front));
    assert!(!plates.doc.motion_ready());
    let state = plates.doc.motion_state().unwrap();
    let refused = state.refused.clone().unwrap_or_default();
    assert!(refused.starts_with("Move 2 uses Body 4"), "{refused}");
    assert!(plates.last_draft().is_none(), "not previewed");
    plates.motion(MotionLook::Keep(Keep::Both));
    assert!(plates.doc.motion_ready());
}

/// A tool body an undo takes away is kept, said to be gone, nothing
/// previewed or committed, until a redo brings it back.
#[test]
fn a_tool_an_undo_takes_away_is_said_to_be_gone() {
    let mut plates = plates();
    let [plate, _, _] = plates.bodies;
    let last = later_disc(&mut plates);
    plates.doc.look(Look::StartSplit);
    plates.click(plate);
    plates.motion(MotionLook::SplitWith(SplitMode::Body));
    plates.click(last);
    assert_eq!(tool(&plates), Some(SplitTool::Body(last)));
    assert!(plates.doc.motion_ready());
    // The last disc's extrude undone: the tool body is gone.
    plates.doc.update(Edit::Undo);
    assert!(plates.doc.editor.document().body(last).is_none());
    let session = plates.doc.motion.as_ref().expect("still set up");
    assert_eq!(session.split.body, Some(last));
    assert!(!plates.doc.motion_ready());
    assert!(shows(&plates, "The tool body is gone: pick another"));
    assert!(plates.last_draft().is_none());
    plates.doc.update(Edit::Redo);
    assert!(plates.doc.motion_ready());
    assert!(drafted(&plates).is_some());
}

/// Editing a split shown split (by two booleans): a face of its new
/// body, made by the plate's extrude, is named on the body split, where
/// it is at the split, and a click on the new body picks the body split.
#[test]
fn faces_of_an_edited_split_s_new_body_are_named_on_the_body_split() {
    varde_regen::testing::split_by_booleans();
    let (editor, id, new) = split_plates();
    let mut plates = held(&editor);
    let [plate, _, _] = plates.bodies;
    plates.doc.look(Look::EditFeature(id));
    plates.motion(MotionLook::SplitWith(SplitMode::Face));
    assert_eq!(picking(&plates), MotionPick::Tool);
    assert!(plates.last_draft().is_none(), "the document's model");
    plates.answer();
    click_face(&mut plates, new, top, DVec3::new(-20.0, 10.0, 10.0));
    let Some(SplitTool::Plane(PlaneRef::Face(face))) = tool(&plates) else {
        panic!("a plane face: {:?} {:?}", tool(&plates), plates.doc.notice);
    };
    assert_eq!(face.body, plate);
    // A click on the new body picks the body it was split from.
    plates.motion(MotionLook::Picking(MotionPick::Bodies));
    plates.click(new);
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [plate]);
}
