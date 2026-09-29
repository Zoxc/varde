//! Boxes and cylinders as patch meshes, built the way an extrude builds
//! its solids: a start cap, an end cap and a wall per profile segment,
//! named and tagged accordingly.

use std::f64::consts::FRAC_1_SQRT_2;

use glam::DVec3;

use super::{Face, FaceName, FacePart, Mesh, MeshBuilder, Quadric, Surface};
use crate::patch::{Conic3, PatchError, cylinder_strip};
use crate::{KernelError, MAX_COORD, Tolerance};

impl Mesh {
    /// The axis-aligned box from `min` to `min + size`, as the extrusion
    /// of its bottom rectangle along `+z`: two flat triangles to a side.
    /// The bottom is the [`FacePart::StartCap`] and the top the
    /// [`FacePart::EndCap`] of `feature`; the sides are
    /// [`FacePart::Side`]s of curves 0 to 3, the rectangle's edges counter-
    /// clockwise seen from `+z` from the one along `-y`, segment 0. Every
    /// face is tagged with its plane.
    ///
    /// Every corner must be within [`MAX_COORD`] of the origin and every
    /// size above zero, and the box must pass [`Mesh::check`] with `tol`
    /// (a box much thinner than the resolution doesn't).
    pub fn cuboid(
        min: DVec3,
        size: DVec3,
        feature: u64,
        tol: &Tolerance,
    ) -> Result<Mesh, KernelError> {
        let max = min + size;
        in_range(min)?;
        in_range(max)?;
        positive(size.min_element())?;
        let mut builder = MeshBuilder::new();
        let corner = |x: f64, y: f64| [DVec3::new(x, y, min.z), DVec3::new(x, y, max.z)];
        let rect = [
            corner(min.x, min.y),
            corner(max.x, min.y),
            corner(max.x, max.y),
            corner(min.x, max.y),
        ];
        let bottom = rect.map(|c| builder.vert(c[0]));
        let top = rect.map(|c| builder.vert(c[1]));
        let [start, end] = caps(&mut builder, feature, min.z, max.z);
        builder.tri([bottom[0], bottom[2], bottom[1]], start);
        builder.tri([bottom[0], bottom[3], bottom[2]], start);
        builder.tri([top[0], top[1], top[2]], end);
        builder.tri([top[0], top[2], top[3]], end);
        let normals = [DVec3::NEG_Y, DVec3::X, DVec3::Y, DVec3::NEG_X];
        for k in 0..4 {
            let n = normals[k];
            let face = builder.face(Face {
                name: FaceName {
                    feature,
                    part: FacePart::Side {
                        curve: k as u64,
                        segment: 0,
                    },
                },
                surface: Surface::Plane {
                    n,
                    d: n.dot(rect[k][0]),
                },
            });
            let (a0, a1, b0, b1) = (bottom[k], bottom[(k + 1) % 4], top[k], top[(k + 1) % 4]);
            builder.tri([a0, a1, b1], face);
            builder.tri([a0, b1, b0], face);
        }
        finish(builder, tol)
    }

    /// The circular cylinder of `radius` standing on `base`, the centre of
    /// its bottom, and rising `height` along `+z`, as the extrusion of a
    /// circle of four exact quarter arcs: each wall an exact
    /// [`cylinder_strip`], each cap four quarter discs around its centre.
    /// The bottom is the [`FacePart::StartCap`] and the top the
    /// [`FacePart::EndCap`] of `feature`, tagged with their planes; the
    /// walls are [`FacePart::Side`]s of curve 0, segments 0 to 3 counter-
    /// clockwise seen from `+z` from `+x`, tagged with the cylinder.
    ///
    /// Every point of it must be within [`MAX_COORD`] of the origin, the
    /// radius and height above zero, and the cylinder must pass
    /// [`Mesh::check`] with `tol`.
    pub fn cylinder(
        base: DVec3,
        radius: f64,
        height: f64,
        feature: u64,
        tol: &Tolerance,
    ) -> Result<Mesh, KernelError> {
        positive(radius)?;
        positive(height)?;
        let up = DVec3::Z * height;
        in_range(base - DVec3::new(radius, radius, 0.0))?;
        in_range(base + DVec3::new(radius, radius, 0.0) + up)?;
        let mut builder = MeshBuilder::new();
        let rim = [DVec3::X, DVec3::Y, DVec3::NEG_X, DVec3::NEG_Y].map(|d| base + d * radius);
        let bottom = rim.map(|p| builder.vert(p));
        let top = rim.map(|p| builder.vert(p + up));
        let centres = [builder.vert(base), builder.vert(base + up)];
        let [start, end] = caps(&mut builder, feature, base.z, base.z + height);
        let wall = Surface::Quadric(
            Quadric::cylinder(base, DVec3::Z, radius).ok_or(PatchError::Parameter(radius))?,
        );
        for k in 0..4 {
            let (p, q) = (rim[k], rim[(k + 1) % 4]);
            let ctrl = p + q - base;
            let arc = Conic3::new(p, ctrl, FRAC_1_SQRT_2, q)?;
            let strip = cylinder_strip(&arc, up)?;
            let (a0, a1, b0, b1) = (bottom[k], bottom[(k + 1) % 4], top[k], top[(k + 1) % 4]);
            builder.edge(a0, a1, ctrl, FRAC_1_SQRT_2);
            builder.edge(b0, b1, ctrl + up, FRAC_1_SQRT_2);
            builder.edge(a0, b1, strip[0].c[2], strip[0].w[2]);
            let face = builder.face(Face {
                name: FaceName {
                    feature,
                    part: FacePart::Side {
                        curve: 0,
                        segment: k as u32,
                    },
                },
                surface: wall,
            });
            builder.tri([a0, a1, b1], face);
            builder.tri([a0, b1, b0], face);
            builder.tri([centres[0], a1, a0], start);
            builder.tri([centres[1], b0, b1], end);
        }
        finish(builder, tol)
    }
}

/// Adds the start cap at height `z0`, facing `-z`, and the end cap at
/// `z1`, facing `+z`, of `feature`, returning their ids.
fn caps(builder: &mut MeshBuilder, feature: u64, z0: f64, z1: f64) -> [u32; 2] {
    let cap = |part, n: DVec3, z: f64| Face {
        name: FaceName { feature, part },
        surface: Surface::Plane { n, d: n.z * z },
    };
    [
        builder.face(cap(FacePart::StartCap, DVec3::NEG_Z, z0)),
        builder.face(cap(FacePart::EndCap, DVec3::Z, z1)),
    ]
}

fn finish(builder: MeshBuilder, tol: &Tolerance) -> Result<Mesh, KernelError> {
    let mesh = builder.build().expect("a primitive's triangles pair up");
    mesh.check(tol).map_err(KernelError::Invalid)?;
    Ok(mesh)
}

/// Refuses a point not within [`MAX_COORD`] of the origin (NaN included).
fn in_range(p: DVec3) -> Result<(), KernelError> {
    let m = p.abs().max_element();
    if p.is_finite() && m <= f64::from(MAX_COORD) {
        Ok(())
    } else {
        Err(PatchError::Coordinate(m).into())
    }
}

/// Refuses a size not above zero (NaN included).
fn positive(x: f64) -> Result<(), KernelError> {
    if x > 0.0 {
        Ok(())
    } else {
        Err(PatchError::Parameter(x).into())
    }
}

#[cfg(test)]
mod tests;
