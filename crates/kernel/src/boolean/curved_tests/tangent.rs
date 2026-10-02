//! Tangent contacts: walls touching along a line, where the contact runs
//! through or ends on a face.

use super::*;

/// The loops on the XY plane extruded from `from` to `to` at `tol`.
fn ex(loops: Vec<Loop>, from: f64, to: f64, feature: u64, tol: &Tolerance) -> Solid {
    let profile = Profile { loops };
    extrude(
        &profile,
        &Frame::XY,
        from,
        to,
        feature,
        tol,
        &Budget::DEFAULT,
    )
    .unwrap()
}

/// Two operands touching tangentially, `a ∩ b`'s volume (the other
/// results' follow), and which of the four results (`a ∪ b`, `a ∩ b`,
/// `a − b`, `b − a`) must work.
struct Case {
    name: &'static str,
    a: Solid,
    b: Solid,
    both: f64,
    works: [bool; 4],
}

/// The minimal tangent contacts, `k` times their size: a `2 × 4 × 1`
/// plate and cylinders against its side from outside and inside (a
/// cylinder of radius 1 inside it touches both its long sides), as tall,
/// taller, standing on it or through its top; a plate with a hole of
/// radius 0.5 and slots ending in it, beside it and across it, tangent
/// to its wall; cylinders on and beside a cylinder. Those working are
/// what works at the default tolerance at `k` = 30 (millimetres).
fn tangent_cases(k: f64, tol: &Tolerance) -> Vec<Case> {
    let v = |x: f64, y: f64| DVec2::new(x, y) * k;
    let plate = || ex(vec![rect(v(-2.0, -2.0), v(0.0, 2.0), 0)], 0.0, k, 1, tol);
    let cyl = |c: DVec2, r: f64, z0: f64, z1: f64| {
        ex(vec![circle(c, r * k, 0, false)], z0 * k, z1 * k, 2, tol)
    };
    let holed = || {
        ex(
            vec![
                rect(v(-3.0, -2.0), v(3.0, 2.0), 0),
                circle(v(0.0, 0.0), 0.5 * k, 10, true),
            ],
            0.0,
            k,
            1,
            tol,
        )
    };
    let slot = |min: DVec2, max: DVec2| ex(vec![rect(min, max, 0)], -0.5 * k, 1.5 * k, 2, tol);
    let k3 = k * k * k;
    let case = |name, a, b, both: f64, works| Case {
        name,
        a,
        b,
        both: both * k3,
        works,
    };
    let (yes, no) = (true, false);
    vec![
        // The union touches itself along the line.
        case(
            "outside, as tall",
            plate(),
            cyl(v(1.0, 0.0), 1.0, 0.0, 1.0),
            0.0,
            [no, yes, yes, yes],
        ),
        case(
            "outside, through",
            plate(),
            cyl(v(1.0, 0.0), 1.0, -0.5, 1.5),
            0.0,
            [no, yes, yes, yes],
        ),
        case(
            "outside, standing",
            plate(),
            cyl(v(1.0, 0.0), 1.0, 1.0, 2.0),
            0.0,
            [no, yes, yes, yes],
        ),
        // The plate less the cylinder touches itself; the intersection
        // ended in a corner of 180° on the plate's top.
        case(
            "inside, as tall",
            plate(),
            cyl(v(-1.0, 0.0), 1.0, 0.0, 1.0),
            PI,
            [yes, yes, no, yes],
        ),
        // The union has a cusp on the plate's top.
        case(
            "inside, through",
            plate(),
            cyl(v(-1.0, 0.0), 1.0, -0.5, 1.5),
            PI,
            [no, yes, no, yes],
        ),
        case(
            "inside, standing",
            plate(),
            cyl(v(-1.0, 0.0), 1.0, 1.0, 2.0),
            0.0,
            [no, yes, yes, yes],
        ),
        case(
            "inside, through the top",
            plate(),
            cyl(v(-1.0, 0.0), 1.0, 0.5, 2.0),
            PI / 2.0,
            [no, yes, no, yes],
        ),
        case(
            "off centre, standing",
            plate(),
            cyl(v(-0.5, 1.0), 0.5, 1.0, 2.0),
            0.0,
            [no, yes, yes, yes],
        ),
        // A knife edge and cusps.
        case(
            "slot ending in the hole",
            holed(),
            slot(v(0.0, -0.5), v(2.0, 0.5)),
            2.0 - PI / 8.0,
            [yes, no, yes, no],
        ),
        case(
            "slot beside the hole",
            holed(),
            slot(v(-1.0, 0.5), v(1.0, 1.5)),
            2.0,
            [no, yes, no, yes],
        ),
        case(
            "slot across the hole",
            holed(),
            slot(v(-2.0, -0.5), v(2.0, 0.5)),
            4.0 - PI / 4.0,
            [yes, no, yes, no],
        ),
        case(
            "cylinder on a cylinder",
            cyl(v(0.0, 0.0), 1.0, 0.0, 1.0),
            cyl(v(0.5, 0.0), 0.5, 1.0, 2.0),
            0.0,
            [no, yes, yes, yes],
        ),
    ]
}

/// The four results of `case` at `tol` within `budget`: each right (its volume within
/// `rel` of the volumes', relative to the operands' total) or refused,
/// and those `case.works` names working. Gives which worked.
fn right_or_refused(case: &Case, tol: &Tolerance, rel: f64, budget: &Budget) -> [bool; 4] {
    let (a, b) = (&case.a, &case.b);
    let (va, vb) = (a.volume(), b.volume());
    let want = [
        va + vb - case.both,
        case.both,
        va - case.both,
        vb - case.both,
    ];
    let mut worked = [false; 4];
    for (k, (x, y, op)) in [
        (a, b, Op::Union),
        (a, b, Op::Intersection),
        (a, b, Op::Difference),
        (b, a, Op::Difference),
    ]
    .into_iter()
    .enumerate()
    {
        match boolean(x, y, op, tol, budget) {
            Ok(solid) => {
                let got = solid.volume();
                assert!(
                    (got - want[k]).abs() <= rel * (va + vb),
                    "{}, result {k}: volume {got}, not {}",
                    case.name,
                    want[k]
                );
                worked[k] = true;
            }
            Err(e) => assert!(!case.works[k], "{}, result {k}: {e:?}", case.name),
        }
    }
    worked
}

#[test]
fn tangent_contacts_are_right_or_refused() {
    // In millimetres at the default tolerance, the crossings a tangency
    // leaves in pairs (some 2e-7 apart) are further apart than the
    // clean-up's short length: the intersections of a cylinder inside a
    // plate, touching its side, ended in a corner of 180° on the top
    // (and a strip of the plate's side as wide as the pairs, between the
    // cylinder's two halves), and failed.
    for case in tangent_cases(30.0, &TOL) {
        right_or_refused(&case, &TOL, 1e-9, &Budget::DEFAULT);
    }
}

#[test]
fn tangent_intersections_work_at_the_finest_tolerance() {
    // Unit size at the finest tolerance: the same pairs, some 7e-9 apart,
    // past the short length there too. The intersections of the cylinder
    // inside the plate (the cylinder, and the cylinder cut to the plate)
    // and of the slot beside the hole.
    let tol = Tolerance::new(Tolerance::MIN_FIT).unwrap();
    let cases = tangent_cases(1.0, &tol);
    for name in [
        "inside, as tall",
        "inside, through",
        "inside, through the top",
        "slot beside the hole",
    ] {
        let case = cases.iter().find(|c| c.name == name).expect("a case");
        let worked = right_or_refused(case, &tol, 1e-9, &Budget::DEFAULT);
        assert!(worked[1], "{name}");
    }
    let case = cases
        .iter()
        .find(|c| c.name == "inside, as tall")
        .expect("a case");
    assert_deterministic(|| boolean(&case.a, &case.b, Op::Intersection, &tol, &Budget::DEFAULT))
        .unwrap();
}

#[test]
fn unions_touching_along_a_line_fail_at_once() {
    // Walls tangent along a line from either side, united: `A` grown
    // crosses `B` in two lines infinitely close, which refinement split
    // until the budget ran out (seconds). The union touches itself, and
    // is refused as such within a small budget: cylinders side by side,
    // with their seams on the line or off it, and a pin in a hole against
    // its wall, either first.
    let budget = Budget::new(100_000);
    let v = DVec2::new;
    let a = cylinder([0.0, 0.0, 0.0], 1.0, 2.0);
    let on_seam = Solid::cylinder(DVec3::new(2.0, 0.0, 0.5), 1.0, 1.0, 3, &TOL).unwrap();
    let q = DQuat::from_rotation_z(0.3);
    let off_seam = moved(&cylinder([0.0, 0.0, 0.0], 1.0, 2.0), |p| {
        q * p + DVec3::X * 2.0
    });
    let holed = ex(
        vec![
            rect(v(-3.0, -2.0), v(3.0, 2.0), 0),
            circle(v(0.0, 0.0), 0.5, 10, true),
        ],
        0.0,
        1.0,
        1,
        &TOL,
    );
    let pin = ex(
        vec![circle(v(0.25, 0.0), 0.25, 0, false)],
        -0.5,
        1.5,
        2,
        &TOL,
    );
    for (x, y) in [(&a, &on_seam), (&a, &off_seam), (&holed, &pin)] {
        for (x, y) in [(x, y), (y, x)] {
            assert_eq!(
                boolean(x, y, Op::Union, &TOL, &budget).map(|s| s.volume()),
                Err(KernelError::Boolean(BooleanError::NotManifold))
            );
        }
    }
    // One inside the other, touching its skin from inside: the union is
    // the outer one, a manifold, never named so.
    let outer = ex(vec![circle(v(0.0, 0.0), 1.0, 0, false)], 0.0, 1.0, 1, &TOL);
    let inner = ex(vec![circle(v(0.5, 0.0), 0.5, 0, false)], 0.0, 1.0, 2, &TOL);
    let union = boolean(&outer, &inner, Op::Union, &TOL, &Budget::DEFAULT).unwrap();
    assert!((union.volume() - PI).abs() < 1e-9, "{}", union.volume());
    let other = boolean(&inner, &outer, Op::Union, &TOL, &budget);
    match other {
        Ok(solid) => assert!((solid.volume() - PI).abs() < 1e-9, "{}", solid.volume()),
        Err(e) => assert_ne!(e, KernelError::Boolean(BooleanError::NotManifold)),
    }
}

/// The plate `[0, 4] × [0, 3] × [0, 2]` with its top edge at `x = 4`
/// rounded by radius 1 (the axis at `x = 3`, `z = 1`, along `y`).
fn rounded_plate(tol: &Tolerance) -> Solid {
    let v = DVec2::new;
    let p = [
        v(0.0, 0.0),
        v(4.0, 0.0),
        v(4.0, 1.0),
        v(3.0, 2.0),
        v(0.0, 2.0),
    ];
    let segments = vec![
        Segment::line(p[0], p[1], 0).unwrap(),
        Segment::line(p[1], p[2], 1).unwrap(),
        arc(v(3.0, 1.0), p[2], p[3], 2),
        Segment::line(p[3], p[4], 3).unwrap(),
        Segment::line(p[4], p[0], 4).unwrap(),
    ];
    on_xz(vec![Loop { segments }], 0.0, 3.0, 1, tol)
}

/// The loops on the XZ plane (frame `x`, `z`) extruded along `+y` from
/// `y0` to `y1`.
fn on_xz(loops: Vec<Loop>, y0: f64, y1: f64, feature: u64, tol: &Tolerance) -> Solid {
    let frame = Frame {
        origin: DVec3::ZERO,
        x: DVec3::X,
        y: DVec3::Z,
    };
    // The frame's normal is `−y`.
    extrude(
        &Profile { loops },
        &frame,
        -y1,
        -y0,
        feature,
        tol,
        &Budget::DEFAULT,
    )
    .unwrap()
}

/// `2∫₀¹ (1 − u)·√(u + u²) du`, by Simpson's rule: what the rounded
/// plate's fillet adds over a disc of radius 0.5 spanning it.
fn over_the_fillet() -> f64 {
    let n = 1 << 16;
    let f = |u: f64| 2.0 * (1.0 - u) * (u + u * u).sqrt();
    let h = 1.0 / n as f64;
    let inner: f64 = (1..n)
        .map(|i| f(i as f64 * h) * if i % 2 == 1 { 4.0 } else { 2.0 })
        .sum();
    (f(0.0) + inner + f(1.0)) * h / 3.0
}

#[test]
fn tangent_contacts_at_rounded_edges_are_right_or_refused() {
    // A plate's edge rounded, and solids tangent to the round or to the
    // faces it runs into: a boss over the edge (its bottom flush with the
    // top, tangent to the round), a notch whose side is flush with the
    // plate's side where the round leaves it, a hole tangent to the side
    // through the round, and bars along the round tangent to it from
    // inside and outside.
    let tol = TOL;
    let plate = rounded_plate(&tol);
    let v = DVec2::new;
    let s = 0.5f64.sqrt();
    let (yes, no) = (true, false);
    let bar = |d: f64| {
        on_xz(
            vec![circle(v(3.0 + d * s, 1.0 + d * s), 0.3, 7, false)],
            -1.0,
            4.0,
            2,
            &tol,
        )
    };
    let cases = [
        Case {
            name: "boss over the round",
            a: plate.clone(),
            b: Solid::cuboid(
                DVec3::new(2.5, 1.0, 2.0),
                DVec3::new(2.5, 1.0, 1.0),
                2,
                &tol,
            )
            .unwrap(),
            both: 0.0,
            works: [no, yes, yes, yes],
        },
        Case {
            name: "notch flush with the side",
            a: plate.clone(),
            b: Solid::cuboid(
                DVec3::new(1.0, 1.0, 0.5),
                DVec3::new(3.0, 1.0, 2.5),
                2,
                &tol,
            )
            .unwrap(),
            both: 3.5 + PI / 4.0,
            works: [no, yes, yes, no],
        },
        Case {
            name: "hole tangent to the side",
            a: plate.clone(),
            b: ex(vec![circle(v(3.5, 1.5), 0.5, 7, false)], -1.0, 3.0, 2, &tol),
            both: PI / 4.0 + over_the_fillet(),
            works: [no, yes, no, no],
        },
        Case {
            name: "bar inside the round",
            a: plate.clone(),
            b: bar(0.7),
            both: 0.27 * PI,
            works: [no, no, no, yes],
        },
        Case {
            name: "bar outside the round",
            a: plate.clone(),
            b: bar(1.3),
            both: 0.0,
            works: [no, yes, yes, yes],
        },
    ];
    for case in &cases {
        // The hole's cut through the round is fitted. The bars' pairs
        // along the round refine to any budget.
        let budget = if case.name.starts_with("bar") {
            Budget::new(300_000)
        } else {
            Budget::DEFAULT
        };
        right_or_refused(case, &tol, 1e-5, &budget);
    }
}
