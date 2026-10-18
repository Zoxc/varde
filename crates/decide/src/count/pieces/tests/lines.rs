//! Trims lying on box lines, against the exact winding: straight p-curves
//! along box sides, tile lines and the run's own outer sides, crossing a
//! side's line at the end of a side, turning at a box corner (inside the
//! run or on its boundary, on the tile line or off it), passing a box
//! corner at a knot, with flipped faces and coedges against their edges,
//! rational spans along a line and signed zeros on it; a slit whose two
//! sides differ in the signs of their zeros; random polylines through
//! the points of a grid finer than the boxes'. Each edge's curve is its
//! p-curve's image (no ribbon width), so a run's degree is `±1` where the
//! query's shadow lies in the run and the face, else 0, and runs of any
//! partition sum to it.

use super::*;

/// A one-coedge trim: a straight polyline in UV, one span per leg, the
/// middle control point's weight `wt`; its edge's curve the image.
struct Poly {
    spans: Vec<UvSpan>,
    curve: Vec<Span>,
    maps: Vec<SpanMap>,
}

impl Poly {
    fn new(pts: &[[f64; 2]], wt: f64) -> Poly {
        let mut spans = Vec::new();
        let mut curve = Vec::new();
        for leg in pts.windows(2) {
            let (a, b) = (leg[0], leg[1]);
            // The middle point keeps a coordinate the legs share exactly
            // (a signed zero included).
            let m = [0, 1].map(|c| {
                if a[c] == b[c] {
                    b[c]
                } else {
                    (a[c] + b[c]) / 2.0
                }
            });
            spans.push([[a[0], a[1], 1.0], [m[0], m[1], wt], [b[0], b[1], 1.0]]);
            curve.push([a, m, b].map(|p| [SU * p[0], SV * p[1], 0.0, 1.0]));
            curve.last_mut().unwrap()[1][3] = wt;
        }
        let maps = (0..spans.len() as u32)
            .map(|span| SpanMap {
                span,
                t: [0.0, 1.0],
            })
            .collect();
        Poly { spans, curve, maps }
    }

    /// One coedge of the rational quadratic p-spans `spans` (`[u, v,
    /// w]`); its edge's curve the image.
    fn spans(spans: &[UvSpan]) -> Poly {
        let curve = spans
            .iter()
            .map(|sp| sp.map(|p| [SU * p[0], SV * p[1], 0.0, p[2]]))
            .collect();
        let maps = (0..spans.len() as u32)
            .map(|span| SpanMap {
                span,
                t: [0.0, 1.0],
            })
            .collect();
        Poly {
            spans: spans.to_vec(),
            curve,
            maps,
        }
    }

    /// Points along the p-curve, 256 per span, in UV.
    fn samples(&self) -> Vec<[f64; 2]> {
        let mut out = Vec::new();
        for sp in &self.spans {
            for i in 0..=256 {
                let t = f64::from(i) / 256.0;
                let b = [(1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t];
                let w: f64 = (0..3).map(|k| b[k] * sp[k][2]).sum();
                out.push(
                    [0, 1].map(|c| (0..3).map(|k| b[k] * sp[k][2] * sp[k][c]).sum::<f64>() / w),
                );
            }
        }
        out
    }

    /// Whether `p` lies within `m` of the p-curve in the plane.
    fn near(&self, p: [f64; 2], m: f64) -> bool {
        self.samples().windows(2).any(|l| {
            let (a, c) = (l[0], l[1]);
            let (dx, dy) = (SU * (c[0] - a[0]), SV * (c[1] - a[1]));
            let (px, py) = (SU * (p[0] - a[0]), SV * (p[1] - a[1]));
            let len2 = dx * dx + dy * dy;
            let s = ((px * dx + py * dy) / len2).clamp(0.0, 1.0);
            let (ex, ey) = (px - s * dx, py - s * dy);
            (ex * ex + ey * ey).sqrt() < m
        })
    }

    /// Whether `p` lies left of the p-curve where it is nearest to it.
    fn left(&self, p: [f64; 2]) -> bool {
        let s = self.samples();
        let d2 = |a: [f64; 2]| {
            let d = [a[0] - p[0], a[1] - p[1]];
            d[0] * d[0] + d[1] * d[1]
        };
        let i = (0..s.len() - 1)
            .min_by(|&i, &j| d2(s[i]).total_cmp(&d2(s[j])))
            .expect("samples");
        let (a, c) = (s[i], s[i + 1]);
        (c[0] - a[0]) * (p[1] - a[1]) - (c[1] - a[1]) * (p[0] - a[0]) > 0.0
    }

    fn degree(
        &self,
        reversed: bool,
        flipped: bool,
        boxes: &[BoxKey],
        q: [f64; 3],
    ) -> Result<PieceDegree, PieceError> {
        let trims = [trim(&self.spans, 1, reversed)];
        let face = FaceUv::new(Operand::A, FaceId(3), TILES, [1.0, 1.0], &trims, flipped).unwrap();
        let rec = rational_rays(&self.curve, q);
        let ribbons = [Ribbon {
            coedge: CoedgeId(1),
            curve: &self.curve,
            map: &self.maps,
            rays: &rec,
        }];
        settle(q, |sides| {
            strip_degree(
                &face,
                &patches(),
                boxes,
                &ribbons,
                &QShadow::of(q).unwrap(),
                point(),
                sides,
            )
        })
    }
}

/// [`rays`] for rational quadratic spans: per span the roots of
/// `Σ wᵢ bᵢ(t) ν·(Pᵢ − q)`, those ahead of `q` along `r̂`.
fn rational_rays(c: &[Span], q: [f64; 3]) -> ChainRec {
    let dot = |a: [f64; 3], p: [f64; 4]| a[0] * p[0] + a[1] * p[1] + a[2] * p[2];
    let (r, n) = ([R_HAT.x, R_HAT.y, R_HAT.z], [NU.x, NU.y, NU.z]);
    let q4 = [q[0], q[1], q[2], 1.0];
    let mut out = ChainRec::default();
    for (k, sp) in c.iter().enumerate() {
        // The weighted Bernstein coefficients of `a·(C(t) − q)` in the
        // power basis.
        let f = |a: [f64; 3]| {
            let [p0, p1, p2] = sp.map(|p| p[3] * (dot(a, p) - dot(a, q4)));
            [p0, 2.0 * (p1 - p0), p0 - 2.0 * p1 + p2]
        };
        let [c0, b, a] = f(n);
        let mut ts: Vec<f64> = Vec::new();
        if a == 0.0 {
            ts.push(-c0 / b);
        } else {
            let disc = b * b - 4.0 * a * c0;
            if disc > 0.0 {
                let h = -(b + b.signum() * disc.sqrt()) / 2.0;
                ts.extend([h / a, c0 / h]);
            }
        }
        ts.sort_by(f64::total_cmp);
        let [rc, rb, ra] = f(r);
        for t in ts {
            if !(0.0..=1.0).contains(&t) || rc + rb * t + ra * t * t <= 0.0 {
                continue;
            }
            let sign = if b + 2.0 * a * t > 0.0 {
                Sign::Pos
            } else {
                Sign::Neg
            };
            out.crossings.push(ChainCrossing {
                span: k as u32,
                crossing: Crossing {
                    t: [t - 1e-12, t + 1e-12],
                    sign,
                    single: true,
                },
            });
        }
    }
    out
}

/// The grid of six boxes: columns `u ∈ [½, ¾], [¾, 1]` of tile 0 and
/// `[0, ½]` of tile 1, rows `v ∈ rows[0], rows[1]`.
fn grid(rows: [Extent; 2]) -> Vec<BoxKey> {
    let cols = [
        (0, ext((1, 1), (3, 2))),
        (0, ext((3, 2), (1, 0))),
        (1, ext((0, 0), (1, 1))),
    ];
    let mut out = Vec::new();
    for v in rows {
        for (i, u) in cols {
            out.push(bx(i, u, v));
        }
    }
    out
}

/// Partitions of the grid (boxes `0..3` the low row, left to right).
fn grid_parts(g: &[BoxKey]) -> Vec<Vec<Vec<BoxKey>>> {
    let pick = |ix: &[usize]| ix.iter().map(|&i| g[i]).collect::<Vec<_>>();
    vec![
        vec![g.to_vec()],
        vec![pick(&[0, 1, 2]), pick(&[3, 4, 5])],
        vec![pick(&[0, 3]), pick(&[1, 4]), pick(&[2, 5])],
        vec![pick(&[0, 3, 1, 4]), pick(&[2, 5])],
        vec![pick(&[0, 3]), pick(&[1, 4, 2, 5])],
        vec![pick(&[0, 1, 3]), pick(&[2, 4, 5])],
        (0..6).map(|i| pick(&[i])).collect(),
    ]
}

/// The query's shadow in UV.
fn uv(q: [f64; 3]) -> [f64; 2] {
    let p = plane_xy(q);
    [p[0] / SU, p[1] / SV]
}

/// A case: the polyline, the face's side of it (for the polyline's own
/// direction on an unflipped face), the run's rows, and the partitions
/// ([`grid_parts`]) that settle at every query: every part meets the
/// polyline, or has a span along its side, or a knot or an end on it.
struct Case {
    name: &'static str,
    pts: Vec<[f64; 2]>,
    face: fn([f64; 2]) -> bool,
    rows: [Extent; 2],
    settle: &'static [usize],
}

fn cases() -> Vec<Case> {
    let mid = [ext((1, 2), (1, 1)), ext((1, 1), (3, 2))];
    let low = [ext((0, 0), (1, 2)), ext((1, 2), (1, 1))];
    vec![
        Case {
            name: "across the low side at a side's end",
            pts: vec![[0.75, 0.1], [0.75, 0.9]],
            face: |p| p[0] < 0.75,
            rows: mid,
            settle: &[0, 1, 4, 5],
        },
        Case {
            name: "along the tile line",
            pts: vec![[1.0, 0.1], [1.0, 0.9]],
            face: |p| p[0] < 1.0,
            rows: mid,
            settle: &[0, 1, 3, 5],
        },
        Case {
            name: "along the box line between the rows",
            pts: vec![[0.1, 0.5], [1.0, 0.5], [1.9, 0.5]],
            face: |p| p[1] > 0.5,
            rows: mid,
            settle: &[0, 1, 2, 3, 4, 5, 6],
        },
        Case {
            name: "along the run's low side",
            pts: vec![[0.1, 0.25], [1.0, 0.25], [1.9, 0.25]],
            face: |p| p[1] > 0.25,
            rows: mid,
            settle: &[0, 2, 3, 4, 5],
        },
        Case {
            name: "left turn at an inner box corner",
            pts: vec![[0.1, 0.5], [0.75, 0.5], [0.75, 0.9]],
            face: |p| p[1] > 0.5 && p[0] < 0.75,
            rows: mid,
            settle: &[0, 1, 4, 5],
        },
        Case {
            name: "right turn at an inner box corner",
            pts: vec![[0.1, 0.5], [0.75, 0.5], [0.75, 0.1]],
            face: |p| p[1] > 0.5 || p[0] > 0.75,
            rows: mid,
            settle: &[0, 1, 4, 5],
        },
        Case {
            name: "transversal knot at an inner box corner",
            pts: vec![[0.6, 0.2], [0.75, 0.5], [1.65, 0.95]],
            face: |p| {
                let v = if p[0] < 0.75 {
                    0.5 + 2.0 * (p[0] - 0.75)
                } else {
                    0.5 + 0.5 * (p[0] - 0.75)
                };
                p[1] > v
            },
            rows: mid,
            settle: &[0, 1, 2, 3, 4, 5],
        },
        Case {
            name: "diagonal through box corners, clipping boxes there",
            pts: vec![[0.375, 0.125], [1.0, 0.75], [1.125, 0.875]],
            face: |p| p[1] > p[0] - 0.25,
            rows: mid,
            settle: &[0, 1, 2, 3, 4, 5],
        },
        Case {
            name: "left turn at a tile corner",
            pts: vec![[0.1, 0.5], [1.0, 0.5], [1.0, 0.9]],
            face: |p| p[1] > 0.5 && p[0] < 1.0,
            rows: mid,
            settle: &[0, 1, 2, 3, 4, 5, 6],
        },
        Case {
            name: "right turn at a tile corner",
            pts: vec![[0.1, 0.5], [1.0, 0.5], [1.0, 0.1]],
            face: |p| p[1] > 0.5 || p[0] > 1.0,
            rows: mid,
            settle: &[0, 1, 2, 3, 4, 5, 6],
        },
        Case {
            name: "turn at a corner of the run's low side",
            pts: vec![[0.1, 0.25], [0.75, 0.25], [0.75, 0.9]],
            face: |p| p[1] > 0.25 && p[0] < 0.75,
            rows: mid,
            settle: &[0, 1, 4, 5],
        },
        Case {
            name: "turn at the run's low left corner",
            pts: vec![[0.1, 0.25], [0.5, 0.25], [0.5, 0.9]],
            face: |p| p[1] > 0.25 && p[0] < 0.5,
            rows: mid,
            settle: &[0, 1],
        },
        Case {
            name: "along the tile's low side, signed zeros",
            pts: vec![[0.1, 0.0], [1.0, -0.0], [1.9, 0.0]],
            face: |p| p[1] > 0.0,
            rows: low,
            settle: &[0, 2, 3, 4, 5],
        },
        Case {
            name: "along the tile's low side, negative zeros",
            pts: vec![[0.1, -0.0], [1.0, -0.0], [1.9, -0.0]],
            face: |p| p[1] > 0.0,
            rows: low,
            settle: &[0, 2, 3, 4, 5],
        },
    ]
}

/// Checks every case with every weight and orientation (reversed,
/// flipped) over every partition at `n` queries: certified sums are the
/// exact degree, and the case's settling partitions settle at every query
/// away from the lines (a part clear of the trim with nothing on its
/// boundary doesn't say whether it is inside).
fn check_cases(weights: &[f64], orients: &[(bool, bool)], n: usize) {
    let mut rng = Rng::new(900);
    for case in cases() {
        let g = grid(case.rows);
        let lo = case.rows[0].lo();
        let hi = case.rows[1].hi();
        let (vlo, vhi) = (dy(lo), dy(hi));
        let in_run = |p: [f64; 2]| p[0] > 0.5 && p[0] < 1.5 && p[1] > vlo && p[1] < vhi;
        // Within `1e-3` (in the plane) of the run's sides or the
        // polyline's lines.
        let near = |p: [f64; 2]| {
            // The distance in the plane to each leg's line.
            let legs = case.pts.windows(2).map(|leg| {
                let (a, b) = (leg[0], leg[1]);
                let (dx, dy) = (SU * (b[0] - a[0]), SV * (b[1] - a[1]));
                let (px, py) = (SU * (p[0] - a[0]), SV * (p[1] - a[1]));
                (dx * py - dy * px) / (dx * dx + dy * dy).sqrt()
            });
            [
                SU * (p[0] - 0.5),
                SU * (p[0] - 1.5),
                SV * (p[1] - vlo),
                SV * (p[1] - vhi),
            ]
            .into_iter()
            .chain(legs)
            .any(|d| d.abs() < 1e-3)
        };
        let qs: Vec<[f64; 3]> = (0..n)
            .map(|_| {
                [
                    rng.range(1.0, 9.0),
                    rng.range(SV * vlo - 1.0, SV * vhi + 1.0),
                    rng.range(-1.0, 1.0),
                ]
            })
            .collect();
        for &wt in weights {
            let poly = Poly::new(&case.pts, wt);
            for &(reversed, flipped) in orients {
                let sign = if flipped { -1 } else { 1 };
                for (k, parts) in grid_parts(&g).iter().enumerate() {
                    for &q in &qs {
                        let p = uv(q);
                        if near(p) {
                            continue;
                        }
                        let inside = in_run(p) && ((case.face)(p) != (reversed != flipped));
                        let mut sum = Some(0);
                        for part in parts {
                            match poly.degree(reversed, flipped, part, q) {
                                Ok(d) => sum = sum.map(|x| x + d.degree),
                                Err(PieceError::Unsettled | PieceError::Through) => sum = None,
                                Err(e) => panic!(
                                    "{}: {e:?} wt {wt} rev {reversed} flip {flipped} \
                                     partition {k} q {q:?}",
                                    case.name
                                ),
                            }
                        }
                        assert!(
                            sum.is_some() || !case.settle.contains(&k),
                            "{}: unsettled, wt {wt} rev {reversed} flip {flipped} \
                             partition {k} q {q:?}",
                            case.name
                        );
                        if let Some(sum) = sum {
                            assert_eq!(
                                sum,
                                if inside { sign } else { 0 },
                                "{}: wt {wt} rev {reversed} flip {flipped} partition {k} \
                                 q {q:?}",
                                case.name
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn trims_on_box_lines_are_the_winding() {
    let all = [(false, false), (true, false), (false, true), (true, true)];
    check_cases(&[1.0], &all, 16);
}

#[test]
fn rational_trims_on_box_lines_are_the_winding() {
    check_cases(&[1.0 / 64.0, 64.0], &[(false, false), (true, true)], 8);
}

/// A slit (two coedges of one edge, the face on both sides) whose p-curves
/// differ only in the signs of their zeros: one curve all the same, its
/// ribbons cancel, and runs of every partition sum to the winding of the
/// run's boundary.
#[test]
fn a_slit_with_signed_zeros_counts() {
    let poly = Poly::new(&[[0.2, 0.0], [1.0, 0.4], [1.8, 0.0]], 1.0);
    let mut other = poly.spans.clone();
    for sp in &mut other {
        for p in sp.iter_mut() {
            if p[1] == 0.0 {
                p[1] = -0.0;
            }
        }
    }
    let rows = [ext((0, 0), (1, 2)), ext((1, 2), (1, 1))];
    let g = grid(rows);
    let mut rng = Rng::new(910);
    let mut settled = 0;
    for _ in 0..24 {
        let q = [
            rng.range(1.0, 9.0),
            rng.range(-1.0, 6.0),
            rng.range(-1.0, 1.0),
        ];
        let p = uv(q);
        let edge = |x: f64, lo: f64, hi: f64| (x - lo).abs() < 1e-3 || (x - hi).abs() < 1e-3;
        if edge(p[0], 0.5, 1.5) || edge(p[1], 0.0, 0.5) {
            continue;
        }
        let in_run = p[0] > 0.5 && p[0] < 1.5 && p[1] > 0.0 && p[1] < 0.5;
        let trims = [trim(&poly.spans, 1, false), trim(&other, 2, true)];
        let face = FaceUv::new(Operand::A, FaceId(3), TILES, [1.0, 1.0], &trims, false).unwrap();
        let rec = rational_rays(&poly.curve, q);
        let ribbons = [1, 2].map(|c| Ribbon {
            coedge: CoedgeId(c),
            curve: &poly.curve,
            map: &poly.maps,
            rays: &rec,
        });
        for (k, parts) in grid_parts(&g).iter().enumerate() {
            let mut sum = Some(0);
            for part in parts {
                let d = settle(q, |sides| {
                    strip_degree(
                        &face,
                        &patches(),
                        part,
                        &ribbons,
                        &QShadow::of(q).unwrap(),
                        point(),
                        sides,
                    )
                });
                match d {
                    Ok(d) => sum = sum.map(|x| x + d.degree),
                    Err(PieceError::Unsettled | PieceError::Through) => sum = None,
                    Err(e) => panic!("{e:?} partition {k} q {q:?}"),
                }
            }
            if let Some(sum) = sum {
                settled += 1;
                assert_eq!(sum, i64::from(in_run), "partition {k} q {q:?}");
            }
        }
    }
    assert!(settled >= 100, "{settled}");
}

/// Whether `p` lies inside the polygon `ring` (even-odd).
fn in_ring(ring: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut odd = false;
    for k in 0..ring.len() {
        let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if p[0] < x {
                odd = !odd;
            }
        }
    }
    odd
}

/// Random polylines through points of the eighth grid (every box line of
/// the grid's boxes and the lines between them), `u` never decreasing
/// (vertical legs on box lines included), from `u = 0.1` to `u = 1.9`,
/// `v` from `vs`: knots and legs on box lines, legs through box corners
/// between knots, turns at box corners. The face lies above the polyline;
/// certified sums over every partition of the grid with rows `rows` are
/// the exact degree, and at least half the sums settle.
fn check_polylines(rows: [Extent; 2], vs: &[f64], seed: u64, cases: usize) {
    let us = [0.5, 0.625, 0.75, 0.875, 1.0, 1.25, 1.5];
    let mut rng = Rng::new(seed);
    let pick = |rng: &mut Rng, xs: &[f64]| xs[(rng.next_u64() % xs.len() as u64) as usize];
    let g = grid(rows);
    let (vlo, vhi) = (dy(rows[0].lo()), dy(rows[1].hi()));
    let (mut settled, mut all) = (0usize, 0usize);
    for case in 0..cases {
        let n = 2 + (rng.next_u64() % 3) as usize;
        let mut mid: Vec<f64> = (0..n).map(|_| pick(&mut rng, &us)).collect();
        mid.sort_by(f64::total_cmp);
        let mut pts = vec![[0.1, pick(&mut rng, vs)]];
        for u in mid {
            let v = pick(&mut rng, vs);
            let last = *pts.last().unwrap();
            // No zero-length leg, and at most one vertical leg in a row.
            let vertical = pts.len() >= 2 && pts[pts.len() - 2][0] == u;
            if last == [u, v] || (last[0] == u && vertical) {
                continue;
            }
            pts.push([u, v]);
        }
        pts.push([1.9, pick(&mut rng, vs)]);
        // Above the polyline: closed over the top.
        let mut ring = pts.clone();
        ring.extend([[1.9, 2.0], [0.1, 2.0]]);
        let near = |p: [f64; 2]| {
            let legs = pts.windows(2).map(|leg| {
                let (a, b) = (leg[0], leg[1]);
                let (dx, dy) = (SU * (b[0] - a[0]), SV * (b[1] - a[1]));
                let (px, py) = (SU * (p[0] - a[0]), SV * (p[1] - a[1]));
                (dx * py - dy * px) / (dx * dx + dy * dy).sqrt()
            });
            [
                SU * (p[0] - 0.5),
                SU * (p[0] - 1.5),
                SV * (p[1] - vlo),
                SV * (p[1] - vhi),
            ]
            .into_iter()
            .chain(legs)
            .any(|d| d.abs() < 1e-3)
        };
        let wt = [1.0, 1.0 / 64.0, 64.0][case % 3];
        let poly = Poly::new(&pts, wt);
        let (reversed, flipped) = (case % 2 == 1, case % 4 >= 2);
        let sign = if flipped { -1 } else { 1 };
        for _ in 0..6 {
            let q = [
                rng.range(2.0, 8.0),
                rng.range(SV * vlo - 1.0, SV * vhi + 1.0),
                rng.range(-1.0, 1.0),
            ];
            let p = uv(q);
            if near(p) {
                continue;
            }
            let in_run = p[0] > 0.5 && p[0] < 1.5 && p[1] > vlo && p[1] < vhi;
            let inside = in_run && (in_ring(&ring, p) != (reversed != flipped));
            for (k, parts) in grid_parts(&g).iter().enumerate() {
                all += 1;
                let mut sum = Some(0);
                for part in parts {
                    match poly.degree(reversed, flipped, part, q) {
                        Ok(d) => sum = sum.map(|x| x + d.degree),
                        Err(PieceError::Unsettled | PieceError::Through) => sum = None,
                        Err(e) => panic!("{pts:?}: {e:?} wt {wt} partition {k} q {q:?}"),
                    }
                }
                if let Some(sum) = sum {
                    settled += 1;
                    assert_eq!(
                        sum,
                        if inside { sign } else { 0 },
                        "{pts:?}: wt {wt} rev {reversed} flip {flipped} partition {k} q {q:?}"
                    );
                }
            }
        }
    }
    assert!(settled * 2 >= all, "{settled} of {all}");
}

#[test]
fn polylines_on_the_eighth_grid_are_the_winding() {
    let rows = [ext((1, 2), (1, 1)), ext((1, 1), (3, 2))];
    let vs = [0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875];
    check_polylines(rows, &vs, 930, 48);
}

/// The same on the tiles' low edge (`v = 0`, read as moved down), the
/// polylines reaching down to it.
#[test]
fn polylines_on_the_tiles_low_edge_are_the_winding() {
    let rows = [ext((0, 0), (1, 2)), ext((1, 2), (1, 1))];
    let vs = [0.0, 0.0, 0.125, 0.25, 0.375, 0.5, 0.625];
    check_polylines(rows, &vs, 940, 48);
}

/// A closed trim of four conic quarters, its knots on box lines and
/// tangent to them there (a circle's quadrant points): on the run's left,
/// low and top sides and on the tile line, the curve touching each from
/// inside the run. Read moved, each side's line cuts the curve twice next
/// to the knot (at `±√δ` along it): every partition of the grid settles
/// at the first depth and sums to the winding.
#[test]
fn knots_tangent_to_box_lines_settle() {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let quarter = |a: [f64; 2], c: [f64; 2], b: [f64; 2]| {
        [[a[0], a[1], 1.0], [c[0], c[1], w], [b[0], b[1], 1.0]]
    };
    let spans = vec![
        quarter([1.0, 0.5], [1.0, 0.75], [0.75, 0.75]),
        quarter([0.75, 0.75], [0.5, 0.75], [0.5, 0.5]),
        quarter([0.5, 0.5], [0.5, 0.25], [0.75, 0.25]),
        quarter([0.75, 0.25], [1.0, 0.25], [1.0, 0.5]),
    ];
    let curve: Vec<Span> = spans
        .iter()
        .map(|s| s.map(|p| [SU * p[0], SV * p[1], 0.0, p[2]]))
        .collect();
    let maps: Vec<SpanMap> = (0..4)
        .map(|span| SpanMap {
            span,
            t: [0.0, 1.0],
        })
        .collect();
    // The curve in UV, finely: the exact circle of radius ¼ round
    // `(¾, ½)`.
    let inside = |p: [f64; 2]| {
        let (du, dv) = (p[0] - 0.75, p[1] - 0.5);
        let d = (du * du + dv * dv).sqrt();
        (d - 0.25).abs() > 1e-3 && d < 0.25
    };
    let near = |p: [f64; 2]| {
        let (du, dv) = (p[0] - 0.75, p[1] - 0.5);
        let d = (du * du + dv * dv).sqrt();
        let line = |x: f64, s: f64| ((x * 4.0).round() / 4.0 - x).abs() * s < 1e-3;
        (d - 0.25).abs() < 1e-3 || line(p[0], SU) || line(p[1], SV)
    };
    let rows = [ext((1, 2), (1, 1)), ext((1, 1), (3, 2))];
    let g = grid(rows);
    let mut rng = Rng::new(950);
    let mut checked = 0;
    for (reversed, flipped) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut t = trim(&spans, 1, reversed);
        t.ends = [VertId(1), VertId(1)];
        let trims = [t];
        let face = FaceUv::new(Operand::A, FaceId(3), TILES, [1.0, 1.0], &trims, flipped).unwrap();
        // Inside the circle: the face where the coedge runs round it
        // counter-clockwise on an unflipped face, or clockwise on a
        // flipped one.
        let disc = reversed == flipped;
        let sign = if flipped { -1 } else { 1 };
        for _ in 0..12 {
            let q = [
                rng.range(2.0, 8.0),
                rng.range(1.5, 8.5),
                rng.range(-1.0, 1.0),
            ];
            let p = uv(q);
            if near(p) {
                continue;
            }
            let in_run = p[0] > 0.5 && p[0] < 1.5 && p[1] > 0.25 && p[1] < 0.75;
            let want = if in_run && inside(p) == disc { sign } else { 0 };
            let rec = rational_rays(&curve, q);
            let ribbons = [Ribbon {
                coedge: CoedgeId(1),
                curve: &curve,
                map: &maps,
                rays: &rec,
            }];
            for (k, parts) in grid_parts(&g).iter().enumerate() {
                let mut sum = 0;
                for part in parts {
                    let d = settle(q, |sides| {
                        strip_degree(
                            &face,
                            &patches(),
                            part,
                            &ribbons,
                            &QShadow::of(q).unwrap(),
                            point(),
                            sides,
                        )
                    });
                    match d {
                        Ok(d) => sum += d.degree,
                        Err(e) => panic!(
                            "{e:?} rev {reversed} flip {flipped} partition {k} part {part:?} \
                             q {q:?}"
                        ),
                    }
                }
                assert_eq!(
                    sum, want,
                    "rev {reversed} flip {flipped} partition {k} q {q:?}"
                );
                checked += 1;
            }
        }
    }
    assert!(checked >= 150, "{checked}");
}

/// A closed straight triangle through the reflex corner `(¾, ½)` of an
/// L-shaped run (the boxes `[½, 1] × [¼, ½]` and `[¾, 1] × [½, ¾]`),
/// otherwise inside the run: read moved, its side of slope `3/2` (above
/// `√2`) leaves the run through the low row's top and comes back through
/// the high box's left side next to the corner, its only cuts (the run
/// holds all of it but there). The box `[½, ¾] × [½, ¾]` above the corner
/// is clipped there (in and out next to the corner). Each part's degree
/// is the winding of its part of the face.
#[test]
fn a_closed_trim_through_a_reflex_corner() {
    let pts = [[0.625, 0.3125], [0.875, 0.6875], [0.875, 0.3125]];
    let tri = |p: [f64; 2]| {
        // Inside the triangle, `None` within `1e-3` of a side.
        let d = [
            1.5 * (p[0] - 0.625) - (p[1] - 0.3125),
            0.875 - p[0],
            p[1] - 0.3125,
        ];
        if d.iter().any(|x| x.abs() < 1e-3) {
            None
        } else {
            Some(d.iter().all(|&x| x > 0.0))
        }
    };
    let mut spans = Vec::new();
    for k in 0..3 {
        let (a, b) = (pts[k], pts[(k + 1) % 3]);
        let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        spans.push([a, m, b].map(|p| [p[0], p[1], 1.0]));
    }
    let curve: Vec<Span> = spans
        .iter()
        .map(|s| s.map(|p| [SU * p[0], SV * p[1], 0.0, 1.0]))
        .collect();
    let maps: Vec<SpanMap> = (0..3)
        .map(|span| SpanMap {
            span,
            t: [0.0, 1.0],
        })
        .collect();
    let g = grid([ext((1, 2), (1, 1)), ext((1, 1), (3, 2))]);
    let parts = [vec![g[0], g[1], g[4]], vec![g[3]]];
    let in_part = |part: &[BoxKey], p: [f64; 2]| {
        part.iter().any(|b| {
            let t = b.tile()[0] as f64;
            let (u, v) = (b.extent(Dir::U), b.extent(Dir::V));
            p[0] > t + dy(u.lo()) && p[0] < t + dy(u.hi()) && p[1] > dy(v.lo()) && p[1] < dy(v.hi())
        })
    };
    let mut rng = Rng::new(960);
    let mut checked = 0;
    for (reversed, flipped) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut t = trim(&spans, 1, reversed);
        t.ends = [VertId(1), VertId(1)];
        let trims = [t];
        let face = FaceUv::new(Operand::A, FaceId(3), TILES, [1.0, 1.0], &trims, flipped).unwrap();
        // The triangle runs clockwise: the face is inside it where the
        // coedge runs against it on an unflipped face.
        let disc = reversed != flipped;
        let sign = if flipped { -1 } else { 1 };
        for _ in 0..40 {
            let q = [
                rng.range(2.5, 5.0),
                rng.range(2.5, 7.5),
                rng.range(-1.0, 1.0),
            ];
            let p = uv(q);
            let Some(inside) = tri(p) else {
                continue;
            };
            let line = |x: f64, s: f64| ((x * 4.0).round() / 4.0 - x).abs() * s < 1e-3;
            if line(p[0], SU) || line(p[1], SV) {
                continue;
            }
            let rec = rational_rays(&curve, q);
            let ribbons = [Ribbon {
                coedge: CoedgeId(1),
                curve: &curve,
                map: &maps,
                rays: &rec,
            }];
            for part in &parts {
                let want = if in_part(part, p) && inside == disc {
                    sign
                } else {
                    0
                };
                let d = settle(q, |sides| {
                    strip_degree(
                        &face,
                        &patches(),
                        part,
                        &ribbons,
                        &QShadow::of(q).unwrap(),
                        point(),
                        sides,
                    )
                });
                match d {
                    Ok(d) => assert_eq!(
                        d.degree, want,
                        "rev {reversed} flip {flipped} part {part:?} q {q:?}"
                    ),
                    Err(e) => panic!("{e:?} rev {reversed} flip {flipped} part {part:?} q {q:?}"),
                }
                checked += 1;
            }
        }
    }
    assert!(checked >= 200, "{checked}");
}

/// A box a trim passes exactly through one corner of, not at a knot:
/// alone, its loop is cut by no curve where the trim leaves the box
/// outside (the moved corner on the box's side), and the face's sector at
/// the corner, the span split there exactly, says whether it is inside;
/// else the trim clips the moved corner. Either way the box settles at
/// every query away from the lines, with the winding.
#[test]
fn a_trim_through_a_box_corner_only_settles() {
    // Lines through each corner, slope `±1`, the box on one side only,
    // with the face's side for the polyline's own direction.
    let lines: [(&str, Vec<[f64; 2]>, Side); 5] = [
        (
            "low right corner",
            vec![[0.375, 0.125], [1.0, 0.75], [1.125, 0.875]],
            |p| p[1] > p[0] - 0.25,
        ),
        (
            "low left corner",
            vec![[0.875, 0.125], [0.125, 0.875], [0.0625, 0.9375]],
            |p| p[1] < 1.0 - p[0],
        ),
        (
            "high left corner",
            vec![[0.125, 0.375], [0.625, 0.875], [0.6875, 0.9375]],
            |p| p[1] > p[0] + 0.25,
        ),
        (
            "high right corner",
            vec![[0.625, 0.875], [0.875, 0.625], [0.9375, 0.5625]],
            |p| p[1] > 1.5 - p[0],
        ),
        // The corner a third of the way along a span.
        ("low right, uneven", vec![[0.5, 0.25], [1.25, 1.0]], |p| {
            p[1] > p[0] - 0.25
        }),
    ];
    // The face's side: the polyline's left, or the curve's own.
    let mut cases: Vec<(String, Poly, Option<Side>)> = Vec::new();
    for (name, pts, face) in lines {
        for wt in [1.0, 1.0 / 64.0, 64.0] {
            cases.push((format!("{name}, wt {wt}"), Poly::new(&pts, wt), Some(face)));
        }
    }
    // Arcs through the low right corner at their middle, tangent to
    // `v = u − ¼` there and below it: the moved corner above, the box
    // uncut; and a steeper one, slope 2.5, clipping the moved corner.
    for (name, sp) in [
        (
            "parabola",
            [[0.5, 0.125, 1.0], [0.75, 0.625, 1.0], [1.0, 0.625, 1.0]],
        ),
        (
            "conic",
            [[0.5, 0.125, 1.0], [0.75, 0.5625, 2.0], [1.0, 0.625, 1.0]],
        ),
        (
            "steep parabola",
            [[0.5, 0.25, 1.0], [0.875, 0.4375, 1.0], [0.75, 0.875, 1.0]],
        ),
    ] {
        cases.push((name.into(), Poly::spans(&[sp]), None));
    }
    check_box(&cases, 901);
}

/// Whether a UV point is on the face's side of a line.
type Side = fn([f64; 2]) -> bool;

/// Checks the box `[½, ¾] × [½, ¾]` of tile 0 alone against each case's
/// trim (the face on its side's given side, or the curve's left) at
/// queries over the box and round it, away from the lines: it settles,
/// with the winding.
fn check_box(cases: &[(String, Poly, Option<Side>)], seed: u64) {
    let b = bx(0, ext((1, 1), (3, 2)), ext((1, 1), (3, 2)));
    let mut rng = Rng::new(seed);
    let qs: Vec<[f64; 3]> = std::iter::from_fn(|| {
        Some([
            rng.range(1.0, 6.0),
            rng.range(3.0, 9.0),
            rng.range(-1.0, 1.0),
        ])
    })
    .filter(|&q| {
        let p = uv(q);
        (0.4..0.85).contains(&p[0]) && (0.4..0.85).contains(&p[1])
    })
    .take(24)
    .collect();
    for (name, poly, face) in cases {
        for (reversed, flipped) in [(false, false), (true, false), (false, true), (true, true)] {
            let sign = if flipped { -1 } else { 1 };
            for &q in &qs {
                let p = uv(q);
                let lines = [p[0] - 0.5, p[0] - 0.75, p[1] - 0.5, p[1] - 0.75];
                if poly.near(p, 1e-3) || lines.iter().any(|d| d.abs() < 1e-3) {
                    continue;
                }
                let in_box = lines[0] > 0.0 && lines[1] < 0.0 && lines[2] > 0.0 && lines[3] < 0.0;
                let left = face.map_or_else(|| poly.left(p), |f| f(p));
                let inside = in_box && (left != (reversed != flipped));
                let d = poly.degree(reversed, flipped, &[b], q).unwrap_or_else(|e| {
                    panic!("{name}: {e:?} rev {reversed} flip {flipped} q {q:?}")
                });
                assert_eq!(
                    d.degree,
                    if inside { sign } else { 0 },
                    "{name}: rev {reversed} flip {flipped} q {q:?}"
                );
            }
        }
    }
}

/// A box whose side a trim touches inside it, tangent to its line at a
/// double root of the span's equation: on the side the line is moved
/// away from, the loop is uncut and the face's sector at the touching
/// point says whether it is inside; on the side the line is moved to, the
/// curve crosses the moved line twice there, two cuts at one point
/// ordered by their germs. Either way the box settles, with the winding.
#[test]
fn a_trim_tangent_to_a_box_side_settles() {
    let mut cases: Vec<(String, Poly, Option<Side>)> = Vec::new();
    for (name, sp) in [
        (
            "below the low side",
            [[0.375, 0.25, 1.0], [0.625, 0.75, 1.0], [0.875, 0.25, 1.0]],
        ),
        (
            "below the low side, a conic",
            [[0.375, 0.25, 1.0], [0.625, 0.625, 2.0], [0.875, 0.25, 1.0]],
        ),
        (
            "above the low side",
            [[0.375, 0.75, 1.0], [0.625, 0.25, 1.0], [0.875, 0.75, 1.0]],
        ),
        (
            "left of the left side",
            [[0.25, 0.375, 1.0], [0.75, 0.625, 1.0], [0.25, 0.875, 1.0]],
        ),
        (
            "right of the right side",
            [[1.0, 0.375, 1.0], [0.5, 0.625, 1.0], [1.0, 0.875, 1.0]],
        ),
        (
            "right of the right side, a conic",
            [[1.0, 0.375, 1.0], [0.625, 0.625, 2.0], [1.0, 0.875, 1.0]],
        ),
    ] {
        for rev in [false, true] {
            let sp = if rev { [sp[2], sp[1], sp[0]] } else { sp };
            cases.push((format!("{name}, rev {rev}"), Poly::spans(&[sp]), None));
        }
    }
    check_box(&cases, 902);
}

/// Random graphs of rational quadratic spans over the grid, their knots
/// and middle control points on the sixteenth grid, weights `1/64` to
/// `64`: spans through box corners and along box lines at their middle,
/// tangent to box lines there or at knots. The face lies above the
/// graph; certified sums over every partition of the grid are the exact
/// degree, and most settle.
#[test]
fn curved_trims_on_box_lines_are_the_winding() {
    let rows = [ext((1, 2), (1, 1)), ext((1, 1), (3, 2))];
    let us = [0.5, 0.625, 0.75, 0.875, 1.0, 1.25, 1.5];
    let vs: Vec<f64> = (2..=14).map(|k| k as f64 / 16.0).collect();
    let mut rng = Rng::new(950);
    let pick = |rng: &mut Rng, xs: &[f64]| xs[(rng.next_u64() % xs.len() as u64) as usize];
    let g = grid(rows);
    let (vlo, vhi) = (dy(rows[0].lo()), dy(rows[1].hi()));
    let (mut settled, mut all) = (0usize, 0usize);
    for case in varde_testing::seeds(12, 96) {
        let case = case as usize;
        let n = 1 + (rng.next_u64() % 3) as usize;
        let mut mid: Vec<f64> = (0..n).map(|_| pick(&mut rng, &us)).collect();
        mid.sort_by(f64::total_cmp);
        mid.dedup();
        let mut knots = vec![[0.1, pick(&mut rng, &vs)]];
        for u in mid {
            knots.push([u, pick(&mut rng, &vs)]);
        }
        knots.push([1.9, pick(&mut rng, &vs)]);
        let mut spans = Vec::new();
        for leg in knots.windows(2) {
            let (a, b) = (leg[0], leg[1]);
            let w = [1.0, 1.0, 1.0 / 64.0, 64.0, 0.5, 2.0][(rng.next_u64() % 6) as usize];
            let vm = pick(&mut rng, &vs);
            spans.push([
                [a[0], a[1], 1.0],
                [0.5 * (a[0] + b[0]), vm, w],
                [b[0], b[1], 1.0],
            ]);
        }
        let poly = Poly::spans(&spans);
        let mut ring = poly.samples();
        ring.extend([[1.9, 2.0], [0.1, 2.0]]);
        let (reversed, flipped) = (case % 2 == 1, case % 4 >= 2);
        let sign = if flipped { -1 } else { 1 };
        for _ in 0..6 {
            let q = [
                rng.range(2.0, 8.0),
                rng.range(SV * vlo - 1.0, SV * vhi + 1.0),
                rng.range(-1.0, 1.0),
            ];
            let p = uv(q);
            let lines = [
                SU * (p[0] - 0.5),
                SU * (p[0] - 1.5),
                SV * (p[1] - vlo),
                SV * (p[1] - vhi),
            ];
            if poly.near(p, 1e-3) || lines.iter().any(|d| d.abs() < 1e-3) {
                continue;
            }
            let in_run = p[0] > 0.5 && p[0] < 1.5 && p[1] > vlo && p[1] < vhi;
            let inside = in_run && (in_ring(&ring, p) != (reversed != flipped));
            for (k, parts) in grid_parts(&g).iter().enumerate() {
                all += 1;
                let mut sum = Some(0);
                for part in parts {
                    match poly.degree(reversed, flipped, part, q) {
                        Ok(d) => sum = sum.map(|x| x + d.degree),
                        Err(PieceError::Unsettled | PieceError::Through) => sum = None,
                        Err(e) => panic!("{spans:?}: {e:?} partition {k} q {q:?}"),
                    }
                }
                if let Some(sum) = sum {
                    settled += 1;
                    assert_eq!(
                        sum,
                        if inside { sign } else { 0 },
                        "{spans:?}: rev {reversed} flip {flipped} partition {k} q {q:?}"
                    );
                }
            }
        }
    }
    assert!(settled * 3 >= all * 2, "{settled} of {all}");
}
