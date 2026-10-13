//! Normal cones: a certified circular cone of directions holding every
//! non-negative combination of a vector form's coefficients.
//!
//! A vector form `N = (N_x, N_y, N_z)` in the Bernstein basis takes at
//! each point a non-negative combination of its coefficient vectors (the
//! basis is non-negative). A circular cone of half-angle `θ < 90°` is
//! convex, so if it holds every coefficient it holds every value, and the
//! values, all with a positive component along its axis, are never zero.
//! For a patch's normal numerator (`W³·(x_u × x_v)`, positive weights) it
//! holds every unit normal and certifies the patch regular.

use super::scalar::Scalar;
use super::tensor::{Dir, Tensor};
use crate::num::bounded::{add_up, down, mul_up, up};

/// A certified cone of directions: every direction whose angle to `axis`
/// is at most `θ`, `0 ≤ θ < 90°`.
///
/// `θ` is kept as bounds on its cosine and sine, so decisions with it use
/// correctly rounded arithmetic only (no `acos`): `cos` is at most
/// `cos θ` and positive, `sin` at least `sin θ` and at most 1. The axis's
/// length is near 1, not exactly 1: only its direction matters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NormalCone {
    /// The axis's direction.
    pub axis: [f64; 3],
    /// A positive lower bound of `cos θ`.
    pub cos: f64,
    /// An upper bound of `sin θ`, at most 1.
    pub sin: f64,
}

impl NormalCone {
    /// The cone about the normalised sum of the coefficients' directions
    /// (their enclosures' centres) that holds every coefficient's
    /// enclosure, so every value of `n` (module docs); `None` unless every
    /// enclosure lies certainly in the open half-space `axis·c > 0`. Any
    /// axis would be sound; this one only makes a tight cone likely.
    ///
    /// The bound: for `c` in a coefficient's box, `a·c ≥ d` (the box's
    /// least dot product, rounded down) and `|c| ≤ m` (rounded up), so
    /// `cos ∠(a, c) ≥ d / (|a|·m)` once `d > 0`; `cos` is the least of
    /// these and `sin = √(1 − cos²)`, rounded up.
    ///
    /// Components of different degrees are first raised to the highest
    /// degree in each direction, so that the coefficients with one index
    /// belong to one Bernstein basis function.
    pub fn of_form<S: Scalar>(n: &[Tensor<S>; 3]) -> Option<NormalCone> {
        let m = n.iter().map(|f| f.degree(Dir::U)).max().unwrap_or(0);
        let k = n.iter().map(|f| f.degree(Dir::V)).max().unwrap_or(0);
        let raised;
        let n = if n.iter().all(|f| f.degrees() == (m, k)) {
            n
        } else {
            raised = n.clone().map(|f| {
                f.elevate_to(m, k)
                    .expect("within the components' own degrees")
            });
            &raised
        };
        let boxes: Vec<[[f64; 2]; 3]> = (0..n[0].coefs().len())
            .map(|i| [0, 1, 2].map(|k| n[k].coefs()[i].enclose()))
            .collect();
        let axis = axis(&boxes)?;
        // `|a|`, rounded up.
        let a_len = up(axis
            .iter()
            .fold(0.0, |s, &a| add_up(s, mul_up(a.abs(), a.abs())))
            .sqrt());
        let mut cos = f64::INFINITY;
        for b in &boxes {
            // `a·c` rounded down and `|c|` rounded up over the box.
            let mut dot = 0.0;
            let mut len2 = 0.0;
            for (&a, &[lo, hi]) in axis.iter().zip(b) {
                let term = if a > 0.0 {
                    down(a * lo)
                } else if a < 0.0 {
                    down(a * hi)
                } else {
                    0.0
                };
                dot = down(dot + term);
                let m = if -lo > hi { -lo } else { hi };
                len2 = add_up(len2, mul_up(m, m));
            }
            if dot.is_nan() || dot <= 0.0 {
                return None;
            }
            let c = down(dot / mul_up(a_len, up(len2.sqrt())));
            if c < cos {
                cos = c;
            }
        }
        if cos.is_nan() || cos <= 0.0 {
            return None;
        }
        // `sin θ = √(1 − cos² θ) ≤ √(1 − cos²)` for `cos ≤ cos θ`.
        let sin = up(up(1.0 - down(cos * cos)).sqrt());
        Some(NormalCone {
            axis,
            cos,
            sin: if sin > 1.0 { 1.0 } else { sin },
        })
    }
}

/// The normalised sum of the unit directions of the boxes' centres (those
/// with a finite, non-zero direction), `None` if that sum has none.
fn axis(boxes: &[[[f64; 2]; 3]]) -> Option<[f64; 3]> {
    let mut s = [0.0; 3];
    for b in boxes {
        if let Some(d) = unit(b.map(|[lo, hi]| lo * 0.5 + hi * 0.5)) {
            for k in 0..3 {
                s[k] += d[k];
            }
        }
    }
    unit(s)
}

/// `v`'s direction, near unit length, scaled first so no square
/// overflows or underflows; `None` for a zero or non-finite `v`.
fn unit(v: [f64; 3]) -> Option<[f64; 3]> {
    let big = v
        .iter()
        .fold(0.0, |m, x| if x.abs() > m { x.abs() } else { m });
    if !(big > 0.0 && v.iter().all(|x| x.is_finite())) {
        return None;
    }
    let v = v.map(|x| x / big);
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    Some(v.map(|x| x / len))
}

#[cfg(test)]
mod tests;
