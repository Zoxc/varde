//! Seam bounds against exact rational deviations: random p-curves on
//! planes, cylinders and spheres at several lattice levels (across and
//! along tile lines, weights out to `1/64` and `64` at every control
//! point, ends and knots included, near the poles, at far cells), curves with their own knots (one ulp off the p-curve's,
//! or extra ones) and spans of one ulp, each bound checked against the
//! deviation sampled densely and maximised locally.

use malachite::Rational as Q;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::Nearest;
use varde_decide::bezier::HomCurve;
use varde_testkit::rng::Rng;

use super::*;
use crate::geom::curve::{Curve3D, PCurve, Span2, Span3};
use crate::geom::form::{CylinderKey, FormKey, PlaneKey, SphereKey};
use crate::geom::lattice::{Lattice, Level, Rect};

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

/// The index of the tile holding the exact `x` in `range` (the last at
/// its end).
fn cell_of(x: &Q, range: &std::ops::Range<i64>) -> i64 {
    // Rounding is monotone and integers are `f64`s: never below the
    // floor, at most one above.
    let mut i = approx(x).floor() as i64;
    if *x < i {
        i -= 1;
    }
    i.clamp(range.start, range.end - 1)
}

/// `x` rounded to the nearest `f64`.
fn approx(x: &Q) -> f64 {
    f64::rounding_from(x, Nearest).0
}

/// The exact span of a chain holding `t` and its local parameter.
fn span_at(knots: &[f64], t: &Q) -> (usize, Q) {
    let k = knots[1..knots.len() - 1]
        .iter()
        .take_while(|&&k| q(k) <= *t)
        .count();
    let (a, z) = (q(knots[k]), q(knots[k + 1]));
    (k, (t - &a) / (z - a))
}

/// The exact `|S(p(t)) − C(t)|²`, the tile chosen from the exact UV.
fn deviation_sq(s: &Surface, c: &Curve3D, p: &PCurve, t: &Q) -> Q {
    let (kc, sc) = span_at(c.knots(), t);
    let (kp, sp) = span_at(p.knots(), t);
    let zero = Q::from(0u32);
    let cs = HomCurve::<Q, 3>::from_affine(
        c.spans()[kc].iter().map(|h| ([h[0], h[1], h[2]], h[3])),
        &zero,
    )
    .unwrap();
    let ps =
        HomCurve::<Q, 2>::from_affine(p.spans()[kp].iter().map(|h| ([h[0], h[1]], h[2])), &zero)
            .unwrap();
    let x = cs.eval(&sc);
    let [u, v] = ps.eval(&sp);
    let rect = s.rect();
    let (i, j) = (cell_of(&u, &rect.u), cell_of(&v, &rect.v));
    let y = s
        .tile(i, j)
        .unwrap()
        .patch_at(&Q::from(0u32))
        .eval(&(u - Q::from(i)), &(v - Q::from(j)));
    (0..3)
        .map(|k| {
            let d = &x[k] - &y[k];
            &d * &d
        })
        .fold(Q::from(0u32), |s, d| s + d)
}

/// The greatest deviation's square found on `[t0, t1]`: `n` samples per
/// piece between the cuts of both chains, then a golden-section search
/// about the best of each piece.
fn max_deviation_sq(s: &Surface, c: &Curve3D, p: &PCurve, n: u32) -> Q {
    let pk = p.knots();
    let (t0, t1) = (pk[0], pk[pk.len() - 1]);
    let mut cuts: Vec<f64> = pk
        .iter()
        .chain(c.knots().iter().filter(|&&t| t0 < t && t < t1))
        .copied()
        .collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let mut most = Q::from(0u32);
    for w in cuts.windows(2) {
        let (a, b) = (q(w[0]), q(w[1]));
        let at = |k: u32| &a + (&b - &a) * Q::from(k) / Q::from(n);
        let mut best = (0u32, Q::from(0u32));
        for k in 0..=n {
            let d = deviation_sq(s, c, p, &at(k));
            if d > best.1 {
                best = (k, d);
            }
        }
        // Golden section on the neighbouring samples.
        let (mut lo, mut hi) = (at(best.0.saturating_sub(1)), at((best.0 + 1).min(n)));
        let g = Q::from_signeds(618_034i64, 1_000_000);
        for _ in 0..24 {
            let m1 = &hi - (&hi - &lo) * &g;
            let m2 = &lo + (&hi - &lo) * &g;
            let (d1, d2) = (deviation_sq(s, c, p, &m1), deviation_sq(s, c, p, &m2));
            if d1 > best.1 {
                best.1 = d1.clone();
            }
            if d2 > best.1 {
                best.1 = d2.clone();
            }
            if d1 > d2 {
                hi = m2;
            } else {
                lo = m1;
            }
            // Keep the rationals short.
            lo = q(approx(&lo));
            hi = q(approx(&hi));
        }
        if best.1 > most {
            most = best.1;
        }
    }
    most
}

fn assert_holds(s: &Surface, c: &Curve3D, p: &PCurve, what: &str) -> Option<f64> {
    let b = match seam_bound(s, c, p) {
        Ok(b) => b,
        Err(_) => return None,
    };
    let d = max_deviation_sq(s, c, p, 12);
    assert!(
        q(b) * q(b) >= d,
        "{what}: bound {b} below the deviation {}",
        approx(&d).sqrt()
    );
    Some(b)
}

/// A random surface: a form, a level and a rectangle.
fn surface(rng: &mut Rng) -> Surface {
    let kind = rng.next_u64() % 3;
    let lu = (rng.next_u64() % 4) as u32;
    let lv = (rng.next_u64() % 4) as u32;
    let centre = rng.point(100.0).to_array();
    let axis = rng.direction().to_array();
    let r = rng.log_range(0.5, 200.0);
    let (key, rect): (FormKey, Rect) = match kind {
        0 => {
            // A fine level: tiles of 4·10⁶ / 2^L mm.
            let (lu, lv) = (lu * 8 + 10, lv * 8 + 10);
            let (i, j) = (
                (rng.next_u64() % 64) as i64 - 32,
                (rng.next_u64() % 64) as i64 - 32,
            );
            let k = PlaneKey::new(axis, centre).unwrap();
            (
                k.into(),
                Rect {
                    level: Level { u: lu, v: lv },
                    u: i..i + 3,
                    v: j..j + 3,
                },
            )
        }
        1 => {
            let k = CylinderKey::new(centre, axis, r).unwrap();
            // Angle cells anywhere round, axial cells near and far.
            let i = (rng.next_u64() % 16) as i64 - 8;
            let far = if rng.unit() < 0.3 { 5 } else { 0 };
            let j = (far << lv) - 1;
            (
                k.into(),
                Rect {
                    level: Level { u: lu, v: lv },
                    u: i..i + 3,
                    v: j..j + 3,
                },
            )
        }
        _ => {
            let k = SphereKey::new(centre, r, axis).unwrap();
            let i = (rng.next_u64() % 16) as i64 - 8;
            // Pole rows half the time.
            let m = 1i64 << lv;
            let j = if rng.unit() < 0.5 { -m } else { m - 3.min(m) };
            (
                k.into(),
                Rect {
                    level: Level { u: lu, v: lv },
                    u: i..i + 3,
                    v: j..(j + 3).min(m),
                },
            )
        }
    };
    Surface::new(Lattice::new(key).unwrap(), rect).unwrap()
}

/// A weight: 1, a power of two out to the range's ends, or any value in
/// the range.
fn weight(rng: &mut Rng) -> f64 {
    match rng.next_u64() % 4 {
        0 => 1.0,
        1 => [1.0 / 64.0, 64.0, 0.5, 2.0][(rng.next_u64() % 4) as usize],
        _ => rng.log_range(1.0 / 64.0, 64.0),
    }
}

/// A random p-curve on `s`: a walk of spans through neighbouring tiles,
/// each span in one tile, crossing tile lines at its ends, sometimes
/// running along one.
fn pcurve(rng: &mut Rng, s: &Surface) -> PCurve {
    let rect = s.rect().clone();
    let pick = |rng: &mut Rng, r: &std::ops::Range<i64>| {
        r.start + (rng.next_u64() % r.end.abs_diff(r.start)) as i64
    };
    let mut tile = (pick(rng, &rect.u), pick(rng, &rect.v));
    let inside = |rng: &mut Rng, (i, j): (i64, i64)| [i as f64 + rng.unit(), j as f64 + rng.unit()];
    let n = 1 + (rng.next_u64() % 3) as usize;
    let mut pts = vec![inside(rng, tile)];
    let mut tiles = Vec::new();
    let mut along = Vec::new();
    for _ in 0..n {
        let (i, j) = tile;
        let mut next = tile;
        let end = match rng.next_u64() % 4 {
            0 if rect.u.contains(&(i + 1)) => {
                next = (i + 1, j);
                [(i + 1) as f64, j as f64 + rng.unit()]
            }
            1 if rect.v.contains(&(j + 1)) => {
                next = (i, j + 1);
                [i as f64 + rng.unit(), (j + 1) as f64]
            }
            2 => {
                // Along the tile's lower `u` line, from a point on it.
                let last = pts.last_mut().unwrap();
                last[0] = i as f64;
                along.push(tiles.len());
                [i as f64, j as f64 + rng.unit()]
            }
            _ => inside(rng, tile),
        };
        tiles.push(tile);
        pts.push(end);
        tile = next;
    }
    // A point moved onto its tile's lower `u` line stays in the earlier
    // span's tile too: that tile is the same, or its neighbour below in
    // `v` or in `u`, whose closed square holds the line's part.
    let spans: Vec<Span2> = (0..n)
        .map(|k| {
            let (a, b) = (pts[k], pts[k + 1]);
            let (i, j) = tiles[k];
            let [wa, wm, wb] = [(); 3].map(|_| weight(rng));
            let m = if along.contains(&k) {
                [i as f64, j as f64 + rng.unit()]
            } else {
                inside(rng, (i, j))
            };
            [[a[0], a[1], wa], [m[0], m[1], wm], [b[0], b[1], wb]]
        })
        .collect();
    let mut knots = vec![rng.range(-2.0, 2.0)];
    for _ in 0..n {
        let step = if rng.unit() < 0.2 {
            rng.log_range(1e-12, 1e-6)
        } else {
            rng.range(0.25, 2.0)
        };
        let last = *knots.last().unwrap();
        knots.push(if rng.unit() < 0.3 {
            last.next_up()
        } else {
            last + step
        });
    }
    // Consecutive spans share their joining control point whatever its
    // weights on either side.
    PCurve::new(spans, knots).unwrap()
}

/// A space curve near `S(p(t))`: its own knots (the p-curve's, one ulp
/// off, with extra ones, or reaching past them), each span through
/// `S(p)` at its ends and middle, the middle moved by up to `wiggle`.
fn curve(rng: &mut Rng, s: &Surface, p: &PCurve, wiggle: f64) -> Curve3D {
    let pk = p.knots();
    let mut knots = pk.to_vec();
    match rng.next_u64() % 4 {
        0 => {
            // Each inner knot an ulp up or down, where that keeps them
            // increasing (a span of one ulp stays).
            for k in 1..pk.len() - 1 {
                let moved = if rng.unit() < 0.5 {
                    pk[k].next_up()
                } else {
                    pk[k].next_down()
                };
                if knots[k - 1] < moved && moved < pk[k + 1] {
                    knots[k] = moved;
                }
            }
        }
        1 => {
            let extra: Vec<f64> = pk
                .windows(2)
                .map(|w| w[0] + (w[1] - w[0]) * rng.range(0.1, 0.9))
                .filter(|&t| pk[0] < t && t < pk[pk.len() - 1])
                .collect();
            knots.extend(extra);
            knots.sort_by(f64::total_cmp);
            knots.dedup();
        }
        2 => {
            knots.insert(0, pk[0] - 1.0);
            knots.push(pk[pk.len() - 1] + 0.5);
        }
        _ => {}
    }
    let at = |t: f64| -> [f64; 3] {
        let t = t.clamp(pk[0], pk[pk.len() - 1]);
        let uv = p.eval(t).unwrap().map(|x| x.v);
        let rect = s.rect();
        let uv = [
            uv[0].clamp(rect.u.start as f64, rect.u.end as f64),
            uv[1].clamp(rect.v.start as f64, rect.v.end as f64),
        ];
        s.eval(uv).unwrap().map(|x| x.v)
    };
    let ends: Vec<[f64; 3]> = knots.iter().map(|&t| at(t)).collect();
    let spans: Vec<Span3> = knots
        .windows(2)
        .enumerate()
        .map(|(k, w)| {
            let (a, b) = (ends[k], ends[k + 1]);
            let mid = at(0.5 * (w[0] + w[1]));
            let [wa, wm, wb] = [(); 3].map(|_| weight(rng));
            let m: [f64; 3] = std::array::from_fn(|c| {
                2.0 * mid[c] - 0.5 * (a[c] + b[c]) + rng.range(-wiggle, wiggle)
            });
            [
                [a[0], a[1], a[2], wa],
                [m[0], m[1], m[2], wm],
                [b[0], b[1], b[2], wb],
            ]
        })
        .collect();
    Curve3D::new(spans, knots).unwrap()
}

#[test]
fn seam_bounds_hold_against_exact_deviations() {
    let mut certified = 0;
    let seeds = varde_testing::seeds(40, 2000);
    let total = seeds.end - seeds.start;
    for seed in seeds {
        let mut rng = Rng::new(seed);
        let s = surface(&mut rng);
        let p = pcurve(&mut rng, &s);
        let wiggle = [0.0, 1e-6, 1e-2][(rng.next_u64() % 3) as usize];
        let c = curve(&mut rng, &s, &p, wiggle);
        if assert_holds(&s, &c, &p, &format!("seed {seed}")).is_some() {
            certified += 1;
        }
    }
    assert!(
        certified * 4 >= total * 3,
        "{certified} of {total} certified"
    );
}

#[test]
fn seam_bounds_are_tight_on_a_constant_offset() {
    // On a plane the surface composed with a polynomial p-curve is the
    // p-curve's affine image: a curve through the images of its control
    // points, moved by `d`, is `d` away everywhere, and the bound (one
    // coordinate deviating, the denominator constant) is `|d|` plus the
    // rounding.
    for seed in varde_testing::seeds(8, 200) {
        let mut rng = Rng::new(seed);
        let level = (rng.next_u64() % 24) as u32;
        let key = PlaneKey::new(rng.direction().to_array(), rng.point(100.0).to_array()).unwrap();
        let rect = Rect {
            level: Level { u: level, v: level },
            u: -1..1,
            v: -1..1,
        };
        let s = Surface::new(Lattice::new(key.into()).unwrap(), rect).unwrap();
        let (i, j) = (
            (rng.next_u64() % 2) as i64 - 1,
            (rng.next_u64() % 2) as i64 - 1,
        );
        let n = 1 + (rng.next_u64() % 3) as usize;
        let mut pts = vec![[i as f64 + rng.unit(), j as f64 + rng.unit()]];
        for _ in 0..2 * n {
            pts.push([i as f64 + rng.unit(), j as f64 + rng.unit()]);
        }
        let spans: Vec<Span2> = (0..n)
            .map(|k| [0, 1, 2].map(|m| [pts[2 * k + m][0], pts[2 * k + m][1], 1.0]))
            .collect();
        let knots: Vec<f64> = (0..=n).map(|k| k as f64 * rng.range(0.5, 1.5)).collect();
        let knots = {
            let mut k = knots;
            k.sort_by(f64::total_cmp);
            k.dedup();
            k
        };
        if knots.len() != n + 1 {
            continue;
        }
        let p = PCurve::new(spans.clone(), knots.clone()).unwrap();
        // Up to a micrometre on tiles of 4 km to a quarter of a metre.
        let d = rng.log_range(1e-3, 1.0);
        let axis = (rng.next_u64() % 3) as usize;
        let image = |uv: [f64; 3]| {
            let mut x = s.eval([uv[0], uv[1]]).unwrap().map(|x| x.v);
            x[axis] += d;
            [x[0], x[1], x[2], 1.0]
        };
        let c = Curve3D::new(spans.iter().map(|sp| sp.map(image)).collect(), knots).unwrap();
        let b = assert_holds(&s, &c, &p, &format!("seed {seed}")).expect("certified");
        assert!(b <= 1.01 * d, "seed {seed}: {b} loose for {d}");
    }
}
