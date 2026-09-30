//! Each cut arc's geometry: the chain of shared edges from its `+` end
//! to its `−` end that both faces of its pair are cut along.
//!
//! The pair's patches pick the path:
//!
//! - **Two planar patches** meet in a straight segment: one straight
//!   edge.
//! - **A plane and a quadric** (by the faces' tags) meet in a conic: the
//!   exact arc, halved where it turns far ([`section`]).
//! - **Anything else** is traced and fitted within the fit tolerance
//!   ([`trace`]). If tracing fails, the arc falls back to a simpler curve
//!   between the same ends: the conic along their tangents, or a straight
//!   edge. Only the geometry suffers, never the topology, which the
//!   counting has already decided.
//!
//! Where one patch is planar, a traced chain's conics lie in its plane,
//! so planar faces stay planar; between two curved patches, in the plane
//! bisecting the crease the result has along the cut (see
//! [`trace::Crease`]).

use glam::DVec3;

use super::segment;
use super::surface::{Guide, Shape, section};
use crate::mesh::Quadric;
use crate::patch::{Conic3, Patch};

pub(crate) mod trace;

use trace::{Crease, Pair, Point, conic_along, invert};

/// One arc to make a chain for: its pair's patches (`p` of `A`, `q` of
/// `B`) and what they lie on, and its ends.
#[derive(Debug, Clone)]
pub(super) struct Job<'a> {
    pub(super) p: &'a Patch,
    pub(super) q: &'a Patch,
    pub(super) shapes: [Shape; 2],
    /// Whether each patch is planar.
    pub(super) planar: [bool; 2],
    /// The `+` end, then the `−` end.
    pub(super) ends: [DVec3; 2],
    /// Each end's barycentric position in `p` and in `q`.
    pub(super) dom: [[DVec3; 2]; 2],
    /// Whether each end lies on the boundary of `p` (else of `q`): the arc
    /// leaves that patch's side inwards.
    pub(super) on_p: [bool; 2],
    /// Whether `q`'s faces are turned over in the result.
    pub(super) flip_q: bool,
}

impl Job<'_> {
    /// The pair, with the plane its fitted conics lie in.
    fn pair(&self) -> Pair<'_> {
        let crease = match (self.shapes, self.planar) {
            ([Shape::Plane { n, .. }, _], [true, _]) | ([_, Shape::Plane { n, .. }], [_, true]) => {
                Crease::Plane(n)
            }
            _ => Crease::Bisect {
                sign: if self.flip_q { -1.0 } else { 1.0 },
            },
        };
        Pair::new(self.p, self.q, crease, self.planar.map(|p| !p))
    }
}

/// A cut arc as edges: the vertices between its ends and the curves from
/// its `+` end to its `−` end, one more than the vertices.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Chain {
    /// The vertices between the ends, from `+` to `−`.
    pub(super) points: Vec<DVec3>,
    /// Every vertex's barycentric position in `p` and in `q`, ends
    /// included, from `+` to `−`.
    pub(super) dom: [Vec<DVec3>; 2],
    pub(super) curves: Vec<Conic3>,
    /// Whether the curves are exact (the surfaces' true cut), rather than
    /// fitted or a fallback.
    pub(super) exact: bool,
}

impl Chain {
    /// The chain with each of its curves `segments` (indices, ascending)
    /// halved: an exact curve at its middle, a fitted one where the curve
    /// crosses the plane square to it through its middle, refitted.
    pub(super) fn split(&self, job: &Job, segments: &[usize], fit: f64) -> Chain {
        let pair = job.pair();
        let mut out = self.clone();
        for &k in segments.iter().rev() {
            let old = out.curves[k];
            let vertex = |i: usize| -> Point {
                let x = if i == 0 {
                    job.ends[0]
                } else if i == out.points.len() + 1 {
                    job.ends[1]
                } else {
                    out.points[i - 1]
                };
                let (u, v) = (out.dom[0][i], out.dom[1][i]);
                let tan = pair.tangent(u, v).unwrap_or(old.p1 - old.p0);
                let along = if i == k {
                    old.c - old.p0
                } else {
                    old.p1 - old.c
                };
                Point {
                    x,
                    u,
                    v,
                    tan: if tan.dot(along) < 0.0 { -tan } else { tan },
                }
            };
            let (a, b) = (vertex(k), vertex(k + 1));
            let (m, halves) = if out.exact {
                match old.split_half() {
                    Ok(halves) => {
                        let x = halves[0].p1;
                        let u = invert(job.p, x, (a.u + b.u) * 0.5);
                        let v = invert(job.q, x, (a.v + b.v) * 0.5);
                        (
                            Point {
                                x,
                                u,
                                v,
                                tan: a.tan,
                            },
                            halves,
                        )
                    }
                    Err(_) => continue,
                }
            } else {
                match trace::split(&pair, &a, &b, &old, fit / 4.0) {
                    Some(split) => split,
                    None => continue,
                }
            };
            out.curves.splice(k..=k, halves);
            out.points.insert(k, m.x);
            out.dom[0].insert(k + 1, m.u);
            out.dom[1].insert(k + 1, m.v);
        }
        out
    }
}

/// The chain of `job`'s arc, within `fit` of the true cut.
pub(super) fn chain(job: &Job, fit: f64) -> Chain {
    let [x, y] = job.ends;
    let straight = |exact| Chain {
        points: Vec::new(),
        dom: [
            vec![job.dom[0][0], job.dom[1][0]],
            vec![job.dom[0][1], job.dom[1][1]],
        ],
        curves: vec![segment(x, y)],
        exact,
    };
    if job.planar[0] && job.planar[1] || x == y {
        return straight(true);
    }
    if let Some(chain) = exact(job) {
        return chain;
    }
    traced(job, fit).unwrap_or_else(|| fallback(job).unwrap_or_else(|| straight(false)))
}

/// The exact arcs of a plane against a quadric, if the pair is one and
/// they stay on its patches.
fn exact(job: &Job) -> Option<Chain> {
    let (plane, quadric, k) = match job.shapes {
        [Shape::Plane { n, .. }, Shape::Quadric(q)] => (n, q, 1),
        [Shape::Quadric(q), Shape::Plane { n, .. }] => (n, q, 0),
        _ => return None,
    };
    let patches = [job.p, job.q];
    let patch = patches[k];
    let [x, y] = job.ends;
    if let Some(edge) = along_edge(job, plane, &quadric) {
        return Some(Chain {
            points: Vec::new(),
            dom: [
                vec![job.dom[0][0], job.dom[1][0]],
                vec![job.dom[0][1], job.dom[1][1]],
            ],
            curves: vec![edge],
            exact: true,
        });
    }
    // Near the arc: the quadric patch's point halfway between the ends in
    // its domain.
    let guide = patch.eval((job.dom[0][k] + job.dom[1][k]) * 0.5);
    let curves = section(&quadric, plane, x, y, Guide::Near(guide))?;
    let points: Vec<DVec3> = curves[1..].iter().map(|c| c.p0).collect();
    let dom = [0, 1].map(|side| {
        let (d0, d1) = (job.dom[0][side], job.dom[1][side]);
        let n = points.len() + 1;
        let mut dom = vec![d0];
        for (i, &p) in points.iter().enumerate() {
            let f = (i + 1) as f64 / n as f64;
            dom.push(invert(patches[side], p, d0 * (1.0 - f) + d1 * f));
        }
        dom.push(d1);
        dom
    });
    // The arcs must lie on the quadric's patch, not the conic's other side.
    let on_patch = curves.iter().all(|c| {
        let m = c.eval(0.5);
        let u = invert(patch, m, DVec3::splat(1.0 / 3.0));
        u.min_element() >= -0.25 && patch.eval(u).distance(m) <= 1e-9 * (1.0 + m.length())
    });
    on_patch.then_some(Chain {
        points,
        dom,
        curves,
        exact: true,
    })
}

/// An edge of either patch from one end of the arc to the other (at
/// their very places, as where the operands are flush: a cap's rim on
/// the other's cap), lying on the plane and the quadric: the cut runs
/// along it, and is it, whole. Halving it as the section's arcs are
/// would leave the cut beside the edge in pieces the edge isn't, a band
/// of zero width no split mends.
fn along_edge(job: &Job, n: DVec3, quadric: &Quadric) -> Option<Conic3> {
    let [x, y] = job.ends;
    let d = n.dot(x);
    [job.p, job.q]
        .iter()
        .flat_map(|patch| (0..3).map(|i| patch.edge(i)))
        .find_map(|e| {
            let e = if e.p0 == x && e.p1 == y {
                e
            } else if e.p0 == y && e.p1 == x {
                e.reversed()
            } else {
                return None;
            };
            let size = 1e-12 * (1.0 + x.abs().max_element().max(y.abs().max_element()));
            let on = [0.25, 0.5, 0.75].into_iter().all(|t| {
                let m = e.eval(t);
                (n.dot(m) - d).abs() <= size && quadric.distance(m) <= size
            });
            on.then_some(e)
        })
}

/// An end of the arc as a point on the curve, its tangent along the arc.
fn end(pair: &Pair, job: &Job, k: usize) -> Option<Point> {
    let [u, v] = job.dom[k];
    let x = job.ends[k];
    let t = pair.tangent(u, v)?;
    // At the `+` end the arc leaves its patch's side inwards; at the `−`
    // end it arrives from inside.
    let (patch, at) = if job.on_p[k] { (job.p, u) } else { (job.q, v) };
    let step = trace::domain_step(patch, at, t);
    // The coordinate that is 0 on the side the end is on: the smallest.
    let side = (0..3)
        .min_by(|&i, &j| at[i].abs().total_cmp(&at[j].abs()))
        .expect("three coordinates");
    let inwards = step[side] > 0.0;
    let forward = if k == 0 { inwards } else { !inwards };
    let tan = if forward { t } else { -t };
    Some(Point { x, u, v, tan })
}

/// The arc traced and fitted.
fn traced(job: &Job, fit: f64) -> Option<Chain> {
    let pair = job.pair();
    let (a, b) = (end(&pair, job, 0)?, end(&pair, job, 1)?);
    let points = trace::trace(&pair, a, b)?;
    let (inner, curves) = trace::fit(&pair, &points, fit / 4.0)?;
    // A planar face's points exactly on its plane.
    let plane = job
        .shapes
        .iter()
        .zip(job.planar)
        .find(|(_, planar)| *planar)
        .map(|(s, _)| *s);
    let points: Vec<DVec3> = inner
        .iter()
        .map(|p| plane.map_or(p.x, |s| s.onto(p.x)))
        .collect();
    let dom = [0, 1].map(|side| {
        let mut dom = vec![job.dom[0][side]];
        dom.extend(inner.iter().map(|p| if side == 0 { p.u } else { p.v }));
        dom.push(job.dom[1][side]);
        dom
    });
    Some(Chain {
        points,
        dom,
        curves,
        exact: false,
    })
}

/// The conic along the ends' tangents with its middle on the curve if it
/// can be found: where tracing fails.
fn fallback(job: &Job) -> Option<Chain> {
    let pair = job.pair();
    let (a, b) = (end(&pair, job, 0)?, end(&pair, job, 1)?);
    let conic = conic_along(&a, &b, |mid, c| {
        let tau = (c - mid).cross(b.x - a.x).cross(c - mid).try_normalize()?;
        pair.solve((a.u + b.u) * 0.5, (a.v + b.v) * 0.5, mid, tau)
            .map(|(x, _, _)| x)
    })?;
    Some(Chain {
        points: Vec::new(),
        dom: [vec![a.u, b.u], vec![a.v, b.v]],
        curves: vec![conic],
        exact: false,
    })
}
