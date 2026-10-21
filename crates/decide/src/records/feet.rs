//! Feet: the stretches of an edge near a point of the other operand. A
//! foot of point `q` on edge `g` is a 3D component of
//! `{s : ‖C_g(s) − q‖ ≤ r}`, `r` the edge's near radius `r_g`, three
//! times its band ([`super::near`]). Feet are part of the point's ray record
//! against the edge; they name the point–curve height key where the point
//! lies exactly on the edge (and the vertex key where it lies exactly at
//! an edge end in the foot), the edge rule's key of the lobe fallback
//! (the first near end in the foot, else the foot's own), and the
//! neighbourhood the fallback re-reads ([`Foot::ends`],
//! [`Foot::end_vertices`]). They are functions of the operands alone,
//! decided once per record.
//!
//! **The polynomial.** On a span with homogeneous point `(P, W)`, `W > 0`
//! (positive weights), the sign of `‖P/W − q‖² − r²` is that of
//! `F = ‖P − q·W‖² − r²·W²`, a Bernstein polynomial of twice the span's
//! degree (4 for a rational quadratic, 2 for a straight edge, a segment
//! of degree 1), formed at each precision level from the stored control
//! points in the frame centred at `q` (each offset `x − q` formed before
//! its weight, so the level's error follows the span's distance from `q`,
//! not the coordinates' magnitude, and a small radius at large
//! coordinates settles at the first level when the span is short). `F ≤
//! 0` is the foot set, closed: a point exactly at distance `r` belongs to
//! it.
//!
//! **Ends and knots, exactly.** A span's ends are its end control points
//! exactly (the edge's vertices and the knots its spans share), so the
//! side of each, `‖k − q‖² − r²`, is decided once, exactly, on the stored
//! `f64`s (zero included: a point on the sphere), and read by the spans
//! on both sides. Where a span's isolation can't certify `F` at its end
//! (a cluster reaching `t = 0` or `t = 1`), that exact side stands, so a
//! foot crossing a knot is one foot, joined there whatever the two spans'
//! levels.
//!
//! **Inside a span.** `F`'s roots on `[0, 1]` are isolated ([`isolate`]):
//! between the results `F` is certified non-zero, a [`Root`] is one
//! boundary point of the set, crossing in or out by its direction, and a
//! cluster counts by its end signs and its root bound: ends of opposite
//! sides with at most two roots hold exactly one crossing (odd count);
//! ends of one side with at most one root hold none; a cluster at a span
//! end exactly on the sphere with at most one root holds just that root,
//! the span beside it of the cluster's other side. Any other cluster (a
//! tangency of the sphere and the curve, two crossings closer than the
//! level separates, a curve lying on the sphere) leaves the span to the
//! exact stage at the first level (below), and where that can't form it,
//! to the next level: double-double, then each big-float width. Past the
//! last level such a cluster is taken **inside** the set as a whole, the
//! closed hull of what the level can't separate: an exact tangency from
//! outside gives a foot at the touching point (the true set there is that
//! point), one from inside joins the stretches on both sides (the
//! touching point belongs to the set), a curve on the sphere is one foot.
//! A foot built so is marked [`Foot::hull`]. The near radius only shares
//! records, so this is never an error. A crossing in the span's second
//! half is bracketed on `F` run backwards (`u = 1 − t`), so a span and
//! the same span stored the other way bracket it alike.
//!
//! **Exactly at the first level.** A cluster the first level leaves (an
//! exact tangency, two crossings closer than `f64` separates) is not
//! carried through the levels: the span's `F` is formed in rationals from
//! the stored values (degree 2 or 4), its distinct roots on `[0, 1]`
//! isolated exactly with their multiplicities, and the set's side between
//! them read at a rational point there. A root where the side changes is
//! a crossing; one where it doesn't (an even multiplicity: the sphere
//! touching the curve) is taken as the hull rule takes it, a foot of the
//! touching point from outside, the stretches beside it joined from
//! inside, marked [`Foot::hull`]; a root at a span end is that end's exact
//! side. A span on the sphere (`F ≡ 0`) is one hull foot.
//!
//! **The feet** run along the edge's stored direction (from its lower-id
//! end), each from a [`FootEnd`] to a [`FootEnd`]: a span and the bounds
//! of a local parameter with the boundary point of the set between them,
//! certified ([`EndKind`]). A foot holding an edge end ends at that
//! vertex. On a closed edge (its ends one vertex) the feet holding the
//! vertex at the start and the end of the chain are one foot, running
//! from its start near the end of the chain through the vertex to its
//! end near the start ([`Foot::wraps`]). [`FootId`] is the index of a foot
//! in this order, a foot holding the start vertex first; a choice among
//! feet reads the geometric order ([`Feet::in_order`]).
//!
//! **Partition-free.** The feet are the components of a set the edge's
//! curve defines, and every end is a certified bracket of that set's
//! boundary point; how the curve is cut into spans changes the brackets
//! only. A cut at a boundary point gives that end exactly at the knot
//! ([`EndKind::OnSphere`]) in place of a bracket. Only where the last
//! level leaves a cluster does a foot's extent read the cluster's
//! interval, below what any level resolves.
//!
//! **The record.** The feet are part of the point's ray record against
//! the edge, keyed by its [`RayKey`], for straight and curved edges
//! alike, and stored in their own table ([`NearFeet`]), filled only in one
//! charged [`Pass`] per batch of keys not yet stored, at each edge's near
//! radius `r_g = 3·β_g` ([`OpBands`](super::near::OpBands), which the
//! geometry gives: [`FeetGeometry::near_radius_of`]). They read nothing but
//! the point's position and the edge's stored curve, so the crossings and
//! the feet of one ray record are decided apart and read together.

use std::cmp::Ordering;

use glam::DVec3;
use malachite::Rational;
use smallvec::SmallVec;

use crate::bezier::roots::{Dyadic, Found, Limits, MAX_ROOT_DEPTH, isolate};
use crate::bezier::{HomCurve, Poly, Scalar};
use crate::ids::{EdgeId, VertId};
use crate::num::decide::{Fail, Row, Sign, TieKey};
use crate::num::exact;
use crate::num::level::{AtLevel, LevelScalar, ladder};
use crate::num::preds::NearGap;
use crate::num::schedule::Level;
use crate::order::Along;
use crate::records::keys::{FootId, Operand, Point, RayKey, RecordKey};
use crate::records::ray::{Span, bracket, mirrored};
use crate::records::shadow::neg;
use crate::records::shadow::{
    Cost, Decided, LevelWork, Pass, ShadowError, ShadowGeometry, replay, shadow_error,
};
use crate::records::table::{Fresh, Table};

#[cfg(doc)]
use crate::bezier::roots::Root;

/// The most splits one span's isolation takes: a quartic's isolation to
/// the deepest level, a few clusters deep, stays far below.
const MAX_SPLITS: u32 = 4096;

/// The curve of an edge, along its stored direction (from its lower-id
/// end).
#[derive(Clone, Copy, Debug)]
pub enum EdgeCurve<'a> {
    /// A straight edge between its two vertices' points.
    Line([DVec3; 2]),
    /// A curved edge: its spans, the first starting at the lower-id
    /// vertex's point, the last ending at the other's, consecutive spans
    /// sharing their joining control point bit for bit.
    Spans {
        /// The spans.
        spans: &'a [Span],
        /// Whether the edge is closed: its two ends one vertex, the
        /// chain's first and last control points one point.
        closed: bool,
    },
}

/// How a foot's end is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EndKind {
    /// The edge's end vertex: the foot reaches it (the vertex lies in the
    /// set). Its bounds are the span's end, `[0, 0]` or `[1, 1]`.
    Vertex,
    /// One certified crossing of the sphere `‖C − q‖ = r` inside the
    /// bounds.
    Crossing,
    /// A span end (a knot, or a vertex) exactly on the sphere, the curve
    /// outside it beyond: bounds `[0, 0]` or `[1, 1]`.
    OnSphere,
    /// A cluster no level separates, taken inside the set: the foot's end
    /// lies somewhere in the bounds.
    Hull,
}

/// An end of a foot: a parameter on one span of the edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FootEnd {
    /// The span, from 0 at the edge's lower-id end (0 on a straight
    /// edge).
    pub span: u32,
    /// `[lo, hi]`, `f64`s on the span's `[0, 1]` with the end between
    /// them.
    pub t: [f64; 2],
    /// How the end is known.
    pub kind: EndKind,
}

/// One foot: a 3D component of the edge's parameters within `r` of the
/// point (module docs), from `lo` to `hi` along the stored direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Foot {
    /// Where it starts.
    pub lo: FootEnd,
    /// Where it ends.
    pub hi: FootEnd,
    /// On a closed edge, whether it runs from `lo` past the chain's end
    /// through the vertex to `hi` near its start.
    pub wraps: bool,
    /// Whether a cluster no level separates is taken inside it (module
    /// docs): it may stand for several true components, or a touching
    /// point, that no level tells apart.
    pub hull: bool,
}

impl Foot {
    /// Whether the edge's stored start and end vertex lie in the foot (on
    /// a closed edge both or neither: they are one vertex).
    pub fn ends(&self) -> [bool; 2] {
        if self.wraps {
            return [true, true];
        }
        [
            self.lo.kind == EndKind::Vertex,
            self.hi.kind == EndKind::Vertex,
        ]
    }

    /// The edge's end vertices lying in the foot, in geometric order: the
    /// edge's ends `ends` (stored start first), its parameter running
    /// `along` from its geometrically lesser end. A closed edge's one
    /// vertex is listed once.
    pub fn end_vertices(&self, ends: [VertId; 2], along: Along) -> SmallVec<[VertId; 2]> {
        let held = self.ends();
        let order = match along {
            Along::Stored => [0, 1],
            Along::Reversed => [1, 0],
        };
        let mut out = SmallVec::new();
        for i in order {
            if held[i] && !out.contains(&ends[i]) {
                out.push(ends[i]);
            }
        }
        out
    }
}

/// A point's feet on an edge of the other operand, in [`FootId`] order
/// (module docs).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Feet {
    /// The feet.
    pub feet: SmallVec<[Foot; 2]>,
}

impl Feet {
    /// Foot `k`, if there is one.
    pub fn get(&self, k: FootId) -> Option<&Foot> {
        usize::try_from(k.0).ok().and_then(|i| self.feet.get(i))
    }

    /// The feet in geometric order on an edge whose stored parameter runs
    /// `along` from its geometrically lesser end: by their parameter from
    /// that end. A foot through a closed edge's vertex holds the parameter
    /// at the vertex, so it comes first either way.
    pub fn in_order(&self, along: Along) -> impl Iterator<Item = FootId> + '_ {
        let n = self.feet.len();
        let wrap = usize::from(self.feet.first().is_some_and(|f| f.wraps));
        let ix: SmallVec<[usize; 4]> = match along {
            Along::Stored => (0..n).collect(),
            Along::Reversed => (0..wrap).chain((wrap..n).rev()).collect(),
        };
        ix.into_iter()
            .map(|i| FootId(u32::try_from(i).expect("few feet")))
    }
}

/// The geometry the feet read, beyond the shadow records'.
pub trait FeetGeometry: ShadowGeometry {
    /// Edge `g` of `operand`'s curve, along its stored direction: its two
    /// vertices' points for a straight edge, its spans for a curved one
    /// (closed where its two ends are one vertex).
    fn curve(&self, operand: Operand, g: EdgeId) -> EdgeCurve<'_>;

    /// Edge `g` of `operand`'s near radius `r_g = 3·β_g` (finite, `≥ 0`):
    /// [`OpBands::near_radius`](super::near::OpBands::near_radius) of its
    /// tolerance.
    fn near_radius_of(&self, operand: Operand, g: EdgeId) -> f64;
}

/// The feet of the ray records of a run, decided once per [`RayKey`]
/// (module docs).
#[derive(Debug, Default)]
pub struct NearFeet {
    feet: Table<RayKey, Feet>,
}

impl NearFeet {
    /// No records.
    pub fn new() -> NearFeet {
        NearFeet::default()
    }

    /// Decides the feet of `wanted` not yet stored, in one pass. A key
    /// whose geometry a caller must not give (module docs of
    /// [`decide_feet`]) fails its pass.
    pub fn fill<G, P>(
        &mut self,
        wanted: impl IntoIterator<Item = RayKey>,
        geom: &G,
        pass: &mut P,
    ) -> Result<(), ShadowError<P::Error>>
    where
        G: FeetGeometry + Sync,
        P: Pass,
    {
        self.feet
            .fill(wanted, |keys| {
                pass.run(keys, &|k: &Fresh<RayKey>| decide(k, geom))
            })
            .map_err(|e| shadow_error(e, RecordKey::Ray))
    }

    /// Point `q`'s feet on edge `g` of the other operand; `None` if they
    /// aren't decided.
    pub fn get(&self, q: Point, g: EdgeId) -> Option<&Feet> {
        self.feet.get(&RayKey { q, g })
    }

    /// The table, for its reads and keys.
    pub fn table(&self) -> &Table<RayKey, Feet> {
        &self.feet
    }

    /// Decides every stored record again, on `geom`, and gives the key of
    /// the first whose answer differs, in key order: a check that the
    /// answers are functions of their keys.
    pub fn replay<G: FeetGeometry>(&self, geom: &G) -> Result<(), RecordKey> {
        replay(&self.feet, |k| decide(k, geom), RecordKey::Ray)
    }
}

/// Decides the feet of a ray record's key.
fn decide<G: FeetGeometry>(k: &Fresh<RayKey>, geom: &G) -> Decided<Feet> {
    let key = *k.key();
    let op = key.q.operand.other();
    decide_feet(
        key,
        geom.point(key.q).at,
        geom.curve(op, key.g),
        geom.near_radius_of(op, key.g),
    )
}

/// What happens to the set along a span, in increasing parameter.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ev {
    /// A foot starts within the bounds.
    Enter([f64; 2], EndKind),
    /// The open foot ends within the bounds.
    Leave([f64; 2], EndKind),
    /// The open foot takes a cluster inside it whole.
    Hull,
}

type Events = SmallVec<[Ev; 4]>;

/// The feet of the point `q` on `curve` at the near radius `r` (finite,
/// `≥ 0`), the record `key`'s (module docs). Fails [`Fail::Internal`],
/// naming the ray record, on a radius or a value that isn't finite or a
/// weight that isn't positive, on spans that don't join bit for bit or a
/// closed chain whose ends aren't one point: a caller's bug no level
/// resolves.
pub fn decide_feet(key: RayKey, q: DVec3, curve: EdgeCurve<'_>, r: f64) -> Decided<Feet> {
    let internal = Fail::Internal {
        key: TieKey {
            row: Row::VertexCurveShadow,
            record: RecordKey::Ray(key),
        },
    };
    let mut cost = Cost::default();
    let line;
    let (pieces, closed): (SmallVec<[&[[f64; 4]]; 4]>, bool) = match curve {
        EdgeCurve::Line([a, b]) => {
            line = [[a.x, a.y, a.z, 1.0], [b.x, b.y, b.z, 1.0]];
            (SmallVec::from_slice(&[&line[..]]), false)
        }
        EdgeCurve::Spans { spans, closed } => (spans.iter().map(|s| &s[..]).collect(), closed),
    };
    let (Some(first), Some(last)) = (pieces.first(), pieces.last()) else {
        return (Err(internal), cost);
    };
    let point = |p: &[f64; 4]| DVec3::new(p[0], p[1], p[2]);
    let joined = pieces
        .windows(2)
        .all(|w| same(point(&w[0][w[0].len() - 1]), point(&w[1][0])));
    let finite = pieces
        .iter()
        .all(|s| s.iter().flatten().all(|x| x.is_finite()));
    let closes = !closed || same(point(&first[0]), point(&last[last.len() - 1]));
    if !(r.is_finite() && r >= 0.0 && q.is_finite() && joined && finite && closes) {
        return (Err(internal), cost);
    }
    let Ok(n) = u32::try_from(pieces.len()) else {
        return (Err(internal), cost);
    };

    // The side of each span end, exactly: the sign of `‖k − q‖² − r²`.
    let rq = Rational::try_from(r).expect("a finite radius");
    let mut sides: SmallVec<[Ordering; 5]> = SmallVec::new();
    let ends = pieces
        .iter()
        .map(|s| s[0])
        .chain(std::iter::once(last[last.len() - 1]));
    for k in ends {
        let (s, w) = exact::value_sign(&NearGap {
            q,
            w: point(&k),
            r: rq.clone(),
        });
        cost.exact = cost.exact.and(w);
        match s {
            Ok(s) => sides.push(s.reverse()),
            Err(_) => return (Err(internal), cost),
        }
    }

    let inside = |s: Ordering| s != Ordering::Greater;
    let mut feet: SmallVec<[Foot; 2]> = SmallVec::new();
    let mut open = inside(sides[0]).then_some((
        FootEnd {
            span: 0,
            t: [0.0, 0.0],
            kind: EndKind::Vertex,
        },
        false,
    ));
    for (at, pts) in pieces.iter().enumerate() {
        let i = u32::try_from(at).expect("fewer spans than n");
        let ends = [sides[at], sides[at + 1]];
        let Some(events) = span_feet(pts, q, r, ends, &mut cost) else {
            return (Err(internal), cost);
        };
        for ev in events {
            match ev {
                Ev::Enter(t, kind) => {
                    if open.is_some() {
                        return (Err(internal), cost);
                    }
                    open = Some((FootEnd { span: i, t, kind }, kind == EndKind::Hull));
                }
                Ev::Leave(t, kind) => {
                    let Some((lo, hull)) = open.take() else {
                        return (Err(internal), cost);
                    };
                    feet.push(Foot {
                        lo,
                        hi: FootEnd { span: i, t, kind },
                        wraps: false,
                        hull: hull || kind == EndKind::Hull,
                    });
                }
                Ev::Hull => match open.as_mut() {
                    Some((_, hull)) => *hull = true,
                    None => return (Err(internal), cost),
                },
            }
        }
        // The span ends where its end's exact side puts it.
        if open.is_some() != inside(ends[1]) {
            return (Err(internal), cost);
        }
    }
    if let Some((lo, hull)) = open {
        feet.push(Foot {
            lo,
            hi: FootEnd {
                span: n - 1,
                t: [1.0, 1.0],
                kind: EndKind::Vertex,
            },
            wraps: false,
            hull,
        });
    }
    // A closed edge's vertex is one point: the feet holding it at the
    // chain's start and end are one.
    if closed
        && feet.len() >= 2
        && feet[0].lo.kind == EndKind::Vertex
        && feet[feet.len() - 1].hi.kind == EndKind::Vertex
    {
        let end = feet.pop().expect("two feet");
        let start = &mut feet[0];
        start.lo = end.lo;
        start.wraps = true;
        start.hull |= end.hull;
    }
    (Ok(Feet { feet }), cost)
}

/// Two points the same `f64`s, bit for bit.
fn same(a: DVec3, b: DVec3) -> bool {
    a.to_array()
        .iter()
        .zip(b.to_array())
        .all(|(x, y)| x.to_bits() == y.to_bits())
}

/// A span's events, at each level in turn until one settles every
/// cluster (the last always does); `None` for a span the levels can't
/// form (a weight not certified positive).
fn span_feet(
    pts: &[[f64; 4]],
    q: DVec3,
    r: f64,
    ends: [Ordering; 2],
    cost: &mut Cost,
) -> Option<Events> {
    let mut at = SpanAt {
        pts,
        q,
        r,
        ends,
        exact: false,
    };
    let out = ladder(&mut at, Level::L0, &mut cost.levels);
    if at.exact {
        // `EXACT_RUNS` times the cube of the span's degree.
        let n = u32::try_from(pts.len().saturating_sub(1)).unwrap_or(u32::MAX);
        let runs = EXACT_RUNS.saturating_mul(n.saturating_mul(n).saturating_mul(n));
        cost.rationals = cost.rationals.saturating_add(runs);
    }
    match out {
        Step::Done(events) => Some(events),
        _ => None,
    }
}

/// The rational runs an exact stage of a span of degree 1 is charged as
/// ([`exact_events`]: the polynomial formed and made square-free, its
/// roots isolated and bracketed, their sides read); a span of degree `n`
/// is charged `n³` times as many, as measured on a straight span and the
/// 3-4-5 arc (about 35 µs and 290 µs).
const EXACT_RUNS: u32 = 8;

/// A span's events at one level.
struct SpanAt<'a> {
    pts: &'a [[f64; 4]],
    q: DVec3,
    r: f64,
    ends: [Ordering; 2],
    /// Whether the first level ran the exact stage.
    exact: bool,
}

impl AtLevel<LevelWork> for SpanAt<'_> {
    type Out = Step;

    fn at<S: LevelScalar>(&mut self, like: &S, work: &mut LevelWork) -> Step {
        let last = like.level() == Level::TOP;
        let step = span_at(self.pts, self.q, self.r, self.ends, like, work, last);
        if like.level() == Level::L0 && matches!(step, Step::Escalate) {
            // A cluster the first level leaves (a tangency, roots closer
            // than it separates): the exact stage decides the span there.
            self.exact = true;
            if let Some(events) = exact_events(self.pts, self.q, self.r, self.ends) {
                return Step::Done(events);
            }
        }
        step
    }

    fn escalates(out: &Step) -> bool {
        matches!(out, Step::Escalate)
    }
}

/// One level's answer for a span.
enum Step {
    Done(Events),
    /// A cluster this level can't settle.
    Escalate,
    /// The span can't be formed.
    Bad,
}

/// A span's events at the level of `like` (module docs), `ends` the
/// exact sides at its ends; at the `last` level an unsettled cluster is
/// taken inside the set whole.
fn span_at<S: Scalar>(
    pts: &[[f64; 4]],
    q: DVec3,
    r: f64,
    ends: [Ordering; 2],
    like: &S,
    work: &mut LevelWork,
    last: bool,
) -> Step {
    let Some(f) = foot_poly(pts, q, r, like) else {
        return Step::Bad;
    };
    // The polynomial run backwards, `u = 1 − t`, for crossings in the
    // span's second half.
    let fr = f.reversed();
    work.feet = work.feet.saturating_add(1);
    let limits = Limits {
        max_depth: MAX_ROOT_DEPTH,
        max_splits: MAX_SPLITS,
    };
    let found = match isolate(&f, limits) {
        Ok(roots) => {
            work.roots = work.roots.and(roots.work);
            roots.found
        }
        Err(out) => {
            work.roots = work.roots.and(out.work);
            return if last {
                Step::Done(hull([0.0, 1.0], ends[0], ends[1]))
            } else {
                Step::Escalate
            };
        }
    };
    let mut events = Events::new();
    for found in &found {
        let cl = match found {
            Found::Root(root) => {
                let kind = EndKind::Crossing;
                let t = narrow(&f, &fr, root.near, root.dir.reverse(), like, work);
                events.push(if root.dir == Ordering::Greater {
                    Ev::Leave(t, kind)
                } else {
                    Ev::Enter(t, kind)
                });
                continue;
            }
            Found::Cluster(cl) => cl,
        };
        // An uncertain sign is only ever at `t = 0` or `t = 1`, where the
        // end's exact side stands.
        let s = cl.start.or((cl.lo == Dyadic::ZERO).then_some(ends[0]));
        let e = cl.end.or((cl.hi == Dyadic::ONE).then_some(ends[1]));
        let (Some(s), Some(e)) = (s, e) else {
            return Step::Bad;
        };
        let t = [cl.lo.bounds()[0], cl.hi.bounds()[1]];
        let settled: Option<Events> = match (s, e) {
            (Ordering::Equal, Ordering::Equal) => None,
            // Just the root at the end on the sphere; the cluster's other
            // side beside it.
            (Ordering::Equal, x) => (cl.mult <= 1).then(|| {
                if x == Ordering::Greater {
                    smallvec::smallvec![Ev::Leave([0.0, 0.0], EndKind::OnSphere)]
                } else {
                    Events::new()
                }
            }),
            (x, Ordering::Equal) => (cl.mult <= 1).then(|| {
                if x == Ordering::Greater {
                    smallvec::smallvec![Ev::Enter([1.0, 1.0], EndKind::OnSphere)]
                } else {
                    Events::new()
                }
            }),
            // No root, an even count of at most one.
            (x, y) if x == y => (cl.mult <= 1).then(Events::new),
            // One crossing, an odd count of at most two.
            (x, y) => (cl.mult <= 2).then(|| {
                let t = narrow(&f, &fr, t, x, like, work);
                smallvec::smallvec![if y == Ordering::Greater {
                    Ev::Leave(t, EndKind::Crossing)
                } else {
                    Ev::Enter(t, EndKind::Crossing)
                }]
            }),
        };
        match settled {
            Some(ev) => events.extend(ev),
            None if last => events.extend(hull(hull_extent(&f, [cl.lo, cl.hi], work), s, e)),
            None => return Step::Escalate,
        }
    }
    Step::Done(events)
}

/// The events of a cluster on `t` taken inside the set whole, the set's
/// side `s` before it and `e` after.
fn hull(t: [f64; 2], s: Ordering, e: Ordering) -> Events {
    let out = |x: Ordering| x == Ordering::Greater;
    let mut ev = Events::new();
    if out(s) {
        ev.push(Ev::Enter(t, EndKind::Hull));
    }
    if out(e) {
        ev.push(Ev::Leave(t, EndKind::Hull));
    }
    if !out(s) && !out(e) {
        ev.push(Ev::Hull);
    }
    ev
}

/// `[a, b]` holding one crossing of `f`, `f` of sign `start` before it
/// and the other after, narrowed by [`bracket`] to where the level can't
/// tell the sign: in the span's second half on `fr`, `f` run backwards
/// (`u = 1 − t`), where `f64` parameters resolve a crossing near `t = 1`
/// as finely as one near `t = 0`, the bounds mirrored back outwards.
fn narrow<S: Scalar>(
    f: &Poly<S>,
    fr: &Poly<S>,
    t: [f64; 2],
    start: Ordering,
    like: &S,
    work: &mut LevelWork,
) -> [f64; 2] {
    match Sign::of(start) {
        Some(start) if t[0] < t[1] && t[0] >= 0.5 => {
            let u = mirrored(t);
            mirrored(bracket(fr, u, neg(start), like, work))
        }
        Some(start) if t[0] < t[1] => bracket(f, t, start, like, work),
        _ => t,
    }
}

/// The most halvings [`enclose_root`] takes: a root's `f64` bounds are
/// adjacent down to `t ≈ 2⁻⁷⁵`.
const ROOT_HALVINGS: u32 = 128;

/// An exact root's `f64` bounds, its interval halved until they are
/// adjacent `f64`s (or one), at most [`ROOT_HALVINGS`] times.
fn enclose_root(root: &crate::num::poly::Root) -> [f64; 2] {
    let mut r = root.clone();
    for _ in 0..ROOT_HALVINGS {
        let [a, b] = r.enclose();
        if b <= a.next_up() {
            break;
        }
        let [lo, hi] = r.interval();
        let half = (hi - lo) / Rational::from(2u32);
        r.narrow(&half);
    }
    r.enclose()
}

/// A span's events from its foot polynomial in rationals (module docs,
/// "Exactly at the first level"): its distinct roots on `[0, 1]` with
/// their multiplicities, the set's side between them read at a rational
/// point there, each root a crossing, or a touching point taken as a hull
/// foot (from outside a foot of that point, from inside joining the
/// stretches beside it), a root at an end the end's exact side. `None`
/// where the span's values aren't finite.
fn exact_events(pts: &[[f64; 4]], q: DVec3, r: f64, ends: [Ordering; 2]) -> Option<Events> {
    use crate::num::poly;
    let rat = |x: f64| Rational::try_from(x).ok();
    let (zero, one) = (Rational::from(0u32), Rational::from(1u32));
    // The Bernstein basis of the span's degree, power form.
    let n = pts.len().checked_sub(1)?;
    let basis: Vec<poly::Poly> = (0..=n)
        .map(|i| {
            let mut b = vec![zero.clone(); n + 1];
            b[i] = one.clone();
            poly::from_bernstein(&b)
        })
        .collect();
    let q = q.to_array().map(rat);
    let mut w: poly::Poly = Vec::new();
    let mut x: [poly::Poly; 3] = std::array::from_fn(|_| Vec::new());
    for (p, b) in pts.iter().zip(&basis) {
        let wk = rat(p[3])?;
        w = poly::add(&w, &poly::scale(b, &wk));
        for k in 0..3 {
            let d = (rat(p[k])? - q[k].as_ref()?) * &wk;
            x[k] = poly::add(&x[k], &poly::scale(b, &d));
        }
    }
    let rr = rat(r)?;
    let mut f = poly::neg(&poly::scale(&poly::mul(&w, &w), &(&rr * &rr)));
    for xk in &x {
        f = poly::add(&f, &poly::mul(xk, xk));
    }
    let f = poly::trim(f);
    if f.is_empty() {
        // The span on the sphere: one foot of it, the hull's.
        return Some(hull([0.0, 1.0], ends[0], ends[1]));
    }
    let roots = poly::roots(&f, &zero, &one);
    // The side of the set at a rational point strictly between two
    // roots' intervals (or at their common end, not a root).
    let side_between = |a: &Rational, b: &Rational| {
        let m = if a < b {
            (a + b) / Rational::from(2u32)
        } else {
            a.clone()
        };
        poly::sign_at(&f, &m)
    };
    let mut events = Events::new();
    let mut before = ends[0];
    let mut from = zero.clone();
    for (i, (root, _)) in roots.iter().enumerate() {
        let [lo, hi] = root.interval().map(Clone::clone);
        let next = roots
            .get(i + 1)
            .map_or_else(|| one.clone(), |(n, _)| n.interval()[0].clone());
        // The side before the root and after it.
        let at_start = root.rational() == Some(&zero);
        let at_end = root.rational() == Some(&one);
        if !at_start && from < lo {
            before = side_between(&from, &lo);
        }
        let after = if at_end {
            ends[1]
        } else {
            side_between(&hi, &next)
        };
        let t = enclose_root(root);
        let out = |x: Ordering| x == Ordering::Greater;
        if at_start {
            if out(after) {
                events.push(Ev::Leave([0.0, 0.0], EndKind::OnSphere));
            }
        } else if at_end {
            if out(before) {
                events.push(Ev::Enter([1.0, 1.0], EndKind::OnSphere));
            }
        } else {
            match (out(before), out(after)) {
                (true, false) => events.push(Ev::Enter(t, EndKind::Crossing)),
                (false, true) => events.push(Ev::Leave(t, EndKind::Crossing)),
                (true, true) => events.extend(hull(t, before, after)),
                (false, false) => events.extend(hull(t, before, after)),
            }
        }
        before = after;
        from = hi;
    }
    Some(events)
}

/// The part of the cluster `[lo, hi]` of `f` that the level can't
/// certify free of roots: its dyadic intervals, split at ½ from `[0, 1]`
/// as [`isolate`] splits, dropped where every coefficient is certified of
/// one sign, kept whole where none is (splitting doesn't help there) or
/// at [`MAX_ROOT_DEPTH`]; the bounds of what is kept. A root on a split
/// point joins the two intervals beside it into one cluster however wide
/// they are; this narrows it to the root. The cluster itself past
/// [`MAX_SPLITS`] splits.
fn hull_extent<S: Scalar>(f: &Poly<S>, [lo, hi]: [Dyadic; 2], work: &mut LevelWork) -> [f64; 2] {
    let whole = [lo.bounds()[0], hi.bounds()[1]];
    let mut kept: Option<[Dyadic; 2]> = None;
    let mut splits = 0u32;
    let mut stack = vec![(0u32, 0u64, f.clone())];
    while let Some((k, j, p)) = stack.pop() {
        let (Some(a), Some(b)) = (Dyadic::new(j, k), Dyadic::new(j + 1, k)) else {
            return whole;
        };
        if b <= lo || a >= hi {
            continue;
        }
        let inside = lo <= a && b <= hi;
        let signs: SmallVec<[Option<Ordering>; 8]> = p.coefs().iter().map(Scalar::sign).collect();
        work.roots.tests = work.roots.tests.saturating_add(1);
        if inside && signs[0].is_some() && signs.iter().all(|s| *s == signs[0]) {
            continue;
        }
        if inside && (k >= MAX_ROOT_DEPTH || signs.iter().all(Option::is_none)) {
            kept = Some(match kept {
                None => [a, b],
                Some([x, y]) => [x.min(a), y.max(b)],
            });
            continue;
        }
        if splits >= MAX_SPLITS {
            return whole;
        }
        splits += 1;
        work.roots.splits = work.roots.splits.saturating_add(1);
        let [l, r] = p.split_half();
        stack.push((k + 1, 2 * j + 1, r));
        stack.push((k + 1, 2 * j, l));
    }
    kept.map_or(whole, |[a, b]| [a.bounds()[0], b.bounds()[1]])
}

/// `F = ‖P − q·W‖² − r²·W²` of the span with the affine control points
/// and weights `pts` at the level of `like`: its sign the side of the
/// sphere about `q` of radius `r` where `W > 0`. `None` unless the span
/// forms with every weight certified positive.
fn foot_poly<S: Scalar>(pts: &[[f64; 4]], q: DVec3, r: f64, like: &S) -> Option<Poly<S>> {
    let f = |x: f64| S::from_f64(x, like);
    // In the frame centred at `q`: each control point's offset `x − q`
    // is formed first (one rounding at most), then weighted, so the
    // coefficients' error scales with the span's distance from `q`, not
    // with the coordinates' magnitude (`w·x − q·w` would carry `u·|q|`).
    let q = q.to_array();
    let c: HomCurve<S, 3> = HomCurve::new(pts.iter().map(|p| {
        let w = f(p[3]);
        let d: [S; 3] = std::array::from_fn(|k| f(p[k]).sub(&f(q[k])).mul(&w));
        (d, w)
    }))?;
    if !c.weights_positive() {
        return None;
    }
    let mut sum: Option<Poly<S>> = None;
    for d in c.coords() {
        let sq = d.mul(d)?;
        sum = Some(match sum {
            None => sq,
            Some(s) => s.add(&sq),
        });
    }
    let rw = c.weights().scale(&f(r));
    Some(sum?.sub(&rw.mul(&rw)?))
}

#[cfg(test)]
mod tests;
