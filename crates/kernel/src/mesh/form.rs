//! What surface a face was meant to be.

use glam::{DMat3, DVec2, DVec3};

use crate::patch::{Conic2, Conic3};

/// What surface a face was built to lie on: the construction's intent,
/// with its parameters. [`Surface`](super::Surface) is the claim the
/// kernel cuts by, checked to the resolution; the form is what offsets,
/// fillets, measuring and picking read. A fitted face is on its form only
/// within the fit tolerance, and a copy a boolean makes claim-free keeps
/// it: it is intent, not a claim.
///
/// Constructors set it, booleans keep it, and a difference flips the
/// subtracted solid's with its tags. Only a plane's says which way the
/// face faces (its normal points out of the solid, as its tag's does); a
/// curved form is the same surface either way, and which side is out is
/// the patches' normals'. Debug builds check every triangle against its
/// face's form within the fit tolerance (see
/// [`Mesh::check`](super::Mesh::check)).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Form {
    /// No known surface.
    Unknown,
    /// The plane `n·x = d`, `n` a unit vector pointing out of the solid.
    Plane { n: DVec3, d: f64 },
    /// The circular cylinder of `radius` around the line through `point`
    /// along the unit `axis` (an extrude's walls: along the extrude).
    Cylinder {
        point: DVec3,
        axis: DVec3,
        radius: f64,
    },
    /// The cylinder over `conic` (not a circle: an ellipse, parabola or
    /// hyperbola arc) along the unit `along`.
    ConicCylinder { conic: Conic3, along: DVec3 },
    /// One nappe of the circular cone with its apex at `apex`: the points
    /// whose direction from the apex makes the half-angle with the unit
    /// `axis`, which points into the nappe. The half-angle is kept as its
    /// cosine and sine, both positive.
    Cone {
        apex: DVec3,
        axis: DVec3,
        cos: f64,
        sin: f64,
    },
    /// The sphere of `radius` around `centre`.
    Sphere { centre: DVec3, radius: f64 },
    /// The torus around the line through `centre` along the unit `axis`:
    /// the circle of radius `minor` whose centre is `major` from the axis,
    /// in a plane through it, turned about it.
    Torus {
        centre: DVec3,
        axis: DVec3,
        major: f64,
        minor: f64,
    },
    /// The conic `meridian` turned about the line through `origin` along
    /// the unit `axis`. The meridian is drawn in a half-plane through the
    /// axis: `x` is the distance from the axis, `y` the height along it
    /// from `origin`.
    Revolved {
        origin: DVec3,
        axis: DVec3,
        meridian: Conic2,
    },
}

impl Form {
    /// The plane `n·x = d` with `n` pointing out of the solid, any length:
    /// both are divided by `n`'s length. [`Form::Unknown`] for a normal
    /// that is zero or not finite.
    pub fn plane(n: DVec3, d: f64) -> Form {
        let length = n.length();
        if length > 0.0 && length.is_finite() && d.is_finite() {
            Form::Plane {
                n: n / length,
                d: d / length,
            }
        } else {
            Form::Unknown
        }
    }

    /// The form of the same surface facing the other way: a plane's normal
    /// turned round, every other form as it is.
    pub fn flipped(self) -> Form {
        match self {
            Form::Plane { n, d } => Form::Plane { n: -n, d: -d },
            form => form,
        }
    }

    /// About how far `x` is from the surface: exactly for planes,
    /// cylinders, cones, spheres and tori, to first order for the
    /// conic-based forms (as quadric tags are measured), 0 for
    /// [`Form::Unknown`]. NaN where the form isn't well defined.
    pub fn distance(&self, x: DVec3) -> f64 {
        match *self {
            Form::Unknown => 0.0,
            Form::Plane { n, d } => (n.dot(x) - d).abs(),
            Form::Cylinder {
                point,
                axis,
                radius,
            } => (polar(x - point, axis).x - radius).abs(),
            Form::ConicCylinder { conic, along } => conic_cylinder_distance(&conic, along, x),
            Form::Cone {
                apex,
                axis,
                cos,
                sin,
            } => {
                let DVec2 { x: r, y: h } = polar(x - apex, axis);
                if r * sin + h * cos >= 0.0 {
                    (r * cos - h * sin).abs()
                } else {
                    // Behind the apex: nearest to it.
                    hypot(r, h)
                }
            }
            Form::Sphere { centre, radius } => ((x - centre).length() - radius).abs(),
            Form::Torus {
                centre,
                axis,
                major,
                minor,
            } => {
                let DVec2 { x: r, y: h } = polar(x - centre, axis);
                (hypot(r - major, h) - minor).abs()
            }
            Form::Revolved {
                origin,
                axis,
                meridian,
            } => conic_distance(&meridian, polar(x - origin, axis)),
        }
    }

    /// The signed distance from `x` to the surface and the unit direction
    /// it grows along (the surface's normal near it), what fitted patches
    /// are fitted against: exact for planes, cylinders, cones (to the
    /// whole line of the half-angle, either side of the apex), spheres
    /// and tori, to first order for the conic forms. Which side is
    /// positive is the form's own (out of a sphere or a torus's tube,
    /// away from a cylinder's or cone's axis), not the solid's. `None`
    /// for [`Form::Unknown`], or where it isn't defined (on a cylinder's,
    /// cone's or torus's axis or tube circle, at a sphere's centre).
    pub(crate) fn signed(&self, x: DVec3) -> Option<(f64, DVec3)> {
        let unit = |v: DVec3| {
            let length = v.length();
            (length > 0.0 && length.is_finite()).then(|| v / length)
        };
        // `y` across `axis` and along it, with the unit direction across.
        let split = |y: DVec3, axis: DVec3| {
            let h = y.dot(axis);
            let across = y - axis * h;
            let r = across.length();
            (r, h, (r > 0.0).then(|| across / r))
        };
        let found = match *self {
            Form::Unknown => None,
            Form::Plane { n, d } => Some((n.dot(x) - d, n)),
            Form::Cylinder {
                point,
                axis,
                radius,
            } => {
                let (r, _, radial) = split(x - point, axis);
                radial.map(|radial| (r - radius, radial))
            }
            Form::ConicCylinder { conic, along } => {
                conic_cylinder_signed(&conic, along, x).and_then(|(d, g)| Some((d, unit(g)?)))
            }
            Form::Cone {
                apex,
                axis,
                cos,
                sin,
            } => {
                let (r, h, radial) = split(x - apex, axis);
                radial.map(|radial| (r * cos - h * sin, radial * cos - axis * sin))
            }
            Form::Sphere { centre, radius } => {
                let y = x - centre;
                unit(y).map(|n| (y.length() - radius, n))
            }
            Form::Torus {
                centre,
                axis,
                major,
                minor,
            } => {
                let (r, h, radial) = split(x - centre, axis);
                let tube = hypot(r - major, h);
                match radial {
                    Some(radial) if tube > 0.0 => {
                        Some((tube - minor, (radial * (r - major) + axis * h) / tube))
                    }
                    _ => None,
                }
            }
            Form::Revolved {
                origin,
                axis,
                meridian,
            } => {
                let (r, h, radial) = split(x - origin, axis);
                let (d, g) = conic_signed(&meridian, DVec2::new(r, h))?;
                radial.and_then(|radial| Some((d, unit(radial * g.x + axis * g.y)?)))
            }
        };
        found.filter(|(d, n)| d.is_finite() && n.is_finite())
    }
}

#[cfg(test)]
impl Form {
    /// The form moved by `f`, a rigid motion (turns, mirrors and moves),
    /// as tests move whole meshes. A plane's normal goes by the cofactor
    /// matrix, as the patches' normals do, so it still points out of a
    /// mirrored mesh whose triangles keep their corners' order (which
    /// turns it inside out, as it turns the patches).
    pub(crate) fn moved(self, f: impl Fn(DVec3) -> DVec3) -> Form {
        let origin = f(DVec3::ZERO);
        let turn = |d: DVec3| f(d) - origin;
        let unit = |d: DVec3| turn(d).normalize();
        let r = DMat3::from_cols(turn(DVec3::X), turn(DVec3::Y), turn(DVec3::Z));
        let cofactor = r.inverse().transpose() * r.determinant();
        match self {
            Form::Unknown => Form::Unknown,
            Form::Plane { n, d } => {
                let normal = cofactor * n;
                Form::plane(normal, normal.dot(f(n * d)))
            }
            Form::Cylinder {
                point,
                axis,
                radius,
            } => Form::Cylinder {
                point: f(point),
                axis: unit(axis),
                radius,
            },
            Form::ConicCylinder { conic, along } => Form::ConicCylinder {
                conic: Conic3 {
                    p0: f(conic.p0),
                    c: f(conic.c),
                    w: conic.w,
                    p1: f(conic.p1),
                },
                along: unit(along),
            },
            Form::Cone {
                apex,
                axis,
                cos,
                sin,
            } => Form::Cone {
                apex: f(apex),
                axis: unit(axis),
                cos,
                sin,
            },
            Form::Sphere { centre, radius } => Form::Sphere {
                centre: f(centre),
                radius,
            },
            Form::Torus {
                centre,
                axis,
                major,
                minor,
            } => Form::Torus {
                centre: f(centre),
                axis: unit(axis),
                major,
                minor,
            },
            Form::Revolved {
                origin,
                axis,
                meridian,
            } => Form::Revolved {
                origin: f(origin),
                axis: unit(axis),
                meridian,
            },
        }
    }
}

/// `√(a² + b²)` by `+ − × ÷ √` (std's `hypot` isn't the same on every
/// platform).
fn hypot(a: f64, b: f64) -> f64 {
    (a * a + b * b).sqrt()
}

/// `y` as its distance from the line through the origin along the unit
/// `axis` and its height along it.
fn polar(y: DVec3, axis: DVec3) -> DVec2 {
    let h = y.dot(axis);
    DVec2::new((y - axis * h).length(), h)
}

/// The circle `conic` is an arc of, as its centre and radius, if it is
/// one: its control point off its chord and equally far from its ends,
/// and its weight the cosine of half the arc's angle (half the chord over
/// that distance), each within `1e-10` of the arc's size plus a few
/// roundings of its coordinates. The centre is where the lines square to
/// the end tangents meet, `c + (m − c)·|c − p0|²/|c − m|²` with `m` the
/// chord's middle (the triangle centre, end, control point is
/// right-angled at the end), and the radius the mean of the distances to
/// the ends. Only `+ − × ÷ √`.
///
/// It names an arc's intent (an extrude's wall is a [`Form::Cylinder`]
/// over one); no topology depends on it.
pub(crate) fn circle_of(conic: &Conic2) -> Option<(DVec2, f64)> {
    const SLACK: f64 = 1e-10;
    let (p0, c, p1) = (conic.p0, conic.c, conic.p1);
    let (l0, l1) = ((c - p0).length(), (c - p1).length());
    let half = (p1 - p0).length() * 0.5;
    let m = (p0 + p1) * 0.5;
    let rise = (c - m).length();
    let leg = (l0 + l1) * 0.5;
    let coords = p0.abs().max(c.abs()).max(p1.abs()).max_element();
    let slack = SLACK * leg + 64.0 * f64::EPSILON * coords;
    if !(rise > slack && (l0 - l1).abs() <= slack && (conic.w * leg - half).abs() <= slack) {
        return None;
    }
    let centre = c + (m - c) * (leg * leg / (rise * rise));
    let radius = ((centre - p0).length() + (centre - p1).length()) * 0.5;
    (centre.is_finite() && radius.is_finite()).then_some((centre, radius))
}

/// `F = λ1² − 4w²·λ0·λ2` of a conic of weight `w` at barycentric
/// coordinates `λ` on its control triangle (zero on the conic), and the
/// factors `(2λ1, −4w²·λ2, −4w²·λ0)` its gradient takes the gradients of
/// `λ1`, `λ0` and `λ2` by.
fn implicit(w: f64, [l0, l1, l2]: [f64; 3]) -> (f64, [f64; 3]) {
    let w2 = 4.0 * w * w;
    (l1 * l1 - w2 * l0 * l2, [2.0 * l1, -w2 * l2, -w2 * l0])
}

/// `|F| / |∇F|`, NaN as infinite.
fn first_order(f: f64, gradient: f64) -> f64 {
    if f == 0.0 {
        return 0.0;
    }
    let d = f.abs() / gradient;
    if d.is_nan() { f64::INFINITY } else { d }
}

/// The distance from `q` to the conic, to first order.
fn conic_distance(conic: &Conic2, q: DVec2) -> f64 {
    let (f, gradient) = conic_implicit(conic, q);
    first_order(f, gradient.length())
}

/// `F` of the conic at `q` (see [`implicit`]) and its gradient.
fn conic_implicit(conic: &Conic2, q: DVec2) -> (f64, DVec2) {
    let (e1, e2) = (conic.c - conic.p0, conic.p1 - conic.p0);
    let det = e1.perp_dot(e2);
    let g1 = DVec2::new(e2.y, -e2.x) / det;
    let g2 = DVec2::new(-e1.y, e1.x) / det;
    let d = q - conic.p0;
    let (l1, l2) = (g1.dot(d), g2.dot(d));
    let (f, [k1, k0, k2]) = implicit(conic.w, [1.0 - l1 - l2, l1, l2]);
    (f, g1 * k1 + (-g1 - g2) * k0 + g2 * k2)
}

/// `F/|∇F|` at `q` and `∇F`: the signed distance to the conic to first
/// order, positive on the side its control point is on (outside a
/// circle), and the direction it grows along. `None` where the gradient
/// vanishes.
fn conic_signed(conic: &Conic2, q: DVec2) -> Option<(f64, DVec2)> {
    let (f, gradient) = conic_implicit(conic, q);
    let length = gradient.length();
    (length > 0.0).then(|| (f / length, gradient))
}

/// The distance from `x` to the cylinder over `conic` along `along`, to
/// first order: the conic's barycentric coordinates are affine in the
/// point and don't change along `along`.
fn conic_cylinder_distance(conic: &Conic3, along: DVec3, x: DVec3) -> f64 {
    let (f, gradient) = conic_cylinder_implicit(conic, along, x);
    first_order(f, gradient.length())
}

/// `F` of the conic at `x`'s place on it along `along`, and its gradient.
fn conic_cylinder_implicit(conic: &Conic3, along: DVec3, x: DVec3) -> (f64, DVec3) {
    let (e1, e2) = (conic.c - conic.p0, conic.p1 - conic.p0);
    // The rows of the inverse of [e1 e2 along] give each coordinate.
    let inverse = DMat3::from_cols(e1, e2, along).inverse().transpose();
    let (g1, g2) = (inverse.x_axis, inverse.y_axis);
    let d = x - conic.p0;
    let (l1, l2) = (g1.dot(d), g2.dot(d));
    let (f, [k1, k0, k2]) = implicit(conic.w, [1.0 - l1 - l2, l1, l2]);
    (f, g1 * k1 + (-g1 - g2) * k0 + g2 * k2)
}

/// [`conic_signed`] for the cylinder over `conic` along `along`.
fn conic_cylinder_signed(conic: &Conic3, along: DVec3, x: DVec3) -> Option<(f64, DVec3)> {
    let (f, gradient) = conic_cylinder_implicit(conic, along, x);
    let length = gradient.length();
    (length > 0.0).then(|| (f / length, gradient))
}

#[cfg(test)]
mod tests;
