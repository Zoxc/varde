//! Two quadrics of revolution on one axis (cylinders, cones, spheres, by
//! their faces' forms): they meet only in **parallels**, circles square
//! to the axis round it, and a circle's arc is a rational quadratic edge
//! exactly. Which of the pair's patches lie on such forms, and on one
//! axis, is told within the resolution: a choice of path, not a
//! decision. The arcs are checked against both patches before they are
//! taken, and nothing about the topology comes from it.
//!
//! The forms are read rather than the face tags: a cone too nearly flat
//! for its quadric to be measured to the resolution claims none, though
//! its patches lie on it exactly.

use glam::DVec3;

use super::chain::trace::invert;
use super::surface::MAX_TURN_COS;
use crate::mesh::{Form, samples};
use crate::patch::{Conic3, Patch};

/// A line, through `point` along the unit `dir`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Axis {
    pub(super) point: DVec3,
    pub(super) dir: DVec3,
}

impl Axis {
    /// `x` square to the axis: from the axis to `x`.
    fn across(&self, x: DVec3) -> DVec3 {
        let r = x - self.point;
        r - self.dir * self.dir.dot(r)
    }

    /// How far along the axis `x` is.
    fn height(&self, x: DVec3) -> f64 {
        self.dir.dot(x - self.point)
    }
}

/// What a quadric of revolution turns about: a line (a cylinder's or a
/// cone's axis), or for a sphere only its centre (any line through it).
#[derive(Debug, Clone, Copy)]
enum Round {
    Line(Axis),
    Centre(DVec3),
}

impl Round {
    fn of(form: &Form) -> Option<Round> {
        let line = |point: DVec3, axis: DVec3| {
            let dir = axis.try_normalize()?;
            point
                .is_finite()
                .then_some(Round::Line(Axis { point, dir }))
        };
        match *form {
            Form::Cylinder { point, axis, .. } => line(point, axis),
            Form::Cone { apex, axis, .. } => line(apex, axis),
            Form::Sphere { centre, .. } => centre.is_finite().then_some(Round::Centre(centre)),
            _ => None,
        }
    }
}

/// The axis two patches' forms both turn about, if each is a quadric of
/// revolution its patch lies on (within `resolution` at the sampled
/// points: not a fitted patch, which is on its form only within the fit
/// tolerance) and the two lines are one within `resolution` where the
/// patches are: at each of `at` (the cut's ends) the two axes' nearest
/// points are that near each other, and over `reach` (how far any point
/// of either patch is from the axis, at most) their directions part by
/// no more. A sphere turns about the other's axis where its centre is
/// that near it; two spheres aren't taken (their axis, through both
/// centres, is told badly where those are near).
///
/// The first form's axis is the one given.
pub(super) fn common_axis(
    forms: [&Form; 2],
    patches: [&Patch; 2],
    at: &[DVec3],
    resolution: f64,
) -> Option<Axis> {
    let rounds = [Round::of(forms[0])?, Round::of(forms[1])?];
    let on = |k: usize| samples().all(|u| forms[k].distance(patches[k].eval(u)) <= resolution);
    if !(on(0) && on(1)) {
        return None;
    }
    let near_line = |axis: &Axis, x: DVec3| axis.across(x).length() <= resolution;
    let axis = match rounds {
        [Round::Line(a), Round::Line(b)] => {
            let reach = patches
                .iter()
                .flat_map(|p| p.p.iter().chain(&p.c))
                .map(|&x| a.across(x).length())
                .fold(0.0, f64::max);
            let tilt = a.dir.cross(b.dir).length();
            let together = at.iter().all(|&x| {
                let (fa, fb) = (x - a.across(x), x - b.across(x));
                fa.distance(fb) <= resolution
            });
            (tilt * reach <= resolution && together).then_some(a)
        }
        [Round::Line(a), Round::Centre(c)] => near_line(&a, c).then_some(a),
        [Round::Centre(c), Round::Line(b)] => near_line(&b, c).then_some(Axis { point: c, ..b }),
        [Round::Centre(_), Round::Centre(_)] => None,
    }?;
    Some(axis)
}

/// The axis about which a plane with unit normal `n` cuts `form` in a
/// parallel, where `form` is a cone square to the plane (its axis along
/// `n` to `1e-12`) or a sphere (any plane: the line through its centre
/// along `n`) and `patch` lies on it within `resolution` at the sampled
/// points: then the plane's section is a circle round that axis, whose
/// arcs [`parallel`] gives by their angles, exact in relative terms
/// however short, where the weight from a point on the curve is noise for
/// an arc bulging by a rounding. Cylinders aren't taken: their sections
/// have their angles' weights already.
pub(super) fn square_axis(form: &Form, n: DVec3, patch: &Patch, resolution: f64) -> Option<Axis> {
    let axis = match Round::of(form)? {
        Round::Centre(point) => Axis {
            point,
            dir: n.try_normalize()?,
        },
        Round::Line(axis) if matches!(form, Form::Cone { .. }) => {
            (axis.dir.cross(n).length() <= 1e-12).then_some(axis)?
        }
        Round::Line(_) => return None,
    };
    samples()
        .all(|u| form.distance(patch.eval(u)) <= resolution)
        .then_some(axis)
}

/// The least slope, `|dρ/dh|` against each other, at which a cylinder's
/// and a cone's meridians (or two cones') on one axis cross clearly
/// enough for [`walls`] to certify their pair: a thousandth. Within the
/// resolution of one axis, their cut is a curve round it whose height
/// wanders from the parallel by up to the axes' offset over the slope,
/// one for each turn about the axis, as long as the slope outweighs the
/// axes' tilt; shallower crossings are refined as before.
const MIN_SLOPE: f64 = 1e-3;

/// A certificate for a pair of patches (see `pairs`): where both lie on
/// cylinders or cones ([`common_axis`]) on one axis whose meridians cross
/// at least [`MIN_SLOPE`] apart, and one of them lies on one side of a
/// plane through the axis (its control points do, so its hull does), they
/// meet in no closed loop: the two surfaces meet in one curve going once
/// round the axis (a parallel, within the resolution), which no patch on
/// one side of such a plane holds whole, so every arc of it in the pair
/// runs out through ends. Then the ends join along it in turn
/// ([`along`]). Returns that patch's direction out from the axis, the
/// axis, and the ends' allowed spread in height.
pub(super) fn walls(forms: [&Form; 2], patches: [&Patch; 2], resolution: f64) -> Option<Walls> {
    // How fast the radius grows along `dir`, nearly either form's axis.
    let slope = |form: &Form, dir: DVec3| match *form {
        Form::Cylinder { .. } => Some(0.0),
        Form::Cone { axis, cos, sin, .. } => {
            let tan = sin / cos;
            tan.is_finite().then(|| tan * axis.dot(dir).signum())
        }
        _ => None,
    };
    // Told before the samples: two cylinders, the commonest pair, never
    // cross.
    let Round::Line(first) = Round::of(forms[0])? else {
        return None;
    };
    let slope = (slope(forms[0], first.dir)? - slope(forms[1], first.dir)?).abs();
    if slope.is_nan() || slope < MIN_SLOPE {
        return None;
    }
    let corners: Vec<DVec3> = patches.iter().flat_map(|p| p.p).collect();
    let axis = common_axis(forms, patches, &corners, resolution)?;
    // A patch on one side of a plane through the axis: every control
    // point's direction from the axis within a right angle of one.
    let side = patches.iter().find_map(|patch| {
        let out = axis
            .across(patch.eval(DVec3::splat(1.0 / 3.0)))
            .try_normalize()?;
        patch
            .p
            .iter()
            .chain(&patch.c)
            .all(|&x| out.dot(axis.across(x)) > 0.0)
            .then_some(out)
    })?;
    Some(Walls {
        axis,
        out: side,
        spread: 4.0 * resolution / slope + resolution,
    })
}

/// What [`walls`] certified a pair with.
#[derive(Debug, Clone, Copy)]
pub(super) struct Walls {
    axis: Axis,
    /// A direction from the axis every end's is within a right angle of.
    out: DVec3,
    /// How far apart in height the ends may be, on one curve round the
    /// axis.
    spread: f64,
}

/// The ends of a pair [`walls`] certified (where they are and their
/// signs), joined along the curve the two surfaces meet in: in order
/// round the axis, each with the next, which must be of the other sign,
/// as their stretches inside both patches run (by index into `ends`).
/// `None` where they don't pair up so, or lie further apart in height
/// than one such curve: the pair is refined as before.
pub(super) fn along(walls: &Walls, ends: &[(DVec3, i8)]) -> Option<Vec<(usize, usize)>> {
    if !ends.len().is_multiple_of(2) {
        return None;
    }
    let heights = ends.iter().map(|e| walls.axis.height(e.0));
    let (low, high) = heights.fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), x| {
        (l.min(x), h.max(x))
    });
    if (high - low).is_nan() || high - low > walls.spread {
        return None;
    }
    // Round the axis: the tangent of the angle from `out`, within a right
    // angle of it either way.
    let turn = walls.axis.dir.cross(walls.out);
    let mut order: Vec<(f64, usize)> = Vec::with_capacity(ends.len());
    for (k, e) in ends.iter().enumerate() {
        let r = walls.axis.across(e.0);
        let (x, y) = (r.dot(walls.out), r.dot(turn));
        if x.is_nan() || x <= 0.0 {
            return None;
        }
        order.push((y / x, k));
    }
    order.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let joined: Vec<(usize, usize)> = order.chunks(2).map(|c| (c[0].1, c[1].1)).collect();
    joined
        .iter()
        .all(|&(x, y)| ends[x].1 + ends[y].1 == 0)
        .then_some(joined)
}

/// How often [`parallel`] may halve an arc.
const MAX_DEPTH: u32 = 6;

/// The arc from `x` to `y` of the parallel through them round `axis`, the
/// shorter way, as exact rational quadratic arcs (halved where one would
/// turn by more than about 45°, as plane sections are): `None` where the
/// two aren't on one parallel (their heights along the axis, or their
/// distances from it, more than `resolution` apart), lie on the axis, or
/// are half a turn apart.
///
/// Each arc's weight is `cos(Δφ/2) = |r̂x + r̂y| / 2` and its control
/// point `m + tan²(Δφ/2)·(m − m̂)`, `r̂` the unit vectors from the axis to
/// the ends, `m` the chord's middle and `m̂` its foot on the axis,
/// `tan²(Δφ/2) = |r̂x − r̂y|² / |r̂x + r̂y|²`: only `+ − × ÷ √`, exact in
/// relative terms however short the arc. Halves meet at the turn's middle
/// direction, at the ends' mean height and distance.
pub(super) fn parallel(axis: &Axis, x: DVec3, y: DVec3, resolution: f64) -> Option<Vec<Conic3>> {
    let (hx, hy) = (axis.height(x), axis.height(y));
    let (fx, fy) = (axis.across(x), axis.across(y));
    let (rx, ry) = (fx.length(), fy.length());
    let apart = (hx - hy).abs().max((rx - ry).abs());
    if !(apart <= resolution && rx > resolution && ry > resolution) {
        return None;
    }
    let mut out = Vec::new();
    arcs(axis, (x, fx / rx), (y, fy / ry), 0, &mut out)?;
    Some(out)
}

fn arcs(
    axis: &Axis,
    (x, ux): (DVec3, DVec3),
    (y, uy): (DVec3, DVec3),
    depth: u32,
    out: &mut Vec<Conic3>,
) -> Option<()> {
    let (sum, diff) = (ux + uy, ux - uy);
    let s = sum.length();
    // Half a turn, or past it: no shorter way.
    if s.is_nan() || s <= 1e-6 {
        return None;
    }
    if ux.dot(uy) < MAX_TURN_COS {
        if depth >= MAX_DEPTH {
            return None;
        }
        let um = sum / s;
        let (fx, fy) = (axis.across(x), axis.across(y));
        let r = 0.5 * (fx.length() + fy.length());
        let h = 0.5 * (axis.height(x) + axis.height(y));
        let mid = axis.point + axis.dir * h + um * r;
        arcs(axis, (x, ux), (mid, um), depth + 1, out)?;
        return arcs(axis, (mid, um), (y, uy), depth + 1, out);
    }
    let m = (x + y) * 0.5;
    let tan2 = diff.length_squared() / sum.length_squared();
    let c = m + axis.across(m) * tan2;
    let w = 0.5 * s;
    (c.is_finite() && w.is_finite() && x != y).then(|| out.push(Conic3 { p0: x, c, w, p1: y }))
}

/// Whether `curve` lies on `patch` within `resolution` at `¼`, `½` and
/// `¾` (each point inverted from the domain positions `from` → `to` of its
/// ends there): on a quadric the implicit value along a conic is a
/// quartic over the weight's square, vanishing at the ends, so three
/// samples bound it along the whole arc, and the material between the arc
/// and the true cut where it isn't is no thicker than a few resolutions.
pub(super) fn on_patch(
    patch: &Patch,
    curve: &Conic3,
    from: DVec3,
    to: DVec3,
    resolution: f64,
) -> bool {
    [0.25, 0.5, 0.75].into_iter().all(|t| {
        let x = curve.eval(t);
        let u = invert(patch, x, from * (1.0 - t) + to * t);
        u.min_element() >= -1e-6 && patch.eval(u).distance(x) <= resolution
    })
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]
mod tests {
    use super::*;

    #[test]
    fn parallels_are_exact_circle_arcs() {
        let dir = DVec3::new(0.3, -0.5, 0.8).normalize();
        let axis = Axis {
            point: DVec3::new(1.5, -0.7, 2.0),
            dir,
        };
        let e1 = dir.any_orthonormal_vector();
        let e2 = dir.cross(e1);
        let (r, h) = (1.3, 0.7);
        let at = |phi: f64| axis.point + dir * h + (e1 * phi.cos() + e2 * phi.sin()) * r;
        for (a, b) in [(0.1, 0.2), (0.0, 1.5), (-1.0, 2.0), (0.3, 0.3 + 1e-6)] {
            let arcs = parallel(&axis, at(a), at(b), 1e-9).unwrap();
            assert_eq!(arcs[0].p0, at(a));
            assert_eq!(arcs.last().unwrap().p1, at(b));
            for arc in &arcs {
                assert!(arc.w > 0.7);
                for k in 0..=16 {
                    let p = arc.eval(k as f64 / 16.0);
                    assert!((axis.across(p).length() - r).abs() < 1e-14, "{p}");
                    assert!((axis.height(p) - h).abs() < 1e-14, "{p}");
                }
            }
        }
        // Not on one parallel, or half a turn apart.
        assert!(parallel(&axis, at(0.0), at(1.0) + dir * 1e-6, 1e-9).is_none());
        assert!(parallel(&axis, at(0.0), at(std::f64::consts::PI), 1e-9).is_none());
    }
}
