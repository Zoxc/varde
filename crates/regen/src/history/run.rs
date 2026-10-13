//! The extrude, revolve, sweep and loft tool run: making the tool solid
//! and joining, cutting or intersecting the bodies it touches with it,
//! merging those a join touches; and a through-all extent's span.

use std::cmp::Ordering;
use std::sync::Arc;

use varde_document::{
    BodyId, Document, Extrude, Feature, Loft, MAX_COORD, Operation, Placement, Revolve, Sweep,
};
use varde_kernel_compat::{Budget, Failure, Frame, Op, Profile, Solid, Tolerance};
use varde_sketch::RegionRef;

use crate::cache::{Cache, Key, Keyer};
use crate::error_geometry::KernelFailure;
use crate::why::{self, Doing, Making};

use super::{
    BodySolid, Evaluation, Failed, SketchOutput, body_name, corners, frame, note_merge, taper,
};

/// The margin a through-all span gets past the bodies' extent at each
/// end: this share of the extent, and [`THROUGH_ALL_MARGIN_MM`].
pub(super) const THROUGH_ALL_MARGIN: f64 = 0.01;
pub(super) const THROUGH_ALL_MARGIN_MM: f64 = 1.0;

/// What a feature making a tool solid makes it with.
#[derive(Clone, Copy)]
pub(super) enum Shape<'a> {
    Extrude(&'a Extrude),
    Revolve(&'a Revolve),
    Sweep(&'a Sweep),
    Loft(&'a Loft),
}

impl<'a> Shape<'a> {
    /// Its regions: none of its sketch's for a loft, whose sections
    /// name their own.
    pub(super) fn regions(self) -> &'a [RegionRef] {
        match self {
            Shape::Extrude(extrude) => &extrude.regions,
            Shape::Revolve(revolve) => &revolve.regions,
            Shape::Sweep(sweep) => &sweep.regions,
            Shape::Loft(_) => &[],
        }
    }

    /// What it makes, for the messages.
    pub(super) fn making(self) -> Making {
        match self {
            Shape::Extrude(_) => Making::Extrude,
            Shape::Revolve(_) => Making::Revolve,
            Shape::Sweep(_) => Making::Sweep,
            Shape::Loft(_) => Making::Loft,
        }
    }
}

/// An extrude, revolve, sweep or loft being evaluated.
pub(super) struct Run<'a> {
    pub(super) document: &'a Document,
    pub(super) feature: &'a Feature,
    pub(super) shape: Shape<'a>,
    pub(super) operation: &'a Operation,
    /// Its sketch: a loft's first section's.
    pub(super) sketch: &'a SketchOutput<'a>,
    /// Every sketch before it, for a sweep's path or a loft's sections
    /// and rails.
    pub(super) sketches: &'a [SketchOutput<'a>],
    pub(super) tolerance: Tolerance,
    /// The budget of each [`varde_kernel_compat::touches`].
    pub(super) touching: Budget,
}

impl Run<'_> {
    /// Adds the body it makes to `evaluation`, or changes those it works
    /// on, given those made before it; or why it fails, changing none.
    pub(super) fn evaluate(
        &self,
        evaluation: &mut Evaluation,
        cache: &mut Cache,
    ) -> Result<(), Failed> {
        // The tool depends on the regions and where it runs, not on what
        // it's then used for, so changing the operation or its bodies
        // finds it again.
        let (tool, tool_key) = match self.shape {
            Shape::Extrude(extrude) => self.extruded(extrude, evaluation, cache)?,
            Shape::Revolve(revolve) => self.revolved(revolve, evaluation, cache)?,
            Shape::Sweep(sweep) => self.swept(sweep, evaluation, cache)?,
            Shape::Loft(loft) => self.lofted(loft, cache)?,
        };
        let (op, doing) = match self.operation {
            Operation::NewBody(body) => {
                evaluation.bodies.push(BodySolid {
                    body: *body,
                    solid: tool,
                    key: tool_key,
                });
                return Ok(());
            }
            Operation::Join(_) => (Op::Union, Doing::Joining),
            Operation::Cut(_) => (Op::Difference, Doing::Cutting),
            Operation::Intersect(_) => (Op::Intersection, Doing::Intersecting),
        };
        let excluded = self.operation.excluded();
        let touched = self.touched(&evaluation.bodies, excluded, (&tool, tool_key), cache);
        let found = touched.as_ref().unwrap_or_else(|(found, _)| found).clone();
        evaluation.touched.push((self.feature.id, found));
        let targets = touched.map_err(|(_, error)| error)?;
        let mut taken_out = false;
        for made in (evaluation.bodies.iter()).filter(|made| excluded.contains(&made.body)) {
            // What the request before worked out, kept for putting it
            // back.
            cache.keep(touches_key(made.key, tool_key));
            cache.keep(boolean_key(doing, made.key, tool_key));
            taken_out = true;
        }
        if taken_out && doing == Doing::Joining {
            // The merge with every body taken out put back: putting back
            // the one taken out finds it.
            let bodies = (evaluation.bodies.iter())
                .filter(|made| targets.contains(&made.body) || excluded.contains(&made.body));
            for key in merge_keys(bodies.collect(), tool_key) {
                cache.keep(key);
            }
        }
        if targets.is_empty() {
            return Err(if taken_out {
                "it doesn't touch any body not taken out of it"
            } else {
                "it doesn't touch any body"
            }
            .into());
        }
        if doing == Doing::Joining && targets.len() > 1 {
            return self.merge_into_first(evaluation, &targets, (&tool, tool_key), cache);
        }
        // Worked out for every target before any body changes. Where
        // there are others, one failing can be left out.
        let mut changed = Vec::with_capacity(targets.len());
        let mut uncut = Vec::new();
        for made in evaluation
            .bodies
            .iter()
            .filter(|m| targets.contains(&m.body))
        {
            let name = self.body_name(made.body);
            let fails = |message: String| match targets.len() {
                1 => message,
                _ => why::leave_out(message, name),
            };
            let key = boolean_key(doing, made.key, tool_key);
            let solid = cached_boolean(cache, key, (&made.solid, &tool), op, &self.tolerance)
                .map_err(|failure| {
                    let words = fails(why::boolean(doing, name, failure.error));
                    Failed::kernel(words, &failure, [&[made.body], &[]])
                })?;
            // The cached empty result stays: its key is right, and the
            // check is cheap to make again. A cut's message says to
            // untick it already.
            if solid.is_empty() {
                let emptied = why::emptied(doing, name);
                return Err(match doing {
                    Doing::Cutting => emptied,
                    _ => fails(emptied),
                }
                .into());
            }
            if doing == Doing::Cutting && self.cuts_nothing(made, &solid, (&tool, tool_key), cache)
            {
                uncut.push(made.body);
            }
            changed.push(BodySolid {
                body: made.body,
                solid,
                key,
            });
        }
        if doing == Doing::Cutting {
            evaluation.uncut.push((self.feature.id, uncut));
        }
        for change in changed {
            evaluation.replace(change);
        }
        Ok(())
    }

    /// Whether the cut leaving `made` as `cut` took nothing from it: the
    /// tool, filed under `tool_key`, only touches it, face to face or
    /// along an edge. Their volumes tell a body cut from one left whole
    /// but for a sliver; the tool's intersection with the body,
    /// worked out (and cached) as an intersect's would be, makes sure
    /// it holds nothing. One that fails says the cut took something:
    /// this is only for a note.
    pub(super) fn cuts_nothing(
        &self,
        made: &BodySolid,
        cut: &Solid,
        (tool, tool_key): (&Solid, Key),
        cache: &mut Cache,
    ) -> bool {
        let before = made.solid.volume();
        let taken = before - cut.volume();
        if taken.abs().partial_cmp(&(UNCUT_SHARE * before.abs())) != Some(Ordering::Less) {
            return false;
        }
        let key = boolean_key(Doing::Intersecting, made.key, tool_key);
        let common = cached_boolean(
            cache,
            key,
            (&made.solid, tool),
            Op::Intersection,
            &self.tolerance,
        );
        common.is_ok_and(|common| common.is_empty())
    }

    /// Merges the bodies `targets` (two or more, in the order they were
    /// made) and the tool, filed under `tool_key`, into the first: it gets
    /// the union of them all, and the others are consumed (taken out of
    /// `evaluation`'s bodies and listed in its `merged`, which entries
    /// naming them are moved on from). Or why it fails, changing nothing.
    pub(super) fn merge_into_first(
        &self,
        evaluation: &mut Evaluation,
        targets: &[BodyId],
        tool: (&Solid, Key),
        cache: &mut Cache,
    ) -> Result<(), Failed> {
        let bodies: Vec<&BodySolid> = (evaluation.bodies.iter())
            .filter(|made| targets.contains(&made.body))
            .collect();
        let merging: Vec<BodyId> = bodies.iter().map(|made| made.body).collect();
        let (into, consumed) = (merging[0], &merging[1..]);
        let (solid, key) = self.merge(&bodies, tool, cache)?;
        // A union of solids that aren't empty isn't, but it's cheap to
        // make sure no body ever is.
        if solid.is_empty() {
            return Err(why::emptied(Doing::Joining, self.body_name(into)).into());
        }
        note_merge(&mut evaluation.merged, &merging);
        (evaluation.bodies).retain(|made| !consumed.contains(&made.body));
        evaluation.replace(BodySolid {
            body: into,
            solid,
            key,
        });
        Ok(())
    }

    /// The union of `bodies` (two or more, in the order they were made)
    /// and the tool filed under `tool_key`, and the key it's filed under;
    /// or why it fails. Every step is a [`varde_kernel_compat::boolean`] union,
    /// the running solid first, cached under its own key from its
    /// operands' keys ([`merge_keys`]). The bodies are united first and
    /// the tool last: that passes the bodies' own faces first, which
    /// keeps the patches few where the tool is flush with one of them
    /// (the tool put first as a flush boss has come out with tens of
    /// thousands of patches), and the bodies' union doesn't depend on
    /// the tool, so dragging a draft reworks only the last step. Bodies
    /// meeting each other only along an edge or at a point make no clean
    /// solid on their own, though the tool bridges them, so if any step
    /// fails the tool is joined to the first body instead, as it would
    /// be alone, and the others are joined to that in turn; if that
    /// fails too, its error is given, with its evidence: the faces it
    /// names of the running solid on the first body and those merged into
    /// it so far (the tool is drawn as no body), and of the body being
    /// merged on that body.
    pub(super) fn merge(
        &self,
        bodies: &[&BodySolid],
        (tool, tool_key): (&Solid, Key),
        cache: &mut Cache,
    ) -> Result<(Arc<Solid>, Key), Failed> {
        let (first, rest) = bodies.split_first().expect("two or more bodies are merged");
        let mut unite = |key: Key, a: &Solid, b: &Solid| {
            let solid = cached_boolean(cache, key, (a, b), Op::Union, &self.tolerance);
            solid.map(|solid| (solid, key))
        };
        // The bodies first, then the tool.
        let mut bodies_first = Ok((Arc::clone(&first.solid), first.key));
        for made in rest {
            bodies_first = bodies_first.and_then(|(solid, key)| {
                unite(
                    boolean_key(Doing::Merging, key, made.key),
                    &solid,
                    &made.solid,
                )
            });
        }
        let bodies_first = bodies_first.and_then(|(solid, key)| {
            unite(boolean_key(Doing::Joining, key, tool_key), &solid, tool)
        });
        if let Ok(merged) = bodies_first {
            return Ok(merged);
        }
        // The tool joined to the first body, then the others.
        let mut merged = unite(
            boolean_key(Doing::Joining, first.key, tool_key),
            &first.solid,
            tool,
        )
        .map_err(|failure| {
            let name = self.body_name(first.body);
            let words = why::leave_out(why::boolean(Doing::Joining, name, failure.error), name);
            Failed::kernel(words, &failure, [&[first.body], &[]])
        })?;
        // The bodies in the order merged: the running solid holds those
        // up to the one being merged (the tool is none), whose faces a
        // failure names on it.
        let order: Vec<BodyId> = bodies.iter().map(|made| made.body).collect();
        for (held, made) in (1..).map(|i| &order[..i]).zip(rest) {
            merged = unite(
                boolean_key(Doing::Merging, merged.1, made.key),
                &merged.0,
                &made.solid,
            )
            .map_err(|failure| {
                let (into, other) = (self.body_name(first.body), self.body_name(made.body));
                let words = why::merging(into, other, failure.error);
                Failed::kernel(words, &failure, [held, &[made.body]])
            })?;
        }
        Ok(merged)
    }

    /// The bodies of `bodies` not in `excluded` that `tool`, filed under
    /// `tool_key`, touches, in their order; or those found before one
    /// couldn't be told and that one, and why. Excluded bodies aren't
    /// asked about: the panel lists them anyway, and one that can't be
    /// told would otherwise use up the budget again on every change,
    /// though taking it out is the way past it.
    pub(super) fn touched(
        &self,
        bodies: &[BodySolid],
        excluded: &[BodyId],
        (tool, tool_key): (&Solid, Key),
        cache: &mut Cache,
    ) -> Result<Vec<BodyId>, (Vec<BodyId>, Failed)> {
        let mut touched = Vec::new();
        for made in bodies.iter().filter(|made| !excluded.contains(&made.body)) {
            let touches = cache.touches(touches_key(made.key, tool_key), || {
                varde_kernel_compat::touches(&made.solid, tool, &self.tolerance, &self.touching)
                    .map_err(|failure| KernelFailure::new(failure, &self.tolerance))
            });
            match touches {
                Ok(true) => touched.push(made.body),
                Ok(false) => {}
                Err(failure) => {
                    let name = self.body_name(made.body);
                    let words = why::boolean(Doing::Touching, name, failure.error);
                    // Listed, so the panel offers to take it out.
                    touched.push(made.body);
                    return Err((
                        touched,
                        Failed::kernel(words, &failure, [&[made.body], &[]]),
                    ));
                }
            }
        }
        Ok(touched)
    }

    /// The name of `body`, as the Timeline's messages give it.
    pub(super) fn body_name(&self, body: BodyId) -> &str {
        body_name(self.document, body)
    }

    /// An extrude's tool solid and the key it's filed under: the
    /// regions swept over its span (worked out from the bodies made
    /// before it for through all) on its sketch's plane, its walls
    /// leaning by its taper if it has one (see `taper`).
    pub(super) fn extruded(
        &self,
        extrude: &Extrude,
        evaluation: &Evaluation,
        cache: &mut Cache,
    ) -> Result<(Arc<Solid>, Key), Failed> {
        let placement = self.placement()?;
        let frame = frame(&placement);
        let (from, to) = match extrude.span() {
            Some(span) => span,
            None => through_all(&frame, evaluation.bodies.iter().map(|made| &*made.solid))
                .ok_or("there's no body to go through")?,
        };
        let mut keyer = Keyer::new("extrude");
        keyer
            .number(self.feature.id.get())
            .value(&extrude.regions)
            .number(self.tolerance.fit().to_bits())
            .number(from.to_bits())
            .number(to.to_bits())
            .key(self.sketch.key)
            .placement(&placement);
        // An untapered extrude keeps the key it had before tapers, one
        // of zero too.
        let tapered = extrude.tapered();
        if let Some(taper) = tapered {
            keyer.bytes(b"taper").number(taper.value.to_bits());
        }
        let key = keyer.finish();
        let solid = cache.solid(key, || {
            let profile = self.profile()?;
            let (feature, budget) = (self.feature.id.get(), &Budget::DEFAULT);
            match tapered {
                None => varde_kernel_compat::extrude(
                    &profile,
                    &frame,
                    from,
                    to,
                    feature,
                    &self.tolerance,
                    budget,
                )
                .map_err(|failure| self.kernel_error(failure)),
                Some(taper) => taper::taperer()(
                    &profile,
                    &frame,
                    from,
                    to,
                    taper.value,
                    feature,
                    &self.tolerance,
                    budget,
                )
                .map_err(|error| self.taper_refused(error)),
            }
        })?;
        Ok((solid, key))
    }

    /// Where its sketch is, or why it isn't anywhere.
    pub(super) fn placement(&self) -> Result<Placement, String> {
        self.sketch
            .placement
            .ok_or_else(|| why::SKETCH_NOT_PLACED.to_owned())
    }

    /// The kernel profile of the regions, merged, in the sketch's
    /// coordinates.
    pub(super) fn profile(&self) -> Result<Profile, String> {
        self.sketch
            .profile(self.shape.regions(), self.tolerance.fit())
    }

    /// Why the kernel couldn't make the tool, in words, and its
    /// failure.
    pub(super) fn kernel_error(&self, failure: Failure) -> Failed {
        tool_failed(self.shape.making(), failure, &self.tolerance)
    }
}

/// Why the kernel couldn't make a tool by `making`, in words, and its
/// failure.
pub(super) fn tool_failed(making: Making, failure: Failure, tolerance: &Tolerance) -> Failed {
    let words = why::tool(making, failure.error, tolerance.fit() <= Tolerance::MIN_FIT);
    Failed::by_value(words, failure, tolerance)
}

/// The key of whether the tool filed under `tool` touches the body's
/// solid filed under `body`.
pub(super) fn touches_key(body: Key, tool: Key) -> Key {
    Keyer::new("touches").key(body).key(tool).finish()
}

/// The share of a body's volume below which a cut may have taken
/// nothing from it, for [`Run::cuts_nothing`] to make sure of: the
/// volumes are integrated, accurate to rounding but not exact.
pub(super) const UNCUT_SHARE: f64 = 1e-6;

/// The key of `doing` the tool filed under `tool` to the body's solid
/// filed under `body`: the body's new solid's.
pub(super) fn boolean_key(doing: Doing, body: Key, tool: Key) -> Key {
    Keyer::new("boolean")
        .bytes(doing.key().as_bytes())
        .key(body)
        .key(tool)
        .finish()
}

/// `op` on `a` and `b` at `tolerance`, filed in `cache` under `key`.
pub(super) fn cached_boolean(
    cache: &mut Cache,
    key: Key,
    (a, b): (&Solid, &Solid),
    op: Op,
    tolerance: &Tolerance,
) -> Result<Arc<Solid>, Arc<KernelFailure>> {
    cache.boolean(key, || {
        varde_kernel_compat::boolean(a, b, op, tolerance, &Budget::DEFAULT)
            .map_err(|failure| KernelFailure::new(failure, tolerance))
    })
}

/// The keys of the steps [`Run::merge`] works out for `bodies`, in the
/// order they were made, and the tool filed under `tool`, in both orders
/// it tries; none for fewer than two bodies.
pub(super) fn merge_keys(bodies: Vec<&BodySolid>, tool: Key) -> Vec<Key> {
    let Some((first, rest)) = bodies.split_first().filter(|(_, rest)| !rest.is_empty()) else {
        return Vec::new();
    };
    let mut keys = Vec::with_capacity(2 * bodies.len());
    let mut key = first.key;
    for made in rest {
        key = boolean_key(Doing::Merging, key, made.key);
        keys.push(key);
    }
    keys.push(boolean_key(Doing::Joining, key, tool));
    let mut key = boolean_key(Doing::Joining, first.key, tool);
    keys.push(key);
    for made in rest {
        key = boolean_key(Doing::Merging, key, made.key);
        keys.push(key);
    }
    keys
}

/// The span along `frame`'s normal that goes through all of `bodies`:
/// from below the lowest of their boxes' corners to past the highest, by
/// a margin ([`THROUGH_ALL_MARGIN`] of the extent and
/// [`THROUGH_ALL_MARGIN_MM`]), within [`MAX_COORD`]. `None` if there are
/// no bodies, or they're all out of reach.
pub(crate) fn through_all<'a>(
    frame: &Frame,
    bodies: impl IntoIterator<Item = &'a Solid>,
) -> Option<(f64, f64)> {
    let normal = frame.normal();
    let mut span: Option<(f64, f64)> = None;
    for bounds in bodies.into_iter().filter_map(Solid::bounds3) {
        for point in corners(&bounds) {
            let height = (point - frame.origin).dot(normal);
            span = Some(span.map_or((height, height), |(lo, hi)| {
                (lo.min(height), hi.max(height))
            }));
        }
    }
    let (lo, hi) = span?;
    let margin = (hi - lo) * THROUGH_ALL_MARGIN + THROUGH_ALL_MARGIN_MM;
    let max = f64::from(MAX_COORD);
    let (from, to) = ((lo - margin).max(-max), (hi + margin).min(max));
    (from < to).then_some((from, to))
}
