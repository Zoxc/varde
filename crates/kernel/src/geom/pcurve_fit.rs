//! Fitted p-curves: a coedge's curve in its face's global UV fitted to
//! its edge's curve `C` with the **same `t`** ([`fit_pcurve`]), for the
//! pairs no exact p-curve covers (an oblique plane on a sphere, a plane
//! or quadric on a torus, quadric pairs, the rest). The certificate is
//! the coedge's seam bound ([`super::certify`]).
//!
//! **Knots and their points.** The fit runs over `[t₀, t₁]` of `C`'s
//! parameter. Its first knots are `t₀`, `C`'s knots strictly inside and
//! `t₁`, so no span crosses a kink of `C`. A knot's UV is the analytic
//! inverse of `C`'s point there ([`Surface::inverse`]), at `C`'s knots
//! its control point exactly; at `t₀` and `t₁` the caller's end UVs
//! where it gives them (the vertices' UVs), kept to the bit. Each knot's
//! UV is computed once and is the end control point of both spans
//! meeting there, so the chain joins bit for bit. A knot whose UV lies
//! outside the surface's domain (the curve leaves it) refuses the fit.
//!
//! **Crossings at a knot.** Where the curve crosses a tile line within an
//! ulp of a knot `t` (the inverse's coordinate at `t` and at a
//! neighbouring `f64` of `t` on the curve have one integer between them,
//! or on either), the knot's UV takes that integer in that coordinate:
//! no knot can be placed between the crossing and `t`, so the tile-line
//! split couldn't break a span ending there with its end left across the
//! line by the inverse's rounding (a vertex on a tile line, a circle
//! about a lattice row). The UV moves by that rounding; the seam bound
//! certifies the result as ever. Given end UVs are kept as they are.
//!
//! **A span.** Over `[a, b]`, with the end UVs `P₀`, `P₂` fixed, the
//! span is quadratic with weights 1 (a linear least-squares fit), its middle
//! control point `P₁` the least-squares fit to the samples `S⁻¹(C(t_k))`
//! at [`SAMPLES`] parameters `t_k = a + (b − a)·k/(SAMPLES + 1)`: with
//! the local parameter `s_k = (t_k − a)/(b − a)`, the seam bound's own,
//! `P₁ = Σ φ_k·r_k / Σ φ_k²`, `φ_k = 2s_k(1 − s_k)` and `r_k` the sample
//! less `(1 − s_k)²P₀ + s_k²P₂`, summed in `k` order. `P₁` is then moved
//! into the domain's rectangle, coordinate by coordinate (a curve inside
//! the convex domain has its span inside too). The span is broken at the
//! tile lines it crosses ([`PCurve::split_at_tile_lines`]) and its seam
//! bound computed towards the target ([`seam_bound`]).
//!
//! **Refinement.** A span is kept once its bound is at most the target,
//! or once the target is **out of reach**: the greatest certified lower
//! bound `L` of `|S(uv) − C(t)|` at the knots made so far is above the
//! target and the span's bound is within [`TIGHT`] of `L`. Knots are never
//! removed, so no refinement takes the deviation at a knot below `L`, and
//! the coedge's bound is within `TIGHT` of its greatest deviation (the
//! seam bound's own rule; the seam bound is then asked for `L`, not the
//! target). Otherwise the span's `[a, b]` is halved at `a/2 + b/2`, a new
//! knot, and both halves fitted again, depth first in `t` order, so the
//! result depends on the input alone. A span whose pieces can't be
//! certified on one tile each ([`SeamError::Tile`]: a middle point
//! pushed over the domain's side) is halved the same way.
//!
//! **An accuracy item.** At [`MAX_FIT_DEPTH`] halvings of a first span,
//! or where no `f64` lies between `a` and `b`, the span is kept with its
//! bound, above the target: the result is always a certified bound, the
//! caller stores it and warns where it is above the target. A span with
//! no certified bound there refuses the fit ([`FitError::Tile`]); running
//! out of budget, or the seam bound's own caps, stop it.
//!
//! **Charges.** Before it is computed: each knot's and sample's point
//! (and a computed knot's two neighbours'), the curve's point, its
//! inverse and the knot's lower bound (`UNITS.fit.point`); each span
//! fitted (`UNITS.fit.span`); each tile line strictly inside its control
//! values' range in either coordinate, the lines its split looks for
//! crossings of (`UNITS.fit.line`); its seam bound as that charges its
//! cells. All ordinary work but the seam bound's precision steps.
//!
//! Near a pole or at a speed mismatch the seam bound overestimates the
//! deviation in space (it is parametric), so such spans refine further
//! than the distance needs.

use varde_decide::num::bounded::{Bounded, down};

use super::certify::{SeamBound, SeamError, TIGHT, seam_bound};
use super::curve::Curve3D;
use super::pcurve::{PCurve, PSpan};
use super::surface::Surface;
use crate::infra::budget::{Ledger, Step, Stop, fit_line_units, fit_point_units, fit_span_units};

/// The samples of the curve inside each span fitted.
pub const SAMPLES: usize = 7;

/// The most halvings of one of the fit's first spans (module docs,
/// "An accuracy item").
pub const MAX_FIT_DEPTH: u32 = 24;

/// A fitted p-curve and its certified seam bound.
#[derive(Clone, Debug, PartialEq)]
pub struct Fitted {
    /// The p-curve over `[t₀, t₁]`, every span in one tile.
    pub pcurve: PCurve,
    /// Its seam bound against the curve: at most the target unless the
    /// target is out of reach (within [`TIGHT`] of [`lower`](Self::lower))
    /// or a span was kept at [`MAX_FIT_DEPTH`].
    pub seam: SeamBound,
    /// The greatest certified lower bound of `|S(uv) − C(t)|` at the
    /// knots: no p-curve through these knot UVs has a smaller seam.
    pub lower: f64,
}

/// Why a p-curve can't be fitted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FitError {
    /// The range isn't `t₀ < t₁` inside the curve's knots.
    Range,
    /// The UV at knot `t` (given, or the inverse's) isn't in the
    /// surface's domain.
    Outside {
        /// The knot.
        t: f64,
    },
    /// The span over `[t0, t1]`, at its last halving, can't be cut into
    /// spans in one tile each of the domain.
    Tile {
        /// The span's start.
        t0: f64,
        /// The span's end.
        t1: f64,
    },
}

/// A knot of the fit: its parameter and UV.
#[derive(Clone, Copy)]
struct Knot {
    t: f64,
    uv: [f64; 2],
}

/// A span to fit: between two knots, after `depth` halvings.
#[derive(Clone, Copy)]
struct Todo {
    a: Knot,
    b: Knot,
    depth: u32,
}

/// The p-curve of `curve` on `surface` over `range` with the same `t`,
/// fitted and refined towards the seam `target` (module docs); its ends
/// the UVs `ends` where given, else the inverse's. Each step charged to
/// `ledger` before it runs. A target below 0, or a NaN, counts as 0.
pub(crate) fn fit_pcurve(
    surface: &Surface,
    curve: &Curve3D,
    range: [f64; 2],
    ends: Option<[[f64; 2]; 2]>,
    target: f64,
    ledger: &mut Ledger<'_>,
) -> Result<Fitted, Stop<FitError>> {
    let ck = curve.knots();
    let [t0, t1] = range;
    // False for a NaN.
    if !(ck[0] <= t0 && t0 < t1 && t1 <= ck[ck.len() - 1]) {
        return Err(Stop::Failed(FitError::Range));
    }
    let target = if target > 0.0 { target } else { 0.0 };
    let mut fit = Fit {
        surface,
        curve,
        target,
        lower: 0.0,
    };
    // The first knots: the range's ends and the curve's knots inside.
    let ts: Vec<f64> = std::iter::once(t0)
        .chain(ck.iter().copied().filter(|&t| t0 < t && t < t1))
        .chain(std::iter::once(t1))
        .collect();
    let mut knots = Vec::with_capacity(ts.len());
    for (k, &t) in ts.iter().enumerate() {
        let given = match (k, ends) {
            (0, Some([e, _])) => Some(e),
            (k, Some([_, e])) if k + 1 == ts.len() => Some(e),
            _ => None,
        };
        knots.push(fit.knot(t, given, ledger)?);
    }
    // A stack: the next span last.
    let mut todo: Vec<Todo> = knots
        .windows(2)
        .rev()
        .map(|w| Todo {
            a: w[0],
            b: w[1],
            depth: 0,
        })
        .collect();
    let mut spans: Vec<PSpan> = Vec::new();
    let mut out_knots = vec![t0];
    let mut seam = SeamBound {
        bound: 0.0,
        level: 0,
    };
    while let Some(span) = todo.pop() {
        let tried = fit.span(&span, ledger)?;
        let kept = match &tried {
            Some((_, b)) => fit.met(b.bound),
            None => false,
        };
        let tm = span.a.t / 2.0 + span.b.t / 2.0;
        let can_halve = span.depth < MAX_FIT_DEPTH && span.a.t < tm && tm < span.b.t;
        if !kept && can_halve {
            let m = fit.knot(tm, None, ledger)?;
            let depth = span.depth + 1;
            todo.push(Todo {
                a: m,
                b: span.b,
                depth,
            });
            todo.push(Todo {
                a: span.a,
                b: m,
                depth,
            });
            continue;
        }
        let Some((pc, b)) = tried else {
            return Err(Stop::Failed(FitError::Tile {
                t0: span.a.t,
                t1: span.b.t,
            }));
        };
        if b.bound > seam.bound {
            seam.bound = b.bound;
        }
        if b.level > seam.level {
            seam.level = b.level;
        }
        spans.extend_from_slice(pc.spans());
        out_knots.extend_from_slice(&pc.knots()[1..]);
    }
    let pcurve = PCurve::from_spans(spans, out_knots).map_err(|_| {
        debug_assert!(false, "fitted spans join at their knots' UVs");
        Stop::Failed(FitError::Tile { t0, t1 })
    })?;
    Ok(Fitted {
        pcurve,
        seam,
        lower: fit.lower,
    })
}

/// A fit in progress: its inputs and the greatest certified lower bound
/// of the deviation at its knots so far.
struct Fit<'a> {
    surface: &'a Surface,
    curve: &'a Curve3D,
    target: f64,
    lower: f64,
}

impl Fit<'_> {
    /// Whether a span of seam bound `bound` is kept (module docs,
    /// "Refinement").
    fn met(&self, bound: f64) -> bool {
        bound <= self.target || (self.lower > self.target && bound <= TIGHT * self.lower)
    }

    /// The target the seam bound is asked for: the greatest lower bound
    /// at the knots where that is above the target.
    fn seam_target(&self) -> f64 {
        if self.lower > self.target {
            self.lower
        } else {
            self.target
        }
    }

    /// The curve's point at `t`, charged, certified.
    fn point(&self, t: f64, ledger: &mut Ledger<'_>) -> Result<[Bounded<f64>; 3], Stop<FitError>> {
        ledger.charge(Step::Work, fit_point_units())?;
        // `t` lies in the curve's range: the fit's range is inside it.
        self.curve.eval(t).ok_or(Stop::Failed(FitError::Range))
    }

    /// The knot at `t`, its UV `given` or the inverse's, its lower bound
    /// counted.
    fn knot(
        &mut self,
        t: f64,
        given: Option<[f64; 2]>,
        ledger: &mut Ledger<'_>,
    ) -> Result<Knot, Stop<FitError>> {
        let x = self.point(t, ledger)?;
        let uv = match given {
            Some(uv) => uv,
            None => self.on_lines(t, self.inverse(&x), ledger)?,
        };
        let outside = Stop::Failed(FitError::Outside { t });
        let s = self.surface.eval(uv).ok_or(outside)?;
        let l = lower(&s, &x);
        if l > self.lower {
            self.lower = l;
        }
        Ok(Knot { t, uv })
    }

    /// `uv`, the inverse's at `t`, with each coordinate that has one
    /// integer between it and the inverse's at a neighbouring `f64` of
    /// `t` on the curve (either included) set to that integer (module
    /// docs, "Crossings at a knot").
    fn on_lines(
        &self,
        t: f64,
        mut uv: [f64; 2],
        ledger: &mut Ledger<'_>,
    ) -> Result<[f64; 2], Stop<FitError>> {
        let ck = self.curve.knots();
        for n in [t.next_down(), t.next_up()] {
            if !(ck[0] <= n && n <= ck[ck.len() - 1]) {
                continue;
            }
            let near = self.inverse(&self.point(n, ledger)?);
            for c in 0..2 {
                let (x, y) = (uv[c], near[c]);
                let (lo, hi) = if x < y { (x, y) } else { (y, x) };
                // The one line in `[lo, hi]`, if there is one; none for a
                // NaN, an infinity or beyond `2⁵²`.
                let line = lo.ceil();
                if line <= hi && hi < line + 1.0 && line.abs() < 4_503_599_627_370_496.0 {
                    uv[c] = line;
                }
            }
        }
        Ok(uv)
    }

    /// The UV of the point `x` by the analytic inverse (a NaN where it has
    /// none: the curve's points are finite, so never).
    fn inverse(&self, x: &[Bounded<f64>; 3]) -> [f64; 2] {
        self.surface
            .inverse(x.map(|c| c.v))
            .map_or([f64::NAN; 2], |i| i.uv)
    }

    /// The span fitted between `span`'s knots, broken at tile lines, and
    /// its seam bound; `None` where it can't be put on the domain's tiles
    /// (module docs).
    fn span(
        &self,
        span: &Todo,
        ledger: &mut Ledger<'_>,
    ) -> Result<Option<(PCurve, SeamBound)>, Stop<FitError>> {
        let Some(pc) = self.fitted(span, ledger)? else {
            return Ok(None);
        };
        match seam_bound(self.surface, self.curve, &pc, self.seam_target(), ledger) {
            Ok(b) => Ok(Some((pc, b))),
            Err(Stop::Failed(SeamError::Tile { .. })) => Ok(None),
            Err(Stop::Failed(SeamError::Range)) => Err(Stop::Failed(FitError::Range)),
            Err(Stop::Exhausted(e)) => Err(Stop::Exhausted(e)),
            Err(Stop::Cap(c)) => Err(Stop::Cap(c)),
        }
    }

    /// The span fitted between `span`'s knots and broken at tile lines
    /// (module docs, "A span"); `None` where the split fails.
    fn fitted(
        &self,
        span: &Todo,
        ledger: &mut Ledger<'_>,
    ) -> Result<Option<PCurve>, Stop<FitError>> {
        let q = self.middle(span, ledger)?;
        // The cuts the tile-line split looks for, charged before it.
        let lines = crossed(&q).min(1 << 20);
        ledger.charge(Step::Work, fit_line_units().saturating_mul(lines))?;
        Ok(PCurve::from_spans(vec![q], vec![span.a.t, span.b.t])
            .and_then(|p| p.split_at_tile_lines())
            .ok())
    }

    /// The Q span between `span`'s knots, its middle control point fitted
    /// to the samples (module docs, "A span").
    fn middle(&self, span: &Todo, ledger: &mut Ledger<'_>) -> Result<PSpan, Stop<FitError>> {
        let (a, b) = (span.a, span.b);
        ledger.charge(Step::Work, fit_span_units())?;
        let mut num = [0.0; 2];
        let mut den = 0.0;
        for k in 1..=SAMPLES {
            let t = a.t + (b.t - a.t) * (k as f64 / (SAMPLES + 1) as f64);
            if !(a.t < t && t < b.t) {
                continue;
            }
            let y = self.inverse(&self.point(t, ledger)?);
            let s = (t - a.t) / (b.t - a.t);
            let (m, phi) = ((1.0 - s) * (1.0 - s), 2.0 * s * (1.0 - s));
            for c in 0..2 {
                let r = y[c] - m * a.uv[c] - s * s * b.uv[c];
                num[c] += phi * r;
            }
            den += phi * phi;
        }
        let rect = self.surface.rect();
        let bounds = [
            [rect.u.start as f64, rect.u.end as f64],
            [rect.v.start as f64, rect.v.end as f64],
        ];
        let middle: [f64; 2] = std::array::from_fn(|c| {
            let p = if den > 0.0 {
                num[c] / den
            } else {
                a.uv[c] / 2.0 + b.uv[c] / 2.0
            };
            // Into the domain; a NaN to its middle.
            let [lo, hi] = bounds[c];
            if p >= lo && p <= hi {
                p
            } else if p < lo {
                lo
            } else if p > hi {
                hi
            } else {
                lo / 2.0 + hi / 2.0
            }
        });
        Ok(PSpan::Q([
            [a.uv[0], a.uv[1], 1.0],
            [middle[0], middle[1], 1.0],
            [b.uv[0], b.uv[1], 1.0],
        ]))
    }
}

/// The tile lines strictly inside the span's control values' range, in
/// either coordinate: the lines its tile-line split looks for crossings
/// of.
fn crossed(q: &PSpan) -> u64 {
    q.coords()
        .iter()
        .map(|c| {
            let [lo, hi] = c.range();
            let n = hi.ceil() - lo.floor() - 1.0;
            // 0 for a NaN.
            if n > 0.0 { n as u64 } else { 0 }
        })
        .sum()
}

/// A certified lower bound of `|s − x|`, each coordinate's difference
/// less its error, rounded down; 0 where none is certified.
fn lower(s: &[Bounded<f64>; 3], x: &[Bounded<f64>; 3]) -> f64 {
    let mut sum = 0.0;
    for k in 0..3 {
        let d = s[k] - x[k];
        if !d.is_certified() {
            return 0.0;
        }
        let l = down0(d.v.abs() - d.err());
        sum = down0(sum + down0(l * l));
    }
    down0(sum.sqrt())
}

/// `x` rounded down, at least 0.
fn down0(x: f64) -> f64 {
    let d = down(x);
    if d > 0.0 { d } else { 0.0 }
}

/// Hooks for the unit table's calibration benchmark
/// (`crates/kernel/benches/calibrate.rs`), which times a fit's points and
/// spans against the units charged for them, and the fixtures it shares
/// with this module's tests. Not part of the kernel's interface.
#[doc(hidden)]
pub mod calibration {
    use super::*;
    use crate::geom::conic::Conic;
    use crate::geom::form::{FormKey, SphereKey};
    use crate::geom::lattice::Lattice;
    use crate::infra::budget::{Budget, MAX_WORK, Pass, Work};
    use varde_decide::num::trig::sin_cos;

    /// Runs `f` with a ledger of a pass on `MAX_WORK`.
    fn with_ledger<R>(f: impl Fn(&mut Ledger<'_>) -> R + Sync) -> R
    where
        R: Send,
    {
        let mut work = Work::new(&Budget::new(MAX_WORK));
        let pass = Pass {
            stage: "fit",
            rung: 0,
        };
        let out = work.pass(pass, &[()], |_, ledger| Ok::<_, Stop<FitError>>(f(ledger)));
        match out {
            Ok(mut v) => v.remove(0),
            Err(_) => unreachable!("a calibration step fits in MAX_WORK"),
        }
    }

    /// The knots of `curve` on `surface` at `ts` (each a point of the
    /// fit, as [`fit_pcurve`] makes them); their number.
    pub fn points(surface: &Surface, curve: &Curve3D, ts: &[f64]) -> u64 {
        with_ledger(|ledger| {
            let mut fit = Fit {
                surface,
                curve,
                target: 0.0,
                lower: 0.0,
            };
            for &t in ts {
                std::hint::black_box(fit.knot(t, None, ledger).is_ok());
            }
            ts.len() as u64
        })
    }

    /// Each span of `curve` on `surface` fitted once between its knots, as
    /// [`fit_pcurve`] fits a span, without its seam bound; their number.
    pub fn spans(surface: &Surface, curve: &Curve3D) -> u64 {
        with_ledger(|ledger| {
            let mut fit = Fit {
                surface,
                curve,
                target: 0.0,
                lower: 0.0,
            };
            let ck = curve.knots();
            let knots: Vec<Knot> = ck
                .iter()
                .map(|&t| fit.knot(t, None, ledger).expect("a knot"))
                .collect();
            for w in knots.windows(2) {
                let span = Todo {
                    a: w[0],
                    b: w[1],
                    depth: 0,
                };
                std::hint::black_box(fit.fitted(&span, ledger).is_ok());
            }
            (ck.len() - 1) as u64
        })
    }

    /// The tile lines strictly inside the control values of the spans
    /// [`spans`] fits, which it charges as lines.
    pub fn lines(surface: &Surface, curve: &Curve3D) -> u64 {
        with_ledger(|ledger| {
            let mut fit = Fit {
                surface,
                curve,
                target: 0.0,
                lower: 0.0,
            };
            let knots: Vec<Knot> = curve
                .knots()
                .iter()
                .map(|&t| fit.knot(t, None, ledger).expect("a knot"))
                .collect();
            knots
                .windows(2)
                .map(|w| {
                    let span = Todo {
                        a: w[0],
                        b: w[1],
                        depth: 0,
                    };
                    fit.middle(&span, ledger).ok().map_or(0, |q| crossed(&q))
                })
                .sum()
        })
    }

    /// The p-curve of `curve` on `surface` over its whole range, fitted
    /// towards `target` in one pass on a budget of `budget` units: the
    /// fit, `None` if the pass ran out or failed, and the units used.
    pub fn fit(
        surface: &Surface,
        curve: &Curve3D,
        target: f64,
        budget: u64,
    ) -> (Option<Fitted>, u64) {
        let ck = curve.knots();
        let range = [ck[0], ck[ck.len() - 1]];
        let mut work = Work::new(&Budget::new(budget));
        let pass = Pass {
            stage: "fit",
            rung: 0,
        };
        let out = work.pass(pass, &[()], |_, ledger| {
            fit_pcurve(surface, curve, range, None, target, ledger)
        });
        (out.ok().map(|mut v| v.remove(0)), work.used())
    }

    /// The sphere of radius `r` about `centre` with axis `axis` on its
    /// canonical half (angle cells 0 and 1, latitudes −1 and 0), at level
    /// `(level, level)`.
    pub fn sphere(centre: [f64; 3], r: f64, axis: [f64; 3], level: u32) -> Surface {
        let key = SphereKey::new(centre, r, axis).expect("a sphere");
        let lat = Lattice::new(key.into()).expect("a lattice");
        let mut rect = lat.half(0).expect("a half");
        rect.level.u = level;
        rect.level.v = level;
        rect.u = (rect.u.start << level)..(rect.u.end << level);
        rect.v = (rect.v.start << level)..(rect.v.end << level);
        Surface::new(lat, rect).expect("a surface")
    }

    /// The circle of `s`, a sphere, about the point at angle `angle` and
    /// latitude `latitude` (in quarter turns, of its lattice's frame)
    /// whose radius is `rho` times the sphere's: an oblique plane's, in
    /// four quarters, its spans the circle's 90° cells at level `level`.
    pub fn small_circle(s: &Surface, angle: f64, latitude: f64, rho: f64, level: u32) -> Curve3D {
        let FormKey::Sphere(k) = s.lattice().key() else {
            panic!("a sphere");
        };
        let (c, r) = (k.centre(), k.radius());
        let [e1, e2, a] = s.lattice().axes();
        let quarter = std::f64::consts::FRAC_PI_2;
        let (st, ct) = sin_cos(angle * quarter);
        let (sp, cp) = sin_cos(latitude * quarter);
        let n: [f64; 3] = std::array::from_fn(|i| cp * (ct * e1[i] + st * e2[i]) + sp * a[i]);
        // Two axes across `n`: along the angle and towards the axis.
        let t1: [f64; 3] = std::array::from_fn(|i| -st * e1[i] + ct * e2[i]);
        let t2: [f64; 3] = std::array::from_fn(|i| -sp * (ct * e1[i] + st * e2[i]) + cp * a[i]);
        let d = r * (1.0 - rho * rho).sqrt();
        let centre: [f64; 3] = std::array::from_fn(|i| c[i] + d * n[i]);
        let conic = Conic::circle(centre, [t1, t2], rho * r).expect("a circle");
        Curve3D::ellipse(&conic, level, 0, 4 << level).expect("a chain")
    }
}

/// [`fit_pcurve`] alone, in a pass of its own on a fresh budget of
/// `budget` units.
#[cfg(test)]
pub(crate) fn fit_alone(
    surface: &Surface,
    curve: &Curve3D,
    range: [f64; 2],
    ends: Option<[[f64; 2]; 2]>,
    target: f64,
    budget: u64,
) -> Result<Fitted, crate::infra::budget::Halt<FitError>> {
    use crate::infra::budget::{Budget, Pass, Work};
    let pass = Pass {
        stage: "fit",
        rung: 0,
    };
    let mut work = Work::new(&Budget::new(budget));
    work.pass(pass, &[()], |_, ledger| {
        fit_pcurve(surface, curve, range, ends, target, ledger)
    })
    .map(|mut f| f.remove(0))
}

#[cfg(test)]
mod tests;
