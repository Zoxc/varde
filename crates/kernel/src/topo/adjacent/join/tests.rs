//! The join-zone flag on a box's edges and a mesh cube's: set; a face's
//! p-curve moved off its edge outwards at seam scale (a lobe of the
//! ribbons) leaves it unset, and the edge's tube still holds; moved
//! inwards it stays set; a set flag the edge no longer certifies is
//! cleared.

use super::*;
use crate::build::primitives::Primitive;
use crate::geom::pcurve::PCurve;
use crate::infra::budget::{Budget, Pass, Work};
use crate::names::topology::tests::primitive;
use crate::topo::adjacent::{window, window_of};
use crate::topo::brep::{CoedgeId, FaceId};
use crate::topo::build::Builder;

fn cuboid() -> Brep {
    primitive(&Primitive::Cuboid {
        min: [1.0, -2.0, 0.5],
        size: [3.0, 5.0, 6.75],
    })
}

fn edges(b: &Brep) -> Vec<EdgeId> {
    (0..b.edge_count() as u32).map(EdgeId).collect()
}

/// The flags of `edges` re-certified on `b`.
fn recertified(b: &Brep, edges: &[EdgeId]) -> Brep {
    let mut builder = Builder::edit(b.clone());
    builder
        .certify_joins(edges, &mut Work::new(&Budget::DEFAULT))
        .expect("certified");
    builder.finish().expect("finished").0
}

/// The certificate of edge `e` alone.
fn certified(b: &Brep, e: EdgeId) -> bool {
    let pass = Pass {
        stage: "test",
        rung: 0,
    };
    Work::new(&Budget::DEFAULT)
        .pass(pass, &[()], |_, ledger| certify_join(b, e, ledger))
        .expect("settled")[0]
}

/// `b` with coedge `c`'s straight p-curve moved by `by` (global UV)
/// across its span's direction, to the face's side (`inward`) or away
/// from it, and its seam bound widened to cover the move.
fn moved(b: &Brep, c: CoedgeId, by: f64, inward: bool) -> Brep {
    let mut b = b.clone();
    let p = b.coedges.pcurve[c.index()];
    let edge = edge_of(&b, b.edge(c)).expect("an edge");
    let side = if edge.sides[0].reversed == b.reversed(c) {
        0
    } else {
        1
    };
    let left = edge.sides[side].left();
    let pc = b.pcurve(p).clone();
    let spans = pc
        .spans()
        .iter()
        .map(|s| {
            let PSpan::Linear(ends) = *s else {
                panic!("a straight span")
            };
            let uv = ends.map(|h| [h[0] / h[2], h[1] / h[2]]);
            let d = [uv[1][0] - uv[0][0], uv[1][1] - uv[0][1]];
            let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
            let mut nu = if left { [-d[1], d[0]] } else { [d[1], -d[0]] };
            if !inward {
                nu = [-nu[0], -nu[1]];
            }
            let shift = |q: [f64; 2]| [q[0] + by * nu[0] / len, q[1] + by * nu[1] / len, 1.0];
            PSpan::Linear(uv.map(shift))
        })
        .collect();
    b.pcurves[p.index()] = PCurve::from_spans(spans, pc.knots().to_vec()).expect("a p-curve");
    b.coedges.seam_bound[c.index()] = 1e-6;
    b
}

/// A box's edges and a mesh cube's are flagged: their faces on axis
/// planes, the ribbons exactly zero, each face on its side of the plane
/// between them.
#[test]
fn a_box_has_embedded_joins() {
    let b = cuboid();
    assert!(edges(&b).iter().all(|&e| b.embedded_join(e)));
    let verts: Vec<[f64; 3]> = (0..8)
        .map(|i| [0, 1, 2].map(|k| if i >> k & 1 == 1 { 4.0 } else { 0.0 }))
        .collect();
    let quads = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    let tris: Vec<[usize; 3]> = quads
        .iter()
        .flat_map(|&[a, b, c, d]| [[a, b, c], [a, c, d]])
        .collect();
    let ctx = crate::infra::ops::OpCtx::new(crate::infra::ops::FeatureId(1));
    let cube = crate::build::polyhedron::build(
        &verts,
        &tris,
        ctx,
        crate::infra::ops::Tolerance::DEFAULT,
        &mut Work::new(&Budget::DEFAULT),
    )
    .expect("a cube");
    assert_eq!(cube.edge_count(), 12);
    assert!(edges(&cube).iter().all(|&e| cube.embedded_join(e)));
}

/// One face's p-curve moved outwards by a ribbon's width: that face's
/// ribbon crosses the plane between the faces, so the flag isn't set,
/// though the edge's tube still holds (valid, only not flagged); moved
/// inwards the flag holds. Re-certified, a set flag the moved edge no
/// longer earns is cleared.
#[test]
fn a_ribbon_overshoot_is_not_flagged() {
    let b = cuboid();
    let face = FaceId(0);
    let c = (0..b.coedge_count() as u32)
        .map(CoedgeId)
        .find(|&c| b.face(c) == face)
        .expect("a coedge");
    let e = b.edge(c);
    let out = moved(&b, c, 1e-9, false);
    assert!(!certified(&out, e));
    let pass = Pass {
        stage: "test",
        rung: 0,
    };
    let tube = Work::new(&Budget::DEFAULT)
        .pass(pass, &[()], |_, ledger| window_of(&out, e, ledger))
        .expect("a window");
    assert!(matches!(tube[0], window::Window::Tube(_)), "{tube:?}");
    assert!(certified(&moved(&b, c, 1e-9, true), e));
    assert!(out.embedded_join(e));
    let cleared = recertified(&out, &[e]);
    assert!(!cleared.embedded_join(e));
    let others: Vec<EdgeId> = edges(&b).into_iter().filter(|&g| g != e).collect();
    assert!(others.iter().all(|&g| cleared.embedded_join(g)));
}
