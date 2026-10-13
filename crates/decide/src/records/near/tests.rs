//! The bands against their definitions.

use super::*;

#[test]
fn the_bands_follow_the_magnitude_and_the_tolerance() {
    let b = OpBands::new(-1e6).expect("finite");
    let rho = b.rho_op();
    // 50·2⁻⁵³·10⁶ ≈ 5.55e-9, rounded upwards by a little.
    assert!(rho >= 50.0 * 1e6 / 9_007_199_254_740_992.0);
    assert!(rho < 5.6e-9, "{rho}");
    let tol = 2.5e-6;
    let beta = b.band(tol).expect("a tolerance");
    assert!(beta >= tol + rho && beta < (tol + rho) * (1.0 + 1e-15));
    let r = b.near_radius(tol).expect("a tolerance");
    assert!(r >= 3.0 * beta && r < 3.0 * beta * (1.0 + 1e-15));
    // Monotone in the tolerance and the magnitude.
    assert!(b.near_radius(2.0 * tol).unwrap() > r);
    assert!(OpBands::new(1e7).unwrap().near_radius(tol).unwrap() > r);
    // Refused inputs.
    assert_eq!(OpBands::new(f64::NAN), None);
    assert_eq!(OpBands::new(f64::INFINITY), None);
    assert_eq!(b.band(-1.0), None);
    assert_eq!(b.near_radius(f64::NAN), None);
    assert_eq!(b.near_radius(f64::MAX), None);
    // The same bits each time.
    assert_eq!(
        OpBands::new(1e6)
            .unwrap()
            .near_radius(tol)
            .map(f64::to_bits),
        b.near_radius(tol).map(f64::to_bits)
    );
}
