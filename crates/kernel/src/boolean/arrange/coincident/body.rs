//! A face of an operand's body realised ([`realise_on`]): its coedges are
//! the lines arcs run along, its vertices those of its p-curves, and each
//! edge of the result is told what it is a piece of: the operand edges
//! it identifies, their `touch`, and whether the operation joined its
//! faces across an opposed coincidence.
//!
//! **Lines.** Every coedge of the face, loop by loop in the face's order
//! and round each loop from its first coedge, is a line, running along
//! its p-curve (the edge's way, whatever the coedge's), between the
//! vertices at its p-curve's ends. Its winding step is `+1` where the
//! face lies left of the p-curve and `−1` where it lies right: left of
//! the coedge's direction on a face not flipped, right on a flipped one,
//! the coedge running the p-curve's way unless reversed. The face's
//! inside winds 1. A coedge's line reuses its p-curve bit for bit, so
//! every arc along it is a piece of its edge on this face, with this
//! face's own p-curve: the per-face reuse, whatever p-curve the faces of
//! the other operand give the same curve. The caller may add **new
//! lines** ([`NewLine`]): arcs that other arcs coincide with, each with
//! its p-curve.
//!
//! **Vertices.** A face vertex is an operand vertex at one point of the
//! face's UV: the end control point of a coedge's p-curve. One vertex
//! seen at two points (on a seam of a closed surface, a pole's row) is
//! two face vertices, numbered in order of first sight along the lines.
//! The caller's **records** follow ([`RecPlace`]): a point of the UV, a
//! distinct vertex at a face vertex's or another record's point, on a
//! line or pinned to one.
//!
//! **Arcs** ([`ArcRec`]) run between face vertices or records, along a
//! chain of lines or transversal, as the records that made them say
//! (the T0 rule: a surface of the face across an edge, an edge-on record
//! of a coedge, a shared curve, coincident pairs; never by distance).
//! The realisation is [`coincident::realise`](super::realise)'s: splits at
//! records on a line and at pins, projected vertices where a chain's
//! joins and splits fall inside an arc's span, slits as bundles of
//! coincident pieces, zero-length edges and contracted points.
//!
//! **Sources** ([`Bundled`]). Per bundle, one edge of the result: the
//! operand edges it is a piece of, ascending ([`OperandEdge`]): a
//! coedge's edge for its line's piece, and each new line's and arc's own
//! (the operand edges the caller's records identify them with: an edge
//! of the other operand they run along). Its operand `touch` is the OR of
//! the sources' kinds
//! ([`EdgeKind::is_touch`](crate::topo::brep::EdgeKind::is_touch)), read
//! from their stores. A bundle of more than one piece is an
//! **identified** edge: its pieces
//! are one edge by record. It records that the operation **joined its
//! faces across an opposed coincidence** where one of its new lines or
//! arcs comes from a pair across an opposed T0 coincidence patch (an
//! `A` face and a `B` face on one surface or form, normals opposed; the
//! tops of boxes placed face to face), as the caller's records say
//! ([`ArcRec::opposed`]). That fact, the sources' `touch` and the faces'
//! keys are what the assembly reads to derive the edge's kind; no kind is
//! set here.
//!
//! **Order.** Lines are the coedges, then the new lines; vertices the
//! face vertices, then the records, then the lines' pin vertices
//! ([`Coincident::pins`]); arcs in input order; bundles as the
//! realisation numbers them.
//!
//! **Work.** One pass on the operation's budget, one item: a unit per
//! coedge, record, new line, arc, chain link and source; then the
//! realisation's passes.

use std::collections::BTreeMap;

use super::{Along, Arc, Coincident, CoincidentError, FaceInput, Line, PieceOf, Place};
use crate::boolean::arrange::fold::OperandEdge;
use crate::geom::pcurve::PCurve;
use crate::infra::budget::{Halt, Ledger, Pass, Step, Stop, Work};
use crate::infra::error::KernelError;
use crate::topo::arrange2d::{ArrangeError, NONE};
use crate::topo::brep::{Brep, CoedgeId, FaceId, VertId};
use varde_decide::records::keys::Operand;

/// A line of the face: one of its coedges, or a new line by index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LineRef {
    /// This coedge of the face.
    Coedge(CoedgeId),
    /// This new line.
    New(u32),
}

/// A vertex of the face: an operand vertex at a coedge's end, or a
/// record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertRef {
    /// The vertex at this coedge's p-curve's start (`end` false) or end.
    Face {
        /// The coedge.
        coedge: CoedgeId,
        /// Which end of its p-curve.
        end: bool,
    },
    /// This record.
    Rec(u32),
}

/// Where a record lies (module docs; [`Place`] on the body).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RecPlace {
    /// At this point of the face's UV.
    Uv([f64; 2]),
    /// At this vertex's point, a distinct vertex there; a record named is
    /// not placed `At` itself.
    At(VertRef),
    /// The vertex of this line at its parameter `s`, strictly inside the
    /// line's range: the record lies on the line.
    On {
        /// The line.
        line: LineRef,
        /// The parameter.
        s: f64,
    },
    /// At this line's point at `s`, strictly inside its range, without
    /// lying on it.
    Pin {
        /// The line.
        line: LineRef,
        /// The parameter.
        s: f64,
    },
}

/// A new line: an arc other arcs coincide with (module docs).
#[derive(Clone, Copy, Debug)]
pub struct NewLine<'a> {
    /// Its vertices at its p-curve's first and last knot.
    pub ends: [VertRef; 2],
    /// Its p-curve on the face.
    pub pcurve: &'a PCurve,
    /// The winding number left of it minus the one right of it.
    pub wind: i32,
    /// The operand edges the records identify it with.
    pub sources: &'a [OperandEdge],
    /// Whether it comes from a pair across an opposed T0 coincidence.
    pub opposed: bool,
}

/// A link of an arc's chain: a line and the way the chain runs along it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlongRef {
    /// The line.
    pub line: LineRef,
    /// Whether the chain runs along the line's p-curve.
    pub forward: bool,
}

/// A new arc on the face (module docs).
#[derive(Clone, Copy, Debug)]
pub struct ArcRec<'a> {
    /// Its start and end vertex.
    pub ends: [VertRef; 2],
    /// The chain of lines it runs along, in its direction; empty for a
    /// transversal arc.
    pub along: &'a [AlongRef],
    /// The winding number left of it minus the one right of it, in its
    /// direction.
    pub wind: i32,
    /// The operand edges the records identify it with.
    pub sources: &'a [OperandEdge],
    /// Whether it comes from a pair across an opposed T0 coincidence
    /// patch (module docs).
    pub opposed: bool,
}

/// A face of an operand with the operation's records and arcs on it.
#[derive(Clone, Copy, Debug)]
pub struct FaceArcs<'a> {
    /// The operand the face is of.
    pub operand: Operand,
    /// The face.
    pub face: FaceId,
    /// The records.
    pub records: &'a [RecPlace],
    /// The new lines.
    pub lines: &'a [NewLine<'a>],
    /// The arcs.
    pub arcs: &'a [ArcRec<'a>],
}

/// What a vertex of the realisation is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertOf {
    /// This operand vertex of the face at this point.
    Face {
        /// The operand vertex.
        vert: VertId,
        /// Its point, by index into [`Realised::face_uvs`].
        at: u32,
    },
    /// This record.
    Rec(u32),
    /// This pin vertex of a line ([`Coincident::pins`]).
    Pin(u32),
}

/// One edge of the result: a bundle's sources (module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bundled {
    /// The operand edges it is a piece of, ascending.
    pub sources: Vec<OperandEdge>,
    /// Whether one of them only touched (`SameForm{touch}`).
    pub touch: bool,
    /// Whether it is identified: more than one piece.
    pub identified: bool,
    /// Whether the operation joined its faces across an opposed
    /// coincidence: identified, and a new line or arc of it comes from a
    /// pair across one.
    pub opposed_join: bool,
}

/// A face realised on its body (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Realised {
    /// The realisation, its vertices numbered as [`Realised::verts`] and
    /// its lines as [`Realised::lines`].
    pub coincident: Coincident,
    /// Per vertex of the realisation, what it is.
    pub verts: Vec<VertOf>,
    /// The face vertices' points.
    pub face_uvs: Vec<[f64; 2]>,
    /// Per line of the realisation, what it is.
    pub lines: Vec<LineRef>,
    /// Per bundle, its sources.
    pub bundles: Vec<Bundled>,
}

/// Why the face can't be realised (module docs).
#[derive(Clone, Debug, PartialEq)]
pub enum FaceError {
    /// More entities than `u32` ids hold.
    TooMany,
    /// The face isn't in its operand's store.
    Face,
    /// This coedge's p-curve doesn't run over its edge's range, or its
    /// ends aren't finite.
    Coedge(CoedgeId),
    /// This vertex reference names a coedge not of the face or a record
    /// not there.
    Vert(VertRef),
    /// This line reference names a coedge not of the face or a new line
    /// not there.
    Line(LineRef),
    /// This record's place is refused: it names what isn't there, is
    /// placed `At` a record placed `At`, lies outside its line's range,
    /// or splits its line where another record does.
    Rec(u32),
    /// This new line or this coedge's line is refused by the realisation
    /// (its ends at one point, its p-curve off its ends' points).
    LineRefused(LineRef),
    /// This arc is refused: its ends, its chain, or its sources.
    Arc(u32),
    /// This new line names an operand edge not in its store.
    Source(u32),
    /// The realisation refused otherwise (bundle ids as realised).
    Realise(CoincidentError),
    /// The budget ran out, or a fixed limit was reached.
    OutOfBudget(KernelError),
}

type Res<T> = Result<T, Stop<FaceError>>;

fn fail<T>(e: FaceError) -> Res<T> {
    Err(Stop::Failed(e))
}

fn units(ledger: &mut Ledger<'_>, n: u64) -> Res<()> {
    ledger.charge(Step::Work, n).map_err(Stop::from)
}

fn id(n: usize) -> Res<u32> {
    u32::try_from(n)
        .ok()
        .filter(|&i| i != NONE)
        .ok_or(Stop::Failed(FaceError::TooMany))
}

/// The face `input.face` of `stores[input.operand]` realised with its
/// records and arcs (module docs), `stores` `A`'s then `B`'s, charged to
/// `work`.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the boolean's face arrangements call it as they land"
    )
)]
pub(crate) fn realise_on(
    stores: [&Brep; 2],
    input: &FaceArcs<'_>,
    work: &mut Work,
) -> Result<Realised, FaceError> {
    let mapped = work
        .pass(
            Pass {
                stage: "coincident arcs on a face",
                rung: 0,
            },
            &[()],
            |_, ledger| map(stores, input, ledger),
        )
        .map_err(|halt| match halt {
            Halt::Failed { error, .. } => error,
            Halt::OutOfBudget(e) => FaceError::OutOfBudget(e),
        })?
        .swap_remove(0);
    let lines: Vec<Line<'_>> = mapped
        .lines
        .iter()
        .map(|l| Line {
            ends: l.ends,
            pcurve: l.pcurve,
            wind: l.wind,
        })
        .collect();
    let arcs: Vec<Arc<'_>> = mapped
        .arcs
        .iter()
        .zip(&mapped.arc_ends)
        .zip(input.arcs)
        .map(|((along, &ends), a)| Arc {
            ends,
            along,
            wind: a.wind,
        })
        .collect();
    let face = FaceInput {
        verts: &mapped.places,
        lines: &lines,
        arcs: &arcs,
    };
    let coincident = super::realise(&face, work).map_err(|e| mapped.refused(e))?;
    let n = mapped.places.len();
    let mut verts = mapped.verts.clone();
    verts.extend((0..coincident.pins.len()).map(|k| VertOf::Pin(k as u32)));
    debug_assert_eq!(verts.len(), n + coincident.pins.len());
    let bundles = sources(stores, input, &mapped, &coincident);
    Ok(Realised {
        coincident,
        verts,
        face_uvs: mapped.face_uvs,
        lines: mapped.refs,
        bundles,
    })
}

/// A line as the realisation reads it.
struct LineIn<'a> {
    ends: [u32; 2],
    pcurve: &'a PCurve,
    wind: i32,
}

/// The input mapped onto the realisation's ids.
struct Mapped<'a> {
    /// Per vertex: face vertices, then records.
    places: Vec<Place>,
    verts: Vec<VertOf>,
    face_uvs: Vec<[f64; 2]>,
    lines: Vec<LineIn<'a>>,
    refs: Vec<LineRef>,
    /// Per arc, its chain.
    arcs: Vec<Vec<Along>>,
    arc_ends: Vec<[u32; 2]>,
    /// Per line, its operand edge (coedges) or none.
    line_edge: Vec<Option<OperandEdge>>,
}

impl Mapped<'_> {
    /// The realisation's refusal with its ids read as the input's.
    fn refused(&self, e: CoincidentError) -> FaceError {
        let line = |l: u32| self.refs.get(l as usize).copied();
        match e {
            CoincidentError::TooMany => FaceError::TooMany,
            CoincidentError::Place(v) | CoincidentError::Split(v) => {
                match self.verts.get(v as usize) {
                    Some(VertOf::Rec(r)) => FaceError::Rec(*r),
                    _ => FaceError::Realise(e),
                }
            }
            CoincidentError::Line(l) => match line(l) {
                Some(r) => FaceError::LineRefused(r),
                None => FaceError::Realise(e),
            },
            CoincidentError::Arc(a) => FaceError::Arc(a),
            CoincidentError::OutOfBudget(k) => FaceError::OutOfBudget(k),
            CoincidentError::Arrange(ArrangeError::OutOfBudget(k)) => FaceError::OutOfBudget(k),
            e => FaceError::Realise(e),
        }
    }
}

/// The input on the realisation's ids (module docs), on one ledger.
fn map<'a>(
    stores: [&'a Brep; 2],
    input: &FaceArcs<'a>,
    ledger: &mut Ledger<'_>,
) -> Res<Mapped<'a>> {
    let brep = store(stores, input.operand);
    let f = input.face;
    if f.is_none() || f.index() >= brep.face_count() {
        return fail(FaceError::Face);
    }

    // The coedges' lines and the face vertices at their ends.
    let mut places = Vec::new();
    let mut verts = Vec::new();
    let mut face_uvs = Vec::new();
    let mut at_point: BTreeMap<(u32, [u64; 2]), u32> = BTreeMap::new();
    let mut lines = Vec::new();
    let mut refs = Vec::new();
    let mut line_edge = Vec::new();
    let mut coedge_line: BTreeMap<CoedgeId, (u32, [u32; 2])> = BTreeMap::new();
    let flipped = brep.flipped(f);
    for l in brep.loops(f) {
        for c in brep.loop_coedges(l) {
            units(ledger, 1)?;
            if coedge_line.contains_key(&c) {
                // A loop that doesn't close: the realisation would refuse
                // its lines; the store's check names it.
                return fail(FaceError::Coedge(c));
            }
            let e = brep.edge(c);
            let pc = brep.coedge_pcurve(c);
            let k = pc.knots();
            let last = k.len() - 1;
            let range = brep.t_range(e);
            if k[0].to_bits() != range[0].to_bits() || k[last].to_bits() != range[1].to_bits() {
                return fail(FaceError::Coedge(c));
            }
            let mut ends = [0u32; 2];
            for (j, (&v, knot)) in brep.ends(e).iter().zip([0, last]).enumerate() {
                let Some(uv) = pc
                    .knot_point(knot)
                    .filter(|p| p.iter().all(|x| x.is_finite()))
                else {
                    return fail(FaceError::Coedge(c));
                };
                let key = (v.0, uv.map(f64::to_bits));
                ends[j] = match at_point.get(&key) {
                    Some(&i) => i,
                    None => {
                        let i = id(places.len())?;
                        at_point.insert(key, i);
                        places.push(Place::Uv(uv));
                        verts.push(VertOf::Face {
                            vert: v,
                            at: id(face_uvs.len())?,
                        });
                        face_uvs.push(uv);
                        i
                    }
                };
            }
            let li = id(lines.len())?;
            coedge_line.insert(c, (li, ends));
            let left = brep.reversed(c) == flipped;
            lines.push(LineIn {
                ends,
                pcurve: pc,
                wind: if left { 1 } else { -1 },
            });
            refs.push(LineRef::Coedge(c));
            line_edge.push(Some((input.operand, e)));
        }
    }
    let nf = places.len();
    let nr = input.records.len();
    if nf + nr >= NONE as usize {
        return fail(FaceError::TooMany);
    }
    let vert = |r: VertRef| -> Res<u32> {
        match r {
            VertRef::Face { coedge, end } => coedge_line
                .get(&coedge)
                .map(|&(_, ends)| ends[end as usize])
                .ok_or(Stop::Failed(FaceError::Vert(r))),
            VertRef::Rec(k) if (k as usize) < nr => Ok((nf + k as usize) as u32),
            VertRef::Rec(_) => fail(FaceError::Vert(r)),
        }
    };
    let new_lines = input.lines.len();
    let line = |r: LineRef| -> Res<u32> {
        match r {
            LineRef::Coedge(c) => coedge_line
                .get(&c)
                .map(|&(l, _)| l)
                .ok_or(Stop::Failed(FaceError::Line(r))),
            LineRef::New(k) if (k as usize) < new_lines => {
                Ok((coedge_line.len() + k as usize) as u32)
            }
            LineRef::New(_) => fail(FaceError::Line(r)),
        }
    };

    // The records.
    for (k, p) in input.records.iter().enumerate() {
        units(ledger, 1)?;
        let rec = || Stop::Failed(FaceError::Rec(k as u32));
        let place = match *p {
            RecPlace::Uv(uv) => Place::Uv(uv),
            RecPlace::At(r) => Place::At(vert(r).map_err(|_| rec())?),
            RecPlace::On { line: l, s } => Place::On {
                line: line(l).map_err(|_| rec())?,
                s,
            },
            RecPlace::Pin { line: l, s } => Place::Pin {
                line: line(l).map_err(|_| rec())?,
                s,
            },
        };
        places.push(place);
        verts.push(VertOf::Rec(k as u32));
    }

    // The new lines.
    for (k, nl) in input.lines.iter().enumerate() {
        units(ledger, 1 + nl.sources.len() as u64)?;
        let ends = [vert(nl.ends[0])?, vert(nl.ends[1])?];
        if !nl.sources.iter().all(|&s| exists(stores, s)) {
            return fail(FaceError::Source(k as u32));
        }
        lines.push(LineIn {
            ends,
            pcurve: nl.pcurve,
            wind: nl.wind,
        });
        refs.push(LineRef::New(k as u32));
        line_edge.push(None);
    }

    // The arcs.
    let mut arcs = Vec::with_capacity(input.arcs.len());
    let mut arc_ends = Vec::with_capacity(input.arcs.len());
    for (k, a) in input.arcs.iter().enumerate() {
        units(ledger, 1 + a.along.len() as u64 + a.sources.len() as u64)?;
        let bad = || Stop::Failed(FaceError::Arc(k as u32));
        let ends = [
            vert(a.ends[0]).map_err(|_| bad())?,
            vert(a.ends[1]).map_err(|_| bad())?,
        ];
        let along = a
            .along
            .iter()
            .map(|r| {
                Ok(Along {
                    line: line(r.line).map_err(|_| bad())?,
                    forward: r.forward,
                })
            })
            .collect::<Res<Vec<_>>>()?;
        if !a.sources.iter().all(|&s| exists(stores, s)) {
            return Err(bad());
        }
        arcs.push(along);
        arc_ends.push(ends);
    }

    Ok(Mapped {
        places,
        verts,
        face_uvs,
        lines,
        refs,
        arcs,
        arc_ends,
        line_edge,
    })
}

fn store(stores: [&Brep; 2], op: Operand) -> &Brep {
    match op {
        Operand::A => stores[0],
        Operand::B => stores[1],
    }
}

/// Whether the operand edge is in its store.
fn exists(stores: [&Brep; 2], (op, e): OperandEdge) -> bool {
    !e.is_none() && e.index() < store(stores, op).edge_count()
}

/// Per bundle, its sources (module docs).
fn sources(
    stores: [&Brep; 2],
    input: &FaceArcs<'_>,
    mapped: &Mapped<'_>,
    realised: &Coincident,
) -> Vec<Bundled> {
    let n_coedges = mapped.refs.len() - input.lines.len();
    realised
        .bundles
        .iter()
        .map(|b| {
            let mut sources: Vec<OperandEdge> = Vec::new();
            let mut opposed = false;
            for &p in &b.members {
                let (srcs, opp): (&[OperandEdge], bool) = match realised.pieces[p as usize].of {
                    PieceOf::Line { line, .. } => {
                        let l = line as usize;
                        match mapped.line_edge[l].as_ref() {
                            Some(e) => (std::slice::from_ref(e), false),
                            None => {
                                let nl = &input.lines[l - n_coedges];
                                (nl.sources, nl.opposed)
                            }
                        }
                    }
                    PieceOf::Arc { arc, .. } => {
                        let a = &input.arcs[arc as usize];
                        (a.sources, a.opposed)
                    }
                };
                sources.extend_from_slice(srcs);
                opposed |= opp;
            }
            sources.sort_unstable();
            sources.dedup();
            let touch = sources
                .iter()
                .any(|&(op, e)| store(stores, op).edge_kind(e).is_touch());
            let identified = b.members.len() > 1;
            Bundled {
                sources,
                touch,
                identified,
                opposed_join: identified && opposed,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
