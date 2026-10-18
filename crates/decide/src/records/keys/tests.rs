//! One key per fact: a point–vertex key and a cut point's crossing key are
//! the same whichever operand's point asks, the `B` reader told it asked
//! from the other side.

use super::*;
use crate::ids::FaceId;

const E: EdgeId = EdgeId(4);
const G: EdgeId = EdgeId(9);
const C: XClusterIx = XClusterIx(1);

#[test]
fn a_point_pair_has_one_key_whichever_point_asks() {
    let (v, w) = (VertId(5), VertId(2));
    let (a, b) = (Query::Vert(v), Query::Vert(w));
    let from_a = VvKey::of(Point::vert(Operand::A, v), w);
    let from_b = VvKey::of(Point::vert(Operand::B, w), v);
    assert_eq!(from_a.key, VvKey { a, b });
    assert_eq!(from_a.key, from_b.key);
    assert!(!from_a.swapped && from_b.swapped);

    let ha = HKey::vv(Point::vert(Operand::A, v), w);
    let hb = HKey::vv(Point::vert(Operand::B, w), v);
    assert_eq!(ha.key, HKey::VV { a, b });
    assert_eq!(ha.key, hb.key);
    assert!(!ha.swapped && hb.swapped);

    // A cut point of `B` against a vertex of `A`: the vertex leads.
    let cut = Query::Cut(G, E, C);
    let from_cut = HKey::vv(
        Point {
            operand: Operand::B,
            q: cut,
        },
        v,
    );
    assert_eq!(from_cut.key, HKey::VV { a, b: cut });
    assert!(from_cut.swapped);
}

#[test]
fn a_cut_point_against_its_own_edge_reads_the_crossing() {
    let x = HKey::X { e: E, g: G, c: C };
    let on_e = Point {
        operand: Operand::A,
        q: Query::Cut(E, G, C),
    };
    let on_g = Point {
        operand: Operand::B,
        q: Query::Cut(G, E, C),
    };
    assert_eq!(
        HKey::x(on_e, G),
        Some(Oriented {
            key: x,
            swapped: false
        })
    );
    assert_eq!(
        HKey::x(on_g, E),
        Some(Oriented {
            key: x,
            swapped: true
        })
    );
    // Another edge, or a vertex, isn't its crossing.
    assert_eq!(HKey::x(on_e, EdgeId(10)), None);
    assert_eq!(HKey::x(Point::vert(Operand::A, VertId(5)), G), None);
}

#[test]
fn operands_swap() {
    assert_eq!(Operand::A.other(), Operand::B);
    assert_eq!(Operand::B.other(), Operand::A);
}

/// A dyadic point of a face has one key whichever tile or line reaches
/// it: a coordinate at a tile's 1 is the next tile's 0, and a line's
/// point is the same as the crossing line's there.
#[test]
fn a_tile_point_has_one_key() {
    let (face, d) = (FaceId(2), |n, e| Dyadic::new(n, e).unwrap());
    let p = |tile, at| TilePoint::new(Operand::B, face, tile, at).unwrap();
    // The corner (1, 1) from each of the four tiles meeting there.
    let c = p([1, 1], [Dyadic::ZERO; 2]);
    assert_eq!(p([0, 0], [Dyadic::ONE; 2]), c);
    assert_eq!(p([0, 1], [Dyadic::ONE, Dyadic::ZERO]), c);
    assert_eq!(p([1, 0], [Dyadic::ZERO, Dyadic::ONE]), c);
    assert_eq!(
        c.corner(),
        Some(TileCorner {
            operand: Operand::B,
            face,
            at: [1, 1]
        })
    );
    // A point inside the tile edge `u = 1` over the cell `v ∈ [0, 1]`.
    let e = p([0, 0], [Dyadic::ONE, d(1, 2)]);
    let (edge, t) = e.on_edge().unwrap();
    assert_eq!(
        (edge.fixed, edge.line, edge.cell, t),
        (Dir::U, 1, 0, d(1, 2))
    );
    assert_eq!(e.corner(), None);
    let line = SideLine::of(Operand::B, face, [0, 0], Dir::U, Dyadic::ONE).unwrap();
    assert_eq!(line, SideLine::Edge(edge));
    assert_eq!(line.point(d(1, 2)), Some(e));
    // The inner line `v = ¼` of tile (0, 0) ends on the tile edges `u = 0`
    // and `u = 1`, and its point at `u = ½` lies inside the tile.
    let inner = SideLine::of(Operand::B, face, [0, 0], Dir::V, d(1, 2)).unwrap();
    let [start, end] = inner.ends().unwrap();
    assert_eq!(
        start.on_edge().map(|(e, _)| (e.fixed, e.line)),
        Some((Dir::U, 0))
    );
    assert_eq!(end, e);
    let mid = inner.point(d(1, 1)).unwrap();
    assert_eq!((mid.on_edge(), mid.corner()), (None, None));
    // Past the range of `i64`: no key.
    assert_eq!(
        TilePoint::new(Operand::B, face, [i64::MAX, 0], [Dyadic::ONE, Dyadic::ZERO]),
        None
    );
}
