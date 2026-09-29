//! Gauss–Legendre quadrature, for areas and volumes.
//!
//! The nodes and weights are written out rather than computed, so no
//! platform's `cos` decides them and a measure is the same to the bit
//! natively and on the web.

/// The 8-point Gauss–Legendre rule on `[-1, 1]`: the positive nodes and
/// their weights. It integrates polynomials up to degree 15 exactly.
const NODES: [f64; 4] = [
    0.183_434_642_495_649_8,
    0.525_532_409_916_329,
    0.796_666_477_413_626_7,
    0.960_289_856_497_536_3,
];
const WEIGHTS: [f64; 4] = [
    0.362_683_783_378_362,
    0.313_706_645_877_887_3,
    0.222_381_034_453_374_5,
    0.101_228_536_290_376_3,
];

/// The 8-point Gauss–Legendre rule on `[0, 1]`, as `(node, weight)` in
/// ascending order of the nodes. The weights sum to 1.
pub(crate) const GAUSS8: [(f64, f64); 8] = {
    let mut out = [(0.0, 0.0); 8];
    let mut i = 0;
    while i < 4 {
        let (x, w) = (NODES[3 - i], WEIGHTS[3 - i] * 0.5);
        out[i] = ((1.0 - x) * 0.5, w);
        out[7 - i] = ((1.0 + x) * 0.5, w);
        i += 1;
    }
    out
};

/// Points and weights for integrating over the parameter triangle of a
/// patch, `u0, u1 ≥ 0` with `u0 + u1 ≤ 1` (area ½), as barycentric
/// points `(u0, u1, 1 - u0 - u1)`: the triangle is cut into its four
/// halves-of-edges pieces, and each gets the 8 × 8 product rule through
/// the collapsed square (`u0 = s`, `u1 = (1 - s)·t`, Jacobian `1 - s`).
/// Integrands with no singularity in the triangle, such as a patch's
/// normal and position, converge fast.
pub(crate) fn triangle_rule() -> impl Iterator<Item = (glam::DVec3, f64)> {
    use glam::DVec3;
    let half = |a: DVec3, b: DVec3| (a + b) * 0.5;
    let (c0, c1, c2) = (DVec3::X, DVec3::Y, DVec3::Z);
    let (m01, m12, m20) = (half(c0, c1), half(c1, c2), half(c2, c0));
    let pieces = [
        [c0, m01, m20],
        [m01, c1, m12],
        [m20, m12, c2],
        [m01, m12, m20],
    ];
    pieces.into_iter().flat_map(|[d0, d1, d2]| {
        GAUSS8.iter().flat_map(move |&(s, ws)| {
            GAUSS8.iter().map(move |&(t, wt)| {
                let (b0, b1) = (s, (1.0 - s) * t);
                let b2 = 1.0 - b0 - b1;
                // Each piece is a quarter of the triangle, and the rule sums to ½ over the collapsed square.
                (d0 * b0 + d1 * b1 + d2 * b2, ws * wt * (1.0 - s) * 0.25)
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rule_integrates_polynomials_exactly() {
        // ∫₀¹ x^k dx = 1 / (k + 1), up to degree 15.
        for k in 0..16 {
            let sum: f64 = GAUSS8.iter().map(|&(x, w)| w * x.powi(k)).sum();
            assert!((sum - 1.0 / f64::from(k + 1)).abs() < 1e-15, "{k}: {sum}");
        }
        assert!(GAUSS8.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn the_triangle_rule_integrates_monomials() {
        // ∫ u0^a u1^b over the triangle = a! b! / (a + b + 2)!.
        let fact = |n: u32| (1..=n).map(f64::from).product::<f64>();
        for a in 0..6 {
            for b in 0..6 {
                let sum: f64 = triangle_rule()
                    .map(|(u, w)| w * u.x.powi(a as i32) * u.y.powi(b as i32))
                    .sum();
                let exact = fact(a) * fact(b) / fact(a + b + 2);
                assert!((sum - exact).abs() < 1e-15, "{a} {b}: {sum} {exact}");
            }
        }
    }
}
