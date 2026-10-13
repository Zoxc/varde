//! Converting a spline between through fit points and by control points,
//! keeping its shape, and fitting the handles' tips that keep it.

use glam::DVec2;

use super::{BSpline, Interpolation, Spline, SplineKind, chord_params, handle_tip_for};
use crate::{EditError, Id, Sketch};

impl Sketch {
    /// Converts the spline `curve` to `to`, keeping its shape: through
    /// fit points to by control points exactly (the interpolation's
    /// control points and knots), the other way through the places its
    /// knots are at, which it then passes through but between them
    /// strays a hair, with handles at its ends or at every one, whichever
    /// keeps nearer its shape (`through_tips`). Knots a spline with a
    /// handle at every fit point would have go back to those fit points
    /// (`handled_params`), not one at each knot, so converting round
    /// and back doesn't grow it. An open spline keeps its first and last points,
    /// the rest are new, and those only it used go with what's on them,
    /// as do its handles. Nothing changes if it's `to` already.
    /// `Target` if `curve` is no spline or has no shape.
    pub fn convert_spline(&mut self, curve: Id, to: SplineKind) -> Result<(), EditError> {
        let target = EditError::Target(curve);
        let spline = self.spline(curve).ok_or(target)?;
        if spline.kind == to {
            return Ok(());
        }
        let spline = spline.clone();
        let shape = self.spline_shape(&spline).ok_or(target)?;
        let mut breaks = Vec::new();
        let (places, knots) = match to {
            SplineKind::Control => (shape.control().to_vec(), shape.knots().to_vec()),
            SplineKind::Through => {
                // A closed spline's knots are where its pieces meet,
                // round from the first; 0 needn't be one.
                breaks = if let Some(params) = handled_params(&shape) {
                    params
                } else if spline.closed {
                    shape.knots().to_vec()
                } else {
                    shape.breaks()
                };
                let places: Vec<DVec2> = breaks.iter().map(|&t| shape.point(t)).collect();
                (places, Vec::new())
            }
        };
        if !to.admits(places.len(), spline.closed) {
            return Err(target);
        }
        // Through fit points, the handles, each its fit point's index and
        // its tip.
        let tips = match to {
            SplineKind::Control => Vec::new(),
            SplineKind::Through => through_tips(&shape, &breaks, &places).ok_or(target)?,
        };
        let last = places.len() - 1;
        let mut points = Vec::with_capacity(places.len());
        for (i, &place) in places.iter().enumerate() {
            let kept = match spline.ends() {
                Some([first, _]) if i == 0 => Some(first),
                Some([_, end]) if i == last => Some(end),
                _ => None,
            };
            points.push(match kept {
                Some(id) => id,
                None => self.add_point(place)?,
            });
        }
        let mut handles = Vec::new();
        for (i, tip) in tips {
            handles.push(self.new_handle(points[i], places[i], tip)?);
        }
        self.replace_spline(
            curve,
            Spline {
                kind: to,
                points,
                closed: spline.closed,
                handles,
                knots,
            },
        )
    }
}

/// How many places in each piece of a spline [`through_tips`] weighs a
/// spline through its breaks against it at.
const FIT_PER_PIECE: usize = 8;

/// How many times [`through_tips`] fits the handles' tips, each to
/// where the spline the last made came nearest the targets.
const FIT_ROUNDS: usize = 8;

/// The parameter near `u` where `spline` comes nearest `want`, by a few
/// steps of Newton's method from `u`; `u` if they go astray.
fn nearest_by_newton(spline: &BSpline, mut u: f64, want: DVec2) -> f64 {
    let start = u;
    for _ in 0..4 {
        let [p, d, dd] = spline.eval(u);
        let slope = d.length_squared() + (p - want).dot(dd);
        if slope.is_nan() || slope <= 0.0 {
            break;
        }
        u -= (p - want).dot(d) / slope;
    }
    if u.is_finite()
        && spline.point(u).distance_squared(want) <= spline.point(start).distance_squared(want)
    {
        u
    } else {
        start
    }
}

/// The handles, each its fit point's index and tip, for the spline
/// through `places`, which `shape` is at its parameters `along` (where
/// its pieces meet: an open one's breaks, a closed one's knots), to keep
/// to `shape` best: of two, the one nearer [`FIT_PER_PIECE`] places of
/// `shape` in each piece, each matched first by the parameter, piece for
/// piece, then where the spline comes nearest it. One has handles at an open
/// spline's ends only ([`end_tips`]), which keeps the shape exactly when
/// the breaks are at the chord-length parameters of `places`, as they are
/// for a spline through fit points without handles converted to control
/// points. The other has a handle at every fit point, as the Spline tool
/// draws them, their tips by least squares
/// ([`Interpolation::nearest_tips`]): the knots a handle brings can't be
/// kept to otherwise, so a spline drawn with handles converted to control
/// points and back strays visibly without them. `None` if neither makes
/// a spline.
fn through_tips(shape: &BSpline, along: &[f64], places: &[DVec2]) -> Option<Vec<(usize, DVec2)>> {
    let closed = shape.closed();
    let n = places.len();
    let params = chord_params(places, closed);
    let pieces = if closed { n } else { n - 1 };
    let mut targets = Vec::with_capacity(pieces * FIT_PER_PIECE);
    for i in 0..pieces {
        let (u0, s0) = (params[i], along[i]);
        let (u1, s1) = match (i + 1 < n, closed) {
            (true, _) => (params[i + 1], along[i + 1]),
            // A closed spline's last piece, round to its first point.
            _ => (1.0, along[0] + 1.0),
        };
        for k in 0..FIT_PER_PIECE {
            let f = (k as f64 + 0.5) / FIT_PER_PIECE as f64;
            let u = u0 + (u1 - u0) * f;
            let [at, d, _] = shape.eval(s0 + (s1 - s0) * f);
            targets.push((u, at, d.normalize_or_zero()));
        }
    }
    // How far the spline through `places` with `tips` at `handles` is
    // from the targets, each its distance from the nearest place near
    // where it's matched, and the targets matched there instead.
    let off = |handles: &[usize], tips: &[DVec2], targets: &[(f64, DVec2, DVec2)]| {
        let spline = Interpolation::at_chords(places, closed, handles)?.spline(places, tips)?;
        let mut off = 0.0;
        let mut nearer = Vec::with_capacity(targets.len());
        for &(u, want, along) in targets {
            let u = nearest_by_newton(&spline, u, want);
            off += spline.point(u).distance_squared(want);
            nearer.push((u, want, along));
        }
        off.is_finite().then_some((off, nearer))
    };
    let mut ends: Vec<(usize, DVec2)> = Vec::new();
    if !closed {
        for (i, tip) in [0, n - 1].into_iter().zip(end_tips(shape, along, places)) {
            if let Some(tip) = tip {
                ends.push((i, tip));
            }
        }
    }
    let (indices, tips): (Vec<usize>, Vec<DVec2>) = ends.iter().copied().unzip();
    let ends_off = off(&indices, &tips, &targets).map(|(off, _)| off);
    // A handle at every fit point, the tips fitted, then fitted again to
    // where the spline they make comes nearest the targets, keeping the
    // best.
    let every: Vec<usize> = (0..n).collect();
    let all = Interpolation::at_chords(places, closed, &every).and_then(|interpolation| {
        let mut targets = targets.clone();
        let mut best: Option<(f64, Vec<DVec2>)> = None;
        for _ in 0..FIT_ROUNDS {
            let Some(tips) = interpolation.nearest_tips(places, &targets) else {
                break;
            };
            let Some((off, nearer)) = off(&every, &tips, &targets) else {
                break;
            };
            if best.as_ref().is_none_or(|(least, _)| off < *least) {
                best = Some((off, tips));
            }
            targets = nearer;
        }
        best
    });
    match (ends_off, all) {
        (Some(ends_off), Some((all_off, tips))) if all_off < ends_off => {
            Some(every.into_iter().zip(tips).collect())
        }
        (None, Some((_, tips))) => Some(every.into_iter().zip(tips).collect()),
        (Some(_), _) => Some(ends),
        (None, None) => None,
    }
}

/// The parameters of the fit points of a spline with a handle at every
/// one whose knots are `shape`'s, if they're such a spline's: each such
/// fit point has two knots, a third of the way to its neighbours
/// ([`Interpolation`]), which a fit point at each would double. Read back
/// pair by pair, then checked by making the knots again from them.
fn handled_params(shape: &BSpline) -> Option<Vec<f64>> {
    const NEAR: f64 = 1e-9;
    let closed = shape.closed();
    let knots: Vec<f64> = if closed {
        shape.knots().to_vec()
    } else {
        let inner = shape.knots().iter().copied();
        inner.filter(|&k| k > 0.0 && k < 1.0).collect()
    };
    let m = knots.len();
    if m == 0 || !m.is_multiple_of(2) {
        return None;
    }
    let matches = |params: &[f64]| {
        let every: Vec<usize> = (0..params.len()).collect();
        let Some(interpolation) = Interpolation::new(params, closed, &every) else {
            return false;
        };
        let mut again: Vec<f64> = interpolation.knots().to_vec();
        if !closed {
            again.retain(|&k| k > 0.0 && k < 1.0);
        }
        again.len() == m && again.iter().zip(&knots).all(|(a, b)| (a - b).abs() < NEAR)
    };
    if closed {
        // The pairs start at the first knot or the second.
        (0..2).find_map(|offset| {
            // Knot `j` on from `offset`, counting round.
            let at = |j: usize| {
                let j = offset + j;
                knots[j % m] + (j / m) as f64
            };
            // Fit point `i` at `a`, its first knot, and its neighbour
            // before's second, `b`: `3a = 2t + t'`, `3b = 2t' + t`.
            let mut params: Vec<f64> = (0..m / 2)
                .map(|i| (2.0 * at(2 * i + m) - at(2 * i + m - 1)).rem_euclid(1.0))
                .collect();
            params.sort_by(f64::total_cmp);
            matches(&params).then_some(params)
        })
    } else {
        let mut params = vec![0.0];
        for pair in knots.chunks(2) {
            let before = *params.last()?;
            params.push((3.0 * pair[0] - before) / 2.0);
        }
        params.push(1.0);
        matches(&params).then_some(params)
    }
}

/// The tips of handles at the first and last of `places`, which `shape`
/// is at its parameters `along`, that give the open spline through
/// `places` the tangents `shape` has there: its derivative, by its own
/// parameter, scaled to the chord-length parameters of `places` by the
/// spans beside each end, as [`Interpolation`] reads a handle. Without
/// them, the spline's ends would have no second derivative, which
/// `shape`'s needn't, and it would stray further. `None` for a tip that
/// isn't finite.
pub(super) fn end_tips(shape: &BSpline, along: &[f64], places: &[DVec2]) -> [Option<DVec2>; 2] {
    let params = chord_params(places, false);
    let last = places.len() - 1;
    [(0, 1), (last, last - 1)].map(|(i, next)| {
        let ratio = (along[next] - along[i]) / (params[next] - params[i]);
        let derivative = shape.eval(along[i])[1];
        handle_tip_for(&params, false, i, places[i], derivative * ratio)
    })
}
