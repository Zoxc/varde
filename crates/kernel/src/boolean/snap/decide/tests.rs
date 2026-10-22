use std::cmp::Ordering;

use malachite::Rational;
use varde_decide::bezier::Dyadic;
use varde_decide::ids::{EdgeId, FaceId, VertId};
use varde_decide::num::decide::Sign;
use varde_decide::records::keys::Operand;

use super::super::cluster::Tangency;
use super::super::cluster::tests::{quad, work};
use super::super::corner::{AnchorSpan, Corner, Placement};
use super::super::rewrite::tests::{bowl_at_floor_edge, snapped_bowl_with};
use super::super::rewrite::{EdgeClass, Geometry, Rewrite, SnapEdge, SnapVert, TargetFace};
use super::super::rim::{Element, Implicit, Rim};
use super::super::stale::{OnT, OnTInput, on_t};
use super::*;

const A: Operand = Operand::A;
const B: Operand = Operand::B;

const FLOOR: Implicit = Implicit::Plane {
    normal: [0.0, 0.0, 1.0],
    offset: 0.0,
};

fn q(x: f64) -> Q {
    Rational::try_from(x).unwrap()
}

// ---------------------------------------------------------------------
// The coincidence.

#[test]
fn the_zone_s_labels_follow_the_coincidence_table() {
    use Label::{Inside, Outside};
    let l = |a, b| ZoneLabels { a, b };
    assert_eq!(
        zone_labels(Tangency::Contact, Sign::Neg),
        l(Outside, Outside)
    );
    assert_eq!(zone_labels(Tangency::Contact, Sign::Pos), l(Inside, Inside));
    assert_eq!(zone_labels(Tangency::Near, Sign::Neg), l(Inside, Outside));
    assert_eq!(zone_labels(Tangency::Near, Sign::Pos), l(Outside, Inside));
    for s in [Sign::Neg, Sign::Pos] {
        // Opposed sheets one label, co-oriented ones complementary.
        let c = zone_labels(Tangency::Contact, s);
        assert_eq!(c.a, c.b);
        let n = zone_labels(Tangency::Near, s);
        assert_ne!(n.a, n.b);
        // The rim's ties go to the snapped side's label.
        for kind in [Tangency::Near, Tangency::Contact] {
            let z = zone_labels(kind, s);
            for (op, lab) in [(A, z.a), (B, z.b)] {
                let side = tie_side(kind, s, op);
                assert_eq!(side == Sign::Pos, lab == Outside, "{kind:?} {s:?} {op:?}");
            }
        }
    }
}

// ---------------------------------------------------------------------
// Walking a loop.

fn part(own: Option<Sign>) -> WalkPart {
    WalkPart {
        rim: true,
        own,
        fallback: None,
    }
}

const OFF: WalkPart = WalkPart {
    rim: false,
    own: None,
    fallback: None,
};

fn plain() -> WalkVert {
    WalkVert {
        tie: false,
        degree: None,
    }
}

fn tie() -> WalkVert {
    WalkVert {
        tie: true,
        degree: None,
    }
}

fn branch(d: i8) -> WalkVert {
    WalkVert {
        tie: true,
        degree: Some(d),
    }
}

/// Each part's crossing count: the sum of its corner records.
fn x21s(w: &Walk, n: usize) -> Vec<i32> {
    (0..n)
        .map(|i| {
            w.records
                .iter()
                .filter(|r| r.0 == i)
                .map(|r| i32::from(r.2))
                .sum()
        })
        .collect()
}

#[test]
fn signs_carry_along_the_rim_and_change_only_at_branch_vertices() {
    use Sign::{Neg, Pos};
    // Outside, a falling branch vertex, two stretches inside (the first
    // all in a root cluster), a rising one, a stretch in a cluster.
    let parts = [part(Some(Pos)), part(None), part(Some(Neg)), part(None)];
    let verts = [plain(), branch(-1), plain(), branch(1)];
    for side in [Pos, Neg] {
        let w = walk(&parts, &verts, side).unwrap();
        assert_eq!(w.signs, [Some(Pos), Some(Neg), Some(Neg), Some(Pos)]);
        // Each branch vertex gives one record, of its degree, to the
        // stretch beside it that disagrees with the side.
        let (want, x21) = if side == Pos {
            (
                vec![(1, RimEnd::Start, -1), (2, RimEnd::End, 1)],
                [0, -1, 1, 0],
            )
        } else {
            (
                vec![(0, RimEnd::End, -1), (3, RimEnd::Start, 1)],
                [-1, 0, 0, 1],
            )
        };
        assert_eq!(w.records, want);
        assert_eq!(x21s(&w, 4), x21);
        // Round the loop the records balance.
        assert_eq!(x21.iter().sum::<i32>(), 0);
    }
}

#[test]
fn a_stretch_between_two_ties_that_disagrees_gets_two_opposite_records() {
    use Sign::{Neg, Pos};
    // A junction on a target edge, a stretch inside, a branch vertex, a
    // stretch outside, then a stretch along a target edge (not a rim
    // element) between two junctions.
    let parts = [part(Some(Neg)), part(Some(Pos)), OFF];
    let verts = [tie(), branch(1), tie()];
    let w = walk(&parts, &verts, Pos).unwrap();
    assert_eq!(w.signs, [Some(Neg), Some(Pos), None]);
    assert_eq!(w.records, [(0, RimEnd::Start, -1), (0, RimEnd::End, 1)]);
    assert_eq!(x21s(&w, 3), [0, 0, 0]);
    let w = walk(&parts, &verts, Neg).unwrap();
    assert_eq!(w.records, [(1, RimEnd::Start, 1), (1, RimEnd::End, -1)]);
    assert_eq!(x21s(&w, 3), [0, 0, 0]);
}

#[test]
fn a_stretch_with_no_sign_of_its_own_reads_its_neighbours_or_the_proxy() {
    use Sign::{Neg, Pos};
    // Not carried across a stretch that isn't a rim element.
    let parts = [part(Some(Neg)), part(None), OFF, part(None)];
    let verts = [plain(), plain(), tie(), tie()];
    let w = walk(&parts, &verts, Pos).unwrap();
    assert_eq!(w.signs, [Some(Neg), Some(Neg), None, Some(Neg)]);
    // Back from a branch vertex at its end.
    let parts = [part(None), part(Some(Pos)), OFF];
    let verts = [tie(), branch(1), tie()];
    let w = walk(&parts, &verts, Pos).unwrap();
    assert_eq!(w.signs[0], Some(Neg));
    // Nothing to read: the proxy, else the side.
    let mut alone = part(None);
    let verts = [tie(), tie()];
    assert_eq!(
        walk(&[alone.clone(), OFF], &verts, Pos).unwrap().signs[0],
        Some(Pos)
    );
    alone.fallback = Some(Neg);
    assert_eq!(
        walk(&[alone, OFF], &verts, Pos).unwrap().signs[0],
        Some(Neg)
    );
}

#[test]
fn signs_that_change_where_no_tie_allows_it_are_refused() {
    use Sign::{Neg, Pos};
    // A change at a vertex that isn't a tie.
    let parts = [part(Some(Pos)), part(Some(Neg)), OFF];
    assert_eq!(walk(&parts, &[tie(), plain(), tie()], Pos), Err(1));
    // At a junction it is allowed.
    assert!(walk(&parts, &[tie(), tie(), tie()], Pos).is_ok());
    // A branch vertex whose sides disagree with its degree.
    assert_eq!(walk(&parts, &[tie(), branch(1), tie()], Neg), Err(1));
    // A loop of one stretch can't hold a branch vertex.
    assert_eq!(walk(&[part(Some(Pos))], &[branch(1)], Pos), Err(0));
    // One that holds no tie needs no record.
    let w = walk(&[part(Some(Neg))], &[plain()], Pos).unwrap();
    assert!(w.records.is_empty());
}

// ---------------------------------------------------------------------
// The bowl.

/// The sign of the bowl's gap against the floor at its face's `uv`:
/// `(10u − 5)² + (10v − 5)² − 9`, exactly.
fn bowl_gap(uv: &[Q; 2]) -> Ordering {
    let d = |x: &Q| Q::from(10u32) * x - Q::from(5u32);
    let r2 = d(&uv[0]) * d(&uv[0]) + d(&uv[1]) * d(&uv[1]);
    r2.cmp(&Q::from(9u32))
}

fn uv_of(r: &Rewrite, v: SnapVert) -> [Q; 2] {
    let nv = r.verts.iter().find(|x| x.key == v).unwrap();
    nv.at.iter().find(|x| x.0 == FaceId(0)).unwrap().1.clone()
}

fn wide_e(_: SnapEdge) -> f64 {
    1e-3
}

fn wide_v(_: SnapVert) -> f64 {
    1e-3
}

fn floor() -> varde_decide::bezier::HomPatch<Rational> {
    quad([-5.0, -5.0, 0.0], [15.0, -5.0, 0.0], [-5.0, 15.0, 0.0])
}

/// The zone of a snapped fixture onto the floor (face 0 of `A`).
fn zone(
    rim: &Rim,
    corner: &Corner,
    r: &Rewrite,
    geo: &Geometry,
    kind: Tangency,
    sigma_a: Sign,
) -> Result<Zone, DecideError> {
    let floor = floor();
    let faces = [TargetFace {
        face: FaceId(0),
        tile: [0, 0],
        patch: &floor,
    }];
    let on = on_t(
        r,
        geo,
        &OnTInput {
            snapped: B,
            faces: &faces,
            edge_band: &wide_e,
            vert_band: &wide_v,
        },
        &mut work(),
    )
    .expect("on-T");
    let input = DecideInput {
        kind,
        sigma_a,
        snapped: B,
        targets: &[(FaceId(0), FLOOR)],
        faces: &faces,
    };
    decide_zone(rim, corner, r, geo, &on, &input, &mut work())
}

fn on_of(z: &Zone) -> &OnT {
    &z.on
}

/// Each rim record's crossing count worked out from the bowl's geometry
/// alone: the gap's sign at the stretch's middle, and at each end the side
/// where the end lies on the floor (a branch vertex) or on its edge at
/// `u = edge_u`, else that sign.
fn x21_from_geometry(r: &Rewrite, rec: &RimRecord, side: Sign, edge_u: Option<f64>) -> i32 {
    let e = r.edges.iter().find(|e| e.key == rec.edge).unwrap();
    let [a, b] = e.ends.map(|v| uv_of(r, v));
    let mid = [
        (&a[0] + &b[0]) / Q::from(2u32),
        (&a[1] + &b[1]) / Q::from(2u32),
    ];
    let s = Sign::of(bowl_gap(&mid)).unwrap();
    let state = |p: &[Q; 2]| {
        let on_edge = edge_u.is_some_and(|u| p[0] == q(u));
        if on_edge || bowl_gap(p) == Ordering::Equal || is_branch(r, e.ends, p) {
            side
        } else {
            s
        }
    };
    let (s0, s1) = (state(&a), state(&b));
    let change = |from: Sign, to: Sign| if from == to { 0 } else { i32::from(to.as_i8()) };
    // Along the rim: the edge's own direction or against it.
    let along = rec.corners.first().is_none_or(|c| match c.end {
        RimEnd::Start => c.vert == e.ends[0],
        RimEnd::End => c.vert == e.ends[1],
    });
    if along {
        change(s0, s) + change(s, s1)
    } else {
        change(s1, s) + change(s, s0)
    }
}

/// Whether `p`, an end of edge `ends`, is a branch vertex's point: within
/// `10⁻⁹` of the floor (the gap's height is `2⁻¹⁵` times its sign's
/// quantity).
fn is_branch(r: &Rewrite, ends: [SnapVert; 2], p: &[Q; 2]) -> bool {
    ends.iter().any(|v| {
        uv_of(r, *v) == *p && {
            let d = |x: &Q| Q::from(10u32) * x - Q::from(5u32);
            let h = (d(&p[0]) * d(&p[0]) + d(&p[1]) * d(&p[1]) - Q::from(9u32)) / Q::from(32768u32);
            h < q(1e-9) && h > q(-1e-9)
        }
    })
}

#[test]
fn the_bowl_s_rim_records_read_the_gap_sign_and_each_branch_vertex_gives_one_record() {
    // The bowl's rim, the square `[2.5, 7.5]²` about its bottom, crosses
    // the floor twice on each side: its corners outside the floor, the
    // middles of its sides inside. The twelve rim edges read that sign;
    // the eight branch vertices are the only ties.
    let (rm, cr, r, geo, _) = snapped_bowl_with();
    let cases = [
        (Tangency::Near, Sign::Neg, Sign::Pos),
        (Tangency::Near, Sign::Pos, Sign::Neg),
        (Tangency::Contact, Sign::Neg, Sign::Pos),
        (Tangency::Contact, Sign::Pos, Sign::Neg),
    ];
    for (kind, sigma, side) in cases {
        let z = zone(&rm, &cr, &r, &geo, kind, sigma).expect("a zone");
        assert_eq!(z.labels, zone_labels(kind, sigma));
        let recs = &z.rim;
        assert_eq!(recs.side, side);
        assert_eq!(recs.edges.len(), 12);
        let mut by_sign = [0; 2];
        for rec in &recs.edges {
            let e = r.edges.iter().find(|e| e.key == rec.edge).unwrap();
            assert_eq!(
                geo.edges.iter().find(|g| g.key == e.key).unwrap().class,
                EdgeClass::Rim
            );
            assert_eq!(rec.targets, [FaceId(0)]);
            // The sign at the stretch's middle, exactly.
            let [a, b] = e.ends.map(|v| uv_of(&r, v));
            let mid = [
                (&a[0] + &b[0]) / Q::from(2u32),
                (&a[1] + &b[1]) / Q::from(2u32),
            ];
            assert_eq!(Some(rec.sign), Sign::of(bowl_gap(&mid)), "{:?}", rec.edge);
            by_sign[usize::from(rec.sign == Sign::Pos)] += 1;
            // Its crossing count is the sum of its corner records, and
            // what the geometry gives.
            let x21 = recs.x21(rec.edge, FaceId(0));
            assert_eq!(
                x21,
                rec.corners.iter().map(|c| i32::from(c.degree)).sum::<i32>()
            );
            assert_eq!(
                x21,
                x21_from_geometry(&r, rec, side, None),
                "{:?}",
                rec.edge
            );
            // Agreeing stretches have none; the middles, between two
            // branch vertices, two opposite ones or none; the others one.
            if rec.sign == side {
                assert!(rec.corners.is_empty());
            } else if rec.sign == Sign::Neg {
                assert_eq!(rec.corners.len(), 2);
                assert_eq!(x21, 0);
            } else {
                assert_eq!(rec.corners.len(), 1);
            }
        }
        assert_eq!(by_sign, [4, 8]);
        // The records balance round the loop.
        let total: i32 = recs.edges.iter().map(|e| recs.x21(e.edge, FaceId(0))).sum();
        assert_eq!(total, 0);
        // Each branch vertex gives one record, of its degree.
        assert_eq!(recs.branches.len(), 8);
        for b in &recs.branches {
            assert!(matches!(b.vert, SnapVert::Branch { .. }), "{b:?}");
            let holder = b.edge.expect("a record");
            let rec = recs.edge(holder, FaceId(0)).unwrap();
            let c: Vec<&CornerRecord> = rec.corners.iter().filter(|c| c.vert == b.vert).collect();
            assert_eq!(c.len(), 1);
            assert_eq!(c[0].degree, b.degree);
            assert_ne!(rec.sign, side);
        }
        // The rim's corners aren't ties and read the outside; the branch
        // vertices read the side.
        let mut ties = 0;
        for v in &recs.verts {
            match v.vert {
                SnapVert::RimBox { .. } => {
                    assert!(!v.tie);
                    assert_eq!(v.sign, Sign::Pos);
                }
                SnapVert::Branch { .. } => {
                    assert!(v.tie);
                    assert_eq!(v.sign, side);
                    ties += 1;
                }
                k => panic!("{k:?}"),
            }
        }
        assert_eq!((ties, recs.verts.len()), (8, 12));
    }
}

#[test]
fn an_anchor_carrying_a_branch_takes_the_branch_vertex_s_record() {
    // One of the bowl's branch vertices made an anchor's: the corner rule
    // keeps no branch vertex there, and an anchor's root cluster over it,
    // of the branch's degree, carries it, its cells hidden. The records
    // are the same, the anchor's rim vertex holding the branch.
    let (rm, cr, r, geo, _) = snapped_bowl_with();
    let base = zone(&rm, &cr, &r, &geo, Tangency::Near, Sign::Neg).unwrap();
    let mut cr2 = cr.clone();
    let b = cr2.branches.remove(0);
    let at = b.at.clone();
    let lo = (0..1024u64)
        .rev()
        .find(|k| Q::from(*k) / Q::from(1024u32) < at)
        .unwrap();
    cr2.spans.push(AnchorSpan {
        anchor: (A, VertId(99)),
        targets: b.targets.clone(),
        from: (b.piece, Dyadic::new(lo, 10).unwrap()),
        to: (b.piece, Dyadic::new(lo + 1, 10).unwrap()),
        net: Some(if b.rising { 1 } else { -1 }),
    });
    let first = SnapVert::Branch {
        face: FaceId(0),
        piece: b.piece,
        index: 0,
    };
    let mut r2 = r.clone();
    r2.verts
        .iter_mut()
        .find(|v| v.key == first)
        .unwrap()
        .moved
        .to = Placement::Vert(A, VertId(99));
    let z = zone(&rm, &cr2, &r2, &geo, Tangency::Near, Sign::Neg).unwrap();
    assert_eq!(z.rim, base.rim);
    // Without a rim vertex joining the anchor, it can't carry the branch.
    let z = zone(&rm, &cr2, &r, &geo, Tangency::Near, Sign::Neg);
    assert_eq!(z, Err(DecideError::Anchor(A, VertId(99))));
}

#[test]
fn a_branch_vertex_the_rim_doesn_t_hold_or_signs_that_contradict_the_rim_are_refused() {
    let (rm, cr, r, geo, _) = snapped_bowl_with();
    // A branch vertex moved off the rim's vertices.
    let mut cr2 = cr.clone();
    cr2.branches[0].at = q(0.5);
    assert_eq!(
        zone(&rm, &cr2, &r, &geo, Tangency::Near, Sign::Neg),
        Err(DecideError::Branch(0))
    );
    // A stretch beside the rim's first corner made inside the floor: the
    // gap changes sign at the corner, which isn't a tie.
    let mut rm2 = rm.clone();
    let first = cr
        .branches
        .iter()
        .find(|b| b.piece == 0)
        .unwrap()
        .at
        .clone();
    for g in rm2.gaps.iter_mut().filter(|g| g.piece == 0) {
        for (span, gap) in &mut g.cells {
            if span[1].rational() <= first {
                *gap = Gap::Negative;
            }
        }
    }
    assert!(matches!(
        zone(&rm2, &cr, &r, &geo, Tangency::Near, Sign::Neg),
        Err(DecideError::Vertex(_))
    ));
    // One cell of a stretch flipped: its cells disagree.
    let mut rm2 = rm.clone();
    let g = rm2.gaps.iter_mut().find(|g| g.piece == 0).unwrap();
    let k = g.cells.iter().position(|c| c.1 == Gap::Positive).unwrap();
    g.cells[k].1 = Gap::Negative;
    assert!(matches!(
        zone(&rm2, &cr, &r, &geo, Tangency::Near, Sign::Neg),
        Err(DecideError::Inconsistent(_))
    ));
    // A target face without a surface.
    let floor = floor();
    let faces = [TargetFace {
        face: FaceId(0),
        tile: [0, 0],
        patch: &floor,
    }];
    let input = DecideInput {
        kind: Tangency::Near,
        sigma_a: Sign::Neg,
        snapped: B,
        targets: &[(FaceId(5), FLOOR)],
        faces: &faces,
    };
    let z = decide_zone(&rm, &cr, &r, &geo, &OnT::default(), &input, &mut work());
    assert_eq!(z, Err(DecideError::Surface(FaceId(0))));
}

#[test]
fn the_rim_s_junctions_on_a_target_edge_are_ties_with_a_record_on_each_disagreeing_stretch() {
    // The floor ends at `x = 6.25` across the bowl's cluster: the rim runs
    // along its edge there (on the target edge, no gap record), its two
    // junctions with the box sides ties. The box stretches beside them are
    // inside the floor, each between a branch vertex and a junction: with
    // the side outside, two opposite records; inside, none.
    let (rm, cr, r, geo, _, _) = bowl_at_floor_edge();
    for (sigma, side) in [(Sign::Neg, Sign::Pos), (Sign::Pos, Sign::Neg)] {
        let z = zone(&rm, &cr, &r, &geo, Tangency::Near, sigma).expect("a zone");
        let recs = &z.rim;
        let junctions: Vec<&RimVertRecord> = recs
            .verts
            .iter()
            .filter(|v| matches!(v.vert, SnapVert::RimTarget { .. }))
            .collect();
        assert_eq!(junctions.len(), 2);
        assert!(junctions.iter().all(|v| v.tie && v.sign == side));
        // Nothing along the target edge.
        assert!(
            recs.edges
                .iter()
                .all(|e| !matches!(e.edge, SnapEdge::Cut { .. }))
        );
        let at_junction: Vec<&RimRecord> = recs
            .edges
            .iter()
            .filter(|e| {
                let ends = r.edges.iter().find(|x| x.key == e.edge).unwrap().ends;
                ends.iter().any(|v| matches!(v, SnapVert::RimTarget { .. }))
            })
            .collect();
        assert_eq!(at_junction.len(), 2);
        for rec in at_junction {
            assert_eq!(rec.sign, Sign::Neg);
            let x21 = recs.x21(rec.edge, FaceId(0));
            assert_eq!(x21, 0);
            assert_eq!(x21, x21_from_geometry(&r, rec, side, Some(0.625)));
            let n = if side == Sign::Pos { 2 } else { 0 };
            assert_eq!(rec.corners.len(), n, "{rec:?}");
            if n == 2 {
                assert!(
                    rec.corners
                        .iter()
                        .any(|c| matches!(c.vert, SnapVert::RimTarget { .. }))
                );
            }
        }
        for rec in &recs.edges {
            assert_eq!(
                recs.x21(rec.edge, FaceId(0)),
                x21_from_geometry(&r, rec, side, Some(0.625))
            );
        }
        let total: i32 = recs.edges.iter().map(|e| recs.x21(e.edge, FaceId(0))).sum();
        assert_eq!(total, 0);
        for b in &recs.branches {
            let rec = recs.edge(b.edge.expect("a record"), FaceId(0)).unwrap();
            assert!(
                rec.corners
                    .iter()
                    .any(|c| c.vert == b.vert && c.degree == b.degree)
            );
        }
    }
}

// ---------------------------------------------------------------------
// Lifted reads and the band.

#[test]
fn elements_on_the_target_read_their_places_there() {
    // The bowl's rim edges lie at the mid-surface; read lifted, on the
    // floor itself, exactly, with their ends at their vertices' corners.
    let (rm, cr, r, geo, _) = snapped_bowl_with();
    let z = zone(&rm, &cr, &r, &geo, Tangency::Near, Sign::Neg).unwrap();
    let on = on_of(&z);
    assert!(!on.edges.is_empty());
    assert_eq!(z.lifted.edges.len(), on.edges.len());
    for (e, c) in &z.lifted.edges {
        assert!(on.edge(*e));
        assert_eq!(c.face, FaceId(0));
        for s in &c.spans {
            for (p, w) in s {
                assert_eq!(p[2], 0u32, "{e:?}");
                assert!(*w > 0u32);
            }
        }
        let ends = r.edges.iter().find(|x| x.key == *e).unwrap().ends;
        assert_eq!(z.lifted.vert(ends[0], FaceId(0)), Some(c.first()));
        assert_eq!(z.lifted.vert(ends[1], FaceId(0)), Some(c.last()));
        // On the floor's plane in between too.
        let mid = c.point(&q(0.5)).unwrap();
        assert_eq!(mid[2], 0u32);
        assert!(c.point(&q(2.0)).is_none());
    }
    // A vertex's place there is its own across the floor, `z` aside.
    for (v, f, p) in &z.lifted.verts {
        assert_eq!(*f, FaceId(0));
        let pos = geo.verts.iter().find(|g| g.key == *v).unwrap().pos;
        for k in 0..2 {
            let d = &p[k] - q(pos[k]);
            assert!(d < q(1e-12) && d > q(-1e-12), "{v:?}");
        }
    }
    assert_eq!(z.lifted.verts.len(), on.verts.len());
}

/// The same-surface rule of band `beta`.
fn same(beta: f64) -> HeightRule {
    HeightRule::SameSurface(Band::same_surface(beta, 0.0).unwrap())
}

#[test]
fn records_on_the_target_take_the_band_and_rim_ones_the_gap_sign() {
    let (rm, cr, r, geo, _) = snapped_bowl_with();
    let z = zone(&rm, &cr, &r, &geo, Tangency::Near, Sign::Neg).unwrap();
    let band = |x: ZoneEl| match x {
        ZoneEl::Vert(_) => 2e-4,
        ZoneEl::Edge(_) => 3e-4,
        _ => 0.0,
    };
    let floor = ZoneEl::TargetFace(FaceId(0));
    for rec in &z.rim.edges {
        let x = ZoneEl::Edge(rec.edge);
        // Against the floor: the gap sign, whichever way round.
        assert_eq!(
            z.height_rule(x, floor, &band),
            Some(HeightRule::RimGap(rec.sign))
        );
        assert_eq!(
            z.height_rule(floor, x, &band),
            Some(HeightRule::RimGap(rec.sign))
        );
        // Against another element on the floor: the band.
        let e = r.edges.iter().find(|e| e.key == rec.edge).unwrap();
        let v = ZoneEl::Vert(e.ends[0]);
        assert_eq!(z.height_rule(x, v, &band), Some(same(3e-4)));
        // Against the floor at another face: none beside it.
        assert_eq!(
            z.height_rule(x, ZoneEl::TargetFace(FaceId(3)), &band),
            Some(HeightRule::Ordinary)
        );
    }
    for v in &z.rim.verts {
        let x = ZoneEl::Vert(v.vert);
        let rule = z.height_rule(x, floor, &band);
        if v.tie {
            // A branch vertex on the floor: a tie by the band.
            assert_eq!(rule, Some(same(2e-4)));
        } else {
            assert_eq!(rule, Some(HeightRule::RimGap(v.sign)));
        }
    }
    // The kept ring's edges never are on the target.
    let kept = ZoneEl::Edge(SnapEdge::Piece {
        edge: EdgeId(0),
        part: 0,
    });
    assert!(!z.on_t(kept));
    assert_eq!(
        z.height_rule(kept, floor, &band),
        Some(HeightRule::Ordinary)
    );
    assert_eq!(
        z.height_rule(ZoneEl::Off, floor, &band),
        Some(HeightRule::Ordinary)
    );
    // A band that isn't finite: no rule.
    let bad = |x: ZoneEl| match x {
        ZoneEl::Edge(_) => f64::INFINITY,
        _ => 0.0,
    };
    for rec in &z.rim.edges {
        let e = r.edges.iter().find(|e| e.key == rec.edge).unwrap();
        let v = ZoneEl::Vert(e.ends[0]);
        assert_eq!(z.height_rule(ZoneEl::Edge(rec.edge), v, &bad), None);
    }
    assert!(!z.on_t(ZoneEl::Operand(Element::Edge(A, EdgeId(40)))));
}
