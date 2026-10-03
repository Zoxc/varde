//! Extruding a profile into a solid.
//!
//! [`extrude`] sweeps a [`Profile`] placed on a [`Frame`] along the frame's
//! normal, from one distance to another. The solid is exact: caps are
//! flat patches whose boundary edges are the profile's conics and whose
//! inner edges are straight, and each segment's wall is two patches
//! ([`cylinder_strip`](crate::patch::cylinder_strip)), flat for a
//! straight segment and on the exact cylinder over the conic for a curved
//! one. Walls and caps share their
//! edge records, so the solid is closed by construction; repair then
//! splits whatever breaks the fold or hull rules, and the result passes
//! [`Mesh::check`].
//!
//! On the way the profile's segments are halved where they come near each
//! other (so the polygon of their chords is simple and every bulge clear
//! of the rest) and where a cap patch's corner would be too narrow or too
//! wide, and the caps get Steiner points where two of the loop's curves
//! meet smoothly, at the circumcentres of triangles with an angle under
//! 5° (refinement for quality) and, on a second try if the first fails,
//! where short segments meet nearly straight. The rules are written down
//! in `agents/kernel.md`.

use glam::{DMat3, DVec2, DVec3};

use crate::budget::{Budget, Work};
use crate::mesh::{Face, FaceName, FacePart, Form, Mesh, MeshBuilder, Quadric, Surface, circle_of};
use crate::patch::{Conic2, Conic3, PatchError};
use crate::profile::evidence::profile_failure;
use crate::profile::{Profile, ProfileError};
use crate::{Failure, KernelError, MAX_COORD, Solid, Tolerance, in_range};

pub(crate) mod cap;
pub(crate) mod chain;

use cap::{Cap, Mode, Rounds};
use chain::Chain;

/// Where a profile lies in space: its origin and the unit axes its `x`
/// and `y` run along. The normal, `x × y`, is the direction it is
/// extruded in, and counter-clockwise is seen from the side it points to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub origin: DVec3,
    pub x: DVec3,
    pub y: DVec3,
}

impl Frame {
    /// The world's XY plane.
    pub const XY: Frame = Frame {
        origin: DVec3::ZERO,
        x: DVec3::X,
        y: DVec3::Y,
    };

    /// How far the axes may be from unit length and square, as a
    /// relative error.
    const SLACK: f64 = 1e-9;

    /// `x × y`.
    pub fn normal(&self) -> DVec3 {
        self.x.cross(self.y)
    }

    /// The point `p` of the profile's plane, moved `height` along the
    /// normal.
    pub fn point(&self, p: DVec2, height: f64) -> DVec3 {
        self.origin + self.x * p.x + self.y * p.y + self.normal() * height
    }

    /// The conic `c` of the profile's plane, moved `height` along the
    /// normal: its control points placed by [`Frame::point`], unchecked.
    pub(crate) fn conic(&self, c: &Conic2, height: f64) -> Conic3 {
        Conic3 {
            p0: self.point(c.p0, height),
            c: self.point(c.c, height),
            w: c.w,
            p1: self.point(c.p1, height),
        }
    }

    /// The origin finite and within [`MAX_COORD`], the axes unit and
    /// square within [`Self::SLACK`].
    pub(crate) fn check(&self) -> Result<(), KernelError> {
        in_range(self.origin)?;
        let unit = |v: DVec3| (v.length() - 1.0).abs() <= Self::SLACK;
        if !(unit(self.x) && unit(self.y) && self.x.dot(self.y).abs() <= Self::SLACK) {
            return Err(PatchError::Degenerate.into());
        }
        Ok(())
    }
}

/// The solid swept by `profile`, placed on `frame`, from `from` to `to`
/// along the frame's normal, as the extrude feature `feature` names its
/// faces: its start cap (at `from`, facing back) is
/// [`FacePart::StartCap`], its end cap [`FacePart::EndCap`], and the wall
/// of the `n`-th segment (in profile order) of curve `c` is
/// [`FacePart::Side`]` { curve: c, segment: n }`. Caps are tagged with
/// their planes, straight walls with theirs and curved walls with the
/// cylinder over their conic; their forms are the planes, and
/// [`Form::Cylinder`] for walls over circular arcs,
/// [`Form::ConicCylinder`] over other conics.
///
/// `from` must be below `to`, both within [`MAX_COORD`], and every corner
/// of the solid within [`MAX_COORD`] of the origin. The profile must pass
/// [`Profile::check`], and its segments must neither touch nor cross
/// within `tol`'s resolution ([`ProfileError::Touching`]), nest as outer
/// loops and holes ([`ProfileError::Nesting`]) and meet at no cusp.
/// A solid too thin or too fine for the resolution fails with
/// [`KernelError::Invalid`] or [`ProfileError::TooFine`], and running out
/// of `budget` or past a limit with [`KernelError::TooComplex`]; it never
/// gives an invalid solid. A profile's error comes with the segments and
/// points it is about, placed on `frame` at height 0, and their sketch
/// curves, as [`Failure::evidence`].
pub fn extrude(
    profile: &Profile,
    frame: &Frame,
    from: f64,
    to: f64,
    feature: u64,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Solid, Failure> {
    extruded(profile, frame, from, to, feature, tol, budget)
        .map_err(|error| profile_failure(error, profile, frame, tol))
}

/// [`extrude`], failing with the error alone.
fn extruded(
    profile: &Profile,
    frame: &Frame,
    from: f64,
    to: f64,
    feature: u64,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Solid, KernelError> {
    profile.check().map_err(KernelError::Profile)?;
    frame.check()?;
    let max = f64::from(MAX_COORD);
    for h in [from, to] {
        // NaN fails too.
        if h.is_nan() || h.abs() > max {
            return Err(PatchError::Coordinate(h.abs()).into());
        }
    }
    if from >= to {
        return Err(PatchError::Parameter(to - from).into());
    }
    let margin = tol.resolution();
    let mut work = Work::new(budget);
    let mut chain = Chain::new(profile, margin)?;
    chain.separate(&mut work)?;
    let solid = |cap: Result<(Chain, Cap), KernelError>, work: &mut Work| {
        let (chain, cap) = cap?;
        let mesh = build(&chain, &cap, frame, from, to, feature)?;
        work.spend(mesh.tris().len())?;
        // Built to pass: checked first, repaired only if it fails. A face
        // per surface: collinear segments' walls, and the arcs of one
        // circle, are one face.
        Solid::new_repaired_within(mesh, tol, work)
    };
    // Caps with slivers along short segments meeting nearly straight fail
    // the hull rules next to the walls. Moving Steiner points in from such
    // corners mends most, but can line the points up into slivers of
    // their own, so it is the second try. It resumes from the first
    // round of the first try that found such a corner: until then the two
    // are the same. With none, it would repeat the first try (with less
    // work left, so it couldn't do better) and isn't made. Both refine
    // the caps for quality; refining a sliver of the region thinner than
    // the pieces the chain may be halved into can leave it worse than the
    // plain caps, so the last tries are the plain caps' two, made as the
    // two above are, so what those mended they mend still, work allowing:
    // the plain first try if the first try did anything it wouldn't
    // (refinement asked for something, or mending left a halving to it
    // that the plain caps would make), else it would repeat it; then the
    // plain second try from its fork, if the refined second try did
    // anything it wouldn't.
    let retry = |e: &KernelError| {
        matches!(
            e,
            KernelError::Invalid(_)
                | KernelError::TooComplex
                | KernelError::Profile(ProfileError::TooFine(..))
        )
    };
    let mut attempt = |start, mode, fork: &mut Option<Rounds>, unlike: &mut bool| {
        let caps = cap::triangulate(start, margin, mode, fork, unlike, &mut work);
        let result = solid(caps, &mut work);
        (result, work.left() > 0)
    };
    let (mut fork, mut unlike_plain) = (None, false);
    let first = match attempt(
        Rounds::new(chain.clone()),
        Mode::QUALITY,
        &mut fork,
        &mut unlike_plain,
    ) {
        (Err(e), true) if retry(&e) => e,
        (result, _) => return result,
    };
    // Whether the refined second try did anything the plain one wouldn't.
    let mut second_unlike = false;
    if let Some(fork) = &fork {
        match attempt(
            fork.clone(),
            Mode::FLAT_CORNERS,
            &mut None,
            &mut second_unlike,
        ) {
            (Err(_), true) => {}
            (Err(_), false) => return Err(first),
            (result, _) => return result,
        }
    }
    let plain_fork = if unlike_plain {
        let mut plain_fork = None;
        match attempt(Rounds::new(chain), Mode::PLAIN, &mut plain_fork, &mut false) {
            (Err(_), true) => {}
            (Err(_), false) => return Err(first),
            (result, _) => return result,
        }
        plain_fork
    } else if second_unlike {
        // The first try was the plain one, fork and all.
        fork
    } else {
        None
    };
    let Some(plain_fork) = plain_fork else {
        return Err(first);
    };
    let (result, _) = attempt(plain_fork, Mode::FLAT_CORNERS_PLAIN, &mut None, &mut false);
    result.map_err(|_| first)
}

/// The closed mesh: the chain's vertices and the Steiner points at `from`,
/// then again at `to`; the walls; the caps.
fn build(
    chain: &Chain,
    cap: &Cap,
    frame: &Frame,
    from: f64,
    to: f64,
    feature: u64,
) -> Result<Mesh, KernelError> {
    let normal = frame.normal();
    let offset = normal * (to - from);
    let (segs, starts) = chain.flat();
    let points: Vec<DVec2> = segs
        .iter()
        .map(|s| s.conic.p0)
        .chain(cap.steiner.iter().copied())
        .collect();
    // The top placed at `to` itself, not the bottom moved by the offset:
    // `from + (to − from)` may round away from `to`, and a solid extruded
    // from `to` on (a boss on this one's top) would then stand a rounding
    // off flush instead of on it.
    let bottom: Vec<DVec3> = points.iter().map(|&p| frame.point(p, from)).collect();
    let top: Vec<DVec3> = points.iter().map(|&p| frame.point(p, to)).collect();
    for &p in bottom.iter().chain(&top) {
        in_range(p)?;
    }

    let mut builder = MeshBuilder::new();
    for &p in bottom.iter().chain(&top) {
        builder.vert(p);
    }
    let up = bottom.len() as u32;
    let name = |part| FaceName::new(feature, part);
    let plane = |n: DVec3, d: f64| (Surface::Plane { n, d }, Form::plane(n, d));
    let (surface, form) = plane(-normal, -normal.dot(frame.origin + normal * from));
    let start = builder.face(Face {
        name: name(FacePart::StartCap),
        surface,
        form,
        slack: 1.0,
    });
    let (surface, form) = plane(normal, normal.dot(frame.origin + normal * to));
    let end = builder.face(Face {
        name: name(FacePart::EndCap),
        surface,
        form,
        slack: 1.0,
    });
    let sides: Vec<u32> = chain
        .sides
        .iter()
        .map(|side| {
            let (surface, form) = if side.curved {
                (
                    conic_cylinder(&side.conic, frame, from),
                    wall_form(&side.conic, frame, from),
                )
            } else {
                let chord = side.conic.p1 - side.conic.p0;
                // To the right of the segment, out of the region, and
                // square to the normal whether or not the axes are.
                let n = (frame.x * chord.x + frame.y * chord.y).cross(normal);
                plane(n, n.dot(frame.point(side.conic.p0, from)))
            };
            builder.face(Face {
                name: name(FacePart::Side {
                    curve: side.curve,
                    segment: side.segment,
                }),
                surface,
                form,
                slack: 1.0,
            })
        })
        .collect();

    for l in 0..starts.len() - 1 {
        let (first, last) = (starts[l], starts[l + 1]);
        for a0 in first..last {
            let a1 = if a0 + 1 == last { first } else { a0 + 1 };
            let (a, b) = ([a0, a1], [a0 + up, a1 + up]);
            let seg = &segs[a0 as usize];
            let face = sides[seg.side as usize];
            if seg.curved {
                let curve = Conic3 {
                    p0: bottom[a0 as usize],
                    c: frame.point(seg.conic.c, from),
                    w: seg.conic.w,
                    p1: bottom[a1 as usize],
                };
                builder.curved_wall(a, b, &curve, offset, face)?;
            } else {
                builder.wall(a, b, face);
            }
        }
    }
    for &[a, b, c] in &cap.tris {
        builder.tri([a, c, b], start);
        builder.tri([a + up, b + up, c + up], end);
    }
    builder
        .build()
        .map_err(|_| KernelError::Profile(ProfileError::Triangulation))
}

/// What a curved wall over `conic`, placed on `frame` at `height`, is: a
/// [`Form::Cylinder`] if the conic is a circle's arc (`circle_of`), else
/// a [`Form::ConicCylinder`]; both along the frame's normal.
fn wall_form(conic: &Conic2, frame: &Frame, height: f64) -> Form {
    let axis = frame.normal().normalize();
    match circle_of(conic) {
        Some((centre, radius)) => Form::Cylinder {
            point: frame.point(centre, height),
            axis,
            radius,
        },
        None => Form::ConicCylinder {
            conic: frame.conic(conic, height),
            along: axis,
        },
    }
}

/// The cylinder over `conic`, placed on `frame` at `height`, along the
/// normal. With `λ` the barycentric coordinates of a point's projection
/// on the conic's control triangle `p0, c, p1`, the conic of weight `w` is
/// `λ1² = 4w²·λ0·λ2` (its points have `λ = ((1-t)², 2wt(1-t), t²) / D`).
/// The `λ` are affine in the point and don't change along the normal, so
/// this is a quadric; it is written around `p0`, where `λ = (1, 0, 0)`.
fn conic_cylinder(conic: &Conic2, frame: &Frame, height: f64) -> Surface {
    let (e1, e2) = (conic.c - conic.p0, conic.p1 - conic.p0);
    let det = e1.perp_dot(e2);
    // λ1 = (d × e2) / det and λ2 = (e1 × d) / det for d = q − p0.
    let a1 = DVec2::new(e2.y, -e2.x) / det;
    let a2 = DVec2::new(-e1.y, e1.x) / det;
    let a0 = -a1 - a2;
    // A point's plane coordinates are `(dx·y, dy·y)` with the dual axes
    // of `x`, `y` and the normal, exact even for axes a little off square.
    let normal = frame.normal();
    let det = frame.x.dot(frame.y.cross(normal));
    let (dx, dy) = (frame.y.cross(normal) / det, normal.cross(frame.x) / det);
    let lift = |a: DVec2| dx * a.x + dy * a.y;
    let (g0, g1, g2) = (lift(a0), lift(a1), lift(a2));
    let outer = |u: DVec3, v: DVec3| DMat3::from_cols(u * v.x, u * v.y, u * v.z);
    let w2 = conic.w * conic.w;
    // λ1² − 4w²·(g0·y + 1)(g2·y), y from p0.
    let a = outer(g1, g1) - (outer(g0, g2) + outer(g2, g0)) * (2.0 * w2);
    Surface::Quadric(Quadric {
        origin: frame.point(conic.p0, height),
        a,
        b: g2 * (-2.0 * w2),
        c: 0.0,
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod quality_tests;
