//! Storage rounding, the floor, the operand checks, `ρ_op` and frames
//! against their definitions.

use malachite::Rational;
use varde_testkit::rng::Rng;

use super::*;
use crate::num::dd::Dd;

/// `2ᵏ` for `k` in the normal range.
fn p2(k: i32) -> f64 {
    f64::from_bits(((1023 + k) as u64) << 52)
}

#[test]
fn the_floor_is_64_spacings_of_the_magnitude() {
    assert_eq!(eps_min(1e6), Some(p2(-27)));
    assert_eq!(eps_min(-1e6), Some(p2(-27)));
    assert_eq!(eps_min(MAX_COORD), Some(EPS_MIN_MAX_COORD));
    assert_eq!(EPS_MIN_MAX_COORD, p2(-27));
    assert_eq!(eps_min(100.0), Some(p2(-40)));
    assert_eq!(eps_min(1.0), Some(p2(-46)));
    // At a power of two, its own (the upper) binade.
    assert_eq!(ulp(1024.0), Some(p2(10 - 52)));
    assert_eq!(eps_min(1024.0), Some(p2(-36)));
    assert_eq!(eps_min(1023.9), Some(p2(-37)));
    // Below the normal range the subnormal spacing.
    let tiny = 64.0 * f64::from_bits(1);
    assert_eq!(eps_min(0.0), Some(tiny));
    assert_eq!(eps_min(f64::MIN_POSITIVE / 4.0), Some(tiny));
    assert_eq!(eps_min(f64::MIN_POSITIVE), Some(tiny));
    assert_eq!(eps_min(f64::MAX), Some(p2(977)));
    assert_eq!(eps_min(f64::NAN), None);
    assert_eq!(eps_min(f64::INFINITY), None);
    // The whole tolerance range is above the floor at the largest
    // coordinate.
    assert!(*EPS_RANGE.start() > EPS_MIN_MAX_COORD);
}

#[test]
fn ulp_is_the_spacing_of_the_binade() {
    let mut rng = Rng::new(3);
    for _ in 0..varde_testing::pick(2_000, 50_000) {
        let x = rng.log_range(1e-300, 1e300);
        let s = ulp(x).unwrap();
        // The next float up is one spacing away, and `x` a multiple of it.
        assert_eq!(x.next_up() - x, s, "{x}");
        assert_eq!((x / s).fract(), 0.0, "{x}");
    }
}

#[test]
fn rho_op_is_fifty_roundings_at_the_magnitude() {
    let r = rho_op(-1e6).unwrap();
    // 50·2⁻⁵³·10⁶ ≈ 5.55e-9, rounded upwards by a little.
    assert!(r >= 50.0 * 1e6 / 9_007_199_254_740_992.0);
    assert!(r < 5.6e-9, "{r}");
    assert_eq!(rho_op(1e6), Some(r));
    assert_eq!(rho_op(f64::NAN), None);
    assert_eq!(rho_op(f64::INFINITY), None);
    // The bands read the same bits.
    let b = crate::records::near::OpBands::new(1e6).unwrap();
    assert_eq!(b.rho_op().to_bits(), r.to_bits());
}

#[test]
fn bounded_positions_are_range_checked_others_only_finite() {
    let mut scan = OperandScan::new();
    assert_eq!(scan.bounded(&[1.0, -MAX_COORD, 3.0]), Ok(()));
    assert_eq!(scan.max_coord(), MAX_COORD);
    let over = MAX_COORD.next_up();
    assert_eq!(
        scan.bounded(&[0.0, over, 0.0]),
        Err(Unrepresentable::OutOfRange {
            value: over,
            magnitude: over,
        })
    );
    // Lattice-scale values pass where they need only be finite.
    assert_eq!(scan.finite(&[4.0 * MAX_COORD, 1e300, -64.0]), Ok(()));
    match scan.finite(&[1.0, f64::NAN]) {
        Err(Unrepresentable::OutOfRange { value, .. }) => assert!(value.is_nan()),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        scan.finite(&[f64::NEG_INFINITY]),
        Err(Unrepresentable::OutOfRange {
            value: f64::NEG_INFINITY,
            magnitude: f64::INFINITY,
        })
    );
    // The failures leave the magnitude as it was.
    assert_eq!(scan.max_coord(), MAX_COORD);
}

#[test]
fn key_points_are_bounded_only_inside_the_box() {
    let body = [[-1.0, -1.0, -1.0], [2.0, 2.0, 2.0]];
    let mut scan = OperandScan::new();
    scan.bounded(&[2.0, 0.0, 0.0]).unwrap();
    // Outside the box, past the range: a lattice foot point, fine.
    assert_eq!(scan.key_point(&[3.0 * MAX_COORD, 0.0, 0.0], &body), Ok(()));
    assert_eq!(scan.max_coord(), 2.0);
    // Inside the box: a bounded position (never larger than the box's).
    assert_eq!(scan.key_point(&[0.5, 1.5, -1.0], &body), Ok(()));
    assert_eq!(scan.max_coord(), 2.0);
    assert!(scan.key_point(&[f64::INFINITY, 0.0, 0.0], &body).is_err());
}

#[test]
fn the_start_checks_the_tolerance_and_fixes_rho_op() {
    let mut scan = OperandScan::new();
    scan.bounded(&[1e6, 0.0, -5.0]).unwrap();
    let start = scan.start(1e-3).unwrap();
    assert_eq!(start.max_coord, 1e6);
    assert_eq!(start.rho_op, rho_op(1e6).unwrap());
    assert_eq!(
        crate::records::near::OpBands::of(&start).rho_op(),
        start.rho_op
    );
    // Every tolerance of the range passes, and the floor itself.
    for eps in [*EPS_RANGE.start(), *EPS_RANGE.end(), EPS_MIN_MAX_COORD] {
        assert!(scan.start(eps).is_ok(), "{eps}");
    }
    let below = EPS_MIN_MAX_COORD.next_down();
    assert_eq!(
        scan.start(below),
        Err(Unrepresentable::Tolerance {
            eps: below,
            eps_min: EPS_MIN_MAX_COORD,
        })
    );
    assert!(matches!(
        scan.start(f64::NAN),
        Err(Unrepresentable::Tolerance { .. })
    ));
    // An operation on empty operands: `M = 0`.
    assert_eq!(OperandScan::new().start(1e-3).unwrap().max_coord, 0.0);
}

#[test]
fn exact_values_round_once_with_their_exact_error() {
    let mut rng = Rng::new(11);
    for _ in 0..varde_testing::pick(1_000, 20_000) {
        let a = rat(rng.log_range(1e-200, 1e200));
        let b = rat(rng.log_range(1e-20, 1e20));
        let x = &a / &b;
        let s = store_exact(&x).unwrap();
        assert_eq!(s.v, nearest(&x));
        // The error bounds the distance and is within a spacing of it.
        let d = (&x - rat(s.v)).abs();
        assert!(rat(s.err) >= d, "{x}");
        assert!(s.err <= ulp(s.v).unwrap(), "{x}");
    }
    // A float stores as itself, exactly.
    let s = store_exact(&rat(0.1)).unwrap();
    assert_eq!((s.v, s.err), (0.1, 0.0));
    // A tie goes to even: 1 + 2⁻⁵³ lies halfway between 1 and its
    // neighbour.
    let tie = Rational::from(1u32) + rat(p2(-53));
    assert_eq!(store_exact(&tie).unwrap().v, 1.0);
    // Past the range: out of range.
    let huge = rat(f64::MAX) * Rational::from(2u32);
    assert!(matches!(
        store_exact(&huge),
        Err(Unrepresentable::OutOfRange { .. })
    ));
}

#[test]
fn level_values_round_to_nearest_and_keep_their_error() {
    // A double-double value `1 + 2⁻⁶⁰`: stored as 1, its low part the
    // error.
    let dd = Bounded::exact(Dd::sum(1.0, p2(-60)));
    let s = store(&dd).unwrap();
    assert_eq!(s.v, 1.0);
    assert!(s.err >= p2(-60) && s.err < p2(-59), "{}", s.err);
    // A first-level value keeps its error, a little rounded up.
    let b = Bounded::new(3.0, 1e-9, 2e-9);
    let s = store(&b).unwrap();
    assert_eq!(s.v, 3.0);
    assert!(s.err >= 3e-9 && s.err < 3.1e-9, "{}", s.err);
    // Exact stays exact.
    assert_eq!(store(&Bounded::exact(2.5)).unwrap().err, 0.0);
    // An uncertified value isn't storable.
    assert!(store(&Bounded::new(1.0, 0.0, f64::INFINITY)).is_err());
}

#[test]
fn points_store_with_a_euclidean_displacement() {
    let p = [
        Bounded::exact(Dd::sum(1.0, p2(-60))),
        Bounded::exact(Dd::sum(-2.0, -p2(-60))),
        Bounded::exact(Dd::from_f64(5.0)),
    ];
    let s = store_point(&p, true).unwrap();
    assert_eq!(s.p, [1.0, -2.0, 5.0]);
    let want = p2(-60) * 2f64.sqrt();
    assert!(s.displacement >= want && s.displacement < want * 1.01);
    let far = [
        Bounded::exact(Dd::from_f64(2.0 * MAX_COORD)),
        Bounded::exact(Dd::ZERO),
        Bounded::exact(Dd::ZERO),
    ];
    // A bounded position past the range is refused; a tile's point isn't.
    assert!(store_point(&far, true).is_err());
    assert_eq!(store_point(&far, false).unwrap().displacement, 0.0);
}

#[test]
fn sub_floor_targets_keep_a_finite_achieved_bound() {
    // At 10⁶ the floor is 2⁻²⁷: a target below it is rounded, not
    // refused, and keeps the target plus the rounding.
    let rounding = p2(-34);
    let a = achieved(1e-9, 1e6, rounding).unwrap();
    assert!(a.below_floor);
    assert!(a.bound.is_finite() && a.bound >= 1e-9 + rounding);
    assert_eq!((a.target, a.magnitude), (1e-9, 1e6));
    // Above the floor: the same rule, no warning.
    let b = achieved(2.5e-6, -1e6, rounding).unwrap();
    assert!(!b.below_floor);
    assert!(b.bound >= 2.5e-6 + rounding);
    assert_eq!(b.magnitude, 1e6);
    // No rounding: the target itself.
    assert_eq!(achieved(1e-9, 1.0, 0.0).unwrap().bound, 1e-9);
    // Refused inputs.
    assert_eq!(achieved(-1.0, 1.0, 0.0), None);
    assert_eq!(achieved(1.0, f64::NAN, 0.0), None);
    assert_eq!(achieved(1.0, 1.0, f64::INFINITY), None);
}

#[test]
fn orders_compare_item_by_item() {
    assert_eq!(
        compare_orders(&[1, 2, 3], &[1, 2, 3]),
        OrderCheck::Preserved
    );
    assert_eq!(
        compare_orders(&[1, 2, 3], &[1, 3, 2]),
        OrderCheck::Mismatch { at: 1 }
    );
    assert_eq!(
        compare_orders(&[1, 2, 3], &[1, 2]),
        OrderCheck::Mismatch { at: 2 }
    );
    assert_eq!(compare_orders::<u8>(&[], &[]), OrderCheck::Preserved);
}

#[test]
fn frame_centres_lie_on_a_power_of_two_grid_inside_the_box() {
    let mut rng = Rng::new(5);
    for _ in 0..varde_testing::pick(2_000, 50_000) {
        let mid = rng.range(-1e6, 1e6);
        let w = rng.log_range(1e-6, 1e6);
        let (lo, hi) = (mid - w, mid + w);
        let c = frame_centre(lo, hi);
        assert!(lo <= c && c <= hi, "{lo} {hi} {c}");
        // Near the centre: within an eighth of the width plus a rounding.
        let m = lo * 0.5 + hi * 0.5;
        assert!(
            (c - m).abs() <= (hi - lo) / 8.0 + ulp(m).unwrap(),
            "{lo} {hi}"
        );
        // On the grid of the greatest power of two at most a quarter of
        // the width.
        let g = pow2_floor((hi * 0.5 - lo * 0.5) * 0.5).unwrap();
        assert_eq!((c / g).fract(), 0.0, "{lo} {hi} {c}");
        // The translation's error is its exact rounding, at most u·|x − c|.
        let x = rng.range(lo, hi);
        let t = translate(x, c);
        let exact = rat(x) - rat(c);
        assert!(rat(t.err()) >= (&exact - rat(t.v)).abs());
        assert!(t.err() <= U * t.v.abs());
    }
    // Small cases: the centre 1 of `[−3, 5]` on the grid of 2.
    assert_eq!(frame_centre(-3.0, 5.0), 2.0);
    assert_eq!(frame_centre(1.0, 1.0), 1.0);
    assert_eq!(frame_centre(f64::NEG_INFINITY, 1.0), 0.0);
    assert_eq!(
        frame_of(&[0.0, -8.0, 7.0], &[4.0, 8.0, 7.0]),
        [2.0, 0.0, 7.0]
    );
}

#[test]
fn translation_near_the_centre_is_exact() {
    // Within a factor 2 of the centre (Sterbenz), and at a zero centre.
    for (x, c) in [(1e6 + 0.1, 1e6), (3.3, 2.0), (0.7, 0.0), (-5.25, -4.0)] {
        let t = translate(x, c);
        assert_eq!(t.err(), 0.0, "{x} {c}");
        assert_eq!(rat(t.v), rat(x) - rat(c));
    }
    assert!(!translate(f64::MAX, -f64::MAX).is_certified());
}

#[test]
fn scans_merge_to_the_greater_magnitude() {
    let (mut a, mut b) = (OperandScan::new(), OperandScan::new());
    a.bounded(&[3.0, -7.0, 0.0]).unwrap();
    b.bounded(&[0.0, 5.0, 0.0]).unwrap();
    let mut ab = a;
    ab.merge(&b);
    assert_eq!(ab.max_coord(), 7.0);
    b.merge(&a);
    assert_eq!(b.max_coord(), 7.0);
    // As one scan visiting both.
    let mut one = OperandScan::new();
    one.bounded(&[3.0, -7.0, 0.0]).unwrap();
    one.bounded(&[0.0, 5.0, 0.0]).unwrap();
    assert_eq!(one, ab);
}

/// A scan's units are its positions' and nets' parts summed and rounded
/// up once: nothing for an empty scan, a whole unit for one position,
/// saturating past any budget.
#[test]
fn a_scan_charges_its_parts_rounded_up_once() {
    use crate::num::schedule::{UNIT_PARTS, UNITS};
    let u = UNITS.operand_scan;
    let units = |positions, nets| ScanSize { positions, nets }.units();
    assert_eq!(units(0, 0), 0);
    assert_eq!(units(1, 0), 1);
    assert_eq!(units(0, 1), 1);
    let (p, n) = (1000, 37);
    assert_eq!(
        units(p, n),
        (p * u.position + n * u.net).div_ceil(UNIT_PARTS)
    );
    assert!(units(p, n) <= units(p, 0) + units(0, n));
    assert_eq!(units(u64::MAX, u64::MAX), u64::MAX.div_ceil(UNIT_PARTS));
}
