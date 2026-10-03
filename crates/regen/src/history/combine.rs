//! Evaluating a combine: the target's solid, as the features before it
//! leave it, and each tool's, one boolean a tool.

use std::sync::Arc;

use varde_document::{BodyId, BodyOp, Combine, Document};
use varde_kernel::{Budget, Op, Solid, Tolerance};

use super::{BodySolid, Evaluation, Failed, boolean_key, note_merge};
use crate::cache::{Cache, Key};
use crate::message::{self, Doing};

/// Changes the bodies of `evaluation` (those the features before the
/// combine made) as `combine` says, or says why it fails, changing
/// nothing.
///
/// Every body it names must have a solid of its own: one a join or an
/// earlier combine consumed fails it, naming the body holding it (the
/// user meant that body as it was, not the one it went into), and so
/// does one whose maker failed. The target's solid is then united with,
/// less, or intersected with each tool's, in the order the tools were
/// made, the running solid first, each step a [`varde_kernel::boolean`]
/// cached by the operation, the running solid's key and the tool's (as
/// a join's merge chain is: a union step is keyed as a join's merging
/// two bodies, so the two share what they work out). A union whose step
/// fails goes on with the next tool and tries the ones that failed again
/// at the end, once, in order: tools meeting the target only along an
/// edge or at a point make no clean solid with it until another tool
/// bridges them, as a join's tool can bridge the bodies it merges. A
/// step that would leave nothing of the target fails the combine (no
/// body is ever empty). The target gets the result; the tools are
/// consumed into it, as a join merging bodies consumes them
/// ([`note_merge`]), unless the combine keeps them, when they stay as
/// they were.
pub(super) fn evaluate(
    document: &Document,
    combine: &Combine,
    tolerance: &Tolerance,
    evaluation: &mut Evaluation,
    cache: &mut Cache,
) -> Result<(), Failed> {
    let name = |body: BodyId| {
        document
            .body(body)
            .map_or("a body", |body| body.name.as_str())
    };
    for body in combine.bodies() {
        if evaluation.bodies.iter().any(|made| made.body == body) {
            continue;
        }
        return Err(match evaluation
            .merged
            .iter()
            .find(|(consumed, _)| *consumed == body)
        {
            Some(&(_, holder)) => message::consumed(name(body), name(holder)),
            None => message::no_solid(name(body)),
        }
        .into());
    }
    let find = |body: BodyId| {
        (evaluation.bodies.iter())
            .find(|made| made.body == body)
            .expect("every body named has a solid")
    };
    let target = find(combine.target);
    // In the order they were made, as the bodies are.
    let tools: Vec<&BodySolid> = (evaluation.bodies.iter())
        .filter(|made| combine.tools.binary_search(&made.body).is_ok())
        .collect();
    let (op, doing) = match combine.op {
        BodyOp::Union => (Op::Union, Doing::Merging),
        BodyOp::Subtract => (Op::Difference, Doing::Cutting),
        BodyOp::Intersect => (Op::Intersection, Doing::Intersecting),
    };
    let target_name = name(target.body);
    let mut step = |(solid, key): &(Arc<Solid>, Key), tool: &BodySolid| {
        let next = boolean_key(doing, *key, tool.key);
        let result = cache.boolean(next, || {
            varde_kernel::boolean(solid, &tool.solid, op, tolerance, &Budget::DEFAULT)
        });
        let words = match doing {
            // The messages say "joining" for a union.
            Doing::Merging => Doing::Joining,
            _ => doing,
        };
        let tool_name = name(tool.body);
        let solid = result.map_err(|failure| {
            let message = message::combining(words, target_name, tool_name, failure.error);
            Failed::kernel(message, failure, [Some(target.body), Some(tool.body)])
        })?;
        if solid.is_empty() {
            return Err(message::combine_emptied(words, target_name, tool_name).into());
        }
        Ok((solid, next))
    };
    let mut running = (Arc::clone(&target.solid), target.key);
    let mut failed = Vec::new();
    for &tool in &tools {
        match step(&running, tool) {
            Ok(next) => running = next,
            Err(error) if op == Op::Union => failed.push((tool, error)),
            Err(error) => return Err(error),
        }
    }
    // Only worth trying again if some step worked: otherwise each is the
    // one that failed, found in the cache (as it is where every step that
    // worked came before the failures).
    if failed.len() < tools.len() {
        for (tool, _) in std::mem::take(&mut failed) {
            running = step(&running, tool)?;
        }
    }
    if let Some((_, error)) = failed.into_iter().next() {
        return Err(error);
    }
    let (solid, key) = running;
    let into = target.body;
    if !combine.keep_tools {
        let merging: Vec<BodyId> = std::iter::once(into)
            .chain(tools.iter().map(|tool| tool.body))
            .collect();
        note_merge(&mut evaluation.merged, &merging);
        (evaluation.bodies).retain(|made| combine.tools.binary_search(&made.body).is_err());
    }
    if let Some(made) = evaluation.bodies.iter_mut().find(|made| made.body == into) {
        *made = BodySolid {
            body: into,
            solid,
            key,
        };
    }
    Ok(())
}
