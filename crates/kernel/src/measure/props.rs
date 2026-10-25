//! What the measure tool reads of a body besides its mass properties
//! ([`mass`]): the inertia, a user face's area and form, a
//! user edge's length and shape, and a face's rectangle. Faces and edges
//! here are the user topology's ([`Topology`]): a region's faces, a
//! chain's edges.
//!
//! **Inertia** ([`inertia`]). The second moments `∫ (xᵢ − oᵢ)(xⱼ − oⱼ)
//! dV` come from the same flux integrals as the volume and the first
//! moments, `⅕·∮ (xᵢ − oᵢ)(xⱼ − oⱼ)·((x − o)·n) dA` (the integrand
//! `P'ᵢP'ⱼ·det/W⁵`), with their fits' residuals and the ribbons' flux
//! (area times `R³`) as error; moved to the centroid `c` by `J = S −
//! M·(c − o)ᵀ` (`M` the first moments) and turned into the inertia
//! tensor `I = tr(J)·1 − J` at unit density, every step in [`Bounded`]
//! arithmetic, so the error carries the cancellation.
//!
//! **Lengths** ([`edges_length`]). A span `x = H/W` runs at the speed
//! `|H'W − HW'|/W² = √q/W²`, `q` a polynomial. As the area's integrand
//! on a cell, it is replaced by `S·A`, `S` a polynomial fit of `√q`
//! with `|√q − S| = |q − S²|/(√q + S)` and `A` one of `1/W²` with the
//! bound of `|1 − A·W²|`, both certified on Bernstein coefficients; the
//! integral of `S·A` is exact (the mean of its coefficients), so the
//! length lies within `(|√q − S| + max|S|·|1 − A·W²|)/min W²` of it,
//! with the rounding. A span whose `q` is zero has length 0.
//!
//! **Shapes** ([`edge_shape`]). A line, circle or ellipse names what an
//! edge was drawn as, for the measure tool and for references; no
//! topology or decision reads it. A chain is a line when every span's
//! middle control point is on its chord and every control point on the
//! line through the chain's ends; a circle or an ellipse when five points
//! of each span lie on the one its most turning span is an arc of; each
//! within [`SHAPE_SLACK`] of the chain's size plus a few roundings of its
//! coordinates. The values are `f64`s from the stored control points.
//!
//! **Charges.** The fits are charged as the mass properties' are; the
//! faces or edges are the items of one pass, summed in their order.

use glam::DVec3;
use varde_decide::bezier::{HomCurve, Poly, Tensor};
use varde_decide::num::bounded::{Bounded, add_up, div_up, mul_up};

use super::mass::{
    self, DEN_TAU, Integrand, Lus, MassError, RESID_DEPTH, ROOT_TAU, interpolate, mean, pick,
    pow_down, search, sup_abs, sup_root,
};
use crate::geom::curve::Span3;
use crate::geom::form::FormKey;
use crate::infra::budget::{Budget, Ledger, Pass, Stop, Work};
use crate::infra::error::{Invalid, KernelError};
use crate::names::topology::{Chain, Topology};
use crate::topo::brep::{BodyKind, Brep, EdgeId, FaceId, VertId};

#[cfg(test)]
mod tests;

type B = Bounded<f64>;

/// A solid's inertia at unit density, each value certified: the true
/// value lies within [`Bounded::err`] of the value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inertia {
    /// The volume (mm³).
    pub volume: Bounded<f64>,
    /// The centroid, `None` where the volume isn't certainly positive.
    pub centroid: Option<[Bounded<f64>; 3]>,
    /// The inertia tensor about the centroid (mm⁵): `Iᵢᵢ = ∫ (|x − c|² −
    /// (xᵢ − cᵢ)²) dV` on the diagonal, `Iᵢⱼ = −∫ (xᵢ − cᵢ)(xⱼ − cⱼ) dV`
    /// off it; `None` with the centroid.
    pub tensor: Option<[[Bounded<f64>; 3]; 3]>,
}

/// The pairs `(i, j)`, `i ≤ j`, of the second moments, in the order
/// [`inertia`] integrates them.
const PAIRS: [(usize, usize); 6] = [(0, 0), (0, 1), (0, 2), (1, 1), (1, 2), (2, 2)];

/// A solid's volume, centroid and inertia tensor (module docs), on a
/// budget of `budget`. A sheet has none: `Invalid(NotClosed)`; a broken
/// store and running out are as for [`mass_props`](super::mass::mass_props).
pub fn inertia(brep: &Brep, budget: &Budget) -> Result<Inertia, KernelError> {
    if brep.kind() != BodyKind::Solid {
        return Err(KernelError::Invalid(Invalid::NotClosed));
    }
    let mut set = vec![
        Integrand::Volume,
        Integrand::Moment(0),
        Integrand::Moment(1),
        Integrand::Moment(2),
    ];
    set.extend(PAIRS.iter().map(|&(i, j)| Integrand::Second(i, j)));
    let o = mass::centre(brep);
    let sums = mass::faces_summed_over(brep, &all_faces(brep), o, &set, budget)?;
    let volume = sums[0] / B::exact(3.0);
    if volume.v <= volume.err() {
        return Ok(Inertia {
            volume,
            centroid: None,
            tensor: None,
        });
    }
    let moment: [B; 3] = std::array::from_fn(|k| sums[1 + k] / B::exact(4.0));
    let shift: [B; 3] = moment.map(|m| m / volume);
    let second = |i: usize, j: usize| {
        let (a, b) = if i <= j { (i, j) } else { (j, i) };
        let p = PAIRS.iter().position(|&q| q == (a, b)).expect("a pair");
        sums[4 + p] / B::exact(5.0)
    };
    // About the centroid: `J = S − M·(c − o)ᵀ`, symmetric.
    let j: [[B; 3]; 3] =
        std::array::from_fn(|a| std::array::from_fn(|b| second(a, b) - moment[a] * shift[b]));
    let trace = j[0][0] + j[1][1] + j[2][2];
    let tensor = std::array::from_fn(|a| {
        std::array::from_fn(|b| if a == b { trace - j[a][b] } else { -j[a][b] })
    });
    Ok(Inertia {
        volume,
        centroid: Some(std::array::from_fn(|k| B::exact(o[k]) + shift[k])),
        tensor: Some(tensor),
    })
}

/// Every face of the body, in order.
fn all_faces(brep: &Brep) -> Vec<FaceId> {
    (0..brep.face_count())
        .map(|f| FaceId(u32::try_from(f).expect("ids below NONE")))
        .collect()
}

/// The area of `faces` (module docs of [`mass`]), of a
/// sheet's as of a solid's, each face once in the order given.
pub fn faces_area(
    brep: &Brep,
    faces: &[FaceId],
    budget: &Budget,
) -> Result<Bounded<f64>, KernelError> {
    let o = mass::centre(brep);
    Ok(mass::faces_summed_over(brep, faces, o, &[Integrand::Area], budget)?[0])
}

/// The length of `edges`' curves (module docs), each edge once in the
/// order given, the edges the items of one pass on `budget`.
pub fn edges_length(
    brep: &Brep,
    edges: &[EdgeId],
    budget: &Budget,
) -> Result<Bounded<f64>, KernelError> {
    let mut work = Work::new(budget);
    let pass = Pass {
        stage: "measure",
        rung: 0,
    };
    let lengths = work
        .pass(pass, edges, |&e, ledger| edge_length(brep, e, ledger))
        .map_err(KernelError::from)?;
    Ok(lengths.into_iter().fold(B::exact(0.0), |s, l| s + l))
}

/// The length of an edge's spans, charged to `ledger`.
fn edge_length(brep: &Brep, e: EdgeId, ledger: &mut Ledger<'_>) -> Result<B, Stop<MassError>> {
    let mut fits = SpanFits::default();
    let mut sum = B::exact(0.0);
    for span in brep.edge_spans(e) {
        sum = sum + fits.length(span, ledger)?;
    }
    Ok(sum)
}

/// The fits of an edge's spans: the factorisations, and the degrees the
/// last span's fits of `√q` and `1/W²` ended at, where the next one's
/// search starts.
#[derive(Default)]
struct SpanFits {
    lus: Lus,
    start: [[usize; 2]; 2],
}

impl SpanFits {
    /// The span's length (module docs).
    fn length(&mut self, span: &Span3, ledger: &mut Ledger<'_>) -> Result<B, Stop<MassError>> {
        let like = B::exact(0.0);
        let uncertified = B::new(0.0, f64::INFINITY, 0.0);
        let Some(c) = HomCurve::from_affine(span.iter().map(|p| ([p[0], p[1], p[2]], p[3])), &like)
        else {
            return Ok(uncertified);
        };
        let d = c.derivative();
        let (w, dw) = (c.weights(), d.weights());
        let mut q: Option<Poly<B>> = None;
        for k in 0..3 {
            let h = w
                .mul(&d.coords()[k])
                .zip(dw.mul(&c.coords()[k]))
                .map(|(a, b)| a.sub(&b));
            let Some(sq) = h.and_then(|h| h.mul(&h)) else {
                return Ok(uncertified);
            };
            q = Some(match q {
                Some(s) => s.add(&sq),
                None => sq,
            });
        }
        let (Some(q), Some(w2)) = (q, w.mul(w)) else {
            return Ok(uncertified);
        };
        if q.coefs().iter().all(|c| c.v == 0.0 && c.err() == 0.0) {
            return Ok(B::exact(0.0));
        }
        let line =
            |p: &Poly<B>| Tensor::new(p.degree(), 0, p.coefs().iter().copied()).expect("a line");
        let (q, w2) = (line(&q), line(&w2));
        let one = Tensor::from_f64(0, 0, &[1.0]).expect("a constant");
        let lus = &mut self.lus;
        let root = search(self.start[0], 1, ledger, |k| {
            let (s, scale) = interpolate(k, lus, |x, y| {
                let v = q.eval(&B::exact(x), &B::exact(y)).v;
                pick(v > 0.0, v, 0.0).sqrt()
            })?;
            let r = q.sub(&s.mul(&s)?);
            let target = ROOT_TAU * scale;
            Some((s.clone(), sup_root(&r, &q, &s, target, RESID_DEPTH), target))
        })?;
        self.start[0] = root.k;
        let den = search(self.start[1], 1, ledger, |k| {
            let (a, _) = interpolate(k, lus, |x, y| 1.0 / w2.eval(&B::exact(x), &B::exact(y)).v)?;
            let r = one.sub(&a.mul(&w2)?);
            Some((a, sup_abs(&r, DEN_TAU, RESID_DEPTH), DEN_TAU))
        })?;
        self.start[1] = den.k;
        let Some(p) = root.p.mul(&den.p) else {
            return Ok(uncertified);
        };
        let s_max = root.p.hull().abs_max();
        let e = div_up(
            add_up(root.e, mul_up(s_max, den.e)),
            pow_down(w2.hull().lo, 1),
        );
        let e = if e.is_nan() { f64::INFINITY } else { e };
        let row = Poly::new(p.coefs().iter().copied()).expect("degree 0 in v");
        Ok(mean(&row).with_trunc(e))
    }
}

/// What a chain of edges makes (module docs): every point in the world
/// frame, in mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EdgeShape {
    /// A straight edge from `from` to `to`, its ends in their geometric
    /// order (lexicographic in `x`, `y`, `z`).
    Line { from: [f64; 3], to: [f64; 3] },
    /// An arc of the circle (or the whole circle) about `centre` in the
    /// plane square to the unit `axis`, which turns the way the chain
    /// runs from its `from` end (right-handed); for a closed chain, whose
    /// way is no one's, the axis's first non-zero component is positive.
    Circle {
        centre: [f64; 3],
        axis: [f64; 3],
        radius: f64,
    },
    /// An arc of the ellipse (or the whole of it) about `centre` in the
    /// plane square to the unit `axis`, with its semi-axes, `major ≥
    /// minor`.
    Ellipse {
        centre: [f64; 3],
        axis: [f64; 3],
        major: f64,
        minor: f64,
    },
    /// Anything else: parabolas, hyperbolas, fitted curves, curves of
    /// several conics.
    Other,
}

impl EdgeShape {
    /// A straight edge's middle, a round edge's centre.
    pub fn point(&self) -> Option<[f64; 3]> {
        match *self {
            EdgeShape::Line { from, to } => Some(std::array::from_fn(|k| (from[k] + to[k]) * 0.5)),
            EdgeShape::Circle { centre, .. } | EdgeShape::Ellipse { centre, .. } => Some(centre),
            EdgeShape::Other => None,
        }
    }
}

/// How far apart, relative to a chain's size, its spans may be from one
/// line, circle or ellipse for it to be named one (with a few roundings
/// of its coordinates more): it names intent, and nothing is decided by
/// it.
pub const SHAPE_SLACK: f64 = 1e-9;

/// A span in the standard form of a conic arc: its ends, its middle
/// control point and the weight `w = w₁/√(w₀w₂)`, ends weighted 1 (the
/// same curve).
#[derive(Clone, Copy, Debug)]
struct Arc {
    p0: DVec3,
    c: DVec3,
    p1: DVec3,
    w: f64,
}

impl Arc {
    fn of(span: &Span3) -> Arc {
        let p = |k: usize| DVec3::new(span[k][0], span[k][1], span[k][2]);
        Arc {
            p0: p(0),
            c: p(1),
            p1: p(2),
            w: span[1][3] / (span[0][3] * span[2][3]).sqrt(),
        }
    }

    /// The arc's points in an order of their own: its ends the lesser
    /// first, then its middle point, as bits ordered as the values (no
    /// NaN reaches a shape: the store's values are finite).
    fn key(&self) -> [Ordered; 9] {
        let (p0, p1) = (self.p0.to_array(), self.p1.to_array());
        let (a, b) = if Ordered::point(p1) < Ordered::point(p0) {
            (p1, p0)
        } else {
            (p0, p1)
        };
        let c = self.c.to_array();
        std::array::from_fn(|i| Ordered([a, b, c][i / 3][i % 3]))
    }

    /// The same arc run the other way.
    fn reversed(self) -> Arc {
        Arc {
            p0: self.p1,
            p1: self.p0,
            ..self
        }
    }

    /// The point at `t`.
    fn eval(&self, t: f64) -> DVec3 {
        let (a, b, c) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t) * self.w, t * t);
        (self.p0 * a + self.c * b + self.p1 * c) / (a + b + c)
    }
}

/// An `f64` ordered by [`f64::total_cmp`].
#[derive(Clone, Copy, Debug, PartialEq)]
struct Ordered(f64);

impl Ordered {
    fn point(p: [f64; 3]) -> [Ordered; 3] {
        p.map(Ordered)
    }
}

impl Eq for Ordered {}

impl PartialOrd for Ordered {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for Ordered {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&o.0)
    }
}

/// The greater of two values, compare and select.
fn fmax(a: f64, b: f64) -> f64 {
    pick(b > a, b, a)
}

/// The largest coordinate magnitude of a point.
fn max_abs(p: DVec3) -> f64 {
    fmax(fmax(p.x.abs(), p.y.abs()), p.z.abs())
}

/// The componentwise least and greatest of two points, compare and
/// select.
fn min_max(lo: DVec3, hi: DVec3, p: DVec3) -> (DVec3, DVec3) {
    let f = |a: [f64; 3], b: [f64; 3], less: bool| {
        DVec3::from_array(std::array::from_fn(|k| {
            pick((b[k] < a[k]) == less && b[k] != a[k], b[k], a[k])
        }))
    };
    (
        f(lo.to_array(), p.to_array(), true),
        f(hi.to_array(), p.to_array(), false),
    )
}

/// What a chain's edges' curves make (module docs); the edges in any
/// order. Edges that aren't one path are [`EdgeShape::Other`].
pub fn edge_shape(brep: &Brep, edges: &[EdgeId]) -> EdgeShape {
    let ends = chain_ends(brep, edges);
    let ends = ends.map(|[a, b]| {
        if brep.pos(b) < brep.pos(a) {
            [b, a]
        } else {
            [a, b]
        }
    });
    let Some(path) = path(brep, edges, ends.map(|[a, _]| a)) else {
        return EdgeShape::Other;
    };
    let arcs: Vec<Arc> = path
        .iter()
        .flat_map(|&(e, rev)| {
            let spans = brep.edge_spans(e).iter().map(Arc::of);
            let arcs: Vec<Arc> = if rev {
                spans.rev().map(Arc::reversed).collect()
            } else {
                spans.collect()
            };
            arcs
        })
        .collect();
    shape_of(&arcs, ends.map(|[a, b]| [brep.pos(a), brep.pos(b)]))
}

/// The edges in their order along the chain, each with whether it runs
/// against the chain: from `start`, else along the first edge given;
/// `None` where they aren't one path.
fn path(brep: &Brep, edges: &[EdgeId], start: Option<VertId>) -> Option<Vec<(EdgeId, bool)>> {
    let &first = edges.first()?;
    let mut at = start.unwrap_or(brep.ends(first)[0]);
    let mut used = vec![false; edges.len()];
    let mut out = Vec::with_capacity(edges.len());
    for _ in 0..edges.len() {
        let (i, &e) = edges
            .iter()
            .enumerate()
            .find(|&(i, &e)| !used[i] && brep.ends(e).contains(&at))?;
        used[i] = true;
        let [a, b] = brep.ends(e);
        let rev = a != at;
        out.push((e, rev));
        at = if rev { a } else { b };
    }
    Some(out)
}

/// The two vertices an open chain's edges end at (each the end of one
/// edge only), `None` for a closed chain or one that isn't a path.
fn chain_ends(brep: &Brep, edges: &[EdgeId]) -> Option<[VertId; 2]> {
    let mut count: Vec<(VertId, u32)> = Vec::new();
    for &e in edges {
        if brep.is_closed(e) {
            return None;
        }
        for v in brep.ends(e) {
            match count.iter_mut().find(|(w, _)| *w == v) {
                Some((_, n)) => *n += 1,
                None => count.push((v, 1)),
            }
        }
    }
    let odd: Vec<VertId> = count
        .iter()
        .filter(|(_, n)| *n == 1)
        .map(|(v, _)| *v)
        .collect();
    match odd[..] {
        [a, b] if count.iter().all(|(_, n)| *n <= 2) => Some([a, b]),
        _ => None,
    }
}

/// The shape of `arcs` (module docs), `ends` the open chain's end
/// points, if it is one.
fn shape_of(arcs: &[Arc], ends: Option<[[f64; 3]; 2]>) -> EdgeShape {
    let points: Vec<DVec3> = arcs.iter().flat_map(|c| [c.p0, c.c, c.p1]).collect();
    let Some(&first) = points.first() else {
        return EdgeShape::Other;
    };
    let (lo, hi) = points
        .iter()
        .fold((first, first), |(lo, hi), &p| min_max(lo, hi, p));
    let size = (hi - lo).length();
    let coords = fmax(max_abs(lo), max_abs(hi));
    let slack = SHAPE_SLACK * size + 64.0 * f64::EPSILON * coords;
    if !(size > 0.0 && size.is_finite()) {
        return EdgeShape::Other;
    }

    // A line: every span straight, every point on the line through the
    // chain's ends.
    if let Some([a, b]) = ends {
        let (a, b) = (DVec3::from_array(a), DVec3::from_array(b));
        if let Some(dir) = (b - a).try_normalize()
            && arcs.iter().all(on_chord)
            && points.iter().all(|&p| {
                let d = p - a;
                (d - dir * d.dot(dir)).length() <= slack
            })
        {
            let (from, to) = if a.to_array() <= b.to_array() {
                (a, b)
            } else {
                (b, a)
            };
            return EdgeShape::Line {
                from: from.to_array(),
                to: to.to_array(),
            };
        }
    }

    // A circle or an ellipse: from the span turning the most (the least
    // weight), and five points of each span on it (five points fix a
    // conic, so each span is then an arc of it).
    // Ties broken by the arcs' points, and the turns summed in the order
    // of their points, so neither depends on the edges' order.
    let Some(reference) = arcs
        .iter()
        .min_by(|a, b| a.w.total_cmp(&b.w).then_with(|| a.key().cmp(&b.key())))
    else {
        return EdgeShape::Other;
    };
    let mut sorted: Vec<&Arc> = arcs.iter().collect();
    sorted.sort_by_key(|a| a.key());
    let axis = sorted
        .iter()
        .fold(DVec3::ZERO, |n, c| n + (c.c - c.p0).cross(c.p1 - c.c));
    let Some(mut axis) = axis.try_normalize() else {
        return EdgeShape::Other;
    };
    if ends.is_none()
        && axis
            .to_array()
            .iter()
            .find(|x| **x != 0.0)
            .is_some_and(|x| *x < 0.0)
    {
        axis = -axis;
    }
    let fits = |on: &dyn Fn(DVec3) -> bool, centre: DVec3| {
        arcs.iter().all(|c| {
            [0.0, 0.25, 0.5, 0.75, 1.0].iter().all(|&t| {
                let p = c.eval(t);
                (p - centre).dot(axis).abs() <= slack && on(p)
            })
        })
    };
    if let Some((centre, radius)) = circle_of(reference)
        && fits(&|p| ((p - centre).length() - radius).abs() <= slack, centre)
    {
        return EdgeShape::Circle {
            centre: centre.to_array(),
            axis: axis.to_array(),
            radius,
        };
    }
    if let Some(ellipse) = Ellipse::of(reference)
        && fits(&|p| ellipse.off(p) <= slack, ellipse.centre)
    {
        return EdgeShape::Ellipse {
            centre: ellipse.centre.to_array(),
            axis: axis.to_array(),
            major: ellipse.major,
            minor: ellipse.minor,
        };
    }
    EdgeShape::Other
}

/// Whether a span is straight: its middle control point on its chord,
/// between its ends, within a few roundings of its coordinates.
fn on_chord(arc: &Arc) -> bool {
    let chord = arc.p1 - arc.p0;
    let to_c = arc.c - arc.p0;
    let coords = fmax(fmax(max_abs(arc.p0), max_abs(arc.c)), max_abs(arc.p1));
    let slack = 64.0 * f64::EPSILON * coords;
    let length2 = chord.length_squared();
    if !(length2 > 0.0 && length2.is_finite()) {
        // Both ends at one point: straight only if the control point is
        // there too.
        return to_c.length() <= slack;
    }
    let along = to_c.dot(chord) / length2;
    let across = (to_c - chord * along).length();
    across <= slack && along * length2.sqrt() >= -slack && (along - 1.0) * length2.sqrt() <= slack
}

/// The circle `arc` is an arc of, as its centre and radius, if it is
/// one: its control point off its chord and equally far from its ends,
/// and its weight the cosine of half the arc's angle (half the chord over
/// that distance), each within `1e-10` of the arc's size plus a few
/// roundings of its coordinates. The centre is where the lines square to
/// the end tangents meet, `c + (m − c)·|c − p0|²/|c − m|²` with `m` the
/// chord's middle, the radius the mean of the distances to the ends.
fn circle_of(arc: &Arc) -> Option<(DVec3, f64)> {
    const SLACK: f64 = 1e-10;
    let (p0, c, p1) = (arc.p0, arc.c, arc.p1);
    let (l0, l1) = ((c - p0).length(), (c - p1).length());
    let half = (p1 - p0).length() * 0.5;
    let m = (p0 + p1) * 0.5;
    let rise = (c - m).length();
    let leg = (l0 + l1) * 0.5;
    let coords = fmax(fmax(max_abs(p0), max_abs(c)), max_abs(p1));
    let slack = SLACK * leg + 64.0 * f64::EPSILON * coords;
    if !(rise > slack && (l0 - l1).abs() <= slack && (arc.w * leg - half).abs() <= slack) {
        return None;
    }
    let centre = c + (m - c) * (leg * leg / (rise * rise));
    let radius = ((centre - p0).length() + (centre - p1).length()) * 0.5;
    (centre.is_finite() && radius.is_finite()).then_some((centre, radius))
}

/// An ellipse a span is an arc of.
struct Ellipse {
    centre: DVec3,
    /// Conjugate semi-diameters: to the arc's middle, and along its chord.
    f: [DVec3; 2],
    major: f64,
    minor: f64,
}

impl Ellipse {
    /// The least `1 − w²` taken: an arc so flat that its ellipse is lost
    /// in rounding is named [`EdgeShape::Other`].
    const MIN_FLATNESS: f64 = 1e-6;

    /// The ellipse `arc` is an arc of, if its weight `w` is under 1. An
    /// affine image of a circle's arc of half-angle `φ`, `w = cos φ`: the
    /// centre is `c + (m − c)/(1 − w²)` (`m` the chord's middle); the
    /// semi-diameter to the arc's middle is `(c − O)·w`, the one along the
    /// chord, conjugate to it, `(p1 − p0)/(2√(1 − w²))`. With `S` the sum
    /// of their squares and `P` the area of the parallelogram they span,
    /// the semi-axes are `(√(S + 2P) ± √(S − 2P))/2`.
    fn of(arc: &Arc) -> Option<Ellipse> {
        let w = arc.w;
        let k = 1.0 - w * w;
        if k.is_nan() || k < Self::MIN_FLATNESS {
            return None;
        }
        let m = (arc.p0 + arc.p1) * 0.5;
        let centre = arc.c + (m - arc.c) / k;
        let f = [(arc.c - centre) * w, (arc.p1 - arc.p0) / (2.0 * k.sqrt())];
        let s = f[0].length_squared() + f[1].length_squared();
        let p = f[0].cross(f[1]).length();
        let d = s - 2.0 * p;
        let (big, small) = ((s + 2.0 * p).sqrt(), pick(d > 0.0, d, 0.0).sqrt());
        let ellipse = Ellipse {
            centre,
            f,
            major: (big + small) * 0.5,
            minor: (big - small) * 0.5,
        };
        (centre.is_finite() && ellipse.minor > 0.0 && ellipse.major.is_finite()).then_some(ellipse)
    }

    /// About how far `p`, in the ellipse's plane, is from it: `p − O` in
    /// the semi-diameters' coordinates `(α, β)`, and how far `√(α² + β²)`
    /// is from 1, times the major semi-axis.
    fn off(&self, p: DVec3) -> f64 {
        let d = p - self.centre;
        let [f0, f1] = self.f;
        let (a, b, c) = (f0.dot(f0), f0.dot(f1), f1.dot(f1));
        let (r0, r1) = (f0.dot(d), f1.dot(d));
        let det = a * c - b * b;
        let (alpha, beta) = ((c * r0 - b * r1) / det, (a * r1 - b * r0) / det);
        let off = ((alpha * alpha + beta * beta).sqrt() - 1.0).abs() * self.major;
        if off.is_finite() { off } else { f64::INFINITY }
    }
}

/// A user face's measures.
#[derive(Clone, Debug, PartialEq)]
pub struct RegionMeasure {
    /// Its area (mm²), certified.
    pub area: Bounded<f64>,
    /// The form its faces were built on, where they share one; `None`
    /// where they don't.
    pub form: Option<FormKey>,
    /// For a flat face bounded by four straight edges, each square to the
    /// next ([`rectangle`]): two sides from one corner.
    pub rectangle: Option<[[f64; 3]; 2]>,
}

/// A user edge's measures.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChainMeasure {
    /// Its length (mm), certified.
    pub length: Bounded<f64>,
    /// Whether it closes on itself.
    pub closed: bool,
    /// What its curves make.
    pub shape: EdgeShape,
}

/// Region `r` of the body's topology `topo` measured, on a budget of
/// `budget`; `None` where `r` names no region.
pub fn region_measure(
    brep: &Brep,
    topo: &Topology,
    r: u32,
    budget: &Budget,
) -> Result<Option<RegionMeasure>, KernelError> {
    let Some(region) = topo.regions().get(r as usize) else {
        return Ok(None);
    };
    let area = faces_area(brep, &region.faces, budget)?;
    let mut keys = region
        .faces
        .iter()
        .map(|&f| brep.face_surface(f).lattice().key());
    let form = keys
        .next()
        .and_then(|first| keys.all(|k| k == first).then(|| first.clone()));
    Ok(Some(RegionMeasure {
        area,
        form,
        rectangle: rectangle(brep, topo, r),
    }))
}

/// Chain `c` of the body's topology `topo` measured, on a budget of
/// `budget`; `None` where `c` names no chain.
pub fn chain_measure(
    brep: &Brep,
    topo: &Topology,
    c: u32,
    budget: &Budget,
) -> Result<Option<ChainMeasure>, KernelError> {
    let Some(chain) = topo.chains().get(c as usize) else {
        return Ok(None);
    };
    Ok(Some(ChainMeasure {
        length: edges_length(brep, &chain.edges, budget)?,
        closed: chain.closed,
        shape: edge_shape(brep, &chain.edges),
    }))
}

/// Under this cosine of the angle between them, two sides of a rectangle
/// are square to each other.
pub const SQUARE: f64 = 1e-6;

/// Two sides from one corner of region `r` of `topo`, if it's a
/// rectangle: flat faces whose chains are four straight ones, none closed
/// or between the region and itself, making one loop, each square to the
/// next. The corner is the first chain's `from` end, the sides to the
/// next corner round the loop and on to the one after. A hole's rim or an
/// edge split where another face meets it makes more chains, and no
/// rectangle.
pub fn rectangle(brep: &Brep, topo: &Topology, r: u32) -> Option<[[f64; 3]; 2]> {
    let region = topo.regions().get(r as usize)?;
    let flat = region
        .faces
        .iter()
        .all(|&f| matches!(brep.face_surface(f).lattice().key(), FormKey::Plane(_)));
    if region.faces.is_empty() || !flat {
        return None;
    }
    let mut sides: Vec<[VertId; 2]> = Vec::new();
    for chain in topo.chains().iter().filter(|c| c.regions.contains(&r)) {
        if sides.len() == 4 || chain.closed || chain.regions[0] == chain.regions[1] {
            return None;
        }
        sides.push(straight(brep, chain)?);
    }
    let &[first, ..] = &sides[..] else {
        return None;
    };
    if sides.len() != 4 {
        return None;
    }
    // The corners in order round the loop, from the first side's start.
    let mut corners = vec![first[0]];
    let mut used = [true, false, false, false];
    let mut at = first[1];
    while at != first[0] {
        if corners.len() == 4 {
            return None;
        }
        corners.push(at);
        let (next, side) = sides
            .iter()
            .enumerate()
            .find(|&(i, side)| !used[i] && side.contains(&at))?;
        used[next] = true;
        at = if side[0] == at { side[1] } else { side[0] };
    }
    if corners.len() != 4 {
        return None;
    }
    let places: Vec<DVec3> = corners
        .iter()
        .map(|&v| DVec3::from_array(brep.pos(v)))
        .collect();
    let sides: Vec<DVec3> = (0..4).map(|i| places[(i + 1) % 4] - places[i]).collect();
    let square = (0..4).all(|i| {
        let (u, w) = (sides[i].try_normalize(), sides[(i + 1) % 4].try_normalize());
        u.zip(w).is_some_and(|(u, w)| u.dot(w).abs() < SQUARE)
    });
    square.then(|| [sides[0].to_array(), sides[1].to_array()])
}

/// A straight chain's end vertices, its `from` end first
/// ([`EdgeShape::Line`]); `None` for another shape.
fn straight(brep: &Brep, chain: &Chain) -> Option<[VertId; 2]> {
    let [a, b] = chain_ends(brep, &chain.edges)?;
    let EdgeShape::Line { from, .. } = edge_shape(brep, &chain.edges) else {
        return None;
    };
    Some(if brep.pos(a) == from { [a, b] } else { [b, a] })
}
