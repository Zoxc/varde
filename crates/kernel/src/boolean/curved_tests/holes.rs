//! Holes drilled one after another into a plate in line with each other,
//! and grids of holes cut at once. A cut face is triangulated inside the
//! input triangle it came from, on its corners and the cut's vertices
//! only, so a drilled box kept long thin triangles from its far corners
//! to the rims; the next hole of the same size in line with the first
//! passed a few tenths of a millimetre from their sides, along them, where
//! no split mends the band between. The clean-up now refines the plane
//! faces a boolean cuts for their shapes (see `agents/kernel.md`,
//! "Clean-up" and Known gaps).

use super::*;
use crate::Stripped;

/// A pin of radius `r` through a plate 1 thick on XY, at `(x, y)`.
fn pin(x: f64, y: f64, r: f64, feature: u64) -> Solid {
    Solid::cylinder(DVec3::new(x, y, -1.0), r, 3.0, feature, &TOL).unwrap()
}

/// The 20 × 20 × 1 box on XY from the origin.
fn large_plate() -> Solid {
    cube([0.0; 3], [20.0, 20.0, 1.0])
}

/// `plate` drilled with pins of radius `r` at `holes` one after another,
/// each step's result or error, at 1 and 8 threads alike.
fn drilled_in_turn(plate: &Solid, holes: &[(f64, f64)], r: f64) -> Vec<Result<Solid, KernelError>> {
    assert_deterministic(|| {
        let mut current = plate.clone();
        let mut out = Vec::new();
        for (k, &(x, y)) in holes.iter().enumerate() {
            let step = boolean(
                &current,
                &pin(x, y, r, 10 + k as u64),
                Op::Difference,
                &TOL,
                &Budget::DEFAULT,
            )
            .stripped();
            if let Ok(next) = &step {
                current = next.clone();
            }
            out.push(step);
        }
        out
    })
}

/// Checks the steps of [`drilled_in_turn`] on the 20 × 20 × 1 box: those
/// that worked have their patches on their faces and the volume `400 −
/// k·π·r²` after `k` holes. Gives the steps that failed.
fn check_drilled(steps: &[Result<Solid, KernelError>], r: f64, name: &str) -> Vec<String> {
    let mut failed = Vec::new();
    let mut holes = 0;
    for (k, step) in steps.iter().enumerate() {
        match step {
            Ok(solid) => {
                holes += 1;
                assert_eq!(
                    solid.mesh().check_faces(&TOL),
                    Ok(()),
                    "{name}, hole {}",
                    k + 1
                );
                let want = 400.0 - f64::from(holes) * PI * r * r;
                let got = solid.volume();
                assert!(
                    (got - want).abs() < 1e-9,
                    "{name}, hole {}: {got} vs {want}",
                    k + 1
                );
            }
            Err(e) => failed.push(format!("{name}, hole {}: {e:?}", k + 1)),
        }
    }
    failed
}

#[test]
fn a_box_drilled_twice_in_line() {
    // A second hole of the same size beside the first, in line with it
    // along x or along y: the band between its rim and the long side of a
    // cap triangle from the box's far corner to the first rim failed the
    // hull rules (both, before the clean-up refined the caps the first
    // hole cut).
    let mut failed = Vec::new();
    for (name, second) in [("along x", (3.4, 1.0)), ("along y", (1.0, 3.4))] {
        let steps = drilled_in_turn(&large_plate(), &[(1.0, 1.0), second], 0.5);
        failed.extend(check_drilled(&steps, 0.5, name));
    }
    assert!(failed.is_empty(), "{failed:?}");
}

#[test]
fn holes_in_line_on_a_large_plate() {
    // Three holes, the second and third each in line with the first.
    let steps = drilled_in_turn(&large_plate(), &[(1.0, 1.0), (3.4, 1.0), (1.0, 3.4)], 0.5);
    let failed = check_drilled(&steps, 0.5, "in line");
    assert!(failed.is_empty(), "{failed:?}");
}

/// The sine of the narrowest angle of the triangle on corners `p`, and
/// that corner's index.
fn narrowest(p: [DVec3; 3]) -> (f64, usize) {
    let side = |i: usize| p[(i + 1) % 3].distance(p[i]);
    // The narrowest corner is opposite the shortest side: corner `i` is
    // opposite side `i + 1`.
    let i = (0..3)
        .min_by(|&i, &j| side((i + 1) % 3).total_cmp(&side((j + 1) % 3)))
        .expect("three corners");
    let twice_area = (p[1] - p[0]).cross(p[2] - p[0]).length();
    (twice_area / (side(i) * side((i + 2) % 3)), i)
}

/// The sines of the narrowest angles of `solid`'s plane-face triangles
/// that `operands` didn't have as they are (by corner positions), sorted,
/// leaving out those a quality bound can't ask more of: the narrowest
/// corner between two constrained sides (curved, or on another face)
/// meeting at under 60°, a circumradius under `MIN_SPLIT` resolutions, or
/// a side no longer than an eighth of the resolution.
fn made_cap_sines(solid: &Solid, operands: &[&Solid]) -> Vec<f64> {
    use std::collections::BTreeSet;
    let key = |p: [DVec3; 3]| {
        let mut k = p.map(|q| [q.x.to_bits(), q.y.to_bits(), q.z.to_bits()]);
        k.sort();
        k
    };
    let corners = |mesh: &Mesh, t: usize| {
        mesh.tris()[t]
            .halfedges
            .map(|h| mesh.verts()[h.start as usize])
    };
    let kept: BTreeSet<_> = operands
        .iter()
        .flat_map(|s| (0..s.mesh().tris().len()).map(|t| key(corners(s.mesh(), t))))
        .collect();
    let mesh = solid.mesh();
    let res = TOL.resolution();
    let mut out = Vec::new();
    for t in 0..mesh.tris().len() {
        let tri = mesh.tris()[t];
        if !matches!(
            mesh.faces()[tri.face as usize].surface,
            Surface::Plane { .. }
        ) {
            continue;
        }
        let p = corners(mesh, t);
        if kept.contains(&key(p)) {
            continue;
        }
        let side = |i: usize| p[(i + 1) % 3].distance(p[i]);
        if (0..3).any(|i| side(i) <= res / 8.0) {
            continue;
        }
        let (sine, i) = narrowest(p);
        let circumradius =
            side(0) * side(1) * side(2) / (2.0 * (p[1] - p[0]).cross(p[2] - p[0]).length());
        if circumradius < crate::mesh::MIN_SPLIT * res {
            continue;
        }
        // Halfedge `i` leaves corner `i`, halfedge `i + 2` arrives at it.
        let constrained = |k: usize| {
            let h = tri.halfedges[k];
            let other = mesh.tris()[(h.pair / 3) as usize].face;
            let ctrl = mesh.edges()[h.edge as usize].ctrl;
            other != tri.face || !crate::mesh::straight(p[k], ctrl, p[(k + 1) % 3], res / 8.0)
        };
        if constrained(i) && constrained((i + 2) % 3) && sine < (PI / 3.0).sin() {
            continue;
        }
        out.push(sine);
    }
    out.sort_by(f64::total_cmp);
    out
}

#[test]
fn cut_caps_are_well_shaped() {
    // After one hole, the box's caps were fanned from their far corners
    // to the rim (24 of the 32 triangles the cut made on them under 10°,
    // the worst of sine 3.75e-3): the clean-up refines them, so none has
    // an angle under its bound, 5°, but for the exemptions in
    // `made_cap_sines`.
    let plate = large_plate();
    let hole = pin(1.0, 1.0, 0.5, 10);
    let drilled = boolean(&plate, &hole, Op::Difference, &TOL, &Budget::DEFAULT).unwrap();
    let sines = made_cap_sines(&drilled, &[&plate, &hole]);
    let bound = (5f64).to_radians().sin();
    let bad = sines.iter().filter(|&&s| s < bound).count();
    println!(
        "{} made cap triangles, {bad} under 5°, the worst sine {:.2e}",
        sines.len(),
        sines.first().copied().unwrap_or(1.0)
    );
    assert_eq!(bad, 0, "worst {:?}", sines.first());
}

/// A plate 100 × 100 × 10 on XY cut through by an `n × n` grid of discs
/// of radius 2 at `100·(k + ½)/n`, all in one profile extruded through it,
/// as the app's through-all cut does (a margin of 1.1 either side): the
/// result, checked against `100 000 − n²·π·4·10`, or the error.
fn grid_cut_at_once(n: usize) -> Result<Solid, KernelError> {
    let plate = extruded(
        vec![rect(DVec2::ZERO, DVec2::splat(100.0), 0)],
        0.0,
        10.0,
        1,
    );
    let at = |k: usize| 100.0 * (k as f64 + 0.5) / n as f64;
    let discs = (0..n * n)
        .map(|k| circle(DVec2::new(at(k % n), at(k / n)), 2.0, 10 + k as u64, false))
        .collect();
    let tool = extruded(discs, -1.1, 11.1, 2);
    let cut = boolean(&plate, &tool, Op::Difference, &TOL, &Budget::DEFAULT).stripped()?;
    let want = 100_000.0 - (n * n) as f64 * PI * 40.0;
    let got = cut.volume();
    assert!((got - want).abs() < 1e-6, "{n} × {n}: {got} vs {want}");
    Ok(cut)
}

#[test]
fn a_six_by_six_grid_of_holes_cut_at_once() {
    let cut = grid_cut_at_once(6).unwrap();
    println!("6 × 6: {} patches", cut.mesh().tris().len());
}

#[test]
#[ignore = "8 × 8 Invalid (a fan from a far rim along a rim's tangent, its corner there closed), 10 × 10 TooComplex before the clean-up"]
fn larger_grids_of_holes_cut_at_once() {
    let mut failed = Vec::new();
    for n in [8, 10] {
        let start = std::time::Instant::now();
        match grid_cut_at_once(n) {
            Ok(cut) => println!("{n} × {n}: {} patches", cut.mesh().tris().len()),
            Err(e) => failed.push(format!("{n} × {n}: {e:?}")),
        }
        println!("{n} × {n}: {:.2} s", start.elapsed().as_secs_f64());
    }
    assert!(failed.is_empty(), "{failed:?}");
}

/// Frames away from the world's for the sketches: far from the origin,
/// turned about `z`, tilted off every axis, and tilted far away.
fn moved_frames() -> [(&'static str, Frame); 4] {
    let tilt =
        DQuat::from_rotation_x(0.7) * DQuat::from_rotation_y(-0.4) * DQuat::from_rotation_z(1.1);
    let turn = DQuat::from_rotation_z(0.6);
    let frame = |origin: DVec3, q: DQuat| Frame {
        origin,
        x: q * DVec3::X,
        y: q * DVec3::Y,
    };
    [
        (
            "far",
            frame(DVec3::new(1234.5, -2345.25, 512.0), DQuat::IDENTITY),
        ),
        ("turned", frame(DVec3::new(3.7, -2.2, 0.9), turn)),
        ("tilted", frame(DVec3::new(-1.5, 2.0, 0.5), tilt)),
        (
            "tilted far",
            frame(DVec3::new(4321.0, -987.0, 2345.0), tilt),
        ),
    ]
}

#[test]
fn holes_in_line_on_moved_frames() {
    // The holes of `holes_in_line_on_a_large_plate`, sketched on frames
    // away from the world's: the clean-up's quality pass works in each
    // plane face's own plane, its points put back onto it. Every step
    // must work, its volume the plate's less `k·π/4`, the same at 1 and 8
    // threads.
    let mut failed = Vec::new();
    for (name, frame) in moved_frames() {
        let plate = extruded_on(
            vec![rect(DVec2::ZERO, DVec2::splat(20.0), 0)],
            frame,
            0.0,
            1.0,
            1,
        );
        let v0 = plate.volume();
        let holes = [(1.0, 1.0), (3.4, 1.0), (1.0, 3.4)];
        let steps = assert_deterministic(|| {
            let mut current = plate.clone();
            let mut out = Vec::new();
            for (k, &(x, y)) in holes.iter().enumerate() {
                let feature = 10 + k as u64;
                let pin = extruded_on(
                    vec![circle(DVec2::new(x, y), 0.5, feature, false)],
                    frame,
                    -1.0,
                    2.0,
                    feature,
                );
                let step = boolean(&current, &pin, Op::Difference, &TOL, &Budget::DEFAULT);
                if let Ok(next) = &step {
                    current = next.clone();
                }
                out.push(step.map(|s| s.volume()));
            }
            out
        });
        for (k, step) in steps.iter().enumerate() {
            match step {
                Ok(got) => {
                    let want = v0 - (k + 1) as f64 * PI / 4.0;
                    assert!(
                        (got - want).abs() < 1e-9 * v0,
                        "{name}, hole {}: {got} vs {want}",
                        k + 1
                    );
                }
                Err(e) => failed.push(format!("{name}, hole {}: {e:?}", k + 1)),
            }
        }
    }
    assert!(failed.is_empty(), "{failed:?}");
}

/// A closed uniform quadratic B-spline round `c` (parabola pieces, as
/// a sketch's spline is drawn), through the middles of the sides of the
/// polygon on `points` (offsets from `c`), clockwise for a hole.
fn spline_hole(c: DVec2, points: &[DVec2], curve: u64) -> Loop {
    let n = points.len();
    let q = |i: usize| c + points[i % n];
    let mid = |i: usize| (q(i) + q(i + 1)) * 0.5;
    Loop {
        segments: (0..n)
            .rev()
            .map(|i| Segment {
                conic: crate::patch::Conic2::new(mid(i + 1), q(i + 1), 1.0, mid(i)).unwrap(),
                curve,
            })
            .collect(),
    }
}

#[test]
fn fitted_grooves_across_drilled_plates() {
    // A plate with a round hole and a spline-shaped one, on a frame tilted
    // off every axis, grooved along its length through both holes (the
    // cuts between the groove and the holes' walls are fitted, on face
    // copies claiming no surface), then a pocket whose floor and walls
    // cross those bands and a hole through it. The quality pass works on
    // the plane faces round the bands: no curve off the plane may go onto
    // a triangle keeping the plane's tag. Each step checked by `a − b +
    // a ∩ b = a` within the fit tolerance's allowance, the same at 1 and 8
    // threads.
    let (_, frame) = moved_frames()[2];
    let along_x = Frame {
        origin: frame.origin,
        x: frame.y,
        y: frame.normal(),
    };
    let spline = [
        (1.2, 0.0),
        (0.7, 0.9),
        (-0.5, 1.1),
        (-1.2, 0.1),
        (-0.6, -1.0),
        (0.6, -1.1),
    ]
    .map(|(x, y)| DVec2::new(x, y));
    let plate = extruded_on(
        vec![
            rect(DVec2::ZERO, DVec2::new(12.0, 8.0), 0),
            circle(DVec2::new(3.5, 4.0), 1.0, 10, true),
            spline_hole(DVec2::new(8.5, 4.0), &spline, 11),
        ],
        frame,
        0.0,
        2.0,
        1,
    );
    let tools = [
        extruded_on(
            vec![circle(DVec2::new(4.3, 2.1), 0.6, 20, false)],
            along_x,
            -1.0,
            13.0,
            20,
        ),
        extruded_on(
            vec![rect(DVec2::new(2.0, 3.3), DVec2::new(10.0, 5.5), 21)],
            frame,
            1.75,
            3.0,
            21,
        ),
        extruded_on(
            vec![circle(DVec2::new(6.3, 4.5), 0.4, 22, false)],
            frame,
            -1.0,
            3.0,
            22,
        ),
    ];
    let steps = assert_deterministic(|| {
        let mut current = plate.clone();
        let mut out = Vec::new();
        for tool in &tools {
            let less = boolean(&current, tool, Op::Difference, &TOL, &Budget::DEFAULT);
            let both = boolean(&current, tool, Op::Intersection, &TOL, &Budget::DEFAULT);
            let free = less.as_ref().ok().map(|s| {
                s.mesh()
                    .faces()
                    .iter()
                    .filter(|f| matches!(f.surface, Surface::Free))
                    .count()
            });
            out.push((
                current.volume(),
                current.area() + tool.area(),
                less.as_ref().ok().map(Solid::volume),
                both.ok().map(|s| s.volume()),
                free,
            ));
            match less {
                Ok(next) => current = next,
                Err(e) => panic!("step {}: {e:?}", out.len() - 1),
            }
        }
        out
    });
    for (k, (va, area, less, both, free)) in steps.into_iter().enumerate() {
        println!("step {k}: {less:?} {both:?}, {free:?} faces claiming no surface");
        assert!(free.is_some_and(|n| n > 0), "step {k}: no fitted bands");
        let (less, both) = (less.unwrap(), both.expect("the intersection"));
        let within = TOL.fit() * area / 5.0;
        assert!(
            (less + both - va).abs() <= within,
            "step {k}: {less} + {both} vs {va}"
        );
    }
}

#[test]
fn a_cross_hole_near_another_holes_mouth() {
    // A user's design: a 2 × 2 × 2 box drilled through along `y` from its
    // front face, then cut along `x` from its side by a slightly smaller
    // hole, their axes 0.04 apart in height. The cut was refused
    // (`Invalid(VertexNeighbours)`): refining the drilled box where the
    // holes cross reached the front cap beside the first rim, and a cap
    // triangle there was bisected by a straight edge from the rim's seam
    // vertex that left it, a piece inside out. Such a triangle is split
    // red now. Each result is held to the analytic volume within a
    // twentieth of the fit over the area claiming no surface (the cut's
    // fitted bands; they come within 0.000, 0.008 and 0.016 of it), and
    // every vertex to the true surfaces, the same at 1 and 8 threads.
    let (r1, z1) = (0.8110238395601597, 1.0155982131481562);
    let (r2, z2) = (0.7809107911587168, 0.9763186052515123);
    let cube = extruded(
        vec![rect(DVec2::splat(-1.0), DVec2::splat(1.0), 4)],
        0.0,
        2.0,
        1,
    );
    let front = Frame {
        origin: DVec3::new(0.0, -1.0, 0.0),
        x: DVec3::X,
        y: DVec3::Z,
    };
    let first = circle(DVec2::new(0.0, z1), r1, 1, false);
    let drilled = run(
        &cube,
        &extruded_on(vec![first], front, -3.6, 0.0, 6),
        Op::Difference,
    );
    let side = Frame {
        origin: DVec3::new(1.0, 0.0, 0.0),
        x: DVec3::Y,
        y: DVec3::Z,
    };
    let second = circle(DVec2::new(0.0, z2), r2, 1, false);
    let tool = extruded_on(vec![second], side, -4.8, 0.0, 7);
    let (va, vb) = (8.0 - 2.0 * PI * r1 * r1, 4.8 * PI * r2 * r2);
    assert!((drilled.volume() - va).abs() < 1e-9);
    // The tool's part inside the drilled box.
    let inside = 2.0 * PI * r2 * r2 - crossing_volume(r1, z1, r2, z2);
    let on = |p: DVec3| {
        let one = (DVec2::new(p.x, p.z - z1).length() - r1).abs();
        let two = (DVec2::new(p.y, p.z - z2).length() - r2).abs();
        let planes = [
            p.x - 1.0,
            p.x + 1.0,
            p.x + 3.8,
            p.y - 1.0,
            p.y + 1.0,
            p.z,
            p.z - 2.0,
        ];
        planes.into_iter().fold(one.min(two), |m, d| m.min(d.abs()))
    };
    let results = [
        (Op::Difference, va - inside),
        (Op::Intersection, inside),
        (Op::Union, va + vb - inside),
    ]
    .map(|(op, want)| {
        let result = assert_deterministic(|| {
            boolean(&drilled, &tool, op, &TOL, &Budget::DEFAULT).map_err(|f| f.error)
        });
        let Ok(solid) = result else {
            assert!(op != Op::Difference, "the cut: {result:?}");
            return None;
        };
        let got = solid.volume();
        let within = 0.05 * TOL.fit() * free_area(&solid) + 1e-9;
        println!(
            "{op:?}: {got} vs {want}: {:+e}, within {within:e}",
            got - want
        );
        assert!(
            (got - want).abs() <= within,
            "{op:?}: {got} vs {want}, within {within:e}"
        );
        for &p in solid.mesh().verts() {
            assert!(
                on(p) <= TOL.fit() / 4.0,
                "{op:?}: the vertex {p} is off the surfaces"
            );
        }
        Some(got)
    });
    if let [Some(d), Some(i), Some(u)] = results {
        let within = TOL.fit() * (drilled.area() + tool.area()) / 5.0;
        assert!(
            (u + i - va - vb).abs() <= within,
            "{u} + {i} vs {va} + {vb}"
        );
        assert!((d - (va - i)).abs() <= within, "{d} vs {va} − {i}");
    }
}

#[test]
fn cross_holes_whose_seams_meet_where_one_touches() {
    // A box less a hole of radius 0.4375 along `y` round (x, z) = (0.0625,
    // 1), then less, with and joined to a hole of 0.25 along `x` round
    // (y, z) = (0.0625, 0.5625), whose side seams (its arcs' joins) lie at
    // the height of the first's bottom seam: there the second's straight
    // seam ruling touches the first's wall, the cut tangent to the side
    // of the patch it starts on, while it also lies on a side of the
    // other's patch. A tangency's double root puts the arc's end some
    // `1e-8` off the touching point, the step across the side `7e-9` of
    // its length: the trace from there started out of the patches, and
    // `−` and `∩` were refused as `Inconsistent`. It starts the way that
    // leaves neither patch, or along the sides (within a millionth of the
    // step) towards the arc's other end.
    let xz = Frame {
        origin: DVec3::ZERO,
        x: DVec3::Z,
        y: DVec3::X,
    };
    let yz = Frame {
        origin: DVec3::ZERO,
        x: DVec3::Y,
        y: DVec3::Z,
    };
    let block = extruded(
        vec![rect(DVec2::splat(-1.0), DVec2::splat(1.0), 0)],
        0.0,
        2.0,
        1,
    );
    let (r1, r2) = (0.4375, 0.25);
    let first = extruded_on(
        vec![circle(DVec2::new(1.0, 0.0625), r1, 0, false)],
        xz,
        -1.5,
        1.5,
        2,
    );
    let second = extruded_on(
        vec![circle(DVec2::new(0.0625, 0.5625), r2, 0, false)],
        yz,
        -1.5,
        1.5,
        3,
    );
    let drilled = run(&block, &first, Op::Difference);
    let va = 8.0 - 2.0 * PI * r1 * r1;
    assert!((drilled.volume() - va).abs() <= 1e-12);
    // The second hole within the block, less where it crosses the first.
    let both = 2.0 * PI * r2 * r2 - crossing_volume(r1, 1.0, r2, 0.5625);
    let vb = 3.0 * PI * r2 * r2;
    let allowed = TOL.fit() * (drilled.area() + second.area()) / 5.0;
    let mut got = Vec::new();
    for (op, want) in [
        (Op::Difference, va - both),
        (Op::Intersection, both),
        (Op::Union, va + vb - both),
    ] {
        let result = run(&drilled, &second, op);
        let v = result.volume();
        assert!((v - want).abs() <= allowed, "{op:?}: {v}, not {want}");
        got.push(v);
    }
    // The identities.
    assert!((got[0] + got[1] - va).abs() <= allowed);
    assert!((got[2] + got[1] - va - vb).abs() <= allowed);
}
