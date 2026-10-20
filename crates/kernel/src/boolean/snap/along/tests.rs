use std::collections::BTreeMap;

use varde_decide::order::{EdgeKey, EntityOrder, VertKey};

use super::super::cluster::tests::{B_K, rat, work};
use super::super::corner::{CornerInput, corner};
use super::super::rim::Along as RimAlong;
use super::*;

const A: Operand = Operand::A;
const B: Operand = Operand::B;

/// Vertices and straight edges of both operands, ordered by position.
#[derive(Default)]
struct Geom {
    verts: BTreeMap<(Operand, VertId), [f64; 3]>,
    edges: BTreeMap<(Operand, EdgeId), [VertId; 2]>,
}

impl Geom {
    fn vert(&mut self, o: Operand, v: u32, p: [f64; 3]) -> &mut Self {
        self.verts.insert((o, VertId(v)), p);
        self
    }

    fn edge(&mut self, o: Operand, g: u32, a: u32, b: u32) -> &mut Self {
        self.edges.insert((o, EdgeId(g)), [VertId(a), VertId(b)]);
        self
    }
}

impl EntityOrder for Geom {
    type Key = ();

    fn ends(&self, o: Operand, g: EdgeId) -> [VertId; 2] {
        self.edges[&(o, g)]
    }

    fn vert_key(&self, o: Operand, v: VertId) -> VertKey<()> {
        VertKey::new(self.verts[&(o, v)], []).unwrap()
    }

    fn edge_key(&self, o: Operand, g: EdgeId) -> EdgeKey<()> {
        let [a, b] = self.ends(o, g);
        let (p, q) = (self.verts[&(o, a)], self.verts[&(o, b)]);
        let mid = [0, 1, 2].map(|k| 0.5 * (p[k] + q[k]));
        EdgeKey::new([self.vert_key(o, a), self.vert_key(o, b)], mid, [(), ()]).unwrap()
    }
}

fn rim_band(_: RimAlong) -> f64 {
    1e-3
}

/// Every element's band: `1e-4` mm, so `R_on = b_K` throughout.
fn narrow(_: Element) -> f64 {
    1e-4
}

fn unrelated(_: EdgeId, _: EdgeId) -> bool {
    false
}

fn related(_: EdgeId, _: EdgeId) -> bool {
    true
}

const EA: Element = Element::Edge(A, EdgeId(0));
const EB0: Element = Element::Edge(B, EdgeId(0));
const EB1: Element = Element::Edge(B, EdgeId(1));

fn va(v: u32) -> Element {
    Element::Vert(A, VertId(v))
}

fn vb(v: u32) -> Element {
    Element::Vert(B, VertId(v))
}

/// The corner rule, then the pairs, of a cluster snapping `B` onto `A`
/// along `z` whose window is `window`.
fn decide(
    g: &Geom,
    window: &[Element],
    same_form: &(dyn Fn(EdgeId, EdgeId) -> bool + Sync),
) -> Result<(Corner, SnapAlong), AlongError> {
    let mut window = window.to_vec();
    window.sort();
    let rim = Rim {
        pieces: Vec::new(),
        loops: Vec::new(),
        gaps: Vec::new(),
        clusters: Vec::new(),
        branches: Vec::new(),
        window,
        bounds: Vec::new(),
    };
    let rim_input = RimInput {
        snapped: B,
        faces: &[],
        targets: &[],
        band: &rim_band,
    };
    let c = corner(
        &rim,
        &rim_input,
        &CornerInput {
            b_k: B_K,
            order: g,
            band: &narrow,
            candidates: &[],
            fixed: &[],
            groups: &[],
            coincident: &[],
        },
        &mut work(),
    )
    .unwrap();
    let map = Map::Along([rat(0.0), rat(0.0), rat(1.0)]);
    let s = snap_along(
        &rim,
        &rim_input,
        &c,
        &AlongInput {
            b_k: B_K,
            order: g,
            band: &narrow,
            map: &map,
            same_form,
        },
        &mut work(),
    )?;
    Ok((c, s))
}

fn pair(s: &SnapAlong, h: u32) -> &AlongPair {
    s.pair(EdgeId(h), EdgeId(0))
        .unwrap_or_else(|| panic!("pair of {h}: {s:?}"))
}

fn classes(p: &AlongPair) -> Vec<CellClass> {
    p.cells.iter().map(|c| c.class).collect()
}

/// The floor's edge `e` from `(0, 0, 0)` to `(len, 0, 0)`.
fn floor(len: f64) -> Geom {
    let mut g = Geom::default();
    g.vert(A, 0, [0.0, 0.0, 0.0])
        .vert(A, 1, [len, 0.0, 0.0])
        .edge(A, 0, 0, 1);
    g
}

#[test]
fn a_touch_at_one_point_reuses_the_target_edge() {
    // The flat analogue of a rod's seam tangent to a hole's rim: two `S`
    // edges meet at a vertex `10⁻⁴` above and beside `e`, which the
    // corner rule places on `e`. Each edge's image touches `e` there and
    // stays on one side: a bracket with no sign change, so the edge's
    // stretch within `b_K` of `e` reuses it, from the foot of its
    // bracket end to the placed vertex.
    let mut g = floor(10.0);
    g.vert(B, 0, [1.0, 2e-3, 1e-4])
        .vert(B, 1, [5.0, 1e-4, 1e-4])
        .vert(B, 2, [9.0, 2e-3, 1e-4])
        .edge(B, 0, 0, 1)
        .edge(B, 1, 1, 2);
    let (c, s) = decide(&g, &[EA, EB0, EB1, vb(1)], &unrelated).unwrap();
    assert!(matches!(
        c.placed
            .iter()
            .find(|p| p.vert == VertId(1))
            .unwrap()
            .moved
            .to,
        Placement::Edge {
            edge: EdgeId(0),
            ..
        }
    ));
    for (h, span, foot, at_vertex) in [
        (0, [rat(0.875), rat(1.0)], 0.45, 1),
        (1, [rat(0.0), rat(0.125)], 0.55, 0),
    ] {
        let p = pair(&s, h);
        assert!(p.crossings.is_empty(), "{p:?}");
        assert_eq!(p.pieces.len(), 1, "{p:?}");
        let piece = &p.pieces[0];
        assert_eq!(piece.span, span);
        assert!(!piece.by_form && piece.index == 0);
        let end = &piece.ends[at_vertex];
        assert_eq!(end.moved.to, Placement::Vert(B, VertId(1)));
        assert_eq!(end.moved.pos, Some([5.0, 0.0, 0.0]));
        let other = &piece.ends[1 - at_vertex];
        assert_eq!(
            other.moved.to,
            Placement::Edge {
                edge: EdgeId(0),
                s: foot
            }
        );
        for end in &piece.ends {
            assert!(end.moved.displacement <= B_K, "{end:?}");
        }
        assert_eq!(
            piece.ends.each_ref().map(|e| e.which),
            [EndKind::Start, EndKind::End]
        );
        // Off `e` up to the bracket: no cell crosses it.
        assert!(
            classes(p)
                .iter()
                .all(|c| matches!(c, CellClass::Clear | CellClass::Near))
        );
    }
}

#[test]
fn a_crossing_at_any_angle_is_ordinary() {
    // Edges crossing `e` at its middle: steep ones certify one sign change
    // in one cell, a grazing one lying within `b_K` of `e` over its whole
    // length is one bracket with one transversal sign change. None reuses
    // `e`.
    for (a, b, class) in [
        ([5.0, -1.0, 1e-4], [5.0, 1.0, 1e-4], CellClass::Crossing),
        ([4.0, -1.0, 1e-4], [6.0, 1.0, 1e-4], CellClass::Crossing),
        ([1.0, -2e-3, 1e-4], [9.0, 2e-3, 1e-4], CellClass::Crossing),
        ([1.0, -2e-4, 1e-4], [9.0, 2e-4, 1e-4], CellClass::Near),
    ] {
        let mut g = floor(10.0);
        g.vert(B, 0, a).vert(B, 1, b).edge(B, 0, 0, 1);
        let (_, s) = decide(&g, &[EA, EB0], &unrelated).unwrap();
        let p = pair(&s, 0);
        assert_eq!(classes(p), vec![class], "{a:?}");
        assert_eq!(
            p.crossings,
            vec![Crossing {
                s: rat(0.5),
                u: rat(0.5)
            }]
        );
        assert!(p.pieces.is_empty());
    }
}

#[test]
fn a_run_along_reuses_the_target_edge_up_to_its_end_vertex() {
    // An `S` edge `10⁻⁴` above and beside `e`, overhanging `e`'s end `w`:
    // the corner rule places its start on `e` and splits it at `w`. The
    // span up to `w` is one bracket, a run along `e`, ending at the
    // placed vertex and at `w`; past `w` the edge leaves `e` (vertex-clear).
    let mut g = floor(5.0);
    g.vert(B, 0, [1.0, 1e-4, 1e-4])
        .vert(B, 1, [8.0, 1e-4, 1e-4])
        .edge(B, 0, 0, 1);
    let (c, s) = decide(&g, &[EA, va(0), va(1), EB0, vb(0), vb(1)], &unrelated).unwrap();
    assert_eq!(c.splits.len(), 1);
    let p = pair(&s, 0);
    assert_eq!(classes(p), vec![CellClass::Near, CellClass::VertexClear]);
    assert!(p.crossings.is_empty());
    assert_eq!(p.pieces.len(), 1);
    let piece = &p.pieces[0];
    assert_eq!(piece.span[0], rat(0.0));
    assert_eq!(piece.span[1], rat(c.splits[0].s));
    assert_eq!(piece.ends[0].moved.to, Placement::Vert(B, VertId(0)));
    assert_eq!(piece.ends[0].moved.pos, Some([1.0, 0.0, 0.0]));
    assert_eq!(piece.ends[1].moved.to, Placement::Vert(A, VertId(1)));
    assert!(piece.ends.iter().all(|e| e.moved.displacement <= B_K));
}

#[test]
fn a_vertex_clear_cell_ends_at_the_shared_vertex() {
    // `S` edges leaving `e`'s end `w`, where the corner rule placed their
    // start: one at an angle, one along `e`'s line away from `e`. Each is
    // off `e` but at `w`, in one cell; neither reuses `e`.
    for b in [[9.0, 3.0, 1e-4], [9.0, 0.0, 1e-4]] {
        let mut g = floor(5.0);
        g.vert(B, 0, [5.0, 1e-4, 1e-4])
            .vert(B, 1, b)
            .edge(B, 0, 0, 1);
        let (c, s) = decide(&g, &[EA, va(1), EB0, vb(0)], &unrelated).unwrap();
        assert_eq!(c.placed[0].moved.to, Placement::Vert(A, VertId(1)));
        let p = pair(&s, 0);
        assert_eq!(classes(p), vec![CellClass::VertexClear], "{b:?}");
        assert!(p.crossings.is_empty() && p.pieces.is_empty());
    }
}

#[test]
fn related_forms_give_the_whole_bracket_at_once() {
    // An `S` edge on `e`'s line (its form related to `e`'s), overhanging
    // `e`'s end: the part over `e`'s range is one bracket, without cells,
    // ending at the foot of its start and at `w`.
    let mut g = floor(10.0);
    g.vert(B, 0, [2.0, 0.0, 0.0])
        .vert(B, 1, [12.0, 0.0, 0.0])
        .edge(B, 0, 0, 1);
    let (_, s) = decide(&g, &[EA, va(1), EB0], &related).unwrap();
    let p = pair(&s, 0);
    assert!(p.cells.is_empty() && p.crossings.is_empty());
    assert_eq!(p.pieces.len(), 1);
    let piece = &p.pieces[0];
    assert!(piece.by_form);
    assert_eq!(piece.span, [rat(0.0), Q::from_unsigneds(4u32, 5)]);
    assert_eq!(
        piece.ends[0].moved.to,
        Placement::Edge {
            edge: EdgeId(0),
            s: 0.2
        }
    );
    assert_eq!(piece.ends[1].moved.to, Placement::Vert(A, VertId(1)));
}

#[test]
fn a_reversed_edge_numbers_its_ends_from_its_lesser_end() {
    // The touch's first edge stored from the vertex on `e`: the same
    // piece, its ends named in geometric order.
    let mut g = floor(10.0);
    g.vert(B, 0, [1.0, 2e-3, 1e-4])
        .vert(B, 1, [5.0, 1e-4, 1e-4])
        .edge(B, 0, 1, 0);
    let (_, s) = decide(&g, &[EA, EB0, vb(1)], &unrelated).unwrap();
    let piece = &pair(&s, 0).pieces[0];
    assert_eq!(piece.span, [rat(0.0), rat(0.125)]);
    assert_eq!(piece.ends[0].which, EndKind::End);
    assert_eq!(piece.ends[0].moved.to, Placement::Vert(B, VertId(1)));
    assert_eq!(piece.ends[1].which, EndKind::Start);
}

#[test]
fn a_meeting_the_corner_rule_does_not_share_ends_at_the_depth_cap() {
    // An `S` edge whose image passes exactly through `e`'s end `w` while
    // its pre-snap points stay `10⁻³` from it (beyond `R_on`, so `w` is
    // not shared): no class certifies at the meeting, and the cells stop
    // at the depth limit, out of budget, never by a distance or a halving.
    let mut g = floor(5.0);
    g.vert(B, 0, [5.0, -1.0, 1e-3])
        .vert(B, 1, [5.0, 1.0, 1e-3])
        .edge(B, 0, 0, 1);
    let r = decide(&g, &[EA, va(1), EB0], &unrelated);
    assert!(
        matches!(
            r,
            Err(AlongError::OutOfBudget(KernelError::OutOfBudget { .. }))
        ),
        "{r:?}"
    );
}

#[test]
fn the_outcome_does_not_depend_on_ids() {
    // The touch with its edges and vertices numbered the other way: the
    // same pieces, by geometry.
    let mut g = floor(10.0);
    g.vert(B, 2, [1.0, 2e-3, 1e-4])
        .vert(B, 1, [5.0, 1e-4, 1e-4])
        .vert(B, 0, [9.0, 2e-3, 1e-4])
        .edge(B, 1, 2, 1)
        .edge(B, 0, 1, 0);
    let (_, s) = decide(&g, &[EA, EB0, EB1, vb(1)], &unrelated).unwrap();
    assert_eq!(pair(&s, 1).pieces[0].span, [rat(0.875), rat(1.0)]);
    assert_eq!(pair(&s, 0).pieces[0].span, [rat(0.0), rat(0.125)]);
    // Pairs in the order of their edges' keys: the lesser edge first.
    assert_eq!(s.pairs[0].h, EdgeId(1));
}
