use glam::{DVec2, DVec3};

use super::super::exact::Pt;
use super::arcs::cross;
use super::ray::{RayEdge, ray};
use super::solve::{edge_patch, hits, near_patch};
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
        let derived = ray(b, &rh, true, &axes, 0.0)
            - ray(a, &rh, true, &axes, 0.0)
            - ray(c, &re, false, &axes, 0.0)
            + ray(d, &re, false, &axes, 0.0);
        assert_eq!(derived, want, "draw {draw}: {e:?} {h:?}");
        let solved = cross(&e, &h, &axes).unwrap();
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
                ray(v, &edge(true), ahead, &axes, 0.0),
                ray(v, &edge(false), ahead, &axes, 0.0)
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
fn points_above_a_steep_patch_are_found() {
    // A patch of a wall along `Z` (a parabolic cylinder's, 0.31 tall and
    // 0.64 across), seen from a vertex 1.08e-3 off the wall: the line
    // along `UP` (7° off `Z`) through it meets the patch 0.0147 below.
    // Every piece's shadow held the vertex's, and the search ran out of
    // pieces before it got to the one holding the point, though Newton's
    // method from the first pieces had ended there: none was found, the
    // winding number's point was taken to be above (most of the patch
    // is), and every edge at the vertex was given a crossing through the
    // patch that wasn't there (seen fuzzing walls over conics, seed 21,
    // case 172).
    let patch = Patch::new(
        [
            DVec3::new(-10.096793101389732, -9.390041076882708, -0.5000000000000002),
            DVec3::new(-10.163922332860405, -8.912621158639585, -0.6874999999999996),
            DVec3::new(-10.183732923230068, -8.760367304147838, -0.8124999999999987),
        ],
        [
            DVec3::new(-10.13226993361693, -9.151228319467872, -0.5937499999999998),
            DVec3::new(-10.174022328125144, -8.836483764568435, -0.7499999999999993),
            DVec3::new(-10.143590270971023, -9.075025321379323, -0.6562499999999993),
        ],
        [1.0; 3],
    )
    .unwrap();
    let v = DVec3::new(-10.172930452156635, -8.835752607009576, -0.75);
    let axes = Axes::new();
    let found = hits(&patch, v, &axes);
    assert_eq!(found.len(), 1, "{found:?}");
    let h = found[0];
    assert!(h.out == 0.0 && h.facing == -1, "{h:?}");
    assert!((h.dh + 0.014688874576312).abs() < 1e-12, "{}", h.dh);
    let p = patch.eval(h.u);
    assert!((p - v - axes.up * h.dh).length() < 1e-12, "{p}");
}

#[test]
#[allow(clippy::disallowed_methods, reason = "std maths to build inputs")]
fn points_above_a_wall_seen_nearly_along_it_are_found_once() {
    // Strips of a cylinder whose axis is 1e-4 off `UP`, seen from vertices
    // above and below points near its silhouette: the search runs out of
    // pieces, and Newton's method from each piece it got to ended at
    // another point of the stretch where the line along `UP` grazes the
    // wall (its shadow within the residual of the vertex's), 1e-9 apart:
    // up to ten points facing one way where the line meets the strip
    // once, which the winding number's fit then has to drop. Each point
    // is found at most once, and at most as many as the line's roots on
    // the strip (by the quadratic).
    let axes = Axes::new();
    let up = axes.up;
    let mut rng = Rng::new(1);
    let (mut seen, mut once) = (0, 0);
    for draw in 0..400 {
        let tilt = rng.direction();
        let a = (up + (tilt - up * up.dot(tilt)).normalize() * 1e-4).normalize();
        let (x, y) = (
            a.any_orthonormal_vector(),
            a.cross(a.any_orthonormal_vector()),
        );
        let (start, sweep, h) = (
            rng.range(0.0, std::f64::consts::TAU),
            rng.range(0.1, 1.5),
            rng.log_range(0.05, 4.0),
        );
        let bottom = Conic3::arc(DVec3::ZERO, x, y, 1.0, start, sweep).unwrap();
        let patch = crate::patch::cylinder_strip(&bottom, a * h).unwrap()[draw % 2];
        // Near the silhouette, where the wall's normal is square to `UP`.
        let d = up - a * a.dot(up);
        let side = a.cross(d).normalize() * if rng.unit() < 0.5 { 1.0 } else { -1.0 };
        let phi = rng.range(-0.05, 0.05);
        let radial = side * phi.cos() + d.normalize() * phi.sin();
        let v = radial + a * rng.range(0.0, h) + up * (rng.range(-1.0, 1.0) * h);
        // |w + s·d|² = 1 on the cylinder, along the line `v + s·UP`.
        let w = v - a * a.dot(v);
        let (qa, qb, qc) = (d.length_squared(), 2.0 * w.dot(d), w.length_squared() - 1.0);
        let disc = qb * qb - 4.0 * qa * qc;
        if disc < 1e-6 * qb * qb {
            continue;
        }
        let (mut on, mut doubt) = (0, false);
        for s in [-1.0, 1.0].map(|k| (-qb + k * disc.sqrt()) / (2.0 * qa)) {
            let tight = near_patch(v + up * s, &patch, 1e-9).0.is_some();
            on += usize::from(tight);
            doubt |= tight != near_patch(v + up * s, &patch, 1e-5).0.is_some();
        }
        if doubt {
            continue;
        }
        seen += 1;
        let found: Vec<_> = hits(&patch, v, &axes)
            .into_iter()
            .filter(|h| h.out == 0.0)
            .collect();
        assert!(found.len() <= on, "draw {draw}: {on} roots, {found:?}");
        once += usize::from(found.len() == on);
    }
    assert!(seen > 300 && once > seen * 9 / 10, "{seen} {once}");
}

#[test]
fn edge_crossings_are_on_both() {
    let mut rng = Rng::new(24);
    let mut seen = 0;
    for draw in 0..300 {
        let patch = random_patch(&mut rng);
        let curved = rng.unit() < 0.7;
        let edge = random_conic(&mut rng, curved);
        let (found, closest, _) = edge_patch(&edge, &patch);
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
    let edge = segment(DVec3::new(-2.0, y, 0.5), DVec3::new(2.0, y, 0.5));
    let mut entering = Vec::new();
    for t in 0..mesh.tris().len() {
        let patch = mesh.patch(t);
        let (found, _, _) = edge_patch(&edge, &patch);
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
fn an_edge_across_a_tall_wall_is_found_cheaply() {
    // A plate's cap edge across a drill 1010 tall and 6 wide: the edge's
    // box holds the wall's whole width, so only the height pruned pieces
    // by their boxes, and the row of pieces at the edge's height was split
    // across the width into more than the search's cap, one patch's
    // crossing not found. Pieces whose control hulls are apart are
    // dropped too.
    let tol = crate::Tolerance::DEFAULT;
    let centre = DVec3::new(-20.0, 10.0, 0.0);
    let r = 3.0;
    let drill = crate::Solid::cylinder(centre - 5.0 * DVec3::Z, r, 1010.0, 1, &tol).unwrap();
    let mesh = drill.mesh();
    let (a, b) = (DVec3::new(-30.0, 20.0, 0.0), DVec3::new(-8.0, 0.0, 0.0));
    let edge = segment(a, b);
    let mut crossings = Vec::new();
    for t in 0..mesh.tris().len() {
        let (found, _, nodes) = edge_patch(&edge, &mesh.patch(t));
        assert!(nodes <= 100, "patch {t}: {nodes} pieces");
        for h in found.iter().filter(|h| h.out == 0.0) {
            crossings.push((h.x, h.t));
        }
    }
    crossings.sort_by(|p, q| p.1.total_cmp(&q.1));
    // Along the edge from `a`: the foot of the perpendicular from the
    // axis at `s0`, the axis `d` from the edge.
    let along = b - a;
    let length = along.length();
    let s0 = (centre - a).dot(along) / length;
    let d = (centre - a).cross(along).length() / length;
    let half = (r * r - d * d).sqrt();
    let want = [(1, (s0 - half) / length), (-1, (s0 + half) / length)];
    assert_eq!(crossings.len(), 2, "{crossings:?}");
    for (got, want) in crossings.iter().zip(want) {
        assert_eq!(got.0, want.0, "{crossings:?}");
        assert!(
            (got.1 - want.1).abs() <= 1e-12,
            "{crossings:?}, not {want:?}"
        );
    }
}

#[test]
fn an_edge_grazing_a_cylinder_is_found_twice() {
    // A plate's cap edge running through a boss's rim a little inside it,
    // on the wall's bottom side: it passes in and out of the wall 0.068
    // apart, within one of the search's small pieces. Newton's method
    // found one and the piece was left: the count (0) then dropped it,
    // and the edge was taken for not crossing the boss at all.
    let wall = Patch::new(
        [
            DVec3::new(-0.7, -0.45, 1.0),
            DVec3::new(-0.05, -1.1, 1.0),
            DVec3::new(-0.05, -1.1, 2.0),
        ],
        [
            DVec3::new(-0.7, -1.1, 1.0),
            DVec3::new(-0.05, -1.1, 1.5),
            DVec3::new(-0.7, -1.1, 1.5),
        ],
        [
            std::f64::consts::FRAC_1_SQRT_2,
            1.0,
            std::f64::consts::FRAC_1_SQRT_2,
        ],
    )
    .unwrap();
    let edge = segment(
        DVec3::new(3.0, -2.0, 1.0),
        DVec3::new(-1.1953577324240583, -0.7969051549493722, 1.0),
    );
    let (found, _, _) = edge_patch(&edge, &wall);
    let signs: Vec<i8> = found.iter().map(|h| h.x).collect();
    assert_eq!(signs, vec![1, -1], "{found:?}");
    for h in &found {
        let p = edge.eval(h.t);
        let r = (p.truncate() - glam::DVec2::new(-0.05, -0.45)).length();
        assert!((r - 0.65).abs() < 1e-12, "{r}");
    }
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
    assert_eq!(pick(&found, 0, 0.5), vec![(1, 0.2, true), (-1, 0.6, true)]);
    // The count has one the search put just outside the patch.
    let found = [hit(0.2, 1, 0.0), hit(0.6, -1, 0.0), hit(0.9, 1, 1e-12)];
    assert_eq!(
        pick(&found, 1, 0.5),
        vec![(1, 0.2, true), (-1, 0.6, true), (1, 0.9, true)]
    );
    // The count has none of a crossing found near the edge's end.
    let found = [hit(1e-13, 1, 0.0), hit(0.5, -1, 0.0), hit(0.7, 1, 0.0)];
    assert_eq!(pick(&found, 0, 0.5), vec![(-1, 0.5, true), (1, 0.7, true)]);
    // One more than found, where it found the two meeting: placed there,
    // not solved.
    let found = [hit(0.3, -1, 1e-6), hit(0.6, 1, 0.0)];
    assert_eq!(pick(&found, 2, 0.5), vec![(1, 0.3, false), (1, 0.6, true)]);
    // Nothing found: at the closest place, not solved.
    assert_eq!(
        pick(&[], -2, 0.25),
        vec![(-1, 0.25, false), (-1, 0.25, false)]
    );
}

#[test]
fn shadows_along_each_other_are_told() {
    // An arc against itself, reversed, and against its halves: its shadow
    // lies on the other's conic, and there are no crossings to solve for.
    let axes = Axes::new();
    let mut rng = Rng::new(23);
    for _ in 0..50 {
        let e = random_conic(&mut rng, true);
        assert!(cross(&e, &e, &axes).is_none());
        assert!(cross(&e, &e.reversed(), &axes).is_none());
        for half in e.split_half().unwrap() {
            assert!(cross(&e, &half, &axes).is_none());
        }
        // Moved off along the rays, it crosses as before.
        let off = Conic3 {
            p0: e.p0 + axes.across * 0.3,
            c: e.c + axes.across * 0.3,
            ..e
        };
        let off = Conic3 {
            p1: e.p1 + axes.across * 0.3,
            ..off
        };
        assert!(cross(&e, &off, &axes).is_some());
    }
}

#[test]
fn points_near_a_patch_are_certified_and_others_not() {
    // Points of random patches, and the same off them along the normal
    // or out past a side: only the first are within a micrometre, and
    // they are found so.
    let mut rng = Rng::new(51);
    let within = 1e-6;
    let mut most = 0;
    for _ in 0..200 {
        let patch = random_patch(&mut rng);
        let at = DVec3::new(
            rng.range(0.1, 1.0),
            rng.range(0.1, 1.0),
            rng.range(0.1, 1.0),
        );
        let at = at / at.element_sum();
        let v = at.y;
        let x = patch.eval(at);
        let (d, nodes) = near_patch(x, &patch, within);
        assert!(d.is_some_and(|d| d <= within), "{d:?} after {nodes}");
        most = most.max(nodes);
        let n = patch.normal(at).normalize();
        assert_eq!(near_patch(x + n * 1e-4, &patch, within).0, None);
        // On the surface beyond a side of the triangle.
        let beyond = patch.eval(DVec3::new(-0.05, v, 1.05 - v));
        assert_eq!(near_patch(beyond, &patch, within).0, None);
    }
    assert!(most <= 256, "{most}");
}
