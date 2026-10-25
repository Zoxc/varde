//! Inertia of the primitives against their closed forms; the user faces'
//! areas, forms and rectangles and the user edges' lengths, shapes and
//! closure on boxes and cylinders; span lengths and shapes of hand-made
//! arcs; the sheet, empty-solid and budget outcomes.

use std::f64::consts::{FRAC_1_SQRT_2, PI};

use super::*;
use crate::build::primitives::Primitive;
use crate::infra::ops::{Status, Tolerance};
use crate::names::topology::tests::primitive;

fn topology(b: &Brep) -> Topology {
    Topology::new(b, &Budget::DEFAULT).expect("within the budget")
}

/// `x` holds `exact` and is within `rel` of it relative to `scale`.
#[track_caller]
fn assert_holds(x: Bounded<f64>, exact: f64, scale: f64, rel: f64) {
    let d = (x.v - exact).abs();
    assert!(d <= x.err() + 1e-15 * scale, "{x:?} misses {exact}");
    assert!(
        x.err() <= rel * scale,
        "{x:?}: error above {rel} of {scale}"
    );
}

/// The inertia tensor holds `diag` on its diagonal and 0 off it, to
/// `rel` of `scale`.
#[track_caller]
fn tensor_holds(t: &[[B; 3]; 3], diag: [f64; 3], scale: f64, rel: f64) {
    for (a, row) in t.iter().enumerate() {
        for (b, x) in row.iter().enumerate() {
            let exact = if a == b { diag[a] } else { 0.0 };
            assert_holds(*x, exact, scale, rel);
        }
    }
}

#[test]
fn a_box_has_its_inertia() {
    let (min, [a, b, c]) = ([1.0, -2.0, 3.0], [2.0, 3.0, 5.0]);
    let body = primitive(&Primitive::Cuboid {
        min,
        size: [a, b, c],
    });
    let m = inertia(&body, &Budget::DEFAULT).expect("within the budget");
    let v = a * b * c;
    assert_holds(m.volume, v, v, 1e-12);
    let centre = m.centroid.expect("a centroid");
    for k in 0..3 {
        assert_holds(centre[k], min[k] + [a, b, c][k] * 0.5, 10.0, 1e-12);
    }
    let diag = [b * b + c * c, a * a + c * c, a * a + b * b].map(|s| v * s / 12.0);
    tensor_holds(&m.tensor.expect("a tensor"), diag, v * 25.0, 1e-12);
}

#[test]
fn a_cylinder_has_its_inertia() {
    let (base, r, h) = ([2.0, 1.0, -3.0], 1.5, 4.0);
    let body = primitive(&Primitive::Cylinder {
        base,
        radius: r,
        height: h,
    });
    let m = inertia(&body, &Budget::DEFAULT).expect("within the budget");
    let v = PI * r * r * h;
    assert_holds(m.volume, v, v, 1e-12);
    let centre = m.centroid.expect("a centroid");
    let exact = [base[0], base[1], base[2] + h * 0.5];
    for k in 0..3 {
        assert_holds(centre[k], exact[k], h, 1e-12);
    }
    let side = v * (3.0 * r * r + h * h) / 12.0;
    tensor_holds(
        &m.tensor.expect("a tensor"),
        [side, side, v * r * r / 2.0],
        v * h * h,
        1e-12,
    );
}

#[test]
fn a_sphere_has_its_inertia() {
    let (c, r) = ([3.0, -1.0, 2.0], 2.0);
    let body = primitive(&Primitive::Sphere {
        centre: c,
        radius: r,
    });
    let m = inertia(&body, &Budget::DEFAULT).expect("within the budget");
    let v = 4.0 / 3.0 * PI * r * r * r;
    assert_holds(m.volume, v, v, 1e-12);
    let centre = m.centroid.expect("a centroid");
    for k in 0..3 {
        assert_holds(centre[k], c[k], r, 1e-12);
    }
    let i = 0.4 * v * r * r;
    tensor_holds(&m.tensor.expect("a tensor"), [i; 3], i, 1e-11);
}

#[test]
fn a_sheet_has_no_inertia_and_the_empty_solid_none_to_show() {
    let status = || {
        Status::new(
            Tolerance::DEFAULT,
            &varde_decide::num::storage::OpStart {
                max_coord: 1.0,
                rho_op: 0.0,
            },
        )
    };
    let sheet = Brep::new(BodyKind::Sheet, status());
    assert_eq!(
        inertia(&sheet, &Budget::DEFAULT),
        Err(KernelError::Invalid(Invalid::NotClosed))
    );
    let empty = Brep::new(BodyKind::Solid, status());
    let m = inertia(&empty, &Budget::DEFAULT).expect("nothing to integrate");
    assert_eq!(m.volume.v, 0.0);
    assert_eq!((m.centroid, m.tensor), (None, None));
}

#[test]
fn a_small_budget_runs_out() {
    let body = primitive(&Primitive::Sphere {
        centre: [0.0; 3],
        radius: 1.0,
    });
    assert!(matches!(
        inertia(&body, &Budget::new(1_000)),
        Err(KernelError::OutOfBudget { .. })
    ));
    assert!(matches!(
        edges_length(
            &body,
            &(0..body.edge_count() as u32)
                .map(EdgeId)
                .collect::<Vec<_>>(),
            &Budget::new(1)
        ),
        Err(KernelError::OutOfBudget { .. })
    ));
}

#[test]
fn a_boxs_faces_are_rectangles_and_its_edges_lines() {
    let (min, size) = ([1.0, -2.0, 3.0], [2.0, 3.0, 5.0]);
    let body = primitive(&Primitive::Cuboid { min, size });
    let topo = topology(&body);
    assert_eq!(topo.regions().len(), 6);
    let mut areas = Vec::new();
    for r in 0..6 {
        let m = region_measure(&body, &topo, r, &Budget::DEFAULT)
            .expect("within the budget")
            .expect("a region");
        assert!(matches!(m.form, Some(FormKey::Plane(_))));
        let [a, b] = m.rectangle.expect("a rectangle");
        let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let area = len(a) * len(b);
        assert_holds(m.area, area, area, 1e-12);
        areas.push(area);
    }
    areas.sort_by(f64::total_cmp);
    assert_eq!(areas, [6.0, 6.0, 10.0, 10.0, 15.0, 15.0]);
    assert!(
        region_measure(&body, &topo, 6, &Budget::DEFAULT)
            .unwrap()
            .is_none()
    );
    assert_eq!(topo.chains().len(), 12);
    for c in 0..12 {
        let m = chain_measure(&body, &topo, c, &Budget::DEFAULT)
            .expect("within the budget")
            .expect("a chain");
        assert!(!m.closed);
        let EdgeShape::Line { from, to } = m.shape else {
            panic!("{:?} isn't a line", m.shape);
        };
        assert!(from < to, "ends in their geometric order");
        let along: Vec<usize> = (0..3).filter(|&k| from[k] != to[k]).collect();
        let &[k] = &along[..] else {
            panic!("{from:?}–{to:?} not along an axis");
        };
        assert_eq!((from[k], to[k]), (min[k], min[k] + size[k]));
        assert_holds(m.length, size[k], size[k], 1e-12);
        assert_eq!(
            m.shape.point(),
            Some(std::array::from_fn(|i| (from[i] + to[i]) * 0.5))
        );
    }
    assert!(
        chain_measure(&body, &topo, 12, &Budget::DEFAULT)
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_cylinders_rims_are_circles_and_its_faces_have_their_forms() {
    let (base, r, h) = ([2.0, 1.0, -3.0], 1.5, 4.0);
    let body = primitive(&Primitive::Cylinder {
        base,
        radius: r,
        height: h,
    });
    let topo = topology(&body);
    assert_eq!((topo.regions().len(), topo.chains().len()), (3, 2));
    let mut caps = 0;
    for rg in 0..3 {
        let m = region_measure(&body, &topo, rg, &Budget::DEFAULT)
            .unwrap()
            .unwrap();
        assert_eq!(m.rectangle, None);
        match m.form {
            Some(FormKey::Plane(_)) => {
                caps += 1;
                assert_holds(m.area, PI * r * r, PI * r * r, 1e-12);
            }
            Some(FormKey::Cylinder(k)) => {
                assert_eq!(k.radius(), r);
                assert_holds(m.area, 2.0 * PI * r * h, 2.0 * PI * r * h, 1e-12);
            }
            ref f => panic!("form {f:?}"),
        }
    }
    assert_eq!(caps, 2);
    let mut heights = Vec::new();
    for c in 0..2 {
        let m = chain_measure(&body, &topo, c, &Budget::DEFAULT)
            .unwrap()
            .unwrap();
        assert!(m.closed);
        assert_holds(m.length, 2.0 * PI * r, 2.0 * PI * r, 1e-12);
        let EdgeShape::Circle {
            centre,
            axis,
            radius,
        } = m.shape
        else {
            panic!("{:?} isn't a circle", m.shape);
        };
        assert!((radius - r).abs() <= 1e-12 * r);
        // A closed chain: the sign rule.
        assert!((axis[2] - 1.0).abs() <= 1e-12 && axis[0].abs() <= 1e-12);
        assert!((centre[0] - base[0]).abs() <= 1e-12 && (centre[1] - base[1]).abs() <= 1e-12);
        heights.push(centre[2]);
    }
    heights.sort_by(f64::total_cmp);
    for (got, want) in heights.iter().zip([base[2], base[2] + h]) {
        assert!((got - want).abs() <= 1e-12 * h, "{got} for {want}");
    }
}

/// A quarter of the circle of radius `r` about the origin in the plane
/// `z = 0`, from `+x` to `+y`, as one standard span, its middle weight
/// scaled by `s` and the ends' too (the same curve).
fn quarter(r: f64, s: f64) -> Span3 {
    [
        [r, 0.0, 0.0, s],
        [r, r, 0.0, FRAC_1_SQRT_2 * s],
        [0.0, r, 0.0, s],
    ]
}

#[test]
fn a_quarter_circle_has_its_length_whatever_its_weights() {
    let r = 3.0;
    let budget = Budget::DEFAULT;
    for s in [1.0, 0.25, 4.0] {
        let mut work = Work::new(&budget);
        let pass = Pass {
            stage: "measure",
            rung: 0,
        };
        let l = work
            .pass(pass, &[quarter(r, s)], |span, ledger| {
                SpanFits::default().length(span, ledger)
            })
            .unwrap()[0];
        assert_holds(l, PI * r / 2.0, r, 1e-12);
    }
}

#[test]
fn arcs_are_named_by_what_they_are_on() {
    let r = 3.0;
    let circle = shape_of(&[Arc::of(&quarter(r, 1.0))], None);
    let EdgeShape::Circle {
        centre,
        axis,
        radius,
    } = circle
    else {
        panic!("{circle:?}");
    };
    assert!(centre.iter().all(|x| x.abs() <= 1e-12) && (radius - r).abs() <= 1e-12);
    assert_eq!(axis, [0.0, 0.0, 1.0]);
    // Stretched along `x`: an ellipse with semi-axes `2r` and `r`.
    let mut stretched = quarter(r, 1.0);
    for p in &mut stretched {
        p[0] *= 2.0;
    }
    let EdgeShape::Ellipse { major, minor, .. } = shape_of(&[Arc::of(&stretched)], None) else {
        panic!("not an ellipse");
    };
    assert!((major - 2.0 * r).abs() <= 1e-9 && (minor - r).abs() <= 1e-9);
    // A parabola's arc (weight 1) is neither.
    let parabola = [
        [0.0, 0.0, 0.0, 1.0],
        [1.0, 1.0, 0.0, 1.0],
        [2.0, 0.0, 0.0, 1.0],
    ];
    assert_eq!(shape_of(&[Arc::of(&parabola)], None), EdgeShape::Other);
    // A straight span is a line between the given ends.
    let line = [
        [2.0, 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 2.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    assert_eq!(
        shape_of(&[Arc::of(&line)], Some([[2.0, 0.0, 0.0], [0.0, 0.0, 0.0]])),
        EdgeShape::Line {
            from: [0.0; 3],
            to: [2.0, 0.0, 0.0]
        }
    );
}
