//! Sheet fixtures for the snap: pairs of bodies whose surfaces run within
//! a small offset of each other, as plain data (vertex positions and
//! faces as loops of vertex indices; a cylinder's parameters and a
//! biquadratic net), independent of the kernel, which builds its bodies
//! from them.
//!
//! **Polygon bodies** ([`Polygons`]): planar faces, each one loop of
//! vertex indices running counter-clockwise seen from the face's outward
//! side (the side its normal points to); a body is `closed` (a solid,
//! every edge used by two faces, once each way) or an open sheet (edges
//! on its boundary used once). Every vertex lies on the common grid of
//! its pair ([`snap`]): the sum of any two coordinates is exact, so a
//! line span from one vertex to another with the rounded midpoint as
//! its middle control point is straight exactly. Coordinates are mm.
//!
//! **The pairs** ([`Pair`]), `a` and `b`, the offset `δ` given (it must
//! be below the snap's decision band, `ε_dec = ε/2`, for the pair to be
//! a tangency cluster; the defaults use `2·10⁻⁴` against `ε = 10⁻³`):
//! - [`wavy`]: two triangulated wavy sheets over one square grid, `b`
//!   the heightfield of `a` raised by `δ`, over all of `a`'s cells or an
//!   inset of them, triangulated alike or with the other diagonals. The
//!   heightfield is a sum of a wave in `x` and one in `y`, so each grid
//!   cell's four corners are coplanar (to their rounding) and both
//!   diagonals give the same surface: `b` lies `δ` above `a` everywhere,
//!   whatever its triangulation;
//! - [`box_edge_fold`]: a box (closed) and a sheet folded over one of its
//!   edges, its two quads `δ` outside the two faces at that edge;
//! - [`l_corner`]: an L-shaped prism (closed) and a sheet folded into its
//!   reflex edge, `δ` outside the two faces there;
//! - [`l_corner_quad`]: a flat L-shaped sheet (one hexagon) and a quad
//!   `δ` above its corner square, flush with the L's two outer edges at
//!   that corner (after the snap, the zero-area regions along them);
//! - [`smooth`]: a biquadratic sheet over a cylinder of radius 50
//!   ([`SmoothPair`]), the only curved pair.
//!
//! [`Polygons::relabelled`] renumbers a body's vertices and faces and
//! turns each loop to start elsewhere, for the id-permutation runs.

use crate::rng::Rng;

/// The default offset of the pairs (mm): below `ε_dec = 5·10⁻⁴` of the
/// default tolerance `ε = 10⁻³`.
pub const OFFSET: f64 = 2e-4;

/// A body of planar polygon faces (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Polygons {
    /// A closed solid, or an open sheet.
    pub closed: bool,
    /// Vertex positions (mm).
    pub verts: Vec<[f64; 3]>,
    /// Per face its loop of vertex indices, counter-clockwise about its
    /// outward normal.
    pub faces: Vec<Vec<u32>>,
}

/// Two bodies near each other (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Pair {
    pub a: Polygons,
    pub b: Polygons,
}

/// Which diagonal each grid cell of a wavy sheet is cut along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diagonals {
    /// From the cell's low corner `(i, j)` to its high one.
    Rising,
    /// From `(i + 1, j)` to `(i, j + 1)`.
    Falling,
    /// Rising where `i + j` is even, falling where it is odd.
    Alternating,
}

/// A wavy pair's shape ([`wavy`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wavy {
    /// Grid cells along each side of `a`.
    pub cells: u32,
    /// The side of `a`'s square (mm), from the origin along `x` and `y`.
    pub size: f64,
    /// The amplitude of each of the two waves (mm).
    pub amplitude: f64,
    /// Whole waves along each side.
    pub waves: u32,
    /// `b`'s height above `a` (mm).
    pub offset: f64,
    /// `a`'s triangulation.
    pub diagonals_a: Diagonals,
    /// `b`'s triangulation.
    pub diagonals_b: Diagonals,
    /// The cells `b` leaves out along each side of `a`'s grid: `b` covers
    /// cells `inset..cells − inset` in each direction.
    pub inset: u32,
}

impl Wavy {
    /// An 8 × 8 grid over 80 mm, waves of 2 mm, `b` [`OFFSET`] above `a`
    /// over all of it, triangulated with the other diagonals.
    pub const DEFAULT: Wavy = Wavy {
        cells: 8,
        size: 80.0,
        amplitude: 2.0,
        waves: 2,
        offset: OFFSET,
        diagonals_a: Diagonals::Rising,
        diagonals_b: Diagonals::Falling,
        inset: 0,
    };
}

/// A biquadratic sheet over a cylinder (module docs): the cylinder of
/// [`SMOOTH_RADIUS`] about the line through `axis_point` along `axis`
/// (the `y` axis), the sheet over its top (`+z`), touching nowhere: its
/// gap to the cylinder is least, `gap`, above the axis's origin, and
/// grows away from there along both `x` and `y`.
#[derive(Clone, Debug, PartialEq)]
pub struct SmoothPair {
    /// A point of the cylinder's axis.
    pub axis_point: [f64; 3],
    /// The axis's direction.
    pub axis: [f64; 3],
    /// The radius (mm).
    pub radius: f64,
    /// The range along the axis from `axis_point` the cylinder's face
    /// covers (its upper half, `z ≥ 0`), past the sheet's on both sides.
    pub along: [f64; 2],
    /// The sheet's net, `(x, y, z, w)` row-major in `u` (control point
    /// `(i, j)` at `3·i + j`, `u` along `x`, `v` along `y`), weights 1.
    pub net: [[f64; 4]; 9],
    /// The least gap from the sheet to the cylinder, at the net's middle
    /// control point's `x, y` (mm).
    pub gap: f64,
}

/// The cylinder's radius in the smooth pair (mm).
pub const SMOOTH_RADIUS: f64 = 50.0;

/// The smooth pair with the least gap `gap` (mm): over `x, y ∈ [−10, 10]`
/// the sheet is `z = r + gap − k·x²/(2r) + y²/400`, `k = ½`, a quadratic
/// in each of `x` and `y` (so a biquadratic patch exactly); the cylinder
/// `z = √(r² − x²) ≤ r − x²/(2r)` lies below it by at least
/// `gap + (1 − k)·x²/(2r) + y²/400`.
pub fn smooth(gap: f64) -> SmoothPair {
    let r = SMOOTH_RADIUS;
    let (half_x, half_y, k, q) = (10.0, 10.0, 0.5, 1.0 / 400.0);
    // A quadratic `c·s²` over `s ∈ [−h, h]` has the Bernstein
    // coefficients `c·h²·(1, −1, 1)`; the constant has all ones, and a
    // sum of quadratics in `x` and in `y` adds them per control point.
    let bern = |h: f64| [h * h, -h * h, h * h];
    let (bx, by) = (bern(half_x), bern(half_y));
    let xs = [-half_x, 0.0, half_x];
    let ys = [-half_y, 0.0, half_y];
    let net = std::array::from_fn(|n| {
        let (i, j) = (n / 3, n % 3);
        let z = r + gap - k / (2.0 * r) * bx[i] + q * by[j];
        [xs[i], ys[j], z, 1.0]
    });
    SmoothPair {
        axis_point: [0.0; 3],
        axis: [0.0, 1.0, 0.0],
        radius: r,
        along: [-20.0, 20.0],
        net,
        gap,
    }
}

/// The wavy pair of `w` (module docs). Panics unless `w.cells` is at
/// least `2·w.inset + 1` and `w.size` is positive.
pub fn wavy(w: &Wavy) -> Pair {
    assert!(w.cells > 2 * w.inset && w.size > 0.0, "{w:?}");
    let n = w.cells;
    let h = w.size / n as f64;
    let wave = |s: f64| {
        let turns = s / w.size * w.waves as f64;
        w.amplitude * libm::sin(2.0 * std::f64::consts::PI * turns)
    };
    let height = |i: u32, j: u32| wave(i as f64 * h) + wave(j as f64 * h);
    let sheet = |lo: u32, hi: u32, raise: f64, diagonals: Diagonals| {
        let side = hi - lo + 1;
        let id = |i: u32, j: u32| (i - lo) * side + (j - lo);
        let mut verts = Vec::new();
        for i in lo..=hi {
            for j in lo..=hi {
                verts.push([i as f64 * h, j as f64 * h, height(i, j) + raise]);
            }
        }
        let mut faces = Vec::new();
        for i in lo..hi {
            for j in lo..hi {
                let [p00, p10, p11, p01] = [id(i, j), id(i + 1, j), id(i + 1, j + 1), id(i, j + 1)];
                let rising = match diagonals {
                    Diagonals::Rising => true,
                    Diagonals::Falling => false,
                    Diagonals::Alternating => (i + j) % 2 == 0,
                };
                // Counter-clockwise seen from above: the normal is up.
                if rising {
                    faces.push(vec![p00, p10, p11]);
                    faces.push(vec![p00, p11, p01]);
                } else {
                    faces.push(vec![p00, p10, p01]);
                    faces.push(vec![p10, p11, p01]);
                }
            }
        }
        Polygons {
            closed: false,
            verts,
            faces,
        }
    };
    snapped(Pair {
        a: sheet(0, n, 0.0, w.diagonals_a),
        b: sheet(w.inset, n - w.inset, w.offset, w.diagonals_b),
    })
}

/// A box `[0, 40] × [0, 30] × [0, 20]` and a sheet folded over its edge
/// `y = 0, z = 20`: a quad `δ` above the top over `x ∈ [10, 30]`,
/// `y ∈ [−δ, 15]`, and one `δ` in front of the front face (`y = −δ`) over
/// `z ∈ [5, 20 + δ]`, sharing the fold edge `y = −δ, z = 20 + δ`; both
/// face away from the box, as the box's faces there do.
pub fn box_edge_fold(offset: f64) -> Pair {
    let d = offset;
    let a = prism(
        &[[0.0, 0.0], [40.0, 0.0], [40.0, 30.0], [0.0, 30.0]],
        0.0,
        20.0,
    );
    let verts = vec![
        [10.0, -d, 20.0 + d],
        [30.0, -d, 20.0 + d],
        [30.0, 15.0, 20.0 + d],
        [10.0, 15.0, 20.0 + d],
        [10.0, -d, 5.0],
        [30.0, -d, 5.0],
    ];
    let b = Polygons {
        closed: false,
        verts,
        faces: vec![vec![0, 1, 2, 3], vec![4, 5, 1, 0]],
    };
    snapped(Pair { a, b })
}

/// The L-shaped prism over `(0, 0), (40, 0), (40, 15), (15, 15),
/// (15, 40), (0, 40)`, `z ∈ [0, 20]`, and a sheet folded into its reflex
/// edge `x = y = 15`: a quad `δ` outside the face `y = 15` over
/// `x ∈ [15 + δ, 30]`, one `δ` outside the face `x = 15` over
/// `y ∈ [15 + δ, 30]`, both `z ∈ [5, 15]`, sharing the fold edge
/// `x = y = 15 + δ`; both face away from the prism.
pub fn l_corner(offset: f64) -> Pair {
    let d = offset;
    let a = prism(&L_SHAPE, 0.0, 20.0);
    let f = 15.0 + d;
    let verts = vec![
        [f, f, 5.0],
        [f, f, 15.0],
        [30.0, f, 15.0],
        [30.0, f, 5.0],
        [f, 30.0, 5.0],
        [f, 30.0, 15.0],
    ];
    let b = Polygons {
        closed: false,
        verts,
        // Normal +y: up the fold, then out along x; normal +x: out
        // along y, then up.
        faces: vec![vec![0, 1, 2, 3], vec![0, 4, 5, 1]],
    };
    snapped(Pair { a, b })
}

/// A flat L-shaped sheet (the hexagon of [`l_corner`]'s prism, `z = 0`,
/// facing up) and the quad over its corner square `[0, 15]²` at `z = δ`,
/// facing up, its sides `x = 0` and `y = 0` over the L's outer edges.
pub fn l_corner_quad(offset: f64) -> Pair {
    let a = Polygons {
        closed: false,
        verts: L_SHAPE.iter().map(|p| [p[0], p[1], 0.0]).collect(),
        faces: vec![(0..6).collect()],
    };
    let b = Polygons {
        closed: false,
        verts: vec![
            [0.0, 0.0, offset],
            [15.0, 0.0, offset],
            [15.0, 15.0, offset],
            [0.0, 15.0, offset],
        ],
        faces: vec![vec![0, 1, 2, 3]],
    };
    snapped(Pair { a, b })
}

/// The L of [`l_corner`], counter-clockwise.
const L_SHAPE: [[f64; 2]; 6] = [
    [0.0, 0.0],
    [40.0, 0.0],
    [40.0, 15.0],
    [15.0, 15.0],
    [15.0, 40.0],
    [0.0, 40.0],
];

/// The prism over the counter-clockwise polygon `base` from `z0` to `z1`
/// (closed): the bottom, the top, a rectangle per side, all facing out.
fn prism(base: &[[f64; 2]], z0: f64, z1: f64) -> Polygons {
    let n = base.len() as u32;
    let mut verts: Vec<[f64; 3]> = base.iter().map(|p| [p[0], p[1], z0]).collect();
    verts.extend(base.iter().map(|p| [p[0], p[1], z1]));
    let mut faces = vec![(0..n).rev().collect(), (n..2 * n).collect()];
    for i in 0..n {
        let k = (i + 1) % n;
        faces.push(vec![i, k, n + k, n + i]);
    }
    Polygons {
        closed: true,
        verts,
        faces,
    }
}

/// `p` with its bodies' vertices on their common grid (module docs).
fn snapped(p: Pair) -> Pair {
    let m =
        p.a.verts
            .iter()
            .chain(&p.b.verts)
            .flatten()
            .fold(0.0f64, |m, x| if x.abs() > m { x.abs() } else { m });
    let g = grid(m);
    let on = |b: Polygons| Polygons {
        verts: b.verts.iter().map(|v| v.map(|x| snap(x, g))).collect(),
        ..b
    };
    Pair {
        a: on(p.a),
        b: on(p.b),
    }
}

/// The grid of values below `m` in magnitude: `2^(E − 52)` for the least
/// `2^E > m` (one ulp of the top binade), so the sum of two values on it
/// is exact; 1 for `m = 0`.
pub fn grid(m: f64) -> f64 {
    assert!(m.is_finite(), "{m}");
    if m == 0.0 {
        return 1.0;
    }
    let e = ((m.to_bits() >> 52) & 0x7ff) as i64 - 1023 + 1;
    f64::from_bits(((e - 52 + 1023) as u64) << 52)
}

/// `x` rounded to a multiple of `g` (a power of two), ties away from 0.
pub fn snap(x: f64, g: f64) -> f64 {
    (x / g).round() * g
}

impl Polygons {
    /// The same body with its vertices and faces renumbered and each loop
    /// starting at another of its corners, drawn from `rng`.
    pub fn relabelled(&self, rng: &mut Rng) -> Polygons {
        let shuffled = |n: usize, rng: &mut Rng| {
            let mut order: Vec<usize> = (0..n).collect();
            for k in (1..n).rev() {
                let j = (rng.next_u64() % (k as u64 + 1)) as usize;
                order.swap(k, j);
            }
            order
        };
        // Vertex `k` becomes `id[k]`; new face `f` is old face `order[f]`.
        let id = shuffled(self.verts.len(), rng);
        let mut verts = vec![[0.0; 3]; self.verts.len()];
        for (k, p) in self.verts.iter().enumerate() {
            verts[id[k]] = *p;
        }
        let order = shuffled(self.faces.len(), rng);
        let faces = order
            .iter()
            .map(|&f| {
                let old = &self.faces[f];
                let start = (rng.next_u64() % old.len() as u64) as usize;
                (0..old.len())
                    .map(|k| id[old[(start + k) % old.len()] as usize] as u32)
                    .collect()
            })
            .collect();
        Polygons {
            closed: self.closed,
            verts,
            faces,
        }
    }

    /// The faces' undirected edges, each as its two vertex indices (low
    /// first) with the number of faces using it, sorted.
    pub fn edges(&self) -> Vec<([u32; 2], u32)> {
        let mut all: Vec<[u32; 2]> = self
            .faces
            .iter()
            .flat_map(|f| {
                (0..f.len()).map(move |k| {
                    let (a, b) = (f[k], f[(k + 1) % f.len()]);
                    if a < b { [a, b] } else { [b, a] }
                })
            })
            .collect();
        all.sort_unstable();
        let mut out: Vec<([u32; 2], u32)> = Vec::new();
        for e in all {
            match out.last_mut() {
                Some((last, n)) if *last == e => *n += 1,
                _ => out.push((e, 1)),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<(&'static str, Pair)> {
        let inset = Wavy {
            inset: 2,
            diagonals_b: Diagonals::Alternating,
            ..Wavy::DEFAULT
        };
        vec![
            ("wavy", wavy(&Wavy::DEFAULT)),
            ("wavy inset", wavy(&inset)),
            ("box-edge fold", box_edge_fold(OFFSET)),
            ("L-corner", l_corner(OFFSET)),
            ("L-corner quad", l_corner_quad(OFFSET)),
        ]
    }

    /// A face's Newell normal (twice its vector area): along the side the
    /// loop runs counter-clockwise about.
    fn newell(p: &Polygons, f: usize) -> [f64; 3] {
        let l = &p.faces[f];
        let mut n = [0.0; 3];
        for k in 0..l.len() {
            let a = p.verts[l[k] as usize];
            let b = p.verts[l[(k + 1) % l.len()] as usize];
            n[0] += (a[1] - b[1]) * (a[2] + b[2]);
            n[1] += (a[2] - b[2]) * (a[0] + b[0]);
            n[2] += (a[0] - b[0]) * (a[1] + b[1]);
        }
        n
    }

    /// Closed bodies use every edge twice, sheets once or twice, and
    /// never one way twice; every vertex is used.
    #[test]
    fn faces_close_up_as_their_kind_says() {
        for (name, pair) in all() {
            for p in [&pair.a, &pair.b] {
                for (e, n) in p.edges() {
                    assert!(n == 2 || (n == 1 && !p.closed), "{name}: {e:?} used {n}");
                }
                let mut directed: Vec<(u32, u32)> = p
                    .faces
                    .iter()
                    .flat_map(|f| (0..f.len()).map(move |k| (f[k], f[(k + 1) % f.len()])))
                    .collect();
                directed.sort_unstable();
                let before = directed.len();
                directed.dedup();
                assert_eq!(before, directed.len(), "{name}: an edge used one way twice");
                let mut used = vec![false; p.verts.len()];
                p.faces
                    .iter()
                    .flatten()
                    .for_each(|&v| used[v as usize] = true);
                assert!(used.iter().all(|&u| u), "{name}: an unused vertex");
            }
        }
    }

    /// Every pair of coordinates of a pair sums exactly: the vertices lie
    /// on one grid.
    #[test]
    fn vertices_lie_on_the_pair_s_grid() {
        for (name, pair) in all() {
            let xs: Vec<f64> = pair
                .a
                .verts
                .iter()
                .chain(&pair.b.verts)
                .flatten()
                .copied()
                .collect();
            let m = xs
                .iter()
                .fold(0.0f64, |m, x| if x.abs() > m { x.abs() } else { m });
            let g = grid(m);
            for &x in &xs {
                assert_eq!(snap(x, g), x, "{name}: {x}");
            }
            for &x in &xs {
                for &y in &xs {
                    // Two-sum: the rounding error of `x + y` is zero.
                    let s = x + y;
                    let bb = s - x;
                    assert_eq!((x - (s - bb)) + (y - bb), 0.0, "{name}: {x} + {y}");
                }
            }
        }
    }

    /// The faces of the flat fixtures face where their docs say: the
    /// wavy sheets and the quads up, the folds and solids outward.
    #[test]
    fn faces_point_out() {
        let p = wavy(&Wavy::DEFAULT);
        for f in 0..p.b.faces.len() {
            assert!(newell(&p.a, f)[2] > 0.0 && newell(&p.b, f)[2] > 0.0);
        }
        let fold = box_edge_fold(OFFSET);
        assert!(newell(&fold.b, 0)[2] > 0.0 && newell(&fold.b, 1)[1] < 0.0);
        let l = l_corner(OFFSET);
        assert!(newell(&l.b, 0)[1] > 0.0 && newell(&l.b, 1)[0] > 0.0);
        let q = l_corner_quad(OFFSET);
        assert!(newell(&q.a, 0)[2] > 0.0 && newell(&q.b, 0)[2] > 0.0);
        // The solids enclose positive volume: the divergence theorem over
        // their faces' fans.
        for s in [&fold.a, &l.a] {
            let mut vol = 0.0;
            for face in &s.faces {
                let o = s.verts[face[0] as usize];
                for k in 1..face.len() - 1 {
                    let a = s.verts[face[k] as usize];
                    let b = s.verts[face[k + 1] as usize];
                    vol += o[0] * (a[1] * b[2] - a[2] * b[1]) - o[1] * (a[0] * b[2] - a[2] * b[0])
                        + o[2] * (a[0] * b[1] - a[1] * b[0]);
                }
            }
            assert!(vol > 0.0, "{vol}");
        }
    }

    /// `b` lies `δ` above `a` at every grid point the two share, to the
    /// grid's rounding, whatever the triangulations.
    #[test]
    fn wavy_sheets_are_offset_by_delta() {
        let w = Wavy {
            inset: 1,
            ..Wavy::DEFAULT
        };
        let p = wavy(&w);
        let side_a = w.cells + 1;
        let side_b = w.cells - 2 * w.inset + 1;
        for i in 0..side_b {
            for j in 0..side_b {
                let vb = p.b.verts[(i * side_b + j) as usize];
                let va = p.a.verts[((i + w.inset) * side_a + j + w.inset) as usize];
                assert_eq!([va[0], va[1]], [vb[0], vb[1]]);
                assert!((vb[2] - va[2] - w.offset).abs() < 1e-12, "{va:?} {vb:?}");
            }
        }
        // A cell's corners are coplanar to their rounding: both diagonals
        // give one surface.
        let a = &p.a.verts;
        for i in 0..w.cells {
            for j in 0..w.cells {
                let at = |i: u32, j: u32| a[(i * side_a + j) as usize][2];
                let twist = at(i, j) + at(i + 1, j + 1) - at(i + 1, j) - at(i, j + 1);
                assert!(twist.abs() < 1e-12, "{i} {j}: {twist}");
            }
        }
    }

    /// The smooth sheet's gap above the cylinder is least at the middle,
    /// `gap`, and positive everywhere: sampled on the patch, against the
    /// circle's height.
    #[test]
    fn smooth_sheet_clears_the_cylinder() {
        let s = smooth(1e-4);
        let b = |t: f64| [(1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t];
        let mut least = f64::INFINITY;
        for a in 0..=40 {
            for c in 0..=40 {
                let (bu, bv) = (b(a as f64 / 40.0), b(c as f64 / 40.0));
                let mut p = [0.0; 3];
                for n in 0..9 {
                    let w = bu[n / 3] * bv[n % 3];
                    for (k, pk) in p.iter_mut().enumerate() {
                        *pk += w * s.net[n][k];
                    }
                }
                let gap = p[2] - libm::sqrt(s.radius * s.radius - p[0] * p[0]);
                assert!(gap > 0.0, "{p:?}");
                if gap < least {
                    least = gap;
                }
            }
        }
        assert!((least - 1e-4).abs() < 1e-12, "{least}");
        assert!(s.net.iter().all(|p| p[3] == 1.0));
    }

    /// A relabelled body has the same faces, as cycles of points.
    #[test]
    fn relabelling_keeps_the_faces() {
        let mut rng = Rng::new(3);
        for (name, pair) in all() {
            for p in [&pair.a, &pair.b] {
                let r = p.relabelled(&mut rng);
                let cycles = |p: &Polygons| {
                    let mut out: Vec<Vec<[u64; 3]>> = p
                        .faces
                        .iter()
                        .map(|f| {
                            let pts: Vec<[u64; 3]> = f
                                .iter()
                                .map(|&v| p.verts[v as usize].map(f64::to_bits))
                                .collect();
                            let least = (0..pts.len()).min_by_key(|&k| pts[k]).unwrap_or(0);
                            (0..pts.len())
                                .map(|k| pts[(least + k) % pts.len()])
                                .collect()
                        })
                        .collect();
                    out.sort();
                    out
                };
                assert_eq!(cycles(p), cycles(&r), "{name}");
                assert_eq!(r.edges().len(), p.edges().len(), "{name}");
            }
        }
    }
}
