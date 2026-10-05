use glam::{DVec2, DVec3, Vec3};

use super::*;
use crate::mesh::tests::{TOL, add_round_octahedron, half_cylinder, round_octahedron, torus};
use crate::mesh::{Mesh, MeshBuilder};
use crate::par::assert_deterministic;
use crate::profile::tests::{circle, rect};
use crate::tessellate::{Limits, patch_triangles};
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
    let mesh = ManifoldMesh::new([0.0; 3], p.clone(), t.clone()).unwrap();
    assert!((mesh.volume() - 1.0 / 6.0).abs() < 1e-15);
    let moved = ManifoldMesh::new([-3.0, 1e6, 2e6], p.clone(), t.clone()).unwrap();
    assert_eq!(moved.origin(), [-3.0, 1e6, 2e6]);
    for origin in [[0.5, 0.0, 0.0], [0.0, 2e6 + 1.0, 0.0], [f64::NAN, 0.0, 0.0]] {
        assert_eq!(
            ManifoldMesh::new(origin, p.clone(), t.clone()),
            Err(ManifoldError::Origin)
        );
    }
}

/// A change to a tetrahedron's parts.
type Edit = dyn Fn(&mut Vec<[f64; 3]>, &mut Vec<[u32; 3]>);

#[test]
fn broken_parts_are_refused() {
    let refused = |edit: &Edit| {
        let (mut p, mut t) = tetrahedron();
        edit(&mut p, &mut t);
        ManifoldMesh::new([0.0; 3], p, t).unwrap_err()
    };
    assert_eq!(refused(&|_, t| t.clear()), ManifoldError::Empty);
    assert_eq!(
        refused(&|p, _| p[2][1] = f64::NAN),
        ManifoldError::Position(2)
    );
    assert_eq!(refused(&|p, _| p[3][2] = 3e6), ManifoldError::Position(3));
    // Not an `f32`: a reader keeping single precision would move it.
    assert_eq!(refused(&|p, _| p[1][0] = 0.1), ManifoldError::Position(1));
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
            let tiny = f64::from(1e-38f32);
            p[1] = [3.0, tiny, 7.0];
            p[3] = [6.0, 2.0 * tiny, 14.0];
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
/// the same triangles, each drawn position within `f32` rounding of a
/// welded one, the origin the middle of the solid's box, rounded, the
/// volume within the chord of the solid's over its area, and the same
/// at 1 and 8 threads. Returns the mesh.
fn welded(solid: &Solid, display: &Display) -> ManifoldMesh {
    let mesh = assert_deterministic(|| solid.manifold_mesh(display).unwrap());
    let drawn = solid.tessellate(display).unwrap();
    assert_eq!(mesh.triangles().len(), drawn.triangle_count());
    assert!(mesh.positions().len() <= drawn.positions().len());
    let bounds = solid.bounds3().unwrap();
    let origin = DVec3::from_array(mesh.origin());
    assert_eq!(origin, ((bounds.min + bounds.max) * 0.5).round());
    // Both round to `f32`, about different points.
    let step = f64::from(f32::EPSILON) * bounds.max.abs().max(bounds.min.abs()).max_element();
    let mut ours: Vec<DVec3> = (mesh.positions().iter())
        .map(|&p| origin + DVec3::from_array(p))
        .collect();
    ours.sort_unstable_by(|a, b| a.x.total_cmp(&b.x));
    for p in drawn.positions() {
        let p = Vec3::from(*p).as_dvec3();
        let from = ours.partition_point(|q| q.x < p.x - step);
        let near = ours[from..].iter().take_while(|q| q.x <= p.x + step);
        assert!(
            near.into_iter()
                .any(|q| (*q - p).abs().max_element() <= step),
            "{p:?}"
        );
    }
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
    assert_eq!(mesh.origin(), [1.0, 2.0, 4.0]);
    // A rim point is one vertex, for the wall and the cap alike.
    let rim = [5.0, 0.0, -1.0];
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
    assert_eq!(
        far.manifold_mesh(&Display::default()),
        Err(ManifoldError::Origin)
    );
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
        edge_points: 0,
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

#[test]
fn far_solids_weld_about_their_own_middle() {
    // Far out, `f32` steps are centimetres: about the origin, readers
    // keeping single precision would merge the samples of the hole.
    for offset in [DVec3::new(9e5, -7e5, 5e5), DVec3::new(1e5, 0.0, -1e5)] {
        let plate = Solid::cuboid(offset, DVec3::new(40.0, 20.0, 3.0), 1, &TOL).unwrap();
        let hole =
            Solid::cylinder(offset + DVec3::new(10.0, 10.0, -1.0), 4.0, 5.0, 2, &TOL).unwrap();
        let solid = boolean(&plate, &hole, Op::Difference, &TOL, &Budget::DEFAULT).unwrap();
        let mesh = welded(&solid, &Display::default());
        let middle = offset + DVec3::new(20.0, 10.0, 1.5);
        assert_eq!(DVec3::from_array(mesh.origin()), middle.round());
        assert!(mesh.positions().iter().flatten().all(|x| x.abs() <= 21.0));
        let exact = 40.0 * 20.0 * 3.0 - std::f64::consts::PI * 16.0 * 3.0;
        assert!((mesh.volume() - exact).abs() < 0.01 * exact);
    }
}

#[test]
fn samples_that_round_together_are_refused() {
    // A thin pin 50 km from the block it's part of: about the middle,
    // `f32` steps are millimetres, and its samples land on each other.
    let block = Solid::cuboid(DVec3::ZERO, DVec3::ONE, 1, &TOL).unwrap();
    let pin = Solid::cylinder(DVec3::new(1e5, 0.0, 0.0), 0.01, 1.0, 2, &TOL).unwrap();
    let both = boolean(&block, &pin, Op::Union, &TOL, &Budget::DEFAULT).unwrap();
    let refused = both.manifold_mesh(&Display::default()).unwrap_err();
    assert!(
        matches!(
            refused,
            ManifoldError::Coincident(..) | ManifoldError::Degenerate(_)
        ),
        "{refused:?}"
    );
}

/// Whether these parts make a [`ManifoldMesh`], checked the plain way,
/// independently of [`ManifoldMesh::new`]: maps of edges and corners, and
/// collinearity in integers. `None` where that can't tell: a coordinate
/// that isn't a multiple of `2^-40` below `2^20`, or a volume too near
/// zero to tell its sign in `f64`.
fn plainly_manifold(origin: [f64; 3], p: &[[f64; 3]], t: &[[u32; 3]]) -> Option<bool> {
    use std::collections::{BTreeMap, BTreeSet};
    let whole = |x: f64| x.abs() <= ManifoldMesh::MAX_POSITION && x.fract() == 0.0;
    if t.is_empty() || !origin.iter().all(|&x| whole(x)) {
        return Some(false);
    }
    let fits = |x: f64| x.is_finite() && x.abs() <= ManifoldMesh::MAX_POSITION;
    if !p
        .iter()
        .flatten()
        .all(|&x| fits(x) && f64::from(x as f32) == x)
    {
        return Some(false);
    }
    let fixed = |x: f64| {
        let scaled = x * (1u64 << 40) as f64;
        (scaled.fract() == 0.0 && scaled.abs() < (1u64 << 60) as f64).then_some(scaled as i128)
    };
    let q: Vec<[i128; 3]> = (p.iter())
        .map(|v| Some([fixed(v[0])?, fixed(v[1])?, fixed(v[2])?]))
        .collect::<Option<_>>()?;
    if (q.iter().collect::<BTreeSet<_>>()).len() != q.len() {
        return Some(false);
    }
    let mut directed = BTreeMap::new();
    let mut around: BTreeMap<u32, BTreeMap<u32, u32>> = BTreeMap::new();
    for &[a, b, c] in t {
        if [a, b, c].iter().any(|&v| v as usize >= p.len()) || a == b || b == c || c == a {
            return Some(false);
        }
        let [pa, pb, pc] = [a, b, c].map(|v| q[v as usize]);
        let u = [0, 1, 2].map(|i| pb[i] - pa[i]);
        let w = [0, 1, 2].map(|i| pc[i] - pa[i]);
        let cross = [0, 1, 2].map(|i| {
            let (j, k) = ((i + 1) % 3, (i + 2) % 3);
            u[j] * w[k] - u[k] * w[j]
        });
        if cross == [0; 3] {
            return Some(false);
        }
        for (from, to, next) in [(a, b, c), (b, c, a), (c, a, b)] {
            if directed.insert((from, to), ()).is_some() {
                return Some(false);
            }
            around.entry(from).or_default().insert(to, next);
        }
    }
    if directed
        .keys()
        .any(|&(a, b)| !directed.contains_key(&(b, a)))
    {
        return Some(false);
    }
    if around.len() != p.len() {
        return Some(false);
    }
    for links in around.values() {
        let (&start, _) = links.iter().next().unwrap();
        let mut at = start;
        let mut seen = 0;
        loop {
            at = links[&at];
            seen += 1;
            if at == start {
                break;
            }
        }
        if seen != links.len() {
            return Some(false);
        }
    }
    let volume = signed_volume(p, t);
    let scale: f64 = (t.iter())
        .map(|tri| {
            let [a, b, c] = tri.map(|v| DVec3::from_array(p[v as usize]));
            a.length() * b.length() * c.length()
        })
        .sum();
    if volume.abs() <= 1e-9 * scale {
        return None;
    }
    Some(volume > 0.0)
}

#[test]
fn mutated_meshes_are_refused_unless_still_manifolds() {
    use crate::test_rng::Rng;
    let display = Display::new(&Tolerance::new(1e-1).unwrap());
    let solids = [
        Solid::cuboid(DVec3::splat(-1.0), DVec3::new(2.0, 3.0, 4.0), 1, &TOL).unwrap(),
        Solid::cylinder(DVec3::new(1.0, 2.0, 3.0), 1.0, 2.0, 1, &TOL).unwrap(),
        Solid::new(torus(8, 6, 3.0, 1.0), &TOL).unwrap(),
    ];
    let meshes: Vec<ManifoldMesh> = (solids.iter())
        .map(|s| s.manifold_mesh(&display).unwrap())
        .collect();
    let mut rng = Rng::new(60);
    let (mut kept, mut refused, mut untold) = (0, 0, 0);
    // Quick mode runs the first quarter.
    let cases = varde_testing::pick(1500, 6000);
    for case in 0..cases {
        let mesh = &meshes[case % meshes.len()];
        let mut origin = mesh.origin();
        let mut p = mesh.positions().to_vec();
        let mut t = mesh.triangles().to_vec();
        let pick = |rng: &mut Rng, n: usize| (rng.next_u64() % n as u64) as usize;
        for _ in 0..1 + pick(&mut rng, 3) {
            let (nt, np) = (t.len().max(1), p.len().max(1));
            match pick(&mut rng, 17) {
                0 => t
                    .get_mut(pick(&mut rng, nt))
                    .map_or((), |tri| tri.swap(0, 1)),
                1 => {
                    let v = pick(&mut rng, np + 1) as u32;
                    t.get_mut(pick(&mut rng, nt))
                        .map_or((), |tri| tri[pick(&mut rng, 3)] = v)
                }
                2 => {
                    if !t.is_empty() {
                        t.remove(pick(&mut rng, nt));
                    }
                }
                3 => t.extend(t.get(pick(&mut rng, nt)).copied()),
                4 => t.extend(t.get(pick(&mut rng, nt)).map(|&[a, b, c]| [a, c, b])),
                5 => {
                    let from = p[pick(&mut rng, np)];
                    p[pick(&mut rng, np)] = from;
                }
                6 => p.push([7.5, -2.25, 0.125]),
                // Two vertices welded into one.
                7 => {
                    let (keep, gone) = (pick(&mut rng, np) as u32, pick(&mut rng, np) as u32);
                    for tri in &mut t {
                        *tri = tri.map(|v| if v == gone { keep } else { v });
                    }
                }
                // A vertex split off for one triangle, a hair away.
                8 => {
                    let i = pick(&mut rng, nt);
                    let corner = pick(&mut rng, 3);
                    let tri = t.get_mut(i).filter(|tri| (tri[corner] as usize) < p.len());
                    if let Some(tri) = tri {
                        let mut at = p[tri[corner] as usize];
                        at[0] = f64::from(f32::from_bits((at[0] as f32).to_bits() + 1));
                        tri[corner] = p.len() as u32;
                        p.push(at);
                    }
                }
                9 => t.iter_mut().for_each(|tri| tri.swap(1, 2)),
                // Renumbered and reordered: still a manifold.
                10 => {
                    let n = p.len() as u32;
                    let shift = 1 + pick(&mut rng, np) as u32;
                    let renumber = |v: u32| (v + shift) % n;
                    let mut moved = p.clone();
                    for (v, at) in p.iter().enumerate() {
                        moved[renumber(v as u32) as usize] = *at;
                    }
                    p = moved;
                    t = t.iter().map(|tri| tri.map(renumber)).collect();
                    t.reverse();
                    t.iter_mut().for_each(|tri| tri.rotate_left(1));
                }
                // A second copy beside it: two shells, still a manifold;
                // or touching it at a vertex: pinched there.
                11 | 12 => {
                    let n = p.len() as u32;
                    p.extend(p.clone().iter().map(|&[x, y, z]| [x + 64.0, y, z]));
                    let copy: Vec<[u32; 3]> = t.iter().map(|tri| tri.map(|v| v + n)).collect();
                    t.extend(copy);
                    if pick(&mut rng, 2) == 0 {
                        let (keep, gone) =
                            (pick(&mut rng, np) as u32, n + pick(&mut rng, np) as u32);
                        for tri in &mut t {
                            *tri = tri.map(|v| if v == gone { keep } else { v });
                        }
                    }
                }
                13 => {
                    p[pick(&mut rng, np)][pick(&mut rng, 3)] =
                        [f64::NAN, 1e300, 0.1][pick(&mut rng, 3)]
                }
                14 => origin[pick(&mut rng, 3)] += [0.5, 3e6, 1.0][pick(&mut rng, 3)],
                // A vertex moved onto the line through two of its
                // triangle's others, or between them.
                15 => {
                    let tri = t.get(pick(&mut rng, nt));
                    let tri = tri.filter(|tri| tri.iter().all(|&v| (v as usize) < p.len()));
                    if let Some(&[a, b, c]) = tri {
                        let [pa, pb] = [a, b].map(|v| DVec3::from_array(p[v as usize]));
                        let k = [2.0, -1.0, 0.5][pick(&mut rng, 3)];
                        let on = pa + (pb - pa) * k;
                        if on.to_array().iter().all(|&x| f64::from(x as f32) == x) {
                            p[c as usize] = on.to_array();
                        }
                    }
                }
                _ => t.truncate(t.len() / 2),
            }
        }
        let plain = plainly_manifold(origin, &p, &t);
        let checked = ManifoldMesh::new(origin, p.clone(), t.clone());
        match plain {
            Some(true) => {
                assert!(checked.is_ok(), "case {case}: {checked:?}");
                kept += 1;
            }
            Some(false) => {
                assert!(checked.is_err(), "case {case}: taken");
                refused += 1;
            }
            None => untold += 1,
        }
    }
    // Both kinds seen plenty.
    assert!(
        kept * 40 > cases && refused * 2 > cases && untold * 3 < cases,
        "{kept} kept, {refused} refused, {untold} untold"
    );
}

/// Drawing, picking and welding share one tessellation plan: the drawn
/// mesh is the same with picking as without, the picking has an entry per
/// drawn triangle, and the welded triangles are the drawn ones in their
/// order, corner for corner, apart from rounding (the drawn positions are
/// `f32` about the world's origin, the welded about the solid's middle),
/// far out too: the strips of both choose their diagonals from the `f64`
/// points, not from the two roundings.
#[test]
fn drawing_picking_and_welding_share_their_triangles() {
    let mut loops = vec![rect(DVec2::ZERO, DVec2::new(40.0, 20.0), 0)];
    for i in 0..3 {
        loops.push(circle(
            DVec2::new(8.0 + 12.0 * i as f64, 10.0),
            3.0 + 0.4 * i as f64,
            10 + i,
            true,
        ));
    }
    let plate = extruded(loops, Frame::XY, 0.0, 3.0, 1);
    let boss = Solid::cylinder(DVec3::new(8.0, 10.0, 1.0), 5.0, 6.0, 2, &TOL).unwrap();
    let joined = boolean(&plate, &boss, Op::Union, &TOL, &Budget::DEFAULT).unwrap();
    let away = crate::Motion::translation(DVec3::new(4096.7, -1515.8, 2130.3)).unwrap();
    let far = |solid: Solid| (solid.transformed(&away, None, &TOL, &Budget::DEFAULT)).unwrap();
    let turned = crate::revolve(
        &Profile {
            loops: vec![circle(DVec2::new(10.0, 0.0), 2.0, 1, false)],
        },
        &Frame {
            origin: DVec3::ZERO,
            x: DVec3::X,
            y: DVec3::Z,
        },
        crate::Sweep::Full,
        3,
        &TOL,
        &Budget::DEFAULT,
    )
    .unwrap();
    let solids = [
        Solid::cuboid(DVec3::splat(-1.0), DVec3::new(2.0, 3.0, 4.0), 1, &TOL).unwrap(),
        Solid::cylinder(DVec3::new(1.0, 2.0, 3.0), 5.0, 2.0, 1, &TOL).unwrap(),
        Solid::new(round_octahedron(DVec3::ZERO), &TOL).unwrap(),
        Solid::new(half_cylinder(4.0, DVec3::ZERO), &TOL).unwrap(),
        Solid::new(torus(24, 12, 3.0, 1.0), &TOL).unwrap(),
        plate,
        joined,
        far(Solid::new(round_octahedron(DVec3::ZERO), &TOL).unwrap()),
        far(turned),
    ];
    let fine = Display::new(&Tolerance::new(1e-4).unwrap());
    for (i, solid) in solids.iter().enumerate() {
        for display in [Display::default(), fine] {
            let drawn = solid.tessellate(&display).unwrap();
            let topology = solid.topology();
            let picked = solid.tessellate_with(&display, &topology).unwrap();
            assert_eq!(picked, drawn, "solid {i}");

            let mesh = solid.manifold_mesh(&display).unwrap();
            assert_eq!(mesh.triangles().len(), drawn.triangle_count(), "solid {i}");
            // The welded triangles go patch by patch, the drawn ones face
            // by face: the welded in the drawn order.
            let counts = patch_triangles(solid.mesh(), &display);
            let mut starts = vec![0usize];
            for &n in &counts {
                starts.push(starts.last().unwrap() + n as usize);
            }
            let welded: Vec<[u32; 3]> = (topology.regions().iter())
                .flat_map(|region| region.tris.iter())
                .flat_map(|&t| &mesh.triangles()[starts[t as usize]..starts[t as usize + 1]])
                .copied()
                .collect();
            let origin = DVec3::from_array(mesh.origin());
            let bounds = solid.bounds3().unwrap();
            let size = bounds.max.abs().max(bounds.min.abs()).max_element();
            let step = 2.0 * f64::from(f32::EPSILON) * size.max(1.0);
            for (t, (welded, shown)) in (welded.iter()).zip(drawn.indices().chunks(3)).enumerate() {
                for (&w, &d) in welded.iter().zip(shown) {
                    let w = origin + DVec3::from_array(mesh.positions()[w as usize]);
                    let d = Vec3::from(drawn.positions()[d as usize]).as_dvec3();
                    assert!(
                        (w - d).abs().max_element() <= step,
                        "solid {i}, triangle {t}: welded {w:?}, drawn {d:?}"
                    );
                }
            }
        }
    }
}
