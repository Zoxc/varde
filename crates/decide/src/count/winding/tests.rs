//! The windings on random graphs: several components, closed edges and
//! lone vertices, each vertex placed at random and given a winding of 0
//! or 1, each edge's change the difference. Carried from seeds they come
//! back; a change off by one on an edge of a cycle disagrees, on a
//! bridge it puts windings outside `{0, 1}`, and the failure's key (the
//! geometric entity) is the same whatever ids the entities get. Seeds are
//! the first cleared vertex of each component in geometric order, or its
//! first one; the search's budget and errors are keyed by the vertex.
//! `clear_of` certifies distances from boxes, never for a point within
//! `eps`.

use std::collections::BTreeMap;

use varde_testkit::rng::Rng;

use super::*;
use crate::count::testing::Order;

mod solids;

/// A graph by geometric labels: vertex `i` at `pos[i]`, its winding
/// `w[i]`; edges by labels.
struct Labelled {
    pos: Vec<[f64; 3]>,
    w: Vec<i64>,
    edges: Vec<[usize; 2]>,
}

fn pick(rng: &mut Rng, n: usize) -> usize {
    (rng.next_u64() % n as u64) as usize
}

impl Labelled {
    /// `parts` components of up to 6 vertices each (a random tree, extra
    /// edges, sometimes a closed edge or a lone vertex).
    fn random(rng: &mut Rng, parts: usize) -> Labelled {
        let mut g = Labelled {
            pos: Vec::new(),
            w: Vec::new(),
            edges: Vec::new(),
        };
        for _ in 0..parts {
            let n = 1 + pick(rng, 6);
            let base = g.pos.len();
            for _ in 0..n {
                g.pos.push([0, 1, 2].map(|_| rng.range(-4.0, 4.0)));
                g.w.push((rng.next_u64() % 2) as i64);
            }
            for i in 1..n {
                g.edges.push([base + pick(rng, i), base + i]);
            }
            for _ in 0..pick(rng, n + 1) {
                let e = [base + pick(rng, n), base + pick(rng, n)];
                // No two edges on one pair of vertices: their keys would
                // tie exactly, where ids decide.
                if !g.edges.iter().any(|&[a, b]| [a, b] == e || [b, a] == e) {
                    g.edges.push(e);
                }
            }
        }
        g
    }

    /// Ids for it: vertices and edges numbered by random permutations,
    /// each edge stored from its lower-id end. Returns the order, the
    /// edges by id, each id's label and the labels' ids.
    fn ids(&self, rng: &mut Rng) -> Ids {
        let vperm = perm(rng, self.pos.len());
        let eperm = perm(rng, self.edges.len());
        let mut edges = vec![[VertId(0); 2]; self.edges.len()];
        let mut flipped = vec![false; self.edges.len()];
        let mut order = Order::default();
        for (l, &p) in self.pos.iter().enumerate() {
            order.pos.insert((Operand::A, VertId(vperm[l] as u32)), p);
        }
        for (l, &[a, b]) in self.edges.iter().enumerate() {
            let (va, vb) = (VertId(vperm[a] as u32), VertId(vperm[b] as u32));
            let id = eperm[l];
            flipped[id] = vb < va;
            edges[id] = if vb < va { [vb, va] } else { [va, vb] };
            order
                .edges
                .insert((Operand::A, EdgeId(id as u32)), edges[id]);
        }
        let mut vlabel = vec![0; vperm.len()];
        for (l, &i) in vperm.iter().enumerate() {
            vlabel[i] = l;
        }
        let mut elabel = vec![0; eperm.len()];
        for (l, &i) in eperm.iter().enumerate() {
            elabel[i] = l;
        }
        Ids {
            order,
            edges,
            flipped,
            vperm,
            vlabel,
            elabel,
        }
    }

    /// The change along edge label `l` from its first label to its second.
    fn change(&self, l: usize) -> i64 {
        let [a, b] = self.edges[l];
        self.w[b] - self.w[a]
    }

    /// Per component (by labels) its vertices.
    fn components(&self) -> Vec<Vec<usize>> {
        let n = self.pos.len();
        let mut root: Vec<usize> = (0..n).collect();
        fn find(r: &mut [usize], x: usize) -> usize {
            if r[x] == x {
                x
            } else {
                let y = find(r, r[x]);
                r[x] = y;
                y
            }
        }
        for &[a, b] in &self.edges {
            let (x, y) = (find(&mut root, a), find(&mut root, b));
            root[x] = y;
        }
        let mut by: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for v in 0..n {
            let r = find(&mut root, v);
            by.entry(r).or_default().push(v);
        }
        by.into_values().collect()
    }
}

struct Ids {
    order: Order,
    edges: Vec<[VertId; 2]>,
    /// Per edge id: stored against its labels' direction.
    flipped: Vec<bool>,
    vperm: Vec<usize>,
    vlabel: Vec<usize>,
    elabel: Vec<usize>,
}

impl Ids {
    fn graph(&self) -> Graph<'_> {
        Graph {
            operand: Operand::A,
            verts: self.vperm.len() as u32,
            edges: &self.edges,
        }
    }

    /// The change along edge `e` in its stored direction, from labels'
    /// `changes`.
    fn change(&self, changes: &[i64], e: EdgeId) -> i64 {
        let c = changes[self.elabel[e.index()]];
        if self.flipped[e.index()] { -c } else { c }
    }

    /// The seeds of every component at its lowest-label vertex.
    fn seeds(&self, g: &Labelled, w0: impl Fn(usize) -> i64) -> Vec<(VertId, i64)> {
        g.components()
            .iter()
            .map(|c| (VertId(self.vperm[c[0]] as u32), w0(c[0])))
            .collect()
    }

    /// An error's entity as a label.
    fn label(&self, e: &WindingError<()>) -> (char, usize) {
        match e {
            WindingError::Disagree { edge, .. } | WindingError::Read { edge, .. } => {
                ('e', self.elabel[edge.index()])
            }
            WindingError::Outside { point, .. } | WindingError::Unseeded(point) => {
                ('v', self.vlabel[vert(point).index()])
            }
            _ => panic!("{e:?}"),
        }
    }
}

fn vert(p: &Point) -> VertId {
    match p.q {
        crate::records::keys::Query::Vert(v) => v,
        _ => panic!("{p:?}"),
    }
}

fn perm(rng: &mut Rng, n: usize) -> Vec<usize> {
    let mut p: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        p.swap(i, pick(rng, i + 1));
    }
    p
}

/// The windings by label, from `changes` by label and seeds at each
/// component's lowest label (`w0`).
fn run(
    g: &Labelled,
    ids: &Ids,
    changes: &[i64],
    w0: impl Fn(usize) -> i64,
) -> Result<Vec<i64>, WindingError<()>> {
    let w = windings(&ids.order, &ids.graph(), &ids.seeds(g, w0), |e| {
        Ok::<_, ()>(ids.change(changes, e))
    })?;
    Ok((0..g.pos.len()).map(|l| w[ids.vperm[l]]).collect())
}

#[test]
fn windings_come_back_from_the_seeds() {
    let mut rng = Rng::new(0x3a1);
    for _ in 0..varde_testing::pick(64, 512) {
        let parts = 1 + pick(&mut rng, 4);
        let g = Labelled::random(&mut rng, parts);
        let changes: Vec<i64> = (0..g.edges.len()).map(|l| g.change(l)).collect();
        for _ in 0..3 {
            let ids = g.ids(&mut rng);
            assert_eq!(run(&g, &ids, &changes, |l| g.w[l]), Ok(g.w.clone()));
        }
    }
}

/// Whether edge label `l` lies on a cycle (removing it keeps its ends
/// joined); a closed edge does.
fn on_cycle(g: &Labelled, l: usize) -> bool {
    let [a, b] = g.edges[l];
    let mut seen = vec![false; g.pos.len()];
    let mut stack = vec![a];
    seen[a] = true;
    while let Some(v) = stack.pop() {
        for (k, &[x, y]) in g.edges.iter().enumerate() {
            if k == l {
                continue;
            }
            for (p, q) in [(x, y), (y, x)] {
                if p == v && !seen[q] {
                    seen[q] = true;
                    stack.push(q);
                }
            }
        }
    }
    seen[b]
}

#[test]
fn a_wrong_change_is_a_signal_keyed_alike_under_any_ids() {
    let mut rng = Rng::new(0x3a2);
    let (mut cycles, mut bridges) = (0, 0);
    for _ in 0..varde_testing::pick(96, 768) {
        let parts = 1 + pick(&mut rng, 3);
        let g = Labelled::random(&mut rng, parts);
        if g.edges.is_empty() {
            continue;
        }
        let bad = pick(&mut rng, g.edges.len());
        let mut changes: Vec<i64> = (0..g.edges.len()).map(|l| g.change(l)).collect();
        changes[bad] += if rng.next_u64().is_multiple_of(2) {
            1
        } else {
            -1
        };
        let cycle = on_cycle(&g, bad);
        let mut keys = Vec::new();
        for _ in 0..4 {
            let ids = g.ids(&mut rng);
            match run(&g, &ids, &changes, |l| g.w[l]) {
                Err(err) => {
                    assert!(err.is_signal(), "{err:?}");
                    if cycle {
                        assert!(matches!(err, WindingError::Disagree { .. }), "{err:?}");
                    } else {
                        // A bridge: the windings beyond it all move.
                        // Never a disagreement: no cycle holds the edge.
                        assert!(matches!(err, WindingError::Outside { .. }), "{err:?}");
                    }
                    keys.push(Some(ids.label(&err)));
                }
                Ok(w) => {
                    // A bridge whose far side's windings all move from
                    // 0 to 1 or back: nothing shows.
                    assert!(!cycle, "{w:?}");
                    assert!(w.iter().all(|w| (0..=1).contains(w)));
                    keys.push(None);
                }
            }
        }
        if cycle {
            cycles += 1;
        } else if keys[0].is_some() {
            bridges += 1;
        }
        assert!(keys.windows(2).all(|k| k[0] == k[1]), "{keys:?}");
    }
    assert!(cycles > 10 && bridges > 10, "{cycles} {bridges}");
}

#[test]
fn a_failed_read_is_keyed_by_the_first_edge_in_order() {
    let mut rng = Rng::new(0x3a3);
    for _ in 0..varde_testing::pick(32, 256) {
        let parts = 2 + pick(&mut rng, 3);
        let g = Labelled::random(&mut rng, parts);
        if g.edges.len() < 2 {
            continue;
        }
        let failing: Vec<bool> = (0..g.edges.len())
            .map(|_| rng.next_u64().is_multiple_of(3))
            .collect();
        if !failing.contains(&true) {
            continue;
        }
        let mut keys = Vec::new();
        for _ in 0..3 {
            let ids = g.ids(&mut rng);
            let err = windings(&ids.order, &ids.graph(), &ids.seeds(&g, |l| g.w[l]), |e| {
                if failing[ids.elabel[e.index()]] {
                    Err(())
                } else {
                    Ok(0)
                }
            })
            .expect_err("a read fails");
            assert!(!err.is_signal());
            // The first failing edge in geometric order.
            let first = (0..ids.edges.len())
                .map(|e| EdgeId(e as u32))
                .filter(|e| failing[ids.elabel[e.index()]])
                .min_by_key(|&e| (ids.order.edge_key(Operand::A, e), e))
                .unwrap();
            assert_eq!(
                err,
                WindingError::Read {
                    operand: Operand::A,
                    edge: first,
                    error: ()
                }
            );
            keys.push(ids.label(&err));
        }
        assert!(keys.windows(2).all(|k| k[0] == k[1]), "{keys:?}");
    }
}

/// A path `0 — 1 — 2` and a lone vertex 3, positions along `x`.
fn path() -> (Order, Vec<[VertId; 2]>) {
    let mut order = Order::default();
    for v in 0..4u32 {
        order
            .pos
            .insert((Operand::B, VertId(v)), [f64::from(v), 0.0, 0.0]);
    }
    let edges = vec![[VertId(0), VertId(1)], [VertId(1), VertId(2)]];
    for (e, &ends) in (0u32..).zip(&edges) {
        order.edges.insert((Operand::B, EdgeId(e)), ends);
    }
    (order, edges)
}

#[test]
fn seeds_must_cover_each_component_once() {
    let (order, edges) = path();
    let graph = Graph {
        operand: Operand::B,
        verts: 4,
        edges: &edges,
    };
    let ok = |_| Ok::<i64, ()>(0);
    let p = |v| Point::vert(Operand::B, VertId(v));
    assert_eq!(
        windings(&order, &graph, &[(VertId(1), 1)], ok),
        Err(WindingError::Unseeded(p(3)))
    );
    assert_eq!(
        windings(&order, &graph, &[(VertId(1), 1), (VertId(3), 0)], ok),
        Ok(vec![1, 1, 1, 0])
    );
    assert_eq!(
        windings(
            &order,
            &graph,
            &[(VertId(1), 1), (VertId(2), 1), (VertId(3), 0)],
            ok
        ),
        Err(WindingError::Seed(p(2)))
    );
    assert_eq!(
        windings(&order, &graph, &[(VertId(4), 0)], ok),
        Err(WindingError::Seed(p(4)))
    );
    // A seed outside `{0, 1}`: the first vertex in order is named.
    assert_eq!(
        windings(&order, &graph, &[(VertId(2), 2), (VertId(3), 0)], ok),
        Err(WindingError::Outside { point: p(0), w: 2 })
    );
    let bad = [[VertId(0), VertId(4)]];
    let graph = Graph {
        operand: Operand::B,
        verts: 4,
        edges: &bad,
    };
    assert_eq!(
        windings(&order, &graph, &[(VertId(0), 0)], ok),
        Err(WindingError::Graph(GraphError::End(EdgeId(0))))
    );
}

#[test]
fn a_winding_past_i64_is_outside() {
    let (order, edges) = path();
    let graph = Graph {
        operand: Operand::B,
        verts: 4,
        edges: &edges,
    };
    let p = |v| Point::vert(Operand::B, VertId(v));
    let err = windings(&order, &graph, &[(VertId(0), 0), (VertId(3), 0)], |_| {
        Ok::<i64, ()>(i64::MAX)
    })
    .expect_err("outside");
    // The first vertex in order outside `{0, 1}`, with its winding.
    assert_eq!(
        err,
        WindingError::Outside {
            point: p(1),
            w: i128::from(i64::MAX)
        }
    );
    assert!(err.is_signal());
    // Carried backwards: `−i64::MIN` is past `i64`.
    let err = windings(&order, &graph, &[(VertId(2), 0), (VertId(3), 0)], |e| {
        Ok::<i64, ()>(if e == EdgeId(1) { i64::MIN } else { 0 })
    })
    .expect_err("outside");
    assert_eq!(
        err,
        WindingError::Outside {
            point: p(0),
            w: -i128::from(i64::MIN)
        }
    );
}

#[test]
fn seeds_are_the_first_cleared_vertex_of_each_component() {
    let mut rng = Rng::new(0x3a4);
    for _ in 0..varde_testing::pick(48, 384) {
        let parts = 1 + pick(&mut rng, 4);
        let g = Labelled::random(&mut rng, parts);
        let cleared: Vec<bool> = (0..g.pos.len())
            .map(|_| rng.next_u64().is_multiple_of(3))
            .collect();
        // By labels: per component the first cleared vertex by position,
        // or the first one.
        let first = |c: &[usize], f: &dyn Fn(usize) -> bool| {
            c.iter()
                .copied()
                .filter(|&l| f(l))
                .min_by(|&a, &b| g.pos[a].partial_cmp(&g.pos[b]).unwrap())
        };
        let mut want: Vec<(usize, bool)> = g
            .components()
            .iter()
            .map(|c| match first(c, &|l| cleared[l]) {
                Some(l) => (l, true),
                None => (first(c, &|_| true).unwrap(), false),
            })
            .collect();
        want.sort_unstable();
        let mut runs = Vec::new();
        for _ in 0..3 {
            let ids = g.ids(&mut rng);
            let mut tested = 0u64;
            let (r, spent) = seeds(&ids.order, &ids.graph(), u64::MAX, |v| {
                tested += 1;
                (Ok::<_, ()>(cleared[ids.vlabel[v.index()]]), 3)
            });
            assert_eq!(spent, 3 * tested);
            let s = r.expect("seeds");
            let mut got: Vec<(usize, bool)> = s
                .iter()
                .map(|s| (ids.vlabel[s.vert.index()], s.cleared))
                .collect();
            runs.push(got.clone());
            got.sort_unstable();
            assert_eq!(got, want);
        }
        // The same seeds in the same order whatever the ids.
        assert!(runs.windows(2).all(|r| r[0] == r[1]));
    }
}

#[test]
fn the_clearance_search_is_charged_and_keyed() {
    let (order, edges) = path();
    let graph = Graph {
        operand: Operand::B,
        verts: 4,
        edges: &edges,
    };
    let p = |v| Point::vert(Operand::B, VertId(v));
    // Nothing clear: every vertex tested, 2 units each.
    let (r, spent) = seeds(&order, &graph, 8, |_| (Ok::<_, ()>(false), 2));
    assert_eq!(spent, 8);
    assert_eq!(
        r,
        Ok(vec![
            Seed {
                vert: VertId(0),
                cleared: false
            },
            Seed {
                vert: VertId(3),
                cleared: false
            }
        ])
    );
    // One unit short: stopped at the last test.
    let (r, spent) = seeds(&order, &graph, 7, |_| (Ok::<_, ()>(false), 2));
    assert_eq!(spent, 8);
    assert_eq!(
        r,
        Err(SeedError::OutOfBudget {
            cause: Exhausted::Work,
            point: p(3)
        })
    );
    // A failing test is keyed by its vertex.
    let (r, _) = seeds(&order, &graph, 100, |v| {
        (if v == VertId(1) { Err(7) } else { Ok(false) }, 1)
    });
    assert_eq!(
        r,
        Err(SeedError::Clearance {
            point: p(1),
            error: 7
        })
    );
}

#[test]
fn clearance_from_boxes_is_certified() {
    let b = HullBox {
        lo: [1.0, 1.0, 1.0],
        hi: [2.0, 2.0, 2.0],
    };
    let eps = 0.25;
    // Along an axis: just past `eps`, at it, inside.
    assert!(clear_of([0.75 - 1e-9, 1.5, 1.5], eps, [&b]));
    assert!(!clear_of([0.75, 1.5, 1.5], eps, [&b]));
    assert!(!clear_of([1.5, 1.5, 1.5], eps, [&b]));
    assert!(clear_of([2.25 + 1e-9, 1.5, 1.5], eps, [&b]));
    // Off a corner: `0.2·√3 ≈ 0.346`, but no axis gap past `eps`.
    assert!(clear_of([0.8, 0.8, 0.8], eps, [&b]));
    assert!(!clear_of([0.86, 0.86, 0.86], eps, [&b]));
    // Every box must clear.
    let far = HullBox {
        lo: [10.0; 3],
        hi: [11.0; 3],
    };
    assert!(clear_of([0.0; 3], eps, [&b, &far]));
    assert!(!clear_of([9.9, 10.5, 10.5], eps, [&b, &far]));
    assert!(clear_of([0.0; 3], eps, []));
    // Sides not certified: unbounded below in `x`.
    let open = HullBox {
        lo: [f64::NEG_INFINITY, 1.0, 1.0],
        hi: [2.0, 2.0, 2.0],
    };
    assert!(!clear_of([-1e300, 1.5, 1.5], eps, [&open]));
    assert!(clear_of([-1e300, 0.0, 1.5], eps, [&open]));
    let nan = HullBox {
        lo: [f64::NAN; 3],
        hi: [f64::NAN; 3],
    };
    assert!(!clear_of([0.0; 3], eps, [&nan]));
    // A point or `eps` that isn't finite, or a negative `eps`.
    assert!(!clear_of([f64::NAN, 0.0, 0.0], eps, [&far]));
    assert!(!clear_of([0.0; 3], f64::INFINITY, [&far]));
    assert!(!clear_of([0.0; 3], -1.0, [&far]));
    // Tiny gaps, whose squares underflow: one axis's gap alone past
    // `eps` clears, the three together aren't certified.
    let tiny = HullBox {
        lo: [1e-200; 3],
        hi: [1.0; 3],
    };
    assert!(clear_of([0.0; 3], 0.9e-200, [&tiny]));
    assert!(!clear_of([0.0; 3], 1.5e-200, [&tiny]));
    assert!(!clear_of([0.0; 3], 2e-200, [&tiny]));
}

#[test]
fn an_edge_that_disagrees_is_named_before_a_winding_too_large() {
    // Along `x`: a cycle 0, 1, 2 whose changes sum to 1, then an edge
    // 2 → 3 carrying the winding past `i64`. The cycle's edge is nearer
    // the cause: it is the one named.
    let mut order = Order::default();
    for v in 0..4u32 {
        order
            .pos
            .insert((Operand::A, VertId(v)), [f64::from(v), 0.0, 0.0]);
    }
    let edges = vec![
        [VertId(0), VertId(1)],
        [VertId(1), VertId(2)],
        [VertId(0), VertId(2)],
        [VertId(2), VertId(3)],
    ];
    for (e, &ends) in (0u32..).zip(&edges) {
        order.edges.insert((Operand::A, EdgeId(e)), ends);
    }
    let graph = Graph {
        operand: Operand::A,
        verts: 4,
        edges: &edges,
    };
    let changes = [0, 0, 1, i64::MAX];
    let err = windings(&order, &graph, &[(VertId(0), 1)], |e| {
        Ok::<_, ()>(changes[e.index()])
    })
    .expect_err("disagrees");
    assert_eq!(
        err,
        WindingError::Disagree {
            operand: Operand::A,
            edge: EdgeId(1),
            w: [1, 2],
            change: 0
        }
    );
}
