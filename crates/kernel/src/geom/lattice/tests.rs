//! Lattices against exact values: every tile, at every level, on its
//! form to the rounding of its coordinates; neighbours sharing their rows
//! to the bit; one route to a level's tile; frames from the key alone.

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

/// The largest coordinate magnitude of a tile's points, `|P/w|`.
fn scale(t: &Tile) -> f64 {
    t.net()
        .iter()
        .flat_map(|p| p[..3].iter().map(move |x| (x / p[3]).abs()))
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
            // The same bits as each tile alone (one route): every
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
    assert_eq!(n[4], [1.0, -1.0, 1.0, 0.5]);
    for r in [2, 5, 8] {
        assert_eq!(n[r][..3], [0.0, 0.0, 2.0 * n[r][3]]);
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
    // levels: shared rows, one route, period 4 cells.
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
