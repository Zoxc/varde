//! Coincidence groups on exact fixtures: the trim overlap test against
//! boxes worked out apart (and under integer shears), membership on the
//! cases that coincide and those that don't (a slotted plate's top, a
//! split edge, faces touching along an edge), the closure, and the
//! directions of the edge-contact cubes, a slit and a slotted plate
//! against boxes of `B`, each record decided by `decide` against `A`
//! moved by `δ·(σ_A·d + η·T₂ + η²·T₃ + η³·T₄)`.

use malachite::Rational as Q;

use super::*;
use crate::ids::VertId;
use crate::num::decide::{Row, Sign, TieKey};
use crate::num::exact::ORDERS;
use crate::perturb::dir::{agrees, dot};
use crate::perturb::vertex::PatchSide;
use crate::perturb::{Moving, PlaneHeight, T, UP, decide_flat, rates_a};
use crate::records::keys::{Query, RecordKey, VvKey};

struct Rng(varde_testkit::rng::Rng);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(varde_testkit::rng::Rng::new(seed))
    }

    fn below(&mut self, n: u64) -> u64 {
        self.0.next_u64() % n
    }

    fn int(&mut self, n: u64) -> i64 {
        self.below(2 * n + 1) as i64 - n as i64
    }
}

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

fn qv(v: DVec3) -> Q3 {
    [q(v.x), q(v.y), q(v.z)]
}

fn v3(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

fn overlap(p: &[Vec<[f64; 2]>], q: &[Vec<[f64; 2]>]) -> bool {
    let mut work = DirWork::default();
    let a = trims_overlap(p, q, &mut work).expect("finite");
    let b = trims_overlap(q, p, &mut work).expect("finite");
    assert_eq!(a, b, "{p:?} {q:?}");
    assert!(work.checks > 0 || !a);
    a
}

/// Overlap over an area, not touching: nested, crossing, equal, along a
/// common edge from either side, at a corner, in a hole, a slit ring.
#[test]
fn trims_overlap_over_an_area_only() {
    let sq = vec![rect(0.0, 0.0, 2.0, 2.0)];
    // Equal, nested, crossing, sharing part of an edge from inside.
    assert!(overlap(&sq, &sq));
    assert!(overlap(&sq, &[rect(0.5, 0.5, 1.0, 1.0)]));
    assert!(overlap(&sq, &[rect(1.0, 1.0, 3.0, 3.0)]));
    assert!(overlap(&sq, &[rect(0.0, 0.0, 1.0, 2.0)]));
    // A cross whose boundaries cross without a vertex inside the other.
    assert!(overlap(
        &[rect(-1.0, 0.0, 3.0, 2.0)],
        &[rect(0.0, -1.0, 2.0, 3.0)]
    ));
    // Along an edge from outside, at a corner, apart.
    assert!(!overlap(&sq, &[rect(2.0, 0.0, 4.0, 2.0)]));
    assert!(!overlap(&sq, &[rect(2.0, 1.0, 4.0, 3.0)]));
    assert!(!overlap(&sq, &[rect(2.0, 2.0, 4.0, 4.0)]));
    assert!(!overlap(&sq, &[rect(5.0, 0.0, 6.0, 1.0)]));
    // A triangle touching the square at a vertex on its edge.
    assert!(!overlap(&sq, &[vec![[1.0, 2.0], [2.0, 3.0], [0.0, 3.0]]]));
    // A face with a hole and one filling it: touching along the hole.
    let holed = vec![rect(0.0, 0.0, 4.0, 4.0), rect(1.0, 1.0, 3.0, 3.0)];
    assert!(!overlap(&holed, &[rect(1.0, 1.0, 3.0, 3.0)]));
    assert!(!overlap(&holed, &[rect(1.5, 1.5, 2.0, 2.0)]));
    assert!(overlap(&holed, &[rect(0.5, 1.0, 3.0, 3.0)]));
    // Loops wound either way.
    let mut cw = rect(0.5, 0.5, 1.5, 1.5);
    cw.reverse();
    assert!(overlap(&sq, &[cw]));
    // A zero-area region overlaps nothing.
    assert!(!overlap(&sq, &[vec![[0.5, 1.0], [1.5, 1.0]]]));
    // Not finite.
    let mut work = DirWork::default();
    assert_eq!(
        trims_overlap(
            &sq,
            &[vec![[f64::NAN, 0.0], [1.0, 1.0], [0.0, 1.0]]],
            &mut work
        ),
        None
    );
}

/// Random unions of rectangles on a small grid (as one rectangle with
/// another as a hole, or two side by side), against the grid's open
/// cells: the regions overlap iff some cell centre is inside both; and
/// the same under integer shears of determinant 1, which keep every
/// overlap.
#[test]
fn trims_overlap_matches_grid_cells() {
    let mut r = Rng::new(23);
    let mut hits = 0;
    for _ in 0..300 {
        let mut region = || {
            let rc = |r: &mut Rng| {
                let (a, b) = (r.below(6) as i64, r.below(6) as i64);
                let (c, d) = (r.below(6) as i64, r.below(6) as i64);
                let (x0, x1) = (a.min(b), a.max(b) + 1);
                let (y0, y1) = (c.min(d), c.max(d) + 1);
                [x0, y0, x1, y1]
            };
            let outer = rc(&mut r);
            let mut loops = vec![outer];
            if r.below(2) == 0 {
                let [x0, y0, x1, y1] = outer;
                if x1 - x0 >= 3 && y1 - y0 >= 3 {
                    loops.push([x0 + 1, y0 + 1, x1 - 1, y1 - 1]);
                }
            }
            loops
        };
        let (p, qq) = (region(), region());
        let cell_in = |loops: &[[i64; 4]], x: i64, y: i64| {
            let inside = |[x0, y0, x1, y1]: [i64; 4]| x >= x0 && x < x1 && y >= y0 && y < y1;
            inside(loops[0]) && !loops[1..].iter().any(|&h| inside(h))
        };
        let want = (0..8).any(|x| (0..8).any(|y| cell_in(&p, x, y) && cell_in(&qq, x, y)));
        hits += usize::from(want);
        for m in [[1, 0, 0, 1], [1, 2, 0, 1], [1, 0, -3, 1], [2, 1, 1, 1]] {
            let map = |loops: &[[i64; 4]]| -> Vec<Vec<[f64; 2]>> {
                loops
                    .iter()
                    .map(|&[x0, y0, x1, y1]| {
                        [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
                            .iter()
                            .map(|&[x, y]| {
                                [(m[0] * x + m[1] * y) as f64, (m[2] * x + m[3] * y) as f64]
                            })
                            .collect()
                    })
                    .collect()
            };
            assert_eq!(overlap(&map(&p), &map(&qq)), want, "{p:?} {qq:?} {m:?}");
        }
    }
    assert!(hits > 20 && hits < 290, "{hits}");
}

/// A face of the fixtures: on `surface`, its normal, its trims as
/// polygons in 3D (charted), its edges and vertices.
fn face(
    surface: u32,
    flipped: bool,
    normal: DVec3,
    outer: &[DVec3],
    edges: &[u32],
    verts: &[u32],
) -> GroupFace {
    GroupFace {
        surface: SurfId(surface),
        flipped,
        normal,
        trims: Some(chart(normal, &[outer.to_vec()])),
        edges: edges.iter().map(|&e| EdgeId(e)).collect(),
        verts: verts.iter().map(|&v| VertId(v)).collect(),
    }
}

fn edge(curve: u32, t: [f64; 2], verts: [u32; 2], faces: &[u32]) -> GroupEdge {
    GroupEdge {
        curve: CurveId(curve),
        t_range: t,
        verts: verts.map(VertId),
        faces: faces.iter().map(|&f| FaceId(f)).collect(),
    }
}

fn vert(at: DVec3, faces: &[u32]) -> GroupVert {
    GroupVert {
        at,
        r: 0.25,
        faces: faces.iter().map(|&f| FaceId(f)).collect(),
    }
}

fn groups(op: &Operand<'_>) -> Groups {
    let mut work = DirWork::default();
    let g = coincidence_groups(op, &mut work).expect("groups");
    assert!(work.checks > 0);
    g
}

/// Stacked blocks with a load-snapped joint at `z = 0`: the lower top
/// (`+z`) and the upper bottom (`−z`) on one plane object, opposite
/// `flipped`, overlapping: one group with their edges and vertices.
/// Shifted so they meet only along an edge, or with equal `flipped` (a
/// slotted plate's top in two pieces), they form none.
#[test]
fn opposed_faces_on_one_surface_group_when_their_trims_overlap() {
    let sq = |x0: f64, x1: f64| {
        vec![
            v3(x0, 0.0, 0.0),
            v3(x1, 0.0, 0.0),
            v3(x1, 10.0, 0.0),
            v3(x0, 10.0, 0.0),
        ]
    };
    let verts: Vec<GroupVert> = (0..8).map(|i| vert(v3(i as f64, 0.0, 0.0), &[])).collect();
    let edges = [
        edge(0, [0.0, 1.0], [0, 1], &[0]),
        edge(1, [0.0, 1.0], [4, 5], &[1]),
    ];
    let op = |faces: &[GroupFace]| -> Groups {
        groups(&Operand {
            verts: &verts,
            edges: &edges,
            coedges: &[],
            faces,
            contacts: &[],
        })
    };
    let lower = face(
        0,
        false,
        v3(0.0, 0.0, 1.0),
        &sq(0.0, 10.0),
        &[0],
        &[0, 1, 2, 3],
    );
    let upper = face(
        0,
        true,
        v3(0.0, 0.0, -1.0),
        &sq(0.0, 10.0),
        &[1],
        &[4, 5, 6, 7],
    );
    let g = op(&[lower.clone(), upper.clone()]);
    assert_eq!(g.groups.len(), 1);
    assert_eq!(g.groups[0].faces, [FaceId(0), FaceId(1)]);
    assert_eq!(g.groups[0].edges, [EdgeId(0), EdgeId(1)]);
    assert_eq!(g.groups[0].verts, (0..8).map(VertId).collect::<Vec<_>>());
    assert_eq!(g.of_vert(VertId(5)), Some(0));
    // Opposed faces never share a direction: n and −n.
    let mut work = DirWork::default();
    let normals = group_normals(
        &Operand {
            verts: &verts,
            edges: &edges,
            coedges: &[],
            faces: &[lower.clone(), upper.clone()],
            contacts: &[],
        },
        &g.groups[0],
    )
    .expect("ids");
    assert_eq!(
        group_dir_flat(&normals, &[], &mut work),
        Some(GroupDir::Zero)
    );
    // Touching along `x = 10` only.
    let beside = face(
        0,
        true,
        v3(0.0, 0.0, -1.0),
        &sq(10.0, 20.0),
        &[1],
        &[4, 5, 6, 7],
    );
    assert!(op(&[lower.clone(), beside]).groups.is_empty());
    // Equal `flipped`: a plate top split in two.
    let mut same = upper.clone();
    same.flipped = false;
    same.normal = v3(0.0, 0.0, 1.0);
    assert!(op(&[lower.clone(), same]).groups.is_empty());
    // Another surface object.
    let mut other = upper;
    other.surface = SurfId(1);
    assert!(op(&[lower.clone(), other.clone()]).groups.is_empty());
    // Opposite faces with trims that aren't straight are refused.
    other.surface = SurfId(0);
    other.trims = None;
    let mut work = DirWork::default();
    let refused = coincidence_groups(
        &Operand {
            verts: &verts,
            edges: &edges,
            coedges: &[],
            faces: &[lower, other],
            contacts: &[],
        },
        &mut work,
    );
    assert_eq!(
        refused,
        Err(GroupError::CurvedTrims([FaceId(0), FaceId(1)]))
    );
}

/// Edges on one curve group over overlapping ranges only (a split
/// edge's pieces touch at a knot); coedges on one p-curve group their
/// edges; contact members group; groups sharing a closure vertex are one;
/// the numbering follows the least member, whatever the input order.
#[test]
fn edges_coedges_and_contacts_group_by_structure() {
    let verts: Vec<GroupVert> = (0..10).map(|i| vert(v3(i as f64, 0.0, 0.0), &[])).collect();
    let edges = [
        edge(0, [0.0, 1.0], [0, 1], &[]),
        edge(0, [1.0, 2.0], [1, 2], &[]), // a split piece: touches edge 0 at t = 1
        edge(1, [0.0, 4.0], [3, 4], &[]),
        edge(1, [1.0, 2.0], [5, 6], &[]), // inside edge 2's range
        edge(1, [3.5, 5.0], [7, 8], &[]), // overlaps edge 2 at its end
        edge(2, [0.0, 1.0], [8, 9], &[]), // shares vertex 8 with edge 4
        edge(3, [0.0, 1.0], [0, 9], &[]),
    ];
    let op = Operand {
        verts: &verts,
        edges: &edges,
        coedges: &[],
        faces: &[],
        contacts: &[],
    };
    let g = groups(&op);
    assert_eq!(g.groups.len(), 1);
    assert_eq!(g.groups[0].edges, [EdgeId(2), EdgeId(3), EdgeId(4)]);
    assert_eq!(g.groups[0].verts, [3, 4, 5, 6, 7, 8].map(VertId));
    assert_eq!(g.of_edge(EdgeId(0)), None);
    assert_eq!(
        g.of_edge(EdgeId(5)),
        None,
        "a closure vertex doesn't pull in its other edges"
    );
    // The same edges in another order group alike.
    let mut shuffled = edges.to_vec();
    shuffled.swap(2, 4);
    let g2 = groups(&Operand {
        edges: &shuffled,
        ..op
    });
    assert_eq!(g2.groups[0].edges, [EdgeId(2), EdgeId(3), EdgeId(4)]);

    // Coedges on one p-curve: a slit's two sides.
    let coedges = [
        GroupCoedge {
            edge: EdgeId(0),
            pcurve: 7,
        },
        GroupCoedge {
            edge: EdgeId(6),
            pcurve: 7,
        },
        GroupCoedge {
            edge: EdgeId(1),
            pcurve: 8,
        },
    ];
    let g = groups(&Operand {
        coedges: &coedges,
        ..op
    });
    assert_eq!(g.groups.len(), 2);
    assert_eq!(g.groups[0].edges, [EdgeId(0), EdgeId(6)]);
    assert_eq!(g.groups[0].verts, [0, 1, 9].map(VertId));
    // Edge 5 shares vertex 9 with the slit and vertex 8 with the curve
    // group, but isn't a member: the groups stay apart.
    assert_eq!(g.groups[1].edges, [EdgeId(2), EdgeId(3), EdgeId(4)]);

    // A contact joining a vertex of each: one group.
    let contacts = [vec![Member::Vert(VertId(1)), Member::Vert(VertId(3))]];
    let g = groups(&Operand {
        coedges: &coedges,
        contacts: &contacts,
        ..op
    });
    assert_eq!(g.groups.len(), 1);
    assert_eq!(g.of_vert(VertId(3)), g.of_edge(EdgeId(6)));
    // An id past the elements.
    let bad = [vec![Member::Edge(EdgeId(70)), Member::Vert(VertId(0))]];
    let mut work = DirWork::default();
    assert_eq!(
        coincidence_groups(
            &Operand {
                contacts: &bad,
                ..op
            },
            &mut work
        ),
        Err(GroupError::Data)
    );
}

fn key() -> TieKey {
    TieKey {
        row: Row::ColumnHit,
        record: RecordKey::Vv(VvKey {
            a: Query::Vert(VertId(1)),
            b: Query::Vert(VertId(2)),
        }),
    }
}

/// The sign of `n·v` at the first order that isn't zero, with `v = σ·d +
/// η·T₂ + η²·T₃ + η³·T₄` and `η = 2⁻¹⁶`: which side of the plane `n·(x −
/// p) = 0` a point `p` on it moves to (`Pos` along `n`). A plane of `B`
/// is fixed.
fn moved_side(n: DVec3, sigma: Sign, d: &Q3) -> Sign {
    let eta = Q::from(1u32) >> 16u64;
    let s = if sigma == Sign::Pos {
        Q::from(1)
    } else {
        Q::from(-1)
    };
    let mut v: Q3 = d.clone().map(|x| x * &s);
    let mut e = eta.clone();
    for t in T {
        let t = qv(t);
        for i in 0..3 {
            v[i] += &e * &t[i];
        }
        e *= &eta;
    }
    if dot(&qv(n), &v) > 0u32 {
        Sign::Pos
    } else {
        Sign::Neg
    }
}

/// The height record of the `B` plane through `at` with outward normal
/// `n` over the `A` point `at` moving along `σ·d`, decided; checked
/// against the moved configuration: the height above the moved point is
/// `−n·(δv)/(n·UP)`. Returns the side of the plane the point moves to.
fn decided_side(n: DVec3, at: DVec3, sigma: Sign, d: &Q3) -> Sign {
    let rec = PlaneHeight {
        normal: n,
        offset: n.dot(at),
        rates: [Rate::ZERO; ORDERS],
        q: Moving {
            at,
            rates: rates_a(sigma, Rate::cleared(d.clone())),
        },
    };
    let got = decide_flat(&rec, key())
        .0
        .expect("decided, never the guard");
    let side = moved_side(n, sigma, d);
    let up = n.dot(UP) > 0.0;
    // The plane above the moved point iff it moved below the plane along
    // `UP`.
    let want = if (side == Sign::Neg) == up {
        Sign::Pos
    } else {
        Sign::Neg
    };
    assert_eq!(got, want, "{n} {at} {sigma:?} {d:?}");
    side
}

/// The edge-contact cubes (r8-sound S5): `[0,1]³` and `[1,2]×[1,2]×[0,1]`
/// with a contact along their edges at `x = y = 1`, `∪` a box `B` filling
/// the notch, `[1,2]×[0,1]×[0,1]`. The contact's edges are one group
/// with no common direction (their faces' normals `±x`, `±y`, `±z`); the
/// flush-patch rule over the two faces opposed to `B`'s takes the
/// bisector, so both cubes' edges move into `B` (fused) under `∪` and out
/// of it otherwise, every record at one rate.
#[test]
fn edge_contact_cubes_move_as_one_into_the_box() {
    // Faces: 0 cube1 +x, 1 cube1 +y, 2 cube1 +z, 3 cube1 −z, 4 cube2 −x,
    // 5 cube2 −y, 6 cube2 +z, 7 cube2 −z.
    let normals = [
        v3(1.0, 0.0, 0.0),
        v3(0.0, 1.0, 0.0),
        v3(0.0, 0.0, 1.0),
        v3(0.0, 0.0, -1.0),
        v3(-1.0, 0.0, 0.0),
        v3(0.0, -1.0, 0.0),
        v3(0.0, 0.0, 1.0),
        v3(0.0, 0.0, -1.0),
    ];
    let faces: Vec<GroupFace> = normals
        .iter()
        .enumerate()
        .map(|(i, &n)| GroupFace {
            surface: SurfId(i as u32),
            flipped: false,
            normal: n,
            trims: None,
            edges: vec![],
            verts: vec![],
        })
        .collect();
    let verts = [
        vert(v3(1.0, 1.0, 0.0), &[0, 1, 3]),
        vert(v3(1.0, 1.0, 1.0), &[0, 1, 2]),
        vert(v3(1.0, 1.0, 0.0), &[4, 5, 7]),
        vert(v3(1.0, 1.0, 1.0), &[4, 5, 6]),
    ];
    let edges = [
        edge(0, [0.0, 1.0], [0, 1], &[0, 1]),
        edge(1, [0.0, 1.0], [2, 3], &[4, 5]),
    ];
    let contacts = [vec![Member::Edge(EdgeId(0)), Member::Edge(EdgeId(1))]];
    let op = Operand {
        verts: &verts,
        edges: &edges,
        coedges: &[],
        faces: &faces,
        contacts: &contacts,
    };
    let g = groups(&op);
    assert_eq!(g.groups.len(), 1);
    let group = &g.groups[0];
    assert_eq!(group.verts.len(), 4);
    let ns = group_normals(&op, group).expect("ids");
    assert_eq!(ns.len(), 8);
    let mut work = DirWork::default();
    // B's faces at the edge: `x = 1` (outward −x) against cube1's +x, and
    // `y = 1` (outward +y) against cube2's −y: both opposed.
    let patches = [
        Patch {
            normal: normals[0],
            side: PatchSide::Opposed,
        },
        Patch {
            normal: normals[5],
            side: PatchSide::Opposed,
        },
    ];
    let dg = group_dir_flat(&ns, &patches, &mut work).expect("finite");
    let GroupDir::Flush(d) = &dg else {
        panic!("{dg:?}")
    };
    assert!(agrees(&[normals[0], normals[5]], d));
    assert!(work.solves > 0);
    // Without patches: no direction, the translations decide.
    assert_eq!(group_dir_flat(&ns, &[], &mut work), Some(GroupDir::Zero));
    let b_planes = [v3(-1.0, 0.0, 0.0), v3(0.0, 1.0, 0.0)];
    for sigma in [Sign::Pos, Sign::Neg] {
        // Points of both cubes' edges, ends and middles: one rate.
        for at in [v3(1.0, 1.0, 0.0), v3(1.0, 1.0, 0.5), v3(1.0, 1.0, 1.0)] {
            let inside = b_planes
                .iter()
                .all(|&n| decided_side(n, at, sigma, &dg.d()) == Sign::Neg);
            assert_eq!(inside, sigma == Sign::Pos, "{at} {sigma:?}");
        }
    }
}

/// The slit of a wedge resting on a plate (r8-sound S5 `slit ∩ box`,
/// r9-walk W4): the plate top `P.top` (`+z`) and the wedge's faces `W1`,
/// `W2` (`(∓1, 0, −1)`) meet the slit's two edges, whose coedges on
/// `P.top` share one p-curve. No direction agrees with all three.
/// - `∩` a box whose corner is on the slit, no patch: `d = 0`, every
///   record decided by the translations (never the guard).
/// - `− H` with `H`'s cap flush with `P.top` (W4): the flush-patch rule
///   gives `+ẑ`, and the slit's points move into `H` whichever sign
///   `T₂·ẑ` has (the configuration and its mirror image in `z`), where
///   `d = 0` would have the translations put them on either side.
#[test]
fn a_slit_moves_as_one_by_its_flush_patch() {
    for mirror in [1.0, -1.0] {
        let z = |v: DVec3| v3(v.x, v.y, mirror * v.z);
        // Faces: 0 P.top, 1 W1, 2 W2.
        let normals = [
            z(v3(0.0, 0.0, 1.0)),
            z(v3(-1.0, 0.0, -1.0)),
            z(v3(1.0, 0.0, -1.0)),
        ];
        let faces: Vec<GroupFace> = normals
            .iter()
            .enumerate()
            .map(|(i, &n)| GroupFace {
                surface: SurfId(i as u32),
                flipped: false,
                normal: n,
                trims: None,
                edges: vec![],
                verts: vec![],
            })
            .collect();
        let verts = [
            vert(v3(0.0, -3.0, 0.0), &[0, 1]),
            vert(v3(0.0, 3.0, 0.0), &[0, 1]),
            vert(v3(0.0, -3.0, 0.0), &[0, 2]),
            vert(v3(0.0, 3.0, 0.0), &[0, 2]),
        ];
        let edges = [
            edge(0, [0.0, 1.0], [0, 1], &[0, 1]),
            edge(1, [0.0, 1.0], [2, 3], &[0, 2]),
        ];
        let coedges = [
            GroupCoedge {
                edge: EdgeId(0),
                pcurve: 4,
            },
            GroupCoedge {
                edge: EdgeId(1),
                pcurve: 4,
            },
        ];
        let op = Operand {
            verts: &verts,
            edges: &edges,
            coedges: &coedges,
            faces: &faces,
            contacts: &[],
        };
        let g = groups(&op);
        assert_eq!(g.groups.len(), 1);
        let ns = group_normals(&op, &g.groups[0]).expect("ids");
        let mut work = DirWork::default();

        // `∩` a box cornered on the slit at the origin, below the plate
        // top: no patch.
        let dg = group_dir_flat(&ns, &[], &mut work).expect("finite");
        assert_eq!(dg, GroupDir::Zero);
        let corner = [
            z(v3(2.0, 0.0, 1.0)),
            z(v3(-1.0, 2.0, 1.0)),
            z(v3(-1.0, -2.0, 1.0)),
        ];
        for &n in &corner {
            decided_side(n, v3(0.0, 0.0, 0.0), Sign::Neg, &dg.d());
        }

        // `− H`, `H`'s cap co-oriented with `P.top`.
        let cap = normals[0];
        let patches = [Patch {
            normal: cap,
            side: PatchSide::CoOriented,
        }];
        let dg = group_dir_flat(&ns, &patches, &mut work).expect("finite");
        assert_eq!(dg, GroupDir::Flush(qv(z(v3(0.0, 0.0, 1.0)))));
        let mut zero_sides = Vec::new();
        for at in [v3(0.0, -3.0, 0.0), v3(0.0, 0.0, 0.0), v3(0.0, 3.0, 0.0)] {
            // Into `H`: below its cap along the cap's normal.
            assert_eq!(decided_side(cap, at, Sign::Neg, &dg.d()), Sign::Neg);
            zero_sides.push(decided_side(cap, at, Sign::Neg, &GroupDir::Zero.d()));
        }
        // With `d = 0` the translations decide: `T₂·n_cap`'s sign.
        let t2 = qv(T[0]);
        let t2_side = if dot(&qv(cap), &t2) > 0u32 {
            Sign::Pos
        } else {
            Sign::Neg
        };
        assert!(zero_sides.iter().all(|&s| s == t2_side));
        // One frame of the two (`T₂·ẑ > 0`) puts the slit outside `H` at
        // `d = 0`, its mirror image inside.
        assert_eq!(t2_side == Sign::Pos, mirror > 0.0);
    }
}

/// The slotted plate of r9-sound S6: the top in two pieces on one plane
/// object, equal `flipped`, forms no group, so the elements at the `+x`
/// side face keep their own directions. With `B` flush against that face
/// (up to the top), the side patch is decided uniformly at order 1 by
/// the face, the top edge and the top corner: all enter `B` under `∪`.
#[test]
fn a_slotted_plate_top_is_no_group_and_its_side_patch_is_uniform() {
    // Faces: 0 top left, 1 top right (slot between `x ∈ (4, 6)`), 2 side
    // `+x` at `x = 10`, 3 front `−y`.
    let top = v3(0.0, 0.0, 1.0);
    let faces = [
        face(
            0,
            false,
            top,
            &[
                v3(0.0, 0.0, 5.0),
                v3(4.0, 0.0, 5.0),
                v3(4.0, 10.0, 5.0),
                v3(0.0, 10.0, 5.0),
            ],
            &[],
            &[],
        ),
        face(
            0,
            false,
            top,
            &[
                v3(6.0, 0.0, 5.0),
                v3(10.0, 0.0, 5.0),
                v3(10.0, 10.0, 5.0),
                v3(6.0, 10.0, 5.0),
            ],
            &[0],
            &[0],
        ),
        face(
            1,
            false,
            v3(1.0, 0.0, 0.0),
            &[
                v3(10.0, 0.0, 0.0),
                v3(10.0, 10.0, 0.0),
                v3(10.0, 10.0, 5.0),
                v3(10.0, 0.0, 5.0),
            ],
            &[0],
            &[0],
        ),
        face(
            2,
            false,
            v3(0.0, -1.0, 0.0),
            &[
                v3(0.0, 0.0, 0.0),
                v3(10.0, 0.0, 0.0),
                v3(10.0, 0.0, 5.0),
                v3(0.0, 0.0, 5.0),
            ],
            &[],
            &[0],
        ),
    ];
    let verts = [
        vert(v3(10.0, 0.0, 5.0), &[1, 2, 3]),
        vert(v3(10.0, 10.0, 5.0), &[1, 2]),
    ];
    let edges = [edge(0, [0.0, 10.0], [0, 1], &[1, 2])];
    let op = Operand {
        verts: &verts,
        edges: &edges,
        coedges: &[],
        faces: &faces,
        contacts: &[],
    };
    assert!(groups(&op).groups.is_empty());
    let mut work = DirWork::default();
    let own = |idx: &[usize], work: &mut DirWork| {
        let ns: Vec<DVec3> = idx.iter().map(|&i| faces[i].normal).collect();
        dir::flat(&ns, work).expect("finite").d()
    };
    let b_face = v3(-1.0, 0.0, 0.0);
    for (at, idx) in [
        (v3(10.0, 5.0, 2.0), &[2usize][..]),
        (v3(10.0, 5.0, 5.0), &[1, 2][..]),
        (v3(10.0, 0.0, 5.0), &[1, 2, 3][..]),
    ] {
        let d = own(idx, &mut work);
        assert_eq!(decided_side(b_face, at, Sign::Pos, &d), Sign::Neg, "{at}");
    }
}

/// The blend of a non-member near a group: `d_G` within `r_G` of the
/// nearest member vertex or edge, the element's own beyond `2r_G`, a mix
/// between, only where `d_G` agrees with the element's faces; a group of
/// no direction leaves it its own. Every blended direction agrees with
/// the element's faces, and records reading it are the moved
/// configuration's signs.
#[test]
fn non_members_blend_with_the_group_only_where_it_agrees() {
    let verts = [vert(v3(0.0, 0.0, 0.0), &[]), vert(v3(4.0, 0.0, 0.0), &[])];
    let edges = [
        edge(0, [0.0, 1.0], [0, 1], &[]),
        edge(0, [0.5, 2.0], [0, 1], &[]),
    ];
    let op = Operand {
        verts: &verts,
        edges: &edges,
        coedges: &[],
        faces: &[],
        contacts: &[],
    };
    let g = groups(&op);
    let reach = Reach::of(&op, &g.groups[0]).expect("finite");
    let mut work = DirWork::default();
    let at = |x: f64, y: f64, z: f64| qv(v3(x, y, z));
    // r_G = 1/4: the segment's middle within it, beyond 2r_G, between.
    assert_eq!(reach.weight(&at(2.0, 0.25, 0.0), &mut work), Q::from(0));
    assert_eq!(reach.weight(&at(2.0, 0.0, 0.5), &mut work), Q::from(1));
    assert_eq!(
        reach.weight(&at(2.0, 0.375, -0.1), &mut work),
        Q::from(1u32) / Q::from(2u32)
    );
    // Past the segment's end, the nearer vertex's distance.
    assert_eq!(
        reach.weight(&at(-0.375, 0.0, 0.0), &mut work),
        Q::from(1u32) / Q::from(2u32)
    );
    // A skew point, off the segment by 3/8 in `y` and `z`.
    assert_eq!(
        reach.weight(&at(2.0, 0.375, 0.375), &mut work),
        Q::from(1u32) / Q::from(2u32)
    );

    let d_g = GroupDir::Common(at(0.0, 0.0, 1.0));
    let x = at(2.0, 0.375, 0.0);
    // A face `+y` near the group: `+z` doesn't agree, its own stays.
    let own_y = at(0.0, 1.0, 0.0);
    let ny = [v3(0.0, 1.0, 0.0)];
    assert_eq!(blend_flat(&d_g, &reach, &own_y, &ny, &x, &mut work), own_y);
    // A face `(0, 1, 1)`: agrees, the mix at λ = 1/2.
    let n_yz = [v3(0.0, 1.0, 1.0)];
    let own_yz = at(0.0, 0.5, 0.5);
    let mixed = blend_flat(&d_g, &reach, &own_yz, &n_yz, &x, &mut work);
    assert_eq!(
        mixed,
        [
            Q::from(0),
            Q::from(1u32) / Q::from(4u32),
            Q::from(3u32) / Q::from(4u32)
        ]
    );
    assert!(agrees(&n_yz, &mixed));
    for sigma in [Sign::Pos, Sign::Neg] {
        let side = decided_side(v3(0.0, -1.0, 0.0), v3(2.0, 0.375, 0.0), sigma, &mixed);
        assert_eq!(side == Sign::Neg, sigma == Sign::Pos);
    }
    // No direction, or far away: its own.
    for dg in [GroupDir::Zero, GroupDir::Opposed] {
        assert_eq!(
            blend_flat(&dg, &reach, &own_yz, &n_yz, &x, &mut work),
            own_yz
        );
    }
    let far = at(2.0, 3.0, 0.0);
    assert_eq!(
        blend_flat(&d_g, &reach, &own_yz, &n_yz, &far, &mut work),
        own_yz
    );
}

/// The max-norm distance to a segment against a dense sampling of it:
/// never above any sample, within the sampling's step of the least.
#[test]
fn segment_distances_are_the_least_over_the_segment() {
    let mut r = Rng::new(5);
    for _ in 0..200 {
        let mut p = || qv(v3(r.int(4) as f64, r.int(4) as f64, r.int(4) as f64));
        let (a, b, x) = (p(), p(), p());
        let exact = seg_dist_inf(&a, &b, &x);
        let n = 256;
        let mut least: Option<Q> = None;
        for k in 0..=n {
            let t = Q::from(k) / Q::from(n);
            let pt: Q3 = [0, 1, 2].map(|i| &a[i] + &t * (&b[i] - &a[i]));
            let d = dist_inf(&pt, &x);
            assert!(exact <= d);
            if least.as_ref().is_none_or(|l| d < *l) {
                least = Some(d);
            }
        }
        // Lipschitz in t by at most the segment's largest component, 8.
        let step = Q::from(8u32) / Q::from(n);
        assert!(least.expect("samples") - &exact <= step);
    }
}
