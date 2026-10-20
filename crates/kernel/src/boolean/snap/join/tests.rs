use std::collections::BTreeMap;

use varde_decide::ids::CoedgeId;

use super::*;
use crate::infra::budget::Budget;
use crate::topo::brep_min::{BodyKind, Brep};

type Org = Origin<&'static str, u32>;

/// The seam bounds' target of the fixture body.
const TARGET: f64 = 2.5e-6;
const EPS_SEAM: f64 = 1e-6;
/// How far the snaps move the bar's bottom corners, down.
const DROP: f64 = 1e-3;
const B_K: f64 = 2e-3;

fn work() -> Work {
    Work::new(&Budget::DEFAULT)
}

/// A bar `10 × 1 × 1`, its bottom corners `0…3` at `z = 0` (`0`, `3` at
/// `x = 0`; `1`, `2` at `x = 10`), its top corners `4…7` above them.
fn bar() -> Brep {
    let verts = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [10.0, 0.0, 1.0],
        [10.0, 1.0, 1.0],
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

/// The bar's edge between corners `a` and `b`.
fn edge(b: &Brep, a: u32, c: u32) -> EdgeId {
    let want = [VertId(a.min(c)), VertId(a.max(c))];
    (0..b.edge_count() as u32)
        .map(EdgeId)
        .find(|&e| b.ends(e) == want)
        .unwrap()
}

/// An edge's curve and p-curves, the curve split at `cts` and the
/// p-curves at `pts`.
struct Parts {
    curve: Curve3D,
    pcurves: Vec<(CoedgeId, PCurve)>,
}

struct Fixture {
    b: Brep,
    parts: BTreeMap<EdgeId, Parts>,
}

impl Fixture {
    /// The bar, the edges `splits` name split (curve knots, p-curve
    /// knots).
    fn new(splits: &[([u32; 2], &[f64], &[f64])]) -> Fixture {
        let b = bar();
        let mut parts = BTreeMap::new();
        for e in (0..b.edge_count() as u32).map(EdgeId) {
            let (cts, pts) = splits
                .iter()
                .find(|s| edge(&b, s.0[0], s.0[1]) == e)
                .map_or((&[][..], &[][..]), |s| (s.1, s.2));
            let c0 = b.edge_coedge(e);
            let pcurves = [c0, b.pair(c0)]
                .into_iter()
                .map(|c| (c, b.pcurve(c).split(pts).unwrap()))
                .collect();
            parts.insert(
                e,
                Parts {
                    curve: b.curve(e).split(cts).unwrap(),
                    pcurves,
                },
            );
        }
        Fixture { b, parts }
    }

    /// Runs `f` on the join input of every edge, with origins `origin`.
    fn with<R>(
        &self,
        origin: impl Fn(EdgeId) -> Org,
        f: impl FnOnce(&JoinInput<'_, &'static str, u32>) -> R,
    ) -> R {
        let b = &self.b;
        let cos: Vec<(EdgeId, Vec<JoinCoedge<'_>>)> = self
            .parts
            .iter()
            .map(|(&e, p)| {
                let cs = p
                    .pcurves
                    .iter()
                    .map(|(c, pc)| JoinCoedge {
                        face: b.face(*c),
                        surface: b.surface(b.face(*c)),
                        pcurve: pc,
                        seam: b.seam_bound(*c),
                    })
                    .collect();
                (e, cs)
            })
            .collect();
        let curves: Vec<JoinCurve<'_>> = self
            .parts
            .iter()
            .map(|(e, p)| JoinCurve {
                id: CurveId(e.0),
                curve: &p.curve,
            })
            .collect();
        let edges: Vec<JoinEdge<'_, &'static str, u32>> = cos
            .iter()
            .map(|(e, cs)| JoinEdge {
                edge: *e,
                curve: CurveId(e.0),
                range: b.t_range(*e),
                ends: b.ends(*e),
                tol: b.edge_tol(*e),
                origin: origin(*e),
                coedges: cs,
            })
            .collect();
        f(&JoinInput {
            curves: &curves,
            edges: &edges,
            eps_seam: EPS_SEAM,
        })
    }
}

fn plain(_: EdgeId) -> Org {
    Origin::Plain("exact")
}

/// Corner `v` of the bar dropped by `DROP`.
fn dropped(b: &Brep, v: u32, r_on: Option<f64>) -> MovedVert {
    let p = b.pos(VertId(v));
    MovedVert {
        vert: VertId(v),
        pos: [p[0], p[1], p[2] - DROP],
        r_on,
    }
}

fn run_join(
    fx: &Fixture,
    clusters: &[ClusterMoves<'_, u32>],
    origin: impl Fn(EdgeId) -> Org,
) -> Result<Joined<&'static str, u32>, JoinError> {
    fx.with(origin, |input| join(clusters, input, &mut work()))
}

fn pieces_of<'a>(
    out: &'a Joined<&'static str, u32>,
    e: EdgeId,
) -> Vec<&'a Piece<&'static str, u32>> {
    out.pieces.iter().filter(|p| p.edge == e).collect()
}

fn bits(c: &Curve3D) -> Vec<u64> {
    c.spans()
        .iter()
        .flatten()
        .flatten()
        .map(|x| x.to_bits())
        .collect()
}

#[test]
fn clusters_commute() {
    // Two clusters at the bar's two ends, joined by its long bottom
    // edges: one pass, the same bits in either order, both clusters on
    // the edges between them.
    let fx = Fixture::new(&[]);
    let b = &fx.b;
    let left = [dropped(b, 0, None), dropped(b, 3, None)];
    let right = [dropped(b, 1, None), dropped(b, 2, Some(3e-3))];
    let k1 = ClusterMoves {
        cluster: 1,
        b_k: B_K,
        moved: &left,
    };
    let k2 = ClusterMoves {
        cluster: 2,
        b_k: B_K,
        moved: &right,
    };
    let one = run_join(&fx, &[k1, k2], plain).unwrap();
    let two = run_join(&fx, &[k2, k1], plain).unwrap();
    // Bit for bit, `−0` against `+0` included.
    assert_eq!(format!("{one:?}"), format!("{two:?}"));
    assert_eq!(one, two);
    // Every edge at a bottom corner is refit whole (one span each).
    assert_eq!(one.pieces.len(), 8);
    assert_eq!(one.curves.len(), 8);
    assert!(one.junctions.is_empty());
    for (a, c, ks) in [
        (0, 1, vec![1, 2]),
        (3, 2, vec![1, 2]),
        (0, 3, vec![1]),
        (1, 2, vec![2]),
        (0, 4, vec![1]),
        (2, 6, vec![2]),
    ] {
        let e = edge(b, a, c);
        let ps = pieces_of(&one, e);
        assert_eq!(ps.len(), 1);
        let p = ps[0];
        assert!(p.refit);
        assert_eq!(
            p.origin,
            Origin::MovedJoin {
                prior: "exact",
                clusters: ks
            }
        );
        // The seams grow by the drop at the moved ends, inside the target.
        assert!(p.tol >= DROP && p.tol <= p.target, "{p:?}");
        let curve = &one
            .curves
            .iter()
            .find(|c| c.id == CurveId(e.0))
            .unwrap()
            .curve;
        let [v0, v1] = b.ends(e);
        assert_eq!(curve.knot_point(0).unwrap()[2], b.pos(v0)[2] - DROP);
        let end = curve.knots().len() - 1;
        let z1 = curve.knot_point(end).unwrap()[2];
        assert_eq!(z1, if v1.0 < 4 { -DROP } else { 1.0 });
    }
    // Corner 2's placement raises its edges' targets there.
    let p = pieces_of(&one, edge(b, 2, 6))[0];
    assert!(p.target >= 3e-3 + EPS_SEAM);
    let p = pieces_of(&one, edge(b, 0, 4))[0];
    assert!(p.target < 3e-3);
    // Edges at no moved corner aren't listed.
    assert!(pieces_of(&one, edge(b, 4, 5)).is_empty());
}

#[test]
fn split_where_the_refit_meets_the_rest() {
    // The long bottom edge in three spans, its p-curves alike.
    let ts: &[f64] = &[0.25, 0.75];
    let fx = Fixture::new(&[([0, 1], ts, ts)]);
    let b = &fx.b;
    let e = edge(b, 0, 1);
    let input = &fx.parts[&e].curve;
    let left = [dropped(b, 0, None)];
    let k1 = ClusterMoves {
        cluster: 1,
        b_k: B_K,
        moved: &left,
    };
    let out = run_join(&fx, &[k1], plain).unwrap();
    let ps = pieces_of(&out, e);
    assert_eq!(ps.len(), 2);
    let junction = Junction {
        edge: e,
        near: Near::Start,
    };
    assert_eq!(ps[0].t, [0.0, 0.25]);
    assert!(ps[0].refit);
    assert_eq!(
        ps[0].ends,
        [PieceEnd::Vert(VertId(0)), PieceEnd::Junction(junction)]
    );
    assert_eq!(
        ps[0].origin,
        Origin::MovedJoin {
            prior: "exact",
            clusters: vec![1]
        }
    );
    // The rest keeps its origin, tolerance and bits.
    assert_eq!(ps[1].t, [0.25, 1.0]);
    assert!(!ps[1].refit);
    assert_eq!(ps[1].origin, Origin::Plain("exact"));
    assert_eq!(ps[1].tol, b.edge_tol(e));
    let refit = &out
        .curves
        .iter()
        .find(|c| c.id == CurveId(e.0))
        .unwrap()
        .curve;
    assert_eq!(
        bits(&refit.slice(0.25, 1.0).unwrap()),
        bits(&input.slice(0.25, 1.0).unwrap())
    );
    for (c, p) in ps[1].coedges.iter().zip(&fx.parts[&e].pcurves) {
        assert_eq!(c.pcurve, p.1.slice(0.25, 1.0).unwrap());
    }
    // The junction at the knot, the same bits on both pieces' curves.
    assert_eq!(out.junctions.len(), 1);
    let j = out.junctions[0];
    assert_eq!(j.key, junction);
    assert_eq!(j.on, (CurveId(e.0), 0.25));
    assert_eq!(bits3(j.pos), bits3(input.knot_point(1).unwrap()));
    assert_eq!(bits3(refit.knot_point(1).unwrap()), bits3(j.pos));
    // The refit span starts at the moved corner.
    assert_eq!(bits3(refit.knot_point(0).unwrap()), bits3(left[0].pos));

    // Both ends moved: refit, rest, refit.
    let right = [dropped(b, 1, None)];
    let k2 = ClusterMoves {
        cluster: 2,
        b_k: B_K,
        moved: &right,
    };
    let out = run_join(&fx, &[k2, k1], plain).unwrap();
    let ps = pieces_of(&out, e);
    let shape: Vec<([f64; 2], bool)> = ps.iter().map(|p| (p.t, p.refit)).collect();
    assert_eq!(
        shape,
        [
            ([0.0, 0.25], true),
            ([0.25, 0.75], false),
            ([0.75, 1.0], true)
        ]
    );
    assert_eq!(
        ps[2].ends,
        [
            PieceEnd::Junction(Junction {
                edge: e,
                near: Near::End
            }),
            PieceEnd::Vert(VertId(1))
        ]
    );
    // Each refit piece names the cluster that moved its end only.
    assert_eq!(
        ps[2].origin,
        Origin::MovedJoin {
            prior: "exact",
            clusters: vec![2]
        }
    );
    assert_eq!(out.junctions.len(), 2);
}

#[test]
fn refit_runs_to_a_knot_the_p_curves_share() {
    // The curve split at 0.25 and 0.75, the p-curves at 0.75 only: the
    // refit runs from the moved end to 0.75.
    let fx = Fixture::new(&[([0, 1], &[0.25, 0.75], &[0.75])]);
    let b = &fx.b;
    let e = edge(b, 0, 1);
    let left = [dropped(b, 0, None)];
    let k1 = ClusterMoves {
        cluster: 1,
        b_k: B_K,
        moved: &left,
    };
    let out = run_join(&fx, &[k1], plain).unwrap();
    let shape: Vec<([f64; 2], bool)> = pieces_of(&out, e).iter().map(|p| (p.t, p.refit)).collect();
    assert_eq!(shape, [([0.0, 0.75], true), ([0.75, 1.0], false)]);
    // The inner knot moves by two thirds of the drop, rounded once.
    let refit = &out
        .curves
        .iter()
        .find(|c| c.id == CurveId(e.0))
        .unwrap()
        .curve;
    let z = refit.knot_point(1).unwrap()[2];
    assert!((z + DROP * (2.0 / 3.0)).abs() < 1e-15, "{z}");

    // No shared inner knot: one end moved refits the whole edge.
    let fx = Fixture::new(&[([0, 1], &[0.25, 0.75], &[])]);
    let out = run_join(&fx, &[k1], plain).unwrap();
    let shape: Vec<([f64; 2], bool)> = pieces_of(&out, e).iter().map(|p| (p.t, p.refit)).collect();
    assert_eq!(shape, [([0.0, 1.0], true)]);
    let refit = &out
        .curves
        .iter()
        .find(|c| c.id == CurveId(e.0))
        .unwrap()
        .curve;
    assert_eq!(bits3(refit.knot_point(3).unwrap()), bits3(b.pos(VertId(1))));
}

#[test]
fn refit_beyond_its_target_fails_unless_pinned() {
    let fx = Fixture::new(&[]);
    let b = &fx.b;
    // The drop is ten times `b_K`.
    let moved = [dropped(b, 0, None)];
    let k = ClusterMoves {
        cluster: 1,
        b_k: DROP / 10.0,
        moved: &moved,
    };
    assert!(matches!(
        run_join(&fx, &[k], plain),
        Err(JoinError::Fit { part: 0, .. })
    ));
    // A corner-rule placement with `R_on` above the drop: vertex-local.
    let moved = [dropped(b, 0, Some(2.0 * DROP))];
    let k = ClusterMoves {
        cluster: 1,
        b_k: DROP / 10.0,
        moved: &moved,
    };
    let out = run_join(&fx, &[k], plain).unwrap();
    for p in &out.pieces {
        assert!(p.tol <= p.target && p.target >= 2.0 * DROP + EPS_SEAM);
    }
}

#[test]
fn moved_join_keeps_its_prior_and_adds_clusters() {
    let fx = Fixture::new(&[]);
    let b = &fx.b;
    let e = edge(b, 0, 1);
    let moved = [dropped(b, 0, None)];
    let k = ClusterMoves {
        cluster: 5,
        b_k: B_K,
        moved: &moved,
    };
    let earlier = |g: EdgeId| {
        if g == e {
            Origin::MovedJoin {
                prior: "simple",
                clusters: vec![7, 3],
            }
        } else {
            Origin::Plain("exact")
        }
    };
    let out = run_join(&fx, &[k], earlier).unwrap();
    assert_eq!(
        pieces_of(&out, e)[0].origin,
        Origin::MovedJoin {
            prior: "simple",
            clusters: vec![3, 5, 7]
        }
    );
}

#[test]
fn one_vertex_moved_by_two_clusters() {
    let fx = Fixture::new(&[]);
    let b = &fx.b;
    let a = [dropped(b, 0, None)];
    let mut other = a;
    other[0].pos[0] += 1e-4;
    let k = |c, m| ClusterMoves {
        cluster: c,
        b_k: B_K,
        moved: m,
    };
    assert_eq!(
        run_join(&fx, &[k(1, &a), k(2, &other)], plain),
        Err(JoinError::Moved(VertId(0)))
    );
    // To one point: both clusters recorded.
    let out = run_join(&fx, &[k(2, &a), k(1, &a)], plain).unwrap();
    assert_eq!(
        pieces_of(&out, edge(b, 0, 4))[0].origin,
        Origin::MovedJoin {
            prior: "exact",
            clusters: vec![1, 2]
        }
    );
    // Two clusters with one key.
    assert_eq!(
        run_join(&fx, &[k(1, &a), k(1, &a)], plain),
        Err(JoinError::Input)
    );
}

#[test]
fn edges_sharing_a_curve_refit_once() {
    let fx = Fixture::new(&[]);
    let b = &fx.b;
    let e = edge(b, 0, 1);
    let twin = EdgeId(100);
    let parts = &fx.parts[&e];
    let cos: Vec<JoinCoedge<'_>> = parts
        .pcurves
        .iter()
        .map(|(c, pc)| JoinCoedge {
            face: b.face(*c),
            surface: b.surface(b.face(*c)),
            pcurve: pc,
            seam: b.seam_bound(*c),
        })
        .collect();
    let curves = [JoinCurve {
        id: CurveId(9),
        curve: &parts.curve,
    }];
    let make = |edge: EdgeId, ends: [VertId; 2]| JoinEdge {
        edge,
        curve: CurveId(9),
        range: b.t_range(e),
        ends,
        tol: b.edge_tol(e),
        origin: Origin::Plain("exact"),
        coedges: &cos,
    };
    let edges = [make(e, b.ends(e)), make(twin, [VertId(100), VertId(101)])];
    let input = JoinInput {
        curves: &curves,
        edges: &edges,
        eps_seam: EPS_SEAM,
    };
    let mut moved = vec![dropped(b, 0, None)];
    let k = ClusterMoves {
        cluster: 1,
        b_k: B_K,
        moved: &moved,
    };
    // The twin's end there kept: the curve can't move under it.
    assert_eq!(
        join(&[k], &input, &mut work()),
        Err(JoinError::Shared(CurveId(9)))
    );
    // Both moved alike: one refit curve, both edges on it.
    moved.push(MovedVert {
        vert: VertId(100),
        ..moved[0]
    });
    let k = ClusterMoves {
        cluster: 1,
        b_k: B_K,
        moved: &moved,
    };
    let out = join(&[k], &input, &mut work()).unwrap();
    assert_eq!(out.curves.len(), 1);
    assert_eq!(out.pieces.len(), 2);
    assert_eq!(out.pieces[0].t, out.pieces[1].t);
    assert_eq!(out.pieces[0].tol, out.pieces[1].tol);
}

#[test]
fn runs_out_of_budget() {
    let fx = Fixture::new(&[]);
    let moved = [dropped(&fx.b, 0, None)];
    let k = ClusterMoves {
        cluster: 1,
        b_k: B_K,
        moved: &moved,
    };
    let out = fx.with(plain, |input| {
        join(&[k], input, &mut Work::new(&Budget::new(20)))
    });
    assert!(matches!(out, Err(JoinError::OutOfBudget(_))));
}
