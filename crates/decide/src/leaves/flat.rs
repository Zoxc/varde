//! Flat leaves: the kind of a pair of pieces, one box of a face of each
//! operand, certified exactly.
//!
//! **Pieces** ([`FlatPiece`]). A piece is the part of a face's tile over
//! one of its dyadic boxes ([`BoxKey`]): the tile's exact sub-patch over
//! the box ([`sub_patch`] on the tile's rational net), which must be
//! polynomial (every weight one value, positive), as a plane's tile is.
//! The piece keeps
//! - its **control points**, exactly: the patch lies in their convex hull;
//! - its **graph vectors**: `σ·(a × b)` for every pair of a
//!   `u`-hodograph vector `a` (a difference of neighbouring control points
//!   along `u`) and a `v`-hodograph vector `b`, `σ = −1` on a flipped face,
//!   each scaled to a largest component of magnitude 1, sorted, repeats
//!   dropped. Every outward normal of the piece is a positive combination
//!   of them (`x_u` and `x_v` are positive combinations of the hodograph
//!   vectors, and the cross product is bilinear), and `ℓ·g > 0` for all of
//!   them makes the piece an injective graph over the plane `ℓ⊥` (its
//!   projection along `ℓ` has a Jacobian of one sign on a convex box) with
//!   every normal in the open hemisphere about `ℓ`. On a plane's tile all
//!   of them are one vector, the plane's normal;
//! - its **world box**, the control points' bounding box rounded outward
//!   to `f64`s.
//!
//! A piece is the patch over the whole box, trims ignored: a superset of
//! the face's part there, so a certificate on it holds on that part. The
//! caller drops boxes outside the trim ([`BoxUse::Drop`]).
//!
//! **Kinds**, tried in this order at each pair, the first that certifies
//! taken ([`certify`]); a pair that certifies Empty or Simple is never
//! snapped:
//! 1. **Empty** ([`empty`]): the hulls are disjoint, a direction `d` with
//!    `d·p < d·q` for every control point `p` of the first piece and `q` of
//!    the second. The pieces' graph vectors are tried first, then an exact
//!    linear program finds one iff the hulls are disjoint (the max–min
//!    direction of the differences `q − p`, positive). Two disjoint flat
//!    pieces always certify here, however close: they never form a
//!    cluster.
//! 2. **Simple** ([`simple`]): no tangency. The normal cones of the first
//!    piece and of `±` the second are disjoint (a direction positive on
//!    the first's graph vectors and negative on the second's, and one
//!    positive on both), and `ℓ` has `ℓ·(g × h) > 0` for every graph vector
//!    `g` of the first and `h` of the second.
//! 3. **Near** ([`near`], co-oriented): `ℓ·g > 0` on the graph vectors of
//!    both pieces (so both are graphs over `ℓ⊥` and their normals lie in
//!    one open hemisphere), and the two-sided gap along `ℓ` is at most
//!    `b_K`. **Contact** ([`contact`], opposed): the same with the second
//!    piece's graph vectors negated.
//!
//! **Directions.** A certificate's direction (`ℓ`, or Empty's `d`) is the
//! direction of its vectors as flat elements take it, exact: the
//! equal-offsets direction of the vectors (scaled to a largest component
//! of 1), else their max–min direction if its minimum is positive
//! ([`crate::perturb::dir`]). So on flat pieces `ℓ` is the shared normal
//! itself (Near, Contact) or the common line's direction (Simple), and it
//! moves with the input under 90° rotations and mirrors of the axes.
//!
//! **The vertical gap along `ℓ`** ([`Cone::contains`]): both pieces are
//! graphs over `ℓ⊥`, so the gap is `h_a − h_b` over their common
//! projection, `h = x·ℓ̂` the height along `ℓ̂ = ℓ/|ℓ|`. It is bounded over
//! pairs of sub-pieces, the pieces split dyadically: a pair whose
//! projections onto `ℓ⊥` are disjoint has no common point and bounds
//! nothing (decided exactly: the hulls of the projections are disjoint
//! iff some direction is positive on every `(q − p) × ℓ`, a max–min
//! program); on any other pair
//! - two **parallel plane** pieces (each one graph vector, the same line)
//!   have the constant gap `(c_b − c_a)/(n·ℓ̂)`, `n·x = c` their planes:
//!   exact, so flush coplanar pieces have gap zero along every `ℓ` they
//!   are graphs over, not only along their normal;
//! - any other pair is bounded by its control hulls, `|h_a − h_b| ≤
//!   max(max ℓ̂·p − min ℓ̂·q, max ℓ̂·q − min ℓ̂·p)` (the signed gap of
//!   Contact included), and a pair above `b_K` splits its piece of the
//!   wider extent along `ℓ` (the first's on a tie) at the middle of the
//!   direction its net varies more in along `ℓ` (`u` on a tie), at most
//!   [`GAP_SPLITS`] times a piece and [`MAX_GAP_PAIRS`] pairs a check.
//!
//! The gap is within `b_K` when every pair is; compared exactly (`s ≤ 0`,
//! or `s² ≤ b_K²·|ℓ|²` for the unnormalised bound `s`), so no square root
//! is taken. Along the shared normal of two parallel planes it is their
//! distance exactly. A pair still above `b_K` that can't split fails the
//! check: as the leaf's own direction the pair is uncertified, and it
//! splits. Every sub-piece pair bounded is counted ([`LeafWork::gaps`]).
//!
//! **Admissible cone** ([`Cone`]): the directions a Near or Contact leaf
//! accepts in place of its own `ℓ`: `ℓ·g > 0` on its (signed) graph
//! vectors and the vertical gap along `ℓ` at most `b_K`, both decided
//! exactly by [`Cone::contains`]. The first condition is an open
//! polyhedral cone, the second a cone (scaling `ℓ` keeps it) that need
//! not be convex. On parallel plane pieces it is `(c_b − c_a)² ≤
//! b_K²·(n·ℓ̂)²` wherever the projections meet: the whole open hemisphere
//! of the normal when the planes coincide.
//!
//! **Leaves** ([`leaves`]). From a pair of start boxes, depth first: the
//! caller says per box whether to drop it (outside the trim), split it
//! or test it ([`BoxUse`]); a tested pair takes the first kind that
//! certifies, else the coarser box (lesser depth, the first piece's on a
//! tie) splits at the middle of its coarser extent (`u` on a tie). The
//! split is a function of the boxes alone, so the leaves are too, and
//! they are returned ordered by their boxes. Every pair tested is
//! counted ([`LeafWork`], with the linear programs run) for the caller
//! to charge; past the caller's cap, or where a box can't split
//! ([`MAX_BOX_DEPTH`]), the run stops.
//!
//! **Exact.** Pieces, vectors, directions and every comparison are exact
//! rationals: no tolerance and no near ties, a gap equal to `b_K`
//! certifies and one a bit above doesn't.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use malachite::Rational;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::{Ceiling, Floor};

use crate::bezier::{Dir, HomPatch};
use crate::count::boxes::{BoxKey, MAX_BOX_DEPTH, sub_patch};
use crate::perturb::dir::{Q3, cross, dot, equal_offsets, max_min};

/// Why a piece can't be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceError {
    /// The patch isn't polynomial: its weights differ or aren't positive.
    Weights,
    /// The net's size doesn't match its degrees, or a degree is zero.
    Net,
    /// A coordinate isn't finite.
    NotFinite,
}

/// The part of a face's tile over one box (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct FlatPiece {
    points: Vec<Q3>,
    graph: Vec<Q3>,
    world: [[f64; 3]; 2],
    net: Net,
}

impl FlatPiece {
    /// The piece of `tile` over `b` (its local extents), on a face
    /// `flipped` against the tile or not.
    pub fn of_box(
        tile: &HomPatch<Rational>,
        b: &BoxKey,
        flipped: bool,
    ) -> Result<FlatPiece, PieceError> {
        let sub = sub_patch(tile, b);
        let w = sub.weights().coefs();
        let w0 = w.first().ok_or(PieceError::Net)?;
        if *w0 <= 0u32 || w.iter().any(|x| x != w0) {
            return Err(PieceError::Weights);
        }
        let (m, n) = sub.degrees();
        let [x, y, z, _] = sub.tensors();
        let points = (0..(m + 1) * (n + 1))
            .map(|k| [x, y, z].map(|t| &t.coefs()[k] / w0))
            .collect();
        FlatPiece::from_points(m, n, points, flipped)
    }

    /// The piece of the polynomial patch of degree `(m, n)` with control
    /// points `net` (row-major in `u`: `(i, j)` at `i·(n + 1) + j`), on a
    /// face `flipped` against it or not.
    pub fn from_net(
        m: usize,
        n: usize,
        net: &[[f64; 3]],
        flipped: bool,
    ) -> Result<FlatPiece, PieceError> {
        let points = net
            .iter()
            .map(|p| {
                let c = |x: f64| Rational::try_from(x).map_err(|_| PieceError::NotFinite);
                Ok([c(p[0])?, c(p[1])?, c(p[2])?])
            })
            .collect::<Result<Vec<_>, _>>()?;
        FlatPiece::from_points(m, n, points, flipped)
    }

    fn from_points(
        m: usize,
        n: usize,
        points: Vec<Q3>,
        flipped: bool,
    ) -> Result<FlatPiece, PieceError> {
        if m == 0 || n == 0 || points.len() != (m + 1) * (n + 1) {
            return Err(PieceError::Net);
        }
        let at = |i: usize, j: usize| &points[i * (n + 1) + j];
        let mut du = Vec::with_capacity(m * (n + 1));
        for i in 0..m {
            for j in 0..=n {
                du.push(sub3(at(i + 1, j), at(i, j)));
            }
        }
        let mut dv = Vec::with_capacity((m + 1) * n);
        for i in 0..=m {
            for j in 0..n {
                dv.push(sub3(at(i, j + 1), at(i, j)));
            }
        }
        let mut graph = Vec::with_capacity(du.len() * dv.len());
        for a in &du {
            for b in &dv {
                let g = cross(a, b);
                graph.push(if flipped { neg3(&g) } else { g });
            }
        }
        let graph = canonical(graph);
        let world = bounding(&points);
        let net = Net {
            deg: [m, n],
            points: points.clone(),
            splits: 0,
        };
        let mut points = points;
        points.sort();
        points.dedup();
        Ok(FlatPiece {
            points,
            graph,
            world,
            net,
        })
    }

    /// The control points, sorted, repeats dropped: the piece lies in
    /// their convex hull.
    pub fn points(&self) -> &[Q3] {
        &self.points
    }

    /// The graph vectors (module docs), each scaled to a largest
    /// component of magnitude 1 (a zero one kept as zero), sorted.
    pub fn graph(&self) -> &[Q3] {
        &self.graph
    }

    /// The control points' bounding box rounded outward, `[lo, hi]`.
    pub fn world_box(&self) -> [[f64; 3]; 2] {
        self.world
    }

    /// The normal of its plane, if it is a plane piece: its one graph
    /// vector (non-zero; all control points then lie in one plane).
    fn plane(&self) -> Option<&Q3> {
        match &self.graph[..] {
            [g] if !is_zero(g) => Some(g),
            _ => None,
        }
    }
}

/// A control net in the vertical-gap check: its degrees, its points
/// (row-major in `u`) and how often it was split.
#[derive(Clone, Debug, PartialEq)]
struct Net {
    deg: [usize; 2],
    points: Vec<Q3>,
    splits: u32,
}

impl Net {
    /// Its halves at the middle of the direction it varies more in along
    /// `ell` (the larger `|ell·d|` over its hodograph vectors `d`, `u` on a
    /// tie), exactly (de Casteljau at ½); `None` once split
    /// [`GAP_SPLITS`] times.
    fn halves(&self, ell: &Q3) -> Option<[Net; 2]> {
        if self.splits >= GAP_SPLITS {
            return None;
        }
        let [m, n] = self.deg;
        let at = |i: usize, j: usize| &self.points[i * (n + 1) + j];
        let abs = |x: Rational| if x < 0u32 { -x } else { x };
        let mut vary = [Rational::from(0), Rational::from(0)];
        for i in 0..=m {
            for j in 0..=n {
                if i < m {
                    let d = abs(dot(&sub3(at(i + 1, j), at(i, j)), ell));
                    vary[0] = Rational::max(d, vary[0].clone());
                }
                if j < n {
                    let d = abs(dot(&sub3(at(i, j + 1), at(i, j)), ell));
                    vary[1] = Rational::max(d, vary[1].clone());
                }
            }
        }
        let along_u = vary[0] >= vary[1];
        let (len, count) = if along_u {
            (m + 1, n + 1)
        } else {
            (n + 1, m + 1)
        };
        let at = |k: usize, c: usize| {
            if along_u {
                k * (n + 1) + c
            } else {
                c * (n + 1) + k
            }
        };
        let mut lo = self.points.clone();
        let mut hi = self.points.clone();
        let half = Rational::from_signeds(1, 2);
        for c in 0..count {
            let mut row: Vec<Q3> = (0..len).map(|k| self.points[at(k, c)].clone()).collect();
            lo[at(0, c)] = row[0].clone();
            hi[at(len - 1, c)] = row[len - 1].clone();
            for r in 1..len {
                for k in 0..len - r {
                    row[k] = [0, 1, 2].map(|i| (&row[k][i] + &row[k + 1][i]) * &half);
                }
                lo[at(r, c)] = row[0].clone();
                hi[at(len - 1 - r, c)] = row[len - 1 - r].clone();
            }
        }
        let splits = self.splits + 1;
        let net = |points| Net {
            deg: self.deg,
            points,
            splits,
        };
        Some([net(lo), net(hi)])
    }
}

/// The most times the vertical-gap check splits a piece (module docs).
pub const GAP_SPLITS: u32 = 12;

/// The most sub-piece pairs one vertical-gap check bounds (module docs).
pub const MAX_GAP_PAIRS: u32 = 1 << 12;

/// The directions a Near or Contact leaf admits (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Cone {
    gens: Vec<Q3>,
    a: FlatPiece,
    b: FlatPiece,
    b_k: Rational,
}

impl Cone {
    /// The graph vectors `ℓ` must be positive on: both pieces', the
    /// second's negated for Contact; scaled, sorted. The leaf's normals
    /// as a common direction reads them.
    pub fn gens(&self) -> &[Q3] {
        &self.gens
    }

    /// The gap bound it was certified with.
    pub fn b_k(&self) -> &Rational {
        &self.b_k
    }

    /// Whether `ell` is admitted: `ell·g > 0` for every generator and the
    /// vertical gap along `ell` at most `b_K` (module docs), exactly; the
    /// sub-piece pairs and programs run are counted in `work`.
    pub fn contains(&self, ell: &Q3, work: &mut LeafWork) -> bool {
        self.gens.iter().all(|g| dot(g, ell) > 0u32)
            && gap_within(&self.a, &self.b, ell, &self.b_k, work)
    }
}

/// A Near or Contact certificate: the leaf's direction and its cone.
#[derive(Clone, Debug, PartialEq)]
pub struct Tangent {
    /// The leaf direction `ℓ`, in its cone.
    pub ell: Q3,
    /// The directions the leaf admits.
    pub cone: Cone,
}

/// A pair's certified kind (module docs).
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Disjoint hulls: `d·p < d·q` for every control point `p` of the
    /// first piece and `q` of the second.
    Empty {
        /// The separating direction.
        d: Q3,
    },
    /// No tangency.
    Simple {
        /// `ℓ·(g × h) > 0` for every graph vector `g` of the first piece
        /// and `h` of the second.
        ell: Q3,
    },
    /// Co-oriented, within `b_K` along `ℓ`.
    Near(Tangent),
    /// Opposed, within `b_K` along `ℓ`.
    Contact(Tangent),
}

/// What certificates ran, for the caller to charge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LeafWork {
    /// Pairs of boxes visited.
    pub pairs: u32,
    /// Exact linear programs solved (each a max–min direction).
    pub programs: u32,
    /// Sub-piece pairs whose vertical gap was bounded (module docs).
    pub gaps: u32,
}

/// Empty: a direction separating the hulls of `a` and `b`, if they are
/// disjoint (module docs).
pub fn empty(a: &FlatPiece, b: &FlatPiece, work: &mut LeafWork) -> Option<Q3> {
    for d in a.graph.iter().chain(&b.graph).filter(|d| !is_zero(d)) {
        let (lo_a, hi_a) = extent(&a.points, d);
        let (lo_b, hi_b) = extent(&b.points, d);
        if hi_a < lo_b {
            return Some(d.clone());
        }
        if hi_b < lo_a {
            return Some(neg3(d));
        }
    }
    let mut diffs: Vec<Q3> = Vec::with_capacity(a.points.len() * b.points.len());
    for p in &a.points {
        for q in &b.points {
            diffs.push(sub3(q, p));
        }
    }
    diffs.sort();
    diffs.dedup();
    work.programs = work.programs.saturating_add(1);
    let (d, min) = max_min(&diffs);
    (min > 0u32).then_some(d)
}

/// Simple: `ℓ`, if the normal cones of `a` and `±b` are disjoint and some
/// `ℓ` is positive on every `g × h` (module docs).
pub fn simple(a: &FlatPiece, b: &FlatPiece, work: &mut LeafWork) -> Option<Q3> {
    if a.graph.iter().chain(&b.graph).any(is_zero) {
        return None;
    }
    let apart = |sign: bool, work: &mut LeafWork| {
        let mut vs = a.graph.clone();
        vs.extend(
            b.graph
                .iter()
                .map(|h| if sign { h.clone() } else { neg3(h) }),
        );
        work.programs = work.programs.saturating_add(1);
        max_min(&canonical(vs)).1 > 0u32
    };
    if !apart(false, work) || !apart(true, work) {
        return None;
    }
    let mut crosses = Vec::with_capacity(a.graph.len() * b.graph.len());
    for g in &a.graph {
        for h in &b.graph {
            crosses.push(cross(g, h));
        }
    }
    direction(&canonical(crosses), work)
}

/// Near: `ℓ` and the cone, if both pieces are graphs over `ℓ⊥` with their
/// normals about `ℓ` and the gap along `ℓ` is at most `b_k` (module
/// docs).
pub fn near(a: &FlatPiece, b: &FlatPiece, b_k: &Rational, work: &mut LeafWork) -> Option<Tangent> {
    tangent(a, b, false, b_k, work)
}

/// Contact: as [`near`] with `b`'s normals reversed.
pub fn contact(
    a: &FlatPiece,
    b: &FlatPiece,
    b_k: &Rational,
    work: &mut LeafWork,
) -> Option<Tangent> {
    tangent(a, b, true, b_k, work)
}

/// The first kind that certifies, in the order Empty, Simple, Near,
/// Contact (module docs); `None` if none does.
pub fn certify(a: &FlatPiece, b: &FlatPiece, b_k: &Rational, work: &mut LeafWork) -> Option<Kind> {
    if let Some(d) = empty(a, b, work) {
        return Some(Kind::Empty { d });
    }
    if let Some(ell) = simple(a, b, work) {
        return Some(Kind::Simple { ell });
    }
    if let Some(t) = near(a, b, b_k, work) {
        return Some(Kind::Near(t));
    }
    contact(a, b, b_k, work).map(Kind::Contact)
}

fn tangent(
    a: &FlatPiece,
    b: &FlatPiece,
    opposed: bool,
    b_k: &Rational,
    work: &mut LeafWork,
) -> Option<Tangent> {
    debug_assert!(*b_k >= 0u32, "a negative gap bound");
    if a.graph.iter().chain(&b.graph).any(is_zero) {
        return None;
    }
    let mut gens = a.graph.clone();
    gens.extend(
        b.graph
            .iter()
            .map(|h| if opposed { neg3(h) } else { h.clone() }),
    );
    let gens = canonical(gens);
    let ell = direction(&gens, work)?;
    if !gap_within(a, b, &ell, b_k, work) {
        return None;
    }
    Some(Tangent {
        ell,
        cone: Cone {
            gens,
            a: a.clone(),
            b: b.clone(),
            b_k: b_k.clone(),
        },
    })
}

/// A face as [`leaves`] reads it: its tile's exact net and whether the
/// face is flipped against the tile.
#[derive(Clone, Copy, Debug)]
pub struct FlatFace<'a> {
    /// The tile's patch, exact (polynomial).
    pub patch: &'a HomPatch<Rational>,
    /// Whether the face's outward side is against the tile's `x_u × x_v`.
    pub flipped: bool,
}

/// What [`leaves`] does with a box, the caller's choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxUse {
    /// Outside the face: no leaf.
    Drop,
    /// Split before testing (a box too large or mixed for the caller).
    Split,
    /// Test it.
    Test,
}

/// A certified pair of boxes.
#[derive(Clone, Debug, PartialEq)]
pub struct Leaf {
    /// The first face's box.
    pub a: BoxKey,
    /// The second face's box.
    pub b: BoxKey,
    /// Its kind.
    pub kind: Kind,
    /// The pieces' world boxes, the first face's first, each `[lo, hi]`
    /// rounded outward.
    pub world: [[[f64; 3]; 2]; 2],
}

/// Why [`leaves`] stopped.
#[derive(Clone, Debug, PartialEq)]
pub enum LeafError {
    /// The pair certifies no kind and its boxes can't split any further:
    /// the first face's box, then the second's.
    Depth(Box<[BoxKey; 2]>),
    /// More pairs than the cap: the work so far.
    Pairs(LeafWork),
    /// A box's piece can't be made.
    Piece(BoxKey, PieceError),
}

/// The certified leaves under the pair of boxes `a0` of face `fa` and
/// `b0` of face `fb` (module docs), with gap bound `b_k`, visiting at most
/// `max_pairs` pairs; `use_box` says per box what to do with it and must
/// depend on the box alone. The leaves are ordered by their boxes, and
/// come with the work done.
pub fn leaves(
    fa: FlatFace<'_>,
    a0: BoxKey,
    fb: FlatFace<'_>,
    b0: BoxKey,
    b_k: &Rational,
    max_pairs: u32,
    mut use_box: impl FnMut(&BoxKey) -> BoxUse,
) -> Result<(Vec<Leaf>, LeafWork), LeafError> {
    let mut work = LeafWork::default();
    let mut out = Vec::new();
    let mut pieces: BTreeMap<BoxKey, FlatPiece> = BTreeMap::new();
    let mut stack = vec![(a0, b0)];
    while let Some((a, b)) = stack.pop() {
        if work.pairs >= max_pairs {
            return Err(LeafError::Pairs(work));
        }
        work.pairs += 1;
        let (ua, ub) = (use_box(&a), use_box(&b));
        if ua == BoxUse::Drop || ub == BoxUse::Drop {
            continue;
        }
        let first = match (ua, ub) {
            (BoxUse::Split, _) => Some(true),
            (_, BoxUse::Split) => Some(false),
            _ => None,
        };
        let first = match first {
            Some(f) => f,
            None => {
                let pa = piece(&mut pieces, fa, &a)?;
                let pb = piece(&mut pieces, fb, &b)?;
                if let Some(kind) = certify(&pa, &pb, b_k, &mut work) {
                    out.push(Leaf {
                        a,
                        b,
                        kind,
                        world: [pa.world, pb.world],
                    });
                    continue;
                }
                a.depth() <= b.depth()
            }
        };
        // The chosen box, or the other if it can't split.
        let halves = if first {
            halve(&a)
                .map(|h| h.map(|x| (x, b)))
                .or_else(|| halve(&b).map(|h| h.map(|y| (a, y))))
        } else {
            halve(&b)
                .map(|h| h.map(|y| (a, y)))
                .or_else(|| halve(&a).map(|h| h.map(|x| (x, b))))
        };
        let [lo, hi] = halves.ok_or_else(|| LeafError::Depth(Box::new([a, b])))?;
        stack.push(hi);
        stack.push(lo);
    }
    out.sort_by_key(|l| (l.a, l.b));
    Ok((out, work))
}

/// The piece of `face` over `b`, made once.
fn piece(
    pieces: &mut BTreeMap<BoxKey, FlatPiece>,
    face: FlatFace<'_>,
    b: &BoxKey,
) -> Result<FlatPiece, LeafError> {
    if let Some(p) = pieces.get(b) {
        return Ok(p.clone());
    }
    let p = FlatPiece::of_box(face.patch, b, face.flipped).map_err(|e| LeafError::Piece(*b, e))?;
    pieces.insert(*b, p.clone());
    Ok(p)
}

/// `b` split at the middle of its coarser extent (`u` on a tie), or of
/// the other one if that can't split; `None` if neither can.
pub fn halve(b: &BoxKey) -> Option<[BoxKey; 2]> {
    let (eu, ev) = (b.extent(Dir::U), b.extent(Dir::V));
    let order = if ev.depth() < eu.depth() {
        [Dir::V, Dir::U]
    } else {
        [Dir::U, Dir::V]
    };
    order.into_iter().find_map(|dir| {
        let e = b.extent(dir);
        (e.depth() < MAX_BOX_DEPTH)
            .then(|| b.split_at(dir, e.mid()))
            .flatten()
    })
}

/// The direction of `vs` as a flat element's (module docs): equal
/// offsets of the scaled vectors, else their max–min direction with a
/// positive minimum. `None` if a vector is zero or none agrees.
fn direction(vs: &[Q3], work: &mut LeafWork) -> Option<Q3> {
    if vs.is_empty() || vs.iter().any(is_zero) {
        return None;
    }
    if let Some(d) = equal_offsets(vs) {
        return Some(d);
    }
    work.programs = work.programs.saturating_add(1);
    let (d, min) = max_min(vs);
    (min > 0u32).then_some(d)
}

/// Whether the vertical gap along `ell` between `a` and `b` is at most
/// `b_k` (module docs), exactly; `ell` must be positive on both pieces'
/// graph vectors (as signed for the leaf) for the gap to be a graph's.
fn gap_within(a: &FlatPiece, b: &FlatPiece, ell: &Q3, b_k: &Rational, work: &mut LeafWork) -> bool {
    let bound = b_k * b_k * dot(ell, ell);
    // Within `b_k` for the unnormalised gap `s` along `ell`.
    let within = |s: &Rational| *s <= 0u32 || s * s <= bound;
    if let (Some(n), Some(m)) = (a.plane(), b.plane())
        && is_zero(&cross(n, m))
    {
        // Parallel planes: `x_b = x_a + λ·ell` over a common point, `λ =
        // (c_b − c_a)/(n·ell)`, so the gap `|λ|·|ell|` is `s/|ell|` with
        // `s = |c_b − c_a|·|ell|²/|n·ell|`; compared as `(c_b − c_a)²·|ell|²
        // ≤ b_k²·(n·ell)²`.
        work.gaps = work.gaps.saturating_add(1);
        let ne = dot(n, ell);
        if ne == 0u32 {
            return false;
        }
        let d = dot(n, &b.points[0]) - dot(n, &a.points[0]);
        if &d * &d * dot(ell, ell) <= b_k * b_k * &ne * &ne {
            return true;
        }
        return !overlap(&a.net.points, &b.net.points, ell, work);
    }
    let mut stack = vec![(a.net.clone(), b.net.clone())];
    let mut pairs = 0u32;
    while let Some((pa, pb)) = stack.pop() {
        if pairs >= MAX_GAP_PAIRS {
            return false;
        }
        pairs += 1;
        work.gaps = work.gaps.saturating_add(1);
        let (lo_a, hi_a) = extent(&pa.points, ell);
        let (lo_b, hi_b) = extent(&pb.points, ell);
        let s = Rational::max(&hi_a - &lo_b, &hi_b - &lo_a);
        if within(&s) || !overlap(&pa.points, &pb.points, ell, work) {
            continue;
        }
        // The wider piece along `ell` splits, else the other.
        let a_first = &hi_a - &lo_a >= &hi_b - &lo_b;
        let split = |p: &Net, q: &Net, p_is_a: bool| {
            p.halves(ell).map(|hs| {
                hs.map(|h| {
                    if p_is_a {
                        (h, q.clone())
                    } else {
                        (q.clone(), h)
                    }
                })
            })
        };
        let halves = if a_first {
            split(&pa, &pb, true).or_else(|| split(&pb, &pa, false))
        } else {
            split(&pb, &pa, false).or_else(|| split(&pa, &pb, true))
        };
        let Some([lo, hi]) = halves else {
            return false;
        };
        stack.push(hi);
        stack.push(lo);
    }
    true
}

/// Whether the projections along `ell` of the hulls of `a` and `b` share
/// a point: no direction is positive on every `(q − p) × ell`, the
/// projected differences turned a quarter in `ell⊥` (exact).
fn overlap(a: &[Q3], b: &[Q3], ell: &Q3, work: &mut LeafWork) -> bool {
    let mut diffs: Vec<Q3> = Vec::with_capacity(a.len() * b.len());
    for p in a {
        for q in b {
            let w = cross(&sub3(q, p), ell);
            if is_zero(&w) {
                return true;
            }
            diffs.push(w);
        }
    }
    let diffs = canonical(diffs);
    work.programs = work.programs.saturating_add(1);
    max_min(&diffs).1 <= 0u32
}

/// `(min, max)` of `d·p` over `ps` (non-empty).
fn extent(ps: &[Q3], d: &Q3) -> (Rational, Rational) {
    let mut vals = ps.iter().map(|p| dot(p, d));
    let first = vals.next().expect("a piece has control points");
    vals.fold((first.clone(), first), |(lo, hi), v| {
        let lo = if v < lo { v.clone() } else { lo };
        let hi = if v > hi { v } else { hi };
        (lo, hi)
    })
}

/// Each vector scaled to a largest component of magnitude 1 (zero kept),
/// sorted, repeats dropped.
fn canonical(vs: Vec<Q3>) -> Vec<Q3> {
    let mut out: Vec<Q3> = vs
        .into_iter()
        .map(|v| {
            let m = v
                .iter()
                .map(|x| if *x < 0u32 { -x } else { x.clone() })
                .max()
                .unwrap_or_default();
            if m == 0u32 { v } else { v.map(|x| x / &m) }
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// The bounding box of `ps` (non-empty), rounded outward.
fn bounding(ps: &[Q3]) -> [[f64; 3]; 2] {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in ps {
        for k in 0..3 {
            let (l, _) = f64::rounding_from(&p[k], Floor);
            let (h, _) = f64::rounding_from(&p[k], Ceiling);
            if l.partial_cmp(&lo[k]) == Some(Ordering::Less) {
                lo[k] = l;
            }
            if h.partial_cmp(&hi[k]) == Some(Ordering::Greater) {
                hi[k] = h;
            }
        }
    }
    [lo, hi]
}

fn is_zero(v: &Q3) -> bool {
    v.iter().all(|x| *x == 0u32)
}

fn sub3(a: &Q3, b: &Q3) -> Q3 {
    [0, 1, 2].map(|i| &a[i] - &b[i])
}

fn neg3(a: &Q3) -> Q3 {
    [0, 1, 2].map(|i| -&a[i])
}

#[cfg(test)]
mod tests;
