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

    /// The box around `points`, or `None` if there are none.
    pub(crate) fn around(points: &[[f32; 3]]) -> Option<Aabb> {
        let mut points = points.iter().map(|p| Vec3::from(*p));
        let first = points.next()?;
        let (min, max) = points.fold((first, first), |(min, max), p| (min.min(p), max.max(p)));
        Some(Aabb { min, max })
    }
}
