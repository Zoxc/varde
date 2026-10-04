//! The measure tool: its session, started by `Look::StartMeasure` (`I`,
//! the toolbar, the rail's Inspect set), picking A and B with the cursor
//! (faces, edges, the snap points of what it's over, bodies by a
//! double-click or their rows in Objects), measured by the regeneration
//! lane with the model (`varde_regen::Inspect`, counted into what the
//! feed asks for as a draft is), and left by `Esc` or Close.
//!
//! It writes nothing to the document, has no undo, and leaves the
//! selection as it was: while it's in use, clicks pick for it and the
//! highlight is its own (hovered, A in the accent, B in the second
//! colour), built only from where the newest answer found the picks
//! (`Probed::at`), so it always names the model shown.
//!
//! A pick is kept by name, as a reference is (a body, the keys of a face,
//! an edge, an edge's point or a corner, and the point it was picked at),
//! and sent with every request, so after an edit regen finds it again in
//! the new model. One the new model doesn't have is shown as not found,
//! and kept, so an undo finds it again.

use std::sync::Arc;

use varde_document::BodyId;
use varde_regen::{At, Entity, InspectPick, Inspected, Measure, Probed};
use varde_view::{
    MeasureLook, MeasureSlot, MeasureState, MeasuredPick, ModelHighlight, Outcome, Pick, PickIndex,
    Picked, Selected, Snapped,
};

use super::Doc;

/// The measure tool, while it's in use: [`Doc::measure`].
#[derive(Debug, Default)]
pub(crate) struct MeasureSession {
    /// A and B. B only with A.
    pub(crate) picks: [Option<InspectPick>; 2],
    /// The one the last click picked, which the second click of a
    /// double-click makes its body.
    last: Option<MeasureSlot>,
    /// Whether each pick's own values are folded away under what's
    /// between them: both to start with.
    folded: [bool; 2],
    /// Built when what it's of changes, so the renderer uploads it only
    /// then.
    highlight: Arc<ModelHighlight>,
    /// What `highlight` was built of.
    built: Option<Built>,
}

/// What a measure highlight is built of: the model, the target hovered
/// and what's highlighted of A and of B.
type Built = (u64, Option<Picked>, [Vec<Picked>; 2]);

impl MeasureSession {
    fn new() -> Self {
        Self {
            folded: [true; 2],
            ..Self::default()
        }
    }

    /// Takes a click on `pick` of `index`'s model, or on nothing: the
    /// first picks A, the second B, a third A again without B; with
    /// `add` it picks B (A while there's none); the second click of a
    /// double-click (`double`) makes what the first picked its body. A
    /// click on nothing lets go of both.
    fn click(&mut self, index: &PickIndex, pick: Option<Pick>, add: bool, double: bool) {
        let Some(pick) = pick else {
            self.picks = [None, None];
            self.last = None;
            return;
        };
        if double && let Some(slot) = self.last {
            self.picks[slot.index()] = Some(body(pick.body));
            return;
        }
        let Some(picked) = inspect_pick(index, pick) else {
            return;
        };
        self.put(picked, add);
    }

    /// Picks `picked`, B if `add` and there's an A, else as the next
    /// click would.
    fn put(&mut self, picked: InspectPick, add: bool) {
        let slot = match self.picks {
            [None, _] => MeasureSlot::A,
            [Some(_), _] if add => MeasureSlot::B,
            [Some(_), None] => MeasureSlot::B,
            [Some(_), Some(_)] => {
                self.picks[1] = None;
                MeasureSlot::A
            }
        };
        self.picks[slot.index()] = Some(picked);
        self.last = Some(slot);
    }

    /// What the regeneration lane is asked to measure: A, and B if
    /// there's one.
    pub(crate) fn inspect(&self) -> Option<(InspectPick, Option<InspectPick>)> {
        Some((self.picks[0]?, self.picks[1]))
    }
}

/// A pick of `body` whole.
fn body(body: BodyId) -> InspectPick {
    InspectPick {
        body,
        entity: Entity::Body,
        near: [0.0; 3],
    }
}

/// What the selected `item` names, as the measure keeps a pick: a vertex
/// as its corner.
fn selected_pick(item: &Selected) -> InspectPick {
    let (entity, near) = match *item {
        Selected::Body(body) => return self::body(body),
        Selected::Face { key, near, .. } => (Entity::Face(key), near),
        Selected::Edge { faces, near, .. } => (Entity::Edge(faces), near),
        Selected::Vertex { faces, near, .. } => (Entity::Corner(faces), near),
    };
    InspectPick {
        body: item.body(),
        entity,
        near: near.to_array(),
    }
}

/// What `pick` of `index`'s model names, as the measure keeps it: the
/// snap point it took, if any (a corner at its point, an edge's point
/// where the edge was picked), else the face or edge, by the keys a
/// reference stores and where it was picked. `None` for what isn't in
/// the tables.
fn inspect_pick(index: &PickIndex, pick: Pick) -> Option<InspectPick> {
    let picking = index.picking();
    let (entity, near) = match (pick.snap, pick.target) {
        (Some(Snapped::Corner(corner)), _) => {
            picking.corners().get(corner as usize)?;
            let point = index.snap_point(Snapped::Corner(corner))?;
            (Entity::Corner(picking.corner_keys(corner)), point)
        }
        (Some(Snapped::EdgePoint(edge)), _) => {
            (Entity::EdgePoint(index.chain_keys(edge)?), pick.at)
        }
        (None, Picked::Face(face)) => (
            Entity::Face(picking.faces().get(face as usize)?.key),
            pick.at,
        ),
        (None, Picked::Edge(edge)) => (Entity::Edge(index.chain_keys(edge)?), pick.at),
        // A vertex is its corner, which it snaps to; one with no corner
        // in the tables (past their bound) isn't picked.
        (None, Picked::Vertex(_)) => return None,
    };
    Some(InspectPick {
        body: pick.body,
        entity,
        near: near.to_array(),
    })
}

impl Doc {
    /// Starts the measure tool outside sketches and the operations being
    /// set up, a read-only document included, or leaves it.
    pub(crate) fn start_measure(&mut self) {
        if self.measure.take().is_some() || self.sketch.is_some() || self.operating() {
            return;
        }
        self.picking_plane = None;
        self.measure = Some(MeasureSession::new());
    }

    /// Takes `message`, changing the measure tool.
    pub(crate) fn measure_look(&mut self, message: MeasureLook) {
        let Some(session) = &mut self.measure else {
            return;
        };
        match message {
            MeasureLook::Close => self.measure = None,
            MeasureLook::Fold(slot) => {
                let folded = &mut session.folded[slot.index()];
                *folded = !*folded;
            }
        }
    }

    /// Takes a click on the model while measuring, see
    /// [`MeasureSession::click`]: nothing else changes.
    pub(crate) fn measure_click(&mut self, pick: Option<Pick>, add: bool, double: bool) {
        if pick.is_some_and(|pick| pick.model != self.feed.model()) {
            return;
        }
        let index = self.feed.pick_index();
        if let Some(session) = &mut self.measure {
            session.click(index, pick, add, double);
        }
    }

    /// Picks `body` whole while measuring, from its row in Objects: B with
    /// `add`, else as the next click would.
    pub(crate) fn measure_body(&mut self, body: BodyId, add: bool) {
        if self.editor.document().body(body).is_none() {
            return;
        }
        if let Some(session) = &mut self.measure {
            session.put(self::body(body), add);
        }
    }

    /// Ends the measure tool once a sketch is edited or an operation set
    /// up, or the document was replaced whole (`replaced`): the bodies its
    /// picks name may be others now.
    pub(crate) fn prune_measure(&mut self, replaced: bool) {
        if replaced || self.sketch.is_some() || self.operating() {
            self.measure = None;
        }
    }

    /// What the regeneration lane is asked to measure of what's selected
    /// in the model, for the status bar: one or two items, while the
    /// cursor picks the model for the selection and the status bar shows
    /// it (no tool, operation or plane pick in use, no feature selected).
    pub(crate) fn selection_inspect(&self) -> Option<(InspectPick, Option<InspectPick>)> {
        if !self.picks()
            || self.measure.is_some()
            || self.combine.is_some()
            || self.picking_plane.is_some()
            || self.selected_feature.is_some()
        {
            return None;
        }
        let mut picks = self.pick.selection.items().map(selected_pick);
        let first = picks.next()?;
        let second = picks.next();
        picks.next().is_none().then_some((first, second))
    }

    /// The newest answer's measures of what's selected in the model, see
    /// [`Doc::selection_inspect`]: `None` while they're on their way.
    pub(crate) fn selection_measured(&self) -> Option<&Inspected> {
        self.feed.inspected_of(self.selection_inspect()?)
    }

    /// The newest answer's outcome of the measure tool's picks, as
    /// [`MeshFeed::inspected`](super::feed::MeshFeed::inspected) gives
    /// it: `None` while it's on its way.
    fn probed(&self) -> [Option<&Result<Probed, String>>; 2] {
        let Some(inspected) = self.feed.inspected() else {
            return [None, None];
        };
        [Some(&inspected.first), inspected.second.as_ref()]
    }

    /// What the measure tool highlights of its picks in the model shown:
    /// where the newest answer found them, A in the accent and B in the
    /// second colour, a body as all its faces; a point is a dot instead
    /// (see [`Doc::measure_state`]).
    fn measure_targets(&self) -> [Vec<Picked>; 2] {
        let mut targets = [Vec::new(), Vec::new()];
        let Some(session) = &self.measure else {
            return targets;
        };
        let index = self.feed.pick_index();
        for (slot, probed) in self.probed().into_iter().enumerate() {
            let (Some(Ok(probed)), Some(pick)) = (probed, session.picks[slot]) else {
                continue;
            };
            let point = matches!(probed.measure, Ok(Measure::Point(_)));
            let targets = &mut targets[slot];
            match (probed.at, pick.entity) {
                (Some(At::Face(face)), _) => targets.push(Picked::Face(face)),
                (Some(At::Edge(edge)), _) if !point => targets.push(Picked::Edge(edge)),
                (None, Entity::Body) => {
                    // A body a join merged into another is measured, and
                    // drawn, as the body holding it.
                    let merged = self.feed.merged_bodies();
                    let holder = (merged.iter())
                        .find(|(merged, _)| *merged == pick.body)
                        .map_or(pick.body, |&(_, holder)| holder);
                    targets.extend(index.body_faces(holder).map(Picked::Face));
                }
                _ => {}
            }
        }
        targets
    }

    /// Rebuilds the measure tool's highlight if the model, the target
    /// hovered or what's highlighted of the picks changed since it was
    /// built.
    pub(crate) fn refresh_measure_highlight(&mut self) {
        if self.measure.is_none() || !self.picks() {
            return;
        }
        let targets = self.measure_targets();
        let hovered = self.pick.hover().map(|pick| pick.target);
        let key = (self.feed.model(), hovered, targets);
        let Some(session) = &mut self.measure else {
            return;
        };
        if session.built.as_ref() == Some(&key) {
            return;
        }
        let (_, hovered, [a, b]) = &key;
        let index = self.feed.pick_index();
        // What's picked keeps its colour under the cursor.
        let hover: Vec<Picked> = (hovered.iter())
            .filter(|target| !a.contains(target) && !b.contains(target))
            .copied()
            .collect();
        session.highlight = Arc::new(index.highlight_with(&hover, a, b));
        session.built = Some(key);
    }

    /// The measure tool's highlight, if it's in use and built for the
    /// model shown.
    pub(crate) fn measure_highlight(&self) -> Option<&Arc<ModelHighlight>> {
        let session = self.measure.as_ref()?;
        let current = (session.built.as_ref()).is_some_and(|built| built.0 == self.feed.model());
        Some(&session.highlight).filter(|highlight| current && !highlight.is_empty())
    }

    /// The name the panel shows `pick` by: what it is and its body's
    /// name, or the body's name for a body.
    fn pick_name(&self, pick: &InspectPick) -> String {
        let document = self.editor.document();
        let body = (document.body(pick.body)).map_or("a body that's gone", |body| &body.name);
        let kind = match pick.entity {
            Entity::Body => return body.to_owned(),
            Entity::Face(_) => "Face",
            Entity::Edge(_) => "Edge",
            Entity::EdgePoint(_) | Entity::Corner(_) => "Point",
        };
        format!("{kind} of {body}")
    }

    /// The measure tool, for the view, while it's in use.
    pub(crate) fn measure_state(&self) -> Option<MeasureState<'_>> {
        let session = self.measure.as_ref()?;
        let probed = self.probed();
        let picks = std::array::from_fn(|slot| {
            let pick = session.picks[slot].as_ref()?;
            let outcome = match probed[slot] {
                None => Outcome::Waiting,
                Some(Err(why)) => Outcome::Missing(why),
                Some(Ok(probed)) => {
                    Outcome::Measured(probed.measure.as_ref().map_err(String::as_str))
                }
            };
            Some(MeasuredPick {
                name: self.pick_name(pick),
                outcome,
            })
        });
        let points = std::array::from_fn(|slot| match probed[slot] {
            Some(Ok(Probed {
                measure: Ok(Measure::Point(at)),
                ..
            })) => Some(glam::DVec3::from(*at)),
            _ => None,
        });
        let between = (self.feed.inspected()).and_then(|inspected| inspected.between.as_ref());
        Some(MeasureState {
            picks,
            between,
            units: self.editor.document().units(),
            folded: session.folded,
            index: self.feed.pick_index(),
            hover: self.pick.hover(),
            points,
        })
    }
}

#[cfg(test)]
mod tests;
