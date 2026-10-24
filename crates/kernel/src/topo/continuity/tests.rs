//! The certified labels on the primitives (convex creases, seams of one
//! form, the singular edges), turned inside out, and a seam taken off its
//! form: tangent, its sign zero along it never settled.

use super::*;
use crate::build::primitives::{Primitive, build};
use crate::infra::budget::{Budget, Pass, Work};
use crate::infra::ops::{FeatureId, OpCtx, Tolerance};

fn built(p: &Primitive) -> Brep {
    let ctx = OpCtx::new(FeatureId(1));
    build(p, ctx, Tolerance::DEFAULT, &mut Work::new(&Budget::DEFAULT))
        .unwrap_or_else(|e| panic!("{p:?}: {e:?}"))
}

fn cuboid() -> Brep {
    built(&Primitive::Cuboid {
        min: [1.0, -2.0, 0.5],
        size: [3.0, 5.0, 6.75],
    })
}

fn cylinder() -> Brep {
    built(&Primitive::Cylinder {
        base: [1.5, -2.0, 3.0],
        radius: 4.0,
        height: 9.5,
    })
}

/// One of each kind, cones and frustums both ways up, a ring and a
/// spindle torus.
fn all() -> Vec<Brep> {
    let cone = |base, radii, height| Primitive::Cone {
        base,
        radii,
        height,
    };
    let torus = |major, minor| Primitive::Torus {
        centre: [3.0, 1.0, -2.0],
        major,
        minor,
    };
    let mut out = vec![cuboid(), cylinder()];
    for p in [
        Primitive::Sphere {
            centre: [10.0, -20.0, 5.0],
            radius: 7.0,
        },
        cone([1.0, 2.0, -3.0], [5.0, 0.0], 7.0),
        cone([1.0, 2.0, -3.0], [0.0, 5.0], 7.0),
        cone([-4.0, 0.25, 2.0], [3.0, 1.5], 2.5),
        cone([-4.0, 0.25, 2.0], [1.5, 3.0], 2.5),
        torus(6.0, 2.5),
        torus(2.0, 3.0),
    ] {
        out.push(built(&p));
    }
    out
}

/// Runs `f` on every edge of `b` in one pass, the same on one thread and
/// on several.
fn per_edge<R>(
    b: &Brep,
    f: impl Fn(EdgeId, &Join<'_>, &mut Ledger<'_>) -> R + Sync,
) -> Vec<Option<R>>
where
    R: Send + std::fmt::Debug,
{
    let edges: Vec<EdgeId> = (0..b.edge_count() as u32).map(EdgeId).collect();
    let run = || {
        let mut work = Work::new(&Budget::DEFAULT);
        let pass = Pass {
            stage: "continuity",
            rung: 0,
        };
        work.pass(pass, &edges, |&e, ledger| {
            let Some((coedges, faces)) = two_faces(b, e) else {
                return Ok(None);
            };
            let join = Join::new::<()>(b, e, coedges, faces, ledger)?;
            Ok(join.map(|join| f(e, &join, ledger)))
        })
        .unwrap_or_else(|e| panic!("{e:?}"))
    };
    #[cfg(not(target_arch = "wasm32"))]
    return crate::infra::par::assert_deterministic(run);
    #[cfg(target_arch = "wasm32")]
    return run();
}

/// Every edge's label, certified.
fn labels(b: &Brep) -> Vec<Continuity> {
    let edges: Vec<EdgeId> = (0..b.edge_count() as u32).map(EdgeId).collect();
    let run = || {
        let mut work = Work::new(&Budget::DEFAULT);
        let pass = Pass {
            stage: "continuity",
            rung: 0,
        };
        work.pass(pass, &edges, |&e, ledger| certify::<()>(b, e, ledger))
            .unwrap_or_else(|e| panic!("{e:?}"))
    };
    #[cfg(not(target_arch = "wasm32"))]
    return crate::infra::par::assert_deterministic(run);
    #[cfg(target_arch = "wasm32")]
    return run();
}

/// The certificate gives each primitive's edges the labels its
/// construction states: convex creases, curvature-continuous seams, mixed
/// singular edges.
#[test]
fn the_primitives_labels_are_certified() {
    let mut creases = 0;
    for b in all() {
        let stored: Vec<Continuity> = b.edges.cont.clone();
        assert_eq!(labels(&b), stored);
        creases += stored
            .iter()
            .filter(|&&c| c == Continuity::Crease(Convexity::Convex))
            .count();
    }
    assert!(creases > 0);
}

/// The box turned inside out (every face's normal and every coedge's
/// direction reversed) has concave creases.
#[test]
fn an_inside_out_box_has_concave_creases() {
    let mut b = cuboid();
    for f in b.faces.flipped.iter_mut() {
        *f = !*f;
    }
    for r in b.coedges.reversed.iter_mut() {
        *r = !*r;
    }
    for cont in labels(&b) {
        assert_eq!(cont, Continuity::Crease(Convexity::Concave));
    }
}

/// A crease is far from tangent: its first cell shows it, past the band.
#[test]
fn a_crease_is_not_tangent() {
    for b in [cuboid(), cylinder()] {
        let out = per_edge(&b, |_, join, ledger| {
            join.tangent::<()>(THETA_TAN, TAN_HALF_THETA, ledger)
        });
        for (e, t) in out.into_iter().enumerate() {
            if b.edges.kind[e] == EdgeKind::Regular {
                assert_eq!(t, Some(Ok(Tangency::Beyond)), "edge {e}");
            }
        }
    }
}

/// The cylinder's seams taken off their form: two faces tangent along
/// them, certified within `θ_tan` with the bound kept, a bound the
/// certificate reaches again as its target; their crease sign is zero
/// along the whole edge, never settled: mixed, the search ended.
#[test]
fn a_seam_off_its_form_is_tangent() {
    let mut b = cylinder();
    let seams: Vec<usize> = (0..b.edge_count())
        .filter(|&e| matches!(b.edges.kind[e], EdgeKind::SameForm { .. }))
        .collect();
    assert!(!seams.is_empty());
    for &e in &seams {
        b.edges.kind[e] = EdgeKind::Regular;
        b.edges.tag[e] = None;
    }
    let got = labels(&b);
    for &e in &seams {
        let Continuity::Tangent { angle } = got[e] else {
            panic!("edge {e}: {:?}", got[e]);
        };
        assert!((0.0..=THETA_TAN).contains(&angle), "edge {e}: {angle}");
    }
    // Again, and with the stored bound as the target and far threshold,
    // as the check reads it.
    let again = per_edge(&b, |e, join, ledger| {
        let Continuity::Tangent { angle } = got[e.index()] else {
            return None;
        };
        Some((
            join.tangent::<()>(THETA_TAN, TAN_HALF_THETA, ledger),
            join.tangent::<()>(angle, angle, ledger),
            join.find_crease::<()>(ledger),
        ))
    });
    for &e in &seams {
        let Continuity::Tangent { angle } = got[e] else {
            unreachable!()
        };
        let (first, stored, sign) = again[e].clone().flatten().expect("two faces");
        assert_eq!(first, Ok(Tangency::Within(angle)));
        assert_eq!(stored, Ok(Tangency::Within(angle)));
        assert_eq!(sign, Ok(Convexity::Mixed));
    }
}

/// The search for a crease's sign: cells of one sign give it; both signs
/// shown, by whole cells or at points, stop it mixed, and a cell that
/// shows neither is halved or climbs a level, never cut out of the edge.
#[test]
fn the_crease_search_stops_mixed_on_both_signs() {
    let pos = Signs {
        all_pos: true,
        end_pos: true,
        rounded: Some(true),
        ..Signs::default()
    };
    let neg = Signs {
        all_neg: true,
        end_neg: true,
        rounded: Some(false),
        ..Signs::default()
    };
    let open = Signs::default();
    let rounding = Signs {
        rounded: Some(true),
        ..Signs::default()
    };
    let point_neg = Signs {
        end_neg: true,
        ..Signs::default()
    };

    let mut seen = Seen::default();
    for s in [pos, pos] {
        assert!(matches!(seen.visit(Shown::Forms(s)), Visit::Done));
    }
    assert_eq!(seen.sign(), Convexity::Convex);
    assert!(matches!(seen.visit(Shown::Forms(neg)), Visit::Stop(())));

    let mut seen = Seen::default();
    assert!(matches!(seen.visit(Shown::Forms(neg)), Visit::Done));
    assert!(matches!(seen.visit(Shown::Forms(open)), Visit::Space));
    assert!(matches!(
        seen.visit(Shown::Forms(rounding)),
        Visit::Precision
    ));
    assert_eq!(seen.sign(), Convexity::Concave);

    let mut seen = Seen::default();
    assert!(matches!(seen.visit(Shown::Forms(pos)), Visit::Done));
    assert!(matches!(
        seen.visit(Shown::Forms(point_neg)),
        Visit::Stop(())
    ));

    for shown in [Shown::Folded, Shown::Degree] {
        assert!(matches!(Seen::default().visit(shown), Visit::Stop(())));
    }
    assert_eq!(Seen::default().sign(), Convexity::Mixed);
}
