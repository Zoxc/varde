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
fn an_arc_across_a_tall_thin_wall_is_found_cheaply() {
    // A hole's rim, a quarter circle of radius 8, through a pin of
    // radius 1 standing 1 010 tall on it: the arc's control hull holds
    // the pin's whole width near the rim's height, so quartered pieces of
    // the pin's wall were split across its width as often as along its
    // height and none was dropped, and the search ran out of pieces.
    // Long pieces are halved across their long side instead.
    let tol = crate::Tolerance::DEFAULT;
    for angle in [0.3f64, 0.785, 1.2] {
        let (sin, cos) = crate::trig::sin_cos(angle);
        let centre = DVec3::new(8.0 * cos, 8.0 * sin, 0.0);
        let pin = crate::Solid::cylinder(centre - 5.0 * DVec3::Z, 1.0, 1010.0, 1, &tol).unwrap();
        let mesh = pin.mesh();
        let rim =
            crate::patch::Conic2::arc_between(DVec2::ZERO, 8.0, DVec2::X * 8.0, DVec2::Y * 8.0)
                .unwrap();
        let edge = Conic3 {
            p0: rim.p0.extend(0.0),
            c: rim.c.extend(0.0),
            w: rim.w,
            p1: rim.p1.extend(0.0),
        };
        let mut crossings = Vec::new();
        for t in 0..mesh.tris().len() {
            let (found, _, nodes) = edge_patch(&edge, &mesh.patch(t));
            assert!(nodes <= 200, "at {angle}, patch {t}: {nodes} pieces");
            for h in found.iter().filter(|h| h.out == 0.0) {
                crossings.push((h.x, edge.eval(h.t)));
            }
        }
        // In and out where the circles of radius 8 and 1 meet.
        assert_eq!(crossings.len(), 2, "at {angle}: {crossings:?}");
        let mut signs: Vec<i8> = crossings.iter().map(|c| c.0).collect();
        signs.sort();
        assert_eq!(signs, [-1, 1], "at {angle}: {crossings:?}");
        for (_, x) in &crossings {
            assert!((x.length() - 8.0).abs() < 1e-12, "at {angle}: {x}");
            assert!((x.distance(centre) - 1.0).abs() < 1e-12, "at {angle}: {x}");
        }
    }
}

#[test]
fn a_crossing_just_past_a_patch_side_is_found_with_the_hulls() {
    // A radial edge through a cylinder's wall a hair past a patch's
    // straight side, the wall turned off the axes so the boxes don't
    // part them: Newton's method counts the crossing (just outside the
    // patch) as the piece's, and the search keeps it with the hulls
    // tested as without them, as the count may want it there.
    let tol = crate::Tolerance::DEFAULT;
    let cyl = crate::Solid::cylinder(DVec3::ZERO, 3.0, 10.0, 1, &tol).unwrap();
    let mesh = cyl.mesh();
    let (sin, cos) = crate::trig::sin_cos(0.5);
    let turn = |p: DVec3| DVec3::new(cos * p.x - sin * p.y, sin * p.x + cos * p.y, p.z);
    let mut seen = 0;
    for t in 0..mesh.tris().len() {
        let patch = mesh.patch(t);
        let patch = Patch {
            p: patch.p.map(turn),
            c: patch.c.map(turn),
            w: patch.w,
        };
        // A side straight up the wall, and the corner off it.
        let Some(i) = (0..3).find(|&i| {
            let (p, q) = (patch.p[i], patch.p[(i + 1) % 3]);
            p.truncate() == q.truncate() && p.z != q.z
        }) else {
            continue;
        };
        let (side, other) = (patch.p[i], patch.p[(i + 2) % 3]);
        // A twentieth of a nanometre past the side, round the axis.
        let away = DVec3::Z.cross(side).dot(other - side).signum();
        let angle = crate::trig::atan2(side.y, side.x) - away * 5e-11 / 3.0;
        let (sin, cos) = crate::trig::sin_cos(angle);
        let at = DVec3::new(3.0 * cos, 3.0 * sin, 5.0);
        // Through it at 0.37, not on a split of the edge's range (where
        // the boxes would part the pieces either side).
        let (a, b) = (0.5 * at, (0.5 + 0.5 / 0.37) * at);
        let edge = segment(DVec3::new(a.x, a.y, 5.0), DVec3::new(b.x, b.y, 5.0));
        for hulls in [false, true] {
            let (found, _, _) = super::solve::edge_patch_with(&edge, &patch, hulls);
            assert_eq!(found.len(), 1, "patch {t}, hulls {hulls}: {found:?}");
            let h = &found[0];
            assert!(h.out > 0.0 && h.out < 1e-9, "patch {t}: {h:?}");
            assert!((h.t - 0.37).abs() < 1e-9, "patch {t}: {h:?}");
        }
        seen += 1;
    }
    assert!(seen >= 4, "{seen}");
}

#[test]
fn a_crossing_just_past_a_rim_at_a_split_or_an_end_is_found() {
    // An edge through a cylinder's wall a twentieth of a nanometre above
    // the wall's top rim (a side straight across the axes), where
    // Newton's method counts the crossing as the piece's. Once with the
    // edge's range split between the rim's height and the crossing, once
    // with the edge ending there, the crossing just past its end: the
    // edge's piece beyond the rim's height kept by its box as by its hull.
    let tol = crate::Tolerance::DEFAULT;
    let cyl = crate::Solid::cylinder(DVec3::ZERO, 3.0, 10.0, 1, &tol).unwrap();
    let mesh = cyl.mesh();
    let mut seen = 0;
    for t in 0..mesh.tris().len() {
        let patch = mesh.patch(t);
        let top: Vec<DVec3> = patch.p.into_iter().filter(|p| p.z == 10.0).collect();
        if top.len() != 2 || patch.p.iter().all(|p| p.z == 10.0) {
            continue;
        }
        let mid = (top[0] + top[1]).truncate().normalize();
        let at = |r: f64, z: f64| (mid * r).extend(z);
        // Out radially, rising a tenth over the edge: at its split (½)
        // 5e-11 above the rim, through the wall 5e-10 further along.
        let rising = segment(at(1.0 - 2e-9, 9.95 + 5e-11), at(5.0 - 2e-9, 10.05 + 5e-11));
        // Out radially, falling 4 over the edge: ends 5e-11 above the
        // rim, 1e-10 short of the wall, and would cross it 2.5e-11 on.
        let ending = segment(
            at(-1.0 - 1e-10, 14.0 + 5e-11),
            at(3.0 - 1e-10, 10.0 + 5e-11),
        );
        for (edge, want, out) in [
            (&rising, 0.5 + 5e-10, false),
            (&ending, 1.0 + 2.5e-11, true),
        ] {
            let (found, _, _) = edge_patch(edge, &patch);
            assert_eq!(found.len(), 1, "patch {t}, ending {out}: {found:?}");
            let h = &found[0];
            assert!(h.out > 0.0 && h.out < 1e-9, "patch {t}: {h:?}");
            assert!((h.t - want).abs() < 1e-12, "patch {t}: {h:?}, not {want}");
            assert_eq!(h.t > 1.0, out, "patch {t}: {h:?}");
        }
        seen += 1;
    }
    assert!(seen >= 4, "{seen}");
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

#[test]
#[allow(clippy::disallowed_methods, reason = "std maths to build inputs")]
fn first_orders_that_are_only_rounding_are_skipped() {
    // A motion lying in a turned plane, against that plane's normal: at
    // the exact tie its first order is zero and `T2` decides, but turned
    // and rounded, `δ·n` is some `1e-17`, its sign noise. Skipped as only
    // rounding, the sign is `T2`'s in every draw, as the exact
    // predicates decide such a tie.
    let mut rng = Rng::new(36);
    let mut noisy = 0;
    for draw in 0..2000 {
        let q = glam::DQuat::from_axis_angle(rng.direction(), rng.range(0.0, 6.0));
        let n = q * DVec3::Z;
        let delta = q * DVec3::new(1.0, 1.0, 0.0).normalize();
        let want = sign(exact::T2.dot(n));
        if exact::T2.dot(n).abs() < 1e-6 {
            continue;
        }
        if delta.dot(n) != 0.0 {
            noisy += 1;
        }
        assert_eq!(first_sign(delta, |d| n.dot(d)), want, "draw {draw}");
    }
    assert!(noisy > 1000, "{noisy}");
    // Rounding in the zero components of a computed direction or
    // gradient, times the other's large component, is rounding too,
    // though the sum of the products `Σ |δ_i·g_i|` has no cancellation to
    // see: measured where two crossing cylinders' seams meet on the cut (a
    // vertex direction, a normal at a refined corner, against `g′ × e′`
    // or a patch normal), and a gradient's own rounding. Each first order
    // is zero exactly; `T2` decides, not the rounding's sign.
    for (delta, g) in [
        (
            DVec3::new(1.0, 0.0, -7.093_142_705_686_246e-18),
            DVec3::new(0.0, 8.131_516_293_641_283e-20, -0.018_378_213_811_792_853),
        ),
        (
            DVec3::new(
                -0.999_999_999_999_999_8,
                2.133_459_107_055_383_7e-15,
                9.689_372_084_813_58e-16,
            ),
            DVec3::new(0.0, -0.013_800_283_404_586_423, 0.001_905_421_516_924_217_4),
        ),
        (DVec3::X, DVec3::new(1e-17, 0.0, -0.05)),
    ] {
        assert_ne!(delta.dot(g), 0.0);
        let want = sign(exact::T2.dot(g));
        assert_ne!(want, 0);
        assert_eq!(first_sign(delta, |d| g.dot(d)), want, "{delta} {g}");
    }
    // A first order that is small but real still decides.
    let n = DVec3::Z;
    let delta = DVec3::new(1.0, 1.0, 1e-6).normalize();
    assert_eq!(first_sign(delta, |d| -n.dot(d)), -1);
    let delta = DVec3::new(1.0, 1.0, 7e-7).normalize();
    assert_eq!(first_sign(delta, |d| -n.dot(d)), -1);
}

/// How the perturbed shadows of edge `e` of `A` and the same edge of `B`
/// (two copies of one solid) cross, found by sampling where `A`'s
/// perturbation takes `e`'s shadow across `g`'s, away from the ends: the
/// split into `a_under` and `b_under`, and the signed number.
fn sampled_split(prims: &Curved, e: u32) -> (i32, i32, i32) {
    let curve = prims.curve(Side::B, e);
    let side = |t: f64| {
        let (_, tangent) = curve.eval_deriv(t);
        prims.perturb_along(e, t).dot(UP.cross(tangent))
    };
    let n = 4096;
    let (mut a_under, mut b_under, mut found) = (0, 0, 0);
    let mut last = side(0.01);
    for k in 1..=n {
        let t = 0.01 + 0.98 * f64::from(k) / f64::from(n);
        let now = side(t);
        if (now > 0.0) != (last > 0.0) {
            let sigma = if now > 0.0 { 1 } else { -1 };
            if prims.parallel_above(e, t) {
                b_under -= sigma;
            } else {
                a_under += sigma;
            }
            found += sigma;
        }
        last = now;
    }
    (a_under, b_under, found)
}

#[test]
fn coincident_edges_cross_where_the_perturbation_parts_them() {
    // Two copies of one cylinder on frames whose caps lie nearly along
    // `UP`: each rim arc and wall diagonal of one lies on the other's.
    // Their perturbed shadows cross round the fold of the thin ellipses,
    // on some edges twice with `A` above at one crossing and below at the
    // other (both sums not zero), which one sample's height for every
    // crossing got wrong. Each split is that of the crossings found by
    // sampling, where they are all the ray tests count.
    use super::super::input::Input;
    use crate::profile::tests::circle;
    use crate::{Budget, Frame, Profile, extrude};
    #[allow(clippy::disallowed_methods, reason = "a test frame")]
    let q = glam::DQuat::from_axis_angle(DVec3::new(-0.3, 0.9, 0.2).normalize(), 2.3);
    let frames = [
        Frame {
            origin: DVec3::new(-40.0, 13.0, 5.5),
            x: q * DVec3::X,
            y: q * DVec3::Y,
        },
        Frame {
            origin: DVec3::ZERO,
            x: DVec3::Y,
            y: DVec3::Z,
        },
    ];
    let tol = Tolerance::DEFAULT;
    let (mut checked, mut both_ways) = (0, 0);
    for frame in &frames {
        let profile = Profile {
            loops: vec![circle(DVec2::new(0.5, 0.2), 1.0, 0, false)],
        };
        let solid = extrude(&profile, frame, 0.0, 1.0, 7, &tol, &Budget::DEFAULT).unwrap();
        let input = Input::new(solid.mesh(), &tol);
        for grow in [false, true] {
            let prims = Curved::new(&input, &input, grow, super::super::tie(&tol), &tol);
            for e in 0..input.edges.len() as u32 {
                if input.straight[e as usize] {
                    continue;
                }
                let split = prims.s11(e, e);
                let count = prims.shadow_crossings(e, e);
                assert_eq!(i32::from(split.a_under) - i32::from(split.b_under), count);
                let (a_under, b_under, found) = sampled_split(&prims, e);
                if found == count {
                    assert_eq!(
                        (i32::from(split.a_under), i32::from(split.b_under)),
                        (a_under, b_under),
                        "edge {e}, grow {grow}"
                    );
                    checked += 1;
                }
                if split.a_under != 0 && split.b_under != 0 {
                    both_ways += 1;
                }
            }
        }
    }
    assert!(checked >= 40, "{checked}");
    assert!(both_ways > 0);
}

/// The Bernstein coefficients of `(t − r0)(t − r1)(t − r2)`: from its
/// power coefficients `a`, `b_k = Σ_{i ≤ k} C(k, i) / C(3, i)·a_i`.
fn cubic([r0, r1, r2]: [f64; 3]) -> [f64; 4] {
    let a = [
        -r0 * r1 * r2,
        r0 * r1 + r0 * r2 + r1 * r2,
        -(r0 + r1 + r2),
        1.0,
    ];
    [
        a[0],
        a[0] + a[1] / 3.0,
        a[0] + 2.0 * a[1] / 3.0 + a[2] / 3.0,
        a[0] + a[1] + a[2] + a[3],
    ]
}

#[test]
fn a_double_root_found_twice_is_no_crossing() {
    // `σ`'s cubic touching zero at ¼ (a tangency, no crossing) and
    // crossing at ¾. Root isolation may give the double root as two
    // roots a rounding apart, or twice at one place, with the sign
    // between them zero; each root's sign change must be the one across
    // the whole cluster, not the sign after it alone (which counted a
    // crossing at the tangency).
    let (r, s) = (0.25, 0.75);
    let poly = cubic([r, r, s]);
    for t in [0.1, 0.25, 0.5, 0.75, 0.9] {
        let want = (t - r) * (t - r) * (t - s);
        assert!((bernstein::eval(&poly, t) - want).abs() < 1e-15, "{t}");
    }
    // At the double root itself the value is rounding, of either sign.
    assert!(bernstein::eval(&poly, r).abs() < 1e-17);
    let zero = 1e-15;
    assert_eq!(sign_changes(&poly, &[r, r, s], zero), vec![0, 0, 1]);
    assert_eq!(sign_changes(&poly, &[r, s], zero), vec![0, 1]);
    // A pair a rounding apart, the sign between them noise.
    for gap in [1e-17, 1e-16, 1e-12, 1e-9] {
        let pair = [r - gap, r + 3.0 * gap, s];
        assert_eq!(sign_changes(&poly, &pair, zero), vec![0, 0, 1], "{gap}");
    }
    // Simple roots alternate, and a real pair close together counts both.
    for rs in [[0.2, 0.5, 0.8], [0.4, 0.4 + 1e-6, 0.9]] {
        let poly = cubic(rs);
        let roots = bernstein::roots(&poly);
        assert_eq!(roots.len(), 3, "{roots:?}");
        assert_eq!(sign_changes(&poly, &roots, zero), vec![1, -1, 1], "{rs:?}");
    }
}

#[test]
fn heights_are_measured_square_to_the_plane_they_share() {
    // A height along `UP` counts as the distance square to the plane
    // with normal `m`: `dh` times the cosine of `m`'s angle to `UP`,
    // the cosine no less than `1/TIES` (a tie reaches the resolution
    // along `UP` at most) and no more than 1.
    let up = UP.normalize();
    let level = up.any_orthonormal_vector();
    let dh = 3.0;
    assert_eq!(square(dh, up * 5.0), dh);
    assert_eq!(square(-dh, -up), dh);
    let tilted = up * 0.5 + level * 0.75f64.sqrt();
    assert!((square(dh, tilted * 7.0) - dh * 0.5).abs() < 1e-12);
    // Nearly or exactly along `UP`'s square: capped.
    assert_eq!(square(dh, level), dh / super::super::TIES);
    let steep = level + up * 1e-6;
    assert_eq!(square(dh, steep), dh / super::super::TIES);
    // No direction to measure along: the height as it is.
    for m in [
        DVec3::ZERO,
        DVec3::splat(f64::NAN),
        DVec3::new(f64::INFINITY, 0.0, 0.0),
        DVec3::new(f64::INFINITY, f64::INFINITY, 1.0),
        // Its square underflows to zero.
        up * 1e-200,
        level * f64::from_bits(1),
    ] {
        assert_eq!(square(dh, m), dh, "{m}");
    }
    // Small but measurable: the same as at unit length, within rounding,
    // and never past the height.
    for scale in [1e-150, 1e-100, 1e100, 1e150] {
        let got = square(dh, tilted * scale);
        assert!((got - dh * 0.5).abs() < 1e-9, "{scale}: {got}");
        assert!(square(dh, up * scale) <= dh, "{scale}");
    }
    // A height that isn't a number stays one (the callers then decide
    // it as they did along `UP`).
    assert!(square(f64::NAN, up).is_nan());
}
