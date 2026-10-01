//! Swept strips: the two patches between two stations of a swept
//! surface, exactly where the surface allows.
//!
//! A strip runs between a **bottom** curve from `a0` to `a1` and a
//! **top** from `b0` to `b1`, joined by a **left** edge from `a0` to `b0`
//! and a **right** one from `a1` to `b1`. Its patches are `(a0, a1, b1)`
//! and `(a0, b1, b0)`, as
//! [`cylinder_strip`](crate::patch::cylinder_strip) makes them, and the
//! **diagonal** from `a0` to `b1` is the one curve a strip makes; the
//! other four are given, so strips beside each other share them (one
//! edge record each).
//!
//! Why the strips here are exact: a rational quadratic triangle lies on a
//! quadric when its three sides are conics on it whose planes meet in one
//! point `O` of the quadric (the patch is then the inverse of the
//! projection from `O`). A straight side is a degenerate conic whose plane
//! turns freely about it, but its parametrization must be the one the
//! projection from `O` induces.
//!
//! - **Cone rulings.** On a cone the projection from any `O` takes a
//!   ruling to a curve through the apex twice over (the plane through `O`
//!   and the ruling cuts the cone in that ruling and the one through `O`,
//!   which meet at the apex), so the ruling's distance from the apex is a
//!   perfect square along it: `((1 − t)·√da + t·√db)²`. That is weight 1
//!   with the control point at the geometric mean `√(da·db)` from the
//!   apex ([`cone_ruling`]), whatever `O` is, so the patches either side
//!   of a ruling (each with its own `O`) share it. (A cylinder's rulings,
//!   the apex at infinity, have it at the midpoint.)
//! - **The diagonal** lies in the plane through `a0` and `b1` along the
//!   bottom's bisecting direction `m = c − (a0 + a1)/2` (on a parallel of
//!   a surface of revolution: square to the axis, through the arc's
//!   middle). That plane meets the bottom's plane in the line through `a0`
//!   along `m`, which cuts the bottom's conic again at `a1'` (on a
//!   parallel: half a turn round from `a1`), and the top's plane in the
//!   line through `b1` along `m`, cutting the top at `b0'`. So the first
//!   patch's planes meet at `a1'`, on the surface, when the right edge's
//!   plane holds it: a cone's ruling (any plane), or a meridian of a
//!   surface of revolution (its plane holds the axis, and `a1'`). Likewise
//!   the second patch's at `b0'`. A cylinder strip's diagonal, the bottom
//!   sheared along the rulings, lies in that same plane.
//!
//! [`cone_strip`] covers any cone over a conic (circular, oblique,
//! elliptic: the bottom and top homothetic about the apex), its diagonal
//! the bottom projected from the apex into that plane.
//! [`revolution_strip`] covers the quadrics of revolution (spheres,
//! ellipsoids, paraboloids, hyperboloids, and cones and cylinders too)
//! between two parallels and two meridians, its diagonal the conic in that
//! plane through `a0` and `b1`, tangent to the surface there, through
//! `a1'`.

use glam::{DVec3, DVec4};

use crate::patch::{Conic, Conic3, Patch, PatchError, Point};

/// How far the diagonal's plane must keep from the apex, or two tangents
/// from parallel, as a sine.
const MIN_SINE: f64 = 1e-9;

/// The straight ruling of a cone from `p` to `q`, which lie on one line
/// through `apex`, on the same side of it: weight 1, with the control
/// point on the line at the geometric mean `√(dp·dq)` of their distances
/// from the apex (see the [module](self) docs). Written symmetrically,
/// `apex + ((p − apex)·√(dq/dp) + (q − apex)·√(dp/dq))/2`, so the ruling
/// from `q` to `p` has the same bits.
///
/// Refuses `p` or `q` at the apex, or on opposite sides of it, with
/// [`PatchError::Degenerate`]; that the three are on one line is the
/// caller's (a face tag checks the patches built on it).
pub fn cone_ruling<P: Point>(p: P, q: P, apex: P) -> Result<Conic<P>, PatchError> {
    let (u, v) = (p - apex, q - apex);
    let (dp, dq) = (u.dot(u).sqrt(), v.dot(v).sqrt());
    if !(dp > 0.0 && dq > 0.0 && u.dot(v) > 0.0) || p == q {
        return Err(PatchError::Degenerate);
    }
    let (rp, rq) = (dp.sqrt(), dq.sqrt());
    let c = apex + (u * (rq / rp) + v * (rp / rq)) * 0.5;
    Conic::new(p, c, 1.0, q)
}

/// The strip of the cone over `bottom` with its apex at `apex`, up to
/// `top`, which must be `bottom` scaled about the apex (as the caller
/// promises; the rulings are built from the ends): its rulings by
/// [`cone_ruling`], its diagonal `bottom` projected from the apex onto
/// the plane through `a0` and `b1` along the bottom's bisecting
/// direction. Exact on any cone over a conic (see the [module](self)
/// docs): circular, oblique (a ruled loft between two circles on parallel
/// planes) or elliptic.
///
/// Refuses a straight bottom (the strip would be flat: a plane strip's
/// business), an apex too close to the diagonal's plane, or a bottom whose
/// control point the projection takes to infinity, with
/// [`PatchError::Degenerate`].
pub fn cone_strip(bottom: &Conic3, top: &Conic3, apex: DVec3) -> Result<[Patch; 2], PatchError> {
    cone_strip_along(bottom, top, apex, bisector(bottom)?)
}

/// [`cone_strip`] with the diagonal in the plane through `a0` and `b1`
/// along `along`, any such plane clear of the apex: exact for every one.
pub(crate) fn cone_strip_along(
    bottom: &Conic3,
    top: &Conic3,
    apex: DVec3,
    along: DVec3,
) -> Result<[Patch; 2], PatchError> {
    bottom.check()?;
    top.check()?;
    let (a0, a1, b0, b1) = (bottom.p0, bottom.p1, top.p0, top.p1);
    let left = cone_ruling(a0, b0, apex)?;
    let right = cone_ruling(a1, b1, apex)?;
    let mut n = (b1 - a0).cross(along);
    let mut k = n.dot(a0 - apex);
    if k < 0.0 {
        (n, k) = (-n, -k);
    }
    if !above(k, MIN_SINE * n.length() * (a0 - apex).length()) {
        return Err(PatchError::Degenerate);
    }
    // x ↦ apex + k·(x − apex)/(n·(x − apex)), homogeneously: linear in
    // (w·x, w), so the conic's homogeneous control points map to the
    // projected conic's.
    let project = |h: DVec4| {
        let rel = h.truncate() - apex * h.w;
        let depth = n.dot(rel);
        (apex * depth + rel * k).extend(depth)
    };
    let image = Conic3::from_hom(bottom.hom().map(project))?;
    // The ends are `a0` and `b1` up to rounding; the corners are those.
    let diagonal = Conic3::new(a0, image.c, image.w, b1)?;
    strip(bottom, top, &left, &right, &diagonal)
}

/// The strip of a quadric of revolution about the line through `origin`
/// along `axis` between two parallels, `bottom` from `a0` to `a1` and
/// `top` from `b0` to `b1` (circular arcs square to the axis, `top` turned
/// through the same angle), and two meridians, `left` from `a0` to `b0`
/// and `right` from `a1` to `b1` (plane sections through the axis, one
/// turned onto the other; on a cone, rulings by [`cone_ruling`]). The
/// diagonal is the conic in the plane through `a0`, `b1` and `a1'`, `a1`
/// turned half a turn about the axis (the plane along the bottom's
/// bisecting direction, see the [module](self) docs), that touches the
/// surface at `a0` and `b1` (its tangent planes there are spanned by the
/// edges' tangents) and passes through `a1'`. Exact on spheres,
/// ellipsoids, paraboloids and hyperboloids of revolution, cones and
/// cylinders; how the edges lie is the caller's (a face tag checks the
/// patches).
///
/// `a1'` comes from the axis rather than from the bottom's conic: met
/// again along its bisecting direction, the conic gives it only to about
/// `ε/θ²` of the radius for a piece of angle `θ` (the bisecting direction
/// is the control point less the chord's middle, a sagitta), which tilts
/// the plane; from the axis it is good to a rounding of the coordinates.
///
/// The edges must meet at the corners to the bit
/// ([`PatchError::Mismatch`]); a zero axis, `a1` on it, or a diagonal that
/// can't be made (its tangents parallel, or `a1'` on it) is
/// [`PatchError::Degenerate`].
pub fn revolution_strip(
    bottom: &Conic3,
    top: &Conic3,
    left: &Conic3,
    right: &Conic3,
    origin: DVec3,
    axis: DVec3,
) -> Result<[Patch; 2], PatchError> {
    for edge in [bottom, top, left, right] {
        edge.check()?;
    }
    let (a0, a1, b0, b1) = (bottom.p0, bottom.p1, top.p0, top.p1);
    if left.p0 != a0 || left.p1 != b0 || right.p0 != a1 || right.p1 != b1 {
        return Err(PatchError::Mismatch);
    }
    let axis = axis.try_normalize().ok_or(PatchError::Degenerate)?;
    let foot = origin + axis * (a1 - origin).dot(axis);
    if !above((a1 - foot).length(), MIN_SINE * (a1 - a0).length()) {
        return Err(PatchError::Degenerate);
    }
    let other = foot + (foot - a1);
    let (diagonal, across) = (b1 - a0, other - a0);
    let n = diagonal.cross(across);
    if !above(n.length(), MIN_SINE * diagonal.length() * across.length()) {
        return Err(PatchError::Degenerate);
    }
    // The surface's normals at `a0` and `b1`, from the edges' tangents.
    let n0 = (bottom.c - a0).cross(left.c - a0);
    let n1 = (top.c - b1).cross(right.c - b1);
    let diagonal = conic_touching(a0, n.cross(n0), b1, n.cross(n1), other)?;
    strip(bottom, top, left, right, &diagonal)
}

/// The bottom's bisecting direction, `c − (a0 + a1)/2`: refused for a
/// straight bottom.
fn bisector(bottom: &Conic3) -> Result<DVec3, PatchError> {
    let chord = bottom.p1 - bottom.p0;
    let m = bottom.c - (bottom.p0 + bottom.p1) * 0.5;
    if !above(
        m.cross(chord).length(),
        MIN_SINE * m.length() * chord.length(),
    ) {
        return Err(PatchError::Degenerate);
    }
    Ok(m)
}

/// The conic from `p0` to `p1` whose tangents there run along `t0` and
/// `t1` (either way) and which passes through `x` off the arc between
/// them: its control point where the tangents meet, its weight from `x`'s
/// barycentric coordinates `β` on the control triangle, `|β1| /
/// 2√(β0·β2)` (every point of the conic has `β1² = 4w²·β0·β2`).
fn conic_touching(
    p0: DVec3,
    t0: DVec3,
    p1: DVec3,
    t1: DVec3,
    x: DVec3,
) -> Result<Conic3, PatchError> {
    let chord = p1 - p0;
    let across = t0.cross(t1);
    let square = across.length_squared();
    let floor = MIN_SINE * t0.length() * t1.length();
    if !above(square, floor * floor) {
        return Err(PatchError::Degenerate);
    }
    let c = p0 + t0 * (chord.cross(t1).dot(across) / square);
    // Signed areas in the triangle's plane, against its own.
    let normal = (c - p0).cross(chord);
    let area = |p: DVec3, q: DVec3, r: DVec3| (q - p).cross(r - p).dot(normal);
    let whole = area(p0, c, p1);
    let beta = [area(x, c, p1), area(p0, x, p1), area(p0, c, x)].map(|a| a / whole);
    // On the arc itself (every coordinate positive), or not on a conic
    // with these tangents.
    if !above(beta[0] * beta[2], 0.0) || beta.iter().all(|&b| b > 0.0) {
        return Err(PatchError::Degenerate);
    }
    let w = beta[1].abs() / (2.0 * (beta[0] * beta[2]).sqrt());
    Conic3::new(p0, c, w, p1)
}

/// `x > floor`, false for NaN: the guards here refuse what isn't clearly
/// past their floor.
fn above(x: f64, floor: f64) -> bool {
    x > floor
}

/// The two patches from the five edges.
fn strip(
    bottom: &Conic3,
    top: &Conic3,
    left: &Conic3,
    right: &Conic3,
    diagonal: &Conic3,
) -> Result<[Patch; 2], PatchError> {
    let (a0, a1, b0, b1) = (bottom.p0, bottom.p1, top.p0, top.p1);
    Ok([
        Patch::new(
            [a0, a1, b1],
            [bottom.c, right.c, diagonal.c],
            [bottom.w, right.w, diagonal.w],
        )?,
        Patch::new(
            [a0, b1, b0],
            [diagonal.c, top.c, left.c],
            [diagonal.w, top.w, left.w],
        )?,
    ])
}

#[cfg(test)]
mod tests;
