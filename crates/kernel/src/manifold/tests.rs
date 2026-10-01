use glam::{DVec2, DVec3, Vec3};

use super::*;
use crate::mesh::tests::{TOL, add_round_octahedron, half_cylinder, round_octahedron, torus};
use crate::mesh::{Mesh, MeshBuilder};
use crate::par::assert_deterministic;
use crate::profile::tests::{circle, rect};
use crate::tessellate::Limits;
use crate::{Budget, Display, Frame, Op, Profile, Solid, Tolerance, boolean, extrude};

/// The unit tetrahedron's corners and outward triangles.
fn tetrahedron() -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    (
        vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
    )
}

#[test]
fn a_tetrahedron_passes() {
    let (p, t) = tetrahedron();
    let mesh = ManifoldMesh::new(p, t).unwrap();
    assert!((mesh.volume() - 1.0 / 6.0).abs() < 1e-15);
}

/// A change to a tetrahedron's parts.
type Edit = dyn Fn(&mut Vec<[f64; 3]>, &mut Vec<[u32; 3]>);

#[test]
fn broken_parts_are_refused() {
    let refused = |edit: &Edit| {
        let (mut p, mut t) = tetrahedron();
        edit(&mut p, &mut t);
        ManifoldMesh::new(p, t).unwrap_err()
    };
    assert_eq!(refused(&|_, t| t.clear()), ManifoldError::Empty);
    assert_eq!(
        refused(&|p, _| p[2][1] = f64::NAN),
        ManifoldError::Position(2)
    );
    assert_eq!(refused(&|p, _| p[3][2] = 3e6), ManifoldError::Position(3));
    assert_eq!(refused(&|_, t| t[1][2] = 4), ManifoldError::Index(1));
    assert_eq!(
        refused(&|_, t| t[2] = [0, 3, 0]),
        ManifoldError::RepeatedVertex(2)
    );
    // Vertex 3 on the line through 0 and 1: triangle 1 is flat.
    assert_eq!(
        refused(&|p, _| p[3] = [0.5, 0.0, 0.0]),
        ManifoldError::Degenerate(1)
    );
    // Exactly on the line, though far from any round number.
    assert_eq!(
        refused(&|p, _| {
            p[1] = [3.0, 1e-300, 7.0];
            p[3] = [6.0, 2e-300, 14.0];
        }),
        ManifoldError::Degenerate(1)
    );
    // A copy of vertex 0 standing in for it in one triangle.
    assert_eq!(
        refused(&|p, t| {
            p.push([-0.0, 0.0, 0.0]);
            t[3] = [1, 2, 4];
        }),
        ManifoldError::Coincident(0, 4)
    );
    assert_eq!(
        refused(&|_, t| {
            t.pop();
        }),
        ManifoldError::EdgeUse {
            a: 1,
            b: 2,
            uses: 1
        }
    );
    // A second copy of a triangle: its edges are used three times.
    assert_eq!(
        refused(&|_, t| t.push([1, 2, 3])),
        ManifoldError::EdgeUse {
            a: 1,
            b: 2,
            uses: 3
        }
    );
    assert_eq!(
        refused(&|_, t| t[0] = [0, 1, 2]),
        ManifoldError::Orientation { a: 0, b: 1 }
    );
    assert_eq!(
        refused(&|p, _| p.push([5.0; 3])),
        ManifoldError::UnusedVertex(4)
    );
    assert_eq!(
        refused(&|p, t| {
            p.insert(0, [5.0; 3]);
            for tri in t.iter_mut() {
                *tri = tri.map(|v| v + 1);
            }
        }),
        ManifoldError::UnusedVertex(0)
    );
    // Two tetrahedra sharing a corner: a closed surface pinched there.
    assert_eq!(
        refused(&|p, t| {
            p.extend([[-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]]);
            t.extend([[0, 4, 5], [0, 6, 4], [0, 5, 6], [4, 6, 5]]);
        }),
        ManifoldError::Fan(0)
    );
    assert_eq!(
        refused(&|_, t| {
            for tri in t.iter_mut() {
                tri.swap(1, 2);
            }
        }),
        ManifoldError::InsideOut
    );
}

/// `solid` welded at `display`, checked against its drawn tessellation:
/// the same triangles, each drawn position one of the welded ones, the
/// volume within the chord of the solid's over its area, and the same
/// at 1 and 8 threads. Returns the mesh.
fn welded(solid: &Solid, display: &Display) -> ManifoldMesh {
    let mesh = assert_deterministic(|| solid.manifold_mesh(display).unwrap());
    let drawn = solid.tessellate(display).unwrap();
    assert_eq!(mesh.triangles().len(), drawn.triangle_count());
    assert!(mesh.positions().len() <= drawn.positions().len());
    let mut ours: Vec<[u32; 3]> = (mesh.positions().iter())
        .map(|p| DVec3::from_array(*p).as_vec3().to_array().map(f32::to_bits))
        .collect();
    ours.sort_unstable();
    for p in drawn.positions() {
        assert!(ours.binary_search(&p.map(f32::to_bits)).is_ok(), "{p:?}");
    }
    let bounds = solid.bounds3().unwrap();
    let chord = display.chord((bounds.max - bounds.min).length());
    let (volume, exact) = (mesh.volume(), solid.volume());
    assert!(
        (volume - exact).abs() <= chord * solid.area(),
        "volume {volume}, not {exact} (chord {chord}, area {})",
        solid.area()
    );
    // And the drawn mesh encloses the same, to `f32` rounding.
    let o = (bounds.min + bounds.max) * 0.5;
    let p = |i: u32| Vec3::from(drawn.positions()[i as usize]).as_dvec3() - o;
    let drawn_volume: f64 = (drawn.indices().chunks(3))
        .map(|t| p(t[0]).dot(p(t[1]).cross(p(t[2]))) / 6.0)
        .sum();
    let step = f64::from(f32::EPSILON) * bounds.max.abs().max(bounds.min.abs()).max_element();
    assert!(
        (drawn_volume - volume).abs() <= 2.0 * step * solid.area() + 1e-9 * exact,
        "drawn {drawn_volume}, welded {volume}"
    );
    mesh
}

#[test]
fn a_box_welds_to_its_corners() {
    let solid = Solid::cuboid(DVec3::splat(-1.0), DVec3::new(2.0, 3.0, 4.0), 1, &TOL).unwrap();
    let mesh = welded(&solid, &Display::default());
    assert_eq!(mesh.positions().len(), 8);
    assert_eq!(mesh.triangles().len(), 12);
    assert!((mesh.volume() - 24.0).abs() < 1e-12);
}

#[test]
fn a_cylinder_shares_its_rims() {
    let solid = Solid::cylinder(DVec3::new(1.0, 2.0, 3.0), 5.0, 2.0, 1, &TOL).unwrap();
    let mesh = welded(&solid, &Display::default());
    // A rim point is one vertex, for the wall and the cap alike.
    let rim = [6.0, 2.0, 3.0];
    assert_eq!(mesh.positions().iter().filter(|&&p| p == rim).count(), 1);
    // A little less than the cylinder: the chords cut inside.
    assert!(mesh.volume() < solid.volume());
}

fn extruded(loops: Vec<crate::Loop>, frame: Frame, from: f64, to: f64, feature: u64) -> Solid {
    let profile = Profile { loops };
    extrude(&profile, &frame, from, to, feature, &TOL, &Budget::DEFAULT).unwrap()
}

#[test]
fn curved_solids_weld_into_manifolds() {
    let offset = DVec3::new(3e5, -2e5, 1e5);
    let mut builder = MeshBuilder::new();
    add_round_octahedron(&mut builder, DVec3::ZERO, 10.0, false);
    add_round_octahedron(&mut builder, DVec3::new(0.0, 0.0, 0.2), 9.7, true);
    let shell = (builder.build().unwrap())
        .repair(&TOL, &Budget::default())
        .unwrap();
    let meshes: Vec<Mesh> = vec![
        round_octahedron(DVec3::ZERO),
        round_octahedron(offset),
        half_cylinder(4.0, DVec3::ZERO),
        half_cylinder(0.01, DVec3::new(10.0, 0.0, 0.0)),
        torus(24, 12, 3.0, 1.0),
        // A thin shell round a void: two shells, the inner facing in.
        shell,
    ];
    let fine = Display::new(&Tolerance::new(1e-5).unwrap());
    for mesh in meshes {
        let solid = Solid::new(mesh, &TOL).unwrap();
        for display in [Display::default(), fine] {
            welded(&solid, &display);
        }
    }
}

#[test]
fn plates_with_holes_and_crossed_cylinders_weld() {
    // A plate with round holes, and a boss joined to it across one.
    let mut loops = vec![rect(DVec2::ZERO, DVec2::new(40.0, 20.0), 0)];
    for i in 0..4 {
        let r = if i % 2 == 0 { 4.0 } else { 3.3 };
        loops.push(circle(
            DVec2::new(5.0 + 10.0 * i as f64, 10.0),
            r,
            10 + i,
            true,
        ));
    }
    let plate = extruded(loops, Frame::XY, 0.0, 3.0, 1);
    let plate_volume = (800.0 - 2.0 * std::f64::consts::PI * (16.0 + 3.3 * 3.3)) * 3.0;
    let mesh = welded(&plate, &Display::default());
    assert!((mesh.volume() - plate_volume).abs() < 0.01 * plate_volume);

    let boss = Solid::cylinder(DVec3::new(15.0, 10.0, 1.0), 5.0, 6.0, 2, &TOL).unwrap();
    let joined = boolean(&plate, &boss, Op::Union, &TOL, &Budget::DEFAULT).unwrap();
    welded(&joined, &Display::default());

    // A cylinder along x through one along z: the cuts are traced.
    let along_x = Frame {
        origin: DVec3::new(-8.0, 0.0, 5.0),
        x: DVec3::Y,
        y: DVec3::Z,
    };
    let bar = extruded(
        vec![circle(DVec2::ZERO, 1.5, 0, false)],
        along_x,
        0.0,
        16.0,
        3,
    );
    let post = Solid::cylinder(DVec3::ZERO, 2.5, 10.0, 4, &TOL).unwrap();
    for op in [Op::Union, Op::Difference, Op::Intersection] {
        let result = boolean(&post, &bar, op, &TOL, &Budget::DEFAULT).unwrap();
        welded(&result, &Display::default());
    }
}

#[test]
fn far_and_empty_solids_are_refused() {
    let far = Solid::new(crate::mesh::tests::tetrahedron(DVec3::splat(5e6)), &TOL).unwrap();
    assert!(matches!(
        far.manifold_mesh(&Display::default()),
        Err(ManifoldError::Position(_))
    ));
    assert_eq!(
        Solid::empty().manifold_mesh(&Display::default()),
        Err(ManifoldError::Empty)
    );
}

#[test]
fn a_mesh_past_the_limits_is_too_large() {
    let solid = Solid::new(torus(24, 12, 3.0, 1.0), &TOL).unwrap();
    let display = Display::default();
    let mesh = solid.manifold_mesh(&display).unwrap();
    let exact = Limits {
        vertices: mesh.positions().len() as u64,
        indices: 3 * mesh.triangles().len() as u64,
        edges: 0,
    };
    assert_eq!(
        solid.manifold_mesh_within(&display, &exact).as_ref(),
        Ok(&mesh)
    );
    for tight in [
        Limits {
            vertices: exact.vertices - 1,
            ..exact
        },
        Limits {
            indices: exact.indices - 1,
            ..exact
        },
    ] {
        assert_eq!(
            solid.manifold_mesh_within(&display, &tight),
            Err(ManifoldError::TooLarge)
        );
    }
}
