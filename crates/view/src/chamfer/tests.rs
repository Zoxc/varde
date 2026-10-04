use glam::DVec3;
use varde_document::{Chamfer, ChamferSize, Document, EdgeRef, FaceKey, LengthUnit, PartKey};
use varde_expr::Value;

use super::*;

fn edge(curve: u64) -> EdgeRef {
    let key = |part| FaceKey {
        feature: 2,
        part,
        instance: 0,
    };
    EdgeRef {
        body: Document::example().bodies()[0].id,
        faces: [key(PartKey::EndCap), key(PartKey::Side { curve })],
        near: DVec3::ZERO,
    }
}

fn value(text: &str, ask: &varde_expr::Ask) -> Value {
    Value::new(text, ask).unwrap()
}

/// The notes say the size the mock's rows do, the first face's distance
/// first, and the status bar the count, the kind and tangent chains.
#[test]
fn a_chamfer_s_notes() {
    let design = Document::example().design();
    let length = Chamfer::distance_ask(&design);
    let angle = Chamfer::angle_ask(&design);
    let mut chamfer = Chamfer {
        edges: vec![edge(0)],
        distances: ChamferSize::Equal(value("1", &length)),
        chains: true,
        flip: false,
    };
    let mm = LengthUnit::Mm;
    assert_eq!(chamfer_note(&chamfer, mm), "1 mm");
    assert_eq!(
        chamfer_info(&chamfer, mm),
        "1 edge · Equal · 1 mm · Tangent chain"
    );
    chamfer.distances = ChamferSize::Two(value("1", &length), value("2", &length));
    chamfer.edges.push(edge(1));
    chamfer.chains = false;
    assert_eq!(chamfer_note(&chamfer, mm), "1 × 2");
    chamfer.flip = true;
    assert_eq!(chamfer_note(&chamfer, mm), "2 × 1");
    assert_eq!(
        chamfer_info(&chamfer, mm),
        "2 edges · Two distances · 2 × 1 mm"
    );
    chamfer.distances = ChamferSize::Angle(value("3", &length), value("30", &angle));
    assert_eq!(chamfer_note(&chamfer, mm), "3 mm 30°");
    assert_eq!(
        chamfer_info(&chamfer, mm),
        "2 edges · Distance and angle · 3 mm at 30°"
    );
}
