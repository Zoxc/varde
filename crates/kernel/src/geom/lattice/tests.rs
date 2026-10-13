//! Lattices against exact values: every tile, at every level, on its
//! form to the rounding of its coordinates; neighbours sharing their rows
//! to the bit; a level's tile its base tile's exact sub-patch rounded
//! once, its corners the base tile's exact points rounded and a sphere's
//! pole rows one point; frames from the key alone.

use malachite::Rational as Q;
use malachite::base::num::arithmetic::traits::Abs;
use varde_decide::bezier::HomPatch;
use varde_testkit::rng::Rng;

use super::*;
use crate::geom::form::{CylinderKey, PlaneKey, SphereKey};

const TOL: f64 = 1e-15;

fn exact(t: &Tile) -> HomPatch<Q> {
    t.patch_at(&Q::from(0u32))
}

/// Random keys of each form: places up to `1e6` out, radii from `1e-3`
/// to `1e5`.
fn keys(seed: u64) -> Vec<FormKey> {
    let mut rng = Rng::new(seed);
    let mut out = vec![
        PlaneKey::new([0.0, 0.0, 1.0], [0.0; 3]).unwrap().into(),
        CylinderKey::new([0.0; 3], [0.0, 0.0, 1.0], 1.0)
            .unwrap()
            .into(),
        SphereKey::new([0.0; 3], 1.0, [0.0, 0.0, 1.0])
            .unwrap()
            .into(),
    ];
    for k in 0..6 {
        let extent = [1.0, 1e3, 1e6][k % 3];
        let p = rng.point(extent).to_array();
        let d = rng.direction().to_array();
        let r = rng.log_range(1e-3, 1e5);
        out.push(PlaneKey::new(d, p).unwrap().into());
        out.push(CylinderKey::new(p, d, r).unwrap().into());
        out.push(SphereKey::new(p, r, d).unwrap().into());
    }
    out
}

/// The largest coordinate magnitude of a tile's points.
fn scale(t: &Tile) -> f64 {
    t.net()
        .iter()
        .flat_map(|p| p[..3].iter().map(|x| x.abs()))
        .fold(0.0, |m, x| if x > m { x } else { m })
}

/// The radius the implicit's distance divides by (1 for a plane).
fn radius(key: &FormKey) -> f64 {
    match key {
        FormKey::Plane(_) => 1.0,
        FormKey::Cylinder(c) => c.radius(),
        FormKey::Sphere(s) => s.radius(),
    }
}

/// Whether every sample of `t` is within `TOL·m` of the form, exactly:
/// `|F| ≤ TOL·m·ρ·|a|²` bounds the distance on a cylinder
/// (`F = |a|²·(D − ρ)(D + ρ)`, `D + ρ ≥ ρ`), likewise on a sphere, and
/// on a plane `|F|` is it (unit normal to rounding). Returns the worst
/// distance over `m`.
fn on_form(key: &FormKey, t: &Tile, m: f64, rng: &mut Rng) -> f64 {
    let p = exact(t);
    let aa = match key {
        FormKey::Cylinder(c) => {
            let a = c.axis().map(q);
            dot(&a, &a)
        }
        _ => Q::from(1u32),
    };
    let rho = q(radius(key));
    let mut ts: Vec<Q> = [0.0, 0.25, 0.5, 0.75, 1.0].map(q).to_vec();
    ts.push(q((rng.next_u64() % 1024) as f64 / 1024.0));
    let mut worst: f64 = 0.0;
    for u in &ts {
        for v in &ts {
            let x = p.eval(u, v);
            let f = key.implicit(&x).abs();
            let d = nearest(&(f / (&rho * &aa))) / m;
            worst = if d > worst { d } else { worst };
        }
    }
    worst
}

/// The levels the tests visit.
const LEVELS: [Level; 6] = [
    Level::BASE,
    Level { u: 1, v: 0 },
    Level { u: 0, v: 1 },
    Level { u: 2, v: 3 },
    Level { u: 3, v: 2 },
    Level { u: 4, v: 4 },
];

#[test]
fn cell_covers_the_coordinate_box() {
    assert_eq!(CELL, (1u64 << 22) as f64);
    // A plane's base cell (0, 0) holds every point of the box on it:
    // their offsets from the origin along u and v lie within CELL/2.
    for key in keys(1) {
        let FormKey::Plane(_) = key else { continue };
        let l = Lattice::new(key).unwrap();
        let [u, v, _] = l.axes();
        let o = l.origin();
        for c in 0..8 {
            let x = [0, 1, 2].map(|k| {
                if c >> k & 1 == 1 {
                    MAX_COORD
                } else {
                    -MAX_COORD
                }
            });
            for a in [u, v] {
                let s: f64 = (0..3).map(|k| (x[k] - o[k]) * a[k]).sum();
                assert!(s.abs() < CELL / 2.0, "{s}");
            }
        }
    }
}

#[test]
fn frames_follow_the_key() {
    let l = Lattice::new(
        PlaneKey::new([0.0, 0.0, 2.0], [1.0, 2.0, 3.0])
            .unwrap()
            .into(),
    )
    .unwrap();
    assert_eq!(l.origin(), [0.0, 0.0, 3.0]);
    assert_eq!(
        l.axes(),
        [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]
    );
    let l = Lattice::new(
        CylinderKey::new([1.0, 2.0, 3.0], [1.0, 0.0, 0.0], 1.0)
            .unwrap()
            .into(),
    )
    .unwrap();
    assert_eq!(l.origin(), [0.0, 2.0, 3.0]);
    // e_y (least |a·e_k|, the lower of the tie) × x = −z; e₂ = x × −z = y.
    assert_eq!(
        l.axes(),
        [[0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]]
    );
    // Orthonormal to rounding.
    for key in keys(2) {
        let axes = Lattice::new(key).unwrap().axes().map(|a| a.map(q));
        for i in 0..3 {
            for j in 0..3 {
                let want = Q::from(u32::from(i == j));
                let d = nearest(&(dot(&axes[i], &axes[j]) - want)).abs();
                assert!(d < 4.0 * f64::EPSILON, "{i} {j} {d}");
            }
        }
    }
}

#[test]
fn tiles_lie_on_their_forms() {
    let mut rng = Rng::new(4);
    let mut worst: f64 = 0.0;
    for key in keys(3) {
        let l = Lattice::new(key).unwrap();
        // The 180° surface, a cell further along each lattice direction.
        let mut cells = vec![(-3, 0)];
        let base = l.half(1).unwrap();
        for k in base.u.clone() {
            for j in base.v.clone() {
                cells.push((k, j));
            }
        }
        if let FormKey::Sphere(_) = key {
            cells[0] = (-3, -1);
        } else {
            cells.push((2, -2));
        }
        for (k, j) in cells {
            let m = scale(&l.base_tile(k, j).unwrap());
            for level in LEVELS {
                // A few tiles of the cell at this level, corners included.
                let (nu, nv) = (1i64 << level.u, 1i64 << level.v);
                for (a, b) in [(0, 0), (nu - 1, nv - 1), (nu / 2, nv / 3)] {
                    let t = l
                        .tile(level, (k << level.u) + a, (j << level.v) + b)
                        .unwrap();
                    let w = on_form(&key, &t, m, &mut rng);
                    assert!(w <= TOL, "{key:?} cell ({k}, {j}) {level:?}: {w:e}");
                    worst = if w > worst { w } else { worst };
                }
            }
        }
    }
    eprintln!("worst distance over the base tile's scale: {worst:e}");
}

/// Every tile of `rect` meets its neighbours in `u` and `v` on a row
/// that is the same bits in both.
fn rows_shared(tiles: &[Tile], rect: &Rect) {
    let nv = (rect.v.end - rect.v.start) as usize;
    let nu = (rect.u.end - rect.u.start) as usize;
    let bits = |t: &Tile, n: usize| t.net()[n].map(f64::to_bits);
    for a in 0..nu {
        for b in 0..nv {
            let t = &tiles[a * nv + b];
            if a + 1 < nu {
                let s = &tiles[(a + 1) * nv + b];
                for r in 0..3 {
                    assert_eq!(bits(t, 6 + r), bits(s, r), "u row at ({a}, {b})");
                }
            }
            if b + 1 < nv {
                let s = &tiles[a * nv + b + 1];
                for r in 0..3 {
                    assert_eq!(bits(t, 3 * r + 2), bits(s, 3 * r), "v row at ({a}, {b})");
                }
            }
        }
    }
}

#[test]
fn neighbours_share_rows_to_the_bit() {
    for key in keys(5) {
        let l = Lattice::new(key).unwrap();
        for level in LEVELS {
            // A block straddling base-cell lines in both directions (less
            // than a turn in u).
            let (hu, hv) = (1i64 << level.u, 1i64 << level.v);
            let v = match key {
                FormKey::Sphere(_) => -hv..hv,
                _ => -hv - 1..hv + 1,
            };
            let rect = Rect {
                level,
                u: -1..hu + 1,
                v,
            };
            let tiles = l.tiles(&rect).unwrap();
            rows_shared(&tiles, &rect);
            // The same bits as each tile alone (`tiles` and `tile` agree): every
            // seventh tile, the last included.
            let nv = (rect.v.end - rect.v.start) as usize;
            let last = tiles.len() - 1;
            for (n, t) in tiles.iter().enumerate() {
                if n % 7 != 0 && n != last {
                    continue;
                }
                let (i, j) = (
                    rect.u.start + (n / nv) as i64,
                    rect.v.start + (n % nv) as i64,
                );
                assert_eq!(*t, l.tile(level, i, j).unwrap(), "({i}, {j}) {level:?}");
            }
        }
    }
}

#[test]
fn angle_cells_repeat_every_full_turn() {
    for key in keys(6) {
        if let FormKey::Plane(_) = key {
            continue;
        }
        let l = Lattice::new(key).unwrap();
        for level in [Level::BASE, Level { u: 2, v: 1 }] {
            for i in [-5, 0, 3, 7] {
                let a = l.tile(level, i, 0).unwrap();
                let b = l.tile(level, i + (4 << level.u), 0).unwrap();
                assert_eq!(a, b);
            }
        }
    }
}

#[test]
fn equal_keys_give_equal_lattices() {
    // One cylinder from two points of its axis and two directions.
    let a = Lattice::new(
        CylinderKey::new([1.0, -2.0, 0.25], [1.0, 2.0, 2.0], 3.0)
            .unwrap()
            .into(),
    )
    .unwrap();
    let b = Lattice::new(
        CylinderKey::new([6.0, 8.0, 10.25], [-0.5, -1.0, -1.0], 3.0)
            .unwrap()
            .into(),
    )
    .unwrap();
    let r = a.half(-1).unwrap();
    assert_eq!(a.tiles(&r).unwrap(), b.tiles(&r).unwrap());
}

#[test]
fn half_surfaces() {
    let l = Lattice::new(
        SphereKey::new([0.0; 3], 2.0, [0.0, 0.0, 1.0])
            .unwrap()
            .into(),
    )
    .unwrap();
    let r = l.half(0).unwrap();
    assert_eq!((r.u.clone(), r.v.clone()), (0..2, -1..1));
    let t = l.tiles(&r).unwrap();
    assert_eq!(t.len(), 4);
    // The frame: e₁ the reference direction of z, e_x × z = −y; e₂ = x.
    assert_eq!(l.axes()[..2], [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0]]);
    // Tile (0, 0): the octant from e₁ to e₂ north of the equator, its
    // last v row the pole, its middle weight ½.
    let n = t[1].net();
    assert_eq!(n[0], [0.0, -2.0, 0.0, 1.0]);
    assert_eq!(n[2], [0.0, 0.0, 2.0, 1.0]);
    assert_eq!(n[4], [2.0, -2.0, 2.0, 0.5]);
    for r in [2, 5, 8] {
        assert_eq!(n[r][..3], [0.0, 0.0, 2.0]);
    }
}

#[test]
fn regular_where_no_pole() {
    for key in keys(7) {
        let l = Lattice::new(key).unwrap();
        let r = l.half(0).unwrap();
        for (n, t) in l.tiles(&r).unwrap().iter().enumerate() {
            match key {
                // The pole rows make a sphere's base tiles singular.
                FormKey::Sphere(_) => assert!(!t.is_regular()),
                _ => assert!(t.is_regular(), "{key:?} tile {n}"),
            }
        }
        if let FormKey::Sphere(_) = key {
            // Away from the poles, at level (0, 1).
            let level = Level { u: 0, v: 1 };
            for j in [-2 + 1, 0] {
                let t = l.tile(level, 0, j).unwrap();
                assert!(t.is_regular(), "{key:?} ({j})");
            }
        }
    }
}

/// A tile certified regular at any level, to the finest, has a cone
/// holding every coefficient of its exact normal numerator and facing
/// the form's outside (a plane's normal): the first level's numerator,
/// in the tile's own frame, never certifies a tile narrower than its
/// coordinates' precision with a wrong normal (the first case: a
/// cylinder tile `3·10⁻¹²` mm wide, `10⁶` mm along the axis).
#[test]
fn regular_tiles_face_out_at_every_level() {
    let reproducer = (
        FormKey::from(
            CylinderKey::new(
                [
                    0.010472473587766444,
                    -0.07404888804929464,
                    0.3038769313671156,
                ],
                [0.8071247384492171, -0.5665968612539347, -0.1658844579834785],
                2.1702567127715007,
            )
            .unwrap(),
        ),
        Level { u: 40, v: 45 },
        -4203934108937,
        10286876419117,
    );
    let mut rng = Rng::new(41);
    let mut cases = vec![reproducer];
    for key in keys(12) {
        for _ in 0..varde_testing::pick(8, 60) {
            let level = Level {
                u: (rng.next_u64() % (u64::from(MAX_LEVEL) + 1)) as u32,
                v: (rng.next_u64() % (u64::from(MAX_LEVEL) + 1)) as u32,
            };
            let i = (rng.next_u64() % (4 << level.u)) as i64 - (2 << level.u);
            let j = (rng.next_u64() % (2 << level.v)) as i64 - (1 << level.v);
            cases.push((key, level, i, j));
        }
    }
    let dot = |a: &[Q; 3], b: &[Q; 3]| &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2];
    for (key, level, i, j) in cases {
        let t = Lattice::new(key).unwrap().tile(level, i, j).unwrap();
        let Some(c) = t.normal_cone() else { continue };
        let a = c.axis.map(q);
        let (aa, cos2) = (dot(&a, &a), q(c.cos) * q(c.cos));
        let e = exact(&t);
        let [x, y, z] = e.normal_numerator().unwrap().map(|f| f.coefs().to_vec());
        for m in 0..x.len() {
            let n = [x[m].clone(), y[m].clone(), z[m].clone()];
            let d = dot(&a, &n);
            assert!(d > 0u32, "{key:?} {level:?} ({i}, {j}): coefficient {m}");
            assert!(
                &d * &d >= &cos2 * &aa * dot(&n, &n),
                "{key:?} {level:?} ({i}, {j})"
            );
        }
        let half = Q::from_signeds(1, 2);
        let p = e.eval(&half, &half);
        let out: [Q; 3] = match key {
            FormKey::Plane(k) => k.normal().map(q),
            FormKey::Cylinder(k) => {
                let (f, ax) = (k.foot().map(q), k.axis().map(q));
                let r: [Q; 3] = std::array::from_fn(|k| &p[k] - &f[k]);
                let s = dot(&r, &ax) / dot(&ax, &ax);
                std::array::from_fn(|k| &r[k] - &s * &ax[k])
            }
            FormKey::Sphere(k) => {
                let o = k.centre().map(q);
                std::array::from_fn(|k| &p[k] - &o[k])
            }
        };
        assert!(
            dot(&a, &out) > 0u32,
            "{key:?} {level:?} ({i}, {j}) faces in"
        );
    }
}

#[test]
fn bad_cells_and_rects_are_refused() {
    let s = Lattice::new(
        SphereKey::new([0.0; 3], 1.0, [0.0, 0.0, 1.0])
            .unwrap()
            .into(),
    )
    .unwrap();
    assert_eq!(s.base_tile(0, 1).unwrap_err(), LatticeError::Index);
    assert_eq!(s.base_tile(0, -2).unwrap_err(), LatticeError::Index);
    let full = Rect {
        level: Level { u: 1, v: 0 },
        u: 0..8,
        v: 0..1,
    };
    assert_eq!(s.tiles(&full).unwrap_err(), LatticeError::Span);
    let empty = Rect {
        level: Level::BASE,
        u: 0..0,
        v: 0..1,
    };
    assert_eq!(s.tiles(&empty).unwrap_err(), LatticeError::Span);
    let deep = Level {
        u: MAX_LEVEL + 1,
        v: 0,
    };
    assert_eq!(s.tile(deep, 0, 0).unwrap_err(), LatticeError::Level);
    let p = Lattice::new(PlaneKey::new([0.0, 0.0, 1.0], [0.0; 3]).unwrap().into()).unwrap();
    assert_eq!(p.base_tile(i64::MAX, 0).unwrap_err(), LatticeError::Index);
    // A plane's lattice has no turn: wide rectangles are fine.
    let wide = Rect {
        level: Level::BASE,
        u: -3..3,
        v: 0..1,
    };
    assert_eq!(p.tiles(&wide).unwrap().len(), 6);
}

#[test]
fn deepest_levels_share_rows_and_repeat() {
    // Blocks straddling base-cell lines and the angle wrap at the finest
    // levels: shared rows, `tiles` as `tile`, period 4 cells.
    let ks = keys(21);
    for key in ks.iter().take(varde_testing::pick(6, ks.len())) {
        let l = Lattice::new(*key).unwrap();
        for level in [
            Level {
                u: MAX_LEVEL,
                v: MAX_LEVEL,
            },
            Level { u: 31, v: 7 },
        ] {
            let (hu, hv) = (1i64 << level.u, 1i64 << level.v);
            let vs = match key {
                // The south pole, the equator, the north pole.
                FormKey::Sphere(_) => vec![-hv..-hv + 2, -1..1, hv - 2..hv],
                _ => vec![-1..1, hv - 1..hv + 1],
            };
            for v in vs {
                for u in [-1..1, 3 * hu - 1..3 * hu + 1] {
                    let rect = Rect {
                        level,
                        u: u.clone(),
                        v: v.clone(),
                    };
                    let tiles = l.tiles(&rect).unwrap();
                    rows_shared(&tiles, &rect);
                    let nv = (rect.v.end - rect.v.start) as usize;
                    for (n, t) in tiles.iter().enumerate() {
                        let i = rect.u.start + (n / nv) as i64;
                        let j = rect.v.start + (n % nv) as i64;
                        assert_eq!(*t, l.tile(level, i, j).unwrap(), "({i}, {j})");
                        if !matches!(key, FormKey::Plane(_)) {
                            assert_eq!(*t, l.tile(level, i - 4 * hu, j).unwrap());
                        }
                    }
                }
            }
        }
    }
}

/// `i/2^level` within its base cell, exactly: the local parameter of
/// index `i`'s lower end there.
fn local(i: i64, level: u32) -> Q {
    Q::from(i - ((i >> level) << level)) / Q::from(1u64 << level)
}

/// A tile at levels to `(8, 8)` is its base tile's exact sub-patch
/// rounded once: the same bits split `v` first; each corner the base
/// tile's exact point there rounded, so the corner every tile around a
/// lattice node holds, at every level, is that one point.
#[test]
fn tiles_round_their_exact_sub_patch() {
    let mut rng = Rng::new(31);
    for key in keys(8).into_iter().take(varde_testing::pick(9, 21)) {
        let l = Lattice::new(key).unwrap();
        for level in [
            Level { u: 1, v: 1 },
            Level { u: 5, v: 2 },
            Level { u: 8, v: 8 },
            Level { u: 0, v: 7 },
        ] {
            for _ in 0..3 {
                let i = (rng.next_u64() % (3 << level.u)) as i64 - (1 << level.u);
                let j = match key {
                    FormKey::Sphere(_) => (rng.next_u64() % (2 << level.v)) as i64 - (1 << level.v),
                    _ => (rng.next_u64() % (3 << level.v)) as i64 - (1 << level.v),
                };
                let base = l.base_tile(i >> level.u, j >> level.v).unwrap();
                let t = l.tile(level, i, j).unwrap();
                let v_first = descend(
                    descend(base.exact(), Dir::V, level.v, j),
                    Dir::U,
                    level.u,
                    i,
                );
                assert_eq!(t, Tile::rounded(&v_first).unwrap(), "{key:?} {level:?}");
                let e = base.exact();
                for (a, b) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                    let (pi, pj) = (i + a, j + b);
                    let u = local(i, level.u) + Q::from(a) / Q::from(1u64 << level.u);
                    let v = local(j, level.v) + Q::from(b) / Q::from(1u64 << level.v);
                    let want = e.eval(&u, &v).map(|x| nearest(&x));
                    let got = t.point(2 * a as usize, 2 * b as usize);
                    assert_eq!(got.map(f64::to_bits), want.map(f64::to_bits), "{level:?}");
                    // The same node at a coarser level, where it is one.
                    if pi % 2 == 0 && pj % 2 == 0 && level.u > 0 && level.v > 0 {
                        let up = Level {
                            u: level.u - 1,
                            v: level.v - 1,
                        };
                        let (ci, cj) = (pi / 2, pj / 2);
                        if let Ok(c) = l.tile(up, ci, cj) {
                            assert_eq!(c.point(0, 0), got, "{level:?} node ({pi}, {pj})");
                        }
                    }
                }
            }
        }
    }
}

/// Neighbours share rows to the bit at every level to `(8, 8)`, on
/// blocks straddling base-cell lines.
#[test]
fn rows_are_shared_at_levels_to_eight() {
    for key in keys(9).into_iter().take(varde_testing::pick(9, 21)) {
        let l = Lattice::new(key).unwrap();
        for lu in 0..=8 {
            let level = Level { u: lu, v: 8 - lu };
            let (hu, hv) = (1i64 << level.u, 1i64 << level.v);
            let v = match key {
                FormKey::Sphere(_) => -2.min(hv)..2.min(hv),
                _ => -2..2,
            };
            for u in [-1..1, hu - 1..hu + 1] {
                let rect = Rect {
                    level,
                    u,
                    v: v.clone(),
                };
                rows_shared(&l.tiles(&rect).unwrap(), &rect);
            }
        }
    }
}

/// A sphere's pole rows are one point to the bit, the rounded pole, at
/// every level.
#[test]
fn pole_rows_are_one_point() {
    for key in keys(10) {
        let FormKey::Sphere(s) = key else { continue };
        let l = Lattice::new(key).unwrap();
        let (c, a, r) = (s.centre().map(q), s.axis().map(q), q(s.radius()));
        let pole = |sign: i32| -> [f64; 3] {
            std::array::from_fn(|k| nearest(&(&c[k] + Q::from(sign) * &r * &a[k])))
        };
        for level in [Level::BASE, Level { u: 3, v: 2 }, Level { u: 8, v: 8 }] {
            let hv = 1i64 << level.v;
            for i in [0i64, 1, (3 << level.u) - 1] {
                let north = l.tile(level, i, hv - 1).unwrap();
                let south = l.tile(level, i, -hv).unwrap();
                for k in 0..3 {
                    assert_eq!(north.point(k, 2), pole(1), "{level:?} {i}");
                    assert_eq!(south.point(k, 0), pole(-1), "{level:?} {i}");
                }
            }
        }
    }
}

/// An edge along a tile row, its spans the tiles' rows (weights other
/// than 1 on a sphere), is a valid chain that ends exactly at the tiles'
/// corners at every knot, at the first level and exactly.
#[test]
fn row_chains_end_at_tile_corners() {
    use crate::geom::curve::Curve3D;
    let key: FormKey = SphereKey::new([0.3, -1.7, 2.25], 3.5, [1.0, 2.0, 2.0])
        .unwrap()
        .into();
    let l = Lattice::new(key).unwrap();
    for level in [Level { u: 3, v: 2 }, Level { u: 6, v: 5 }] {
        let n = 1i64 << level.u;
        // The rows `v = j` (low side of tile j) and the columns `u = i`.
        for j in [-(1 << level.v) + 1, 1] {
            let tiles: Vec<Tile> = (n - 2..n + 2)
                .map(|i| l.tile(level, i, j).unwrap())
                .collect();
            let spans = tiles
                .iter()
                .map(|t| [0, 1, 2].map(|i| t.net()[3 * i]))
                .collect();
            let knots = (0..=tiles.len()).map(|k| k as f64).collect();
            let c = Curve3D::new(spans, knots).expect("a chain sharing its knot points");
            for (k, t) in tiles.iter().enumerate() {
                let corner = t.point(0, 0);
                assert_eq!(c.eval(k as f64).unwrap().map(|x| x.v), corner);
                assert_eq!(c.eval_at(k as f64, &Q::from(0u32)).unwrap(), corner.map(q));
                assert_eq!(t.eval(0.0, 0.0).map(|x| x.v), corner);
                assert!(t.net()[3][3] != 1.0, "a weight other than 1");
            }
        }
    }
}
