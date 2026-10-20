//! A square face's lines with arcs along them: a flush tab whose bottom
//! is a piece of the face's edge, an arc along a chain through its joining
//! vertex, an arc running against its line; zero-length arcs at a corner
//! and a whole loop of them contracted; a record pinned to an edge, alone
//! and where another record is the edge's vertex; a line bent at a knot;
//! spans along each other without a record refused; each refusal of the
//! input with its entity; the same result whatever the arcs' order; and
//! running out of budget.

use super::*;
use crate::geom::curve::PCurve;
use crate::infra::budget::{Budget, Work};

/// The straight p-curve from `a` to `b` over `[0, 1]`.
fn seg(a: [f64; 2], b: [f64; 2]) -> PCurve {
    let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    PCurve::new(
        vec![[[a[0], a[1], 1.0], [m[0], m[1], 1.0], [b[0], b[1], 1.0]]],
        vec![0.0, 1.0],
    )
    .unwrap()
}

/// The polyline p-curve through `pts`, one straight span each, knots
/// `0, 1, 2, …`.
fn poly(pts: &[[f64; 2]]) -> PCurve {
    let spans = pts
        .windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            [[a[0], a[1], 1.0], [m[0], m[1], 1.0], [b[0], b[1], 1.0]]
        })
        .collect();
    PCurve::new(spans, (0..pts.len()).map(|k| k as f64).collect()).unwrap()
}

/// An arc's ends and its chain of lines, each with its way.
type Spec<'a> = ([u32; 2], &'a [(u32, bool)]);

const CORNERS: [[f64; 2]; 4] = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];

/// A face: vertices, lines' p-curves (and their ends and windings), arcs.
#[derive(Default)]
struct Face {
    verts: Vec<Place>,
    curves: Vec<(PCurve, [u32; 2], i32)>,
    arcs: Vec<(Vec<Along>, [u32; 2], i32)>,
}

impl Face {
    /// The square `[0, 2]²`, vertices 0..4 counter-clockwise from the
    /// origin, lines 0..4 along its sides, winding 1 inside.
    fn square() -> Face {
        let mut f = Face::default();
        for c in CORNERS {
            f.vert(Place::Uv(c));
        }
        for k in 0..4u32 {
            let (a, b) = (k, (k + 1) % 4);
            f.line(seg(CORNERS[a as usize], CORNERS[b as usize]), [a, b], 1);
        }
        f
    }

    fn vert(&mut self, p: Place) -> u32 {
        self.verts.push(p);
        (self.verts.len() - 1) as u32
    }

    fn line(&mut self, c: PCurve, ends: [u32; 2], wind: i32) -> u32 {
        self.curves.push((c, ends, wind));
        (self.curves.len() - 1) as u32
    }

    fn arc(&mut self, ends: [u32; 2], along: &[(u32, bool)], wind: i32) -> u32 {
        let along = along
            .iter()
            .map(|&(line, forward)| Along { line, forward })
            .collect();
        self.arcs.push((along, ends, wind));
        (self.arcs.len() - 1) as u32
    }

    fn run_with(&self, budget: Budget) -> Result<Coincident, CoincidentError> {
        let lines: Vec<Line<'_>> = self
            .curves
            .iter()
            .map(|(c, ends, wind)| Line {
                ends: *ends,
                pcurve: c,
                wind: *wind,
            })
            .collect();
        let arcs: Vec<Arc<'_>> = self
            .arcs
            .iter()
            .map(|(along, ends, wind)| Arc {
                ends: *ends,
                along,
                wind: *wind,
            })
            .collect();
        let input = FaceInput {
            verts: &self.verts,
            lines: &lines,
            arcs: &arcs,
        };
        realise(&input, &mut Work::new(&budget))
    }

    fn run(&self) -> Result<Coincident, CoincidentError> {
        self.run_with(Budget::DEFAULT)
    }
}

/// The regions' windings, ascending.
fn windings(c: &Coincident) -> Vec<i64> {
    let mut w: Vec<i64> = c.arrangement.regions().iter().map(|r| r.winding).collect();
    w.sort_unstable();
    w
}

/// The pieces of `arc`, in order.
fn arc_pieces(c: &Coincident, arc: u32) -> Vec<Piece> {
    c.pieces
        .iter()
        .filter(|p| matches!(p.of, PieceOf::Arc { arc: a, .. } if a == arc))
        .copied()
        .collect()
}

/// The square with a tab on its bottom side: records `P`, `Q` on line 0
/// at `1/4` and `3/4` (vertices 4, 5), `R`, `S` above them (6, 7); the
/// tab `P → Q → R → S` counter-clockwise, winding 1 inside, its bottom
/// along line 0. Arcs in `order` (a permutation of `0..4`).
fn tab(order: [usize; 4]) -> Face {
    let mut f = Face::square();
    let p = f.vert(Place::On { line: 0, s: 0.25 });
    let q = f.vert(Place::On { line: 0, s: 0.75 });
    let r = f.vert(Place::Uv([1.5, 1.0]));
    let s = f.vert(Place::Uv([0.5, 1.0]));
    let arcs: [Spec<'_>; 4] = [
        ([p, q], &[(0, true)]),
        ([q, r], &[]),
        ([r, s], &[]),
        ([s, p], &[]),
    ];
    for k in order {
        f.arc(arcs[k].0, arcs[k].1, 1);
    }
    f
}

#[test]
fn flush_tab_bottom_is_a_piece_of_the_edge() {
    let c = tab([0, 1, 2, 3]).run().unwrap();
    // Line 0 split at P and Q; the arc's one piece reuses its middle.
    let line0: Vec<Piece> = c
        .pieces
        .iter()
        .filter(|p| matches!(p.of, PieceOf::Line { line: 0, .. }))
        .copied()
        .collect();
    assert_eq!(
        line0.iter().map(|p| p.ends).collect::<Vec<_>>(),
        [[0, 4], [4, 5], [5, 1]]
    );
    let bottom = arc_pieces(&c, 0);
    assert_eq!(bottom.len(), 1);
    assert_eq!(bottom[0].ends, [4, 5]);
    assert_eq!(bottom[0].reuses, Some((0, [0.25, 0.75])));
    assert_eq!(bottom[0].bundle, line0[1].bundle);
    let b = &c.bundles[bottom[0].bundle as usize];
    assert_eq!(b.members.len(), 2);
    assert_eq!(b.wind, 2);
    // The points are where line 0's p-curve puts them, bit for bit.
    assert_eq!(c.points[c.point_of[4] as usize].uv, [0.5, 0.0]);
    // The square's inside (1) and the tab (2), around the outside (0).
    assert_eq!(windings(&c), [0, 1, 2]);
    // Every piece is in one bundle; no zero-length edges.
    assert!(c.zeros.is_empty());
    let members: usize = c.bundles.iter().map(|b| b.members.len()).sum();
    assert_eq!(members, c.pieces.len());
}

#[test]
fn arcs_order_doesnt_change_the_result() {
    let base = tab([0, 1, 2, 3]).run().unwrap();
    for order in [[3, 2, 1, 0], [2, 0, 3, 1], [1, 3, 0, 2]] {
        let c = tab(order).run().unwrap();
        assert_eq!(windings(&c), windings(&base));
        assert_eq!(c.points, base.points);
        let mut sizes: Vec<(usize, i32)> = c
            .bundles
            .iter()
            .map(|b| (b.members.len(), b.wind))
            .collect();
        let mut want: Vec<(usize, i32)> = base
            .bundles
            .iter()
            .map(|b| (b.members.len(), b.wind))
            .collect();
        sizes.sort_unstable();
        want.sort_unstable();
        assert_eq!(sizes, want);
    }
}

#[test]
fn arc_along_a_chain_splits_at_its_joining_vertex() {
    // The bottom side as two lines joined at M = (1, 0).
    let mut f = Face::default();
    for c in CORNERS {
        f.vert(Place::Uv(c));
    }
    let m = f.vert(Place::Uv([1.0, 0.0]));
    f.line(seg(CORNERS[0], [1.0, 0.0]), [0, m], 1);
    f.line(seg([1.0, 0.0], CORNERS[1]), [m, 1], 1);
    for k in 1..4u32 {
        let (a, b) = (k, (k + 1) % 4);
        f.line(seg(CORNERS[a as usize], CORNERS[b as usize]), [a, b], 1);
    }
    let p = f.vert(Place::On { line: 0, s: 0.5 });
    let q = f.vert(Place::On { line: 1, s: 0.5 });
    let r = f.vert(Place::Uv([1.5, 1.0]));
    let s = f.vert(Place::Uv([0.5, 1.0]));
    f.arc([p, q], &[(0, true), (1, true)], 1);
    f.arc([q, r], &[], 1);
    f.arc([r, s], &[], 1);
    f.arc([s, p], &[], 1);
    let c = f.run().unwrap();
    let ps = arc_pieces(&c, 0);
    assert_eq!(
        ps.iter().map(|p| (p.ends, p.reuses)).collect::<Vec<_>>(),
        [
            ([p, m], Some((0, [0.5, 1.0]))),
            ([m, q], Some((1, [0.0, 0.5]))),
        ]
    );
    assert!(
        ps.iter()
            .all(|p| c.bundles[p.bundle as usize].members.len() == 2)
    );
    assert_eq!(windings(&c), [0, 1, 2]);
}

#[test]
fn arc_against_its_line_subtracts_its_step() {
    // The tab's bottom given from Q to P against line 0, winding −1 that
    // way: the same tab.
    let mut f = Face::square();
    let p = f.vert(Place::On { line: 0, s: 0.25 });
    let q = f.vert(Place::On { line: 0, s: 0.75 });
    let r = f.vert(Place::Uv([1.5, 1.0]));
    let s = f.vert(Place::Uv([0.5, 1.0]));
    f.arc([q, p], &[(0, false)], -1);
    f.arc([q, r], &[], 1);
    f.arc([r, s], &[], 1);
    f.arc([s, p], &[], 1);
    let c = f.run().unwrap();
    let ps = arc_pieces(&c, 0);
    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].ends, [q, p]);
    assert_eq!(ps[0].reuses, Some((0, [0.75, 0.25])));
    assert!(!ps[0].forward);
    assert_eq!(c.bundles[ps[0].bundle as usize].wind, 2);
    assert_eq!(windings(&c), [0, 1, 2]);
}

#[test]
fn zero_length_arc_at_a_corner() {
    // Two corner records at vertex 0, each a distinct vertex there; the
    // arc between them is zero-length, the tab's sides end at them.
    let mut f = Face::square();
    let a = f.vert(Place::At(0));
    let b = f.vert(Place::At(0));
    let r = f.vert(Place::Uv([1.0, 1.0]));
    f.arc([a, b], &[], 1);
    f.arc([b, r], &[], 0);
    let c = f.run().unwrap();
    assert_eq!(
        c.zeros,
        [Zero {
            of: ZeroOf::Arc(0),
            ends: [a, b],
            point: c.point_of[0],
        }]
    );
    assert_eq!(c.point_of[a as usize], c.point_of[0]);
    let pt = &c.points[c.point_of[0] as usize];
    assert_eq!(pt.verts, [0, a, b]);
    assert!(!pt.contracted);
    // The arc to R dangles inside the square.
    assert_eq!(windings(&c), [0, 1]);
}

#[test]
fn whole_zero_length_loop_is_contracted() {
    // A point contact inside the face: three records at one point, a
    // loop of zero-length arcs through them. The face stays whole.
    let mut f = Face::square();
    let x = f.vert(Place::Uv([1.0, 1.0]));
    let y = f.vert(Place::At(x));
    let z = f.vert(Place::At(x));
    f.arc([x, y], &[(0, true)], 1);
    f.arc([y, z], &[], 1);
    f.arc([z, x], &[], 1);
    let c = f.run().unwrap();
    assert_eq!(c.zeros.len(), 3);
    let pt = &c.points[c.point_of[x as usize] as usize];
    assert!(pt.contracted);
    assert_eq!(pt.verts, [x, y, z]);
    assert_eq!(c.bundles.len(), 4);
    assert_eq!(windings(&c), [0, 1]);
}

#[test]
fn pinned_record_splits_the_edge_at_its_own_vertex() {
    // A slit's end at the middle of line 0, of another member: line 0
    // gets a pin vertex there, joined to the slit's end.
    let mut f = Face::square();
    let x = f.vert(Place::Pin { line: 0, s: 0.5 });
    let y = f.vert(Place::Uv([1.0, 1.0]));
    f.arc([x, y], &[], 0);
    let c = f.run().unwrap();
    let pin = f.verts.len() as u32;
    assert_eq!(c.pins, [PinVert { line: 0, s: 0.5 }]);
    assert_eq!(
        c.zeros,
        [Zero {
            of: ZeroOf::Pin(x),
            ends: [pin, x],
            point: c.point_of[x as usize],
        }]
    );
    assert_eq!(c.point_of[pin as usize], c.point_of[x as usize]);
    assert_eq!(c.points[c.point_of[x as usize] as usize].verts, [x, pin]);
    let line0: Vec<[u32; 2]> = c
        .pieces
        .iter()
        .filter(|p| matches!(p.of, PieceOf::Line { line: 0, .. }))
        .map(|p| p.ends)
        .collect();
    assert_eq!(line0, [[0, pin], [pin, 1]]);
    assert_eq!(windings(&c), [0, 1]);
}

#[test]
fn pin_joins_the_records_vertex_on_the_edge() {
    // One record lies on line 0 at 1/2, another member's is pinned there:
    // no pin vertex, the join goes to the first.
    let mut f = Face::square();
    let on = f.vert(Place::On { line: 0, s: 0.5 });
    let x = f.vert(Place::Pin { line: 0, s: 0.5 });
    let y = f.vert(Place::Uv([1.0, 1.0]));
    f.arc([x, y], &[], 0);
    let c = f.run().unwrap();
    assert!(c.pins.is_empty());
    assert_eq!(c.zeros[0].ends, [on, x]);
    assert_eq!(c.point_of[on as usize], c.point_of[x as usize]);
}

#[test]
fn line_bends_at_its_inner_knots() {
    let mut f = Face::default();
    for c in CORNERS {
        f.vert(Place::Uv(c));
    }
    f.line(poly(&[CORNERS[0], [1.0, -0.5], CORNERS[1]]), [0, 1], 1);
    for k in 1..4u32 {
        let (a, b) = (k, (k + 1) % 4);
        f.line(seg(CORNERS[a as usize], CORNERS[b as usize]), [a, b], 1);
    }
    // A record on the bent line's second span, and an arc along it from
    // the start: it keeps the bend.
    let p = f.vert(Place::On { line: 0, s: 1.5 });
    let r = f.vert(Place::Uv([1.0, 1.0]));
    f.arc([0, p], &[(0, true)], 1);
    f.arc([p, r], &[], 1);
    f.arc([r, 0], &[], 1);
    let c = f.run().unwrap();
    assert_eq!(c.points.len(), 6);
    // The bundle from vertex 0 to P runs as two spans, through the bend.
    let b = c.pieces[arc_pieces(&c, 0)[0].bundle as usize].bundle;
    assert_eq!(c.span_bundle.iter().filter(|&&x| x == b).count(), 2);
    assert_eq!(c.span_bundle.len(), 8);
    assert_eq!(windings(&c), [0, 1, 2]);
}

#[test]
fn spans_along_each_other_without_a_record_are_refused() {
    // The tab's bottom given as a transversal arc: it runs along line 0's
    // piece with nothing that says so.
    let mut f = Face::square();
    let p = f.vert(Place::On { line: 0, s: 0.25 });
    let q = f.vert(Place::On { line: 0, s: 0.75 });
    let r = f.vert(Place::Uv([1.0, 1.0]));
    f.arc([p, q], &[], 1);
    f.arc([q, r], &[], 1);
    f.arc([r, p], &[], 1);
    let c = f.run().unwrap_err();
    // Line 0's middle piece is bundle 1; the transversal arc's bundle
    // follows the six line pieces.
    assert!(matches!(
        c,
        CoincidentError::Arrange(ArrangeError::Coincident { edges: [1, 6], .. })
    ));
}

#[test]
fn refusals_name_their_entity() {
    let mut f = Face::square();
    f.vert(Place::At(4));
    assert_eq!(f.run().unwrap_err(), CoincidentError::Place(4));

    let mut f = Face::square();
    let a = f.vert(Place::Uv([1.0, 1.0]));
    f.vert(Place::At(a));
    f.vert(Place::At(5));
    assert_eq!(f.run().unwrap_err(), CoincidentError::Place(6));

    let mut f = Face::square();
    f.vert(Place::On { line: 0, s: 1.0 });
    assert_eq!(f.run().unwrap_err(), CoincidentError::Place(4));

    let mut f = Face::square();
    f.vert(Place::On { line: 1, s: 0.5 });
    f.vert(Place::On { line: 1, s: 0.5 });
    assert_eq!(f.run().unwrap_err(), CoincidentError::Split(5));

    // A p-curve not ending at its vertex's point, to the bit.
    let mut f = Face::square();
    f.curves[2].0 = seg(CORNERS[2], [1e-300, 2.0]);
    assert_eq!(f.run().unwrap_err(), CoincidentError::Line(2));

    // An arc whose end isn't on its chain's first line.
    let mut f = Face::square();
    let p = f.vert(Place::On { line: 0, s: 0.5 });
    f.arc([p, 2], &[(1, true)], 1);
    assert_eq!(f.run().unwrap_err(), CoincidentError::Arc(0));

    // An arc whose chain doesn't join up.
    let mut f = Face::square();
    f.arc([0, 3], &[(0, true), (2, true)], 1);
    assert_eq!(f.run().unwrap_err(), CoincidentError::Arc(0));

    // An arc whose span runs against its line.
    let mut f = Face::square();
    let p = f.vert(Place::On { line: 0, s: 0.25 });
    let q = f.vert(Place::On { line: 0, s: 0.75 });
    f.arc([q, p], &[(0, true)], 1);
    assert_eq!(f.run().unwrap_err(), CoincidentError::Arc(0));

    // A bundle's winding step out of range.
    let mut f = Face::square();
    f.curves[0].2 = i32::MAX;
    f.arc([0, 1], &[(0, true)], 1);
    assert_eq!(f.run().unwrap_err(), CoincidentError::Wind(0));
}

#[test]
fn small_budget_runs_out() {
    let got = tab([0, 1, 2, 3]).run_with(Budget::new(5));
    assert!(matches!(got, Err(CoincidentError::OutOfBudget(_))));
}
