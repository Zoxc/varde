//! Trim windings against the windings of densely sampled polygons, on
//! random closed loops of every span kind split into coedges run either
//! way, at points clear of the loops; points on and next to trims come
//! out near. Extrema against known values (ends, an interior minimum, the
//! sphere's reach in every direction) and dense samples, escalating where
//! the first level's rounding is too coarse for the resolution. Charges
//! are the same on every run, and a budget short of them runs out.

use varde_decide::bezier::Dir;
use varde_decide::num::trig;
use varde_testkit::rng::Rng;

use super::calibration::{circle, sphere};
use super::*;
use crate::geom::certify::calibration::quarter;
use crate::geom::pcurve::{PSpan, Span2};
use crate::infra::budget::{Budget, Halt, Pass, Work};

/// `f` as the one item of a pass on a budget of `budget` units: its
/// outcome and the units used.
fn run<R: Send>(
    budget: u64,
    f: impl Fn(&mut Ledger<'_>) -> Result<R, Stop<QueryError>> + Sync + Send,
) -> (Result<R, Halt<QueryError>>, u64) {
    let mut work = Work::new(&Budget::new(budget));
    let pass = Pass {
        stage: "query",
        rung: 0,
    };
    let out = work.pass(pass, &[()], |_, ledger| f(ledger));
    (out.map(|mut v| v.remove(0)), work.used())
}

fn wind(trims: &[TrimCurve<'_>], uv: [f64; 2], near: [f64; 2]) -> Membership {
    run(MAX_WORK_TEST, |l| winding(trims, uv, near, l))
        .0
        .expect("a winding")
}

const MAX_WORK_TEST: u64 = crate::MAX_WORK;

/// The extrema `f` gives on a full budget.
fn ext(f: impl Fn(&mut Ledger<'_>) -> Result<Extrema, Stop<QueryError>> + Sync + Send) -> Extrema {
    run(MAX_WORK_TEST, f).0.expect("extrema")
}

/// The closed polyline through `pts` (and back to the first) as one
/// p-curve of linear spans, the weights `ws` in turn.
fn polygon(pts: &[[f64; 2]], ws: &[f64]) -> PCurve {
    let n = pts.len();
    let spans = (0..n)
        .map(|k| {
            let (a, b) = (pts[k], pts[(k + 1) % n]);
            let (w0, w1) = (ws[k % ws.len()], ws[(k + 1) % ws.len()]);
            PSpan::Linear([[a[0], a[1], w0], [b[0], b[1], w1]])
        })
        .collect();
    let knots = (0..=n).map(|k| k as f64).collect();
    PCurve::from_spans(spans, knots).expect("a polygon")
}

/// `p` run backwards: the spans reversed in reverse order, the knots
/// mirrored.
fn reversed(p: &PCurve) -> PCurve {
    let spans = p.spans().iter().rev().map(PSpan::reversed).collect();
    let ks = p.knots();
    let knots = ks
        .iter()
        .rev()
        .map(|k| ks[ks.len() - 1] - k + ks[0])
        .collect();
    PCurve::from_spans(spans, knots).expect("a reversed p-curve")
}

#[test]
fn a_square_with_a_round_hole() {
    let outer = polygon(
        &[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]],
        &[1.0, 3.0, 0.25],
    );
    // The hole runs clockwise: its circle reversed by its coedge.
    let hole = circle([2.0, 2.0], 1.0);
    let trims = [
        TrimCurve {
            pcurve: &outer,
            reversed: false,
        },
        TrimCurve {
            pcurve: &hole,
            reversed: true,
        },
    ];
    let near = [1e-9, 1e-9];
    let at = |uv| wind(&trims, uv, near);
    assert_eq!(at([0.5, 0.5]), Membership::Winding(1));
    assert_eq!(at([2.0, 2.0]), Membership::Winding(0));
    assert_eq!(at([2.5, 2.25]), Membership::Winding(0));
    assert_eq!(at([5.0, 2.0]), Membership::Winding(0));
    assert_eq!(at([-1.0, 2.0]), Membership::Winding(0));
    // On the ray's line through vertices and the circle's ends: the
    // height `v_p − ε` counts each once.
    assert_eq!(at([0.5, 2.0]), Membership::Winding(1));
    assert_eq!(at([3.5, 2.0]), Membership::Winding(1));
    assert_eq!(at([5.0, 4.0]), Membership::Winding(0));
    assert_eq!(at([-1.0, 0.0]), Membership::Winding(0));
    // On a trim: near, at a vertex, on a side, on the circle's knots.
    for uv in [
        [0.0, 0.0],
        [4.0, 4.0],
        [2.0, 0.0],
        [4.0, 1.5],
        [3.0, 2.0],
        [2.0, 1.0],
    ] {
        assert_eq!(at(uv), Membership::Near, "{uv:?}");
    }
    // Just off a side, inside the near box: decided all the same where
    // the side is certified off the point.
    assert_eq!(at([2.0, 1e-10]), Membership::Winding(1));
    assert_eq!(at([2.0, -1e-10]), Membership::Winding(0));
    assert_eq!(at([4.0 + 1e-10, 3.0]), Membership::Winding(0));
    // The coedges given the other way, each p-curve reversed too: the
    // same windings.
    let (o, h) = (reversed(&outer), reversed(&hole));
    let back = [
        TrimCurve {
            pcurve: &h,
            reversed: false,
        },
        TrimCurve {
            pcurve: &o,
            reversed: true,
        },
    ];
    for uv in [[0.5, 0.5], [2.0, 2.0], [0.5, 2.0], [5.0, 4.0]] {
        assert_eq!(wind(&back, uv, near), at(uv), "{uv:?}");
    }
}

#[test]
fn trims_must_close_and_inputs_be_finite() {
    let sq = polygon(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]], &[1.0]);
    let open = sq.slice(0.0, 2.0).expect("two spans");
    let t = |pcurve, reversed| TrimCurve { pcurve, reversed };
    let ask =
        |trims: &[TrimCurve<'_>], uv, near| run(MAX_WORK_TEST, |l| winding(trims, uv, near, l)).0;
    let failed = |e| Err(Halt::Failed { item: 0, error: e });
    assert_eq!(
        ask(&[t(&open, false)], [0.5, 0.25], [1e-6; 2]),
        failed(QueryError::Open)
    );
    // Two pieces of one loop close it, whichever way each is given.
    let rest = sq.slice(2.0, 3.0).expect("one span");
    let rest_back = reversed(&rest);
    assert_eq!(
        ask(
            &[t(&open, false), t(&rest_back, true)],
            [0.75, 0.25],
            [1e-6; 2]
        ),
        Ok(Membership::Winding(1))
    );
    // Pieces running into each other don't.
    assert_eq!(
        ask(
            &[t(&open, false), t(&rest_back, false)],
            [0.75, 0.25],
            [1e-6; 2]
        ),
        failed(QueryError::Open)
    );
    for (uv, near) in [
        ([f64::NAN, 0.0], [1e-6; 2]),
        ([0.0, f64::INFINITY], [1e-6; 2]),
        ([0.5, 0.25], [0.0, 1e-6]),
        ([0.5, 0.25], [1e-6, -1.0]),
        ([0.5, 0.25], [1e-6, f64::NAN]),
    ] {
        assert_eq!(
            ask(&[t(&sq, false)], uv, near),
            failed(QueryError::Input),
            "{uv:?} {near:?}"
        );
    }
}

/// A random closed loop about `c`: points round it at increasing angles
/// joined by spans of every kind (an iso span and a G span stepping to
/// each corner of a staircase now and then), weights in `[1/8, 8]`.
fn random_loop(rng: &mut Rng, c: [f64; 2]) -> PCurve {
    let n = 3 + (rng.next_u64() % 6) as usize;
    let mut angles: Vec<f64> = (0..n)
        .map(|_| rng.range(0.0, std::f64::consts::TAU))
        .collect();
    angles.sort_by(f64::total_cmp);
    let pts: Vec<[f64; 2]> = angles
        .iter()
        .map(|&a| {
            let r = rng.range(0.2, 1.0);
            [c[0] + r * trig::cos(a), c[1] + r * trig::sin(a)]
        })
        .collect();
    let mut w = || rng.log_range(0.125, 8.0);
    let mut spans = Vec::new();
    for k in 0..n {
        let (a, b) = (pts[k], pts[(k + 1) % n]);
        let mid = [
            (a[0] + b[0]) / 2.0 + (rng_like(k) - 0.5) * 0.3,
            (a[1] + b[1]) / 2.0 + (rng_like(k + 7) - 0.5) * 0.3,
        ];
        match k % 4 {
            0 => spans.push(PSpan::Q([
                [a[0], a[1], w()],
                [mid[0], mid[1], w()],
                [b[0], b[1], w()],
            ])),
            1 => spans.push(PSpan::Linear([[a[0], a[1], w()], [b[0], b[1], w()]])),
            2 => {
                // Along u at `a`'s height, then a G span on to `b`.
                let corner = [b[0], a[1]];
                spans.push(PSpan::Iso {
                    fixed: Dir::V,
                    at: a[1],
                    m: [[a[0], w()], [corner[0], w()]],
                });
                spans.push(PSpan::G {
                    mobius: Dir::U,
                    m: [[corner[0], w()], [b[0], w()]],
                    q: [[corner[1], w()], [mid[1], w()], [b[1], w()]],
                });
            }
            _ => spans.push(PSpan::G {
                mobius: Dir::V,
                m: [[a[1], w()], [b[1], w()]],
                q: [[a[0], w()], [mid[0], w()], [b[0], w()]],
            }),
        }
    }
    let knots = (0..=spans.len()).map(|k| k as f64).collect();
    PCurve::from_spans(spans, knots).expect("a loop")
}

/// A fixed pseudo-random value in `[0, 1)` for `k`.
fn rng_like(k: usize) -> f64 {
    Rng::new(k as u64 ^ 0x9e37).unit()
}

/// The polygon through `per_span` points of each span of `p` (from its
/// start), closing.
fn sampled(p: &PCurve, per_span: usize) -> Vec<[f64; 2]> {
    let ks = p.knots();
    let mut out = Vec::new();
    for k in 0..p.spans().len() {
        for i in 0..per_span {
            let t = ks[k] + (ks[k + 1] - ks[k]) * i as f64 / per_span as f64;
            out.push(p.point_at(t).expect("in range"));
        }
    }
    out
}

/// The winding of the closed polygon `poly` about `q` (crossings of the
/// ray along `+u`), and the least distance from `q` to its sides.
fn polygon_winding(poly: &[[f64; 2]], q: [f64; 2]) -> (i64, f64) {
    let (mut w, mut d) = (0i64, f64::INFINITY);
    for k in 0..poly.len() {
        let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
        let (ab, aq) = ([b[0] - a[0], b[1] - a[1]], [q[0] - a[0], q[1] - a[1]]);
        let len2 = ab[0] * ab[0] + ab[1] * ab[1];
        let t = if len2 > 0.0 {
            ((aq[0] * ab[0] + aq[1] * ab[1]) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let e = [aq[0] - t * ab[0], aq[1] - t * ab[1]];
        let dist = (e[0] * e[0] + e[1] * e[1]).sqrt();
        if dist < d {
            d = dist;
        }
        let cross = ab[0] * aq[1] - ab[1] * aq[0];
        if a[1] <= q[1] && b[1] > q[1] && cross > 0.0 {
            w += 1;
        } else if a[1] > q[1] && b[1] <= q[1] && cross < 0.0 {
            w -= 1;
        }
    }
    (w, d)
}

#[test]
fn windings_match_sampled_polygons_on_random_loops() {
    let near = [1e-9, 1e-9];
    let mut checked = 0;
    for seed in varde_testing::seeds(30, 600) {
        let mut rng = Rng::new(seed);
        // Two loops, overlapping or not, one of them split into two
        // coedges and one coedge given backwards.
        let a = random_loop(&mut rng, [0.0, 0.0]);
        let c = [rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)];
        let b = random_loop(&mut rng, c);
        let ks = a.knots();
        let cut = ks[ks.len() / 2];
        let (a0, a1) = (
            a.slice(ks[0], cut).expect("knots"),
            a.slice(cut, ks[ks.len() - 1]).expect("knots"),
        );
        let b_back = reversed(&b);
        let flip = rng.unit() < 0.5;
        let trims = [
            TrimCurve {
                pcurve: &a0,
                reversed: false,
            },
            TrimCurve {
                pcurve: &a1,
                reversed: false,
            },
            TrimCurve {
                pcurve: if flip { &b_back } else { &b },
                reversed: flip,
            },
        ];
        let (pa, pb) = (sampled(&a, 400), sampled(&b, 400));
        for _ in 0..40 {
            let q = [rng.range(-2.0, 2.0), rng.range(-2.0, 2.0)];
            let (wa, da) = polygon_winding(&pa, q);
            let (wb, db) = polygon_winding(&pb, q);
            let got = wind(&trims, q, near);
            // The polygons are within about 1e-5 of the loops.
            if da > 1e-3 && db > 1e-3 {
                assert_eq!(got, Membership::Winding(wa + wb), "seed {seed} at {q:?}");
                checked += 1;
            }
        }
        // On a loop: near.
        let t = rng.range(ks[0], ks[ks.len() - 1]);
        let on = a.point_at(t).expect("in range");
        assert_eq!(
            wind(&trims, on, near),
            Membership::Near,
            "seed {seed} at {on:?}"
        );
    }
    assert!(checked > 0);
}

#[test]
fn windings_escalate_where_the_first_level_cannot_settle() {
    // A loop far out in UV with a point a few ulps inside one side: the
    // first level's rounding of `u − u_p` is too coarse.
    let o = 1_099_511_627_776.0;
    let sq = polygon(
        &[[o, o], [o + 1.0, o], [o + 1.0, o + 1.0], [o, o + 1.0]],
        &[3.0, 0.125],
    );
    let trims = [TrimCurve {
        pcurve: &sq,
        reversed: false,
    }];
    let ulp = 1.0 / 4096.0;
    let (inside, outside) = ([o + 0.5, o + 4.0 * ulp], [o + 0.5, o - 4.0 * ulp]);
    let near = [ulp, ulp];
    assert_eq!(wind(&trims, inside, near), Membership::Winding(1));
    assert_eq!(wind(&trims, outside, near), Membership::Winding(0));
}

#[test]
fn extrema_enclose_known_values_within_the_resolution() {
    let res = 1e-9;
    let holds = |e: [f64; 2], x: f64| e[0] <= x && x <= e[1] && e[1] - e[0] <= res;
    // A quarter circle, any end weights: x from 0 to 50, at its ends.
    for w in [1.0, 3.0, 1.0 / 64.0] {
        let c = quarter(w);
        let e = ext(|l| curve_extrema(&c, [1.0, 0.0, 0.0], res, l));
        assert!(holds(e.min, 0.0) && holds(e.max, 50.0), "{w}: {e:?}");
        let e = ext(|l| curve_extrema(&c, [0.0, 0.0, 1.0], res, l));
        assert!(holds(e.min, 0.0) && holds(e.max, 0.0), "{w}: {e:?}");
    }
    // An interior minimum: `u = −2s(1 − s)`, least `−½` at `s = ½`; in
    // `v` a line from 0 to 1.
    let p = PCurve::new(
        vec![[[0.0, 0.0, 1.0], [-1.0, 0.5, 1.0], [0.0, 1.0, 1.0]]],
        vec![0.0, 1.0],
    )
    .expect("a p-curve");
    let e = ext(|l| pcurve_extrema(&p, [1.0, 0.0], res, l));
    assert!(holds(e.min, -0.5) && holds(e.max, 0.0), "{e:?}");
    let e = ext(|l| pcurve_extrema(&p, [0.0, -2.0], res, l));
    assert!(holds(e.min, -2.0) && holds(e.max, 0.0), "{e:?}");
    // The sphere of radius 10 about the origin: `±10·|d|` in every
    // direction `d`, its tiles rounded once.
    let halves = sphere();
    let tiles: Vec<&Tile> = halves.iter().flat_map(|s| s.tiles()).collect();
    let mut rng = Rng::new(7);
    for _ in 0..varde_testing::pick(4, 40) {
        let d = rng.direction().to_array();
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let e = ext(|l| tile_extrema(&tiles, d, res, l));
        let (reach, slack) = (10.0 * len, 1e-12);
        assert!(
            e.max[1] - e.max[0] <= res && e.min[1] - e.min[0] <= res,
            "{e:?}"
        );
        assert!(
            e.max[0] <= reach + slack && e.max[1] >= reach - slack,
            "{d:?}: {e:?}"
        );
        assert!(
            e.min[0] <= -reach + slack && e.min[1] >= -reach - slack,
            "{d:?}: {e:?}"
        );
    }
}

#[test]
fn extrema_hold_against_dense_samples() {
    let res = 1e-6;
    for seed in varde_testing::seeds(10, 200) {
        let mut rng = Rng::new(seed);
        let c = [rng.range(-5.0, 5.0), rng.range(-5.0, 5.0)];
        let p = random_loop(&mut rng, c);
        let d = [rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)];
        let e = run(MAX_WORK_TEST, |l| pcurve_extrema(&p, d, res, l))
            .0
            .expect("extrema");
        let vals: Vec<f64> = sampled(&p, 2000)
            .iter()
            .map(|q| d[0] * q[0] + d[1] * q[1])
            .collect();
        let lo = vals
            .iter()
            .copied()
            .fold(f64::INFINITY, |a, x| if x < a { x } else { a });
        let hi = vals
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, |a, x| if x > a { x } else { a });
        // Each sample's value rounded (a few ulps of the coordinates);
        // samples miss the extrema by second order (spans of 2000 steps).
        let ulps = 1e-13;
        assert!(
            e.min[0] <= lo + ulps && e.max[1] >= hi - ulps,
            "seed {seed}: {e:?}"
        );
        assert!(
            e.min[1] >= lo - 1e-5 && e.max[0] <= hi + 1e-5,
            "seed {seed}: {e:?}"
        );
        assert!(e.min[1] - e.min[0] <= res && e.max[1] - e.max[0] <= res);
    }
}

#[test]
fn extrema_escalate_past_the_first_levels_rounding() {
    // A span far out at a resolution finer than the first level's
    // rounding of its coordinates.
    let o = 1_073_741_824.0;
    let span: Span2 = [[o, o, 1.0], [o - 1.0, o + 0.5, 4.0], [o, o + 1.0, 0.25]];
    let p = PCurve::new(vec![span], vec![0.0, 1.0]).expect("a p-curve");
    // Sixteen ulps of the values: the enclosures are `f64`s rounded
    // outwards, so a resolution of an ulp or two is out of reach.
    let res = 1.0 / 4_194_304.0 * 16.0;
    let (e, _) = run(MAX_WORK_TEST, |l| pcurve_extrema(&p, [1.0, 0.0], res, l));
    let e = e.expect("extrema");
    assert!(e.level > 0, "{e:?}");
    assert!(
        e.min[1] - e.min[0] <= res && e.max[1] - e.max[0] <= res,
        "{e:?}"
    );
    assert!(
        e.max[0] <= o && o <= e.max[1] && e.min[1] < o - 0.5,
        "{e:?}"
    );
}

#[test]
fn charges_are_the_same_every_run_and_a_short_budget_runs_out() {
    let c = circle([0.25, 0.5], 0.5);
    let trims = [TrimCurve {
        pcurve: &c,
        reversed: false,
    }];
    let q = quarter(3.0);
    let ask = |budget| {
        run(budget, |l| {
            let m = winding(&trims, [0.25, 0.75], [1e-12; 2], l)?;
            let e = curve_extrema(&q, [0.6, 0.8, 0.0], 1e-10, l)?;
            Ok((m, e))
        })
    };
    let (full, used) = ask(MAX_WORK_TEST);
    assert!(full.is_ok() && used > 0, "{full:?}");
    assert_eq!(ask(MAX_WORK_TEST), (full.clone(), used));
    assert_eq!(ask(used).0, full);
    assert!(matches!(ask(used - 1).0, Err(Halt::OutOfBudget(_))));
}

#[test]
fn extrema_refuse_what_they_cannot_bound() {
    let q = quarter(1.0);
    let failed = |e| Err(Halt::Failed { item: 0, error: e });
    for (dir, res) in [
        ([f64::NAN, 0.0, 0.0], 1e-6),
        ([1.0, 0.0, 0.0], 0.0),
        ([1.0, 0.0, 0.0], -1.0),
        ([1.0, 0.0, 0.0], f64::INFINITY),
    ] {
        assert_eq!(
            run(MAX_WORK_TEST, |l| curve_extrema(&q, dir, res, l)).0,
            failed(QueryError::Input),
            "{dir:?} {res}"
        );
    }
    assert_eq!(
        run(MAX_WORK_TEST, |l| tile_extrema(
            &[],
            [1.0, 0.0, 0.0],
            1e-6,
            l
        ))
        .0,
        failed(QueryError::Input)
    );
}
