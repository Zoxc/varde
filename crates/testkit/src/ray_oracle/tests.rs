//! The oracle on closed solids: winding numbers against analytic inside
//! tests, curve crossings against winding differences, thin ribbons,
//! and ties against explicitly moved configurations.

use super::*;
use crate::rng::Rng;

const UP: [f64; 3] = [2.0, 3.0, 32.0];
const DOWN: [f64; 3] = [-2.0, -3.0, -32.0];
const S: f64 = std::f64::consts::FRAC_1_SQRT_2;

fn p4(p: [f64; 3], w: f64) -> [f64; 4] {
    [p[0], p[1], p[2], w]
}

fn mid3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|k| (a[k] + b[k]) / 2.0)
}

fn line3(a: [f64; 3], b: [f64; 3], knots: [f64; 2]) -> Chain {
    Chain {
        spans: vec![[p4(a, 1.0), p4(mid3(a, b), 1.0), p4(b, 1.0)]],
        knots: knots.to_vec(),
    }
}

fn line_uv(a: [f64; 2], b: [f64; 2], knots: [f64; 2]) -> UvChain {
    let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    UvChain {
        spans: vec![[[a[0], a[1], 1.0], [m[0], m[1], 1.0], [b[0], b[1], 1.0]]],
        knots: knots.to_vec(),
    }
}

/// The biquadratic net of the bilinear patch with corners `c[uv]` at
/// UV `(0, 0), (1, 0), (1, 1), (0, 1)`.
fn bilinear(c: [[f64; 3]; 4]) -> [[f64; 4]; 9] {
    std::array::from_fn(|k| {
        let (s, t) = ((k / 3) as f64 / 2.0, (k % 3) as f64 / 2.0);
        let p = [0, 1, 2].map(|a| {
            (1.0 - s) * (1.0 - t) * c[0][a]
                + s * (1.0 - t) * c[1][a]
                + s * t * c[2][a]
                + (1.0 - s) * t * c[3][a]
        });
        p4(p, 1.0)
    })
}

/// The faces of the box `[lo, hi]`, outward. Face `f` is parametrised
/// with `x_u × x_v` inward and its loop clockwise in UV when `flip[f]`.
/// Edge `e` runs over the knots `knots(e)`.
fn box_faces(
    lo: [f64; 3],
    hi: [f64; 3],
    flip: [bool; 6],
    knots: impl Fn(usize) -> [f64; 2],
) -> Vec<Face> {
    let v = |i: usize| [0, 1, 2].map(|a| if i >> a & 1 == 1 { hi[a] } else { lo[a] });
    // Corners counter-clockwise seen from outside.
    let quads = [
        [0, 4, 6, 2],
        [1, 3, 7, 5],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 2, 3, 1],
        [4, 5, 7, 6],
    ];
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for q in &quads {
        for k in 0..4 {
            let (a, b) = (q[k], q[(k + 1) % 4]);
            let e = (a.min(b), a.max(b));
            if !edges.contains(&e) {
                edges.push(e);
            }
        }
    }
    quads
        .iter()
        .enumerate()
        .map(|(f, q)| {
            let uv: [[f64; 2]; 4] = if flip[f] {
                [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]
            } else {
                [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
            };
            // Corner k sits at uv[k]; the net wants the corners by UV.
            let at = |t: [f64; 2]| q[uv.iter().position(|x| *x == t).expect("corner")];
            let net = bilinear([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]].map(|t| v(at(t))));
            let lp = (0..4)
                .map(|k| {
                    let (a, b) = (q[k], q[(k + 1) % 4]);
                    let (s, e) = (a.min(b), a.max(b));
                    let id = edges.iter().position(|x| *x == (s, e)).expect("edge");
                    let kn = knots(id);
                    let ua = uv[q.iter().position(|&x| x == s).expect("corner")];
                    let ub = uv[q.iter().position(|&x| x == e).expect("corner")];
                    Coedge {
                        pcurve: line_uv(ua, ub, kn),
                        curve: line3(v(s), v(e), kn),
                        t_range: kn,
                        reversed: a > b,
                    }
                })
                .collect();
            Face {
                tiles: vec![([0, 0], net)],
                loops: vec![lp],
            }
        })
        .collect()
}

const LO: [f64; 3] = [-1.0, -0.75, -0.5];
const HI: [f64; 3] = [1.25, 1.5, 0.875];

fn plain_box(flip: [bool; 6]) -> Vec<Face> {
    box_faces(LO, HI, flip, |_| [0.0, 1.0])
}

fn in_box(p: [f64; 3]) -> Option<i64> {
    let d = (0..3).fold(f64::INFINITY, |m, a| {
        let (x, y) = ((p[a] - LO[a]).abs(), (p[a] - HI[a]).abs());
        if x < m && x < y {
            x
        } else if y < m {
            y
        } else {
            m
        }
    });
    (d > 1e-6).then(|| (0..3).all(|a| LO[a] < p[a] && p[a] < HI[a]) as i64)
}

fn arc(z: f64, r: f64) -> Chain {
    Chain {
        spans: vec![
            [p4([r, 0.0, z], 1.0), p4([r, r, z], S), p4([0.0, r, z], 1.0)],
            [
                p4([0.0, r, z], 1.0),
                p4([-r, r, z], S),
                p4([-r, 0.0, z], 1.0),
            ],
        ],
        knots: vec![0.0, 1.0, 2.0],
    }
}

const R: f64 = 1.25;
const H: f64 = 0.875;

/// The half cylinder `x² + y² < R²`, `y > 0`, `0 < z < H`: the curved
/// side as two quarter-circle tiles, the plane `y = 0`, and two
/// half-disc caps trimmed by the arcs (the bottom one parametrised
/// inward with a clockwise loop).
fn half_cylinder() -> Vec<Face> {
    let (r, h) = (R, H);
    let (bot_arc, top_arc) = (arc(0.0, r), arc(h, r));
    let right = line3([r, 0.0, 0.0], [r, 0.0, h], [0.0, 1.0]);
    let left = line3([-r, 0.0, 0.0], [-r, 0.0, h], [0.0, 1.0]);
    let bot_d = line3([-r, 0.0, 0.0], [r, 0.0, 0.0], [0.0, 1.0]);
    let top_d = line3([-r, 0.0, h], [r, 0.0, h], [0.0, 1.0]);
    let ce = |pcurve: UvChain, curve: &Chain, reversed: bool| Coedge {
        t_range: [curve.knots[0], *curve.knots.last().expect("knots")],
        pcurve,
        curve: curve.clone(),
        reversed,
    };
    let quarter = |q: [[f64; 3]; 3]| -> [[f64; 4]; 9] {
        std::array::from_fn(|k| {
            let (i, j) = (k / 3, k % 3);
            [q[i][0], q[i][1], h * j as f64 / 2.0, q[i][2]]
        })
    };
    let uv_row = |v: f64| UvChain {
        spans: vec![
            [[0.0, v, 1.0], [0.5, v, 1.0], [1.0, v, 1.0]],
            [[1.0, v, 1.0], [1.5, v, 1.0], [2.0, v, 1.0]],
        ],
        knots: vec![0.0, 1.0, 2.0],
    };
    let side = Face {
        tiles: vec![
            ([0, 0], quarter([[r, 0.0, 1.0], [r, r, S], [0.0, r, 1.0]])),
            ([1, 0], quarter([[0.0, r, 1.0], [-r, r, S], [-r, 0.0, 1.0]])),
        ],
        loops: vec![vec![
            ce(uv_row(0.0), &bot_arc, false),
            ce(line_uv([2.0, 0.0], [2.0, 1.0], [0.0, 1.0]), &left, false),
            ce(uv_row(1.0), &top_arc, true),
            ce(line_uv([0.0, 0.0], [0.0, 1.0], [0.0, 1.0]), &right, true),
        ]],
    };
    let plane = Face {
        tiles: vec![(
            [0, 0],
            bilinear([[-r, 0.0, 0.0], [r, 0.0, 0.0], [r, 0.0, h], [-r, 0.0, h]]),
        )],
        loops: vec![vec![
            ce(line_uv([0.0, 0.0], [1.0, 0.0], [0.0, 1.0]), &bot_d, false),
            ce(line_uv([1.0, 0.0], [1.0, 1.0], [0.0, 1.0]), &right, false),
            ce(line_uv([0.0, 1.0], [1.0, 1.0], [0.0, 1.0]), &top_d, true),
            ce(line_uv([0.0, 0.0], [0.0, 1.0], [0.0, 1.0]), &left, true),
        ]],
    };
    let cap_arc = UvChain {
        spans: vec![
            [[1.0, 0.0, 1.0], [1.0, 1.0, S], [0.5, 1.0, 1.0]],
            [[0.5, 1.0, 1.0], [0.0, 1.0, S], [0.0, 0.0, 1.0]],
        ],
        knots: vec![0.0, 1.0, 2.0],
    };
    let cap = |z: f64, d: &Chain, a: &Chain, reversed: bool| Face {
        tiles: vec![(
            [0, 0],
            bilinear([[-r, 0.0, z], [r, 0.0, z], [r, r, z], [-r, r, z]]),
        )],
        loops: vec![vec![
            ce(line_uv([0.0, 0.0], [1.0, 0.0], [0.0, 1.0]), d, reversed),
            ce(cap_arc.clone(), a, reversed),
        ]],
    };
    vec![
        side,
        plane,
        cap(0.0, &bot_d, &bot_arc, true),
        cap(h, &top_d, &top_arc, false),
    ]
}

fn in_half_cylinder(p: [f64; 3]) -> Option<i64> {
    let rho = (p[0] * p[0] + p[1] * p[1]).sqrt();
    let near =
        (rho - R).abs() < 1e-6 || p[1].abs() < 1e-6 || p[2].abs() < 1e-6 || (p[2] - H).abs() < 1e-6;
    (!near).then(|| (rho < R && p[1] > 0.0 && 0.0 < p[2] && p[2] < H) as i64)
}

fn exact_faces(faces: &[Face]) -> Vec<ExactFace> {
    faces.iter().map(ExactFace::new).collect()
}

/// The winding number of `q` (shifted by `qs`) in the faces (shifted by
/// `fs`), and whether a tie was resolved.
fn wind(faces: &[ExactFace], fs: &Shift, q: [f64; 3], qs: &Shift, dir: [f64; 3]) -> (i64, bool) {
    faces.iter().fold((0, false), |(n, t), f| {
        let c = f.ray(fs, q, qs, dir).expect("ray count");
        (n + c.count, t || c.tie)
    })
}

fn crossings(faces: &[ExactFace], fs: &Shift, c: &Chain, cs: &Shift) -> (i64, bool) {
    faces.iter().fold((0, false), |(n, t), f| {
        let x = f.curve(fs, c, cs).expect("curve count");
        (n + x.count, t || x.tie)
    })
}

fn random_point(rng: &mut Rng, lo: [f64; 3], hi: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|a| rng.range(lo[a], hi[a]))
}

#[test]
fn box_windings_match_inside_test_in_both_directions_and_parametrisations() {
    let mut rng = Rng::new(1);
    let variants = [
        exact_faces(&plain_box([false; 6])),
        exact_faces(&plain_box([true; 6])),
        exact_faces(&box_faces(
            LO,
            HI,
            [true, false, false, true, false, true],
            |e| [-0.5 * e as f64, 0.75 + e as f64],
        )),
    ];
    let mut inside = 0;
    for _ in 0..150 {
        let q = random_point(&mut rng, [-1.5, -1.25, -1.0], [1.75, 2.0, 1.375]);
        let Some(expect) = in_box(q) else { continue };
        inside += expect;
        for faces in &variants {
            for dir in [UP, DOWN] {
                assert_eq!(
                    wind(faces, &Shift::NONE, q, &Shift::NONE, dir),
                    (expect, false),
                    "{q:?} {dir:?}"
                );
            }
        }
    }
    assert!(inside > 20, "too few inside points: {inside}");
}

#[test]
fn half_cylinder_windings_match_inside_test() {
    let faces = exact_faces(&half_cylinder());
    let mut rng = Rng::new(2);
    let mut inside = 0;
    for _ in 0..150 {
        let q = random_point(&mut rng, [-1.5, -0.5, -0.25], [1.5, 1.5, 1.125]);
        let Some(expect) = in_half_cylinder(q) else {
            continue;
        };
        inside += expect;
        for dir in [UP, DOWN] {
            assert_eq!(
                wind(&faces, &Shift::NONE, q, &Shift::NONE, dir),
                (expect, false),
                "{q:?} {dir:?}"
            );
        }
    }
    assert!(inside > 20, "too few inside points: {inside}");
}

fn random_chain(rng: &mut Rng, lo: [f64; 3], hi: [f64; 3]) -> Chain {
    let (a, b) = (random_point(rng, lo, hi), random_point(rng, lo, hi));
    if rng.unit() < 0.5 {
        line3(a, b, [0.0, 1.0])
    } else {
        // An arc through a random middle control point, two spans.
        let m = random_point(rng, lo, hi);
        let w = rng.range(0.3, 2.0);
        let (ma, mb) = (mid3(a, m), mid3(m, b));
        let j = mid3(ma, mb);
        Chain {
            spans: vec![
                [p4(a, 1.0), p4(ma, w), p4(j, 1.0)],
                [p4(j, 1.0), p4(mb, w), p4(b, 1.0)],
            ],
            knots: vec![-1.0, 0.5, 3.0],
        }
    }
}

fn chain_ends(c: &Chain) -> ([f64; 3], [f64; 3]) {
    let (a, b) = (c.spans[0][0], c.spans.last().expect("spans")[2]);
    ([a[0], a[1], a[2]], [b[0], b[1], b[2]])
}

/// A solid's faces, its inside test and a box around it.
type Solid = (
    Vec<ExactFace>,
    fn([f64; 3]) -> Option<i64>,
    [f64; 3],
    [f64; 3],
);

#[test]
fn curve_crossings_sum_to_winding_differences() {
    let solids: [Solid; 2] = [
        (
            exact_faces(&plain_box([false, true, false, true, false, false])),
            in_box,
            [-1.5, -1.25, -1.0],
            [1.75, 2.0, 1.375],
        ),
        (
            exact_faces(&half_cylinder()),
            in_half_cylinder,
            [-1.5, -0.5, -0.25],
            [1.5, 1.5, 1.125],
        ),
    ];
    let mut rng = Rng::new(3);
    for (faces, inside, lo, hi) in &solids {
        let mut nonzero = 0;
        for _ in 0..40 {
            let c = random_chain(&mut rng, *lo, *hi);
            let (a, b) = chain_ends(&c);
            let (Some(wa), Some(wb)) = (inside(a), inside(b)) else {
                continue;
            };
            let (n, tie) = crossings(faces, &Shift::NONE, &c, &Shift::NONE);
            assert_eq!((n, tie), (wb - wa, false), "{c:?}");
            nonzero += (wb != wa) as i32;
        }
        assert!(nonzero > 3, "too few crossing chains: {nonzero}");
    }
}

/// The box with every p-curve's middle control point moved off its
/// chord by up to `1e-7` (ends kept), so every ribbon has area.
fn thin_box(rng: &mut Rng) -> Vec<Face> {
    let mut faces = plain_box([false, true, false, false, true, false]);
    for f in &mut faces {
        for ce in &mut f.loops[0] {
            let m = &mut ce.pcurve.spans[0][1];
            m[0] += rng.range(-1e-7, 1e-7);
            m[1] += rng.range(-1e-7, 1e-7);
        }
    }
    faces
}

#[test]
fn thin_ribbons_close_the_surface() {
    let mut rng = Rng::new(4);
    let faces = exact_faces(&thin_box(&mut rng));
    for _ in 0..60 {
        let q = random_point(&mut rng, [-1.5, -1.25, -1.0], [1.75, 2.0, 1.375]);
        let Some(expect) = in_box(q) else { continue };
        for dir in [UP, DOWN] {
            assert_eq!(
                wind(&faces, &Shift::NONE, q, &Shift::NONE, dir).0,
                expect,
                "{q:?}"
            );
        }
    }
    for _ in 0..20 {
        let c = random_chain(&mut rng, [-1.5, -1.25, -1.0], [1.75, 2.0, 1.375]);
        let (a, b) = chain_ends(&c);
        let (Some(wa), Some(wb)) = (in_box(a), in_box(b)) else {
            continue;
        };
        assert_eq!(
            crossings(&faces, &Shift::NONE, &c, &Shift::NONE).0,
            wb - wa,
            "{c:?}"
        );
    }
}

#[test]
fn a_ray_between_trimmed_patch_and_edge_counts_once_by_the_ribbon() {
    // The bottom face (z = LO.z), corners 0, 2, 3, 1: its edge from
    // vertex 0 to 2 runs along y at x = LO.x, UV (0, 0) to (1, 0), and
    // `x_v` is along x. Bowing that p-curve inward (v > 0, by 1e-7 at
    // its middle) leaves a sliver 2.25e-7 wide between the trimmed
    // patch and the edge, covered by the ribbon.
    let mut faces = plain_box([false; 6]);
    let bottom = &mut faces[4];
    let ce = bottom.loops[0]
        .iter_mut()
        .find(|c| c.pcurve.spans[0][2][..2] == [1.0, 0.0])
        .expect("coedge");
    ce.pcurve.spans[0][1][1] = 2e-7;
    let exact = exact_faces(&faces);
    let hit = [LO[0] + 1e-7, (LO[1] + HI[1]) / 2.0, LO[2]];
    let q = [
        hit[0] - UP[0] / 32.0,
        hit[1] - UP[1] / 32.0,
        hit[2] - UP[2] / 32.0,
    ];
    let face = &exact[4];
    let parts = |ribbon: bool| {
        face.ray_parts(&Shift::NONE, q, &Shift::NONE, UP, |p| {
            p.is_ribbon() == ribbon
        })
        .expect("count")
        .count
    };
    assert_eq!((parts(false), parts(true)), (0, -1));
    assert_eq!(wind(&exact, &Shift::NONE, q, &Shift::NONE, UP), (0, false));
    // Inside, just above the sliver.
    let inner = [
        hit[0] + UP[0] / 1024.0,
        hit[1] + UP[1] / 1024.0,
        hit[2] + UP[2] / 1024.0,
    ];
    assert_eq!(
        wind(&exact, &Shift::NONE, inner, &Shift::NONE, DOWN),
        (1, false)
    );
    // Well inside the face, the patch part counts.
    let q2 = [0.0 - UP[0] / 32.0, 0.25 - UP[1] / 32.0, LO[2] - 1.0];
    let face_parts = |ribbon: bool| {
        face.ray_parts(&Shift::NONE, q2, &Shift::NONE, UP, |p| {
            p.is_ribbon() == ribbon
        })
        .expect("count")
        .count
    };
    assert_eq!((face_parts(false), face_parts(true)), (-1, 0));
}

/// `q + 2^-20·σ·d (+ 2^-40·T₂)`, exactly representable for the inputs
/// used.
fn moved(q: [f64; 3], d: [f64; 3], sigma: f64, second: bool) -> [f64; 3] {
    let t2 = [3.0 / 16.0, 7.0 / 16.0, 11.0 / 16.0];
    [0, 1, 2].map(|a| {
        let x = q[a] + sigma * d[a] * libm::scalbn(1.0, -20);
        if second {
            x + t2[a] * libm::scalbn(1.0, -40)
        } else {
            x
        }
    })
}

fn check_point_tie(
    faces: &[ExactFace],
    q: [f64; 3],
    d: [f64; 3],
    parallel: bool,
    expect: [i64; 2],
) {
    for (k, sigma) in [1i8, -1].into_iter().enumerate() {
        let s = f64::from(sigma);
        let there = moved(q, d, s, parallel);
        let back = moved(q, d, -s, parallel);
        let back = if parallel {
            // The face moving by +T₂ is the point moving by −T₂.
            let t2 = [3.0 / 16.0, 7.0 / 16.0, 11.0 / 16.0];
            [0, 1, 2].map(|a| back[a] - 2.0 * t2[a] * libm::scalbn(1.0, -40))
        } else {
            back
        };
        for dir in [UP, DOWN] {
            let (n, tie) = wind(faces, &Shift::NONE, q, &Shift::tie(d, sigma), dir);
            assert!(tie, "{q:?} σ {sigma}: no tie seen");
            assert_eq!(n, expect[k], "{q:?} σ {sigma} {dir:?}");
            assert_eq!(
                wind(faces, &Shift::NONE, there, &Shift::NONE, dir),
                (n, false),
                "{there:?}"
            );
            // The face moved instead: the point moves the other way.
            let (m, _) = wind(faces, &Shift::tie(d, sigma), q, &Shift::NONE, dir);
            assert_eq!(
                wind(faces, &Shift::NONE, back, &Shift::NONE, dir),
                (m, false),
                "{back:?}"
            );
            assert!((0..=1).contains(&m), "{m}");
        }
    }
}

#[test]
fn box_point_ties_resolve_as_the_moved_point() {
    let faces = exact_faces(&plain_box([false, true, false, true, false, false]));
    // On the bottom face, moved up (inside) for σ = +1.
    check_point_tie(&faces, [0.25, 0.5, LO[2]], [0.25, 0.5, 1.0], false, [1, 0]);
    // On the edge x = LO.x, z = LO.z.
    check_point_tie(&faces, [LO[0], 0.5, LO[2]], [0.5, 0.25, 1.0], false, [1, 0]);
    check_point_tie(
        &faces,
        [LO[0], 0.5, LO[2]],
        [0.5, 0.25, -1.0],
        false,
        [0, 0],
    );
    // At the vertex LO.
    check_point_tie(&faces, LO, [1.0, 0.75, 0.5], false, [1, 0]);
    check_point_tie(&faces, LO, [1.0, -0.75, 0.5], false, [0, 0]);
    // Moving along the face: the second term decides (T₂ points up).
    check_point_tie(&faces, [0.25, 0.5, LO[2]], [1.0, 0.0, 0.0], true, [1, 1]);
}

#[test]
fn half_cylinder_point_ties_resolve_as_the_moved_point() {
    let faces = exact_faces(&half_cylinder());
    // On the plane y = 0, on the bottom cap, on the side's tile line
    // (x = 0, y = R), on the arc edge at its knot.
    check_point_tie(&faces, [0.25, 0.0, 0.5], [0.25, 1.0, 0.5], false, [1, 0]);
    check_point_tie(&faces, [0.25, 0.5, 0.0], [0.25, 0.5, 1.0], false, [1, 0]);
    check_point_tie(&faces, [0.0, R, 0.5], [0.25, -1.0, 0.5], false, [1, 0]);
    check_point_tie(&faces, [0.0, R, 0.0], [0.25, -1.0, 0.5], false, [1, 0]);
}

#[test]
fn rays_exactly_through_edges_and_tile_lines() {
    let boxed = exact_faces(&plain_box([false; 6]));
    let cyl = exact_faces(&half_cylinder());
    let back = |p: [f64; 3], s: f64| [0, 1, 2].map(|a| p[a] - s * UP[a]);
    // Through the box's edge y = LO.y, z = LO.z, from below and from
    // inside (looking down).
    let e = [0.5, LO[1], LO[2]];
    let d = [0.25, 0.5, 1.0];
    for sigma in [1, -1] {
        assert_eq!(
            wind(
                &boxed,
                &Shift::NONE,
                back(e, 1.0 / 32.0),
                &Shift::tie(d, sigma),
                UP
            ),
            (0, true)
        );
        assert_eq!(
            wind(
                &boxed,
                &Shift::NONE,
                back(e, -1.0 / 32.0),
                &Shift::tie(d, sigma),
                DOWN
            ),
            (1, true)
        );
        assert_eq!(
            wind(
                &boxed,
                &Shift::tie(d, sigma),
                back(e, -1.0 / 32.0),
                &Shift::NONE,
                DOWN
            ),
            (1, true)
        );
    }
    // Through the side's tile line at (0, R, 0.5), from below and from
    // inside.
    let p = [0.0, R, 0.5];
    for sigma in [1, -1] {
        assert_eq!(
            wind(
                &cyl,
                &Shift::NONE,
                back(p, 1.0 / 16.0),
                &Shift::tie(d, sigma),
                UP
            ),
            (0, true)
        );
        assert_eq!(
            wind(
                &cyl,
                &Shift::NONE,
                back(p, 1.0 / 256.0),
                &Shift::tie(d, sigma),
                UP
            ),
            (1, true)
        );
    }
}

#[test]
fn curve_ties_resolve_as_the_moved_curve() {
    let boxed = exact_faces(&plain_box([false; 6]));
    let cyl = exact_faces(&half_cylinder());
    let d = [0.25, 0.5, 1.0];
    let shift_chain = |c: &Chain, sigma: f64| Chain {
        spans: c
            .spans
            .iter()
            .map(|s| {
                s.map(|p| {
                    let m = moved([p[0], p[1], p[2]], d, sigma, false);
                    [m[0], m[1], m[2], p[3]]
                })
            })
            .collect(),
        knots: c.knots.clone(),
    };
    let cases: [(&[ExactFace], Chain); 5] = [
        // Ends on the bottom face.
        (
            &boxed,
            line3([0.25, 0.5, -1.5], [0.25, 0.5, LO[2]], [0.0, 1.0]),
        ),
        // Through the edge y = LO.y, z = LO.z.
        (
            &boxed,
            line3([0.5, -1.0, -0.75], [0.5, -0.5, -0.25], [0.0, 1.0]),
        ),
        // Starts at a vertex.
        (&boxed, line3(LO, [0.0, 0.0, 0.0], [0.0, 1.0])),
        // Through the side's tile line, and through the arc's knot.
        (&cyl, line3([0.0, 1.0, 0.5], [0.0, 1.5, 0.5], [0.0, 1.0])),
        (&cyl, line3([0.0, 1.5, 0.25], [0.0, 1.0, -0.25], [0.0, 1.0])),
    ];
    for (faces, c) in &cases {
        for sigma in [1i8, -1] {
            let (n, tie) = crossings(faces, &Shift::NONE, c, &Shift::tie(d, sigma));
            assert!(tie, "{c:?}: no tie seen");
            let expect = crossings(
                faces,
                &Shift::NONE,
                &shift_chain(c, f64::from(sigma)),
                &Shift::NONE,
            );
            assert_eq!((n, false), expect, "{c:?} σ {sigma}");
            let (m, _) = crossings(faces, &Shift::tie(d, sigma), c, &Shift::NONE);
            let back = crossings(
                faces,
                &Shift::NONE,
                &shift_chain(c, -f64::from(sigma)),
                &Shift::NONE,
            );
            assert_eq!((m, false), back, "{c:?} face σ {sigma}");
        }
    }
}

#[test]
fn split_pcurve_knots_and_a_shared_tile_line() {
    // The box's −x face with its p-curves split at t = 0.5 (the edge
    // curves keep one span), and its patch as two tiles meeting at
    // u = 1 (UV [0, 2] × [0, 1]).
    let mut faces = plain_box([false; 6]);
    let f = &mut faces[0];
    let net = f.tiles[0].1;
    let half = |net: [[f64; 4]; 9], lo: bool| -> [[f64; 4]; 9] {
        // The bilinear net restricted to u ∈ [0, ½] or [½, 1].
        std::array::from_fn(|k| {
            let (i, j) = (k / 3, k % 3);
            let s = if lo {
                i as f64 / 4.0
            } else {
                0.5 + i as f64 / 4.0
            };
            let a = net[j];
            let b = net[6 + j];
            [0, 1, 2, 3].map(|c| {
                if c == 3 {
                    1.0
                } else {
                    a[c] + (b[c] - a[c]) * s
                }
            })
        })
    };
    f.tiles = vec![([0, 0], half(net, true)), ([1, 0], half(net, false))];
    for ce in &mut f.loops[0] {
        let sp = ce.pcurve.spans[0];
        let (a, b) = ([sp[0][0] * 2.0, sp[0][1]], [sp[2][0] * 2.0, sp[2][1]]);
        let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        let l1 = line_uv(a, m, [0.0, 0.5]);
        let l2 = line_uv(m, b, [0.5, 1.0]);
        ce.pcurve = UvChain {
            spans: vec![l1.spans[0], l2.spans[0]],
            knots: vec![0.0, 0.5, 1.0],
        };
    }
    let faces = exact_faces(&faces);
    let mut rng = Rng::new(5);
    for _ in 0..60 {
        let q = random_point(&mut rng, [-1.5, -1.25, -1.0], [1.75, 2.0, 1.375]);
        let Some(expect) = in_box(q) else { continue };
        for dir in [UP, DOWN] {
            assert_eq!(
                wind(&faces, &Shift::NONE, q, &Shift::NONE, dir),
                (expect, false),
                "{q:?}"
            );
        }
    }
    // A ray through the tile line u = 1: x = LO.x, z = (LO.z + HI.z)/2
    // (the face's u runs along z).
    let p = [LO[0], 0.25, (LO[2] + HI[2]) / 2.0];
    let q = [p[0] - 1.0 / 16.0, p[1] - 3.0 / 32.0, p[2] - 1.0];
    let (n, tie) = wind(
        &faces,
        &Shift::NONE,
        q,
        &Shift::tie([1.0, 0.5, 0.25], 1),
        UP,
    );
    assert_eq!((n, tie), (0, true));
}

#[test]
fn shifts_are_exact_and_relative() {
    let t = Shift::tie([2.0, 3.0, 32.0], -1);
    let v = t.0.as_ref().expect("a shift");
    // σ·d·2^-512 dominates: the signs are those of −d.
    assert!(v.iter().all(|x| x.m < 0));
    assert_eq!(t.minus(&t), None);
    assert_eq!(Shift::NONE.minus(&Shift::NONE), None);
    let e = Shift::exact([1.0, 0.0, -0.5], -3);
    let d = e.minus(&Shift::NONE).expect("non-zero");
    assert_eq!(d.map(|x| x.approx()), [0.125, 0.0, -0.0625]);
}

#[test]
fn queries_without_a_shift_report_degeneracy() {
    let faces = exact_faces(&plain_box([false; 6]));
    let on_face = [0.25, 0.5, LO[2]];
    let r = faces[4].ray(&Shift::NONE, on_face, &Shift::NONE, UP);
    assert!(r.is_err(), "{r:?}");
}

/// A random point of the grid of eighths of `[lo, hi]`.
fn grid(rng: &mut Rng, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * (rng.next_u64() % 9) as f64 / 8.0
}

#[test]
fn random_ties_resolve_as_the_moved_configuration() {
    // Points on the solids' faces, edges and vertices, random tie
    // directions: the winding with the tie shift on the point, and the
    // crossings of a segment from the point, equal those of the point
    // (segment) moved by 2^-20·σ·d. The moved configuration is skipped
    // when it is degenerate itself (a ray through an edge).
    let mut rng = Rng::new(6);
    let boxed = exact_faces(&plain_box([false, true, false, true, false, false]));
    let cyl = exact_faces(&half_cylinder());
    let sum = |faces: &[ExactFace], f: &dyn Fn(&ExactFace) -> Result<Crossings, Unresolved>| {
        faces
            .iter()
            .map(|x| f(x).map(|c| (c.count, c.tie)))
            .try_fold((0, false), |(n, t), c| c.map(|(m, u)| (n + m, t || u)))
    };
    let mut ties = 0;
    for it in 0..24 {
        let (faces, q): (&[ExactFace], [f64; 3]) = if it % 2 == 0 {
            let mut q = [0, 1, 2].map(|a| grid(&mut rng, LO[a], HI[a]));
            for (a, x) in q.iter_mut().enumerate() {
                if rng.unit() < 0.5 {
                    *x = if rng.unit() < 0.5 { LO[a] } else { HI[a] };
                }
            }
            (&boxed, q)
        } else {
            let q = match rng.next_u64() % 4 {
                0 => [grid(&mut rng, -R, R), 0.0, grid(&mut rng, 0.0, H)],
                1 => [
                    grid(&mut rng, -0.875, 0.875),
                    grid(&mut rng, 0.0, 0.875),
                    if rng.unit() < 0.5 { 0.0 } else { H },
                ],
                2 => [0.0, R, grid(&mut rng, 0.0, H)],
                _ => [
                    if rng.unit() < 0.5 { R } else { -R },
                    0.0,
                    grid(&mut rng, 0.0, H),
                ],
            };
            (&cyl, q)
        };
        let d = [0, 1, 2].map(|_| (rng.next_u64() % 16) as f64 / 8.0 - 0.9375);
        let other = random_point(&mut rng, [-1.5, -1.25, -1.0], [1.75, 2.0, 1.375]);
        let seg = line3(q, other, [0.0, 1.0]);
        for sigma in [1i8, -1] {
            let s = f64::from(sigma);
            let tie = Shift::tie(d, sigma);
            for dir in [UP, DOWN] {
                let (n, t) =
                    sum(faces, &|f| f.ray(&Shift::NONE, q, &tie, dir)).expect("tie resolved");
                ties += t as i32;
                assert!((0..=1).contains(&n), "{q:?} {d:?} σ {sigma}: winding {n}");
                let m = moved(q, d, s, false);
                if let Ok((e, _)) = sum(faces, &|f| f.ray(&Shift::NONE, m, &Shift::NONE, dir)) {
                    assert_eq!(n, e, "{q:?} {d:?} σ {sigma} {dir:?}");
                }
            }
            let (n, _) = sum(faces, &|f| f.curve(&Shift::NONE, &seg, &tie)).expect("tie resolved");
            let mseg = line3(moved(q, d, s, false), moved(other, d, s, false), [0.0, 1.0]);
            if let Ok((e, _)) = sum(faces, &|f| f.curve(&Shift::NONE, &mseg, &Shift::NONE)) {
                assert_eq!(n, e, "{seg:?} {d:?} σ {sigma}");
            }
        }
    }
    assert!(ties > 20, "too few ties: {ties}");
}

/// A rational quadratic span's point at `s`.
fn span_at(sp: &[[f64; 4]; 3], s: f64) -> [f64; 3] {
    let b = [(1.0 - s) * (1.0 - s), 2.0 * s * (1.0 - s), s * s];
    let w: f64 = (0..3).map(|i| b[i] * sp[i][3]).sum();
    [0, 1, 2].map(|a| (0..3).map(|i| b[i] * sp[i][3] * sp[i][a]).sum::<f64>() / w)
}

#[test]
fn a_ribbon_crossing_a_tile_line() {
    // The side's bottom p-curve as one span (0, 0), (0.9, 0), (2, 0)
    // over t ∈ [0, 2]: the same half circle, run at another speed than
    // the arc edge, so the ribbon spans chords inside the bottom cap's
    // plane, and its piece over t ∈ [1, 2] crosses the tile line u = 1.
    let mut faces = half_cylinder();
    let ce = &mut faces[0].loops[0][0];
    ce.pcurve = UvChain {
        spans: vec![[[0.0, 0.0, 1.0], [0.9, 0.0, 1.0], [2.0, 0.0, 1.0]]],
        knots: vec![0.0, 2.0],
    };
    let arc = ce.curve.clone();
    let exact = exact_faces(&faces);
    assert!(
        exact[0]
            .pieces
            .iter()
            .any(|p| p.is_ribbon() && !p.preds.is_empty()),
        "no ribbon piece with tile conditions"
    );
    let mut rng = Rng::new(7);
    for _ in 0..40 {
        let q = random_point(&mut rng, [-1.5, -0.5, -0.25], [1.5, 1.5, 1.125]);
        let Some(expect) = in_half_cylinder(q) else {
            continue;
        };
        for dir in [UP, DOWN] {
            assert_eq!(
                wind(&exact, &Shift::NONE, q, &Shift::NONE, dir).0,
                expect,
                "{q:?}"
            );
        }
    }
    // Rays through the ribbon's chords, a quarter along, at t = 1.5 and
    // where its p-curve crosses the tile line (s solving
    // 0.2·s² + 1.8·s − 1 = 0 on the p-curve span). Both sides of the
    // ribbon lie on the arc, so in its plane it folds over itself: every
    // ray crosses it an even number of times with opposite signs, and a
    // crossing lost at a tile condition would show as ±1.
    let s_line = (-1.8 + (1.8f64 * 1.8 + 0.8).sqrt()) / 0.4;
    for t in [1.5, 2.0 * s_line] {
        let s = t / 2.0;
        let u = 2.0 * s * (1.0 - s) * 0.9 + s * s * 2.0;
        let quarter = &faces[0].tiles[usize::from(u >= 1.0)].1;
        let row = [quarter[0], quarter[3], quarter[6]];
        let on_patch = span_at(&row, if u >= 1.0 { u - 1.0 } else { u });
        let on_edge = span_at(
            &arc.spans[usize::from(t >= 1.0)],
            if t >= 1.0 { t - 1.0 } else { t },
        );
        let p = [0, 1, 2].map(|a| on_patch[a] + 0.25 * (on_edge[a] - on_patch[a]));
        let above = [0, 1, 2].map(|a| p[a] + UP[a] / 1024.0);
        let below = [0, 1, 2].map(|a| p[a] - UP[a] / 64.0);
        for (q, dir) in [(above, DOWN), (below, UP)] {
            let expect = in_half_cylinder(q).expect("off the surface");
            assert_eq!(
                wind(&exact, &Shift::NONE, q, &Shift::NONE, dir).0,
                expect,
                "t {t} {q:?}"
            );
        }
        let ribbon = |q: [f64; 3], dir: [f64; 3]| {
            exact[0]
                .ray_parts(&Shift::NONE, q, &Shift::NONE, dir, |p| p.is_ribbon())
                .expect("count")
                .count
        };
        assert_eq!((ribbon(below, UP), ribbon(above, DOWN)), (0, 0), "t {t}");
        let reaches = |p: &Piece| !p.preds.is_empty() && ray_meets_box(below, UP, &p.bbox, 0.0);
        assert!(
            exact[0].pieces.iter().any(reaches),
            "t {t}: no ribbon piece with tile conditions on the way"
        );
    }
}

#[test]
fn trim_refinement_keeps_the_curves_through_a_box() {
    // The top cap's trims: the diameter v = 0 and the arc
    // (2u − 1)² + v² = 1 over UV [0, 1]².
    let faces = exact_faces(&half_cylinder());
    let trims = &faces[3].trims;
    let d = |x: f64| Dy::from_f64(x);
    let free =
        |lo: [f64; 2], hi: [f64; 2]| trim::refine(trims.pieces(), &lo.map(d), &hi.map(d), 64).1;
    // The arc passes through u = 0.2 at v = 0.8 (a box once wrongly
    // cleared, its sides not on a grid of quarters).
    assert!(!free([0.0078125, 0.4375], [0.255859375, 1.0]));
    assert!(!free([0.0, 0.0], [1.0, 0.0078125]));
    assert!(free([0.25, 0.125], [0.75, 0.5]));
    assert!(free([0.0, 0.875], [0.0625, 1.0]));
    let wind_at = |p: [f64; 2]| trims.winding(&p.map(d), 64);
    assert_eq!(wind_at([0.5, 0.25]), Some(1));
    assert_eq!(wind_at([0.03125, 0.9375]), Some(0));
    assert_eq!(wind_at([0.5, -0.25]), Some(0));
}

#[test]
fn exact_faces_are_shared_between_threads() {
    fn shared<T: Send + Sync>() {}
    shared::<ExactFace>();
    shared::<Shift>();
    let faces = exact_faces(&half_cylinder());
    let counts: Vec<i64> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..4)
            .map(|i| {
                let faces = &faces;
                s.spawn(move || {
                    wind(
                        faces,
                        &Shift::NONE,
                        [0.125 * i as f64, 0.5, 0.25],
                        &Shift::NONE,
                        UP,
                    )
                    .0
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("thread"))
            .collect()
    });
    assert_eq!(counts, [1, 1, 1, 1]);
}

/// The box `o + Σ xᵢ·axes[i]`, `x ∈ [0, 1]³`, as a modeller with huge
/// plane cells stores it: each face's tile is a square `size` wide
/// around the face (its control points rounded at that scale, so the
/// patch planes miss the box's vertices by about 1e-10), the face in the
/// middle of the tile's UV, and every p-curve's middle control point
/// moved off its chord by up to `bow` (UV).
fn huge_cell_box(
    o: [f64; 3],
    axes: [[f64; 3]; 3],
    size: f64,
    bow: f64,
    rng: &mut Rng,
) -> Vec<Face> {
    let det = axes[0][0] * (axes[1][1] * axes[2][2] - axes[1][2] * axes[2][1])
        - axes[0][1] * (axes[1][0] * axes[2][2] - axes[1][2] * axes[2][0])
        + axes[0][2] * (axes[1][0] * axes[2][1] - axes[1][1] * axes[2][0]);
    let v = |i: usize| {
        [0, 1, 2].map(|k| {
            o[k] + (0..3)
                .map(|a| if i >> a & 1 == 1 { axes[a][k] } else { 0.0 })
                .sum::<f64>()
        })
    };
    let quads = [
        [0, 4, 6, 2],
        [1, 3, 7, 5],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 2, 3, 1],
        [4, 5, 7, 6],
    ];
    quads
        .iter()
        .map(|q| {
            // Counter-clockwise from outside for a right-handed frame.
            let q = if det > 0.0 {
                *q
            } else {
                [q[0], q[3], q[2], q[1]]
            };
            let (a, b, d) = (v(q[0]), v(q[1]), v(q[3]));
            let (eu, ev) = (
                [0, 1, 2].map(|k| b[k] - a[k]),
                [0, 1, 2].map(|k| d[k] - a[k]),
            );
            let centre = [0, 1, 2].map(|k| a[k] + 0.5 * eu[k] + 0.5 * ev[k]);
            let scale = size / (eu.iter().map(|x| x * x).sum::<f64>()).sqrt();
            // The face's point (s, t) (along eu, ev, in [0, 1]) is at UV
            // (0.5 + (s − 0.5)/scale, 0.5 + (t − 0.5)/scale).
            let at = |u: f64, w: f64| {
                [0, 1, 2].map(|k| centre[k] + (u - 0.5) * scale * eu[k] + (w - 0.5) * scale * ev[k])
            };
            let net: [[f64; 4]; 9] =
                std::array::from_fn(|n| p4(at((n / 3) as f64 / 2.0, (n % 3) as f64 / 2.0), 1.0));
            let uv = |corner: usize| {
                let (s, t) = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)][corner];
                [0.5 + (s - 0.5) / scale, 0.5 + (t - 0.5) / scale]
            };
            let lp = (0..4)
                .map(|k| {
                    let (x, y) = (q[k], q[(k + 1) % 4]);
                    let (s, e) = (x.min(y), x.max(y));
                    let us = uv(q.iter().position(|&c| c == s).expect("corner"));
                    let ue = uv(q.iter().position(|&c| c == e).expect("corner"));
                    let mut pcurve = line_uv(us, ue, [0.0, 1.0]);
                    pcurve.spans[0][1][0] += rng.range(-bow, bow);
                    pcurve.spans[0][1][1] += rng.range(-bow, bow);
                    Coedge {
                        pcurve,
                        curve: line3(v(s), v(e), [0.0, 1.0]),
                        t_range: [0.0, 1.0],
                        reversed: x > y,
                    }
                })
                .collect();
            Face {
                tiles: vec![([0, 0], net)],
                loops: vec![lp],
            }
        })
        .collect()
}

#[test]
fn ties_at_a_vertex_are_one_configuration() {
    // At a vertex every ribbon ends in the segment from the vertex to
    // its patch's corner, which misses the vertex by the rounding of the
    // face data (here about 1e-10: plane cells 4e6 wide; still 1e-16
    // for small cells). Within that distance the counting surface is a
    // cone through those rounding directions, not the ideal corner, so a
    // tie shift (2^-512) may land inside it with any winding, even
    // where the ideal box has the point outside. Whatever the count, it
    // is one configuration's: every ray direction must agree, for the
    // tie shift and for explicit shifts; and an explicit shift far beyond
    // that zone must give the point moved in f64. (Recounting only the
    // tied pieces shifted, as once done, mixed two configurations: wrong
    // once the shift exceeds the other pieces' separations.)
    let mut rng = Rng::new(11);
    let o = [
        -1.5785136201804029,
        -0.02477300372817126,
        -0.5648703278880334,
    ];
    let axes = [
        [1.101584674776772, -0.31336764483233703, -0.7960797176421429],
        [0.5976830570221477, 1.3924550157159792, 0.27892702484257276],
        [0.9824898121093227, -0.7534566793906026, 1.6561214109972102],
    ];
    let dirs = [
        UP,
        DOWN,
        [1.0, 0.0, 0.0],
        [-1.0, 0.25, 0.5],
        [0.3, -1.0, 0.1],
        [0.7, 0.7, -0.2],
    ];
    let corner = |i: usize| {
        [0, 1, 2].map(|k| {
            o[k] + (0..3)
                .map(|a| if i >> a & 1 == 1 { axes[a][k] } else { 0.0 })
                .sum::<f64>()
        })
    };
    let d = [-0.12076856999888481, 0.0989474181185174, 0.9877369897640281];
    let mut ties = 0;
    for (size, bow) in [(4e6, 2e-13), (4.0, 2e-7)] {
        let faces = exact_faces(&huge_cell_box(o, axes, size, bow, &mut rng));
        for vertex in [4, 0, 7] {
            let q = corner(vertex);
            for (name, shift, far) in [
                ("tie +", Shift::tie(d, 1), None),
                ("tie −", Shift::tie(d, -1), None),
                ("2^-40", Shift::exact(d, -40), None),
                ("2^-30", Shift::exact(d, -30), Some(-30)),
                ("2^-20", Shift::exact(d, -20), Some(-20)),
            ] {
                let w: Vec<(i64, bool)> = dirs
                    .iter()
                    .map(|&dir| wind(&faces, &Shift::NONE, q, &shift, dir))
                    .collect();
                ties += w.iter().filter(|x| x.1).count();
                assert!(
                    w.iter().all(|x| x.0 == w[0].0),
                    "size {size} vertex {vertex} {name}: {w:?}"
                );
                if let Some(k) = far {
                    let m = [0, 1, 2].map(|a| q[a] + libm::scalbn(d[a], k));
                    let at_m = wind(&faces, &Shift::NONE, m, &Shift::NONE, UP).0;
                    assert_eq!(
                        w[0].0, at_m,
                        "size {size} vertex {vertex} {name}: the moved point {m:?}"
                    );
                    assert!((0..=1).contains(&at_m));
                }
            }
        }
    }
    assert!(ties > 0, "no ties seen");
}

#[test]
#[ignore = "timing: run by hand with --release -- --ignored --nocapture"]
fn timing() {
    use std::time::Instant;
    let mut rng = Rng::new(9);
    let thin = thin_box(&mut rng);
    let solids = [
        ("half cylinder", exact_faces(&half_cylinder())),
        ("thin-ribbon box", exact_faces(&thin)),
    ];
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64 * 1e6;
    let t = Instant::now();
    let raw = [half_cylinder(), thin.clone()];
    let n: usize = raw.iter().map(|f| exact_faces(f).len()).sum();
    println!(
        "ExactFace::new: {:.0} µs per face",
        t.elapsed().as_secs_f64() / n as f64 * 1e6
    );
    for (name, faces) in &solids {
        // Per face query, split by whether it counted a crossing.
        let (mut miss, mut hit) = (Vec::new(), Vec::new());
        for _ in 0..1000 {
            let q = random_point(&mut rng, [-3.0, -3.0, -3.0], [3.0, 3.0, 3.0]);
            for f in faces {
                let t = Instant::now();
                let c = f.ray(&Shift::NONE, q, &Shift::NONE, UP).expect("count");
                let dt = t.elapsed().as_secs_f64();
                if c.count == 0 {
                    miss.push(dt)
                } else {
                    hit.push(dt)
                }
            }
        }
        println!(
            "{name} ray: miss {:.2} µs ({}), hit {:.1} µs ({})",
            mean(&miss),
            miss.len(),
            mean(&hit),
            hit.len()
        );
        let (mut miss, mut hit) = (Vec::new(), Vec::new());
        for _ in 0..200 {
            let c = random_chain(&mut rng, [-1.5, -1.0, -0.75], [1.75, 1.75, 1.125]);
            for f in faces {
                let t = Instant::now();
                let x = f.curve(&Shift::NONE, &c, &Shift::NONE).expect("count");
                let dt = t.elapsed().as_secs_f64();
                if x.count == 0 {
                    miss.push(dt)
                } else {
                    hit.push(dt)
                }
            }
        }
        println!(
            "{name} curve: miss {:.1} µs ({}), hit {:.1} µs ({})",
            mean(&miss),
            miss.len(),
            mean(&hit),
            hit.len()
        );
    }
    let faces = &solids[0].1;
    let d = [0.25, -1.0, 0.5];
    for (name, q) in [
        ("tile line", [0.0, R, 0.5]),
        ("arc knot", [0.0, R, 0.0]),
        ("plane", [0.25, 0.0, 0.5]),
    ] {
        let t = Instant::now();
        wind(faces, &Shift::NONE, q, &Shift::tie(d, 1), UP);
        println!(
            "tied winding at {name}: {:.1} ms",
            t.elapsed().as_secs_f64() * 1e3
        );
    }
}

/// A shift made with [`Shift::always`] is the configuration counted,
/// tied or not: a point just inside a face (by `2⁻⁴⁰`, a configuration
/// with no tie) moved out by `2⁻²⁰` is outside, while the same move as
/// a tie shift ([`Shift::exact`]) leaves the untied point inside.
#[test]
fn always_shifts_count_the_moved_configuration() {
    let faces = exact_faces(&plain_box([false; 6]));
    let up = [2.0, 3.0, 32.0];
    for (axis, low) in [(0, true), (1, false), (2, true)] {
        let mut q = [0.125, 0.25, 0.125];
        let (at, inward) = if low {
            (LO[axis], 1.0)
        } else {
            (HI[axis], -1.0)
        };
        q[axis] = at + inward * (1.0 / 1_099_511_627_776.0);
        let mut out = [0.0; 3];
        out[axis] = -inward;
        let (kept, tie) = wind(&faces, &Shift::NONE, q, &Shift::exact(out, -20), up);
        assert_eq!((kept, tie), (1, false), "axis {axis}");
        let (moved, tie) = wind(&faces, &Shift::NONE, q, &Shift::always(out, -20), up);
        assert_eq!((moved, tie), (0, true), "axis {axis}");
    }
}

/// A p-curve running along a tile line, across another tile line: its
/// ribbon is counted once, from one of the two tiles it lies between,
/// not from both. The face is the square `[1, 2] × [½, 3/2]` of the
/// plane `z = 0` tiled `2 × 2` over `[0, 2]²`; its left side lies on
/// `u = 1` across `v = 1`, its edge bowed up to `z = 1/64` in the middle.
#[test]
fn a_pcurve_along_a_tile_line_has_one_ribbon() {
    let tile = |i: i64, j: i64| {
        let (x, y) = (i as f64, j as f64);
        (
            [i, j],
            bilinear([
                [x, y, 0.0],
                [x + 1.0, y, 0.0],
                [x + 1.0, y + 1.0, 0.0],
                [x, y + 1.0, 0.0],
            ]),
        )
    };
    let corners = [[1.0, 0.5], [2.0, 0.5], [2.0, 1.5], [1.0, 1.5]];
    let loop0 = (0..4)
        .map(|k| {
            let (a, b) = (corners[k], corners[(k + 1) % 4]);
            let mut curve = line3([a[0], a[1], 0.0], [b[0], b[1], 0.0], [0.0, 1.0]);
            if k == 3 {
                curve.spans[0][1][2] = 1.0 / 32.0;
            }
            Coedge {
                pcurve: line_uv(a, b, [0.0, 1.0]),
                curve,
                t_range: [0.0, 1.0],
                reversed: false,
            }
        })
        .collect();
    let face = Face {
        tiles: vec![tile(0, 0), tile(1, 0), tile(0, 1), tile(1, 1)],
        loops: vec![loop0],
    };
    let exact = ExactFace::new(&face);
    // Rays along +x through the ribbon (the wall x = 1, 0 ≤ z ≤ bow)
    // below and above the tile line v = 1.
    for y in [0.75, 1.25] {
        let q = [0.5, y, 1.0 / 256.0];
        let ribbon = exact
            .ray_parts(&Shift::NONE, q, &Shift::NONE, [1.0, 0.0, 0.0], |p| {
                p.is_ribbon()
            })
            .expect("count")
            .count;
        assert_eq!(ribbon.abs(), 1, "y {y}");
    }
}

/// A point a few subnormals inside a face on the plane `z = 0` is no
/// tie: it counts as inside, unshifted, and a tie shift pointing out of
/// the box doesn't move it out. The same for a curve ending there.
#[test]
fn points_subnormally_inside_a_face_are_not_ties() {
    let lo = [-1.0, -0.75, 0.0];
    let faces = exact_faces(&box_faces(lo, HI, [false; 6], |_| [0.0, 1.0]));
    let out = Shift::tie([0.25, 0.5, -1.0], 1);
    for z in [f64::from_bits(1), f64::from_bits(7) * 1e10, 1e-200, -1e-200] {
        let q = [0.125, 0.25, z];
        let expect = i64::from(z > 0.0);
        for dir in [UP, DOWN] {
            for qs in [&Shift::NONE, &out] {
                let c = faces
                    .iter()
                    .map(|f| f.ray(&Shift::NONE, q, qs, dir))
                    .collect::<Result<Vec<_>, _>>();
                let c = c.map(|c| {
                    (
                        c.iter().map(|x| x.count).sum::<i64>(),
                        c.iter().any(|x| x.tie),
                    )
                });
                assert_eq!(c, Ok((expect, false)), "z {z:e} dir {dir:?}");
            }
        }
        let c = line3([0.125, 0.25, 0.5], q, [0.0, 1.0]);
        let (n, tie) = crossings(&faces, &Shift::NONE, &c, &out);
        assert_eq!((n, tie), (expect - 1, false), "z {z:e}");
    }
    // On the face, and on it a few subnormals off its edge's line x = 0
    // (of the box with that corner too): ties, resolved as the moved
    // point (out, or in), the shift far below the subnormals.
    let q = [0.125, 0.25, 0.0];
    let r = [f64::from_bits(3), 0.25, 0.0];
    let faces = exact_faces(&box_faces([0.0, -0.75, 0.0], HI, [false; 6], |_| {
        [0.0, 1.0]
    }));
    for (p, sigma, expect) in [(q, 1, 0), (q, -1, 1), (r, 1, 0), (r, -1, 1)] {
        for dir in [UP, DOWN] {
            let s = Shift::tie([0.25, 0.5, -1.0], sigma);
            assert_eq!(
                wind(&faces, &Shift::NONE, p, &s, dir),
                (expect, true),
                "{p:?}"
            );
        }
    }
}

/// A box `2^-1000` in size: a point inside it near a face is no tie,
/// and one on a face is resolved as the moved point, though the tie
/// shift `2^-512` is far larger than the box.
#[test]
fn a_tiny_box_counts_as_a_unit_box() {
    let k = libm::scalbn(1.0, -1000);
    let (lo, hi) = (LO.map(|x| x * k), HI.map(|x| x * k));
    let faces = exact_faces(&box_faces(lo, hi, [false; 6], |_| [0.0, 1.0]));
    let inside = [0.125 * k, 0.25 * k, lo[2] + libm::scalbn(1.0, -1040)];
    let on = [0.125 * k, 0.25 * k, lo[2]];
    let out = Shift::tie([0.25, 0.5, -1.0], 1);
    let into = Shift::tie([0.25, 0.5, -1.0], -1);
    for dir in [UP, DOWN] {
        assert_eq!(wind(&faces, &Shift::NONE, inside, &out, dir), (1, false));
        assert_eq!(wind(&faces, &Shift::NONE, on, &out, dir), (0, true));
        assert_eq!(wind(&faces, &Shift::NONE, on, &into, dir), (1, true));
    }
}
