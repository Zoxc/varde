//! Planar fast paths: which tiles of a face are planes, exactly, and
//! which column hits count alone, so that the counting takes a piece on
//! flat data by its exact hits instead of refining boxes towards a size.
//!
//! On a plane tile the column's hit is its plane's point, solved in the
//! tile's coordinates, and a ribbon over a plane tile meets the column at
//! the roots of a polynomial of low degree (two for a straight p-curve
//! span and edge; [`hit`](crate::records::height::hit)): the hits there
//! cost little and need no subdivision. So:
//! - a core box on a plane tile that the level doesn't certify clear of
//!   the query point's height is a piece once its hits are settled (at
//!   most one, or all at one exact side of the point) and lie strictly
//!   inside it, however large ([`s02`](super::s02));
//! - a **plane face** (every tile a plane, [`FlatFace::all`]) whose
//!   column misses its trims' padded hull ([`column_misses`]) holds no
//!   hit; one whose column hits are all clear ([`clear_hit`]), or which
//!   has none, is counted by them alone, no box tested: each by its exact
//!   height, or at the query point's height by its own height record.
//!
//! Hits are exact on any stored data, so neither shortcut changes a
//! count: the counting surface's hits are what a face's pieces sum to
//! (partition-free), and the shortcuts only skip the boxes that would
//! find them.
//!
//! A tile is a plane where its nine control points lie on one plane
//! exactly, decided on the stored `f64`s as rationals, never by a
//! distance.

use std::cmp::Ordering;

use malachite::Rational;

use crate::num::exact::rat;
use crate::perturb::{NU, R_HAT};
use crate::records::height::hit::{Hit, HitAt, HitFace};
use crate::records::ray::TileNet;

type Q3 = [Rational; 3];

fn sub(a: &Q3, b: &Q3) -> Q3 {
    std::array::from_fn(|k| &a[k] - &b[k])
}

fn cross(a: &Q3, b: &Q3) -> Q3 {
    [
        &a[1] * &b[2] - &a[2] * &b[1],
        &a[2] * &b[0] - &a[0] * &b[2],
        &a[0] * &b[1] - &a[1] * &b[0],
    ]
}

fn dot(a: &Q3, b: &Q3) -> Rational {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}

/// The exact normal of a tile net whose nine control points lie on one
/// plane (any orientation), its weights finite and positive; `None`
/// where they don't, lie on a line, or a coordinate isn't finite. The
/// normal is the cross product of the net's corner edges `(0, 0) → (2,
/// 0)` and `(0, 0) → (0, 2)` (control points row-major in `u`).
pub fn plane_normal(net: &TileNet) -> Option<Q3> {
    if !net
        .iter()
        .all(|c| c.iter().all(|x| x.is_finite()) && c[3] > 0.0)
    {
        return None;
    }
    let p: Vec<Q3> = net.iter().map(|c| [c[0], c[1], c[2]].map(rat)).collect();
    let n = cross(&sub(&p[6], &p[0]), &sub(&p[2], &p[0]));
    if n.iter().all(|x| *x == 0u32) {
        return None;
    }
    p.iter()
        .all(|x| dot(&n, &sub(x, &p[0])) == 0u32)
        .then_some(n)
}

/// Whether a tile net is a plane exactly ([`plane_normal`]).
pub fn plane_tile(net: &TileNet) -> bool {
    plane_normal(net).is_some()
}

/// Which tiles of a face are planes (module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlatFace {
    /// Per tile, in the face's nets' order (`u` index first), whether it
    /// is a plane.
    pub planes: Vec<bool>,
    /// Whether every tile is a plane: a plane face.
    pub all: bool,
}

impl FlatFace {
    /// The plane tiles of `face`, from its stored nets.
    pub fn of(face: &HitFace<'_>) -> FlatFace {
        let planes: Vec<bool> = face.nets.iter().map(plane_tile).collect();
        let all = !planes.is_empty() && planes.iter().all(|&p| p);
        FlatFace { planes, all }
    }

    /// Whether tile `[i, j]` of `face` is a plane; `false` for a tile not
    /// the face's.
    pub fn plane_at(&self, face: &HitFace<'_>, [i, j]: [i64; 2]) -> bool {
        let [us, vs] = &face.tiles;
        if !us.contains(&i) || !vs.contains(&j) {
            return false;
        }
        let (Ok(nv), Ok(di), Ok(dj)) = (
            usize::try_from(vs.end - vs.start),
            usize::try_from(i - us.start),
            usize::try_from(j - vs.start),
        ) else {
            return false;
        };
        di.checked_mul(nv)
            .and_then(|k| k.checked_add(dj))
            .and_then(|k| self.planes.get(k))
            .copied()
            .unwrap_or(false)
    }
}

/// Whether the column of `q` misses the counting surface of `face`, a
/// plane face: along `r̂`, or along `ν`, every control point of its
/// edges' spans lies on one side of `q` by more than the edge's near
/// radius `3·β_g` (times the direction's 1-norm), exactly. The face's
/// part of its plane is the region its trims `S∘p` bound, so it lies in
/// their hull, and each trim and its ribbon lie within the edge's band
/// of the edge's curve, which lies in its spans' hull (weights
/// positive): the column meets none of them, the face holds no hit,
/// without a search. A face with no trims never misses.
pub fn column_misses(face: &HitFace<'_>, q: [f64; 3]) -> bool {
    if face.trims.is_empty() || !q.iter().all(|x| x.is_finite()) {
        return false;
    }
    let q = q.map(rat);
    [R_HAT, NU].iter().any(|d| {
        let n1 = rat(d.x.abs() + d.y.abs() + d.z.abs());
        let d = [d.x, d.y, d.z].map(rat);
        let (d, q) = (&d, &q);
        // Per control point its offset from `q` along `d` less or plus
        // its pad: `Greater` past `q` on the far side, `Less` on the near
        // one, `Equal` within the pad (or not finite).
        let mut sides = face.trims.iter().flat_map(|t| {
            let r = t.near_radius();
            let pad = r.is_finite().then(|| rat(r) * &n1);
            t.curve.iter().flatten().map(move |c| {
                let (Some(pad), true) = (&pad, c[..3].iter().all(|x| x.is_finite())) else {
                    return Ordering::Equal;
                };
                let x = dot(d, &sub(&[c[0], c[1], c[2]].map(rat), q));
                if x > *pad {
                    Ordering::Greater
                } else if x < -pad {
                    Ordering::Less
                } else {
                    Ordering::Equal
                }
            })
        });
        match sides.next() {
            Some(s) if s != Ordering::Equal => sides.all(|x| x == s),
            _ => false,
        }
    })
}

/// Whether a column hit counts by its own exact height and facing alone
/// and its height record (module docs): a tile hit, its facing formed,
/// not on an edge of the face, and outside every edge's near radius
/// ([`Hit::near_edge`]); off `q`'s height its exact height decides, at
/// it the hit's own key (`Hit(q, f, sheet, i)`). The moved column meets the same hit there: a
/// column through the face's boundary meets a ribbon, whose hits are
/// near their edge, and one through an edge's neighbourhood is left to
/// the boxes (the lobe fallback reads hits there).
pub fn clear_hit(face: &HitFace<'_>, h: &Hit) -> bool {
    matches!(h.at, HitAt::Tile(_))
        && h.facing.is_some()
        && h.inc.is_none()
        && face.trims.iter().all(|t| !h.near_edge(face, t.edge))
}

#[cfg(test)]
mod tests;
