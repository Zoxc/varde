//! Per-piece crossing counts and root clusters.
//!
//! **Per-piece counts.** A face `f` (with its ribbons) is partitioned
//! into pieces `P` whose free sides (the parts of a piece's boundary
//! inside the trim, shared with another piece) are clear of the edge
//! `e = a→b` in space. The crossings of `e` through one piece are, by
//! the same identity as the face's ([`varde_decide::count::cross`]):
//!
//! ```text
//! x12(e, P) = s02(b, P) − s02(a, P) − Σ_{γ⊂∂P} s11(e, γ)
//! ```
//!
//! - `s02(q, P)` sums the column hits of `q` that lie in `P`, each by
//!   its contribution to `s02(q, f)` (its facing where it lies above
//!   `q`, else 0). Hits are points, so they fall in one piece each and
//!   the pieces' shares sum to the face's count, whatever the partition.
//! - On a trim part `γ` of coedge `c` (an extent of its edge `g`'s
//!   span parameter, [`TrimPart`]), `s11(e, γ)` is `sgn(c)` times the
//!   degrees of the clusters of the crossing record `XRec(e, g)` that
//!   lie in that extent along `g` with `g` above `e`. A cluster lies in
//!   exactly one part or the cut between two parts runs through it,
//!   which is [`PieceError::CutInCluster`]: the caller moves the cut.
//!   A cluster above `e` in no part is [`PieceError::Uncovered`].
//! - A free side is shared by two pieces, run in opposite senses: its
//!   sweep term (`s11` of `e` against the side, decided by geometry as
//!   the side is clear of `e`) counts for one and negated for the other
//!   ([`FreeSide`]).
//!
//! Summed over the pieces the free sides cancel, the trim parts give
//! `Σ_c sgn(c)·s11(e, g_c)` and the hits `s02(·, f)`: so
//! `Σ_P x12(e, P) = x12(e, f)` exactly. [`piece_counts`] checks that
//! sum against the face's own count; a difference is
//! [`PieceError::Unbalanced`], a defect, never rounded away.
//!
//! **Root clusters** ([`implicit_roots`]). On a face whose surface is an
//! implicit `F = 0` the edge's span substituted into `F` is one
//! polynomial in the span's parameter (degree `≤ 8` for a quartic on a
//! conic span), isolated on `[0, 1]`. A simple root counts
//! `−sign(C′·n)`: `n` is `∇F` times the face's sense (`outward`, `Pos`
//! where `∇F` points out of the face's solid), and `C′·∇F` is the sign
//! the polynomial crosses zero with (the weights are positive). A
//! cluster (roots one level can't separate: a tangency, a run of the
//! edge lying in the face) counts by the sign change of `F` across it,
//! read at its certified end signs: `−outward` from negative to
//! positive, `+outward` the other way, and no record where both ends
//! have one sign (an edge touching the face). A cluster at a span's end
//! has no certified sign there: the end is settled once from that
//! point's record, which the caller reads ([`EndSigns`]); an edge lying
//! in the face along a whole span has none at either end and is
//! [`RootError::InFace`]. Results are dyadic intervals of the span
//! parameter in increasing order, each root in exactly one.
//!
//! **Core pieces** are realised by roots: the degrees of the roots in a
//! core piece sum to its count, else [`realise`] raises the ladder
//! signal [`RootError::Unrealised`].

use varde_decide::bezier::{Dyadic, Found, HomCurve, Implicit, Limits, Scalar, isolate};
use varde_decide::count::cross::{Coedge, CrossError, x12};
use varde_decide::ids::EdgeId;
use varde_decide::num::decide::Sign;
use varde_decide::records::keys::XClusterIx;
use varde_decide::records::xrec::{XCluster, XRec};

/// A piece of a face's partition, by index.
pub type PieceIx = u32;

/// A part of a trim inside one piece: coedge `coedge` (an index into the
/// face's boundary) along the extent `s` of its edge's span `span`, in
/// the edge's stored direction, its ends exact dyadic values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrimPart {
    /// The coedge, an index into [`FaceParts::boundary`].
    pub coedge: u32,
    /// The span of the coedge's edge.
    pub span: u32,
    /// `[lo, hi]` on the span's `[0, 1]`.
    pub s: [f64; 2],
    /// The piece it bounds.
    pub piece: PieceIx,
}

/// A free side shared by two pieces: its sweep term `s11` as `pieces[0]`
/// runs it; `pieces[1]` runs it the other way and counts it negated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreeSide {
    /// The two pieces it lies between.
    pub pieces: [PieceIx; 2],
    /// Its `s11` term along `pieces[0]`'s boundary.
    pub s11: i64,
}

/// A face's partition against an edge `e = a→b`.
#[derive(Clone, Copy, Debug)]
pub struct FaceParts<'a> {
    /// The number of pieces; every [`PieceIx`] is below it.
    pub pieces: u32,
    /// The coedges of all of the face's loops.
    pub boundary: &'a [Coedge],
    /// The column hits of `a` and of `b`: the piece each lies in and its
    /// contribution to `s02` (its facing above the point, else 0).
    pub hits: [&'a [(PieceIx, i64)]; 2],
    /// The trims' parts; each coedge's parts don't overlap.
    pub trims: &'a [TrimPart],
    /// The free sides.
    pub free: &'a [FreeSide],
}

/// Why the per-piece counts have no value.
#[derive(Clone, Debug, PartialEq)]
pub enum PieceError<E> {
    /// Reading a cluster's z-order failed.
    Read(E),
    /// A piece index at or past [`FaceParts::pieces`], or a coedge
    /// index past the boundary.
    BadIndex,
    /// A cut between two parts of coedge `coedge` runs through cluster
    /// `cluster` of its edge's record: the cut moves.
    CutInCluster {
        /// The coedge.
        coedge: u32,
        /// The cluster.
        cluster: XClusterIx,
    },
    /// A cluster above `e` lies in no part of coedge `coedge`.
    Uncovered {
        /// The coedge.
        coedge: u32,
        /// The cluster.
        cluster: XClusterIx,
    },
    /// A sum left `i64`.
    Overflow,
    /// The pieces' counts don't sum to the face's (a defect).
    Unbalanced {
        /// `x12(e, f)` by the face's identity.
        face: i64,
        /// `Σ_P x12(e, P)`.
        pieces: i64,
    },
}

/// The crossing count of `e` through each piece (module docs), checked
/// to sum to `x12(e, f)` exactly. `xrec(g)` gives the crossing record of
/// `e` against a trim's edge `g`, `above` reads a cluster's z-order
/// where it isn't clear (`Pos` for `g` above `e`).
pub fn piece_counts<'r, E>(
    parts: &FaceParts<'_>,
    mut xrec: impl FnMut(EdgeId) -> &'r XRec,
    mut above: impl FnMut(EdgeId, XClusterIx, &XCluster) -> Result<Sign, E>,
) -> Result<Vec<i64>, PieceError<E>> {
    let n = usize::try_from(parts.pieces).map_err(|_| PieceError::BadIndex)?;
    let ix = |p: PieceIx| -> Result<usize, PieceError<E>> {
        usize::try_from(p)
            .ok()
            .filter(|&i| i < n)
            .ok_or(PieceError::BadIndex)
    };
    let add = |acc: &mut i64, v: i64| -> Result<(), PieceError<E>> {
        *acc = acc.checked_add(v).ok_or(PieceError::Overflow)?;
        Ok(())
    };
    let mut x = vec![0i64; n];
    let mut ends = [0i64; 2];
    for (k, sign) in [(0, -1i64), (1, 1)] {
        for &(p, h) in parts.hits[k] {
            let i = ix(p)?;
            add(&mut ends[k], h)?;
            add(&mut x[i], h.checked_mul(sign).ok_or(PieceError::Overflow)?)?;
        }
    }
    let mut s11 = Vec::with_capacity(parts.boundary.len());
    for (ci, c) in parts.boundary.iter().enumerate() {
        let ci32 = u32::try_from(ci).map_err(|_| PieceError::BadIndex)?;
        let rec = xrec(c.edge);
        let mine: Vec<&TrimPart> = parts.trims.iter().filter(|t| t.coedge == ci32).collect();
        let mut total = 0i64;
        for (cx, cl) in rec.iter() {
            let z = match cl.clear {
                Some(z) => z,
                None => above(c.edge, cx, cl).map_err(PieceError::Read)?,
            };
            if z != Sign::Pos {
                continue;
            }
            add(&mut total, cl.degree)?;
            let at = part_of(&mine, cl).map_err(|e| match e {
                Miss::Cut => PieceError::CutInCluster {
                    coedge: ci32,
                    cluster: cx,
                },
                Miss::None => PieceError::Uncovered {
                    coedge: ci32,
                    cluster: cx,
                },
            })?;
            let i = ix(at)?;
            let v = cl.degree.checked_mul(c.sgn()).ok_or(PieceError::Overflow)?;
            add(&mut x[i], v.checked_neg().ok_or(PieceError::Overflow)?)?;
        }
        s11.push(total);
    }
    if parts
        .trims
        .iter()
        .any(|t| t.coedge as usize >= parts.boundary.len())
    {
        return Err(PieceError::BadIndex);
    }
    for side in parts.free {
        let [p, q] = side.pieces;
        let (i, j) = (ix(p)?, ix(q)?);
        add(
            &mut x[i],
            side.s11.checked_neg().ok_or(PieceError::Overflow)?,
        )?;
        add(&mut x[j], side.s11)?;
    }
    let face = x12(ends, parts.boundary, |g| {
        let i = parts.boundary.iter().position(|c| c.edge == g);
        Ok::<_, ()>(i.map_or(0, |i| s11[i]))
    })
    .map_err(|e| match e {
        CrossError::Overflow | CrossError::Read(()) => PieceError::Overflow,
    })?;
    let mut sum = 0i64;
    for &v in &x {
        add(&mut sum, v)?;
    }
    if sum != face {
        return Err(PieceError::Unbalanced { face, pieces: sum });
    }
    Ok(x)
}

/// Why a cluster has no part.
enum Miss {
    Cut,
    None,
}

/// The piece of the part holding cluster `cl`'s extent along `g`.
fn part_of(parts: &[&TrimPart], cl: &XCluster) -> Result<PieceIx, Miss> {
    let [lo, hi] = cl.g.t;
    let mut met = false;
    for t in parts.iter().filter(|t| t.span == cl.g.span) {
        let [a, b] = t.s;
        if a <= lo && hi <= b {
            return Ok(t.piece);
        }
        met |= lo < b && a < hi;
    }
    Err(if met { Miss::Cut } else { Miss::None })
}

/// What kind of root set an end record's root cluster is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootKind {
    /// A certified simple root.
    Isolated,
    /// Roots one level can't separate, counted by the sign change.
    Cluster,
}

/// A root cluster of non-zero degree of an edge's span on an implicit
/// face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RootEnd {
    /// The span of the edge, from 0 at its lower-id end.
    pub span: u32,
    /// `[lo, hi]` on the span's `[0, 1]`, closed.
    pub t: [Dyadic; 2],
    /// `f64`s with the root (or the cluster) between them.
    pub near: [f64; 2],
    /// `−sign(C′·n)`: `+1` entering the face's solid.
    pub degree: i8,
    /// Isolated root or cluster.
    pub kind: RootKind,
}

/// The sign of `F` at a span's end where a cluster reaches it, read from
/// that point's record: `end_sign(span, at_end)`, `at_end` for `t = 1`.
pub trait EndSigns {
    /// The sign there; `None` where `F` is zero there.
    fn end_sign(&mut self, span: u32, at_end: bool) -> Option<Sign>;
}

impl<T: FnMut(u32, bool) -> Option<Sign>> EndSigns for T {
    fn end_sign(&mut self, span: u32, at_end: bool) -> Option<Sign> {
        self(span, at_end)
    }
}

/// Why an edge's roots on a face have no answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootError {
    /// The substituted polynomial is past the degree limit, or a span's
    /// weights aren't positive.
    Form {
        /// The span.
        span: u32,
    },
    /// The isolation ran out of work on a span.
    OutOfWork {
        /// The span.
        span: u32,
    },
    /// The span lies in the face (the sign is zero at both ends of a
    /// cluster): the coincidence rules decide it.
    InFace {
        /// The span.
        span: u32,
    },
    /// A cluster's end sign is zero at a span end whose record leaves it
    /// zero: the pieces meeting there settle it.
    EndUnsettled {
        /// The span.
        span: u32,
    },
    /// A core piece's roots don't realise its count (a ladder signal).
    Unrealised {
        /// The piece's count.
        want: i64,
        /// The roots' degrees summed.
        got: i64,
    },
}

/// The root clusters of non-zero degree of an edge, given as its spans
/// in order, on the implicit face `f = 0` (module docs). `outward` is
/// `Pos` where `∇F` points out of the face's solid.
pub fn implicit_roots<S: Scalar>(
    spans: &[HomCurve<S, 3>],
    f: &Implicit<S>,
    outward: Sign,
    limits: Limits,
    mut ends: impl EndSigns,
) -> Result<Vec<RootEnd>, RootError> {
    let mut out = Vec::new();
    for (k, c) in spans.iter().enumerate() {
        let span = u32::try_from(k).map_err(|_| RootError::Form { span: u32::MAX })?;
        if !c.weights_positive() {
            return Err(RootError::Form { span });
        }
        let p = c.compose_implicit(f).ok_or(RootError::Form { span })?;
        let roots = isolate(&p, limits).map_err(|_| RootError::OutOfWork { span })?;
        for found in roots.found {
            match found {
                Found::Root(r) => {
                    let Some(dir) = Sign::of(r.dir) else {
                        continue;
                    };
                    out.push(RootEnd {
                        span,
                        t: [r.lo, r.hi],
                        near: r.near,
                        degree: entering(dir, outward),
                        kind: RootKind::Isolated,
                    });
                }
                Found::Cluster(cl) => {
                    let start = match cl.start.and_then(Sign::of) {
                        Some(s) => Some(s),
                        None if cl.lo == Dyadic::ZERO => ends.end_sign(span, false),
                        None => None,
                    };
                    let end = match cl.end.and_then(Sign::of) {
                        Some(s) => Some(s),
                        None if cl.hi == Dyadic::ONE => ends.end_sign(span, true),
                        None => None,
                    };
                    let (start, end) = match (start, end) {
                        (Some(a), Some(b)) => (a, b),
                        (None, None) => return Err(RootError::InFace { span }),
                        _ => return Err(RootError::EndUnsettled { span }),
                    };
                    if start == end {
                        continue;
                    }
                    out.push(RootEnd {
                        span,
                        t: [cl.lo, cl.hi],
                        near: [cl.lo.bounds()[0], cl.hi.bounds()[1]],
                        degree: entering(end, outward),
                        kind: RootKind::Cluster,
                    });
                }
            }
        }
    }
    Ok(out)
}

/// `−sign(C′·n)` for `F` turning to `to` along the edge.
fn entering(to: Sign, outward: Sign) -> i8 {
    if to == outward { -1 } else { 1 }
}

/// Checks that the roots in a core piece realise its count `want`.
pub fn realise(want: i64, roots: &[RootEnd]) -> Result<(), RootError> {
    let got: i64 = roots.iter().map(|r| i64::from(r.degree)).sum();
    if got == want {
        Ok(())
    } else {
        Err(RootError::Unrealised { want, got })
    }
}

#[cfg(test)]
mod tests;
