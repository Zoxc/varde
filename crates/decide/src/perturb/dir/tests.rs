//! Directions on random vertex stars: convex corners, reflex ones, knife
//! edges, axis-aligned corners and random links (often saddles), every
//! flat direction checked exactly in rationals (equal offsets, sign
//! agreement, the max–min optimum against every vertex of its linear
//! program, `d = 0` at saddles), the blends, the rows reading exact rates,
//! and the curved directions' certificates.

use malachite::Rational as Q;

use super::*;
use crate::bezier::Scalar;
use crate::num::bigf::{Big, BigF, Width};
use crate::num::bounded::Bounded;
use crate::num::dd::Dd;
use crate::num::decide::Sign;
use crate::num::exact::{self, Signs};
use crate::perturb::{Moving, MovingAt, PlaneHeight, UP};

struct Rng(varde_testkit::rng::Rng);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(varde_testkit::rng::Rng::new(seed))
    }

    fn below(&mut self, n: u64) -> u64 {
        self.0.next_u64() % n
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        self.0.range(lo, hi)
    }

    /// A small integer in `-n..=n`.
    fn int(&mut self, n: u64) -> f64 {
        self.below(2 * n + 1) as f64 - n as f64
    }

    fn ints(&mut self, n: u64) -> DVec3 {
        DVec3::new(self.int(n), self.int(n), self.int(n))
    }
}

/// A vertex star: its edges' directions in turn about the vertex, face
/// `i` between edges `i` and `i + 1` with the outward normal `eᵢ × eᵢ₊₁`
/// (counter-clockwise seen from outside).
#[derive(Debug)]
struct Star {
    edges: Vec<DVec3>,
    normals: Vec<DVec3>,
}

impl Star {
    fn new(edges: Vec<DVec3>) -> Star {
        let k = edges.len();
        let normals = (0..k).map(|i| edges[i].cross(edges[(i + 1) % k])).collect();
        Star { edges, normals }
    }

    /// The faces at edge `i`: `i − 1` and `i`.
    fn edge_faces(&self, i: usize) -> [DVec3; 2] {
        let k = self.edges.len();
        [self.normals[(i + k - 1) % k], self.normals[i]]
    }
}

/// Angles about `z`, sorted, no gap of `π` or more: a convex cone's.
fn turn(r: &mut Rng, k: usize) -> Vec<f64> {
    loop {
        let mut a: Vec<f64> = (0..k)
            .map(|_| r.range(0.0, std::f64::consts::TAU))
            .collect();
        a.sort_by(f64::total_cmp);
        let gap = (0..k)
            .map(|i| {
                let next = if i + 1 < k {
                    a[i + 1]
                } else {
                    a[0] + std::f64::consts::TAU
                };
                next - a[i]
            })
            .fold(0.0, |m: f64, g| if g > m { g } else { m });
        if gap < 3.0 {
            return a;
        }
    }
}

/// The kinds of star the tests draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A convex corner: edges about `−z` on a cone.
    Convex,
    /// Edges about `z` at heights of either sign: reflex edges, a graph
    /// over the plane, so `z` agrees.
    Reflex,
    /// An axis-aligned corner of a box or an L: small integer edges.
    Grid,
    /// A convex corner with one wedge nearly closed: a knife edge.
    Knife,
    /// Random edge directions: often normals in no open half-space.
    Random,
}

const KINDS: [Kind; 5] = [
    Kind::Convex,
    Kind::Reflex,
    Kind::Grid,
    Kind::Knife,
    Kind::Random,
];

fn star(r: &mut Rng, kind: Kind) -> Star {
    let k = 3 + r.below(5) as usize;
    let polar = |a: f64, z: f64| {
        let (s, c) = crate::num::trig::sin_cos(a);
        DVec3::new(c, s, z)
    };
    let edges = match kind {
        Kind::Convex => {
            let h = r.range(0.1, 3.0);
            turn(r, k).into_iter().map(|a| polar(a, -h)).collect()
        }
        Kind::Reflex => {
            let a = turn(r, k);
            a.into_iter()
                .map(|a| polar(a, r.range(-2.0, 2.0)))
                .collect()
        }
        Kind::Grid => loop {
            let e: Vec<DVec3> = (0..k).map(|_| r.ints(1)).collect();
            if e.iter().all(|v| *v != DVec3::ZERO) {
                break e;
            }
        },
        Kind::Knife => {
            let h = r.range(0.1, 3.0);
            let mut e: Vec<DVec3> = turn(r, k.max(4))
                .into_iter()
                .map(|a| polar(a, -h))
                .collect();
            // Edge 1's neighbours meet: faces 0 and 1 nearly back to back.
            let eps = f64::from_bits((1023 - 10 - r.below(40)) << 52);
            e[2] = e[0] + DVec3::new(r.int(2), r.int(2), r.int(2)) * eps;
            e
        }
        Kind::Random => (0..k).map(|_| r.0.direction()).collect(),
    };
    Star::new(edges)
}

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

fn qdot(a: &Q3, b: &Q3) -> Q {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}

fn qcross(a: &Q3, b: &Q3) -> Q3 {
    [
        &a[1] * &b[2] - &a[2] * &b[1],
        &a[2] * &b[0] - &a[0] * &b[2],
        &a[0] * &b[1] - &a[1] * &b[0],
    ]
}

fn is_zero(v: &Q3) -> bool {
    v.iter().all(|x| *x == 0u32)
}

/// Scaled, sorted, without repeats: what [`flat`] solves over.
fn scaled(normals: &[DVec3]) -> Vec<Q3> {
    let mut ns: Vec<Q3> = normals.iter().map(|&n| scaled_normal(n).unwrap()).collect();
    ns.sort();
    ns.dedup();
    ns
}

/// `A·x = b` for a square `A`, exactly; `None` when singular.
fn solve(mut a: Vec<Vec<Q>>, mut b: Vec<Q>) -> Option<Vec<Q>> {
    let n = b.len();
    for c in 0..n {
        let p = (c..n).find(|&r| a[r][c] != 0u32)?;
        a.swap(c, p);
        b.swap(c, p);
        for r in 0..n {
            if r != c && a[r][c] != 0u32 {
                let f = &a[r][c] / &a[c][c];
                for j in c..n {
                    let t = &f * &a[c][j];
                    a[r][j] -= t;
                }
                let t = &f * &b[c];
                b[r] -= t;
            }
        }
    }
    Some((0..n).map(|i| &b[i] / &a[i][i]).collect())
}

/// The max–min value by brute force: every vertex of the program
/// `max t, n·d ≥ t, −1 ≤ dᵢ ≤ 1` in `(d, t)`, four constraints tight,
/// the best feasible one.
fn brute_max_min(ns: &[Q3]) -> Q {
    // Rows `a·(d, t) ≤ b`.
    let mut rows: Vec<([Q; 4], Q)> = Vec::new();
    for n in ns {
        rows.push(([-&n[0], -&n[1], -&n[2], Q::from(1)], Q::from(0)));
    }
    for i in 0..3 {
        for s in [1, -1] {
            let mut a = [0, 1, 2, 3].map(|_| Q::from(0));
            a[i] = Q::from(s);
            rows.push((a, Q::from(1)));
        }
    }
    let m = rows.len();
    let mut best: Option<Q> = None;
    for i in 0..m {
        for j in i + 1..m {
            for k in j + 1..m {
                for l in k + 1..m {
                    let pick = [i, j, k, l];
                    let a = pick.iter().map(|&r| rows[r].0.to_vec()).collect();
                    let b = pick.iter().map(|&r| rows[r].1.clone()).collect();
                    let Some(x) = solve(a, b) else { continue };
                    let feasible = rows.iter().all(|(a, b)| {
                        let v: Q = (0..4).map(|c| &a[c] * &x[c]).sum();
                        v <= *b
                    });
                    if feasible && best.as_ref().is_none_or(|t| x[3] > *t) {
                        best = Some(x[3].clone());
                    }
                }
            }
        }
    }
    best.expect("the program has a vertex")
}

/// The rank of the normals and, below 3, a vector orthogonal to all.
fn rank(ns: &[Q3]) -> (usize, Option<Q3>) {
    let Some(a) = ns.iter().find(|n| !is_zero(n)) else {
        return (0, None);
    };
    let Some(b) = ns.iter().find(|n| !is_zero(&qcross(a, n))) else {
        // One direction: any vector orthogonal to it.
        let other = if a[0] != 0u32 || a[1] != 0u32 {
            [-&a[1], a[0].clone(), Q::from(0)]
        } else {
            [Q::from(1), Q::from(0), Q::from(0)]
        };
        return (1, Some(other));
    };
    let c = qcross(a, b);
    if ns.iter().any(|n| qdot(&c, n) != 0u32) {
        (3, None)
    } else {
        (2, Some(c))
    }
}

/// Every flat direction checked exactly: equal offsets `n̄·d = 1`, the
/// least such `d` (orthogonal to whatever the normals don't span); the
/// max–min direction in the box, agreeing, its minimum the brute-force
/// optimum, equal offsets impossible; a saddle only where the optimum is
/// not above zero, with `d = 0`. Also: [`max_min`] on every star against
/// the brute force, and every kind of outcome met.
#[test]
fn flat_directions_agree_on_random_stars() {
    let mut met = [0; 3];
    let mut kinds_met = std::collections::BTreeMap::new();
    for seed in varde_testing::seeds(8, 128) {
        let mut r = Rng::new(seed);
        for kind in KINDS {
            for _ in 0..4 {
                let s = star(&mut r, kind);
                let Some(dir) = flat(&s.normals) else {
                    // A zero normal: two edges along one line.
                    assert!(s.normals.contains(&DVec3::ZERO), "{s:?}");
                    continue;
                };
                let ns = scaled(&s.normals);
                let (d_lp, min_lp) = max_min(&ns);
                let best = brute_max_min(&ns);
                assert_eq!(min_lp, best, "{s:?}");
                assert!(d_lp.iter().all(|x| *x <= 1u32 && *x >= -1i32), "{s:?}");
                let outcome = match &dir {
                    FlatDir::EqualOffsets(d) => {
                        assert!(ns.iter().all(|n| qdot(n, d) == 1u32), "{s:?}");
                        if let (_, Some(v)) = rank(&ns) {
                            assert_eq!(qdot(&v, d), 0u32, "least: {s:?}");
                        }
                        assert!(agrees(&s.normals, d), "{s:?}");
                        assert!(best > 0u32, "{s:?}");
                        0
                    }
                    FlatDir::MaxMin { d, min } => {
                        assert_eq!(equal_offsets(&ns), None, "{s:?}");
                        assert!(d.iter().all(|x| *x <= 1u32 && *x >= -1i32), "{s:?}");
                        let least = ns.iter().map(|n| qdot(n, d)).min().unwrap();
                        assert_eq!((&least, min), (&best, &best), "{s:?}");
                        assert!(*min > 0u32 && agrees(&s.normals, d), "{s:?}");
                        1
                    }
                    FlatDir::Saddle => {
                        assert_eq!(equal_offsets(&ns), None, "{s:?}");
                        assert!(best <= 0u32, "{s:?}");
                        assert!(is_zero(&dir.d()));
                        assert!(!agrees(&s.normals, &dir.d()));
                        2
                    }
                };
                met[outcome] += 1;
                *kinds_met.entry(format!("{kind:?}{outcome}")).or_insert(0) += 1;
                // Order, repeats and positive scales don't matter.
                let mut shuffled: Vec<DVec3> = s.normals.iter().rev().copied().collect();
                shuffled.push(s.normals[0] * 4.0);
                assert_eq!(flat(&shuffled), Some(dir), "{s:?}");
            }
        }
    }
    assert!(met.iter().all(|&n| n > 3), "{met:?}");
    // Convex and knife corners of three faces solve equal offsets; random
    // links give saddles.
    for want in ["Convex0", "Reflex1", "Grid0", "Grid1", "Knife1", "Random2"] {
        assert!(kinds_met.contains_key(want), "{want}: {kinds_met:?}");
    }
}

/// Faces, edges and three faces always solve equal offsets (unless two
/// are opposite), a knife edge with offsets growing as its faces close,
/// past the predicates' `f64` input bound; an edge with opposite normals,
/// and a slab's two sides, are saddles; a pyramid's apex takes the
/// max–min direction.
#[test]
fn flat_directions_of_simple_elements() {
    let one = Q::from(1);
    let face = DVec3::new(2.0, -1.0, 0.5);
    let FlatDir::EqualOffsets(d) = flat(&[face]).unwrap() else {
        panic!("a face")
    };
    // `n̄/|n̄|²` with `n̄ = (1, −½, ¼)`.
    let nb = scaled_normal(face).unwrap();
    let len2 = qdot(&nb, &nb);
    assert_eq!(d, nb.clone().map(|x| x / &len2));
    for k in 10..60 {
        let eps = f64::from_bits((1023 - k) << 52);
        let edge = [DVec3::new(1.0, 0.0, eps), DVec3::new(-1.0, 0.0, eps)];
        let FlatDir::EqualOffsets(d) = flat(&edge).unwrap() else {
            panic!("a knife edge")
        };
        assert_eq!(d, [Q::from(0), Q::from(0), &one / q(eps)]);
    }
    assert_eq!(flat(&[DVec3::X, -DVec3::X]), Some(FlatDir::Saddle));
    assert_eq!(
        flat(&[DVec3::Z, -DVec3::Z, DVec3::X]),
        Some(FlatDir::Saddle)
    );
    let apex = [
        DVec3::new(2.0, 0.0, 1.0),
        DVec3::new(0.0, 1.0, 1.0),
        DVec3::new(-1.0, 0.0, 1.0),
        DVec3::new(0.0, -1.0, 1.0),
    ];
    let FlatDir::MaxMin { d, min } = flat(&apex).unwrap() else {
        panic!("an apex")
    };
    assert!(agrees(&apex, &d) && min > 0u32);
    assert_eq!(flat(&[]), None);
    assert_eq!(flat(&[DVec3::ZERO]), None);
    assert_eq!(flat(&[DVec3::new(f64::NAN, 0.0, 1.0)]), None);
}

/// The blend from a vertex to an edge or face of its star: `d(a)` within
/// `r_a`, the element's own direction beyond `2r_a`, between them still
/// agreeing with the faces at the point, exactly; with equal offsets at
/// the vertex a face's offset stays 1 along the blend (it moves as a
/// plane); at a saddle zero within `r_a` and a positive multiple of the
/// element's direction beyond.
#[test]
fn blends_keep_sign_agreement() {
    let mut blended = 0;
    for seed in varde_testing::seeds(8, 128) {
        let mut r = Rng::new(seed);
        for kind in KINDS {
            let s = star(&mut r, kind);
            let Some(at_a) = flat(&s.normals) else {
                continue;
            };
            let d_a = at_a.d();
            let a = exact(DVec3::new(r.int(4), r.int(4), r.int(4))).unwrap();
            let r_a = f64::from_bits((1023 - 3 - r.below(6)) << 52);
            let k = s.edges.len();
            let i = r.below(k as u64) as usize;
            // A point of edge `i`, or of face `i`, at a distance in
            // `[0, 3r_a]` in the max norm.
            let on_edge = r.below(2) == 0;
            let (along, faces) = if on_edge {
                (s.edges[i], s.edge_faces(i).to_vec())
            } else {
                (s.edges[i] + s.edges[(i + 1) % k], vec![s.normals[i]])
            };
            let Some(own) = flat(&faces) else { continue };
            let d_x = own.d();
            let along = exact(along).unwrap();
            let far = along.iter().map(abs).max().unwrap();
            if far == 0u32 {
                continue;
            }
            let t = q(r.range(0.0, 3.0)) * q(r_a) / &far;
            let x: Q3 = [0, 1, 2].map(|j| &a[j] + &t * &along[j]);
            let l = weight_flat(&a, &x, r_a);
            let dh = blend_flat(&d_a, &d_x, &a, &x, r_a);
            let dist = (0..3).map(|j| abs(&(&x[j] - &a[j]))).max().unwrap();
            if dist <= q(r_a) {
                assert_eq!((&l, &dh), (&Q::from(0), &d_a));
            } else if dist >= q(2.0 * r_a) {
                assert_eq!((&l, &dh), (&Q::from(1), &d_x));
            }
            match at_a {
                FlatDir::Saddle => {
                    // Zero, or `λ·d(x)`.
                    assert!(is_zero(&qcross(&dh, &d_x)), "{s:?}");
                    assert_eq!(is_zero(&dh), l == 0u32 || own == FlatDir::Saddle);
                }
                _ => {
                    let both = own != FlatDir::Saddle;
                    assert!(!both || agrees(&faces, &dh), "{s:?} {l}");
                    blended += usize::from(l > 0u32 && l < 1u32);
                }
            }
            if let (FlatDir::EqualOffsets(_), FlatDir::EqualOffsets(_)) = (&at_a, &own) {
                for n in &faces {
                    assert_eq!(qdot(&scaled_normal(*n).unwrap(), &dh), 1u32);
                }
            }
        }
    }
    assert!(blended > 5, "{blended}");
}

/// The layer-height row of a `B` plane through an `A` vertex moving along
/// its exact direction: a tie decided at order 1 by `−σ_A·(n·d)·(n·UP)`,
/// exactly, through the filter or the expansions (the rate's denominators
/// cleared), never the rationals; a knife edge's direction past `2³²`
/// included.
#[test]
fn rows_read_exact_directions() {
    let (mut decided, mut ties) = (0, 0);
    for seed in varde_testing::seeds(8, 128) {
        let mut r = Rng::new(seed);
        for kind in KINDS.repeat(3) {
            let s = star(&mut r, kind);
            let Some(dir) = flat(&s.normals) else {
                continue;
            };
            if dir == FlatDir::Saddle {
                continue;
            }
            let d = dir.d();
            // A plane through the vertex, often containing `d` (a tie at
            // order 1 too, which `T₂` then decides): `w × D`, `D` the
            // direction with its denominators cleared, where small.
            let Rate::Exact(cleared) = dir.rate() else {
                unreachable!("an exact direction")
            };
            let small = cleared.iter().all(|x| abs(x) < 1u64 << 20);
            let n = if small && r.below(2) == 0 {
                let big = cleared.each_ref().map(|x| f64::rounding_from(x, Nearest).0);
                r.ints(3).cross(DVec3::from_array(big))
            } else {
                r.ints(3)
            };
            if n == DVec3::ZERO || n.dot(UP) == 0.0 {
                continue;
            }
            let at = DVec3::ZERO;
            let sigma = if r.below(2) == 0 {
                Sign::Pos
            } else {
                Sign::Neg
            };
            let pred = PlaneHeight {
                normal: n,
                offset: 0.0,
                rates: [Rate::ZERO, Rate::ZERO, Rate::ZERO, Rate::ZERO],
                q: Moving::of_a(at, sigma, dir.rate()),
            };
            let sigma_q = Q::from(if sigma == Sign::Pos { 1 } else { -1 });
            let g1 = -sigma_q * qdot(&exact(n).unwrap(), &d) * q(n.dot(UP));
            let Signs { first, work } = exact::sign(&pred);
            if g1 != 0u32 {
                assert_eq!(first, Some((1, Ok(g1.cmp(&Q::from(0))))), "{s:?} {n}");
                decided += 1;
            } else {
                assert!(matches!(first, Some((k, Ok(_))) if k > 1), "{s:?} {n}");
                ties += 1;
            }
            assert_eq!(work.rationals, 0, "{s:?} {n}");
        }
    }
    assert!(decided > 20 && ties > 5, "{decided} {ties}");
    // A knife edge's equal offsets, `1/ε` up, past the input bound.
    let eps = f64::from_bits((1023 - 50) << 52);
    let dir = flat(&[DVec3::new(1.0, 0.0, eps), DVec3::new(-1.0, 0.0, eps)]).unwrap();
    let pred = PlaneHeight {
        normal: DVec3::Z,
        offset: 0.0,
        rates: [Rate::ZERO, Rate::ZERO, Rate::ZERO, Rate::ZERO],
        q: Moving::of_a(DVec3::ZERO, Sign::Pos, Rate::exact(dir.d())),
    };
    assert_eq!(exact::sign(&pred).first, Some((1, Ok(Ordering::Less))));
    // Read by a curved row: exact rationals exactly, a level's floats
    // enclosing the rate.
    let d = flat(&[DVec3::new(3.0, 1.0, 1.0), DVec3::new(-1.0, 2.0, 1.0)])
        .unwrap()
        .d();
    let m = Moving::of_a(DVec3::ZERO, Sign::Neg, Rate::exact(d.clone()));
    let want = d.map(|x| -x);
    let exactly = MovingAt::from_flat(&m, &Q::from(0));
    assert_eq!(exactly.rates[0], want);
    let near = MovingAt::from_flat(&m, &Bounded::exact(0.0));
    for (x, w) in near.rates[0].iter().zip(&want) {
        let [lo, hi] = x.enclose();
        assert!(q(lo) <= *w && *w <= q(hi) && hi - lo < 1e-14, "{x:?} {w}");
    }
}

/// Curved directions at a level, from the stars' normals (exact): a
/// direction returned agrees with every normal over its whole enclosure,
/// exactly; a saddle only where the exact max–min optimum is not above
/// zero; faces always certify; a convex corner's vertex direction too.
fn curved_directions<S: Exactly>(like: &S) {
    let mut met = [0; 3];
    for seed in varde_testing::seeds(8, 128) {
        let mut r = Rng::new(seed);
        for kind in KINDS {
            let s = star(&mut r, kind);
            if s.normals.contains(&DVec3::ZERO) {
                continue;
            }
            let at = |n: DVec3| lit3(n, like);
            // `n·d > 0` for the true `d`, exactly: `n·v` beyond `Σ|nᵢ|·errᵢ`.
            let check = |d: &[S; 3], normals: &[DVec3]| {
                for n in normals {
                    let n = exact(*n).unwrap();
                    let (v, e) = (d.each_ref().map(S::q_value), d.each_ref().map(S::q_err));
                    let slack: Q = (0..3).map(|i| abs(&n[i]) * &e[i]).sum();
                    assert!(qdot(&n, &v) > slack, "{s:?} {n:?} {d:?}");
                }
            };
            for n in &s.normals {
                let CurvedDir::Dir(d) = face(&at(*n)) else {
                    panic!("a face: {s:?}")
                };
                check(&d, &[*n]);
            }
            let k = s.edges.len();
            for i in 0..k {
                let [n1, n2] = s.edge_faces(i);
                match edge(&at(n1), &at(n2)) {
                    CurvedDir::Dir(d) => check(&d, &[n1, n2]),
                    CurvedDir::Escalate => {
                        let ns = scaled(&[n1, n2]);
                        assert!(qdot(&ns[0], &ns[ns.len() - 1]) < 0u32, "{s:?}");
                    }
                    CurvedDir::Saddle => panic!("an edge is never a saddle"),
                }
            }
            let star_at = corners(&s, like);
            let best = brute_max_min(&scaled(&s.normals));
            match vertex(&star_at) {
                CurvedDir::Dir(d) => {
                    check(&d, &s.normals);
                    met[0] += 1;
                }
                CurvedDir::Saddle => {
                    assert_eq!(best, 0u32, "{s:?}");
                    assert_eq!(flat(&s.normals), Some(FlatDir::Saddle), "{s:?}");
                    met[1] += 1;
                }
                CurvedDir::Escalate => {
                    assert!(kind != Kind::Convex, "{s:?}");
                    met[2] += 1;
                }
            }
            // The blend from the vertex to its edge 0, halfway.
            if let (CurvedDir::Dir(d_a), CurvedDir::Dir(d_x)) = (
                vertex(&star_at),
                edge(&at(s.edge_faces(0)[0]), &at(s.edge_faces(0)[1])),
            ) {
                let x = s.edges[0] * (1.5 / s.edges[0].length());
                let l = blend_weight(DVec3::ZERO, x, 1.0);
                assert!(l > 0.4 && l < 0.6, "{l}");
                let dh = blend(&d_a, &d_x, l);
                if agrees_at(&dh, &s.edge_faces(0).map(at)) {
                    check(&dh, &s.edge_faces(0));
                }
                assert_eq!(blend(&d_a, &d_x, 0.0), d_a);
                assert_eq!(blend(&d_a, &d_x, 1.0), d_x);
            }
        }
    }
    assert!(met[0] > 10 && met[1] > 3, "{met:?}");
}

/// A level's value and error as exact rationals.
trait Exactly: Curved {
    fn q_value(&self) -> Q;
    fn q_err(&self) -> Q;
}

impl Exactly for Bounded<f64> {
    fn q_value(&self) -> Q {
        q(self.v)
    }
    fn q_err(&self) -> Q {
        q(self.err())
    }
}

impl Exactly for Bounded<Dd> {
    fn q_value(&self) -> Q {
        q(self.v.hi()) + q(self.v.lo())
    }
    fn q_err(&self) -> Q {
        q(self.err())
    }
}

impl Exactly for Bounded<Big> {
    fn q_value(&self) -> Q {
        Q::try_from(self.v.as_float()).expect("finite")
    }
    fn q_err(&self) -> Q {
        q(self.err())
    }
}

#[test]
fn curved_directions_certify_at_every_level() {
    curved_directions(&Bounded::exact(0.0));
    curved_directions(&Bounded::exact(Dd::from(0.0)));
    let big = Bounded::exact(Big::from_f64(0.0, Width::ALL[0]));
    curved_directions(&big);
}

/// The 48 signed permutations of the axes (the cube's symmetries: 90°
/// rotations and mirrors), as `(π, s)` with `(P·v)ᵢ = sᵢ·v_{πᵢ}`.
fn signed_perms() -> Vec<([usize; 3], [f64; 3])> {
    let perms = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let mut out = Vec::new();
    for p in perms {
        for bits in 0..8 {
            let s = [0, 1, 2].map(|i| if bits >> i & 1 == 1 { -1.0 } else { 1.0 });
            out.push((p, s));
        }
    }
    out
}

/// Every flat direction is equivariant under the cube's symmetries: the
/// direction of the moved normals is the moved direction, the max–min
/// one the least of the optimal directions (so not the vertex the
/// simplex happens to reach), and still optimal.
#[test]
fn flat_directions_are_equivariant() {
    let perms = signed_perms();
    let mut maxmin = 0;
    for seed in varde_testing::seeds(4, 64) {
        let mut r = Rng::new(seed);
        for kind in KINDS.iter().map(Some).chain([None, None, None]) {
            // Stars, and sets of small integer normals of rank 1 to 3
            // (often several optimal directions).
            let s = match kind {
                Some(&kind) => star(&mut r, kind),
                None => normal_set(&mut r),
            };
            let Some(dir) = flat(&s.normals) else {
                continue;
            };
            if let FlatDir::MaxMin { d, min } = &dir {
                maxmin += 1;
                let ns = scaled(&s.normals);
                assert_eq!(*min, brute_max_min(&ns), "{s:?}");
                assert_eq!(*d, brute_least_optimal(&ns, min), "{s:?}");
            }
            for &(p, sg) in &perms {
                let mv = |v: DVec3| DVec3::from_array([0, 1, 2].map(|i| sg[i] * v[p[i]]));
                let mq = |v: &Q3| -> Q3 { [0, 1, 2].map(|i| q(sg[i]) * &v[p[i]]) };
                let moved: Vec<DVec3> = s.normals.iter().map(|&n| mv(n)).collect();
                let want = match &dir {
                    FlatDir::EqualOffsets(d) => FlatDir::EqualOffsets(mq(d)),
                    FlatDir::MaxMin { d, min } => FlatDir::MaxMin {
                        d: mq(d),
                        min: min.clone(),
                    },
                    FlatDir::Saddle => FlatDir::Saddle,
                };
                assert_eq!(flat(&moved), Some(want), "{s:?} {p:?} {sg:?}");
            }
        }
    }
    assert!(maxmin > 3, "{maxmin}");
}

/// Two to six small integer normals, in a plane through `z` or not, as a
/// star's normals (its edges unused).
fn normal_set(r: &mut Rng) -> Star {
    let k = 2 + r.below(5) as usize;
    let flat_z = r.below(2) == 0;
    let normals = (0..k)
        .map(|_| {
            let mut n = r.ints(2);
            if flat_z {
                n.z = 0.0;
            }
            n
        })
        .collect();
    Star {
        edges: Vec::new(),
        normals,
    }
}

/// The least optimal direction by brute force: over every set of one to
/// three independent constraints of the optimal face (`n·d ≥ min`,
/// `|dᵢ| ≤ 1`), the least point where they are tight, if feasible; the
/// least of those.
fn brute_least_optimal(ns: &[Q3], min: &Q) -> Q3 {
    let mut rows: Vec<(Q3, Q)> = ns.iter().map(|n| (n.clone(), min.clone())).collect();
    for i in 0..3 {
        for s in [1, -1] {
            let mut a = [0, 1, 2].map(|_| Q::from(0));
            a[i] = Q::from(s);
            rows.push((a, Q::from(-1)));
        }
    }
    let m = rows.len();
    let mut best: Option<(Q, Q3)> = None;
    let mut sets: Vec<Vec<usize>> = (0..m).map(|i| vec![i]).collect();
    for i in 0..m {
        for j in i + 1..m {
            sets.push(vec![i, j]);
            for k in j + 1..m {
                sets.push(vec![i, j, k]);
            }
        }
    }
    for set in sets {
        // `d = Σ cⱼ·aⱼ` with `aᵢ·d = bᵢ`: the Gram system.
        let g = set
            .iter()
            .map(|&i| set.iter().map(|&j| qdot(&rows[i].0, &rows[j].0)).collect())
            .collect();
        let b = set.iter().map(|&i| rows[i].1.clone()).collect();
        let Some(c) = solve(g, b) else { continue };
        let mut d = [0, 1, 2].map(|_| Q::from(0));
        for (cj, &j) in c.iter().zip(&set) {
            for k in 0..3 {
                d[k] += cj * &rows[j].0[k];
            }
        }
        if rows.iter().all(|(a, b)| qdot(a, &d) >= *b) {
            let len = qdot(&d, &d);
            if best.as_ref().is_none_or(|(l, _)| len < *l) {
                best = Some((len, d));
            }
        }
    }
    best.expect("the optimal face has a least point").1
}

/// A star's faces at its vertex at a level, with their corner angles.
fn corners<S: Curved>(s: &Star, like: &S) -> Vec<Corner<S>> {
    let k = s.edges.len();
    (0..k)
        .map(|i| Corner {
            normal: lit3(s.normals[i], like),
            approx: s.normals[i],
            angle: corner_angle(s.edges[i], s.edges[(i + 1) % k]),
        })
        .collect()
}

/// The directions of a star's vertex and its edge 0 at a level, if both
/// certify.
fn vertex_and_edge<S: Curved>(s: &Star, like: &S) -> Option<([S; 3], [S; 3])> {
    let star_at = corners(s, like);
    let [n1, n2] = s.edge_faces(0);
    match (vertex(&star_at), edge(&lit3(n1, like), &lit3(n2, like))) {
        (CurvedDir::Dir(a), CurvedDir::Dir(x)) => Some((a, x)),
        _ => None,
    }
}

/// The curved blend weight is one `f64` function of the record's point:
/// `0` up to `r_a` (exactly at it too), `1` from `2r_a` on, between them
/// increasing; so a blend taken at every level from one star's directions
/// is one field: the levels' enclosures of it overlap.
#[test]
fn curved_blend_is_one_field_at_every_level() {
    let a = DVec3::new(1.0, -2.0, 0.5);
    let r_a = 0.25;
    assert_eq!(blend_weight(a, a, r_a), 0.0);
    assert_eq!(blend_weight(a, a + DVec3::X * r_a, r_a), 0.0);
    assert_eq!(blend_weight(a, a - DVec3::Z * (2.0 * r_a), r_a), 1.0);
    assert_eq!(blend_weight(a, a + DVec3::Y * (1.5 * r_a), r_a), 0.5);
    assert_eq!(blend_weight(a, a + DVec3::Y, 0.0), 1.0);
    assert_eq!(blend_weight(a, a + DVec3::Y, f64::NAN), 1.0);
    // A blend radius that isn't finite and positive: no blend, flat or
    // curved.
    let (qa, qx) = (exact(a).unwrap(), exact(a + DVec3::Y).unwrap());
    for r in [0.0, -1.0, f64::INFINITY, f64::NAN] {
        assert_eq!(blend_weight(a, a + DVec3::Y, r), 1.0, "{r}");
        assert_eq!(weight_flat(&qa, &qx, r), 1u32, "{r}");
    }
    assert_eq!(blend_weight(a, DVec3::splat(f64::NAN), r_a), 0.0);
    assert_eq!(blend_weight(a, DVec3::splat(f64::INFINITY), r_a), 1.0);
    let mut blended = 0;
    for seed in varde_testing::seeds(8, 128) {
        let mut r = Rng::new(seed);
        for kind in KINDS {
            let s = star(&mut r, kind);
            if s.normals.contains(&DVec3::ZERO) {
                continue;
            }
            let x = s.edges[0] * (r.range(0.0, 3.0) * r_a / s.edges[0].length());
            let l = blend_weight(DVec3::ZERO, x, r_a);
            let f = Bounded::exact(0.0);
            let Some((fa, fx)) = vertex_and_edge(&s, &f) else {
                continue;
            };
            let dd = Bounded::exact(Dd::from(0.0));
            let (da, dx) = vertex_and_edge(&s, &dd).expect("certified at f64, so at Dd");
            let big = Bounded::exact(Big::from_f64(0.0, Width::ALL[0]));
            let (ba, bx) = vertex_and_edge(&s, &big).expect("certified at f64, so at Big");
            let encl = [
                blend(&fa, &fx, l).map(|c| c.enclose()),
                blend(&da, &dx, l).map(|c| c.enclose()),
                blend(&ba, &bx, l).map(|c| c.enclose()),
            ];
            for i in 0..3 {
                for (e, f) in encl.iter().flat_map(|e| encl.iter().map(move |f| (e, f))) {
                    assert!(e[i][0] <= f[i][1], "{s:?} {l} {encl:?}");
                }
            }
            blended += usize::from(l > 0.0 && l < 1.0);
        }
    }
    assert!(blended > 2, "{blended}");
}

/// Degenerate linear programs: sets of up to ten normals from the cube's
/// 26 directions `{−1, 0, 1}³` and their multiples (many through one
/// vertex of the program, many optimal directions), each against the
/// brute force: Bland's rule ends, the max–min value and the least
/// optimal direction are the brute force's, a saddle exactly where that
/// value is not above zero.
#[test]
fn degenerate_programs_match_brute_force() {
    let cube: Vec<DVec3> = (0..27)
        .map(|i| {
            DVec3::new(
                (i % 3) as f64 - 1.0,
                (i / 3 % 3) as f64 - 1.0,
                (i / 9) as f64 - 1.0,
            )
        })
        .filter(|v| *v != DVec3::ZERO)
        .collect();
    let mut met = [0; 3];
    for seed in varde_testing::seeds(16, 256) {
        let mut r = Rng::new(seed);
        let k = 1 + r.below(10) as usize;
        let normals: Vec<DVec3> = (0..k)
            .map(|_| cube[r.below(26) as usize] * (1 + r.below(3)) as f64)
            .collect();
        let dir = flat(&normals).expect("non-zero normals");
        let ns = scaled(&normals);
        let best = brute_max_min(&ns);
        let (d, min) = max_min(&ns);
        assert_eq!(min, best, "{normals:?}");
        assert_eq!(d, brute_least_optimal(&ns, &best), "{normals:?}");
        match dir {
            FlatDir::EqualOffsets(d) => {
                assert!(ns.iter().all(|n| qdot(n, &d) == 1u32), "{normals:?}");
                met[0] += 1;
            }
            FlatDir::MaxMin { d: m, min } => {
                assert_eq!((m, min), (d, best), "{normals:?}");
                met[1] += 1;
            }
            FlatDir::Saddle => {
                assert!(best <= 0u32, "{normals:?}");
                met[2] += 1;
            }
        }
    }
    assert!(met.iter().all(|&n| n > 0), "{met:?}");
}

/// A vertex whose angle-weighted normal sum lies exactly on one face's
/// plane (normals `(−4, 0, 3)`, `(3, 0, −4)`, `(−3, 4, 0)` weighted 2,
/// 3, 3: the sum is `⊥` the second), while directions agree: no level
/// certifies the sum either way, so it takes the max–min direction, the
/// same at every level.
#[test]
fn vertex_with_its_sum_on_a_face_plane() {
    fn at<S: Curved>(like: &S) -> CurvedDir<S> {
        let faces = [
            (DVec3::new(-4.0, 0.0, 3.0), 2.0),
            (DVec3::new(3.0, 0.0, -4.0), 3.0),
            (DVec3::new(-3.0, 4.0, 0.0), 3.0),
        ];
        let star: Vec<Corner<S>> = faces
            .iter()
            .map(|&(n, angle)| Corner {
                normal: lit3(n, like),
                approx: n,
                angle,
            })
            .collect();
        vertex(&star)
    }
    let CurvedDir::Dir(f) = at(&Bounded::exact(0.0)) else {
        panic!("f64")
    };
    let CurvedDir::Dir(dd) = at(&Bounded::exact(Dd::from(0.0))) else {
        panic!("Dd")
    };
    let CurvedDir::Dir(big) = at(&Bounded::exact(Big::from_f64(0.0, Width::ALL[1]))) else {
        panic!("Big")
    };
    for i in 0..3 {
        let e = [f[i].enclose(), dd[i].enclose(), big[i].enclose()];
        assert!(
            e.iter().all(|a| e.iter().all(|b| a[0] <= b[1])),
            "{f:?} {dd:?} {big:?}"
        );
    }
}
/// Normals are any positive multiples, huge or tiny: a face, an edge
/// and a vertex certify at every level with their normals scaled by
/// powers of two from the least subnormals' range to near the top float,
/// to the same directions.
#[test]
fn curved_directions_of_huge_and_tiny_normals() {
    fn check<S: Curved>(like: &S) {
        let ns = [
            DVec3::new(1.0, 2.0, -0.5),
            DVec3::new(0.5, 2.0, 1.0),
            DVec3::new(1.0, 0.0, 1.0),
        ];
        let dirs = |k: i32| {
            // `2^k` in two factors, each a float.
            let pow2 = |k: i32| f64::from_bits(((k + 1023) as u64) << 52);
            let s = pow2(k / 2) * pow2(k - k / 2);
            let at = |n: DVec3| lit3(n * s, like);
            let star: Vec<Corner<S>> = ns
                .iter()
                .map(|&n| Corner {
                    normal: at(n),
                    approx: n * s,
                    angle: 1.0,
                })
                .collect();
            [
                face(&at(ns[0])),
                edge(&at(ns[0]), &at(ns[1])),
                vertex(&star),
            ]
        };
        let base = dirs(0);
        for k in (-1070..1020).step_by(7) {
            for (got, want) in dirs(k).iter().zip(&base) {
                let (CurvedDir::Dir(got), CurvedDir::Dir(want)) = (got, want) else {
                    panic!("2^{k}: {got:?} {}", std::any::type_name::<S>())
                };
                for i in 0..3 {
                    let [a, b] = [got[i].enclose(), want[i].enclose()];
                    assert!(a[0] <= b[1] && b[0] <= a[1], "2^{k}: {got:?} {want:?}");
                }
            }
        }
    }
    check(&Bounded::exact(0.0));
    check(&Bounded::exact(Dd::from(0.0)));
    check(&Bounded::exact(Big::from_f64(0.0, Width::ALL[0])));
}
