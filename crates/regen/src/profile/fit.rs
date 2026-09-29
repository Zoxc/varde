//! Fitting a spline piece with a chain of conics, within the tolerance.
//!
//! The piece is cut at its breaks into cubic Bézier segments, exactly
//! (each from its ends and the derivatives there, worked out once per
//! break, so neighbours share them). Each segment is fitted by one conic
//! from its start to its end, its control point where the end tangents
//! meet, so the chain turns smoothly where conics meet, and its weight
//! putting the conic's shoulder (its point at ½) where the cubic crosses
//! the line from the chord's middle to that control point, which about
//! halves the error of a parabola, or else weight 1 (a parabola). A segment the conic misses by more
//! than half the tolerance, or whose tangents don't meet ahead of both
//! ends (an inflection, or a turn past 90°), is halved and each half
//! fitted again, at most [`MAX_DEPTH`] times; a segment nearly straight
//! within a quarter of the tolerance can become a straight conic
//! instead. How far a cubic is from the conic is measured at
//! [`SAMPLES`] points, each by the conic's implicit form over its
//! gradient: in the barycentric coordinates `λ` of the control triangle
//! `p0, c, p1`, a conic of weight `w` is `λ1² = 4w²·λ0·λ2`.

use glam::DVec2;
use varde_kernel::patch::Conic2;
use varde_sketch::BSpline;

use super::{ProfileError, Segments};

/// How often a Bézier segment may be halved: `2^24` pieces is past what
/// a profile may hold, so fitting ends first by [`MAX_PROFILE_SEGMENTS`].
///
/// [`MAX_PROFILE_SEGMENTS`]: varde_kernel::MAX_PROFILE_SEGMENTS
const MAX_DEPTH: u32 = 24;

/// Points along a Bézier segment measured against its conic.
const SAMPLES: usize = 15;

/// How far out of the control triangle, in barycentric terms, a cubic's
/// point may be from rounding.
const SLACK: f64 = 1e-9;

/// The weights a fitted conic may have; past them it's a parabola, or
/// halved. Nearer 1 than the kernel allows: a conic hugging its chord or
/// its control point only fits a cubic that nearly does too, and makes
/// the kernel's work harder.
const W_FIT: (f64, f64) = (0.25, 4.0);

/// A cubic Bézier segment by its control points.
type Bezier = [DVec2; 4];

/// Adds the conics of `part`, run backwards if `reversed`, from `a` to
/// `b`: the vertices its ends are put at.
pub(super) fn spline(
    part: &BSpline,
    reversed: bool,
    a: DVec2,
    b: DVec2,
    curve: u64,
    out: &mut Segments,
) -> Result<(), ProfileError> {
    let breaks = part.breaks();
    let at: Vec<[DVec2; 3]> = breaks.iter().map(|&t| part.eval(t)).collect();
    let mut beziers: Vec<Bezier> = breaks
        .windows(2)
        .zip(at.windows(2))
        .map(|(t, at)| {
            let h = (t[1] - t[0]) / 3.0;
            let ([p, d, _], [q, e, _]) = (at[0], at[1]);
            [p, p + d * h, q - e * h, q]
        })
        .filter(|bezier| bezier.iter().any(|&p| p != bezier[0]))
        .collect();
    if reversed {
        beziers.reverse();
        for bezier in &mut beziers {
            bezier.reverse();
        }
    }
    let Some(first) = beziers.first_mut() else {
        return Err(ProfileError::Missing);
    };
    first[0] = a;
    if let Some(last) = beziers.last_mut() {
        last[3] = b;
    }
    let tolerance = out.fit;
    for bezier in beziers {
        fit(&bezier, tolerance, 0, curve, out)?;
    }
    Ok(())
}

/// Adds the conics fitting `bezier` within `tolerance`, halving it as
/// needed, now halved `depth` times.
fn fit(
    bezier: &Bezier,
    tolerance: f64,
    depth: u32,
    curve: u64,
    out: &mut Segments,
) -> Result<(), ProfileError> {
    if let Some(conic) = conic(bezier, tolerance / 2.0) {
        return out.push(conic, curve);
    }
    if bezier[0] != bezier[3] && flatness(bezier) <= tolerance / 4.0 {
        return out.push(Conic2::line(bezier[0], bezier[3])?, curve);
    }
    if depth >= MAX_DEPTH {
        return Err(ProfileError::Fit);
    }
    let (left, right) = halves(bezier);
    fit(&left, tolerance, depth + 1, curve, out)?;
    fit(&right, tolerance, depth + 1, curve, out)
}

/// The conic from `bezier`'s start to its end along its end tangents and
/// through its shoulder, if it's within `tolerance` of the cubic.
fn conic(bezier: &Bezier, tolerance: f64) -> Option<Conic2> {
    let [p0, .., p1] = *bezier;
    let t0 = [bezier[1], bezier[2], p1]
        .into_iter()
        .find_map(|p| (p - p0).try_normalize())?;
    let t1 = [bezier[2], bezier[1], p0]
        .into_iter()
        .find_map(|p| (p1 - p).try_normalize())?;
    // Turning by less than 90°.
    if t0.dot(t1) <= 0.0 {
        return None;
    }
    // p0 + s·t0 = p1 − u·t1.
    let chord = p1 - p0;
    let det = t0.perp_dot(t1);
    if det.abs() <= 1e-12 {
        return None;
    }
    let s = chord.perp_dot(t1) / det;
    let u = t0.perp_dot(chord) / det;
    if !(s > 0.0 && u > 0.0) {
        return None;
    }
    let c = p0 + t0 * s;
    let middle = p0.midpoint(p1);
    let towards = c - middle;
    let k = (point(bezier, 0.5) - middle).dot(towards) / towards.length_squared();
    if !(k > 0.0 && k < 1.0) {
        return None;
    }
    // Through the shoulder, or else a parabola.
    let shoulder = k / (1.0 - k);
    [shoulder, 1.0]
        .into_iter()
        .filter(|w| (W_FIT.0..=W_FIT.1).contains(w))
        .filter_map(|w| Conic2::new(p0, c, w, p1).ok())
        .find(|conic| {
            (1..=SAMPLES)
                .map(|i| point(bezier, i as f64 / (SAMPLES + 1) as f64))
                .all(|q| distance(conic, q).is_some_and(|d| d <= tolerance))
        })
}

/// About how far `q` is from `conic`, to first order, or `None` if it's
/// outside the control triangle, off the part of the conic between its
/// ends.
fn distance(conic: &Conic2, q: DVec2) -> Option<f64> {
    let (e1, e2) = (conic.c - conic.p0, conic.p1 - conic.p0);
    let area = e1.perp_dot(e2);
    let d = q - conic.p0;
    let l1 = d.perp_dot(e2) / area;
    let l2 = e1.perp_dot(d) / area;
    let l0 = 1.0 - l1 - l2;
    if ![l0, l1, l2].iter().all(|&l| l >= -SLACK) {
        return None;
    }
    let (g1, g2) = (
        DVec2::new(e2.y, -e2.x) / area,
        DVec2::new(-e1.y, e1.x) / area,
    );
    let g0 = -g1 - g2;
    let ww = 4.0 * conic.w * conic.w;
    let f = l1 * l1 - ww * l0 * l2;
    let grad = g1 * (2.0 * l1) - (g0 * l2 + g2 * l0) * ww;
    let length = grad.length();
    (length > 0.0).then(|| f.abs() / length)
}

/// The place on `b` at `t`.
fn point(b: &Bezier, t: f64) -> DVec2 {
    let s = 1.0 - t;
    b[0] * (s * s * s) + b[1] * (3.0 * s * s * t) + b[2] * (3.0 * s * t * t) + b[3] * (t * t * t)
}

/// `b` cut in two at its middle, the halves sharing the middle point.
fn halves(b: &Bezier) -> (Bezier, Bezier) {
    let ab = b[0].midpoint(b[1]);
    let bc = b[1].midpoint(b[2]);
    let cd = b[2].midpoint(b[3]);
    let abc = ab.midpoint(bc);
    let bcd = bc.midpoint(cd);
    let middle = abc.midpoint(bcd);
    ([b[0], ab, abc, middle], [middle, bcd, cd, b[3]])
}

/// How far `b`'s inner control points are from its chord at most, which
/// it strays from the chord at most; infinite if one lies past an end,
/// where it could run back. Its ends are apart.
fn flatness(b: &Bezier) -> f64 {
    let chord = b[3] - b[0];
    let length = chord.length();
    let off = |p: DVec2| {
        let along = chord.dot(p - b[0]) / length;
        if (0.0..=length).contains(&along) {
            chord.perp_dot(p - b[0]).abs() / length
        } else {
            f64::INFINITY
        }
    };
    off(b[1]).max(off(b[2]))
}

#[cfg(test)]
mod tests;
