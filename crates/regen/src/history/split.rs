//! Evaluating a split: the body's solid, as the features before it leave
//! it, cut in two by a tool, one piece kept by the body and the other
//! given to the split's new body (or dropped, keeping one side).
//!
//! The tool is built from the body's box (its faces past the box lie
//! clear of the body): an origin plane's or a flat face's half-space
//! ([`varde_kernel::half_space`], the face found on its body as the
//! features before the split leave it, as a mirror's plane is), a face's
//! surface continued past the body ([`varde_kernel::surface_tool`], the
//! face found the same way; a form with no surface to continue is
//! refused), another body's solid as it is, a sketch's regions extruded
//! through the body both ways (as an extrude's tool, from the body's
//! extent along the sketch's normal with through all's margin), or an
//! open chain of the sketch's curves ([`chain`], then
//! [`varde_kernel::chain_tool`]). Each tool is cached by its inputs, the
//! body's box and the fit tolerance; a tool body by its own key. Then
//! one [`varde_kernel::split`] gives both pieces from one arrangement,
//! cached by the body's key and the tool's. A side that holds nothing
//! fails the split ("lies all on one side"): no body is ever empty.
//!
//! The piece the split's [`Split::kept`] names keeps the body's id; the
//! other, where both are kept, is the new body's, pushed after the
//! bodies made before (it's made by the split), and the pair is noted in
//! [`Evaluation::splits`] for a sketch on a face that went to the new
//! body to follow it there (see [`super::place_on_face`]).
//!
//! The kernel's split and its tools aren't built yet: they fail as too
//! complex, which reaches the user as any boolean's "too complex"
//! message, and the rest of the history goes on.

use std::sync::Arc;

use varde_document::{BodyId, Document, FaceRef, FeatureId, PlaneRef, Side, Split, SplitTool};
use varde_kernel::mesh::Form;
use varde_kernel::patch::Bounds3;
use varde_kernel::{Budget, Failure, Frame, Solid, Tolerance, ToolError};

use super::{BodySolid, Evaluation, Failed, SketchOutput, face_geometry, own_solids, through_all};
use crate::cache::{Cache, Key, Keyer};
use crate::error_geometry::KernelFailure;
use crate::message::{self, Making};
use crate::picking::region_form;
use crate::profile::{chain, profile};

/// The kernel's split, which tests may replace with one built of two
/// booleans to check what regeneration does with the pieces before the
/// kernel's is built.
type Splitter = fn(&Solid, &Solid, &Tolerance, &Budget) -> Result<(Solid, Solid), Failure>;

#[cfg(any(test, feature = "testing"))]
thread_local! {
    /// The split a test asks for in place of the kernel's, on its own
    /// thread.
    pub(crate) static SPLITTER: std::cell::Cell<Option<Splitter>> =
        const { std::cell::Cell::new(None) };
}

/// The split to run: the kernel's, or the one a test set.
fn splitter() -> Splitter {
    #[cfg(any(test, feature = "testing"))]
    if let Some(splitter) = SPLITTER.get() {
        return splitter;
    }
    varde_kernel::split
}

/// The kernel's split as two booleans, the front `body ∩ tool` and the
/// back `body − tool`: what regeneration does with the pieces is tested
/// with it until the kernel's is built.
#[cfg(any(test, feature = "testing"))]
pub(crate) fn by_booleans(
    body: &Solid,
    tool: &Solid,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<(Solid, Solid), Failure> {
    use varde_kernel::Op;
    let front = varde_kernel::boolean(body, tool, Op::Intersection, tol, budget)?;
    let back = varde_kernel::boolean(body, tool, Op::Difference, tol, budget)?;
    Ok((front, back))
}

/// Splits on this thread by [`by_booleans`] from now on.
#[cfg(any(test, feature = "testing"))]
pub(crate) fn split_by_booleans() {
    SPLITTER.set(Some(by_booleans));
}

/// The profile curve id a chain tool's rectangle is named by: past every
/// sketch curve's id (a `u32`).
const RIM: u64 = 1 << 32;

/// Changes the bodies of `evaluation` as the split `split`, the feature
/// `feature`, says, given the sketches evaluated before it, or says why
/// it fails, changing nothing.
///
/// Its body must have a solid of its own, and so must a tool body
/// ([`own_solids`]: one a join or a combine consumed fails it, naming the
/// body holding it). Then the tool, the split and the pieces, as the
/// module's docs say.
pub(super) fn evaluate_split(
    document: &Document,
    feature: FeatureId,
    split: &Split,
    sketches: &[SketchOutput],
    tolerance: &Tolerance,
    evaluation: &mut Evaluation,
    cache: &mut Cache,
) -> Result<(), Failed> {
    own_solids(document, std::iter::once(split.body), evaluation)?;
    if let SplitTool::Body(tool) = split.tool {
        own_solids(document, std::iter::once(tool), evaluation)?;
    }
    let name = |body: BodyId| {
        document
            .body(body)
            .map_or("a body", |body| body.name.as_str())
    };
    let made = (evaluation.bodies.iter())
        .find(|made| made.body == split.body)
        .expect("the body has a solid of its own");
    let body_name = name(split.body);
    // A body is never empty, so it has a box.
    let bounds = made
        .solid
        .bounds3()
        .ok_or_else(|| message::split_one_side(body_name))?;
    let building = Building {
        feature,
        body: &made.solid,
        bounds,
        tolerance: *tolerance,
        body_name,
    };
    let (tool, tool_key) = building.tool(&split.tool, sketches, evaluation, cache)?;
    let key = Keyer::new("split")
        .key(made.key)
        .key(tool_key)
        .number(tolerance.fit().to_bits())
        .finish();
    let split_by = splitter();
    let [front, back] = cache
        .split(key, || {
            split_by(&made.solid, &tool, tolerance, &Budget::DEFAULT)
                .map_err(|failure| KernelFailure::new(failure, tolerance))
        })
        .map_err(|failure| {
            let tools: &[BodyId] = match &split.tool {
                SplitTool::Body(tool) => std::slice::from_ref(tool),
                _ => &[],
            };
            let words = message::splitting(body_name, failure.error);
            Failed::kernel(words, &failure, [&[split.body], tools])
        })?;
    if front.is_empty() || back.is_empty() {
        return Err(message::split_one_side(body_name).into());
    }
    let piece_key = |side: Side| {
        let side = match side {
            Side::Front => 0,
            Side::Back => 1,
        };
        Keyer::new("split piece").key(key).number(side).finish()
    };
    let kept = split.kept();
    let piece = |side: Side| match side {
        Side::Front => Arc::clone(&front),
        Side::Back => Arc::clone(&back),
    };
    let new = split.new_body.map(|body| BodySolid {
        body,
        solid: piece(kept.other()),
        key: piece_key(kept.other()),
    });
    if let Some(made) = evaluation.bodies.iter_mut().find(|m| m.body == split.body) {
        *made = BodySolid {
            body: split.body,
            solid: piece(kept),
            key: piece_key(kept),
        };
    }
    if let Some(new) = new {
        evaluation.splits.push((split.body, new.body));
        evaluation.bodies.push(new);
    }
    Ok(())
}

/// What building a split's tool needs.
struct Building<'a> {
    feature: FeatureId,
    /// The split body's solid and its box.
    body: &'a Solid,
    bounds: Bounds3,
    tolerance: Tolerance,
    body_name: &'a str,
}

impl Building<'_> {
    /// The tool `tool` names and the key it's filed under, or why there's
    /// none.
    fn tool(
        &self,
        tool: &SplitTool,
        sketches: &[SketchOutput],
        evaluation: &Evaluation,
        cache: &mut Cache,
    ) -> Result<(Arc<Solid>, Key), Failed> {
        match tool {
            SplitTool::Plane(PlaneRef::Origin(plane)) => {
                let normal = plane.placement().normal;
                let key = self.keyer("split plane").value(plane).finish();
                let solid = cache.solid(key, || self.half_space(normal, 0.0))?;
                Ok((solid, key))
            }
            SplitTool::Plane(PlaneRef::Face(face)) => {
                let made = super::motion::holding(face.body, evaluation)
                    .ok_or(message::SPLIT_PLANE_BODY_GONE)?;
                let key = self.face_key("split plane face", made.key, face);
                let solid = cache.solid(key, || {
                    let (n, d) = plane_of(&made.solid, face, &self.tolerance)?;
                    self.half_space(n, d)
                })?;
                Ok((solid, key))
            }
            SplitTool::Face(face) => {
                let made = super::motion::holding(face.body, evaluation)
                    .ok_or(message::SPLIT_FACE_BODY_GONE)?;
                let key = self.face_key("split face", made.key, face);
                let solid = cache.solid(key, || self.surface(&made.solid, face))?;
                Ok((solid, key))
            }
            SplitTool::Body(body) => {
                let made = (evaluation.bodies.iter())
                    .find(|made| made.body == *body)
                    .expect("the tool body has a solid of its own");
                Ok((Arc::clone(&made.solid), made.key))
            }
            SplitTool::Regions { sketch, regions } => {
                let sketch = find_sketch(sketches, *sketch)?;
                let placement = sketch
                    .placement
                    .ok_or_else(|| message::SKETCH_NOT_PLACED.to_owned())?;
                let frame = Frame {
                    origin: placement.origin,
                    x: placement.x,
                    y: placement.y,
                };
                let (from, to) = through_all(&frame, [self.body])
                    .ok_or_else(|| message::split_one_side(self.body_name))?;
                let key = self
                    .keyer("split regions")
                    .value(regions)
                    .number(from.to_bits())
                    .number(to.to_bits())
                    .key(sketch.key)
                    .placement(&placement)
                    .finish();
                let solid = cache.solid(key, || {
                    let profiles = match &*sketch.profiles {
                        Ok(profiles) => profiles,
                        Err(e) => return Err(format!("its sketch is {e}").into()),
                    };
                    let found = profiles
                        .resolve(regions)
                        .into_iter()
                        .collect::<Option<Vec<usize>>>()
                        .ok_or("region not found")?;
                    let loops = profiles.merge(&found).map_err(|e| e.to_string())?;
                    let fit = self.tolerance.fit();
                    let profile =
                        profile(sketch.sketch, profiles, &loops, fit).map_err(|e| e.to_string())?;
                    varde_kernel::extrude(
                        &profile,
                        &frame,
                        from,
                        to,
                        self.feature.get(),
                        &self.tolerance,
                        &Budget::DEFAULT,
                    )
                    .map_err(|failure| {
                        let finest = self.tolerance.fit() <= Tolerance::MIN_FIT;
                        let words = message::tool(Making::Extrude, failure.error, finest);
                        let failure = KernelFailure::new(failure, &self.tolerance);
                        Failed::kernel(words, &failure, [&[], &[]])
                    })
                })?;
                Ok((solid, key))
            }
            SplitTool::Chain { sketch, curves } => {
                let sketch = find_sketch(sketches, *sketch)?;
                let placement = sketch
                    .placement
                    .ok_or_else(|| message::SKETCH_NOT_PLACED.to_owned())?;
                let frame = Frame {
                    origin: placement.origin,
                    x: placement.x,
                    y: placement.y,
                };
                let key = self
                    .keyer("split chain")
                    .value(curves)
                    .key(sketch.key)
                    .placement(&placement)
                    .finish();
                let solid = cache.solid(key, || {
                    let segments = chain(
                        sketch.sketch,
                        curves,
                        self.tolerance.resolution(),
                        self.tolerance.fit(),
                    )
                    .map_err(message::split_line)?;
                    varde_kernel::chain_tool(
                        &segments,
                        &frame,
                        &self.bounds,
                        RIM,
                        self.feature.get(),
                        &self.tolerance,
                        &Budget::DEFAULT,
                    )
                    .map_err(|failure| self.tool_failed(failure))
                })?;
                Ok((solid, key))
            }
        }
    }

    /// A keyer for a tool of `kind`: the feature naming its faces, the
    /// body's box and the fit tolerance.
    fn keyer(&self, kind: &str) -> Keyer {
        let mut keyer = Keyer::new(kind);
        keyer.number(self.feature.get());
        for corner in [self.bounds.min, self.bounds.max] {
            for x in corner.to_array() {
                keyer.number(x.to_bits());
            }
        }
        keyer.number(self.tolerance.fit().to_bits());
        keyer
    }

    /// The key of a tool from `face`, found on the solid filed under
    /// `solid`.
    fn face_key(&self, kind: &str, solid: Key, face: &FaceRef) -> Key {
        let mut keyer = self.keyer(kind);
        keyer.key(solid).value(&face.key);
        for x in face.near.to_array() {
            keyer.number(x.to_bits());
        }
        keyer.finish()
    }

    /// The half-space on the side of `n·x = d` that `n` points to, past
    /// the body.
    fn half_space(&self, n: glam::DVec3, d: f64) -> Result<Solid, Failed> {
        let feature = self.feature.get();
        let budget = &Budget::DEFAULT;
        varde_kernel::half_space(n, d, &self.bounds, feature, &self.tolerance, budget)
            .map_err(|failure| self.tool_failed(failure))
    }

    /// The tool `face`'s surface bounds, continued past the body, the face
    /// found on `solid`.
    fn surface(&self, solid: &Solid, face: &FaceRef) -> Result<Solid, Failed> {
        let topology = solid.topology();
        let region = (topology.face(solid, &face.key, face.near))
            .map_err(|_| message::SPLIT_FACE_NOT_FOUND)?;
        let region = &topology.regions()[region as usize];
        let form = region_form(solid, region);
        // A point inside the face, which picks a cone's nappe: the middle
        // of its first patch.
        let on = solid
            .mesh()
            .patch(region.tris[0] as usize)
            .eval(glam::DVec3::splat(1.0 / 3.0));
        let feature = self.feature.get();
        let budget = &Budget::DEFAULT;
        varde_kernel::surface_tool(form, on, &self.bounds, feature, &self.tolerance, budget)
            .map_err(|error| match error {
                ToolError::CantExtend => Failed {
                    message: message::SPLIT_CANT_EXTEND.to_owned(),
                    geometry: face_geometry(solid, region, &self.tolerance),
                },
                ToolError::Failed(failure) => self.tool_failed(failure),
            })
    }

    /// Why the kernel couldn't build the tool, in words, with its
    /// evidence (by value: the tool is no body).
    fn tool_failed(&self, failure: Failure) -> Failed {
        let words = message::split_tool(self.body_name, failure.error);
        let failure = KernelFailure::new(failure, &self.tolerance);
        Failed::kernel(words, &failure, [&[], &[]])
    }
}

/// The sketch `id` among those evaluated before the split.
fn find_sketch<'s, 'a>(
    sketches: &'s [SketchOutput<'a>],
    id: FeatureId,
) -> Result<&'s SketchOutput<'a>, Failed> {
    (sketches.iter())
        .find(|sketch| sketch.id == id)
        .ok_or_else(|| message::SPLIT_SKETCH_GONE.into())
}

/// The plane of the flat face `face` names on `solid`, its unit normal
/// out of the solid and offset, `n·x = d`, or why there's none.
fn plane_of(
    solid: &Solid,
    face: &FaceRef,
    tolerance: &Tolerance,
) -> Result<(glam::DVec3, f64), Failed> {
    let topology = solid.topology();
    let region =
        (topology.face(solid, &face.key, face.near)).map_err(|_| message::SPLIT_PLANE_NOT_FOUND)?;
    let region = &topology.regions()[region as usize];
    match *region_form(solid, region) {
        Form::Plane { n, d } if n != glam::DVec3::ZERO && n.is_finite() && d.is_finite() => {
            Ok((n, d))
        }
        _ => Err(Failed {
            message: message::SPLIT_PLANE_NOT_FLAT.to_owned(),
            geometry: face_geometry(solid, region, tolerance),
        }),
    }
}
