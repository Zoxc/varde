use malachite::Rational;
use varde_decide::bezier::HomPatch;
use varde_decide::count::boxes::BoxKey;
use varde_decide::ids::FaceId;
use varde_decide::leaves::flat::BoxUse;
use varde_decide::leaves::flat::{self, FlatPiece, LeafWork, certify};
use varde_decide::leaves::near_quadric::{self, QuadricFace, SheetFace};
use varde_decide::num::bounded::Bounded;
use varde_decide::records::keys::Operand;
use varde_testkit::sheets::{self, SMOOTH_RADIUS};

use super::super::cluster::merge_sharing;
use super::super::cluster::tests::{
    B_K, ext, fixture_clusters, fixture_leaves, rat, rectangle, whole, work,
};
use super::*;

const NONE: [Candidate<'static>; 2] = [
    Candidate {
        singular: false,
        sheet: None,
    },
    Candidate {
        singular: false,
        sheet: None,
    },
];

fn q3(v: [i64; 3]) -> Q3 {
    v.map(Rational::from)
}

/// The flat leaves of `leaves` at positions `held`.
fn picked(leaves: &[flat::Leaf], held: impl Iterator<Item = u32>) -> Vec<&flat::Leaf> {
    held.map(|i| &leaves[i as usize]).collect()
}

/// A flush pair of the parallelogram `p0, p0 + u, p0 + v` on faces `fa` of
/// the first operand and `fb` of the second, facing along `u × v`:
/// certified Near.
fn flush(fa: u32, fb: u32, p0: [f64; 3], u: [f64; 3], v: [f64; 3]) -> flat::Leaf {
    let at = |s: f64, t: f64| [0, 1, 2].map(|k| p0[k] + s * u[k] + t * v[k]);
    let net = [at(0.0, 0.0), at(0.0, 1.0), at(1.0, 0.0), at(1.0, 1.0)];
    let p = FlatPiece::from_net(1, 1, &net, false).expect("a piece");
    let kind = certify(&p, &p, &rat(B_K), &mut LeafWork::default()).expect("certified");
    assert!(matches!(kind, flat::Kind::Near(_)), "{kind:?}");
    flat::Leaf {
        a: whole(Operand::A, fa, [0, 0]),
        b: whole(Operand::B, fb, [0, 0]),
        kind,
        world: [p.world_box(), p.world_box()],
    }
}

#[test]
fn a_flush_quad_on_the_l_projects_along_the_normal_onto_the_first_operand() {
    let pair = sheets::l_corner_quad(0.0);
    let ls = fixture_leaves(&pair, rectangle, 2);
    let cs = fixture_clusters(&pair, &ls);
    let [c] = &cs[..] else { panic!("{cs:?}") };
    let held = picked(&ls, c.leaves());
    let t = choose(c.kind(), &held, NONE, &mut work()).unwrap();
    let along = Target {
        operand: Operand::A,
        map: Map::Along(q3([0, 0, 1])),
    };
    assert_eq!(t, Some(along.clone()));
    // No leaf order is read.
    let mut rev = held.clone();
    rev.reverse();
    assert_eq!(
        choose(c.kind(), &rev, NONE, &mut work()).unwrap(),
        Some(along)
    );
    let mut rot = held.clone();
    rot.rotate_left(held.len() / 2);
    assert_eq!(choose(c.kind(), &rot, NONE, &mut work()).unwrap(), t);
    // A singular point on the first operand puts the second first.
    let singular_a = [
        Candidate {
            singular: true,
            sheet: None,
        },
        NONE[1],
    ];
    let t = choose(c.kind(), &held, singular_a, &mut work()).unwrap();
    assert_eq!(
        t,
        Some(Target {
            operand: Operand::B,
            map: Map::Along(q3([0, 0, 1])),
        })
    );
}

#[test]
fn a_common_direction_is_found_iff_one_exists() {
    let b_k = rat(B_K);
    // Two flush pairs facing up and towards `+y`, 10⁻⁴ wide: the max–min
    // direction of `(0, 0, 1)` and `(0, 1, 0)` is `(0, 1, 1)`, and on
    // pieces this small its gap stays within `b_K`.
    let w = 1e-4;
    let up = flush(0, 0, [0.0; 3], [w, 0.0, 0.0], [0.0, w, 0.0]);
    let side = flush(1, 1, [0.0, w, 0.0], [0.0, 0.0, w], [w, 0.0, 0.0]);
    let found = choose(Tangency::Near, &[&up, &side], NONE, &mut work()).unwrap();
    let along = Some(Target {
        operand: Operand::A,
        map: Map::Along(q3([0, 1, 1])),
    });
    assert_eq!(found, along);
    assert_eq!(
        choose(Tangency::Near, &[&side, &up], NONE, &mut work()).unwrap(),
        along
    );
    // No open hemisphere holds the normals (up and down): none.
    let down = flush(2, 2, [0.0, 0.0, -1.0], [0.0, w, 0.0], [w, 0.0, 0.0]);
    assert_eq!(
        choose(Tangency::Near, &[&up, &down], NONE, &mut work()).unwrap(),
        None
    );
    // 10⁻² wide, every direction the up pair admits is within 1/20 of
    // `z` and every one the side pair admits within 1/20 of `y`: no
    // common direction exists, though the max–min one is positive on
    // every normal; the cones' gap condition refuses it.
    let w = 1e-2;
    let up = flush(0, 0, [0.0; 3], [w, 0.0, 0.0], [0.0, w, 0.0]);
    let side = flush(1, 1, [0.0, w, 0.0], [0.0, 0.0, w], [w, 0.0, 0.0]);
    let ell = q3([0, 1, 1]);
    for l in [&up, &side] {
        let flat::Kind::Near(t) = &l.kind else {
            unreachable!()
        };
        assert!(t.cone.gens().iter().all(|g| dot(g, &ell) > 0u32));
        assert!(!t.cone.contains(&ell));
        assert_eq!(t.cone.b_k(), &b_k);
    }
    assert_eq!(
        choose(Tangency::Near, &[&up, &side], NONE, &mut work()).unwrap(),
        None
    );
}

#[test]
fn a_sheet_over_a_box_edge_never_takes_the_closest_point() {
    // The flush fold: a cluster on the box's front and one on its top,
    // merged (their windows share the fold edge). Each alone projects
    // along its normal; together the max–min direction `(0, −1, 1)`
    // leaves the cones (the pieces are wide), and a creased target has no
    // closest point: no candidate certifies, for the escalation.
    let pair = sheets::box_edge_fold(0.0);
    let ls = fixture_leaves(&pair, rectangle, 2);
    let cs = fixture_clusters(&pair, &ls);
    assert_eq!(cs.len(), 2);
    let normals = [q3([0, -1, 0]), q3([0, 0, 1])];
    for (c, n) in cs.iter().zip(&normals) {
        let held = picked(&ls, c.leaves());
        let t = choose(c.kind(), &held, NONE, &mut work()).unwrap();
        assert_eq!(
            t,
            Some(Target {
                operand: Operand::A,
                map: Map::Along(n.clone()),
            })
        );
    }
    let merged = merge_sharing(cs, &[vec![0u32], vec![0u32]], &mut work()).unwrap();
    let [m] = &merged[..] else {
        panic!("{merged:?}")
    };
    let held = picked(&ls, m.cluster.leaves());
    assert_eq!(
        choose(m.cluster.kind(), &held, NONE, &mut work()).unwrap(),
        None
    );
}

#[test]
fn leaves_of_another_kind_or_none_are_refused() {
    let w = 1e-4;
    let up = flush(0, 0, [0.0; 3], [w, 0.0, 0.0], [0.0, w, 0.0]);
    assert_eq!(
        choose(Tangency::Contact, &[&up], NONE, &mut work()),
        Err(TargetError::Kind(0))
    );
    let none: [&flat::Leaf; 0] = [];
    assert_eq!(
        choose(Tangency::Near, &none, NONE, &mut work()),
        Err(TargetError::Empty)
    );
    let mut same = up.clone();
    same.b = whole(Operand::A, 1, [0, 0]);
    assert_eq!(
        choose(Tangency::Near, &[&up, &same], NONE, &mut work()),
        Err(TargetError::Operands(1))
    );
}

// The smooth pair: the biquadratic sheet touching the cylinder of radius
// 50 about `y` at its top (gap 0), the cylinder's top half as two faces,
// angle cell −1 (from `−x` to `+z`, face 0) and cell 0 (`+z` to `+x`,
// face 1) of its lattice (`e₁ = +z`, `e₂ = +x`).

type S = Bounded<f64>;

fn like() -> S {
    Bounded::exact(0.0)
}

fn cylinder() -> Quadric {
    Quadric::cylinder([0.0; 3], [0.0, 1.0, 0.0], SMOOTH_RADIUS).expect("a cylinder")
}

/// The cylinder's tile of angle cell `k` over `y ∈ [−20, 20]`, its
/// `x_u × x_v` outward.
fn cylinder_tile(k: i32) -> HomPatch<S> {
    let r = SMOOTH_RADIUS;
    let dirs = [
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, -1.0],
        [-1.0, 0.0, 0.0],
    ];
    let dir = |i: i32| dirs[i.rem_euclid(4) as usize];
    let (from, to) = (dir(k), dir(k + 1));
    let half = std::f64::consts::FRAC_1_SQRT_2;
    let arc = [
        (from.map(|x| r * x), 1.0),
        ([0, 1, 2].map(|i| r * (from[i] + to[i])), half),
        (to.map(|x| r * x), 1.0),
    ];
    let ys = [-20.0, 0.0, 20.0];
    let net = (0..9).map(|n| {
        let ((p, w), y) = (arc[n / 3], ys[n % 3]);
        [p[0], p[1] + y, p[2], w]
    });
    HomPatch::from_affine(2, 2, net, &like()).expect("a biquadratic net")
}

/// The Near leaves of the smooth pair with gap 0 around the touch: the
/// sheet's box `x, y ∈ [−0.15625, 0.15625]` against each face's box at
/// the top, `y ∈ [−0.625, 0.625]`.
fn smooth_leaves() -> Vec<near_quadric::Leaf<S>> {
    let q = cylinder();
    let sheet = HomPatch::from_affine(2, 2, sheets::smooth(0.0).net, &like()).expect("a net");
    let fb = SheetFace {
        patch: &sheet,
        flipped: false,
    };
    let b0 = BoxKey::new(
        Operand::B,
        FaceId(0),
        [0, 0],
        ext(63, 65, 7),
        ext(63, 65, 7),
    );
    let mut out = Vec::new();
    for (face, cell, u) in [(0, -1, ext(15, 16, 4)), (1, 0, ext(0, 1, 4))] {
        let tile = cylinder_tile(cell);
        let fa = QuadricFace {
            quadric: &q,
            patch: &tile,
            flipped: false,
        };
        let a0 = BoxKey::new(Operand::A, FaceId(face), [0, 0], u, ext(31, 33, 6));
        let (ls, _) = near_quadric::leaves(fa, a0, fb, b0, B_K, 1 << 16, |_| BoxUse::Test)
            .expect("the leaves certify");
        out.extend(
            ls.into_iter()
                .filter(|l| l.tangency() == Some(Tangency::Near)),
        );
    }
    out
}

/// The cylinder's top half over `y ∈ [−20, 20]` as one sheet of `faces`.
fn top_half(faces: &[u32]) -> CylinderSheet {
    CylinderSheet::new(
        cylinder(),
        faces.iter().map(|&f| FaceId(f)),
        [[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        [-20.0, 20.0],
    )
    .expect("a sheet")
}

#[test]
fn the_smooth_pair_takes_the_closest_point_onto_the_cylinder() {
    let ls = smooth_leaves();
    let faces: std::collections::BTreeSet<u32> = ls.iter().map(|l| l.a.face().0).collect();
    assert_eq!(faces.into_iter().collect::<Vec<_>>(), [0, 1]);
    let held: Vec<&near_quadric::Leaf<S>> = ls.iter().collect();
    let sheet = top_half(&[0, 1]);
    let on_a = |sheet| {
        [
            Candidate {
                singular: false,
                sheet: Some(sheet),
            },
            NONE[1],
        ]
    };
    let t = choose(Tangency::Near, &held, on_a(&sheet), &mut work()).unwrap();
    assert_eq!(
        t,
        Some(Target {
            operand: Operand::A,
            map: Map::Closest,
        })
    );
    let mut rev = held.clone();
    rev.reverse();
    assert_eq!(
        choose(Tangency::Near, &rev, on_a(&sheet), &mut work()).unwrap(),
        t
    );

    // Without the sheet, or with a sheet missing one of the cluster's
    // target faces (a crease, as the map sees it), projection along the
    // common direction, about `+z`.
    let along = |t: Option<Target>, operand| match t {
        Some(Target {
            operand: o,
            map: Map::Along(ell),
        }) if o == operand => ell,
        t => panic!("{t:?}"),
    };
    let ell = along(
        choose(Tangency::Near, &held, NONE, &mut work()).unwrap(),
        Operand::A,
    );
    assert!(ell[2] > 0u32 && ell[0] == 0u32);
    let part = top_half(&[1]);
    let t = choose(Tangency::Near, &held, on_a(&part), &mut work()).unwrap();
    assert_eq!(along(t, Operand::A), ell);

    // A singular point on the cylinder: the sheet is the target, and it
    // offers no closest point.
    let singular = [
        Candidate {
            singular: true,
            sheet: Some(&sheet),
        },
        NONE[1],
    ];
    let t = choose(Tangency::Near, &held, singular, &mut work()).unwrap();
    assert_eq!(along(t, Operand::B), ell);
}

#[test]
fn the_closest_point_needs_feet_inside_and_half_the_radius() {
    let ls = smooth_leaves();
    let held: Vec<&near_quadric::Leaf<S>> = ls.iter().collect();
    let closest_on = |sheet: &CylinderSheet| {
        let c = [
            Candidate {
                singular: false,
                sheet: Some(sheet),
            },
            NONE[1],
        ];
        matches!(
            choose(Tangency::Near, &held, c, &mut work()).unwrap(),
            Some(Target {
                map: Map::Closest,
                ..
            })
        )
    };
    let inside = |wedge: [[f64; 3]; 2], axial: [f64; 2]| {
        let s = CylinderSheet::new(cylinder(), [FaceId(0), FaceId(1)], wedge, axial).unwrap();
        closest_on(&s)
    };
    let (mx, px, pz) = ([-1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    assert!(inside([mx, px], [-20.0, 20.0]));
    // The touch is above the axis's origin: a sheet ending there, along
    // the axis or round it, doesn't hold every foot inside.
    assert!(!inside([mx, px], [0.0, 20.0]));
    assert!(!inside([mx, px], [-20.0, 0.0]));
    assert!(!inside([pz, px], [-20.0, 20.0]));
    assert!(!inside([mx, pz], [-20.0, 20.0]));
    assert!(inside([mx, px], [-0.25, 0.25]));
    // A cylinder of another radius isn't the leaves'.
    let other = Quadric::cylinder([0.0; 3], [0.0, 1.0, 0.0], 51.0).unwrap();
    let s = CylinderSheet::new(other, [FaceId(0), FaceId(1)], [mx, px], [-20.0, 20.0]).unwrap();
    assert!(!closest_on(&s));
    // `b_K·κ_max < ½` fails on a cylinder of radius below `2b_K`, whose
    // leaves are certified with this `b_K`.
    let thin = Quadric::cylinder([0.0; 3], [0.0, 1.0, 0.0], 2.0 * B_K).unwrap();
    let tiny = CylinderSheet::new(thin, [FaceId(0)], [mx, px], [-1.0, 1.0]).unwrap();
    let l = &ls[0];
    assert!(l.closest(Operand::A, &top_half(&[0, 1])));
    assert!(!l.closest(Operand::A, &tiny));
    assert!(!l.closest(Operand::B, &top_half(&[0, 1])));
}

#[test]
fn sheets_are_checked() {
    let (mx, px, pz, py) = (
        [-1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0],
    );
    let ok = |faces: &[u32], wedge, axial| {
        CylinderSheet::new(cylinder(), faces.iter().map(|&f| FaceId(f)), wedge, axial).is_some()
    };
    assert!(ok(&[1, 0, 1], [mx, px], [-1.0, 1.0]));
    assert_eq!(top_half(&[1, 0, 1]).faces(), [FaceId(0), FaceId(1)]);
    assert!(ok(&[0], [pz, px], [-1.0, 1.0]));
    // More than a half turn, nothing, along the axis, no face, empty or
    // NaN ranges.
    assert!(!ok(&[0], [px, pz], [-1.0, 1.0]));
    assert!(!ok(&[0], [px, px], [-1.0, 1.0]));
    assert!(!ok(&[0], [py, px], [-1.0, 1.0]));
    assert!(!ok(&[], [mx, px], [-1.0, 1.0]));
    assert!(!ok(&[0], [mx, px], [1.0, 1.0]));
    assert!(!ok(&[0], [mx, px], [f64::NAN, 1.0]));
}
