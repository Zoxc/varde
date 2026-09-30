use glam::{DMat3, DVec3};

/// A face of a solid: the triangles that came from one surface of one
/// feature, with the surface they lie on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Face {
    pub name: FaceName,
    pub surface: Surface,
}

/// A face's stable name: the feature that made it and which part of that
/// feature it is. It stays the same when the history is evaluated again,
/// for feature edges and picking now and for naming faces later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FaceName {
    pub feature: u64,
    pub part: FacePart,
}

/// Which part of a feature a face is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FacePart {
    /// The cap where an extrusion starts.
    StartCap,
    /// The cap where an extrusion ends.
    EndCap,
    /// The wall swept by segment `segment` of profile curve `curve`.
    Side { curve: u64, segment: u32 },
    /// A face an operation made, numbered in the order it made them.
    Split(u32),
}

/// The surface a face lies on, as its construction claims. It is a claim
/// the kernel may use to cut exactly, checked by
/// [`Mesh::check_faces`](super::Mesh::check_faces) as part of every
/// [`Mesh::check`](super::Mesh::check) (so every `Solid`'s tags hold), and
/// by repair before it splits a patch as planar; never a second copy of
/// the geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Surface {
    /// The points `x` with `n·x = d`; `n` need not be a unit vector.
    Plane {
        n: DVec3,
        d: f64,
    },
    Quadric(Quadric),
    /// No claim.
    Free,
}

impl Surface {
    /// About how far `x` is from the surface: exact for a plane, to first
    /// order for a quadric, 0 for [`Surface::Free`]. Infinite, or NaN, for
    /// a surface that isn't well defined (a zero or non-finite normal).
    ///
    /// A plane's `n` and `d` are first divided by `n`'s largest
    /// coordinate, so a normal of any finite size measures the same: its
    /// length could otherwise overflow, making every distance 0, or its
    /// square underflow, making every distance infinite.
    pub fn distance(&self, x: DVec3) -> f64 {
        match self {
            Surface::Plane { n, d } => {
                let scale = n.abs().max_element();
                if !(scale > 0.0 && scale.is_finite()) {
                    return f64::INFINITY;
                }
                let (n, d) = (*n / scale, d / scale);
                (n.dot(x) - d).abs() / n.length()
            }
            Surface::Quadric(q) => q.distance(x),
            Surface::Free => 0.0,
        }
    }
}

/// The quadric `F(x) = 0`, with `F(x) = y·(a·y) + 2·b·y + c` for `y = x -
/// origin`. Measuring from an `origin` near the surface keeps the rounding
/// of `F` relative to the quadric's size rather than to its distance from
/// the model's origin. `a` need not be symmetric: only its symmetric part
/// counts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quadric {
    pub origin: DVec3,
    pub a: DMat3,
    pub b: DVec3,
    pub c: f64,
}

impl Quadric {
    /// The circular cylinder of `radius` around the line through `point`
    /// along `axis`, or `None` for a zero or non-finite axis.
    pub fn cylinder(point: DVec3, axis: DVec3, radius: f64) -> Option<Quadric> {
        let axis = axis.try_normalize()?;
        // |y|² - (y·axis)² - r²
        let a = DMat3::IDENTITY - DMat3::from_cols(axis * axis.x, axis * axis.y, axis * axis.z);
        Some(Quadric {
            origin: point,
            a,
            b: DVec3::ZERO,
            c: -radius * radius,
        })
    }

    /// `F(x)`.
    pub fn value(&self, x: DVec3) -> f64 {
        let y = x - self.origin;
        y.dot(self.a * y) + 2.0 * self.b.dot(y) + self.c
    }

    /// The gradient of `F` at `x`.
    pub fn gradient(&self, x: DVec3) -> DVec3 {
        let y = x - self.origin;
        self.a * y + self.a.transpose() * y + 2.0 * self.b
    }

    /// `|F(x)| / |∇F(x)|`: the distance from `x` to the quadric to first
    /// order. Infinite where the gradient vanishes off the surface, or
    /// where it overflows (a finite value over an infinite gradient would
    /// otherwise put `x` on the surface), and NaN where `F` does.
    pub fn distance(&self, x: DVec3) -> f64 {
        let f = self.value(x).abs();
        if f == 0.0 {
            return 0.0;
        }
        let gradient = self.gradient(x).length();
        if !gradient.is_finite() {
            return f64::INFINITY;
        }
        f / gradient
    }
}
