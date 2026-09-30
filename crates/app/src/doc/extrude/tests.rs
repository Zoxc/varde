use std::cell::RefCell;
use std::rc::Rc;

use iced::keyboard::{self, key};
use varde_document::{Command, Document, Editor, FeatureId, FeatureKind, Operation, Tolerance};
use varde_regen::Request;
use varde_view::{Distance, Edit, ExtentKind, ExtrudeLook, Look, Mode, OperationKind};

use super::*;
use crate::doc::sketch::CHECKING;
use crate::tests::{answer, deferred, example, key_in, press_in};

type Requests = Rc<RefCell<Vec<Request>>>;

/// A document holding the example's plate sketch, a rectangle with a
/// hole, shown and not extruded, whose regeneration requests wait for the
/// test, and the sketch.
fn plate() -> (Doc, FeatureId, Requests) {
    let mut editor = Editor::new(Document::example());
    let sketch = editor.document().features()[0].id;
    let extrude = editor.document().features()[1].id;
    editor.apply(Command::RemoveFeature(extrude)).unwrap();
    editor
        .apply(Command::SetFeatureVisible(sketch, true))
        .unwrap();
    let (mut doc, requests) = deferred();
    doc.apply(Command::Replace(Box::new(editor.document().clone())));
    doc.sync();
    answer(&mut doc, &requests);
    (doc, sketch, requests)
}

/// The plate's region with the hole in it, by its index.
fn plate_region(doc: &Doc, sketch: FeatureId) -> usize {
    let Some(FeatureKind::Sketch { sketch, .. }) =
        doc.editor.document().feature(sketch).map(|f| &f.kind)
    else {
        panic!("no sketch");
    };
    let profiles = sketch.profiles().unwrap();
    profiles
        .regions
        .iter()
        .position(|region| region.holes.len() == 1)
        .unwrap()
}

fn key(c: &str) -> keyboard::Key {
    keyboard::Key::Character(c.into())
}

fn enter() -> keyboard::Key {
    keyboard::Key::Named(key::Named::Enter)
}

fn extrude(doc: &mut Doc, message: ExtrudeLook) {
    doc.look(Look::Extrude(message));
}

/// The draft the last request waiting carries, if any.
fn last_draft(requests: &Requests) -> Option<varde_regen::Draft> {
    let requests = requests.borrow();
    let Request::Regenerate { draft, .. } = requests.last()?;
    draft.clone()
}

/// The extrudes of `doc`'s document.
fn extrudes(doc: &Doc) -> Vec<&varde_document::Extrude> {
    doc.editor
        .document()
        .features()
        .iter()
        .filter_map(|feature| match &feature.kind {
            FeatureKind::Extrude(extrude) => Some(extrude),
            FeatureKind::Sketch { .. } => None,
        })
        .collect()
}

#[test]
fn e_starts_a_session_where_a_click_picks_a_region_previewed() {
    let (mut doc, sketch, requests) = plate();
    key_in(&mut doc, key("e"));
    let session = doc.extrude.as_ref().expect("E starts a session");
    // Nothing selected: the first region picked sets the sketch.
    assert_eq!(session.source, None);
    let state = doc.extrude_state().unwrap();
    assert_eq!(state.candidates.len(), 1);
    assert!(!state.ready);
    assert!(last_draft(&requests).is_none());

    let region = plate_region(&doc, sketch);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region });
    let session = doc.extrude.as_ref().unwrap();
    assert_eq!(session.source, Some(sketch));
    assert_eq!(session.picked, BTreeSet::from([region]));
    assert!(doc.extrude_state().unwrap().ready);

    // The preview is the draft applied, a new body.
    let draft = last_draft(&requests).expect("a draft is asked for");
    assert_eq!(draft.feature, None);
    assert_eq!(draft.extrude.sketch, sketch);
    assert_eq!(draft.extrude.span(), Some((0.0, 10.0)));
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.shown_draft(), Some(draft.revision));
    assert!(doc.feed.mesh().triangle_count() > 0);
    assert_eq!(doc.feed.draft_error(), None);
    // Nothing is in the document yet.
    assert!(extrudes(&doc).is_empty());

    // Clicked again, it's taken out, and the preview goes.
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region });
    assert!(doc.extrude.as_ref().unwrap().picked.is_empty());
    assert!(last_draft(&requests).is_none());
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.shown_draft(), None);
    assert_eq!(doc.feed.mesh().triangle_count(), 0);
}

#[test]
fn a_selected_sketch_is_the_source() {
    let (mut doc, sketch, _) = plate();
    doc.look(Look::SelectFeature(sketch));
    doc.look(Look::StartExtrude);
    assert_eq!(doc.extrude.as_ref().unwrap().source, Some(sketch));
    // The toolbar's button again backs out.
    doc.look(Look::StartExtrude);
    assert!(doc.extrude.is_none());
}

#[test]
fn a_typed_distance_updates_the_draft_and_a_refused_one_blocks_ok() {
    let (mut doc, sketch, requests) = plate();
    key_in(&mut doc, key("e"));
    let region = plate_region(&doc, sketch);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region });
    let first = last_draft(&requests).unwrap().revision;

    let input = |text: &str| ExtrudeLook::Input {
        distance: Distance::First,
        text: text.to_owned(),
    };
    extrude(&mut doc, input("25"));
    let draft = last_draft(&requests).unwrap();
    assert!(draft.revision > first);
    assert_eq!(draft.extrude.span(), Some((0.0, 25.0)));

    // Refused, it says why and blocks OK; the preview keeps the last.
    extrude(&mut doc, input("25 +"));
    let state = doc.extrude_state().unwrap();
    assert!(state.fields[0].error.is_some());
    assert!(!state.ready);
    assert!(press_in(&doc, enter()).is_none());
    assert_eq!(last_draft(&requests).unwrap().revision, draft.revision);
    extrude(&mut doc, input("2 * 15"));
    assert_eq!(
        last_draft(&requests).unwrap().extrude.span(),
        Some((0.0, 30.0))
    );
    assert!(doc.extrude_state().unwrap().ready);
}

#[test]
fn enter_commits_the_extrude_as_one_undo_step() {
    let (mut doc, sketch, requests) = plate();
    let before = doc.editor.document().clone();
    key_in(&mut doc, key("e"));
    let region = plate_region(&doc, sketch);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region });
    extrude(&mut doc, ExtrudeLook::Extent(ExtentKind::Symmetric));
    answer(&mut doc, &requests);

    key_in(&mut doc, enter());
    assert!(doc.extrude.is_none());
    let [extrude] = extrudes(&doc)[..] else {
        panic!("one extrude");
    };
    assert_eq!(extrude.span(), Some((-5.0, 5.0)));
    let feature = doc.editor.document().features().last().unwrap().id;
    assert_eq!(doc.selected_feature, Some(feature));
    assert_eq!(doc.editor.document().bodies().len(), 1);
    // The model is asked for without a draft now.
    assert!(last_draft(&requests).is_none());
    answer(&mut doc, &requests);
    assert!(doc.feed.mesh().triangle_count() > 0);

    doc.update(Edit::Undo);
    assert_eq!(*doc.editor.document(), before);
}

#[test]
fn escape_leaves_no_trace() {
    let (mut doc, sketch, requests) = plate();
    let before = doc.editor.document().clone();
    let revision = doc.editor.revision();
    key_in(&mut doc, key("e"));
    let region = plate_region(&doc, sketch);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region });
    answer(&mut doc, &requests);
    assert!(doc.feed.mesh().triangle_count() > 0);

    doc.look(Look::Escape);
    assert!(doc.extrude.is_none());
    assert_eq!(*doc.editor.document(), before);
    assert_eq!(doc.editor.revision(), revision);
    assert!(last_draft(&requests).is_none());
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.mesh().triangle_count(), 0);
}

#[test]
fn a_double_clicked_extrude_reopens_with_its_values_and_is_set_again() {
    let (mut doc, requests) = example();
    let features = doc.editor.document().features();
    let (sketch, feature) = (features[0].id, features[1].id);

    doc.look(Look::EditFeature(feature));
    let session = doc.extrude.as_ref().expect("editing it");
    assert_eq!(session.feature, Some(feature));
    assert_eq!(session.source, Some(sketch));
    assert_eq!(session.picked.len(), 1);
    assert_eq!(session.missing, 0);
    assert_eq!(session.extent, ExtentKind::OneSide);
    assert_eq!(session.fields[0].text, "10");
    assert_eq!(session.operation, OperationKind::NewBody);
    let state = doc.extrude_state().unwrap();
    assert_eq!(state.editing, Some("Extrude 1"));
    assert!(state.ready);

    extrude(
        &mut doc,
        ExtrudeLook::Input {
            distance: Distance::First,
            text: "20".to_owned(),
        },
    );
    assert_eq!(last_draft(&requests).unwrap().feature, Some(feature));
    doc.update(Edit::CommitExtrude);
    assert!(doc.extrude.is_none());
    let [extrude] = extrudes(&doc)[..] else {
        panic!("one extrude");
    };
    assert_eq!(extrude.span(), Some((0.0, 20.0)));
    assert!(matches!(extrude.operation, Operation::NewBody(_)));
    assert_eq!(doc.editor.document().bodies().len(), 1);
    doc.update(Edit::Undo);
    let [extrude] = extrudes(&doc)[..] else {
        panic!("one extrude");
    };
    assert_eq!(extrude.span(), Some((0.0, 10.0)));
}

#[test]
fn enter_on_a_selected_extrude_edits_it() {
    let (mut doc, _) = example();
    let feature = doc.editor.document().features()[1].id;
    doc.look(Look::SelectFeature(feature));
    key_in(&mut doc, enter());
    assert_eq!(doc.extrude.as_ref().unwrap().feature, Some(feature));
    // Enter again is OK, which changes nothing here.
    let revision = doc.editor.revision();
    key_in(&mut doc, enter());
    assert!(doc.extrude.is_none());
    assert_eq!(doc.editor.revision(), revision);
}

#[test]
fn the_handle_drags_the_distance_and_flips_one_side() {
    let (mut doc, sketch, requests) = plate();
    key_in(&mut doc, key("e"));
    let region = plate_region(&doc, sketch);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region });
    let handle = doc.extrude_state().unwrap().handle().expect("a handle");
    assert_eq!(handle.knobs, [(Distance::First, 10.0)]);

    // Not grabbed, a drag does nothing.
    let drag = |to| ExtrudeLook::DragHandle {
        distance: Distance::First,
        to,
    };
    extrude(&mut doc, drag(4.0));
    assert_eq!(doc.extrude.as_ref().unwrap().fields[0].text, "10 mm");
    extrude(&mut doc, ExtrudeLook::GrabHandle(Distance::First));
    extrude(&mut doc, drag(-4.0));
    let session = doc.extrude.as_ref().unwrap();
    assert!(session.flip);
    assert_eq!(session.fields[0].value.as_ref().unwrap().value, 4.0);
    assert_eq!(
        last_draft(&requests).unwrap().extrude.span(),
        Some((-4.0, 0.0))
    );
    // On the plane, it stays.
    extrude(&mut doc, drag(0.0));
    assert_eq!(doc.extrude.as_ref().unwrap().fields[0].text, "4 mm");
    extrude(&mut doc, ExtrudeLook::DropHandle);
    assert_eq!(doc.extrude.as_ref().unwrap().grabbed, None);
}

#[test]
fn two_sides_over_the_limit_block_ok() {
    let (mut doc, sketch, requests) = plate();
    key_in(&mut doc, key("e"));
    let region = plate_region(&doc, sketch);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region });
    extrude(&mut doc, ExtrudeLook::Extent(ExtentKind::TwoSides));
    for distance in [Distance::First, Distance::Second] {
        let text = "600000".to_owned();
        extrude(&mut doc, ExtrudeLook::Input { distance, text });
    }
    // Each side is taken, but together they're over: OK says why at
    // once, before the preview's answer.
    let state = doc.extrude_state().unwrap();
    assert!(state.fields.iter().all(|field| field.error.is_none()));
    assert!(!state.ready);
    assert_eq!(state.refused, Some(varde_document::ExtrudeError::Length));
    let _ = doc.view(false, Mode::default());
    assert!(press_in(&doc, enter()).is_none());
    doc.update(Edit::CommitExtrude);
    assert!(doc.extrude.is_some());
    assert_eq!(doc.edit_error, None);
    assert!(extrudes(&doc).is_empty());
    answer(&mut doc, &requests);
    assert!(doc.extrude_state().unwrap().error.is_some());

    // Exactly at the limit, the document takes it.
    let text = "400000".to_owned();
    extrude(
        &mut doc,
        ExtrudeLook::Input {
            distance: Distance::Second,
            text,
        },
    );
    let state = doc.extrude_state().unwrap();
    assert_eq!(state.refused, None);
    assert!(state.ready);
    doc.update(Edit::CommitExtrude);
    assert_eq!(doc.edit_error, None);
    assert!(doc.extrude.is_none());
    assert_eq!(extrudes(&doc)[0].span(), Some((-400000.0, 600000.0)));
}

#[test]
fn a_two_sides_knob_stops_at_the_limit() {
    let (mut doc, sketch, _) = plate();
    key_in(&mut doc, key("e"));
    let region = plate_region(&doc, sketch);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region });
    extrude(&mut doc, ExtrudeLook::Extent(ExtentKind::TwoSides));
    let text = "600000".to_owned();
    extrude(
        &mut doc,
        ExtrudeLook::Input {
            distance: Distance::First,
            text,
        },
    );
    extrude(&mut doc, ExtrudeLook::GrabHandle(Distance::Second));
    let drag = |to| ExtrudeLook::DragHandle {
        distance: Distance::Second,
        to,
    };
    let second = |doc: &Doc| doc.extrude.as_ref().unwrap().fields[1].value.clone();
    let before = second(&doc);
    extrude(&mut doc, drag(-500000.0));
    assert_eq!(second(&doc), before);
    extrude(&mut doc, drag(-400000.0));
    assert_eq!(second(&doc).unwrap().value, 400000.0);
    assert!(doc.extrude_state().unwrap().ready);
}

/// The example, and a sketch on XY after it holding a circle of radius 3
/// about (-20, 10), on the plate, selected: its id.
fn example_and_a_hole() -> (Doc, FeatureId, Requests) {
    let (mut doc, requests) = example();
    let plane = varde_document::Plane::Origin(varde_document::OriginPlane::XY);
    doc.apply(doc.editor.document().add_sketch(plane));
    let sketch = doc.editor.document().features().last().unwrap().id;
    let mut drawn = varde_sketch::Sketch::default();
    let center = drawn.add_point(glam::DVec2::new(-20.0, 10.0)).unwrap();
    drawn
        .add_curve(
            varde_sketch::Curve::Circle {
                center,
                radius: 3.0,
            },
            false,
        )
        .unwrap();
    doc.apply(Command::SetSketch {
        feature: sketch,
        sketch: Box::new(drawn),
    });
    doc.sync();
    answer(&mut doc, &requests);
    doc.look(Look::SelectFeature(sketch));
    (doc, sketch, requests)
}

#[test]
fn a_cut_lists_the_bodies_it_touches_and_goes_through_all() {
    let (mut doc, sketch, requests) = example_and_a_hole();
    let body = doc.editor.document().bodies()[0].id;
    doc.look(Look::StartExtrude);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region: 0 });
    // Only a cut goes through all.
    extrude(&mut doc, ExtrudeLook::Extent(ExtentKind::ThroughAll));
    assert_eq!(doc.extrude.as_ref().unwrap().extent, ExtentKind::OneSide);
    extrude(&mut doc, ExtrudeLook::Operation(OperationKind::Cut));
    extrude(&mut doc, ExtrudeLook::Extent(ExtentKind::ThroughAll));
    assert_eq!(doc.extrude.as_ref().unwrap().extent, ExtentKind::ThroughAll);
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.draft_error(), None);
    let listed = |doc: &Doc| {
        let state = doc.extrude_state().unwrap();
        (state.targets.iter())
            .map(|target| (target.body, target.name.to_owned(), target.included))
            .collect::<Vec<_>>()
    };
    assert_eq!(listed(&doc), [(body, "Body 1".to_owned(), true)]);
    let _ = doc.view(false, Mode::default());

    // Taken out, it stays listed, and the cut has nothing to cut.
    extrude(&mut doc, ExtrudeLook::Target(body));
    assert_eq!(listed(&doc), [(body, "Body 1".to_owned(), false)]);
    answer(&mut doc, &requests);
    assert_eq!(
        doc.feed.draft_error(),
        Some("it doesn't touch any body not taken out of it")
    );
    extrude(&mut doc, ExtrudeLook::Target(body));
    assert_eq!(doc.extrude.as_ref().unwrap().excluded, []);

    // Joining can't go through all: it goes back to one side.
    extrude(&mut doc, ExtrudeLook::Operation(OperationKind::Join));
    assert_eq!(doc.extrude.as_ref().unwrap().extent, ExtentKind::OneSide);
    extrude(&mut doc, ExtrudeLook::Operation(OperationKind::Cut));
    extrude(&mut doc, ExtrudeLook::Extent(ExtentKind::ThroughAll));
    doc.update(Edit::CommitExtrude);
    assert_eq!(doc.edit_error, None);
    assert!(doc.extrude.is_none());
    let cut = extrudes(&doc)[1].clone();
    assert_eq!(cut.extent, varde_document::Extent::ThroughAll);
    assert_eq!(cut.operation, Operation::Cut(Default::default()));
    // The cut adds no body.
    assert_eq!(doc.editor.document().bodies().len(), 1);
    answer(&mut doc, &requests);
    assert!(doc.feed.failed_features().is_empty());
    assert!(doc.feed.mesh().triangle_count() > 0);
}

#[test]
fn undoing_the_sketch_away_ends_the_session() {
    let (mut doc, sketch, _) = plate();
    doc.look(Look::SelectFeature(sketch));
    doc.look(Look::StartExtrude);
    assert!(doc.extrude.is_some());
    doc.update(Edit::RemoveFeature(sketch));
    assert!(doc.extrude.is_none());
}

#[test]
fn a_sketch_changed_under_the_session_keeps_its_regions_picked() {
    let (mut doc, sketch, _) = plate();
    doc.look(Look::SelectFeature(sketch));
    doc.look(Look::StartExtrude);
    let region = plate_region(&doc, sketch);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region });
    // A point added elsewhere changes the sketch, not the region.
    let Some(FeatureKind::Sketch { sketch: drawn, .. }) =
        doc.editor.document().feature(sketch).map(|f| &f.kind)
    else {
        panic!("no sketch");
    };
    let mut drawn = drawn.clone();
    drawn.add_point(glam::DVec2::new(100.0, 100.0)).unwrap();
    doc.apply(Command::SetSketch {
        feature: sketch,
        sketch: Box::new(drawn),
    });
    doc.sync();
    let session = doc.extrude.as_ref().unwrap();
    assert_eq!(session.picked, BTreeSet::from([plate_region(&doc, sketch)]));
    // And undone, too.
    doc.update(Edit::Undo);
    let session = doc.extrude.as_ref().unwrap();
    assert_eq!(session.picked, BTreeSet::from([plate_region(&doc, sketch)]));
}

#[test]
fn undoing_the_extrude_edited_away_ends_the_session_and_its_draft() {
    let (mut doc, requests) = example();
    let feature = doc.editor.document().features()[1].id;
    doc.look(Look::EditFeature(feature));
    extrude(
        &mut doc,
        ExtrudeLook::Input {
            distance: Distance::First,
            text: "20".to_owned(),
        },
    );
    assert!(last_draft(&requests).is_some());
    // The example came in as one step.
    doc.update(Edit::Undo);
    assert!(doc.editor.document().features().is_empty());
    assert!(doc.extrude.is_none());
    assert!(last_draft(&requests).is_none());
}

#[test]
fn a_read_only_document_has_no_session() {
    let (mut doc, requests) = example();
    let feature = doc.editor.document().features()[1].id;
    doc.look(Look::EditFeature(feature));
    assert!(doc.extrude.is_some());
    // Say its file turned out read-only.
    doc.read_only = Some("test".to_owned());
    doc.sync();
    assert!(doc.extrude.is_none());
    assert!(last_draft(&requests).is_none());
    doc.look(Look::EditFeature(feature));
    doc.look(Look::StartExtrude);
    assert!(doc.extrude.is_none());
    assert!(press_in(&doc, key("e")).is_none());
}

#[test]
fn the_panel_s_field_takes_typing_enter_as_ok_and_escape_as_cancel() {
    use crate::tests::{pressed, typing};
    use varde_view::Message as Ui;

    let (mut doc, sketch, _) = plate();
    key_in(&mut doc, key("e"));
    let region = plate_region(&doc, sketch);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region });

    let (sent, _) = pressed(&doc, &[typing(key("5"), Some("5"))], true);
    assert!(
        matches!(
            &sent[..],
            [Ui::Look(Look::Extrude(ExtrudeLook::Input { distance: Distance::First, text }))]
                if text == "5"
        ),
        "{sent:?}"
    );
    let (sent, shortcuts) = pressed(&doc, &[typing(enter(), None)], true);
    assert!(
        matches!(sent[..], [Ui::Edit(Edit::CommitExtrude)]),
        "{sent:?} {shortcuts:?}"
    );
    let escape = keyboard::Key::Named(key::Named::Escape);
    let (sent, _) = pressed(&doc, &[typing(escape, None)], true);
    assert!(
        matches!(sent[..], [Ui::Look(Look::Extrude(ExtrudeLook::Cancel))]),
        "{sent:?}"
    );
    // Unfocused, Enter is the screen's shortcut, OK too.
    let (sent, shortcuts) = pressed(&doc, &[typing(enter(), None)], false);
    assert!(sent.is_empty(), "{sent:?}");
    assert!(
        matches!(
            shortcuts[..],
            [crate::Message::Ui(Ui::Edit(Edit::CommitExtrude))]
        ),
        "{shortcuts:?}"
    );
}

#[test]
fn a_failing_extrude_is_marked_in_the_timeline() {
    let (mut doc, requests) = example();
    let feature = doc.editor.document().features()[1].id;
    assert!(doc.feed.failed_features().is_empty());

    // A join has no body before it to join, so it fails and makes
    // nothing.
    let mut extrude = extrudes(&doc)[0].clone();
    extrude.operation = Operation::Join(Default::default());
    doc.apply(Command::SetExtrude {
        feature,
        extrude: Box::new(extrude),
    });
    doc.sync();
    answer(&mut doc, &requests);
    let failed = doc.feed.failed_features();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].0, feature);
    assert_eq!(failed[0].1, "it doesn't touch any body");
    assert_eq!(doc.feed.mesh().triangle_count(), 0);
    doc.look(Look::SelectPanel(varde_view::Panel::Timeline));
    let _ = doc.view(false, Mode::default());

    // Undone, it goes again.
    doc.update(Edit::Undo);
    answer(&mut doc, &requests);
    assert!(doc.feed.failed_features().is_empty());
}

#[test]
fn the_tolerance_is_set_from_the_file_menu() {
    let (mut doc, _) = example();
    doc.update(Edit::ToggleFileMenu);
    let _ = doc.view(false, Mode::default());
    let coarse = Tolerance::new(1e-2).unwrap();
    doc.update(Edit::SetTolerance(coarse));
    assert_eq!(doc.editor.document().tolerance(), coarse);
    // A value the menu doesn't offer shows too.
    doc.update(Edit::SetTolerance(Tolerance::new(5e-5).unwrap()));
    doc.update(Edit::ToggleFileMenu);
    let _ = doc.view(false, Mode::default());
    doc.update(Edit::Undo);
    doc.update(Edit::Undo);
    assert_eq!(doc.editor.document().tolerance(), Tolerance::DEFAULT);
}

#[test]
fn a_bare_distance_keeps_its_length_when_the_units_change() {
    let (mut doc, sketch, requests) = plate();
    key_in(&mut doc, key("e"));
    let region = plate_region(&doc, sketch);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region });
    let text = "20".to_owned();
    extrude(
        &mut doc,
        ExtrudeLook::Input {
            distance: Distance::First,
            text,
        },
    );
    // Typed in millimetres, it stays 20 mm in inches, which the draft
    // and the document take.
    doc.update(Edit::SetUnits(varde_expr::LengthUnit::In));
    let session = doc.extrude.as_ref().unwrap();
    assert_eq!(session.fields[0].value.as_ref().unwrap().value, 20.0);
    assert_eq!(session.fields[0].error, None);
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.draft_error(), None);
    doc.update(Edit::CommitExtrude);
    assert_eq!(doc.edit_error, None);
    let extrudes = extrudes(&doc);
    assert_eq!(extrudes.len(), 1);
    assert_eq!(extrudes[0].span(), Some((0.0, 20.0)));
}

/// The plate of [`plate`] with its solver lane, being sketched in, the
/// hole selected and deleted, the deletion left with the solver, and the
/// sketch left: the plate's region, picked from the extrude session
/// started then, is to lose its hole.
fn plate_with_the_hole_deleted_waiting() -> (Doc, FeatureId, Requests, crate::tests::SolveLane) {
    let (mut doc, sketch, requests) = plate();
    let mut lane = crate::tests::SolveLane::connect(&mut doc);
    doc.look(Look::EditFeature(sketch));
    lane.answer(&mut doc);
    answer(&mut doc, &requests);
    let Some(FeatureKind::Sketch { sketch: drawn, .. }) =
        doc.editor.document().feature(sketch).map(|f| &f.kind)
    else {
        panic!("no sketch");
    };
    let circle = drawn
        .curves
        .iter()
        .find(|curve| matches!(curve.curve, varde_sketch::Curve::Circle { .. }))
        .unwrap()
        .id;
    doc.look(Look::SelectBox {
        ids: vec![circle],
        add: false,
    });
    doc.update(Edit::DeleteSelection);
    assert!(doc.proposing());
    doc.look(Look::FinishSketch);
    assert!(doc.sketch.is_none());
    assert!(doc.proposing());
    key_in(&mut doc, key("e"));
    let region = plate_region(&doc, sketch);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region });
    (doc, sketch, requests, lane)
}

#[test]
fn ok_waits_for_the_sketch_edits_left_with_the_solver() {
    let (mut doc, sketch, requests, mut lane) = plate_with_the_hole_deleted_waiting();
    let state = doc.extrude_state().unwrap();
    assert!(!state.ready);
    assert!(!state.checking);
    // Neither the screen's Enter nor the field's commits.
    assert!(press_in(&doc, enter()).is_none());
    doc.update(Edit::CommitExtrude);
    assert!(doc.extrude.is_some());
    assert!(extrudes(&doc).is_empty());
    assert_eq!(doc.edit_error, None);

    // Answered, the pick is the plate's region without its hole, which
    // the preview is asked for again.
    lane.answer(&mut doc);
    assert!(!doc.proposing());
    let Some(FeatureKind::Sketch { sketch: drawn, .. }) =
        doc.editor.document().feature(sketch).map(|f| &f.kind)
    else {
        panic!("no sketch");
    };
    let profiles = drawn.profiles().unwrap();
    let [whole] = &profiles.regions[..] else {
        panic!("{:?}", profiles.regions);
    };
    assert!(whole.holes.is_empty());
    assert_eq!(doc.extrude.as_ref().unwrap().picked, BTreeSet::from([0]));
    assert!(doc.extrude_state().unwrap().ready);
    let draft = last_draft(&requests).expect("the preview is asked for again");
    assert_eq!(profiles.resolve(&draft.extrude.regions), [Some(0)]);

    key_in(&mut doc, enter());
    assert!(doc.extrude.is_none());
    let [extrude] = extrudes(&doc)[..] else {
        panic!("one extrude");
    };
    assert_eq!(profiles.resolve(&extrude.regions), [Some(0)]);
    // One undo takes the extrude out and leaves the sketch edit.
    doc.update(Edit::Undo);
    assert!(extrudes(&doc).is_empty());
    let Some(FeatureKind::Sketch { sketch: undone, .. }) =
        doc.editor.document().feature(sketch).map(|f| &f.kind)
    else {
        panic!("no sketch");
    };
    assert_eq!(undone.profiles().unwrap(), profiles);
}

#[test]
fn a_slow_solver_says_checking_in_the_extrude_panel() {
    let (mut doc, _, _, mut lane) = plate_with_the_hole_deleted_waiting();
    assert!(!doc.extrude_state().unwrap().checking);
    // Frames are wanted to tell, outside the sketch too.
    assert!(doc.timing());
    let later = iced::time::Instant::now() + CHECKING + std::time::Duration::from_millis(1);
    doc.tick(later);
    let state = doc.extrude_state().unwrap();
    assert!(state.checking);
    assert!(!state.ready);
    let _ = doc.view(false, Mode::default());
    lane.answer(&mut doc);
    let state = doc.extrude_state().unwrap();
    assert!(!state.checking);
    assert!(state.ready);
}

#[test]
fn undo_while_the_sketch_edits_wait_frees_ok() {
    let (mut doc, sketch, _, mut lane) = plate_with_the_hole_deleted_waiting();
    let before = doc.editor.revision();
    doc.update(Edit::Undo);
    assert!(!doc.proposing());
    assert_eq!(doc.editor.revision(), before);
    let session = doc.extrude.as_ref().unwrap();
    assert_eq!(session.picked, BTreeSet::from([plate_region(&doc, sketch)]));
    assert!(doc.extrude_state().unwrap().ready);
    // The dropped edit's answer changes nothing.
    lane.answer(&mut doc);
    doc.update(Edit::CommitExtrude);
    let [extrude] = extrudes(&doc)[..] else {
        panic!("one extrude");
    };
    let Some(FeatureKind::Sketch { sketch: drawn, .. }) =
        doc.editor.document().feature(sketch).map(|f| &f.kind)
    else {
        panic!("no sketch");
    };
    let profiles = drawn.profiles().unwrap();
    assert_eq!(
        profiles.resolve(&extrude.regions),
        [Some(plate_region(&doc, sketch))]
    );
}
