//! Root isolation of systems with known roots, built exactly in rationals
//! and taken to each level with their rounding as the error: triangular
//! systems `gᵢ = pᵢ(xᵢ) + Σ_{r<i} c·p_r(x_r)·x_s` (each `p` a product of
//! linear factors), mixed by a regular integer matrix, whose roots are
//! exactly the grid of the factors' roots. Every root lies in exactly one
//! result; a [`NdRoot`] holds exactly one, simple, inside `near`, its
//! orientation the sign of the true Jacobian's determinant. Simple roots
//! isolate, on split faces and corners too; tangential roots are clusters
//! above the depth limit; roots on the domain's boundary are clusters
//! reaching it; reversing a variable mirrors the result.

use malachite::Rational as Q;

use super::*;
use crate::bezier::Poly;
use crate::bezier::tests::{Level, Rng, at_level, q};
use crate::num::bounded::Bounded;
use crate::num::dd::Dd;

const DEEP: Limits = Limits {
    max_depth: 60,
    max_splits: 400_000,
};

fn one() -> Poly<Q> {
    Poly::new([Q::from(1u32)]).expect("degree 0")
}

/// `Π (t − r)^m` in Bernstein form, exactly.
fn factors(roots: &[(Q, u32)]) -> Poly<Q> {
    let mut p = one();
    for (r, m) in roots {
        let f = Poly::new([-r.clone(), Q::from(1u32) - r]).expect("degree 1");
        for _ in 0..*m {
            p = p.mul(&f).expect("degree in range");
        }
    }
    p
}

/// `k·Π_a ps[a](x_a)`, `None` the constant 1: the outer product of the
/// coefficients.
fn outer(k: &Q, ps: &[Option<&Poly<Q>>]) -> NdTensor<Q> {
    let one = one();
    let ps: Vec<&Poly<Q>> = ps.iter().map(|p| p.unwrap_or(&one)).collect();
    let deg: Vec<usize> = ps.iter().map(|p| p.degree()).collect();
    let len: usize = deg.iter().map(|d| d + 1).product();
    let c = (0..len).map(|mut flat| {
        let mut v = k.clone();
        for a in (0..ps.len()).rev() {
            v *= &ps[a].coefs()[flat % (deg[a] + 1)];
            flat /= deg[a] + 1;
        }
        v
    });
    NdTensor::new(&deg, c).expect("degrees")
}

/// `d` factors, `p` in variable `k` for each `(k, p)`, else 1.
fn place<'a>(d: usize, pairs: &[(usize, &'a Poly<Q>)]) -> Vec<Option<&'a Poly<Q>>> {
    let mut v = vec![None; d];
    for (k, p) in pairs {
        v[*k] = Some(*p);
    }
    v
}

/// The determinant of the `d × d` rational matrix, exactly.
fn det(mut a: Vec<Q>, d: usize) -> Q {
    let mut r = Q::from(1u32);
    for col in 0..d {
        let Some(p) = (col..d).find(|&i| a[i * d + col] != 0u32) else {
            return Q::from(0u32);
        };
        if p != col {
            for j in 0..d {
                a.swap(p * d + j, col * d + j);
            }
            r = -r;
        }
        let piv = a[col * d + col].clone();
        r *= &piv;
        for i in col + 1..d {
            let f = &a[i * d + col] / &piv;
            for j in col..d {
                let t = &f * &a[col * d + j];
                a[i * d + j] -= t;
            }
        }
    }
    r
}

/// A system and its roots in `[0, 1]ᵈ`.
struct Case {
    forms: Vec<NdTensor<Q>>,
    roots: Vec<Vec<Q>>,
}

fn small(rng: &mut Rng, k: u64) -> Q {
    Q::from(rng.below(2 * k + 1) as i64 - k as i64)
}

/// The triangular system of the factors `axes[a]` (roots of `p_a`),
/// coupled and mixed at random (module docs). Its roots are the grid of
/// the factors' roots.
fn grid(rng: &mut Rng, axes: &[Vec<(Q, u32)>]) -> Case {
    let d = axes.len();
    let ps: Vec<Poly<Q>> = axes.iter().map(|r| factors(r)).collect();
    let x = Poly::new([Q::from(0u32), Q::from(1u32)]).expect("degree 1");
    let at = |a: usize, p| place(d, &[(a, p)]);
    let mut g: Vec<NdTensor<Q>> = Vec::new();
    for i in 0..d {
        let mut gi = outer(&Q::from(1u32), &at(i, &ps[i]));
        for (r, pr) in ps.iter().enumerate().take(i) {
            let s = rng.below(d as u64) as usize;
            let c = small(rng, 2);
            if s == r || c == 0u32 {
                continue;
            }
            let mut f = at(r, pr);
            f[s] = Some(&x);
            gi = gi.add(&outer(&c, &f)).expect("same variables");
        }
        g.push(gi);
    }
    let m = loop {
        let m: Vec<Q> = (0..d * d).map(|_| small(rng, 3)).collect();
        if det(m.clone(), d) != 0u32 {
            break m;
        }
    };
    let forms = (0..d)
        .map(|i| {
            let mut f = g[0].scale(&m[i * d]);
            for k in 1..d {
                f = f.add(&g[k].scale(&m[i * d + k])).expect("same variables");
            }
            f
        })
        .collect();
    let mut roots: Vec<Vec<Q>> = vec![vec![]];
    for a in axes {
        let inside: Vec<&Q> = a
            .iter()
            .map(|(r, _)| r)
            .filter(|r| **r >= 0u32 && **r <= 1u32)
            .collect();
        roots = roots
            .into_iter()
            .flat_map(|p| {
                inside.iter().map(move |r| {
                    let mut p = p.clone();
                    p.push((*r).clone());
                    p
                })
            })
            .collect();
    }
    Case { forms, roots }
}

fn level<S: Level>(f: &NdTensor<Q>) -> NdTensor<S> {
    NdTensor::new(f.degrees(), f.coefs().iter().map(at_level::<S>)).expect("degrees")
}

fn in_box(b: &NdBox, x: &[Q]) -> bool {
    b.axes
        .iter()
        .zip(x)
        .all(|(a, x)| a[0].rational() <= *x && *x <= a[1].rational())
}

/// The sign of the true Jacobian's determinant at `x`.
fn jacobian_sign(forms: &[NdTensor<Q>], x: &[Q]) -> Ordering {
    let d = forms.len();
    let j: Vec<Q> = forms
        .iter()
        .flat_map(|f| (0..d).map(move |a| f.derivative(a).eval(x).expect("d variables")))
        .collect();
    det(j, d).partial_cmp(&0u32).expect("ordered")
}

/// Checks `r` against the true system and its roots; the numbers of root
/// and cluster results.
fn check(what: &str, r: &NdRoots, forms: &[NdTensor<Q>], roots: &[Vec<Q>]) -> (usize, usize) {
    for w in r.found.windows(2) {
        let h = |f: &NdFound| NdBox::hull(f.boxes());
        assert_ne!(
            h(&w[0]).order(&h(&w[1])),
            Ordering::Greater,
            "{what}: order"
        );
    }
    for x in roots {
        let n = r
            .found
            .iter()
            .filter(|f| f.boxes().iter().any(|b| in_box(b, x)))
            .count();
        assert_eq!(n, 1, "{what}: root {x:?} in {n} results: {r:#?}");
    }
    let mut counts = (0, 0);
    for f in &r.found {
        let held: Vec<&Vec<Q>> = roots
            .iter()
            .filter(|x| f.boxes().iter().any(|b| in_box(b, x)))
            .collect();
        match f {
            NdFound::Root(root) => {
                counts.0 += 1;
                assert_eq!(held.len(), 1, "{what}: {root:?} holds {held:?}");
                let x = held[0];
                for (n, x) in root.near.iter().zip(x) {
                    assert!(q(n[0]) <= *x && *x <= q(n[1]), "{what}: near {root:?}");
                }
                assert_eq!(root.orient, jacobian_sign(forms, x), "{what}: {root:?}");
            }
            NdFound::Cluster(c) => {
                counts.1 += 1;
                assert_eq!(c.hull, NdBox::hull(&c.boxes), "{what}: {c:?}");
                assert_eq!(c.boundary, c.hull.on_boundary(), "{what}: {c:?}");
            }
        }
    }
    counts
}

fn run<S: Level>(case: &Case, targets: &[f64]) -> NdRoots {
    let forms: Vec<NdTensor<S>> = case.forms.iter().map(level::<S>).collect();
    isolate_nd(&forms, targets, DEEP).expect("within the cap")
}

fn qs(xs: &[f64]) -> Vec<(Q, u32)> {
    xs.iter().map(|&x| (q(x), 1)).collect()
}

fn third(k: u32) -> (Q, u32) {
    (Q::from(k) / Q::from(3u32), 1)
}

#[test]
fn simple_roots_isolate_in_two_variables() {
    for seed in varde_testing::seeds(1, 8) {
        let mut rng = Rng::new(seed);
        // Roots apart, on split points, at a split corner, and off the
        // dyadics; one factor's root outside the domain.
        let axes = [
            vec![(q(0.5), 1), third(1), (q(0.8125), 1), (q(1.25), 1)],
            vec![(q(0.25), 1), (q(0.5), 1), third(2)],
        ];
        let case = grid(&mut rng, &axes);
        for (name, r) in [
            ("f64", run::<Bounded<f64>>(&case, &[0.0; 2])),
            ("dd", run::<Bounded<Dd>>(&case, &[0.0; 2])),
        ] {
            let what = format!("seed {seed} {name}");
            let (roots, clusters) = check(&what, &r, &case.forms, &case.roots);
            assert_eq!((roots, clusters), (9, 0), "{what}: {r:#?}");
        }
    }
}

#[test]
fn simple_roots_isolate_exactly() {
    // On rationals split points are exact zeros (no sign either way).
    let mut rng = Rng::new(11);
    let case = grid(&mut rng, &[qs(&[0.5, 0.75]), vec![third(1), (q(0.5), 1)]]);
    let exact = isolate_nd(&case.forms, &[0.0; 2], DEEP).expect("within the cap");
    assert_eq!(check("rational", &exact, &case.forms, &case.roots), (4, 0));
}

#[test]
fn simple_roots_isolate_in_three_and_four_variables() {
    for seed in varde_testing::seeds(1, 4) {
        let mut rng = Rng::new(seed);
        let axes3 = [qs(&[0.5, 0.3]), vec![third(1)], qs(&[0.75, 0.125])];
        let case = grid(&mut rng, &axes3);
        let r = run::<Bounded<f64>>(&case, &[0.0; 3]);
        let what = format!("seed {seed} 3D");
        assert_eq!(check(&what, &r, &case.forms, &case.roots), (4, 0), "{what}");
        let axes4 = [qs(&[0.5]), vec![third(2)], qs(&[0.25, 0.6]), qs(&[0.5])];
        let case = grid(&mut rng, &axes4);
        let r = run::<Bounded<f64>>(&case, &[0.0; 4]);
        let what = format!("seed {seed} 4D");
        assert_eq!(check(&what, &r, &case.forms, &case.roots), (2, 0), "{what}");
    }
}

#[test]
fn narrowed_roots_are_tight() {
    let mut rng = Rng::new(3);
    let case = grid(&mut rng, &[qs(&[0.5, 0.2]), vec![third(1), (q(0.75), 1)]]);
    let r = run::<Bounded<f64>>(&case, &[0.0; 2]);
    for f in &r.found {
        let NdFound::Root(root) = f else {
            panic!("a cluster: {f:?}")
        };
        for n in &root.near {
            assert!(n[1] - n[0] <= 1e-12, "{root:?}");
        }
    }
}

/// The curves `x₁ = b + (x₀ − a)²` and `x₁ = b` touch at `(a, b)`, a
/// double root, mixed at random; with `extra`, two more variables with
/// simple roots.
fn tangent(rng: &mut Rng, a: f64, b: f64, extra: bool) -> Case {
    let d = if extra { 4 } else { 2 };
    let sq = factors(&[(q(a), 2)]);
    let lin = factors(&[(q(b), 1)]);
    let g1 = outer(&Q::from(1u32), &place(d, &[(1, &lin)]));
    let g0 = g1
        .sub(&outer(&Q::from(1u32), &place(d, &[(0, &sq)])))
        .expect("same variables");
    let mut g = vec![g0, g1];
    let (p2, p3) = (factors(&qs(&[0.375])), factors(&[third(2)]));
    if extra {
        g.push(outer(&Q::from(1u32), &place(d, &[(2, &p2)])));
        g.push(outer(&Q::from(1u32), &place(d, &[(3, &p3)])));
    }
    let m = loop {
        let m: Vec<Q> = (0..d * d).map(|_| small(rng, 3)).collect();
        if det(m.clone(), d) != 0u32 {
            break m;
        }
    };
    let forms = (0..d)
        .map(|i| {
            let mut f = g[0].scale(&m[i * d]);
            for k in 1..d {
                f = f.add(&g[k].scale(&m[i * d + k])).expect("same variables");
            }
            f
        })
        .collect();
    let mut root = vec![q(a), q(b)];
    if extra {
        root.extend([q(0.375), Q::from(2u32) / Q::from(3u32)]);
    }
    Case {
        forms,
        roots: vec![root],
    }
}

#[test]
fn a_tangency_is_one_cluster_above_the_depth_limit() {
    for seed in varde_testing::seeds(1, 4) {
        let mut rng = Rng::new(seed);
        for (a, b) in [(0.3, 0.6), (0.5, 0.5)] {
            let case = tangent(&mut rng, a, b, false);
            let r = run::<Bounded<f64>>(&case, &[1e-6; 2]);
            let what = format!("seed {seed} at ({a}, {b})");
            assert_eq!(check(&what, &r, &case.forms, &case.roots), (0, 1), "{what}");
            let NdFound::Cluster(c) = &r.found[0] else {
                unreachable!()
            };
            assert!(!c.at_depth && c.simple.is_none(), "{what}: {c:?}");
            assert!(r.work.tests < 20_000, "{what}: {:?}", r.work);
        }
        let case = tangent(&mut rng, 0.3, 0.6, true);
        let r = run::<Bounded<f64>>(&case, &[1e-4; 4]);
        let what = format!("seed {seed} in four variables");
        assert_eq!(check(&what, &r, &case.forms, &case.roots), (0, 1), "{what}");
        let NdFound::Cluster(c) = &r.found[0] else {
            unreachable!()
        };
        assert!(!c.at_depth, "{what}: {c:?}");
    }
}

#[test]
fn roots_on_the_boundary_are_clusters_reaching_it() {
    for seed in varde_testing::seeds(1, 4) {
        let mut rng = Rng::new(seed);
        let case = grid(&mut rng, &[qs(&[0.0, 0.4]), qs(&[1.0, 0.5])]);
        let r = run::<Bounded<f64>>(&case, &[0.0; 2]);
        let what = format!("seed {seed}");
        let (roots, clusters) = check(&what, &r, &case.forms, &case.roots);
        assert_eq!((roots, clusters), (1, 3), "{what}: {r:#?}");
        for f in &r.found {
            if let NdFound::Cluster(c) = f {
                assert!(
                    c.boundary && c.simple.is_some() && !c.at_depth,
                    "{what}: {c:?}"
                );
            }
        }
    }
}

fn mirror(d: Dyadic) -> Dyadic {
    Dyadic::new((1u64 << d.exp()) - d.num(), d.exp()).expect("in [0, 1]")
}

fn mirror_box(b: &NdBox, axis: usize) -> NdBox {
    let mut b = b.clone();
    let [lo, hi] = b.axes[axis];
    b.axes[axis] = [mirror(hi), mirror(lo)];
    b
}

/// The result with `axis` reversed, near bounds dropped.
fn mirrored(r: &NdRoots, axis: usize) -> Vec<NdFound> {
    let mut out: Vec<NdFound> = r
        .found
        .iter()
        .map(|f| {
            let flip = |bs: &[NdBox]| {
                let mut bs: Vec<NdBox> = bs.iter().map(|b| mirror_box(b, axis)).collect();
                bs.sort_by(NdBox::order);
                bs
            };
            match f {
                NdFound::Root(x) => NdFound::Root(NdRoot {
                    boxes: flip(&x.boxes),
                    near: Axes::new(),
                    orient: x.orient.reverse(),
                }),
                NdFound::Cluster(c) => NdFound::Cluster(NdCluster {
                    boxes: flip(&c.boxes),
                    hull: mirror_box(&c.hull, axis),
                    ..c.clone()
                }),
            }
        })
        .collect();
    out.sort_by(|a, b| NdBox::hull(a.boxes()).order(&NdBox::hull(b.boxes())));
    out
}

fn without_near(r: &NdRoots) -> Vec<NdFound> {
    r.found
        .iter()
        .map(|f| match f {
            NdFound::Root(x) => NdFound::Root(NdRoot {
                near: Axes::new(),
                ..x.clone()
            }),
            c => c.clone(),
        })
        .collect()
}

#[test]
fn reversing_a_variable_mirrors_the_result() {
    for seed in varde_testing::seeds(1, 6) {
        let mut rng = Rng::new(seed);
        let mut cases = vec![
            grid(
                &mut rng,
                &[
                    qs(&[0.5, 0.0, 0.7]),
                    vec![third(1), (q(0.5), 1), (q(1.0), 1)],
                ],
            ),
            tangent(&mut rng, 0.3, 0.5, false),
        ];
        cases.push(grid(&mut rng, &[qs(&[0.5]), qs(&[0.25]), vec![third(1)]]));
        for (n, case) in cases.iter().enumerate() {
            let d = case.forms.len();
            let targets = vec![1e-6; d];
            let r = run::<Bounded<f64>>(case, &targets);
            for axis in 0..d {
                let rev = Case {
                    forms: case.forms.iter().map(|f| f.reversed(axis)).collect(),
                    roots: case
                        .roots
                        .iter()
                        .map(|x| {
                            let mut x = x.clone();
                            x[axis] = Q::from(1u32) - &x[axis];
                            x
                        })
                        .collect(),
                };
                let s = run::<Bounded<f64>>(&rev, &targets);
                let what = format!("seed {seed} case {n} axis {axis}");
                check(&what, &s, &rev.forms, &rev.roots);
                assert_eq!(mirrored(&r, axis), without_near(&s), "{what}");
            }
        }
    }
}

#[test]
fn the_work_cap_stops_it_whichever_side_the_halvings_are_on() {
    let mut rng = Rng::new(5);
    let case = grid(&mut rng, &[qs(&[0.3, 0.6]), qs(&[0.2, 0.7])]);
    let capped = Limits {
        max_depth: 60,
        max_splits: 6,
    };
    let forms: Vec<NdTensor<Bounded<f64>>> = case.forms.iter().map(level).collect();
    let rev: Vec<_> = forms.iter().map(|f| f.reversed(0)).collect();
    let a = isolate_nd(&forms, &[0.0; 2], capped).expect_err("past the cap");
    let b = isolate_nd(&rev, &[0.0; 2], capped).expect_err("past the cap");
    assert_eq!(a, b);
    let NdFail::OutOfWork(w) = a else {
        panic!("{a:?}")
    };
    assert_eq!(w.work.splits, 6);
}

#[test]
fn shapes_other_than_square_systems_are_refused() {
    let f = NdTensor::new(&[1, 1], [0.0, 1.0, -1.0, 2.0].map(Bounded::exact)).expect("degrees");
    let g = NdTensor::new(&[1], [0.0, 1.0].map(Bounded::exact)).expect("degrees");
    let fail = |forms: &[NdTensor<Bounded<f64>>], t: &[f64]| isolate_nd(forms, t, DEEP).err();
    assert_eq!(fail(std::slice::from_ref(&f), &[0.0]), Some(NdFail::Shape));
    assert_eq!(fail(&[f.clone(), g], &[0.0; 2]), Some(NdFail::Shape));
    assert_eq!(fail(&[f.clone(), f.clone()], &[0.0]), Some(NdFail::Shape));
    assert_eq!(
        fail(&[f.clone(), f.clone()], &[0.0, f64::NAN]),
        Some(NdFail::Shape)
    );
    assert_eq!(fail(&[], &[]), Some(NdFail::Shape));
}

#[test]
fn a_system_identically_zero_is_one_cluster() {
    let z = NdTensor::new(&[2, 1], [0.0; 6].map(Bounded::exact)).expect("degrees");
    let r = isolate_nd(&[z.clone(), z], &[0.0; 2], DEEP).expect("no splits");
    assert_eq!(r.found.len(), 1);
    let NdFound::Cluster(c) = &r.found[0] else {
        panic!("{r:?}")
    };
    assert_eq!(c.boxes.len(), 1);
    assert!(c.within_target && c.boundary && !c.at_depth && c.simple.is_none());
}
