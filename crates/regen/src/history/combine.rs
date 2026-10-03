//! Evaluating a combine: the target's solid, as the features before it
//! leave it, and each tool's, one boolean a tool.

use std::sync::Arc;

use varde_document::{BodyId, BodyOp, Combine, Document};
use varde_kernel::{Budget, KernelError, Op, Solid, Tolerance};

use super::{BodySolid, Evaluation, Failed, boolean_key, note_merge, own_solids};
use crate::cache::{Cache, Key};
use crate::error_geometry::KernelFailure;
use crate::message::{self, Doing};

/// Changes the bodies of `evaluation` (those the features before the
/// combine made) as `combine` says, or says why it fails, changing
/// nothing.
///
/// Every body it names must have a solid of its own ([`own_solids`]):
/// one a join or an earlier combine consumed fails it, naming the body
/// holding it, and so does one whose maker failed. The target's solid is then united with,
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
/// they were. A step that fails shows the kernel's evidence, the faces
/// it names of the running solid looked for on the target and the tools
/// combined into it so far, and those of the tool on the tool.
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
    own_solids(document, combine.bodies(), evaluation)?;
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
    // A step on the running solid: on success it holds the tool too; on
    // failure it stays as it was. A failure names faces of the running
    // solid on every body it holds.
    let mut step = |running: &mut Running, tool: &BodySolid| {
        let next = boolean_key(doing, running.key, tool.key);
        let result = cache.boolean(next, || {
            varde_kernel::boolean(&running.solid, &tool.solid, op, tolerance, &Budget::DEFAULT)
                .map_err(|failure| KernelFailure::new(failure, tolerance))
        });
        let words = match doing {
            // The messages say "joining" for a union.
            Doing::Merging => Doing::Joining,
            _ => doing,
        };
        let tool_name = name(tool.body);
        let solid = result.map_err(|failure| {
            let pieces = match failure.error {
                KernelError::TooComplex => shells(&tool.solid),
                _ => 1,
            };
            let message = message::combining(words, target_name, tool_name, pieces, failure.error);
            Failed::kernel(message, &failure, [&running.held, &[tool.body]])
        })?;
        if solid.is_empty() {
            return Err(message::combine_emptied(words, target_name, tool_name).into());
        }
        running.solid = solid;
        running.key = next;
        running.held.push(tool.body);
        Ok(())
    };
    let mut running = Running {
        solid: Arc::clone(&target.solid),
        key: target.key,
        held: vec![target.body],
    };
    let mut failed = Vec::new();
    for &tool in &tools {
        match step(&mut running, tool) {
            Ok(()) => {}
            Err(error) if op == Op::Union => failed.push((tool, error)),
            Err(error) => return Err(error),
        }
    }
    // Only worth trying again if some step worked: otherwise each is the
    // one that failed, found in the cache (as it is where every step that
    // worked came before the failures).
    if failed.len() < tools.len() {
        for (tool, _) in std::mem::take(&mut failed) {
            step(&mut running, tool)?;
        }
    }
    if let Some((_, error)) = failed.into_iter().next() {
        return Err(error);
    }
    let Running { solid, key, .. } = running;
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

/// A combine's running solid: the target's, with the tools united with,
/// taken from or intersected with it so far.
struct Running {
    solid: Arc<Solid>,
    /// What it's filed under.
    key: Key,
    /// The bodies it holds, the target and those tools, in that order:
    /// where the faces a failing step names of it are looked for.
    held: Vec<BodyId>,
}

/// How many separate pieces `solid` is: the components of its triangles
/// joined across their edges (a pattern's copies apart are one each).
fn shells(solid: &Solid) -> usize {
    let tris = solid.mesh().tris();
    let mut seen = vec![false; tris.len()];
    let mut stack = Vec::new();
    let mut count = 0;
    for start in 0..tris.len() {
        if seen[start] {
            continue;
        }
        count += 1;
        seen[start] = true;
        stack.push(start);
        while let Some(t) = stack.pop() {
            for halfedge in tris[t].halfedges {
                let next = halfedge.pair as usize / 3;
                if let Some(false) = seen.get(next) {
                    seen[next] = true;
                    stack.push(next);
                }
            }
        }
    }
    count
}
