//! The model shown for a document, its mesh and its finished sketches' lines,
//! fed by the regeneration side.

use std::cell::OnceCell;
use std::collections::VecDeque;
use std::sync::Arc;

use varde_document::{
    BodyId, Document, Editor, FeatureId, FeatureKind, Generation, Operation, Placement, Plane,
    Snapshot,
};
use varde_kernel::{RenderLines, RenderMesh};
use varde_regen::{
    Draft, Drafted, ErrorGeometry, FeatureFailure, Inspect, InspectPick, Inspected, Picking,
    Request, Response, Transport,
};
use varde_view::{MeshStatus, PickIndex};

/// The mesh and sketch lines shown for the document. They're built by the
/// regeneration side, so they may lag behind the editor: the last ones stay
/// shown until newer ones arrive.
#[derive(Default)]
pub(crate) struct MeshFeed {
    /// Builds the meshes: the document's lane, a thread natively and a Web
    /// Worker on the web. `None` until the lane has started, see
    /// [`Varde::regen_lane`]; nothing is requested until then.
    ///
    /// [`Varde::regen_lane`]: crate::Varde::regen_lane
    regen: Option<Box<dyn Transport<Request>>>,
    mesh: Arc<RenderMesh>,
    /// `mesh`'s picking tables: the body each of its parts is of, its
    /// faces' keys and summaries, its edges' tangent chains.
    picking: Arc<Picking>,
    /// Counts the models shown, up as `mesh` or `picking` changes: what
    /// picks name the model by ([`varde_view::Pick::model`]).
    model: u64,
    /// `mesh` and `picking` made ready for picking, once it's asked for.
    index: OnceCell<PickIndex>,
    /// The visible sketches' curves, of the same generation as `mesh`.
    sketches: Arc<RenderLines>,
    /// The sketches that don't solve, of the same generation as `mesh`.
    unsolved: Vec<FeatureId>,
    /// The features that failed and why, of the same generation as
    /// `mesh`, in the document's order.
    failed_features: Vec<FeatureFailure>,
    /// Each join, cut or intersect that got as far as its tool, with the
    /// bodies it touches, of the same generation as `mesh`, in the
    /// document's order.
    touched_features: Vec<(FeatureId, Vec<BodyId>)>,
    /// Each body a join merged into another, and the body holding it, of
    /// the same generation as `mesh`, in the document's order.
    merged_bodies: Vec<(BodyId, BodyId)>,
    /// The bodies that have a solid, shown or not, of the same generation
    /// as `mesh`, in the order they were made.
    solid_bodies: Vec<BodyId>,
    /// Where each sketch on a face that was placed is, of the same
    /// generation as `mesh`, in the document's order: those that failed
    /// aren't listed.
    placements: Vec<(FeatureId, Placement)>,
    /// The document `mesh` is of, if it's known (see `asked`): a
    /// placement is given out only for a sketch on the plane it had
    /// there, so one an undo has put on another face since isn't drawn
    /// or edited at the old face's place.
    shown_document: Option<Snapshot>,
    /// The documents asked about whose models may yet come, oldest first,
    /// at most [`MAX_ASKED`]: an answer whose document has been let go of
    /// gives out no placements.
    asked: VecDeque<(Generation, Snapshot)>,
    /// Tags the next [`Request::Export`].
    next_export: u64,
    /// The generation the document was last replaced whole by, if it was,
    /// see [`MeshFeed::replaced`]: the ids in `unsolved`,
    /// `failed_features`, `touched_features` and `merged_bodies` of an
    /// older answer may name other features or bodies now, so they aren't
    /// given out.
    replaced: Option<Generation>,
    /// How the draft of the model shown went, if it had one.
    drafted: Option<Drafted>,
    /// The bodies the newest draft answered that ran the touch test
    /// touches, and that draft's revision: kept across answers that
    /// didn't run it, so the panel's list doesn't empty while a draft
    /// fails before its tool exists, or while New body is picked.
    touched: Option<(u64, Vec<BodyId>)>,
    /// The first revision of the current run of drafts: those of one
    /// extrude set up without a pause. A run starts when a draft is
    /// asked for after none, or for another feature; touched bodies
    /// answered for an earlier run aren't listed.
    run: u64,
    /// What `mesh` is of, or `None` before the first one arrives.
    shown: Option<Asked>,
    /// What was asked for last.
    requested: Option<Asked>,
    /// The draft asked for last, if any: another one is given the next
    /// revision.
    draft: Option<Draft>,
    /// The revision the last draft was given: drafts are counted up
    /// across the document's life, so no two differing ones share one.
    revision: u64,
    /// What was asked for, at least as new as `shown`, whose model
    /// couldn't be built, and why.
    failed: Option<(Asked, String)>,
    /// The measure asked for last, if any: other picks are given the next
    /// revision.
    inspect: Option<Inspect>,
    /// The revision the last measure was given, counted up across the
    /// document's life as drafts' are.
    inspect_revision: u64,
    /// What the measure asked with the model shown came to, if it had
    /// one: its places name entries of `picking`.
    inspected: Option<Inspected>,
    /// The newest model shown without a draft, if there's been one: what
    /// a save's thumbnail shows, see [`MeshFeed::committed`].
    committed: Option<Committed>,
}

/// A model shown without a draft: its mesh, its picking tables for the
/// bodies of its parts, and what it's of.
struct Committed {
    mesh: Arc<RenderMesh>,
    picking: Arc<Picking>,
    generation: Generation,
}

/// How many of the documents asked about [`MeshFeed`] keeps for their
/// answers: the lane answers the one it's working on and the newest, so
/// more pile up only while it's slower than the edits, and an answer
/// that old gives out no placements.
const MAX_ASKED: usize = 64;

/// What a request asks for, and so what its answer is of: a generation
/// of the document, the sketch left out of the lines, the revision of the
/// draft applied, if any, and the revision of the measure taken on it, if
/// any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Asked {
    generation: Generation,
    exclude: Option<FeatureId>,
    draft: Option<u64>,
    inspect: Option<u64>,
}

impl Asked {
    /// What `response` answers, if it's a model's: `None` for an export's.
    fn of(response: &Response) -> Option<Self> {
        Some(Self {
            generation: response.generation()?,
            exclude: response.exclude(),
            draft: response.draft(),
            inspect: response.inspect(),
        })
    }
}

impl MeshFeed {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Starts sending requests to `lane`, the document's regeneration
    /// lane, once it has started. What to ask for is up to the next
    /// [`MeshFeed::request_with`].
    pub(crate) fn connect(&mut self, lane: impl Transport<Request> + 'static) {
        self.regen = Some(Box::new(lane));
    }

    /// Whether the lane has started, see [`MeshFeed::connect`].
    #[cfg(test)]
    pub(crate) fn connected(&self) -> bool {
        self.regen.is_some()
    }

    /// [`MeshFeed::request_with`] without a draft.
    #[cfg(test)]
    pub(crate) fn request(&mut self, editor: &Editor, exclude: Option<FeatureId>) {
        self.request_with(editor, exclude, None, None);
    }

    /// Asks the lane for the document's mesh, leaving the sketch `exclude`
    /// out of the lines, with `draft` applied, the feature being set up
    /// (an extrude, a revolve) and the feature it's edited from, if any:
    /// if the editor moved on since the last request, the sketch to leave
    /// out changed, as it does on entering or leaving one, or the draft
    /// did. A draft differing
    /// from the one asked for last is given the next revision; without
    /// one, the model is asked for again without the last. Likewise
    /// `inspect`, the measure tool's picks, measured on the model
    /// answered: other picks than those asked for last are given the next
    /// revision, and none ask again without a measure. Nothing, not
    /// even remembering it, before the lane has started: then the first
    /// request asks for the editor's newest.
    pub(crate) fn request_with(
        &mut self,
        editor: &Editor,
        exclude: Option<FeatureId>,
        draft: Option<(Option<FeatureId>, FeatureKind)>,
        inspect: Option<(InspectPick, Option<InspectPick>)>,
    ) {
        let Some(regen) = &mut self.regen else {
            return;
        };
        let draft = draft.map(|(feature, kind)| match &self.draft {
            Some(last) if last.feature == feature && last.kind == kind => last.clone(),
            last => {
                // One per change of a draft: a u64 won't run out.
                self.revision += 1;
                if last.as_ref().is_none_or(|last| last.feature != feature) {
                    self.run = self.revision;
                }
                Draft {
                    revision: self.revision,
                    feature,
                    kind,
                }
            }
        });
        let inspect = inspect.map(|(first, second)| match &self.inspect {
            Some(last) if last.first == first && last.second == second => last.clone(),
            _ => {
                // One per change of the picks: a u64 won't run out.
                self.inspect_revision += 1;
                Inspect {
                    revision: self.inspect_revision,
                    first,
                    second,
                }
            }
        });
        let asked = Asked {
            generation: editor.generation(),
            exclude,
            draft: draft.as_ref().map(|draft| draft.revision),
            inspect: inspect.as_ref().map(|inspect| inspect.revision),
        };
        self.draft = draft.clone();
        self.inspect = inspect.clone();
        if self.requested.is_none_or(|requested| {
            requested.generation < asked.generation
                || (requested.generation == asked.generation && requested != asked)
        }) {
            self.requested = Some(asked);
            let document = editor.snapshot();
            if (self.asked.back()).is_none_or(|&(newest, _)| newest < asked.generation) {
                if self.asked.len() == MAX_ASKED {
                    self.asked.pop_front();
                }
                self.asked.push_back((asked.generation, document.clone()));
            }
            regen.send(Request::Regenerate {
                generation: asked.generation,
                document,
                exclude,
                draft: draft.map(Box::new),
                inspect: inspect.map(Box::new),
            });
        }
    }

    /// Asks the lane to weld the visible bodies of the editor's committed
    /// document for export, returning the tag its
    /// [`Response::Exported`] will carry: `None` before the lane has
    /// started. Answered even if regenerations follow: the lane queues
    /// exports rather than replacing them.
    pub(crate) fn request_export(&mut self, editor: &Editor) -> Option<u64> {
        let regen = self.regen.as_mut()?;
        let export = self.next_export;
        // One per click: a u64 won't run out.
        self.next_export += 1;
        regen.send(Request::Export {
            export,
            document: editor.snapshot(),
        });
        Some(export)
    }

    /// Shows the model in `response`, or its error next to the last one,
    /// unless a response as new was applied already. Responses arriving out
    /// of order or superseded are dropped, and so are exports' answers,
    /// which are the document's to take (see `Doc::computed`).
    pub(crate) fn apply(&mut self, response: Response) {
        let Some(asked) = Asked::of(&response) else {
            return;
        };
        if !self.wanted(asked) {
            return;
        }
        match response {
            Response::Regenerated {
                mesh,
                picking,
                sketches,
                unsolved,
                failed,
                touched,
                merged,
                draft,
                placements,
                bodies,
                inspected,
                ..
            } => {
                // The lane hands an unchanged model back as the same
                // `Arc`s, natively; the web worker's come anew, so they're
                // compared, far cheaper than building the index again
                // (and a difference shows early). Unchanged, its index
                // stays and what's selected needn't be looked for again.
                fn same<T: PartialEq>(a: &Arc<T>, b: &Arc<T>) -> bool {
                    Arc::ptr_eq(a, b) || a == b
                }
                if !(same(&self.mesh, &mesh) && same(&self.picking, &picking)) {
                    self.model = self.model.wrapping_add(1);
                    self.index = OnceCell::new();
                }
                self.mesh = mesh;
                self.solid_bodies = bodies.into_iter().map(|(body, _)| body).collect();
                self.picking = picking;
                self.sketches = sketches;
                self.unsolved = unsolved;
                self.failed_features = failed;
                self.touched_features = touched;
                self.merged_bodies = merged;
                self.placements = placements;
                // Older documents' models won't be shown any more.
                (self.asked).retain(|&(generation, _)| generation >= asked.generation);
                self.shown_document = (self.asked.front())
                    .filter(|&&(generation, _)| generation == asked.generation)
                    .map(|(_, document)| document.clone());
                if let Some(Drafted {
                    revision,
                    touched: Some(touched),
                    ..
                }) = draft.as_deref()
                    && self
                        .touched
                        .as_ref()
                        .is_none_or(|(kept, _)| kept <= revision)
                {
                    self.touched = Some((*revision, touched.clone()));
                }
                if draft.is_none() {
                    self.committed = Some(Committed {
                        mesh: self.mesh.clone(),
                        picking: self.picking.clone(),
                        generation: asked.generation,
                    });
                }
                self.drafted = draft.map(|draft| *draft);
                self.inspected = inspected.map(|inspected| *inspected);
                self.shown = Some(asked);
                self.failed = None;
            }
            Response::Failed { error, .. } => self.failed = Some((asked, error)),
            Response::Exported { .. } => {}
        }
    }

    /// Whether `response` is newer than what was applied: of a newer
    /// generation, or of the same one answering the request asked last,
    /// which left out another sketch than the answer applied, model or
    /// failure. Once a request failed, nothing more of it is taken, but a
    /// request of the same generation leaving out another sketch, entering
    /// or leaving one, is answered as usual.
    fn wanted(&self, asked: Asked) -> bool {
        let Some(answered) = self.answered() else {
            return true;
        };
        if asked.generation != answered {
            return asked.generation > answered;
        }
        // Of the same generation: only the answer to what was asked last,
        // the same sketch left out and the same draft applied, once.
        let failed = self
            .failed
            .as_ref()
            .is_some_and(|(failed, _)| *failed == asked);
        self.requested.is_some_and(|requested| {
            (requested.exclude, requested.draft, requested.inspect)
                == (asked.exclude, asked.draft, asked.inspect)
        }) && self.shown != Some(asked)
            && !failed
    }

    /// The newest generation a response was applied for.
    fn answered(&self) -> Option<Generation> {
        self.last_answered().map(|asked| asked.generation)
    }

    /// What the response applied last answered, model or failure.
    fn last_answered(&self) -> Option<Asked> {
        self.failed.as_ref().map(|(asked, _)| *asked).or(self.shown)
    }

    /// Why the draft asked for last fails, once its answer is shown: none
    /// if it goes, it's not answered yet, or there's none.
    pub(crate) fn draft_error(&self) -> Option<&str> {
        let revision = self.draft.as_ref()?.revision;
        let drafted = self.drafted.as_ref()?;
        (drafted.revision == revision)
            .then_some(drafted.error.as_deref())
            .flatten()
    }

    /// The bodies the draft asked for last, a cut that works, takes
    /// nothing from, once its answer is shown ([`Drafted::uncut`]).
    pub(crate) fn draft_uncut(&self) -> &[BodyId] {
        match (&self.draft, &self.drafted) {
            (Some(draft), Some(drafted)) if drafted.revision == draft.revision => &drafted.uncut,
            _ => &[],
        }
    }

    /// What to draw of where the draft asked for last fails, when
    /// [`MeshFeed::draft_error`] gives why and the failure has geometry.
    pub(crate) fn draft_geometry(&self) -> Option<&Arc<ErrorGeometry>> {
        let revision = self.draft.as_ref()?.revision;
        let drafted = self.drafted.as_ref()?;
        (drafted.revision == revision && drafted.error.is_some())
            .then_some(drafted.geometry.as_ref())
            .flatten()
    }

    /// Whether the model shown has a draft of the current run of drafts
    /// (see `run`), answered or failing: the draft's failure, not the
    /// committed feature's, is then what the edited feature's is.
    pub(crate) fn draft_shown(&self) -> bool {
        self.draft.is_some()
            && (self.drafted.as_ref()).is_some_and(|drafted| drafted.revision >= self.run)
    }

    /// Whether the model shown is of a draft of the current run of drafts
    /// asked for, while one is: not one of a draft since let go of, nor
    /// the document's alone.
    pub(crate) fn shows_draft_of_run(&self) -> bool {
        self.draft.is_some()
            && (self.shown)
                .and_then(|shown| shown.draft)
                .is_some_and(|revision| revision >= self.run)
    }

    /// Where the newest draft answered of the current run of drafts found
    /// its axis or plane, a move's or a mirror's, if it did
    /// ([`Drafted::reference`]): a point on it and its direction (a
    /// plane's normal), while a draft is asked for.
    pub(crate) fn draft_reference(&self) -> Option<[glam::DVec3; 2]> {
        if !self.draft_shown() {
            return None;
        }
        let [point, along] = *self.drafted.as_ref()?.reference.as_deref()?;
        Some([point.into(), along.into()])
    }

    /// What the newest draft answered of the current run of drafts found
    /// of an align's references, if it's an align's
    /// ([`Drafted::datums`]), while a draft is asked for.
    pub(crate) fn draft_datums(&self) -> Option<varde_regen::AlignDatums> {
        if !self.draft_shown() {
            return None;
        }
        self.drafted.as_ref()?.datums.as_deref().copied()
    }

    /// What the newest draft answered of the current run of drafts found
    /// of a scale (its point, its edge's length, its factors and the
    /// fitted faces), if it's a scale's ([`Drafted::scale`]), while a
    /// draft is asked for.
    pub(crate) fn draft_scale(&self) -> Option<varde_regen::ScaleFound> {
        if !self.draft_shown() {
            return None;
        }
        self.drafted.as_ref()?.scale.as_deref().copied()
    }

    /// Whether the model shown answers what was asked last: picks on it
    /// are of the document, and the draft, as they're set up now. What
    /// was measured on it doesn't count: another measure asks for the
    /// same model, so a selection changed just before doesn't hold picks
    /// back until its measures come.
    pub(crate) fn answers_request(&self) -> bool {
        let model = |asked: Asked| (asked.generation, asked.exclude, asked.draft);
        self.requested.is_some() && self.shown.map(model) == self.requested.map(model)
    }

    /// The bodies the draft's solid touches, as the newest answer of the
    /// current run of drafts that ran the touch test found, while a draft
    /// is asked for: kept while a changed draft is on its way, and while
    /// one fails before its tool exists or makes a new body, so the
    /// panel's list doesn't blink. Empty until the run's first such
    /// answer, so another extrude's bodies aren't listed.
    pub(crate) fn draft_touched(&self) -> &[BodyId] {
        match (&self.draft, &self.touched) {
            (Some(_), Some((revision, touched))) if *revision >= self.run => touched,
            _ => &[],
        }
    }

    /// The draft revision whose answer [`MeshFeed::draft_touched`] gives,
    /// if it gives one.
    pub(crate) fn draft_touched_revision(&self) -> Option<u64> {
        match (&self.draft, &self.touched) {
            (Some(_), Some((revision, _))) if *revision >= self.run => Some(*revision),
            _ => None,
        }
    }

    /// What the measure asked for last came to on the model shown, once
    /// an answer for those picks is shown: none while it's on its way
    /// (after an edit, the same picks' answer on the model before shows
    /// until the new one comes, as the model does), or without a measure.
    pub(crate) fn inspected(&self) -> Option<&Inspected> {
        let revision = self.inspect.as_ref()?.revision;
        (self.inspected.as_ref()).filter(|inspected| inspected.revision == revision)
    }

    /// The newest answer's measures, if they're of `picks`.
    pub(crate) fn inspected_of(
        &self,
        (first, second): (InspectPick, Option<InspectPick>),
    ) -> Option<&Inspected> {
        let asked = self.inspect.as_ref()?;
        (asked.first == first && asked.second == second)
            .then(|| self.inspected())
            .flatten()
    }

    /// The revision the newest draft was given: every later draft gets a
    /// higher one.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// How the mesh shown stands against the editor's document. A failure
    /// of an older generation than the editor's isn't reported: that document
    /// is gone, and the current one is still being built. Nor is the model
    /// shown current while another draft than its, or none, is asked for:
    /// a preview cancelled or changed still shows until the answer.
    pub(crate) fn status(&self, editor: &Editor) -> MeshStatus<'_> {
        let redrafted =
            self.last_answered()
                .zip(self.requested)
                .is_some_and(|(last, requested)| {
                    last.generation == requested.generation && last.draft != requested.draft
                });
        if redrafted
            || self
                .answered()
                .is_none_or(|answered| answered < editor.generation())
        {
            return MeshStatus::Regenerating;
        }
        match &self.failed {
            Some((.., error)) => MeshStatus::Failed(error),
            None => MeshStatus::Current,
        }
    }

    /// Whether the model shown has a solid for a body `document` shows:
    /// one of the bodies an export would write, if the model is current.
    pub(crate) fn shows_a_body(&self, document: &Document) -> bool {
        (self.solid_bodies.iter()).any(|&body| document.body(body).is_some_and(|body| body.visible))
    }

    /// The generation of the mesh shown, if there is one yet.
    pub(crate) fn generation(&self) -> Option<Generation> {
        self.shown.map(|shown| shown.generation)
    }

    /// The sketch left out of the lines shown, if there are lines yet.
    #[cfg(test)]
    pub(crate) fn left_out(&self) -> Option<Option<FeatureId>> {
        self.shown.map(|shown| shown.exclude)
    }

    /// The revision of the draft applied to the model shown, if there's
    /// a model and it had one.
    #[cfg(test)]
    pub(crate) fn shown_draft(&self) -> Option<u64> {
        self.shown.and_then(|shown| shown.draft)
    }

    pub(crate) fn mesh(&self) -> &Arc<RenderMesh> {
        &self.mesh
    }

    /// Whether the model shown has a draft applied: the preview of the
    /// extrude being set up, or of one that just ended, until the answer
    /// without it comes.
    pub(crate) fn shows_draft(&self) -> bool {
        self.shown.is_some_and(|shown| shown.draft.is_some())
    }

    /// The mesh of the newest model shown without a draft, the last the
    /// committed document regenerated to, with the body each of its parts
    /// is of: none once the document was replaced whole until a model of
    /// it shows, as for [`MeshFeed::parts`]. `None` before there's been
    /// one.
    pub(crate) fn committed(&self) -> Option<(&Arc<RenderMesh>, &[BodyId])> {
        let committed = self.committed.as_ref()?;
        let current = self
            .replaced
            .is_none_or(|replaced| committed.generation >= replaced);
        let parts = if current {
            committed.picking.bodies()
        } else {
            &[]
        };
        Some((&committed.mesh, parts))
    }

    /// Counts the models shown: see [`varde_view::Pick::model`].
    pub(crate) fn model(&self) -> u64 {
        self.model
    }

    /// The model shown made ready for picking, built the first time it's
    /// asked for.
    pub(crate) fn pick_index(&self) -> &PickIndex {
        self.index
            .get_or_init(|| PickIndex::new(self.mesh.clone(), self.picking.clone(), self.model))
    }

    /// The body each of [`mesh`](Self::mesh)'s parts is of, in order.
    /// None after the document is replaced, whose ids may name other
    /// bodies, until a model of it shows: the parts are drawn opaque.
    pub(crate) fn parts(&self) -> &[BodyId] {
        if self.marks() {
            self.picking.bodies()
        } else {
            &[]
        }
    }

    pub(crate) fn sketches(&self) -> &Arc<RenderLines> {
        &self.sketches
    }

    /// Notes that the document was replaced whole (restoring recovered
    /// changes, or undoing or redoing that) as of `generation`, the
    /// editor's: the features of models shown from before name features by
    /// the ids of the document replaced, which may name others now, so
    /// none are marked until a model of `generation` or newer is shown.
    pub(crate) fn replaced(&mut self, generation: Generation) {
        self.replaced = Some(generation);
    }

    /// Whether the model shown is of a document since replaced whole,
    /// whose ids may name other things now, see [`MeshFeed::replaced`].
    pub(crate) fn predates_replacement(&self) -> bool {
        matches!((self.shown, self.replaced),
            (Some(shown), Some(replaced)) if shown.generation < replaced)
    }

    /// Whether the features the model shown found name features of the
    /// document as it is, see [`MeshFeed::replaced`].
    fn marks(&self) -> bool {
        match (self.shown, self.replaced) {
            (Some(shown), Some(replaced)) => shown.generation >= replaced,
            (shown, _) => shown.is_some(),
        }
    }

    /// Where the sketch on a face `feature`, now on `plane`, is, as the
    /// model shown placed it: with a draft, as the document with the
    /// draft applied did. None if it wasn't placed, the model shown
    /// doesn't know it, had it on another plane (an undo or redo of a
    /// change of plane since) or its document isn't known, or the
    /// document was replaced since.
    pub(crate) fn placement(&self, feature: FeatureId, plane: &Plane) -> Option<Placement> {
        let on_plane = self.shown_document.as_ref().is_some_and(|document| {
            matches!(document.feature(feature).map(|feature| &feature.kind),
                Some(FeatureKind::Sketch { plane: shown, .. }) if shown == plane)
        });
        let placements = if self.marks() && on_plane {
            &self.placements[..]
        } else {
            &[]
        };
        (placements.iter())
            .find(|(placed, _)| *placed == feature)
            .map(|&(_, placement)| placement)
    }

    /// The sketches that don't solve, as the model shown found, unless
    /// the document was replaced since.
    pub(crate) fn unsolved(&self) -> &[FeatureId] {
        if self.marks() { &self.unsolved } else { &[] }
    }

    /// The features that failed and why, as the model shown found: with
    /// a draft, those of the document with the draft applied. None if the
    /// document was replaced since.
    pub(crate) fn failed_features(&self) -> &[FeatureFailure] {
        if self.marks() {
            &self.failed_features
        } else {
            &[]
        }
    }

    /// Each join, cut or intersect that got as far as its tool, with the
    /// bodies it touches, as the model shown found: with a draft, those
    /// of the document with the draft applied. None if the document was
    /// replaced since.
    pub(crate) fn touched_features(&self) -> &[(FeatureId, Vec<BodyId>)] {
        if self.marks() {
            &self.touched_features
        } else {
            &[]
        }
    }

    /// Each body a join merged into another (*consumed*), with the body
    /// holding it now, as the model shown found: with a draft, as the
    /// document with the draft applied merges them. None if the document
    /// was replaced since.
    pub(crate) fn merged_bodies(&self) -> &[(BodyId, BodyId)] {
        if self.marks() {
            &self.merged_bodies
        } else {
            &[]
        }
    }
    /// The bodies the joins and combines of `document` before `until`
    /// (all of them without one) merged into others, in the document's
    /// order, as the model shown found them: what
    /// [`MeshFeed::merged_bodies`] would be with the history stopped
    /// there. A join merges the bodies the model shown found it touch, if
    /// it touched two or more and didn't fail; one the model shown doesn't
    /// know (added since, or a new extrude's draft) merges nothing. A
    /// combine that uses its tools up merges them into its target unless
    /// it failed, as regen's own rule has it.
    pub(crate) fn merged_before(&self, document: &Document, until: Option<FeatureId>) -> Merges {
        let mut merges = Merges::default();
        for feature in document.features() {
            if Some(feature.id) == until {
                break;
            }
            match &feature.kind {
                FeatureKind::Combine(combine) => {
                    if self.consumes(document, feature.id) {
                        let bodies: Vec<BodyId> = combine.bodies().collect();
                        merges.join(&bodies);
                    }
                }
                _ if self.merges(document, feature.id) => {
                    let touched = (self.touched_features().iter())
                        .find(|(touched, _)| *touched == feature.id);
                    if let Some((_, touched)) = touched {
                        merges.join(touched);
                    }
                }
                _ => {}
            }
        }
        merges
    }

    /// Whether `feature` of `document` is a join the model shown has
    /// working: one that merges the bodies it touches.
    pub(crate) fn merges(&self, document: &Document, feature: FeatureId) -> bool {
        let join = document
            .feature(feature)
            .is_some_and(|feature| matches!(feature.kind.operation(), Some(Operation::Join(_))));
        join && !self.failed(feature)
    }

    /// Whether `feature` of `document` is a combine using its tools up
    /// that the model shown didn't find failing: one that merges them
    /// into its target.
    pub(crate) fn consumes(&self, document: &Document, feature: FeatureId) -> bool {
        let consuming = document.feature(feature).is_some_and(
            |feature| matches!(&feature.kind, FeatureKind::Combine(combine) if !combine.keep_tools),
        );
        consuming && !self.failed(feature)
    }

    /// Whether the model shown found `feature` failing.
    fn failed(&self, feature: FeatureId) -> bool {
        (self.failed_features().iter()).any(|failed| failed.feature == feature)
    }
}

/// Which bodies joins and combines merged into which, replayed in the
/// document's order by regen's own rule ([`varde_regen::note_merge`]): a
/// join touching two or more merges them into the first made (the
/// *holder*), a combine using its tools up merges them into its target,
/// and a body merged into one that's merged later moves on to the later
/// holder.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Merges(Vec<(BodyId, BodyId)>);

impl Merges {
    /// Notes a working join that touched `touched`, in the order they
    /// were made, or a combine using its tools up: its target, then its
    /// tools.
    pub(crate) fn join(&mut self, touched: &[BodyId]) {
        varde_regen::note_merge(&mut self.0, touched);
    }

    /// The body `body` was merged into, if it was.
    pub(crate) fn holder(&self, body: BodyId) -> Option<BodyId> {
        (self.0.iter())
            .find(|(consumed, _)| *consumed == body)
            .map(|&(_, holder)| holder)
    }

    /// The bodies merged into `holder`.
    pub(crate) fn held_by(&self, holder: BodyId) -> impl Iterator<Item = BodyId> + '_ {
        (self.0.iter())
            .filter(move |(_, held_in)| *held_in == holder)
            .map(|(consumed, _)| *consumed)
    }
}

#[cfg(test)]
mod tests;
