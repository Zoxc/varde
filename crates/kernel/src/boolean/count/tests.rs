//! The counting's `Inconsistent` failures and what they show, from
//! primitives that disagree with each other on purpose (consistent ones
//! never give them).

use std::cmp::Ordering;

use glam::DVec3;

use super::*;
use crate::boolean::flat::Flat;
use crate::boolean::{Cross11, Found};
use crate::mesh::FaceKey;
use crate::mesh::tests::TOL;
use crate::{Budget, Operand, Solid};

/// The exact flat primitives, but `s02` of vertex `v` of `A` doubled for
/// each `v` of `doubled`.
struct Doubled<'a> {
    flat: Flat<'a>,
    doubled: Vec<u32>,
}

impl Primitives for Doubled<'_> {
    fn s02(&self, side: Side, v: u32, f: u32) -> i8 {
        let s = self.flat.s02(side, v, f);
        if side == Side::A && self.doubled.contains(&v) {
            2 * s
        } else {
            s
        }
    }
    fn s11(&self, e: u32, g: u32) -> Cross11 {
        self.flat.s11(e, g)
    }
    fn searches(&self, side: Side, e: u32, f: u32) -> bool {
        self.flat.searches(side, e, f)
    }
    fn search_work(&self) -> usize {
        self.flat.search_work()
    }
    fn margin(&self) -> f64 {
        self.flat.margin()
    }
    fn crossings(&self, side: Side, e: u32, f: u32, x: i32) -> Result<Found, BooleanError> {
        self.flat.crossings(side, e, f, x)
    }
    fn order(&self, side: Side, e: u32, c1: &Crossing, c2: &Crossing) -> Ordering {
        self.flat.order(side, e, c1, c2)
    }
}

fn cube(min: f64, size: f64) -> Solid {
    Solid::cuboid(DVec3::splat(min), DVec3::splat(size), 1, &TOL).unwrap()
}

/// The name of the face triangle `t` of `input` lies on.
fn name(input: &Input, t: u32) -> FaceKey {
    input.mesh.faces()[input.face(t) as usize].name.key()
}

/// Checks `failure` shows vertex `v` of `a` (inside `b`, whose top is
/// above it): the vertex, the top's triangles above it, and the
/// triangles round it, with their faces' names.
fn shows_vertex(failure: &Failure, a: &Input, b: &Input, v: u32) {
    assert_eq!(
        failure.error,
        KernelError::Boolean(BooleanError::Inconsistent)
    );
    let e = &failure.evidence;
    assert_eq!(e.points, [a.pos(v)]);
    assert!(e.curves.is_empty() && !e.truncated);
    let round: Vec<u32> = (0..a.tris.len() as u32)
        .filter(|&t| a.tris[t as usize].contains(&v))
        .collect();
    let above = e.patches.len() - round.len();
    assert!(above >= 1, "{e:?}");
    // The top's triangles first, above the vertex.
    for patch in &e.patches[..above] {
        assert!(patch.p.iter().all(|p| p.z == 1.0), "{patch:?}");
    }
    let rounds: Vec<_> = round.iter().map(|&t| a.patches[t as usize]).collect();
    assert_eq!(e.patches[above..], rounds[..]);
    let top = (0..b.tris.len() as u32)
        .find(|&t| b.corners(t).iter().all(|p| p.z == 1.0))
        .unwrap();
    assert_eq!(e.faces[0], (Operand::B, name(b, top)));
    let mut own: Vec<(Operand, FaceKey)> =
        round.iter().map(|&t| (Operand::A, name(a, t))).collect();
    own.dedup();
    assert_eq!(e.faces[1..], own[..]);
}

#[test]
fn a_ray_that_disagrees_shows_its_vertex() {
    // A small cube inside a larger one, the layers above its last vertex
    // (the second ray's) doubled: its own ray finds 2 where its edges
    // carried 1 from the first.
    let (a, b) = (cube(0.25, 0.5), cube(0.0, 1.0));
    let (ia, ib) = (Input::new(a.mesh(), &TOL), Input::new(b.mesh(), &TOL));
    let last = ia.mesh.verts().len() as u32 - 1;
    let prims = Doubled {
        flat: Flat::tied(&ia, &ib, true, 0.0),
        doubled: vec![last],
    };
    let mut work = Work::new(&Budget::DEFAULT);
    let failure = count(&ia, &ib, &prims, &TOL, &mut work).unwrap_err();
    shows_vertex(&failure, &ia, &ib, last);
}

#[test]
fn a_winding_number_out_of_range_shows_its_vertex() {
    // Both rays' layers doubled: they agree, but every winding number of
    // the small cube is 2, and the first vertex shows it.
    let (a, b) = (cube(0.25, 0.5), cube(0.0, 1.0));
    let (ia, ib) = (Input::new(a.mesh(), &TOL), Input::new(b.mesh(), &TOL));
    let last = ia.mesh.verts().len() as u32 - 1;
    let prims = Doubled {
        flat: Flat::tied(&ia, &ib, true, 0.0),
        doubled: vec![0, last],
    };
    let mut work = Work::new(&Budget::DEFAULT);
    let failure = count(&ia, &ib, &prims, &TOL, &mut work).unwrap_err();
    shows_vertex(&failure, &ia, &ib, 0);
}

#[test]
fn an_edge_whose_winding_numbers_dont_add_up_shows_its_crossings() {
    // Cubes crossing, counted right; then one vertex's winding number
    // changed, so the first edge from it disagrees: the edge, its
    // crossings and the faces they cross.
    let (a, b) = (cube(0.0, 1.0), cube(0.5, 1.0));
    let (ia, ib) = (Input::new(a.mesh(), &TOL), Input::new(b.mesh(), &TOL));
    let prims = Flat::tied(&ia, &ib, true, 0.0);
    let mut work = Work::new(&Budget::DEFAULT);
    let mut counts = count(&ia, &ib, &prims, &TOL, &mut work).unwrap();
    assert_eq!(agree(&ia, &counts.x12, &counts.w03), None);
    // The corner inside the other: the edges from it cross its faces.
    let v = (0..ia.mesh.verts().len() as u32)
        .find(|&v| ia.pos(v) == DVec3::ONE)
        .unwrap();
    counts.w03[v as usize] += 1;
    let e = agree(&ia, &counts.x12, &counts.w03).unwrap();
    assert!(ia.edges[e as usize].contains(&v));
    let failure = edge_failure(Side::A, &ia, &ib, &counts.x12, e);
    assert_eq!(
        failure.error,
        KernelError::Boolean(BooleanError::Inconsistent)
    );
    let ev = &failure.evidence;
    assert_eq!(ev.curves, [ia.conic(e)]);
    let crossed: Vec<&Crossing> = counts.x12.iter().filter(|c| c.edge == e).collect();
    assert!(!crossed.is_empty());
    let at: Vec<DVec3> = crossed.iter().map(|c| ia.conic(e).eval(c.t)).collect();
    assert_eq!(ev.points, at);
    // Each crossing is on the face it crosses, at 0.5 on one axis.
    for (p, c) in at.iter().zip(&crossed) {
        assert!(
            ib.corners(c.face)
                .iter()
                .all(|q| (0..3).any(|k| q[k] == 0.5 && p[k] == 0.5))
        );
    }
    let mut faces: Vec<_> = crossed
        .iter()
        .map(|c| (Operand::B, name(&ib, c.face)))
        .collect();
    faces.dedup();
    assert_eq!(ev.faces, faces);
    assert!(!ev.patches.is_empty() && !ev.truncated);
}

#[test]
fn the_scan_round_a_vertex_stops_where_the_allowance_does() {
    // A vertex of a prism of 400 sides whose triangles are spread over
    // several chunks of the scan, given an allowance that runs out after
    // the first chunk holding any: those are kept, in order, the rest
    // left out and the evidence marked truncated. Scanning all or none
    // would keep none.
    let n = 400;
    let points: Vec<glam::DVec2> = (0..n)
        .map(|i| {
            let (sin, cos) =
                crate::trig::sin_cos(std::f64::consts::TAU * f64::from(i) / f64::from(n));
            glam::DVec2::new(cos, sin)
        })
        .collect();
    let profile = crate::Profile {
        loops: vec![crate::profile::tests::polygon(&points, 1)],
    };
    let a = crate::extrude(
        &profile,
        &crate::Frame::XY,
        0.0,
        1.0,
        1,
        &TOL,
        &Budget::DEFAULT,
    )
    .unwrap();
    let ia = Input::new(a.mesh(), &TOL);
    let chunk = |t: u32| t as usize / SCAN_PER_UNIT;
    let chunks = ia.tris.len().div_ceil(SCAN_PER_UNIT);
    let round = |v: u32| -> Vec<u32> {
        (0..ia.tris.len() as u32)
            .filter(|&t| ia.tris[t as usize].contains(&v))
            .collect()
    };
    let (v, kept) = (0..ia.mesh.verts().len() as u32)
        .find_map(|v| {
            let round = round(v);
            let first = chunk(round[0]);
            let kept: Vec<u32> = round
                .iter()
                .copied()
                .filter(|&t| chunk(t) == first)
                .collect();
            // Past the first chunk with any, and more than the allowance.
            let spread = kept.len() < round.len();
            let allowance = 1 + (first + 1) + kept.len();
            (spread && allowance < chunks).then_some((v, kept))
        })
        .expect("a vertex whose triangles are spread out");
    let allowance = 1 + (chunk(kept[0]) + 1) + kept.len();
    let empty = Table {
        keys: Vec::new(),
        values: Vec::new(),
    };
    let failure = crate::boolean::evidence::tests::with_allowance(allowance as u64, || {
        vertex_failure(Side::A, &ia, &ia, &empty, v)
    });
    let e = &failure.evidence;
    assert_eq!(e.points, [ia.pos(v)]);
    let patches: Vec<_> = kept.iter().map(|&t| ia.patches[t as usize]).collect();
    assert_eq!(e.patches, patches);
    assert!(e.truncated);
}
