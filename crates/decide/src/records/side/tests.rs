//! Box degrees with side records against the exact winding: faces of 3 × 2
//! biquadratic tiles, their outer boundary straight (so its shadow is a
//! quadrilateral whose winding about the query is computed exactly on
//! rationals), their inner control points thrown about so the shadow
//! folds, and the query point's shadow exactly on an inner tile line (at a
//! stored row's point) or exactly at an inner tile corner, the face of
//! either operand. Random partitions, each tile refined to its own depth
//! (T-junctions along the tile lines), sum to the winding about the moved
//! point, and each box read with a side record is, alone, its degree
//! about the point really moved (by a tiny `f64` offset) where that is
//! certified; so do the whole tiles. Also: a tile edge's count is the sum of
//! its parts' however cut, each record decided once and replayed alike,
//! and records refused where their key or geometry is wrong.

use std::collections::{BTreeMap, VecDeque};
use std::ops::Range;

use glam::DVec3;
use malachite::Rational as Q;
use varde_testkit::rng::Rng;

use super::*;
use crate::bezier::{Dir, Dyadic, HomPatch, Scalar};
use crate::count::boxes::{BoxKey, FaceUv, QShadow, Trim, dy, split, sub_patch};
use crate::count::degree::{DegreeError, SidedError, box_degree, box_degree_sided, hull_off};
use crate::ids::{EdgeId, VertId};
use crate::num::bounded::Bounded;
use crate::perturb::{NU, R_HAT, Seg, T, UP, rates_a};
use crate::records::keys::InnerLine;

use Operand::{A, B};

const TILES: [Range<i64>; 2] = [0..3, 0..2];
const NU_PTS: usize = 7;
const NV_PTS: usize = 5;
const FACE: FaceId = FaceId(5);

fn first() -> Bounded<f64> {
    Bounded::exact(0.0)
}

fn band() -> Band {
    Band::curved(0.0, 1.0 / 1_073_741_824.0).expect("a band")
}

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

/// A random integer in `[−k, k]`, as a float.
fn int(rng: &mut Rng, k: u64) -> f64 {
    (rng.next_u64() % (2 * k + 1)) as f64 - k as f64
}

/// A face over the tiles, of `face_op`, and the query points of the
/// other operand, with `A`'s motion.
struct Geom {
    face_op: Operand,
    /// Control points with weights, `[u][v]`: two per tile and one more.
    grid: Vec<Vec<[f64; 4]>>,
    /// The four corners `[[c00, c01], [c10, c11]]`.
    corners: [[[f64; 3]; 2]; 2],
    pos: BTreeMap<Point, DVec3>,
    d: DVec3,
    sigma: Sign,
    /// The scale of every position (a power of two, so exact).
    sc: f64,
}

/// The query point.
fn qp() -> Point {
    Point::vert(B, VertId(1))
}

impl Geom {
    /// A random face: corners at integers, the outer boundary's control
    /// points on the segments between them (at dyadic positions, exact),
    /// the inner ones moved by integers up to `wild`; the tile lines'
    /// rows of weight 1 (so their points at dyadic parameters are exact),
    /// the tiles' inner points of weight ½, 1 or 2.
    fn new(rng: &mut Rng, face_op: Operand, wild: u64) -> Geom {
        let mut corners = [[[0.0; 3]; 2]; 2];
        for (a, row) in corners.iter_mut().enumerate() {
            for (b, c) in row.iter_mut().enumerate() {
                *c = [
                    48.0 * a as f64 + int(rng, 6),
                    32.0 * b as f64 + int(rng, 4),
                    int(rng, 8),
                ];
            }
        }
        let su = [0.0, 1.0, 3.0, 4.0, 5.0, 7.0, 8.0].map(|k| k / 8.0);
        let sv = [0.0, 1.0, 2.0, 3.0, 4.0].map(|k| k / 4.0);
        let lerp = |p: [f64; 3], r: [f64; 3], t: f64| -> [f64; 3] {
            std::array::from_fn(|k| p[k] + t * (r[k] - p[k]))
        };
        let mut grid = vec![vec![[0.0; 4]; NV_PTS]; NU_PTS];
        for i in 0..NU_PTS {
            for j in 0..NV_PTS {
                let (s, t) = (su[i], sv[j]);
                let c = &corners;
                let mut w = 1.0;
                let p = if j == 0 || j == NV_PTS - 1 {
                    lerp(c[0][usize::from(j != 0)], c[1][usize::from(j != 0)], s)
                } else if i == 0 || i == NU_PTS - 1 {
                    lerp(c[usize::from(i != 0)][0], c[usize::from(i != 0)][1], t)
                } else {
                    let x = lerp(lerp(c[0][0], c[1][0], s), lerp(c[0][1], c[1][1], s), t);
                    if i % 2 == 1 && j % 2 == 1 {
                        w = [0.5, 1.0, 2.0][(rng.next_u64() % 3) as usize];
                    }
                    [
                        x[0] + int(rng, wild),
                        x[1] + int(rng, wild),
                        x[2] + int(rng, 8),
                    ]
                };
                grid[i][j] = [p[0], p[1], p[2], w];
            }
        }
        let d = DVec3::new(int(rng, 3), int(rng, 3), int(rng, 3));
        let sigma = if rng.next_u64().is_multiple_of(2) {
            Sign::Pos
        } else {
            Sign::Neg
        };
        Geom {
            face_op,
            grid,
            corners,
            pos: BTreeMap::new(),
            d,
            sigma,
            sc: 1.0,
        }
    }

    /// The face scaled by `sc`, a power of two: every sign the same.
    fn scaled(mut self, sc: f64) -> Geom {
        for p in self.grid.iter_mut().flatten() {
            for x in &mut p[..3] {
                *x *= sc;
            }
        }
        for x in self.corners.iter_mut().flatten().flatten() {
            *x *= sc;
        }
        self.sc *= sc;
        self
    }

    /// Every weight 1, so the face's points at dyadic parameters are
    /// exact.
    fn unit_weights(mut self) -> Geom {
        for p in self.grid.iter_mut().flatten() {
            p[3] = 1.0;
        }
        self
    }

    /// The query's shadow with `A` really moved, `δ·(σ·d + η·T₂ +
    /// η²·T₃)`, `δ = 2⁻²⁰`, `η = 2⁻¹⁰`: exact in `f64` here, and far
    /// enough off the face's straight tile lines for the plain degree.
    fn moved_query(&self) -> QShadow {
        let sigma = match self.sigma {
            Sign::Pos => 1.0,
            Sign::Neg => -1.0,
        };
        let off =
            (self.d * (sigma / 1_048_576.0) + T[0] / 1_073_741_824.0 + T[1] / 1_099_511_627_776.0)
                * self.sc;
        let p = self.pos[&self.query()];
        let p = match self.face_op {
            B => p + off,
            A => p - off,
        };
        QShadow::of([p.x, p.y, p.z]).unwrap()
    }

    fn point_op(&self) -> Operand {
        self.face_op.other()
    }

    fn query(&self) -> Point {
        Point::vert(self.point_op(), VertId(1))
    }

    fn rates(&self, op: Operand) -> [Rate; ORDERS] {
        match op {
            A => rates_a(self.sigma, self.d),
            B => [Rate::ZERO, Rate::ZERO, Rate::ZERO, Rate::ZERO],
        }
    }

    /// Tile `(i, j)`'s net, row-major in `u`.
    fn net(&self, [i, j]: [i64; 2]) -> Vec<[f64; 4]> {
        let mut n = Vec::new();
        for a in 0..3 {
            for b in 0..3 {
                n.push(self.grid[2 * i as usize + a][2 * j as usize + b]);
            }
        }
        n
    }

    fn patches<S: Scalar>(&self, like: &S) -> Vec<HomPatch<S>> {
        let mut out = Vec::new();
        for i in TILES[0].clone() {
            for j in TILES[1].clone() {
                out.push(HomPatch::from_affine(2, 2, self.net([i, j]), like).unwrap());
            }
        }
        out
    }

    fn edge_span(&self, e: &TileEdge) -> Option<Span> {
        let ok = |r: &Range<i64>, x: i64, closed: bool| r.contains(&x) || (closed && x == r.end);
        let (k, o) = match e.fixed {
            Dir::U => (0, 1),
            Dir::V => (1, 0),
        };
        if e.face != FACE
            || e.operand != self.face_op
            || !ok(&TILES[k], e.line, true)
            || !ok(&TILES[o], e.cell, false)
        {
            return None;
        }
        let (l, c) = (2 * e.line as usize, 2 * e.cell as usize);
        Some(std::array::from_fn(|n| match e.fixed {
            Dir::U => self.grid[l][c + n],
            Dir::V => self.grid[c + n][l],
        }))
    }

    /// The exact winding of the outer boundary's shadow about `p`'s,
    /// `None` when `p`'s shadow lies on it.
    fn winding(&self, p: DVec3) -> Option<i64> {
        let c = &self.corners;
        let ring = [c[0][0], c[1][0], c[1][1], c[0][1]].map(exact_shadow);
        polygon_winding(&ring, &exact_shadow([p.x, p.y, p.z]))
    }
}

/// `(r̂·x, ν·x)` exactly.
fn exact_shadow(x: [f64; 3]) -> [Q; 2] {
    let d = |a: DVec3| q(a.x) * q(x[0]) + q(a.y) * q(x[1]) + q(a.z) * q(x[2]);
    [d(R_HAT), d(NU)]
}

fn polygon_winding(ring: &[[Q; 2]], p: &[Q; 2]) -> Option<i64> {
    let mut w = 0;
    for k in 0..ring.len() {
        let (a, b) = (&ring[k], &ring[(k + 1) % ring.len()]);
        let cross = (&b[0] - &a[0]) * (&p[1] - &a[1]) - (&b[1] - &a[1]) * (&p[0] - &a[0]);
        let within = |s: &Q, t: &Q, x: &Q| (s <= x && x <= t) || (t <= x && x <= s);
        if cross == 0 && within(&a[0], &b[0], &p[0]) && within(&a[1], &b[1], &p[1]) {
            return None;
        }
        if a[1] <= p[1] && p[1] < b[1] && cross > 0 {
            w += 1;
        } else if b[1] <= p[1] && p[1] < a[1] && cross < 0 {
            w -= 1;
        }
    }
    Some(w)
}

impl ShadowGeometry for Geom {
    fn point(&self, p: Point) -> Moving {
        Moving {
            at: self.pos[&p],
            rates: self.rates(p.operand),
        }
    }

    fn near_radius(&self, _: Point) -> f64 {
        0.0
    }

    fn ends(&self, _: Operand, _: EdgeId) -> [VertId; 2] {
        unreachable!("no edges here")
    }

    fn line(&self, _: Point, _: EdgeId) -> Seg {
        unreachable!("no edges here")
    }
}

impl SideGeometry for Geom {
    fn tile_edge(&self, e: &TileEdge) -> Option<Span> {
        self.edge_span(e)
    }

    fn corner(&self, _: Point, c: &TileCorner) -> Option<Moving> {
        let in_range = |r: &Range<i64>, x: i64| r.start <= x && x <= r.end;
        if c.face != FACE
            || c.operand != self.face_op
            || !in_range(&TILES[0], c.at[0])
            || !in_range(&TILES[1], c.at[1])
        {
            return None;
        }
        let p = self.grid[2 * c.at[0] as usize][2 * c.at[1] as usize];
        Some(Moving {
            at: DVec3::new(p[0], p[1], p[2]),
            rates: self.rates(self.face_op),
        })
    }

    fn tile_net(&self, operand: Operand, face: FaceId, tile: [i64; 2]) -> Option<TileNet> {
        let ok = TILES[0].contains(&tile[0]) && TILES[1].contains(&tile[1]);
        (operand == self.face_op && face == FACE && ok)
            .then(|| self.net(tile).try_into().expect("nine points"))
    }

    fn point_rates(&self, _: Point, _: &TilePoint) -> [Rate; ORDERS] {
        self.rates(self.face_op)
    }

    fn line_rates<S: Curved>(&self, _: Point, _: &SideLine, t: &S) -> [[S; 3]; ORDERS] {
        let lit = |v: DVec3| [v.x, v.y, v.z].map(|x| S::from_f64(x, t));
        match self.face_op {
            A => {
                let own = match self.sigma {
                    Sign::Pos => self.d,
                    Sign::Neg => -self.d,
                };
                [lit(own), lit(T[0]), lit(T[1]), lit(T[2])]
            }
            B => std::array::from_fn(|_| lit(DVec3::ZERO)),
        }
    }
}

/// A pass deciding in order on one thread, counting the keys it decided.
#[derive(Default)]
struct Seq {
    decided: usize,
}

impl Pass for Seq {
    type Error = Fail;

    fn run<K: Sync, V: Send>(
        &mut self,
        keys: &[Fresh<K>],
        decide: &(dyn Fn(&Fresh<K>) -> Decided<V> + Sync),
    ) -> Result<Vec<V>, Fail> {
        keys.iter()
            .map(|k| {
                self.decided += 1;
                decide(k).0
            })
            .collect()
    }
}

fn face_uv<'a>(g: &Geom, trims: &'a [Trim<'a>], flipped: bool) -> FaceUv<'a> {
    FaceUv::new(g.face_op, FACE, TILES, [1.0, 1.0], trims, flipped).unwrap()
}

fn patch_of<'p, P>(ps: &'p [P], b: &BoxKey) -> &'p P {
    let [i, j] = b.tile();
    let nv = (TILES[1].end - TILES[1].start) as usize;
    &ps[(i - TILES[0].start) as usize * nv + (j - TILES[1].start) as usize]
}

/// A random partition of the face's tiles: each box split (off `qs`)
/// with probability `p` while shallower than `depth`, always while its
/// hull holds the query and shallower than `near`.
fn partition(
    rng: &mut Rng,
    face: &FaceUv<'_>,
    ps: &[HomPatch<Bounded<f64>>],
    qs: &QShadow,
    p: f64,
    depth: u32,
    near: u32,
) -> Vec<BoxKey> {
    let mut queue: VecDeque<BoxKey> = face.tile_boxes().collect();
    let mut leaves = Vec::new();
    while let Some(b) = queue.pop_front() {
        let patch = patch_of(ps, &b);
        let close = !hull_off(&sub_patch(patch, &b), qs) && b.depth() < near;
        if close || (b.depth() < depth && rng.unit() < p) {
            queue.extend(split(face, &b, patch, Some(qs)).unwrap().halves);
        } else {
            leaves.push(b);
        }
        assert!(leaves.len() + queue.len() < 100_000, "refinement runs away");
    }
    leaves
}

/// The sum of the leaves' degrees, filling the side records they ask
/// for, how many boxes read a record, and how many of those were checked
/// alone against the point really moved; or the first box no record
/// certifies.
fn total(
    g: &Geom,
    face: &FaceUv<'_>,
    ps: &[HomPatch<Bounded<f64>>],
    leaves: &[BoxKey],
    qs: &QShadow,
    sides: &mut Sides,
) -> Result<(i64, usize, usize), Box<(BoxKey, SidedError)>> {
    let (mut sum, mut sided, mut moved) = (0, 0, 0);
    let qm = g.moved_query();
    for b in leaves {
        let patch = patch_of(ps, b);
        let d = loop {
            match box_degree_sided(face, b, patch, qs, g.query(), sides) {
                Err(SidedError::Need(k)) => {
                    sides
                        .fill([k], g, band(), &mut Seq::default())
                        .unwrap_or_else(|e| panic!("{k:?} at scale {:e}: {e:?}", g.sc));
                }
                r => break r,
            }
        };
        let d = d.map_err(|e| Box::new((*b, e)))?;
        sum += d.degree;
        if matches!(
            box_degree(face, b, patch, qs),
            Err(DegreeError::Through(_) | DegreeError::Unsettled)
        ) {
            sided += 1;
            // Each box alone: its degree about the point really moved,
            // where that certifies it.
            if let Ok(m) = box_degree(face, b, patch, &qm) {
                moved += 1;
                assert_eq!(d.degree, m.degree, "{b:?} against the moved point");
            }
        }
    }
    Ok((sum, sided, moved))
}

/// Where the query point's shadow is put.
#[derive(Clone, Copy, Debug)]
enum Place {
    /// On an inner tile line, at a stored row's point at `t = k/4`.
    Line,
    /// At an inner tile corner.
    Corner,
    /// With an inner tile corner exactly on its `r̂` line, ahead of it or
    /// behind.
    CornerOnRay,
    /// On the inner tile line `v = 1`, made to run along the `r̂` line in
    /// shadow between its two inner corners (the tile edge between them
    /// lies along the query's ray line, both corners on it).
    AlongRay,
}

/// The query point for `place`, exactly over (or under) the face's point
/// there along `UP`.
fn place(rng: &mut Rng, g: &mut Geom, place: Place) -> DVec3 {
    let k = int(rng, 3);
    let on = match place {
        Place::Line => {
            // Inner lines: `u = 1, 2` over a `v` cell, `v = 1` over a `u` cell.
            let e = if rng.next_u64().is_multiple_of(3) {
                TileEdge {
                    operand: g.face_op,
                    face: FACE,
                    fixed: Dir::V,
                    line: 1,
                    cell: (rng.next_u64() % 3) as i64,
                }
            } else {
                TileEdge {
                    operand: g.face_op,
                    face: FACE,
                    fixed: Dir::U,
                    line: 1 + (rng.next_u64() % 2) as i64,
                    cell: (rng.next_u64() % 2) as i64,
                }
            };
            let s = g.edge_span(&e).unwrap();
            // Weights 1: `Σ Bᵢ(t)·pᵢ` at `t ∈ {¼, ½, ¾}`, exact.
            let b: [f64; 3] = match rng.next_u64() % 3 {
                0 => [9.0, 6.0, 1.0],
                1 => [4.0, 8.0, 4.0],
                _ => [1.0, 6.0, 9.0],
            };
            let c = |k: usize| (b[0] * s[0][k] + b[1] * s[1][k] + b[2] * s[2][k]) / 16.0;
            DVec3::new(c(0), c(1), c(2))
        }
        Place::AlongRay => {
            let a = g.grid[2][2];
            let a = DVec3::new(a[0], a[1], a[2]);
            for (i, row) in g.grid.iter_mut().enumerate().take(6).skip(1) {
                let p = a + (R_HAT * (0.5 * (i as f64 - 2.0)) + UP * int(rng, 1)) * g.sc;
                row[2] = [p.x, p.y, p.z, 1.0];
            }
            let m = [-0.25, 0.25, 0.5, 0.75, 1.25][(rng.next_u64() % 5) as usize];
            a + R_HAT * (m * g.sc)
        }
        Place::Corner | Place::CornerOnRay => {
            let p = g.grid[2 * (1 + rng.next_u64() % 2) as usize][2];
            let c = DVec3::new(p[0], p[1], p[2]);
            if matches!(place, Place::Corner) {
                c
            } else {
                let m = [0.25, 0.5, 1.0][(rng.next_u64() % 3) as usize] * g.sc;
                c + R_HAT
                    * if rng.next_u64().is_multiple_of(2) {
                        m
                    } else {
                        -m
                    }
            }
        }
    };
    on + UP * (k * g.sc)
}

/// What [`partitions_sum_to_the_winding`] saw.
#[derive(Debug, Default)]
struct Seen {
    partitions: usize,
    sided: usize,
    moved: usize,
    inside: usize,
}

/// The M5 invariant on faces scaled by `sc`: the degrees of every
/// partition, tile lines and corners through the query included, sum to
/// the winding about the moved point; T-junctions and depths differ
/// across the tile lines. Every partition is certified.
fn partitions_sum_to_the_winding(seeds: std::ops::Range<u64>, sc: f64) -> Seen {
    let trims: [Trim<'_>; 0] = [];
    let mut seen = Seen::default();
    for seed in seeds {
        let mut rng = Rng::new(seed);
        let face_op = if (seed / 4) % 2 == 0 { B } else { A };
        let wild = [0, 4, 12][((seed / 3) % 3) as usize];
        let mut g = Geom::new(&mut rng, face_op, wild).scaled(sc);
        let pl = [
            Place::Line,
            Place::Corner,
            Place::CornerOnRay,
            Place::AlongRay,
        ][(seed % 4) as usize];
        let at = place(&mut rng, &mut g, pl);
        let Some(w) = g.winding(at) else { continue };
        g.pos.insert(g.query(), at);
        let flipped = (seed / 8) % 2 == 1;
        let face = face_uv(&g, &trims, flipped);
        let want = if flipped { -w } else { w };
        let ps = g.patches(&first());
        let qs = QShadow::of([at.x, at.y, at.z]).unwrap();
        let mut sides = Sides::new();
        let whole: Vec<BoxKey> = face.tile_boxes().collect();
        for leaves in [
            whole.clone(),
            partition(&mut rng, &face, &ps, &qs, 0.5, 4, 5),
            partition(&mut rng, &face, &ps, &qs, 0.3, 6, 3),
        ] {
            let (got, n, m) = total(&g, &face, &ps, &leaves, &qs, &mut sides)
                .unwrap_or_else(|e| panic!("seed {seed}, {pl:?} at {at:?}: {e:?}"));
            seen.partitions += 1;
            seen.sided += n;
            seen.moved += m;
            seen.inside += usize::from(want != 0);
            assert_eq!(
                got,
                want,
                "seed {seed}, {pl:?} at {at:?}, face of {face_op:?}, {} boxes",
                leaves.len()
            );
        }
        assert_eq!(sides.replay(&g, band()), Ok(()), "seed {seed}");
    }
    seen
}

#[test]
fn degrees_through_tile_lines_and_corners_sum_to_the_winding() {
    let s = partitions_sum_to_the_winding(varde_testing::seeds(24, 300), 1.0);
    assert!(
        s.moved * 8 >= s.sided,
        "{} of {} sided boxes checked alone",
        s.moved,
        s.sided
    );
    assert!(s.sided * 2 >= s.partitions, "{s:?}");
    assert!(s.inside * 3 >= s.partitions, "{s:?}");
}

/// The same faces at coordinates about `2¹⁹` and about `10⁻³⁰⁰` (powers of
/// two, so every sign is the same), the band still absolute: the records
/// decide (the curved ones at a higher level where the first can't).
#[test]
fn scaled_faces_decide() {
    for sc in [524_288.0, f64::from_bits((1023 - 997) << 52)] {
        let s = partitions_sum_to_the_winding(varde_testing::seeds(8, 100), sc);
        assert!(s.sided > 0, "scale {sc:e}: {s:?}");
    }
}

/// `b` cut at dyadic points of its extents, not shifted off any query
/// (a quarter, a half or three quarters of the way, the direction at
/// random) with probability `p` while shallower than `depth`.
fn cut_freely(rng: &mut Rng, b: BoxKey, p: f64, depth: u32, out: &mut Vec<BoxKey>) {
    if b.depth() < depth && rng.unit() < p {
        let dir = if rng.next_u64().is_multiple_of(2) {
            Dir::U
        } else {
            Dir::V
        };
        let (lo, hi, e) = b.extent(dir).nums();
        let k = 1 + rng.next_u64() % 3;
        let at = Dyadic::new(4 * lo + k * (hi - lo), e + 2).unwrap();
        if let Some(halves) = b.split_at(dir, at) {
            for h in halves {
                cut_freely(rng, h, p, depth, out);
            }
            return;
        }
    }
    out.push(b);
}

/// Additivity inside a tile: the query point exactly on the face at a
/// dyadic point of a tile, `(½, k/4)` local, so the iso lines `u = ½`
/// and `v = k/4` of the tile pass through its shadow; the tile cut along
/// one of them and each side refined on its own, freely (T-junctions
/// along the line, box corners on it, at the query's point among them).
/// Box sides on those lines are read from the inner lines' records, cuts
/// inside their crossings ordered by the dyadic points' records; the
/// degrees sum to the winding about the moved point, and each box read
/// with a record is its degree about the point really moved where that
/// is certified.
#[test]
fn inner_lines_through_the_query_add_up_across_t_junctions() {
    let trims: [Trim<'_>; 0] = [];
    let (mut inner, mut checked, mut sided) = (0, 0, 0);
    for seed in varde_testing::seeds(16, 200) {
        let mut rng = Rng::new(seed);
        let face_op = if seed % 2 == 0 { B } else { A };
        let wild = [0, 4, 12][((seed / 2) % 3) as usize];
        let mut g = Geom::new(&mut rng, face_op, wild).unit_weights();
        let tile = [(rng.next_u64() % 3) as i64, (rng.next_u64() % 2) as i64];
        let net = g.net(tile);
        // Weights 1: `Σ Bᵢ(½)·Bⱼ(k/4)·pᵢⱼ`, exact (sixty-fourths of
        // integers).
        let k = 1 + rng.next_u64() % 3;
        let bu = [1.0, 2.0, 1.0];
        let bv = [(4 - k) * (4 - k), 2 * k * (4 - k), k * k].map(|x| x as f64);
        let mut at = DVec3::ZERO;
        for (n, p) in net.iter().enumerate() {
            let c = bu[n / 3] * bv[n % 3] / 64.0;
            at += DVec3::new(p[0], p[1], p[2]) * c;
        }
        let at = at + UP * int(&mut rng, 3);
        let Some(w) = g.winding(at) else { continue };
        g.pos.insert(g.query(), at);
        let flipped = seed % 4 >= 2;
        let face = face_uv(&g, &trims, flipped);
        let want = if flipped { -w } else { w };
        let ps = g.patches(&first());
        let qs = QShadow::of([at.x, at.y, at.z]).unwrap();
        let mut sides = Sides::new();
        for _ in 0..3 {
            let mut leaves = Vec::new();
            for b in face.tile_boxes() {
                if b.tile() != tile {
                    cut_freely(&mut rng, b, 0.3, 2, &mut leaves);
                    continue;
                }
                let (dir, cut) = if rng.next_u64().is_multiple_of(2) {
                    (Dir::U, Dyadic::new(1, 1).unwrap())
                } else {
                    (Dir::V, Dyadic::new(k, 2).unwrap())
                };
                for h in b.split_at(dir, cut).unwrap() {
                    cut_freely(&mut rng, h, 0.6, 5, &mut leaves);
                }
            }
            let (got, n, m) = total(&g, &face, &ps, &leaves, &qs, &mut sides)
                .unwrap_or_else(|e| panic!("seed {seed} at {at:?}: {e:?}"));
            sided += n;
            checked += m;
            assert_eq!(
                got,
                want,
                "seed {seed} at {at:?}, face of {face_op:?}, {} boxes",
                leaves.len()
            );
        }
        inner += sides
            .side_table()
            .iter()
            .filter(|(k, _)| matches!(k.line, SideLine::Inner(_)))
            .count();
        assert_eq!(sides.replay(&g, band()), Ok(()), "seed {seed}");
    }
    assert!(
        inner > 0 && sided > 0,
        "{inner} inner records, {sided} sided boxes"
    );
    assert!(
        checked * 8 >= sided,
        "{checked} of {sided} sided boxes checked alone"
    );
}

/// The lines of the face: every tile edge, and an inner iso line of each
/// tile at a random dyadic coordinate (`k/2^d`, `d ≤ 6`).
fn lines(rng: &mut Rng, g: &Geom) -> Vec<SideLine> {
    let mut out: Vec<SideLine> = (0..=3)
        .flat_map(|line| (0..2).map(move |cell| (Dir::U, line, cell)))
        .chain((0..=2).flat_map(|line| (0..3).map(move |cell| (Dir::V, line, cell))))
        .map(|(fixed, line, cell)| {
            SideLine::Edge(TileEdge {
                operand: g.face_op,
                face: FACE,
                fixed,
                line,
                cell,
            })
        })
        .collect();
    for i in TILES[0].clone() {
        for j in TILES[1].clone() {
            let d = 1 + (rng.next_u64() % 6) as u32;
            let k = 1 + 2 * (rng.next_u64() % (1 << (d - 1)));
            out.push(SideLine::Inner(InnerLine {
                operand: g.face_op,
                face: FACE,
                tile: [i, j],
                fixed: if rng.next_u64().is_multiple_of(2) {
                    Dir::U
                } else {
                    Dir::V
                },
                at: Dyadic::new(k, d).unwrap(),
            }));
        }
    }
    out
}

/// The count of `line`'s record over `[x, y]`, the cuts' sides read from
/// their dyadic points' records, filled as asked.
fn part(g: &Geom, sides: &mut Sides, line: SideLine, [x, y]: [Dyadic; 2]) -> Result<i64, Over> {
    let mut cut = [None, None];
    loop {
        let rec = sides.side(g.query(), line).expect("decided");
        match rec.count_over(g.point_op(), [dy(x), dy(y)], cut) {
            Err(Over::Side(j)) => {
                let corner = line.point([x, y][j]).unwrap();
                let key = CornerKey {
                    q: g.query(),
                    corner,
                };
                sides
                    .fill([key], g, band(), &mut Seq::default())
                    .expect("a point decides");
                cut[j] = Some(sides.corner(g.query(), corner).unwrap().side);
            }
            r => return r,
        }
    }
}

/// A line's count is the sum of its parts' over any dyadic cuts, a cut
/// inside a crossing's enclosure ordered by its dyadic point's record, on
/// tile edges and inner lines alike; each record is decided once, its
/// ends first.
#[test]
fn line_counts_add_up_over_parts() {
    let (mut parts, mut ordered) = (0, 0);
    for seed in varde_testing::seeds(16, 128) {
        let mut rng = Rng::new(seed);
        let mut g = Geom::new(&mut rng, if seed % 2 == 0 { B } else { A }, 8);
        let at = place(
            &mut rng,
            &mut g,
            [Place::Line, Place::AlongRay][(seed / 2 % 2) as usize],
        );
        g.pos.insert(g.query(), at);
        let lines = lines(&mut rng, &g);
        let keys: Vec<SideKey> = lines
            .iter()
            .map(|&line| SideKey { q: g.query(), line })
            .collect();
        let ends: std::collections::BTreeSet<TilePoint> =
            lines.iter().flat_map(|l| l.ends().unwrap()).collect();
        let mut sides = Sides::new();
        let mut pass = Seq::default();
        sides
            .fill(keys.clone(), &g, band(), &mut pass)
            .expect("decides");
        // Each end once, each line once.
        assert_eq!(pass.decided, ends.len() + lines.len(), "seed {seed}");
        sides
            .fill(keys.clone(), &g, band(), &mut pass)
            .expect("nothing to decide");
        assert_eq!(
            pass.decided,
            ends.len() + lines.len(),
            "seed {seed}: decided again"
        );
        for &line in &lines {
            let whole = sides.side(g.query(), line).unwrap().count();
            for _ in 0..4 {
                let d = 1 + (rng.next_u64() % 5) as u32;
                let mut cuts: Vec<Dyadic> = (1..1u64 << d)
                    .filter(|_| rng.unit() < 0.5)
                    .map(|k| Dyadic::new(k, d).unwrap())
                    .collect();
                cuts.insert(0, Dyadic::ZERO);
                cuts.push(Dyadic::ONE);
                let before = sides.corner_table().len();
                let sum: Result<i64, Over> = cuts
                    .windows(2)
                    .map(|w| part(&g, &mut sides, line, [w[0], w[1]]))
                    .sum();
                ordered += usize::from(sides.corner_table().len() > before);
                match sum {
                    Ok(s) => {
                        parts += 1;
                        assert_eq!(s, whole, "seed {seed}: {line:?} cut at {cuts:?}");
                    }
                    Err(e) => assert_eq!(e, Over::Unordered, "seed {seed}"),
                }
            }
        }
        assert_eq!(sides.replay(&g, band()), Ok(()), "seed {seed}");
    }
    assert!(
        parts > 0 && ordered > 0,
        "{parts} parts, {ordered} ordered by a cut"
    );
}

#[test]
fn count_over_orders_cuts_inside_crossings() {
    let c = |t: [f64; 2], sign: Sign, single: bool| SideCrossing { t, sign, single };
    let rec = SideRec {
        crossings: [
            c([0.0, 0.0], Sign::Pos, true),
            c([0.375, 0.5], Sign::Neg, true),
            c([0.75, 0.875], Sign::Pos, false),
            c([1.0, 1.0], Sign::Neg, true),
        ]
        .into_iter()
        .collect(),
        touches: [[0.125, 0.1875]].into_iter().collect(),
    };
    let none = [None, None];
    assert_eq!(rec.count(), 0);
    assert_eq!(rec.count_over(B, [0.0, 0.25], none), Ok(1));
    assert_eq!(rec.count_over(B, [0.25, 0.625], none), Ok(-1));
    assert_eq!(rec.count_over(B, [0.625, 1.0], none), Ok(0));
    assert_eq!(rec.count_over(B, [0.0, 1.0], none), Ok(0));
    // A cut inside the second crossing's enclosure: its side says which
    // part has it. The crossing (`−` for `B`'s `+r̂` ray) goes to the
    // right of the ray, so a cut on the right lies after it.
    assert_eq!(rec.count_over(B, [0.5, 1.0], none), Err(Over::Side(0)));
    assert_eq!(rec.count_over(B, [0.0, 0.4375], none), Err(Over::Side(1)));
    let (left, right) = (Some(Sign::Pos), Some(Sign::Neg));
    for (cut, before) in [(right, -1), (left, 0)] {
        let after = rec.count_over(B, [0.4375, 1.0], [cut, None]).unwrap();
        let first = rec.count_over(B, [0.0, 0.4375], [None, cut]).unwrap();
        assert_eq!(first, 1 + before);
        assert_eq!(first + after, rec.count());
    }
    // `A`'s ray runs `−r̂`: the same crossing goes to the left.
    assert_eq!(rec.count_over(A, [0.0, 0.4375], [None, left]), Ok(0));
    // A tangency's crossing isn't ordered against a cut.
    assert_eq!(
        rec.count_over(B, [0.0, 0.8125], [None, left]),
        Err(Over::Unordered)
    );
    // Nor is a tangency whose two crossings cancel: a cut inside it holds
    // one of them, whichever side the cut's point is on.
    for cut in [left, right] {
        assert_eq!(
            rec.count_over(B, [0.15625, 1.0], [cut, None]),
            Err(Over::Unordered)
        );
    }
    assert_eq!(rec.count_over(B, [0.0, 0.25], none), Ok(1));
}

#[test]
fn refuses_wrong_keys() {
    let mut rng = Rng::new(7);
    let mut g = Geom::new(&mut rng, B, 0);
    g.pos.insert(qp(), DVec3::new(10.0, 10.0, 0.0));
    let edge = TileEdge {
        operand: B,
        face: FACE,
        fixed: Dir::U,
        line: 1,
        cell: 0,
    };
    // The point on the face's own operand.
    let mut sides = Sides::new();
    let line = SideLine::Edge(edge);
    let r = sides.fill([SideKey { q: qp(), line }], &g, band(), &mut Seq::default());
    assert!(
        matches!(r, Err(ShadowError::Pass(Fail::Internal { .. }))),
        "{r:?}"
    );
    assert!(sides.side_table().is_empty());
    // An edge the face hasn't got.
    let a = Point::vert(A, VertId(2));
    g.pos.insert(a, DVec3::new(10.0, 10.0, 0.0));
    let far = SideLine::Edge(TileEdge { cell: 7, ..edge });
    let r = sides.fill(
        [SideKey { q: a, line: far }],
        &g,
        band(),
        &mut Seq::default(),
    );
    assert!(
        matches!(r, Err(ShadowError::Pass(Fail::Internal { .. }))),
        "{r:?}"
    );
    // A sided degree asked by a point of the face's operand.
    let trims: [Trim<'_>; 0] = [];
    let face = face_uv(&g, &trims, false);
    let ps = g.patches(&first());
    let b = face.tile_boxes().next().unwrap();
    let qs = QShadow::of([10.0, 10.0, 0.0]).unwrap();
    assert!(matches!(
        box_degree_sided(&face, &b, patch_of(&ps, &b), &qs, qp(), &sides),
        Err(SidedError::Box(_))
    ));
}

/// A tile whose patch's weights aren't certified positive has no degree
/// from its sides, recorded or not: the box is unsettled, not counted.
#[test]
fn weights_not_positive_leave_the_box_unsettled() {
    let mut g = Geom::new(&mut Rng::new(3), B, 0);
    // The query exactly at the middle of the inner tile edge `u = 1` over
    // the `v` cell 0 (its row has weights 1), the tile `[1, 0]` beside it
    // given a negative inner weight.
    let e = g
        .edge_span(&TileEdge {
            operand: B,
            face: FACE,
            fixed: Dir::U,
            line: 1,
            cell: 0,
        })
        .unwrap();
    let mid = |k: usize| (e[0][k] + 2.0 * e[1][k] + e[2][k]) / 4.0;
    let at = DVec3::new(mid(0), mid(1), mid(2));
    g.grid[3][1][3] = -1.0;
    g.pos.insert(g.query(), at);
    let trims: [Trim<'_>; 0] = [];
    let face = face_uv(&g, &trims, false);
    let ps = g.patches(&first());
    let qs = QShadow::of([at.x, at.y, at.z]).unwrap();
    let b = face
        .tile_boxes()
        .find(|b| b.tile() == [1, 0])
        .expect("the tile");
    let patch = patch_of(&ps, &b);
    assert!(!patch.weights_positive());
    let mut sides = Sides::new();
    let d = loop {
        match box_degree_sided(&face, &b, patch, &qs, g.query(), &sides) {
            Err(SidedError::Need(k)) => sides
                .fill([k], &g, band(), &mut Seq::default())
                .expect("a side record decides"),
            r => break r,
        }
    };
    assert_eq!(d, Err(SidedError::Unsettled));
}

/// Additivity along an inner line lying along the query's ray line: the
/// iso line `u = ½` of a tile made to run along the `r̂` line in shadow
/// (its control points on it), the query exactly on that line, before
/// it, on it or past it; the tile cut along the line and each side
/// refined on its own, freely, box corners on the line among them (every
/// point of the line a tie). The degrees sum to the winding about the
/// moved point, every box settled.
#[test]
fn an_inner_line_along_the_ray_adds_up() {
    let trims: [Trim<'_>; 0] = [];
    let (mut sided, mut inner, mut cuts) = (0, 0, 0);
    for seed in varde_testing::seeds(16, 200) {
        let mut rng = Rng::new(seed);
        let face_op = if seed % 2 == 0 { B } else { A };
        let wild = [0, 4, 12][((seed / 2) % 3) as usize];
        let mut g = Geom::new(&mut rng, face_op, wild).unit_weights();
        let tile = [(rng.next_u64() % 3) as i64, (rng.next_u64() % 2) as i64];
        let (ci, cj) = (2 * tile[0] as usize, 2 * tile[1] as usize);
        // The line's control point `b` (weights 1): `¼·n₀ + ½·n₁ + ¼·n₂`
        // of the net's column `b`. The one on the face's outer boundary
        // row is kept; the others put on the `r̂` line through it, `½`
        // apart along `r̂`, each `n₁` solved for exactly (eighths and
        // their quarters, exact in `f64`).
        let line_pt = |g: &Geom, b: usize| {
            let p = |a: usize| {
                let x = g.grid[ci + a][cj + b];
                DVec3::new(x[0], x[1], x[2])
            };
            p(0) * 0.25 + p(1) * 0.5 + p(2) * 0.25
        };
        let keep = if tile[1] == 0 { 0 } else { 2 };
        let a0 = line_pt(&g, keep);
        for b in 0..3 {
            if b == keep {
                continue;
            }
            let s = 0.5 * (b as f64 - keep as f64);
            let want = a0 + R_HAT * s + UP * int(&mut rng, 1);
            let n0 = g.grid[ci][cj + b];
            let n2 = g.grid[ci + 2][cj + b];
            let n = &mut g.grid[ci + 1][cj + b];
            for k in 0..3 {
                n[k] = 2.0 * (want[k] - 0.25 * n0[k] - 0.25 * n2[k]);
            }
        }
        let dir = if keep == 0 { 1.0 } else { -1.0 };
        let m = [-0.25, 0.25, 0.5, 0.75, 1.25][(rng.next_u64() % 5) as usize];
        let at = a0 + R_HAT * (dir * m) + UP * int(&mut rng, 3);
        let Some(w) = g.winding(at) else { continue };
        g.pos.insert(g.query(), at);
        let flipped = seed % 4 >= 2;
        let face = face_uv(&g, &trims, flipped);
        let want = if flipped { -w } else { w };
        let ps = g.patches(&first());
        let qs = QShadow::of([at.x, at.y, at.z]).unwrap();
        let mut sides = Sides::new();
        for _ in 0..3 {
            let mut leaves = Vec::new();
            for b in face.tile_boxes() {
                if b.tile() != tile {
                    cut_freely(&mut rng, b, 0.3, 2, &mut leaves);
                    continue;
                }
                for h in b.split_at(Dir::U, Dyadic::new(1, 1).unwrap()).unwrap() {
                    cut_freely(&mut rng, h, 0.6, 5, &mut leaves);
                }
            }
            let (got, n, _) = total(&g, &face, &ps, &leaves, &qs, &mut sides)
                .unwrap_or_else(|e| panic!("seed {seed} at {at:?}, m {m}: {e:?}"));
            sided += n;
            assert_eq!(
                got,
                want,
                "seed {seed} at {at:?}, m {m}, face of {face_op:?}, {} boxes",
                leaves.len()
            );
        }
        inner += sides
            .side_table()
            .iter()
            .filter(|(k, _)| matches!(k.line, SideLine::Inner(l) if l.tile == tile && l.fixed == Dir::U))
            .count();
        cuts += sides
            .corner_table()
            .iter()
            .filter(|(k, _)| {
                k.corner.tile == tile
                    && k.corner.at[0] == Dyadic::new(1, 1).unwrap()
                    && k.corner.at[1] != Dyadic::ZERO
            })
            .count();
        assert_eq!(sides.replay(&g, band()), Ok(()), "seed {seed}");
    }
    eprintln!("sided {sided} inner {inner} cuts {cuts}");
    assert!(
        sided > 0 && inner > 0,
        "{sided} sided boxes, {inner} records of the line"
    );
}
