use glam::Vec3;

/// An axis-aligned box, from its `min` corner to its `max` corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Radius of the sphere around [`Self::center`] that holds the box.
    pub fn radius(&self) -> f32 {
        (self.max - self.min).length() * 0.5
    }
}
