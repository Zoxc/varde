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
        _ => unreachable!("a form with a lattice"),
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
            let f = key.implicit(&x).expect("an implicit").abs();
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
        let l = Lattice::new(key.clone()).unwrap();
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
        let axes = Lattice::new(key.clone()).unwrap().axes().map(|a| a.map(q));
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
        let l = Lattice::new(key.clone()).unwrap();
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
        let l = Lattice::new(key.clone()).unwrap();
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
        let l = Lattice::new(key.clone()).unwrap();
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

/// Every tile of a canonical surface is regular, a sphere's with its
/// pole rows as singular sides; and at level `(0, 1)`, where only the
/// tiles at the poles reach them.
#[test]
fn regular_with_the_poles() {
    for key in keys(7) {
        let l = Lattice::new(key.clone()).unwrap();
        let r = l.half(0).unwrap();
        for (n, t) in l.tiles(&r).unwrap().iter().enumerate() {
            assert!(t.is_regular(), "{key:?} tile {n}");
        }
        if let FormKey::Sphere(_) = key {
            let level = Level { u: 0, v: 1 };
            for (j, sides) in [
                (-2, Side::V0.into()),
                (-1, Sides::NONE),
                (0, Sides::NONE),
                (1, Side::V1.into()),
            ] {
                let t = l.tile(level, 0, j).unwrap();
                assert_eq!(t.singular(), sides, "{key:?} ({j})");
                assert!(t.is_regular(), "{key:?} ({j})");
            }
        }
    }
}

/// A cylinder found certifying a tile at level `(40, 45)` regular yet
/// facing in, before levels were capped: the tile `3·10⁻¹²` mm wide,
/// `10⁶` mm along the axis, far finer than its coordinates' precision.
fn far_cylinder() -> FormKey {
    CylinderKey::new(
        [
            0.010472473587766444,
            -0.07404888804929464,
            0.3038769313671156,
        ],
        [0.8071247384492171, -0.5665968612539347, -0.1658844579834785],
        2.1702567127715007,
    )
    .unwrap()
    .into()
}

/// A random level up to `cap` in each direction.
fn level_to(cap: Level, rng: &mut Rng) -> Level {
    Level {
        u: (rng.next_u64() % (u64::from(cap.u) + 1)) as u32,
        v: (rng.next_u64() % (u64::from(cap.v) + 1)) as u32,
    }
}

/// A random tile `(i, j)` of `level` in the cells holding the
/// coordinate box: any angle cell, a plane's cell `(0, 0)`, a cylinder's
/// axial cell 0, a sphere's latitudes.
fn in_box(key: &FormKey, level: Level, rng: &mut Rng) -> (i64, i64) {
    let (hu, hv) = (1i64 << level.u, 1i64 << level.v);
    let mut pick = |n: i64| (rng.next_u64() % n as u64) as i64;
    match key {
        FormKey::Plane(_) => (pick(hu), pick(hv)),
        FormKey::Cylinder(_) => (pick(4 * hu) - 2 * hu, pick(hv)),
        _ => (pick(4 * hu) - 2 * hu, pick(2 * hv) - hv),
    }
}

/// A tile certified regular at any admitted level, to the cap, has a
/// cone holding every coefficient of its exact normal numerator and
/// facing the form's outside (a plane's normal): the first level's
/// numerator, in the tile's own frame, never certifies a tile with a
/// wrong normal.
#[test]
fn regular_tiles_face_out_at_every_level() {
    let mut rng = Rng::new(41);
    let mut cases = Vec::new();
    for key in keys(12).into_iter().chain([far_cylinder()]) {
        let cap = Lattice::new(key.clone()).unwrap().cap();
        for n in 0..varde_testing::pick(8, 60) {
            // The cap itself first.
            let level = if n == 0 { cap } else { level_to(cap, &mut rng) };
            let (i, j) = in_box(&key, level, &mut rng);
            cases.push((key.clone(), level, i, j));
        }
    }
    let dot = |a: &[Q; 3], b: &[Q; 3]| &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2];
    for (key, level, i, j) in cases {
        let t = Lattice::new(key.clone())
            .unwrap()
            .tile(level, i, j)
            .unwrap();
        let Some(c) = t.normal_cone() else { continue };
        let a = c.axis.map(q);
        let (aa, cos2) = (dot(&a, &a), q(c.cos) * q(c.cos));
        let e = exact(&t);
        // The numerator the fold check reads: divided by the pole rows'
        // factors, else zero on them.
        let [x, y, z] = t
            .fold_numerator_at(&Q::from(0u32))
            .unwrap()
            .map(|f| f.coefs().to_vec());
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
            _ => unreachable!("a form with a lattice"),
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
        let l = Lattice::new(key.clone()).unwrap();
        // The cap of the cells visited: angle cells to 3, axial and plane
        // cells to 1.
        let cells = Rect {
            level: Level::BASE,
            u: -1..4,
            v: -1..2,
        };
        let cap = match key {
            FormKey::Sphere(_) => l.cap(),
            FormKey::Cylinder(_) => l.cap_of(&Rect { u: 0..1, ..cells }).unwrap(),
            _ => l.cap_of(&cells).unwrap(),
        };
        for level in [
            cap,
            Level {
                u: cap.u.min(31),
                v: cap.v.min(7),
            },
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
        let l = Lattice::new(key.clone()).unwrap();
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
                let rounded = Tile::rounded(&v_first).unwrap();
                assert_eq!(t, rounded.with_singular(t.singular()), "{key:?} {level:?}");
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
        let l = Lattice::new(key.clone()).unwrap();
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
        let l = Lattice::new(key.clone()).unwrap();
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
    let l = Lattice::new(key.clone()).unwrap();
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

/// [`far_cylinder`]: its cap is fixed by its geometry, a constructor asking
/// for `(40, 45)` gets the cap (a rectangle at it covering the one
/// asked for), an explicit request there is refused with the key, the
/// level and the cap, and the cap's tiles are regular.
#[test]
fn levels_beyond_the_cap_are_refused_or_coarsened() {
    let key = far_cylinder();
    let l = Lattice::new(key.clone()).unwrap();
    let cap = l.cap();
    assert_eq!(cap, Level { u: 24, v: 44 });
    let level = Level { u: 40, v: 45 };
    let (i, j) = (-4203934108937, 10286876419117);
    let beyond = LatticeError::LevelBeyondCap {
        form: Box::new(key.clone()),
        level,
        cap,
    };
    assert_eq!(l.tile(level, i, j).unwrap_err(), beyond);
    let rect = Rect {
        level,
        u: i..i + 3,
        v: j..j + 2,
    };
    assert_eq!(l.tiles(&rect).unwrap_err(), beyond);
    let c = l.capped(&rect).unwrap();
    assert_eq!(c.level, cap);
    // The coarse rectangle covers the fine one: its ends, scaled up, lie
    // outside the fine ends.
    let (du, dv) = (level.u - cap.u, level.v - cap.v);
    assert!(c.u.start << du <= rect.u.start && rect.u.end <= c.u.end << du);
    assert!(c.v.start << dv <= rect.v.start && rect.v.end <= c.v.end << dv);
    assert_eq!(c.u.end - c.u.start, 1);
    for t in l.tiles(&c).unwrap() {
        assert!(t.is_regular());
    }
    // One level past the cap in either direction is refused; within it
    // a rectangle is its own coarsening.
    for past in [
        Level {
            u: cap.u + 1,
            v: cap.v,
        },
        Level {
            u: cap.u,
            v: cap.v + 1,
        },
    ] {
        assert!(matches!(
            l.tile(past, 0, 0),
            Err(LatticeError::LevelBeyondCap { level, .. }) if level == past
        ));
    }
    let within = Rect {
        level: Level { u: 3, v: 9 },
        u: -2..5,
        v: 7..9,
    };
    assert_eq!(l.capped(&within).unwrap(), within);
    // Above the lattice's fixed maximum a level is refused as such.
    let deep = Rect {
        level: Level {
            u: MAX_LEVEL + 1,
            v: 0,
        },
        ..within
    };
    assert_eq!(l.capped(&deep).unwrap_err(), LatticeError::Level);
}

/// The cap follows the geometry: a smaller radius or a form further out
/// caps the turns sooner, a cell further out has a lower cap than the
/// box's, and the cap is the deepest level whose chord is `K_CAP` times
/// the deviation bound.
#[test]
fn caps_follow_the_geometry() {
    let cyl = |p: [f64; 3], r: f64| {
        Lattice::new(CylinderKey::new(p, [1.0, 2.0, 2.0], r).unwrap().into()).unwrap()
    };
    let near = cyl([0.0; 3], 1.0).cap();
    // The axial cell's far rows set the precision: `2²¹·⅔` mm out along
    // the axis's largest component, ulp `2⁻³²`.
    assert_eq!(near, Level { u: 23, v: 44 });
    let small = cyl([0.0; 3], 1e-3).cap();
    assert!(small.u + 9 <= near.u && small.v == near.v, "{small:?}");
    let big = cyl([0.0; 3], 1e3).cap();
    assert!(big.u >= near.u + 9, "{big:?}");
    let s = |c: [f64; 3], r: f64| {
        Lattice::new(SphereKey::new(c, r, [0.0, 0.0, 1.0]).unwrap().into())
            .unwrap()
            .cap()
    };
    let unit = s([0.0; 3], 1.0);
    let out = s([9e5, 0.0, 0.0], 1.0);
    assert!(
        unit.u == unit.v && out.u == out.v && out.u + 10 <= unit.u,
        "{unit:?} {out:?}"
    );
    // A radius whose coordinates leave the range: no level past the base
    // (its tiles are refused).
    assert_eq!(s([0.0; 3], f64::MAX), Level::BASE);
    // A plane's cell further out, a cylinder's axial cell further out.
    let p = Lattice::new(PlaneKey::new([1.0, 2.0, 2.0], [0.0; 3]).unwrap().into()).unwrap();
    let far = |u: std::ops::Range<i64>, v: std::ops::Range<i64>| Rect {
        level: Level::BASE,
        u,
        v,
    };
    assert_eq!(p.cap_of(&far(0..1, 0..1)).unwrap(), p.cap());
    let pc = p.cap_of(&far(0..1, 7..8)).unwrap();
    assert!(pc.u < p.cap().u && pc.v < p.cap().v, "{pc:?}");
    let c = cyl([0.0; 3], 1.0);
    assert_eq!(c.cap_of(&far(-9..-6, 0..1)).unwrap(), c.cap());
    let cc = c.cap_of(&far(0..1, -9..-8)).unwrap();
    assert!(cc.u < near.u && cc.v < near.v, "{cc:?}");
    assert!(c.tile(cc, 0, -9 << cc.v).is_ok());
    assert!(matches!(
        c.tile(near, 0, -9 << near.v),
        Err(LatticeError::LevelBeyondCap { cap, .. }) if cap == cc
    ));
    // The bound itself: the chord at the cap holds `K_CAP·δ`, the next
    // level's doesn't (a linear direction: the cell's extent).
    let delta = |m: f64, rho: f64, t: f64| 3.0 * ulp(m).unwrap() + pow2(-48) * rho + pow2(-50) * t;
    let d = delta(pow2(20), 1.0, CELL / 2.0);
    let extent = |level: u32| CELL / pow2(level as i32);
    assert!(extent(near.v) >= K_CAP * d && extent(near.v + 1) < K_CAP * d);
}

/// The admitted levels follow the form's tangent: at every level to the
/// cap, every coefficient of a tile's exact normal numerator lies within
/// `2⁻⁵` rad of the form's normal at the tile's centre, beyond the turn
/// the tile itself spans; and the tile is regular. A sphere's tiles to
/// 60° latitude: longitude chords shrink with the latitude's cosine,
/// the cap reads the sphere's radius.
#[test]
fn tiles_follow_the_tangent_to_the_cap() {
    let mut rng = Rng::new(43);
    let dot = |a: &[Q; 3], b: &[Q; 3]| &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2];
    let mut worst: f64 = 0.0;
    for key in keys(14).into_iter().chain([far_cylinder()]) {
        let l = Lattice::new(key.clone()).unwrap();
        let cap = l.cap();
        for n in 0..varde_testing::pick(6, 40) {
            let level = match n {
                0 => cap,
                1 => Level {
                    u: cap.u,
                    v: cap.v.min(2),
                },
                _ => level_to(cap, &mut rng),
            };
            let (i, j) = in_box(&key, level, &mut rng);
            let h = 1i64 << level.v;
            // A sphere's longitude cells shrink towards the poles, where
            // the cap (at the sphere's radius) leaves less margin: rows
            // to 60° latitude, whose circles have half the radius or more.
            let pole = matches!(key, FormKey::Sphere(_))
                && (3 * j.abs().max((j + 1).abs())) as f64 > 2.0 * h as f64;
            let t = l.tile(level, i, j).unwrap();
            // The turn the tile spans, in rad, in each angle direction.
            let turn = |lv: u32| std::f64::consts::FRAC_PI_2 / pow2(lv as i32);
            let span = match key {
                FormKey::Plane(_) => 0.0,
                FormKey::Cylinder(_) => turn(level.u),
                _ => turn(level.u) + turn(level.v),
            };
            if pole || span > 0.1 {
                continue;
            }
            assert!(t.is_regular(), "{key:?} {level:?} ({i}, {j})");
            let e = exact(&t);
            let half = Q::from_signeds(1, 2);
            let p = e.eval(&half, &half);
            let out: [Q; 3] = match &key {
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
                _ => unreachable!("a form with a lattice"),
            };
            let [x, y, z] = e.normal_numerator().unwrap().map(|f| f.coefs().to_vec());
            for m in 0..x.len() {
                let c = [x[m].clone(), y[m].clone(), z[m].clone()];
                let d = dot(&c, &out);
                assert!(d > 0u32, "{key:?} {level:?} ({i}, {j}) faces in");
                let cos2 = nearest(&(&d * &d / (dot(&c, &c) * dot(&out, &out))));
                let cos = if cos2 < 1.0 { cos2.sqrt() } else { 1.0 };
                let angle = varde_decide::num::trig::acos(cos) - 2.0 * span;
                worst = if angle > worst { angle } else { worst };
                assert!(angle < pow2(-5), "{key:?} {level:?} ({i}, {j}): {angle:e}");
            }
        }
    }
    eprintln!("worst angle beyond the tile's turn: {worst:e} rad");
}

/// Every form's frame is its key's: the revolution frame about its axis
/// at its foot point, apex or centre (the world origin for a conic-chain
/// cylinder, whose profile's `x` runs along `e₁`); an affine image's,
/// its source's mapped; their lattices the same.
#[test]
fn frames_of_every_form() {
    use crate::geom::form::{AffineKey, ConeKey, ConicCylinderKey, RevolvedKey, TorusKey};
    let z = [0.0, 0.0, 1.0];
    // e_x × z = −y, z × −y = x (the sphere's in `half_surfaces`).
    let about_z = [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], z];
    let quarter = [
        [2.0, 0.0, 0.0, 1.0],
        [2.0, 2.0, 0.0, 0.5],
        [0.0, 2.0, 0.0, 1.0],
    ];
    let keys: [(FormKey, [f64; 3]); 4] = [
        (
            ConeKey::new([1.0, 2.0, 3.0], z, 1.0, 1.0).unwrap().into(),
            [1.0, 2.0, 3.0],
        ),
        (
            TorusKey::new([1.0, 2.0, 3.0], [0.0, 0.0, -2.0], 3.0, 1.0)
                .unwrap()
                .into(),
            [1.0, 2.0, 3.0],
        ),
        (
            RevolvedKey::new(
                [1.0, 2.0, 3.0],
                z,
                &[[[1.0, 0.0, 1.0], [2.0, 1.0, 1.0], [1.0, 2.0, 1.0]]],
            )
            .unwrap()
            .into(),
            [1.0, 2.0, 0.0],
        ),
        (
            ConicCylinderKey::new(z, &[quarter]).unwrap().into(),
            [0.0; 3],
        ),
    ];
    for (key, origin) in keys {
        let f = frame(&key).unwrap();
        assert_eq!(
            f,
            Frame {
                origin,
                axes: about_z
            },
            "{key:?}"
        );
        assert_eq!(Lattice::new(key).unwrap().axes(), about_z);
    }
    // A tilted conic-chain cylinder: its profile's `x` is the span
    // points' offset along `e₁`, to rounding.
    let a = [1.0, 2.0, 2.0];
    let c = ConicCylinderKey::new(a, &[quarter]).unwrap();
    let f = frame(&c.clone().into()).unwrap();
    let [e1, e2, _] = f.axes;
    // The chain may run reversed: each end against either.
    let ends: Vec<[f64; 2]> = c.profile().spans()[0]
        .iter()
        .map(|s| [s[0], s[1]])
        .collect();
    for p in [quarter[0], quarter[2]] {
        let at = |e: [f64; 3]| (0..3).map(|k| p[k] * e[k]).sum::<f64>();
        let want = [at(e1), at(e2)];
        assert!(
            ends.iter()
                .any(|e| (e[0] - want[0]).abs() < 1e-15 && (e[1] - want[1]).abs() < 1e-15),
            "{want:?} in {ends:?}"
        );
    }
    // An affine image: the source plane's frame under `2x + (1, 0, 0)`.
    let plane: FormKey = PlaneKey::new(z, [0.0, 0.0, 3.0]).unwrap().into();
    let src = frame(&plane).unwrap();
    let m = [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]];
    let img: FormKey = AffineKey::new(plane, m, [1.0, 0.0, 0.0]).unwrap().into();
    let f = frame(&img).unwrap();
    assert_eq!(f.origin, [1.0, 0.0, 6.0]);
    assert_eq!(f.axes, src.axes.map(|e| e.map(|x| 2.0 * x)));
    let l = Lattice::new(img).unwrap();
    assert_eq!((l.origin(), l.axes()), (f.origin, f.axes));
}
