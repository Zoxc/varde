//! A unit cube cut through its edges: by `x = 2z` through the edge
//! `{x = 0, z = 0}`, and by the diagonal `x = y` through two of its
//! vertical edges, where exactly those edges' coedges lie in the cut;
//! an edge refit to a moved end vertex whose faces kept their p-curves,
//! in the cut by its traces (as Q and as iso spans) though not by its
//! curve; the face's own form, a surface that isn't a plane, the
//! first-tier rule across an edge; which plane tiles are affine maps;
//! the answers in any order of the asks;
//! an edge without spans refused; and running out of budget.

use super::*;
use crate::geom::form::CylinderKey;
use crate::infra::budget::Budget;
use crate::topo::brep_min::{BodyKind, EdgeId, VertId};

const TARGET: f64 = 2.5e-6;

fn work() -> Work {
    Work::new(&Budget::DEFAULT)
}

/// The cube `[0, 1]³`: its bottom corners `0…3` at `z = 0` (`0` at the
/// origin, `1` on `x`, `3` on `y`), its top corners `4…7` above them.
fn cube() -> Brep {
    let verts = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ];
    let faces = vec![
        vec![0, 3, 2, 1],
        vec![4, 5, 6, 7],
        vec![0, 1, 5, 4],
        vec![1, 2, 6, 5],
        vec![2, 3, 7, 6],
        vec![3, 0, 4, 7],
    ];
    Brep::from_polygons(BodyKind::Solid, &verts, &faces, TARGET, &mut work()).unwrap()
}

/// The cube's edge between corners `a` and `b`.
fn edge(b: &Brep, a: u32, c: u32) -> EdgeId {
    let want = [VertId(a.min(c)), VertId(a.max(c))];
    (0..b.edge_count() as u32)
        .map(EdgeId)
        .find(|&e| b.ends(e) == want)
        .unwrap()
}

fn plane(normal: [f64; 3], point: [f64; 3]) -> FormKey {
    FormKey::Plane(PlaneKey::new(normal, point).unwrap())
}

fn key(b: &Brep, c: CoedgeId) -> &FormKey {
    b.surface(b.face(c)).lattice().key()
}

/// Every coedge of `b` against `s`, by coedge id.
fn all(b: &Brep, s: &FormKey) -> Vec<Option<Clause>> {
    let asks: Vec<Ask<'_>> = (0..b.coedge_count() as u32)
        .map(|c| Ask {
            coedge: Coedge::of(b, CoedgeId(c)),
            surface: s,
        })
        .collect();
    decide(&asks, &mut work()).unwrap()
}

/// The coedges whose record holds, with their clauses.
fn holding(b: &Brep, s: &FormKey) -> Vec<(EdgeId, Clause)> {
    all(b, s)
        .into_iter()
        .enumerate()
        .filter_map(|(c, r)| r.map(|r| (b.edge(CoedgeId(c as u32)), r)))
        .collect()
}

#[test]
fn a_cut_through_a_cube_edge_holds_on_both_its_coedges_only() {
    let b = cube();
    let cut = plane([1.0, 0.0, -2.0], [0.0, 0.0, 0.0]);
    let g = edge(&b, 0, 3);
    assert_eq!(holding(&b, &cut), vec![(g, Clause::Curve); 2]);
    // On each face of `g` the arc of the cut is a piece of `g`.
    let c = b.edge_coedge(g);
    for c in [c, b.pair(c)] {
        let across = key(&b, b.pair(c));
        assert_eq!(
            t0_coincident(&cut, Some(across), Some(Clause::Curve)),
            Some(T0::EdgeOn(Clause::Curve))
        );
    }
}

#[test]
fn a_diagonal_cut_holds_on_the_two_edges_it_contains() {
    let b = cube();
    let cut = plane([1.0, -1.0, 0.0], [0.0, 0.0, 0.0]);
    let mut got = holding(&b, &cut);
    got.sort_by_key(|&(e, _)| e.0);
    let (g, h) = (edge(&b, 0, 4), edge(&b, 2, 6));
    let mut want = vec![(g, Clause::Curve), (g, Clause::Curve)];
    want.extend([(h, Clause::Curve); 2]);
    want.sort_by_key(|&(e, _)| e.0);
    assert_eq!(got, want);
}

/// The edge `{0, 3}` refit to its end vertex `3` moved by `dx` along
/// `x`: a line from the origin to `(dx, 1, 0)`, its middle point the
/// rounded midpoint.
fn moved(dx: f64) -> Vec<Span3> {
    vec![[
        [0.0, 0.0, 0.0, 1.0],
        [0.5 * dx, 0.5, 0.0, 1.0],
        [dx, 1.0, 0.0, 1.0],
    ]]
}

#[test]
fn a_refit_edge_lies_in_the_cut_by_the_traces_its_faces_kept() {
    let b = cube();
    let cut = plane([1.0, 0.0, -2.0], [0.0, 0.0, 0.0]);
    let curve = moved(1.0 / 1024.0);
    let c = b.edge_coedge(edge(&b, 0, 3));
    let asks: Vec<Ask<'_>> = [c, b.pair(c)]
        .map(|c| Ask {
            coedge: Coedge {
                curve: &curve,
                ..Coedge::of(&b, c)
            },
            surface: &cut,
        })
        .to_vec();
    assert_eq!(
        decide(&asks, &mut work()).unwrap(),
        vec![Some(Clause::Trace); 2]
    );
    // A cut the trace leaves: `x = 2z` moved by the same amount.
    let off = plane([1.0, 0.0, -2.0], [1.0 / 1024.0, 0.0, 0.0]);
    let asks: Vec<Ask<'_>> = asks
        .iter()
        .map(|a| Ask {
            surface: &off,
            ..*a
        })
        .collect();
    assert_eq!(decide(&asks, &mut work()).unwrap(), vec![None; 2]);
}

#[test]
fn an_iso_trace_is_tested_at_its_control_points() {
    let b = cube();
    let cut = plane([1.0, 0.0, -2.0], [0.0, 0.0, 0.0]);
    let curve = moved(1.0 / 1024.0);
    let c = b.edge_coedge(edge(&b, 0, 3));
    // On the bottom face the edge runs along a row of fixed `v`.
    let c = if b.surface(b.face(c)).lattice().key() == &plane([0.0, 0.0, 1.0], [0.0; 3]) {
        c
    } else {
        b.pair(c)
    };
    let span = b.pcurve(c).spans()[0];
    let ([u0, v0], [u1, v1]) = (span.start(), span.end());
    assert_eq!(v0, v1);
    let iso = |at: f64| {
        PCurve::from_spans(
            vec![PSpan::Iso {
                fixed: Dir::V,
                at,
                m: [[u0, 1.0], [u1, 1.0]],
            }],
            b.pcurve(c).knots().to_vec(),
        )
        .unwrap()
    };
    let decide_on = |p: &PCurve| {
        let ask = Ask {
            coedge: Coedge {
                pcurve: p,
                curve: &curve,
                ..Coedge::of(&b, c)
            },
            surface: &cut,
        };
        decide(&[ask], &mut work()).unwrap()[0]
    };
    assert_eq!(decide_on(&iso(v0)), Some(Clause::Trace));
    // One row over, off the cut.
    assert_eq!(decide_on(&iso(v0 + f64::EPSILON)), None);
}

#[test]
fn the_face_own_form_holds_and_a_surface_not_a_plane_does_not() {
    let b = cube();
    let c = b.edge_coedge(edge(&b, 0, 3));
    let own = key(&b, c).clone();
    let ask = |s| Ask {
        coedge: Coedge::of(&b, c),
        surface: s,
    };
    assert_eq!(
        decide(&[ask(&own)], &mut work()).unwrap(),
        vec![Some(Clause::SameForm)]
    );
    // A cylinder about the edge's line holds it, but only planes are
    // tested here.
    let cyl = FormKey::Cylinder(CylinderKey::new([0.0; 3], [0.0, 1.0, 0.0], 1.0).unwrap());
    assert_eq!(decide(&[ask(&cyl)], &mut work()).unwrap(), vec![None]);
}

#[test]
fn an_arc_from_the_face_across_the_edge_is_coincident_across() {
    let b = cube();
    let c = b.edge_coedge(edge(&b, 0, 3));
    let across = key(&b, b.pair(c)).clone();
    let other = plane([1.0, 0.0, -2.0], [0.0; 3]);
    assert_eq!(
        t0_coincident(&across, Some(&across), Some(Clause::Curve)),
        Some(T0::Across)
    );
    assert_eq!(t0_coincident(&other, Some(&across), None), None);
    assert_eq!(t0_coincident(&other, None, None), None);
}

#[test]
fn the_answers_follow_the_asks_in_any_order() {
    let b = cube();
    let cut = plane([1.0, 0.0, -2.0], [0.0; 3]);
    let ids: Vec<CoedgeId> = (0..b.coedge_count() as u32).map(CoedgeId).collect();
    let ask = |c: CoedgeId| Ask {
        coedge: Coedge::of(&b, c),
        surface: &cut,
    };
    let forward = decide(
        &ids.iter().map(|&c| ask(c)).collect::<Vec<_>>(),
        &mut work(),
    )
    .unwrap();
    let back = decide(
        &ids.iter().rev().map(|&c| ask(c)).collect::<Vec<_>>(),
        &mut work(),
    )
    .unwrap();
    assert_eq!(forward, back.into_iter().rev().collect::<Vec<_>>());
}

#[test]
fn an_edge_without_spans_is_refused_at_its_ask() {
    let b = cube();
    let cut = plane([1.0, 0.0, -2.0], [0.0; 3]);
    let c = CoedgeId(0);
    let asks = [
        Ask {
            coedge: Coedge::of(&b, c),
            surface: &cut,
        },
        Ask {
            coedge: Coedge {
                curve: &[],
                ..Coedge::of(&b, c)
            },
            surface: &cut,
        },
    ];
    assert_eq!(
        decide(&asks, &mut work()),
        Err(EdgeOnError::Ask {
            ask: 1,
            fault: Fault::Empty
        })
    );
}

#[test]
fn running_out_of_budget_is_reported() {
    let b = cube();
    let cut = plane([1.0, 0.0, -2.0], [0.0; 3]);
    let asks: Vec<Ask<'_>> = (0..b.coedge_count() as u32)
        .map(|c| Ask {
            coedge: Coedge::of(&b, CoedgeId(c)),
            surface: &cut,
        })
        .collect();
    let got = decide(&asks, &mut Work::new(&Budget::new(3)));
    assert!(matches!(got, Err(EdgeOnError::OutOfBudget(_))), "{got:?}");
}

/// Whether the tile of the plane through `point` with `normal` is an
/// exact affine map.
fn affine(normal: [f64; 3], point: [f64; 3]) -> bool {
    let s = crate::geom::forms::plane::surface(PlaneKey::new(normal, point).unwrap()).unwrap();
    let pass = Pass {
        stage: "test",
        rung: 0,
    };
    work()
        .pass(pass, &[()], |_, ledger| {
            is_affine(&s.tiles()[0], &mut Meter { ledger, parts: 0 })
        })
        .map_err(|_: Halt<Fault>| ())
        .unwrap()[0]
}

#[test]
fn a_plane_tile_is_affine_where_its_net_rounded_to_a_parallelogram() {
    // Axis-aligned, and through the origin at a dyadic slope: exact.
    assert!(affine([0.0, 0.0, 1.0], [0.3, 0.1, 0.7]));
    assert!(affine([1.0, 0.0, -2.0], [0.0; 3]));
    // Oblique, off the origin: the net's points rounded apart.
    assert!(!affine([1.0, 0.0, -2.0], [0.3, 0.1, 0.7]));
    assert!(!affine([1.0, 2.0, 3.0], [0.0; 3]));
}
