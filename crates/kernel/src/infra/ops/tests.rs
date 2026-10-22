use malachite::Rational;
use varde_decide::num::exact::rat;
use varde_decide::num::storage::{
    EPS_MIN_MAX_COORD, MAX_COORD, OpStart, check_eps, nearest, rho_op,
};

use super::*;

fn start(rho_op: f64) -> OpStart {
    OpStart {
        max_coord: 0.0,
        rho_op,
    }
}

fn warning(kind: WarningKind, bound: f64) -> Warning {
    Warning::new(kind, bound).expect("a finite bound")
}

#[test]
fn tolerance_range_and_default() {
    assert_eq!(Tolerance::default(), Tolerance::DEFAULT);
    assert_eq!(Tolerance::DEFAULT.eps(), 1e-3);
    assert_eq!(Tolerance::new(1e-3), Some(Tolerance::DEFAULT));
    assert_eq!((Tolerance::MIN, Tolerance::MAX), (1e-5, 1e-1));
    for eps in [Tolerance::MIN, 1e-4, 1e-2, Tolerance::MAX] {
        let t = Tolerance::new(eps).expect("in range");
        assert_eq!(t.eps(), eps);
        // Every tolerance in range is storable at the largest coordinate.
        assert_eq!(check_eps(t.eps()), Ok(()));
    }
    for eps in [
        Tolerance::MIN.next_down(),
        Tolerance::MAX.next_up(),
        EPS_MIN_MAX_COORD,
        0.0,
        -0.0,
        -1e-3,
        1.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        assert_eq!(Tolerance::new(eps), None, "{eps}");
    }
}

/// `3ε/4 + ρ_op` exactly, and the largest `f64` at most it.
fn contract(eps: f64, rho: f64) -> (Rational, f64) {
    let exact = rat(eps) * Rational::from_unsigneds(3u32, 4u32) + rat(rho);
    let mut d = nearest(&exact);
    if rat(d) > exact {
        d = d.next_down();
    }
    (exact, d)
}

#[test]
fn within_contract_at_three_quarters_eps_plus_rho_op() {
    let rho_big = rho_op(MAX_COORD).expect("finite");
    for (eps, rho) in [
        (1e-3, 0.0),
        (1e-3, rho_big),
        (Tolerance::MIN, rho_big),
        (Tolerance::MAX, rho_op(1.0).expect("finite")),
        (0.1 + 0.2 - 0.3 + 1e-3, 1e-11),
    ] {
        let tol = Tolerance::new(eps).expect("in range");
        let (exact, d) = contract(eps, rho);
        assert!(tol.within_contract(d, rho), "{eps} {rho}");
        assert!(!tol.within_contract(d.next_up(), rho), "{eps} {rho}");
        // The boundary is the exact sum, not a rounding of it.
        assert_eq!(tol.within_contract(d, rho), rat(d) <= exact);

        let mut status = Status::new(tol, &start(rho));
        assert!(status.within_contract());
        status.warn(warning(WarningKind::Accuracy, d));
        assert!(status.within_contract());
        assert!(status.approximate());
        status.warn(warning(WarningKind::StorageRounding, d.next_up()));
        assert!(!status.within_contract());
    }
    // An exactly representable boundary is within: `3·2⁻¹⁰/4 + 2⁻²⁰`.
    let eps = 1.0 / 1024.0;
    let rho = 1.0 / 1_048_576.0;
    let tol = Tolerance::new(eps).expect("in range");
    assert!(tol.within_contract(0.75 * eps + rho, rho));
    assert!(!tol.within_contract((0.75 * eps + rho).next_up(), rho));
    // Nothing infinite or not a number is within contract.
    assert!(!tol.within_contract(f64::INFINITY, rho));
    assert!(!tol.within_contract(f64::NAN, rho));
    assert!(!tol.within_contract(0.0, f64::NAN));
}

#[test]
fn no_warning_is_exact_and_within_contract() {
    let status = Status::new(Tolerance::DEFAULT, &start(1e-10));
    assert!(!status.approximate());
    assert_eq!(status.max_deviation(), 0.0);
    assert_eq!(status.warning_count(), 0);
    assert!(status.warnings().is_empty());
    assert!(status.within_contract());
    assert_eq!(status.eps(), Tolerance::DEFAULT);
    assert_eq!(status.rho_op(), 1e-10);
}

#[test]
fn flag_and_deviation_are_exact_past_the_listed_warnings() {
    let tol = Tolerance::DEFAULT;
    let rho = 1e-10;
    let kinds = [
        WarningKind::Accuracy,
        WarningKind::StorageRounding,
        WarningKind::Repair(Repair::Reorder),
        WarningKind::Repair(Repair::Bend),
        WarningKind::Repair(Repair::Collapse),
    ];
    let mut status = Status::new(tol, &start(rho));
    let n = MAX_WARNINGS + 36;
    for i in 0..n {
        // Small bounds while listed; past the list one beyond the
        // contract, then smaller ones again.
        let bound = if i == MAX_WARNINGS + 10 {
            2.0 * tol.eps()
        } else {
            1e-9 * i as f64
        };
        status.warn(warning(kinds[i % kinds.len()], bound));
    }
    assert_eq!(status.warnings().len(), MAX_WARNINGS);
    assert_eq!(status.warning_count(), n as u64);
    assert!(status.approximate());
    assert_eq!(status.max_deviation(), 2.0 * tol.eps());
    assert!(!status.within_contract());
    // The listed ones are the first added, in order.
    for (i, w) in status.warnings().iter().enumerate() {
        assert_eq!(w.kind(), kinds[i % kinds.len()]);
        assert_eq!(w.bound(), 1e-9 * i as f64);
    }

    // A single unlisted warning past a full list still flags the result.
    let mut status = Status::new(tol, &start(rho));
    for _ in 0..MAX_WARNINGS {
        status.warn(warning(WarningKind::Repair(Repair::Bend), 0.0));
    }
    assert_eq!(status.max_deviation(), 0.0);
    assert!(status.within_contract());
    status.warn(warning(WarningKind::Repair(Repair::Collapse), 1e-7));
    assert_eq!(status.warnings().len(), MAX_WARNINGS);
    assert_eq!(status.warning_count(), MAX_WARNINGS as u64 + 1);
    assert_eq!(status.max_deviation(), 1e-7);
}

#[test]
fn every_warning_bound_is_finite() {
    for bound in [f64::INFINITY, f64::NAN, -1e-9, f64::NEG_INFINITY] {
        assert_eq!(Warning::new(WarningKind::Accuracy, bound), None, "{bound}");
    }
    let w = warning(WarningKind::StorageRounding, -0.0);
    assert!(w.bound().is_sign_positive());
    let w = warning(WarningKind::Repair(Repair::Reorder), f64::MAX);
    assert_eq!(
        (w.kind(), w.bound()),
        (WarningKind::Repair(Repair::Reorder), f64::MAX)
    );
    // A finite bound past the contract is a warning like any other.
    let mut status = Status::new(Tolerance::DEFAULT, &start(0.0));
    status.warn(w);
    assert_eq!(status.max_deviation(), f64::MAX);
    assert!(!status.within_contract());
}

#[test]
fn contexts_order_by_feature_then_instance() {
    let ctx = |f, i| OpCtx {
        feature: FeatureId(f),
        instance: InstanceId(i),
    };
    let mut v = vec![ctx(2, 0), ctx(1, 5), ctx(1, 0), ctx(0, 9), ctx(2, 1)];
    v.sort();
    assert_eq!(v, [ctx(0, 9), ctx(1, 0), ctx(1, 5), ctx(2, 0), ctx(2, 1)]);
    assert_eq!(OpCtx::new(FeatureId(7)), ctx(7, 0));
    assert_eq!(InstanceId::default(), InstanceId::ORIGINAL);
    // Feature ids span the caller's 64 bits.
    assert!(ctx(u64::MAX, 0) > ctx(u64::MAX - 1, u32::MAX));
}
