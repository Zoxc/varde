use varde_document::{FaceKey, FaceRef, Move, PartKey};
use varde_expr::Value;

use super::*;
use crate::probe::Shown;
use crate::testing::Laid;

fn field(text: &str) -> TypedField<'_> {
    TypedField {
        text,
        error: None,
        value: Some(0.0),
    }
}

fn state_of<'a>(kind: MotionKind, bodies: Vec<CombineBody<'a>>) -> MotionState<'a> {
    MotionState {
        kind,
        editing: None,
        bodies,
        picking: MotionPick::Bodies,
        fields: [
            field("0 mm"),
            field("0 mm"),
            field("0 mm"),
            field("0°"),
            field("3"),
            field("100 mm"),
        ],
        reference: Some("Z axis".to_owned()),
        line: None,
        bounds: None,
        centre: None,
        origin_axis: Some(Axis3::Z),
        units: LengthUnit::Mm,
        keep_original: true,
        flip: false,
        mode: PatternMode::Spacing,
        spread_error: None,
        copies: None,
        need: None,
        refused: None,
        error: None,
        show_error: None,
        checking: false,
        ready: false,
        accept: false,
        editable: true,
        hover: None,
    }
}

/// A body named `name`: the panel only sends ids, so they're all the
/// example's.
fn body(name: &str) -> CombineBody<'_> {
    let example = Document::example();
    CombineBody {
        body: example.bodies()[0].id,
        name,
    }
}

/// Each text of `state`'s panel and where it's laid out.
fn texts_of(state: &MotionState<'_>) -> Vec<Shown> {
    Laid::new(panel(state), iced::Size::new(400.0, 900.0)).texts()
}

fn found<'s>(shown: &'s [Shown], text: &str) -> &'s Shown {
    shown
        .iter()
        .find(|shown| shown.text == text)
        .unwrap_or_else(|| panic!("no {text:?} in {shown:?}"))
}

fn has(shown: &[Shown], text: &str) -> bool {
    shown.iter().any(|shown| shown.text == text)
}

#[test]
fn a_move_s_panel_is_the_mock_s_bodies_translate_and_rotate() {
    let state = state_of(MotionKind::Move, vec![body("Body 1"), body("Body 2")]);
    let shown = texts_of(&state);
    let order = [
        "New move",
        "Bodies",
        "Body 1",
        "Body 2",
        "Click bodies",
        "Translate",
        "X",
        "Y",
        "Z",
        "Rotate",
        "Axis",
        "Z axis",
        "Angle",
    ];
    let mut y = f32::MIN;
    for text in order {
        let at = found(&shown, text).bounds.y;
        assert!(at >= y, "{text} above what comes before it: {shown:?}");
        y = at;
    }
    // No Create copy: a move has none in the document.
    assert!(!has(&shown, "Create copy"));
    // Nothing picked: what to click, in each field.
    let mut state = state_of(MotionKind::Move, Vec::new());
    state.reference = None;
    state.picking = MotionPick::Reference;
    let shown = texts_of(&state);
    found(&shown, "Click bodies");
    found(&shown, "Click an axis or edge");
}

#[test]
fn a_mirror_s_panel_has_its_plane_and_create_copy() {
    let mut state = state_of(MotionKind::Mirror, vec![body("Body 1")]);
    state.reference = None;
    state.editing = Some("Mirror 2");
    let shown = texts_of(&state);
    for text in [
        "Mirror 2",
        "Bodies",
        "Body 1",
        "Plane",
        "Click a plane or face",
        "Create copy",
    ] {
        found(&shown, text);
    }
    assert!(!has(&shown, "Translate"));
    // A refusal shows as the operation's failure, without Add anyway.
    state.refused = Some("it names 0 bodies".to_owned());
    let shown = texts_of(&state);
    found(&shown, "Mirror fails");
    assert!(!has(&shown, "Add anyway"));
}

#[test]
fn notes_and_infos_are_the_mock_s() {
    let example = Document::example();
    let document = &example;
    let plate = document.bodies()[0].id;
    let design = document.design();
    let length = |text: &str| Value::new(text, &Move::offset_ask(&design)).unwrap();
    let angle = |text: &str| Value::new(text, &Move::angle_ask(&design)).unwrap();
    let mut moved = Move {
        bodies: vec![plate],
        offset: [length("-20"), length("-80"), length("0")],
        turn: Some((AxisRef::Origin(Axis3::Z), angle("30"))),
    };
    let units = design.units;
    assert_eq!(move_note(&moved, units), "82.462 mm 30°");
    assert_eq!(
        move_info(document, &moved),
        "Body 1 by -20, -80, 0 mm, 30° about Z axis"
    );
    moved.turn = None;
    assert_eq!(move_note(&moved, units), "82.462 mm");
    assert_eq!(move_info(document, &moved), "Body 1 by -20, -80, 0 mm");
    moved.offset = [length("0"), length("0"), length("0")];
    moved.turn = Some((AxisRef::Origin(Axis3::X), angle("90")));
    assert_eq!(move_note(&moved, units), "90°");
    assert_eq!(move_info(document, &moved), "Body 1 90° about X axis");

    let extrude = document.features()[1].id;
    let face = FaceRef {
        body: plate,
        key: FaceKey {
            feature: extrude.get(),
            part: PartKey::EndCap,
            instance: 0,
        },
        near: glam::DVec3::ZERO,
    };
    let mirror = varde_document::Mirror {
        bodies: vec![plate],
        plane: PlaneRef::Face(face),
        keep_original: true,
    };
    assert_eq!(plane_short(document, &mirror.plane), "Extrude 1's end");
    assert_eq!(
        mirror_info(document, &mirror),
        "Body 1 across Extrude 1's end · copy"
    );
    let mirror = varde_document::Mirror {
        plane: PlaneRef::Origin(OriginPlane::XY),
        keep_original: false,
        ..mirror
    };
    assert_eq!(plane_short(document, &mirror.plane), "XY");
    assert_eq!(mirror_info(document, &mirror), "Body 1 across XY plane");
}

#[test]
fn a_linear_pattern_s_panel_is_the_mock_s() {
    let mut state = state_of(MotionKind::LinearPattern, vec![body("Body 1")]);
    state.reference = Some("X axis".to_owned());
    let shown = texts_of(&state);
    let order = [
        "New linear pattern",
        "Bodies",
        "Body 1",
        "Direction",
        "X axis",
        "Flip direction",
        "Copies",
        "Count",
        "Spacing",
        "Total",
    ];
    let mut y = f32::MIN;
    for text in order {
        let at = found(&shown, text).bounds.y;
        assert!(at >= y, "{text} above what comes before it: {shown:?}");
        y = at;
    }
    // No Join to original: copies always join their body.
    assert!(!has(&shown, "Join to original"));
    assert!(!has(&shown, "Full 360°"));
    // The field is the mode's, its whole error under it.
    state.mode = PatternMode::Total;
    state.spread_error = Some("The pattern runs past 1000000 mm".to_owned());
    let shown = texts_of(&state);
    let labels = shown.iter().filter(|shown| shown.text == "Total").count();
    assert_eq!(labels, 2, "the tile and the field: {shown:?}");
    found(&shown, "The pattern runs past 1000000 mm");
}

#[test]
fn a_circular_pattern_s_panel_has_no_field_for_a_full_turn() {
    let mut state = state_of(MotionKind::CircularPattern, vec![body("Body 1")]);
    state.mode = PatternMode::Full;
    let shown = texts_of(&state);
    for text in [
        "New circular pattern",
        "Axis",
        "Z axis",
        "Copies",
        "Count",
        "Full 360°",
        "Spacing",
        "Total",
    ] {
        found(&shown, text);
    }
    assert!(!has(&shown, "Flip direction"));
    assert!(!has(&shown, "Direction"));
    // Full 360° has only the tile named Spacing; another mode its field.
    let spacings = |shown: &[Shown]| shown.iter().filter(|s| s.text == "Spacing").count();
    assert_eq!(spacings(&shown), 1);
    state.mode = PatternMode::Spacing;
    assert_eq!(spacings(&texts_of(&state)), 2);
}

#[test]
fn a_pattern_s_infos_are_the_mock_s() {
    let example = Document::example();
    let document = &example;
    let plate = document.bodies()[0].id;
    let design = document.design();
    let count = Value::new("4", &Pattern::count_ask(&design)).unwrap();
    let spacing = Value::new("-25", &Pattern::spacing_ask(&design)).unwrap();
    let mut pattern = Pattern {
        bodies: vec![plate],
        kind: PatternKind::Linear {
            along: AxisRef::Origin(Axis3::X),
            count: count.clone(),
            spacing,
        },
    };
    assert_eq!(pattern_note(&pattern), "×4");
    assert_eq!(
        pattern_copies(document, &pattern),
        "4 × 25 mm along X axis, flipped"
    );
    assert_eq!(
        pattern_info(document, &pattern),
        "Body 1 · 4 × 25 mm along X axis, flipped"
    );
    pattern.kind = PatternKind::Circular {
        about: AxisRef::Origin(Axis3::Z),
        count,
        angle: Value::new("360", &Pattern::angle_ask(&design)).unwrap(),
    };
    assert_eq!(
        pattern_info(document, &pattern),
        "Body 1 · 4 × 90° about Z axis"
    );
}
