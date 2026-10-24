//! Affine images against their sources: every base tile the source's
//! with each coordinate mapped exactly and rounded once, weights kept;
//! the source's cells and canonical rectangle; tiles at every level on
//! the image, sharing rows, pole rows one point; cusps refused; the
//! level cap no deeper than the source's.

use malachite::Rational as Q;
use varde_testkit::rng::Rng;

use super::super::testing::{exact, level_to, rows_shared, scale};
use super::*;
use crate::geom::conic::Conic;
use crate::geom::form::{
    ConeKey, ConicCylinderKey, CylinderKey, FormKey, PlaneKey, RevolvedKey, SphereKey, TorusKey,
};
use crate::geom::lattice::{Level, Rect, frame, larger};

const TOL: f64 = 1e-15;

/// Sources of every kind, about points up to `1e3` out.
fn sources(rng: &mut Rng) -> Vec<FormKey> {
    let mut p = || rng.point(1e3).to_array();
    let (p0, p1, p2, p3, p4, p5, p6) = (p(), p(), p(), p(), p(), p(), p());
    let a = [0.3, -0.4, 0.85];
    let quarter = Conic::ellipse(
        [0.0, 0.0, 0.0],
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        [2.0, 1.0],
    )
    .unwrap()
    .quarter::<4>(0)
    .unwrap();
    let meridian = Conic::ellipse([3.0, 1.0], [[1.0, 0.0], [0.0, 1.0]], [1.0, 2.0]).unwrap();
    let profile: Vec<_> = (0..3).map(|k| meridian.quarter::<3>(k).unwrap()).collect();
    vec![
        PlaneKey::new(a, p0).unwrap().into(),
        CylinderKey::new(p1, a, 2.5).unwrap().into(),
        ConicCylinderKey::new(a, &[quarter]).unwrap().into(),
        ConeKey::new(p2, a, 1.0, 0.75).unwrap().into(),
        SphereKey::new(p3, 4.0, a).unwrap().into(),
        TorusKey::new(p4, a, 5.0, 2.0).unwrap().into(),
        TorusKey::new(p5, a, 2.0, 3.0).unwrap().into(),
        RevolvedKey::new(p6, a, &profile).unwrap().into(),
    ]
}

/// Maps: a per-axis scale, a reflection, a well-conditioned general map
/// (the identity's double plus a random part), with shifts up to `1e3`.
fn maps(rng: &mut Rng) -> Vec<([[f64; 3]; 3], [f64; 3])> {
    let general: [[f64; 3]; 3] = std::array::from_fn(|r| {
        std::array::from_fn(|c| rng.range(-1.0, 1.0) + if r == c { 2.0 } else { 0.0 })
    });
    let mut t = || rng.point(1e3).to_array();
    vec![
        ([[2.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.5]], t()),
        (
            [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            [0.0; 3],
        ),
        (general, t()),
    ]
}

fn images(seed: u64) -> Vec<AffineKey> {
    let mut rng = Rng::new(seed);
    let maps = maps(&mut rng);
    let mut out = Vec::new();
    for s in sources(&mut rng) {
        for (m, t) in &maps {
            out.push(AffineKey::new(s.clone(), *m, *t).unwrap());
        }
    }
    out
}

#[test]
fn base_tiles_are_the_source_s_mapped() {
    for key in images(1) {
        let src = Lattice::new(key.source().clone()).unwrap();
        let img = Lattice::new(key.clone().into()).unwrap();
        let f = frame(&key.clone().into()).unwrap();
        assert_eq!((img.origin(), img.axes()), (f.origin, f.axes));
        let half = src.half(-1).unwrap();
        assert_eq!(img.half(-1).unwrap(), half);
        // Its base cells (a finer level's tiles are the image's base
        // tiles' sub-patches, not the source's mapped).
        let rect = Rect {
            level: Level::BASE,
            u: half.u.start >> half.level.u..((half.u.end - 1) >> half.level.u) + 1,
            v: half.v.start >> half.level.v..((half.v.end - 1) >> half.level.v) + 1,
        };
        let m = key.linear().map(|r| r.map(q));
        let t = key.shift().map(q);
        for (a, b) in src
            .tiles(&rect)
            .unwrap()
            .iter()
            .zip(img.tiles(&rect).unwrap())
        {
            for (p, r) in a.net().iter().zip(b.net()) {
                let x = [p[0], p[1], p[2]].map(q);
                let want: [f64; 3] = std::array::from_fn(|c| {
                    nearest(&(&m[c][0] * &x[0] + &m[c][1] * &x[1] + &m[c][2] * &x[2] + &t[c]))
                });
                assert_eq!(
                    [r[0], r[1], r[2], r[3]].map(f64::to_bits),
                    [want[0], want[1], want[2], p[3]].map(f64::to_bits),
                    "{key:?}"
                );
            }
        }
    }
}

/// `M⁻¹` exactly.
fn inverse(m: &[[f64; 3]; 3]) -> [[Q; 3]; 3] {
    let m = m.map(|r| r.map(q));
    let co = |r: usize, c: usize| {
        let (r0, r1, c0, c1) = ((r + 1) % 3, (r + 2) % 3, (c + 1) % 3, (c + 2) % 3);
        &m[r0][c0] * &m[r1][c1] - &m[r0][c1] * &m[r1][c0]
    };
    let det = &m[0][0] * co(0, 0) + &m[0][1] * co(0, 1) + &m[0][2] * co(0, 2);
    std::array::from_fn(|r| std::array::from_fn(|c| co(c, r) / &det))
}

/// The tiles of every level visited lie on the image: a tile's exact
/// point mapped back lies on the source's implicit (plane, cylinder,
/// sphere: its distance from it at most `|f|/ρ`, the plane's `|f|`)
/// within the source tile's rounding plus the image's, scaled back.
#[test]
fn tiles_lie_on_the_image_at_every_level() {
    let mut rng = Rng::new(3);
    let mut worst: f64 = 0.0;
    for key in images(2) {
        let radius = match key.source() {
            FormKey::Plane(_) => 1.0,
            FormKey::Cylinder(c) => c.radius(),
            FormKey::Sphere(s) => s.radius(),
            _ => continue,
        };
        let src = Lattice::new(key.source().clone()).unwrap();
        let img = Lattice::new(key.clone().into()).unwrap();
        let inv = inverse(&key.linear());
        let norm = inv
            .iter()
            .flatten()
            .map(|x| {
                let x = nearest(x);
                x * x
            })
            .sum::<f64>()
            .sqrt();
        let t = key.shift().map(q);
        let rect = src.half(0).unwrap();
        for n in 0..8 {
            let at = n % (rect.u.end - rect.u.start) as usize;
            let (k, l) = (rect.u.start + at as i64, rect.v.start);
            let level = if n == 0 {
                rect.level
            } else {
                let cap = img.cap_of(&Rect {
                    level: Level::BASE,
                    u: k..k + 1,
                    v: l..l + 1,
                });
                level_to(cap.unwrap(), &mut rng)
            };
            let level = Level {
                u: level.u.max(rect.level.u),
                v: level.v.max(rect.level.v),
            };
            let i = (k << level.u) + (rng.next_u64() % (1 << level.u)) as i64;
            let j = (l << level.v) + (rng.next_u64() % (1 << level.v)) as i64;
            let tile = img.tile(level, i, j).unwrap();
            let (ms, mi) = (scale(&src.base_tile(k, l).unwrap()), scale(&tile));
            let e = exact(&tile);
            for (u, v) in [(0, 0), (1, 3), (2, 2), (4, 1)] {
                let p = e.eval(&Q::from_signeds(u, 4), &Q::from_signeds(v, 4));
                let d: [Q; 3] = std::array::from_fn(|c| &p[c] - &t[c]);
                let x: [Q; 3] = std::array::from_fn(|r| {
                    &inv[r][0] * &d[0] + &inv[r][1] * &d[1] + &inv[r][2] * &d[2]
                });
                let f = key.source().implicit(&x).unwrap();
                let off = nearest(&f).abs() / radius;
                let bound = 2.0 * TOL * (ms + norm * mi);
                assert!(
                    off <= bound,
                    "{key:?} {level:?} ({i}, {j}): {off:e} > {bound:e}"
                );
                worst = larger(worst, off / bound);
            }
        }
    }
    eprintln!("worst distance over its bound: {worst:e}");
}

/// Rows shared across cells at every level; a row that is one point in
/// the source (a pole or apex) is one point in the image, and a singular
/// side of its tiles as of the source's; every tile regular.
#[test]
fn rows_shared_and_axis_rows_one_point() {
    for key in images(4) {
        let src = Lattice::new(key.source().clone()).unwrap();
        let img = Lattice::new(key.clone().into()).unwrap();
        let half = img.half(-1).unwrap();
        for extra in [Level::BASE, Level { u: 1, v: 1 }, Level { u: 2, v: 3 }] {
            let level = Level {
                u: half.level.u + extra.u,
                v: half.level.v + extra.v,
            };
            let rect = Rect {
                level,
                u: half.u.start << extra.u..half.u.end << extra.u,
                v: half.v.start << extra.v..half.v.end << extra.v,
            };
            let tiles = img.tiles(&rect).unwrap();
            rows_shared(&tiles, &rect);
            let point = |t: &crate::geom::tile::Tile, i: usize, row: usize| {
                let x = t.net()[3 * i + row];
                [x[0], x[1], x[2]].map(f64::to_bits)
            };
            for (a, b) in src.tiles(&rect).unwrap().iter().zip(&tiles) {
                assert_eq!(b.singular(), a.singular(), "{key:?} {level:?}");
                assert!(b.is_regular(), "{key:?} {level:?}");
                for row in [0, 2] {
                    if (0..3).all(|i| point(a, i, row) == point(a, 0, row)) {
                        assert!(
                            (0..3).all(|i| point(b, i, row) == point(b, 0, row)),
                            "{key:?} {level:?}"
                        );
                    }
                }
            }
        }
    }
}

/// A source's cusp stays refused in its image.
#[test]
fn cusps_stay_refused() {
    let circle = Conic::circle([1.0, 0.0], [[1.0, 0.0], [0.0, 1.0]], 1.0).unwrap();
    let quarters: Vec<_> = (0..2).map(|k| circle.quarter::<3>(k).unwrap()).collect();
    let horn = RevolvedKey::new([0.0; 3], [0.0, 0.0, 1.0], &quarters).unwrap();
    let cusp = horn
        .profile()
        .spans()
        .iter()
        .position(|s| s[0][0] == 0.0 || s[2][0] == 0.0)
        .unwrap() as i64;
    let m = [[2.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 3.0]];
    let key = AffineKey::new(horn.into(), m, [1.0, 0.0, 0.0]).unwrap();
    let l = Lattice::new(key.clone().into()).unwrap();
    assert_eq!(l.base_tile(0, cusp).unwrap_err(), LatticeError::Cusp);
    assert!(l.base_tile(0, 1 - cusp).is_ok());
    assert_eq!(affine_image(key, 0).unwrap_err(), LatticeError::Cusp);
}

/// The image's cap is no deeper than its source's in either direction,
/// within a few levels of it under a doubling, and lower under a squash;
/// a level beyond it is refused, the cap itself admitted.
#[test]
fn caps_follow_the_map() {
    let mut rng = Rng::new(5);
    let double = [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]];
    let squash = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1e-3]];
    for s in sources(&mut rng) {
        let src = Lattice::new(s.clone()).unwrap();
        let cap = src.cap();
        let image =
            |m| Lattice::new(AffineKey::new(s.clone(), m, [0.0; 3]).unwrap().into()).unwrap();
        let (d, q) = (image(double), image(squash));
        for (a, b) in [(d.cap().u, cap.u), (d.cap().v, cap.v)] {
            assert!(a <= b && a + 6 >= b, "{s:?} {:?} {cap:?}", d.cap());
        }
        assert!(q.cap().u <= d.cap().u && q.cap().v <= d.cap().v, "{s:?}");
        assert!(
            q.cap().u < cap.u.max(1) || q.cap().v < cap.v.max(1),
            "{s:?} {:?} {cap:?}",
            q.cap()
        );
        let base = d.half(0).unwrap();
        let (k, l) = (base.u.start >> base.level.u, base.v.start >> base.level.v);
        let at = |level: Level| d.tile(level, k << level.u, l << level.v);
        // The cap of the corner tile at the form's cap: lower around
        // the axis beside a pole or apex.
        let deep = d.cap();
        let one = |n: i64, lv: u32| (n << lv)..(n << lv) + 1;
        let cap = d
            .cap_of(&Rect {
                level: deep,
                u: one(k, deep.u),
                v: one(l, deep.v),
            })
            .unwrap();
        assert!(cap.v == deep.v && cap.u <= deep.u, "{s:?} {cap:?}");
        assert!(at(cap).is_ok(), "{s:?}");
        if cap.u < crate::geom::lattice::MAX_LEVEL {
            let past = Level {
                u: cap.u + 1,
                v: cap.v,
            };
            assert!(
                matches!(at(past), Err(LatticeError::LevelBeyondCap { .. })),
                "{s:?}"
            );
        }
    }
}
