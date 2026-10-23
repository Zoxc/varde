//! The torus's tiles, ring (`R > r`) and spindle (`R < r`), `R` the
//! major and `r` the minor radius: cell `(k, l)` is the angle `[k, k +
//! 1]·90°` around the axis `a` from `e₁` towards `e₂` times the tube
//! arc `l`, the tube angle `φ` measured in the meridian from the outer
//! equator towards `a`. The point at `(θ, φ)` is `centre + (R + r·cos
//! φ)·(cos θ·e₁ + sin θ·e₂) + r·sin φ·a`, and control point `(i, j)` is
//! `centre + ρ_j·d_i + z_j·a`, `d_i` the angle cell's quarter circle's
//! control directions (its two radii and their corner, weights `1, √½,
//! 1`) and `(ρ_j, z_j)` with weight `w_j` the tube arc's control points
//! in the meridian, the weight the product of the two (OCCT's
//! `Convert_TorusToBSplineSurface` has the same net over a cell).
//!
//! **Ring torus.** The tube arcs are 90° cells `φ ∈ [l, l + 1]·90°`:
//! control points `(R + r·c, r·s)` for the cell's two unit radii `(c,
//! s)` and their sum, weights `1, √½, 1`. Arcs `l` and `l + 4` are the
//! same; cells −1 and 0 are the tube's outer half (`ρ ≥ R`), 1 and 2
//! its inner half. The whole tube is regular and injective.
//!
//! **Spindle torus.** Only the outer part ("apple"), `φ ∈ [−φ*, φ*]`
//! with `cos φ* = −R/r`, is the form's domain: it reaches the axis at
//! the two apexes `centre ± √(r² − R²)·a` and never enters the inner
//! "lemon" (`|φ| > φ*`, the tube's part across the axis). Its tube
//! arcs are the apple's two halves from the outer equator, cell −1
//! `[−φ*, 0]` and cell 0 `[0, φ*]`, each one rational quadratic arc of
//! angle `φ* ∈ (90°, 180°)`: from the equator `(R + r, 0)` over the
//! tangents' corner `(R + r, r·√(r² − R²)/(r − R))` to the apex `(0,
//! √(r² − R²))` (mirrored in `z` for cell −1), the middle weight
//! `cos(φ*/2) = √((r − R)/(2r))`. No other tube cell exists (`Index`).
//! A finer level is the halves' exact sub-patches over dyadic parameter
//! cells, as on every lattice ([`Lattice::tile`]), anchored at the
//! apexes and the equator, with no cosine of a fraction of `φ*`: at
//! level 1 the halves' middles (their end weights are equal, so ½ is
//! the middle), four equal arcs of `φ*/2`; below, node `t` of a half
//! lies at `φ*/2 + 2·atan((2t − 1)·tan(φ*/4))` from its equator end, so
//! the arcs are not of equal angle (the sub-arcs' end weights differ).
//! The canonical surface is at level `(0, 1)`: four tube arcs of `φ*/2
//! < 90°`, as short as every other form's cells ([`Lattice::half`]).
//!
//! **The apex rows.** Cell `(k, 0)`'s last `v` row and cell `(k, −1)`'s
//! first are `ρ = 0`: their three control points are the apex, to the
//! bit, at every level: singular sides of the tile, which the fold
//! check divides out, as a cone's apex row.
//!
//! **Rounding.** A ring's control points are rational in the key's and
//! the frame's values, rounded once as every lattice's. A spindle's
//! coordinates are `A + B·√(r² − R²)`, `A` and `B` rational: the square
//! root is enclosed at doubling precision until both ends of the
//! value's enclosure round alike (the value is irrational unless the
//! root is rational, then it is rounded exactly), so it is rounded once
//! to the nearest `f64`, ties to even. Its middle weights are the
//! square roots of the exact products `½·(r − R)/(2r)` and `(r − R)/(2r)`,
//! rounded once. A tile's weights must lie in `[1/64, 64]`, so a spindle
//! with `R/r` above `1 − 2⁻¹⁰` (middle weight `√½·cos(φ*/2)` below
//! `1/64`) is refused by its tiles ([`LatticeError::Tile`]).
//!
//! The orientation `x_u × x_v` points out of the tube (away from its
//! centre circle).
//!
//! **Level cap.** Along the tube, a circle of radius `r` (a spindle's
//! cells, wider than 90°, have longer chords than the bound reads).
//! Around the axis, a ring's least radius over the cells asked for:
//! `R` on the outer half, `R − r` where an inner cell is asked for; a
//! spindle's outer equator `R + r`, as a sphere's equator (towards an
//! apex a cell's chord shrinks to 0, which the apex rows' singular
//! sides, not the cap, account for).

use malachite::base::num::arithmetic::traits::CheckedSqrt;
use malachite::base::rounding_modes::RoundingMode::{Ceiling, Floor};
use malachite::{Float, Rational};

use super::super::form::TorusKey;
use super::super::lattice::{
    Cells, ExactFrame, Frame, Lattice, LatticeError, Level, Reach, Rect, Run, arc, larger,
};
use super::super::round::{nearest, nearest_sqrt, q};
use super::super::surface::Surface;
use super::super::tile::Net;

/// Whether the torus is a spindle (`R < r`).
pub(in crate::geom) fn spindle(key: &TorusKey) -> bool {
    key.major() < key.minor()
}

/// One control point of a tube arc in the meridian: `ρ`, `z = z + root·√D`
/// (`D = r² − R²`) and the square of its weight.
struct Meridian {
    rho: Rational,
    z: Rational,
    root: Rational,
    w2: Rational,
}

/// The tube arc `l`'s control points (module docs); `Index` for a
/// spindle's cell outside the apple.
fn tube(key: &TorusKey, l: i64) -> Result<[Meridian; 3], LatticeError> {
    let (big, small) = (q(key.major()), q(key.minor()));
    let zero = || Rational::from(0u32);
    let one = || Rational::from(1u32);
    if spindle(key) {
        let outer = &big + &small;
        // The corner's `z` over `√D`: `r/(r − R)`.
        let corner = &small / (&small - &big);
        let w2 = (&small - &big) / (Rational::from(2u32) * &small);
        let point = |rho: &Rational, root: Rational, w2: Rational| Meridian {
            rho: rho.clone(),
            z: zero(),
            root,
            w2,
        };
        let equator = || point(&outer, zero(), one());
        return match l {
            -1 => Ok([
                point(&zero(), -one(), one()),
                point(&outer, -corner, w2),
                equator(),
            ]),
            0 => Ok([
                equator(),
                point(&outer, corner, w2),
                point(&zero(), one(), one()),
            ]),
            _ => Err(LatticeError::Index),
        };
    }
    let radii: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];
    let m = l.rem_euclid(4) as usize;
    let (a, b) = (radii[m], radii[(m + 1) % 4]);
    let corner = (a.0 + b.0, a.1 + b.1);
    let point = |(c, s): (i32, i32), w2: Rational| Meridian {
        rho: &big + &small * Rational::from(c),
        z: &small * Rational::from(s),
        root: zero(),
        w2,
    };
    Ok([
        point(a, one()),
        point(corner, Rational::from_signeds(1, 2)),
        point(b, one()),
    ])
}

/// The base tile's net of cell `(k, l)` (module docs); `Index` for a
/// spindle's tube cell other than −1 or 0.
pub(in crate::geom) fn net(
    f: &ExactFrame,
    key: &TorusKey,
    k: i64,
    l: i64,
) -> Result<Net, LatticeError> {
    let rows = tube(key, l)?;
    let (big, small) = (q(key.major()), q(key.minor()));
    let d = &small * &small - &big * &big;
    let (o, [a0, a1, a2]) = (&f.origin, &f.axes);
    let around = arc(a0, a1, k);
    // The squares of the quarter circle's weights.
    let around_w2 = [
        Rational::from(1u32),
        Rational::from_signeds(1, 2),
        Rational::from(1u32),
    ];
    Ok(std::array::from_fn(|n| {
        let (dir, _) = &around[n / 3];
        let row = &rows[n % 3];
        let p = [0, 1, 2].map(|c| {
            let a = &o[c] + &row.rho * &dir[c] + &row.z * &a2[c];
            nearest_plus_root(&a, &(&row.root * &a2[c]), &d)
        });
        let w = nearest_sqrt(&(&around_w2[n / 3] * &row.w2));
        [p[0], p[1], p[2], w]
    }))
}

/// `a + b·√d` rounded to the nearest `f64`, ties to even, for `d ≥ 0`
/// (module docs): exactly where it is rational, else from enclosures of
/// the root at doubling precision until both ends round alike (an
/// irrational value is never a midpoint, so that ends).
fn nearest_plus_root(a: &Rational, b: &Rational, d: &Rational) -> f64 {
    if *b == 0u32 {
        return nearest(a);
    }
    if let Some(s) = d.checked_sqrt() {
        return nearest(&(a + b * s));
    }
    let mut prec = 64u64;
    loop {
        let root = |mode| {
            let (f, _) = Float::sqrt_rational_prec_round_ref(d, prec, mode);
            Rational::try_from(&f).expect("the root of a positive rational is finite")
        };
        let (x, y) = (
            nearest(&(a + b * root(Floor))),
            nearest(&(a + b * root(Ceiling))),
        );
        if x.to_bits() == y.to_bits() {
            return x;
        }
        prec = prec.saturating_mul(2);
    }
}

/// Whether the tube cells `v` lie in the lattice: any arc of a ring,
/// a spindle's −1 and 0.
pub(in crate::geom) fn valid(key: &TorusKey, v: &Cells) -> bool {
    !spindle(key) || (*v.start() >= -1 && *v.end() <= 0)
}

/// Whether the ring's tube cells `v` all lie on its outer half (cells
/// −1 and 0, modulo 4).
fn outer_half(v: &Cells) -> bool {
    let outer = |l: i64| matches!(l.rem_euclid(4), 0 | 3);
    match v.end().abs_diff(*v.start()) {
        0 => outer(*v.start()),
        1 => v.start().rem_euclid(4) == 3,
        _ => false,
    }
}

/// What the level cap reads of the tube cells `v` (any angle cells):
/// coordinates to the outer equator's radius `R + r` around the axis
/// and the tube's highest control point along it; `u` around a circle
/// of the cells' least radius (a spindle's outer equator), `v` around
/// one of radius `r` (module docs).
pub(in crate::geom) fn reach(f: &Frame, key: &TorusKey, v: &Cells) -> Reach {
    let (o, [a0, a1, a2]) = (f.origin, f.axes);
    let (big, small) = (key.major(), key.minor());
    let outer = big + small;
    let (z, turn) = if spindle(key) {
        // The corner's `z`, `r·√((r + R)/(r − R))`.
        (small * ((small + big) / (small - big)).sqrt(), outer)
    } else if outer_half(v) {
        (small, big)
    } else {
        (small, big - small)
    };
    let m = (0..3)
        .map(|c| o[c].abs() + outer * (a0[c].abs() + a1[c].abs()) + z * a2[c].abs())
        .fold(0.0, larger);
    Reach {
        m,
        rho: outer + z,
        t: 0.0,
        runs: [Run::Turn(turn), Run::Turn(small)],
    }
}

/// The canonical rectangle (module docs, [`Lattice::half`]): angle
/// cells `first` and `first + 1` times the ring's tube cells −1 and 0
/// at the base level, or the spindle's apple at level `(0, 1)`.
pub(in crate::geom) fn half(key: &TorusKey, first: i64, next: i64) -> Rect {
    if spindle(key) {
        Rect {
            level: Level { u: 0, v: 1 },
            u: first..next,
            v: -2..2,
        }
    } else {
        Rect {
            level: Level::BASE,
            u: first..next,
            v: -1..1,
        }
    }
}

/// The torus's 180° surface of angle cells `first` and `first + 1`
/// times the ring's outer half (tube cells −1 and 0: from its lowest
/// circle over the outer equator to its highest) or the spindle's whole
/// apple, apex to apex, in four arcs (module docs).
pub fn torus(key: TorusKey, first: i64) -> Result<Surface, LatticeError> {
    let lattice = Lattice::new(key.into())?;
    let rect = lattice.half(first)?;
    Surface::new(lattice, rect)
}

/// The ring torus's 180° surface of angle cells `first` and `first + 1`
/// times its inner half (tube cells 1 and 2: from its highest circle
/// over the inner equator to its lowest), the other half of the tube
/// [`torus`] gives; `Index` for a spindle (its lemon is no part of the
/// form).
pub fn torus_inner(key: TorusKey, first: i64) -> Result<Surface, LatticeError> {
    let lattice = Lattice::new(key.into())?;
    let next = first.checked_add(2).ok_or(LatticeError::Index)?;
    let rect = Rect {
        level: Level::BASE,
        u: first..next,
        v: 1..3,
    };
    Surface::new(lattice, rect)
}

/// Whether `nv` tube tiles at level `lv` make a full turn of a ring's
/// tube (`4·2^lv` or more; a spindle's apple is no closed turn).
pub(in crate::geom) fn full_tube(key: &TorusKey, nv: u64, lv: u32) -> bool {
    !spindle(key) && u128::from(nv) >= 4u128 << lv
}

#[cfg(test)]
mod tests;
