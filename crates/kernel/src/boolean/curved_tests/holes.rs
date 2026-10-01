//! Holes drilled one after another into a plate in line with each other,
//! and grids of holes cut at once. A cut face is triangulated inside the
//! input triangle it came from, on its corners and the cut's vertices
//! only, so a drilled box keeps long thin triangles from its far corners
//! to the rims; the next hole of the same size in line with the first
//! passes a few tenths of a millimetre from their sides, along them, where
//! no split mends the band between (see `agents/kernel.md`, Known gaps).

use super::*;

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
            );
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
#[ignore = "Invalid(Hull) at the second hole: until the clean-up's quality pass on plane faces"]
fn a_box_drilled_twice_in_line() {
    // A second hole of the same size beside the first, in line with it
    // along x or along y: the band between its rim and the long side of a
    // cap triangle from the box's far corner to the first rim fails the
    // hull rules (both today).
    let mut failed = Vec::new();
    for (name, second) in [("along x", (3.4, 1.0)), ("along y", (1.0, 3.4))] {
        let steps = drilled_in_turn(&large_plate(), &[(1.0, 1.0), second], 0.5);
        failed.extend(check_drilled(&steps, 0.5, name));
    }
    assert!(failed.is_empty(), "{failed:?}");
}

#[test]
#[ignore = "Invalid(Hull) at the second and third holes: until the clean-up's quality pass on plane faces"]
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
#[ignore = "fans from the box's corners to the rim: until the clean-up's quality pass on plane faces"]
fn cut_caps_are_well_shaped() {
    // After one hole, the box's caps are fanned from their far corners to
    // the rim: every triangle the cut made on a plane face should have no
    // angle under 10°, but for the exemptions in `made_cap_sines`.
    let plate = large_plate();
    let hole = pin(1.0, 1.0, 0.5, 10);
    let drilled = boolean(&plate, &hole, Op::Difference, &TOL, &Budget::DEFAULT).unwrap();
    let sines = made_cap_sines(&drilled, &[&plate, &hole]);
    let bound = (10f64).to_radians().sin();
    let bad = sines.iter().filter(|&&s| s < bound).count();
    println!(
        "{} made cap triangles, {bad} under 10°, the worst sine {:.2e}",
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
    let cut = boolean(&plate, &tool, Op::Difference, &TOL, &Budget::DEFAULT)?;
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
#[ignore = "8 × 8 Invalid, 10 × 10 TooComplex: until the clean-up's quality pass on plane faces"]
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
