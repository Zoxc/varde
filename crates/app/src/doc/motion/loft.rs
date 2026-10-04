//! A loft in the move's session ([`MotionKind::Loft`]): its sections
//! picked in order in the viewport, each a region of a visible sketch
//! before the loft (or of a section's own sketch, shown or not) or, first
//! or last, a sketch point on its own; a region's start, a sketch point
//! at one of its corners, set as it's picked (the corner nearest the
//! previous section's start, or the first) and moved by a click on
//! another corner. Rows move a section up or down or take it out. Then
//! Smooth or Ruled, Closed (no rails while it's on: they're kept for
//! when it's off), the rails, each the chain of a sketch's curves a click
//! on one of them adds (picked as a sweep's path's parts), and the
//! operation and bodies as an extrude's. What the document no longer
//! takes is kept and said to be gone.

use std::borrow::Cow;

use glam::DVec2;
use varde_document::{
    BodyId, CurveChain, Document, FeatureId, FeatureKind, Loft, LoftError, LoftMode,
    MAX_LOFT_RAILS, MAX_LOFT_SECTIONS, MAX_RAIL_CURVES, Section,
};
use varde_sketch::{Id, Profiles, Sketch};
use varde_view::{
    LoftSection, LoftShape, LoftView, MotionKind, MotionLook, MotionPick, OperationKind,
    SketchLines, SweepPart, loft_info,
};

use super::{Doc, MotionSession};
use crate::doc::regions::{BodyTargets, RegionPick};
use crate::doc::revolve::sketch_of;

/// How near a sketch point is to a region's vertex to be its corner,
/// relative to how far out it is: the vertices are the curves' ends'
/// places, so the points there are on them but for rounding.
const CORNER_SLACK: f64 = 1e-9;

/// A loft's sections, rails and options as picked.
#[derive(Debug, Clone)]
pub(crate) struct LoftSetup {
    /// Its sections in order, as the loft stores them.
    pub(crate) sections: Vec<Section>,
    /// Its rails, in the order they were picked, each sorted.
    pub(crate) rails: Vec<CurveChain>,
    pub(crate) mode: LoftMode,
    pub(crate) closed: bool,
    pub(crate) operation: OperationKind,
    pub(crate) targets: BodyTargets,
    /// The profiles of the sketches whose regions can be picked: the
    /// visible ones, and its sections' own (shown or not). No source:
    /// its sections are of any of them.
    pub(crate) regions: RegionPick,
    /// Whether a section's sketch, region, start or point is gone: the
    /// document no longer has it at the feature's place.
    sections_gone: bool,
    /// Whether a rail's sketch or curve is gone likewise.
    rails_gone: bool,
}

impl Default for LoftSetup {
    fn default() -> Self {
        Self {
            sections: Vec::new(),
            rails: Vec::new(),
            mode: LoftMode::Smooth,
            closed: false,
            operation: OperationKind::NewBody,
            targets: BodyTargets::default(),
            regions: RegionPick::new(None, MAX_LOFT_SECTIONS),
            sections_gone: false,
            rails_gone: false,
        }
    }
}

/// The corners of region `region` of `profiles` (of `sketch`) that are
/// sketch points: the outer loop's pieces' starts with a point there, in
/// the loop's order, each with where it is.
fn corners(sketch: &Sketch, profiles: &Profiles, region: usize) -> Vec<(Id, DVec2)> {
    let Some(region) = profiles.regions.get(region) else {
        return Vec::new();
    };
    let mut corners: Vec<(Id, DVec2)> = (region.outer.iter())
        .filter_map(|piece| {
            let vertex = *profiles.vertices.get(piece.start)?;
            let slack = CORNER_SLACK * (1.0 + vertex.abs().max_element());
            (sketch.points.iter())
                .find(|point| point.at.distance(vertex) <= slack)
                .map(|point| (point.id, point.at))
        })
        .collect();
    corners.dedup_by_key(|(id, _)| *id);
    corners
}

impl MotionSession {
    /// Opens the loft `loft` of `document` in this session: its sections,
    /// rails and options.
    pub(super) fn open_loft(&mut self, document: &Document, loft: &Loft) {
        let setup = &mut self.loft;
        setup.sections.clone_from(&loft.sections);
        setup.rails.clone_from(&loft.rails);
        setup.mode = loft.mode;
        setup.closed = loft.closed;
        setup.operation = OperationKind::of(&loft.operation);
        setup.targets = BodyTargets::new(loft.operation.excluded());
        setup.regions.also = loft.section_sketches();
        setup.regions.refresh(document);
        self.picking = MotionPick::Nothing;
    }

    /// The loft as set up, if it's whole: two sections or more; its rails
    /// left out while it's closed.
    pub(super) fn loft(&self) -> Option<Loft> {
        let setup = &self.loft;
        (setup.sections.len() >= 2).then(|| Loft {
            sections: setup.sections.clone(),
            mode: setup.mode,
            closed: setup.closed,
            rails: if setup.closed {
                Vec::new()
            } else {
                setup.rails.clone()
            },
            operation: setup.targets.operation(setup.operation),
        })
    }

    /// What's still to be done before it can be committed, the words for
    /// the status bar.
    pub(super) fn loft_need(&self) -> Option<&'static str> {
        match self.loft.sections.len() {
            0 => Some("pick the sections to loft: regions or sketch points"),
            1 => Some("pick the next section"),
            _ => None,
        }
    }

    /// The words for what it names being gone, if anything is: a
    /// section's sketch, region, start or point, or a rail's sketch or
    /// curve (not while it's closed, which leaves them out).
    pub(super) fn loft_gone(&self) -> Option<&'static str> {
        if self.loft.sections_gone {
            return Some("A section's sketch, region or point is gone: take it out");
        }
        (self.loft.rails_gone && !self.loft.closed)
            .then_some("A rail's sketch or curve is gone: take it out")
    }

    /// Whether `sketch` is a sketch of `document` the loft at its place
    /// can take: before the feature edited.
    fn loft_takes(&self, document: &Document, sketch: FeatureId) -> bool {
        let index = self.index_in(document);
        (document.features()[..index].iter()).any(|feature| {
            feature.id == sketch && matches!(feature.kind, FeatureKind::Sketch { .. })
        })
    }

    /// Finds the sketches' profiles again where they changed, and notes
    /// whether `document` no longer takes a section's or a rail's sketch,
    /// region, start, point or curves at feature `index` (an undo or an
    /// edit took them away).
    pub(super) fn prune_loft(&mut self, document: &Document, index: usize) {
        if self.kind != MotionKind::Loft {
            return;
        }
        let setup = &mut self.loft;
        setup.regions.also = (setup.sections.iter().map(Section::sketch)).collect();
        setup.regions.also.sort_unstable();
        setup.regions.also.dedup();
        setup.regions.refresh(document);
        setup.targets.prune(document);
        let features = document.features();
        let before = |sketch: FeatureId| {
            (features[..index.min(features.len())].iter()).any(|feature| feature.id == sketch)
        };
        let regions = &setup.regions;
        setup.sections_gone = (setup.sections.iter()).any(|section| {
            let sketch = section.sketch();
            let Some(drawn) = sketch_of(document, sketch).filter(|_| before(sketch)) else {
                return true;
            };
            match section {
                Section::Point { point, .. } => drawn.point(*point).is_none(),
                Section::Region { region, start, .. } => {
                    let found = (regions.found(sketch))
                        .and_then(|found| found.profiles.resolve(std::slice::from_ref(region))[0]);
                    found.is_none() || start.is_some_and(|start| drawn.point(start).is_none())
                }
            }
        });
        setup.rails_gone = (setup.rails.iter()).any(|rail| {
            !before(rail.sketch)
                || sketch_of(document, rail.sketch).is_none_or(|sketch| {
                    (rail.curves.iter()).any(|&curve| sketch.curve(curve).is_none())
                })
        });
    }

    /// The region of the section `section` of `document`, as its sketch's
    /// profiles find it: the profiles and the region's index.
    fn section_region(&self, section: &Section) -> Option<(&Profiles, usize)> {
        let Section::Region { sketch, region, .. } = section else {
            return None;
        };
        let found = self.loft.regions.found(*sketch)?;
        let index = found.profiles.resolve(std::slice::from_ref(region))[0]?;
        Some((&found.profiles, index))
    }

    /// Adds the point `point` of `sketch` as its first or last section,
    /// or takes that section out. Refused, why, if it can't be.
    pub(super) fn loft_point(
        &mut self,
        sketch: FeatureId,
        point: Id,
        document: &Document,
    ) -> Result<(), Cow<'static, str>> {
        if !self.loft_takes(document, sketch) {
            return Err("Only a sketch made before the loft can hold its sections".into());
        }
        let has = sketch_of(document, sketch).is_some_and(|drawn| drawn.point(point).is_some());
        if !has {
            return Err("That point isn't in the sketch".into());
        }
        let sections = &mut self.loft.sections;
        let this = Section::Point { sketch, point };
        if let Some(at) = sections.iter().position(|section| *section == this) {
            sections.remove(at);
            return Ok(());
        }
        if sections.len() >= MAX_LOFT_SECTIONS {
            return Err(format!("A loft takes at most {MAX_LOFT_SECTIONS} sections").into());
        }
        match (sections.first(), sections.last()) {
            (_, None) => sections.push(this),
            (_, Some(last)) if !last.is_point() => sections.push(this),
            (Some(first), _) if !first.is_point() => sections.insert(0, this),
            _ => {
                return Err("A loft takes a point only as its first or last section".into());
            }
        }
        Ok(())
    }

    /// Moves the start of section `section` to its corner at `point`.
    pub(super) fn loft_start(&mut self, section: usize, point: Id) {
        let is_corner = (self.loft.sections.get(section))
            .and_then(|section| {
                let (profiles, region) = self.section_region(section)?;
                let found = self.loft.regions.found(section.sketch())?;
                Some(corners(&found.sketch, profiles, region))
            })
            .is_some_and(|corners| corners.iter().any(|&(id, _)| id == point));
        if !is_corner {
            return;
        }
        if let Some(Section::Region { start, .. }) = self.loft.sections.get_mut(section) {
            *start = Some(point);
        }
    }

    /// Adds the chain of `sketch`'s curves that `curve` is in as a rail,
    /// or takes out the rail it's in. Refused, why, if not.
    pub(super) fn loft_rail(
        &mut self,
        sketch: FeatureId,
        curve: Id,
        document: &Document,
    ) -> Result<(), Cow<'static, str>> {
        let drawn = sketch_of(document, sketch)
            .filter(|drawn| drawn.curve(curve).is_some())
            .ok_or("That curve isn't in the sketch")?;
        if !self.loft_takes(document, sketch) {
            return Err("Only a sketch made before the loft can hold its rails".into());
        }
        let rails = &mut self.loft.rails;
        if let Some(at) = (rails.iter())
            .position(|rail| rail.sketch == sketch && rail.curves.binary_search(&curve).is_ok())
        {
            rails.remove(at);
            return Ok(());
        }
        if rails.len() >= MAX_LOFT_RAILS {
            return Err(format!("A loft takes at most {MAX_LOFT_RAILS} rails").into());
        }
        let mut curves = drawn.chain_of(curve);
        curves.sort_unstable();
        curves.dedup();
        if curves.len() > MAX_RAIL_CURVES {
            return Err(format!("A rail takes at most {MAX_RAIL_CURVES} curves").into());
        }
        rails.push(CurveChain { sketch, curves });
        Ok(())
    }

    /// Moves section `at` up, before the one above it.
    pub(super) fn section_up(&mut self, at: usize) {
        if (1..self.loft.sections.len()).contains(&at) {
            self.loft.sections.swap(at - 1, at);
        }
    }

    /// Takes section `at` out.
    pub(super) fn drop_section(&mut self, at: usize) {
        if at < self.loft.sections.len() {
            self.loft.sections.remove(at);
        }
    }

    /// Takes rail `at` out.
    pub(super) fn drop_rail(&mut self, at: usize) {
        if at < self.loft.rails.len() {
            self.loft.rails.remove(at);
        }
    }

    /// Turns Closed on or off: on, the rails aren't picked.
    pub(super) fn loft_closed(&mut self) {
        self.loft.closed = !self.loft.closed;
        if self.loft.closed && self.picking == MotionPick::Path {
            self.picking = MotionPick::Nothing;
        }
    }

    /// Makes `picking` what clicks pick (its sections or its rails, not
    /// while it's closed), or stops picking if it already is.
    pub(super) fn loft_picking(&mut self, picking: MotionPick) {
        if !matches!(picking, MotionPick::Regions | MotionPick::Path)
            || (picking == MotionPick::Path && self.loft.closed)
        {
            return;
        }
        self.picking = if self.picking == picking {
            MotionPick::Nothing
        } else {
            picking
        };
    }
}

/// Whether `message` is one [`Doc::loft_look`] takes for a loft.
pub(super) fn loft_message(message: &MotionLook) -> bool {
    matches!(
        message,
        MotionLook::Picking(_)
            | MotionLook::LoftRegion { .. }
            | MotionLook::LoftPoint { .. }
            | MotionLook::LoftStart { .. }
            | MotionLook::SectionUp(_)
            | MotionLook::DropSection(_)
            | MotionLook::LoftRail { .. }
            | MotionLook::DropRail(_)
            | MotionLook::LoftMode(_)
            | MotionLook::Closed
            | MotionLook::Operation(_)
            | MotionLook::Target(_)
    )
}

impl Doc {
    /// Takes `message` for the loft being set up, in a document that can
    /// be changed ([`loft_message`]).
    pub(super) fn loft_look(&mut self, message: MotionLook) {
        let document = self.editor.document();
        let Some(session) = &mut self.motion else {
            return;
        };
        let refused = match message {
            MotionLook::Picking(picking) => {
                session.loft_picking(picking);
                Ok(())
            }
            MotionLook::LoftRegion { sketch, region } => {
                self.loft_region(sketch, region);
                Ok(())
            }
            MotionLook::LoftPoint { sketch, point } => session.loft_point(sketch, point, document),
            MotionLook::LoftStart { section, point } => {
                session.loft_start(section, point);
                Ok(())
            }
            MotionLook::SectionUp(at) => {
                session.section_up(at);
                Ok(())
            }
            MotionLook::DropSection(at) => {
                session.drop_section(at);
                Ok(())
            }
            MotionLook::LoftRail { sketch, curve } => session.loft_rail(sketch, curve, document),
            MotionLook::DropRail(at) => {
                session.drop_rail(at);
                Ok(())
            }
            MotionLook::LoftMode(mode) => {
                session.loft.mode = mode;
                Ok(())
            }
            MotionLook::Closed => {
                session.loft_closed();
                Ok(())
            }
            MotionLook::Operation(kind) => {
                session.loft.operation = kind;
                Ok(())
            }
            MotionLook::Target(body) => {
                self.loft_target(body);
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(why) = refused {
            self.notice = Some(why.into_owned());
        }
        self.prune_loft_now();
    }

    /// Notes what's gone of the loft being set up after a change to it.
    fn prune_loft_now(&mut self) {
        let document = self.editor.document();
        if let Some(session) = &mut self.motion {
            let index = session.index_in(document);
            session.prune_loft(document, index);
        }
    }

    /// Adds the region `region` of `sketch` as the loft's last section
    /// (before a last point), starting at its corner nearest the
    /// previous section's start (the first corner for the first), or
    /// takes that section out. Refused, said in the status bar, if it
    /// can't be: a sketch after the loft, a region with holes, past the
    /// limit.
    fn loft_region(&mut self, sketch: FeatureId, region: usize) {
        let document = self.editor.document();
        let Some(session) = &self.motion else {
            return;
        };
        if !session.loft_takes(document, sketch) {
            self.notice = Some("Only a sketch made before the loft can hold its sections".into());
            return;
        }
        let Some(found) = session.loft.regions.found(sketch) else {
            return;
        };
        if region >= found.profiles.regions.len() {
            return;
        }
        // The section of this region, if it's one already: out it goes.
        let same = (session.loft.sections.iter()).position(|section| {
            section.sketch() == sketch
                && session
                    .section_region(section)
                    .is_some_and(|(_, index)| index == region)
        });
        if let Some(at) = same {
            if let Some(session) = &mut self.motion {
                session.drop_section(at);
            }
            return;
        }
        let sections = &session.loft.sections;
        if sections.len() >= MAX_LOFT_SECTIONS {
            self.notice = Some(format!("A loft takes at most {MAX_LOFT_SECTIONS} sections"));
            return;
        }
        let Some(reference) = found.profiles.reference(region) else {
            self.notice = Some("That region is too thin to be a section".into());
            return;
        };
        if !reference.holes.is_empty() {
            self.notice = Some("A section is one loop: pick a region without holes".into());
            return;
        }
        // The start: the corner nearest the previous section's start in
        // the world, or the first corner.
        let corners = corners(&found.sketch, &found.profiles, region);
        let placement = self.placement(sketch);
        let insert_at = match sections.last() {
            Some(last) if last.is_point() && sections.len() >= 2 => sections.len() - 1,
            _ => sections.len(),
        };
        let previous = insert_at
            .checked_sub(1)
            .and_then(|at| self.section_dot(session, at));
        let start = match (previous, placement) {
            (Some(previous), Some(placement)) => (corners.iter())
                .map(|&(id, at)| (id, placement.to_world(at).distance(previous)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(id, _)| id),
            _ => corners.first().map(|&(id, _)| id),
        };
        let section = Section::Region {
            sketch,
            region: reference,
            start,
        };
        if let Some(session) = &mut self.motion {
            session.loft.sections.insert(insert_at, section);
        }
    }

    /// Where the start dot of the section `at` of the loft set up in
    /// `session` is in the world, if it has one.
    fn section_dot(&self, session: &MotionSession, at: usize) -> Option<glam::DVec3> {
        let section = session.loft.sections.get(at)?;
        let placement = self.placement(section.sketch())?;
        let document = self.editor.document();
        let drawn = sketch_of(document, section.sketch())?;
        let point = match section {
            Section::Region { start, .. } => (*start)?,
            Section::Point { point, .. } => *point,
        };
        Some(placement.to_world(drawn.point(point)?.at))
    }

    /// Takes the body `body` out of what the loft being set up joins,
    /// cuts or intersects, or puts it back.
    fn loft_target(&mut self, body: BodyId) {
        let revision = self.feed.revision();
        let document = self.editor.document();
        let Some(session) = &mut self.motion else {
            return;
        };
        let feature = session.feature;
        (session.loft.targets).toggle(body, feature, document, revision);
    }

    /// Why the loft as set up can't be committed, if what it names is
    /// refused at its place: a start point, a point section or a rail's
    /// curves its sketch doesn't have. Its own parts are
    /// [`Loft::check_own`]'s.
    pub(super) fn loft_refused(&self, session: &MotionSession) -> Option<LoftError> {
        let loft = session.loft()?;
        let document = self.editor.document();
        loft.check_names(|id| sketch_of(document, id)).err()
    }

    /// The sketches a loft's points and rails' curves are picked from,
    /// where they're placed: every visible sketch before the feature, and
    /// each section's and rail's own.
    fn loft_lines<'s>(&'s self, session: &MotionSession) -> Vec<SketchLines<'s>> {
        let document = self.editor.document();
        let setup = &session.loft;
        (document.features().iter())
            .filter(|feature| {
                feature.visible
                    || (setup.sections.iter()).any(|section| section.sketch() == feature.id)
                    || (setup.rails.iter()).any(|rail| rail.sketch == feature.id)
            })
            .filter(|feature| session.loft_takes(document, feature.id))
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

    /// What's drawn of the loft being set up, and named in its panel.
    pub(super) fn loft_view<'s>(&'s self, session: &'s MotionSession) -> LoftView<'s> {
        let document = self.editor.document();
        let setup = &session.loft;
        let candidates = (setup
            .regions
            .candidates(|id| self.placement(id))
            .into_iter())
        .filter(|candidate| session.loft_takes(document, candidate.feature))
        .collect();
        let name = |sketch: FeatureId| {
            (document.feature(sketch)).map_or_else(|| "A sketch".to_owned(), |f| f.name.clone())
        };
        let sections = (setup.sections.iter())
            .map(|section| {
                let sketch = section.sketch();
                let drawn = sketch_of(document, sketch);
                let shape = match section {
                    Section::Region { start, .. } => {
                        let found = setup.regions.found(sketch);
                        let resolved = session.section_region(section);
                        let (region, corners) = match (found, resolved) {
                            (Some(found), Some((profiles, index))) => (
                                profiles.regions.get(index),
                                corners(&found.sketch, profiles, index),
                            ),
                            _ => (None, Vec::new()),
                        };
                        LoftShape::Region {
                            region,
                            corners,
                            start: *start,
                        }
                    }
                    Section::Point { point, .. } => {
                        LoftShape::Point(drawn.and_then(|drawn| drawn.point(*point)).map(|p| p.at))
                    }
                };
                let gone = match &shape {
                    LoftShape::Region { region, .. } => region.is_none(),
                    LoftShape::Point(at) => at.is_none(),
                } || !session.loft_takes(document, sketch);
                LoftSection {
                    name: name(sketch),
                    gone,
                    sketch,
                    placement: self.placement(sketch),
                    shape,
                }
            })
            .collect();
        let count = |n: usize| {
            if n == 1 {
                "1 curve".to_owned()
            } else {
                format!("{n} curves")
            }
        };
        LoftView {
            candidates,
            lines: self.loft_lines(session),
            sections,
            rails: (setup.rails.iter())
                .map(|rail| SweepPart {
                    name: name(rail.sketch),
                    meta: Some(count(rail.curves.len())),
                })
                .collect(),
            chains: if setup.closed {
                Vec::new()
            } else {
                (setup.rails.iter())
                    .map(|rail| (rail.sketch, rail.curves.as_slice()))
                    .collect()
            },
            mode: setup.mode,
            closed: setup.closed,
            operation: setup.operation,
            targets: self.body_targets(setup.operation, session.feature, &setup.targets),
            info: session.loft().map(|loft| loft_info(&loft)),
        }
    }
}
