//! Points and directions picked on a solid's topology, by the same names
//! and points as faces, edges and corners ([`Topology::face`],
//! [`Topology::edge`], [`Topology::corner`]): what align moves a body by,
//! and what other features take as centres, axes and planes.
//!
//! - **Points**: a corner (its vertex), the middle of a straight edge
//!   (the mean of its ends), the centre of a round one (a circle's or an
//!   ellipse's, from its conics, trig-free: [`edge_shape`]).
//! - **Directions**: a flat face's normal (out of the solid), a round
//!   face's axis (as its form has it: an extrude's walls along the
//!   extrude, a revolve's along its axis line), a straight edge's
//!   direction, a round edge's axis.
//!
//! Normals and axes come from the faces' forms, the intent rather than
//! the fitted patches, so they are exact where the forms are: a face
//! square to a world axis gives that axis to the bit. Edges come from
//! their curves ([`edge_shape`], the same as measure's and the picking
//! tables' snap points, so the point shown is the point used).
//!
//! **Signs.** A straight edge runs with the face of its first key on its
//! left seen from outside ([`runs_with`]: the faces' own boundaries run
//! round them that way), as a revolve's axis edge and a move's turn axis
//! run. A round edge's axis is the outward normal of a flat face beside
//! it, else the axis of a round face beside it that runs along it (the
//! first key's face first in each case), else the circle's own axis
//! turning the way the edge runs: so a hole's rim and a pin's rim point
//! out of their flat faces, and "face to face" puts a pin in a hole.
//! Where the geometry has no sign these are arbitrary but follow the
//! keys, so they survive edits; the features that use them have a flip.

use glam::DVec3;

use super::{Chain, NotFound, Topology};
use crate::Solid;
use crate::measure::{EdgeShape, edge_shape};
use crate::mesh::{FaceKey, Form};

/// How near parallel, as the sine of the angle between them, a round
/// face's axis must be to a round edge's circle's axis for the edge's
/// axis to take the face's sign and value: it tells a rim round a
/// cylinder from a torus's meridian circle, which are square or far from
/// it, so it only names what the edge is.
const ALONG: f64 = 1e-6;

/// Why a point or a direction isn't there: its face, edge or corner
/// isn't found, or isn't the kind it must be. The region or chain found
/// comes with it, for showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unresolved {
    NotFound(NotFound),
    /// A face whose normal is asked isn't flat.
    NotFlat {
        region: u32,
    },
    /// A face whose axis is asked isn't round: a cylinder, cone, torus
    /// or other surface of revolution.
    NotRound {
        region: u32,
    },
    /// An edge whose middle is asked isn't straight.
    NotStraight {
        chain: u32,
    },
    /// An edge whose centre is asked isn't a circle or an ellipse.
    NotCircular {
        chain: u32,
    },
    /// An edge whose direction is asked is neither straight nor round.
    NotAnAxis {
        chain: u32,
    },
    /// Aliases name both faces of an edge by both keys, so which is the
    /// first key's, and so which way the edge runs, can't be told.
    Undirected {
        chain: u32,
    },
}

impl From<NotFound> for Unresolved {
    fn from(not_found: NotFound) -> Unresolved {
        Unresolved::NotFound(not_found)
    }
}

impl std::fmt::Display for Unresolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unresolved::NotFound(not_found) => not_found.fmt(f),
            Unresolved::NotFlat { .. } => f.write_str("isn't flat"),
            Unresolved::NotRound { .. } | Unresolved::NotCircular { .. } => {
                f.write_str("isn't round")
            }
            Unresolved::NotStraight { .. } => f.write_str("isn't straight"),
            Unresolved::NotAnAxis { .. } => f.write_str("isn't straight or round"),
            Unresolved::Undirected { .. } => f.write_str("its direction can't be told"),
        }
    }
}

impl std::error::Error for Unresolved {}

/// Whether an edge between faces of the keys `faces` (a reference's, the
/// first and second as stored) runs the way it does with the face `left`
/// on its left seen from outside (as a face's own boundary runs: the
/// halfedges on its triangles), `right` the face on its other side, each
/// given by its key and its aliases, sorted: `Some(true)` if `left` is
/// the first key's face, `Some(false)` if `right` is. A face is the
/// first key's if that key names it (its key or an alias) and the second
/// key names the other. Where that holds both ways round (aliases naming
/// each face by both keys), the face whose own key is the first, or
/// whose other's own key is the second, is; `None` if that doesn't tell,
/// or the keys name the faces neither way.
pub fn runs_with(
    faces: &[FaceKey; 2],
    left: (&FaceKey, &[FaceKey]),
    right: (&FaceKey, &[FaceKey]),
) -> Option<bool> {
    let [a, b] = faces;
    let named = |(key, aliases): (&FaceKey, &[FaceKey]), by: &FaceKey| {
        key == by || aliases.binary_search(by).is_ok()
    };
    let along = named(left, a) && named(right, b);
    let against = named(right, a) && named(left, b);
    match (along, against) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        (false, false) => None,
        (true, true) => {
            let along = left.0 == a || right.0 == b;
            let against = right.0 == a || left.0 == b;
            (along != against).then_some(along)
        }
    }
}

impl Topology {
    /// The form of region `region` of `solid` (the one the topology was
    /// made from): a region's faces lie on one surface, so its first
    /// triangle's face's form stands for them.
    pub fn form<'a>(&self, solid: &'a Solid, region: u32) -> &'a Form {
        let mesh = solid.mesh();
        let tri = &mesh.tris()[self.regions[region as usize].tris[0] as usize];
        &mesh.faces()[tri.face as usize].form
    }

    /// Whether chain `chain` runs the way a reference naming it by
    /// `faces` (first and second as stored) directs it: see
    /// [`runs_with`]. Its halfedges run on its first region's side, along
    /// that region's own boundary (anticlockwise seen from outside, a
    /// mirrored copy's too, as a mirror reverses its triangles), so they
    /// run the right way where that region is the first key's.
    pub fn runs_with(&self, chain: u32, faces: &[FaceKey; 2]) -> Option<bool> {
        let [left, right] = self.chains[chain as usize].regions.map(|r| {
            let region = &self.regions[r as usize];
            (&region.key, &region.aliases[..])
        });
        runs_with(faces, left, right)
    }

    /// The corner named by `faces`, nearest `near` ([`Topology::corner`]):
    /// its vertex.
    pub fn corner_point(
        &self,
        solid: &Solid,
        faces: [FaceKey; 3],
        near: DVec3,
    ) -> Result<DVec3, Unresolved> {
        let corner = self.corner(solid, faces, near)?;
        Ok(solid.mesh().verts()[self.corners[corner as usize].vertex as usize])
    }

    /// The middle of the straight edge named by `faces`, nearest `near`
    /// ([`Topology::edge`]): the mean of its ends.
    pub fn middle(
        &self,
        solid: &Solid,
        faces: [FaceKey; 2],
        near: DVec3,
    ) -> Result<DVec3, Unresolved> {
        let (chain, shape) = self.edge_and_shape(solid, faces, near)?;
        match shape {
            EdgeShape::Line { from, to } => Ok((from + to) * 0.5),
            _ => Err(Unresolved::NotStraight { chain }),
        }
    }

    /// The centre of the round edge (a circle or an ellipse, whole or an
    /// arc) named by `faces`, nearest `near`, from its conics
    /// ([`edge_shape`]). It may be off the solid, and for a nearly
    /// straight arc far off it: what uses it bounds it.
    pub fn centre(
        &self,
        solid: &Solid,
        faces: [FaceKey; 2],
        near: DVec3,
    ) -> Result<DVec3, Unresolved> {
        let (chain, shape) = self.edge_and_shape(solid, faces, near)?;
        match shape {
            EdgeShape::Circle { centre, .. } | EdgeShape::Ellipse { centre, .. }
                if centre.is_finite() =>
            {
                Ok(centre)
            }
            _ => Err(Unresolved::NotCircular { chain }),
        }
    }

    /// The outward unit normal of the flat face named `key`, nearest
    /// `near` ([`Topology::face`]): its form's.
    pub fn normal(&self, solid: &Solid, key: &FaceKey, near: DVec3) -> Result<DVec3, Unresolved> {
        let region = self.face(solid, key, near)?;
        match *self.form(solid, region) {
            Form::Plane { n, .. } if usable(n) => Ok(n),
            _ => Err(Unresolved::NotFlat { region }),
        }
    }

    /// The axis of the round face named `key`, nearest `near`: its form's
    /// (a cylinder's, cone's, torus's or other surface of revolution's),
    /// directed as the form has it, as the point of it nearest `near`
    /// ([`beside`]) and its unit direction.
    pub fn face_axis(
        &self,
        solid: &Solid,
        key: &FaceKey,
        near: DVec3,
    ) -> Result<[DVec3; 2], Unresolved> {
        let region = self.face(solid, key, near)?;
        match axis_of(self.form(solid, region)) {
            Some([point, axis]) => Ok([beside(point, axis, near), axis]),
            None => Err(Unresolved::NotRound { region }),
        }
    }

    /// The direction of the edge named `faces`, nearest `near`, not unit:
    /// a straight edge's from end to end, the way it runs with the first
    /// key's face on its left ([`runs_with`]); a round edge's axis, unit,
    /// signed as the `topology/datums.rs` docs say (a flat neighbour's
    /// outward normal, else a round neighbour's axis along it, else as
    /// the edge runs).
    pub fn edge_direction(
        &self,
        solid: &Solid,
        faces: [FaceKey; 2],
        near: DVec3,
    ) -> Result<DVec3, Unresolved> {
        let (chain, shape) = self.edge_and_shape(solid, faces, near)?;
        let runs = || {
            self.runs_with(chain, &faces)
                .ok_or(Unresolved::Undirected { chain })
        };
        match shape {
            EdgeShape::Line { from, to } => Ok(if runs()? { to - from } else { from - to }),
            EdgeShape::Circle { axis, .. } | EdgeShape::Ellipse { axis, .. } => {
                // The first key's face first, else the lower region.
                let [r0, r1] = self.chains[chain as usize].regions;
                let beside = match self.runs_with(chain, &faces) {
                    Some(false) => [r1, r0],
                    _ => [r0, r1],
                };
                let forms = beside.map(|r| self.form(solid, r));
                if let Some(n) = forms.iter().find_map(|form| match **form {
                    Form::Plane { n, .. } if usable(n) => Some(n),
                    _ => None,
                }) {
                    return Ok(n);
                }
                let along = |form: &Form| {
                    let [_, a] = axis_of(form)?;
                    let cross = a.cross(axis).length_squared();
                    (cross <= ALONG * ALONG * a.length_squared()).then_some(a)
                };
                if let Some(a) = forms.iter().find_map(|form| along(form)) {
                    return Ok(a);
                }
                if !usable(axis) {
                    return Err(Unresolved::NotAnAxis { chain });
                }
                Ok(if runs()? { axis } else { -axis })
            }
            EdgeShape::Other => Err(Unresolved::NotAnAxis { chain }),
        }
    }

    /// The chain named `faces` nearest `near`, and its shape.
    fn edge_and_shape(
        &self,
        solid: &Solid,
        faces: [FaceKey; 2],
        near: DVec3,
    ) -> Result<(u32, EdgeShape), Unresolved> {
        let chain = self.edge(solid, faces, near)?;
        let found: &Chain = &self.chains[chain as usize];
        Ok((chain, edge_shape(solid, found)))
    }
}

/// Whether `v` is a direction: finite and not zero.
fn usable(v: DVec3) -> bool {
    v.is_finite() && v != DVec3::ZERO
}

/// A round form's axis: a point on it and its unit direction as the form
/// has it. `None` for any other form.
fn axis_of(form: &Form) -> Option<[DVec3; 2]> {
    let [point, axis] = match *form {
        Form::Cylinder { point, axis, .. } => [point, axis],
        Form::Cone { apex, axis, .. } => [apex, axis],
        Form::Torus { centre, axis, .. } => [centre, axis],
        Form::Revolved { origin, axis, .. } => [origin, axis],
        _ => return None,
    };
    (usable(axis) && point.is_finite()).then_some([point, axis])
}

/// The point of the line through `point` along `axis` nearest `near`, or
/// `point` where that isn't a finite point. A form's own point can be far
/// out (a cone that's nearly a cylinder has its apex far along its
/// axis); the face's point is on the face, so the point found is beside
/// it, no further from it than the face's radius there.
pub fn beside(point: DVec3, axis: DVec3, near: DVec3) -> DVec3 {
    let foot = point + axis * ((near - point).dot(axis) / axis.length_squared());
    if foot.is_finite() { foot } else { point }
}
