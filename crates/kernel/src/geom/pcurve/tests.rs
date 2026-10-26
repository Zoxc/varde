//! Span kinds: each ends on its end control values, evaluates to its
//! exact point, splits keeping its kind (an iso span its fixed value to
//! the bit), and is refused for its first offence. Tile-line splits: a
//! Möbius coordinate is cut at its exact inverse, the knot the nearest
//! `f64` and the point on the line; a quadratic one at its roots, the
//! knot nearest each; a line through a lattice node cuts once there, a
//! touch at its double root, a bulge over a line it doesn't cross is
//! halved; values past the exact integers and crossings within an ulp
//! are refused; random chains of every kind come out in one tile per
//! span, close to the original, the spans already in a tile and the
//! original knots kept, and splitting again changes nothing. A linear or
//! iso span read as a Q span is the same curve, or refused where its
//! middle control point would round.

use malachite::Rational as Q;
use varde_testkit::rng::Rng;

use super::*;

/// The exact UV point of span `k` of `p` at local `s`.
fn exact_at(p: &PCurve, k: usize, s: &Q) -> [Q; 2] {
    let zero = Q::from(0u32);
    p.coords_at(k, &zero)
        .expect("a span")
        .map(|c| c.eval(s)[0].clone())
}

/// The exact UV point of `p` at `t` (inside its knots).
fn exact_t(p: &PCurve, t: f64) -> [Q; 2] {
    let ks = p.knots();
    let k = ks.partition_point(|&x| x <= t).clamp(1, ks.len() - 1) - 1;
    exact_at(p, k, &local_exact(t, ks[k], ks[k + 1]))
}

/// One span of each kind from `(0.25, 0.5)` to `(0.75, 0.625)` (an iso
/// span along `v = 0.5`), weights off 1.
fn kinds() -> Vec<PSpan> {
    vec![
        PSpan::Q([[0.25, 0.5, 2.0], [0.5, 0.75, 0.5], [0.75, 0.625, 1.5]]),
        PSpan::G {
            mobius: Dir::U,
            m: [[0.25, 3.0], [0.75, 0.25]],
            q: [[0.5, 1.0], [0.875, 0.75], [0.625, 2.0]],
        },
        PSpan::G {
            mobius: Dir::V,
            m: [[0.5, 0.5], [0.625, 4.0]],
            q: [[0.25, 1.0], [0.125, 3.0], [0.75, 1.0]],
        },
        PSpan::Linear([[0.25, 0.5, 1.0], [0.75, 0.625, 8.0]]),
        PSpan::Iso {
            fixed: Dir::V,
            at: 0.5,
            m: [[0.25, 1.0], [0.75, 1.0 / 16.0]],
        },
    ]
}

#[test]
fn each_kind_ends_on_its_end_values_and_evaluates_its_exact_point() {
    let degrees = [[2, 2], [1, 2], [2, 1], [1, 1], [1, 0]];
    for (span, deg) in kinds().into_iter().zip(degrees) {
        assert_eq!(span.degrees(), deg, "{span:?}");
        let p = PCurve::from_spans(vec![span], vec![1.0, 3.0]).expect("a p-curve");
        // At the knots: the end control values, with no error.
        assert_eq!(p.knot_point(0), Some(span.start()));
        assert_eq!(p.knot_point(1), Some(span.end()));
        for (t, end) in [(1.0, span.start()), (3.0, span.end())] {
            let e = p.eval(t).expect("in range");
            assert_eq!(e.map(|x| x.v), end);
            assert!(e.iter().all(|x| x.err_trunc == 0.0 && x.err_round == 0.0));
            assert_eq!(p.point_at(t), Some(end));
        }
        // Inside: the canonical point is the exact one rounded, and the
        // certified evaluation and the span over one denominator
        // enclose it.
        let zero = Q::from(0u32);
        let joint = p.span_at(0, &zero).expect("a span");
        for i in 1..32 {
            let t = 1.0 + f64::from(i) / 16.0;
            let s = local_exact(t, 1.0, 3.0);
            let x = exact_at(&p, 0, &s);
            assert_eq!(p.point_at(t), Some(x.clone().map(|c| nearest(&c))));
            assert_eq!(joint.eval(&s), x, "{span:?} at {t}");
            for (e, x) in p.eval(t).expect("in range").iter().zip(&x) {
                let [lo, hi] = e.enclose();
                assert!(q(lo) <= *x && *x <= q(hi), "{span:?} at {t}");
            }
        }
        // The box of the control values holds the span; run backwards,
        // the same points.
        let [lo, hi] = span.hull();
        assert!(lo[0] <= 0.25 && hi[0] >= 0.75 && lo[1] <= 0.5);
        let back = PCurve::from_spans(vec![span.reversed()], vec![1.0, 3.0]).expect("a p-curve");
        assert_eq!(back.knot_point(0), p.knot_point(1));
        let s = local_exact(1.75, 1.0, 3.0);
        let one = Q::from(1u32);
        assert_eq!(exact_at(&back, 0, &(&one - &s)), exact_at(&p, 0, &s));
        assert_eq!(span.tile(), Some([0, 0]));
    }
}

#[test]
fn splits_keep_kinds_and_carry_the_canonical_points() {
    let mut rng = Rng::new(11);
    for span in kinds() {
        let p = PCurve::from_spans(vec![span], vec![0.0, 2.0]).expect("a p-curve");
        for _ in 0..varde_testing::pick(20, 200) {
            let ts = [rng.range(0.0, 2.0), rng.range(0.0, 2.0), 7.0, f64::NAN];
            let s = p.split(&ts).expect("a p-curve");
            assert!(s.spans().iter().all(|x| x.kind() == span.kind()));
            for (k, &t) in s.knots().iter().enumerate() {
                assert_eq!(s.knot_point(k), p.point_at(t), "{span:?} at {t}");
            }
            if let PSpan::Iso { at, .. } = span {
                assert!(
                    s.spans().iter().all(
                        |x| matches!(x, PSpan::Iso { at: a, .. } if a.to_bits() == at.to_bits())
                    )
                );
            }
            // Within a few ulps of the original along it.
            for i in 0..=32 {
                let t = f64::from(i) / 16.0;
                let (a, b) = (p.eval(t).unwrap(), s.eval(t).unwrap());
                for k in 0..2 {
                    assert!((a[k].v - b[k].v).abs() <= 1e-14, "{span:?} at {t}");
                }
            }
        }
        assert_eq!(p.split(&[]).expect("a p-curve"), p);
    }
}

#[test]
fn p_curves_split_the_same_way() {
    let p = PCurve::new(
        vec![[[0.25, 0.25, 1.0], [0.5, 0.375, 1.5], [0.75, 0.25, 1.0]]],
        vec![0.0, 1.0],
    )
    .expect("a p-curve");
    let t = 0.3;
    let at = p.point_at(t).expect("in range");
    let s = p.split(&[t]).expect("a p-curve");
    assert_eq!(s.knots(), [0.0, t, 1.0]);
    assert_eq!(s.knot_point(1), Some(at));
    assert_eq!(s.knot_point(0), p.knot_point(0));
    assert_eq!(s.knot_point(2), p.knot_point(1));
    // Slices take whole spans.
    assert_eq!(s.slice(t, 1.0).expect("knots").spans(), &s.spans()[1..]);
    assert_eq!(s.slice(0.5, 1.0), None);
}

#[test]
fn chains_are_refused_for_their_first_offence() {
    let [q_span, g, _, line, iso] = kinds().try_into().expect("five kinds");
    let ks = vec![0.0, 1.0];
    let heavy = PSpan::G {
        mobius: Dir::U,
        m: [[0.25, 1.0], [0.75, 1.0]],
        q: [[0.5, 1.0], [0.875, 65.0], [0.625, 1.0]],
    };
    assert_eq!(
        PCurve::from_spans(vec![g, heavy], vec![0.0, 1.0, 2.0]),
        Err(CurveError::Weight { span: 1 })
    );
    let nan = PSpan::Iso {
        fixed: Dir::U,
        at: f64::NAN,
        m: [[0.5, 1.0], [0.75, 1.0]],
    };
    assert_eq!(
        PCurve::from_spans(vec![nan], ks.clone()),
        Err(CurveError::NotFinite { span: 0 })
    );
    // Each span starts where the one before ends, whatever the kinds:
    // the line only run backwards from the Q span's end.
    let back = line.reversed();
    assert_eq!(
        PCurve::from_spans(vec![q_span, back, iso, line], vec![0.0, 1.0, 2.0, 3.0, 4.0]),
        Err(CurveError::Join { span: 3 })
    );
    assert!(PCurve::from_spans(vec![q_span, back, iso], vec![0.0, 1.0, 2.0, 3.0]).is_ok());
    assert_eq!(
        PCurve::from_spans(vec![iso], vec![0.0]),
        Err(CurveError::Shape)
    );
    assert_eq!(
        PCurve::from_spans(vec![iso], vec![1.0, 1.0]),
        Err(CurveError::Knots { knot: 1 })
    );
    // Only Q spans move their middle point.
    let p = PCurve::from_spans(vec![iso], ks).expect("a p-curve");
    assert_eq!(p.perturbed(&[[0.1, 0.0]]), Err(CurveError::Shape));
    assert_eq!(p.q_spans(), None);
}

/// The local parameter where the Möbius coordinate `(x, w)` crosses
/// `line`, exactly.
fn mobius_inverse(m: &[[f64; 2]], line: i64) -> Q {
    let l = Q::from(line);
    let f0 = (q(m[0][0]) - &l) * q(m[0][1]);
    let f1 = (q(m[1][0]) - &l) * q(m[1][1]);
    &f0 / (&f0 - f1)
}

#[test]
fn mobius_crossings_cut_at_the_exact_inverse() {
    let line = PSpan::Linear([[0.3, 0.2, 1.0], [2.7, 3.1, 3.0]]);
    let (a, b) = (-0.5, 1.25);
    let p = PCurve::from_spans(vec![line], vec![a, b]).expect("a p-curve");
    let s = p.split_at_tile_lines().expect("cut at the tile lines");
    assert!(s.in_tiles());
    // `u = 1, 2` and `v = 1, 2, 3`.
    assert_eq!(s.spans().len(), 6);
    assert!(s.spans().iter().all(|x| x.kind() == Kind::Linear));
    let coords = line.coords();
    for k in 1..s.spans().len() {
        let uv = s.knot_point(k).expect("a knot");
        let t = s.knots()[k];
        // One coordinate on its line exactly; the knot the nearest `f64`
        // to the exact crossing, the other coordinate the exact one
        // there, rounded.
        let c = (0..2).find(|&c| uv[c].fract() == 0.0).expect("on a line");
        let star = mobius_inverse(coords[c].values(), uv[c] as i64);
        assert_eq!(t, nearest(&(q(a) + &star * (q(b) - q(a)))));
        let x = exact_at(&p, 0, &star);
        assert_eq!(uv[1 - c], nearest(&x[1 - c]), "knot {k}");
    }
    // Already in tiles: the same chain.
    assert_eq!(s.split_at_tile_lines().expect("in tiles"), s);
}

#[test]
fn iso_spans_keep_their_fixed_value_through_tile_lines() {
    // Along the tile line `v = 2` itself, `u` from 0.5 to 3.5.
    let m = [[0.5, 2.0], [3.5, 0.5]];
    let iso = PSpan::Iso {
        fixed: Dir::V,
        at: 2.0,
        m,
    };
    let p = PCurve::from_spans(vec![iso], vec![0.0, 1.0]).expect("a p-curve");
    let s = p.split_at_tile_lines().expect("cut at the tile lines");
    assert_eq!(s.spans().len(), 4);
    for (k, x) in s.spans().iter().enumerate() {
        let PSpan::Iso { fixed, at, .. } = x else {
            panic!("an iso span: {x:?}");
        };
        assert_eq!((*fixed, at.to_bits()), (Dir::V, 2f64.to_bits()));
        assert_eq!(x.tile(), Some([k as i64, 2]));
    }
    for (k, line) in [(1, 1), (2, 2), (3, 3)] {
        assert_eq!(s.knot_point(k), Some([line as f64, 2.0]));
        assert_eq!(s.knots()[k], nearest(&mobius_inverse(&m, line)));
    }
}

#[test]
fn quadratic_crossings_cut_at_the_knot_nearest_each_root() {
    // `v` rises over the line `v = 1` and falls back: two roots, neither
    // rational.
    let g = PSpan::G {
        mobius: Dir::U,
        m: [[0.2, 1.0], [0.8, 2.0]],
        q: [[0.5, 1.0], [2.5, 0.7], [0.375, 1.5]],
    };
    let p = PCurve::from_spans(vec![g], vec![0.0, 1.0]).expect("a p-curve");
    let s = p.split_at_tile_lines().expect("cut at the tile lines");
    assert_eq!(s.spans().len(), 3);
    assert!(s.in_tiles());
    assert_eq!(
        s.spans()
            .iter()
            .map(|x| x.tile().unwrap())
            .collect::<Vec<_>>(),
        [[0, 0], [0, 1], [0, 0]]
    );
    let one = Q::from(1u32);
    for k in [1, 2] {
        let t = s.knots()[k];
        let uv = s.knot_point(k).expect("a knot");
        assert_eq!(uv[1], 1.0);
        // The root lies within half an ulp of the knot: the exact
        // coordinate less 1 changes sign across the knot's neighbours.
        let side = |t: f64| sgn(&(&exact_t(&p, t)[1] - &one));
        assert_ne!(side(t.next_down()), side(t.next_up()), "knot {t}");
        // `u`, a Möbius coordinate, the span's own there, rounded.
        assert_eq!(uv[0], nearest(&exact_t(&p, t)[0]));
    }
}

#[test]
fn a_line_through_a_lattice_node_cuts_once_there() {
    let line = PSpan::Linear([[0.5, 0.5, 1.0], [1.5, 1.5, 1.0]]);
    let p = PCurve::from_spans(vec![line], vec![0.0, 1.0]).expect("a p-curve");
    let s = p.split_at_tile_lines().expect("cut at the tile lines");
    assert_eq!(s.knots(), [0.0, 0.5, 1.0]);
    assert_eq!(s.knot_point(1), Some([1.0, 1.0]));
    assert_eq!(
        s.spans()
            .iter()
            .map(|x| x.tile().unwrap())
            .collect::<Vec<_>>(),
        [[0, 0], [1, 1]]
    );
}

#[test]
fn a_touch_cuts_at_its_double_root() {
    // `v = 0.5 + 2s(1 − s)` touches `v = 1` at `s = ½` from below.
    let touch = PSpan::Q([[0.25, 0.5, 1.0], [0.5, 1.5, 1.0], [0.75, 0.5, 1.0]]);
    let p = PCurve::from_spans(vec![touch], vec![0.0, 1.0]).expect("a p-curve");
    let s = p.split_at_tile_lines().expect("cut at the tile lines");
    assert_eq!(s.knots(), [0.0, 0.5, 1.0]);
    assert_eq!(s.knot_point(1), Some([0.5, 1.0]));
    // The halves end tangent to the line: their middle points on it.
    for x in s.spans() {
        assert_eq!(x.as_q().unwrap()[1][1], 1.0);
        assert_eq!(x.tile(), Some([0, 0]));
    }
}

#[test]
fn a_bulge_over_a_line_it_does_not_cross_is_halved_into_its_tile() {
    // The middle control value past `u = 1`, the curve below it (at most
    // 0.8 at the middle).
    let bulge = PSpan::Q([[0.2, 0.25, 1.0], [1.4, 0.5, 0.5], [0.8, 0.75, 1.0]]);
    let p = PCurve::from_spans(vec![bulge], vec![0.0, 4.0]).expect("a p-curve");
    assert_eq!(bulge.tile(), None);
    let s = p.split_at_tile_lines().expect("halved into the tile");
    assert!(s.spans().len() >= 2);
    for (k, x) in s.spans().iter().enumerate() {
        assert_eq!(x.tile(), Some([0, 0]));
        // Halvings at canonical points, nothing moved onto a line.
        assert_eq!(s.knot_point(k), p.point_at(s.knots()[k]));
    }
}

#[test]
fn values_past_the_exact_integers_and_crossings_within_an_ulp_are_refused() {
    let far = PSpan::Linear([[0.5, 0.5, 1.0], [5e15, 0.5, 1.0]]);
    let inside = PSpan::Linear([[5e15, 0.5, 1.0], [5e15, 0.75, 1.0]]);
    let p = PCurve::from_spans(vec![inside.reversed(), far.reversed()], vec![0.0, 1.0, 2.0])
        .expect("a p-curve");
    assert_eq!(p.split_at_tile_lines(), Err(CurveError::Tile { span: 0 }));
    // A span over two ulps of `t`, one `f64` inside: two lines crossed
    // there have one knot between them.
    let ts = vec![1.0, 1f64.next_up().next_up()];
    let fast = PSpan::Linear([[0.5, 0.5, 1.0], [2.5, 0.5, 1.0]]);
    let p = PCurve::from_spans(vec![fast], ts.clone()).expect("a p-curve");
    assert_eq!(p.split_at_tile_lines(), Err(CurveError::Tile { span: 0 }));
    // One line is cut there; over one ulp there is no room.
    let one = PSpan::Linear([[0.5, 0.5, 1.0], [1.5, 0.5, 1.0]]);
    let p = PCurve::from_spans(vec![one], ts).expect("a p-curve");
    let s = p.split_at_tile_lines().expect("one cut");
    assert_eq!(s.knots()[1], 1f64.next_up());
    assert_eq!(s.knot_point(1), Some([1.0, 0.5]));
    let p = PCurve::from_spans(vec![one], vec![1.0, 1f64.next_up()]).expect("a p-curve");
    assert_eq!(p.split_at_tile_lines(), Err(CurveError::Tile { span: 0 }));
}

/// A random weight in `[1/64, 64]`, log-uniform, sometimes 1.
fn weight(rng: &mut Rng) -> f64 {
    if rng.unit() < 0.3 {
        1.0
    } else {
        rng.log_range(MIN_WEIGHT, MAX_WEIGHT)
    }
}

/// A random UV value in `[−3, 3]`, sometimes on a tile line.
fn value(rng: &mut Rng) -> f64 {
    if rng.unit() < 0.15 {
        (rng.next_u64() % 7) as f64 - 3.0
    } else {
        rng.range(-3.0, 3.0)
    }
}

/// A random span of a random kind from `a` (an iso one along `a`'s
/// fixed coordinate); its end.
fn random_span(rng: &mut Rng, a: [f64; 2]) -> PSpan {
    let b = [value(rng), value(rng)];
    let d = if rng.unit() < 0.5 { Dir::U } else { Dir::V };
    let k = ix(d);
    match rng.next_u64() % 4 {
        0 => {
            let m = [value(rng), value(rng)];
            PSpan::Q([
                [a[0], a[1], weight(rng)],
                [m[0], m[1], weight(rng)],
                [b[0], b[1], weight(rng)],
            ])
        }
        1 => PSpan::G {
            mobius: d,
            m: [[a[k], weight(rng)], [b[k], weight(rng)]],
            q: [
                [a[1 - k], weight(rng)],
                [value(rng), weight(rng)],
                [b[1 - k], weight(rng)],
            ],
        },
        2 => {
            let w = weight(rng);
            PSpan::Linear([[a[0], a[1], w], [b[0], b[1], weight(rng)]])
        }
        _ => PSpan::Iso {
            fixed: d,
            at: a[k],
            m: [[a[1 - k], weight(rng)], [b[1 - k], weight(rng)]],
        },
    }
}

#[test]
fn random_chains_split_into_tiles_close_to_the_original() {
    let mut refused = 0;
    let seeds = varde_testing::seeds(300, 5000);
    let total = seeds.end - seeds.start;
    for seed in seeds {
        let mut rng = Rng::new(seed);
        let n = 1 + (rng.next_u64() % 4) as usize;
        let mut spans = Vec::new();
        let mut at = [value(&mut rng), value(&mut rng)];
        for _ in 0..n {
            let s = random_span(&mut rng, at);
            at = s.end();
            spans.push(s);
        }
        let mut knots = vec![rng.range(-4.0, 4.0)];
        for _ in 0..n {
            let last = *knots.last().unwrap();
            knots.push(last + rng.log_range(1e-3, 8.0));
        }
        let p = PCurve::from_spans(spans, knots).expect("a p-curve");
        let s = match p.split_at_tile_lines() {
            Ok(s) => s,
            Err(CurveError::Tile { .. }) => {
                refused += 1;
                continue;
            }
            Err(e) => panic!("seed {seed}: {e:?}"),
        };
        assert!(s.in_tiles(), "seed {seed}");
        // The original knots stay, with their points; each span keeps
        // its kind; a span already in a tile keeps its bits.
        for (k, &t) in p.knots().iter().enumerate() {
            let j = s.knots().iter().position(|&x| x == t).expect("kept");
            assert_eq!(s.knot_point(j), p.knot_point(k), "seed {seed}");
        }
        for (j, x) in s.spans().iter().enumerate() {
            let t = s.knots()[j];
            let k = p.knots().partition_point(|&y| y <= t) - 1;
            assert_eq!(x.kind(), p.spans()[k].kind(), "seed {seed}");
        }
        for (k, x) in p.spans().iter().enumerate() {
            if x.tile().is_some() {
                let t = p.knots()[k];
                let j = s.knots().iter().position(|&y| y == t).expect("kept");
                assert_eq!(&s.spans()[j], x, "seed {seed}");
            }
        }
        // Along the chain, the split one is the original to within the
        // rounding of its values (weights up to 64 apart).
        let (t0, t1) = (p.knots()[0], p.knots()[n]);
        for i in 0..=64 {
            let t = (t0 + (t1 - t0) * f64::from(i) / 64.0).clamp(t0, t1);
            let (a, b) = (p.eval(t).unwrap(), s.eval(t).unwrap());
            for k in 0..2 {
                let d = (a[k].v - b[k].v).abs();
                assert!(d <= 1e-9, "seed {seed} at {t}: {d}");
            }
        }
        assert_eq!(s.split_at_tile_lines().as_ref(), Ok(&s), "seed {seed}");
    }
    // Refusals need crossings within an ulp or spans that won't halve
    // into a tile: none on these.
    assert_eq!(refused, 0, "{refused} of {total} refused");
}

/// Linear and iso spans of equal end weights read as Q spans the same
/// curve over the same parameter, exactly; a midpoint that would round,
/// unequal weights or a graph span is no Q span.
#[test]
fn straight_spans_read_as_q_spans_exactly() {
    let mut rng = Rng::new(31);
    // Multiples of 2^-20 below 2^10: their sums and halves are exact.
    let mut grid = || (rng.next_u64() % (1 << 30)) as f64 / (1u64 << 20) as f64 - 512.0;
    for _ in 0..50 {
        let w = [1.0, 0.75, 3.0][(grid().abs() as usize) % 3];
        let linear = PSpan::Linear([[grid(), grid(), w], [grid(), grid(), w]]);
        let iso = PSpan::Iso {
            fixed: if grid() > 0.0 { Dir::U } else { Dir::V },
            at: grid(),
            m: [[grid(), w], [grid(), w]],
        };
        for span in [linear, iso] {
            let raised = PCurve::new(vec![span.to_q().expect("a Q span")], vec![0.0, 1.0])
                .expect("a p-curve");
            let held = PCurve::from_spans(vec![span], vec![0.0, 1.0]).expect("a p-curve");
            assert_eq!(held.q_chain(), Some(raised.q_spans().expect("Q spans")));
            for s in [0u32, 1, 3, 7] {
                let s = Q::from(s) / Q::from(7u32);
                assert_eq!(exact_at(&raised, 0, &s), exact_at(&held, 0, &s), "{span:?}");
            }
        }
    }
    let tiny = f64::EPSILON / 2.0;
    assert_eq!(
        PSpan::Linear([[1.0, 0.0, 1.0], [tiny, 0.0, 1.0]]).to_q(),
        None,
        "1 + 2^-53 rounds"
    );
    assert_eq!(
        PSpan::Linear([[0.0, 0.0, 1.0], [1.0, 0.0, 2.0]]).to_q(),
        None
    );
    let q = [[0.0, 0.0, 1.0], [0.5, 1.0, 1.0], [1.0, 0.0, 1.0]];
    assert_eq!(PSpan::Q(q).to_q(), Some(q));
}
