//! The tree against brute force in 2D and 3D, over boxes of sizes on a
//! log scale, flat, repeated (equal codes), empty, unbounded and touching;
//! its shape (every leaf reached once, every internal box the union of its
//! children's, the depth); the Morton codes; outward inflation; the
//! charges, counted before collected; and the same pairs, work and
//! profiles on 1 and 8 threads and in either order.

use malachite::Rational;
use varde_testkit::rng::Rng;

use super::*;
use crate::infra::budget::Budget;
#[cfg(not(target_arch = "wasm32"))]
use crate::infra::budget::{MAX_WORK, assert_early_stop_changes_nothing};
use crate::infra::error::Cause;

const PASS: Pass = Pass {
    stage: "broad",
    rung: 0,
};

/// `n` random boxes in `D` dimensions in a cube of side 100: sizes on a
/// log scale, a fifth flat on an axis, some repeated (so their codes tie),
/// a few empty or unbounded, a few on a coarse grid (so sides touch).
fn random_boxes<const D: usize>(rng: &mut Rng, n: usize) -> Vec<Aabb<D>> {
    let mut boxes: Vec<Aabb<D>> = Vec::with_capacity(n);
    for _ in 0..n {
        let pick = rng.unit();
        let b = if pick < 0.08 && !boxes.is_empty() {
            boxes[(rng.next_u64() % boxes.len() as u64) as usize]
        } else if pick < 0.12 {
            let mut b = one_box(rng);
            b.lo[0] = b.hi[0] + 1.0;
            b
        } else if pick < 0.14 {
            let mut b = one_box(rng);
            b.lo[D - 1] = f64::NAN;
            b
        } else if pick < 0.17 {
            let mut b = one_box(rng);
            if rng.unit() < 0.5 {
                b.lo[0] = f64::NEG_INFINITY;
            } else {
                b.hi[D - 1] = f64::INFINITY;
            }
            b
        } else if pick < 0.27 {
            let lo: [f64; D] = std::array::from_fn(|_| (rng.next_u64() % 8) as f64 * 12.5 - 50.0);
            Aabb {
                lo,
                hi: lo.map(|x| x + 12.5),
            }
        } else {
            one_box(rng)
        };
        boxes.push(b);
    }
    boxes
}

fn one_box<const D: usize>(rng: &mut Rng) -> Aabb<D> {
    let lo: [f64; D] = std::array::from_fn(|_| rng.range(-50.0, 50.0));
    let flat = rng.unit() < 0.2;
    let hi = std::array::from_fn(|k| {
        if flat && k == 0 {
            lo[k]
        } else {
            lo[k] + rng.log_range(1e-3, 10.0)
        }
    });
    Aabb { lo, hi }
}

/// The tree over `boxes`, built in a pass of an ample budget.
fn tree<const D: usize>(boxes: &[Aabb<D>]) -> Bvh<D> {
    let mut work = Work::new(&Budget::DEFAULT);
    work.pass(PASS, &[()], |_, ledger| {
        Bvh::build(boxes, ledger).map_err(Stop::<Infallible>::from)
    })
    .expect("built")
    .swap_remove(0)
}

/// `bvh.hits(query)` in a pass of an ample budget.
fn hits<const D: usize>(bvh: &Bvh<D>, query: &Aabb<D>) -> Vec<u32> {
    let mut work = Work::new(&Budget::DEFAULT);
    work.pass(PASS, &[()], |_, ledger| {
        bvh.hits(query, ledger).map_err(Stop::<Infallible>::from)
    })
    .expect("queried")
    .swap_remove(0)
}

fn brute_pairs<const D: usize>(queries: &[Aabb<D>], boxes: &[Aabb<D>]) -> Vec<[u32; 2]> {
    let mut out = Vec::new();
    for (q, a) in queries.iter().enumerate() {
        for (l, b) in boxes.iter().enumerate() {
            if a.meets(b) {
                out.push([q as u32, l as u32]);
            }
        }
    }
    out
}

fn matches_brute_force<const D: usize>(seed: u64) {
    let mut rng = Rng::new(seed);
    for n in [0, 1, 2, 3, 5, 17, 130, 600] {
        let boxes = random_boxes::<D>(&mut rng, n);
        let queries = random_boxes::<D>(&mut rng, 40);
        let bvh = tree(&boxes);
        assert_eq!(bvh.len(), n);
        assert_eq!(bvh.is_empty(), n == 0);
        let mut work = Work::new(&Budget::DEFAULT);
        let pairs = bvh.pairs(&queries, &mut work, PASS).expect("in budget");
        assert_eq!(pairs, brute_pairs(&queries, &boxes), "n = {n}");
        for (q, query) in queries.iter().enumerate() {
            let want: Vec<u32> = pairs
                .iter()
                .filter(|p| p[0] == q as u32)
                .map(|p| p[1])
                .collect();
            assert_eq!(hits(&bvh, query), want);
        }
        let own = bvh.self_pairs(&mut work, PASS).expect("in budget");
        let want: Vec<[u32; 2]> = brute_pairs(&boxes, &boxes)
            .into_iter()
            .filter(|&[i, j]| i < j)
            .collect();
        assert_eq!(own, want, "n = {n}");
    }
}

#[test]
fn pairs_match_brute_force_in_3d() {
    for seed in varde_testing::seeds(4, 32) {
        matches_brute_force::<3>(seed);
    }
}

#[test]
fn pairs_match_brute_force_in_2d() {
    for seed in varde_testing::seeds(4, 32) {
        matches_brute_force::<2>(100 + seed);
    }
}

#[test]
fn points_and_touching_sides_meet() {
    // Unit squares side by side: neighbours share a side, diagonal ones a
    // corner; a point query on a shared corner meets all four around it.
    let boxes: Vec<Aabb<2>> = (0..16)
        .map(|i| {
            let (x, y) = (f64::from(i % 4), f64::from(i / 4));
            Aabb {
                lo: [x, y],
                hi: [x + 1.0, y + 1.0],
            }
        })
        .collect();
    let bvh = tree(&boxes);
    assert_eq!(hits(&bvh, &Aabb::point([1.0, 1.0])), [0, 1, 4, 5]);
    assert_eq!(hits(&bvh, &Aabb::point([0.5, 3.5])), [12]);
    assert_eq!(hits(&bvh, &Aabb::point([4.5, 0.5])), Vec::<u32>::new());
    let mut work = Work::new(&Budget::DEFAULT);
    let own = bvh.self_pairs(&mut work, PASS).expect("in budget");
    // Each square meets its up to 8 neighbours: 9·8 interior-corner
    // incidences give 42 pairs on a 4 × 4 grid.
    assert_eq!(own.len(), 42);
    assert!(own.contains(&[0, 5]) && own.contains(&[5, 10]) && !own.contains(&[0, 2]));
    // Moved off the side `x = 2` by the least step, a query no longer
    // meets the squares left of it.
    let apart = Aabb {
        lo: [2.0f64.next_up(), 0.0],
        hi: [3.0, 1.0],
    };
    assert_eq!(hits(&bvh, &apart), [2, 3, 6, 7]);
    assert_eq!(
        hits(
            &bvh,
            &Aabb {
                lo: [2.0, 0.0],
                hi: [3.0, 1.0]
            }
        ),
        [1, 2, 3, 5, 6, 7]
    );
}

#[test]
fn one_leaf_empty_and_unbounded_boxes() {
    // A tree of one leaf is queried (it has no internal node).
    let one = [Aabb {
        lo: [0.0; 3],
        hi: [1.0; 3],
    }];
    let bvh = tree(&one);
    assert_eq!(hits(&bvh, &Aabb::point([1.0, 1.0, 1.0])), [0]);
    assert_eq!(bvh.bounds(), Some(one[0]));
    // Empty boxes meet nothing, as queries or as leaves, not even an
    // unbounded box.
    let all = Aabb {
        lo: [f64::NEG_INFINITY; 3],
        hi: [f64::INFINITY; 3],
    };
    let empty = Aabb {
        lo: [2.0, 0.0, 0.0],
        hi: [1.0, 1.0, 1.0],
    };
    let nan = Aabb {
        lo: [0.0, f64::NAN, 0.0],
        hi: [1.0; 3],
    };
    assert!(empty.is_empty() && nan.is_empty() && !all.is_empty());
    assert!(!all.meets(&empty) && !nan.meets(&all));
    let boxes = [empty, one[0], nan, all, one[0]];
    let bvh = tree(&boxes);
    assert_eq!(bvh.len(), 5);
    assert_eq!(bvh.bounds(), Some(all));
    assert_eq!(hits(&bvh, &all), [1, 3, 4]);
    assert_eq!(hits(&bvh, &empty), Vec::<u32>::new());
    let mut work = Work::new(&Budget::DEFAULT);
    let own = bvh.self_pairs(&mut work, PASS).expect("in budget");
    assert_eq!(own, [[1, 3], [1, 4], [3, 4]]);
    // A tree of empty boxes only has none to give.
    let bvh = tree(&[empty, nan]);
    assert_eq!(bvh.bounds(), None);
    assert_eq!(hits(&bvh, &all), Vec::<u32>::new());
    assert_eq!(tree::<3>(&[]).bounds(), None);
}

/// Every leaf reached once from the root, every internal box the union
/// of its children's, and the depth within the 62 bits of code and index.
fn check_shape<const D: usize>(bvh: &Bvh<D>) {
    let n = bvh.leaves.len();
    if n < 2 {
        return;
    }
    let mut seen = vec![0u32; n];
    let mut stack = vec![(ROOT, 0usize)];
    let mut deepest = 0;
    while let Some((node, depth)) = stack.pop() {
        deepest = deepest.max(depth);
        if node % 2 == 0 {
            seen[(node / 2) as usize] += 1;
            continue;
        }
        let [a, b] = bvh.children[(node / 2) as usize];
        assert_eq!(
            bvh.nodes[node as usize],
            bvh.nodes[a as usize].union(&bvh.nodes[b as usize])
        );
        stack.push((a, depth + 1));
        stack.push((b, depth + 1));
    }
    assert!(seen.iter().all(|&s| s == 1), "{seen:?}");
    assert!(deepest <= 62, "depth {deepest}");
}

#[test]
fn the_tree_covers_every_leaf_once() {
    let mut rng = Rng::new(7);
    for n in [2, 3, 4, 9, 64, 257, 2000] {
        check_shape(&tree(&random_boxes::<3>(&mut rng, n)));
        check_shape(&tree(&random_boxes::<2>(&mut rng, n)));
    }
    // All codes equal: the positions alone split the leaves.
    let same = vec![
        Aabb {
            lo: [0.0; 3],
            hi: [1.0; 3],
        };
        300
    ];
    let bvh = tree(&same);
    check_shape(&bvh);
    let mut work = Work::new(&Budget::DEFAULT);
    assert_eq!(
        bvh.self_pairs(&mut work, PASS).expect("in budget").len(),
        300 * 299 / 2
    );
}

#[test]
fn morton_codes() {
    let (lo, hi) = ([0.0; 3], [1.0; 3]);
    assert_eq!(morton(&[0.0, 0.0, 0.0], &lo, &hi), 0);
    // The top cell on every axis: 30 bits set.
    assert_eq!(morton(&[1.0, 1.0, 1.0], &lo, &hi), (1 << 30) - 1);
    // x is the highest bit of each triple.
    assert_eq!(morton(&[0.5, 0.0, 0.0], &lo, &hi), 1 << 29);
    assert_eq!(morton(&[0.0, 0.5, 0.0], &lo, &hi), 1 << 28);
    assert_eq!(morton(&[0.0, 0.0, 0.5], &lo, &hi), 1 << 27);
    assert_eq!(morton(&[0.0, 0.0, 1.0 / 1024.0], &lo, &hi), 1);
    // Outside the box, NaN, infinite or on an axis of no extent: clamped.
    let inf = f64::INFINITY;
    assert_eq!(morton(&[-3.0, f64::NAN, -inf], &lo, &hi), 0);
    assert_eq!(morton(&[7.0, inf, 1.0], &lo, &hi), (1 << 30) - 1);
    assert_eq!(morton(&[0.7, 0.7, 0.7], &[0.7; 3], &[0.7; 3]), 0);
    // 2D: 15 bits an axis, x highest.
    let (lo, hi) = ([0.0; 2], [1.0; 2]);
    assert_eq!(morton(&[1.0, 1.0], &lo, &hi), (1 << 30) - 1);
    assert_eq!(morton(&[0.5, 0.0], &lo, &hi), 1 << 29);
    assert_eq!(morton(&[0.0, 0.5], &lo, &hi), 1 << 28);
    assert_eq!(morton(&[1.0 / 32768.0, 0.0], &lo, &hi), 2);
    assert_eq!(spread2(0x7FFF), 0x1555_5555);
    assert_eq!(spread3(0x3FF), 0x0924_9249);
}

#[test]
fn inflation_rounds_outward() {
    let mut rng = Rng::new(3);
    for _ in 0..2000 {
        let b: Aabb<3> = one_box(&mut rng);
        let margin = rng.log_range(1e-12, 1.0);
        let wide = b.inflated(margin);
        let m = Rational::try_from(margin).expect("finite");
        for k in 0..3 {
            let lo = Rational::try_from(b.lo[k]).expect("finite");
            let hi = Rational::try_from(b.hi[k]).expect("finite");
            assert!(Rational::try_from(wide.lo[k]).expect("finite") <= lo - &m);
            assert!(Rational::try_from(wide.hi[k]).expect("finite") >= hi + &m);
        }
    }
    let b = Aabb {
        lo: [f64::NEG_INFINITY, 0.0],
        hi: [1.0, f64::INFINITY],
    };
    assert_eq!(b.inflated(0.0), b);
    assert_eq!(b.inflated(1.0).lo[0], f64::NEG_INFINITY);
    assert!(b.inflated(f64::NAN).is_empty());
}

#[test]
fn hull_boxes_convert() {
    let hull = HullBox {
        lo: [0.0, -1.0, f64::NEG_INFINITY],
        hi: [1.0, 2.0, 3.0],
    };
    let b = Aabb::from(hull);
    assert_eq!((b.lo, b.hi), (hull.lo, hull.hi));
}

#[test]
fn queries_are_charged_before_collecting() {
    // 200 boxes all meeting each other: the query's walk tests every
    // node's two children, 2·199 tests, then collects 200 hits.
    let boxes = vec![
        Aabb {
            lo: [0.0; 3],
            hi: [1.0; 3],
        };
        200
    ];
    let bvh = tree(&boxes);
    let query = boxes[0];
    let mut work = Work::new(&Budget::DEFAULT);
    let found = work
        .pass(PASS, &[()], |_, ledger| {
            let mut meter = Meter::new(ledger);
            let counted = bvh.count(&query, |_| true, &mut meter)?;
            meter.finish()?;
            Ok::<_, Stop<Infallible>>(counted)
        })
        .expect("counted")
        .swap_remove(0);
    assert_eq!(found, (200, 2 * 199));
    let used = work.used();
    let tests = 2 * 199 * TEST_PARTS;
    assert_eq!(used, tests.div_ceil(UNIT_PARTS));
    let mut work = Work::new(&Budget::DEFAULT);
    let mut pass_hits = || {
        work.pass(PASS, &[()], |_, ledger| {
            bvh.hits(&query, ledger).map_err(Stop::<Infallible>::from)
        })
    };
    assert_eq!(pass_hits().expect("in budget")[0].len(), 200);
    let charged = (2 * tests + 200 * HIT_PARTS).div_ceil(UNIT_PARTS);
    assert_eq!(work.used(), charged);
    // A budget the count fits in but not the collection runs out before
    // anything is collected.
    let mut work = Work::new(&Budget::new(charged - 1));
    let error = work
        .pass(PASS, &[()], |_, ledger| {
            bvh.hits(&query, ledger).map_err(Stop::<Infallible>::from)
        })
        .expect_err("out of budget");
    assert!(matches!(
        error,
        Halt::OutOfBudget(KernelError::OutOfBudget {
            cause: Cause::Work,
            ..
        })
    ));
    // Building charges its leaves first.
    let mut work = Work::new(&Budget::new(200 * BUILD_PARTS / UNIT_PARTS - 1));
    let built = work.pass(PASS, &[()], |_, ledger| {
        Bvh::build(&boxes, ledger)
            .map(|_| ())
            .map_err(Stop::<Infallible>::from)
    });
    assert!(built.is_err());
}

#[test]
fn pairs_count_in_one_pass_and_collect_in_the_next() {
    // Crowded boxes: every two meet. The count pass fits in a budget the
    // collection doesn't, which then runs out in the second pass.
    let boxes = vec![
        Aabb {
            lo: [0.0; 2],
            hi: [1.0; 2],
        };
        500
    ];
    let bvh = tree(&boxes);
    let mut work = Work::new(&Budget::DEFAULT);
    let all = bvh.self_pairs(&mut work, PASS).expect("in budget");
    assert_eq!(all.len(), 500 * 499 / 2);
    let used = work.used();
    let mut work = Work::new(&Budget::new(used / 2 + 1));
    match bvh.self_pairs(&mut work, PASS) {
        Err(KernelError::OutOfBudget { cause, profile }) => {
            assert_eq!(cause, Cause::Work);
            assert_eq!(profile.stopped.at.pass, 1, "the collecting pass");
        }
        other => panic!("{other:?}"),
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn same_pairs_and_work_on_any_threads() {
    let mut rng = Rng::new(11);
    let boxes = random_boxes::<3>(&mut rng, 3000);
    let queries = random_boxes::<3>(&mut rng, 1500);
    let flat = random_boxes::<2>(&mut rng, 3000);
    // The tree, its pairs with the queries and its own pairs, then the 2D
    // tree's own pairs, on one budget: the outcomes, the work used after
    // each step and the tree's nodes.
    let run = |budget: u64| {
        let mut work = Work::new(&Budget::new(budget));
        let mut used = Vec::new();
        let built = work.pass(PASS, &[()], |_, ledger| {
            Bvh::build(&boxes, ledger).map_err(Stop::<Infallible>::from)
        });
        used.push(work.used());
        let Ok(mut built) = built else {
            return format!("{built:?} {used:?}");
        };
        let bvh = built.swap_remove(0);
        let pairs = bvh.pairs(&queries, &mut work, PASS);
        used.push(work.used());
        let own = bvh.self_pairs(&mut work, PASS);
        used.push(work.used());
        let plane = work
            .pass(PASS, &[()], |_, ledger| {
                Bvh::build(&flat, ledger).map_err(Stop::<Infallible>::from)
            })
            .map(|mut b| b.swap_remove(0).self_pairs(&mut work, PASS));
        used.push(work.used());
        format!("{pairs:?} {own:?} {plane:?} {used:?} {:?}", bvh.nodes)
    };
    let full = assert_early_stop_changes_nothing(|| run(MAX_WORK));
    assert!(!full.contains("OutOfBudget"));
    // Running out while building, counting the queries' pairs, and
    // collecting them (the count takes just under half of the pairing:
    // its tests, without the hits): the same failure and profile.
    let mut work = Work::new(&Budget::DEFAULT);
    let bvh = work
        .pass(PASS, &[()], |_, ledger| {
            Bvh::build(&boxes, ledger).map_err(Stop::<Infallible>::from)
        })
        .expect("built")
        .swap_remove(0);
    let built = work.used();
    bvh.pairs(&queries, &mut work, PASS).expect("in budget");
    let paired = work.used();
    assert!(paired - built > 100, "{built} {paired}");
    let pairing = paired - built;
    for budget in [built / 2, built + pairing / 6, built + 2 * pairing / 3] {
        let short = assert_early_stop_changes_nothing(|| run(budget));
        assert!(short.contains("OutOfBudget"), "budget {budget}");
    }
}
