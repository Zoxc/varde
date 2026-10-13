//! The [`Regenerator`]: what a lane runs, answering requests and
//! keeping its [`Cache`] for the next ones.

use std::borrow::Cow;
use std::sync::Arc;

use varde_document::{Command, Document, Editor, FeatureId};

use crate::drafted::of_feature;
use crate::{
    Answer, Asked, COARSER_STEPS, Cache, Draft, Drafted, ErrorGeometry, Evaluation, Inspect,
    Progress, Query, Regenerated, Request, Response, Sight, evaluate, flatten_sketches,
    tessellate_scene, unsolved,
};

/// `document` rolled back to before `until`, if it's given.
pub(crate) fn rolled_back(document: &Document, until: Option<FeatureId>) -> Cow<'_, Document> {
    match until {
        Some(until) => Cow::Owned(document.before(until)),
        None => Cow::Borrowed(document),
    }
}

/// Answers requests, keeping what it worked out for the next ones (see
/// [`Cache`]): what a lane runs.
#[derive(Default)]
pub struct Regenerator {
    cache: Cache,
}

impl Regenerator {
    /// One whose cache holds `budget` bytes (see [`Cache`]), for tests.
    #[cfg(test)]
    pub(crate) fn with_budget(budget: usize) -> Regenerator {
        Regenerator {
            cache: Cache::with_budget(budget),
        }
    }

    /// Does the work of `request`.
    pub fn handle(&mut self, request: Request) -> Response {
        self.handle_reporting(request, &mut |_| {})
    }

    /// [`Regenerator::handle`], telling `report` of a regeneration's
    /// [`Progress`] as it goes.
    pub fn handle_reporting(
        &mut self,
        request: Request,
        report: &mut dyn FnMut(Progress),
    ) -> Response {
        match request {
            Request::Regenerate {
                ref document,
                ref draft,
                ref inspect,
                ref sight,
                ..
            } => {
                let asked = request.asked().expect("a regeneration asks");
                self.cache.begin();
                let document = rolled_back(document, asked.until);
                let payloads = (draft.as_deref(), inspect.as_deref());
                match self.regenerate(&document, asked, payloads, sight.as_deref(), report) {
                    Ok(model) => Response::Regenerated(model),
                    Err(error) => Response::Failed { asked, error },
                }
            }
            // Not a request of its own to the cache: `Cache::begin` would
            // let go of the meshes the regeneration before used and an
            // export doesn't, which the next one would draw again.
            Request::Export { export, document } => {
                let evaluation = evaluate(&document, &mut self.cache);
                let result = crate::export(&document, &evaluation).map_err(|e| e.to_string());
                Response::Exported { export, result }
            }
            // Not a request of its own to the cache either, as an export.
            Request::Query {
                tag,
                generation,
                document,
                until,
                query,
            } => {
                let document = rolled_back(&document, until);
                let evaluation = evaluate(&document, &mut self.cache);
                let answer = match query {
                    Query::Topology(body) => Answer::Topology {
                        body,
                        result: match evaluation.solid(body) {
                            Some(made) => {
                                let topology = crate::inspect::topology(made, &mut self.cache);
                                crate::edge_facts::body_edges(&made.solid, &topology)
                            }
                            None => Err("the body has no solid there".to_owned()),
                        },
                    },
                    Query::Measure(inspect) => {
                        Answer::Measured(Ok(Box::new(crate::inspect::inspect(
                            &inspect,
                            &document,
                            &evaluation,
                            None,
                            &mut self.cache,
                        ))))
                    }
                };
                Response::Answered {
                    tag,
                    generation,
                    until,
                    answer,
                }
            }
        }
    }

    /// The model of `document` with `draft` applied, or without it if it
    /// fails, leaving out the lines of the sketch `exclude`, with
    /// `inspect` measured on it, telling `report` how far it has got.
    fn regenerate(
        &mut self,
        document: &Document,
        asked: Asked,
        (draft, inspect): (Option<&Draft>, Option<&Inspect>),
        sight: Option<&Sight>,
        report: &mut dyn FnMut(Progress),
    ) -> Result<Regenerated, String> {
        let Some(draft) = draft else {
            return self.model(document, asked, None, (inspect, sight), report);
        };
        let drafted = match applied(document, draft) {
            Ok((with_draft, feature)) => {
                let mut evaluation = self.evaluate(&with_draft, report);
                let mut drafted = Drafted::found(draft.revision, &evaluation, feature);
                match (evaluation.failed.iter()).position(|failed| failed.feature == feature) {
                    Some(at) => {
                        let failed = evaluation.failed.swap_remove(at);
                        drafted.error = Some(failed.message);
                        drafted.geometry = failed.geometry;
                        drafted
                    }
                    None => {
                        drafted.uncut = of_feature(&evaluation.uncut, feature).unwrap_or_default();
                        report(Progress::drawing(with_draft.features().len()));
                        let payloads = (inspect, sight);
                        return self.draw(&with_draft, evaluation, asked, Some(drafted), payloads);
                    }
                }
            }
            Err(error) => Drafted::refused(draft.revision, error),
        };
        self.model(document, asked, Some(drafted), (inspect, sight), report)
    }

    /// The model of `document`, telling `report` how far it has got.
    fn model(
        &mut self,
        document: &Document,
        asked: Asked,
        draft: Option<Drafted>,
        payloads: (Option<&Inspect>, Option<&Sight>),
        report: &mut dyn FnMut(Progress),
    ) -> Result<Regenerated, String> {
        let evaluation = self.evaluate(document, report);
        report(Progress::drawing(document.features().len()));
        self.draw(document, evaluation, asked, draft, payloads)
    }

    /// The history of `document` evaluated, telling `report` of each
    /// feature as it starts on it.
    fn evaluate(&mut self, document: &Document, report: &mut dyn FnMut(Progress)) -> Evaluation {
        let count = document.features().len();
        crate::history::evaluate_reporting(document, &mut self.cache, &mut |index, feature| {
            report(Progress::feature(index, count, &feature.name));
        })
    }

    /// The model of `document`, whose history gave `evaluation`, with
    /// `inspect` measured on it, drawn by `sight`.
    fn draw(
        &mut self,
        document: &Document,
        mut evaluation: Evaluation,
        asked: Asked,
        mut draft: Option<Drafted>,
        (inspect, sight): (Option<&Inspect>, Option<&Sight>),
    ) -> Result<Regenerated, String> {
        // Only a draft that worked is drawn: one that failed is answered
        // with the committed model.
        let drafted = draft.as_ref().is_some_and(|draft| draft.error.is_none());
        // A sight too fine for the mesh's limits is drawn coarser, a few
        // levels at a time, and in the end at the share of each solid's
        // size, rather than failing the model.
        let mut sight = sight.cloned();
        let scene = loop {
            let scene = tessellate_scene(
                document,
                &evaluation,
                drafted,
                sight.as_ref(),
                &mut self.cache,
            );
            match (scene, sight) {
                (Err(_), Some(fine)) => sight = fine.coarser(COARSER_STEPS),
                (scene, _) => break scene,
            }
        }
        .map_err(|error| error.to_string())?;
        let sketches = flatten_sketches(document, &evaluation.placements, asked.exclude)
            .map_err(|error| error.to_string())?;
        // The failures' operand faces, on the model drawn.
        let mut failed = std::mem::take(&mut evaluation.failed);
        let holder = |body| evaluation.holder(body);
        let drafted = draft.as_mut().map(|draft| &mut draft.geometry);
        for geometry in (failed.iter_mut().map(|f| &mut f.geometry)).chain(drafted) {
            ErrorGeometry::resolve_shared(geometry, &scene.mesh, &scene.picking, holder);
        }
        evaluation.failed = failed;
        let bodies = evaluation
            .bodies
            .iter()
            .filter_map(|made| Some((made.body, made.solid.bounds()?)))
            .collect();
        let inspected = inspect.map(|inspect| {
            crate::inspect::inspect(
                inspect,
                document,
                &evaluation,
                Some((&scene.mesh, &scene.picking)),
                &mut self.cache,
            )
        });
        // A draft's links aren't followed: only the committed
        // document's are the app's to fold in.
        let relinked = if draft.is_some() {
            Vec::new()
        } else {
            std::mem::take(&mut evaluation.relinked)
        };
        Ok(Regenerated {
            asked,
            relinked,
            broken: std::mem::take(&mut evaluation.broken),
            draft: draft.map(Box::new),
            mesh: scene.mesh,
            picking: scene.picking,
            tables: scene.tables,
            sketches: Arc::new(sketches),
            unsolved: unsolved(document, &mut self.cache),
            failed: evaluation.failed,
            touched: evaluation.touched,
            merged: evaluation.merged,
            placements: evaluation.placements,
            bodies,
            inspected: inspected.map(Box::new),
        })
    }

    /// What the regenerator's cache has found and worked out so far.
    pub fn cache(&self) -> &Cache {
        &self.cache
    }
}

/// `document` with `draft` applied, and the draft's feature, or why it
/// can't be applied.
pub(crate) fn applied(document: &Document, draft: &Draft) -> Result<(Document, FeatureId), String> {
    let kind = draft.kind.clone();
    let command = match draft.feature {
        None => document.add_feature(kind),
        // `SetFeature` of a missing feature changes nothing, and the
        // draft would seem to go; a sketch feature, or a sketch kind, the
        // editor refuses.
        Some(feature) if document.feature(feature).is_none() => {
            return Err("the draft's feature isn't there".to_owned());
        }
        Some(feature) => Command::SetFeature {
            feature,
            kind: Box::new(kind),
        },
    };
    let mut editor = Editor::new(document.clone());
    editor.apply(command).map_err(|error| error.to_string())?;
    let drafted = editor.document();
    let feature = match draft.feature {
        Some(feature) => feature,
        None => {
            drafted
                .features()
                .last()
                .ok_or("the draft wasn't added")?
                .id
        }
    };
    Ok((drafted.clone(), feature))
}

/// Does the work of `request` without a cache. Pure, so it can run
/// anywhere.
pub fn handle(request: Request) -> Response {
    Regenerator::default().handle(request)
}
