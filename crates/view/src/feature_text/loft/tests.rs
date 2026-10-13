use glam::DVec2;
use varde_document::{BodyId, CurveChain, Document, Operation, Section, Targets};
use varde_sketch::Id;

use super::*;

/// The notes count the sections, and the status bar says the mode (two
/// sections ruled whatever), closed, the rails and the operation.
#[test]
fn a_loft_s_notes() {
    let example = Document::example();
    let sketch = example.features()[0].id;
    let point = |id: Id| Section::Point { sketch, point: id };
    let region = Section::Region {
        sketch,
        region: varde_sketch::RegionRef {
            curves: Vec::new(),
            holes: Vec::new(),
            inside: DVec2::ZERO,
        },
        start: None,
    };
    let first_point = |document: &Document| match &document.features()[0].kind {
        varde_document::FeatureKind::Sketch { sketch, .. } => sketch.points[0].id,
        _ => unreachable!(),
    };
    let mut loft = Loft {
        sections: vec![region.clone(), point(first_point(&example))],
        mode: LoftMode::Smooth,
        closed: false,
        rails: Vec::new(),
        operation: Operation::NewBody(BodyId::NEW),
    };
    assert_eq!(loft_note(&loft), "2 sections");
    assert_eq!(loft_info(&loft), "2 sections · Ruled · New body");
    loft.sections = vec![region.clone(), region.clone(), region];
    loft.closed = true;
    loft.operation = Operation::Join(Targets::default());
    assert_eq!(loft_note(&loft), "3 sections");
    assert_eq!(loft_info(&loft), "3 sections · Smooth · Closed · Join");
    loft.closed = false;
    loft.mode = LoftMode::Ruled;
    loft.rails = vec![
        CurveChain {
            sketch,
            curves: Vec::new(),
        };
        2
    ];
    assert_eq!(loft_info(&loft), "3 sections · Ruled · 2 rails · Join");
    loft.rails.truncate(1);
    assert_eq!(loft_info(&loft), "3 sections · Ruled · 1 rail · Join");
}
