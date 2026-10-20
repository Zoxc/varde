use std::collections::BTreeMap;

use varde_decide::bezier::HomPatch;
use varde_decide::count::boxes::{BoxKey, UvSpan};
use varde_decide::order::{EdgeKey, EntityOrder, VertKey};

use super::super::cluster::tests::{B_K, ext, quad, whole, work};
use super::super::cluster::{ClusterLeaf, Tangency};
use super::super::rim::{RimTrim, SnappedFace, rim};
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

/// A rim with only a window: the corner rule's vertex decisions read
/// nothing else.
fn window_rim(window: &[Element]) -> Rim {
    let mut window = window.to_vec();
    window.sort();
    Rim {
        pieces: Vec::new(),
        loops: Vec::new(),
        gaps: Vec::new(),
        clusters: Vec::new(),
        branches: Vec::new(),
        window,
        bounds: Vec::new(),
    }
}

fn rim_band(_: Along) -> f64 {
    1e-3
}

fn no_rim_input() -> RimInput<'static> {
    RimInput {
        snapped: B,
        faces: &[],
        targets: &[],
        band: &rim_band,
    }
}

/// Every element's band: `1e-4` mm, so `R_on = b_K` throughout.
fn narrow(_: Element) -> f64 {
    1e-4
}

fn input<'a>(geom: &'a Geom, band: &'a (dyn Fn(Element) -> f64 + Sync)) -> CornerInput<'a, Geom> {
    CornerInput {
        b_k: B_K,
        order: geom,
        band,
        candidates: &[],
        fixed: &[],
        groups: &[],
        coincident: &[],
    }
}

fn placement(c: &Corner, v: u32) -> Moved {
    c.placed
        .iter()
        .find(|p| p.vert == VertId(v))
        .unwrap_or_else(|| panic!("vertex {v} placed: {c:?}"))
        .moved
}

/// The target `A`: the floor's edge along `y = 5` split at `w = (5, 5, 0)`.
fn floor_edge() -> Geom {
    let mut g = Geom::default();
    g.vert(A, 0, [0.0, 5.0, 0.0])
        .vert(A, 1, [5.0, 5.0, 0.0])
        .vert(A, 2, [10.0, 5.0, 0.0])
        .edge(A, 0, 0, 1)
        .edge(A, 1, 1, 2);
    g
}

#[test]
fn a_snapped_vertex_goes_to_the_target_vertex_or_edge_it_is_on() {
    // The flat analogue of a pole on a seam: `S` vertices `1e-4` above the
    // floor next to its edge, placed on the vertex or the edge's foot,
    // else left where they are; no distance escalates.
    let mut g = floor_edge();
    g.vert(B, 0, [2.0, 5.0 + 1e-4, 1e-4])
        .vert(B, 1, [5.0 + 2e-4, 5.0, 1e-4])
        .vert(B, 2, [8.0, 5.01, 1e-4]);
    let window: Vec<Element> = vec![
        Element::Vert(A, VertId(1)),
        Element::Edge(A, EdgeId(0)),
        Element::Edge(A, EdgeId(1)),
        Element::Vert(B, VertId(0)),
        Element::Vert(B, VertId(1)),
        Element::Vert(B, VertId(2)),
    ];
    let r = window_rim(&window);
    let c = corner(&r, &no_rim_input(), &input(&g, &narrow), &mut work()).unwrap();
    let m0 = placement(&c, 0);
    assert_eq!(
        m0.to,
        Placement::Edge {
            edge: EdgeId(0),
            s: 0.4
        }
    );
    assert_eq!(m0.pos, Some([2.0, 5.0, 0.0]));
    // Moved by `√2·10⁻⁴`, within `R_on = b_K`.
    assert!(m0.displacement > 1.4e-4 && m0.displacement <= B_K);
    let m1 = placement(&c, 1);
    assert_eq!(m1.to, Placement::Vert(A, VertId(1)));
    assert_eq!(m1.pos, Some([5.0, 5.0, 0.0]));
    assert!(m1.displacement <= B_K && m1.storage == 0.0);
    assert_eq!(placement(&c, 2), Moved::STAYS);
    assert!(c.splits.is_empty() && c.rim_verts.is_empty() && c.spans.is_empty());
}

#[test]
fn on_holds_up_to_r_on_exactly_and_off_just_past_it() {
    // `b_K = 2⁻¹¹` and bands zero: `R_on = b_K`. A vertex exactly `b_K`
    // from the target vertex is on it; one a hair farther is off (above
    // `R_off = b_K/2`), certified at once.
    let b_k = 1.0 / 2048.0;
    let zero = |_: Element| 0.0;
    for (d, on) in [
        (b_k, true),
        (b_k * (1.0 + 1.0 / (1u64 << 40) as f64), false),
    ] {
        let mut g = Geom::default();
        g.vert(A, 0, [1.0, 1.0, 0.0]).vert(B, 0, [1.0, 1.0, d]);
        let r = window_rim(&[Element::Vert(A, VertId(0)), Element::Vert(B, VertId(0))]);
        let mut i = input(&g, &zero);
        i.b_k = b_k;
        let c = corner(&r, &no_rim_input(), &i, &mut work()).unwrap();
        let want = if on {
            Placement::Vert(A, VertId(0))
        } else {
            Placement::Stays
        };
        assert_eq!(placement(&c, 0).to, want, "{d}");
    }
}

#[test]
fn a_wide_band_widens_r_on_to_the_sum_of_the_bands() {
    // An inherited wide target edge (band `10⁻³`): `R_on = β_e + β_v`, so a
    // vertex `9·10⁻⁴` off it is on it; at the default bands it is off.
    let mut g = floor_edge();
    g.vert(B, 0, [2.0, 5.0 + 9e-4, 0.0]);
    let r = window_rim(&[Element::Edge(A, EdgeId(0)), Element::Vert(B, VertId(0))]);
    let wide = |e: Element| match e {
        Element::Edge(Operand::A, _) => 1e-3,
        _ => 1e-4,
    };
    let c = corner(&r, &no_rim_input(), &input(&g, &wide), &mut work()).unwrap();
    let m = placement(&c, 0);
    assert!(matches!(
        m.to,
        Placement::Edge {
            edge: EdgeId(0),
            ..
        }
    ));
    assert!(m.displacement <= up(1e-3 + 1e-4) && m.displacement > B_K);
    let c = corner(&r, &no_rim_input(), &input(&g, &narrow), &mut work()).unwrap();
    assert_eq!(placement(&c, 0).to, Placement::Stays);
}

#[test]
fn the_first_target_vertex_in_geometric_order_wins_whatever_the_ids() {
    // Two target vertices `2·10⁻⁴` apart, the `S` vertex on both: it goes
    // to the lesser by position, under either numbering. They aren't
    // identified with each other.
    for swap in [false, true] {
        let (lo, hi) = if swap { (7, 3) } else { (3, 7) };
        let mut g = Geom::default();
        g.vert(A, lo, [4.0, 4.0, 0.0])
            .vert(A, hi, [4.0 + 2e-4, 4.0, 0.0])
            .vert(B, 0, [4.0 + 1e-4, 4.0, 1e-4]);
        let r = window_rim(&[
            Element::Vert(A, VertId(3)),
            Element::Vert(A, VertId(7)),
            Element::Vert(B, VertId(0)),
        ]);
        let c = corner(&r, &no_rim_input(), &input(&g, &narrow), &mut work()).unwrap();
        assert_eq!(placement(&c, 0).to, Placement::Vert(A, VertId(lo)));
    }
}

#[test]
fn the_window_takes_snapped_elements_within_r_on_of_a_target_element() {
    // A vertex and an edge of `S` outside the rim but on the target edge
    // join the window (and the vertex is placed); farther ones don't.
    let mut g = floor_edge();
    g.vert(B, 0, [1.0, 5.0 + 3e-4, 1e-4])
        .vert(B, 1, [1.0, 6.0, 1e-4])
        .vert(B, 2, [3.0, 5.0 - 2e-4, 0.0])
        .vert(B, 3, [3.0, 7.0, 0.0])
        .edge(B, 0, 1, 3)
        .edge(B, 1, 2, 3);
    let r = window_rim(&[Element::Edge(A, EdgeId(0))]);
    let cands = [
        Element::Vert(B, VertId(0)),
        Element::Vert(B, VertId(1)),
        Element::Edge(B, EdgeId(0)),
        Element::Edge(B, EdgeId(1)),
    ];
    let mut i = input(&g, &narrow);
    i.candidates = &cands;
    let c = corner(&r, &no_rim_input(), &i, &mut work()).unwrap();
    assert_eq!(
        c.window,
        [
            Element::Edge(A, EdgeId(0)),
            Element::Edge(B, EdgeId(1)),
            Element::Vert(B, VertId(0)),
        ]
    );
    assert!(matches!(
        placement(&c, 0).to,
        Placement::Edge {
            edge: EdgeId(0),
            ..
        }
    ));
}

#[test]
fn a_target_vertex_on_a_snapped_edge_splits_it_named_by_the_pair() {
    // The flat analogue of a plate vertex under a rod's cap circle: two
    // target vertices on `S` edge `h`, which is split at each, in order
    // from its geometrically lesser end whichever way it is stored; a
    // target vertex at an end placed on it splits nothing.
    for reversed in [false, true] {
        let mut g = Geom::default();
        g.vert(B, 0, [0.0, 0.0, 1e-4])
            .vert(B, 1, [10.0, 0.0, 1e-4])
            .vert(A, 0, [4.0, 1e-4, 0.0])
            .vert(A, 1, [6.0, -1e-4, 0.0])
            .vert(A, 2, [10.0, 1e-4, 0.0]);
        if reversed {
            g.edge(B, 0, 1, 0);
        } else {
            g.edge(B, 0, 0, 1);
        }
        let r = window_rim(&[
            Element::Edge(B, EdgeId(0)),
            Element::Vert(B, VertId(0)),
            Element::Vert(B, VertId(1)),
            Element::Vert(A, VertId(0)),
            Element::Vert(A, VertId(1)),
            Element::Vert(A, VertId(2)),
        ]);
        let c = corner(&r, &no_rim_input(), &input(&g, &narrow), &mut work()).unwrap();
        assert_eq!(placement(&c, 1).to, Placement::Vert(A, VertId(2)));
        let got: Vec<(EdgeId, (Operand, VertId), f64)> =
            c.splits.iter().map(|s| (s.edge, s.at, s.s)).collect();
        let (s0, s1) = if reversed { (0.6, 0.4) } else { (0.4, 0.6) };
        assert_eq!(
            got,
            [
                (EdgeId(0), (A, VertId(0)), s0),
                (EdgeId(0), (A, VertId(1)), s1)
            ]
        );
        for s in &c.splits {
            assert_eq!(s.pos, g.verts[&s.at]);
            assert!(s.displacement <= B_K);
        }
    }
}

#[test]
fn a_vertex_group_is_placed_as_one_by_its_first_vertex() {
    // A group of two `S` vertices: the lesser is on the target vertex,
    // the other (off it) goes with it.
    let mut g = floor_edge();
    g.vert(B, 0, [5.0, 5.0, 1e-4])
        .vert(B, 1, [5.0, 5.0 + 4e-3, 1e-4]);
    let r = window_rim(&[
        Element::Vert(A, VertId(1)),
        Element::Vert(B, VertId(0)),
        Element::Vert(B, VertId(1)),
    ]);
    let group: [VertId; 2] = [VertId(1), VertId(0)];
    let groups: [&[VertId]; 1] = [&group];
    let mut i = input(&g, &narrow);
    i.groups = &groups;
    let c = corner(&r, &no_rim_input(), &i, &mut work()).unwrap();
    for v in [0, 1] {
        assert_eq!(placement(&c, v).to, Placement::Vert(A, VertId(1)));
    }
    assert!(placement(&c, 1).displacement >= 4e-3);
}

// ---------------------------------------------------------------------
// With a rim.

/// A leaf of two given boxes, Near.
struct Boxes([BoxKey; 2]);

impl ClusterLeaf for Boxes {
    fn boxes(&self) -> [BoxKey; 2] {
        self.0
    }

    fn world(&self) -> [[[f64; 3]; 2]; 2] {
        [[[0.0; 3], [1.0; 3]]; 2]
    }

    fn tangency(&self) -> Option<Tangency> {
        Some(Tangency::Near)
    }
}

const FLOOR: Implicit = Implicit::Plane {
    normal: [0.0, 0.0, 1.0],
    offset: 0.0,
};

/// The straight span from `a` to `b`, weights 1.
fn span(a: [f64; 2], b: [f64; 2]) -> UvSpan {
    let m = [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1])];
    [[a[0], a[1], 1.0], [m[0], m[1], 1.0], [b[0], b[1], 1.0]]
}

const KNOTS: [f64; 2] = [0.0, 1.0];

/// The box `[a, b]²/4` of face 0 of `S`, against the floor's tile.
fn square(a: u64, b: u64) -> Vec<Boxes> {
    let s = BoxKey::new(B, FaceId(0), [0, 0], ext(a, b, 2), ext(a, b, 2));
    vec![Boxes([whole(A, 0, [0, 0]), s])]
}

/// The tilted sheet `z = c·(x − x0)` over `[0, 10]²` (`u` along `x`).
fn tilted(c: f64, x0: f64) -> HomPatch<Rational> {
    quad(
        [0.0, 0.0, -c * x0],
        [10.0, 0.0, c * (10.0 - x0)],
        [0.0, 10.0, -c * x0],
    )
}

#[test]
fn a_root_cluster_within_r_on_of_a_target_vertex_is_carried_by_it() {
    // The sheet crosses the floor along `x = 5`: on the square's bottom
    // and top sides the gap changes sign at `x = 5`, a branch vertex each.
    // A target vertex `10⁻⁴` from the bottom one carries that cluster, of
    // net +1 (inside to outside along the rim): it is the branch vertex;
    // the top one stays the rim's.
    let tiles = [([0, 0], tilted(1e-5, 5.0))];
    let faces = [SnappedFace {
        face: FaceId(0),
        tiles: &tiles,
        own: &[],
        target: &[],
        flipped: false,
    }];
    let ri = RimInput {
        snapped: B,
        faces: &faces,
        targets: &[(FaceId(0), FLOOR)],
        band: &rim_band,
    };
    let mut r = rim(&square(1, 3), &ri, &mut work()).unwrap();
    assert_eq!(r.branches.len(), 2, "{:?}", r.branches);
    let (bottom, top) = (r.branches[0].clone(), r.branches[1].clone());
    assert_eq!((bottom.piece, top.piece), (0, 2));
    let mut g = Geom::default();
    g.vert(A, 0, [5.0, 2.5 + 1e-4, 0.0]);
    r.window.push(Element::Vert(A, VertId(0)));
    let c = corner(&r, &ri, &input(&g, &narrow), &mut work()).unwrap();
    assert_eq!(c.spans.len(), 1, "{:?}", c.spans);
    let s = &c.spans[0];
    assert_eq!(s.anchor, (A, VertId(0)));
    assert_eq!(s.net, Some(1));
    assert_eq!((s.from.0, s.to.0), (0, 0));
    // Within `R_on` (here `β_w + β_rim`) of the anchor along the side.
    let x = |t: Dyadic| 2.5 + 5.0 * t.to_f64().unwrap();
    assert!(x(s.from.1) < 5.0 && 5.0 < x(s.to.1), "{s:?}");
    assert!(x(s.from.1) > 5.0 - 2e-3 && x(s.to.1) < 5.0 + 2e-3, "{s:?}");
    assert_eq!(c.branches, [top]);
    assert!(c.clusters.len() == r.clusters.len() - 1);
    // Far from the anchor, nothing is carried.
    let mut g = Geom::default();
    g.vert(A, 0, [6.0, 2.5 + 1e-4, 0.0]);
    let c = corner(&r, &ri, &input(&g, &narrow), &mut work()).unwrap();
    assert_eq!(c.spans.len(), 1);
    assert_eq!(c.spans[0].net, Some(0));
    assert_eq!(c.branches.len(), 2);
}

/// The tilted sheet's rim on `square(1, 3)` against the floor.
fn tilted_input<'a>(faces: &'a [SnappedFace<'a>]) -> RimInput<'a> {
    RimInput {
        snapped: B,
        faces,
        targets: &[(FaceId(0), FLOOR)],
        band: &rim_band,
    }
}

#[test]
fn two_anchors_closer_than_twice_r_on_cut_their_spans_or_the_first_carries_both() {
    // Target vertices either side of `x = 5` on the bottom side, `R_on =
    // 1.1·10⁻³`. `8·10⁻⁴` off with the root at `x = 5`: the cells each is
    // on meet only across the root cluster, so their spans overlap there
    // alone: no cut, the first in geometric order (the lesser `x`,
    // whatever its id) carries it, net +1, the other nothing. `2·10⁻⁴` off
    // with the root at `x = 5.0004`: the overlap reads negative left of
    // the cluster, cut at its first cell, the first carries net 0 and the
    // second +1.
    for (off, x0, cut) in [(8e-4, 5.0, false), (2e-4, 5.0004, true)] {
        let tiles = [([0, 0], tilted(1e-5, x0))];
        let faces = [SnappedFace {
            face: FaceId(0),
            tiles: &tiles,
            own: &[],
            target: &[],
            flipped: false,
        }];
        let ri = tilted_input(&faces);
        let mut r = rim(&square(1, 3), &ri, &mut work()).unwrap();
        let mut g = Geom::default();
        g.vert(A, 5, [5.0 - off, 2.5, 0.0])
            .vert(A, 2, [5.0 + off, 2.5, 0.0]);
        r.window.push(Element::Vert(A, VertId(2)));
        r.window.push(Element::Vert(A, VertId(5)));
        r.window.sort();
        let c = corner(&r, &ri, &input(&g, &narrow), &mut work()).unwrap();
        let got: Vec<((Operand, VertId), Option<i8>)> =
            c.spans.iter().map(|s| (s.anchor, s.net)).collect();
        if cut {
            assert_eq!(got, [((A, VertId(5)), Some(0)), ((A, VertId(2)), Some(1))]);
            let (a, b) = (&c.spans[0], &c.spans[1]);
            assert!(a.to.0 == b.from.0 && a.to.1 < b.from.1, "{:?}", c.spans);
        } else {
            assert_eq!(got, [((A, VertId(5)), Some(1))]);
        }
        // The bottom branch is absorbed either way; the top one stays.
        assert_eq!(c.branches.len(), 1);
        assert_eq!(c.branches[0].piece, 2);
    }
}

#[test]
fn rim_vertices_join_the_first_anchor_they_are_on_never_each_other() {
    // The sheet crosses the floor at `x = 2.5002`, beside the square's
    // left corners: the branch vertices sit `2·10⁻⁴` from them (bottom
    // and top sides). The bottom-left corner joins the bottom branch; the
    // bottom-right corner joins a target vertex `10⁻⁴` off it; the
    // top-right corner stays.
    let tiles = [([0, 0], tilted(1e-5, 2.5002))];
    let faces = [SnappedFace {
        face: FaceId(0),
        tiles: &tiles,
        own: &[],
        target: &[],
        flipped: false,
    }];
    let ri = tilted_input(&faces);
    let mut r = rim(&square(1, 3), &ri, &mut work()).unwrap();
    assert_eq!(r.branches.len(), 2, "{:?}", r.branches);
    let mut g = Geom::default();
    g.vert(A, 0, [7.5 + 1e-4, 2.5, 0.0]);
    r.window.push(Element::Vert(A, VertId(0)));
    let c = corner(&r, &ri, &input(&g, &narrow), &mut work()).unwrap();
    assert_eq!(c.branches.len(), 2);
    let on_bottom = c.branches.iter().position(|b| b.piece == 0).unwrap() as u32;
    let to: Vec<Placement> = c.rim_verts.iter().map(|v| v.moved.to).collect();
    assert_eq!(
        to,
        [
            Placement::Branch(on_bottom),
            Placement::Vert(A, VertId(0)),
            Placement::Stays,
            Placement::Branch(1 - on_bottom),
        ]
    );
    assert!(c.rim_verts.iter().all(|v| v.kind == RimVertKind::Box));
    let m = c.rim_verts[1].moved;
    assert_eq!(m.pos, Some([7.5 + 1e-4, 2.5, 0.0]));
    assert!(m.displacement <= up(1e-4 + 1e-3) && m.displacement >= 1e-4);
    // The anchor's span at the corner reads positive on both sides.
    assert_eq!(c.spans.len(), 1);
    assert_eq!(c.spans[0].net, Some(0));
}

/// Two faces of `S` meeting along coincident edges `2` (face 0's top)
/// and `4` (face 1's bottom), `10⁻⁴` above the floor.
struct Crack {
    tiles: [[([i64; 2], HomPatch<Rational>); 1]; 2],
    spans: Vec<UvSpan>,
    geom: Geom,
}

fn crack() -> Crack {
    let z = 1e-4;
    let tiles = [
        [([0, 0], quad([0.0, 0.0, z], [10.0, 0.0, z], [0.0, 5.0, z]))],
        [([0, 0], quad([0.0, 5.0, z], [10.0, 5.0, z], [0.0, 10.0, z]))],
    ];
    let c = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let spans: Vec<UvSpan> = (0..8).map(|k| span(c[k % 4], c[(k + 1) % 4])).collect();
    let mut geom = Geom::default();
    let pts = [[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [0.0, 5.0]];
    for f in 0..2u32 {
        for k in 0..4u32 {
            let p = pts[k as usize];
            geom.vert(B, 4 * f + k, [p[0], p[1] + 5.0 * f as f64, z]);
            geom.edge(B, 4 * f + k, 4 * f + k, 4 * f + (k + 1) % 4);
        }
    }
    Crack { tiles, spans, geom }
}

fn crack_trims(c: &Crack, f: u32) -> Vec<RimTrim<'_>> {
    (0..4u32)
        .map(|k| {
            let e = 4 * f + k;
            RimTrim {
                edge: EdgeId(e),
                ends: [VertId(e), VertId(4 * f + (k + 1) % 4)],
                spans: &c.spans[k as usize..k as usize + 1],
                knots: &KNOTS,
                left: true,
            }
        })
        .collect()
}

#[test]
fn crossings_of_coincident_edges_are_one_point_joined_by_a_zero_length_edge() {
    // The flat analogue of a slit at a rim: the cluster's box spans the
    // crack, so the rim runs along edge 2 on face 0 and edge 4 on face 1,
    // its vertices on them at `x = 2.5` and `x = 7.5` pairwise at one
    // point. Each pair is one vertex (the second with the first, bitwise)
    // and a zero-length rim edge; with a target vertex on one pair, both
    // go to it.
    let c = crack();
    let trims = [crack_trims(&c, 0), crack_trims(&c, 1)];
    let faces: Vec<SnappedFace<'_>> = (0..2)
        .map(|f| SnappedFace {
            face: FaceId(f as u32),
            tiles: &c.tiles[f],
            own: &trims[f],
            target: &[],
            flipped: false,
        })
        .collect();
    let ri = RimInput {
        snapped: B,
        faces: &faces,
        targets: &[(FaceId(0), FLOOR)],
        band: &rim_band,
    };
    let t = whole(A, 0, [0, 0]);
    let leaves = [
        Boxes([
            t,
            BoxKey::new(B, FaceId(0), [0, 0], ext(1, 3, 2), ext(1, 2, 1)),
        ]),
        Boxes([
            t,
            BoxKey::new(B, FaceId(1), [0, 0], ext(1, 3, 2), ext(0, 1, 1)),
        ]),
    ];
    let mut r = rim(&leaves, &ri, &mut work()).unwrap();
    assert_eq!(r.loops.len(), 2);
    let class: [EdgeId; 2] = [EdgeId(2), EdgeId(4)];
    let classes: [&[EdgeId]; 1] = [&class];
    let mut i = input(&c.geom, &narrow);
    i.coincident = &classes;
    let k = corner(&r, &ri, &i, &mut work()).unwrap();
    let on = |e: u32| -> Vec<u32> {
        (0..k.rim_verts.len() as u32)
            .filter(|&j| k.rim_verts[j as usize].kind == RimVertKind::OnEdge(B, EdgeId(e)))
            .collect()
    };
    let (top, bottom) = (on(2), on(4));
    assert_eq!((top.len(), bottom.len()), (2, 2), "{:?}", k.rim_verts);
    assert_eq!(k.zero_edges.len(), 2, "{:?}", k.zero_edges);
    for [l, f] in &k.zero_edges {
        assert_eq!(k.rim_verts[*l as usize].moved.to, Placement::Stays);
        assert_eq!(k.rim_verts[*f as usize].moved.to, Placement::With(*l));
        assert_eq!(k.rim_verts[*f as usize].moved.displacement, 0.0);
        // One crossing of each edge.
        let edges = [*l, *f].map(|j| k.rim_verts[j as usize].kind);
        assert_ne!(edges[0], edges[1]);
    }
    // Without the class, nothing is joined.
    let k2 = corner(&r, &ri, &input(&c.geom, &narrow), &mut work()).unwrap();
    assert!(k2.zero_edges.is_empty());
    assert!(k2.rim_verts.iter().all(|v| v.moved.to == Placement::Stays));
    // A target vertex on the pair at `x = 2.5`: both go to it.
    let mut g = crack().geom;
    g.vert(A, 0, [2.5, 5.0 + 1e-4, 0.0]);
    r.window.push(Element::Vert(A, VertId(0)));
    let mut i = input(&g, &narrow);
    i.coincident = &classes;
    let k = corner(&r, &ri, &i, &mut work()).unwrap();
    let at: Vec<u32> = (0..k.rim_verts.len() as u32)
        .filter(|&j| k.rim_verts[j as usize].moved.to == Placement::Vert(A, VertId(0)))
        .collect();
    assert_eq!(at.len(), 2, "{:?}", k.rim_verts);
    assert_eq!(k.zero_edges.len(), 2);
    assert!(
        at.iter()
            .all(|&j| k.zero_edges.iter().any(|z| z.contains(&j)))
    );
}
