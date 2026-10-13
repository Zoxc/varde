//! Drawing the model: tessellating the visible bodies, flattening the
//! sketches' curves, welding bodies for an export, and telling the
//! sketches that don't solve.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use varde_document::{BodyId, Document, FeatureId, FeatureKind, Placement, Plane, Sketch};
use varde_kernel_compat::{
    Display, LinesError, ManifoldError, ManifoldMesh, MeshError, RenderLines, RenderMesh,
};
use varde_sketch::{Budget, Goal};

use crate::cache::Keyer;
use crate::picking::{Drawn, Scene};
use crate::{Cache, Evaluation, Picking, Sight};

/// Tessellates the solids of the visible bodies of `document` in
/// `evaluation` into a single mesh in world space, a part per body in
/// the order they were made, within the [`Display`] of the document's
/// tolerance ([`Document::tolerance`]), each drawn once and kept in
/// `cache`. The joined mesh is kept too, filed by the shown bodies and
/// their mesh keys in order (which hold the tolerance): a scene that
/// didn't change gives the same `Arc` without joining again (see
/// [`Cache`]). Fails if it would have more vertices, indices or edges
/// than a [`RenderMesh`] may hold, which a file with enough bodies in it
/// can ask for.
pub fn tessellate(
    document: &Document,
    evaluation: &Evaluation,
    cache: &mut Cache,
) -> Result<Arc<RenderMesh>, MeshError> {
    tessellate_scene(document, evaluation, false, None, cache).map(|scene| scene.mesh)
}

/// [`tessellate`], with the scene's [`Picking`] tables.
pub fn tessellate_picking(
    document: &Document,
    evaluation: &Evaluation,
    cache: &mut Cache,
) -> Result<(Arc<RenderMesh>, Arc<Picking>), MeshError> {
    tessellate_scene(document, evaluation, false, None, cache)
        .map(|scene| (scene.mesh, scene.picking))
}

/// [`tessellate_picking`], for a draft's answer if `drafted`, whose scene
/// doesn't become the committed one the cache never evicts (see
/// [`Cache`]), each body drawn at its level of the view's `sight` if
/// there is one ([`Display::for_view`]), each level's meshes filed apart.
pub(crate) fn tessellate_scene(
    document: &Document,
    evaluation: &Evaluation,
    drafted: bool,
    sight: Option<&Sight>,
    cache: &mut Cache,
) -> Result<Scene, MeshError> {
    let fit = document.tolerance().fit().to_bits();
    let shown: Vec<_> = evaluation
        .bodies
        .iter()
        .filter(|made| document.body(made.body).is_some_and(|body| body.visible))
        .map(|made| {
            let detail = sight.and_then(|sight| sight.level(made.body));
            // Levels past the bounds draw as the bounds do, so they're
            // filed so.
            let level = detail.map_or(u64::MAX, |detail| detail.chord().to_bits());
            let key = (Keyer::new("mesh").key(made.key).number(fit))
                .number(level)
                .finish();
            (made, key, detail)
        })
        .collect();
    // The bodies too: the picking tables name the body of each part.
    let mut scene = Keyer::new("scene");
    for (made, key, _) in &shown {
        scene.value(&made.body).key(*key);
    }
    let filed = scene.number(shown.len() as u64).finish();
    let mut found = true;
    let scene = cache.scene(filed, drafted, |cache| {
        found = false;
        let tolerance = document.tolerance();
        let mut mesh = RenderMesh::default();
        let mut picking = Picking::default();
        for (made, key, detail) in &shown {
            let display = match detail {
                Some(detail) => Display::for_view(&tolerance, detail.chord()),
                None => Display::new(&tolerance),
            };
            // The topology is kept too, for the measure tool's picks.
            let drawn = cache.mesh(*key, |cache| {
                Drawn::new(
                    &made.solid,
                    &crate::inspect::topology(made, cache),
                    &display,
                )
            })?;
            mesh.append(&drawn.mesh)?;
            picking.append(made.body, &drawn)?;
        }
        Ok(Scene::new(mesh, picking))
    })?;
    if found {
        // The bodies' meshes stay for the next scene that changes one.
        for (_, key, _) in &shown {
            cache.keep(*key);
        }
    }
    Ok(scene)
}

/// A visible body welded for export: see [`export`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportedBody {
    pub body: BodyId,
    /// The body's name in the document.
    pub name: String,
    pub mesh: ManifoldMesh,
}

/// Why the visible bodies can't be exported: the first body, in the
/// order the history made them, whose solid gives no [`ManifoldMesh`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{name} can't be exported: {error}")]
pub struct ExportError {
    pub body: BodyId,
    pub name: String,
    /// What [`ManifoldError`] the mesh failed with, in words.
    pub error: String,
}

/// The solids of the visible bodies of `document` in `evaluation`, in the
/// order the history made them, each welded into a closed, oriented
/// [`ManifoldMesh`] at the [`Display`] of the document's tolerance (the
/// samples it is drawn with, see [`varde_kernel_compat::Solid::manifold_mesh`]),
/// for a file such as 3MF. None visible gives none. Fails with the first
/// body whose mesh fails its check or is too large: a body is never
/// written as something that isn't a manifold.
pub fn export(
    document: &Document,
    evaluation: &Evaluation,
) -> Result<Vec<ExportedBody>, ExportError> {
    let display = Display::new(&document.tolerance());
    let shown = (evaluation.bodies.iter())
        .filter_map(|made| Some((made, document.body(made.body).filter(|body| body.visible)?)));
    let mut out = Vec::new();
    for (made, body) in shown {
        let failed = |error: ManifoldError| ExportError {
            body: body.id,
            name: body.name.clone(),
            error: error.to_string(),
        };
        out.push(ExportedBody {
            body: body.id,
            name: body.name.clone(),
            mesh: made.solid.manifold_mesh(&display).map_err(failed)?,
        });
    }
    Ok(out)
}

/// The non-construction curves of the visible sketches of `document`,
/// except `exclude`, flattened (see [`Sketch::flatten`]), lines without
/// the ends fillets and chamfers cut off ([`Sketch::cut_back`]), and placed on
/// their planes in world space, a polyline each: a sketch on an origin
/// plane at the plane's placement, one on a face at its placement in
/// `placements` ([`Evaluation::placements`]); one on a face that isn't
/// there isn't placed, and isn't drawn. Fails if there would be more
/// points than [`RenderLines`] may hold, which a file with enough
/// sketches in it can ask for.
///
/// [`Sketch::flatten`]: varde_document::Sketch::flatten
/// [`Sketch::cut_back`]: varde_document::Sketch::cut_back
pub fn flatten_sketches(
    document: &Document,
    placements: &[(FeatureId, Placement)],
    exclude: Option<FeatureId>,
) -> Result<RenderLines, LinesError> {
    let placements: BTreeMap<FeatureId, Placement> = placements.iter().copied().collect();
    let mut lines = RenderLines::default();
    let shown = document
        .features()
        .iter()
        .filter(|feature| feature.visible && Some(feature.id) != exclude);
    for feature in shown {
        let FeatureKind::Sketch { plane, sketch, .. } = &feature.kind else {
            continue;
        };
        let placement = match plane {
            Plane::Origin(origin) => origin.placement(),
            Plane::Face(_) => match placements.get(&feature.id) {
                Some(placement) => *placement,
                None => continue,
            },
        };
        push_sketch_lines(&mut lines, sketch, placement)?;
    }
    Ok(lines)
}

/// Adds the curves of `sketch`, placed at `placement`, to `lines` as
/// [`flatten_sketches`] draws them: construction curves aside, and what a
/// fillet or chamfer cuts off a line left out.
pub fn push_sketch_lines(
    lines: &mut RenderLines,
    sketch: &Sketch,
    placement: Placement,
) -> Result<(), LinesError> {
    let cut_back = sketch.cut_back();
    for entry in sketch.curves.iter().filter(|entry| !entry.construction) {
        // A checked sketch's curves name its own points.
        let Some(mut polyline) = sketch.flatten(&entry.curve) else {
            continue;
        };
        // What a fillet or chamfer cuts off a line isn't drawn.
        if let (Some(kept), &[start, end]) = (cut_back.get(&entry.id), &polyline[..]) {
            match varde_sketch::cut_line(start, end, *kept).0 {
                Some(kept) => polyline = kept.to_vec(),
                None => continue,
            }
        }
        lines.push(
            polyline
                .into_iter()
                .map(|at| placement.to_world(at).as_vec3()),
        )?;
    }
    Ok(())
}

/// The sketches of `document` that don't solve, warm started from where
/// their points are, within the default [`Budget`]: those a file holds
/// that a different build's solver solved, or that were never solved. A
/// sketch the app committed is solved, and settles at once. Each sketch
/// is solved once and kept in `cache`.
pub fn unsolved(document: &Document, cache: &mut Cache) -> Vec<FeatureId> {
    let budget = Budget::default();
    document
        .features()
        .iter()
        .filter(|feature| {
            let FeatureKind::Sketch { sketch, .. } = &feature.kind else {
                return false;
            };
            let key = Keyer::new("solves").value(sketch).finish();
            !cache.solves(key, || {
                varde_sketch::solve(sketch, &Goal::Settle, &budget).is_ok()
            })
        })
        .map(|feature| feature.id)
        .collect()
}
