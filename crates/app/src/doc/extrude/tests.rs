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
    // Typed "10", it shows with its unit, as a new extrude's does.
    assert_eq!(session.fields[0].text, "10 mm");
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

/// An intersect that would leave nothing of the plate: the draft says
/// why, and committed it's marked failed in the Timeline while the plate
/// stays drawn, rather than an empty body and an empty mesh.
#[test]
fn an_intersect_leaving_nothing_is_marked_failed_and_the_plate_kept() {
    let (mut doc, sketch, requests) = example_and_a_hole();
    let triangles = doc.feed.mesh().triangle_count();
    assert!(triangles > 0);
    doc.look(Look::StartExtrude);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region: 0 });
    extrude(&mut doc, ExtrudeLook::Operation(OperationKind::Intersect));
    extrude(&mut doc, ExtrudeLook::Flip);
    answer(&mut doc, &requests);
    let emptied = "intersecting it with Body 1 leaves nothing";
    let error = doc.feed.draft_error().unwrap();
    assert!(error.starts_with(emptied), "{error}");
    assert_eq!(doc.feed.mesh().triangle_count(), triangles);
    let _ = doc.view(false, Mode::default());

    doc.update(Edit::CommitExtrude);
    answer(&mut doc, &requests);
    let feature = doc.editor.document().features().last().unwrap().id;
    let failed = doc.feed.failed_features();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].0, feature);
    assert!(failed[0].1.starts_with(emptied), "{}", failed[0].1);
    assert_eq!(doc.feed.mesh().triangle_count(), triangles);
    doc.look(Look::SelectPanel(varde_view::Panel::Timeline));
    let _ = doc.view(false, Mode::default());
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

#[test]
fn a_body_ticked_again_stays_listed_until_the_answer() {
    let (mut doc, sketch, requests) = example_and_a_hole();
    let body = doc.editor.document().bodies()[0].id;
    start_a_cut(&mut doc, sketch);
    answer(&mut doc, &requests);
    assert_eq!(listed(&doc), [body]);

    // Taken out and answered: the touch test found nothing to touch.
    extrude(&mut doc, ExtrudeLook::Target(body));
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.draft_touched(), []);
    assert_eq!(listed(&doc), [body]);

    // Ticked again: still listed while its touch test is on its way,
    // and after it, which finds it touched.
    extrude(&mut doc, ExtrudeLook::Target(body));
    assert!(doc.extrude.as_ref().unwrap().excluded.is_empty());
    assert_eq!(listed(&doc), [body]);
    assert!(doc.extrude_state().unwrap().targets[0].included);
    answer(&mut doc, &requests);
    assert_eq!(listed(&doc), [body]);

    // Out and in again before any answer: listed throughout.
    extrude(&mut doc, ExtrudeLook::Target(body));
    answer(&mut doc, &requests);
    extrude(&mut doc, ExtrudeLook::Target(body));
    extrude(&mut doc, ExtrudeLook::Target(body));
    extrude(&mut doc, ExtrudeLook::Target(body));
    assert_eq!(listed(&doc), [body]);
    answer(&mut doc, &requests);
    assert_eq!(listed(&doc), [body]);
}

#[test]
fn a_body_ticked_again_that_isn_t_touched_goes_with_the_answer() {
    let (mut doc, sketch, requests) = example_and_a_hole();
    let body = doc.editor.document().bodies()[0].id;
    start_a_cut(&mut doc, sketch);
    answer(&mut doc, &requests);
    extrude(&mut doc, ExtrudeLook::Target(body));
    answer(&mut doc, &requests);
    // The circle moved clear of the body and the body ticked again:
    // listed until the answer says it isn't touched.
    let mut drawn = drawn(&doc, sketch).clone();
    drawn.points[0].at = glam::DVec2::new(-500.0, 10.0);
    doc.apply(Command::SetSketch {
        feature: sketch,
        sketch: Box::new(drawn),
    });
    doc.sync();
    assert_eq!(doc.extrude.as_ref().unwrap().picked.len(), 1);
    extrude(&mut doc, ExtrudeLook::Target(body));
    assert_eq!(listed(&doc), [body]);
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.draft_touched(), []);
    assert_eq!(listed(&doc), []);
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
    assert_eq!(first(&doc), "30 mm");
    doc.update(Edit::Undo);
    assert_eq!(*doc.editor.document(), before);
    assert!(doc.extrude.is_none());
    assert!(last_draft(&requests).is_none());
    // Redone, then undone again: each ends the session opened in between.
    doc.look(Look::EditFeature(a));
    assert_eq!(first(&doc), "10 mm");
    doc.update(Edit::Redo);
    assert_eq!(*doc.editor.document(), recovered);
    assert!(doc.extrude.is_none());
    doc.look(Look::EditFeature(a));
    assert_eq!(first(&doc), "30 mm");
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
    assert_eq!(first(&doc), "10 mm");
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
fn a_body_ticked_again_and_undone_away_is_forgotten() {
    let (mut doc, sketch, requests) = example_and_a_hole();
    // A second body, a peg through the hole's circle, as the newest edit.
    doc.look(Look::StartExtrude);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region: 0 });
    extrude(&mut doc, ExtrudeLook::Extent(ExtentKind::Symmetric));
    doc.update(Edit::CommitExtrude);
    assert_eq!(doc.edit_error, None);
    let peg = doc.editor.document().bodies()[1].id;
    answer(&mut doc, &requests);

    // Taken out of a cut and put back, then undone away: it's forgotten
    // with the bodies taken out, so the session holds no more entries
    // than the document has bodies.
    doc.look(Look::SelectFeature(sketch));
    start_a_cut(&mut doc, sketch);
    answer(&mut doc, &requests);
    extrude(&mut doc, ExtrudeLook::Target(peg));
    answer(&mut doc, &requests);
    extrude(&mut doc, ExtrudeLook::Target(peg));
    assert_eq!(doc.extrude.as_ref().unwrap().reticked.len(), 1);
    doc.update(Edit::Undo);
    assert!(doc.editor.document().body(peg).is_none());
    let session = doc.extrude.as_ref().unwrap();
    assert_eq!(session.excluded, []);
    assert_eq!(session.reticked, []);
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
    // Waiting for the answer still, which closing or saving waits for.
    assert!(doc.proposing());
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
    assert!(!doc.proposing());
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
fn a_delete_from_the_queue_confirmed_is_made_in_its_turn() {
    let (mut doc, mut lane, sketch, a) = a_delete_between_sketch_edits();
    // The circle's deletion is answered: the delete is made, and asks,
    // and the line's waits for the answer.
    lane.answer(&mut doc);
    assert_eq!(drawn(&doc, sketch).curves.len(), 4);
    assert!(doc.proposing());
    assert!(lane.waiting().is_empty());
    assert_eq!(doc.delete_prompt().unwrap().features.len(), 2);
    // Confirmed, it's made at once; the line's deletion, made after it,
    // has no sketch left to go to, as after a delete that asks nothing.
    doc.update(Edit::ConfirmDelete);
    assert!(doc.delete_prompt().is_none());
    assert!(doc.editor.document().features().is_empty());
    lane.answer(&mut doc);
    assert!(!doc.proposing());
    // Undone in order: the deletion, the circle.
    doc.update(Edit::Undo);
    assert!(doc.editor.document().feature(a).is_some());
    assert_eq!(drawn(&doc, sketch).curves.len(), 4);
    doc.update(Edit::Undo);
    assert_eq!(drawn(&doc, sketch).curves.len(), 5);
}

#[test]
fn the_edits_behind_a_delete_from_the_queue_wait_for_its_answer() {
    let (mut doc, mut lane, sketch, _) = a_delete_between_sketch_edits();
    lane.answer(&mut doc);
    assert_eq!(drawn(&doc, sketch).curves.len(), 4);
    assert_eq!(doc.delete_prompt().unwrap().features.len(), 2);
    // Cancelled, the line's deletion goes on.
    doc.look(Look::CancelDelete);
    assert!(doc.proposing());
    lane.answer(&mut doc);
    assert!(!doc.proposing());
    assert_eq!(drawn(&doc, sketch).curves.len(), 3);
    assert!(doc.delete_prompt().is_none());
}

/// The example, a hole sketched and cut through its body, and a point
/// added to the hole's sketch waiting on the solver, with the hole's
/// deletion and then the example sketch's asked for behind it: the lane,
/// the hole's sketch and the example's.
fn two_deletes_waiting() -> (Doc, crate::tests::SolveLane, FeatureId, FeatureId) {
    let (mut doc, hole, requests) = example_and_a_hole();
    start_a_cut(&mut doc, hole);
    extrude(&mut doc, ExtrudeLook::Extent(ExtentKind::ThroughAll));
    doc.update(Edit::CommitExtrude);
    answer(&mut doc, &requests);
    assert_eq!(doc.editor.document().features().len(), 4);
    let example = doc.editor.document().features()[0].id;
    let mut lane = crate::tests::SolveLane::connect(&mut doc);
    doc.look(Look::EditFeature(hole));
    lane.answer(&mut doc);
    doc.look(Look::SelectTool(varde_view::Tool::Point));
    point_at(&mut doc, 30.0, 30.0);
    doc.look(Look::FinishSketch);
    assert!(doc.proposing());
    doc.update(Edit::RemoveFeature(hole));
    doc.update(Edit::RemoveFeature(example));
    assert!(doc.delete_prompt().is_none());
    assert_eq!(doc.editor.document().features().len(), 4);
    (doc, lane, hole, example)
}

/// Clicks the Point tool at `x`, `y` on nothing.
fn point_at(doc: &mut Doc, x: f64, y: f64) {
    doc.update(Edit::ToolClick(varde_view::ToolClick {
        at: glam::DVec2::new(x, y),
        target: None,
        inference: None,
        hit: None,
        pixel: 0.1,
        double: false,
        reference: false,
    }));
}

/// The points of the sketch feature `sketch`, if it's there.
fn points(doc: &Doc, sketch: FeatureId) -> Option<usize> {
    match &doc.editor.document().feature(sketch)?.kind {
        FeatureKind::Sketch { sketch, .. } => Some(sketch.points.len()),
        FeatureKind::Extrude(_) => None,
    }
}

#[test]
fn two_deletes_waiting_that_ask_are_asked_in_turn() {
    let (mut doc, mut lane, hole, example) = two_deletes_waiting();
    lane.answer(&mut doc);
    assert_eq!(points(&doc, hole), Some(2));
    // The hole's first, the other waiting for the answer.
    let prompt = doc.delete_prompt().unwrap();
    assert_eq!(prompt.name, "Sketch 2");
    assert_eq!(prompt.features.len(), 2);
    assert!(doc.proposing());
    doc.update(Edit::ConfirmDelete);
    assert!(doc.editor.document().feature(hole).is_none());
    // Then the example's.
    let prompt = doc.delete_prompt().unwrap();
    assert_eq!(
        prompt.name,
        doc.editor.document().feature(example).unwrap().name
    );
    assert_eq!(prompt.features.len(), 2);
    assert!(doc.proposing());
    doc.update(Edit::ConfirmDelete);
    assert!(!doc.proposing());
    assert!(doc.editor.document().features().is_empty());
    // Undone in order.
    doc.update(Edit::Undo);
    assert_eq!(doc.editor.document().features().len(), 2);
    doc.update(Edit::Undo);
    assert_eq!(doc.editor.document().features().len(), 4);
    assert_eq!(points(&doc, hole), Some(2));
    doc.update(Edit::Undo);
    assert_eq!(points(&doc, hole), Some(1));
}

#[test]
fn a_delete_waiting_cancelled_asks_the_next() {
    let (mut doc, mut lane, hole, example) = two_deletes_waiting();
    lane.answer(&mut doc);
    assert!(doc.delete_prompt().is_some());
    doc.look(Look::CancelDelete);
    assert!(doc.editor.document().feature(hole).is_some());
    let prompt = doc.delete_prompt().unwrap();
    assert_eq!(
        prompt.name,
        doc.editor.document().feature(example).unwrap().name
    );
    // Escape cancels it as well.
    doc.look(Look::Escape);
    assert!(doc.delete_prompt().is_none());
    assert!(!doc.proposing());
    assert_eq!(doc.editor.document().features().len(), 4);
}

#[test]
fn nothing_waiting_moves_while_a_delete_from_the_queue_asks() {
    let (mut doc, mut lane, hole, _) = two_deletes_waiting();
    // Another point, behind the deletes.
    doc.look(Look::EditFeature(hole));
    lane.answer(&mut doc);
    doc.look(Look::SelectTool(varde_view::Tool::Point));
    point_at(&mut doc, 40.0, 40.0);
    lane.answer(&mut doc);
    assert!(doc.delete_prompt().is_some());
    assert_eq!(points(&doc, hole), Some(2), "the second point waits");
    assert!(lane.waiting().is_empty());
    // Not "Checking…" while the user is asked.
    doc.tick(std::time::Instant::now() + CHECKING * 2);
    assert!(!doc.proposals.slow());
    // Undo takes back the newest first: the point, the example's
    // deletion, then the question.
    doc.update(Edit::Undo);
    doc.update(Edit::Undo);
    assert!(doc.delete_prompt().is_some());
    assert!(doc.proposing());
    doc.update(Edit::Undo);
    assert!(doc.delete_prompt().is_none());
    assert!(!doc.proposing());
    assert_eq!(doc.editor.document().features().len(), 4);
    assert_eq!(points(&doc, hole), Some(2));
    doc.update(Edit::Undo);
    assert_eq!(points(&doc, hole), Some(1));
}

#[test]
fn closing_waits_for_a_delete_waiting_to_be_asked_and_answered() {
    let (mut doc, mut lane, hole, example) = two_deletes_waiting();
    let mut files = crate::Files::new(None);
    assert!(matches!(
        doc.leave(&mut files, crate::doc::Leave::Close),
        crate::Next::Stay
    ));
    lane.answer(&mut doc);
    assert!(matches!(
        doc.proposals_settled(&mut files),
        crate::Next::Stay
    ));
    assert!(doc.delete_prompt().is_some());
    doc.update(Edit::ConfirmDelete);
    assert!(matches!(
        doc.proposals_settled(&mut files),
        crate::Next::Stay
    ));
    assert!(doc.editor.document().feature(hole).is_none());
    assert!(doc.delete_prompt().is_some());
    doc.look(Look::CancelDelete);
    assert!(doc.editor.document().feature(example).is_some());
    // Then it asks about the unsaved changes, the deletion among them.
    let _ = doc.proposals_settled(&mut files);
    assert_eq!(doc.prompt(), Some(crate::doc::Leave::Close));
}

#[test]
fn a_restarted_solver_lane_gets_the_edit_the_old_one_had() {
    let (mut doc, lane, hole, _) = two_deletes_waiting();
    // The lane goes before answering, and another starts in its place.
    assert_eq!(lane.waiting().len(), 1);
    drop(lane);
    let mut again = crate::tests::SolveLane::connect(&mut doc);
    again.answer(&mut doc);
    assert_eq!(points(&doc, hole), Some(2));
    assert!(doc.delete_prompt().is_some());
}

#[test]
fn read_only_while_a_delete_from_the_queue_asks_deletes_nothing() {
    let (mut doc, mut lane, _, _) = two_deletes_waiting();
    lane.answer(&mut doc);
    assert!(doc.delete_prompt().is_some());
    let before = doc.editor.document().clone();
    doc.read_only = Some("test".to_owned());
    doc.sync();
    // Confirmed, it can't be made, nor can the one behind it, which asks
    // nothing.
    doc.update(Edit::ConfirmDelete);
    assert!(doc.delete_prompt().is_none());
    assert!(!doc.proposing());
    assert_eq!(*doc.editor.document(), before);
}

#[test]
fn read_only_before_the_answer_makes_none_of_what_waits() {
    let (mut doc, mut lane, hole, _) = two_deletes_waiting();
    let before = doc.editor.document().clone();
    doc.read_only = Some("test".to_owned());
    doc.sync();
    lane.answer(&mut doc);
    assert!(!doc.proposing());
    assert!(doc.delete_prompt().is_none());
    assert_eq!(*doc.editor.document(), before);
    assert_eq!(points(&doc, hole), Some(1));
}

#[test]
fn restoring_while_a_delete_from_the_queue_asks_drops_the_question() {
    let (mut doc, mut lane, _, _) = two_deletes_waiting();
    lane.answer(&mut doc);
    assert!(doc.delete_prompt().is_some());
    let (recovered, _, _) = plate_extruded("30");
    let recovered = recovered.editor.document().clone();
    // As restoring recovered changes does.
    doc.drop_proposals();
    doc.apply(Command::Replace(Box::new(recovered.clone())));
    doc.sync();
    assert!(doc.delete_prompt().is_none());
    assert!(doc.deleting.is_none());
    assert!(!doc.proposing());
    lane.answer(&mut doc);
    assert_eq!(*doc.editor.document(), recovered);
}

#[test]
fn an_edit_made_while_a_delete_prompt_is_up_waits_for_its_answer() {
    let (mut doc, a, _) = plate_extruded("10");
    let sketch = doc.editor.document().features()[0].id;
    let mut lane = crate::tests::SolveLane::connect(&mut doc);
    doc.look(Look::EditFeature(sketch));
    lane.answer(&mut doc);
    doc.update(Edit::RemoveFeature(sketch));
    assert!(doc.delete_prompt().is_some());
    // Behind the prompt, if anything gets there.
    doc.look(Look::SelectTool(varde_view::Tool::Point));
    point_at(&mut doc, 30.0, 30.0);
    assert!(doc.proposing());
    assert!(lane.waiting().is_empty());
    doc.look(Look::CancelDelete);
    lane.answer(&mut doc);
    assert!(!doc.proposing());
    assert!(doc.editor.document().feature(a).is_some());
    assert!(
        drawn(&doc, sketch)
            .points
            .iter()
            .any(|p| p.at == glam::DVec2::new(30.0, 30.0))
    );
}

/// `count` circles round a place left of the example's plate, the
/// innermost last: fifteen hundred, a good share of the work one
/// sketch's profiles may take; three thousand, more than that.
fn concentric(count: usize) -> varde_sketch::Sketch {
    let mut drawn = varde_sketch::Sketch::default();
    let center = drawn.add_point(glam::DVec2::new(-40.0, 0.0)).unwrap();
    for k in (1..=count).rev() {
        let radius = k as f64 * 0.01;
        let circle = varde_sketch::Curve::Circle { center, radius };
        drawn.add_curve(circle, false).unwrap();
    }
    drawn
}

/// Adds `drawn` to `doc`'s document as a visible sketch.
fn add_visible(doc: &mut Doc, drawn: varde_sketch::Sketch) -> FeatureId {
    let plane = varde_document::Plane::Origin(varde_document::OriginPlane::XY);
    doc.apply(doc.editor.document().add_sketch(plane));
    let sketch = doc.editor.document().features().last().unwrap().id;
    doc.apply(Command::SetSketch {
        feature: sketch,
        sketch: Box::new(drawn),
    });
    doc.apply(Command::SetFeatureVisible(sketch, true));
    sketch
}

/// Seconds the UI may take over a hostile sketch's profiles: a tenth of
/// a second or so in a release build, some twenty times that
/// unoptimised, and more again on a loaded machine; it took seconds
/// released.
fn ui_bound() -> f64 {
    if cfg!(debug_assertions) { 30.0 } else { 1.0 }
}

#[test]
fn a_sketch_too_complex_is_skipped_quickly_and_once() {
    let (mut doc, plate, requests) = plate();
    let hostile = add_visible(&mut doc, concentric(3000));
    doc.sync();
    answer(&mut doc, &requests);
    let started = std::time::Instant::now();
    doc.look(Look::StartExtrude);
    let took = started.elapsed().as_secs_f64();
    assert!(took < ui_bound(), "{took}");
    let session = doc.extrude.as_ref().unwrap();
    assert_eq!(session.source, None);
    let found: Vec<FeatureId> = session.found.iter().map(|found| found.feature).collect();
    assert_eq!(found, vec![plate]);
    assert_eq!(session.skipped.len(), 1);
    assert_eq!(session.skipped[0].0, hostile);
    assert_eq!(session.worked_out, 2);
    // Changes to the document elsewhere don't work it out again.
    let region = plate_region(&doc, plate);
    extrude(
        &mut doc,
        ExtrudeLook::PickRegion {
            sketch: plate,
            region,
        },
    );
    extrude(
        &mut doc,
        ExtrudeLook::PickRegion {
            sketch: plate,
            region,
        },
    );
    let flip = doc.extrude.as_ref().unwrap().flip;
    doc.apply(Command::SetFeatureVisible(plate, true));
    doc.sync();
    let session = doc.extrude.as_ref().unwrap();
    assert_eq!(session.flip, flip);
    assert_eq!(session.source, None);
    assert_eq!(session.skipped.len(), 1);
    assert_eq!(session.worked_out, 2);
    // Selected, it's the source, which has no regions to pick.
    doc.look(Look::StartExtrude);
    doc.look(Look::SelectFeature(hostile));
    let started = std::time::Instant::now();
    doc.look(Look::StartExtrude);
    let took = started.elapsed().as_secs_f64();
    assert!(took < ui_bound(), "{took}");
    let session = doc.extrude.as_ref().unwrap();
    assert_eq!(session.source, Some(hostile));
    assert!(session.found.is_empty());
    assert_eq!(session.skipped.len(), 1);
}

#[test]
fn the_visible_sketches_share_the_work() {
    let (mut doc, plate, requests) = plate();
    // Each takes a good share of what one may: those past what all may
    // together have no regions to pick.
    let drawn = concentric(1500);
    let mut left = usize::MAX;
    drawn.profiles_spending(&mut left).unwrap();
    let each = usize::MAX - left;
    let fit = REFRESH_WORK / each;
    assert!((2..=6).contains(&fit), "{each}");
    let sketches: Vec<FeatureId> = (0..fit + 2)
        .map(|_| add_visible(&mut doc, drawn.clone()))
        .collect();
    doc.sync();
    answer(&mut doc, &requests);
    let started = std::time::Instant::now();
    doc.look(Look::StartExtrude);
    let took = started.elapsed().as_secs_f64();
    assert!(took < ui_bound(), "{took}");
    let session = doc.extrude.as_ref().unwrap();
    let found: Vec<FeatureId> = session.found.iter().map(|found| found.feature).collect();
    // The plate's is cheap, but worked out first, so it eats into the
    // last that would fit.
    let mut expected = vec![plate];
    expected.extend(&sketches[..fit - 1]);
    let skipped: Vec<FeatureId> = session.skipped.iter().map(|(id, _)| *id).collect();
    if found.len() == fit + 1 {
        expected.push(sketches[fit - 1]);
    }
    assert_eq!(found, expected);
    // Not for good: those past it are no more complex than the rest.
    assert!(skipped.is_empty(), "{skipped:?}");
    // The first past it takes all that's left, and those after aren't
    // worked out at all.
    assert_eq!(session.worked_out, found.len() + 1);
    // Those found are kept, so the work's there for the rest as the
    // document changes, a share each time.
    let mut rounds = 0;
    while doc.extrude.as_ref().unwrap().found.len() < sketches.len() + 1 {
        rounds += 1;
        assert!(rounds <= sketches.len(), "{rounds}");
        let visible = rounds % 2 == 0;
        doc.apply(Command::SetFeatureVisible(plate, visible));
        let started = std::time::Instant::now();
        doc.sync();
        let took = started.elapsed().as_secs_f64();
        assert!(took < ui_bound(), "{took}");
    }
    let session = doc.extrude.as_ref().unwrap();
    assert!(session.skipped.is_empty());
}

/// [`example_and_a_hole`] with `more` New body extrudes of the plate
/// added, a cut of the hole set up, and every body taken out of it, so
/// the panel lists them all without asking the regeneration lane.
fn a_cut_listing(more: usize) -> (Doc, FeatureId) {
    let (mut doc, sketch, _) = example_and_a_hole();
    let FeatureKind::Extrude(plate) = doc.editor.document().features()[1].kind.clone() else {
        panic!("the example's second feature is its extrude");
    };
    for _ in 0..more {
        let mut extrude = plate.clone();
        extrude.operation = Operation::NewBody(varde_document::BodyId::NEW);
        doc.apply(doc.editor.document().add_extrude(extrude));
    }
    doc.sync();
    doc.look(Look::SelectFeature(sketch));
    doc.look(Look::StartExtrude);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region: 0 });
    extrude(&mut doc, ExtrudeLook::Operation(OperationKind::Cut));
    let bodies: Vec<_> = doc
        .editor
        .document()
        .bodies()
        .iter()
        .map(|b| b.id)
        .collect();
    for body in bodies {
        extrude(&mut doc, ExtrudeLook::Target(body));
    }
    (doc, sketch)
}

/// The texts of the extrude panel, from its title to its `OK`, in the
/// order the screen reports them, and the `OK`.
fn panel_texts(
    texts: &[varde_view::probe::Shown],
) -> (Vec<varde_view::probe::Shown>, varde_view::probe::Shown) {
    let title = texts.iter().position(|text| text.text == "New extrude");
    let title = title.expect("the panel's title");
    let ok = texts[title..].iter().position(|text| text.text == "OK");
    let panel = texts[title..=title + ok.expect("the panel's OK")].to_vec();
    let ok = panel.last().unwrap().clone();
    (panel, ok)
}

#[test]
fn many_bodies_keep_ok_and_cancel_on_screen() {
    use crate::tests::{clicked, shown, texts};
    use iced::advanced::widget::operation::scrollable::{RelativeOffset, snap_to};
    use varde_view::Message as Ui;
    use varde_view::probe::renderer as headless;

    for (height, bodies) in [
        (800.0, 15),
        (800.0, 30),
        (600.0, 4),
        (600.0, 8),
        (600.0, 30),
        (800.0, 300),
    ] {
        let (doc, _) = a_cut_listing(bodies - 1);
        assert_eq!(doc.extrude_state().unwrap().targets.len(), bodies);
        let size = iced::Size::new(1280.0, height);
        let status_top = height - varde_view::STATUS_BAR_HEIGHT;
        let mut renderer = headless();
        let mut ui = shown(doc.view(false, Mode::Light), size, &mut renderer);
        let check = |texts: &[varde_view::probe::Shown], last: &str| {
            let (panel, ok) = panel_texts(texts);
            let at = format!("{bodies} bodies at {height}");
            for button in ["OK", "Cancel"] {
                let button = panel.iter().find(|text| text.text == button).unwrap();
                // Whole, and above the status bar.
                assert!(button.bounds.height >= 14.0, "{at}: {button:?}");
                assert!(button.visible.is_none(), "{at}: {button:?}");
                assert!(
                    button.bounds.y + button.bounds.height <= status_top,
                    "{at}: {button:?}"
                );
            }
            // Nothing of the panel shows over the status bar, nor over the
            // buttons.
            for text in panel.iter().filter(|text| text.seen().height > 0.0) {
                let seen = text.seen();
                assert!(seen.y + seen.height <= status_top, "{at}: {text:?}");
                if text.text.starts_with("Body ") {
                    assert!(seen.y + seen.height <= ok.bounds.y, "{at}: {text:?}");
                }
            }
            // The last body, whole once scrolled to.
            let last = panel.iter().find(|text| text.text == last).unwrap();
            last.whole()
        };
        let last = format!("Body {bodies}");
        let texts_now = texts(&mut ui, &renderer);
        let _ = check(&texts_now, &last);
        let mut snap = snap_to(
            varde_view::PANEL_BODY,
            RelativeOffset {
                x: None,
                y: Some(1.0),
            },
        );
        ui.operate(&renderer, &mut snap);
        let texts_now = texts(&mut ui, &renderer);
        assert!(
            check(&texts_now, &last),
            "{bodies} bodies at {height}, scrolled"
        );

        // Cancel takes the click.
        let (panel, _) = panel_texts(&texts_now);
        let cancel = panel.iter().find(|text| text.text == "Cancel").unwrap();
        let cancel = cancel.bounds.center();
        let sent = clicked(&mut ui, &mut renderer, cancel);
        assert!(
            matches!(sent[..], [Ui::Look(Look::Extrude(ExtrudeLook::Cancel))]),
            "{sent:?}"
        );
    }
}

#[test]
fn a_click_on_a_body_s_label_toggles_it() {
    use crate::tests::{clicked, shown, texts};
    use varde_view::Message as Ui;
    use varde_view::probe::renderer as headless;

    let (doc, _) = a_cut_listing(2);
    let body = doc.editor.document().bodies()[0].id;
    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = headless();
    let mut ui = shown(doc.view(false, Mode::Light), size, &mut renderer);
    let texts_now = texts(&mut ui, &renderer);
    let (panel, _) = panel_texts(&texts_now);
    let label = panel.iter().find(|text| text.text == "Body 1").unwrap();
    // Right of the box, on the label.
    let at = iced::Point::new(label.bounds.x + 40.0, label.bounds.center_y());
    let sent = clicked(&mut ui, &mut renderer, at);
    assert!(
        matches!(&sent[..], [Ui::Look(Look::Extrude(ExtrudeLook::Target(b)))] if *b == body),
        "{sent:?}"
    );
}

#[test]
fn the_wheel_over_the_panel_scrolls_it_not_the_camera_and_keeps_the_focus() {
    use crate::tests::{shown, texts, typing};
    use iced::advanced::widget::operation::{focusable, text_input};
    use iced::mouse::{Cursor, Event, ScrollDelta};
    use varde_view::Message as Ui;

    let (doc, _) = a_cut_listing(29);
    let size = iced::Size::new(1280.0, 600.0);
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(doc.view(false, Mode::Light), size, &mut renderer);
    ui.operate(&renderer, &mut focusable::focus(varde_view::VALUE_FIELD));
    ui.operate(
        &renderer,
        &mut text_input::select_all(varde_view::VALUE_FIELD),
    );
    let mut send = |ui: &mut crate::tests::Headless<'_>, event: iced::Event, at| {
        let mut sent = Vec::new();
        let _ = ui.update(
            &[event],
            Cursor::Available(at),
            &mut renderer,
            &mut iced::advanced::clipboard::Null,
            &mut sent,
        );
        sent
    };
    let wheel = |at| {
        [
            iced::Event::Mouse(Event::CursorMoved { position: at }),
            iced::Event::Mouse(Event::WheelScrolled {
                delta: ScrollDelta::Lines { x: 0.0, y: -3.0 },
            }),
        ]
    };
    // Over the scene, the wheel zooms.
    let scene = iced::Point::new(400.0, 300.0);
    let sent: Vec<_> = wheel(scene)
        .into_iter()
        .flat_map(|event| send(&mut ui, event, scene))
        .collect();
    assert!(matches!(sent[..], [Ui::Look(Look::Zoom(_))]), "{sent:?}");
    // Over the panel's body, it scrolls the body and nothing else.
    let before = texts(&mut ui, &varde_view::probe::renderer());
    let (panel, _) = panel_texts(&before);
    let first = panel.iter().find(|text| text.text == "Body 1").unwrap();
    let at = first.bounds.center();
    let sent: Vec<_> = wheel(at)
        .into_iter()
        .flat_map(|event| send(&mut ui, event, at))
        .collect();
    assert!(sent.is_empty(), "{sent:?}");
    let after = texts(&mut ui, &varde_view::probe::renderer());
    let (panel, _) = panel_texts(&after);
    let moved = panel.iter().find(|text| text.text == "Body 1").unwrap();
    assert!(
        moved.bounds.y < first.bounds.y - 10.0,
        "{first:?} {moved:?}"
    );
    // The distance field keeps the focus: it takes typing, and Esc
    // cancels.
    let sent = send(&mut ui, typing(key("5"), Some("5")), at);
    assert!(
        matches!(
            &sent[..],
            [Ui::Look(Look::Extrude(ExtrudeLook::Input { distance: Distance::First, text }))]
                if text == "5"
        ),
        "{sent:?}"
    );
    let escape = keyboard::Key::Named(key::Named::Escape);
    let sent = send(&mut ui, typing(escape, None), at);
    assert!(
        matches!(sent[..], [Ui::Look(Look::Extrude(ExtrudeLook::Cancel))]),
        "{sent:?}"
    );
}

#[test]
fn two_sides_with_errors_keep_ok_on_a_short_screen() {
    use crate::tests::{shown, texts};
    use iced::advanced::widget::operation::scrollable::{RelativeOffset, snap_to};

    let (mut doc, _) = a_cut_listing(5);
    extrude(&mut doc, ExtrudeLook::Extent(ExtentKind::TwoSides));
    for distance in [Distance::First, Distance::Second] {
        let text = "12 parsecs".to_owned();
        extrude(&mut doc, ExtrudeLook::Input { distance, text });
    }
    let size = iced::Size::new(1024.0, 600.0);
    let status_top = size.height - varde_view::STATUS_BAR_HEIGHT;
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(doc.view(false, Mode::Light), size, &mut renderer);
    let mut snap = snap_to(
        varde_view::PANEL_BODY,
        RelativeOffset {
            x: None,
            y: Some(1.0),
        },
    );
    for scrolled in [false, true] {
        if scrolled {
            ui.operate(&renderer, &mut snap);
        }
        let shown = texts(&mut ui, &renderer);
        let (panel, ok) = panel_texts(&shown);
        assert!(ok.whole() && ok.bounds.height >= 14.0, "{ok:?}");
        assert!(ok.bounds.y + ok.bounds.height <= status_top, "{ok:?}");
        // What the body shows stays above the footer.
        for text in panel.iter().filter(|text| text.visible.is_some()) {
            let seen = text.seen();
            assert!(
                seen.height <= 0.0 || seen.y + seen.height <= ok.bounds.y,
                "{text:?}"
            );
        }
        let errors: Vec<_> = panel
            .iter()
            .filter(|text| text.text.starts_with("Unknown unit"))
            .collect();
        assert_eq!(errors.len(), 2, "{panel:?}");
        // Unscrolled, both sides' errors show; scrolled to the end, the
        // last body does.
        if scrolled {
            let last = panel.iter().find(|text| text.text == "Body 6").unwrap();
            assert!(last.whole(), "{last:?}");
        } else {
            assert!(errors.iter().all(|error| error.whole()), "{errors:?}");
        }
    }
}

#[test]
fn the_through_all_tip_shows_under_it_in_the_scrolled_body() {
    use crate::tests::{shown, texts};
    use iced::advanced::renderer::Headless;
    use iced::advanced::widget::operation::scrollable::{AbsoluteOffset, scroll_to};
    use iced::mouse::{Cursor, Event};
    use iced::theme::Base;

    let (mut doc, _) = a_cut_listing(29);
    extrude(&mut doc, ExtrudeLook::Operation(OperationKind::Join));
    let size = iced::Size::new(1280.0, 600.0);
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(doc.view(false, Mode::Light), size, &mut renderer);
    let offset = AbsoluteOffset {
        x: None,
        y: Some(40.0),
    };
    ui.operate(&renderer, &mut scroll_to(varde_view::PANEL_BODY, offset));
    let shown_now = texts(&mut ui, &renderer);
    let (panel, _) = panel_texts(&shown_now);
    let through = panel
        .iter()
        .find(|text| text.text == "Through all")
        .unwrap();
    assert!(through.whole(), "{through:?}");
    let theme = varde_view::iced_theme(Mode::Light);
    let base = theme.base();
    let style = iced::advanced::renderer::Style {
        text_color: base.text_color,
    };
    let mut drawn = |at: iced::Point| {
        let mut sent = Vec::new();
        let _ = ui.update(
            &[iced::Event::Mouse(Event::CursorMoved { position: at })],
            Cursor::Available(at),
            &mut renderer,
            &mut iced::advanced::clipboard::Null,
            &mut sent,
        );
        ui.draw(&mut renderer, &theme, &style, Cursor::Available(at));
        let physical = iced::Size::new(size.width as u32, size.height as u32);
        renderer.screenshot(physical, 1.0, base.background_color)
    };
    let away = drawn(iced::Point::new(1100.0, 20.0));
    let over = drawn(through.bounds.center());
    // What the tip changes, its box and shadow, centres just under the
    // choice where the body has scrolled it to: not 40 px off, as it
    // would be if it missed the scroll.
    let (mut top, mut bottom) = (f32::MAX, f32::MIN);
    for (k, (a, b)) in away.chunks(4).zip(over.chunks(4)).enumerate() {
        if a != b {
            let (x, y) = (
                (k % size.width as usize) as f32,
                (k / size.width as usize) as f32,
            );
            if (x - through.bounds.center_x()).abs() < 150.0 {
                top = top.min(y);
                bottom = bottom.max(y);
            }
        }
    }
    let below = through.bounds.y + through.bounds.height;
    let centre = (top + bottom) / 2.0;
    assert!(top > through.bounds.y, "{top}..{bottom} {through:?}");
    assert!(
        centre > below && centre < below + 35.0,
        "{top}..{bottom} {through:?}"
    );
}

/// Whether the value field has the focus on `ui`.
fn value_field_focused(ui: &mut crate::tests::Headless<'_>, renderer: &iced::Renderer) -> bool {
    use iced::advanced::widget::operation::Focusable;
    use iced::advanced::widget::{Id, Operation};

    struct Focused(bool);
    impl Operation for Focused {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<()>)) {
            operate(self);
        }
        fn focusable(&mut self, id: Option<&Id>, _: iced::Rectangle, state: &mut dyn Focusable) {
            if id == Some(&varde_view::VALUE_FIELD) {
                self.0 |= state.is_focused();
            }
        }
    }
    let mut focused = Focused(false);
    ui.operate(renderer, &mut focused);
    focused.0
}

/// Where the panel's scrollable body is on `ui`.
fn panel_body(ui: &mut crate::tests::Headless<'_>, renderer: &iced::Renderer) -> iced::Rectangle {
    use iced::advanced::widget::operation::Scrollable;
    use iced::advanced::widget::{Id, Operation};

    struct Body(Option<iced::Rectangle>);
    impl Operation for Body {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<()>)) {
            operate(self);
        }
        fn scrollable(
            &mut self,
            id: Option<&Id>,
            bounds: iced::Rectangle,
            _: iced::Rectangle,
            _: iced::Vector,
            _: &mut dyn Scrollable,
        ) {
            if id == Some(&varde_view::PANEL_BODY) {
                self.0 = Some(bounds);
            }
        }
    }
    let mut body = Body(None);
    ui.operate(renderer, &mut body);
    body.0.expect("the panel's body")
}

#[test]
fn unpicking_the_last_region_keeps_the_panel_s_scroll_and_focus() {
    use crate::tests::{shown, texts};
    use iced::advanced::widget::operation::focusable;
    use iced::advanced::widget::operation::scrollable::{AbsoluteOffset, scroll_to};
    use iced_runtime::user_interface::UserInterface;

    let (mut doc, sketch) = a_cut_listing(29);
    let size = iced::Size::new(1280.0, 600.0);
    let mut renderer = varde_view::probe::renderer();
    let body_1 = |ui: &mut crate::tests::Headless<'_>, renderer: &iced::Renderer| {
        let shown = texts(ui, renderer);
        let (panel, _) = panel_texts(&shown);
        panel
            .iter()
            .find(|text| text.text == "Body 1")
            .unwrap()
            .bounds
    };
    let (cache, scrolled) = {
        let mut ui = shown(doc.view(false, Mode::Light), size, &mut renderer);
        let unscrolled = body_1(&mut ui, &renderer);
        ui.operate(&renderer, &mut focusable::focus(varde_view::VALUE_FIELD));
        let offset = AbsoluteOffset {
            x: None,
            y: Some(100.0),
        };
        ui.operate(&renderer, &mut scroll_to(varde_view::PANEL_BODY, offset));
        let scrolled = body_1(&mut ui, &renderer);
        assert!(
            scrolled.y < unscrolled.y - 50.0,
            "{unscrolled:?} {scrolled:?}"
        );
        (ui.into_cache(), scrolled)
    };
    // The handle and its knobs go with the last region.
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region: 0 });
    let state = doc.extrude_state().unwrap();
    assert!(state.picked.is_empty() && state.handle().is_none());
    let mut ui = UserInterface::build(doc.view(false, Mode::Light), size, cache, &mut renderer);
    assert_eq!(body_1(&mut ui, &renderer), scrolled);
    assert!(value_field_focused(&mut ui, &renderer));
}

#[test]
fn a_short_window_lifts_the_panel_to_keep_its_buttons() {
    use crate::tests::{shown, texts};

    let (doc, _) = a_cut_listing(29);
    for height in [250.0, 300.0, 400.0] {
        let size = iced::Size::new(1280.0, height);
        let status_top = height - varde_view::STATUS_BAR_HEIGHT;
        let mut renderer = varde_view::probe::renderer();
        let mut ui = shown(doc.view(false, Mode::Light), size, &mut renderer);
        let shown = texts(&mut ui, &renderer);
        let (panel, ok) = panel_texts(&shown);
        let title = &panel[0];
        assert!(title.bounds.height >= 14.0, "{height}: {title:?}");
        for button in ["OK", "Cancel"] {
            let button = panel.iter().find(|text| text.text == button).unwrap();
            assert!(button.bounds.height >= 14.0, "{height}: {button:?}");
            assert!(
                button.bounds.y + button.bounds.height <= status_top,
                "{height}: {button:?}"
            );
        }
        // A few rows of the body still show between them.
        let body = panel_body(&mut ui, &renderer);
        assert!(body.height >= 40.0, "{height}: {body:?}");
        assert!(body.y + body.height <= ok.bounds.y, "{height}: {body:?}");
    }
}

/// Runs the widget operations of `task` on `ui`, as the runtime does.
fn run_task(
    ui: &mut crate::tests::Headless<'_>,
    renderer: &iced::Renderer,
    task: iced::Task<crate::Message>,
) {
    use iced::advanced::widget::operation::Outcome;
    use iced::futures::StreamExt;
    use iced_runtime::Action;

    let Some(stream) = iced_runtime::task::into_stream(task) else {
        return;
    };
    let actions: Vec<_> = iced::futures::executor::block_on(stream.collect());
    for action in actions {
        let Action::Widget(mut operation) = action else {
            continue;
        };
        loop {
            ui.operate(renderer, operation.as_mut());
            match operation.finish() {
                Outcome::Chain(next) => operation = next,
                _ => break,
            }
        }
    }
}

#[test]
fn editing_an_extrude_from_a_scrolled_panel_shows_its_field() {
    use crate::tests::{shown, texts};
    use iced::advanced::widget::operation::scrollable::{RelativeOffset, snap_to};
    use iced_runtime::user_interface::UserInterface;

    // Short enough that even a New body extrude's panel scrolls.
    let (mut doc, _) = a_cut_listing(3);
    let size = iced::Size::new(1280.0, 300.0);
    let mut renderer = varde_view::probe::renderer();
    let cache = {
        let mut ui = shown(doc.view(false, Mode::Light), size, &mut renderer);
        let mut end = snap_to(
            varde_view::PANEL_BODY,
            RelativeOffset {
                x: None,
                y: Some(1.0),
            },
        );
        ui.operate(&renderer, &mut end);
        ui.into_cache()
    };
    // The panel stays, now for the last New body extrude.
    let id = doc.editor.document().features().last().unwrap().id;
    doc.look(Look::EditFeature(id));
    assert_eq!(doc.extrude.as_ref().unwrap().feature, Some(id));
    let focus = doc.take_focus().expect("the field takes the focus");
    let mut ui = UserInterface::build(doc.view(false, Mode::Light), size, cache, &mut renderer);
    run_task(&mut ui, &renderer, crate::focus_field(focus));
    assert!(value_field_focused(&mut ui, &renderer));
    // The field's label, beside it, shows whole.
    let shown = texts(&mut ui, &renderer);
    let distance = shown
        .iter()
        .find(|text| text.text == "Distance")
        .expect("the distance's label");
    assert!(distance.whole(), "{distance:?}");
}

#[test]
fn dragging_the_panel_s_scrollbar_over_the_scene_only_scrolls() {
    use crate::tests::{shown, texts};
    use iced::mouse::{Button, Cursor, Event, ScrollDelta};

    let (doc, _) = a_cut_listing(29);
    let size = iced::Size::new(1280.0, 600.0);
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(doc.view(false, Mode::Light), size, &mut renderer);
    let body_1 = |ui: &mut crate::tests::Headless<'_>, renderer: &iced::Renderer| {
        let shown = texts(ui, renderer);
        let (panel, _) = panel_texts(&shown);
        panel
            .iter()
            .find(|text| text.text == "Body 1")
            .unwrap()
            .bounds
    };
    let before = body_1(&mut ui, &renderer);
    let body = panel_body(&mut ui, &renderer);
    let mut sent = Vec::new();
    let mut send = |ui: &mut crate::tests::Headless<'_>, event: Event, at: iced::Point| {
        let _ = ui.update(
            &[iced::Event::Mouse(event)],
            Cursor::Available(at),
            &mut renderer,
            &mut iced::advanced::clipboard::Null,
            &mut sent,
        );
    };
    // The wheel up at the top of the body has nothing to scroll, and
    // still doesn't zoom.
    let inside = iced::Point::new(body.center_x(), body.y + 20.0);
    send(&mut ui, Event::CursorMoved { position: inside }, inside);
    let up = ScrollDelta::Lines { x: 0.0, y: 3.0 };
    send(&mut ui, Event::WheelScrolled { delta: up }, inside);
    // The scroller, at the top of the scrollbar in the body's right
    // padding, dragged down and out over the scene, let go there.
    let grab = iced::Point::new(body.x + body.width - 5.0, body.y + 4.0);
    let scene = iced::Point::new(400.0, body.y + 150.0);
    send(&mut ui, Event::CursorMoved { position: grab }, grab);
    send(&mut ui, Event::ButtonPressed(Button::Left), grab);
    send(&mut ui, Event::CursorMoved { position: scene }, scene);
    send(&mut ui, Event::ButtonReleased(Button::Left), scene);
    assert!(sent.is_empty(), "{sent:?}");
    let after = body_1(&mut ui, &renderer);
    assert!(after.y < before.y - 100.0, "{before:?} {after:?}");
}

/// Screenshots of the extrude session and the screens around it, to look
/// at: `#[ignore]`d, and written only where `VARDE_SHOTS` says.
mod shots;
