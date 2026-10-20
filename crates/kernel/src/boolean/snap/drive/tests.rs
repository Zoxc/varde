use std::collections::BTreeSet;

use varde_decide::ids::EdgeId;
use varde_decide::num::decide::Sign;
use varde_decide::records::keys::Operand;
use varde_testkit::rational_oracle::ExactPair;
use varde_testkit::rng::Rng;
use varde_testkit::sheets::{self, OFFSET, Pair, Wavy};

use super::super::rewrite::{EdgeClass, SnapEdge};
use super::super::rim::Along;
use super::op::{Settled, operation};
use super::run::{Setup, rim_cut_points};
use super::zone::ZoneError;
use super::*;
use crate::boolean::arrange::fold::FoldError;

fn bodies(p: &Pair) -> (Body, Body) {
    (Body::new(Operand::A, &p.a), Body::new(Operand::B, &p.b))
}

/// A point by its bits.
type At = [u64; 3];

/// An edge by its ends' positions, the lesser first.
fn edge_at(body: &Body, e: EdgeId) -> [At; 2] {
    let mut ends = body
        .brep
        .ends(e)
        .map(|v| body.brep.pos(v).map(f64::to_bits));
    ends.sort_unstable();
    ends
}

/// An oracle edge (vertex ids) by its ends' positions.
fn ids_at(body: &Body, e: [u32; 2]) -> [At; 2] {
    let mut ends = e.map(|v| body.brep.pos(VertId(v)).map(f64::to_bits));
    ends.sort_unstable();
    ends
}

/// One zone's outcome, named by positions only.
#[derive(Debug, PartialEq)]
struct ZoneOut {
    /// Its snapped faces, each by its corners' positions, sorted.
    faces: Vec<Vec<At>>,
    area: f64,
    /// The relative difference from the oracle's area.
    rel: f64,
    rim_pieces: usize,
    /// Rim pieces along box sides (the level-set part).
    box_pieces: usize,
    rim_verts: usize,
    branches: usize,
    rewrite: [usize; 3],
    /// The snapped edges merged into target edges.
    merged: BTreeSet<([At; 2], [At; 2])>,
    /// The oracle's coincident pairs over the zone's faces.
    coincident: BTreeSet<([At; 2], [At; 2])>,
    /// The cut-point check on the rim records passed.
    p1: bool,
    /// Per target face, the zero-area regions folded and dropped where
    /// regions of winding at least `keep` are kept, or why the fold
    /// refused.
    fates: Vec<Result<[usize; 2], String>>,
}

/// A fixture's outcome.
#[derive(Debug, PartialEq)]
struct Outcome {
    leaves: usize,
    mismatches: Vec<String>,
    clusters: usize,
    merged: usize,
    runs: u32,
    merges: u32,
    settled: Vec<(Tangency, f64, u32, u32)>,
    zones: Vec<ZoneOut>,
}

use super::super::cluster::Tangency;

/// `p` snapped end to end at `σ_A = +1`, its target faces' regions of
/// winding at least `keep` kept.
fn run_fixture(p: &Pair, keep: i64) -> Outcome {
    let (a, b) = bodies(p);
    let s = Setup::new(&a, &b, B_K);
    let ls = leaves(&a, &b, B_K);
    let n = ls.len();
    let exact = ExactPair::new(p).expect("the oracle takes the fixture");
    let (_, bad) = oracle::face_pairs(&s, &exact, &ls, B_K);
    let op = operation(&s, ls, Sign::Pos).unwrap_or_else(|e| panic!("{e:?}"));
    let mut zones = Vec::new();
    for z in op.driver.snaps.values() {
        let t_op = z.target.operand;
        let (sb, tb) = (s.body(t_op.other()), s.body(t_op));
        let (area, rel) = oracle::zone_area(&s, &exact, z, z.b_k);
        let mut faces: Vec<Vec<At>> = z
            .faces
            .iter()
            .map(|f| {
                let mut c: Vec<At> = sb.poly.faces[f.0 as usize]
                    .iter()
                    .map(|&v| sb.poly.verts[v as usize].map(f64::to_bits))
                    .collect();
                c.sort_unstable();
                c
            })
            .collect();
        faces.sort();
        let merged = z
            .geometry
            .edges
            .iter()
            .filter_map(|g| match (g.key, g.class) {
                (SnapEdge::Piece { edge, .. }, EdgeClass::Merged(e)) => {
                    Some((edge_at(sb, edge), edge_at(tb, e)))
                }
                _ => None,
            })
            .collect();
        let coincident = oracle::coincident(&s, &exact, z)
            .into_iter()
            .map(|(x, y)| (ids_at(sb, x), ids_at(tb, y)))
            .collect();
        let keep_rule = |w: i64| w >= keep;
        let fates = match zone::faces(&s, z, &keep_rule) {
            Ok(fs) => fs.iter().map(|f| Ok(f.fates())).collect(),
            Err(ZoneError::Fold(_, e)) => vec![Err(format!("{e:?}"))],
            Err(e) => panic!("{e:?}"),
        };
        zones.push(ZoneOut {
            faces,
            area,
            rel,
            rim_pieces: z.rim.pieces.len(),
            box_pieces: z
                .rim
                .pieces
                .iter()
                .filter(|p| p.along == Along::Box)
                .count(),
            rim_verts: z.corner.rim_verts.len(),
            branches: z.rim.branches.len(),
            rewrite: [
                z.rewrite.faces.len(),
                z.rewrite.edges.len(),
                z.rewrite.verts.len(),
            ],
            merged,
            coincident,
            p1: rim_cut_points(z)
                .expect("the check takes the zone")
                .is_none(),
            fates,
        });
    }
    Outcome {
        leaves: n,
        mismatches: bad.into_iter().map(|m| m.what).collect(),
        clusters: op.clusters,
        merged: op.merged,
        runs: op.snapped.runs,
        merges: op.snapped.merges,
        settled: op
            .driver
            .settled
            .iter()
            .map(|x: &Settled| (x.kind, x.b_k, x.halvings, x.attempts))
            .collect(),
        zones,
    }
}

fn flush() -> Vec<(&'static str, Pair)> {
    let mut out = vec![
        ("L-corner quad", sheets::l_corner_quad(0.0)),
        ("box-edge fold", sheets::box_edge_fold(0.0)),
        ("L-corner", sheets::l_corner(0.0)),
    ];
    if varde_testing::pick(false, true) {
        let w = Wavy {
            cells: 2,
            offset: 0.0,
            ..Wavy::DEFAULT
        };
        out.push(("wavy 2×2", sheets::wavy(&w)));
        let inset = Wavy {
            cells: 4,
            inset: 1,
            diagonals_b: sheets::Diagonals::Alternating,
            ..w
        };
        out.push(("wavy 4×4 inset", sheets::wavy(&inset)));
    }
    out
}

/// The measurements' table row of a zone.
fn row(name: &str, o: &Outcome, k: usize) -> String {
    let z = &o.zones[k];
    let (kind, b_k, halvings, attempts) = o.settled[k];
    format!(
        "| {name} | {k} | {kind:?} | {} | {b_k:e} | {halvings} | {attempts} | {} | {} | {} | {} | {:e} |",
        z.faces.len(),
        z.rim_pieces,
        z.rim_verts,
        z.box_pieces,
        z.branches,
        z.rel
    )
}

#[test]
fn flush_fixtures_snap_end_to_end_as_the_oracle_says() {
    // Every flat fixture flush: the face pairs' kinds, each zone's area
    // and its coincident edges as the exact oracle gives them, the zone's
    // rim records consistent with its ties, and every snap passing at the
    // operation's bound.
    for (name, p) in flush() {
        let o = run_fixture(&p, 1);
        assert!(o.mismatches.is_empty(), "{name}: {:?}", o.mismatches);
        assert!(!o.zones.is_empty(), "{name}");
        for (k, z) in o.zones.iter().enumerate() {
            eprintln!("{}", row(name, &o, k));
            // The stores' UVs on the `2⁻⁵²` grid of a 4·10⁶ mm tile, and the
            // area summed in `f64`.
            assert!(
                z.rel <= 1e-12,
                "{name}: area {} off the oracle's by {}",
                z.area,
                z.rel
            );
            assert_eq!(z.merged, z.coincident, "{name}");
            assert!(z.p1, "{name}");
        }
        for &(_, b_k, halvings, _) in &o.settled {
            assert_eq!((b_k, halvings), (B_K, 0), "{name}");
        }
    }
}

#[test]
fn a_fold_over_a_box_edge_is_one_zone_across_both_faces() {
    // The sheet's two faces lie on the box's top and front, flipped
    // against their surfaces one way and the other: their clusters share
    // the fold edge and merge, the rim is one loop round both faces with
    // no piece along the fold, the fold edge is merged into the box's
    // edge, and on each of the box's faces the sliver between them folds.
    let o = run_fixture(&sheets::box_edge_fold(0.0), 1);
    assert_eq!((o.clusters, o.merged, o.runs), (2, 1, 1));
    let [z] = &o.zones[..] else {
        panic!("{:?}", o.zones)
    };
    assert_eq!(z.faces.len(), 2);
    assert_eq!(z.rim_pieces, 6);
    assert_eq!(z.merged.len(), 1);
    assert_eq!(z.fates, [Ok([1, 0]), Ok([1, 0])]);
}

#[test]
fn zero_area_regions_are_folded_where_kept_and_dropped_where_not() {
    // Each sliver between a target edge and the snapped edge merged into
    // it has the winding of one sheet: kept and folded on a union's rule,
    // dropped on an intersection's (both sheets needed).
    for (name, p) in [
        ("box-edge fold", sheets::box_edge_fold(0.0)),
        ("L-corner", sheets::l_corner(0.0)),
    ] {
        let union = run_fixture(&p, 1);
        let both = run_fixture(&p, 2);
        for (u, i) in union.zones.iter().zip(&both.zones) {
            for (fu, fi) in u.fates.iter().zip(&i.fates) {
                let (fu, fi) = (fu.clone().expect(name), fi.clone().expect(name));
                assert!(fu[0] >= 1 && fu[1] == 0, "{name}: {fu:?}");
                assert_eq!([fi[0], fi[1]], [0, fu[0]], "{name}");
            }
        }
    }
    // The flat L-corner quad: its two slivers along the L's outer edges.
    let o = run_fixture(&sheets::l_corner_quad(0.0), 2);
    assert_eq!(o.zones[0].fates, [Ok([0, 2])]);
}

#[test]
fn a_folded_sliver_on_a_sheet_s_boundary_has_one_face() {
    // On the L-corner quad (two sheets) the slivers lie on both sheets'
    // boundary: folded, the identified edge has the one kept face inside.
    let o = run_fixture(&sheets::l_corner_quad(0.0), 1);
    let want = format!("{:?}", FoldError::Faces { edge: 0, faces: 1 });
    assert_eq!(o.zones[0].fates, [Err(want)]);
}

#[test]
fn relabelled_fixtures_snap_alike() {
    // New vertex and face ids and loops starting elsewhere: the same
    // clusters in the same geometric order and the same zones, named by
    // positions.
    let mut rng = Rng::new(41);
    for (name, p) in flush().into_iter().take(3) {
        let base = run_fixture(&p, 2);
        for _ in 0..varde_testing::pick(1, 3) {
            let r = Pair {
                a: p.a.relabelled(&mut rng),
                b: p.b.relabelled(&mut rng),
            };
            let o = run_fixture(&r, 2);
            assert_eq!(o.zones.len(), base.zones.len(), "{name}");
            for (x, y) in o.zones.iter().zip(&base.zones) {
                assert_eq!(x.faces, y.faces, "{name}");
                assert_eq!(x.area.to_bits(), y.area.to_bits(), "{name}");
                assert_eq!(x.rel.to_bits(), y.rel.to_bits(), "{name}");
                assert_eq!(
                    [x.rim_pieces, x.box_pieces, x.rim_verts, x.branches],
                    [y.rim_pieces, y.box_pieces, y.rim_verts, y.branches],
                    "{name}"
                );
                assert_eq!(x.rewrite, y.rewrite, "{name}");
                assert_eq!(x.merged, y.merged, "{name}");
                assert_eq!(x.coincident, y.coincident, "{name}");
                assert_eq!((x.p1, &x.fates), (y.p1, &y.fates), "{name}");
            }
            assert_eq!(o.settled, base.settled, "{name}");
            assert_eq!(o.mismatches, base.mismatches, "{name}");
            assert_eq!(
                [o.leaves, o.clusters, o.merged],
                [base.leaves, base.clusters, base.merged],
                "{name}"
            );
            assert_eq!(o, base, "{name}");
        }
    }
}

#[test]
fn offset_fixtures_form_no_zone() {
    // Faces `δ` apart certify Empty: no cluster, and the oracle agrees the
    // pairs are apart.
    for p in [
        sheets::box_edge_fold(OFFSET),
        sheets::l_corner(OFFSET),
        sheets::l_corner_quad(OFFSET),
    ] {
        let o = run_fixture(&p, 1);
        assert!(o.mismatches.is_empty(), "{:?}", o.mismatches);
        assert_eq!((o.clusters, o.zones.len()), (0, 0));
    }
}
