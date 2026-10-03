use std::cell::Cell;

use glam::DVec3;

use super::*;
use crate::mesh::tests::TOL;
use crate::{Solid, Tolerance};

thread_local! {
    /// An allowance other than `EVIDENCE_WORK` for the gathers this
    /// thread starts, for tests of running out ([`with_allowance`]).
    pub(super) static ALLOWANCE: Cell<Option<u64>> = const { Cell::new(None) };
}

/// `f`, every gather this thread starts in it given `units` of
/// allowance.
pub(in crate::boolean) fn with_allowance<T>(units: u64, f: impl FnOnce() -> T) -> T {
    ALLOWANCE.set(Some(units));
    let result = f();
    ALLOWANCE.set(None);
    result
}

/// Checks that `failure`, a boolean's of `a` and `b` failing as
/// [`BooleanError::Inconsistent`], shows something, within the caps, and
/// lies where the operands are: every face it names is one of that
/// operand's (by its name or an alias), and every point, curve end and
/// patch corner is within the operands' boxes, grown by the resolution.
pub(in crate::boolean) fn on_operands(a: &Solid, b: &Solid, failure: &Failure, tol: &Tolerance) {
    assert_eq!(
        failure.error,
        KernelError::Boolean(BooleanError::Inconsistent)
    );
    let evidence = &failure.evidence;
    assert!(
        !evidence.is_empty() && evidence.within_caps(),
        "{evidence:?}"
    );
    for &(operand, key) in &evidence.faces {
        let mesh = match operand {
            Operand::A => a.mesh(),
            Operand::B => b.mesh(),
        };
        let named = (mesh.faces().iter()).any(|f| f.name.key() == key)
            || mesh.aliases().iter().any(|&(_, k)| k == key);
        assert!(named, "{operand:?} has no face {key:?}");
    }
    let bounds = a.bounds3().unwrap().union(b.bounds3().unwrap());
    let r = tol.resolution();
    let inside = |p: DVec3| {
        assert!(
            (bounds.min - p).max_element() <= r && (p - bounds.max).max_element() <= r,
            "{p} outside {bounds:?}"
        );
    };
    evidence.points.iter().copied().for_each(inside);
    for curve in &evidence.curves {
        inside(curve.p0);
        inside(curve.p1);
    }
    for patch in &evidence.patches {
        patch.p.into_iter().for_each(inside);
    }
}

#[test]
fn a_pair_is_whole_at_the_cap() {
    // One patch short of room for a pair: it is left out whole, its faces
    // too, and the evidence marked truncated.
    let a = Solid::cuboid(DVec3::ZERO, DVec3::ONE, 1, &TOL).unwrap();
    let b = Solid::cuboid(DVec3::ONE, DVec3::ONE, 2, &TOL).unwrap();
    let (ia, ib) = (Input::new(a.mesh(), &TOL), Input::new(b.mesh(), &TOL));
    let mut gather = Gather::new();
    for _ in 0..MAX_EVIDENCE.patches - 1 {
        gather.tri(Side::A, &ia, 0);
    }
    assert!(!gather.truncated());
    gather.pair(&ia, &ib, [1, 0]);
    let failure = gather.failure(BooleanError::NotManifold);
    let e = &failure.evidence;
    assert_eq!(e.patches.len(), MAX_EVIDENCE.patches - 1);
    assert!(e.faces.iter().all(|&(operand, _)| operand == Operand::A));
    assert!(e.truncated);
}
