//! Tiles against exact rationals: every certified value holds the exact
//! one of the stored net, the hull box the exact points, the normal cone
//! the exact normals; splits keep shared rows to the bit; singular sides
//! and folds are never regular.

use glam::DVec3;
use malachite::Rational as Q;
use varde_decide::bezier::{HomCurve, Implicit};
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
        rounded(t)
    );
}

/// `t` rounded to the nearest `f64`, ties to even.
fn rounded(t: &Q) -> f64 {
    use malachite::base::num::conversion::traits::RoundingFrom;
    use malachite::base::rounding_modes::RoundingMode::Nearest;
    f64::rounding_from(t, Nearest).0
}

fn exact(t: &Tile) -> HomPatch<Q> {
    t.patch_at(&Q::from(0u32))
}

fn affine(p: DVec3, w: f64) -> [f64; 4] {
    [p.x, p.y, p.z, w]
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
        affine(p, rng.log_range(1.0 / wr, wr))
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
                    // The cached derivatives give the patch's own bits;
                    // a corner is its control point exactly.
                    let (bu, bv) = (Bounded::exact(u), Bounded::exact(v));
                    let want = tile.patch().eval_partials(&bu, &bv);
                    assert_eq!((&got.du, &got.dv), (&want.du, &want.dv));
                    if let Some((i, j)) = corner(u, v) {
                        let c = tile.point(i, j).map(Bounded::exact);
                        assert_eq!((got.p, tile.eval(u, v)), (c, c));
                        assert_eq!(truth.p, c.map(|x| q(x.v)));
                    } else {
                        assert_eq!(got.p, want.p);
                        assert_eq!(tile.eval(u, v), tile.patch().eval(&bu, &bv));
                    }
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
            assert_eq!(tile.normal_cone_level().is_some(), cones[0].is_some());
            // At the level that certified it, the cone at a level is the
            // cached one; below it none is certified.
            let first = tile.normal_cone_at(&Bounded::exact(0.0));
            match tile.normal_cone_level() {
                Some(Level::L0) | None => assert_eq!(first, cones[0]),
                Some(level) => {
                    assert_eq!(first, None);
                    assert_eq!(cone_at(&tile, level), cones[0]);
                }
            }
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

/// The cone at `level`, in that level's type.
fn cone_at(tile: &Tile, level: Level) -> Option<NormalCone> {
    match level.width() {
        None if level == Level::L0 => tile.normal_cone_at(&Bounded::exact(0.0)),
        None => tile.normal_cone_at(&Bounded::exact(Dd::from_f64(0.0))),
        Some(w) => tile.normal_cone_at(&Bounded::exact(Big::from_f64(0.0, w))),
    }
}

/// Every exact normal at a grid of points and the corners lies in `c`.
fn holds_normals(tile: &Tile, c: &NormalCone) {
    cone_is_consistent(c);
    let e = exact(tile);
    for i in 0..=8 {
        for j in 0..=8 {
            let (u, v) = (f64::from(i) / 8.0, f64::from(j) / 8.0);
            assert!(
                in_cone(c, &normal(&e, u, v)),
                "{tile:?} at ({u}, {v}): {c:?}"
            );
        }
    }
}

/// A plane tile with `x = u`, `y = v·g(u)`, `g`'s coefficients `1, −0.9,
/// 1`: `g` is positive (`0.05` at its least), so the tile doesn't fold
/// and its normals are all `+z`, but the numerator's coefficients (the
/// elevation of `g`'s) are not, so the cone is certified only on the
/// split numerator, at the first level still.
#[test]
fn a_tile_past_its_coefficients_is_certified_in_space() {
    let tile = plane_tile([1.0, -0.9, 1.0]);
    assert_eq!(NormalCone::of_form(tile.normal_numerator()), None);
    let c = tile.normal_cone().expect("certified in space");
    assert_eq!(tile.normal_cone_level(), Some(Level::L0));
    assert_eq!(tile.normal_cone_at(&Bounded::exact(0.0)), Some(c));
    assert!(c.axis[2] > 0.0 && c.cos > 1.0 - 1e-12, "{c:?}");
    holds_normals(&tile, &c);
    // The exact numerator is certified in space too.
    assert!(tile.normal_cone_at(&Q::from(0u32)).is_some());
}

/// The plane tile `x = u`, `y = v·g(u)`, `z = 0` with `g`'s Bernstein
/// coefficients `a`.
fn plane_tile(a: [f64; 3]) -> Tile {
    let net = std::array::from_fn(|k| {
        let (i, j) = (k / 3, k % 3);
        affine(DVec3::new(i as f64 / 2.0, j as f64 / 2.0 * a[i], 0.0), 1.0)
    });
    Tile::new(net).unwrap()
}

/// A plane tile (as above) whose side `u = 0` is `2⁻⁶⁰` long: the numerator there is
/// below the first level's rounding (precision decides), and
/// double-double certifies the cone, the plane's normal.
#[test]
fn a_needle_corner_is_certified_at_the_next_level() {
    // `g`'s coefficients `2⁻⁶⁰, ½, 1`: positive on `[0, 1]`, so no
    // fold, and `2⁻⁶⁰` at `u = 0`.
    let tile = plane_tile([1.0 / (1u64 << 60) as f64, 0.5, 1.0]);
    assert_eq!(tile.normal_cone_at(&Bounded::exact(0.0)), None);
    let c = tile.normal_cone().expect("certified at a higher level");
    assert_eq!(tile.normal_cone_level(), Some(Level::L1));
    assert_eq!(cone_at(&tile, Level::L1), Some(c));
    assert!(c.axis[2] > 0.0 && c.cos > 1.0 - 1e-12, "{c:?}");
    holds_normals(&tile, &c);
}

#[test]
fn a_plane_tile_has_the_plane_normal() {
    let mut rng = Rng::new(7);
    let net = std::array::from_fn(|k| {
        let (i, j) = ((k / 3) as f64, (k % 3) as f64);
        affine(
            DVec3::new(3.0 * i + rng.range(-0.2, 0.2), 2.0 * j, 5.0),
            rng.log_range(0.25, 4.0),
        )
    });
    let tile = Tile::new(net).unwrap();
    let c = tile.normal_cone().expect("regular");
    assert!(c.axis[2] > 1.0 - 1e-12 && c.cos > 1.0 - 1e-12, "{c:?}");
    assert!(c.sin < 1e-5, "{c:?}");
}

/// An oblique plane's tile is flat: its numerator's one value holds the
/// exact numerator everywhere on the tile; a cylinder's tile isn't flat.
#[test]
fn a_plane_tile_is_flat_and_a_cylinder_tile_is_not() {
    use crate::geom::form::PlaneKey;
    use crate::geom::lattice::Lattice;
    use crate::geom::surface::Surface;
    let key = PlaneKey::new([1.0, 2.0, 3.0], [7.0, -1.0, 0.5]).unwrap();
    let lattice = Lattice::new(key.into()).unwrap();
    let rect = lattice.half(0).unwrap();
    let surface = Surface::new(lattice, rect).unwrap();
    let tile = &surface.tiles()[0];
    let flat = tile.flat_normal().expect("flat");
    let n = exact(tile).normal_numerator().expect("in range");
    let mut rng = Rng::new(11);
    for _ in 0..20 {
        let (u, v) = (q(rng.range(0.0, 1.0)), q(rng.range(0.0, 1.0)));
        for k in 0..3 {
            holds("flat normal", &flat[k], &n[k].eval(&u, &v));
        }
    }
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let arc = [
        (DVec3::new(10.0, 0.0, 0.0), 1.0),
        (DVec3::new(10.0, 10.0, 0.0), s),
        (DVec3::new(0.0, 10.0, 0.0), 1.0),
    ];
    let net = std::array::from_fn(|k| {
        let (p, w) = arc[k / 3];
        affine(p + DVec3::Z * (2.0 * (k % 3) as f64), w)
    });
    assert_eq!(Tile::new(net).unwrap().flat_normal(), None);
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
        affine(p + DVec3::Z * (2.0 * (k % 3) as f64), w)
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

/// An octant of a unit sphere with its pole row at `v = 0`: the three
/// control points of that row at the pole.
fn octant() -> Tile {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let meridian = [(1.0, 0.0, 1.0), (1.0, 1.0, s), (0.0, 1.0, 1.0)];
    let around = [(1.0, 0.0, 1.0), (1.0, 1.0, s), (0.0, 1.0, 1.0)];
    let net = std::array::from_fn(|k| {
        let ((cu, su, wu), (cv, sv, wv)) = (around[k / 3], meridian[k % 3]);
        // Pole at z = 1 (v = 0), equator at v = 1.
        let p = DVec3::new(sv * cu, sv * su, cv);
        affine(p, wu * wv)
    });
    Tile::new(net).unwrap()
}

/// Undeclared, a pole row stops the fold check (the exact numerator is
/// zero at its corners): no cone at any level.
#[test]
fn an_undeclared_singular_side_is_not_regular() {
    let tile = octant();
    assert_eq!(tile.singular(), Sides::NONE);
    assert!(tile.fold_stops());
    assert_eq!(tile.singular_proof(), None);
    assert!(!tile.is_regular());
    assert_eq!(tile.normal_cone_level(), None);
    assert_eq!(tile.normal_cone_at(&Q::from(0u32)), None);
    assert_eq!(
        tile.normal_cone_at(&Bounded::exact(Dd::from_f64(0.0))),
        None
    );
    // No level was tried; the corners' lines were formed.
    let work = tile.cone_work();
    assert_eq!(work.pieces, [0; LEVELS]);
    assert_eq!((work.exact_lines, work.exact_full), (2, false));
    // Away from the pole the same sphere is regular.
    let [_, upper] = tile.split_half(Dir::V).unwrap();
    assert!(upper.is_regular());
}

/// Declared, the pole row is proven singular on the exact net and
/// divided out: the tile is regular, its cone holding every normal
/// inside and the pole's normal, the quotient's limit there.
#[test]
fn a_declared_singular_side_is_regular() {
    let tile = octant().with_singular(Side::V0.into());
    assert_eq!(tile.singular(), Side::V0.into());
    assert!(!tile.fold_stops());
    let proof = tile.singular_proof().expect("a proven side");
    assert_eq!(proof.sides(), Side::V0.into());
    let c = tile.normal_cone().expect("regular");
    assert!(tile.is_regular());
    cone_is_consistent(&c);
    // Every normal off the pole row (where `x_u × x_v` is zero).
    let e = exact(&tile);
    let mut rng = Rng::new(3);
    let ps = params(&mut rng);
    for &u in ps.iter().chain(&[0.0, 1.0]) {
        for &v in ps.iter().chain(&[1.0 / (1u64 << 40) as f64, 1.0]) {
            if v > 0.0 {
                assert!(in_cone(&c, &normal(&e, u, v)), "({u}, {v})");
            }
        }
    }
    // The exact quotient's coefficients lie in the cone too, and with
    // them its values on the pole row, the normal's limit there.
    let [x, y, z] = tile
        .fold_numerator_at(&Q::from(0u32))
        .unwrap()
        .map(|f| f.coefs().to_vec());
    assert_eq!(x.len(), 6 * 5);
    for m in 0..x.len() {
        assert!(in_cone(&c, &[x[m].clone(), y[m].clone(), z[m].clone()]));
    }
    // Every level certifies it alike.
    assert!(tile.normal_cone_at(&Q::from(0u32)).is_some());
    assert!(cone_at(&tile, Level::L1).is_some());
    // A declared side that isn't singular (the equator) stops the check.
    let wrong = octant().with_singular(Side::V1.into());
    assert!(wrong.fold_stops());
    assert!(!wrong.is_regular());
    assert_eq!(wrong.normal_cone_at(&Q::from(0u32)), None);
    // Equal nets with other sides are other tiles.
    assert_ne!(tile, octant());
    assert_eq!(tile, octant().with_singular(Side::V0.into()));
}

/// The pole row stays declared on the halves it bounds, and both
/// halves are regular.
#[test]
fn splits_keep_the_singular_sides_they_bound() {
    let tile = octant().with_singular(Side::V0.into());
    let [lo, hi] = tile.split_half(Dir::V).unwrap();
    assert_eq!(
        (lo.singular(), hi.singular()),
        (Side::V0.into(), Sides::NONE)
    );
    let [a, b] = tile.split_half(Dir::U).unwrap();
    assert_eq!(
        (a.singular(), b.singular()),
        (Side::V0.into(), Side::V0.into())
    );
    for t in [lo, hi, a, b] {
        assert!(t.is_regular(), "{t:?}");
    }
    // The transposed octant, its pole row at `u = 0`.
    let net = *octant().net();
    let t = Tile::new(std::array::from_fn(|k| net[3 * (k % 3) + k / 3]))
        .unwrap()
        .with_singular(Side::U0.into());
    assert!(t.is_regular());
    let [lo, hi] = t.split_half(Dir::U).unwrap();
    assert_eq!(
        (lo.singular(), hi.singular()),
        (Side::U0.into(), Sides::NONE)
    );
}

/// A corner between two singular sides reads the whole exact quotient:
/// on `x = (u·v, u·v², u²·v)` the rows `u = 0` and `v = 0` are each one
/// point and `x_u × x_v = u·v·(−3u·v, u, v)`, whose quotient is zero at
/// that corner, so the check stops there.
#[test]
fn a_corner_between_two_singular_sides_reads_the_whole_quotient() {
    // The Bernstein coefficients of `t` and `t²`.
    let (a, b) = ([0.0, 0.5, 1.0], [0.0, 0.0, 1.0]);
    let net = std::array::from_fn(|k| {
        let (i, j) = (k / 3, k % 3);
        affine(DVec3::new(a[i] * a[j], a[i] * b[j], b[i] * a[j]), 1.0)
    });
    let t = Tile::new(net).unwrap();
    let both = t.with_singular(Sides::NONE.with(Side::U0).with(Side::V0));
    assert!(exact(&both).singular_sides(both.singular()).is_some());
    assert!(both.fold_stops());
    assert!(!both.is_regular());
    let work = both.cone_work();
    assert!(work.exact_full, "{work:?}");
    assert_eq!(work.pieces, [0; LEVELS]);
}

/// The charges of a cone search, in order.
#[derive(Default)]
struct Record(Vec<(Step, u64)>);

impl Charge for Record {
    type Err = std::convert::Infallible;

    fn charge(&mut self, step: Step, units: u64) -> Result<(), Self::Err> {
        self.0.push((step, units));
        Ok(())
    }
}

/// The cone search charges its work: the exact lines, then each level it
/// ran at, its numerator and pieces, as a precision step above the
/// first; the same on every call, cached or not, and on every copy.
#[test]
fn the_cone_search_is_charged_by_its_work() {
    let needle = plane_tile([1.0 / (1u64 << 60) as f64, 0.5, 1.0]);
    let xs = [0.0, 2.0, 1.0];
    let fold = Tile::new(std::array::from_fn(|k| {
        affine(DVec3::new(xs[k / 3], (k % 3) as f64, 0.0), 1.0)
    }))
    .unwrap();
    let u = UNITS.fold;
    for tile in [
        needle,
        fold,
        octant(),
        octant().with_singular(Side::V0.into()),
        plane_tile([1.0, -0.9, 1.0]),
    ] {
        let fresh = Tile::new(*tile.net())
            .unwrap()
            .with_singular(tile.singular());
        let mut first = Record::default();
        let Ok(cone) = fresh.normal_cone_charged(&mut first);
        let mut again = Record::default();
        let Ok(cached) = fresh.normal_cone_charged(&mut again);
        assert_eq!((cone, &first.0), (cached, &again.0));
        assert_eq!(cone, tile.normal_cone());
        let w = fresh.cone_work();
        assert_eq!(w, tile.cone_work());
        let mut want = vec![(Step::Work, u64::from(w.exact_lines) * u.line)];
        for (l, &p) in w.pieces.iter().enumerate() {
            if p > 0 {
                want.push((level_step(l as u8), u.level[l] + p * u.piece[l]));
            }
        }
        assert_eq!(first.0, want, "{tile:?}");
    }
    // The needle: one piece at the first level, which rounding decides,
    // then double-double.
    let w = plane_tile([1.0 / (1u64 << 60) as f64, 0.5, 1.0]).cone_work();
    assert_eq!(w.pieces[..3], [1, 1, 0]);
    // The fold: every depth at the first level, `1 + 4 + 16 + 64`.
    let w = Tile::new(std::array::from_fn(|k| {
        affine(DVec3::new(xs[k / 3], (k % 3) as f64, 0.0), 1.0)
    }))
    .unwrap()
    .cone_work();
    assert_eq!(w.pieces, [85, 0, 0, 0, 0]);
}

#[test]
fn a_fold_is_not_regular() {
    // In u the points 0, 2, 1 along x: x(u) turns back at u = 2/3.
    let xs = [0.0, 2.0, 1.0];
    let net = std::array::from_fn(|k| affine(DVec3::new(xs[k / 3], (k % 3) as f64, 0.0), 1.0));
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
                let pe = exact(&tile).split_half(dir);
                for (h, t) in halves.iter().zip(&pe) {
                    // Each value the exact half's, rounded once.
                    let [x, y, z, w] = t.tensors().each_ref().map(Tensor::coefs);
                    for (n, p) in h.net().iter().enumerate() {
                        for (k, c) in [x, y, z].into_iter().enumerate() {
                            assert_eq!(p[k], rounded(&(&c[n] / &w[n])), "{n} {k}");
                        }
                        assert_eq!(p[3], rounded(&w[n]), "{n}");
                    }
                }
                // The kept end lines are copied.
                let (a, b) = (line(tile.net(), dir, 0), line(tile.net(), dir, 2));
                assert_eq!(line(halves[0].net(), dir, 0), a);
                assert_eq!(line(halves[1].net(), dir, 2), b);
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
/// the exact one and at double-double, through `normal_cone` and
/// `normal_cone_at` alike: the translation to the frame rounds relative
/// to the tile's size, not to its distance from the origin.
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
            affine(p, [MIN_WEIGHT, MAX_WEIGHT][(rng.next_u64() % 2) as usize])
        });
        let tile = Tile::new(net).unwrap();
        let exact = tile.normal_cone_at(&Q::from(0u32));
        let first = tile.normal_cone();
        assert_eq!(first.is_some(), exact.is_some(), "{tile:?}");
        assert_eq!(tile.normal_cone_at(&Bounded::exact(0.0)), first);
        let dd = tile.normal_cone_at(&Bounded::exact(Dd::from_f64(0.0)));
        assert_eq!(dd.is_some(), exact.is_some(), "{tile:?}");
    }
}

#[test]
fn rounding_an_exact_net() {
    let (tile, _) = tiles(5).swap_remove(1);
    assert_eq!(Tile::rounded(&tile.exact()).unwrap(), tile);
    let one = || Q::from(1u32);
    let net = |w: Q| {
        (0..9).map(move |n| {
            let w = if n == 4 { w.clone() } else { one() };
            [one(), one(), one(), w]
        })
    };
    let zero = HomPatch::new(2, 2, net(Q::from(0u32))).unwrap();
    assert_eq!(Tile::rounded(&zero), Err(TileError::Weight { index: 4 }));
    let tiny = HomPatch::new(2, 2, net(Q::from_signeds(1, 128))).unwrap();
    assert_eq!(Tile::rounded(&tiny), Err(TileError::Weight { index: 4 }));
}

/// Every member of the patch trait on `tile` at `like`'s level against
/// the tile's operations there: the patch at the level
/// ([`Tile::patch_at`]) with the corners exact, the numerator in the
/// frame, the cone search, the split, the hull box, the singular sides on
/// the exact net and the closest point from the stored net.
fn trait_members<S: Scalar + 'static>(tile: &Tile, like: &S) {
    let p = tile.patch_at(like);
    let f = |x: f64| S::from_f64(x, like);
    assert_eq!(Patch::<S>::degrees(tile), (2, 2));
    assert_eq!(*Patch::hom(tile, like), p);
    let mut rng = Rng::new(11);
    let ps = params(&mut rng);
    for &u in &ps {
        for &v in &ps[..4] {
            let (bu, bv) = (f(u), f(v));
            let e = Patch::eval(tile, &bu, &bv);
            let d = Patch::eval_partials(tile, &bu, &bv);
            let want = p.eval_partials(&bu, &bv);
            assert_eq!((&d.du, &d.dv), (&want.du, &want.dv));
            match corner(u, v) {
                Some((i, j)) => {
                    let c = tile.point(i, j).map(f);
                    assert_eq!((&e, &d.p), (&c, &c));
                }
                None => assert_eq!((&e, &d.p), (&p.eval(&bu, &bv), &want.p)),
            }
            assert_eq!(
                Patch::derivatives(tile, &bu, &bv, 2),
                p.derivatives_at(&bu, &bv, 2)
            );
        }
    }
    for dir in [Dir::U, Dir::V] {
        assert_eq!(Patch::<S>::split_half(tile, dir), tile.split_half(dir));
    }
    assert_eq!(Patch::<S>::hull_box(tile), Some(tile.hull_box()));

    let n = numerator(&tile.framed(like));
    assert_eq!(Patch::normal_numerator(tile, like).as_ref(), Some(&n));
    let cone = Patch::normal_cone_at(tile, like);
    assert_eq!(cone.ok(), tile.normal_cone_at(like));
    assert_eq!(Patch::<S>::normal_cone(tile), tile.normal_cone());

    let e = tile.exact();
    for sides in [Sides::NONE, Side::V0.into(), Side::U1.into()] {
        let proof = e.singular_sides(sides);
        assert_eq!(Patch::singular_sides(tile, sides, like), proof);
        assert_eq!(
            Patch::fold_numerator(tile, sides, like),
            proof.and_then(|s| s.quotient(&n))
        );
    }
    assert_eq!(Patch::fold_numerator(tile, Sides::NONE, like), Some(n));

    let c = tile.hull_box().lo.map(f);
    let sphere = Implicit::new(
        2,
        c,
        [
            ([2, 0, 0], f(1.0)),
            ([0, 2, 0], f(1.0)),
            ([0, 0, 2], f(1.0)),
            ([0, 0, 0], f(-1.0)),
        ],
    )
    .expect("a quadric");
    let composed = p.compose_implicit(&sphere);
    assert!(composed.is_some());
    assert_eq!(Patch::compose_implicit(tile, &sphere), composed);
    let span = HomCurve::from_affine(
        [([0.125, 0.0], 1.0), ([0.5, 0.75], 0.5), ([1.0, 0.5], 2.0)],
        like,
    )
    .expect("a quadratic span");
    let curve = HomCurve::from_affine(
        [
            (tile.point(0, 0), 1.0),
            (tile.point(1, 1), 2.0),
            (tile.point(2, 2), 1.0),
        ],
        like,
    )
    .expect("a quadratic curve");
    let along = p.compose_span(&span).expect("in range");
    assert_eq!(Patch::compose_span(tile, &span).as_ref(), Some(&along));
    assert_eq!(
        Patch::seam_numerator(tile, &span, &curve),
        along.seam_numerator(&curve)
    );

    let x = tile.point(1, 1);
    assert_eq!(
        Patch::closest(tile, x, like),
        Closest::patch(2, 2, tile.net(), x, like)
    );
}

/// At the first level the members are the tile's inherent (cached)
/// operations, to the bit.
fn first_level_members(tile: &Tile) {
    let like = Bounded::exact(0.0);
    assert!(matches!(Patch::hom(tile, &like), Cow::Borrowed(p) if p == tile.patch()));
    let mut rng = Rng::new(12);
    let ps = params(&mut rng);
    for &u in &ps {
        for &v in &ps {
            let (bu, bv) = (Bounded::exact(u), Bounded::exact(v));
            assert_eq!(Patch::eval(tile, &bu, &bv), tile.eval(u, v));
            assert_eq!(
                Patch::eval_partials(tile, &bu, &bv),
                tile.eval_partials(u, v)
            );
        }
    }
    assert_eq!(
        Patch::normal_numerator(tile, &like).as_ref(),
        Some(tile.normal_numerator())
    );
    assert_eq!(
        Patch::normal_cone_at(tile, &like).ok(),
        tile.normal_cone_at(&like)
    );
}

#[test]
fn the_patch_trait_is_the_tiles_operations() {
    let dd = Bounded::exact(Dd::ZERO);
    let mut all: Vec<Tile> = tiles(5).into_iter().map(|(t, _)| t).collect();
    all.push(octant());
    all.push(octant().with_singular(Side::V0.into()));
    all.push(plane_tile([1.0 / (1u64 << 60) as f64, 0.5, 1.0]));
    for tile in &all {
        first_level_members(tile);
        trait_members(tile, &Bounded::exact(0.0));
        trait_members(tile, &dd);
    }
    // The pole row: proven singular whatever the level, the fold
    // numerator its exact quotient's, no cone (no level helps).
    let pole = octant();
    let side: Sides = Side::V0.into();
    let proof = Patch::singular_sides(&pole, side, &dd).expect("the pole row");
    assert_eq!(
        Patch::singular_sides(&pole, side, &Bounded::exact(0.0)),
        Some(proof)
    );
    assert_eq!(
        Patch::fold_numerator(&pole, side, &dd).map(|n| n[0].degrees()),
        Some((5, 4))
    );
    assert_eq!(Patch::normal_cone_at(&pole, &dd), Err(ConeFail::Space));
    // The needle corner: rounding decides at the first level, the next
    // certifies the cached cone.
    let needle = plane_tile([1.0 / (1u64 << 60) as f64, 0.5, 1.0]);
    assert_eq!(
        Patch::normal_cone_at(&needle, &Bounded::exact(0.0)),
        Err(ConeFail::Precision)
    );
    assert_eq!(
        Patch::normal_cone_at(&needle, &dd).ok(),
        needle.normal_cone()
    );
    assert!(Patch::<Bounded<Dd>>::normal_cone(&needle).is_some());
}
