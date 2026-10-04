//! What the features changing one body's solid in place share (chamfer,
//! shell, offset face, draft): the body's own solid and topology as the
//! features before leave it, its faces found by their references, the
//! cache key's start, the kernel's result put in the body's place, and a
//! region drawn for a refusal; for tests, the box their stand-ins take.

use std::sync::Arc;

use varde_document::{BodyId, Document, FaceRef, FeatureId};
use varde_kernel::{Budget, Evidence, Solid, Tolerance, Topology};

use super::{Evaluation, Failed, own_solids};
use crate::cache::{Cache, Key, Keyer};
use crate::inspect;

/// The body a feature changes in place, as the features before it leave
/// it: its own solid (one a join or a combine consumed fails the
/// feature, see [`own_solids`]), what that's filed under, its topology
/// (the one drawing it keeps, [`inspect::topology`]) and its name.
pub(super) struct InPlace<'a> {
    pub(super) body: BodyId,
    pub(super) name: &'a str,
    pub(super) solid: Arc<Solid>,
    pub(super) key: Key,
    pub(super) topology: Arc<Topology>,
}

impl<'a> InPlace<'a> {
    /// `body` in `evaluation`, or why the feature naming it fails.
    pub(super) fn of(
        document: &'a Document,
        body: BodyId,
        evaluation: &Evaluation,
        cache: &mut Cache,
    ) -> Result<InPlace<'a>, Failed> {
        own_solids(document, std::iter::once(body), evaluation)?;
        let name = document
            .body(body)
            .map_or("a body", |body| body.name.as_str());
        let made = (evaluation.bodies.iter())
            .find(|made| made.body == body)
            .expect("the body has a solid of its own");
        Ok(InPlace {
            body,
            name,
            solid: Arc::clone(&made.solid),
            key: made.key,
            topology: inspect::topology(made, cache),
        })
    }

    /// The regions `faces` name on the body (by their keys and points),
    /// sorted, each once (two references may name one face), or, for the
    /// first not found, `not_found` with its index (from 0) and the
    /// count.
    pub(super) fn regions(
        &self,
        faces: &[FaceRef],
        not_found: fn(usize, usize) -> String,
    ) -> Result<Vec<u32>, Failed> {
        let count = faces.len();
        let mut regions = Vec::with_capacity(count);
        for (i, face) in faces.iter().enumerate() {
            let region = (self.topology.face(&self.solid, &face.key, face.near))
                .map_err(|_| not_found(i, count))?;
            regions.push(region);
        }
        regions.sort_unstable();
        regions.dedup();
        Ok(regions)
    }

    /// A cache key of the kind `kind` begun with what every such
    /// feature's result depends on: the body's solid, the feature (which
    /// names new faces) and the fit tolerance. The feature's own values
    /// follow.
    pub(super) fn keyer(&self, kind: &str, feature: FeatureId, tolerance: &Tolerance) -> Keyer {
        let mut keyer = Keyer::new(kind);
        keyer
            .key(self.key)
            .number(feature.get())
            .number(tolerance.fit().to_bits());
        keyer
    }

    /// Puts the solid `make` gives (the kernel's operation, run with the
    /// default budget, its result cached under `key`) in place of the
    /// body's in `evaluation`, the body keeping its id; or says why it
    /// fails: as `make` has it, or `leaves_nothing` (naming the body)
    /// for an empty solid.
    pub(super) fn replace(
        &self,
        key: Key,
        evaluation: &mut Evaluation,
        cache: &mut Cache,
        leaves_nothing: fn(&str) -> String,
        make: impl FnOnce(&Budget) -> Result<Solid, Failed>,
    ) -> Result<(), Failed> {
        let result = cache.solid(key, || {
            let made = make(&Budget::DEFAULT)?;
            if made.is_empty() {
                return Err(leaves_nothing(self.name).into());
            }
            Ok(made)
        })?;
        if let Some(made) = (evaluation.bodies.iter_mut()).find(|made| made.body == self.body) {
            made.solid = result;
            made.key = key;
        }
        Ok(())
    }
}

/// Adds the region `region` of `topology` (of `solid`) to `evidence`,
/// for a refusal about that face; nothing for a region that isn't
/// there.
pub(super) fn draw_region(
    evidence: &mut Evidence,
    solid: &Solid,
    topology: &Topology,
    region: u32,
) {
    let mesh = solid.mesh();
    if let Some(region) = topology.regions().get(region as usize) {
        evidence.add_patches(
            (region.tris.iter())
                .filter(|&&t| (t as usize) < mesh.tris().len())
                .map(|&t| mesh.patch(t as usize)),
        );
    }
}

/// A box's least and greatest corners, and each region's side of it
/// ([`box_sides`]).
#[cfg(any(test, feature = "testing"))]
pub(super) type BoxSides = (glam::DVec3, glam::DVec3, Vec<(usize, usize)>);

/// The box a stand-in for the kernel takes, for tests: `solid` must be
/// a box along the world's axes (six planar faces square to them, its
/// volume its box's). Its least and greatest corners, and each region's
/// side of it: the axis, and 0 at the low end, 1 at the high. `None`
/// for anything else.
#[cfg(any(test, feature = "testing"))]
pub(super) fn box_sides(solid: &Solid, topology: &Topology) -> Option<BoxSides> {
    use varde_kernel::mesh::Form;
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
