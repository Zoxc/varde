//! The revolved conic's tiles: a profile of rational quadratic spans in
//! the meridian half-plane, turned about the key's axis `a`. Cell `(k,
//! l)` is the angle `[k, k + 1]·90°` from `e₁` towards `e₂` (the
//! cylinder's angle cells) times the profile's span `l` (`0 ≤ l <` the
//! number of spans, in the key's canonical direction): the tensor
//! product of the angle cell's quarter circle and the span. Control
//! point `(i, j)` is `foot + ρ_j·d_i + z_j·a`, `d_i` the quarter circle's
//! control directions (its two radii and their corner, weights `1, √½,
//! 1`) and `(ρ_j, z_j)` the span's stored control points, with the
//! weight `w_j` beside a radius and `√½·w_j` beside the corner, `w_j` the
//! span's weights in standard form (ends 1, middle `ω`,
//! [`super::super::condition`]: the row's Möbius conditioning), each
//! rounded once from its exact value (NURBS Book A8.1, the surface of
//! revolution's net; a sphere's or torus's net is this one with their
//! meridians, OCCT's `Convert_{Sphere,Torus}ToBSplineSurface` the same
//! over a cell).
//!
//! **Joints and closed profiles.** Neighbouring spans share their end
//! point bit for bit (the key's chain) and their end weight 1, so
//! neighbouring tiles share their row bit for bit, whatever weights the
//! profile was given with. A surface over every span of a closed profile (one
//! whose last span ends at its first span's start: a revolved ellipse)
//! would meet itself, and is refused (`Span`). Angle cells `k` and `k +
//! 4` are the same tile.
//!
//! **The axis.** A span may meet the axis only where its distance from
//! it has a simple root, at an end ([`AxisContact::Regular`]; the key
//! refuses a span crossing the axis). That end's control points have
//! `ρ = 0`, so the tile's row there is one point to the bit at every
//! level: a **pole** where the span leaves the axis perpendicularly, an
//! **apex** where it leaves it obliquely, a singular side of the tile in
//! both cases, which the fold check divides out, as a sphere's pole or a
//! cone's apex. A span **tangent** to the axis, at an end or inside
//! (the meridian of a horn torus `R = r` at its cusp point, a conic
//! touching the axis), has a double root there: the surface has a
//! **cusp**, which no singular side covers. No tile holds a cusp point:
//! every cell of such a span is refused ([`LatticeError::Cusp`]); an op
//! that would need it reports its own `Feature` refusal, and nothing is
//! fitted in its place. Away from its cusp point such a surface is this
//! form all the same: a horn torus's meridian arcs that don't reach the
//! axis are spans of their own, and tile.
//!
//! **Orientation.** `x_u × x_v` is `ρ·(z′·r̂ − ρ′·a)`, `r̂` the unit
//! radius and `(ρ′, z′)` the span's tangent: the profile's direction
//! turned a quarter clockwise in the `(ρ, z)` half-plane (out of the
//! surface for a profile running up along `a` away from the axis).
//!
//! **Level cap.** Along the profile, its spans' least chord rate
//! ([`super::cyl::chord_rate`]; the meridian's chords are the surface's
//! along `v`). Around the axis, the least radius over the spans asked
//! for, each span's bounded below by `min N/max w` (`N` as
//! [`AxisContact`]'s, its least value on the span exactly); a span
//! reaching the axis takes its largest radius instead, as a sphere's
//! equator (towards the axis a cell's chord shrinks to 0, which the pole
//! or apex row's singular side, not the cap, accounts for).

use std::ops::Range;

use malachite::Rational;
use malachite::base::num::basic::traits::Zero;

use super::super::condition::middle_square;
use super::super::form::{AxisContact, RevolvedKey, Span2, axis_contact};
use super::super::lattice::{
    Cells, ExactFrame, Frame, Lattice, LatticeError, Level, Reach, Rect, Run, affine, arc, larger,
    pow2,
};
use super::super::round::{nearest, nearest_sqrt, q};
use super::super::surface::Surface;
use super::super::tile::Net;
use super::cyl::{chord_rate, profile_closed};

/// The base tile's net of cell `(k, l)` (module docs); `Index` for a
/// span index outside the profile, `Cusp` for a span tangent to the
/// axis.
pub(in crate::geom) fn net(
    f: &ExactFrame,
    key: &RevolvedKey,
    k: i64,
    l: i64,
) -> Result<Net, LatticeError> {
    let span = usize::try_from(l)
        .ok()
        .and_then(|l| key.profile().spans().get(l))
        .ok_or(LatticeError::Index)?;
    if axis_contact(span) == AxisContact::Cusp {
        return Err(LatticeError::Cusp);
    }
    let (o, [a0, a1, a2]) = (&f.origin, &f.axes);
    let around = arc(a0, a1, k);
    let half = Rational::from_signeds(1, 2);
    // The standard weights' squares: 1 at the ends, `ω²` in the middle.
    let one = Rational::from(1u32);
    let squares = [one.clone(), middle_square(span), one];
    Ok(std::array::from_fn(|n| {
        let (dir, _) = &around[n / 3];
        let [rho, z, _] = span[n % 3];
        let (rho, z) = (q(rho), q(z));
        let p = [0, 1, 2].map(|c| &o[c] + &rho * &dir[c] + &z * &a2[c]);
        let w2 = &squares[n % 3];
        let w = if n / 3 == 1 {
            nearest_sqrt(&(w2 * &half))
        } else {
            nearest_sqrt(w2)
        };
        affine(&p, w)
    }))
}

/// Whether the span cells `v` are the profile's.
pub(in crate::geom) fn valid(key: &RevolvedKey, v: &Cells) -> bool {
    *v.start() >= 0 && (*v.end() as u64) < key.profile().spans().len() as u64
}

/// Whether `nu` angle tiles at level `lu` make a full turn, or `nv` span
/// tiles at level `lv` every span of a closed profile (module docs).
pub(in crate::geom) fn full(key: &RevolvedKey, (nu, lu): (u64, u32), (nv, lv): (u64, u32)) -> bool {
    let spans = key.profile().spans().len() as u128;
    u128::from(nu) >= 4u128 << lu
        || (profile_closed(key.profile()) && u128::from(nv) >= spans << lv)
}

/// The base cells holding the coordinate box: any angle cell, every
/// span.
pub(in crate::geom) fn box_cells(key: &RevolvedKey) -> (Cells, Cells) {
    (0..=0, 0..=(key.profile().spans().len() as i64 - 1))
}

/// What the level cap reads of the span cells `v` (any angle cells),
/// the profile's spans `spans` in standard form: coordinates to the
/// spans' largest `|ρ|` around the axis and `|z|` along it; `u` around a
/// circle of the spans' least radius, `v` along the profile at their
/// least chord rate (module docs). Spans outside the profile are left
/// out.
pub(in crate::geom) fn reach(f: &Frame, spans: &[Span2], v: &Cells) -> Reach {
    let (o, [a0, a1, a2]) = (f.origin, f.axes);
    let from = usize::try_from(*v.start()).unwrap_or(0);
    let to = usize::try_from(*v.end()).map_or(0, |e| e.saturating_add(1).min(spans.len()));
    let least = |a: f64, b: f64| if b < a { b } else { a };
    let (mut rho, mut z) = (0.0, 0.0);
    let (mut turn, mut rate) = (f64::INFINITY, f64::INFINITY);
    for s in spans.get(from..to).unwrap_or(&[]) {
        let far = s.iter().map(|p| p[0].abs()).fold(0.0, larger);
        rho = larger(rho, far);
        z = s.iter().map(|p| p[1].abs()).fold(z, larger);
        let low = least_radius(s);
        turn = least(turn, if low > 0.0 { low } else { far });
        rate = least(rate, chord_rate(s));
    }
    let m = (0..3)
        .map(|c| o[c].abs() + rho * (a0[c].abs() + a1[c].abs()) + z * a2[c].abs())
        .fold(0.0, larger);
    let finite = |x: f64| if x.is_finite() { x } else { 0.0 };
    Reach {
        m,
        rho,
        t: z,
        runs: [Run::Turn(finite(turn)), Run::Span(finite(rate))],
    }
}

/// A lower bound on the span's distance from the axis, `min N/max w`
/// (module docs), rounded down; 0 where the span reaches the axis.
fn least_radius(span: &Span2) -> f64 {
    let [a, b, c] = span.map(|p| q(p[2]) * q(p[0]));
    let mut low = if a < c { a.clone() } else { c.clone() };
    if b < a && b < c {
        let inner = (&a * &c - &b * &b) / (&a - Rational::from(2u32) * &b + &c);
        if inner < low {
            low = inner;
        }
    }
    if low <= Rational::ZERO {
        return 0.0;
    }
    let high = span.iter().map(|p| p[2]).fold(0.0, larger);
    // One rounding of the quotient, and the margin.
    nearest(&(low / q(high))) * (1.0 - pow2(-40))
}

/// The canonical rectangle at the base level ([`Lattice::half`]): angle
/// cells `first` and `first + 1` times every span of the profile (a
/// closed profile's is refused by [`Lattice::tiles`]).
pub(in crate::geom) fn half(key: &RevolvedKey, first: i64, next: i64) -> Rect {
    Rect {
        level: Level::BASE,
        u: first..next,
        v: 0..key.profile().spans().len() as i64,
    }
}

/// The revolved conic's 180° surface of the angle cells `first` and
/// `first + 1` times the profile's spans `spans` (indices in the key's
/// canonical direction; module docs); `Index` for spans outside the
/// profile, `Span` for every span of a closed one, `Cusp` where a span
/// is tangent to the axis.
pub fn revolved(key: RevolvedKey, first: i64, spans: Range<i64>) -> Result<Surface, LatticeError> {
    let lattice = Lattice::new(key.into())?;
    let next = first.checked_add(2).ok_or(LatticeError::Index)?;
    let rect = Rect {
        level: Level::BASE,
        u: first..next,
        v: spans,
    };
    Surface::new(lattice, rect)
}

#[cfg(test)]
mod tests;
