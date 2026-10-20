//! Arrangements of squares and triangles: nested, side by side, split by a
//! chord, touching at a vertex, with a dangling edge, and a container the
//! ray from an island meets at its vertices and along a span; the exact
//! rotation order at a vertex; each refusal with its entity; the same
//! arrangement under any relabelling of vertices and edges; running out
//! of budget; and a face of the planar sheet store arranged from its
//! stored UVs.

use varde_testkit::rng::Rng;

use super::*;
use crate::infra::budget::{Budget, Work};
use crate::topo::brep_min::{FaceId, sheets};

fn arr(verts: &[[f64; 2]], edges: &[Edge]) -> Result<Arrangement, ArrangeError> {
    arrange(verts, edges, &mut Work::new(&Budget::DEFAULT))
}

/// Vertices and edges, built up from closed rings and single edges.
#[derive(Clone, Debug, Default)]
struct Fig {
    verts: Vec<[f64; 2]>,
    edges: Vec<Edge>,
}

impl Fig {
    fn vert(&mut self, p: [f64; 2]) -> u32 {
        self.verts.push(p);
        (self.verts.len() - 1) as u32
    }

    fn edge(&mut self, a: u32, b: u32, wind: i32) {
        self.edges.push(Edge { ends: [a, b], wind });
    }

    /// A closed ring through `pts` in order, each edge stepping `wind`;
    /// its vertices.
    fn ring(&mut self, pts: &[[f64; 2]], wind: i32) -> Vec<u32> {
        let vs: Vec<u32> = pts.iter().map(|&p| self.vert(p)).collect();
        for k in 0..vs.len() {
            self.edge(vs[k], vs[(k + 1) % vs.len()], wind);
        }
        vs
    }

    fn build(&self) -> Result<Arrangement, ArrangeError> {
        arr(&self.verts, &self.edges)
    }
}

/// The counter-clockwise square with low corner `(x, y)` and side `s`.
fn square(x: f64, y: f64, s: f64) -> [[f64; 2]; 4] {
    [[x, y], [x + s, y], [x + s, y + s], [x, y + s]]
}

/// `pts` in reverse order.
fn cw(pts: &[[f64; 2]]) -> Vec<[f64; 2]> {
    pts.iter().rev().copied().collect()
}

/// The positions of cycle `c`'s vertices, from where it was found.
fn cycle_pts(a: &Arrangement, f: &Fig, c: u32) -> Vec<[f64; 2]> {
    a.cycle(c).map(|h| f.verts[a.origin(h) as usize]).collect()
}

/// Component `k`'s bounded face cycles.
fn bounded(a: &Arrangement, k: u32) -> Vec<u32> {
    (0..a.cycle_count() as u32)
        .filter(|&c| a.cycle_component(c) == k && a.is_bounded(c))
        .collect()
}

#[test]
fn square_bounds_one_region() {
    let mut f = Fig::default();
    f.ring(&square(0.0, 0.0, 1.0), 1);
    let a = f.build().unwrap();
    assert_eq!((a.component_count(), a.cycle_count()), (1, 2));
    // Found at the least vertex, the half-edge along +u first: the
    // counter-clockwise inside, then the clockwise outside.
    assert_eq!(a.outer_cycle(0), 1);
    assert!(a.is_bounded(0) && !a.is_bounded(1));
    assert_eq!(cycle_pts(&a, &f, 0), square(0.0, 0.0, 1.0));
    assert_eq!(
        cycle_pts(&a, &f, 1),
        [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]
    );
    assert_eq!(
        a.regions(),
        [
            Region {
                outer: NONE,
                holes: vec![1],
                winding: 0
            },
            Region {
                outer: 0,
                holes: vec![],
                winding: 1
            },
        ]
    );
    assert_eq!((a.cycle_region(0), a.cycle_region(1)), (1, 0));
    assert!(a.loop_in().is_empty());
    assert_eq!(a.container(0), None);
}

#[test]
fn hole_nests_in_its_square() {
    let mut f = Fig::default();
    f.ring(&square(0.0, 0.0, 10.0), 1);
    // A hole runs clockwise: the material is on its left.
    f.ring(&cw(&square(4.0, 4.0, 2.0)), 1);
    let a = f.build().unwrap();
    assert_eq!(a.component_count(), 2);
    assert_eq!((bounded(&a, 0), bounded(&a, 1)), (vec![0], vec![2]));
    // The square's least vertex lies outside the hole's box: not asked.
    assert_eq!(
        a.loop_in(),
        [LoopIn {
            component: 1,
            cycle: 0,
            inside: true
        }]
    );
    assert_eq!((a.container(0), a.container(1)), (None, Some(0)));
    let r = a.regions();
    assert_eq!(r.len(), 3);
    assert_eq!((r[0].holes.clone(), r[0].winding), (vec![1], 0));
    assert_eq!(
        (r[1].outer, r[1].holes.clone(), r[1].winding),
        (0, vec![3], 1)
    );
    assert_eq!(
        (r[2].outer, r[2].holes.clone(), r[2].winding),
        (2, vec![], 0)
    );
    assert_eq!(a.cycle_region(3), 1);
}

#[test]
fn nested_squares_take_the_innermost_container() {
    let mut f = Fig::default();
    f.ring(&square(0.0, 0.0, 30.0), 1);
    f.ring(&square(5.0, 5.0, 20.0), 1);
    f.ring(&square(10.0, 10.0, 10.0), 1);
    f.ring(&square(26.0, 1.0, 2.0), 1);
    let a = f.build().unwrap();
    assert_eq!(a.component_count(), 4);
    for k in 0..4 {
        assert_eq!(bounded(&a, k), [2 * k]);
    }
    // The small square lies in the large one and the middle one; the
    // middle one is itself in the large one, so it is the innermost.
    let ins: Vec<(u32, u32)> = a
        .loop_in()
        .iter()
        .filter(|r| r.inside)
        .map(|r| (r.component, r.cycle))
        .collect();
    assert_eq!(ins, [(1, 0), (2, 0), (2, 2), (3, 0)]);
    assert_eq!(
        (0..4).map(|k| a.container(k)).collect::<Vec<_>>(),
        [None, Some(0), Some(2), Some(0)]
    );
    let r = a.regions();
    let got: Vec<(u32, Vec<u32>, i64)> = r
        .iter()
        .map(|r| (r.outer, r.holes.clone(), r.winding))
        .collect();
    assert_eq!(
        got,
        [
            (NONE, vec![1], 0),
            (0, vec![3, 7], 1),
            (2, vec![5], 2),
            (4, vec![], 3),
            (6, vec![], 2),
        ]
    );
}

#[test]
fn ray_through_container_vertices_counts_the_spans_above() {
    let mut f = Fig::default();
    // Counter-clockwise; on the line `v = 0` of the island's least vertex
    // `(0, 0)` it has a vertex touched from below at `u = 3`, a span on
    // the line from `(12, 0)` to `(11, 0)` leaving upwards, and a vertex
    // touched from above at `u = 9`.
    f.ring(
        &[
            [-5.0, -5.0],
            [2.0, -5.0],
            [3.0, 0.0],
            [4.0, -5.0],
            [12.0, -5.0],
            [12.0, 0.0],
            [11.0, 0.0],
            [10.0, 3.0],
            [9.0, 0.0],
            [8.0, 3.0],
            [-5.0, 5.0],
        ],
        1,
    );
    f.ring(&[[0.0, 0.0], [1.0, -0.5], [1.0, 0.5]], 1);
    // In the dent from above, outside.
    f.ring(&[[8.8, 2.0], [9.2, 1.8], [9.2, 2.2]], 1);
    let a = f.build().unwrap();
    let c = bounded(&a, 0);
    assert_eq!(c.len(), 1);
    assert_eq!(
        a.loop_in(),
        [
            LoopIn {
                component: 1,
                cycle: c[0],
                inside: true
            },
            LoopIn {
                component: 2,
                cycle: c[0],
                inside: false
            },
        ]
    );
    assert_eq!((a.container(1), a.container(2)), (Some(c[0]), None));
    let w: Vec<i64> = a.regions().iter().map(|r| r.winding).collect();
    assert_eq!(w, [0, 1, 2, 1]);
}

#[test]
fn island_takes_the_face_of_a_split_square_it_lies_in() {
    let mut f = Fig::default();
    let s = f.ring(&square(0.0, 0.0, 4.0), 1);
    f.edge(s[0], s[2], 0);
    f.ring(&[[2.5, 0.5], [3.5, 0.5], [3.5, 1.5]], 1);
    let a = f.build().unwrap();
    let faces = bounded(&a, 0);
    assert_eq!(faces.len(), 2);
    // Both halves' boxes hold the island's least vertex: both asked.
    let asked: Vec<(u32, bool)> = a.loop_in().iter().map(|r| (r.cycle, r.inside)).collect();
    assert_eq!(asked.len(), 2);
    let lower = faces
        .iter()
        .copied()
        .find(|&c| cycle_pts(&a, &f, c).contains(&[4.0, 0.0]))
        .unwrap();
    for (c, inside) in asked {
        assert_eq!(inside, c == lower);
    }
    assert_eq!(a.container(1), Some(lower));
    let w: Vec<i64> = a.regions().iter().map(|r| r.winding).collect();
    assert_eq!(w, [0, 1, 1, 2]);
}

#[test]
fn figure_eight_is_one_component() {
    let mut f = Fig::default();
    let s = f.vert([2.0, 2.0]);
    let (a0, a1) = (f.vert([0.0, 0.0]), f.vert([0.0, 4.0]));
    let (b0, b1) = (f.vert([4.0, 0.0]), f.vert([4.0, 4.0]));
    for (p, q) in [(a0, s), (s, a1), (a1, a0), (s, b0), (b0, b1), (b1, s)] {
        f.edge(p, q, 1);
    }
    let a = f.build().unwrap();
    assert_eq!((a.component_count(), a.cycle_count()), (1, 3));
    let around: Vec<[f64; 2]> = a
        .rotation(s)
        .iter()
        .map(|&h| f.verts[a.dest(h) as usize])
        .collect();
    assert_eq!(around, [[4.0, 4.0], [0.0, 4.0], [0.0, 0.0], [4.0, 0.0]]);
    let r = a.regions();
    assert_eq!(r.len(), 3);
    assert!(r[1..].iter().all(|r| r.winding == 1 && r.holes.is_empty()));
    assert_eq!(r[0].holes, [a.outer_cycle(0)]);
    // The outer cycle passes the shared vertex twice.
    assert_eq!(
        a.cycle(a.outer_cycle(0))
            .filter(|&h| a.origin(h) == s)
            .count(),
        2
    );
}

#[test]
fn dangling_edge_stays_in_its_face() {
    let mut f = Fig::default();
    let s = f.ring(&square(0.0, 0.0, 10.0), 1);
    let tip = f.vert([3.0, 3.0]);
    f.edge(s[0], tip, 0);
    f.ring(&[[6.0, 2.0], [8.0, 2.0], [8.0, 4.0]], 1);
    let a = f.build().unwrap();
    let faces = bounded(&a, 0);
    assert_eq!(faces.len(), 1);
    // The face's cycle runs out along the dangling edge and back.
    let along: Vec<u32> = a.cycle(faces[0]).filter(|&h| a.edge(h) == 4).collect();
    assert_eq!(along, [8, 9]);
    assert_eq!(a.rotation(tip), [9]);
    assert_eq!(a.container(1), Some(faces[0]));
    let w: Vec<i64> = a.regions().iter().map(|r| r.winding).collect();
    assert_eq!(w, [0, 1, 2]);
}

/// The rotation order at `(1, 1)` of edges to points one ulp off a common
/// direction, in every input order and either edge direction.
#[test]
fn rotation_order_is_exact_and_independent_of_input_order() {
    let e = 2.0 * f64::EPSILON; // an ulp at 3
    let ends = [
        [1.0, 0.0],
        [3.0, 3.0 + 2.0 * e],
        [0.0, 1.0],
        [3.0, 3.0],
        [2.0, 1.0],
        [3.0, 3.0 - e],
    ];
    let want = [
        [2.0, 1.0],
        [3.0, 3.0 - e],
        [3.0, 3.0],
        [3.0, 3.0 + 2.0 * e],
        [0.0, 1.0],
        [1.0, 0.0],
    ];
    let mut rng = Rng::new(7);
    for _ in 0..64 {
        let mut f = Fig::default();
        let v = f.vert([1.0, 1.0]);
        let mut order: Vec<usize> = (0..ends.len()).collect();
        shuffle(&mut rng, &mut order);
        for &i in &order {
            let w = f.vert(ends[i]);
            if rng.next_u64() % 2 == 0 {
                f.edge(v, w, 0);
            } else {
                f.edge(w, v, 0);
            }
        }
        let a = f.build().unwrap();
        let got: Vec<[f64; 2]> = a
            .rotation(v)
            .iter()
            .map(|&h| f.verts[a.dest(h) as usize])
            .collect();
        assert_eq!(got, want);
        // A tree: one cycle, unbounded, one region.
        assert_eq!((a.cycle_count(), a.regions().len()), (1, 1));
    }
}

#[test]
fn coincident_directions_are_refused() {
    // The same pair of vertices twice.
    let mut f = Fig::default();
    let s = f.ring(&square(0.0, 0.0, 1.0), 1);
    f.edge(s[0], s[1], 0);
    assert_eq!(
        f.build(),
        Err(ArrangeError::Coincident {
            vertex: 0,
            edges: [0, 4]
        })
    );
    // Overlapping spans from one vertex, exactly collinear.
    let mut f = Fig::default();
    let v = f.vert([0.5, 0.5]);
    let (p, q) = (f.vert([12.0, 12.0]), f.vert([24.0, 24.0]));
    f.edge(v, p, 0);
    f.edge(q, v, 0);
    assert_eq!(
        f.build(),
        Err(ArrangeError::Coincident {
            vertex: 0,
            edges: [0, 1]
        })
    );
}

#[test]
fn bad_input_is_refused_with_its_entity() {
    let e = |a, b| Edge {
        ends: [a, b],
        wind: 0,
    };
    let pts = [[0.0, 0.0], [1.0, 0.0], [1.0, 0.0], [-0.0, 0.0]];
    assert_eq!(
        arr(&pts, &[e(0, 1), e(1, 1)]),
        Err(ArrangeError::SelfLoop(1))
    );
    assert_eq!(arr(&pts, &[e(0, 4)]), Err(ArrangeError::Index(0)));
    assert_eq!(
        arr(&pts, &[e(0, 1), e(1, 2)]),
        Err(ArrangeError::ZeroLength(1))
    );
    // `−0` and `+0` are one point.
    assert_eq!(arr(&pts, &[e(3, 0)]), Err(ArrangeError::ZeroLength(0)));
    let mut bad = pts;
    bad[2][1] = f64::NAN;
    assert_eq!(arr(&bad, &[e(0, 1)]), Err(ArrangeError::Coord(2)));
    bad[2][1] = 2.0 * MAX_INPUT;
    assert_eq!(arr(&bad, &[e(0, 1)]), Err(ArrangeError::Coord(2)));
}

#[test]
fn island_touching_a_cycle_is_refused() {
    // Its least vertex inside the square's bottom edge.
    let mut f = Fig::default();
    f.ring(&square(0.0, 0.0, 4.0), 1);
    f.ring(&[[2.0, 0.0], [3.0, 1.0], [2.5, 2.0]], 1);
    assert_eq!(
        f.build(),
        Err(ArrangeError::Touch {
            component: 1,
            cycle: 0
        })
    );
    // At the square's corner, another vertex at `−0`: the island comes
    // first, `−0` before `+0`.
    let mut f = Fig::default();
    f.ring(&square(0.0, 0.0, 4.0), 1);
    f.ring(&[[-0.0, 0.0], [2.0, 1.0], [1.0, 2.0]], 1);
    assert_eq!(
        f.build(),
        Err(ArrangeError::Touch {
            component: 0,
            cycle: 2
        })
    );
}

#[test]
fn self_crossing_ring_is_refused() {
    // A bow-tie without a vertex where it crosses; the island lies in the
    // lobe it winds round negatively.
    let mut f = Fig::default();
    f.ring(&[[0.0, 0.0], [2.0, 2.0], [2.0, 0.0], [0.0, 2.0]], 1);
    f.ring(&[[1.6, 1.0], [1.9, 0.8], [1.9, 1.2]], 1);
    assert_eq!(
        f.build(),
        Err(ArrangeError::Crossing {
            component: 1,
            cycle: 0
        })
    );
}

#[test]
fn unbalanced_windings_are_refused() {
    let mut f = Fig::default();
    f.ring(&square(0.0, 0.0, 1.0), 1);
    f.edges[3].wind = 2;
    assert!(matches!(f.build(), Err(ArrangeError::Winding(_))));
    // A dangling edge has one face on both sides: its step must be 0.
    let mut f = Fig::default();
    let s = f.ring(&square(0.0, 0.0, 4.0), 1);
    let tip = f.vert([1.0, 2.0]);
    f.edge(s[0], tip, 1);
    assert_eq!(f.build(), Err(ArrangeError::Winding(4)));
}

/// Every fixture above that builds, side by side, `100` apart in `v`.
fn composite() -> Fig {
    let mut f = Fig::default();
    let mut at = 0.0;
    let mut place = |f: &mut Fig, rings: &[(&[[f64; 2]], i32)]| {
        for &(pts, w) in rings {
            let moved: Vec<[f64; 2]> = pts.iter().map(|p| [p[0], p[1] + at]).collect();
            f.ring(&moved, w);
        }
        at += 100.0;
    };
    place(
        &mut f,
        &[
            (&square(0.0, 0.0, 30.0), 1),
            (&square(5.0, 5.0, 20.0), 1),
            (&cw(&square(10.0, 10.0, 10.0)), 1),
            (&square(26.0, 1.0, 2.0), 1),
        ],
    );
    place(
        &mut f,
        &[
            (
                &[
                    [-5.0, -5.0],
                    [2.0, -5.0],
                    [3.0, 0.0],
                    [4.0, -5.0],
                    [12.0, -5.0],
                    [12.0, 0.0],
                    [11.0, 0.0],
                    [10.0, 3.0],
                    [9.0, 0.0],
                    [8.0, 3.0],
                    [-5.0, 5.0],
                ],
                1,
            ),
            (&[[0.0, 0.0], [1.0, -0.5], [1.0, 0.5]], 1),
            (&[[8.8, 2.0], [9.2, 1.8], [9.2, 2.2]], 1),
        ],
    );
    let base = f.verts.len() as u32;
    place(
        &mut f,
        &[
            (&square(0.0, 0.0, 4.0), 1),
            (&[[2.5, 0.5], [3.5, 0.5], [3.5, 1.5]], 1),
        ],
    );
    f.edge(base, base + 2, 0);
    let y = at;
    let s = f.vert([2.0, 2.0 + y]);
    let pts = [[0.0, 0.0], [0.0, 4.0], [4.0, 0.0], [4.0, 4.0]].map(|p| f.vert([p[0], p[1] + y]));
    for (p, q) in [
        (pts[0], s),
        (s, pts[1]),
        (pts[1], pts[0]),
        (s, pts[2]),
        (pts[2], pts[3]),
        (pts[3], s),
    ] {
        f.edge(p, q, 1);
    }
    let tip = f.vert([1.0, 2.0 + y]);
    f.edge(pts[0], tip, 0);
    f
}

fn shuffle<T>(rng: &mut Rng, v: &mut [T]) {
    for i in (1..v.len()).rev() {
        let j = (rng.next_u64() % (i as u64 + 1)) as usize;
        v.swap(i, j);
    }
}

/// What the arrangement says, by positions instead of vertex and edge
/// ids: components by least vertex; per cycle its vertices from where it
/// was found, its component and boundedness; the certificates, regions
/// and containers; per vertex position its rotation's far ends.
type Seen = (
    Vec<[f64; 2]>,
    Vec<(Vec<[f64; 2]>, u32, bool)>,
    Vec<LoopIn>,
    Vec<Region>,
    Vec<Option<u32>>,
    Vec<([f64; 2], Vec<[f64; 2]>)>,
);

fn seen(a: &Arrangement, f: &Fig) -> Seen {
    let pos = |v: u32| f.verts[v as usize];
    let comps = a.component_count() as u32;
    let mut stars: Vec<([f64; 2], Vec<[f64; 2]>)> = (0..f.verts.len() as u32)
        .map(|v| {
            (
                pos(v),
                a.rotation(v).iter().map(|&h| pos(a.dest(h))).collect(),
            )
        })
        .collect();
    stars.sort_by(|x, y| x.0[0].total_cmp(&y.0[0]).then(x.0[1].total_cmp(&y.0[1])));
    (
        (0..comps).map(|k| pos(a.least_vertex(k))).collect(),
        (0..a.cycle_count() as u32)
            .map(|c| (cycle_pts(a, f, c), a.cycle_component(c), a.is_bounded(c)))
            .collect(),
        a.loop_in().to_vec(),
        a.regions().to_vec(),
        (0..comps).map(|k| a.container(k)).collect(),
        stars,
    )
}

#[test]
fn relabelling_changes_nothing() {
    let f = composite();
    let a = f.build().unwrap();
    let want = seen(&a, &f);
    assert_eq!(a.component_count(), 10);
    let mut rng = Rng::new(11);
    for _ in 0..24 {
        let mut perm: Vec<u32> = (0..f.verts.len() as u32).collect();
        shuffle(&mut rng, &mut perm);
        let mut g = Fig {
            verts: vec![[0.0; 2]; f.verts.len()],
            edges: Vec::new(),
        };
        for (v, &p) in perm.iter().enumerate() {
            g.verts[p as usize] = f.verts[v];
        }
        let mut edges = f.edges.clone();
        shuffle(&mut rng, &mut edges);
        for e in edges {
            let [x, y] = e.ends.map(|v| perm[v as usize]);
            if rng.next_u64() % 2 == 0 {
                g.edge(x, y, e.wind);
            } else {
                g.edge(y, x, -e.wind);
            }
        }
        assert_eq!(seen(&g.build().unwrap(), &g), want);
    }
}

#[test]
fn small_budget_runs_out() {
    let f = composite();
    let got = arrange(&f.verts, &f.edges, &mut Work::new(&Budget::new(40)));
    assert!(
        matches!(
            got,
            Err(ArrangeError::OutOfBudget(KernelError::OutOfBudget { .. }))
        ),
        "{got:?}"
    );
}

/// The L-shaped face of the planar sheet store, from its coedges' stored
/// UVs, with an island in its corner square.
#[test]
fn store_face_arranges_from_its_uvs() {
    let pair = varde_testkit::sheets::l_corner_quad(varde_testkit::sheets::OFFSET);
    let b = sheets::store(&pair.a);
    let face = FaceId(0);
    let mut f = Fig::default();
    for l in b.loops(face) {
        let uvs: Vec<[f64; 2]> = b
            .loop_coedges(l)
            .map(|c| b.coedge_uv(c, false).unwrap())
            .collect();
        f.ring(&uvs, 1);
    }
    assert_eq!(f.verts.len(), 6);
    // An island near the least corner, at weights of it and its two
    // neighbours: inside the L whichever corner that is.
    let k = least_corner(&f.verts);
    let c = [(k + 5) % 6, k, (k + 1) % 6].map(|i| f.verts[i]);
    let mix =
        |w: [f64; 3]| [0, 1].map(|a| (w[0] * c[0][a] + w[1] * c[1][a] + w[2] * c[2][a]) / 8.0);
    let island = [
        mix([1.0, 6.0, 1.0]),
        mix([1.0, 5.0, 2.0]),
        mix([2.0, 5.0, 1.0]),
    ];
    // The island runs the way the face's loop does; the least corner is
    // convex, so the turn there gives the loop's way.
    let turn = |p: [[f64; 2]; 3]| value_sign(&Orient(p)).0 == Ok(Ordering::Greater);
    let loop_ccw = turn(c);
    if turn(island) == loop_ccw {
        f.ring(&island, 1);
    } else {
        f.ring(&cw(&island), 1);
    }
    let a = f.build().unwrap();
    let w: Vec<i64> = a.regions().iter().map(|r| r.winding).collect();
    let inner = if loop_ccw { [0, 1, 2] } else { [0, -1, -2] };
    assert_eq!(w, inner);
    assert_eq!(a.container(1), Some(a.regions()[1].outer));
}

/// The index of the least corner of `p`.
fn least_corner(p: &[[f64; 2]]) -> usize {
    (0..p.len())
        .min_by(|&i, &j| {
            p[i][0]
                .total_cmp(&p[j][0])
                .then(p[i][1].total_cmp(&p[j][1]))
        })
        .unwrap()
}
