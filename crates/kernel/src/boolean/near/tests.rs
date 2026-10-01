#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use glam::{DVec2, DVec3};

use super::*;
use crate::boolean::pairs::tests::{poke, reach};
use crate::budget::Budget;
use crate::mesh::tests::{TOL, round_octahedron};
use crate::par::assert_deterministic;
use crate::profile::tests::{arc, circle, rect};
use crate::profile::{Loop, Segment};
use crate::{Frame, Profile, Solid, Tolerance, extrude, touches};

/// Less than the refinement `touches` used to run took on any of these:
/// it ran out of the whole budget at the default tolerance, and took
/// some 0.7 million units at the coarsest.
const BUDGET: u64 = 200_000;

fn coarse() -> Tolerance {
    Tolerance::new(Tolerance::MAX_FIT).unwrap()
}

/// `loops` on `frame`, extruded from `z0` to `z1` along its normal.
fn prism(loops: Vec<Loop>, frame: &Frame, z: [f64; 2], tol: &Tolerance) -> Solid {
    extrude(
        &Profile { loops },
        frame,
        z[0],
        z[1],
        1,
        tol,
        &Budget::DEFAULT,
    )
    .unwrap()
}

/// The round rod of radius `r` round `center` of the XY plane, from `z0`
/// to `z1` up `z`.
fn rod(center: DVec2, r: f64, z: [f64; 2], tol: &Tolerance) -> Solid {
    prism(vec![circle(center, r, 0, false)], &Frame::XY, z, tol)
}

/// The point `d` from the origin at angle `phase` from `+x`.
fn at(phase: f64, d: f64) -> DVec2 {
    DVec2::new(phase.cos(), phase.sin()) * d
}

/// `touches` both ways round within [`BUDGET`], which must agree.
fn both(a: &Solid, b: &Solid, tol: &Tolerance) -> bool {
    let budget = Budget::new(BUDGET);
    let ab = touches(a, b, tol, &budget).unwrap();
    let ba = touches(b, a, tol, &budget).unwrap();
    assert_eq!(ab, ba);
    ab
}

/// The gaps tried, in resolutions, and whether they touch: within the
/// resolution they do, and past `1 + √2` resolutions (where the search
/// may still keep a pair of flat pieces) they don't.
const GAPS: [(f64, bool); 5] = [
    (0.0, true),
    (0.5, true),
    (0.99, true),
    (2.5, false),
    (3.0, false),
];

#[test]
fn cylinders_side_by_side_touch_along_a_ruling() {
    // On the seams of both (phase 0) and off them; the second as tall as
    // the first and shorter.
    for tol in [TOL, coarse()] {
        let r = tol.resolution();
        let a = rod(DVec2::ZERO, 1.0, [0.0, 2.0], &tol);
        for phase in [0.0, 0.3] {
            for z in [[0.0, 2.0], [0.5, 1.5]] {
                for (gap, want) in GAPS {
                    let b = rod(at(phase, 2.0 + gap * r), 1.0, z, &tol);
                    assert_eq!(both(&a, &b, &tol), want, "{tol:?} {phase} {z:?} {gap}");
                }
            }
        }
    }
}

#[test]
fn turned_rods_touch_along_a_ruling() {
    // Rods of radii 1 and 0.7 along a direction off every axis.
    let d = DVec3::new(1.0, 2.0, 3.0).normalize();
    let (x, y) = d.any_orthonormal_pair();
    let frame = Frame {
        origin: DVec3::new(0.3, -0.2, 0.1),
        x,
        y,
    };
    for tol in [TOL, coarse()] {
        let r = tol.resolution();
        let a = prism(
            vec![circle(DVec2::ZERO, 1.0, 0, false)],
            &frame,
            [0.0, 3.0],
            &tol,
        );
        for (gap, want) in GAPS {
            let c = at(1.1, 1.7 + gap * r);
            let b = prism(vec![circle(c, 0.7, 0, false)], &frame, [1.0, 2.0], &tol);
            assert_eq!(both(&a, &b, &tol), want, "{tol:?} {gap}");
        }
    }
}

/// A plate from `-3` to `3` in `x` and `y` and `0` to `1` in `z`, with a
/// hole of radius 1 round the origin.
fn holed_plate(tol: &Tolerance) -> Solid {
    let outer = rect(DVec2::splat(-3.0), DVec2::splat(3.0), 0);
    let hole = circle(DVec2::ZERO, 1.0, 4, true);
    prism(vec![outer, hole], &Frame::XY, [0.0, 1.0], tol)
}

#[test]
fn a_pin_against_a_holes_wall_touches() {
    // A pin of radius 0.5 in the hole, against its wall on the seams and
    // off them, through the plate and inside it.
    for tol in [TOL, coarse()] {
        let r = tol.resolution();
        let plate = holed_plate(&tol);
        for phase in [0.0, 0.3] {
            for z in [[-1.0, 2.0], [0.25, 0.75]] {
                for (gap, want) in GAPS {
                    let pin = rod(at(phase, 0.5 - gap * r), 0.5, z, &tol);
                    assert_eq!(
                        both(&plate, &pin, &tol),
                        want,
                        "{tol:?} {phase} {z:?} {gap}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_tool_tangent_to_a_hole_from_outside_touches() {
    // Through the plate beside the hole, tangent to its wall: the
    // crossings through the caps show it at once.
    for tol in [TOL, coarse()] {
        let tool = rod(at(0.3, 2.5), 1.5, [-1.0, 2.0], &tol);
        assert!(both(&holed_plate(&tol), &tool, &tol), "{tol:?}");
    }
}

/// A plate from `0` to `6` in `x` and `y` and `0` to `1` in `z`, its
/// corner at `(6, 6)` rounded with radius 2 round `(4, 4)`.
fn rounded_plate(tol: &Tolerance) -> Solid {
    let p = DVec2::new;
    let line = |a, b, curve| Segment::line(a, b, curve).unwrap();
    let outline = Loop {
        segments: vec![
            line(p(0.0, 0.0), p(6.0, 0.0), 0),
            line(p(6.0, 0.0), p(6.0, 4.0), 1),
            arc(p(4.0, 4.0), p(6.0, 4.0), p(4.0, 6.0), 2),
            line(p(4.0, 6.0), p(0.0, 6.0), 3),
            line(p(0.0, 6.0), p(0.0, 0.0), 4),
        ],
    };
    prism(vec![outline], &Frame::XY, [0.0, 1.0], tol)
}

#[test]
fn a_boss_tangent_to_a_plates_side_touches() {
    // A boss of radius 1 beside the plate, past both its faces, tangent
    // to its rounded corner, or to a flat side (a plane against a
    // cylinder, which the counting saw before).
    for tol in [TOL, coarse()] {
        let r = tol.resolution();
        let plate = rounded_plate(&tol);
        for (gap, want) in GAPS {
            let corner = DVec2::splat(4.0) + at(0.6, 3.0 + gap * r);
            let side = DVec2::new(-1.0 - gap * r, 3.0);
            for center in [corner, side] {
                let boss = rod(center, 1.0, [-1.0, 2.0], &tol);
                assert_eq!(both(&plate, &boss, &tol), want, "{tol:?} {center} {gap}");
            }
        }
    }
}

#[test]
fn a_box_corner_in_a_cylinders_hull_doesnt_touch() {
    // The corner at (0.8, 0.8) is inside the control hull of the
    // cylinder's quarter wall (its control points at (1, 1)), but 0.13
    // off the wall.
    for tol in [TOL, coarse()] {
        let cylinder = Solid::cylinder(DVec3::ZERO, 1.0, 2.0, 2, &tol).unwrap();
        let cube = Solid::cuboid(DVec3::new(0.8, 0.8, 0.5), DVec3::ONE, 1, &tol).unwrap();
        assert!(!both(&cylinder, &cube, &tol), "{tol:?}");
    }
}

#[test]
fn a_ball_by_a_slab_touches_at_a_point() {
    // The round octahedron's patch reaches furthest along a diagonal at
    // its middle; a slab square to it, its face the gap past that
    // point, there or across the whole patch. At the finest tolerance
    // the slab's large faces against the ball's small pieces left GJK
    // short of telling them apart at 3 resolutions.
    for fit in [Tolerance::MIN_FIT, TOL.fit(), Tolerance::MAX_FIT] {
        let tol = Tolerance::new(fit).unwrap();
        let r = tol.resolution();
        let ball = Solid::new(round_octahedron(DVec3::ZERO), &tol).unwrap();
        for d in [DVec3::ONE, DVec3::new(-1.0, -1.0, 1.0)] {
            let d = d.normalize();
            let (x, y) = d.any_orthonormal_pair();
            let frame = Frame {
                origin: DVec3::ZERO,
                x,
                y,
            };
            let along = x.cross(y).dot(d).signum();
            for (gap, want) in GAPS {
                let h = reach() + gap * r;
                let span = if along > 0.0 {
                    [h, h + 3.0]
                } else {
                    [-h - 3.0, -h]
                };
                let slab = prism(
                    vec![rect(DVec2::splat(-3.0), DVec2::splat(3.0), 0)],
                    &frame,
                    span,
                    &tol,
                );
                assert_eq!(both(&ball, &slab, &tol), want, "{fit} {d} {gap}");
            }
        }
    }
}

#[test]
fn a_loop_inside_one_patch_touches() {
    // The slab's top cuts a cap 1e-4 deep off one patch of the round
    // octahedron, crossing no edge of either.
    let (ball, slab) = poke(reach() - 1e-4);
    assert!(both(&ball, &slab, &TOL));
}

/// What `touches` does with curved operands, and the work it leaves.
fn spent(a: &Solid, b: &Solid, tol: &Tolerance) -> (Result<bool, KernelError>, u64) {
    let mut work = Work::new(&Budget::new(BUDGET));
    let (ia, ib) = (Input::new(a.mesh(), tol), Input::new(b.mesh(), tol));
    let answer = touching(&ia, &ib, tol, &mut work);
    (answer, work.left())
}

#[test]
fn deterministic() {
    let r = TOL.resolution();
    let a = rod(DVec2::ZERO, 1.0, [0.0, 2.0], &TOL);
    for (gap, want) in [(0.0, true), (3.0, false)] {
        let b = rod(at(0.3, 2.0 + gap * r), 1.0, [0.5, 1.5], &TOL);
        let (answer, left) = assert_deterministic(|| spent(&a, &b, &TOL));
        assert_eq!(answer, Ok(want));
        assert_eq!(touches(&a, &b, &TOL, &Budget::new(BUDGET)), Ok(want));
        assert!(left > 0);
    }
}

#[test]
fn out_of_budget_is_too_complex() {
    // The near miss with too little left for the search.
    let r = TOL.resolution();
    let a = rod(DVec2::ZERO, 1.0, [0.0, 2.0], &TOL);
    let b = rod(at(0.3, 2.0 + 3.0 * r), 1.0, [0.5, 1.5], &TOL);
    let (_, left) = spent(&a, &b, &TOL);
    let needed = BUDGET - left;
    assert_eq!(
        touches(&a, &b, &TOL, &Budget::new(needed - 1)),
        Err(KernelError::TooComplex)
    );
    assert_eq!(touches(&a, &b, &TOL, &Budget::new(needed)), Ok(false));
}
