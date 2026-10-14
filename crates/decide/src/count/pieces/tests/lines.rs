//! Trims lying on box lines, against the exact winding: straight p-curves
//! along box sides, tile lines and the run's own outer sides, crossing a
//! side's line at the end of a side, turning at a box corner (inside the
//! run or on its boundary, on the tile line or off it), with flipped faces
//! and coedges against their edges, rational spans along a line and signed
//! zeros on it. Each edge's curve is its p-curve's image (no ribbon
//! width), so a run's degree is `±1` where the query's shadow lies in the
//! run and the face, else 0, and runs of any partition sum to it.

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
/// direction on an unflipped face), the run's rows.
struct Case {
    name: &'static str,
    pts: Vec<[f64; 2]>,
    face: fn([f64; 2]) -> bool,
    rows: [Extent; 2],
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
        },
        Case {
            name: "along the tile line",
            pts: vec![[1.0, 0.1], [1.0, 0.9]],
            face: |p| p[0] < 1.0,
            rows: mid,
        },
        Case {
            name: "along the box line between the rows",
            pts: vec![[0.1, 0.5], [1.0, 0.5], [1.9, 0.5]],
            face: |p| p[1] > 0.5,
            rows: mid,
        },
        Case {
            name: "along the run's low side",
            pts: vec![[0.1, 0.25], [1.0, 0.25], [1.9, 0.25]],
            face: |p| p[1] > 0.25,
            rows: mid,
        },
        Case {
            name: "left turn at an inner box corner",
            pts: vec![[0.1, 0.5], [0.75, 0.5], [0.75, 0.9]],
            face: |p| p[1] > 0.5 && p[0] < 0.75,
            rows: mid,
        },
        Case {
            name: "right turn at an inner box corner",
            pts: vec![[0.1, 0.5], [0.75, 0.5], [0.75, 0.1]],
            face: |p| p[1] > 0.5 || p[0] > 0.75,
            rows: mid,
        },
        Case {
            name: "left turn at a tile corner",
            pts: vec![[0.1, 0.5], [1.0, 0.5], [1.0, 0.9]],
            face: |p| p[1] > 0.5 && p[0] < 1.0,
            rows: mid,
        },
        Case {
            name: "right turn at a tile corner",
            pts: vec![[0.1, 0.5], [1.0, 0.5], [1.0, 0.1]],
            face: |p| p[1] > 0.5 || p[0] > 1.0,
            rows: mid,
        },
        Case {
            name: "turn at a corner of the run's low side",
            pts: vec![[0.1, 0.25], [0.75, 0.25], [0.75, 0.9]],
            face: |p| p[1] > 0.25 && p[0] < 0.75,
            rows: mid,
        },
        Case {
            name: "turn at the run's low left corner",
            pts: vec![[0.1, 0.25], [0.5, 0.25], [0.5, 0.9]],
            face: |p| p[1] > 0.25 && p[0] < 0.5,
            rows: mid,
        },
        Case {
            name: "along the tile's low side, signed zeros",
            pts: vec![[0.1, 0.0], [1.0, -0.0], [1.9, 0.0]],
            face: |p| p[1] > 0.0,
            rows: low,
        },
        Case {
            name: "along the tile's low side, negative zeros",
            pts: vec![[0.1, -0.0], [1.0, -0.0], [1.9, -0.0]],
            face: |p| p[1] > 0.0,
            rows: low,
        },
    ]
}

/// Checks every case with every weight and orientation (reversed,
/// flipped) over every partition at `n` queries: certified sums are the
/// exact degree, and the whole run settles at most of them (parts clear
/// of the trim, and a turn on a part's boundary, don't).
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
            let legs = case.pts.windows(2).map(|leg| {
                if leg[0][0] == leg[1][0] {
                    SU * (p[0] - leg[0][0])
                } else {
                    SV * (p[1] - leg[0][1])
                }
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
                let (mut whole, mut all) = (0, 0);
                for (k, parts) in grid_parts(&g).iter().enumerate() {
                    for &q in &qs {
                        let p = uv(q);
                        if near(p) {
                            continue;
                        }
                        let inside = in_run(p) && ((case.face)(p) != (reversed != flipped));
                        all += usize::from(k == 0);
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
                        if let Some(sum) = sum {
                            whole += usize::from(k == 0);
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
                assert!(
                    whole * 10 >= all * 9,
                    "{}: wt {wt} rev {reversed} flip {flipped}: {whole} of {all}",
                    case.name
                );
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
