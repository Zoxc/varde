//! The crossing record of two edges on one curve, and of two straight
//! edges whose shadows lie on one line: their crossings are the sign
//! changes of `h = ê·(T × Δ)` along the overlap ([`shared_clusters`]).
//!
//! **Where moved shared edges cross.** Over the edges' overlap their
//! shadows coincide, so no box of their square ever settles. With `e`
//! moved by `δ·Δ` (its rates) and `g` fixed, the moved shadows meet near
//! the overlap exactly where `h(s) = ê·(T(s) × Δ(s))` changes sign along
//! it (`T` the curve's tangent: the shadow tangent parallel to the shadow
//! of the motion; [`shared_crossing`](crate::perturb::shared_crossing),
//! its sign over a piece from [`shared_crossing_h1`]): where `h > 0` the
//! moved `e` lies
//! to the left of `g` in the shadow. So a change of sign is a crossing,
//! of degree `+1` where `h` goes from `−` to `+` along `e`'s stored
//! direction with `g` running the same way (`T_g × T_e > 0` there), and
//! the other way round negated: the four terms' orientation.
//!
//! **The overlap** is made of **runs**: maximal stretches of the curve
//! both edges cover, from `P` to `Q`. On one line it is the exact common
//! range of the two shadows on `e`'s parameter (rounded outwards to `f64`,
//! a split never past the exact ends). On one chain it is made of whole
//! spans: those of `e` that are spans of `g` bit for bit (either way
//! round), a run being consecutive ones. That holds where every end of
//! either edge is a knot or end of the other or off it (no feet on it);
//! an end lying on the other edge inside a span is refused
//! ([`SharedError::Unaligned`]). A common end of abutting edges (a point)
//! has no inside: no crossing.
//!
//! **Signs at stored points.** At `P`, `Q` and each inner knot, `h`'s sign
//! is that of the moved configuration, exactly: the first of `h₁ … h₄`
//! that isn't 0 ([`shared_crossing_signs`]), with the exact tangent (at a
//! span's end the chord to its middle control point; at an inner knot
//! from both sides, which must agree) and the edge's exact rates there
//! ([`XGeometry::edge_point_rates`]; on a line, at an end of `g` inside
//! `e`, the rates of `e`'s point under it, [`XGeometry::edge_line`]). So
//! an exact zero of `h₁` at an overlap end is decided like any tie, and
//! there is no crossing at an end unless its moved sign differs from
//! `h`'s just inside: end ties of the four terms are never crossings.
//!
//! **Pieces.** Each run's spans are pieces of `e`'s parameter, each with
//! the moved sign of `h` at its two ends, refined breadth first at each
//! precision level. A piece is settled where
//!
//! - `h₁` is certified one sign over it, the ends' signs alike: no root;
//! - or its **z-order is clear** over it: the shared-curve z-order row
//!   ([`shared_zorder`], order 1; the heights coincide, so order 0 is
//!   exactly 0) certified one sign over the piece, or, for two distinct
//!   lines on one vertical plane, their height gap (exact, linear along
//!   the overlap) one strict sign over it. Every root of `h` in the piece
//!   then reads that z-order, so the piece is one cluster of degree the
//!   change of sign across it (the ends' signs, oriented as above), none
//!   where it is 0 (a tangential root, or a pair of one z-order).
//!
//! Any other piece is split at the first of the canonical positions
//! (the middle, then shifted ones, mirrored on an edge whose geometric
//! order runs against its stored direction) where `h₁` is certified
//! non-zero, its sign there the halves' common end; a piece of non-zero
//! degree is also split while wider than [`NARROW_PIECE`] and a position
//! certifies, so roots apart are separated. A piece no position splits
//! within [`MIN_H_PIECE`], or more than [`MAX_H_PIECES`] evaluations at a
//! level, leaves the level to the next; past the last the pair is refused
//! ([`SharedError::Unsettled`]: a root of `h` at a point no level
//! separates from its neighbours of another z-order, a vertical shadow
//! tangent, `h ≡ 0` on a curved stretch whose z-order isn't clear).
//!
//! **Clusters** ([`ZRow::Root`]). Each settled piece of non-zero degree is
//! a cluster at the root of `h` inside it, its piece of `e` and the
//! matching piece of `g`, its z-order clear (distinct lines) or read from
//! the shared-curve z-order key `XRoot(e, g, c)` at the root
//! ([`HKey::XRoot`](crate::records::keys::HKey::XRoot)). The root
//! clusters come first in the record, numbered along `e` from its
//! geometrically lesser end (the point-on-edge order), never by id.
//!
//! **Away from the overlap** (the curve's shadow crossing itself, or the
//! parts of `e` and `g` off the overlap crossing): the ordinary box
//! search of the square ([`crate::count::xrec::form`]), skipping the
//! boxes of a **diagonal band**: those whose two pieces lie on one
//! stretch of the curve (one span, or two sharing a knot) whose shadow is
//! certified injective there (every control point difference of the
//! stretch, oriented along it, certified on the positive side of one
//! direction), so the shadows meet there only where the curve meets
//! itself, on the overlap. Its clusters follow the root clusters. Two
//! lines never cross away from their overlap.
//!
//! **Check.** The clusters' degrees sum to the four terms' `X`; a
//! mismatch is a rule gap, never answered ([`SharedError::Inconsistent`]).
//!
//! **Cut points** ([`root_order`]). The cut point of a root cluster is
//! keyed by the pair and its index ([`Query::Root`](crate::records::keys::Query::Root)).
//! Against a point `w` of `e`'s curve it is ordered by the sign of `h(w)`:
//! inside the cluster's piece, `w` lies before the root exactly where
//! `h(w)` has the sign `h` has before it; outside, the piece decides.

use std::cmp::Ordering;
use std::collections::VecDeque;

use glam::DVec3;
use malachite::Rational;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::{Ceiling, Floor};

use crate::bezier::Dyadic;
use crate::count::boxes::SPLIT_SIXTEENTHS;
use crate::count::s02::{Charge, Exhausted};
use crate::count::xrec::{Allow, XClusterError, form_charged};
use crate::ids::EdgeId;
use crate::num::bigf::{Big, BigF, Width};
use crate::num::bounded::Bounded;
use crate::num::dd::Dd;
use crate::num::decide::Sign;
use crate::num::exact::{ORDERS, rat};
use crate::num::schedule::LEVELS;
use crate::order::{Along, EntityOrder};
use crate::perturb::{
    Curved, NU, R_HAT, UP, between, curve_point, rate_rational, shared_crossing_h1,
    shared_crossing_signs, shared_zorder,
};
use crate::records::height::HeightReads;
use crate::records::keys::{EdgeAt, Operand, Point, RayKey, RecordKey, XKey};
use crate::records::ray::Span;
use crate::records::shadow::Cost;

use super::{XCluster, XCurve, XGeometry, XPair, XPart, XRec, XSides, XWant, ZRow, x_count};

/// Why a shared-curve pair isn't answered (module docs): never a wrong
/// record, a refusal naming what it met.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SharedError {
    /// An end's feet on the other edge aren't decided.
    Missing(RecordKey),
    /// An edge has no spans, one is straight and the other curved, two
    /// straight edges on one curve aren't on one line in the shadow, or
    /// two curved edges aren't on one curve.
    Shape,
    /// This end of one edge lies on the other edge inside one of its
    /// spans, not at a knot or vertex of it: the overlap isn't made of
    /// whole spans of both, which the rule reads it as.
    Unaligned(Point),
    /// No level settles `h` over the overlap: roots of `h` no level
    /// separates whose z-orders aren't one, a vertical shadow tangent
    /// (`T_xy = 0`), `h ≡ 0` on a curved stretch whose z-order isn't
    /// clear, or the signs at a knot's two sides differ (a corner).
    Unsettled,
    /// The box search away from the overlap reads these side and corner
    /// records, not yet decided: fill them and ask again.
    Need(Vec<XWant>),
    /// The box search away from the overlap failed so.
    Away(XClusterError),
    /// The root search ran past the share it was given
    /// ([`shared_clusters_charged`]): [`Exhausted::Work`] at the first
    /// level, [`Exhausted::Precision`] above it. (The box search away from
    /// the overlap runs out as [`SharedError::Away`].)
    OutOfBudget(Exhausted),
    /// The clusters' degrees don't sum to the four terms' count: a gap in
    /// the rule, never answered.
    Inconsistent {
        /// The clusters' degrees summed.
        clusters: i64,
        /// The four terms' count.
        x: i64,
    },
}

/// The narrowest piece of a span's parameter `h` is halved to: below
/// it, a level that hasn't settled the piece leaves it to the next
/// (`2⁻⁴⁰`).
pub const MIN_H_PIECE: f64 = 1.0 / 1_099_511_627_776.0;

/// The most evaluations of `h` one level makes over a pair's overlap
/// before it is left to the next level.
pub const MAX_H_PIECES: usize = 4096;

/// A root cluster's piece is split while wider than this (`2⁻¹²` of its
/// span) where a position certifies, separating roots apart.
pub const NARROW_PIECE: f64 = 1.0 / 4096.0;

/// A run of the overlap (module docs): consecutive pieces of `e`.
#[derive(Clone, Debug)]
struct Run {
    /// Each span of `e` with its parameter range, along `e`'s stored
    /// direction.
    segs: Vec<(u32, [f64; 2])>,
    /// The moved sign of `h` at the run's start, at the knot after each
    /// span but the last (from its two sides: a shadow corner there, at
    /// the stored data's rounding, may make them differ), and at its end:
    /// `[before, after]` along `e`.
    signs: Vec<[Sign; 2]>,
    /// `+1` where `g` runs along `e`'s stored direction over the run.
    dir: i64,
    /// The matching piece of `g`.
    on_g: OnG,
    /// Two distinct lines: their height gap `ê·(C_g − C_e)` as `c₀ + c₁·t`
    /// on `e`'s parameter, exactly; `None` where the heights coincide.
    gap: Option<[Rational; 2]>,
    /// A line's overlap ends on `e`'s parameter, exactly: no split past
    /// them.
    inside: Option<[Rational; 2]>,
}

/// Where a piece of `e` lies on `g`.
#[derive(Clone, Debug)]
enum OnG {
    /// The span of `g` matching each span of the run, and whether it is
    /// stored the other way round (its parameter `1 − t`).
    Spans(Vec<(u32, bool)>),
    /// One line: `g`'s parameter `s₀ + s₁·t` at `e`'s `t`.
    Line([Rational; 2]),
}

/// A piece being refined: a part of span `seg` of run `run`, the moved
/// signs of `h` at its two ends.
#[derive(Clone, Copy, Debug)]
struct Piece {
    run: usize,
    seg: usize,
    t: [f64; 2],
    ends: [Sign; 2],
}

/// A settled piece of non-zero degree: a root cluster.
#[derive(Clone, Copy, Debug)]
struct Leaf {
    piece: Piece,
    degree: i64,
    /// Its z-order, certified over it.
    z: Sign,
    /// Whether it is that z-order's record (two distinct lines), not the
    /// shared-curve key's.
    clear: bool,
    /// A knot whose two sides' signs of `h` differ.
    knot: bool,
}

/// What the pieces read at every level.
struct Ctx<'a, G> {
    geom: &'a G,
    e: EdgeId,
    curve: XCurve<'a>,
    runs: Vec<Run>,
    /// The split positions mirrored: `e`'s geometric order runs against
    /// its stored direction.
    mirror: bool,
}

/// The record of a shared-curve pair `x` (one curve, or two straight
/// edges whose shadows lie on one line; module docs): a cluster at each
/// sign change of `h` along the overlap, numbered along `e` in geometric
/// order, then the clusters of the box search away from the overlap,
/// checked against the four terms. `geom` gives `e`'s rates; `reads` the
/// order and the ends' feet; `sides` the side records the box search
/// reads ([`SharedError::Need`] for those missing).
pub fn shared_clusters<R: HeightReads, G: XGeometry>(
    reads: &R,
    geom: &G,
    sides: &XSides,
    x: XKey,
    pair: &XPair<'_>,
) -> Result<XRec, SharedError> {
    shared_clusters_charged(reads, geom, sides, x, pair, &Charge::xrec(u64::MAX)).0
}

/// [`shared_clusters`], charged against `charge` at its level's unit per
/// piece ([`Charge::xrec`]): each evaluation of `h` the root search makes
/// and every piece the box search away from the overlap tests (its
/// diagonal band's included, [`form_charged`]), on what the roots left.
/// With the record, or the error, the pieces per level
/// ([`LevelWork::evals`](crate::records::shadow::LevelWork::evals)) for
/// the pass to charge ([`Charge::spent`]).
pub fn shared_clusters_charged<R: HeightReads, G: XGeometry>(
    reads: &R,
    geom: &G,
    sides: &XSides,
    x: XKey,
    pair: &XPair<'_>,
    charge: &Charge,
) -> (Result<XRec, SharedError>, Cost) {
    let mut cost = Cost::default();
    let r = shared_in(reads, geom, sides, x, pair, charge, &mut cost);
    (r, cost)
}

fn shared_in<R: HeightReads, G: XGeometry>(
    reads: &R,
    geom: &G,
    sides: &XSides,
    x: XKey,
    pair: &XPair<'_>,
    charge: &Charge,
    cost: &mut Cost,
) -> Result<XRec, SharedError> {
    if pair.e.span_count() == 0 || pair.g.span_count() == 0 {
        return Err(SharedError::Shape);
    }
    let chains = matches!((pair.e, pair.g), (XCurve::Spans(_), XCurve::Spans(_)));
    let runs = match (pair.e, pair.g) {
        (XCurve::Line(a, b), XCurve::Line(g0, g1)) => {
            line_run(geom, x, [a, b, g0, g1], pair.shared)?
                .into_iter()
                .collect()
        }
        (XCurve::Spans(es), XCurve::Spans(gs)) if pair.shared => {
            aligned(reads, x, pair, es, gs)?;
            chain_runs(geom, x.e, es, gs)?
        }
        _ => return Err(SharedError::Shape),
    };
    let ctx = Ctx {
        geom,
        e: x.e,
        curve: pair.e,
        runs,
        mirror: along(reads, Operand::A, x.e) == Along::Reversed,
    };
    let mut leaves = None;
    let mut spent = 0u64;
    for level in 0..LEVELS {
        let allow = Allow {
            unit: charge.units[level],
            left: charge.share.saturating_sub(spent),
        };
        let mut evals = 0u64;
        let r = roots_at(&ctx, level, allow, &mut evals);
        cost.levels[level].evals = cost.levels[level]
            .evals
            .saturating_add(u32::try_from(evals).unwrap_or(u32::MAX));
        spent = spent.saturating_add(evals.saturating_mul(allow.unit));
        match r {
            None => break,
            Some(Ok(l)) => {
                leaves = Some(l);
                break;
            }
            Some(Err(SharedError::Unsettled)) => {}
            Some(Err(SharedError::OutOfBudget(Exhausted::Work))) if level > 0 => {
                return Err(SharedError::OutOfBudget(Exhausted::Precision));
            }
            Some(Err(e)) => return Err(e),
        }
    }
    let mut leaves = leaves.ok_or(SharedError::Unsettled)?;
    // Along `e` in geometric order.
    leaves.sort_by(|a, b| {
        let key = |l: &Leaf| (ctx.runs[l.piece.run].segs[l.piece.seg].0, l.piece.t);
        let (ka, kb) = (key(a), key(b));
        ka.0.cmp(&kb.0)
            .then(ka.1[0].total_cmp(&kb.1[0]))
            .then(ka.1[1].total_cmp(&kb.1[1]))
    });
    if ctx.mirror {
        leaves.reverse();
    }
    let mut clusters: Vec<XCluster> = leaves.iter().map(|l| ctx.cluster(l)).collect();
    let mut level = 0;
    if chains {
        let rest = Charge {
            share: charge.share.saturating_sub(spent),
            units: charge.units,
        };
        let (away, c) = form_charged(x, pair, sides, &rest);
        *cost = cost.and(c);
        let away = away.map_err(|e| match e {
            XClusterError::Need(ks) => SharedError::Need(ks),
            e => SharedError::Away(e),
        })?;
        level = away.rec.level;
        clusters.extend(away.rec.clusters);
    }
    let sum: i64 = clusters.iter().map(|c| c.degree).sum();
    let want = x_count(&pair.terms);
    if sum != want {
        return Err(SharedError::Inconsistent {
            clusters: sum,
            x: want,
        });
    }
    Ok(XRec { clusters, level })
}

/// The way edge `g` of `of`'s stored parameter runs from its lesser end.
fn along<O: EntityOrder>(order: &O, of: Operand, g: EdgeId) -> Along {
    let [s, e] = order.ends(of, g);
    Along::of(&order.vert_key(of, s), &order.vert_key(of, e))
}

/// Each end of either chain must be a span end of the other or have no
/// feet on it (module docs, "The overlap").
fn aligned<R: HeightReads>(
    reads: &R,
    x: XKey,
    pair: &XPair<'_>,
    es: &[Span],
    gs: &[Span],
) -> Result<(), SharedError> {
    let ends = |s: &[Span]| -> Vec<[f64; 3]> {
        s.iter()
            .flat_map(|sp| [sp[0], sp[2]])
            .map(|p| [p[0], p[1], p[2]])
            .collect()
    };
    let (e_pts, g_pts) = (ends(es), ends(gs));
    for (of, edge, other, curve, theirs) in [
        (Operand::A, x.e, x.g, &pair.e, &g_pts),
        (Operand::B, x.g, x.e, &pair.g, &e_pts),
    ] {
        for (i, v) in reads.ends(of, edge).into_iter().enumerate() {
            let p = curve.end(i == 1).ok_or(SharedError::Shape)?;
            if theirs.contains(&p) {
                continue;
            }
            let q = Point::vert(of, v);
            let feet = reads
                .feet(q, other)
                .ok_or(SharedError::Missing(RecordKey::Ray(RayKey { q, g: other })))?;
            if !feet.feet.is_empty() {
                return Err(SharedError::Unaligned(q));
            }
        }
    }
    Ok(())
}

/// The affine point of a control point, exactly.
fn affine(p: &[f64; 4]) -> [Rational; 3] {
    [rat(p[0]), rat(p[1]), rat(p[2])]
}

fn qsub(a: &[Rational; 3], b: &[Rational; 3]) -> [Rational; 3] {
    [&a[0] - &b[0], &a[1] - &b[1], &a[2] - &b[2]]
}

/// The moved sign of `h` at a stored point (module docs): the first of
/// its orders that isn't 0; `None` where all are (a vertical tangent).
fn moved_sign(tangent: &[Rational; 3], rates: &[crate::perturb::Rate; ORDERS]) -> Option<Sign> {
    let d: [[Rational; 3]; ORDERS] = std::array::from_fn(|k| rate_rational(&rates[k]));
    shared_crossing_signs(tangent, &d)
        .into_iter()
        .find_map(Sign::of)
}

/// The runs of two chains on one curve (module docs).
fn chain_runs<G: XGeometry>(
    geom: &G,
    e: EdgeId,
    es: &[Span],
    gs: &[Span],
) -> Result<Vec<Run>, SharedError> {
    let back = |sp: &Span| [sp[2], sp[1], sp[0]];
    let matches: Vec<Option<(u32, bool)>> = es
        .iter()
        .map(|sp| {
            (0u32..).zip(gs).find_map(|(j, g)| {
                if *g == *sp {
                    Some((j, false))
                } else if *g == back(sp) {
                    Some((j, true))
                } else {
                    None
                }
            })
        })
        .collect();
    let rates = |k: u32, at: Dyadic| -> Result<[crate::perturb::Rate; ORDERS], SharedError> {
        let p = EdgeAt::new(k, at).ok_or(SharedError::Shape)?;
        Ok(geom.edge_point_rates(Operand::A, e, p))
    };
    let mut runs = Vec::new();
    let mut k = 0usize;
    while k < es.len() {
        let Some((_, rev)) = matches[k] else {
            k += 1;
            continue;
        };
        let start = k;
        let mut map = Vec::new();
        while let Some(&Some((j, r))) = matches.get(k) {
            if r != rev {
                return Err(SharedError::Shape);
            }
            map.push((j, r));
            k += 1;
        }
        let at = |i: usize| u32::try_from(i).map_err(|_| SharedError::Shape);
        let first = &es[start];
        let p = moved_sign(
            &qsub(&affine(&first[1]), &affine(&first[0])),
            &rates(at(start)?, Dyadic::ZERO)?,
        )
        .ok_or(SharedError::Unsettled)?;
        let mut signs = vec![[p, p]];
        for i in start + 1..k {
            let r = rates(at(i)?, Dyadic::ZERO)?;
            let left = moved_sign(&qsub(&affine(&es[i - 1][2]), &affine(&es[i - 1][1])), &r);
            let right = moved_sign(&qsub(&affine(&es[i][1]), &affine(&es[i][0])), &r);
            // A vertical tangent: no rule here.
            signs.push([
                left.ok_or(SharedError::Unsettled)?,
                right.ok_or(SharedError::Unsettled)?,
            ]);
        }
        let last = &es[k - 1];
        let q = moved_sign(
            &qsub(&affine(&last[2]), &affine(&last[1])),
            &rates(at(k - 1)?, Dyadic::ONE)?,
        )
        .ok_or(SharedError::Unsettled)?;
        signs.push([q, q]);
        runs.push(Run {
            segs: (start..k)
                .map(|i| Ok((at(i)?, [0.0, 1.0])))
                .collect::<Result<_, SharedError>>()?,
            signs,
            dir: if rev { -1 } else { 1 },
            on_g: OnG::Spans(map),
            gap: None,
            inside: None,
        });
    }
    Ok(runs)
}

/// The shadow coordinates `(r̂·p, ν·p)` and height `ê·p` of a point,
/// exactly (unnormalised axes: ratios along a line are kept).
fn frame(p: [f64; 3]) -> ([Rational; 2], Rational) {
    let p = p.map(rat);
    let axis = |v: DVec3| [v.x, v.y, v.z].map(rat);
    let dot = |a: &[Rational; 3]| &a[0] * &p[0] + &a[1] * &p[1] + &a[2] * &p[2];
    ([dot(&axis(R_HAT)), dot(&axis(NU))], dot(&axis(UP)))
}

/// The run of two straight edges `e = a→b`, `g = g0→g1` whose shadows
/// lie on one line (module docs): `None` where the shadows don't share a
/// stretch (apart, meeting at a point, parallel apart, a shadow a point);
/// [`SharedError::Shape`] for a pair on one curve whose shadows aren't on
/// one line; [`SharedError::Unsettled`] for such a pair whose `e` is
/// vertical (`T_xy = 0`).
fn line_run<G: XGeometry>(
    geom: &G,
    x: XKey,
    [a, b, g0, g1]: [[f64; 3]; 4],
    shared: bool,
) -> Result<Option<Run>, SharedError> {
    let ((sa, za), (sb, zb), (s0, z0), (s1, z1)) = (frame(a), frame(b), frame(g0), frame(g1));
    let d2 = |p: &[Rational; 2], o: &[Rational; 2]| [&p[0] - &o[0], &p[1] - &o[1]];
    let cross = |u: &[Rational; 2], v: &[Rational; 2]| &u[0] * &v[1] - &u[1] * &v[0];
    let dot = |u: &[Rational; 2], v: &[Rational; 2]| &u[0] * &v[0] + &u[1] * &v[1];
    let zero = Rational::from(0u32);
    let one = Rational::from(1u32);
    let (de, dg) = (d2(&sb, &sa), d2(&s1, &s0));
    let ee = dot(&de, &de);
    if ee == zero {
        return if shared {
            Err(SharedError::Unsettled)
        } else {
            Ok(None)
        };
    }
    let collinear = cross(&de, &d2(&s0, &sa)) == zero && cross(&de, &d2(&s1, &sa)) == zero;
    if !collinear {
        return if shared {
            Err(SharedError::Shape)
        } else {
            Ok(None)
        };
    }
    let gg = dot(&dg, &dg);
    if gg == zero {
        return Ok(None);
    }
    let t0 = dot(&d2(&s0, &sa), &de) / &ee;
    let t1 = dot(&d2(&s1, &sa), &de) / &ee;
    let (lo, lo_at, hi, hi_at) = if t0 <= t1 {
        (t0, g0, t1, g1)
    } else {
        (t1, g1, t0, g0)
    };
    let (lo, lo_at) = if lo > zero {
        (lo, Some(lo_at))
    } else {
        (zero.clone(), None)
    };
    let (hi, hi_at) = if hi < one {
        (hi, Some(hi_at))
    } else {
        (one.clone(), None)
    };
    if lo >= hi {
        return Ok(None);
    }
    // The moved signs at the overlap's ends: an end of `e`, or an end of
    // `g` over `e`, read at `e`'s point under it.
    let tangent = qsub(&b.map(rat), &a.map(rat));
    let sign_at = |end: Option<[f64; 3]>, last: bool| -> Result<Sign, SharedError> {
        let rates = match end {
            Some(p) => geom.edge_line(Operand::A, x.e, DVec3::from_array(p)).rates,
            None => {
                let at = if last { Dyadic::ONE } else { Dyadic::ZERO };
                let p = EdgeAt::new(0, at).ok_or(SharedError::Shape)?;
                geom.edge_point_rates(Operand::A, x.e, p)
            }
        };
        moved_sign(&tangent, &rates).ok_or(SharedError::Unsettled)
    };
    let (p, q) = (sign_at(lo_at, false)?, sign_at(hi_at, true)?);
    let signs = vec![[p, p], [q, q]];
    let dir = if dot(&de, &dg) > zero { 1 } else { -1 };
    // `g`'s parameter at `e`'s `t`, and the heights' gap there.
    let sg = [dot(&d2(&sa, &s0), &dg) / &gg, dot(&de, &dg) / &gg];
    let dz = &z1 - &z0;
    let gap = [&z0 + &sg[0] * &dz - &za, &sg[1] * &dz - (&zb - &za)];
    let gap = (gap[0] != zero || gap[1] != zero).then_some(gap);
    let lo_f = f64::rounding_from(&lo, Floor).0;
    let hi_f = f64::rounding_from(&hi, Ceiling).0;
    Ok(Some(Run {
        segs: vec![(0, [lo_f, hi_f])],
        signs,
        dir,
        on_g: OnG::Line(sg),
        gap,
        inside: Some([lo, hi]),
    }))
}

/// The root clusters at precision level `level` ([`LEVELS`]), `None`
/// past the last.
fn roots_at<G: XGeometry>(
    ctx: &Ctx<'_, G>,
    level: usize,
    allow: Allow,
    evals: &mut u64,
) -> Option<Result<Vec<Leaf>, SharedError>> {
    Some(match level {
        0 => roots(ctx, &Bounded::exact(0.0), allow, evals),
        1 => roots(ctx, &Bounded::exact(Dd::from_f64(0.0)), allow, evals),
        _ => {
            let w = *Width::ALL.get(level - 2)?;
            roots(ctx, &Bounded::exact(Big::from_f64(0.0, w)), allow, evals)
        }
    })
}

/// `h₁` and the curve's tangent and `e`'s rates at `t`, over span `span`
/// of `e`, at `like`'s level.
type At<S> = (S, [S; 3], [[S; 3]; ORDERS]);

impl<G: XGeometry> Ctx<'_, G> {
    fn at<S: Curved>(&self, span: u32, t: &S, like: &S) -> Result<At<S>, SharedError> {
        let tangent = match self.curve {
            // A line's tangent, exactly: no spread from the parameter.
            XCurve::Line(a, b) => {
                [0, 1, 2].map(|i| S::from_f64(b[i], like).sub(&S::from_f64(a[i], like)))
            }
            XCurve::Spans(_) => {
                let c = self.curve.span(span, like).ok_or(SharedError::Shape)?;
                curve_point(&c, t).1
            }
        };
        let rates = self.geom.edge_rates(Operand::A, self.e, span, t);
        let h = shared_crossing_h1(&tangent, &rates[0]);
        Ok((h, tangent, rates))
    }

    /// The certified sign of `h₁` at `x` of span `span`.
    fn sign_at<S: Curved>(&self, span: u32, x: f64, like: &S) -> Result<Option<Sign>, SharedError> {
        let (h, _, _) = self.at(span, &S::from_f64(x, like), like)?;
        Ok(h.sign().and_then(Sign::of))
    }

    /// The piece split at the first canonical position (module docs)
    /// where `h₁` is certified, `None` where none is.
    fn split<S: Curved>(&self, p: &Piece, like: &S) -> Result<Option<[Piece; 2]>, SharedError> {
        let [lo, hi] = p.t;
        // Also where a bound isn't a number.
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(hi - lo > MIN_H_PIECE) {
            return Ok(None);
        }
        let run = &self.runs[p.run];
        let span = run.segs[p.seg].0;
        for k in SPLIT_SIXTEENTHS {
            let k = if self.mirror { 16 - k } else { k };
            let x = lo + (hi - lo) * (k as f64 / 16.0);
            if !(lo < x && x < hi) {
                continue;
            }
            if let Some([a, b]) = &run.inside {
                let q = rat(x);
                if !(*a < q && q < *b) {
                    continue;
                }
            }
            if let Some(s) = self.sign_at(span, x, like)? {
                return Ok(Some([
                    Piece {
                        t: [lo, x],
                        ends: [p.ends[0], s],
                        ..*p
                    },
                    Piece {
                        t: [x, hi],
                        ends: [s, p.ends[1]],
                        ..*p
                    },
                ]));
            }
        }
        Ok(None)
    }

    /// The z-order over the piece where it is clear (module docs).
    fn clear<S: Curved>(
        &self,
        run: &Run,
        p: &Piece,
        tangent: &[S; 3],
        rates: &[[S; 3]; ORDERS],
    ) -> Option<Sign> {
        match &run.gap {
            Some([c0, c1]) => {
                let at = |t: f64| (c0 + c1 * rat(t)).cmp(&Rational::from(0u32));
                let (a, b) = (at(p.t[0]), at(p.t[1]));
                (a == b).then(|| Sign::of(a)).flatten()
            }
            None => {
                let ord = shared_zorder(tangent, rates)?;
                ord[0].sign().and_then(Sign::of)
            }
        }
    }

    /// The z-order at the knot starting span `seg` of the run where it is
    /// clear: the shared-curve row there certified one sign with the
    /// tangents of both sides (the chords to the middle control points)
    /// and `e`'s rates at the knot.
    fn knot_clear<S: Curved>(
        &self,
        run: &Run,
        seg: usize,
        like: &S,
    ) -> Result<Option<Sign>, SharedError> {
        let XCurve::Spans(es) = self.curve else {
            return Err(SharedError::Shape);
        };
        let span = run.segs[seg].0;
        let k = usize::try_from(span).map_err(|_| SharedError::Shape)?;
        let (left, right) = (es.get(k - 1).ok_or(SharedError::Shape)?, &es[k]);
        let at = EdgeAt::new(span, Dyadic::ZERO).ok_or(SharedError::Shape)?;
        let rates = self
            .geom
            .edge_point_rates(Operand::A, self.e, at)
            .map(|r| r.at(like));
        let chord = |p: &[f64; 4], o: &[f64; 4]| {
            [0, 1, 2].map(|i| S::from_f64(p[i], like).sub(&S::from_f64(o[i], like)))
        };
        let mut sign = None;
        for t in [chord(&left[2], &left[1]), chord(&right[1], &right[0])] {
            let Some(s) = shared_zorder(&t, &rates)
                .and_then(|o| o[0].sign())
                .and_then(Sign::of)
            else {
                return Ok(None);
            };
            if sign.is_some_and(|x| x != s) {
                return Ok(None);
            }
            sign = Some(s);
        }
        Ok(sign)
    }

    /// The record's cluster of a leaf.
    fn cluster(&self, l: &Leaf) -> XCluster {
        let run = &self.runs[l.piece.run];
        let (span, _) = run.segs[l.piece.seg];
        let [lo, hi] = l.piece.t;
        let g = match &run.on_g {
            OnG::Spans(map) => {
                let (j, rev) = map[l.piece.seg];
                XPart {
                    span: j,
                    t: if rev { [1.0 - hi, 1.0 - lo] } else { [lo, hi] },
                }
            }
            OnG::Line([s0, s1]) => {
                let at = |t: f64| s0 + s1 * rat(t);
                let (a, b) = (at(lo), at(hi));
                let (a, b) = if a <= b { (a, b) } else { (b, a) };
                let clamp = |x: f64| x.clamp(0.0, 1.0);
                XPart {
                    span: 0,
                    t: [
                        clamp(f64::rounding_from(&a, Floor).0),
                        clamp(f64::rounding_from(&b, Ceiling).0),
                    ],
                }
            }
        };
        XCluster {
            e: XPart { span, t: [lo, hi] },
            g,
            degree: l.degree,
            clear: l.clear.then_some(l.z),
            row: ZRow::Root {
                before: l.piece.ends[0],
            },
        }
    }
}

/// The root clusters at `like`'s level (module docs, "Pieces").
/// Each evaluation of `h` is added to `charged`, the search stopping
/// once they cost more than `allow` gives.
fn roots<S: Curved, G: XGeometry>(
    ctx: &Ctx<'_, G>,
    like: &S,
    allow: Allow,
    charged: &mut u64,
) -> Result<Vec<Leaf>, SharedError> {
    // Breadth first, so every part of the overlap is looked at before
    // any is refined far.
    let mut work: VecDeque<Piece> = VecDeque::new();
    for (r, run) in ctx.runs.iter().enumerate() {
        for (i, &(_, t)) in run.segs.iter().enumerate() {
            work.push_back(Piece {
                run: r,
                seg: i,
                t,
                ends: [run.signs[i][1], run.signs[i + 1][0]],
            });
        }
    }
    let mut out = Vec::new();
    // A knot whose two sides' signs differ is a root there.
    for (r, run) in ctx.runs.iter().enumerate() {
        for i in 1..run.segs.len() {
            let [before, after] = run.signs[i];
            if before == after {
                continue;
            }
            let z = ctx
                .knot_clear(run, i, like)?
                .ok_or(SharedError::Unsettled)?;
            out.push(Leaf {
                piece: Piece {
                    run: r,
                    seg: i,
                    t: [0.0, 0.0],
                    ends: [before, after],
                },
                degree: run.dir * i64::from(after.as_i8() - before.as_i8()) / 2,
                z,
                clear: run.gap.is_some(),
                knot: true,
            });
        }
    }
    let mut evals = 0usize;
    while let Some(p) = work.pop_front() {
        evals += 1;
        if evals > MAX_H_PIECES {
            return Err(SharedError::Unsettled);
        }
        *charged = charged.saturating_add(1);
        if allow.over(*charged) {
            return Err(SharedError::OutOfBudget(Exhausted::Work));
        }
        let run = &ctx.runs[p.run];
        let span = run.segs[p.seg].0;
        let (h, tangent, rates) = ctx.at(span, &between(p.t[0], p.t[1], like), like)?;
        if let Some(s) = h.sign().and_then(Sign::of)
            && p.ends == [s, s]
        {
            continue;
        }
        let degree = run.dir * i64::from(p.ends[1].as_i8() - p.ends[0].as_i8()) / 2;
        if let Some(z) = ctx.clear(run, &p, &tangent, &rates) {
            if degree == 0 {
                continue;
            }
            if p.t[1] - p.t[0] > NARROW_PIECE
                && let Some(halves) = ctx.split(&p, like)?
            {
                work.extend(halves);
                continue;
            }
            out.push(Leaf {
                piece: p,
                degree,
                z,
                clear: run.gap.is_some(),
                knot: false,
            });
            continue;
        }
        match ctx.split(&p, like)? {
            Some(halves) => work.extend(halves),
            None => return Err(SharedError::Unsettled),
        }
    }
    Ok(merge_knots(out))
}

/// The leaves with each knot root merged with the leaves touching the
/// knot from either side whose z-order is its own (module docs,
/// "Pieces"): roots on either side of a knot whose two sides' signs of
/// `h` differ lie within the stored data's rounding of it, one crossing
/// of the ideal curve; the knot keeps their summed degree, none where it
/// is 0.
fn merge_knots(leaves: Vec<Leaf>) -> Vec<Leaf> {
    let touches = |k: &Leaf, l: &Leaf| {
        !l.knot
            && l.piece.run == k.piece.run
            && l.z == k.z
            && ((l.piece.seg + 1 == k.piece.seg && l.piece.t[1] == 1.0)
                || (l.piece.seg == k.piece.seg && l.piece.t[0] == 0.0))
    };
    let (knots, rest): (Vec<Leaf>, Vec<Leaf>) = leaves.into_iter().partition(|l| l.knot);
    let mut used = vec![false; rest.len()];
    let mut out = Vec::new();
    for mut k in knots {
        for (l, u) in rest.iter().zip(used.iter_mut()) {
            if !*u && touches(&k, l) {
                k.degree += l.degree;
                // The signs before and after the merged stretch.
                if l.piece.seg < k.piece.seg {
                    k.piece.ends[0] = l.piece.ends[0];
                } else {
                    k.piece.ends[1] = l.piece.ends[1];
                }
                *u = true;
            }
        }
        if k.degree != 0 {
            out.push(k);
        }
    }
    out.extend(
        rest.into_iter()
            .zip(used)
            .filter(|(_, u)| !u)
            .map(|(l, _)| l),
    );
    out
}

/// A point of `e`'s curve a root cluster's cut point is ordered against
/// ([`root_order`]): span `span` of `e` at the parameter `t`, along `e`'s
/// stored direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurveAt {
    /// The span (0 on a straight edge).
    pub span: u32,
    /// The parameter on it, in `[0, 1]`.
    pub t: f64,
}

/// Where the cut point of root cluster `cl` of the shared-curve pair `x`
/// lies against the point `w` of `e`'s curve, along `e`'s stored
/// direction (module docs, "Cut points"): `Less` where it lies before
/// `w`. Outside the cluster's piece of `e`, the piece decides; inside,
/// the sign of `h` at `w`: exactly at a span's end (a vertex or knot of
/// `e`, by the moved sign there), else certified at the first precision
/// level that does. A root at a knot whose two sides' signs differ lies
/// after that knot in the geometric order (`order` gives it). `None` for
/// a cluster that isn't a root cluster, or where no level certifies
/// `h(w)` (at or too near the root).
pub fn root_order<O: EntityOrder, G: XGeometry>(
    order: &O,
    geom: &G,
    x: XKey,
    curve: &XCurve<'_>,
    cl: &XCluster,
    w: CurveAt,
) -> Option<Ordering> {
    let ZRow::Root { before } = cl.row else {
        return None;
    };
    let [lo, hi] = cl.e.t;
    // A root at a knot (its two sides' signs of `h` differ, so `h(w)`
    // there has no one sign) against that knot: the cut point after it
    // along the geometric order, whichever way `e` is stored.
    let at_knot = cl.e.t == [0.0, 0.0]
        && cl.e.span > 0
        && (w
            == CurveAt {
                span: cl.e.span,
                t: 0.0,
            }
            || w == CurveAt {
                span: cl.e.span - 1,
                t: 1.0,
            });
    if at_knot {
        return Some(match along(order, Operand::A, x.e) {
            Along::Stored => Ordering::Greater,
            Along::Reversed => Ordering::Less,
        });
    }
    match w.span.cmp(&cl.e.span) {
        Ordering::Less => return Some(Ordering::Greater),
        Ordering::Greater => return Some(Ordering::Less),
        Ordering::Equal if w.t < lo => return Some(Ordering::Greater),
        Ordering::Equal if w.t > hi => return Some(Ordering::Less),
        Ordering::Equal => {}
    }
    let sign = if w.t == 0.0 || w.t == 1.0 {
        let at = if w.t == 0.0 {
            Dyadic::ZERO
        } else {
            Dyadic::ONE
        };
        let rates = geom.edge_point_rates(Operand::A, x.e, EdgeAt::new(w.span, at)?);
        let tangent = match curve {
            XCurve::Line(a, b) => qsub(&b.map(rat), &a.map(rat)),
            XCurve::Spans(s) => {
                let sp = s.get(usize::try_from(w.span).ok()?)?;
                if w.t == 0.0 {
                    qsub(&affine(&sp[1]), &affine(&sp[0]))
                } else {
                    qsub(&affine(&sp[2]), &affine(&sp[1]))
                }
            }
        };
        moved_sign(&tangent, &rates)?
    } else {
        let ctx = Ctx {
            geom,
            e: x.e,
            curve: *curve,
            runs: Vec::new(),
            mirror: false,
        };
        (0..LEVELS).find_map(|level| -> Option<Sign> {
            match level {
                0 => ctx.sign_at(w.span, w.t, &Bounded::exact(0.0)).ok()?,
                1 => ctx
                    .sign_at(w.span, w.t, &Bounded::exact(Dd::from_f64(0.0)))
                    .ok()?,
                _ => {
                    let width = *Width::ALL.get(level - 2)?;
                    ctx.sign_at(w.span, w.t, &Bounded::exact(Big::from_f64(0.0, width)))
                        .ok()?
                }
            }
        })?
    };
    // `w` before the root where `h(w)` has the sign before it: the cut
    // point after `w`.
    Some(if sign == before {
        Ordering::Greater
    } else {
        Ordering::Less
    })
}
