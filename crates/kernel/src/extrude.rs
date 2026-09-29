//! Extruding a profile into a solid.
//!
//! [`extrude`] sweeps a [`Profile`] placed on a [`Frame`] along the frame's
//! normal, from one distance to another. The solid is exact: caps are
//! flat patches whose boundary edges are the profile's conics and whose
//! inner edges are straight, and each segment's wall is two patches
//! ([`cylinder_strip`]), flat for a straight segment and on the exact
//! cylinder over the conic for a curved one. Walls and caps share their
//! edge records, so the solid is closed by construction; repair then
//! splits whatever breaks the fold or hull rules, and the result passes
//! [`Mesh::check`].
//!
//! On the way the profile's segments are halved where they come near each
//! other (so the polygon of their chords is simple and every bulge clear
//! of the rest) and where a cap patch's corner would be too narrow or too
//! wide, and the caps get Steiner points where two of the loop's curves
//! meet smoothly. The rules are written down in `agents/kernel.md`.

use glam::{DMat3, DVec2, DVec3};

use crate::budget::{Budget, Work};
use crate::mesh::{Face, FaceName, FacePart, Mesh, MeshBuilder, Quadric, Surface};
use crate::patch::{Conic2, Conic3, PatchError, cylinder_strip};
use crate::profile::{Profile, ProfileError};
use crate::{KernelError, MAX_COORD, Solid, Tolerance};

mod cap;
mod chain;

use cap::Cap;
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

    /// The origin finite and within [`MAX_COORD`], the axes unit and
    /// square within [`Self::SLACK`].
    fn check(&self) -> Result<(), KernelError> {
        let m = self.origin.abs().max_element();
        if !(self.origin.is_finite() && m <= f64::from(MAX_COORD)) {
            return Err(PatchError::Coordinate(m).into());
        }
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
/// cylinder over their conic.
///
/// `from` must be below `to`, both within [`MAX_COORD`], and every corner
/// of the solid within [`MAX_COORD`] of the origin. The profile must pass
/// [`Profile::check`], and its segments must neither touch nor cross
/// within `tol`'s resolution ([`ProfileError::Touching`]), nest as outer
/// loops and holes ([`ProfileError::Nesting`]) and meet at no cusp.
/// Running out of `budget` fails with [`KernelError::TooComplex`], and a
/// solid too thin or too fine for the resolution with
/// [`KernelError::Invalid`] or `TooComplex`; it never gives an invalid
/// solid.
pub fn extrude(
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
    let cap = cap::triangulate(&mut chain, margin, &mut work)?;
    let mesh = build(&chain, &cap, frame, from, to, feature)?;
    work.spend(mesh.tris().len())?;
    let mesh = mesh.repair_within(tol, &mut work)?;
    Solid::new(mesh, tol)
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
    let bottom: Vec<DVec3> = points.iter().map(|&p| frame.point(p, from)).collect();
    let max = f64::from(MAX_COORD);
    for &p in &bottom {
        for p in [p, p + offset] {
            let m = p.abs().max_element();
            if !(p.is_finite() && m <= max) {
                return Err(PatchError::Coordinate(m).into());
            }
        }
    }

    let mut builder = MeshBuilder::new();
    for &p in &bottom {
        builder.vert(p);
    }
    for &p in &bottom {
        builder.vert(p + offset);
    }
    let up = bottom.len() as u32;
    let name = |part| FaceName { feature, part };
    let base = frame.origin + normal * from;
    let start = builder.face(Face {
        name: name(FacePart::StartCap),
        surface: Surface::Plane {
            n: -normal,
            d: -normal.dot(base),
        },
    });
    let end = builder.face(Face {
        name: name(FacePart::EndCap),
        surface: Surface::Plane {
            n: normal,
            d: normal.dot(base + offset),
        },
    });
    let sides: Vec<u32> = chain
        .sides
        .iter()
        .map(|side| {
            let surface = if side.curved {
                conic_cylinder(&side.conic, frame, from)
            } else {
                let chord = side.conic.p1 - side.conic.p0;
                // To the right of the segment, out of the region, and
                // square to the normal whether or not the axes are.
                let n = (frame.x * chord.x + frame.y * chord.y).cross(normal);
                Surface::Plane {
                    n,
                    d: n.dot(frame.point(side.conic.p0, from)),
                }
            };
            builder.face(Face {
                name: name(FacePart::Side {
                    curve: side.curve,
                    segment: side.segment,
                }),
                surface,
            })
        })
        .collect();

    for l in 0..starts.len() - 1 {
        let (first, last) = (starts[l], starts[l + 1]);
        for a0 in first..last {
            let a1 = if a0 + 1 == last { first } else { a0 + 1 };
            let (b0, b1) = (a0 + up, a1 + up);
            let seg = &segs[a0 as usize];
            if seg.curved {
                let ctrl = frame.point(seg.conic.c, from);
                let curve = Conic3 {
                    p0: bottom[a0 as usize],
                    c: ctrl,
                    w: seg.conic.w,
                    p1: bottom[a1 as usize],
                };
                let strip = cylinder_strip(&curve, offset)?;
                builder.edge(a0, a1, ctrl, seg.conic.w);
                builder.edge(b0, b1, ctrl + offset, seg.conic.w);
                builder.edge(a0, b1, strip[0].c[2], strip[0].w[2]);
            }
            let face = sides[seg.side as usize];
            builder.tri([a0, a1, b1], face);
            builder.tri([a0, b1, b0], face);
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
