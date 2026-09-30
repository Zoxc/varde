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
//! with nothing in or on them, then ears of zero area, then any ear, and
//! leaves degenerate triangles to the clean-up after it. It never adds a
//! diagonal between two vertices on one side of the domain triangle (they
//! would lie along it, and the patch beside that side could add the same
//! one), nor one that is already an edge. Then diagonals are flipped
//! towards the Delaunay triangulation.
//!
//! Extrude's caps use `spade`'s constrained Delaunay triangulation
//! instead: it merges coincident points, which these loops have, while
//! caps have none but can have many thousand vertices, where ear
//! clipping is too slow.

use std::collections::{BTreeMap, BTreeSet};

use glam::DVec2;

use super::BooleanError;
use super::exact::orient2d_towards;

/// A vertex of a loop: its id, where it is in the domain, and the sides
/// of the domain triangle it lies on (bit `i` for the side from corner `i`
/// to corner `i + 1`).
#[derive(Debug, Clone, Copy)]
pub(super) struct Vert {
    pub(super) id: u32,
    pub(super) at: DVec2,
    pub(super) sides: u8,
}

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

    /// Whether it is at the same place as `other`, moves included.
    fn same(&self, other: &Vert) -> bool {
        self.at == other.at && self.inside() == other.inside()
    }
}

/// The sign of `(b − a) × (c − a)`, exactly, with the vertices inside the
/// domain moved as [`Vert::inside`] says.
fn orient(a: &Vert, b: &Vert, c: &Vert) -> i8 {
    orient2d_towards([a, b, c].map(|v| (v.at, v.inside())), CENTER)
}

/// How many vertices a polygon may have for the best ear to be looked for
/// among all of them each time; past it the first proper ear is taken.
const SEARCH: usize = 64;

/// The triangles, counter-clockwise, covering the region the loops bound.
pub(super) fn triangulate(loops: Vec<Vec<Vert>>) -> Result<Vec<[u32; 3]>, BooleanError> {
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
        let poly = bridge(outer, holes);
        let from = tris.len();
        let fixed = sides_of(&poly);
        let verts = poly.clone();
        clip(poly, &mut tris)?;
        improve(&mut tris[from..], &verts, &fixed);
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
/// triangles are touched, the new diagonal must be allowed as the ear
/// clipping's are, and the number of flips is bounded.
fn improve(tris: &mut [[u32; 3]], verts: &[Vert], fixed: &BTreeSet<(u32, u32)>) {
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
                if s == t
                    || c == d
                    || vc.sides & vd.sides != 0
                    || owner.contains_key(&(c, d))
                    || owner.contains_key(&(d, c))
                    || orient(&va, &vb, &vc) <= 0
                    || orient(&vb, &va, &vd) <= 0
                    || orient(&vc, &va, &vd) <= 0
                    || orient(&vc, &vd, &vb) <= 0
                    || !in_circle(va.at, vb.at, vc.at, vd.at)
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
fn clip(mut ring: Vec<Vert>, out: &mut Vec<[u32; 3]>) -> Result<(), BooleanError> {
    let key = |a: u32, b: u32| (a.min(b), a.max(b));
    let n = ring.len();
    let mut edges: BTreeSet<(u32, u32)> = (0..n)
        .map(|i| key(ring[i].id, ring[(i + 1) % n].id))
        .collect();
    let mut start = 0;
    while ring.len() > 3 {
        let n = ring.len();
        let best = if n <= SEARCH {
            (0..n)
                .filter_map(|i| ear(&ring, i, &edges).map(|e| (e, i)))
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
                    ear(&ring, i, &edges)
                        .filter(|e| e.level <= 1)
                        .map(|e| (e, i))
                })
                .or_else(|| {
                    (0..n)
                        .filter_map(|i| ear(&ring, i, &edges).map(|e| (e, i)))
                        .min_by(|(a, i), (b, j)| a.level.cmp(&b.level).then(i.cmp(j)))
                })
        };
        let Some((_, i)) = best else {
            return Err(BooleanError::Degenerate);
        };
        let (prev, cur, next) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
        out.push([prev.id, cur.id, next.id]);
        edges.insert(key(prev.id, next.id));
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
/// make them), 1 a proper triangle with no other vertex in or on it, 2
/// one of zero area with no other vertex on it, 3 anything else; and
/// within a level, its shape.
struct Ear {
    level: u8,
    quality: f64,
}

/// The ear cutting vertex `i` off, if its diagonal is allowed.
fn ear(ring: &[Vert], i: usize, edges: &BTreeSet<(u32, u32)>) -> Option<Ear> {
    let n = ring.len();
    let (p, c, q) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
    if p.id == q.id || p.id == c.id || c.id == q.id {
        return None;
    }
    if p.sides & q.sides != 0 || edges.contains(&(p.id.min(q.id), p.id.max(q.id))) {
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
            (true, _) => 3,
            (false, true) => 0,
            (false, false) => 2,
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
        if blocked { 3 } else { 1 }
    } else {
        3
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
        let tris = triangulate(vec![outer, hole]).unwrap();
        assert_eq!(tris.len(), 8);
        let at = |id: u32| all[id as usize].at;
        assert_eq!(total_area(&tris, at), 12.0);
        for &[a, b, c] in &tris {
            assert!(orient2d(at(a), at(b), at(c)) > 0);
        }
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
        let tris = triangulate(vec![l]).unwrap();
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
        let tris = triangulate(vec![l]).unwrap();
        assert_eq!(tris.len(), 4);
    }

    #[test]
    fn a_vertex_on_a_side_is_respected() {
        // Two triangles joined at a vertex on the domain's side (not
        // tagged with it: an interior vertex that lands there).
        let v = |id, x, y, sides| Vert {
            id,
            at: DVec2::new(x, y),
            sides,
        };
        let l = vec![
            v(0, 0.0, 0.0, 0b101),
            v(1, 1.0, 0.0, 0b011),
            v(2, 0.5, 0.5, 0b010),
            v(3, 0.5, 0.0, 0),
            v(4, 0.0, 0.5, 0b100),
        ];
        let all = l.clone();
        let tris = triangulate(vec![l]).unwrap();
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
