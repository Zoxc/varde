use malachite::Rational;
use varde_decide::bezier::{Dyadic, HomPatch};
use varde_decide::count::boxes::{BoxKey, Extent};
use varde_decide::ids::FaceId;
use varde_decide::leaves::flat::{
    BoxUse, FlatFace, FlatPiece, Kind, Leaf, LeafWork, certify, leaves,
};
use varde_decide::order::FaceOrd;
use varde_decide::records::keys::Operand;
use varde_testkit::rng::Rng;
use varde_testkit::sheets::{self, OFFSET, Pair, Polygons, Wavy};

use super::*;
use crate::geom::form::{FormKey, PlaneKey};
use crate::infra::budget::{Budget, Work};

/// The gap bound of the snap at the default tolerance, `ε/2 = 5·10⁻⁴`.
pub(crate) const B_K: f64 = 5e-4;

pub(crate) type Key = FaceOrd<FormKey, ()>;

pub(crate) fn rat(x: f64) -> Rational {
    Rational::try_from(x).expect("finite")
}

fn h(p: [f64; 3]) -> [Rational; 4] {
    [rat(p[0]), rat(p[1]), rat(p[2]), Rational::from(1)]
}

/// The parallelogram `p0 + u·(pu − p0) + v·(pv − p0)` as a bilinear tile.
pub(crate) fn quad(p0: [f64; 3], pu: [f64; 3], pv: [f64; 3]) -> HomPatch<Rational> {
    let p11 = [0, 1, 2].map(|k| pu[k] + pv[k] - p0[k]);
    HomPatch::new(1, 1, [h(p0), h(pv), h(pu), h(p11)]).expect("a bilinear net")
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|k| a[k] - b[k])
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The Newell normal of face `f`: its outward side.
fn newell(body: &Polygons, f: usize) -> [f64; 3] {
    let l = &body.faces[f];
    let mut n = [0.0; 3];
    for k in 0..l.len() {
        let (p, q) = (
            body.verts[l[k] as usize],
            body.verts[l[(k + 1) % l.len()] as usize],
        );
        n = [0, 1, 2].map(|i| n[i] + cross(p, q)[i]);
    }
    n
}

/// Face `f`'s key: its plane's.
pub(crate) fn face_key(body: &Polygons, f: usize) -> Key {
    let p = body.verts[body.faces[f][0] as usize];
    FaceOrd {
        form: Some(FormKey::Plane(
            PlaneKey::new(newell(body, f), p).expect("a plane"),
        )),
        name: (),
    }
}

/// A face's tile and whether the face is flipped against it.
pub(crate) type TileOf = fn(&Polygons, usize) -> (HomPatch<Rational>, bool);

/// The tile of an axis-aligned face: its bounding rectangle in its
/// plane, from the least corner along the other two axes in order, so it
/// doesn't depend on the vertex ids or where the loop starts.
pub(crate) fn rectangle(body: &Polygons, f: usize) -> (HomPatch<Rational>, bool) {
    let n = newell(body, f);
    let k = (0..3).find(|&k| n[k] != 0.0).expect("a face has a normal");
    assert!((0..3).all(|j| j == k || n[j] == 0.0), "axis-aligned");
    let corners = body.faces[f].iter().map(|&v| body.verts[v as usize]);
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in corners {
        for j in 0..3 {
            lo[j] = if p[j] < lo[j] { p[j] } else { lo[j] };
            hi[j] = if p[j] > hi[j] { p[j] } else { hi[j] };
        }
    }
    let [i, j] = match k {
        0 => [1, 2],
        1 => [0, 2],
        _ => [0, 1],
    };
    let mut pu = lo;
    pu[i] = hi[i];
    let mut pv = lo;
    pv[j] = hi[j];
    let flipped = dot(cross(sub(pu, lo), sub(pv, lo)), n) < 0.0;
    (quad(lo, pu, pv), flipped)
}

/// The tile of any polygon face: the parallelogram on its first corner
/// and the corners after and before it, facing as the loop does.
pub(crate) fn parallelogram(body: &Polygons, f: usize) -> (HomPatch<Rational>, bool) {
    let l = &body.faces[f];
    let v = |k: u32| body.verts[k as usize];
    (quad(v(l[0]), v(l[1]), v(l[l.len() - 1])), false)
}

/// Every leaf of every face pair of `pair`, each face's tile split
/// `depth` times in each direction before testing.
pub(crate) fn fixture_leaves(pair: &Pair, tile_of: TileOf, depth: u32) -> Vec<Leaf> {
    let ta: Vec<_> = (0..pair.a.faces.len())
        .map(|f| tile_of(&pair.a, f))
        .collect();
    let tb: Vec<_> = (0..pair.b.faces.len())
        .map(|f| tile_of(&pair.b, f))
        .collect();
    let mut out = Vec::new();
    for (fa, (pa, flip_a)) in ta.iter().enumerate() {
        for (fb, (pb, flip_b)) in tb.iter().enumerate() {
            let (ls, _) = leaves(
                FlatFace {
                    patch: pa,
                    flipped: *flip_a,
                },
                whole(Operand::A, fa as u32, [0, 0]),
                FlatFace {
                    patch: pb,
                    flipped: *flip_b,
                },
                whole(Operand::B, fb as u32, [0, 0]),
                &rat(B_K),
                1 << 20,
                |b| {
                    if b.extent(Dir::U).depth() < depth || b.extent(Dir::V).depth() < depth {
                        BoxUse::Split
                    } else {
                        BoxUse::Test
                    }
                },
            )
            .expect("flat pieces certify");
            out.extend(ls);
        }
    }
    out
}

pub(crate) fn work() -> Work {
    Work::new(&Budget::DEFAULT)
}

/// The clusters of a fixture's leaves, faces keyed by their planes.
pub(crate) fn fixture_clusters(pair: &Pair, leaves: &[Leaf]) -> Vec<Cluster<Key>> {
    clusters(
        leaves,
        |op, f| match op {
            Operand::A => face_key(&pair.a, f.0 as usize),
            Operand::B => face_key(&pair.b, f.0 as usize),
        },
        &mut work(),
    )
    .expect("clusters")
}

/// What the order and the grouping consist of, without ids: per cluster
/// its kind, its key and its leaves' world boxes in order.
type Signature = Vec<(Tangency, ClusterKey<Key>, Vec<[[[f64; 3]; 2]; 2]>)>;

fn signature(cs: &[Cluster<Key>], leaves: &[Leaf]) -> Signature {
    cs.iter()
        .map(|c| {
            let worlds = c.leaves().map(|i| leaves[i as usize].world).collect();
            (c.kind(), c.key().clone(), worlds)
        })
        .collect()
}

#[test]
fn offset_fixtures_form_no_clusters() {
    // Flat faces δ apart certify Empty or Simple: nothing to snap.
    for pair in [
        sheets::box_edge_fold(OFFSET),
        sheets::l_corner(OFFSET),
        sheets::l_corner_quad(OFFSET),
    ] {
        let ls = fixture_leaves(&pair, rectangle, 2);
        assert!(!ls.is_empty());
        assert_eq!(fixture_clusters(&pair, &ls), []);
    }
    let small = Wavy {
        cells: 3,
        ..Wavy::DEFAULT
    };
    let pair = sheets::wavy(&small);
    let ls = fixture_leaves(&pair, parallelogram, 0);
    assert_eq!(fixture_clusters(&pair, &ls), []);
}

#[test]
fn a_flush_quad_on_the_l_is_one_near_cluster() {
    let pair = sheets::l_corner_quad(0.0);
    let ls = fixture_leaves(&pair, rectangle, 2);
    let cs = fixture_clusters(&pair, &ls);
    let [c] = &cs[..] else { panic!("{cs:?}") };
    assert_eq!(c.kind(), Tangency::Near);
    // Every Near leaf is in it, in leaf-key order.
    let near: Vec<u32> = (0..ls.len() as u32)
        .filter(|&i| matches!(ls[i as usize].kind, Kind::Near(_)))
        .collect();
    let mut held: Vec<u32> = c.leaves().collect();
    held.sort_unstable();
    assert_eq!(held, near);
    let keys: Vec<_> = (0..c.leaves().len())
        .map(|k| c.leaf_key(k).unwrap())
        .collect();
    assert!(keys.windows(2).all(|w| w[0] <= w[1]));
    // Its corner is the L's corner; both faces lie in `z = 0`.
    assert_eq!(c.key().corner().get(), [0.0, 0.0, 0.0]);
    let z0 = face_key(&pair.a, 0);
    assert_eq!(c.key().target(), &z0);
    assert_eq!(c.key().pair(), &[z0, z0]);
}

#[test]
fn flush_folds_give_a_cluster_per_face_pair_in_corner_order() {
    // The box's front (face 2) behind the front quad (1), its top (1)
    // under the top quad (0): their leaves abut in no face's UV, so two
    // clusters; the front's least corner `(0, 0, 0)` comes before the
    // top's `(0, 0, 20)`.
    let pair = sheets::box_edge_fold(0.0);
    let ls = fixture_leaves(&pair, rectangle, 2);
    let cs = fixture_clusters(&pair, &ls);
    let faces = |c: &Cluster<Key>| {
        let mut fs: Vec<(u32, u32)> = c
            .leaves()
            .map(|i| (ls[i as usize].a.face().0, ls[i as usize].b.face().0))
            .collect();
        fs.dedup();
        fs
    };
    assert_eq!(cs.len(), 2, "{cs:?}");
    assert_eq!(faces(&cs[0]), [(2, 1)]);
    assert_eq!(faces(&cs[1]), [(1, 0)]);
    assert_eq!(cs[0].key().corner().get(), [0.0, 0.0, 0.0]);
    assert_eq!(cs[1].key().corner().get(), [0.0, 0.0, 20.0]);
    // In the L's reflex edge both clusters have the least corner
    // `(15, 15, 0)`: the least face key decides, the plane `y = 15`
    // (normal `(0, 1, 0)`) before `x = 15`.
    let pair = sheets::l_corner(0.0);
    let ls = fixture_leaves(&pair, rectangle, 2);
    let cs = fixture_clusters(&pair, &ls);
    assert_eq!(cs.len(), 2, "{cs:?}");
    for c in &cs {
        assert_eq!(c.kind(), Tangency::Near);
        assert_eq!(c.key().corner().get(), [15.0, 15.0, 0.0]);
    }
    let normal = |c: &Cluster<Key>| match c.key().target().form {
        Some(FormKey::Plane(k)) => k.normal(),
        _ => panic!("a plane"),
    };
    assert_eq!(normal(&cs[0]), [0.0, 1.0, 0.0]);
    assert_eq!(normal(&cs[1]), [1.0, 0.0, 0.0]);
}

/// `v` shuffled by `rng`.
fn shuffled<T>(mut v: Vec<T>, rng: &mut Rng) -> Vec<T> {
    for k in (1..v.len()).rev() {
        let j = (rng.next_u64() % (k as u64 + 1)) as usize;
        v.swap(k, j);
    }
    v
}

#[test]
fn relabelled_fixtures_give_the_same_clusters_in_the_same_order() {
    let mut rng = Rng::new(29);
    for pair in [
        sheets::box_edge_fold(0.0),
        sheets::l_corner(0.0),
        sheets::l_corner_quad(0.0),
    ] {
        let ls = fixture_leaves(&pair, rectangle, 2);
        let expected = signature(&fixture_clusters(&pair, &ls), &ls);
        assert!(!expected.is_empty());
        for _ in 0..varde_testing::pick(2, 6) {
            // New vertex and face ids, loops starting elsewhere, and the
            // leaves given in another order.
            let p = Pair {
                a: pair.a.relabelled(&mut rng),
                b: pair.b.relabelled(&mut rng),
            };
            let ls = shuffled(fixture_leaves(&p, rectangle, 2), &mut rng);
            assert_eq!(signature(&fixture_clusters(&p, &ls), &ls), expected);
        }
    }
}

// Synthetic leaves: boxes placed by hand, the kinds taken from real
// certificates.

/// A Near and a Contact certificate: one square against itself, facing
/// the same way and the other way.
fn kinds() -> (Kind, Kind) {
    let s = quad([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let b = whole(Operand::A, 0, [0, 0]);
    let p = FlatPiece::of_box(&s, &b, false).unwrap();
    let q = FlatPiece::of_box(&s, &b, true).unwrap();
    let mut w = LeafWork::default();
    let near = certify(&p, &p, &rat(B_K), &mut w).unwrap();
    let contact = certify(&p, &q, &rat(B_K), &mut w).unwrap();
    assert!(matches!(near, Kind::Near(_)) && matches!(contact, Kind::Contact(_)));
    (near, contact)
}

pub(crate) fn whole(op: Operand, face: u32, tile: [i64; 2]) -> BoxKey {
    BoxKey::new(op, FaceId(face), tile, Extent::WHOLE, Extent::WHOLE)
}

/// The extent `[a, b]/2^e`.
pub(crate) fn ext(a: u64, b: u64, e: u32) -> Extent {
    Extent::new(Dyadic::new(a, e).unwrap(), Dyadic::new(b, e).unwrap()).unwrap()
}

/// A box of `face` of `op` in tile `tile` over `u × v`.
fn bx(op: Operand, face: u32, tile: [i64; 2], u: Extent, v: Extent) -> BoxKey {
    BoxKey::new(op, FaceId(face), tile, u, v)
}

/// A leaf of `kind` between `a` and `b` whose world boxes are the unit
/// cube moved to `at` (both pieces).
fn leaf(a: BoxKey, b: BoxKey, kind: &Kind, at: [f64; 3]) -> Leaf {
    let w = [at, at.map(|x| x + 1.0)];
    Leaf {
        a,
        b,
        kind: kind.clone(),
        world: [w, w],
    }
}

/// Synthetic face keys: the face id's own number, operand `A`'s first;
/// `relabel` maps a face id back to the face it stands for.
fn synthetic(ls: &[Leaf], relabel: impl Fn(Operand, u32) -> u32 + Sync) -> Vec<Cluster<(u8, u32)>> {
    clusters(ls, |op, f| (op as u8, relabel(op, f.0)), &mut work()).unwrap()
}

/// Per cluster its leaves' positions in order.
fn groups<K: Ord + Clone>(cs: &[Cluster<K>]) -> Vec<Vec<u32>> {
    cs.iter().map(|c| c.leaves().collect()).collect()
}

#[test]
fn boxes_join_when_they_share_a_side_or_a_corner() {
    let (near, _) = kinds();
    let (a, b) = (Operand::A, Operand::B);
    let half = |lo| ext(lo, lo + 1, 1);
    let quarter = |lo| ext(lo, lo + 1, 2);
    let ls = vec![
        // Corner to corner on A's face 0, B's boxes on faces apart.
        leaf(
            bx(a, 0, [0, 0], half(0), half(0)),
            whole(b, 0, [0, 0]),
            &near,
            [0.0; 3],
        ),
        leaf(
            bx(a, 0, [0, 0], half(1), half(1)),
            whole(b, 1, [0, 0]),
            &near,
            [1.0; 3],
        ),
        // A quarter apart from both on face 1: alone.
        leaf(
            bx(a, 1, [0, 0], quarter(0), quarter(0)),
            whole(b, 2, [0, 0]),
            &near,
            [2.0; 3],
        ),
        leaf(
            bx(a, 1, [0, 0], half(1), quarter(0)),
            whole(b, 3, [0, 0]),
            &near,
            [3.0; 3],
        ),
        // Across a tile line: tile (0, 0)'s right half and tile (1, 0)'s
        // left half share the line `u = 1`; tile (2, 0) is a tile apart.
        leaf(
            bx(a, 2, [0, 0], half(1), half(0)),
            whole(b, 4, [0, 0]),
            &near,
            [4.0; 3],
        ),
        leaf(
            bx(a, 2, [1, 0], half(0), half(1)),
            whole(b, 5, [0, 0]),
            &near,
            [5.0; 3],
        ),
        leaf(
            bx(a, 2, [2, 0], half(0), half(0)),
            whole(b, 6, [0, 0]),
            &near,
            [6.0; 3],
        ),
        // Different faces of A, joined through a box of B they share.
        leaf(
            whole(a, 3, [0, 0]),
            bx(b, 7, [0, 0], half(0), half(0)),
            &near,
            [7.0; 3],
        ),
        leaf(
            whole(a, 4, [0, 0]),
            bx(b, 7, [0, 0], half(0), half(0)),
            &near,
            [8.0; 3],
        ),
    ];
    let cs = synthetic(&ls, |_, f| f);
    assert_eq!(
        groups(&cs),
        [
            vec![0, 1],
            vec![2],
            vec![3],
            vec![4, 5],
            vec![6],
            vec![7, 8]
        ]
    );
}

#[test]
fn near_and_contact_never_join_and_other_kinds_take_no_part() {
    let (near, contact) = kinds();
    let (a, b) = (Operand::A, Operand::B);
    let simple = Kind::Simple {
        ell: [0, 1, 2].map(|k| Rational::from(i64::from(k == 2))),
    };
    let half = |lo| ext(lo, lo + 1, 1);
    let ls = vec![
        leaf(
            bx(a, 0, [0, 0], half(0), half(0)),
            whole(b, 0, [0, 0]),
            &near,
            [0.0; 3],
        ),
        // Abuts both Near leaves, but is Contact.
        leaf(
            bx(a, 0, [0, 0], half(1), half(0)),
            whole(b, 1, [0, 0]),
            &contact,
            [1.0; 3],
        ),
        // A Simple leaf over both Near leaves' boxes bridges nothing.
        leaf(whole(a, 0, [0, 0]), whole(b, 2, [0, 0]), &simple, [2.0; 3]),
        leaf(
            bx(a, 0, [0, 0], half(1), half(1)),
            whole(b, 3, [0, 0]),
            &near,
            [3.0; 3],
        ),
    ];
    let cs = synthetic(&ls, |_, f| f);
    assert_eq!(groups(&cs), [vec![0, 3], vec![1]]);
    assert_eq!(cs[0].kind(), Tangency::Near);
    assert_eq!(cs[1].kind(), Tangency::Contact);
}

#[test]
fn the_order_is_geometric_whatever_the_ids_and_the_input_order() {
    let (near, contact) = kinds();
    let (a, b) = (Operand::A, Operand::B);
    let half = |lo| ext(lo, lo + 1, 1);
    // Four clusters on four faces of each operand: corners at x = 3, 1,
    // 2, and 1 again, the last tie decided by the face keys; each of two
    // leaves whose corners differ.
    let base = [
        (0, 0, [3.0, 0.0, 0.0], &near),
        (0, 0, [4.0, 0.0, 0.0], &near),
        (1, 1, [1.0, 0.0, 5.0], &contact),
        (1, 1, [1.0, 0.0, 4.0], &contact),
        (2, 2, [2.0, 0.0, 0.0], &near),
        (2, 2, [2.0, 1.0, 0.0], &near),
        (3, 3, [1.0, 0.0, 4.0], &near),
        (3, 3, [9.0, 0.0, 0.0], &near),
    ];
    let make = |fa: &dyn Fn(u32) -> u32, fb: &dyn Fn(u32) -> u32, order: &[usize]| {
        order
            .iter()
            .map(|&k| {
                let (f, g, at, kind) = base[k];
                // The two leaves of a cluster: the halves of one box.
                let u = half((k % 2) as u64);
                leaf(
                    bx(a, fa(f), [0, 0], u, Extent::WHOLE),
                    whole(b, fb(g), [0, 0]),
                    kind,
                    at,
                )
            })
            .collect::<Vec<_>>()
    };
    let order: Vec<usize> = (0..base.len()).collect();
    let ls = make(&|f| f, &|g| g, &order);
    let cs = synthetic(&ls, |_, f| f);
    let corners: Vec<[f64; 3]> = cs.iter().map(|c| c.key().corner().get()).collect();
    assert_eq!(
        corners,
        [
            [1.0, 0.0, 4.0],
            [1.0, 0.0, 4.0],
            [2.0, 0.0, 0.0],
            [3.0, 0.0, 0.0]
        ]
    );
    // The tie at `(1, 0, 4)`: A's face 1 (key `(0, 1)`) before face 3.
    assert_eq!(
        groups(&cs),
        [vec![3, 2], vec![6, 7], vec![4, 5], vec![0, 1]]
    );
    let expected: Vec<_> = cs
        .iter()
        .map(|c| {
            let worlds: Vec<_> = c.leaves().map(|i| ls[i as usize].world).collect();
            (c.kind(), c.key().clone(), worlds)
        })
        .collect();
    // Faces renumbered (the keys follow the faces, not the ids), leaves
    // shuffled.
    let mut rng = Rng::new(5);
    let perm = [2u32, 3, 0, 1];
    let inv = [2u32, 3, 0, 1];
    for _ in 0..varde_testing::pick(4, 16) {
        let order = shuffled(order.clone(), &mut rng);
        let ls = make(&|f| perm[f as usize], &|g| 3 - g, &order);
        let cs = synthetic(&ls, |op, f| match op {
            Operand::A => inv[f as usize],
            Operand::B => 3 - f,
        });
        let got: Vec<_> = cs
            .iter()
            .map(|c| {
                let worlds: Vec<_> = c.leaves().map(|i| ls[i as usize].world).collect();
                (c.kind(), c.key().clone(), worlds)
            })
            .collect();
        assert_eq!(got, expected);
    }
}

#[test]
fn bad_leaves_and_a_short_budget_are_refused() {
    let (near, _) = kinds();
    let (a, b) = (Operand::A, Operand::B);
    let ok = leaf(whole(a, 0, [0, 0]), whole(b, 0, [0, 0]), &near, [0.0; 3]);
    let swapped = leaf(whole(b, 0, [0, 0]), whole(a, 0, [0, 0]), &near, [0.0; 3]);
    let run = |ls: &[Leaf], budget: u64| {
        clusters(
            ls,
            |op, f| (op as u8, f.0),
            &mut Work::new(&Budget::new(budget)),
        )
    };
    assert_eq!(
        run(&[ok.clone(), swapped], 1000),
        Err(ClusterError::Operands(1))
    );
    let mut nan = ok.clone();
    nan.world[1][0][2] = f64::NAN;
    assert_eq!(
        run(&[ok.clone(), nan], 1000),
        Err(ClusterError::NotANumber(1))
    );
    let many: Vec<Leaf> = (0..8).map(|_| ok.clone()).collect();
    assert!(run(&many, 1000).is_ok());
    assert!(matches!(
        run(&many, 4),
        Err(ClusterError::OutOfBudget(KernelError::OutOfBudget { .. }))
    ));
}

#[test]
fn clusters_of_one_kind_sharing_a_window_element_merge() {
    let (near, contact) = kinds();
    let (a, b) = (Operand::A, Operand::B);
    let ls = vec![
        leaf(
            whole(a, 0, [0, 0]),
            whole(b, 0, [0, 0]),
            &near,
            [3.0, 0.0, 0.0],
        ),
        leaf(
            whole(a, 1, [0, 0]),
            whole(b, 1, [0, 0]),
            &near,
            [1.0, 0.0, 0.0],
        ),
        leaf(
            whole(a, 2, [0, 0]),
            whole(b, 2, [0, 0]),
            &contact,
            [2.0, 0.0, 0.0],
        ),
        leaf(
            whole(a, 3, [0, 0]),
            whole(b, 3, [0, 0]),
            &near,
            [0.0, 0.0, 0.0],
        ),
        leaf(
            whole(a, 4, [0, 0]),
            whole(b, 4, [0, 0]),
            &near,
            [4.0, 0.0, 0.0],
        ),
    ];
    let cs = synthetic(&ls, |_, f| f);
    // In order: leaves 3, 1, 2, 0, 4.
    assert_eq!(groups(&cs), [vec![3], vec![1], vec![2], vec![0], vec![4]]);
    // Clusters 0 and 3 share `x`, 3 and 1 share `y` (so all three merge),
    // the Contact cluster 2 shares `x` too but is of the other kind;
    // cluster 4 shares nothing.
    let windows = vec![
        vec!["x", "p"],
        vec!["y"],
        vec!["x"],
        vec!["y", "x"],
        vec!["q"],
    ];
    let merged = merge_sharing(cs.clone(), &windows, &mut work()).unwrap();
    let parts: Vec<_> = merged.iter().map(|m| m.parts.clone()).collect();
    assert_eq!(parts, [vec![0, 1, 3], vec![2], vec![4]]);
    let m = &merged[0].cluster;
    assert_eq!(m.kind(), Tangency::Near);
    assert_eq!(m.leaves().collect::<Vec<_>>(), [3, 1, 0]);
    // The key is the least part by part: cluster 1's corner (leaf 3's),
    // cluster 3's face key and pair (leaf 0's faces).
    assert_eq!(m.key().corner().get(), [0.0; 3]);
    assert_eq!(m.key().target(), &(0, 0));
    assert_eq!(m.key().pair(), &[(0, 0), (1, 0)]);
    assert_eq!(&merged[1].cluster, &cs[2]);
    assert_eq!(&merged[2].cluster, &cs[4]);
    // Nothing shared: nothing merged.
    let apart: Vec<Vec<&str>> = (0..5).map(|_| vec![]).collect();
    let same = merge_sharing(cs.clone(), &apart, &mut work()).unwrap();
    assert_eq!(same.into_iter().map(|m| m.cluster).collect::<Vec<_>>(), cs);
    assert_eq!(
        merge_sharing(cs, &windows[..2], &mut work()),
        Err(ClusterError::Windows)
    );
}
