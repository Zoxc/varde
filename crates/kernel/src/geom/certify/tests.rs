//! Seam bounds against exact rational deviations: random p-curves of
//! every span kind on planes, cylinders and spheres at several lattice levels (across and
//! along tile lines, weights out to `1/64` and `64` at every control
//! point, ends and knots included, near the poles, at far cells), curves with their own knots (one ulp off the p-curve's,
//! or extra ones) and spans of one ulp, each bound checked against the
//! deviation sampled densely and maximised locally. Exact pairs (tile
//! rows with their iso lines on cylinders and spheres) are bounded by
//! their rounding. Weighted arcs on a
//! plane's 4 km tile escalate to their target, or bound a moved arc
//! within a small factor of its deviation; the charges are the same on
//! any number of threads, the budget running out at a precision step
//! names the coedge, and the top precision gives its achieved bound.

use malachite::Rational as Q;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::Nearest;
use varde_decide::bezier::HomCurve;
use varde_testkit::rng::Rng;

use super::calibration::{CellsError, arcs, base_plane, cells, on_plane, quarter};
use super::*;
use crate::geom::curve::{Curve3D, Span3};
use crate::geom::form::{CylinderKey, FormKey, PlaneKey, SphereKey};
use crate::geom::lattice::{Lattice, Level, Rect};
use crate::geom::pcurve::{PCurve, PSpan, Span2};
use crate::infra::budget::{Budget, Halt, Limit, MAX_WORK, Pass, Step, Work};
use crate::infra::error::{Cause, KernelError};
use varde_decide::bezier::Dir;

/// A seam target: a quarter of the least tolerance, `10⁻⁵` mm.
const TARGET: f64 = 2.5e-6;

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
    let x = cs.eval(&sc);
    let [u, v] = p
        .coords_at(kp, &zero)
        .unwrap()
        .map(|c| c.eval(&sp)[0].clone());
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

/// The bound towards `target` if it can be given, checked against the
/// deviation sampled; with that deviation.
fn assert_holds(
    s: &Surface,
    c: &Curve3D,
    p: &PCurve,
    target: f64,
    what: &str,
) -> Option<(SeamBound, f64)> {
    let b = seam_bound_alone(s, c, p, target, MAX_WORK).ok()?;
    let d = max_deviation_sq(s, c, p, 12);
    assert!(
        q(b.bound) * q(b.bound) >= d,
        "{what}: bound {} below the deviation {}",
        b.bound,
        approx(&d).sqrt()
    );
    Some((b, approx(&d).sqrt()))
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
    // Of each kind: Q; G, Möbius in `u` or `v`; linear; iso along the
    // tile line.
    let spans: Vec<PSpan> = (0..n)
        .map(|k| {
            let (a, b) = (pts[k], pts[k + 1]);
            let (i, j) = tiles[k];
            let [wa, wm, wb] = [(); 3].map(|_| weight(rng));
            let m = if along.contains(&k) {
                [i as f64, j as f64 + rng.unit()]
            } else {
                inside(rng, (i, j))
            };
            match rng.next_u64() % 5 {
                0 => PSpan::G {
                    mobius: Dir::U,
                    m: [[a[0], wa], [b[0], wb]],
                    q: [[a[1], weight(rng)], [m[1], wm], [b[1], weight(rng)]],
                },
                1 => PSpan::G {
                    mobius: Dir::V,
                    m: [[a[1], wa], [b[1], wb]],
                    q: [[a[0], weight(rng)], [m[0], wm], [b[0], weight(rng)]],
                },
                2 => PSpan::Linear([[a[0], a[1], wa], [b[0], b[1], wb]]),
                3 if along.contains(&k) => PSpan::Iso {
                    fixed: Dir::U,
                    at: i as f64,
                    m: [[a[1], wa], [b[1], wb]],
                },
                _ => PSpan::Q([[a[0], a[1], wa], [m[0], m[1], wm], [b[0], b[1], wb]]),
            }
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
    PCurve::from_spans(spans, knots).unwrap()
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
        let target = [TARGET, 1e-9][(rng.next_u64() % 2) as usize];
        let what = format!("seed {seed}");
        if let Some((b, seen)) = assert_holds(&s, &c, &p, target, &what) {
            certified += 1;
            // Above the target only where the deviation is (or a piece
            // reached the deepest split). A span of uneven speed can hide
            // its greatest deviation from a dozen samples: sample densely
            // before giving up.
            let seen = if b.bound <= target || b.bound <= 2.0 * seen {
                seen
            } else {
                approx(&max_deviation_sq(&s, &c, &p, 400)).sqrt()
            };
            assert!(
                b.bound <= target || b.bound <= 2.0 * seen,
                "{what}: {} for {seen}, target {target}",
                b.bound
            );
        }
    }
    assert_eq!(certified, total, "{certified} of {total} certified");
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
        let (b, _) = assert_holds(&s, &c, &p, TARGET, &format!("seed {seed}")).expect("certified");
        assert!(
            b.bound <= 1.01 * d,
            "seed {seed}: {} loose for {d}",
            b.bound
        );
    }
}

#[test]
fn weighted_arcs_escalate_to_their_target() {
    // The arcs lie exactly on the plane. In `f64` alone their bounds are
    // about 3·10⁻⁹, 6·10⁻⁵ and 10⁹ mm, growing with the weights'
    // spread; escalated, each meets the target, weights 1 and 1 at the
    // first level.
    let s = base_plane();
    for (w, first) in [(1.0, true), (3.0, false), (64.0, false)] {
        let c = quarter(w);
        let p = on_plane(&s, &c);
        for target in [TARGET, 1e-12] {
            let what = format!("weights {w}, 1/{w}, target {target}");
            let (b, _) = assert_holds(&s, &c, &p, target, &what).expect(&what);
            assert!(b.bound <= target, "{what}: {b:?}");
            if target == TARGET {
                assert_eq!(b.level == 0, first, "{what}: {b:?}");
            }
        }
    }
    // Weight-1 data, a line, stays at the first level.
    let c = Curve3D::new(
        vec![[
            [-30.0, 7.0, 0.0, 1.0],
            [10.0, 20.0, 0.0, 1.0],
            [50.0, 33.0, 0.0, 1.0],
        ]],
        vec![0.0, 1.0],
    )
    .unwrap();
    let (b, _) = assert_holds(&s, &c, &on_plane(&s, &c), TARGET, "line").unwrap();
    assert_eq!(b.level, 0, "{b:?}");
}

#[test]
fn moved_weighted_arcs_are_bounded_near_their_deviation() {
    // The arcs with end weights 3 and ⅓, 64 and 1/64, 1 and 1, their
    // p-curves' middle control point moved by `2·10⁻⁷` to `3·10⁻⁸` of the
    // 4 km tile each way: their deviations, half a millimetre to a tenth,
    // are out of reach of the target, so each bound settles within
    // `TIGHT` of its deviation.
    let s = base_plane();
    for (w, off) in [(3.0, 2e-7), (64.0, 1e-7), (1.0, 3e-8)] {
        let c = quarter(w);
        let p = on_plane(&s, &c).perturbed(&[[off, -off]]).unwrap();
        let what = format!("weights {w}, 1/{w}");
        let (b, seen) = assert_holds(&s, &c, &p, TARGET, &what).expect(&what);
        assert!(seen > 0.05, "{what}: {seen}");
        assert!(
            b.bound <= TIGHT * seen * 1.01,
            "{what}: {} for {seen}",
            b.bound
        );
    }
}

/// The seam bounds of `coedges` on `s` towards `target`, a pass on a
/// budget of `budget` units; the outcome and the units used.
fn pass(
    s: &Surface,
    coedges: &[(Curve3D, PCurve)],
    target: f64,
    budget: u64,
) -> (Result<Vec<SeamBound>, Halt<SeamError>>, u64) {
    let mut work = Work::new(&Budget::new(budget));
    let pass = Pass {
        stage: "seam",
        rung: 0,
    };
    let out = work.pass(pass, coedges, |(c, p), ledger| {
        seam_bound(s, c, p, target, ledger)
    });
    (out, work.used())
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn seam_bounds_are_charged_the_same_on_any_number_of_threads() {
    use crate::infra::par::assert_deterministic;
    let s = base_plane();
    let coedges = arcs(&s);
    let (full, used) = assert_deterministic(|| pass(&s, &coedges, TARGET, MAX_WORK));
    let bounds = full.unwrap();
    // Escalation costs: each arc alone, weights 1 the cheapest.
    let alone: Vec<u64> = coedges
        .iter()
        .map(|e| pass(&s, std::slice::from_ref(e), TARGET, MAX_WORK).1)
        .collect();
    assert_eq!(alone.iter().sum::<u64>(), used);
    assert!(alone[0] < alone[1] && alone[0] < alone[2], "{alone:?}");
    assert!(bounds[..3].iter().all(|b| b.bound <= TARGET), "{bounds:?}");
    // Budgets running out anywhere along the pass, the same outcome and
    // charge on any number of threads.
    for budget in [1, alone[0] + 2, alone[0] + alone[1] / 2, used - 1] {
        let (out, _) = assert_deterministic(|| pass(&s, &coedges, TARGET, budget));
        assert!(matches!(out, Err(Halt::OutOfBudget(_))), "{budget}");
    }
}

/// Each coedge's escalation, its ledger's steps in order (consecutive
/// steps at one level merged), is the same on any number of threads:
/// the weighted arcs climb the schedule level by level to their target,
/// the cells of one level charged at its row, the weight-1 arc at the
/// first level alone for the usual target.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn escalation_sequences_are_the_same_on_any_number_of_threads() {
    use crate::infra::par::assert_deterministic;
    let s = base_plane();
    let coedges = arcs(&s);
    for target in [TARGET, 1e-12] {
        let seqs = assert_deterministic(|| {
            let mut work = Work::new(&Budget::new(MAX_WORK));
            let pass = Pass {
                stage: "seam",
                rung: 0,
            };
            work.pass(pass, &coedges, |(c, p), ledger| {
                let b = seam_bound(&s, c, p, target, ledger)?;
                Ok::<_, Stop<SeamError>>((b.level, ledger.steps().to_vec()))
            })
            .expect("within the budget")
        });
        let level = |step: Step| match step {
            Step::Work => 0,
            Step::Precision { level } => level,
        };
        for (i, (top, steps)) in seqs.iter().enumerate() {
            // A level is reached only from the one below it, and the
            // highest charged is the bound's.
            let levels: Vec<u8> = steps.iter().map(|&(step, _)| level(step)).collect();
            let mut seen = 0u8;
            for &l in &levels {
                assert!(l <= seen + 1, "coedge {i}, target {target}: {levels:?}");
                seen = seen.max(l);
            }
            assert_eq!(seen, *top, "coedge {i}, target {target}: {levels:?}");
            for &(step, units) in steps {
                assert_eq!(units % seam_cell_units(level(step)), 0, "{step:?}");
            }
        }
        let tops: Vec<u8> = seqs.iter().map(|(top, _)| *top).collect();
        if target == TARGET {
            assert_eq!(tops[0], 0, "the weight-1 arc at the first level");
        } else {
            assert!(tops.contains(&2), "past double-double: {tops:?}");
        }
        assert!(tops[1..3].iter().all(|&top| top > 0), "{tops:?}");
    }
}

/// The cause and profile of running out.
fn ran_out(out: Result<Vec<SeamBound>, Halt<SeamError>>) -> (Cause, crate::BudgetProfile) {
    match out {
        Err(Halt::OutOfBudget(KernelError::OutOfBudget { cause, profile })) => (cause, *profile),
        other => panic!("{other:?}"),
    }
}

#[test]
fn targets_out_of_reach_run_out_of_precision_at_the_coedge() {
    let s = base_plane();
    let coedges = arcs(&s);
    // The budget runs out at the arc with weights 64 and 1/64 as it
    // escalates: past its first cell at the first level, in its first
    // double-double step.
    let first = pass(&s, &coedges[..2], TARGET, MAX_WORK).1;
    let budget = first + seam_cell_units(0) + 2;
    let (cause, profile) = ran_out(pass(&s, &coedges[..3], TARGET, budget).0);
    assert_eq!(cause, Cause::Precision);
    assert_eq!(profile.limit, Limit::Budget(budget));
    assert_eq!(profile.stopped.at.item, 2);
    assert_eq!(profile.stopped.step, Some(Step::Precision { level: 1 }));
}

#[test]
fn the_top_precision_gives_its_achieved_bound() {
    // A line along the base plane, its curve cut at `t = 1` of `[0, 3]`:
    // the p-curve's piece ends at `⅓` of its span, rounded at every
    // level, so no precision certifies the zero deviation exactly. A
    // seam is an accuracy item: at the top level its certified bound is
    // the achieved one, above the target; the operation warns.
    let s = base_plane();
    let x = |t: f64| [t, 0.0, 0.0, 1.0];
    let c = Curve3D::new(
        vec![[x(0.0), x(0.5), x(1.0)], [x(1.0), x(2.0), x(3.0)]],
        vec![0.0, 1.0, 3.0],
    )
    .unwrap();
    let line = on_plane(
        &s,
        &Curve3D::new(vec![[x(0.0), x(1.5), x(3.0)]], vec![0.0, 3.0]).unwrap(),
    );
    let coedges = vec![arcs(&s)[3].clone(), (c, line)];
    let (out, used) = pass(&s, &coedges, 0.0, MAX_WORK);
    let b = out.unwrap()[1];
    assert_eq!(b.level, LEVELS - 1, "{b:?}");
    // A unit short, the budget runs out at the top level's last cell.
    let (cause, profile) = ran_out(pass(&s, &coedges, 0.0, used - 1).0);
    assert_eq!(cause, Cause::Precision);
    let top = Step::Precision { level: LEVELS - 1 };
    assert_eq!(profile.stopped.step, Some(top), "{profile:?}");
    assert!(0.0 < b.bound && b.bound <= 1e-200, "{b:?}");
    // Within reach of the top level's rounding, it is met.
    let (out, _) = pass(&s, &coedges, 1e-200, MAX_WORK);
    assert!(out.unwrap()[1].bound <= 1e-200);
}

#[test]
fn targets_below_zero_or_nan_count_as_zero() {
    // Never met, a target that compares false with every bound would
    // refine a deviation out of reach until the budget runs out; it is
    // met as a target of 0 is, within `TIGHT` of the deviation.
    let s = base_plane();
    let c = quarter(3.0);
    let p = on_plane(&s, &c).perturbed(&[[2e-7, -2e-7]]).unwrap();
    let zero = seam_bound_alone(&s, &c, &p, 0.0, MAX_WORK).unwrap();
    for target in [f64::NAN, -1.0, f64::NEG_INFINITY] {
        let b = seam_bound_alone(&s, &c, &p, target, MAX_WORK);
        assert_eq!(b, Ok(zero), "{target}");
    }
}

/// Budgets at the exact end of each coedge's charges and a unit short of
/// it, on any number of threads: the whole charge passes; at the end of
/// a coedge the budget runs out at the next one's first cell, ordinary
/// work at the first level; a unit short it runs out at that coedge's
/// last cell, precision when that cell was above the first level. The
/// work used is the budget, split by step kind as the ledger charged it.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn budgets_at_each_coedge_s_end_run_out_at_the_next() {
    use crate::infra::par::assert_deterministic;
    let s = base_plane();
    let coedges = arcs(&s);
    let alone: Vec<u64> = coedges
        .iter()
        .map(|e| pass(&s, std::slice::from_ref(e), TARGET, MAX_WORK).1)
        .collect();
    let mut end = 0;
    for (k, &units) in alone.iter().enumerate() {
        // A unit short of this coedge's end.
        end += units;
        let (out, used) = assert_deterministic(|| pass(&s, &coedges, TARGET, end - 1));
        assert_eq!(used, end - 1);
        let (cause, profile) = ran_out(out);
        assert_eq!(profile.stopped.at.item, k);
        let step = profile.stopped.step.expect("charged");
        let want = match step {
            Step::Work => Cause::Work,
            Step::Precision { .. } => Cause::Precision,
        };
        assert_eq!(cause, want, "{profile:?}");
        assert_eq!(profile.used, end - 1);
        assert_eq!(
            profile.work + profile.precision.iter().sum::<u64>(),
            end - 1
        );
        // At its end: the next coedge's first cell, or all of them.
        let (out, used) = assert_deterministic(|| pass(&s, &coedges, TARGET, end));
        if k + 1 == coedges.len() {
            assert_eq!(used, end);
            assert!(out.is_ok());
        } else {
            let (cause, profile) = ran_out(out);
            assert_eq!(cause, Cause::Work);
            assert_eq!(profile.stopped.at.item, k + 1);
            assert_eq!(profile.stopped.step, Some(Step::Work));
            assert_eq!(profile.used, end);
        }
    }
    // The arc with weights 64 and 1/64 ends above the first level.
    let (_, p) = ran_out(pass(&s, &coedges, TARGET, alone[..3].iter().sum::<u64>() - 1).0);
    assert!(
        matches!(p.stopped.step, Some(Step::Precision { .. })),
        "{p:?}"
    );
}

/// A pass of copies of the arcs towards a target of 0, none of which it
/// meets, uses up its budget the same way on any number of threads: all
/// of it, running out at the same coedge and step.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_pass_past_its_budget_uses_it_up_on_any_number_of_threads() {
    use crate::infra::par::assert_deterministic;
    let s = base_plane();
    let coedges: Vec<_> = arcs(&s).into_iter().cycle().take(64).collect();
    for budget in [1, seam_cell_units(0), seam_cell_units(0) + 1, 100_000] {
        let (out, used) = assert_deterministic(|| pass(&s, &coedges, 0.0, budget));
        assert_eq!(used, budget);
        let (_, profile) = ran_out(out);
        assert_eq!(profile.used, budget);
        assert_eq!(profile.limit, Limit::Budget(budget));
    }
}

/// Seam passes over copies of the arcs on budgets running out anywhere
/// along them, at the first level and above, and the whole: the same bounds, or the
/// same error, cause and profile, with the coedges after the deciding one
/// stopped early or run on.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn seam_passes_are_the_same_without_early_stop() {
    use crate::infra::budget::assert_early_stop_changes_nothing;
    let s = base_plane();
    let coedges: Vec<_> = arcs(&s).into_iter().cycle().take(8).collect();
    let (out, used) = pass(&s, &coedges, TARGET, MAX_WORK);
    assert!(out.is_ok(), "{out:?}");
    for k in 0..=12 {
        let (out, _) =
            assert_early_stop_changes_nothing(|| pass(&s, &coedges, TARGET, used * k / 12));
        assert_eq!(out.is_ok(), k == 12, "{k}");
    }
}

#[test]
fn calibration_cells_refuse_a_level_or_depth_the_seam_bound_lacks() {
    let s = base_plane();
    let (c, p) = &arcs(&s)[0];
    assert_eq!(cells(&s, c, p, LEVELS - 1, 2), Ok(4));
    assert_eq!(cells(&s, c, p, LEVELS, 0), Err(CellsError::Level(LEVELS)));
    assert_eq!(cells(&s, c, p, u8::MAX, 0), Err(CellsError::Level(u8::MAX)));
    let deep = MAX_SUBDIV_DEPTH + 1;
    assert_eq!(cells(&s, c, p, 0, deep), Err(CellsError::Depth(deep)));
    assert_eq!(cells(&s, c, p, 0, 64), Err(CellsError::Depth(64)));
    // A p-curve outside the curve's knots has no seam bound.
    let short = Curve3D::new(c.spans().to_vec(), vec![0.0, 0.5]).unwrap();
    assert_eq!(
        cells(&s, &short, p, 0, 0),
        Err(CellsError::Seam(SeamError::Range))
    );
}

/// The seam passes' escalations as text: per target, each coedge's
/// bound and its ledger's steps (level, units) in order, and the units
/// used; then budgets running out along the pass (the cause, the coedge,
/// the step and the profile).
fn escalations(s: &Surface, coedges: &[(Curve3D, PCurve)]) -> String {
    let mut text = String::new();
    for target in [TARGET, 1e-12, 0.0] {
        let mut work = Work::new(&Budget::new(MAX_WORK));
        let pass_at = Pass {
            stage: "seam",
            rung: 0,
        };
        let seqs = work.pass(pass_at, coedges, |(c, p), ledger| {
            let b = seam_bound(s, c, p, target, ledger)?;
            Ok::<_, Stop<SeamError>>((b, ledger.steps().to_vec()))
        });
        let used = work.used();
        text += &format!("{target:e}: {seqs:?} {used}\n");
        for budget in [1, seam_cell_units(0) + 1, used / 3, used / 2, used - 1] {
            let (out, spent) = pass(s, coedges, target, budget);
            text += &format!("{budget}: {out:?} {spent}\n");
        }
    }
    text
}

/// The seam passes' escalations and charges ([`escalations`]) as bits:
/// the same on every platform and thread count. Natively on 1 and 8
/// threads and backwards ([`assert_deterministic`]), and `tools/ci.sh`
/// runs it on one thread and on wasm (`wasm32-wasip1`). A change of the
/// seam bound, the schedule or the unit table changes the hash, which
/// then needs updating here.
///
/// [`assert_deterministic`]: crate::infra::par::assert_deterministic
#[test]
fn seam_escalations_are_the_same_bits_everywhere() {
    let s = base_plane();
    let coedges = arcs(&s);
    #[cfg(not(target_arch = "wasm32"))]
    let text = crate::infra::par::assert_deterministic(|| escalations(&s, &coedges));
    #[cfg(target_arch = "wasm32")]
    let text = escalations(&s, &coedges);
    let h = text.bytes().fold(0xCBF2_9CE4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3)
    });
    println!("hash of the escalations: {h:#018x}");
    assert_eq!(
        h, 0xff34_6de7_6b3d_84ae,
        "escalations differ from the reference bits"
    );
}

/// The surfaces of the exact pairs' test: cylinders, cones and spheres
/// at lattice levels 0 to 3 in random frames.
fn exact_surfaces(rng: &mut Rng) -> Vec<Surface> {
    let centre = rng.point(100.0).to_array();
    let axis = rng.direction().to_array();
    let r = rng.log_range(0.5, 200.0);
    let mut out = Vec::new();
    for l in 0..4u32 {
        let m = 1i64 << l;
        let k = CylinderKey::new(centre, axis, r).unwrap();
        let rect = Rect {
            level: Level { u: l, v: l },
            u: -m..m,
            v: -1..2,
        };
        out.push(Surface::new(Lattice::new(k.into()).unwrap(), rect).unwrap());
        let k = SphereKey::new(centre, r, axis).unwrap();
        let rect = Rect {
            level: Level { u: l, v: l },
            u: -m..m,
            v: -m..m,
        };
        out.push(Surface::new(Lattice::new(k.into()).unwrap(), rect).unwrap());
    }
    out
}

#[test]
fn exact_iso_seams_are_bounded_by_their_rounding() {
    // An exact pair: a tile line's row as the edge's curve, the iso line
    // along it as the p-curve, the same `t` (Möbius weights 1). The
    // surface along the p-curve is the row itself, so the deviation is
    // zero and the bound is the rounding of the composition alone: at
    // the first level, for the usual target, within `2¹⁰` units of the
    // last bit of the tile's coordinates (the rounding of each step
    // carried through the degree-4 numerator, about a hundred units at
    // most here); towards a target of 0, the top level's.
    let mut worst: f64 = 0.0;
    for seed in varde_testing::seeds(3, 30) {
        let mut rng = Rng::new(seed);
        for s in exact_surfaces(&mut rng) {
            let rect = s.rect().clone();
            for (along, lines, cells) in [
                (Dir::U, rect.v.start..rect.v.end + 1, rect.u.clone()),
                (Dir::V, rect.u.start..rect.u.end + 1, rect.v.clone()),
            ] {
                for at in lines {
                    for cell in cells.clone() {
                        let row = s.row(along, at, cell).unwrap();
                        let Ok(c) = Curve3D::new(vec![row], vec![0.0, 1.0]) else {
                            continue;
                        };
                        let fixed = match along {
                            Dir::U => Dir::V,
                            Dir::V => Dir::U,
                        };
                        let span = PSpan::Iso {
                            fixed,
                            at: at as f64,
                            m: [[cell as f64, 1.0], [(cell + 1) as f64, 1.0]],
                        };
                        let p = PCurve::from_spans(vec![span], vec![0.0, 1.0]).unwrap();
                        let what = format!("seed {seed}, {along:?} line {at}, cell {cell}");
                        // The tile the row bounds, and its coordinates' scale.
                        let (i, j) = match along {
                            Dir::U => (cell, at.min(rect.v.end - 1)),
                            Dir::V => (at.min(rect.u.end - 1), cell),
                        };
                        let scale = s.tile(i, j).unwrap().net().iter().fold(1.0f64, |m, h| {
                            h[..3]
                                .iter()
                                .fold(m, |m, x| if x.abs() > m { x.abs() } else { m })
                        });
                        let b = seam_bound_alone(&s, &c, &p, TARGET, MAX_WORK).expect(&what);
                        assert_eq!(b.level, 0, "{what}: {b:?}");
                        let ulps = b.bound / (scale * f64::EPSILON);
                        assert!(ulps <= 1024.0, "{what}: {b:?}, {ulps} ulps");
                        if ulps > worst {
                            worst = ulps;
                        }
                        let top = seam_bound_alone(&s, &c, &p, 0.0, MAX_WORK).expect(&what);
                        assert!(top.bound <= 1e-200 * scale, "{what}: {top:?}");
                    }
                }
            }
        }
    }
    println!("worst first-level bound: {worst} units of the last bit");
}
