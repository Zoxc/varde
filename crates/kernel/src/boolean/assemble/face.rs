//! Cutting one face: its kept loops triangulated, and the curves of the
//! triangles' sides that aren't pieces of its edges or of its cuts.
//!
//! How a face is laid out for triangulating depends on its patch:
//!
//! - A **flat** patch (straight edges) is its corner triangle, and its
//!   parameter domain is the plane's, affinely: vertices placed by
//!   projection.
//! - A **planar** patch with curved edges (a cap along an arc) is laid out
//!   in the plane by the same projection, and its new inner edges are
//!   straight there, as refinement makes them: the patches cover exactly
//!   the region, the plane being the plane.
//! - A **curved** patch is laid out in its parameter domain: vertices on
//!   its sides at their parameters, others where the patch inverts them.
//!   Its new inner edges are the patch's own curves over straight domain
//!   segments (by blossoming, exact), except on a quadric, where they are
//!   chosen so that the triangles along a cut are exact too (see
//!   [`exact_bands`]).

use std::collections::{BTreeMap, BTreeSet};

use glam::{DVec2, DVec3};

use super::super::chain::trace::{domain_step, invert};
use super::super::exact::orient2d;
use super::super::input::Input;
use super::super::surface::{Guide, Shape, second_point, section};
use super::super::triangulate::{Bends, Meter, NO_CUT, Vert, triangulate};
use super::super::{BooleanError, segment};
use super::{Along, Curves, key};
use crate::failure::evidence_work;
use crate::mesh::{Edge, MIN_CURVED_SPLIT, Quadric, Surface, off_surface, samples, straight};
use crate::patch::{Conic3, Patch};
use crate::{Evidence, Tolerance};

/// How a face is laid out for triangulating: see the [module](self) docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Layout {
    Flat,
    Planar,
    Curved,
}

impl Layout {
    pub(super) fn of(input: &Input, t: u32) -> Layout {
        if input.flat[t as usize] {
            Layout::Flat
        } else if input.planar[t as usize] {
            Layout::Planar
        } else {
            Layout::Curved
        }
    }
}

/// Everything one face needs to be cut.
pub(super) struct Cut {
    pub(super) side: super::Side,
    pub(super) tri: u32,
    /// Its cut edges, as it runs them.
    pub(super) cuts: Vec<[u32; 2]>,
    /// Where the cuts' vertices are in its patch (barycentric), by id.
    pub(super) inside: Vec<(u32, DVec3)>,
    /// The cuts (by arc) each vertex of them lies on, by id, where
    /// diagonals may not join two vertices of one cut (a face of `B`: the
    /// face of `A` across may join them).
    pub(super) on_cut: Vec<(u32, u32)>,
}

/// What cutting one face gives: its triangles, whether each is off the
/// face's surface (a fitted band on a quadric), the curves of the new
/// inner edges, and the points the triangulation added inside, numbered
/// from [`FIRST_STEINER`] in the triangles and curves.
pub(super) struct Cutout {
    pub(super) tris: Vec<[u32; 3]>,
    pub(super) off: Vec<bool>,
    pub(super) curves: Vec<((u32, u32), Edge)>,
    pub(super) steiner: Vec<DVec3>,
    /// The boundary edges to split: the cut edges some triangle beside
    /// which strays from the patch by more than half the fit tolerance,
    /// and the sides of a triangle off its face's quadric by that much.
    pub(super) split: Vec<(u32, u32)>,
    /// How far the triangles held to the fit tolerance stray, at most:
    /// those along a cut from the face's patch, and those off the face's
    /// quadric (on its copy claiming no surface) from the quadric; NaN
    /// counted as infinite. The others lie on the patch (its own curves)
    /// or within half the resolution of the quadric.
    pub(super) stray: f64,
}

/// Where a face's own added points are numbered from until they are
/// given ids in the mesh: past any id a mesh has (three per patch at
/// most).
pub(super) const FIRST_STEINER: u32 = 0xF000_0000;

/// The corners of the parameter domain.
const DOMAIN: [DVec2; 3] = [DVec2::ZERO, DVec2::X, DVec2::Y];

/// Domain coordinates smaller than this are zero: far below anything
/// rounding can tell, and it keeps the exact orientation tests' products
/// clear of underflow.
const TINY: f64 = 1.0 / (1u128 << 64) as f64;

/// The point at `t` from corner `from` to corner `to` of the domain,
/// exactly on that side.
fn on_side(from: DVec2, to: DVec2, t: f64) -> DVec2 {
    let t = if t < TINY { 0.0 } else { t };
    let p = from + (to - from) * t;
    if from.x + from.y == 1.0 && to.x + to.y == 1.0 {
        on_hypotenuse(p.y)
    } else {
        p
    }
}

/// The point of the side `u + v = 1` at `v` (in `[0, 1]`), with the sum
/// exact: the larger coordinate is taken as given and the other is one
/// less it, which is exact from ½ up.
fn on_hypotenuse(v: f64) -> DVec2 {
    let v = v.clamp(0.0, 1.0);
    if v >= 0.5 {
        DVec2::new(1.0 - v, v)
    } else {
        let u = 1.0 - v;
        DVec2::new(u, 1.0 - u)
    }
}

/// A vertex inside the face (it is, for the perturbed operands) as
/// placed from its position, moved onto the domain's side where rounding
/// or a tie put it on or beyond it, or within `snap` of it. The
/// triangulation then treats it as inside, infinitely close.
fn into_domain(at: DVec2, snap: f64) -> DVec2 {
    let snap = snap.max(TINY);
    let u = if at.x < snap { 0.0 } else { at.x };
    let v = if at.y < snap { 0.0 } else { at.y };
    let p = DVec2::new(u, v);
    if orient2d(DOMAIN[1], DOMAIN[2], p) > 0 && 1.0 - (u + v) >= snap {
        p
    } else {
        on_hypotenuse((v - u + 1.0) / 2.0)
    }
}

/// How near a side of a curved patch's domain a vertex inverted into it
/// is taken to be on it (see [`into_domain`]): the inversion's rounding.
/// A cut along a flush rim inverts to points on the side, each a rounding
/// inside it or not; left where rounding put them, their order across
/// the side was noise, and the triangulation took a fan from the corner
/// across them for proper, three corners on the rim in the patch: of
/// zero area, which fails the fold check. On the side they lie on one
/// line, moved inwards alike.
const SNAP: f64 = 1e-12;

/// Where `x` is in the plane of `corners`, affinely: `(0, 0)` at the
/// first, `(1, 0)` at the second, `(0, 1)` at the third.
pub(super) fn project(corners: [DVec3; 3], x: DVec3) -> DVec2 {
    let [p0, p1, p2] = corners;
    let (d1, d2) = (p1 - p0, p2 - p0);
    let n = d1.cross(d2);
    let nn = n.length_squared();
    let x = x - p0;
    DVec2::new(x.cross(d2).dot(n) / nn, d1.cross(x).dot(n) / nn)
}

/// The barycentric point at domain position `at`.
fn bary(at: DVec2) -> DVec3 {
    DVec3::new(1.0 - at.x - at.y, at.x, at.y)
}

/// Cuts one face of `input` (its vertices numbered from `offset`): the
/// triangles of its kept part, as its triangle runs, and their curves.
#[allow(clippy::too_many_arguments)]
pub(super) fn cut_face(
    input: &Input,
    job: &Cut,
    along: &Along,
    offset: u32,
    pos: &[DVec3],
    curves: &Curves,
    fitted: &BTreeSet<(u32, u32)>,
    meter: &Meter,
    tol: &Tolerance,
) -> Result<Cutout, BooleanError> {
    let t = job.tri;
    let layout = Layout::of(input, t);
    let corner_pos = input.tris[t as usize].map(|v| input.pos(v));
    let (halfedges, known) = boundary(input, job, along, offset, pos);
    if halfedges.windows(2).any(|w| w[0][0] == w[1][0]) {
        return Err(BooleanError::Inconsistent);
    }
    let vert = |id: u32| -> Vert {
        if let Ok(i) = known.binary_search_by_key(&id, |v| v.id) {
            return known[i];
        }
        let projected = || project(corner_pos, pos[id as usize]);
        let at = match layout {
            Layout::Flat => into_domain(projected(), 0.0),
            Layout::Planar => projected(),
            Layout::Curved => {
                let inside = job
                    .inside
                    .binary_search_by_key(&id, |x| x.0)
                    .map(|i| job.inside[i].1);
                into_domain(
                    match inside {
                        Ok(u) => DVec2::new(u.y, u.z),
                        Err(_) => projected(),
                    },
                    SNAP,
                )
            }
        };
        Vert {
            id,
            at,
            sides: 0,
            cuts: [NO_CUT; 2],
        }
    };
    // The cuts each vertex lies on.
    let on_cut = |mut v: Vert| {
        let from = job.on_cut.partition_point(|x| x.0 < v.id);
        for (k, &(_, arc)) in job.on_cut[from..]
            .iter()
            .take_while(|x| x.0 == v.id)
            .take(2)
            .enumerate()
        {
            v.cuts[k] = arc;
        }
        v
    };
    let mut used = vec![false; halfedges.len()];
    let mut loops = Vec::new();
    let mut placed: Vec<Vert> = Vec::with_capacity(halfedges.len());
    for i in 0..halfedges.len() {
        if used[i] {
            continue;
        }
        let first = halfedges[i][0];
        let mut l = Vec::new();
        let mut u = first;
        loop {
            let k = halfedges
                .binary_search_by_key(&u, |h| h[0])
                .map_err(|_| BooleanError::Inconsistent)?;
            if used[k] {
                return Err(BooleanError::Inconsistent);
            }
            used[k] = true;
            let v = on_cut(vert(u));
            l.push(v);
            placed.push(v);
            u = halfedges[k][1];
            if u == first {
                break;
            }
        }
        loops.push(l);
    }
    placed.sort_by_key(|v| v.id);
    // The curved sides' tangents where they leave their ends, laid out:
    // the cuts', and on a flat or planar patch its curved edges' pieces
    // too (on a curved patch those run along the domain's sides).
    let patch = &input.patches[t as usize];
    let lay = |at: DVec2, d: DVec3| -> DVec2 {
        match layout {
            Layout::Curved => {
                let step = domain_step(patch, bary(at), d);
                DVec2::new(step.y, step.z)
            }
            _ => project(corner_pos, corner_pos[0] + d),
        }
    };
    let mut bends = Bends::new();
    for &[u, v] in &halfedges {
        let Some(edge) = curves.get(&key(u, v)) else {
            continue;
        };
        let cut = job.cuts.contains(&[u, v]);
        let (pu, pv) = (pos[u as usize], pos[v as usize]);
        if (layout == Layout::Curved && !cut) || straight(pu, edge.ctrl, pv, 0.0) {
            continue;
        }
        bends.insert(
            (u, v),
            [
                lay(found(&placed, u), edge.ctrl - pu),
                lay(found(&placed, v), edge.ctrl - pv),
            ],
        );
    }
    // Points for the triangles' shapes on a curved patch, in triangles no
    // smaller than repair splits: a circumradius of `MIN_CURVED_SPLIT`
    // resolutions over the patch's longest side, in the layout (about 1
    // across).
    let shapes = (layout == Layout::Curved).then(|| {
        let [p0, p1, p2] = corner_pos;
        let across = p0.distance(p1).max(p1.distance(p2)).max(p2.distance(p0));
        MIN_CURVED_SPLIT * tol.resolution() / across
    });
    let triangulation = triangulate(loops, &bends, FIRST_STEINER, shapes, meter)?;
    let tris = triangulation.tris;
    // Curves the triangulation wants split for its corners.
    let wanted: Vec<(u32, u32)> = triangulation
        .split
        .iter()
        .map(|&(u, v)| key(u, v))
        .collect();
    // The points added inside: where they are.
    let steiner: Vec<DVec3> = triangulation
        .steiner
        .iter()
        .map(|v| match layout {
            Layout::Curved => patch.eval(bary(v.at)),
            _ => {
                let [p0, p1, p2] = corner_pos;
                p0 + (p1 - p0) * v.at.x + (p2 - p0) * v.at.y
            }
        })
        .collect();
    placed.extend(triangulation.steiner.iter().copied());
    placed.sort_by_key(|v| v.id);
    let at_of = |id: u32| found(&placed, id);
    let pos_of = |id: u32| {
        if id >= FIRST_STEINER {
            steiner[(id - FIRST_STEINER) as usize]
        } else {
            pos[id as usize]
        }
    };
    let boundary: BTreeSet<(u32, u32)> = halfedges.iter().map(|h| key(h[0], h[1])).collect();
    let straight_edge = |u: u32, v: u32| Edge::straight(pos_of(u), pos_of(v));
    // Inner edges between two vertices at the very places a boundary
    // edge's ends are (a tie left vertices of flush operands at one
    // place, and a triangle of zero width between them) take that edge's
    // curve, so the clean-up can merge the two.
    let at_place = |u: u32, v: u32| {
        let (a, b) = (
            pos_of(u).to_array().map(f64::to_bits),
            pos_of(v).to_array().map(f64::to_bits),
        );
        (a.min(b), a.max(b))
    };
    let twins: BTreeMap<_, (u32, u32)> =
        boundary.iter().map(|&k| (at_place(k.0, k.1), k)).collect();
    let mut twinned: BTreeMap<(u32, u32), Option<Edge>> = BTreeMap::new();
    for tri in &tris {
        for i in 0..3 {
            let k = key(tri[i], tri[(i + 1) % 3]);
            if !boundary.contains(&k)
                && let Some(twin) = twins.get(&at_place(k.0, k.1))
            {
                twinned.insert(k, curves.get(twin).copied());
            }
        }
    }
    // Ends only a rounding apart do too, where the clean-up will collapse
    // them: a triangle with two corners within its short length (an
    // eighth of the resolution) of each other, whose side from the third
    // corner to one of them is inner, takes the curve of a boundary edge
    // from that third corner to a vertex as near the first (a tie whose
    // two vertices came by different roundings: a cap's inner edge
    // crossing at a rim vertex, and a wall's diagonal crossing the cap's
    // plane, 1e-16 apart).
    let short = super::super::short(tol);
    let near_ends: Vec<(u32, u32)> = tris
        .iter()
        .flat_map(|tri| {
            (0..3).flat_map(move |i| {
                let (x, y, far) = (tri[i], tri[(i + 1) % 3], tri[(i + 2) % 3]);
                [(far, x), (far, y)]
                    .into_iter()
                    .filter(move |_| pos_of(x).distance(pos_of(y)) <= short)
            })
        })
        .filter(|&(far, near)| !boundary.contains(&key(far, near)))
        .collect();
    if !near_ends.is_empty() {
        let mut at_vertex: BTreeMap<u32, Vec<(u32, u32)>> = BTreeMap::new();
        for &k in &boundary {
            at_vertex.entry(k.0).or_default().push(k);
            at_vertex.entry(k.1).or_default().push(k);
        }
        for (far, near) in near_ends {
            let k = key(far, near);
            if twinned.contains_key(&k) {
                continue;
            }
            let twin = at_vertex.get(&far).into_iter().flatten().find(|b| {
                let other = if b.0 == far { b.1 } else { b.0 };
                other != near && pos_of(other).distance(pos_of(near)) <= short
            });
            if let Some(twin) = twin {
                twinned.insert(k, curves.get(twin).copied());
            }
        }
    }
    if layout != Layout::Curved {
        // Inner edges straight; the patches stay in the plane.
        return Ok(Cutout {
            off: vec![false; tris.len()],
            tris,
            curves: twinned
                .into_iter()
                .filter_map(|(k, e)| e.map(|e| (k, e)))
                .collect(),
            steiner,
            split: wanted,
            stray: 0.0,
        });
    }

    let at = |id: u32| bary(at_of(id));
    // The patch's own curve over each straight domain segment.
    let mut inner: BTreeMap<(u32, u32), Edge> = BTreeMap::new();
    for tri in &tris {
        for i in 0..3 {
            let k = key(tri[i], tri[(i + 1) % 3]);
            if !boundary.contains(&k) {
                inner.entry(k).or_insert_with(|| match twinned.get(&k) {
                    Some(twin) => twin.unwrap_or_else(|| straight_edge(k.0, k.1)),
                    None => patch
                        .curve(at(k.0), at(k.1))
                        .map_or_else(|_| straight_edge(k.0, k.1), |c| Edge::of(&c)),
                });
            }
        }
    }
    // The curve of the edge `k`: an inner edge's in `inner`, a boundary
    // edge's record, else straight.
    let edge_at = |k: (u32, u32), inner: &BTreeMap<(u32, u32), Edge>| {
        inner
            .get(&k)
            .or_else(|| curves.get(&k))
            .copied()
            .unwrap_or_else(|| straight_edge(k.0, k.1))
    };
    let surface = input.mesh.faces()[input.face(t) as usize].surface;
    if let Shape::Quadric(q) = Shape::of(input, t) {
        let fixed: BTreeSet<(u32, u32)> = boundary.iter().chain(twinned.keys()).copied().collect();
        let kind = |u: u32, v: u32| -> Kind {
            let k = key(u, v);
            if fitted.contains(&k) {
                return Kind::Fitted;
            }
            Kind::of(pos_of(u), edge_at(k, &inner), pos_of(v), tol.resolution())
        };
        let mut changed = inner.clone();
        exact_bands(&q, &tris, &pos_of, &fixed, kind, edge_at, &mut changed);
        inner = changed;
    }
    let edge_of = |u: u32, v: u32| edge_at(key(u, v), &inner);
    let cuts: BTreeSet<(u32, u32)> = job.cuts.iter().map(|h| key(h[0], h[1])).collect();
    let mut split: BTreeSet<(u32, u32)> = wanted.into_iter().collect();
    let mut stray = 0.0f64;
    // A distance, NaN as infinite (so it strays, and the maximum sees it).
    let far = |d: f64| if d.is_nan() { f64::INFINITY } else { d };
    let off = tris
        .iter()
        .map(|&tri| {
            let e = [0, 1, 2].map(|i| edge_of(tri[i], tri[(i + 1) % 3]));
            let Ok(piece) = Patch::new(tri.map(pos_of), e.map(|e| e.ctrl), e.map(|e| e.weight))
            else {
                return true;
            };
            let keys = [0, 1, 2].map(|i| key(tri[i], tri[(i + 1) % 3]));
            // A triangle along a cut: how far it strays from the patch.
            let sides: Vec<(u32, u32)> = keys.into_iter().filter(|k| cuts.contains(k)).collect();
            if !sides.is_empty() {
                let d = tri.map(at);
                let from_patch = samples()
                    .map(|u| {
                        let x = piece.eval(u);
                        let guess = d[0] * u.x + d[1] * u.y + d[2] * u.z;
                        far(patch.eval(invert(patch, x, guess)).distance(x))
                    })
                    .fold(0.0, f64::max);
                stray = stray.max(from_patch);
                if from_patch > tol.fit() / 2.0 {
                    split.extend(sides);
                }
            }
            let Surface::Quadric(_) = surface else {
                return false;
            };
            let from_surface = off_surface(&piece, &surface);
            if from_surface <= tol.resolution() / 2.0 {
                return false;
            }
            // Off the quadric, onto the copy claiming no surface: held to
            // the fit tolerance all the same (a band tree's root that no
            // ruling frees, which nothing along a cut bounds), its sides
            // on the face's boundary halved while it strays.
            stray = stray.max(from_surface);
            if from_surface > tol.fit() / 2.0 {
                split.extend(keys.into_iter().filter(|k| boundary.contains(k)));
            }
            true
        })
        .collect();
    Ok(Cutout {
        tris,
        off,
        curves: inner.into_iter().collect(),
        steiner,
        split: split.into_iter().collect(),
        stray,
    })
}

/// What a face that couldn't be triangulated shows (see [`cut_face`],
/// whose arguments these are): its kept halfedges in order, the loops
/// that wouldn't triangulate, each as its curve where it has one, else
/// straight, between its vertices' positions (where the face's layout
/// puts them back, but for the rounding and snapping onto the domain's
/// sides it takes them through), and the operand's face it is. From a
/// fresh allowance, a unit a halfedge.
pub(super) fn loops_evidence(
    input: &Input,
    job: &Cut,
    along: &Along,
    offset: u32,
    pos: &[DVec3],
    curves: &Curves,
) -> Evidence {
    let mut evidence = Evidence::default();
    let face = input.mesh.faces()[input.face(job.tri) as usize].name.key();
    evidence.add_faces([(job.side.into(), face)]);
    let mut work = evidence_work();
    let (halfedges, _) = boundary(input, job, along, offset, pos);
    for [u, v] in halfedges {
        if !evidence.afford(&mut work, 1) {
            break;
        }
        let (p, q) = (pos[u as usize], pos[v as usize]);
        evidence.add_curves([curves.get(&key(u, v)).map_or_else(
            || segment(p, q),
            |edge| Conic3 {
                p0: p,
                c: edge.ctrl,
                w: edge.weight,
                p1: q,
            },
        )]);
    }
    evidence
}

/// A face's boundary (see [`cut_face`], whose arguments these are): its
/// kept halfedges (pieces of its edges, and its cuts, as it runs them),
/// sorted, and its corners and the vertices on its edges laid out, by
/// id.
fn boundary(
    input: &Input,
    job: &Cut,
    along: &Along,
    offset: u32,
    pos: &[DVec3],
) -> (Vec<[u32; 2]>, Vec<Vert>) {
    let t = job.tri;
    let layout = Layout::of(input, t);
    let corners = input.tris[t as usize];
    let corner_ids = corners.map(|v| v + offset);
    let corner_pos = corners.map(|v| input.pos(v));
    // The domain position and sides of every vertex on the boundary.
    let mut known: Vec<Vert> = (0..3)
        .map(|i| Vert {
            id: corner_ids[i],
            at: DOMAIN[i],
            sides: (1 << i) | (1 << ((i + 2) % 3)),
            cuts: [NO_CUT; 2],
        })
        .collect();
    let mut halfedges: Vec<[u32; 2]> = job.cuts.clone();
    for (i, &(e, forward)) in input.tri_edges[t as usize].iter().enumerate() {
        let (verts, kept) = along.of(e);
        // The edge's own direction, from its start corner.
        let (s, en) = if forward {
            (i, (i + 1) % 3)
        } else {
            ((i + 1) % 3, i)
        };
        // A planar patch's curved side isn't a side of the plane's
        // triangle: its vertices go where they are.
        let bulges = layout == Layout::Planar && !input.straight[e as usize];
        let mut chain = vec![corner_ids[s]];
        for &(id, param) in verts {
            chain.push(id);
            known.push(Vert {
                id,
                at: if bulges {
                    project(corner_pos, pos[id as usize])
                } else {
                    on_side(DOMAIN[s], DOMAIN[en], param)
                },
                sides: 1 << i,
                cuts: [NO_CUT; 2],
            });
        }
        chain.push(corner_ids[en]);
        for (k, &keep) in kept.iter().enumerate() {
            if keep {
                let (u, v) = (chain[k], chain[k + 1]);
                halfedges.push(if forward { [u, v] } else { [v, u] });
            }
        }
    }
    halfedges.sort_unstable();
    known.sort_by_key(|v| v.id);
    (halfedges, known)
}

/// Where vertex `id` of `placed` (sorted by id) is laid out.
fn found(placed: &[Vert], id: u32) -> DVec2 {
    let i = placed
        .binary_search_by_key(&id, |v| v.id)
        .expect("a vertex of the loops");
    placed[i].at
}

/// What a triangle's side on a quadric is, for making the triangles
/// along a cut exact: a straight ruling, which lies in a plane through
/// any point and so fits any triangle; a conic in a plane (normal `n`);
/// or a fitted curve, in no plane, whose triangles can't be exact.
#[derive(Debug, Clone, Copy)]
enum Kind {
    Ruling,
    Plane(DVec3),
    Fitted,
}

impl Kind {
    fn of(p: DVec3, edge: Edge, q: DVec3, margin: f64) -> Kind {
        if straight(p, edge.ctrl, q, margin) {
            return Kind::Ruling;
        }
        match (edge.ctrl - p).cross(q - p).try_normalize() {
            Some(n) => Kind::Plane(n),
            None => Kind::Ruling,
        }
    }
}

/// Makes the triangles of a face on the quadric `q` exact where they can
/// be, by choosing the curves of their inner edges (`inner`, first the
/// patch's own curves).
///
/// A rational quadratic triangle lies on a quadric when its three sides
/// are conics on it whose planes meet in one point `O` on it; a ruling of
/// a cylinder lies in a plane through any point, with the same curve
/// whatever the point. The patch's own curves all lie in planes through
/// its own common point, so triangles away from the cut are exact already;
/// a cut's conic lies in its cutting plane, which generally doesn't hold
/// that point. So: over a spanning tree of the triangles (across inner
/// edges) rooted at one with a ruling or a fitted side, which needs
/// nothing, each triangle from the leaves in chooses the edge to its
/// parent: with two sides in planes meeting at their shared vertex `V`,
/// `O` is where the line those planes meet in leaves the quadric again,
/// and the edge is the quadric's conic in the plane through its ends and
/// `O`. Only the root, and triangles with fitted sides, may stay off the
/// quadric.
fn exact_bands(
    q: &Quadric,
    tris: &[[u32; 3]],
    pos: &dyn Fn(u32) -> DVec3,
    boundary: &BTreeSet<(u32, u32)>,
    kind: impl Fn(u32, u32) -> Kind,
    edge_at: impl Fn((u32, u32), &BTreeMap<(u32, u32), Edge>) -> Edge,
    inner: &mut BTreeMap<(u32, u32), Edge>,
) {
    // Whether triangle `t` passes the fold check with the curves `inner`.
    let unfolded = |t: usize, inner: &BTreeMap<(u32, u32), Edge>| {
        let tri = tris[t];
        let e = [0, 1, 2].map(|i| edge_at(key(tri[i], tri[(i + 1) % 3]), inner));
        Patch::new(tri.map(pos), e.map(|e| e.ctrl), e.map(|e| e.weight))
            .is_ok_and(|p| p.fold_direction().is_some())
    };
    let n = tris.len();
    // The triangles on each inner edge.
    let mut on: BTreeMap<(u32, u32), Vec<usize>> = BTreeMap::new();
    for (t, tri) in tris.iter().enumerate() {
        for i in 0..3 {
            let k = key(tri[i], tri[(i + 1) % 3]);
            if !boundary.contains(&k) {
                on.entry(k).or_default().push(t);
            }
        }
    }
    let free = |t: usize| {
        let tri = tris[t];
        (0..3).any(|i| {
            let (u, v) = (tri[i], tri[(i + 1) % 3]);
            boundary.contains(&key(u, v)) && !matches!(kind(u, v), Kind::Plane(_))
        })
    };
    // Spanning trees, breadth first, each from its lowest free triangle
    // (else its lowest).
    let mut parent: Vec<Option<(usize, (u32, u32))>> = vec![None; n];
    let mut seen = vec![false; n];
    let mut order = Vec::with_capacity(n);
    let mut roots: Vec<usize> = (0..n).filter(|&t| free(t)).collect();
    roots.extend(0..n);
    for root in roots {
        if seen[root] {
            continue;
        }
        seen[root] = true;
        let from = order.len();
        order.push(root);
        let mut k = from;
        while k < order.len() {
            let t = order[k];
            k += 1;
            let tri = tris[t];
            for i in 0..3 {
                let e = key(tri[i], tri[(i + 1) % 3]);
                for &s in on.get(&e).into_iter().flatten() {
                    if !seen[s] {
                        seen[s] = true;
                        parent[s] = Some((t, e));
                        order.push(s);
                    }
                }
            }
        }
    }
    // Edges not in the trees keep the patch's own curves.
    let plane_of = |e: (u32, u32), inner: &BTreeMap<(u32, u32), Edge>| -> Kind {
        match inner.get(&e) {
            Some(&edge) => Kind::of(pos(e.0), edge, pos(e.1), 0.0),
            None => kind(e.0, e.1),
        }
    };
    for &t in order.iter().rev() {
        let Some((up, d)) = parent[t] else { continue };
        if free(t) {
            continue;
        }
        let tri = tris[t];
        let others: Vec<((u32, u32), Kind)> = (0..3)
            .map(|i| key(tri[i], tri[(i + 1) % 3]))
            .filter(|&e| e != d)
            .map(|e| (e, plane_of(e, inner)))
            .collect();
        let [(e1, Kind::Plane(n1)), (e2, Kind::Plane(n2))] = others[..] else {
            continue;
        };
        // The vertex the two share, and the far point their planes meet
        // the quadric in.
        let v = [e1.0, e1.1]
            .into_iter()
            .find(|&x| x == e2.0 || x == e2.1)
            .expect("two sides share a vertex");
        let Some((dir, inv)) = second_point(q, pos(v), n1.cross(n2)) else {
            continue;
        };
        // The plane through the edge's ends `x`, `y` and `O = V + dir/inv`:
        // `(x − O) × (y − O)` times `inv`, which holds as `O` goes to
        // infinity (on a parabolic cylinder: the plane through `x` and `y`
        // along its axis, the patch's own curves' plane).
        let (x, y) = (pos(d.0), pos(d.1));
        let (a, b) = (x - pos(v), y - pos(v));
        let Some(normal) = (a.cross(b) * inv + (y - x).cross(dir)).try_normalize() else {
            continue;
        };
        let old = inner[&d];
        let guide = Conic3 {
            p0: x,
            c: old.ctrl,
            w: old.weight,
            p1: y,
        }
        .eval(0.5);
        if let Some(arcs) = section(q, normal, x, y, Guide::Near(guide))
            && let [arc] = arcs[..]
        {
            // Only where both triangles on it still don't fold: a far `O`
            // can turn the curve's parametrization far from the patch's.
            inner.insert(d, Edge::of(&arc));
            if !(unfolded(t, inner) && unfolded(up, inner)) {
                inner.insert(d, old);
            }
        }
    }
}
