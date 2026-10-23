//! Per-tile implicits: the polynomial `F` whose zero set holds a tile's
//! form over its cell, at every precision level ([`ImplicitAt`]), for
//! the compositions with a span or patch ([`Implicit`]) and the distance
//! to the form ([`super::certify::implicit`]).
//!
//! **The polynomials**, each in its own frame at `o`, `y = x − o`, with
//! the stored key's and the lattice frame's values (`a` the stored axis,
//! `A = |a|²`, `z = a·y`, `|y|²` the squared length):
//! - plane: `n·x − d`, `o` the world origin (degree 1);
//! - cylinder: `A·|y|² − z² − ρ²·A`, `o` the foot point, the quadric of
//!   the near-quadric leaves, the same bits (degree 2);
//! - cone: `cos²·(A·|y|² − z²) − sin²·z²`, `o` the apex: both nappes, the
//!   apex singular (degree 2);
//! - sphere: `|y|² − ρ²`, `o` the centre (degree 2);
//! - torus: `A·(|y|² + R² − r²)² − 4R²·(A·|y|² − z²)`, `o` the centre:
//!   the whole torus, a spindle's lemon included (degree 4);
//! - conic-chain cylinder, per span: the span's conic `Q(ξ, η)` in the
//!   coordinates along the frame's axes `e₁, e₂, a` (by the dual basis,
//!   `ξ·Δ = (e₂ × a)·y`, `η·Δ = (a × e₁)·y`, `Δ = e₁·(e₂ × a)`, so the
//!   stored axes' rounding moves nothing however far along the axis),
//!   homogenised by `Δ`, `o` the span's middle control point in the
//!   profile's plane, `x₁·e₁ + y₁·e₂` (degree 2; 1 for a straight span);
//! - revolved conic, per span: the span's conic `Q(ρ, z)` in the meridian
//!   turned about the axis, `o` the foot point moved along the axis to the
//!   span's middle control point's height, `foot + z₁·a`, `z = a·y/A` and
//!   `ρ² = |y|² − (a·y)²/A` (the tiles' axial and radial coordinates from
//!   there, whatever the stored axis's length): split as `Q = P(ρ², z) + ρ·L(z)` by the parity
//!   of `ρ`, the surface is `P² − ρ²·L² = 0`, times `A⁴` (degree 4; `P`
//!   alone, times `A²`, degree 2 or 1, where `L` is zero: a conic symmetric
//!   about the axis's normal, or a straight span perpendicular to the
//!   axis);
//! - affine image `x ↦ M·x + t`: its source's `F` at `M⁻¹(x − t)`, times
//!   `det M` to the degree, `o = t + M·o_src`: each term `c·Π yₖ^eₖ` of the
//!   source becomes `c·det^(d − |e|)·Π (adj M·y)ₖ^eₖ` (the source's
//!   degree).
//!
//! The **conic of a span** with control points `P₀, P₁, P₂` and weights
//! `w₀, w₁, w₂`: with `λᵢ` the barycentric coordinates of the point in
//! the triangle `P₀P₁P₂` times its doubled signed area (`λ₀ = (P₁ − X) ×
//! (P₂ − X)` and cyclically, affine in `X`), the span's points satisfy
//! `w₀w₂·λ₁² = 4w₁²·λ₀λ₂` (from `λᵢ ∝ wᵢBᵢ`), so `Q = w₀w₂·λ₁² −
//! 4w₁²·λ₀λ₂`. Where the control points are collinear (decided exactly
//! on the stored values) the span is straight and `Q` the line through
//! its ends, `(X − P₀) × (P₂ − P₀)`; a straight span whose ends coincide
//! has no curve ([`ImplicitError::Degenerate`]). The conic is written
//! about the implicit's origin (the span's points less that point's
//! coordinates), so its coefficients are of the span's size, whatever
//! its distance from the frame's origin: the first level's rounding of
//! the polynomial and of its compositions stays of the span's scale.
//!
//! **Exactness.** Every coefficient is a polynomial in the stored values,
//! formed at the level asked for: exact at the rational level, each
//! operation's rounding carried by `Bounded` at the others. A coefficient
//! of the conic exactly zero (decided on the rational values) is left
//! out at every level, so the degree and the terms present are the same
//! at every level. The polynomial is the one of the tiles' construction
//! from the stored key and frame, before their rounding: a tile's
//! distance from its zero set is its rounding (and, around an axis, the
//! frame's departure from orthonormal and the rounded weight `√½`, a few
//! ulps of the radius and of the distance along the axis), within the
//! level cap's deviation ([`super::lattice`]).
//!
//! **Conditioning.** A revolved span whose conic is symmetric about the
//! axis's normal only to rounding (an ellipse's quarter from a pole, its
//! weight `√½` rounded) has `L` of that rounding: its quartic is nearly
//! the square `P²`, whose gradient vanishes on the surface to that
//! rounding, so the distance's ball condition fails there and
//! compositions have nearly double roots. The polynomial is still the
//! exact one of the stored span.
//!
//! **Per tile.** A form of one piece has one implicit for all its tiles;
//! a conic-chain cylinder's and a revolved conic's are their spans'
//! ([`Lattice::implicit`], [`Surface::implicit`]). Each is a small value
//! built on demand from the key and the cell, the same for every tile of
//! the cell at every level, and formed at a level when used. Ruled
//! surfaces have none (no key).

use std::collections::BTreeMap;

use malachite::Rational;
use varde_decide::bezier::{Implicit, Scalar};
use varde_decide::leaves::near_quadric::Quadric;

use super::certify::implicit::ImplicitAt;
use super::form::{FormKey, Span2};
use super::lattice::Lattice;
use super::round::q;
use super::surface::Surface;

/// Why a cell has no implicit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImplicitError {
    /// A span index outside a conic-chain cylinder's or revolved conic's
    /// profile.
    Index,
    /// A profile span with no curve: straight with coinciding ends, or a
    /// revolved span whose conic has no term but its constant.
    Degenerate,
}

/// The implicit of a tile's form over its cell (module docs): its
/// stored values, formed at a level by [`ImplicitAt::implicit_at`].
#[derive(Clone, Debug, PartialEq)]
pub struct TileImplicit {
    kind: Kind,
    degree: usize,
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Plane {
        normal: [f64; 3],
        offset: f64,
    },
    Cylinder(Quadric),
    Cone {
        apex: [f64; 3],
        axis: [f64; 3],
        cos: f64,
        sin: f64,
    },
    Sphere {
        centre: [f64; 3],
        radius: f64,
    },
    Torus {
        centre: [f64; 3],
        axis: [f64; 3],
        major: f64,
        minor: f64,
    },
    /// A conic-chain cylinder's span along the frame's axes `e₁, e₂, a`.
    Chain {
        axes: [[f64; 3]; 3],
        span: Span2,
        shape: Shape,
    },
    /// A revolved conic's span about the axis through the foot point.
    Revolved {
        foot: [f64; 3],
        axis: [f64; 3],
        span: Span2,
        shape: Shape,
    },
    Affine {
        source: Box<TileImplicit>,
        linear: [[f64; 3]; 3],
        shift: [f64; 3],
    },
}

/// A span's conic about a point of its plane: which of its coefficients
/// `[A, B, C, D, E, F]` (of `X², XY, Y², X, Y, 1`) are exactly zero, and
/// whether it is straight.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Shape {
    straight: bool,
    nonzero: [bool; 6],
    /// The point of the span's plane the conic is written about (its
    /// middle control point's coordinates, or `z` alone in a meridian),
    /// the implicit's origin there: so its coefficients are of the span's
    /// size, not of its distance from the frame's origin.
    shift: [f64; 2],
}

impl TileImplicit {
    /// The degree of the polynomial: 1 to 4.
    #[inline]
    pub fn degree(&self) -> usize {
        self.degree
    }

    /// The implicit of a form of one piece, the same for all its cells:
    /// `None` for a conic-chain cylinder or revolved conic (per span) or
    /// an affine image of one.
    pub fn of_form(key: &FormKey) -> Option<TileImplicit> {
        let (kind, degree) = match key {
            FormKey::Plane(k) => (
                Kind::Plane {
                    normal: k.normal(),
                    offset: k.offset(),
                },
                1,
            ),
            FormKey::Cylinder(k) => (
                Kind::Cylinder(
                    Quadric::cylinder(k.foot(), k.axis(), k.radius())
                        .expect("a stored key's values are finite, its radius positive"),
                ),
                2,
            ),
            FormKey::Cone(k) => (
                Kind::Cone {
                    apex: k.apex(),
                    axis: k.axis(),
                    cos: k.cos(),
                    sin: k.sin(),
                },
                2,
            ),
            FormKey::Sphere(k) => (
                Kind::Sphere {
                    centre: k.centre(),
                    radius: k.radius(),
                },
                2,
            ),
            FormKey::Torus(k) => (
                Kind::Torus {
                    centre: k.centre(),
                    axis: k.axis(),
                    major: k.major(),
                    minor: k.minor(),
                },
                4,
            ),
            FormKey::ConicCylinder(_) | FormKey::Revolved(_) => return None,
            FormKey::Affine(k) => {
                return TileImplicit::of_form(k.source())
                    .map(|s| TileImplicit::affine(s, k.linear(), k.shift()));
            }
        };
        Some(TileImplicit { kind, degree })
    }

    fn affine(source: TileImplicit, linear: [[f64; 3]; 3], shift: [f64; 3]) -> TileImplicit {
        let degree = source.degree;
        TileImplicit {
            kind: Kind::Affine {
                source: Box::new(source),
                linear,
                shift,
            },
            degree,
        }
    }
}

impl Lattice {
    /// The implicit of base cell `(k, l)`'s form (module docs): the
    /// form's own where it is of one piece, else that of the span the
    /// cell lies on (`k` on a conic-chain cylinder, `l` on a revolved
    /// conic); an affine image's its source's, mapped. `Index` for a span
    /// outside the profile, `Degenerate` for a span with no curve.
    pub fn implicit(&self, k: i64, l: i64) -> Result<TileImplicit, ImplicitError> {
        let span = |profile: &super::form::Profile, at: i64| -> Result<Span2, ImplicitError> {
            usize::try_from(at)
                .ok()
                .and_then(|n| profile.spans().get(n))
                .copied()
                .ok_or(ImplicitError::Index)
        };
        match self.key() {
            FormKey::ConicCylinder(c) => {
                let span = span(c.profile(), k)?;
                let shape = shape(&span, [span[1][0], span[1][1]])?;
                Ok(TileImplicit {
                    kind: Kind::Chain {
                        axes: self.axes(),
                        span,
                        shape,
                    },
                    degree: if shape.straight { 1 } else { 2 },
                })
            }
            FormKey::Revolved(r) => {
                let span = span(r.profile(), l)?;
                let shape = shape(&span, [0.0, span[1][1]])?;
                let degree = revolved_degree(&shape)?;
                Ok(TileImplicit {
                    kind: Kind::Revolved {
                        foot: r.foot(),
                        axis: r.axis(),
                        span,
                        shape,
                    },
                    degree,
                })
            }
            FormKey::Affine(a) => {
                let source = self
                    .source()
                    .expect("an affine image's lattice has its source's")
                    .implicit(k, l)?;
                Ok(TileImplicit::affine(source, a.linear(), a.shift()))
            }
            key => Ok(TileImplicit::of_form(key).expect("a form of one piece")),
        }
    }
}

impl Surface {
    /// The implicit of tile `(i, j)`: its base cell's
    /// ([`Lattice::implicit`]); `None` outside the rectangle.
    pub fn implicit(&self, i: i64, j: i64) -> Option<Result<TileImplicit, ImplicitError>> {
        self.index(i, j)?;
        let level = self.rect().level;
        Some(self.lattice().implicit(i >> level.u, j >> level.v))
    }
}

impl ImplicitAt for TileImplicit {
    fn implicit_at<S: Scalar>(&self, like: &S) -> Implicit<S> {
        let (origin, poly) = match &self.kind {
            Kind::Cylinder(c) => return c.implicit_form(like),
            kind => form(kind, like),
        };
        Implicit::new(self.degree, origin, poly.terms)
            .expect("an implicit's terms are within its degree")
    }
}

/// The origin and the polynomial in `y = x − o` of `kind` at `like`'s
/// level (module docs).
fn form<S: Scalar>(kind: &Kind, like: &S) -> ([S; 3], Poly3<S>) {
    let c = |x: f64| S::from_f64(x, like);
    let c3 = |x: &[f64; 3]| x.map(c);
    let zero3 = || [0.0; 3].map(c);
    match kind {
        Kind::Plane { normal, offset } => {
            let p = Poly3::linear(&c3(normal)).add(&Poly3::constant(c(*offset).neg()));
            (zero3(), p)
        }
        Kind::Cylinder(_) => unreachable!("the quadric forms its own"),
        Kind::Cone {
            apex,
            axis,
            cos,
            sin,
        } => {
            let a = c3(axis);
            let (yy, z, aa) = (squared(like), Poly3::linear(&a), sdot(&a, &a));
            let zz = z.mul(&z);
            let (cc, ss) = (c(*cos).mul(&c(*cos)), c(*sin).mul(&c(*sin)));
            let p = yy.scale(&aa).sub(&zz).scale(&cc).sub(&zz.scale(&ss));
            (c3(apex), p)
        }
        Kind::Sphere { centre, radius } => {
            let r = c(*radius);
            let p = squared(like).add(&Poly3::constant(r.mul(&r).neg()));
            (c3(centre), p)
        }
        Kind::Torus {
            centre,
            axis,
            major,
            minor,
        } => {
            let a = c3(axis);
            let (yy, z, aa) = (squared(like), Poly3::linear(&a), sdot(&a, &a));
            let (big, small) = (c(*major), c(*minor));
            let rr = big.mul(&big);
            let k = rr.sub(&small.mul(&small));
            let inner = yy.add(&Poly3::constant(k));
            let four = S::int(4, like).mul(&rr);
            let p = inner
                .mul(&inner)
                .scale(&aa)
                .sub(&yy.scale(&aa).sub(&z.mul(&z)).scale(&four));
            (c3(centre), p)
        }
        Kind::Chain { axes, span, shape } => {
            // `ξ·Δ = (e₂ × a)·y`, `η·Δ = (a × e₁)·y`, `Δ = e₁·(e₂ × a)`:
            // the coordinates along the frame's own axes.
            let [e1, e2, a] = axes.map(|x| c3(&x));
            let (xi, eta) = (
                Poly3::linear(&cross(&e2, &a)),
                Poly3::linear(&cross(&a, &e1)),
            );
            let delta = sdot(&e1, &cross(&e2, &a));
            let [sx, sy] = shape.shift.map(c);
            let origin: [S; 3] = std::array::from_fn(|k| sx.mul(&e1[k]).add(&sy.mul(&e2[k])));
            let q = conic(span, shape, like);
            let pieces = [
                xi.mul(&xi),
                xi.mul(&eta),
                eta.mul(&eta),
                xi.scale(&delta),
                eta.scale(&delta),
                Poly3::constant(delta.mul(&delta)),
            ];
            (origin, combine(&q, &pieces))
        }
        Kind::Revolved {
            foot,
            axis,
            span,
            shape,
        } => {
            // With `Z = a·y = A·z` and `R = A·|y|² − Z² = A·ρ²`:
            // `A²·P = A·R·q₀ + Z²·q₂ + A·Z·q₄ + A²·q₅`, `A·L = Z·q₁ + A·q₃`.
            let q = conic(span, shape, like);
            let a = c3(axis);
            let aa = sdot(&a, &a);
            let sz = c(shape.shift[1]);
            let origin: [S; 3] = std::array::from_fn(|k| c(foot[k]).add(&sz.mul(&a[k])));
            let z = Poly3::linear(&a);
            let zz = z.mul(&z);
            let rr = squared(like).scale(&aa).sub(&zz);
            let p = combine(
                &[q[0].clone(), q[2].clone(), q[4].clone(), q[5].clone()],
                &[
                    rr.scale(&aa),
                    zz,
                    z.scale(&aa),
                    Poly3::constant(aa.mul(&aa)),
                ],
            );
            let l = combine(
                &[q[1].clone(), q[3].clone()],
                &[z, Poly3::constant(aa.clone())],
            );
            let poly = if l.terms.is_empty() {
                p
            } else {
                // `A⁴·F = (A²·P)² − A·R·(A·L)²`.
                p.mul(&p).sub(&rr.mul(&l.mul(&l)).scale(&aa))
            };
            (origin, poly)
        }
        Kind::Affine {
            source,
            linear,
            shift,
        } => {
            let src = source.implicit_at(like);
            let m = linear.map(|row| row.map(c));
            let minor = |r0: usize, r1: usize, c0: usize, c1: usize| {
                m[r0][c0].mul(&m[r1][c1]).sub(&m[r0][c1].mul(&m[r1][c0]))
            };
            // `adj M`, row `r` the cofactors of column `r` (cyclic minors
            // carry their signs).
            let adj: [[S; 3]; 3] = std::array::from_fn(|r| {
                std::array::from_fn(|col| {
                    minor((col + 1) % 3, (col + 2) % 3, (r + 1) % 3, (r + 2) % 3)
                })
            });
            let det = sdot(&m[0], &[0, 1, 2].map(|col| adj[col][0].clone()));
            let o = src.origin();
            let origin: [S; 3] = std::array::from_fn(|r| c(shift[r]).add(&sdot(&m[r], o)));
            let lin: [Poly3<S>; 3] = std::array::from_fn(|k| Poly3::linear(&adj[k]));
            let mut out = Poly3::zero();
            for (e, coef) in src.terms() {
                let mut t = Poly3::constant(coef.clone());
                for _ in 0..e[3] {
                    t = t.scale(&det);
                }
                for (k, l) in lin.iter().enumerate() {
                    for _ in 0..e[k] {
                        t = t.mul(l);
                    }
                }
                out = out.add(&t);
            }
            (origin, out)
        }
    }
}

/// The degree of a revolved span's polynomial (module docs):
/// `max(2·deg P, 2 + 2·deg L)`, or `deg P` where `L` is zero;
/// `Degenerate` where `Q` has no term but its constant.
fn revolved_degree(s: &Shape) -> Result<usize, ImplicitError> {
    let [a, b, c, d, e, _] = s.nonzero;
    let deg_p = if a || c {
        2
    } else if e {
        1
    } else {
        0
    };
    if !(b || d) {
        return if deg_p == 0 {
            Err(ImplicitError::Degenerate)
        } else {
            Ok(deg_p)
        };
    }
    let deg_l = usize::from(b);
    Ok((2 * deg_p).max(2 + 2 * deg_l))
}

/// `Σ cᵢ·pᵢ` over the coefficients present (`None` exactly zero).
fn combine<S: Scalar>(coefs: &[Option<S>], pieces: &[Poly3<S>]) -> Poly3<S> {
    coefs
        .iter()
        .zip(pieces)
        .filter_map(|(c, p)| c.as_ref().map(|c| p.scale(c)))
        .fold(Poly3::zero(), |acc, p| acc.add(&p))
}

/// The span's shape (module docs), decided exactly on its stored values;
/// `Degenerate` for a straight span whose ends coincide.
fn shape(span: &Span2, shift: [f64; 2]) -> Result<Shape, ImplicitError> {
    let straight = straight(span);
    if straight && span[0][..2] == span[2][..2] {
        return Err(ImplicitError::Degenerate);
    }
    let mut s = Shape {
        straight,
        nonzero: [true; 6],
        shift,
    };
    let exact = conic(span, &s, &Rational::from(0u32));
    s.nonzero = std::array::from_fn(|k| exact[k].as_ref().is_some_and(|x| *x != 0u32));
    Ok(s)
}

/// Whether the span's control points are collinear (a straight span),
/// decided exactly on its stored values.
fn straight(span: &Span2) -> bool {
    let pts = span.map(|p| [q(p[0]), q(p[1])]);
    let cr = |a: &[Rational; 2], b: &[Rational; 2]| &a[0] * &b[1] - &a[1] * &b[0];
    let sub = |a: &[Rational; 2], b: &[Rational; 2]| [&a[0] - &b[0], &a[1] - &b[1]];
    cr(&sub(&pts[1], &pts[0]), &sub(&pts[2], &pts[0])) == 0u32
}

/// The span's conic `[A, B, C, D, E, F]` about `shape`'s point at
/// `like`'s level (module docs), the coefficients `shape` has as zero
/// left out.
fn conic<S: Scalar>(span: &Span2, shape: &Shape, like: &S) -> [Option<S>; 6] {
    let c = |x: f64| S::from_f64(x, like);
    let [sx, sy] = shape.shift.map(c);
    let p = span.map(|x| [c(x[0]).sub(&sx), c(x[1]).sub(&sy)]);
    let sub = |a: &[S; 2], b: &[S; 2]| [a[0].sub(&b[0]), a[1].sub(&b[1])];
    let cr = |a: &[S; 2], b: &[S; 2]| a[0].mul(&b[1]).sub(&a[1].mul(&b[0]));
    // `(X − Pᵢ) × (Pⱼ − Pᵢ)`-style forms `α·X + β·Y + γ`: `X × d + γ`.
    let line = |d: [S; 2], g: S| [d[1].clone(), d[0].neg(), g];
    let q: [S; 6] = if shape.straight {
        let d = sub(&p[2], &p[0]);
        let g = cr(&p[0], &d).neg();
        let [a, b, g] = line(d, g);
        let z = S::int(0, like);
        [z.clone(), z.clone(), z, a, b, g]
    } else {
        let l0 = line(sub(&p[1], &p[2]), cr(&p[1], &p[2]));
        let l1 = line(sub(&p[2], &p[0]), cr(&p[2], &p[0]));
        let l2 = line(sub(&p[0], &p[1]), cr(&p[0], &p[1]));
        let [w0, w1, w2] = span.map(|x| c(x[2]));
        let k1 = w0.mul(&w2);
        let k0 = S::int(4, like).mul(&w1.mul(&w1));
        let a = product(&l1, &l1);
        let b = product(&l0, &l2);
        std::array::from_fn(|n| k1.mul(&a[n]).sub(&k0.mul(&b[n])))
    };
    let mut q = q.map(Some);
    for (x, keep) in q.iter_mut().zip(shape.nonzero) {
        if !keep {
            *x = None;
        }
    }
    q
}

/// The product of two linear forms `α·X + β·Y + γ` as `[A, …, F]`.
fn product<S: Scalar>(l: &[S; 3], m: &[S; 3]) -> [S; 6] {
    let cross = |i: usize, j: usize| l[i].mul(&m[j]).add(&l[j].mul(&m[i]));
    [
        l[0].mul(&m[0]),
        cross(0, 1),
        l[1].mul(&m[1]),
        cross(0, 2),
        cross(1, 2),
        l[2].mul(&m[2]),
    ]
}

/// `|y|²`.
fn squared<S: Scalar>(like: &S) -> Poly3<S> {
    let one = S::int(1, like);
    Poly3 {
        terms: [[2, 0, 0], [0, 2, 0], [0, 0, 2]]
            .into_iter()
            .map(|e| (e, one.clone()))
            .collect(),
    }
}

fn cross<S: Scalar>(a: &[S; 3], b: &[S; 3]) -> [S; 3] {
    [
        a[1].mul(&b[2]).sub(&a[2].mul(&b[1])),
        a[2].mul(&b[0]).sub(&a[0].mul(&b[2])),
        a[0].mul(&b[1]).sub(&a[1].mul(&b[0])),
    ]
}

fn sdot<S: Scalar>(a: &[S; 3], b: &[S; 3]) -> S {
    a[0].mul(&b[0]).add(&a[1].mul(&b[1])).add(&a[2].mul(&b[2]))
}

/// A polynomial in `y`: its terms by exponent, in increasing order, each
/// operation's sums taken in that order.
#[derive(Clone, Debug)]
struct Poly3<S> {
    terms: BTreeMap<[usize; 3], S>,
}

impl<S: Scalar> Poly3<S> {
    fn zero() -> Self {
        Poly3 {
            terms: BTreeMap::new(),
        }
    }

    fn constant(c: S) -> Self {
        Poly3 {
            terms: BTreeMap::from([([0; 3], c)]),
        }
    }

    /// `a·y`.
    fn linear(a: &[S; 3]) -> Self {
        Poly3 {
            terms: [[1, 0, 0], [0, 1, 0], [0, 0, 1]]
                .into_iter()
                .zip(a.iter().cloned())
                .collect(),
        }
    }

    fn put(terms: &mut BTreeMap<[usize; 3], S>, e: [usize; 3], c: S) {
        match terms.get_mut(&e) {
            Some(s) => *s = s.add(&c),
            None => {
                terms.insert(e, c);
            }
        }
    }

    fn add(&self, o: &Self) -> Self {
        let mut terms = self.terms.clone();
        for (e, c) in &o.terms {
            Self::put(&mut terms, *e, c.clone());
        }
        Poly3 { terms }
    }

    fn sub(&self, o: &Self) -> Self {
        let mut terms = self.terms.clone();
        for (e, c) in &o.terms {
            Self::put(&mut terms, *e, c.neg());
        }
        Poly3 { terms }
    }

    fn scale(&self, c: &S) -> Self {
        Poly3 {
            terms: self.terms.iter().map(|(e, x)| (*e, c.mul(x))).collect(),
        }
    }

    fn mul(&self, o: &Self) -> Self {
        let mut terms = BTreeMap::new();
        for (ea, ca) in &self.terms {
            for (eb, cb) in &o.terms {
                let e = [ea[0] + eb[0], ea[1] + eb[1], ea[2] + eb[2]];
                Self::put(&mut terms, e, ca.mul(cb));
            }
        }
        Poly3 { terms }
    }
}

#[cfg(test)]
mod tests;
