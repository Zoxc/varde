//! The geometric order: `−0` before `+0` in positions, the two one point
//! as parameters, NaNs refused, ties only where the bits are equal; and on random sets with many exact ties, permuting
//! the ids (which turns edges stored from their lower-id end round)
//! changes neither the order nor the first entity, except among exact
//! ties.

use super::*;

/// Coordinates with many exact ties: both zeros, a subnormal, both
/// infinities.
const POOL: [f64; 9] = [
    -0.0,
    0.0,
    1.0,
    -1.0,
    2.5,
    5e-324,
    -5e-324,
    f64::INFINITY,
    f64::NEG_INFINITY,
];

struct Rng(varde_testkit::rng::Rng);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(varde_testkit::rng::Rng::new(seed))
    }

    fn below(&mut self, n: usize) -> usize {
        (self.0.next_u64() % n as u64) as usize
    }

    fn coord(&mut self) -> f64 {
        POOL[self.below(POOL.len())]
    }

    fn pos(&mut self) -> [f64; 3] {
        [self.coord(), self.coord(), self.coord()]
    }

    /// A permutation of `0..n`.
    fn perm(&mut self, n: usize) -> Vec<u32> {
        let mut p: Vec<u32> = (0..n as u32).collect();
        for i in (1..n).rev() {
            p.swap(i, self.below(i + 1));
        }
        p
    }
}

#[test]
fn zeros_are_ordered_and_nans_refused() {
    let (neg, pos) = (Coord::new(-0.0).unwrap(), Coord::new(0.0).unwrap());
    assert!(neg < pos);
    assert_ne!(neg, pos);
    assert_eq!(Coord::new(f64::NAN), Err(NotANumber));
    assert_eq!(Coord::new(-f64::NAN), Err(NotANumber));
    // Bitwise equality is the only tie.
    assert_eq!(
        Coord::new(0.1 + 0.2).unwrap(),
        Coord::new(0.1 + 0.2).unwrap()
    );
    assert!(Coord::new(f64::NEG_INFINITY).unwrap() < Coord::new(-f64::MAX).unwrap());
    assert!(Coord::new(-5e-324).unwrap() < neg);
    assert!(pos < Coord::new(5e-324).unwrap());

    // Two vertices at x = −0 and x = +0 with equal face keys don't tie.
    let a = VertKey::new([0.0, 1.0, 2.0], [7u8, 3]).unwrap();
    let b = VertKey::new([-0.0, 1.0, 2.0], [3u8, 7]).unwrap();
    assert!(b < a);
    assert_eq!(first([(a.clone(), 0u32), (b.clone(), 1)]), Some(1));
    assert_eq!(first([(b, 0u32), (a, 1)]), Some(0));

    assert_eq!(Pos::new([0.0, f64::NAN, 0.0]), Err(NotANumber));
    assert_eq!(VertKey::new([f64::NAN, 0.0, 0.0], [()]), Err(NotANumber));
    let v = VertKey::new([0.0; 3], [()]).unwrap();
    let w = VertKey::new([1.0; 3], [()]).unwrap();
    let mid = [0.5, f64::NAN, 0.0];
    assert_eq!(EdgeKey::new([v, w], mid, [(), ()]), Err(NotANumber));
    assert_eq!(Along::Stored.param(f64::NAN), Err(NotANumber));
    assert_eq!(Along::Reversed.cmp_params(0.0, f64::NAN), Err(NotANumber));
}

#[test]
fn vertices_by_position_then_face_keys() {
    let k = |p: [f64; 3], f: &[u8]| VertKey::new(p, f.iter().copied()).unwrap();
    assert!(k([0.0, 5.0, 5.0], &[9]) < k([1.0, 0.0, 0.0], &[0]));
    assert!(k([1.0, 0.0, 5.0], &[9]) < k([1.0, 1.0, 0.0], &[0]));
    assert!(k([1.0, 1.0, -1.0], &[9]) < k([1.0, 1.0, 0.0], &[0]));
    // Equal positions: the sorted face keys, as lists.
    assert!(k([1.0; 3], &[2, 1]) < k([1.0; 3], &[1, 3]));
    assert!(k([1.0; 3], &[1, 2]) < k([1.0; 3], &[1, 2, 2]));
    assert_eq!(k([1.0; 3], &[3, 1, 2]), k([1.0; 3], &[2, 3, 1]));
    assert_eq!(k([1.0; 3], &[3, 1, 2]).faces(), &[1, 2, 3]);
}

#[test]
fn face_keys_put_a_missing_form_last() {
    let f = |form, name| FaceOrd { form, name };
    assert!(f(Some(9u8), 9u8) < f(None, 0));
    assert!(f(Some(1), 9) < f(Some(2), 0));
    assert!(f(Some(1), 0) < f(Some(1), 1));
    assert!(f(None, 0) < f(None, 1));
    let mut keys = [f(None, 0), f(Some(3), 1), f(None, 2), f(Some(0), 5)];
    keys.sort();
    assert_eq!(keys, [f(Some(0), 5), f(Some(3), 1), f(None, 0), f(None, 2)]);
}

#[test]
fn edges_by_ends_then_middle_then_faces() {
    let v = |x: f64| VertKey::new([x, 0.0, 0.0], [0u8]).unwrap();
    let e = |a, b, y: f64, f: [u8; 2]| EdgeKey::new([v(a), v(b)], [0.5, y, 0.0], f).unwrap();
    // Either stored direction gives one key.
    assert_eq!(e(0.0, 1.0, 1.0, [1, 2]), e(1.0, 0.0, 1.0, [2, 1]));
    assert_eq!(e(1.0, 0.0, 1.0, [2, 1]).lo(), &v(0.0));
    assert_eq!(e(1.0, 0.0, 1.0, [2, 1]).faces(), &[1, 2]);
    // The lesser end first, then the other end.
    assert!(e(-1.0, 5.0, 0.0, [0, 0]) < e(0.0, 1.0, 0.0, [0, 0]));
    assert!(e(0.0, 1.0, 9.0, [9, 9]) < e(0.0, 2.0, 0.0, [0, 0]));
    // Two semicircles between one pair of vertices: the middle point.
    assert!(e(1.0, 0.0, -0.5, [9, 9]) < e(0.0, 1.0, 0.5, [0, 0]));
    assert!(e(1.0, 0.0, -0.0, [9, 9]) < e(0.0, 1.0, 0.0, [0, 0]));
    // Coincident: the faces.
    assert!(e(1.0, 0.0, 0.5, [3, 1]) < e(0.0, 1.0, 0.5, [2, 2]));
    // A closed edge: its one vertex twice.
    let c = EdgeKey::new([v(0.0), v(0.0)], [1.0, 1.0, 0.0], [4u8, 4]).unwrap();
    assert!(c < e(0.0, 1.0, 0.0, [0, 0]));
}

#[test]
fn points_on_an_edge_from_its_lesser_end() {
    // A segment from p = (0,0,0) to q = (1,0,0) at parameters −1 and 1,
    // stored from whichever end has the lower id; feet at s (parameter
    // from p's end) are at stored parameter s, or −s when stored from q.
    let p = VertKey::new([0.0; 3], [()]).unwrap();
    let q = VertKey::new([1.0, 0.0, 0.0], [()]).unwrap();
    let feet = [0.5, -0.0, 0.0, -0.75];
    let along_p = Along::of(&p, &q);
    let along_q = Along::of(&q, &p);
    assert_eq!((along_p, along_q), (Along::Stored, Along::Reversed));
    let order = |along: Along, flip: bool| {
        let mut ix: Vec<usize> = (0..feet.len()).collect();
        let t = |i: usize| if flip { -feet[i] } else { feet[i] };
        ix.sort_by(|&i, &j| along.cmp_params(t(i), t(j)).unwrap());
        ix
    };
    // From p: −0.75, then −0 and +0 (one point, tied), then 0.5, whichever
    // way the edge is stored.
    assert_eq!(order(along_p, false), [3, 1, 2, 0]);
    assert_eq!(order(along_q, true), [3, 1, 2, 0]);
    assert_eq!(along_p.cmp_params(-0.0, 0.0), Ok(Ordering::Equal));
    // The lesser end itself comes first.
    assert_eq!(along_q.cmp_params(1.0, -0.75), Ok(Ordering::Less));
    // Ends tied exactly (here a closed edge): the stored direction.
    assert_eq!(Along::of(&p, &p), Along::Stored);
}

/// A vertex: its position and incident faces' keys.
type Vert = ([f64; 3], Vec<u8>);

fn random_verts(rng: &mut Rng, n: usize) -> Vec<Vert> {
    (0..n)
        .map(|_| {
            let faces = (0..1 + rng.below(3)).map(|_| rng.below(3) as u8).collect();
            (rng.pos(), faces)
        })
        .collect()
}

fn vert_key((pos, faces): &Vert) -> VertKey<u8> {
    VertKey::new(*pos, faces.iter().copied()).unwrap()
}

/// Sorts entities `0..keys.len()` with ids `ids` into geometric order and
/// returns the entities in that order.
fn ranked<G: Ord + Clone>(keys: &[G], ids: &[u32]) -> Vec<usize> {
    let mut items: Vec<(G, u32)> = keys.iter().cloned().zip(ids.iter().copied()).collect();
    sort(&mut items);
    let entity = |id: u32| ids.iter().position(|&i| i == id).unwrap();
    items.into_iter().map(|(_, id)| entity(id)).collect()
}

/// The two orders `a` and `b` of entities with keys `keys` agree up to
/// exact ties: the same key at every place, the same entity at a place
/// whose key no other entity has.
fn agree<G: Ord + std::fmt::Debug>(keys: &[G], a: &[usize], b: &[usize]) {
    for (&i, &j) in a.iter().zip(b) {
        assert_eq!(keys[i], keys[j]);
        if keys.iter().filter(|k| **k == keys[i]).count() == 1 {
            assert_eq!(i, j);
        }
    }
}

#[test]
fn permuting_ids_changes_no_order_but_among_exact_ties() {
    for seed in varde_testing::seeds(16, 256) {
        let mut rng = Rng::new(seed);
        let n = 2 + rng.below(30);
        let verts = random_verts(&mut rng, n);
        let keys: Vec<VertKey<u8>> = verts.iter().map(vert_key).collect();
        let (ids, permuted) = (rng.perm(n), rng.perm(n));
        let (a, b) = (ranked(&keys, &ids), ranked(&keys, &permuted));
        agree(&keys, &a, &b);
        // Sorted, and first is the head.
        assert!(a.windows(2).all(|w| keys[w[0]] <= keys[w[1]]));
        let first_of = |ids: &[u32]| first(keys.iter().cloned().zip(ids.iter().copied()));
        let entity = |ids: &[u32], id| ids.iter().position(|&i| i == id).unwrap();
        assert_eq!(entity(&ids, first_of(&ids).unwrap()), a[0]);
        assert_eq!(entity(&permuted, first_of(&permuted).unwrap()), b[0]);

        // Edges between random vertices (some closed), stored from the
        // lower-id end, which the permutation may turn round.
        let m = 1 + rng.below(20);
        let edges: Vec<(usize, usize, [f64; 3], [u8; 2])> = (0..m)
            .map(|_| {
                let (u, v) = (rng.below(n), rng.below(n));
                let f = [rng.below(3) as u8, rng.below(3) as u8];
                (u, v, rng.pos(), f)
            })
            .collect();
        let edge_key = |vids: &[u32], &(u, v, mid, f): &(usize, usize, [f64; 3], [u8; 2])| {
            let (s, e) = if vids[u] <= vids[v] { (u, v) } else { (v, u) };
            EdgeKey::new([keys[s].clone(), keys[e].clone()], mid, f).unwrap()
        };
        let ekeys: Vec<_> = edges.iter().map(|e| edge_key(&ids, e)).collect();
        let ekeys2: Vec<_> = edges.iter().map(|e| edge_key(&permuted, e)).collect();
        assert_eq!(ekeys, ekeys2);
        let (eids, eperm) = (rng.perm(m), rng.perm(m));
        agree(&ekeys, &ranked(&ekeys, &eids), &ranked(&ekeys, &eperm));

        // Feet on each edge, at parameters `s` from its `u` end, the
        // curve running over [−1, 1] in its stored direction; the feet
        // numbered in stored parameter order break exact ties.
        for &(u, v, _, _) in &edges {
            let feet: Vec<f64> = (0..1 + rng.below(6)).map(|_| rng.coord()).collect();
            let order = |vids: &[u32]| {
                let from_u = vids[u] <= vids[v];
                let (s, e) = if from_u { (u, v) } else { (v, u) };
                let along = Along::of(&keys[s], &keys[e]);
                let t = |i: usize| if from_u { feet[i] } else { -feet[i] };
                // Foot ids along the stored direction.
                let mut stored: Vec<usize> = (0..feet.len()).collect();
                stored.sort_by(|&i, &j| t(i).total_cmp(&t(j)).then(i.cmp(&j)));
                let mut items: Vec<(Coord, u32)> = stored
                    .iter()
                    .enumerate()
                    .map(|(foot, &i)| (along.param(t(i)).unwrap(), foot as u32))
                    .collect();
                sort(&mut items);
                items
                    .iter()
                    .map(|&(_, foot)| stored[foot as usize])
                    .collect::<Vec<usize>>()
            };
            let (a, b) = (order(&ids), order(&permuted));
            if keys[u] == keys[v] {
                continue; // which end is lesser is an id's choice
            }
            // From the lesser end: increasing `s` from `u` if `u` is lesser,
            // the two zeros one point.
            let geo: Vec<Coord> = (feet.iter())
                .map(|&s| Coord::new(if keys[v] < keys[u] { -s } else { s } + 0.0).unwrap())
                .collect();
            assert!(a.windows(2).all(|w| geo[w[0]] <= geo[w[1]]));
            agree(&geo, &a, &b);
        }
    }
}

/// The keys' order, checked against a slow reference on the raw values:
/// coordinates by their bits mapped to `total_cmp`'s integer order, the
/// face keys as sorted lists. Heavy ties: every slot drawn from `POOL`
/// (both zeros and both subnormals in every coordinate), a few face keys,
/// closed edges and edges between bitwise-equal vertices.
#[test]
fn keys_order_as_the_reference_and_hash_with_equality() {
    use std::collections::hash_map::DefaultHasher;
    let bits = |x: f64| {
        let b = x.to_bits() as i64;
        b ^ (((b >> 63) as u64) >> 1) as i64
    };
    let pos_ref = |p: [f64; 3]| p.map(bits);
    let hash = |k: &dyn Fn(&mut DefaultHasher)| {
        let mut h = DefaultHasher::new();
        k(&mut h);
        h.finish()
    };
    type Face = FaceOrd<u8, u8>;
    for seed in varde_testing::seeds(16, 256) {
        let mut rng = Rng::new(seed);
        let face = |rng: &mut Rng| FaceOrd {
            form: [None, Some(0u8), Some(1)][rng.below(3)],
            name: rng.below(2) as u8,
        };
        let face_ref = |f: &Face| (f.form.is_none(), f.form, f.name);
        let n = 2 + rng.below(12);
        let verts: Vec<([f64; 3], Vec<Face>)> = (0..n)
            .map(|_| {
                let p = if rng.below(2) == 0 {
                    [0.0; 3]
                } else {
                    rng.pos()
                };
                let p = p.map(|c| if rng.below(4) == 0 { -c } else { c });
                (p, (0..rng.below(4)).map(|_| face(&mut rng)).collect())
            })
            .collect();
        let vref = |(p, f): &([f64; 3], Vec<Face>)| {
            let mut f: Vec<_> = f.iter().map(face_ref).collect();
            f.sort();
            (pos_ref(*p), f)
        };
        let vkeys: Vec<VertKey<Face>> = (verts.iter())
            .map(|(p, f)| VertKey::new(*p, f.iter().rev().copied()).unwrap())
            .collect();
        for i in 0..n {
            for j in 0..n {
                let (a, b) = (&vkeys[i], &vkeys[j]);
                assert_eq!(a.cmp(b), vref(&verts[i]).cmp(&vref(&verts[j])));
                if a == b {
                    assert_eq!(hash(&|h| a.hash(h)), hash(&|h| b.hash(h)));
                }
            }
        }
        let edges: Vec<(usize, usize, [f64; 3], [Face; 2])> = (0..2 + rng.below(12))
            .map(|_| {
                let u = rng.below(n);
                let v = if rng.below(3) == 0 { u } else { rng.below(n) };
                (u, v, rng.pos(), [face(&mut rng), face(&mut rng)])
            })
            .collect();
        let eref = |&(u, v, mid, f): &(usize, usize, [f64; 3], [Face; 2])| {
            let (a, b) = (vref(&verts[u]), vref(&verts[v]));
            let mut f = [face_ref(&f[0]), face_ref(&f[1])];
            f.sort();
            (a.clone().min(b.clone()), a.max(b), pos_ref(mid), f)
        };
        let ekey = |&(u, v, mid, f): &(usize, usize, [f64; 3], [Face; 2]), flip: bool| {
            let (ends, f) = if flip {
                ([vkeys[v].clone(), vkeys[u].clone()], [f[1], f[0]])
            } else {
                ([vkeys[u].clone(), vkeys[v].clone()], f)
            };
            EdgeKey::new(ends, mid, f).unwrap()
        };
        for (i, ei) in edges.iter().enumerate() {
            assert_eq!(ekey(ei, false), ekey(ei, true));
            for ej in &edges[i..] {
                let (a, b) = (ekey(ei, false), ekey(ej, true));
                assert_eq!(a.cmp(&b), eref(ei).cmp(&eref(ej)));
                if a == b {
                    assert_eq!(hash(&|h| a.hash(h)), hash(&|h| b.hash(h)));
                }
            }
            // A NaN in any slot of the middle point is refused.
            for k in 0..3 {
                let mut mid = ei.2;
                mid[k] = f64::NAN;
                let ends = [vkeys[ei.0].clone(), vkeys[ei.1].clone()];
                assert_eq!(EdgeKey::new(ends, mid, ei.3), Err(NotANumber));
                let mut p = verts[ei.0].0;
                p[k] = -f64::NAN;
                assert_eq!(VertKey::new(p, [face(&mut rng)]), Err(NotANumber));
            }
        }
        // `first` is the head of `sort`, and neither reads an id before
        // the keys: ids reversed to the key order change no key's place.
        let mut items: Vec<(EdgeKey<Face>, u32)> = (edges.iter().enumerate())
            .map(|(i, e)| (ekey(e, i % 2 == 0), (edges.len() - i) as u32))
            .collect();
        let head = first(items.iter().cloned());
        sort(&mut items);
        assert_eq!(head, Some(items[0].1));
        assert!(items.windows(2).all(|w| w[0].0 <= w[1].0));
        assert!((items.windows(2)).all(|w| w[0].0 < w[1].0 || w[0].1 < w[1].1));
    }
}

/// A stored range starting at `−0`, `[−0, 2]`, reversed as `t0 + t1 − t`
/// is `[+0, 2]` the other way round: the point at the start (`−0`) and one
/// stored at `+0` are one point, and both ways round they tie, so neither
/// stored direction puts one before the other.
#[test]
fn signed_zero_parameters_tie_in_both_stored_directions() {
    let p = VertKey::new([0.0; 3], [()]).unwrap();
    let q = VertKey::new([1.0, 0.0, 0.0], [()]).unwrap();
    let (fwd, bwd) = (Along::of(&p, &q), Along::of(&q, &p));
    let (t0, t1) = (-0.0f64, 2.0f64);
    let rev = |t: f64| t0 + t1 - t;
    for (s, t) in [(t0, 0.0), (0.0, t0), (t0, t0), (0.0, 0.0)] {
        assert_eq!(fwd.cmp_params(s, t), Ok(Ordering::Equal));
        assert_eq!(bwd.cmp_params(rev(s), rev(t)), Ok(Ordering::Equal));
    }
    // Reversed storage gives `+0` at what was the far end; read from the
    // lesser end, both zeros are the same place as a stored `−0`.
    assert_eq!(rev(t1).to_bits(), 0.0f64.to_bits());
    assert_eq!(fwd.param(-0.0), fwd.param(0.0));
    assert_eq!(bwd.param(-0.0), bwd.param(0.0));
    assert_eq!(bwd.param(0.0).unwrap().get().to_bits(), 0.0f64.to_bits());
    // Every other point keeps its order both ways round.
    assert_eq!(fwd.cmp_params(t0, 5e-324), Ok(Ordering::Less));
    assert_eq!(bwd.cmp_params(rev(t0), rev(1.0)), Ok(Ordering::Less));
    assert_eq!(bwd.cmp_params(-5e-324, 0.0), Ok(Ordering::Greater));
}

/// Points on an edge between two vertices: the same order from the
/// lesser end whichever way it is stored, for parameters with signed zeros
/// and subnormals, the stored parameter running over `[t0, t1]` and
/// reversed as `t0 + t1 − t` where that is exact.
#[test]
fn points_on_a_reversed_edge_keep_their_order() {
    for seed in varde_testing::seeds(16, 256) {
        let mut rng = Rng::new(seed);
        let p = VertKey::new(rng.pos(), [rng.below(2) as u8]).unwrap();
        let q = VertKey::new(rng.pos(), [rng.below(2) as u8]).unwrap();
        if p == q {
            continue;
        }
        // A symmetric range, reversed by exact negation (signed zeros and
        // subnormals included), or an integer one, reversed exactly on
        // integers and halves.
        let sym = rng.below(2) == 0;
        let t1 = rng.below(4) as f64 + 1.0;
        let t0 = if sym { -t1 } else { -(rng.below(4) as f64) };
        let pool: &[f64] = if sym {
            &[0.0, -0.0, 5e-324, -5e-324, 0.5, -0.5, 1.0]
        } else {
            &[0.0, 0.5, 1.0, -0.5]
        };
        let ts: Vec<f64> = (0..1 + rng.below(8))
            .map(|_| [t0, t1, pool[rng.below(pool.len())]][rng.below(3)])
            .collect();
        // Stored from p: parameter t; stored from q: t0 + t1 − t.
        let rev = |t: f64| if sym { -t } else { t0 + t1 - t };
        let mut a: Vec<usize> = (0..ts.len()).collect();
        let mut b = a.clone();
        let fwd = Along::of(&p, &q);
        let bwd = Along::of(&q, &p);
        assert_ne!(fwd, bwd);
        a.sort_by(|&i, &j| fwd.cmp_params(ts[i], ts[j]).unwrap().then(i.cmp(&j)));
        b.sort_by(|&i, &j| (bwd.cmp_params(rev(ts[i]), rev(ts[j])).unwrap()).then(i.cmp(&j)));
        let key = |i: usize| fwd.param(ts[i]).unwrap();
        for (&i, &j) in a.iter().zip(&b) {
            assert_eq!(key(i), key(j), "{ts:?}");
        }
    }
}
