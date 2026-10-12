//! The planned tests of the tapered extrude, written out against its
//! signature and ignored until it's built: analytic volumes (frustums of
//! pyramids and cones, a tapered slot's integral), exact faces where the
//! plan says they're exact, the refusals, and the same bits at 1 and 8
//! threads. The stand-in's own behaviour (zero is the extrude, anything
//! else too complex) is tested too, until it's replaced.

#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::f64::consts::PI;

use glam::{DVec2, DVec3};

use super::*;
use crate::mesh::{Form, Surface};
use crate::par::assert_deterministic;
use crate::profile::tests::{arc, circle, rect};
use crate::{Loop, Op, Segment, boolean, trig};

const TOL: Tolerance = Tolerance::DEFAULT;

const XY: Frame = Frame::XY;

fn v2(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn profile(loops: Vec<Loop>) -> Profile {
    Profile { loops }
}

/// The square of side `2 half` centred on the origin.
fn square(half: f64) -> Profile {
    profile(vec![rect(v2(-half, -half), v2(half, half), 1)])
}

/// The circle of `radius` round the origin.
fn disc(radius: f64) -> Profile {
    profile(vec![circle(DVec2::ZERO, radius, 1, false)])
}

/// A slot along x: straight sides `length` long, round ends of `radius`
/// (each two quarter arcs), centred on the origin.
fn slot(length: f64, radius: f64) -> Profile {
    let (a, r) = (length / 2.0, radius);
    let line = |p: DVec2, q: DVec2, curve| Segment::line(p, q, curve).unwrap();
    let right = v2(a, 0.0);
    let left = v2(-a, 0.0);
    let segments = vec![
        line(v2(-a, -r), v2(a, -r), 1),
        arc(right, v2(a, -r), v2(a + r, 0.0), 2),
        arc(right, v2(a + r, 0.0), v2(a, r), 2),
        line(v2(a, r), v2(-a, r), 3),
        arc(left, v2(-a, r), v2(-a - r, 0.0), 4),
        arc(left, v2(-a - r, 0.0), v2(-a, -r), 4),
    ];
    profile(vec![Loop { segments }])
}

fn tan(angle: f64) -> f64 {
    let (sin, cos) = trig::sin_cos(angle);
    sin / cos
}

/// `profile` on `frame` from `from` to `to` tapered by `taper`, checked
/// whole and its faces checked against their forms.
fn tapered(profile: &Profile, frame: &Frame, from: f64, to: f64, taper: f64) -> Solid {
    let solid = extrude_tapered(profile, frame, from, to, taper, 7, &TOL, &Budget::DEFAULT)
        .unwrap_or_else(|e| panic!("{e}"));
    let mesh = solid.mesh().clone();
    mesh.check(&TOL).unwrap();
    mesh.check_faces(&TOL).unwrap();
    solid
}

/// Whether `solid`'s faces all claim a form (none fitted).
fn exact(solid: &Solid) -> bool {
    (solid.mesh().faces().iter()).all(|face| !matches!(face.surface, Surface::Free))
}

fn assert_near(got: f64, want: f64, relative: f64, what: &str) {
    assert!(
        (got - want).abs() <= relative * want.abs(),
        "{what}: {got} for {want} ({:e})",
        (got - want) / want
    );
}

/// The volume of a frustum `h` high between parallel faces of areas
/// `a` and `b` similar to each other.
fn frustum(h: f64, a: f64, b: f64) -> f64 {
    h / 3.0 * (a + b + (a * b).sqrt())
}

/// The volume of a square of side `2 half` tapered by `t = tan α` over
/// `h` away from its plane.
fn square_frustum(half: f64, t: f64, h: f64) -> f64 {
    let top = 2.0 * (half - h * t);
    frustum(h, 4.0 * half * half, top * top)
}

#[test]
fn no_taper_is_the_extrude() {
    let profile = square(2.0);
    let budget = Budget::DEFAULT;
    let plain = crate::extrude(&profile, &XY, -1.0, 3.0, 7, &TOL, &budget).unwrap();
    let solid = extrude_tapered(&profile, &XY, -1.0, 3.0, 0.0, 7, &TOL, &budget).unwrap();
    assert_eq!(format!("{plain:?}"), format!("{solid:?}"));
}

#[test]
fn a_taper_is_not_implemented_until_the_kernel_drafts_walls() {
    let failed = extrude_tapered(&square(2.0), &XY, 0.0, 3.0, 0.1, 7, &TOL, &Budget::DEFAULT);
    assert!(
        matches!(&failed, Err(TaperError::Failed(f)) if matches!(f.error, KernelError::NotImplemented(_))),
        "{failed:?}"
    );
}

#[test]
#[ignore = "kernel taper not built"]
fn a_tapered_square_boss_is_a_pyramid_frustum() {
    let angle = 10f64.to_radians();
    let solid = tapered(&square(2.0), &XY, 0.0, 5.0, angle);
    assert_near(
        solid.volume(),
        square_frustum(2.0, tan(angle), 5.0),
        1e-12,
        "volume",
    );
    assert!(exact(&solid));
    // Four walls, each a plane leaning in.
    let walls = (solid.mesh().faces().iter())
        .filter(|face| matches!(face.form, Form::Plane { n, .. } if n.z.abs() < 0.99))
        .count();
    assert!(walls >= 4, "{walls} leaning walls");
}

#[test]
#[ignore = "kernel taper not built"]
fn a_tapered_circle_is_a_cone_frustum() {
    let angle = 15f64.to_radians();
    let (r, h) = (3.0, 4.0);
    let solid = tapered(&disc(r), &XY, 0.0, h, angle);
    let top = r - h * tan(angle);
    assert_near(
        solid.volume(),
        PI * h / 3.0 * (r * r + r * top + top * top),
        1e-12,
        "volume",
    );
    assert!(exact(&solid));
    assert!(
        (solid.mesh().faces().iter()).any(|face| matches!(face.form, Form::Cone { .. })),
        "the wall is an exact cone"
    );
}

#[test]
#[ignore = "kernel taper not built"]
fn a_negative_taper_widens() {
    let angle = -8f64.to_radians();
    let solid = tapered(&square(2.0), &XY, 0.0, 5.0, angle);
    let want = square_frustum(2.0, tan(angle), 5.0);
    assert!(want > 80.0, "wider than the straight extrude's 80");
    assert_near(solid.volume(), want, 1e-12, "volume");
}

#[test]
#[ignore = "kernel taper not built"]
fn two_sides_narrow_away_from_the_sketch_both_ways() {
    let angle = 5f64.to_radians();
    let t = tan(angle);
    let solid = tapered(&square(2.0), &XY, -3.0, 5.0, angle);
    let want = square_frustum(2.0, t, 5.0) + square_frustum(2.0, t, 3.0);
    assert_near(solid.volume(), want, 1e-12, "volume");
    // The walls are split at the sketch's plane: eight wall faces.
    let walls = (solid.mesh().faces().iter())
        .filter(|face| matches!(face.form, Form::Plane { n, .. } if n.z.abs() < 0.99))
        .count();
    assert_eq!(walls, 8);
}

#[test]
#[ignore = "kernel taper not built"]
fn a_symmetric_taper_is_two_equal_frustums() {
    let angle = 5f64.to_radians();
    let solid = tapered(&disc(2.0), &XY, -4.0, 4.0, angle);
    let top = 2.0 - 4.0 * tan(angle);
    let half = PI * 4.0 / 3.0 * (4.0 + 2.0 * top + top * top);
    assert_near(solid.volume(), 2.0 * half, 1e-12, "volume");
}

#[test]
#[ignore = "kernel taper not built"]
fn a_tapered_slot_has_planes_and_cones() {
    let angle = 6f64.to_radians();
    let (length, r, h) = (8.0, 2.0, 5.0);
    let t = tan(angle);
    let solid = tapered(&slot(length, r), &XY, 0.0, h, angle);
    // At height z the slot is offset in by z t: straight sides 2 (r − z t)
    // apart and length long, round ends of radius r − z t.
    let want = 2.0 * length * (r * h - t * h * h / 2.0)
        + PI * (r * r * h - r * t * h * h + t * t * h * h * h / 3.0);
    assert_near(solid.volume(), want, 1e-12, "volume");
    assert!(exact(&solid));
    let cones = (solid.mesh().faces().iter())
        .filter(|face| matches!(face.form, Form::Cone { .. }))
        .count();
    assert!(cones >= 2, "{cones} cones");
}

/// A plate 20 × 20 × 10 on XY, and the frame on its top face.
fn plate() -> (Solid, Frame) {
    let budget = Budget::DEFAULT;
    let plate = crate::extrude(&square(10.0), &XY, 0.0, 10.0, 1, &TOL, &budget).unwrap();
    let top = Frame {
        origin: DVec3::new(0.0, 0.0, 10.0),
        ..XY
    };
    (plate, top)
}

#[test]
#[ignore = "kernel taper not built"]
fn a_tapered_pocket_narrows_into_the_plate() {
    let (plate, top) = plate();
    let angle = 10f64.to_radians();
    let tool = tapered(&square(3.0), &top, -4.0, 0.0, angle);
    let pocket = boolean(&plate, &tool, Op::Difference, &TOL, &Budget::DEFAULT).unwrap();
    let want = 4000.0 - square_frustum(3.0, tan(angle), 4.0);
    assert_near(pocket.volume(), want, 1e-12, "volume");
}

#[test]
#[ignore = "kernel taper not built"]
fn a_tapered_cut_through_a_plate_narrows_into_it() {
    let (plate, top) = plate();
    let angle = 10f64.to_radians();
    let t = tan(angle);
    let tool = tapered(&square(3.0), &top, -11.0, 0.0, angle);
    let cut = boolean(&plate, &tool, Op::Difference, &TOL, &Budget::DEFAULT).unwrap();
    // The hole is 6 wide at the top and 6 − 20 t at the bottom.
    let want = 4000.0 - square_frustum(3.0, t, 10.0);
    assert_near(cut.volume(), want, 1e-12, "volume");
}

#[test]
#[ignore = "kernel taper not built"]
fn a_taper_closing_a_narrow_profile_is_refused() {
    // 1 wide: closed 0.5 / tan 10° ≈ 2.8 up.
    let narrow = profile(vec![rect(v2(-5.0, -0.5), v2(5.0, 0.5), 1)]);
    let angle = 10f64.to_radians();
    let failed = extrude_tapered(&narrow, &XY, 0.0, 10.0, angle, 7, &TOL, &Budget::DEFAULT);
    assert_eq!(failed.unwrap_err(), TaperError::Closes);
    // A circle shrinking to nothing likewise.
    let failed = extrude_tapered(&disc(1.0), &XY, 0.0, 10.0, angle, 7, &TOL, &Budget::DEFAULT);
    assert_eq!(failed.unwrap_err(), TaperError::Closes);
}

#[test]
#[ignore = "kernel taper not built"]
fn a_taper_widening_out_of_range_is_refused() {
    let angle = -89f64.to_radians();
    let max = f64::from(crate::MAX_COORD);
    let failed = extrude_tapered(
        &square(1.0),
        &XY,
        0.0,
        max / 2.0,
        angle,
        7,
        &TOL,
        &Budget::DEFAULT,
    );
    assert_eq!(failed.unwrap_err(), TaperError::OutOfRange);
}

#[test]
#[ignore = "kernel taper not built"]
fn tapers_are_the_same_on_any_thread_count() {
    let angle = 7f64.to_radians();
    assert_deterministic(|| {
        let solid = extrude_tapered(
            &slot(6.0, 1.5),
            &XY,
            -2.0,
            4.0,
            angle,
            7,
            &TOL,
            &Budget::DEFAULT,
        )
        .unwrap();
        (solid.volume(), solid)
    });
}
