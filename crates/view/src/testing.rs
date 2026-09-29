//! What the crate's tests build sketches with.

use glam::DVec2;
use varde_expr::{LengthUnit, Value};
use varde_sketch::{Curve, Design, Dimension, Handle, Id, Measure, Sketch, Spline};

use crate::typed::{DEFAULT_SIDES, Field};
use crate::{ActiveTool, Target, Tool};

/// A design in millimetres.
pub(crate) const DESIGN: Design = Design {
    max: 1e6,
    units: LengthUnit::Mm,
};

pub(crate) fn at(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

pub(crate) fn point(sketch: &mut Sketch, x: f64, y: f64) -> Id {
    sketch.add_point(at(x, y)).unwrap()
}

pub(crate) fn line(sketch: &mut Sketch, start: Id, end: Id) -> Id {
    sketch.add_curve(Curve::Line { start, end }, false).unwrap()
}

/// An open spline through fit points at `places`: it and its fit points.
pub(crate) fn spline(sketch: &mut Sketch, places: &[(f64, f64)]) -> (Id, Vec<Id>) {
    let fit: Vec<Id> = places.iter().map(|&(x, y)| point(sketch, x, y)).collect();
    let curve = Curve::Spline(Spline::through(fit.clone(), false));
    (sketch.add_curve(curve, false).unwrap(), fit)
}

/// A spline through (0, 0), (10, 4) and (20, 0) with a handle at its
/// middle fit point, its tip at (13, 7): the spline, its fit points and
/// the tip.
pub(crate) fn handled_spline(sketch: &mut Sketch) -> (Id, Vec<Id>, Id) {
    let (spline, fit) = spline(sketch, &[(0.0, 0.0), (10.0, 4.0), (20.0, 0.0)]);
    let tip = point(sketch, 13.0, 7.0);
    if let Some(Curve::Spline(shape)) = sketch.curve_mut(spline).map(|entry| &mut entry.curve) {
        shape.handles.push(Handle { at: fit[1], tip });
    }
    (spline, fit, tip)
}

/// A plate from (-`x`, -`y`) to (`x`, `y`) with a hole of `radius` at the
/// origin: four lines and a circle.
pub(crate) fn plate(x: f64, y: f64, radius: f64) -> Sketch {
    let mut sketch = Sketch::default();
    let corners = [(-x, -y), (x, -y), (x, y), (-x, y)].map(|(x, y)| point(&mut sketch, x, y));
    for (i, &start) in corners.iter().enumerate() {
        line(&mut sketch, start, corners[(i + 1) % 4]);
    }
    let center = point(&mut sketch, 0.0, 0.0);
    sketch
        .add_curve(Curve::Circle { center, radius }, false)
        .unwrap();
    sketch
}

/// Adds a dimension of `measure`, `text` read in millimetres, on the side
/// the geometry is on, its label `label` from its anchor.
pub(crate) fn dimension(
    sketch: &mut Sketch,
    measure: Measure,
    text: &str,
    driving: bool,
    label: DVec2,
) -> Id {
    let value = Value::new(text, &measure.ask(&DESIGN)).unwrap();
    let side = sketch.side(&measure);
    let dimension = Dimension {
        measure,
        value,
        driving,
        label,
        side,
    };
    sketch.add_dimension(dimension).unwrap()
}

/// `tool` in use with `placed` placed, snapped to `targets`, drawing
/// normal geometry with nothing typed.
pub(crate) fn tool<'a>(
    tool: Tool,
    placed: &'a [DVec2],
    targets: &'a [Option<Target>],
) -> ActiveTool<'a> {
    ActiveTool {
        tool,
        placed,
        targets,
        construction: false,
        picked: &[],
        about: false,
        switched: false,
        typed: &[],
        sides: DEFAULT_SIDES,
        centered: false,
        control: false,
    }
}

/// `text` typed in `field`, read in millimetres.
pub(crate) fn typed(field: Field, text: &str) -> (Field, Value) {
    (field, Value::new(text, &field.ask(&DESIGN)).unwrap())
}
