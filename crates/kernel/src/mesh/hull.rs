//! The control-hull rules between two patches.
//!
//! A patch lies in the convex hull of its six control points while its
//! weights are positive. Three rules, by what two patches share, keep the
//! surface from passing through itself:
//!
//! - **Non-neighbours** (no shared vertex): the hulls are more than the
//!   margin apart ([`non_neighbours_apart`], by GJK).
//! - **Edge neighbours**: a plane through the shared edge's three control
//!   points has the other three control points of one patch on one side
//!   and the other patch's on the other ([`edge_neighbours_apart`]); or,
//!   for a curved edge, the two patches lie on opposite sides of the
//!   cylinder over the edge's conic ([`cylinder_apart`]), which is what
//!   parts them where the surface touches the edge's plane all along it;
//!   or, at a crease where both leave the edge on one side of its plane
//!   and of that cylinder, opposite sides of a quadric from the pencil of
//!   the two ([`pencil_member`]).
//! - **Vertex neighbours** (sharing one vertex only): a plane through the
//!   vertex has the other five control points of each on opposite sides
//!   ([`vertex_neighbours_apart`]).
//!
//! A point of a patch is a Bernstein-weighted mean of its control points,
//! so it can lie on the plane only where every control point off the
//! plane has weight zero: on the shared edge, or at the shared vertex.
//! With one patch strictly off the plane, the two can meet only there.
//! (A curved edge lets one side cross its plane by less than the margin;
//! the other then clears that side's highest point by more than the
//! margin, so they may overlap by less than the margin, anywhere along
//! the pair.)
//!
//! "Apart" always means by more than a margin (the resolution), so a
//! check doesn't flip under rounding. Every test here is conservative: it
//! says apart only when it has shown it.

use glam::DVec3;

use crate::patch::Patch;

/// The most GJK steps before giving up (and not calling the hulls apart).
/// Two hulls of six points converge in a handful.
const MAX_GJK_STEPS: usize = 64;

/// Two patches sharing no vertex: their control hulls are more than
/// `margin` apart.
pub(crate) fn non_neighbours_apart(a: &Patch, b: &Patch, margin: f64) -> bool {
    apart(&a.hull(), &b.hull(), margin)
}

/// Two patches sharing corner `ka` of `a`, which is corner `kb` of `b`,
/// and no other: some plane through the corner has the other five control
/// points of `a` more than `margin` on one side and those of `b` more than
/// `margin` on the other.
///
/// A plane through `v` with unit normal `n` does it when `n·x > margin`
/// for every `x` in `{a_i - v} ∪ {v - b_i}`, and the best such `n` clears
/// them by the distance from the origin to their convex hull.
pub(crate) fn vertex_neighbours_apart(
    a: &Patch,
    ka: usize,
    b: &Patch,
    kb: usize,
    margin: f64,
) -> bool {
    let v = a.p[ka];
    let mut points = [DVec3::ZERO; 10];
    let others = |x: &Patch, k: usize| {
        let hull = x.hull();
        let mut out = [DVec3::ZERO; 5];
        // Corner k is hull point 2k.
        for (o, i) in out.iter_mut().zip((0..6).filter(|&i| i != 2 * k)) {
            *o = hull[i];
        }
        out
    };
    for (i, x) in others(a, ka).into_iter().enumerate() {
        points[i] = x - v;
    }
    for (i, x) in others(b, kb).into_iter().enumerate() {
        points[5 + i] = v - x;
    }
    apart(&points, &[DVec3::ZERO], margin)
}

/// Two patches sharing edge `ea` of `a`, which is edge `eb` of `b` (run
/// the other way): a plane through the edge's control points has the
/// other three control points of `a` and those of `b` on opposite sides.
///
/// A straight edge (its control point within `margin` of the line through
/// its ends) leaves the plane free to turn about the line, and both sides
/// must clear it by more than `margin`: the distance from the origin to
/// the hull of `a`'s points and `b`'s mirrored, projected along the line.
/// A curved edge fixes the plane. Then one side must clear it by more than
/// `margin` and the other must not cross it by more than `margin`, so a
/// flat cap whose curved edge meets a wall passes: the cap lies in the
/// plane, and the wall meets the plane only along the edge. The strict
/// side must also clear the lax side's highest point (or the plane, if
/// that is higher) by more than `margin`, so the two sides' far control
/// points are always more than `margin` apart, as for non-neighbours: a
/// lax side leaning in by `c` needs the strict side `margin + c` off the
/// plane. Any overlap is less than `margin` deep, but it isn't confined
/// to the edge's neighbourhood: a strict side nearly parallel to the
/// plane leaves a band under the margin across the whole pair.
pub(crate) fn edge_neighbours_apart(
    a: &Patch,
    ea: usize,
    b: &Patch,
    eb: usize,
    margin: f64,
) -> bool {
    let (p, c, q) = (a.p[ea], a.c[ea], a.p[(ea + 1) % 3]);
    let others = |x: &Patch, e: usize| [x.p[(e + 2) % 3], x.c[(e + 1) % 3], x.c[(e + 2) % 3]];
    let (oa, ob) = (others(a, ea), others(b, eb));
    let Some((u, across)) = across_line(p, q) else {
        return false;
    };
    let bulge = across(c);
    if bulge.length() <= margin {
        let mut points = [DVec3::ZERO; 6];
        for i in 0..3 {
            points[i] = across(oa[i]);
            points[3 + i] = -across(ob[i]);
        }
        return apart(&points, &[DVec3::ZERO], margin);
    }
    let n = u.cross(bulge).normalize();
    let side = |x: DVec3| n.dot(x - p);
    let (sa, sb) = (oa.map(side), ob.map(side));
    [1.0, -1.0].into_iter().any(|sign: f64| {
        let a_min = sa.iter().map(|&s| sign * s).fold(f64::INFINITY, f64::min);
        let b_max = sb
            .iter()
            .map(|&s| sign * s)
            .fold(f64::NEG_INFINITY, f64::max);
        // One side lax (not past the plane by the margin), the other
        // strict: clearing the plane and the lax side's highest point by
        // more than the margin, which is clearing the plane by more than
        // it when the lax side doesn't lean in.
        (b_max < margin && a_min - b_max.max(0.0) > margin)
            || (a_min > -margin && a_min.min(0.0) - b_max > margin)
    })
}

/// Two patches sharing edge `ea` of `a`, which is edge `eb` of `b`: the
/// plane rule ([`edge_neighbours_apart`]), else the cylinder rule
/// ([`cylinder_apart`]), else the pencil rule ([`pencil_member`]), the
/// last two on one pass over each patch's coefficients ([`CurvedPair`]).
/// What `check` and repair ask of edge neighbours.
///
/// Bands and caps (`sweep/lathe.rs`) grade their strips by the plane rule
/// alone: its failure on a fitted diagonal is what tells them a strip is
/// too coarse, and with the cylinder too they chose strips that then
/// failed the vertex rule.
pub(crate) fn edge_neighbours_parted(
    a: &Patch,
    ea: usize,
    b: &Patch,
    eb: usize,
    margin: f64,
) -> bool {
    edge_neighbours_apart(a, ea, b, eb, margin)
        || CurvedPair::new(a, ea, b, eb, margin).is_some_and(|pair| {
            cylinder_apart(&pair, margin) || pencil_member(&pair, margin).is_some()
        })
}

/// Two patches sharing a curved edge (`pair`): they lie on opposite sides
/// of the cylinder over the edge's conic (through it, square to its
/// plane), each clear of it but along the edge.
///
/// Where the surface touches the edge's plane all along the edge (a ring
/// at a turn of a solid of revolution: a torus's top, a flat face meeting
/// a round tangentially) both patches hold the edge's control points and
/// lie on one side of every plane through them, so the plane rule can't
/// part them. Leaving the edge across it, though, one goes in towards the
/// conic's centre and the other out.
///
/// With the edge `P, C, Q` of weight `w`, and `λP, λC, λQ` the barycentric
/// coordinates of a point's projection onto the control triangle,
/// `F = λC² − 4w²·λP·λQ` is a quadratic function of space whose zero set
/// is that cylinder (for a circle, `ρ² − ρ0²` up to a positive factor). On
/// a patch `F` is `N/W²`, with `N` and `W²` of degree four: their
/// Bernstein coefficients are sums over pairs of the patch's homogeneous
/// control points of `F`'s polar form (and of the weights' product),
/// times whole multinomial factors. With the shared edge as row 0 (the
/// far corner's exponent 0), row 0 is the edge itself, where `F` is zero:
/// its coefficients vanish exactly, so they aren't computed, but the two
/// patches must hold the same edge (its bits, as a mesh's neighbours do).
/// The rule asks every coefficient ratio `N_γ/W²_γ` in rows 1 to 4 to be
/// past `margin·|∇F|` and its rounding bound with one sign on one patch
/// and the other sign on the other. Then `F` (a positive mean of the
/// ratios) has one sign on each patch but on the edge, so the two meet
/// only there, as the plane rule promises. Splitting keeps that in exact
/// arithmetic: a half of the edge lies on the same conic, so its `F` is
/// the parent's times a positive constant, and a piece's ratios are
/// weighted means of its parent's. (The rounding bound, and off a circle
/// the threshold, are each piece's own.)
///
/// `|∇F|` is taken as `4w²` over the larger of the far ends' heights
/// above the lines through the other end and `C`: its value at the end
/// where it is smaller, which on a circle is its value all along the
/// edge (on other conics it may dip between the ends). Unlike the plane
/// rule's margin, which is a distance of control points, this one is a
/// scale for the coefficients of `F`.
///
/// **Rounding.** On a flat control triangle (a short arc, or a long edge
/// barely curved) the normal `n` is known only to about `ε·κ` of its
/// length, `κ = |P − C|·|Q − C|/|n| = 1/sin φ` with `φ` the angle at `C`:
/// a control point far off the edge's plane then gets coordinates off by
/// its height over the triangle's times that, and terms of `F` in the
/// hundreds or more may cancel. So each point's coordinates carry a bound
/// on their error, `e = 64·ε·κ·(d·reach + m + ω)` (`d` its distance from
/// `C`, `reach` the larger of `|P − C|`, `|Q − C|` over `|n|`, so that
/// `d·reach` bounds `|λP|` and `|λQ|`; `m` the largest `|λ|` computed, `ω`
/// the weight), and each ratio one of `(1 + 4w²)·Σ k·(ex·my + mx·ey +
/// ex·ey + 16·ε·mx·my) / W²_γ` over the pairs that sum to it, `k` their
/// multinomial factors (derivations at [`LAMBDA_ROUNDING`] and
/// [`CYLINDER_ROUNDING`]). A
/// ratio must clear the threshold plus that bound, so a certificate that
/// rounding made up is refused, not believed: a long edge barely curved
/// with patches folded far out of its plane, in a tilted frame, passed
/// without it with both patches truly outside the cylinder.
///
/// A straight edge (its control point within `margin` of its chord), a
/// nearly degenerate control triangle, different edges, and anything not
/// finite are refused.
fn cylinder_apart(pair: &CurvedPair, margin: f64) -> bool {
    let edge = &pair.edge;
    // `margin·|∇F|`, `|∇F|` at the end where it is smaller.
    let threshold = margin * 4.0 * edge.w2 / edge.height();
    // The sign of F on the patch, or None.
    let side = |coefficients: &[Coefficient; 10]| -> Option<f64> {
        let mut sign = 0.0;
        for k in coefficients {
            // NaN fails the comparison.
            let clear = k.f.abs() > threshold + k.ef;
            if !clear || (sign != 0.0 && k.f.signum() != sign) {
                return None;
            }
            sign = k.f.signum();
        }
        Some(sign)
    };
    matches!((side(&pair.a), side(&pair.b)), (Some(sa), Some(sb)) if sa == -sb)
}

/// Two patches sharing a curved edge (`pair`, first `a` then `b`): the
/// member `(α, β)` of the pencil `G = α·F + β·P` of the cylinder over the
/// edge's conic (`F`, as in [`cylinder_apart`]) and the edge's plane (`P`,
/// the signed distance from it) that has one sign on `a` (`G > 0`) and
/// the other on `b`, each clear of `G = 0` but along the edge, if one is
/// found.
///
/// At a crease (two faces meeting at an angle along a curved edge) both
/// faces may leave the edge on one side of its plane and on one side of
/// the cylinder over its conic, or one along it: an acute corner of a
/// revolved profile against a wall, two sides leaving a corner into one
/// quadrant of the meridian plane. Neither the plane rule nor the
/// cylinder rule parts them then. Every `G` of the pencil vanishes on the
/// edge (both `F` and `P` do) and is quadratic in space; near the edge,
/// in the plane square to it, `F = 0` and `P = 0` are two lines square to
/// each other (for a circle `G = 0` is a paraboloid of revolution through
/// the ring), and their mixtures turn the line through any direction, so
/// to first order some member parts any two faces that leave the edge in
/// different directions. Curvature is second order: on coarse pieces it
/// may still spoil it, and repair's splitting mends that.
///
/// **Coefficients.** On a patch `G = (α·N_F + β·N_P)/W²`, `N_P` the
/// degree-four numerator of `P`: `P·W` is quadratic, with control values
/// `ν = n·s/|n|` for a homogeneous control point `(s, ω)`, `s = ω·(x − C)`,
/// and `P·W²`'s polar form is `(ν(X)·ω(Y) + ν(Y)·ω(X))/2`. One pass over
/// the pairs ([`CurvedEdge::coefficients`]) gives, per coefficient `γ` in
/// rows 1 to 4, `rF_γ = N_F,γ/W²_γ`, `rP_γ = N_P,γ/W²_γ` and bounds on
/// their rounding. Row 0 (the edge) is exactly zero for both: the edge's
/// three control points lie on the conic's cylinder and in its plane.
///
/// **The search.** `F` is scaled by `g`, the cylinder rule's `|∇F|`, so
/// `F/g` and `P` both have unit gradient at the edge (on a circle; a scale
/// on other conics) and square to each other (`∇F` lies in the plane,
/// `∇P` is its normal). A unit `u = (α', β)` gives `G = α'·F/g + β·P` with
/// `|∇G| = 1`, and does it when `s_γ·(α'·rF_γ/g + β·rP_γ) > m_γ` for every
/// coefficient of both patches (`s = +1` on `a`, `−1` on `b`), with `m_γ =
/// margin + eF_γ/g + eP_γ` (the threshold plus the rounding bound for any
/// unit `u`). So the 20 points `p_γ = s_γ·(rF_γ/g, rP_γ)/m_γ` must all
/// have `u·p_γ > 1`: the distance from the origin to their convex hull
/// must be more than 1, as for the plane rule's straight edge in two
/// dimensions. The best `u` points at the hull's closest point, a vertex
/// or the foot on a segment, so every vertex direction and the outer
/// normal of every segment are tried (210 directions against 20 points,
/// in a fixed order) and the best kept. No trigonometry. A point within 1
/// of the origin refuses at once, a direction whose own point can't beat
/// the best so far is dropped before it is normalised, and one is dropped
/// at its first point under the best: a pair costs a few times the
/// cylinder's test, whose coefficients it shares.
///
/// **The check.** The chosen member is then checked with its own bounds,
/// so nothing rests on the search: every `s_γ·r_γ > margin + |α'|·eF_γ/g +
/// |β|·eP_γ + 4ε·(|α'·rF_γ/g| + |β·rP_γ|)`, `r_γ = α'·rF_γ/g + β·rP_γ` (the
/// last term the rounding of `r_γ` itself and of the division by `g`).
///
/// **Soundness.** `G` is a quadratic function of space vanishing on the
/// edge. With its exact ratios in rows 1 to 4 positive on `a` and negative
/// on `b`, `N_G = Σ B_γ·W²_γ·r_γ` (row 0 zero, the weights' coefficients
/// positive) is positive on `a` but on the edge and negative on `b` but on
/// it, so the two meet only on the edge: the cylinder rule's argument for
/// another quadric through the edge (`F` and `P` are the members `β = 0`
/// and `α = 0`). The floating-point ratios carry the cylinder rule's
/// bounds on `F` and [`NU_ROUNDING`]'s on `ν`; a ratio clearing the
/// threshold plus its bound has the exact ratio's sign.
///
/// **Splitting** keeps it in exact arithmetic: a piece of the edge lies on
/// the same conic and in the same plane, its `F` the parent's times a
/// positive constant and its `P` the parent's (its control triangle turns
/// the same way), so the parent's member is in the piece's pencil, and the
/// piece's ratios are weighted means of the parent's. Under the margin
/// they may fail, as the plane's and the cylinder's do.
///
/// **The threshold** is `margin` times `|∇G|` at the edge: exactly that on
/// a circle, a scale on other conics, as for the cylinder; not a distance
/// of control points as the plane rule's.
///
/// The cylinder rule's refusals come first (a straight edge, a nearly
/// degenerate control triangle, different edges); anything not finite is
/// refused too.
fn pencil_member(pair: &CurvedPair, margin: f64) -> Option<(f64, f64)> {
    let g = 4.0 * pair.edge.w2 / pair.edge.height();
    let finite = |k: &Coefficient| {
        k.f.is_finite() && k.ef.is_finite() && k.p.is_finite() && k.ep.is_finite()
    };
    if !(g.is_finite() && g > 0.0 && pair.a.iter().chain(&pair.b).all(finite)) {
        return None;
    }
    // Each coefficient with its side's sign: `(s, F/g, eF/g, P, eP)`.
    let signed = |i: usize| {
        let (s, k) = if i < 10 {
            (1.0, pair.a[i])
        } else {
            (-1.0, pair.b[i - 10])
        };
        (s, k.f / g, k.ef / g, k.p, k.ep)
    };
    let mut points = [(0.0, 0.0); 20];
    for (i, point) in points.iter_mut().enumerate() {
        let (s, f, ef, p, ep) = signed(i);
        let m = margin + ef + ep;
        *point = (s * f / m, s * p / m);
    }
    // No unit `u` has `u·p > 1` for a point within 1 of the origin
    // (NaN refused too).
    if !points.iter().all(|p| p.0 * p.0 + p.1 * p.1 > 1.0) {
        return None;
    }
    // The least `u·p` over the points, for a unit `u`, if it is more
    // than `floor`: it stops at the first point under it.
    let score = |u: (f64, f64), floor: f64| {
        let mut least = f64::INFINITY;
        for p in &points {
            let d = u.0 * p.0 + u.1 * p.1;
            // NaN fails the comparison.
            let over = d > floor;
            if !over {
                return None;
            }
            least = least.min(d);
        }
        Some(least)
    };
    // The best direction with a score over 1 (the first of equals).
    let mut best = (1.0, None);
    // A direction `v`, not of unit length, with `v·p = c` at the point
    // it was made from: its score is at most `c/|v|`, so it is
    // dropped unless that beats the best (NaN dropped too).
    let mut consider = |v: (f64, f64), c: f64| {
        let vv = v.0 * v.0 + v.1 * v.1;
        if !(c > 0.0 && c * c > best.0 * best.0 * vv) {
            return;
        }
        let length = vv.sqrt();
        let u = (v.0 / length, v.1 / length);
        if let Some(s) = score(u, best.0) {
            best = (s, Some(u));
        }
    };
    for (i, &p) in points.iter().enumerate() {
        consider(p, p.0 * p.0 + p.1 * p.1);
        for q in &points[i + 1..] {
            // The segment's normal on the far side from the origin:
            // the other one has a score of at most 0.
            let d = (q.0 - p.0, q.1 - p.1);
            let c = d.0 * p.1 - d.1 * p.0;
            if c >= 0.0 {
                consider((-d.1, d.0), c);
            } else {
                consider((d.1, -d.0), -c);
            }
        }
    }
    let (alpha, beta) = best.1?;
    // The member checked with its own bounds.
    let parts = (0..20).all(|i| {
        let (s, f, ef, p, ep) = signed(i);
        let r = alpha * f + beta * p;
        let bound = margin
            + alpha.abs() * ef
            + beta.abs() * ep
            + 4.0 * f64::EPSILON * (alpha.abs() * f.abs() + beta.abs() * p.abs());
        s * r > bound
    });
    parts.then_some((alpha / g, beta))
}

/// Two patches sharing a curved edge as the cylinder and pencil rules see
/// them: the edge and the coefficients of `F` and `P` on each, `a` first.
struct CurvedPair {
    edge: CurvedEdge,
    a: [Coefficient; 10],
    b: [Coefficient; 10],
}

impl CurvedPair {
    /// The edge `ea` of `a`, shared with `b` as its edge `eb`
    /// ([`CurvedEdge::new`]), and both patches' coefficients
    /// ([`CurvedEdge::coefficients`]).
    fn new(a: &Patch, ea: usize, b: &Patch, eb: usize, margin: f64) -> Option<Self> {
        let edge = CurvedEdge::new(a, ea, b, eb, margin)?;
        let (ka, kb) = (edge.coefficients(a, ea)?, edge.coefficients(b, eb)?);
        Some(CurvedPair { edge, a: ka, b: kb })
    }
}

/// What the cylinder and pencil rules know about a curved edge `P, C, Q`
/// of weight `w` shared by two patches: its control triangle's edges from
/// `C`, their normal `n` and the sizes the rounding bounds use.
#[derive(Debug, Clone, Copy)]
struct CurvedEdge {
    c: DVec3,
    w2: f64,
    e1: DVec3,
    e2: DVec3,
    n: DVec3,
    nn: f64,
    root: f64,
    l1: f64,
    l2: f64,
    kappa: f64,
    reach: f64,
}

/// One Bernstein coefficient (rows 1 to 4) of [`cylinder_apart`]'s `F`
/// and [`pencil_member`]'s `P` over the patch's `W²`: the ratios `f`, `p`
/// and bounds `ef`, `ep` on their rounding.
#[derive(Debug, Clone, Copy, Default)]
struct Coefficient {
    f: f64,
    ef: f64,
    p: f64,
    ep: f64,
}

impl CurvedEdge {
    /// The edge `ea` of `a`, if `b`'s edge `eb` is the same (bit for bit,
    /// run the other way), it isn't straight within `margin` and its
    /// control triangle isn't nearly degenerate.
    fn new(a: &Patch, ea: usize, b: &Patch, eb: usize, margin: f64) -> Option<Self> {
        let (p, c, q, w) = (a.p[ea], a.c[ea], a.p[(ea + 1) % 3], a.w[ea]);
        let same_edge = b.p[eb] == q && b.p[(eb + 1) % 3] == p && b.c[eb] == c && b.w[eb] == w;
        if !same_edge || straight(p, c, q, margin) {
            return None;
        }
        let (e1, e2) = (p - c, q - c);
        let n = e1.cross(e2);
        let nn = n.dot(n);
        if nn.is_nan() || nn <= 1e-24 * e1.length_squared() * e2.length_squared() {
            return None;
        }
        let root = nn.sqrt();
        let (l1, l2) = (e1.length(), e2.length());
        Some(CurvedEdge {
            c,
            w2: w * w,
            e1,
            e2,
            n,
            nn,
            root,
            l1,
            l2,
            kappa: l1 * l2 / root,
            reach: l1.max(l2) / root,
        })
    }

    /// The larger of the far ends' heights above the lines through the
    /// other end and `C` (`hQ`, the height of `Q` above the line through
    /// `P` and `C`, or `hP`): `|∇F|` is `4w²` over it, its value at the end
    /// where it is smaller.
    fn height(&self) -> f64 {
        (self.root / self.l1).max(self.root / self.l2)
    }

    /// The coefficients of `F` and `P` on patch `x`, whose edge `e` is
    /// this one, rows 1 to 4 in a fixed order; `None` if a weights'
    /// coefficient isn't positive.
    fn coefficients(&self, x: &Patch, e: usize) -> Option<[Coefficient; 10]> {
        let CurvedEdge {
            c,
            w2,
            e1,
            e2,
            n,
            nn,
            root,
            kappa,
            reach,
            ..
        } = *self;
        // Barycentric coordinates (λP, λC, λQ) of a homogeneous point
        // `(s, ω)`, `s = ω·(x − C)`, projected onto the control triangle,
        // the largest's size and a bound on their rounding errors; and its
        // height `ν` over the triangle's plane, times `ω`, with its bound.
        let lambda = |s: DVec3, omega: f64| {
            let lp = n.dot(s.cross(e2)) / nn;
            let lq = n.dot(e1.cross(s)) / nn;
            let lc = omega - lp - lq;
            let m = lp.abs().max(lc.abs()).max(lq.abs());
            let length = s.length();
            let e = LAMBDA_ROUNDING * kappa * (length * reach + m + omega);
            let nu = n.dot(s) / root;
            let enu = NU_ROUNDING * kappa * length;
            ([lp, lc, lq], m, e, nu, enu)
        };
        // F's polar form, on the points' coordinates.
        let polar = |[xp, xc, xq]: [f64; 3], [yp, yc, yq]: [f64; 3]| {
            xc * yc - 2.0 * w2 * (xp * yq + xq * yp)
        };
        let (next, last) = ((e + 1) % 3, (e + 2) % 3);
        // Each control point's coordinates (with their size and error
        // bound, and its height with its bound) and its weight.
        let corner = |i: usize| (lambda(x.p[i] - c, 1.0), 1.0);
        let edge = |i: usize| (lambda((x.c[i] - c) * x.w[i], x.w[i]), x.w[i]);
        // Row 0's three (the edge's start, its control point, its end)
        // first, their multi-indices over (the edge's start, its end, the
        // far corner) alongside.
        let net = [
            corner(e),
            edge(e),
            corner(next),
            edge(last),
            edge(next),
            corner(last),
        ];
        let index: [[usize; 3]; 6] = [
            [2, 0, 0],
            [1, 1, 0],
            [0, 2, 0],
            [1, 0, 1],
            [0, 1, 1],
            [0, 0, 2],
        ];
        // The quadratic multinomials: 2 for a mixed index, else 1.
        let multi = |g: [usize; 3]| if g.contains(&1) { 2.0 } else { 1.0 };
        // Coefficients scaled by the quartic multinomial, by the first
        // two exponents, and bounds on their rounding errors (for F but
        // for the factor `1 + 4w²`). Pairs within row 0 only add to row 0,
        // which is skipped.
        let mut num = [[0.0; 5]; 5];
        let mut den = [[0.0; 5]; 5];
        let mut size = [[0.0; 5]; 5];
        let mut height = [[0.0; 5]; 5];
        let mut height_size = [[0.0; 5]; 5];
        for i in 0..6 {
            for j in i.max(3)..6 {
                let (((xa, ma, da, na, ena), wa), ((xb, mb, db, nb, enb), wb)) = (net[i], net[j]);
                let (ga, gb) = (index[i], index[j]);
                // A pair of different points comes twice.
                let k = multi(ga) * multi(gb) * if i == j { 1.0 } else { 2.0 };
                let (g0, g1) = (ga[0] + gb[0], ga[1] + gb[1]);
                num[g0][g1] += k * polar(xa, xb);
                den[g0][g1] += k * wa * wb;
                size[g0][g1] += k * (da * mb + ma * db + da * db + CYLINDER_ROUNDING * ma * mb);
                height[g0][g1] += k * (0.5 * (na * wb + nb * wa));
                height_size[g0][g1] += k
                    * (0.5 * (ena * wb + enb * wa)
                        + CYLINDER_ROUNDING * (na.abs() * wb + nb.abs() * wa));
            }
        }
        let mut out = [Coefficient::default(); 10];
        let mut slot = out.iter_mut();
        for g0 in 0..4 {
            for g1 in 0..4 - g0 {
                let d = den[g0][g1];
                // NaN fails the comparison.
                let positive = d > 0.0;
                if !positive {
                    return None;
                }
                *slot.next()? = Coefficient {
                    f: num[g0][g1] / d,
                    ef: (1.0 + 4.0 * w2) * size[g0][g1] / d,
                    p: height[g0][g1] / d,
                    ep: height_size[g0][g1] / d,
                };
            }
        }
        Some(out)
    }
}

/// A bound on the rounding of [`cylinder_apart`]'s coordinates `λ`,
/// times `κ·(d·reach + m + ω)` (see there). With `u = ε/2`, rounding the
/// normal `n` is off by about `7·u·|P − C|·|Q − C| = 7·u·κ·|n|`. In `λP =
/// n·(s × Q − C)/n²` that moves the numerator by about `7·u·κ·d·reach`
/// of `n²` (`n` tilting under a point off the plane) and `n²` by about
/// `14·u·κ` of itself; with the other roundings, `λP` and `λQ` are off by
/// at most about `u·κ·(18·d·reach + 19·m)`, and `λC = ω − λP − λQ` by
/// twice that and `2·u·(ω + 2m)`: `64·ε = 128·u` has room.
const LAMBDA_ROUNDING: f64 = 64.0 * f64::EPSILON;

/// A bound on the rounding in [`cylinder_apart`]'s polar forms and sums,
/// relative to the sizes of the coordinates multiplied: a polar form's
/// products and sums round by about `4·u·(1 + 4w²)·mx·my`, adding up to
/// four terms and dividing by `W²_γ` (itself off by about `5·u`) adds a
/// few `u` more. `16·ε = 32·u` has room. [`pencil_member`]'s `P·W²` takes
/// it too, times `|νX|·ωY + |νY|·ωX` (twice the size of its polar form).
const CYLINDER_ROUNDING: f64 = 16.0 * f64::EPSILON;

/// A bound on the rounding of [`pencil_member`]'s heights `ν = n·s/|n|`,
/// times `κ·|s|`. With `u = ε/2`: `n` is off by about `7·u·κ·|n|` (from
/// `P − C`, `Q − C` and the cross product, as for [`LAMBDA_ROUNDING`]),
/// which tilts `n·s` by `7·u·κ·|n|·|s|`; `s` itself is off by `2·u·|s|`,
/// the dot product by `3·u·|n|·|s|`; `|n|` by about `7·u·κ + 2·u` of
/// itself (its square by twice that and the root halving it), moving `ν`
/// (at most `|s|`) by as much, and the division by `u` more. In all under
/// `(14·κ + 8)·u·|s| ≤ 22·u·κ·|s|`: `32·ε = 64·u` has room. Without the
/// bound, flat control triangles far out let `n` tilt enough under
/// rounding to make up certificates.
const NU_ROUNDING: f64 = 32.0 * f64::EPSILON;

/// The unit direction from `p` to `q`, and the part of `x - p` across the
/// line through them, or `None` if they are the same point.
fn across_line(p: DVec3, q: DVec3) -> Option<(DVec3, impl Fn(DVec3) -> DVec3)> {
    let u = (q - p).try_normalize()?;
    Some((u, move |x: DVec3| {
        let d = x - p;
        d - u * d.dot(u)
    }))
}

/// Whether every edge's control point is within `margin` of the line
/// through its ends: the patch is then within `margin` of its flat
/// triangle, and the hull rules on it are about the triangle itself.
///
/// Its hull is not the triangle, though: on a curved surface a piece flat
/// within the resolution still has a hull up to about half of it off the
/// surface. So repair stops splitting a failing pair of non-neighbours
/// only once both are flat within a sixteenth of the resolution
/// ([`FLAT_STOP`](super::repair::FLAT_STOP)).
pub(crate) fn flat(patch: &Patch, margin: f64) -> bool {
    (0..3).all(|i| straight(patch.p[i], patch.c[i], patch.p[(i + 1) % 3], margin))
}

/// Whether the curve from `p` to `q` with control point `c` is straight
/// within `margin`: `c` within `margin` of the line through its ends
/// (which are apart). With a positive weight, the curve is then within
/// `margin` of that line.
pub(crate) fn straight(p: DVec3, c: DVec3, q: DVec3, margin: f64) -> bool {
    across_line(p, q).is_some_and(|(_, across)| across(c).length() <= margin)
}

/// Whether the convex hulls of `a` and `b` (neither empty) are more than
/// `margin` apart, by GJK on their Minkowski difference.
///
/// Each step has the closest point `v` of the current simplex, a point of
/// the difference and so an upper bound on the distance, and the support
/// point `w` furthest along `-v`, which gives the lower bound `v·w / |v|`.
/// It answers apart only once the lower bound clears `margin`, and not
/// apart once `|v|` is within it, or when it stops making progress.
pub(crate) fn apart(a: &[DVec3], b: &[DVec3], margin: f64) -> bool {
    // Measured from a point of `a`, so rounding is relative to the hulls'
    // size and distance, not their distance from the model's origin.
    let origin = a[0];
    let furthest = |points: &[DVec3], d: DVec3| {
        let mut best = points[0] - origin;
        let mut best_dot = best.dot(d);
        for &p in &points[1..] {
            let p = p - origin;
            let dot = p.dot(d);
            if dot > best_dot {
                (best, best_dot) = (p, dot);
            }
        }
        best
    };
    let support = |d: DVec3| furthest(a, d) - furthest(b, -d);

    let mut simplex = Simplex {
        points: [a[0] - b[0]; 4],
        len: 1,
    };
    let mut v = simplex.points[0];
    for _ in 0..MAX_GJK_STEPS {
        let vv = v.dot(v);
        // Also stops on NaN.
        if vv.is_nan() || vv <= margin * margin {
            return false;
        }
        let w = support(-v);
        let vw = v.dot(w);
        if vw > margin * vv.sqrt() {
            return true;
        }
        if vv - vw <= 1e-12 * vv || simplex.points[..simplex.len].contains(&w) {
            return false;
        }
        simplex.points[simplex.len] = w;
        simplex.len += 1;
        (v, simplex) = simplex.closest();
        if simplex.len == 4 {
            // The origin is inside the tetrahedron.
            return false;
        }
    }
    false
}

/// Up to four points of the Minkowski difference.
#[derive(Debug, Clone, Copy)]
struct Simplex {
    points: [DVec3; 4],
    len: usize,
}

impl Simplex {
    /// The point of the simplex's hull closest to the origin, and the
    /// smallest face of the simplex holding it. The last point is the
    /// support point just added, and the face holds it.
    ///
    /// Every face holding the last point (every subset of affinely
    /// independent points with it) is tried: the origin's projection onto
    /// the face's affine hull, kept when its barycentric coordinates are
    /// all non-negative. The closest point lies inside some face and is
    /// that face's projection, so the nearest projection kept is it.
    /// Subsets that are nearly degenerate are skipped; their points are
    /// covered by smaller faces. Ties keep the first found, smaller faces
    /// first.
    ///
    /// Only faces with the last point: the support point `w` was added
    /// because `v·w < v·v`, so the segment from the old closest point `v`
    /// towards `w` comes closer to the origin, and the new closest point
    /// is on a face with `w` (faces without it are the old simplex, no
    /// closer than `v`). When `w` is square to `v` and far out, the
    /// segment comes closer by less than the rounding of a squared length;
    /// trying the old faces too then kept `v` and dropped `w`, and the
    /// next step found `w` again, until GJK gave up.
    fn closest(&self) -> (DVec3, Simplex) {
        let last = 1u32 << (self.len - 1);
        let mut best: Option<(f64, DVec3, Simplex)> = None;
        for mask in (1..(1u32 << self.len)).filter(|mask| mask & last != 0) {
            let mut face = Simplex {
                points: [DVec3::ZERO; 4],
                len: 0,
            };
            for (i, &p) in self.points[..self.len].iter().enumerate() {
                if mask & (1 << i) != 0 {
                    face.points[face.len] = p;
                    face.len += 1;
                }
            }
            if let Some(q) = face.projection() {
                let qq = q.dot(q);
                let better = match &best {
                    Some((d, _, s)) => qq < *d || (qq == *d && face.len < s.len),
                    None => true,
                };
                if better {
                    best = Some((qq, q, face));
                }
            }
        }
        // The last point alone always projects to itself.
        let (_, q, face) = best.expect("a closest point");
        (q, face)
    }

    /// The origin's projection onto the affine hull of the points, if its
    /// barycentric coordinates are all non-negative and the points are
    /// clearly affinely independent.
    ///
    /// Triangles and tetrahedra are solved with cross and triple products
    /// of the edges, not the Gram matrix: that squares the conditioning,
    /// and on a long thin triangle (a hull thousands of times longer than
    /// it is wide) lost enough digits to put the point off the closest, so
    /// that GJK stopped short and called hulls far apart not apart.
    fn projection(&self) -> Option<DVec3> {
        let x0 = self.points[0];
        let e = [1, 2, 3].map(|i| self.points[i.min(self.len - 1)] - x0);
        match self.len {
            1 => Some(x0),
            2 => {
                let g = e[0].dot(e[0]);
                if g.is_nan() || g <= 0.0 {
                    return None;
                }
                let mu = -e[0].dot(x0) / g;
                (0.0..=1.0).contains(&mu).then(|| x0 + e[0] * mu)
            }
            3 => {
                // With n = e0 × e1, x0 + μ0·e0 + μ1·e1 is the projection
                // for μ0 = -n·(x0 × e1)/n² and μ1 = -n·(e0 × x0)/n².
                let n = e[0].cross(e[1]);
                let nn = n.dot(n);
                if nn.is_nan() || nn <= DEGENERATE * DEGENERATE * e[0].dot(e[0]) * e[1].dot(e[1]) {
                    return None;
                }
                let mu0 = -n.dot(x0.cross(e[1])) / nn;
                let mu1 = -n.dot(e[0].cross(x0)) / nn;
                if mu0 < 0.0 || mu1 < 0.0 || mu0 + mu1 > 1.0 {
                    return None;
                }
                // The point itself straight from the normal, which rounds
                // relative to its distance rather than to x0's.
                Some(n * (n.dot(x0) / nn))
            }
            _ => {
                // x0 + Σ μi·ei = 0 by Cramer's rule on the edges.
                let det = e[0].dot(e[1].cross(e[2]));
                let size = e[0].length() * e[1].length() * e[2].length();
                if det.is_nan() || det.abs() <= DEGENERATE * size {
                    return None;
                }
                let r = -x0;
                let mu = [
                    r.dot(e[1].cross(e[2])) / det,
                    e[0].dot(r.cross(e[2])) / det,
                    e[0].dot(e[1].cross(r)) / det,
                ];
                if mu.iter().any(|&m| m < 0.0) || mu.iter().sum::<f64>() > 1.0 {
                    return None;
                }
                Some(DVec3::ZERO)
            }
        }
    }
}

/// Faces of a simplex flatter than this (the sine of the angle between
/// two edges, or the volume of three over their lengths' product) are
/// skipped: their points lie within that much of a smaller face.
const DEGENERATE: f64 = 1e-12;

#[cfg(test)]
mod tests;
