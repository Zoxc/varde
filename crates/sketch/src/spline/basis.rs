//! Cubic B-splines by their knots and control points: chord-length
//! parameters, evaluating, interpolating through fit points (with
//! tangents) as a linear map, and cutting into Bézier segments.
//!
//! Every spline's parameter runs from 0 to 1. An open one is clamped: it
//! starts at its first control point and ends at its last, its knots
//! repeated four times at 0 and at 1 around the interior ones kept. A
//! closed one is periodic, of period 1: its knots are one per control
//! point, within `[0, 1)`, and the control points wrap round.

use glam::DVec2;

use super::bezier::Path;

/// The least span between chord-length parameters, as a share of the
/// whole: points at one place still get distinct knots.
pub const MIN_SPAN: f64 = 1e-6;

/// The least gap between knots kept with a spline by its control points:
/// below what [`chord_params`], [`control_knots`] and [`Interpolation`]
/// give, with room for rounding.
pub const MIN_KNOT_GAP: f64 = 1e-7;

/// A matrix's pivot at most this share of its largest entry is taken as
/// zero: the system has no single solution.
const SINGULAR: f64 = 1e-12;

/// Chord-length parameters of `points`: 0 at the first, each one on by
/// the distance from the one before, scaled so the last is 1 for an open
/// spline, and the way back round from the last to the first ends at 1
/// for a closed one (whose parameters are then all below 1). Each span is
/// at least [`MIN_SPAN`] of the whole, and all even if the points are at
/// one place or aren't finite. As many as the points.
pub fn chord_params(points: &[DVec2], closed: bool) -> Vec<f64> {
    let n = points.len();
    if n == 0 {
        return Vec::new();
    }
    let chords = if closed { n } else { n - 1 };
    let mut spans: Vec<f64> = (0..chords)
        .map(|i| points[i].distance(points[(i + 1) % n]))
        .collect();
    let total: f64 = spans.iter().sum();
    if total > 0.0 && total.is_finite() {
        for span in &mut spans {
            *span = span.max(MIN_SPAN * total);
        }
    } else {
        spans.fill(1.0);
    }
    let total: f64 = spans.iter().sum();
    let mut params = Vec::with_capacity(n);
    let mut at = 0.0;
    params.push(at);
    for &span in &spans[..n - 1] {
        at += span / total;
        params.push(at);
    }
    if !closed && n > 1 {
        params[n - 1] = 1.0;
    }
    params
}

/// The knots a spline by its control points `points` is given when it's
/// made: chord-length parameters of its control polygon
/// ([`chord_params`]), averaged three at a time for an open one (its
/// `points.len() - 4` interior knots), as they are for a closed one. What
/// [`BSpline::clamped`] and [`BSpline::periodic`] take.
pub fn control_knots(points: &[DVec2], closed: bool) -> Vec<f64> {
    let params = chord_params(points, closed);
    if closed {
        return params;
    }
    let interior = points.len().saturating_sub(4);
    (1..=interior)
        .map(|j| (params[j] + params[j + 1] + params[j + 2]) / 3.0)
        .collect()
}

/// The knots of a cubic B-spline, with what evaluating it needs.
#[derive(Debug, Clone, PartialEq)]
struct Knots {
    /// Every knot, `count + 4` for an open spline, `count + 7` for a
    /// closed one (its knots on round either side), non-decreasing.
    all: Vec<f64>,
    /// The knots as kept: an open spline's interior ones, a closed one's
    /// one per control point.
    kept: Vec<f64>,
    /// How many control points: an open spline's are `all.len() - 4`, a
    /// closed one's wrap round, the first three again after the last.
    count: usize,
    closed: bool,
}

impl Knots {
    /// A closed spline's ([`Knots::periodic`]) or an open one's
    /// ([`Knots::clamped`]).
    fn new(knots: &[f64], closed: bool) -> Option<Knots> {
        if closed {
            Knots::periodic(knots)
        } else {
            Knots::clamped(knots)
        }
    }

    /// An open spline's, from its interior knots: strictly increasing
    /// within `(0, 1)`. `None` otherwise.
    fn clamped(interior: &[f64]) -> Option<Knots> {
        if !increasing(interior, 0.0, 1.0) {
            return None;
        }
        let mut all = vec![0.0; 4];
        all.extend_from_slice(interior);
        all.extend([1.0; 4]);
        Some(Knots {
            all,
            kept: interior.to_vec(),
            count: interior.len() + 4,
            closed: false,
        })
    }

    /// A closed spline's, from one knot per control point, at least three,
    /// strictly increasing within `[0, 1)`. `None` otherwise.
    fn periodic(knots: &[f64]) -> Option<Knots> {
        let m = knots.len();
        let (&first, &last) = (knots.first()?, knots.last()?);
        // Round from the last to the first a period on, too.
        let within = |k: &f64| (0.0..1.0).contains(k);
        if m < 3
            || !within(&first)
            || !within(&last)
            || !increasing(&knots[1..], first, first + 1.0)
        {
            return None;
        }
        let all = (0..m + 7).map(|i| wrapped(knots, i as isize - 3)).collect();
        Some(Knots {
            all,
            kept: knots.to_vec(),
            count: m,
            closed: true,
        })
    }

    /// How many control points the knots take, counting a closed spline's
    /// wrapped round again: one past the last span.
    fn spans_end(&self) -> usize {
        self.all.len() - 4
    }

    /// `t` within the knots' domain: a closed spline's from its first
    /// knot a period on, wrapped round; an open one's clamped to `[0, 1]`.
    fn domain(&self, t: f64) -> f64 {
        if self.closed {
            let first = self.all[3];
            let t = t.rem_euclid(1.0);
            if t < first { t + 1.0 } else { t }
        } else {
            t.clamp(0.0, 1.0)
        }
    }

    /// The span `t` (within [`Knots::domain`]) is in: the last knot at or
    /// before it, within the spans there are.
    fn span(&self, t: f64) -> usize {
        let after = self.all.partition_point(|&k| k <= t);
        after.saturating_sub(1).clamp(3, self.spans_end() - 1)
    }

    /// The four cubic basis functions not zero in the span `span` at `t`,
    /// with their first and second derivatives: `[order][j]` is that of
    /// the one of control point `span - 3 + j`.
    fn basis(&self, span: usize, t: f64) -> [[f64; 4]; 3] {
        let u = &self.all;
        let ratio = |a: f64, b: f64| if b > 0.0 { a / b } else { 0.0 };
        // table[d][j]: the basis function of degree d of index
        // span - d + j.
        let mut table = [[0.0; 4]; 4];
        table[0][0] = 1.0;
        for d in 1..=3 {
            for j in 0..=d {
                let i = span - d + j;
                let lower = if j >= 1 { table[d - 1][j - 1] } else { 0.0 };
                let upper = if j < d { table[d - 1][j] } else { 0.0 };
                table[d][j] = ratio(t - u[i], u[i + d] - u[i]) * lower
                    + ratio(u[i + d + 1] - t, u[i + d + 1] - u[i + 1]) * upper;
            }
        }
        // The basis function of degree d and index i, zero where it's not
        // among those in the span.
        let n = |d: usize, i: usize| {
            if i + d >= span && i <= span {
                table[d][i + d - span]
            } else {
                0.0
            }
        };
        let mut ders = [[0.0; 4]; 3];
        for j in 0..4 {
            let i = span - 3 + j;
            ders[0][j] = table[3][j];
            ders[1][j] =
                3.0 * (ratio(n(2, i), u[i + 3] - u[i]) - ratio(n(2, i + 1), u[i + 4] - u[i + 1]));
            let left = ratio(n(1, i), u[i + 2] - u[i]) - ratio(n(1, i + 1), u[i + 3] - u[i + 1]);
            let right =
                ratio(n(1, i + 1), u[i + 3] - u[i + 1]) - ratio(n(1, i + 2), u[i + 4] - u[i + 2]);
            ders[2][j] = 6.0 * (ratio(left, u[i + 3] - u[i]) - ratio(right, u[i + 4] - u[i + 1]));
        }
        ders
    }

    /// The control point, of `count`, that the one at `index` of the
    /// wrapped-round list is.
    fn control(&self, index: usize) -> usize {
        index % self.count
    }

    /// Where the spline's polynomial pieces meet within `[0, 1]`, with 0
    /// and 1: its Bézier segments' ends.
    fn breaks(&self) -> Vec<f64> {
        let mut breaks = vec![0.0];
        breaks.extend(self.kept.iter().copied().filter(|&k| k > 0.0 && k < 1.0));
        breaks.push(1.0);
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        breaks
    }
}

/// Knot `j` of a closed spline whose knots within its first period are
/// `kept`, counting on (or back) round its periods.
fn wrapped(kept: &[f64], j: isize) -> f64 {
    let m = kept.len() as isize;
    kept[j.rem_euclid(m) as usize] + j.div_euclid(m) as f64
}

/// The curvature of a curve with first and second derivatives `first`
/// and `second`: positive turning left going on, one over the radius.
/// Not a number where it doesn't move.
pub(crate) fn curvature(first: DVec2, second: DVec2) -> f64 {
    first.perp_dot(second) / first.length().powi(3)
}

/// Whether `knots` strictly increase from `from` to `to`, each at least
/// [`MIN_KNOT_GAP`] on from the one before, the first from `from` and
/// `to` from the last.
fn increasing(knots: &[f64], from: f64, to: f64) -> bool {
    let mut before = from;
    for &k in knots {
        if !(MIN_KNOT_GAP..=f64::INFINITY).contains(&(k - before)) {
            return false;
        }
        before = k;
    }
    to - before >= MIN_KNOT_GAP
}

/// How many periods a closed spline is laid out over either side of its
/// own for inserting knots ([`BSpline::unwrapped`]): what's inserted
/// near the far ends, where the periods before or after aren't there,
/// doesn't reach the middle.
const UNWRAPPED_PERIODS: usize = 2;

/// A cubic B-spline's knots and control points laid out plainly, as
/// inserting knots takes them: control point `i`'s basis function runs
/// over `knots[i..=i + 4]`, and the spline is defined from `knots[3]` to
/// `knots[knots.len() - 4]`.
#[derive(Debug, Clone, PartialEq)]
struct Unwrapped {
    knots: Vec<f64>,
    control: Vec<DVec2>,
}

impl Unwrapped {
    /// How many of its knots are `t`.
    fn multiplicity(&self, t: f64) -> usize {
        self.knots.iter().filter(|&&k| k == t).count()
    }

    /// Inserts a knot at `t`, and a control point, leaving the spline as
    /// it was (Boehm's algorithm). `false`, changing nothing, unless `t`
    /// is where it's defined, short of its last knot.
    fn insert(&mut self, t: f64) -> bool {
        let n = self.knots.len();
        if n < 8 || !(t >= self.knots[3] && t < self.knots[n - 4]) {
            return false;
        }
        // The last knot at or before `t`: within `3..n - 4`.
        let k = self.knots.partition_point(|&u| u <= t) - 1;
        let control: Vec<DVec2> = (0..=self.control.len())
            .map(|i| {
                if i + 3 <= k {
                    self.control[i]
                } else if i > k {
                    self.control[i - 1]
                } else {
                    // The knots either side of `t` differ.
                    let (from, to) = (self.knots[i], self.knots[i + 3]);
                    let along = (t - from) / (to - from);
                    self.control[i - 1] * (1.0 - along) + self.control[i] * along
                }
            })
            .collect();
        self.knots.insert(k + 1, t);
        self.control = control;
        true
    }
}

/// A cubic, non-rational B-spline: open (clamped) or closed (periodic),
/// its parameter running from 0 to 1.
#[derive(Debug, Clone, PartialEq)]
pub struct BSpline {
    knots: Knots,
    /// A closed spline's wrap round, the first three again after the last.
    control: Vec<DVec2>,
}

impl BSpline {
    /// The open spline of `control` points, at least four, with interior
    /// knots `interior`, one for each control point past four, strictly
    /// increasing within `(0, 1)`, at least [`MIN_KNOT_GAP`] apart and
    /// from 0 and 1. `None` otherwise.
    pub fn clamped(interior: &[f64], control: Vec<DVec2>) -> Option<BSpline> {
        let knots = Knots::clamped(interior)?;
        (control.len() == knots.count).then_some(BSpline { knots, control })
    }

    /// The closed spline of `control` points, at least three, with a
    /// knot for each, strictly increasing within `[0, 1)`, at least
    /// [`MIN_KNOT_GAP`] apart and round from the last to the first.
    /// `None` otherwise.
    pub fn periodic(knots: &[f64], mut control: Vec<DVec2>) -> Option<BSpline> {
        let knots = Knots::periodic(knots)?;
        if control.len() != knots.count {
            return None;
        }
        control.extend_from_within(..3);
        Some(BSpline { knots, control })
    }

    /// A closed spline ([`BSpline::periodic`]) or an open one
    /// ([`BSpline::clamped`]).
    pub fn new(knots: &[f64], control: Vec<DVec2>, closed: bool) -> Option<BSpline> {
        if closed {
            BSpline::periodic(knots, control)
        } else {
            BSpline::clamped(knots, control)
        }
    }

    /// The spline through `points`, without handles, at their
    /// chord-length parameters (see [`Interpolation`]).
    pub fn through(points: &[DVec2], closed: bool) -> Option<BSpline> {
        Interpolation::at_chords(points, closed, &[])?.spline(points, &[])
    }

    pub fn closed(&self) -> bool {
        self.knots.closed
    }

    /// The control points, a closed spline's each once.
    pub fn control(&self) -> &[DVec2] {
        &self.control[..self.knots.count]
    }

    /// The knots as [`BSpline::clamped`] or [`BSpline::periodic`] take
    /// them.
    pub fn knots(&self) -> &[f64] {
        &self.knots.kept
    }

    /// The place at `t`, and the first and second derivatives there by
    /// `t`. An open spline's `t` is clamped to `[0, 1]`, a closed one's
    /// taken round.
    pub fn eval(&self, t: f64) -> [DVec2; 3] {
        let t = self.knots.domain(t);
        self.eval_in(self.knots.span(t), t)
    }

    /// [`BSpline::eval`] by the polynomial of the span `span`, at `t` in
    /// the knots' domain (see [`Knots::domain`]).
    fn eval_in(&self, span: usize, t: f64) -> [DVec2; 3] {
        let basis = self.knots.basis(span, t);
        basis.map(|weights| {
            (0..4)
                .map(|j| self.control[span - 3 + j] * weights[j])
                .sum()
        })
    }

    pub fn point(&self, t: f64) -> DVec2 {
        self.eval(t)[0]
    }

    /// The curvature at `t`: positive turning left going on, one over the
    /// radius. Not a number where it doesn't move.
    pub fn curvature(&self, t: f64) -> f64 {
        let [_, first, second] = self.eval(t);
        curvature(first, second)
    }

    /// Where its polynomial pieces meet, from 0 to 1: each piece is a
    /// cubic Bézier segment, as drawing and intersecting take it.
    pub fn breaks(&self) -> Vec<f64> {
        self.knots.breaks()
    }

    /// Parameters along it: `per` in each piece between its
    /// [`breaks`](BSpline::breaks), evenly by the parameter from the
    /// piece's start, and an open one's end.
    pub fn samples(&self, per: usize) -> Vec<f64> {
        let breaks = self.breaks();
        let mut params: Vec<f64> = breaks
            .windows(2)
            .flat_map(|pair| {
                let (from, to) = (pair[0], pair[1]);
                (0..per).map(move |i| from + (to - from) * i as f64 / per as f64)
            })
            .collect();
        if !self.closed() {
            params.push(1.0);
        }
        params
    }

    /// Its knots and control points laid out plainly (see [`Unwrapped`]):
    /// an open one's as they are, a closed one's over five periods, its
    /// own the middle one, from `control[2 m]` (`m` control points).
    fn unwrapped(&self) -> Unwrapped {
        if !self.closed() {
            return Unwrapped {
                knots: self.knots.all.clone(),
                control: self.control.clone(),
            };
        }
        let m = self.knots.count;
        let kept = &self.knots.kept;
        let shift = UNWRAPPED_PERIODS * m;
        let count = (2 * UNWRAPPED_PERIODS + 1) * m + 3;
        let knots = (0..count + 4)
            .map(|i| wrapped(kept, i as isize - 3 - shift as isize))
            .collect();
        let control = (0..count).map(|i| self.control[i % m]).collect();
        Unwrapped { knots, control }
    }

    /// The open spline running as this one does from `a` to `b`, its
    /// parameter taken from 0 at `a` to 1 at `b`, exactly: its knots
    /// inserted at both until it passes through a control point there
    /// (Boehm's algorithm), and what's between kept. An open spline's `a`
    /// and `b` are within `[0, 1]`, a closed one's `b` at most a turn past
    /// `a`, round its start if need be. A cut within a hair of a knot
    /// (less than twice [`MIN_KNOT_GAP`] of the piece) is made at the
    /// knot, so the piece's knots stay apart. `None` for a piece of
    /// nothing, or parameters that aren't so.
    pub fn piece(&self, a: f64, b: f64) -> Option<BSpline> {
        let span = b - a;
        let open_within = self.closed() || (0.0 <= a && b <= 1.0);
        if !(span > 0.0 && span <= 1.0 && open_within) {
            return None;
        }
        let mut unwrapped = self.unwrapped();
        let a = if self.closed() {
            self.knots.domain(a)
        } else {
            a
        };
        let b = a + span;
        let hair = 2.0 * MIN_KNOT_GAP * span;
        let knots = &unwrapped.knots;
        let a = knots
            .iter()
            .copied()
            .find(|&k| k > a && k - a < hair)
            .unwrap_or(a);
        let b = knots
            .iter()
            .copied()
            .rev()
            .find(|&k| k < b && b - k < hair)
            .unwrap_or(b);
        if b <= a {
            return None;
        }
        for t in [a, b] {
            while unwrapped.multiplicity(t) < 3 {
                if !unwrapped.insert(t) {
                    return None;
                }
            }
        }
        let knots = &unwrapped.knots;
        let last_a = knots.iter().rposition(|&k| k == a)?;
        let first_b = knots.iter().position(|&k| k == b)?;
        let (from, to) = (last_a.checked_sub(3)?, first_b.checked_sub(1)?);
        let control = unwrapped.control.get(from..=to)?.to_vec();
        let interior: Vec<f64> = knots
            .get(last_a + 1..first_b)?
            .iter()
            .map(|&k| (k - a) / (b - a))
            .collect();
        BSpline::clamped(&interior, control)
    }

    /// The same spline with a knot more at `t`, and a control point more,
    /// exactly (Boehm's algorithm): an open one's `t` within `(0, 1)`, a
    /// closed one's taken round. `None` where that's within
    /// [`MIN_KNOT_GAP`] of a knot it has, or an open spline's end.
    pub fn with_knot(&self, t: f64) -> Option<BSpline> {
        if !t.is_finite() {
            return None;
        }
        let t = if self.closed() {
            self.knots.domain(t)
        } else {
            t
        };
        let near = |k: f64| (k - t).abs() < MIN_KNOT_GAP;
        if self
            .knots
            .all
            .iter()
            .any(|&k| near(k) || near(k + 1.0) || near(k - 1.0))
        {
            return None;
        }
        let mut unwrapped = self.unwrapped();
        if !self.closed() {
            if !unwrapped.insert(t) {
                return None;
            }
            let n = unwrapped.knots.len();
            let interior = unwrapped.knots.get(4..n - 4)?;
            return BSpline::clamped(interior, unwrapped.control);
        }
        // Once in every period, so the middle one is as it should be.
        let within = t - t.floor();
        let periods = UNWRAPPED_PERIODS as isize;
        for turns in -periods..=periods {
            // Those at the far ends are past what's laid out, and don't
            // reach the middle period.
            unwrapped.insert(within + turns as f64);
        }
        let mut kept = self.knots.kept.clone();
        let at = kept.partition_point(|&k| k < within);
        kept.insert(at, within);
        let first = unwrapped.knots.iter().position(|&k| k == kept[0])?;
        let control = unwrapped
            .control
            .get(first.checked_sub(3)?..first - 3 + kept.len())?
            .to_vec();
        BSpline::periodic(&kept, control)
    }

    /// The spline as cubic Bézier segments, one per piece between its
    /// [`breaks`](BSpline::breaks), exactly: each by its ends and the
    /// derivatives there.
    pub(crate) fn path(&self) -> Path {
        let breaks = self.breaks();
        let mut segments: Vec<[DVec2; 4]> = Vec::with_capacity(breaks.len() - 1);
        for pair in breaks.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let middle = self.knots.domain((a + b) / 2.0);
            // The period the piece is in, for a closed spline's.
            let shift = middle - (a + b) / 2.0;
            let span = self.knots.span(middle);
            let [p, d, _] = self.eval_in(span, a + shift);
            let [q, e, _] = self.eval_in(span, b + shift);
            let h = b - a;
            let start = segments.last().map_or(p, |before| before[3]);
            segments.push([start, p + d * h / 3.0, q - e * h / 3.0, q]);
        }
        if self.closed()
            && let Some(&[first, ..]) = segments.first()
            && let Some(last) = segments.last_mut()
        {
            last[3] = first;
        }
        Path::new(breaks, segments, self.closed())
    }
}

/// Interpolating fit points by a cubic B-spline, for parameters fixed
/// beforehand: the control points as a linear map of the fit points and
/// the handles' tips.
///
/// The spline passes through fit point `i` at its parameter `t_i`. A
/// handle at a fit point sets the derivative there: `3 / h (tip - point)`,
/// `h` the parameter span beside it (their mean between two), so a handle
/// a third of the chord long is the pull the curve would have anyway, and
/// longer pulls harder. An open spline's ends without handles have no
/// second derivative (natural ends). Knots are the fit points'
/// parameters, a point with a handle getting two instead, a third of the
/// way to the points either side, so the spline stays twice continuously
/// differentiable everywhere.
///
/// The solver keeps the parameters fixed through a solve, so the spline
/// is linear in its points while it solves.
#[derive(Debug, Clone, PartialEq)]
pub struct Interpolation {
    knots: Knots,
    /// How many fit points and tips, the map's inputs.
    inputs: usize,
    /// The map: a row per control point, a column per input, the fit
    /// points first, then the tips in order of their points.
    weights: Vec<f64>,
}

impl Interpolation {
    /// The interpolation through fit points at `params` (see
    /// [`chord_params`]: increasing, at least two for an open spline and
    /// three for a closed one, whose parameters are below 1), with
    /// handles at the fit points `handles` (increasing indices). `None`
    /// if the parameters or handles aren't so, or no single spline
    /// interpolates them.
    pub fn new(params: &[f64], closed: bool, handles: &[usize]) -> Option<Interpolation> {
        let n = params.len();
        let fits = handles.windows(2).all(|pair| pair[0] < pair[1])
            && handles.last().is_none_or(|&last| last < n);
        let least = if closed { 3 } else { 2 };
        if n < least || !fits {
            return None;
        }
        let has_handle = |i: usize| handles.binary_search(&i).is_ok();
        let spans = |i: usize| spans_beside(params, closed, i);
        let knots = if closed {
            let mut kept = Vec::with_capacity(n + handles.len());
            for (i, &t) in params.iter().enumerate() {
                if has_handle(i) {
                    let (before, after) = spans(i);
                    kept.push((t - before / 3.0).rem_euclid(1.0));
                    kept.push(t + after / 3.0);
                } else {
                    kept.push(t);
                }
            }
            kept.sort_by(f64::total_cmp);
            Knots::periodic(&kept)?
        } else {
            let mut interior = Vec::with_capacity(n + handles.len());
            for (i, &t) in params.iter().enumerate().take(n - 1).skip(1) {
                if has_handle(i) {
                    let (before, after) = spans(i);
                    interior.extend([t - before / 3.0, t + after / 3.0]);
                } else {
                    interior.push(t);
                }
            }
            Knots::clamped(&interior)?
        };
        let m = knots.count;
        let inputs = n + handles.len();
        // A row per condition: the derivative's order, where, and its
        // right-hand side over the inputs.
        let mut a = vec![0.0; m * m];
        let mut b = vec![0.0; m * inputs];
        let mut row = 0;
        let mut condition = |order: usize, t: f64, rhs: &[(usize, f64)]| -> Option<()> {
            if row >= m {
                return None;
            }
            let t = knots.domain(t);
            let span = knots.span(t);
            let basis = knots.basis(span, t);
            for j in 0..4 {
                a[row * m + knots.control(span - 3 + j)] += basis[order][j];
            }
            for &(column, value) in rhs {
                b[row * inputs + column] += value;
            }
            row += 1;
            Some(())
        };
        let mut tips = n..;
        for (i, &t) in params.iter().enumerate() {
            condition(0, t, &[(i, 1.0)])?;
            let end = !closed && (i == 0 || i == n - 1);
            if has_handle(i) {
                let tip = tips.next()?;
                let scale = handle_scale(params, closed, i);
                condition(1, t, &[(tip, scale), (i, -scale)])?;
            } else if end {
                condition(2, t, &[])?;
            }
        }
        if row != m {
            return None;
        }
        let weights = solve(a, b, m, inputs)?;
        Some(Interpolation {
            knots,
            inputs,
            weights,
        })
    }

    /// The interpolation through fit points at `places`, at their
    /// chord-length parameters ([`chord_params`]), see
    /// [`Interpolation::new`].
    pub fn at_chords(places: &[DVec2], closed: bool, handles: &[usize]) -> Option<Interpolation> {
        Interpolation::new(&chord_params(places, closed), closed, handles)
    }

    /// The knots of the spline it makes, as [`BSpline::clamped`] or
    /// [`BSpline::periodic`] take them.
    pub fn knots(&self) -> &[f64] {
        &self.knots.kept
    }

    /// How many control points the spline has.
    pub fn controls(&self) -> usize {
        self.knots.count
    }

    /// The weights of the fit points, then of the tips, in control point
    /// `control`.
    pub fn weights(&self, control: usize) -> &[f64] {
        &self.weights[control * self.inputs..(control + 1) * self.inputs]
    }

    /// The control points of the spline through `fit`, with `tips` the
    /// tips of its handles in order of their fit points. `None` unless
    /// there are as many as it was made for.
    pub fn control(&self, fit: &[DVec2], tips: &[DVec2]) -> Option<Vec<DVec2>> {
        if fit.len() + tips.len() != self.inputs {
            return None;
        }
        Some(
            (0..self.knots.count)
                .map(|c| {
                    let inputs = fit.iter().chain(tips);
                    inputs.zip(self.weights(c)).map(|(&p, &w)| p * w).sum()
                })
                .collect(),
        )
    }

    /// The spline through `fit` with handle tips `tips`, see
    /// [`Interpolation::control`].
    pub fn spline(&self, fit: &[DVec2], tips: &[DVec2]) -> Option<BSpline> {
        let control = self.control(fit, tips)?;
        BSpline::new(&self.knots.kept, control, self.knots.closed)
    }
}

/// A weight of a spline's point in a control point at most this is left
/// out of [`SplineMap`]: an interpolation's weights fall by about a
/// quarter a span, so each control point keeps the weights of some
/// twenty-five fit points either side, and what's left out moves the
/// curve by less than a ten-thousandth of what the solver holds it to.
const NEGLIGIBLE: f64 = 1e-15;

/// A [`SplineMap`]'s cubic basis functions not zero at a parameter, with
/// their first and second derivatives, and the span they're of.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Basis {
    span: usize,
    values: [[f64; 4]; 3],
}

/// A spline as a linear map of its points, for the solver, which holds
/// its knots (and, through fit points, its parameters) fixed through a
/// solve: each control point a weighted sum of its points, the fit
/// points then the handles' tips in order of their fit points
/// ([`Interpolation`]), or the control points themselves.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SplineMap {
    knots: Knots,
    /// Per control point, the weights of the points in it, by index.
    rows: Vec<Vec<(usize, f64)>>,
}

impl SplineMap {
    /// The map of a spline by `count` control points over `knots` (as
    /// [`BSpline::clamped`] or [`BSpline::periodic`] take them): each
    /// control point is its point. `None` if the knots aren't so.
    pub fn control(knots: &[f64], closed: bool, count: usize) -> Option<SplineMap> {
        let knots = Knots::new(knots, closed)?;
        (knots.count == count).then(|| SplineMap {
            rows: (0..count).map(|i| vec![(i, 1.0)]).collect(),
            knots,
        })
    }

    /// The map of the spline `interpolation` makes.
    pub fn through(interpolation: &Interpolation) -> SplineMap {
        let rows = (0..interpolation.controls())
            .map(|c| {
                let weights = interpolation.weights(c);
                let weights = weights.iter().copied().enumerate();
                weights.filter(|(_, w)| w.abs() > NEGLIGIBLE).collect()
            })
            .collect();
        SplineMap {
            knots: interpolation.knots.clone(),
            rows,
        }
    }

    /// Whether it's closed, its parameter taken round.
    pub fn closed(&self) -> bool {
        self.knots.closed
    }

    /// The spline it makes of its points at `points`. `None` unless
    /// they're as many as it maps.
    pub fn spline(&self, points: &[DVec2]) -> Option<BSpline> {
        let mut control = Vec::with_capacity(self.rows.len());
        for row in &self.rows {
            let mut sum = DVec2::ZERO;
            for &(point, weight) in row {
                sum += *points.get(point)? * weight;
            }
            control.push(sum);
        }
        BSpline::new(&self.knots.kept, control, self.knots.closed)
    }

    /// Its basis at `t`, clamped to `[0, 1]` or taken round as
    /// [`BSpline::eval`] takes it, for [`SplineMap::weights`].
    pub fn basis(&self, t: f64) -> Basis {
        let t = self.knots.domain(t);
        let span = self.knots.span(t);
        Basis {
            span,
            values: self.knots.basis(span, t),
        }
    }

    /// Calls `f` with each point's weight in the spline's derivative of
    /// `order` (0, the place, to 2) where `basis` is of, perhaps more than
    /// once for a point, the weights adding up: at most four control
    /// points' worth.
    pub fn weights(&self, basis: &Basis, order: usize, mut f: impl FnMut(usize, f64)) {
        for (j, &b) in basis.values[order].iter().enumerate() {
            if b != 0.0 {
                for &(point, w) in &self.rows[self.knots.control(basis.span - 3 + j)] {
                    f(point, b * w);
                }
            }
        }
    }
}

/// The parameter spans before and after fit point `i`, round from the
/// last to the first for a closed spline; an open spline's end has the one
/// beside it for both.
fn spans_beside(params: &[f64], closed: bool, i: usize) -> (f64, f64) {
    let n = params.len();
    let next = |i: usize| {
        if i + 1 < n {
            params[i + 1] - params[i]
        } else {
            1.0 - params[i] + params[0]
        }
    };
    match (closed, i) {
        (true, 0) => (next(n - 1), next(0)),
        (true, _) => (next(i - 1), next(i)),
        (false, 0) => (next(0), next(0)),
        (false, _) if i + 1 == n => (next(i - 1), next(i - 1)),
        (false, _) => (next(i - 1), next(i)),
    }
}

/// What a handle's tip less its fit point is multiplied by to give the
/// derivative there, see [`Interpolation`]: three over the mean of the
/// parameter spans beside fit point `i`.
pub fn handle_scale(params: &[f64], closed: bool, i: usize) -> f64 {
    let (before, after) = spans_beside(params, closed, i);
    6.0 / (before + after)
}

/// Solves `a x = b` for `x`, `a` an `m` by `m` matrix and `b` an `m` by
/// `columns` one, both by rows, by Gaussian elimination with partial
/// pivoting, skipping zeros: a spline's matrix is banded (but for a
/// closed one's corners), so it takes about `m²` steps and a few per
/// entry of `x`, not `m³`. `None` if `a` is singular, or so near it that
/// the solution means nothing.
fn solve(mut a: Vec<f64>, mut b: Vec<f64>, m: usize, columns: usize) -> Option<Vec<f64>> {
    let largest = a.iter().fold(0.0f64, |most, v| most.max(v.abs()));
    if !(largest > 0.0 && largest.is_finite()) {
        return None;
    }
    for k in 0..m {
        let pivot = (k..m).max_by(|&r, &s| a[r * m + k].abs().total_cmp(&a[s * m + k].abs()))?;
        let size = a[pivot * m + k].abs();
        if size.is_nan() || size <= SINGULAR * largest {
            return None;
        }
        if pivot != k {
            for c in 0..m {
                a.swap(k * m + c, pivot * m + c);
            }
            for c in 0..columns {
                b.swap(k * columns + c, pivot * columns + c);
            }
        }
        let diagonal = a[k * m + k];
        for r in k + 1..m {
            let factor = a[r * m + k] / diagonal;
            if factor == 0.0 {
                continue;
            }
            for c in k..m {
                a[r * m + c] -= factor * a[k * m + c];
            }
            for c in 0..columns {
                b[r * columns + c] -= factor * b[k * columns + c];
            }
        }
    }
    // A spline's rows have a few entries each, and keep few once
    // eliminated, so only those are gone through for each column.
    let mut entries = Vec::with_capacity(m);
    for k in (0..m).rev() {
        entries.clear();
        entries.extend((k + 1..m).filter(|&j| a[k * m + j] != 0.0));
        for c in 0..columns {
            let mut value = b[k * columns + c];
            for &j in &entries {
                value -= a[k * m + j] * b[j * columns + c];
            }
            b[k * columns + c] = value / a[k * m + k];
        }
    }
    b.iter().all(|v| v.is_finite()).then_some(b)
}
