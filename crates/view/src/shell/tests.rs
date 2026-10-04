use glam::DVec3;
use varde_document::{Document, FaceKey, FaceRef, LengthUnit, PartKey, Shell};
use varde_expr::Value;

use super::*;

fn face(part: PartKey) -> FaceRef {
    FaceRef {
        body: Document::example().bodies()[0].id,
        key: FaceKey {
            feature: 2,
            part,
            instance: 0,
        },
        near: DVec3::ZERO,
    }
}

/// The notes say what the mock's rows do: the thickness, and in the
/// status bar the faces removed (or closed) and the direction.
#[test]
fn a_shell_s_notes() {
    let design = Document::example().design();
    let ask = Shell::thickness_ask(&design);
    let mut shell = Shell {
        body: Document::example().bodies()[0].id,
        open: vec![face(PartKey::EndCap)],
        thickness: Value::new("2", &ask).unwrap(),
        outward: false,
    };
    let mm = LengthUnit::Mm;
    assert_eq!(shell_note(&shell, mm), "2 mm");
    assert_eq!(shell_info(&shell, mm), "1 face removed · 2 mm inward");
    shell.open.push(face(PartKey::StartCap));
    assert_eq!(shell_info(&shell, mm), "2 faces removed · 2 mm inward");
    shell.open.clear();
    shell.outward = true;
    shell.thickness = Value::new("0.5", &ask).unwrap();
    assert_eq!(shell_info(&shell, mm), "Closed · 0.5 mm outward");
}
