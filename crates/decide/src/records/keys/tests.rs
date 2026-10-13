//! One key per fact: a point–vertex key and a cut point's crossing key are
//! the same whichever operand's point asks, the `B` reader told it asked
//! from the other side.

use super::*;

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
