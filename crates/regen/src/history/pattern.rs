//! Evaluating a pattern: each of its bodies' solids, as the features
//! before it leave it, with copies of itself placed along a line or about
//! an axis, put together in the body, or each copy a body of its own.

use varde_document::{Document, FeatureId, Pattern, PatternKind};
use varde_kernel::{Budget, Instance, MAX_PATCHES, Motion, Solid, Tolerance, assemble};

use super::motion::{note_reference, resolve_axis, within};
use super::{BodySolid, Evaluation, Failed, own_solids};
use crate::cache::{Cache, Key, Keyer};
use crate::error_geometry::KernelFailure;
use crate::message::{self, Moving};

/// Changes the bodies of `evaluation` (those the features before it
/// made) as the pattern `pattern`, the feature `feature`, says, or says
/// why it fails, changing nothing.
///
/// Its bodies must have solids of their own, as a move's. Its axis is
/// found as a move's ([`resolve_axis`]); a linear pattern uses only its
/// direction. Copy `k` (`1 ≤ k < count`) is placed directly
/// ([`placements`]), never by composing `k` steps, so no rounding piles
/// up along the pattern. Before anything is copied each body is held to
/// the count: `count × patches` within [`MAX_PATCHES`] (a checked
/// product: both come from the user), and every copy's box within the
/// coordinate limit (the farthest copy isn't always the last: a circular
/// pattern's swing out and back). Each body is then its solid and its
/// copies, copy `k`'s faces named as copy `k` of the pattern
/// ([`Instance`]), put together by [`assemble`]: side by side where
/// they're apart, united where they meet. Cached by the body's key, the
/// pattern, every copy's motion's bits and the fit tolerance. Unjoined
/// ([`Copies::Separate`](varde_document::Copies::Separate)), the body is
/// left as it is and each copy, named the same way, is the solid of its
/// own body ([`Pattern::copy_body`]), never united with anything (they
/// may overlap), cached by the body's key, the copy's motion and
/// instance and the fit tolerance ([`copy_key`]); the new bodies go
/// after the others, in the order of their ids.
pub(super) fn evaluate_pattern(
    document: &Document,
    feature: FeatureId,
    pattern: &Pattern,
    tolerance: &Tolerance,
    evaluation: &mut Evaluation,
    cache: &mut Cache,
) -> Result<(), Failed> {
    own_solids(document, pattern.bodies.iter().copied(), evaluation)?;
    let count = pattern.count().ok_or(message::PATTERN_COUNT)?;
    let [point, direction] = resolve_axis(pattern.kind.axis(), evaluation, tolerance, cache)?;
    note_reference(evaluation, feature, [point, direction]);
    let motions =
        placements(pattern, count, [point, direction]).ok_or(message::AXIS_NO_DIRECTION)?;
    let mut changed = Vec::with_capacity(pattern.bodies.len());
    let mut separate = Vec::new();
    for made in (evaluation.bodies.iter()).filter(|m| pattern.bodies.binary_search(&m.body).is_ok())
    {
        let name = document
            .body(made.body)
            .map_or("a body", |body| body.name.as_str());
        let patches = made.solid.mesh().tris().len();
        if !copies_fit(count, patches) {
            return Err(message::too_many_copies(name, count, patches).into());
        }
        if !motions.iter().all(|motion| within(&made.solid, motion)) {
            return Err(message::out_of_range(Moving::Pattern, name).into());
        }
        if !pattern.joins() {
            // Found: the filter above takes only the pattern's bodies.
            let source = pattern.bodies.binary_search(&made.body).unwrap_or(0);
            for (k, motion) in (1..).zip(&motions) {
                // A checked pattern lists a body per copy.
                let body = pattern.copy_body(source, k).ok_or(message::COPY_BODIES)?;
                let copy = Instance {
                    feature: feature.get(),
                    index: u64::from(k),
                };
                let key = copy_key(made.key, motion, copy, tolerance);
                let solid =
                    cache.solid(key, || copy_of(&made.solid, motion, copy, tolerance, name))?;
                separate.push(BodySolid { body, solid, key });
            }
            continue;
        }
        let key = pattern_key(made.key, feature, &motions, tolerance);
        let solid = cache.solid(key, || {
            copied(&made.solid, feature, &motions, tolerance, name)
        })?;
        changed.push(BodySolid {
            body: made.body,
            solid,
            key,
        });
    }
    for change in changed {
        if let Some(made) = evaluation.bodies.iter_mut().find(|m| m.body == change.body) {
            *made = change;
        }
    }
    separate.sort_by_key(|made| made.body);
    evaluation.bodies.extend(separate);
    Ok(())
}

/// `solid`, the body named `name`'s, moved by `motion` as the copy
/// `copy` of a pattern whose copies are bodies of their own; or why not,
/// with the kernel's evidence, as [`copied`] has it.
fn copy_of(
    solid: &Solid,
    motion: &Motion,
    copy: Instance,
    tolerance: &Tolerance,
    name: &str,
) -> Result<Solid, Failed> {
    solid
        .transformed(motion, Some(copy), tolerance, &Budget::DEFAULT)
        .map_err(|failure| {
            let words = message::moving(Moving::Pattern, name, failure.error);
            let failure = KernelFailure::new(failure, tolerance);
            Failed::kernel(words, &failure, [&[], &[]])
        })
}

/// Whether `count` copies of a solid of `patches` patches are at most
/// [`MAX_PATCHES`] together: the product checked, both coming from the
/// user.
pub(crate) fn copies_fit(count: u32, patches: usize) -> bool {
    usize::try_from(count)
        .ok()
        .and_then(|count| count.checked_mul(patches))
        .is_some_and(|total| total <= MAX_PATCHES)
}

/// The motions placing copies `1..count` of `pattern` about the axis
/// `[point, direction]` (as [`resolve_axis`] finds it), or `None` where
/// the axis gives none (no direction): a linear pattern's copy `k` moved
/// `k · spacing` along the direction ([`Motion::pattern_step`]), a
/// circular one's turned `k · span / steps` ([`Pattern::span_steps`],
/// [`Motion::pattern_turn`]), so a whole turn split into quarters is
/// exact.
pub(crate) fn placements(
    pattern: &Pattern,
    count: u32,
    [point, direction]: [glam::DVec3; 2],
) -> Option<Vec<Motion>> {
    let span = pattern.span_steps();
    (1..count)
        .map(|k| match &pattern.kind {
            PatternKind::Linear { spacing, .. } => {
                Motion::pattern_step(direction, spacing.value, k)
            }
            PatternKind::Circular { .. } => {
                let (degrees, steps) = span?;
                Motion::pattern_turn(point, direction, degrees, k, steps)
            }
        })
        .collect()
}

/// The key of copy `copy` alone, by `motion`, of the solid filed under
/// `body`, at `tolerance`, a body of its own: apart from
/// `motion.rs`'s `moved_key`, whose copies are the solid with its image.
fn copy_key(body: Key, motion: &Motion, copy: Instance, tolerance: &Tolerance) -> Key {
    let mut keyer = Keyer::new("pattern copy");
    keyer.key(body);
    for bits in motion.bits() {
        keyer.number(bits);
    }
    keyer
        .number(copy.feature)
        .number(copy.index)
        .number(tolerance.fit().to_bits())
        .finish()
}

/// The key of `feature`'s copies by `motions` of the solid filed under
/// `body`, put together with it, at `tolerance`.
fn pattern_key(body: Key, feature: FeatureId, motions: &[Motion], tolerance: &Tolerance) -> Key {
    let mut keyer = Keyer::new("pattern");
    keyer
        .key(body)
        .number(feature.get())
        .number(motions.len() as u64);
    for motion in motions {
        for bits in motion.bits() {
            keyer.number(bits);
        }
    }
    keyer.number(tolerance.fit().to_bits()).finish()
}

/// `solid`, the body named `name`'s, with its copies by `motions` (copy
/// `k` by the `k`th, named as copy `k` of `feature`), put together; or
/// why not, with the kernel's evidence (patches and curves by value, as
/// a move's).
fn copied(
    solid: &Solid,
    feature: FeatureId,
    motions: &[Motion],
    tolerance: &Tolerance,
    name: &str,
) -> Result<Solid, Failed> {
    let kernel = |words: String, failure| {
        let failure = KernelFailure::new(failure, tolerance);
        Failed::kernel(words, &failure, [&[], &[]])
    };
    let mut parts = Vec::with_capacity(motions.len().saturating_add(1));
    parts.push(solid.clone());
    for (index, motion) in (1..).zip(motions) {
        let copy = Instance {
            feature: feature.get(),
            index,
        };
        let part = solid
            .transformed(motion, Some(copy), tolerance, &Budget::DEFAULT)
            .map_err(|failure| {
                kernel(
                    message::moving(Moving::Pattern, name, failure.error),
                    failure,
                )
            })?;
        parts.push(part);
    }
    assemble(&parts, tolerance, &Budget::DEFAULT)
        .map_err(|failure| kernel(message::with_copies(name, failure.error), failure))
}
