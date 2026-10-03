//! The pattern sessions: `P` and the circular pattern, their modes, the
//! mock's errors, editing from the Timeline.

use std::f64::consts::TAU;

use glam::DVec3;
use varde_document::{Axis3, AxisRef, FeatureKind, Pattern, PatternKind};
use varde_view::{Edit, Look, MotionField, MotionKind, MotionLook, PatternMode};

use super::{Plates, character, enter, near, plates};
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
    assert!(!shows(&plates, "Join to original"));

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
