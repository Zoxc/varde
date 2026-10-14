//! The side records: a point's ray against a box side whose shadow the
//! counting can't certify off the point's, decided once per canonical key
//! and read by every box with that side.
//!
//! **Why.** The counting reads a box's degree from the turns of its four
//! sides about the query point's shadow ([`degree`](crate::count::degree)),
//! which a side whose shadow passes through the point's (or too close
//! for the level) has none of. The splits move interior box sides off
//! the point; a tile line can't move (a box never straddles one), nor
//! can a silhouette or an overlapping shadow along it. Such a side is
//! counted instead under the perturbation, as a curved edge's span is
//! ([`super::ray`]): the signed crossings of its shadow by the point's
//! ray, the point moved, each crossing's sign `sign(ρ × T)` seen from
//! above, `ρ` the ray's actual direction (`−r̂` for a point of `A`, `+r̂`
//! for one of `B`), `T` the side's tangent in its increasing parameter.
//! Summed round a closed boundary these are its winding about the moved
//! point, so the degrees of boxes reading them stay additive.
//!
//! **Keys: one fact per tile edge** ([`SideKey`], [`TileEdge`]). Boxes on
//! the two sides of a tile line are refined independently, so their
//! sides along it don't match (T-junctions, other depths). The record is
//! therefore kept per *maximal* dyadic segment, the tile edge: the row of
//! the tile's stored control points along the line over one cell of the
//! other coordinate, a rational quadratic with exact `f64` data, keyed by
//! the line's global coordinate and the cell, so the tiles on both sides
//! read the one record. Its crossings carry their parameter enclosures
//! ([`Crossing::t`]); a box side over part of the edge counts those in
//! its part ([`SideRec::count_over`]), so every partition of the edge
//! counts each crossing once.
//!
//! **Corners, half-open** ([`CornerKey`], [`CornerRec`]). An edge's ends
//! are tile corners, stored control points shared bit for bit by the up
//! to four tiles and edges meeting there. Their side against the point's
//! `r̂` line and whether they are ahead along it are one record per
//! (point, corner), the flat point–vertex shadow row against the corner
//! moving with its face, by [`decide`](crate::num::decide::decide) on the
//! moved configuration: the edges read their ends from it (a ray through
//! a corner counted once along the line), and a box side certified off
//! the point but ending on its ray line at a corner takes that end's side
//! from it ([`box_degree_sided`](crate::count::degree::box_degree_sided)).
//!
//! **Deciding.** The edge's crossings are decided as a curved edge span's
//! ([`super::ray`], module docs there: roots of the shadow across the ray
//! line isolated, their place along it certified or decided by the
//! vertex–curve shadow row at the piece, here
//! [`Row::BoxSide`], the side's points
//! moving with their face), level by level until one decides. A side
//! inside a tile (an interior split the splits couldn't move off the
//! point) has no stored row and no key here.
//!
//! **Decided once.** The records live in two [`Table`]s owned by
//! [`Sides`]; their deciding functions run only inside [`Sides::fill`]:
//! one charged [`Pass`] for the corners of the edges asked, one for the
//! edges, reading those.

use smallvec::SmallVec;

use crate::num::decide::{Band, Fail, Row, Sign, TieKey};
use crate::num::exact::ORDERS;
use crate::perturb::{Curved, Moving, Rate};
use crate::records::keys::{CornerKey, Operand, Point, RecordKey, SideKey, TileCorner, TileEdge};
use crate::records::ray::{Crossing, Span, SpanRates, span_crossings};
use crate::records::shadow::{
    Cost, Decided, Pass, ShadowError, ShadowGeometry, ray_ahead, replay, shadow_error,
    side_and_ahead, table_error,
};
use crate::records::table::{Fresh, Table};

/// A point's ray against a tile edge of a face of the other operand
/// (module docs).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SideRec {
    /// The crossings, in increasing parameter on the edge's `[0, 1]`.
    pub crossings: SmallVec<[Crossing; 2]>,
}

impl SideRec {
    /// The crossings summed as `±1`s: the whole edge's count.
    pub fn count(&self) -> i64 {
        self.crossings
            .iter()
            .map(|c| i64::from(c.sign.as_i8()))
            .sum()
    }

    /// The count over the part `[lo, hi]` of the edge's parameter, a box
    /// side's (`0 ≤ lo < hi ≤ 1`): the crossings whose enclosures lie in
    /// it. A crossing at an end of the edge (`[0, 0]`, `[1, 1]`) is the
    /// part's reaching that end. `None` where a crossing's enclosure
    /// reaches a cut inside the edge (`lo` or `hi` in `(0, 1)`): which
    /// part it is in isn't known, and the box's corner there lies on or
    /// next to the point's ray line.
    pub fn count_over(&self, [lo, hi]: [f64; 2]) -> Option<i64> {
        let inner = |x: f64| x > 0.0 && x < 1.0;
        let mut n = 0;
        for c in &self.crossings {
            let [a, b] = c.t;
            if [lo, hi].into_iter().any(|x| inner(x) && a <= x && x <= b) {
                return None;
            }
            if lo <= a && b <= hi {
                n += i64::from(c.sign.as_i8());
            }
        }
        Some(n)
    }
}

/// A point against a tile corner of a face of the other operand, oriented
/// from the point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CornerRec {
    /// The side of the corner against the point's `r̂` line, `ν·(c − q)`:
    /// `Pos` to its left seen from above.
    pub side: Sign,
    /// Whether the corner is ahead of the point along `r̂`, `r̂·(c − q)`.
    pub ahead: Sign,
}

impl CornerRec {
    /// Whether the corner lies the way the ray of the point (of `of`)
    /// goes.
    pub fn on_ray(&self, of: Operand) -> bool {
        self.ahead == ray_ahead(of)
    }
}

/// The geometry the side records read, beyond the flat records'.
pub trait SideGeometry: ShadowGeometry {
    /// Tile edge `e`'s stored control points with their weights, from
    /// its first corner to its second ([`TileEdge::corners`]); the same
    /// row whichever tile beside it is asked. `None` if the face has no
    /// such edge.
    fn tile_edge(&self, e: &TileEdge) -> Option<Span>;

    /// Tile corner `c`'s stored point (the tile edges' end control point
    /// there) with its rates for `q`'s record: moving with its face on
    /// `A`, zero on `B`. `None` if the face has no such corner.
    fn corner(&self, q: Point, c: &TileCorner) -> Option<Moving>;

    /// The rates of tile edge `e`'s point at the parameter `t` (an
    /// enclosure at its level), for `q`'s record: moving with its face
    /// on `A`, zero on `B`.
    fn edge_rates<S: Curved>(&self, q: Point, e: &TileEdge, t: &S) -> [[S; 3]; ORDERS];
}

/// The side and corner records of a run, decided once per key (module
/// docs).
#[derive(Debug, Default)]
pub struct Sides {
    side: Table<SideKey, SideRec>,
    corner: Table<CornerKey, CornerRec>,
}

impl Sides {
    /// No records.
    pub fn new() -> Sides {
        Sides::default()
    }

    /// Decides the side records of `wanted` not yet stored: first the
    /// corner records of their ends, in one pass, then the edges, in
    /// another, at the curved band `band`. A key whose point is of the
    /// face's operand, whose edge or corner the geometry doesn't have, or
    /// whose edge's ends aren't its corners' points bit for bit fails its
    /// pass ([`Fail::Internal`]).
    pub fn fill<G, P>(
        &mut self,
        wanted: impl IntoIterator<Item = SideKey>,
        geom: &G,
        band: Band,
        pass: &mut P,
    ) -> Result<(), ShadowError<P::Error>>
    where
        G: SideGeometry + Sync,
        P: Pass,
    {
        let batch = self.side.missing(wanted);
        if batch.is_empty() {
            return Ok(());
        }
        let corners: Vec<CornerKey> = batch
            .keys()
            .iter()
            .flat_map(|k| {
                let k = *k.key();
                k.edge.corners().map(|corner| CornerKey { q: k.q, corner })
            })
            .collect();
        self.corner
            .fill(corners, |keys| {
                pass.run(keys, &|k: &Fresh<CornerKey>| decide_corner(k, geom))
            })
            .map_err(|e| shadow_error(e, RecordKey::Corner))?;
        let corner = &self.corner;
        let values = pass
            .run(batch.keys(), &|k: &Fresh<SideKey>| {
                decide_side(k, corner, geom, band)
            })
            .map_err(ShadowError::Pass)?;
        self.side
            .insert(batch, values)
            .map_err(|e| ShadowError::Table(table_error(e, RecordKey::Side)))
    }

    /// Point `q`'s record against tile edge `edge` of a face of the other
    /// operand; `None` if it isn't decided.
    pub fn side(&self, q: Point, edge: TileEdge) -> Option<&SideRec> {
        self.side.get(&SideKey { q, edge })
    }

    /// Point `q`'s record against tile corner `corner` of a face of the
    /// other operand; `None` if it isn't decided.
    pub fn corner(&self, q: Point, corner: TileCorner) -> Option<CornerRec> {
        self.corner.get(&CornerKey { q, corner }).copied()
    }

    /// The side table, for its reads and keys.
    pub fn side_table(&self) -> &Table<SideKey, SideRec> {
        &self.side
    }

    /// The corner table, for its reads and keys.
    pub fn corner_table(&self) -> &Table<CornerKey, CornerRec> {
        &self.corner
    }

    /// Decides every stored record again, on `geom` at `band`, and gives
    /// the key of the first whose answer differs (corners first, then
    /// sides, each table in key order): a check that the answers are
    /// functions of their keys.
    pub fn replay<G: SideGeometry>(&self, geom: &G, band: Band) -> Result<(), RecordKey> {
        replay(&self.corner, |k| decide_corner(k, geom), RecordKey::Corner)?;
        replay(
            &self.side,
            |k| decide_side(k, &self.corner, geom, band),
            RecordKey::Side,
        )
    }
}

/// Decides a corner record, by the flat point–vertex shadow row against
/// the corner moving with its face.
fn decide_corner<G: SideGeometry>(k: &Fresh<CornerKey>, geom: &G) -> Decided<CornerRec> {
    let key = *k.key();
    let tie = TieKey {
        row: Row::VertexVertexShadow,
        record: RecordKey::Corner(key),
    };
    let w = (key.q.operand != key.corner.operand)
        .then(|| geom.corner(key.q, &key.corner))
        .flatten();
    let Some(w) = w else {
        return (Err(Fail::Internal { key: tie }), Cost::default());
    };
    let q = geom.point(key.q);
    let (rec, w) = side_and_ahead(&q, &w, tie);
    (
        rec.map(|[side, ahead]| CornerRec { side, ahead }),
        Cost::exact(w),
    )
}

/// A tile edge, for its points' rates.
struct EdgeOf<'a, G> {
    geom: &'a G,
    key: SideKey,
}

impl<G: SideGeometry> SpanRates for EdgeOf<'_, G> {
    fn rates_at<S: Curved>(&self, t: &S) -> [[S; 3]; ORDERS] {
        self.geom.edge_rates(self.key.q, &self.key.edge, t)
    }
}

/// Decides a side record as a curved edge span's, its ends from the
/// corner records.
fn decide_side<G: SideGeometry>(
    k: &Fresh<SideKey>,
    corner: &Table<CornerKey, CornerRec>,
    geom: &G,
    band: Band,
) -> Decided<SideRec> {
    let key = *k.key();
    let tie = TieKey {
        row: Row::BoxSide,
        record: RecordKey::Side(key),
    };
    let internal = (Err(Fail::Internal { key: tie }), Cost::default());
    if key.q.operand == key.edge.operand {
        return internal;
    }
    let Some(span) = geom.tile_edge(&key.edge) else {
        return internal;
    };
    let corners = key.edge.corners();
    let mut ends = [(Sign::Pos, false); 2];
    let mut rates: [Option<[Rate; ORDERS]>; 2] = [None, None];
    for (j, c) in corners.iter().enumerate() {
        let Some(m) = geom.corner(key.q, c) else {
            return internal;
        };
        let p = span[2 * j];
        if m.at != glam::DVec3::new(p[0], p[1], p[2]) {
            // The geometry breaks its contract: the edge must end at its
            // corners' points exactly.
            return internal;
        }
        // The batch's corners are filled first: a missing one is a bug.
        let Some(r) = corner.get(&CornerKey {
            q: key.q,
            corner: *c,
        }) else {
            return internal;
        };
        ends[j] = (r.side, r.on_ray(key.q.operand));
        rates[j] = Some(m.rates);
    }
    let (rec, cost) = span_crossings(
        &span,
        ends,
        |j| rates[j].clone().expect("both corners read"),
        geom.point(key.q),
        key.q.operand,
        &EdgeOf { geom, key },
        band,
        tie,
    );
    (
        rec.map(|r| SideRec {
            crossings: r.crossings,
        }),
        cost,
    )
}

#[cfg(test)]
mod tests;
