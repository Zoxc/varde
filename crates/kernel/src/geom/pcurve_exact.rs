//! Exact p-curves: a coedge's curve in its face's UV where the trace of
//! the edge's curve on the face's form is a rational function of the
//! curve's own parameter, so the p-curve shares `t` with the curve and
//! is exact up to the rounding of its stored values.
//!
//! **Which pairs.** [`trace`] gives the p-curve of any rational quadratic
//! curve lying on
//! - a **plane**: each span's image under the plane frame's affine map,
//!   weights kept, a **Q** span, or a **linear** one where the span is a
//!   line at a Möbius speed (its middle control point the weighted mean
//!   of its ends, in homogeneous form);
//! - a **circular cylinder**, a **conic-chain cylinder** or a **cone**:
//!   **G** spans. Around the axis (along the profile) the curve and the
//!   lattice's quarter circle (profile span) are two proper rational
//!   quadratic parametrisations of one conic, the curve's projection
//!   along the axis (from the apex), so within one cell `u` is a Möbius
//!   function of `t`; along the axis `v` is an affine function of the
//!   point (the cylinder's height, the cone's height over its cosine:
//!   the slant distance on the cone), so it is rational quadratic over
//!   the curve's own denominator. On a cone the lattice's denominator
//!   cancels in the composition, as it must for a conic to come out.
//!
//! Where the stored `v` values of a span come out equal (a circle about
//! the axis) the span is an **iso** span instead, as a linear span whose
//! two ends share a coordinate is. [`iso`] gives the p-curve of a curve
//! on a coordinate line `fixed = at` the caller knows (a circle about the
//! axis of any form of revolution, a ruling, a meridian of a sphere,
//! torus or revolved conic, a lattice-parallel line of a plane): `at` is
//! stored bit for bit and the other coordinate is a Möbius function of
//! `t` (around the axis, along a meridian circle or profile span, along
//! a ruling at a Möbius speed); along a ruling whose speed is quadratic,
//! a G span whose Möbius coordinate is the constant `at`.
//!
//! **Cells.** A Möbius coordinate is one function of `t` only within one
//! base cell of its lattice (a quarter turn about the axis, a profile
//! span, a quarter of a torus's tube, a hemisphere's meridian), so each
//! span is cut where it crosses a base cell's line. Those lines are
//! lines of a plane section in the lattice's frame: about the axis, the
//! frame's `ξ = 0` and `η = 0`; along a meridian `(ρ, ζ)`, its equator
//! `ζ = 0`, a ring torus's tube top and bottom `ρ = R`; along a profile,
//! the chords of the spans the curve visits, each of which meets the
//! curve's conic only at its two ends. The frame coordinates are exact
//! functions of the stored values ([`Frame`]), so each crossing is the
//! root of an exact quadratic, its knot placed as the tile-line split
//! places one ([`PCurve::split_at_tile_lines`]): the nearest `f64` of `t`,
//! the cut point the curve's exact point there, its crossing coordinate
//! set to the line's value exactly (moved by the curve's speed times half
//! an ulp of `t` at most). A meridian's radial coordinate `ρ` is taken
//! along the direction of `at`, rounded; its cuts are only placed, as
//! every cut's knot, to that rounding. A meridian crossing the axis (over
//! a pole or an apex) has no such coordinate and is refused, as a curve
//! through a cone's apex in [`trace`]; one ending on it may cross it by
//! the rounding of its end, which is accepted only where that end is the
//! lattice's own point on the axis, bit for bit (the pole or apex row's;
//! a vertex there), the crossing then a cut on the axis line.
//!
//! **Values.** A p-curve's point at a knot of the curve, or at a cut, is
//! the surface's analytic inverse of the curve's point there
//! ([`Surface::inverse`]), except a fixed iso coordinate (`at`), a cut's
//! crossing coordinate and a meridian's end on the axis (the lattice's
//! own pole or apex point, bit for bit), which takes the axis line's
//! value: so consecutive spans join bit for bit, and a knot
//! shared with a vertex gets that vertex's inverse. A Möbius coordinate's
//! ratio of weights is fixed by its value at the middle of the piece (the
//! inverse of the curve's exact point there), stored as the weights
//! `1/√μ`, `√μ`; a ratio outside `[2⁻¹², 2¹²]` or not positive (only
//! where the coordinate hardly moves) is clamped or taken as 1, and the
//! seam bound measures the result as it does any p-curve. A quadratic
//! coordinate's middle control value is the exact affine image of the
//! piece's middle control point, its weights the piece's, each rounded
//! once. The chain is then split at the finer levels' tile lines
//! ([`PCurve::split_at_tile_lines`]) and must lie in the surface's
//! rectangle.
//!
//! **Bound.** In exact arithmetic the composition `S∘p` is the curve. The
//! seam bound ([`super::certify`]) is then the rounding: of the tiles, of
//! the curve's control points (off the form by their rounding), of the
//! p-curve's stored values and of the cut knots. On a cone the height
//! carries the curve's distance off the cone into `v` divided by the
//! cosine of the half-angle (the slant along the generator).
//!
//! Construction-time geometry, as the p-curve's splits: not charged; a
//! caller charges the spans made.

use malachite::Rational;
use malachite::base::num::arithmetic::traits::Sign;
use varde_decide::bezier::Dir;

use super::curve::{Curve3D, CurveError, Span3, blossom, hom_exact, local_exact};
use super::form::{FormKey, Span2};
use super::lattice::{CELL, Lattice, Rect, pow2};
use super::pcurve::{Mobius, PCurve, PSpan, Quadratic, zero_knots};
use super::round::{nearest, q};
use super::surface::Surface;

/// The least and greatest ratio of a Möbius coordinate's weights (module
/// docs): their square roots are the weights' range.
const RATIO: [f64; 2] = [1.0 / 4096.0, 4096.0];

/// Why an exact p-curve can't be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExactError {
    /// The face's form has no exact trace of a general curve ([`trace`]
    /// on a sphere, torus or revolved conic, or an affine image of one:
    /// only isos are exact there).
    Form,
    /// The iso's fixed value is not finite.
    At,
    /// Span `span` of the curve runs through the axis where its
    /// coordinate about the axis, or along a meridian, is not defined.
    Axis { span: usize },
    /// The crossings of span `span` with its cells' lines can't be placed
    /// at distinct knots strictly inside it.
    Cut { span: usize },
    /// Span `span` of the p-curve lies outside the surface's rectangle.
    Domain { span: usize },
    /// The p-curve's chain is refused, or its tile-line split fails.
    Curve(CurveError),
}

impl From<CurveError> for ExactError {
    fn from(e: CurveError) -> Self {
        ExactError::Curve(e)
    }
}

/// The p-curve of `curve` on `surface` for a plane, circular or
/// conic-chain cylinder or cone (or an affine image of one), Q or linear
/// spans on a plane, G spans on the others, iso spans where a coordinate
/// comes out constant (module docs). `Form` for another form.
pub fn trace(surface: &Surface, curve: &Curve3D) -> Result<PCurve, ExactError> {
    let ctx = Ctx::new(surface)?;
    let mode = match ctx.shape {
        Shape::Plane => Mode::Plane([
            ctx.linear(Dir::U).expect("a plane's u"),
            ctx.linear(Dir::V).expect("a plane's v"),
        ]),
        Shape::Cylinder | Shape::ConicChain(_) | Shape::Cone(_) => Mode::Graph(
            ctx.cells(Dir::U, None).expect("about the axis"),
            ctx.linear(Dir::V).expect("along the axis"),
        ),
        Shape::Sphere | Shape::Torus { .. } | Shape::Revolved(_) => return Err(ExactError::Form),
    };
    ctx.build(curve, &mode)
}

/// The iso p-curve of `curve` on `surface` along the line `fixed = at`
/// of its global UV, which the caller knows the curve to lie on (module
/// docs): `at` bit for bit, the other coordinate Möbius in `t`; along a
/// ruling or plane line at a quadratic speed, G spans with the constant
/// Möbius coordinate `at`. `At` for an `at` not finite.
pub fn iso(surface: &Surface, curve: &Curve3D, fixed: Dir, at: f64) -> Result<PCurve, ExactError> {
    if !at.is_finite() {
        return Err(ExactError::At);
    }
    let ctx = Ctx::new(surface)?;
    let free = other(fixed);
    let mode = match ctx.linear(free) {
        Some(l) => Mode::IsoLinear { fixed, at, free: l },
        None => {
            // Along a meridian: its radial direction from `at`.
            let towards = match fixed {
                Dir::U => {
                    let column = (at.floor() as i64).clamp(ctx.rect.u.start, ctx.rect.u.end - 1);
                    Some((direction(at * pow2(-(ctx.rect.level.u as i32))), column))
                }
                Dir::V => None,
            };
            let c = ctx
                .cells(free, towards)
                .expect("a coordinate is linear or in cells");
            Mode::IsoCells { fixed, at, free: c }
        }
    };
    ctx.build(curve, &mode)
}

/// The other coordinate.
fn other(d: Dir) -> Dir {
    match d {
        Dir::U => Dir::V,
        Dir::V => Dir::U,
    }
}

/// The index of a coordinate.
fn ix(d: Dir) -> usize {
    match d {
        Dir::U => 0,
        Dir::V => 1,
    }
}

/// The unit direction `(cos φ, sin φ)` in the frame's `ξ`, `η` at `turns`
/// quarter turns of the lattice's angle cells: within cell `k`, the
/// quarter circle's parameter `s` has the half-angle tangent
/// `τ = s/(√2 − (√2 − 1)·s)` (the inverse's Möbius map undone), in `f64`.
fn direction(turns: f64) -> [f64; 2] {
    let k = turns.floor();
    let s = turns - k;
    let r = std::f64::consts::SQRT_2;
    let tau = s / (r - (r - 1.0) * s);
    let d = 1.0 + tau * tau;
    let (c, s) = ((1.0 - tau * tau) / d, 2.0 * tau / d);
    match (k as i64).rem_euclid(4) {
        0 => [c, s],
        1 => [-s, c],
        2 => [-c, -s],
        _ => [s, -c],
    }
}

/// The form a surface's lattice is (an affine image's, its source's).
enum Shape {
    Plane,
    Cylinder,
    ConicChain(Vec<Span2>),
    /// The cone, with its half-angle's cosine.
    Cone(f64),
    Sphere,
    /// The torus, with its major radius; ring or spindle.
    Torus {
        major: f64,
        spindle: bool,
    },
    Revolved(Vec<Span2>),
}

/// The exact affine map from space to the lattice's frame coordinates
/// `(ξ, η, ζ)`, `x = o + ξ·e₁ + η·e₂ + ζ·a` with the stored origin and
/// axes (an affine image's through its stored map first): `F·x + g`.
struct Frame {
    f: [[Rational; 3]; 3],
    g: [Rational; 3],
}

impl Frame {
    fn new(lattice: &Lattice) -> Frame {
        let base = |l: &Lattice| {
            let axes = l.axes().map(|a| a.map(q));
            // The matrix with the axes as columns, inverted.
            let a: [[Rational; 3]; 3] =
                std::array::from_fn(|r| std::array::from_fn(|c| axes[c][r].clone()));
            (l.origin().map(q), invert(&a))
        };
        match lattice.key() {
            FormKey::Affine(k) => {
                let src = lattice
                    .source()
                    .expect("an affine image's lattice has its source's");
                let (o, ai) = base(src);
                let mi = invert(&k.linear().map(|r| r.map(q)));
                let f = mul(&ai, &mi);
                let t = k.shift().map(q);
                let ft = apply(&f, &t);
                let ao = apply(&ai, &o);
                let g = std::array::from_fn(|r| -(&ft[r] + &ao[r]));
                Frame { f, g }
            }
            _ => {
                let (o, ai) = base(lattice);
                let ao = apply(&ai, &o);
                Frame {
                    f: ai,
                    g: ao.map(|x| -x),
                }
            }
        }
    }

    /// The homogeneous point `(X, W)` in frame coordinates, `(F·X + g·W,
    /// W)`.
    fn map(&self, h: &[Rational; 4]) -> [Rational; 4] {
        let x = [h[0].clone(), h[1].clone(), h[2].clone()];
        let fx = apply(&self.f, &x);
        std::array::from_fn(|r| {
            if r == 3 {
                h[3].clone()
            } else {
                &fx[r] + &self.g[r] * &h[3]
            }
        })
    }
}

fn dot(a: &[Rational; 3], b: &[Rational; 3]) -> Rational {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}

fn apply(m: &[[Rational; 3]; 3], x: &[Rational; 3]) -> [Rational; 3] {
    std::array::from_fn(|r| dot(&m[r], x))
}

fn mul(a: &[[Rational; 3]; 3], b: &[[Rational; 3]; 3]) -> [[Rational; 3]; 3] {
    std::array::from_fn(|r| {
        std::array::from_fn(|c| &a[r][0] * &b[0][c] + &a[r][1] * &b[1][c] + &a[r][2] * &b[2][c])
    })
}

/// `m⁻¹` by the adjugate; `m` invertible (a lattice's axes, an affine
/// key's map).
fn invert(m: &[[Rational; 3]; 3]) -> [[Rational; 3]; 3] {
    let minor = |r0: usize, r1: usize, c0: usize, c1: usize| {
        &m[r0][c0] * &m[r1][c1] - &m[r0][c1] * &m[r1][c0]
    };
    // Row `r` of the adjugate: the cofactors of column `r`.
    let adj: [[Rational; 3]; 3] = std::array::from_fn(|r| {
        std::array::from_fn(|c| minor((c + 1) % 3, (c + 2) % 3, (r + 1) % 3, (r + 2) % 3))
    });
    let det = &m[0][0] * &adj[0][0] + &m[0][1] * &adj[1][0] + &m[0][2] * &adj[2][0];
    debug_assert!(det != 0u32, "an invertible frame");
    adj.map(|row| row.map(|x| x / &det))
}

/// A coordinate that is an affine function of the frame coordinates, at
/// the base level: `lin·(ξ, η, ζ) + k`.
struct Linear {
    dir: Dir,
    lin: [Rational; 3],
    k: Rational,
}

/// A coordinate read by the inverse, its base cells meeting on lines of
/// a plane section of the frame (module docs, "Cells").
struct Cells {
    dir: Dir,
    /// The section's coordinates `(p, q)` as linear functions of the
    /// frame coordinates.
    plane: [[Rational; 3]; 2],
    /// The fixed lines `α·p + β·q + γ = 0` the cells meet on.
    lines: Vec<[Rational; 3]>,
    /// A profile in the section: the chords of the spans visited are
    /// cell lines too.
    profile: Option<Vec<Span2>>,
    /// Whether the coordinate turns: base values `x` and `x + 4` the
    /// same point.
    turns: bool,
    /// Whether the section's `p = 0` is the axis: a point there has no
    /// value about the axis (`turns`), and a meridian may not cross it.
    axis: bool,
    /// A meridian's column of tiles (the one holding its `u`).
    column: Option<i64>,
}

/// How each span's p-curve is made.
enum Mode {
    /// Both coordinates affine: Q or linear spans.
    Plane([Linear; 2]),
    /// `u` in cells, `v` affine: G spans.
    Graph(Cells, Linear),
    /// `fixed = at`, the other affine.
    IsoLinear { fixed: Dir, at: f64, free: Linear },
    /// `fixed = at`, the other in cells.
    IsoCells { fixed: Dir, at: f64, free: Cells },
}

impl Mode {
    fn cells(&self) -> Option<&Cells> {
        match self {
            Mode::Graph(c, _) | Mode::IsoCells { free: c, .. } => Some(c),
            _ => None,
        }
    }
}

/// What the construction reads of the surface.
struct Ctx<'a> {
    surface: &'a Surface,
    rect: &'a Rect,
    frame: Frame,
    shape: Shape,
}

/// A point of a piece: its homogeneous point in space, exactly, and
/// whether it is a cut (on a cell line).
struct End {
    h: [Rational; 4],
    cut: bool,
}

impl<'a> Ctx<'a> {
    fn new(surface: &'a Surface) -> Result<Ctx<'a>, ExactError> {
        let lattice = surface.lattice();
        let key = match lattice.key() {
            FormKey::Affine(k) => k.source(),
            k => k,
        };
        // The profile as the tiles hold it (an affine image's, its
        // source's).
        let profile = || {
            lattice
                .source()
                .unwrap_or(lattice)
                .profile()
                .expect("a profile form's lattice holds its spans")
                .to_vec()
        };
        let shape = match key {
            FormKey::Plane(_) => Shape::Plane,
            FormKey::Cylinder(_) => Shape::Cylinder,
            FormKey::ConicCylinder(_) => Shape::ConicChain(profile()),
            FormKey::Cone(k) => Shape::Cone(k.cos()),
            FormKey::Sphere(_) => Shape::Sphere,
            FormKey::Torus(k) => Shape::Torus {
                major: k.major(),
                spindle: k.major() < k.minor(),
            },
            FormKey::Revolved(_) => Shape::Revolved(profile()),
            FormKey::Affine(_) => return Err(ExactError::Form),
        };
        Ok(Ctx {
            surface,
            rect: surface.rect(),
            frame: Frame::new(lattice),
            shape,
        })
    }

    /// The coordinate `d` if it is affine in the frame coordinates.
    fn linear(&self, d: Dir) -> Option<Linear> {
        let z = || Rational::from(0u32);
        let inv = Rational::from(1u32) / q(CELL);
        let half = Rational::from_signeds(1, 2);
        let (lin, k) = match (&self.shape, d) {
            (Shape::Plane, Dir::U) => ([inv, z(), z()], half),
            (Shape::Plane, Dir::V) => ([z(), inv, z()], half),
            (Shape::Cylinder | Shape::ConicChain(_), Dir::V) => ([z(), z(), inv], half),
            (Shape::Cone(cos), Dir::V) => ([z(), z(), inv / q(*cos)], z()),
            _ => return None,
        };
        Some(Linear { dir: d, lin, k })
    }

    /// The coordinate `d` in cells (module docs); along a meridian
    /// (`v` of a sphere, torus or revolved conic) its radial direction
    /// `towards` in the frame's `ξ`, `η`. `None` for an affine coordinate.
    fn cells(&self, d: Dir, towards: Option<([f64; 2], i64)>) -> Option<Cells> {
        let r = |a: i32, b: i32, c: i32| [a, b, c].map(Rational::from);
        let about = || Cells {
            dir: d,
            plane: [r(1, 0, 0), r(0, 1, 0)],
            lines: vec![r(1, 0, 0), r(0, 1, 0)],
            profile: None,
            turns: true,
            axis: true,
            column: None,
        };
        let meridian = |lines: Vec<[Rational; 3]>, profile, turns, axis| {
            let ([c, s], column) = towards.expect("a meridian's direction");
            let [c, s] = [c, s].map(q);
            Cells {
                column: Some(column),
                dir: d,
                plane: [[c, s, Rational::from(0u32)], r(0, 0, 1)],
                lines,
                profile,
                turns,
                axis,
            }
        };
        Some(match (&self.shape, d) {
            (Shape::Plane, _) => return None,
            (Shape::ConicChain(p), Dir::U) => Cells {
                dir: d,
                plane: [r(1, 0, 0), r(0, 1, 0)],
                lines: Vec::new(),
                profile: Some(p.clone()),
                turns: false,
                axis: false,
                column: None,
            },
            (_, Dir::U) => about(),
            (Shape::Cylinder | Shape::ConicChain(_) | Shape::Cone(_), Dir::V) => return None,
            (Shape::Sphere, Dir::V) => meridian(vec![r(0, 1, 0)], None, false, true),
            (Shape::Torus { spindle: true, .. }, Dir::V) => {
                meridian(vec![r(0, 1, 0)], None, false, true)
            }
            (Shape::Torus { major, .. }, Dir::V) => {
                let ring = [Rational::from(1u32), Rational::from(0u32), -q(*major)];
                meridian(vec![r(0, 1, 0), ring], None, true, false)
            }
            (Shape::Revolved(p), Dir::V) => meridian(Vec::new(), Some(p.clone()), false, true),
        })
    }

    /// Whether the stored point `p` is, bit for bit, the lattice's point
    /// on the axis where meridian `c` meets it: the row of the line
    /// nearest its value, in the meridian's column.
    fn on_axis(&self, c: &Cells, p: &[f64; 4]) -> bool {
        let Some(column) = c.column else {
            return false;
        };
        let x = self.value(c.dir, &hom_exact(p)) * self.scale(c.dir);
        if x.is_nan() || x.abs() >= 4_503_599_627_370_496.0 {
            return false;
        }
        let line = x.round() as i64;
        let Some(row) = self.surface.row(Dir::U, line, column) else {
            return false;
        };
        row.iter()
            .all(|r| (0..3).all(|k| r[k].to_bits() == p[k].to_bits()))
    }

    /// The level's scale of coordinate `d`: `2^L`.
    fn scale(&self, d: Dir) -> f64 {
        let l = match d {
            Dir::U => self.rect.level.u,
            Dir::V => self.rect.level.v,
        };
        pow2(l as i32)
    }

    /// The inverse's coordinate `d` of the exact point `h`, at the base
    /// level.
    fn value(&self, d: Dir, h: &[Rational; 4]) -> f64 {
        let x: [f64; 3] = std::array::from_fn(|k| nearest(&(&h[k] / &h[3])));
        match self.surface.inverse(x) {
            Ok(inv) => inv.uv[ix(d)] / self.scale(d),
            Err(_) => f64::NAN,
        }
    }

    /// The section's `(P, Q, W)` of the exact homogeneous point `h`.
    fn section(&self, c: &Cells, h: &[Rational; 4]) -> [Rational; 3] {
        let f = self.frame.map(h);
        let x = [f[0].clone(), f[1].clone(), f[2].clone()];
        [dot(&c.plane[0], &x), dot(&c.plane[1], &x), f[3].clone()]
    }

    /// The value of `l` at the base level of the exact homogeneous point
    /// `h`, scaled to the level and rounded once.
    fn linear_value(&self, l: &Linear, h: &[Rational; 4]) -> f64 {
        let f = self.frame.map(h);
        let x = [f[0].clone(), f[1].clone(), f[2].clone()];
        let v = (dot(&l.lin, &x) + &l.k * &f[3]) / &f[3];
        nearest(&(v * q(self.scale(l.dir))))
    }

    /// The p-curve of `curve` in `mode`.
    fn build(&self, curve: &Curve3D, mode: &Mode) -> Result<PCurve, ExactError> {
        let knots = curve.knots();
        let mut spans = Vec::with_capacity(curve.spans().len());
        let mut out_knots = Vec::with_capacity(knots.len());
        out_knots.push(knots[0]);
        // The cells coordinate's value at the last piece's end, base level:
        // turns are taken continuous along the chain.
        let mut prev = None;
        for (k, span) in curve.spans().iter().enumerate() {
            let (a, b) = (knots[k], knots[k + 1]);
            let h = span.each_ref().map(hom_exact);
            let last = curve.spans().len() - 1;
            let chain = [(k == 0).then(|| span[0]), (k == last).then(|| span[2])];
            let cuts = match mode.cells() {
                Some(c) => self.cuts(c, span, &h, a, b, k, chain)?,
                None => Vec::new(),
            };
            // The pieces' ends: the span's own, and the cuts.
            let mut ends: Vec<(f64, Rational, bool)> = Vec::with_capacity(cuts.len() + 2);
            // A meridian's chain end on the axis takes the axis line's
            // value, as a cut does.
            let pole = |p: &Option<[f64; 4]>| match mode.cells() {
                Some(c) if c.column.is_some() => p.is_some_and(|p| self.on_axis(c, &p)),
                _ => false,
            };
            ends.push((a, Rational::from(0u32), pole(&chain[0])));
            for &t in &cuts {
                ends.push((t, local_exact(t, a, b), true));
            }
            ends.push((b, Rational::from(1u32), pole(&chain[1])));
            for w in ends.windows(2) {
                let ((_, s0, c0), (t1, s1, c1)) = (&w[0], &w[1]);
                let sub = [
                    blossom(&h, s0, s0),
                    blossom(&h, s0, s1),
                    blossom(&h, s1, s1),
                ];
                let mid = {
                    let m = (s0 + s1) / Rational::from(2u32);
                    blossom(&h, &m, &m)
                };
                let [e0, mid1, e1] = sub;
                let piece = Piece {
                    start: End { h: e0, cut: *c0 },
                    middle: mid1,
                    end: End { h: e1, cut: *c1 },
                    point: mid,
                };
                spans.push(self.span(&piece, mode, k, &mut prev)?);
                out_knots.push(*t1);
            }
        }
        let p = PCurve::from_spans(spans, out_knots)?.split_at_tile_lines()?;
        if let Some(span) = p.spans().iter().position(|s| !in_rect(self.rect, s)) {
            return Err(ExactError::Domain { span });
        }
        Ok(p)
    }

    /// The knots where span `k` (`h` its exact homogeneous control
    /// points, over `[a, b]`) crosses its cells' lines, sorted.
    /// `chain` holds the chain's first point if this is its first span,
    /// and its last if its last.
    #[allow(clippy::too_many_arguments)]
    fn cuts(
        &self,
        c: &Cells,
        span: &Span3,
        h: &[[Rational; 4]; 3],
        a: f64,
        b: f64,
        k: usize,
        chain: [Option<[f64; 4]>; 2],
    ) -> Result<Vec<f64>, ExactError> {
        let sec = h.each_ref().map(|p| self.section(c, p));
        let form = |l: &[Rational; 3]| -> Vec<Rational> {
            sec.iter()
                .map(|s| &l[0] * &s[0] + &l[1] * &s[1] + &l[2] * &s[2])
                .collect()
        };
        let knots = |f: &[Rational]| zero_knots(f, a, b).ok_or(ExactError::Cut { span: k });
        let mut ts = Vec::new();
        if c.axis && !c.turns {
            let over = knots(&form(&[1, 0, 0].map(Rational::from)))?;
            if !over.is_empty() {
                // A meridian reaching the axis: only at the chain's end,
                // that end the lattice's own point there (module docs).
                let on = |p: &Option<[f64; 4]>| p.is_some_and(|p| self.on_axis(c, &p));
                if over.len() > 1 || !(on(&chain[0]) || on(&chain[1])) {
                    return Err(ExactError::Axis { span: k });
                }
                ts.extend(over);
            }
        }
        for l in &c.lines {
            ts.extend(knots(&form(l))?);
        }
        if let Some(profile) = &c.profile {
            // The cells visited, from the values at the span's ends.
            let at = |p: &[f64; 4]| self.value(c.dir, &hom_exact(p));
            let (x0, x1) = (at(&span[0]), at(&span[2]));
            let (lo, hi) = if x0 < x1 { (x0, x1) } else { (x1, x0) };
            if !(lo.is_finite() && hi.is_finite()) {
                return Err(ExactError::Cut { span: k });
            }
            let first = lo.floor() as i64;
            let last = hi.ceil() as i64 - 1;
            for m in first.max(0)..last.min(profile.len() as i64) {
                // The chord of span `m`: the crossings at its far end,
                // boundary `m + 1`, belong to it.
                let s = &profile[m as usize];
                let (p0, p2) = ([q(s[0][0]), q(s[0][1])], [q(s[2][0]), q(s[2][1])]);
                let (dx, dy) = (&p2[0] - &p0[0], &p2[1] - &p0[1]);
                let gamma = &dy * &p0[0] - &dx * &p0[1];
                let chord = [-dy, dx, gamma];
                for t in knots(&form(&chord))? {
                    let s = local_exact(t, a, b);
                    let x = self.value(c.dir, &blossom(h, &s, &s));
                    if x.round() == (m + 1) as f64 {
                        ts.push(t);
                    }
                }
            }
        }
        ts.sort_by(f64::total_cmp);
        ts.dedup();
        Ok(ts)
    }

    /// The span of one piece in `mode`.
    fn span(
        &self,
        piece: &Piece,
        mode: &Mode,
        k: usize,
        prev: &mut Option<f64>,
    ) -> Result<PSpan, ExactError> {
        Ok(match mode {
            Mode::Plane([lu, lv]) => {
                let [u, v] = [lu, lv].map(|l| self.quadratic(l, piece));
                if piece.straight() {
                    let ends = [0, 2].map(|i| [u[i][0], v[i][0], u[i][1]]);
                    if u[0][0] == u[2][0] {
                        PSpan::Iso {
                            fixed: Dir::U,
                            at: u[0][0],
                            m: [v[0], v[2]],
                        }
                    } else if v[0][0] == v[2][0] {
                        PSpan::Iso {
                            fixed: Dir::V,
                            at: v[0][0],
                            m: [u[0], u[2]],
                        }
                    } else {
                        PSpan::Linear(ends)
                    }
                } else {
                    PSpan::Q(std::array::from_fn(|i| [u[i][0], v[i][0], u[i][1]]))
                }
            }
            Mode::Graph(c, l) => {
                let m = self.mobius(c, piece, k, prev)?;
                let q = self.quadratic(l, piece);
                if q[0][0] == q[1][0] && q[1][0] == q[2][0] {
                    PSpan::Iso {
                        fixed: Dir::V,
                        at: q[0][0],
                        m,
                    }
                } else {
                    PSpan::G {
                        mobius: Dir::U,
                        m,
                        q,
                    }
                }
            }
            Mode::IsoLinear { fixed, at, free } => {
                let q = self.quadratic(free, piece);
                if piece.straight() {
                    PSpan::Iso {
                        fixed: *fixed,
                        at: *at,
                        m: [q[0], q[2]],
                    }
                } else {
                    PSpan::G {
                        mobius: *fixed,
                        m: [[*at, 1.0], [*at, 1.0]],
                        q,
                    }
                }
            }
            Mode::IsoCells { fixed, at, free } => PSpan::Iso {
                fixed: *fixed,
                at: *at,
                m: self.mobius(free, piece, k, prev)?,
            },
        })
    }

    /// The affine coordinate `l` of a piece: its ends' values the
    /// inverse's, its middle the exact image, weights the piece's.
    fn quadratic(&self, l: &Linear, piece: &Piece) -> Quadratic {
        let w = |h: &[Rational; 4]| nearest(&h[3]);
        let end = |e: &End| [self.value(l.dir, &e.h) * self.scale(l.dir), w(&e.h)];
        [
            end(&piece.start),
            [self.linear_value(l, &piece.middle), w(&piece.middle)],
            end(&piece.end),
        ]
    }

    /// The Möbius coordinate `c` of a piece of span `k` (module docs),
    /// `prev` the value at the last piece's end (turns continuous along
    /// the chain), set to this one's.
    fn mobius(
        &self,
        c: &Cells,
        piece: &Piece,
        k: usize,
        prev: &mut Option<f64>,
    ) -> Result<Mobius, ExactError> {
        let value = |h: &[Rational; 4]| {
            if c.turns && c.axis {
                let s = self.section(c, h);
                if s[0].sign() == std::cmp::Ordering::Equal
                    && s[1].sign() == std::cmp::Ordering::Equal
                {
                    // On the axis: any angle is the point.
                    return None;
                }
            }
            Some(self.value(c.dir, h))
        };
        let axis = ExactError::Axis { span: k };
        let mut xm = value(&piece.point).ok_or(axis)?;
        let (x0, x1) = (
            value(&piece.start.h).unwrap_or(xm),
            value(&piece.end.h).unwrap_or(xm),
        );
        if !(x0.is_finite() && xm.is_finite() && x1.is_finite()) {
            return Err(axis);
        }
        if c.turns {
            // The turn nearest the last piece's end, or this one's start.
            let near = prev.unwrap_or(x0);
            xm += 4.0 * ((near - xm) / 4.0 + 0.5).floor();
        }
        let cell = xm.floor();
        let local = |x: f64, cut: bool| {
            let mut x = x;
            if c.turns {
                // The turn nearest the middle.
                x += 4.0 * ((xm - x) / 4.0 + 0.5).floor();
            }
            let s = x - cell;
            if cut {
                if s < 0.5 { 0.0 } else { 1.0 }
            } else {
                s.clamp(0.0, 1.0)
            }
        };
        let (s0, s1) = (local(x0, piece.start.cut), local(x1, piece.end.cut));
        let sm = xm - cell;
        let scale = self.scale(c.dir);
        let at = |s: f64| (cell + s) * scale;
        let ratio = (sm - s0) / (s1 - sm);
        let ratio = if ratio.is_nan() || ratio <= 0.0 {
            1.0
        } else {
            ratio.clamp(RATIO[0], RATIO[1])
        };
        let r = ratio.sqrt();
        *prev = Some(cell + s1);
        Ok([[at(s0), 1.0 / r], [at(s1), r]])
    }
}

/// A piece of a curve's span between two of its knots or cuts: its exact
/// homogeneous control points (ends and middle) and its exact point at
/// the middle of its parameter.
struct Piece {
    start: End,
    middle: [Rational; 4],
    end: End,
    point: [Rational; 4],
}

impl Piece {
    /// Whether the piece is a line at a Möbius speed: its middle control
    /// point, homogeneous, the mean of its ends.
    fn straight(&self) -> bool {
        (0..4).all(|k| Rational::from(2u32) * &self.middle[k] == &self.start.h[k] + &self.end.h[k])
    }
}

/// Whether span `s` lies in a tile of `rect`: each coordinate's control
/// values within `[i, i + 1]` for `i` the floor of the least or the one
/// below (a span on the rectangle's far line), inside the rectangle.
fn in_rect(rect: &Rect, s: &PSpan) -> bool {
    let [lo, hi] = s.hull();
    let fits = |k: usize, range: &std::ops::Range<i64>| {
        let least = lo[k];
        // Below 2⁵² in magnitude, so the candidates are exact.
        if least.is_nan() || least.abs() >= 4_503_599_627_370_496.0 {
            return false;
        }
        let c = least.floor() as i64;
        [c, c - 1]
            .into_iter()
            .any(|i| range.contains(&i) && (i as f64) <= lo[k] && hi[k] <= (i + 1) as f64)
    };
    fits(0, &rect.u) && fits(1, &rect.v)
}

#[cfg(test)]
mod tests;
