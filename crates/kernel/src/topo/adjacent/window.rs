//! The tube's window round an edge (module docs of [`super`], "The
//! window"): `ρ_min`, from which on the faces are apart by more than the
//! window's threshold; `ρ_graph`, up to which the inside certificate
//! holds and no other edge of the two faces comes; the join zone `r₀`;
//! and the search for `ρ_adj` among the powers of two.

use varde_decide::bezier::{HomCurve, Implicit, Scalar};
use varde_decide::num::bounded::{Bounded, div_up, down, up};
use varde_decide::num::level::{AtLevel, LevelScalar};
use varde_decide::num::schedule::UNITS;

use super::{
    Edge, EdgeCert, LEVELS, Ledger, MAX_SUBDIV_DEPTH, SBox, Settle, Skip, Stop, Strip,
    dyadic_floor, level_step, reach_bound, run_at, strip_patch,
};
use crate::geom::certify::implicit::ImplicitAt;
use crate::geom::certify::restrict;
use crate::geom::curve::{Curve3D, local};
use crate::infra::budget::{Cap, Step};

/// `1 + margin` of `ρ_min` (§ "The window"): `1 + 2⁻⁸`, the margin the
/// constructors' extents and gaps keep against rounding.
pub(crate) const MARGIN: f64 = 1.0 + 1.0 / 256.0;

/// The most halvings of a separation box beyond its cell, in `s` from
/// the edge, and in `t` and `s` together away from it: past them a box
/// not shown apart counts whole.
const SEP_DEPTH: [u32; 2] = [32, 12];

/// The most certificate runs of one window search: each narrows a strip
/// to below a failing point or widens one short of the tube, halving or
/// doubling it.
const MAX_ROUNDS: u32 = 2 * MAX_SUBDIV_DEPTH;

/// A span of a curve over part of its parameter, at the first level.
pub(crate) type Arc3 = HomCurve<Bounded<f64>, 3>;

/// The tube an edge's window gives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Tube {
    /// Its radius, a power of two.
    pub(crate) rho_adj: f64,
    /// The certified upper bound of `ρ_min` it is at least (0 for faces
    /// on one surface).
    pub(crate) rho_min: f64,
    /// The certified upper bound of the join zone `r₀` it is above.
    pub(crate) r0: f64,
}

/// An edge's window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Window {
    /// Not empty: the tube.
    Tube(Tube),
    /// Certified empty: `ρ_graph < ρ_min`, a certified upper bound of the
    /// one below a certified lower bound of the other.
    Empty {
        /// A certified lower bound of `ρ_min`.
        rho_min: f64,
        /// A certified upper bound of `ρ_graph`.
        rho_graph: f64,
    },
}

/// `δ/s` rounded up: a crease's `ρ_min` from a lower bound `sin2` of
/// `sin²α` over its cell (`δ` the threshold with its margin).
pub(super) fn crease_hi(delta: f64, sin2: f64) -> f64 {
    div_up(delta, down(sin2.sqrt()))
}

/// `δ/s` rounded down: a lower bound of a crease's `ρ_min` from an upper
/// bound `sin2` of `sin²α` at a point.
pub(super) fn crease_lo(delta: f64, sin2: f64) -> f64 {
    let s = up(sin2.sqrt());
    if s > 0.0 && s.is_finite() {
        let r = down(delta / s);
        if r > 0.0 { r } else { 0.0 }
    } else {
        0.0
    }
}

/// The least power of two above `x` (`x ≥ 0`); the least normal one for
/// a subnormal `x` or 0.
pub(crate) fn dyadic_above(x: f64) -> f64 {
    let f = dyadic_floor(x);
    if f > 0.0 { 2.0 * f } else { f64::MIN_POSITIVE }
}

/// The least power of two at least `x` (`x ≥ 0`).
fn dyadic_at_least(x: f64) -> f64 {
    let f = dyadic_floor(x);
    if f == x { f } else { dyadic_above(x) }
}

/// Whether no power of two lies in `[min_lo, graph_hi)`: then no tube
/// radius is at least `ρ_min ≥ min_lo` and short of `ρ_graph < graph_hi`
/// (a radius reaching `graph_hi` would reach a failing point or another
/// edge), and the window is certified empty.
fn no_radius(min_lo: f64, graph_hi: f64) -> bool {
    min_lo > 0.0 && dyadic_at_least(min_lo) >= graph_hi
}

/// The greatest power of two below `x`; 0 where there is none.
fn dyadic_below(x: f64) -> f64 {
    let f = dyadic_floor(x);
    if f == x { f * 0.5 } else { f }
}

// ---------------------------------------------------------------------
// The search.

/// The window of `edge` (module docs of [`super`], "The window"), the
/// faces' other edges (those sharing no end with it) as `near`, `delta`
/// the threshold the faces must be apart by beyond `ρ_min`; each
/// certificate run and each box charged to `ledger`. A window neither
/// certified empty nor holding a tube is unsettled
/// ([`Cap::PrecisionBits`]).
pub(crate) fn certify_window(
    edge: &Edge<'_>,
    near: &[Arc3],
    delta: f64,
    ledger: &mut Ledger<'_>,
) -> Result<Window, Stop<Skip>> {
    if edge.one_surface {
        // One injective surface: no certificate, never empty (module
        // docs); the least power of two above the ribbons.
        let [a, b] = edge.sides.map(|s| s.seam);
        let r0 = up(if a > b { a } else { b });
        if !r0.is_finite() {
            return Err(Stop::Failed(Skip::Unreadable));
        }
        return Ok(Window::Tube(Tube {
            rho_adj: dyadic_above(r0),
            rho_min: 0.0,
            r0,
        }));
    }
    let unsettled = Err(Stop::Cap(Cap::PrecisionBits));
    let curve = arcs(edge.curve, edge.t_range).ok_or(Stop::Failed(Skip::Unreadable))?;
    let mut width = initial_widths(edge, near);
    let mut narrowed = [false; 2];
    // Certified bounds gathered on the way: `ρ_graph` below the nearest
    // failing point, `ρ_min` above the creases' least angles.
    let mut graph_hi = f64::INFINITY;
    let mut min_lo: f64 = 0.0;
    for _ in 0..MAX_ROUNDS {
        let held = match super::certify_edge(edge, &Strip { width }, delta, ledger)? {
            EdgeCert::Fails {
                side,
                s,
                dist,
                rho_min_lo,
                ..
            } => {
                if dist < graph_hi {
                    graph_hi = dist;
                }
                if rho_min_lo > min_lo {
                    min_lo = rho_min_lo;
                }
                if no_radius(min_lo, graph_hi) {
                    return Ok(Window::Empty {
                        rho_min: min_lo,
                        rho_graph: graph_hi,
                    });
                }
                let w = dyadic_below(s);
                if w <= 0.0 || w.is_nan() {
                    return Err(Stop::Cap(Cap::SubdivisionDepth));
                }
                width[side] = w;
                narrowed[side] = true;
                continue;
            }
            EdgeCert::Held(h) => h,
        };
        if held.rho_min_lo > min_lo {
            min_lo = held.rho_min_lo;
        }
        let r0 = if held.r0[0] > held.r0[1] {
            held.r0[0]
        } else {
            held.r0[1]
        };
        if !(held.rho_min.is_finite() && r0.is_finite()) {
            return unsettled;
        }
        let mut rho = dyadic_at_least(held.rho_min);
        if rho <= r0 {
            rho = dyadic_above(r0);
        }
        // The other edges first: a certified bound of `ρ_graph` whatever
        // the strips.
        match apart(&curve, near, rho, ledger)? {
            Apart::Yes => {}
            Apart::No(u) => {
                if u < graph_hi {
                    graph_hi = u;
                }
                return if no_radius(min_lo, graph_hi) {
                    Ok(Window::Empty {
                        rho_min: min_lo,
                        rho_graph: graph_hi,
                    })
                } else {
                    unsettled
                };
            }
            Apart::Open => return unsettled,
        }
        if rho >= graph_hi {
            return if no_radius(min_lo, graph_hi) {
                Ok(Window::Empty {
                    rho_min: min_lo,
                    rho_graph: graph_hi,
                })
            } else {
                unsettled
            };
        }
        // Then the strips' far sides: a strip short of the tube is
        // widened where it may be.
        let mut short = None;
        for (side, far) in held.far.iter().enumerate() {
            if apart(&curve, far, rho, ledger)? != Apart::Yes {
                short = Some(side);
                break;
            }
        }
        if let Some(side) = short {
            let cut = held.reach[side] < width[side];
            if narrowed[side] || cut || width[side] >= 1.0 {
                return unsettled;
            }
            width[side] *= 2.0;
            continue;
        }
        // The largest power of two the window holds.
        loop {
            let next = 2.0 * rho;
            if next >= graph_hi || next.is_nan() {
                break;
            }
            let walls = [near, &held.far[0], &held.far[1]];
            let mut all = true;
            for w in walls {
                if apart(&curve, w, next, ledger)? != Apart::Yes {
                    all = false;
                    break;
                }
            }
            if !all {
                break;
            }
            rho = next;
        }
        return Ok(Window::Tube(Tube {
            rho_adj: rho,
            rho_min: held.rho_min,
            r0,
        }));
    }
    Err(Stop::Cap(Cap::SubdivisionDepth))
}

/// The strips' first widths: reaching about twice as far as the nearest
/// other edge's ends from the curve's middle (no tube reaches past
/// that), by the faces' speeds across the edge there, approximately (the
/// certificate checks what they reach); the whole tile where no other
/// edge bounds it or the estimate fails.
fn initial_widths(edge: &Edge<'_>, near: &[Arc3]) -> [f64; 2] {
    let [t0, t1] = edge.t_range;
    let tm = (t0 + t1) * 0.5;
    let Some(c) = edge.curve.point_at(tm) else {
        return [1.0; 2];
    };
    let mut d = f64::INFINITY;
    for arc in near {
        for t in [0.0, 1.0] {
            let p = arc.eval(&Bounded::exact(t)).map(|x| x.v);
            let q = [p[0] - c[0], p[1] - c[1], p[2] - c[2]];
            let e = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt();
            if e < d {
                d = e;
            }
        }
    }
    if !d.is_finite() {
        return [1.0; 2];
    }
    edge.sides.map(|side| {
        let speed = || -> Option<f64> {
            let p = side.pcurve.point_at(tm)?;
            let dt = (t1 - t0) * 1e-3;
            let (a, b) = (
                side.pcurve.point_at(tm - dt)?,
                side.pcurve.point_at(tm + dt)?,
            );
            let dir = [b[0] - a[0], b[1] - a[1]];
            let (du, dv) = (dir[0].abs(), dir[1].abs());
            let big = if du > dv { du } else { dv };
            if big <= 0.0 || big.is_nan() {
                return None;
            }
            let nu = [dir[1] / big, dir[0] / big];
            let pd = side.surface.eval_partials(p)?;
            let v: Vec<f64> = (0..3)
                .map(|k| pd.du[k].v * nu[0] + pd.dv[k].v * nu[1])
                .collect();
            let sp = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            (sp > 0.0 && sp.is_finite()).then_some(sp)
        };
        match speed() {
            Some(sp) => {
                let w = dyadic_at_least(2.0 * d / sp);
                if w > 0.0 && w < 1.0 { w } else { 1.0 }
            }
            None => 1.0,
        }
    })
}

// ---------------------------------------------------------------------
// Distances between curves.

/// The spans of `curve` over `range` of its parameter, at the first
/// level; `None` where one can't be read.
pub(crate) fn arcs(curve: &Curve3D, range: [f64; 2]) -> Option<Vec<Arc3>> {
    let like = Bounded::exact(0.0);
    let k = curve.knots();
    let mut out = Vec::new();
    for i in 0..curve.spans().len() {
        let (a, b) = (*k.get(i)?, *k.get(i + 1)?);
        if b <= range[0] || a >= range[1] {
            continue;
        }
        let lo = if range[0] > a { range[0] } else { a };
        let hi = if range[1] < b { range[1] } else { b };
        let span = curve.span_at(i, &like)?;
        out.push(restrict(
            &span,
            local(lo, a, b, &like),
            local(hi, a, b, &like),
        ));
    }
    Some(out)
}

/// Whether two sets of spans are at least `ρ` apart.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Apart {
    /// Certified: every pair of their points at least `ρ` apart.
    Yes,
    /// Certified not: two of their points less than the value apart, a
    /// certified upper bound below `ρ`.
    No(f64),
    /// Neither, at the deepest subdivision.
    Open,
}

/// A certified lower bound of the distance between two boxes.
fn box_gap(a: &[[f64; 2]; 3], b: &[[f64; 2]; 3]) -> f64 {
    let mut sum = 0.0;
    for k in 0..3 {
        let g = down(if b[k][0] > a[k][1] {
            b[k][0] - a[k][1]
        } else if a[k][0] > b[k][1] {
            a[k][0] - b[k][1]
        } else {
            0.0
        });
        if g > 0.0 {
            sum = down(sum + down(g * g));
        }
    }
    if sum > 0.0 { down(sum.sqrt()) } else { 0.0 }
}

/// An upper bound of `x` (either sign) computed in round-to-nearest.
fn up_any(x: f64) -> f64 {
    if x >= 0.0 { up(x) } else { -down(-x) }
}

/// The enclosure of a span's first point (its first control point: the
/// span passes through it).
fn start(a: &Arc3) -> [[f64; 2]; 3] {
    let [lo, hi] = a.weights().coefs()[0].enclose();
    std::array::from_fn(|k| {
        let [xl, xh] = a.coords()[k].coefs()[0].enclose();
        let c = [xl / lo, xl / hi, xh / lo, xh / hi];
        let (mut l, mut h) = (c[0], c[0]);
        for &x in &c[1..] {
            if x < l || x.is_nan() {
                l = x;
            }
            if x > h || x.is_nan() {
                h = x;
            }
        }
        [down(l), up_any(h)]
    })
}

/// A certified upper bound of the distance between two points' boxes.
fn point_gap(a: &[[f64; 2]; 3], b: &[[f64; 2]; 3]) -> f64 {
    let mut sum = 0.0;
    for k in 0..3 {
        let (x, y) = (up_any(a[k][1] - b[k][0]), up_any(b[k][1] - a[k][0]));
        let d = if x > y { x } else { y };
        sum = up(sum + up(d * d));
    }
    let r = up(sum.sqrt());
    if r.is_nan() { f64::INFINITY } else { r }
}

/// The diagonal of a box, approximately (which span to halve).
fn diagonal(b: &[[f64; 2]; 3]) -> f64 {
    let d = [0, 1, 2].map(|k| b[k][1] - b[k][0]);
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

/// Whether the spans `a` and `b` are at least `rho` apart (module docs):
/// pairs of spans by their control boxes, the larger halved until the
/// boxes are apart or two of their first points are near, each pair
/// charged one unit.
pub(crate) fn apart(
    a: &[Arc3],
    b: &[Arc3],
    rho: f64,
    ledger: &mut Ledger<'_>,
) -> Result<Apart, Stop<Skip>> {
    let mut stack: Vec<(Arc3, Arc3, u32)> = Vec::new();
    for x in a.iter().rev() {
        for y in b.iter().rev() {
            stack.push((x.clone(), y.clone(), 0));
        }
    }
    while let Some((x, y, depth)) = stack.pop() {
        ledger.charge(Step::Work, 1)?;
        let (Some(bx), Some(by)) = (x.control_box(), y.control_box()) else {
            return Ok(Apart::Open);
        };
        if box_gap(&bx, &by) >= rho {
            continue;
        }
        let u = point_gap(&start(&x), &start(&y));
        if u < rho {
            return Ok(Apart::No(u));
        }
        if depth >= MAX_SUBDIV_DEPTH {
            return Ok(Apart::Open);
        }
        if diagonal(&bx) >= diagonal(&by) {
            let [l, r] = x.split_half();
            stack.push((r, y.clone(), depth + 1));
            stack.push((l, y, depth + 1));
        } else {
            let [l, r] = y.split_half();
            stack.push((x.clone(), r, depth + 1));
            stack.push((x, l, depth + 1));
        }
    }
    Ok(Apart::Yes)
}

// ---------------------------------------------------------------------
// The faces' separation.

/// An interval `[lo, hi]`, rounded outwards.
#[derive(Clone, Copy, Debug)]
struct Iv {
    lo: f64,
    hi: f64,
}

impl Iv {
    const ZERO: Iv = Iv { lo: 0.0, hi: 0.0 };

    fn of(x: [f64; 2]) -> Iv {
        Iv { lo: x[0], hi: x[1] }
    }

    fn add(self, o: Iv) -> Iv {
        Iv {
            lo: down(self.lo + o.lo),
            hi: up_any(self.hi + o.hi),
        }
    }

    fn mul(self, o: Iv) -> Iv {
        let p = [
            self.lo * o.lo,
            self.lo * o.hi,
            self.hi * o.lo,
            self.hi * o.hi,
        ];
        let (mut l, mut h) = (p[0], p[0]);
        for &x in &p[1..] {
            if x < l || x.is_nan() {
                l = x;
            }
            if x > h || x.is_nan() {
                h = x;
            }
        }
        Iv {
            lo: down(l),
            hi: up_any(h),
        }
    }

    fn abs_max(self) -> f64 {
        let (a, b) = (-self.lo, self.hi);
        let m = if b > a { b } else { a };
        if m.is_nan() { f64::INFINITY } else { m }
    }
}

/// A certified upper bound of `|∇F|` over the box `[lo, hi]` (world
/// coordinates) of the implicit `f`: its partial derivatives term by term
/// in interval arithmetic.
fn gradient_bound<S: Scalar>(f: &Implicit<S>, lo: [f64; 3], hi: [f64; 3]) -> f64 {
    let o = f.origin().each_ref().map(|x| x.enclose());
    let y: [Iv; 3] = std::array::from_fn(|k| Iv {
        lo: down(lo[k] - o[k][1]),
        hi: up_any(hi[k] - o[k][0]),
    });
    let pow = |x: Iv, n: usize| (0..n).fold(Iv { lo: 1.0, hi: 1.0 }, |acc, _| acc.mul(x));
    let mut sum = 0.0;
    for j in 0..3 {
        let mut g = Iv::ZERO;
        for (e, c) in f.terms() {
            if e[j] == 0 {
                continue;
            }
            let mut t = Iv::of(c.enclose()).mul(Iv {
                lo: e[j] as f64,
                hi: e[j] as f64,
            });
            for k in 0..3 {
                let n = if k == j { e[k] - 1 } else { e[k] };
                t = t.mul(pow(y[k], n));
            }
            g = g.add(t);
        }
        let m = g.abs_max();
        sum = up(sum + up(m * m));
    }
    let r = up(sum.sqrt());
    if r.is_nan() { f64::INFINITY } else { r }
}

/// A separation box's outcome.
enum Sep {
    /// The face's points there are farther than `δ` from the other
    /// face's surface.
    Apart,
    /// Not shown: the box's points lie within the value of the curve.
    Near(f64),
    /// Rounding decides: one level up.
    Precision,
    /// The forms can't be built.
    Unformed,
}

/// One box of a face's strip against the other face's surface.
struct SepAt<'a, 'b> {
    at: &'a Settle<'a, 'b>,
    side: usize,
    f: &'a crate::geom::implicit::TileImplicit,
    b: SBox,
}

impl AtLevel<()> for SepAt<'_, '_> {
    type Out = Sep;

    fn at<S: LevelScalar>(&mut self, like: &S, _: &mut ()) -> Sep {
        let a = self.at;
        let Some((x, sp)) = strip_patch(
            a.edge,
            a.piece,
            a.cell,
            a.frame,
            self.side,
            (self.b.kt, self.b.dt),
            self.b.s_range(),
            like,
        ) else {
            return Sep::Unformed;
        };
        let f = self.f.implicit_at(like);
        let Some(phi) = x.compose_implicit(&f) else {
            return Sep::Unformed;
        };
        let reach = || reach_bound(&x, &sp.c).unwrap_or(f64::INFINITY);
        let w = x.weights().hull();
        let Some(bx) = x.hull_box() else {
            return Sep::Near(reach());
        };
        if w.lo <= 0.0 || w.lo.is_nan() {
            return Sep::Near(reach());
        }
        let d = a.delta;
        let lo = bx.lo.map(|v| down(v - d));
        let hi = bx.hi.map(|v| up_any(v + d));
        let g = gradient_bound(&f, lo, hi);
        // `|F| ≥ min|φ| / W_max^d` over the box.
        let wd = (0..f.degree()).fold(1.0, |acc, _| up(acc * w.hi));
        let ph = phi.hull();
        let least = if ph.lo > 0.0 {
            ph.lo
        } else if ph.hi < 0.0 {
            -ph.hi
        } else {
            0.0
        };
        if least > 0.0 && down(least / wd) > up(g * d) {
            return Sep::Apart;
        }
        // The values alone would show it: rounding decides.
        let vals: Vec<f64> = phi.coefs().iter().map(LevelScalar::approx).collect();
        let (vl, vh) = vals
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), &v| {
                (if v < l { v } else { l }, if v > h { v } else { h })
            });
        let vleast = if vl > 0.0 {
            vl
        } else if vh < 0.0 {
            -vh
        } else {
            0.0
        };
        if vleast / wd > g * d * 2.0 {
            return Sep::Precision;
        }
        Sep::Near(reach())
    }

    fn escalates(_: &Sep) -> bool {
        false
    }
}

/// `ρ_min`'s certified upper bound from one cell where the faces may be
/// tangent: face `side`'s strip against the other face's surface (its
/// tile's implicit, the form extended across the edge), box by box: a box
/// whose points are all farther than `δ` from it ([`gradient_bound`]: a
/// value of the implicit above its gradient's bound times `δ`) is apart;
/// the largest distance from the curve of a box not shown apart bounds
/// `ρ_min`. Boxes from the edge are halved in `s` until their distance is
/// at most `δ` or the largest so far; others in turn, to the depth limit.
/// `∞` where the other surface has no implicit or the forms can't be
/// built.
pub(super) fn separation(
    at: &Settle<'_, '_>,
    side: usize,
    ledger: &mut Ledger<'_>,
) -> Result<f64, Stop<Skip>> {
    let other = 1 - side;
    let (oi, oj) = at.frame.tile[other];
    let Some(Ok(f)) = at.edge.sides[other].surface.implicit(oi, oj) else {
        return Ok(f64::INFINITY);
    };
    let mut worst: f64 = 0.0;
    let mut boxes = vec![SBox::WHOLE];
    while let Some(b) = boxes.pop() {
        ledger.charge(
            level_step(b.level),
            UNITS.adjacent.sign[usize::from(b.level)],
        )?;
        let out = run_at(&mut SepAt { at, side, f: &f, b }, b.level);
        let deep = at.cell.depth + b.dt + b.ds >= MAX_SUBDIV_DEPTH;
        let reach = match out {
            Sep::Apart => continue,
            Sep::Unformed => return Ok(f64::INFINITY),
            Sep::Precision if b.level + 1 < LEVELS => {
                boxes.push(SBox {
                    level: b.level + 1,
                    ..b
                });
                continue;
            }
            Sep::Precision => f64::INFINITY,
            Sep::Near(r) => r,
        };
        let limit = if b.ks == 0 {
            b.ds >= SEP_DEPTH[0]
        } else {
            b.dt + b.ds >= SEP_DEPTH[1]
        };
        // A box within `δ` (or the largest distance so far) of the curve
        // adds nothing: its points are inside the tube however it splits.
        let settled = reach <= at.delta || reach <= worst;
        if settled || limit || deep || !reach.is_finite() {
            if reach > worst || reach.is_nan() {
                worst = reach;
            }
            continue;
        }
        let halves = if b.ks == 0 || b.dt > b.ds {
            [0, 1].map(|x| SBox {
                ks: 2 * b.ks + x,
                ds: b.ds + 1,
                ..b
            })
        } else {
            [0, 1].map(|x| SBox {
                kt: 2 * b.kt + x,
                dt: b.dt + 1,
                ..b
            })
        };
        // The farther half first, so the edge's box meets the largest
        // distance so far.
        boxes.push(halves[0]);
        boxes.push(halves[1]);
    }
    Ok(worst)
}
