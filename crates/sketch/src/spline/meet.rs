//! Where a spline ([`Path`]) meets lines, circles, arcs and other
//! splines, or itself: found by subdividing to the tolerance with
//! bounded work.

use glam::DVec2;

use super::bezier::{Bezier, MAX_DEPTH, Path, first, flatness, halves, hull_box, point};
use crate::bounds::{self, overlap};
use crate::intersect::Geom;

/// The most steps finding where a spline meets another curve takes:
/// pairs of segments whose boxes overlap, and pieces subdivided. Well
/// past what two splines crossing at a few places take; two lying along
/// each other for a stretch would take ever more, and stop here with
/// what's found, which may miss places (profiles are then too complex,
/// see [`crossings`]).
pub(crate) const MAX_MEET_STEPS: usize = 50_000;

/// What [`crossings`] and [`self_crossings`] give for a step of
/// subdivision, in the work a caller counts, whose unit is about a box
/// compared to another: halving pieces and their boxes, the flatness, and
/// a chord crossing or Newton's polish now and then. Segments' boxes
/// compared count one each, without taking a step.
pub(crate) const STEP_COST: usize = 16;

/// How many times a root of Bernstein coefficients is halved to.
const BISECTIONS: usize = 64;

/// The work of halving to a root, in the unit of [`STEP_COST`]: a
/// polynomial of degree three or six evaluated [`BISECTIONS`] times, each
/// about a step. A step of halving the coefficients themselves
/// ([`bernstein_roots`]) is about one of subdivision, and counted as one.
const BISECT_COST: usize = STEP_COST * BISECTIONS;

/// Work finding where curves meet: the steps left, see
/// [`MAX_MEET_STEPS`], and the work besides, which takes none: boxes
/// compared, roots halved to.
struct Steps {
    left: usize,
    besides: usize,
}

impl Steps {
    fn new() -> Steps {
        Steps {
            left: MAX_MEET_STEPS,
            besides: 0,
        }
    }

    /// Takes a step, if one is left.
    fn take(&mut self) -> bool {
        self.left = self.left.saturating_sub(1);
        self.left > 0
    }

    /// Counts two boxes compared.
    fn compare(&mut self) {
        self.add(1);
    }

    /// Counts `work` besides the steps, in the unit of [`STEP_COST`].
    fn add(&mut self, work: usize) {
        self.besides = self.besides.saturating_add(work);
    }

    /// The work done, in the unit of [`STEP_COST`]; `usize::MAX` if the
    /// steps ran out, so places may be missing, which no count of work
    /// can afford: profiles missing a crossing would join pieces that
    /// don't meet.
    fn work(&self) -> usize {
        if self.left == 0 {
            return usize::MAX;
        }
        (MAX_MEET_STEPS - self.left)
            .saturating_mul(STEP_COST)
            .saturating_add(self.besides)
    }
}

/// Where `a` and `b`, at least one of them a spline, cross or touch
/// (within `tolerance`), as pairs of their parameters, pushed onto `out`:
/// the work done, see [`Steps::work`], of at most about
/// [`MAX_MEET_STEPS`] steps, or `usize::MAX` where those ran out. What
/// [`meet`](crate::intersect::meet) does for splines, but for the ends,
/// which it finds alike for every curve.
pub(crate) fn crossings(a: &Geom, b: &Geom, tolerance: f64, out: &mut Vec<(f64, f64)>) -> usize {
    let mut steps = Steps::new();
    match (a, b) {
        (Geom::Spline(a), Geom::Spline(b)) => {
            paths(a, b, tolerance, &mut steps, &mut |ua, ub| {
                out.push((ua, ub))
            });
        }
        (Geom::Spline(path), other) => {
            on_other(path, other, tolerance, &mut steps, &mut |u, v| {
                out.push((u, v))
            });
        }
        (other, Geom::Spline(path)) => {
            on_other(path, other, tolerance, &mut steps, &mut |u, v| {
                out.push((v, u))
            });
        }
        _ => {}
    }
    steps.work()
}

/// Where `path` crosses itself, as pairs of its parameters, the lower
/// first, pushed onto `out`: the work done, see [`crossings`]. Its
/// segments are halved, so that one looping back across itself is found
/// too, and each half met with those after it, but where two halves
/// join.
pub(crate) fn self_crossings(path: &Path, tolerance: f64, out: &mut Vec<(f64, f64)>) -> usize {
    let mut steps = Steps::new();
    let mut halves_of: Vec<(Bezier, f64, f64)> = Vec::with_capacity(2 * path.segments().len());
    for (b, u0, u1) in path.pieces() {
        let (first, second) = halves(b);
        let middle = (u0 + u1) / 2.0;
        halves_of.push((first, u0, middle));
        halves_of.push((second, middle, u1));
    }
    let count = halves_of.len();
    let boxes: Vec<_> = halves_of.iter().map(|(b, ..)| hull_box(b)).collect();
    for i in 0..count {
        for j in i + 1..count {
            steps.compare();
            if !overlap(boxes[i], boxes[j], tolerance) {
                continue;
            }
            if !steps.take() {
                return steps.work();
            }
            let (a, a0, a1) = halves_of[i];
            let (b, b0, b1) = halves_of[j];
            // Where two halves join, they meet, and that's no crossing.
            let joint = if j == i + 1 {
                Some(a1)
            } else if path.closed() && i == 0 && j == count - 1 {
                Some(a0)
            } else {
                None
            };
            bezier_pair(&a, &b, tolerance, &mut steps, &mut |s, t| {
                let (u, v) = (a0 + (a1 - a0) * s, b0 + (b1 - b0) * t);
                let at_joint = joint.is_some_and(|joint| {
                    let place = path.at(joint);
                    path.at(u).distance(place) <= tolerance
                        && path.at(v).distance(place) <= tolerance
                });
                if !at_joint {
                    out.push((u.min(v), u.max(v)));
                }
            });
        }
    }
    steps.work()
}

/// Where the splines `a` and `b` meet, see [`crossings`]: each pair of
/// segments whose boxes meet, subdivided.
fn paths(a: &Path, b: &Path, tolerance: f64, steps: &mut Steps, found: &mut dyn FnMut(f64, f64)) {
    let boxes: Vec<_> = b.segments().iter().map(hull_box).collect();
    for (sa, pa) in a.segments().iter().enumerate() {
        let own = hull_box(pa);
        for (sb, pb) in b.segments().iter().enumerate() {
            steps.compare();
            if !overlap(own, boxes[sb], tolerance) {
                continue;
            }
            if !steps.take() {
                return;
            }
            bezier_pair(pa, pb, tolerance, steps, &mut |s, t| {
                found(a.param(sa, s), b.param(sb, t));
            });
        }
    }
}

/// Where the Bézier segments `a` and `b` meet, as places along each from
/// 0 to 1: pieces of each halved, the larger first, while their boxes
/// (grown by `tolerance`) overlap, until both are within `tolerance` of
/// their chords, whose crossing, or nearest places if they're within
/// `tolerance` of each other without crossing (touching), is then
/// polished by Newton's method on the segments themselves.
fn bezier_pair(
    a: &Bezier,
    b: &Bezier,
    tolerance: f64,
    steps: &mut Steps,
    found: &mut dyn FnMut(f64, f64),
) {
    // Pieces of each, with where along its segment each starts and ends.
    let mut stack = vec![(*a, 0.0, 1.0, *b, 0.0, 1.0, 0u32)];
    while let Some((pa, a0, a1, pb, b0, b1, depth)) = stack.pop() {
        if !steps.take() {
            return;
        }
        let (abox, bbox) = (hull_box(&pa), hull_box(&pb));
        if !overlap(abox, bbox, tolerance) {
            continue;
        }
        let (fa, fb) = (flatness(&pa), flatness(&pb));
        if (fa <= tolerance && fb <= tolerance) || depth >= MAX_DEPTH {
            if let Some((s, t)) = chords(&pa, &pb, tolerance) {
                let (s, t) = polish(a, b, a0 + (a1 - a0) * s, b0 + (b1 - b0) * t, tolerance);
                found(s, t);
            }
            continue;
        }
        // The larger, flat or not: a straight piece is flat at once, but
        // halving only the other would keep every piece of it in the
        // straight one's box, however far from it.
        let (asize, bsize) = (bounds::size(abox), bounds::size(bbox));
        if asize >= bsize {
            let (first, second) = halves(&pa);
            let middle = (a0 + a1) / 2.0;
            stack.push((second, middle, a1, pb, b0, b1, depth + 1));
            stack.push((first, a0, middle, pb, b0, b1, depth + 1));
        } else {
            let (first, second) = halves(&pb);
            let middle = (b0 + b1) / 2.0;
            stack.push((pa, a0, a1, second, middle, b1, depth + 1));
            stack.push((pa, a0, a1, first, b0, middle, depth + 1));
        }
    }
}

/// Where the chords of `a` and `b` cross, as places along each from 0 to
/// 1, or, not crossing, the nearest places of the two if they're within
/// `tolerance` of each other.
fn chords(a: &Bezier, b: &Bezier, tolerance: f64) -> Option<(f64, f64)> {
    let (p, r) = (a[0], a[3] - a[0]);
    let (q, s) = (b[0], b[3] - b[0]);
    if let Some((t, u)) = crate::crossing(p, r, q, s)
        && (0.0..=1.0).contains(&t)
        && (0.0..=1.0).contains(&u)
    {
        return Some((t, u));
    }
    // The nearest of an end of either and its foot on the other.
    let along = |from: DVec2, way: DVec2, at: DVec2| {
        let length = way.length_squared();
        if length > 0.0 {
            (way.dot(at - from) / length).clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    let candidates = [
        (0.0, along(q, s, p)),
        (1.0, along(q, s, p + r)),
        (along(p, r, q), 0.0),
        (along(p, r, q + s), 1.0),
    ];
    candidates
        .into_iter()
        .map(|(t, u)| ((p + r * t).distance(q + s * u), (t, u)))
        .filter(|&(distance, _)| distance <= tolerance)
        .min_by(|x, y| x.0.total_cmp(&y.0))
        .map(|(_, place)| place)
}

/// The places `s` on `a` and `t` on `b` made closer by Newton's method on
/// `a(s) = b(t)`, if it converges within the segments and closer than
/// they were, else as they were: a touch, where it doesn't converge,
/// stays where subdividing found it.
fn polish(a: &Bezier, b: &Bezier, s: f64, t: f64, tolerance: f64) -> (f64, f64) {
    let gap = |s: f64, t: f64| point(a, s) - point(b, t);
    let (mut ns, mut nt) = (s, t);
    for _ in 0..8 {
        let g = gap(ns, nt);
        if g.length() <= tolerance * 1e-6 {
            break;
        }
        let (da, db) = (first(a, ns), -first(b, nt));
        let det = da.perp_dot(db);
        if det.abs() <= 1e-12 * da.length() * db.length() {
            return (s, t);
        }
        // Solve [da db] (δs, δt) = -g.
        let ds = -g.perp_dot(db) / det;
        let dt = -da.perp_dot(g) / det;
        ns += ds;
        nt += dt;
        if !(0.0..=1.0).contains(&ns) || !(0.0..=1.0).contains(&nt) {
            return (s, t);
        }
    }
    if gap(ns, nt).length() <= gap(s, t).length() {
        (ns, nt)
    } else {
        (s, t)
    }
}

/// Where `path` meets a line, a circle or an arc, see [`crossings`]: on
/// each segment, where the other's implicit function (the signed
/// distance from a line, or the distance squared from a circle's centre
/// less its radius squared) is zero or, at a turn, within the tolerance
/// of it (touching), as a polynomial in Bernstein form subdivided while
/// it may change sign, then halved to where it does. Where on the other
/// curve is its parameter of the place, if within its ends.
fn on_other(
    path: &Path,
    other: &Geom,
    tolerance: f64,
    steps: &mut Steps,
    found: &mut dyn FnMut(f64, f64),
) {
    let bounds = other.bounds();
    for (s, b) in path.segments().iter().enumerate() {
        steps.compare();
        if !overlap(hull_box(b), bounds, tolerance) {
            continue;
        }
        if !steps.take() {
            return;
        }
        let (coefficients, slack) = match *other {
            Geom::Segment { start, end } => {
                let Some(normal) = (end - start).perp().try_normalize() else {
                    continue;
                };
                let c = b.map(|p| normal.dot(p - start));
                (Coefficients::new(&c), tolerance)
            }
            Geom::Round { center, radius, .. } => (
                distance_squared(b, center, radius),
                2.0 * radius * tolerance + tolerance * tolerance,
            ),
            Geom::Spline(_) => continue,
        };
        let mut roots = Vec::new();
        bernstein_roots(&coefficients, slack, steps, &mut roots);
        for t in roots {
            let at = point(b, t);
            if let Some(v) = other.param(at, tolerance) {
                found(path.param(s, t), v);
            }
        }
    }
}

/// The Bernstein coefficients, of degree six, of `|b(t) - center|² -
/// radius²`.
fn distance_squared(b: &Bezier, center: DVec2, radius: f64) -> Coefficients {
    const CHOOSE_3: [f64; 4] = [1.0, 3.0, 3.0, 1.0];
    const CHOOSE_6: [f64; 7] = [1.0, 6.0, 15.0, 20.0, 15.0, 6.0, 1.0];
    let d = b.map(|p| p - center);
    let mut coefficients = [0.0; 7];
    for i in 0..4 {
        for j in 0..4 {
            coefficients[i + j] += CHOOSE_3[i] * CHOOSE_3[j] * d[i].dot(d[j]);
        }
    }
    for (k, c) in coefficients.iter_mut().enumerate() {
        *c = *c / CHOOSE_6[k] - radius * radius;
    }
    Coefficients::new(&coefficients)
}

/// The most Bernstein coefficients root finding takes: of degree six, a
/// cubic's distance squared from a circle's centre.
const MAX_COEFFICIENTS: usize = 7;

/// Bernstein coefficients, at most [`MAX_COEFFICIENTS`], kept in place
/// rather than allocated, as root finding copies and halves them often.
#[derive(Clone, Copy)]
struct Coefficients {
    values: [f64; MAX_COEFFICIENTS],
    len: usize,
}

impl Coefficients {
    /// `values`, of which there are at most [`MAX_COEFFICIENTS`].
    fn new(values: &[f64]) -> Coefficients {
        let mut c = Coefficients {
            values: [0.0; MAX_COEFFICIENTS],
            len: values.len(),
        };
        c.values[..values.len()].copy_from_slice(values);
        c
    }
}

impl std::ops::Deref for Coefficients {
    type Target = [f64];

    fn deref(&self) -> &[f64] {
        &self.values[..self.len]
    }
}

impl std::ops::DerefMut for Coefficients {
    fn deref_mut(&mut self) -> &mut [f64] {
        &mut self.values[..self.len]
    }
}

/// The value at `t` of the polynomial with Bernstein coefficients
/// `coefficients`, by de Casteljau's algorithm.
fn bernstein(coefficients: &[f64], t: f64) -> f64 {
    let mut values = Coefficients::new(coefficients);
    for level in 1..values.len() {
        for i in 0..values.len() - level {
            values[i] = values[i] * (1.0 - t) + values[i + 1] * t;
        }
    }
    values[0]
}

/// The roots within `[0, 1]` of the polynomial with Bernstein
/// coefficients `coefficients`, and where it turns within `slack` of zero
/// without crossing it (touching), onto `roots`. Parts of `[0, 1]` whose
/// coefficients are all beyond `slack` either side hold none; parts where
/// they only rise or only fall hold at most one, found by halving; the
/// rest are halved, at most [`MAX_DEPTH`] times.
fn bernstein_roots(coefficients: &[f64], slack: f64, steps: &mut Steps, roots: &mut Vec<f64>) {
    let mut stack = vec![(Coefficients::new(coefficients), 0.0, 1.0, 0u32)];
    while let Some((c, t0, t1, depth)) = stack.pop() {
        if !steps.take() {
            return;
        }
        if c.iter().all(|&v| v > slack) || c.iter().all(|&v| v < -slack) {
            continue;
        }
        // Lying along the other curve all the way: where that starts and
        // stops, rather than every place halving would find.
        if c.iter().all(|&v| v.abs() <= slack) {
            roots.extend([t0, t1]);
            continue;
        }
        let rising = c.windows(2).all(|pair| pair[1] >= pair[0]);
        let falling = c.windows(2).all(|pair| pair[1] <= pair[0]);
        let (first, last) = (c[0], c[c.len() - 1]);
        if rising || falling {
            if first == 0.0 {
                roots.push(t0);
            } else if first.signum() != last.signum() {
                // Halved to where it changes sign, on the part's own
                // coefficients.
                steps.add(BISECT_COST);
                let (mut low, mut high) = (0.0, 1.0);
                for _ in 0..BISECTIONS {
                    let middle = (low + high) / 2.0;
                    if bernstein(&c, middle).signum() == first.signum() {
                        low = middle;
                    } else {
                        high = middle;
                    }
                }
                roots.push(t0 + (t1 - t0) * (low + high) / 2.0);
            } else if first.abs().min(last.abs()) <= slack {
                // Nearest zero at an end of the part: where it turns.
                roots.push(if first.abs() <= last.abs() { t0 } else { t1 });
            }
            continue;
        }
        if depth >= MAX_DEPTH {
            let middle = (t0 + t1) / 2.0;
            if bernstein(&c, 0.5).abs() <= slack {
                roots.push(middle);
            }
            continue;
        }
        let (left, right) = split_bernstein(&c);
        let middle = (t0 + t1) / 2.0;
        stack.push((right, middle, t1, depth + 1));
        stack.push((left, t0, middle, depth + 1));
    }
}

/// The Bernstein coefficients of a polynomial's halves, by de Casteljau's
/// algorithm at the middle.
fn split_bernstein(c: &Coefficients) -> (Coefficients, Coefficients) {
    let n = c.len();
    let (mut values, mut left, mut right) = (*c, *c, *c);
    for level in 1..n {
        for i in 0..n - level {
            values[i] = (values[i] + values[i + 1]) / 2.0;
        }
        left[level] = values[0];
        right[n - 1 - level] = values[n - 1 - level];
    }
    (left, right)
}
