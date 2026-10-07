//! Building sketches in tests.

use glam::DVec2;

use varde_expr::{LengthUnit, Params, Value};

use crate::intersect::Geom;
use crate::{
    Accepted, Budget, Constraint, Curve, Design, Dimension, Id, Measure, Rejected, Sketch,
    SketchEdit, Spline, propose,
};

pub(crate) fn point(sketch: &mut Sketch, x: f64, y: f64) -> Id {
    sketch.add_point(DVec2::new(x, y)).unwrap()
}

/// A handle at the fit point `at` with its tip `tip`, its end a new
/// point mirroring the tip.
pub(crate) fn handle(sketch: &mut Sketch, at: Id, tip: Id) -> crate::Handle {
    let (from, to) = (sketch.point(at).unwrap().at, sketch.point(tip).unwrap().at);
    let end = sketch.add_point(2.0 * from - to).unwrap();
    crate::Handle { at, tip, end }
}

/// New points at `places`, as [`point`] adds them one by one, but
/// numbered from the highest found once: a sketch of thousands of points
/// would otherwise look through them all for each.
pub(crate) fn points(sketch: &mut Sketch, places: &[(f64, f64)]) -> Vec<Id> {
    let mut number = sketch.next_number(crate::Kind::Point.name());
    (places.iter())
        .map(|&(x, y)| {
            let id = sketch.new_id().unwrap();
            let at = DVec2::new(x, y);
            sketch.points.push(crate::Point { id, number, at });
            number = number.saturating_add(1);
            id
        })
        .collect()
}

pub(crate) fn line(sketch: &mut Sketch, start: Id, end: Id) -> Id {
    sketch.add_curve(Curve::Line { start, end }, false).unwrap()
}

pub(crate) fn circle(sketch: &mut Sketch, center: Id, radius: f64) -> Id {
    sketch
        .add_curve(Curve::Circle { center, radius }, false)
        .unwrap()
}

pub(crate) fn arc(sketch: &mut Sketch, center: Id, start: Id, end: Id) -> Id {
    sketch
        .add_curve(Curve::Arc { center, start, end }, false)
        .unwrap()
}

/// A spline through new points at `places`, open or `closed`: its id and
/// its fit points.
pub(crate) fn spline(sketch: &mut Sketch, places: &[(f64, f64)], closed: bool) -> (Id, Vec<Id>) {
    let fit = points(sketch, places);
    let spline = Spline::through(fit.clone(), closed);
    let id = sketch.add_curve(Curve::Spline(spline), false).unwrap();
    (id, fit)
}

/// The shape of the curve `id`, as intersecting sees it.
pub(crate) fn geom(sketch: &Sketch, id: Id) -> Geom {
    Geom::of(sketch, &sketch.curve(id).unwrap().curve).unwrap()
}

/// Adds `constraint`, which leaves the sketch passing its check.
pub(crate) fn constrain(sketch: &mut Sketch, constraint: Constraint) -> Id {
    let id = sketch.add_constraint(constraint).unwrap();
    assert_eq!(sketch.check(&DESIGN), Ok(()));
    id
}

pub(crate) fn at(sketch: &Sketch, id: Id) -> DVec2 {
    sketch.point(id).unwrap().at
}

/// Whether `a` and `b` are the same place, but for rounding.
pub(crate) fn near(a: DVec2, b: DVec2) -> bool {
    a.distance(b) < 1e-9
}

/// A quadrilateral drawn roughly as a rectangle: its corners from the
/// bottom left counter-clockwise, and its lines bottom, right, top, left.
pub(crate) fn quadrilateral() -> (Sketch, [Id; 4], [Id; 4]) {
    let mut sketch = Sketch::default();
    let corners =
        [(0.0, 0.0), (10.0, 0.5), (10.5, 6.0), (-0.5, 5.0)].map(|(x, y)| point(&mut sketch, x, y));
    let lines = [0, 1, 2, 3].map(|i| line(&mut sketch, corners[i], corners[(i + 1) % 4]));
    (sketch, corners, lines)
}

/// What tests check sketches against: a limit of a kilometre and
/// millimetres.
pub(crate) const DESIGN: Design<'static> = Design {
    max: 1e6,
    units: LengthUnit::Mm,
    params: Params::EMPTY,
};

/// `text` read in millimetres as a value of `measure`.
pub(crate) fn value(text: &str, measure: &Measure) -> Value {
    Value::new(text, &measure.ask(&DESIGN)).unwrap()
}

/// Adds a driving dimension of `measure`, `text` read in millimetres, on
/// the side the geometry is on, which leaves the sketch passing its check.
pub(crate) fn dimension(sketch: &mut Sketch, measure: Measure, text: &str) -> Id {
    let value = value(text, &measure);
    let side = sketch.side(&measure);
    let dimension = Dimension {
        measure,
        value,
        driving: true,
        label: DVec2::ZERO,
        side,
    };
    let id = sketch.add_dimension(dimension).unwrap();
    assert_eq!(sketch.check(&DESIGN), Ok(()));
    id
}

/// `edit` proposed on `sketch` against [`DESIGN`], with the default
/// budget.
pub(crate) fn propose_it(sketch: &Sketch, edit: &SketchEdit) -> Result<Accepted, Rejected> {
    propose(sketch, edit, &DESIGN, &Budget::default())
}
