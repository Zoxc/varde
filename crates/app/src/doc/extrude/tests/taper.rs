//! The extrude's taper: typed in the panel's Taper field, in the draft
//! the preview regenerates, refused where it isn't an angle under 90°
//! either way, 0° stored as none. The kernel's tapered extrude isn't
//! built (it fails as not built yet): the preview fails, OK waits and Add
//! anyway keeps it, failing in the Timeline with its note; with
//! regeneration tapering rectangles into frustums
//! ([`varde_regen::testing::taper_by_frustum`]) the preview works and
//! OK adds it as one undo step. Editing a tapered extrude shows its
//! taper; units changed mid-session keep it.

use varde_document::{Command, Document, Editor, Extrude, FeatureKind, OriginPlane, Plane};
use varde_view::{Edit, ExtrudeLook, Look, Message as Ui};

use super::{Requests, enter, extrude, extrudes, key, last_draft};
use crate::doc::Doc;
use crate::tests::{answer, holding, key_in, screen_texts};

/// A document holding a sketch on XY of the rectangle 0–20 × 0–10,
/// shown, and an extrude session started with its region picked, 10 mm
/// one side: the doc and its requests.
fn rectangle_picked() -> (Doc, Requests) {
    let mut editor = Editor::new(Document::default());
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XY)))
        .unwrap();
    let sketch = editor.document().features()[0].id;
    let mut drawn = varde_sketch::Sketch::default();
    let corners = [(0.0, 0.0), (20.0, 0.0), (20.0, 10.0), (0.0, 10.0)]
        .map(|(x, y)| drawn.add_point(glam::DVec2::new(x, y)).unwrap());
    for (k, &start) in corners.iter().enumerate() {
        let end = corners[(k + 1) % 4];
        let line = varde_sketch::Curve::Line { start, end };
        drawn.add_curve(line, false).unwrap();
    }
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn),
        })
        .unwrap();
    let (mut doc, requests) = holding(editor.document().clone());
    doc.camera.set_view_height(40.0);
    key_in(&mut doc, key("x"));
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region: 0 });
    (doc, requests)
}

fn typed(doc: &mut Doc, text: &str) {
    extrude(doc, ExtrudeLook::Taper(text.to_owned()));
}

/// The taper the last draft previews.
fn drafted_taper(requests: &Requests) -> Option<f64> {
    let draft = last_draft(requests).expect("a draft");
    draft.extrude.taper.map(|taper| taper.value)
}

/// Whether the screen shows a text holding `wanted`.
fn shows(doc: &Doc, wanted: &str) -> bool {
    screen_texts(doc).iter().any(|text| text.contains(wanted))
}

/// The panel's Taper field starts at 0°, stored as none; a typed taper
/// goes in the draft; the kernel's stand-in fails the preview as too
/// complex, OK waits, Add anyway keeps it as one undo step, failing in
/// the Timeline, whose row notes the taper.
#[test]
fn a_typed_taper_is_previewed_and_add_anyway_keeps_it() {
    let (mut doc, requests) = rectangle_picked();
    let before = doc.editor.document().clone();
    let session = doc.extrude.as_ref().unwrap();
    assert_eq!(session.taper.text, "0°");
    assert!(shows(&doc, "Taper"));
    assert_eq!(drafted_taper(&requests), None);
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.draft_error(), None);

    typed(&mut doc, "5");
    assert_eq!(drafted_taper(&requests), Some(5f64.to_radians()));
    answer(&mut doc, &requests);
    let error = doc.feed.draft_error().expect("the stand-in fails");
    assert_eq!(error, "a tapered extrude isn't supported yet");
    assert!(shows(&doc, "Add anyway"));
    let state = doc.extrude_state().unwrap();
    assert!(!state.ready && state.accept);
    let features = doc.editor.document().features().len();
    key_in(&mut doc, enter());
    assert_eq!(doc.editor.document().features().len(), features);
    assert!(doc.extrude.is_some());

    doc.update(Edit::AcceptError);
    assert!(doc.extrude.is_none());
    let [tapered] = extrudes(&doc)[..] else {
        panic!("one extrude");
    };
    assert_eq!(tapered.taper.as_ref().unwrap().text, "5");
    let id = doc.editor.document().features().last().unwrap().id;
    answer(&mut doc, &requests);
    assert!(
        (doc.feed.failed_features().iter()).any(|failed| failed.feature == id),
        "it fails"
    );
    doc.look(Look::SelectPanel(varde_view::Panel::Timeline));
    assert!(shows(&doc, "10 mm · 5°"), "{:?}", screen_texts(&doc));
    doc.update(Edit::Undo);
    assert_eq!(*doc.editor.document(), before);
}

/// A taper that isn't an angle under 90° either way is refused in the
/// field and blocks OK, the preview keeping the last; 0 is none.
#[test]
fn a_refused_taper_blocks_ok_and_zero_is_none() {
    let (mut doc, requests) = rectangle_picked();
    typed(&mut doc, "-12");
    assert_eq!(drafted_taper(&requests), Some(-(12f64.to_radians())));
    let revision = last_draft(&requests).unwrap().revision;
    for text in ["90", "-95", "3 mm", "4 +"] {
        typed(&mut doc, text);
        let state = doc.extrude_state().unwrap();
        assert!(state.taper.error.is_some(), "{text}");
        assert!(!state.ready && !state.accept, "{text}");
        assert_eq!(last_draft(&requests).unwrap().revision, revision);
    }
    typed(&mut doc, "3 - 3");
    assert_eq!(drafted_taper(&requests), None);
    answer(&mut doc, &requests);
    assert!(doc.extrude_state().unwrap().ready);
    doc.update(Edit::CommitExtrude);
    assert_eq!(extrudes(&doc)[0].taper, None);
}

/// With regeneration tapering rectangles, the preview works: OK adds
/// the tapered extrude as one undo step, and the model is a frustum.
#[test]
fn a_tapered_rectangle_previews_and_ok_adds_it() {
    varde_regen::testing::taper_by_frustum();
    let (mut doc, requests) = rectangle_picked();
    let before = doc.editor.document().clone();
    typed(&mut doc, "10");
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.draft_error(), None);
    assert!(doc.extrude_ready());
    key_in(&mut doc, enter());
    assert!(doc.extrude.is_none());
    let [tapered] = extrudes(&doc)[..] else {
        panic!("one extrude");
    };
    assert_eq!(tapered.taper.as_ref().unwrap().value, 10f64.to_radians());
    answer(&mut doc, &requests);
    assert!(doc.feed.failed_features().is_empty());
    assert!(doc.feed.mesh().triangle_count() > 0);
    doc.update(Edit::Undo);
    assert_eq!(*doc.editor.document(), before);
}

/// A tapered extrude edited shows its taper as typed; changed and set
/// again, undo brings the first back; the taper taken to 0 stores none.
#[test]
fn editing_a_tapered_extrude_shows_and_changes_its_taper() {
    let (mut doc, requests) = rectangle_picked();
    typed(&mut doc, "2 + 1");
    answer(&mut doc, &requests);
    doc.update(Edit::AcceptError);
    let id = doc.editor.document().features().last().unwrap().id;
    answer(&mut doc, &requests);

    doc.look(Look::EditFeature(id));
    let session = doc.extrude.as_ref().expect("editing it");
    assert_eq!(session.taper.text, "2 + 1");
    // Nothing changed, OK writes nothing.
    let revision = doc.editor.revision();
    answer(&mut doc, &requests);
    doc.update(Edit::AcceptError);
    assert_eq!(doc.editor.revision(), revision);

    doc.look(Look::EditFeature(id));
    typed(&mut doc, "-4");
    assert_eq!(last_draft(&requests).unwrap().feature, Some(id));
    answer(&mut doc, &requests);
    doc.update(Edit::AcceptError);
    let taper = |doc: &Doc| extrudes(doc)[0].taper.as_ref().map(|t| t.value);
    assert_eq!(taper(&doc), Some(-(4f64.to_radians())));
    doc.update(Edit::Undo);
    assert_eq!(taper(&doc), Some(3f64.to_radians()));

    doc.look(Look::EditFeature(id));
    typed(&mut doc, "0");
    answer(&mut doc, &requests);
    doc.update(Edit::CommitExtrude);
    assert_eq!(taper(&doc), None);
}

/// The design's units changed mid-session keep the taper typed, which
/// the document takes.
#[test]
fn a_taper_keeps_its_angle_when_the_units_change() {
    let (mut doc, requests) = rectangle_picked();
    typed(&mut doc, "7.5");
    doc.update(Edit::SetUnits(varde_expr::LengthUnit::In));
    let session = doc.extrude.as_ref().unwrap();
    assert_eq!(session.taper.text, "7.5");
    assert_eq!(drafted_taper(&requests), Some(7.5f64.to_radians()));
    answer(&mut doc, &requests);
    doc.update(Edit::AcceptError);
    assert_eq!(doc.edit_error, None);
    let FeatureKind::Extrude(Extrude { taper, .. }) =
        &doc.editor.document().features().last().unwrap().kind
    else {
        panic!("an extrude");
    };
    assert_eq!(taper.as_ref().unwrap().value, 7.5f64.to_radians());
}

/// `Esc` cancels an extrude once, whatever fields its extent shows (the
/// distances and the taper each send the cancel, the first takes the
/// key), through all's taper alone too.
#[test]
fn escape_cancels_an_extrude_once() {
    use crate::tests::{pressed, typing};
    use varde_view::{ExtentKind, OperationKind};
    let (mut doc, _requests) = rectangle_picked();
    extrude(&mut doc, ExtrudeLook::Operation(OperationKind::Cut));
    let escape = iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape);
    for extent in ExtentKind::ALL {
        extrude(&mut doc, ExtrudeLook::Extent(extent));
        for focused in [true, false] {
            let (sent, shortcuts) = pressed(&doc, &[typing(escape.clone(), None)], focused);
            assert!(
                matches!(sent[..], [Ui::Look(Look::Extrude(ExtrudeLook::Cancel))]),
                "{extent:?} {sent:?}"
            );
            assert!(shortcuts.is_empty(), "{shortcuts:?}");
        }
    }
}

/// Whatever is typed in the Taper field, the session holds a finite
/// angle under 90° either way or refuses the text, keeping the last;
/// the draft carries the angle (none for zero), and OK is offered only
/// for one taken.
#[test]
fn typed_tapers_are_taken_or_refused_never_out_of_range() {
    let (mut doc, requests) = rectangle_picked();
    let texts = [
        "5",
        "-5",
        "1e400",
        "-1e400",
        "1e-400",
        "nan",
        "inf",
        "-inf",
        "1/0",
        "0/0",
        "",
        "   ",
        "°",
        "5 deg",
        "5°",
        "0.5 rad",
        "-1.5 rad",
        "1.5708 rad",
        "-1.5707963 rad",
        "89.99999999",
        "-89.99999999",
        "90 - 1e-12",
        "90",
        "-90",
        "180",
        "360 + 5",
        "5 mm",
        "5 mm / 1 mm",
        "2 * 3",
        "3 - 3",
        "-0",
        "(",
        "5 5",
        "1e308 * 10",
        "9999999999999999999999",
        "0x10",
        "5 grad",
        "ÿ",
        "1e-300 rad",
    ];
    let half = std::f64::consts::FRAC_PI_2;
    let mut last = None;
    for text in texts {
        typed(&mut doc, text);
        let session = doc.extrude.as_ref().unwrap();
        match &session.taper.error {
            Some(_) => {
                assert_eq!(
                    session.taper.value.as_ref().map(|v| v.value),
                    last,
                    "{text}"
                );
                let state = doc.extrude_state().unwrap();
                assert!(!state.ready && !state.accept, "{text}");
            }
            None => {
                let value = session.taper.value.as_ref().unwrap().value;
                assert!(value.is_finite() && value.abs() < half, "{text}: {value}");
                last = Some(value);
                // Asked for again only where it changed: the newest
                // request is its.
                let drafted = drafted_taper(&requests);
                assert_eq!(drafted, (value != 0.0).then_some(value), "{text}");
            }
        }
    }
}

/// The taper stays with the extrude as its extent and Flip change: each
/// draft carries it. With regeneration tapering rectangles, one side
/// either way previews; symmetric, two sides and through all, spanning
/// the sketch's plane, fail as too complex; OK on one adds it as one
/// undo step, which redo puts back.
#[test]
fn a_taper_stays_through_extents_and_flips() {
    use varde_view::{ExtentKind, OperationKind};
    varde_regen::testing::taper_by_frustum();
    let (mut doc, requests) = rectangle_picked();
    let before = doc.editor.document().clone();
    typed(&mut doc, "8");
    let eight = Some(8f64.to_radians());
    extrude(&mut doc, ExtrudeLook::Operation(OperationKind::Cut));
    extrude(&mut doc, ExtrudeLook::Operation(OperationKind::NewBody));
    for (look, works) in [
        (ExtrudeLook::Flip, Some(true)),
        (ExtrudeLook::Extent(ExtentKind::Symmetric), Some(false)),
        (ExtrudeLook::Extent(ExtentKind::TwoSides), Some(false)),
        (ExtrudeLook::Operation(OperationKind::Cut), Some(false)),
        // There's no body to go through.
        (ExtrudeLook::Extent(ExtentKind::ThroughAll), None),
        (ExtrudeLook::Operation(OperationKind::NewBody), Some(true)),
        (ExtrudeLook::Flip, Some(true)),
    ] {
        extrude(&mut doc, look.clone());
        assert_eq!(drafted_taper(&requests), eight, "{look:?}");
        answer(&mut doc, &requests);
        let error = doc.feed.draft_error();
        if works == Some(true) {
            assert_eq!(error, None, "{look:?}");
        } else if works == Some(false) {
            assert_eq!(
                error,
                Some("tapering its walls is too complex to work out"),
                "{look:?}"
            );
        }
    }
    let session = doc.extrude.as_ref().unwrap();
    assert_eq!(session.extent, ExtentKind::OneSide);
    assert!(doc.extrude_ready());
    key_in(&mut doc, enter());
    let tapered = doc.editor.document().clone();
    let taper = |doc: &Doc| extrudes(doc)[0].taper.as_ref().map(|t| t.value);
    assert_eq!(taper(&doc), eight);
    doc.update(Edit::Undo);
    assert_eq!(*doc.editor.document(), before);
    doc.update(Edit::Redo);
    assert_eq!(*doc.editor.document(), tapered);
    answer(&mut doc, &requests);
    assert!(doc.feed.failed_features().is_empty());
}
