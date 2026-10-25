//! Coincidence groups: elements of `A` that coincide within the operand
//! and move as one, by one direction.
//!
//! **Membership is coincidence only**, decided on the operand's structure
//! and its stored values exactly, never by a distance
//! ([`coincidence_groups`]):
//!
//! - faces on one surface object with **opposite** `flipped` whose trims
//!   overlap over an area ([`trims_overlap`], exact on straight trims in
//!   the surface's chart): the two sides of a zero-width gap;
//! - edges on one space curve over **overlapping** parameter ranges
//!   (open intervals meeting; pieces of one split edge only touch);
//! - the edges of coedges sharing one p-curve (the two sides of a slit);
//! - the members of one contact record (a point on an edge or a face
//!   stands for that edge or face).
//!
//! Faces on one surface object with equal `flipped` (a plate's top split
//! by a slot) and pieces of one split edge don't coincide and form no
//! group. A group is closed downward: a member face's edges and vertices
//! and a member edge's vertices are members too, so every point of a
//! member reads the group's direction, and groups sharing such an
//! element are one. Every union is counted ([`DirWork::checks`]) with the
//! trim tests' segment pairs and point tests, for the caller to charge.
//!
//! **Direction, in order** ([`group_dir_flat`]): the max–min direction
//! over the normals of every face at a member ([`group_normals`]), if its
//! minimum is `> 0` ([`GroupDir::Common`]); else the flush-patch rule for
//! the group as one vertex, over the group's faces with coincident `B`
//! patches ([`GroupDir::Flush`], [`super::vertex::flush_flat`]); else,
//! with patches of both kinds, the opposed-patch rule
//! ([`GroupDir::Opposed`]); else `d = 0` ([`GroupDir::Zero`]) and the
//! generic translations `T₂…T₄` decide. Opposed faces on one surface
//! object never have a common direction (`n` and `−n`), so a zero-width
//! gap takes a flush direction or `d = 0`: a translation either way, and
//! `A` moved stays embedded where it touches itself. Every member point
//! moves at the group's one rate ([`GroupDir::rate`]), so records sharing
//! an order-0 key at a group point read one motion.
//!
//! **Blend** ([`blend_flat`]): an element that isn't a member moves along
//! `(1 − λ)·d_G + λ·d(x)` near the group, `λ = clamp(dist/r_G − 1, 0,
//! 1)` with `dist` the max-norm distance from the record's point to the
//! nearest member vertex or member edge ([`Reach`]) and `r_G` the largest
//! blend radius of the member vertices, **only if `d_G` agrees with the
//! element's own faces** (exactly); otherwise, and for a group of no
//! direction, it keeps its own `d(x)`. A member face's interior is a
//! member's, so its distance is read at its boundary.
//!
//! Flat only: the trims are straight, the directions exact; a face pair
//! whose trims aren't given as polygons is refused
//! ([`GroupError::CurvedTrims`]).

use std::cmp::Ordering;

use glam::DVec3;
use malachite::Rational;

use super::Rate;
use super::dir::{self, DirWork, FlatDir, Q3, mix_flat};
use super::vertex::{Flush, Patch, flush_flat};
use crate::ids::{CurveId, EdgeId, FaceId, SurfId, VertId};

/// A vertex of `A` as grouping sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupVert {
    /// Where it is.
    pub at: DVec3,
    /// Its blend radius (finite and `> 0`; else it widens no group's
    /// blend).
    pub r: f64,
    /// The faces at it.
    pub faces: Vec<FaceId>,
}

/// An edge of `A` as grouping sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupEdge {
    /// Its space curve.
    pub curve: CurveId,
    /// Its range on the curve, increasing.
    pub t_range: [f64; 2],
    /// Its vertices.
    pub verts: [VertId; 2],
    /// Its faces.
    pub faces: Vec<FaceId>,
}

/// A coedge of `A` as grouping sees it: its edge and its p-curve (any id
/// naming the p-curve object).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupCoedge {
    /// Its edge.
    pub edge: EdgeId,
    /// Its p-curve.
    pub pcurve: u32,
}

/// A face of `A` as grouping sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupFace {
    /// Its surface object.
    pub surface: SurfId,
    /// Whether it faces against its surface.
    pub flipped: bool,
    /// Its stored outward normal (a plane's).
    pub normal: DVec3,
    /// Its trim loops as polygons in one chart of the surface (shared by
    /// the faces of the surface object: its parameters, or [`chart`]),
    /// `None` where they aren't straight.
    pub trims: Option<Vec<Vec<[f64; 2]>>>,
    /// Its edges.
    pub edges: Vec<EdgeId>,
    /// Its vertices.
    pub verts: Vec<VertId>,
}

/// An element a contact record names: a point on an edge or a face is
/// given as that edge or face.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Member {
    /// A vertex.
    Vert(VertId),
    /// An edge.
    Edge(EdgeId),
    /// A face.
    Face(FaceId),
}

/// The operand's structure as grouping reads it, by id.
#[derive(Clone, Copy, Debug)]
pub struct Operand<'a> {
    /// The vertices.
    pub verts: &'a [GroupVert],
    /// The edges.
    pub edges: &'a [GroupEdge],
    /// The coedges.
    pub coedges: &'a [GroupCoedge],
    /// The faces.
    pub faces: &'a [GroupFace],
    /// The contact records, each its members (both sides of every pair).
    pub contacts: &'a [Vec<Member>],
}

/// Why no groups were formed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupError {
    /// An id past its kind's elements, or a value that isn't finite.
    Data,
    /// Two faces on one surface object with opposite `flipped` whose
    /// trims aren't straight: their overlap isn't decided here.
    CurvedTrims([FaceId; 2]),
}

/// One coincidence group: its members, each list increasing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Group {
    /// The member vertices.
    pub verts: Vec<VertId>,
    /// The member edges.
    pub edges: Vec<EdgeId>,
    /// The member faces.
    pub faces: Vec<FaceId>,
}

/// The coincidence groups of an operand ([`coincidence_groups`]), in the
/// order of their least member (vertices, then edges, then faces).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Groups {
    /// The groups.
    pub groups: Vec<Group>,
    vert: Vec<u32>,
    edge: Vec<u32>,
    face: Vec<u32>,
}

const NO_GROUP: u32 = u32::MAX;

impl Groups {
    fn get(of: &[u32], i: usize) -> Option<usize> {
        of.get(i).filter(|&&g| g != NO_GROUP).map(|&g| g as usize)
    }

    /// The group of vertex `v`, if it is a member.
    pub fn of_vert(&self, v: VertId) -> Option<usize> {
        Self::get(&self.vert, v.index())
    }

    /// The group of edge `e`, if it is a member.
    pub fn of_edge(&self, e: EdgeId) -> Option<usize> {
        Self::get(&self.edge, e.index())
    }

    /// The group of face `f`, if it is a member.
    pub fn of_face(&self, f: FaceId) -> Option<usize> {
        Self::get(&self.face, f.index())
    }
}

/// Union–find over every element, vertices first, then edges, then
/// faces; a root is its set's least element.
struct Sets {
    parent: Vec<usize>,
    grouped: Vec<bool>,
}

impl Sets {
    fn root(&mut self, mut i: usize) -> usize {
        while self.parent[i] != i {
            self.parent[i] = self.parent[self.parent[i]];
            i = self.parent[i];
        }
        i
    }

    /// Joins `a` and `b`; `coincide` marks the set as a group (a closure
    /// union only joins).
    fn join(&mut self, a: usize, b: usize, coincide: bool, work: &mut DirWork) {
        work.check(1);
        let (ra, rb) = (self.root(a), self.root(b));
        let (lo, hi) = match ra.cmp(&rb) {
            Ordering::Greater => (rb, ra),
            _ => (ra, rb),
        };
        self.parent[hi] = lo;
        self.grouped[lo] = self.grouped[lo] || self.grouped[hi] || coincide;
    }
}

/// The coincidence groups of `op` (module docs): faces on one surface
/// object with opposite `flipped` and trims overlapping over an area,
/// edges on one curve over overlapping ranges, the edges of coedges on
/// one p-curve, and each contact record's members, closed downward. The
/// unions and the trim tests are counted in `work`. Elements that
/// coincide with none are in no group.
pub fn coincidence_groups(op: &Operand<'_>, work: &mut DirWork) -> Result<Groups, GroupError> {
    let (nv, ne, nf) = (op.verts.len(), op.edges.len(), op.faces.len());
    let v_at = |v: VertId| {
        (v.index() < nv)
            .then_some(v.index())
            .ok_or(GroupError::Data)
    };
    let e_at = |e: EdgeId| {
        (e.index() < ne)
            .then_some(nv + e.index())
            .ok_or(GroupError::Data)
    };
    let f_at = |f: FaceId| {
        (f.index() < nf)
            .then_some(nv + ne + f.index())
            .ok_or(GroupError::Data)
    };
    let n = nv + ne + nf;
    let mut sets = Sets {
        parent: (0..n).collect(),
        grouped: vec![false; n],
    };

    // Edges on one curve over overlapping ranges: in order of curve and
    // start, each edge starting before the furthest end so far on its
    // curve overlaps the edge reaching it, which joins the components
    // every overlapping pair would.
    let mut by_curve: Vec<(CurveId, Rational, Rational, usize)> = Vec::with_capacity(ne);
    for (i, e) in op.edges.iter().enumerate() {
        let [t0, t1] = e.t_range.map(Rational::try_from);
        let (Ok(t0), Ok(t1)) = (t0, t1) else {
            return Err(GroupError::Data);
        };
        by_curve.push((e.curve, t0, t1, i));
    }
    by_curve.sort();
    let mut reach: Option<(CurveId, Rational, usize)> = None;
    for (c, t0, t1, i) in by_curve {
        work.check(1);
        match &mut reach {
            Some((rc, end, j)) if *rc == c => {
                if t0 < *end {
                    sets.join(nv + *j, nv + i, true, work);
                }
                if t1 > *end {
                    *end = t1;
                    *j = i;
                }
            }
            _ => reach = Some((c, t1, i)),
        }
    }

    // Coedges on one p-curve: their edges.
    let mut by_pcurve: Vec<(u32, EdgeId)> = op.coedges.iter().map(|c| (c.pcurve, c.edge)).collect();
    by_pcurve.sort();
    for w in by_pcurve.windows(2) {
        if w[0].0 == w[1].0 && w[0].1 != w[1].1 {
            sets.join(e_at(w[0].1)?, e_at(w[1].1)?, true, work);
        }
    }

    // Contact members.
    let at = |m: Member| match m {
        Member::Vert(v) => v_at(v),
        Member::Edge(e) => e_at(e),
        Member::Face(f) => f_at(f),
    };
    for c in op.contacts {
        let mut ms = c.iter();
        let Some(&first) = ms.next() else { continue };
        let first = at(first)?;
        for &m in ms {
            sets.join(first, at(m)?, true, work);
        }
    }

    // Opposed faces on one surface object with overlapping trims.
    let mut by_surface: Vec<(SurfId, usize)> = op
        .faces
        .iter()
        .enumerate()
        .map(|(i, f)| (f.surface, i))
        .collect();
    by_surface.sort();
    let mut lo = 0;
    while lo < by_surface.len() {
        let s = by_surface[lo].0;
        let hi = lo + by_surface[lo..].iter().take_while(|x| x.0 == s).count();
        let bucket = &by_surface[lo..hi];
        for (k, &(_, i)) in bucket.iter().enumerate() {
            for &(_, j) in &bucket[k + 1..] {
                let (fi, fj) = (&op.faces[i], &op.faces[j]);
                work.check(1);
                if fi.flipped == fj.flipped {
                    continue;
                }
                let ids = [FaceId(i as u32), FaceId(j as u32)];
                let (Some(pi), Some(pj)) = (&fi.trims, &fj.trims) else {
                    return Err(GroupError::CurvedTrims(ids));
                };
                if trims_overlap(pi, pj, work).ok_or(GroupError::Data)? {
                    sets.join(nv + ne + i, nv + ne + j, true, work);
                }
            }
        }
        lo = hi;
    }

    // Closed downward, from every element already in a group.
    let in_group = |sets: &mut Sets, i: usize| {
        let r = sets.root(i);
        sets.grouped[r]
    };
    for (i, f) in op.faces.iter().enumerate() {
        let fi = nv + ne + i;
        if !in_group(&mut sets, fi) {
            continue;
        }
        for &e in &f.edges {
            sets.join(fi, e_at(e)?, false, work);
        }
        for &v in &f.verts {
            sets.join(fi, v_at(v)?, false, work);
        }
    }
    for (i, e) in op.edges.iter().enumerate() {
        let ei = nv + i;
        if !in_group(&mut sets, ei) {
            continue;
        }
        for v in e.verts {
            sets.join(ei, v_at(v)?, false, work);
        }
    }

    // The groups, numbered by their least member.
    let mut index = vec![NO_GROUP; n];
    let mut out = Groups {
        groups: Vec::new(),
        vert: vec![NO_GROUP; nv],
        edge: vec![NO_GROUP; ne],
        face: vec![NO_GROUP; nf],
    };
    for i in 0..n {
        let r = sets.root(i);
        if !sets.grouped[r] {
            continue;
        }
        if index[r] == NO_GROUP {
            index[r] = u32::try_from(out.groups.len()).map_err(|_| GroupError::Data)?;
            out.groups.push(Group::default());
        }
        let g = index[r];
        let group = &mut out.groups[g as usize];
        if i < nv {
            group.verts.push(VertId(i as u32));
            out.vert[i] = g;
        } else if i < nv + ne {
            group.edges.push(EdgeId((i - nv) as u32));
            out.edge[i - nv] = g;
        } else {
            group.faces.push(FaceId((i - nv - ne) as u32));
            out.face[i - nv - ne] = g;
        }
    }
    Ok(out)
}

/// The stored normals of every face at a member of `group`: the member
/// faces, the faces of its edges and of its vertices, each once, in face
/// order. `None` for an id past its kind's elements.
pub fn group_normals(op: &Operand<'_>, group: &Group) -> Option<Vec<DVec3>> {
    let mut faces: Vec<FaceId> = group.faces.clone();
    for &e in &group.edges {
        faces.extend(&op.edges.get(e.index())?.faces);
    }
    for &v in &group.verts {
        faces.extend(&op.verts.get(v.index())?.faces);
    }
    faces.sort();
    faces.dedup();
    faces
        .iter()
        .map(|f| op.faces.get(f.index()).map(|f| f.normal))
        .collect()
}

/// A coincidence group's direction ([`group_dir_flat`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroupDir {
    /// The max–min direction over the group's faces, agreeing with each.
    Common(Q3),
    /// The flush-patch direction over the faces with coincident patches.
    Flush(Q3),
    /// No direction agrees and the patches are of both kinds: the
    /// opposed-patch rule decides the opposed members; motion 1 is zero
    /// for the rest of the group.
    Opposed,
    /// No direction: `d = 0`, the generic translations decide.
    Zero,
}

impl GroupDir {
    /// The direction; zero without one.
    pub fn d(&self) -> Q3 {
        match self {
            GroupDir::Common(d) | GroupDir::Flush(d) => d.clone(),
            GroupDir::Opposed | GroupDir::Zero => dir::zero(),
        }
    }

    /// The rate of every member point along motion 1 (before `σ_A`):
    /// the direction, exact, its denominators cleared.
    pub fn rate(&self) -> Rate {
        Rate::cleared(self.d())
    }
}

/// A coincidence group's direction on flat faces (module docs): the
/// max–min direction over `normals`, the stored outward normals of every
/// face at a member ([`group_normals`]), if its minimum is `> 0`; else the
/// flush-patch direction over `patches`, the group's faces with
/// coincident `B` patches; else the opposed-patch rule for patches of both
/// kinds; else none. `None` without a face, or for a zero normal or one
/// that isn't finite. The programs are counted in `work`.
pub fn group_dir_flat(
    normals: &[DVec3],
    patches: &[Patch],
    work: &mut DirWork,
) -> Option<GroupDir> {
    if let FlatDir::MaxMin { d, .. } = dir::max_min_flat(normals, work)? {
        return Some(GroupDir::Common(d));
    }
    Some(match flush_flat(patches, work)? {
        Flush::Dir(f) => GroupDir::Flush(f.d()),
        Flush::Opposed => GroupDir::Opposed,
        Flush::Saddle | Flush::Escalate => GroupDir::Zero,
    })
}

/// Where a group's blend reaches from, exactly: its member vertices and
/// member edges (as the segments between their vertices), and its blend
/// radius `r_G`, the largest of the member vertices' radii.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reach {
    points: Vec<Q3>,
    segs: Vec<[Q3; 2]>,
    r: Option<Rational>,
}

impl Reach {
    /// The reach of `group` in `op`. `None` for an id past its kind's
    /// elements or a position that isn't finite.
    pub fn of(op: &Operand<'_>, group: &Group) -> Option<Reach> {
        let at = |v: VertId| op.verts.get(v.index()).and_then(|v| dir::exact(v.at));
        let mut r: Option<Rational> = None;
        let mut points = Vec::with_capacity(group.verts.len());
        for &v in &group.verts {
            let gv = op.verts.get(v.index())?;
            points.push(at(v)?);
            if gv.r.is_finite() && gv.r > 0.0 {
                let rv = Rational::try_from(gv.r).ok()?;
                if r.as_ref().is_none_or(|r| rv > *r) {
                    r = Some(rv);
                }
            }
        }
        let segs = group
            .edges
            .iter()
            .map(|&e| {
                let [a, b] = op.edges.get(e.index())?.verts;
                Some([at(a)?, at(b)?])
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Reach { points, segs, r })
    }

    /// The blend weight at `x`: `λ = clamp(dist/r_G − 1, 0, 1)`, `dist`
    /// the max-norm distance from `x` to the nearest member vertex or edge,
    /// exactly; `1` (no blend) without a radius or a member. The distance
    /// tests are counted in `work`.
    pub fn weight(&self, x: &Q3, work: &mut DirWork) -> Rational {
        let one = Rational::from(1);
        let Some(r) = &self.r else { return one };
        work.check(self.points.len() + self.segs.len());
        let near = self
            .points
            .iter()
            .map(|p| dist_inf(p, x))
            .chain(self.segs.iter().map(|[a, b]| seg_dist_inf(a, b, x)))
            .min();
        let Some(dist) = near else { return one };
        (dist / r - Rational::from(1)).clamp(Rational::from(0), one)
    }
}

/// The direction at a point `x` of an element that isn't a member, near
/// a group (module docs): `(1 − λ)·d_G + λ·d(x)` with `λ` the group's
/// weight at `x` ([`Reach::weight`]), if the group has a direction and it
/// agrees with every face of the element (`normals`, its stored outward
/// normals), exactly; else `own`, the element's own direction. Not
/// normalised. The tests are counted in `work`.
pub fn blend_flat(
    group: &GroupDir,
    reach: &Reach,
    own: &Q3,
    normals: &[DVec3],
    x: &Q3,
    work: &mut DirWork,
) -> Q3 {
    let d_g = match group {
        GroupDir::Common(d) | GroupDir::Flush(d) => d,
        GroupDir::Opposed | GroupDir::Zero => return own.clone(),
    };
    let l = reach.weight(x, work);
    if l == 1u32 {
        return own.clone();
    }
    work.check(normals.len());
    if !dir::agrees(normals, d_g) {
        return own.clone();
    }
    mix_flat(d_g, own, &l)
}

fn abs(x: Rational) -> Rational {
    if x < 0u32 { -x } else { x }
}

/// `‖x − p‖∞`, exactly.
fn dist_inf(p: &Q3, x: &Q3) -> Rational {
    (0..3).map(|i| abs(&x[i] - &p[i])).max().unwrap_or_default()
}

/// The max-norm distance from `x` to the segment `[a, b]`, exactly: the
/// least of `max_i |x_i − a_i − t·v_i|` over `t ∈ [0, 1]`, `v = b − a`, a
/// convex piecewise-linear function, at its ends or a break (a component
/// zero, or two of equal magnitude).
fn seg_dist_inf(a: &Q3, b: &Q3, x: &Q3) -> Rational {
    let v: Q3 = [0, 1, 2].map(|i| &b[i] - &a[i]);
    let w: Q3 = [0, 1, 2].map(|i| &x[i] - &a[i]);
    let at = |t: &Rational| {
        (0..3)
            .map(|i| abs(&w[i] - t * &v[i]))
            .max()
            .unwrap_or_default()
    };
    let (zero, one) = (Rational::from(0), Rational::from(1));
    let mut ts = vec![zero.clone(), one.clone()];
    for i in 0..3 {
        if v[i] != 0u32 {
            ts.push(&w[i] / &v[i]);
        }
        for j in i + 1..3 {
            // w_i − t·v_i = ±(w_j − t·v_j).
            for s in [1i32, -1] {
                let s = Rational::from(s);
                let den = &v[i] - &s * &v[j];
                if den != 0u32 {
                    ts.push((&w[i] - &s * &w[j]) / den);
                }
            }
        }
    }
    ts.into_iter()
        .filter(|t| *t >= zero && *t <= one)
        .map(|t| at(&t))
        .min()
        .unwrap_or_else(|| at(&zero))
}

/// Polygons on a plane in a chart of the plane: each point with the
/// coordinate along the normal's largest component (the first of equal
/// ones) dropped, the others in axis order. A bijection of the plane onto
/// the coordinate plane, the same for every face of one plane object, so
/// overlaps are kept ([`trims_overlap`]).
pub fn chart(normal: DVec3, loops: &[Vec<DVec3>]) -> Vec<Vec<[f64; 2]>> {
    let a = normal.abs();
    let drop = if a.x >= a.y && a.x >= a.z {
        0
    } else if a.y >= a.z {
        1
    } else {
        2
    };
    let keep = match drop {
        0 => [1, 2],
        1 => [0, 2],
        _ => [0, 1],
    };
    loops
        .iter()
        .map(|l| l.iter().map(|p| keep.map(|i| p[i])).collect())
        .collect()
}

type Q2 = [Rational; 2];

fn sub2(a: &Q2, b: &Q2) -> Q2 {
    [&a[0] - &b[0], &a[1] - &b[1]]
}

fn cross2(a: &Q2, b: &Q2) -> Rational {
    &a[0] * &b[1] - &a[1] * &b[0]
}

fn dot2(a: &Q2, b: &Q2) -> Rational {
    &a[0] * &b[0] + &a[1] * &b[1]
}

/// A region's boundary as segments, exactly, zero ones dropped. `None`
/// for a value that isn't finite.
fn segments(loops: &[Vec<[f64; 2]>]) -> Option<Vec<[Q2; 2]>> {
    let mut out = Vec::new();
    for l in loops {
        let pts = l
            .iter()
            .map(|p| {
                Some([
                    Rational::try_from(p[0]).ok()?,
                    Rational::try_from(p[1]).ok()?,
                ])
            })
            .collect::<Option<Vec<Q2>>>()?;
        for (i, a) in pts.iter().enumerate() {
            let b = &pts[(i + 1) % pts.len()];
            if a != b {
                out.push([a.clone(), b.clone()]);
            }
        }
    }
    Some(out)
}

/// Whether the regions bounded by the loops `p` and `q` (each a face's
/// trims in one chart, inside by the even–odd rule) share an area: their
/// interiors meet. Exact on the stored points: each boundary segment of
/// either region is cut where the other's boundary meets it, and the
/// sides of each piece's midpoint are read against both regions (inside
/// one side of `p` and the same side of `q`); a region of their common
/// interior has such a piece on its boundary. Touching along an edge or
/// at a point is no overlap. The segment pairs and point tests are
/// counted in `work`. `None` for a value that isn't finite.
pub fn trims_overlap(p: &[Vec<[f64; 2]>], q: &[Vec<[f64; 2]>], work: &mut DirWork) -> Option<bool> {
    let (p, q) = (segments(p)?, segments(q)?);
    if p.is_empty() || q.is_empty() {
        return Some(false);
    }
    // Boxes apart (or touching only): no common interior.
    let bounds = |s: &[[Q2; 2]], i: usize| {
        let it = s.iter().flat_map(|[a, b]| [&a[i], &b[i]]);
        (it.clone().min().cloned(), it.max().cloned())
    };
    for i in 0..2 {
        let ((Some(plo), Some(phi)), (Some(qlo), Some(qhi))) = (bounds(&p, i), bounds(&q, i))
        else {
            return Some(false);
        };
        if phi <= qlo || qhi <= plo {
            return Some(false);
        }
    }
    for (src, other) in [(&p, &q), (&q, &p)] {
        for [a, b] in src.iter() {
            let r = sub2(b, a);
            let mut ts = vec![Rational::from(0), Rational::from(1)];
            work.check(other.len());
            for [c, d] in other.iter() {
                cuts(a, &r, c, d, &mut ts);
            }
            ts.sort();
            ts.dedup();
            let left = [-&r[1], r[0].clone()];
            let right = [r[1].clone(), -&r[0]];
            for w in ts.windows(2) {
                let t = (&w[0] + &w[1]) / Rational::from(2);
                let m = [&a[0] + &t * &r[0], &a[1] + &t * &r[1]];
                work.check(2 * (src.len() + other.len()));
                for side in [&left, &right] {
                    if inside(src, &m, side) && inside(other, &m, side) {
                        return Some(true);
                    }
                }
            }
        }
    }
    Some(false)
}

/// Adds to `ts` the parameters in `(0, 1)` along `a + t·r` where the
/// segment `[c, d]` meets it: a crossing, either end of `[c, d]` on it,
/// or the ends of a collinear overlap.
fn cuts(a: &Q2, r: &Q2, c: &Q2, d: &Q2, ts: &mut Vec<Rational>) {
    let s = sub2(d, c);
    let ca = sub2(c, a);
    let den = cross2(r, &s);
    let (zero, one) = (Rational::from(0), Rational::from(1));
    let mut push = |t: Rational| {
        if t > zero && t < one {
            ts.push(t);
        }
    };
    if den != 0u32 {
        let t = cross2(&ca, &s) / &den;
        let u = cross2(&ca, r) / &den;
        if u >= zero && u <= one {
            push(t);
        }
    } else if cross2(&ca, r) == 0u32 {
        let rr = dot2(r, r);
        push(dot2(&ca, r) / &rr);
        push(dot2(&sub2(d, a), r) / &rr);
    }
}

/// Whether the point `m + ε·n`, for every small enough `ε > 0`, is inside
/// the region bounded by `segs` (even–odd), `m` off every segment but
/// those through it along `⊥ n`: the crossings of the ray `m + λ·n`, `λ >
/// 0`, a segment counting where its ends are on opposite sides of the
/// ray's line, one on it counting as the negative side (a turn of the ray
/// by less than any angle, so no end is ever on it).
fn inside(segs: &[[Q2; 2]], m: &Q2, n: &Q2) -> bool {
    let mut odd = false;
    for [a, b] in segs {
        let (sa, sb) = (cross2(n, &sub2(a, m)), cross2(n, &sub2(b, m)));
        if (sa > 0u32) == (sb > 0u32) {
            continue;
        }
        // The crossing `a + τ·(b − a)` with the line, ahead of `m`.
        let tau = &sa / (&sa - &sb);
        let x = [
            &a[0] + &tau * (&b[0] - &a[0]),
            &a[1] + &tau * (&b[1] - &a[1]),
        ];
        if dot2(n, &sub2(&x, m)) > 0u32 {
            odd = !odd;
        }
    }
    odd
}

#[cfg(test)]
mod tests;
