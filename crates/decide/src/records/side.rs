//! The side records: a point's ray against a box side whose shadow the
//! counting can't certify off the point's, decided once per canonical key
//! and read by every box with that side.
//!
//! **Why.** The counting reads a box's degree from the turns of its four
//! sides about the query point's shadow ([`degree`](crate::count::degree)),
//! which a side whose shadow passes through the point's (or too close
//! for the level) has none of. The splits move interior box sides off
//! the point where they can; a tile line can't move (a box never
//! straddles one), and on a silhouette or an overlapping shadow no split
//! position may be certified off the point. Such a side is counted
//! instead under the perturbation, as a curved edge's span is
//! ([`super::ray`]): the signed crossings of its shadow by the point's
//! ray, the point moved, each crossing's sign `sign(ρ × T)` seen from
//! above, `ρ` the ray's actual direction (`−r̂` for a point of `A`, `+r̂`
//! for one of `B`), `T` the side's tangent in its increasing parameter.
//! Summed round a closed boundary these are its winding about the moved
//! point, so the degrees of boxes reading them stay additive.
//!
//! **Keys: one fact per maximal segment** ([`SideKey`], [`SideLine`]).
//! Boxes on the two sides of a line are refined independently, so their
//! sides along it don't match (T-junctions, other depths). The record is
//! therefore kept per *maximal* segment of the line: on a tile line the
//! tile edge ([`TileEdge`]), the row of the tile's stored control points
//! along the line over one cell of the other coordinate, a rational
//! quadratic with exact `f64` data, keyed by the line's global coordinate
//! and the cell, so the tiles on both sides read the one record; inside a
//! tile the whole iso line across the tile at its dyadic coordinate
//! ([`InnerLine`](crate::records::keys::InnerLine)), cut from the tile's stored net at each level. Its
//! crossings carry their parameter enclosures ([`SideCrossing::t`]); a
//! box side over part of the line counts those in its part
//! ([`SideRec::count_over`]), so every partition of the line counts each
//! crossing once.
//!
//! **Dyadic points, half-open** ([`CornerKey`], [`CornerRec`]). A line's
//! ends, and the box corners along it, are dyadic points of the face's
//! UV ([`TilePoint`], one key whichever tile reaches it): a tile corner
//! is a stored control point shared bit for bit by the up to four tiles
//! and edges meeting there; a point inside a tile edge is that edge's
//! stored row at a dyadic parameter, a point inside a tile the tile's
//! net at two, both exact rationals of the stored data, homogeneous with
//! a positive weight ([`HomPoint`]). The point's side against the query
//! point's `r̂` line and whether it is ahead along it are one record per
//! (point, dyadic point), the flat point–vertex shadow row against the
//! dyadic point moving with its face, decided exactly by
//! [`decide`](crate::num::decide::decide) on the moved configuration:
//! the lines read their ends from it (a ray through a corner counted once
//! along the line); a box side certified off the point but ending on its
//! ray line takes that end's side from it
//! ([`box_degree_sided`](crate::count::degree::box_degree_sided)); and a
//! box side over part of a line whose cut lies inside a crossing's
//! enclosure orders the crossing against the cut by it: a crossing that
//! is the one root of a piece with ends on either side of the ray line is
//! before the cut exactly where the cut's point is on the side the
//! crossing goes to (exactly on the line, its moved side). So a line
//! along the ray's line, all of whose points are ties, is cut at any
//! dyadic point consistently.
//!
//! **Deciding.** A line's crossings are decided as a curved edge span's
//! ([`super::ray`], module docs there: roots of the shadow across the ray
//! line isolated, their place along it certified or decided by the
//! vertex–curve shadow row at the piece, here [`Row::BoxSide`], the
//! side's points moving with their face), level by level until one
//! decides. Beside an end the row is the line through the end and the
//! middle control point, both exact (stored, or homogeneous for an inner
//! line), decided flat.
//!
//! **Decided once.** The records live in two [`Table`]s owned by
//! [`Sides`]; their deciding functions run only inside [`Sides::fill`]:
//! one charged [`Pass`] for the dyadic points asked and the ends of the
//! lines asked, one for the lines, reading those. A reader asks for what
//! it misses one key at a time ([`Want`]): a dyadic point alone is filled
//! alone, so a line that can't be decided never takes a point's record
//! with it.

use glam::DVec3;
use malachite::Rational;
use smallvec::SmallVec;

use crate::bezier::{Dir, Dyadic};
use crate::ids::FaceId;
use crate::num::decide::{Band, Fail, Row, Sign, TieKey};
use crate::num::exact::ORDERS;
use crate::perturb::{Curved, HomPoint, HomShadow, Moving, Rate};
use crate::records::keys::{
    CornerKey, Operand, Point, RecordKey, SideKey, SideLine, TileCorner, TileEdge, TilePoint,
};
use crate::records::ray::{Span, SpanCurve, SpanRates, TileNet, bernstein, span_crossings};
use crate::records::shadow::{
    Cost, Decided, Pass, ShadowError, ShadowGeometry, SideAhead, replay, shadow_error,
    side_and_ahead, side_and_ahead_by, table_error,
};
use crate::records::table::{Fresh, Table};

/// One crossing of a line's shadow by a point's ray.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SideCrossing {
    /// `[lo, hi]`, `f64`s with the crossing's parameter on the line's
    /// `[0, 1]` between them (an end, `[0, 0]` or `[1, 1]`, for a crossing
    /// at the line's end itself).
    pub t: [f64; 2],
    /// `sign(ρ × T)` seen from above.
    pub sign: Sign,
    /// Whether it is the one crossing of a piece whose ends lie on either
    /// side of the ray's line (else one of a tangency's pair): only such a
    /// crossing is ordered against a cut inside its enclosure.
    pub single: bool,
}

impl SideCrossing {
    /// The side of the ray's line the line goes to at the crossing:
    /// `ρ × T` along `+r̂` is `ν·T`, negated along `−r̂`.
    fn to(&self, of: Operand) -> Sign {
        match (of, self.sign) {
            (Operand::A, Sign::Pos) => Sign::Neg,
            (Operand::A, Sign::Neg) => Sign::Pos,
            (Operand::B, s) => s,
        }
    }
}

/// A point's ray against a line of a face of the other operand (module
/// docs).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SideRec {
    /// The crossings, in increasing parameter on the line's `[0, 1]`.
    pub crossings: SmallVec<[SideCrossing; 2]>,
}

/// Why a part of a line's count isn't read off its record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Over {
    /// A crossing's enclosure holds the cut at this end of the part (`0`
    /// its start, `1` its end), whose side isn't given: read the cut's
    /// dyadic point record and ask again.
    Side(usize),
    /// A crossing of a tangency's pair has its enclosure over a cut: not
    /// ordered against it.
    Unordered,
}

impl SideRec {
    /// The crossings summed as `±1`s: the whole line's count.
    pub fn count(&self) -> i64 {
        self.crossings
            .iter()
            .map(|c| i64::from(c.sign.as_i8()))
            .sum()
    }

    /// The count over the part `[lo, hi]` of the line's parameter, a box
    /// side's (`0 ≤ lo < hi ≤ 1`), for the ray of a point of `of`: the
    /// crossings in it. A crossing at an end of the line (`[0, 0]`, `[1,
    /// 1]`) is the part's reaching that end. A cut inside the line (`lo`
    /// or `hi` in `(0, 1)`) inside a crossing's enclosure is ordered
    /// against it by `sides`, the side of the cut's point against the
    /// ray's line from its record (module docs).
    pub fn count_over(
        &self,
        of: Operand,
        [lo, hi]: [f64; 2],
        sides: [Option<Sign>; 2],
    ) -> Result<i64, Over> {
        // Whether the crossing lies after the cut `x`, end `j` of the part.
        let after = |c: &SideCrossing, x: f64, j: usize| -> Result<bool, Over> {
            let [a, b] = c.t;
            if x <= 0.0 || a > x {
                return Ok(true);
            }
            if x >= 1.0 || b < x {
                return Ok(false);
            }
            if !c.single {
                return Err(Over::Unordered);
            }
            let s = sides[j].ok_or(Over::Side(j))?;
            // Before the cut exactly where the cut is on the side the
            // crossing goes to.
            Ok(s != c.to(of))
        };
        let mut n = 0;
        for c in &self.crossings {
            let starts = after(c, lo, 0)?;
            let ends = hi >= 1.0 || !after(c, hi, 1)?;
            if starts && ends {
                n += i64::from(c.sign.as_i8());
            }
        }
        Ok(n)
    }
}

/// A point against a dyadic point of a face of the other operand,
/// oriented from the point (module docs).
pub type CornerRec = SideAhead;

/// What a reader of the side records misses: a line's record (and its
/// ends'), or a dyadic point's alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Want {
    /// A line's record.
    Side(SideKey),
    /// A dyadic point's record.
    Corner(CornerKey),
}

impl From<SideKey> for Want {
    fn from(k: SideKey) -> Want {
        Want::Side(k)
    }
}

impl From<CornerKey> for Want {
    fn from(k: CornerKey) -> Want {
        Want::Corner(k)
    }
}

/// The geometry the side records read, beyond the flat records'.
pub trait SideGeometry: ShadowGeometry {
    /// Tile edge `e`'s stored control points with their weights, from
    /// its first corner to its second ([`TileEdge::corners`]); the same
    /// row whichever tile beside it is asked. `None` if the face has no
    /// such edge.
    fn tile_edge(&self, e: &TileEdge) -> Option<Span>;

    /// Tile `tile`'s stored net of face `face` of `operand`, biquadratic;
    /// its boundary rows are its tile edges' rows bit for bit. `None` if
    /// the face has no such tile.
    fn tile_net(&self, operand: Operand, face: FaceId, tile: [i64; 2]) -> Option<TileNet>;

    /// Tile corner `c`'s stored point (the tile edges' end control point
    /// there) with its rates for `q`'s record: moving with its face on
    /// `A`, zero on `B`. `None` if the face has no such corner.
    fn corner(&self, q: Point, c: &TileCorner) -> Option<Moving>;

    /// The rates of the face's dyadic point `p` for `q`'s record: moving
    /// with its face on `A` (the field's value there), zero on `B`; at a
    /// tile corner [`SideGeometry::corner`]'s.
    fn point_rates(&self, q: Point, p: &TilePoint) -> [Rate; ORDERS];

    /// The rates of line `line`'s point at the parameter `t` (an
    /// enclosure at its level), for `q`'s record: moving with its face on
    /// `A`, zero on `B`.
    fn line_rates<S: Curved>(&self, q: Point, line: &SideLine, t: &S) -> [[S; 3]; ORDERS];
}

/// The side and dyadic point records of a run, decided once per key
/// (module docs).
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

    /// Decides the records of `wanted` not yet stored: first the dyadic
    /// points asked and the ends of the lines asked, in one pass, then the
    /// lines, in another, at the curved band `band`. A key whose point is
    /// of the face's operand, whose line, tile or point the geometry
    /// doesn't have, or whose line's ends aren't its points' bit for bit
    /// fails its pass ([`Fail::Internal`]).
    pub fn fill<G, P>(
        &mut self,
        wanted: impl IntoIterator<Item = impl Into<Want>>,
        geom: &G,
        band: Band,
        pass: &mut P,
    ) -> Result<(), ShadowError<P::Error>>
    where
        G: SideGeometry + Sync,
        P: Pass,
    {
        let (mut lines, mut corners) = (Vec::new(), Vec::new());
        for w in wanted {
            match w.into() {
                Want::Side(k) => lines.push(k),
                Want::Corner(k) => corners.push(k),
            }
        }
        let batch = self.side.missing(lines);
        for k in batch.keys() {
            let k = *k.key();
            // A line past the range of `i64` fails its own pass.
            if let Some(ends) = k.line.ends() {
                corners.extend(ends.map(|corner| CornerKey { q: k.q, corner }));
            }
        }
        self.corner
            .fill(corners, |keys| {
                pass.run(keys, &|k: &Fresh<CornerKey>| decide_corner(k, geom))
            })
            .map_err(|e| shadow_error(e, RecordKey::Corner))?;
        if batch.is_empty() {
            return Ok(());
        }
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

    /// Point `q`'s record against line `line` of a face of the other
    /// operand; `None` if it isn't decided.
    pub fn side(&self, q: Point, line: SideLine) -> Option<&SideRec> {
        self.side.get(&SideKey { q, line })
    }

    /// Point `q`'s record against the dyadic point `corner` of a face of
    /// the other operand; `None` if it isn't decided.
    pub fn corner(&self, q: Point, corner: TilePoint) -> Option<CornerRec> {
        self.corner.get(&CornerKey { q, corner }).copied()
    }

    /// The side table, for its reads and keys.
    pub fn side_table(&self) -> &Table<SideKey, SideRec> {
        &self.side
    }

    /// The dyadic point table, for its reads and keys.
    pub fn corner_table(&self) -> &Table<CornerKey, CornerRec> {
        &self.corner
    }

    /// Decides every stored record again, on `geom` at `band`, and gives
    /// the key of the first whose answer differs (points first, then
    /// lines, each table in key order): a check that the answers are
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

/// The homogeneous point of the stored points `pts` blended by the exact
/// `basis`, moving at `rates`; `None` unless every weight is positive.
fn blend<'a>(
    pts: impl IntoIterator<Item = (&'a [f64; 4], Rational)>,
    rates: [Rate; ORDERS],
) -> Option<HomPoint> {
    let mut terms = Vec::new();
    for (p, b) in pts {
        if p[3].partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            return None;
        }
        let w = Rational::try_from(p[3]).ok()?;
        terms.push((DVec3::new(p[0], p[1], p[2]), b * w));
    }
    Some(HomPoint { terms, rates })
}

/// The face's dyadic point `p`, homogeneous, for `q`'s record: a point
/// inside a tile edge from the edge's row, any other from its tile's net;
/// `None` if the geometry hasn't the edge or tile, or a weight isn't
/// positive. Not for a tile corner.
fn dyadic_point<G: SideGeometry>(geom: &G, q: Point, p: &TilePoint) -> Option<HomPoint> {
    let rates = geom.point_rates(q, p);
    if let Some((edge, t)) = p.on_edge() {
        let row = geom.tile_edge(&edge)?;
        return blend(row.iter().zip(bernstein(&t.rational())), rates);
    }
    let net = geom.tile_net(p.operand, p.face, p.tile)?;
    let [bu, bv] = p.at.map(|t| bernstein(&t.rational()));
    blend(
        net.iter()
            .enumerate()
            .map(|(k, x)| (x, &bu[k / 3] * &bv[k % 3])),
        rates,
    )
}

/// Decides a dyadic point's record, by the flat point–vertex shadow row
/// against the point moving with its face: at a tile corner its stored
/// point, elsewhere the exact homogeneous point.
fn decide_corner<G: SideGeometry>(k: &Fresh<CornerKey>, geom: &G) -> Decided<CornerRec> {
    let key = *k.key();
    let tie = TieKey {
        row: Row::VertexVertexShadow,
        record: RecordKey::Corner(key),
    };
    let internal = (Err(Fail::Internal { key: tie }), Cost::default());
    if key.q.operand == key.corner.operand {
        return internal;
    }
    let q = geom.point(key.q);
    let (rec, w) = if let Some(c) = key.corner.corner() {
        let Some(w) = geom.corner(key.q, &c) else {
            return internal;
        };
        side_and_ahead(&q, &w, tie)
    } else {
        let Some(w) = dyadic_point(geom, key.q, &key.corner) else {
            return internal;
        };
        side_and_ahead_by(
            |axis| HomShadow {
                q: q.clone(),
                w: w.clone(),
                axis,
            },
            tie,
        )
    };
    (rec, Cost::exact(w))
}

/// A line, for its points' rates.
struct LineOf<'a, G> {
    geom: &'a G,
    key: SideKey,
}

impl<G: SideGeometry> SpanRates for LineOf<'_, G> {
    fn rates_at<S: Curved>(&self, t: &S) -> [[S; 3]; ORDERS] {
        self.geom.line_rates(self.key.q, &self.key.line, t)
    }
}

/// `t` as an `f64`, if exactly.
fn exact_f64(t: Dyadic) -> Option<f64> {
    let [lo, hi] = t.bounds();
    (lo == hi).then_some(lo)
}

/// Decides a line's record as a curved edge span's, its ends from their
/// dyadic points' records.
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
    if key.q.operand == key.line.operand() {
        return internal;
    }
    let Some(ends) = key.line.ends() else {
        return internal;
    };
    let (span, net);
    let curve = match key.line {
        SideLine::Edge(e) => {
            let Some(s) = geom.tile_edge(&e) else {
                return internal;
            };
            span = s;
            // The geometry must end the edge at its corners' points
            // exactly.
            for (j, c) in ends.iter().enumerate() {
                let p = span[2 * j];
                let at = c
                    .corner()
                    .and_then(|c| geom.corner(key.q, &c))
                    .map(|m| m.at);
                if at != Some(DVec3::new(p[0], p[1], p[2])) {
                    return internal;
                }
            }
            SpanCurve::Stored(&span)
        }
        SideLine::Inner(l) => {
            let (Some(n), Some(at)) = (geom.tile_net(l.operand, l.face, l.tile), exact_f64(l.at))
            else {
                return internal;
            };
            net = n;
            // The net's boundary rows across the line must be its tile
            // edges' rows exactly, where the line's ends lie.
            let other = match l.fixed {
                Dir::U => Dir::V,
                Dir::V => Dir::U,
            };
            for j in 0..2 {
                let row = SideLine::of(
                    l.operand,
                    l.face,
                    l.tile,
                    other,
                    [Dyadic::ZERO, Dyadic::ONE][j],
                )
                .and_then(|s| match s {
                    SideLine::Edge(e) => geom.tile_edge(&e),
                    SideLine::Inner(_) => None,
                });
                if row != Some(SpanCurve::column(&net, l.fixed, 2 * j)) {
                    return internal;
                }
            }
            SpanCurve::Iso {
                net: &net,
                fixed: l.fixed,
                at,
            }
        }
    };
    let mut sides = [(Sign::Pos, false); 2];
    let mut rates: [Option<[Rate; ORDERS]>; 2] = [None, None];
    for (j, c) in ends.iter().enumerate() {
        // The batch's ends are filled first: a missing one is a bug.
        let Some(r) = corner.get(&CornerKey {
            q: key.q,
            corner: *c,
        }) else {
            return internal;
        };
        sides[j] = (r.side, r.on_ray(key.q.operand));
        rates[j] = Some(match c.corner() {
            Some(tc) => match geom.corner(key.q, &tc) {
                Some(m) => m.rates,
                None => return internal,
            },
            None => geom.point_rates(key.q, c),
        });
    }
    let (rec, cost) = span_crossings(
        curve,
        sides,
        |j| rates[j].clone().expect("both ends read"),
        geom.point(key.q),
        key.q.operand,
        &LineOf { geom, key },
        band,
        tie,
    );
    (
        rec.map(|r| SideRec {
            crossings: r
                .rec
                .crossings
                .iter()
                .zip(&r.single)
                .map(|(c, &single)| SideCrossing {
                    t: c.t,
                    sign: c.sign,
                    single,
                })
                .collect(),
        }),
        cost,
    )
}

#[cfg(test)]
mod tests;
