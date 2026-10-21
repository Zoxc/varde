use malachite::Rational;
use varde_decide::bezier::{Dir, HomPatch};
use varde_decide::count::boxes::BoxKey;
use varde_decide::ids::FaceId;
use varde_decide::leaves::flat::BoxUse;
use varde_decide::leaves::flat::{self, FlatPiece, LeafWork, certify};
use varde_decide::leaves::near_quadric::{self, QuadricFace, SheetFace};
use varde_decide::num::bounded::Bounded;
use varde_decide::records::keys::Operand;
use varde_testkit::sheets::{self, Diagonals, SMOOTH_RADIUS, Wavy};

use super::super::cluster::merge_sharing;
use super::super::cluster::tests::{
    B_K, ext, fixture_clusters, fixture_leaves, parallelogram, rat, rectangle, whole, work,
};
use super::super::cluster::{ClusterLeaf, clusters};
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
    // Two flush pairs facing up and towards `+y`: the max–min direction
    // of `(0, 0, 1)` and `(0, 1, 0)` is `(0, 1, 1)`. Flush pieces have a
    // vertical gap of zero along every direction they are graphs over,
    // so their cones are the open hemispheres of their normals, however
    // wide the pieces.
    for w in [1e-4, 1e-2, 20.0] {
        let up = flush(0, 0, [0.0; 3], [w, 0.0, 0.0], [0.0, w, 0.0]);
        let side = flush(1, 1, [0.0, w, 0.0], [0.0, 0.0, w], [w, 0.0, 0.0]);
        let along = Some(Target {
            operand: Operand::A,
            map: Map::Along(q3([0, 1, 1])),
        });
        assert_eq!(
            choose(Tangency::Near, &[&up, &side], NONE, &mut work()).unwrap(),
            along,
            "{w}"
        );
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
        let flat::Kind::Near(t) = &up.kind else {
            unreachable!()
        };
        let mut lw = LeafWork::default();
        assert!(t.cone.contains(&q3([0, 1, 1]), &mut lw));
        assert!(!t.cone.contains(&q3([0, 1, 0]), &mut lw));
        assert_eq!(t.cone.b_k(), &rat(B_K));
    }
}

/// A parallelogram `(p0, u, v)`.
type Para = [[f64; 3]; 3];

/// The common `ℓ` moves with the leaves under the cube's 48 symmetries
/// (signed permutations of the axes: 90° rotations and mirrors): the
/// least of the optimal directions, not a vertex the solver reaches. With
/// two normals the optimal directions form a segment (`ℓ` free along
/// their common perpendicular), with three a point.
#[test]
fn a_common_direction_moves_with_the_leaves_under_signed_axis_permutations() {
    let perms = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    // Parallelograms `(p0, u, v)` facing along `u × v`, and their `ℓ`.
    let w = 1e-2;
    let cases: [(Vec<Para>, [i64; 3]); 2] = [
        (
            vec![
                [[0.0; 3], [w, 0.0, 0.0], [0.0, w, 0.0]],
                [[0.0, w, 0.0], [0.0, 0.0, w], [w, 0.0, 0.0]],
            ],
            [0, 1, 1],
        ),
        (
            vec![
                [[0.0; 3], [w, 0.0, 0.0], [0.0, w, 0.0]],
                [[0.0, w, 0.0], [0.0, 0.0, w], [w, 0.0, 0.0]],
                [[w, 0.0, 0.0], [0.0, w, 0.0], [0.0, 0.0, w]],
            ],
            [1, 1, 1],
        ),
    ];
    for (pieces, ell) in &cases {
        for p in perms {
            for bits in 0..8u32 {
                let s = [0, 1, 2].map(|i| if bits >> i & 1 == 1 { -1i64 } else { 1 });
                let mv = |v: [f64; 3]| [0, 1, 2].map(|i| s[i] as f64 * v[p[i]]);
                // A mirror turns `u × v` round; swapping `u` and `v`
                // keeps the moved piece facing the moved normal.
                let det = {
                    let parity = if [[0, 1, 2], [1, 2, 0], [2, 0, 1]].contains(&p) {
                        1
                    } else {
                        -1
                    };
                    parity * s[0] * s[1] * s[2]
                };
                let leaves: Vec<flat::Leaf> = pieces
                    .iter()
                    .enumerate()
                    .map(|(k, [p0, u, v])| {
                        let (u, v) = if det > 0 {
                            (mv(*u), mv(*v))
                        } else {
                            (mv(*v), mv(*u))
                        };
                        flush(k as u32, k as u32, mv(*p0), u, v)
                    })
                    .collect();
                let held: Vec<&flat::Leaf> = leaves.iter().collect();
                let want = Some(Target {
                    operand: Operand::A,
                    map: Map::Along(q3([0, 1, 2].map(|i| s[i] * ell[p[i]]))),
                });
                assert_eq!(
                    choose(Tangency::Near, &held, NONE, &mut work()).unwrap(),
                    want,
                    "{p:?} {s:?}"
                );
            }
        }
    }
}

/// The merged cluster of `pair`'s flush leaves (every cluster sharing a
/// window element), its leaves, and the leaf normals of each cluster
/// before the merge, in order.
fn merged_flush(pair: &sheets::Pair, ls: &[flat::Leaf]) -> (Vec<u32>, Vec<Option<Target>>) {
    let cs = fixture_clusters(pair, ls);
    let alone = cs
        .iter()
        .map(|c| choose(c.kind(), &picked(ls, c.leaves()), NONE, &mut work()).unwrap())
        .collect();
    let windows: Vec<Vec<u32>> = cs.iter().map(|_| vec![0]).collect();
    let merged = merge_sharing(cs, &windows, &mut work()).unwrap();
    let [m] = &merged[..] else {
        panic!("{merged:?}")
    };
    (m.cluster.leaves().collect(), alone)
}

#[test]
fn a_merged_flush_box_edge_fold_projects_along_a_tilted_direction() {
    // The flush fold: a cluster on the box's front and one on its top,
    // merged (their windows share the fold edge). Each alone projects
    // along its normal; together along the max–min direction `(0, −1,
    // 1)`, which every leaf admits (flush: no gap along it), though the
    // pieces are wide. A creased target has no closest point, so `ℓ`.
    let pair = sheets::box_edge_fold(0.0);
    let ls = fixture_leaves(&pair, rectangle, 2);
    let (held, alone) = merged_flush(&pair, &ls);
    let along = |op, n| {
        Some(Target {
            operand: op,
            map: Map::Along(q3(n)),
        })
    };
    assert_eq!(
        alone,
        [along(Operand::A, [0, -1, 0]), along(Operand::A, [0, 0, 1])]
    );
    let held = picked(&ls, held.into_iter());
    let t = choose(Tangency::Near, &held, NONE, &mut work()).unwrap();
    assert_eq!(t, along(Operand::A, [0, -1, 1]));
    let mut rev = held.clone();
    rev.reverse();
    assert_eq!(choose(Tangency::Near, &rev, NONE, &mut work()).unwrap(), t);
}

#[test]
fn a_merged_flush_l_corner_projects_along_the_diagonal() {
    // The sheet in the L's reflex edge: clusters on the faces `y = 15` and
    // `x = 15`, merged, project along `(1, 1, 0)`, into the corner.
    let pair = sheets::l_corner(0.0);
    let ls = fixture_leaves(&pair, rectangle, 2);
    let (held, alone) = merged_flush(&pair, &ls);
    let along = |n| {
        Some(Target {
            operand: Operand::A,
            map: Map::Along(q3(n)),
        })
    };
    assert_eq!(alone, [along([0, 1, 0]), along([1, 0, 0])]);
    let held = picked(&ls, held.into_iter());
    assert_eq!(
        choose(Tangency::Near, &held, NONE, &mut work()).unwrap(),
        along([1, 1, 0])
    );
}

#[test]
fn merged_flush_wavy_sheets_project_along_one_direction() {
    // Two wavy sheets triangulated alike, flush: a cluster per triangle
    // (neighbours aren't coplanar), merged into one of many normals; the
    // max–min direction is positive on all and every flush leaf admits
    // it.
    let w = Wavy {
        cells: 4,
        waves: 1,
        offset: 0.0,
        diagonals_b: Diagonals::Rising,
        ..Wavy::DEFAULT
    };
    let pair = sheets::wavy(&w);
    let ls = fixture_leaves(&pair, parallelogram, 0);
    let cs = fixture_clusters(&pair, &ls);
    assert!(cs.len() > 1, "{cs:?}");
    let windows: Vec<Vec<u32>> = cs.iter().map(|_| vec![0]).collect();
    let merged = merge_sharing(cs, &windows, &mut work()).unwrap();
    let [m] = &merged[..] else {
        panic!("{merged:?}")
    };
    let held = picked(&ls, m.cluster.leaves());
    let normals: std::collections::BTreeSet<Q3> = held
        .iter()
        .flat_map(|l| TangentLeaf::normals(*l))
        .map(|n| scaled(&n))
        .collect();
    assert!(normals.len() > 2, "{normals:?}");
    match choose(Tangency::Near, &held, NONE, &mut work()).unwrap() {
        Some(Target {
            operand: Operand::A,
            map: Map::Along(ell),
        }) => {
            assert!(normals.iter().all(|n| dot(n, &ell) > 0u32));
            // Most leaves are tilted against it.
            assert!(normals.iter().filter(|n| **n != scaled(&ell)).count() > 1);
        }
        t => panic!("{t:?}"),
    }
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
fn the_smooth_pair_clusters_and_takes_the_closest_point() {
    // The curved leaves form their cluster as flat ones do: the sheet's
    // boxes join the leaves on both of the cylinder's faces into one.
    let ls = smooth_leaves();
    let key = |op: Operand, f: FaceId| (op as u8, f.0);
    let cs = clusters(&ls, key, &mut work()).unwrap();
    let [c] = &cs[..] else { panic!("{cs:?}") };
    assert_eq!(c.kind(), Tangency::Near);
    assert_eq!(c.leaves().len(), ls.len());
    let held: Vec<&near_quadric::Leaf<S>> = c.leaves().map(|i| &ls[i as usize]).collect();
    let sheet = top_half(&[0, 1]);
    let on_a = [
        Candidate {
            singular: false,
            sheet: Some(&sheet),
        },
        NONE[1],
    ];
    assert_eq!(
        choose(c.kind(), &held, on_a, &mut work()).unwrap(),
        Some(Target {
            operand: Operand::A,
            map: Map::Closest,
        })
    );
    // The cylinder as the second operand: each leaf's boxes read in the
    // operands' order, the same clusters.
    let swap =
        |b: BoxKey, op| BoxKey::new(op, b.face(), b.tile(), b.extent(Dir::U), b.extent(Dir::V));
    let swapped: Vec<near_quadric::Leaf<S>> = ls
        .iter()
        .map(|l| near_quadric::Leaf {
            a: swap(l.a, Operand::B),
            b: swap(l.b, Operand::A),
            ..l.clone()
        })
        .collect();
    let cs2 = clusters(&swapped, key, &mut work()).unwrap();
    let [c2] = &cs2[..] else { panic!("{cs2:?}") };
    assert_eq!(c2.leaves().len(), ls.len());
    // With a flat flush pair elsewhere in one run: two clusters, the
    // curved one unchanged.
    let up = flush(7, 7, [100.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let mut mixed: Vec<&dyn ClusterLeaf> = vec![&up];
    mixed.extend(ls.iter().map(|l| l as &dyn ClusterLeaf));
    let both = clusters(&mixed, key, &mut work()).unwrap();
    assert_eq!(both.len(), 2);
    let curved = both
        .iter()
        .find(|m| m.leaves().len() == ls.len())
        .expect("the curved cluster");
    let shifted: Vec<u32> = c.leaves().map(|i| i + 1).collect();
    assert_eq!(curved.leaves().collect::<Vec<_>>(), shifted);
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
