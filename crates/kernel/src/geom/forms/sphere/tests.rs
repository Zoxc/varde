//! The sphere's constructor against exact values and OCCT's net: every
//! control point the exact value rounded once, the canonical surface
//! from pole to pole, its tiles regular with the pole rows as singular
//! sides, at every level to `(8, 8)`; no latitude past the poles.

use malachite::Rational as Q;
use varde_decide::bezier::Side;
use varde_decide::num::trig::{cos, sin};
use varde_testkit::rng::Rng;

use super::super::testing::{frame, rounded_once, rows_shared, scale};
use super::*;
use crate::geom::lattice::{Level, Rect};
use crate::geom::round::nearest;

const TOL: f64 = 1e-15;

/// Spheres with centres up to `1e6` out, radii from `1e-3` to `1e5`.
fn spheres(seed: u64) -> Vec<SphereKey> {
    let mut rng = Rng::new(seed);
    let mut out = vec![SphereKey::new([0.0; 3], 1.0, [0.0, 0.0, 1.0]).unwrap()];
    for k in 0..9 {
        let p = rng.point([1.0, 1e3, 1e6][k % 3]).to_array();
        let r = rng.log_range(1e-3, 1e5);
        out.push(SphereKey::new(p, r, rng.direction().to_array()).unwrap());
    }
    out
}

#[test]
fn tiles_are_exact_values_rounded_once() {
    let root = std::f64::consts::FRAC_1_SQRT_2;
    for key in spheres(1) {
        let l = Lattice::new(key.into()).unwrap();
        let (o, [e1, e2, a]) = frame(&l);
        assert_eq!(l.origin(), key.centre());
        assert_eq!(l.axes()[2], key.axis());
        let r = q(key.radius());
        let neg = |x: &[Q; 3]| x.clone().map(|c| -c);
        let radii = [e1.clone(), e2.clone(), neg(&e1), neg(&e2)];
        for k in -2..3i64 {
            for cell in [-1i64, 0] {
                let r0 = &radii[k.rem_euclid(4) as usize];
                let r1 = &radii[(k + 1).rem_euclid(4) as usize];
                let corner: [Q; 3] = std::array::from_fn(|i| &r0[i] + &r1[i]);
                let dirs = [(r0.clone(), 1.0), (corner, root), (r1.clone(), 1.0)];
                // (ρ, z) on the unit meridian, and the weights.
                let m: [(i64, i64, f64); 3] = if cell == 0 {
                    [(1, 0, 1.0), (1, 1, root), (0, 1, 1.0)]
                } else {
                    [(0, -1, 1.0), (1, -1, root), (1, 0, 1.0)]
                };
                let want: [([Q; 3], f64); 9] = std::array::from_fn(|n| {
                    let (d, wu) = &dirs[n / 3];
                    let (rho, z, wv) = m[n % 3];
                    let (rho, z) = (Q::from(rho), Q::from(z));
                    let p = std::array::from_fn(|i| &o[i] + &r * (&rho * &d[i] + &z * &a[i]));
                    // `√½·√½` is ½ exactly, not the product of the
                    // rounded roots.
                    let w = if *wu != 1.0 && wv != 1.0 {
                        0.5
                    } else {
                        wu * wv
                    };
                    (p, w)
                });
                let what = format!("{key:?} ({k}, {cell})");
                rounded_once(&l.base_tile(k, cell).unwrap(), &want, &what);
            }
        }
    }
}

/// OCCT's `Convert_SphereToBSplineSurface` (its `ComputePoles`, OCCT
/// commit b8f597c677811d1f9f4d8a97f5ae2825c0353a42) over the cell's
/// longitude `[U₁, U₂]` and latitude `[V₁, V₂]`: meridian poles `(x, z)
/// = R·(cos V, sin V)`, the middle one over `cos α`, swept as `(x cos U,
/// x sin U, z)`, the middle ones over `cos α` too, weights the products
/// of `1, cos α, 1`. The tile is that net in the key's frame.
#[test]
fn tiles_are_occt_s_nets() {
    for key in spheres(2) {
        let l = Lattice::new(key.into()).unwrap();
        let (o, [e1, e2, a]) = (l.origin(), l.axes());
        let r = key.radius();
        let alpha = std::f64::consts::FRAC_PI_4;
        for (k, cell) in [(-1i64, 0i64), (0, -1), (2, 0), (1, -1)] {
            let tile = l.base_tile(k, cell).unwrap();
            let m = scale(&tile);
            let u1 = k as f64 * std::f64::consts::FRAC_PI_2;
            let v1 = cell as f64 * std::f64::consts::FRAC_PI_2;
            for i in 0..3 {
                let u = u1 + i as f64 * alpha;
                let fu = if i == 1 { 1.0 / cos(alpha) } else { 1.0 };
                for j in 0..3 {
                    let v = v1 + j as f64 * alpha;
                    let fv = if j == 1 { 1.0 / cos(alpha) } else { 1.0 };
                    let (x, z) = (r * cos(v) * fv, r * sin(v) * fv);
                    let (px, py) = (x * cos(u) * fu, x * sin(u) * fu);
                    let want: [f64; 3] =
                        std::array::from_fn(|c| o[c] + px * e1[c] + py * e2[c] + z * a[c]);
                    let got = tile.net()[3 * i + j];
                    for c in 0..3 {
                        let d = (got[c] - want[c]).abs();
                        assert!(
                            d <= 8.0 * TOL * m,
                            "{key:?} ({k}, {cell}) ({i}, {j}): {d:e}"
                        );
                    }
                    let w = (1.0 / fu) * (1.0 / fv);
                    assert!((got[3] - w).abs() <= f64::EPSILON, "{key:?} weight");
                }
            }
        }
    }
}

/// Every sphere's tiles at levels to `(8, 8)` are regular, those at a
/// pole with its row as a singular side.
#[test]
fn tiles_are_regular_with_the_poles() {
    for key in spheres(5) {
        let l = Lattice::new(key.into()).unwrap();
        for level in [
            Level::BASE,
            Level { u: 1, v: 1 },
            Level { u: 3, v: 5 },
            Level { u: 8, v: 8 },
        ] {
            let h = 1i64 << level.v;
            for j in [-h, -h + 1, -1, 0, h - 2, h - 1] {
                for i in [-1i64, 0, (1 << level.u) - 1, 3 << level.u] {
                    let t = l.tile(level, i, j).unwrap();
                    let want = if j == -h {
                        Side::V0.into()
                    } else if j == h - 1 {
                        Side::V1.into()
                    } else {
                        varde_decide::bezier::Sides::NONE
                    };
                    let what = format!("{key:?} {level:?} ({i}, {j})");
                    assert_eq!(t.singular(), want, "{what}");
                    assert!(t.is_regular(), "{what}");
                }
            }
        }
    }
}

#[test]
fn the_canonical_sphere_is_half_a_turn_pole_to_pole() {
    let key = SphereKey::new([1.0, -2.0, 0.25], 1.5, [1.0, 2.0, 2.0]).unwrap();
    let (c, a, r) = (key.centre().map(q), key.axis().map(q), q(key.radius()));
    let pole = |s: i32| -> [f64; 3] {
        std::array::from_fn(|k| nearest(&(&c[k] + Q::from(s) * &r * &a[k])))
    };
    for first in [-1, 0, 3] {
        let s = sphere(key, first).unwrap();
        assert_eq!(
            (s.rect().u.clone(), s.rect().v.clone()),
            (first..first + 2, -1..1)
        );
        rows_shared(s.tiles(), s.rect());
        // Row-major in `u`: the south tile, then the north one; the pole
        // rows singular sides, and every tile regular.
        for (n, t) in s.tiles().iter().enumerate() {
            let (side, row, singular) = if n % 2 == 0 {
                (-1, 0, Side::V0)
            } else {
                (1, 2, Side::V1)
            };
            for k in 0..3 {
                assert_eq!(t.point(k, row), pole(side), "{first} tile {n}");
            }
            assert_eq!(t.singular(), singular.into(), "{first} tile {n}");
            assert!(t.is_regular(), "{first} tile {n}");
        }
    }
    // No latitude past the poles, at any level.
    let l = Lattice::new(key.into()).unwrap();
    for cell in [-2, 1, i64::MAX, i64::MIN] {
        assert_eq!(l.base_tile(0, cell).unwrap_err(), LatticeError::Index);
    }
    let level = Level { u: 1, v: 2 };
    assert_eq!(l.tile(level, 0, 4).unwrap_err(), LatticeError::Index);
    assert_eq!(l.tile(level, 0, -5).unwrap_err(), LatticeError::Index);
    let rect = Rect {
        level,
        u: 0..2,
        v: -4..5,
    };
    assert_eq!(l.tiles(&rect).unwrap_err(), LatticeError::Index);
    let all = Rect {
        level,
        u: 0..2,
        v: -4..4,
    };
    assert_eq!(l.tiles(&all).unwrap().len(), 16);
}

/// Beside a pole the longitude cap is each latitude tile's own, read at
/// its far row's radius: a sphere capped at 38 (at the equator) coarsens
/// a request at `(16, 30)` next to a pole around the axis, refuses it
/// asked for explicitly, and its tiles there are regular; a level
/// further from the pole gets a deeper longitude cap, and next to the
/// equator the sphere's own.
#[test]
fn longitude_caps_shrink_towards_the_poles() {
    let key = SphereKey::new([100.0, 0.0, 0.0], 1.0, [0.0, 0.0, 1.0]).unwrap();
    let l = Lattice::new(key.into()).unwrap();
    let cap = l.cap();
    assert_eq!(cap, Level { u: 38, v: 38 });
    let level = Level { u: 16, v: 30 };
    let h = 1i64 << level.v;
    let one = |i: i64, j: i64| Rect {
        level,
        u: i..i + 1,
        v: j..j + 1,
    };
    for (i, j) in [(5, h - 1), (-7, -h)] {
        let near = l.cap_of(&one(i, j)).unwrap();
        // The far row `2⁻³⁰` of a quarter turn from the pole: its radius
        // about `√2·2⁻³⁰`, so `L_u + L_v` stays near the cap.
        assert!(near.v == cap.v && near.u < level.u, "{near:?}");
        assert!(near.u + level.v <= cap.u + 1, "{near:?}");
        assert_eq!(
            l.tile(level, i, j).unwrap_err(),
            LatticeError::LevelBeyondCap {
                form: Box::new(key.into()),
                level,
                cap: near,
            }
        );
        let c = l.capped(&one(i, j)).unwrap();
        assert_eq!(c.level, Level { u: near.u, ..level });
        assert_eq!(
            (c.u.start << (level.u - near.u), c.v.start),
            (i & !((1 << (level.u - near.u)) - 1), j)
        );
        let t = l.tile(c.level, c.u.start, c.v.start).unwrap();
        assert!(t.singular() != varde_decide::bezier::Sides::NONE);
        assert!(t.is_regular(), "{c:?}");
        // One level finer around the axis is refused.
        let past = Level {
            u: near.u + 1,
            ..level
        };
        assert!(matches!(
            l.tile(past, c.u.start, j),
            Err(LatticeError::LevelBeyondCap { .. })
        ));
        // A rectangle reaching the pole takes its tile's cap.
        let rows = Rect {
            level,
            u: 0..4,
            v: if j < 0 { -h..-h + 8 } else { h - 8..h },
        };
        assert_eq!(l.cap_of(&rows).unwrap(), near);
    }
    // Further from the pole a deeper cap, and next to the equator (its
    // far row the pole's other end) the sphere's own.
    let pole = l.cap_of(&one(0, h - 1)).unwrap();
    let off = l.cap_of(&one(0, h - (1 << 10))).unwrap();
    let mid = l.cap_of(&one(0, h / 2)).unwrap();
    assert!(
        off.u == pole.u + 10 && mid.u == cap.u,
        "{pole:?} {off:?} {mid:?}"
    );
    for j in [-1, 0] {
        assert_eq!(l.cap_of(&one(0, j)).unwrap(), cap);
        assert!(
            l.tile(Level { u: cap.u, ..level }, 3, j)
                .unwrap()
                .is_regular()
        );
    }
    // At the base latitude level, the sphere's own cap.
    let base = Rect {
        level: Level { u: 3, v: 0 },
        u: 0..8,
        v: -1..1,
    };
    assert_eq!(l.cap_of(&base).unwrap(), cap);
}
