use glam::DVec3;
use varde_document::{Document, FaceDraft, FaceKey, FaceRef, OriginPlane, PartKey, PlaneRef};
use varde_expr::Value;

use super::*;

fn face(part: PartKey) -> FaceRef {
    FaceRef {
        body: Document::example().bodies()[0].id,
        key: FaceKey {
            feature: Document::example().features()[1].id.get(),
            part,
            instance: 0,
        },
        near: DVec3::ZERO,
    }
}

/// The notes say the angle and how many faces, and in the status bar
/// the neutral plane and whether the pull is flipped.
#[test]
fn a_draft_s_notes() {
    let document = Document::example();
    let ask = FaceDraft::angle_ask(&document.design());
    let mut draft = FaceDraft {
        faces: vec![face(PartKey::Side { curve: 1 })],
        neutral: PlaneRef::Origin(OriginPlane::XY),
        angle: Value::new("3", &ask).unwrap(),
        flip: false,
        tangent: true,
    };
    assert_eq!(draft_note(&draft), "3° · 1 face");
    assert_eq!(draft_info(&document, &draft), "1 face · 3° from XY");
    draft.faces.push(face(PartKey::Side { curve: 2 }));
    draft.neutral = PlaneRef::Face(face(PartKey::StartCap));
    draft.angle = Value::new("1.5", &ask).unwrap();
    draft.flip = true;
    assert_eq!(draft_note(&draft), "1.5° · 2 faces");
    assert_eq!(
        draft_info(&document, &draft),
        "2 faces · 1.5° from Extrude 1's start, flipped"
    );
}
