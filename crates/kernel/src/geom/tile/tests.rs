//! Tiles against exact rationals: every certified value holds the exact
//! one of the stored net, the hull box the exact points, the normal cone
//! the exact normals; splits keep shared rows to the bit; singular sides
//! and folds are never regular.

use glam::DVec3;
use malachite::Rational as Q;
use varde_decide::num::bigf::{BigF, Width};
use varde_decide::num::{bigf::Big, dd::Dd};
use varde_testkit::rng::Rng;

use super::*;

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

fn holds<S: Scalar>(what: &str, b: &S, t: &Q) {
    let [lo, hi] = b.enclose();
    assert!(
        lo.is_finite() && hi.is_finite(),
        "{what}: uncertified {b:?}"
    );
    assert!(
        q(lo) <= *t && *t <= q(hi),
        "{what}: {b:?} misses {}",
        approx(t)
    );
}

/// `t` rounded to the nearest `f64`, for messages.
fn approx(t: &Q) -> f64 {
    use malachite::base::num::conversion::traits::RoundingFrom;
    use malachite::base::rounding_modes::RoundingMode::Nearest;
    f64::rounding_from(t, Nearest).0
}

fn exact(t: &Tile) -> HomPatch<Q> {
    t.patch_at(&Q::from(0u32))
}

fn hom(p: DVec3, w: f64) -> [f64; 4] {
    [w * p.x, w * p.y, w * p.z, w]
}

/// A random tile: a parallelogram grid at a random place, scale and
/// orientation, its points moved off the plane by up to `bump` of its
/// size and its weights in `[1/wr, wr]`.
fn random(rng: &mut Rng, bump: f64, wr: f64) -> Tile {
    let extent = [1.0, 1e3, 1e6][(rng.next_u64() % 3) as usize];
    let origin = rng.point(extent);
    let size = rng.log_range(1e-3, 1e3);
    let e1 = rng.direction();
    let e2 = loop {
        let d = rng.direction();
        let o = d - e1 * d.dot(e1);
        if o.length() > 0.3 {
            break o.normalize();
        }
    };
    let n = e1.cross(e2);
    let net = std::array::from_fn(|k| {
        let (i, j) = ((k / 3) as f64, (k % 3) as f64);
        let shift = rng.range(-0.1, 0.1);
        let p =
            origin + (e1 * (0.5 * i + shift) + e2 * (0.5 * j) + n * rng.range(-bump, bump)) * size;
        hom(p, rng.log_range(1.0 / wr, wr))
    });
    Tile::new(net).expect("a valid tile")
}

/// Near-flat tiles with moderate weights (certainly regular) and wild
/// ones (any weights, large bumps).
fn tiles(seed: u64) -> Vec<(Tile, bool)> {
    let mut rng = Rng::new(seed);
    (0..8)
        .map(|k| {
            if k % 2 == 0 {
                (random(&mut rng, 0.1, 2.0), true)
            } else {
                (random(&mut rng, 1.0, 64.0), false)
            }
        })
        .collect()
}

fn params(rng: &mut Rng) -> Vec<f64> {
    let mut v = vec![0.0, 1.0, 0.5];
    for _ in 0..3 {
        v.push((rng.next_u64() % 65) as f64 / 64.0);
        v.push(rng.unit());
    }
    v
}

fn cross(a: &[Q; 3], b: &[Q; 3]) -> [Q; 3] {
    [
        &a[1] * &b[2] - &a[2] * &b[1],
        &a[2] * &b[0] - &a[0] * &b[2],
        &a[0] * &b[1] - &a[1] * &b[0],
    ]
}

fn dot(a: &[Q; 3], b: &[Q; 3]) -> Q {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}

/// Whether `n` is certainly in the cone: `a·n > 0` and
/// `(a·n)² ≥ cos²·|a|²·|n|²`, exactly.
fn in_cone(c: &NormalCone, n: &[Q; 3]) -> bool {
    let a = c.axis.map(q);
    let d = dot(&a, n);
    let cos2 = q(c.cos) * q(c.cos);
    d > 0u32 && &d * &d >= cos2 * dot(&a, &a) * dot(n, n)
}

fn cone_is_consistent(c: &NormalCone) {
    assert!(c.cos > 0.0 && c.cos <= 1.0 && c.sin <= 1.0, "{c:?}");
    // `sin ≥ √(1 − cos²)`.
    assert!(q(c.sin) * q(c.sin) + q(c.cos) * q(c.cos) >= 1u32, "{c:?}");
}

/// The exact normal `x_u × x_v` at `(u, v)`.
fn normal(e: &HomPatch<Q>, u: f64, v: f64) -> [Q; 3] {
    let p = e.eval_partials(&q(u), &q(v));
    cross(&p.du, &p.dv)
}

#[test]
fn eval_and_partials_hold_the_exact_values() {
    for seed in varde_testing::seeds(8, 256) {
        let mut rng = Rng::new(seed ^ 0x51);
        for (tile, _) in tiles(seed) {
            let e = exact(&tile);
            for &u in &params(&mut rng) {
                for &v in &params(&mut rng) {
                    let got = tile.eval_partials(u, v);
                    let truth = e.eval_partials(&q(u), &q(v));
                    for k in 0..3 {
                        holds("point", &got.p[k], &truth.p[k]);
                        holds("x_u", &got.du[k], &truth.du[k]);
                        holds("x_v", &got.dv[k], &truth.dv[k]);
                    }
                    // The cached derivatives give the patch's own bits.
                    let (bu, bv) = (Bounded::exact(u), Bounded::exact(v));
                    assert_eq!(got, tile.patch().eval_partials(&bu, &bv));
                    assert_eq!(tile.eval(u, v), tile.patch().eval(&bu, &bv));
                }
            }
        }
    }
}

#[test]
fn hull_box_holds_the_tile() {
    for seed in varde_testing::seeds(8, 256) {
        let mut rng = Rng::new(seed ^ 0x52);
        for (tile, _) in tiles(seed) {
            let (e, b) = (exact(&tile), tile.hull_box());
            for &u in &params(&mut rng) {
                for &v in &params(&mut rng) {
                    let p = e.eval(&q(u), &q(v));
                    for k in 0..3 {
                        assert!(q(b.lo[k]) <= p[k] && p[k] <= q(b.hi[k]), "{b:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn higher_levels_hold_the_exact_values() {
    let dd = Bounded::exact(Dd::from_f64(0.0));
    let big = Bounded::exact(Big::from_f64(0.0, Width::MIN));
    for seed in varde_testing::seeds(4, 64) {
        let mut rng = Rng::new(seed ^ 0x53);
        for (tile, flat) in tiles(seed) {
            let (e, pd, pb) = (exact(&tile), tile.patch_at(&dd), tile.patch_at(&big));
            for &u in &params(&mut rng) {
                let v = rng.unit();
                let truth = e.eval_partials(&q(u), &q(v));
                let gd = pd.eval_partials(&Scalar::from_f64(u, &dd), &Scalar::from_f64(v, &dd));
                let gb = pb.eval_partials(&Scalar::from_f64(u, &big), &Scalar::from_f64(v, &big));
                for k in 0..3 {
                    holds("dd point", &gd.p[k], &truth.p[k]);
                    holds("dd x_u", &gd.du[k], &truth.du[k]);
                    holds("big point", &gb.p[k], &truth.p[k]);
                    holds("big x_v", &gb.dv[k], &truth.dv[k]);
                }
            }
            if flat {
                assert!(tile.normal_cone_at(&dd).is_some(), "{tile:?}");
                assert!(tile.normal_cone_at(&big).is_some(), "{tile:?}");
            }
        }
    }
}

#[test]
fn normal_cone_holds_every_normal() {
    let mut wild = [0u32; 2];
    for seed in varde_testing::seeds(8, 256) {
        let mut rng = Rng::new(seed ^ 0x54);
        for (tile, flat) in tiles(seed) {
            let cones = [tile.normal_cone(), tile.normal_cone_at(&Q::from(0u32))];
            assert_eq!(tile.is_regular(), cones[0].is_some());
            if flat {
                assert!(tile.is_regular(), "{tile:?}");
            } else {
                wild[usize::from(tile.is_regular())] += 1;
            }
            let e = exact(&tile);
            for c in cones.iter().flatten() {
                cone_is_consistent(c);
                for &u in &params(&mut rng) {
                    for &v in &params(&mut rng) {
                        let n = normal(&e, u, v);
                        assert!(in_cone(c, &n), "{tile:?} at ({u}, {v}): {c:?}");
                    }
                }
            }
            // The first level's cone is never certified where the exact
            // one isn't.
            if cones[0].is_some() {
                assert!(cones[1].is_some());
            }
        }
    }
    assert!(wild[1] > 0, "some wild tiles are regular: {wild:?}");
}

#[test]
fn a_plane_tile_has_the_plane_normal() {
    let mut rng = Rng::new(7);
    let net = std::array::from_fn(|k| {
        let (i, j) = ((k / 3) as f64, (k % 3) as f64);
        hom(
            DVec3::new(3.0 * i + rng.range(-0.2, 0.2), 2.0 * j, 5.0),
            rng.log_range(0.25, 4.0),
        )
    });
    let tile = Tile::new(net).unwrap();
    let c = tile.normal_cone().expect("regular");
    assert!(c.axis[2] > 1.0 - 1e-12 && c.cos > 1.0 - 1e-12, "{c:?}");
    assert!(c.sin < 1e-5, "{c:?}");
}

#[test]
fn a_cylinder_tile_is_regular() {
    // A quarter of a cylinder about z, radius 10 and height 4.
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let arc = [
        (DVec3::new(10.0, 0.0, 0.0), 1.0),
        (DVec3::new(10.0, 10.0, 0.0), s),
        (DVec3::new(0.0, 10.0, 0.0), 1.0),
    ];
    let net = std::array::from_fn(|k| {
        let (p, w) = arc[k / 3];
        hom(p + DVec3::Z * (2.0 * (k % 3) as f64), w)
    });
    let tile = Tile::new(net).unwrap();
    let c = tile.normal_cone().expect("regular");
    cone_is_consistent(&c);
    // Outward: x_u turns counter-clockwise, x_v is up.
    assert!(
        (c.axis[0] - s).abs() < 1e-9 && (c.axis[1] - s).abs() < 1e-9,
        "{c:?}"
    );
    // The normals span 90°: the cone's half-angle is at least 45°.
    assert!(c.cos < s && c.cos > 0.5, "{c:?}");
    let e = exact(&tile);
    for &(u, v) in &[(0.0, 0.0), (1.0, 1.0), (0.5, 0.25), (0.125, 0.75)] {
        let n = normal(&e, u, v);
        assert!(in_cone(&c, &n));
        assert_eq!(n[2], 0u32);
    }
}

#[test]
fn a_singular_side_is_not_regular() {
    // An octant of a unit sphere with its pole row at v = 0: the three
    // control points of that row at the pole.
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let meridian = [(1.0, 0.0, 1.0), (1.0, 1.0, s), (0.0, 1.0, 1.0)];
    let around = [(1.0, 0.0, 1.0), (1.0, 1.0, s), (0.0, 1.0, 1.0)];
    let net = std::array::from_fn(|k| {
        let ((cu, su, wu), (cv, sv, wv)) = (around[k / 3], meridian[k % 3]);
        // Pole at z = 1 (v = 0), equator at v = 1.
        let p = DVec3::new(sv * cu, sv * su, cv);
        hom(p, wu * wv)
    });
    let tile = Tile::new(net).unwrap();
    assert!(!tile.is_regular());
    assert_eq!(tile.normal_cone_at(&Q::from(0u32)), None);
    // Away from the pole the same sphere is regular.
    let [_, upper] = tile.split_half(Dir::V).unwrap();
    assert!(upper.is_regular());
}

#[test]
fn a_fold_is_not_regular() {
    // In u the points 0, 2, 1 along x: x(u) turns back at u = 2/3.
    let xs = [0.0, 2.0, 1.0];
    let net = std::array::from_fn(|k| hom(DVec3::new(xs[k / 3], (k % 3) as f64, 0.0), 1.0));
    let tile = Tile::new(net).unwrap();
    assert!(!tile.is_regular());
    assert_eq!(tile.normal_cone_at(&Q::from(0u32)), None);
}

#[test]
fn nets_are_validated() {
    let good: Net = std::array::from_fn(|k| [(k / 3) as f64, (k % 3) as f64, 0.0, 1.0]);
    assert!(Tile::new(good).is_ok());
    let with = |index: usize, c: usize, x: f64| {
        let mut n = good;
        n[index][c] = x;
        Tile::new(n).map(|_| ())
    };
    assert_eq!(with(4, 3, MIN_WEIGHT), Ok(()));
    assert_eq!(with(4, 3, MAX_WEIGHT), Ok(()));
    let below = f64::from_bits(MIN_WEIGHT.to_bits() - 1);
    let above = f64::from_bits(MAX_WEIGHT.to_bits() + 1);
    assert_eq!(with(4, 3, below), Err(TileError::Weight { index: 4 }));
    assert_eq!(with(5, 3, above), Err(TileError::Weight { index: 5 }));
    assert_eq!(with(0, 3, -1.0), Err(TileError::Weight { index: 0 }));
    assert_eq!(with(0, 3, 0.0), Err(TileError::Weight { index: 0 }));
    assert_eq!(with(2, 0, f64::NAN), Err(TileError::NotFinite { index: 2 }));
    assert_eq!(
        with(8, 2, f64::INFINITY),
        Err(TileError::NotFinite { index: 8 })
    );
    assert_eq!(with(8, 3, f64::NAN), Err(TileError::NotFinite { index: 8 }));
    // Coordinates far past any body are finite, so they stand.
    assert_eq!(with(1, 1, 1e300), Ok(()));
    // Non-finite values are reported before weights out of range.
    let mut n = good;
    n[0][3] = 100.0;
    n[7][0] = f64::NEG_INFINITY;
    assert_eq!(
        Tile::new(n).map(|_| ()),
        Err(TileError::NotFinite { index: 7 })
    );
}

/// Row `i` of a net (fixed `u` index), or column `j` for `Dir::V`.
fn line(net: &Net, dir: Dir, at: usize) -> [u64; 12] {
    let idx = |k: usize| match dir {
        Dir::U => 3 * at + k,
        Dir::V => 3 * k + at,
    };
    std::array::from_fn(|n| net[idx(n / 4)][n % 4].to_bits())
}

#[test]
fn splits_keep_shared_lines() {
    for seed in varde_testing::seeds(8, 256) {
        let mut rng = Rng::new(seed ^ 0x55);
        for (tile, _) in tiles(seed) {
            for dir in [Dir::U, Dir::V] {
                let halves = tile.split_half(dir).unwrap();
                let pieces = tile.patch().split_half(dir);
                let pe = exact(&tile).split_half(dir);
                for (h, (p, t)) in halves.iter().zip(pieces.iter().zip(&pe)) {
                    // The stored values of the first level's split, which
                    // hold the exact halves.
                    assert_eq!(*h.net(), values(p));
                    for k in 0..4 {
                        for (b, x) in p.tensors()[k].coefs().iter().zip(t.tensors()[k].coefs()) {
                            holds("split", b, x);
                        }
                    }
                }
                assert_eq!(line(halves[0].net(), dir, 2), line(halves[1].net(), dir, 0));
            }
            // A neighbour past u = 1, sharing that row, splits it into the
            // same halves, and evaluates on it to the same bits.
            let other = random(&mut rng, 0.5, 8.0);
            let mut net = *other.net();
            net[..3].copy_from_slice(&tile.net()[6..]);
            let next = Tile::new(net).unwrap();
            for &v in &params(&mut rng) {
                assert_eq!(tile.eval(1.0, v), next.eval(0.0, v));
            }
            let (a, b) = (
                tile.split_half(Dir::V).unwrap(),
                next.split_half(Dir::V).unwrap(),
            );
            for h in 0..2 {
                assert_eq!(line(a[h].net(), Dir::U, 2), line(b[h].net(), Dir::U, 0));
            }
        }
    }
}

#[test]
fn caches_match_the_patch() {
    let (tile, _) = tiles(3).swap_remove(0);
    assert_eq!(*tile.derivative(Dir::U), tile.patch().derivative(Dir::U));
    assert_eq!(*tile.derivative(Dir::V), tile.patch().derivative(Dir::V));
    assert_eq!(tile.normal_numerator()[0].degrees(), (5, 5));
    let copy = tile.clone();
    assert_eq!(copy, tile);
    assert_eq!(copy.normal_cone(), tile.normal_cone());
    // The frame changes the numerator's rounding only: it holds the
    // exact numerator of the stored net.
    let truth = exact(&tile).normal_numerator().unwrap();
    for (got, t) in tile.normal_numerator().iter().zip(&truth) {
        for (b, x) in got.coefs().iter().zip(t.coefs()) {
            holds("numerator", b, x);
        }
    }
}

/// A near-flat tile far from the origin, small or large and with weights
/// `1/64` and `64` mixed, gets its normal cone at the first level as at
/// the exact one: the translation to the frame rounds relative to the
/// tile's size, not to its distance from the origin.
#[test]
fn far_tiles_are_regular_at_the_first_level() {
    for seed in varde_testing::seeds(40, 400) {
        let mut rng = Rng::new(seed ^ 0x56);
        let origin = rng.point([1e6, 1e9, 1e12][(seed % 3) as usize]);
        let size = rng.log_range(1e-9, 1e3);
        let e1 = rng.direction();
        let e2 = rng.direction().cross(e1).normalize();
        let n = e1.cross(e2);
        let net = std::array::from_fn(|k| {
            let (i, j) = ((k / 3) as f64, (k % 3) as f64);
            let p = origin
                + (e1 * (0.5 * i + rng.range(-0.05, 0.05))
                    + e2 * (0.5 * j)
                    + n * rng.range(-1e-3, 1e-3))
                    * size;
            hom(p, [MIN_WEIGHT, MAX_WEIGHT][(rng.next_u64() % 2) as usize])
        });
        let tile = Tile::new(net).unwrap();
        let exact = tile.normal_cone_at(&Q::from(0u32));
        assert_eq!(tile.normal_cone().is_some(), exact.is_some(), "{tile:?}");
    }
}
