//! Tight boxes of the primitives against their closed forms: each side's
//! enclosure holds the exact value and is within the resolution; the
//! faces' insides found where no edge reaches the side (a sphere's and a
//! torus's equators, a cylinder's sides); the empty body and the budget.

use super::*;
use crate::build::primitives::Primitive;
use crate::infra::ops::{Status, Tolerance};
use crate::names::topology::tests::primitive;

const RES: f64 = 1e-9;

/// Each side holds the exact box's and is within `RES` (and its
/// roundings) of it.
#[track_caller]
fn holds(b: &Brep, lo: [f64; 3], hi: [f64; 3]) {
    let t = tight_box(b, RES, &Budget::DEFAULT)
        .expect("within the budget")
        .expect("a box");
    for k in 0..3 {
        for (e, x) in [(t.min[k], lo[k]), (t.max[k], hi[k])] {
            let slack = 1e-14 * (1.0 + x.abs());
            assert!(
                e[0] <= x + slack && x - slack <= e[1],
                "axis {k}: {e:?} misses {x}"
            );
            assert!(
                e[1] - e[0] <= RES + slack,
                "axis {k}: {e:?} wider than the resolution"
            );
        }
    }
    let [olo, ohi] = t.outer();
    let [ilo, ihi] = t.inner();
    for k in 0..3 {
        assert!(olo[k] <= ilo[k] && ihi[k] <= ohi[k]);
    }
}

#[test]
fn a_boxs_box_is_its_corners() {
    let (min, size) = ([1.0, -2.0, 3.0], [2.0, 3.0, 5.0]);
    let b = primitive(&Primitive::Cuboid { min, size });
    holds(&b, min, std::array::from_fn(|k| min[k] + size[k]));
}

#[test]
fn a_cylinders_box_reaches_its_sides() {
    let (c, r, h) = ([2.0, 1.0, -3.0], 1.5, 4.0);
    let b = primitive(&Primitive::Cylinder {
        base: c,
        radius: r,
        height: h,
    });
    holds(
        &b,
        [c[0] - r, c[1] - r, c[2]],
        [c[0] + r, c[1] + r, c[2] + h],
    );
}

#[test]
fn a_cones_box_reaches_its_base() {
    let (c, h) = ([0.5, -1.0, 2.0], 3.0);
    let b = primitive(&Primitive::Cone {
        base: c,
        radii: [2.0, 0.5],
        height: h,
    });
    holds(
        &b,
        [c[0] - 2.0, c[1] - 2.0, c[2]],
        [c[0] + 2.0, c[1] + 2.0, c[2] + h],
    );
}

#[test]
fn a_spheres_box_reaches_its_equator() {
    let (c, r) = ([3.0, -1.0, 2.0], 2.0);
    let b = primitive(&Primitive::Sphere {
        centre: c,
        radius: r,
    });
    holds(&b, c.map(|x| x - r), c.map(|x| x + r));
}

#[test]
fn a_torus_box_reaches_its_outer_equator_and_its_top() {
    let (c, rr, r) = ([3.0, 1.0, -2.0], 6.0, 2.5);
    let b = primitive(&Primitive::Torus {
        centre: c,
        major: rr,
        minor: r,
    });
    let e = rr + r;
    holds(
        &b,
        [c[0] - e, c[1] - e, c[2] - r],
        [c[0] + e, c[1] + e, c[2] + r],
    );
}

#[test]
fn the_empty_body_has_no_box() {
    let start = varde_decide::num::storage::OpStart {
        max_coord: 1.0,
        rho_op: 0.0,
    };
    let b = Brep::new(
        crate::topo::brep::BodyKind::Solid,
        Status::new(Tolerance::DEFAULT, &start),
    );
    assert_eq!(tight_box(&b, RES, &Budget::DEFAULT), Ok(None));
}

#[test]
fn a_small_budget_runs_out() {
    let b = primitive(&Primitive::Sphere {
        centre: [0.0; 3],
        radius: 1.0,
    });
    assert!(matches!(
        tight_box(&b, RES, &Budget::new(100)),
        Err(KernelError::OutOfBudget { .. })
    ));
}

#[test]
fn a_coarse_resolution_is_coarse_but_holds() {
    let (c, r) = ([0.0; 3], 2.0);
    let b = primitive(&Primitive::Sphere {
        centre: c,
        radius: r,
    });
    let t = tight_box(&b, 0.1, &Budget::DEFAULT).unwrap().unwrap();
    for k in 0..3 {
        assert!(t.max[k][0] <= r && r <= t.max[k][1] && t.max[k][1] - t.max[k][0] <= 0.1);
        assert!(t.min[k][0] <= -r && -r <= t.min[k][1]);
    }
}
