//! Each cut arc's geometry: the chain of shared edges from its `+` end
//! to its `−` end that both faces of its pair are cut along.
//!
//! The pair's patches pick the path:
//!
//! - **Two planar patches** meet in a straight segment: one straight
//!   edge.
//! - **A plane and a quadric** (by the faces' tags) meet in a conic: the
//!   exact arc, halved where it turns far ([`section`]): the one on the
//!   side of the chord the quadric patch's middle is on, else the other,
//!   whichever lies on the patch; through a cone's apex its rulings
//!   ([`super::surface::ruling`]); square to a cone's axis, or on a
//!   sphere, a circle's arcs by their angles.
//! - **Two quadrics of revolution on one axis** (by the faces' forms)
//!   meet in a parallel: its exact arcs ([`super::coaxial`]), or an edge
//!   of either patch lying on both, whole.
//! - **Anything else**, or a conic that doesn't, is traced and fitted
//!   within the fit tolerance ([`trace`]). If tracing fails, the arc
//!   falls back to a simpler curve between the same ends: the conic
//!   along their tangents, or a straight edge, each kept only if it
//!   follows the true cut at three points ([`verified`]). Else the
//!   boolean is refused (`Inconsistent`): the faces' bands along a curve
//!   in the wrong place can still lie within the fit of their own
//!   surfaces, and a sliver between the two then loses material.
//!
//! Where one patch is planar, a traced chain's conics lie in its plane,
//! so planar faces stay planar; between two curved patches, in the plane
//! bisecting the crease the result has along the cut (see
//! [`trace::Crease`]).

use glam::DVec3;

use super::coaxial;
use super::curved::SeamRules;
use super::surface::{Guide, Shape, section};
use super::{segment, tie};
use crate::Tolerance;
use crate::mesh::{Form, Quadric};
use crate::par::par_map;
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
    /// What each patch's face was built to be.
    pub(super) forms: [Form; 2],
    /// Whether two quadrics of revolution on one axis may be cut in their
    /// parallel ([`parallel`]); else they are traced.
    pub(super) coaxial: bool,
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
    /// Whether the arc starts into both patches ([`inward_sign`]), and
    /// where to note that this changed its start.
    pub(super) rules: &'a SeamRules,
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
    /// Whether they are a parallel of two quadrics of revolution on one
    /// axis ([`parallel`]), a shortcut a failed operation is tried again
    /// without.
    pub(super) coaxial: bool,
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

#[cfg(test)]
thread_local! {
    /// Tests only: how many chains of a plane against a quadric weren't
    /// exact, counted on the thread that asks for the chains.
    pub(super) static NOT_EXACT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Tests only: how many chains were refused, counted the same way.
    pub(super) static REFUSED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Every job's chain (see [`chain`]), or the first job whose chain can't
/// be found near enough the true cut to trust, with the curve refused
/// for it: the boolean fails as `Inconsistent` then.
pub(super) fn chains(jobs: &[Job], tol: &Tolerance) -> Result<Vec<Chain>, (usize, Conic3)> {
    let chains = par_map(jobs, |job| chain(job, tol));
    #[cfg(test)]
    {
        NOT_EXACT.set(
            NOT_EXACT.get()
                + jobs
                    .iter()
                    .zip(&chains)
                    .filter(|(job, chain)| {
                        plane_and_quadric(job).is_some() && !chain.as_ref().is_ok_and(|c| c.exact)
                    })
                    .count(),
        );
        REFUSED.set(REFUSED.get() + chains.iter().filter(|c| c.is_err()).count());
    }
    if let Some((i, &Err(refused))) = chains.iter().enumerate().find(|(_, c)| c.is_err()) {
        return Err((i, refused));
    }
    Ok(chains.into_iter().flatten().collect())
}

/// The chain of `job`'s arc, within the fit tolerance of the true cut:
/// refused where neither the exact curve nor tracing gives it and the
/// fallbacks aren't near enough the true cut ([`verified`]), with the
/// first fallback tried (the conic along the ends' tangents if there is
/// one, else the straight edge) as the curve refused.
///
/// Ends at one place, or within the tie of each other (vertices of one
/// place by different roundings, as where a cap's corner lies on the
/// other operand's wall), make a straight edge of about zero length,
/// which the clean-up collapses: the exact section through two points a
/// rounding apart may run round the whole conic, outside both patches.
pub(super) fn chain(job: &Job, tol: &Tolerance) -> Result<Chain, Conic3> {
    let [x, y] = job.ends;
    let straight = |exact| Chain {
        points: Vec::new(),
        dom: [
            vec![job.dom[0][0], job.dom[1][0]],
            vec![job.dom[0][1], job.dom[1][1]],
        ],
        curves: vec![segment(x, y)],
        exact,
        coaxial: false,
    };
    if job.planar[0] && job.planar[1] || x.distance(y) <= tie(tol) {
        return Ok(straight(true));
    }
    if let Some(chain) = exact(job, tol.resolution()) {
        return Ok(chain);
    }
    if job.coaxial
        && let Some(chain) = parallel(job, tol.resolution())
    {
        return Ok(Chain {
            coaxial: true,
            ..chain
        });
    }
    if let Some(chain) = traced(job, tol.fit()) {
        return Ok(chain);
    }
    // The fallbacks know only the ends (and the conic its middle), not
    // where the cut runs between them: each is checked against it.
    let conic = match fallback(job) {
        Some(chain) if verified(job, &chain, tol) => return Ok(chain),
        conic => conic,
    };
    let chord = straight(false);
    if verified(job, &chord, tol) {
        return Ok(chord);
    }
    Err(conic.map_or(chord.curves[0], |chain| chain.curves[0]))
}

/// Whether a fallback's `chain` follows the true cut: at `¼`, `½` and `¾`
/// of each of its curves, the curve's point lies within the resolution
/// of both patches, as the chord of a tie or of a line contact does (it
/// is the cut), or the patches' cut on the plane square to the curve
/// there is within half the fit tolerance of it (by Newton's method from
/// the domain positions there, interpolated between its vertices').
/// Measured only against the faces the chain's bands lie on, a chord in
/// the wrong place passes wherever the two are within the fit of each
/// other along it: a plane `1e-4` off a cylinder's rulings, inside its
/// wall by as much, cuts it in a U 3.4 long whose chord, a tenth off the
/// cut, lies on the plane and within the fit of the wall, and the sliver
/// between them lost its tip.
///
/// A point within the resolution of both patches passes without Newton's
/// method: where the faces meet at a small angle `θ` it may be up to
/// about `resolution / θ` from the cut (Newton's method lands anywhere
/// along that band, or not at all at a line contact), but what lies
/// between it and the cut is thinner than a few resolutions. On a plane
/// or a quadric, the implicit value along a chord is a quadratic in its
/// parameter, along a conic a quartic over the weight's square; both
/// vanish at the ends, so the three samples bound them along the whole
/// curve, and the material a chord skips there is about the resolution
/// times the area between it and the cut.
fn verified(job: &Job, chain: &Chain, tol: &Tolerance) -> bool {
    let pair = job.pair();
    let (resolution, fit) = (tol.resolution(), tol.fit());
    let on = |patch: &Patch, x: DVec3, guess: DVec3| {
        patch.eval(invert(patch, x, guess)).distance(x) <= resolution
    };
    chain.curves.iter().enumerate().all(|(i, curve)| {
        [0.25, 0.5, 0.75].into_iter().all(|t| {
            let (x, d) = curve.eval_deriv(t);
            let [u, v] =
                [0, 1].map(|side| chain.dom[side][i] * (1.0 - t) + chain.dom[side][i + 1] * t);
            if on(job.p, x, u) && on(job.q, x, v) {
                return true;
            }
            let Some(tau) = d.try_normalize() else {
                return false;
            };
            pair.solve(u, v, x, tau)
                .is_some_and(|(y, _, _)| y.distance(x) <= fit / 2.0)
        })
    })
}

/// The exact arcs of a plane against a quadric, if the pair is one and
/// they stay on its patches.
fn exact(job: &Job, resolution: f64) -> Option<Chain> {
    let (plane, quadric, k) = plane_and_quadric(job)?;
    if let Some(edge) = along_edge(job, plane, &quadric) {
        return Some(Chain {
            points: Vec::new(),
            dom: [
                vec![job.dom[0][0], job.dom[1][0]],
                vec![job.dom[0][1], job.dom[1][1]],
            ],
            curves: vec![edge],
            exact: true,
            coaxial: false,
        });
    }
    // A circle round an axis square to the plane (a cone's, a sphere's):
    // its arcs by their angles.
    let patch = [job.p, job.q][k];
    if let Some(axis) = coaxial::square_axis(&job.forms[k], plane, patch, resolution)
        && let Some(curves) = coaxial::parallel(&axis, job.ends[0], job.ends[1], resolution)
    {
        let points: Vec<DVec3> = curves[1..].iter().map(|c| c.p0).collect();
        let dom = domains(job, &points);
        if curves
            .iter()
            .enumerate()
            .all(|(i, c)| coaxial::on_patch(patch, c, dom[k][i], dom[k][i + 1], resolution))
        {
            return Some(Chain {
                points,
                dom,
                curves,
                exact: true,
                coaxial: false,
            });
        }
    }
    // Near the arc: the quadric patch's point halfway between the ends in
    // its domain. Where the arc turns back within the patch, or the plane
    // nearly touches the patch along it, that point is on the chord or
    // nearly, and its side of it a rounding's: then the arc on the other
    // side. At most one of the two lies on the patch.
    let guide = patch.eval((job.dom[0][k] + job.dom[1][k]) * 0.5);
    exact_with(job, plane, &quadric, k, Guide::Near(guide))
        .or_else(|| exact_with(job, plane, &quadric, k, Guide::Away(guide)))
}

/// The exact arcs of the plane with unit normal `plane` against
/// `quadric`, on `job`'s patch `k`, between the ends through the point
/// `guide` picks, if they stay on that patch.
fn exact_with(job: &Job, plane: DVec3, quadric: &Quadric, k: usize, guide: Guide) -> Option<Chain> {
    let patches = [job.p, job.q];
    let patch = patches[k];
    let [x, y] = job.ends;
    let curves = section(quadric, plane, x, y, guide)?;
    let points: Vec<DVec3> = curves[1..].iter().map(|c| c.p0).collect();
    let dom = domains(job, &points);
    // The arcs must lie on the quadric's patch, not the conic's other side:
    // each one's middle inverted from between its ends' places in the
    // patch, or from the patch's middle (Newton's method from either may
    // stray on a long thin patch, a nearly flat cone's).
    let on_patch = curves.iter().enumerate().all(|(i, c)| {
        let m = c.eval(0.5);
        let near = (dom[k][i] + dom[k][i + 1]) * 0.5;
        [near, DVec3::splat(1.0 / 3.0)].into_iter().any(|guess| {
            let u = invert(patch, m, guess);
            u.min_element() >= -0.25 && patch.eval(u).distance(m) <= 1e-9 * (1.0 + m.length())
        })
    });
    on_patch.then_some(Chain {
        points,
        dom,
        curves,
        exact: true,
        coaxial: false,
    })
}

/// Every vertex's position in each of `job`'s patches, the ends' as
/// given and those of `points` between them inverted (from where they
/// would be spaced evenly).
fn domains(job: &Job, points: &[DVec3]) -> [Vec<DVec3>; 2] {
    let patches = [job.p, job.q];
    [0, 1].map(|side| {
        let (d0, d1) = (job.dom[0][side], job.dom[1][side]);
        let n = points.len() + 1;
        let mut dom = vec![d0];
        for (i, &p) in points.iter().enumerate() {
            let f = (i + 1) as f64 / n as f64;
            dom.push(invert(patches[side], p, d0 * (1.0 - f) + d1 * f));
        }
        dom.push(d1);
        dom
    })
}

/// The arcs of the parallel two quadrics of revolution on one axis meet
/// in (see [`coaxial`]), if the pair's are such and the arcs lie on both
/// patches within the resolution.
fn parallel(job: &Job, resolution: f64) -> Option<Chain> {
    let patches = [job.p, job.q];
    let axis = coaxial::common_axis(
        [&job.forms[0], &job.forms[1]],
        patches,
        &job.ends,
        resolution,
    )?;
    let [x, y] = job.ends;
    // Along an edge lying on both (two turned walls joined end to end): the
    // edge, whole.
    if let Some(edge) = along_edge_on(job, |m, size| {
        job.forms.iter().all(|f| f.distance(m) <= size)
    }) {
        return Some(Chain {
            points: Vec::new(),
            dom: [
                vec![job.dom[0][0], job.dom[1][0]],
                vec![job.dom[0][1], job.dom[1][1]],
            ],
            curves: vec![edge],
            exact: true,
            coaxial: false,
        });
    }
    let curves = coaxial::parallel(&axis, x, y, resolution)?;
    let points: Vec<DVec3> = curves[1..].iter().map(|c| c.p0).collect();
    let dom = domains(job, &points);
    let on = curves.iter().enumerate().all(|(i, c)| {
        (0..2).all(|side| {
            coaxial::on_patch(patches[side], c, dom[side][i], dom[side][i + 1], resolution)
        })
    });
    on.then_some(Chain {
        points,
        dom,
        curves,
        exact: true,
        coaxial: false,
    })
}

/// The plane's normal and the quadric of a pair of a planar face and a
/// quadric's, and which of the two patches is on the quadric.
fn plane_and_quadric(job: &Job) -> Option<(DVec3, Quadric, usize)> {
    match job.shapes {
        [Shape::Plane { n, .. }, Shape::Quadric(q)] => Some((n, q, 1)),
        [Shape::Quadric(q), Shape::Plane { n, .. }] => Some((n, q, 0)),
        _ => None,
    }
}

/// An edge of either patch from one end of the arc to the other (at
/// their very places, as where the operands are flush: a cap's rim on
/// the other's cap), lying on the plane and the quadric: the cut runs
/// along it, and is it, whole. Halving it as the section's arcs are
/// would leave the cut beside the edge in pieces the edge isn't, a band
/// of zero width no split mends.
fn along_edge(job: &Job, n: DVec3, quadric: &Quadric) -> Option<Conic3> {
    let d = n.dot(job.ends[0]);
    along_edge_on(job, |m, size| {
        (n.dot(m) - d).abs() <= size && quadric.distance(m) <= size
    })
}

/// An edge of either patch from one end of the arc to the other (at their
/// very places) on which `on(x, size)` holds at `¼`, `½` and `¾`, `size`
/// a rounding of the ends' coordinates.
fn along_edge_on(job: &Job, on: impl Fn(DVec3, f64) -> bool) -> Option<Conic3> {
    let [x, y] = job.ends;
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
            [0.25, 0.5, 0.75]
                .into_iter()
                .all(|t| on(e.eval(t), size))
                .then_some(e)
        })
}

/// An end of the arc as a point on the curve, its tangent along the arc.
fn end(pair: &Pair, job: &Job, k: usize) -> Option<Point> {
    let [u, v] = job.dom[k];
    let x = job.ends[k];
    let t = pair.tangent(u, v)?;
    // At the `+` end the arc leaves its patch's side inwards; at the `−`
    // end it arrives from inside. Without the rules, the side is the
    // smallest coordinate's, in the patch the end was found on.
    let old = || {
        let (patch, at) = if job.on_p[k] { (job.p, u) } else { (job.q, v) };
        let step = trace::domain_step(patch, at, t);
        let side = (0..3)
            .min_by(|&i, &j| at[i].abs().total_cmp(&at[j].abs()))
            .expect("three coordinates");
        step[side] > 0.0
    };
    let inwards = match job.rules.on().then(|| inward_sign(job, k, t)).flatten() {
        Some(s) => {
            let inwards = s > 0.0;
            if inwards != old() {
                job.rules.note();
            }
            inwards
        }
        None => old(),
    };
    let forward = if k == 0 { inwards } else { !inwards };
    let tan = if forward { t } else { -t };
    Some(Point { x, u, v, tan })
}

/// How far across a side, as a share of the step's largest component, a
/// step must go to leave or enter it rather than run along it: a
/// tangency's double root is placed some `1e-8` of the size off the
/// touching point, where the step across the side it touches reads as
/// `7e-9` of its length. Double roots read up to `4e-6` near the origin
/// and `2e-5` a thousand out, where real crossings start at `1.5e-5`;
/// those over the share go by their sign, as before it, and `1e-4`
/// decided the same operations in the sweeps.
const ALONG_SIDE: f64 = 1e-6;

/// Which way along `t` (`±1`) the arc goes into both patches from end
/// `k`: the way that leaves none of the sides the end lies on, in either
/// patch (coordinates within `1e-9` of 0: two at a corner, more where the
/// end is on both patches' boundaries), and so enters one of them; where
/// it runs along all of them (within [`ALONG_SIDE`] of the step: the cut
/// tangent to every side), the way towards the arc's other end. `None`
/// where both ways leave one (or the end is on no side, or the chord is
/// square to `t`): the smallest coordinate of the end's own patch decides.
/// Both leave one where the end lies on a side of its own patch and
/// within `1e-9` of one of the other's that the cut leaves: in the
/// sweeps only at tiny arcs (ends `5e-9` to `2e-6` apart along `t`),
/// where the chord's way decided the same operations.
///
/// The smallest coordinate of one patch alone is not enough where seam
/// rulings of two crossing cylinders meet on the cut, and refinement puts
/// a patch's corner there: the cut is tangent to the seam, and the first
/// zero coordinate may be the seam's, across which the step is rounding
/// (`6e-16` against `−1.1` across the corner's other side), so the trace
/// started out of the patch. Or the end lies inside a side of one patch,
/// the cut tangent to it (a straight ruling touching the other wall),
/// while it also lies on a side of the other patch. An arc between two
/// ends leaves a tangent end towards the other unless it turns back by
/// more than a right angle (fitting halves arcs turning past 45°, and the
/// rounds split long ones); if it does, the trace fails, an error. Ends
/// within the march's first step it joins at once, the short way: a
/// hairpin then fails the fit, and an arc back round beside its start
/// can't lie in a pair certified to meet in one arc (normal cones apart
/// keep `n_P × n_Q` in a half-space; see `agents/kernel.md`, "Chains").
fn inward_sign(job: &Job, k: usize, t: DVec3) -> Option<f64> {
    let [u, v] = job.dom[k];
    // Each patch's step and the end's place in it.
    let steps = [(job.p, u), (job.q, v)].map(|(patch, at)| (trace::domain_step(patch, at, t), at));
    let on = |at: DVec3, i: usize| at[i].abs() <= 1e-9;
    if !steps.iter().any(|&(_, at)| (0..3).any(|i| on(at, i))) {
        return None;
    }
    // Entering a side one way is leaving it the other: where one way
    // leaves none and the other leaves one, the first enters it.
    let leaves = |s: f64| {
        steps.iter().any(|&(step, at)| {
            let size = ALONG_SIDE * step.abs().max_element();
            (0..3).any(|i| on(at, i) && s * step[i] < -size)
        })
    };
    match (leaves(1.0), leaves(-1.0)) {
        (false, true) => Some(1.0),
        (true, false) => Some(-1.0),
        (false, false) => {
            let d = t.dot(job.ends[1 - k] - job.ends[k]);
            (d != 0.0).then(|| d.signum())
        }
        (true, true) => None,
    }
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
        coaxial: false,
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
        coaxial: false,
    })
}
