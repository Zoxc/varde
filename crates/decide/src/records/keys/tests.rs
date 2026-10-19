//! One key per fact: a point–vertex key and a cut point's crossing key are
//! the same whichever operand's point asks, the `B` reader told it asked
//! from the other side. A key names its record's operand entities, which
//! is how evidence names it, and holds `NONE` exactly when one of them
//! is `NONE`.

use super::*;
use crate::bezier::Dyadic;
use crate::ids::{CoedgeId, FaceId, NONE};

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

fn named(operand: Operand, entity: impl Into<Entity>) -> Named {
    Named {
        operand,
        entity: entity.into(),
    }
}

/// Each key names the entities it holds with their operands: a cut point
/// its edge and the other operand's edge it was cut at, a `B` reader's
/// point on `B`, a hit its face and ribbon coedge on the face's operand,
/// an anchor its own; each entity once, places on them (ordinals, spans,
/// tiles) not at all.
#[test]
fn keys_name_their_operand_entities() {
    use Operand::{A, B};
    let (v, w) = (VertId(5), VertId(2));
    let cut_b = Point {
        operand: B,
        q: Query::Cut(G, E, C),
    };
    let names = |k: RecordKey| k.entities();
    let vv = VvKey::of(Point::vert(A, v), w).key;
    assert_eq!(names(vv.record()), [named(A, v), named(B, w)]);
    // A cut point of `B` on `G` at `A`'s `E`, against `A`'s vertex.
    let vv = VvKey::of(cut_b, v).key;
    assert_eq!(names(vv.into()), [named(A, v), named(B, G), named(A, E)]);
    // The ray of that cut point against `E` itself: `E` named once.
    let ray = RayKey { q: cut_b, g: E };
    assert_eq!(names(ray.record()), [named(B, G), named(A, E)]);
    let span = SpanKey {
        q: Point::vert(B, w),
        g: E,
        span: 3,
    };
    assert_eq!(names(span.record()), [named(B, w), named(A, E)]);
    assert_eq!(
        names(XKey { e: E, g: G }.record()),
        [named(A, E), named(B, G)]
    );
    let hit = HKey::Hit {
        q: Point::vert(A, v),
        f: FaceId(7),
        sheet: Sheet::Ribbon(CoedgeId(11), 0),
        i: HitIx(2),
    };
    let at = HeightKey {
        h: hit,
        anchor: Anchor::Face(B, FaceId(7)),
    };
    assert_eq!(
        names(at.record()),
        [named(A, v), named(B, FaceId(7)), named(B, CoedgeId(11))]
    );
    let corner = CornerKey {
        q: Point::vert(B, w),
        corner: TilePoint {
            operand: A,
            face: FaceId(3),
            tile: [-1, 4],
            at: [Dyadic::ZERO; 2],
        },
    };
    assert_eq!(names(corner.record()), [named(B, w), named(A, FaceId(3))]);
    let tie = XTieKey {
        x: XKey { e: E, g: G },
        e: EdgeAt {
            span: 0,
            at: Dyadic::ZERO,
        },
        g: EdgeAt {
            span: 1,
            at: Dyadic::ZERO,
        },
    };
    assert_eq!(names(tie.record()), [named(A, E), named(B, G)]);
}

/// A key holds `NONE` when an entity it names is `NONE`, never for an
/// ordinal at `u32::MAX`.
#[test]
fn a_key_holds_none_only_for_an_entity() {
    let q = Point::vert(Operand::A, VertId(NONE - 1));
    assert!(!RayKey { q, g: E }.record().holds_none());
    assert!(RayKey { q, g: EdgeId::NONE }.record().holds_none());
    assert!(
        RayKey {
            q: Point::vert(Operand::B, VertId::NONE),
            g: E
        }
        .record()
        .holds_none()
    );
    let cut = |e, c| Point {
        operand: Operand::A,
        q: Query::Cut(e, G, c),
    };
    assert!(
        !VvKey::of(cut(E, XClusterIx(NONE)), VertId(0))
            .key
            .record()
            .holds_none()
    );
    assert!(
        VvKey::of(cut(EdgeId::NONE, C), VertId(0))
            .key
            .record()
            .holds_none()
    );
    let x = HKey::X {
        e: E,
        g: G,
        c: XClusterIx(NONE),
    };
    assert!(!x.record().holds_none());
    let at = HeightKey {
        h: x,
        anchor: Anchor::Edge(Operand::B, EdgeId::NONE),
    };
    assert!(at.record().holds_none());
}
