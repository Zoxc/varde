//! Invariants 5 to 7 on the primitives: each passes, and each defect
//! seeded into one is caught by the check that names it (a seam bound
//! that certifies nothing, tolerances over their targets, a vertex off
//! its faces, stale same-form edges and seam tags, crease labels of the
//! wrong sign or tangent on a crease, a trim on a singular side), or
//! refused as not built (a declared curvature-continuous join, a moved
//! join's target, a fitted surface); seams taken off their form pass with
//! their certified tangent labels.

use super::*;
use crate::build::primitives::{Primitive, build};
use crate::geom::fit_types::{ImportRef, Intent};
use crate::infra::budget::{Budget, Work};
use crate::infra::ops::{FeatureId, OpCtx, Tolerance, Warning, WarningKind};
use crate::topo::brep::{Fit, SurfId};
use crate::topo::check::topo::tests::{Passing, failure};
use crate::topo::check::{Changed, CheckError, CheckFailure, Inv, Invariants, Scope, run};

/// Invariants 5 to 7, the others passing.
fn table() -> Invariants<'static> {
    let p: &dyn Invariant = &Passing;
    [p, p, p, p, &Seams, &Regularity, &Forms, p, p, p]
}

fn check_with(b: &Brep, scope: Scope<'_>) -> Result<(), CheckError> {
    let work = &mut Work::new(&Budget::DEFAULT);
    run(b, scope, None, &table(), work, 0)
}

/// Invariants 5 to 7 on `b` over `scope`, the same on one thread and on
/// several.
fn check_in(b: &Brep, scope: Scope<'_>) -> Result<(), CheckError> {
    #[cfg(not(target_arch = "wasm32"))]
    return crate::infra::par::assert_deterministic(|| check_with(b, scope));
    #[cfg(target_arch = "wasm32")]
    return check_with(b, scope);
}

fn check(b: &Brep) -> Result<(), CheckError> {
    check_in(b, Scope::Full)
}

/// The failure of `b`'s check: its invariant, check and finding.
fn fails(b: &Brep) -> (Inv, u32, Finding) {
    let f: CheckFailure = failure(check(b));
    (f.inv, f.check, f.finding)
}

fn eps() -> f64 {
    Tolerance::DEFAULT.eps()
}

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

fn sphere() -> Brep {
    built(&Primitive::Sphere {
        centre: [10.0, -20.0, 5.0],
        radius: 7.0,
    })
}

/// One of each kind.
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
    let mut out = vec![cuboid(), cylinder(), sphere()];
    for p in [
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

/// The first edge of `b` of kind `kind`.
fn edge_of(b: &Brep, kind: impl Fn(EdgeKind) -> bool) -> EdgeId {
    (0..b.edge_count() as u32)
        .map(EdgeId)
        .find(|&e| kind(b.edge_kind(e)))
        .expect("an edge of the kind")
}

/// A crease of `b`.
fn crease_of(b: &Brep) -> EdgeId {
    edge_of(b, |k| k == EdgeKind::Regular)
}

#[test]
fn the_primitives_pass() {
    for b in all() {
        assert_eq!(check(&b), Ok(()));
    }
}

/// A seam bound refined again from the stored geometry must come out
/// within itself: zero certifies nothing.
#[test]
fn a_seam_bound_that_certifies_nothing_is_caught() {
    for mut b in [cuboid(), sphere()] {
        let c = b.edge_coedge(edge_of(&b, |_| true));
        b.coedges.seam_bound[c.index()] = 0.0;
        let f = failure(check(&b));
        assert_eq!(
            (f.inv, f.check, f.item),
            (Inv::Seams, SeamCheck::SeamBound as u32, Item::Coedge(c))
        );
    }
}

#[test]
fn a_seam_bound_over_its_edge_tolerance_is_caught() {
    let mut b = cylinder();
    let c = CoedgeId(0);
    let tol = b.edge_tol(b.edge(c));
    b.coedges.seam_bound[c.index()] = tol * 2.0;
    assert_eq!(
        fails(&b),
        (Inv::Seams, SeamCheck::SeamTol as u32, Finding::Defect)
    );
}

/// A new edge's tolerance is within its origin's target unless warned,
/// and every edge's within the body's `max_tol`.
#[test]
fn tolerances_over_their_targets_are_caught() {
    let wide = 0.3 * eps();
    let widened = |tol: f64| {
        let mut b = cuboid();
        let e = crease_of(&b);
        b.edges.tol[e.index()] = tol;
        for v in b.ends(e) {
            b.verts.tol[v.index()] = tol;
        }
        (b, e)
    };
    let (b, _) = widened(wide);
    assert_eq!(
        fails(&b),
        (Inv::Seams, SeamCheck::Target as u32, Finding::Defect)
    );
    // Within its target as a near cluster's edge.
    let (mut near, e) = widened(wide);
    near.edges.origin[e.index()] =
        Origin::Plain(PlainOrigin::Cluster(crate::topo::brep::ClusterOrigin {
            kind: ClusterKind::Near,
            sa: SurfId(0),
            sb: SurfId(1),
            sigma: varde_decide::num::decide::Sign::Pos,
        }));
    assert_eq!(check(&near), Ok(()));
    // Warned with its bound.
    let (mut warned, _) = widened(wide);
    let w = Warning::new(WarningKind::Accuracy, wide).expect("finite");
    warned.status_mut().op_mut().warn(w);
    assert_eq!(check(&warned), Ok(()));
    // Over `max_tol`, warned or not.
    let (mut over, _) = widened(eps());
    let w = Warning::new(WarningKind::Accuracy, eps()).expect("finite");
    over.status_mut().op_mut().warn(w);
    assert_eq!(
        fails(&over),
        (Inv::Seams, SeamCheck::MaxTol as u32, Finding::Defect)
    );
    // A moved join's target isn't built.
    let (mut moved, e) = widened(0.0);
    let origin = moved.edges.origin[e.index()].clone();
    moved.edges.origin[e.index()] = Origin::MovedJoin {
        prior: origin.prior(),
        clusters: Vec::new(),
    };
    moved.edges.tol[e.index()] = cuboid().edge_tol(e);
    assert_eq!(
        fails(&moved),
        (Inv::Seams, SeamCheck::Target as u32, Finding::Unbuilt)
    );
}

/// A scoped check tests the targets of the changed edges only: the
/// others keep their tolerances.
#[test]
fn a_scoped_check_tests_the_targets_of_new_edges() {
    let mut b = cuboid();
    let e = crease_of(&b);
    let wide = 0.3 * eps();
    b.edges.tol[e.index()] = wide;
    for v in b.ends(e) {
        b.verts.tol[v.index()] = wide;
    }
    let face = b.face(b.edge_coedge(e));
    let kept = Changed {
        faces: vec![face],
        ..Changed::default()
    };
    assert_eq!(check_in(&b, Scope::Changed(&kept)), Ok(()));
    let new = Changed {
        faces: vec![face],
        edges: vec![e],
        ..Changed::default()
    };
    let f = failure(check_in(&b, Scope::Changed(&new)));
    assert_eq!(
        (f.inv, f.check, f.item),
        (Inv::Seams, SeamCheck::Target as u32, Item::Edge(e))
    );
}

/// A vertex's tolerance covers its edges', and its faces pass within it:
/// moved off them it is caught, unless its tolerance covers the move.
#[test]
fn a_vertex_off_its_faces_is_caught() {
    let mut b = sphere();
    let v = VertId(0);
    b.verts.tol[v.index()] = 0.0;
    let f = failure(check(&b));
    assert_eq!(
        (f.inv, f.check, f.item),
        (Inv::Seams, SeamCheck::VertTol as u32, Item::Vert(v))
    );
    for (tol, ok) in [(1e-5, true), (1e-7, false)] {
        let mut b = cuboid();
        b.verts.pos[v.index()][0] += 1e-6;
        b.verts.tol[v.index()] = tol;
        let r = check(&b);
        if ok {
            assert_eq!(r, Ok(()));
        } else {
            let f = failure(r);
            assert_eq!(
                (f.inv, f.check, f.item),
                (Inv::Seams, SeamCheck::Ball as u32, Item::Vert(v))
            );
        }
    }
}

/// An edge of the same form, or tagged `Seam`, between faces of
/// different form keys is caught.
#[test]
fn stale_same_form_edges_and_seam_tags_are_caught() {
    let mut b = cuboid();
    let e = crease_of(&b);
    b.edges.kind[e.index()] = EdgeKind::SameForm { touch: false };
    let f = failure(check(&b));
    assert_eq!(
        (f.inv, f.check, f.item),
        (
            Inv::Regularity,
            RegularityCheck::SameForm as u32,
            Item::Edge(e)
        )
    );
    let mut b = cuboid();
    b.edges.tag[e.index()] = Some(EdgeTag::Seam);
    let f = failure(check(&b));
    assert_eq!(
        (f.inv, f.check, f.item),
        (
            Inv::Regularity,
            RegularityCheck::SeamTag as u32,
            Item::Edge(e)
        )
    );
}

/// A crease labelled of the wrong sign or tangent is caught; a mixed one
/// claims nothing; declared curvature-continuous joins aren't built.
#[test]
fn crease_labels_are_certified() {
    use RegularityCheck as R;
    for b in [cuboid(), cylinder()] {
        let e = crease_of(&b);
        let with = |cont: Continuity| {
            let mut b = b.clone();
            b.edges.cont[e.index()] = cont;
            b
        };
        let f = failure(check(&with(Continuity::Crease(Convexity::Concave))));
        assert_eq!(
            (f.inv, f.check, f.finding, f.item),
            (
                Inv::Regularity,
                R::Crease as u32,
                Finding::Defect,
                Item::Edge(e)
            )
        );
        assert_eq!(check(&with(Continuity::Crease(Convexity::Mixed))), Ok(()));
        let tangent = |angle| with(Continuity::Tangent { angle });
        for angle in [1e-3, THETA_TAN, 0.1] {
            assert_eq!(
                fails(&tangent(angle)),
                (Inv::Regularity, R::Tangent as u32, Finding::Defect)
            );
        }
        assert_eq!(
            fails(&tangent(-1.0)),
            (Inv::Regularity, R::Tangent as u32, Finding::Defect)
        );
        assert_eq!(
            fails(&with(Continuity::Curvature)),
            (Inv::Regularity, R::Curvature as u32, Finding::Unbuilt)
        );
    }
}

/// The cylinder's seams taken off their form: their faces are tangent,
/// and the labels the continuity certifies pass.
#[test]
fn certified_tangent_labels_pass() {
    use crate::infra::budget::Pass;
    let mut b = cylinder();
    let seams: Vec<usize> = (0..b.edge_count())
        .filter(|&e| matches!(b.edges.kind[e], EdgeKind::SameForm { .. }))
        .collect();
    for &e in &seams {
        b.edges.kind[e] = EdgeKind::Regular;
        b.edges.tag[e] = None;
    }
    let edges: Vec<EdgeId> = (0..b.edge_count() as u32).map(EdgeId).collect();
    let mut work = Work::new(&Budget::DEFAULT);
    let pass = Pass {
        stage: "continuity",
        rung: 0,
    };
    let labels = work
        .pass(pass, &edges, |&e, ledger| {
            crate::topo::continuity::certify::<()>(&b, e, ledger)
        })
        .expect("certified");
    b.edges.cont = labels;
    assert!(
        seams
            .iter()
            .all(|&e| matches!(b.edges.cont[e], Continuity::Tangent { .. }))
    );
    assert_eq!(check(&b), Ok(()));
}

/// The box turned inside out (every face's normal and every coedge's
/// direction reversed): its edges are concave creases, and labelled so
/// they pass.
#[test]
fn an_inside_out_box_has_concave_creases() {
    let mut b = cuboid();
    for f in b.faces.flipped.iter_mut() {
        *f = !*f;
    }
    for r in b.coedges.reversed.iter_mut() {
        *r = !*r;
    }
    let f = failure(check(&b));
    assert_eq!(
        (f.inv, f.check),
        (Inv::Regularity, RegularityCheck::Crease as u32)
    );
    for c in b.edges.cont.iter_mut() {
        *c = Continuity::Crease(Convexity::Concave);
    }
    assert_eq!(check(&b), Ok(()));
}

/// A pole's edge that isn't singular is a trim on the singular side.
#[test]
fn a_trim_on_a_singular_side_is_caught() {
    let mut b = sphere();
    let e = edge_of(&b, |k| k == EdgeKind::Singular);
    b.edges.kind[e.index()] = EdgeKind::Regular;
    let f = failure(check(&b));
    assert_eq!(
        (f.inv, f.check, f.finding),
        (
            Inv::Regularity,
            RegularityCheck::SingularSide as u32,
            Finding::Defect
        )
    );
}

/// A face on a fitted surface needs the fits' certificates.
#[test]
fn a_fitted_surface_is_not_built() {
    let mut b = cylinder();
    b.surfaces.fit[0] = Some(Fit {
        intent: Intent::Import {
            entity: ImportRef(1),
        },
        dev: varde_decide::num::bounded::Bounded::exact(0.0),
        recheckable: false,
    });
    assert_eq!(
        fails(&b),
        (Inv::Forms, FormCheck::Fit as u32, Finding::Unbuilt)
    );
}
