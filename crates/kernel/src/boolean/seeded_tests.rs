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
