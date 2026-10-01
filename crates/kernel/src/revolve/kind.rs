//! What face each profile segment turns into.

use glam::DVec2;

use super::Turn;
use crate::extrude::chain::Side;
use crate::mesh::{Form, Quadric, Surface, circle_of, hypot};

/// The face an input segment turns into, with its tag and form.
#[derive(Debug, Clone, Copy)]
pub(super) enum Kind {
    /// Along the axis: no face.
    Axis,
    /// Square to the axis: a flat ring, disc or sector at `height` along
    /// the axis, its plane facing out.
    Flat {
        surface: Surface,
        form: Form,
        height: f64,
    },
    /// A cone (or cylinder) for a straight segment, a sphere for an arc
    /// centred on the axis: exact strips, fitted caps at the axis.
    Exact {
        surface: Surface,
        form: Form,
        straight: bool,
    },
    /// Anything else (a torus for an arc off the axis, another conic
    /// turned about it): fitted, claiming no surface.
    Fitted { form: Form },
}

impl Kind {
    /// The kind of `side` (its conic in the profile's `(x, y)`, `x` from
    /// the axis) on `turn`.
    ///
    /// Straight segments whose ends are within `margin` of one height
    /// are flat (their faces then within it of their plane, which `check`
    /// holds them to); those whose ends are within `margin` of one radius
    /// have a cylinder for their form. A circle's arc of weight `w` whose
    /// centre is within `margin·w/4` of the axis is a sphere's (as an
    /// extrude takes a conic whose control point is within the resolution
    /// of its chord for a line): its exact strips are then within about
    /// `δ·(1 + 1/2w)` of the sphere for a centre `δ` off the axis
    /// (measured; the diagonal of a wide arc's strip strays furthest), so
    /// within `3/8` of the resolution, which the tag check holds them to.
    /// Only the face's kind hangs on these; its tag is checked.
    pub(super) fn of(side: &Side, turn: &Turn, margin: f64) -> Kind {
        let c = &side.conic;
        let (axis, origin) = (turn.axis(), turn.origin());
        let on_axis = |h: f64| turn.place(DVec2::new(0.0, h));
        if !side.curved {
            let (p0, p1) = (c.p0, c.p1);
            if p0.x == 0.0 && p1.x == 0.0 {
                return Kind::Axis;
            }
            let (dr, dh) = (p1.x - p0.x, p1.y - p0.y);
            if dh.abs() <= margin {
                // The region is on the segment's left: above it when it
                // runs out from the axis.
                let n = if dr > 0.0 { -axis } else { axis };
                let d = n.dot((turn.place(p0) + turn.place(p1)) * 0.5);
                return Kind::Flat {
                    surface: Surface::Plane { n, d },
                    form: Form::plane(n, d),
                    height: 0.5 * (p0.y + p1.y),
                };
            }
            let form = if dr.abs() <= margin {
                Form::Cylinder {
                    point: origin,
                    axis,
                    radius: 0.5 * (p0.x + p1.x),
                }
            } else {
                Form::Cone {
                    apex: on_axis(apex(p0, p1)),
                    axis: if 0.5 * (p0.y + p1.y) > apex(p0, p1) {
                        axis
                    } else {
                        -axis
                    },
                    cos: dh.abs() / hypot(dr, dh),
                    sin: dr.abs() / hypot(dr, dh),
                }
            };
            // Written about a point near the segment: about the apex where
            // the cone is wide (the apex is then near), about the
            // segment's foot where it is steep (`ρ² = (ρ0 + s·h)²` from
            // there, `s` the slope, well conditioned up to a cylinder).
            let quadric = if dr.abs() >= dh.abs() {
                let length = hypot(dr, dh);
                Quadric::cone(
                    on_axis(apex(p0, p1)),
                    axis,
                    dh.abs() / length,
                    dr.abs() / length,
                )
            } else {
                let s = dr / dh;
                Quadric::revolution(on_axis(p0.y), axis, p0.x * p0.x, 2.0 * p0.x * s, s * s)
            };
            return Kind::Exact {
                surface: quadric.map_or(Surface::Free, Surface::Quadric),
                form,
                straight: true,
            };
        }
        match circle_of(c) {
            Some((centre, radius)) if centre.x.abs() <= 0.25 * margin * c.w => {
                let centre = on_axis(centre.y);
                Kind::Exact {
                    surface: Surface::Quadric(Quadric::sphere(centre, radius)),
                    form: Form::Sphere { centre, radius },
                    straight: false,
                }
            }
            Some((centre, radius)) if centre.x > 0.0 => Kind::Fitted {
                form: Form::Torus {
                    centre: on_axis(centre.y),
                    axis,
                    major: centre.x,
                    minor: radius,
                },
            },
            // A circle whose centre is across the axis, or another conic.
            _ => Kind::Fitted {
                form: Form::Revolved {
                    origin,
                    axis,
                    meridian: *c,
                },
            },
        }
    }

    /// What the face claims: its plane or quadric where exact.
    pub(super) fn surface(&self) -> Surface {
        match *self {
            Kind::Flat { surface, .. } | Kind::Exact { surface, .. } => surface,
            Kind::Axis | Kind::Fitted { .. } => Surface::Free,
        }
    }

    /// What surface the face is meant to be.
    pub(super) fn form(&self) -> Form {
        match *self {
            Kind::Flat { form, .. } | Kind::Exact { form, .. } | Kind::Fitted { form } => form,
            Kind::Axis => Form::Unknown,
        }
    }
}

/// The height along the axis where the line through `p0` and `p1` (at
/// different radii) meets it.
fn apex(p0: DVec2, p1: DVec2) -> f64 {
    (p0.y * p1.x - p1.y * p0.x) / (p1.x - p0.x)
}
