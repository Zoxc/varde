//! Checks across every form at once, each over the sample surfaces of
//! every form ([`super::samples`]) at levels from the base to the cap:
//! tiles on their implicits within `10⁻¹⁵` of their base tile's
//! coordinate scale, every tile regular, equal keys from other
//! construction frames giving the same lattice bit for bit, and `extend`
//! keeping tiles and the p-curves on them. Each prints what it measured
//! (`--nocapture`).

use malachite::Rational as Q;
use varde_decide::bezier::{Dir, Implicit, Sides};
use varde_testkit::rng::Rng;

use super::certify::implicit::ImplicitAt;
use super::certify::seam_bound_alone;
use super::curve::Curve3D;
use super::form::{
    AffineKey, ConeKey, ConicCylinderKey, CylinderKey, FormError, FormKey, PlaneKey, RevolvedKey,
    SphereKey, TorusKey,
};
use super::lattice::{Lattice, LatticeError, Level, Rect};
use super::pcurve::PCurve;
use super::round::nearest;
use super::samples::{Sample, surfaces};
use super::surface::Surface;
use super::tile::Tile;
use crate::infra::budget::MAX_WORK;

/// The bound on a tile's distance from its implicit, as a part of its
/// base tile's coordinate scale.
const IMPLICIT_TOL: f64 = 1e-15;

fn larger(a: f64, b: f64) -> f64 {
    if a > b { a } else { b }
}

/// The largest coordinate magnitude of a tile's control points.
fn scale(t: &Tile) -> f64 {
    t.net()
        .iter()
        .flat_map(|p| p[..3].iter().map(|x| x.abs()))
        .fold(0.0, larger)
}

/// `|∇F|` at `x`, from the exact implicit's terms (rounded at the end).
fn gradient(imp: &Implicit<Q>, x: &[Q; 3]) -> f64 {
    let y: [Q; 3] = std::array::from_fn(|k| &x[k] - &imp.origin()[k]);
    let mut g = [Q::from(0u32), Q::from(0u32), Q::from(0u32)];
    for (e, c) in imp.terms() {
        for k in 0..3 {
            if e[k] == 0 {
                continue;
            }
            let mut t = c * Q::from(e[k] as u32);
            for (m, ym) in y.iter().enumerate() {
                let p = if m == k { e[m] - 1 } else { e[m] };
                for _ in 0..p {
                    t *= ym;
                }
            }
            g[k] += t;
        }
    }
    nearest(&(&g[0] * &g[0] + &g[1] * &g[1] + &g[2] * &g[2])).sqrt()
}

/// The levels of one direction from `lo` to the cap `hi`: the first few,
/// the middle and the last few.
fn levels_to(lo: u32, hi: u32) -> Vec<u32> {
    let mut out: Vec<u32> = [lo, lo + 1, lo + 2, lo + 4, lo + 8, (lo + hi) / 2]
        .into_iter()
        .chain([hi.saturating_sub(2), hi.saturating_sub(1), hi])
        .filter(|&l| lo <= l && l <= hi)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The levels a sample is checked at, from its rectangle's to the cap
/// of its cells: each direction's levels against the other's at the
/// same step, and each direction's cap against the other's base.
fn levels(rect: &Rect, cap: Level) -> Vec<Level> {
    let us = levels_to(rect.level.u, cap.u);
    let vs = levels_to(rect.level.v, cap.v);
    let n = us.len().max(vs.len());
    let pick = |l: &[u32], k: usize| l[k * (l.len() - 1) / (n - 1).max(1)];
    let mut out: Vec<Level> = (0..n)
        .map(|k| Level {
            u: pick(&us, k),
            v: pick(&vs, k),
        })
        .collect();
    out.push(Level {
        u: cap.u,
        v: rect.level.v,
    });
    out.push(Level {
        u: rect.level.u,
        v: cap.v,
    });
    out.sort_unstable();
    out.dedup();
    out
}

/// Tiles `(i, j)` of `level` within `rect`: its four corners' (next to
/// the rectangle's sides, where poles and apexes are) and `random` more.
fn tiles_at(rect: &Rect, level: Level, random: usize, rng: &mut Rng) -> Vec<(i64, i64)> {
    let (su, sv) = (level.u - rect.level.u, level.v - rect.level.v);
    let (u0, u1) = (rect.u.start << su, (rect.u.end << su) - 1);
    let (v0, v1) = (rect.v.start << sv, (rect.v.end << sv) - 1);
    let mut out = vec![(u0, v0), (u0, v1), (u1, v0), (u1, v1)];
    let draw = |rng: &mut Rng, a: i64, b: i64| a + (rng.next_u64() % ((b - a + 1) as u64)) as i64;
    for _ in 0..random {
        out.push((draw(rng, u0, u1), draw(rng, v0, v1)));
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// The canonical (base-level) samples: the refined ones are their
/// rectangles at finer levels, which the level sweeps cover.
fn canonical(seed: u64) -> Vec<Sample> {
    surfaces(seed)
        .into_iter()
        .filter(|s| !s.name.ends_with("(refined)"))
        .collect()
}

/// Every tile of every form, at levels from the base to the cap, lies on
/// its implicit within `10⁻¹⁵` of its base tile's coordinate scale: at
/// points inside the tile, `|F|/|∇F|` exactly (rounded once), `F` the
/// exact implicit of the tile's cell and the point the tile's exact
/// point. Also measures each level's distance against the base level's
/// and against the tile's own coordinate scale.
#[test]
fn tiles_lie_on_their_implicits_to_the_cap() {
    let at = [0.125, 0.5, 0.875].map(|x| Q::try_from(x).unwrap());
    let one = Q::from(1u32);
    let mut worst_all: f64 = 0.0;
    for seed in varde_testing::seeds(1, 8) {
        let mut rng = Rng::new(1000 + seed);
        for Sample { name, surface } in canonical(seed) {
            let lattice = surface.lattice();
            let rect = surface.rect();
            let cap = lattice.cap_of(rect).unwrap();
            // Worst distance over the base tile's scale, at the
            // rectangle's level and at any finer one; worst over the
            // tile's own scale.
            let (mut base, mut finer, mut own) = (0.0f64, 0.0f64, 0.0f64);
            let mut count = 0;
            for level in levels(rect, cap) {
                for (i, j) in tiles_at(rect, level, 3, &mut rng) {
                    let what = format!("seed {seed}, {name}, {level:?} ({i}, {j})");
                    let tile = lattice.tile(level, i, j).expect(&what);
                    let (k, l) = (i >> level.u, j >> level.v);
                    let b = rect.level;
                    let s0 = scale(&lattice.base_tile(k, l).expect(&what));
                    let imp = lattice
                        .implicit(k, l)
                        .expect(&what)
                        .implicit_at(&Q::from(0u32));
                    let patch = tile.exact();
                    let s = scale(&tile);
                    for u in &at {
                        for v in &at {
                            let x = patch.eval(u, v);
                            let f = nearest(&imp.eval_hom(&x, &one)).abs();
                            if f == 0.0 {
                                continue;
                            }
                            let g = gradient(&imp, &x);
                            assert!(g > 0.0, "{what}: a regular point of the implicit");
                            let d = f / g;
                            if level == b {
                                base = larger(base, d / s0);
                            } else {
                                finer = larger(finer, d / s0);
                            }
                            own = larger(own, d / s);
                            assert!(d <= IMPLICIT_TOL * s0, "{what}: {:e} of {s0:e}", d / s0);
                        }
                    }
                    count += 1;
                }
            }
            worst_all = larger(worst_all, larger(base, finer));
            println!(
                "{name:<26} cap {:>2},{:<2} {count:>3} tiles: base {base:.2e}, \
                 finer {finer:.2e} of the base scale; {own:.2e} of the tile's",
                cap.u, cap.v
            );
        }
    }
    println!("worst: {worst_all:.3e} of the base tile's scale (bound {IMPLICIT_TOL:e})");
}

/// Every tile of every form is regular, at levels from the base to the
/// cap: the fold check certifies its normal cone (dividing out the
/// declared singular sides). Not yet beside a pole or an apex at levels
/// deep in both directions: there a cell's rows next to the singular one
/// turn by less than their coordinates' spacing (their chord is the
/// row's radius, a small part of the form's radius the cap reads, times
/// the cell's angle), round to one point, and the fold check stops. Those
/// tiles are counted and printed apart; every other tile must be
/// regular, and those must all be of that kind.
#[test]
fn every_tile_is_regular_to_the_cap() {
    let mut failed = Vec::new();
    let mut beside = Vec::new();
    let mut total = 0;
    for seed in varde_testing::seeds(1, 8) {
        let mut rng = Rng::new(2000 + seed);
        for Sample { name, surface } in canonical(seed) {
            let lattice = surface.lattice();
            let rect = surface.rect();
            let cap = lattice.cap_of(rect).unwrap();
            for level in levels(rect, cap) {
                for (i, j) in tiles_at(rect, level, 2, &mut rng) {
                    let tile = lattice.tile(level, i, j).unwrap();
                    total += 1;
                    if tile.is_regular() {
                        continue;
                    }
                    let what = format!("seed {seed}, {name}, cap {cap:?}, {level:?} ({i}, {j})");
                    if tile.singular() != Sides::NONE && tile.fold_stops() {
                        beside.push(what);
                    } else {
                        failed.push(what);
                    }
                }
            }
        }
    }
    println!(
        "{total} tiles: {} not regular beside a singular side, {} others",
        beside.len(),
        failed.len()
    );
    for f in &beside {
        println!("  beside: {f}");
    }
    assert!(failed.is_empty(), "not regular: {failed:#?}");
}

/// Pairs of one form built from other construction frames: another
/// point on the plane or axis, the direction scaled (to huge and tiny)
/// and reversed, a cone's half-angle legs scaled, a profile given from
/// its other end and at another height along the axis, an affine image of
/// such a source; each kind with values that round on both routes.
fn other_frames(rng: &mut Rng) -> Vec<(&'static str, FormKey, FormKey)> {
    let int = |rng: &mut Rng, m: u64| (rng.next_u64() % (2 * m + 1)) as f64 - m as f64;
    // Directions of exact length `len`, scales that keep them exact.
    const AXES: [([f64; 3], f64); 5] = [
        ([1.0, 2.0, 2.0], 3.0),
        ([2.0, -3.0, 6.0], 7.0),
        ([-4.0, 4.0, 7.0], 9.0),
        ([0.0, 3.0, -4.0], 5.0),
        ([2.0, 6.0, 9.0], 11.0),
    ];
    let scales = [
        -3.0,
        0.5,
        -f64::from_bits(1 << 14),
        f64::from_bits(1900 << 52),
    ];
    let (a, len) = AXES[(rng.next_u64() % AXES.len() as u64) as usize];
    let s = scales[(rng.next_u64() % scales.len() as u64) as usize];
    let a2 = a.map(|x| x * s);
    let p = [0; 3].map(|_| int(rng, 1000));
    let t = int(rng, 50);
    let moved = [0, 1, 2].map(|k| p[k] + t * a[k]);
    // `p + a × w` lies on the plane through `p` normal to `a`.
    let w = [0; 3].map(|_| int(rng, 7));
    let on_plane = [
        p[0] + a[1] * w[2] - a[2] * w[1],
        p[1] + a[2] * w[0] - a[0] * w[2],
        p[2] + a[0] * w[1] - a[1] * w[0],
    ];
    let r = 1.0 + int(rng, 40).abs() / 8.0;
    let (h, g) = (1.0 + int(rng, 9).abs(), 1.0 + int(rng, 9).abs());
    let pos = s.abs();
    let pt = |rng: &mut Rng| [0; 3].map(|_| int(rng, 100));
    let x: Vec<[f64; 3]> = (0..5).map(|_| pt(rng)).collect();
    let lift = |x: [f64; 3], w: f64| [x[0] + t * a[0], x[1] + t * a[1], x[2] + t * a[2], w];
    let at = |x: [f64; 3], w: f64| [x[0], x[1], x[2], w];
    let chain = [
        [at(x[0], 1.0), at(x[1], 0.5), at(x[2], 1.0)],
        [at(x[2], 2.0), at(x[3], 1.0), at(x[4], 1.0)],
    ];
    let back = [
        [lift(x[4], 1.0), lift(x[3], 1.0), lift(x[2], 2.0)],
        [lift(x[2], 1.0), lift(x[1], 0.5), lift(x[0], 1.0)],
    ];
    let m = |r: f64, z: f64, w: f64| [r, z, w];
    let z = [0; 3].map(|_| int(rng, 100));
    let rr = [0; 3].map(|_| 1.0 + int(rng, 100).abs());
    let fwd = [[
        m(rr[0], z[0], 1.0),
        m(rr[1], z[1], 0.5),
        m(rr[2], z[2], 1.0),
    ]];
    let z2 = |z: f64| t * len - z;
    let rev = [[
        m(rr[2], z2(z[2]), 1.0),
        m(rr[1], z2(z[1]), 0.5),
        m(rr[0], z2(z[0]), 1.0),
    ]];
    let lin: [[f64; 3]; 3] = std::array::from_fn(|i| {
        std::array::from_fn(|j| int(rng, 4) / 4.0 + if i == j { 2.0 } else { 0.0 })
    });
    let shift = pt(rng);
    let k = |x: Result<FormKey, FormError>| x.unwrap();
    let cyl = |q, d| k(CylinderKey::new(q, d, r).map(Into::into));
    let sphere = |d| k(SphereKey::new(p, r, d).map(Into::into));
    vec![
        (
            "plane",
            k(PlaneKey::new(a, p).map(Into::into)),
            k(PlaneKey::new(a2, on_plane).map(Into::into)),
        ),
        ("cylinder", cyl(p, a), cyl(moved, a2)),
        (
            "conic chain",
            k(ConicCylinderKey::new(a, &chain).map(Into::into)),
            k(ConicCylinderKey::new(a2, &back).map(Into::into)),
        ),
        (
            "cone",
            k(ConeKey::new(p, a, h, g).map(Into::into)),
            k(ConeKey::new(p, a.map(|x| x * pos), h * 0.125, g * 0.125).map(Into::into)),
        ),
        ("sphere", sphere(a), sphere(a2)),
        (
            "ring torus",
            k(TorusKey::new(p, a, 3.0 * r, r).map(Into::into)),
            k(TorusKey::new(p, a2, 3.0 * r, r).map(Into::into)),
        ),
        (
            "spindle torus",
            k(TorusKey::new(p, a, 2.0 * r, 3.0 * r).map(Into::into)),
            k(TorusKey::new(p, a2, 2.0 * r, 3.0 * r).map(Into::into)),
        ),
        (
            "revolved",
            k(RevolvedKey::new(p, a, &fwd).map(Into::into)),
            k(RevolvedKey::new(moved, a.map(|x| -x), &rev).map(Into::into)),
        ),
        (
            "affine cylinder",
            k(AffineKey::new(cyl(p, a), lin, shift).map(Into::into)),
            k(AffineKey::new(cyl(moved, a2), lin, shift).map(Into::into)),
        ),
    ]
}

/// Equal keys from other construction frames give bit-identical
/// lattices: the same frame, cap and profile, and the same tiles, with
/// their singular sides, at the base level and at random finer levels up
/// to the cap.
#[test]
fn equal_keys_from_other_frames_give_the_same_lattice() {
    let mut compared = 0;
    for seed in varde_testing::seeds(8, 100) {
        let mut rng = Rng::new(3000 + seed);
        for (name, ka, kb) in other_frames(&mut rng) {
            let what = format!("seed {seed}, {name}");
            assert_eq!(ka, kb, "{what}");
            let (a, b) = (Lattice::new(ka).unwrap(), Lattice::new(kb).unwrap());
            let bits = |l: &Lattice| {
                let (o, x) = (l.origin(), l.axes());
                (o.map(f64::to_bits), x.map(|e| e.map(f64::to_bits)))
            };
            assert_eq!(bits(&a), bits(&b), "{what}: frame");
            assert_eq!(a.cap(), b.cap(), "{what}: cap");
            assert_eq!(a.profile(), b.profile(), "{what}: profile");
            let rect = a.half(0).unwrap();
            assert_eq!(a.tiles(&rect).unwrap(), b.tiles(&rect).unwrap(), "{what}");
            let cap = a.cap_of(&rect).unwrap();
            for level in levels(&rect, cap) {
                for (i, j) in tiles_at(&rect, level, 2, &mut rng) {
                    let (ta, tb) = (a.tile(level, i, j), b.tile(level, i, j));
                    assert_eq!(ta, tb, "{what}: {level:?} ({i}, {j})");
                    assert_eq!(
                        ta.unwrap().singular(),
                        tb.unwrap().singular(),
                        "{what}: {level:?} ({i}, {j})"
                    );
                    compared += 1;
                }
            }
        }
    }
    println!("{compared} tiles compared, all the same bits");
}

/// The rectangle `r` grown by `du` cells at the start and end of `u`
/// and by `dv` in `v`.
fn grown(r: &Rect, du: [i64; 2], dv: [i64; 2]) -> Rect {
    Rect {
        level: r.level,
        u: r.u.start - du[0]..r.u.end + du[1],
        v: r.v.start - dv[0]..r.v.end + dv[1],
    }
}

/// A curve along tile line `v = j` of `s` over every tile of its
/// rectangle in `u`, with its p-curve: the tiles' rows as the curve's
/// spans (knots `0, 1, …`) and the line's Q spans in global UV, weights
/// 1, so that `S∘p` is the curve to rounding. `None` where a row is one
/// point (a pole or an apex).
fn row_curve(s: &Surface, j: i64) -> Option<(Curve3D, PCurve)> {
    let r = s.rect();
    let (mut spans, mut pspans) = (Vec::new(), Vec::new());
    let at = j as f64;
    for i in r.u.clone() {
        let row = s.row(Dir::U, j, i)?;
        if row[0][..3] == row[1][..3] && row[1][..3] == row[2][..3] {
            return None;
        }
        spans.push(row);
        let u = i as f64;
        pspans.push([[u, at, 1.0], [u + 0.5, at, 1.0], [u + 1.0, at, 1.0]]);
    }
    let knots: Vec<f64> = (0..=spans.len()).map(|k| k as f64).collect();
    Some((
        Curve3D::new(spans, knots.clone()).ok()?,
        PCurve::new(pspans, knots).ok()?,
    ))
}

/// `extend` keeps every tile of the surface bit for bit, at its own
/// level, for every form, so a p-curve on the old surface is one on the
/// new: its spans lie in the same tiles, and its seam bound, read off
/// those tiles, is the same bits. Rectangles grown on every side in turn
/// (cells off the lattice, past a profile or a full turn refused), from
/// the canonical surface and from one two levels finer. The p-curves run
/// along a tile line inside the rectangle: one on its side is in the
/// tiles on both sides of that line once it is extended across it, and
/// the seam bound may then read the other one (the same row, bit for bit,
/// so the bound holds, but not the same bits).
#[test]
fn extending_keeps_tiles_and_the_p_curves_on_them() {
    let grows: [([i64; 2], [i64; 2]); 5] = [
        ([1, 0], [0, 0]),
        ([0, 1], [0, 0]),
        ([0, 0], [1, 0]),
        ([0, 0], [0, 1]),
        ([1, 1], [1, 1]),
    ];
    let (mut extended, mut carried, mut worst) = (0, 0, 0.0f64);
    for seed in varde_testing::seeds(1, 6) {
        for Sample { name, surface } in surfaces(seed) {
            let rect = surface.rect().clone();
            let mut made = 0;
            for (du, dv) in grows {
                let what = format!("seed {seed}, {name}, grown {du:?} {dv:?}");
                let big = match surface.extend(&grown(&rect, du, dv)) {
                    Ok(big) => big,
                    Err(e) => {
                        assert!(
                            matches!(e, LatticeError::Index | LatticeError::Span),
                            "{what}: {e:?}"
                        );
                        continue;
                    }
                };
                made += 1;
                assert_eq!(big.rect().level, rect.level, "{what}");
                for (k, t) in surface.tiles().iter().enumerate() {
                    let (i, j) = surface.indices(k).unwrap();
                    assert_eq!(big.tile(i, j), Some(t), "{what}: ({i}, {j})");
                }
                // A tile line inside the rectangle, with its tiles on
                // both sides in it.
                let Some((c, p)) =
                    (rect.v.start + 1..rect.v.end).find_map(|j| row_curve(&surface, j))
                else {
                    continue;
                };
                let unit = surface.tiles().iter().map(scale).fold(0.0, larger) * f64::EPSILON;
                let small = seam_bound_alone(&surface, &c, &p, 64.0 * unit, MAX_WORK);
                let large = seam_bound_alone(&big, &c, &p, 64.0 * unit, MAX_WORK);
                assert_eq!(small, large, "{what}");
                let b = small.expect(&what);
                worst = larger(worst, b.bound / unit);
                carried += 1;
            }
            assert!(made >= 1, "seed {seed}, {name}: no extension");
            extended += made;
        }
    }
    println!(
        "{extended} extensions keep every tile; {carried} row p-curves carried over, \
         seam bounds the same bits, worst {worst:.1} units of the last bit"
    );
}
