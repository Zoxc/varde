//! The vertex rules of the direction field: vertex groups, the flush-patch
//! direction and the saddle vertex's partner direction. Each gives one
//! direction for every record of its vertex (or group), blended into the
//! elements' own directions as a vertex's is ([`super::dir`]).
//!
//! **Vertex groups.** `A` vertices whose `2r` balls meet along an edge
//! (an edge shorter than `2(r_a + r_b)`, compared exactly) move as one
//! group ([`vertex_groups`]), by one direction `d_G` taken as for a vertex
//! over all the group's faces ([`super::dir::flat`],
//! [`super::dir::vertex`]); none agreeing, the saddle rule for the group.
//! The blend uses `d_G` within the group's largest radius `r_G` of its
//! nearest member: `λ = clamp(dist/r_G − 1, 0, 1)` with `dist` the
//! distance to the nearest member ([`group_weight_flat`],
//! [`group_weight`]), the same field as a vertex's for a group of one.
//!
//! **Flush patches.** A vertex (or group) without a common direction
//! whose incident faces are coincident with patches of `B` (co-oriented or
//! opposed) takes the max–min direction over those faces' normals alone
//! ([`flush_flat`], [`flush_at`]): exact with its least-norm tie-break on
//! flat faces, the `f64` normals' optimum certified at a level on curved
//! ones. Its minimum not above zero: the opposed-patch rule if the patches
//! are of both kinds ([`Flush::Opposed`]), else the saddle rule.
//!
//! **Saddle partner.** A saddle vertex (no direction agrees) whose
//! partner is a convex vertex or edge of `B`, with tangent cone `K_B =
//! {x : n_B·x ≤ 0}` over the partner's outward normals, moves along a
//! direction `d` inside `K_B` ([`saddle_partner`]) where `T_A ∩ K_B = {0}`
//! is certified, `T_A` the closed cone of `A` at the vertex: then `(T_A −
//! λd) ∩ K_B = {0}` for `λ > 0`, so the moved vertex enters `B` under a
//! union and leaves it untouched otherwise. `d` is the flat direction of
//! the negated normals ([`super::dir::flat`]: equal offsets, else the
//! max–min one), so `n_B·d < 0` on every partner face, exactly, and it
//! moves with the input under the cube's symmetries. The certificate is
//! exact on the corners of the vertex's faces ([`Sector`]), split into
//! convex pieces: no piece meets `K_B` but at the apex (an interval of
//! one parameter per piece), and `d` lies outside `T_A`, read at the
//! first crossing of `∂T_A` along a great circle from `d` through a
//! piece (the side of that piece's plane the circle arrives from). The
//! boundary of `T_A` is the pieces, so with neither `K_B \ {0}`, which is
//! connected, lies outside `T_A`. A degenerate circle (through an edge's
//! ray, along a piece's plane) tries the next piece's; none left, or any
//! test failing, the rule isn't certified and the vertex takes `d = 0`
//! (the generic translations decide), which is always realisable.

use std::cmp::Ordering;

use glam::DVec3;
use malachite::Rational;

use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::Nearest;

use super::dir::{
    self, DirWork, FlatDir, Q3, add, blend_weight, cross, dot, exact, max_min_counted, rescale,
    scale, scaled_normal, verdict, weight_flat,
};
use super::{Curved, lit3, unit3};

/// A vertex of `A` as grouping sees it: where it is and its blend radius
/// `r` (finite and `> 0`, else it groups with no neighbour).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blended {
    /// The vertex.
    pub at: DVec3,
    /// Its blend radius.
    pub r: f64,
}

/// The vertex groups (module docs): for each vertex the least index of its
/// group, vertices joined by an edge shorter than `2(r_a + r_b)` (an edge
/// as the indices of its two vertices), exactly; a vertex alone is its
/// own group. Each edge's test is counted in `work`. Indices past the
/// vertices are ignored.
pub fn vertex_groups(verts: &[Blended], edges: &[[usize; 2]], work: &mut DirWork) -> Vec<usize> {
    let mut parent: Vec<usize> = (0..verts.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let radius = |r: f64| {
        (r.is_finite() && r > 0.0)
            .then(|| Rational::try_from(r).ok())
            .flatten()
    };
    for &[a, b] in edges {
        work.check(1);
        let (Some(va), Some(vb)) = (verts.get(a), verts.get(b)) else {
            continue;
        };
        let (Some(ra), Some(rb), Some(pa), Some(pb)) =
            (radius(va.r), radius(vb.r), exact(va.at), exact(vb.at))
        else {
            continue;
        };
        let v = sub(&pb, &pa);
        let reach = Rational::from(2) * (ra + rb);
        if dot(&v, &v) < &reach * &reach {
            let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
            // The lesser root stays, so a group's root is its least member.
            match ra.cmp(&rb) {
                Ordering::Less => parent[rb] = ra,
                Ordering::Greater => parent[ra] = rb,
                Ordering::Equal => {}
            }
        }
    }
    (0..verts.len()).map(|i| root(&mut parent, i)).collect()
}

/// The flat blend weight of a group (module docs): `λ = clamp(dist/r_G −
/// 1, 0, 1)` with `dist` the max-norm distance from `x` to the nearest of
/// `members` and `r_G` the group's largest radius, exactly: the least of
/// the members' own weights at `r_G`. `1` without members.
pub fn group_weight_flat(members: &[Q3], x: &Q3, r_g: f64) -> Rational {
    members
        .iter()
        .map(|a| weight_flat(a, x, r_g))
        .min()
        .unwrap_or_else(|| Rational::from(1))
}

/// The curved blend weight of a group (module docs), in `f64` from the
/// `f64` coordinates of the members and of the record's point: the least
/// of the members' own weights at `r_G` ([`blend_weight`]), `1` without
/// members.
pub fn group_weight(members: &[DVec3], x: DVec3, r_g: f64) -> f64 {
    members
        .iter()
        .map(|&a| blend_weight(a, x, r_g))
        .fold(1.0, |m, l| if l < m { l } else { m })
}

/// Which way a face's coincident `B` patch faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PatchSide {
    /// The patch faces the way the `A` face does.
    CoOriented,
    /// The patch faces against it.
    Opposed,
}

/// An incident face of `A` with a coincident patch of `B` (module docs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Patch {
    /// The face's stored outward normal.
    pub normal: DVec3,
    /// How its patch faces.
    pub side: PatchSide,
}

/// A flush-patch direction (module docs).
#[derive(Clone, Debug, PartialEq)]
pub enum Flush<D> {
    /// The max–min direction over the patches' faces, agreeing with each.
    Dir(D),
    /// None agrees and the patches are of both kinds: the opposed-patch
    /// rule decides.
    Opposed,
    /// None agrees and the patches are of one kind: the saddle rule
    /// decides.
    Saddle,
    /// The level settles neither (curved only): refine.
    Escalate,
}

/// What no agreeing direction leaves: the opposed-patch rule where the
/// patches are of both kinds, else the saddle rule.
fn none_agrees<D>(sides: impl IntoIterator<Item = PatchSide>) -> Flush<D> {
    let (mut co, mut opp) = (false, false);
    for s in sides {
        match s {
            PatchSide::CoOriented => co = true,
            PatchSide::Opposed => opp = true,
        }
    }
    if co && opp {
        Flush::Opposed
    } else {
        Flush::Saddle
    }
}

/// The flush-patch direction on flat faces (module docs): the max–min
/// direction over the patches' faces' normals scaled to max-norm 1 (equal
/// ones once), exactly, the least-norm point of the program's optimal set
/// ([`FlatDir::MaxMin`]) where its minimum is `> 0`. No patch is the
/// saddle rule. `None` for a zero normal or one that isn't finite. The
/// program is counted in `work`.
pub fn flush_flat(patches: &[Patch], work: &mut DirWork) -> Option<Flush<FlatDir>> {
    if patches.is_empty() {
        return Some(Flush::Saddle);
    }
    let normals: Vec<DVec3> = patches.iter().map(|p| p.normal).collect();
    Some(match dir::max_min_flat(&normals, work)? {
        d @ FlatDir::MaxMin { .. } => Flush::Dir(d),
        _ => none_agrees(patches.iter().map(|p| p.side)),
    })
}

/// A curved face with a coincident patch at the vertex, for [`flush_at`].
#[derive(Clone, Debug, PartialEq)]
pub struct CurvedPatch<S> {
    /// The face's normal at the vertex at the level (any positive
    /// multiple, oriented outward).
    pub normal: [S; 3],
    /// That normal in `f64`, the same at every level (any positive
    /// multiple, near the normal): the candidate is taken from these.
    pub approx: DVec3,
    /// How its patch faces.
    pub side: PatchSide,
}

/// The flush-patch direction on curved faces (module docs): the max–min
/// direction of the `f64` normals, exact as the flat program gives it and
/// rounded to `f64`, unit at the level, if it is certified to agree with
/// every face; a fixed vector whatever the level, which only certifies
/// it. Certainly no direction agreeing: [`Flush::Opposed`] or
/// [`Flush::Saddle`] by the patches' kinds, where two faces' `f64`
/// normals are exact opposites (one surface's two sides, as coincident
/// faces on one surface object are: no direction agrees with both, which
/// no level could certify, the max–min value being exactly zero) or by
/// the vertex rule's saddle certificate. Neither settled:
/// [`Flush::Escalate`]. No patch is the saddle rule. The programs are
/// counted in `work`.
pub fn flush_at<S: Curved>(patches: &[CurvedPatch<S>], work: &mut DirWork) -> Flush<[S; 3]> {
    let Some(like) = patches.first().map(|p| p.normal[0].clone()) else {
        return Flush::Saddle;
    };
    let sides = || patches.iter().map(|p| p.side);
    let opposite = patches.iter().enumerate().any(|(i, p)| {
        p.approx != DVec3::ZERO && patches[i + 1..].iter().any(|o| o.approx == -p.approx)
    });
    if opposite {
        return none_agrees(sides());
    }
    let normals: Vec<[S; 3]> = patches.iter().map(|p| rescale(&p.normal)).collect();
    let approx: Option<Vec<Q3>> = patches.iter().map(|p| scaled_normal(p.approx)).collect();
    if let Some(ns) = approx {
        let (d, min) = max_min_counted(&ns, work);
        if min > 0u32 {
            let rounded = d.each_ref().map(|x| f64::rounding_from(x, Nearest).0);
            let d = unit3(&lit3(DVec3::from_array(rounded), &like));
            match verdict(&d, &normals) {
                Some(true) => return Flush::Dir(d),
                Some(false) => {}
                None => return Flush::Escalate,
            }
        }
    }
    if dir::certified_saddle(&normals, work) {
        none_agrees(sides())
    } else {
        Flush::Escalate
    }
}

/// A face's corner at a vertex of `A`, exactly: the directions of its two
/// edges from the vertex, `from` then `to` counter-clockwise about the
/// face's outward normal seen from outside (the face lies between them,
/// turning that way), and that normal, for which way the corner turns
/// (convex, straight or reflex). The corners of a vertex's faces are the
/// boundary of `A`'s cone there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sector {
    /// The first edge's direction.
    pub from: Q3,
    /// The second edge's direction.
    pub to: Q3,
    /// The face's outward normal (any positive multiple).
    pub normal: Q3,
}

impl Sector {
    /// The corner at the stored vertex `at` between the edges to the
    /// stored points `from` and `to` (their other ends, or any points on
    /// them), of a face with the stored outward normal `normal`: the
    /// differences exact. `None` for a value that isn't finite.
    pub fn of(at: DVec3, from: DVec3, to: DVec3, normal: DVec3) -> Option<Sector> {
        let at = exact(at)?;
        Some(Sector {
            from: sub(&exact(from)?, &at),
            to: sub(&exact(to)?, &at),
            normal: exact(normal)?,
        })
    }

    /// The corner as convex pieces `[g₁, g₂]`, each turning by less than
    /// `π` from `g₁` to `g₂` about `g₁ × g₂` (its outward normal), in the
    /// corner's own plane: the corner itself if it turns by less than
    /// `π`, else cut at its quarter and half turns. `None` for a zero
    /// edge or normal, or a corner of no turn.
    fn pieces(&self) -> Option<Vec<[Q3; 2]>> {
        let (a, b) = (&self.from, &self.to);
        if is_zero(a) || is_zero(b) || is_zero(&self.normal) {
            return None;
        }
        let c = cross(a, b);
        let turn = dot(&c, &self.normal);
        if turn > 0u32 {
            return Some(vec![[a.clone(), b.clone()]]);
        }
        // The corner's plane's normal, outward: `−(a × b)` for a reflex
        // corner; for a straight one the stored normal less its part
        // along `a`, exactly `⊥ a` (and `b`, along `−a`).
        let p = if turn < 0u32 {
            c.map(|x| -x)
        } else if is_zero(&c) && dot(a, b) < 0u32 {
            let n = &self.normal;
            let k = dot(n, a) / dot(a, a);
            sub(n, &scale(a, &k))
        } else {
            return None;
        };
        if is_zero(&p) {
            return None;
        }
        let quarter = cross(&p, a);
        let half = a.clone().map(|x| -x);
        if turn < 0u32 {
            Some(vec![
                [a.clone(), quarter.clone()],
                [quarter, half.clone()],
                [half, b.clone()],
            ])
        } else {
            Some(vec![[a.clone(), quarter.clone()], [quarter, b.clone()]])
        }
    }
}

/// The saddle vertex's direction toward a convex partner of `B` (module
/// docs): `d` inside `K_B = {x : n·x ≤ 0}`, `n` over `partner`, the
/// partner's stored outward normals (a convex vertex's or edge's, at
/// least two independent; the caller vouches for its convexity), where
/// `T_A ∩ K_B = {0}` is certified on `star`, the corners of the vertex's
/// faces; `None` where it isn't (or the data is degenerate): the vertex
/// takes `d = 0`. The tests are counted in `work`.
pub fn saddle_partner(star: &[Sector], partner: &[DVec3], work: &mut DirWork) -> Option<Q3> {
    let normals = partner
        .iter()
        .map(|&n| exact(n))
        .collect::<Option<Vec<Q3>>>()?;
    if rank(&normals) < 2 || star.is_empty() {
        return None;
    }
    let inward: Vec<DVec3> = partner.iter().map(|&n| -n).collect();
    let d = match dir::flat(&inward, work)? {
        FlatDir::Saddle => return None,
        f => f.d(),
    };
    let mut pieces = Vec::new();
    for s in star {
        pieces.extend(s.pieces()?);
    }
    // No piece meets `K_B` but at the apex.
    for [g1, g2] in &pieces {
        work.check(normals.len());
        if meets(g1, g2, &normals) {
            return None;
        }
    }
    // `d` outside `T_A`, along the first circle that settles it.
    for [g1, g2] in &pieces {
        work.check(pieces.len());
        if let Some(outside) = outside_along(&d, &add(g1, g2), &pieces) {
            return outside.then_some(d);
        }
    }
    None
}

/// Whether the convex piece `cone(g₁, g₂)` meets `{x : n·x ≤ 0}` over
/// `normals` away from the apex: some `x = g₂ + λ·(g₁ − g₂)`, `λ ∈ [0,
/// 1]`, has every `n·x ≤ 0` (each an interval of `λ`), exactly. Such an
/// `x` is never zero: the piece turns by less than `π`.
fn meets(g1: &Q3, g2: &Q3, normals: &[Q3]) -> bool {
    let (mut lo, mut hi) = (Rational::from(0), Rational::from(1));
    let step = sub(g1, g2);
    for n in normals {
        // `a + λ·b ≤ 0`.
        let (a, b) = (dot(n, g2), dot(n, &step));
        match b.cmp(&Rational::from(0)) {
            Ordering::Greater => hi = hi.min(-a / b),
            Ordering::Less => lo = lo.max(-a / b),
            Ordering::Equal if a > 0u32 => return false,
            Ordering::Equal => {}
        }
        if lo > hi {
            return false;
        }
    }
    true
}

/// Whether `d` lies outside the cone bounded by `pieces`, read along the
/// great circle through `d` and `s` (module docs): at its first crossing
/// of a piece turning from `d` toward `s`, the side of that piece's plane
/// the circle arrives from. `None` where the circle is degenerate: `s`
/// along `±d`, through an edge's ray, along a piece's plane or tangent to
/// it at the crossing, two pieces crossed at once, or no piece crossed.
fn outside_along(d: &Q3, s: &Q3, pieces: &[[Q3; 2]]) -> Option<bool> {
    let m = cross(d, s);
    if is_zero(&m) {
        return None;
    }
    // In the circle's plane: angle 0 at `d`, a quarter turn at `m × d`.
    let ahead = cross(&m, d);
    // The half of the turn a ray lies in: 0 for `(0, π)`, 1 at `π`, 2 for
    // `(π, 2π)`.
    let half = |x: &Q3| match dot(&ahead, x).cmp(&Rational::from(0)) {
        Ordering::Greater => 0,
        Ordering::Equal => 1,
        Ordering::Less => 2,
    };
    let mut first: Option<(Q3, &[Q3; 2])> = None;
    for piece in pieces {
        let [g1, g2] = piece;
        let (a, b) = (dot(&m, g1), dot(&m, g2));
        let (sa, sb) = (a.cmp(&Rational::from(0)), b.cmp(&Rational::from(0)));
        if sa == Ordering::Equal || sb == Ordering::Equal {
            return None;
        }
        if sa == sb {
            continue;
        }
        // `|b|·g₁ + |a|·g₂`, on the circle's plane.
        let x = add(&scale(g1, &abs(&b)), &scale(g2, &abs(&a)));
        if half(&x) == 1 && dot(d, &x) > 0u32 {
            return None;
        }
        let earlier = match &first {
            None => Ordering::Less,
            Some((y, _)) => match half(&x).cmp(&half(y)) {
                Ordering::Equal => dot(&m, &cross(y, &x)).cmp(&Rational::from(0)),
                o => o,
            },
        };
        match earlier {
            Ordering::Less => first = Some((x, piece)),
            Ordering::Equal => return None,
            Ordering::Greater => {}
        }
    }
    let (x, [g1, g2]) = first?;
    // Arriving from `d`: against the turn's tangent `m × x`.
    let arrive = dot(&cross(g1, g2), &cross(&m, &x));
    match arrive.cmp(&Rational::from(0)) {
        Ordering::Less => Some(true),
        Ordering::Greater => Some(false),
        Ordering::Equal => None,
    }
}

/// The rank of `vs`, up to 3.
fn rank(vs: &[Q3]) -> usize {
    let Some(a) = vs.iter().find(|v| !is_zero(v)) else {
        return 0;
    };
    let Some(b) = vs.iter().find(|v| !is_zero(&cross(a, v))) else {
        return 1;
    };
    let c = cross(a, b);
    if vs.iter().any(|v| dot(&c, v) != 0u32) {
        3
    } else {
        2
    }
}

fn sub(a: &Q3, b: &Q3) -> Q3 {
    [0, 1, 2].map(|i| &a[i] - &b[i])
}

fn abs(x: &Rational) -> Rational {
    if *x < 0u32 { -x } else { x.clone() }
}

fn is_zero(v: &Q3) -> bool {
    v.iter().all(|x| *x == 0u32)
}

#[cfg(test)]
mod tests;
