use glam::{DMat3, DVec3};

use super::Form;

/// A face of a solid: the triangles that came from one surface of one
/// feature, with the surface they lie on (a claim, checked) and the one
/// they were meant to (their [`Form`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Face {
    pub name: FaceName,
    pub surface: Surface,
    pub form: Form,
}

/// A face's stable name: the feature that made it, which part of that
/// feature it is, and which copy. It stays the same when the history is
/// evaluated again, whatever the dimensions, the tolerance or the
/// triangulation, since it is made only from what the feature was given
/// (curve ids, references): never from mesh indices or positions. Faces
/// are drawn, picked and referred to by their [`FaceKey`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FaceName {
    pub feature: u64,
    pub part: FacePart,
    /// 0 for a face that isn't a copy; a copy's is
    /// [`mix`](crate::topology::mix)`(parent's instance, feature, index)`
    /// ([`FaceName::copy`]), so copies of copies stay unique and stable.
    pub instance: u64,
}

impl FaceName {
    /// The name of `part` of `feature`, not a copy.
    pub fn new(feature: u64, part: FacePart) -> FaceName {
        FaceName {
            feature,
            part,
            instance: 0,
        }
    }

    /// The name of copy `index` that `feature` (a pattern or mirror) makes
    /// of a face so named.
    pub fn copy(self, feature: u64, index: u64) -> FaceName {
        FaceName {
            instance: crate::topology::mix(&[self.instance, feature, index]),
            ..self
        }
    }

    /// The key of the face users see and pick that this one is a piece
    /// of: see [`FaceKey`].
    pub fn key(self) -> FaceKey {
        let part = match self.part {
            FacePart::StartCap => PartKey::StartCap,
            FacePart::EndCap => PartKey::EndCap,
            FacePart::Side { curve, .. } => PartKey::Side { curve },
            FacePart::Split(n) => PartKey::Split(n),
            FacePart::Blend { edge, .. } => PartKey::Blend { edge },
            FacePart::Corner { vertex } => PartKey::Corner { vertex },
            FacePart::Offset { of } => PartKey::Offset { of },
            FacePart::BackSide { curve, .. } => PartKey::BackSide { curve },
            FacePart::Swept { curve, .. } => PartKey::Swept { curve },
            FacePart::Lofted { curve, span, .. } => PartKey::Lofted { curve, span },
        };
        FaceKey {
            feature: self.feature,
            part,
            instance: self.instance,
        }
    }
}

/// Which part of a feature a face is. Ids (`curve`, `edge`, `vertex`,
/// `of`) are the sketch curve's id or a [`mix`](crate::topology::mix) of
/// the keys the part came from; `segment`, `piece` and `span` number the
/// pieces of one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FacePart {
    /// The cap where an extrusion starts.
    StartCap,
    /// The cap where an extrusion ends.
    EndCap,
    /// The wall swept by segment `segment` of profile curve `curve`, by an
    /// extrude or a revolve.
    Side { curve: u64, segment: u32 },
    /// A face an operation made, numbered in the order it made them (a
    /// split's cut face is 0).
    Split(u32),
    /// A chamfer's or fillet's face along one chain: `edge` mixes the edge
    /// reference's two face keys and its ordinal among the feature's
    /// references with that pair ([`blend_edge`](crate::topology::blend_edge)).
    Blend { edge: u64, segment: u32 },
    /// A blend at a vertex.
    Corner { vertex: u64 },
    /// A shell's inner face, offset from the face whose key mixes to `of`.
    Offset { of: u64 },
    /// A two-sided tapered extrude's wall behind the sketch: its own face,
    /// meeting the front wall at a crease.
    BackSide { curve: u64, segment: u32 },
    /// A sweep's wall: profile curve, path piece.
    Swept {
        curve: u64,
        piece: u32,
        segment: u32,
    },
    /// A loft's wall: the first section's curve, the pair of sections.
    Lofted { curve: u64, span: u32, segment: u32 },
}

/// The face users see, pick and refer to: a [`FaceName`] without what
/// numbers the pieces of one surface (`segment`, and a sweep's `piece`),
/// so a circle's four quarter walls, a blend's pieces and a sweep's pieces
/// along a smooth path are one face. Stored in documents by references,
/// so its fields and their order are fixed; see
/// [`Topology`](crate::topology::Topology) for resolving one.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct FaceKey {
    pub feature: u64,
    pub part: PartKey,
    pub instance: u64,
}

impl FaceKey {
    /// The key as one number, for names derived from keys (a blend's
    /// edge, a shell's offset faces): the [`mix`](crate::topology::mix) of
    /// the feature, the part's number (its place in [`PartKey`], from 0)
    /// and its two fields (0 where it has fewer), and the instance.
    pub fn mixed(&self) -> u64 {
        let (tag, a, b) = match self.part {
            PartKey::StartCap => (0, 0, 0),
            PartKey::EndCap => (1, 0, 0),
            PartKey::Side { curve } => (2, curve, 0),
            PartKey::Split(n) => (3, u64::from(n), 0),
            PartKey::Blend { edge } => (4, edge, 0),
            PartKey::Corner { vertex } => (5, vertex, 0),
            PartKey::Offset { of } => (6, of, 0),
            PartKey::BackSide { curve } => (7, curve, 0),
            PartKey::Swept { curve } => (8, curve, 0),
            PartKey::Lofted { curve, span } => (9, curve, u64::from(span)),
        };
        crate::topology::mix(&[self.feature, tag, a, b, self.instance])
    }
}

/// A [`FacePart`] without the numbers of its pieces: see [`FaceKey`]. A
/// ruled loft's spans meet at creases, so `span` stays.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum PartKey {
    StartCap,
    EndCap,
    Side { curve: u64 },
    Split(u32),
    Blend { edge: u64 },
    Corner { vertex: u64 },
    Offset { of: u64 },
    BackSide { curve: u64 },
    Swept { curve: u64 },
    Lofted { curve: u64, span: u32 },
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
        let a = DMat3::IDENTITY - outer(axis, axis);
        Some(Quadric {
            origin: point,
            a,
            b: DVec3::ZERO,
            c: -radius * radius,
        })
    }

    /// The sphere of `radius` around `centre`, written around its centre.
    pub fn sphere(centre: DVec3, radius: f64) -> Quadric {
        Quadric {
            origin: centre,
            a: DMat3::IDENTITY,
            b: DVec3::ZERO,
            c: -radius * radius,
        }
    }

    /// The circular cone (both nappes) with its apex at `apex`, around the
    /// line along `axis`, of the half-angle whose cosine and sine are
    /// `cos` and `sin`: `cos²·|y|² − (y·axis)² = 0` for `y` from the apex
    /// (`cos² + sin² = 1` folds the sine in). `None` for a zero or
    /// non-finite axis.
    pub fn cone(apex: DVec3, axis: DVec3, cos: f64, sin: f64) -> Option<Quadric> {
        let axis = axis.try_normalize()?;
        // Normalized here, so a half-angle given a little off unit still
        // describes one cone.
        let length = (cos * cos + sin * sin).sqrt();
        let cos = cos / length;
        Some(Quadric {
            origin: apex,
            a: DMat3::IDENTITY * (cos * cos) - outer(axis, axis),
            b: DVec3::ZERO,
            c: 0.0,
        })
    }

    /// The quadric of revolution about the line through `origin` along
    /// `axis` whose points at height `h` along the axis (from `origin`) lie
    /// `ρ` from it with `ρ² = r0 + r1·h + r2·h²`: a cylinder (`r1 = r2 =
    /// 0`), a sphere around `origin` (`r2 = −1`), a cone, an ellipsoid, a
    /// paraboloid or a hyperboloid of revolution. `None` for a zero or
    /// non-finite axis.
    pub fn revolution(origin: DVec3, axis: DVec3, r0: f64, r1: f64, r2: f64) -> Option<Quadric> {
        let axis = axis.try_normalize()?;
        // |y|² − (y·axis)² − r0 − r1·(y·axis) − r2·(y·axis)²
        Some(Quadric {
            origin,
            a: DMat3::IDENTITY - outer(axis, axis) * (1.0 + r2),
            b: axis * (-0.5 * r1),
            c: -r0,
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

/// `u·vᵀ`.
fn outer(u: DVec3, v: DVec3) -> DMat3 {
    DMat3::from_cols(u * v.x, u * v.y, u * v.z)
}
