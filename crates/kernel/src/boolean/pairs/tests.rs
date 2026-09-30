#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use glam::{DQuat, DVec2, DVec3};

use super::*;
use crate::budget::Budget;
use crate::mesh::tests::{TOL, face, free, round_octahedron};
use crate::mesh::{MeshBuilder, Surface};
use crate::par::assert_deterministic;
use crate::profile::tests::circle;
use crate::{Frame, Profile, Solid, extrude, touches};

fn cube(min: [f64; 3], size: [f64; 3]) -> Solid {
    Solid::cuboid(DVec3::from(min), DVec3::from(size), 1, &TOL).unwrap()
}

fn cylinder(base: [f64; 3], r: f64, h: f64) -> Solid {
    Solid::cylinder(DVec3::from(base), r, h, 2, &TOL).unwrap()
}

/// The cylinder of radius `r` along `x` from `x0` to `x1`, its axis
/// through `(·, y, z)`.
pub(crate) fn cylinder_x(y: f64, z: f64, r: f64, x0: f64, x1: f64) -> Solid {
    let frame = Frame {
        origin: DVec3::new(0.0, y, z),
        x: DVec3::Y,
        y: DVec3::Z,
    };
    let profile = Profile {
        loops: vec![circle(DVec2::ZERO, r, 0, false)],
    };
    extrude(&profile, &frame, x0, x1, 3, &TOL, &Budget::DEFAULT).unwrap()
}

/// The box from `min` to `max` in the frame turned by `q`: each face its
/// own plane.
pub(crate) fn turned_box(min: DVec3, max: DVec3, q: DQuat) -> Solid {
    let verts: Vec<DVec3> = (0..8)
        .map(|i| {
            let pick = |bit: u32, lo: f64, hi: f64| if i >> bit & 1 == 0 { lo } else { hi };
            q * DVec3::new(
                pick(0, min.x, max.x),
                pick(1, min.y, max.y),
                pick(2, min.z, max.z),
            )
        })
        .collect();
    let tris = [
        [0, 2, 1],
        [1, 2, 3],
        [4, 5, 6],
        [5, 7, 6],
        [0, 1, 4],
        [1, 5, 4],
        [2, 6, 3],
        [3, 6, 7],
        [0, 4, 2],
        [2, 4, 6],
        [1, 3, 5],
        [3, 7, 5],
    ];
    let mut builder = MeshBuilder::new();
    for &p in &verts {
        builder.vert(p);
    }
    for (i, &[a, b, c]) in tris.iter().enumerate() {
        let [p, x, y] = [a, b, c].map(|v| verts[v as usize]);
        let n = (x - p).cross(y - p);
        let f = builder.face(face(i as u32, Surface::Plane { n, d: n.dot(p) }));
        builder.tri([a, b, c], f);
    }
    Solid::new(builder.build().unwrap(), &TOL).unwrap()
}

fn refine(a: &Solid, b: &Solid, grow: bool) -> Refined {
    refined(
        a.mesh(),
        b.mesh(),
        grow,
        &TOL,
        &mut Work::new(&Budget::DEFAULT),
    )
    .unwrap()
}

/// How many closed curves the arcs make, joined at their ends; every end
/// must be in exactly two arcs (one for each face beside its edge).
fn loops(r: &Refined) -> usize {
    let mut uses: std::collections::BTreeMap<u32, usize> = Default::default();
    for arc in &r.arcs {
        *uses.entry(arc.plus).or_default() += 1;
        *uses.entry(arc.minus).or_default() += 1;
    }
    assert!(uses.values().all(|&n| n == 2), "{uses:?}");
    let ids: Vec<u32> = uses.keys().copied().collect();
    let index = |id: u32| ids.binary_search(&id).unwrap() as u32;
    let part = crate::boolean::parts(
        ids.len(),
        r.arcs.iter().map(|a| [index(a.plus), index(a.minus)]),
    );
    (0..ids.len()).filter(|&i| part[i] == i as u32).count()
}

/// Where each end of the cut is.
fn ends_at(r: &Refined) -> Vec<(u32, DVec3)> {
    let ia = Input::new(&r.a, &TOL);
    let ib = Input::new(&r.b, &TOL);
    let [first12, first21] = first_ids(&ia, &ib, &r.counts);
    let mut out = Vec::new();
    for (i, c) in r.counts.x12.iter().enumerate() {
        out.push((first12 + i as u32, ia.conic(c.edge).eval(c.t)));
    }
    for (i, c) in r.counts.x21.iter().enumerate() {
        out.push((first21 + i as u32, ib.conic(c.edge).eval(c.t)));
    }
    out
}

#[test]
fn a_cylinder_through_a_box() {
    // A round bar through a slab: two circles, at its top and bottom.
    let slab = cube([-2.0, -2.0, 0.0], [4.0, 4.0, 2.0]);
    let bar = cylinder([0.0, 0.0, -1.0], 1.0, 4.0);
    for (a, b, bar_is_a) in [(&bar, &slab, true), (&slab, &bar, false)] {
        for grow in [true, false] {
            let r = refine(a, b, grow);
            assert_eq!(loops(&r), 2);
            // Every end on both surfaces.
            for (_, p) in ends_at(&r) {
                let radius = DVec2::new(p.x, p.y).length();
                assert!((radius - 1.0).abs() < 1e-9, "{p}");
                assert!(p.z.abs() < 1e-9 || (p.z - 2.0).abs() < 1e-9, "{p}");
            }
            // Nothing of one is inside the other at a vertex: the bar's
            // ends stick out, and the slab's corners are outside it.
            assert!(r.counts.w03.iter().chain(&r.counts.w30).all(|&w| w == 0));
            assert!(r.counts.meet(), "{bar_is_a}");
        }
    }
}

#[test]
fn a_blind_hole() {
    // The bar ends inside the slab: one circle, and its lower end's
    // vertices inside.
    let slab = cube([-2.0, -2.0, 0.0], [4.0, 4.0, 2.0]);
    let bar = cylinder([0.3, -0.2, 1.0], 0.7, 2.0);
    let r = refine(&bar, &slab, false);
    assert_eq!(loops(&r), 1);
    let inside: usize = r.counts.w03.iter().filter(|&&w| w == 1).count();
    // The bottom cap's centre and four rim vertices.
    assert_eq!(inside, 5, "{:?}", r.counts.w03);
    for (_, p) in ends_at(&r) {
        assert!((p.z - 2.0).abs() < 1e-9, "{p}");
    }
}

#[test]
fn crossing_cylinders() {
    // A thinner bar along x through an upright one: it goes in and comes
    // out, two closed curves.
    let upright = cylinder([0.0, 0.0, -2.0], 1.0, 4.0);
    let across = cylinder_x(0.1, 0.2, 0.7, -2.0, 2.0);
    for grow in [true, false] {
        let r = refine(&upright, &across, grow);
        assert_eq!(loops(&r), 2);
        for (_, p) in ends_at(&r) {
            let on_upright = DVec2::new(p.x, p.y).length() - 1.0;
            let on_across = DVec2::new(p.y - 0.1, p.z - 0.2).length() - 0.7;
            assert!(on_upright.abs() < 1e-9 && on_across.abs() < 1e-9, "{p}");
        }
    }
}

#[test]
fn an_arc_through_a_face_and_back() {
    // A slab turned 45° about z, its face cutting each quarter arc of the
    // cylinder's caps between x and y twice: the count along the arc
    // through that face is 0, and the search finds both crossings.
    let bar = cylinder([0.0, 0.0, 0.0], 1.0, 1.0);
    let q = DQuat::from_rotation_z(std::f64::consts::FRAC_PI_4);
    let slab = turned_box(DVec3::new(0.9, -5.0, -1.0), DVec3::new(5.0, 5.0, 2.0), q);
    let (ia, ib) = (Input::new(bar.mesh(), &TOL), Input::new(slab.mesh(), &TOL));
    let prims = Curved::new(&ia, &ib, false, &TOL);
    let mut work = Work::new(&Budget::DEFAULT);
    let counts = count::count(&ia, &ib, &prims, &TOL, &mut work).unwrap();
    let twice = counts
        .x12
        .chunk_by(|x, y| (x.edge, x.face) == (y.edge, y.face))
        .filter(|run| run.len() == 2 && run[0].x + run[1].x == 0 && run[0].t < run[1].t)
        .count();
    // The top and bottom arcs.
    assert_eq!(twice, 2, "{:?}", counts.x12);
    // Refined, the arcs are split between the two crossings, and the cut
    // is one closed curve on the face's plane.
    let r = refine(&bar, &slab, false);
    assert_eq!(loops(&r), 1);
    let n = q * DVec3::X;
    for (_, p) in ends_at(&r) {
        assert!((p.dot(n) - 0.9).abs() < 1e-9, "{p}");
    }
}

#[test]
fn touching() {
    let slab = cube([-2.0, -2.0, 0.0], [4.0, 4.0, 2.0]);
    let t = |a: &Solid, b: &Solid| touches(a, b, &TOL, &Budget::DEFAULT).unwrap();
    // Through, inside, standing on it, and apart.
    assert!(t(&cylinder([0.0, 0.0, -1.0], 1.0, 4.0), &slab));
    assert!(t(&slab, &cylinder([0.0, 0.0, 0.5], 0.5, 1.0)));
    assert!(t(&cylinder([0.0, 0.0, 2.0], 0.5, 1.0), &slab));
    assert!(!t(&cylinder([0.0, 0.0, 2.5], 0.5, 1.0), &slab));
    assert!(!t(&slab, &cylinder([4.0, 0.0, 0.0], 1.0, 1.0)));
}

/// The round octahedron, and a box whose top face, square to `d`, is `h`
/// from the centre along it, reaching well past the octahedron to the
/// sides; the top's diagonal is off `d`.
pub(crate) fn poke(h: f64) -> (Solid, Solid) {
    let ball = Solid::new(round_octahedron(DVec3::ZERO), &TOL).unwrap();
    let d = DVec3::ONE.normalize();
    let q = DQuat::from_rotation_arc(DVec3::Z, d);
    let slab = turned_box(DVec3::new(-2.0, -2.0, -5.0), DVec3::new(6.0, 6.0, h), q);
    (ball, slab)
}

/// How far the round octahedron reaches along `(1, 1, 1)`: at its
/// patch's middle, by symmetry.
pub(crate) fn reach() -> f64 {
    let mesh = round_octahedron(DVec3::ZERO);
    let d = DVec3::ONE.normalize();
    (0..mesh.tris().len())
        .map(|t| mesh.patch(t).eval(DVec3::splat(1.0 / 3.0)).dot(d))
        .fold(f64::NEG_INFINITY, f64::max)
}

#[test]
fn a_hidden_loop_is_found() {
    // The slab's top cuts a small cap off one patch of the octahedron,
    // crossing no edge of either: the counting alone sees nothing, and
    // the pair has no certificate, so it is refined until it does.
    let top = reach();
    let (ball, slab) = poke(top - 0.01);
    for grow in [true, false] {
        let r = refine(&ball, &slab, grow);
        assert_eq!(loops(&r), 1);
        let d = DVec3::ONE.normalize();
        for (_, p) in ends_at(&r) {
            assert!((p.dot(d) - (top - 0.01)).abs() < 1e-9, "{p}");
        }
    }
    // Short of it, the ball is inside the slab, and nothing is cut.
    let (ball, slab) = poke(top + 0.01);
    let r = refine(&ball, &slab, true);
    assert!(r.arcs.is_empty());
    assert!(r.counts.w03.iter().all(|&w| w == 1));
}

/// A solid whose top is the saddle `z = 10 + (x² − y²)/2` over a
/// triangle round the saddle point, on vertical walls down to `z = 0`.
pub(crate) fn saddle() -> Solid {
    let f = |x: f64, y: f64| 10.0 + 0.5 * (x * x - y * y);
    // Its polar form: the height of the control point between two corners.
    let blossom = |a: DVec2, b: DVec2| 10.0 + 0.5 * (a.x * b.x - a.y * b.y);
    let xy = [
        DVec2::new(-1.5, -1.0),
        DVec2::new(1.5, -1.0),
        DVec2::new(0.0, 1.5),
    ];
    let mut builder = MeshBuilder::new();
    let top = xy.map(|p| builder.vert(p.extend(f(p.x, p.y))));
    let bottom = xy.map(|p| builder.vert(p.extend(0.0)));
    let top_face = free(&mut builder);
    builder.tri(top, top_face);
    for i in 0..3 {
        let j = (i + 1) % 3;
        let (a, b) = (xy[i], xy[j]);
        let mid = (a + b) * 0.5;
        builder.edge(top[i], top[j], mid.extend(blossom(a, b)), 1.0);
        // The wall below the edge, facing out (to the edge's right).
        let n = (b - a).perp().extend(0.0) * -1.0;
        let wall = builder.face(face(
            1 + i as u32,
            Surface::Plane {
                n,
                d: n.dot(a.extend(0.0)),
            },
        ));
        builder.tri([top[i], bottom[i], bottom[j]], wall);
        builder.tri([top[i], bottom[j], top[j]], wall);
    }
    let floor = builder.face(face(
        4,
        Surface::Plane {
            n: -DVec3::Z,
            d: 0.0,
        },
    ));
    builder.tri([bottom[0], bottom[2], bottom[1]], floor);
    Solid::new(builder.build().unwrap(), &TOL).unwrap()
}

#[test]
fn a_saddle_pairs_its_ends_by_the_side_of_its_saddle_point() {
    // A plane cuts the saddle in a hyperbola: above the saddle point its
    // branches lie either side of x = 0, below it either side of y = 0.
    // The four ends on the saddle's patch pair up accordingly, which the
    // ends alone don't say.
    let solid = saddle();
    let saddle_face = 0;
    for (c, by_x, curves) in [(10.2, true, 2), (9.8, false, 1)] {
        let slab = cube([-5.0, -5.0, -1.0], [14.0, 14.0, c + 1.0]);
        let r = refine(&solid, &slab, false);
        let at: std::collections::BTreeMap<u32, DVec3> = ends_at(&r).into_iter().collect();
        let mut on_saddle = 0;
        for arc in &r.arcs {
            if r.a.tris()[arc.tris[0] as usize].face != saddle_face {
                continue;
            }
            on_saddle += 1;
            let (p, q) = (at[&arc.plus], at[&arc.minus]);
            let (sp, sq) = if by_x { (p.x, q.x) } else { (p.y, q.y) };
            assert!(sp * sq > 0.0, "{c}: {p} {q}");
        }
        assert!(on_saddle >= 2, "{c}");
        // Above the saddle point, the two lobes above the plane each
        // have their curve; below it, the part above is one band.
        assert_eq!(loops(&r), curves, "{c}");
    }
}

/// Two cylinders side by side, touching along a line, at the coarsest
/// tolerance (to keep the tests quick: pieces are flat within it sooner).
fn tangent_cylinders() -> (Solid, Solid, Tolerance) {
    let tol = Tolerance::new(Tolerance::MAX_FIT).unwrap();
    let (s, c) = (0.3f64.sin(), 0.3f64.cos());
    let a = Solid::cylinder(DVec3::ZERO, 1.0, 2.0, 2, &tol).unwrap();
    let b = Solid::cylinder(DVec3::new(2.0 * c, 2.0 * s, 0.5), 1.0, 0.25, 2, &tol).unwrap();
    (a, b, tol)
}

#[test]
fn tangent_cylinders_are_decided() {
    // Decided at the size floor or once the pieces along the tangency
    // are flat, within the budget, the same way every time.
    let (a, b, tol) = tangent_cylinders();
    let r = assert_deterministic(|| {
        refined(
            a.mesh(),
            b.mesh(),
            false,
            &tol,
            &mut Work::new(&Budget::DEFAULT),
        )
        .map(|r| (r.counts.x12, r.counts.x21, r.arcs))
    });
    r.unwrap();
}

#[test]
fn tangent_cylinders_touch_or_not() {
    // Whether a tangency touches is a tie no crossing shows; the fixed
    // rules take it as not meeting (flat solids touching do meet).
    let (a, b, tol) = tangent_cylinders();
    touches(&a, &b, &tol, &Budget::DEFAULT).unwrap();
}

#[test]
fn deterministic() {
    let upright = cylinder([0.0, 0.0, -2.0], 1.0, 4.0);
    let across = cylinder_x(0.1, 0.2, 0.7, -2.0, 2.0);
    assert_deterministic(|| {
        let r = refine(&upright, &across, true);
        (r.a, r.b, r.counts.x12, r.counts.x21, r.arcs)
    });
    let (ball, slab) = poke(reach() - 0.01);
    assert_deterministic(|| {
        let r = refine(&ball, &slab, false);
        (r.a, r.b, r.counts.x12, r.counts.x21, r.arcs)
    });
}

#[test]
fn the_budget_bounds_it() {
    let upright = cylinder([0.0, 0.0, -2.0], 1.0, 4.0);
    let across = cylinder_x(0.1, 0.2, 0.7, -2.0, 2.0);
    let r = refined(
        upright.mesh(),
        across.mesh(),
        true,
        &TOL,
        &mut Work::new(&Budget::new(200)),
    );
    assert!(matches!(r, Err(KernelError::TooComplex)));
}

#[test]
fn ends_join_round_the_pair_as_parentheses_do() {
    let end = |id: u32, sign: i8, x: f64, y: f64| End {
        id,
        sign,
        at: DVec3::new(x, y, 0.0),
    };
    let corners = [DVec3::ZERO, DVec3::X, DVec3::Y];
    // Round the middle: + at east, − north, + west, − south.
    let ends = [
        end(1, 1, 1.0, 0.0),
        end(2, -1, 0.0, 1.0),
        end(3, 1, -1.0, 0.0),
        end(4, -1, 0.0, -1.0),
    ];
    let joined = round_order(corners, &ends).unwrap();
    let ids: Vec<(u32, u32)> = joined.iter().map(|(p, m)| (p.id, m.id)).collect();
    assert_eq!(ids, vec![(1, 2), (3, 4)]);
    // Starting with a −: still every − after its +.
    let ends = [
        end(1, -1, 1.0, 0.0),
        end(2, 1, 0.0, 1.0),
        end(3, 1, -1.0, 0.0),
        end(4, -1, 0.0, -1.0),
    ];
    let joined = round_order(corners, &ends).unwrap();
    let ids: Vec<(u32, u32)> = joined.iter().map(|(p, m)| (p.id, m.id)).collect();
    assert_eq!(ids, vec![(3, 4), (2, 1)]);
    // Unbalanced ends are refused.
    assert!(round_order(corners, &ends[..3]).is_err());
}
