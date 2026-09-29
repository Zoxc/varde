use glam::Vec3;

use crate::{Aabb, RenderMesh, Shape};

/// A closed solid, as the kernel works with it: built from a [`Shape`] by
/// [`Shape::build`], and never stored.
///
/// For now it only holds its shape, which it tessellates analytically.
/// Booleans, fillets, etc. will need a proper kernel backend behind it.
#[derive(Debug, Clone, PartialEq)]
pub struct Solid {
    shape: Shape,
}

impl Solid {
    pub(crate) fn new(shape: Shape) -> Self {
        Solid { shape }
    }

    /// The solid's axis-aligned bounds in its own coordinates.
    pub fn bounds(&self) -> Aabb {
        match self.shape {
            Shape::Cuboid { size } => Aabb {
                min: Vec3::ZERO,
                max: size,
            },
        }
    }

    pub fn tessellate(&self) -> RenderMesh {
        match self.shape {
            Shape::Cuboid { size } => cuboid(size),
        }
    }
}

fn cuboid(size: Vec3) -> RenderMesh {
    let v = |x: f32, y: f32, z: f32| Vec3::new(x, y, z) * size;
    let mut mesh = RenderMesh::default();

    // Each face listed counter-clockwise when viewed from outside.
    mesh.push_quad([v(0., 0., 0.), v(0., 1., 0.), v(1., 1., 0.), v(1., 0., 0.)]); // -Z
    mesh.push_quad([v(0., 0., 1.), v(1., 0., 1.), v(1., 1., 1.), v(0., 1., 1.)]); // +Z
    mesh.push_quad([v(0., 0., 0.), v(1., 0., 0.), v(1., 0., 1.), v(0., 0., 1.)]); // -Y
    mesh.push_quad([v(0., 1., 0.), v(0., 1., 1.), v(1., 1., 1.), v(1., 1., 0.)]); // +Y
    mesh.push_quad([v(0., 0., 0.), v(0., 0., 1.), v(0., 1., 1.), v(0., 1., 0.)]); // -X
    mesh.push_quad([v(1., 0., 0.), v(1., 1., 0.), v(1., 1., 1.), v(1., 0., 1.)]); // +X

    mesh
}

#[cfg(test)]
mod tests;
