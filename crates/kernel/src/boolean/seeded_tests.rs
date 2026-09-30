//! The seeded random suite: booleans of extruded and primitive solids in
//! the configurations CAD makes on purpose (flush, coaxial, stacked,
//! nested, a pin in its hole, tangent, a hair apart, holes drilled one
//! after another) and at random, alone and in chains each fed the
//! previous result.
//!
//! Every operation must give a solid that is right, or fail: never a
//! wrong one, and never a panic. Right means it passes `check` (every
//! `Solid` does, face tags included), the four results of a pair keep the
//! volume identities `|A ∪ B| + |A ∩ B| = |A| + |B|` and `|A − B| = |A| −
//! |A ∩ B|` within the fit tolerance, analytic volumes where there are
//! some, and points sampled around the operands are inside the result
//! exactly when the operation says (by winding numbers of the operands'
//! and the result's tessellations, away from their surfaces). Failures
//! are allowed but counted, and each test holds its share of successes
//! above a floor, so a change that refuses more shows.
//!
//! In release builds the whole suite runs in well under a minute; debug
//! builds run a case of most (`cases`), and none of the slowest, which
//! other tests cover there.

#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::f64::consts::PI;

use glam::{DMat3, DQuat, DVec2, DVec3, Vec3};

use super::*;
use crate::mesh::{Quadric, Surface};
use crate::par::assert_deterministic;
use crate::profile::tests::{arc, circle, rect};
use crate::test_rng::Rng;
use crate::{Display, Frame, Loop, Profile, Segment, extrude};

/// `release` cases in a release build, `debug` in a debug one.
fn cases(release: usize, debug: usize) -> usize {
    if cfg!(debug_assertions) {
        debug
    } else {
        release
    }
}

/// `solid` moved by the rigid motion `f`, its face tags too.
fn moved(solid: &Solid, tol: &Tolerance, f: impl Fn(DVec3) -> DVec3) -> Solid {
    let mesh = solid.mesh();
    let origin = f(DVec3::ZERO);
    let turn = |d: DVec3| f(d) - origin;
    let r = DMat3::from_cols(turn(DVec3::X), turn(DVec3::Y), turn(DVec3::Z));
    let mut builder = crate::mesh::MeshBuilder::new();
    for &p in mesh.verts() {
        builder.vert(f(p));
    }
    for &face in mesh.faces() {
        let surface = match face.surface {
            Surface::Plane { n, d } => {
                let n = r * n;
                Surface::Plane {
                    n,
                    d: d + n.dot(origin),
                }
            }
            Surface::Quadric(q) => Surface::Quadric(Quadric {
                origin: f(q.origin),
                a: r * q.a * r.transpose(),
                b: r * q.b,
                c: q.c,
            }),
            Surface::Free => Surface::Free,
        };
        builder.face(crate::mesh::Face { surface, ..face });
    }
    for (t, tri) in mesh.tris().iter().enumerate() {
        let corners = tri.halfedges.map(|h| h.start);
        let patch = mesh.patch(t);
        for i in 0..3 {
            builder.edge(corners[i], corners[(i + 1) % 3], f(patch.c[i]), patch.w[i]);
        }
        builder.tri(corners, tri.face);
    }
    Solid::new(builder.build().unwrap(), tol).unwrap()
}

/// A stadium along `x` round `c`: straight sides `half` either way of
/// it, half circles of radius `r` at the ends.
fn slot(c: DVec2, half: f64, r: f64, curve: u64) -> Loop {
    let p = |x: f64, y: f64| c + DVec2::new(x, y);
    Loop {
        segments: vec![
            Segment::line(p(-half, -r), p(half, -r), curve).unwrap(),
            arc(p(half, 0.0), p(half, -r), p(half + r, 0.0), curve + 1),
            arc(p(half, 0.0), p(half + r, 0.0), p(half, r), curve + 1),
            Segment::line(p(half, r), p(-half, r), curve + 2).unwrap(),
            arc(p(-half, 0.0), p(-half, r), p(-half - r, 0.0), curve + 3),
            arc(p(-half, 0.0), p(-half - r, 0.0), p(-half, -r), curve + 3),
        ],
    }
}

/// A rectangle from `min` to `max` with its corners rounded to `r`.
fn rounded(min: DVec2, max: DVec2, r: f64, curve: u64) -> Loop {
    let p = |x: f64, y: f64| DVec2::new(x, y);
    let (x0, y0, x1, y1) = (min.x, min.y, max.x, max.y);
    Loop {
        segments: vec![
            Segment::line(p(x0 + r, y0), p(x1 - r, y0), curve).unwrap(),
            arc(p(x1 - r, y0 + r), p(x1 - r, y0), p(x1, y0 + r), curve + 1),
            Segment::line(p(x1, y0 + r), p(x1, y1 - r), curve + 2).unwrap(),
            arc(p(x1 - r, y1 - r), p(x1, y1 - r), p(x1 - r, y1), curve + 3),
            Segment::line(p(x1 - r, y1), p(x0 + r, y1), curve + 4).unwrap(),
            arc(p(x0 + r, y1 - r), p(x0 + r, y1), p(x0, y1 - r), curve + 5),
            Segment::line(p(x0, y1 - r), p(x0, y0 + r), curve + 6).unwrap(),
            arc(p(x0 + r, y0 + r), p(x0, y0 + r), p(x0 + r, y0), curve + 7),
        ],
    }
}

/// The loops extruded on `frame` from `from` to `to`, if they can be.
fn extruded(
    loops: Vec<Loop>,
    frame: &Frame,
    from: f64,
    to: f64,
    feature: u64,
    tol: &Tolerance,
) -> Option<Solid> {
    extrude(
        &Profile { loops },
        frame,
        from,
        to,
        feature,
        tol,
        &Budget::DEFAULT,
    )
    .ok()
}

/// The triangles of `solid`'s tessellation.
fn tris_of(solid: &Solid, tol: &Tolerance) -> Vec<[Vec3; 3]> {
    if solid.is_empty() {
        return Vec::new();
    }
    let mesh = solid.tessellate(&Display::new(tol)).unwrap();
    let pos = mesh.positions();
    mesh.indices()
        .chunks(3)
        .map(|c| [0, 1, 2].map(|k| Vec3::from(pos[c[k] as usize])))
        .collect()
}

/// The winding number of the triangles round `p`, by solid angles.
fn winding(tris: &[[Vec3; 3]], p: Vec3) -> f64 {
    let mut sum = 0.0f64;
    for t in tris {
        let [a, b, c] = t.map(|v| (v - p).as_dvec3());
        let (la, lb, lc) = (a.length(), b.length(), c.length());
        let num = a.dot(b.cross(c));
        let den = la * lb * lc + a.dot(b) * lc + b.dot(c) * la + c.dot(a) * lb;
        sum += 2.0 * num.atan2(den);
    }
    sum / (4.0 * PI)
}

/// The distance from `p` to the triangle `a`, `b`, `c`.
fn to_triangle(p: DVec3, [a, b, c]: [DVec3; 3]) -> f64 {
    let n = (b - a).cross(c - a);
    let inside = [(a, b), (b, c), (c, a)]
        .iter()
        .all(|&(u, v)| (v - u).cross(p - u).dot(n) >= 0.0);
    if inside && n.length_squared() > 0.0 {
        return (p - a).dot(n).abs() / n.length();
    }
    [(a, b), (b, c), (c, a)]
        .iter()
        .map(|&(u, v)| {
            let d = v - u;
            let t = ((p - u).dot(d) / d.length_squared().max(f64::MIN_POSITIVE)).clamp(0.0, 1.0);
            p.distance(u + d * t)
        })
        .fold(f64::INFINITY, f64::min)
}

/// Points sampled in the box round `a` and `b` whose membership in the
/// result `r` of `a op b` isn't what the operation says, away (a small
/// fraction of the size) from both operands' surfaces.
fn wrong_points(a: &Solid, b: &Solid, op: Op, r: &Solid, tol: &Tolerance, rng: &mut Rng) -> usize {
    let samples = cases(24, 0);
    if samples == 0 {
        return 0;
    }
    let (ta, tb, tr) = (tris_of(a, tol), tris_of(b, tol), tris_of(r, tol));
    let (Some(ba), Some(bb)) = (a.bounds3(), b.bounds3()) else {
        return 0;
    };
    let (lo, hi) = (ba.min.min(bb.min), ba.max.max(bb.max));
    let margin = 0.01 + 3e-3 * (hi - lo).length();
    let near = |tris: &[[Vec3; 3]], p: DVec3| {
        tris.iter()
            .any(|t| to_triangle(p, t.map(|v| v.as_dvec3())) < margin)
    };
    let mut wrong = 0;
    for _ in 0..samples {
        let p = lo + (hi - lo) * DVec3::new(rng.unit(), rng.unit(), rng.unit());
        if near(&ta, p) || near(&tb, p) {
            continue;
        }
        let q = p.as_vec3();
        let (ia, ib) = (winding(&ta, q) > 0.5, winding(&tb, q) > 0.5);
        let w = winding(&tr, q);
        let want = match op {
            Op::Union => ia || ib,
            Op::Intersection => ia && ib,
            Op::Difference => ia && !ib,
        };
        if (w - w.round()).abs() > 0.1 || (w > 0.5) != want {
            wrong += 1;
        }
    }
    wrong
}

/// What a batch of operations came to.
#[derive(Debug, Default)]
struct Tally {
    ok: usize,
    failed: usize,
}

impl Tally {
    /// Asserts at least `share` of the operations worked, when there are
    /// enough of them to tell (not in the few cases of a debug build).
    fn at_least(&self, share: f64, name: &str) {
        let total = self.ok + self.failed;
        if total < 40 {
            return;
        }
        assert!(
            self.ok as f64 >= share * total as f64,
            "{name}: only {} of {total} worked",
            self.ok
        );
    }
}

/// `a ∪ b`, `a ∩ b`, `a − b` and `b − a`, each right or failed: see the
/// module docs. `both`, if known, is `|a ∩ b|`. Gives the results.
fn four(
    a: &Solid,
    b: &Solid,
    both: Option<f64>,
    tol: &Tolerance,
    rng: &mut Rng,
    tally: &mut Tally,
    name: &str,
) -> [Option<Solid>; 4] {
    let jobs = [
        (a, b, Op::Union),
        (a, b, Op::Intersection),
        (a, b, Op::Difference),
        (b, a, Op::Difference),
    ];
    let out = jobs.map(
        |(x, y, op)| match boolean(x, y, op, tol, &Budget::DEFAULT) {
            Ok(solid) => {
                let wrong = wrong_points(x, y, op, &solid, tol, rng);
                assert_eq!(wrong, 0, "{name}, {op:?}: {wrong} points on the wrong side");
                tally.ok += 1;
                Some(solid)
            }
            Err(_) => {
                tally.failed += 1;
                None
            }
        },
    );
    let (va, vb) = (a.volume(), b.volume());
    let within = tol.fit() * (a.area() + b.area()) / 5.0 + 1e-9;
    let v = |k: usize| out[k].as_ref().map(Solid::volume);
    let close = |x: f64, y: f64, what: &str| {
        assert!((x - y).abs() <= within, "{name}: {what}: {x} vs {y}");
    };
    if let (Some(u), Some(i)) = (v(0), v(1)) {
        close(u + i, va + vb, "union and intersection");
    }
    if let (Some(d), Some(i)) = (v(2), v(1)) {
        close(d, va - i, "a less b");
    }
    if let (Some(e), Some(i)) = (v(3), v(1)) {
        close(e, vb - i, "b less a");
    }
    if let Some(both) = both {
        let want = [va + vb - both, both, va - both, vb - both];
        for (k, want) in want.into_iter().enumerate() {
            if let Some(got) = v(k) {
                close(got, want, &format!("result {k}"));
            }
        }
    }
    for k in 0..4 {
        if let Some(x) = v(k) {
            assert!(x >= -within, "{name}: result {k} of volume {x}");
        }
    }
    out
}

/// A random frame: one of the sketch planes through a random point, or
/// turned off every axis.
fn frame(rng: &mut Rng, snap: &impl Fn(f64) -> f64) -> Frame {
    let origin = DVec3::new(
        snap(rng.range(-1.0, 1.0)),
        snap(rng.range(-1.0, 1.0)),
        snap(rng.range(-1.0, 1.0)),
    );
    let (x, y) = match rng.next_u64() % 4 {
        0 => (DVec3::X, DVec3::Y),
        1 => (DVec3::Y, DVec3::Z),
        2 => (DVec3::Z, DVec3::X),
        _ => {
            let q = DQuat::from_rotation_x(rng.range(-1.0, 1.0))
                * DQuat::from_rotation_y(rng.range(-1.0, 1.0));
            (q * DVec3::X, q * DVec3::Y)
        }
    };
    Frame { origin, x, y }
}

/// Two solids related as CAD makes them, on one random frame: stacked
/// flush on the other's top, over the same span, inside its span,
/// crossing it, or a bar across it; the second a cylinder or a square
/// prism round the same centre.
fn related(rng: &mut Rng, snap: bool, tol: &Tolerance) -> Option<(Solid, Solid)> {
    let s = |x: f64| if snap { (x * 4.0).round() / 4.0 } else { x };
    let frame = frame(rng, &s);
    let mode = rng.next_u64() % 5;
    let c = DVec2::new(s(rng.range(-0.5, 0.5)), s(rng.range(-0.5, 0.5)));
    let r1 = s(rng.range(0.5, 1.2)).max(0.5);
    let r2 = s(rng.range(0.25, 1.2)).max(0.25);
    let (f1, t1) = (s(rng.range(-1.0, 0.0)), s(rng.range(0.25, 1.5)).max(0.25));
    let (f2, t2) = match mode {
        0 => (t1, t1 + s(rng.range(0.25, 1.0)).max(0.25)),
        1 => (f1, t1),
        2 => (f1 + 0.25 * (t1 - f1), t1 - 0.25 * (t1 - f1)),
        _ => (s(rng.range(-1.5, 0.5)), s(rng.range(0.5, 2.0))),
    };
    let c2 = if mode == 3 {
        c + DVec2::new(s(rng.range(-0.5, 0.5)), s(rng.range(-0.5, 0.5)))
    } else {
        c
    };
    let a = extruded(vec![circle(c, r1, 0, false)], &frame, f1, t1, 1, tol)?;
    let b = if mode == 4 {
        let across = Frame {
            origin: frame.origin + frame.y * c.y,
            x: frame.y,
            y: frame.x.cross(frame.y),
        };
        let z = s(rng.range(f1, t1));
        let r = r2.min(r1);
        extruded(
            vec![circle(DVec2::new(0.0, z), r, 0, false)],
            &across,
            c.x - 2.0,
            c.x + 2.0,
            2,
            tol,
        )?
    } else if rng.next_u64().is_multiple_of(2) {
        extruded(vec![circle(c2, r2, 0, false)], &frame, f2, t2, 2, tol)?
    } else {
        let h = DVec2::splat(r2);
        extruded(vec![rect(c2 - h, c2 + h, 0)], &frame, f2, t2, 2, tol)?
    };
    Some((a, b))
}

#[test]
fn related_solids_are_right_or_refused() {
    let tol = Tolerance::DEFAULT;
    let mut tally = Tally::default();
    for snap in [true, false].into_iter().take(cases(2, 1)) {
        let mut rng = Rng::new(11 + u64::from(snap));
        let mut samples = Rng::new(12);
        for case in 0..cases(15, 1) {
            let Some((a, b)) = related(&mut rng, snap, &tol) else {
                continue;
            };
            let name = format!("related, snap {snap}, case {case}");
            four(&a, &b, None, &tol, &mut samples, &mut tally, &name);
        }
    }
    tally.at_least(0.75, "related");
}

/// A random solid of a part: a plate, a boss, a slot, a rounded block, a
/// plate with holes, extruded on a sketch plane through the origin, with
/// dimensions on a grid of `step`.
fn part(rng: &mut Rng, step: f64, feature: u64, tol: &Tolerance) -> Option<Solid> {
    let v = |rng: &mut Rng, lo: f64, hi: f64| ((rng.range(lo, hi) / step).round() * step).max(step);
    let frame = match rng.next_u64() % 3 {
        0 => Frame::XY,
        1 => Frame {
            origin: DVec3::ZERO,
            x: DVec3::X,
            y: DVec3::Z,
        },
        _ => Frame {
            origin: DVec3::ZERO,
            x: DVec3::Y,
            y: DVec3::Z,
        },
    };
    let c = DVec2::new(
        (rng.range(-2.0, 2.0) / step).round() * step,
        (rng.range(-2.0, 2.0) / step).round() * step,
    );
    let loops = match rng.next_u64() % 6 {
        0 | 1 => vec![circle(c, v(rng, 0.25, 1.5), 0, false)],
        2 => {
            let h = DVec2::new(v(rng, 0.5, 4.0), v(rng, 0.5, 4.0)) / 2.0;
            vec![rect(c - h, c + h, 0)]
        }
        3 => {
            let (half, r) = (v(rng, 0.25, 1.5), v(rng, 0.25, 0.75));
            vec![slot(c, half, r, 0)]
        }
        4 => {
            let h = DVec2::new(v(rng, 1.0, 4.0), v(rng, 1.0, 4.0)) / 2.0;
            let r = v(rng, 0.25, 0.5).min(h.min_element() - step);
            vec![rounded(c - h, c + h, r, 0)]
        }
        _ => {
            let h = DVec2::new(v(rng, 3.0, 6.0), v(rng, 2.0, 4.0)) / 2.0;
            vec![
                rect(c - h, c + h, 0),
                circle(c - DVec2::new(h.x / 2.0, 0.0), 0.5, 10, true),
                circle(c + DVec2::new(h.x / 2.0, 0.0), 0.5, 20, true),
            ]
        }
    };
    let from = (rng.range(-2.0, 1.5) / step).round() * step;
    let to = from + v(rng, 0.25, 2.5);
    extruded(loops, &frame, from, to, feature, tol)
}

#[test]
fn parts_built_in_chains_of_twenty_are_right_or_refused() {
    // As a user builds a part: solids on the sketch planes, joined, cut
    // and now and then intersected, each result fed on.
    let tol = Tolerance::DEFAULT;
    let mut tally = Tally::default();
    let mut rng = Rng::new(21);
    let mut samples = Rng::new(22);
    for chain in 0..cases(3, 1) {
        let Some(mut current) = part(&mut rng, 0.25, 1, &tol) else {
            continue;
        };
        for step in 0..cases(20, 3) {
            let Some(tool) = part(&mut rng, 0.25, 10 + step as u64, &tol) else {
                continue;
            };
            let name = format!("chain {chain}, step {step}");
            let out = four(&current, &tool, None, &tol, &mut samples, &mut tally, &name);
            let pick = match rng.next_u64() % 10 {
                0..=4 => 0,
                5..=8 => 2,
                _ => 1,
            };
            if let Some(next) = out[pick].clone().filter(|s| !s.is_empty()) {
                current = next;
            }
        }
    }
    tally.at_least(0.7, "chains");
}

#[test]
fn turned_and_moved_solids_are_right_or_refused() {
    // Cylinders, slots and plates turned and moved at random, against
    // each other and against boxes.
    let tol = Tolerance::DEFAULT;
    let mut tally = Tally::default();
    let mut rng = Rng::new(31);
    let mut samples = Rng::new(32);
    for case in 0..cases(40, 3) {
        let q = DQuat::from_axis_angle(rng.direction(), rng.range(0.0, 6.0));
        let shift = rng.point(2.0);
        let Some(a) = part(&mut rng, 0.25, 1, &tol) else {
            continue;
        };
        let a = moved(&a, &tol, |p| q * p + shift);
        let b = if rng.next_u64().is_multiple_of(2) {
            let size = DVec3::new(
                rng.range(0.5, 3.0),
                rng.range(0.5, 3.0),
                rng.range(0.5, 3.0),
            );
            Solid::cuboid(rng.point(1.5) - size / 2.0, size, 2, &tol).unwrap()
        } else {
            let r = rng.log_range(0.2, 1.5);
            let base = rng.point(1.0) - DVec3::Z * 2.0;
            let turn = DQuat::from_axis_angle(rng.direction(), rng.range(0.0, 6.0));
            let bar = Solid::cylinder(DVec3::ZERO, r, 4.0, 2, &tol).unwrap();
            moved(&bar, &tol, |p| turn * p + base)
        };
        four(
            &a,
            &b,
            None,
            &tol,
            &mut samples,
            &mut tally,
            &format!("case {case}"),
        );
    }
    tally.at_least(0.75, "turned");
}

#[test]
fn near_tangent_cylinders_are_right_or_refused() {
    // Upright cylinders side by side, a gap or an overlap of 1e-9 to 1e-3
    // between them (a tangency, a hair off it), at the coarsest and a
    // middle tolerance, the second shorter, turned about its axis or not.
    // A gap leaves the operands as they are.
    let mut tally = Tally::default();
    let mut samples = Rng::new(41);
    let gaps = [
        (Tolerance::MAX_FIT, 0.0),
        (Tolerance::MAX_FIT, 1e-9),
        (Tolerance::MAX_FIT, -1e-9),
        (Tolerance::MAX_FIT, 1e-6),
        (Tolerance::MAX_FIT, -1e-6),
        (Tolerance::MAX_FIT, 1e-3),
        (1e-2, 1e-9),
        (1e-2, 1e-3),
    ];
    for &(fit, gap) in &gaps[..cases(gaps.len(), 0)] {
        let tol = Tolerance::new(fit).unwrap();
        {
            let configs = [(0.5, 1.0, 0.0), (0.0, 2.0, 0.3), (0.5, 0.25, 0.0)];
            for &(z0, h, turn) in &configs[..cases(configs.len(), 1)] {
                let a = Solid::cylinder(DVec3::ZERO, 1.0, 2.0, 2, &tol).unwrap();
                let b = Solid::cylinder(DVec3::ZERO, 1.0, h, 3, &tol).unwrap();
                let q = DQuat::from_rotation_z(turn);
                let b = moved(&b, &tol, |p| q * p + DVec3::new(2.0 + gap, 0.0, z0));
                let both = if gap >= 0.0 { Some(0.0) } else { None };
                let name = format!("fit {fit}, gap {gap}, z0 {z0}, h {h}");
                four(&a, &b, both, &tol, &mut samples, &mut tally, &name);
            }
        }
    }
    // Their unions touch along a line: no manifold, so refused. The rest
    // must mostly work.
    tally.at_least(0.6, "tangent");
}

#[test]
fn coaxial_solids_and_pins_in_holes() {
    // Walls on one cylinder: a pin cut by the circle of a plate's hole,
    // over the plate's span, through it, and inside it; cylinders of one
    // radius stacked, over one span, and overlapping; cylinders of two
    // radii flush at one end.
    let tol = Tolerance::DEFAULT;
    let mut tally = Tally::default();
    let mut samples = Rng::new(51);
    let c = DVec2::new(0.5, 0.2);
    let plate = extruded(
        vec![
            rect(DVec2::new(-3.0, -2.0), DVec2::new(3.0, 2.0), 0),
            circle(c, 1.0, 4, true),
        ],
        &Frame::XY,
        0.0,
        1.0,
        5,
        &tol,
    )
    .unwrap();
    // Filling the hole over the plate's span comes last: its union meets
    // the plate's caps along the rim in their plane, which repair splits
    // down to flat pieces (right, and heavy).
    let pins = [
        (-1.0, 2.0),
        (0.5, 2.0),
        (0.25, 0.75),
        (1.0, 2.0),
        (0.0, 1.0),
    ];
    for &(f, t) in &pins[..cases(pins.len(), 1)] {
        let pin = extruded(vec![circle(c, 1.0, 0, false)], &Frame::XY, f, t, 7, &tol).unwrap();
        let name = format!("pin {f}..{t}");
        four(
            &plate,
            &pin,
            Some(0.0),
            &tol,
            &mut samples,
            &mut tally,
            &name,
        );
    }
    let a = extruded(
        vec![circle(c, 1.0, 0, false)],
        &Frame::XY,
        0.0,
        1.0,
        7,
        &tol,
    )
    .unwrap();
    let stacks = [
        (1.0, 1.0, 2.0),
        (0.5, 0.0, 2.0),
        (1.0, 0.0, 2.0),
        (1.0, 0.5, 2.0),
        (0.5, 1.0, 2.0),
    ];
    for &(r, f, t) in &stacks[..cases(stacks.len(), 0)] {
        let b = extruded(vec![circle(c, r, 0, false)], &Frame::XY, f, t, 8, &tol).unwrap();
        let both = PI * r * r * (t.min(1.0) - f.max(0.0)).max(0.0);
        let name = format!("coaxial r {r} {f}..{t}");
        four(&a, &b, Some(both), &tol, &mut samples, &mut tally, &name);
    }
    tally.at_least(0.8, "coaxial");
}

#[test]
fn flush_bosses_on_plates() {
    // A circle inside a rectangle (one near its corner), both extruded
    // over the same span or sharing one cap's plane: flush caps with
    // curved rims, on a sketch plane and on a frame whose axes are
    // swapped round.
    let tol = Tolerance::DEFAULT;
    let mut tally = Tally::default();
    let mut samples = Rng::new(61);
    let frames = [
        Frame::XY,
        Frame {
            origin: DVec3::new(0.5, 0.25, -0.25),
            x: DVec3::Z,
            y: DVec3::X,
        },
    ];
    for frame in &frames[..cases(2, 1)] {
        let plate = extruded(
            vec![rect(DVec2::splat(-1.25), DVec2::splat(1.25), 0)],
            frame,
            -0.25,
            0.25,
            1,
            &tol,
        )
        .unwrap();
        for (r, c) in [(0.5, DVec2::new(0.25, 0.0)), (0.3, DVec2::new(0.9, 0.5))] {
            for (f, t) in [(-0.25, 0.25), (-0.25, 0.75), (0.25, 0.75), (-1.0, 0.25)] {
                let boss = extruded(vec![circle(c, r, 0, false)], frame, f, t, 2, &tol).unwrap();
                let both = PI * r * r * (t.min(0.25) - f.max(-0.25)).max(0.0);
                let name = format!("boss r {r} at {c}, {f}..{t}");
                four(
                    &plate,
                    &boss,
                    Some(both),
                    &tol,
                    &mut samples,
                    &mut tally,
                    &name,
                );
            }
        }
    }
    tally.at_least(0.9, "bosses");
}

#[test]
fn boss_rim_tangent_to_cap_edges_between_holes() {
    // A boss standing on a plate whose rim passes between eight holes
    // placed symmetrically: the plate's cap has an edge along `x = 3`
    // between two holes' rims, tangent to the rim at the boss's own vertex
    // `(3, 0)`, which is a vertex of the cap too. The crossings the counting
    // gives that edge there were placed a micrometre apart in the wrong
    // order (`Inconsistent`), and once put in and out in turn left
    // triangles of zero width at the rim (`Invalid`).
    let tol = Tolerance::DEFAULT;
    let mut loops = vec![rect(DVec2::splat(-6.0), DVec2::splat(6.0), 0)];
    let holes = [(1.0, 3.0), (3.0, 1.0), (3.0, -1.0), (1.0, -3.0)];
    for (k, &(x, y)) in holes.iter().enumerate() {
        for (sx, sy) in [(1.0, 1.0), (-1.0, -1.0)] {
            let c = DVec2::new(sx * x, sy * y);
            loops.push(circle(c, 0.4, 10 + 2 * k as u64, true));
        }
    }
    let plate = extruded(loops, &Frame::XY, 0.0, 1.0, 1, &tol).unwrap();
    let boss = extruded(
        vec![circle(DVec2::ZERO, 3.0, 0, false)],
        &Frame::XY,
        1.0,
        2.0,
        2,
        &tol,
    )
    .unwrap();
    let joined = boolean(&plate, &boss, Op::Union, &tol, &Budget::DEFAULT).unwrap();
    let want = plate.volume() + boss.volume();
    assert!((joined.volume() - want).abs() < 1e-9, "{}", joined.volume());
}

#[test]
fn seeded_booleans_are_deterministic() {
    let tol = Tolerance::DEFAULT;
    let mut rng = Rng::new(71);
    for _ in 0..cases(4, 0) {
        let Some((a, b)) = related(&mut rng, true, &tol) else {
            continue;
        };
        for op in [Op::Union, Op::Difference] {
            let _ = assert_deterministic(|| {
                boolean(&a, &b, op, &tol, &Budget::DEFAULT).map(Solid::into_mesh)
            });
        }
    }
}

#[test]
fn plates_drilled_hole_after_hole() {
    // Holes drilled one after another into a plate, as a user places
    // them: of one size, in rows or anywhere on a grid. Each cuts the
    // long cap triangles the ones before left between the plate's corners
    // and their rims, at a glancing angle where rims share a tangent: 148
    // of these 160 operations worked before slivers on plane faces were
    // flipped towards Delaunay, all of them after.
    let tol = Tolerance::DEFAULT;
    let mut tally = Tally::default();
    let mut rng = Rng::new(81);
    let mut samples = Rng::new(82);
    for plate in 0..cases(4, 1) {
        let size = DVec3::new(
            (rng.range(4.0, 12.0) * 4.0).round() / 4.0,
            (rng.range(3.0, 8.0) * 4.0).round() / 4.0,
            1.0,
        );
        let mut current = Solid::cuboid(DVec3::ZERO, size, 1, &tol).unwrap();
        let r = [0.25, 0.4, 0.5][rng.next_u64() as usize % 3];
        let rows = rng.next_u64().is_multiple_of(2);
        for hole in 0..cases(10, 2) {
            let (x, y) = if rows {
                (1.0 + 1.5 * (hole % 4) as f64, 1.0 + 1.5 * (hole / 4) as f64)
            } else {
                (
                    (rng.range(0.75, size.x - 0.75) * 4.0).round() / 4.0,
                    (rng.range(0.75, size.y - 0.75) * 4.0).round() / 4.0,
                )
            };
            let drill =
                Solid::cylinder(DVec3::new(x, y, -1.0), r, 3.0, 10 + hole as u64, &tol).unwrap();
            let name = format!("plate {plate}, hole {hole}");
            let out = four(
                &current,
                &drill,
                None,
                &tol,
                &mut samples,
                &mut tally,
                &name,
            );
            if let Some(next) = out[2].clone() {
                current = next;
            }
        }
    }
    tally.at_least(0.95, "drilled");
}
