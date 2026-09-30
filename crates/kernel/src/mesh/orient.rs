//! Invariant 5: every shell faces the right way (see [`Mesh::check`]).
//!
//! Topology already makes each connected shell consistently oriented, so
//! what is left is one sign per shell and how the shells nest. A mesh
//! bounds a solid when its winding number is 0 or 1 everywhere. Its
//! shells don't meet (invariant 4), so that holds exactly when, for every
//! shell `S`, the other shells' winding number at a point of `S` is 0 if
//! `S` faces out (its volume is positive) and 1 if it faces in (a void):
//! every region of space borders some shell, and near `S` the two sides
//! are the others' winding number and that plus `S`'s sign.
//!
//! - **Shells**: the components of the triangles by halfedge pairs,
//!   numbered by their lowest triangle. A vertex has one fan, so shells
//!   share no vertex, and every patch of one is a non-neighbour of every
//!   patch of another: their hulls are more than the resolution apart.
//! - **Sign**: the shell's volume, bounded rather than integrated. The
//!   corner triangles' volume, summed in triangle order, then the
//!   difference each patch makes to it integrated ([`patch_volume`]) for
//!   the patches that could move it most, one by one, until what the rest
//!   could still move it by ([`lune_bound`]) and the rounding can't change
//!   its sign.
//! - **Nesting**: the other shells' winding number at the shell's first
//!   corner `p`, taken only when another shell's box holds `p` (a shell's
//!   winding number is 0 outside its box). It is the signed number of
//!   their **corner triangles** a ray from `p` passes through, each by
//!   exact signs. That is the curved surface's own: moving each patch
//!   straight onto its corner triangle (and each curved edge onto its
//!   chord, which the two patches beside it do alike) keeps every point
//!   in the patch's control hull, which stays more than the resolution
//!   from `p`, so the winding number round `p` doesn't change on the way.
//!   `p` is moved an infinitely small way in a fixed generic direction
//!   (the symbolic perturbation of [`exact`]), so the ray never grazes an
//!   edge or a corner: every sign is decided and there is no special case.

use glam::{DVec2, DVec3};

use super::check::CheckError;
use super::{Bvh, Mesh};
use crate::boolean::exact::{self, Num, Pred, Pt, V3, det, dir, sub};
use crate::par::par_map;
use crate::patch::{Bounds3, Patch};
use crate::solid::patch_volume;

/// The rays' direction: nearly `+z`, off every axis so walls along the
/// axes aren't edge-on to it, in small integers so the exact predicates
/// take it as it is.
const RAY: DVec3 = DVec3::new(2.0, 3.0, 32.0);

/// The direction a ray's start is moved in first, by an infinitely small
/// amount (then along [`exact::T2`] and [`exact::T3`]). With the two it
/// spans space, so the start lies on no plane through an edge along
/// [`RAY`] and no triangle's plane in every power of the perturbation.
const NUDGE: DVec3 = DVec3::new(0.6, 0.0, 0.8);

/// How far an integrated patch's volume may be off, relative to the
/// larger of it and its corner triangle's (the quadrature isn't exact for
/// rational patches; the tests hold it to `1e-12`).
const QUADRATURE: f64 = 1e-9;

/// The most patches of one shell integrated in one parallel batch. Batches
/// start at one patch and double, so a shell decided after a few wastes
/// little; which patches count as integrated doesn't depend on them.
const MAX_BATCH: usize = 64;

/// The mesh's shells: components of its triangles by halfedge pairs.
struct Shells {
    /// Each shell's triangles in ascending order, shell after shell.
    tris: Vec<u32>,
    /// Where each shell's triangles start in `tris`, and the end.
    starts: Vec<usize>,
    /// Each triangle's shell.
    of: Vec<u32>,
}

impl Shells {
    fn new(mesh: &Mesh) -> Shells {
        let n = mesh.tris.len();
        let mut of = vec![u32::MAX; n];
        let mut count = 0u32;
        let mut stack = Vec::new();
        for t in 0..n {
            if of[t] != u32::MAX {
                continue;
            }
            of[t] = count;
            stack.push(t);
            while let Some(u) = stack.pop() {
                for h in mesh.tris[u].halfedges {
                    let v = h.pair as usize / 3;
                    if of[v] == u32::MAX {
                        of[v] = count;
                        stack.push(v);
                    }
                }
            }
            count += 1;
        }
        let mut starts = vec![0usize; count as usize + 1];
        for &s in &of {
            starts[s as usize + 1] += 1;
        }
        for s in 0..count as usize {
            starts[s + 1] += starts[s];
        }
        let mut fill = starts.clone();
        let mut tris = vec![0u32; n];
        for (t, &s) in of.iter().enumerate() {
            tris[fill[s as usize]] = t as u32;
            fill[s as usize] += 1;
        }
        Shells { tris, starts, of }
    }

    fn len(&self) -> usize {
        self.starts.len() - 1
    }

    /// Shell `s`'s triangles, ascending.
    fn tris(&self, s: usize) -> &[u32] {
        &self.tris[self.starts[s]..self.starts[s + 1]]
    }
}

/// A triangle's share of its shell's volume, measured from the shell's
/// first corner.
#[derive(Debug, Clone, Copy)]
struct Share {
    /// The corner triangle's signed volume (the cone to the origin).
    tet: f64,
    /// A bound on how far rounding took `tet` from the exact value.
    err: f64,
    /// How much the patch could move the volume from `tet`
    /// ([`lune_bound`]).
    bound: f64,
}

impl Mesh {
    /// Invariant 5 on a mesh that passes 1 to 4, with `patches` its
    /// patches and `bvh` over their boxes: every shell faces out, or in
    /// where it bounds a void, and no shell lies where the others make
    /// it wrong (see the [module](self) docs). Returns how many patches'
    /// volumes it integrated. Fails with [`CheckError::InsideOut`] naming
    /// the lowest triangle of the first bad shell (shells in the order
    /// of their lowest triangles), also when a shell's volume is too
    /// close to zero for its sign to be told.
    pub(super) fn check_orientation(
        &self,
        patches: &[Patch],
        bvh: &Bvh,
        margin: f64,
    ) -> Result<usize, CheckError> {
        let shells = Shells::new(self);
        let origins: Vec<DVec3> = (0..shells.len())
            .map(|s| self.verts[self.first_corner(&shells, s) as usize])
            .collect();
        let tris: Vec<u32> = (0..self.tris.len() as u32).collect();
        let shares = par_map(&tris, |&t| {
            self.share(
                t,
                &patches[t as usize],
                origins[shells.of[t as usize] as usize],
            )
        });
        let boxes: Vec<Bounds3> = (0..shells.len())
            .map(|s| {
                let ts = shells.tris(s);
                ts[1..]
                    .iter()
                    .fold(bvh.bounds(ts[0]), |b, &t| b.union(bvh.bounds(t)))
            })
            .collect();
        let shell_bvh = Bvh::new(boxes.clone());
        let ids: Vec<usize> = (0..shells.len()).collect();
        let results = par_map(&ids, |&s| {
            let (sign, integrated) = shell_sign(self, shells.tris(s), &shares, patches, origins[s]);
            let others =
                || self.others_winding(s, origins[s], &shells, &boxes, &shell_bvh, bvh, margin);
            let right = match sign {
                Some(true) => others() == 0,
                Some(false) => others() == 1,
                None => false,
            };
            (right, integrated)
        });
        let integrated = results.iter().map(|&(_, n)| n).sum();
        match results.iter().position(|&(right, _)| !right) {
            Some(s) => Err(CheckError::InsideOut(shells.tris(s)[0])),
            None => Ok(integrated),
        }
    }

    /// Triangle `t`'s share of its shell's volume measured from `o`, its
    /// patch `patch`.
    fn share(&self, t: u32, patch: &Patch, o: DVec3) -> Share {
        let (six, err) = orient3d(o, self.corner_points(t));
        let tet = six / 6.0;
        Share {
            tet,
            err: err / 6.0 + f64::EPSILON * tet.abs(),
            bound: lune_bound(patch),
        }
    }

    /// The vertex at corner 0 of shell `s`'s lowest triangle.
    fn first_corner(&self, shells: &Shells, s: usize) -> u32 {
        self.tris[shells.tris(s)[0] as usize].halfedges[0].start
    }

    /// Triangle `t`'s corners' positions.
    fn corner_points(&self, t: u32) -> [DVec3; 3] {
        self.tris[t as usize]
            .halfedges
            .map(|h| self.verts[h.start as usize])
    }

    /// The winding number round `p`, a corner of shell `s`, of the other
    /// shells whose boxes hold it: the signed number of their corner
    /// triangles a ray from `p` along [`RAY`] passes through, `p` moved
    /// an infinitely small way along [`NUDGE`], [`exact::T2`] and
    /// [`exact::T3`]. The ray runs to the top of those boxes; `bvh`, over
    /// the patches' boxes, gives the triangles near it, and `margin`
    /// covers the rounding of its end.
    #[allow(clippy::too_many_arguments)]
    fn others_winding(
        &self,
        s: usize,
        p: DVec3,
        shells: &Shells,
        boxes: &[Bounds3],
        shell_bvh: &Bvh,
        bvh: &Bvh,
        margin: f64,
    ) -> i64 {
        let mut holding = Vec::new();
        shell_bvh.query(&Bounds3::point(p), 0.0, &mut holding);
        holding.retain(|&x| x as usize != s);
        if holding.is_empty() {
            return 0;
        }
        let top = holding
            .iter()
            .map(|&x| boxes[x as usize].max.z)
            .fold(p.z, f64::max);
        let end = p + RAY * ((top - p.z) / RAY.z);
        let mut near = Vec::new();
        bvh.query(&Bounds3::point(p).include(end), margin, &mut near);
        let start = Pt { p, n: Some(NUDGE) };
        near.iter()
            .filter(|&&t| holding.binary_search(&shells.of[t as usize]).is_ok())
            .map(|&t| i64::from(crossing(start, self.corner_points(t))))
            .sum()
    }
}

/// Whether the shell of triangles `tris`, measured from `o`, faces out
/// (`Some(true)`), faces in (`Some(false)`), or has a volume too close to
/// zero to tell (`None`), and how many patches' volumes that took
/// integrating.
///
/// The corner triangles' volume, `shares`' `tet` summed in triangle
/// order, with a bound on its rounding (each term's, and the sum's), is
/// off the shell's by what each patch adds to its triangle's. Patches are integrated in order of how much they could
/// add (`bound`, largest first, then by index) until the volume is further
/// from zero than the rest of them could move it plus the rounding and
/// the integrals' error ([`QUADRATURE`]). Where the corner triangles'
/// rounding is what leaves it open (their cones from `o` cancel to digits
/// floating point doesn't have), their volume is worked out exactly, once.
fn shell_sign(
    mesh: &Mesh,
    tris: &[u32],
    shares: &[Share],
    patches: &[Patch],
    o: DVec3,
) -> (Option<bool>, usize) {
    let share = |t: u32| shares[t as usize];
    // Summed with every addition's error carried (Ogita, Rump and Oishi's
    // `Sum2`): off by at most `u·|sum| + γ²·Σ|terms|`, `γ = n·u / (1 −
    // n·u)`, `u = 2⁻⁵³`, so the sum's own rounding doesn't grow with the
    // number of triangles as a plain one's `γ·Σ|terms|` does.
    let (mut high, mut low, mut flat_err, mut sizes) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for &t in tris {
        let (sum, error) = two_sum(high, share(t).tet);
        (high, low) = (sum, low + error);
        flat_err += share(t).err;
        sizes += share(t).tet.abs();
    }
    let mut flat = high + low;
    let u = f64::EPSILON / 2.0;
    let gamma = (tris.len() as f64 + 2.0) * u / (1.0 - (tris.len() as f64 + 2.0) * u);
    flat_err += 2.0 * u * flat.abs() + 2.0 * gamma * gamma * sizes;
    let mut exact = false;
    let mut order: Vec<(f64, u32)> = tris
        .iter()
        .map(|&t| (share(t).bound, t))
        .filter(|&(b, _)| b > 0.0)
        .collect();
    order.sort_by(|x, y| y.0.total_cmp(&x.0).then(x.1.cmp(&y.1)));
    // What the patches from each on could still move it by.
    let mut rest = vec![0.0f64; order.len() + 1];
    for k in (0..order.len()).rev() {
        rest[k] = rest[k + 1] + order[k].0;
    }
    // What the integrated patches add, and its error.
    let (mut fix, mut fix_err) = (0.0f64, 0.0f64);
    let mut k = 0;
    let mut batch = 1;
    let mut volumes: Vec<f64> = Vec::new();
    loop {
        let v = flat + fix;
        let slack = flat_err + fix_err + 2.0 * f64::EPSILON * (flat.abs() + fix.abs());
        if v.abs() > rest[k] + slack {
            return (Some(v > 0.0), k);
        }
        if !exact && (k == order.len() || flat_err > rest[k] + fix_err) {
            let corners = Corners { mesh, tris, o };
            flat = exact::value(&corners) / 6.0;
            flat_err = 8.0 * f64::EPSILON * flat.abs();
            exact = true;
            continue;
        }
        if k == order.len() {
            return (None, k);
        }
        if volumes.is_empty() {
            let end = (k + batch).min(order.len());
            volumes = par_map(&order[k..end], |&(_, t)| {
                patch_volume(&patches[t as usize], o)
            });
            volumes.reverse();
            batch = (2 * batch).min(MAX_BATCH);
        }
        let volume = volumes.pop().expect("a batch");
        let s = share(order[k].1);
        fix += volume - s.tet;
        fix_err += QUADRATURE * volume.abs().max(s.tet.abs())
            + s.err
            + 2.0 * f64::EPSILON * (volume.abs() + s.tet.abs() + fix.abs());
        k += 1;
    }
}

/// `6·` the volume of a shell's corner triangles, `tris`, measured from
/// `o`: the sum of `det[a − o, b − o, c − o]`.
struct Corners<'a> {
    mesh: &'a Mesh,
    tris: &'a [u32],
    o: DVec3,
}

impl Pred for Corners<'_> {
    fn eval<N: Num>(&self) -> N {
        let o = unmoved::<N>(self.o);
        self.tris.iter().fold(N::lit(0.0), |sum, &t| {
            let [a, b, c] = self
                .mesh
                .corner_points(t)
                .map(|p| sub(&unmoved::<N>(p), &o));
            sum.add(&det(&a, &b, &c))
        })
    }
}

/// `p` as a point of a predicate, not perturbed.
fn unmoved<N: Num>(p: DVec3) -> V3<N> {
    Pt { p, n: None }.v3()
}

/// `a + b` and its rounding error, exactly (Knuth).
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let x = a + b;
    let bv = x - a;
    let av = x - bv;
    (x, (a - av) + (b - bv))
}

/// `det[a − o, b − o, c − o]` in floating point, and a bound on its
/// rounding error: Shewchuk's first bound for his `orient3d`, which is
/// worked out the same way, `(7 + 56ε)ε` times the permanent (the same
/// sum with every product's absolute value), `ε = 2⁻⁵³`.
fn orient3d(o: DVec3, [a, b, c]: [DVec3; 3]) -> (f64, f64) {
    const EPS: f64 = f64::EPSILON / 2.0;
    const BOUND: f64 = (7.0 + 56.0 * EPS) * EPS;
    let (a, b, c) = (a - o, b - o, c - o);
    let (bc, cb) = (b.y * c.z, b.z * c.y);
    let (ca, ac) = (c.y * a.z, c.z * a.y);
    let (ab, ba) = (a.y * b.z, a.z * b.y);
    let det = a.x * (bc - cb) + b.x * (ca - ac) + c.x * (ab - ba);
    let permanent = a.x.abs() * (bc.abs() + cb.abs())
        + b.x.abs() * (ca.abs() + ac.abs())
        + c.x.abs() * (ab.abs() + ba.abs());
    (det, BOUND * permanent)
}

/// A bound on how much `patch` changes the volume a shell encloses from
/// that of its corner triangle: the patch, its triangle and the lunes
/// between its curved edges and their chords (which the two patches
/// beside an edge share, turned opposite ways, so they cancel in a
/// shell) lie in its control points' hull, as its weights are positive,
/// so the volume between them is no more than the hull's. That is no more
/// than the prism of the hull's shadow on the triangle's plane (the
/// convex hull of the control points' shadows) as deep as the control
/// points lie either side of the plane; twice that, for safety. Infinite
/// for a triangle with no plane.
pub(crate) fn lune_bound(patch: &Patch) -> f64 {
    let [p0, p1, p2] = patch.p;
    let Some(n) = (p1 - p0).cross(p2 - p0).try_normalize() else {
        return f64::INFINITY;
    };
    let Some(u) = (p1 - p0).try_normalize() else {
        return f64::INFINITY;
    };
    let v = n.cross(u);
    let (mut low, mut high) = (0.0f64, 0.0f64);
    let shadow = [p0, p1, p2, patch.c[0], patch.c[1], patch.c[2]].map(|x| {
        let d = x - p0;
        let h = d.dot(n);
        (low, high) = (low.min(h), high.max(h));
        DVec2::new(d.dot(u), d.dot(v))
    });
    2.0 * (high - low) * hull_area(shadow)
}

/// The area of the convex hull of `points` (Andrew's monotone chain).
fn hull_area(mut points: [DVec2; 6]) -> f64 {
    points.sort_unstable_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    let turn = |o: DVec2, a: DVec2, b: DVec2| (a - o).perp_dot(b - o);
    // The lower chain, then the upper back to the first point.
    let mut hull = [DVec2::ZERO; 12];
    let mut n = 0;
    for &p in &points {
        while n >= 2 && turn(hull[n - 2], hull[n - 1], p) <= 0.0 {
            n -= 1;
        }
        hull[n] = p;
        n += 1;
    }
    let lower = n + 1;
    for &p in points.iter().rev().skip(1) {
        while n >= lower && turn(hull[n - 2], hull[n - 1], p) <= 0.0 {
            n -= 1;
        }
        hull[n] = p;
        n += 1;
    }
    (0..n - 1)
        .map(|i| hull[i].perp_dot(hull[i + 1]))
        .sum::<f64>()
        .abs()
        / 2.0
}

/// Whether a ray from `start` along [`RAY`] passes through the triangle
/// `a`, `b`, `c`: its sign where it does (+1 leaving through its outer
/// side, the side its corners run counter-clockwise round), else 0.
///
/// With `A = a − start` and so on, and `D = det[A, B, C]`, the ray's
/// direction is `αA + βB + γC`, and `det[A, B, RAY] = γD`,
/// `det[B, C, RAY] = αD`, `det[C, A, RAY] = βD`: it passes through the
/// triangle when all three have `D`'s sign, and leaves through the outer
/// side when `D` is positive (`D` is the normal's dot with `A`). The
/// perturbed start leaves no sign zero but where it has to be: `D` for a
/// triangle whose corners are on a line (which the ray misses), and an
/// edge's for an edge along the ray (then the ray runs alongside the
/// triangle's plane, off it, and misses it too).
fn crossing(start: Pt, [a, b, c]: [DVec3; 3]) -> i8 {
    let d = exact::sign(&Volume { start, a, b, c });
    if d == 0 {
        return 0;
    }
    let through = [(a, b), (b, c), (c, a)]
        .into_iter()
        .all(|(a, b)| exact::sign(&Beside { start, a, b }) == d);
    if through { d } else { 0 }
}

/// `det[a − start, b − start, RAY]`: which side of the plane through `a`
/// and `b` along the ray `start` lies on.
struct Beside {
    start: Pt,
    a: DVec3,
    b: DVec3,
}

impl Pred for Beside {
    fn eval<N: Num>(&self) -> N {
        let s = self.start.v3::<N>();
        let [a, b] = [self.a, self.b].map(unmoved::<N>);
        det(&sub(&a, &s), &sub(&b, &s), &dir(RAY))
    }
}

/// `det[a − start, b − start, c − start]`: which side of the triangle's
/// plane `start` lies on.
struct Volume {
    start: Pt,
    a: DVec3,
    b: DVec3,
    c: DVec3,
}

impl Pred for Volume {
    fn eval<N: Num>(&self) -> N {
        let s = self.start.v3::<N>();
        let [a, b, c] = [self.a, self.b, self.c].map(unmoved::<N>);
        det(&sub(&a, &s), &sub(&b, &s), &sub(&c, &s))
    }
}

#[cfg(test)]
mod tests;
