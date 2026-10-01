//! A spline as cubic Bézier segments ([`Path`]): what drawing, snapping,
//! profiles and the shape tools ask of it, and where it meets lines,
//! circles, arcs and other splines, found by subdividing to the
//! tolerance with bounded work.

use glam::DVec2;

use super::basis::curvature;
use crate::CIRCLE_SEGMENTS;
use crate::angle;
use crate::intersect::{CHORD_COST, Geom};

/// How far a spline's polyline may stray from it, as a share of its size
/// (the larger side of its control points' box): about what a circle's
/// [`CIRCLE_SEGMENTS`] stray, relative to its diameter.
const FLATNESS: f64 = 5e-4;

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

/// What [`Path::closest_counting`] adds for a segment it searches for
/// the place nearest a point, in the unit of [`STEP_COST`]: the segment
/// sampled, and Newton's method from the nearest samples.
pub(crate) const CLOSEST_COST: usize = 192;

/// How many times a root of Bernstein coefficients is halved to.
const BISECTIONS: usize = 64;

/// The work of halving to a root, in the unit of [`STEP_COST`]: a
/// polynomial of degree three or six evaluated [`BISECTIONS`] times, each
/// about a step. A step of halving the coefficients themselves
/// ([`bernstein_roots`]) is about one of subdivision, and counted as one.
const BISECT_COST: usize = STEP_COST * BISECTIONS;

/// How many times a piece is halved looking for where it meets another:
/// a billionth of the sketch's size needs about fifteen.
const MAX_DEPTH: u32 = 48;

/// Nodes and weights of three- and five-point Gauss-Legendre quadrature
/// on `[0, 1]`: exact for polynomials up to degree five and nine.
const GAUSS_3: [(f64, f64); 3] = [
    (0.112_701_665_379_258_3, 0.277_777_777_777_777_8),
    (0.5, 0.444_444_444_444_444_4),
    (0.887_298_334_620_741_7, 0.277_777_777_777_777_8),
];
const GAUSS_5: [(f64, f64); 5] = [
    (0.046_910_077_030_668, 0.118_463_442_528_094_5),
    (0.230_765_344_947_158_4, 0.239_314_335_249_683_2),
    (0.5, 0.284_444_444_444_444_4),
    (0.769_234_655_052_841_6, 0.239_314_335_249_683_2),
    (0.953_089_922_969_332, 0.118_463_442_528_094_5),
];

/// A cubic Bézier segment by its four control points.
pub(crate) type Bezier = [DVec2; 4];

/// The place on `b` at `t`.
pub(crate) fn point(b: &Bezier, t: f64) -> DVec2 {
    let s = 1.0 - t;
    b[0] * (s * s * s) + b[1] * (3.0 * s * s * t) + b[2] * (3.0 * s * t * t) + b[3] * (t * t * t)
}

/// The first derivative of `b` at `t`.
fn first(b: &Bezier, t: f64) -> DVec2 {
    let s = 1.0 - t;
    ((b[1] - b[0]) * (s * s) + (b[2] - b[1]) * (2.0 * s * t) + (b[3] - b[2]) * (t * t)) * 3.0
}

/// The second derivative of `b` at `t`.
fn second(b: &Bezier, t: f64) -> DVec2 {
    ((b[2] - b[1] * 2.0 + b[0]) * (1.0 - t) + (b[3] - b[2] * 2.0 + b[1]) * t) * 6.0
}

/// `b` from `t0` to `t1`, either way, as a Bézier segment of its own.
fn part(b: &Bezier, t0: f64, t1: f64) -> Bezier {
    // The ends and the derivatives there, scaled to the part.
    let h = t1 - t0;
    let (p, q) = (point(b, t0), point(b, t1));
    [p, p + first(b, t0) * h / 3.0, q - first(b, t1) * h / 3.0, q]
}

/// `b` cut in two at its middle.
fn halves(b: &Bezier) -> (Bezier, Bezier) {
    let ab = b[0].midpoint(b[1]);
    let bc = b[1].midpoint(b[2]);
    let cd = b[2].midpoint(b[3]);
    let abc = ab.midpoint(bc);
    let bcd = bc.midpoint(cd);
    let middle = abc.midpoint(bcd);
    ([b[0], ab, abc, middle], [middle, bcd, cd, b[3]])
}

/// The box `b`'s control points lie in, which it lies in.
fn hull_box(b: &Bezier) -> (DVec2, DVec2) {
    let min = b[0].min(b[1]).min(b[2]).min(b[3]);
    let max = b[0].max(b[1]).max(b[2]).max(b[3]);
    (min, max)
}

/// Whether the boxes `a` and `b`, each grown by `tolerance`, overlap.
fn overlap(a: (DVec2, DVec2), b: (DVec2, DVec2), tolerance: f64) -> bool {
    !((a.0 - tolerance).cmpgt(b.1).any() || (b.0 - tolerance).cmpgt(a.1).any())
}

/// How far `b`'s inner control points are from the line through its
/// ends, or from its start where its ends are one place: how far it
/// strays from its chord at most.
fn flatness(b: &Bezier) -> f64 {
    let chord = b[3] - b[0];
    let length = chord.length();
    let off = |p: DVec2| {
        if length > 0.0 {
            chord.perp_dot(p - b[0]).abs() / length
        } else {
            p.distance(b[0])
        }
    };
    off(b[1]).max(off(b[2]))
}

/// How many straight segments `b` is drawn with to stray at most
/// `tolerance` from it (Wang's formula), from one to
/// [`CIRCLE_SEGMENTS`].
fn segments_for(b: &Bezier, tolerance: f64) -> usize {
    let bend = (b[0] - b[1] * 2.0 + b[2])
        .length()
        .max((b[1] - b[2] * 2.0 + b[3]).length());
    let n = (0.75 * bend / tolerance).sqrt().ceil();
    // NaN, from a tolerance of zero, `as` makes 0.
    (n as usize).clamp(1, CIRCLE_SEGMENTS)
}

/// A spline as cubic Bézier segments, its parameter `u` running from 0
/// to 1: segment `s` from `breaks[s]` to `breaks[s + 1]`, and a closed
/// one's last ending where its first starts.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Path {
    breaks: Vec<f64>,
    segments: Vec<Bezier>,
    closed: bool,
    /// How far its polyline strays from it at most: [`FLATNESS`] of its
    /// size.
    flat: f64,
}

impl Path {
    /// The path of `segments` between `breaks`, one more of them,
    /// increasing from 0 to 1.
    pub(crate) fn new(breaks: Vec<f64>, segments: Vec<Bezier>, closed: bool) -> Path {
        let mut path = Path {
            breaks,
            segments,
            closed,
            flat: 0.0,
        };
        let (min, max) = path.bounds();
        path.flat = (max - min).max_element() * FLATNESS;
        path
    }

    pub(crate) fn closed(&self) -> bool {
        self.closed
    }

    /// How many cubic segments it has.
    pub(crate) fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Whether it's finite and has a size: not all at one place, but for
    /// rounding.
    pub(crate) fn is_sound(&self) -> bool {
        let (min, max) = self.bounds();
        let scale = min.abs().max(max.abs()).max_element();
        let size = (max - min).max_element();
        min.is_finite() && max.is_finite() && size > 16.0 * f64::EPSILON * scale
    }

    /// Where its parameter ends: 1.
    pub(crate) fn last(&self) -> f64 {
        self.breaks[self.breaks.len() - 1]
    }

    /// The segment `u` is on, and where along it, from 0 to 1: `u`
    /// clamped to the ends, or for a closed spline taken round.
    fn locate(&self, u: f64) -> (usize, f64) {
        let last = self.last();
        let u = if self.closed && !(0.0..=last).contains(&u) {
            u.rem_euclid(last)
        } else {
            u.clamp(0.0, last)
        };
        let s = self.breaks[1..]
            .partition_point(|&b| b <= u)
            .min(self.segments.len() - 1);
        let (a, b) = (self.breaks[s], self.breaks[s + 1]);
        (s, ((u - a) / (b - a)).clamp(0.0, 1.0))
    }

    /// The parameter of segment `s` at `t` along it.
    fn param(&self, s: usize, t: f64) -> f64 {
        let (a, b) = (self.breaks[s], self.breaks[s + 1]);
        a + (b - a) * t
    }

    /// The segments, each with where its parameter starts and ends.
    fn pieces(&self) -> impl Iterator<Item = (&Bezier, f64, f64)> {
        let spans = self.breaks.windows(2).map(|pair| (pair[0], pair[1]));
        self.segments.iter().zip(spans).map(|(b, (a, c))| (b, a, c))
    }

    pub(crate) fn at(&self, u: f64) -> DVec2 {
        let (s, t) = self.locate(u);
        point(&self.segments[s], t)
    }

    /// The first and second derivatives by `u` at `u`.
    pub(crate) fn derivatives(&self, u: f64) -> (DVec2, DVec2) {
        let (s, t) = self.locate(u);
        let h = self.breaks[s + 1] - self.breaks[s];
        let b = &self.segments[s];
        (first(b, t) / h, second(b, t) / (h * h))
    }

    /// The unit direction at `u` going towards increasing `u` if
    /// `forward`, else back, and the curvature that way, positive turning
    /// left. Where it stops (a cusp), the way it leaves.
    pub(crate) fn heading(&self, u: f64, forward: bool) -> (DVec2, f64) {
        let sign = if forward { 1.0 } else { -1.0 };
        let (d1, d2) = self.derivatives(u);
        let direction = d1.try_normalize().or_else(|| {
            // Where it stops, it leaves along its second derivative, or
            // failing that towards a place a little along it.
            let ahead = self.at(u + sign * 1e-6 * self.last()) - self.at(u);
            (d2 * sign).try_normalize().or(ahead.try_normalize())
        });
        let Some(direction) = direction else {
            return (DVec2::X * sign, 0.0);
        };
        let curvature = curvature(d1, d2);
        let curvature = if curvature.is_finite() {
            curvature
        } else {
            0.0
        };
        (direction * sign, curvature * sign)
    }

    /// The box its control points lie in, which it lies in.
    pub(crate) fn bounds(&self) -> (DVec2, DVec2) {
        self.segments.iter().map(hull_box).fold(
            (DVec2::INFINITY, DVec2::NEG_INFINITY),
            |(min, max), (a, b)| (min.min(a), max.max(b)),
        )
    }

    /// The parts of segments from `u0` to `u1`, in the order travelled:
    /// the segment and where along it each starts and ends, backwards
    /// when `u1` is below `u0`. A closed spline goes round past its last
    /// parameter, and on from its first.
    fn span(&self, u0: f64, u1: f64) -> Vec<(usize, f64, f64)> {
        let (low, high) = (u0.min(u1), u0.max(u1));
        let last = self.last();
        let mut parts = Vec::new();
        if self.closed && high > last {
            self.range(low.min(last), last, &mut parts);
            self.range(0.0, (high - last).min(last), &mut parts);
        } else {
            self.range(low, high, &mut parts);
        }
        if u1 < u0 {
            parts.reverse();
            for part in &mut parts {
                std::mem::swap(&mut part.1, &mut part.2);
            }
        }
        parts
    }

    /// The parts of segments from `low` to `high`, within `[0, 1]`, onto
    /// `parts`: none empty, but for a span of nothing, one.
    fn range(&self, low: f64, high: f64, parts: &mut Vec<(usize, f64, f64)>) {
        let (first, t0) = self.locate(low);
        let (end, t1) = self.locate(high);
        if first == end || (end == first + 1 && t1 == 0.0) {
            let t1 = if end == first { t1 } else { 1.0 };
            parts.push((first, t0, t1));
            return;
        }
        if t0 < 1.0 {
            parts.push((first, t0, 1.0));
        }
        for s in first + 1..end {
            parts.push((s, 0.0, 1.0));
        }
        if t1 > 0.0 {
            parts.push((end, 0.0, t1));
        }
    }

    /// The curve from `u0` to `u1` (either way) as a polyline through
    /// both ends, each segment's part in as many straight pieces as it
    /// takes to stray at most [`FLATNESS`] of its size, and at most
    /// [`CIRCLE_SEGMENTS`].
    pub(crate) fn polyline(&self, u0: f64, u1: f64) -> Vec<DVec2> {
        let mut polyline = vec![self.at(u0)];
        for (s, t0, t1) in self.span(u0, u1) {
            let b = part(&self.segments[s], t0, t1);
            let n = segments_for(&b, self.flat);
            polyline.extend((1..=n).map(|i| point(&b, i as f64 / n as f64)));
        }
        polyline
    }

    /// Its whole length as a polyline, closed if it is, as
    /// [`Sketch::flatten`](crate::Sketch::flatten) draws it.
    pub(crate) fn flatten(&self) -> Vec<DVec2> {
        let mut polyline = self.polyline(0.0, self.last());
        if self.closed
            && let Some(&first) = polyline.first()
            && let Some(last) = polyline.last_mut()
        {
            *last = first;
        }
        polyline
    }

    /// The box the curve from `u0` to `u1` (either way) lies in, exactly:
    /// its parts' ends and where they turn along x or y.
    pub(crate) fn span_bounds(&self, u0: f64, u1: f64) -> (DVec2, DVec2) {
        let (a, b) = (self.at(u0), self.at(u1));
        let (mut min, mut max) = (a.min(b), a.max(b));
        for (s, t0, t1) in self.span(u0, u1) {
            let b = &self.segments[s];
            let (low, high) = (t0.min(t1), t0.max(t1));
            for t in [low, high].into_iter().chain(turns(b)) {
                if (low..=high).contains(&t) {
                    let at = point(b, t);
                    (min, max) = (min.min(at), max.max(at));
                }
            }
        }
        (min, max)
    }

    /// The length from `u0` to `u1`, either way, by five-point
    /// quadrature on each segment's part.
    pub(crate) fn length(&self, u0: f64, u1: f64) -> f64 {
        self.span(u0, u1)
            .into_iter()
            .map(|(s, t0, t1)| {
                let b = &self.segments[s];
                let h = t1 - t0;
                let sum: f64 = GAUSS_5
                    .iter()
                    .map(|&(x, w)| w * first(b, t0 + h * x).length())
                    .sum();
                (sum * h).abs()
            })
            .sum()
    }

    /// The area between the curve from `u0` to `u1` (either way) and the
    /// chord joining its ends, positive where it bulges left of travel:
    /// half the integral of `(p - start) × p'`, exact by three-point
    /// quadrature on each segment's part, the integrand of degree five.
    pub(crate) fn bulge(&self, u0: f64, u1: f64) -> f64 {
        let start = self.at(u0);
        let total: f64 = self
            .span(u0, u1)
            .into_iter()
            .map(|(s, t0, t1)| {
                let b = &self.segments[s];
                let h = t1 - t0;
                let sum: f64 = GAUSS_3
                    .iter()
                    .map(|&(x, w)| {
                        let t = t0 + h * x;
                        w * (point(b, t) - start).perp_dot(first(b, t))
                    })
                    .sum();
                sum * h
            })
            .sum();
        total / 2.0
    }

    /// How far round `point` the curve from `u0` to `u1` (either way)
    /// winds, in radians, counter-clockwise positive: each part's chord's
    /// angle, once `point` is outside the box of the part's control
    /// points, and so outside the loop of the part and its chord back.
    /// Adds the work done to `work`, [`CHORD_COST`] a part.
    pub(crate) fn winding(&self, u0: f64, u1: f64, at: DVec2, work: &mut usize) -> f64 {
        self.span(u0, u1)
            .into_iter()
            .map(|(s, t0, t1)| wind(&part(&self.segments[s], t0, t1), at, MAX_DEPTH, work))
            .sum()
    }

    /// The parameter of the place nearest `at`: on each segment whose box
    /// could hold a nearer place than found so far, from the nearest of a
    /// few places along it, by Newton's method kept within where the
    /// nearest is bracketed, halving the bracket where a step leaves it.
    pub(crate) fn closest(&self, at: DVec2) -> f64 {
        self.closest_counting(at, &mut 0)
    }

    /// [`Path::closest`], adding the work done to `work`: one a segment's
    /// box, [`CLOSEST_COST`] a segment searched.
    pub(crate) fn closest_counting(&self, at: DVec2, work: &mut usize) -> f64 {
        let mut best = (f64::INFINITY, 0.0);
        for (s, b) in self.segments.iter().enumerate() {
            *work = work.saturating_add(1);
            let (min, max) = hull_box(b);
            let outside = (min - at).max(at - max).max(DVec2::ZERO);
            if outside.length_squared() > best.0 {
                continue;
            }
            *work = work.saturating_add(CLOSEST_COST);
            let t = closest_on(b, at);
            let distance = point(b, t).distance_squared(at);
            if distance < best.0 {
                best = (distance, self.param(s, t));
            }
        }
        best.1
    }

    /// The parameter of `at` if it's within `tolerance` of the curve.
    pub(crate) fn param_of(&self, at: DVec2, tolerance: f64) -> Option<f64> {
        let u = self.closest(at);
        (self.at(u).distance(at) <= tolerance).then_some(u)
    }

    /// Whether the piece from `from` to `to`, whose ends are one place,
    /// goes round rather than being a sliver of nothing: its middle is
    /// further than `tolerance` from its ends.
    pub(crate) fn goes_round(&self, from: f64, to: f64, tolerance: f64) -> bool {
        self.at(from).distance(self.at((from + to) / 2.0)) > tolerance
    }
}

/// Where along `b` it turns along x or y: the roots of each coordinate's
/// derivative within `(0, 1)`.
fn turns(b: &Bezier) -> impl Iterator<Item = f64> {
    let mut found = Vec::new();
    for axis in 0..2 {
        // The derivative's Bernstein coefficients, a quadratic's.
        let [p, q, r] = [b[1] - b[0], b[2] - b[1], b[3] - b[2]].map(|d| d[axis]);
        let (a, bb, c) = (p - 2.0 * q + r, 2.0 * (q - p), p);
        if a.abs() <= 1e-12 * (p.abs() + q.abs() + r.abs()) {
            if bb != 0.0 {
                found.push(-c / bb);
            }
            continue;
        }
        let discriminant = bb * bb - 4.0 * a * c;
        if discriminant >= 0.0 {
            let root = discriminant.sqrt();
            found.push((-bb - root) / (2.0 * a));
            found.push((-bb + root) / (2.0 * a));
        }
    }
    found.into_iter().filter(|t| *t > 0.0 && *t < 1.0)
}

/// How far `b` winds round `at` from its start to its end, see
/// [`Path::winding`]: halved while `at` is in its control points' box, at
/// most `depth` times, and the chord's angle once it isn't. Adds
/// [`CHORD_COST`] a part to `work`.
fn wind(b: &Bezier, at: DVec2, depth: u32, work: &mut usize) -> f64 {
    *work = work.saturating_add(CHORD_COST);
    let (min, max) = hull_box(b);
    let inside = at.cmpge(min).all() && at.cmple(max).all();
    if !inside || depth == 0 {
        let (p, q) = (b[0] - at, b[3] - at);
        return angle::atan2(p.perp_dot(q), p.dot(q));
    }
    let (first, second) = halves(b);
    wind(&first, at, depth - 1, work) + wind(&second, at, depth - 1, work)
}

/// The parameter of the place on `b` nearest `at`, see [`Path::closest`]:
/// from each sample nearer than those beside it, refined, as a long
/// segment may pass `at` twice, the sample nearest it being on the
/// other pass.
fn closest_on(b: &Bezier, at: DVec2) -> f64 {
    const SAMPLES: usize = 8;
    let distance = |t: f64| point(b, t).distance_squared(at);
    let sampled: Vec<f64> = (0..=SAMPLES)
        .map(|i| distance(i as f64 / SAMPLES as f64))
        .collect();
    let mut found = vec![0.0, 1.0];
    for i in 0..=SAMPLES {
        let before = i == 0 || sampled[i] <= sampled[i - 1];
        let after = i == SAMPLES || sampled[i] <= sampled[i + 1];
        if before && after {
            found.push(refine(b, at, i, SAMPLES));
        }
    }
    found
        .into_iter()
        .min_by(|&i, &j| distance(i).total_cmp(&distance(j)))
        .unwrap_or(0.0)
}

/// The parameter of the place on `b` nearest `at` about its sample
/// `sample` of `samples`: Newton's method on the distance's slope, kept
/// between the sample and the one beside it the slope leads towards,
/// where it changes sign there, halving where a step leaves them; else
/// the sample.
fn refine(b: &Bezier, at: DVec2, sample: usize, samples: usize) -> f64 {
    let place = |i: usize| i as f64 / samples as f64;
    let t = place(sample);
    // Half the distance squared's derivative: negative before the
    // nearest place, positive after.
    let slope = |t: f64| (point(b, t) - at).dot(first(b, t));
    let g = slope(t);
    let (mut low, mut high) = if g < 0.0 {
        (t, place((sample + 1).min(samples)))
    } else {
        (place(sample.saturating_sub(1)), t)
    };
    if !(slope(low) < 0.0 && slope(high) > 0.0) {
        return t;
    }
    let mut t = t;
    for _ in 0..60 {
        let g = slope(t);
        if g == 0.0 {
            break;
        }
        if g < 0.0 {
            low = t;
        } else {
            high = t;
        }
        let (d1, d2) = (first(b, t), second(b, t));
        let gradient = d1.length_squared() + (point(b, t) - at).dot(d2);
        let newton = t - g / gradient;
        let next = if gradient > 0.0 && newton > low && newton < high {
            newton
        } else {
            (low + high) / 2.0
        };
        if (next - t).abs() <= f64::EPSILON || high - low <= f64::EPSILON {
            return next;
        }
        t = next;
    }
    t
}

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
    let mut halves_of: Vec<(Bezier, f64, f64)> = Vec::with_capacity(2 * path.segments.len());
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
            } else if path.closed && i == 0 && j == count - 1 {
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
    let boxes: Vec<_> = b.segments.iter().map(hull_box).collect();
    for (sa, pa) in a.segments.iter().enumerate() {
        let own = hull_box(pa);
        for (sb, pb) in b.segments.iter().enumerate() {
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
        let ((amin, amax), (bmin, bmax)) = (hull_box(&pa), hull_box(&pb));
        if !overlap((amin, amax), (bmin, bmax), tolerance) {
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
        let (asize, bsize) = ((amax - amin).max_element(), (bmax - bmin).max_element());
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
    for (s, b) in path.segments.iter().enumerate() {
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
