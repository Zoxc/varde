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
    let profiles = drawn(doc, sketch).profiles().unwrap();
    profiles
        .regions
        .iter()
        .position(|region| region.holes.len() == 1)
        .unwrap()
}

/// The sketch of the sketch feature `sketch` of `doc`'s document.
fn drawn(doc: &Doc, sketch: FeatureId) -> &varde_sketch::Sketch {
    let Some(FeatureKind::Sketch { sketch, .. }) =
        doc.editor.document().feature(sketch).map(|f| &f.kind)
    else {
        panic!("no sketch");
    };
    sketch
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

/// The plate's session, its region picked and its extent `kind`, the
/// first distance typed as `first`.
fn plate_session(kind: ExtentKind, first: &str) -> (Doc, Requests) {
    let (mut doc, sketch, requests) = plate();
    key_in(&mut doc, key("e"));
    let region = plate_region(&doc, sketch);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region });
    extrude(&mut doc, ExtrudeLook::Extent(kind));
    let text = first.to_owned();
    extrude(
        &mut doc,
        ExtrudeLook::Input {
            distance: Distance::First,
            text,
        },
    );
    (doc, requests)
}

#[test]
fn one_side_and_symmetric_knobs_stop_only_where_the_field_would() {
    let first = |doc: &Doc| doc.extrude.as_ref().unwrap().fields[0].value.clone();
    let drag = |to| ExtrudeLook::DragHandle {
        distance: Distance::First,
        to,
    };
    // One side reaches the limit, flipped too.
    let (mut doc, _) = plate_session(ExtentKind::OneSide, "10");
    extrude(&mut doc, ExtrudeLook::GrabHandle(Distance::First));
    extrude(&mut doc, drag(-1_000_000.0));
    assert_eq!(first(&doc).unwrap().value, 1_000_000.0);
    assert!(doc.extrude.as_ref().unwrap().flip);
    assert!(doc.extrude_state().unwrap().ready);
    extrude(&mut doc, drag(1_000_001.0));
    assert_eq!(first(&doc).unwrap().value, 1_000_000.0);

    // Symmetric's knob is half its distance.
    let (mut doc, _) = plate_session(ExtentKind::Symmetric, "10");
    extrude(&mut doc, ExtrudeLook::GrabHandle(Distance::First));
    extrude(&mut doc, drag(500_000.0));
    assert_eq!(first(&doc).unwrap().value, 1_000_000.0);
    assert!(doc.extrude_state().unwrap().ready);
    extrude(&mut doc, drag(-500_001.0));
    assert_eq!(first(&doc).unwrap().value, 1_000_000.0);
}

#[test]
fn an_extrude_over_the_limit_can_be_cancelled() {
    let (mut doc, _) = plate_session(ExtentKind::TwoSides, "600000");
    extrude(
        &mut doc,
        ExtrudeLook::Input {
            distance: Distance::Second,
            text: "600000".to_owned(),
        },
    );
    assert!(doc.extrude_state().unwrap().refused.is_some());
    extrude(&mut doc, ExtrudeLook::Cancel);
    assert!(doc.extrude.is_none());

    let (mut doc, _) = plate_session(ExtentKind::TwoSides, "600000");
    extrude(
        &mut doc,
        ExtrudeLook::Input {
            distance: Distance::Second,
            text: "600000".to_owned(),
        },
    );
    doc.look(Look::Escape);
    assert!(doc.extrude.is_none());
    assert!(extrudes(&doc).is_empty());
}

#[test]
fn two_sides_at_the_limit_stay_ready_when_the_units_change() {
    let (mut doc, _) = plate_session(ExtentKind::TwoSides, "600000");
    extrude(
        &mut doc,
        ExtrudeLook::Input {
            distance: Distance::Second,
            text: "400000".to_owned(),
        },
    );
    assert!(doc.extrude_state().unwrap().ready);
    doc.update(Edit::SetUnits(varde_expr::LengthUnit::In));
    let state = doc.extrude_state().unwrap();
    assert_eq!(state.refused, None);
    assert!(state.ready);
    doc.update(Edit::CommitExtrude);
    assert_eq!(doc.edit_error, None);
    assert_eq!(extrudes(&doc)[0].span(), Some((-400000.0, 600000.0)));
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
    let mut drawn = drawn(&doc, sketch).clone();
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
    assert!(doc.extrude_state().unwrap().ready);
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
    let circle = drawn(&doc, sketch)
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
    let profiles = drawn(&doc, sketch).profiles().unwrap();
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
    assert_eq!(drawn(&doc, sketch).profiles().unwrap(), profiles);
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
    let profiles = drawn(&doc, sketch).profiles().unwrap();
    assert_eq!(
        profiles.resolve(&extrude.regions),
        [Some(plate_region(&doc, sketch))]
    );
}

#[test]
fn the_sketch_deleted_while_its_edits_wait_frees_ok() {
    let (mut doc, sketch, requests, mut lane) = plate_with_the_hole_deleted_waiting();
    // The deletion waits behind the edit, and is made once it's answered.
    doc.update(Edit::RemoveFeature(sketch));
    assert!(doc.editor.document().feature(sketch).is_some());
    lane.answer(&mut doc);
    assert!(doc.editor.document().feature(sketch).is_none());
    // Its session goes with it.
    assert!(doc.extrude.is_none());
    assert!(!doc.proposing());
    assert!(!doc.proposals.slow());
    // Back again, with the edit, a session on it is ready at once.
    doc.update(Edit::Undo);
    assert!(!doc.proposing());
    assert!(doc.editor.document().feature(sketch).is_some());
    answer(&mut doc, &requests);
    doc.look(Look::SelectFeature(sketch));
    key_in(&mut doc, key("e"));
    // The plate alone, its hole deleted.
    let regions = drawn(&doc, sketch).profiles().unwrap().regions;
    let [plate] = &regions[..] else {
        panic!("{regions:?}");
    };
    assert!(plate.holes.is_empty());
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region: 0 });
    assert!(doc.extrude_state().unwrap().ready);
    doc.update(Edit::CommitExtrude);
    assert_eq!(extrudes(&doc).len(), 1);
}

#[test]
fn read_only_while_the_sketch_edits_wait_ends_the_session() {
    let (mut doc, sketch, _, mut lane) = plate_with_the_hole_deleted_waiting();
    let before = drawn(&doc, sketch).clone();
    doc.read_only = Some("test".to_owned());
    doc.sync();
    assert!(doc.extrude.is_none());
    doc.update(Edit::CommitExtrude);
    // The answer can't be committed, and nothing waits any more.
    lane.answer(&mut doc);
    assert!(!doc.proposing());
    assert_eq!(*drawn(&doc, sketch), before);
    assert!(extrudes(&doc).is_empty());
    // Editable again (a Save As to a file of its own), OK is free.
    doc.read_only = None;
    doc.sync();
    key_in(&mut doc, key("e"));
    let region = plate_region(&doc, sketch);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region });
    assert!(doc.extrude_state().unwrap().ready);
}

#[test]
fn recovery_restored_while_the_sketch_edits_wait_frees_ok() {
    let (plate_doc, sketch, _) = plate();
    let document = plate_doc.editor.document().clone();
    let origin = crate::doc::Origin {
        recovered: Some(varde_io::Offer {
            document: document.clone(),
            design_changed: false,
        }),
        ..crate::doc::Origin::new(
            crate::doc::Target::None,
            varde_io::Access::Edit,
            "Design".to_owned(),
        )
    };
    let mut doc = Doc::new(document, origin);
    let requests = Requests::default();
    doc.feed
        .connect(crate::tests::Deferred(Rc::clone(&requests)));
    doc.sync();
    answer(&mut doc, &requests);
    let mut lane = crate::tests::SolveLane::connect(&mut doc);
    doc.look(Look::EditFeature(sketch));
    lane.answer(&mut doc);
    let circle = drawn(&doc, sketch)
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
    doc.look(Look::FinishSketch);
    key_in(&mut doc, key("e"));
    let region = plate_region(&doc, sketch);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region });
    assert!(doc.proposing());
    assert!(!doc.extrude_state().unwrap().ready);

    let _ = doc.restore_recovered(&mut crate::Files::new(None));
    assert!(!doc.proposing());
    // The recovered plate has its hole: the session found its region
    // again, and OK is free.
    let session = doc.extrude.as_ref().unwrap();
    assert_eq!(session.picked, BTreeSet::from([plate_region(&doc, sketch)]));
    assert!(doc.extrude_state().unwrap().ready);
    lane.answer(&mut doc);
    assert_eq!(drawn(&doc, sketch).profiles().unwrap().regions.len(), 2);
    doc.update(Edit::CommitExtrude);
    assert_eq!(extrudes(&doc).len(), 1);
}

#[test]
fn units_changed_while_the_sketch_edits_wait_are_set_once_answered() {
    let (mut doc, sketch, _, mut lane) = plate_with_the_hole_deleted_waiting();
    doc.update(Edit::SetUnits(varde_expr::LengthUnit::In));
    assert_eq!(doc.editor.document().units(), varde_expr::LengthUnit::Mm);
    assert!(!doc.extrude_state().unwrap().ready);
    lane.answer(&mut doc);
    assert!(!doc.proposing());
    assert_eq!(doc.editor.document().units(), varde_expr::LengthUnit::In);
    let state = doc.extrude_state().unwrap();
    assert_eq!(state.refused, None);
    assert!(state.ready);
    doc.update(Edit::CommitExtrude);
    assert_eq!(doc.edit_error, None);
    let [extrude] = extrudes(&doc)[..] else {
        panic!("one extrude");
    };
    // Still the 10 mm it started with.
    assert_eq!(extrude.span(), Some((0.0, 10.0)));
    let profiles = drawn(&doc, sketch).profiles().unwrap();
    assert_eq!(profiles.resolve(&extrude.regions), [Some(0)]);
}

#[test]
fn an_edit_the_solver_rejects_while_waiting_frees_ok_and_keeps_the_pick() {
    let (mut doc, sketch, _) = plate();
    let mut lane = crate::tests::SolveLane::connect(&mut doc);
    doc.look(Look::EditFeature(sketch));
    lane.answer(&mut doc);
    let line = drawn(&doc, sketch)
        .curves
        .iter()
        .find(|curve| matches!(curve.curve, varde_sketch::Curve::Line { .. }))
        .unwrap()
        .id;
    let constrain = |doc: &mut Doc| {
        doc.look(Look::SelectBox {
            ids: vec![line],
            add: false,
        });
        doc.update(Edit::Constrain(varde_view::ConstraintKind::Horizontal));
    };
    constrain(&mut doc);
    lane.answer(&mut doc);
    let committed = drawn(&doc, sketch).clone();
    assert_eq!(committed.constraints.len(), 1);
    // Again restates it, which the solver refuses once the sketch is
    // left.
    constrain(&mut doc);
    assert!(doc.proposing());
    doc.look(Look::FinishSketch);
    key_in(&mut doc, key("e"));
    let region = plate_region(&doc, sketch);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region });
    assert!(!doc.extrude_state().unwrap().ready);
    lane.answer(&mut doc);
    assert!(!doc.proposing());
    assert_eq!(*drawn(&doc, sketch), committed);
    assert_eq!(
        doc.extrude.as_ref().unwrap().picked,
        BTreeSet::from([region])
    );
    assert!(doc.extrude_state().unwrap().ready);
    doc.update(Edit::CommitExtrude);
    assert_eq!(extrudes(&doc).len(), 1);
}

#[test]
fn ok_waits_for_a_solver_lane_not_started_yet() {
    let (mut doc, sketch, requests) = plate();
    doc.look(Look::EditFeature(sketch));
    answer(&mut doc, &requests);
    let circle = drawn(&doc, sketch)
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
    doc.look(Look::FinishSketch);
    assert!(doc.proposing());
    key_in(&mut doc, key("e"));
    let region = plate_region(&doc, sketch);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region });
    assert!(!doc.extrude_state().unwrap().ready);
    let later = iced::time::Instant::now() + CHECKING + std::time::Duration::from_millis(1);
    doc.tick(later);
    assert!(doc.extrude_state().unwrap().checking);
    doc.update(Edit::CommitExtrude);
    assert!(extrudes(&doc).is_empty());

    // The lane starts, gets the edit and answers it.
    let mut lane = crate::tests::SolveLane::connect(&mut doc);
    lane.answer(&mut doc);
    assert!(!doc.proposing());
    let state = doc.extrude_state().unwrap();
    assert!(!state.checking);
    assert!(state.ready);
    assert_eq!(drawn(&doc, sketch).profiles().unwrap().regions.len(), 1);
    doc.update(Edit::CommitExtrude);
    assert_eq!(extrudes(&doc).len(), 1);
}

#[test]
fn a_two_sides_knob_over_the_limit_only_goes_back_towards_it() {
    let (mut doc, _) = plate_session(ExtentKind::TwoSides, "600000");
    extrude(
        &mut doc,
        ExtrudeLook::Input {
            distance: Distance::Second,
            text: "600000".to_owned(),
        },
    );
    assert!(doc.extrude_state().unwrap().refused.is_some());
    extrude(&mut doc, ExtrudeLook::GrabHandle(Distance::Second));
    let drag = |to| ExtrudeLook::DragHandle {
        distance: Distance::Second,
        to,
    };
    let second = |doc: &Doc| doc.extrude.as_ref().unwrap().fields[1].value.clone();
    // Further over, it stays.
    extrude(&mut doc, drag(-700000.0));
    assert_eq!(second(&doc).unwrap().value, 600000.0);
    // Back towards the limit, still over, it follows.
    extrude(&mut doc, drag(-500000.0));
    assert_eq!(second(&doc).unwrap().value, 500000.0);
    assert!(!doc.extrude_state().unwrap().ready);
    extrude(&mut doc, drag(-400000.0));
    assert_eq!(second(&doc).unwrap().value, 400000.0);
    assert!(doc.extrude_state().unwrap().ready);
}

/// The plate with its region extruded by `distance` as a new body,
/// committed, and the extrude.
fn plate_extruded(distance: &str) -> (Doc, FeatureId, Requests) {
    let (mut doc, requests) = plate_session(ExtentKind::OneSide, distance);
    doc.update(Edit::CommitExtrude);
    let feature = doc.editor.document().features().last().unwrap().id;
    answer(&mut doc, &requests);
    (doc, feature, requests)
}

#[test]
fn restoring_a_document_whose_extrude_has_the_edited_id_ends_the_session() {
    let (mut doc, a, requests) = plate_extruded("10");
    // Recovered changes from the same file: another extrude, which took
    // the same id.
    let (recovered, b, _) = plate_extruded("30");
    assert_eq!(a, b);
    let recovered = recovered.editor.document().clone();

    doc.look(Look::EditFeature(a));
    extrude(
        &mut doc,
        ExtrudeLook::Input {
            distance: Distance::First,
            text: "20".to_owned(),
        },
    );
    assert!(last_draft(&requests).is_some());
    // As restoring them does.
    doc.drop_proposals();
    doc.apply(Command::Replace(Box::new(recovered.clone())));
    doc.sync();
    assert!(doc.extrude.is_none());
    assert!(last_draft(&requests).is_none());
    // OK has nothing to write over the recovered extrude.
    doc.update(Edit::CommitExtrude);
    assert_eq!(*doc.editor.document(), recovered);
    let [extrude] = extrudes(&doc)[..] else {
        panic!("one extrude");
    };
    assert_eq!(extrude.span(), Some((0.0, 30.0)));
}

#[test]
fn undoing_a_replacement_ends_a_new_extrude_session() {
    let (mut doc, sketch, requests) = plate();
    // An ordinary edit, then a replacement differing from it.
    let mut drawn = drawn(&doc, sketch).clone();
    drawn.add_point(glam::DVec2::new(100.0, 100.0)).unwrap();
    doc.apply(Command::SetSketch {
        feature: sketch,
        sketch: Box::new(drawn.clone()),
    });
    doc.sync();
    let mut replaced = Editor::new(doc.editor.document().clone());
    drawn.add_point(glam::DVec2::new(110.0, 100.0)).unwrap();
    replaced
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn),
        })
        .unwrap();
    doc.apply(Command::Replace(Box::new(replaced.document().clone())));
    doc.sync();

    doc.look(Look::SelectFeature(sketch));
    doc.look(Look::StartExtrude);
    let region = plate_region(&doc, sketch);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region });
    assert!(last_draft(&requests).is_some());
    // Back across the replacement: the sketch is still there, but its id
    // may name another thing now.
    doc.update(Edit::Undo);
    assert!(is_sketch(doc.editor.document(), sketch));
    assert!(doc.extrude.is_none());
    assert!(last_draft(&requests).is_none());

    // And forward across it again.
    doc.look(Look::SelectFeature(sketch));
    doc.look(Look::StartExtrude);
    assert!(doc.extrude.is_some());
    doc.update(Edit::Redo);
    assert!(doc.extrude.is_none());
}

/// The bodies the extrude panel lists.
fn listed(doc: &Doc) -> Vec<varde_document::BodyId> {
    let state = doc.extrude_state().unwrap();
    state.targets.iter().map(|target| target.body).collect()
}

/// Starts a cut of `example_and_a_hole`'s circle.
fn start_a_cut(doc: &mut Doc, sketch: FeatureId) {
    doc.look(Look::StartExtrude);
    extrude(doc, ExtrudeLook::PickRegion { sketch, region: 0 });
    extrude(doc, ExtrudeLook::Operation(OperationKind::Cut));
}

#[test]
fn a_new_session_doesn_t_list_the_last_one_s_bodies() {
    let (mut doc, sketch, requests) = example_and_a_hole();
    let body = doc.editor.document().bodies()[0].id;
    start_a_cut(&mut doc, sketch);
    answer(&mut doc, &requests);
    assert_eq!(listed(&doc), [body]);

    // Cancelled, and started again before the answer without the draft.
    extrude(&mut doc, ExtrudeLook::Cancel);
    assert!(doc.extrude.is_none());
    doc.look(Look::SelectFeature(sketch));
    start_a_cut(&mut doc, sketch);
    assert!(last_draft(&requests).is_some());
    assert_eq!(listed(&doc), []);
    answer(&mut doc, &requests);
    assert_eq!(listed(&doc), [body]);
}

#[test]
fn switching_to_another_extrude_doesn_t_list_its_bodies() {
    let (mut doc, sketch, requests) = example_and_a_hole();
    let body = doc.editor.document().bodies()[0].id;
    for _ in 0..2 {
        doc.look(Look::SelectFeature(sketch));
        start_a_cut(&mut doc, sketch);
        doc.update(Edit::CommitExtrude);
        assert_eq!(doc.edit_error, None);
    }
    answer(&mut doc, &requests);
    let features = doc.editor.document().features();
    let (first, second) = (features[3].id, features[4].id);
    doc.look(Look::EditFeature(second));
    answer(&mut doc, &requests);
    assert_eq!(listed(&doc), [body]);

    // The other one, straight from the Timeline: nothing until its
    // answer.
    doc.look(Look::EditFeature(first));
    assert_eq!(doc.extrude.as_ref().unwrap().feature, Some(first));
    assert_eq!(listed(&doc), []);
    answer(&mut doc, &requests);
    assert_eq!(listed(&doc), [body]);
}

#[test]
fn new_body_and_back_keeps_the_bodies_listed() {
    let (mut doc, sketch, requests) = example_and_a_hole();
    let body = doc.editor.document().bodies()[0].id;
    start_a_cut(&mut doc, sketch);
    answer(&mut doc, &requests);
    extrude(&mut doc, ExtrudeLook::Operation(OperationKind::NewBody));
    answer(&mut doc, &requests);
    extrude(&mut doc, ExtrudeLook::Operation(OperationKind::Cut));
    assert_eq!(listed(&doc), [body]);
}

#[test]
fn a_draft_the_document_refuses_keeps_the_bodies_listed() {
    let (mut doc, sketch, requests) = example_and_a_hole();
    let body = doc.editor.document().bodies()[0].id;
    start_a_cut(&mut doc, sketch);
    answer(&mut doc, &requests);
    extrude(&mut doc, ExtrudeLook::Extent(ExtentKind::TwoSides));
    for distance in [Distance::First, Distance::Second] {
        let text = "600000".to_owned();
        extrude(&mut doc, ExtrudeLook::Input { distance, text });
    }
    answer(&mut doc, &requests);
    assert!(doc.extrude_state().unwrap().error.is_some());
    assert!(doc.feed.draft_error().is_some());
    assert_eq!(listed(&doc), [body]);
}

/// The plate extruded 10 as extrude A, and then replaced whole by a
/// document from the same base whose extrude, 30, took A's id: as
/// restoring recovered changes does. And A's id.
fn plate_replaced() -> (Doc, FeatureId, Requests, Document, Document) {
    let (mut doc, a, requests) = plate_extruded("10");
    let before = doc.editor.document().clone();
    let (recovered, b, _) = plate_extruded("30");
    assert_eq!(a, b);
    let recovered = recovered.editor.document().clone();
    doc.drop_proposals();
    doc.apply(Command::Replace(Box::new(recovered.clone())));
    doc.sync();
    answer(&mut doc, &requests);
    (doc, a, requests, before, recovered)
}

/// The first distance of the session's extrude edited, typed.
fn first(doc: &Doc) -> String {
    doc.extrude.as_ref().unwrap().fields[0].text.clone()
}

#[test]
fn every_step_across_a_replacement_ends_the_session_opened_before_it() {
    let (mut doc, a, requests, before, recovered) = plate_replaced();
    // Opened after the restore, on the recovered extrude: undoing the
    // restore ends it, as the id names the one replaced again.
    doc.look(Look::EditFeature(a));
    assert_eq!(first(&doc), "30");
    doc.update(Edit::Undo);
    assert_eq!(*doc.editor.document(), before);
    assert!(doc.extrude.is_none());
    assert!(last_draft(&requests).is_none());
    // Redone, then undone again: each ends the session opened in between.
    doc.look(Look::EditFeature(a));
    assert_eq!(first(&doc), "10");
    doc.update(Edit::Redo);
    assert_eq!(*doc.editor.document(), recovered);
    assert!(doc.extrude.is_none());
    doc.look(Look::EditFeature(a));
    assert_eq!(first(&doc), "30");
    extrude(
        &mut doc,
        ExtrudeLook::Input {
            distance: Distance::First,
            text: "20".to_owned(),
        },
    );
    doc.update(Edit::Undo);
    assert!(doc.extrude.is_none());
    assert!(last_draft(&requests).is_none());
    // OK then has nothing to write.
    doc.update(Edit::CommitExtrude);
    assert_eq!(*doc.editor.document(), before);
    // A session opened on this side of it stays through an ordinary edit
    // and its undo.
    doc.look(Look::EditFeature(a));
    doc.update(Edit::SetTolerance(Tolerance::new(1e-2).unwrap()));
    doc.update(Edit::Undo);
    assert_eq!(*doc.editor.document(), before);
    assert_eq!(first(&doc), "10");
}

#[test]
fn restoring_drops_the_session_and_the_changes_waiting() {
    let (opened, a, _) = plate_extruded("10");
    let opened = opened.editor.document().clone();
    let (recovered, _, _) = plate_extruded("30");
    let recovered = recovered.editor.document().clone();
    let sketch = opened.features()[0].id;
    let units = opened.units();
    let origin = crate::doc::Origin {
        recovered: Some(varde_io::Offer {
            document: recovered.clone(),
            design_changed: false,
        }),
        ..crate::doc::Origin::new(
            crate::doc::Target::None,
            varde_io::Access::Edit,
            "Design".to_owned(),
        )
    };
    let mut doc = Doc::new(opened, origin);
    let requests = Requests::default();
    doc.feed
        .connect(crate::tests::Deferred(Rc::clone(&requests)));
    doc.sync();
    answer(&mut doc, &requests);
    let mut lane = crate::tests::SolveLane::connect(&mut doc);
    // An edit waiting on the solver.
    doc.look(Look::EditFeature(sketch));
    lane.answer(&mut doc);
    let circle = drawn(&doc, sketch)
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
    doc.look(Look::FinishSketch);
    assert!(doc.proposing());
    // The extrude being edited, and new units and a delete waiting
    // behind the edit.
    doc.look(Look::EditFeature(a));
    assert!(doc.extrude.is_some());
    doc.update(Edit::SetUnits(varde_expr::LengthUnit::In));
    doc.update(Edit::RemoveFeature(sketch));
    assert!(doc.delete_prompt().is_none());
    assert_eq!(doc.editor.document().units(), units);

    let _ = doc.restore_recovered(&mut crate::Files::new(None));
    assert_eq!(*doc.editor.document(), recovered);
    assert!(doc.extrude.is_none());
    assert!(last_draft(&requests).is_none());
    assert!(doc.delete_prompt().is_none());
    assert!(doc.deleting.is_none());
    // The dropped edit's answer sets nothing, and what waited behind it
    // was dropped too.
    lane.answer(&mut doc);
    assert!(!doc.proposing());
    assert_eq!(*doc.editor.document(), recovered);
    doc.update(Edit::ConfirmDelete);
    doc.update(Edit::CommitExtrude);
    assert_eq!(*doc.editor.document(), recovered);
}

#[test]
fn undo_and_redo_mid_session_keep_the_bodies_listed() {
    let (mut doc, sketch, requests) = example_and_a_hole();
    let body = doc.editor.document().bodies()[0].id;
    start_a_cut(&mut doc, sketch);
    answer(&mut doc, &requests);
    assert_eq!(listed(&doc), [body]);
    let revision = last_draft(&requests).map(|draft| draft.revision);
    doc.update(Edit::SetTolerance(Tolerance::new(1e-2).unwrap()));
    assert_eq!(listed(&doc), [body]);
    answer(&mut doc, &requests);
    for edit in [Edit::Undo, Edit::Redo, Edit::Undo] {
        doc.update(edit);
        assert!(doc.extrude.is_some());
        // The same draft, asked for of the document undone or redone.
        let draft = last_draft(&requests).unwrap();
        assert!(revision.is_none() || Some(draft.revision) >= revision);
        assert_eq!(listed(&doc), [body]);
        answer(&mut doc, &requests);
        assert_eq!(listed(&doc), [body]);
    }
}

#[test]
fn a_body_taken_out_and_undone_away_leaves_the_draft_whole() {
    let (mut doc, sketch, requests) = example_and_a_hole();
    let body = doc.editor.document().bodies()[0].id;
    // A second body, a peg through the hole's circle.
    doc.look(Look::StartExtrude);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region: 0 });
    extrude(&mut doc, ExtrudeLook::Extent(ExtentKind::Symmetric));
    extrude(
        &mut doc,
        ExtrudeLook::Input {
            distance: Distance::First,
            text: "100".to_owned(),
        },
    );
    doc.update(Edit::CommitExtrude);
    assert_eq!(doc.edit_error, None);
    let peg = doc.editor.document().bodies()[1].id;
    answer(&mut doc, &requests);

    // A cut of the circle, a wider one, taking the peg out.
    doc.look(Look::SelectFeature(sketch));
    start_a_cut(&mut doc, sketch);
    extrude(&mut doc, ExtrudeLook::Extent(ExtentKind::ThroughAll));
    answer(&mut doc, &requests);
    assert_eq!(listed(&doc), [body, peg]);
    extrude(&mut doc, ExtrudeLook::Target(peg));
    answer(&mut doc, &requests);
    assert_eq!(doc.extrude.as_ref().unwrap().excluded, [peg]);
    assert_eq!(listed(&doc), [body, peg]);
    assert_eq!(doc.feed.draft_error(), None);

    // Undone away, the peg is neither listed nor named by the draft,
    // which the document takes.
    doc.update(Edit::Undo);
    assert!(doc.editor.document().body(peg).is_none());
    assert!(doc.extrude.is_some());
    assert_eq!(doc.extrude.as_ref().unwrap().excluded, []);
    let draft = last_draft(&requests).unwrap();
    assert_eq!(draft.extrude.operation, Operation::Cut(Default::default()));
    assert_eq!(listed(&doc), [body]);
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.draft_error(), None);
    assert_eq!(listed(&doc), [body]);
    // Redone, it's back but not taken out again: after an undo, a new
    // edit could have given its id to another body. The list shows it.
    doc.update(Edit::Redo);
    assert!(doc.editor.document().body(peg).is_some());
    assert_eq!(doc.extrude.as_ref().unwrap().excluded, []);
    answer(&mut doc, &requests);
    assert_eq!(listed(&doc), [body, peg]);
    assert!(doc.extrude_state().unwrap().targets[1].included);
    doc.update(Edit::CommitExtrude);
    assert_eq!(doc.edit_error, None);
    assert!(doc.extrude.is_none());
}

#[test]
fn timeline_a_b_a_lists_only_the_extrude_edited() {
    let (mut doc, sketch, requests) = example_and_a_hole();
    let body = doc.editor.document().bodies()[0].id;
    for _ in 0..2 {
        doc.look(Look::SelectFeature(sketch));
        start_a_cut(&mut doc, sketch);
        doc.update(Edit::CommitExtrude);
        assert_eq!(doc.edit_error, None);
    }
    answer(&mut doc, &requests);
    let features = doc.editor.document().features();
    let (a, b) = (features[3].id, features[4].id);
    doc.look(Look::EditFeature(a));
    answer(&mut doc, &requests);
    assert_eq!(listed(&doc), [body]);

    // To B and straight back to A, neither answered: A's earlier answer
    // is of another run.
    doc.look(Look::EditFeature(b));
    assert_eq!(listed(&doc), []);
    doc.look(Look::EditFeature(a));
    assert_eq!(doc.extrude.as_ref().unwrap().feature, Some(a));
    assert_eq!(listed(&doc), []);
    // B's answer is dropped; A's lists the body.
    let b_request = requests.borrow_mut().remove(0);
    doc.computed(varde_regen::handle(b_request));
    assert_eq!(listed(&doc), []);
    answer(&mut doc, &requests);
    assert_eq!(listed(&doc), [body]);
}

#[test]
fn a_delete_waiting_behind_sketch_edits_asks_once_it_is_made() {
    let (mut doc, a, _) = plate_extruded("10");
    let sketch = doc.editor.document().features()[0].id;
    let mut lane = crate::tests::SolveLane::connect(&mut doc);
    doc.look(Look::EditFeature(sketch));
    lane.answer(&mut doc);
    let circle = drawn(&doc, sketch)
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
    doc.look(Look::FinishSketch);
    assert!(doc.proposing());
    // The extrude goes with the sketch: asked once the edit is answered.
    doc.update(Edit::RemoveFeature(sketch));
    assert!(doc.delete_prompt().is_none());
    lane.answer(&mut doc);
    assert!(!doc.proposing());
    let circles = |doc: &Doc| {
        drawn(doc, sketch)
            .curves
            .iter()
            .filter(|curve| matches!(curve.curve, varde_sketch::Curve::Circle { .. }))
            .count()
    };
    assert_eq!(circles(&doc), 0);
    let prompt = doc.delete_prompt().unwrap();
    assert_eq!(prompt.features.len(), 2);
    doc.update(Edit::ConfirmDelete);
    assert!(doc.editor.document().features().is_empty());
    // Undone in order: the deletion, then the edit.
    doc.update(Edit::Undo);
    assert!(doc.editor.document().feature(a).is_some());
    assert_eq!(circles(&doc), 0);
    doc.update(Edit::Undo);
    assert_eq!(circles(&doc), 1);
}

/// The plate extruded, its sketch edited with the circle deleted and then
/// a line, and the sketch's deletion asked for between them, all waiting
/// on the solver (five curves committed): the lane, the sketch and the
/// extrude.
fn a_delete_between_sketch_edits() -> (Doc, crate::tests::SolveLane, FeatureId, FeatureId) {
    let (mut doc, a, _) = plate_extruded("10");
    let sketch = doc.editor.document().features()[0].id;
    let mut lane = crate::tests::SolveLane::connect(&mut doc);
    doc.look(Look::EditFeature(sketch));
    lane.answer(&mut doc);
    let find = |doc: &Doc, circle: bool| {
        drawn(doc, sketch)
            .curves
            .iter()
            .find(|curve| matches!(curve.curve, varde_sketch::Curve::Circle { .. }) == circle)
            .unwrap()
            .id
    };
    let (circle, line) = (find(&doc, true), find(&doc, false));
    for id in [circle, line] {
        doc.look(Look::SelectBox {
            ids: vec![id],
            add: false,
        });
        doc.update(Edit::DeleteSelection);
        if id == circle {
            doc.update(Edit::RemoveFeature(sketch));
        }
    }
    assert!(doc.proposing());
    assert_eq!(drawn(&doc, sketch).curves.len(), 5);
    (doc, lane, sketch, a)
}

#[test]
fn confirming_the_delete_prompt_while_edits_wait_deletes_after_them() {
    let (mut doc, mut lane, sketch, a) = a_delete_between_sketch_edits();
    // The circle's deletion is answered: the delete is made, and asks,
    // while the line's still waits.
    lane.answer_first(&mut doc);
    assert_eq!(drawn(&doc, sketch).curves.len(), 4);
    assert!(doc.proposing());
    assert_eq!(doc.delete_prompt().unwrap().features.len(), 2);
    // Confirmed, it waits behind the line's.
    doc.update(Edit::ConfirmDelete);
    assert!(doc.delete_prompt().is_none());
    assert!(doc.editor.document().feature(sketch).is_some());
    lane.answer(&mut doc);
    assert!(!doc.proposing());
    assert!(doc.editor.document().features().is_empty());
    // Undone in order: the deletion, the line, the circle.
    doc.update(Edit::Undo);
    assert!(doc.editor.document().feature(a).is_some());
    assert_eq!(drawn(&doc, sketch).curves.len(), 3);
    doc.update(Edit::Undo);
    assert_eq!(drawn(&doc, sketch).curves.len(), 4);
    doc.update(Edit::Undo);
    assert_eq!(drawn(&doc, sketch).curves.len(), 5);
}

#[test]
fn the_delete_prompt_stays_while_the_edits_behind_it_commit() {
    let (mut doc, mut lane, sketch, _) = a_delete_between_sketch_edits();
    lane.answer(&mut doc);
    assert!(!doc.proposing());
    assert_eq!(drawn(&doc, sketch).curves.len(), 3);
    assert_eq!(doc.delete_prompt().unwrap().features.len(), 2);
    doc.update(Edit::ConfirmDelete);
    assert!(doc.editor.document().features().is_empty());
}
