//! The stand-in for the kernel's draft that tests run in its place
//! until the kernel's is built (see `crate::testing`).

use super::*;
use crate::history::in_place;

/// A stand-in for the kernel's draft, for tests: the solid must be a box
/// along the world's axes ([`in_place::box_sides`]) of eight corners,
/// and the pull along a world axis. Each face picked must be one of the
/// four sides along the pull (a top or bottom faces it:
/// [`DraftError::FacingPull`]); it turns about its hinge line on the
/// neutral plane, its corners moved across the pull by `tan α` times
/// their height above the plane (inward above it, outward below), so it
/// stays flat on the turned plane and the faces around it stay on
/// theirs, every face keeping its name. Sides drafted until they meet
/// the one opposite at the box's top or bottom are
/// [`DraftError::PastNeighbour`], a box past
/// [`MAX_COORD`](varde_kernel::MAX_COORD) [`DraftError::OutOfRange`];
/// anything else is [`KernelError::TooComplex`]. The result passes the
/// kernel's check as every solid does.
///
/// [`KernelError::TooComplex`]: varde_kernel::KernelError::TooComplex
#[allow(clippy::too_many_arguments)]
pub(crate) fn by_boxes(
    solid: &Solid,
    topology: &Topology,
    faces: &[u32],
    neutral: DVec3,
    pull: DVec3,
    angle: f64,
    _tangent: bool,
    _feature: u64,
    tol: &Tolerance,
    _budget: &Budget,
) -> Result<Solid, DraftError> {
    use varde_kernel::KernelError;
    use varde_kernel::mesh::{Edge, Form, Mesh, Surface};
    let too_complex = || DraftError::Failed(KernelError::TooComplex.into());
    let (min, max, sides) = in_place::box_sides(solid, topology).ok_or_else(too_complex)?;
    let mesh = solid.mesh();
    let up = pull.abs().max_position();
    if (pull.abs()[up] - 1.0).abs() > 1e-12 || mesh.verts().len() != 8 {
        return Err(too_complex());
    }
    let sign = pull[up].signum();
    let (sin, cos) = varde_kernel::trig::sin_cos(angle);
    let tan = sin / cos;
    // Each axis's low and high side: the region drafted there, if any.
    let mut drafted = [[None; 2]; 3];
    for &region in faces {
        let (axis, end) = *sides.get(region as usize).ok_or_else(too_complex)?;
        if axis == up {
            return Err(DraftError::FacingPull { region });
        }
        drafted[axis][end] = Some(region);
    }
    // A point's height above the neutral plane, along the pull: a side
    // drafted moves inward by `tan α` times it there.
    let height = |x: DVec3| sign * (x[up] - neutral[up]);
    // The sides meeting the ones opposite them, at either end of the box
    // along the pull (the width is linear in the height between).
    let least = tol.resolution();
    for t in [height(min), height(max)] {
        for (axis, ends) in drafted.iter().enumerate() {
            let shrink = ends.iter().flatten().count() as f64 * tan * t;
            if max[axis] - min[axis] - shrink <= least {
                let region = ends.iter().flatten().next().copied();
                return Err(DraftError::PastNeighbour {
                    region: region.expect("only a drafted side shrinks an axis"),
                });
            }
        }
    }
    let size = (max - min).length();
    let on = |a: f64, b: f64| (a - b).abs() <= 1e-9 * (1.0 + size);
    let verts: Vec<DVec3> = (mesh.verts().iter())
        .map(|&v| {
            let mut moved = v;
            let t = height(v);
            for (axis, ends) in drafted.iter().enumerate() {
                if ends[0].is_some() && on(v[axis], min[axis]) {
                    moved[axis] = min[axis] + tan * t;
                }
                if ends[1].is_some() && on(v[axis], max[axis]) {
                    moved[axis] = max[axis] - tan * t;
                }
            }
            moved
        })
        .collect();
    let reach = f64::from(varde_kernel::MAX_COORD);
    if verts.iter().any(|v| v.abs().max_element() > reach) {
        return Err(DraftError::OutOfRange);
    }
    let mut edges = mesh.edges().to_vec();
    for tri in mesh.tris() {
        for (i, half) in tri.halfedges.iter().enumerate() {
            let end = tri.halfedges[(i + 1) % 3].start;
            edges[half.edge as usize] =
                Edge::straight(verts[half.start as usize], verts[end as usize]);
        }
    }
    let mut new_faces = mesh.faces().to_vec();
    for (region, &(axis, end)) in sides.iter().enumerate() {
        if drafted[axis][end] != Some(region as u32) {
            continue;
        }
        // The hinge: on the side's plane and the neutral plane.
        let s = if end == 1 { 1.0 } else { -1.0 };
        let c = if end == 1 { max[axis] } else { min[axis] };
        let mut n = pull * sin;
        n[axis] = s * cos;
        let d = s * cos * c + sin * sign * neutral[up];
        let key = topology.regions()[region].key;
        for face in new_faces.iter_mut().filter(|face| face.name.key() == key) {
            face.form = Form::Plane { n, d };
            face.surface = Surface::Plane { n, d };
        }
    }
    let drafted_mesh = Mesh::from_parts(verts, edges, mesh.tris().to_vec(), new_faces)
        .with_aliases(mesh.aliases().to_vec());
    Solid::new(drafted_mesh, tol).map_err(|error| DraftError::Failed(error.into()))
}
