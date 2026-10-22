//! The L-shaped sheet with the quad over its corner square (the snap's
//! L-corner quad pair, the quad laid on the L): the zero-area region
//! along the L's two outer edges at the corner folds onto a tree, or is
//! dropped; a fold edge with one kept face passes the check where all its
//! members lie on the sheets' open boundaries, and fails where one is an
//! inner edge, a solid's or a new curve's; a ring of
//! arcs along a square's sides folds onto a cycle, also pinched at a
//! contracted corner; survivors by source; the quotient's shapes; the
//! input's refusals; the same result whatever the arcs' order; and
//! running out of budget.

use super::*;
use crate::boolean::arrange::coincident::{self, Along, Arc, Line, Place};
use crate::geom::curve::PCurve;
use crate::infra::budget::{Budget, Work};
use crate::topo::brep_min::{BodyKind, Brep};
use varde_testkit::sheets::{self, OFFSET, Polygons, l_corner_quad};

/// The straight p-curve from `a` to `b` over `[0, 1]`.
fn seg(a: [f64; 2], b: [f64; 2]) -> PCurve {
    let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    PCurve::new(
        vec![[[a[0], a[1], 1.0], [m[0], m[1], 1.0], [b[0], b[1], 1.0]]],
        vec![0.0, 1.0],
    )
    .unwrap()
}

const A0: Source = Source {
    operand: Operand::A,
    order: 0,
};

/// A face and what the fold reads of it.
#[derive(Default)]
struct Face {
    verts: Vec<Place>,
    vsrc: Vec<Source>,
    curves: Vec<(PCurve, [u32; 2], i32, Source)>,
    arcs: Vec<(Vec<Along>, [u32; 2], i32, u32)>,
    contract: Vec<u32>,
    /// The operands' stores (empty solids unless set).
    stores: [Brep; 2],
    /// Per line and per arc, the operand edge it is a piece of.
    line_edges: Vec<Option<OperandEdge>>,
    arc_edges: Vec<Option<OperandEdge>>,
}

/// The store of `p`, a sheet or a solid as `p` says.
fn store_of(p: &Polygons) -> Brep {
    let kind = if p.closed {
        BodyKind::Solid
    } else {
        BodyKind::Sheet
    };
    Brep::from_polygons(
        kind,
        &p.verts,
        &p.faces,
        2.5e-6,
        &mut Work::new(&Budget::DEFAULT),
    )
    .unwrap()
}

/// The edge of `b` from `p` to `q` either way, by its ends' positions.
fn edge_at(b: &Brep, p: [f64; 3], q: [f64; 3]) -> EdgeId {
    (0..b.edge_count() as u32)
        .map(EdgeId)
        .find(|&e| {
            let [x, y] = b.ends(e).map(|v| b.pos(v));
            (x, y) == (p, q) || (x, y) == (q, p)
        })
        .unwrap()
}

/// The fold's other inputs, per piece once the face is realised.
struct Rules {
    /// The side rank of a piece: lines' pieces 0, arcs' 1 (the arcs left).
    side: fn(&coincident::Piece) -> u32,
    across: u32,
    keep: fn(i64) -> bool,
}

impl Rules {
    fn new(across: u32, keep: fn(i64) -> bool) -> Rules {
        Rules {
            side: |p| u32::from(matches!(p.of, PieceOf::Arc { .. })),
            across,
            keep,
        }
    }
}

impl Face {
    fn vert(&mut self, p: Place, s: Source) -> u32 {
        self.verts.push(p);
        self.vsrc.push(s);
        (self.verts.len() - 1) as u32
    }

    fn line(&mut self, c: PCurve, ends: [u32; 2], wind: i32, s: Source) -> u32 {
        self.curves.push((c, ends, wind, s));
        self.line_edges.push(None);
        (self.curves.len() - 1) as u32
    }

    fn arc(&mut self, ends: [u32; 2], along: &[(u32, bool)], wind: i32, order: u32) -> u32 {
        let along = along
            .iter()
            .map(|&(line, forward)| Along { line, forward })
            .collect();
        self.arcs.push((along, ends, wind, order));
        self.arc_edges.push(None);
        (self.arcs.len() - 1) as u32
    }

    /// A closed polygon of lines through `corners`, winding 1 inside if
    /// they run counter-clockwise, all `A`'s.
    fn polygon(corners: &[[f64; 2]]) -> Face {
        let mut f = Face::default();
        let n = corners.len() as u32;
        for (k, &c) in corners.iter().enumerate() {
            f.vert(
                Place::Uv(c),
                Source {
                    operand: Operand::A,
                    order: k as u32,
                },
            );
        }
        for k in 0..n {
            let (a, b) = (k, (k + 1) % n);
            f.line(
                seg(corners[a as usize], corners[b as usize]),
                [a, b],
                1,
                Source {
                    operand: Operand::A,
                    order: k,
                },
            );
        }
        f
    }

    fn run_with(&self, rules: &Rules, budget: Budget) -> Result<(Coincident, Fold), FoldError> {
        let lines: Vec<Line<'_>> = self
            .curves
            .iter()
            .map(|(c, ends, wind, _)| Line {
                ends: *ends,
                pcurve: c,
                wind: *wind,
            })
            .collect();
        let arcs: Vec<Arc<'_>> = self
            .arcs
            .iter()
            .map(|(along, ends, wind, _)| Arc {
                ends: *ends,
                along,
                wind: *wind,
            })
            .collect();
        let face = FaceInput {
            verts: &self.verts,
            lines: &lines,
            arcs: &arcs,
        };
        let c = coincident::realise(&face, &mut Work::new(&Budget::DEFAULT)).unwrap();
        let side: Vec<u32> = c.pieces.iter().map(rules.side).collect();
        let across = vec![rules.across; c.pieces.len()];
        let lsrc: Vec<Source> = self.curves.iter().map(|x| x.3).collect();
        let aord: Vec<u32> = self.arcs.iter().map(|x| x.3).collect();
        let keep = rules.keep;
        let input = FoldInput {
            face: &face,
            realised: &c,
            side: &side,
            contract: &self.contract,
            lines: &lsrc,
            arcs: &aord,
            verts: &self.vsrc,
            across: &across,
            stores: [&self.stores[0], &self.stores[1]],
            line_edges: &self.line_edges,
            arc_edges: &self.arc_edges,
            keep: &keep,
        };
        let f = fold(&input, &mut Work::new(&budget))?;
        Ok((c, f))
    }

    fn run(&self, rules: &Rules) -> Result<(Coincident, Fold), FoldError> {
        self.run_with(rules, Budget::DEFAULT)
    }
}

/// The piece of `of` in `c`.
fn piece(c: &Coincident, of: PieceOf) -> u32 {
    c.pieces.iter().position(|p| p.of == of).unwrap() as u32
}

/// An arc's ends and its chain of lines, each with its way.
type Spec<'a> = ([u32; 2], &'a [(u32, bool)]);

/// The L-corner quad pair in the L's UV (`x`, `y`; the L faces up): the
/// L's six corners and sides (`A`'s), the quad's corners as records:
/// `c` at the L's corner 0, `P` on side 0, `R` at the reflex corner 3,
/// `Q` on side 5; its sides `c → P` along side 0 and `Q → c` along side
/// 5, `P → R → Q` across the L. Arcs in `order`, a permutation of
/// `0..4`. The stores are the pair's two sheets, each line a piece of
/// the L's edge, each arc of the quad's. Returns the face and
/// `[c, P, R, Q]`.
fn l_corner(order: [usize; 4]) -> (Face, [u32; 4]) {
    let pair = l_corner_quad(OFFSET);
    let l: Vec<[f64; 2]> = pair.a.verts.iter().map(|p| [p[0], p[1]]).collect();
    let q: Vec<[f64; 2]> = pair.b.verts.iter().map(|p| [p[0], p[1]]).collect();
    // The records the quad's corners give: at the L's corners 0 and 3,
    // and on its sides 0 (from corner 0 to 1) and 5 (from 5 to 0).
    assert_eq!((q[0], q[2]), (l[0], l[3]));
    assert!(q[1][1] == 0.0 && q[3][0] == 0.0);
    let mut f = Face::polygon(&l);
    let b = |order| Source {
        operand: Operand::B,
        order,
    };
    let new = |order| Source {
        operand: Operand::New,
        order,
    };
    let c = f.vert(Place::At(0), b(0));
    let p = f.vert(
        Place::On {
            line: 0,
            s: q[1][0] / l[1][0],
        },
        new(0),
    );
    let r = f.vert(Place::At(3), b(2));
    let qq = f.vert(
        Place::On {
            line: 5,
            s: (l[5][1] - q[3][1]) / l[5][1],
        },
        new(1),
    );
    let arcs: [Spec<'_>; 4] = [
        ([c, p], &[(0, true)]),
        ([p, r], &[]),
        ([r, qq], &[]),
        ([qq, c], &[(5, true)]),
    ];
    for k in order {
        f.arc(arcs[k].0, arcs[k].1, 1, k as u32);
    }
    f.stores = [store_of(&pair.a), store_of(&pair.b)];
    let ring = |n: usize, k: usize| [k, (k + 1) % n];
    for k in 0..6 {
        let [i, j] = ring(6, k);
        let e = edge_at(&f.stores[0], pair.a.verts[i], pair.a.verts[j]);
        f.line_edges[k] = Some((keys::Operand::A, e));
    }
    for (slot, &k) in order.iter().enumerate() {
        let [i, j] = ring(4, k);
        let e = edge_at(&f.stores[1], pair.b.verts[i], pair.b.verts[j]);
        f.arc_edges[slot] = Some((keys::Operand::B, e));
    }
    (f, [c, p, r, qq])
}

/// The slot of arc `k` (its place in the geometric order) in `f`.
fn arc_slot(f: &Face, k: u32) -> usize {
    f.arcs.iter().position(|a| a.3 == k).unwrap()
}

/// A's face kept outside the quad (`∪` with the quad's body on it: the
/// region under it goes), each edge with one kept side face across.
fn union_rules() -> Rules {
    Rules::new(1, |w| w == 1)
}

#[test]
fn l_corner_quad_folds_onto_a_tree() {
    let (f, [c_, p_, r_, q_]) = l_corner([0, 1, 2, 3]);
    let (c, fold) = f.run(&union_rules()).unwrap();
    assert_eq!(fold.regions.len(), 1);
    let reg = &fold.regions[0];
    assert_eq!(reg.winding, 1);
    assert_eq!(reg.fate, Fate::Folded(Shape::Tree));
    // One cycle: the L's sides into the corner, the quad's sides out.
    assert_eq!(reg.cycles.len(), 1);
    assert_eq!(reg.cycles[0].len(), 4);
    let side0 = piece(&c, PieceOf::Line { line: 0, part: 0 });
    let side5 = piece(&c, PieceOf::Line { line: 5, part: 1 });
    let arc_x = piece(&c, PieceOf::Arc { arc: 0, part: 0 });
    let arc_y = piece(&c, PieceOf::Arc { arc: 3, part: 0 });
    let mut want = vec![
        Collapsed {
            into: side0,
            members: vec![side0, arc_x],
            faces: 2,
        },
        Collapsed {
            into: side5,
            members: vec![side5, arc_y],
            faces: 2,
        },
    ];
    want.sort_by_key(|e| e.members[0]);
    assert_eq!(fold.edges, want);
    assert_eq!(fold.piece_into[arc_x as usize], side0);
    assert_eq!(fold.piece_into[arc_y as usize], side5);
    // The quad's corner merges into the L's; the others stay.
    assert_eq!(fold.vert_into[c_ as usize], 0);
    for v in [0, p_, r_, q_, 3] {
        assert_eq!(fold.vert_into[v as usize], v);
    }
}

#[test]
fn discarded_region_is_dropped() {
    // Only the overlap kept: the region between the sides goes.
    let (f, _) = l_corner([0, 1, 2, 3]);
    let (c, fold) = f.run(&Rules::new(1, |w| w == 2)).unwrap();
    assert_eq!(fold.regions.len(), 1);
    assert_eq!(fold.regions[0].fate, Fate::Dropped);
    assert!(fold.edges.is_empty());
    let ids: Vec<u32> = (0..c.pieces.len() as u32).collect();
    assert_eq!(fold.piece_into, ids);
}

/// Both sheets kept, no faces across (`∪` of the two sheets): each fold
/// edge has the quad on one side and nothing on the other.
fn sheet_union() -> Rules {
    Rules::new(0, |w| w >= 1)
}

#[test]
fn fold_edge_on_both_sheets_open_boundary_has_one_face() {
    // Each fold identifies an L side with a quad side, both on their
    // sheet's open boundary: one kept face, the edge open in the result.
    let (f, _) = l_corner([0, 1, 2, 3]);
    let (c, fold) = f.run(&sheet_union()).unwrap();
    assert_eq!(fold.regions.len(), 1);
    assert_eq!(fold.regions[0].fate, Fate::Folded(Shape::Tree));
    let side0 = piece(&c, PieceOf::Line { line: 0, part: 0 });
    let side5 = piece(&c, PieceOf::Line { line: 5, part: 1 });
    let into: Vec<(u32, u32)> = fold.edges.iter().map(|e| (e.into, e.faces)).collect();
    let mut want = vec![(side0, 1), (side5, 1)];
    want.sort_unstable();
    assert_eq!(into, want);
}

/// The refusal of a one-face fold edge.
fn assert_one_face_refused(f: &Face) {
    let err = f.run(&sheet_union()).unwrap_err();
    assert!(matches!(err, FoldError::Faces { faces: 1, .. }), "{err:?}");
    assert!(err.is_inconsistent());
    assert_eq!(
        err.kernel_error(),
        Some(KernelError::Internal(Internal::Inconsistent(
            Inconsistency::Proxy
        )))
    );
}

#[test]
fn fold_edge_with_one_kept_face_and_an_inner_member_fails() {
    // The quad with a flap hung on its side `c → P`: that side is an
    // inner edge of its sheet (its coedge has a pair), the L's side 0 is
    // on the L's open boundary. Mixed, so one kept face is refused.
    let (mut f, _) = l_corner([0, 1, 2, 3]);
    let mut b = l_corner_quad(OFFSET).b;
    let [q0, q1] = [b.verts[0], b.verts[1]];
    b.verts.push([q0[0], q0[1], -10.0]);
    b.verts.push([q1[0], q1[1], -10.0]);
    b.faces.push(vec![1, 0, 4, 5]);
    f.stores[1] = store_of(&b);
    let e = edge_at(&f.stores[1], q0, q1);
    let ce = f.stores[1].edge_coedge(e);
    assert_ne!(f.stores[1].pair(ce), CoedgeId::NONE);
    for k in 0..4 {
        let [i, j] = [k, (k + 1) % 4];
        let e = edge_at(&f.stores[1], b.verts[i], b.verts[j]);
        let slot = arc_slot(&f, k as u32);
        f.arc_edges[slot] = Some((keys::Operand::B, e));
    }
    assert_one_face_refused(&f);
}

#[test]
fn fold_edge_with_one_kept_face_on_a_solid_fails() {
    // The L's sides as the L prism's bottom edges: a closed body has no
    // open boundary, so one kept face is refused.
    let (mut f, _) = l_corner([0, 1, 2, 3]);
    let prism = sheets::l_corner(0.0).a;
    f.stores[0] = store_of(&prism);
    let l = l_corner_quad(OFFSET).a;
    for k in 0..6 {
        let e = edge_at(&f.stores[0], l.verts[k], l.verts[(k + 1) % 6]);
        f.line_edges[k] = Some((keys::Operand::A, e));
    }
    assert_one_face_refused(&f);
}

#[test]
fn fold_edge_with_one_kept_face_and_a_new_member_fails() {
    // The quad's side `c → P` given as a new curve, no operand edge.
    let (mut f, _) = l_corner([0, 1, 2, 3]);
    let slot = arc_slot(&f, 0);
    f.arc_edges[slot] = None;
    assert_one_face_refused(&f);
}

#[test]
fn interior_folds_keep_two_faces_or_none() {
    // A face across each side (the L's sides inner edges of a larger
    // body): the folds keep two faces under `∪`, the region dropped
    // under `∩`, whatever the stores say.
    let (f, _) = l_corner([0, 1, 2, 3]);
    let (_, fold) = f.run(&union_rules()).unwrap();
    assert!(fold.edges.iter().all(|e| e.faces == 2), "{:?}", fold.edges);
    let (_, fold) = f.run(&Rules::new(1, |w| w == 2)).unwrap();
    assert_eq!(fold.regions[0].fate, Fate::Dropped);
    assert!(fold.edges.is_empty());
}

#[test]
fn arcs_order_doesnt_change_the_fold() {
    let (f, _) = l_corner([0, 1, 2, 3]);
    let (c, fold) = f.run(&union_rules()).unwrap();
    let base = merges(&f, &c, &fold);
    for order in [[3, 2, 1, 0], [2, 0, 3, 1], [1, 3, 0, 2]] {
        let (f, _) = l_corner(order);
        let (c, got) = f.run(&union_rules()).unwrap();
        assert_eq!(got.vert_into, fold.vert_into);
        assert_eq!(merges(&f, &c, &got), base);
        assert_eq!(got.regions[0].fate, fold.regions[0].fate);
    }
}

/// A piece by what it is: a line's (0, line, part) or an arc's (1, its
/// place in the geometric order, part).
fn name(f: &Face, of: PieceOf) -> (u8, u32, u32) {
    match of {
        PieceOf::Line { line, part } => (0, line, part),
        PieceOf::Arc { arc, part } => (1, f.arcs[arc as usize].3, part),
    }
}

/// Each piece and the piece it merges into, by name, sorted.
fn merges(f: &Face, c: &Coincident, fold: &Fold) -> Vec<[(u8, u32, u32); 2]> {
    let mut m: Vec<_> = c
        .pieces
        .iter()
        .zip(&fold.piece_into)
        .map(|(p, &into)| [name(f, p.of), name(f, c.pieces[into as usize].of)])
        .collect();
    m.sort_unstable();
    m
}

const SQUARE: [[f64; 2]; 4] = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];

/// The square with a ring of arcs along its four sides from records
/// `c_k` at its corners (vertices 4..8), the ring's inside winding 2.
fn ring() -> Face {
    let mut f = Face::polygon(&SQUARE);
    let cs: Vec<u32> = (0..4)
        .map(|k| {
            f.vert(
                Place::At(k),
                Source {
                    operand: Operand::B,
                    order: k,
                },
            )
        })
        .collect();
    for k in 0..4u32 {
        f.arc(
            [cs[k as usize], cs[(k as usize + 1) % 4]],
            &[(k, true)],
            1,
            k,
        );
    }
    f
}

#[test]
fn ring_folds_onto_a_cycle() {
    let (_, fold) = ring().run(&union_rules()).unwrap();
    assert_eq!(fold.regions.len(), 1);
    let reg = &fold.regions[0];
    // An annulus: the sides' cycle and the ring's.
    assert_eq!(reg.cycles.len(), 2);
    assert_eq!(reg.fate, Fate::Folded(Shape::Cycle));
    assert_eq!(fold.edges.len(), 4);
    assert!(fold.edges.iter().all(|e| e.faces == 2));
    assert_eq!(&fold.vert_into[4..8], &[0, 1, 2, 3]);
}

#[test]
fn ring_pinched_at_a_contracted_corner_still_folds_onto_a_cycle() {
    // The ring's record at corner 0 joined to the corner by a zero-length
    // edge that is contracted: one boundary cycle through the corner.
    let mut f = ring();
    f.arc([0, 4], &[], 0, 4);
    f.contract.push(0);
    let (c, fold) = f.run(&union_rules()).unwrap();
    assert_eq!(c.zeros.len(), 1);
    assert_eq!(fold.regions.len(), 1);
    assert_eq!(fold.regions[0].cycles.len(), 1);
    assert_eq!(fold.regions[0].cycles[0].len(), 8);
    assert_eq!(fold.regions[0].fate, Fate::Folded(Shape::Cycle));
}

#[test]
fn survivors_follow_sources() {
    // The sides new (after the arcs in the geometric order), the corners
    // new and the ring's records `B`'s: the arcs' pieces and the records
    // survive.
    let mut f = ring();
    for (k, l) in f.curves.iter_mut().enumerate() {
        l.3 = Source {
            operand: Operand::New,
            order: 10 + k as u32,
        };
    }
    for s in &mut f.vsrc[..4] {
        s.operand = Operand::New;
    }
    let (c, fold) = f.run(&union_rules()).unwrap();
    for e in &fold.edges {
        assert!(matches!(c.pieces[e.into as usize].of, PieceOf::Arc { .. }));
    }
    assert_eq!(&fold.vert_into[..4], &[4, 5, 6, 7]);
}

#[test]
fn quotient_shapes() {
    assert_eq!(shape(3, &[[0, 1], [1, 2]]), Some(Shape::Tree));
    assert_eq!(shape(1, &[]), Some(Shape::Tree));
    assert_eq!(shape(3, &[[0, 1], [1, 2], [2, 0]]), Some(Shape::Cycle));
    assert_eq!(shape(1, &[[0, 0]]), Some(Shape::Cycle));
    // A theta, a disconnected pair, a cycle with a tail.
    assert_eq!(shape(2, &[[0, 1], [0, 1], [0, 1]]), None);
    assert_eq!(shape(4, &[[0, 1], [2, 3]]), None);
    assert_eq!(shape(3, &[[0, 1], [1, 1], [1, 2]]), None);
    assert_eq!(shape(4, &[[0, 1], [1, 2], [2, 0], [2, 3]]), None);
}

#[test]
fn refusals_name_their_entity() {
    // Two members of a bundle on one side rank.
    let (f, _) = l_corner([0, 1, 2, 3]);
    let rules = Rules {
        side: |_| 0,
        ..union_rules()
    };
    let err = f.run(&rules).unwrap_err();
    assert!(matches!(err, FoldError::Side(_)), "{err:?}");
    assert_eq!(err.kernel_error(), None);

    // A contracted entry naming no zero-length edge.
    let (mut f, _) = l_corner([0, 1, 2, 3]);
    f.contract.push(0);
    assert_eq!(f.run(&union_rules()).unwrap_err(), FoldError::Contract(0));

    // A column of the wrong length.
    let (mut f, _) = l_corner([0, 1, 2, 3]);
    f.vsrc.pop();
    assert_eq!(f.run(&union_rules()).unwrap_err(), FoldError::Input);
    let (mut f, _) = l_corner([0, 1, 2, 3]);
    f.arc_edges.pop();
    assert_eq!(f.run(&union_rules()).unwrap_err(), FoldError::Input);

    // An operand edge its store doesn't have.
    let (mut f, _) = l_corner([0, 1, 2, 3]);
    f.line_edges[0] = Some((keys::Operand::B, EdgeId(99)));
    assert_eq!(f.run(&union_rules()).unwrap_err(), FoldError::Input);
}

#[test]
fn no_coincident_arcs_no_regions() {
    let mut f = Face::polygon(&SQUARE);
    let x = f.vert(Place::Uv([1.0, 1.0]), A0);
    f.arc([0, x], &[], 0, 0);
    let (_, fold) = f.run(&union_rules()).unwrap();
    assert!(fold.regions.is_empty() && fold.edges.is_empty());
}

#[test]
fn small_budget_runs_out() {
    let (f, _) = l_corner([0, 1, 2, 3]);
    let got = f.run_with(&union_rules(), Budget::new(10));
    assert!(matches!(got, Err(FoldError::OutOfBudget(_))), "{got:?}");
}
