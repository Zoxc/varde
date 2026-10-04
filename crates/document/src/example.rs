//! [`Document::example`]: a plate with a hole.

use glam::DVec2;
use varde_expr::Value;
use varde_sketch::{Curve, Sketch};

use crate::{Command, Document, Editor, Extent, Extrude, Operation, OriginPlane, Plane};

/// The example plate's size in millimetres: its width, depth and
/// thickness, and its hole's radius.
const PLATE: [f64; 3] = [60.0, 40.0, 10.0];
const HOLE: f64 = 8.0;

impl Document {
    /// A sample design, for tests: "Sketch 1" on XY, a 60 × 40 mm
    /// rectangle about the origin with a hole of radius 8 mm in its middle,
    /// and "Extrude 1" making "Body 1" from the rectangle less the hole,
    /// 10 mm up, as the commands a user would give make them. New designs
    /// start from [`Document::default`], empty.
    pub fn example() -> Self {
        let mut editor = Editor::new(Document::default());
        let plane = Plane::Origin(OriginPlane::XY);
        editor
            .apply(editor.document().add_sketch(plane))
            .expect("a new design has ids");
        let sketch = editor.document().features()[0].id;

        let [width, depth, thickness] = PLATE;
        let mut plate = Sketch::default();
        let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(x, y)| DVec2::new(x * width / 2.0, y * depth / 2.0))
            .map(|at| plate.add_point(at).expect("a new sketch has ids"));
        for (k, &start) in corners.iter().enumerate() {
            let end = corners[(k + 1) % corners.len()];
            plate
                .add_curve(Curve::Line { start, end }, false)
                .expect("a new sketch has ids");
        }
        let center = plate.add_point(DVec2::ZERO).expect("a new sketch has ids");
        plate
            .add_curve(
                Curve::Circle {
                    center,
                    radius: HOLE,
                },
                false,
            )
            .expect("a new sketch has ids");
        let profiles = plate
            .profiles()
            .expect("a rectangle and a circle are simple");
        let region = profiles
            .regions
            .iter()
            .position(|region| region.holes.len() == 1)
            .and_then(|index| profiles.reference(index))
            .expect("the rectangle less the hole is a region");
        editor
            .apply(Command::SetSketch {
                feature: sketch,
                sketch: Box::new(plate),
            })
            .expect("the plate is a valid sketch");

        let design = editor.document().design();
        let distance = Value::new(&thickness.to_string(), &Extent::ask(&design))
            .expect("the thickness is a length");
        let extrude = Extrude {
            taper: None,
            sketch,
            regions: vec![region],
            extent: Extent::OneSide(distance),
            flip: false,
            operation: Operation::NewBody(crate::BodyId::NEW),
        };
        editor
            .apply(editor.document().add_feature(extrude.into()))
            .expect("the extrude is valid");
        editor.document().clone()
    }
}
