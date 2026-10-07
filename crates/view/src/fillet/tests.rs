use glam::DVec3;
use varde_document::{Document, EdgeRef, FaceKey, Fillet, LengthUnit, PartKey};
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

/// The notes say what the mock's rows do: "R2", and in the status bar
/// the edges, the radius with its unit and the tangent chain.
#[test]
fn a_fillet_s_notes() {
    let example = Document::example();
    let design = example.design();
    let ask = Fillet::radius_ask(&design);
    let mut fillet = Fillet {
        edges: vec![edge(0), edge(1)],
        radius: Value::new("2", &ask).unwrap(),
        chains: true,
    };
    let mm = LengthUnit::Mm;
    assert_eq!(fillet_note(&fillet, mm), "R2");
    assert_eq!(fillet_info(&fillet, mm), "2 edges · R2 mm · Tangent chain");
    fillet.edges.pop();
    fillet.chains = false;
    fillet.radius = Value::new("0.5", &ask).unwrap();
    assert_eq!(fillet_note(&fillet, mm), "R0.5");
    assert_eq!(fillet_info(&fillet, mm), "1 edge · R0.5 mm");
    // In inches, a radius typed in millimetres: as the units show it.
    assert_eq!(fillet_note(&fillet, LengthUnit::In), "R0.0197");
}
