//! The column hits of a face above or below a point: `s02(q, f)`, `q` a
//! point of `A` and `f` a face of `B`, the winding of an upward ray from
//! `q` through `f`'s counting surface (`+1` per up-facing hit above `q`,
//! `−1` per down-facing one), and `s20(q, f)`, `q` of `B` and `f` of `A`,
//! the downward ray's (`+1` per down-facing hit below `q`, `−1` per
//! up-facing one).
//!
//! **Pieces.** The face's tiles are refined into boxes ([`role`]): an
//! outside box is dropped; a box whose shadow is certified off `q_xy` has
//! degree 0 and is dropped too (a strip or corner box with its ribbons:
//! its hull padded by its curves' band, which the ribbons stay within);
//! a thin box always refines. The others refine until **height-clear**,
//! their (padded) hull certified above or below `q` along `UP`, or small:
//! a strip or corner box within its curves' least band across, a core box
//! within `ε_tie` ([`FaceQuery::eps_tie`]). The size is no stop for the
//! heights: on a face given its stored nets, plane or curved, the
//! **column hits** are exact points on the stored tiles and ribbons
//! ([`hit`](crate::records::height::hit)), which need no box size to
//! hold them apart, and boxes refined that far are mostly height-clear,
//! which costs less than their hits. A core
//! leaf is a piece of its own, its degree its box's
//! ([`box_degree_sided`]); where the level doesn't certify that (a side
//! inside a tile not certified off `q_xy`, too long for its turn or
//! through it), it refines. The strip and corner leaves form **runs**:
//! the leaves of one strip (one coedge, or a slit's) or one corner, of one
//! height class (above, below, near), joined where they share a side;
//! each run with its curves' ribbons is a piece
//! ([`strip_degree`](super::pieces::strip_degree)), a corner's run with
//! every coedge of the corner. A run passing a point twice is taken box
//! by box. A near piece counts each hit it holds by its own height and
//! facing where every hit lies strictly inside its boxes, none on the
//! face's boundary at `q` (which the moved column may miss), and their
//! facings sum to its degree; else its degree by one hit where they are
//! at one exact side of `q` (or all at `q`), or where `q` is a vertex of
//! the face and they are hits at `q` on its boundary beside hits within
//! `ε_tie` of it (the vertex's, one point of the ideal face: a box at its
//! last split holding such stops there too); else it is taken box by box
//! (degrees are additive, so any grouping of a run's boxes partitions its
//! piece), and a box that still doesn't settle refines.
//!
//! **Refining runs.** A run the level can't settle (a cut next to the
//! ray, a run the curve doesn't cut and nothing on its boundary places,
//! an outer side inside a tile not certified off `q_xy`) refines its
//! boxes where it is unsettled, those meeting the place its piece names
//! (a cut's window, a side's stretch, a curve's end, the uncut loop:
//! [`strip_degree_near`]), or all of them where the piece names none, and
//! the runs are built again. So a long run refines next to its cut, not
//! along its length. A run whose boxes the trims miss altogether (checked
//! exactly, [`span_meets`]) is no strip: its boxes are core or outside
//! by the trims' winding about a point of them, exactly
//! ([`winding_of`]).
//!
//! **Runs near `q`.** A cut between a near run and dropped leaves of its
//! owner next to it that no window certifies off the ray stays there
//! however the boxes refine: a dropped box's parts are dropped, and the
//! near run's boxes are within their size already. So the dropped leaves
//! there join the near run (their degree, ribbons included, is 0, and
//! they hold no hit), and its cut moves past them, away from `q`. A
//! height-clear leaf may hold the column's hit, which counts by the
//! leaf's height, so a cut next to one refines as any other; only where
//! the run holds the column's hit at `q` on the face's boundary and its
//! leaves there are refined as far as they go (one of them a split short
//! of [`MAX_SUBDIV_DEPTH`] in `u` and in `v`) does a clear leaf there
//! whose closed box holds no hit of the column, exactly (on a face with
//! its nets), join. Where `q` lies on a trim of a steep face, every box
//! beside the near one is clear of `q`'s height however small, so
//! refining there never moves the cut off the ray; joining those
//! hit-free clear boxes does.
//!
//! **Around a vertex.** A corner's vertex lying on a box side (on a tile
//! line, or a dyadic line the splits couldn't move) lies in one box only
//! (box lines read as moved,
//! [`strip_degree`](super::pieces::strip_degree)), but next to the sides
//! of the boxes around it: where `q` lies at the vertex (a shared
//! corner), a run that doesn't hold every box around it has cuts next to
//! `q` that no level certifies off its ray. So the corner's leaves
//! holding one of its vertices (in their closed UV box) are one run,
//! whatever their heights: their class is the one class of those not
//! dropped, or, where they differ, near, once each is settled (a dropped
//! one joins: its degree, ribbons included, is 0).
//!
//! **Heights.** A height-clear piece is geometric: `s02` sums the degrees
//! of the pieces above `q`, `s20` the negated degrees of those below. A
//! near piece reads its hits' height records ([`height_key`] against the
//! hit, anchored at the face: the hit's own key `Hit(q, f, sheet, i)`
//! but where the hit lies exactly on a vertex or edge of the face at
//! `q`'s height, the germ key): each hit above (below for `s20`) counts
//! its facing, or the piece's degree where it counts by one hit (a piece
//! of degree 0 so counting reads nothing). So an overhanging sheet
//! (a face overhanging its own edge in the shadow) counts by its own
//! height, the tile and ribbon of one face straddling `q` count each by
//! its own, and every face at a shared vertex reads the one point–vertex
//! record. Where `q` is a vertex of the face, the piece's hits within
//! `ε_tie` of it read that vertex's germ key with the hit at it: the
//! face's tiles pass within rounding of their vertices, not through
//! them, so a tile's hit a rounding off the vertex is the vertex of the
//! ideal face, whose first-order germ the key reads (its own height, of
//! the rounding's sign, would mix that sign with the germ's); the piece
//! counts its degree by the germ. A near piece of non-zero degree
//! holding no hit is [`CountError::NoHit`] (only where its hits can't be
//! counted alone, below); on a face counted without its nets it is
//! [`CountError::HitsUnformed`].
//!
//! **By the hits.** The column's hits are exact, and
//! their facings sum to the degree of the face's pieces. Where the hits
//! were found and the pieces disagree with them (their degrees summing to
//! another value, or a near piece of non-zero degree holding none: a
//! column within rounding of an edge, where the boxes' records decide at
//! a level the hits are exact past), and where the boxes don't settle at
//! any level, the face is counted by its hits alone: each hit clear of
//! `q`'s height and of every edge's near radius by its exact height, the
//! others as one near piece counting each by its record and facing. A
//! hit whose facing isn't formed, or on the face's boundary at `q` (which
//! the moved column may miss), leaves the pieces' answer.
//!
//! **The lobe fallback** ([`FacePieces::count`]'s `lobe`). Where the
//! winding the hits give leaves `{0, 1}` at a point within an edge `g`'s
//! near radius (the faces at `g` overshooting each other there, so their
//! counting surfaces aren't embedded: a lobe; [`crate::count::winding`]),
//! the point's hits near `g` (each
//! hit records the edges whose near radius holds it, with the point's
//! foot there) read the edge rule's key
//! ([`edge_rule_key`]) instead.
//!
//! **The fold rule.** A piece whose record is a fold's (its anchor's
//! facing certified at no level, [`HeightRec::fold`]) contributes the
//! fold's upper sheet's facing (`+1` up), for `s02` and `s20` alike: `q`
//! lies between the two sheets, the upper one above it and the lower
//! below.
//!
//! **What the caller gives** ([`FaceQuery`]): the face's boxes' data
//! ([`FaceUv`], its [`Corners`], its tiles' patches at the level worked
//! at), per trim its edge and ribbon ([`TrimEdge`]: the edge's curve, the
//! p-spans' places on it, the query point's ray record against it,
//! [`line_chain`] for a straight edge), the face's stored tile nets, the
//! query point, its motion against the face (the fold rule's side) and
//! `ε_tie`. The counting asks for records it reads and
//! doesn't find ([`CountError::Need`], [`CountError::NeedHeight`]): fill
//! them and ask again.
//!
//! **Termination** ([`count_pieces`]). Refinement is in space first: a
//! box splits at most [`MAX_SUBDIV_DEPTH`] times from its tile in each of `u`
//! and `v` (halvings of its sides, as the cap counts them), and a
//! piece the level doesn't settle by then (or at the boxes' own depth
//! limit) leaves the level unsettled: a face with its nets is then
//! counted by its hits (above), one without runs again in precision: at
//! the next level
//! of the schedule ([`LEVELS`]: `f64`, double-double, the big-float
//! widths), the patches lifted exactly. A count still unsettled at the
//! last level is
//! [`CountError::OutOfBudget`] with [`Exhausted::Precision`], keyed by
//! the point and the face, never a loop. Every piece tested is charged,
//! at its level's units ([`Charge`]), against the share of the budget the
//! count is given: a box's classification and split (a split the cap
//! then refuses included), a degree attempted whether it settles or not
//! (a side or a run read again at a finer box is work again), a trim span
//! scanned against a run's sides or a loop's points. The column's hits
//! are found once for every level and charged when found, per tile and
//! ribbon span searched, at their own row. A step is charged after it
//! runs, so the one that crosses the share is the crossing: the count
//! stops at once, [`Exhausted::Work`] at the first level and
//! [`Exhausted::Precision`] above it, keyed by the box whose test
//! crossed it (none where the hits did). [`MAX_LEAVES`] and
//! [`MAX_ROUNDS`] are fixed limits, [`Exhausted::SizeLimit`]. The face's
//! corners are its setup, built and charged once per operation
//! ([`Corners::new`]).

use std::cell::OnceCell;
use std::cmp::Ordering;
use std::collections::BTreeMap;

use malachite::Rational;
use smallvec::SmallVec;

use super::boxes::{
    BoxError, BoxKey, Corners, FaceUv, QShadow, Role, SpanRef, box_hull, global, role, split,
    sub_patch, within_band,
};
use super::degree::{SidedError, box_degree_counted};
use super::pieces::{ExactRect, PieceError, Ribbon, UvRect, exact_rect, strip_degree_counted};
use crate::bezier::{Dir, HomPatch, Scalar};
use crate::ids::{CoedgeId, EdgeId, FaceId, VertId};
use crate::num::bounded::{Bounded, add_up, mul_up, up};
use crate::num::decide::Sign;
use crate::num::schedule::{LEVELS, MAX_SUBDIV_DEPTH, UNIT_PARTS, UNITS};
use crate::order::{Along, EntityOrder};
use crate::perturb::{NU, R_HAT, UP};
use crate::records::feet::Foot;
use crate::records::height::hit::{
    Hit, HitAt, HitError, HitFace, HitTrim, Motion, edge_t, face_hits, span_meets, winding_of,
};
use crate::records::height::{HeightReads, Heights, KeyError, edge_rule_key, height_key};
use crate::records::keys::{
    Anchor, FootId, HKey, HeightKey, Incidence, Operand, Oriented, Point, RayKey, RecordKey, Target,
};
use crate::records::ray::RayLevel;
use crate::records::ray::{ChainCrossing, ChainRec, Crossing, TileNet};
use crate::records::shadow::{AtLevel, Cost, LevelWork, RayRec, ladder};
use crate::records::side::{Sides, Want};

#[cfg(doc)]
use super::{degree::box_degree_sided, pieces::strip_degree_near};
#[cfg(doc)]
use crate::records::height::HeightRec;

/// The most leaves one face's count keeps.
pub const MAX_LEAVES: usize = 1 << 16;

/// The most times the runs are built again after refining some (not
/// counting the times after only joining some to a near run).
pub const MAX_ROUNDS: u32 = 64;

/// What a count (or another search charged per piece: a face's setup,
/// an edge pair's clusters) may spend (module docs): `share` units, a
/// piece tested at level `k` costing `units[k]`, a column hit search its
/// own row ([`RecordUnits::hit`](crate::num::schedule::RecordUnits::hit)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Charge {
    /// The units the search may spend, its share of the budget left.
    pub share: u64,
    /// The units of one piece tested, per precision level.
    pub units: [u64; LEVELS],
}

impl Charge {
    /// A share of `share` units at the table's units for count pieces
    /// ([`Units::count_piece`](crate::num::schedule::Units::count_piece)).
    pub fn new(share: u64) -> Charge {
        Charge {
            share,
            units: UNITS.count_piece,
        }
    }

    /// A share of `share` units for a face's counting setup
    /// ([`Corners::new`]), at the table's unit for its pieces
    /// ([`Units::face_setup`](crate::num::schedule::Units::face_setup)).
    pub fn setup(share: u64) -> Charge {
        Charge {
            share,
            units: [UNITS.face_setup; LEVELS],
        }
    }

    /// A share of `share` units for an edge pair's cluster search
    /// ([`clusters`](crate::count::xrec::clusters)), at the table's units
    /// for its pieces
    /// ([`Units::xrec_piece`](crate::num::schedule::Units::xrec_piece)).
    pub fn xrec(share: u64) -> Charge {
        Charge {
            share,
            units: UNITS.xrec_piece,
        }
    }

    /// The units of `cost`'s pieces, and of its column hit searches
    /// (rounded up to whole units).
    pub fn spent(&self, cost: &Cost) -> u64 {
        let hits = hit_units(cost.hits);
        cost.levels.iter().zip(self.units).fold(hits, |s, (l, u)| {
            s.saturating_add(u64::from(l.evals).saturating_mul(u))
        })
    }
}

/// The units of `n` column hit searches, rounded up.
fn hit_units(n: u32) -> u64 {
    u64::from(n)
        .saturating_mul(UNITS.record.hit)
        .div_ceil(UNIT_PARTS)
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
    /// The face's tiles' stored nets, one per tile in
    /// [`FaceUv::tile_boxes`]' order (module docs, "Pieces"): its column
    /// hits are exact on them; `None` for a face counted without its hits
    /// (a near piece of non-zero degree is then refused).
    pub nets: Option<&'a [TileNet]>,
    /// The face's rates less the query point's at its column, per motion
    /// (the fold rule's side, [`Motion`]).
    pub motion: &'a Motion,
}

/// Why a face's count isn't certified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CountError {
    /// A box or the face is refused, or the query point is of the face's
    /// operand, or its position isn't finite, or `ε_tie` isn't.
    Box(BoxError),
    /// A piece reads this side or dyadic point record, not yet decided:
    /// fill it and ask again.
    Need(Want),
    /// A piece reads this height record, not yet decided: fill it (with
    /// every anchor of its key) and ask again.
    NeedHeight(HeightKey),
    /// A record a hit's key reads is missing.
    Key(KeyError),
    /// The column's hits on the face can't be formed (the column in a
    /// planar tile, along a radial, tangent to a ribbon at `q`'s height, a
    /// fold whose signs aren't formed at its root). A fold that forms no
    /// hit (a zero bend, or on a trim at `q`) is
    /// [`CountError::OutOfBudget`] with [`Exhausted::Precision`] instead.
    Hits(HitError),
    /// A near piece of non-zero degree holds no hit of the column: the
    /// exact hits and the boxes' records disagree.
    NoHit {
        /// The query point.
        point: Point,
        /// The face.
        face: FaceId,
    },
    /// A near piece of a face counted without its nets holds the column
    /// (non-zero degree): its hit's height needs the hits, which aren't
    /// formed then.
    HitsUnformed {
        /// The query point.
        point: Point,
        /// The face.
        face: FaceId,
    },
    /// A strip or corner piece is refused (a ribbon that doesn't fit, a
    /// corner missing a coedge's ribbon).
    Piece(PieceError),
    /// A trim of the face has no [`TrimEdge`].
    Edge { coedge: CoedgeId },
    /// The level doesn't settle a piece down to the deepest boxes
    /// ([`MAX_SUBDIV_DEPTH`]), a box side inside a tile not certified
    /// off `q_xy` included (no record counts such a side): escalate.
    Unsettled,
    /// The count ran out (module docs), keyed by its point and face.
    OutOfBudget {
        /// What ran out.
        cause: Exhausted,
        /// The query point.
        point: Point,
        /// The face.
        face: FaceId,
        /// Where the share ran out: the box whose test crossed it (`None`
        /// past the share on the column's hits, and for the fixed limits
        /// and the last level unsettled, which no one box reaches).
        at: Option<BoxKey>,
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

/// A piece's failure other than unsettled (or through, which refine), as
/// the count's.
impl From<PieceError> for CountError {
    fn from(e: PieceError) -> CountError {
        match e {
            PieceError::Need(k) => CountError::Need(k),
            PieceError::Box(e) => CountError::Box(e),
            e => CountError::Piece(e),
        }
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
    /// Taken into the near run next to it across a cut the level doesn't
    /// settle (module docs, "Runs near `q`"): near whatever its height.
    joined: bool,
    met: SmallVec<[SpanRef; 8]>,
    /// The splits from its tile, in `u` and in `v`.
    splits: [u32; 2],
    /// The column's hits in its closed box, by index (with the face's nets,
    /// found for a near leaf; `None` until asked for).
    hits: Option<SmallVec<[u32; 2]>>,
}

/// Whether a set of hits is settled for one piece: at most one, or all at
/// one exact side of `q`, or all at `q` itself (one point, on several
/// sheets: a vertex or an edge of the face there, the germ rule's; module
/// docs, "Pieces").
fn settled(hits: &[Hit], ix: &[u32]) -> bool {
    match ix {
        [] | [_] => true,
        [first, rest @ ..] => {
            let h = hits[*first as usize].height;
            rest.iter().all(|&k| hits[k as usize].height == h)
        }
    }
}

/// Whether `at` is a vertex of `face`: an end of one of its trims'
/// edges, exactly.
fn at_vertex(face: &HitFace<'_>, at: [f64; 3]) -> bool {
    face.trims.iter().any(|t| {
        let ends = [t.curve.first().map(|s| s[0]), t.curve.last().map(|s| s[2])];
        ends.into_iter()
            .flatten()
            .any(|p| p[0] == at[0] && p[1] == at[1] && p[2] == at[2])
    })
}

/// Whether the closed box `b` lies within `r` of `at` along each axis,
/// exactly.
fn within(b: &[[Rational; 2]; 3], at: [f64; 3], r: f64) -> bool {
    let (Ok(r), true) = (Rational::try_from(r), at.iter().all(|x| x.is_finite())) else {
        return false;
    };
    (0..3).all(|k| {
        let Ok(c) = Rational::try_from(at[k]) else {
            return false;
        };
        b[k][0] >= &c - &r && b[k][1] <= &c + &r
    })
}

/// The union of index sets, ascending.
fn union<'i>(sets: impl IntoIterator<Item = &'i SmallVec<[u32; 2]>>) -> SmallVec<[u32; 2]> {
    let mut out: SmallVec<[u32; 2]> = sets.into_iter().flatten().copied().collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// A near piece: its degree and the column's hits it holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NearPiece {
    /// Its degree.
    pub degree: i64,
    /// The hits in its boxes (closed), by index into [`FacePieces::hits`],
    /// ascending.
    pub hits: SmallVec<[u32; 2]>,
    /// Whether it counts each of its hits by its own height and facing
    /// (each strictly inside its boxes, their facings summing to its
    /// degree); else its degree counts by one hit's height (at most one
    /// hit, or all at one exact side of `q` or at `q`).
    pub each: bool,
    /// Where it counts each hit: the first of its hits at `q` on the
    /// face's boundary (a vertex or an edge of the face there), which
    /// reads its germ key for the piece's degree less the other hits'
    /// facings, those hits at `q` counted so and not each (module docs,
    /// "Heights").
    pub germ: Option<u32>,
}

/// A column hit of the point on the face, as the counting reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HitTarget {
    /// The target its height key is read against (its incidence
    /// resolved to the point's foot).
    pub target: Target,
    /// Its height above the point, exactly (`Equal`: a tie the record's
    /// orders decide).
    pub height: Ordering,
    /// The sheet's facing there as an upward ray counts it, `Pos` facing
    /// up (`None` at a radial of no length at the point).
    pub facing: Option<Sign>,
    /// The face's edges whose near radius holds the hit, with the point's
    /// foot on each that the edge rule reads ([`edge_rule_key`]); an edge
    /// on which the point has no foot is left out.
    pub near: SmallVec<[(EdgeId, FootId); 2]>,
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
    /// The column's hits on the face; empty where it is counted without
    /// its nets.
    pub hits: Vec<HitTarget>,
    /// Whether the face is counted without its nets: its hits aren't
    /// formed.
    pub unformed: bool,
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
        crossing: Crossing {
            t,
            sign,
            single: true,
        },
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
fn rect(b: &BoxKey) -> ExactRect {
    [Dir::U, Dir::V].map(|d| {
        let k = usize::from(d == Dir::V);
        let e = b.extent(d);
        [global(b.tile()[k], e.lo()), global(b.tile()[k], e.hi())]
    })
}

/// Whether two boxes share a stretch of a side.
fn share_side(a: &ExactRect, b: &ExactRect) -> bool {
    (0..2).any(|k| {
        let o = 1 - k;
        (a[k][1] == b[k][0] || b[k][1] == a[k][0]) && a[o][0] < b[o][1] && b[o][0] < a[o][1]
    })
}

/// Whether two closed boxes meet.
fn touch(a: &ExactRect, b: &ExactRect) -> bool {
    (0..2).all(|k| a[k][0] <= b[k][1] && b[k][0] <= a[k][1])
}

fn holds(r: &ExactRect, p: &[Rational; 2]) -> bool {
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

/// The column's hits on a face, found when first asked for and kept
/// for every level ([`count_pieces`]), with whether their search is
/// charged yet.
#[derive(Default)]
struct HitCell {
    hits: OnceCell<Result<Vec<Hit>, HitError>>,
    charged: std::cell::Cell<bool>,
}

/// A face's exact data and the column's hits on it, found when
/// first asked for (a face whose leaves are all clear or dropped asks
/// none).
struct Flat<'a, 'h> {
    face: HitFace<'a>,
    point: Point,
    at: [f64; 3],
    motion: &'a Motion,
    cell: &'h HitCell,
}

impl Flat<'_, '_> {
    /// The column's hits on the face.
    fn hits(&self) -> Result<&[Hit], CountError> {
        self.cell
            .hits
            .get_or_init(|| face_hits(&self.face, self.at, self.motion))
            .as_deref()
            .map_err(|e| self.error(*e))
    }

    /// The count error of the column's hits not formed: a fold that forms
    /// no hit (a zero bend, or on a trim at `q`) ends as a piece no level
    /// certifies does, out of precision; the rest are the hits' own.
    fn error(&self, e: HitError) -> CountError {
        match e {
            HitError::Fold(_) => CountError::OutOfBudget {
                cause: Exhausted::Precision,
                point: self.point,
                face: self.face.face,
                at: None,
            },
            e => CountError::Hits(e),
        }
    }

    /// The tiles and ribbon spans whose hits the column's search forms.
    fn searches(&self) -> u32 {
        let spans: usize = self.face.trims.iter().map(|t| t.pspans.len()).sum();
        u32::try_from(self.face.nets.len().saturating_add(spans)).unwrap_or(u32::MAX)
    }
}

/// What one level of a count may spend: `unit` units a piece, `left`
/// units in all (module docs).
#[derive(Clone, Copy, Debug)]
struct Allowance {
    unit: u64,
    left: u64,
}

impl Allowance {
    const FREE: Allowance = Allowance {
        unit: 0,
        left: u64::MAX,
    };
}

struct Ctx<'q, 'a, 'f, S> {
    fq: &'q FaceQuery<'a, 'f, S>,
    sides: &'q Sides,
    q: QShadow,
    up_q: [f64; 2],
    like: S,
    pieces: u64,
    /// The column hit searches charged at this level.
    hits: u32,
    /// What this level may spend.
    allow: Allowance,
    /// With the face's nets, its hits.
    flat: Option<Flat<'a, 'q>>,
}

impl<'a, S: Scalar> Ctx<'_, 'a, '_, S> {
    fn patch(&self, b: &BoxKey) -> Result<&'a HomPatch<S>, CountError> {
        self.fq
            .face
            .patch_of(self.fq.patches, b.tile())
            .ok_or(CountError::Box(BoxError::Patches))
    }

    /// Counts `n` pieces tested at box `at` (and the column's hits
    /// if found since), failing once past what the level may spend: a
    /// step's work is charged after it runs, so the step that crosses is
    /// the crossing (module docs).
    fn charge(&mut self, n: u64, at: &BoxKey) -> Result<(), CountError> {
        self.pieces = self.pieces.saturating_add(n);
        self.take_hits();
        if self.spent() > self.allow.left {
            return Err(self.out_at(Exhausted::Work, Some(*at)));
        }
        Ok(())
    }

    /// Charges the column's hits if they were found and aren't charged
    /// yet, failing once past what the level may spend.
    fn settle(&mut self) -> Result<(), CountError> {
        self.take_hits();
        if self.spent() > self.allow.left {
            return Err(self.out_at(Exhausted::Work, None));
        }
        Ok(())
    }

    fn take_hits(&mut self) {
        if let Some(f) = &self.flat
            && f.cell.hits.get().is_some()
            && !f.cell.charged.replace(true)
        {
            self.hits = self.hits.saturating_add(f.searches());
        }
    }

    /// The units spent at this level.
    fn spent(&self) -> u64 {
        self.pieces
            .saturating_mul(self.allow.unit)
            .saturating_add(hit_units(self.hits))
    }

    fn out(&self, cause: Exhausted) -> CountError {
        self.out_at(cause, None)
    }

    fn out_at(&self, cause: Exhausted, at: Option<BoxKey>) -> CountError {
        CountError::OutOfBudget {
            cause,
            point: self.fq.point,
            face: self.fq.face.tile_boxes().next().expect("tiles").face(),
            at,
        }
    }

    /// The split of `b`, `splits` splits from its tile in `u` and in `v`,
    /// and the halves' counts: the level is unsettled where the split
    /// would take a direction past [`MAX_SUBDIV_DEPTH`] halvings, or past
    /// the boxes' depth.
    fn split(
        &mut self,
        b: &BoxKey,
        splits: [u32; 2],
    ) -> Result<([BoxKey; 2], [u32; 2]), CountError> {
        // Both directions at the cap: no split is tried.
        if splits.iter().all(|&n| n >= MAX_SUBDIV_DEPTH) {
            return Err(CountError::Unsettled);
        }
        let patch = self.patch(b)?;
        let s = split(self.fq.face, b, patch, Some(&self.q)).map_err(|e| match e {
            BoxError::Depth => CountError::Unsettled,
            e => CountError::Box(e),
        })?;
        // Its work is done: charged, whether the cap takes it or not.
        self.charge(s.pieces, b)?;
        let k = usize::from(s.halves[0].extent(Dir::U) == b.extent(Dir::U));
        if splits[k] >= MAX_SUBDIV_DEPTH {
            return Err(CountError::Unsettled);
        }
        let mut next = splits;
        next[k] += 1;
        Ok((s.halves, next))
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
        splits: [u32; 2],
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
            self.charge(r.pieces, &b)?;
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
            self.charge(1, &b)?;
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
                        joined: false,
                        met: r.met,
                        splits,
                        hits: None,
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
            // Settled for its hits (module docs, "Pieces"): with the
            // face's nets a near box holding at most one, or several at
            // one exact side of `q`; without, a box within its size.
            // Small: a strip or corner box within its curves' least band
            // across, a core box within `ε_tie`. The hits are exact points, which need no box size to hold them apart; its
            // near boxes still refine to that size, as height-clear boxes
            // cost less than hits (module docs, "Pieces").
            let size = if owner == Owner::Core {
                self.fq.eps_tie
            } else {
                pad
            };
            let mut small = box_hull(patch, &b).is_some_and(|hb| within_band(&hb, size));
            // With the face's nets a near box at its last split holding settled
            // hits (a hit at `q`'s height, which no box size clears), or a
            // vertex's hits (one point of the ideal face, which the near
            // piece reads by the germ key), stops there as a small one.
            if !small
                && !thin
                && h.is_none()
                && splits.iter().any(|&s| s + 1 >= MAX_SUBDIV_DEPTH)
                && let Some(f) = &self.flat
            {
                let ix = self.hits_in(&b)?;
                let all = f.hits()?;
                small = settled(all, &ix) || self.at_vertex_hits(all, &ix);
            }
            let hits = match &self.flat {
                Some(_) if h.is_none() && small && !thin => Some(self.hits_in(&b)?),
                _ => None,
            };
            if !thin && (h.is_some() || small) {
                out.push(Leaf {
                    b,
                    owner,
                    height: Some(h.unwrap_or(Height::Near)),
                    small,
                    joined: false,
                    met: r.met,
                    splits,
                    hits,
                });
                continue;
            }
            let ([lo, hi], next) = self.split(&b, splits)?;
            stack.push((hi, Some(r.met.clone()), next));
            stack.push((lo, Some(r.met), next));
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
            let (halves, next) = self.split(&l.b, l.splits)?;
            for h in halves {
                self.refine(h, Some(l.met.clone()), next, leaves)?;
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

    /// The near piece of the leaves `run` of degree `degree` with the
    /// hits they hold (module docs, "Heights"): counting each hit where
    /// all lie strictly inside its boxes and their facings sum to its
    /// degree, else by one hit where they are settled; `None` where
    /// neither holds (the piece is taken apart, or refined). A curved
    /// face's pieces hold no hits.
    fn near_piece(
        &self,
        leaves: &[Leaf],
        run: &[usize],
        degree: i64,
    ) -> Result<Option<NearPiece>, CountError> {
        let Some(f) = &self.flat else {
            return Ok(Some(NearPiece {
                degree,
                hits: SmallVec::new(),
                each: false,
                germ: None,
            }));
        };
        let mut sets = Vec::with_capacity(run.len());
        for &k in run {
            sets.push(match &leaves[k].hits {
                Some(h) => h.clone(),
                None => self.hits_in(&leaves[k].b)?,
            });
        }
        let hits = union(&sets);
        let all = f.hits()?;
        let inside = hits.iter().all(|&h| {
            let hit = &all[h as usize];
            run.iter()
                .any(|&k| hit.in_rect_open(&f.face, &rect(&leaves[k].b)))
        });
        let facings: Option<i64> = hits
            .iter()
            .map(|&h| all[h as usize].facing.map(|s| i64::from(s.as_i8())))
            .sum();
        // A hit on the face's boundary at `q` counts by the side of the
        // moved column the boxes' degree reads, not by its own facing.
        let clear = hits.iter().all(|&h| all[h as usize].inc.is_none());
        if hits.len() > 1 && inside && clear && facings == Some(degree) {
            return Ok(Some(NearPiece {
                degree,
                hits,
                each: true,
                germ: None,
            }));
        }
        if settled(all, &hits) {
            return Ok(Some(NearPiece {
                degree,
                hits,
                each: false,
                germ: None,
            }));
        }
        // Hits at `q` on the face's boundary beside others that are not:
        // the others, strictly inside the boxes and of formed facing, count
        // each by its own height, the rest of the degree (degrees are
        // additive) by the germ key at `q`.
        let (at_q, others): (Vec<u32>, Vec<u32>) =
            hits.iter().partition(|&&h| all[h as usize].inc.is_some());
        // But where `q` is a vertex of the face, hits within `ε_tie` of it
        // are the vertex's: the face's tiles pass within rounding of their
        // vertices, not through them, so a tile hit a rounding off the
        // vertex beside the ribbon's hit at it is one point of the ideal
        // face, which the germ key reads (its own height, of the rounding's
        // sign, would mix that sign with the germ's first-order one). The
        // piece's degree then counts by the germ hit.
        if self.at_vertex_hits(all, &hits) {
            return Ok(Some(NearPiece {
                degree,
                hits,
                each: false,
                germ: None,
            }));
        }
        let others_ok = others.iter().all(|&h| {
            let hit = &all[h as usize];
            hit.facing.is_some()
                && run
                    .iter()
                    .any(|&k| hit.in_rect_open(&f.face, &rect(&leaves[k].b)))
        });
        if let (Some(&g), false, true) = (at_q.first(), others.is_empty(), others_ok) {
            return Ok(Some(NearPiece {
                degree,
                hits,
                each: true,
                germ: Some(g),
            }));
        }
        Ok(None)
    }

    /// Whether the leaves `run` hold a hit of the column at `q` on the
    /// face's boundary (`q` exactly on a trim or at a vertex), with the
    /// face's nets.
    fn holds_boundary_hit(&self, leaves: &[Leaf], run: &[usize]) -> Result<bool, CountError> {
        let Some(f) = &self.flat else {
            return Ok(false);
        };
        let Ok(all) = f.hits() else {
            return Ok(false);
        };
        for &k in run {
            let held = match &leaves[k].hits {
                Some(h) => h.clone(),
                None => self.hits_in(&leaves[k].b)?,
            };
            if held.iter().any(|&h| all[h as usize].inc.is_some()) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Whether the leaf's closed box holds no hit of the column, known
    /// exactly: with the face's nets and its hits formed; `false` without.
    fn holds_no_hit(&self, l: &Leaf) -> Result<bool, CountError> {
        let Some(f) = &self.flat else {
            return Ok(false);
        };
        if f.hits().is_err() {
            return Ok(false);
        }
        Ok(match &l.hits {
            Some(h) => h.is_empty(),
            None => self.hits_in(&l.b)?.is_empty(),
        })
    }

    /// Whether the hits `ix` are a vertex's (module docs, "Pieces"): `q`
    /// a vertex of the face, some of them at `q` on the face's boundary
    /// and the rest within `ε_tie` of `q`, exactly; one point of the ideal
    /// face, read by the germ key.
    fn at_vertex_hits(&self, all: &[Hit], ix: &[u32]) -> bool {
        let Some(f) = &self.flat else {
            return false;
        };
        let (at_q, others): (Vec<u32>, Vec<u32>) =
            ix.iter().partition(|&&h| all[h as usize].inc.is_some());
        !at_q.is_empty()
            && at_vertex(&f.face, f.at)
            && others
                .iter()
                .all(|&h| within(&all[h as usize].near, f.at, self.fq.eps_tie))
    }

    /// The column's hits in `b`'s closed box, by index (with the face's
    /// nets).
    fn hits_in(&self, b: &BoxKey) -> Result<SmallVec<[u32; 2]>, CountError> {
        let Some(f) = &self.flat else {
            return Ok(SmallVec::new());
        };
        let r = rect(b);
        Ok((0u32..)
            .zip(f.hits()?)
            .filter(|(_, h)| h.in_rect(&f.face, &r))
            .map(|(k, _)| k)
            .collect())
    }
}

/// Box `b` of `face` against the trims, where the spans `met` (the ones
/// not separated from it) miss its closed box exactly (module docs,
/// "Refining runs"): `Some(true)` core, `Some(false)` outside, by the
/// trims' winding about its centre; `None` where a span meets it.
pub(crate) fn clear_class(
    face: &FaceUv<'_>,
    b: &BoxKey,
    met: &[SpanRef],
) -> Result<Option<bool>, CountError> {
    let r = rect(b);
    for s in met {
        let t = face
            .trims()
            .iter()
            .find(|t| t.coedge == s.coedge)
            .ok_or(CountError::Edge { coedge: s.coedge })?;
        let sp = t
            .spans
            .get(s.span as usize)
            .ok_or(CountError::Edge { coedge: s.coedge })?;
        if span_meets(sp, &r) {
            return Ok(None);
        }
    }
    let two = Rational::from(2u32);
    let c = [0, 1].map(|k| (&r[k][0] + &r[k][1]) / &two);
    let trims = face.trims().iter().map(|t| (t.spans, t.reversed));
    match winding_of(trims, &c) {
        Ok(0) => Ok(Some(false)),
        Ok(w) if w == face.inside_winding() => Ok(Some(true)),
        Ok(w) => Err(CountError::Box(BoxError::Winding(w))),
        // The centre on a trim: some span meets the box after all.
        Err(_) => Ok(None),
    }
}

/// The hits of a face as the counting reads them: each with its
/// target (the incidence resolved to the point's foot) and the face's
/// edges whose near radius holds it (module docs, "The lobe fallback").
fn hit_targets<R: HeightReads>(
    reads: &R,
    q: Point,
    flat: &Flat<'_, '_>,
) -> Result<Vec<HitTarget>, CountError> {
    let f = &flat.face;
    let missing = |g: EdgeId| CountError::Key(KeyError::Missing(RecordKey::Ray(RayKey { q, g })));
    let Some(hits) = flat.cell.hits.get() else {
        return Ok(Vec::new());
    };
    let hits = hits.as_deref().map_err(|e| flat.error(*e))?;
    let mut out = Vec::with_capacity(hits.len());
    for h in hits {
        let inc = match h.inc {
            None => None,
            Some(inc) => {
                let feet = reads.feet(q, inc.edge).ok_or(missing(inc.edge))?;
                let along = along_of(reads, q.operand.other(), inc.edge);
                let k = feet
                    .in_order(along)
                    .find(|&k| {
                        feet.get(k)
                            .is_some_and(|ft| foot_meets(ft, inc.span, inc.t))
                    })
                    .ok_or(CountError::NoHit {
                        point: q,
                        face: f.face,
                    })?;
                Some(Incidence::Edge(inc.edge, k))
            }
        };
        let mut near: SmallVec<[(EdgeId, FootId); 2]> = SmallVec::new();
        let mut edges: SmallVec<[EdgeId; 4]> = f.trims.iter().map(|t| t.edge).collect();
        edges.sort_unstable();
        edges.dedup();
        for g in edges {
            if !h.near_edge(f, g) {
                continue;
            }
            let feet = reads.feet(q, g).ok_or(missing(g))?;
            let along = along_of(reads, q.operand.other(), g);
            // A ribbon hit's own foot where one meets it, else (and for a
            // tile hit) the first in geometric order.
            let own = match &h.at {
                HitAt::Ribbon { trim, span, tau } if f.trims[*trim].edge == g => {
                    let t = &f.trims[*trim];
                    let at = edge_t(t, *span, tau);
                    let s = t.map[*span].span;
                    feet.in_order(along)
                        .find(|&k| feet.get(k).is_some_and(|ft| foot_meets(ft, s, at)))
                }
                _ => None,
            };
            if let Some(k) = own.or_else(|| feet.in_order(along).next()) {
                near.push((g, k));
            }
        }
        out.push(HitTarget {
            target: Target::Hit {
                f: f.face,
                sheet: h.sheet,
                i: h.i,
                inc,
            },
            height: h.height,
            facing: h.facing,
            near,
        });
    }
    Ok(out)
}

/// The way edge `g` of `of`'s stored parameter runs from its lesser end.
fn along_of<O: EntityOrder>(order: &O, of: Operand, g: EdgeId) -> Along {
    let [s, e] = order.ends(of, g);
    Along::of(&order.vert_key(of, s), &order.vert_key(of, e))
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
    let cell = HitCell::default();
    pieces_within(
        fq,
        sides,
        reads,
        Allowance::FREE,
        &cell,
        &mut Spent::default(),
    )
}

/// The pieces of `fq.face` against `fq.point` (module docs) at the
/// first level, then at each next one while the level is unsettled,
/// each level's patches lifted exactly from `fq.patches`;
/// every piece tested charged against `charge`, and the column's hits
/// (found once, for every level) at their own row. With the pieces, or
/// the error, the work ([`LevelWork::evals`] per level, [`Cost::hits`])
/// for the pass to charge ([`Charge::spent`]). Unsettled at the last
/// level, or past the share: [`CountError::OutOfBudget`] (module docs).
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
        cell: HitCell::default(),
        hits: 0,
    };
    let r = ladder(&mut at, 0, &mut cost).expect("the first level runs");
    cost.hits = at.hits;
    let r = match r {
        Err(CountError::Unsettled) => Err(CountError::OutOfBudget {
            cause: Exhausted::Precision,
            point: fq.point,
            face: fq.face.tile_boxes().next().expect("tiles").face(),
            at: None,
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
    /// The column's hits, for every level.
    cell: HitCell,
    /// Their searches charged.
    hits: u32,
}

/// What one level of a count tested ([`pieces_within`]).
#[derive(Clone, Copy, Debug, Default)]
struct Spent {
    pieces: u64,
    hits: u32,
}

impl<R: HeightReads> AtLevel for CountAt<'_, '_, '_, '_, R> {
    type Out = Result<FacePieces, CountError>;

    fn at<S: RayLevel>(&mut self, like: &S, level: usize, work: &mut LevelWork) -> Self::Out {
        let unit = self.charge.units[level];
        let allow = Allowance {
            unit,
            left: self.charge.share.saturating_sub(self.spent),
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
            nets: f.nets,
            motion: f.motion,
        };
        let mut done = Spent::default();
        let r = pieces_within(&fq, self.sides, self.reads, allow, &self.cell, &mut done);
        work.evals = work
            .evals
            .saturating_add(u32::try_from(done.pieces).unwrap_or(u32::MAX));
        self.hits = self.hits.saturating_add(done.hits);
        self.spent = self.spent.saturating_add(
            done.pieces
                .saturating_mul(unit)
                .saturating_add(hit_units(done.hits)),
        );
        match r {
            Err(CountError::OutOfBudget {
                cause: Exhausted::Work,
                point,
                face,
                at,
            }) if level > 0 => Err(CountError::OutOfBudget {
                cause: Exhausted::Precision,
                point,
                face,
                at,
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

/// [`face_pieces`], spending at most what `allow` gives, the column's
/// hits found once in `cell`; `done` what it tested, also on failure.
fn pieces_within<S: Scalar, R: HeightReads>(
    fq: &FaceQuery<'_, '_, S>,
    sides: &Sides,
    reads: &R,
    allow: Allowance,
    cell: &HitCell,
    done: &mut Spent,
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
    let flat = match fq.nets {
        None => None,
        Some(nets) => Some(Flat {
            face: hit_face(fq, nets, reads)?,
            point: fq.point,
            at: fq.at,
            motion: fq.motion,
            cell,
        }),
    };
    let mut ctx = Ctx {
        fq,
        sides,
        q,
        up_q,
        like,
        pieces: 0,
        hits: 0,
        allow,
        flat,
    };
    let r = rounds(&mut ctx);
    let r = match r {
        Err(e @ CountError::OutOfBudget { .. }) => Err(e),
        r => by_hits_where(&ctx, reads, r),
    };
    // The column's hits, if found, are charged after: a crossing there is
    // the count's (module docs).
    let settled = ctx.settle();
    *done = Spent {
        pieces: ctx.pieces,
        hits: ctx.hits,
    };
    match (r, settled) {
        (Err(e @ CountError::OutOfBudget { .. }), _) | (_, Err(e)) => Err(e),
        (r, Ok(())) => r,
    }
}

/// The face's pieces from the rounds' answer `r`, checked against the
/// column's hits where they were found, or by them where the boxes don't
/// settle (module docs, "By the hits").
fn by_hits_where<S, R: HeightReads>(
    ctx: &Ctx<'_, '_, '_, S>,
    reads: &R,
    r: Result<FacePieces, CountError>,
) -> Result<FacePieces, CountError> {
    let fq = ctx.fq;
    let Some(f) = &ctx.flat else {
        return r;
    };
    let mut out = match r {
        Ok(out) => out,
        // Boxes the levels don't settle: the face's hits decide it.
        Err(CountError::Unsettled) => {
            return by_hits(reads, fq.point, f, Err(CountError::Unsettled));
        }
        Err(e) => return Err(e),
    };
    out.hits = hit_targets(reads, fq.point, f)?;
    // Where the column's hits were found, the pieces' degrees sum to their
    // facings, and a near piece of non-zero degree holds one; else the
    // boxes and the hits disagree (a column within rounding of an edge,
    // the boxes' records at a level the hits are exact past) and the
    // hits decide.
    if let Some(Ok(hits)) = f.cell.hits.get() {
        let facings: Option<i64> = hits
            .iter()
            .map(|h| h.facing.map(|s| i64::from(s.as_i8())))
            .sum();
        let degrees = out.above + out.below + out.near.iter().map(|p| p.degree).sum::<i64>();
        let empty = out.near.iter().any(|p| p.degree != 0 && p.hits.is_empty());
        if empty || facings != Some(degrees) {
            return by_hits(reads, fq.point, f, Ok(out));
        }
    }
    Ok(out)
}

/// The face's count by its hits alone (module docs, "By the hits"): each
/// hit clear of `q`'s height and of every edge's near radius counted by
/// its exact height, the others read as one piece counting each by its
/// facing. `boxes` (the pieces' answer) where a hit's facing is unformed.
fn by_hits<R: HeightReads>(
    reads: &R,
    q: Point,
    flat: &Flat<'_, '_>,
    boxes: Result<FacePieces, CountError>,
) -> Result<FacePieces, CountError> {
    let all = flat.hits()?;
    // A hit on the face's boundary at `q` (on its edge or vertex) counts
    // by the side of the moved column the boxes' records read, which the
    // hits don't.
    if all.iter().any(|h| h.facing.is_none() || h.inc.is_some()) {
        return boxes;
    }
    let hits = hit_targets(reads, q, flat)?;
    let mut out = FacePieces {
        point: q,
        face: flat.face.face,
        above: 0,
        below: 0,
        near: Vec::new(),
        hits,
        unformed: false,
        pieces: boxes.as_ref().map_or(0, |p| p.pieces),
    };
    let mut near = NearPiece {
        degree: 0,
        hits: SmallVec::new(),
        each: true,
        germ: None,
    };
    for (k, h) in (0u32..).zip(&out.hits) {
        let f = h.facing.map_or(0, |s| i64::from(s.as_i8()));
        match h.height {
            Ordering::Greater if h.near.is_empty() => out.above += f,
            Ordering::Less if h.near.is_empty() => out.below += f,
            _ => {
                near.degree += f;
                near.hits.push(k);
            }
        }
    }
    if !near.hits.is_empty() {
        out.near.push(near);
    }
    Ok(out)
}

/// The face of `fq` as its hits read it, its tiles' nets `nets`.
fn hit_face<'a, S, O: EntityOrder>(
    fq: &FaceQuery<'a, '_, S>,
    nets: &'a [TileNet],
    order: &O,
) -> Result<HitFace<'a>, CountError> {
    if !fq.face.patches_fit(nets) {
        return Err(CountError::Box(BoxError::Patches));
    }
    let face = fq.face;
    let first = face
        .tile_boxes()
        .next()
        .ok_or(CountError::Box(BoxError::Grid))?;
    let mut trims = Vec::new();
    for t in face.trims() {
        if t.spans.is_empty() {
            continue;
        }
        let e = find_edge(fq.edges, t.coedge)?;
        let of = first.operand();
        trims.push(HitTrim {
            coedge: t.coedge,
            edge: e.edge,
            reversed: t.reversed,
            ends: t.ends,
            along: along_of(order, of, e.edge),
            band: t.band,
            pspans: t.spans,
            curve: e.ribbon.curve,
            map: e.ribbon.map,
        });
    }
    Ok(HitFace {
        operand: first.operand(),
        face: first.face(),
        tiles: face.tiles().clone(),
        nets,
        inside: face.inside_winding(),
        trims,
    })
}

/// The leaves refined, then the rounds until the pieces settle.
fn rounds<S: Scalar>(ctx: &mut Ctx<'_, '_, '_, S>) -> Result<FacePieces, CountError> {
    let mut leaves: Vec<Leaf> = Vec::new();
    for b in ctx.fq.face.tile_boxes() {
        ctx.refine(b, None, [0, 0], &mut leaves)?;
    }

    let mut cache = Cache::new();
    // Only rounds that refine count: a round that only joins leaves
    // takes each into the near run once (a joined leaf joins no more),
    // so such rounds are fewer than the leaves.
    let mut refined = 0;
    while refined < MAX_ROUNDS {
        leaves.sort_by_key(|l| l.b);
        match round(ctx, &mut leaves, &mut cache)? {
            Round::Settled(out) => return Ok(out),
            Round::Refined => refined += 1,
            Round::Joined => (),
        }
    }
    Err(ctx.out(Exhausted::SizeLimit))
}

/// How a round ended.
enum Round {
    /// The pieces.
    Settled(FacePieces),
    /// Some leaves were refined: the runs are built again.
    Refined,
    /// Some leaves joined a near run, none refined: the runs are built
    /// again.
    Joined,
}

/// One round over the leaves (module docs).
fn round<S: Scalar>(
    ctx: &mut Ctx<'_, '_, '_, S>,
    leaves: &mut Vec<Leaf>,
    cache: &mut Cache,
) -> Result<Round, CountError> {
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
        return Ok(Round::Refined);
    }

    let mut out = FacePieces {
        point: fq.point,
        face: fq.face.tile_boxes().next().expect("tiles").face(),
        above: 0,
        below: 0,
        near: Vec::new(),
        hits: Vec::new(),
        unformed: ctx.flat.is_none(),
        pieces: 0,
    };
    let add = |out: &mut FacePieces, h: Height, degree: i64, near: Option<NearPiece>| match h {
        Height::Above => out.above += degree,
        Height::Below => out.below += degree,
        Height::Near => out.near.push(near.expect("a near piece")),
    };

    // Core leaves, each a piece; one whose degree the level doesn't
    // certify (a side inside a tile not certified off `q_xy`, or too long
    // for its turn, a corner next to the ray) refines, as any piece does,
    // until the caps escalate.
    for (k, l) in leaves.iter().enumerate() {
        let (Owner::Core, Some(h)) = (&l.owner, l.height) else {
            continue;
        };
        let patch = ctx.patch(&l.b)?;
        // Every attempt is charged, settled or not.
        let mut tested = 0;
        let r = box_degree_counted(
            fq.face,
            &l.b,
            patch,
            &ctx.q,
            fq.point,
            ctx.sides,
            &mut tested,
        );
        ctx.charge(tested, &l.b)?;
        let d = match r {
            Ok(d) => d,
            Err(SidedError::Through(_) | SidedError::Unsettled) => {
                refine.push(k);
                continue;
            }
            Err(SidedError::Box(e)) => return Err(CountError::Box(e)),
            Err(SidedError::Need(k)) => return Err(CountError::Need(k)),
        };
        let near = if h == Height::Near {
            match ctx.near_piece(leaves, &[k], d.degree)? {
                Some(p) => Some(p),
                None => {
                    refine.push(k);
                    continue;
                }
            }
        } else {
            None
        };
        add(&mut out, h, d.degree, near);
    }

    // Runs: leaves of one owner and class, joined along shared sides.
    let class: Vec<Option<Height>> = (0..leaves.len())
        .map(|k| match forced.get(&k) {
            Some(c) => *c,
            None if leaves[k].joined => Some(Height::Near),
            None => leaves[k].height,
        })
        .collect();
    // Where `q`'s column lies at a corner (its leaves round a vertex
    // near), the near strip leaves of its curves joined to them along
    // shared sides are of the corner's piece too, so its cuts lie past
    // them, not next to `q` (module docs, "Around a vertex").
    let mut owner_of: Vec<Owner> = leaves.iter().map(|l| l.owner.clone()).collect();
    let near_corners: Vec<VertId> = {
        let mut v: Vec<VertId> = forced
            .iter()
            .filter(|(_, c)| **c == Some(Height::Near))
            .filter_map(|(&k, _)| match leaves[k].owner {
                Owner::Corner(v) => Some(v),
                _ => None,
            })
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    for v in near_corners {
        let Some(c) = fq.corners.corner(v) else {
            continue;
        };
        let mut queue: Vec<usize> = (0..leaves.len())
            .filter(|&k| owner_of[k] == Owner::Corner(v) && class[k] == Some(Height::Near))
            .collect();
        while let Some(k) = queue.pop() {
            for j in 0..leaves.len() {
                let absorbs = match &owner_of[j] {
                    Owner::Strip(cs) => cs.iter().all(|x| c.curves.contains(x)),
                    _ => false,
                };
                if absorbs && class[j] == Some(Height::Near) && share_side(&rects[k], &rects[j]) {
                    owner_of[j] = Owner::Corner(v);
                    queue.push(j);
                }
            }
        }
    }
    let mut groups: BTreeMap<(Owner, Height), Vec<usize>> = BTreeMap::new();
    for (k, o) in owner_of.iter().enumerate() {
        if *o == Owner::Core {
            continue;
        }
        if let Some(h) = class[k] {
            groups.entry((o.clone(), h)).or_default().push(k);
        }
    }
    // The runs left unsettled, with where, and whether at a side through
    // `q_xy` (refined there, never joined).
    let mut failed: Vec<(Owner, Vec<usize>, Option<UvRect>, bool)> = Vec::new();
    for ((owner, h), ks) in groups {
        let parts = components(ks.len(), |i, j| share_side(&rects[ks[i]], &rects[ks[j]]));
        for part in parts {
            let idx: Vec<usize> = part.iter().map(|&i| ks[i]).collect();
            let runs: Vec<Vec<usize>> = match run_degree(ctx, &owner, leaves, &idx, cache)? {
                // A near run whose hits don't settle it: box by box
                // (module docs, "Pieces").
                (Ok(d), _) if h == Height::Near && ctx.near_piece(leaves, &idx, d)?.is_none() => {
                    idx.iter().map(|&k| vec![k]).collect()
                }
                (Ok(_), _) => vec![idx.clone()],
                (Err(PieceError::Pinch), _) => idx.iter().map(|&k| vec![k]).collect(),
                (Err(PieceError::Unsettled), near) => {
                    failed.push((owner.clone(), idx, near, false));
                    continue;
                }
                // An outer side inside a tile not certified off `q_xy`:
                // the run refines there, as a core leaf does.
                (Err(PieceError::Through), near) => {
                    failed.push((owner.clone(), idx, near, true));
                    continue;
                }
                (Err(e), _) => return Err(e.into()),
            };
            for run in runs {
                let d = match run_degree(ctx, &owner, leaves, &run, cache)? {
                    (Ok(d), _) => d,
                    (Err(PieceError::Unsettled), near) => {
                        failed.push((owner.clone(), run, near, false));
                        continue;
                    }
                    (Err(PieceError::Through), near) => {
                        failed.push((owner.clone(), run, near, true));
                        continue;
                    }
                    (Err(PieceError::Pinch), _) => {
                        refine.extend(run.iter().copied());
                        continue;
                    }
                    (Err(e), _) => return Err(e.into()),
                };
                let near = if h == Height::Near {
                    match ctx.near_piece(leaves, &run, d)? {
                        Some(p) => Some(p),
                        None => {
                            // One box whose hits don't settle it: it
                            // refines, holding them apart.
                            refine.extend(run.iter().copied());
                            continue;
                        }
                    }
                } else {
                    None
                };
                add(&mut out, h, d, near);
            }
        }
    }
    // A run the trims miss altogether is no strip: its boxes are core or
    // outside by the trims' winding about their centres (module docs,
    // "Refining runs").
    let mut reclass: Vec<(usize, bool)> = Vec::new();
    for (_, run, _, through) in &failed {
        if *through {
            continue;
        }
        let mut classes = Vec::with_capacity(run.len());
        for &k in run {
            match clear_class(fq.face, &leaves[k].b, &leaves[k].met)? {
                Some(core) => classes.push((k, core)),
                None => break,
            }
        }
        if classes.len() == run.len() {
            reclass.extend(classes);
        }
    }
    if !reclass.is_empty() {
        reclass.sort_unstable();
        for &(k, core) in reclass.iter().rev() {
            if core {
                let l = &mut leaves[k];
                l.owner = Owner::Core;
                l.met.clear();
                l.joined = false;
            } else {
                leaves.remove(k);
            }
        }
        return Ok(Round::Refined);
    }

    // An unsettled cut between a near run and dropped leaves of the same
    // owner: those join the near run (module docs, "Runs near `q`"); any
    // other unsettled run refines there, but for the leaves joining.
    let mut join: Vec<usize> = Vec::new();
    let mut elsewhere: Vec<usize> = Vec::new();
    for (owner, run, near, through) in failed {
        let Some(r) = near else {
            elsewhere.extend(run);
            continue;
        };
        let rq = exact_rect(&r);
        if through {
            elsewhere.extend(near_leaves(&rects, &run, &rq));
            continue;
        }
        // A corner's run takes its curves' strip leaves too.
        let curves = match &owner {
            Owner::Corner(v) => fq.corners.corner(*v).map(|c| c.curves.clone()),
            _ => None,
        };
        let ours = |k: usize| {
            owner_of[k] == owner
                || match (&curves, &leaves[k].owner) {
                    (Some(c), Owner::Strip(cs)) => cs.iter().all(|x| c.contains(x)),
                    _ => false,
                }
        };
        let there: Vec<usize> = (0..leaves.len())
            .filter(|&k| ours(k) && touch(&rects[k], &rq))
            .collect();
        let near_there = there.iter().any(|&k| class[k] == Some(Height::Near));
        // Dropped there (degree 0, ribbons included: they join any run),
        // their class not fixed by a vertex they hold (joining wouldn't
        // move the cut). A height-clear leaf joins only where the run
        // holds the column's hit at `q` on the face's boundary, its leaves
        // there are refined as far as they go, and the leaf holds no hit
        // of the column, exactly (its degree, ribbons included, is then 0
        // too): one holding the column's crossing counts it by its own
        // height. So a near run whose cut next to `q` no level certifies
        // off the ray (`q` on a trim of a steep face, every box beside
        // its own clear of `q`'s height however small) grows past the
        // clear leaves round it, which refining only made smaller.
        let spent = near_leaves(&rects, &run, &rq)
            .iter()
            .any(|&k| leaves[k].splits.iter().all(|&n| n + 1 >= MAX_SUBDIV_DEPTH))
            && ctx.holds_boundary_hit(leaves, &run)?;
        let mut dropped: Vec<usize> = Vec::new();
        for &k in &there {
            if forced.contains_key(&k) {
                continue;
            }
            let clear = matches!(class[k], Some(Height::Above | Height::Below));
            if class[k].is_none() || (clear && spent && ctx.holds_no_hit(&leaves[k])?) {
                dropped.push(k);
            }
        }
        if near_there && !dropped.is_empty() {
            join.extend(dropped);
        } else {
            // A joined leaf refines only where nothing else there does: its
            // halves would be dropped again.
            let at = near_leaves(&rects, &run, &rq);
            let own: Vec<usize> = at.iter().copied().filter(|&k| !leaves[k].joined).collect();
            elsewhere.extend(if own.is_empty() { at } else { own });
        }
    }
    refine.extend(elsewhere.into_iter().filter(|k| !join.contains(k)));
    for &k in &join {
        leaves[k].joined = true;
    }
    if !refine.is_empty() {
        ctx.resplit(leaves, &refine)?;
        return Ok(Round::Refined);
    }
    if !join.is_empty() {
        return Ok(Round::Joined);
    }

    out.pieces = ctx.pieces;
    Ok(Round::Settled(out))
}

/// The runs' degrees found so far, by owner and boxes, each with where an
/// unsettled one is unsettled ([`strip_degree_near`]).
type Cache = BTreeMap<(Owner, Vec<BoxKey>), (Result<i64, PieceError>, Option<UvRect>)>;

/// The degree of the run `idx` of `owner`'s leaves, cached, with where it
/// is unsettled as far as known.
fn run_degree<S: Scalar>(
    ctx: &mut Ctx<'_, '_, '_, S>,
    owner: &Owner,
    leaves: &[Leaf],
    idx: &[usize],
    cache: &mut Cache,
) -> Result<(Result<i64, PieceError>, Option<UvRect>), CountError> {
    let boxes: Vec<BoxKey> = idx.iter().map(|&k| leaves[k].b).collect();
    let key = (owner.clone(), boxes);
    if let Some(r) = cache.get(&key) {
        return Ok(*r);
    }
    let ribbons = ctx.ribbons(owner)?;
    let fq = ctx.fq;
    // Every attempt is charged, settled or not.
    let mut tested = 0;
    let (r, near) = strip_degree_counted(
        fq.face,
        fq.patches,
        &key.1,
        &ribbons,
        &ctx.q,
        fq.point,
        ctx.sides,
        &mut tested,
    );
    ctx.charge(tested, &key.1[0])?;
    let r = (r.map(|d| d.degree), near);
    // A record not yet decided is asked for, not cached.
    if !matches!(r.0, Err(PieceError::Need(_))) {
        cache.insert(key, r);
    }
    Ok(r)
}

/// The leaves of `idx` to refine where their run is unsettled near `r`
/// (exactly, [`exact_rect`]): those whose closed box meets it, or all
/// where none does (module docs, "Refining runs").
fn near_leaves(rects: &[ExactRect], idx: &[usize], r: &ExactRect) -> Vec<usize> {
    let some: Vec<usize> = idx
        .iter()
        .copied()
        .filter(|&k| touch(&rects[k], r))
        .collect();
    if some.is_empty() { idx.to_vec() } else { some }
}

impl FacePieces {
    /// The degrees of all the pieces summed: the shadow winding of the
    /// face's counting surface about `q` ([`ray_degree`]).
    pub fn degree(&self) -> i64 {
        self.above + self.below + self.near.iter().map(|p| p.degree).sum::<i64>()
    }

    /// The anchor the hits' records are read at.
    fn anchor(&self) -> Anchor {
        Anchor::Face(self.point.operand.other(), self.face)
    }

    /// The height key hit `k` reads (module docs, "Heights"), by the edge
    /// rule where it lies near an edge of `lobe` (module docs, "The lobe
    /// fallback").
    fn hit_key<R: HeightReads>(
        &self,
        reads: &R,
        k: u32,
        lobe: &[EdgeId],
    ) -> Result<Oriented<HKey>, CountError> {
        let h = &self.hits[k as usize];
        if let Some(&(g, foot)) = h.near.iter().find(|(g, _)| lobe.contains(g)) {
            return Ok(edge_rule_key(reads, self.point, g, foot)?);
        }
        Ok(height_key(reads, self.point, h.target)?)
    }

    /// The hits near piece `p` reads, each with the weight its record's
    /// side counts by: its facing where the piece counts each hit, else
    /// one hit (a germ hit first) by the piece's degree; none for a piece
    /// of degree 0 counting by one hit.
    fn piece_reads(&self, p: &NearPiece) -> Result<Vec<(u32, i64)>, CountError> {
        if p.each {
            let at_q = |k: u32| {
                matches!(
                    self.hits[k as usize].target,
                    Target::Hit { inc: Some(_), .. }
                )
            };
            let mut out: Vec<(u32, i64)> = p
                .hits
                .iter()
                .filter(|&&k| p.germ.is_none() || !at_q(k))
                .map(|&k| {
                    let f = self.hits[k as usize]
                        .facing
                        .map_or(0, |s| i64::from(s.as_i8()));
                    (k, f)
                })
                .collect();
            if let Some(g) = p.germ {
                let rest = p.degree - out.iter().map(|(_, f)| f).sum::<i64>();
                if rest != 0 {
                    out.push((g, rest));
                }
            }
            return Ok(out);
        }
        if p.degree == 0 {
            return Ok(Vec::new());
        }
        if self.unformed {
            return Err(CountError::HitsUnformed {
                point: self.point,
                face: self.face,
            });
        }
        // The piece's hits are at one side of the point, or all at it: a
        // hit on a vertex or edge of the face there reads its germ key
        // first, else the first hit.
        let first = p
            .hits
            .iter()
            .copied()
            .find(|&k| {
                matches!(
                    self.hits[k as usize].target,
                    Target::Hit { inc: Some(_), .. }
                )
            })
            .or(p.hits.first().copied());
        match first {
            Some(k) => Ok(vec![(k, p.degree)]),
            None => Err(CountError::NoHit {
                point: self.point,
                face: self.face,
            }),
        }
    }

    /// The height records the count reads (module docs): its near pieces'
    /// hits', those near an edge of `lobe` by the edge rule.
    pub fn wanted<R: HeightReads>(
        &self,
        reads: &R,
        lobe: &[EdgeId],
    ) -> Result<Vec<HeightKey>, CountError> {
        let mut out = Vec::new();
        for p in &self.near {
            for (k, _) in self.piece_reads(p)? {
                let h = self.hit_key(reads, k, lobe)?;
                let k = HeightKey {
                    h: h.key,
                    anchor: self.anchor(),
                };
                if !out.contains(&k) {
                    out.push(k);
                }
            }
        }
        Ok(out)
    }

    /// `s02(q, f)` for a point of `A`, `s20(q, f)` for one of `B` (module
    /// docs), the hits' records from `heights`, those near an edge of
    /// `lobe` by the edge rule.
    pub fn count<R: HeightReads>(
        &self,
        reads: &R,
        heights: &Heights,
        lobe: &[EdgeId],
    ) -> Result<i64, CountError> {
        let a = self.point.operand == Operand::A;
        let mut s = if a { self.above } else { -self.below };
        for p in &self.near {
            for (k, weight) in self.piece_reads(p)? {
                let h = self.hit_key(reads, k, lobe)?;
                let rec =
                    heights
                        .get(h, self.anchor())
                        .ok_or(CountError::NeedHeight(HeightKey {
                            h: h.key,
                            anchor: self.anchor(),
                        }))?;
                s += if rec.fold {
                    i64::from(rec.above.as_i8())
                } else {
                    match (a, rec.above) {
                        (true, Sign::Pos) => weight,
                        (false, Sign::Neg) => -weight,
                        _ => 0,
                    }
                };
            }
        }
        Ok(s)
    }

    /// Whether some near piece of non-zero degree holds a hit near edge
    /// `g` (the lobe fallback would re-read it).
    pub fn near_edge(&self, g: EdgeId) -> bool {
        self.near.iter().filter(|p| p.degree != 0).any(|p| {
            p.hits
                .iter()
                .any(|&k| self.hits[k as usize].near.iter().any(|(e, _)| *e == g))
        })
    }
}

#[cfg(test)]
mod tests;
