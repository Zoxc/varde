//! Points and directions on extruded, revolved and boolean solids.

use glam::{DVec2, DVec3};

use super::{BOTTOM, TOP, chain_keys, cuboid, key, op, side};
use crate::extrude::Frame;
use crate::mesh::tests::TOL;
use crate::mesh::{FaceKey, Form};
use crate::par::assert_deterministic;
use crate::profile::tests::{circle, polygon, rect};
use crate::profile::{Loop, Profile};
use crate::topology::{NotFound, Topology, Unresolved};
use crate::{Budget, Motion, Op, Solid, Sweep, extrude, revolve};

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn near(a: DVec3, b: DVec3, within: f64) -> bool {
    (a - b).abs().max_element() <= within
}

/// A 10 × 6 plate 2 thick, extruded up `z` as feature 1, with a hole of
/// radius 1.5 round (3, 3): its sides are curves 1 (`y = 0`), 2 (`x =
/// 10`), 3 (`y = 6`) and 4 (`x = 0`), the hole's wall curve 5.
fn plate() -> Solid {
    let profile = Profile {
        loops: vec![
            rect(v(0.0, 0.0), v(10.0, 6.0), 1),
            circle(v(3.0, 3.0), 1.5, 5, true),
        ],
    };
    extrude(&profile, &Frame::XY, 0.0, 2.0, 1, &TOL, &Budget::DEFAULT).unwrap()
}

/// `loops` revolved a full turn about the line through `origin` along
/// `z`, as feature 7, the profile drawn in distance from the axis and
/// height along it.
fn revolved(loops: Vec<Loop>, origin: DVec3) -> Solid {
    let frame = Frame {
        origin,
        x: DVec3::X,
        y: DVec3::Z,
    };
    revolve(
        &Profile { loops },
        &frame,
        Sweep::Full,
        7,
        &TOL,
        &Budget::DEFAULT,
    )
    .unwrap()
}

/// The key of the region of `solid` whose form `pick` takes.
fn face_with(solid: &Solid, topology: &Topology, pick: impl Fn(&Form) -> bool) -> FaceKey {
    let found: Vec<FaceKey> = (0..topology.regions().len() as u32)
        .filter(|&r| pick(topology.form(solid, r)))
        .map(|r| topology.regions()[r as usize].key)
        .collect();
    assert_eq!(found.len(), 1, "{found:?}");
    found[0]
}

fn sorted(a: FaceKey, b: FaceKey) -> [FaceKey; 2] {
    [a.min(b), a.max(b)]
}

#[test]
fn an_extruded_plate_s_points_and_directions() {
    let solid = plate();
    let t = solid.topology();
    let (top, bottom, hole) = (key(1, TOP), key(1, BOTTOM), key(1, side(5)));
    let anywhere = DVec3::ZERO;
    // A corner by its three faces, a straight edge's middle, a rim's
    // centre: exact where the numbers are.
    let corner = t.corner_point(&solid, [top, key(1, side(1)), key(1, side(2))], anywhere);
    assert_eq!(corner, Ok(DVec3::new(10.0, 0.0, 2.0)));
    let front = sorted(top, key(1, side(1)));
    assert_eq!(
        t.middle(&solid, front, anywhere),
        Ok(DVec3::new(5.0, 0.0, 2.0))
    );
    let rim = sorted(top, hole);
    let centre = t.centre(&solid, rim, anywhere).unwrap();
    assert!(near(centre, DVec3::new(3.0, 3.0, 2.0), 1e-14), "{centre}");
    let low = t.centre(&solid, sorted(bottom, hole), anywhere).unwrap();
    assert!(near(low, DVec3::new(3.0, 3.0, 0.0), 1e-14), "{low}");
    // Normals out, to the bit.
    assert_eq!(t.normal(&solid, &top, anywhere), Ok(DVec3::Z));
    assert_eq!(t.normal(&solid, &bottom, anywhere), Ok(DVec3::NEG_Z));
    assert_eq!(
        t.normal(&solid, &key(1, side(1)), anywhere),
        Ok(DVec3::NEG_Y)
    );
    assert_eq!(t.normal(&solid, &key(1, side(2)), anywhere), Ok(DVec3::X));
    // The hole's axis, along the extrude, through the point beside the
    // face's point.
    let [at, axis] = t
        .face_axis(&solid, &hole, DVec3::new(4.5, 3.0, 1.25))
        .unwrap();
    assert_eq!(axis, DVec3::Z);
    assert!(near(at, DVec3::new(3.0, 3.0, 1.25), 1e-14), "{at}");
    // A rim's axis is its flat face's normal: out of the plate on top,
    // out of it underneath.
    assert_eq!(t.edge_direction(&solid, rim, anywhere), Ok(DVec3::Z));
    assert_eq!(
        t.edge_direction(&solid, sorted(bottom, hole), anywhere),
        Ok(DVec3::NEG_Z)
    );
    // A straight edge runs with its first key's face on its left seen
    // from outside: that face lies to the left, towards which the other
    // face's normal points away.
    for c in 0..t.chains().len() as u32 {
        let faces = chain_keys(&t, c);
        if t.middle(&solid, faces, anywhere).is_err() {
            continue;
        }
        let d = t.edge_direction(&solid, faces, anywhere).unwrap();
        let n0 = t.normal(&solid, &faces[0], anywhere).unwrap();
        let n1 = t.normal(&solid, &faces[1], anywhere).unwrap();
        assert!(n0.cross(d).dot(n1) < 0.0, "{faces:?}: {d}");
        // The keys the other way round reverse it.
        let back = t.edge_direction(&solid, [faces[1], faces[0]], anywhere);
        assert_eq!(back, Ok(-d));
    }
    // The wrong kinds, and names that aren't there.
    let missing = key(9, TOP);
    assert_eq!(
        t.normal(&solid, &missing, anywhere),
        Err(Unresolved::NotFound(NotFound::Face))
    );
    assert!(matches!(
        t.normal(&solid, &hole, anywhere),
        Err(Unresolved::NotFlat { .. })
    ));
    assert!(matches!(
        t.face_axis(&solid, &top, anywhere),
        Err(Unresolved::NotRound { .. })
    ));
    assert!(matches!(
        t.middle(&solid, rim, anywhere),
        Err(Unresolved::NotStraight { .. })
    ));
    assert!(matches!(
        t.centre(&solid, front, anywhere),
        Err(Unresolved::NotCircular { .. })
    ));
    assert_eq!(
        t.middle(&solid, sorted(top, missing), anywhere),
        Err(Unresolved::NotFound(NotFound::Edge))
    );
    assert_eq!(
        t.corner_point(&solid, [top, bottom, hole], anywhere),
        Err(Unresolved::NotFound(NotFound::Corner))
    );
    assert_eq!(Unresolved::NotFlat { region: 0 }.to_string(), "isn't flat");
}

#[test]
fn revolved_faces_give_their_axes_and_rims_their_centres() {
    let origin = DVec3::new(5.0, -2.0, 1.0);
    // A cylinder of radius 3 standing on `origin`, a cone of height 3
    // on top: a flat bottom, a cylinder (curve 2) and a cone (curve 3).
    let solid = revolved(
        vec![polygon(
            &[v(0.0, 0.0), v(3.0, 0.0), v(3.0, 5.0), v(0.0, 8.0)],
            1,
        )],
        origin,
    );
    let t = solid.topology();
    let wall = face_with(&solid, &t, |f| matches!(f, Form::Cylinder { .. }));
    let cone = face_with(&solid, &t, |f| matches!(f, Form::Cone { .. }));
    let floor = face_with(&solid, &t, |f| matches!(f, Form::Plane { .. }));
    let anywhere = DVec3::ZERO;
    let [at, axis] = t
        .face_axis(&solid, &wall, origin + DVec3::new(3.0, 0.0, 2.0))
        .unwrap();
    assert!(axis.cross(DVec3::Z).length() <= 1e-15, "{axis}");
    assert!(near(at, origin + DVec3::Z * 2.0, 1e-13), "{at}");
    let [_, cone_axis] = t.face_axis(&solid, &cone, anywhere).unwrap();
    assert!(cone_axis.cross(DVec3::Z).length() <= 1e-15);
    // The bottom rim: its centre, and its axis out of the floor.
    let bottom = sorted(floor, wall);
    let centre = t.centre(&solid, bottom, anywhere).unwrap();
    assert!(near(centre, origin, 1e-13), "{centre}");
    assert_eq!(t.edge_direction(&solid, bottom, anywhere), Ok(DVec3::NEG_Z));
    // The rim between the cylinder and the cone, no flat face beside
    // it: its centre, and the first key's round face's axis.
    let shoulder = sorted(wall, cone);
    let centre = t.centre(&solid, shoulder, anywhere).unwrap();
    assert!(near(centre, origin + DVec3::Z * 5.0, 1e-13), "{centre}");
    let d = t.edge_direction(&solid, shoulder, anywhere).unwrap();
    let first = t.face_axis(&solid, &shoulder[0], anywhere).unwrap()[1];
    assert_eq!(d, first);
    // A torus's axis.
    let torus = revolved(vec![circle(v(10.0, 0.0), 2.0, 1, false)], origin);
    let t = torus.topology();
    let ring = face_with(&torus, &t, |f| matches!(f, Form::Torus { .. }));
    let [at, axis] = t
        .face_axis(&torus, &ring, origin + DVec3::new(12.0, 0.0, 0.5))
        .unwrap();
    assert!(axis.cross(DVec3::Z).length() <= 1e-15);
    assert!(near(at, origin + DVec3::Z * 0.5, 1e-13), "{at}");
}

#[test]
fn a_boolean_result_s_points_and_directions() {
    // A boss joined on a plate, a hole drilled through both, a pocket
    // cut into the plate's corner.
    let plate = cuboid([0.0, 0.0, 0.0], [10.0, 8.0, 2.0], 1, &TOL);
    let boss = Solid::cylinder(DVec3::new(5.0, 4.0, 2.0), 2.0, 3.0, 2, &TOL).unwrap();
    let hole = Solid::cylinder(DVec3::new(5.0, 4.0, -1.0), 1.0, 10.0, 3, &TOL).unwrap();
    let pocket = cuboid([8.0, -1.0, 1.0], [3.0, 3.0, 2.0], 4, &TOL);
    let solid = op(&plate, &boss, Op::Union, &TOL);
    let solid = op(&solid, &hole, Op::Difference, &TOL);
    let solid = op(&solid, &pocket, Op::Difference, &TOL);
    let t = solid.topology();
    let anywhere = DVec3::ZERO;
    // The boss's foot: its centre on the plate's top, its axis that
    // face's normal.
    let foot = sorted(key(1, TOP), key(2, side(0)));
    let centre = t.centre(&solid, foot, anywhere).unwrap();
    assert!(near(centre, DVec3::new(5.0, 4.0, 2.0), 1e-14), "{centre}");
    assert_eq!(t.edge_direction(&solid, foot, anywhere), Ok(DVec3::Z));
    // The hole's rims, out of the boss's top and the plate's bottom.
    let top_rim = sorted(key(2, TOP), key(3, side(0)));
    let centre = t.centre(&solid, top_rim, anywhere).unwrap();
    assert!(near(centre, DVec3::new(5.0, 4.0, 5.0), 1e-14), "{centre}");
    assert_eq!(t.edge_direction(&solid, top_rim, anywhere), Ok(DVec3::Z));
    let low_rim = sorted(key(1, BOTTOM), key(3, side(0)));
    assert_eq!(
        t.edge_direction(&solid, low_rim, anywhere),
        Ok(DVec3::NEG_Z)
    );
    // The hole's wall's axis.
    let [at, axis] = t
        .face_axis(&solid, &key(3, side(0)), DVec3::new(6.0, 4.0, 3.0))
        .unwrap();
    assert!(axis.cross(DVec3::Z).length() == 0.0, "{axis}");
    assert_eq!(at, DVec3::new(5.0, 4.0, 3.0));
    // The pocket's inner corner, its floor's normal (out of the solid:
    // up) and the middle of its edge along the floor.
    let corner = [key(4, BOTTOM), key(4, side(3)), key(4, side(2))];
    assert_eq!(
        t.corner_point(&solid, corner, anywhere),
        Ok(DVec3::new(8.0, 2.0, 1.0))
    );
    assert_eq!(t.normal(&solid, &key(4, BOTTOM), anywhere), Ok(DVec3::Z));
    let floor_edge = sorted(key(4, BOTTOM), key(4, side(3)));
    assert_eq!(
        t.middle(&solid, floor_edge, anywhere),
        Ok(DVec3::new(8.0, 1.0, 1.0))
    );
    // The pocket's wall's normal points into the pocket, out of the
    // solid.
    assert_eq!(t.normal(&solid, &key(4, side(3)), anywhere), Ok(DVec3::X));
}

#[test]
fn points_and_directions_follow_a_motion() {
    let solid = plate();
    let m = Motion::turn(DVec3::new(1.0, 2.0, 3.0), DVec3::new(1.0, -2.0, 0.5), 37.0).unwrap();
    let moved = solid.transformed(&m, None, &TOL, &Budget::DEFAULT).unwrap();
    let (t, tm) = (solid.topology(), moved.topology());
    let anywhere = DVec3::ZERO;
    let rim = sorted(key(1, TOP), key(1, side(5)));
    let corner = [key(1, TOP), key(1, side(1)), key(1, side(2))];
    let edge = sorted(key(1, TOP), key(1, side(1)));
    let point = |t: &Topology, s: &Solid| {
        [
            t.centre(s, rim, anywhere).unwrap(),
            t.corner_point(s, corner, anywhere).unwrap(),
            t.middle(s, edge, anywhere).unwrap(),
        ]
    };
    for (a, b) in point(&t, &solid).iter().zip(point(&tm, &moved)) {
        assert!(near(m.point(*a), b, 1e-13), "{a} {b}");
    }
    let direction = |t: &Topology, s: &Solid| {
        [
            t.normal(s, &key(1, TOP), anywhere).unwrap(),
            t.face_axis(s, &key(1, side(5)), anywhere).unwrap()[1],
            t.edge_direction(s, rim, anywhere).unwrap(),
            t.edge_direction(s, edge, anywhere).unwrap(),
        ]
    };
    for (a, b) in direction(&t, &solid).iter().zip(direction(&tm, &moved)) {
        assert!(near(m.vector(*a), b, 1e-14), "{a} {b}");
    }
}

#[test]
fn points_and_directions_are_deterministic() {
    let run = || {
        let plate = cuboid([0.0, 0.0, 0.0], [10.0, 8.0, 2.0], 1, &TOL);
        let hole = Solid::cylinder(DVec3::new(5.0, 4.0, -1.0), 1.0, 10.0, 3, &TOL).unwrap();
        let solid = op(&plate, &hole, Op::Difference, &TOL);
        let t = solid.topology();
        let rim = sorted(key(1, TOP), key(3, side(0)));
        let near = DVec3::new(6.0, 4.0, 2.0);
        (
            t.centre(&solid, rim, near)
                .map(|p| p.to_array().map(f64::to_bits)),
            t.edge_direction(&solid, rim, near),
            t.face_axis(&solid, &key(3, side(0)), near),
        )
    };
    let (centre, direction, axis) = assert_deterministic(run);
    assert!(centre.is_ok() && direction.is_ok() && axis.is_ok());
}

#[test]
fn a_pin_aligned_by_its_rim_drops_into_a_hole() {
    use crate::transform::{AlignOptions, Datum};
    // A pin of radius 1.5 and length 5, lying turned somewhere, its
    // bottom rim's centre and axis onto the plate's hole's top rim's,
    // face to face: the pin stands in the hole, 3 above the plate.
    let pin = Solid::cylinder(DVec3::ZERO, 1.5, 5.0, 2, &TOL).unwrap();
    let lying = Motion::turn(DVec3::new(2.0, 0.0, 0.0), DVec3::new(1.0, 1.0, 0.0), 90.0)
        .unwrap()
        .then(&Motion::translation(DVec3::new(-20.0, 4.0, 1.0)).unwrap());
    let pin = pin
        .transformed(&lying, None, &TOL, &Budget::DEFAULT)
        .unwrap();
    let plate = plate();
    let (tp, tt) = (pin.topology(), plate.topology());
    let anywhere = DVec3::ZERO;
    let foot = sorted(key(2, BOTTOM), key(2, side(0)));
    let rim = sorted(key(1, TOP), key(1, side(5)));
    let datum = |t: &Topology, s: &Solid, rim| Datum {
        point: t.centre(s, rim, anywhere).unwrap(),
        primary: Some(t.edge_direction(s, rim, anywhere).unwrap()),
        secondary: None,
    };
    let options = AlignOptions {
        flip: true,
        offset: -2.0,
        degrees: 0.0,
    };
    let m = Motion::align(&datum(&tp, &pin, foot), &datum(&tt, &plate, rim), &options).unwrap();
    let placed = pin.transformed(&m, None, &TOL, &Budget::DEFAULT).unwrap();
    let t = placed.topology();
    let centre = t.centre(&placed, foot, anywhere).unwrap();
    assert!(near(centre, DVec3::new(3.0, 3.0, 0.0), 1e-13), "{centre}");
    let down = t.edge_direction(&placed, foot, anywhere).unwrap();
    assert!(near(down, DVec3::NEG_Z, 1e-15), "{down}");
    // Joined: the plate with its hole filled and the pin standing 3
    // above it.
    let joined = op(&plate, &placed, Op::Union, &TOL);
    let volume = 10.0 * 6.0 * 2.0 + std::f64::consts::PI * 1.5 * 1.5 * 3.0;
    let got = joined.volume();
    assert!(
        (got - volume).abs() <= 1e-9 * volume,
        "{got} against {volume}"
    );
}

#[test]
fn a_pin_drops_into_a_tilted_plate_s_hole() {
    use crate::transform::{AlignOptions, Datum};
    let tilt = Motion::turn(DVec3::new(1.0, 2.0, 0.0), DVec3::new(1.0, -0.5, 0.25), 30.0).unwrap();
    let plate = plate()
        .transformed(&tilt, None, &TOL, &Budget::DEFAULT)
        .unwrap();
    let pin = Solid::cylinder(DVec3::new(-20.0, 0.0, 0.0), 1.5, 5.0, 2, &TOL).unwrap();
    let (tp, tt) = (pin.topology(), plate.topology());
    let anywhere = DVec3::ZERO;
    let foot = sorted(key(2, BOTTOM), key(2, side(0)));
    let rim = sorted(key(1, TOP), key(1, side(5)));
    let datum = |t: &Topology, s: &Solid, rim| Datum {
        point: t.centre(s, rim, anywhere).unwrap(),
        primary: Some(t.edge_direction(s, rim, anywhere).unwrap()),
        secondary: None,
    };
    let options = AlignOptions {
        flip: true,
        offset: -2.0,
        degrees: 0.0,
    };
    let m = Motion::align(&datum(&tp, &pin, foot), &datum(&tt, &plate, rim), &options).unwrap();
    let placed = pin.transformed(&m, None, &TOL, &Budget::DEFAULT).unwrap();
    // Its foot on the hole's bottom rim, to rounding.
    let bottom = tt
        .centre(&plate, sorted(key(1, BOTTOM), key(1, side(5))), anywhere)
        .unwrap();
    let centre = placed.topology().centre(&placed, foot, anywhere).unwrap();
    assert!(near(centre, bottom, 1e-13), "{centre} {bottom}");
    let joined = op(&plate, &placed, Op::Union, &TOL);
    let volume = 10.0 * 6.0 * 2.0 + std::f64::consts::PI * 1.5 * 1.5 * 3.0;
    let got = joined.volume();
    assert!(
        (got - volume).abs() <= 1e-9 * volume,
        "{got} against {volume}"
    );
}
