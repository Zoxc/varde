//! Boxes and cylinders as patch meshes, built the way an extrude builds
//! its solids: a start cap, an end cap and a wall per profile segment,
//! named and tagged accordingly.

use std::f64::consts::FRAC_1_SQRT_2;

use glam::DVec3;

use super::{Face, FaceName, FacePart, Form, Mesh, MeshBuilder, Quadric, Surface};
use crate::patch::{Conic3, PatchError};
use crate::{KernelError, Tolerance, in_range};

impl Mesh {
    /// The axis-aligned box from `min` to `min + size`, as the extrusion
    /// of its bottom rectangle along `+z`: two flat triangles to a side.
    /// The bottom is the [`FacePart::StartCap`] and the top the
    /// [`FacePart::EndCap`] of `feature`; the sides are
    /// [`FacePart::Side`]s of curves 0 to 3, the rectangle's edges counter-
    /// clockwise seen from `+z` from the one along `-y`, segment 0. Every
    /// face is tagged with its plane, and has it as its [`Form`].
    ///
    /// Every corner must be within [`MAX_COORD`](crate::MAX_COORD) of the
    /// origin and every size above zero, and the box must pass
    /// [`Mesh::check`] with `tol` (a box much thinner than the resolution
    /// doesn't).
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
                name: FaceName::new(
                    feature,
                    FacePart::Side {
                        curve: k as u64,
                        segment: 0,
                    },
                ),
                surface: Surface::Plane {
                    n,
                    d: n.dot(rect[k][0]),
                },
                form: Form::plane(n, n.dot(rect[k][0])),
                slack: 1.0,
            });
            let (a, b) = (k, (k + 1) % 4);
            builder.wall([bottom[a], bottom[b]], [top[a], top[b]], face);
        }
        finish(builder, tol)
    }

    /// The circular cylinder of `radius` standing on `base`, the centre of
    /// its bottom, and rising `height` along `+z`, as the extrusion of a
    /// circle of four exact quarter arcs: each wall an exact
    /// [`cylinder_strip`](crate::patch::cylinder_strip), each cap four
    /// quarter discs around its centre.
    /// The bottom is the [`FacePart::StartCap`] and the top the
    /// [`FacePart::EndCap`] of `feature`, tagged with their planes; the
    /// walls are [`FacePart::Side`]s of curve 0, segments 0 to 3 counter-
    /// clockwise seen from `+z` from `+x`, tagged with the cylinder. Forms
    /// as the tags, the walls' [`Form::Cylinder`] along `+z`.
    ///
    /// Every point of it must be within [`MAX_COORD`](crate::MAX_COORD) of
    /// the origin, the radius and height above zero, and the cylinder must
    /// pass [`Mesh::check`] with `tol`.
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
            let face = builder.face(Face {
                name: FaceName::new(
                    feature,
                    FacePart::Side {
                        curve: 0,
                        segment: k as u32,
                    },
                ),
                surface: wall,
                form: Form::Cylinder {
                    point: base,
                    axis: DVec3::Z,
                    radius,
                },
                slack: 1.0,
            });
            let next = (k + 1) % 4;
            let (a, b) = ([bottom[k], bottom[next]], [top[k], top[next]]);
            builder.curved_wall(a, b, &arc, up, face)?;
            builder.tri([centres[0], a[1], a[0]], start);
            builder.tri([centres[1], b[0], b[1]], end);
        }
        finish(builder, tol)
    }
}

/// Adds the start cap at height `z0`, facing `-z`, and the end cap at
/// `z1`, facing `+z`, of `feature`, returning their ids.
fn caps(builder: &mut MeshBuilder, feature: u64, z0: f64, z1: f64) -> [u32; 2] {
    let cap = |part, n: DVec3, z: f64| Face {
        name: FaceName::new(feature, part),
        surface: Surface::Plane { n, d: n.z * z },
        form: Form::plane(n, n.z * z),
        slack: 1.0,
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
