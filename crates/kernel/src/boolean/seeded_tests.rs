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

use glam::{DQuat, DVec2, DVec3, Vec3};

use super::curved_tests::moved_at as moved;
use super::*;
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
pub(super) fn rounded(min: DVec2, max: DVec2, r: f64, curve: u64) -> Loop {
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
        // Exactly 0 or 1: a result winding round twice (a shell facing
        // out inside another) or minus once (a shell facing in outside
        // every other) is wrong even where it lands on the right side of
        // a half.
        if (w - w.round()).abs() > 0.1 || w.round() != f64::from(u8::from(want)) {
            wrong += 1;
        }
    }
    wrong
}

/// `mesh`'s shells, the components of its triangles by halfedge pairs, in
/// the order of their lowest triangles, each as a mesh of its own with the
/// vertices and faces it uses. Worked out here, apart from `check`, for
/// [`shells_face_out`].
fn shell_meshes(mesh: &Mesh) -> Vec<Mesh> {
    let n = mesh.tris().len();
    let mut shell = vec![usize::MAX; n];
    let mut out = Vec::new();
    for first in 0..n {
        if shell[first] != usize::MAX {
            continue;
        }
        let id = out.len();
        shell[first] = id;
        let (mut stack, mut tris) = (vec![first], Vec::new());
        while let Some(t) = stack.pop() {
            tris.push(t);
            for h in mesh.tris()[t].halfedges {
                let u = h.pair as usize / 3;
                if shell[u] == usize::MAX {
                    shell[u] = id;
                    stack.push(u);
                }
            }
        }
        tris.sort_unstable();
        let mut builder = crate::mesh::MeshBuilder::new();
        let mut verts = vec![u32::MAX; mesh.verts().len()];
        let mut faces = vec![u32::MAX; mesh.faces().len()];
        for &t in &tris {
            let tri = mesh.tris()[t];
            let corners = tri.halfedges.map(|h| {
                let v = h.start as usize;
                if verts[v] == u32::MAX {
                    verts[v] = builder.vert(mesh.verts()[v]);
                }
                verts[v]
            });
            let f = tri.face as usize;
            if faces[f] == u32::MAX {
                faces[f] = builder.face(mesh.faces()[f]);
            }
            let patch = mesh.patch(t);
            for i in 0..3 {
                builder.edge(corners[i], corners[(i + 1) % 3], patch.c[i], patch.w[i]);
            }
            builder.tri(corners, faces[f]);
        }
        out.push(builder.build().unwrap());
    }
    out
}

/// Whether every shell of `mesh` faces the right way for where it lies,
/// by a floating-point oracle that shares nothing with `check`: each
/// shell's volume by quadrature, and the other shells' winding number at
/// one of its corners by the solid angles of their tessellations, which
/// must be 0 for a shell facing out and 1 for one facing in (a void).
/// The corner is the first of the shell's first few that is clear of the
/// other shells' tessellations by several chords; a shell with none is
/// passed over. A boolean that took a whole shell of an operand for
/// inside when it is outside, or the other way, keeps it turned the
/// wrong way or nested wrongly, and the volume identities can't see it.
fn shells_face_out(mesh: &Mesh, tol: &Tolerance) -> Result<(), String> {
    let shells = shell_meshes(mesh);
    if shells.len() < 2 {
        let volume = shells.first().map_or(1.0, shell_volume);
        return if volume > 0.0 {
            Ok(())
        } else {
            Err(format!("the only shell has volume {volume}"))
        };
    }
    let display = Display::new(tol);
    let tessellated: Vec<(Vec<[Vec3; 3]>, f64)> = shells
        .iter()
        .map(|m| {
            let drawn = crate::tessellate::tessellate(m, &display).unwrap();
            let pos = drawn.positions();
            let tris = drawn
                .indices()
                .chunks(3)
                .map(|c| [0, 1, 2].map(|k| Vec3::from(pos[c[k] as usize])))
                .collect();
            let bounds = crate::patch::Bounds3::around(m.verts()).unwrap();
            let bounds = m.edges().iter().fold(bounds, |b, e| b.include(e.ctrl));
            (tris, display.chord((bounds.max - bounds.min).length()))
        })
        .collect();
    let clear = 8.0 * tessellated.iter().map(|&(_, c)| c).fold(0.0, f64::max);
    for (i, m) in shells.iter().enumerate() {
        let volume = shell_volume(m);
        let others = || (0..shells.len()).filter(move |&j| j != i);
        let away = |p: DVec3| {
            others().all(|j| {
                tessellated[j]
                    .0
                    .iter()
                    .all(|t| to_triangle(p, t.map(|v| v.as_dvec3())) > clear)
            })
        };
        let Some(&p) = m.verts().iter().take(16).find(|&&p| away(p)) else {
            continue;
        };
        let w: f64 = others()
            .map(|j| winding(&tessellated[j].0, p.as_vec3()))
            .sum();
        let want = if volume > 0.0 { 0.0 } else { 1.0 };
        if (w - w.round()).abs() > 0.1 || w.round() != want {
            return Err(format!(
                "shell {i} of {}: volume {volume}, the others wind {w} round {p}",
                shells.len()
            ));
        }
    }
    Ok(())
}

/// The volume a shell's mesh encloses, by quadrature.
fn shell_volume(mesh: &Mesh) -> f64 {
    let o = mesh.verts()[0];
    (0..mesh.tris().len())
        .map(|t| crate::solid::patch_volume(&mesh.patch(t), o).0)
        .sum()
}

#[test]
fn the_shell_oracle_sees_wrong_shells() {
    // A 10 mm cube with a second shell: turned in far off, facing out
    // inside it (reached by a boolean's other operand or not), turned in
    // outside it where another operand would cover it: all wrong. A void
    // (turned in inside it) and a second body beside it are right.
    let tol = Tolerance::DEFAULT;
    let cube =
        |min: f64, size: f64| Mesh::cuboid(DVec3::splat(min), DVec3::splat(size), 1, &tol).unwrap();
    let big = cube(0.0, 10.0);
    let far = Mesh::cuboid(DVec3::new(50.0, 0.0, 0.0), DVec3::splat(2.0), 2, &tol).unwrap();
    let far_cylinder = Mesh::cylinder(DVec3::new(50.0, 0.0, 0.0), 2.0, 3.0, 2, &tol).unwrap();
    let wrong = [
        (&far, true),
        (&far_cylinder, true),
        (&cube(1.0, 2.0), false),
        (&cube(6.0, 2.0), false),
        (&cube(12.0, 2.0), true),
    ];
    for (i, &(shell, turned)) in wrong.iter().enumerate() {
        let mesh = crate::mesh::tests::joined(&[(&big, false), (shell, turned)]);
        assert!(shells_face_out(&mesh, &tol).is_err(), "case {i}");
    }
    let turned = crate::mesh::tests::joined(&[(&big, true)]);
    assert!(shells_face_out(&turned, &tol).is_err());
    for (i, (shell, turned)) in [
        (&cube(1.0, 2.0), true),
        (&far, false),
        (&far_cylinder, false),
    ]
    .into_iter()
    .enumerate()
    {
        let mesh = crate::mesh::tests::joined(&[(&big, false), (shell, turned)]);
        assert_eq!(shells_face_out(&mesh, &tol), Ok(()), "case {i}");
    }
    assert_eq!(shells_face_out(&big, &tol), Ok(()));
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
        println!("TALLY {name}: {} of {total}", self.ok);
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
                if let Err(why) = shells_face_out(solid.mesh(), tol) {
                    panic!("{name}, {op:?}: {why}");
                }
                tally.ok += 1;
                Some(solid)
            }
            Err(why) => {
                println!("REFUSED {name}, {op:?}: {why:?}");
                // Decisions that don't fit together show where.
                if why.error == KernelError::Boolean(BooleanError::Inconsistent) {
                    super::evidence::tests::on_operands(x, y, &why, tol);
                }
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
    // and now and then intersected, each result fed on. A debug build
    // takes about 1 s, nearly all of it the first step, whose four
    // operations are all refused after trying with ties and exactly;
    // the later steps take milliseconds, so fewer wouldn't help.
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

/// A rigid placement of the seams suite's solids: `p` to `q·p + shift`.
#[derive(Clone, Copy)]
struct Placement {
    q: DQuat,
    shift: DVec3,
}

impl Placement {
    /// The frame whose normal is axis `k`: its `x` along axis `k + 1`,
    /// its `y` along `k + 2`.
    fn frame(&self, k: usize) -> Frame {
        let axes = [DVec3::X, DVec3::Y, DVec3::Z];
        Frame {
            origin: self.shift,
            x: (self.q * axes[(k + 1) % 3]).normalize(),
            y: (self.q * axes[(k + 2) % 3]).normalize(),
        }
    }

    /// Where `p` is in the placement's own frame.
    fn local(&self, p: DVec3) -> DVec3 {
        self.q.inverse() * (p - self.shift)
    }
}

/// A surface of the seams suite's solids, in their own frame: the plane
/// where coordinate `k` is the value, or the cylinder along axis `k` of
/// the radius round the point in axes `k + 1` and `k + 2`.
#[derive(Clone, Copy)]
enum Wall {
    Plane(usize, f64),
    Cylinder(usize, DVec2, f64),
}

impl Wall {
    fn distance(self, p: DVec3) -> f64 {
        match self {
            Wall::Plane(k, v) => (p[k] - v).abs(),
            Wall::Cylinder(k, c, r) => {
                { (DVec2::new(p[(k + 1) % 3], p[(k + 2) % 3]) - c).length() - r }.abs()
            }
        }
    }
}

/// A value on the grid of `step` from `lo` to `hi`.
fn on_grid(rng: &mut Rng, lo: f64, hi: f64, step: f64) -> f64 {
    let n = ((hi - lo) / step).floor();
    lo + step * (rng.unit() * (n + 1.0)).floor().min(n)
}

/// The height of the second cylinder's axis (radius `r`) where seams of
/// the two meet on the cut, the first's axis (radius `big`) at `z`, by
/// `kind`: the second's bottom or top seam on the first's side seam, the
/// side seams level, the second's side seams on the first's top or
/// bottom seam.
fn meeting(kind: usize, z: f64, big: f64, r: f64) -> f64 {
    match kind % 5 {
        0 => z + r,
        1 => z - r,
        2 => z,
        3 => z + big,
        _ => z - big,
    }
}

/// A perpendicular pair whose seams meet on the cut, `place`d: crossing
/// cylinders, or (`holes`) a box less a hole along `y` and a hole along
/// `x`. Their surfaces in the placement's frame and `|a ∩ b|`, if the
/// operands can be built.
fn seams_meeting(
    rng: &mut Rng,
    kind: usize,
    holes: bool,
    place: &Placement,
    tol: &Tolerance,
) -> Option<(Solid, Solid, Vec<Wall>, f64)> {
    let cylinder = |c: DVec2, r: f64, k: usize, from: f64, to: f64, feature: u64| {
        extruded(
            vec![circle(c, r, 0, false)],
            &place.frame(k),
            from,
            to,
            feature,
            tol,
        )
    };
    if !holes {
        let big = on_grid(rng, 0.5, 1.25, 0.125);
        let r = on_grid(rng, 0.125, big * 0.9, 0.0625);
        let x0 = on_grid(rng, -1.0, 2.0, 0.25);
        let z0 = on_grid(rng, -1.0, 2.0, 0.25);
        let y0 = on_grid(rng, -1.0, 3.0, 0.25);
        let zb = meeting(kind, z0, big, r);
        let ly = r + on_grid(rng, 0.25, 1.0, 0.25);
        let lx = big + on_grid(rng, 0.25, 1.0, 0.25);
        let (ac, bc) = (DVec2::new(z0, x0), DVec2::new(y0, zb));
        let a = cylinder(ac, big, 1, y0 - ly, y0 + ly, 1)?;
        let b = cylinder(bc, r, 0, x0 - lx, x0 + lx, 2)?;
        let walls = vec![
            Wall::Cylinder(1, ac, big),
            Wall::Plane(1, y0 - ly),
            Wall::Plane(1, y0 + ly),
            Wall::Cylinder(0, bc, r),
            Wall::Plane(0, x0 - lx),
            Wall::Plane(0, x0 + lx),
        ];
        let both = super::curved_tests::crossing_volume(big, z0, r, zb);
        return Some((a, b, walls, both));
    }
    // Both holes inside the box's height, with room.
    let (r1, r2, x1, y2, z1, z2) = loop {
        let r1 = on_grid(rng, 0.375, 0.75, 0.0625);
        let r2 = on_grid(rng, 0.125, (r1 * 0.95).min(0.6), 0.0625);
        let x1 = on_grid(rng, -0.125, 0.125, 0.0625);
        let y2 = on_grid(rng, -0.125, 0.125, 0.0625);
        let z1 = on_grid(rng, 0.875, 1.125, 0.0625);
        let z2 = meeting(kind, z1, r1, r2);
        if z2 + r2 <= 1.875 && z2 - r2 >= 0.125 && z1 + r1 <= 1.875 && z1 - r1 >= 0.125 {
            break (r1, r2, x1, y2, z1, z2);
        }
    };
    let block = extruded(
        vec![rect(DVec2::splat(-1.0), DVec2::splat(1.0), 0)],
        &place.frame(2),
        0.0,
        2.0,
        1,
        tol,
    )?;
    let (c1, c2) = (DVec2::new(z1, x1), DVec2::new(y2, z2));
    let first = cylinder(c1, r1, 1, -1.5, 1.5, 2)?;
    let a = boolean(&block, &first, Op::Difference, tol, &Budget::DEFAULT).ok()?;
    let b = cylinder(c2, r2, 0, -1.5, 1.5, 3)?;
    let mut walls: Vec<Wall> = (0..3)
        .flat_map(|k| {
            [
                Wall::Plane(k, if k == 2 { 0.0 } else { -1.0 }),
                Wall::Plane(k, if k == 2 { 2.0 } else { 1.0 }),
            ]
        })
        .collect();
    walls.extend([
        Wall::Cylinder(1, c1, r1),
        Wall::Cylinder(0, c2, r2),
        Wall::Plane(0, -1.5),
        Wall::Plane(0, 1.5),
    ]);
    // The second hole within the box, less where it crosses the first.
    let both = 2.0 * PI * r2 * r2 - super::curved_tests::crossing_volume(r1, z1, r2, z2);
    Some((a, b, walls, both))
}

#[test]
fn seams_on_the_cut() {
    // Perpendicular cylinders, and cross holes through a box, drawn from
    // circles on their frames' axis points as a sketch draws them, so
    // each wall has seam rulings at its arcs' joins, placed so that seams
    // of the two meet on the cut (see `meeting`), on the world frame and
    // turned and moved: on round numbers refinement puts a vertex of
    // both operands there. Every first order of a tie there is zero
    // exactly, and every side of the patches the cut starts from is
    // tangent to it. Every result is right (by `four`, against its
    // analytic volume within the fit times the operands' area over 5),
    // every patch sampled within the fit of the solids' surfaces; most
    // work.
    let tol = Tolerance::DEFAULT;
    let mut tally = Tally::default();
    let mut rng = Rng::new(39);
    let mut samples = Rng::new(40);
    for case in 0..cases(40, 2) {
        let holes = case % 4 >= 2;
        let place = if case % 2 == 0 {
            Placement {
                q: DQuat::IDENTITY,
                shift: DVec3::ZERO,
            }
        } else {
            Placement {
                q: DQuat::from_axis_angle(rng.direction(), rng.range(0.3, 6.0)),
                shift: rng.point(10.0),
            }
        };
        let kind = case / 4;
        let Some((a, b, walls, both)) = seams_meeting(&mut rng, kind, holes, &place, &tol) else {
            println!("SKIPPED case {case}: the operands");
            continue;
        };
        let name = format!("case {case}");
        let results = four(&a, &b, Some(both), &tol, &mut samples, &mut tally, &name);
        for (k, solid) in results.iter().enumerate() {
            let Some(solid) = solid else { continue };
            let mesh = solid.mesh();
            for t in 0..mesh.tris().len() {
                let patch = mesh.patch(t);
                for u in crate::mesh::samples() {
                    let p = place.local(patch.eval(u));
                    let off = walls
                        .iter()
                        .map(|w| w.distance(p))
                        .fold(f64::INFINITY, f64::min);
                    assert!(off <= tol.fit(), "{name}, result {k}: {p} {off:e} off");
                }
            }
        }
    }
    tally.at_least(0.9, "seams on the cut");
}

/// Whether the tangent test lets operation `k` of [`four`] (union,
/// intersection, `a − b`, `b − a`) fail at a `gap` between the cylinders,
/// at resolution `res`. Everything else must work.
///
/// Unions with `|gap| < res`: the cylinders touch along a line, or leave
/// a neck or a gap narrower than the resolution, which no manifold at the
/// kernel's resolution can hold. They may fail; that they do isn't
/// asserted, so non-manifold results or better ties don't break the
/// test. (Intersections and differences at `-res < gap < 0` with both
/// seams on the tangent line failed as `Inconsistent` while the count's
/// heights were ties along `UP`, a few times the normal distance on a
/// wall near parallel to it: the walls lay on one surface within the
/// resolution and the pair still had ends. Measured in space, they work.)
fn tangent_may_fail(k: usize, gap: f64, res: f64) -> bool {
    k == 0 && gap.abs() < res
}

#[test]
fn near_tangent_cylinders_are_right_or_refused() {
    // Upright cylinders side by side, a gap or an overlap of 1e-9 to 1e-3
    // between them (a tangency, a hair off it), at the coarsest and a
    // middle tolerance, the second shorter, turned about its axis or not.
    // A gap leaves the operands as they are. Each operation must work
    // unless `tangent_may_fail` lets it fail.
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
    let ops = ["union", "intersection", "a less b", "b less a"];
    let mut lost = Vec::new();
    for &(fit, gap) in &gaps[..cases(gaps.len(), 0)] {
        let tol = Tolerance::new(fit).unwrap();
        let configs = [(0.5, 1.0, 0.0), (0.0, 2.0, 0.3), (0.5, 0.25, 0.0)];
        for &(z0, h, turn) in &configs[..cases(configs.len(), 1)] {
            let a = Solid::cylinder(DVec3::ZERO, 1.0, 2.0, 2, &tol).unwrap();
            let b = Solid::cylinder(DVec3::ZERO, 1.0, h, 3, &tol).unwrap();
            let q = DQuat::from_rotation_z(turn);
            let b = moved(&b, &tol, |p| q * p + DVec3::new(2.0 + gap, 0.0, z0));
            let both = if gap >= 0.0 { Some(0.0) } else { None };
            let name = format!("fit {fit}, gap {gap}, z0 {z0}, h {h}, turn {turn}");
            let out = four(&a, &b, both, &tol, &mut samples, &mut tally, &name);
            for (k, result) in out.iter().enumerate() {
                if result.is_none() && !tangent_may_fail(k, gap, tol.resolution()) {
                    lost.push(format!("{name}: {}", ops[k]));
                }
            }
        }
    }
    println!("TALLY tangent: {} of {}", tally.ok, tally.ok + tally.failed);
    assert!(lost.is_empty(), "failed but must work: {lost:#?}");
}

#[test]
#[ignore = "a probe: the work tangent walls cost at the default tolerance"]
fn near_tangent_cylinders_at_the_default_tolerance() {
    // As `near_tangent_cylinders_are_right_or_refused`, at the default
    // tolerance (a resolution of 1e-6), gaps and overlaps from 1e-12 to
    // 1e-4: what each operation costs, and that what works is right.
    let tol = Tolerance::DEFAULT;
    let mut samples = Rng::new(43);
    let (mut worked, mut over, mut total) = (0, Vec::new(), 0u64);
    let gaps = [
        0.0, 1e-12, -1e-12, 1e-9, -1e-9, 1e-7, -1e-7, 1e-5, -1e-5, 1e-4, -1e-4,
    ];
    let ops = ["union", "intersection", "a less b", "b less a"];
    for gap in gaps {
        for (z0, h, turn) in [(0.5, 1.0, 0.0), (0.0, 2.0, 0.3)] {
            let a = Solid::cylinder(DVec3::ZERO, 1.0, 2.0, 2, &tol).unwrap();
            let b = Solid::cylinder(DVec3::ZERO, 1.0, h, 3, &tol).unwrap();
            let q = DQuat::from_rotation_z(turn);
            let b = moved(&b, &tol, |p| q * p + DVec3::new(2.0 + gap, 0.0, z0));
            // The lens the unit circles share, centres `2 + gap` apart.
            let d = 2.0 + gap;
            let lens = if d < 2.0 {
                2.0 * (d / 2.0).acos() - d / 2.0 * (4.0 - d * d).sqrt()
            } else {
                0.0
            };
            let both = lens * (2.0f64.min(z0 + h) - z0.max(0.0));
            let (va, vb) = (a.volume(), b.volume());
            let want = [va + vb - both, both, va - both, vb - both];
            let within = tol.fit() * (a.area() + b.area()) / 5.0 + 1e-9;
            let jobs = [
                (&a, &b, Op::Union),
                (&a, &b, Op::Intersection),
                (&a, &b, Op::Difference),
                (&b, &a, Op::Difference),
            ];
            for (k, (x, y, op)) in jobs.into_iter().enumerate() {
                let start = std::time::Instant::now();
                let mut work = Work::new(&Budget::DEFAULT);
                let r = boolean_within(x, y, op, &tol, &mut work);
                let spent = Budget::DEFAULT.work() - work.left();
                total += spent;
                let name = format!("gap {gap:e}, z0 {z0}, turn {turn}: {}", ops[k]);
                if let Ok(solid) = &r {
                    let wrong = wrong_points(x, y, op, solid, &tol, &mut samples);
                    assert_eq!(wrong, 0, "{name}: {wrong} points on the wrong side");
                    if let Err(why) = shells_face_out(solid.mesh(), &tol) {
                        panic!("{name}: {why}");
                    }
                    let v = solid.volume();
                    assert!((v - want[k]).abs() <= within, "{name}: {v} vs {}", want[k]);
                    worked += 1;
                }
                if spent > 1_000_000 {
                    over.push(format!("{name}: {:?}", r.as_ref().map(|_| ())));
                }
                println!(
                    "NEAR {name}: {:?}, {spent} units, {:.2?}",
                    r.map(|s| s.volume()),
                    start.elapsed()
                );
            }
        }
    }
    println!(
        "NEAR worked {worked} of {}, {} over a million units, {total} units",
        gaps.len() * 8,
        over.len()
    );
    println!("NEAR over a million: {over:#?}");
}

/// The frames the coaxial cases are built on: the three sketch planes
/// (on XZ and YZ a cap's plane lies nearly along `UP`, and a rim's shadow
/// is a thin ellipse), a frame turned off every axis and moved, and one
/// whose caps are upright off the axes, again nearly along `UP`.
fn coaxial_frames() -> [(&'static str, Frame); 5] {
    let turned = DQuat::from_axis_angle(DVec3::new(1.0, 2.0, 3.0).normalize(), 1.1);
    [
        ("XY", Frame::XY),
        (
            "XZ",
            Frame {
                origin: DVec3::ZERO,
                x: DVec3::X,
                y: DVec3::Z,
            },
        ),
        (
            "YZ",
            Frame {
                origin: DVec3::ZERO,
                x: DVec3::Y,
                y: DVec3::Z,
            },
        ),
        (
            "tilted",
            Frame {
                origin: DVec3::new(2.5, -1.25, 7.0),
                x: turned * DVec3::X,
                y: turned * DVec3::Y,
            },
        ),
        (
            "upright",
            Frame {
                origin: DVec3::new(0.3, -0.7, 0.2),
                x: DVec3::new(0.6, 0.8, 0.0),
                y: DVec3::Z,
            },
        ),
    ]
}

#[test]
fn coaxial_solids_and_pins_in_holes() {
    // Walls on one cylinder: a pin cut by the circle of a plate's hole,
    // over the plate's span, through it, and inside it; cylinders of one
    // radius stacked, over one span, and overlapping; cylinders of two
    // radii flush at one end. On each of the frames above.
    let tol = Tolerance::DEFAULT;
    let mut tally = Tally::default();
    let mut samples = Rng::new(51);
    let c = DVec2::new(0.5, 0.2);
    let frames = coaxial_frames();
    for (plane, frame) in &frames[..cases(frames.len(), 1)] {
        let plate = extruded(
            vec![
                rect(DVec2::new(-3.0, -2.0), DVec2::new(3.0, 2.0), 0),
                circle(c, 1.0, 4, true),
            ],
            frame,
            0.0,
            1.0,
            5,
            &tol,
        )
        .unwrap();
        // Filling the hole over the plate's span comes last: its union
        // meets the plate's caps along the rim in their plane, which
        // repair splits down to flat pieces (right, and heavy).
        let pins = [
            (-1.0, 2.0),
            (0.5, 2.0),
            (0.25, 0.75),
            (1.0, 2.0),
            (0.0, 1.0),
        ];
        for &(f, t) in &pins[..cases(pins.len(), 1)] {
            let pin = extruded(vec![circle(c, 1.0, 0, false)], frame, f, t, 7, &tol).unwrap();
            let name = format!("{plane} pin {f}..{t}");
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
        let a = extruded(vec![circle(c, 1.0, 0, false)], frame, 0.0, 1.0, 7, &tol).unwrap();
        let stacks = [
            (1.0, 1.0, 2.0),
            (0.5, 0.0, 2.0),
            (1.0, 0.0, 2.0),
            (1.0, 0.5, 2.0),
            (0.5, 1.0, 2.0),
        ];
        for &(r, f, t) in &stacks[..cases(stacks.len(), 0)] {
            let b = extruded(vec![circle(c, r, 0, false)], frame, f, t, 8, &tol).unwrap();
            let both = PI * r * r * (t.min(1.0) - f.max(0.0)).max(0.0);
            let name = format!("{plane} coaxial r {r} {f}..{t}");
            four(&a, &b, Some(both), &tol, &mut samples, &mut tally, &name);
        }
    }
    // 195 of 200 work. Left: `pin 1..2`'s union on every frame, the pin
    // standing on the plate touching it only along the hole's rim (no
    // manifold, refused).
    tally.at_least(0.97, "coaxial");
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
                let out = four(
                    &plate,
                    &boss,
                    Some(both),
                    &tol,
                    &mut samples,
                    &mut tally,
                    &name,
                );
                light(&plate, &boss, &out[0], &mut tally, &name);
                // The boss first, as a join onto a body that is the boss.
                let want = plate.volume() + boss.volume() - both;
                let first =
                    union_checked(&boss, &plate, want, &tol, &mut samples, &mut tally, &name);
                light(&boss, &plate, &first, &mut tally, &name);
            }
        }
    }
    tally.at_least(0.9, "bosses");
}

/// The patches a union may have, for operands of `inputs` patches all
/// told, before it counts as failed: flush caps meeting along curves
/// once left seams that repair split down to flat pieces, 1 800 to tens
/// of thousands of patches. The heaviest union that works is under 7
/// times its operands.
fn heavy(inputs: usize) -> usize {
    10 * inputs
}

/// Counts `union` of `a` and `b`, if it worked, as failed when it is
/// heavier than [`heavy`] allows.
fn light(a: &Solid, b: &Solid, union: &Option<Solid>, tally: &mut Tally, name: &str) {
    let inputs = a.mesh().tris().len() + b.mesh().tris().len();
    if let Some(union) = union
        && union.mesh().tris().len() > heavy(inputs)
    {
        println!("HEAVY {name}: {} patches", union.mesh().tris().len());
        tally.ok -= 1;
        tally.failed += 1;
    }
}

/// `a ∪ b`, right (of volume `want` within the fit tolerance, its points
/// on the right sides, its shells facing out) or failed.
fn union_checked(
    a: &Solid,
    b: &Solid,
    want: f64,
    tol: &Tolerance,
    rng: &mut Rng,
    tally: &mut Tally,
    name: &str,
) -> Option<Solid> {
    match boolean(a, b, Op::Union, tol, &Budget::DEFAULT) {
        Ok(solid) => {
            let wrong = wrong_points(a, b, Op::Union, &solid, tol, rng);
            assert_eq!(
                wrong, 0,
                "{name}, the other way: {wrong} points on the wrong side"
            );
            if let Err(why) = shells_face_out(solid.mesh(), tol) {
                panic!("{name}, the other way: {why}");
            }
            let within = tol.fit() * (a.area() + b.area()) / 5.0 + 1e-9;
            let got = solid.volume();
            assert!(
                (got - want).abs() <= within,
                "{name}, the other way: {got} vs {want}"
            );
            tally.ok += 1;
            Some(solid)
        }
        Err(why) => {
            println!("REFUSED {name}, the other way: {why:?}");
            tally.failed += 1;
            None
        }
    }
}

/// A random closed outline with curved sides round `c`, of about size
/// `k`: a circle, a rounded square or a slot, its numbers on a grid of
/// `q`.
fn curved_outline(rng: &mut Rng, c: DVec2, k: f64, q: f64, curve: u64) -> Loop {
    let s = |x: f64| ((x / q).round() * q).max(q);
    match rng.next_u64() % 3 {
        0 => circle(c, s(k * rng.range(0.3, 1.3)), curve, false),
        1 => {
            let r = s(k * rng.range(0.2, 0.7));
            let w = s(k * rng.range(0.5, 2.0)) + r;
            rounded(c - DVec2::splat(w), c + DVec2::splat(w), r, curve)
        }
        _ => slot(
            c,
            s(k * rng.range(0.2, 1.2)),
            s(k * rng.range(0.3, 0.8)),
            curve,
        ),
    }
}

/// Two solids from one sketch plane whose caps are flush with curved
/// rims: a plate (drilled or not) or a curved outline, and a curved
/// outline over the same span, sharing only its bottom's plane, standing
/// through it or flush with its top; at millimetre or unit scale, on a
/// sketch plane or a turned frame.
fn flush_pair(rng: &mut Rng, tol: &Tolerance) -> Option<(Solid, Solid, String)> {
    let mm = rng.next_u64().is_multiple_of(2);
    let k = if mm { 10.0 } else { 0.1 };
    let q = k / 20.0;
    let s = |x: f64| (x / q).round() * q;
    let frame = match rng.next_u64() % 3 {
        0 => Frame::XY,
        1 => Frame {
            origin: DVec3::new(0.5, 0.25, -0.25) * k,
            x: DVec3::Z,
            y: DVec3::X,
        },
        _ => {
            let turn = DQuat::from_rotation_x(rng.range(-1.0, 1.0))
                * DQuat::from_rotation_y(rng.range(-1.0, 1.0));
            Frame {
                origin: rng.point(k),
                x: turn * DVec3::X,
                y: turn * DVec3::Y,
            }
        }
    };
    let h = if mm { 10.0 } else { 0.5 };
    let at = |rng: &mut Rng| DVec2::new(s(rng.range(-3.0, 3.0) * k), s(rng.range(-3.0, 3.0) * k));
    let w = 3.0 * k;
    let (kind, first) = match rng.next_u64() % 3 {
        0 => ("plate", vec![rect(DVec2::splat(-w), DVec2::splat(w), 0)]),
        1 => {
            let c = DVec2::new(s(rng.range(-1.0, 1.0) * k), s(rng.range(-1.0, 1.0) * k));
            let r = s(rng.range(0.3, 0.8) * k).max(q);
            let hole = circle(c, r, 10, true);
            (
                "drilled",
                vec![rect(DVec2::splat(-w), DVec2::splat(w), 0), hole],
            )
        }
        _ => {
            let c = at(rng);
            ("outlines", vec![curved_outline(rng, c, k, q, 0)])
        }
    };
    let c = at(rng);
    let second = curved_outline(rng, c, k, q, 20);
    let (from, to) = match rng.next_u64() % 4 {
        0 => (0.0, h),
        1 => (0.0, 2.0 * h),
        2 => (-h, h),
        _ => (0.5 * h, h),
    };
    let a = extruded(first, &frame, 0.0, h, 1, tol)?;
    let b = extruded(vec![second], &frame, from, to, 2, tol)?;
    let scale = if mm { "mm" } else { "unit" };
    Some((a, b, format!("{kind} {scale} {from}..{to}")))
}

#[test]
fn flush_unions_either_order() {
    // Flush caps with curved rims, from one sketch plane: the first
    // operand's cap kept whole, the other's cut along its rim, left a
    // curve between two patches in one plane, which repair split down to
    // flat pieces (1 800 to 81 000 patches here), whichever went first.
    // All four operations both ways round; a union heavier than `heavy`
    // counts as failed. Before the seams were mended 189 of 240 worked,
    // 38 of the 60 unions; now 210, and 59 of the unions. What fails is
    // intersections and differences that keep a cap corner where a
    // straight side runs on into an arc at a tangent (a degenerate
    // corner, `Invalid(Fold)`), and one union whose cap piece has its
    // corner inside the other's rim (`Invalid(VertexNeighbours)`).
    let tol = Tolerance::DEFAULT;
    let mut tally = Tally::default();
    let mut unions = Tally::default();
    let mut rng = Rng::new(93);
    let mut samples = Rng::new(92);
    for case in 0..cases(30, 2) {
        let Some((a, b, what)) = flush_pair(&mut rng, &tol) else {
            continue;
        };
        for (x, y, way) in [(&a, &b, "a, b"), (&b, &a, "b, a")] {
            let name = format!("case {case}, {what}, {way}");
            let out = four(x, y, None, &tol, &mut samples, &mut tally, &name);
            let before = tally.failed;
            light(x, y, &out[0], &mut tally, &name);
            if out[0].is_some() && tally.failed == before {
                unions.ok += 1;
            } else {
                unions.failed += 1;
            }
        }
    }
    unions.at_least(0.95, "flush unions");
    tally.at_least(0.86, "flush operations");
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

#[test]
fn flush_pairs_keep_their_names_either_order() {
    // Whichever operand's flush cap the perturbation keeps, a union or an
    // intersection of a flush pair names the same keys (as faces or
    // aliases) either way round, so a reference doesn't depend on which
    // body came first. 5 of these 35 pairs named different keys when only
    // a dropped cap's first triangle was looked for on the result.
    use std::collections::BTreeSet;

    use crate::mesh::FaceKey;
    let tol = Tolerance::DEFAULT;
    let named = |solid: &Solid| -> BTreeSet<FaceKey> {
        let topology = solid.topology();
        topology
            .regions()
            .iter()
            .flat_map(|r| std::iter::once(r.key).chain(r.aliases.iter().copied()))
            .collect()
    };
    let mut rng = Rng::new(93);
    let mut compared = 0;
    for case in 0..cases(20, 2) {
        let Some((a, b, what)) = flush_pair(&mut rng, &tol) else {
            continue;
        };
        for op in [Op::Union, Op::Intersection] {
            let ab = boolean(&a, &b, op, &tol, &Budget::DEFAULT);
            let ba = boolean(&b, &a, op, &tol, &Budget::DEFAULT);
            if let (Ok(ab), Ok(ba)) = (ab, ba) {
                assert_eq!(named(&ab), named(&ba), "case {case}, {what}, {op:?}");
                compared += 1;
            }
        }
    }
    assert!(compared >= cases(30, 2), "{compared}");
}

/// A circle's upper (`+1`) or lower (`−1`) half as a function of `x`, or
/// a constant: the sides of the regions [`disc_less_discs`] integrates.
#[derive(Clone, Copy)]
enum Side2 {
    Arc(DVec2, f64, f64),
    Level(f64),
}

impl Side2 {
    fn at(self, x: f64) -> f64 {
        match self {
            Side2::Arc(c, r, s) => c.y + s * (r * r - (x - c.x).powi(2)).max(0.0).sqrt(),
            Side2::Level(y) => y,
        }
    }

    /// Its integral from `a` to `b`, in closed form.
    fn integral(self, a: f64, b: f64) -> f64 {
        match self {
            Side2::Level(y) => y * (b - a),
            Side2::Arc(c, r, s) => {
                let f = |x: f64| {
                    let u = ((x - c.x) / r).clamp(-1.0, 1.0);
                    0.5 * r * r * (u * (1.0 - u * u).max(0.0).sqrt() + u.asin())
                };
                c.y * (b - a) + s * (f(b) - f(a))
            }
        }
    }
}

/// The area of the disc round `c` of radius `r` within `|y| ≤ half`,
/// less the discs `holes`, in closed form: between the `x` where any two
/// of the circles and lines meet or a circle turns, the region is a fixed
/// set of strips between arcs and levels, each integrated exactly.
fn disc_less_discs(c: DVec2, r: f64, half: f64, holes: &[(DVec2, f64)]) -> f64 {
    let circles: Vec<(DVec2, f64)> = std::iter::once((c, r))
        .chain(holes.iter().copied())
        .collect();
    let mut xs = Vec::new();
    for (i, &(p, a)) in circles.iter().enumerate() {
        xs.extend([p.x - a, p.x + a]);
        for y in [-half, half] {
            let w = a * a - (y - p.y).powi(2);
            if w >= 0.0 {
                xs.extend([p.x - w.sqrt(), p.x + w.sqrt()]);
            }
        }
        for &(q, b) in &circles[i + 1..] {
            let d = p.distance(q);
            if d > 0.0 && d <= a + b && d >= (a - b).abs() {
                let l = (a * a - b * b + d * d) / (2.0 * d);
                let h = (a * a - l * l).max(0.0).sqrt();
                let e = (q - p) / d;
                let m = p + e * l;
                xs.extend([m.x - e.y * h, m.x + e.y * h]);
            }
        }
    }
    let mut xs: Vec<f64> = xs
        .into_iter()
        .filter(|x| (c.x - r..=c.x + r).contains(x))
        .collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup();
    let mut total = 0.0;
    for w in xs.windows(2) {
        let (a, b) = (w[0], w[1]);
        let m = 0.5 * (a + b);
        let (low, high) = (Side2::Arc(c, r, -1.0), Side2::Arc(c, r, 1.0));
        let low = if low.at(m) > -half {
            low
        } else {
            Side2::Level(-half)
        };
        let high = if high.at(m) < half {
            high
        } else {
            Side2::Level(half)
        };
        let mut strips = vec![(low, high)];
        for &(p, s) in holes {
            if (m - p.x).abs() >= s {
                continue;
            }
            let (hl, hh) = (Side2::Arc(p, s, -1.0), Side2::Arc(p, s, 1.0));
            strips = strips
                .into_iter()
                .flat_map(|(l, u)| {
                    if hh.at(m) <= l.at(m) || hl.at(m) >= u.at(m) {
                        return vec![(l, u)];
                    }
                    let mut kept = Vec::new();
                    if hl.at(m) > l.at(m) {
                        kept.push((l, hl));
                    }
                    if hh.at(m) < u.at(m) {
                        kept.push((hh, u));
                    }
                    kept
                })
                .collect();
        }
        for (l, u) in strips {
            if u.at(m) > l.at(m) {
                total += u.integral(a, b) - l.integral(a, b);
            }
        }
    }
    total
}

#[test]
fn the_closed_form_areas_are_right() {
    let c = DVec2::new(0.1, 0.2);
    assert!((disc_less_discs(c, 0.7, 2.0, &[]) - PI * 0.49).abs() < 1e-12);
    // A disc through the line y = 2: the segment past it cut off.
    let segment = (0.5f64).acos() - 0.5 * 0.75f64.sqrt();
    let got = disc_less_discs(DVec2::new(0.0, 1.5), 1.0, 2.0, &[]);
    assert!((got - (PI - segment)).abs() < 1e-12);
    // A hole holding the centre of another: the two lenses, by sampling.
    let holes = [(DVec2::new(0.5, 0.0), 0.4), (DVec2::new(0.2, 0.5), 0.35)];
    let got = disc_less_discs(c, 0.7, 2.0, &holes);
    let n = 2000;
    let mut inside = 0;
    for i in 0..n {
        for j in 0..n {
            let p = c + DVec2::new(i as f64 + 0.5, j as f64 + 0.5) * (1.4 / n as f64)
                - DVec2::splat(0.7);
            if p.distance(c) < 0.7 && holes.iter().all(|&(q, s)| p.distance(q) >= s) {
                inside += 1;
            }
        }
    }
    let sampled = inside as f64 * (1.4 / n as f64).powi(2);
    assert!((got - sampled).abs() < 1e-3, "{got} vs {sampled}");
}

/// Plates 1 thick drilled twice, and a boss (a cylinder) standing on
/// each, sunk from its bottom up 2, or through it from bottom to top,
/// flush with both: the first `n` of seed 5's cases, each operation
/// tallied (a result heavier than 20 times its operands as failed: flush
/// caps meeting along curves once left seams 10k patches heavy).
fn drilled_bosses(n: usize) -> Tally {
    let tol = Tolerance::DEFAULT;
    let mut tally = Tally::default();
    let mut rng = Rng::new(5);
    let mut samples = Rng::new(6);
    let q = |x: f64| (x * 20.0).round() / 20.0;
    for case in 0..n {
        let (x1, y1, r1) = (
            q(rng.range(-2.2, -0.5)),
            q(rng.range(-1.2, 1.2)),
            q(rng.range(0.2, 0.7)),
        );
        let (x2, y2, r2) = (
            q(rng.range(0.5, 2.2)),
            q(rng.range(-1.2, 1.2)),
            q(rng.range(0.2, 0.7)),
        );
        let (bx, by, br) = (
            q(rng.range(-1.0, 1.0)),
            q(rng.range(-0.8, 0.8)),
            q(rng.range(0.2, 1.2)),
        );
        let mode = rng.next_u64() % 3;
        let slab = Solid::cuboid(
            DVec3::new(-3.0, -2.0, 0.0),
            DVec3::new(6.0, 4.0, 1.0),
            1,
            &tol,
        )
        .unwrap();
        let drill = |x: f64, y: f64, r: f64, feature: u64| {
            Solid::cylinder(DVec3::new(x, y, -1.0), r, 4.0, feature, &tol).unwrap()
        };
        let drilled = |a: &Solid, b: &Solid| boolean(a, b, Op::Difference, &tol, &Budget::DEFAULT);
        let Ok(plate) = drilled(&slab, &drill(x1, y1, r1, 2))
            .and_then(|plate| drilled(&plate, &drill(x2, y2, r2, 3)))
        else {
            // Two holes that nearly touch.
            println!("REFUSED case {case}: drilling");
            continue;
        };
        let (z0, h, what) =
            [(1.0, 1.0, "on"), (0.0, 2.0, "sunk"), (0.0, 1.0, "through")][mode as usize];
        let boss = Solid::cylinder(DVec3::new(bx, by, z0), br, h, 4, &tol).unwrap();
        let holes = [(DVec2::new(x1, y1), r1), (DVec2::new(x2, y2), r2)];
        let shared = disc_less_discs(DVec2::new(bx, by), br, 2.0, &holes);
        let both = shared * ((z0 + h).min(1.0) - z0.max(0.0));
        let name = format!("case {case}, boss {what} at ({bx}, {by}) r {br}, holes {holes:?}");
        let out = four(
            &plate,
            &boss,
            Some(both),
            &tol,
            &mut samples,
            &mut tally,
            &name,
        );
        let inputs = plate.mesh().tris().len() + boss.mesh().tris().len();
        for solid in out.iter().flatten() {
            if solid.mesh().tris().len() > 20 * inputs {
                println!("HEAVY {name}: {} patches", solid.mesh().tris().len());
                tally.ok -= 1;
                tally.failed += 1;
            }
        }
    }
    tally
}

#[test]
fn bosses_sunk_through_drilled_plates() {
    // Near the holes refinement splits the boss's wall at the middle of
    // its rulings, which is the plate's top; one in five of these failed
    // so, mostly sunk. The first 40 cases hold every failure left of the
    // 150 below: 148 of their 160 operations work. What fails is a boss through the plate with its wall
    // crossing a hole's wall (thin triangles where the two walls meet), a
    // boss rim tangent to a hole's rim, and a rim vertex of one lying on
    // the other's rim.
    drilled_bosses(cases(40, 2)).at_least(0.92, "bosses in drilled plates");
}

#[test]
#[ignore = "slow: 150 cases, a minute or so; run in release"]
fn many_bosses_sunk_through_drilled_plates() {
    // 588 of the 600 operations work; 567 did before a cut beside a
    // wall's curve with both ends in a plate's face was given a strip.
    drilled_bosses(150).at_least(0.97, "many bosses in drilled plates");
}

/// A 20 × 20 × 1 box drilled with `count` holes of radius `r` one after
/// another, as a user places them one feature at a time: rows of 8 from
/// (1, 1), `pitch` apart both ways. A failed step is skipped and the
/// chain goes on. Each step that works is checked against the volume
/// `400 − k·π·r²` after `k` holes. Gives the failed steps (0-based) and
/// the last body's patch count.
fn drilled_grid(r: f64, pitch: f64, count: usize) -> (Vec<usize>, usize) {
    let tol = Tolerance::DEFAULT;
    let mut current = Solid::cuboid(DVec3::ZERO, DVec3::new(20.0, 20.0, 1.0), 1, &tol).unwrap();
    let (mut failed, mut holes) = (Vec::new(), 0);
    for step in 0..count {
        let (x, y) = (
            1.0 + (step % 8) as f64 * pitch,
            1.0 + (step / 8) as f64 * pitch,
        );
        let pin = Solid::cylinder(DVec3::new(x, y, -1.0), r, 3.0, 100 + step as u64, &tol).unwrap();
        match boolean(&current, &pin, Op::Difference, &tol, &Budget::DEFAULT) {
            Ok(next) => {
                holes += 1;
                let want = 400.0 - holes as f64 * PI * r * r;
                let got = next.volume();
                assert!(
                    (got - want).abs() < 1e-8,
                    "r {r}, pitch {pitch}, step {step}: {got} vs {want}"
                );
                current = next;
            }
            Err(why) => {
                println!("REFUSED r {r}, pitch {pitch}, step {step} at ({x}, {y}): {why:?}");
                failed.push(step);
            }
        }
    }
    (failed, current.mesh().tris().len())
}

#[test]
fn drilled_grids_in_line() {
    // The second hole in line with an earlier one along x or y grazed the
    // long cap triangles the earlier holes left from the box's far
    // corners: 4 of the 180 steps failed (steps 1 and 8 at pitch 2.4, 3
    // and 24 at 2.2) before the clean-up refined the plane faces a cut
    // makes. In a debug build, the first nine of each; in a quick one,
    // the first four at 2.2 and nine at 2.4, which hold the steps that
    // failed.
    let mut total = 0;
    let pitches: &[(f64, usize)] = if varde_testing::full() || !cfg!(debug_assertions) {
        &[(2.2, 9), (2.3, 9), (2.4, 9)]
    } else {
        &[(2.2, 4), (2.4, 9)]
    };
    for &(pitch, debug) in pitches {
        let (failed, patches) = drilled_grid(0.5, pitch, cases(60, debug));
        println!("pitch {pitch}: failed steps {failed:?}, {patches} patches");
        total += failed.len();
    }
    assert_eq!(total, 0);
}

#[test]
#[ignore = "slow, a few minutes; run in release. 1 of 540 steps fails (r 0.6, pitch 1.3, step 29: Invalid on the new hole's wall at the plate's bottom)"]
fn boxes_drilled_in_grids() {
    // Nine grids of 60 holes (three radii, three pitches); in a debug
    // build, one row of eight. 12 of the 540 steps failed before the
    // clean-up refined the plane faces a cut makes, nearly all the first
    // or second hole in line with an earlier one; 1 does now.
    let configurations: &[(f64, f64)] = if cfg!(debug_assertions) {
        &[(0.5, 2.4)]
    } else {
        &[
            (0.25, 2.4),
            (0.5, 2.4),
            (0.6, 2.4),
            (0.25, 2.2),
            (0.5, 2.2),
            (0.6, 2.2),
            (0.25, 1.3),
            (0.5, 1.3),
            (0.6, 1.3),
        ]
    };
    let mut total = 0;
    for &(r, pitch) in configurations {
        let (failed, patches) = drilled_grid(r, pitch, cases(60, 8));
        println!("r {r}, pitch {pitch}: failed steps {failed:?}, {patches} patches");
        total += failed.len();
    }
    assert_eq!(total, 0);
}

#[test]
#[ignore = "all four fail (Invalid), the clean-up's quality pass on plane faces or not: walls crossing in a vertical line"]
fn a_boss_through_a_drilled_plate_across_a_hole() {
    // A 6 × 4 × 1 plate drilled twice, and a boss through it flush with
    // both its faces whose wall crosses the first hole's wall: thin
    // triangles where the two walls meet in a vertical line.
    let tol = Tolerance::DEFAULT;
    let slab = Solid::cuboid(
        DVec3::new(-3.0, -2.0, 0.0),
        DVec3::new(6.0, 4.0, 1.0),
        1,
        &tol,
    )
    .unwrap();
    let drill = |x: f64, y: f64, r: f64, feature: u64| {
        Solid::cylinder(DVec3::new(x, y, -1.0), r, 4.0, feature, &tol).unwrap()
    };
    let difference = |a: &Solid, b: &Solid| boolean(a, b, Op::Difference, &tol, &Budget::DEFAULT);
    let (h1, h2) = (
        (DVec2::new(-1.1, 1.15), 0.45),
        (DVec2::new(1.95, 0.25), 0.6),
    );
    let plate = difference(&slab, &drill(h1.0.x, h1.0.y, h1.1, 2))
        .and_then(|p| difference(&p, &drill(h2.0.x, h2.0.y, h2.1, 3)))
        .unwrap();
    let want = 24.0 - PI * (h1.1 * h1.1 + h2.1 * h2.1);
    assert!((plate.volume() - want).abs() < 1e-9, "{}", plate.volume());
    let (c, r) = (DVec2::new(-0.8, 0.6), 0.65);
    let boss = Solid::cylinder(DVec3::new(c.x, c.y, 0.0), r, 1.0, 4, &tol).unwrap();
    // The boss misses the second hole and crosses the first.
    assert!(c.distance(h2.0) > r + h2.1);
    let both = disc_less_discs(c, r, 2.0, &[h1, h2]);
    let mut tally = Tally::default();
    let mut samples = Rng::new(6);
    four(
        &plate,
        &boss,
        Some(both),
        &tol,
        &mut samples,
        &mut tally,
        "boss across a hole",
    );
    println!("worked {} of 4", tally.ok);
    assert_eq!(tally.failed, 0);
}

#[test]
fn a_cut_face_that_cant_be_triangulated_shows_its_loops() {
    // Case 11 of the flush unions' pairs (in millimetres), the second
    // less the first: a wall of the first is left with loops of two
    // vertices, along one curve there and back, which no triangle takes
    // and which aren't asked to be split (on a curved patch only a cut's
    // are), so it can't be triangulated. The failure gives the face, by
    // its name on the operand, and its loops as curves: closed, on the
    // wall. The same at 1 and 8 threads.
    let tol = Tolerance::DEFAULT;
    let mut rng = Rng::new(93);
    let mut pair = None;
    for _ in 0..=11 {
        pair = flush_pair(&mut rng, &tol);
    }
    let (a, b, what) = pair.expect("case 11");
    assert_eq!(what, "plate mm 5..10");
    let failure = assert_deterministic(|| {
        boolean(&b, &a, Op::Difference, &tol, &Budget::DEFAULT).unwrap_err()
    });
    assert_eq!(
        failure.error,
        KernelError::Boolean(BooleanError::Degenerate)
    );
    let evidence = &failure.evidence;
    let [(crate::Operand::A, key)] = evidence.faces[..] else {
        panic!("{:?}", evidence.faces);
    };
    let face = (b.mesh().faces().iter())
        .find(|f| f.name.key() == key)
        .expect("a face of the first operand");
    assert!(!evidence.curves.is_empty() && !evidence.truncated);
    let bits = |p: DVec3| p.to_array().map(f64::to_bits);
    let mut starts: Vec<_> = evidence.curves.iter().map(|c| bits(c.p0)).collect();
    let mut ends: Vec<_> = evidence.curves.iter().map(|c| bits(c.p1)).collect();
    starts.sort_unstable();
    ends.sort_unstable();
    assert_eq!(starts, ends);
    for curve in &evidence.curves {
        for p in [curve.p0, curve.p1] {
            assert!(face.surface.distance(p) <= tol.resolution(), "{p}");
        }
    }
}

#[test]
fn tangent_cylinders_that_dont_fit_together_show_where() {
    // About 1.5 s in a debug build, over the quick target: three
    // near-tangent kinds of contact, each needing its four boolean tries.
    // Near-tangent cylinders at the coarsest tolerance, whose decisions
    // with near ties don't fit together (`Inconsistent`) at three kinds
    // of place, each with what it is about, on the walls where they
    // touch (`x` 1, `y` 0), and the same at 1 and 8 threads: seen with
    // the try with ties alone (`ONE_TRY`). Decided again without them,
    // the unions are refused as touching along a line (`NotManifold`),
    // the intersection is the overlap's sliver, under the fit. (Overlaps
    // of `1e-6`, whose pairs of walls had ends and whose edges' crossings
    // couldn't be placed, were refused too until heights were measured
    // in space: their intersections now work with the ties. No pair with
    // ends was seen since among near-tangent cylinders; flat operands
    // still give them, see
    // `near_ties_that_dont_fit_together_are_decided_again_exactly`.)
    let tol = Tolerance::new(Tolerance::MAX_FIT).unwrap();
    let r = tol.resolution();
    let a = Solid::cylinder(DVec3::ZERO, 1.0, 2.0, 2, &tol).unwrap();
    // Each wall's distance from a point, by its operand.
    let off = |gap: f64, operand: crate::Operand, p: DVec3| {
        let axis = match operand {
            crate::Operand::A => DVec2::ZERO,
            crate::Operand::B => DVec2::new(2.0 + gap, 0.0),
        };
        (p.truncate() - axis).length() - 1.0
    };
    let wall = |solid: &Solid| {
        (solid.mesh().faces().iter())
            .find(|f| matches!(f.surface, Surface::Quadric(_)))
            .unwrap()
            .name
            .key()
    };
    let near_line = |p: DVec3| (p.x - 1.0).abs() < 0.05 && p.y.abs() < 0.15;
    // One case of each kind of place first (a vertex, a crossing, an
    // edge), then a second crossing: a quick run takes the three, about
    // 1.5 s in a debug build (measured: the vertex about 0.6 s, the
    // crossing 0.2, the edge 0.75), as each takes four tries of the
    // boolean (two for the threads, two deciding again) on curved walls
    // a hair apart, most of it in the curved searches and exact signs.
    let tries = [
        (-1.5e-6, 0.75, Op::Intersection),
        (1e-6, 1.0, Op::Union),
        (1.5e-6, 1.0, Op::Union),
        (1e-6, 0.25, Op::Union),
    ];
    for (gap, h, op) in tries.into_iter().take(varde_testing::pick(3, 4)) {
        let b = Solid::cylinder(DVec3::ZERO, 1.0, h, 3, &tol).unwrap();
        let b = moved(&b, &tol, |p| p + DVec3::new(2.0 + gap, 0.0, 0.5));
        let failure = assert_deterministic(|| {
            super::ONE_TRY.set(true);
            let tied = boolean(&a, &b, op, &tol, &Budget::DEFAULT);
            super::ONE_TRY.set(false);
            tied.unwrap_err()
        });
        assert_eq!(
            failure.error,
            KernelError::Boolean(BooleanError::Inconsistent)
        );
        match (op, boolean(&a, &b, op, &tol, &Budget::DEFAULT)) {
            (Op::Union, Err(e)) => {
                assert_eq!(e.error, KernelError::Boolean(BooleanError::NotManifold));
            }
            (Op::Intersection, Ok(both)) => {
                let within = tol.fit() * (a.area() + b.area()) / 5.0;
                assert!(both.volume() <= within, "{}", both.volume());
            }
            (_, again) => panic!("{gap} {h} {op:?}: {:?}", again.map(|s| s.volume())),
        }
        super::evidence::tests::on_operands(&a, &b, &failure, &tol);
        let e = &failure.evidence;
        assert!(!e.truncated);
        for &p in &e.points {
            assert!(near_line(p), "{p}");
        }
        if gap < 0.0 {
            // A vertex whose winding number doesn't fit what its edges
            // carried: the vertex, on both walls, the faces of the other
            // operand above it and the triangles round it.
            let [point] = e.points[..] else {
                panic!("{e:?}");
            };
            for operand in [crate::Operand::A, crate::Operand::B] {
                assert!(off(gap, operand, point).abs() <= r, "{point}");
                assert!(e.faces.iter().any(|f| f.0 == operand), "{e:?}");
            }
            assert!(e.curves.is_empty() && !e.patches.is_empty());
            assert!(
                (e.patches.iter()).any(|patch| patch.p.contains(&point)),
                "{e:?}"
            );
        } else if e.points.is_empty() {
            // An edge through a face whose crossings can't be placed: the
            // edge, a piece of one wall from where it meets the other's
            // rim, and the patch of the other's cap there, which the edge
            // starts on.
            let ([curve], [patch], [(operand, key)]) =
                (&e.curves[..], &e.patches[..], &e.faces[..])
            else {
                panic!("{e:?}");
            };
            let (other, solid) = match operand {
                crate::Operand::A => (crate::Operand::B, &a),
                crate::Operand::B => (crate::Operand::A, &b),
            };
            assert_ne!(*key, wall(solid));
            for t in [0.0, 0.5, 1.0] {
                assert!(off(gap, other, curve.eval(t)).abs() <= r);
            }
            for &p in &patch.p {
                assert!(near_line(p) && (p.z - curve.p0.z).abs() <= r, "{p}");
            }
            assert!(near_line(curve.p0) && near_line(curve.p1));
        } else {
            // A crossing the search only placed, off the face it
            // crosses: its vertex, on its edge (the other's rim) and
            // farther than the resolution from the face's wall, and that
            // wall's patch.
            let ([curve], [_], [point], [(operand, key)]) =
                (&e.curves[..], &e.patches[..], &e.points[..], &e.faces[..])
            else {
                panic!("{e:?}");
            };
            let (other, solid) = match operand {
                crate::Operand::A => (crate::Operand::B, &a),
                crate::Operand::B => (crate::Operand::A, &b),
            };
            assert_eq!(*key, wall(solid));
            assert!(off(gap, other, *point).abs() <= r, "{point}");
            assert!(off(gap, *operand, *point) > r, "{point}");
            for t in [0.0, 0.5, 1.0] {
                assert!(off(gap, other, curve.eval(t)).abs() <= r);
            }
        }
    }
}

#[test]
fn a_cut_face_whose_boundary_doesnt_close_shows_it() {
    // The first two parts of seed 361: a disc of radius 0.5 round (x, y)
    // = (−1, −0.25) over z 1..1.25, and a slab with rounded corners on the
    // YZ plane extruded over x −1..−0.75, its start cap through the
    // disc's axis and its side y = 0.25 touching the disc's wall at the
    // seam there. The slab less the disc leaves a face of the slab with
    // pieces of boundary that don't close into loops: refused, with those
    // pieces as curves, the vertices where they stop as points, and the
    // face by its name; the same at 1 and 8 threads.
    let tol = Tolerance::DEFAULT;
    let mut rng = Rng::new(361);
    let disc = part(&mut rng, 0.25, 1, &tol).unwrap();
    let slab = part(&mut rng, 0.25, 10, &tol).unwrap();
    assert!((disc.volume() - PI / 16.0).abs() <= 1e-12);
    let (a, b) = (slab, disc);
    let op = Op::Difference;
    let failure = assert_deterministic(|| boolean(&a, &b, op, &tol, &Budget::DEFAULT).unwrap_err());
    assert_eq!(
        failure.error,
        KernelError::Boolean(BooleanError::Inconsistent)
    );
    super::evidence::tests::on_operands(&a, &b, &failure, &tol);
    let e = &failure.evidence;
    let [(crate::Operand::A, key)] = e.faces[..] else {
        panic!("{:?}", e.faces);
    };
    let face = (a.mesh().faces().iter())
        .find(|f| f.name.key() == key)
        .unwrap();
    assert!(!e.curves.is_empty() && e.patches.is_empty() && !e.truncated);
    for curve in &e.curves {
        for t in [0.0, 0.5, 1.0] {
            let p = curve.eval(t);
            assert!(face.surface.distance(p) <= tol.resolution(), "{p}");
        }
    }
    // Each point is where a piece stops: an end of one curve and of
    // no other's other end.
    assert_eq!(e.points.len(), 2);
    let starts = |p: DVec3| e.curves.iter().filter(|c| c.p0 == p).count();
    let ends = |p: DVec3| e.curves.iter().filter(|c| c.p1 == p).count();
    for &p in &e.points {
        assert_ne!((starts(p), ends(p)), (1, 1), "{p}");
        assert!(starts(p) + ends(p) > 0, "{p}");
    }
    // Working the boundary out again is charged too: an allowance
    // that covers the face and the items but not that gives the face
    // alone, the error as it was.
    let items = 1 + e.curves.len() + e.points.len();
    let short = super::evidence::tests::with_allowance(items as u64, || {
        boolean(&a, &b, op, &tol, &Budget::DEFAULT).unwrap_err()
    });
    assert_eq!(short.error, failure.error);
    assert_eq!(short.evidence.faces, e.faces);
    assert!(short.evidence.curves.is_empty() && short.evidence.points.is_empty());
    assert!(short.evidence.truncated);
}
