//! The pattern sessions: `P` and the circular pattern, their modes, the
//! mock's errors, editing from the Timeline, long texts, what's gone.

use std::f64::consts::TAU;

use glam::DVec3;
use varde_document::{Axis3, AxisRef, Command, FeatureId, FeatureKind, Pattern, PatternKind};
use varde_expr::Value;
use varde_regen::Summary;
use varde_view::{
    Edit, Look, MotionField, MotionKind, MotionLook, MotionPick, PatternMode, Picked,
};

use super::{Plates, character, enter, later_disc, near, plates};
use crate::tests::{key_in, screen_texts};

/// The pattern the last request previews, if it previews one.
fn drafted(plates: &Plates) -> Option<Pattern> {
    match plates.last_draft()?.1 {
        FeatureKind::Pattern(pattern) => Some(pattern),
        _ => None,
    }
}

/// The document's last feature as a pattern.
fn last_pattern(plates: &Plates) -> Pattern {
    let FeatureKind::Pattern(pattern) = plates.last_feature().1 else {
        panic!("a pattern");
    };
    pattern
}

/// A linear pattern's spacing, or a circular one's angle.
fn spread(pattern: &Pattern) -> f64 {
    match &pattern.kind {
        PatternKind::Linear { spacing, .. } => spacing.value,
        PatternKind::Circular { angle, .. } => angle.value,
    }
}

fn shows(plates: &Plates, wanted: &str) -> bool {
    screen_texts(&plates.doc)
        .iter()
        .any(|text| text.contains(wanted))
}

#[test]
fn p_starts_a_linear_pattern_typing_a_count_and_spacing_previews_the_row() {
    let mut plates = plates();
    let [_, right, _] = plates.bodies;
    let before = plates.doc.editor.document().clone();
    let [low, high] = plates.bounds(right);
    plates.click(right);
    key_in(&mut plates.doc, character("p"));
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(session.kind, MotionKind::LinearPattern);
    assert_eq!(session.bodies, [right]);
    assert_eq!(session.axis, Some(AxisRef::Origin(Axis3::X)));
    assert_eq!(session.mode, PatternMode::Spacing);
    for text in [
        "New linear pattern",
        "Bodies",
        "Direction",
        "X axis",
        "Flip direction",
        "Copies",
        "Count",
        "Spacing",
        "Total",
    ] {
        assert!(shows(&plates, text), "{text}");
    }
    assert!(shows(&plates, "Join to original"));

    // The count and spacing typed preview the row.
    plates.input(MotionField::Count, "4");
    plates.input(MotionField::Spread, "12");
    assert!(plates.doc.motion_ready());
    let pattern = drafted(&plates).expect("a pattern's draft");
    assert_eq!(pattern.count(), Some(4));
    assert_eq!(spread(&pattern), 12.0);
    assert_eq!(pattern.kind.axis(), &AxisRef::Origin(Axis3::X));
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    let [row_low, row_high] = plates.bounds(right);
    let run = DVec3::new(36.0, 0.0, 0.0);
    assert!(
        near(row_low, low) && near(row_high, high + run),
        "{row_low} {row_high}"
    );
    assert!(shows(&plates, "Body 2 · 4 × 12 mm along X axis"));

    // Enter commits it, one undo step, selected and named.
    key_in(&mut plates.doc, enter());
    assert!(plates.doc.motion.is_none());
    let (id, _) = plates.last_feature();
    assert_eq!(last_pattern(&plates), pattern);
    assert_eq!(plates.doc.selected_feature, Some(id));
    let document = plates.doc.editor.document();
    assert_eq!(document.feature(id).unwrap().name, "Pattern 1");
    plates.answer();
    plates
        .doc
        .look(Look::SelectPanel(varde_view::Panel::Timeline));
    assert!(shows(&plates, "×4"));
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

#[test]
fn total_stores_the_spacing_it_comes_to_and_flip_turns_it() {
    let mut plates = plates();
    let [_, right, _] = plates.bodies;
    plates.click(right);
    plates.doc.look(Look::StartPattern);
    plates.input(MotionField::Count, "4");
    plates.motion(MotionLook::Mode(PatternMode::Total));
    plates.input(MotionField::Spread, "45");
    assert!(shows(&plates, "Body 2 · 4 × 15 mm along X axis"));
    plates.motion(MotionLook::Flip);
    let pattern = drafted(&plates).expect("a draft");
    assert_eq!(spread(&pattern), -15.0);
    assert!(shows(&plates, "4 × 15 mm along X axis, flipped"));
    plates.answer();
    let [low, _] = plates.bounds(right);
    assert!((low.x - (15.0 - 45.0)).abs() < 1e-3, "{low}");
    plates.doc.update(Edit::CommitMotion);
    let (id, _) = plates.last_feature();
    let stored = last_pattern(&plates);
    assert_eq!(spread(&stored), -15.0);
    let committed = plates.doc.editor.document().clone();

    // Edited again, it's in the mode it was set up in, as typed.
    plates.doc.look(Look::EditFeature(id));
    let session = plates.doc.motion.as_ref().expect("editing it");
    assert_eq!(session.mode, PatternMode::Total);
    assert!(session.flip);
    assert_eq!(session.fields[MotionField::Spread.index()].text, "45 mm");
    // OK with nothing changed writes nothing.
    plates.doc.update(Edit::CommitMotion);
    assert_eq!(*plates.doc.editor.document(), committed);

    // Without what the session chose (another run), the spacing it
    // stores, flipped.
    plates.doc.pattern_shapes.clear();
    plates.doc.look(Look::EditFeature(id));
    let session = plates.doc.motion.as_ref().expect("editing it");
    assert_eq!(session.mode, PatternMode::Spacing);
    assert!(session.flip);
    let field = &session.fields[MotionField::Spread.index()];
    assert_eq!(field.value.as_ref().map(|value| value.value), Some(15.0));
    plates.doc.update(Edit::CommitMotion);
    let (_, kind) = plates.last_feature();
    let FeatureKind::Pattern(again) = kind else {
        panic!("a pattern");
    };
    assert_eq!(spread(&again), -15.0);
}

#[test]
fn a_circular_pattern_full_turn_spreads_the_copies_round_the_axis() {
    let mut plates = plates();
    let [_, right, _] = plates.bodies;
    plates.click(right);
    plates.doc.look(Look::StartCircularPattern);
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(session.kind, MotionKind::CircularPattern);
    assert_eq!(session.axis, Some(AxisRef::Origin(Axis3::Z)));
    assert_eq!(session.mode, PatternMode::Full);
    for text in ["New circular pattern", "Axis", "Z axis", "Full 360°"] {
        assert!(shows(&plates, text), "{text}");
    }
    assert!(!shows(&plates, "Flip direction"));
    // Full 360° shows no angle field: four copies a quarter apart.
    assert!(plates.doc.motion_ready());
    let pattern = drafted(&plates).expect("a draft");
    assert_eq!(pattern.count(), Some(4));
    assert!(pattern.full_turn());
    assert!((spread(&pattern) - TAU).abs() < 1e-12);
    assert_eq!(pattern.step_degrees(), Some(90.0));
    assert!(shows(&plates, "Body 2 · 4 × 90° about Z axis"));
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    // The disc, radius 5 about (20, 0), at each quarter about Z.
    let [low, high] = plates.bounds(right);
    assert!(near(low, DVec3::new(-25.0, -25.0, -5.0)), "{low}");
    assert!(near(high, DVec3::new(25.0, 25.0, 15.0)), "{high}");
    plates.doc.update(Edit::CommitMotion);
    let stored = last_pattern(&plates);
    assert_eq!(stored, pattern);
    plates.answer();
    assert!(plates.doc.feed.failed_features().is_empty());
}

#[test]
fn a_spacing_past_a_full_turn_is_the_mock_s_error() {
    let mut plates = plates();
    let [_, right, _] = plates.bodies;
    plates.click(right);
    plates.doc.look(Look::StartCircularPattern);
    plates.motion(MotionLook::Mode(PatternMode::Spacing));
    // 4 copies 90° apart span 270°: three quarters.
    let pattern = drafted(&plates).expect("a draft");
    assert!((spread(&pattern) - 1.5 * std::f64::consts::PI).abs() < 1e-12);
    assert_eq!(pattern.step_degrees(), Some(90.0));
    plates.input(MotionField::Spread, "120");
    assert!(!plates.doc.motion_ready());
    assert!(shows(&plates, "4 copies 120° apart go past a full turn"));
    // Just a turn is past it too: the last copy would be on the first.
    plates.input(MotionField::Count, "5");
    plates.input(MotionField::Spread, "90");
    assert!(shows(&plates, "5 copies 90° apart go past a full turn"));
    plates.input(MotionField::Count, "4");
    assert!(plates.doc.motion_ready());
    // A total of a whole turn is Full 360°'s.
    plates.motion(MotionLook::Mode(PatternMode::Total));
    plates.input(MotionField::Spread, "360");
    assert!(!plates.doc.motion_ready());
    assert!(shows(
        &plates,
        "A whole turn puts the last copy on the first: use Full 360°"
    ));
    // Three over 90°: one at each end.
    plates.input(MotionField::Count, "3");
    plates.input(MotionField::Spread, "90");
    assert!(plates.doc.motion_ready());
    let pattern = drafted(&plates).expect("a draft");
    assert_eq!(pattern.step_degrees(), Some(45.0));
    // A linear one running past the limit.
    plates.doc.look(Look::StartPattern);
    plates.input(MotionField::Spread, "900000");
    assert!(!plates.doc.motion_ready());
    assert!(shows(&plates, "The pattern runs past 1000000 mm"));
}

#[test]
fn editing_a_pattern_from_the_timeline_changes_it_as_one_undo_step() {
    let mut plates = plates();
    let [_, right, _] = plates.bodies;
    plates.doc.look(Look::StartPattern);
    plates.doc.look(Look::ClickBody {
        body: right,
        add: false,
    });
    plates.input(MotionField::Spread, "15");
    plates.doc.update(Edit::CommitMotion);
    plates.answer();
    let (id, first) = plates.last_feature();
    let committed = plates.doc.editor.document().clone();

    // Enter on its row edits it with its values.
    plates.doc.look(Look::SelectFeature(id));
    key_in(&mut plates.doc, enter());
    let session = plates.doc.motion.as_ref().expect("editing it");
    assert_eq!(session.feature, Some(id));
    assert_eq!(session.kind, MotionKind::LinearPattern);
    assert_eq!(session.bodies, [right]);
    assert_eq!(session.fields[MotionField::Count.index()].text, "3");
    assert_eq!(session.fields[MotionField::Spread.index()].text, "15 mm");
    assert!(shows(&plates, "Pattern 1"));

    // Another direction from the toolbar, and a count.
    plates.motion(MotionLook::Picking(varde_view::MotionPick::Reference));
    plates.motion(MotionLook::OriginAxis(Axis3::Y));
    plates.input(MotionField::Count, "5");
    plates.answer();
    let [low, high] = plates.bounds(right);
    assert!(
        (low.y + 5.0).abs() < 1e-3 && (high.y - 65.0).abs() < 1e-3,
        "{low} {high}"
    );
    plates.doc.update(Edit::CommitMotion);
    assert!(plates.doc.motion.is_none());
    let (same, changed) = plates.last_feature();
    assert_eq!(same, id);
    assert_ne!(changed, first);
    let FeatureKind::Pattern(changed) = changed else {
        panic!("a pattern");
    };
    assert_eq!(changed.count(), Some(5));
    assert_eq!(changed.kind.axis(), &AxisRef::Origin(Axis3::Y));
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), committed);
}

/// Sets the pattern `id` to `kind` as an undo step of its own, as a
/// file or another edit could.
fn set_pattern(plates: &mut Plates, id: FeatureId, kind: PatternKind) {
    let FeatureKind::Pattern(mut pattern) = plates.last_feature().1 else {
        panic!("a pattern");
    };
    pattern.kind = kind;
    plates.doc.apply(Command::SetFeature {
        feature: id,
        kind: Box::new(FeatureKind::Pattern(pattern)),
    });
    assert_eq!(plates.doc.edit_error, None);
    plates.doc.sync();
    plates.answer();
}

/// A spread typed as long as an expression may be, which the session's
/// "(...) / 3" or "-(...)" would take past it, is stored as the value it
/// comes to, written exactly, not refused for the length; and a stored
/// spacing whose text turned would be too long opens as its size, not as
/// the new pattern's 100 mm.
#[test]
fn a_spread_too_long_to_wrap_is_stored_as_its_value() {
    let mut plates = plates();
    let [_, right, _] = plates.bodies;
    plates.click(right);
    plates.doc.look(Look::StartPattern);
    plates.input(MotionField::Count, "4");
    plates.motion(MotionLook::Mode(PatternMode::Total));
    let long = format!("45{}", " + 0".repeat(63));
    assert!(long.len() <= varde_expr::MAX_LEN && long.len() + 6 > varde_expr::MAX_LEN);
    plates.input(MotionField::Spread, &long);
    let session = plates.doc.motion.as_ref().unwrap();
    assert_eq!(session.fields[MotionField::Spread.index()].error, None);
    assert_eq!(session.spread_error(), None);
    assert!(plates.doc.motion_ready());
    let pattern = drafted(&plates).expect("a draft");
    assert_eq!(spread(&pattern), 15.0);
    plates.motion(MotionLook::Flip);
    let pattern = drafted(&plates).expect("a draft");
    assert_eq!(spread(&pattern), -15.0);
    plates.doc.update(Edit::CommitMotion);
    let (id, _) = plates.last_feature();
    assert_eq!(spread(&last_pattern(&plates)), -15.0);
    // Edited, it's as typed.
    plates.doc.look(Look::EditFeature(id));
    let session = plates.doc.motion.as_ref().expect("editing it");
    assert_eq!(session.mode, PatternMode::Total);
    assert!(session.flip);
    plates.motion(MotionLook::Cancel);

    // A spacing of -15 whose text, turned, is too long.
    let design = plates.doc.editor.document().design();
    let text = format!("0 - 15{}", " + 0".repeat(62));
    let spacing = Value::new(&text, &Pattern::spacing_ask(&design)).unwrap();
    assert_eq!(spacing.value, -15.0);
    let PatternKind::Linear { along, count, .. } = last_pattern(&plates).kind else {
        panic!("a linear pattern");
    };
    set_pattern(
        &mut plates,
        id,
        PatternKind::Linear {
            along,
            count,
            spacing,
        },
    );
    plates.doc.look(Look::EditFeature(id));
    let session = plates.doc.motion.as_ref().expect("editing it");
    assert_eq!(session.mode, PatternMode::Spacing);
    assert!(session.flip);
    let field = &session.fields[MotionField::Spread.index()];
    assert_eq!(field.value.as_ref().map(|value| value.value), Some(15.0));
    assert_eq!(field.error, None);
    plates.doc.update(Edit::CommitMotion);
    assert_eq!(spread(&last_pattern(&plates)), -15.0);

    // A circular Spacing too long to multiply: refused only past a turn.
    plates.doc.look(Look::StartCircularPattern);
    plates.motion(MotionLook::Mode(PatternMode::Spacing));
    plates.input(MotionField::Spread, &format!("30{}", " + 0".repeat(63)));
    let session = plates.doc.motion.as_ref().unwrap();
    assert_eq!(session.spread_error(), None);
    let pattern = drafted(&plates).expect("a draft");
    assert_eq!(pattern.step_degrees(), Some(30.0));
    plates.input(MotionField::Spread, &format!("120{}", " + 0".repeat(63)));
    assert!(shows(&plates, "4 copies 120° apart go past a full turn"));
}

/// An undo setting the other kind of pattern in place of the one edited
/// ends the session, as the feature's removal would.
#[test]
fn an_undo_swapping_the_pattern_s_kind_ends_its_session() {
    let mut plates = plates();
    let [_, right, _] = plates.bodies;
    plates.click(right);
    plates.doc.look(Look::StartPattern);
    plates.doc.update(Edit::CommitMotion);
    let (id, _) = plates.last_feature();
    let design = plates.doc.editor.document().design();
    let circular = PatternKind::Circular {
        about: AxisRef::Origin(Axis3::Z),
        count: Value::new("4", &Pattern::count_ask(&design)).unwrap(),
        angle: Value::new("360", &Pattern::angle_ask(&design)).unwrap(),
    };
    set_pattern(&mut plates, id, circular);
    plates.doc.look(Look::EditFeature(id));
    let session = plates.doc.motion.as_ref().expect("editing it");
    assert_eq!(session.kind, MotionKind::CircularPattern);
    assert_eq!(session.mode, PatternMode::Full);
    plates.doc.update(Edit::Undo);
    assert!(plates.doc.motion.is_none());
    assert!(matches!(
        last_pattern(&plates).kind,
        PatternKind::Linear { .. }
    ));
    // The linear one edits as one again.
    plates.doc.look(Look::EditFeature(id));
    let session = plates.doc.motion.as_ref().expect("editing it");
    assert_eq!(session.kind, MotionKind::LinearPattern);
}

/// A pattern's direction on a body taken away is said to be gone, the
/// mock's words; editing it, the model shown while another is picked is
/// the bodies left where they are (the gone axis left out of that
/// preview), and another picked mends it.
#[test]
fn a_pattern_s_direction_on_a_body_taken_away_is_gone() {
    let mut plates = plates();
    let [_, right, _] = plates.bodies;
    let later = later_disc(&mut plates);
    plates.doc.look(Look::StartPattern);
    plates.click(right);
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    plates.answer();
    let wall = plates.face(later, |summary| matches!(summary, Summary::Cylinder { .. }));
    plates.click_at(later, Picked::Face(wall), DVec3::new(5.0, 30.0, 2.0));
    let axis = plates.doc.motion.as_ref().unwrap().axis;
    assert!(matches!(axis, Some(AxisRef::Face(face)) if face.body == later));
    plates.input(MotionField::Count, "2");
    plates.input(MotionField::Spread, "30");
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    assert!(plates.doc.motion_ready());
    // The disc's axis is along Z: the copy 30 up or down from it.
    let [low, high] = plates.bounds(right);
    assert!((high.z - low.z - 50.0).abs() < 1e-3, "{low} {high}");

    // Undone, the new one's direction is gone.
    plates.doc.update(Edit::Undo);
    assert!(plates.last_draft().is_none(), "nothing previewed");
    plates.answer();
    assert!(!plates.doc.motion_ready());
    assert!(shows(&plates, "The direction is gone: pick another"));
    plates.doc.update(Edit::Redo);
    plates.answer();
    assert!(plates.doc.motion_ready());
    plates.doc.update(Edit::CommitMotion);
    let (id, _) = plates.last_feature();
    plates.answer();
    assert!(plates.doc.feed.failed_features().is_empty());

    // The disc removed: the pattern stays, failing, and its edit says so.
    plates.doc.apply(Command::RemoveBody(later));
    assert_eq!(plates.doc.edit_error, None);
    plates.doc.sync();
    plates.answer();
    assert!(plates.doc.editor.document().feature(id).is_some());
    plates.doc.look(Look::EditFeature(id));
    assert!(plates.doc.motion.is_some());
    assert!(
        shows(&plates, "The direction is gone: pick another"),
        "{:?} {:?}",
        screen_texts(&plates.doc),
        plates.doc.motion
    );
    assert!(!plates.doc.motion_ready());
    // Picking another: the bodies where the pattern finds them, by a
    // move of nothing that names no axis.
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    let (feature, draft) = plates.last_draft().expect("a draft");
    assert_eq!(feature, Some(id));
    let FeatureKind::Move(neutral) = draft else {
        panic!("a move");
    };
    assert_eq!(neutral.bodies, [right]);
    assert_eq!(neutral.offset_vector(), DVec3::ZERO);
    assert_eq!(neutral.turn, None);
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    let [low, high] = plates.bounds(right);
    assert!((high.z - low.z - 20.0).abs() < 1e-3, "{low} {high}");
    plates.motion(MotionLook::OriginAxis(Axis3::Z));
    assert!(plates.doc.motion_ready());
    plates.doc.update(Edit::CommitMotion);
    plates.answer();
    assert!(plates.doc.feed.failed_features().is_empty());
    let [low, high] = plates.bounds(right);
    assert!((high.z - 45.0).abs() < 1e-3, "{low} {high}");
}

mod fuzz;

/// OK on a pattern edited straight away writes nothing, whichever way its
/// values were typed: a negative spacing typed "-15" (read with Flip on),
/// a full turn typed "360", a stored Total.
#[test]
fn ok_on_a_pattern_opened_writes_nothing_however_it_was_typed() {
    let mut plates = plates();
    let [_, right, _] = plates.bodies;
    plates.click(right);
    plates.doc.look(Look::StartPattern);
    plates.doc.update(Edit::CommitMotion);
    let (id, _) = plates.last_feature();
    let design = plates.doc.editor.document().design();
    let count = Value::new("3", &Pattern::count_ask(&design)).unwrap();
    let kinds = [
        PatternKind::Linear {
            along: AxisRef::Origin(Axis3::Y),
            count: count.clone(),
            spacing: Value::new("-15", &Pattern::spacing_ask(&design)).unwrap(),
        },
        PatternKind::Linear {
            along: AxisRef::Origin(Axis3::Y),
            count: count.clone(),
            spacing: Value::new("-(5 + 10)", &Pattern::spacing_ask(&design)).unwrap(),
        },
        PatternKind::Linear {
            along: AxisRef::Origin(Axis3::Y),
            count: count.clone(),
            spacing: Value::new("0 - 15", &Pattern::spacing_ask(&design)).unwrap(),
        },
        PatternKind::Circular {
            about: AxisRef::Origin(Axis3::Z),
            count: count.clone(),
            angle: Value::new("360", &Pattern::angle_ask(&design)).unwrap(),
        },
        PatternKind::Circular {
            about: AxisRef::Origin(Axis3::Z),
            count,
            angle: Value::new("100", &Pattern::angle_ask(&design)).unwrap(),
        },
    ];
    for kind in kinds {
        set_pattern(&mut plates, id, kind.clone());
        plates.doc.pattern_shapes.clear();
        let revision = plates.doc.editor.revision();
        plates.doc.look(Look::EditFeature(id));
        assert!(plates.doc.motion_ready(), "{kind:?}");
        plates.doc.update(Edit::CommitMotion);
        assert!(plates.doc.motion.is_none());
        assert_eq!(last_pattern(&plates).kind, kind);
        assert_eq!(plates.doc.editor.revision(), revision, "{kind:?}");
    }
}

/// What's picked going away while the axis is being picked: the body
/// holding the axis, and a body picked, taken by an undo mid-pick. Nothing
/// is previewed or committed while either is gone; an origin axis picked
/// and the gone body dropped mend it, and what's committed is that.
#[test]
fn what_goes_away_while_the_axis_is_picked_is_gone() {
    let mut plates = plates();
    let [_, right, _] = plates.bodies;
    let later = later_disc(&mut plates);
    plates.doc.look(Look::StartCircularPattern);
    plates.click(right);
    plates.click(later);
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    plates.answer();
    let wall = plates.face(later, |summary| matches!(summary, Summary::Cylinder { .. }));
    plates.click_at(later, Picked::Face(wall), DVec3::new(5.0, 30.0, 2.0));
    plates.answer();
    assert!(plates.doc.motion_ready());
    // Picking another axis, the disc is taken away.
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    plates.doc.update(Edit::Undo);
    plates.answer();
    assert!(plates.doc.motion.is_some());
    assert!(plates.last_draft().is_none(), "nothing previewed");
    assert!(!plates.doc.motion_ready());
    let session = plates.doc.motion.as_ref().unwrap();
    assert!(matches!(session.axis, Some(AxisRef::Face(face)) if face.body == later));
    plates.motion(MotionLook::OriginAxis(Axis3::Z));
    assert!(shows(&plates, "A picked body is gone"));
    assert!(!plates.doc.motion_ready());
    assert!(plates.last_draft().is_none(), "nothing previewed");
    plates.motion(MotionLook::Drop(later));
    assert!(plates.doc.motion_ready());
    plates.doc.update(Edit::CommitMotion);
    assert!(plates.doc.motion.is_none());
    let stored = last_pattern(&plates);
    assert_eq!(stored.bodies, [right]);
    assert_eq!(stored.kind.axis(), &AxisRef::Origin(Axis3::Z));
    plates.answer();
    assert!(plates.doc.feed.failed_features().is_empty());
}

mod separate;
