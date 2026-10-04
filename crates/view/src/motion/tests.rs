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
            field("0 mm"),
            field("1"),
            field("1"),
            field("1"),
            field("1"),
            field(""),
            field("1 mm"),
            field("2 mm"),
            field("45°"),
            field("2 mm"),
            field("2 mm"),
            field("10 mm"),
            field("5"),
            field("0°"),
        ],
        reference: Some("Z axis".to_owned()),
        line: None,
        bounds: None,
        centre: None,
        origin_axis: Some(Axis3::Z),
        units: LengthUnit::Mm,
        keep_original: true,
        flip: false,
        join: true,
        warning: None,
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
        align: None,
        scale: None,
        split: None,
        chamfer: None,
        shell: None,
        fillet: None,
        offset_face: None,
        draft: None,
        sweep: None,
        loft: None,
        knobs: Vec::new(),
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
    // Join to original last, ticked.
    let join = found(&shown, "Join to original").bounds.y;
    assert!(join > found(&shown, "Total").bounds.y);
    assert!(!has(&shown, "Full 360°"));
    // Unticked, the overlap warning where there's nothing else.
    state.join = false;
    let warning = "The copies overlap (12 mm long this way): tick Join to original to merge them";
    state.warning = Some(warning.to_owned());
    found(&texts_of(&state), warning);
    state.refused = Some("it names 0 bodies".to_owned());
    assert!(!has(&texts_of(&state), warning));
    state.refused = None;
    state.warning = None;
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
    found(&shown, "Join to original");
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
        copies: Default::default(),
    };
    assert_eq!(pattern_note(&pattern), "×4");
    assert_eq!(
        pattern_copies(document, &pattern),
        "4 × 25 mm along X axis, flipped · joined"
    );
    assert_eq!(
        pattern_info(document, &pattern),
        "Body 1 · 4 × 25 mm along X axis, flipped · joined"
    );
    pattern.copies = varde_document::Copies::Separate(Vec::new());
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
    pattern.copies = Default::default();
    assert_eq!(
        pattern_info(document, &pattern),
        "Body 1 · 4 × 90° about Z axis · joined"
    );
}

/// An align's panel: the mock's Move panel's style, Body, From and To
/// with a Point, Direction and Second direction each, Flip, and Offset's
/// Distance and Angle; the status bar's words for it whole.
#[test]
fn an_align_s_panel_has_its_sides_flip_and_offset() {
    let mut state = state_of(MotionKind::Align, vec![body("Body 2")]);
    state.reference = None;
    state.picking = MotionPick::Align(AlignSlot::new(AlignSide::Target, AlignRole::Point));
    state.align = Some(Box::new(AlignView {
        names: [
            [
                Some("Centre of an edge of Body 2".to_owned()),
                Some("Edge of Body 2".to_owned()),
                None,
            ],
            [None, None, None],
        ],
        info: None,
        marks: Default::default(),
        snaps: None,
    }));
    let shown = texts_of(&state);
    let order = [
        "New align",
        "Body",
        "Body 2",
        "From",
        "Centre of an edge of Body 2",
        "Edge of Body 2",
        "To",
        "Click a corner, middle, centre or origin",
        "Flip",
        "Offset",
        "Distance",
        "Angle",
    ];
    let mut y = f32::MIN;
    for text in order {
        let at = found(&shown, text).bounds.y;
        assert!(at >= y, "{text} above what comes before it: {shown:?}");
        y = at;
    }
    assert!(!has(&shown, "Translate") && !has(&shown, "Bodies"));
    assert_eq!(status_info(&state), "Body 2");
    if let Some(align) = &mut state.align {
        align.info = Some("Body 2 to Body 1".to_owned());
    }
    assert_eq!(status_info(&state), "Body 2 to Body 1");
    state.need = Some("pick the point to align it to");
    assert_eq!(status_info(&state), "pick the point to align it to");
}

#[test]
fn a_scale_s_notes() {
    use varde_document::{EdgeRef, PointRef, Scale, ScaleFactor};
    let example = Document::example();
    let document = &example;
    let plate = document.bodies()[0].id;
    let design = document.design();
    let factor = |text: &str| Value::new(text, &Scale::factor_ask(&design)).unwrap();
    let mut scale = Scale {
        bodies: vec![plate],
        about: PointRef::Origin,
        factor: ScaleFactor::Uniform(factor("2")),
    };
    let units = design.units;
    assert_eq!(scale_note(&scale, units), "×2");
    assert_eq!(scale_info(document, &scale, units), "Body 1 ×2");
    scale.factor = ScaleFactor::PerAxis([factor("1"), factor("1"), factor("25.4")]);
    assert_eq!(scale_note(&scale, units), "×1 · 1 · 25.4");
    let key = FaceKey {
        feature: 1,
        part: PartKey::EndCap,
        instance: 0,
    };
    scale.factor = ScaleFactor::EdgeLength {
        edge: EdgeRef {
            body: plate,
            faces: [key, key],
            near: glam::DVec3::ZERO,
        },
        length: Value::new("50", &Scale::length_ask(&design)).unwrap(),
        axis_only: false,
    };
    assert_eq!(scale_note(&scale, units), "edge → 50 mm");
    assert_eq!(scale_note(&scale, LengthUnit::In), "edge → 1.9685 in");
}

/// A split's note names its tool, and the side kept for a trim.
#[test]
fn a_split_s_notes() {
    use varde_document::{Keep, OriginPlane, PlaneRef, Side, Split, SplitTool};
    let example = Document::example();
    let document = &example;
    let plate = document.bodies()[0].id;
    let extrude = document.features()[1].id;
    let sketch = document.features()[0].id;
    let mut split = Split {
        body: plate,
        tool: SplitTool::Plane(PlaneRef::Origin(OriginPlane::XY)),
        original: Side::Front,
        keep: Keep::Both,
        new_body: None,
    };
    assert_eq!(split_note(document, &split), "by XY");
    assert_eq!(split_info(document, &split), "Body 1 by XY");
    split.keep = Keep::Back;
    assert_eq!(split_note(document, &split), "by XY · back only");
    split.keep = Keep::Both;
    split.tool = SplitTool::Face(FaceRef {
        body: plate,
        key: FaceKey {
            feature: extrude.get(),
            part: PartKey::EndCap,
            instance: 0,
        },
        near: glam::DVec3::ZERO,
    });
    assert_eq!(split_note(document, &split), "by Extrude 1's end");
    split.tool = SplitTool::Body(plate);
    assert_eq!(split_note(document, &split), "by Body 1");
    split.tool = SplitTool::Chain {
        sketch,
        curves: Vec::new(),
    };
    assert_eq!(split_note(document, &split), "by Sketch 1");
}

/// A scale's panel, built in the mock's Move panel's style: Bodies, the
/// Point, Scale's three tiles and the mode's fields; to an edge's
/// length, the edge with its length now, Length and Along its axis only
/// where offered; the status bar's words for it whole.
#[test]
fn a_scale_s_panel_has_its_point_modes_and_fields() {
    let mut state = state_of(MotionKind::Scale, vec![body("Body 1")]);
    state.reference = None;
    let view = |mode, offered| ScaleView {
        mode,
        point: "Origin".to_owned(),
        at: Some(DVec3::ZERO),
        edge: Some("Edge of Body 1".to_owned()),
        length: Some("60 mm".to_owned()),
        offered,
        axis_only: false,
        info: None,
        snaps: None,
    };
    state.scale = Some(Box::new(view(ScaleMode::Uniform, true)));
    let shown = texts_of(&state);
    let order = [
        "New scale",
        "Bodies",
        "Body 1",
        "Point",
        "Origin",
        "Scale",
        "Uniform",
        "Factor",
    ];
    let mut y = f32::MIN;
    for text in order {
        let at = found(&shown, text).bounds.y;
        assert!(at >= y, "{text} above what comes before it: {shown:?}");
        y = at;
    }
    assert!(has(&shown, "Per axis") && has(&shown, "Edge length"));
    assert!(!has(&shown, "Translate") && !has(&shown, "Edge of Body 1"));

    state.scale = Some(Box::new(view(ScaleMode::PerAxis, true)));
    let shown = texts_of(&state);
    assert!(["X", "Y", "Z"].iter().all(|axis| has(&shown, axis)));
    assert!(!has(&shown, "Factor"));

    state.scale = Some(Box::new(view(ScaleMode::EdgeLength, true)));
    let shown = texts_of(&state);
    let order = ["Edge", "Edge of Body 1", "Length", "Along its axis only"];
    let mut y = f32::MIN;
    for text in order {
        let at = found(&shown, text).bounds.y;
        assert!(at >= y, "{text} above what comes before it: {shown:?}");
        y = at;
    }
    // The edge's length now beside it.
    let edge = found(&shown, "Edge of Body 1").bounds;
    let length = found(&shown, "60 mm").bounds;
    assert!((length.y - edge.y).abs() < 4.0 && length.x > edge.x);
    state.scale = Some(Box::new(view(ScaleMode::EdgeLength, false)));
    assert!(!has(&texts_of(&state), "Along its axis only"));

    assert_eq!(status_info(&state), "Body 1");
    if let Some(scale) = &mut state.scale {
        scale.info = Some("Body 1 ×2".to_owned());
    }
    assert_eq!(status_info(&state), "Body 1 ×2");
}

/// A split's view: the Body picked, its tool, both kept with the front
/// keeping the id.
fn split_view(tool: Option<(&str, Option<&str>)>) -> SplitView<'static> {
    SplitView {
        mode: SplitMode::Regions,
        tool: tool.map(|(name, meta)| (name.to_owned(), meta.map(str::to_owned))),
        body: Some("Body 1"),
        original: Side::Front,
        keep: Keep::Both,
        later: None,
        info: None,
        candidates: Vec::new(),
        source: None,
        picked: SplitView::none_picked(),
        lines: Vec::new(),
        chain: None,
        pieces: Vec::new(),
    }
}

#[test]
fn a_split_s_panel_has_its_body_tool_and_what_it_keeps() {
    let mut state = state_of(MotionKind::Split, vec![body("Body 1")]);
    state.reference = None;
    state.split = Some(Box::new(split_view(Some(("Sketch 2", Some("2 regions"))))));
    let shown = texts_of(&state);
    let order = [
        "New split",
        "Body",
        "Body 1",
        "Split with",
        "Face",
        "Sketch 2",
        "Keeps Body 1",
        "Front",
        "Keep",
        "Both",
    ];
    let mut y = f32::MIN;
    for text in order {
        let at = found(&shown, text).bounds.y;
        assert!(at >= y, "{text} above what comes before it: {shown:?}");
        y = at;
    }
    for text in ["Body", "Region", "Line", "Back", "2 regions"] {
        found(&shown, text);
    }
    // The tool's count beside its sketch.
    let tool = found(&shown, "Sketch 2").bounds;
    let count = found(&shown, "2 regions").bounds;
    assert!((count.y - tool.y).abs() < 4.0 && count.x > tool.x);
    assert!(!has(&shown, "Click regions of a sketch"));
    assert!(!has(&shown, "Bodies") && !has(&shown, "Plane"));

    // Picking its tool, with none yet: where to click.
    state.picking = MotionPick::Tool;
    state.split = Some(Box::new(split_view(None)));
    found(&texts_of(&state), "Click regions of a sketch");

    // The later features' warning, under which piece keeps the id.
    if let Some(split) = &mut state.split {
        split.later = Some("2 later features use Body 1: they'll get the back piece".to_owned());
    }
    let shown = texts_of(&state);
    let warning = found(
        &shown,
        "2 later features use Body 1: they'll get the back piece",
    );
    assert!(warning.bounds.y > found(&shown, "Keeps Body 1").bounds.y);
    assert!(warning.bounds.y < found(&shown, "Both").bounds.y);

    assert_eq!(status_info(&state), "Body 1");
    if let Some(split) = &mut state.split {
        split.info = Some("Body 1 by XY".to_owned());
    }
    state.need = None;
    assert_eq!(status_info(&state), "Body 1 by XY");
    state.need = Some("pick the regions of a sketch to split with");
    assert_eq!(
        status_info(&state),
        "pick the regions of a sketch to split with"
    );
}

#[test]
fn a_trim_s_kept_piece_keeps_the_id_whatever_the_original() {
    let mut view = split_view(None);
    view.original = Side::Front;
    view.keep = Keep::Back;
    assert_eq!(view.kept(), Side::Back);
    view.keep = Keep::Both;
    view.original = Side::Back;
    assert_eq!(view.kept(), Side::Back);
    view.original = Side::Front;
    assert_eq!(view.kept(), Side::Front);
}

/// A chamfer's view: the edges `edges` (name, meta, round), of `kind`.
fn chamfer_view(edges: &[(&str, Option<&str>, bool)], kind: ChamferType) -> ChamferView {
    let example = Document::example();
    let body = example.bodies()[0].id;
    let key = |curve| FaceKey {
        feature: 1,
        part: PartKey::Side { curve },
        instance: 0,
    };
    let edges = (edges.iter().enumerate())
        .map(|(at, &(name, meta, round))| BlendEdge {
            edge: varde_document::EdgeRef {
                body,
                faces: [key(0), key(1)],
                near: glam::DVec3::new(at as f64, 0.0, 0.0),
            },
            name: name.to_owned(),
            meta: meta.map(str::to_owned),
            round,
        })
        .collect();
    ChamferView {
        edges: BlendEdges {
            edges,
            chains: true,
        },
        kind,
        info: None,
    }
}

/// A chamfer's panel is the mock's: Edges (each with its measure beside
/// it), Type's tiles, the type's fields, Flip sides but for Equal, and
/// the Tangent chain tick the mock has on the fillet's only; no Bodies.
#[test]
fn a_chamfer_s_panel_is_the_mock_s() {
    let mut state = state_of(MotionKind::Chamfer, vec![body("Body 1")]);
    state.reference = None;
    state.picking = MotionPick::Edges;
    let edges = [
        ("Edge 1", Some("60 mm"), false),
        ("Edge 2", Some("Ø16 mm"), true),
    ];
    state.chamfer = Some(Box::new(chamfer_view(&edges, ChamferType::Equal)));
    let shown = texts_of(&state);
    let order = [
        "New chamfer",
        "Edges",
        "Edge 1",
        "Edge 2",
        "Click edges",
        "Type",
        "Equal",
        "Distance",
        "Tangent chain",
    ];
    let mut y = f32::MIN;
    for text in order {
        let at = found(&shown, text).bounds.y;
        assert!(at >= y, "{text} above what comes before it: {shown:?}");
        y = at;
    }
    for text in ["Two distances", "Distance and angle", "60 mm", "Ø16 mm"] {
        found(&shown, text);
    }
    let edge = found(&shown, "Edge 1").bounds;
    let meta = found(&shown, "60 mm").bounds;
    assert!((meta.y - edge.y).abs() < 4.0 && meta.x > edge.x);
    for text in [
        "Flip sides",
        "Distance 1",
        "Distance 2",
        "Angle",
        "Bodies",
        "Body 1",
    ] {
        assert!(!has(&shown, text), "{text}");
    }

    // Two distances: Distance 1 and 2, then Flip sides.
    state.chamfer = Some(Box::new(chamfer_view(&edges, ChamferType::Two)));
    state.picking = MotionPick::Nothing;
    let shown = texts_of(&state);
    assert!(!has(&shown, "Click edges"), "not picking");
    let mut y = f32::MIN;
    for text in ["Distance 1", "Distance 2", "Flip sides", "Tangent chain"] {
        let at = found(&shown, text).bounds.y;
        assert!(at >= y, "{text} above what comes before it: {shown:?}");
        y = at;
    }
    // Distance and angle.
    state.chamfer = Some(Box::new(chamfer_view(&edges, ChamferType::Angle)));
    let shown = texts_of(&state);
    let mut y = f32::MIN;
    for text in ["Distance", "Angle", "Flip sides", "Tangent chain"] {
        let at = found(&shown, text).bounds.y;
        assert!(at >= y, "{text} above what comes before it: {shown:?}");
        y = at;
    }
    assert!(!has(&shown, "Distance 2"));

    state.need = Some("pick the edges to chamfer");
    assert_eq!(status_info(&state), "pick the edges to chamfer");
    state.need = None;
    if let Some(chamfer) = &mut state.chamfer {
        chamfer.info = Some("2 edges · Equal · 1 mm · Tangent chain".to_owned());
    }
    assert_eq!(
        status_info(&state),
        "2 edges · Equal · 1 mm · Tangent chain"
    );
}

/// A fillet's panel is the mock's: Edges (each with its measure beside
/// it), Radius and Tangent chain; no Bodies, no Type, no Flip sides.
#[test]
fn a_fillet_s_panel_is_the_mock_s() {
    let mut state = state_of(MotionKind::Fillet, vec![body("Body 1")]);
    state.reference = None;
    state.picking = MotionPick::Edges;
    let edges = [
        ("Edge 1", Some("60 mm"), false),
        ("Edge 2", Some("Ø16 mm"), true),
    ];
    let edges = chamfer_view(&edges, ChamferType::Equal).edges;
    state.fillet = Some(Box::new(FilletView { edges, info: None }));
    let shown = texts_of(&state);
    let order = [
        "New fillet",
        "Edges",
        "Edge 1",
        "Edge 2",
        "Click edges",
        "Radius",
        "Tangent chain",
    ];
    let mut y = f32::MIN;
    for text in order {
        let at = found(&shown, text).bounds.y;
        assert!(at >= y, "{text} above what comes before it: {shown:?}");
        y = at;
    }
    for text in ["60 mm", "Ø16 mm"] {
        found(&shown, text);
    }
    for text in [
        "Type",
        "Equal",
        "Flip sides",
        "Distance",
        "Bodies",
        "Body 1",
    ] {
        assert!(!has(&shown, text), "{text}");
    }
    state.need = Some("pick the edges to fillet");
    assert_eq!(status_info(&state), "pick the edges to fillet");
    state.need = None;
    if let Some(fillet) = &mut state.fillet {
        fillet.info = Some("2 edges · R2 mm · Tangent chain".to_owned());
    }
    assert_eq!(status_info(&state), "2 edges · R2 mm · Tangent chain");
}

/// A shell's view: the faces `faces` (name, meta), `direction`.
fn shell_view(faces: &[(&str, Option<&str>)], direction: ShellDirection) -> ShellView {
    let example = Document::example();
    let body = example.bodies()[0].id;
    let faces = (faces.iter().enumerate())
        .map(|(at, &(name, meta))| PickedFace {
            face: FaceRef {
                body,
                key: FaceKey {
                    feature: 1,
                    part: PartKey::Side { curve: 0 },
                    instance: 0,
                },
                near: glam::DVec3::new(at as f64, 0.0, 0.0),
            },
            name: name.to_owned(),
            meta: meta.map(str::to_owned),
        })
        .collect();
    ShellView {
        faces: PickedFaces { faces },
        direction,
        info: None,
    }
}

/// A shell's panel is the mock's: Remove (each face with its kind beside
/// it, and where to click while picking or with none), Thickness, and
/// Direction's Inward and Outward tiles; no Bodies. Its warning for no
/// faces shows in the foot, and the status bar says it as the mock's row.
#[test]
fn a_shell_s_panel_is_the_mock_s() {
    let mut state = state_of(MotionKind::Shell, vec![body("Body 1")]);
    state.reference = None;
    state.picking = MotionPick::Faces;
    let faces = [("Face 1", Some("Planar face")), ("Face 2", None)];
    state.shell = Some(Box::new(shell_view(&faces, ShellDirection::Inward)));
    let shown = texts_of(&state);
    let order = [
        "New shell",
        "Remove",
        "Face 1",
        "Face 2",
        "Click faces",
        "Thickness",
        "Direction",
        "Inward",
    ];
    let mut y = f32::MIN;
    for text in order {
        let at = found(&shown, text).bounds.y;
        assert!(at >= y, "{text} above what comes before it: {shown:?}");
        y = at;
    }
    let inward = found(&shown, "Inward").bounds;
    let outward = found(&shown, "Outward").bounds;
    assert!((inward.y - outward.y).abs() < 1.0 && outward.x > inward.x);
    let face = found(&shown, "Face 1").bounds;
    let meta = found(&shown, "Planar face").bounds;
    assert!((meta.y - face.y).abs() < 4.0 && meta.x > face.x);
    for text in ["Bodies", "Body 1", "Plane", "Tangent chain"] {
        assert!(!has(&shown, text), "{text}");
    }
    // Not picking, with faces: no place to click.
    state.picking = MotionPick::Nothing;
    assert!(!has(&texts_of(&state), "Click faces"));
    // None: where to click, and the mock's warning in the foot.
    state.shell = Some(Box::new(shell_view(&[], ShellDirection::Outward)));
    let warning = "No faces removed: the body becomes closed and hollow";
    state.warning = Some(warning.to_owned());
    let shown = texts_of(&state);
    found(&shown, "Click faces");
    assert!(found(&shown, warning).bounds.y > found(&shown, "Outward").bounds.y);

    state.need = Some("pick faces to remove, or the body to hollow");
    assert_eq!(
        status_info(&state),
        "pick faces to remove, or the body to hollow"
    );
    state.need = None;
    if let Some(shell) = &mut state.shell {
        shell.info = Some("Closed · 2 mm outward".to_owned());
    }
    assert_eq!(status_info(&state), "Closed · 2 mm outward");
}

/// An offset face's panel, in the style of the mock's shell panel:
/// Faces (each with its kind, and where to click), Distance, Inward and
/// Tangent faces; no Bodies, no Direction tiles. The status bar says
/// its info once whole, else what's needed.
#[test]
fn an_offset_face_s_panel() {
    let mut state = state_of(MotionKind::OffsetFace, Vec::new());
    state.reference = None;
    state.picking = MotionPick::Faces;
    let faces = shell_view(&[("Face 1", Some("Planar face"))], ShellDirection::Inward).faces;
    state.offset_face = Some(Box::new(crate::OffsetFaceView {
        faces,
        inward: false,
        tangent: true,
        handle: None,
        info: Some("1 face · 1 mm outward".to_owned()),
    }));
    let shown = texts_of(&state);
    let order = [
        "New offset face",
        "Faces",
        "Face 1",
        "Click faces",
        "Distance",
        "Inward",
        "Tangent faces",
    ];
    let mut y = f32::MIN;
    for text in order {
        let at = found(&shown, text).bounds.y;
        assert!(at >= y, "{text} above what comes before it: {shown:?}");
        y = at;
    }
    for text in ["Bodies", "Plane", "Direction", "Outward", "Remove"] {
        assert!(!has(&shown, text), "{text}");
    }
    assert_eq!(status_info(&state), "1 face · 1 mm outward");
    state.need = Some("pick the faces to move");
    assert_eq!(status_info(&state), "pick the faces to move");
}

/// A draft's panel, in the style of the mock's shell panel: Faces (each
/// with its kind, and where to click), Neutral plane (the plane's row),
/// Angle, Flip and Tangent faces; no Bodies, no Direction tiles. The
/// status bar says its info once whole, else what's needed.
#[test]
fn a_draft_s_panel() {
    let mut state = state_of(MotionKind::Draft, Vec::new());
    state.reference = Some("XY plane".to_owned());
    state.picking = MotionPick::Faces;
    let faces = shell_view(&[("Face 1", Some("Planar face"))], ShellDirection::Inward).faces;
    state.draft = Some(Box::new(crate::DraftView {
        faces,
        flip: false,
        tangent: true,
        info: Some("1 face · 3° from XY".to_owned()),
    }));
    let shown = texts_of(&state);
    let order = [
        "New draft",
        "Faces",
        "Face 1",
        "Click faces",
        "Neutral plane",
        "XY plane",
        "Angle",
        "Flip",
        "Tangent faces",
    ];
    let mut y = f32::MIN;
    for text in order {
        let at = found(&shown, text).bounds.y;
        assert!(at >= y, "{text} above what comes before it: {shown:?}");
        y = at;
    }
    for text in ["Bodies", "Direction", "Inward", "Remove", "Distance"] {
        assert!(!has(&shown, text), "{text}");
    }
    assert_eq!(status_info(&state), "1 face · 3° from XY");
    state.need = Some("pick the faces to draft");
    assert_eq!(status_info(&state), "pick the faces to draft");
}
