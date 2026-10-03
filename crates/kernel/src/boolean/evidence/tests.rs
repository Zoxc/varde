use glam::DVec3;

use super::*;
use crate::{Solid, Tolerance};

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
