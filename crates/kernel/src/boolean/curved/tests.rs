use glam::{DVec2, DVec3};

use super::super::exact::Pt;
use super::arcs::cross;
use super::ray::{RayEdge, ray};
use super::solve::{edge_patch, hits};
use super::*;
use crate::patch::{Conic3, Patch};
use crate::test_rng::Rng;

/// A random curve within the unit box, straight or bulging, with a
/// weight between ⅓ and 3.
fn random_conic(rng: &mut Rng, curved: bool) -> Conic3 {
    let (p0, p1) = (rng.point(1.0), rng.point(1.0));
    let c = if curved {
        (p0 + p1) * 0.5 + rng.point(0.6)
    } else {
        (p0 + p1) * 0.5
    };
    let w = if curved {
        rng.log_range(1.0 / 3.0, 3.0)
    } else {
        1.0
    };
    Conic3 { p0, c, w, p1 }
}

/// A random patch: corners well apart, control points off the edges'
/// midpoints, weights between ½ and 2.
fn random_patch(rng: &mut Rng) -> Patch {
    loop {
        let p = [rng.point(1.0), rng.point(1.0), rng.point(1.0)];
        if (p[1] - p[0]).cross(p[2] - p[0]).length() < 0.5 {
            continue;
        }
        let c = [0, 1, 2].map(|i| (p[i] + p[(i + 1) % 3]) * 0.5 + rng.point(0.3));
        let w = [0, 1, 2].map(|_| rng.log_range(0.5, 2.0));
        return Patch::new(p, c, w).unwrap();
    }
}

fn flat2(p: DVec3, axes: &Axes) -> DVec2 {
    DVec2::new(p.dot(axes.along), p.dot(axes.across))
}

fn cross2(a: DVec2, b: DVec2) -> f64 {
    a.x * b.y - a.y * b.x
}

/// The shadow of `c` as a polyline of `n` segments.
fn polyline(c: &Conic3, n: usize, axes: &Axes) -> Vec<DVec2> {
    (0..=n)
        .map(|k| flat2(c.eval(k as f64 / n as f64), axes))
        .collect()
}

/// The crossings of two polylines: for each, the parameters along both
/// (0 to 1) and the sine of the angle `det[h', e', UP]` between them.
fn polyline_crossings(e: &[DVec2], h: &[DVec2]) -> Vec<(f64, f64, f64)> {
    let mut out = Vec::new();
    let (ne, nh) = ((e.len() - 1) as f64, (h.len() - 1) as f64);
    for i in 0..e.len() - 1 {
        let (a, b) = (e[i], e[i + 1]);
        for j in 0..h.len() - 1 {
            let (c, d) = (h[j], h[j + 1]);
            let den = cross2(b - a, d - c);
            if den == 0.0 {
                continue;
            }
            let s = cross2(c - a, d - c) / den;
            let t = cross2(c - a, b - a) / den;
            if (0.0..1.0).contains(&s) && (0.0..1.0).contains(&t) {
                let sine = cross2(d - c, b - a) / ((d - c).length() * (b - a).length());
                out.push(((i as f64 + s) / ne, (j as f64 + t) / nh, sine));
            }
        }
    }
    out
}

fn a_pt(p: DVec3, rng: &mut Rng) -> Pt {
    Pt {
        p,
        n: Some(rng.direction()),
    }
}

fn b_pt(p: DVec3) -> Pt {
    Pt { p, n: None }
}

#[test]
fn shadow_crossings_come_from_the_rays() {
    // `I(e, h) = ρ(b, h) − ρ(a, h) − ρ⁻(c, e) + ρ⁻(d, e)` is the signed
    // number of crossings of the shadows, and the solve finds them.
    let axes = Axes::new();
    let mut rng = Rng::new(21);
    let mut checked = 0;
    for draw in 0..200 {
        let (ce, ch) = (rng.unit() < 0.8, rng.unit() < 0.8);
        let (e, h) = (random_conic(&mut rng, ce), random_conic(&mut rng, ch));
        let (pe, ph) = (polyline(&e, 400, &axes), polyline(&h, 400, &axes));
        let found = polyline_crossings(&pe, &ph);
        // Only draws the polylines decide: crossings clear of the ends
        // and not grazing, and no end near the other curve.
        let near_end = |p: DVec2, line: &[DVec2]| line.iter().any(|&q| (p - q).length() < 1e-2);
        if found
            .iter()
            .any(|&(t, s, sine)| t.min(1.0 - t).min(s).min(1.0 - s) < 1e-2 || sine.abs() < 0.05)
            || near_end(pe[0], &ph)
            || near_end(pe[400], &ph)
            || near_end(ph[0], &pe)
            || near_end(ph[400], &pe)
        {
            continue;
        }
        checked += 1;
        let want: i32 = found.iter().map(|&(_, _, sine)| sine.signum() as i32).sum();
        let (a, b) = (a_pt(e.p0, &mut rng), a_pt(e.p1, &mut rng));
        let (c, d) = (b_pt(h.p0), b_pt(h.p1));
        let re = RayEdge {
            c: a,
            d: b,
            conic: e,
            straight: !ce,
        };
        let rh = RayEdge {
            c,
            d,
            conic: h,
            straight: !ch,
        };
        let derived =
            ray(b, &rh, true, &axes) - ray(a, &rh, true, &axes) - ray(c, &re, false, &axes)
                + ray(d, &re, false, &axes);
        assert_eq!(derived, want, "draw {draw}: {e:?} {h:?}");
        let solved = cross(&e, &h, &axes);
        assert_eq!(solved.len(), found.len(), "draw {draw}");
        for (s, f) in solved.iter().zip(&found) {
            assert!(
                (s.t - f.0).abs() < 1e-3 && (s.s - f.1).abs() < 1e-3,
                "draw {draw}"
            );
            assert_eq!(s.sigma, f.2.signum() as i8, "draw {draw}");
            let dh = (e.eval(s.t) - h.eval(s.s)).dot(axes.up);
            assert!((s.dh - dh).abs() < 1e-9);
        }
    }
    assert!(checked > 120, "{checked}");
}

#[test]
fn a_straight_edge_rays_alike_exactly_and_not() {
    // The exact path for straight edges and the numerical one agree away
    // from ties.
    let axes = Axes::new();
    let mut rng = Rng::new(22);
    for _ in 0..2000 {
        let g = random_conic(&mut rng, false);
        let v = a_pt(rng.point(1.0), &mut rng);
        let (c, d) = (b_pt(g.p0), b_pt(g.p1));
        let edge = |straight| RayEdge {
            c,
            d,
            conic: g,
            straight,
        };
        for ahead in [true, false] {
            assert_eq!(
                ray(v, &edge(true), ahead, &axes),
                ray(v, &edge(false), ahead, &axes)
            );
        }
    }
}

#[test]
fn points_above_a_vertex_add_up_to_its_winding_number() {
    // The signed number of points of a patch straight above or below a
    // point is how often the patch's shadow's boundary winds round it.
    let axes = Axes::new();
    let mut rng = Rng::new(23);
    let mut checked = 0;
    for draw in 0..300 {
        let patch = random_patch(&mut rng);
        let boundary: Vec<DVec2> = (0..3)
            .flat_map(|i| {
                let mut line = polyline(&patch.edge(i), 2000, &axes);
                line.pop();
                line
            })
            .collect();
        let b = patch.bounds();
        let v = b.min + (b.max - b.min) * DVec3::new(rng.unit(), rng.unit(), rng.unit());
        let q = flat2(v, &axes);
        // Clear of the boundary, where the polyline decides.
        let n = boundary.len();
        let near = (0..n).any(|k| {
            let (x, y) = (boundary[k], boundary[(k + 1) % n]);
            let t = ((q - x).dot(y - x) / (y - x).length_squared()).clamp(0.0, 1.0);
            (x + (y - x) * t - q).length() < 1e-3
        });
        if near {
            continue;
        }
        let winding: i32 = (0..n)
            .map(|k| {
                let (x, y) = (boundary[k] - q, boundary[(k + 1) % n] - q);
                // Crossings of the ray along +x.
                if (x.y > 0.0) != (y.y > 0.0) {
                    let at = x.x + (y.x - x.x) * (-x.y) / (y.y - x.y);
                    if at > 0.0 {
                        return if y.y > 0.0 { 1 } else { -1 };
                    }
                }
                0
            })
            .sum();
        let found = hits(&patch, v, &axes);
        for h in &found {
            let p = patch.eval(h.u);
            assert!((flat2(p, &axes) - q).length() < 1e-8, "draw {draw}");
            assert!((h.dh - (p - v).dot(axes.up)).abs() < 1e-12);
        }
        // Where the patch is nearly vertical, a fold's two points may be
        // found one without the other; only draws clear of that count.
        let steep = found
            .iter()
            .any(|h| patch.normal(h.u).normalize().dot(axes.up).abs() < 0.05);
        if steep {
            continue;
        }
        checked += 1;
        let sum: i32 = found
            .iter()
            .filter(|h| h.out == 0.0)
            .map(|h| i32::from(h.facing))
            .sum();
        assert_eq!(sum, winding, "draw {draw}: {patch:?} {v}");
    }
    assert!(checked > 200, "{checked}");
}

#[test]
fn edge_crossings_are_on_both() {
    let mut rng = Rng::new(24);
    let mut seen = 0;
    for draw in 0..300 {
        let patch = random_patch(&mut rng);
        let curved = rng.unit() < 0.7;
        let edge = random_conic(&mut rng, curved);
        let (found, closest) = edge_patch(&edge, &patch);
        assert!((0.0..=1.0).contains(&closest));
        for h in &found {
            let gap = (edge.eval(h.t) - patch.eval(h.u)).length();
            assert!(gap < 1e-8, "draw {draw}: {gap}");
            let (_, d) = edge.eval_deriv(h.t);
            let n = patch.normal(h.u);
            assert_eq!(h.x, -(d.dot(n).signum() as i8));
            seen += usize::from(h.out == 0.0);
        }
    }
    assert!(seen > 20, "{seen}");
}

#[test]
fn an_edge_through_a_cylinder_is_found_where_it_is() {
    // A line along x through the unit cylinder about z at height ½: it
    // enters at x = −√(1 − y²) and leaves at +√(1 − y²).
    let tol = crate::Tolerance::DEFAULT;
    let cyl = crate::Solid::cylinder(DVec3::ZERO, 1.0, 1.0, 1, &tol).unwrap();
    let mesh = cyl.mesh();
    let y: f64 = 0.3;
    let edge = chord(DVec3::new(-2.0, y, 0.5), DVec3::new(2.0, y, 0.5));
    let mut entering = Vec::new();
    for t in 0..mesh.tris().len() {
        let patch = mesh.patch(t);
        let (found, _) = edge_patch(&edge, &patch);
        for h in found.iter().filter(|h| h.out == 0.0) {
            let x = edge.eval(h.t).x;
            entering.push((h.x, x));
        }
    }
    entering.sort_by(|a, b| a.1.total_cmp(&b.1));
    // Found once each, even where the diagonal between a wall's two
    // patches runs.
    let r = (1.0 - y * y).sqrt();
    assert_eq!(entering.len(), 2, "{entering:?}");
    assert_eq!(entering[0].0, 1);
    assert_eq!(entering[1].0, -1);
    assert!((entering[0].1 + r).abs() < 1e-12, "{entering:?}");
    assert!((entering[1].1 - r).abs() < 1e-12, "{entering:?}");
}

#[test]
fn picked_crossings_add_up_to_the_count() {
    let hit = |t: f64, x: i8, out: f64| solve::EdgeHit {
        t,
        u: DVec3::splat(1.0 / 3.0),
        x,
        out,
    };
    // As found.
    let found = [hit(0.2, 1, 0.0), hit(0.6, -1, 0.0)];
    assert_eq!(pick(&found, 0, 0.5), vec![(1, 0.2), (-1, 0.6)]);
    // The count has one the search put just outside the patch.
    let found = [hit(0.2, 1, 0.0), hit(0.6, -1, 0.0), hit(0.9, 1, 1e-12)];
    assert_eq!(pick(&found, 1, 0.5), vec![(1, 0.2), (-1, 0.6), (1, 0.9)]);
    // The count has none of a crossing found near the edge's end.
    let found = [hit(1e-13, 1, 0.0), hit(0.5, -1, 0.0), hit(0.7, 1, 0.0)];
    assert_eq!(pick(&found, 0, 0.5), vec![(-1, 0.5), (1, 0.7)]);
    // Nothing found: at the closest place.
    assert_eq!(pick(&[], -2, 0.25), vec![(-1, 0.25), (-1, 0.25)]);
}
