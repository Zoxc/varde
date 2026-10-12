//! The stand-ins for the kernel operations that aren't built yet, one
//! file each, and the box several of them take.

pub(super) mod chamfer;
pub(super) mod face_draft;
pub(super) mod fillet;
pub(super) mod loft;
pub(super) mod offset_face;
pub(super) mod shell;
pub(super) mod split;
pub(super) mod sweep;
pub(super) mod taper;

use glam::DVec3;
use varde_kernel_compat::mesh::Form;
use varde_kernel_compat::{Solid, Topology};

/// A box's least and greatest corners, and each region's side of it
/// ([`box_sides`]).
pub(super) type BoxSides = (DVec3, DVec3, Vec<(usize, usize)>);

/// The box a stand-in for the kernel takes: `solid` must be a box along
/// the world's axes (six planar faces square to them, its volume its
/// box's). Its least and greatest corners, and each region's side of
/// it: the axis, and 0 at the low end, 1 at the high. `None` for
/// anything else.
pub(super) fn box_sides(solid: &Solid, topology: &Topology) -> Option<BoxSides> {
    let bounds = solid.bounds3()?;
    let (min, max) = (bounds.min, bounds.max);
    let size = max - min;
    let volume = size.x * size.y * size.z;
    if topology.regions().len() != 6 || (solid.volume() - volume).abs() > 1e-9 * volume {
        return None;
    }
    let mut sides = Vec::with_capacity(6);
    for region in topology.regions() {
        let Form::Plane { n, .. } = *crate::picking::region_form(solid, region) else {
            return None;
        };
        let n = n.normalize();
        let axis = n.abs().max_position();
        if (n.abs()[axis] - 1.0).abs() > 1e-12 {
            return None;
        }
        sides.push((axis, usize::from(n[axis] > 0.0)));
    }
    Some((min, max, sides))
}
