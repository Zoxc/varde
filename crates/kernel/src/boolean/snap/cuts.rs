//! The zone's cut-point check: once a snapped zone is decided, its trim
//! boolean on the target must realise what the zone's records count, per
//! piece of every curve between its cut points, and the rim's corner
//! records must be the ones its ties give.
//!
//! **Input** ([`CutInput`]). The zone's **curves** ([`ZoneCurve`]): the
//! edges on the target in the zone, the target's own and the rewrite's
//! ([`Curve`]). Each comes cut at its **cut points** ([`Cut`]), in order
//! along it: its ends and the zone's vertices on it, and the cut points
//! of its crossing records' clusters with the other zone curves (a
//! cluster of the pair's crossing record, or a root cluster where the two
//! lie on one curve). Each **piece** between consecutive cut points
//! ([`CurvePiece`]) carries its **record count** ([`Count`]): the label
//! left of it minus the label right of it, along the curve, as its
//! records count it; and the pieces of the trim boolean it is realised
//! as ([`TrimAt`]). The trim boolean is given per face of the zone as
//! realised ([`Coincident`]) with the **label** of each of its cells (the
//! regions of its arrangement): the other operand's winding there, from
//! the records, or none for a region that is no cell of the zone (outside
//! the face) ([`CutFace`]). And the zone's rim records ([`RimRecords`]).
//! Which piece is realised where, and the counts and labels, are the
//! caller's, from the records; nothing here reads a position.
//!
//! **Counts.** A piece's count is its records' ([`Count::Records`]), or,
//! for a piece of a rim edge against a target face beside it, the sum of
//! the rim edge's corner records at the piece's ends ([`Count::Rim`]):
//! so a rim edge in one piece counts its crossings with that face, the
//! sum of all its corner records ([`RimRecords::x21`]), its junction and
//! bracket-end records included.
//!
//! **The check.**
//! - **Cells.** Every edge of a face's arrangement that holds a piece of
//!   a zone curve (a bundle: the pieces on one line piece, or one
//!   transversal arc) has its two cells, where both are labelled, differ
//!   by the sum of its members' counts, each turned to the bundle's way.
//!   Each of its members must be a piece of a zone curve.
//! - **Corner records.** A rim edge whose gap sign disagrees with the
//!   tie side has one corner record at each of its ends that is a tie
//!   (a branch vertex, a rim junction, a bracket end, a vertex on a
//!   target edge), of degree its sign at its start along the rim and the
//!   opposite at its end; one that agrees has none.
//! - **Branch vertices.** A branch vertex's record is the one corner
//!   record at it on its surface, on the rim edge it names, of its
//!   degree; one naming no rim edge has none.
//!
//! **Output.** Pass, or the cluster's failure ([`CutFailure`]): the
//! failing piece first in key order (its curve's key, then its place
//! along the curve; a bundle's failure is its least member's, a rim
//! vertex's the piece of the rim edge ending there), and what failed.
//! Its escalation signal ([`CutFailure::signal`]) names the rim pieces
//! along box sides (the level-set part) whose counts it reads; a failure
//! at a corner record, a branch vertex or a piece off those names only
//! pinned elements. It locates the piece by its curve and what bounds it
//! (the vertices, or the curves whose crossings cut it), never by a piece
//! number ([`Locator`]).
//!
//! **Order.** Curves by key, pieces along them; bundles by face, then
//! bundle; rim edges and branch vertices in the rim records' order. The
//! outcome reads no input order.
//!
//! **Work.** Charged on the operation's budget in passes, at the caller's
//! rung: the curves (an item each, a unit per piece and per trim piece),
//! the faces (an item each, a unit per arrangement edge), the bundles (an
//! item each, a unit per member), the rim (an item per rim edge record,
//! a unit per corner record; an item per branch vertex, a unit per rim
//! edge record).

use std::collections::{BTreeMap, BTreeSet};

use varde_decide::ids::{EdgeId, FaceId};
use varde_decide::records::keys::{Operand, XClusterIx};

use super::decide::{RimEnd, RimRecord, RimRecords};
use super::escalate::{Check, Signal};
use super::rewrite::{SnapEdge, SnapVert};
use crate::boolean::arrange::coincident::Coincident;
use crate::infra::budget::{Halt, Ledger, Pass, Step, Stop, Work};
use crate::infra::error::KernelError;
use crate::topo::arrange2d::NONE;

/// A zone curve: an edge on the target in the zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Curve {
    /// An edge of the rewrite: a rim, snapped or cut piece.
    Snap(SnapEdge),
    /// An operand's edge the rewrite doesn't hold: the target's.
    Operand(Operand, EdgeId),
}

/// A cut point of a zone curve (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Cut {
    /// A vertex: an end of the curve, or a vertex of the zone on it.
    Vert(SnapVert),
    /// The cut point of a cluster of the crossing record with this curve.
    X {
        /// The other curve.
        with: Curve,
        /// The cluster.
        cluster: XClusterIx,
    },
    /// The cut point of a root cluster with this curve, the two on one
    /// curve.
    Root {
        /// The other curve.
        with: Curve,
        /// The root cluster.
        cluster: XClusterIx,
    },
}

/// A piece of the trim boolean a curve piece is realised as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrimAt {
    /// The face, by its position in [`CutInput::faces`].
    pub face: u32,
    /// The piece, by its index in that face's [`Coincident::pieces`].
    pub piece: u32,
    /// Whether the curve runs the piece's way.
    pub forward: bool,
}

/// A curve piece's record count (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    /// This count, from its records.
    Records(i64),
    /// A piece of a rim edge: its corner records at the piece's ends
    /// against this target face.
    Rim {
        /// The target face.
        target: FaceId,
    },
}

/// A piece of a zone curve between consecutive cut points (module docs).
#[derive(Clone, Copy, Debug)]
pub struct CurvePiece<'a> {
    /// The cut point it starts at.
    pub from: Cut,
    /// The cut point it ends at.
    pub to: Cut,
    /// The pieces of the trim boolean it is realised as; none where it
    /// has no length.
    pub trim: &'a [TrimAt],
    /// Its record count.
    pub count: Count,
}

/// A zone curve and its pieces in order along it, the first starting at
/// a vertex, the last ending at one.
#[derive(Clone, Copy, Debug)]
pub struct ZoneCurve<'a> {
    /// The curve.
    pub curve: Curve,
    /// Its pieces.
    pub pieces: &'a [CurvePiece<'a>],
}

/// A face of the zone's trim boolean and its cells' labels.
#[derive(Clone, Copy, Debug)]
pub struct CutFace<'a> {
    /// The face as realised.
    pub realised: &'a Coincident,
    /// Per region of its arrangement, its label; `None` where the region
    /// is no cell of the zone.
    pub labels: &'a [Option<i64>],
}

/// What [`check`] reads (module docs).
#[derive(Clone, Copy, Debug)]
pub struct CutInput<'a> {
    /// The zone's rim records.
    pub rim: &'a RimRecords,
    /// The faces of the trim boolean.
    pub faces: &'a [CutFace<'a>],
    /// The zone curves.
    pub curves: &'a [ZoneCurve<'a>],
    /// The ladder's rung the check runs at.
    pub rung: u8,
}

/// A piece of a zone curve by key: its curve, then its place along it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PieceAt {
    /// The curve.
    pub curve: Curve,
    /// Its place along the curve, from 0.
    pub index: u32,
}

/// What failed (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Fault {
    /// The cells beside an edge of the trim boolean differ by `cells`;
    /// its members' records count `records`.
    Count { cells: i64, records: i64 },
    /// The corner records of a rim edge at this vertex: missing, extra or
    /// of the wrong degree.
    Corner { vert: SnapVert },
    /// The corner records at this branch vertex aren't its one record of
    /// its degree.
    Branch { vert: SnapVert, degree: i8 },
}

/// What bounds a failing piece: a vertex, or the crossing with a curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mark {
    /// A vertex.
    Vert(SnapVert),
    /// A crossing, or a root, with this curve.
    Cross(Curve),
}

/// A failing piece located by its curve and what bounds it at its start
/// and its end (a failure at a vertex: that vertex twice); no piece
/// number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Locator {
    /// The curve.
    pub curve: Curve,
    /// Its bounds.
    pub at: [Mark; 2],
}

/// The cluster's check failing (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct CutFailure<C> {
    /// The cluster.
    pub cluster: C,
    /// The failing piece, first in key order.
    pub piece: PieceAt,
    /// What failed there.
    pub fault: Fault,
    /// The failing piece located.
    pub locator: Locator,
    /// The rim pieces along box sides whose counts the failure read, by
    /// their positions in the cluster's rim, ascending.
    pub level_set: Vec<u32>,
}

impl<C> CutFailure<C> {
    /// The escalation's signal for it.
    pub fn signal(&self) -> Signal<Locator> {
        Signal {
            check: Check::CutPoints,
            level_set: self.level_set.clone(),
            locator: self.locator,
        }
    }
}

/// Why the check can't run: input it doesn't take.
#[derive(Clone, Debug, PartialEq)]
pub enum CutError {
    /// More faces than `u32` ids hold.
    TooMany,
    /// This curve is given twice, has no pieces, its pieces don't join up,
    /// or it doesn't start and end at a vertex.
    Curve(Curve),
    /// A piece of this curve names a face or a trim piece not there, a trim
    /// piece another curve piece already names, or counts a rim edge it
    /// isn't, against a face it has no record against.
    Piece(PieceAt),
    /// This face's labels aren't one per region, or one of its bundles has
    /// no arrangement edge.
    Face(u32),
    /// A member of this bundle of this face is no zone curve's piece.
    Bundle { face: u32, bundle: u32 },
    /// This rim edge has records but isn't a zone curve.
    Rim(SnapEdge),
    /// A count or a label difference at this piece leaves `i64`.
    Overflow(PieceAt),
    /// The budget ran out ([`KernelError::OutOfBudget`]).
    OutOfBudget(KernelError),
}

/// Runs a pass of `f` over `items` at `rung`.
fn run<T, R, F>(
    work: &mut Work,
    stage: &'static str,
    rung: u8,
    items: &[T],
    f: F,
) -> Result<Vec<R>, CutError>
where
    T: Sync,
    R: Send,
    F: Fn(&T, &mut Ledger<'_>) -> Result<R, Stop<CutError>> + Sync + Send,
{
    work.pass(Pass { stage, rung }, items, f)
        .map_err(|halt| match halt {
            Halt::Failed { error, .. } => error,
            Halt::OutOfBudget(e) => CutError::OutOfBudget(e),
        })
}

/// Charges `units` of plain work.
fn units(ledger: &mut Ledger<'_>, units: usize) -> Result<(), Stop<CutError>> {
    ledger
        .charge(Step::Work, u64::try_from(units).unwrap_or(u64::MAX))
        .map_err(Stop::from)
}

fn fail<T>(e: CutError) -> Result<T, Stop<CutError>> {
    Err(Stop::Failed(e))
}

/// What bounds a piece at a cut point.
fn mark(c: Cut) -> Mark {
    match c {
        Cut::Vert(v) => Mark::Vert(v),
        Cut::X { with, .. } | Cut::Root { with, .. } => Mark::Cross(with),
    }
}

/// Whether two face lists share a face.
fn overlap(a: &[FaceId], b: &[FaceId]) -> bool {
    a.iter().any(|f| b.contains(f))
}

/// The rim piece along box sides a curve is, if it is one.
fn level_set(c: Curve) -> Option<u32> {
    match c {
        Curve::Snap(SnapEdge::Rim { piece, .. }) => Some(piece),
        _ => None,
    }
}

/// A failure found: where, what, located, the level-set pieces it read.
type Found = (PieceAt, Fault, Locator, Vec<u32>);

/// A curve piece's place: the curve's position in key order, the piece's
/// index along it.
type Place = (usize, usize);

/// The zone curves in key order, with what the check reads of them.
struct Curves<'a> {
    input: &'a CutInput<'a>,
    /// Positions in `input.curves`, by key.
    order: Vec<usize>,
    /// Per curve in key order, its pieces' counts.
    counts: Vec<Vec<i64>>,
}

impl Curves<'_> {
    fn curve(&self, k: usize) -> &ZoneCurve<'_> {
        &self.input.curves[self.order[k]]
    }

    fn piece(&self, (k, j): Place) -> &CurvePiece<'_> {
        &self.curve(k).pieces[j]
    }

    fn at(&self, (k, j): Place) -> PieceAt {
        PieceAt {
            curve: self.curve(k).curve,
            // Fewer pieces than `u32::MAX`, checked on the curve pass.
            index: j as u32,
        }
    }

    /// The position in key order of the curve `c`.
    fn find(&self, c: Curve) -> Option<usize> {
        self.order
            .binary_search_by(|&i| self.input.curves[i].curve.cmp(&c))
            .ok()
    }

    /// The curve's two end vertices (there are, by the curve pass).
    fn ends(&self, k: usize) -> Option<[SnapVert; 2]> {
        let ps = self.curve(k).pieces;
        match (ps.first()?.from, ps.last()?.to) {
            (Cut::Vert(a), Cut::Vert(b)) => Some([a, b]),
            _ => None,
        }
    }

    /// The first piece of curve `k` starting or ending at `v` (its first
    /// piece if none does), located at `v`.
    fn at_vert(&self, k: usize, v: SnapVert) -> (PieceAt, Locator) {
        let ps = self.curve(k).pieces;
        let j = ps
            .iter()
            .position(|p| p.from == Cut::Vert(v) || p.to == Cut::Vert(v))
            .unwrap_or(0);
        let locator = Locator {
            curve: self.curve(k).curve,
            at: [Mark::Vert(v); 2],
        };
        (self.at((k, j)), locator)
    }
}

/// The count of a piece `p` of curve `c` (module docs).
fn count_of(rim: &RimRecords, c: Curve, p: &CurvePiece<'_>) -> Option<i64> {
    match p.count {
        Count::Records(n) => Some(n),
        Count::Rim { target } => {
            let Curve::Snap(e) = c else { return None };
            let rec = rim.edge(e, target)?;
            let sum: i64 = rec
                .corners
                .iter()
                .filter(|r| Cut::Vert(r.vert) == p.from || Cut::Vert(r.vert) == p.to)
                .map(|r| i64::from(r.degree))
                .sum();
            Some(sum)
        }
    }
}

/// Checks the zone of `cluster` (module docs), charged to `work`: `None`
/// when it passes.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the boolean's snap calls it as it lands")
)]
pub(crate) fn check<C: Clone>(
    cluster: &C,
    input: &CutInput<'_>,
    work: &mut Work,
) -> Result<Option<CutFailure<C>>, CutError> {
    let rung = input.rung;
    let mut order: Vec<usize> = (0..input.curves.len()).collect();
    order.sort_by(|&a, &b| input.curves[a].curve.cmp(&input.curves[b].curve));
    for w in order.windows(2) {
        let c = input.curves[w[0]].curve;
        if c == input.curves[w[1]].curve {
            return Err(CutError::Curve(c));
        }
    }

    // The curves: joined up, from a vertex to a vertex, and counted.
    let counts = run(work, "zone cut points: curves", rung, &order, |&i, l| {
        let zc = &input.curves[i];
        let trims: usize = zc.pieces.iter().map(|p| p.trim.len()).sum();
        units(l, 1 + zc.pieces.len() + trims)?;
        let (Some(first), Some(last)) = (zc.pieces.first(), zc.pieces.last()) else {
            return fail(CutError::Curve(zc.curve));
        };
        let joined = zc.pieces.windows(2).all(|w| w[0].to == w[1].from);
        let ends = matches!(first.from, Cut::Vert(_)) && matches!(last.to, Cut::Vert(_));
        if !(joined && ends) || u32::try_from(zc.pieces.len()).is_err() {
            return fail(CutError::Curve(zc.curve));
        }
        zc.pieces
            .iter()
            .enumerate()
            .map(|(j, p)| {
                count_of(input.rim, zc.curve, p).ok_or(Stop::Failed(CutError::Piece(PieceAt {
                    curve: zc.curve,
                    index: j as u32,
                })))
            })
            .collect::<Result<Vec<i64>, _>>()
    })?;
    let curves = Curves {
        input,
        order,
        counts,
    };

    // Where each trim piece is named (charged on the curve pass).
    let mut named: BTreeMap<(u32, u32), (Place, bool)> = BTreeMap::new();
    for k in 0..curves.order.len() {
        for (j, p) in curves.curve(k).pieces.iter().enumerate() {
            for t in p.trim {
                let there = input
                    .faces
                    .get(t.face as usize)
                    .is_some_and(|f| (t.piece as usize) < f.realised.pieces.len());
                if !there
                    || named
                        .insert((t.face, t.piece), ((k, j), t.forward))
                        .is_some()
                {
                    return Err(CutError::Piece(curves.at((k, j))));
                }
            }
        }
    }

    // Each face's first arrangement edge per bundle.
    let face_ids: Vec<u32> =
        (0..u32::try_from(input.faces.len()).map_err(|_| CutError::TooMany)?).collect();
    let first_span = run(work, "zone cut points: faces", rung, &face_ids, |&f, l| {
        let face = &input.faces[f as usize];
        let c = face.realised;
        units(l, 1 + c.span_bundle.len())?;
        if face.labels.len() != c.arrangement.regions().len() {
            return fail(CutError::Face(f));
        }
        let mut first = vec![NONE; c.bundles.len()];
        for (e, &b) in c.span_bundle.iter().enumerate() {
            let Ok(e) = u32::try_from(e) else {
                return fail(CutError::Face(f));
            };
            if let Some(x) = first.get_mut(b as usize)
                && *x == NONE
            {
                *x = e;
            }
        }
        Ok(first)
    })?;

    // The cells beside each bundle holding a zone curve's piece.
    let bundles: Vec<(u32, u32)> = named
        .keys()
        .map(|&(f, p)| {
            (
                f,
                input.faces[f as usize].realised.pieces[p as usize].bundle,
            )
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let cells = run(
        work,
        "zone cut points: cells",
        rung,
        &bundles,
        |&(f, b), l| {
            let face = &input.faces[f as usize];
            let c = face.realised;
            let Some(bundle) = c.bundles.get(b as usize) else {
                return fail(CutError::Face(f));
            };
            units(l, 1 + bundle.members.len())?;
            let mut records: i64 = 0;
            let mut members: Vec<Place> = Vec::with_capacity(bundle.members.len());
            for &m in &bundle.members {
                let Some(&(place, forward)) = named.get(&(f, m)) else {
                    return fail(CutError::Bundle { face: f, bundle: b });
                };
                let n = curves.counts[place.0][place.1];
                let turned = if forward == c.pieces[m as usize].forward {
                    Some(n)
                } else {
                    n.checked_neg()
                };
                records = turned
                    .and_then(|n| records.checked_add(n))
                    .ok_or(Stop::Failed(CutError::Overflow(curves.at(place))))?;
                members.push(place);
            }
            let e = first_span[f as usize][b as usize];
            if e == NONE {
                return fail(CutError::Face(f));
            }
            let a = &c.arrangement;
            let region = |h: u32| a.cycle_region(a.cycle_of(h));
            let (left, right) = (region(2 * e), region(2 * e + 1));
            let (Some(lv), Some(rv)) = (face.labels[left as usize], face.labels[right as usize])
            else {
                return Ok(None);
            };
            let least = *members.iter().min().unwrap_or(&(0, 0));
            let cells = lv
                .checked_sub(rv)
                .ok_or(Stop::Failed(CutError::Overflow(curves.at(least))))?;
            if cells == records {
                return Ok(None);
            }
            let p = curves.piece(least);
            let locator = Locator {
                curve: curves.at(least).curve,
                at: [mark(p.from), mark(p.to)],
            };
            let mut level: Vec<u32> = members
                .iter()
                .filter_map(|&(k, _)| level_set(curves.curve(k).curve))
                .collect();
            level.sort_unstable();
            level.dedup();
            Ok(Some((
                curves.at(least),
                Fault::Count { cells, records },
                locator,
                level,
            )))
        },
    )?;

    // The rim's corner records and branch vertices.
    let rim = input.rim;
    let items: Vec<RimItem> = (0..rim.edges.len())
        .map(RimItem::Edge)
        .chain((0..rim.branches.len()).map(RimItem::Branch))
        .collect();
    let rims = run(
        work,
        "zone cut points: rim",
        rung,
        &items,
        |item, l| match *item {
            RimItem::Edge(i) => corners(&curves, rim, &rim.edges[i], l),
            RimItem::Branch(i) => branch(&curves, rim, i, l),
        },
    )?;

    let found = cells
        .into_iter()
        .flatten()
        .chain(rims.into_iter().flatten())
        .min_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    Ok(found.map(|(piece, fault, locator, level_set)| CutFailure {
        cluster: cluster.clone(),
        piece,
        fault,
        locator,
        level_set,
    }))
}

/// An item of the rim pass.
enum RimItem {
    /// A rim edge's record, by position in [`RimRecords::edges`].
    Edge(usize),
    /// A branch vertex, by position in [`RimRecords::branches`].
    Branch(usize),
}

/// The first failure of a rim edge record's corner records (module docs).
fn corners(
    curves: &Curves<'_>,
    rim: &RimRecords,
    rec: &RimRecord,
    l: &mut Ledger<'_>,
) -> Result<Option<Found>, Stop<CutError>> {
    units(l, 1 + rec.corners.len())?;
    let Some(k) = curves.find(Curve::Snap(rec.edge)) else {
        return fail(CutError::Rim(rec.edge));
    };
    let Some(ends) = curves.ends(k) else {
        return fail(CutError::Curve(Curve::Snap(rec.edge)));
    };
    let tie = |v: SnapVert| {
        rim.verts
            .iter()
            .any(|r| r.vert == v && r.tie && overlap(&r.targets, &rec.targets))
    };
    let disagrees = rec.sign != rim.side;
    let s = rec.sign.as_i8();
    let mut bad: Vec<SnapVert> = Vec::new();
    for &v in &ends {
        let want = if disagrees {
            ends.iter().filter(|&&w| w == v && tie(w)).count()
        } else {
            0
        };
        let have = rec.corners.iter().filter(|c| c.vert == v).count();
        if have != want {
            bad.push(v);
        }
    }
    for c in &rec.corners {
        let degree = match c.end {
            RimEnd::Start => s,
            RimEnd::End => -s,
        };
        if !ends.contains(&c.vert) || c.degree != degree {
            bad.push(c.vert);
        }
    }
    Ok(bad
        .into_iter()
        .map(|v| {
            let (at, loc) = curves.at_vert(k, v);
            (at, Fault::Corner { vert: v }, loc, Vec::new())
        })
        .min_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1))))
}

/// The failure of branch vertex `i`'s record, if it fails (module docs).
fn branch(
    curves: &Curves<'_>,
    rim: &RimRecords,
    i: usize,
    l: &mut Ledger<'_>,
) -> Result<Option<Found>, Stop<CutError>> {
    let b = &rim.branches[i];
    units(l, 1 + rim.edges.len())?;
    let mut at: Vec<(SnapEdge, i8)> = rim
        .edges
        .iter()
        .filter(|r| overlap(&r.targets, &b.targets))
        .flat_map(|r| {
            r.corners
                .iter()
                .filter(|c| c.vert == b.vert)
                .map(|c| (r.edge, c.degree))
        })
        .collect();
    at.sort_unstable();
    let ok = match b.edge {
        Some(k) => at == [(k, b.degree)],
        None => at.is_empty(),
    };
    if ok {
        return Ok(None);
    }
    // A fault has an edge: the one named, else one holding a record.
    let Some(edge) = b.edge.or(at.first().map(|x| x.0)) else {
        return Ok(None);
    };
    let Some(k) = curves.find(Curve::Snap(edge)) else {
        return fail(CutError::Rim(edge));
    };
    let (piece, loc) = curves.at_vert(k, b.vert);
    Ok(Some((
        piece,
        Fault::Branch {
            vert: b.vert,
            degree: b.degree,
        },
        loc,
        Vec::new(),
    )))
}

#[cfg(test)]
mod tests;
