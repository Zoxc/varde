#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::cell::RefCell;
use std::rc::Rc;

use super::*;

/// A point of variables `index` and `index + 1`.
fn point(index: usize) -> PointSlots {
    [Slot::Var(index), Slot::Var(index + 1)]
}

fn line(index: usize) -> LineSlots {
    LineSlots {
        start: point(index),
        end: point(index + 2),
    }
}

/// Every kind of equation, over the variables of [`values`], some reading
/// constants too.
fn equations() -> Vec<Residual> {
    let circle = RoundSlots::Circle {
        center: point(0),
        radius: Slot::Var(16),
    };
    let arc = RoundSlots::Arc {
        center: point(4),
        start: point(8),
    };
    let fixed = [Slot::Const(0.5), Slot::Const(-2.0)];
    let fixed_circle = RoundSlots::Circle {
        center: fixed,
        radius: Slot::Const(3.0),
    };
    vec![
        Residual::Equal(Slot::Var(0), Slot::Var(3)),
        Residual::Equal(Slot::Var(0), Slot::Const(4.0)),
        Residual::Midpoint {
            point: Slot::Var(1),
            a: Slot::Var(5),
            b: Slot::Var(10),
        },
        Residual::OnLine {
            point: point(12),
            line: line(0),
        },
        Residual::OnRound {
            point: point(12),
            round: circle,
        },
        Residual::OnRound {
            point: point(12),
            round: arc,
        },
        Residual::OnRound {
            point: point(12),
            round: fixed_circle,
        },
        Residual::Parallel(line(0), line(6)),
        Residual::Perpendicular(line(2), line(10)),
        Residual::TangentLine {
            line: line(10),
            round: circle,
            sign: 1.0,
        },
        Residual::TangentLine {
            line: line(0),
            round: arc,
            sign: -1.0,
        },
        Residual::TangentRounds {
            a: circle,
            b: arc,
            sign: 1.0,
        },
        Residual::TangentRounds {
            a: arc,
            b: fixed_circle,
            sign: -1.0,
        },
        Residual::EqualLength(line(0), line(12)),
        Residual::EqualRadius(circle, arc),
        Residual::MidpointOn {
            a: point(0),
            b: point(14),
            line: line(6),
        },
        Residual::Across {
            a: point(0),
            b: point(14),
            line: line(6),
        },
        Residual::ArcRadius {
            center: point(4),
            start: point(8),
            end: point(14),
        },
        Residual::Fix {
            slot: Slot::Var(7),
            at: 1.5,
        },
        Residual::FixDirection {
            point: point(14),
            center: point(4),
            direction: [0.6, -0.8],
        },
        Residual::Distance {
            a: point(2),
            b: point(12),
            value: 4.0,
        },
        Residual::Distance {
            a: fixed,
            b: point(6),
            value: 1.5,
        },
        Residual::Offset {
            a: Slot::Var(0),
            b: Slot::Var(9),
            sign: -1.0,
            value: 2.0,
        },
        Residual::LineDistance {
            a: point(12),
            b: point(12),
            line: line(2),
            sign: 1.0,
            value: 3.0,
        },
        Residual::LineDistance {
            a: point(0),
            b: point(14),
            line: line(8),
            sign: -1.0,
            value: 0.5,
        },
        Residual::Radius {
            round: circle,
            value: 2.5,
        },
        Residual::Radius {
            round: arc,
            value: 7.0,
        },
        Residual::Angle {
            a: line(0),
            b: line(10),
            sign: 1.0,
            cos: 0.6,
            sin: 0.8,
        },
        Residual::Angle {
            a: line(4),
            b: line(12),
            sign: -1.0,
            cos: -0.28,
            sin: 0.96,
        },
        Residual::PairOffset {
            pair: PairRead::Slots(PairSlots::Lines {
                line: line(0),
                copy: line(10),
            }),
            sign: -1.0,
            value: 2.0,
        },
        Residual::PairOffset {
            pair: PairRead::Slots(PairSlots::Rounds {
                round: circle,
                copy: arc,
            }),
            sign: 1.0,
            value: 0.5,
        },
        Residual::PairOffset {
            pair: PairRead::Slots(PairSlots::Join { copy: arc }),
            sign: 1.0,
            value: 3.0,
        },
        // Sixteen variables: two pairs of lines.
        Residual::EqualOffset(
            PairRead::Slots(PairSlots::Lines {
                line: line(0),
                copy: line(4),
            }),
            PairRead::Slots(PairSlots::Lines {
                line: line(8),
                copy: line(12),
            }),
        ),
        Residual::EqualOffset(
            PairRead::Slots(PairSlots::Rounds {
                round: fixed_circle,
                copy: circle,
            }),
            PairRead::Slots(PairSlots::Join { copy: arc }),
        ),
    ]
}

/// Values in general position: no two points alike, no line of no
/// length.
fn values() -> Vec<f64> {
    (0..17)
        .map(|i| {
            let i = i as f64;
            (i * 1.37).sin() * 10.0 + i * 0.3 + 2.0
        })
        .collect()
}

#[test]
fn every_equation_s_gradient_matches_finite_differences() {
    let x = values();
    for equation in equations() {
        let mut gradient = vec![0.0; x.len()];
        let value = equation.gradient(&x, |var, derivative| gradient[var] += derivative);
        assert_eq!(value, equation.value(&x), "{equation:?}");
        for var in 0..x.len() {
            // Central differences, with a step suited to values near ten.
            let h = 1e-6;
            let at = |offset: f64| {
                let mut x = x.clone();
                x[var] += offset;
                equation.value(&x)
            };
            let expected = (at(h) - at(-h)) / (2.0 * h);
            assert!(
                (gradient[var] - expected).abs() <= 1e-6 * expected.abs().max(1.0),
                "{equation:?} by {var}: {} against {expected}",
                gradient[var]
            );
        }
    }
}

#[test]
fn an_angle_is_zero_only_at_its_own_angle_and_no_number_at_its_mirror_image() {
    // Line 0 along x, line 4 from its start at `angle`.
    let lines = |angle: f64| {
        vec![
            0.0,
            0.0,
            2.0,
            0.0,
            0.0,
            0.0,
            3.0 * angle.cos(),
            3.0 * angle.sin(),
        ]
    };
    let asked = 1.0_f64;
    let equation = |sign| Residual::Angle {
        a: line(0),
        b: line(4),
        sign,
        cos: asked.cos(),
        sin: asked.sin(),
    };
    assert!(equation(1.0).value(&lines(asked)).abs() < 1e-12);
    // The chord between the directions: 2 sin(δ / 2) of the difference,
    // times the mean length, √6.
    for off in [-2.5, -1.0, 0.3, 2.0] {
        let expected = 2.0 * (off / 2.0_f64).sin() * 6.0_f64.sqrt();
        let value = equation(1.0).value(&lines(asked + off));
        assert!((value - expected).abs() < 1e-12, "{off}: {value}");
    }
    let flipped = equation(1.0).value(&lines(asked + std::f64::consts::PI));
    assert!(!flipped.is_finite() || flipped.abs() > 1e6, "{flipped}");
    // The first line reversed, it's the angle from the other way along it.
    let reversed = asked + std::f64::consts::PI;
    assert!(equation(-1.0).value(&lines(reversed)).abs() < 1e-12);
}

#[test]
fn every_equation_reads_few_enough_variables_once_each() {
    for equation in equations() {
        let mut vars = Vec::new();
        equation.slots(|slot| {
            if let Slot::Var(var) = slot {
                vars.push(var);
            }
        });
        assert!(vars.len() <= MAX_INPUTS, "{equation:?}");
        let mut seen = Vec::new();
        equation.gradient(&values(), |var, _| seen.push(var));
        vars.sort_unstable();
        vars.dedup();
        seen.sort_unstable();
        assert_eq!(seen, vars, "{equation:?}");
    }
}

#[test]
fn residuals_are_lengths_signed_by_side() {
    // A unit circle at the origin, a line along y = 2 running either way.
    let x = [0.0, 0.0, 1.0, -5.0, 2.0, 5.0, 2.0];
    let circle = RoundSlots::Circle {
        center: point(0),
        radius: Slot::Var(2),
    };
    let right = line(3);
    let left = LineSlots {
        start: right.end,
        end: right.start,
    };
    // The centre is right of the line running along +x: tangent on that
    // side, two away; the other side is three away.
    let tangent = |line, sign| Residual::TangentLine {
        line,
        round: circle,
        sign,
    };
    assert_eq!(tangent(right, -1.0).value(&x), -1.0);
    assert_eq!(tangent(right, 1.0).value(&x), -3.0);
    assert_eq!(tangent(left, 1.0).value(&x), 1.0);
    // Scaled by the geometry, not by the lines' lengths.
    let long = [0.0, 0.0, 100.0, 0.0, 0.0, 200.0];
    let short = [0.0, 0.0, 1.0, 0.0, 0.0, 2.0];
    let up = LineSlots {
        start: point(0),
        end: point(4),
    };
    let perpendicular = Residual::Perpendicular(line(0), up);
    let parallel = Residual::Parallel(line(0), up);
    assert_eq!(perpendicular.value(&long), 0.0);
    assert!((parallel.value(&long) - 100.0 * 2f64.sqrt()).abs() < 1e-9);
    assert!((parallel.value(&short) - 2f64.sqrt()).abs() < 1e-12);
}

#[test]
fn a_length_of_zero_has_no_direction() {
    // Two points at one place: the length's derivatives are zero, not
    // infinite.
    let x = [1.0, 1.0, 1.0, 1.0, 3.0, 1.0];
    let arc = Residual::ArcRadius {
        center: point(0),
        start: point(2),
        end: point(4),
    };
    let value = arc.gradient(&x, |_, derivative| assert!(derivative.is_finite()));
    assert_eq!(value, 2.0);
}

/// A spline through the points of variables `first`, `first + 2`, ...
/// (`count` of them) at `x`, with a handle at its first point whose tip
/// is variable `tip`, if given, open or `closed`.
fn spline_slots(
    x: &[f64],
    first: usize,
    count: usize,
    tip: Option<usize>,
    closed: bool,
) -> Rc<SplineSlots> {
    use crate::spline::{Interpolation, SplineMap, chord_params};
    let places: Vec<glam::DVec2> = (0..count)
        .map(|i| glam::DVec2::new(x[first + 2 * i], x[first + 2 * i + 1]))
        .collect();
    let params = chord_params(&places, closed);
    let handles: Vec<usize> = tip.iter().map(|_| 0).collect();
    let interpolation = Interpolation::new(&params, closed, &handles).unwrap();
    let mut inputs: Vec<PointSlots> = (0..count).map(|i| point(first + 2 * i)).collect();
    inputs.extend(tip.map(point));
    Rc::new(SplineSlots {
        map: RefCell::new(SplineMap::through(&interpolation)),
        inputs,
        length: 30.0,
        refit: None,
    })
}

/// Values for [`spline_equations`]: a wave's five fit points (0 to 9), a
/// tip (10, 11), a parameter (12), a point (13, 14), a line (15 to 18), a
/// circle's centre and radius (19 to 21), and a second spline's three
/// fit points (22 to 27) and tip (28, 29).
fn spline_values() -> Vec<f64> {
    vec![
        0.0, 0.0, 10.0, 6.0, 20.0, 4.0, 30.0, -3.0, 40.0, 2.0, // fit points
        3.0, 1.5,  // tip
        11.0, // parameter, as a length
        12.0, 7.0, // a point
        -10.0, 1.0, -1.0, 0.5, // a line
        0.5, -6.0, 5.5, // a circle
        40.0, 2.0, 50.0, -4.0, 60.0, 3.0, // the second spline
        44.0, 1.0, // its tip
    ]
}

/// Every kind of equation reading a spline, over [`spline_values`].
fn spline_equations() -> Vec<Residual> {
    let x = spline_values();
    let wave = spline_slots(&x, 0, 5, Some(10), false);
    let closed = spline_slots(&x, 0, 5, None, true);
    let second = spline_slots(&x, 22, 3, Some(28), false);
    let at = |spline: &Rc<SplineSlots>, t: Slot| SplineAt {
        spline: spline.clone(),
        t,
    };
    let start = Heading::Spline(at(&wave, Slot::Const(0.0)));
    let end = Heading::Spline(at(&wave, Slot::Const(30.0)));
    let line = Heading::Line(line(15));
    let circle = Heading::Round {
        round: RoundSlots::Circle {
            center: point(19),
            radius: Slot::Var(21),
        },
        at: point(0),
    };
    let next = Heading::Spline(at(&second, Slot::Const(0.0)));
    let mut equations: Vec<Residual> = (0..2)
        .flat_map(|axis| {
            [&wave, &closed].map(|spline| Residual::OnSpline {
                point: point(13),
                at: at(spline, Slot::Var(12)),
                axis,
            })
        })
        .collect();
    // A point's offset from a spline, at a parameter of its own.
    for spline in [&wave, &closed] {
        equations.push(Residual::Nearest {
            point: point(13),
            at: at(spline, Slot::Var(12)),
        });
    }
    let from_wave = PairRead::Spline {
        point: point(13),
        at: at(&wave, Slot::Var(12)),
    };
    let from_second = PairRead::Spline {
        point: point(15),
        at: at(&second, Slot::Var(21)),
    };
    equations.extend([
        Residual::PairOffset {
            pair: from_wave.clone(),
            sign: -1.0,
            value: 2.0,
        },
        Residual::EqualOffset(from_wave.clone(), from_second),
        Residual::EqualOffset(
            from_wave,
            PairRead::Slots(PairSlots::Lines {
                line: self::line(15),
                copy: self::line(17),
            }),
        ),
    ]);
    for (a, b) in [
        (&start, &line),
        (&start, &circle),
        (&end, &next),
        (&line, &start),
    ] {
        for sign in [1.0, -1.0] {
            equations.push(Residual::Along {
                a: a.clone(),
                b: b.clone(),
                sign,
            });
            equations.push(Residual::Curving {
                a: a.clone(),
                b: b.clone(),
                sign,
            });
        }
    }
    equations
}

#[test]
fn every_spline_equation_s_gradient_matches_finite_differences() {
    let x = spline_values();
    for equation in spline_equations() {
        let mut gradient = vec![0.0; x.len()];
        let mut seen = Vec::new();
        let value = equation.gradient(&x, |var, derivative| {
            gradient[var] += derivative;
            seen.push(var);
        });
        assert_eq!(value, equation.value(&x), "{equation:?}");
        assert!(value.is_finite(), "{equation:?}");
        // What it's differentiated over as dual numbers fits in one.
        let mut direct = 0;
        equation.direct_slots(&mut |_| direct += 1);
        assert!(
            direct + 2 * equation.reads().len() <= MAX_INPUTS,
            "{equation:?}"
        );
        // Each variable once.
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), count, "{equation:?}");
        for var in 0..x.len() {
            let h = 1e-6;
            let at = |offset: f64| {
                let mut x = x.clone();
                x[var] += offset;
                equation.value(&x)
            };
            let expected = (at(h) - at(-h)) / (2.0 * h);
            assert!(
                (gradient[var] - expected).abs() <= 1e-6 * expected.abs().max(1.0),
                "{equation:?} by {var}: {} against {expected}",
                gradient[var]
            );
        }
    }
}

#[test]
fn running_along_is_zero_only_the_way_asked() {
    // A spline's start running along x, and a line along x either way.
    let x = [0.0, 0.0, 10.0, 0.0, 20.0, 0.0, 0.0, 0.0, 5.0, 0.0];
    let wave = spline_slots(&x, 0, 3, None, false);
    let start = Heading::Spline(SplineAt {
        spline: wave,
        t: Slot::Const(0.0),
    });
    let along = |line: LineSlots, sign| Residual::Along {
        a: start.clone(),
        b: Heading::Line(line),
        sign,
    };
    let forward = line(6);
    let back = LineSlots {
        start: forward.end,
        end: forward.start,
    };
    assert!(along(forward, 1.0).value(&x).abs() < 1e-12);
    assert!(along(back, -1.0).value(&x).abs() < 1e-12);
    // The other way round is no number, or far from zero.
    let flipped = along(back, 1.0).value(&x);
    assert!(!flipped.is_finite() || flipped.abs() > 1e6, "{flipped}");
}
