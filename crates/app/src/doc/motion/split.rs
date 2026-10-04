//! A split in the move's session ([`MotionKind::Split`]): its body picked
//! as an align's (one body; a click on another replaces it), then what it
//! splits with, by its "Split with" tiles: a plane or face (an origin
//! plane from the toolbar, or a face of the model clicked, flat as a
//! plane and curved as the face's surface, named as of the feature by
//! [`Naming`](varde_view::Naming)), another body clicked, regions of a sketch (picked as an
//! extrude's, [`RegionPick`]) or the curves of an open line of one,
//! clicked in the viewport. Each tile keeps its own tool while another is
//! shown. Then which piece keeps the body's id (Front or Back) and which
//! pieces stay (both, or one as a trim). What's picked the document no
//! longer takes is kept and said to be gone; the preview's pieces are
//! tinted apart and labelled.

use std::borrow::Cow;

use glam::DVec3;
use varde_document::{
    BodyId, Document, Feature, FeatureId, FeatureKind, Keep, MAX_EXTRUDE_REGIONS, MAX_SPLIT_CURVES,
    PlaneRef, Side, Split, SplitTool,
};
use varde_regen::Summary;
use varde_sketch::{Id, RegionRef};
use varde_view::{
    MotionKind, MotionPick, Pick, Picked, SketchLines, SplitMode, SplitPiece, SplitView, Unnamed,
    plane_name, split_info,
};

use super::{Doc, MotionSession, OUT_OF_DATE, unnamed};
use crate::doc::combine::pickable;
use crate::doc::feed::Merges;
use crate::doc::regions::RegionPick;

/// A split's tool as picked for each of its "Split with" tiles, and
/// which pieces it keeps.
#[derive(Debug, Clone)]
pub(crate) struct SplitSetup {
    pub(crate) mode: SplitMode,
    /// A plane or face clicked, or an origin plane: [`SplitTool::Plane`]
    /// or [`SplitTool::Face`].
    pub(crate) surface: Option<SplitTool>,
    /// Another body.
    pub(crate) body: Option<BodyId>,
    /// Regions of a sketch.
    pub(crate) regions: RegionPick,
    /// The curves of a line, sorted, and their sketch: none picked, none.
    pub(crate) chain: Option<(FeatureId, Vec<Id>)>,
    pub(crate) original: Side,
    pub(crate) keep: Keep,
    /// Whether the tool of `mode` is gone: the document no longer takes
    /// it at the feature's place (an undo took its body, face's maker or
    /// sketch away, or a curve of its line), as of the last change. Kept,
    /// said to be gone, until another is picked or it's back.
    gone: bool,
    /// Regions picked whose sketch is gone (an undo took it away), by
    /// their sketch: put by while the visible sketches' regions are
    /// offered in their place, and picked again if their sketch comes
    /// back before others are picked.
    stale_regions: Option<(FeatureId, Vec<RegionRef>)>,
}

impl Default for SplitSetup {
    fn default() -> Self {
        Self {
            mode: SplitMode::Face,
            surface: None,
            body: None,
            regions: RegionPick::new(None, MAX_EXTRUDE_REGIONS),
            chain: None,
            original: Side::Front,
            keep: Keep::Both,
            gone: false,
            stale_regions: None,
        }
    }
}

impl SplitSetup {
    /// The tool and options of `split`, a feature of `document`.
    pub(super) fn of(document: &Document, split: &Split) -> Self {
        let mut setup = Self {
            original: split.original,
            keep: split.keep,
            ..Self::default()
        };
        match &split.tool {
            tool @ (SplitTool::Plane(_) | SplitTool::Face(_)) => {
                setup.mode = SplitMode::Face;
                setup.surface = Some(tool.clone());
            }
            SplitTool::Body(body) => {
                setup.mode = SplitMode::Body;
                setup.body = Some(*body);
            }
            SplitTool::Regions { sketch, regions } => {
                setup.mode = SplitMode::Regions;
                setup.regions =
                    RegionPick::editing(document, *sketch, regions, MAX_EXTRUDE_REGIONS);
            }
            SplitTool::Chain { sketch, curves } => {
                setup.mode = SplitMode::Line;
                setup.chain = Some((*sketch, curves.clone()));
            }
        }
        setup
    }

    /// The tool picked for its mode, if one is.
    pub(super) fn tool(&self) -> Option<SplitTool> {
        match self.mode {
            SplitMode::Face => self.surface.clone(),
            SplitMode::Body => self.body.map(SplitTool::Body),
            SplitMode::Regions => {
                let regions = self.regions.references();
                let sketch = self.regions.source?;
                (!regions.is_empty()).then(|| SplitTool::Regions {
                    sketch,
                    regions: regions.to_vec(),
                })
            }
            SplitMode::Line => {
                let (sketch, curves) = self.chain.clone()?;
                (!curves.is_empty()).then_some(SplitTool::Chain { sketch, curves })
            }
        }
    }

    /// Whether `document` no longer takes the tool of its mode at feature
    /// `index`: its body, face's maker or sketch gone or not before the
    /// feature, a curve of its line gone, or its regions' sketch gone.
    fn tool_gone(&self, document: &Document, index: usize) -> bool {
        if self.mode == SplitMode::Regions && self.stale_regions.is_some() {
            return true;
        }
        let held = |body: BodyId| document.body(body).is_some();
        self.tool().is_some_and(|tool| {
            let body = match &tool {
                SplitTool::Plane(PlaneRef::Face(face)) | SplitTool::Face(face) => Some(face.body),
                SplitTool::Body(body) => Some(*body),
                _ => None,
            };
            let curves = match &tool {
                SplitTool::Chain { sketch, curves } => {
                    match document.feature(*sketch).map(|feature| &feature.kind) {
                        Some(FeatureKind::Sketch { sketch, .. }) => {
                            curves.iter().all(|&curve| sketch.curve(curve).is_some())
                        }
                        _ => false,
                    }
                }
                _ => true,
            };
            document.check_split_tool(index, &tool).is_err()
                || body.is_some_and(|body| !held(body))
                || !curves
        })
    }

    /// Picks the curve `curve` of `sketch` for its line, or takes it out:
    /// only of the line's sketch, or of any while none is picked. At most
    /// [`MAX_SPLIT_CURVES`].
    fn toggle_curve(&mut self, sketch: FeatureId, curve: Id) {
        match &mut self.chain {
            Some((of, curves)) if *of == sketch => match curves.binary_search(&curve) {
                Ok(at) => {
                    curves.remove(at);
                    if curves.is_empty() {
                        self.chain = None;
                    }
                }
                Err(at) if curves.len() < MAX_SPLIT_CURVES => curves.insert(at, curve),
                Err(_) => {}
            },
            Some(_) => {}
            None => self.chain = Some((sketch, vec![curve])),
        }
    }
}

impl MotionSession {
    /// The split as set up, if it's whole: its body and the tool of its
    /// mode. A new body stands for the piece the body doesn't keep while
    /// it keeps both, which the document gives an id.
    pub(super) fn split(&self) -> Option<Split> {
        let setup = &self.split;
        Some(Split {
            body: *self.bodies.first()?,
            tool: setup.tool()?,
            original: setup.original,
            keep: setup.keep,
            new_body: (setup.keep == Keep::Both).then_some(BodyId::NEW),
        })
    }

    /// What's still to be done before it can be committed, the words for
    /// the status bar: its body, and its tool.
    pub(super) fn split_need(&self) -> Option<&'static str> {
        if self.bodies.is_empty() {
            return Some("pick the body to split");
        }
        if self.split.tool().is_some() {
            return None;
        }
        Some(match self.split.mode {
            SplitMode::Face => "pick a plane or a face to split with",
            SplitMode::Body => "pick a body to split with",
            SplitMode::Regions => "pick the regions of a sketch to split with",
            SplitMode::Line => "pick the curves of a line to split with",
        })
    }

    /// The words for its tool being gone, if it is.
    pub(super) fn split_gone(&self) -> Option<&'static str> {
        let stale = self.split.mode == SplitMode::Regions && self.split.stale_regions.is_some();
        if !self.split.gone || (self.split.tool().is_none() && !stale) {
            return None;
        }
        Some(match self.split.mode {
            SplitMode::Face => "The plane or face is gone: pick another",
            SplitMode::Body => "The tool body is gone: pick another",
            SplitMode::Regions => "The regions' sketch is gone: pick other regions",
            SplitMode::Line => "The line is gone: pick its curves again",
        })
    }

    /// The feature's place in `document`: the edited one's, or the end.
    pub(super) fn index_in(&self, document: &Document) -> usize {
        let features = document.features();
        (self.feature)
            .and_then(|id| features.iter().position(|feature| feature.id == id))
            .unwrap_or(features.len())
    }

    /// Notes whether `document` no longer takes its tool at feature
    /// `index` ([`SplitSetup::gone`]), finding its sketch's regions
    /// again where they changed. Regions whose sketch is gone are put by
    /// ([`SplitSetup::stale_regions`]), the visible sketches' regions
    /// offered meanwhile, and picked again if their sketch comes back
    /// before others are picked.
    pub(super) fn prune_split(&mut self, document: &Document, index: usize) {
        if self.kind != MotionKind::Split {
            return;
        }
        let setup = &mut self.split;
        let is_sketch = |id: FeatureId| {
            matches!(
                document.feature(id).map(|feature| &feature.kind),
                Some(FeatureKind::Sketch { .. })
            )
        };
        if let Some(source) = setup.regions.source
            && !is_sketch(source)
        {
            setup.stale_regions = Some((source, setup.regions.references().to_vec()));
            setup.regions = RegionPick::new(None, MAX_EXTRUDE_REGIONS);
        } else if setup.regions.source.is_none()
            && let Some((source, regions)) = setup
                .stale_regions
                .take_if(|(source, _)| is_sketch(*source))
        {
            setup.regions = RegionPick::editing(document, source, &regions, MAX_EXTRUDE_REGIONS);
        }
        if setup.mode == SplitMode::Regions {
            setup.regions.refresh(document);
        }
        setup.gone = setup.tool_gone(document, index);
    }

    /// Moves its tool body on to the body holding it where `merges` (the
    /// merges before the feature) have it merged into another, its body
    /// already followed: whether it moved. One merged with the body split
    /// (either into the other, or both into a third) is let go of, to be
    /// picked again: a body can't split itself.
    pub(super) fn follow_split(&mut self, merges: &Merges) -> bool {
        let Some(body) = self.split.body else {
            return false;
        };
        let held = merges.holder(body).unwrap_or(body);
        if self.bodies.first() == Some(&held) {
            self.split.body = None;
            if self.split.mode == SplitMode::Body && self.picking == MotionPick::Nothing {
                self.picking = MotionPick::Tool;
            }
            return true;
        }
        self.split.body = Some(held);
        held != body
    }

    /// Sets what it splits with: picking that tool next unless it has
    /// one, finding the sketches' regions for regions.
    pub(super) fn split_mode(&mut self, mode: SplitMode, document: &Document) {
        self.split.mode = mode;
        if mode == SplitMode::Regions {
            self.split.regions.refresh(document);
        }
        // Whether the tile's own tool is gone, not the last one's.
        let index = self.index_in(document);
        self.split.gone = self.split.tool_gone(document, index);
        self.picking = if self.bodies.is_empty() {
            MotionPick::Bodies
        } else if self.split.tool().is_none() {
            MotionPick::Tool
        } else {
            MotionPick::Nothing
        };
    }

    /// Takes the origin plane `plane` as its tool while a plane or face is
    /// picked.
    pub(super) fn split_origin(&mut self, plane: varde_document::OriginPlane) {
        if self.picking != MotionPick::Tool || self.split.mode != SplitMode::Face {
            return;
        }
        self.split.surface = Some(SplitTool::Plane(PlaneRef::Origin(plane)));
        self.split.gone = false;
        self.picking = MotionPick::Nothing;
    }

    /// Whether `sketch` is a sketch of `document` the feature at its
    /// place (a split's, a sweep's, a loft's) can take: before the
    /// feature edited.
    pub(super) fn takes_sketch(&self, document: &Document, sketch: FeatureId) -> bool {
        let index = self.index_in(document);
        (document.features()[..index].iter()).any(|feature| {
            feature.id == sketch && matches!(feature.kind, FeatureKind::Sketch { .. })
        })
    }

    /// Picks the region `region` of `sketch` for its tool, or takes it
    /// out, while its regions are picked.
    pub(super) fn split_region(&mut self, sketch: FeatureId, region: usize, document: &Document) {
        if self.picking != MotionPick::Tool
            || self.split.mode != SplitMode::Regions
            || !self.takes_sketch(document, sketch)
        {
            return;
        }
        self.split.regions.toggle(sketch, region, false, document);
        if self.split.regions.source.is_some() {
            self.split.stale_regions = None;
            self.split.gone = false;
        }
    }

    /// Picks the curve `curve` of `sketch` for its line, or takes it out,
    /// while its line is picked.
    pub(super) fn split_curve(&mut self, sketch: FeatureId, curve: Id, document: &Document) {
        let has = matches!(
            document.feature(sketch).map(|feature| &feature.kind),
            Some(FeatureKind::Sketch { sketch, .. }) if sketch.curve(curve).is_some()
        );
        if self.picking != MotionPick::Tool
            || self.split.mode != SplitMode::Line
            || !has
            || !self.takes_sketch(document, sketch)
        {
            return;
        }
        if self.split.gone {
            // A line gone is picked again: of the curves still there,
            // or afresh where its sketch is gone.
            self.split.chain = (self.split.chain.take())
                .filter(|(of, _)| self.takes_sketch(document, *of))
                .and_then(|(of, mut curves)| {
                    let Some(FeatureKind::Sketch { sketch, .. }) =
                        document.feature(of).map(|feature| &feature.kind)
                    else {
                        return None;
                    };
                    curves.retain(|&curve| sketch.curve(curve).is_some());
                    (!curves.is_empty()).then_some((of, curves))
                });
        }
        self.split.toggle_curve(sketch, curve);
        let index = self.index_in(document);
        self.split.gone = self.split.tool_gone(document, index);
    }

    /// Whether its tool is picked in the viewport in its sketches, where
    /// the model isn't picked: regions or a line.
    /// A sweep's profile's regions likewise, and a loft's sections and
    /// rails.
    pub(crate) fn picks_sketches(&self) -> bool {
        let split = self.kind == MotionKind::Split
            && self.picking == MotionPick::Tool
            && matches!(self.split.mode, SplitMode::Regions | SplitMode::Line);
        let loft = self.kind == MotionKind::Loft
            && matches!(self.picking, MotionPick::Regions | MotionPick::Path);
        split || loft || (self.kind == MotionKind::Sweep && self.picking == MotionPick::Regions)
    }
}

/// Why a pick can't be a split's tool.
const NOT_A_FACE: &str = "Only a face or an origin plane can split a body here";
const SAME_BODY: &str = "That's the body being split: pick another body to split with";

impl Doc {
    /// `body` of the model shown as the body a feature being set up names:
    /// the body holding it before the feature where a join merged it,
    /// and the body a split at or after the feature cut it from.
    pub(super) fn named_body(&self, body: BodyId) -> BodyId {
        let Some(session) = &self.motion else {
            return body;
        };
        let document = self.editor.document();
        let merged = self.feed.merged_before(document, session.feature);
        let body = (self.motion_naming()).map_or(body, |naming| naming.unsplit(body));
        merged.holder(body).unwrap_or(body)
    }

    /// `pick` of the model shown as the tool of the split being set up,
    /// of the kind its mode picks: a face, flat as its plane, curved as its
    /// surface, named as the feature stores it; or the body it's on, any
    /// the feature can name but the one split. Refused, why, if not.
    pub(super) fn split_tool_of(&self, pick: Pick) -> Result<SplitTool, Cow<'static, str>> {
        let session = self.motion.as_ref().ok_or("Nothing is set up")?;
        let document = self.editor.document();
        match session.split.mode {
            SplitMode::Face => {
                let Picked::Face(face) = pick.target else {
                    return Err(NOT_A_FACE.into());
                };
                let naming = self.motion_naming().ok_or("Nothing is set up")?;
                let index = self.feed.pick_index();
                let named = naming
                    .checked_face_ref(index, face, pick.at)
                    .map_err(|why| unnamed(why, "face", MotionKind::Split))?;
                let summary = (index.picking().faces().get(face as usize)).map(|face| face.summary);
                Ok(match summary {
                    Some(Summary::Plane { .. }) => SplitTool::Plane(PlaneRef::Face(named)),
                    _ => SplitTool::Face(named),
                })
            }
            SplitMode::Body => {
                let body = self.named_body(pick.body);
                if session.bodies.first() == Some(&body) {
                    return Err(SAME_BODY.into());
                }
                if !pickable(document, body, session.feature) {
                    return Err(unnamed(Unnamed::Later, "body", MotionKind::Split));
                }
                Ok(SplitTool::Body(body))
            }
            SplitMode::Regions | SplitMode::Line => {
                Err("Click the sketch's regions or curves, not the model".into())
            }
        }
    }

    /// Takes `pick` as the tool of the split being set up, handing the
    /// clicks on to nothing (the preview shows), or says why it can't be.
    pub(super) fn split_tool(&mut self, pick: Pick) -> Result<(), Cow<'static, str>> {
        if !self.feed.answers_request() || self.feed.predates_replacement() {
            return Err(OUT_OF_DATE.into());
        }
        let tool = self.split_tool_of(pick)?;
        let Some(session) = &mut self.motion else {
            return Ok(());
        };
        match tool {
            SplitTool::Body(body) => session.split.body = Some(body),
            tool => session.split.surface = Some(tool),
        }
        session.split.gone = false;
        session.picking = MotionPick::Nothing;
        Ok(())
    }

    /// Picks `body` as the one the split being set up splits, if the
    /// feature can name it, taking it out as the tool body if it's that;
    /// clicks go on to its tool if it has none.
    pub(super) fn split_body(&mut self, body: BodyId) {
        let Some(session) = &mut self.motion else {
            return;
        };
        session.bodies = vec![body];
        if session.split.body == Some(body) {
            session.split.body = None;
        }
        session.picking = if session.split.tool().is_none() {
            MotionPick::Tool
        } else {
            MotionPick::Nothing
        };
    }

    /// The pieces of the split being set up as the model shown has them,
    /// if it shows its preview splitting: the body's, keeping its id, and
    /// the other's, a body the document doesn't hold yet for a new split
    /// (or one kept both sides again), named "New body" then.
    fn split_pieces(&self, session: &MotionSession) -> Vec<(BodyId, String, bool)> {
        let Some(split) = session.split() else {
            return Vec::new();
        };
        let shown = self.feed.shows_draft()
            && self.feed.answers_request()
            && self.feed.draft_error().is_none()
            && !self.feed.predates_replacement();
        if !shown {
            return Vec::new();
        }
        let document = self.editor.document();
        let parts = self.feed.parts();
        let name = |body: BodyId| (document.body(body)).map(|body| body.name.clone());
        let mut pieces = Vec::new();
        if parts.contains(&split.body) {
            let kept = name(split.body).unwrap_or_else(|| "Body".to_owned());
            pieces.push((split.body, kept, true));
        }
        if split.keeps_both() {
            let stored = (session.feature)
                .and_then(|id| document.feature(id))
                .and_then(|feature| match &feature.kind {
                    FeatureKind::Split(split) => split.new_body,
                    _ => None,
                })
                .filter(|body| parts.contains(body));
            let other = stored.or_else(|| {
                (parts.iter())
                    .copied()
                    .find(|&body| document.body(body).is_none())
            });
            if let Some(other) = other {
                let named = name(other).unwrap_or_else(|| "New body".to_owned());
                pieces.push((other, named, false));
            }
        }
        pieces
    }

    /// The faces of the piece of the split being set up that doesn't keep
    /// the body's id, as its preview shows them: tinted in the second
    /// colour.
    pub(super) fn split_lit(&self) -> Vec<Picked> {
        let Some(session) = self.motion.as_ref().filter(|s| s.kind == MotionKind::Split) else {
            return Vec::new();
        };
        let index = self.feed.pick_index();
        (self.split_pieces(session).into_iter())
            .filter(|(_, _, keeps)| !keeps)
            .flat_map(|(body, _, _)| index.body_faces(body).map(Picked::Face).collect::<Vec<_>>())
            .collect()
    }

    /// Why the split being edited can't keep one side as set up, if its
    /// new body, which that would drop, is named by a later feature: the
    /// document refuses that rather than drop the feature.
    pub(super) fn split_held(&self, session: &MotionSession) -> Option<String> {
        let edited = session.feature?;
        if session.kind != MotionKind::Split || session.split.keep == Keep::Both {
            return None;
        }
        let document = self.editor.document();
        let FeatureKind::Split(stored) = &document.feature(edited)?.kind else {
            return None;
        };
        let body = document.body(stored.new_body?)?;
        let user = (document.features().iter())
            .find(|feature| feature.kind.bodies().contains(&body.id))?;
        let (user, body) = (&user.name, &body.name);
        Some(format!(
            "{user} uses {body}, the piece this split would no longer keep: keep both, or take {body} out of {user} or delete it first"
        ))
    }

    /// The panel's warning of the split being set up, if later features
    /// name its body: they get the piece keeping its id. "2 later features
    /// use Body 1: they'll get the back piece".
    fn split_later(&self, session: &MotionSession) -> Option<String> {
        let edited = session.feature?;
        let body = *session.bodies.first()?;
        let document = self.editor.document();
        let features = document.features();
        let index = features.iter().position(|feature| feature.id == edited)?;
        let later = (features[index + 1..].iter())
            .filter(|feature| feature.kind.bodies().contains(&body))
            .count();
        let name = &document.body(body)?.name;
        let kept = session.split.keep.kept(session.split.original);
        let piece = kept.name().to_lowercase();
        match later {
            0 => None,
            1 => Some(format!(
                "1 later feature uses {name}: it'll get the {piece} piece"
            )),
            n => Some(format!(
                "{n} later features use {name}: they'll get the {piece} piece"
            )),
        }
    }

    /// The tool of the split being set up as its panel names it, and
    /// what's beside the name.
    fn split_tool_name(&self, session: &MotionSession) -> Option<(String, Option<String>)> {
        let document = self.editor.document();
        let sketch_name = |sketch: FeatureId| {
            (document.feature(sketch)).map_or_else(|| "A sketch".to_owned(), |f| f.name.clone())
        };
        let count = |n: usize, one: &str, many: &str| {
            if n == 1 {
                format!("1 {one}")
            } else {
                format!("{n} {many}")
            }
        };
        Some(match session.split.tool()? {
            SplitTool::Plane(plane) => (plane_name(document, &plane), None),
            SplitTool::Face(face) => (plane_name(document, &PlaneRef::Face(face)), None),
            SplitTool::Body(body) => (
                (document.body(body)).map_or_else(|| "Missing body".to_owned(), |b| b.name.clone()),
                None,
            ),
            SplitTool::Regions { sketch, regions } => (
                sketch_name(sketch),
                Some(count(regions.len(), "region", "regions")),
            ),
            SplitTool::Chain { sketch, curves } => (
                sketch_name(sketch),
                Some(count(curves.len(), "curve", "curves")),
            ),
        })
    }

    /// What's drawn of the split being set up, and named in its panel.
    pub(super) fn split_view<'s>(&'s self, session: &'s MotionSession) -> SplitView<'s> {
        let document = self.editor.document();
        let setup = &session.split;
        let index = self.feed.pick_index();
        let pieces = (self.split_pieces(session).into_iter())
            .filter_map(|(body, name, keeps)| {
                let [low, high] = index.bodies_bounds(&[body])?;
                Some(SplitPiece {
                    at: (low + high) / 2.0,
                    name,
                    keeps,
                })
            })
            .collect();
        let regions = setup.mode == SplitMode::Regions;
        let candidates = if regions {
            (setup
                .regions
                .candidates(|id| self.placement(id))
                .into_iter())
            .filter(|candidate| session.takes_sketch(document, candidate.feature))
            .collect()
        } else {
            Vec::new()
        };
        let lines = if setup.mode == SplitMode::Line {
            self.split_lines(session)
        } else {
            Vec::new()
        };
        SplitView {
            mode: setup.mode,
            tool: self.split_tool_name(session),
            body: (session.bodies.first())
                .and_then(|&body| document.body(body))
                .map(|body| body.name.as_str()),
            original: setup.original,
            keep: setup.keep,
            later: self.split_later(session),
            info: session.split().map(|split| split_info(document, &split)),
            candidates,
            source: setup.regions.source.filter(|_| regions),
            picked: if regions {
                &setup.regions.picked
            } else {
                SplitView::none_picked()
            },
            lines,
            chain: (setup.chain.as_ref()).map(|(sketch, curves)| (*sketch, curves.as_slice())),
            pieces,
        }
    }

    /// The sketches a split's line may be picked from, where they're
    /// placed: the line's own once a curve is picked, else every visible
    /// sketch before the feature.
    fn split_lines<'s>(&'s self, session: &MotionSession) -> Vec<SketchLines<'s>> {
        // A line gone is picked again from any.
        let chosen = (session.split.chain.as_ref())
            .filter(|_| !session.split.gone)
            .map(|(sketch, _)| *sketch);
        self.sketch_lines(session, |feature| match chosen {
            Some(sketch) => feature.id == sketch,
            None => feature.visible,
        })
    }

    /// The sketches of the features `wanted` takes whose curves or points
    /// the feature being set up in `session` (a split, a sweep, a loft)
    /// picks, where they're placed: each a sketch it can take at its
    /// place ([`MotionSession::takes_sketch`]) that's placed.
    pub(super) fn sketch_lines<'s>(
        &'s self,
        session: &MotionSession,
        wanted: impl Fn(&Feature) -> bool,
    ) -> Vec<SketchLines<'s>> {
        let document = self.editor.document();
        (document.features().iter())
            .filter(|feature| wanted(feature))
            .filter(|feature| session.takes_sketch(document, feature.id))
            .filter_map(|feature| {
                let FeatureKind::Sketch { sketch, .. } = &feature.kind else {
                    return None;
                };
                Some(SketchLines {
                    feature: feature.id,
                    placement: self.placement(feature.id)?,
                    sketch,
                })
            })
            .collect()
    }

    /// The split being set up's origin plane, if its tool is one: its name
    /// and where it is, for the viewport to draw as a mirror's.
    pub(super) fn split_reference(
        &self,
        session: &MotionSession,
    ) -> (Option<String>, Option<[DVec3; 2]>) {
        match session.split.tool() {
            Some(SplitTool::Plane(plane @ PlaneRef::Origin(origin))) => (
                Some(plane_name(self.editor.document(), &plane)),
                Some([DVec3::ZERO, origin.placement().normal]),
            ),
            _ => (None, None),
        }
    }
}
