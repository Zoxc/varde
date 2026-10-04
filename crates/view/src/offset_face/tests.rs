use glam::DVec3;
use varde_document::{Document, FaceKey, FaceRef, LengthUnit, OffsetFace, PartKey};
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

/// The notes say the distance and how many faces, and in the status bar
/// the side.
#[test]
fn an_offset_face_s_notes() {
    let design = Document::example().design();
    let ask = OffsetFace::distance_ask(&design);
    let mut offset = OffsetFace {
        faces: vec![face(PartKey::EndCap)],
        distance: Value::new("2", &ask).unwrap(),
        inward: false,
        tangent: true,
    };
    let mm = LengthUnit::Mm;
    assert_eq!(offset_note(&offset, mm), "2 mm · 1 face");
    assert_eq!(offset_info(&offset, mm), "1 face · 2 mm outward");
    offset.faces.push(face(PartKey::StartCap));
    offset.inward = true;
    offset.distance = Value::new("0.5", &ask).unwrap();
    assert_eq!(offset_note(&offset, mm), "0.5 mm in · 2 faces");
    assert_eq!(offset_info(&offset, mm), "2 faces · 0.5 mm inward");
}
