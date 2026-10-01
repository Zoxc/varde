//! Evaluating the feature history into the bodies' solids.
//!
//! Features run in the document's order. A sketch gives its profiles
//! ([`Sketch::profiles`]). An extrude finds its regions again in its
//! sketch's profiles ([`Profiles::resolve`]; one that's gone is "region
//! not found"), merges them ([`Profiles::merge`]), turns the loops into a
//! kernel profile ([`profile`]) and sweeps it
//! with [`varde_kernel::extrude`] on the sketch's plane, over
//! [`Extrude::span`], within the document's tolerance and the default
//! budget, its faces named by its feature id: the tool solid. A new body
//! gets it. A join, cut or intersect finds the bodies made before it,
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
//! A join, cut or intersect that would leave nothing of a body fails
//! (bodies are the document's, so an emptied one would stay listed with
//! no geometry): no body in an [`Evaluation`] is empty.
//! A feature that fails records why, in words for the Timeline
//! (`src/message.rs`), and changes no body; the later ones
//! still run.
//!
//! Every result goes through the [`Cache`], keyed by what it depends on,
//! so only what an edit changes runs again.
//!
//! [`Sketch::profiles`]: varde_sketch::Sketch::profiles

use std::sync::Arc;

use varde_document::{
    BodyId, Document, Extrude, Feature, FeatureId, FeatureKind, MAX_COORD, Operation, Plane, Sketch,
};
use varde_kernel::{Budget, Frame, Op, Solid, Tolerance};
use varde_sketch::{Profiles, TooComplex};

use crate::cache::{Cache, Key, Keyer};
use crate::message::{self, Doing};
use crate::profile::profile;

/// What the history gives: the solids of the bodies, and the features
/// that failed.
#[derive(Debug, Clone, Default)]
pub struct Evaluation {
    /// Each body that has a solid, in the order the features made them.
    /// None is empty: a feature that would empty one fails. A body a
    /// join merged into another isn't here (see [`Evaluation::merged`]).
    pub bodies: Vec<BodySolid>,
    /// Each body a join merged into another (*consumed*), and the body in
    /// [`Evaluation::bodies`] that now holds it, in the document's order
    /// of the consumed bodies. A body merged into one that a later join
    /// merged in turn names the later one, so every entry names a body
    /// in `bodies`, and no consumed body is in `bodies`.
    pub merged: Vec<(BodyId, BodyId)>,
    /// The features that failed and why, in the document's order.
    pub failed: Vec<(FeatureId, String)>,
    /// Each join, cut or intersect that got as far as its tool solid,
    /// with the bodies made before it and not taken out of it that the
    /// tool touches, in the order they were made; in the document's
    /// order.
    /// One failing while finding them lists those found before and the
    /// body it couldn't tell, which taking out gets past.
    pub touched: Vec<(FeatureId, Vec<BodyId>)>,
}

impl Evaluation {
    /// The body in [`Evaluation::bodies`] holding `body`'s solid: `body`
    /// itself, or the body it was merged into. `None` for a body with no
    /// solid (its maker failed, or it isn't the document's). Whatever
    /// lives on a consumed body (a sketch on one of its faces, once
    /// sketches can be placed on faces) is looked for here.
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

/// A body's solid.
#[derive(Debug, Clone)]
pub struct BodySolid {
    pub body: BodyId,
    pub solid: Arc<Solid>,
    /// What the solid was filed under, for drawing it.
    pub(crate) key: Key,
}

/// A sketch evaluated: the sketch, its profiles, where it is, and its
/// key.
struct SketchOutput<'a> {
    id: FeatureId,
    sketch: &'a Sketch,
    profiles: Arc<Result<Profiles, TooComplex>>,
    plane: Plane,
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
                let key = Keyer::new("sketch").value(plane).value(sketch).finish();
                let profiles = cache.profiles(key, || sketch.profiles());
                sketches.push(SketchOutput {
                    id: feature.id,
                    sketch,
                    profiles,
                    plane: *plane,
                    key,
                });
            }
            FeatureKind::Extrude(extrude) => {
                // A checked document's extrude names a sketch before it.
                let Some(sketch) = sketches.iter().find(|s| s.id == extrude.sketch) else {
                    evaluation
                        .failed
                        .push((feature.id, "its sketch isn't there".to_owned()));
                    continue;
                };
                let run = Run {
                    document,
                    feature,
                    extrude,
                    sketch,
                    tolerance,
                    touching,
                };
                if let Err(error) = run.evaluate(&mut evaluation, cache) {
                    evaluation.failed.push((feature.id, error));
                }
            }
        }
    }
    // In the document's order (bodies are kept in increasing id order).
    evaluation.merged.sort_by_key(|&(consumed, _)| consumed);
    // A new body's extrude is never empty, a union of two solids that
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

/// An extrude being evaluated.
struct Run<'a> {
    document: &'a Document,
    feature: &'a Feature,
    extrude: &'a Extrude,
    sketch: &'a SketchOutput<'a>,
    tolerance: Tolerance,
    /// The budget of each [`varde_kernel::touches`].
    touching: Budget,
}

impl Run<'_> {
    /// Adds the body it makes to `evaluation`, or changes those it works
    /// on, given those made before it; or why it fails, changing none.
    fn evaluate(&self, evaluation: &mut Evaluation, cache: &mut Cache) -> Result<(), String> {
        let placement = self.sketch.plane.placement();
        let frame = Frame {
            origin: placement.origin,
            x: placement.x,
            y: placement.y,
        };
        let span = match self.extrude.span() {
            Some(span) => span,
            None => through_all(&frame, evaluation.bodies.iter().map(|made| &*made.solid))
                .ok_or("there's no body to go through")?,
        };
        // The tool depends on the regions and where it runs, not on what
        // it's then used for, so changing the operation or its bodies
        // finds it again.
        let tool_key = Keyer::new("extrude")
            .number(self.feature.id.get())
            .value(&self.extrude.regions)
            .number(self.tolerance.fit().to_bits())
            .number(span.0.to_bits())
            .number(span.1.to_bits())
            .key(self.sketch.key)
            .finish();
        let tool = cache.solid(tool_key, || self.solid(&frame, span))?;
        let (op, doing) = match &self.extrude.operation {
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
        let excluded = self.extrude.operation.excluded();
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
            .to_owned());
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
                })
                .map_err(|error| fails(message::boolean(doing, name, error)))?;
            // The cached empty result stays: its key is right, and the
            // check is cheap to make again. A cut's message says to
            // untick it already.
            if solid.is_empty() {
                let emptied = message::emptied(doing, name);
                return Err(match doing {
                    Doing::Cutting => emptied,
                    _ => fails(emptied),
                });
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
    ) -> Result<(), String> {
        let bodies: Vec<&BodySolid> = (evaluation.bodies.iter())
            .filter(|made| targets.contains(&made.body))
            .collect();
        let merging: Vec<BodyId> = bodies.iter().map(|made| made.body).collect();
        let (into, consumed) = (merging[0], &merging[1..]);
        let (solid, key) = self.merge(&bodies, tool, cache)?;
        // A union of solids that aren't empty isn't, but it's cheap to
        // make sure no body ever is.
        if solid.is_empty() {
            return Err(message::emptied(Doing::Joining, self.body_name(into)));
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
    ) -> Result<(Arc<Solid>, Key), String> {
        let (first, rest) = bodies.split_first().expect("two or more bodies are merged");
        let mut unite = |key: Key, a: &Solid, b: &Solid| {
            let solid = cache.boolean(key, || {
                varde_kernel::boolean(a, b, Op::Union, &self.tolerance, &Budget::DEFAULT)
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
        .map_err(|error| {
            let name = self.body_name(first.body);
            message::leave_out(message::boolean(Doing::Joining, name, error), name)
        })?;
        for made in rest {
            merged = unite(
                boolean_key(Doing::Merging, merged.1, made.key),
                &merged.0,
                &made.solid,
            )
            .map_err(|error| {
                let (into, other) = (self.body_name(first.body), self.body_name(made.body));
                message::merging(into, other, error)
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
    ) -> Result<Vec<BodyId>, (Vec<BodyId>, String)> {
        let mut touched = Vec::new();
        for made in bodies.iter().filter(|made| !excluded.contains(&made.body)) {
            let touches = cache.touches(touches_key(made.key, tool_key), || {
                varde_kernel::touches(&made.solid, tool, &self.tolerance, &self.touching)
            });
            match touches {
                Ok(true) => touched.push(made.body),
                Ok(false) => {}
                Err(error) => {
                    let error = message::boolean(Doing::Touching, self.body_name(made.body), error);
                    // Listed, so the panel offers to take it out.
                    touched.push(made.body);
                    return Err((touched, error));
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

    /// The solid swept from the regions over `span` on `frame`.
    fn solid(&self, frame: &Frame, (from, to): (f64, f64)) -> Result<Solid, String> {
        let profiles = match &*self.sketch.profiles {
            Ok(profiles) => profiles,
            Err(e) => return Err(format!("its sketch is {e}")),
        };
        let regions = profiles
            .resolve(&self.extrude.regions)
            .into_iter()
            .collect::<Option<Vec<usize>>>()
            .ok_or("region not found")?;
        let loops = profiles.merge(&regions).map_err(|e| e.to_string())?;
        let profile = profile(self.sketch.sketch, profiles, &loops, self.tolerance.fit())
            .map_err(|e| e.to_string())?;
        varde_kernel::extrude(
            &profile,
            frame,
            from,
            to,
            self.feature.id.get(),
            &self.tolerance,
            &Budget::DEFAULT,
        )
        .map_err(|error| message::extrude(error, self.tolerance.fit() <= Tolerance::MIN_FIT))
    }
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
