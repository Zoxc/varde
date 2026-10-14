//! The layers of a face above or below a point: `s02(q, f)`, `q` a point
//! of `A` and `f` a face of `B`, the winding of an upward ray from `q`
//! through `f`'s counting surface (`+1` per up-facing layer above `q`,
//! `−1` per down-facing one), and `s20(q, f)`, `q` of `B` and `f` of `A`,
//! the downward ray's (`+1` per down-facing layer below `q`, `−1` per
//! up-facing one).
//!
//! **Pieces.** The face's tiles are refined into boxes ([`role`]): an
//! outside box is dropped; a box whose shadow is certified off `q_xy` has
//! degree 0 and is dropped too (a strip or corner box with its ribbons:
//! its hull padded by its curves' band, which the ribbons stay within);
//! a thin box always refines. The others refine until **height-clear**,
//! their (padded) hull certified above or below `q` along `UP`, or small:
//! a strip or corner box within its curves' least band across, a core box
//! within `ε_tie` ([`FaceQuery::eps_tie`]). A core leaf is a piece of its
//! own, its degree its box's ([`box_degree_sided`]). The strip and corner
//! leaves form **runs**: the leaves of one strip (one coedge, or a slit's)
//! or one corner, of one height class (above, below, near), joined where
//! they share a side; each run with its curves' ribbons is a piece
//! ([`strip_degree`]), a corner's run with every coedge of the corner. A
//! run the level can't settle (a cut at a box corner or next to the ray,
//! a run the curve doesn't cut) refines its boxes and is built again; a
//! run passing a point twice is taken box by box.
//!
//! **Around a vertex.** A corner's vertex lying on a box side (on a tile
//! line, or a dyadic line the splits couldn't move) is on the outer
//! boundary of any run that doesn't hold every box around it, and no cut
//! there settles. So the corner's leaves holding one of its vertices (in
//! their closed UV box) are one run, whatever their heights: their class
//! is the one class of those not dropped, or, where they differ, near,
//! once each is within the band (a dropped one joins: its degree, ribbons
//! included, is 0). Such a run lies within a band or two of the vertex:
//! where it is near, the vertex is near `q` and names the layer.
//!
//! **Heights.** A height-clear piece is geometric: `s02` sums the degrees
//! of the pieces above `q`, `s20` the negated degrees of those below. The
//! near pieces lie within `q`'s near radius and are found as layers
//! ([`LayerPieces`]): a near piece holds the corner's vertices where it
//! is a corner's run around them, and `q`'s feet on its ribbons' edges
//! whose stretch meets the p-curve spans its boxes meet; near pieces
//! touching in UV are joined; a piece without features is keyed by its
//! first box's hull centre. A layer of near pieces reads its height
//! record ([`height_key`] against the layer, anchored at the face) once:
//! above (below for `s20`), its pieces' degrees count. So an overhanging
//! layer away from `q` (a face overhanging its own edge in the shadow)
//! is height-clear and counts by its own height, and every face at a
//! shared vertex reads the one point–vertex record.
//!
//! **The fold rule.** A layer none of whose pieces has a non-zero degree
//! contributes nothing and reads no record (the column doesn't enter it).
//! A layer whose record is a fold's (its anchor's facing certified at no
//! level, [`HeightRec::fold`]) contributes the fold's upper sheet's
//! facing (`+1` up), for `s02` and `s20` alike: `q` lies between the two
//! sheets, the upper one above it and the lower below.
//!
//! **What the caller gives** ([`FaceQuery`]): the face's boxes' data
//! ([`FaceUv`], its [`Corners`], its tiles' patches at the level worked
//! at), per trim its edge and ribbon ([`TrimEdge`]: the edge's curve, the
//! p-spans' places on it, the query point's ray record against it,
//! [`line_chain`] for a straight edge), the query point and `ε_tie`. The
//! counting asks for records it reads and doesn't find
//! ([`CountError::Need`], [`CountError::NeedHeight`]): fill them and ask
//! again.
//!
//! **Termination** ([`count_pieces`]). Refinement is in space first: a
//! box splits at most [`MAX_SUBDIV_DEPTH`] times from its tile, and a
//! piece the level doesn't settle by then (or at the boxes' own depth
//! limit) leaves the level unsettled. Then in precision: the whole count
//! runs again at the next level of the schedule ([`LEVELS`]: `f64`,
//! double-double, the big-float widths), the patches lifted exactly. A
//! count still unsettled at the last level is
//! [`CountError::OutOfBudget`] with [`Exhausted::Precision`], keyed by
//! the point and the face, never a loop. Every piece tested is charged,
//! at its level's units ([`Charge`]), against the share of the budget the
//! count is given: running past it stops the count at once,
//! [`Exhausted::Work`] at the first level and [`Exhausted::Precision`]
//! above it. [`MAX_LEAVES`] and [`MAX_ROUNDS`] are fixed limits,
//! [`Exhausted::SizeLimit`].

use std::cmp::Ordering;
use std::collections::BTreeMap;

use malachite::Rational;
use smallvec::SmallVec;

use super::boxes::{
    BoxError, BoxKey, Corners, FaceUv, QShadow, Role, SpanRef, box_hull, dy, role, split,
    sub_patch, within_band,
};
use super::degree::{SidedError, box_degree_sided};
use super::pieces::{PieceError, Ribbon, strip_degree};
use crate::bezier::{Dir, HomPatch, Scalar};
use crate::ids::{CoedgeId, EdgeId, FaceId, VertId};
use crate::num::bounded::{Bounded, add_up, mul_up, up};
use crate::num::decide::Sign;
use crate::order::Pos;
use crate::perturb::{NU, R_HAT, UP};
use crate::records::feet::Foot;
use crate::records::height::{
    Feature, HeightReads, Heights, KeyError, LayerPieces, LayerTable, Layers, height_key,
};
use crate::records::keys::{Anchor, HeightKey, LayerId, Operand, Point, SideKey, Target};
use crate::records::ray::RayLevel;
use crate::records::ray::{ChainCrossing, ChainRec, Crossing};
use crate::records::shadow::{
    AtLevel, Cost, LEVELS, LevelWork, MAX_SUBDIV_DEPTH, RayRec, SCHEDULE_UNITS, ladder,
};
use crate::records::side::Sides;

#[cfg(doc)]
use crate::records::height::HeightRec;

/// The most leaves one face's count keeps.
pub const MAX_LEAVES: usize = 1 << 16;

/// The most times the runs are built again after refining some.
pub const MAX_ROUNDS: u32 = 64;

/// What a count may spend (module docs): `share` units, a piece tested
/// at level `k` costing `units[k]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Charge {
    /// The units the count may spend, its share of the budget left.
    pub share: u64,
    /// The units of one piece tested, per precision level.
    pub units: [u64; LEVELS],
}

impl Charge {
    /// A share of `share` units at the schedule's units
    /// ([`SCHEDULE_UNITS`]).
    pub fn new(share: u64) -> Charge {
        Charge {
            share,
            units: SCHEDULE_UNITS,
        }
    }

    /// The units of `cost`'s pieces.
    pub fn spent(&self, cost: &Cost) -> u64 {
        cost.levels.iter().zip(self.units).fold(0u64, |s, (l, u)| {
            s.saturating_add(u64::from(l.evals).saturating_mul(u))
        })
    }
}

/// What a count ran out of ([`CountError::OutOfBudget`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Exhausted {
    /// The share, at the first level.
    Work,
    /// The share at a level above the first, or the last level reached
    /// with a piece unsettled.
    Precision,
    /// [`MAX_LEAVES`] or [`MAX_ROUNDS`].
    SizeLimit,
}

/// One trim of the face as the counting reads it: its edge and its
/// ribbon.
#[derive(Clone, Copy, Debug)]
pub struct TrimEdge<'a> {
    /// The trim's edge.
    pub edge: EdgeId,
    /// Its ribbon ([`Ribbon::coedge`] the trim's coedge).
    pub ribbon: Ribbon<'a>,
}

/// A face and a point to count it against (module docs).
#[derive(Clone, Copy, Debug)]
pub struct FaceQuery<'a, 'f, S> {
    /// The face.
    pub face: &'a FaceUv<'f>,
    /// Its corners, built once per operation.
    pub corners: &'a Corners,
    /// Its tiles' patches at the level worked at, one per tile in
    /// [`FaceUv::tile_boxes`]' order.
    pub patches: &'a [HomPatch<S>],
    /// Per trim of positive length (and every zero-length trim with
    /// spans), its edge and ribbon, in any order.
    pub edges: &'a [TrimEdge<'a>],
    /// The query point, of the other operand.
    pub point: Point,
    /// Its stored position.
    pub at: [f64; 3],
    /// The size a core box near `q` refines to, finite and positive.
    pub eps_tie: f64,
}

/// Why a face's count isn't certified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CountError {
    /// A box or the face is refused, or the query point is of the face's
    /// operand, or its position isn't finite, or `ε_tie` isn't.
    Box(BoxError),
    /// A piece reads this side record, not yet decided: fill it and ask
    /// again.
    Need(SideKey),
    /// A layer reads this height record, not yet decided: fill it (with
    /// every anchor of its key) and ask again.
    NeedHeight(HeightKey),
    /// A record a layer's key reads is missing, or the layers aren't
    /// stored ([`FacePieces::layers`]).
    Key(KeyError),
    /// A strip or corner piece is refused (a ribbon that doesn't fit, a
    /// corner missing a coedge's ribbon).
    Piece(PieceError),
    /// A trim of the face has no [`TrimEdge`].
    Edge { coedge: CoedgeId },
    /// A box side inside a tile isn't certified off `q_xy` and no split
    /// moves it: no record counts it.
    Through,
    /// The level doesn't settle a piece down to the deepest boxes
    /// ([`MAX_SUBDIV_DEPTH`]): escalate.
    Unsettled,
    /// The count ran out (module docs), keyed by its point and face.
    OutOfBudget {
        /// What ran out.
        cause: Exhausted,
        /// The query point.
        point: Point,
        /// The face.
        face: FaceId,
    },
}

impl From<BoxError> for CountError {
    fn from(e: BoxError) -> CountError {
        CountError::Box(e)
    }
}

impl From<KeyError> for CountError {
    fn from(e: KeyError) -> CountError {
        CountError::Key(e)
    }
}

/// A piece's place against `q`'s height.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Height {
    Above,
    Below,
    Near,
}

/// What a leaf is part of: a core box alone, or a strip's or corner's
/// runs.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Owner {
    Core,
    Strip(SmallVec<[CoedgeId; 4]>),
    Corner(VertId),
}

#[derive(Clone, Debug)]
struct Leaf {
    b: BoxKey,
    owner: Owner,
    /// `None`: dropped (shadow off `q_xy`).
    height: Option<Height>,
    /// Within its stopping size.
    small: bool,
    met: SmallVec<[SpanRef; 8]>,
    /// The splits from its tile.
    splits: u32,
}

/// A near piece: its degree and what names its layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NearPiece {
    /// Its degree.
    pub degree: i64,
    /// Its features: the corner's vertices, `q`'s feet on its edges.
    pub features: SmallVec<[Feature; 2]>,
    /// Its geometric key, for a layer without features: its first box's
    /// hull centre.
    pub core: Option<Pos>,
    boxes: Vec<BoxKey>,
}

/// A face's pieces against a point (module docs): the height-clear
/// pieces' sums and the near pieces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FacePieces {
    /// The query point.
    pub point: Point,
    /// The face (of the other operand).
    pub face: FaceId,
    /// The degrees of the pieces above `q`, summed.
    pub above: i64,
    /// The degrees of the pieces below `q`, summed.
    pub below: i64,
    /// The near pieces.
    pub near: Vec<NearPiece>,
    joins: Vec<(usize, usize)>,
    /// The pieces tested (boxes, span pieces, sides).
    pub pieces: u64,
}

/// The shadow winding of the face's counting surface about `q`, from its
/// ray records: `Σ_c ±RayRec(q, g_c)`, a coedge against its edge negated.
/// [`FacePieces::degree`] must equal it.
pub fn ray_degree(face: &FaceUv<'_>, edges: &[TrimEdge<'_>]) -> Result<i64, CountError> {
    let mut d = 0i64;
    for t in face.trims() {
        if t.spans.is_empty() {
            continue;
        }
        let e = find_edge(edges, t.coedge)?;
        let c = i64::from(e.ribbon.rays.count());
        d += if t.reversed { -c } else { c };
    }
    Ok(d)
}

/// The ray record of a straight edge `a → b` (its stored direction) as a
/// one-span chain ([`ChainRec`]), from the flat record `rec` of the query
/// point at `q`: its crossing, if any, at the parameter where the edge's
/// shadow meets `q`'s `r̂` line, enclosed (`[0, 1]` where the edge's
/// shadow runs along the line).
pub fn line_chain(rec: &RayRec, a: [f64; 3], b: [f64; 3], q: [f64; 3]) -> ChainRec {
    let mut out = ChainRec::default();
    let Some(sign) = rec.crossing else {
        return out;
    };
    let nu = [NU.x, NU.y, NU.z];
    let dot = |p: [f64; 3], o: [f64; 3]| {
        (0..3).fold(Bounded::exact(0.0), |s, k| {
            s + Bounded::exact(nu[k]) * (Bounded::exact(p[k]) - Bounded::exact(o[k]))
        })
    };
    let (num, den) = (dot(q, a), dot(b, a));
    let [dl, dh] = den.enclose();
    let t = if dl > 0.0 || dh < 0.0 {
        let [lo, hi] = (num / den).enclose();
        // Clamped to `[0, 1]`, a NaN widened to it, `−0` read as `+0`.
        let lo = if lo.is_nan() || lo <= 0.0 {
            0.0
        } else if lo > 1.0 {
            1.0
        } else {
            lo
        };
        let hi = if hi.is_nan() || hi >= 1.0 {
            1.0
        } else if hi < lo {
            lo
        } else {
            hi + 0.0
        };
        [lo, hi]
    } else {
        [0.0, 1.0]
    };
    out.crossings.push(ChainCrossing {
        span: 0,
        crossing: Crossing { t, sign },
    });
    out
}

fn find_edge<'e, 'a>(
    edges: &'e [TrimEdge<'a>],
    c: CoedgeId,
) -> Result<&'e TrimEdge<'a>, CountError> {
    edges
        .iter()
        .find(|e| e.ribbon.coedge == c)
        .ok_or(CountError::Edge { coedge: c })
}

/// Which side of `[lo, hi] + pad` the patch's values of `a·x` lie on,
/// certified: `Greater` beyond `hi + pad`, `Less` below `lo − pad`.
fn lin_side<S: Scalar>(
    p: &HomPatch<S>,
    a: [f64; 3],
    [lo, hi]: [f64; 2],
    pad: f64,
) -> Option<Ordering> {
    if !p.weights_positive() {
        return None;
    }
    let like = p.weights().coefs()[0].clone();
    let norm = up(up(a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt());
    let pad = mul_up(pad, norm);
    let av = a.map(|x| S::from_f64(x, &like));
    // `a·x − (hi + pad) > 0`, `a·x − (lo − pad) < 0`.
    let c_hi = -add_up(hi, pad);
    let c_lo = add_up(-lo, pad);
    if !(c_hi.is_finite() && c_lo.is_finite()) {
        return None;
    }
    if p.compose_linear(&av, &S::from_f64(c_hi, &like)).hull().lo > 0.0 {
        return Some(Ordering::Greater);
    }
    if p.compose_linear(&av, &S::from_f64(c_lo, &like)).hull().hi < 0.0 {
        return Some(Ordering::Less);
    }
    None
}

/// The certified enclosure of `a·q`.
fn enclose_dot(a: [f64; 3], q: [f64; 3]) -> [f64; 2] {
    (0..3)
        .fold(Bounded::exact(0.0), |s, k| {
            s + Bounded::exact(a[k]) * Bounded::exact(q[k])
        })
        .enclose()
}

/// A box's closed rectangle in global UV, exactly.
fn rect(b: &BoxKey) -> [[Rational; 2]; 2] {
    let at = |i: i64, t: crate::bezier::Dyadic| {
        Rational::from(i) + Rational::try_from(dy(t)).expect("a dyadic")
    };
    [Dir::U, Dir::V].map(|d| {
        let k = usize::from(d == Dir::V);
        let e = b.extent(d);
        [at(b.tile()[k], e.lo()), at(b.tile()[k], e.hi())]
    })
}

/// Whether two boxes share a stretch of a side.
fn share_side(a: &[[Rational; 2]; 2], b: &[[Rational; 2]; 2]) -> bool {
    (0..2).any(|k| {
        let o = 1 - k;
        (a[k][1] == b[k][0] || b[k][1] == a[k][0]) && a[o][0] < b[o][1] && b[o][0] < a[o][1]
    })
}

/// Whether two closed boxes meet.
fn touch(a: &[[Rational; 2]; 2], b: &[[Rational; 2]; 2]) -> bool {
    (0..2).all(|k| a[k][0] <= b[k][1] && b[k][0] <= a[k][1])
}

fn holds(r: &[[Rational; 2]; 2], p: &[Rational; 2]) -> bool {
    (0..2).all(|k| r[k][0] <= p[k] && p[k] <= r[k][1])
}

fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// The connected parts of `items` under `linked`, each in the items'
/// order, ordered by their first item.
fn components(n: usize, linked: impl Fn(usize, usize) -> bool) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..n).collect();
    for i in 0..n {
        for j in 0..i {
            if linked(i, j) {
                let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                let (lo, hi) = if a < b { (a, b) } else { (b, a) };
                parent[hi] = lo;
            }
        }
    }
    let mut out: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..n {
        let r = root(&mut parent, i);
        out.entry(r).or_default().push(i);
    }
    out.into_values().collect()
}

struct Ctx<'q, 'a, 'f, S, R> {
    fq: &'q FaceQuery<'a, 'f, S>,
    sides: &'q Sides,
    reads: &'q R,
    q: QShadow,
    up_q: [f64; 2],
    like: S,
    pieces: u64,
    /// The most pieces the count may test.
    limit: u64,
}

impl<'a, S: Scalar, R: HeightReads> Ctx<'_, 'a, '_, S, R> {
    fn patch(&self, b: &BoxKey) -> Result<&'a HomPatch<S>, CountError> {
        self.fq
            .face
            .patch_of(self.fq.patches, b.tile())
            .ok_or(CountError::Box(BoxError::Patches))
    }

    /// Counts `n` pieces tested, failing past the limit.
    fn charge(&mut self, n: u64) -> Result<(), CountError> {
        self.pieces = self.pieces.saturating_add(n);
        if self.pieces > self.limit {
            return Err(self.out(Exhausted::Work));
        }
        Ok(())
    }

    fn out(&self, cause: Exhausted) -> CountError {
        CountError::OutOfBudget {
            cause,
            point: self.fq.point,
            face: self.fq.face.tile_boxes().next().expect("tiles").face(),
        }
    }

    /// The split of `b`, `splits` splits from its tile: the level is
    /// unsettled past [`MAX_SUBDIV_DEPTH`] or the boxes' depth.
    fn split(&mut self, b: &BoxKey, splits: u32) -> Result<[BoxKey; 2], CountError> {
        if splits >= MAX_SUBDIV_DEPTH {
            return Err(CountError::Unsettled);
        }
        let patch = self.patch(b)?;
        let s = split(self.fq.face, b, patch, Some(&self.q)).map_err(|e| match e {
            BoxError::Depth => CountError::Unsettled,
            e => CountError::Box(e),
        })?;
        self.charge(s.pieces)?;
        Ok(s.halves)
    }

    fn band(&self, coedges: &[CoedgeId]) -> f64 {
        let trims = self.fq.face.trims();
        coedges
            .iter()
            .filter_map(|&c| trims.iter().find(|t| t.coedge == c))
            .map(|t| t.band)
            .fold(f64::INFINITY, |m, b| if b < m { b } else { m })
    }

    /// Refines `b` (with the spans `among` its parent met) into leaves.
    fn refine(
        &mut self,
        b: BoxKey,
        among: Option<SmallVec<[SpanRef; 8]>>,
        splits: u32,
        out: &mut Vec<Leaf>,
    ) -> Result<(), CountError> {
        let mut stack = vec![(b, among, splits)];
        while let Some((b, among, splits)) = stack.pop() {
            if out.len() >= MAX_LEAVES {
                return Err(self.out(Exhausted::SizeLimit));
            }
            let r = role(
                self.fq.face,
                self.fq.corners,
                &b,
                &self.like,
                among.as_deref(),
            )?;
            self.charge(r.pieces)?;
            let (owner, pad, thin) = match &r.role {
                Role::Outside => continue,
                Role::Core => (Owner::Core, 0.0, false),
                Role::Strip(c) => (Owner::Strip(c.clone()), self.band(c), false),
                Role::Corner { vertex, met } => (Owner::Corner(*vertex), self.band(met), false),
                // A thin box refines: its owner is never read.
                Role::Thin(c) => (Owner::Core, self.band(c), true),
            };
            if !pad.is_finite() {
                return Err(CountError::Box(BoxError::Foreign));
            }
            let patch = self.patch(&b)?;
            let sub = sub_patch(patch, &b);
            self.charge(1)?;
            let r_hat = [R_HAT.x, R_HAT.y, R_HAT.z];
            let nu = [NU.x, NU.y, NU.z];
            let off = lin_side(&sub, r_hat, self.q.r, pad).is_some()
                || lin_side(&sub, nu, self.q.n, pad).is_some();
            if off && !thin {
                if owner != Owner::Core {
                    out.push(Leaf {
                        b,
                        owner,
                        height: None,
                        small: false,
                        met: r.met,
                        splits,
                    });
                }
                continue;
            }
            if off {
                // A thin box off the query: degree 0, ribbons included.
                continue;
            }
            let h = match lin_side(&sub, [UP.x, UP.y, UP.z], self.up_q, pad) {
                Some(Ordering::Greater) => Some(Height::Above),
                Some(Ordering::Less) => Some(Height::Below),
                _ => None,
            };
            let size = if owner == Owner::Core {
                self.fq.eps_tie
            } else {
                pad
            };
            let small = box_hull(patch, &b).is_some_and(|hb| within_band(&hb, size));
            if !thin && (h.is_some() || small) {
                out.push(Leaf {
                    b,
                    owner,
                    height: Some(h.unwrap_or(Height::Near)),
                    small,
                    met: r.met,
                    splits,
                });
                continue;
            }
            let [lo, hi] = self.split(&b, splits)?;
            stack.push((hi, Some(r.met.clone()), splits + 1));
            stack.push((lo, Some(r.met), splits + 1));
        }
        Ok(())
    }

    /// Splits leaf `k` in place: removes it and refines its halves.
    fn resplit(&mut self, leaves: &mut Vec<Leaf>, ks: &[usize]) -> Result<(), CountError> {
        let mut ks = ks.to_vec();
        ks.sort_unstable();
        ks.dedup();
        let mut taken = Vec::with_capacity(ks.len());
        for &k in ks.iter().rev() {
            taken.push(leaves.remove(k));
        }
        for l in taken.into_iter().rev() {
            for h in self.split(&l.b, l.splits)? {
                self.refine(h, Some(l.met.clone()), l.splits + 1, leaves)?;
            }
        }
        Ok(())
    }

    /// The UV points of the corner named `v`'s vertices.
    fn corner_points(&self, v: VertId) -> Vec<[Rational; 2]> {
        let Some(c) = self.fq.corners.corner(v) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for t in self.fq.face.trims() {
            if t.spans.is_empty() {
                continue;
            }
            let last = t.spans.len() - 1;
            for (k, p) in [(0, t.spans[0][0]), (1, t.spans[last][2])] {
                if c.vertices.contains(&t.ends[k]) {
                    let r = [p[0], p[1]].map(|x| Rational::try_from(x).expect("finite"));
                    if !out.contains(&r) {
                        out.push(r);
                    }
                }
            }
        }
        out
    }

    /// The ribbons of an owner's runs.
    fn ribbons(&self, owner: &Owner) -> Result<Vec<Ribbon<'_>>, CountError> {
        let coedges: SmallVec<[CoedgeId; 8]> = match owner {
            Owner::Core => SmallVec::new(),
            Owner::Strip(c) => c.iter().copied().collect(),
            Owner::Corner(v) => {
                let c = self
                    .fq
                    .corners
                    .corner(*v)
                    .ok_or(CountError::Box(BoxError::Foreign))?;
                let mut all: SmallVec<[CoedgeId; 8]> = c.curves.iter().copied().collect();
                for &z in &c.zero {
                    let has_spans = self
                        .fq
                        .face
                        .trims()
                        .iter()
                        .any(|t| t.coedge == z && !t.spans.is_empty());
                    if has_spans {
                        all.push(z);
                    }
                }
                all.sort_unstable();
                all
            }
        };
        coedges
            .iter()
            .map(|&c| find_edge(self.fq.edges, c).map(|e| e.ribbon))
            .collect()
    }

    /// `q`'s feet on the run's edges whose stretch meets the p-spans its
    /// boxes meet.
    fn feet(&self, owner: &Owner, run: &[&Leaf]) -> Result<SmallVec<[Feature; 2]>, CountError> {
        let mut out: SmallVec<[Feature; 2]> = SmallVec::new();
        let ribbons = self.ribbons(owner)?;
        for r in &ribbons {
            let e = find_edge(self.fq.edges, r.coedge)?;
            let spans: SmallVec<[u32; 8]> = run
                .iter()
                .flat_map(|l| l.met.iter())
                .filter(|s| s.coedge == r.coedge)
                .map(|s| s.span)
                .collect();
            if spans.is_empty() {
                continue;
            }
            let feet = self
                .reads
                .feet(self.fq.point, e.edge)
                .ok_or(CountError::Key(KeyError::Missing(
                    crate::records::keys::RecordKey::Ray(crate::records::keys::RayKey {
                        q: self.fq.point,
                        g: e.edge,
                    }),
                )))?;
            for (k, foot) in feet.feet.iter().enumerate() {
                let meets = spans.iter().any(|&s| {
                    r.map
                        .get(s as usize)
                        .is_some_and(|m| foot_meets(foot, m.span, m.t))
                });
                if meets {
                    let id = crate::records::keys::FootId(u32::try_from(k).expect("few feet"));
                    let x = Feature::Foot(e.edge, id);
                    if !out.contains(&x) {
                        out.push(x);
                    }
                }
            }
        }
        Ok(out)
    }
}

/// Whether foot `f` meets the stretch `[t0, t1]` of span `span`.
fn foot_meets(f: &Foot, span: u32, [t0, t1]: [f64; 2]) -> bool {
    let lo = (f.lo.span, f.lo.t[0]);
    let hi = (f.hi.span, f.hi.t[1]);
    let after_lo = |p: (u32, f64)| (p.0, p.1) >= lo;
    let before_hi = |p: (u32, f64)| (p.0, p.1) <= hi;
    let (a, b) = ((span, t0), (span, t1));
    if f.wraps {
        // From `lo` to the chain's end and from its start to `hi`.
        before_hi(a) || after_lo(b)
    } else {
        // The stretch `[a, b]` meets `[lo, hi]`.
        before_hi(a) && after_lo(b)
    }
}

/// The pieces of `fq.face` against `fq.point` (module docs) at the level
/// of `fq.patches`, reading the side records `sides` and, for the near
/// pieces' features, `reads` (the point–vertex records and the feet).
/// Not charged: [`count_pieces`] charges and escalates.
pub fn face_pieces<S: Scalar, R: HeightReads>(
    fq: &FaceQuery<'_, '_, S>,
    sides: &Sides,
    reads: &R,
) -> Result<FacePieces, CountError> {
    let mut pieces = 0;
    pieces_within(fq, sides, reads, u64::MAX, &mut pieces)
}

/// The pieces of `fq.face` against `fq.point` (module docs) at the
/// first level, then at each next one while the level is unsettled,
/// each level's patches lifted exactly from `fq.patches`;
/// every piece tested charged against `charge`. With the pieces, or the
/// error, the work per level ([`LevelWork::evals`]) for the pass to
/// charge. Unsettled at the last level, or past the share:
/// [`CountError::OutOfBudget`] (module docs).
pub fn count_pieces<R: HeightReads>(
    fq: &FaceQuery<'_, '_, Bounded<f64>>,
    sides: &Sides,
    reads: &R,
    charge: &Charge,
) -> (Result<FacePieces, CountError>, Cost) {
    let mut cost = Cost::default();
    let mut at = CountAt {
        fq,
        sides,
        reads,
        charge,
        spent: 0,
    };
    let r = ladder(&mut at, 0, &mut cost).expect("the first level runs");
    let r = match r {
        Err(CountError::Unsettled) => Err(CountError::OutOfBudget {
            cause: Exhausted::Precision,
            point: fq.point,
            face: fq.face.tile_boxes().next().expect("tiles").face(),
        }),
        r => r,
    };
    (r, cost)
}

/// The count at one level ([`count_pieces`]).
struct CountAt<'c, 'q, 'a, 'f, R> {
    fq: &'c FaceQuery<'a, 'f, Bounded<f64>>,
    sides: &'q Sides,
    reads: &'q R,
    charge: &'c Charge,
    /// The units the levels before spent.
    spent: u64,
}

impl<R: HeightReads> AtLevel for CountAt<'_, '_, '_, '_, R> {
    type Out = Result<FacePieces, CountError>;

    fn at<S: RayLevel>(&mut self, like: &S, level: usize, work: &mut LevelWork) -> Self::Out {
        let unit = self.charge.units[level];
        // A level whose pieces cost nothing has no limit.
        let limit = match unit {
            0 => u64::MAX,
            u => self.charge.share.saturating_sub(self.spent) / u,
        };
        let patches: Vec<HomPatch<S>> = self.fq.patches.iter().map(|p| lift(p, like)).collect();
        let f = self.fq;
        let fq = FaceQuery {
            face: f.face,
            corners: f.corners,
            patches: &patches,
            edges: f.edges,
            point: f.point,
            at: f.at,
            eps_tie: f.eps_tie,
        };
        let mut pieces = 0;
        let r = pieces_within(&fq, self.sides, self.reads, limit, &mut pieces);
        work.evals = work
            .evals
            .saturating_add(u32::try_from(pieces).unwrap_or(u32::MAX));
        self.spent = self.spent.saturating_add(pieces.saturating_mul(unit));
        match r {
            Err(CountError::OutOfBudget {
                cause: Exhausted::Work,
                point,
                face,
            }) if level > 0 => Err(CountError::OutOfBudget {
                cause: Exhausted::Precision,
                point,
                face,
            }),
            r => r,
        }
    }

    fn escalates(out: &Self::Out) -> bool {
        matches!(out, Err(CountError::Unsettled))
    }
}

/// `p` at the level of `like`: each value exactly, its error kept.
fn lift<S: RayLevel>(p: &HomPatch<Bounded<f64>>, like: &S) -> HomPatch<S> {
    let [x, y, z, w] = p.tensors().each_ref().map(|t| t.coefs());
    let (m, n) = p.degrees();
    let at = |b: &Bounded<f64>| {
        let v = S::from_f64(b.v, like);
        let e = b.err();
        if e == 0.0 { v } else { v.widened(e) }
    };
    HomPatch::new(
        m,
        n,
        (0..x.len()).map(|k| [at(&x[k]), at(&y[k]), at(&z[k]), at(&w[k])]),
    )
    .expect("the same degrees")
}

/// [`face_pieces`], testing at most `limit` pieces; `pieces` the pieces
/// tested, also on failure.
fn pieces_within<S: Scalar, R: HeightReads>(
    fq: &FaceQuery<'_, '_, S>,
    sides: &Sides,
    reads: &R,
    limit: u64,
    pieces: &mut u64,
) -> Result<FacePieces, CountError> {
    let face = fq.face;
    let first = face
        .tile_boxes()
        .next()
        .ok_or(CountError::Box(BoxError::Grid))?;
    if fq.point.operand == first.operand() {
        return Err(CountError::Box(BoxError::Foreign));
    }
    if !(fq.eps_tie.is_finite() && fq.eps_tie > 0.0) {
        return Err(CountError::Box(BoxError::Grid));
    }
    let q = QShadow::of(fq.at).ok_or(CountError::Box(BoxError::Grid))?;
    let up_q = enclose_dot([UP.x, UP.y, UP.z], fq.at);
    if !(up_q[0].is_finite() && up_q[1].is_finite()) {
        return Err(CountError::Box(BoxError::Grid));
    }
    let like = fq
        .patches
        .first()
        .ok_or(CountError::Box(BoxError::Patches))?
        .weights()
        .coefs()[0]
        .clone();
    let mut ctx = Ctx {
        fq,
        sides,
        reads,
        q,
        up_q,
        like,
        pieces: 0,
        limit,
    };
    let r = rounds(&mut ctx);
    *pieces = ctx.pieces;
    r
}

/// The leaves refined, then the rounds until the pieces settle.
fn rounds<S: Scalar, R: HeightReads>(
    ctx: &mut Ctx<'_, '_, '_, S, R>,
) -> Result<FacePieces, CountError> {
    let mut leaves: Vec<Leaf> = Vec::new();
    for b in ctx.fq.face.tile_boxes() {
        ctx.refine(b, None, 0, &mut leaves)?;
    }

    let mut cache: BTreeMap<(Owner, Vec<BoxKey>), Result<i64, PieceError>> = BTreeMap::new();
    for _ in 0..MAX_ROUNDS {
        leaves.sort_by_key(|l| l.b);
        match round(ctx, &mut leaves, &mut cache)? {
            Some(out) => return Ok(out),
            None => continue,
        }
    }
    Err(ctx.out(Exhausted::SizeLimit))
}

/// One round over the leaves: the pieces, or `None` where some leaves
/// were refined and the runs must be built again.
fn round<S: Scalar, R: HeightReads>(
    ctx: &mut Ctx<'_, '_, '_, S, R>,
    leaves: &mut Vec<Leaf>,
    cache: &mut BTreeMap<(Owner, Vec<BoxKey>), Result<i64, PieceError>>,
) -> Result<Option<FacePieces>, CountError> {
    let fq = ctx.fq;
    let rects: Vec<_> = leaves.iter().map(|l| rect(&l.b)).collect();

    // Around a corner's vertices: one class.
    let mut forced: BTreeMap<usize, Option<Height>> = BTreeMap::new();
    let mut corners: Vec<VertId> = leaves
        .iter()
        .filter_map(|l| match l.owner {
            Owner::Corner(v) => Some(v),
            _ => None,
        })
        .collect();
    corners.sort_unstable();
    corners.dedup();
    let mut refine: Vec<usize> = Vec::new();
    for v in corners {
        let pts = ctx.corner_points(v);
        let around: Vec<usize> = (0..leaves.len())
            .filter(|&k| {
                leaves[k].owner == Owner::Corner(v) && pts.iter().any(|p| holds(&rects[k], p))
            })
            .collect();
        let mut classes: Vec<Height> = around.iter().filter_map(|&k| leaves[k].height).collect();
        classes.sort_unstable();
        classes.dedup();
        let class = match classes.as_slice() {
            [] => None,
            [h] => Some(*h),
            _ => {
                let big: Vec<usize> = around
                    .iter()
                    .copied()
                    .filter(|&k| !leaves[k].small)
                    .collect();
                refine.extend(big);
                Some(Height::Near)
            }
        };
        for k in around {
            forced.insert(k, class);
        }
    }
    if !refine.is_empty() {
        ctx.resplit(leaves, &refine)?;
        return Ok(None);
    }

    let mut out = FacePieces {
        point: fq.point,
        face: fq.face.tile_boxes().next().expect("tiles").face(),
        above: 0,
        below: 0,
        near: Vec::new(),
        joins: Vec::new(),
        pieces: 0,
    };
    let add = |out: &mut FacePieces, h: Height, degree: i64, near: Option<NearPiece>| match h {
        Height::Above => out.above += degree,
        Height::Below => out.below += degree,
        Height::Near => out.near.push(near.expect("a near piece")),
    };

    // Core leaves, each a piece.
    for l in leaves.iter() {
        let (Owner::Core, Some(h)) = (&l.owner, l.height) else {
            continue;
        };
        let patch = ctx.patch(&l.b)?;
        let d = box_degree_sided(fq.face, &l.b, patch, &ctx.q, fq.point, ctx.sides).map_err(
            |e| match e {
                SidedError::Box(e) => CountError::Box(e),
                SidedError::Need(k) => CountError::Need(k),
                SidedError::Through(_) => CountError::Through,
                SidedError::Unsettled => CountError::Unsettled,
            },
        )?;
        ctx.charge(d.pieces)?;
        let near = (h == Height::Near).then(|| NearPiece {
            degree: d.degree,
            features: SmallVec::new(),
            core: core_key(patch, &l.b),
            boxes: vec![l.b],
        });
        add(&mut out, h, d.degree, near);
    }

    // Runs: leaves of one owner and class, joined along shared sides.
    let mut groups: BTreeMap<(Owner, Height), Vec<usize>> = BTreeMap::new();
    for (k, l) in leaves.iter().enumerate() {
        if l.owner == Owner::Core {
            continue;
        }
        let h = match forced.get(&k) {
            Some(c) => *c,
            None => l.height,
        };
        if let Some(h) = h {
            groups.entry((l.owner.clone(), h)).or_default().push(k);
        }
    }
    for ((owner, h), ks) in groups {
        let parts = components(ks.len(), |i, j| share_side(&rects[ks[i]], &rects[ks[j]]));
        for part in parts {
            let idx: Vec<usize> = part.iter().map(|&i| ks[i]).collect();
            let runs: Vec<Vec<usize>> = match run_degree(ctx, &owner, leaves, &idx, cache)? {
                Ok(_) => vec![idx.clone()],
                Err(PieceError::Pinch) => idx.iter().map(|&k| vec![k]).collect(),
                Err(PieceError::Unsettled) => {
                    refine.extend(idx.iter().copied());
                    continue;
                }
                Err(PieceError::Need(k)) => return Err(CountError::Need(k)),
                Err(PieceError::Through) => return Err(CountError::Through),
                Err(PieceError::Box(e)) => return Err(CountError::Box(e)),
                Err(e) => return Err(CountError::Piece(e)),
            };
            for run in runs {
                let d = match run_degree(ctx, &owner, leaves, &run, cache)? {
                    Ok(d) => d,
                    Err(PieceError::Unsettled | PieceError::Pinch) => {
                        refine.extend(run.iter().copied());
                        continue;
                    }
                    Err(PieceError::Need(k)) => return Err(CountError::Need(k)),
                    Err(PieceError::Through) => return Err(CountError::Through),
                    Err(PieceError::Box(e)) => return Err(CountError::Box(e)),
                    Err(e) => return Err(CountError::Piece(e)),
                };
                let near = if h == Height::Near {
                    let ls: Vec<&Leaf> = run.iter().map(|&k| &leaves[k]).collect();
                    let mut features = ctx.feet(&owner, &ls)?;
                    if let Owner::Corner(v) = owner
                        && run.iter().any(|k| forced.contains_key(k))
                        && let Some(c) = fq.corners.corner(v)
                    {
                        for &w in &c.vertices {
                            features.push(Feature::Vert(w));
                        }
                    }
                    let patch = ctx.patch(&leaves[run[0]].b)?;
                    Some(NearPiece {
                        degree: d,
                        features,
                        core: core_key(patch, &leaves[run[0]].b),
                        boxes: run.iter().map(|&k| leaves[k].b).collect(),
                    })
                } else {
                    None
                };
                add(&mut out, h, d, near);
            }
        }
    }
    if !refine.is_empty() {
        ctx.resplit(leaves, &refine)?;
        return Ok(None);
    }

    // Near pieces touching in UV are of one layer.
    let near_rects: Vec<Vec<[[Rational; 2]; 2]>> = out
        .near
        .iter()
        .map(|p| p.boxes.iter().map(rect).collect())
        .collect();
    for i in 0..out.near.len() {
        for j in 0..i {
            let meet = near_rects[i]
                .iter()
                .any(|a| near_rects[j].iter().any(|b| touch(a, b)));
            if meet {
                out.joins.push((j, i));
            }
        }
    }
    out.pieces = ctx.pieces;
    Ok(Some(out))
}

/// The degree of the run `idx` of `owner`'s leaves, cached.
fn run_degree<S: Scalar, R: HeightReads>(
    ctx: &mut Ctx<'_, '_, '_, S, R>,
    owner: &Owner,
    leaves: &[Leaf],
    idx: &[usize],
    cache: &mut BTreeMap<(Owner, Vec<BoxKey>), Result<i64, PieceError>>,
) -> Result<Result<i64, PieceError>, CountError> {
    let boxes: Vec<BoxKey> = idx.iter().map(|&k| leaves[k].b).collect();
    let key = (owner.clone(), boxes);
    if let Some(r) = cache.get(&key) {
        return Ok(*r);
    }
    let ribbons = ctx.ribbons(owner)?;
    let fq = ctx.fq;
    let r = strip_degree(
        fq.face, fq.patches, &key.1, &ribbons, &ctx.q, fq.point, ctx.sides,
    );
    if let Ok(d) = &r {
        ctx.charge(d.pieces)?;
    }
    let r = r.map(|d| d.degree);
    // A record not yet decided is asked for, not cached.
    if !matches!(r, Err(PieceError::Need(_))) {
        cache.insert(key, r);
    }
    Ok(r)
}

/// A box's hull centre, its layer's key where the layer has no feature.
fn core_key<S: Scalar>(patch: &HomPatch<S>, b: &BoxKey) -> Option<Pos> {
    let h = box_hull(patch, b)?;
    Pos::new(std::array::from_fn(|k| h.lo[k] / 2.0 + h.hi[k] / 2.0)).ok()
}

impl FacePieces {
    /// The degrees of all the pieces summed: the shadow winding of the
    /// face's counting surface about `q` ([`ray_degree`]).
    pub fn degree(&self) -> i64 {
        self.above + self.below + self.near.iter().map(|p| p.degree).sum::<i64>()
    }

    fn layer_pieces(&self) -> LayerPieces {
        let mut lp = LayerPieces::new();
        for p in &self.near {
            lp.piece(p.features.iter().copied(), p.core);
        }
        for &(a, b) in &self.joins {
            lp.join(a, b);
        }
        lp
    }

    /// The face's layers at the point from its near pieces, to store in
    /// the [`LayerTable`] the counting reads.
    pub fn layers<R: HeightReads>(&self, reads: &R) -> Result<Layers, KeyError> {
        self.layer_pieces().layers(reads, self.point, self.face)
    }

    /// Per layer the column enters, its pieces' degrees summed. The
    /// layers stored in `table` must be these pieces' own: layers found
    /// from another refinement may number core layers differently, and a
    /// record read through them could be another layer's.
    fn layer_degrees<R: HeightReads>(
        &self,
        reads: &R,
        table: &LayerTable,
    ) -> Result<Vec<(LayerId, i64)>, KeyError> {
        let (layers, of) = self
            .layer_pieces()
            .layers_of(reads, self.point, self.face)?;
        let entered = self.near.iter().any(|p| p.degree != 0);
        if entered && table.get(self.point, self.face) != Some(&layers) {
            return Err(KeyError::NoLayer {
                q: self.point,
                f: self.face,
                layer: LayerId(0),
            });
        }
        let mut sums: Vec<(i64, bool)> = vec![(0, false); layers.len()];
        for (p, l) in self.near.iter().zip(of) {
            let s = &mut sums[l.0 as usize];
            s.0 += p.degree;
            s.1 |= p.degree != 0;
        }
        Ok(layers
            .iter()
            .zip(sums)
            .filter(|(_, (_, entered))| *entered)
            .map(|((id, _), (d, _))| (id, d))
            .collect())
    }

    /// The anchor the layers' records are read at.
    fn anchor(&self) -> Anchor {
        Anchor::Face(self.point.operand.other(), self.face)
    }

    /// The height records the count reads (module docs): one per layer
    /// the column enters.
    pub fn wanted<R: HeightReads>(
        &self,
        reads: &R,
        layers: &LayerTable,
    ) -> Result<Vec<HeightKey>, KeyError> {
        let mut out = Vec::new();
        for (l, _) in self.layer_degrees(reads, layers)? {
            let h = height_key(reads, layers, self.point, Target::Layer(self.face, l))?;
            out.push(HeightKey {
                h: h.key,
                anchor: self.anchor(),
            });
        }
        Ok(out)
    }

    /// `s02(q, f)` for a point of `A`, `s20(q, f)` for one of `B` (module
    /// docs), the layers read from `layers` and their records from
    /// `heights`.
    pub fn count<R: HeightReads>(
        &self,
        reads: &R,
        layers: &LayerTable,
        heights: &Heights,
    ) -> Result<i64, CountError> {
        let a = self.point.operand == Operand::A;
        let mut s = if a { self.above } else { -self.below };
        for (l, d) in self.layer_degrees(reads, layers)? {
            let h = height_key(reads, layers, self.point, Target::Layer(self.face, l))?;
            let rec = heights
                .get(h, self.anchor())
                .ok_or(CountError::NeedHeight(HeightKey {
                    h: h.key,
                    anchor: self.anchor(),
                }))?;
            s += if rec.fold {
                i64::from(rec.above.as_i8())
            } else {
                match (a, rec.above) {
                    (true, Sign::Pos) => d,
                    (false, Sign::Neg) => -d,
                    _ => 0,
                }
            };
        }
        Ok(s)
    }
}

#[cfg(test)]
mod tests;
