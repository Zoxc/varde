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
use crate::bezier::{Dir, HomPatch, Scalar};
use crate::count::boxes::{BoxKey, FaceUv, QShadow, Trim, split, sub_patch};
use crate::count::degree::{DegreeError, SidedError, box_degree, box_degree_sided, hull_off};
use crate::ids::{EdgeId, FaceId, VertId};
use crate::num::bounded::Bounded;
use crate::perturb::{NU, R_HAT, Seg, T, UP, rates_a};

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
        }
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
            self.d * (sigma / 1_048_576.0) + T[0] / 1_073_741_824.0 + T[1] / 1_099_511_627_776.0;
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

    fn edge_rates<S: Curved>(&self, _: Point, _: &TileEdge, t: &S) -> [[S; 3]; ORDERS] {
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

/// What summing a partition's degrees came to.
#[derive(Debug, PartialEq)]
enum Sum {
    /// The sum, how many boxes read a side record, and how many of those
    /// were checked against the point really moved.
    Total(i64, usize, usize),
    /// A box no record certifies (a side inside a tile through the
    /// query, or unsettled).
    Uncertified(SidedError),
}

/// The sum of the leaves' degrees, filling the side records they ask for.
fn total(
    g: &Geom,
    face: &FaceUv<'_>,
    ps: &[HomPatch<Bounded<f64>>],
    leaves: &[BoxKey],
    qs: &QShadow,
    sides: &mut Sides,
) -> Sum {
    let (mut sum, mut sided, mut moved) = (0, 0, 0);
    let qm = g.moved_query();
    for b in leaves {
        let patch = patch_of(ps, b);
        let d = loop {
            match box_degree_sided(face, b, patch, qs, g.query(), sides) {
                Err(SidedError::Need(k)) => {
                    sides
                        .fill([k], g, band(), &mut Seq::default())
                        .expect("a side record decides");
                }
                r => break r,
            }
        };
        match d {
            Ok(d) => {
                sum += d.degree;
                if matches!(
                    box_degree(face, b, patch, qs),
                    Err(DegreeError::Through(_) | DegreeError::Unsettled)
                ) {
                    sided += 1;
                    // Each box alone: its degree about the point really
                    // moved, where that certifies it.
                    if let Ok(m) = box_degree(face, b, patch, &qm) {
                        moved += 1;
                        assert_eq!(d.degree, m.degree, "{b:?} against the moved point");
                    }
                }
            }
            Err(e) => return Sum::Uncertified(e),
        }
    }
    Sum::Total(sum, sided, moved)
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
                let p = a + R_HAT * (0.5 * (i as f64 - 2.0)) + UP * int(rng, 1);
                row[2] = [p.x, p.y, p.z, 1.0];
            }
            let m = [-0.25, 0.25, 0.5, 0.75, 1.25][(rng.next_u64() % 5) as usize];
            a + R_HAT * m
        }
        Place::Corner | Place::CornerOnRay => {
            let p = g.grid[2 * (1 + rng.next_u64() % 2) as usize][2];
            let c = DVec3::new(p[0], p[1], p[2]);
            if matches!(place, Place::Corner) {
                c
            } else {
                let m = [0.25, 0.5, 1.0][(rng.next_u64() % 3) as usize];
                c + R_HAT
                    * if rng.next_u64().is_multiple_of(2) {
                        m
                    } else {
                        -m
                    }
            }
        }
    };
    on + UP * k
}

/// The M5 invariant: the degrees of every partition, tile lines and
/// corners through the query included, sum to the winding about the
/// moved point; T-junctions and depths differ across the tile lines.
#[test]
fn degrees_through_tile_lines_and_corners_sum_to_the_winding() {
    let trims: [Trim<'_>; 0] = [];
    let (mut tried, mut used, mut sided, mut inside) = (0, 0, 0, 0);
    let mut moved = 0;
    for seed in varde_testing::seeds(24, 300) {
        let mut rng = Rng::new(seed);
        let face_op = if (seed / 4) % 2 == 0 { B } else { A };
        let wild = [0, 4, 12][((seed / 3) % 3) as usize];
        let mut g = Geom::new(&mut rng, face_op, wild);
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
            tried += 1;
            match total(&g, &face, &ps, &leaves, &qs, &mut sides) {
                Sum::Total(got, n, m) => {
                    used += 1;
                    sided += n;
                    moved += m;
                    inside += usize::from(want != 0);
                    assert_eq!(
                        got,
                        want,
                        "seed {seed}, {pl:?} at {at:?}, face of {face_op:?}, {} boxes",
                        leaves.len()
                    );
                }
                Sum::Uncertified(e) => {
                    // Only a box corner too close to the query's ray line
                    // inside a tile edge, or an interior side through it.
                    assert!(
                        matches!(e, SidedError::Unsettled | SidedError::Through(_)),
                        "seed {seed}: {e:?}"
                    );
                }
            }
        }
        assert_eq!(sides.replay(&g, band()), Ok(()), "seed {seed}");
    }
    assert!(
        moved * 8 >= sided,
        "{moved} of {sided} sided boxes checked alone"
    );
    assert!(
        used * 10 >= tried * 8,
        "{used} of {tried} partitions certified"
    );
    assert!(
        sided * 2 >= used,
        "{sided} boxes read side records in {used}"
    );
    assert!(inside * 3 >= used, "{inside} of {used} queries inside");
}

/// A tile edge's count is the sum of its parts' over any dyadic cuts the
/// crossings don't straddle, and its record is decided once.
#[test]
fn edge_counts_add_up_over_parts() {
    let mut parts = 0;
    for seed in varde_testing::seeds(16, 128) {
        let mut rng = Rng::new(seed);
        let mut g = Geom::new(&mut rng, if seed % 2 == 0 { B } else { A }, 8);
        let at = place(&mut rng, &mut g, Place::Line);
        g.pos.insert(g.query(), at);
        let edges: Vec<TileEdge> = (0..=3)
            .flat_map(|line| (0..2).map(move |cell| (Dir::U, line, cell)))
            .chain((0..=2).flat_map(|line| (0..3).map(move |cell| (Dir::V, line, cell))))
            .map(|(fixed, line, cell)| TileEdge {
                operand: g.face_op,
                face: FACE,
                fixed,
                line,
                cell,
            })
            .collect();
        let keys: Vec<SideKey> = edges
            .iter()
            .map(|&edge| SideKey { q: g.query(), edge })
            .collect();
        let mut sides = Sides::new();
        let mut pass = Seq::default();
        sides
            .fill(keys.clone(), &g, band(), &mut pass)
            .expect("decides");
        // Each corner once, each edge once.
        assert_eq!(pass.decided, 12 + edges.len(), "seed {seed}");
        sides
            .fill(keys.clone(), &g, band(), &mut pass)
            .expect("nothing to decide");
        assert_eq!(pass.decided, 12 + edges.len(), "seed {seed}: decided again");
        for &e in &edges {
            let rec = sides.side(g.query(), e).unwrap();
            for _ in 0..4 {
                let n = 1u64 << (1 + rng.next_u64() % 5);
                let mut cuts: Vec<f64> = (1..n)
                    .filter(|_| rng.unit() < 0.5)
                    .map(|k| k as f64 / n as f64)
                    .collect();
                cuts.insert(0, 0.0);
                cuts.push(1.0);
                let sum: Option<i64> = cuts.windows(2).map(|w| rec.count_over([w[0], w[1]])).sum();
                if let Some(s) = sum {
                    parts += 1;
                    assert_eq!(s, rec.count(), "seed {seed}: {e:?} cut at {cuts:?}");
                }
            }
        }
        assert_eq!(sides.replay(&g, band()), Ok(()), "seed {seed}");
    }
    assert!(parts > 0);
}

#[test]
fn count_over_refuses_straddled_cuts() {
    let c = |t: [f64; 2], s: Sign| Crossing { t, sign: s };
    let rec = SideRec {
        crossings: [
            c([0.0, 0.0], Sign::Pos),
            c([0.375, 0.5], Sign::Neg),
            c([1.0, 1.0], Sign::Neg),
        ]
        .into_iter()
        .collect(),
    };
    assert_eq!(rec.count(), -1);
    assert_eq!(rec.count_over([0.0, 0.25]), Some(1));
    assert_eq!(rec.count_over([0.25, 0.75]), Some(-1));
    assert_eq!(rec.count_over([0.75, 1.0]), Some(-1));
    assert_eq!(rec.count_over([0.5, 1.0]), None);
    assert_eq!(rec.count_over([0.0, 0.375]), None);
    assert_eq!(rec.count_over([0.0, 1.0]), Some(-1));
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
    let r = sides.fill([SideKey { q: qp(), edge }], &g, band(), &mut Seq::default());
    assert!(
        matches!(r, Err(ShadowError::Pass(Fail::Internal { .. }))),
        "{r:?}"
    );
    assert!(sides.side_table().is_empty());
    // An edge the face hasn't got.
    let a = Point::vert(A, VertId(2));
    g.pos.insert(a, DVec3::new(10.0, 10.0, 0.0));
    let far = TileEdge { cell: 7, ..edge };
    let r = sides.fill(
        [SideKey { q: a, edge: far }],
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
