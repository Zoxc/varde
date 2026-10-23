//! The revolved conic's constructor against exact values: every control
//! point the exact value rounded once; every tile at every level on the
//! key's surface of revolution, sharing rows across spans and angle
//! cells, its rows on the axis one point; regular off the axis and
//! following the tangent to the cap; equal keys from other inputs the
//! same tiles; cusps refused, as are crossing profiles, closed profiles
//! taken whole and joints whose weights differ.

use malachite::Rational as Q;
use varde_decide::bezier::{HomCurve, Side, Sides};
use varde_testkit::rng::Rng;

use super::super::testing::{
    dot, exact, frame, level_to, local, normal_angle, rounded_once, rows_shared, scale,
};
use super::*;
use crate::geom::conic::Conic;
use crate::geom::form::{CylinderKey, FormError, FormKey};
use crate::geom::lattice::larger;
use crate::geom::round::{nearest, nearest_sqrt, q};

const TOL: f64 = 1e-15;

const Z: [f64; 3] = [0.0, 0.0, 1.0];

/// The `(ρ, z)` axes of a profile plane.
const MERIDIAN: [[f64; 2]; 2] = [[1.0, 0.0], [0.0, 1.0]];

/// Profiles in the meridian half-plane that keep off the axis, `kind` 0
/// three quarters of an ellipse, 1 parabola arcs, 2 hyperbola arcs (whose
/// middle weights share a binade, so their joints' weights agree).
fn off_axis(rng: &mut Rng, kind: u64) -> Vec<Span2> {
    let r = rng.log_range(1e-2, 1e3);
    let c = [r * rng.range(1.5, 4.0), r * rng.range(-3.0, 3.0)];
    match kind {
        0 => {
            let e = Conic::ellipse(c, MERIDIAN, [r, r * rng.range(0.3, 1.0)]).unwrap();
            (0..3).map(|k| e.quarter::<3>(k).unwrap()).collect()
        }
        1 => {
            // Opening away from the axis, vertex at `ρ > 0`.
            let p = Conic::parabola(c, [[0.0, 1.0], [1.0, 0.0]], r).unwrap();
            let ts = [-1.5, -0.5, 0.25, 1.0];
            ts.windows(2)
                .map(|t| p.arc::<3>(t[0], t[1]).unwrap())
                .collect()
        }
        _ => {
            let h = Conic::hyperbola(c, MERIDIAN, [r, 0.7 * r]).unwrap();
            let ss = [1.1, 1.3, 1.6, 1.9];
            ss.windows(2)
                .map(|s| h.arc::<3>(s[0], s[1]).unwrap())
                .collect()
        }
    }
}

/// Profiles meeting the axis transversally at an end: an ellipse centred
/// on the axis from pole to pole (two quarters), and an oblique span
/// leaving the axis at an apex followed by a span off it.
fn on_axis(rng: &mut Rng) -> [Vec<Span2>; 2] {
    let r = rng.log_range(1e-2, 1e3);
    let z = r * rng.range(-3.0, 3.0);
    let e = Conic::ellipse([0.0, z], MERIDIAN, [r, r * rng.range(0.3, 2.0)]).unwrap();
    let poles = vec![e.quarter::<3>(3).unwrap(), e.quarter::<3>(0).unwrap()];
    let apex = vec![
        [
            [0.0, z, 1.0],
            [r, z + 2.0 * r, 0.75],
            [3.0 * r, z + 2.5 * r, 1.0],
        ],
        [
            [3.0 * r, z + 2.5 * r, 1.0],
            [5.0 * r, z + 3.0 * r, 2.0],
            [5.0 * r, z + 4.0 * r, 1.0],
        ],
    ];
    [poles, apex]
}

/// Revolved conics about random axes up to `1e6` out, every kind of
/// profile; `axis` with the profiles meeting the axis too.
fn keys(seed: u64, axis: bool) -> Vec<RevolvedKey> {
    let mut rng = Rng::new(seed);
    let mut out = Vec::new();
    for k in 0..9 {
        let p = rng.point([1.0, 1e3, 1e6][k % 3]).to_array();
        let a = rng.direction().to_array();
        out.push(RevolvedKey::new(p, a, &off_axis(&mut rng, k as u64 % 3)).unwrap());
        if axis {
            for spans in on_axis(&mut rng) {
                out.push(RevolvedKey::new(p, a, &spans).unwrap());
            }
        }
    }
    out
}

/// The exact point `(ρ, z)` of a profile span at `s` and its tangent
/// `N′W − NW′` (a positive multiple of the derivative).
fn on_span(span: &Span2, s: &Q) -> ([Q; 2], [Q; 2]) {
    let c =
        HomCurve::<Q, 2>::from_affine(span.map(|p| ([p[0], p[1]], p[2])), &Q::from(0u32)).unwrap();
    let ([nx, ny], w) = c.eval_hom(s);
    let ([dx, dy], dw) = c.derivative().eval_hom(s);
    let p = [&nx / &w, &ny / &w];
    let t = [&dx * &w - &nx * &dw, &dy * &w - &ny * &dw];
    (p, t)
}

#[test]
fn tiles_are_exact_values_rounded_once() {
    for key in keys(1, true) {
        let l = Lattice::new(key.clone().into()).unwrap();
        assert_eq!((l.origin(), l.axes()[2]), (key.foot(), key.axis()));
        let (o, [e1, e2, _]) = frame(&l);
        let a = key.axis().map(q);
        let neg = |x: &[Q; 3]| x.clone().map(|c| -c);
        let radii = [e1.clone(), e2.clone(), neg(&e1), neg(&e2)];
        for k in -2..3i64 {
            let r0 = &radii[k.rem_euclid(4) as usize];
            let r1 = &radii[(k + 1).rem_euclid(4) as usize];
            let corner: [Q; 3] = std::array::from_fn(|c| &r0[c] + &r1[c]);
            let dirs = [r0.clone(), corner, r1.clone()];
            for (cell, span) in key.profile().spans().iter().enumerate() {
                let want: [([Q; 3], f64); 9] = std::array::from_fn(|n| {
                    let [rho, z, w] = span[n % 3];
                    let p =
                        std::array::from_fn(|c| &o[c] + q(rho) * &dirs[n / 3][c] + q(z) * &a[c]);
                    let w = if n / 3 == 1 {
                        nearest_sqrt(&(q(w) * q(w) / Q::from(2u32)))
                    } else {
                        w
                    };
                    (p, w)
                });
                let t = l.base_tile(k, cell as i64).unwrap();
                rounded_once(&t, &want, &format!("{key:?} ({k}, {cell})"));
            }
        }
    }
}

/// The tiles of every level visited lie on the form at their own profile
/// parameter: a tile's exact point at `(u, v)` has the profile's height
/// along the axis and distance from it at the span parameter there, to
/// the rounding of the coordinates (`TOL` of the base tile's scale).
#[test]
fn tiles_follow_the_profile_at_every_level() {
    let mut rng = Rng::new(5);
    let mut worst: f64 = 0.0;
    for key in keys(4, true) {
        let l = Lattice::new(key.clone().into()).unwrap();
        let (o, _) = frame(&l);
        let a = key.axis().map(q);
        let spans = key.profile().spans();
        for n in 0..12 {
            let k = (rng.next_u64() % 4) as i64 - 2;
            let c = (rng.next_u64() % spans.len() as u64) as i64;
            let cell = Rect {
                level: Level::BASE,
                u: k..k + 1,
                v: c..c + 1,
            };
            let cap = l.cap_of(&cell).unwrap();
            let level = if n == 0 {
                Level::BASE
            } else {
                level_to(cap, &mut rng)
            };
            let i = (k << level.u) + (rng.next_u64() % (1 << level.u)) as i64;
            let j = (c << level.v) + (rng.next_u64() % (1 << level.v)) as i64;
            let m = scale(&l.base_tile(k, c).unwrap());
            let e = exact(&l.tile(level, i, j).unwrap());
            for (u, v) in [(0, 0), (1, 3), (2, 2), (4, 1), (4, 4)] {
                let (u, v) = (Q::from_signeds(u, 4), Q::from_signeds(v, 4));
                let p = e.eval(&u, &v);
                let s = local(j, level.v) + &v / Q::from(1u64 << level.v);
                let ([rho, z], _) = on_span(&spans[c as usize], &s);
                let d: [Q; 3] = std::array::from_fn(|n| &p[n] - &o[n]);
                // The stored axis is unit to rounding: divide by `|a|²`.
                let h = dot(&d, &a);
                let r2 = dot(&d, &d) - &h * &h / dot(&a, &a);
                let r = if r2 > 0u32 { nearest(&r2).sqrt() } else { 0.0 };
                let off = larger((r - nearest(&rho)).abs(), nearest(&(h - z)).abs()) / m;
                assert!(off <= TOL, "{key:?} {level:?} ({i}, {j}): {off:e}");
                worst = larger(worst, off);
            }
        }
    }
    eprintln!("worst distance over the base tile's scale: {worst:e}");
}

#[test]
fn rows_are_shared_across_spans_and_cells() {
    let levels = [
        Level::BASE,
        Level { u: 1, v: 0 },
        Level { u: 0, v: 2 },
        Level { u: 3, v: 2 },
    ];
    for key in keys(6, true) {
        let l = Lattice::new(key.clone().into()).unwrap();
        let n = key.profile().spans().len() as i64;
        for level in levels {
            let rect = Rect {
                level,
                u: -1 << level.u..2 << level.u,
                v: 0..n << level.v,
            };
            rows_shared(&l.tiles(&rect).unwrap(), &rect);
        }
    }
}

/// A span's end on the axis is one point, to the bit, along the whole
/// row of every tile beside it, at every level: a pole where the span
/// leaves the axis perpendicularly, an apex where it leaves obliquely.
#[test]
fn axis_rows_are_one_point() {
    let mut rng = Rng::new(7);
    for _ in 0..6 {
        let p = rng.point(1e3).to_array();
        let a = rng.direction().to_array();
        for spans in on_axis(&mut rng) {
            let key = RevolvedKey::new(p, a, &spans).unwrap();
            let l = Lattice::new(key.clone().into()).unwrap();
            for (c, span) in key.profile().spans().iter().enumerate() {
                for (end, row) in [(0usize, 0usize), (2, 2)] {
                    if span[end][0] != 0.0 {
                        continue;
                    }
                    for level in [Level::BASE, Level { u: 2, v: 3 }] {
                        // The tiles of the cell's row on the axis.
                        let j = if end == 0 {
                            (c as i64) << level.v
                        } else {
                            ((c as i64 + 1) << level.v) - 1
                        };
                        for i in -2..(2 << level.u) {
                            let t = l.tile(level, i, j).unwrap();
                            let pts: Vec<[u64; 3]> = (0..3)
                                .map(|i| {
                                    let x = t.net()[3 * i + row];
                                    [x[0], x[1], x[2]].map(f64::to_bits)
                                })
                                .collect();
                            assert!(pts.iter().all(|x| *x == pts[0]), "{key:?} {level:?}");
                            // A singular side, which the fold check
                            // divides out.
                            let side = if end == 0 { Side::V0 } else { Side::V1 };
                            assert!(t.singular().contains(side), "{key:?} {level:?}");
                            assert!(t.is_regular(), "{key:?} {level:?} ({i}, {j})");
                        }
                    }
                }
            }
        }
    }
}

/// Every tile of the canonical surfaces is regular, at the base level
/// and finer, those on the axis with their pole or apex rows as declared
/// singular sides and no other tile with any.
#[test]
fn canonical_tiles_are_regular_poles_and_apexes_included() {
    for key in keys(13, true) {
        let l = Lattice::new(key.clone().into()).unwrap();
        let spans = key.profile().spans();
        for extra in [Level::BASE, Level { u: 1, v: 2 }] {
            let half = l.half(-1).unwrap();
            let rect = Rect {
                level: extra,
                u: half.u.start << extra.u..half.u.end << extra.u,
                v: half.v.start << extra.v..half.v.end << extra.v,
            };
            let nv = (rect.v.end - rect.v.start) as usize;
            for (n, t) in l.tiles(&rect).unwrap().iter().enumerate() {
                let j = rect.v.start + (n % nv) as i64;
                let span = &spans[(j >> extra.v) as usize];
                let first = j & ((1 << extra.v) - 1) == 0;
                let last = j & ((1 << extra.v) - 1) == (1 << extra.v) - 1;
                let mut want = Sides::NONE;
                if first && span[0][0] == 0.0 {
                    want = want.with(Side::V0);
                }
                if last && span[2][0] == 0.0 {
                    want = want.with(Side::V1);
                }
                let what = format!("{key:?} {extra:?} tile {n}");
                assert_eq!(t.singular(), want, "{what}");
                assert!(t.is_regular(), "{what}");
            }
        }
    }
}

/// At every level to the cap, a tile off the axis is regular and its
/// normals lie within `2⁻⁵` rad of the form's at its centre, `ρ·(z′r̂ −
/// ρ′a)` (the orientation of the module docs), beyond the turn of its
/// cell.
#[test]
fn tiles_follow_the_tangent_to_the_cap() {
    let mut rng = Rng::new(10);
    let mut worst: f64 = 0.0;
    for key in keys(11, false) {
        let l = Lattice::new(key.clone().into()).unwrap();
        let cap = l.cap();
        assert!(cap.u >= 10 && cap.v >= 10, "{key:?} {cap:?}");
        let (o, _) = frame(&l);
        let a = key.axis().map(q);
        let spans = key.profile().spans();
        for n in 0..varde_testing::pick(6, 40) {
            let level = if n == 0 { cap } else { level_to(cap, &mut rng) };
            let c = (rng.next_u64() % spans.len() as u64) as i64;
            let i = (rng.next_u64() % (4 << level.u)) as i64;
            let j = (c << level.v) + (rng.next_u64() % (1 << level.v)) as i64;
            let t = l.tile(level, i, j).unwrap();
            let s0 = local(j, level.v);
            let h = Q::from_signeds(1, 1i64 << level.v);
            let mid = &s0 + &h / Q::from(2u32);
            let (_, t0) = on_span(&spans[c as usize], &s0);
            let (_, t1) = on_span(&spans[c as usize], &(&s0 + &h));
            let (_, tm) = on_span(&spans[c as usize], &mid);
            let angle = |p: &[Q; 2], r: &[Q; 2]| {
                let (p, r) = (p.each_ref().map(nearest), r.each_ref().map(nearest));
                let d = (p[0] * r[0] + p[1] * r[1])
                    / ((p[0] * p[0] + p[1] * p[1]).sqrt() * (r[0] * r[0] + r[1] * r[1]).sqrt());
                varde_decide::num::trig::acos(d.clamp(-1.0, 1.0))
            };
            // The cell's turn: its profile's and its angle's.
            let turn = angle(&t0, &t1) + std::f64::consts::FRAC_PI_2 / (1u64 << level.u) as f64;
            if turn > 0.2 {
                continue;
            }
            let what = format!("{key:?} {level:?} ({i}, {j})");
            assert!(t.is_regular(), "{what}");
            // The radius at the tile's centre.
            let pc = exact(&t).eval(&Q::from_signeds(1, 2), &Q::from_signeds(1, 2));
            let d: [Q; 3] = std::array::from_fn(|n| &pc[n] - &o[n]);
            let hgt = dot(&d, &a);
            let radial: [Q; 3] = std::array::from_fn(|n| &d[n] - &hgt * &a[n]);
            let len = nearest(&dot(&radial, &radial)).sqrt();
            let out: [f64; 3] = std::array::from_fn(|n| {
                nearest(&(&tm[1] * &radial[n])) / len - nearest(&(&tm[0] * &a[n]))
            });
            let beyond = normal_angle(&t, out, &what) - 2.0 * turn;
            worst = larger(worst, beyond);
            assert!(beyond < 1.0 / 32.0, "{what}: {beyond:e}");
        }
    }
    eprintln!("worst angle beyond the tile's turn: {worst:e} rad");
}

/// One exact form given as another point on the axis, the axis reversed
/// and the chain reversed has the same key and the same tiles, bit for
/// bit; a coaxial cylinder's lattice has the same frame.
#[test]
fn equal_keys_give_equal_tiles() {
    let mut rng = Rng::new(12);
    for _ in 0..6 {
        // Points and heights on a grid of `2⁻²⁰`, and an integer axis of
        // length 3: the moved point and `9 − z` are exact.
        let grid = |z: f64| (z * 1048576.0).round() / 1048576.0;
        let p = rng.point(1e3).to_array().map(grid);
        let a = [1.0, 2.0, 2.0];
        let kind = rng.next_u64() % 3;
        let spans: Vec<Span2> = off_axis(&mut rng, kind)
            .iter()
            .map(|s| s.map(|c| [c[0], grid(c[1]), c[2]]))
            .collect();
        let moved = [0, 1, 2].map(|c| p[c] + 3.0 * a[c]);
        // Heights from `moved` (9 along the axis) along `−a`: `9 − z`.
        let rev: Vec<Span2> = spans
            .iter()
            .rev()
            .map(|s| [s[2], s[1], s[0]].map(|c| [c[0], 9.0 - c[1], c[2]]))
            .collect();
        let k1 = RevolvedKey::new(p, a, &spans).unwrap();
        let k2 = RevolvedKey::new(moved, a.map(|x| -x), &rev).unwrap();
        assert_eq!(FormKey::from(k1.clone()), FormKey::from(k2.clone()));
        let (l1, l2) = (
            Lattice::new(k1.clone().into()).unwrap(),
            Lattice::new(k2.into()).unwrap(),
        );
        let rect = l1.half(-1).unwrap();
        let bits = |l: &Lattice| -> Vec<u64> {
            l.tiles(&rect)
                .unwrap()
                .iter()
                .flat_map(|t| {
                    t.net()
                        .as_flattened()
                        .iter()
                        .map(|x| x.to_bits())
                        .collect::<Vec<_>>()
                })
                .collect()
        };
        assert_eq!(bits(&l1), bits(&l2));
        let cyl = Lattice::new(CylinderKey::new(p, a, 1.0).unwrap().into()).unwrap();
        assert_eq!((cyl.origin(), cyl.axes()), (l1.origin(), l1.axes()));
    }
}

/// A horn torus `R = r`: its meridian's quarters reaching the axis are
/// tangent to it there, a cusp no tile holds; the quarters away from it
/// tile. A conic touching the axis inside a span is refused too; a span
/// crossing the axis or lying on it is no key at all.
#[test]
fn cusps_are_refused() {
    let r = 1.5;
    let circle = Conic::circle([r, 0.0], MERIDIAN, r).unwrap();
    let quarters: Vec<Span2> = (0..4).map(|k| circle.quarter::<3>(k).unwrap()).collect();
    let horn = RevolvedKey::new([1.0, 2.0, 3.0], [0.0, 1.0, 1.0], &quarters).unwrap();
    // In the key's direction, which span is which: those reaching ρ = 0.
    let spans = horn.profile().spans();
    let cusp: Vec<bool> = spans
        .iter()
        .map(|s| s[0][0] == 0.0 || s[2][0] == 0.0)
        .collect();
    assert_eq!(cusp.iter().filter(|c| **c).count(), 2);
    let l = Lattice::new(horn.clone().into()).unwrap();
    for (c, is_cusp) in cusp.iter().enumerate() {
        let c = c as i64;
        for k in -1..3 {
            match l.base_tile(k, c) {
                Err(e) => assert!(*is_cusp && e == LatticeError::Cusp, "{c}: {e:?}"),
                Ok(_) => assert!(!is_cusp, "{c}"),
            }
        }
        // Every level's tiles of the span: its sub-cells away from the
        // cusp point too, the whole span being refused.
        let rect = Rect {
            level: Level { u: 1, v: 3 },
            u: 0..4,
            v: c << 3..(c + 1) << 3,
        };
        assert_eq!(l.tiles(&rect).is_err(), *is_cusp);
        if *is_cusp {
            assert_eq!(l.tiles(&rect).unwrap_err(), LatticeError::Cusp);
        }
    }
    // A surface over the regular spans is made; over all of them refused.
    let regular: Vec<i64> = (0..4).filter(|c| !cusp[*c as usize]).collect();
    assert_eq!(regular.len(), 2);
    for c in regular {
        let s = revolved(horn.clone(), 0, c..c + 1).unwrap();
        assert_eq!(s.tiles().len(), 2);
    }
    assert_eq!(
        revolved(horn.clone(), 0, 0..3).unwrap_err(),
        LatticeError::Cusp
    );
    // A parabola-like span tangent to the axis at its middle: `N = (1 −
    // 2s)²`.
    let inner = [[1.0, 0.0, 1.0], [-1.0, 1.0, 1.0], [1.0, 2.0, 1.0]];
    let k = RevolvedKey::new([0.0; 3], Z, &[inner]).unwrap();
    let l = Lattice::new(k.into()).unwrap();
    assert_eq!(l.base_tile(0, 0).unwrap_err(), LatticeError::Cusp);
    // A span leaving the axis along it (its middle point on the axis).
    let along = [[0.0, 0.0, 1.0], [0.0, 1.0, 0.5], [2.0, 2.0, 1.0]];
    let k = RevolvedKey::new([0.0; 3], Z, &[along]).unwrap();
    let l = Lattice::new(k.into()).unwrap();
    assert_eq!(l.base_tile(0, 0).unwrap_err(), LatticeError::Cusp);
    // Crossing the axis, inside or past an end, or lying on it.
    for span in [
        [[1.0, 0.0, 1.0], [-2.0, 1.0, 1.0], [1.0, 2.0, 1.0]],
        [[0.0, 0.0, 1.0], [-1.0, 1.0, 1.0], [2.0, 2.0, 1.0]],
        [[-1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [2.0, 2.0, 1.0]],
        [[0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 2.0, 1.0]],
    ] {
        assert_eq!(
            RevolvedKey::new([0.0; 3], Z, &[span]).unwrap_err(),
            FormError::Axis,
            "{span:?}"
        );
    }
}

/// The span classes on the axis, exactly: transversal ends and spans
/// whose middle control point lies past the axis while the span keeps
/// off it are regular.
#[test]
fn axis_contact_classes() {
    use crate::geom::form::{AxisContact, axis_contact};
    let cases = [
        (
            [[1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [1.0, 2.0, 1.0]],
            AxisContact::Regular,
        ),
        (
            [[0.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 2.0, 1.0]],
            AxisContact::Regular,
        ),
        // `b = −1` below `a = c = 2`, `b² < ac`: it keeps off the axis.
        (
            [[2.0, 0.0, 1.0], [-1.0, 1.0, 1.0], [2.0, 2.0, 1.0]],
            AxisContact::Regular,
        ),
        // The same with weights: `a = 4, b = −2·1, c = 1`, `b² = ac`.
        (
            [[1.0, 0.0, 4.0], [-2.0, 1.0, 1.0], [1.0, 2.0, 1.0]],
            AxisContact::Cusp,
        ),
        (
            [[1.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 2.0, 1.0]],
            AxisContact::Cusp,
        ),
        (
            [[1.0, 0.0, 4.0], [-2.0, 1.0, 1.01], [1.0, 2.0, 1.0]],
            AxisContact::Crosses,
        ),
        (
            [[1.0, 0.0, 1.0], [-1.0, 1.0, 1.0], [0.0, 2.0, 1.0]],
            AxisContact::Crosses,
        ),
    ];
    for (span, want) in cases {
        assert_eq!(axis_contact(&span), want, "{span:?}");
    }
}

/// A closed profile (a whole ellipse off the axis) may not be taken
/// whole; joints whose weights differ are refused.
#[test]
fn closed_profiles_and_joints() {
    let e = Conic::ellipse([3.0, 1.0], MERIDIAN, [1.0, 2.0]).unwrap();
    let all: Vec<Span2> = (0..4).map(|k| e.quarter::<3>(k).unwrap()).collect();
    let key = RevolvedKey::new([0.0; 3], Z, &all).unwrap();
    let l = Lattice::new(key.clone().into()).unwrap();
    let half = l.half(0).unwrap();
    assert_eq!(l.tiles(&half).unwrap_err(), LatticeError::Span);
    assert_eq!(
        revolved(key.clone(), 0, 0..4).unwrap_err(),
        LatticeError::Span
    );
    assert_eq!(revolved(key, 1, 0..3).unwrap().tiles().len(), 6);
    // A full turn is refused, three quarters are not.
    let rect = |u: std::ops::Range<i64>| Rect {
        level: Level::BASE,
        u,
        v: 0..1,
    };
    assert_eq!(l.tiles(&rect(0..4)).unwrap_err(), LatticeError::Span);
    assert_eq!(l.tiles(&rect(-1..2)).unwrap().len(), 3);
    // Spans outside the profile.
    assert_eq!(l.base_tile(0, 4).unwrap_err(), LatticeError::Index);
    assert_eq!(l.base_tile(0, -1).unwrap_err(), LatticeError::Index);
    let mut heavy = all[..2].to_vec();
    heavy[1] = heavy[1].map(|p| [p[0], p[1], 2.0 * p[2]]);
    let key = RevolvedKey::new([0.0; 3], Z, &heavy).unwrap();
    assert_eq!(Lattice::new(key.into()).unwrap_err(), LatticeError::Joint);
}
