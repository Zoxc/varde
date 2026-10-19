//! Root isolation of a polynomial in Bernstein form on `[0, 1]`, at one
//! precision level: certified isolating intervals for its simple roots,
//! and clusters for what that level can't separate, which the caller
//! escalates (in precision) or decides by its sign change.
//!
//! **Subdivision.** Dyadic intervals `[j/2ᵏ, (j + 1)/2ᵏ]`, each split at
//! ½ by [`Poly::split_half`], depth first from the left, so the work and
//! the result follow from the coefficients alone and a reversed
//! polynomial gives the mirrored result (the split's same bits under
//! reversal). On each interval a coefficient counts with a sign only when
//! that sign is certified ([`Scalar::sign`]: `|c| > err`); one that isn't
//! may be of either sign or zero. The count of sign variations taken at
//! its largest over those choices bounds the roots on the closed interval,
//! with multiplicity (Descartes' rule on the Bernstein basis, an end root
//! of multiplicity `r` needing `r` zero end coefficients). An interval is
//! settled, and not split again, when it is
//!
//! - *empty*: every coefficient certified, of one sign (or monotone, below,
//!   away from zero);
//! - *simple*: at most one root on the closed interval, and that one simple,
//!   with the sign of the derivative there known: the variation count is at
//!   most 1 with an end certified, or the derivative's coefficients are all
//!   certified of one sign (the polynomial is monotone);
//! - *open*: every coefficient uncertain (splitting no longer helps at this
//!   precision), or at the depth limit.
//!
//! **Ends and split points, each once.** The domain is the closed `[0, 1]`.
//! A split point is shared by the two intervals beside it, which see the
//! same bits there (the split keeps the middle value to the bit on both
//! sides). Where that value's sign is certified, or follows from a
//! monotone neighbour, no root is there and the intervals are apart. Where
//! it isn't, the intervals on either side are joined: a maximal run of
//! settled intervals linked by such points is one result, a root or a
//! cluster, so a root on or beside a split point is reported once, by the
//! run that straddles it, and never by one side alone. Ends of the results
//! inside `(0, 1)` are thus always certified non-zero; only `t = 0` and
//! `t = 1` can be uncertain, in a cluster reaching them.
//!
//! **Results.** A run whose ends have opposite certified signs and whose
//! intervals are all simple with the same derivative sign, that of the
//! end, holds exactly one root, simple (two simple roots in a row cross
//! zero in opposite directions): a [`Root`], on the open interval between
//! the run's ends. A single interval's root is narrowed by Krawczyk's
//! operator, `K(X) = y − Y·p(y) + (1 − Y·p'(X))·(X − y)` with `Y` the
//! inverse of the middle of `p'(X)`, which holds every root of `X` and
//! contracts when `p'` is certified of one sign and its range narrow, and
//! by halves at the certified sign of `p(y)` meanwhile; always where the
//! root crosses upwards, on the mirrored interval for one crossing
//! downwards, so the reversed polynomial takes the same steps. Anything
//! else is a
//! [`Cluster`] (a tangential root, roots closer than the depth limit, an
//! end root, a polynomial within its error of zero): its closed interval,
//! a bound on its roots counted with multiplicity, and its end signs, from
//! which the caller reads the sign change across it.
//!
//! **For callers counting roots along a chain.** Within one call each root
//! is in exactly one result: results meet only at points certified
//! non-zero, so a root never sits on a shared end, and counting results
//! (a [`Root`] once, a [`Cluster`] by its end signs) counts each root once.
//! The results are closed, not half-open; only `t = 0` and `t = 1` can hold
//! a root on a result's end, and there it is always a cluster with that
//! end's sign `None`. Pieces isolated by separate calls (the spans of a
//! curve, or sub-intervals a caller cut itself) share their ends, so a
//! root at a shared end shows up twice: as a cluster reaching `t = 1` of
//! one piece and one reaching `t = 0` of the next. The caller settles each
//! shared end once, by its own rule: it reads the sign there (or that it
//! is zero) from one record of that point, which both pieces take for
//! their `None` end, and a root exactly there is counted by that record,
//! by a half-open convention (the point belongs to one side only), never
//! by both clusters. A caller cutting a piece itself does so with
//! [`Poly::split_half`] or [`Poly::split_at`], which give both sides the
//! same bits at the cut, so the two calls certify the same sign there.
//!
//! **Work.** Every split and test is counted ([`Work`]); this layer has no
//! budget, so the caller charges it, and gives a cap on splits past which
//! the isolation stops ([`OutOfWork`]).

use std::cmp::Ordering;

use malachite::Rational;
use smallvec::SmallVec;

use super::poly::Poly;
use super::scalar::Scalar;
use crate::num::bounded::down;

/// The deepest subdivision [`isolate`] takes, 63: the intervals' ends are
/// `j/2ᵏ` with `j ≤ 2ᵏ` in a `u64`.
pub const MAX_ROOT_DEPTH: u32 = 63;

/// The most Krawczyk steps narrowing one root.
const KRAWCZYK_STEPS: u32 = 8;

/// The most halvings one [`narrow`] takes: a representation limit (an
/// `f64` interval inside `[0, 1]` halves at most about this often before
/// its middle is one of its ends).
pub const ROOT_HALVINGS: u32 = 64;

/// A dyadic point `num/2^exp` of `[0, 1]`, in lowest terms (`num` odd,
/// or `0/2⁰`), so equal points are equal values; ordered by value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Dyadic {
    num: u64,
    exp: u32,
}

impl Dyadic {
    /// `t = 0`.
    pub const ZERO: Dyadic = Dyadic { num: 0, exp: 0 };

    /// `t = 1`.
    pub const ONE: Dyadic = Dyadic { num: 1, exp: 0 };

    /// `num/2^exp` in lowest terms; `None` unless `exp ≤`
    /// [`MAX_ROOT_DEPTH`] and `num ≤ 2^exp`.
    pub fn new(num: u64, exp: u32) -> Option<Dyadic> {
        if exp > MAX_ROOT_DEPTH || num > 1u64 << exp {
            return None;
        }
        if num == 0 {
            return Some(Dyadic::ZERO);
        }
        let z = num.trailing_zeros().min(exp);
        Some(Dyadic {
            num: num >> z,
            exp: exp - z,
        })
    }

    /// The numerator, odd unless the point is 0.
    #[inline]
    pub fn num(self) -> u64 {
        self.num
    }

    /// The exponent of the denominator `2^exp`.
    #[inline]
    pub fn exp(self) -> u32 {
        self.exp
    }

    /// The point as an exact rational, `num/2^exp`.
    pub fn rational(self) -> Rational {
        Rational::from(self.num) >> u64::from(self.exp)
    }

    /// `[lo, hi]`, `f64`s with the point between them: both the point
    /// itself when it is a float (`num < 2⁵³`).
    pub fn bounds(self) -> [f64; 2] {
        let s = pow2_neg(self.exp);
        let [lo, hi] = int_bounds(self.num);
        [lo * s, hi * s]
    }

    /// The point as an `f64`, if it is one exactly (`num < 2⁵³`).
    pub fn to_f64(self) -> Option<f64> {
        (self.num < 1 << 53).then(|| self.bounds()[0])
    }
}

impl Ord for Dyadic {
    fn cmp(&self, o: &Dyadic) -> Ordering {
        // `num < 2⁶⁴` shifted by at most 63: below `2¹²⁷`.
        (u128::from(self.num) << o.exp).cmp(&(u128::from(o.num) << self.exp))
    }
}

impl PartialOrd for Dyadic {
    fn partial_cmp(&self, o: &Dyadic) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// A certified simple root: exactly one root of the polynomial on the open
/// interval `(lo, hi)`, of multiplicity 1; the polynomial is certified
/// non-zero at `lo` and `hi`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Root {
    pub lo: Dyadic,
    pub hi: Dyadic,
    /// The sign of the derivative at the root: `Greater` where the
    /// polynomial crosses from negative to positive.
    pub dir: Ordering,
    /// `[a, b]`, `f64`s with the root between them, within the floats
    /// around the ends, `lo.bounds()[0] ≤ a` and `b ≤ hi.bounds()[1]`
    /// (inside `[lo, hi]` when the ends are floats; an end whose `num`
    /// passes `2⁵³` isn't, and the root may lie between it and the
    /// nearest float): narrowed by Krawczyk's operator and halving for a
    /// root found on one interval, else those bounds of `lo` and `hi`.
    pub near: [f64; 2],
}

/// What one precision level can't separate: the roots on the closed
/// interval `[lo, hi]`, none certified, their number bounded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cluster {
    pub lo: Dyadic,
    pub hi: Dyadic,
    /// At least the number of roots on `[lo, hi]`, counted with
    /// multiplicity, unless the polynomial is identically zero; at most
    /// its degree.
    pub mult: u32,
    /// The certified sign of the polynomial at `lo`: `None` only at
    /// `t = 0`, where a root may be.
    pub start: Option<Ordering>,
    /// The certified sign at `hi`: `None` only at `t = 1`.
    pub end: Option<Ordering>,
    /// Whether part of it reached the depth limit; if not, it is as far as
    /// subdivision gets at this precision.
    pub at_depth: bool,
}

/// One result of [`isolate`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Found {
    Root(Root),
    Cluster(Cluster),
}

impl Found {
    /// The interval's ends, `[lo, hi]`.
    pub fn span(&self) -> [Dyadic; 2] {
        match self {
            Found::Root(r) => [r.lo, r.hi],
            Found::Cluster(c) => [c.lo, c.hi],
        }
    }
}

/// What an isolation ran, for the caller to charge to its budget.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    /// Splits at ½.
    pub splits: u32,
    /// Intervals tested (coefficient signs, maybe the derivative's).
    pub tests: u32,
    /// Krawczyk steps (an evaluation and two splits of the derivative
    /// each).
    pub steps: u32,
}

impl Work {
    /// This work and `o`'s together.
    pub fn and(self, o: Work) -> Work {
        Work {
            splits: self.splits.saturating_add(o.splits),
            tests: self.tests.saturating_add(o.tests),
            steps: self.steps.saturating_add(o.steps),
        }
    }
}

/// The limits of one isolation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The deepest interval, `2^-max_depth` wide (at most
    /// [`MAX_ROOT_DEPTH`]); one not settled there is part of a cluster.
    pub max_depth: u32,
    /// The most splits; one more stops the isolation with
    /// [`OutOfWork`].
    pub max_splits: u32,
}

/// The isolation stopped at [`Limits::max_splits`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutOfWork {
    /// The work to charge: the cap's splits and the most tests that many
    /// splits take, `2·max_splits + 1`, an upper bound of the tests run.
    /// The tests run before the cap depend on the side the splits are on,
    /// depth first from the left; this doesn't, so a reversed polynomial
    /// stops with the same work.
    pub work: Work,
}

/// The roots of a polynomial on `[0, 1]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Roots {
    /// In increasing `t`, the closed intervals disjoint but for shared
    /// ends, where the polynomial is certified non-zero: every root is in
    /// exactly one result.
    pub found: Vec<Found>,
    pub work: Work,
}

/// An interval's state once tested.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// No root on the closed interval.
    Empty,
    /// At most one root on the closed interval, simple, the derivative of
    /// this sign there.
    Simple(Ordering),
    /// At most `bound` roots, with multiplicity, unless identically zero.
    Open { bound: u32, at_depth: bool },
}

/// A settled interval `[j/2ᵏ, (j + 1)/2ᵏ]`; the polynomial on it kept
/// for a simple one (to narrow its root).
struct Leaf<S> {
    k: u32,
    j: u64,
    kind: Kind,
    start: Option<Ordering>,
    end: Option<Ordering>,
    q: Option<Poly<S>>,
}

impl<S> Leaf<S> {
    fn lo(&self) -> Dyadic {
        Dyadic::new(self.j, self.k).expect("a leaf's start is in [0, 1]")
    }

    fn hi(&self) -> Dyadic {
        Dyadic::new(self.j + 1, self.k).expect("a leaf's end is in [0, 1]")
    }
}

/// The roots of `p` on `[0, 1]` (module docs), or [`OutOfWork`] past
/// `limits.max_splits` splits.
pub fn isolate<S: Scalar>(p: &Poly<S>, limits: Limits) -> Result<Roots, OutOfWork> {
    let mut work = Work::default();
    let mut leaves: Vec<Leaf<S>> = subdivide(p, limits, &mut work, test)?
        .into_iter()
        .map(|(k, j, (kind, start, end), q)| Leaf {
            k,
            j,
            kind,
            start,
            end,
            q: matches!(kind, Kind::Simple(_)).then_some(q),
        })
        .collect();
    let degree = u32::try_from(p.degree()).expect("degree at most MAX_DEGREE");
    let found = runs(&mut leaves, degree, &mut work);
    Ok(Roots { found, work })
}

/// A settled interval of [`subdivide`]: its depth `k`, index `j`
/// (`[j/2ᵏ, (j + 1)/2ᵏ]`), verdict and polynomial there.
type Settled<S, K> = (u32, u64, K, Poly<S>);

/// The settled intervals of `p` by `test` (its verdict, or `None` to
/// split; told whether a split is still allowed), depth first from the
/// left, so in increasing `t`: each interval's depth `k`, index `j`,
/// verdict and polynomial. Splits and tests counted in `work`; past
/// `limits.max_splits` splits [`OutOfWork`]. The one subdivision both
/// [`isolate`] and [`isolate_exact`] run.
fn subdivide<S: Scalar, K>(
    p: &Poly<S>,
    limits: Limits,
    work: &mut Work,
    test: impl Fn(&Poly<S>, bool) -> Option<K>,
) -> Result<Vec<Settled<S, K>>, OutOfWork> {
    let max_depth = limits.max_depth.min(MAX_ROOT_DEPTH);
    let mut leaves = Vec::new();
    // Depth first, left before right: leaves in increasing `t`.
    let mut stack = vec![(0u32, 0u64, p.clone())];
    while let Some((k, j, q)) = stack.pop() {
        work.tests = work.tests.saturating_add(1);
        match test(&q, k < max_depth) {
            Some(kind) => leaves.push((k, j, kind, q)),
            None => {
                if work.splits >= limits.max_splits {
                    let tests = limits.max_splits.saturating_mul(2).saturating_add(1);
                    return Err(OutOfWork {
                        work: Work {
                            splits: limits.max_splits,
                            tests,
                            steps: 0,
                        },
                    });
                }
                work.splits += 1;
                let [l, r] = q.split_half();
                stack.push((k + 1, 2 * j + 1, r));
                stack.push((k + 1, 2 * j, l));
            }
        }
    }
    Ok(leaves)
}

/// One result of [`isolate_exact`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exact {
    /// A root exactly at this point.
    Point(Dyadic),
    /// Exactly one root strictly inside `(lo, hi)`, simple, the
    /// polynomial non-zero at both ends and crossing zero towards `dir`
    /// (`Greater` from negative to positive).
    Root {
        lo: Dyadic,
        hi: Dyadic,
        dir: Ordering,
    },
    /// At most `mult` roots strictly inside `(lo, hi)`, counted with
    /// multiplicity, not separated at the depth limit; the polynomial
    /// non-zero at both ends, or a [`Exact::Point`] there.
    Cluster { lo: Dyadic, hi: Dyadic, mult: u32 },
}

/// The roots of an exact polynomial on `[0, 1]`.
#[derive(Clone, Debug, PartialEq)]
pub struct ExactRoots {
    /// In increasing `t`: every root is a [`Exact::Point`] or inside
    /// exactly one interval.
    pub found: Vec<Exact>,
    pub work: Work,
}

/// An exact interval's verdict: no root inside, exactly one (simple,
/// crossing towards the sign given), or at most this many at the depth
/// limit.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ExactKind {
    Empty,
    Simple(Ordering),
    Open(u32),
}

/// The roots of `p`, not identically zero, on `[0, 1]` with exact
/// coefficients: the subdivision of [`isolate`] with the **exact-zero
/// rule**. A rational coefficient's sign is known even when it is zero,
/// so a zero is not a sign either way (as an uncertain coefficient of a
/// level is): Descartes' rule counts the variations of the non-zero
/// coefficients, which bound the roots strictly inside the interval, and
/// one variation with both ends non-zero is exactly one root there,
/// simple. The value at each end and split point is exact (the end
/// coefficients, the same at a shared split point on both sides): zero
/// there is a root exactly there ([`Exact::Point`]), reported once, and
/// the intervals beside it stay apart. So every root is either a point
/// or inside one interval, never on a result's end, and results never
/// join; only roots closer than `2^-max_depth` stay together, a
/// [`Exact::Cluster`] the caller may isolate again on its own interval.
/// [`OutOfWork`] past `limits.max_splits` splits.
pub fn isolate_exact(p: &Poly<Rational>, limits: Limits) -> Result<ExactRoots, OutOfWork> {
    let mut work = Work::default();
    let leaves = subdivide(p, limits, &mut work, test_exact)?;
    let dy = |j: u64, k: u32| Dyadic::new(j, k).expect("a leaf's end is in [0, 1]");
    let mut found = Vec::new();
    for (i, (k, j, kind, q)) in leaves.iter().enumerate() {
        if i == 0 && q.coefs()[0] == 0u32 {
            found.push(Exact::Point(Dyadic::ZERO));
        }
        let (lo, hi) = (dy(*j, *k), dy(j + 1, *k));
        match kind {
            ExactKind::Empty => {}
            ExactKind::Simple(dir) => found.push(Exact::Root { lo, hi, dir: *dir }),
            ExactKind::Open(mult) => found.push(Exact::Cluster {
                lo,
                hi,
                mult: *mult,
            }),
        }
        if q.coefs()[q.degree()] == 0u32 {
            found.push(Exact::Point(hi));
        }
    }
    Ok(ExactRoots { found, work })
}

/// An exact interval's verdict ([`isolate_exact`]), or `None` to split.
fn test_exact(q: &Poly<Rational>, can_split: bool) -> Option<ExactKind> {
    let c = q.coefs();
    let mut last = Ordering::Equal;
    let mut v = 0u32;
    for x in c {
        let s = x.partial_cmp(&0u32).expect("rationals are ordered");
        if s != Ordering::Equal {
            if last != Ordering::Equal && s != last {
                v += 1;
            }
            last = s;
        }
    }
    if last == Ordering::Equal {
        // Identically zero: every point a root; the caller excludes it.
        return Some(ExactKind::Open(
            u32::try_from(q.degree()).unwrap_or(u32::MAX),
        ));
    }
    if v == 0 {
        return Some(ExactKind::Empty);
    }
    let ends = c[0] != 0u32 && c[c.len() - 1] != 0u32;
    if v == 1 && ends {
        return Some(ExactKind::Simple(last));
    }
    (!can_split).then_some(ExactKind::Open(v))
}

/// An interval's kind and its end signs (certified, or implied by
/// monotonicity), or `None` to split it.
fn test<S: Scalar>(
    q: &Poly<S>,
    can_split: bool,
) -> Option<(Kind, Option<Ordering>, Option<Ordering>)> {
    let c = q.coefs();
    let signs: SmallVec<[Option<Ordering>; 16]> = c.iter().map(Scalar::sign).collect();
    let (start, end) = (signs[0], signs[signs.len() - 1]);
    if start.is_some() && signs.iter().all(|s| *s == start) {
        return Some((Kind::Empty, start, end));
    }
    let v = max_variations(&signs);
    if signs.iter().all(Option::is_none) {
        return Some((
            Kind::Open {
                bound: v,
                at_depth: false,
            },
            None,
            None,
        ));
    }
    if v <= 1 {
        // At most one root on the open interval, and with a zero end
        // coefficient none there: one root at most, simple.
        // (A certified coefficient between two uncertain ends makes at
        // least two variations, so an end is certified.)
        let kind = match (start, end) {
            (Some(s), Some(e)) if s == e => Some(Kind::Empty),
            (_, Some(e)) => Some(Kind::Simple(e)),
            (Some(s), None) => Some(Kind::Simple(s.reverse())),
            (None, None) => None,
        };
        if let Some(kind) = kind {
            return Some((kind, start, end));
        }
    }
    if let Some(d) = one_sign(&q.derivative()) {
        // Monotone: one root at most, simple; an end moving away from
        // zero gives the other end its sign.
        return Some(match (start, end) {
            (Some(s), Some(e)) if s == e => (Kind::Empty, start, end),
            (Some(_), Some(e)) => (Kind::Simple(e), start, end),
            (Some(s), None) if s == d => (Kind::Empty, start, start),
            (None, Some(e)) if e != d => (Kind::Empty, end, end),
            _ => (Kind::Simple(d), start, end),
        });
    }
    (!can_split).then_some((
        Kind::Open {
            bound: v,
            at_depth: true,
        },
        start,
        end,
    ))
}

/// The certified sign every coefficient shares, if any.
fn one_sign<S: Scalar>(p: &Poly<S>) -> Option<Ordering> {
    let s = p.coefs()[0].sign()?;
    p.coefs()[1..]
        .iter()
        .all(|c| c.sign() == Some(s))
        .then_some(s)
}

/// The most sign variations of the sequence over every choice of sign
/// for its uncertain entries (`None`): an upper bound of the variations
/// of the true values, zeros dropped, since giving a dropped zero a sign
/// adds variations or none.
fn max_variations(signs: &[Option<Ordering>]) -> u32 {
    // The most variations of the prefix ending negative, positive; `None`
    // where it can't.
    let mut best: [Option<u32>; 2] = [None, None];
    for (i, s) in signs.iter().enumerate() {
        let allowed = |x: usize| match s {
            None => true,
            Some(Ordering::Less) => x == 0,
            Some(_) => x == 1,
        };
        let mut next = [None, None];
        for (x, slot) in next.iter_mut().enumerate() {
            if allowed(x) {
                *slot = if i == 0 {
                    Some(0)
                } else {
                    let stay = best[x];
                    let flip = best[1 - x].map(|v| v + 1);
                    stay.max(flip)
                };
            }
        }
        best = next;
    }
    best[0].max(best[1]).unwrap_or(0)
}

/// The runs of leaves linked by uncertain split points, each a root or a
/// cluster (module docs).
fn runs<S: Scalar>(leaves: &mut [Leaf<S>], degree: u32, work: &mut Work) -> Vec<Found> {
    let m = leaves.len();
    // The sign at each leaf boundary: certified (the same bits on both
    // sides) or implied by either side.
    let mut at: Vec<Option<Ordering>> = Vec::with_capacity(m + 1);
    at.push(leaves[0].start);
    for i in 1..m {
        at.push(leaves[i - 1].end.or(leaves[i].start));
    }
    at.push(leaves[m - 1].end);
    for (i, l) in leaves.iter_mut().enumerate() {
        (l.start, l.end) = (at[i], at[i + 1]);
        if let (Kind::Simple(_), Some(s), Some(e)) = (l.kind, l.start, l.end)
            && s == e
        {
            l.kind = Kind::Empty;
        }
    }
    let mut found = Vec::new();
    let mut i = 0;
    while i < m {
        if leaves[i].kind == Kind::Empty {
            i += 1;
            continue;
        }
        let mut last = i;
        while last + 1 < m && at[last + 1].is_none() {
            last += 1;
        }
        let run = &leaves[i..=last];
        let (start, end) = (at[i], at[last + 1]);
        let (lo, hi) = (run[0].lo(), run[run.len() - 1].hi());
        let crossing = match (start, end) {
            (Some(s), Some(e)) if s != e => Some(e),
            _ => None,
        };
        let one_root = crossing.filter(|&e| run.iter().all(|l| l.kind == Kind::Simple(e)));
        found.push(match one_root {
            Some(dir) => {
                let near = match run {
                    [l] => {
                        let q = l.q.as_ref().expect("a simple leaf keeps its polynomial");
                        let x = if dir == Ordering::Greater {
                            krawczyk(q, work)
                        } else {
                            mirror(krawczyk(&q.reversed(), work))
                        };
                        to_t(l.k, l.j, x)
                    }
                    _ => [lo.bounds()[0], hi.bounds()[1]],
                };
                Found::Root(Root { lo, hi, dir, near })
            }
            None => {
                let mut mult = 0u32;
                let mut at_depth = false;
                for l in run {
                    let b = match l.kind {
                        Kind::Empty => 0,
                        Kind::Simple(_) => 1,
                        Kind::Open { bound, at_depth: d } => {
                            at_depth |= d;
                            bound
                        }
                    };
                    mult = mult.saturating_add(b);
                }
                Found::Cluster(Cluster {
                    lo,
                    hi,
                    mult: mult.min(degree),
                    start,
                    end,
                    at_depth,
                })
            }
        });
        i = last + 1;
    }
    found
}

/// A root's interval after [`narrow`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Narrowed {
    /// `[a, b]`, `f64`s holding the root.
    pub span: [f64; 2],
    /// The halvings taken, each an evaluation of the polynomial, for the
    /// caller to charge.
    pub halvings: u32,
    /// Whether the caller's test settled: `false` where the halving
    /// stopped first (the polynomial's sign at a middle not certified,
    /// the middle not strictly inside, or [`ROOT_HALVINGS`] taken).
    pub settled: bool,
}

/// Narrows the interval `span` holding exactly one root of `p`, simple,
/// `p` crossing zero in the direction `dir` there (a [`Root`]'s `near`
/// and `dir`), to the width its caller needs ("narrow to width"):
/// certified halving, each step keeping the half the certified sign of
/// `p` at the middle puts the root in, while `wide` says the interval is
/// still too wide for the caller (its own test: a width, or a position
/// not yet certified on one side of something). At most
/// [`ROOT_HALVINGS`] halvings; an error of `wide` stops it and is
/// returned.
pub fn narrow<S: Scalar, E>(
    p: &Poly<S>,
    span: [f64; 2],
    dir: Ordering,
    mut wide: impl FnMut([f64; 2]) -> Result<bool, E>,
) -> Result<Narrowed, E> {
    let like = &p.coefs()[0];
    let mut out = Narrowed {
        span,
        halvings: 0,
        settled: false,
    };
    while wide(out.span)? {
        let [a, b] = out.span;
        let mid = 0.5 * a + 0.5 * b;
        if out.halvings >= ROOT_HALVINGS || !(mid > a && mid < b) {
            return Ok(out);
        }
        out.halvings += 1;
        // Past the root `p` has the sign `dir`.
        let past = match p.eval(&S::from_f64(mid, like)).sign() {
            Some(Ordering::Equal) | None => return Ok(out),
            Some(x) => x == dir,
        };
        out.span = if past { [a, mid] } else { [mid, b] };
    }
    out.settled = true;
    Ok(out)
}

/// The interval `X ⊆ [0, 1]` holding `q`'s one root, simple and crossing
/// zero upwards, narrowed while steps shrink it: each step
/// takes `X ∩ K(X)` by Krawczyk's operator (every root of `X` lies in
/// `K(X)` for any `y` in `X` and real `Y`, given an enclosure of `q'` on
/// `X`, so the intersection still holds it), and the half of `X` beside
/// `y` that the sign of `q(y)`, where certified, puts the root in (`q` is
/// positive past the root and negative before), so it narrows by halves
/// where the derivative's range is too wide for the operator.
fn krawczyk<S: Scalar>(q: &Poly<S>, work: &mut Work) -> [f64; 2] {
    let dq = q.derivative();
    let like = &q.coefs()[0];
    let mut x = [0.0, 1.0];
    for _ in 0..KRAWCZYK_STEPS {
        work.steps = work.steps.saturating_add(1);
        // In `X`, as the operator needs (halving a subnormal end may
        // round it out).
        let m = 0.5 * x[0] + 0.5 * x[1];
        let y = if m < x[0] {
            x[0]
        } else if m > x[1] {
            x[1]
        } else {
            m
        };
        let qy = q.eval(&S::from_f64(y, like));
        let mut nx = match qy.sign() {
            Some(Ordering::Greater) => [x[0], y],
            Some(_) => [y, x[1]],
            None => x,
        };
        let d = derivative_range(&dq, x, like);
        let inv = 1.0 / (0.5 * d[0] + 0.5 * d[1]);
        if (d[0] > 0.0 || d[1] < 0.0) && inv.is_finite() {
            let k = add(
                sub([y, y], mul([inv, inv], qy.enclose())),
                mul(sub([1.0, 1.0], mul([inv, inv], d)), sub(x, [y, y])),
            );
            if k[0] > nx[0] {
                nx[0] = k[0];
            }
            if k[1] < nx[1] {
                nx[1] = k[1];
            }
        }
        if nx[0] > nx[1] || nx[1] - nx[0] >= x[1] - x[0] {
            break;
        }
        x = nx;
    }
    x
}

/// `[1 − b, 1 − a]`, each end rounded outwards where the difference
/// isn't a float, for `0 ≤ a ≤ b ≤ 1`.
fn mirror([a, b]: [f64; 2]) -> [f64; 2] {
    // `r = fl(1 − x)` is exact iff `1 − r` gives `x` back: for `x ≥ ½`
    // it is (Sterbenz), and below ½ `r ≥ ½`, so `1 − r` is exact itself.
    let one_minus = |x: f64, round: fn(f64) -> f64| {
        let r = 1.0 - x;
        if 1.0 - r == x { r } else { round(r) }
    };
    [one_minus(b, down), one_minus(a, up_signed)]
}

/// An enclosure of `dq` on `[a, b] ⊆ [0, 1]`: the hull of its piece on
/// `[s·b, b] ⊇ [a, b]`, `s ≤ a/b` (split at exact parameters).
fn derivative_range<S: Scalar>(dq: &Poly<S>, [a, b]: [f64; 2], like: &S) -> [f64; 2] {
    let mut p = dq.clone();
    if b < 1.0 {
        p = p.split_at(&S::from_f64(b, like))[0].clone();
    }
    if a > 0.0 {
        let s = down(a / b);
        if s > 0.0 {
            p = p.split_at(&S::from_f64(s, like))[1].clone();
        }
    }
    let h = p.hull();
    [h.lo, h.hi]
}

/// A local `[a, b]` of the leaf `[j/2ᵏ, (j + 1)/2ᵏ]` as bounds in `t`,
/// kept inside the leaf's own bounds.
fn to_t(k: u32, j: u64, [a, b]: [f64; 2]) -> [f64; 2] {
    let s = pow2_neg(k);
    let [jl, ju] = int_bounds(j);
    let [_, jh] = int_bounds(j + 1);
    let lo = down(jl + a);
    let hi = up_signed(ju + b);
    // Scaling by `2⁻ᵏ` is exact unless the product is subnormal (only on
    // the first leaf, `j = 0`), where it is rounded outwards.
    let (lo, hi) = (scaled(lo, s, down), scaled(hi, s, up_signed));
    let (jl, jh) = (jl * s, jh * s);
    [if lo > jl { lo } else { jl }, if hi < jh { hi } else { jh }]
}

/// `x·s`, `s` a power of two, through `round` when the product may have
/// rounded (below the normal range).
fn scaled(x: f64, s: f64, round: fn(f64) -> f64) -> f64 {
    let p = x * s;
    if p.abs() < f64::MIN_POSITIVE {
        round(p)
    } else {
        p
    }
}

/// `[lo, hi]`, `f64`s with the integer `n` between them, both `n` for
/// `n < 2⁵³`.
fn int_bounds(n: u64) -> [f64; 2] {
    // Rounds to nearest above `2⁵³`.
    let x = n as f64;
    if n < 1 << 53 {
        [x, x]
    } else {
        [down(x), up_signed(x)]
    }
}

/// `2⁻ᵏ` exactly, `k ≤ 1022`.
fn pow2_neg(k: u32) -> f64 {
    f64::from_bits((1023 - u64::from(k)) << 52)
}

/// An upper bound of the exact value `t` was rounded from (any sign).
fn up_signed(t: f64) -> f64 {
    -down(-t)
}

/// Interval sum, bounded outwards (a NaN end becomes infinite).
fn add(x: [f64; 2], y: [f64; 2]) -> [f64; 2] {
    [down(x[0] + y[0]), up_signed(x[1] + y[1])]
}

/// Interval difference, bounded outwards.
fn sub(x: [f64; 2], y: [f64; 2]) -> [f64; 2] {
    [down(x[0] - y[1]), up_signed(x[1] - y[0])]
}

/// Interval product, bounded outwards.
fn mul(x: [f64; 2], y: [f64; 2]) -> [f64; 2] {
    let p = [x[0] * y[0], x[0] * y[1], x[1] * y[0], x[1] * y[1]];
    let mut r = [f64::INFINITY, f64::NEG_INFINITY];
    for v in p {
        let (lo, hi) = (down(v), up_signed(v));
        if lo < r[0] {
            r[0] = lo;
        }
        if hi > r[1] {
            r[1] = hi;
        }
    }
    r
}

#[cfg(test)]
mod tests;
