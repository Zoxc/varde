use super::*;
use crate::par::assert_deterministic;
use crate::test_rng::Rng;

/// `n` random boxes of sizes on a log scale, some flat, in a cube.
fn random_boxes(rng: &mut Rng, n: usize) -> Vec<Bounds3> {
    (0..n)
        .map(|_| {
            let min = rng.point(50.0);
            let mut size = glam::DVec3::new(
                rng.log_range(1e-3, 5.0),
                rng.log_range(1e-3, 5.0),
                rng.log_range(1e-3, 5.0),
            );
            if rng.unit() < 0.2 {
                size.z = 0.0;
            }
            Bounds3 {
                min,
                max: min + size,
            }
        })
        .collect()
}

#[test]
fn queries_match_brute_force() {
    let mut rng = Rng::new(31);
    for n in [0, 1, 3, 4, 5, 17, 300] {
        let boxes = random_boxes(&mut rng, n);
        let bvh = Bvh::new(boxes.clone());
        assert_eq!(bvh.len(), n);
        for margin in [0.0, 1e-6, 0.5] {
            for _ in 0..20 {
                let query = random_boxes(&mut rng, 1)[0];
                let mut found = vec![7];
                bvh.query(&query, margin, &mut found);
                let expected: Vec<u32> = std::iter::once(7)
                    .chain((0..n as u32).filter(|&i| near(&boxes[i as usize], &query, margin)))
                    .collect();
                assert_eq!(found, expected);
            }
            let mut expected = Vec::new();
            for i in 0..n as u32 {
                for j in i + 1..n as u32 {
                    if near(&boxes[i as usize], &boxes[j as usize], margin) {
                        expected.push([i, j]);
                    }
                }
            }
            assert_eq!(bvh.self_pairs(margin), expected);
        }
    }
}

#[test]
fn touching_and_margins() {
    let unit = |x: f64| Bounds3 {
        min: glam::DVec3::new(x, 0.0, 0.0),
        max: glam::DVec3::new(x + 1.0, 1.0, 1.0),
    };
    // Touching faces are near; a gap is near only within the margin.
    let bvh = Bvh::new(vec![unit(0.0), unit(1.0), unit(2.5)]);
    assert_eq!(bvh.self_pairs(0.0), vec![[0, 1]]);
    assert_eq!(bvh.self_pairs(0.5), vec![[0, 1], [1, 2]]);
    // Many copies of one box: the splits still end.
    let bvh = Bvh::new(vec![unit(0.0); 100]);
    assert_eq!(bvh.self_pairs(0.0).len(), 100 * 99 / 2);
}

#[test]
fn pairs_are_deterministic() {
    let boxes = random_boxes(&mut Rng::new(32), 5000);
    let pairs = assert_deterministic(|| Bvh::new(boxes.clone()).self_pairs(0.1));
    assert!(!pairs.is_empty());
}
