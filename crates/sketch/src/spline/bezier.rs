//! A spline as cubic Bézier segments ([`Path`]): what drawing, snapping,
//! profiles and the shape tools ask of it. Where it meets other curves is
//! [`meet`](super::meet).

use glam::DVec2;

use super::basis::curvature;
use crate::CIRCLE_SEGMENTS;
use crate::angle;
use crate::bounds;
use crate::intersect::CHORD_COST;

/// How far a spline's polyline may stray from it, as a share of its size
/// (the larger side of its control points' box): about what a circle's
/// [`CIRCLE_SEGMENTS`] stray, relative to its diameter.
const FLATNESS: f64 = 5e-4;

/// What [`Path::closest_counting`] adds for a segment it searches for
/// the place nearest a point, in the unit of [`STEP_COST`](super::meet::STEP_COST): the segment
/// sampled, and Newton's method from the nearest samples.
pub(crate) const CLOSEST_COST: usize = 192;

/// How many times a piece is halved looking for where it meets another:
/// a billionth of the sketch's size needs about fifteen.
pub(super) const MAX_DEPTH: u32 = 48;

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
pub(super) fn first(b: &Bezier, t: f64) -> DVec2 {
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
pub(super) fn halves(b: &Bezier) -> (Bezier, Bezier) {
    let ab = b[0].midpoint(b[1]);
    let bc = b[1].midpoint(b[2]);
    let cd = b[2].midpoint(b[3]);
    let abc = ab.midpoint(bc);
    let bcd = bc.midpoint(cd);
    let middle = abc.midpoint(bcd);
    ([b[0], ab, abc, middle], [middle, bcd, cd, b[3]])
}

/// The box `b`'s control points lie in, which it lies in.
pub(super) fn hull_box(b: &Bezier) -> (DVec2, DVec2) {
    let min = b[0].min(b[1]).min(b[2]).min(b[3]);
    let max = b[0].max(b[1]).max(b[2]).max(b[3]);
    (min, max)
}

/// How far `b`'s inner control points are from the line through its
/// ends, or from its start where its ends are one place: how far it
/// strays from its chord at most.
pub(super) fn flatness(b: &Bezier) -> f64 {
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
        path.flat = bounds::size(path.bounds()) * FLATNESS;
        path
    }

    pub(crate) fn closed(&self) -> bool {
        self.closed
    }

    /// Its segments.
    pub(super) fn segments(&self) -> &[Bezier] {
        &self.segments
    }

    /// How many cubic segments it has.
    pub(crate) fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Whether it's finite and has a size: not all at one place, but for
    /// rounding.
    pub(crate) fn is_sound(&self) -> bool {
        let (min, max) = self.bounds();
        let (scale, size) = (bounds::reach((min, max)), bounds::size((min, max)));
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
    pub(super) fn param(&self, s: usize, t: f64) -> f64 {
        let (a, b) = (self.breaks[s], self.breaks[s + 1]);
        a + (b - a) * t
    }

    /// The segments, each with where its parameter starts and ends.
    pub(super) fn pieces(&self) -> impl Iterator<Item = (&Bezier, f64, f64)> {
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
        (self.segments.iter().map(hull_box))
            .fold((DVec2::INFINITY, DVec2::NEG_INFINITY), bounds::union)
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
            if bounds::outside(hull_box(b), at).length_squared() > best.0 {
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
