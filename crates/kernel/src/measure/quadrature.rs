//! Gauss–Legendre quadrature, for areas and volumes: the 8-point rule
//! the facade measures profiles with, the 32-point rule with certified
//! nodes and weights the kernel's mass properties integrate with
//! ([`super::mass`]), and a triangle rule.
//!
//! The nodes and weights are written out rather than computed, so no
//! platform's `cos` decides them and a measure is the same to the bit
//! natively and on the web.

use varde_decide::num::bounded::{Bounded, up};

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
    0.313_706_645_877_887_27,
    0.222_381_034_453_374_48,
    0.101_228_536_290_376_26,
];

/// The 8-point Gauss–Legendre rule on `[0, 1]`, as `(node, weight)` in
/// ascending order of the nodes. The weights sum to 1.
pub const GAUSS8: [(f64, f64); 8] = {
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

/// The 32-point Gauss–Legendre rule on `[-1, 1]`: the positive nodes in
/// ascending order, each the `f64` nearest the true node, and their
/// weights likewise. It integrates polynomials up to degree 63 exactly.
const NODES32: [f64; 16] = [
    0.04830766568773832,
    0.1444719615827965,
    0.23928736225213706,
    0.33186860228212767,
    0.42135127613063533,
    0.5068999089322294,
    0.5877157572407623,
    0.6630442669302152,
    0.7321821187402897,
    0.7944837959679424,
    0.84936761373257,
    0.8963211557660521,
    0.9349060759377397,
    0.9647622555875064,
    0.9856115115452684,
    0.9972638618494816,
];
const WEIGHTS32: [f64; 16] = [
    0.0965400885147278,
    0.09563872007927486,
    0.09384439908080457,
    0.09117387869576389,
    0.08765209300440381,
    0.08331192422694675,
    0.07819389578707031,
    0.0723457941088485,
    0.06582222277636185,
    0.058684093478535544,
    0.050998059262376175,
    0.04283589802222668,
    0.03427386291302143,
    0.02539206530926206,
    0.01627439473090567,
    0.007018610009470096,
];

/// The 32-point Gauss–Legendre rule on `[0, 1]` with certified nodes and
/// weights, as `(node, weight)` in ascending order of the nodes: each
/// stored value is the nearest `f64` to the true one, so the true node
/// and weight lie within half an ulp of it, and the halving and the
/// reflection `(1 ± x)/2` carry their rounding. A sum `Σ wᵢ·f(xᵢ)` of a
/// function evaluated in [`Bounded`] arithmetic at these nodes encloses
/// the exact rule's sum, which is the integral of any polynomial of
/// degree up to 63.
pub(crate) fn gauss32_bounded() -> [(Bounded<f64>, Bounded<f64>); 32] {
    let half = Bounded::exact(0.5);
    let one = Bounded::exact(1.0);
    let within = |x: f64| Bounded::new(x, 0.0, up(half_ulp(x)));
    let mut out = [(Bounded::exact(0.0), Bounded::exact(0.0)); 32];
    for i in 0..16 {
        let (x, w) = (within(NODES32[15 - i]), within(WEIGHTS32[15 - i]) * half);
        out[i] = ((one - x) * half, w);
        out[31 - i] = ((one + x) * half, w);
    }
    out
}

/// Half the spacing of the `f64`s at the positive normal `x`: an upper
/// bound of the distance to the true value it is the nearest `f64` to.
fn half_ulp(x: f64) -> f64 {
    (x.next_up() - x) * 0.5
}

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
#[allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]
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
        assert_eq!(GAUSS8.iter().map(|&(_, w)| w).sum::<f64>(), 1.0);
    }

    #[test]
    fn the_nodes_and_weights_are_correctly_rounded() {
        // To 30 digits, from Newton's method on P8 in 60-digit decimals;
        // Rust parses a decimal to the nearest f64.
        let nodes = [
            "0.183434642495649804939476142360",
            "0.525532409916328985817739049189",
            "0.796666477413626739591553936476",
            "0.960289856497536231683560868569",
        ];
        let weights = [
            "0.362683783378361982965150449277",
            "0.313706645877887287337962201987",
            "0.222381034453374470544355994426",
            "0.101228536290376259152531354310",
        ];
        for i in 0..4 {
            let (x, w) = (nodes[i].parse::<f64>(), weights[i].parse::<f64>());
            assert_eq!(NODES[i].to_bits(), x.unwrap().to_bits(), "node {i}");
            assert_eq!(WEIGHTS[i].to_bits(), w.unwrap().to_bits(), "weight {i}");
        }
    }

    #[test]
    fn the_32_point_nodes_and_weights_are_correctly_rounded() {
        // To 40 digits, from Newton's method on P32 in 80-digit decimals.
        let nodes = [
            "0.0483076656877383162348125704405021636908",
            "0.1444719615827964934851863735988106522038",
            "0.2392873622521370745446032091655015206089",
            "0.3318686022821276497799168057301879961958",
            "0.4213512761306353453641194361724264783359",
            "0.5068999089322293900237474743778212301803",
            "0.5877157572407623290407454764018268584509",
            "0.6630442669302152009751151686632383689770",
            "0.7321821187402896803874266650912671466303",
            "0.7944837959679424069630972989704289020955",
            "0.8493676137325699701336930049677425389549",
            "0.8963211557660521239653072437192122684790",
            "0.9349060759377396891709191348354093255287",
            "0.9647622555875064307738119281182749603889",
            "0.9856115115452683354001750446309019786324",
            "0.9972638618494815635449811286650407271385",
        ];
        let weights = [
            "0.0965400885147278005667648300635757947369",
            "0.0956387200792748594190820022041311005949",
            "0.0938443990808045656391802376681172600361",
            "0.0911738786957638847128685771116370625449",
            "0.0876520930044038111427714627518022875484",
            "0.0833119242269467552221990746043486115387",
            "0.0781938957870703064717409188283066710398",
            "0.0723457941088485062253993564784877916043",
            "0.0658222227763618468376500637069387728775",
            "0.0586840934785355471452836373001708867501",
            "0.0509980592623761761961632446895216952602",
            "0.0428358980222266806568786466061255284928",
            "0.0342738629130214331026877322523727069948",
            "0.0253920653092620594557525897892240292876",
            "0.0162743947309056706051705622063866181795",
            "0.0070186100094700966004070637388531825134",
        ];
        for i in 0..16 {
            let (x, w) = (nodes[i].parse::<f64>(), weights[i].parse::<f64>());
            assert_eq!(NODES32[i].to_bits(), x.unwrap().to_bits(), "node {i}");
            assert_eq!(WEIGHTS32[i].to_bits(), w.unwrap().to_bits(), "weight {i}");
        }
    }

    #[test]
    fn the_32_point_rule_encloses_the_integrals_of_polynomials() {
        // ∫₀¹ x^k dx = 1 / (k + 1) up to degree 63, inside the enclosure of
        // the rule's sum, which is itself tight.
        let rule = gauss32_bounded();
        for k in 0..64 {
            let mut sum = Bounded::exact(0.0);
            for &(x, w) in &rule {
                let mut p = Bounded::exact(1.0);
                for _ in 0..k {
                    p = p * x;
                }
                sum = sum + w * p;
            }
            let exact = 1.0 / f64::from(k + 1);
            assert!((sum.v - exact).abs() <= sum.err() + 1e-17, "{k}: {sum:?}");
            assert!(sum.err() < 1e-14, "{k}: {sum:?}");
        }
        assert!(rule.windows(2).all(|w| w[0].0.v < w[1].0.v));
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
