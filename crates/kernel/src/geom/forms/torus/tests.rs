//! The torus's constructor against exact values and OCCT's net: every
//! control point the exact value rounded once (a spindle's irrational
//! ones against a 400-bit reference), every tile at every level on the
//! key's torus and, on a spindle, on its outer part and never on the
//! lemon; the apex rows the apexes at every level; a spindle's tube
//! levels equal arcs; regular off the apexes and following the tangent
//! to the cap.

use malachite::Rational as Q;
use malachite::base::rounding_modes::RoundingMode::Nearest;
use varde_decide::bezier::{HomPatch, Side, Sides};
use varde_decide::num::trig::{acos, atan2, cos, sin, tan};
use varde_testkit::rng::Rng;

use super::super::testing::{dot, exact, frame, level_to, local, normal_angle, rows_shared, scale};
use super::*;
use crate::geom::form::FormKey;
use crate::geom::lattice::Level;
use crate::geom::tile::{Tile, TileError};

const TOL: f64 = 1e-15;

/// The spindle torus of the milestone's exit, `R = 2`, `r = 3`.
fn lemon_spindle() -> TorusKey {
    TorusKey::new([0.0; 3], [0.0, 0.0, 1.0], 2.0, 3.0).unwrap()
}

/// Ring and spindle tori with centres up to `1e6` out, minor radii from
/// `1e-3` to `1e3`, rings from near the horn to thin, spindles from
/// near a sphere to `φ*` of 150°.
fn tori(seed: u64) -> Vec<TorusKey> {
    let mut rng = Rng::new(seed);
    let mut out = vec![
        TorusKey::new([0.0; 3], [0.0, 0.0, 1.0], 3.0, 1.0).unwrap(),
        lemon_spindle(),
        // `√(r² − R²) = 4`: a spindle whose coordinates are rational.
        TorusKey::new([0.5, -1.0, 2.0], [1.0, 2.0, 2.0], 3.0, 5.0).unwrap(),
    ];
    for k in 0..12 {
        let p = rng.point([1.0, 1e3, 1e6][k % 3]).to_array();
        let r = rng.log_range(1e-3, 1e3);
        let ratio = match k % 4 {
            0 => rng.range(1.2, 10.0),
            1 => 1.0 + rng.log_range(1e-3, 1e-1),
            2 => rng.range(0.05, 0.6),
            _ => rng.range(0.6, 0.86),
        };
        let key = TorusKey::new(p, rng.direction().to_array(), r * ratio, r).unwrap();
        out.push(key);
    }
    out
}

/// `√s` as a rational within `2⁻ᵖʳᵉᶜ` relative.
fn root(s: &Q, prec: u64) -> Q {
    let (f, _) = malachite::Float::sqrt_rational_prec_round_ref(s, prec, Nearest);
    Q::try_from(&f).unwrap()
}

/// `a + b·√d` rounded from a 400-bit root: an independent reference (a
/// double rounding would need the value within `2⁻³⁴⁰` ulp of a
/// midpoint, which the cases don't hit).
fn reference(a: &Q, b: &Q, d: &Q) -> f64 {
    if *b == 0u32 {
        return nearest(a);
    }
    nearest(&(a + b * root(d, 400)))
}

/// The exact net of cell `(k, l)` from the parametrisation (module
/// docs), each value rounded by [`reference`]: a spindle's corner `z` as
/// `r·(r + R)/√D`, not as the constructor forms it.
fn want_net(l: &Lattice, key: &TorusKey, k: i64, cell: i64) -> [[f64; 4]; 9] {
    let (o, [e1, e2, a]) = frame(l);
    let (big, small) = (q(key.major()), q(key.minor()));
    let d = &small * &small - &big * &big;
    let neg = |x: &[Q; 3]| x.clone().map(|c| -c);
    let radii = [e1.clone(), e2.clone(), neg(&e1), neg(&e2)];
    let r0 = &radii[k.rem_euclid(4) as usize];
    let r1 = &radii[(k + 1).rem_euclid(4) as usize];
    let corner: [Q; 3] = std::array::from_fn(|i| &r0[i] + &r1[i]);
    let half = Q::from_signeds(1, 2);
    let dirs = [
        (r0.clone(), Q::from(1u32)),
        (corner, half.clone()),
        (r1.clone(), Q::from(1u32)),
    ];
    let zero = Q::from(0u32);
    // (ρ, z, coefficient of √D in z, weight²) of the tube arc.
    let rows: [(Q, Q, Q, Q); 3] = if key.major() < key.minor() {
        let outer = &big + &small;
        let c = &small * &outer / &d;
        let w2 = (&small - &big) / (Q::from(2u32) * &small);
        let s = Q::from(if cell == 0 { 1 } else { -1 });
        let eq = (outer.clone(), zero.clone(), zero.clone(), Q::from(1u32));
        let mid = (outer.clone(), zero.clone(), &s * &c, w2);
        let apex = (zero.clone(), zero.clone(), s, Q::from(1u32));
        if cell == 0 {
            [eq, mid, apex]
        } else {
            [apex, mid, eq]
        }
    } else {
        let t: [(i64, i64); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];
        let (p0, p1) = (
            t[cell.rem_euclid(4) as usize],
            t[(cell + 1).rem_euclid(4) as usize],
        );
        let row = |(c, s): (i64, i64), w2: Q| {
            (
                &big + &small * Q::from(c),
                &small * Q::from(s),
                zero.clone(),
                w2,
            )
        };
        [
            row(p0, Q::from(1u32)),
            row((p0.0 + p1.0, p0.1 + p1.1), half.clone()),
            row(p1, Q::from(1u32)),
        ]
    };
    std::array::from_fn(|n| {
        let (dir, wu2) = &dirs[n / 3];
        let (rho, z, rz, wv2) = &rows[n % 3];
        let p: [f64; 3] = std::array::from_fn(|c| {
            let base = &o[c] + rho * &dir[c] + z * &a[c];
            reference(&base, &(rz * &a[c]), &d)
        });
        let w = reference(&zero, &Q::from(1u32), &(wu2 * wv2));
        [p[0], p[1], p[2], w]
    })
}

/// The tube cells of a key: a ring's four arcs, a spindle's two.
fn cells(key: &TorusKey) -> Vec<i64> {
    if spindle(key) {
        vec![-1, 0]
    } else {
        vec![-2, -1, 0, 1, 2, 5]
    }
}

#[test]
fn tiles_are_exact_values_rounded_once() {
    for key in tori(1) {
        let l = Lattice::new(key.into()).unwrap();
        assert_eq!(l.origin(), key.centre());
        assert_eq!(l.axes()[2], key.axis());
        for k in -2..3i64 {
            for cell in cells(&key) {
                let got = l.base_tile(k, cell).unwrap();
                let want = want_net(&l, &key, k, cell);
                for (n, (g, w)) in got.net().iter().zip(&want).enumerate() {
                    assert_eq!(
                        g.map(f64::to_bits),
                        w.map(f64::to_bits),
                        "{key:?} ({k}, {cell}) control point {n}"
                    );
                }
            }
        }
    }
}

/// The distance bound from `p` to the key's tube circle in `p`'s
/// meridian half-plane (a spindle's outer part, not its lemon):
/// `|(ρ − R)² + h² − r²|/r` (at least the distance, `(ρ − R)² + h² − r²
/// = (D − r)(D + r)` for `D` the distance from the tube's centre), `h`
/// along the axis and `ρ` from it, `ρ` from a 256-bit root.
fn apple_distance(key: &TorusKey, p: &[Q; 3]) -> f64 {
    let (c, a) = (key.centre().map(q), key.axis().map(q));
    let x: [Q; 3] = std::array::from_fn(|i| &p[i] - &c[i]);
    let ss = dot(&x, &x);
    let xa = dot(&x, &a);
    let h2 = &xa * &xa / dot(&a, &a);
    let rho2 = &ss - &h2;
    let rho = if rho2 > 0u32 {
        root(&rho2, 256)
    } else {
        Q::from(0u32)
    };
    let (big, small) = (q(key.major()), q(key.minor()));
    let num = &rho2 - Q::from(2u32) * &big * &rho + &big * &big + &h2 - &small * &small;
    nearest(&num).abs() / key.minor()
}

/// The signed radius of a spindle's point `p` at `u` on tile `i` of
/// `level` (its base cell's exact patch `base`, in tube cell `cell`):
/// `p`'s offset from the axis dotted with the outer equator's at the
/// same angle, over the latter's length. Positive on the outer part,
/// negative on the lemon (`p` across the axis).
fn signed_radius(
    key: &TorusKey,
    base: &HomPatch<Q>,
    cell: i64,
    (level, i): (Level, i64),
    p: &[Q; 3],
    u: &Q,
) -> f64 {
    let ub = local(i, level.u) + u / Q::from(1i64 << level.u);
    let veq = Q::from(if cell == 0 { 0u32 } else { 1u32 });
    let e = base.eval(&ub, &veq);
    let (c, a) = (key.centre().map(q), key.axis().map(q));
    let aa = dot(&a, &a);
    let perp = |x: &[Q; 3]| -> [Q; 3] {
        let r: [Q; 3] = std::array::from_fn(|n| &x[n] - &c[n]);
        let s = dot(&r, &a) / &aa;
        std::array::from_fn(|n| &r[n] - &s * &a[n])
    };
    let (pp, pe) = (perp(p), perp(&e));
    nearest(&(dot(&pp, &pe) / root(&dot(&pe, &pe), 128)))
}

#[test]
fn tiles_lie_on_the_torus_at_every_level() {
    let mut rng = Rng::new(2);
    let mut worst: f64 = 0.0;
    let levels = [
        Level::BASE,
        Level { u: 1, v: 0 },
        Level { u: 0, v: 1 },
        Level { u: 3, v: 2 },
        Level { u: 4, v: 4 },
        Level { u: 9, v: 12 },
    ];
    for key in tori(3) {
        let l = Lattice::new(key.into()).unwrap();
        for (k, cell) in [-3i64, 1].into_iter().flat_map(|k| {
            let c = cells(&key);
            [(k, c[0]), (k, c[c.len() / 2])]
        }) {
            let base = l.base_tile(k, cell).unwrap();
            let m = scale(&base);
            let base = exact(&base);
            for level in levels {
                let (nu, nv) = (1i64 << level.u, 1i64 << level.v);
                for (a, b) in [(0, 0), (nu - 1, nv - 1), (nu / 2, nv / 3)] {
                    let (i, j) = ((k << level.u) + a, (cell << level.v) + b);
                    let t = l.tile(level, i, j).unwrap();
                    let e = exact(&t);
                    let mut ts: Vec<Q> = [0.0, 0.25, 0.5, 0.75, 1.0].map(q).to_vec();
                    ts.push(q((rng.next_u64() % 1024) as f64 / 1024.0));
                    for u in &ts {
                        for v in &ts {
                            let p = e.eval(u, v);
                            let d = apple_distance(&key, &p) / m;
                            let what = format!("{key:?} ({k}, {cell}) {level:?}");
                            assert!(d <= TOL, "{what}: {d:e}");
                            worst = if d > worst { d } else { worst };
                            if spindle(&key) {
                                let s = signed_radius(&key, &base, cell, (level, i), &p, u);
                                assert!(s >= -TOL * m, "{what}: on the lemon, {s:e}");
                            }
                        }
                    }
                }
            }
        }
    }
    eprintln!("worst distance over the base tile's scale: {worst:e}");
}

/// The exit's spindle (`R = 2`, `r = 3`): its lattice has the outer
/// part's two tube cells only, at every level, so no surface reaches
/// the lemon; the canonical surface runs from apex to apex, every tile
/// on the outer part (its points' signed radius positive off the apexes)
/// and none on the lemon (whose points would be across the axis).
#[test]
fn the_spindle_domain_excludes_the_lemon() {
    let key = lemon_spindle();
    let l = Lattice::new(key.into()).unwrap();
    for cell in [-3, -2, 1, 2, i64::MAX, i64::MIN] {
        assert_eq!(
            l.base_tile(0, cell).unwrap_err(),
            LatticeError::Index,
            "{cell}"
        );
    }
    for level in [Level { u: 0, v: 1 }, Level { u: 2, v: 3 }] {
        let h = 1i64 << level.v;
        assert_eq!(l.tile(level, 0, h).unwrap_err(), LatticeError::Index);
        assert_eq!(l.tile(level, 0, -h - 1).unwrap_err(), LatticeError::Index);
        for v in [-h - 1..h, -h..h + 1] {
            let rect = Rect { level, u: 0..2, v };
            assert_eq!(l.tiles(&rect).unwrap_err(), LatticeError::Index);
        }
        let all = Rect {
            level,
            u: 0..2,
            v: -h..h,
        };
        assert_eq!(l.tiles(&all).unwrap().len() as i64, 4 * h, "{level:?}");
    }
    assert_eq!(
        torus_inner(key, 0).unwrap_err(),
        LatticeError::Index,
        "no inner half"
    );
    let s = torus(key, 0).unwrap();
    assert_eq!(*s.rect(), l.half(0).unwrap());
    let r = s.rect().clone();
    assert_eq!(
        (r.level, r.u.clone(), r.v.clone()),
        (Level { u: 0, v: 1 }, 0..2, -2..2)
    );
    rows_shared(s.tiles(), &r);
    // `√(r² − R²) = √5`: the apexes at `±√5` on the axis.
    let apex = nearest(&root(&Q::from(5u32), 400));
    let nv = (r.v.end - r.v.start) as usize;
    for (n, t) in s.tiles().iter().enumerate() {
        let j = r.v.start + (n % nv) as i64;
        if j == -2 {
            assert_eq!(t.point(1, 0), [0.0, 0.0, -apex]);
        }
        if j == 1 {
            assert_eq!(t.point(1, 2), [0.0, 0.0, apex]);
        }
    }
    let ts: Vec<Q> = (0..=8).map(|k| Q::from_signeds(k, 8)).collect();
    let (mut least, mut most): (f64, f64) = (f64::INFINITY, 0.0);
    for i in r.u.clone() {
        for j in r.v.clone() {
            let cell = j >> r.level.v;
            let base = exact(&l.base_tile(i >> r.level.u, cell).unwrap());
            let e = exact(&l.tile(r.level, i, j).unwrap());
            for u in &ts {
                for v in &ts {
                    let p = e.eval(u, v);
                    let s = signed_radius(&key, &base, cell, (r.level, i), &p, u);
                    assert!(s >= -1e-15, "({i}, {j}) at ({u}, {v}): {s:e}");
                    // Away from the apex rows the point is well off the
                    // axis, never across it.
                    let at_apex = (j == -2 && *v == 0u32) || (j == 1 && *v == 1u32);
                    if !at_apex {
                        least = if s < least { s } else { least };
                    }
                    most = if s > most { s } else { most };
                }
            }
        }
    }
    assert!(least > 0.05 && (most - 5.0).abs() < 1e-14, "{least} {most}");
}

#[test]
fn the_apex_rows_are_the_apexes_at_every_level() {
    for key in tori(4).into_iter().filter(spindle) {
        let l = Lattice::new(key.into()).unwrap();
        let (c, a) = (key.centre().map(q), key.axis().map(q));
        let (big, small) = (q(key.major()), q(key.minor()));
        let d = &small * &small - &big * &big;
        let apex = |s: i64| -> [f64; 3] {
            std::array::from_fn(|n| reference(&c[n], &(Q::from(s) * &a[n]), &d))
        };
        for level in [
            Level::BASE,
            Level { u: 3, v: 2 },
            Level { u: 8, v: 8 },
            Level { u: 20, v: 30 },
        ] {
            let cap = l.cap();
            let level = Level {
                u: level.u.min(cap.u),
                v: level.v.min(cap.v),
            };
            let h = 1i64 << level.v;
            for i in [0i64, 1, (3 << level.u) - 1, -5] {
                let north = l.tile(level, i, h - 1).unwrap();
                let south = l.tile(level, i, -h).unwrap();
                for k in 0..3 {
                    assert_eq!(north.point(k, 2), apex(1), "{key:?} {level:?} {i}");
                    assert_eq!(south.point(k, 0), apex(-1), "{key:?} {level:?} {i}");
                }
                // Singular sides, which the fold check divides out.
                assert_eq!(north.singular(), Side::V1.into());
                assert_eq!(south.singular(), Side::V0.into());
                // At the cap an angle cell next to an apex may turn
                // less than the coordinates' spacing there (the cap reads
                // the outer equator's radius): each control row across
                // the angle rounds to one point, the stored tile is no
                // surface, and its fold check stops. To `(8, 8)` the
                // tiles are regular.
                if level.u <= 8 {
                    assert!(
                        north.is_regular() && south.is_regular(),
                        "{key:?} {level:?} {i}"
                    );
                }
            }
        }
    }
}

/// A spindle's tube levels split its two halves at ½ in their
/// parameter, exactly: at level 1 the halves' middles, so four equal
/// arcs of `φ*/2`; below, node `t` of a half lies at `φ*/2 +
/// 2·atan((2t − 1)·tan(φ*/4))` from its end at the equator (a symmetric
/// arc's parametrisation), no cosine of the angles computed.
#[test]
fn spindle_tube_levels_split_the_halves_dyadically() {
    let axes = [[0.0, 0.0, 1.0], [1.0, 2.0, 2.0], [0.3, -0.7, 0.1]];
    for (n, ratio) in [0.1, 2.0 / 3.0, 0.8, 0.95].into_iter().enumerate() {
        let small = 1.5;
        let key = TorusKey::new([0.0; 3], axes[n % 3], ratio * small, small).unwrap();
        let l = Lattice::new(key.into()).unwrap();
        let [e1, _, a] = l.axes();
        let star = acos(-key.major() / key.minor());
        let quarter = tan(star / 4.0);
        for lv in 1..6u32 {
            let level = Level { u: 0, v: lv };
            let h = 1i64 << lv;
            for j in -h..=h {
                // Node `j`: tile `j`'s first row, or the last one's end.
                let (t, row) = if j < h {
                    (l.tile(level, 0, j).unwrap(), 0)
                } else {
                    (l.tile(level, 0, h - 1).unwrap(), 2)
                };
                let p = t.point(0, row);
                let rho: f64 = (0..3).map(|c| p[c] * e1[c]).sum();
                let z: f64 = (0..3).map(|c| p[c] * a[c]).sum();
                let phi = atan2(z, rho - key.major());
                // The half's parameter at the node, and its middle.
                let (t, mid) = if j < 0 {
                    ((j + h) as f64 / h as f64, -star / 2.0)
                } else {
                    (j as f64 / h as f64, star / 2.0)
                };
                let want = mid + 2.0 * atan2((2.0 * t - 1.0) * quarter, 1.0);
                assert!(
                    (phi - want).abs() < 1e-14,
                    "{key:?} level {lv} node {j}: {phi} vs {want}"
                );
                if lv == 1 {
                    let equal = j as f64 * star / 2.0;
                    assert!((phi - equal).abs() < 1e-14, "{key:?} node {j}");
                }
            }
        }
    }
}

/// OCCT's `Convert_TorusToBSplineSurface` (its `ComputePoles`, OCCT
/// commit b8f597c677811d1f9f4d8a97f5ae2825c0353a42) over the cell's turn
/// `[U₁, U₂]` and tube arc `[V₁, V₂]` (one span each, both under its
/// 150° limit): tube poles `(x, z) = (R + r·cos V, r·sin V)`, the middle
/// one `(R + r·cos(V₁ + α)/cos α, r·sin(V₁ + α)/cos α)`, swept as `(x
/// cos U, x sin U, z)`, the middle ones over `cos α` too, weights the
/// products of `1, cos α, 1`. The tile is that net in the key's frame.
#[test]
fn tiles_are_occt_s_nets() {
    for key in tori(5) {
        let l = Lattice::new(key.into()).unwrap();
        let (o, [e1, e2, a]) = (l.origin(), l.axes());
        let (big, small) = (key.major(), key.minor());
        let au = std::f64::consts::FRAC_PI_4;
        for k in [-1i64, 0, 2] {
            for cell in cells(&key) {
                let tile = l.base_tile(k, cell).unwrap();
                let m = scale(&tile);
                let (v1, v2) = if spindle(&key) {
                    let star = acos(-big / small);
                    if cell == 0 { (0.0, star) } else { (-star, 0.0) }
                } else {
                    let v1 = cell as f64 * std::f64::consts::FRAC_PI_2;
                    (v1, v1 + std::f64::consts::FRAC_PI_2)
                };
                let av = (v2 - v1) / 2.0;
                let u1 = k as f64 * std::f64::consts::FRAC_PI_2;
                for i in 0..3 {
                    let u = u1 + i as f64 * au;
                    let fu = if i == 1 { 1.0 / cos(au) } else { 1.0 };
                    for j in 0..3 {
                        let v = v1 + j as f64 * av;
                        let fv = if j == 1 { 1.0 / cos(av) } else { 1.0 };
                        let (x, z) = (big + small * cos(v) * fv, small * sin(v) * fv);
                        let (px, py) = (x * cos(u) * fu, x * sin(u) * fu);
                        let want: [f64; 3] =
                            std::array::from_fn(|c| o[c] + px * e1[c] + py * e2[c] + z * a[c]);
                        let got = tile.net()[3 * i + j];
                        for c in 0..3 {
                            let d = (got[c] - want[c]).abs();
                            assert!(
                                d <= 64.0 * TOL * m,
                                "{key:?} ({k}, {cell}) ({i}, {j}): {d:e}"
                            );
                        }
                        let w = (1.0 / fu) * (1.0 / fv);
                        assert!((got[3] - w).abs() <= 4.0 * f64::EPSILON, "{key:?} weight");
                    }
                }
            }
        }
    }
}

#[test]
fn rows_are_shared_and_cells_repeat_every_turn() {
    for key in tori(6) {
        let l = Lattice::new(key.into()).unwrap();
        for level in [Level::BASE, Level { u: 2, v: 1 }, Level { u: 4, v: 3 }] {
            let (hu, hv) = (1i64 << level.u, 1i64 << level.v);
            // A ring's tube from below its outer half to past its inner
            // equator; a spindle's whole outer part.
            let v = if spindle(&key) {
                -hv..hv
            } else {
                -hv..3 * hv - 1
            };
            let rect = Rect {
                level,
                u: -1..hu + 1,
                v,
            };
            let tiles = l.tiles(&rect).unwrap();
            rows_shared(&tiles, &rect);
            let nv = (rect.v.end - rect.v.start) as usize;
            for (n, t) in tiles.iter().enumerate().step_by(5) {
                let (i, j) = (
                    rect.u.start + (n / nv) as i64,
                    rect.v.start + (n % nv) as i64,
                );
                assert_eq!(*t, l.tile(level, i, j).unwrap(), "({i}, {j}) {level:?}");
                assert_eq!(*t, l.tile(level, i + (4 << level.u), j).unwrap());
                if !spindle(&key) {
                    assert_eq!(*t, l.tile(level, i, j - (4 << level.v)).unwrap());
                }
            }
        }
    }
}

/// A ring's two halves and their 180° turns close the torus: the four
/// canonical surfaces share their boundary rows bit for bit; no surface
/// takes a full turn of the tube.
#[test]
fn the_canonical_ring_is_half_a_turn_by_half_the_tube() {
    let key = TorusKey::new([1.0, -2.0, 0.25], [1.0, 2.0, 2.0], 3.0, 1.25).unwrap();
    let outer = torus(key, 0).unwrap();
    let inner = torus_inner(key, 0).unwrap();
    let back = torus(key, 2).unwrap();
    assert_eq!(
        (
            outer.rect().level,
            outer.rect().u.clone(),
            outer.rect().v.clone()
        ),
        (Level::BASE, 0..2, -1..1)
    );
    assert_eq!(
        (inner.rect().u.clone(), inner.rect().v.clone()),
        (0..2, 1..3)
    );
    assert_eq!(
        (back.rect().u.clone(), back.rect().v.clone()),
        (2..4, -1..1)
    );
    for s in [&outer, &inner, &back] {
        rows_shared(s.tiles(), s.rect());
    }
    let bits = |t: &Tile, n: usize| t.net()[n].map(f64::to_bits);
    // Tiles row-major in `u`, two by two: `(u, v)` at `2u + v`.
    for a in 0..2 {
        // The outer half's top row is the inner half's first.
        let (o, i) = (&outer.tiles()[2 * a + 1], &inner.tiles()[2 * a]);
        // Its bottom row the inner half's last (tube cell 3 is −1).
        let (ob, il) = (&outer.tiles()[2 * a], &inner.tiles()[2 * a + 1]);
        for r in 0..3 {
            assert_eq!(bits(o, 3 * r + 2), bits(i, 3 * r));
            assert_eq!(bits(ob, 3 * r), bits(il, 3 * r + 2));
        }
    }
    // The outer half's far column is the back half's first.
    for b in 0..2 {
        for r in 0..3 {
            assert_eq!(
                bits(&outer.tiles()[2 + b], 6 + r),
                bits(&back.tiles()[b], r)
            );
        }
    }
    let l = Lattice::new(key.into()).unwrap();
    let rect = |v: std::ops::Range<i64>| Rect {
        level: Level::BASE,
        u: 0..1,
        v,
    };
    assert_eq!(l.tiles(&rect(0..4)).unwrap_err(), LatticeError::Span);
    assert_eq!(l.tiles(&rect(-5..-1)).unwrap_err(), LatticeError::Span);
    assert_eq!(l.tiles(&rect(-1..2)).unwrap().len(), 3);
    let fine = Rect {
        level: Level { u: 0, v: 2 },
        u: 0..1,
        v: 0..16,
    };
    assert_eq!(l.tiles(&fine).unwrap_err(), LatticeError::Span);
}

/// A ring's tiles are regular at the base level, outer and inner; a
/// spindle's at its canonical level, those reaching an apex with it as a
/// singular side.
#[test]
fn regular_with_the_apexes() {
    for key in tori(7) {
        let l = Lattice::new(key.into()).unwrap();
        if spindle(&key) {
            let r = l.half(1).unwrap();
            let nv = (r.v.end - r.v.start) as usize;
            for (n, t) in l.tiles(&r).unwrap().iter().enumerate() {
                let j = r.v.start + (n % nv) as i64;
                let sides = if j == r.v.start {
                    Side::V0.into()
                } else if j == r.v.end - 1 {
                    Side::V1.into()
                } else {
                    Sides::NONE
                };
                assert_eq!(t.singular(), sides, "{key:?} tile {n}");
                assert!(t.is_regular(), "{key:?} tile {n}");
            }
        } else {
            for v in [-1i64, 1] {
                let rect = Rect {
                    level: Level::BASE,
                    u: -1..2,
                    v: v..v + 2,
                };
                for (n, t) in l.tiles(&rect).unwrap().iter().enumerate() {
                    assert!(t.is_regular(), "{key:?} tube cell {v} tile {n}");
                }
            }
        }
    }
}

/// At every level to the cap of its cells, a tile is regular, faces
/// out of the tube, and its normals lie within `2⁻⁵` rad of the torus's
/// at its centre beyond the turn it spans; a spindle's tiles where the
/// circle around the axis has half the outer equator's radius or more
/// (the cap reads the equator's).
#[test]
fn tiles_follow_the_tangent_to_the_cap() {
    let mut rng = Rng::new(8);
    let mut worst: f64 = 0.0;
    for key in tori(9) {
        let l = Lattice::new(key.into()).unwrap();
        let (big, small) = (key.major(), key.minor());
        let (c, a) = (key.centre(), key.axis());
        for n in 0..varde_testing::pick(8, 48) {
            // A tube cell: a ring's inner one every other time.
            let inner = !spindle(&key) && n % 4 >= 2;
            let cell = [-1, 0, 1, 2][n % 2 + 2 * usize::from(inner)];
            let base = Rect {
                level: Level::BASE,
                u: 0..1,
                v: cell..cell + 1,
            };
            let cap = l.cap_of(&base).unwrap();
            let level = if n == 0 { cap } else { level_to(cap, &mut rng) };
            let (hu, hv) = (1i64 << level.u, 1i64 << level.v);
            let i = (rng.next_u64() % (4 * hu) as u64) as i64 - 2 * hu;
            let j = (cell << level.v) + (rng.next_u64() % hv as u64) as i64;
            let tube = if spindle(&key) {
                acos(-big / small)
            } else {
                std::f64::consts::FRAC_PI_2
            };
            let turn = std::f64::consts::FRAC_PI_2 / hu as f64 + tube / hv as f64;
            if turn > 0.1 {
                continue;
            }
            let t = l.tile(level, i, j).unwrap();
            let half = Q::from_signeds(1, 2);
            let p = exact(&t).eval(&half, &half).map(|x| nearest(&x));
            let r: [f64; 3] = std::array::from_fn(|k| p[k] - c[k]);
            let h = r[0] * a[0] + r[1] * a[1] + r[2] * a[2];
            let rad: [f64; 3] = std::array::from_fn(|k| r[k] - h * a[k]);
            let len = (rad[0] * rad[0] + rad[1] * rad[1] + rad[2] * rad[2]).sqrt();
            if spindle(&key) && len < (big + small) / 2.0 {
                continue;
            }
            let what = format!("{key:?} {level:?} ({i}, {j})");
            assert!(t.is_regular(), "{what}");
            // Out of the tube: from the tube's centre circle to the point.
            let out: [f64; 3] = std::array::from_fn(|k| r[k] - big * rad[k] / len);
            let beyond = normal_angle(&t, out, &what) - 2.0 * turn;
            worst = if beyond > worst { beyond } else { worst };
            assert!(beyond < 1.0 / 32.0, "{what}: {beyond:e}");
        }
    }
    eprintln!("worst angle beyond the tile's turn: {worst:e} rad");
}

#[test]
fn caps_follow_the_geometry() {
    let t = |big: f64, small: f64| {
        Lattice::new(
            TorusKey::new([0.0; 3], [1.0, 2.0, 2.0], big, small)
                .unwrap()
                .into(),
        )
        .unwrap()
    };
    let cells = |v: i64| Rect {
        level: Level::BASE,
        u: 0..1,
        v: v..v + 1,
    };
    // A ring's outer half reads `R` around the axis, its inner half `R −
    // r`: a thin ring's inner cap is lower.
    let thin = t(1.001, 1.0);
    let (outer, inner) = (thin.cap(), thin.cap_of(&cells(1)).unwrap());
    assert_eq!(thin.cap_of(&cells(-1)).unwrap(), outer);
    assert!(
        inner.u + 9 <= outer.u && inner.v == outer.v,
        "{outer:?} {inner:?}"
    );
    // Coarsened to the inner cells' cap, never refused.
    let rect = Rect {
        level: outer,
        u: 0..1,
        v: 1 << outer.v..(1 << outer.v) + 1,
    };
    assert!(matches!(
        thin.tiles(&rect),
        Err(LatticeError::LevelBeyondCap { cap, .. }) if cap == inner
    ));
    assert_eq!(thin.capped(&rect).unwrap().level, inner);
    // Along the tube the minor radius; a spindle reads its outer equator
    // around the axis.
    let (ring, spin) = (t(3.0, 1.0).cap(), t(2.0, 3.0).cap());
    assert!(ring.u > ring.v && spin.u >= spin.v, "{ring:?} {spin:?}");
    // Far out, the coordinates' spacing grows: a lower cap.
    let far = Lattice::new(
        TorusKey::new([1e5, 0.0, 0.0], [1.0, 2.0, 2.0], 3.0, 1.0)
            .unwrap()
            .into(),
    )
    .unwrap()
    .cap();
    assert!(far.u + 9 <= ring.u && far.v + 9 <= ring.v, "{far:?}");
}

/// A spindle too close to the horn torus (`R/r` above `1 − 2⁻¹⁰`) has
/// middle weights below the tiles' range: refused by its tiles, its
/// lattice made.
#[test]
fn near_horn_spindles_are_refused_by_their_tiles() {
    let ok = TorusKey::new([0.0; 3], [0.0, 0.0, 1.0], 0.99, 1.0).unwrap();
    assert!(torus(ok, 0).is_ok());
    let key = TorusKey::new([0.0; 3], [0.0, 0.0, 1.0], 0.9995, 1.0).unwrap();
    let l = Lattice::new(key.into()).unwrap();
    assert!(matches!(
        l.base_tile(0, 0),
        Err(LatticeError::Tile(TileError::Weight { .. }))
    ));
    assert!(matches!(
        torus(key, 0),
        Err(LatticeError::Tile(TileError::Weight { .. }))
    ));
}

#[test]
fn equal_keys_give_equal_lattices() {
    // One torus from a scaled, reversed axis.
    let a = Lattice::new(
        TorusKey::new([1.0, -2.0, 0.25], [1.0, 2.0, 2.0], 2.0, 3.0)
            .unwrap()
            .into(),
    )
    .unwrap();
    let b = Lattice::new(
        TorusKey::new([1.0, -2.0, 0.25], [-0.5, -1.0, -1.0], 2.0, 3.0)
            .unwrap()
            .into(),
    )
    .unwrap();
    assert_eq!(a.key(), b.key());
    let r = a.half(-1).unwrap();
    assert_eq!(a.tiles(&r).unwrap(), b.tiles(&r).unwrap());
    let FormKey::Torus(k) = a.key() else {
        unreachable!()
    };
    assert!(spindle(k));
}
