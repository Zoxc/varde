//! Cylinders' constructors against exact values and OCCT's nets: every
//! control point the exact value rounded once; a conic-chain cylinder's
//! tiles on its profile at every level, sharing rows across spans,
//! regular and following the tangent to the cap; joints whatever their
//! weights, closed profiles and the chord rate.

use glam::DVec3;
use malachite::Rational as Q;
use varde_decide::bezier::HomCurve;
use varde_decide::num::trig::{cos, sin};
use varde_testkit::rng::Rng;

use super::super::testing::{
    dot, exact, frame, level_to, local, normal_angle, rounded_once, rows_shared, scale,
};
use super::*;
use crate::geom::conic::Conic;
use crate::geom::curve::Span3;
use crate::geom::form::FormKey;
use crate::geom::round::{nearest, nearest_sqrt, q};

const TOL: f64 = 1e-15;

fn q0() -> Q {
    Q::from(0u32)
}

/// Circular cylinders about axes up to `1e6` out, radii `1e-3` to `1e5`.
fn cylinders(seed: u64) -> Vec<CylinderKey> {
    let mut rng = Rng::new(seed);
    let mut out = vec![CylinderKey::new([0.0; 3], [0.0, 0.0, 1.0], 1.0).unwrap()];
    for k in 0..6 {
        let p = rng.point([1.0, 1e3, 1e6][k % 3]).to_array();
        let r = rng.log_range(1e-3, 1e5);
        out.push(CylinderKey::new(p, rng.direction().to_array(), r).unwrap());
    }
    out
}

/// A conic's spans in a plane across `a` (roughly: the key projects
/// along `a`), `kind` 0 an ellipse's three quarters, 1 parabola arcs, 2
/// hyperbola arcs whose middle weights share a binade (so their joints'
/// weights agree).
fn conic_spans(rng: &mut Rng, a: DVec3, kind: u64) -> Vec<Span3> {
    let (e1, e2) = a.any_orthonormal_pair();
    // Tilt the conic's plane a little off the axis's normal plane.
    let e2 = (e2 + 0.25 * rng.unit() * a).to_array();
    let e1 = e1.to_array();
    let c = rng.point(1e3).to_array();
    let r = rng.log_range(1e-2, 1e3);
    match kind {
        0 => {
            let e = Conic::ellipse(c, [e1, e2], [r, r * rng.range(0.3, 1.0)]).unwrap();
            (0..3).map(|k| e.quarter::<4>(k).unwrap()).collect()
        }
        1 => {
            let p = Conic::parabola(c, [e1, e2], r).unwrap();
            let ts = [-1.5, -0.5, 0.25, 1.0];
            ts.windows(2)
                .map(|t| p.arc::<4>(t[0], t[1]).unwrap())
                .collect()
        }
        _ => {
            let h = Conic::hyperbola(c, [e1, e2], [r, 0.7 * r]).unwrap();
            let ss = [1.1, 1.3, 1.6, 1.9];
            ss.windows(2)
                .map(|s| h.arc::<4>(s[0], s[1]).unwrap())
                .collect()
        }
    }
}

/// Conic-chain cylinders along random axes over each kind of chain.
fn chains(seed: u64) -> Vec<ConicCylinderKey> {
    let mut rng = Rng::new(seed);
    (0..9)
        .map(|k| {
            let a = rng.direction();
            let spans = conic_spans(&mut rng, a, k % 3);
            ConicCylinderKey::new(a.to_array(), &spans).unwrap()
        })
        .collect()
}

#[test]
fn circular_tiles_are_exact_values_rounded_once() {
    let root = nearest_root_half();
    for key in cylinders(1) {
        let l = Lattice::new(key.into()).unwrap();
        let (o, [e1, e2, a]) = frame(&l);
        let rho = q(key.radius());
        let neg = |x: &[Q; 3]| x.clone().map(|c| -c);
        let radii = [e1.clone(), e2.clone(), neg(&e1), neg(&e2)];
        for k in -2..3i64 {
            for c in -1..2i64 {
                let (r0, r1) = (
                    &radii[k.rem_euclid(4) as usize],
                    &radii[(k + 1).rem_euclid(4) as usize],
                );
                let corner: [Q; 3] = std::array::from_fn(|i| &r0[i] + &r1[i]);
                let dirs = [(r0.clone(), 1.0), (corner, root), (r1.clone(), 1.0)];
                let want: [([Q; 3], f64); 9] = std::array::from_fn(|n| {
                    let (d, w) = &dirs[n / 3];
                    let t = Q::from(2 * c - 1 + (n % 3) as i64) * q(CELL) / Q::from(2u32);
                    (
                        std::array::from_fn(|i| &o[i] + &rho * &d[i] + &t * &a[i]),
                        *w,
                    )
                });
                rounded_once(
                    &l.base_tile(k, c).unwrap(),
                    &want,
                    &format!("{key:?} ({k}, {c})"),
                );
            }
        }
    }
}

/// `√½` rounded to the nearest `f64`.
fn nearest_root_half() -> f64 {
    crate::geom::round::nearest_sqrt(&Q::from_signeds(1, 2))
}

/// OCCT's `Convert_CylinderToBSplineSurface` (its `ComputePoles`, OCCT
/// commit b8f597c677811d1f9f4d8a97f5ae2825c0353a42) over the cell's turn
/// `[U₁, U₂]` and axial extent `[V₁, V₂]` in the key's frame: poles
/// `(R cos U, R sin U, V)` at the ends, `R cos(U + α)/cos α` (and `sin`)
/// at the middle, weights `1, cos α, 1` for `α` half the turn; degree 1
/// in `V`. The tile is that net, elevated to degree 2 in `V`, rounded.
#[test]
fn circular_tiles_are_occt_s_nets() {
    for key in cylinders(2) {
        let l = Lattice::new(key.into()).unwrap();
        let (o, [e1, e2, a2]) = (l.origin(), l.axes());
        let r = key.radius();
        for k in [-1i64, 0, 1, 2] {
            let tile = l.base_tile(k, 0).unwrap();
            let m = scale(&tile);
            let (u1, alpha) = (
                k as f64 * std::f64::consts::FRAC_PI_2,
                std::f64::consts::FRAC_PI_4,
            );
            let us = [u1, u1 + alpha, u1 + 2.0 * alpha];
            let vs = [-CELL / 2.0, 0.0, CELL / 2.0];
            for (i, u) in us.iter().enumerate() {
                let s = if i == 1 { 1.0 / cos(alpha) } else { 1.0 };
                let (x, y) = (r * cos(*u) * s, r * sin(*u) * s);
                let w = if i == 1 { cos(alpha) } else { 1.0 };
                for (j, v) in vs.iter().enumerate() {
                    let want: [f64; 3] =
                        std::array::from_fn(|c| o[c] + x * e1[c] + y * e2[c] + v * a2[c]);
                    let got = tile.net()[3 * i + j];
                    for c in 0..3 {
                        assert!(
                            (got[c] - want[c]).abs() <= 8.0 * TOL * m,
                            "{key:?} {k} ({i}, {j})"
                        );
                    }
                    assert!((got[3] - w).abs() <= f64::EPSILON, "{key:?} weight");
                }
            }
        }
    }
}

#[test]
fn the_canonical_cylinder_is_half_a_turn() {
    let key = CylinderKey::new([1.0, -2.0, 0.25], [1.0, 2.0, 2.0], 3.0).unwrap();
    for first in [-1, 0, 3] {
        let s = cylinder(key, first).unwrap();
        assert_eq!(
            (s.rect().u.clone(), s.rect().v.clone()),
            (first..first + 2, 0..1)
        );
        rows_shared(s.tiles(), s.rect());
        for (n, t) in s.tiles().iter().enumerate() {
            assert_eq!(*t, s.lattice().base_tile(first + n as i64, 0).unwrap());
            assert!(t.is_regular());
        }
    }
}

#[test]
fn chain_tiles_are_exact_values_rounded_once() {
    for key in chains(3) {
        let l = Lattice::new(key.clone().into()).unwrap();
        let (o, [e1, e2, a]) = frame(&l);
        assert_eq!(l.origin(), [0.0; 3]);
        for (k, span) in key.profile().spans().iter().enumerate() {
            for c in [-1i64, 0, 2] {
                // The span's standard weights: ends 1, the middle
                // `w₁/√(w₀w₂)` rounded once.
                let [w0, w1, w2] = span.map(|p| q(p[2]));
                let mid = nearest_sqrt(&(&w1 * &w1 / (w0 * w2)));
                let want: [([Q; 3], f64); 9] = std::array::from_fn(|n| {
                    let [x, y, _] = span[n / 3];
                    let w = [1.0, mid, 1.0][n / 3];
                    let t = Q::from(2 * c - 1 + (n % 3) as i64) * q(CELL) / Q::from(2u32);
                    let p =
                        std::array::from_fn(|i| &o[i] + q(x) * &e1[i] + q(y) * &e2[i] + &t * &a[i]);
                    (p, w)
                });
                let t = l.base_tile(k as i64, c).unwrap();
                rounded_once(&t, &want, &format!("{key:?} ({k}, {c})"));
            }
        }
    }
}

/// The exact point of a profile span at `s` and its tangent `N′W − NW′`
/// (a positive multiple of the derivative).
fn on_span(span: &Span2, s: &Q) -> ([Q; 2], [Q; 2]) {
    let c = HomCurve::<Q, 2>::from_affine(span.map(|p| ([p[0], p[1]], p[2])), &q0()).unwrap();
    let ([nx, ny], w) = c.eval_hom(s);
    let ([dx, dy], dw) = c.derivative().eval_hom(s);
    let p = [&nx / &w, &ny / &w];
    let t = [&dx * &w - &nx * &dw, &dy * &w - &ny * &dw];
    (p, t)
}

/// The tiles of every level visited lie on the form at their own
/// parameters: a tile's exact point at `(u, v)` is the profile's at the
/// span parameter and the axial distance there, to the rounding of the
/// coordinates (`TOL` of the base tile's scale).
#[test]
fn chain_tiles_follow_the_profile_at_every_level() {
    let mut rng = Rng::new(5);
    let mut worst: f64 = 0.0;
    let levels = [
        Level::BASE,
        Level { u: 1, v: 0 },
        Level { u: 3, v: 2 },
        Level { u: 8, v: 5 },
        Level { u: 20, v: 30 },
    ];
    for key in chains(4) {
        let l = Lattice::new(key.clone().into()).unwrap();
        let (_, [e1, e2, a]) = frame(&l);
        // The tiles' own spans: the profile's in standard form.
        let spans = l.profile().unwrap();
        for level in levels {
            for _ in 0..3 {
                let k = (rng.next_u64() % spans.len() as u64) as i64;
                let c = (rng.next_u64() % 3) as i64 - 1;
                // Within the cell's cap.
                let cell = Rect {
                    level: Level::BASE,
                    u: k..k + 1,
                    v: c..c + 1,
                };
                let cap = l.cap_of(&cell).unwrap();
                let level = Level {
                    u: level.u.min(cap.u),
                    v: level.v.min(cap.v),
                };
                let i = (k << level.u) + (rng.next_u64() % (1 << level.u)) as i64;
                let j = (c << level.v) + (rng.next_u64() % (1 << level.v)) as i64;
                let m = scale(&l.base_tile(k, c).unwrap());
                let e = exact(&l.tile(level, i, j).unwrap());
                for (u, v) in [(0, 0), (1, 3), (2, 2), (4, 1), (4, 4)] {
                    let (u, v) = (Q::from_signeds(u, 4), Q::from_signeds(v, 4));
                    let p = e.eval(&u, &v);
                    let s = local(i, level.u) + &u / Q::from(1u64 << level.u);
                    let t = (Q::from(c) - Q::from_signeds(1, 2)
                        + local(j, level.v)
                        + &v / Q::from(1u64 << level.v))
                        * q(CELL);
                    let ([x, y], _) = on_span(&spans[k as usize], &s);
                    let want: [Q; 3] =
                        std::array::from_fn(|n| &x * &e1[n] + &y * &e2[n] + &t * &a[n]);
                    let d: [Q; 3] = std::array::from_fn(|n| &p[n] - &want[n]);
                    let d = nearest(&dot(&d, &d)).sqrt() / m;
                    assert!(d <= TOL, "{key:?} {level:?} ({i}, {j}): {d:e}");
                    worst = if d > worst { d } else { worst };
                }
            }
        }
    }
    eprintln!("worst distance over the base tile's scale: {worst:e}");
}

#[test]
fn chain_rows_are_shared_across_spans_and_cells() {
    for key in chains(6) {
        let l = Lattice::new(key.clone().into()).unwrap();
        let n = key.profile().spans().len() as i64;
        for level in [Level::BASE, Level { u: 2, v: 1 }, Level { u: 5, v: 3 }] {
            let rect = Rect {
                level,
                u: 0..n << level.u,
                v: -(1 << level.v) - 1..(1 << level.v) + 1,
            };
            let tiles = l.tiles(&rect).unwrap();
            rows_shared(&tiles, &rect);
            let nv = (rect.v.end - rect.v.start) as usize;
            for (m, t) in tiles.iter().enumerate().step_by(5) {
                let (i, j) = (
                    rect.u.start + (m / nv) as i64,
                    rect.v.start + (m % nv) as i64,
                );
                assert_eq!(*t, l.tile(level, i, j).unwrap(), "({i}, {j}) {level:?}");
            }
        }
        // The canonical surface: every span by axial cell 0.
        let s = conic_chain(key.clone(), 0..n).unwrap();
        assert_eq!(*s.rect(), l.half(0).unwrap());
    }
}

#[test]
fn equal_chain_keys_give_equal_lattices() {
    let mut rng = Rng::new(7);
    for kind in 0..3 {
        let a = rng.direction();
        let spans = conic_spans(&mut rng, a, kind);
        // The chain reversed, shifted along the axis, the axis scaled
        // and reversed.
        let back: Vec<Span3> = spans
            .iter()
            .rev()
            .map(|s| {
                [s[2], s[1], s[0]].map(|p| {
                    let x = DVec3::new(p[0], p[1], p[2]) + 4.0 * a;
                    [x.x, x.y, x.z, p[3]]
                })
            })
            .collect();
        let k1 = ConicCylinderKey::new(a.to_array(), &spans).unwrap();
        let k2 = ConicCylinderKey::new((-2.0 * a).to_array(), &back).unwrap();
        if FormKey::from(k1.clone()) != FormKey::from(k2.clone()) {
            // The shift rounds the inputs differently; nothing to compare.
            continue;
        }
        let (l1, l2) = (
            Lattice::new(k1.into()).unwrap(),
            Lattice::new(k2.into()).unwrap(),
        );
        let r = l1.half(0).unwrap();
        assert_eq!(l1.tiles(&r).unwrap(), l2.tiles(&r).unwrap());
    }
}

#[test]
fn joints_share_their_row_whatever_their_weights() {
    // Hyperbola arcs whose middle weights fall in different binades: the
    // arc ending at `s = 1` has the end weight 2, the next starts with 1.
    let h = Conic::hyperbola([0.0; 3], [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], [1.0, 0.5]).unwrap();
    let ss = [0.6, 0.8, 1.0, 1.2];
    let spans: Vec<Span3> = ss
        .windows(2)
        .map(|s| h.arc::<4>(s[0], s[1]).unwrap())
        .collect();
    assert_ne!(spans[1][2][3], spans[2][0][3]);
    let key = ConicCylinderKey::new([0.0, 0.0, 1.0], &spans).unwrap();
    let s = conic_chain(key, 0..3).unwrap();
    rows_shared(s.tiles(), s.rect());
}

#[test]
fn a_closed_profile_takes_fewer_than_all_spans() {
    let e = Conic::ellipse(
        [0.5, -1.0, 0.0],
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        [2.0, 1.0],
    )
    .unwrap();
    let spans: Vec<Span3> = (0..4).map(|k| e.quarter::<4>(k).unwrap()).collect();
    let key = ConicCylinderKey::new([0.0, 0.0, 1.0], &spans).unwrap();
    let l = Lattice::new(key.clone().into()).unwrap();
    let rect = |level: Level, u: Range<i64>| Rect { level, u, v: 0..1 };
    assert_eq!(
        l.tiles(&rect(Level::BASE, 0..4)).unwrap_err(),
        LatticeError::Span
    );
    assert_eq!(l.tiles(&rect(Level::BASE, 0..3)).unwrap().len(), 3);
    let fine = Level { u: 1, v: 0 };
    assert_eq!(l.tiles(&rect(fine, 0..8)).unwrap_err(), LatticeError::Span);
    assert_eq!(l.tiles(&rect(fine, 1..8)).unwrap().len(), 7);
    // Spans outside the profile.
    assert_eq!(
        l.tiles(&rect(Level::BASE, 2..5)).unwrap_err(),
        LatticeError::Index
    );
    assert_eq!(
        l.tiles(&rect(Level::BASE, -1..1)).unwrap_err(),
        LatticeError::Index
    );
    assert_eq!(l.base_tile(4, 0).unwrap_err(), LatticeError::Index);
    assert_eq!(l.tile(fine, -1, 0).unwrap_err(), LatticeError::Index);
    assert_eq!(
        conic_chain(key.clone(), 0..4).unwrap_err(),
        LatticeError::Span
    );
    // The quarters' tiles meet their neighbours, the last the first.
    let s = conic_chain(key, 1..4).unwrap();
    rows_shared(s.tiles(), s.rect());
    let (first, last) = (l.base_tile(0, 0).unwrap(), l.base_tile(3, 0).unwrap());
    for r in 0..3 {
        assert_eq!(
            last.net()[6 + r].map(f64::to_bits),
            first.net()[r].map(f64::to_bits)
        );
    }
}

/// The chord of every sub-span of a level is at least the rate times
/// its parameter length, exactly.
#[test]
fn chord_rates_bound_every_sub_span() {
    let mut rng = Rng::new(8);
    let mut spans: Vec<Span2> = Vec::new();
    for key in chains(9) {
        spans.extend_from_slice(key.profile().spans());
    }
    // Random weights, and nearly straight and sharply turning spans.
    for _ in 0..20 {
        let p = [0, 1, 2].map(|_| {
            [
                rng.range(-3.0, 3.0),
                rng.range(-3.0, 3.0),
                rng.log_range(1.0 / 64.0, 64.0),
            ]
        });
        spans.push(p);
    }
    spans.push([[0.0, 0.0, 1.0], [1.0, 1e-9, 1.0], [2.0, 0.0, 1.0]]);
    spans.push([[0.0, 0.0, 1.0], [1.0, 1.0, 64.0], [0.0, 2e-6, 1.0 / 64.0]]);
    let mut positive = 0;
    for span in &spans {
        let rate = chord_rate(span);
        assert!(rate >= 0.0 && rate.is_finite());
        if rate == 0.0 {
            continue;
        }
        positive += 1;
        for level in [0u32, 1, 4, 13, 30] {
            for _ in 0..4 {
                let j = rng.next_u64() % (1u64 << level);
                let h = step(level);
                let s0 = Q::from(j) * &h;
                let s1 = &s0 + &h;
                let (p0, _) = on_span(span, &s0);
                let (p1, _) = on_span(span, &s1);
                let d = [&p1[0] - &p0[0], &p1[1] - &p0[1]];
                let chord2 = &d[0] * &d[0] + &d[1] * &d[1];
                let low = q(rate) * &h;
                assert!(chord2 >= &low * &low, "{span:?} level {level} sub-span {j}");
            }
        }
    }
    assert!(positive > 20, "{positive}");
    // No rate where the polygon turns back or a leg is zero.
    assert_eq!(
        chord_rate(&[[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]]),
        0.0
    );
    assert_eq!(
        chord_rate(&[[0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [1.0, 0.0, 1.0]]),
        0.0
    );
}

/// `2^−level`, exactly.
fn step(level: u32) -> Q {
    Q::from_signeds(1, 1i64 << level)
}

/// At every level to the cap, a tile in the cells holding the
/// coordinate box is regular and its normals lie within `2⁻⁵` rad of the
/// form's at its centre, beyond the turn of its profile sub-span; the
/// cap is no deeper than a circular cylinder's of the profile's size
/// would be by more than a few levels.
#[test]
fn chain_tiles_follow_the_tangent_to_the_cap() {
    let mut rng = Rng::new(10);
    let mut worst: f64 = 0.0;
    for key in chains(11) {
        let l = Lattice::new(key.clone().into()).unwrap();
        let cap = l.cap();
        assert!(cap.u >= 10 && cap.v >= 40, "{key:?} {cap:?}");
        let (_, [e1, e2, a]) = frame(&l);
        let spans = key.profile().spans();
        for n in 0..varde_testing::pick(6, 40) {
            let level = if n == 0 { cap } else { level_to(cap, &mut rng) };
            let k = (rng.next_u64() % spans.len() as u64) as i64;
            let i = (k << level.u) + (rng.next_u64() % (1 << level.u)) as i64;
            let j = (rng.next_u64() % (1 << level.v)) as i64;
            let t = l.tile(level, i, j).unwrap();
            let h = step(level.u);
            let s0 = local(i, level.u);
            let s1 = &s0 + &h;
            let mid = (&s0 + &s1) / Q::from(2u32);
            let dir = |s: &Q| -> [f64; 3] {
                let (_, [x, y]) = on_span(&spans[k as usize], s);
                std::array::from_fn(|c| nearest(&(&x * &e1[c] + &y * &e2[c])))
            };
            let angle = |p: [f64; 3], r: [f64; 3]| {
                let d = (p[0] * r[0] + p[1] * r[1] + p[2] * r[2])
                    / ((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt()
                        * (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt());
                varde_decide::num::trig::acos(d.clamp(-1.0, 1.0))
            };
            let turn = angle(dir(&s0), dir(&s1));
            if turn > 0.1 {
                continue;
            }
            let what = format!("{key:?} {level:?} ({i}, {j})");
            assert!(t.is_regular(), "{what}");
            // The form's normal: the tangent times the axis.
            let tg = dir(&mid);
            let ax = a.clone().map(|x| nearest(&x));
            let out = [
                tg[1] * ax[2] - tg[2] * ax[1],
                tg[2] * ax[0] - tg[0] * ax[2],
                tg[0] * ax[1] - tg[1] * ax[0],
            ];
            let beyond = normal_angle(&t, out, &what) - 2.0 * turn;
            worst = if beyond > worst { beyond } else { worst };
            assert!(beyond < 1.0 / 32.0, "{what}: {beyond:e}");
        }
    }
    eprintln!("worst angle beyond the tile's turn: {worst:e} rad");
}

/// OCCT's `Convert_ParabolaToBSplineCurve` (OCCT commit
/// b8f597c677811d1f9f4d8a97f5ae2825c0353a42): the arc `[U_F, U_L]` of
/// the parabola with parameter `p` (twice the focal length) has the
/// poles `(U², U)/(2p)`-style `(U_F²/2p, U_F)`, `(U_F U_L/2p, (U_F +
/// U_L)/2)`, `(U_L²/2p, U_L)` in its axis frame, weights 1. A conic-chain
/// cylinder over a parabola's arcs has those poles, projected along the
/// axis, as its tiles' row at axial distance 0.
#[test]
fn parabola_chain_rows_are_occt_s_poles() {
    let mut rng = Rng::new(12);
    for _ in 0..6 {
        let a = rng.direction();
        let (e1, e2) = a.any_orthonormal_pair();
        let vertex = rng.point(1e3).to_array();
        let f = rng.log_range(1e-2, 1e2);
        let par = Conic::parabola(vertex, [e1.to_array(), e2.to_array()], f).unwrap();
        let ts = [-1.0, 0.5, 2.0];
        let spans: Vec<Span3> = ts
            .windows(2)
            .map(|t| par.arc::<4>(t[0], t[1]).unwrap())
            .collect();
        let key = ConicCylinderKey::new(a.to_array(), &spans).unwrap();
        let l = Lattice::new(key.into()).unwrap();
        let p = 2.0 * f;
        let mut rows = Vec::new();
        for k in 0..2 {
            let t = l.base_tile(k, 0).unwrap();
            rows.push((scale(&t), [0, 1, 2].map(|i| t.net()[3 * i + 1])));
        }
        for w in ts.windows(2) {
            // OCCT's parameter is `U = 2f·t` along the tangent e₁.
            let (uf, ul) = (2.0 * f * w[0], 2.0 * f * w[1]);
            let poles = [
                (uf, uf * uf / (2.0 * p)),
                ((uf + ul) / 2.0, uf * ul / (2.0 * p)),
                (ul, ul * ul / (2.0 * p)),
            ];
            let world: Vec<DVec3> = poles
                .iter()
                .map(|(y, x)| {
                    let v = DVec3::from_array(vertex) + *y * e1 + *x * e2;
                    v - v.dot(a) * a
                })
                .collect();
            // The chain may run reversed in the key: find this arc's row.
            let close = |row: &[[f64; 4]; 3], m: f64, rev: bool| {
                (0..3).all(|i| {
                    let g = row[if rev { 2 - i } else { i }];
                    let d = DVec3::new(g[0], g[1], g[2]) - world[i];
                    d.length() <= 8.0 * TOL * (m + world[i].length()) && g[3] == 1.0
                })
            };
            assert!(
                rows.iter()
                    .any(|(m, r)| close(r, *m, false) || close(r, *m, true)),
                "{key:?}: the arc over {w:?}",
                key = l.key()
            );
        }
    }
}

#[test]
fn chain_tiles_away_from_turning_back_are_regular() {
    for key in chains(13) {
        let s = conic_chain(key.clone(), 0..key.profile().spans().len() as i64).unwrap();
        for t in s.tiles() {
            assert!(t.is_regular(), "{key:?}");
        }
    }
}

#[test]
fn chain_frames_are_orthonormal_to_rounding() {
    // The frame the nets are built in: e₁, e₂, a.
    for key in chains(14) {
        let (_, axes) = frame(&Lattice::new(key.into()).unwrap());
        for i in 0..3 {
            for j in 0..3 {
                let want = Q::from(u32::from(i == j));
                let d = nearest(&(dot(&axes[i], &axes[j]) - want)).abs();
                assert!(d < 4.0 * f64::EPSILON, "{i} {j} {d}");
            }
        }
    }
}
