//! The ray ties past the span's own row, against the moved configuration:
//! a point a tiny distance off a tangency of the span with its ray line,
//! decided at targets far below `f64` parameter spacing; points in the
//! fold columns of circles and ellipses in vertical planes (the shadow's
//! tangent along `UP` at the foot, at a vertex, a knot or inside a span),
//! the plane across the ray line, oblique or along it; and the ties
//! beside a near end decided alike at the first rungs' bands, the band of
//! the point fixed.

use super::*;

fn p2(k: i32) -> f64 {
    libm::ldexp(1.0, k)
}

/// The span records of `qs` against every span of the other operand's
/// edges, at `band`; the first failure if one fails.
fn fill_at(g: &Geom, qs: &[Point], band: Band) -> Result<CurvedRays, ShadowError<Fail>> {
    let mut keys = Vec::new();
    for &q in qs {
        for ((op, e), (_, spans)) in &g.edges {
            if *op == q.operand.other() {
                keys.extend((0..spans.len() as u32).map(|span| SpanKey { q, g: *e, span }));
            }
        }
    }
    let (mut r, mut s) = (CurvedRays::new(), Shadows::new());
    r.fill(keys, &mut s, g, band, &mut Seq::default())?;
    Ok(r)
}

/// A span with weights `1/64` tangent to the ray line at `t = ½`, near the
/// origin and far from it, against points off its middle by `ν·2⁻ᵏ` on
/// either side (two crossings close together, or none), up to `k = 140`,
/// and the same with `UP·2` added where the coordinates hold it: decided
/// at targets of `2⁻⁽ᵏ⁺²⁰⁾` and `2⁻⁽²ᵏ⁺²⁰⁾` (`2⁻¹⁰⁰` for `k = 60`, past
/// the spacing of `f64` parameters), every crossing the moved
/// configuration's.
#[test]
fn a_tiny_offset_from_a_tangency_decides_at_tiny_targets() {
    let mut decided = 0;
    for (op, d, at) in [
        (B, DVec3::ZERO, DVec3::ZERO),
        (A, DIRECTIONS[0], DVec3::ZERO),
        (B, DVec3::ZERO, lat(1024.0, 3.0, 77.0)),
        (A, DIRECTIONS[0], lat(-1e6, 3.0, 1e5)),
    ] {
        for k in [40, 60, 80, 100, 120, 140] {
            for side in [1.0, -1.0] {
                for up in [0.0, 2.0] {
                    let mut g = Geom::with(d, Sign::Pos);
                    let w = 1.0 / 64.0;
                    let s: Span = [
                        cp(at + lat(-4.0, 1.0, 0.0), w),
                        cp(at + lat(0.0, -1.0, 0.0), w),
                        cp(at + lat(4.0, 1.0, 0.0), w),
                    ];
                    let e = g.edge(op, 0, 1, 2, &[s]);
                    let mid = middle(&s) + UP * up;
                    let q = g.query(op.other(), 9, mid + NU * (side * p2(-k)));
                    if g.pos[&q] == mid {
                        // The offset rounds away at these coordinates.
                        continue;
                    }
                    for target in [k + 20, 2 * k + 20] {
                        let band = Band::curved(0.0, p2(-target)).expect("a band");
                        let r = fill_at(&g, &[q], band).expect("decided");
                        check(&r, &g, q, true, &format!("{op:?} {at} 2^-{k} {up}"));
                        assert!(r.span(q, e, 0).is_some());
                        decided += 1;
                    }
                }
            }
        }
    }
    assert!(decided >= 60, "{decided}");
}

/// A circle in the vertical plane through `c` along `v` (and [`UP`]),
/// radius `r` in units of `v` and `UP`: four quarter spans from angle 0,
/// where (and at 180°) its tangent is along `UP`, a fold of its shadow.
fn vertical_circle(c: DVec3, v: DVec3, r: f64) -> Vec<Span> {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let at = |x: f64, y: f64| c + v * (r * x) + UP * (r * y);
    let pts = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0), (1.0, 0.0)];
    (0..4)
        .map(|k| {
            let (a, b) = (pts[k], pts[k + 1]);
            [
                cp(at(a.0, a.1), 1.0),
                cp(at(a.0 + b.0, a.1 + b.1), w),
                cp(at(b.0, b.1), 1.0),
            ]
        })
        .collect()
}

/// The planes of the vertical circles: across the ray line, oblique, along
/// it (the circle's shadow on the ray line of every point in its plane),
/// oblique the other way.
const PLANES: [DVec3; 4] = [
    NU,
    DVec3::new(NU.x + R_HAT.x * 0.25, NU.y, NU.z + R_HAT.z * 0.25),
    R_HAT,
    DVec3::new(
        NU.x * 0.5 - R_HAT.x * 2.0,
        NU.y * 0.5,
        NU.z * 0.5 - R_HAT.z * 2.0,
    ),
];

/// Circles in vertical planes, closed chains of two edges whose vertices
/// are the folds of their shadows (the span's tangent along `UP` at its
/// end), against points in the fold columns (above, below, at the fold)
/// and on the folds' ray lines before and past them: every record decides
/// at the first rungs' target, each span's crossings summing to the moved
/// configuration's, and the chain's to its count along `ν` too.
#[test]
fn points_in_the_fold_columns_of_vertical_circles() {
    for (vi, v) in PLANES.into_iter().enumerate() {
        for (op, d) in [
            (B, DVec3::ZERO),
            (A, DIRECTIONS[0]),
            (A, R_HAT),
            (A, NU),
            (A, UP),
        ] {
            let mut g = Geom::with(d, Sign::Pos);
            let c = lat(1.0, 2.0, 0.5);
            let spans = vertical_circle(c, v, 2.0);
            let e0 = g.edge(op, 0, 0, 1, &spans[0..2]);
            let e1 = g.edge(op, 1, 1, 0, &spans[2..4]);
            let mut qs = Vec::new();
            let mut id = 100;
            for j in [point_of(&spans[0][0]), point_of(&spans[2][0])] {
                for m in [0.0, 3.0, -5.0, 0.5] {
                    for off in [DVec3::ZERO, R_HAT * 0.25, -R_HAT * 0.25] {
                        id += 1;
                        qs.push(g.query(op.other(), id, j + UP * m + off));
                    }
                }
            }
            let r = fill_at(&g, &qs, band()).expect("decided");
            let nu = q3(NU);
            for &q in &qs {
                let here = format!("plane {vi}, {op:?} {d}, {}", g.pos[&q]);
                check(&r, &g, q, false, &here);
                // The chain's count along the ray, and along `ν`.
                let qm = g.moved(q.operand, g.pos[&q]);
                let (mut sum, mut across) = (0, 0);
                for (e, forward) in [(e0, true), (e1, false)] {
                    for i in 0..2 {
                        let c = r.span(q, e, i as u32).expect("stored").count();
                        let o: i32 = oracle(&g, op, e, i, &qm, &nu)
                            .iter()
                            .map(|&x| i32::from(x))
                            .sum();
                        sum += if forward { c } else { -c };
                        across += if forward { o } else { -o };
                    }
                }
                assert_eq!(sum, across, "{here}");
            }
        }
    }
}

/// One span of an ellipse in a vertical plane whose shadow folds at
/// `t = ½` (weights 1.5, 0.5, 1.5: its point there exact), against points
/// in the fold column and on the fold's ray line: no crossing of its own;
/// a touch exactly where the moved configuration crosses the ray twice
/// there (the two crossings cancel), else none, every count the moved
/// configuration's.
#[test]
fn points_in_a_fold_column_inside_a_span() {
    let mut touched = 0;
    for (vi, v) in PLANES.into_iter().enumerate() {
        if vi == 2 {
            // Along the ray line: no fold seen across it.
            continue;
        }
        for (op, d) in [
            (B, DVec3::ZERO),
            (A, DIRECTIONS[0]),
            (A, -R_HAT),
            (A, NU),
            (A, -NU),
            (A, UP),
        ] {
            let mut g = Geom::with(d, Sign::Pos);
            let c = lat(-1.0, 0.5, 0.25);
            let s: Span = [
                cp(c + v - UP, 1.5),
                cp(c + v * 2.0, 0.5),
                cp(c + v + UP, 1.5),
            ];
            let e = g.edge(op, 0, 1, 2, &[s]);
            let fold = middle(&s);
            let mut qs = Vec::new();
            let mut id = 100;
            for m in [0.0, 3.0, -5.0, 0.5] {
                for off in [0.0, 0.25, -0.25] {
                    id += 1;
                    qs.push((
                        g.query(op.other(), id, fold + UP * m + R_HAT * off),
                        off == 0.0,
                    ));
                }
            }
            let points: Vec<Point> = qs.iter().map(|&(q, _)| q).collect();
            let r = fill_at(&g, &points, band()).expect("decided");
            for &(q, column) in &qs {
                let here = format!("plane {vi}, {op:?} {d}, {}", g.pos[&q]);
                check(&r, &g, q, false, &here);
                let rec = r.span(q, e, 0).expect("stored");
                let qm = g.moved(q.operand, g.pos[&q]);
                let want = oracle(&g, op, e, 0, &qm, &ray_dir(q.operand));
                if column {
                    // In the fold column: no crossing of its own, a touch
                    // where the moved line crosses the span twice.
                    assert!(rec.crossings.is_empty(), "{here}: {rec:?}");
                    assert_eq!(
                        rec.touches.len(),
                        want.len() / 2,
                        "{here}: {rec:?} {want:?}"
                    );
                    touched += rec.touches.len();
                }
            }
        }
    }
    assert!(touched > 0);
}

/// Points beside the joints of a closed chain (vertices and knots of a
/// circle, off them by nothing, by a few roundings or by up to `4·2⁻ᵏ`),
/// their band fixed at `2⁻³⁰`, decided at the bands of the first three
/// rungs (targets `2⁻³⁰`, `2⁻³²`, `2⁻³⁴`, the last with a tie band): the
/// same crossings at each, the moved configuration's.
#[test]
fn ties_beside_a_near_end_read_alike_at_every_rung() {
    let bands = [
        Band::curved(0.0, p2(-30)),
        Band::curved(0.0, p2(-32)),
        Band::curved(p2(-36), p2(-34)),
    ]
    .map(|b| b.expect("a band"));
    for seed in varde_testing::seeds(12, 256) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let op = if seed % 3 == 0 { A } else { B };
        let mut g = Geom::with(DIRECTIONS[(seed / 2) as usize % 5], sigmas(seed));
        let c = lat(rng.range(-1.0, 1.0), 0.0, rng.range(-1.0, 1.0));
        let spans: Vec<Span> = circle(c, 1.0 + rng.range(0.0, 2.0), 4, rng.range(0.0, 1.0))
            .iter()
            .map(|s| {
                let mut pick = || UNEVEN[(rng.next_u64() % 4) as usize];
                reweighted(s, pick(), pick())
            })
            .collect();
        g.edge(op, 0, 0, 1, &spans[0..2]);
        g.edge(op, 1, 1, 0, &spans[2..4]);
        let mut qs = Vec::new();
        for (i, s) in spans.iter().enumerate() {
            let j = point_of(&s[0]);
            for k in 0..4 {
                let mut off = || match rng.next_u64() % 4 {
                    0 => 0.0,
                    1 => rng.range(-4.0, 4.0) * f64::EPSILON,
                    _ => rng.range(-4.0, 4.0) * p2(-((rng.next_u64() % 40) as i32)),
                };
                let (a, b) = (off(), off());
                let m = [0.0, 1.5, -2.0][(rng.next_u64() % 3) as usize];
                let at = j + R_HAT * a + NU * (b / 32.0) + UP * m;
                qs.push(g.query(op.other(), 100 + 4 * i as u32 + k, at));
            }
        }
        let recs = bands.map(|b| fill_at(&g, &qs, b).expect("decided"));
        for &q in &qs {
            check(&recs[0], &g, q, true, &format!("seed {seed}"));
            for (e, n) in [(EdgeId(0), 2), (EdgeId(1), 2)] {
                for i in 0..n {
                    let read = |r: &CurvedRays| -> Vec<(i8, bool)> {
                        r.span(q, e, i)
                            .expect("stored")
                            .crossings
                            .iter()
                            .map(|c| (c.sign.as_i8(), c.single))
                            .collect()
                    };
                    let first = read(&recs[0]);
                    for r in &recs[1..] {
                        assert_eq!(read(r), first, "seed {seed}, {q:?}, {e:?} span {i}");
                    }
                }
            }
        }
    }
}
