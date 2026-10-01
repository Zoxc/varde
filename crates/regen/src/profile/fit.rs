//! Fitting a spline piece with a chain of conics, within the tolerance.
//!
//! The piece is cut at its breaks into cubic Bézier segments, exactly
//! (each from its ends and the derivatives there, worked out once per
//! break, so neighbours share them), always in the spline's own
//! direction; a piece run backwards is fitted forwards and its chain
//! reversed after, conic by conic, so both directions give the same bits
//! and neighbouring regions sharing a piece share their walls.
//!
//! The segments are fitted in runs ([`runs`]): from each segment on, one
//! conic for the longest run of segments it fits, growing the run until
//! the next segment doesn't fit. A conic fits a run ([`run`]):
//!
//! - straight, if every control point of the run lies within a quarter of
//!   the tolerance of the chord and between its ends, so by the convex
//!   hull the whole run does; tried first, so a run that is straight
//!   within the tolerance is a line, whatever its detail;
//! - else from its start to its end, its control point where the run's
//!   end tangents meet, its weight putting the conic's shoulder (its
//!   point at ½, where it's furthest from its chord) as far from the
//!   chord as the run's furthest sample, or else weight 1 (a parabola);
//!   it fits if within half the tolerance at [`SAMPLES`] points of each
//!   segment and at each segment's end. Like the conic of a single
//!   segment before runs, this is checked at samples, not proven between
//!   them; a run is sampled as densely as its segments alone would be.
//!
//! Curved conics meet along the same tangent, the spline's at their
//! joint; a line meets its neighbours with a kink of about the tolerance
//! over its length at most. A segment no run fits, not even alone, is
//! fitted on its own ([`fit`]): a line if nearly straight within a
//! quarter of the tolerance, else the conic as above with its shoulder
//! at its point at ½; or halved and each half fitted again, at most
//! [`MAX_DEPTH`] times, if the conic misses by more than half the
//! tolerance or the tangents don't meet ahead of both ends (an
//! inflection, or a turn past 90°). How far a cubic is from the conic is
//! measured by the conic's implicit form over its gradient: in the
//! barycentric coordinates `λ` of the control triangle `p0, c, p1`, a
//! conic of weight `w` is `λ1² = 4w²·λ0·λ2`.

use glam::DVec2;
use varde_kernel::MAX_PROFILE_SEGMENTS;
use varde_kernel::patch::Conic2;
use varde_sketch::BSpline;

use super::{ProfileError, Segments};

/// How often a Bézier segment may be halved: `2^24` pieces is past what
/// a profile may hold, so fitting ends first by [`MAX_PROFILE_SEGMENTS`].
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
    // The vertices at the spline's parameter ends, whichever way it's run.
    let (start, end) = if reversed { (b, a) } else { (a, b) };
    let Some(first) = beziers.first_mut() else {
        return Err(ProfileError::Missing);
    };
    first[0] = start;
    if let Some(last) = beziers.last_mut() {
        last[3] = end;
    }
    let mut chain = Chain {
        conics: Vec::new(),
        room: MAX_PROFILE_SEGMENTS.saturating_sub(out.count),
    };
    runs(&beziers, out.fit, &mut chain)?;
    if reversed {
        for conic in chain.conics.iter().rev() {
            out.push(conic.reversed(), curve)?;
        }
    } else {
        for conic in chain.conics {
            out.push(conic, curve)?;
        }
    }
    Ok(())
}

/// The conics of a piece fitted so far, at most `room` of them.
struct Chain {
    conics: Vec<Conic2>,
    /// How many more segments the profile may take.
    room: usize,
}

impl Chain {
    fn push(&mut self, conic: Conic2) -> Result<(), ProfileError> {
        if self.conics.len() >= self.room {
            return Err(ProfileError::TooManySegments);
        }
        self.conics.push(conic);
        Ok(())
    }
}

/// Adds the conics fitting `beziers`, a chain of segments, within
/// `tolerance`, in runs: from each segment on, one conic for the longest
/// run [`run`] fits, grown until the next segment doesn't fit; or, if not
/// even the segment alone fits, that segment by [`fit`]. Each start tries
/// at most as many runs as there are segments left, each sampled at
/// [`SAMPLES`] points a segment, so a piece of `n` segments costs at most
/// about `16·n²` evaluations.
fn runs(beziers: &[Bezier], tolerance: f64, out: &mut Chain) -> Result<(), ProfileError> {
    let mut i = 0;
    while let Some(bezier) = beziers.get(i) {
        let mut longest = None;
        for j in i..beziers.len() {
            match run(&beziers[i..=j], tolerance) {
                Some(conic) => longest = Some((j, conic)),
                None => break,
            }
        }
        match longest {
            Some((j, conic)) => {
                out.push(conic)?;
                i = j + 1;
            }
            None => {
                fit(bezier, tolerance, 0, out)?;
                i += 1;
            }
        }
    }
    Ok(())
}

/// The one conic fitting the run of segments `run` within `tolerance`,
/// if there is one (see the module docs).
fn run(run: &[Bezier], tolerance: f64) -> Option<Conic2> {
    let (first, last) = (run.first()?, run.last()?);
    let (p0, p1) = (first[0], last[3]);
    if p0 == p1 {
        return None;
    }
    if run
        .iter()
        .flatten()
        .all(|&p| off_chord(p0, p1, p) <= tolerance / 4.0)
    {
        return Conic2::line(p0, p1).ok();
    }
    let c = control(first, last)?;
    let samples: Vec<DVec2> = run
        .iter()
        .flat_map(|bezier| samples(bezier).chain([bezier[3]]))
        .collect();
    // A conic is furthest from its chord at its shoulder.
    let chord = p1 - p0;
    let height = chord.perp_dot(c - p0);
    let k = samples
        .iter()
        .map(|&q| chord.perp_dot(q - p0) / height)
        .fold(f64::NEG_INFINITY, f64::max);
    weighted(p0, c, p1, k, &samples, tolerance / 2.0)
}

/// Adds the conics fitting `bezier` within `tolerance`, halving it as
/// needed, now halved `depth` times: a line if it's straight within a
/// quarter of the tolerance, else [`conic`]'s.
fn fit(bezier: &Bezier, tolerance: f64, depth: u32, out: &mut Chain) -> Result<(), ProfileError> {
    if bezier[0] != bezier[3] && flatness(bezier) <= tolerance / 4.0 {
        return out.push(Conic2::line(bezier[0], bezier[3])?);
    }
    if let Some(conic) = conic(bezier, tolerance / 2.0) {
        return out.push(conic);
    }
    if depth >= MAX_DEPTH {
        return Err(ProfileError::Fit);
    }
    let (left, right) = halves(bezier);
    fit(&left, tolerance, depth + 1, out)?;
    fit(&right, tolerance, depth + 1, out)
}

/// The conic from `bezier`'s start to its end along its end tangents and
/// through its shoulder, if it's within `tolerance` of the cubic.
fn conic(bezier: &Bezier, tolerance: f64) -> Option<Conic2> {
    let [p0, .., p1] = *bezier;
    let c = control(bezier, bezier)?;
    let middle = p0.midpoint(p1);
    let towards = c - middle;
    let k = (point(bezier, 0.5) - middle).dot(towards) / towards.length_squared();
    let samples: Vec<DVec2> = samples(bezier).collect();
    weighted(p0, c, p1, k, &samples, tolerance)
}

/// The [`SAMPLES`] points measured along `bezier`, its ends left out.
fn samples(bezier: &Bezier) -> impl Iterator<Item = DVec2> + '_ {
    (1..=SAMPLES).map(|i| point(bezier, i as f64 / (SAMPLES + 1) as f64))
}

/// Where the tangent leaving the start of `first` and the one arriving
/// at the end of `last` (the same segment, or a run's ends) meet, if
/// ahead of both ends, turning by less than 90°. A tangent is along the
/// first control point apart from its end.
fn control(first: &Bezier, last: &Bezier) -> Option<DVec2> {
    let (p0, p1) = (first[0], last[3]);
    let t0 = first[1..].iter().find_map(|&p| (p - p0).try_normalize())?;
    let t1 = last[..3]
        .iter()
        .rev()
        .find_map(|&p| (p1 - p).try_normalize())?;
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
    (s > 0.0 && u > 0.0).then(|| p0 + t0 * s)
}

/// The conic `p0, c, p1` through the point `k` of the way from the
/// chord's middle to `c` (weight `k/(1−k)`), or else the parabola: the
/// first of weight within [`W_FIT`] within `tolerance` of every one of
/// `samples`.
fn weighted(
    p0: DVec2,
    c: DVec2,
    p1: DVec2,
    k: f64,
    samples: &[DVec2],
    tolerance: f64,
) -> Option<Conic2> {
    if !(k > 0.0 && k < 1.0) {
        return None;
    }
    let shoulder = k / (1.0 - k);
    [shoulder, 1.0]
        .into_iter()
        .filter(|w| (W_FIT.0..=W_FIT.1).contains(w))
        .filter_map(|w| Conic2::new(p0, c, w, p1).ok())
        .find(|conic| {
            samples
                .iter()
                .all(|&q| distance(conic, q).is_some_and(|d| d <= tolerance))
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
    off_chord(b[0], b[3], b[1]).max(off_chord(b[0], b[3], b[2]))
}

/// How far `p` is from the chord from `p0` to `p1` (apart), or infinite
/// if it lies past an end.
fn off_chord(p0: DVec2, p1: DVec2, p: DVec2) -> f64 {
    let chord = p1 - p0;
    // Unscaled, so `p1` itself is between the ends to the bit.
    if (0.0..=chord.length_squared()).contains(&chord.dot(p - p0)) {
        chord.perp_dot(p - p0).abs() / chord.length()
    } else {
        f64::INFINITY
    }
}

#[cfg(test)]
mod tests;
