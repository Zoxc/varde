//! The tight box of a body: per axis, certified enclosures of the least
//! and greatest coordinate over its vertices, its edges and its faces'
//! trimmed regions ([`tight_box`]).
//!
//! **Along a direction `d`** (each axis, either way) the greatest `d·x`
//! is found from below and above at once. Below, by values the body
//! takes: its vertices' positions, the lower ends of its edge curves'
//! greatest values ([`curve_extrema`]), and the corners of tile cells
//! certified inside their face. Above, by bounds of everything: the
//! edges' greatest values plus the edge's tolerance (a face's trims lie
//! within their seam bound of their edge, so this bounds the faces'
//! boundaries too), and the faces' cells.
//!
//! **A face's cells.** The tiles under the box of the face's trims are
//! searched by branch and bound, the cells in order of their upper bound
//! (then by face, tile, depth and position, so the order is the inputs'
//! alone): a cell's value `d·P/W` lies between its least and greatest
//! coefficient ratio, the weights positive. A cell is done when:
//! - its upper bound is within the resolution of the best value found
//!   (it counts towards the upper end); the first such cell ends the
//!   search, the rest being lower;
//! - `d·x_u` or `d·x_v` has a certified sign on it (its partial
//!   numerator's coefficients one sign): the greatest value over the
//!   face is at a point where both vanish or on its boundary, so the
//!   cell holds neither but boundary points, which the edges bound;
//! - no trim's control box meets it and its centre is outside the face
//!   (winding 0, [`winding`]): it lies outside; one whose centre is
//!   inside is inside whole, and so are its halves;
//! - it is [`MAX_DEPTH`] halvings deep: its upper bound counts.
//!
//! Otherwise its best corner, if better than the best value and inside
//! the face (certified so, or by its winding), becomes the best, and the
//! cell is halved, in `u` at even depths and in `v` at odd ones (the
//! partial numerators halved with it: a halving scales them, keeping
//! their signs).
//!
//! **Escalation.** Everything runs at the first precision level: a cell's
//! bounds carry their rounding. A cell whose bounds can't come within
//! the resolution there is done at the depth limit, so the enclosure is
//! then wider than the resolution: an achieved bound.
//!
//! **Charges.** Each cell is charged before it is examined at the
//! extremum query's tile cell row, the edges' extrema and the windings
//! as those queries charge; the six directions are the items of one
//! pass.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

use varde_decide::bezier::{Dir, Tensor};
use varde_decide::num::bounded::{Bounded, add_up, down};

use crate::geom::curve::Curve3D;
use crate::geom::query::{Membership, QueryError, TrimCurve, curve_extrema, winding};
use crate::geom::tile::Tile;
use crate::infra::budget::{Budget, Ledger, Pass, Step, Stop, Work, extremum_cell_units};
use crate::infra::error::{Invalid, KernelError};
use crate::topo::brep::{Brep, EdgeId, FaceId, VertId};

#[cfg(test)]
mod tests;

type B = Bounded<f64>;

/// A body's tight box: per axis, the enclosures `[lo, hi]` of its least
/// and greatest coordinate, each within the resolution asked unless a
/// search was done at its depth limit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TightBox {
    /// The least coordinate's enclosure, per axis.
    pub min: [[f64; 2]; 3],
    /// The greatest coordinate's enclosure, per axis.
    pub max: [[f64; 2]; 3],
}

impl TightBox {
    /// The box certainly holding the body: each least coordinate's lower
    /// end, each greatest's upper end.
    pub fn outer(&self) -> [[f64; 3]; 2] {
        [
            std::array::from_fn(|k| self.min[k][0]),
            std::array::from_fn(|k| self.max[k][1]),
        ]
    }

    /// The box the body certainly reaches: each least coordinate's upper
    /// end, each greatest's lower end.
    pub fn inner(&self) -> [[f64; 3]; 2] {
        [
            std::array::from_fn(|k| self.min[k][1]),
            std::array::from_fn(|k| self.max[k][0]),
        ]
    }
}

/// The deepest a face's cell is halved (in `u` and `v` together).
pub const MAX_DEPTH: u32 = 48;

/// Why the box can't be found: the store breaks an invariant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoundsError {
    /// The invariant numbered: a face's trims don't close (1), or lie
    /// outside its surface's tiles (2).
    Check(u8),
}

impl From<BoundsError> for KernelError {
    fn from(e: BoundsError) -> Self {
        let BoundsError::Check(inv) = e;
        KernelError::Invalid(Invalid::Check { inv })
    }
}

impl From<Stop<QueryError>> for Stop<BoundsError> {
    fn from(s: Stop<QueryError>) -> Self {
        match s {
            Stop::Exhausted(e) => Stop::Exhausted(e),
            Stop::Cap(c) => Stop::Cap(c),
            Stop::Failed(QueryError::Open) => Stop::Failed(BoundsError::Check(1)),
            // The inputs are formed here, finite and positive, but for a
            // store value that is not a number.
            Stop::Failed(QueryError::Input) => Stop::Failed(BoundsError::Check(2)),
        }
    }
}

/// The body's tight box (module docs), refined to `resolution` (mm; a
/// value that isn't a positive number refines to the depth limits), on a
/// budget of `budget`; `None` for a body with no vertex. A face whose
/// trims don't close or leave its surface's tiles is `Invalid(Check)`;
/// running out of the budget is `OutOfBudget`.
pub fn tight_box(
    brep: &Brep,
    resolution: f64,
    budget: &Budget,
) -> Result<Option<TightBox>, KernelError> {
    if brep.vert_count() == 0 {
        return Ok(None);
    }
    let res = if resolution > 0.0 {
        pick(resolution.is_finite(), resolution, f64::MAX)
    } else {
        f64::MIN_POSITIVE
    };
    // `+x, −x, +y, −y, +z, −z`.
    let dirs: Vec<(usize, bool)> = (0..3).flat_map(|k| [(k, false), (k, true)]).collect();
    let mut work = Work::new(budget);
    let pass = Pass {
        stage: "measure",
        rung: 0,
    };
    let id = |i: usize| u32::try_from(i).expect("ids below NONE");
    let part = Part {
        verts: (0..brep.vert_count()).map(|v| VertId(id(v))).collect(),
        edges: (0..brep.edge_count()).map(|e| EdgeId(id(e))).collect(),
        faces: (0..brep.face_count()).map(|f| FaceId(id(f))).collect(),
    };
    let ext = work
        .pass(pass, &dirs, |&(k, neg), ledger| {
            greatest(brep, &part, k, neg, res, ledger)
        })
        .map_err(KernelError::from)?;
    Ok(Some(TightBox {
        min: std::array::from_fn(|k| {
            let [lo, hi] = ext[2 * k + 1];
            [-hi, -lo]
        }),
        max: std::array::from_fn(|k| ext[2 * k]),
    }))
}

/// `a` if `c`, else `b`.
fn pick(c: bool, a: f64, b: f64) -> f64 {
    if c { a } else { b }
}

/// Whether `x` is certainly above 0: false for a NaN.
fn positive(x: f64) -> bool {
    x > 0.0
}

/// The greater of two values (NaN-free), compare and select.
fn max(a: f64, b: f64) -> f64 {
    pick(b > a, b, a)
}

/// What a box bounds: some faces of a body, with the edges and vertices
/// bounding them (all of the body's for its tight box).
struct Part {
    verts: Vec<VertId>,
    edges: Vec<EdgeId>,
    faces: Vec<FaceId>,
}

/// A certified box holding the faces `faces` of `brep` (each face with
/// its trims, `face_edges[f]` the edges of face `f`), each side refined
/// to `res` (mm) as [`tight_box`] does, the six directions in turn on
/// `ledger`: `[lo, hi]`, empty (`lo > hi`) for no faces. For a shell's
/// box in the check's and the regions' placement (`topo::regions`).
pub(crate) fn faces_box(
    brep: &Brep,
    faces: &[FaceId],
    face_edges: &[Vec<EdgeId>],
    res: f64,
    ledger: &mut Ledger<'_>,
) -> Result<[[f64; 3]; 2], Stop<BoundsError>> {
    let mut edges: Vec<EdgeId> = faces
        .iter()
        .filter_map(|f| face_edges.get(f.index()))
        .flatten()
        .copied()
        .filter(|e| e.index() < brep.edge_count())
        .collect();
    edges.sort_unstable();
    edges.dedup();
    let mut verts: Vec<VertId> = edges
        .iter()
        .flat_map(|&e| brep.ends(e))
        .filter(|v| v.index() < brep.vert_count())
        .collect();
    verts.sort_unstable();
    verts.dedup();
    let mut faces = faces.to_vec();
    faces.sort_unstable();
    faces.dedup();
    let part = Part {
        verts,
        edges,
        faces,
    };
    let res = if res > 0.0 && res.is_finite() {
        res
    } else {
        f64::MIN_POSITIVE
    };
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    if part.verts.is_empty() {
        return Ok([lo, hi]);
    }
    for k in 0..3 {
        hi[k] = greatest(brep, &part, k, false, res, ledger)?[1];
        lo[k] = -greatest(brep, &part, k, true, res, ledger)?[1];
    }
    Ok([lo, hi])
}

/// The enclosure `[lo, hi]` of the greatest `±xₖ` over the part `part`
/// of the body (module docs), `−xₖ` if `neg`.
fn greatest(
    brep: &Brep,
    part: &Part,
    k: usize,
    neg: bool,
    res: f64,
    ledger: &mut Ledger<'_>,
) -> Result<[f64; 2], Stop<BoundsError>> {
    let sign = if neg { -1.0 } else { 1.0 };
    let mut dir = [0.0; 3];
    dir[k] = sign;
    let mut best = f64::NEG_INFINITY;
    let mut upper = f64::NEG_INFINITY;
    for &v in &part.verts {
        let x = sign * brep.pos(v)[k];
        best = max(best, x);
        upper = max(upper, x);
    }
    for &e in &part.edges {
        let Some(curve) = edge_part(brep, e) else {
            continue;
        };
        let ext = curve_extrema(&curve, dir, res, ledger)?;
        best = max(best, ext.max[0]);
        upper = max(upper, add_up(ext.max[1], brep.edge_tol(e)));
    }
    let mut search = Search {
        brep,
        dir,
        res,
        best,
        upper,
        faces: Vec::new(),
        heap: BinaryHeap::new(),
    };
    for &f in &part.faces {
        search.add_face(f)?;
    }
    search.run(ledger)?;
    Ok([search.best, max(search.upper, search.best)])
}

/// An edge's curve over its parameter range, the stored chain itself
/// where that is the whole of it; `None` for an edge without spans.
fn edge_part(brep: &Brep, e: EdgeId) -> Option<Cow<'_, Curve3D>> {
    let c = brep.edge_curve(e);
    let [t0, t1] = brep.t_range(e);
    let at = |t: f64| c.knots().iter().position(|k| k.to_bits() == t.to_bits());
    let (i, j) = (at(t0)?, at(t1)?);
    if i >= j {
        return None;
    }
    if i == 0 && j == c.spans().len() {
        return Some(Cow::Borrowed(c));
    }
    Curve3D::new(c.spans()[i..j].to_vec(), c.knots()[i..=j].to_vec())
        .ok()
        .map(Cow::Owned)
}

/// Below `2⁵²` in magnitude, UV values have exact integer neighbours.
const UV_LIMIT: f64 = 4_503_599_627_370_496.0;

/// A face as the search reads it: its trims and their control boxes in
/// global UV.
struct Face<'a> {
    trims: Vec<TrimCurve<'a>>,
    boxes: Vec<[[f64; 2]; 2]>,
}

/// A cell of a tile (module docs): its place, its forms in the cell's
/// coordinates and its bounds.
struct Cell {
    /// The face's index in the search, and the tile's indices.
    face: usize,
    tile: [i64; 2],
    /// Halvings from the tile, and which halves (a bit per halving, the
    /// first the highest).
    depth: u32,
    path: u64,
    /// The cell's least corner and size in the tile's coordinates.
    lo: [f64; 2],
    size: [f64; 2],
    /// `d·P` (homogeneous), `W`, and `d·` the partial numerators.
    num: Tensor<B>,
    w: Tensor<B>,
    du: Tensor<B>,
    dv: Tensor<B>,
    /// Certified inside the face, whole.
    inside: bool,
    /// The greatest coefficient ratio, certified upwards; `+∞` unless
    /// every weight is certified positive.
    ub: f64,
}

impl Cell {
    /// The order of the search: the greatest upper bound first, then the
    /// least face, tile, depth and position.
    fn order(&self, o: &Cell) -> Ordering {
        self.ub
            .total_cmp(&o.ub)
            .then_with(|| o.face.cmp(&self.face))
            .then_with(|| [o.tile[1], o.tile[0]].cmp(&[self.tile[1], self.tile[0]]))
            .then_with(|| o.depth.cmp(&self.depth))
            .then_with(|| o.path.cmp(&self.path))
    }

    /// The cell's UV box in global UV.
    fn uv_box(&self) -> [[f64; 2]; 2] {
        let lo = [0, 1].map(|k| self.tile[k] as f64 + self.lo[k]);
        let hi = [0, 1].map(|k| self.tile[k] as f64 + (self.lo[k] + self.size[k]));
        [lo, hi]
    }

    /// The best corner: its global UV and a certified lower bound of the
    /// value there; `None` where none is certified.
    fn best_corner(&self) -> Option<([f64; 2], f64)> {
        let (m, n) = self.num.degrees();
        let mut best: Option<([f64; 2], f64)> = None;
        for (i, j) in [(0, 0), (0, n), (m, 0), (m, n)] {
            let (p, w) = (self.num.get(i, j), self.w.get(i, j));
            if !positive(w.v - w.err()) {
                continue;
            }
            let q = *p / *w;
            let lo = down(q.v - q.err());
            if lo.is_nan() || best.is_some_and(|(_, b)| lo <= b) {
                continue;
            }
            let at = |k: usize, end: bool| {
                self.tile[k] as f64
                    + if end {
                        self.lo[k] + self.size[k]
                    } else {
                        self.lo[k]
                    }
            };
            best = Some(([at(0, i == m), at(1, j == n)], lo));
        }
        best
    }

    /// The cell's halves along `u` at even depths, `v` at odd ones.
    fn halves(&self) -> [Cell; 2] {
        let dir = if self.depth % 2 == 0 { Dir::U } else { Dir::V };
        let k = if dir == Dir::U { 0 } else { 1 };
        let [n0, n1] = self.num.split_half(dir);
        let [w0, w1] = self.w.split_half(dir);
        let [u0, u1] = self.du.split_half(dir);
        let [v0, v1] = self.dv.split_half(dir);
        let mut size = self.size;
        size[k] *= 0.5;
        let half = |right: bool, num, w, du, dv| {
            let mut lo = self.lo;
            if right {
                lo[k] += size[k];
            }
            cell(
                self.face,
                self.tile,
                self.depth + 1,
                (self.path << 1) | u64::from(right),
                lo,
                size,
                [num, w, du, dv],
                self.inside,
            )
        };
        [half(false, n0, w0, u0, v0), half(true, n1, w1, u1, v1)]
    }
}

/// A cell with its upper bound.
#[allow(clippy::too_many_arguments)]
fn cell(
    face: usize,
    tile: [i64; 2],
    depth: u32,
    path: u64,
    lo: [f64; 2],
    size: [f64; 2],
    [num, w, du, dv]: [Tensor<B>; 4],
    inside: bool,
) -> Cell {
    let mut ub = f64::NEG_INFINITY;
    for (p, q) in num.coefs().iter().zip(w.coefs()) {
        if !positive(q.v - q.err()) {
            ub = f64::INFINITY;
            break;
        }
        let r = *p / *q;
        let hi = add_up(r.v, r.err());
        ub = if hi.is_nan() {
            f64::INFINITY
        } else {
            max(ub, hi)
        };
    }
    Cell {
        face,
        tile,
        depth,
        path,
        lo,
        size,
        num,
        w,
        du,
        dv,
        inside,
        ub,
    }
}

/// A cell in the search's heap, by [`Cell::order`].
struct Queued(Box<Cell>);

impl PartialEq for Queued {
    fn eq(&self, o: &Self) -> bool {
        self.0.order(&o.0) == Ordering::Equal
    }
}

impl Eq for Queued {}

impl PartialOrd for Queued {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for Queued {
    fn cmp(&self, o: &Self) -> Ordering {
        self.0.order(&o.0)
    }
}

/// The search over the faces' cells along one direction (module docs).
struct Search<'a> {
    brep: &'a Brep,
    dir: [f64; 3],
    res: f64,
    /// The best value found, and the upper bound of the cells done.
    best: f64,
    upper: f64,
    faces: Vec<Face<'a>>,
    heap: BinaryHeap<Queued>,
}

impl<'a> Search<'a> {
    /// The face's trims, and its tiles' cells queued.
    fn add_face(&mut self, f: FaceId) -> Result<(), Stop<BoundsError>> {
        let brep = self.brep;
        let mut trims = Vec::new();
        let mut boxes = Vec::new();
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for l in brep.loops(f) {
            for c in brep.loop_coedges(l) {
                let pcurve = brep.coedge_pcurve(c);
                trims.push(TrimCurve {
                    pcurve,
                    reversed: brep.reversed(c),
                });
                for s in pcurve.spans() {
                    let [a, b] = s.hull();
                    for k in 0..2 {
                        if a[k].is_nan() || b[k].is_nan() {
                            return Err(Stop::Failed(BoundsError::Check(2)));
                        }
                        lo[k] = pick(a[k] < lo[k], a[k], lo[k]);
                        hi[k] = pick(b[k] > hi[k], b[k], hi[k]);
                    }
                    boxes.push([a, b]);
                }
            }
        }
        if trims.is_empty() {
            return Ok(());
        }
        if !lo.iter().chain(&hi).all(|x| x.abs() < UV_LIMIT) {
            return Err(Stop::Failed(BoundsError::Check(2)));
        }
        let surface = brep.face_surface(f);
        let face = self.faces.len();
        let range = |k: usize| {
            (
                lo[k].floor() as i64,
                pick(hi[k] > lo[k], hi[k], lo[k] + 1.0).ceil() as i64 - 1,
            )
        };
        let ((i0, i1), (j0, j1)) = (range(0), range(1));
        for j in j0..=j1 {
            for i in i0..=i1 {
                let Some(tile) = surface.tile(i, j) else {
                    return Err(Stop::Failed(BoundsError::Check(2)));
                };
                if let Some(c) = self.root(face, [i, j], tile) {
                    self.heap.push(Queued(Box::new(c)));
                }
            }
        }
        self.faces.push(Face { trims, boxes });
        Ok(())
    }

    /// A tile's whole cell; `None` where its forms can't be formed.
    fn root(&self, face: usize, tile: [i64; 2], t: &Tile) -> Option<Cell> {
        let patch = t.patch();
        let a = self.dir.map(B::exact);
        let num = patch.compose_linear(&a, &B::exact(0.0));
        let k = self.dir.iter().position(|x| *x != 0.0)?;
        let neg = self.dir[k] < 0.0;
        let partial = |d: Dir| {
            let t = patch.partial_numerator(d)?.into_iter().nth(k)?;
            Some(if neg { t.neg() } else { t })
        };
        Some(cell(
            face,
            tile,
            0,
            0,
            [0.0, 0.0],
            [1.0, 1.0],
            [
                num,
                patch.weights().clone(),
                partial(Dir::U)?,
                partial(Dir::V)?,
            ],
            false,
        ))
    }

    /// The branch and bound (module docs).
    fn run(&mut self, ledger: &mut Ledger<'_>) -> Result<(), Stop<BoundsError>> {
        while let Some(Queued(c)) = self.heap.pop() {
            ledger.charge(Step::Work, extremum_cell_units(true, 0))?;
            if c.ub <= self.best + self.res {
                // Every cell left is lower.
                self.upper = max(self.upper, c.ub);
                self.heap.clear();
                break;
            }
            if signed(&c.du) || signed(&c.dv) {
                continue;
            }
            let mut c = *c;
            if !c.inside && self.clear(&c) {
                let [lo, hi] = c.uv_box();
                let centre = [0, 1].map(|k| lo[k] * 0.5 + hi[k] * 0.5);
                let near = [0, 1].map(|k| (hi[k] - lo[k]) * 0.25);
                match self.membership(c.face, centre, near, ledger)? {
                    Membership::Winding(0) => continue,
                    Membership::Winding(_) => c.inside = true,
                    Membership::Near => {}
                }
            }
            if let Some((uv, value)) = c.best_corner()
                && value > self.best
            {
                let inside = c.inside || {
                    let near = [0, 1].map(|k| {
                        let s = c.size[k] / 16.0;
                        max(s, (1.0 + uv[k].abs()) / 1_099_511_627_776.0) // 2⁻⁴⁰
                    });
                    matches!(
                        self.membership(c.face, uv, near, ledger)?,
                        Membership::Winding(w) if w != 0
                    )
                };
                if inside {
                    self.best = value;
                    if c.ub <= self.best + self.res {
                        self.upper = max(self.upper, c.ub);
                        continue;
                    }
                }
            }
            if c.depth >= MAX_DEPTH {
                self.upper = max(self.upper, c.ub);
                continue;
            }
            for h in c.halves() {
                self.heap.push(Queued(Box::new(h)));
            }
        }
        Ok(())
    }

    /// Whether no trim's control box meets the cell's box.
    fn clear(&self, c: &Cell) -> bool {
        let [lo, hi] = c.uv_box();
        self.faces[c.face]
            .boxes
            .iter()
            .all(|[a, b]| (0..2).any(|k| b[k] < lo[k] || a[k] > hi[k]))
    }

    /// The face's winding about `uv` ([`winding`]).
    fn membership(
        &self,
        face: usize,
        uv: [f64; 2],
        near: [f64; 2],
        ledger: &mut Ledger<'_>,
    ) -> Result<Membership, Stop<BoundsError>> {
        Ok(winding(&self.faces[face].trims, uv, near, ledger)?)
    }
}

/// Whether a form has a certified sign on its cell: every coefficient
/// certified positive, or every one negative.
fn signed(t: &Tensor<B>) -> bool {
    let h = t.hull();
    h.lo > 0.0 || h.hi < 0.0
}
