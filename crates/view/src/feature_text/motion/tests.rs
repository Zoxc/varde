use varde_document::{
    Axis3, AxisRef, FaceKey, FaceRef, Move, OriginPlane, PartKey, PatternKind, Plane,
};
use varde_expr::Value;

use super::*;
use crate::motion::pattern_copies;

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
        plane: Plane::Face(face),
        keep_original: true,
    };
    assert_eq!(plane_short(document, &mirror.plane), "Extrude 1's end");
    assert_eq!(
        mirror_info(document, &mirror),
        "Body 1 across Extrude 1's end · copy"
    );
    let mirror = varde_document::Mirror {
        plane: Plane::Origin(OriginPlane::XY),
        keep_original: false,
        ..mirror
    };
    assert_eq!(plane_short(document, &mirror.plane), "XY");
    assert_eq!(mirror_info(document, &mirror), "Body 1 across XY plane");
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
    use varde_document::{Keep, OriginPlane, Plane, Side, Split, SplitTool};
    let example = Document::example();
    let document = &example;
    let plate = document.bodies()[0].id;
    let extrude = document.features()[1].id;
    let sketch = document.features()[0].id;
    let mut split = Split {
        body: plate,
        tool: SplitTool::Plane(Plane::Origin(OriginPlane::XY)),
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
