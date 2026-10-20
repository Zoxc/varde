use malachite::Rational;
use varde_decide::bezier::{Dir, HomPatch};
use varde_decide::count::boxes::{BoxKey, Extent};
use varde_decide::ids::{EdgeId, FaceId, VertId};
use varde_decide::leaves::flat::{self, BoxUse, FlatFace};
use varde_decide::leaves::near_quadric::{self, Quadric, QuadricFace, SheetFace};
use varde_decide::num::bounded::Bounded;
use varde_decide::records::keys::Operand;
use varde_testkit::sheets::{self, Polygons, SMOOTH_RADIUS};

use super::super::cluster::tests::{B_K, ext, fixture_leaves, quad, rat, rectangle, whole, work};
use super::super::cluster::{ClusterLeaf, Tangency};
use super::*;
use crate::infra::budget::Budget;
use crate::infra::error::Cause;

/// The band of every rim piece in these tests (mm).
const BETA: f64 = 1e-3;

fn band(_: Along) -> f64 {
    BETA
}

fn q(x: f64) -> Rational {
    rat(x)
}

fn uv(u: f64, v: f64) -> P2 {
    [q(u), q(v)]
}

/// A leaf of two given boxes, Near, for rims read off boxes alone.
struct Boxes([BoxKey; 2]);

impl ClusterLeaf for Boxes {
    fn boxes(&self) -> [BoxKey; 2] {
        self.0
    }

    fn world(&self) -> [[[f64; 3]; 2]; 2] {
        [[[0.0; 3], [1.0; 3]]; 2]
    }

    fn tangency(&self) -> Option<Tangency> {
        Some(Tangency::Near)
    }
}

/// The scale of the bowl's height, `2⁻¹⁵` mm per mm².
const C: f64 = 1.0 / 32768.0;

/// The bowl `z = c·((x − 5)² + (y − 5)²) − h` over `x, y ∈ [0, 10]`, a
/// biquadratic patch exactly (`u` along `x`, `v` along `y`).
fn bowl(h: f64) -> HomPatch<Rational> {
    // `(s − 5)²` over `s ∈ [0, 10]` has the Bernstein coefficients
    // `25, −25, 25`.
    let bern = [25.0, -25.0, 25.0];
    let xs = [0.0, 5.0, 10.0];
    let net = (0..9).map(|n| {
        let (i, j) = (n / 3, n % 3);
        let z = q(C) * (q(bern[i]) + q(bern[j])) - q(h);
        [q(xs[i]), q(xs[j]), z, Rational::from(1)]
    });
    HomPatch::new(2, 2, net).expect("a biquadratic net")
}

/// The plane `z = 0` over `[0, 10]²`, facing up.
fn floor_plane() -> HomPatch<Rational> {
    quad([0.0; 3], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0])
}

const FLOOR: Implicit = Implicit::Plane {
    normal: [0.0, 0.0, 1.0],
    offset: 0.0,
};

/// The Near leaves of the floor (first operand, face 0) and the bowl of
/// depth `h` (second, face 0), tested at boxes split twice each way: the
/// four boxes about the bowl's bottom, `[2.5, 7.5]²`.
fn bowl_leaves(h: f64) -> Vec<flat::Leaf> {
    let (fl, b) = (floor_plane(), bowl(h));
    let (ls, _) = flat::leaves(
        FlatFace {
            patch: &fl,
            flipped: false,
        },
        whole(Operand::A, 0, [0, 0]),
        FlatFace {
            patch: &b,
            flipped: false,
        },
        whole(Operand::B, 0, [0, 0]),
        &rat(B_K),
        1 << 16,
        |bx| {
            if bx.extent(Dir::U).depth() < 2 || bx.extent(Dir::V).depth() < 2 {
                BoxUse::Split
            } else {
                BoxUse::Test
            }
        },
    )
    .expect("flat pieces certify");
    ls.into_iter()
        .filter(|l| l.tangency() == Some(Tangency::Near))
        .collect()
}

/// The bowl's four boxes about its bottom, `[2.5, 7.5]²`, as leaves
/// against the floor's tile.
fn centre_boxes() -> Vec<Boxes> {
    let t = whole(Operand::A, 0, [0, 0]);
    let e = [ext(1, 2, 2), ext(2, 3, 2)];
    e.iter()
        .flat_map(|&u| {
            e.iter()
                .map(move |&v| Boxes([t, BoxKey::new(Operand::B, FaceId(0), [0, 0], u, v)]))
        })
        .collect()
}

/// The rim of `leaves` on the bowl of depth `h` with `target` trims and
/// band `beta`.
fn bowl_rim<L: ClusterLeaf>(
    leaves: &[L],
    h: f64,
    target: &[RimTrim<'_>],
    beta: &(dyn Fn(Along) -> f64 + Sync),
) -> Result<Rim, RimError> {
    let tiles = [([0, 0], bowl(h))];
    let faces = [SnappedFace {
        face: FaceId(0),
        tiles: &tiles,
        own: &[],
        target,
        flipped: false,
    }];
    let input = RimInput {
        snapped: Operand::B,
        faces: &faces,
        targets: &[(FaceId(0), FLOOR)],
        band: beta,
    };
    rim(leaves, &input, &mut work())
}

/// The bowl's height at `(x, y)`, in `f64`.
fn height(h: f64, x: f64, y: f64) -> f64 {
    C * ((x - 5.0) * (x - 5.0) + (y - 5.0) * (y - 5.0)) - h
}

fn fl(x: &Rational) -> f64 {
    f64::rounding_from(x, Nearest).0
}

#[test]
fn the_bowl_s_rim_is_its_cluster_s_square_with_a_branch_at_each_crossing() {
    // The bowl's zero circle, radius 3 about (5, 5), crosses each side of
    // the cluster's square `[2.5, 7.5]²` twice: eight branch vertices,
    // the gap falling, then rising, along each side.
    let h = 9.0 * C;
    let ls = bowl_leaves(h);
    let r = bowl_rim(&ls, h, &[], &band).unwrap();
    // The certified cluster is the four boxes about the bottom.
    let mut held: Vec<BoxKey> = ls.iter().map(|l| l.b).collect();
    held.sort();
    held.dedup();
    let mut centre: Vec<BoxKey> = centre_boxes().iter().map(|l| l.0[1]).collect();
    centre.sort();
    assert_eq!(held, centre);
    assert_eq!(bowl_rim(&centre_boxes(), h, &[], &band).unwrap(), r);
    assert_eq!(r.loops, [vec![0, 1, 2, 3]]);
    let corners = [(0.25, 0.25), (0.75, 0.25), (0.75, 0.75), (0.25, 0.75)];
    for (k, p) in r.pieces.iter().enumerate() {
        let (a, b) = (corners[k], corners[(k + 1) % 4]);
        assert_eq!(p.ends, [uv(a.0, a.1), uv(b.0, b.1)], "{k}");
        assert_eq!(
            (p.along, p.tile, &p.targets[..]),
            (Along::Box, [0, 0], &[FaceId(0)][..])
        );
    }
    assert_eq!(r.branches.len(), 8, "{:?}", r.branches);
    let rising: Vec<bool> = r.branches.iter().map(|b| b.rising).collect();
    assert_eq!(rising, [false, true].repeat(4));
    let off = 2.75f64.sqrt() / 10.0;
    for (k, b) in r.branches.iter().enumerate() {
        assert_eq!(b.piece as usize, k / 2);
        assert!(b.residual <= BETA, "{b:?}");
        // On the zero circle, to within the band.
        let (x, y) = (10.0 * fl(&b.uv[0]), 10.0 * fl(&b.uv[1]));
        assert!(height(h, x, y).abs() <= BETA, "{b:?}");
        let along = if k / 2 % 2 == 0 {
            fl(&b.uv[0])
        } else {
            fl(&b.uv[1])
        };
        let from_mid = (along - 0.5).abs();
        assert!((from_mid - off).abs() < 1e-3, "{b:?}");
    }
    // Every cluster has a net change, one-signed cells either side.
    assert_eq!(r.clusters.len(), 8);
    assert!(r.clusters.iter().all(|c| c.net.is_some_and(|n| n != 0)));
    assert!(r.window.is_empty());
    // The leaves in any order give the same rim.
    let mut rev = ls.clone();
    rev.reverse();
    assert_eq!(bowl_rim(&rev, h, &[], &band).unwrap(), r);
}

#[test]
fn a_double_root_of_the_gap_gives_no_vertex() {
    // Radius 2.5: the zero circle touches each side at its middle, the
    // gap along it `c·(x − 5)²` (a double root): a root cluster of net 0
    // per side and no branch vertex. The rim reads the cluster's boxes
    // only: those of the first test's certified leaves.
    let h = 6.25 * C;
    let r = bowl_rim(&centre_boxes(), h, &[], &band).unwrap();
    assert_eq!(r.pieces.len(), 4);
    assert!(r.branches.is_empty(), "{:?}", r.branches);
    assert_eq!(r.clusters.len(), 4);
    for c in &r.clusters {
        assert_eq!(c.net, Some(0));
        assert_eq!(c.from.0, c.to.0);
        let mid = Dyadic::new(1, 1).unwrap();
        assert!(c.from.1 < mid && mid < c.to.1, "{c:?}");
    }
    // The rest of each piece reads positive.
    for g in &r.gaps {
        assert!(
            g.cells
                .iter()
                .all(|(_, s)| matches!(s, Gap::Positive | Gap::Cluster))
        );
    }
}

#[test]
fn two_crossings_within_the_band_give_no_vertex() {
    // The circle a little wider than 2.5 crosses each side twice,
    // `2·2⁻¹⁵` mm apart: within `β_rim = 10⁻³` one root cluster of net 0;
    // with a band below that, two branch vertices a side.
    let h = (6.25 + 1.0 / (1u64 << 30) as f64) * C;
    let ls = bowl_leaves(h);
    let r = bowl_rim(&ls, h, &[], &band).unwrap();
    assert!(r.branches.is_empty(), "{:?}", r.branches);
    assert!(r.clusters.iter().all(|c| c.net == Some(0)));
    let fine = |_: Along| 1e-5;
    let r = bowl_rim(&ls, h, &[], &fine).unwrap();
    assert_eq!(r.branches.len(), 8, "{:?}", r.branches);
    assert!(r.branches.iter().all(|b| b.residual <= 1e-5));
}

#[test]
fn refinement_is_in_space_to_the_depth_limit_never_a_halving() {
    // A band no subdivision reaches: the cells about the simple roots
    // split down to the depth limit, a size limit, not a failure that
    // would halve `b_K` (the rim reads no `b_K`).
    let h = 9.0 * C;
    let ls = bowl_leaves(h);
    let tiny = |_: Along| 1e-30;
    match bowl_rim(&ls, h, &[], &tiny) {
        Err(RimError::OutOfBudget(KernelError::OutOfBudget { cause, .. })) => {
            assert_eq!(cause, Cause::SizeLimit)
        }
        r => panic!("{r:?}"),
    }
    // A budget too small runs out the same way, as work.
    let mut w = Work::new(&Budget::new(50));
    let tiles = [([0, 0], bowl(h))];
    let faces = [SnappedFace {
        face: FaceId(0),
        tiles: &tiles,
        own: &[],
        target: &[],
        flipped: false,
    }];
    let input = RimInput {
        snapped: Operand::B,
        faces: &faces,
        targets: &[(FaceId(0), FLOOR)],
        band: &band,
    };
    match rim(&ls, &input, &mut w) {
        Err(RimError::OutOfBudget(KernelError::OutOfBudget { cause, .. })) => {
            assert_eq!(cause, Cause::Work)
        }
        r => panic!("{r:?}"),
    }
}

/// The straight span from `a` to `b`, weights 1.
fn span(a: [f64; 2], b: [f64; 2]) -> UvSpan {
    let m = [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1])];
    [[a[0], a[1], 1.0], [m[0], m[1], 1.0], [b[0], b[1], 1.0]]
}

const KNOTS: [f64; 2] = [0.0, 1.0];

#[test]
fn where_the_cluster_reaches_a_target_edge_the_rim_runs_along_it() {
    // The floor's face ends at `x = 6.25` (`u = 0.625` on the bowl), across
    // the cluster's square: the rim runs along that edge there, and the
    // branch vertices past it are gone.
    let h = 9.0 * C;
    let ls = bowl_leaves(h);
    let c = [[0.0, 0.0], [0.625, 0.0], [0.625, 1.0], [0.0, 1.0]];
    let spans: Vec<UvSpan> = (0..4).map(|k| span(c[k], c[(k + 1) % 4])).collect();
    let trims: Vec<RimTrim<'_>> = (0..4)
        .map(|k| RimTrim {
            edge: EdgeId(k as u32),
            ends: [VertId(k as u32), VertId(((k + 1) % 4) as u32)],
            spans: &spans[k..k + 1],
            knots: &KNOTS,
            left: true,
        })
        .collect();
    let r = bowl_rim(&ls, h, &trims, &band).unwrap();
    let alongs: Vec<Along> = r.pieces.iter().map(|p| p.along).collect();
    assert_eq!(
        alongs,
        [Along::Box, Along::Target(EdgeId(1)), Along::Box, Along::Box]
    );
    let edge = &r.pieces[1];
    assert_eq!(edge.ends, [uv(0.625, 0.25), uv(0.625, 0.75)]);
    assert_eq!(edge.params, Some([q(0.25), q(0.75)]));
    // One crossing on the bottom and the top, two on the left side.
    let at: Vec<(u32, bool)> = r.branches.iter().map(|b| (b.piece, b.rising)).collect();
    assert_eq!(at, [(0, false), (2, true), (3, false), (3, true)]);
    // The edge under the rim is in the window; its vertices aren't.
    assert_eq!(r.window, [Element::Edge(Operand::A, EdgeId(1))]);
}

#[test]
fn boxes_touching_at_a_corner_bound_one_loop() {
    // Two boxes meeting at one point: the closed union is connected, and
    // its boundary one loop through the point (the rightmost turn there).
    let s = |u: Extent, v: Extent| BoxKey::new(Operand::B, FaceId(0), [0, 0], u, v);
    let (lo, hi) = (ext(0, 1, 1), ext(1, 2, 1));
    let r = raised_rim(&[s(lo, lo), s(hi, hi)], &[[0, 0]]);
    assert_eq!(r.loops.len(), 1);
    assert_eq!(r.pieces.len(), 8);
    let path: Vec<P2> = r.pieces.iter().map(|p| p.ends[0].clone()).collect();
    let pts = [
        (0.0, 0.0),
        (0.5, 0.0),
        (0.5, 0.5),
        (1.0, 0.5),
        (1.0, 1.0),
        (0.5, 1.0),
        (0.5, 0.5),
        (0.0, 0.5),
    ];
    assert_eq!(path, pts.map(|(a, b)| uv(a, b)));
    // The sheet lies 10⁻⁴ above the floor: one-signed throughout.
    assert!(r.clusters.is_empty() && r.branches.is_empty());
    assert!(
        r.gaps
            .iter()
            .all(|g| g.cells.iter().all(|c| c.1 == Gap::Positive))
    );
}

/// A face's own trims on its rectangle tile: per edge of its loop, the
/// straight span between its vertices' UVs.
struct OwnTrims {
    spans: Vec<UvSpan>,
    edges: Vec<(EdgeId, [VertId; 2], bool)>,
}

fn own_trims(body: &Polygons, f: usize, tile: &HomPatch<Rational>, flipped: bool) -> OwnTrims {
    let net = tile.tensors();
    let ctl = |i: usize, j: usize| [0, 1, 2].map(|k| net[k].get(i, j).clone());
    let (p0, pv, pu) = (ctl(0, 0), ctl(0, 1), ctl(1, 0));
    let dot = |a: &[Rational; 3], b: &[Rational; 3]| {
        (0..3).fold(Rational::from(0), |s, k| s + &a[k] * &b[k])
    };
    let d = |a: &[Rational; 3], b: &[Rational; 3]| [0, 1, 2].map(|k| &a[k] - &b[k]);
    let (du, dv) = (d(&pu, &p0), d(&pv, &p0));
    let uv_of = |v: u32| {
        let p = body.verts[v as usize].map(q);
        let r = d(&p, &p0);
        [
            fl(&(dot(&r, &du) / dot(&du, &du))),
            fl(&(dot(&r, &dv) / dot(&dv, &dv))),
        ]
    };
    let edges = body.edges();
    let l = &body.faces[f];
    let mut out = OwnTrims {
        spans: Vec::new(),
        edges: Vec::new(),
    };
    for k in 0..l.len() {
        let (a, b) = (l[k], l[(k + 1) % l.len()]);
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        let id = edges
            .iter()
            .position(|(e, _)| *e == [lo, hi])
            .expect("an edge");
        out.spans.push(span(uv_of(lo), uv_of(hi)));
        out.edges.push((
            EdgeId(id as u32),
            [VertId(lo), VertId(hi)],
            (a == lo) != flipped,
        ));
    }
    out
}

fn trims_of(o: &OwnTrims) -> Vec<RimTrim<'_>> {
    o.edges
        .iter()
        .enumerate()
        .map(|(k, (e, ends, left))| RimTrim {
            edge: *e,
            ends: *ends,
            spans: &o.spans[k..k + 1],
            knots: &KNOTS,
            left: *left,
        })
        .collect()
}

/// A face's plane, outward.
fn plane_of(body: &Polygons, f: usize) -> Implicit {
    let l = &body.faces[f];
    let mut n = [0.0; 3];
    for k in 0..l.len() {
        let (a, b) = (
            body.verts[l[k] as usize],
            body.verts[l[(k + 1) % l.len()] as usize],
        );
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let p = body.verts[l[0] as usize];
    Implicit::Plane {
        normal: n,
        offset: n[0] * p[0] + n[1] * p[1] + n[2] * p[2],
    }
}

#[test]
fn a_fold_flush_on_a_box_edge_has_one_rim_loop_round_both_faces() {
    // The sheet folded over the box's edge, flush: the leaves on the box's
    // top and front, one cluster after the window merge. The fold edge
    // lies inside the cluster (both its faces are), so it is no rim: one
    // loop of the sheet's six outer edges, across both faces, every piece
    // on the box exactly (gap zero), no branch.
    let pair = sheets::box_edge_fold(0.0);
    let ls: Vec<flat::Leaf> = fixture_leaves(&pair, rectangle, 2)
        .into_iter()
        .filter(|l| l.tangency() == Some(Tangency::Near))
        .collect();
    let tiles: Vec<[([i64; 2], HomPatch<Rational>); 1]> = (0..2)
        .map(|f| [([0, 0], rectangle(&pair.b, f).0)])
        .collect();
    let owns: Vec<OwnTrims> = (0..2)
        .map(|f| {
            let (t, flipped) = rectangle(&pair.b, f);
            own_trims(&pair.b, f, &t, flipped)
        })
        .collect();
    let trims: Vec<Vec<RimTrim<'_>>> = owns.iter().map(trims_of).collect();
    let faces: Vec<SnappedFace<'_>> = (0..2)
        .map(|f| SnappedFace {
            face: FaceId(f as u32),
            tiles: &tiles[f],
            own: &trims[f],
            target: &[],
            flipped: false,
        })
        .collect();
    let targets: Vec<(FaceId, Implicit)> = (0..pair.a.faces.len())
        .map(|f| (FaceId(f as u32), plane_of(&pair.a, f)))
        .collect();
    let input = RimInput {
        snapped: Operand::B,
        faces: &faces,
        targets: &targets,
        band: &band,
    };
    let r = rim(&ls, &input, &mut work()).unwrap();
    let fold = EdgeId(0);
    assert_eq!(pair.b.edges()[0].0, [0, 1]);
    assert_eq!(r.loops, [vec![0, 1, 2, 3, 4, 5]]);
    let mut edges: Vec<EdgeId> = r
        .pieces
        .iter()
        .map(|p| match p.along {
            Along::Own(e) => e,
            a => panic!("{a:?}"),
        })
        .collect();
    edges.sort();
    assert_eq!(edges, (1..7).map(EdgeId).collect::<Vec<_>>());
    // Each face holds three pieces, the top's against the box's top
    // (face 1), the front's against its front (face 2).
    for p in &r.pieces {
        let t = if p.face == FaceId(0) { 1 } else { 2 };
        assert_eq!(p.targets, [FaceId(t)]);
    }
    assert!(
        r.gaps
            .iter()
            .all(|g| g.cells == [([Dyadic::ZERO, Dyadic::ONE], Gap::Zero)])
    );
    assert!(r.clusters.is_empty() && r.branches.is_empty());
    // The window: every edge and vertex of the sheet, the fold included.
    let mut want: Vec<Element> = (0..7)
        .map(|e| Element::Edge(Operand::B, EdgeId(e)))
        .collect();
    want.extend((0..6).map(|v| Element::Vert(Operand::B, VertId(v))));
    want.sort();
    assert_eq!(r.window, want);
    assert!(r.window.contains(&Element::Edge(Operand::B, fold)));
}

// The smooth pair a little through the cylinder: the biquadratic sheet's
// box `x, y ∈ [−0.15625, 0.15625]` against the cylinder's top half as two
// faces (angle cells −1 and 0).

type S = Bounded<f64>;

fn cylinder() -> Quadric {
    Quadric::cylinder([0.0; 3], [0.0, 1.0, 0.0], SMOOTH_RADIUS).expect("a cylinder")
}

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
    HomPatch::from_affine(2, 2, net, &Bounded::exact(0.0)).expect("a biquadratic net")
}

fn smooth_leaves(gap: f64) -> Vec<near_quadric::Leaf<S>> {
    let q = cylinder();
    let sheet =
        HomPatch::from_affine(2, 2, sheets::smooth(gap).net, &Bounded::exact(0.0)).expect("a net");
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

#[test]
fn the_smooth_pair_through_the_cylinder_has_branches_where_its_gap_changes_sign() {
    // At gap −10⁻⁴ the sheet dips into the cylinder about the touch, its
    // zero curve `x²/200 + y²/400 ≈ 10⁻⁴` crossing the box's sides
    // `y = ±0.15625` at `x ≈ ±0.088`: two branch vertices on each, none on
    // the sides `x = ±0.15625`, where the sheet is outside.
    let gap = -1e-4;
    let ls = smooth_leaves(gap);
    assert!(!ls.is_empty());
    let sheet = sheets::smooth(gap);
    let net = sheet.net.map(|p| p.map(q));
    let tiles = [([0, 0], HomPatch::new(2, 2, net).expect("a net"))];
    let faces = [SnappedFace {
        face: FaceId(0),
        tiles: &tiles,
        own: &[],
        target: &[],
        flipped: false,
    }];
    let surface = Implicit::Cylinder {
        quadric: cylinder(),
        outward: true,
    };
    let input = RimInput {
        snapped: Operand::B,
        faces: &faces,
        targets: &[(FaceId(0), surface), (FaceId(1), surface)],
        band: &band,
    };
    let r = rim(&ls, &input, &mut work()).unwrap();
    assert_eq!(r.loops.len(), 1);
    let x = |b: &Branch| -10.0 + 20.0 * fl(&b.uv[0]);
    let y = |b: &Branch| -10.0 + 20.0 * fl(&b.uv[1]);
    assert_eq!(r.branches.len(), 4, "{:?}", r.branches);
    let rising: Vec<bool> = r.branches.iter().map(|b| b.rising).collect();
    assert_eq!(rising, [false, true, false, true]);
    for b in &r.branches {
        assert!((y(b).abs() - 0.15625).abs() < 1e-12, "{b:?}");
        assert!((x(b).abs() - 0.0884).abs() < 1e-3, "{b:?}");
        assert!(b.residual <= BETA);
        // One surface: both faces of the cylinder beside it, where both are.
        assert!(!b.targets.is_empty());
    }
    let mut rev = ls.clone();
    rev.reverse();
    assert_eq!(rim(&rev, &input, &mut work()).unwrap(), r);
    // At gap 0 the sheet stays outside along the whole rim.
    let ls0 = smooth_leaves(0.0);
    let net0 = sheets::smooth(0.0).net.map(|p| p.map(q));
    let tiles0 = [([0, 0], HomPatch::new(2, 2, net0).expect("a net"))];
    let faces0 = [SnappedFace {
        tiles: &tiles0,
        ..faces[0]
    }];
    let r0 = rim(
        &ls0,
        &RimInput {
            faces: &faces0,
            ..input
        },
        &mut work(),
    )
    .unwrap();
    assert!(r0.branches.is_empty() && r0.clusters.is_empty());
}

#[test]
fn the_smooth_pair_s_branch_vertices_agree_with_the_big_float_oracle() {
    // The oracle's exact zeros of `D − r²` along each side of the sheet's
    // box: one branch vertex per zero, and each branch vertex within its
    // certified residual of the cylinder (`|√D − r| ≤ ρ` iff
    // `|D − r²| ≤ ρ·(2r + ρ)`, exactly); at gap 0 no zero and none.
    use varde_testkit::rational_oracle::{Line, SmoothOracle};
    for gap in [-1e-4, 0.0] {
        let ls = smooth_leaves(gap);
        let sheet = sheets::smooth(gap);
        let oracle = SmoothOracle::new(&sheet).expect("the oracle takes the pair");
        let net = sheet.net.map(|p| p.map(q));
        let tiles = [([0, 0], HomPatch::new(2, 2, net).expect("a net"))];
        let faces = [SnappedFace {
            face: FaceId(0),
            tiles: &tiles,
            own: &[],
            target: &[],
            flipped: false,
        }];
        let surface = Implicit::Cylinder {
            quadric: cylinder(),
            outward: true,
        };
        let input = RimInput {
            snapped: Operand::B,
            faces: &faces,
            targets: &[(FaceId(0), surface), (FaceId(1), surface)],
            band: &band,
        };
        let r = rim(&ls, &input, &mut work()).unwrap();
        let (lo, hi) = (rat(63.0 / 128.0), rat(65.0 / 128.0));
        let width = Rational::from(1) / Rational::from(1u64 << 40);
        let mut zeros = 0;
        for side in [&lo, &hi] {
            for line in [
                Line::AlongU { v: side.clone() },
                Line::AlongV { u: side.clone() },
            ] {
                zeros += oracle
                    .roots(&line, &width)
                    .iter()
                    .filter(|z| z.single && z.lo >= lo && z.hi <= hi)
                    .count();
            }
        }
        assert_eq!(r.branches.len(), zeros, "gap {gap}");
        let radius = rat(SMOOTH_RADIUS);
        for b in &r.branches {
            let d = oracle.implicit(&b.uv[0], &b.uv[1]);
            let rho = rat(b.residual);
            let bound = &rho * (Rational::from(2) * &radius + &rho);
            assert!(d <= bound && -d <= bound, "{b:?}");
        }
    }
}

#[test]
fn bad_input_is_refused() {
    let h = 9.0 * C;
    let ls = bowl_leaves(h);
    // A target face without a surface.
    let tiles = [([0, 0], bowl(h))];
    let faces = [SnappedFace {
        face: FaceId(0),
        tiles: &tiles,
        own: &[],
        target: &[],
        flipped: false,
    }];
    let input = RimInput {
        snapped: Operand::B,
        faces: &faces,
        targets: &[],
        band: &band,
    };
    assert_eq!(
        rim(&ls, &input, &mut work()),
        Err(RimError::Target(FaceId(0)))
    );
    // No face of `S` described.
    let input = RimInput {
        faces: &[],
        targets: &[(FaceId(0), FLOOR)],
        ..input
    };
    assert_eq!(
        rim(&ls, &input, &mut work()),
        Err(RimError::Face(FaceId(0)))
    );
    // A curved trim span.
    let bent = [[[0.0, 0.0, 1.0], [0.5, 0.1, 1.0], [1.0, 0.0, 1.0]]];
    let trim = [RimTrim {
        edge: EdgeId(0),
        ends: [VertId(0), VertId(1)],
        spans: &bent,
        knots: &KNOTS,
        left: true,
    }];
    assert_eq!(
        bowl_rim(&ls, h, &trim, &band),
        Err(RimError::Trim(FaceId(0)))
    );
    // A negative band.
    let neg = |_: Along| -1.0;
    assert_eq!(bowl_rim(&ls, h, &[], &neg), Err(RimError::Band));
    let none: [Boxes; 0] = [];
    assert_eq!(bowl_rim(&none, h, &[], &band), Err(RimError::Empty));
}

/// The rim of leaves of the given boxes of `S` (face 0) against the
/// floor, on a sheet `10⁻⁴` above it in every tile listed.
fn raised_rim(boxes: &[BoxKey], tiles: &[[i64; 2]]) -> Rim {
    let t = whole(Operand::A, 0, [0, 0]);
    let ls: Vec<Boxes> = boxes.iter().map(|&b| Boxes([t, b])).collect();
    let tiles: Vec<([i64; 2], HomPatch<Rational>)> = tiles
        .iter()
        .map(|&k| {
            let raised = quad([0.0, 0.0, 1e-4], [10.0, 0.0, 1e-4], [0.0, 10.0, 1e-4]);
            (k, raised)
        })
        .collect();
    let faces = [SnappedFace {
        face: FaceId(0),
        tiles: &tiles,
        own: &[],
        target: &[],
        flipped: false,
    }];
    let input = RimInput {
        snapped: Operand::B,
        faces: &faces,
        targets: &[(FaceId(0), FLOOR)],
        band: &band,
    };
    rim(&ls, &input, &mut work()).unwrap()
}

#[test]
fn a_ring_of_boxes_has_an_outer_and_an_inner_loop_and_tile_lines_cut_pieces() {
    // The tile's 4 × 4 boxes but the middle four: the outer loop runs
    // counter-clockwise, the hole's clockwise, the region on the left.
    let mut boxes = Vec::new();
    for i in 0..4 {
        for j in 0..4 {
            if (1..3).contains(&i) && (1..3).contains(&j) {
                continue;
            }
            let b = BoxKey::new(
                Operand::B,
                FaceId(0),
                [0, 0],
                ext(i, i + 1, 2),
                ext(j, j + 1, 2),
            );
            boxes.push(b);
        }
    }
    let r = raised_rim(&boxes, &[[0, 0]]);
    assert_eq!(r.loops, [vec![0, 1, 2, 3], vec![4, 5, 6, 7]]);
    let starts: Vec<P2> = r.pieces.iter().map(|p| p.ends[0].clone()).collect();
    let want = [
        (0.0, 0.0),
        (1.0, 0.0),
        (1.0, 1.0),
        (0.0, 1.0),
        (0.25, 0.25),
        (0.25, 0.75),
        (0.75, 0.75),
        (0.75, 0.25),
    ];
    assert_eq!(starts, want.map(|(a, b)| uv(a, b)));
    // Two whole tiles side by side: the bottom and top cut at `u = 1`.
    let tile = |i: i64| whole(Operand::B, 0, [i, 0]);
    let r = raised_rim(&[tile(1), tile(0)], &[[0, 0], [1, 0]]);
    let pieces: Vec<([i64; 2], P2)> = r
        .pieces
        .iter()
        .map(|p| (p.tile, p.ends[0].clone()))
        .collect();
    let want = [
        ([0, 0], (0.0, 0.0)),
        ([1, 0], (1.0, 0.0)),
        ([1, 0], (2.0, 0.0)),
        ([1, 0], (2.0, 1.0)),
        ([0, 0], (1.0, 1.0)),
        ([0, 0], (0.0, 1.0)),
    ];
    assert_eq!(pieces, want.map(|(t, (a, b))| (t, uv(a, b))));
}
