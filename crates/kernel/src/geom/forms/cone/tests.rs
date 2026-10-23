//! The cone's constructor against exact values and OCCT's net: every
//! control point the exact value rounded once, the apex row the apex at
//! every level, every tile on the key's cone, regular off the apex and
//! following the tangent to the cap.

use malachite::Rational as Q;
use varde_decide::num::trig::{cos, sin};
use varde_testkit::rng::Rng;

use super::super::testing::{
    dot, exact, frame, level_to, normal_angle, rounded_once, rows_shared, scale,
};
use super::*;
use crate::geom::form::FormKey;
use crate::geom::lattice::{Level, Rect};
use crate::geom::round::nearest;

const TOL: f64 = 1e-15;

/// Cones with apexes up to `1e6` out, half-angles from narrow to flat.
fn cones(seed: u64) -> Vec<ConeKey> {
    let mut rng = Rng::new(seed);
    let mut out = vec![ConeKey::new([0.0; 3], [0.0, 0.0, 1.0], 1.0, 1.0).unwrap()];
    for k in 0..9 {
        let p = rng.point([1.0, 1e3, 1e6][k % 3]).to_array();
        let (along, across) = match k % 3 {
            0 => (1.0, rng.range(0.1, 10.0)),
            1 => (1.0, rng.log_range(1e-3, 1e-1)),
            _ => (rng.log_range(1e-3, 1e-1), 1.0),
        };
        out.push(ConeKey::new(p, rng.direction().to_array(), along, across).unwrap());
    }
    out
}

#[test]
fn tiles_are_exact_values_rounded_once() {
    let root = std::f64::consts::FRAC_1_SQRT_2;
    for key in cones(1) {
        let l = Lattice::new(key.into()).unwrap();
        let (o, [e1, e2, a]) = frame(&l);
        assert_eq!(l.origin(), key.apex());
        assert_eq!(l.axes()[2], key.axis());
        let (c, s) = (q(key.cos()), q(key.sin()));
        let neg = |x: &[Q; 3]| x.clone().map(|c| -c);
        let radii = [e1.clone(), e2.clone(), neg(&e1), neg(&e2)];
        for k in -2..3i64 {
            for cell in [0i64, 1, 5] {
                let r0 = &radii[k.rem_euclid(4) as usize];
                let r1 = &radii[(k + 1).rem_euclid(4) as usize];
                let corner: [Q; 3] = std::array::from_fn(|i| &r0[i] + &r1[i]);
                let dirs = [(r0.clone(), 1.0), (corner, root), (r1.clone(), 1.0)];
                let want: [([Q; 3], f64); 9] = std::array::from_fn(|n| {
                    let (d, w) = &dirs[n / 3];
                    let t = Q::from(2 * cell + (n % 3) as i64) * q(CELL) / Q::from(2u32);
                    let p = std::array::from_fn(|i| &o[i] + &t * (&c * &a[i] + &s * &d[i]));
                    (p, *w)
                });
                let what = format!("{key:?} ({k}, {cell})");
                rounded_once(&l.base_tile(k, cell).unwrap(), &want, &what);
            }
        }
    }
}

/// The distance from `p` to the key's cone, over the line of its
/// generators in the meridian plane: `|ρ·cos − h·sin|/√(cos² + sin²)`
/// for `h` along the axis and `ρ` from it, as `(ρ²cos² − h²sin²)/(ρ·cos +
/// h·sin)` (the numerator exact); `None` at the apex. Asserts `h ≥ 0`
/// (the nappe used) to the rounding of `|r|`.
fn distance(key: &ConeKey, p: &[Q; 3]) -> Option<f64> {
    let (apex, a) = (key.apex().map(q), key.axis().map(q));
    let r: [Q; 3] = std::array::from_fn(|i| &p[i] - &apex[i]);
    let rr = dot(&r, &r);
    if rr == 0u32 {
        return None;
    }
    let aa = dot(&a, &a);
    let ra = dot(&r, &a);
    let (c, s) = (q(key.cos()), q(key.sin()));
    // `h² = (r·a)²/|a|²`, `ρ² = |r|² − h²`.
    let h2 = &ra * &ra / &aa;
    let rho2 = &rr - &h2;
    let num = &rho2 * &c * &c - &h2 * &s * &s;
    let h = nearest(&h2).sqrt() * if ra < 0u32 { -1.0 } else { 1.0 };
    assert!(
        h >= -1e-14 * nearest(&rr).sqrt(),
        "{key:?}: the other nappe"
    );
    let den = nearest(&rho2).sqrt() * key.cos() + h.abs() * key.sin();
    let norm = (key.cos() * key.cos() + key.sin() * key.sin()).sqrt();
    Some(nearest(&num).abs() / den / norm)
}

#[test]
fn tiles_lie_on_the_cone_at_every_level() {
    let mut rng = Rng::new(2);
    let mut worst: f64 = 0.0;
    let levels = [
        Level::BASE,
        Level { u: 1, v: 0 },
        Level { u: 0, v: 1 },
        Level { u: 3, v: 2 },
        Level { u: 4, v: 4 },
        Level { u: 9, v: 20 },
    ];
    for key in cones(3) {
        let l = Lattice::new(key.into()).unwrap();
        for (k, cell) in [(-3i64, 0i64), (0, 0), (1, 0), (2, 1), (1, 3)] {
            let m = scale(&l.base_tile(k, cell).unwrap());
            for level in levels {
                let (nu, nv) = (1i64 << level.u, 1i64 << level.v);
                for (a, b) in [(0, 0), (nu - 1, nv - 1), (nu / 2, nv / 3)] {
                    let t = l
                        .tile(level, (k << level.u) + a, (cell << level.v) + b)
                        .unwrap();
                    let e = exact(&t);
                    let mut ts: Vec<Q> = [0.0, 0.25, 0.5, 0.75, 1.0].map(q).to_vec();
                    ts.push(q((rng.next_u64() % 1024) as f64 / 1024.0));
                    for u in &ts {
                        for v in &ts {
                            let Some(d) = distance(&key, &e.eval(u, v)) else {
                                continue;
                            };
                            let d = d / m;
                            assert!(d <= TOL, "{key:?} ({k}, {cell}) {level:?}: {d:e}");
                            worst = if d > worst { d } else { worst };
                        }
                    }
                }
            }
        }
    }
    eprintln!("worst distance over the base tile's scale: {worst:e}");
}

#[test]
fn the_apex_row_is_the_apex_at_every_level() {
    for key in cones(4) {
        let l = Lattice::new(key.into()).unwrap();
        let apex = key.apex().map(f64::to_bits);
        for level in [
            Level::BASE,
            Level { u: 3, v: 2 },
            Level { u: 8, v: 8 },
            Level { u: 20, v: 30 },
        ] {
            for i in [0i64, 1, (3 << level.u) - 1, -5] {
                let t = l.tile(level, i, 0).unwrap();
                for k in 0..3 {
                    assert_eq!(
                        t.point(k, 0).map(f64::to_bits),
                        apex,
                        "{key:?} {level:?} {i}"
                    );
                }
                // The weights along the row are the quarter circle's.
                if level.u == 0 {
                    let w = [0, 1, 2].map(|k| t.net()[3 * k][3]);
                    assert_eq!(w, [1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]);
                }
                assert!(!t.is_regular(), "an apex row is not regular here");
            }
        }
    }
}

#[test]
fn the_canonical_cone_is_half_a_turn_from_the_apex() {
    let key = ConeKey::new([1.0, -2.0, 0.25], [1.0, 2.0, 2.0], 3.0, 1.0).unwrap();
    for first in [-1, 0, 3] {
        let s = cone(key, first).unwrap();
        assert_eq!(
            (s.rect().u.clone(), s.rect().v.clone()),
            (first..first + 2, 0..1)
        );
        rows_shared(s.tiles(), s.rect());
        for t in s.tiles() {
            assert_eq!(t.point(1, 0), key.apex());
        }
    }
    // The other nappe is not the cone's.
    let l = Lattice::new(key.into()).unwrap();
    assert_eq!(l.base_tile(0, -1).unwrap_err(), LatticeError::Index);
    let rect = Rect {
        level: Level { u: 0, v: 2 },
        u: 0..1,
        v: -1..2,
    };
    assert_eq!(l.tiles(&rect).unwrap_err(), LatticeError::Index);
    assert_eq!(
        l.tile(Level { u: 0, v: 2 }, 0, -1).unwrap_err(),
        LatticeError::Index
    );
    assert_eq!(l.base_tile(0, i64::MAX).unwrap_err(), LatticeError::Index);
}

#[test]
fn rows_are_shared_and_cells_repeat_every_turn() {
    for key in cones(5) {
        let l = Lattice::new(key.into()).unwrap();
        for level in [Level::BASE, Level { u: 2, v: 1 }, Level { u: 5, v: 4 }] {
            let (hu, hv) = (1i64 << level.u, 1i64 << level.v);
            let rect = Rect {
                level,
                u: -1..hu + 1,
                v: 0..hv + 2,
            };
            let tiles = l.tiles(&rect).unwrap();
            rows_shared(&tiles, &rect);
            let nv = (rect.v.end - rect.v.start) as usize;
            for (n, t) in tiles.iter().enumerate().step_by(7) {
                let (i, j) = (
                    rect.u.start + (n / nv) as i64,
                    rect.v.start + (n % nv) as i64,
                );
                assert_eq!(*t, l.tile(level, i, j).unwrap(), "({i}, {j}) {level:?}");
                assert_eq!(*t, l.tile(level, i + (4 << level.u), j).unwrap());
            }
        }
    }
}

#[test]
fn equal_keys_give_equal_lattices() {
    // One cone from a scaled axis and a scaled half-angle's legs.
    let a = Lattice::new(
        ConeKey::new([1.0, -2.0, 0.25], [1.0, 2.0, 2.0], 3.0, 4.0)
            .unwrap()
            .into(),
    )
    .unwrap();
    let b = Lattice::new(
        ConeKey::new([1.0, -2.0, 0.25], [0.5, 1.0, 1.0], 0.75, 1.0)
            .unwrap()
            .into(),
    )
    .unwrap();
    assert_eq!(a.key(), b.key());
    let r = a.half(-1).unwrap();
    assert_eq!(a.tiles(&r).unwrap(), b.tiles(&r).unwrap());
    // The 3-4-5 cone's sine and cosine are exact.
    let FormKey::Cone(k) = a.key() else {
        unreachable!()
    };
    assert_eq!((k.cos(), k.sin()), (0.6, 0.8));
}

/// OCCT's `Convert_ConeToBSplineSurface` (its `ComputePoles`, OCCT
/// commit b8f597c677811d1f9f4d8a97f5ae2825c0353a42), with reference
/// radius 0 at the apex, the semi-angle's sine and cosine the key's, over
/// the cell's turn `[U₁, U₂]` and slant extent `[V₁, V₂]`: poles `(x cos
/// U, x sin U, z)` with `x = V·sin`, `z = V·cos`, the middle ones `x cos(U
/// + α)/cos α` (and `sin`), weights `1, cos α, 1`, degree 1 in `V`. The
/// tile is that net, elevated to degree 2 in `V`, in the key's frame.
#[test]
fn tiles_are_occt_s_nets() {
    for key in cones(6) {
        let l = Lattice::new(key.into()).unwrap();
        let (o, [e1, e2, a]) = (l.origin(), l.axes());
        for (k, cell) in [(-1i64, 0i64), (0, 0), (2, 0), (1, 2)] {
            let tile = l.base_tile(k, cell).unwrap();
            let m = scale(&tile);
            let alpha = std::f64::consts::FRAC_PI_4;
            let u1 = k as f64 * std::f64::consts::FRAC_PI_2;
            let us = [u1, u1 + alpha, u1 + 2.0 * alpha];
            let v1 = cell as f64 * CELL;
            let vs = [v1, v1 + CELL / 2.0, v1 + CELL];
            for (i, u) in us.iter().enumerate() {
                let f = if i == 1 { 1.0 / cos(alpha) } else { 1.0 };
                let w = if i == 1 { cos(alpha) } else { 1.0 };
                for (j, v) in vs.iter().enumerate() {
                    let (x, z) = (v * key.sin(), v * key.cos());
                    let (px, py) = (x * cos(*u) * f, x * sin(*u) * f);
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
                    assert!((got[3] - w).abs() <= f64::EPSILON, "{key:?} weight");
                }
            }
        }
    }
}

/// Off the apex, at every level to the cap, a tile in slant cell 0 is
/// regular, faces out of the cone, and its normals lie within `2⁻⁵` rad
/// of the cone's at its centre beyond the turn it spans; rows from half
/// the cell's slant on (the cap reads the far row's radius, and around
/// the axis a cell's chord shrinks towards the apex).
#[test]
fn tiles_follow_the_tangent_to_the_cap() {
    let mut rng = Rng::new(7);
    let mut worst: f64 = 0.0;
    for key in cones(8) {
        let l = Lattice::new(key.into()).unwrap();
        let cap = l.cap();
        let a = key.axis();
        for n in 0..varde_testing::pick(6, 40) {
            let mut level = if n == 0 { cap } else { level_to(cap, &mut rng) };
            level.v = level.v.max(1);
            let (hu, hv) = (1i64 << level.u, 1i64 << level.v);
            let i = (rng.next_u64() % (4 * hu) as u64) as i64 - 2 * hu;
            let j = hv / 2 + (rng.next_u64() % (hv / 2).max(1) as u64) as i64;
            let turn = std::f64::consts::FRAC_PI_2 / (1u64 << level.u) as f64;
            if turn > 0.1 {
                continue;
            }
            let t = l.tile(level, i, j).unwrap();
            let what = format!("{key:?} {level:?} ({i}, {j})");
            assert!(t.is_regular(), "{what}");
            let half = Q::from_signeds(1, 2);
            let p = exact(&t).eval(&half, &half).map(|x| nearest(&x));
            let r: [f64; 3] = std::array::from_fn(|c| p[c] - key.apex()[c]);
            let h = r[0] * a[0] + r[1] * a[1] + r[2] * a[2];
            let rad: [f64; 3] = std::array::from_fn(|c| r[c] - h * a[c]);
            let len = (rad[0] * rad[0] + rad[1] * rad[1] + rad[2] * rad[2]).sqrt();
            // Out of the cone: away from the axis, back towards the apex.
            let out: [f64; 3] =
                std::array::from_fn(|c| key.cos() * rad[c] / len - key.sin() * a[c]);
            let beyond = normal_angle(&t, out, &what) - 2.0 * turn;
            worst = if beyond > worst { beyond } else { worst };
            assert!(beyond < 1.0 / 32.0, "{what}: {beyond:e}");
        }
    }
    eprintln!("worst angle beyond the tile's turn: {worst:e} rad");
}

#[test]
fn caps_follow_the_geometry() {
    let c = |apex: [f64; 3], across: f64| {
        Lattice::new(
            ConeKey::new(apex, [1.0, 2.0, 2.0], 1.0, across)
                .unwrap()
                .into(),
        )
        .unwrap()
    };
    let near = c([0.0; 3], 1.0);
    // Along the generators a line of the slant cell; around the axis the
    // far row's radius `sin·CELL`.
    assert!(near.cap().v >= 40 && near.cap().u >= 40, "{:?}", near.cap());
    // A narrower cone has a smaller far radius: a lower cap around.
    let narrow = c([0.0; 3], 1e-3);
    assert!(narrow.cap().u + 5 <= near.cap().u, "{:?}", narrow.cap());
    // A slant cell further out has a lower cap along the generators than
    // the box's; around the axis its far radius grows as its coordinates'
    // spacing does.
    let rect = Rect {
        level: Level::BASE,
        u: 0..1,
        v: 30..31,
    };
    let far = near.cap_of(&rect).unwrap();
    assert!(
        far.u <= near.cap().u && far.v + 3 <= near.cap().v,
        "{far:?}"
    );
}
