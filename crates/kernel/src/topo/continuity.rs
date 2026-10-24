//! The continuity of an edge: how smoothly its two faces meet, certified
//! on the stored geometry ([`certify`]), and the two certificates it is
//! made of, which the check reads again ([`Join`]).
//!
//! **The label**, in this order:
//! - a singular edge, an edge whose curve is one point (zero-length: no
//!   tangent to certify with) and an edge with fewer than two faces are a
//!   mixed crease, uncertified;
//! - an edge of the same form is curvature-continuous: its faces lie on
//!   one form;
//! - tangent when the angle between the faces' outward normals is
//!   certified at most the tangent threshold `θ_tan` (1°) along the whole
//!   edge, the achieved bound kept;
//! - else a crease, convex or concave when the sign of `(n₁ × n₂)·T` is
//!   certified along the whole edge, mixed when it is certified to change
//!   or isn't settled. A mixed crease is a label, never a cut: the edge is
//!   not split for it, and its consumers test it again where they need a
//!   sign.
//!
//! A construction that declares a join curvature-continuous (a designed
//! `G²` joint) labels it so itself: the store keeps no record to certify
//! it from.
//!
//! **The cells.** Along the edge, with `c₁` its coedge of the lower id on
//! face `f₁` and `c₂` the other on `f₂`, the edge is cut at the knots of
//! both p-curves and of the curve inside its range, each piece in one span
//! of each and one tile of each surface. On a cell of a piece the fold
//! numerator `Nₖ` of each tile (a positive multiple of its surface's
//! normal, [`Tile::fold_numerator_at`]) is composed with the p-curve's
//! span in the tile's local coordinates (a positive multiple again):
//! Bernstein forms in the cell's parameter.
//! - **Crease**: with the curve's tangent numerator `T' = W·X' − W'·X`,
//!   `det[N₁, N₂, T']` has the crease's sign, turned by each face's
//!   `flipped` and `c₁`'s direction (degree `2·(5·k₁ + 5·k₂) + 2n − 1`, 43
//!   for quadratic spans). A cell holds a sign when every coefficient has
//!   it, certified; an end coefficient (the value there) certified of a
//!   sign shows that sign at a point.
//! - **Tangent**: with `σ` the product of the faces' orientations, the
//!   angle's tangent is `|N₁ × N₂| / (σ·N₁·N₂)`. By the convex hull of the
//!   Bernstein coefficients it is at most `r = max |cᵢ| / min dᵢ` on the
//!   cell, `cᵢ` the coefficients of `N₁ × N₂` and `dᵢ` of `σ·N₁·N₂` (all
//!   certified positive), each rounded up; the angle is at most
//!   `atan r ≤ r`, so `r` is a bound on the angle in radians. An end of
//!   the cell where `|c| > t·σd` is certified (or `σd ≤ 0`) shows an angle
//!   whose tangent is above `t`.
//!
//! A cell that decides nothing is computed again one level up where the
//! rounded values would decide it (rounding decides), else halved, the
//! left half first; each cell is charged as a seam-bound cell at its
//! level. The deepest subdivision, the top level and a fixed number of
//! cells per edge ([`cell_limit`]: a sign that is zero on a whole interval
//! would otherwise halve to the bottom everywhere) end the certificate
//! open.
//!
//! **Bands.** The tangent certificate stops once some end shows an angle
//! above `θ_tan/2` (tangent value [`TAN_HALF_THETA`]): the bands
//! overlap on `(θ_tan/2, θ_tan]`, so it ends without halving to the
//! bottom, first certified wins. A crease of a constant 0.9° may thus be
//! labelled a crease of its sign. The crease search stops, mixed, once
//! both signs are shown, at a cell or at a point.
//!
//! **Checking a label.** The check reads the same cells: a claimed sign
//! must hold on every cell, and fails where a point shows the other; a
//! tangent's stored bound `r` must be reached on every cell (target `r`)
//! and fails where a point shows an angle above it. Whether a cell climbs
//! a level or is halved is decided against `θ_tan` whatever the target,
//! so a bound [`certify`] stored refines to itself: the same cells, each
//! within the largest of them.

use varde_decide::bezier::{HomCurve, Poly, cross, dot};
use varde_decide::num::bounded::{add_up, div_up, down, mul_up, up};
use varde_decide::num::level::{AtLevel, LevelScalar, at_level};
use varde_decide::num::schedule::Level;

use crate::geom::certify::{restrict, tile_of};
use crate::geom::curve::{Curve3D, local};
use crate::geom::pcurve::PCurve;
use crate::geom::surface::Surface;
use crate::geom::tile::Tile;
use crate::infra::budget::{
    Cap, LEVELS, Ledger, MAX_SUBDIV_DEPTH, Step, Stop, level_step, seam_cell_units,
};
use crate::topo::brep::{Brep, CoedgeId, Continuity, Convexity, EdgeId, EdgeKind, FaceId};

#[cfg(test)]
mod tests;

/// The tangent threshold `θ_tan`: 1° (radians).
pub(crate) const THETA_TAN: f64 = std::f64::consts::PI / 180.0;

/// At least `tan(θ_tan/2)`: an angle whose tangent is certified above it
/// is certified above `θ_tan/2`.
pub(crate) const TAN_HALF_THETA: f64 = 0.008_726_867_790_76;

/// Cells beyond four per piece an edge's certificate may compute: twice
/// the deepest subdivision for each of four isolated zeros.
const EXTRA_CELLS: usize = 8 * MAX_SUBDIV_DEPTH as usize;

/// The most cells one certificate computes on `pieces` pieces.
fn cell_limit(pieces: usize) -> usize {
    pieces.saturating_mul(4).saturating_add(EXTRA_CELLS)
}

/// The continuity of edge `e` of `b` (module docs), charged to `ledger`.
pub(crate) fn certify<E>(
    b: &Brep,
    e: EdgeId,
    ledger: &mut Ledger<'_>,
) -> Result<Continuity, Stop<E>> {
    let mixed = Continuity::Crease(Convexity::Mixed);
    let Some(kind) = at(&b.edges.kind, e.0) else {
        return Ok(mixed);
    };
    ledger.charge(Step::Work, 1)?;
    if kind == EdgeKind::Singular || zero_length(b, e) {
        return Ok(mixed);
    }
    if matches!(kind, EdgeKind::SameForm { .. }) {
        return Ok(Continuity::Curvature);
    }
    let Some((coedges, faces)) = two_faces(b, e) else {
        return Ok(mixed);
    };
    let Some(join) = Join::new(b, e, coedges, faces, ledger)? else {
        return Ok(mixed);
    };
    if let Tangency::Within(angle) = join.tangent(THETA_TAN, TAN_HALF_THETA, ledger)? {
        return Ok(Continuity::Tangent { angle });
    }
    Ok(Continuity::Crease(join.find_crease(ledger)?))
}

/// `v[i]`, if there is one.
fn at<T: Copy>(v: &[T], i: u32) -> Option<T> {
    v.get(i as usize).copied()
}

pub(crate) fn same_bits<const N: usize>(a: &[f64; N], b: &[f64; N]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

/// A coedge's face, `None` where its id names nothing.
pub(crate) fn coedge_face(b: &Brep, c: CoedgeId) -> Option<FaceId> {
    at(&b.coedges.face, c.0).filter(|f| f.index() < b.face_count())
}

/// A coedge's p-curve, `None` where an id names nothing.
pub(crate) fn pcurve(b: &Brep, c: CoedgeId) -> Option<&PCurve> {
    b.pcurves.get(at(&b.coedges.pcurve, c.0)?.index())
}

/// A face's surface, `None` where an id names nothing.
pub(crate) fn face_surface(b: &Brep, f: FaceId) -> Option<&Surface> {
    b.surfaces.surface.get(at(&b.faces.surface, f.0)?.index())
}

/// An edge's curve, `None` where its id names nothing.
pub(crate) fn edge_curve(b: &Brep, e: EdgeId) -> Option<&Curve3D> {
    b.curves3.get(at(&b.edges.curve, e.0)?.index())
}

/// The position of `t` among `knots`, bit for bit.
pub(crate) fn knot_index(knots: &[f64], t: f64) -> Option<usize> {
    knots.iter().position(|k| k.to_bits() == t.to_bits())
}

/// Whether edge `e`'s curve is one point over its range: a zero-length
/// edge.
pub(crate) fn zero_length(b: &Brep, e: EdgeId) -> bool {
    let (Some(curve), Some([t0, t1])) = (edge_curve(b, e), at(&b.edges.t_range, e.0)) else {
        return false;
    };
    let (Some(i), Some(j)) = (knot_index(curve.knots(), t0), knot_index(curve.knots(), t1)) else {
        return false;
    };
    let Some(spans) = curve.spans().get(i..j) else {
        return false;
    };
    let mut points = spans.iter().flatten().map(|p| [p[0], p[1], p[2]]);
    let first = points.next();
    first.is_some_and(|f| points.all(|p| same_bits(&p, &f)))
}

/// Edge `e`'s two coedges, the lower id first, and their faces: its
/// coedge and that one's pair, each the other's, both on `e`.
fn two_faces(b: &Brep, e: EdgeId) -> Option<([CoedgeId; 2], [FaceId; 2])> {
    let c = at(&b.edges.coedge, e.0)?;
    let p = at(&b.coedges.pair, c.0)?;
    let on_e = |x: CoedgeId| at(&b.coedges.edge, x.0) == Some(e);
    if p == c || at(&b.coedges.pair, p.0) != Some(c) || !on_e(c) || !on_e(p) {
        return None;
    }
    let pair = if p.0 < c.0 { [p, c] } else { [c, p] };
    Some((pair, [coedge_face(b, pair[0])?, coedge_face(b, pair[1])?]))
}

/// A piece of an edge: `[a, b]` of `t`, in span `sp[k]` of p-curve `k`
/// on tile `tiles[k]` (with its indices) and span `sc` of the curve.
struct Piece<'a> {
    tiles: [(&'a Tile, i64, i64); 2],
    sp: [usize; 2],
    sc: usize,
    a: f64,
    b: f64,
}

/// Part `k` of `2^depth` of a piece, at a precision level.
#[derive(Clone, Copy, Debug)]
struct Cell {
    piece: usize,
    k: u64,
    depth: u32,
    level: u8,
}

/// The geometry of an edge between two faces, cut into its pieces: what
/// both certificates read (module docs).
pub(crate) struct Join<'a> {
    curve: &'a Curve3D,
    p: [&'a PCurve; 2],
    pieces: Vec<Piece<'a>>,
    /// Whether `det[N₁, N₂, T']` is positive where the crease is convex.
    convex_positive: bool,
    /// Whether `N₁·N₂` is positive where the outward normals agree.
    agree_positive: bool,
}

/// What a cell's forms show, the cell's other outcomes aside.
enum Shown<T> {
    /// A tile whose fold check stops: no normal to certify with.
    Folded,
    /// A form past the greatest degree.
    Degree,
    /// The forms' verdict.
    Forms(T),
}

/// The signs a cell's crease form shows, in the convex sense (positive
/// convex).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Signs {
    /// Every coefficient certified positive.
    all_pos: bool,
    /// Every coefficient certified negative.
    all_neg: bool,
    /// An end coefficient certified positive.
    end_pos: bool,
    /// An end coefficient certified negative.
    end_neg: bool,
    /// Every coefficient's value positive (`Some(true)`) or negative.
    rounded: Option<bool>,
}

/// What a tangent cell shows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Tan {
    /// The angle at most this bound (radians), at most the target.
    Within(f64),
    /// An angle above the far threshold at an end.
    Beyond,
    /// Compute it again one level up.
    Precision,
    /// Halve it.
    Space,
}

/// How a cell is followed up.
enum Visit<R> {
    /// The cell is decided; go on.
    Done,
    /// The certificate is decided.
    Stop(R),
    /// Compute it again one level up.
    Precision,
    /// Halve it.
    Space,
}

/// How the cells ran out.
enum Walk<R> {
    /// Every cell decided.
    Finished,
    /// A cell decided the certificate.
    Stopped(R),
    /// A fixed limit reached, the certificate open.
    Cap(Cap),
}

/// A crease sign checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CreaseCheck {
    /// Certified on every cell.
    Holds,
    /// The other sign certified at a point, or a tile without a normal.
    Wrong,
    /// A form past the greatest degree: not built.
    Degree,
    /// Open at a fixed limit.
    Cap(Cap),
}

/// The tangent certificate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Tangency {
    /// Within the target on every cell; the largest cell bound.
    Within(f64),
    /// An angle above the far threshold at a point, or a tile without a
    /// normal.
    Beyond,
    /// A form past the greatest degree: not built.
    Degree,
    /// Open at a fixed limit.
    Cap(Cap),
}

impl<'a> Join<'a> {
    /// The join of edge `e` between `faces` (`coedges` their uses of it,
    /// the first's direction the tangent's), cut into its pieces, the cuts
    /// charged; `None` where the body can't be read so (the check's
    /// earlier invariants refuse it).
    pub(crate) fn new<E>(
        b: &'a Brep,
        e: EdgeId,
        coedges: [CoedgeId; 2],
        faces: [FaceId; 2],
        ledger: &mut Ledger<'_>,
    ) -> Result<Option<Join<'a>>, Stop<E>> {
        let (Some(curve), Some(p1), Some(p2), Some([t0, t1])) = (
            edge_curve(b, e),
            pcurve(b, coedges[0]),
            pcurve(b, coedges[1]),
            at(&b.edges.t_range, e.0),
        ) else {
            return Ok(None);
        };
        let (Some(s1), Some(s2)) = (face_surface(b, faces[0]), face_surface(b, faces[1])) else {
            return Ok(None);
        };
        let (Some(flip1), Some(flip2), Some(rev)) = (
            at(&b.faces.flipped, faces[0].0),
            at(&b.faces.flipped, faces[1].0),
            at(&b.coedges.reversed, coedges[0].0),
        ) else {
            return Ok(None);
        };
        let ck = curve.knots();
        let over = |pk: &[f64]| {
            pk.first().map(|t| t.to_bits()) == Some(t0.to_bits())
                && pk.last().map(|t| t.to_bits()) == Some(t1.to_bits())
        };
        if !(over(p1.knots()) && over(p2.knots())) || knot_index(ck, t0).is_none() {
            return Ok(None);
        }
        let mut cuts: Vec<f64> = p1
            .knots()
            .iter()
            .chain(p2.knots())
            .chain(ck.iter().filter(|&&t| t0 < t && t < t1))
            .copied()
            .collect();
        cuts.sort_by(f64::total_cmp);
        cuts.dedup();
        ledger.charge(Step::Work, u64::try_from(cuts.len()).unwrap_or(u64::MAX))?;
        let mut pieces = Vec::with_capacity(cuts.len());
        for w in cuts.windows(2) {
            let (a, z) = (w[0], w[1]);
            let span = |knots: &[f64]| knots.partition_point(|&k| k <= a) - 1;
            let sp = [span(p1.knots()), span(p2.knots())];
            let (Some(a1), Some(a2)) = (
                tile_of(s1, &p1.spans()[sp[0]]),
                tile_of(s2, &p2.spans()[sp[1]]),
            ) else {
                return Ok(None);
            };
            pieces.push(Piece {
                tiles: [a1, a2],
                sp,
                sc: span(ck),
                a,
                b: z,
            });
        }
        Ok(Some(Join {
            curve,
            p: [p1, p2],
            pieces,
            convex_positive: !(flip1 ^ flip2 ^ rev),
            agree_positive: !(flip1 ^ flip2),
        }))
    }

    /// Runs `visit` on the cells, the next one last on a stack, each
    /// charged as a seam cell at its level.
    fn walk<E, R>(
        &self,
        ledger: &mut Ledger<'_>,
        mut visit: impl FnMut(&Self, Cell, Level) -> Visit<R>,
    ) -> Result<Walk<R>, Stop<E>> {
        let limit = cell_limit(self.pieces.len());
        let mut cells: Vec<Cell> = (0..self.pieces.len())
            .rev()
            .map(|piece| Cell {
                piece,
                k: 0,
                depth: 0,
                level: 0,
            })
            .collect();
        let mut seen = 0usize;
        while let Some(cell) = cells.pop() {
            if seen == limit {
                return Ok(Walk::Cap(Cap::SubdivisionDepth));
            }
            seen += 1;
            ledger.charge(level_step(cell.level), seam_cell_units(cell.level))?;
            let level = Level::from_index(usize::from(cell.level)).unwrap_or(Level::TOP);
            match visit(self, cell, level) {
                Visit::Done => {}
                Visit::Stop(r) => return Ok(Walk::Stopped(r)),
                Visit::Precision if cell.level + 1 < LEVELS => cells.push(Cell {
                    level: cell.level + 1,
                    ..cell
                }),
                Visit::Precision => return Ok(Walk::Cap(Cap::PrecisionBits)),
                Visit::Space if cell.depth < MAX_SUBDIV_DEPTH => {
                    // The left half on top.
                    for k in [2 * cell.k + 1, 2 * cell.k] {
                        cells.push(Cell {
                            k,
                            depth: cell.depth + 1,
                            ..cell
                        });
                    }
                }
                Visit::Space => return Ok(Walk::Cap(Cap::SubdivisionDepth)),
            }
        }
        Ok(Walk::Finished)
    }

    fn signs(&self, cell: Cell, level: Level) -> Shown<Signs> {
        let mut at = CellAt {
            join: self,
            cell,
            form: CreaseForm,
        };
        at_level(&mut at, level, &mut ())
    }

    /// Whether the crease has sign `sign` (convex or concave) on every
    /// cell (module docs).
    pub(crate) fn check_crease<E>(
        &self,
        sign: Convexity,
        ledger: &mut Ledger<'_>,
    ) -> Result<CreaseCheck, Stop<E>> {
        let convex = sign == Convexity::Convex;
        let walk = self.walk(ledger, |join, cell, level| {
            let s = match join.signs(cell, level) {
                Shown::Folded => return Visit::Stop(CreaseCheck::Wrong),
                Shown::Degree => return Visit::Stop(CreaseCheck::Degree),
                Shown::Forms(s) => s,
            };
            let (all, other_end) = if convex {
                (s.all_pos, s.end_neg)
            } else {
                (s.all_neg, s.end_pos)
            };
            if all {
                Visit::Done
            } else if other_end {
                Visit::Stop(CreaseCheck::Wrong)
            } else if s.rounded == Some(convex) {
                Visit::Precision
            } else {
                Visit::Space
            }
        })?;
        Ok(match walk {
            Walk::Finished => CreaseCheck::Holds,
            Walk::Stopped(r) => r,
            Walk::Cap(cap) => CreaseCheck::Cap(cap),
        })
    }

    /// The crease's sign: convex or concave where certified on every
    /// cell, mixed where both signs are shown or it isn't settled.
    pub(crate) fn find_crease<E>(&self, ledger: &mut Ledger<'_>) -> Result<Convexity, Stop<E>> {
        let mut seen = Seen::default();
        let walk = self.walk(ledger, |join, cell, level| {
            seen.visit(join.signs(cell, level))
        })?;
        Ok(match walk {
            Walk::Finished => seen.sign(),
            Walk::Stopped(()) | Walk::Cap(_) => Convexity::Mixed,
        })
    }

    /// The tangent certificate with target `target` and far threshold
    /// `far` (a tangent value): within `target` on every cell, or an angle
    /// whose tangent is above `far` shown at a point (module docs).
    pub(crate) fn tangent<E>(
        &self,
        target: f64,
        far: f64,
        ledger: &mut Ledger<'_>,
    ) -> Result<Tangency, Stop<E>> {
        let mut bound = 0.0f64;
        let walk = self.walk(ledger, |join, cell, level| {
            let mut at = CellAt {
                join,
                cell,
                form: TangentForm { target, far },
            };
            match at_level(&mut at, level, &mut ()) {
                Shown::Folded => Visit::Stop(Tangency::Beyond),
                Shown::Degree => Visit::Stop(Tangency::Degree),
                Shown::Forms(Tan::Within(r)) => {
                    if r > bound {
                        bound = r;
                    }
                    Visit::Done
                }
                Shown::Forms(Tan::Beyond) => Visit::Stop(Tangency::Beyond),
                Shown::Forms(Tan::Precision) => Visit::Precision,
                Shown::Forms(Tan::Space) => Visit::Space,
            }
        })?;
        Ok(match walk {
            Walk::Finished => Tangency::Within(bound),
            Walk::Stopped(r) => r,
            Walk::Cap(cap) => Tangency::Cap(cap),
        })
    }
}

/// The signs the crease search has seen so far.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Seen {
    pos: bool,
    neg: bool,
}

impl Seen {
    /// Takes in one cell's verdict: a whole cell of one sign is decided,
    /// a point of either sign is noted; both signs seen stop the search
    /// (mixed); a cell without a normal or past the degree is mixed.
    fn visit(&mut self, shown: Shown<Signs>) -> Visit<()> {
        let s = match shown {
            Shown::Folded | Shown::Degree => return Visit::Stop(()),
            Shown::Forms(s) => s,
        };
        self.pos |= s.all_pos || s.end_pos;
        self.neg |= s.all_neg || s.end_neg;
        if self.pos && self.neg {
            Visit::Stop(())
        } else if s.all_pos || s.all_neg {
            Visit::Done
        } else if s.rounded.is_some() {
            Visit::Precision
        } else {
            Visit::Space
        }
    }

    /// The sign once every cell is decided.
    fn sign(self) -> Convexity {
        match (self.pos, self.neg) {
            (true, false) => Convexity::Convex,
            (false, true) => Convexity::Concave,
            _ => Convexity::Mixed,
        }
    }
}

/// `c` halved down to `cell`: the halves `cell.k`'s bits name, from the
/// top.
fn halved<S: LevelScalar, const N: usize>(mut c: HomCurve<S, N>, cell: Cell) -> HomCurve<S, N> {
    for bit in (0..cell.depth).rev() {
        let [l, r] = c.split_half();
        c = if (cell.k >> bit) & 1 == 1 { r } else { l };
    }
    c
}

/// What a cell computes from its normals.
trait CellForm {
    type Out;

    /// The verdict of the cell from the composed normal numerators
    /// `n`, at `like`'s level.
    fn verdict<S: LevelScalar>(
        &self,
        join: &Join<'_>,
        cell: Cell,
        n: [[Poly<S>; 3]; 2],
        like: &S,
    ) -> Shown<Self::Out>;
}

/// One cell at one level.
struct CellAt<'j, 'a, F> {
    join: &'j Join<'a>,
    cell: Cell,
    form: F,
}

impl<F: CellForm> CellAt<'_, '_, F> {
    /// The fold numerators of both tiles composed with the p-curves'
    /// spans over the cell: positive multiples of the surfaces' normals.
    fn normals<S: LevelScalar>(&self, like: &S) -> Shown<[[Poly<S>; 3]; 2]> {
        let piece = &self.join.pieces[self.cell.piece];
        let mut normals: Vec<[Poly<S>; 3]> = Vec::with_capacity(2);
        for side in 0..2 {
            let (p, sp) = (self.join.p[side], piece.sp[side]);
            let pk = p.knots();
            let (a, z) = (
                local(piece.a, pk[sp], pk[sp + 1], like),
                local(piece.b, pk[sp], pk[sp + 1], like),
            );
            let Some(coords) = p.coords_at(sp, like) else {
                return Shown::Degree;
            };
            let coords = coords.map(|x| halved(restrict(&x, a.clone(), z.clone()), self.cell));
            let (tile, i, j) = piece.tiles[side];
            // Tile indices below 2⁵² in magnitude (`tile_of`): exact.
            let [pu, pv] = [(&coords[0], i), (&coords[1], j)]
                .map(|(x, k)| x.translated(&[S::from_f64(k as f64, like)]));
            let Some(n) = tile.fold_numerator_at(like) else {
                return Shown::Folded;
            };
            let composed: Option<Vec<Poly<S>>> = n
                .iter()
                .map(|t| {
                    t.compose_params(
                        [&pu.coords()[0], pu.weights()],
                        [&pv.coords()[0], pv.weights()],
                    )
                })
                .collect();
            let Some(n) = composed.and_then(|v| <[Poly<S>; 3]>::try_from(v).ok()) else {
                return Shown::Degree;
            };
            normals.push(n);
        }
        match <[[Poly<S>; 3]; 2]>::try_from(normals) {
            Ok(n) => Shown::Forms(n),
            Err(_) => Shown::Degree,
        }
    }
}

impl<F: CellForm> AtLevel<()> for CellAt<'_, '_, F> {
    type Out = Shown<F::Out>;

    fn at<S: LevelScalar>(&mut self, like: &S, _: &mut ()) -> Shown<F::Out> {
        match self.normals(like) {
            Shown::Forms(n) => self.form.verdict(self.join, self.cell, n, like),
            Shown::Folded => Shown::Folded,
            Shown::Degree => Shown::Degree,
        }
    }

    /// The caller climbs the levels and halves the cells itself.
    fn escalates(_: &Shown<F::Out>) -> bool {
        false
    }
}

/// The crease form `det[N₁, N₂, T']`.
struct CreaseForm;

impl CellForm for CreaseForm {
    type Out = Signs;

    fn verdict<S: LevelScalar>(
        &self,
        join: &Join<'_>,
        cell: Cell,
        n: [[Poly<S>; 3]; 2],
        like: &S,
    ) -> Shown<Signs> {
        let piece = &join.pieces[cell.piece];
        let ck = join.curve.knots();
        let sc = piece.sc;
        let Some(c) = join.curve.span_at(sc, like) else {
            return Shown::Degree;
        };
        let c = halved(
            restrict(
                &c,
                local(piece.a, ck[sc], ck[sc + 1], like),
                local(piece.b, ck[sc], ck[sc + 1], like),
            ),
            cell,
        );
        let g = c
            .tangent_numerator()
            .and_then(|t| dot(&cross(&n[0], &n[1])?, &t));
        match g {
            Some(g) => Shown::Forms(signs(g.coefs(), join.convex_positive)),
            None => Shown::Degree,
        }
    }
}

/// The signs of a crease form with coefficients `coefs`, turned positive
/// where convex.
fn signs<S: LevelScalar>(coefs: &[S], convex_positive: bool) -> Signs {
    let turned = |c: &S| {
        let [lo, hi] = c.enclose();
        if convex_positive {
            [lo, hi]
        } else {
            [-hi, -lo]
        }
    };
    let ends = [coefs.first(), coefs.last()];
    let value = |c: &S| {
        if convex_positive {
            c.approx()
        } else {
            -c.approx()
        }
    };
    let rounded = if coefs.iter().all(|c| value(c) > 0.0) {
        Some(true)
    } else if coefs.iter().all(|c| value(c) < 0.0) {
        Some(false)
    } else {
        None
    };
    Signs {
        all_pos: !coefs.is_empty() && coefs.iter().all(|c| turned(c)[0] > 0.0),
        all_neg: !coefs.is_empty() && coefs.iter().all(|c| turned(c)[1] < 0.0),
        end_pos: ends.into_iter().flatten().any(|c| turned(c)[0] > 0.0),
        end_neg: ends.into_iter().flatten().any(|c| turned(c)[1] < 0.0),
        rounded,
    }
}

/// The tangent forms `N₁ × N₂` and `σ·N₁·N₂`, against a target bound and
/// a far threshold.
struct TangentForm {
    target: f64,
    far: f64,
}

impl CellForm for TangentForm {
    type Out = Tan;

    fn verdict<S: LevelScalar>(
        &self,
        join: &Join<'_>,
        _: Cell,
        n: [[Poly<S>; 3]; 2],
        _: &S,
    ) -> Shown<Tan> {
        let (Some(c), Some(d)) = (cross(&n[0], &n[1]), dot(&n[0], &n[1])) else {
            return Shown::Degree;
        };
        Shown::Forms(tan_verdict(
            [c[0].coefs(), c[1].coefs(), c[2].coefs()],
            d.coefs(),
            join.agree_positive,
            self.target,
            self.far,
        ))
    }
}

/// The verdict of a tangent cell whose cross product has the coefficients
/// `c` (per coordinate) and dot product `d`, `agree` whether `d` is
/// positive where the outward normals agree (module docs).
fn tan_verdict<S: LevelScalar>(c: [&[S]; 3], d: &[S], agree: bool, target: f64, far: f64) -> Tan {
    let oriented = |x: &S| {
        let [lo, hi] = x.enclose();
        if agree { [lo, hi] } else { [-hi, -lo] }
    };
    let n = d.len();
    if n == 0 || c.iter().any(|k| k.len() != n) {
        return Tan::Space;
    }
    // The bound over the cell: `max |cᵢ|` over `min dᵢ`, rounded up.
    let d_min = d
        .iter()
        .map(|x| oriented(x)[0])
        .fold(f64::INFINITY, |m, v| if v < m { v } else { m });
    if d_min > 0.0 {
        let mut c_max2 = 0.0f64;
        for i in 0..n {
            let mut s = 0.0f64;
            for k in c {
                let [lo, hi] = k[i].enclose();
                let m = if -lo > hi { -lo } else { hi };
                s = add_up(s, mul_up(m, m));
            }
            if s > c_max2 {
                c_max2 = s;
            }
        }
        let r = div_up(up(c_max2.sqrt()), d_min);
        if r <= target {
            return Tan::Within(r);
        }
    }
    // An end showing an angle whose tangent is above `far`.
    for i in [0, n - 1] {
        let d_hi = oriented(&d[i])[1];
        if d_hi <= 0.0 {
            return Tan::Beyond;
        }
        let mut s = 0.0f64;
        for k in c {
            let [lo, hi] = k[i].enclose();
            let m = if lo > 0.0 {
                lo
            } else if hi < 0.0 {
                -hi
            } else {
                0.0
            };
            s += nonneg(down(m * m));
        }
        let norm = down(nonneg(down(s)).sqrt());
        if norm > mul_up(far, d_hi) {
            return Tan::Beyond;
        }
    }
    // Climb a level where the rounded values reach `θ_tan`, else halve.
    let sign = if agree { 1.0 } else { -1.0 };
    let d_val: Vec<f64> = d.iter().map(|x| sign * x.approx()).collect();
    if d_val.iter().all(|&v| v > 0.0) {
        let d_least = d_val
            .iter()
            .fold(f64::INFINITY, |m, &v| if v < m { v } else { m });
        let c_most2 = (0..n)
            .map(|i| c.iter().map(|k| k[i].approx() * k[i].approx()).sum::<f64>())
            .fold(0.0f64, |m, v| if v > m { v } else { m });
        if c_most2.sqrt() / d_least <= THETA_TAN {
            return Tan::Precision;
        }
    }
    Tan::Space
}

/// `x`, or 0 where it is below.
fn nonneg(x: f64) -> f64 {
    if x > 0.0 { x } else { 0.0 }
}
