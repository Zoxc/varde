use glam::DVec3;
use varde_document::{
    Axis3, AxisRef, BodyId, CurveChain, Document, EdgeRef, FaceKey, FeatureKind, Helix, Operation,
    Orientation, PartKey, PathPart, PathRef, Sweep, Targets,
};
use varde_expr::Value;

use super::*;

/// The example's plate region swept as `path` says, doing `operation`.
fn swept(document: &Document, path: PathRef, operation: Operation) -> Sweep {
    let FeatureKind::Extrude(extrude) = &document.features()[1].kind else {
        panic!("the example's extrude");
    };
    Sweep {
        sketch: extrude.sketch,
        regions: extrude.regions.clone(),
        path,
        orientation: Orientation::FollowPath,
        twist: None,
        operation,
    }
}

/// The notes say what the plan's rows do: what it runs along, or the
/// helix's turns, and in the status bar the options and the operation.
#[test]
fn a_sweep_s_notes() {
    let document = Document::example();
    let design = document.design();
    let sketch = document.features()[0].id;
    let curves = PathPart::Curves(CurveChain {
        sketch,
        curves: Vec::new(),
    });
    let one = swept(
        &document,
        PathRef::Chain(vec![curves.clone()]),
        Operation::NewBody(BodyId::NEW),
    );
    assert_eq!(sweep_note(&document, &one), "along Sketch 1");
    assert_eq!(
        sweep_info(&document, &one),
        "Along Sketch 1 · Follow path · New body"
    );
    let edge = EdgeRef {
        body: document.bodies()[0].id,
        faces: [PartKey::StartCap, PartKey::EndCap].map(|part| FaceKey {
            feature: 1,
            part,
            instance: 0,
        }),
        near: DVec3::ZERO,
    };
    let edges = PathPart::Edges {
        edges: vec![edge],
        tangent: true,
    };
    let mut two = swept(
        &document,
        PathRef::Chain(vec![edges.clone()]),
        Operation::Join(Targets::default()),
    );
    assert_eq!(sweep_note(&document, &two), "along Body 1");
    two.path = PathRef::Chain(vec![edges, curves]);
    two.orientation = Orientation::Keep;
    two.twist = Some(Value::new("90", &Sweep::twist_ask(&design)).unwrap());
    assert_eq!(sweep_note(&document, &two), "along 2 parts");
    assert_eq!(
        sweep_info(&document, &two),
        "Along 2 parts · Keep orientation · Twist 90° · Join"
    );
    let helix = |turns: &str| {
        swept(
            &document,
            PathRef::Helix(Helix {
                axis: AxisRef::Origin(Axis3::Z),
                pitch: Value::new("4", &Sweep::pitch_ask(&design)).unwrap(),
                turns: Value::new(turns, &Sweep::turns_ask(&design)).unwrap(),
                left_handed: false,
                flip: false,
            }),
            Operation::Cut(Targets::default()),
        )
    };
    assert_eq!(sweep_note(&document, &helix("10")), "helix · 10 turns");
    assert_eq!(sweep_note(&document, &helix("1")), "helix · 1 turn");
    assert_eq!(
        sweep_info(&document, &helix("2.5")),
        "Helix · 2.5 turns · Cut"
    );
}
