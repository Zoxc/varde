//! What a revolve refuses, and how.

use super::*;
use crate::Stripped;

fn run(p: &Profile, sweep: Sweep) -> Result<Solid, KernelError> {
    revolve(p, &Z, sweep, 7, &TOL, &Budget::DEFAULT).stripped()
}

fn part(to: f64) -> Sweep {
    Sweep::Part { from: 0.0, to }
}

#[test]
fn profiles_across_the_axis_are_refused() {
    // A rectangle reaching across, a segment bulging across from ends on
    // it, and one leaving the axis the wrong way.
    let across = profile(vec![rect(v(-1.0, 0.0), v(3.0, 1.0), 1)]);
    let line = |a, b, curve| Segment::line(a, b, curve).unwrap();
    let bulging = profile(vec![Loop {
        segments: vec![
            line(v(0.0, -2.0), v(2.0, 0.0), 1),
            line(v(2.0, 0.0), v(0.0, 2.0), 2),
            Segment {
                conic: Conic2::new(v(0.0, 2.0), v(-1.0, 0.0), 1.0, v(0.0, -2.0)).unwrap(),
                curve: 3,
            },
        ],
    }]);
    for sweep in [Sweep::Full, part(1.0)] {
        assert_eq!(
            run(&across, sweep),
            Err(KernelError::Profile(ProfileError::CrossesAxis(0, 0)))
        );
        assert_eq!(
            run(&bulging, sweep),
            Err(KernelError::Profile(ProfileError::CrossesAxis(0, 2)))
        );
    }
    // Beyond the resolution off the axis on the wrong side.
    let res = TOL.resolution();
    let off = profile(vec![polygon(
        &[
            v(-2.0 * res, 0.0),
            v(3.0, 0.0),
            v(3.0, 5.0),
            v(-2.0 * res, 5.0),
        ],
        1,
    )]);
    assert_eq!(
        run(&off, Sweep::Full),
        Err(KernelError::Profile(ProfileError::CrossesAxis(0, 0)))
    );
}

#[test]
fn profiles_touching_the_axis_at_a_point_are_refused_in_a_full_turn() {
    // A vertex alone on the axis pinches a full turn to a point; a part
    // turn is a solid (see the diamond among the shapes).
    let diamond = profile(vec![polygon(
        &[v(0.0, 0.0), v(2.0, -1.0), v(4.0, 0.0), v(2.0, 1.0)],
        1,
    )]);
    assert_eq!(
        run(&diamond, Sweep::Full),
        Err(KernelError::Profile(ProfileError::TouchesAxis(0, 0)))
    );
    assert!(run(&diamond, part(2.0)).is_ok());
    // A parabola touching the axis half way along, `x = (1 − 2t)²`:
    // refused in any turn.
    let line = |a, b, curve| Segment::line(a, b, curve).unwrap();
    let touching = profile(vec![Loop {
        segments: vec![
            line(v(1.0, 0.0), v(3.0, 0.0), 1),
            line(v(3.0, 0.0), v(3.0, 2.0), 2),
            line(v(3.0, 2.0), v(1.0, 2.0), 3),
            Segment {
                conic: Conic2::new(v(1.0, 2.0), v(-1.0, 1.0), 1.0, v(1.0, 0.0)).unwrap(),
                curve: 4,
            },
        ],
    }]);
    for sweep in [Sweep::Full, part(2.0)] {
        assert_eq!(
            run(&touching, sweep),
            Err(KernelError::Profile(ProfileError::TouchesAxis(0, 3)))
        );
    }
    // A lens whose arc comes within rounding, or within the resolution,
    // of the axis half way along: touching it too (it was repaired until
    // it ran out, `TooComplex`). Its circle drawn round a centre `2 + gap`
    // out never meets the axis exactly.
    let res = TOL.resolution();
    for gap in [0.0, 1e-12, 0.5 * res, 0.9 * res] {
        let centre = v(2.0 + gap, 0.0);
        let h = 3f64.sqrt();
        let (top, bottom) = (centre + v(-1.0, h), centre + v(-1.0, -h));
        let lens = profile(vec![Loop {
            segments: vec![arc(centre, top, bottom, 1), line(bottom, top, 2)],
        }]);
        for sweep in [Sweep::Full, part(1.0)] {
            assert_eq!(
                run(&lens, sweep),
                Err(KernelError::Profile(ProfileError::TouchesAxis(0, 0))),
                "{gap:e}"
            );
        }
    }
}

#[test]
fn turns_too_close_to_a_full_one_are_refused() {
    let washer = profile(vec![rect(v(1.0, 0.0), v(3.0, 1.0), 1)]);
    for gap in [0.0, 1e-12, 1e-10, 0.2 * TOL.resolution()] {
        assert_eq!(
            run(&washer, part(TAU - gap)),
            if gap == 0.0 {
                Err(KernelError::Patch(PatchError::Parameter(TAU)))
            } else {
                Err(KernelError::Profile(ProfileError::NearlyFullTurn))
            },
            "{gap:e}"
        );
    }
    // A thousandth of a radian short: a solid, its ends 3e-3 apart at
    // the outside.
    revolved(
        &Shape {
            name: "washer",
            profile: washer,
            fitted: None,
            part_only: false,
        },
        &Z,
        part(TAU - 1e-3),
        &TOL,
    );
}

#[test]
fn bad_turns_and_frames_are_refused() {
    let washer = profile(vec![rect(v(1.0, 0.0), v(3.0, 1.0), 1)]);
    let parameter = |x: f64| Err(KernelError::Patch(PatchError::Parameter(x)));
    assert_eq!(run(&washer, part(0.0)), parameter(0.0));
    assert_eq!(run(&washer, part(-1.0)), parameter(-1.0));
    assert_eq!(run(&washer, part(7.0)), parameter(7.0));
    assert!(matches!(
        run(&washer, part(f64::NAN)),
        Err(KernelError::Patch(PatchError::Parameter(x))) if x.is_nan()
    ));
    assert_eq!(
        run(
            &washer,
            Sweep::Part {
                from: 30.0,
                to: 31.0
            }
        ),
        parameter(30.0)
    );
    let skewed = Frame {
        x: DVec3::new(1.0, 0.1, 0.0),
        ..Z
    };
    assert_eq!(
        revolve(&washer, &skewed, Sweep::Full, 7, &TOL, &Budget::DEFAULT).stripped(),
        Err(KernelError::Patch(PatchError::Degenerate))
    );
    let far = Frame {
        origin: DVec3::new(2e6, 0.0, 0.0),
        ..Z
    };
    assert!(matches!(
        revolve(&washer, &far, Sweep::Full, 7, &TOL, &Budget::DEFAULT).stripped(),
        Err(KernelError::Patch(PatchError::Coordinate(_)))
    ));
}

#[test]
fn bad_regions_are_refused_as_an_extrude_refuses_them() {
    // Touching loops, a hole outside the outline, an empty profile.
    let touching = profile(vec![
        rect(v(1.0, 0.0), v(3.0, 1.0), 1),
        rect(v(3.0, 0.0), v(5.0, 1.0), 5),
    ]);
    assert!(matches!(
        run(&touching, Sweep::Full),
        Err(KernelError::Profile(ProfileError::Touching(_)))
    ));
    let outside = profile(vec![
        rect(v(1.0, 0.0), v(3.0, 1.0), 1),
        reversed(&rect(v(4.0, 0.0), v(5.0, 1.0), 5)),
    ]);
    assert_eq!(
        run(&outside, part(1.0)),
        Err(KernelError::Profile(ProfileError::Nesting(1)))
    );
    assert_eq!(
        run(&outside, Sweep::Full),
        Err(KernelError::Profile(ProfileError::Nesting(1)))
    );
    assert_eq!(
        run(&Profile::default(), Sweep::Full),
        Err(KernelError::Profile(ProfileError::Empty))
    );
}

#[test]
fn work_past_the_budget_is_too_complex() {
    let shapes = shapes();
    for budget in [1, 100, 10_000] {
        assert_eq!(
            revolve(
                &shapes[8].profile,
                &Z,
                Sweep::Full,
                7,
                &TOL,
                &Budget::new(budget)
            )
            .stripped(),
            Err(KernelError::TooComplex)
        );
    }
}
