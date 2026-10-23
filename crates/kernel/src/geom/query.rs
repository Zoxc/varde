//! Non-decision queries: **trim membership**, the winding number of a
//! face's trimming curves about a UV point, and **extrema**, certified
//! bounds of the least and greatest value of a direction over curves and
//! tiles. They serve the measures (mass properties over trimmed regions,
//! tight bounds) and finding named entities again; **no decision of an
//! operation reads them**: an operation's topology comes from its records
//! alone, decided once, and these answers are refined to a caller's
//! resolution, not decided. The crate's lint test (`lint` below) keeps
//! every reference to this module inside it and the measures'.
//!
//! **Trim membership** ([`winding`]). The trims are p-curves in the face's
//! global UV, each with whether its coedge runs against it; together they
//! must close (every point a trim starts at is where as many end, bit for
//! bit), and their winding number about a point inside the face is the
//! face's inside winding (the boolean counting's convention: `+1` round a
//! point of an unflipped face, `−1` of a flipped one, `0` outside). The
//! winding about `p` is counted along the ray from `p` along `+u`, taken
//! at the height `v_p − ε` (`ε` infinitely small): a trim point with
//! `v ≥ v_p` lies above it, one with `v < v_p` below. Each span is first
//! tested by its stored control values (strictly above, below or left of
//! `p`: no crossing; strictly right: the change of side between its ends,
//! exact on its stored end points). Any other span is cut into pieces, a
//! piece the `k`-th of `2^d` halvings of the span, each tested in the
//! line equations through `p` (`u − u_p`, `v − v_p`, times the positive
//! weight) by its coefficients' hull: **above**, **below**, **left** or
//! **right** of `p` when certified so, or **near** when certified inside
//! the near box `[u_p ± near_u] × [v_p ± near_v]`. Along a run of right
//! pieces the ray is crossed by half the change of side from the run's
//! start to its end, those sides read from the pieces above or below
//! beside the run or the span's exact ends. A piece certified near ends
//! the query: the point lies within the near box of a trim
//! ([`Membership::Near`]), its membership not refined further. So a point
//! on a trim is always near, and a point whose box is clear of the trims
//! always gets its winding. The near box is in UV: a caller with a
//! length in mm divides it by its bound of the surface's speed along
//! each direction.
//!
//! **Extrema** ([`curve_extrema`], [`pcurve_extrema`], [`tile_extrema`]).
//! The value `a·x` of a direction `a` over each span or tile is a
//! rational Bernstein form `N/D`, `N = a·X` and `D` the weights, positive.
//! On a cell (a span's `k`-th of `2^d` halvings; a tile's, halved in `u`
//! at even depths and in `v` at odd ones) the value lies between the
//! least and greatest coefficient ratio `Nᵢ/Dᵢ` (a convex combination,
//! the weights positive), and its corners' ratios are values it takes.
//! So the least ratio bounds the least value from below and the least
//! corner ratio from above. Branch and bound: the cells in order of their
//! lower bound (then by root, depth and position, so the order is the
//! inputs' alone); the least is enclosed once the least lower bound left
//! is within the resolution of the least corner value found; the cell
//! with the least lower bound is otherwise refined. The greatest is the
//! least of `−a·x`, the direction negated exactly.
//!
//! **Escalation: space, then precision** (as the seam bound,
//! [`certify`](super::certify)). A piece or cell is formed at a level of
//! the precision schedule, from the stored values (exact at every level
//! but the first, certified there). One that its certified hulls don't
//! settle is computed again one level up where its coefficients' values
//! alone would settle it (a trim piece), or where the rounding of its
//! lower bound exceeds its hull's spread (an extremum cell); else it is
//! halved, its halves at its level. A trim piece open at the top level
//! and at [`MAX_SUBDIV_DEPTH`] halvings stops the query at
//! [`Cap::SubdivisionDepth`] or [`Cap::PrecisionBits`]. An extremum cell
//! open at either limit is done with its lower bound: the extremum's
//! enclosure is then wider than the resolution, an achieved bound; one
//! with no certified lower bound there stops the query at the cap.
//!
//! **Charges.** Each piece and cell is charged before it is computed, at
//! the unit table's row for its kind and level (`UNITS.trim_piece`,
//! `UNITS.extremum_cell`), ordinary work at the first level and a
//! precision step above it; a trim span's test by its stored values is
//! charged as a first-level piece. Running out of the ledger stops the
//! query.

use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

use varde_decide::bezier::{Dir, HomCurve, Poly, Scalar, Tensor};
use varde_decide::num::level::{AtLevel, LevelScalar, at_level};
use varde_decide::num::schedule::Level;

use super::curve::Curve3D;
use super::pcurve::PCurve;
use super::tile::Tile;
use crate::infra::budget::{
    Cap, LEVELS, Ledger, MAX_SUBDIV_DEPTH, Stop, extremum_cell_units, level_step, trim_piece_units,
};

#[cfg(test)]
mod lint;
#[cfg(test)]
mod tests;

/// A face's trimming curve: a coedge's p-curve in the face's global UV and
/// whether the coedge runs against it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TrimCurve<'a> {
    /// The p-curve, along its edge's parameter.
    pub(crate) pcurve: &'a PCurve,
    /// Whether the coedge runs against the p-curve.
    pub(crate) reversed: bool,
}

/// Where a UV point lies against a face's trims (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Membership {
    /// The point is clear of every trim and they wind this many times
    /// round it: the face's inside winding inside it, `0` outside.
    Winding(i64),
    /// A trim passes through the point's near box: its membership isn't
    /// refined further.
    Near,
}

/// Why a query can't run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QueryError {
    /// The trims don't close: their ends don't pair up into cycles.
    Open,
    /// A point, direction or resolution isn't finite, a near box or a
    /// resolution isn't positive, or there is nothing to bound.
    Input,
}

/// The winding of `trims` about the UV point `uv`, refined until each
/// trim is certified clear of it or one passes through its near box
/// `[uv ± near]` (module docs), each piece charged to `ledger`.
pub(crate) fn winding(
    trims: &[TrimCurve<'_>],
    uv: [f64; 2],
    near: [f64; 2],
    ledger: &mut Ledger<'_>,
) -> Result<Membership, Stop<QueryError>> {
    let finite = uv.iter().chain(&near).all(|x| x.is_finite());
    if !finite || !near.iter().all(|&n| n > 0.0) {
        return Err(Stop::Failed(QueryError::Input));
    }
    if !closed(trims) {
        return Err(Stop::Failed(QueryError::Open));
    }
    let mut w = 0i64;
    for t in trims {
        for k in 0..t.pcurve.spans().len() {
            let n = match span_crossings(t.pcurve, k, uv, near, ledger)? {
                Crossings::Near => return Ok(Membership::Near),
                Crossings::Count(n) => n,
            };
            w = w.saturating_add(if t.reversed { -n } else { n });
        }
    }
    Ok(Membership::Winding(w))
}

/// Whether the trims' ends pair up: every point is the start of as many
/// trims (in their coedges' directions) as it is the end of, bit for bit
/// (a zero of either sign the same point).
fn closed(trims: &[TrimCurve<'_>]) -> bool {
    let key = |p: [f64; 2]| p.map(|x| (x + 0.0).to_bits());
    let mut starts = Vec::with_capacity(trims.len());
    let mut ends = Vec::with_capacity(trims.len());
    for t in trims {
        let n = t.pcurve.spans().len();
        let (Some(a), Some(b)) = (t.pcurve.knot_point(0), t.pcurve.knot_point(n)) else {
            return false;
        };
        let (a, b) = if t.reversed { (b, a) } else { (a, b) };
        starts.push(key(a));
        ends.push(key(b));
    }
    starts.sort_unstable();
    ends.sort_unstable();
    starts == ends
}

/// A span's signed crossings of the ray, `+1` upwards, or near.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Crossings {
    Count(i64),
    Near,
}

/// Where a piece of a span lies against the query point (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Piece {
    /// Above (`+1`) or below (`−1`) the ray's line, certified.
    Off(i8),
    /// Left of the point, certified: never crosses the ray.
    Left,
    /// Right of the point, certified.
    Right,
    /// Inside the near box, certified.
    Near,
    /// None of these.
    Open,
}

/// Part `k` of `2^depth` of a span or a root form, at a precision level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Cell {
    root: usize,
    depth: u32,
    k: u64,
    level: u8,
}

impl Cell {
    fn root(root: usize) -> Cell {
        Cell {
            root,
            depth: 0,
            k: 0,
            level: 0,
        }
    }

    /// The two halves, at its level, left first.
    fn halves(self) -> [Cell; 2] {
        [2 * self.k, 2 * self.k + 1].map(|k| Cell {
            k,
            depth: self.depth + 1,
            ..self
        })
    }

    /// The cell one level up.
    fn up(self) -> Cell {
        Cell {
            level: self.level + 1,
            ..self
        }
    }

    /// Whether the halving at `bit` (from the root, the deepest last) takes
    /// the right half.
    fn right(self, bit: u32) -> bool {
        (self.k >> (self.depth - 1 - bit)) & 1 == 1
    }
}

/// The level of a cell.
fn level_of(cell: Cell) -> Level {
    Level::from_index(usize::from(cell.level)).unwrap_or(Level::TOP)
}

/// Span `k` of `p`'s crossings of the ray from `uv` (module docs).
fn span_crossings(
    p: &PCurve,
    k: usize,
    uv: [f64; 2],
    near: [f64; 2],
    ledger: &mut Ledger<'_>,
) -> Result<Crossings, Stop<QueryError>> {
    let span = &p.spans()[k];
    ledger.charge(level_step(0), trim_piece_units(0))?;
    let side = |v: f64| if v >= uv[1] { 1i8 } else { -1 };
    let [lo, hi] = span.hull();
    let (first, last) = (side(span.start()[1]), side(span.end()[1]));
    if lo[1] > uv[1] || hi[1] < uv[1] || hi[0] < uv[0] {
        return Ok(Crossings::Count(0));
    }
    if lo[0] > uv[0] {
        return Ok(Crossings::Count(i64::from((last - first) / 2)));
    }
    let mut tally = Tally {
        n: 0,
        run: None,
        before: Some(first),
    };
    // A stack: the next piece last, so the pieces come in order.
    let mut cells = vec![Cell::root(k)];
    while let Some(cell) = cells.pop() {
        ledger.charge(level_step(cell.level), trim_piece_units(cell.level))?;
        let (piece, settles) = at_level(
            &mut PieceAt {
                pcurve: p,
                cell,
                uv,
                near,
            },
            level_of(cell),
            &mut (),
        );
        match piece {
            Piece::Near => return Ok(Crossings::Near),
            Piece::Open => {
                let up = cell.level + 1 < LEVELS;
                let down = cell.depth < MAX_SUBDIV_DEPTH;
                if (settles || !down) && up {
                    cells.push(cell.up());
                } else if down {
                    let [l, r] = cell.halves();
                    cells.push(r);
                    cells.push(l);
                } else if settles {
                    return Err(Stop::Cap(Cap::PrecisionBits));
                } else {
                    return Err(Stop::Cap(Cap::SubdivisionDepth));
                }
            }
            piece => {
                if !tally.push(piece) {
                    // Pieces next to each other share a point, which
                    // can't be both left and right of the query point.
                    debug_assert!(false, "a left piece beside a right one");
                    return Ok(Crossings::Near);
                }
            }
        }
    }
    Ok(Crossings::Count(tally.finish(last)))
}

/// The crossings of a span's pieces, in order (module docs).
struct Tally {
    n: i64,
    /// The side where the current run of right pieces started.
    run: Option<i8>,
    /// The side at the boundary before the next piece, where known.
    before: Option<i8>,
}

impl Tally {
    /// Takes the next piece (not near, not open); `false` where it
    /// contradicts the one before.
    fn push(&mut self, piece: Piece) -> bool {
        match piece {
            Piece::Off(side) => {
                if let Some(start) = self.run.take() {
                    self.n += i64::from((side - start) / 2);
                }
                self.before = Some(side);
            }
            Piece::Left => {
                if self.run.is_some() {
                    return false;
                }
                self.before = None;
            }
            Piece::Right => {
                if self.run.is_none() {
                    let Some(start) = self.before else {
                        return false;
                    };
                    self.run = Some(start);
                }
                self.before = None;
            }
            Piece::Near | Piece::Open => return false,
        }
        true
    }

    /// The crossings, the span ending on side `last`.
    fn finish(self, last: i8) -> i64 {
        match self.run {
            Some(start) => self.n + i64::from((last - start) / 2),
            None => self.n,
        }
    }
}

/// A trim piece at one precision level ([`piece_at`]).
struct PieceAt<'p> {
    pcurve: &'p PCurve,
    cell: Cell,
    uv: [f64; 2],
    near: [f64; 2],
}

impl AtLevel<()> for PieceAt<'_> {
    type Out = (Piece, bool);

    fn at<S: LevelScalar>(&mut self, like: &S, _: &mut ()) -> (Piece, bool) {
        piece_at(self.pcurve, self.cell, self.uv, self.near, like)
    }

    /// The query escalates piece by piece, not here.
    fn escalates(_: &(Piece, bool)) -> bool {
        false
    }
}

/// The piece `cell` of its span at `like`'s level: where it lies, and
/// whether its coefficients' values alone would settle it where the
/// certified hulls don't.
fn piece_at<S: LevelScalar>(
    p: &PCurve,
    cell: Cell,
    uv: [f64; 2],
    near: [f64; 2],
    like: &S,
) -> (Piece, bool) {
    let Some(mut c) = p.span_at(cell.root, like) else {
        return (Piece::Open, false);
    };
    for bit in 0..cell.depth {
        let [l, r] = c.split_half();
        c = if cell.right(bit) { r } else { l };
    }
    let f = |x: f64| S::from_f64(x, like);
    let axes = [
        [S::int(1, like), S::int(0, like)],
        [S::int(0, like), S::int(1, like)],
    ];
    // Per axis: the point's line, and the near box's lower and upper.
    let lines: [[Hulls; 3]; 2] = std::array::from_fn(|a| {
        let at = f(uv[a]);
        let n = f(near[a]);
        [at.clone(), at.sub(&n), at.add(&n)].map(|x| hulls(&c.compose_linear(&axes[a], &x.neg())))
    });
    let read = |pick: fn(&Hulls) -> [f64; 2]| -> Piece {
        let h = |a: usize, l: usize| pick(&lines[a][l]);
        if h(1, 0)[0] > 0.0 {
            Piece::Off(1)
        } else if h(1, 0)[1] < 0.0 {
            Piece::Off(-1)
        } else if h(0, 0)[1] < 0.0 {
            Piece::Left
        } else if h(0, 0)[0] > 0.0 {
            Piece::Right
        } else if (0..2).all(|a| h(a, 1)[0] >= 0.0 && h(a, 2)[1] <= 0.0) {
            Piece::Near
        } else {
            Piece::Open
        }
    };
    let certified = read(|h| h.certified);
    let settles = certified == Piece::Open && read(|h| h.values) != Piece::Open;
    (certified, settles)
}

/// A form's coefficient hull, certified and from the values alone.
struct Hulls {
    certified: [f64; 2],
    values: [f64; 2],
}

fn hulls<S: LevelScalar>(p: &Poly<S>) -> Hulls {
    let h = p.hull();
    let mut values = [f64::INFINITY, f64::NEG_INFINITY];
    for c in p.coefs() {
        let x = c.approx();
        if x < values[0] {
            values[0] = x;
        }
        if x > values[1] {
            values[1] = x;
        }
    }
    Hulls {
        certified: [h.lo, h.hi],
        values,
    }
}

/// Certified enclosures of the least and greatest value of a direction
/// (module docs): each `[lo, hi]` holds the extremum, within the
/// resolution asked unless a cell was done at a limit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Extrema {
    /// The least value's enclosure.
    pub(crate) min: [f64; 2],
    /// The greatest value's enclosure.
    pub(crate) max: [f64; 2],
    /// The highest level a cell was computed at: 0 for `f64`, 1 for
    /// double-double, 2 to 4 for the big-float widths.
    pub(crate) level: u8,
}

/// The extrema of `dir·C(t)` over `curve`, every span, refined to
/// `resolution` (module docs), each cell charged to `ledger`.
pub(crate) fn curve_extrema(
    curve: &Curve3D,
    dir: [f64; 3],
    resolution: f64,
    ledger: &mut Ledger<'_>,
) -> Result<Extrema, Stop<QueryError>> {
    extrema(&Spans3 { curve, dir }, resolution, ledger)
}

/// The extrema of `dir·p(t)` over the p-curve `pcurve` in UV, every span,
/// refined to `resolution` (module docs), each cell charged to `ledger`.
pub(crate) fn pcurve_extrema(
    pcurve: &PCurve,
    dir: [f64; 2],
    resolution: f64,
    ledger: &mut Ledger<'_>,
) -> Result<Extrema, Stop<QueryError>> {
    extrema(&Spans2 { pcurve, dir }, resolution, ledger)
}

/// The extrema of `dir·S(u, v)` over `tiles`, refined to `resolution`
/// (module docs), each cell charged to `ledger`.
pub(crate) fn tile_extrema(
    tiles: &[&Tile],
    dir: [f64; 3],
    resolution: f64,
    ledger: &mut Ledger<'_>,
) -> Result<Extrema, Stop<QueryError>> {
    extrema(&Tiles { tiles, dir }, resolution, ledger)
}

/// The roots of an extremum query: spans or tiles, each a rational form
/// in a direction.
trait Roots {
    /// How many.
    fn count(&self) -> usize;

    /// Whether they are tiles (for the charges).
    fn tiles(&self) -> bool;

    /// Whether the direction is finite.
    fn finite(&self) -> bool;

    /// Root `root` in the direction, negated if `neg`, at `like`'s level.
    fn net<S: LevelScalar>(&self, root: usize, neg: bool, like: &S) -> Option<Net<S>>;
}

/// The direction, negated exactly if `neg`, at `like`'s level.
fn direction<S: Scalar, const N: usize>(dir: [f64; N], neg: bool, like: &S) -> [S; N] {
    dir.map(|x| S::from_f64(if neg { -x } else { x }, like))
}

/// A curve's span in a direction.
fn span_net<S: Scalar, const N: usize>(c: HomCurve<S, N>, dir: [f64; N], neg: bool) -> Net<S> {
    let like = c.weights().coefs()[0].clone();
    let zero = S::int(0, &like);
    let n = c.compose_linear(&direction(dir, neg, &like), &zero);
    Net::Span(n, c.weights().clone())
}

struct Spans3<'c> {
    curve: &'c Curve3D,
    dir: [f64; 3],
}

impl Roots for Spans3<'_> {
    fn count(&self) -> usize {
        self.curve.spans().len()
    }

    fn tiles(&self) -> bool {
        false
    }

    fn finite(&self) -> bool {
        self.dir.iter().all(|x| x.is_finite())
    }

    fn net<S: LevelScalar>(&self, root: usize, neg: bool, like: &S) -> Option<Net<S>> {
        let c = self.curve.span_at(root, like)?;
        Some(span_net(c, self.dir, neg))
    }
}

struct Spans2<'c> {
    pcurve: &'c PCurve,
    dir: [f64; 2],
}

impl Roots for Spans2<'_> {
    fn count(&self) -> usize {
        self.pcurve.spans().len()
    }

    fn tiles(&self) -> bool {
        false
    }

    fn finite(&self) -> bool {
        self.dir.iter().all(|x| x.is_finite())
    }

    fn net<S: LevelScalar>(&self, root: usize, neg: bool, like: &S) -> Option<Net<S>> {
        let c = self.pcurve.span_at(root, like)?;
        Some(span_net(c, self.dir, neg))
    }
}

struct Tiles<'t> {
    tiles: &'t [&'t Tile],
    dir: [f64; 3],
}

impl Roots for Tiles<'_> {
    fn count(&self) -> usize {
        self.tiles.len()
    }

    fn tiles(&self) -> bool {
        true
    }

    fn finite(&self) -> bool {
        self.dir.iter().all(|x| x.is_finite())
    }

    fn net<S: LevelScalar>(&self, root: usize, neg: bool, like: &S) -> Option<Net<S>> {
        let p = self.tiles.get(root)?.patch_at(like);
        let n = p.compose_linear(&direction(self.dir, neg, like), &S::int(0, like));
        Some(Net::Tile(n, p.weights().clone()))
    }
}

/// A root's value form `N/D`: a span's or a tile's.
enum Net<S> {
    Span(Poly<S>, Poly<S>),
    Tile(Tensor<S>, Tensor<S>),
}

impl<S: Scalar> Net<S> {
    /// The half at a halving `depth` deep (from 0): a tile's in `u` at
    /// even depths, in `v` at odd ones.
    fn half(self, right: bool, depth: u32) -> Net<S> {
        match self {
            Net::Span(n, d) => Net::Span(pick(n.split_half(), right), pick(d.split_half(), right)),
            Net::Tile(n, d) => {
                let dir = if depth % 2 == 0 { Dir::U } else { Dir::V };
                Net::Tile(
                    pick(n.split_half(dir), right),
                    pick(d.split_half(dir), right),
                )
            }
        }
    }

    /// The numerator's and the weights' coefficients.
    fn coefs(&self) -> (&[S], &[S]) {
        match self {
            Net::Span(n, d) => (n.coefs(), d.coefs()),
            Net::Tile(n, d) => (n.coefs(), d.coefs()),
        }
    }

    /// The coefficients' indices at the corners, where the form takes
    /// their ratio.
    fn corners(&self) -> [usize; 4] {
        match self {
            Net::Span(n, _) => [0, n.degree(), 0, n.degree()],
            Net::Tile(n, _) => {
                let (m, k) = n.degrees();
                let at = |i: usize, j: usize| i * (k + 1) + j;
                [at(0, 0), at(0, k), at(m, 0), at(m, k)]
            }
        }
    }
}

/// The right half if `right`, else the left.
fn pick<T>([l, r]: [T; 2], right: bool) -> T {
    if right { r } else { l }
}

/// A cell's bounds of the least value (module docs).
#[derive(Clone, Copy, Debug)]
struct Bound {
    /// The least coefficient ratio, certified: a lower bound; `−∞` unless
    /// every weight is certified positive.
    lo: f64,
    /// The same from the values alone; `−∞` unless every weight's value
    /// is positive.
    lo_value: f64,
    /// The least corner ratio, certified: an upper bound of the least
    /// value; `+∞` where none is certified.
    hi: f64,
    /// The same from the values alone.
    hi_value: f64,
}

/// A root's cell at one precision level ([`cell_bound`]).
struct CellAt<'r, R> {
    roots: &'r R,
    neg: bool,
    cell: Cell,
}

impl<R: Roots> AtLevel<()> for CellAt<'_, R> {
    type Out = Bound;

    fn at<S: LevelScalar>(&mut self, like: &S, _: &mut ()) -> Bound {
        cell_bound(self.roots, self.neg, self.cell, like)
    }

    /// The query escalates cell by cell, not here.
    fn escalates(_: &Bound) -> bool {
        false
    }
}

/// The bounds of `cell` at `like`'s level.
fn cell_bound<R: Roots, S: LevelScalar>(roots: &R, neg: bool, cell: Cell, like: &S) -> Bound {
    let open = Bound {
        lo: f64::NEG_INFINITY,
        lo_value: f64::NEG_INFINITY,
        hi: f64::INFINITY,
        hi_value: f64::INFINITY,
    };
    let Some(mut net) = roots.net(cell.root, neg, like) else {
        return open;
    };
    for bit in 0..cell.depth {
        net = net.half(cell.right(bit), bit);
    }
    let (n, d) = net.coefs();
    let mut b = Bound {
        lo: f64::INFINITY,
        lo_value: f64::INFINITY,
        ..open
    };
    let (mut certified, mut valued) = (true, true);
    for (x, w) in n.iter().zip(d) {
        certified &= Scalar::sign(w) == Some(Ordering::Greater);
        let lo = Scalar::enclose(&Scalar::div(x, w))[0];
        if lo < b.lo {
            b.lo = lo;
        }
        let wv = LevelScalar::approx(w);
        valued &= wv > 0.0;
        let v = LevelScalar::approx(x) / wv;
        if v < b.lo_value {
            b.lo_value = v;
        }
    }
    // A NaN lower bound is no bound.
    if !certified || b.lo.is_nan() {
        b.lo = f64::NEG_INFINITY;
    }
    if !valued || b.lo_value.is_nan() {
        b.lo_value = f64::NEG_INFINITY;
    }
    for i in net.corners() {
        let (x, w) = (&n[i], &d[i]);
        if Scalar::sign(w) == Some(Ordering::Greater) {
            let hi = Scalar::enclose(&Scalar::div(x, w))[1];
            if hi < b.hi {
                b.hi = hi;
            }
        }
        let v = LevelScalar::approx(x) / LevelScalar::approx(w);
        if v < b.hi_value {
            b.hi_value = v;
        }
    }
    b
}

/// A cell waiting in the branch and bound, least lower bound first, then
/// by its cell.
struct Open {
    bound: Bound,
    cell: Cell,
}

impl Open {
    fn key(&self) -> (f64, Cell) {
        (self.bound.lo, self.cell)
    }
}

impl PartialEq for Open {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}

impl Eq for Open {}

impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for Open {
    fn cmp(&self, o: &Self) -> Ordering {
        let (a, b) = (self.key(), o.key());
        a.0.total_cmp(&b.0).then(a.1.cmp(&b.1))
    }
}

/// Both extrema of `roots` (module docs).
fn extrema<R: Roots>(
    roots: &R,
    resolution: f64,
    ledger: &mut Ledger<'_>,
) -> Result<Extrema, Stop<QueryError>> {
    if roots.count() == 0 || !roots.finite() || !(resolution > 0.0 && resolution.is_finite()) {
        return Err(Stop::Failed(QueryError::Input));
    }
    let (min, l0) = least(roots, false, resolution, ledger)?;
    let ([lo, hi], l1) = least(roots, true, resolution, ledger)?;
    Ok(Extrema {
        min,
        max: [-hi, -lo],
        level: if l1 > l0 { l1 } else { l0 },
    })
}

/// The enclosure of the least value of `roots` (of its negation if
/// `neg`), and the highest level a cell took (module docs).
fn least<R: Roots>(
    roots: &R,
    neg: bool,
    resolution: f64,
    ledger: &mut Ledger<'_>,
) -> Result<([f64; 2], u8), Stop<QueryError>> {
    let mut open = BinaryHeap::new();
    // The least certified corner value found.
    let mut best = f64::INFINITY;
    // The least lower bound of the cells done at a limit.
    let mut floor = f64::INFINITY;
    let mut level = 0u8;
    let mut eval = |cell: Cell,
                    open: &mut BinaryHeap<Reverse<Open>>,
                    best: &mut f64|
     -> Result<(), Stop<QueryError>> {
        ledger.charge(
            level_step(cell.level),
            extremum_cell_units(roots.tiles(), cell.level),
        )?;
        let bound = at_level(&mut CellAt { roots, neg, cell }, level_of(cell), &mut ());
        if bound.hi < *best {
            *best = bound.hi;
        }
        if cell.level > level {
            level = cell.level;
        }
        open.push(Reverse(Open { bound, cell }));
        Ok(())
    };
    for root in 0..roots.count() {
        eval(Cell::root(root), &mut open, &mut best)?;
    }
    let mut lo = f64::INFINITY;
    while let Some(Reverse(Open { bound: b, cell })) = open.pop() {
        if best - b.lo <= resolution {
            lo = b.lo;
            break;
        }
        // Space or precision, as the seam bound judges a cell.
        let precision = if b.lo == f64::NEG_INFINITY {
            b.lo_value > f64::NEG_INFINITY
        } else {
            b.lo_value - b.lo > b.hi_value - b.lo_value
        };
        if precision && cell.level + 1 < LEVELS {
            eval(cell.up(), &mut open, &mut best)?;
        } else if !precision && cell.depth < MAX_SUBDIV_DEPTH {
            for half in cell.halves() {
                eval(half, &mut open, &mut best)?;
            }
        } else if b.lo > f64::NEG_INFINITY {
            // Done at a limit, with its bound.
            if b.lo < floor {
                floor = b.lo;
            }
        } else if precision {
            return Err(Stop::Cap(Cap::PrecisionBits));
        } else {
            return Err(Stop::Cap(Cap::SubdivisionDepth));
        }
    }
    if best == f64::INFINITY {
        return Err(Stop::Cap(Cap::PrecisionBits));
    }
    let lo = if floor < lo { floor } else { lo };
    // Certified bounds can't cross; a lower bound above the least value
    // found would be its own rounding's doing.
    let lo = if lo > best { best } else { lo };
    Ok(([lo, best], level))
}

/// Hooks for the unit table's calibration benchmark
/// (`crates/kernel/benches/calibrate.rs`), which times trim pieces and
/// extremum cells per precision level against the units charged for
/// them. Not part of the kernel's interface.
#[doc(hidden)]
pub mod calibration {
    use super::*;
    use crate::geom::form::SphereKey;
    use crate::geom::lattice::Lattice;
    use crate::geom::surface::Surface;

    /// The circle of radius `r` about `c` in UV, four quarter spans of
    /// middle weight `√2/2` (rounded), counter-clockwise from `(c_u + r,
    /// c_v)`.
    pub fn circle(c: [f64; 2], r: f64) -> PCurve {
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let p = |a: f64, b: f64| [c[0] + a * r, c[1] + b * r];
        let corners = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)];
        let spans = (0..4)
            .map(|k| {
                let (a, b) = corners[k];
                let (e, f) = corners[(k + 1) % 4];
                let [s, m, t] = [p(a, b), p(a + e, b + f), p(e, f)];
                [[s[0], s[1], 1.0], [m[0], m[1], h], [t[0], t[1], 1.0]]
            })
            .collect();
        PCurve::new(spans, vec![0.0, 1.0, 2.0, 3.0, 4.0]).expect("a circle")
    }

    /// The sphere of radius 10 about the origin as its two halves at the
    /// base level (four tiles each), longitudes `[−180°, 0°]` and `[0°,
    /// 180°]`: a surface spans less than a full turn.
    pub fn sphere() -> [Surface; 2] {
        let k = SphereKey::new([0.0; 3], 10.0, [0.0, 0.0, 1.0]).expect("a sphere");
        let lat = Lattice::new(k.into()).expect("a lattice");
        [-2, 0].map(|first| {
            let rect = lat.half(first).expect("a half");
            Surface::new(lat.clone(), rect).expect("a surface")
        })
    }

    /// Computes the first `n` pieces `depth` halvings deep of each span
    /// of `p` at precision level `level`, against the point `uv` with the
    /// near box `near`, as the trim query does; the number computed. A
    /// level past the schedule's or a depth past [`MAX_SUBDIV_DEPTH`]
    /// computes none.
    pub fn trim_pieces(
        p: &PCurve,
        uv: [f64; 2],
        near: [f64; 2],
        level: u8,
        depth: u32,
        n: u64,
    ) -> u64 {
        if level >= LEVELS || depth > MAX_SUBDIV_DEPTH {
            return 0;
        }
        let mut count = 0u64;
        let mut seen = 0usize;
        for root in 0..p.spans().len() {
            for k in 0..n.min(1 << depth) {
                let cell = Cell {
                    root,
                    depth,
                    k,
                    level,
                };
                let mut at = PieceAt {
                    pcurve: p,
                    cell,
                    uv,
                    near,
                };
                let (piece, _) = at_level(&mut at, level_of(cell), &mut ());
                // Kept, so the computation isn't optimised away.
                seen += usize::from(piece == Piece::Open);
                count += 1;
            }
        }
        std::hint::black_box(seen);
        count
    }

    /// Computes the first `n` cells `depth` halvings deep of each span of
    /// `curve` (or of each tile of `tiles` if given) in the direction
    /// `dir` at precision level `level`, as the extremum query does; the
    /// number computed. A level past the schedule's or a depth past
    /// [`MAX_SUBDIV_DEPTH`] computes none.
    pub fn extremum_cells(
        curve: Option<&Curve3D>,
        tiles: &[&Tile],
        dir: [f64; 3],
        level: u8,
        depth: u32,
        n: u64,
    ) -> u64 {
        if level >= LEVELS || depth > MAX_SUBDIV_DEPTH {
            return 0;
        }
        match curve {
            Some(curve) => cells(&Spans3 { curve, dir }, level, depth, n),
            None => cells(&Tiles { tiles, dir }, level, depth, n),
        }
    }

    fn cells<R: Roots>(roots: &R, level: u8, depth: u32, n: u64) -> u64 {
        let mut count = 0u64;
        let mut least = f64::INFINITY;
        for root in 0..roots.count() {
            for k in 0..n.min(1 << depth) {
                let cell = Cell {
                    root,
                    depth,
                    k,
                    level,
                };
                let b = at_level(
                    &mut CellAt {
                        roots,
                        neg: false,
                        cell,
                    },
                    level_of(cell),
                    &mut (),
                );
                if b.lo < least {
                    least = b.lo;
                }
                count += 1;
            }
        }
        std::hint::black_box(least);
        count
    }
}
