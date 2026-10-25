//! The broad phase on primitives: every face tile's box holds its face's
//! points, a restricted tile's box lies in the tile's hull; two boxes'
//! pairs are those whose boxes meet, none when far apart; the column
//! along an axis and along a slanted direction finds the faces above and
//! below a point; running out is `OutOfBudget`, the same on any number of
//! threads.

use super::*;
use crate::build::primitives::Primitive;
use crate::infra::budget::Budget;
use crate::names::topology::tests::primitive;

fn cuboid(min: [f64; 3], size: [f64; 3]) -> Brep {
    primitive(&Primitive::Cuboid { min, size })
}

fn sphere() -> Brep {
    primitive(&Primitive::Sphere {
        centre: [1.0, 2.0, 3.0],
        radius: 4.0,
    })
}

fn leaves_of(b: &Brep) -> Leaves {
    leaves(b, &mut Work::new(&Budget::DEFAULT)).expect("leaves")
}

/// The point of face `f` at UV `uv` (`None` off the surface).
fn at(b: &Brep, f: FaceId, uv: [f64; 2]) -> Option<[f64; 3]> {
    b.face_surface(f).eval(uv).map(|p| p.map(|x| x.v))
}

/// Every vertex and every face's trim points lie in a leaf box of the
/// face; a sphere's restricted boxes lie in their tiles' hulls.
#[test]
fn leaves_hold_their_faces() {
    for b in [cuboid([1.0, -2.0, 0.5], [3.0, 5.0, 6.75]), sphere()] {
        let l = leaves_of(&b);
        assert_eq!(
            l.edges.len(),
            (0..b.edge_count())
                .map(|e| {
                    let e = EdgeId(e as u32);
                    let [t0, t1] = b.t_range(e);
                    let k = b.edge_curve(e).knots();
                    (0..k.len() - 1)
                        .filter(|&i| k[i] < t1 && k[i + 1] > t0)
                        .count()
                })
                .sum::<usize>()
        );
        for f in 0..b.face_count() as u32 {
            let f = FaceId(f);
            let boxes: Vec<&Aabb<3>> = l
                .faces
                .iter()
                .filter(|t| t.face == f)
                .map(|t| &t.boxed)
                .collect();
            assert!(!boxes.is_empty());
            for lp in b.loops(f) {
                for c in b.loop_coedges(lp) {
                    for s in b.coedge_pcurve(c).spans() {
                        for uv in [s.start(), s.end()] {
                            let p = at(&b, f, uv).expect("on the surface");
                            assert!(
                                boxes
                                    .iter()
                                    .any(|x| x.inflated(1e-12).meets(&Aabb::point(p))),
                                "{p:?}"
                            );
                        }
                    }
                }
            }
        }
        for t in &l.faces {
            let hull: Aabb<3> = b
                .face_surface(t.face)
                .tile(t.tile[0], t.tile[1])
                .unwrap()
                .hull_box()
                .into();
            let grown = hull.inflated(1e-9);
            assert!((0..3).all(|k| grown.lo[k] <= t.boxed.lo[k] && t.boxed.hi[k] <= grown.hi[k]));
        }
    }
}

fn pairs(a: &Brep, b: &Brep, budget: &Budget) -> Result<Candidates, KernelError> {
    let mut work = Work::new(budget);
    let (la, lb) = (leaves(a, &mut work)?, leaves(b, &mut work)?);
    candidates(
        &la,
        &lb,
        |e| a.edge_tol(e),
        |e| b.edge_tol(e),
        5e-4,
        &mut work,
    )
}

/// Overlapping boxes pair exactly the leaves whose inflated boxes meet;
/// boxes far apart pair nothing; boxes a gap under `ε_dec` apart pair
/// their facing sides.
#[test]
fn pairs_are_the_leaves_whose_boxes_meet() {
    let a = cuboid([0.0; 3], [4.0; 3]);
    let b = cuboid([2.0, 1.0, -1.0], [4.0, 2.0, 2.0]);
    let c = pairs(&a, &b, &Budget::DEFAULT).expect("pairs");
    let (la, lb) = (leaves_of(&a), leaves_of(&b));
    let mut want = Vec::new();
    for (i, x) in la.faces.iter().enumerate() {
        for (j, y) in lb.faces.iter().enumerate() {
            if x.boxed.inflated(5e-4).meets(&y.boxed.inflated(5e-4)) {
                want.push([i as u32, j as u32]);
            }
        }
    }
    assert_eq!(c.faces, want);
    assert!(!c.a_edges.is_empty() && !c.b_edges.is_empty());
    let far = cuboid([100.0; 3], [1.0; 3]);
    assert_eq!(pairs(&a, &far, &Budget::DEFAULT), Ok(Candidates::default()));
    let near = cuboid([4.0 + 1e-4, 0.0, 0.0], [1.0; 3]);
    let c = pairs(&a, &near, &Budget::DEFAULT).expect("pairs");
    assert!(!c.faces.is_empty());
}

/// Pairs on one thread and on several are the same, and a budget too
/// small runs out as `OutOfBudget`.
#[test]
fn pairs_run_out_the_same_way() {
    let a = sphere();
    let b = cuboid([0.0; 3], [4.0; 3]);
    #[cfg(not(target_arch = "wasm32"))]
    crate::infra::par::assert_deterministic(|| pairs(&a, &b, &Budget::DEFAULT)).expect("pairs");
    assert!(matches!(
        pairs(&a, &b, &Budget::new(20)),
        Err(KernelError::OutOfBudget { .. })
    ));
}

/// The column along `+z` through a point over a box's top finds its top
/// and bottom faces' tiles and no side wholly off the column; along a
/// slanted direction a point moved along it from the box's top face
/// still finds that face.
#[test]
fn the_column_finds_the_faces_on_it() {
    let b = cuboid([0.0; 3], [4.0; 3]);
    let l = leaves_of(&b);
    let mut work = Work::new(&Budget::DEFAULT);
    let hits = column(
        &l,
        &[[1.0, 3.0, 10.0], [9.0, 9.0, 0.0]],
        [0.0, 0.0, 1.0],
        &mut work,
    )
    .expect("hits");
    let faces: Vec<FaceId> = hits
        .iter()
        .filter(|h| h[0] == 0)
        .map(|h| l.faces[h[1] as usize].face)
        .collect();
    // Bottom 0 and top 1; the sides' shadows are segments on the
    // square's rim, off the point's.
    assert_eq!(faces, [FaceId(0), FaceId(1)]);
    assert!(hits.iter().all(|h| h[0] == 0));
    let r = [0.3, -0.7, 1.1];
    let on_top = [1.5, 2.5, 4.0];
    let p = [0, 1, 2].map(|k| on_top[k] + 3.0 * r[k]);
    let hits = column(&l, &[p], r, &mut work).expect("hits");
    assert!(
        hits.iter()
            .any(|h| l.faces[h[1] as usize].face == FaceId(1))
    );
    assert_eq!(column(&l, &[p], [0.0; 3], &mut work), Ok(Vec::new()));
}
