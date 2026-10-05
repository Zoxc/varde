//! Telling what places along a curve make: a point, a line, a circle or
//! an arc, or else a spline through fit points within a tolerance
//! ([`LinkShape::fit`]), for links whose geometry comes from places
//! worked out along it (a model's edge, where a plane cuts a face, a
//! circle projected onto a tilted plane).

use std::fmt;

use glam::DVec2;

use super::LinkShape;
use crate::{BSpline, Curve, MAX_SPLINE_POINTS, Spline};

/// Places along one curve, in order, exact: the curve passes through
/// each. A closed one runs from its last back round to its first, which
/// isn't repeated.
#[derive(Debug, Clone, PartialEq)]
pub struct SampledChain {
    pub places: Vec<DVec2>,
    pub closed: bool,
}

/// Why places give no shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitError {
    /// No spline of [`MAX_SPLINE_POINTS`] fit points or fewer passes
    /// within the tolerance of them.
    TooComplex,
}

impl fmt::Display for FitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FitError::TooComplex => {
                f.write_str("its shape can't be held by a spline within the design's tolerance")
            }
        }
    }
}

impl LinkShape {
    /// The shape of `chains`, each in turn: a point where its places are
    /// all within `exact` of one another; a line where they're all within
    /// `exact` of the line through the two farthest apart (from one to
    /// the other, the one nearer the first place first, so a curve seen
    /// edge on, folding back, is the line it covers); a circle (closed, or an open chain whose ends meet) or an
    /// arc (counter-clockwise, its ends the chain's, at its radius) where
    /// they're all within `exact` of the circle fitted to them; else a
    /// spline through fit points among them, as few as keep it within
    /// `fit` of every place, at most [`MAX_SPLINE_POINTS`]
    /// ([`FitError::TooComplex`] past that).
    pub fn fit(chains: &[SampledChain], exact: f64, fit: f64) -> Result<LinkShape, FitError> {
        let mut shape = LinkShape::default();
        for chain in chains {
            shape.fit_chain(chain, exact, fit)?;
        }
        Ok(shape)
    }

    fn fit_chain(&mut self, chain: &SampledChain, exact: f64, fit: f64) -> Result<(), FitError> {
        let places = &chain.places;
        let Some(&first) = places.first() else {
            return Ok(());
        };
        let far = |from: DVec2| {
            (places.iter().copied())
                .max_by(|a, b| {
                    a.distance_squared(from)
                        .total_cmp(&b.distance_squared(from))
                })
                .unwrap_or(from)
        };
        // The two farthest apart, the one nearer the first place first,
        // so a line runs the way its places do.
        let (a, b) = {
            let a = far(first);
            let b = far(a);
            if a.distance(first) <= b.distance(first) {
                (a, b)
            } else {
                (b, a)
            }
        };
        if a.distance(b) <= exact {
            self.point((a + b) * 0.5);
            return Ok(());
        }
        let along = (b - a) / a.distance(b);
        if places.iter().all(|&p| along.perp_dot(p - a).abs() <= exact) {
            let start = self.point(a);
            let end = self.point(b);
            self.curve(Curve::Line { start, end });
            return Ok(());
        }
        if let Some((center, radius)) = circle_through(places)
            && places
                .iter()
                .all(|&p| (p.distance(center) - radius).abs() <= exact)
        {
            let last = places[places.len() - 1];
            let on = |p: DVec2| center + (p - center).normalize_or_zero() * radius;
            if chain.closed || first.distance(last) <= exact {
                let center = self.point(center);
                self.curve(Curve::Circle { center, radius });
            } else {
                // The way it turns about the centre.
                let turning: f64 = (places.windows(2))
                    .map(|pair| (pair[0] - center).perp_dot(pair[1] - center))
                    .sum();
                let (from, to) = if turning >= 0.0 {
                    (first, last)
                } else {
                    (last, first)
                };
                let center = self.point(center);
                let start = self.point(on(from));
                let end = self.point(on(to));
                self.curve(Curve::Arc { center, start, end });
            }
            return Ok(());
        }
        let fitted = spline_through(places, chain.closed, fit).ok_or(FitError::TooComplex)?;
        let points = fitted.iter().map(|&p| self.point(p)).collect();
        self.curve(Curve::Spline(Spline::through(points, chain.closed)));
        Ok(())
    }
}

/// The circle nearest `places` by least squares of their squared
/// distances (Kåsa's fit, about their mean so it's well conditioned):
/// its centre and radius. `None` for fewer than three places, or places
/// on a line.
fn circle_through(places: &[DVec2]) -> Option<(DVec2, f64)> {
    if places.len() < 3 {
        return None;
    }
    let n = places.len() as f64;
    let mean = places.iter().copied().sum::<DVec2>() / n;
    let (mut suu, mut suv, mut svv, mut suuu, mut svvv, mut suvv, mut svuu) =
        (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for &p in places {
        let d = p - mean;
        let (u, v) = (d.x, d.y);
        suu += u * u;
        suv += u * v;
        svv += v * v;
        suuu += u * u * u;
        svvv += v * v * v;
        suvv += u * v * v;
        svuu += v * u * u;
    }
    let det = suu * svv - suv * suv;
    let scale = (suu + svv) * (suu + svv);
    if det.is_nan() || det.abs() <= 1e-12 * scale {
        return None;
    }
    let bu = 0.5 * (suuu + suvv);
    let bv = 0.5 * (svvv + svuu);
    let uc = (bu * svv - bv * suv) / det;
    let vc = (bv * suu - bu * suv) / det;
    let center = mean + DVec2::new(uc, vc);
    let radius = places.iter().map(|p| p.distance(center)).sum::<f64>() / n;
    (center.is_finite() && radius.is_finite() && radius > 0.0).then_some((center, radius))
}

/// Fit points among `places` for a spline through them (open or
/// `closed`) passing within `fit` of every place: evenly along the
/// places by their chord lengths, doubling in number until it does, at
/// most [`MAX_SPLINE_POINTS`]. An open one keeps its first and last.
fn spline_through(places: &[DVec2], closed: bool, fit: f64) -> Option<Vec<DVec2>> {
    let n = places.len();
    let least = if closed { 3 } else { 2 };
    if n < least {
        return None;
    }
    // How far along the places each is.
    let mut along = Vec::with_capacity(n);
    let mut total = 0.0;
    along.push(0.0);
    for pair in places.windows(2) {
        total += pair[0].distance(pair[1]);
        along.push(total);
    }
    if closed {
        total += places[n - 1].distance(places[0]);
    }
    if total.is_nan() || total <= 0.0 {
        return None;
    }
    let mut count = 4.max(least);
    loop {
        let count_now = count.min(n).min(MAX_SPLINE_POINTS);
        let picked = pick(&along, total, count_now, closed);
        let fitted: Vec<DVec2> = picked.iter().map(|&i| places[i]).collect();
        if fitted.len() >= least && within(places, &fitted, closed, fit) {
            return Some(fitted);
        }
        if count_now >= n.min(MAX_SPLINE_POINTS) {
            return None;
        }
        count = count.saturating_mul(2);
    }
}

/// `count` indices into places at `along` (their distances along, of
/// `total`), evenly by distance, increasing and none repeated; an open
/// chain's first and last among them.
fn pick(along: &[f64], total: f64, count: usize, closed: bool) -> Vec<usize> {
    let n = along.len();
    let steps = if closed { count } else { count - 1 };
    let mut picked: Vec<usize> = Vec::with_capacity(count);
    let mut at = 0;
    for k in 0..count {
        let target = total * k as f64 / steps.max(1) as f64;
        while at + 1 < n && along[at + 1] <= target {
            at += 1;
        }
        let nearest = if at + 1 < n && along[at + 1] - target < target - along[at] {
            at + 1
        } else {
            at
        };
        if picked.last().is_none_or(|&last| nearest > last) {
            picked.push(nearest);
        }
    }
    if !closed && picked.last() != Some(&(n - 1)) {
        if picked.len() == count {
            picked.pop();
        }
        if picked.last().is_none_or(|&last| last < n - 1) {
            picked.push(n - 1);
        }
    }
    picked
}

/// Whether the spline through `fitted` passes within `fit` of every
/// place: each measured from the place on it nearest.
fn within(places: &[DVec2], fitted: &[DVec2], closed: bool, fit: f64) -> bool {
    let Some(spline) = BSpline::through(fitted, closed) else {
        return false;
    };
    let path = spline.path();
    // Not a number isn't within.
    (places.iter()).all(|&p| path.at(path.closest(p)).distance(p) <= fit)
}
