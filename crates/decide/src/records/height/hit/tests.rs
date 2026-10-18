use std::cmp::Ordering;

use glam::DVec3;
use malachite::Rational;
use varde_testkit::ray_oracle::{Chain, Coedge, ExactFace, Face, Shift, UvChain};
use varde_testkit::rng::Rng;

use super::*;
use crate::count::pieces::SpanMap;
use crate::count::testing::motion_of;
use crate::num::bounded::Bounded;
use crate::perturb::{MovingAt, ribbon_row};

mod curved;

/// A plane's map of the UV, `origin + u·du + v·dv`, exact on dyadic data.
#[derive(Clone, Copy, Debug)]
struct PlaneFrame {
    origin: [f64; 3],
    du: [f64; 3],
    dv: [f64; 3],
}

impl PlaneFrame {
    fn at(&self, u: f64, v: f64) -> [f64; 3] {
        std::array::from_fn(|k| self.origin[k] + u * self.du[k] + v * self.dv[k])
    }

    /// The net of tile `[0, 1]²`: the map at the half steps, weights 1.
    fn net(&self) -> TileNet {
        std::array::from_fn(|n| {
            let p = self.at((n / 3) as f64 / 2.0, (n % 3) as f64 / 2.0);
            [p[0], p[1], p[2], 1.0]
        })
    }
}

/// A test plane face: one tile `[0, 1]²` on an exact frame, a loop of
/// straight edges between UV points mapped exactly, its p-curves'
/// middle control points pushed off the chords (the ribbons have width).
struct TestFace {
    frame: PlaneFrame,
    nets: Vec<TileNet>,
    pspans: Vec<UvSpan>,
    curves: Vec<Span>,
    reversed: Vec<bool>,
    maps: Vec<SpanMap>,
}

fn dyadic(rng: &mut Rng, lo: f64, hi: f64, bits: u32) -> f64 {
    let k = (1u64 << bits) as f64;
    (rng.range(lo, hi) * k).round() / k
}

impl TestFace {
    /// A random face, its p-curves pushed off their chords by up to
    /// `push`, its edges' vertices lifted off its plane by up to `lift`
    /// along `z` (the edges between faces that meet at an angle, so the
    /// ribbons leave the plane).
    fn random(rng: &mut Rng, push: f64, lift: f64) -> TestFace {
        let mut d = || dyadic(rng, -0.75, 0.75, 6);
        let frame = PlaneFrame {
            origin: [0.0, 0.0, 0.0],
            du: [1.0, d(), d()],
            dv: [d(), 1.0, d()],
        };
        // A convex quad round the tile's centre, counter-clockwise.
        let corners: Vec<[f64; 2]> = (0..4)
            .map(|k| {
                let a = std::f64::consts::FRAC_PI_2 * k as f64 + rng.range(-0.4, 0.4);
                let r = rng.range(0.2, 0.4);
                let (s, c) = crate::num::trig::sin_cos(a);
                [0.5 + r * c, 0.5 + r * s].map(|x| (x * 256.0).round() / 256.0)
            })
            .collect();
        let lifts: Vec<f64> = (0..4).map(|_| dyadic(rng, -lift, lift, 20)).collect();
        let at = |k: usize| -> [f64; 3] {
            let p = corners[k];
            let mut x: [f64; 3] =
                std::array::from_fn(|i| frame.origin[i] + p[0] * frame.du[i] + p[1] * frame.dv[i]);
            x[2] += lifts[k];
            x
        };
        let (mut pspans, mut curves, mut reversed) = (Vec::new(), Vec::new(), Vec::new());
        for k in 0..4 {
            let (ka, kb) = (k, (k + 1) % 4);
            // Stored along the loop or against it.
            let back = rng.next_u64() % 2 == 1;
            let (ka, kb) = if back { (kb, ka) } else { (ka, kb) };
            let (a, b) = (corners[ka], corners[kb]);
            let m = [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1])];
            let off = [dyadic(rng, -push, push, 20), dyadic(rng, -push, push, 20)];
            pspans.push([
                [a[0], a[1], 1.0],
                [m[0] + off[0], m[1] + off[1], 1.0],
                [b[0], b[1], 1.0],
            ]);
            let (pa, pb) = (at(ka), at(kb));
            let pm: [f64; 3] = std::array::from_fn(|k| 0.5 * (pa[k] + pb[k]));
            curves.push([
                [pa[0], pa[1], pa[2], 1.0],
                [pm[0], pm[1], pm[2], 1.0],
                [pb[0], pb[1], pb[2], 1.0],
            ]);
            reversed.push(back);
        }
        TestFace {
            nets: vec![frame.net()],
            frame,
            pspans,
            curves,
            reversed,
            maps: vec![
                SpanMap {
                    span: 0,
                    t: [0.0, 1.0]
                };
                4
            ],
        }
    }

    fn flat(&self) -> HitFace<'_> {
        HitFace {
            operand: Operand::B,
            face: FaceId(0),
            tiles: [0..1, 0..1],
            nets: &self.nets,
            inside: 1,
            trims: (0..4)
                .map(|k| HitTrim {
                    coedge: CoedgeId(k as u32),
                    edge: EdgeId(k as u32),
                    reversed: self.reversed[k],
                    ends: [VertId(0), VertId(1)],
                    along: Along::Stored,
                    band: 1e-3,
                    pspans: std::slice::from_ref(&self.pspans[k]),
                    curve: std::slice::from_ref(&self.curves[k]),
                    map: std::slice::from_ref(&self.maps[k]),
                })
                .collect(),
        }
    }

    fn oracle(&self) -> ExactFace {
        let net = self.nets[0];
        let loops = vec![
            (0..4)
                .map(|k| Coedge {
                    pcurve: UvChain {
                        spans: vec![self.pspans[k]],
                        knots: vec![0.0, 1.0],
                    },
                    curve: Chain {
                        spans: vec![self.curves[k]],
                        knots: vec![0.0, 1.0],
                    },
                    t_range: [0.0, 1.0],
                    reversed: self.reversed[k],
                })
                .collect(),
        ];
        ExactFace::new(&Face {
            tiles: vec![([0, 0], net)],
            loops,
        })
    }

    /// A point of edge `k` at `t`.
    fn edge_point(&self, k: usize, t: f64) -> DVec3 {
        let c = &self.curves[k];
        DVec3::from_array([0, 1, 2].map(|i| (1.0 - t) * c[0][i] + t * c[2][i]))
    }
}

/// The upward ray's count from the hits: the facings of those above.
fn count_up(hits: &[Hit]) -> Option<i64> {
    let mut n = 0;
    for h in hits {
        match h.height {
            Ordering::Equal => return None,
            Ordering::Greater => n += i64::from(h.facing?.as_i8()),
            Ordering::Less => {}
        }
    }
    Some(n)
}

#[test]
fn hits_count_as_the_oracle_near_and_away_from_edges() {
    let mut rng = Rng::new(0x41f);
    let (mut checked, mut with_ribbon) = (0, 0);
    let up = [UP.x, UP.y, UP.z];
    for _ in 0..30 {
        let tf = TestFace::random(&mut rng, 1.0 / 64.0, 1.0 / 64.0);
        let face = tf.flat();
        let oracle = tf.oracle();
        for _ in 0..30 {
            // Near an edge's point within a few ribbon widths, or anywhere
            // over the tile.
            let q = if rng.next_u64().is_multiple_of(4) {
                let u = rng.range(-0.1, 1.1);
                let v = rng.range(-0.1, 1.1);
                let f = &tf.frame;
                DVec3::from_array(std::array::from_fn(|k| u * f.du[k] + v * f.dv[k]))
                    + rng.direction() * rng.range(0.0, 0.5)
            } else {
                let k = (rng.next_u64() % 4) as usize;
                tf.edge_point(k, rng.range(0.05, 0.95)) + rng.direction() * rng.range(0.0, 0.03)
            };
            let hits = face_hits(&face, q.to_array(), &motion_of(Operand::A)).expect("hits");
            let Some(mine) = count_up(&hits) else {
                continue;
            };
            let c = oracle
                .ray(&Shift::NONE, q.to_array(), &Shift::NONE, up)
                .expect("settled");
            if c.tie {
                continue;
            }
            assert_eq!(mine, c.count, "q {q:?}, hits {hits:#?}");
            checked += 1;
            with_ribbon += usize::from(hits.iter().any(|h| matches!(h.sheet, Sheet::Ribbon(..))));
        }
    }
    assert!(checked > 600, "{checked}");
    assert!(with_ribbon > 50, "{with_ribbon}");
}

#[test]
fn a_tile_and_its_ribbon_straddling_the_point_count_each_by_its_height() {
    // The p-curve pushed far out: next to the edge the column meets the
    // tile on one side of `q` and the ribbon on the other, a piece of
    // degree 0 whose two hits don't cancel in the upward count.
    let mut rng = Rng::new(7);
    let mut straddled = 0;
    for _ in 0..2000 {
        let tf = TestFace::random(&mut rng, 1.0 / 8.0, 1.0 / 4.0);
        let face = tf.flat();
        let k = (rng.next_u64() % 4) as usize;
        // Next to the edge in the face's plane, a little above or below.
        let up = UP.normalize() * rng.range(-0.02, 0.02);
        let side = rng.direction() * rng.range(0.0, 0.03);
        let q = tf.edge_point(k, rng.range(0.2, 0.8)) + side + up;
        let hits = face_hits(&face, q.to_array(), &motion_of(Operand::A)).expect("hits");
        let above = hits
            .iter()
            .filter(|h| h.height == Ordering::Greater)
            .count();
        if above > 0 && above < hits.len() {
            straddled += 1;
            let c = tf
                .oracle()
                .ray(&Shift::NONE, q.to_array(), &Shift::NONE, [UP.x, UP.y, UP.z])
                .expect("settled");
            assert_eq!(count_up(&hits), Some(c.count), "q {q:?}");
        }
    }
    assert!(straddled > 20, "{straddled}");
}

#[test]
fn the_winding_counts_each_crossing_once() {
    let tf = TestFace {
        frame: PlaneFrame {
            origin: [0.0; 3],
            du: [1.0, 0.0, 0.0],
            dv: [0.0, 1.0, 0.0],
        },
        nets: vec![
            PlaneFrame {
                origin: [0.0; 3],
                du: [1.0, 0.0, 0.0],
                dv: [0.0, 1.0, 0.0],
            }
            .net(),
        ],
        // The square `[¼, ¾]²` counter-clockwise, its sides straight.
        pspans: [
            ([0.25, 0.25], [0.75, 0.25]),
            ([0.75, 0.25], [0.75, 0.75]),
            ([0.75, 0.75], [0.25, 0.75]),
            ([0.25, 0.75], [0.25, 0.25]),
        ]
        .map(|(a, b)| {
            [
                [a[0], a[1], 1.0],
                [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1]), 1.0],
                [b[0], b[1], 1.0],
            ]
        })
        .to_vec(),
        curves: vec![[[0.0; 4]; 3]; 4],
        reversed: vec![false; 4],
        maps: vec![
            SpanMap {
                span: 0,
                t: [0.0, 1.0]
            };
            4
        ],
    };
    let face = tf.flat();
    let w = |u: (i64, i64), v: (i64, i64)| {
        winding(
            &face,
            &[
                Rational::from_signeds(u.0, u.1),
                Rational::from_signeds(v.0, v.1),
            ],
        )
    };
    assert_eq!(w((1, 2), (1, 2)), Ok(1));
    assert_eq!(w((1, 8), (1, 2)), Ok(0));
    assert_eq!(w((7, 8), (1, 2)), Ok(0));
    // On the ray's line through the bottom side's line, and through a
    // vertex: still counted once, or not at all.
    assert_eq!(w((1, 2), (1, 4)).map_err(|e| e.trim), Err(0));
    assert_eq!(w((1, 8), (1, 4)), Ok(0));
    assert_eq!(w((1, 8), (3, 4)), Ok(0));
    assert_eq!(w((7, 8), (3, 4)), Ok(0));
    // A point on a side.
    assert_eq!(w((3, 4), (1, 2)).map_err(|e| e.trim), Err(1));
}

#[test]
fn spans_meet_rectangles_exactly() {
    let r = |a: f64, b: f64, c: f64, d: f64| -> ExactRect { [[rat(a), rat(b)], [rat(c), rat(d)]] };
    // A parabola from (0, 0) to (1, 0) through (½, ½)'s control point:
    // its top at v = ¼.
    let sp: UvSpan = [[0.0, 0.0, 1.0], [0.5, 0.5, 1.0], [1.0, 0.0, 1.0]];
    assert!(span_meets(&sp, &r(0.4, 0.6, 0.2, 0.3)));
    assert!(span_meets(&sp, &r(0.4, 0.6, 0.25, 0.3)));
    assert!(!span_meets(&sp, &r(0.4, 0.6, 0.2500001, 0.3)));
    assert!(!span_meets(&sp, &r(2.0, 3.0, -1.0, 1.0)));
    assert!(span_meets(&sp, &r(0.9, 2.0, -1.0, 1.0)));
    // A span along a side's line, past its end or reaching it.
    let along: UvSpan = [[0.0, 0.0, 1.0], [0.5, 0.0, 1.0], [1.0, 0.0, 1.0]];
    assert!(!span_meets(&along, &r(1.5, 2.0, 0.0, 1.0)));
    assert!(span_meets(&along, &r(1.0, 2.0, 0.0, 1.0)));
}

/// The ribbon of trim `k` at `(τ, λ)` and its partials, in `f64`.
fn ribbon_at(tf: &TestFace, k: usize, tau: f64, lambda: f64) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let sp = &tf.pspans[k];
    let b = [
        (1.0 - tau) * (1.0 - tau),
        2.0 * tau * (1.0 - tau),
        tau * tau,
    ];
    let db = [-2.0 * (1.0 - tau), 2.0 - 4.0 * tau, 2.0 * tau];
    let f = &tf.frame;
    let map = |u: f64, v: f64| -> [f64; 3] { std::array::from_fn(|i| u * f.du[i] + v * f.dv[i]) };
    let uv = |w: &[f64; 3]| [0, 1].map(|i| (0..3).map(|j| w[j] * sp[j][i]).sum::<f64>());
    let (p, dp) = (map(uv(&b)[0], uv(&b)[1]), map(uv(&db)[0], uv(&db)[1]));
    let c = &tf.curves[k];
    let cp: [f64; 3] = std::array::from_fn(|i| (0..3).map(|j| b[j] * c[j][i]).sum());
    let dc: [f64; 3] = std::array::from_fn(|i| (0..3).map(|j| db[j] * c[j][i]).sum());
    let r = std::array::from_fn(|i| (1.0 - lambda) * p[i] + lambda * cp[i]);
    let rt = std::array::from_fn(|i| (1.0 - lambda) * dp[i] + lambda * dc[i]);
    let rl = std::array::from_fn(|i| cp[i] - p[i]);
    (r, rt, rl)
}

/// The ribbon hit of trim `k`'s column through `q` near `(τ, λ)` by
/// Newton in `f64`: `(τ, λ, s)`, `R(τ, λ) = q + s·UP`.
fn newton(tf: &TestFace, k: usize, q: DVec3, mut x: [f64; 3]) -> [f64; 3] {
    let up = UP;
    for _ in 0..50 {
        let (r, rt, rl) = ribbon_at(tf, k, x[0], x[1]);
        let f = DVec3::from_array(r) - q - up * x[2];
        let j = glam::DMat3::from_cols(DVec3::from_array(rt), DVec3::from_array(rl), -up);
        let d = j.inverse() * f;
        x = [x[0] - d.x, x[1] - d.y, x[2] - d.z];
    }
    x
}

#[test]
fn the_ribbon_hit_row_is_the_derivative_of_the_height() {
    let mut rng = Rng::new(0xfd);
    let mut checked = 0;
    while checked < 40 {
        let tf = TestFace::random(&mut rng, 1.0 / 16.0, 1.0 / 16.0);
        let face = tf.flat();
        let k = (rng.next_u64() % 4) as usize;
        let q = tf.edge_point(k, rng.range(0.3, 0.7)) + rng.direction() * rng.range(0.0, 0.02);
        let hits = ribbon_hits(&face, k, 0, q.to_array()).expect("hits");
        let Some(h) = hits.into_iter().find(|h| h.height != Ordering::Equal) else {
            continue;
        };
        let HitAt::Ribbon { tau, .. } = &h.at else {
            unreachable!()
        };
        let mut tau = tau.clone();
        tau.narrow(&Rational::from_signeds(1, 1i64 << 50));
        let t0 = tau.enclose()[0];
        // λ from the shadow: a start for Newton.
        let x0 = newton(&tf, k, q, [t0, 0.5, 0.0]);
        // A motion of `q` (the ribbon of `B` fixed).
        let dir = rng.direction();
        let step = 1e-6;
        let hp = newton(&tf, k, q + dir * step, x0)[2];
        let hm = newton(&tf, k, q - dir * step, x0)[2];
        let fd = (hp - hm) / (2.0 * step) * UP.length();
        let (_, rt, rl) = ribbon_at(&tf, k, x0[0], x0[1]);
        let b = |v: [f64; 3]| v.map(Bounded::exact);
        let zero = [Bounded::exact(0.0); 3];
        let qm = MovingAt {
            at: b(q.to_array()),
            rates: [b(dir.to_array()), zero, zero, zero],
        };
        let row = ribbon_row(&b(rt), &b(rl), &[zero; ORDERS], &qm).expect("transversal");
        let n = DVec3::from_array(rt)
            .cross(DVec3::from_array(rl))
            .normalize();
        let want = fd * n.dot(UP.normalize()).abs();
        let got = row[0].v;
        assert!(
            (got - want).abs() <= 1e-5 * (1.0 + want.abs()),
            "row {got}, finite differences {want}"
        );
        // The exact row's sign is the derivative's.
        let deltas: [Q3; ORDERS] = std::array::from_fn(|n| {
            if n == 0 {
                (-dir).to_array().map(rat)
            } else {
                [0.0; 3].map(rat)
            }
        });
        let signs = h
            .ribbon_orders(&face, q.to_array(), &deltas)
            .expect("a ribbon hit");
        if want.abs() > 1e-9 {
            let s = if want > 0.0 {
                Ordering::Greater
            } else {
                Ordering::Less
            };
            assert_eq!(signs[0], Ok(s));
        }
        checked += 1;
    }
}

#[test]
fn a_point_on_a_ribbon_is_a_tie_its_orders_decide() {
    // `q` exactly on the ribbon of trim 0 at `(τ, λ) = (½, ¼)`, dyadic
    // data: the hit's height is an exact zero, and its orders say which
    // way the moved point leaves the ribbon.
    let mut rng = Rng::new(3);
    let mut tied = 0;
    for _ in 0..20 {
        let tf = TestFace::random(&mut rng, 1.0 / 16.0, 1.0 / 16.0);
        let face = tf.flat();
        let (r, rt, rl) = ribbon_at(&tf, 0, 0.5, 0.25);
        let q = DVec3::from_array(r);
        let hits = ribbon_hits(&face, 0, 0, r).expect("hits");
        let Some(h) = hits.iter().find(|h| h.height == Ordering::Equal) else {
            continue;
        };
        tied += 1;
        for _ in 0..4 {
            let dir = rng.direction();
            let deltas: [Q3; ORDERS] = std::array::from_fn(|n| {
                if n == 0 {
                    (-dir).to_array().map(rat)
                } else {
                    [0.0; 3].map(rat)
                }
            });
            let signs = h.ribbon_orders(&face, r, &deltas).expect("a ribbon hit");
            let x0 = [0.5, 0.25, 0.0];
            let moved = newton(&tf, 0, q + dir * 1e-7, x0)[2];
            let n = DVec3::from_array(rt).cross(DVec3::from_array(rl));
            if moved.abs() > 1e-12 && n.dot(UP).abs() > 1e-6 {
                let s = if moved > 0.0 {
                    Ordering::Greater
                } else {
                    Ordering::Less
                };
                assert_eq!(signs[0], Ok(s), "q {q:?} moved along {dir:?}");
            }
        }
    }
    assert!(tied > 10, "{tied}");
}
