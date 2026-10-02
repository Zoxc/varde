//! The minimum distance between two picks, and the two points where it is
//! reached: [`distance`].
//!
//! A pick is cut into elements, patches for a face or a body (a body's
//! distance is its surface's), curves for an edge and a point for a
//! corner, and the distance is a branch and bound on
//! [`near`](crate::boolean::near)'s depth-first search: over pairs of
//! nodes of a box [`Tree`] of each pick's elements, nearest first, then
//! over pairs of their elements' pieces. Its visit keeps the closest
//! points found so far (the best) and drops a pair that can't come nearer
//! than the best less the resolution.
//!
//! - **Lower bounds**: a pair's boxes' and control hulls' distance (GJK,
//!   [`apart`], and for flat pieces along their planes' normals); and
//!   the gap between the ranges, over the two pieces, of the distance
//!   from a round's axis or centre ([`Round`]: a cylindrical face's axis,
//!   a spherical one's centre, a circular edge's axis and centre), which
//!   moves no faster than the point. The ranges come from the Bernstein
//!   coefficients of the distance's square over each piece
//!   ([`Round::range`]), exact on pieces of the round: a tube's coaxial
//!   walls, or a pin's wall and the circular edge of the tube's end round
//!   it, are their radii's difference apart everywhere, which hulls only
//!   show once split to pieces about `√(8·resolution·r)` across
//!   (hundreds of thousands along a circle, millions over a wall).
//! - **Upper bounds**: the pieces' corners and middles, and the closest
//!   points by Newton's method ([`closest`]) on the pair, edges and
//!   corners included: a step leaving a piece stops on its boundary and
//!   the search goes on along that edge, then at that corner, and back
//!   into the piece where that brings the points nearer. Newton runs at
//!   the first visit of every pair of elements, at pairs of flat pieces at
//!   most every [`RETRY`] splits along one lineage, at up to
//!   [`BAND_TRIES`] pairs of elements that can come nearer than the best
//!   only by less than the resolution, and on the best pair at the end.
//! - **Stopping**: a pair is dropped when a lower bound clears the best
//!   less the resolution, or when both pieces are within half the
//!   resolution across (their corners are then within the resolution of
//!   any of their points); the search stops once the best is within the
//!   resolution. So the answer is a distance between two points of the
//!   picks, within the resolution of the least (up to the splits'
//!   rounding, as `near`'s), and in practice to rounding: Newton's method
//!   converges on minima inside pieces and on their edges and corners.
//!   Where the picks touch or cross, it is 0 to rounding (where curved
//!   faces touch along a line, to about `1e-13`: the squared distance
//!   grows as the fourth power off the line, so the points settle only to
//!   about the square root of the rounding).
//!
//! Sequential, so the same bits at any thread count. Every part is
//! charged to the budget: past it, [`MeasureError::TooComplex`]. Only
//! `+ − × ÷ √`.

use glam::DVec3;

use super::{MeasureError, Pick, Target};
use crate::boolean::near::{Step, Which, apart_along, corner_normal, search, settled};
use crate::budget::{Budget, Work};
use crate::mesh::{Form, MIN_SPLIT, apart, circle_of, straight};
use crate::patch::{Bounds3, Conic3, Patch};
use crate::{KernelError, Tolerance};

/// The work of one search for closest points by Newton's method: up to
/// [`CLOSEST_STEPS`] steps of ten patch evaluations and a few more for
/// the step's length, in units of about half a microsecond.
const CLOSEST_WORK: usize = 32;

/// Newton steps of one search for closest points, steps onto a boundary
/// and back included.
const CLOSEST_STEPS: usize = 32;

/// How often a Newton step that would move the points apart is halved
/// before the search gives up.
const HALVINGS: usize = 8;

/// How many splits a pair must be below the last pair of its lineage
/// Newton's method ran on for it to run again.
const RETRY: u32 = 2;

/// How many pairs of elements that can come nearer than the best by
/// less than the resolution are searched by Newton's method anyway.
const BAND_TRIES: usize = 64;

/// How much nearer than the best, relatively, a pair of elements must be
/// able to come for such a search: more than GJK's rounding, so ties
/// with the best (a box's faces round its nearest corner) don't count.
const BAND: f64 = 1e-9;

/// The step of the central differences for second derivatives, in a
/// piece's domain.
const H: f64 = 1e-5;

/// The work of an element: its box and its rounds.
const ELEMENT_WORK: usize = 1;

/// The work of the ranges of the distance from a pair's rounds over its
/// pieces, on top of the visit's.
const ROUND_WORK: usize = 1;

/// The minimum distance between two picks and where it is reached.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Distance {
    /// `|points[1] − points[0]|`.
    pub distance: f64,
    /// A point of the first pick and one of the second, as far apart as
    /// `distance`.
    pub points: [DVec3; 2],
}

/// The minimum distance between `a` and `b`, faces, edges, corners or
/// bodies of the same solid or of two (a body's is its surface's: a body
/// inside another is as far as its surface is from the other's), and the
/// two points where it is reached: within `tol`'s resolution of the least,
/// and to rounding where the closest points are inside faces, on edges or
/// at corners: a branch and bound over pairs of pieces of them, bounded
/// below by their control hulls and by the rounds (cylinders, spheres,
/// circles) they lie on, and above by Newton's method on their closest
/// points, edges and corners included. 0 to rounding where they
/// touch or cross. [`MeasureError::TooComplex`] past `budget`,
/// [`MeasureError::NotFound`] for a pick naming nothing and
/// [`MeasureError::Empty`] for the empty solid's body.
pub fn distance(
    a: &Target<'_>,
    b: &Target<'_>,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Distance, MeasureError> {
    distance_within(a, b, tol, &mut Work::new(budget))
}

/// [`distance`], charged to `work`.
fn distance_within(
    a: &Target<'_>,
    b: &Target<'_>,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<Distance, MeasureError> {
    let xs = elements(a)?;
    let ys = elements(b)?;
    if xs.is_empty() || ys.is_empty() {
        return Err(MeasureError::Empty);
    }
    work.spend(
        xs.len()
            .saturating_add(ys.len())
            .saturating_mul(ELEMENT_WORK),
    )?;
    Ok(least(&xs, &ys, tol.resolution(), work)?)
}

/// A pick's patch, curve or point, and the rounds it lies on.
#[derive(Debug, Clone, Copy)]
struct Element {
    shape: Shape,
    rounds: [Option<Round>; 2],
}

/// The elements of a pick: a body's patches, a face's, an edge's curves,
/// a corner's point.
fn elements(target: &Target<'_>) -> Result<Vec<Element>, MeasureError> {
    let mesh = target.solid.mesh();
    let patch = |t: u32| {
        let patch = mesh.patch(t as usize);
        let face = &mesh.faces()[mesh.tris()[t as usize].face as usize];
        Element {
            shape: Shape::Patch(patch),
            rounds: Round::of_form(&face.form),
        }
    };
    Ok(match target.pick {
        Pick::Body => (0..mesh.tris().len() as u32).map(patch).collect(),
        Pick::Face(r) => target.region(r)?.tris.iter().map(|&t| patch(t)).collect(),
        Pick::Edge(c) => target
            .chain(c)?
            .halfedges
            .iter()
            .map(|&h| {
                let curve = mesh.curve(h);
                Element {
                    shape: Shape::Curve(curve),
                    rounds: Round::of_curve(&curve),
                }
            })
            .collect(),
        Pick::Corner(c) => {
            let p = target.corner(c)?;
            vec![Element {
                shape: Shape::Point(p),
                rounds: [None, None],
            }]
        }
    })
}

/// A piece of an element.
#[derive(Debug, Clone, Copy)]
enum Shape {
    Patch(Patch),
    Curve(Conic3),
    Point(DVec3),
}

impl Shape {
    /// The control points, whose convex hull holds it, and how many.
    fn hull(&self) -> ([DVec3; 6], usize) {
        match *self {
            Shape::Patch(patch) => (patch.hull(), 6),
            Shape::Curve(curve) => {
                let [a, b, c] = curve.hull();
                ([a, b, c, c, c, c], 3)
            }
            Shape::Point(p) => ([p; 6], 1),
        }
    }

    fn bounds(&self) -> Bounds3 {
        match self {
            Shape::Patch(patch) => patch.bounds(),
            Shape::Curve(curve) => curve.bounds(),
            Shape::Point(p) => Bounds3::point(*p),
        }
    }

    /// The normal of a patch piece's corners' plane.
    fn normal(&self) -> Option<DVec3> {
        match self {
            Shape::Patch(patch) => corner_normal(patch),
            Shape::Curve(_) | Shape::Point(_) => None,
        }
    }

    /// Whether it needs no more splitting to be searched by Newton's
    /// method: flat within a quarter of `eps` ([`settled`]), or no more
    /// than `floor` across; a point always.
    fn settled(&self, eps: f64, floor: f64) -> bool {
        match self {
            Shape::Patch(patch) => settled(patch, eps, floor),
            Shape::Curve(curve) => {
                let b = curve.bounds();
                (b.max - b.min).max_element() <= floor
                    || straight(curve.p0, curve.c, curve.p1, eps / 4.0)
            }
            Shape::Point(_) => true,
        }
    }

    /// Whether it can be split: not a point.
    fn splits(&self) -> bool {
        !matches!(self, Shape::Point(_))
    }

    /// Its pieces into `out`, and how many: a patch's four
    /// ([`Patch::split4`]), a curve's halves, a point itself.
    fn split(&self, out: &mut [Shape; 4]) -> Result<usize, KernelError> {
        match self {
            Shape::Patch(patch) => {
                let pieces = patch.split4().map_err(|_| KernelError::TooComplex)?;
                *out = pieces.map(Shape::Patch);
                Ok(4)
            }
            Shape::Curve(curve) => {
                let [a, b] = curve.split_half().map_err(|_| KernelError::TooComplex)?;
                out[0] = Shape::Curve(a);
                out[1] = Shape::Curve(b);
                Ok(2)
            }
            Shape::Point(_) => {
                out[0] = *self;
                Ok(1)
            }
        }
    }

    /// Its middle, where Newton's method starts.
    fn middle(&self) -> Feature {
        match *self {
            Shape::Patch(patch) => Feature::Patch(patch, DVec3::splat(1.0 / 3.0)),
            Shape::Curve(curve) => Feature::Curve(curve, 0.5),
            Shape::Point(p) => Feature::Point(p),
        }
    }

    /// Its corners (a curve's ends) and middle, with their points, and how
    /// many.
    fn samples(&self) -> ([(Feature, DVec3); 4], usize) {
        match *self {
            Shape::Patch(patch) => {
                let at = |u: DVec3| (Feature::Patch(patch, u), patch.eval(u));
                (
                    [
                        at(DVec3::X),
                        at(DVec3::Y),
                        at(DVec3::Z),
                        at(DVec3::splat(1.0 / 3.0)),
                    ],
                    4,
                )
            }
            Shape::Curve(curve) => {
                let at = |t: f64| (Feature::Curve(curve, t), curve.eval(t));
                let samples = [at(0.0), at(1.0), at(0.5)];
                ([samples[0], samples[1], samples[2], samples[2]], 3)
            }
            Shape::Point(p) => ([(Feature::Point(p), p); 4], 1),
        }
    }
}

/// A function no faster than the point it is of, `|f(p) − f(q)| ≤ |p −
/// q|`, so the gap between its ranges over two pieces bounds their
/// distance: the distance from the line through `centre` along the unit
/// `axis` (a cylinder's or a circle's), or from `centre` without one (a
/// sphere's or a circle's).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Round {
    centre: DVec3,
    axis: Option<DVec3>,
}

impl Round {
    fn line(point: DVec3, axis: DVec3) -> Option<Round> {
        let axis = axis.try_normalize()?;
        (point.is_finite()).then_some(Round {
            centre: point,
            axis: Some(axis),
        })
    }

    fn point(centre: DVec3) -> Option<Round> {
        centre.is_finite().then_some(Round { centre, axis: None })
    }

    /// The rounds a patch of a face of `form` is on: a cylinder's axis, a
    /// sphere's centre.
    fn of_form(form: &Form) -> [Option<Round>; 2] {
        match *form {
            Form::Cylinder { point, axis, .. } => [Round::line(point, axis), None],
            Form::Sphere { centre, .. } => [Round::point(centre), None],
            _ => [None, None],
        }
    }

    /// The rounds a curve is on: a circle's axis and centre (see
    /// [`circle_of`]).
    fn of_curve(curve: &Conic3) -> [Option<Round>; 2] {
        let Some((centre, _)) = circle_of(curve) else {
            return [None, None];
        };
        let axis = (curve.c - curve.p0).cross(curve.p1 - curve.c);
        [Round::line(centre, axis), Round::point(centre)]
    }

    /// `x − centre`, across the axis.
    fn across(&self, x: DVec3) -> DVec3 {
        let v = x - self.centre;
        self.axis.map_or(v, |u| v - u * v.dot(u))
    }

    /// The range of the distance over `shape`, if it is finite.
    ///
    /// A point's is its distance. A patch or a curve is `Q/W` with `Q =
    /// Σ B_α q_α` and `W = Σ B_α w_α` over the quadratic Bernstein basis,
    /// `q_α = w_α·(x_α − centre)` across the axis, so the square of the
    /// distance is `|Q|²/W²`; both are quartics whose Bernstein
    /// coefficients are `Σ_{α+β=γ} m_α m_β/C₄(γ)·(q_α·q_β)` and the same of
    /// `w_α w_β` (`m_α` the quadratic basis's multinomial coefficient,
    /// `C₄` the quartic's), and with every weight positive the ratio lies
    /// between the least and the greatest ratio of their coefficients. On
    /// a piece of the round itself (a cylinder's patch round its axis) the
    /// ratios are all the radius squared, so the range is the radius to
    /// rounding, and on a piece ending on it (a cap's piece at a circular
    /// edge) the least is; hulls show either only once split to
    /// pieces about `√(8·resolution·r)` across. Widened by the rounding
    /// of the products.
    fn range(&self, shape: &Shape) -> Option<(f64, f64)> {
        let mut coeffs = [(DVec3::ZERO, 0.0, [0u8; 3]); 6];
        let n = match *shape {
            Shape::Point(p) => {
                let f = self.across(p).length();
                return f.is_finite().then_some((f, f));
            }
            Shape::Patch(patch) => {
                for i in 0..3 {
                    let j = (i + 1) % 3;
                    let mut corner = [0u8; 3];
                    corner[i] = 2;
                    let mut edge = [0u8; 3];
                    edge[i] = 1;
                    edge[j] = 1;
                    coeffs[i] = (self.across(patch.p[i]), 1.0, corner);
                    coeffs[3 + i] = (self.across(patch.c[i]) * patch.w[i], patch.w[i], edge);
                }
                6
            }
            Shape::Curve(curve) => {
                coeffs[0] = (self.across(curve.p0), 1.0, [2, 0, 0]);
                coeffs[1] = (self.across(curve.c) * curve.w, curve.w, [1, 1, 0]);
                coeffs[2] = (self.across(curve.p1), 1.0, [0, 2, 0]);
                3
            }
        };
        const FACTORIAL: [f64; 5] = [1.0, 1.0, 2.0, 6.0, 24.0];
        let multinomial =
            |e: [u8; 3], n: f64| n / e.iter().map(|&k| FACTORIAL[k as usize]).product::<f64>();
        // Indexed by the quartic's first two exponents.
        let mut top = [[0.0f64; 5]; 5];
        let mut bottom = [[0.0f64; 5]; 5];
        let mut biggest = 0.0f64;
        for &(qa, wa, ea) in &coeffs[..n] {
            biggest = biggest.max(qa.length_squared());
            for &(qb, wb, eb) in &coeffs[..n] {
                let e = [ea[0] + eb[0], ea[1] + eb[1], ea[2] + eb[2]];
                let k = multinomial(ea, 2.0) * multinomial(eb, 2.0) / multinomial(e, 24.0);
                top[e[0] as usize][e[1] as usize] += k * qa.dot(qb);
                bottom[e[0] as usize][e[1] as usize] += k * wa * wb;
            }
        }
        // Each coefficient is a mean of its terms (the factors `k` of one
        // add up to 1), so it rounds by a few ulps of the largest.
        let err = 32.0 * f64::EPSILON * biggest;
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for (t, b) in top.iter().flatten().zip(bottom.iter().flatten()) {
            if *b > 0.0 {
                lo = lo.min((t - err) / b);
                hi = hi.max((t + err) / b);
            }
        }
        let (lo, hi) = (
            lo * (1.0 - 16.0 * f64::EPSILON),
            hi * (1.0 + 16.0 * f64::EPSILON),
        );
        (lo.is_finite() && hi.is_finite()).then(|| (lo.max(0.0).sqrt(), hi.max(0.0).sqrt()))
    }
}

/// Whether one of the rounds `rounds` shows the pieces `x` and `y` more
/// than `margin` apart: the ranges of its distance over them more than
/// that apart, less the rounding of the coordinates.
fn round_apart(rounds: &[Round], x: &Shape, y: &Shape, margin: f64) -> bool {
    rounds.iter().any(|round| {
        let (Some((xlo, xhi)), Some((ylo, yhi))) = (round.range(x), round.range(y)) else {
            return false;
        };
        let gap = (ylo - xhi).max(xlo - yhi);
        let scale = round.centre.abs().max_element() + xhi.max(yhi);
        gap - 64.0 * f64::EPSILON * scale > margin
    })
}

/// The distance between two boxes: 0 where they meet.
fn box_gap(a: &Bounds3, b: &Bounds3) -> f64 {
    (a.min - b.max).max(b.min - a.max).max(DVec3::ZERO).length()
}

/// A point of a piece by its parameters: barycentric on a patch, `t` on
/// a curve.
#[derive(Debug, Clone, Copy)]
enum Feature {
    Patch(Patch, DVec3),
    Curve(Conic3, f64),
    Point(DVec3),
}

impl Feature {
    fn point(&self) -> DVec3 {
        match *self {
            Feature::Patch(patch, u) => patch.eval(u),
            Feature::Curve(curve, t) => curve.eval(t),
            Feature::Point(p) => p,
        }
    }

    /// How many parameters it has.
    fn dims(&self) -> usize {
        match self {
            Feature::Patch(..) => 2,
            Feature::Curve(..) => 1,
            Feature::Point(_) => 0,
        }
    }

    /// The point and its derivatives along the parameters (a patch's
    /// along `u0` and `u1`, with `u2 = 1 − u0 − u1`).
    fn derivs(&self) -> (DVec3, [DVec3; 2]) {
        match *self {
            Feature::Patch(patch, u) => {
                let [p, pu, pv] = patch.eval_derivs(u);
                (p, [pu, pv])
            }
            Feature::Curve(curve, t) => {
                let (p, d) = curve.eval_deriv(t);
                (p, [d, DVec3::ZERO])
            }
            Feature::Point(p) => (p, [DVec3::ZERO; 2]),
        }
    }

    /// The second derivatives by central differences, by the sum of the
    /// two parameters' indices: `[uu, uv, vv]` (a curve's `tt` first).
    fn second(&self) -> [DVec3; 3] {
        match *self {
            Feature::Patch(patch, u) => {
                let d = |u: DVec3| {
                    let [_, pu, pv] = patch.eval_derivs(u);
                    (pu, pv)
                };
                let (along_u, along_v) = (DVec3::new(H, 0.0, -H), DVec3::new(0.0, H, -H));
                let ((uu1, uv1), (uu0, uv0)) = (d(u + along_u), d(u - along_u));
                let ((_, vv1), (_, vv0)) = (d(u + along_v), d(u - along_v));
                [
                    (uu1 - uu0) / (2.0 * H),
                    (uv1 - uv0) / (2.0 * H),
                    (vv1 - vv0) / (2.0 * H),
                ]
            }
            Feature::Curve(curve, t) => {
                let (_, d1) = curve.eval_deriv(t + H);
                let (_, d0) = curve.eval_deriv(t - H);
                [(d1 - d0) / (2.0 * H), DVec3::ZERO, DVec3::ZERO]
            }
            Feature::Point(_) => [DVec3::ZERO; 3],
        }
    }

    /// How much of the step `d` (in its parameters) keeps inside the
    /// piece, at most all of it, and which side it meets first if not
    /// all: the barycentric coordinate reaching 0, or the curve's end.
    fn room(&self, d: &[f64]) -> (f64, Option<usize>) {
        let mut room = (1.0, None);
        match *self {
            Feature::Patch(_, u) => {
                let du = DVec3::new(d[0], d[1], -d[0] - d[1]);
                for k in 0..3 {
                    if du[k] < 0.0 && u[k] + du[k] < 0.0 {
                        let a = u[k] / -du[k];
                        if a < room.0 {
                            room = (a, Some(k));
                        }
                    }
                }
            }
            Feature::Curve(_, t) => {
                if t + d[0] < 0.0 {
                    room = (t / -d[0], Some(0));
                } else if t + d[0] > 1.0 {
                    room = ((1.0 - t) / d[0], Some(1));
                }
            }
            Feature::Point(_) => {}
        }
        room
    }

    /// Moved by `alpha` times the step `d`, onto the side `side` exactly
    /// if it meets one, and kept inside.
    fn moved(&self, d: &[f64], alpha: f64, side: Option<usize>) -> Feature {
        match *self {
            Feature::Patch(patch, u) => {
                let mut u = u + DVec3::new(d[0], d[1], -d[0] - d[1]) * alpha;
                if let Some(k) = side {
                    u[k] = 0.0;
                }
                let u = u.max(DVec3::ZERO);
                let sum = u.element_sum();
                let u = if sum > 0.0 && sum.is_finite() {
                    u / sum
                } else {
                    DVec3::splat(1.0 / 3.0)
                };
                Feature::Patch(patch, u)
            }
            Feature::Curve(curve, t) => {
                let t = match side {
                    Some(0) => 0.0,
                    Some(_) => 1.0,
                    None => (t + d[0] * alpha).clamp(0.0, 1.0),
                };
                Feature::Curve(curve, t)
            }
            Feature::Point(_) => *self,
        }
    }

    /// This feature (it stepped down onto its side `side` to `child`) at
    /// `child`'s point, and the direction into it from there in its
    /// parameters.
    fn lifted(&self, child: &Feature, side: usize) -> (Feature, [f64; 2]) {
        match (*self, *child) {
            (Feature::Patch(patch, _), Feature::Curve(_, t)) => {
                let (e, f) = ((side + 1) % 3, (side + 2) % 3);
                let mut u = DVec3::ZERO;
                u[e] = 1.0 - t;
                u[f] = t;
                let mut inward = DVec3::splat(-0.5);
                inward[side] = 1.0;
                (Feature::Patch(patch, u), [inward[0], inward[1]])
            }
            (Feature::Curve(curve, _), _) => {
                let (t, inward) = if side == 0 { (0.0, 1.0) } else { (1.0, -1.0) };
                (Feature::Curve(curve, t), [inward, 0.0])
            }
            _ => (*self, [0.0; 2]),
        }
    }

    /// The side `side` it is on ([`Self::room`]'s): a patch's edge where
    /// the coordinate is 0, a curve's end.
    fn restricted(&self, side: usize) -> Feature {
        match *self {
            Feature::Patch(patch, u) => {
                // Edge `e` runs from corner `e` to corner `e + 1`, the
                // patch's restriction there by its parameter.
                let (e, f) = ((side + 1) % 3, (side + 2) % 3);
                let sum = u[e] + u[f];
                let t = if sum > 0.0 { u[f] / sum } else { 0.5 };
                Feature::Curve(patch.edge(e), t)
            }
            Feature::Curve(curve, _) => Feature::Point(if side == 0 { curve.p0 } else { curve.p1 }),
            Feature::Point(_) => *self,
        }
    }
}

/// How often one search for closest points may step back up from an edge
/// or a corner into the piece it stepped down from.
const RELEASES: usize = 4;

/// One side of a search for closest points: where it is, and the
/// features it stepped down from onto an edge or a corner, with the side
/// it stepped onto, latest last.
#[derive(Debug, Clone, Copy)]
struct Side {
    at: Feature,
    up: [Option<(Feature, usize)>; 2],
}

impl Side {
    fn new(at: Feature) -> Side {
        Side { at, up: [None; 2] }
    }

    /// Steps down onto side `k` ([`Feature::room`]'s).
    fn restrict(&mut self, k: usize) {
        let slot = usize::from(self.up[0].is_some());
        self.up[slot] = Some((self.at, k));
        self.at = self.at.restricted(k);
    }

    /// Steps back up into the feature it last stepped down from, if moving
    /// into it from here brings it nearer `other`: so a step that left a
    /// patch too early (a Newton step overshooting onto an edge) comes
    /// back. At a patch's corner reached along one of its edges, it may
    /// instead go on along the other edge there: where moving into the
    /// patch from its corner brings it nearer, moving along one of the two
    /// edges does.
    fn release(&mut self, other: DVec3) -> bool {
        let slot = usize::from(self.up[1].is_some());
        let Some((parent, k)) = self.up[slot] else {
            return false;
        };
        let (up, inward) = parent.lifted(&self.at, k);
        if nearer(&up, inward, other) {
            self.up[slot] = None;
            self.at = up;
            return true;
        }
        if let [Some((Feature::Patch(patch, u), side)), Some((_, end))] = self.up {
            // Down edge `e`, then to its end `end`: the corner, and the
            // other edge there.
            let e = (side + 1) % 3;
            let corner = if end == 0 { e } else { (e + 1) % 3 };
            let next = if e == corner {
                (corner + 2) % 3
            } else {
                corner
            };
            let (t, inward) = if next == corner {
                (0.0, 1.0)
            } else {
                (1.0, -1.0)
            };
            let along = Feature::Curve(patch.edge(next), t);
            if nearer(&along, [inward, 0.0], other) {
                self.at = along;
                // Edge `next` is where coordinate `next + 2` is 0.
                self.up = [Some((Feature::Patch(patch, u), (next + 2) % 3)), None];
                return true;
            }
        }
        false
    }
}

/// Whether moving from `at` along `inward` (in its parameters) brings
/// it nearer `other`, by more than the rounding.
fn nearer(at: &Feature, inward: [f64; 2], other: DVec3) -> bool {
    let (p, d) = at.derivs();
    let r = p - other;
    let along = d[0] * inward[0] + d[1] * inward[1];
    r.dot(along) < -1e-9 * r.length() * along.length()
}

/// The closest points of two pieces near `x` and `y` by Newton's method
/// on the squared distance over both pieces' parameters (second
/// derivatives by central differences; Levenberg–Marquardt's step on
/// Gauss–Newton's matrix where that isn't a minimum's; a step halved
/// while it would move the points apart). A step leaving a piece stops
/// where it meets its boundary, and the search goes on along that edge,
/// then at that corner (a curve's end), stepping back up when moving into
/// the piece brings the points nearer ([`Side::release`]): so minima on
/// edges and at corners are found as those inside. Points of the pieces
/// at every step, no further apart than `x`'s and `y`'s but for the
/// rounding of their distance.
fn closest(x: Feature, y: Feature) -> (Feature, Feature) {
    let (mut x, mut y) = (Side::new(x), Side::new(y));
    let mut releases = 0;
    for _ in 0..CLOSEST_STEPS {
        if newton_step(&mut x, &mut y) {
            continue;
        }

        let (px, py) = (x.at.point(), y.at.point());
        if px == py || releases == RELEASES || !(x.release(py) || y.release(px)) {
            break;
        }
        releases += 1;
    }
    (x.at, y.at)
}

/// One step of [`closest`]: whether it moved on (by more than rounding,
/// or onto an edge or a corner).
fn newton_step(sx: &mut Side, sy: &mut Side) -> bool {
    let (x, y) = (sx.at, sy.at);
    let (nx, ny) = (x.dims(), y.dims());
    let n = nx + ny;
    if n == 0 {
        return false;
    }
    let (px, dx) = x.derivs();
    let (py, dy) = y.derivs();
    let r = px - py;
    let f0 = r.length_squared();
    if f0 == 0.0 {
        return false;
    }
    let mut cols = [DVec3::ZERO; 4];
    cols[..nx].copy_from_slice(&dx[..nx]);
    for i in 0..ny {
        cols[nx + i] = -dy[i];
    }
    let mut g = [0.0; 4];
    let mut gauss = [[0.0; 4]; 4];
    for i in 0..n {
        g[i] = -cols[i].dot(r);
        for j in 0..n {
            gauss[i][j] = cols[i].dot(cols[j]);
        }
    }
    let mut newton = gauss;
    let (ssx, ssy) = (x.second(), y.second());
    for i in 0..nx {
        for j in 0..nx {
            newton[i][j] += r.dot(ssx[i + j]);
        }
    }
    for i in 0..ny {
        for j in 0..ny {
            newton[nx + i][nx + j] -= r.dot(ssy[i + j]);
        }
    }
    // Levenberg–Marquardt's damping, by the gradient's size: along a line
    // of closest points (a pin along a hole's wall) the matrix is
    // singular, and a damping by its scale alone made steps of `10⁵`
    // along that line out of the gradient's rounding.
    let damped = {
        let most = (0..n).map(|i| gauss[i][i]).fold(0.0, f64::max);
        let steep = g[..n].iter().fold(0.0f64, |m, v| m.max(v.abs()));
        let mut m = gauss;
        for (i, row) in m.iter_mut().enumerate().take(n) {
            row[i] += steep + 1e-9 * most;
        }
        m
    };
    let Some(mut d) = solve(&newton, &g, n).or_else(|| solve(&damped, &g, n)) else {
        return false;
    };
    // At most a piece across.
    let longest = d[..n].iter().fold(0.0f64, |m, v| m.max(v.abs()));
    if longest > 1.0 {
        d = d.map(|v| v / longest);
    }
    for _ in 0..HALVINGS {
        let (ax, sidex) = x.room(&d[..nx]);
        let (ay, sidey) = y.room(&d[nx..n]);
        let alpha = ax.min(ay);
        let (sidex, sidey) = if ax <= ay {
            (sidex, None)
        } else {
            (None, sidey)
        };
        let (x1, y1) = (
            x.moved(&d[..nx], alpha, sidex),
            y.moved(&d[nx..n], alpha, sidey),
        );
        // A step that moves the points apart by no more than the
        // rounding of their squared distance still counts: near the
        // closest points the distance is flat, and a test to the bit
        // stopped Newton's method `10⁻¹¹` short of them.
        if (x1.point() - y1.point()).length_squared() <= f0 * (1.0 + 4.0 * f64::EPSILON) {
            let step = d[..n].iter().fold(0.0f64, |m, v| m.max(v.abs())) * alpha;
            (sx.at, sy.at) = (x1, y1);
            return match (sidex, sidey) {
                (Some(k), _) => {
                    sx.restrict(k);
                    true
                }
                (_, Some(k)) => {
                    sy.restrict(k);
                    true
                }
                // Converged: a step this small in the parameters moves
                // the distance by its square times the curvature.
                (None, None) => step > 1e-12,
            };
        }
        d = d.map(|v| v * 0.5);
    }
    false
}

/// `a·x = b` for the symmetric `n × n` top left of `a` by Cholesky's
/// factoring, if it is clearly positive definite (each pivot more than
/// `1e-12` of its diagonal's).
fn solve(a: &[[f64; 4]; 4], b: &[f64; 4], n: usize) -> Option<[f64; 4]> {
    let mut l = [[0.0; 4]; 4];
    for i in 0..n {
        for j in 0..=i {
            let mut s = a[i][j];
            for k in 0..j {
                s -= l[i][k] * l[j][k];
            }
            if i == j {
                if s.is_nan() || s <= 1e-12 * a[i][i] {
                    return None;
                }
                l[i][i] = s.sqrt();
            } else {
                l[i][j] = s / l[j][j];
            }
        }
    }
    let mut y = [0.0; 4];
    for i in 0..n {
        let mut s = b[i];
        for k in 0..i {
            s -= l[i][k] * y[k];
        }
        y[i] = s / l[i][i];
    }
    let mut x = [0.0; 4];
    for i in (0..n).rev() {
        let mut s = y[i];
        for k in i + 1..n {
            s -= l[k][i] * x[k];
        }
        x[i] = s / l[i][i];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// The closest points found so far.
#[derive(Debug, Clone, Copy)]
struct Best {
    distance: f64,
    points: [DVec3; 2],
    /// Where they are, for a last search by Newton's method.
    at: (Feature, Feature),
}

impl Best {
    fn new() -> Best {
        let nowhere = Feature::Point(DVec3::ZERO);
        Best {
            distance: f64::INFINITY,
            points: [DVec3::ZERO; 2],
            at: (nowhere, nowhere),
        }
    }

    /// Takes `x` and `y`, at `p` and `q`, if they are nearer.
    fn offer_at(&mut self, x: Feature, p: DVec3, y: Feature, q: DVec3) {
        let distance = p.distance(q);
        if distance < self.distance {
            *self = Best {
                distance,
                points: [p, q],
                at: (x, y),
            };
        }
    }

    fn offer(&mut self, (x, y): (Feature, Feature)) {
        self.offer_at(x, x.point(), y, y.point());
    }

    /// Takes the nearest of the two pieces' corners and middles.
    fn samples(&mut self, x: &Shape, y: &Shape) {
        let ((xs, nx), (ys, ny)) = (x.samples(), y.samples());
        for &(fx, p) in &xs[..nx] {
            for &(fy, q) in &ys[..ny] {
                self.offer_at(fx, p, fy, q);
            }
        }
    }
}

/// A pair of pieces of the elements `ex` of the first pick and `ey` of
/// the second, `depth` splits below theirs, and how deep the last pair of
/// its lineage Newton's method ran on was.
#[derive(Debug, Clone, Copy)]
struct Pair {
    x: Shape,
    y: Shape,
    ex: u32,
    ey: u32,
    depth: u32,
    tried: Option<u32>,
}

/// What the search visits: a pair of nodes of the two picks' trees, or a
/// pair of pieces.
#[allow(
    clippy::large_enum_variant,
    reason = "most of what the search visits are pieces; boxing them would allocate on every split"
)]
#[derive(Debug, Clone, Copy)]
enum Visit {
    Nodes([u32; 2]),
    Pieces(Pair),
}

/// How many elements a leaf of a [`Tree`] holds at most.
const LEAF: usize = 4;

/// A tree of boxes over a pick's elements, split at the median along the
/// longest side of their centres' box (ties by index), for the search to
/// go down both picks' trees together, nearest pairs of nodes first.
#[derive(Debug)]
struct Tree {
    nodes: Vec<Node>,
    /// Element indices; a leaf holds a run of them.
    items: Vec<u32>,
}

#[derive(Debug, Clone, Copy)]
struct Node {
    bounds: Bounds3,
    kind: Kind,
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    /// Its children's node indices.
    Inner(u32, u32),
    /// The run `items[start..start + count]`.
    Leaf(u32, u32),
}

impl Tree {
    /// The tree over `boxes` (not empty); node 0 is the root.
    fn new(boxes: &[Bounds3]) -> Tree {
        let mut tree = Tree {
            nodes: Vec::with_capacity(2 * boxes.len() / LEAF + 1),
            items: (0..boxes.len() as u32).collect(),
        };
        tree.build(boxes, 0, boxes.len());
        tree
    }

    /// Builds the node over `items[start..end]`, and returns its index.
    fn build(&mut self, boxes: &[Bounds3], start: usize, end: usize) -> u32 {
        let run = &mut self.items[start..end];
        let bounds = run
            .iter()
            .map(|&i| boxes[i as usize])
            .reduce(Bounds3::union)
            .expect("not empty");
        let index = self.nodes.len() as u32;
        if run.len() <= LEAF {
            self.nodes.push(Node {
                bounds,
                kind: Kind::Leaf(start as u32, run.len() as u32),
            });
            return index;
        }
        let centre = |i: u32| {
            let b = boxes[i as usize];
            (b.min + b.max) * 0.5
        };
        let centres = run
            .iter()
            .map(|&i| Bounds3::point(centre(i)))
            .reduce(Bounds3::union)
            .expect("not empty");
        let size = centres.max - centres.min;
        let k = if size.x >= size.y && size.x >= size.z {
            0
        } else if size.y >= size.z {
            1
        } else {
            2
        };
        run.sort_by(|&a, &b| centre(a)[k].total_cmp(&centre(b)[k]).then(a.cmp(&b)));
        // Placeholder until the children are built.
        self.nodes.push(Node {
            bounds,
            kind: Kind::Leaf(0, 0),
        });
        let middle = start + (end - start) / 2;
        let first = self.build(boxes, start, middle);
        let second = self.build(boxes, middle, end);
        self.nodes[index as usize].kind = Kind::Inner(first, second);
        index
    }
}

/// The minimum distance between the elements `xs` and `ys` (neither
/// empty), within `eps` (see the [module](self) docs).
fn least(
    xs: &[Element],
    ys: &[Element],
    eps: f64,
    work: &mut Work,
) -> Result<Distance, KernelError> {
    let bx: Vec<Bounds3> = xs.iter().map(|e| e.shape.bounds()).collect();
    let by: Vec<Bounds3> = ys.iter().map(|e| e.shape.bounds()).collect();
    // Building the trees: a sort of each level.
    let levels = |n: usize| (usize::BITS - n.leading_zeros()) as usize;
    work.spend(
        xs.len()
            .saturating_mul(levels(xs.len()))
            .saturating_add(ys.len().saturating_mul(levels(ys.len())))
            / 4,
    )?;
    let (tx, ty) = (Tree::new(&bx), Tree::new(&by));
    let mut best = Best::new();

    let floor = MIN_SPLIT * eps;
    let mut band = 0;
    let visit = |visit: &mut Visit, work: &mut Work| -> Result<Step, KernelError> {
        let margin = best.distance - eps;
        if margin.is_nan() || margin < 0.0 {
            // Nothing can come nearer than the best by more than `eps`.
            return Ok(Step::Stop);
        }
        let pair = match visit {
            Visit::Nodes([a, b]) => {
                // While pairs that can come nearer than the best by less
                // than `eps` still get Newton's method (below), the nodes
                // they are in are gone down.
                let limit = if band < BAND_TRIES {
                    margin.max(best.distance * (1.0 - BAND))
                } else {
                    margin
                };
                let gap = box_gap(&tx.nodes[*a as usize].bounds, &ty.nodes[*b as usize].bounds);
                return Ok(if gap > limit {
                    Step::Drop
                } else {
                    Step::Split(Which::Both)
                });
            }
            Visit::Pieces(pair) => pair,
        };
        let (bpx, bpy) = (pair.x.bounds(), pair.y.bounds());
        let mut rounds = [Round::point(DVec3::ZERO).expect("finite"); 4];
        let mut n = 0;
        for round in xs[pair.ex as usize]
            .rounds
            .iter()
            .chain(&ys[pair.ey as usize].rounds)
            .flatten()
        {
            if !rounds[..n].contains(round) {
                rounds[n] = *round;
                n += 1;
            }
        }
        let rounds = &rounds[..n];
        let ((hx, nx), (hy, ny)) = (pair.x.hull(), pair.y.hull());
        let (hx, hy) = (&hx[..nx], &hy[..ny]);
        let (sx, sy) = (pair.x.settled(eps, floor), pair.y.settled(eps, floor));
        let normals = [pair.x.normal(), pair.y.normal()];
        let beyond = |margin: f64, work: &mut Work| -> Result<bool, KernelError> {
            if box_gap(&bpx, &bpy) > margin
                || apart(hx, hy, margin)
                || (sx && sy && apart_along(hx, hy, normals.into_iter().flatten(), margin))
            {
                return Ok(true);
            }
            if rounds.is_empty() {
                return Ok(false);
            }
            work.spend(ROUND_WORK)?;
            Ok(round_apart(rounds, &pair.x, &pair.y, margin))
        };
        if beyond(margin, work)? {
            // A pair of elements that may still come nearer than the best
            // by less than `eps` (and more than rounding: not ties, as
            // the pairs of a box's faces round a corner are): Newton's
            // method on it anyway, a bounded number of times, so the
            // answer is to rounding in practice and not only within
            // `eps`.
            if pair.depth == 0 && band < BAND_TRIES && !beyond(best.distance * (1.0 - BAND), work)?
            {
                band += 1;
                work.spend(CLOSEST_WORK)?;
                best.offer(closest(pair.x.middle(), pair.y.middle()));
            }
            return Ok(Step::Drop);
        }
        let before = best.distance;
        best.samples(&pair.x, &pair.y);
        if pair.depth == 0
            || (sx
                && sy
                && pair
                    .tried
                    .is_none_or(|t| pair.depth >= t.saturating_add(RETRY)))
        {
            work.spend(CLOSEST_WORK)?;
            pair.tried = Some(pair.depth);
            best.offer(closest(pair.x.middle(), pair.y.middle()));
        }
        if best.distance < before {
            let margin = best.distance - eps;
            if margin.is_nan() || margin < 0.0 {
                return Ok(Step::Stop);
            }
            if beyond(margin, work)? {
                return Ok(Step::Drop);
            }
        }
        // Both within half of `eps` across: their samples are within
        // `eps` of their closest points.
        let size = |b: &Bounds3| (b.max - b.min).length();
        let (zx, zy) = (size(&bpx), size(&bpy));
        if zx <= eps * 0.5 && zy <= eps * 0.5 {
            return Ok(Step::Drop);
        }
        let (px, py) = (pair.x.splits(), pair.y.splits());
        let which = if sx && sy {
            // Both flat: the larger, or both if neither is twice the
            // other. A point never splits, and a pair of points has been
            // dropped.
            if !py || (px && zx >= 2.0 * zy) {
                Which::First
            } else if !px || zy >= 2.0 * zx {
                Which::Second
            } else {
                Which::Both
            }
        } else {
            match (sx, sy) {
                (true, false) => Which::Second,
                (false, true) => Which::First,
                _ => Which::Both,
            }
        };
        Ok(Step::Split(which))
    };
    let split = |visit: &Visit, which: Which, out: &mut Vec<Visit>| -> Result<(), KernelError> {
        match *visit {
            Visit::Nodes([a, b]) => {
                let (na, nb) = (tx.nodes[a as usize], ty.nodes[b as usize]);
                let diagonal = |n: &Node| (n.bounds.max - n.bounds.min).length();
                let from = out.len();
                match (na.kind, nb.kind) {
                    (Kind::Leaf(sa, ca), Kind::Leaf(sb, cb)) => {
                        for &i in &tx.items[sa as usize..(sa + ca) as usize] {
                            for &j in &ty.items[sb as usize..(sb + cb) as usize] {
                                out.push(Visit::Pieces(Pair {
                                    x: xs[i as usize].shape,
                                    y: ys[j as usize].shape,
                                    ex: i,
                                    ey: j,
                                    depth: 0,
                                    tried: None,
                                }));
                            }
                        }
                    }
                    (Kind::Inner(c0, c1), Kind::Leaf(..)) => {
                        out.extend([Visit::Nodes([c0, b]), Visit::Nodes([c1, b])]);
                    }
                    (Kind::Leaf(..), Kind::Inner(c0, c1)) => {
                        out.extend([Visit::Nodes([a, c0]), Visit::Nodes([a, c1])]);
                    }
                    (Kind::Inner(c0, c1), Kind::Inner(d0, d1)) => {
                        if diagonal(&na) >= diagonal(&nb) {
                            out.extend([Visit::Nodes([c0, b]), Visit::Nodes([c1, b])]);
                        } else {
                            out.extend([Visit::Nodes([a, d0]), Visit::Nodes([a, d1])]);
                        }
                    }
                }
                // Nearest first (stable: equal gaps in the order made).
                let gap = |v: &Visit| match v {
                    Visit::Nodes([a, b]) => {
                        box_gap(&tx.nodes[*a as usize].bounds, &ty.nodes[*b as usize].bounds)
                    }
                    Visit::Pieces(pair) => box_gap(&bx[pair.ex as usize], &by[pair.ey as usize]),
                };
                out[from..].sort_by(|v, w| gap(v).total_cmp(&gap(w)));
            }
            Visit::Pieces(pair) => {
                let (mut xs, mut ys) = ([pair.x; 4], [pair.y; 4]);
                let nx = if which == Which::Second {
                    1
                } else {
                    pair.x.split(&mut xs)?
                };
                let ny = if which == Which::First {
                    1
                } else {
                    pair.y.split(&mut ys)?
                };
                for x in &xs[..nx] {
                    for y in &ys[..ny] {
                        out.push(Visit::Pieces(Pair {
                            x: *x,
                            y: *y,
                            depth: pair.depth + 1,
                            ..pair
                        }));
                    }
                }
            }
        }
        Ok(())
    };
    search([Visit::Nodes([0, 0])], visit, split, work)?;

    // Newton's method from the best, which samples alone may have found.
    work.spend(CLOSEST_WORK)?;
    let (x, y) = best.at;
    best.offer(closest(x, y));
    Ok(Distance {
        distance: best.distance,
        points: best.points,
    })
}

#[cfg(test)]
mod tests;
