//! Evaluating the feature history into the bodies' solids.
//!
//! Features run in the document's order. A sketch gives its profiles
//! ([`Sketch::profiles`]) and where it is: an origin plane's placement,
//! or, for a sketch on a face, the placement of the face's plane
//! ([`Placement::on_plane`]) on the body as the features before the
//! sketch leave it (see [`place_on_face`]); one that isn't placed fails,
//! and so does every extrude or revolve made from it. An extrude or a
//! revolve finds its regions again in its sketch's profiles
//! ([`Profiles::resolve`]; one that's gone is "region not found"), merges
//! them ([`Profiles::merge`]) and turns the loops into a kernel profile
//! ([`profile`]). An extrude sweeps it
//! with [`varde_kernel::extrude`] on the sketch's plane, over
//! [`Extrude::span`]; a revolve finds its axis in the sketch
//! ([`axis_line`]; a line that's gone is "axis not found"), moves the
//! profile into the axis's frame ([`axis_frame`]) and turns it with
//! [`varde_kernel::revolve`] over [`Revolve::span`]. Both run within the
//! document's tolerance and the default budget, their faces named by
//! their feature id: the tool solid. A new body gets it. A join, cut or
//! intersect finds the bodies made before it,
//! less those it excludes, that the tool touches
//! ([`varde_kernel::touches`]). A cut or intersect replaces each of those
//! by its difference from or intersection with the tool
//! ([`varde_kernel::boolean`], the body first), one at a time. A join
//! touching one body replaces it by its union with the tool; one touching
//! several merges them: the first made gets the union of them all and the
//! tool, and the others are *consumed*, left out of
//! [`Evaluation::bodies`] and listed in [`Evaluation::merged`] (see
//! `Run::merge` for the order). A through-all extent's
//! span is worked out from the bodies made before it ([`through_all`]).
//! A combine works on the bodies as the features before it leave it: the
//! target's solid united with, less or intersected with each tool's, the
//! tools then consumed into the target as a join's merged bodies are,
//! unless it keeps them (see `combine::evaluate`); one naming a body with
//! no solid of its own (consumed before, or its maker failed) fails.
//! A join, cut, intersect or combine that would leave nothing of a body fails
//! (bodies are the document's, so an emptied one would stay listed with
//! no geometry): no body in an [`Evaluation`] is empty.
//! A feature that fails records why, in words for the Timeline
//! (`src/message.rs`), and, where the kernel failed, what its failure's
//! evidence shows of where ([`FeatureFailure::geometry`], see
//! `src/error_geometry.rs`; the operand faces are resolved once the model
//! is drawn), and changes no body; the later ones still run.
//!
//! Every result goes through the [`Cache`], keyed by what it depends on,
//! so only what an edit changes runs again.
//!
//! [`Sketch::profiles`]: varde_sketch::Sketch::profiles

use std::collections::BTreeSet;
use std::sync::Arc;

use glam::DVec2;
use varde_document::{
    AxisLine, BodyId, Document, Extrude, FaceRef, Feature, FeatureId, FeatureKind, MAX_COORD,
    Operation, Placement, Plane, Revolve, Sketch,
};
use varde_kernel::mesh::Form;
use varde_kernel::patch::Conic2;
use varde_kernel::{Budget, Failure, Frame, Loop, Op, Profile, Segment, Solid, Sweep, Tolerance};
use varde_sketch::{Curve, Id, Profiles, RegionRef, TooComplex};

use crate::cache::{Cache, Key, Keyer};
use crate::error_geometry::{ErrorGeometry, FeatureFailure, KernelFailure};
use crate::message::{self, Doing, Making};
use crate::picking::region_form;
use crate::profile::profile;

mod combine;

/// What the history gives: the solids of the bodies, and the features
/// that failed.
#[derive(Debug, Clone, Default)]
pub struct Evaluation {
    /// Each body that has a solid, in the order the features made them.
    /// None is empty: a feature that would empty one fails. A body a
    /// join merged into another, or a combine used as a tool without
    /// keeping it, isn't here (see [`Evaluation::merged`]).
    pub bodies: Vec<BodySolid>,
    /// Each body a join merged into another or a combine consumed as a
    /// tool into its target (*consumed*), and the body in
    /// [`Evaluation::bodies`] that now holds it, in the document's order
    /// of the consumed bodies. A body merged into one that a later join
    /// or combine consumed in turn names the later one, so every entry names a body
    /// in `bodies`, and no consumed body is in `bodies`.
    pub merged: Vec<(BodyId, BodyId)>,
    /// The features that failed, why and where, in the document's
    /// order.
    pub failed: Vec<FeatureFailure>,
    /// Each join, cut or intersect that got as far as its tool solid,
    /// with the bodies made before it and not taken out of it that the
    /// tool touches, in the order they were made; in the document's
    /// order.
    /// One failing while finding them lists those found before and the
    /// body it couldn't tell, which taking out gets past.
    pub touched: Vec<(FeatureId, Vec<BodyId>)>,
    /// Each sketch on a face that was placed, and where, in the
    /// document's order: on the plane of its face on the body as the
    /// features before it leave it, by [`Placement::on_plane`]. A
    /// sketch on an origin plane isn't listed: its placement is the
    /// plane's. A sketch on a face that isn't listed failed, and is
    /// drawn nowhere.
    pub placements: Vec<(FeatureId, Placement)>,
}

impl Evaluation {
    /// The body in [`Evaluation::bodies`] holding `body`'s solid: `body`
    /// itself, or the body it was merged into. `None` for a body with no
    /// solid (its maker failed, or it isn't the document's). Whatever
    /// lives on a consumed body is looked for here: a sketch on one of
    /// its faces is placed on the holder's solid, where the face lives
    /// on.
    pub fn holder(&self, body: BodyId) -> Option<BodyId> {
        let body = (self.merged.iter())
            .find(|(consumed, _)| *consumed == body)
            .map_or(body, |&(_, into)| into);
        self.bodies
            .iter()
            .any(|made| made.body == body)
            .then_some(body)
    }
}

/// Notes in `merged`, a list like [`Evaluation::merged`], that a join
/// merged `bodies` (in the order they were made) into the first, the
/// *holder*: each of the others is consumed into it, and so are the bodies
/// merged into those before. Nothing for fewer than two. How regen keeps
/// `merged`, and how the bodies the joins touched replay it.
pub fn note_merge(merged: &mut Vec<(BodyId, BodyId)>, bodies: &[BodyId]) {
    let Some((&holder, consumed)) = bodies.split_first() else {
        return;
    };
    for (_, held_in) in merged.iter_mut() {
        if consumed.contains(held_in) {
            *held_in = holder;
        }
    }
    merged.extend(consumed.iter().map(|&body| (body, holder)));
}

/// Why a feature fails, as the history carries it: in words, and what
/// to draw of where, made from the kernel's failure where it's the
/// kernel's ([`FeatureFailure`] without the feature).
#[derive(Debug, Clone)]
pub(crate) struct Failed {
    pub(crate) message: String,
    pub(crate) geometry: Option<Arc<ErrorGeometry>>,
}

impl Failed {
    /// The kernel's `failure`, worded as `message`, of the bodies its
    /// operands are (`[a, b]`, see [`KernelFailure::geometry`]).
    pub(crate) fn kernel(
        message: String,
        failure: &KernelFailure,
        operands: [Option<BodyId>; 2],
    ) -> Failed {
        Failed {
            message,
            geometry: failure.geometry(operands),
        }
    }

    /// The failure of `feature`.
    pub(crate) fn of(self, feature: FeatureId) -> FeatureFailure {
        FeatureFailure {
            feature,
            message: self.message,
            geometry: self.geometry,
        }
    }
}

impl From<String> for Failed {
    fn from(message: String) -> Failed {
        Failed {
            message,
            geometry: None,
        }
    }
}

impl From<&str> for Failed {
    fn from(message: &str) -> Failed {
        Failed::from(message.to_owned())
    }
}

/// A body's solid.
#[derive(Debug, Clone)]
pub struct BodySolid {
    pub body: BodyId,
    pub solid: Arc<Solid>,
    /// What the solid was filed under, for drawing it.
    pub(crate) key: Key,
}

/// A sketch evaluated: the sketch, its profiles, where it is (`None` for
/// one that isn't placed: on a face that wasn't found, isn't flat or
/// whose body is gone), and its key, which is of the sketch alone.
struct SketchOutput<'a> {
    id: FeatureId,
    sketch: &'a Sketch,
    profiles: Arc<Result<Profiles, TooComplex>>,
    placement: Option<Placement>,
    key: Key,
}

/// The margin a through-all span gets past the bodies' extent at each
/// end: this share of the extent, and [`THROUGH_ALL_MARGIN_MM`].
const THROUGH_ALL_MARGIN: f64 = 0.01;
const THROUGH_ALL_MARGIN_MM: f64 = 1.0;

/// Evaluates the history of `document`, see the module's docs.
pub fn evaluate(document: &Document, cache: &mut Cache) -> Evaluation {
    evaluate_within(document, cache, Budget::DEFAULT)
}

/// [`evaluate`], with `touching` the budget of each
/// [`varde_kernel::touches`], which tests make small to see it fail.
pub(crate) fn evaluate_within(
    document: &Document,
    cache: &mut Cache,
    touching: Budget,
) -> Evaluation {
    let tolerance = document.tolerance();
    let mut sketches: Vec<SketchOutput> = Vec::new();
    let mut evaluation = Evaluation::default();
    for feature in document.features() {
        match &feature.kind {
            FeatureKind::Sketch { plane, sketch } => {
                // Its profiles are 2D: they don't depend on where it is.
                let key = Keyer::new("sketch").value(sketch).finish();
                let profiles = cache.profiles(key, || sketch.profiles());
                let placement = match plane {
                    Plane::Origin(origin) => Some(origin.placement()),
                    Plane::Face(face) => match place_on_face(face, &evaluation, cache) {
                        Ok(placement) => {
                            evaluation.placements.push((feature.id, placement));
                            Some(placement)
                        }
                        Err(error) => {
                            (evaluation.failed).push(Failed::from(error).of(feature.id));
                            None
                        }
                    },
                };
                sketches.push(SketchOutput {
                    id: feature.id,
                    sketch,
                    profiles,
                    placement,
                    key,
                });
            }
            FeatureKind::Extrude(_) | FeatureKind::Revolve(_) => {
                let (sketch, shape, operation) = match &feature.kind {
                    FeatureKind::Extrude(extrude) => {
                        (extrude.sketch, Shape::Extrude(extrude), &extrude.operation)
                    }
                    FeatureKind::Revolve(revolve) => {
                        (revolve.sketch, Shape::Revolve(revolve), &revolve.operation)
                    }
                    FeatureKind::Sketch { .. } | FeatureKind::Combine(_) => {
                        unreachable!("matched apart")
                    }
                };
                // A checked document's extrude or revolve names a sketch
                // before it.
                let Some(sketch) = sketches.iter().find(|s| s.id == sketch) else {
                    let failed = Failed::from("its sketch isn't there");
                    evaluation.failed.push(failed.of(feature.id));
                    continue;
                };
                let run = Run {
                    document,
                    feature,
                    shape,
                    operation,
                    sketch,
                    tolerance,
                    touching,
                };
                if let Err(failed) = run.evaluate(&mut evaluation, cache) {
                    evaluation.failed.push(failed.of(feature.id));
                }
            }
            FeatureKind::Combine(combine) => {
                if let Err(failed) =
                    combine::evaluate(document, combine, &tolerance, &mut evaluation, cache)
                {
                    evaluation.failed.push(failed.of(feature.id));
                }
            }
        }
    }
    // In the document's order (bodies are kept in increasing id order).
    evaluation.merged.sort_by_key(|&(consumed, _)| consumed);
    // A new body's tool is never empty, a union of two solids that
    // aren't isn't, and a cut or intersect that would empty one fails.
    debug_assert!(
        evaluation.bodies.iter().all(|made| !made.solid.is_empty()),
        "a body is never empty"
    );
    debug_assert!(
        evaluation.merged.iter().all(|(consumed, into)| {
            let held = |body: &BodyId| evaluation.bodies.iter().any(|made| made.body == *body);
            !held(consumed) && held(into)
        }),
        "a consumed body has no solid and names one that has"
    );
    evaluation
}

/// What a feature making a tool solid makes it with.
#[derive(Clone, Copy)]
enum Shape<'a> {
    Extrude(&'a Extrude),
    Revolve(&'a Revolve),
}

impl<'a> Shape<'a> {
    /// Its regions.
    fn regions(self) -> &'a [RegionRef] {
        match self {
            Shape::Extrude(extrude) => &extrude.regions,
            Shape::Revolve(revolve) => &revolve.regions,
        }
    }

    /// What it makes, for the messages.
    fn making(self) -> Making {
        match self {
            Shape::Extrude(_) => Making::Extrude,
            Shape::Revolve(_) => Making::Revolve,
        }
    }
}

/// An extrude or revolve being evaluated.
struct Run<'a> {
    document: &'a Document,
    feature: &'a Feature,
    shape: Shape<'a>,
    operation: &'a Operation,
    sketch: &'a SketchOutput<'a>,
    tolerance: Tolerance,
    /// The budget of each [`varde_kernel::touches`].
    touching: Budget,
}

impl Run<'_> {
    /// Adds the body it makes to `evaluation`, or changes those it works
    /// on, given those made before it; or why it fails, changing none.
    fn evaluate(&self, evaluation: &mut Evaluation, cache: &mut Cache) -> Result<(), Failed> {
        // The tool depends on the regions and where it runs, not on what
        // it's then used for, so changing the operation or its bodies
        // finds it again.
        let (tool, tool_key) = match self.shape {
            Shape::Extrude(extrude) => self.extruded(extrude, evaluation, cache)?,
            Shape::Revolve(revolve) => self.revolved(revolve, cache)?,
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
        for made in evaluation
            .bodies
            .iter()
            .filter(|m| targets.contains(&m.body))
        {
            let name = self.body_name(made.body);
            let fails = |message: String| match targets.len() {
                1 => message,
                _ => message::leave_out(message, name),
            };
            let key = boolean_key(doing, made.key, tool_key);
            let solid = cache
                .boolean(key, || {
                    varde_kernel::boolean(&made.solid, &tool, op, &self.tolerance, &Budget::DEFAULT)
                        .map_err(|failure| KernelFailure::new(failure, &self.tolerance))
                })
                .map_err(|failure| {
                    let words = fails(message::boolean(doing, name, failure.error));
                    Failed::kernel(words, &failure, [Some(made.body), None])
                })?;
            // The cached empty result stays: its key is right, and the
            // check is cheap to make again. A cut's message says to
            // untick it already.
            if solid.is_empty() {
                let emptied = message::emptied(doing, name);
                return Err(match doing {
                    Doing::Cutting => emptied,
                    _ => fails(emptied),
                }
                .into());
            }
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
        Ok(())
    }

    /// Merges the bodies `targets` (two or more, in the order they were
    /// made) and the tool, filed under `tool_key`, into the first: it gets
    /// the union of them all, and the others are consumed (taken out of
    /// `evaluation`'s bodies and listed in its `merged`, which entries
    /// naming them are moved on from). Or why it fails, changing nothing.
    fn merge_into_first(
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
            return Err(message::emptied(Doing::Joining, self.body_name(into)).into());
        }
        note_merge(&mut evaluation.merged, &merging);
        (evaluation.bodies).retain(|made| !consumed.contains(&made.body));
        if let Some(made) = evaluation.bodies.iter_mut().find(|m| m.body == into) {
            *made = BodySolid {
                body: into,
                solid,
                key,
            };
        }
        Ok(())
    }

    /// The union of `bodies` (two or more, in the order they were made)
    /// and the tool filed under `tool_key`, and the key it's filed under;
    /// or why it fails. Every step is a [`varde_kernel::boolean`] union,
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
    /// fails too, its error is given.
    fn merge(
        &self,
        bodies: &[&BodySolid],
        (tool, tool_key): (&Solid, Key),
        cache: &mut Cache,
    ) -> Result<(Arc<Solid>, Key), Failed> {
        let (first, rest) = bodies.split_first().expect("two or more bodies are merged");
        let mut unite = |key: Key, a: &Solid, b: &Solid| {
            let solid = cache.boolean(key, || {
                varde_kernel::boolean(a, b, Op::Union, &self.tolerance, &Budget::DEFAULT)
                    .map_err(|failure| KernelFailure::new(failure, &self.tolerance))
            });
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
            let words =
                message::leave_out(message::boolean(Doing::Joining, name, failure.error), name);
            Failed::kernel(words, &failure, [Some(first.body), None])
        })?;
        for made in rest {
            merged = unite(
                boolean_key(Doing::Merging, merged.1, made.key),
                &merged.0,
                &made.solid,
            )
            .map_err(|failure| {
                let (into, other) = (self.body_name(first.body), self.body_name(made.body));
                let words = message::merging(into, other, failure.error);
                Failed::kernel(words, &failure, [Some(first.body), Some(made.body)])
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
    fn touched(
        &self,
        bodies: &[BodySolid],
        excluded: &[BodyId],
        (tool, tool_key): (&Solid, Key),
        cache: &mut Cache,
    ) -> Result<Vec<BodyId>, (Vec<BodyId>, Failed)> {
        let mut touched = Vec::new();
        for made in bodies.iter().filter(|made| !excluded.contains(&made.body)) {
            let touches = cache.touches(touches_key(made.key, tool_key), || {
                varde_kernel::touches(&made.solid, tool, &self.tolerance, &self.touching)
                    .map_err(|failure| KernelFailure::new(failure, &self.tolerance))
            });
            match touches {
                Ok(true) => touched.push(made.body),
                Ok(false) => {}
                Err(failure) => {
                    let name = self.body_name(made.body);
                    let words = message::boolean(Doing::Touching, name, failure.error);
                    // Listed, so the panel offers to take it out.
                    touched.push(made.body);
                    return Err((
                        touched,
                        Failed::kernel(words, &failure, [Some(made.body), None]),
                    ));
                }
            }
        }
        Ok(touched)
    }

    /// The name of `body`, as the Timeline's messages give it.
    fn body_name(&self, body: BodyId) -> &str {
        self.document
            .body(body)
            .map_or("a body", |body| body.name.as_str())
    }

    /// An extrude's tool solid and the key it's filed under: the
    /// regions swept over its span (worked out from the bodies made
    /// before it for through all) on its sketch's plane.
    fn extruded(
        &self,
        extrude: &Extrude,
        evaluation: &Evaluation,
        cache: &mut Cache,
    ) -> Result<(Arc<Solid>, Key), Failed> {
        let placement = self.placement()?;
        let frame = Frame {
            origin: placement.origin,
            x: placement.x,
            y: placement.y,
        };
        let (from, to) = match extrude.span() {
            Some(span) => span,
            None => through_all(&frame, evaluation.bodies.iter().map(|made| &*made.solid))
                .ok_or("there's no body to go through")?,
        };
        let key = Keyer::new("extrude")
            .number(self.feature.id.get())
            .value(&extrude.regions)
            .number(self.tolerance.fit().to_bits())
            .number(from.to_bits())
            .number(to.to_bits())
            .key(self.sketch.key)
            .placement(&placement)
            .finish();
        let solid = cache.solid(key, || {
            let profile = self.profile()?;
            varde_kernel::extrude(
                &profile,
                &frame,
                from,
                to,
                self.feature.id.get(),
                &self.tolerance,
                &Budget::DEFAULT,
            )
            .map_err(|failure| self.kernel_error(failure))
        })?;
        Ok((solid, key))
    }

    /// A revolve's tool solid and the key it's filed under: the regions
    /// turned about its axis over its span (see [`axis_frame`]).
    fn revolved(&self, revolve: &Revolve, cache: &mut Cache) -> Result<(Arc<Solid>, Key), Failed> {
        let span = revolve.span();
        let placement = self.placement()?;
        // The axis is the sketch's, so its key holds where the axis line
        // is.
        let key = Keyer::new("revolve")
            .number(self.feature.id.get())
            .value(&revolve.regions)
            .value(&revolve.axis)
            .number(self.tolerance.fit().to_bits())
            .value(&span)
            .key(self.sketch.key)
            .placement(&placement)
            .finish();
        let solid = cache.solid(key, || {
            let profile = self.profile()?;
            let axis = axis_line(self.sketch.sketch, revolve.axis)?;
            let (profile, frame, same_way) = axis_frame(&profile, &axis, &placement)?;
            let sweep = match span {
                None => Sweep::Full,
                Some((from, to)) if same_way => Sweep::Part { from, to },
                Some((from, to)) => Sweep::Part {
                    from: -to,
                    to: -from,
                },
            };
            varde_kernel::revolve(
                &profile,
                &frame,
                sweep,
                self.feature.id.get(),
                &self.tolerance,
                &Budget::DEFAULT,
            )
            .map_err(|failure| self.kernel_error(failure))
        })?;
        Ok((solid, key))
    }

    /// Where its sketch is, or why it isn't anywhere.
    fn placement(&self) -> Result<Placement, String> {
        self.sketch
            .placement
            .ok_or_else(|| message::SKETCH_NOT_PLACED.to_owned())
    }

    /// The kernel profile of the regions, merged, in the sketch's
    /// coordinates.
    fn profile(&self) -> Result<Profile, String> {
        let profiles = match &*self.sketch.profiles {
            Ok(profiles) => profiles,
            Err(e) => return Err(format!("its sketch is {e}")),
        };
        let regions = profiles
            .resolve(self.shape.regions())
            .into_iter()
            .collect::<Option<Vec<usize>>>()
            .ok_or("region not found")?;
        let loops = profiles.merge(&regions).map_err(|e| e.to_string())?;
        profile(self.sketch.sketch, profiles, &loops, self.tolerance.fit())
            .map_err(|e| e.to_string())
    }

    /// Why the kernel couldn't make the tool, in words, and its
    /// failure.
    fn kernel_error(&self, failure: Failure) -> Failed {
        let words = message::tool(
            self.shape.making(),
            failure.error,
            self.tolerance.fit() <= Tolerance::MIN_FIT,
        );
        let failure = KernelFailure::new(failure, &self.tolerance);
        Failed::kernel(words, &failure, [None, None])
    }
}

/// Where a sketch on `face` is, given the bodies the features before it
/// made (`evaluation`), or why it isn't anywhere. The face's body is
/// looked for through [`Evaluation::holder`] (a body a join consumed
/// lives on in the one holding it; one with no solid is "gone"); the
/// face is found on its solid's [`Topology`](varde_kernel::Topology) by
/// name or alias, the nearest to its point among several
/// ([`Topology::face`](varde_kernel::Topology::face); none is "wasn't
/// found"); its form must be a plane ("isn't flat"), whose `n` and `d`
/// give the placement by [`Placement::on_plane`], the same rule and the
/// same bits as the app's from the picking tables' summary of the face
/// (both take the region's form by [`region_form`]). A placement that
/// isn't [`Placement::valid`] (its origin past the coordinate limit) is
/// refused. Cached by the solid's key and the face's name and point,
/// so the topology is worked out again only when the solid changes.
pub(crate) fn place_on_face(
    face: &FaceRef,
    evaluation: &Evaluation,
    cache: &mut Cache,
) -> Result<Placement, &'static str> {
    let holder = evaluation.holder(face.body);
    let made = (evaluation.bodies.iter())
        .find(|made| Some(made.body) == holder)
        .ok_or(message::FACE_BODY_GONE)?;
    let near = face.near.to_array().map(f64::to_bits);
    let key = Keyer::new("placement")
        .key(made.key)
        .value(&face.key)
        .number(near[0])
        .number(near[1])
        .number(near[2])
        .finish();
    cache.placement(key, || {
        let solid = &made.solid;
        let topology = solid.topology();
        let region =
            (topology.face(solid, &face.key, face.near)).map_err(|_| message::FACE_NOT_FOUND)?;
        let Form::Plane { n, d } = *region_form(solid, &topology.regions()[region as usize]) else {
            return Err(message::FACE_NOT_FLAT);
        };
        let placement = Placement::on_plane(n, d).ok_or(message::FACE_NOT_FLAT)?;
        if placement.valid() {
            Ok(placement)
        } else {
            Err(message::FACE_TOO_FAR)
        }
    })
}

/// A revolve's axis in its sketch's coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Axis {
    /// A point on it: the line's start, or the sketch's origin.
    pub(crate) at: DVec2,
    /// The way it points, not zero (not unit).
    pub(crate) along: DVec2,
    /// The line it is, if it's one of the sketch's curves.
    pub(crate) curve: Option<Id>,
}

/// The axis `axis` names in `sketch`, or why there's none: the curve it
/// names isn't there or isn't a line any more ("axis not found"), or
/// the line has no length.
pub(crate) fn axis_line(sketch: &Sketch, axis: AxisLine) -> Result<Axis, String> {
    let (at, along, curve) = match axis {
        AxisLine::SketchX => (DVec2::ZERO, DVec2::X, None),
        AxisLine::SketchY => (DVec2::ZERO, DVec2::Y, None),
        AxisLine::Curve(id) => {
            let not_found = || "axis not found".to_owned();
            let entry = sketch.curve(id).ok_or_else(not_found)?;
            let Curve::Line { start, end } = entry.curve else {
                return Err(not_found());
            };
            let at = |point| (sketch.point(point).map(|point| point.at)).ok_or_else(not_found);
            let (start, end) = (at(start)?, at(end)?);
            // Both within the coordinate limit, so the difference is
            // finite.
            (start, end - start, Some(id))
        }
    };
    if along == DVec2::ZERO {
        return Err("its axis line has no length".to_owned());
    }
    Ok(Axis { at, along, curve })
}

/// `profile`, in its sketch's coordinates, moved into the frame the
/// kernel revolves it on about `axis`, and that frame, on the sketch
/// placed at `placement`; and whether the kernel's angles turn as the
/// revolve's do (otherwise they're the other way).
///
/// The frame's origin is the axis's point, its `y` along the axis and its
/// `x` square to it in the sketch's plane, toward the profile: to the
/// side of the profile's point farthest from the axis (of its segments'
/// ends and middles). `y` points along or against the axis so that `x ×
/// y` is the sketch's normal: the map is a rigid motion of the plane,
/// keeping the loops' turning. The kernel turns `x` toward `x × y`,
/// right-handed about `−y`, and the revolve right-handed about the
/// axis's direction, so they agree where `y` points against the axis.
///
/// The ends of the segments of the axis line itself, and every segment
/// end at the same point, are put at `x = 0` exactly (and the axis
/// segments' control points); the kernel puts the others within its
/// resolution there, and refuses any reaching across. Fails if the axis
/// has no direction a float can hold, or the profile moved isn't within
/// the coordinate limit.
pub(crate) fn axis_frame(
    profile: &Profile,
    axis: &Axis,
    placement: &Placement,
) -> Result<(Profile, Frame, bool), String> {
    let along = (axis.along.try_normalize()).ok_or("its axis line has no length")?;
    let left = along.perp();
    let segments = || profile.loops.iter().flat_map(|lp| lp.segments.iter());
    // The side of the farthest point; the first of equals.
    let mut far = 0.0_f64;
    for segment in segments() {
        let c = &segment.conic;
        // The conic's middle; its weight is positive.
        let middle = (c.p0 + c.c * (2.0 * c.w) + c.p1) / (2.0 + 2.0 * c.w);
        for p in [c.p0, middle, c.p1] {
            let distance = (p - axis.at).dot(left);
            if distance.abs() > far.abs() {
                far = distance;
            }
        }
    }
    let same_way = far >= 0.0;
    let (x, y) = if same_way {
        (left, -along)
    } else {
        (-left, along)
    };
    let map = |p: DVec2| {
        let q = p - axis.at;
        DVec2::new(q.dot(x), q.dot(y))
    };
    let on_axis =
        |segment: &Segment| (axis.curve).is_some_and(|id| segment.curve == u64::from(id.get()));
    // The ends of the axis line's own segments, as mapped.
    let bits = |p: DVec2| (p.x.to_bits(), p.y.to_bits());
    let ends: BTreeSet<(u64, u64)> = segments()
        .filter(|segment| on_axis(segment))
        .flat_map(|segment| [map(segment.conic.p0), map(segment.conic.p1)])
        .map(bits)
        .collect();
    let snap = |p: DVec2| {
        let p = map(p);
        if ends.contains(&bits(p)) {
            DVec2::new(0.0, p.y)
        } else {
            p
        }
    };
    let max = f64::from(MAX_COORD);
    let within = |p: DVec2| p.is_finite() && p.abs().max_element() <= max;
    let mut moved = Profile::default();
    for lp in &profile.loops {
        let mut segments = Vec::with_capacity(lp.segments.len());
        for segment in &lp.segments {
            let c = segment.conic;
            let mut conic = Conic2 {
                p0: snap(c.p0),
                c: map(c.c),
                w: c.w,
                p1: snap(c.p1),
            };
            if on_axis(segment) {
                conic.c.x = 0.0;
            }
            if ![conic.p0, conic.c, conic.p1].into_iter().all(within) {
                return Err("its regions are too far from the axis to revolve".to_owned());
            }
            segments.push(Segment {
                conic,
                curve: segment.curve,
            });
        }
        moved.loops.push(Loop { segments });
    }
    let world = |v: DVec2| placement.x * v.x + placement.y * v.y;
    let frame = Frame {
        origin: placement.to_world(axis.at),
        x: world(x),
        y: world(y),
    };
    Ok((moved, frame, same_way))
}

/// The key of whether the tool filed under `tool` touches the body's
/// solid filed under `body`.
fn touches_key(body: Key, tool: Key) -> Key {
    Keyer::new("touches").key(body).key(tool).finish()
}

/// The key of `doing` the tool filed under `tool` to the body's solid
/// filed under `body`: the body's new solid's.
fn boolean_key(doing: Doing, body: Key, tool: Key) -> Key {
    Keyer::new("boolean")
        .bytes(doing.name().as_bytes())
        .key(body)
        .key(tool)
        .finish()
}

/// The keys of the steps [`Run::merge`] works out for `bodies`, in the
/// order they were made, and the tool filed under `tool`, in both orders
/// it tries; none for fewer than two bodies.
fn merge_keys(bodies: Vec<&BodySolid>, tool: Key) -> Vec<Key> {
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
        for corner in 0..8 {
            let pick = |bit: u32, min: f64, max: f64| if corner & bit == 0 { min } else { max };
            let point = glam::DVec3::new(
                pick(1, bounds.min.x, bounds.max.x),
                pick(2, bounds.min.y, bounds.max.y),
                pick(4, bounds.min.z, bounds.max.z),
            );
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

#[cfg(test)]
pub(crate) mod tests;
