use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};

use varde_decide::count::boxes::BoxKey;
use varde_decide::ids::{FaceId, VertId};
use varde_decide::records::keys::Operand;

use super::super::cluster::tests::{quad, whole, work};
use super::super::cluster::{Cluster, ClusterLeaf, Tangency, clusters};
use super::super::rewrite::tests::{snapped_bowl, tilted};
use super::super::rewrite::{EdgeClass, SnapEdge, SnapFace, SnapVert, TargetFace};
use super::*;
use crate::infra::budget::{Budget, Work};

const A: Operand = Operand::A;
const B: Operand = Operand::B;

// ---------------------------------------------------------------------
// On the target.

fn wide(_: SnapEdge) -> f64 {
    1e-3
}

fn wide_v(_: SnapVert) -> f64 {
    1e-3
}

fn tight(_: SnapEdge) -> f64 {
    1e-9
}

fn tight_v(_: SnapVert) -> f64 {
    1e-9
}

fn zero(_: SnapEdge) -> f64 {
    0.0
}

fn zero_v(_: SnapVert) -> f64 {
    0.0
}

#[test]
fn rim_edges_at_the_mid_surface_are_on_the_target_within_their_band_only() {
    // The bowl's centre snapped onto the floor: the snapped square lies on
    // it, the kept ring doesn't; the rim edges (at the mid-surface, within
    // about `2⁻¹⁴` of the floor) are on it with a band of `10⁻³`, and so
    // are their vertices, but not with `10⁻⁹`; the kept pieces of the
    // bowl's own edges never are, however wide the band.
    let (r, geo, floor) = snapped_bowl();
    let faces = [TargetFace {
        face: FaceId(0),
        tile: [0, 0],
        patch: &floor,
    }];
    let class = |k: SnapEdge| geo.edges.iter().find(|e| e.key == k).unwrap().class;
    let input = OnTInput {
        snapped: B,
        faces: &faces,
        edge_band: &wide,
        vert_band: &wide_v,
    };
    let on = on_t(&r, &geo, &input, &mut work()).expect("on-T");
    for f in &r.faces {
        assert_eq!(
            on.face(f.key),
            matches!(f.key, SnapFace::Snapped { .. }),
            "{f:?}"
        );
    }
    let mut rims = 0;
    for e in &r.edges {
        match class(e.key) {
            EdgeClass::Rim => {
                rims += 1;
                assert!(on.edge(e.key), "{e:?}");
                for v in e.ends {
                    assert!(on.vert(v), "{v:?}");
                }
            }
            EdgeClass::Kept => assert!(!on.edge(e.key), "{e:?}"),
            c => panic!("{c:?}"),
        }
    }
    assert!(rims >= 4);
    // The bowl's corners only bound kept edges: outside the window.
    for k in 0..4 {
        assert!(!on.vert(SnapVert::Vert(B, VertId(k))));
    }
    let input = OnTInput {
        edge_band: &tight,
        vert_band: &tight_v,
        ..input
    };
    let on = on_t(&r, &geo, &input, &mut work()).expect("on-T");
    // Only the branch vertices, where the gap changes sign, are within
    // `10⁻⁹` of the floor.
    assert!(on.edges.is_empty(), "{on:?}");
    assert!(!on.verts.is_empty());
    for v in &on.verts {
        assert!(matches!(v, SnapVert::Branch { .. }), "{v:?}");
        let p = geo.verts.iter().find(|g| g.key == *v).unwrap().pos;
        assert!(p[2].abs() <= 1e-9, "{p:?}");
    }
    assert_eq!(on.faces.len(), 1);
}

#[test]
fn reused_and_merged_edges_are_on_the_target_by_construction() {
    // The tilted sheet over the floor: the piece of its edge merged into
    // the floor's edge is on it with no band at all, and so are the edges
    // fitted exactly onto the floor (`z = 0`) and their vertices.
    let (r, geo, _) = tilted(1e-5);
    let geo = geo.expect("geometry");
    let floor = quad([-8.0, 0.0, 0.0], [16.0, 0.0, 0.0], [-8.0, 16.0, 0.0]);
    let faces = [TargetFace {
        face: FaceId(0),
        tile: [0, 0],
        patch: &floor,
    }];
    let input = OnTInput {
        snapped: B,
        faces: &faces,
        edge_band: &zero,
        vert_band: &zero_v,
    };
    let on = on_t(&r, &geo, &input, &mut work()).expect("on-T");
    let merged: Vec<_> = geo
        .edges
        .iter()
        .filter(|e| matches!(e.class, EdgeClass::Merged(_)))
        .collect();
    assert_eq!(merged.len(), 1);
    assert_eq!(on.edges.len(), r.edges.len());
    assert_eq!(on.verts.len(), r.verts.len());
    assert_eq!(on.faces.len(), 1);
}

#[test]
fn a_target_face_without_a_plane_or_a_bad_band_is_refused() {
    let (r, geo, floor) = snapped_bowl();
    let faces = [TargetFace {
        face: FaceId(0),
        tile: [0, 0],
        patch: &floor,
    }];
    let input = OnTInput {
        snapped: B,
        faces: &[],
        edge_band: &wide,
        vert_band: &wide_v,
    };
    assert_eq!(
        on_t(&r, &geo, &input, &mut work()),
        Err(OnTError::Chart(FaceId(0)))
    );
    let nan = |_: SnapEdge| f64::NAN;
    let input = OnTInput {
        faces: &faces,
        edge_band: &nan,
        ..input
    };
    assert_eq!(on_t(&r, &geo, &input, &mut work()), Err(OnTError::Band));
}

// ---------------------------------------------------------------------
// Windows.

#[test]
fn a_leaf_meets_a_window_where_its_box_shares_a_point_with_it() {
    let mut w = Window::new();
    w.add_box(&whole(B, 0, [0, 0]));
    // A side, a corner, and apart; and another face.
    assert!(w.meets_box(&whole(B, 0, [1, 0])));
    assert!(w.meets_box(&whole(B, 0, [-1, -1])));
    assert!(!w.meets_box(&whole(B, 0, [2, 0])));
    assert!(!w.meets_box(&whole(B, 1, [0, 0])));
    assert!(!w.meets_box(&whole(A, 0, [0, 0])));
    // A p-curve's control points' box.
    let pc = PCurve::new(
        vec![[[3.0, 0.5, 1.0], [3.5, 0.25, 2.0], [4.0, 0.5, 1.0]]],
        vec![0.0, 1.0],
    )
    .unwrap();
    let mut j = Window::new();
    j.add_pcurve(B, FaceId(0), &pc);
    assert!(j.meets_box(&whole(B, 0, [2, 0])));
    assert!(j.meets_box(&whole(B, 0, [4, 0])));
    assert!(!j.meets_box(&whole(B, 0, [5, 0])));
    assert!(!j.meets_box(&whole(B, 0, [3, 1])));
    w.add_window(&j);
    assert_eq!(w.len(), 2);
}

// ---------------------------------------------------------------------
// Stale leaves and merges.

/// A synthetic leaf: boxes of face 0 of each operand in one tile, the
/// world box the tile's unit cube.
#[derive(Clone, Debug, PartialEq)]
struct L {
    boxes: [BoxKey; 2],
    kind: Option<Tangency>,
}

fn leaf(u: i64, kind: Option<Tangency>) -> L {
    L {
        boxes: [whole(A, 0, [u, 0]), whole(B, 0, [u, 0])],
        kind,
    }
}

fn tile(l: &L) -> i64 {
    l.boxes[1].tile()[0]
}

impl ClusterLeaf for L {
    fn boxes(&self) -> [BoxKey; 2] {
        self.boxes
    }

    fn world(&self) -> [[[f64; 3]; 2]; 2] {
        let u = tile(self) as f64;
        let w = [[u, 0.0, 0.0], [u + 1.0, 1.0, 1.0]];
        [w, w]
    }

    fn tangency(&self) -> Option<Tangency> {
        self.kind
    }
}

const NEAR: Option<Tangency> = Some(Tangency::Near);
const CONTACT: Option<Tangency> = Some(Tangency::Contact);

/// What a recertification of the leaf in a tile gives.
#[derive(Clone)]
enum Recert {
    /// The leaf with this kind.
    As(Option<Tangency>),
    /// Shared curve.
    Shared,
    /// Near, and a Simple leaf in the next tile: a chain without end.
    Spawn,
    /// A Near leaf with the boxes of the leaf in this tile.
    Copy(i64),
}

/// The snap's steps, recorded: each snap's window is its cluster's
/// boxes on `B`; recertification by tile from a table (unchanged
/// elsewhere); the join pass's windows from a list per call.
#[derive(Default)]
struct Mock {
    recert: BTreeMap<i64, Recert>,
    joins: Vec<Vec<(u32, i64)>>,
    runs: Vec<(u32, Vec<i64>, Vec<u32>)>,
    join_calls: u32,
    recertified: AtomicU32,
}

impl Snapper<L, u32> for Mock {
    type Error = ();

    fn face_key(&self, _: Operand, face: FaceId) -> u32 {
        face.0
    }

    fn snap(&mut self, run: &Run<'_, u32>, leaves: &[L], _: &mut Work) -> Result<Window, ()> {
        let mut tiles: Vec<i64> = run
            .cluster
            .leaves()
            .map(|i| tile(&leaves[i as usize]))
            .collect();
        tiles.sort_unstable();
        self.runs.push((run.slot, tiles, run.replaces.to_vec()));
        Ok(cluster_window(run.cluster, leaves, B))
    }

    fn join(
        &mut self,
        _: &[(u32, &Cluster<u32>)],
        _: &[L],
        _: &mut Work,
    ) -> Result<Vec<(u32, Window)>, ()> {
        let k = self.join_calls as usize;
        self.join_calls += 1;
        Ok(self
            .joins
            .get(k)
            .into_iter()
            .flatten()
            .map(|&(slot, u)| {
                let mut w = Window::new();
                w.add_box(&whole(B, 0, [u, 0]));
                (slot, w)
            })
            .collect())
    }

    fn recertify(&self, l: &L, ledger: &mut Ledger<'_>) -> Result<Vec<Fresh<L>>, Stop<()>> {
        units(ledger, 1)?;
        self.recertified.fetch_add(1, Ordering::Relaxed);
        let u = tile(l);
        Ok(match self.recert.get(&u) {
            None => vec![Fresh::Leaf(l.clone())],
            Some(Recert::As(k)) => vec![Fresh::Leaf(leaf(u, *k))],
            Some(Recert::Shared) => vec![Fresh::SharedCurve],
            Some(Recert::Spawn) => vec![Fresh::Leaf(leaf(u, NEAR)), Fresh::Leaf(leaf(u + 1, None))],
            Some(Recert::Copy(v)) => vec![Fresh::Leaf(leaf(*v, NEAR))],
        })
    }
}

fn snap_all(
    leaves: Vec<L>,
    mock: &mut Mock,
    work: &mut Work,
) -> Result<Snapped<L, u32>, StaleError<()>> {
    let cs = clusters(&leaves, |_, f| f.0, work).expect("clusters");
    snap_clusters(cs, leaves, mock, work)
}

fn tiles_of(s: &Snapped<L, u32>, d: &Done<u32>) -> Vec<i64> {
    let mut t: Vec<i64> = d
        .cluster
        .leaves()
        .map(|i| tile(&s.leaves[i as usize]))
        .collect();
    t.sort_unstable();
    t
}

#[test]
fn apart_clusters_snap_once_each_and_nothing_is_recertified() {
    let leaves = vec![leaf(0, NEAR), leaf(10, CONTACT), leaf(20, None)];
    let mut mock = Mock::default();
    let s = snap_all(leaves, &mut mock, &mut work()).expect("snapped");
    assert_eq!(s.runs, 2);
    assert_eq!(s.merges, 0);
    assert_eq!(mock.join_calls, 1);
    assert_eq!(mock.recertified.load(Ordering::Relaxed), 0);
    assert_eq!(s.clusters.len(), 2);
    assert_eq!(s.free, [2]);
    assert_eq!(mock.runs[0], (0, vec![0], vec![]));
    assert_eq!(mock.runs[1], (1, vec![10], vec![]));
}

#[test]
fn a_recertified_leaf_of_the_cluster_s_kind_merges_and_the_cluster_re_runs_until_none_grows() {
    // Simple leaves in tiles 1, 2, 3 beside a Near cluster in tile 0 come
    // back Near: each re-run's window reaches the next one, which merges
    // in turn; the leaf in tile 4 stays Simple, and the chain ends.
    let leaves = vec![
        leaf(0, NEAR),
        leaf(1, None),
        leaf(2, None),
        leaf(3, None),
        leaf(4, None),
    ];
    let mut mock = Mock::default();
    for u in 1..4 {
        mock.recert.insert(u, Recert::As(NEAR));
    }
    let s = snap_all(leaves, &mut mock, &mut work()).expect("snapped");
    assert_eq!(s.merges, 3);
    assert_eq!(s.runs, 4);
    assert_eq!(s.clusters.len(), 1);
    assert_eq!(tiles_of(&s, &s.clusters[0]), [0, 1, 2, 3]);
    let replaces: Vec<Vec<u32>> = mock.runs.iter().map(|r| r.2.clone()).collect();
    assert_eq!(replaces, [vec![], vec![0], vec![0], vec![0]]);
    // The leaf in tile 4, recertified as it was, is free.
    assert_eq!(s.free.len(), 1);
    assert_eq!(tile(&s.leaves[s.free[0] as usize]), 4);
    assert!(s.clusters[0].window.meets_box(&whole(B, 0, [4, 0])));
}

#[test]
fn merges_without_end_run_out_of_budget() {
    // Every recertification gives a Near leaf and a new Simple leaf beside
    // it: the cluster would grow forever; the budget ends it.
    let leaves = vec![leaf(0, NEAR), leaf(1, None)];
    let mut mock = Mock::default();
    for u in 1..=4096 {
        mock.recert.insert(u, Recert::Spawn);
    }
    let mut w = Work::new(&Budget::new(20_000));
    let out = snap_all(leaves, &mut mock, &mut w);
    assert!(matches!(out, Err(StaleError::OutOfBudget(_))), "{out:?}");
    assert!(mock.runs.len() > 10);
}

#[test]
fn a_leaf_the_cluster_already_holds_adds_nothing() {
    // The Simple leaf beside the cluster comes back as a copy of the
    // cluster's own leaf: no growth, no re-run.
    let leaves = vec![leaf(0, NEAR), leaf(1, None)];
    let mut mock = Mock::default();
    mock.recert.insert(1, Recert::Copy(0));
    let s = snap_all(leaves, &mut mock, &mut work()).expect("snapped");
    assert_eq!(s.runs, 1);
    assert_eq!(s.merges, 0);
    assert_eq!(tiles_of(&s, &s.clusters[0]), [0]);
    assert!(s.free.is_empty());
}

#[test]
fn shared_curves_are_exempt_and_the_other_kind_abuts() {
    // Beside a Near cluster: a leaf coincident along a shared curve (no
    // leaf left), and one that comes back Contact (the abutment case, left
    // free).
    let leaves = vec![leaf(5, None), leaf(6, NEAR), leaf(7, None)];
    let mut mock = Mock::default();
    mock.recert.insert(5, Recert::Shared);
    mock.recert.insert(7, Recert::As(CONTACT));
    let s = snap_all(leaves, &mut mock, &mut work()).expect("snapped");
    assert_eq!(s.exempt, [0]);
    assert_eq!(s.abutments.len(), 1);
    let a = s.abutments[0];
    assert_eq!(a.cluster, 0);
    assert_eq!(s.leaves[a.leaf as usize], leaf(7, CONTACT));
    assert_eq!(s.free, [a.leaf]);
    assert_eq!(s.runs, 1);
}

#[test]
fn a_later_cluster_reached_by_a_window_merges_whole_and_never_runs_alone() {
    // Near clusters in tiles 0 and 2–3 (apart, so two clusters); after
    // the first snaps, a Simple leaf in tile 1 comes back Near and merges
    // into it; the re-run's window reaches the second cluster's leaf in
    // tile 2, recertified as it was (Near): the second cluster joins the
    // first whole, before it ever runs.
    let leaves = vec![leaf(0, NEAR), leaf(1, None), leaf(2, NEAR), leaf(3, NEAR)];
    let mut mock = Mock::default();
    mock.recert.insert(1, Recert::As(NEAR));
    let s = snap_all(leaves, &mut mock, &mut work()).expect("snapped");
    assert_eq!(s.clusters.len(), 1);
    assert_eq!(s.clusters[0].parts, [0, 1]);
    assert_eq!(tiles_of(&s, &s.clusters[0]), [0, 1, 2, 3]);
    assert!(mock.runs.iter().all(|r| r.0 == 0), "{:?}", mock.runs);
}

#[test]
fn two_snapped_clusters_met_by_one_leaf_merge_and_re_run_from_both() {
    // Near clusters in tiles 0 and 2 both snap; the join pass's window of
    // the second reaches the Simple leaf in tile 1, which comes back Near
    // and meets both windows: one cluster, re-run replacing both, and the
    // join pass runs again.
    let leaves = vec![leaf(0, NEAR), leaf(1, None), leaf(2, NEAR)];
    // The leaf in tile 1 meets both clusters' windows already: recertified
    // as it was after each snap, and after the join pass.
    let mut mock = Mock {
        joins: vec![vec![(1, 2)]],
        ..Mock::default()
    };
    let s = snap_all(leaves.clone(), &mut mock, &mut work()).expect("snapped");
    assert_eq!(s.clusters.len(), 2, "unchanged leaf: no merge");
    assert_eq!(mock.join_calls, 1);

    struct Late(Mock);
    impl Snapper<L, u32> for Late {
        type Error = ();
        fn face_key(&self, o: Operand, f: FaceId) -> u32 {
            self.0.face_key(o, f)
        }
        fn snap(&mut self, r: &Run<'_, u32>, l: &[L], w: &mut Work) -> Result<Window, ()> {
            self.0.snap(r, l, w)
        }
        fn join(
            &mut self,
            s: &[(u32, &Cluster<u32>)],
            l: &[L],
            w: &mut Work,
        ) -> Result<Vec<(u32, Window)>, ()> {
            self.0.join(s, l, w)
        }
        fn recertify(&self, l: &L, ledger: &mut Ledger<'_>) -> Result<Vec<Fresh<L>>, Stop<()>> {
            // Near once the join pass ran.
            if tile(l) == 1 && self.0.join_calls > 0 {
                units(ledger, 1)?;
                return Ok(vec![Fresh::Leaf(leaf(1, NEAR))]);
            }
            self.0.recertify(l, ledger)
        }
    }
    let mut late = Late(Mock {
        joins: vec![vec![(1, 2)]],
        ..Mock::default()
    });
    let cs = clusters(&leaves, |_, f| f.0, &mut work()).unwrap();
    let s = snap_clusters(cs, leaves, &mut late, &mut work()).expect("snapped");
    assert_eq!(s.clusters.len(), 1);
    assert_eq!(s.clusters[0].parts, [0, 1]);
    assert_eq!(tiles_of(&s, &s.clusters[0]), [0, 1, 2]);
    assert_eq!(s.merges, 1);
    let last = late.0.runs.last().unwrap();
    assert_eq!(*last, (0, vec![0, 1, 2], vec![0, 1]));
    assert_eq!(late.0.join_calls, 2);
}

#[test]
fn leaves_of_snapped_clusters_are_never_recertified() {
    // Two Near clusters abutting only through a Contact cluster between
    // them (kinds never join): each window meets the next cluster's
    // leaves, but once a cluster has snapped its leaves are not stale.
    let leaves = vec![leaf(0, NEAR), leaf(1, CONTACT), leaf(2, NEAR)];
    let mut mock = Mock::default();
    let s = snap_all(leaves, &mut mock, &mut work()).expect("snapped");
    assert_eq!(s.runs, 3);
    // After the first snap: the Contact leaf (its cluster not yet run);
    // after the second: the third cluster's leaf; after the third: none.
    assert_eq!(mock.recertified.load(Ordering::Relaxed), 2);
    assert_eq!(s.clusters.len(), 3);
    assert!(s.abutments.is_empty());
}

#[test]
fn a_leaf_in_two_clusters_is_refused() {
    let leaves = vec![leaf(0, NEAR), leaf(1, NEAR)];
    let cs = clusters(&leaves, |_, f| f.0, &mut work()).unwrap();
    let twice = vec![cs[0].clone(), cs[0].clone()];
    let out = snap_clusters(twice, leaves, &mut Mock::default(), &mut work());
    assert!(
        matches!(out, Err(StaleError::Cluster(ClusterError::Kind(_)))),
        "{out:?}"
    );
}
