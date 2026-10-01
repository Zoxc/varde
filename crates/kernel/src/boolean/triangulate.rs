//! Triangulating the part of a patch's parameter domain that a boolean
//! keeps: loops of vertices, outer ones counter-clockwise and holes
//! clockwise, into triangles on those vertices.
//!
//! The loops come from the counting, so they are right for the perturbed
//! operands however close their vertices' positions are: flush faces
//! give loops of zero width, whose points coincide. So the triangulation
//! always completes whenever the loops can be triangulated at all: it
//! first cuts off ears with two corners at one position (so loops of zero
//! width come apart into triangles of zero width), then proper triangles
//! with nothing in or on them (nor, with curved sides, a rounding off
//! the diagonal they leave), then ears of zero area, then any ear, and
//! leaves degenerate triangles to the clean-up after it. It never adds a
//! diagonal between two vertices on one side of the domain triangle (they
//! would lie along it, and the patch beside that side could add the same
//! one), nor one that is already an edge. Then diagonals are flipped
//! towards the Delaunay triangulation.
//!
//! Sides of the loops may be curves ([`Bends`]: their tangents at their
//! ends). A triangle is then only a proper ear if its corners are open
//! between those tangents and the straight sides (a straight side along
//! a curve's tangent at a corner makes a patch whose corner is
//! degenerate), and flips keep them open.
//!
//! Extrude's caps use `spade`'s constrained Delaunay triangulation
//! instead: it merges coincident points, which these loops have, while
//! caps have none but can have many thousand vertices, where ear
//! clipping is too slow.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};

use glam::DVec2;

use super::BooleanError;
use super::exact::{exact_count, orient2d_towards};

/// A vertex of a loop: its id, where it is in the domain, the sides of
/// the domain triangle it lies on (bit `i` for the side from corner `i`
/// to corner `i + 1`), and the cuts along the face it lies on, if any
/// ([`NO_CUT`] for none).
#[derive(Debug, Clone, Copy)]
pub(super) struct Vert {
    pub(super) id: u32,
    pub(super) at: DVec2,
    pub(super) sides: u8,
    pub(super) cuts: [u32; 2],
}

/// No cut, in [`Vert::cuts`].
pub(super) const NO_CUT: u32 = u32::MAX;

/// Where vertices inside the domain move towards, infinitely little, to
/// break their ties with its sides: a point inside the domain triangle.
const CENTER: DVec2 = DVec2::new(0.25, 0.25);

impl Vert {
    /// Whether it lies inside the domain (on none of its sides), however
    /// close to a side its position is: then it counts as moved towards
    /// [`CENTER`] by an infinitely small fraction of the way.
    fn inside(&self) -> bool {
        self.sides == 0
    }

    /// Whether it is at the same place as `other`, as far as positions
    /// tell: within [`SHORT`] (vertices of tied operands whose positions
    /// came by different roundings are a few ulps apart, and a cut's
    /// vertex inside the domain a moment off a side it lies on, as where
    /// a face is flush with the other operand along its side).
    fn same(&self, other: &Vert) -> bool {
        self.at.distance(other.at) <= SHORT
    }

    /// Whether a diagonal may not join it to `other`: both lie on one
    /// side of the domain (the diagonal would run along it, and the patch
    /// beyond that side could add the same edge), or on one cut, which
    /// the face beyond the cut could join them across too.
    fn along_one(&self, other: &Vert) -> bool {
        self.sides & other.sides != 0
            || self
                .cuts
                .iter()
                .any(|&c| c != NO_CUT && other.cuts.contains(&c))
    }
}

/// The sign of `(b − a) × (c − a)`, exactly, with the vertices inside the
/// domain moved as [`Vert::inside`] says.
fn orient(a: &Vert, b: &Vert, c: &Vert) -> i8 {
    orient2d_towards([a, b, c].map(|v| (v.at, v.inside())), CENTER)
}

thread_local! {
    /// [`exact_count`] where [`exact_steps`] last looked. A face is
    /// triangulated on one thread, start to end, so what it counts is its
    /// own, however the faces are shared out.
    static SEEN: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// What an orientation worked out exactly counts for in steps: some two
/// microseconds, where a step is a tenth of that. A face whose vertices
/// all lie near a line (cuts along one) had its ear clipping decide
/// almost every orientation exactly, a million of them a round, 1.3 s
/// counted as 0.08 s of work.
const EXACT_STEPS: usize = 64;

/// The steps the orientations worked out exactly since the last call
/// stand for.
fn exact_steps() -> usize {
    let now = exact_count();
    SEEN.with(|seen| now.wrapping_sub(seen.replace(now)))
        .saturating_mul(EXACT_STEPS)
}

/// How many vertices a polygon may have for the best ear to be looked for
/// among all of them each time; past it the first proper ear is taken.
const SEARCH: usize = 64;

/// The loops' curved sides, by the ids they run from and to, as the
/// loops run them: the curve's tangent where it leaves its start, and
/// where it leaves its end back towards the start. Sides not in it are
/// straight.
pub(super) type Bends = BTreeMap<(u32, u32), [DVec2; 2]>;

/// Sides shorter than this (the domain is about 1 across) are where
/// vertices of flush or tied operands coincide, whose corners aren't
/// judged.
const SHORT: f64 = 1e-9;

/// The smallest sine of a triangle's angle between a curved side's
/// tangent and its other side at a corner, relative to their lengths.
const SIN_OPEN: f64 = 1e-3;

/// The corners of the triangle `tri` (counter-clockwise) that a curved
/// side meets and that aren't open: the angle from one side's direction
/// out of the corner to the other's, counter-clockwise, within
/// [`SIN_OPEN`] of 0° or past 180°, with curved sides leaving along their
/// tangents. Each as the curved sides there: the one leaving it, then the
/// one arriving. Corners of straight sides alone, and of sides about zero
/// long, aren't looked at.
fn closed_corners(tri: [&Vert; 3], bends: &Bends) -> Vec<[Option<(u32, u32)>; 2]> {
    if bends.is_empty() {
        return Vec::new();
    }
    let side = |i: usize| (tri[i].id, tri[(i + 1) % 3].id);
    (0..3)
        .filter_map(|i| {
            let (prev, next) = ((i + 2) % 3, (i + 1) % 3);
            let (out, into) = (side(i), side(prev));
            let (bo, bi) = (bends.get(&out), bends.get(&into));
            if bo.is_none() && bi.is_none() {
                return None;
            }
            let (v, a, b) = (tri[i].at, tri[next].at, tri[prev].at);
            if a.distance(v) <= SHORT || b.distance(v) <= SHORT {
                // A side of about zero length, where the operands are
                // flush or tie: the clean-up takes it out.
                return None;
            }
            let leave = bo.map_or(a - v, |t| t[0]);
            let back = bi.map_or(b - v, |t| t[1]);
            let open = leave.perp_dot(back) > SIN_OPEN * leave.length() * back.length();
            (!open).then_some([bo.map(|_| out), bi.map(|_| into)])
        })
        .collect()
}

/// Whether the triangle `tri` is about flat: twice its area within
/// `1e-9` of its longest side's square (in the domain, about 1 across,
/// its corners on a line up to rounding).
fn thin(tri: [&Vert; 3]) -> bool {
    let [a, b, c] = tri.map(|v| v.at);
    let longest = (b - a)
        .length_squared()
        .max((c - b).length_squared())
        .max((a - c).length_squared());
    (b - a).perp_dot(c - a).abs() <= 1e-9 * longest
}

/// Whether the corners of the triangle `tri` along its curved sides are
/// all open (see [`closed_corners`]).
fn corners_open(tri: [&Vert; 3], bends: &Bends) -> bool {
    closed_corners(tri, bends).is_empty()
}

/// How many rounds of Steiner points mend corners along curved sides.
const MEND_ROUNDS: usize = 4;

/// The steps the triangulations of one round of cutting faces take, a
/// step about a vertex tested against a diagonal or a triangle looked at,
/// counted together by the faces cut in parallel, and the most there may
/// be: past it every triangulation stops. Whether they get past it
/// depends only on how many steps they take all told, not on the order
/// the threads count them in.
pub(super) struct Meter {
    used: AtomicU64,
    limit: u64,
}

impl Meter {
    pub(super) fn new(limit: u64) -> Meter {
        Meter {
            used: AtomicU64::new(0),
            limit,
        }
    }

    /// Counts `n` more steps; false once past the limit.
    fn take(&self, n: usize) -> bool {
        let n = u64::try_from(n).unwrap_or(u64::MAX);
        let before = self.used.fetch_add(n, Ordering::Relaxed);
        before.saturating_add(n) <= self.limit
    }

    /// Whether the steps went past the limit.
    pub(super) fn over(&self) -> bool {
        self.used.load(Ordering::Relaxed) > self.limit
    }

    /// The steps taken.
    pub(super) fn used(&self) -> u64 {
        self.used.load(Ordering::Relaxed)
    }
}

/// A triangulation: its triangles, counter-clockwise, the points it
/// added inside, and the curved sides that should be split for its
/// corners to open (see [`triangulate`]).
#[derive(Debug, Clone)]
pub(super) struct Triangulation {
    pub(super) tris: Vec<[u32; 3]>,
    pub(super) steiner: Vec<Vert>,
    pub(super) split: Vec<(u32, u32)>,
}

/// The triangles, counter-clockwise, covering the region the loops bound.
///
/// Where a triangle's corner between two curved sides isn't open, as
/// where two arcs of one smooth curve meet (which any triangle having
/// both as sides folds at), a point is added at the triangle's centroid,
/// and the triangle split at it, in a few rounds: those points are
/// numbered from `first_steiner` and returned too. Where a curved side's
/// corner with a straight one isn't open (the curve leaves the triangle
/// there, bulging past its other side), no point in the triangle mends
/// it: the curved side is returned, to be split.
pub(super) fn triangulate(
    loops: Vec<Vec<Vert>>,
    bends: &Bends,
    first_steiner: u32,
    meter: &Meter,
) -> Result<Triangulation, BooleanError> {
    // Only this face's exact orientations are counted from here, and
    // what they took is counted at the end too.
    exact_steps();
    let out = triangulate_counted(loops, bends, first_steiner, meter);
    if !meter.take(exact_steps()) {
        return Err(BooleanError::Degenerate);
    }
    out
}

/// [`triangulate`], counting its steps as it goes.
fn triangulate_counted(
    loops: Vec<Vec<Vert>>,
    bends: &Bends,
    first_steiner: u32,
    meter: &Meter,
) -> Result<Triangulation, BooleanError> {
    // A loop of two vertices (two curves between the same two points)
    // has no triangle: its curves are to be split, and until they are it
    // is left out, which the mesh then fails to close over.
    let (short, loops): (Vec<Vec<Vert>>, Vec<Vec<Vert>>) =
        loops.into_iter().partition(|l| l.len() < 3);
    let lens_sides: Vec<(u32, u32)> = short
        .iter()
        .flat_map(|l| (0..l.len()).map(move |i| (l[i].id, l[(i + 1) % l.len()].id)))
        .filter(|side| bends.contains_key(side))
        .collect();
    if loops.is_empty() {
        return if lens_sides.is_empty() {
            Err(BooleanError::Degenerate)
        } else {
            Ok(Triangulation {
                tris: Vec::new(),
                steiner: Vec::new(),
                split: lens_sides,
            })
        };
    }
    let mut all: Vec<Vert> = loops.iter().flatten().copied().collect();
    // The loops' sides, which flips leave.
    let mut fixed: BTreeSet<(u32, u32)> = loops.iter().flat_map(|l| sides_of(l)).collect();
    let mut bends = bends.clone();
    let mut source = BTreeMap::new();
    let tris = triangulate_loops(loops, &mut bends, &mut fixed, &mut source, meter)?;
    let bends = &bends;
    let mut out = Triangulation {
        tris,
        steiner: Vec::new(),
        split: lens_sides,
    };
    if bends.is_empty() {
        return Ok(out);
    }
    for _ in 0..MEND_ROUNDS {
        if !meter.take(out.tris.len().saturating_add(exact_steps())) {
            return Err(BooleanError::Degenerate);
        }
        all.sort_by_key(|v| v.id);
        let at = |id: u32| all[all.binary_search_by_key(&id, |v| v.id).expect("a vertex")];
        // Triangles with a corner between two curved sides that isn't
        // open take a point; others ask for their curved sides to be
        // split. Triangles of about zero width, where the operands tie,
        // are the clean-up's.
        let mut bad = Vec::new();
        for t in 0..out.tris.len() {
            let tri = out.tris[t].map(&at);
            // Of zero width in the layout too (its corners on a line: a
            // band between two curves lying on each other, where
            // operands are flush), no point or split mends it either.
            if (0..3).any(|i| tri[i].at.distance(tri[(i + 1) % 3].at) <= SHORT)
                || thin(tri.each_ref())
            {
                continue;
            }
            let closed = closed_corners(tri.each_ref(), bends);
            if closed.iter().any(|c| c[0].is_some() && c[1].is_some()) {
                bad.push(t);
            } else {
                // A diagonal standing for a curve (see `clip`) asks for
                // that curve.
                out.split.extend(
                    closed
                        .iter()
                        .flatten()
                        .flatten()
                        .map(|k| source.get(k).copied().unwrap_or(*k)),
                );
            }
        }
        if bad.is_empty() {
            break;
        }
        let mut added = Vec::new();
        for t in bad {
            let [a, b, c] = out.tris[t];
            let id = first_steiner + (out.steiner.len() + added.len()) as u32;
            let s = Vert {
                id,
                at: (at(a).at + at(b).at + at(c).at) / 3.0,
                sides: 0,
                cuts: [NO_CUT; 2],
            };
            added.push(s);
            out.tris[t] = [id, a, b];
            out.tris.push([id, b, c]);
            out.tris.push([id, c, a]);
        }
        all.extend(&added);
        out.steiner.extend(added);
        all.sort_by_key(|v| v.id);
        improve(&mut out.tris, &all, &fixed, bends, meter);
    }
    out.split.sort_unstable();
    out.split.dedup();
    Ok(out)
}

/// [`triangulate`] without the mending.
fn triangulate_loops(
    loops: Vec<Vec<Vert>>,
    bends: &mut Bends,
    kept: &mut BTreeSet<(u32, u32)>,
    source: &mut BTreeMap<(u32, u32), (u32, u32)>,
    meter: &Meter,
) -> Result<Vec<[u32; 3]>, BooleanError> {
    let mut outers = Vec::new();
    let mut holes = Vec::new();
    for l in loops {
        // A loop along the domain's sides bounds the region inside it;
        // others by their orientation.
        if l.iter().any(|v| v.sides != 0) || area(&l) >= 0.0 {
            outers.push(l);
        } else {
            holes.push(l);
        }
    }
    if outers.is_empty() {
        return Err(BooleanError::Degenerate);
    }
    let mut owned: Vec<Vec<Vec<Vert>>> = vec![Vec::new(); outers.len()];
    for hole in holes {
        let inside = (0..outers.len())
            .filter(|&o| winding(&outers[o], &hole[0]) != 0)
            .min_by(|&x, &y| area(&outers[x]).total_cmp(&area(&outers[y])));
        let o = inside.unwrap_or_else(|| {
            (0..outers.len())
                .max_by(|&x, &y| area(&outers[x]).total_cmp(&area(&outers[y])))
                .expect("an outer loop")
        });
        owned[o].push(hole);
    }
    let mut tris = Vec::new();
    for (outer, holes) in outers.into_iter().zip(owned) {
        // Bridging looks at every side for each vertex tried.
        let size: usize = outer.len() + holes.iter().map(Vec::len).sum::<usize>();
        if !meter.take(size.saturating_mul(size).saturating_add(exact_steps())) {
            return Err(BooleanError::Degenerate);
        }
        let poly = bridge(outer, holes);
        let from = tris.len();
        let mut fixed = sides_of(&poly);
        let verts = poly.clone();
        clip(poly, &mut tris, bends, &mut fixed, source, meter)?;
        improve(&mut tris[from..], &verts, &fixed, bends, meter);
        kept.extend(fixed);
    }
    Ok(tris)
}

/// Twice the signed area.
fn area(l: &[Vert]) -> f64 {
    let n = l.len();
    (0..n)
        .map(|i| {
            let (p, q) = (l[i].at, l[(i + 1) % n].at);
            p.x * q.y - p.y * q.x
        })
        .sum()
}

/// The winding number of `l` round `p`.
fn winding(l: &[Vert], p: &Vert) -> i32 {
    let n = l.len();
    let mut w = 0;
    for i in 0..n {
        let (a, b) = (&l[i], &l[(i + 1) % n]);
        if a.at.y <= p.at.y {
            if b.at.y > p.at.y && orient(a, b, p) > 0 {
                w += 1;
            }
        } else if b.at.y <= p.at.y && orient(a, b, p) < 0 {
            w -= 1;
        }
    }
    w
}

/// The outer loop with its holes joined in by bridges: each hole's
/// rightmost vertex to the nearest vertex it can see, the bridge run both
/// ways, so one loop bounds the same region.
fn bridge(outer: Vec<Vert>, mut holes: Vec<Vec<Vert>>) -> Vec<Vert> {
    let right = |l: &[Vert]| {
        (0..l.len())
            .max_by(|&i, &j| {
                let (a, b) = (l[i], l[j]);
                a.at.x
                    .total_cmp(&b.at.x)
                    .then(a.at.y.total_cmp(&b.at.y))
                    .then(b.id.cmp(&a.id))
            })
            .expect("a vertex")
    };
    holes.sort_by(|x, y| {
        let (a, b) = (x[right(x)], y[right(y)]);
        b.at.x
            .total_cmp(&a.at.x)
            .then(b.at.y.total_cmp(&a.at.y))
            .then(a.id.cmp(&b.id))
    });
    let mut poly = outer;
    for k in 0..holes.len() {
        let hole = &holes[k];
        let hi = right(hole);
        let h = hole[hi];
        let mut order: Vec<usize> = (0..poly.len()).collect();
        order.sort_by(|&i, &j| {
            let (a, b) = (poly[i], poly[j]);
            a.at.distance_squared(h.at)
                .total_cmp(&b.at.distance_squared(h.at))
                .then(a.id.cmp(&b.id))
                .then(i.cmp(&j))
        });
        let others = &holes[k..];
        let pick = order
            .iter()
            .copied()
            .find(|&i| in_cone(&poly, i, &h) && sees(h, poly[i], &poly, others))
            .unwrap_or(order[0]);
        let mut joined = Vec::with_capacity(poly.len() + hole.len() + 2);
        joined.extend_from_slice(&poly[..=pick]);
        joined.extend((0..hole.len()).map(|j| hole[(hi + j) % hole.len()]));
        joined.push(h);
        joined.extend_from_slice(&poly[pick..]);
        poly = joined;
    }
    poly
}

/// Whether `p` lies in the polygon's interior angle at vertex `i`.
fn in_cone(poly: &[Vert], i: usize, p: &Vert) -> bool {
    let n = poly.len();
    let (prev, m, next) = (&poly[(i + n - 1) % n], &poly[i], &poly[(i + 1) % n]);
    let (left_in, left_out) = (orient(prev, m, p) > 0, orient(m, next, p) > 0);
    if orient(prev, m, next) >= 0 {
        left_in && left_out
    } else {
        left_in || left_out
    }
}

/// Whether the segment from `h` to `m` crosses no side of the polygon or
/// of the holes, nor passes through one of their vertices.
fn sees(h: Vert, m: Vert, poly: &[Vert], holes: &[Vec<Vert>]) -> bool {
    std::iter::once(poly)
        .chain(holes.iter().map(Vec::as_slice))
        .all(|l| {
            let n = l.len();
            (0..n).all(|i| {
                let (u, v) = (l[i], l[(i + 1) % n]);
                let ends = [h.id, m.id];
                if ends.contains(&u.id) || ends.contains(&v.id) {
                    return true;
                }
                let (ou, ov) = (orient(&h, &m, &u), orient(&h, &m, &v));
                if ou == 0 && between(h.at, m.at, u.at) {
                    return false;
                }
                if ou * ov >= 0 {
                    return true;
                }
                orient(&u, &v, &h) * orient(&u, &v, &m) > 0
            })
        })
}

/// Whether `p`, on the line through `a` and `b`, lies strictly between
/// them.
fn between(a: DVec2, b: DVec2, p: DVec2) -> bool {
    let d = b - a;
    let t = (p - a).dot(d);
    t > 0.0 && t < d.length_squared()
}

/// The polygon's sides, each way round, by ids.
fn sides_of(poly: &[Vert]) -> BTreeSet<(u32, u32)> {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i].id, poly[(i + 1) % n].id);
            (a.min(b), a.max(b))
        })
        .collect()
}

/// How many times [`improve`] may flip, per triangle.
const FLIPS: usize = 8;

/// Flips diagonals of the polygon's triangulation towards the Delaunay
/// one, which has no thin triangles the polygon doesn't force: a diagonal
/// whose quadrilateral is convex is flipped when the far corner lies
/// inside the circle through the near triangle's corners. Only proper
/// triangles are touched (none with two corners at one place), the new
/// diagonal must be allowed as the ear clipping's are, and the number of
/// flips is bounded.
fn improve(
    tris: &mut [[u32; 3]],
    verts: &[Vert],
    fixed: &BTreeSet<(u32, u32)>,
    bends: &Bends,
    meter: &Meter,
) {
    let mut by_id: Vec<Vert> = verts.to_vec();
    by_id.sort_by_key(|v| v.id);
    by_id.dedup_by_key(|v| v.id);
    let at = |id: u32| by_id[by_id.binary_search_by_key(&id, |v| v.id).expect("a vertex")];
    let key = |a: u32, b: u32| (a.min(b), a.max(b));
    // Directed edge → its triangle.
    let mut owner: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    for (t, tri) in tris.iter().enumerate() {
        for i in 0..3 {
            owner.insert((tri[i], tri[(i + 1) % 3]), t);
        }
    }
    let mut flips = FLIPS * tris.len();
    let mut changed = true;
    while changed && flips > 0 {
        changed = false;
        if !meter.take(tris.len().saturating_add(exact_steps())) {
            return;
        }
        for t in 0..tris.len() {
            for i in 0..3 {
                let (a, b, c) = (tris[t][i], tris[t][(i + 1) % 3], tris[t][(i + 2) % 3]);
                if fixed.contains(&key(a, b)) {
                    continue;
                }
                let Some(&s) = owner.get(&(b, a)) else {
                    continue;
                };
                let other = tris[s];
                let Some(d) = other.iter().copied().find(|&w| w != a && w != b) else {
                    continue;
                };
                let (va, vb, vc, vd) = (at(a), at(b), at(c), at(d));
                // Two corners at one place: a triangle of zero width, whose
                // short side the clean-up collapses, merging its two other
                // sides; flipped, the side it shares with a proper triangle
                // became a straight diagonal there that the collapse then
                // made the zero-width triangle's curve, two curved sides
                // meeting along one smooth curve, which folds.
                let quad = [&va, &vb, &vc, &vd];
                let coincident = (0..4).any(|i| (i + 1..4).any(|j| quad[i].same(quad[j])));
                if s == t
                    || c == d
                    || coincident
                    || vc.along_one(&vd)
                    || owner.contains_key(&(c, d))
                    || owner.contains_key(&(d, c))
                    || orient(&va, &vb, &vc) <= 0
                    || orient(&vb, &va, &vd) <= 0
                    || orient(&vc, &va, &vd) <= 0
                    || orient(&vc, &vd, &vb) <= 0
                    || !in_circle(va.at, vb.at, vc.at, vd.at)
                    || !corners_open([&vc, &va, &vd], bends)
                    || !corners_open([&vc, &vd, &vb], bends)
                {
                    continue;
                }
                for tri in [tris[t], tris[s]] {
                    for j in 0..3 {
                        owner.remove(&(tri[j], tri[(j + 1) % 3]));
                    }
                }
                tris[t] = [c, a, d];
                tris[s] = [c, d, b];
                for u in [t, s] {
                    let tri = tris[u];
                    for j in 0..3 {
                        owner.insert((tri[j], tri[(j + 1) % 3]), u);
                    }
                }
                changed = true;
                flips -= 1;
                if flips == 0 {
                    return;
                }
                break;
            }
        }
    }
}

/// Whether `d` lies clearly inside the circle through `a`, `b`, `c`
/// (counter-clockwise), in floating point: a flip only improves shapes,
/// so rounding decides nothing that matters, and the margin keeps
/// cocircular points from flipping back and forth.
fn in_circle(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let (a, b, c) = (a - d, b - d, c - d);
    let det = a.length_squared() * b.perp_dot(c) - b.length_squared() * a.perp_dot(c)
        + c.length_squared() * a.perp_dot(b);
    let scale = a.length_squared() * b.length() * c.length()
        + b.length_squared() * a.length() * c.length()
        + c.length_squared() * a.length() * b.length();
    det > 1e-9 * scale
}

/// Ear clipping, adding the triangles to `out`.
///
/// An ear with two corners at about one place, one of its sides from
/// them a curve (a tie left a vertex at the curve's end), leaves a
/// diagonal the clean-up will make that curve: it takes the curve's
/// tangents in `bends`, and is added to `fixed`, so no flip takes it;
/// `source` maps it to the curve (as the loops run it).
fn clip(
    mut ring: Vec<Vert>,
    out: &mut Vec<[u32; 3]>,
    bends: &mut Bends,
    fixed: &mut BTreeSet<(u32, u32)>,
    source: &mut BTreeMap<(u32, u32), (u32, u32)>,
    meter: &Meter,
) -> Result<(), BooleanError> {
    let key = |a: u32, b: u32| (a.min(b), a.max(b));
    let n = ring.len();
    let mut edges: BTreeSet<(u32, u32)> = (0..n)
        .map(|i| key(ring[i].id, ring[(i + 1) % n].id))
        .collect();
    let mut start = 0;
    while ring.len() > 3 {
        let n = ring.len();
        // Each ear looked at tests every vertex against it.
        let tried = std::cell::Cell::new(0usize);
        let ear = |ring: &[Vert], i: usize, edges: &BTreeSet<(u32, u32)>, bends: &Bends| {
            tried.set(tried.get() + 1);
            ear(ring, i, edges, bends)
        };
        let best = if n <= SEARCH {
            (0..n)
                .filter_map(|i| ear(&ring, i, &edges, bends).map(|e| (e, i)))
                .min_by(|(a, i), (b, j)| {
                    a.level
                        .cmp(&b.level)
                        .then(b.quality.total_cmp(&a.quality))
                        .then(i.cmp(j))
                })
        } else {
            // The first proper ear from where the last was cut, else the
            // best there is.
            (0..n)
                .map(|k| (start + k) % n)
                .find_map(|i| {
                    ear(&ring, i, &edges, bends)
                        .filter(|e| e.level <= 1)
                        .map(|e| (e, i))
                })
                .or_else(|| {
                    (0..n)
                        .filter_map(|i| ear(&ring, i, &edges, bends).map(|e| (e, i)))
                        .min_by(|(a, i), (b, j)| a.level.cmp(&b.level).then(i.cmp(j)))
                })
        };
        if !meter.take(tried.get().saturating_mul(n).saturating_add(exact_steps())) {
            return Err(BooleanError::Degenerate);
        }
        let Some((_, i)) = best else {
            return Err(BooleanError::Degenerate);
        };
        let (prev, cur, next) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
        out.push([prev.id, cur.id, next.id]);
        edges.insert(key(prev.id, next.id));
        let near = |a: &Vert, b: &Vert| a.at.distance(b.at) <= SHORT;
        let from = if near(&cur, &next) {
            Some((prev.id, cur.id))
        } else if near(&prev, &cur) {
            Some((cur.id, next.id))
        } else {
            None
        };
        if let Some(from) = from
            && let Some(&t) = bends.get(&from)
        {
            bends.insert((prev.id, next.id), t);
            fixed.insert(key(prev.id, next.id));
            let root = source.get(&from).copied().unwrap_or(from);
            source.insert((prev.id, next.id), root);
        }
        ring.remove(i);
        start = i % ring.len();
    }
    let [a, b, c] = [ring[0].id, ring[1].id, ring[2].id];
    if a == b || b == c || c == a {
        return Err(BooleanError::Degenerate);
    }
    out.push([a, b, c]);
    Ok(())
}

/// How good the ear at a vertex is: level 0 one with two corners at one
/// position (cutting it off takes out a zero-length side, as flush faces
/// make them), 1 a proper triangle with no other vertex in or on it or,
/// with curved sides, a rounding off its diagonal (the distance [`thin`]
/// takes as none), 2 a
/// proper one whose corners along a curved side aren't open (the curve
/// is split and the face cut again), 3 one of zero area with no other
/// vertex on it (the clean-up flips it away, which a curve beside it can
/// stop), 4 anything else; and within a level, its shape.
struct Ear {
    level: u8,
    quality: f64,
}

/// The ear cutting vertex `i` off, if its diagonal is allowed.
fn ear(ring: &[Vert], i: usize, edges: &BTreeSet<(u32, u32)>, bends: &Bends) -> Option<Ear> {
    let n = ring.len();
    let (p, c, q) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
    if p.id == q.id || p.id == c.id || c.id == q.id {
        return None;
    }
    if p.along_one(&q) || edges.contains(&(p.id.min(q.id), p.id.max(q.id))) {
        return None;
    }
    let turn = orient(&p, &c, &q);
    let coincident = p.same(&c) || c.same(&q) || p.same(&q);
    // The ends of a zero-area ear: its two corners furthest apart.
    let (s, e) = [(p, q), (p, c), (c, q)]
        .into_iter()
        .max_by(|x, y| {
            x.0.at
                .distance_squared(x.1.at)
                .total_cmp(&y.0.at.distance_squared(y.1.at))
        })
        .expect("three sides");
    let others = ring.iter().filter(|v| ![p.id, c.id, q.id].contains(&v.id));
    let level = if coincident || turn == 0 {
        // Nothing else on the segment it covers.
        let blocked = others
            .filter(|v| !v.same(&s) && !v.same(&e))
            .any(|v| orient(&s, &e, v) == 0 && between(s.at, e.at, v.at));
        match (blocked, coincident) {
            (true, _) => 4,
            (false, true) => 0,
            (false, false) => 3,
        }
    } else if turn > 0 {
        // Nothing else in or on it. A vertex at a corner's position (a
        // bridge's other end, loops touching) only if one of its sides
        // leaves it into the ear or along one of the ear's sides.
        let corners = [(q, p, c), (p, c, q), (c, q, p)];
        let ids = [p.id, c.id, q.id];
        let blocked = (0..n).any(|j| {
            let v = &ring[j];
            if ids.contains(&v.id)
                || orient(&p, &c, v) < 0
                || orient(&c, &q, v) < 0
                || orient(&q, &p, v) < 0
            {
                return false;
            }
            let Some(&(before, k, after)) = corners.iter().find(|(_, k, _)| k.same(v)) else {
                return true;
            };
            [ring[(j + n - 1) % n], ring[(j + 1) % n]].iter().any(|w| {
                !ids.contains(&w.id)
                    && !w.same(&k)
                    && orient(&before, &k, w) >= 0
                    && orient(&k, &after, w) >= 0
            })
        });
        // Nor any a rounding off its diagonal, outside it: cut off, it
        // leaves a pocket of zero width between the diagonal and them,
        // whose triangles can't be flipped away where its sides are
        // curves (a cut along the domain's side, its vertices on it
        // moved inwards alike, one of its ends on the next side a few
        // ulps in: three corners on the cut in the patch, of zero area).
        // Only with curved sides: among straight ones the clean-up flips
        // such a pocket away, and refusing the ear there costs flat faces
        // a better triangulation (turned grid boxes chained: one more
        // step refused as a fold).
        let grazed = || {
            let d = q.at - p.at;
            let length = d.length_squared();
            ring.iter().any(|v| {
                let w = v.at - p.at;
                let along = w.dot(d);
                !ids.contains(&v.id)
                    && !v.same(&p)
                    && !v.same(&q)
                    && along > 0.0
                    && along < length
                    && d.perp_dot(w).abs() <= 1e-9 * length
            })
        };
        // With four left, the triangle cutting this ear leaves is taken as
        // it is: its corners count too, and among curves, a remaining
        // triangle of zero area (which the clean-up would have to flip
        // across a curve) is as bad as a zero-area ear.
        let rest = [&q, &ring[(i + 2) % n], &p];
        let flat_rest = !bends.is_empty() && thin(rest);
        if blocked || !bends.is_empty() && grazed() {
            4
        } else if n == 4 && flat_rest || !bends.is_empty() && thin([&p, &c, &q]) {
            3
        } else if corners_open([&p, &c, &q], bends) && (n != 4 || corners_open(rest, bends)) {
            1
        } else {
            2
        }
    } else {
        4
    };
    let (e0, e1, e2) = (c.at - p.at, q.at - c.at, p.at - q.at);
    let size = e0.length_squared() + e1.length_squared() + e2.length_squared();
    let quality = if size > 0.0 {
        e0.perp_dot(-e2) / size
    } else {
        0.0
    };
    Some(Ear { level, quality })
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]
mod tests {
    use super::super::exact::orient2d;
    use super::*;

    fn loop_of(points: &[(f64, f64)], first: u32) -> Vec<Vert> {
        points
            .iter()
            .enumerate()
            .map(|(i, &(x, y))| Vert {
                id: first + i as u32,
                at: DVec2::new(x, y),
                sides: 0,
                cuts: [NO_CUT; 2],
            })
            .collect()
    }

    fn total_area(tris: &[[u32; 3]], at: impl Fn(u32) -> DVec2) -> f64 {
        tris.iter()
            .map(|&[a, b, c]| (at(b) - at(a)).perp_dot(at(c) - at(a)) / 2.0)
            .sum()
    }

    #[test]
    fn a_square_with_a_hole() {
        let outer = loop_of(&[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)], 0);
        let hole = loop_of(&[(1.0, 1.0), (1.0, 3.0), (3.0, 3.0), (3.0, 1.0)], 4);
        let all: Vec<Vert> = outer.iter().chain(&hole).copied().collect();
        let tris = triangulate(vec![outer, hole], &Bends::new(), 100, &Meter::new(u64::MAX))
            .unwrap()
            .tris;
        assert_eq!(tris.len(), 8);
        let at = |id: u32| all[id as usize].at;
        assert_eq!(total_area(&tris, at), 12.0);
        for &[a, b, c] in &tris {
            assert!(orient2d(at(a), at(b), at(c)) > 0);
        }
    }

    #[test]
    fn orientations_worked_out_exactly_are_counted() {
        // A band along a line, its vertices on its two sides, as cuts
        // along a straight edge leave them: nearly every orientation the
        // ear clipping asks is a tie floating point can't tell, some
        // hundred times the work of one it can, and each counts for
        // `EXACT_STEPS`. As many vertices round a circle take a fraction.
        let n = 40;
        let low = (0..=n).map(|i| (f64::from(i) / f64::from(n), 0.0));
        let high = (0..=n).rev().map(|i| (f64::from(i) / f64::from(n), 0.25));
        let band: Vec<(f64, f64)> = low.chain(high).collect();
        let round: Vec<(f64, f64)> = (0..band.len())
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / band.len() as f64;
                (a.cos(), a.sin())
            })
            .collect();
        let used = |points: &[(f64, f64)]| {
            let meter = Meter::new(u64::MAX);
            triangulate(vec![loop_of(points, 0)], &Bends::new(), 100, &meter).unwrap();
            meter.used()
        };
        let (band, round) = (used(&band), used(&round));
        assert!(
            band > 100 * EXACT_STEPS as u64 && band > 10 * round,
            "{band} {round}"
        );
    }

    #[test]
    fn a_concave_loop() {
        let l = loop_of(
            &[
                (0.0, 0.0),
                (3.0, 0.0),
                (3.0, 3.0),
                (2.0, 1.0),
                (1.0, 1.0),
                (0.0, 3.0),
            ],
            0,
        );
        let all = l.clone();
        let tris = triangulate(vec![l], &Bends::new(), 100, &Meter::new(u64::MAX))
            .unwrap()
            .tris;
        let at = |id: u32| all[id as usize].at;
        assert_eq!(tris.len(), 4);
        assert_eq!(total_area(&tris, at), 5.0);
        for &[a, b, c] in &tris {
            assert!(orient2d(at(a), at(b), at(c)) > 0);
        }
    }

    #[test]
    fn a_loop_of_zero_width_still_triangulates() {
        // Points pairwise coincident, as flush faces give.
        let l = loop_of(
            &[
                (0.0, 0.0),
                (1.0, 0.0),
                (1.0, 1.0),
                (1.0, 1.0),
                (1.0, 0.0),
                (0.0, 0.0),
            ],
            0,
        );
        let tris = triangulate(vec![l], &Bends::new(), 100, &Meter::new(u64::MAX))
            .unwrap()
            .tris;
        assert_eq!(tris.len(), 4);
    }

    #[test]
    fn two_arcs_of_one_curve_meeting_take_a_point() {
        // The sides from 1 to 2 and from 2 to 0 are arcs of one smooth
        // curve through 2: any triangle with both folds at 2, so a point
        // goes inside and the triangle is split at it.
        let l = loop_of(&[(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)], 0);
        let d = DVec2::new(-0.2, -1.0);
        let mut bends = Bends::new();
        bends.insert((1, 2), [DVec2::new(0.0, 1.0), -d]);
        bends.insert((2, 0), [d, DVec2::new(-0.2, 1.0)]);
        let all = l.clone();
        let out = triangulate(vec![l], &bends, 100, &Meter::new(u64::MAX)).unwrap();
        assert_eq!(out.steiner.len(), 1, "{out:?}");
        assert!(out.split.is_empty(), "{out:?}");
        assert_eq!(out.tris.len(), 3, "{out:?}");
        let vert = |id: u32| out_vert(&all, &out, id);
        let at = |id: u32| vert(id).at;
        assert!((total_area(&out.tris, at) - 0.5).abs() < 1e-15);
        for tri in &out.tris {
            let [a, b, c] = tri.map(at);
            assert!(orient2d(a, b, c) > 0, "{out:?}");
            assert!(corners_open(tri.map(vert).each_ref(), &bends), "{out:?}");
        }
    }

    /// Vertex `id` of the loops `all` or the points `out` added.
    fn out_vert(all: &[Vert], out: &Triangulation, id: u32) -> Vert {
        if id >= 100 {
            out.steiner[(id - 100) as usize]
        } else {
            all[id as usize]
        }
    }

    #[test]
    fn a_curve_closing_a_corner_asks_to_be_split() {
        // The side from 1 to 2 leaves 1 along the side from 1 to 0: every
        // triangle on it has a closed corner there, which no point inside
        // mends. The curve is asked for.
        let l = loop_of(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)], 0);
        let mut bends = Bends::new();
        bends.insert((1, 2), [DVec2::new(-1.0, 1e-6), DVec2::new(0.0, -1.0)]);
        let out = triangulate(vec![l], &bends, 100, &Meter::new(u64::MAX)).unwrap();
        assert_eq!(out.tris.len(), 2, "{out:?}");
        assert!(out.steiner.is_empty(), "{out:?}");
        assert_eq!(out.split, vec![(1, 2)]);
    }

    #[test]
    fn a_vertex_on_a_side_is_respected() {
        // Two triangles joined at a vertex on the domain's side (not
        // tagged with it: an interior vertex that lands there).
        let v = |id, x, y, sides| Vert {
            id,
            at: DVec2::new(x, y),
            sides,
            cuts: [NO_CUT; 2],
        };
        let l = vec![
            v(0, 0.0, 0.0, 0b101),
            v(1, 1.0, 0.0, 0b011),
            v(2, 0.5, 0.5, 0b010),
            v(3, 0.5, 0.0, 0),
            v(4, 0.0, 0.5, 0b100),
        ];
        let all = l.clone();
        let tris = triangulate(vec![l], &Bends::new(), 100, &Meter::new(u64::MAX))
            .unwrap()
            .tris;
        let at = |id: u32| all[id as usize].at;
        assert_eq!(tris.len(), 3, "{tris:?}");
        assert_eq!(total_area(&tris, at), 0.25, "{tris:?}");
        for &[a, b, c] in &tris {
            assert!(orient2d(at(a), at(b), at(c)) >= 0, "{tris:?}");
        }
        // No diagonal between vertices on one side of the domain.
        for &[a, b, c] in &tris {
            for (u, v) in [(a, b), (b, c), (c, a)] {
                let (u, v) = (all[u as usize], all[v as usize]);
                let side = (u.id as usize + 1) % 5 == v.id as usize
                    || (v.id as usize + 1) % 5 == u.id as usize;
                assert!(side || u.sides & v.sides == 0, "{tris:?}");
            }
        }
    }
}
