//! The planned tests of the path sweep, written out against its
//! signature and ignored until it's built: analytic volumes (Pappus: the
//! profile's area times the path's length for a profile whose centroid
//! is on the path; a helix's area times `2π·x̄` times its turns), exact
//! faces where the plan says they're exact, the refusals, and the same
//! bits at 1 and 8 threads.

#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::f64::consts::{FRAC_1_SQRT_2, PI};

use glam::{DVec2, DVec3};

use super::*;
use crate::mesh::Surface;
use crate::par::assert_deterministic;
use crate::profile::tests::{circle, polygon, rect};
use crate::{Loop, Op, boolean, extrude};

const TOL: Tolerance = Tolerance::DEFAULT;

/// The world's XY plane, the profile's plane for paths starting up +z.
const XY: Frame = Frame::XY;

fn v2(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

fn profile(loops: Vec<Loop>) -> Profile {
    Profile { loops }
}

/// The square of side 2 centred on the origin.
fn square() -> Profile {
    profile(vec![rect(v2(-1.0, -1.0), v2(1.0, 1.0), 1)])
}

/// The circle of `radius` round the origin.
fn disc(radius: f64) -> Profile {
    profile(vec![circle(DVec2::ZERO, radius, 1, false)])
}

fn line(from: DVec3, to: DVec3) -> Piece {
    Piece::Line { from, to }
}

/// The arc round `centre` from `a` to `b` (at most a quarter turn each
/// part), split into `parts` equal conics by halving the angle: `parts`
/// is 1 or 2.
fn arc(centre: DVec3, a: DVec3, b: DVec3, parts: u32) -> Piece {
    let radius = (a - centre).length();
    let axis = (a - centre).cross(b - centre).normalize();
    let conics = match parts {
        1 => vec![Conic3::arc_between(centre, radius, a, b).unwrap()],
        _ => {
            let middle = centre + ((a - centre) + (b - centre)).normalize() * radius;
            vec![
                Conic3::arc_between(centre, radius, a, middle).unwrap(),
                Conic3::arc_between(centre, radius, middle, b).unwrap(),
            ]
        }
    };
    Piece::Arc {
        conics,
        centre,
        axis,
    }
}

/// A half turn round `centre` from `a`, turning right-handed about
/// `axis`: two quarter arcs.
fn half_turn(centre: DVec3, a: DVec3, axis: DVec3) -> Piece {
    let u = a - centre;
    let middle = centre + axis.normalize().cross(u);
    let b = centre - u;
    let conics = vec![
        Conic3::arc_between(centre, u.length(), a, middle).unwrap(),
        Conic3::arc_between(centre, u.length(), middle, b).unwrap(),
    ];
    Piece::Arc {
        conics,
        centre,
        axis: axis.normalize(),
    }
}

fn chain(pieces: Vec<Piece>) -> Path {
    Path::Chain {
        pieces,
        closed: false,
    }
}

/// `profile` on `frame` swept along `path`, checked whole and its faces
/// checked against their forms.
fn swept(
    profile: &Profile,
    frame: &Frame,
    path: &Path,
    orientation: Orientation,
    twist: f64,
) -> Solid {
    let solid = sweep(
        profile,
        frame,
        path,
        orientation,
        twist,
        7,
        &TOL,
        &Budget::DEFAULT,
    )
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

/// The length of `conics`, by Gauss–Legendre quadrature on each conic
/// halved until it's well within `1e-9` of itself.
fn length(conics: &[Conic3]) -> f64 {
    const NODES: [(f64, f64); 5] = [
        (0.0, 0.568_888_888_888_888_9),
        (-0.538_469_310_105_683, 0.478_628_670_499_366_5),
        (0.538_469_310_105_683, 0.478_628_670_499_366_5),
        (-0.906_179_845_938_664, 0.236_926_885_056_189_1),
        (0.906_179_845_938_664, 0.236_926_885_056_189_1),
    ];
    let mut total = 0.0;
    for conic in conics {
        let pieces = 256;
        for k in 0..pieces {
            let (a, b) = (k as f64 / pieces as f64, (k + 1) as f64 / pieces as f64);
            for (x, w) in NODES {
                let t = (a + b) / 2.0 + (b - a) / 2.0 * x;
                total += w * (b - a) / 2.0 * conic.eval_deriv(t).1.length();
            }
        }
    }
    total
}

/// The length of `path`'s pieces.
fn path_length(path: &Path) -> f64 {
    let Path::Chain { pieces, .. } = path else {
        panic!("a chain");
    };
    pieces
        .iter()
        .map(|piece| match piece {
            Piece::Line { from, to } => from.distance(*to),
            Piece::Arc { conics, .. } | Piece::Curve { conics, .. } => length(conics),
        })
        .sum()
}

fn assert_near(got: f64, want: f64, relative: f64, what: &str) {
    assert!(
        (got - want).abs() <= relative * want.abs(),
        "{what}: {got} for {want} ({:e})",
        (got - want) / want
    );
}

/// Up +z from the origin 5, a quarter bend toward +x of radius 5, then 5
/// along +x: in the XZ plane.
fn bent() -> Path {
    chain(vec![
        line(v(0.0, 0.0, 0.0), v(0.0, 0.0, 5.0)),
        arc(v(5.0, 0.0, 5.0), v(0.0, 0.0, 5.0), v(5.0, 0.0, 10.0), 1),
        line(v(5.0, 0.0, 10.0), v(10.0, 0.0, 10.0)),
    ])
}

/// Along +x from the origin 5, a quarter bend in XY of radius 3 toward
/// +y, 3 along +y, a quarter bend in the plane `x = 8` of radius 3
/// toward +z, then 5 up +z: a pipe's route through two planes.
fn routed() -> Path {
    chain(vec![
        line(v(0.0, 0.0, 0.0), v(5.0, 0.0, 0.0)),
        arc(v(5.0, 3.0, 0.0), v(5.0, 0.0, 0.0), v(8.0, 3.0, 0.0), 1),
        line(v(8.0, 3.0, 0.0), v(8.0, 6.0, 0.0)),
        arc(v(8.0, 6.0, 3.0), v(8.0, 6.0, 0.0), v(8.0, 9.0, 3.0), 1),
        line(v(8.0, 9.0, 3.0), v(8.0, 9.0, 8.0)),
    ])
}

/// The YZ plane at the origin, its normal +x: [`routed`]'s profile plane.
const YZ: Frame = Frame {
    origin: DVec3::ZERO,
    x: DVec3::Y,
    y: DVec3::Z,
};

/// The XZ plane, `x` along the world's z and `y` along its x (so `x ×
/// y` is +y): a helix's profile plane, holding the z axis, a profile's
/// `(u, v)` at height `u` and `v` out from it.
const XZ: Frame = Frame {
    origin: DVec3::ZERO,
    x: DVec3::Z,
    y: DVec3::X,
};

// Task 55: paths in one plane.

#[test]
#[ignore = "kernel sweep not built"]
fn a_rectangle_along_a_line_is_a_box() {
    let path = chain(vec![line(DVec3::ZERO, v(0.0, 0.0, 10.0))]);
    let solid = swept(&square(), &XY, &path, Orientation::Follow, 0.0);
    assert_near(solid.volume(), 40.0, 1e-12, "volume");
    assert_near(solid.area(), 4.0 * 2.0 * 10.0 + 2.0 * 4.0, 1e-12, "area");
    assert!(exact(&solid));
}

#[test]
#[ignore = "kernel sweep not built"]
fn a_rectangle_along_an_arc_is_a_revolve() {
    let path = chain(vec![arc(
        v(5.0, 0.0, 0.0),
        DVec3::ZERO,
        v(5.0, 0.0, 5.0),
        1,
    )]);
    let solid = swept(&square(), &XY, &path, Orientation::Follow, 0.0);
    // Pappus: the area times the centroid's path, a quarter of radius 5.
    assert_near(solid.volume(), 4.0 * 5.0 * PI / 2.0, 1e-10, "volume");
    assert!(exact(&solid));
}

#[test]
#[ignore = "kernel sweep not built"]
fn a_bent_bar_is_exact() {
    let path = bent();
    let solid = swept(&square(), &XY, &path, Orientation::Follow, 0.0);
    let length = 10.0 + 5.0 * PI / 2.0;
    assert_near(solid.volume(), 4.0 * length, 1e-10, "volume");
    assert!(exact(&solid));
}

#[test]
#[ignore = "kernel sweep not built"]
fn a_pipe_along_a_bent_path_is_cylinders_and_a_torus() {
    let path = bent();
    let solid = swept(&disc(1.0), &XY, &path, Orientation::Follow, 0.0);
    let length = 10.0 + 5.0 * PI / 2.0;
    // The bend's torus is fitted, within the tolerance.
    assert_near(solid.volume(), PI * length, 1e-5, "volume");
    let cylinders = (solid.mesh().faces().iter())
        .filter(|face| matches!(face.surface, Surface::Quadric(_)))
        .count();
    assert!(cylinders >= 2, "{cylinders} exact round faces");
}

/// A closed rounded rectangle in the XZ plane through the origin, up
/// its left side there: 2·`a` straight up the left and right, `l`
/// across the top and bottom, corners of radius `r`, starting at the
/// left side's foot (not on the profile's plane: the kernel splits the
/// left side there).
fn rounded_rectangle(a: f64, l: f64, r: f64) -> Path {
    let pieces = vec![
        line(v(0.0, 0.0, -a), v(0.0, 0.0, a)),
        arc(v(r, 0.0, a), v(0.0, 0.0, a), v(r, 0.0, a + r), 1),
        line(v(r, 0.0, a + r), v(r + l, 0.0, a + r)),
        arc(
            v(r + l, 0.0, a),
            v(r + l, 0.0, a + r),
            v(2.0 * r + l, 0.0, a),
            1,
        ),
        line(v(2.0 * r + l, 0.0, a), v(2.0 * r + l, 0.0, -a)),
        arc(
            v(r + l, 0.0, -a),
            v(2.0 * r + l, 0.0, -a),
            v(r + l, 0.0, -a - r),
            1,
        ),
        line(v(r + l, 0.0, -a - r), v(r, 0.0, -a - r)),
        arc(v(r, 0.0, -a), v(r, 0.0, -a - r), v(0.0, 0.0, -a), 1),
    ];
    Path::Chain {
        pieces,
        closed: true,
    }
}

#[test]
#[ignore = "kernel sweep not built"]
fn a_closed_rounded_rectangle_is_a_frame() {
    let path = rounded_rectangle(4.0, 6.0, 3.0);
    let solid = swept(&square(), &XY, &path, Orientation::Follow, 0.0);
    let length = 4.0 * 4.0 + 2.0 * 6.0 + 2.0 * PI * 3.0;
    assert_near(solid.volume(), 4.0 * length, 1e-10, "volume");
    assert!(exact(&solid));
    // No caps: a frame has a hole through it.
    assert_eq!(solid.mesh().check(&TOL), Ok(()));
}

#[test]
#[ignore = "kernel sweep not built"]
fn keeping_orientation_along_an_arc_sweeps_exact_cylinders() {
    // An eighth of a turn of radius 5 from the origin, up +z bending
    // toward +x: short of the quarter turn that would end parallel to
    // the profile's plane.
    let rise = 5.0 * FRAC_1_SQRT_2;
    let end = v(5.0 - rise, 0.0, rise);
    let path = chain(vec![arc(v(5.0, 0.0, 0.0), DVec3::ZERO, end, 1)]);
    let solid = swept(&square(), &XY, &path, Orientation::Keep, 0.0);
    // Translated, never turned: the area times the rise.
    assert_near(solid.volume(), 4.0 * rise, 1e-10, "volume");
    assert!(exact(&solid));
}

#[test]
#[ignore = "kernel sweep not built"]
fn a_spline_path_is_followed_within_the_tolerance() {
    // A parabola in the XZ plane, up +z at the origin.
    let conic = Conic3::new(DVec3::ZERO, v(0.0, 0.0, 5.0), 1.0, v(5.0, 0.0, 10.0)).unwrap();
    let path = chain(vec![Piece::Curve {
        conics: vec![conic],
        normal: Some(DVec3::Y),
    }]);
    let solid = swept(&disc(0.5), &XY, &path, Orientation::Follow, 0.0);
    let length = path_length(&path);
    assert_near(solid.volume(), PI * 0.25 * length, 1e-4, "volume");
}

#[test]
#[ignore = "kernel sweep not built"]
fn a_twist_keeps_the_volume() {
    let path = chain(vec![line(DVec3::ZERO, v(0.0, 0.0, 10.0))]);
    let solid = swept(&square(), &XY, &path, Orientation::Follow, PI / 2.0);
    assert_near(solid.volume(), 40.0, 1e-5, "volume");
}

/// What sweeping the square on XY along `path` is refused with.
fn refused(path: &Path, orientation: Orientation) -> SweepError {
    match sweep(
        &square(),
        &XY,
        path,
        orientation,
        0.0,
        7,
        &TOL,
        &Budget::DEFAULT,
    ) {
        Ok(_) => panic!("swept"),
        Err(error) => error,
    }
}

#[test]
#[ignore = "kernel sweep not built"]
fn corners_and_bad_starts_are_refused() {
    let corner = chain(vec![
        line(DVec3::ZERO, v(0.0, 0.0, 5.0)),
        line(v(0.0, 0.0, 5.0), v(5.0, 0.0, 5.0)),
    ]);
    assert!(matches!(
        refused(&corner, Orientation::Follow),
        SweepError::Corner { at } if at == v(0.0, 0.0, 5.0)
    ));
    let off = chain(vec![line(v(0.0, 0.0, 1.0), v(0.0, 0.0, 5.0))]);
    assert_eq!(refused(&off, Orientation::Follow), SweepError::OffStart);
    let slanted = chain(vec![line(DVec3::ZERO, v(1.0, 0.0, 5.0))]);
    assert_eq!(
        refused(&slanted, Orientation::Follow),
        SweepError::NotSquare
    );
    // A bend of radius 0.5 for a section 1 out from the path.
    let tight = chain(vec![arc(
        v(0.5, 0.0, 0.0),
        DVec3::ZERO,
        v(0.5, 0.0, 0.5),
        1,
    )]);
    assert_eq!(
        refused(&tight, Orientation::Follow),
        SweepError::TooTight { piece: 0 }
    );
}

#[test]
#[ignore = "kernel sweep not built"]
fn a_path_back_into_itself_is_refused() {
    // Up 10, three quarters round a circle of radius 3, then back
    // across the first run at height 7.
    let path = chain(vec![
        line(DVec3::ZERO, v(0.0, 0.0, 10.0)),
        half_turn(v(3.0, 0.0, 10.0), v(0.0, 0.0, 10.0), DVec3::Y),
        arc(v(3.0, 0.0, 10.0), v(6.0, 0.0, 10.0), v(3.0, 0.0, 7.0), 1),
        line(v(3.0, 0.0, 7.0), v(-3.0, 0.0, 7.0)),
    ]);
    assert_eq!(refused(&path, Orientation::Follow), SweepError::IntoItself);
}

#[test]
#[ignore = "kernel sweep not built"]
fn plane_sweeps_are_the_same_on_any_thread_count() {
    let path = bent();
    assert_deterministic(|| {
        let solid = sweep(
            &disc(1.0),
            &XY,
            &path,
            Orientation::Follow,
            0.3,
            7,
            &TOL,
            &Budget::DEFAULT,
        )
        .unwrap();
        (solid.volume(), solid)
    });
}

// Task 56: 3D paths and helices.

#[test]
#[ignore = "kernel sweep not built"]
fn a_pipe_routed_through_two_planes() {
    let path = routed();
    let solid = swept(&disc(1.0), &YZ, &path, Orientation::Follow, 0.0);
    let length = 13.0 + 3.0 * PI;
    assert_near(solid.volume(), PI * length, 1e-5, "volume");
}

#[test]
#[ignore = "kernel sweep not built"]
fn a_rectangle_routed_through_two_planes_is_exact() {
    let path = routed();
    let solid = swept(&square(), &YZ, &path, Orientation::Follow, 0.0);
    let length = 13.0 + 3.0 * PI;
    assert_near(solid.volume(), 4.0 * length, 1e-10, "volume");
    assert!(exact(&solid));
}

#[test]
#[ignore = "kernel sweep not built"]
fn a_pipe_along_an_oblique_ellipse() {
    // The ellipse of semi-axes 8 (along x) and 5 (along the slant (0, c,
    // s)), as four quarter conics, round the origin; the profile at its
    // point on +x, square to it there.
    let (c, s) = (0.6, 0.8);
    let slant = v(0.0, c, s);
    let (a, b) = (8.0, 5.0);
    let corners = [v(a, 0.0, 0.0), slant * b, v(-a, 0.0, 0.0), slant * -b];
    let conics: Vec<Conic3> = (0..4)
        .map(|k| {
            let (p, q) = (corners[k], corners[(k + 1) % 4]);
            Conic3::new(p, p + q, FRAC_1_SQRT_2, q).unwrap()
        })
        .collect();
    let path = Path::Chain {
        pieces: vec![Piece::Curve {
            conics,
            normal: None,
        }],
        closed: true,
    };
    let frame = Frame {
        origin: v(a, 0.0, 0.0),
        x: DVec3::X,
        y: v(0.0, s, -c),
    };
    let solid = swept(&disc(0.5), &frame, &path, Orientation::Follow, 0.0);
    let length = path_length(&path);
    assert_near(solid.volume(), PI * 0.25 * length, 1e-4, "volume");
}

#[test]
#[ignore = "kernel sweep not built"]
fn a_pipe_along_the_rim_where_two_cylinders_cross() {
    // A cylinder of radius 5 up z and one of radius 3 along x, united:
    // the rims where they meet are closed and out of any one plane.
    let up = extrude(&disc(5.0), &XY, -10.0, 10.0, 1, &TOL, &Budget::DEFAULT).unwrap();
    let across = extrude(&disc(3.0), &YZ, -10.0, 10.0, 2, &TOL, &Budget::DEFAULT).unwrap();
    let both = boolean(&up, &across, Op::Union, &TOL, &Budget::DEFAULT).unwrap();
    let topology = both.topology();
    let mesh = both.mesh();
    let rim = (topology.chains().iter())
        .find(|chain| {
            chain.closed && {
                let first = mesh.curve(chain.halfedges[0]);
                first.p0.x > 0.0
            }
        })
        .expect("a closed rim on +x");
    let conics: Vec<Conic3> = rim.halfedges.iter().map(|&h| mesh.curve(h)).collect();
    let start = conics[0];
    let tangent = (start.c - start.p0).normalize();
    let x = tangent.any_orthonormal_vector();
    let frame = Frame {
        origin: start.p0,
        x,
        y: tangent.cross(x),
    };
    let path = Path::Chain {
        pieces: vec![Piece::Curve {
            conics,
            normal: None,
        }],
        closed: true,
    };
    let solid = swept(&disc(0.3), &frame, &path, Orientation::Follow, 0.0);
    let length = path_length(&path);
    assert_near(solid.volume(), PI * 0.09 * length, 1e-3, "volume");
}

/// A helix about the z axis through the origin.
fn helix(pitch: f64, turns: f64, left: bool) -> Path {
    Path::Helix(Helix {
        point: DVec3::ZERO,
        axis: DVec3::Z,
        pitch,
        turns,
        left,
    })
}

#[test]
#[ignore = "kernel sweep not built"]
fn springs_either_way() {
    // A circle of radius 1, 5 out from the axis, ten turns of pitch 4.
    let section = profile(vec![circle(v2(0.0, 5.0), 1.0, 1, false)]);
    for left in [false, true] {
        let solid = swept(
            &section,
            &XZ,
            &helix(4.0, 10.0, left),
            Orientation::Follow,
            0.0,
        );
        let volume = PI * 2.0 * PI * 5.0 * 10.0;
        assert_near(solid.volume(), volume, 1e-4, "volume");
        // The plan's estimate: about 1 300 patches and the caps.
        let patches = solid.mesh().tris().len();
        assert!(patches < 4 * 1_300, "{patches} patches");
    }
}

#[test]
#[ignore = "kernel sweep not built"]
fn a_thread() {
    // A triangle 1 high along the axis, 5 to 6 out from it.
    let section = profile(vec![polygon(
        &[v2(0.0, 5.0), v2(1.0, 5.0), v2(0.5, 6.0)],
        1,
    )]);
    let solid = swept(
        &section,
        &XZ,
        &helix(1.5, 8.0, false),
        Orientation::Follow,
        0.0,
    );
    let (area, centroid) = (0.5, 16.0 / 3.0);
    assert_near(
        solid.volume(),
        area * 2.0 * PI * centroid * 8.0,
        1e-4,
        "volume",
    );
}

#[test]
#[ignore = "kernel sweep not built"]
fn three_d_refusals() {
    // A gap: the second line starts a hair off the first's end.
    let gap = chain(vec![
        line(DVec3::ZERO, v(0.0, 0.0, 5.0)),
        line(v(0.0, 0.1, 5.0), v(0.0, 0.1, 10.0)),
    ]);
    assert!(matches!(
        refused(&gap, Orientation::Follow),
        SweepError::Corner { .. }
    ));
    // Keeping orientation along a quarter turn ends parallel to the
    // profile's plane.
    let quarter = chain(vec![arc(
        v(5.0, 0.0, 0.0),
        DVec3::ZERO,
        v(5.0, 0.0, 5.0),
        1,
    )]);
    assert_eq!(refused(&quarter, Orientation::Keep), SweepError::Parallel);
    // A route back into its first run: up 10, over and down 5 beside
    // it, under and up again along it.
    let crossing = chain(vec![
        line(DVec3::ZERO, v(0.0, 0.0, 10.0)),
        half_turn(v(0.0, 2.0, 10.0), v(0.0, 0.0, 10.0), DVec3::NEG_X),
        line(v(0.0, 4.0, 10.0), v(0.0, 4.0, 5.0)),
        half_turn(v(0.0, 2.0, 5.0), v(0.0, 4.0, 5.0), DVec3::NEG_X),
        line(v(0.0, 0.0, 5.0), v(0.0, 0.0, 8.0)),
    ]);
    assert_eq!(
        refused(&crossing, Orientation::Follow),
        SweepError::IntoItself
    );
    let helix_refused = |section: Profile, frame: &Frame, path: &Path| match sweep(
        &section,
        frame,
        path,
        Orientation::Follow,
        0.0,
        7,
        &TOL,
        &Budget::DEFAULT,
    ) {
        Ok(_) => panic!("swept"),
        Err(error) => error,
    };
    let ring = profile(vec![circle(v2(0.0, 5.0), 1.0, 1, false)]);
    // The XY plane doesn't hold the z axis.
    assert_eq!(
        helix_refused(ring.clone(), &XY, &helix(4.0, 3.0, false)),
        SweepError::HelixPlane
    );
    // A circle round the axis itself.
    let on_axis = profile(vec![circle(v2(0.0, 0.5), 1.0, 1, false)]);
    assert_eq!(
        helix_refused(on_axis, &XZ, &helix(4.0, 3.0, false)),
        SweepError::ReachesAxis
    );
    // 2 along the axis, a pitch of 1.5.
    assert_eq!(
        helix_refused(ring, &XZ, &helix(1.5, 3.0, false)),
        SweepError::Pitch
    );
}

#[test]
#[ignore = "kernel sweep not built"]
fn three_d_sweeps_are_the_same_on_any_thread_count() {
    let path = routed();
    assert_deterministic(|| {
        let solid = sweep(
            &disc(1.0),
            &YZ,
            &path,
            Orientation::Follow,
            0.0,
            7,
            &TOL,
            &Budget::DEFAULT,
        )
        .unwrap();
        (solid.volume(), solid)
    });
    let section = profile(vec![circle(v2(0.0, 5.0), 1.0, 1, false)]);
    assert_deterministic(|| {
        let solid = sweep(
            &section,
            &XZ,
            &helix(4.0, 2.5, true),
            Orientation::Follow,
            0.0,
            7,
            &TOL,
            &Budget::DEFAULT,
        )
        .unwrap();
        (solid.volume(), solid)
    });
}

/// The stand-in fails every sweep as too complex, until the kernel's is
/// built.
#[test]
fn the_stand_in_is_not_implemented() {
    let path = chain(vec![line(DVec3::ZERO, v(0.0, 0.0, 10.0))]);
    assert!(matches!(
        sweep(
            &square(),
            &XY,
            &path,
            Orientation::Follow,
            0.0,
            7,
            &TOL,
            &Budget::DEFAULT
        ),
        Err(SweepError::Failed(Failure {
            error: KernelError::NotImplemented(_),
            ..
        }))
    ));
}
