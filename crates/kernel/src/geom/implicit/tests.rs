//! Per-tile implicits against their tiles: every tile of every form, at
//! the base level and finer, within rounding of its implicit's zero set
//! (exact points, exact polynomial); the first level's coefficients
//! enclosing the exact ones; the degrees by form and profile span; the
//! distance certificate reading them; the cylinder's the leaves'
//! quadric, bit for bit; spans outside a profile and degenerate spans
//! refused.

use malachite::Rational as Q;
use varde_decide::num::bounded::Bounded;

use super::*;
use crate::geom::certify::implicit::calibration::alone;
use crate::geom::curve::Curve3D;
use crate::geom::form::{ConicCylinderKey, CylinderKey, RevolvedKey};
use crate::geom::lattice::{Lattice, Level, Rect};
use crate::geom::round::nearest;
use crate::geom::samples::{Sample, surfaces};
use crate::geom::surface::Surface;
use crate::geom::tile::Tile;

/// A point's distance from the zero set, estimated to first order,
/// within this many units of its tile's coordinate scale.
const TOL: f64 = 1e-14;

/// The largest coordinate magnitude of a tile's control points.
fn scale(t: &Tile) -> f64 {
    t.net()
        .iter()
        .flat_map(|p| p[..3].iter().map(|x| x.abs()))
        .fold(0.0, |m, x| if x > m { x } else { m })
}

/// `|∇F|` at `x`, from the exact implicit's terms (rounded at the end).
fn gradient(imp: &Implicit<Q>, x: &[Q; 3]) -> f64 {
    let y: [Q; 3] = std::array::from_fn(|k| &x[k] - &imp.origin()[k]);
    let mut g = [Q::from(0u32), Q::from(0u32), Q::from(0u32)];
    for (e, c) in imp.terms() {
        for k in 0..3 {
            if e[k] == 0 {
                continue;
            }
            let mut t = c * Q::from(e[k] as u32);
            for (m, ym) in y.iter().enumerate() {
                let p = if m == k { e[m] - 1 } else { e[m] };
                for _ in 0..p {
                    t *= ym;
                }
            }
            g[k] += t;
        }
    }
    nearest(&(&g[0] * &g[0] + &g[1] * &g[1] + &g[2] * &g[2])).sqrt()
}

#[test]
fn tiles_lie_on_their_implicits() {
    let at = [0.125, 0.5, 0.875].map(|x| Q::try_from(x).unwrap());
    for Sample { name, surface } in surfaces(1) {
        let mut worst: f64 = 0.0;
        for (n, tile) in surface.tiles().iter().enumerate() {
            let (i, j) = surface.indices(n).unwrap();
            let imp = surface.implicit(i, j).unwrap().unwrap();
            let exact = imp.implicit_at(&Q::from(0u32));
            let patch = tile.exact();
            let s = scale(tile);
            for u in &at {
                for v in &at {
                    let x = patch.eval(u, v);
                    let f = nearest(&exact.eval_hom(&x, &Q::from(1u32))).abs();
                    let g = gradient(&exact, &x);
                    assert!(g > 0.0, "{name}: a regular point of the implicit");
                    let d = f / g / s;
                    worst = if d > worst { d } else { worst };
                }
            }
        }
        assert!(worst <= TOL, "{name}: {worst:e} of the scale off");
    }
}

#[test]
fn the_first_level_encloses_the_exact_coefficients() {
    let like = Bounded::exact(0.0);
    for Sample { name, surface } in surfaces(2) {
        let (i, j) = surface.indices(0).unwrap();
        let imp = surface.implicit(i, j).unwrap().unwrap();
        let exact = imp.implicit_at(&Q::from(0u32));
        let first = imp.implicit_at(&like);
        assert_eq!(first.degree(), exact.degree(), "{name}");
        assert_eq!(exact.degree(), imp.degree(), "{name}");
        let inside = |b: &Bounded<f64>, x: &Q| {
            let [lo, hi] = b.enclose();
            assert!(
                *x >= Q::try_from(lo).unwrap() && *x <= Q::try_from(hi).unwrap(),
                "{name}: {x} outside [{lo:e}, {hi:e}]"
            );
        };
        for k in 0..3 {
            inside(&first.origin()[k], &exact.origin()[k]);
        }
        for (e, b) in first.terms() {
            let x = exact
                .terms()
                .iter()
                .find(|(f, _)| f == e)
                .map_or(Q::from(0u32), |(_, x)| x.clone());
            inside(b, &x);
        }
        for (e, _) in exact.terms() {
            assert!(
                first.terms().iter().any(|(f, _)| f == e),
                "{name}: term {e:?} lost"
            );
        }
    }
}

/// The degree of span `l`'s implicit on a revolved conic of `profile`
/// about `z`.
fn revolved_degree(profile: &[Span2]) -> Vec<usize> {
    let key = RevolvedKey::new([0.0; 3], [0.0, 0.0, 1.0], profile).unwrap();
    let lattice = Lattice::new(key.into()).unwrap();
    (0..profile.len() as i64)
        .map(|l| lattice.implicit(0, l).unwrap().degree())
        .collect()
}

#[test]
fn degrees_by_form_and_span() {
    let mut by_name: Vec<(String, usize)> = surfaces(3)
        .into_iter()
        .map(|s| {
            let (i, j) = s.surface.indices(0).unwrap();
            (s.name, s.surface.implicit(i, j).unwrap().unwrap().degree())
        })
        .collect();
    by_name.retain(|(n, _)| !n.ends_with("(refined)"));
    let want = |n: &str| match n.trim_start_matches("affine ") {
        "plane" => 1,
        "revolved flat line" => 1,
        "cylinder" | "conic chain" | "hyperbola chain" | "cone" | "sphere" => 2,
        "revolved oblique line" => 2,
        _ => 4,
    };
    for (n, d) in &by_name {
        assert_eq!(*d, want(n), "{n}");
    }
    // The meridians of a cylinder, a cone and a plane; a quarter circle
    // off the axis (a torus's) and from a pole (its weight rounded, so
    // not exactly centred on the axis: a quartic too); a parabola
    // symmetric about the axis (a paraboloid).
    let line = |a: [f64; 2], b: [f64; 2]| {
        [
            [a[0], a[1], 1.0],
            [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, 1.0],
            [b[0], b[1], 1.0],
        ]
    };
    assert_eq!(revolved_degree(&[line([2.0, 0.0], [2.0, 3.0])]), [2]);
    assert_eq!(revolved_degree(&[line([1.0, 0.0], [3.0, 2.0])]), [2]);
    assert_eq!(revolved_degree(&[line([1.0, 1.0], [3.0, 1.0])]), [1]);
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let arc = |c: [f64; 2], r: f64| {
        [
            [c[0] + r, c[1], 1.0],
            [c[0] + r, c[1] + r, h],
            [c[0], c[1] + r, 1.0],
        ]
    };
    assert_eq!(revolved_degree(&[arc([3.0, 0.0], 1.0)]), [4]);
    assert_eq!(revolved_degree(&[arc([0.0, 0.0], 1.0)]), [4]);
    let parabola = [[0.0, 0.0, 1.0], [0.5, 0.0, 1.0], [1.0, 1.0, 1.0]];
    assert_eq!(revolved_degree(&[parabola]), [2]);
    // A straight chain span is a plane.
    let key = ConicCylinderKey::new(
        [0.0, 0.0, 1.0],
        &[[
            [0.0, 0.0, 0.0, 1.0],
            [1.0, 1.0, 0.0, 2.0],
            [2.0, 2.0, 0.0, 1.0],
        ]],
    )
    .unwrap();
    let lattice = Lattice::new(key.into()).unwrap();
    assert_eq!(lattice.implicit(0, 0).unwrap().degree(), 1);
}

#[test]
fn spans_outside_the_profile_and_degenerate_spans_are_refused() {
    let key = RevolvedKey::new(
        [0.0; 3],
        [0.0, 0.0, 1.0],
        &[[[1.0, 0.0, 1.0], [2.0, 1.0, 1.0], [3.0, 0.0, 1.0]]],
    )
    .unwrap();
    let lattice = Lattice::new(key.into()).unwrap();
    assert_eq!(lattice.implicit(0, 1), Err(ImplicitError::Index));
    assert_eq!(lattice.implicit(0, -1), Err(ImplicitError::Index));
    // A span folded back on itself: straight, its ends one point.
    let folded = [[1.0, 0.0, 1.0], [2.0, 0.0, 1.0], [1.0, 0.0, 1.0]];
    let key = RevolvedKey::new([0.0; 3], [0.0, 0.0, 1.0], &[folded]).unwrap();
    let lattice = Lattice::new(key.into()).unwrap();
    assert_eq!(lattice.implicit(0, 0), Err(ImplicitError::Degenerate));
}

#[test]
fn the_cylinder_s_is_the_leaves_quadric() {
    let key = CylinderKey::new([1.0, -2.0, 0.5], [0.3, 0.4, 1.2], 2.5).unwrap();
    let quadric = Quadric::cylinder(key.foot(), key.axis(), key.radius()).unwrap();
    let imp = TileImplicit::of_form(&key.into()).unwrap();
    let like = Bounded::exact(0.0);
    assert_eq!(imp.implicit_at(&like), quadric.implicit_form(&like));
    assert_eq!(
        imp.implicit_at(&Q::from(0u32)),
        quadric.implicit_form(&Q::from(0u32))
    );
}

#[test]
fn distances_of_tile_rows_are_their_rounding() {
    for Sample { name, surface } in surfaces(4) {
        // An ellipse's quarters from a pole: their rounded weight `√½`
        // makes the meridian a conic not quite symmetric about the axis's
        // normal, whose quartic `P² − ρ²·L²` is nearly a square (`L` of
        // the weight's rounding): its gradient vanishes to that rounding on
        // the surface, and no ball certifies a distance.
        if name.ends_with("(refined)") || name.ends_with("revolved poles") {
            continue;
        }
        // The rows along `u` at `v` 0 and 1 and along `v` at `u` 0 and 1,
        // leaving out rows that are one point (poles, apexes) and those
        // from such a point. A plane's
        // and the forms' along a line take a cell `2⁻²⁰` of the base one,
        // at the axis's foot (the cone's at its apex): the base cells
        // reach `CELL/2` along the line, where the distance's first level
        // can't tell its rounding from the geometry.
        let form = name.trim_start_matches("affine ");
        // A lattice tile's deviation is its base tile's (plus a rounding):
        // the scale is the base tiles'.
        let base = surface
            .tiles()
            .iter()
            .map(scale)
            .fold(0.0, |m, x| if x > m { x } else { m });
        let surface = match form {
            "plane" | "cylinder" | "cone" | "conic chain" | "hyperbola chain" => {
                let lattice = surface.lattice().clone();
                let r = surface.rect();
                // The cell at the foot (the middle of base cell 0), or at
                // the apex (its start).
                let at = |apex: bool| if apex { 0..1 } else { 1 << 19..(1 << 19) + 1 };
                let line_u = form == "plane";
                let rect = Rect {
                    level: Level {
                        u: if line_u { 20 } else { r.level.u },
                        v: 20,
                    },
                    u: if line_u { at(false) } else { r.u.clone() },
                    v: at(form == "cone"),
                };
                Surface::new(lattice, rect).unwrap()
            }
            _ => surface,
        };
        for (n, tile) in surface.tiles().iter().enumerate() {
            let (i, j) = surface.indices(n).unwrap();
            let imp = surface.implicit(i, j).unwrap().unwrap();
            let s = base;
            let net = tile.net();
            let rows = [[0, 3, 6], [2, 5, 8], [0, 1, 2], [6, 7, 8]];
            let one = |row: &[usize; 3]| {
                let span = row.map(|k| net[k]);
                span[0][..3] == span[1][..3] && span[1][..3] == span[2][..3]
            };
            // A pole's or apex's point, where a cone's or spindle's
            // implicit is singular: rows from it have no ball about them.
            let singular: Vec<[f64; 3]> = rows
                .iter()
                .filter(|r| one(r))
                .map(|r| [net[r[0]][0], net[r[0]][1], net[r[0]][2]])
                .collect();
            for row in rows {
                let span = row.map(|k| net[k]);
                let at = |p: &[f64; 4]| singular.iter().any(|q| q[..] == p[..3]);
                if one(&row) || at(&span[0]) || at(&span[2]) {
                    continue;
                }
                let curve = Curve3D::new(vec![span], vec![0.0, 1.0]).unwrap();
                let (bound, _) = alone(&imp, &curve, TOL * s, 1 << 22);
                let bound =
                    bound.unwrap_or_else(|| panic!("{name}: tile {n} row {row:?}: no bound"));
                assert!(
                    bound.bound <= TOL * s,
                    "{name}: tile {n} row {row:?}: {:e} of {s:e}",
                    bound.bound
                );
            }
        }
    }
}
