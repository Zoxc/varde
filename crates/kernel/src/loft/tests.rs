//! The planned tests of the loft, written out against its signature and
//! ignored until it's built: analytic volumes (a ruled loft's section at
//! each height is the sections' corresponding points interpolated, so its
//! volume is `h·(A₀/3 + A₁/3 + C/6)` with `C` the two outlines' mixed
//! term; a smooth one's by quadrature of its interpolated sections),
//! exact faces where the plan says they're exact, the refusals, and the
//! same bits at 1 and 8 threads. A section with holes can't reach the
//! kernel (a section is one loop): regeneration refuses it, and its
//! tests say so.

#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::f64::consts::{FRAC_1_SQRT_2, PI};

use glam::{DVec2, DVec3};

use super::*;
use crate::mesh::Surface;
use crate::par::assert_deterministic;
use crate::profile::tests::{arc, circle, polygon};
use crate::{Profile, extrude};

const TOL: Tolerance = Tolerance::DEFAULT;

fn v2(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

/// The plane parallel to XY at height `z`, seen from above.
fn at(z: f64) -> Frame {
    Frame {
        origin: v(0.0, 0.0, z),
        ..Frame::XY
    }
}

/// The square of side `side` about the origin, counter-clockwise from its
/// corner at `+x +y`, curves `curve..`.
fn square(side: f64, curve: u64) -> Loop {
    let h = side / 2.0;
    polygon(&[v2(h, h), v2(-h, h), v2(-h, -h), v2(h, -h)], curve)
}

/// The square of side `side` about the origin turned by `angle`
/// counter-clockwise, from its corner that was at `+x +y`.
fn turned_square(side: f64, angle: f64, curve: u64) -> Loop {
    let h = side / 2.0;
    let (s, c) = angle.sin_cos();
    let turn = |p: DVec2| v2(c * p.x - s * p.y, s * p.x + c * p.y);
    polygon(
        &[v2(h, h), v2(-h, h), v2(-h, -h), v2(h, -h)].map(turn),
        curve,
    )
}

/// The circle round `center` of radius `r` as four quarter arcs of
/// `curve` joined at 45°, 135°, 225° and 315°, counter-clockwise from
/// 45°: its vertices in the directions of [`square`]'s corners.
fn diagonal_circle(center: DVec2, r: f64, curve: u64) -> Loop {
    let d = r * FRAC_1_SQRT_2;
    let points = [v2(d, d), v2(-d, d), v2(-d, -d), v2(d, -d)].map(|p| center + p);
    Loop {
        segments: (0..4)
            .map(|i| arc(center, points[i], points[(i + 1) % 4], curve))
            .collect(),
    }
}

fn section(outline: Loop, frame: Frame) -> Section {
    Section::Loop {
        outline,
        frame,
        start: Some(0),
    }
}

/// The loft of `sections`, checked whole and its faces checked against
/// their forms.
fn lofted(sections: &[Section], mode: LoftMode, closed: bool, rails: &[Rail]) -> Solid {
    let solid = loft(sections, mode, closed, rails, 7, &TOL, &Budget::DEFAULT)
        .unwrap_or_else(|e| panic!("{e}"));
    let mesh = solid.mesh().clone();
    mesh.check(&TOL).unwrap();
    mesh.check_faces(&TOL).unwrap();
    solid
}

/// The refusal of the loft of `sections`.
fn refused(sections: &[Section], closed: bool, rails: &[Rail]) -> LoftError {
    match loft(
        sections,
        LoftMode::Ruled,
        closed,
        rails,
        7,
        &TOL,
        &Budget::DEFAULT,
    ) {
        Ok(_) => panic!("lofted"),
        Err(error) => error,
    }
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

/// Five-point Gauss–Legendre nodes and weights on `[-1, 1]`.
const GAUSS: [(f64, f64); 5] = [
    (0.0, 0.568_888_888_888_888_9),
    (-0.538_469_310_105_683, 0.478_628_670_499_366_5),
    (0.538_469_310_105_683, 0.478_628_670_499_366_5),
    (-0.906_179_845_938_664, 0.236_926_885_056_189_1),
    (0.906_179_845_938_664, 0.236_926_885_056_189_1),
];

/// `∫₀¹ f`, by Gauss–Legendre on `pieces` equal pieces.
fn integral(pieces: u32, f: impl Fn(f64) -> f64) -> f64 {
    let mut total = 0.0;
    for k in 0..pieces {
        let (a, b) = (
            f64::from(k) / f64::from(pieces),
            f64::from(k + 1) / f64::from(pieces),
        );
        for (x, w) in GAUSS {
            total += w * (b - a) / 2.0 * f((a + b) / 2.0 + (b - a) / 2.0 * x);
        }
    }
    total
}

/// The volume of a ruled loft `h` high between the outlines `bottom`
/// and `top`, closed curves of `u ∈ [0, 1]` corresponding point to point,
/// each with its derivative: `h·(A₀/3 + A₁/3 + C/6)`, where the section
/// at height `t·h` is `(1 − t)·bottom + t·top`, whose area is
/// `(1 − t)²A₀ + t²A₁ + t(1 − t)C`.
fn ruled_volume(
    h: f64,
    bottom: impl Fn(f64) -> (DVec2, DVec2),
    top: impl Fn(f64) -> (DVec2, DVec2),
) -> f64 {
    let area = |c: &dyn Fn(f64) -> (DVec2, DVec2)| {
        integral(4096, |u| {
            let (p, d) = c(u);
            (p.x * d.y - p.y * d.x) / 2.0
        })
    };
    let mixed = integral(4096, |u| {
        let ((p0, d0), (p1, d1)) = (bottom(u), top(u));
        (p0.x * d1.y + p1.x * d0.y - p0.y * d1.x - p1.y * d0.x) / 2.0
    });
    h * (area(&bottom) / 3.0 + area(&top) / 3.0 + mixed / 6.0)
}

/// The outline of [`square`] of `side` as a curve of `u`, each side a
/// quarter of `u` at even speed.
fn square_curve(side: f64) -> impl Fn(f64) -> (DVec2, DVec2) {
    let h = side / 2.0;
    let corners = [v2(h, h), v2(-h, h), v2(-h, -h), v2(h, -h)];
    move |u| {
        let k = ((u * 4.0).floor() as usize).min(3);
        let s = u * 4.0 - k as f64;
        let (a, b) = (corners[k], corners[(k + 1) % 4]);
        (a + (b - a) * s, (b - a) * 4.0)
    }
}

/// The outline of [`diagonal_circle`] about the origin as a curve of
/// `u`, each quarter by its length.
fn circle_curve(r: f64) -> impl Fn(f64) -> (DVec2, DVec2) {
    move |u| {
        let angle = PI / 4.0 + 2.0 * PI * u;
        let (s, c) = angle.sin_cos();
        (v2(c, s) * r, v2(-s, c) * r * 2.0 * PI)
    }
}

/// The value at `t` of the cubic Hermite interpolation of `values` (at
/// `t = 0, 1, 2, ...`), Catmull–Rom tangents inside and natural at the
/// ends (no second derivative there), as a smooth loft interpolates its
/// sections' corresponding points.
fn hermite(values: &[f64], t: f64) -> (f64, f64) {
    let n = values.len() - 1;
    let mut m: Vec<f64> = (0..=n)
        .map(|i| {
            if i == 0 || i == n {
                0.0
            } else {
                (values[i + 1] - values[i - 1]) / 2.0
            }
        })
        .collect();
    if n >= 2 {
        m[0] = (3.0 * (values[1] - values[0]) - m[1]) / 2.0;
        m[n] = (3.0 * (values[n] - values[n - 1]) - m[n - 1]) / 2.0;
    } else {
        m[0] = values[1] - values[0];
        m[1] = m[0];
    }
    let i = (t.floor() as usize).min(n - 1);
    let s = t - i as f64;
    let (s2, s3) = (s * s, s * s * s);
    let value = (2.0 * s3 - 3.0 * s2 + 1.0) * values[i]
        + (s3 - 2.0 * s2 + s) * m[i]
        + (-2.0 * s3 + 3.0 * s2) * values[i + 1]
        + (s3 - s2) * m[i + 1];
    let slope = (6.0 * s2 - 6.0 * s) * values[i]
        + (3.0 * s2 - 4.0 * s + 1.0) * m[i]
        + (-6.0 * s2 + 6.0 * s) * values[i + 1]
        + (3.0 * s2 - 2.0 * s) * m[i + 1];
    (value, slope)
}

/// A square of side 20 to one of side 10, 15 above it: a frustum, every
/// face a plane, its volume `h/3·(A₀ + A₁ + √(A₀A₁))`.
#[test]
#[ignore = "kernel loft not built"]
fn a_square_to_a_smaller_square_is_a_frustum() {
    let solid = lofted(
        &[
            section(square(20.0, 1), at(0.0)),
            section(square(10.0, 5), at(15.0)),
        ],
        LoftMode::Ruled,
        false,
        &[],
    );
    assert!(exact(&solid));
    let want = 15.0 / 3.0 * (400.0 + 100.0 + 200.0);
    assert_near(solid.volume(), want, 1e-12, "frustum");
}

/// A square to the same square turned 30°, 10 above it: hyperbolic
/// paraboloids between the sides, exact; each section a square scaled
/// by `|(1 − t) + t·e^{iθ}|`, so the volume is `a²h(2 + cos θ)/3`.
#[test]
#[ignore = "kernel loft not built"]
fn a_square_to_a_turned_square_is_hyperbolic_paraboloids() {
    let turn = PI / 6.0;
    let solid = lofted(
        &[
            section(square(20.0, 1), at(0.0)),
            section(turned_square(20.0, turn, 5), at(10.0)),
        ],
        LoftMode::Ruled,
        false,
        &[],
    );
    assert!(exact(&solid));
    let want = 400.0 * 10.0 * (2.0 + turn.cos()) / 3.0;
    assert_near(solid.volume(), want, 1e-12, "twisted square");
}

/// A circle of radius 5 to one of radius 3 on a plane 8 above, its
/// centre moved 2 along x: an oblique cone's frustum, exact (the plan
/// measured `2e-14` on its strips), `πh(r₀² + r₀r₁ + r₁²)/3`.
#[test]
#[ignore = "kernel loft not built"]
fn a_circle_to_a_smaller_offset_circle_is_an_oblique_cone() {
    let solid = lofted(
        &[
            section(circle(v2(0.0, 0.0), 5.0, 1, false), at(0.0)),
            section(circle(v2(2.0, 0.0), 3.0, 2, false), at(8.0)),
        ],
        LoftMode::Ruled,
        false,
        &[],
    );
    assert!(exact(&solid));
    let want = PI * 8.0 * (25.0 + 15.0 + 9.0) / 3.0;
    assert_near(solid.volume(), want, 1e-12, "oblique cone frustum");
}

/// A circle of radius 4 to a point 9 above its centre: a cone,
/// `πr²h/3`, its wall exact.
#[test]
#[ignore = "kernel loft not built"]
fn a_circle_to_a_point_is_a_cone() {
    let solid = lofted(
        &[
            section(circle(v2(0.0, 0.0), 4.0, 1, false), at(0.0)),
            Section::Point(v(0.0, 0.0, 9.0)),
        ],
        LoftMode::Ruled,
        false,
        &[],
    );
    let want = PI * 16.0 * 9.0 / 3.0;
    assert_near(solid.volume(), want, 1e-9, "cone");
    // Its two points the other way round: the same cone upside down.
    let solid = lofted(
        &[
            Section::Point(v(0.0, 0.0, -9.0)),
            section(circle(v2(0.0, 0.0), 4.0, 1, false), at(0.0)),
        ],
        LoftMode::Ruled,
        false,
        &[],
    );
    assert_near(solid.volume(), want, 1e-9, "cone from its apex");
}

/// A square of side 20 to a circle of radius 10, 12 above, both of four
/// pieces from the `+x +y` corner (so vertex meets vertex), each piece's
/// points meeting at equal fractions of their lengths: fitted strips,
/// the volume within the tolerance of the integral of the interpolated
/// sections' areas.
#[test]
#[ignore = "kernel loft not built"]
fn a_square_to_a_circle_is_fitted() {
    let solid = lofted(
        &[
            section(square(20.0, 1), at(0.0)),
            section(diagonal_circle(v2(0.0, 0.0), 10.0, 5), at(12.0)),
        ],
        LoftMode::Ruled,
        false,
        &[],
    );
    assert!(!exact(&solid));
    let want = ruled_volume(12.0, square_curve(20.0), circle_curve(10.0));
    // Within the fit tolerance over the wall's area (about 4·20·12).
    assert!(
        (solid.volume() - want).abs() <= TOL.fit() * 1200.0,
        "{} for {want}",
        solid.volume()
    );
}

/// Three squares, sides 20, 30 and 20 at heights 0, 10 and 20: ruled,
/// two frustums, exact; smooth, the sections' sides interpolated by the
/// cubic through them (natural at the ends), within the tolerance.
#[test]
#[ignore = "kernel loft not built"]
fn three_sections_ruled_and_smooth() {
    let sections = [
        section(square(20.0, 1), at(0.0)),
        section(square(30.0, 5), at(10.0)),
        section(square(20.0, 9), at(20.0)),
    ];
    let ruled = lofted(&sections, LoftMode::Ruled, false, &[]);
    assert!(exact(&ruled));
    let frustum = 10.0 / 3.0 * (400.0 + 900.0 + 600.0);
    assert_near(ruled.volume(), 2.0 * frustum, 1e-12, "ruled");
    let smooth = lofted(&sections, LoftMode::Smooth, false, &[]);
    let sides = [20.0, 30.0, 20.0];
    let heights = [0.0, 10.0, 20.0];
    let want = 2.0
        * integral(64, |u| {
            let t = 2.0 * u;
            let (side, _) = hermite(&sides, t);
            let (_, rise) = hermite(&heights, t);
            side * side * rise
        });
    assert!(
        (smooth.volume() - want).abs() <= TOL.fit() * 2000.0,
        "smooth: {} for {want}",
        smooth.volume()
    );
    // Smooth through the middle section: more than the ruled one.
    assert!(smooth.volume() > ruled.volume());
}

/// Four squares of side 4 standing on the planes through the z axis at
/// 0°, 90°, 180° and 270°, their centres 10 out, lofted closed and
/// ruled: a square frame (its top and bottom planes, its sides planes
/// between parallel rulings), `4Ra²`, and no caps.
#[test]
#[ignore = "kernel loft not built"]
fn a_closed_loft_is_a_frame() {
    let (r, a) = (10.0, 4.0);
    let sections: Vec<Section> = (0..4u32)
        .map(|k| {
            let angle = PI / 2.0 * f64::from(k);
            let (s, c) = angle.sin_cos();
            let x = v(c, s, 0.0);
            section(
                square(a, 1 + 4 * u64::from(k)),
                Frame {
                    origin: x * r,
                    x,
                    y: DVec3::Z,
                },
            )
        })
        .collect();
    let solid = lofted(&sections, LoftMode::Ruled, true, &[]);
    assert!(exact(&solid));
    assert_near(solid.volume(), 4.0 * r * a * a, 1e-12, "frame");
}

/// Three equal squares at heights 0, 10 and 20, smooth, with a rail
/// straight up through their corners at `-x -y`: a box.
#[test]
#[ignore = "kernel loft not built"]
fn a_rail_through_the_corners_keeps_a_box() {
    let sections = [
        section(square(20.0, 1), at(0.0)),
        section(square(20.0, 5), at(10.0)),
        section(square(20.0, 9), at(20.0)),
    ];
    let rail = Rail {
        conics: vec![Conic3::line(v(-10.0, -10.0, 0.0), v(-10.0, -10.0, 20.0)).unwrap()],
    };
    let solid = lofted(&sections, LoftMode::Smooth, false, &[rail]);
    assert_near(solid.volume(), 400.0 * 20.0, 1e-9, "box");
}

/// Starts half a turn apart (every ruling through the axis), two
/// sections on one plane, sections crossing, a rail missing a section:
/// each refused as such.
#[test]
#[ignore = "kernel loft not built"]
fn twists_planes_crossings_and_rails_are_refused() {
    let twisted = [
        section(square(20.0, 1), at(0.0)),
        Section::Loop {
            outline: square(20.0, 5),
            frame: at(10.0),
            start: Some(2),
        },
    ];
    assert_eq!(refused(&twisted, false, &[]), LoftError::Twists);
    let flat = [
        section(square(20.0, 1), at(0.0)),
        section(square(10.0, 5), at(0.0)),
    ];
    assert_eq!(
        refused(&flat, false, &[]),
        LoftError::OnePlane { section: 0 }
    );
    let crossing = [
        section(square(20.0, 1), at(0.0)),
        section(
            square(20.0, 5),
            Frame {
                origin: DVec3::ZERO,
                x: DVec3::Y,
                y: DVec3::Z,
            },
        ),
    ];
    assert_eq!(refused(&crossing, false, &[]), LoftError::IntoItself);
    let box_sections = [
        section(square(20.0, 1), at(0.0)),
        section(square(20.0, 5), at(10.0)),
    ];
    let off = Rail {
        conics: vec![Conic3::line(v(-10.0, -10.0, 0.0), v(-9.0, -10.0, 10.0)).unwrap()],
    };
    assert_eq!(
        refused(&box_sections, false, &[off]),
        LoftError::RailMisses {
            rail: 0,
            section: 1
        }
    );
}

/// A ruled and a smooth loft come out to the same bits on 1 and 8
/// threads.
#[test]
#[ignore = "kernel loft not built"]
fn lofts_are_the_same_on_any_thread_count() {
    let sections = [
        section(square(20.0, 1), at(0.0)),
        section(diagonal_circle(v2(1.0, 0.0), 8.0, 5), at(10.0)),
        section(square(12.0, 9), at(20.0)),
    ];
    for mode in [LoftMode::Ruled, LoftMode::Smooth] {
        assert_deterministic(|| {
            let solid = loft(&sections, mode, false, &[], 7, &TOL, &Budget::DEFAULT).unwrap();
            (solid.volume(), solid)
        });
    }
}

/// The stand-in fails every loft as too complex, until the kernel's is
/// built: even two equal squares, which make a box.
#[test]
fn the_stand_in_is_too_complex() {
    let sections = [
        section(square(20.0, 1), at(0.0)),
        section(square(20.0, 5), at(10.0)),
    ];
    assert!(matches!(
        loft(
            &sections,
            LoftMode::Ruled,
            false,
            &[],
            7,
            &TOL,
            &Budget::DEFAULT
        ),
        Err(LoftError::Failed(Failure {
            error: KernelError::TooComplex,
            ..
        }))
    ));
    // What the box would be: its first section extruded.
    let profile = Profile {
        loops: vec![square(20.0, 1)],
    };
    let solid = extrude(&profile, &at(0.0), 0.0, 10.0, 7, &TOL, &Budget::DEFAULT).unwrap();
    assert_near(solid.volume(), 4000.0, 1e-12, "the box");
}

/// Sections are on one plane when each one's vertices are within the
/// resolution of the other's plane, whichever way it faces; a point on a
/// loop's plane is on one plane with it; two points never are.
#[test]
fn sections_on_one_plane_are_told() {
    let resolution = TOL.resolution();
    let low = section(square(20.0, 1), at(0.0));
    let flipped = section(
        square(10.0, 5),
        Frame {
            origin: v(3.0, 0.0, 0.5 * resolution),
            x: DVec3::X,
            y: DVec3::NEG_Y,
        },
    );
    assert!(on_one_plane(&low, &flipped, resolution));
    assert!(on_one_plane(&flipped, &low, resolution));
    let above = section(square(20.0, 5), at(2.0 * resolution));
    assert!(!on_one_plane(&low, &above, resolution));
    // Tilted through the first's middle: its vertices off the plane.
    let tilted = section(
        square(10.0, 5),
        Frame {
            origin: DVec3::ZERO,
            x: DVec3::X,
            y: v(0.0, FRAC_1_SQRT_2, FRAC_1_SQRT_2),
        },
    );
    assert!(!on_one_plane(&low, &tilted, resolution));
    let point = Section::Point(v(4.0, -2.0, 0.0));
    assert!(on_one_plane(&low, &point, resolution));
    assert!(on_one_plane(&point, &low, resolution));
    assert!(!on_one_plane(&above, &point, resolution));
    assert!(!on_one_plane(&point, &point.clone(), resolution));
    // A circle's vertices are its arcs' ends.
    let disc = section(circle(v2(0.0, 0.0), 3.0, 9, false), at(0.0));
    assert!(on_one_plane(&disc, &low, resolution));
    assert_eq!(disc.vertices().len(), 4);
    assert_eq!(point.vertices(), [v(4.0, -2.0, 0.0)]);
}

/// The references the ignored tests measure against agree with closed
/// forms: the ruled volume between two squares is the frustum's, between
/// two equal circles a cylinder's; the interpolation of evenly spaced
/// heights is linear, and runs through its values.
#[test]
fn the_references_agree_with_closed_forms() {
    let frustum = ruled_volume(15.0, square_curve(20.0), square_curve(10.0));
    assert_near(frustum, 15.0 / 3.0 * 700.0, 1e-12, "frustum");
    let cylinder = ruled_volume(5.0, circle_curve(3.0), circle_curve(3.0));
    assert_near(cylinder, PI * 9.0 * 5.0, 1e-12, "cylinder");
    for t in [0.0, 0.25, 1.0, 1.5, 2.0] {
        let (z, rise) = hermite(&[0.0, 10.0, 20.0], t);
        assert_near(z.max(1e-300), (10.0 * t).max(1e-300), 1e-12, "height");
        assert_near(rise, 10.0, 1e-12, "rise");
    }
    assert_eq!(hermite(&[20.0, 30.0, 20.0], 1.0).0, 30.0);
}
