//! A square face cut by two crossing curves, labelled one more above and
//! left of each: the counts realised pass, a wrong count fails at the
//! first piece in key order whatever the curves' order, located by what
//! bounds it, and cells outside the zone aren't compared. A rim edge
//! across the square whose sign disagrees with the tie side: two
//! opposite corner records, its branch vertex's one, pass; a wrong branch
//! degree, a missing corner record, records on an agreeing edge and
//! labels its records don't count each fail, only the last naming its
//! rim piece as level-set. Input it doesn't take, each refused; running
//! out of budget.

use super::*;
use crate::boolean::arrange::coincident::{Along, Arc, FaceInput, Line, PieceOf, Place, realise};
use crate::boolean::snap::decide::{BranchRecord, CornerRecord, RimVertRecord};
use crate::geom::curve::PCurve;
use crate::infra::budget::{Budget, Work};
use varde_decide::ids::VertId;
use varde_decide::num::decide::Sign;

/// The straight p-curve from `a` to `b` over `[0, 1]`.
fn seg(a: [f64; 2], b: [f64; 2]) -> PCurve {
    let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    PCurve::new(
        vec![[[a[0], a[1], 1.0], [m[0], m[1], 1.0], [b[0], b[1], 1.0]]],
        vec![0.0, 1.0],
    )
    .unwrap()
}

/// A face bounded by the polygon `boundary` (counter-clockwise, a line
/// per side with its winding step), with the extra vertices `verts`
/// (after the boundary's) and the arcs `arcs`: ends, the lines they run
/// along (empty: transversal) and winding step.
fn face(
    boundary: &[([f64; 2], i32)],
    verts: &[Place],
    arcs: &[([u32; 2], &[u32], i32)],
) -> Coincident {
    let n = boundary.len();
    let mut places: Vec<Place> = boundary.iter().map(|&(c, _)| Place::Uv(c)).collect();
    places.extend_from_slice(verts);
    let curves: Vec<PCurve> = (0..n)
        .map(|k| seg(boundary[k].0, boundary[(k + 1) % n].0))
        .collect();
    let lines: Vec<Line<'_>> = curves
        .iter()
        .enumerate()
        .map(|(k, c)| Line {
            ends: [k as u32, ((k + 1) % n) as u32],
            pcurve: c,
            wind: boundary[k].1,
        })
        .collect();
    let chains: Vec<Vec<Along>> = arcs
        .iter()
        .map(|a| {
            a.1.iter()
                .map(|&line| Along {
                    line,
                    forward: true,
                })
                .collect()
        })
        .collect();
    let arcs: Vec<Arc<'_>> = arcs
        .iter()
        .zip(&chains)
        .map(|(&(ends, _, wind), along)| Arc { ends, along, wind })
        .collect();
    let input = FaceInput {
        verts: &places,
        lines: &lines,
        arcs: &arcs,
    };
    realise(&input, &mut Work::new(&Budget::DEFAULT)).unwrap()
}

/// The index of arc `arc`'s one piece.
fn arc_piece(c: &Coincident, arc: u32) -> u32 {
    c.pieces
        .iter()
        .position(|p| p.of == PieceOf::Arc { arc, part: 0 })
        .unwrap() as u32
}

/// Labels from the regions' windings: one less, none outside the face.
fn labels(c: &Coincident) -> Vec<Option<i64>> {
    c.arrangement
        .regions()
        .iter()
        .map(|r| (r.winding >= 1).then(|| r.winding - 1))
        .collect()
}

fn vert(k: u32) -> SnapVert {
    SnapVert::Vert(Operand::A, VertId(k))
}

const NO_RIM: RimRecords = RimRecords {
    side: Sign::Neg,
    edges: Vec::new(),
    verts: Vec::new(),
    branches: Vec::new(),
};

/// The curve `g`, an edge of the rewrite, rightwards through the middle.
const G: Curve = Curve::Snap(SnapEdge::Piece {
    edge: EdgeId(5),
    part: 0,
});
/// The curve `e`, a target edge, upwards through the middle.
const E: Curve = Curve::Operand(Operand::B, EdgeId(1));

/// The square `[0, 2]²` cut by `e` from `P = (1, 0)` up to `Q = (1,
/// 2)` and `g` from `L = (0, 1)` right to `R = (2, 1)`, crossing at `X =
/// (1, 1)`: arcs `P→X`, `X→Q`, `L→X`, `X→R`, each a step of 1 to its
/// left, the square's windings 1 below-right, 2 below-left and
/// above-right, 3 above-left.
fn cross() -> Coincident {
    face(
        &[
            ([0.0, 0.0], 2),
            ([1.0, 0.0], 1),
            ([2.0, 0.0], 1),
            ([2.0, 1.0], 2),
            ([2.0, 2.0], 2),
            ([1.0, 2.0], 3),
            ([0.0, 2.0], 3),
            ([0.0, 1.0], 2),
        ],
        &[Place::Uv([1.0, 1.0])],
        &[
            ([1, 8], &[], 1),
            ([8, 5], &[], 1),
            ([7, 8], &[], 1),
            ([8, 3], &[], 1),
        ],
    )
}

/// The trim pieces of `cross`'s arcs, one slice each.
fn trims(c: &Coincident) -> [[TrimAt; 1]; 4] {
    [0, 1, 2, 3].map(|a| {
        [TrimAt {
            face: 0,
            piece: arc_piece(c, a),
            forward: true,
        }]
    })
}

/// The cut point of `e` and `g`'s crossing on either.
fn x_on(other: Curve) -> Cut {
    Cut::X {
        with: other,
        cluster: XClusterIx(0),
    }
}

/// Checks `cross` with `e`'s and `g`'s counts, labels `labels`, the
/// curves in the order `e_first` says.
fn run_cross(
    counts: [i64; 4],
    labels: &[Option<i64>],
    c: &Coincident,
    e_first: bool,
) -> Result<Option<CutFailure<u32>>, CutError> {
    let t = trims(c);
    let piece = |from, to, k: usize| CurvePiece {
        from,
        to,
        trim: &t[k],
        count: Count::Records(counts[k]),
    };
    let e = [
        piece(Cut::Vert(vert(10)), x_on(G), 0),
        piece(x_on(G), Cut::Vert(vert(11)), 1),
    ];
    let g = [
        piece(Cut::Vert(vert(12)), x_on(E), 2),
        piece(x_on(E), Cut::Vert(vert(13)), 3),
    ];
    let mut curves = vec![
        ZoneCurve {
            curve: E,
            pieces: &e,
        },
        ZoneCurve {
            curve: G,
            pieces: &g,
        },
    ];
    if !e_first {
        curves.reverse();
    }
    let faces = [CutFace {
        realised: c,
        labels,
    }];
    let input = CutInput {
        rim: &NO_RIM,
        faces: &faces,
        curves: &curves,
        rung: 0,
    };
    check(&7u32, &input, &mut Work::new(&Budget::DEFAULT))
}

#[test]
fn crossing_curves_counted_by_their_cells_pass() {
    let c = cross();
    let l = labels(&c);
    // Below-right 0, below-left and above-right 1, above-left 2.
    let mut sorted: Vec<_> = l.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, [None, Some(0), Some(1), Some(1), Some(2)]);
    for e_first in [true, false] {
        assert_eq!(run_cross([1; 4], &l, &c, e_first), Ok(None));
    }
}

#[test]
fn a_wrong_count_fails_at_the_first_piece_in_key_order() {
    let c = cross();
    let l = labels(&c);
    // `e`'s second piece and `g`'s first are wrong; `g`, an edge of the
    // rewrite, comes first by key whatever the input order.
    for e_first in [true, false] {
        let f = run_cross([1, 0, 0, 1], &l, &c, e_first).unwrap().unwrap();
        assert_eq!(f.cluster, 7);
        assert_eq!(f.piece, PieceAt { curve: G, index: 0 });
        assert_eq!(
            f.fault,
            Fault::Count {
                cells: 1,
                records: 0
            }
        );
        assert_eq!(
            f.locator,
            Locator {
                curve: G,
                at: [Mark::Vert(vert(12)), Mark::Cross(E)],
            }
        );
        let s = f.signal();
        assert_eq!(s.check, Check::CutPoints);
        assert!(s.level_set.is_empty());
        assert_eq!(s.locator, f.locator);
    }
    // Only `e`'s second piece wrong: it is named, located between the
    // crossing and its end.
    let f = run_cross([1, -1, 1, 1], &l, &c, true).unwrap().unwrap();
    assert_eq!(f.piece, PieceAt { curve: E, index: 1 });
    assert_eq!(f.locator.at, [Mark::Cross(G), Mark::Vert(vert(11))]);
}

#[test]
fn cells_outside_the_zone_are_not_compared() {
    let c = cross();
    let mut l = labels(&c);
    // The cell above-left is no cell of the zone: the pieces beside it
    // (`e`'s second, `g`'s first) aren't checked.
    let tl = l.iter().position(|x| *x == Some(2)).unwrap();
    l[tl] = None;
    assert_eq!(run_cross([1, 5, -3, 1], &l, &c, true), Ok(None));
    assert!(run_cross([5, 1, 1, 1], &l, &c, true).unwrap().is_some());
}

// ---------------------------------------------------------------------
// The rim.

const RIM_FACE: FaceId = FaceId(3);
const TARGET: FaceId = FaceId(9);
/// The rim edge: a rim piece along box sides, at position 2 in the rim.
const RIM: SnapEdge = SnapEdge::Rim {
    face: RIM_FACE,
    piece: 2,
    part: 0,
};
/// Its start, a branch vertex, and its end, a junction on a target edge.
const BRANCH: SnapVert = SnapVert::Branch {
    face: RIM_FACE,
    piece: 2,
    index: 0,
};
const JUNCTION: SnapVert = SnapVert::RimTarget {
    edge: EdgeId(4),
    piece: 1,
};

/// The square `[0, 2]²` cut by one arc from `(1, 0)` up to `(1, 2)`,
/// windings 2 left of it and 1 right.
fn halves() -> Coincident {
    face(
        &[
            ([0.0, 0.0], 2),
            ([1.0, 0.0], 1),
            ([2.0, 0.0], 1),
            ([2.0, 2.0], 1),
            ([1.0, 2.0], 2),
            ([0.0, 2.0], 2),
        ],
        &[],
        &[([1, 4], &[], 1)],
    )
}

/// The rim records of a rim edge of gap sign `sign` against the tie side
/// `Neg`, with `corners`, its ends both ties, its start a branch vertex
/// of `degree` naming `named`.
fn rim(sign: Sign, corners: Vec<CornerRecord>, degree: i8, named: Option<SnapEdge>) -> RimRecords {
    let tie = |vert| RimVertRecord {
        vert,
        targets: vec![TARGET],
        sign: Sign::Neg,
        tie: true,
    };
    RimRecords {
        side: Sign::Neg,
        edges: vec![RimRecord {
            edge: RIM,
            targets: vec![TARGET],
            sign,
            corners,
        }],
        verts: vec![tie(BRANCH), tie(JUNCTION)],
        branches: vec![BranchRecord {
            vert: BRANCH,
            targets: vec![TARGET],
            degree,
            edge: named,
        }],
    }
}

/// The records of a disagreeing rim edge between two ties.
fn opposite() -> Vec<CornerRecord> {
    vec![
        CornerRecord {
            vert: BRANCH,
            end: RimEnd::Start,
            degree: 1,
        },
        CornerRecord {
            vert: JUNCTION,
            end: RimEnd::End,
            degree: -1,
        },
    ]
}

/// Checks `halves` with the rim edge along its arc, cells labelled
/// `left`, `right`.
fn run_rim(rim: &RimRecords, left: i64, right: i64) -> Result<Option<CutFailure<u32>>, CutError> {
    let c = halves();
    // Left of the arc winding 2, right of it 1.
    let l: Vec<Option<i64>> = c
        .arrangement
        .regions()
        .iter()
        .map(|r| match r.winding {
            2 => Some(left),
            1 => Some(right),
            _ => None,
        })
        .collect();
    let t = [TrimAt {
        face: 0,
        piece: arc_piece(&c, 0),
        forward: true,
    }];
    let pieces = [CurvePiece {
        from: Cut::Vert(BRANCH),
        to: Cut::Vert(JUNCTION),
        trim: &t,
        count: Count::Rim { target: TARGET },
    }];
    let curves = [ZoneCurve {
        curve: Curve::Snap(RIM),
        pieces: &pieces,
    }];
    let faces = [CutFace {
        realised: &c,
        labels: &l,
    }];
    let input = CutInput {
        rim,
        faces: &faces,
        curves: &curves,
        rung: 2,
    };
    check(&7u32, &input, &mut Work::new(&Budget::DEFAULT))
}

#[test]
fn a_disagreeing_rim_edge_between_two_ties_counts_zero() {
    let r = rim(Sign::Pos, opposite(), 1, Some(RIM));
    assert_eq!(r.x21(RIM, TARGET), 0);
    assert_eq!(run_rim(&r, 4, 4), Ok(None));
}

#[test]
fn a_branch_degree_its_record_doesnt_carry_fails() {
    let at = PieceAt {
        curve: Curve::Snap(RIM),
        index: 0,
    };
    for (degree, named) in [(-1, Some(RIM)), (1, None)] {
        let f = run_rim(&rim(Sign::Pos, opposite(), degree, named), 0, 0)
            .unwrap()
            .unwrap();
        assert_eq!(f.piece, at);
        assert_eq!(
            f.fault,
            Fault::Branch {
                vert: BRANCH,
                degree
            }
        );
        assert_eq!(f.locator.at, [Mark::Vert(BRANCH); 2]);
        // A branch vertex is pinned.
        assert!(f.signal().level_set.is_empty());
    }
}

#[test]
fn corner_records_follow_the_ties() {
    // The junction's record missing: the edge counts `+1`, as its cells
    // do, but the junction is a tie of a disagreeing edge.
    let mut c = opposite();
    c.pop();
    let f = run_rim(&rim(Sign::Pos, c, 1, Some(RIM)), 1, 0)
        .unwrap()
        .unwrap();
    assert_eq!(f.fault, Fault::Corner { vert: JUNCTION });
    assert!(f.level_set.is_empty());
    // Records on an edge agreeing with the tie side.
    let f = run_rim(&rim(Sign::Neg, opposite(), 1, Some(RIM)), 0, 0)
        .unwrap()
        .unwrap();
    assert!(matches!(f.fault, Fault::Corner { .. }), "{f:?}");
    // A record of the wrong degree at the junction.
    let mut c = opposite();
    c[1].degree = 1;
    let f = run_rim(&rim(Sign::Pos, c, 1, Some(RIM)), 2, 0)
        .unwrap()
        .unwrap();
    assert_eq!(f.fault, Fault::Corner { vert: JUNCTION });
}

#[test]
fn cells_a_rim_edge_doesnt_count_name_its_level_set_piece() {
    let f = run_rim(&rim(Sign::Pos, opposite(), 1, Some(RIM)), 1, 0)
        .unwrap()
        .unwrap();
    assert_eq!(
        f.fault,
        Fault::Count {
            cells: 1,
            records: 0
        }
    );
    assert_eq!(f.locator.at, [Mark::Vert(BRANCH), Mark::Vert(JUNCTION)]);
    assert_eq!(f.signal().level_set, [2]);
}

// ---------------------------------------------------------------------
// Input it doesn't take.

#[test]
fn input_it_doesnt_take_is_refused() {
    let c = halves();
    let l = labels(&c);
    let faces = [CutFace {
        realised: &c,
        labels: &l,
    }];
    let t = [TrimAt {
        face: 0,
        piece: arc_piece(&c, 0),
        forward: true,
    }];
    let p = |from, to, trim| CurvePiece {
        from,
        to,
        trim,
        count: Count::Records(1),
    };
    let go = |rim: &RimRecords, curves: &[ZoneCurve<'_>], faces: &[CutFace<'_>]| {
        let input = CutInput {
            rim,
            faces,
            curves,
            rung: 0,
        };
        check(&0u32, &input, &mut Work::new(&Budget::DEFAULT))
    };
    let ok = [p(Cut::Vert(vert(1)), Cut::Vert(vert(2)), &t[..])];
    let one = [ZoneCurve {
        curve: E,
        pieces: &ok,
    }];
    assert_eq!(go(&NO_RIM, &one, &faces), Ok(None));
    // Twice the same curve.
    let empty: [CurvePiece<'_>; 0] = [];
    let twice = [
        one[0],
        ZoneCurve {
            curve: E,
            pieces: &empty,
        },
    ];
    assert_eq!(go(&NO_RIM, &twice, &faces), Err(CutError::Curve(E)));
    // No pieces; pieces not joined; an end that isn't a vertex.
    let none = [ZoneCurve {
        curve: E,
        pieces: &empty,
    }];
    assert_eq!(go(&NO_RIM, &none, &faces), Err(CutError::Curve(E)));
    let split = [
        p(Cut::Vert(vert(1)), x_on(G), &t[..]),
        p(Cut::Vert(vert(3)), Cut::Vert(vert(2)), &[]),
    ];
    let open = [p(Cut::Vert(vert(1)), x_on(G), &t[..])];
    for pieces in [&split[..], &open[..]] {
        let cs = [ZoneCurve { curve: E, pieces }];
        assert_eq!(go(&NO_RIM, &cs, &faces), Err(CutError::Curve(E)));
    }
    // A trim piece two curves name; a face not there.
    let both = [
        one[0],
        ZoneCurve {
            curve: G,
            pieces: &ok,
        },
    ];
    let at = |curve| PieceAt { curve, index: 0 };
    assert_eq!(go(&NO_RIM, &both, &faces), Err(CutError::Piece(at(E))));
    assert_eq!(go(&NO_RIM, &one, &[]), Err(CutError::Piece(at(E))));
    // A rim count on an edge that isn't a rim edge.
    let rim_count = [CurvePiece {
        count: Count::Rim { target: TARGET },
        ..ok[0]
    }];
    let cs = [ZoneCurve {
        curve: E,
        pieces: &rim_count,
    }];
    assert_eq!(go(&NO_RIM, &cs, &faces), Err(CutError::Piece(at(E))));
    // Labels not one per region.
    let short = [CutFace {
        realised: &c,
        labels: &l[1..],
    }];
    assert_eq!(go(&NO_RIM, &one, &short), Err(CutError::Face(0)));
    // A rim edge with records that isn't a zone curve.
    let r = rim(Sign::Pos, opposite(), 1, Some(RIM));
    assert_eq!(go(&r, &one, &faces), Err(CutError::Rim(RIM)));
}

#[test]
fn a_bundle_member_no_curve_names_is_refused() {
    // An arc along the square's bottom side: one bundle with the side's
    // piece, which no zone curve names.
    let c = face(
        &[
            ([0.0, 0.0], 1),
            ([2.0, 0.0], 1),
            ([2.0, 2.0], 1),
            ([0.0, 2.0], 1),
        ],
        &[
            Place::On { line: 0, s: 0.25 },
            Place::On { line: 0, s: 0.75 },
        ],
        &[([4, 5], &[0], 0)],
    );
    let l = labels(&c);
    let faces = [CutFace {
        realised: &c,
        labels: &l,
    }];
    let t = [TrimAt {
        face: 0,
        piece: arc_piece(&c, 0),
        forward: true,
    }];
    let ps = [CurvePiece {
        from: Cut::Vert(vert(1)),
        to: Cut::Vert(vert(2)),
        trim: &t,
        count: Count::Records(1),
    }];
    let cs = [ZoneCurve {
        curve: E,
        pieces: &ps,
    }];
    let input = CutInput {
        rim: &NO_RIM,
        faces: &faces,
        curves: &cs,
        rung: 0,
    };
    let b = c.pieces[t[0].piece as usize].bundle;
    assert_eq!(
        check(&0u32, &input, &mut Work::new(&Budget::DEFAULT)),
        Err(CutError::Bundle { face: 0, bundle: b })
    );
}

#[test]
fn small_budget_runs_out() {
    let c = cross();
    let l = labels(&c);
    let t = trims(&c);
    let ps = [CurvePiece {
        from: Cut::Vert(vert(1)),
        to: Cut::Vert(vert(2)),
        trim: &t[0],
        count: Count::Records(1),
    }];
    let cs = [ZoneCurve {
        curve: E,
        pieces: &ps,
    }];
    let faces = [CutFace {
        realised: &c,
        labels: &l,
    }];
    let input = CutInput {
        rim: &NO_RIM,
        faces: &faces,
        curves: &cs,
        rung: 0,
    };
    let got = check(&0u32, &input, &mut Work::new(&Budget::new(3)));
    assert!(matches!(got, Err(CutError::OutOfBudget(_))), "{got:?}");
}
