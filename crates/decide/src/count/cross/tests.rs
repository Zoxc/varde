//! The identities on random records: two random closed operands (a
//! tetrahedron's faces split at random, some faces merged so they hold
//! several loops, edges stored from their lower-id end) with every
//! `s02`, `s20` and `s11` value random. Per face pair the `x12` ends
//! round `f_A`'s loops sum to the `x21` ends round `f_B`'s; `Σ_f x12(e,
//! f)` is `w(b) − w(a)` and sums to 0 round closed walks, which fails
//! with a face of `B` left out or per face; `+Σ s11` in `x21` breaks the
//! face-pair identity. Sums leaving `i64` and failed reads are errors.

use std::collections::BTreeMap;

use varde_testkit::rng::Rng;

use super::*;

mod geometry;

/// A combinatorial operand: per face its loops, each a vertex cycle, and
/// the edges by their ends (lower id first), each with its id.
struct Surface {
    faces: Vec<Vec<Vec<u32>>>,
    verts: u32,
}

impl Surface {
    /// A tetrahedron, each face's loop outward.
    fn tetrahedron() -> Surface {
        Surface {
            faces: vec![
                vec![vec![0, 2, 1]],
                vec![vec![0, 1, 3]],
                vec![vec![1, 2, 3]],
                vec![vec![0, 3, 2]],
            ],
            verts: 4,
        }
    }

    /// A random closed surface: `ops` random splits of the tetrahedron
    /// (a face at a new vertex, an edge at a new vertex, a face along a
    /// diagonal), then `merges` pairs of faces made one face of several
    /// loops. Vertex ids are shuffled so the stored edge directions are.
    fn random(rng: &mut Rng, ops: usize, merges: usize) -> Surface {
        let mut s = Surface::tetrahedron();
        for _ in 0..ops {
            let f = pick(rng, s.faces.len());
            let lp = s.faces[f][0].clone();
            let k = lp.len();
            match rng.next_u64() % 3 {
                0 => {
                    let n = s.new_vertex();
                    s.faces.remove(f);
                    for i in 0..k {
                        s.faces.push(vec![vec![lp[i], lp[(i + 1) % k], n]]);
                    }
                }
                1 => {
                    let i = pick(rng, k);
                    let (u, v) = (lp[i], lp[(i + 1) % k]);
                    let n = s.new_vertex();
                    for face in &mut s.faces {
                        let l = &mut face[0];
                        let m = l.len();
                        if let Some(j) = (0..m).find(|&j| {
                            (l[j], l[(j + 1) % m]) == (u, v) || (l[j], l[(j + 1) % m]) == (v, u)
                        }) {
                            l.insert(j + 1, n);
                        }
                    }
                }
                _ if k >= 4 => {
                    let i = pick(rng, k);
                    let j = (i + 2 + pick(rng, k - 3)) % k;
                    let (i, j) = (i.min(j), i.max(j));
                    if s.edges().contains_key(&sorted(lp[i], lp[j])) {
                        // Already an edge: the split would leave it three faces.
                        continue;
                    }
                    let first: Vec<u32> = lp[i..=j].to_vec();
                    let second: Vec<u32> = lp[j..].iter().chain(&lp[..=i]).copied().collect();
                    s.faces[f] = vec![first];
                    s.faces.push(vec![second]);
                }
                _ => {}
            }
        }
        for _ in 0..merges {
            if s.faces.len() < 3 {
                break;
            }
            let f = pick(rng, s.faces.len());
            let loops = s.faces.remove(f);
            let g = pick(rng, s.faces.len());
            s.faces[g].extend(loops);
        }
        let mut perm: Vec<u32> = (0..s.verts).collect();
        for i in (1..perm.len()).rev() {
            perm.swap(i, pick(rng, i + 1));
        }
        for l in s.faces.iter_mut().flatten() {
            for v in l {
                *v = perm[*v as usize];
            }
        }
        s
    }

    fn new_vertex(&mut self) -> u32 {
        self.verts += 1;
        self.verts - 1
    }

    /// The edges by their ends, lower id first, numbered in that order.
    fn edges(&self) -> BTreeMap<(u32, u32), EdgeId> {
        let mut ends: Vec<(u32, u32)> = self
            .faces
            .iter()
            .flatten()
            .flat_map(|l| (0..l.len()).map(move |i| sorted(l[i], l[(i + 1) % l.len()])))
            .collect();
        ends.sort_unstable();
        ends.dedup();
        ends.into_iter()
            .zip(0u32..)
            .map(|(k, i)| (k, EdgeId(i)))
            .collect()
    }

    /// Face `f`'s coedges, every loop.
    fn boundary(&self, f: usize, edges: &BTreeMap<(u32, u32), EdgeId>) -> Vec<Coedge> {
        self.faces[f]
            .iter()
            .flat_map(|l| {
                (0..l.len()).map(move |i| {
                    let (u, v) = (l[i], l[(i + 1) % l.len()]);
                    Coedge {
                        edge: edges[&sorted(u, v)],
                        forward: u < v,
                    }
                })
            })
            .collect()
    }

    /// Every edge has one coedge each way.
    fn closed(&self) -> bool {
        let mut d: BTreeMap<(u32, u32), i32> = BTreeMap::new();
        for l in self.faces.iter().flatten() {
            for i in 0..l.len() {
                let (u, v) = (l[i], l[(i + 1) % l.len()]);
                *d.entry((u, v)).or_default() += 1;
            }
        }
        d.iter()
            .all(|(&(u, v), &n)| n == 1 && d.get(&(v, u)) == Some(&1))
    }
}

fn sorted(u: u32, v: u32) -> (u32, u32) {
    (u.min(v), u.max(v))
}

fn pick(rng: &mut Rng, n: usize) -> usize {
    (rng.next_u64() % n as u64) as usize
}

fn val(rng: &mut Rng) -> i64 {
    (rng.next_u64() % 7) as i64 - 3
}

/// Two operands with random records and every crossing count by the
/// identities: `x12[e][f]`, `x21[g][f]`.
struct World {
    a: Surface,
    b: Surface,
    ea: BTreeMap<(u32, u32), EdgeId>,
    eb: BTreeMap<(u32, u32), EdgeId>,
    /// `s02[v][f]`, `v` of `A`, `f` of `B`.
    s02: Vec<Vec<i64>>,
    x12: Vec<Vec<i64>>,
    x21: Vec<Vec<i64>>,
}

impl World {
    fn new(rng: &mut Rng, a: Surface, b: Surface, x21_sign: i64) -> World {
        let (ea, eb) = (a.edges(), b.edges());
        let s02: Vec<Vec<i64>> = (0..a.verts)
            .map(|_| b.faces.iter().map(|_| val(rng)).collect())
            .collect();
        let s20: Vec<Vec<i64>> = (0..b.verts)
            .map(|_| a.faces.iter().map(|_| val(rng)).collect())
            .collect();
        let s11: Vec<Vec<i64>> = ea
            .iter()
            .map(|_| eb.iter().map(|_| val(rng)).collect())
            .collect();
        let read = |e: EdgeId, g: EdgeId| Ok::<i64, ()>(s11[e.index()][g.index()]);
        let x12 = ea
            .iter()
            .map(|(&(u, v), &e)| {
                (0..b.faces.len())
                    .map(|f| {
                        let ends = [s02[u as usize][f], s02[v as usize][f]];
                        x12(ends, &b.boundary(f, &eb), |g| read(e, g)).unwrap()
                    })
                    .collect()
            })
            .collect();
        let x21 = eb
            .iter()
            .map(|(&(u, v), &g)| {
                (0..a.faces.len())
                    .map(|f| {
                        let ends = [s20[u as usize][f], s20[v as usize][f]];
                        x21(ends, &a.boundary(f, &ea), |e| {
                            read(e, g).map(|s| x21_sign * s)
                        })
                        .unwrap()
                    })
                    .collect()
            })
            .collect();
        World {
            a,
            b,
            ea,
            eb,
            s02,
            x12,
            x21,
        }
    }

    /// Whether `Σ_{c∈∂f_A} sgn·x12(e_c, f_B) = Σ_{c∈∂f_B} sgn·x21(g_c,
    /// f_A)` for every face pair.
    fn face_pairs_agree(&self) -> bool {
        (0..self.a.faces.len()).all(|fa| {
            (0..self.b.faces.len()).all(|fb| {
                let lhs: i64 = self
                    .a
                    .boundary(fa, &self.ea)
                    .iter()
                    .map(|c| c.sgn() * self.x12[c.edge.index()][fb])
                    .sum();
                let rhs: i64 = self
                    .b
                    .boundary(fb, &self.eb)
                    .iter()
                    .map(|c| c.sgn() * self.x21[c.edge.index()][fa])
                    .sum();
                lhs == rhs
            })
        })
    }

    /// The change of `w` along edge `(u, v)` of `A` walked from `u`, over
    /// the faces `faces` of `B`.
    fn step(&self, u: u32, v: u32, faces: &[usize]) -> i64 {
        let e = self.ea[&sorted(u, v)];
        let d: i64 = faces.iter().map(|&f| self.x12[e.index()][f]).sum();
        if u < v { d } else { -d }
    }

    /// `w(v)` over all of `B`'s faces.
    fn w(&self, v: u32) -> i64 {
        self.s02[v as usize].iter().sum()
    }
}

/// Closed walks along `A`'s edges: each face loop, and the cycles random
/// walks close on revisiting a vertex.
fn walks(rng: &mut Rng, a: &Surface) -> Vec<Vec<u32>> {
    let mut out: Vec<Vec<u32>> = a.faces.iter().flatten().cloned().collect();
    let mut adj: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for &(u, v) in a.edges().keys() {
        adj.entry(u).or_default().push(v);
        adj.entry(v).or_default().push(u);
    }
    for _ in 0..8 {
        let mut path = vec![pick(rng, a.verts as usize) as u32];
        loop {
            let n = &adj[path.last().unwrap()];
            let next = n[pick(rng, n.len())];
            if let Some(i) = path.iter().position(|&p| p == next) {
                if path.len() - i >= 3 {
                    out.push(path[i..].to_vec());
                    break;
                }
                path.truncate(i + 1);
            } else {
                path.push(next);
            }
        }
    }
    out
}

/// Two random closed operands.
fn operands(rng: &mut Rng) -> (Surface, Surface) {
    let ops = 2 + pick(rng, 12);
    let merges = pick(rng, 3);
    let a = Surface::random(rng, ops, merges);
    let merges = pick(rng, 3);
    (a, Surface::random(rng, ops / 2 + 1, merges))
}

fn walk_sum(w: &World, walk: &[u32], faces: &[usize]) -> i64 {
    (0..walk.len())
        .map(|i| w.step(walk[i], walk[(i + 1) % walk.len()], faces))
        .sum()
}

/// Both identities on random records over random closed operands, with the
/// controls that show what each needs.
#[test]
fn identities_hold_for_any_records() {
    let mut open_broke = 0;
    let mut face_broke = 0;
    let mut sign_broke = 0;
    for seed in varde_testing::seeds(60, 1200) {
        let mut rng = Rng::new(seed);
        let (a, b) = operands(&mut rng);
        assert!(a.closed() && b.closed(), "seed {seed}");
        let w = World::new(&mut rng, a, b, 1);
        assert!(w.face_pairs_agree(), "seed {seed}: face pair identity");
        let all: Vec<usize> = (0..w.b.faces.len()).collect();
        for &(u, v) in w.ea.keys() {
            assert_eq!(
                w.step(u, v, &all),
                w.w(v) - w.w(u),
                "seed {seed}: edge {u}→{v}"
            );
        }
        let walks = walks(&mut rng, &w.a);
        for walk in &walks {
            assert_eq!(walk_sum(&w, walk, &all), 0, "seed {seed}: walk {walk:?}");
        }
        // Without one of B's faces, or per face, walks needn't close.
        let open: Vec<usize> = all[1..].to_vec();
        open_broke += usize::from(walks.iter().any(|k| walk_sum(&w, k, &open) != 0));
        face_broke += usize::from(walks.iter().any(|k| walk_sum(&w, k, &[0]) != 0));
        // `x21` reading `s11` negated breaks the face-pair identity.
        let mut rng = Rng::new(seed);
        let (a, b) = operands(&mut rng);
        sign_broke += usize::from(!World::new(&mut rng, a, b, -1).face_pairs_agree());
    }
    let n = varde_testing::seeds(60, 1200).count();
    assert!(open_broke * 2 > n, "open B: {open_broke} of {n}");
    assert!(face_broke * 2 > n, "one face: {face_broke} of {n}");
    assert!(sign_broke * 2 > n, "negated s11: {sign_broke} of {n}");
}

/// The surfaces the generator makes are closed and hold faces of several
/// loops, so the identities above saw both.
#[test]
fn random_surfaces_are_closed_with_multi_loop_faces() {
    let mut multi = 0;
    for seed in 0..50 {
        let mut rng = Rng::new(seed);
        let s = Surface::random(&mut rng, 10, 2);
        assert!(s.closed(), "seed {seed}");
        multi += usize::from(s.faces.iter().any(|f| f.len() > 1));
    }
    assert!(multi > 25, "{multi}");
}

/// The values are combined exactly as written: a hand example with a
/// reversed coedge, an edge used twice (a slit: its two terms cancel).
#[test]
fn hand_example() {
    let c = |e: u32, forward: bool| Coedge {
        edge: EdgeId(e),
        forward,
    };
    let s11 = |g: EdgeId| Ok::<i64, ()>([2, -1, 5][g.index()]);
    let b = [c(0, true), c(1, false), c(2, true), c(2, false)];
    // 4 − 1 − (2 − (−1) + 5 − 5) = 0.
    assert_eq!(x12([1, 4], &b, s11), Ok(0));
    assert_eq!(x21([4, 1], &b, s11), Ok(-6));
    assert_eq!(x12::<()>([0, 0], &[], |_| unreachable!()), Ok(0));
}

/// A sum leaving `i64` is an error, never a wrapped count; a failed read
/// is passed on.
#[test]
fn overflow_and_reads_are_errors() {
    let fwd = [Coedge {
        edge: EdgeId(0),
        forward: true,
    }];
    let rev = [Coedge {
        edge: EdgeId(0),
        forward: false,
    }];
    let ok = |v: i64| move |_| Ok::<i64, ()>(v);
    assert_eq!(x12([i64::MIN, 1], &[], ok(0)), Err(CrossError::Overflow));
    assert_eq!(x12([0, i64::MIN], &fwd, ok(1)), Err(CrossError::Overflow));
    assert_eq!(x21([0, 0], &rev, ok(i64::MIN)), Err(CrossError::Overflow));
    assert_eq!(x21([0, i64::MAX], &rev, ok(1)), Err(CrossError::Overflow));
    assert_eq!(x12([0, i64::MAX - 1], &rev, ok(1)), Ok(i64::MAX));
    assert_eq!(
        x12([0, 0], &fwd, |_| Err::<i64, &str>("missing")),
        Err(CrossError::Read("missing"))
    );
}

/// Closed edges: an edge whose two ends are one vertex reads that
/// vertex's `s02` at both ends, so only the trims' sweeps remain; a face
/// bounded by one closed edge has one coedge, its sense the sign.
#[test]
fn closed_edges() {
    let one = |forward: bool| {
        [Coedge {
            edge: EdgeId(0),
            forward,
        }]
    };
    let s11 = |_| Ok::<i64, ()>(1);
    // A closed `e` passing once under a disc bounded by a closed `g`.
    assert_eq!(x12([3, 3], &one(true), s11), Ok(-1));
    assert_eq!(x12([3, 3], &one(false), s11), Ok(1));
    assert_eq!(x21([-2, -2], &one(false), s11), Ok(1));
}
