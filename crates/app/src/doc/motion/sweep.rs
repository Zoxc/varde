//! A sweep in the move's session ([`MotionKind::Sweep`]): its profile's
//! regions picked as an extrude's ([`RegionPick`], on its sketches in the
//! viewport), then its path, by its Path and Helix tiles. A path's parts
//! are each a chain of a sketch's curves, one added per click on a curve
//! of a visible sketch before the sweep (the chain the curve is in,
//! [`varde_sketch::Sketch::chain_of`]; a click on a part's curve takes the
//! part out), and the model's edges, picked as a blend's (see `refs`:
//! named as of the sweep, all on one body, with the Tangent chain tick),
//! stored as one part. A helix's axis is picked as a move's (an origin
//! axis from the toolbar, a straight or round edge, a round face), with
//! its pitch, turns, Left-handed and Flip. Then Keep orientation and the
//! twist for a path, and the operation and bodies as an extrude's. What
//! the document no longer takes is kept and said to be gone.

use std::borrow::Cow;

use varde_document::{
    BodyId, CurveChain, Design, Document, FeatureId, Helix, MAX_PATH_CURVES, MAX_PATH_PARTS,
    MAX_SWEEP_REGIONS, Orientation, PathPart, PathRef, Sweep, SweepError,
};
use varde_expr::{AngleUnit, Unit, Value};
use varde_render::Camera;
use varde_sketch::{Id, RegionRef, Sketch};
use varde_view::{
    MotionField, MotionKind, MotionPick, OperationKind, SketchLines, SweepPart, SweepPath,
    SweepView, sweep_info,
};

use super::{Doc, MotionSession, length_field};
use crate::doc::camera::fitting_length;
use crate::doc::regions::{BodyTargets, RegionPick, TypedText};
use crate::doc::revolve::sketch_of;

/// A sweep's profile, path and options as picked; its model edges are
/// the session's blend's ([`MotionSession::blend`]), its helix's axis
/// and Flip the session's own.
#[derive(Debug, Clone)]
pub(crate) struct SweepSetup {
    pub(crate) path: SweepPath,
    /// The profile's regions, and their sketch.
    pub(crate) regions: RegionPick,
    /// The path's parts of sketch curves, in the order they were picked,
    /// each sorted.
    pub(crate) chains: Vec<CurveChain>,
    /// An edited sweep's edge parts past its first, which a session's
    /// edges (all on one body) can't hold: kept as stored, listed after
    /// the chains.
    pub(crate) stored_edges: Vec<PathPart>,
    pub(crate) keep_orientation: bool,
    pub(crate) left_handed: bool,
    pub(crate) operation: OperationKind,
    pub(crate) targets: BodyTargets,
    /// Whether the edited sweep stored a twist: a twist of nothing is
    /// then stored as typed, else left out.
    twist_stored: bool,
    /// The edited sweep's path's parts in the order it stored them: a
    /// path set up of the same parts keeps that order (the session lists
    /// sketch parts before edge parts), so OK on it unchanged writes
    /// nothing.
    stored_parts: Vec<PathPart>,
    /// Whether a part's sketch or curve is gone: the document no longer
    /// takes it at the feature's place.
    chains_gone: bool,
    /// The profile's regions whose sketch an edit took away, and their
    /// sketch: put by (said to be gone) while the visible sketches'
    /// regions are offered in their place, and picked again if their
    /// sketch comes back before others are picked, as a split's.
    stale_regions: Option<(FeatureId, Vec<RegionRef>)>,
}

impl Default for SweepSetup {
    fn default() -> Self {
        Self {
            path: SweepPath::Path,
            regions: RegionPick::new(None, MAX_SWEEP_REGIONS),
            chains: Vec::new(),
            stored_edges: Vec::new(),
            keep_orientation: false,
            left_handed: false,
            operation: OperationKind::NewBody,
            targets: BodyTargets::default(),
            twist_stored: false,
            stored_parts: Vec::new(),
            chains_gone: false,
            stale_regions: None,
        }
    }
}

/// The part of the view's height a new helix's pitch starts at, at most:
/// its 5 turns then rise at most half of it.
const PITCH_SHARE: f64 = 0.1;

/// The fields a new sweep opens with, seen by `camera`: a helix's pitch
/// of [`PITCH_SHARE`] of the view's height made nice ([`fitting_length`])
/// and 5 turns, and no twist.
pub(super) fn sweep_fields(design: &Design, camera: &Camera) -> [TypedText; 3] {
    let read = |text: &str, ask: &varde_expr::Ask| {
        Value::new(text, ask).map_or_else(
            |_| TypedText::read(text.to_owned(), ask),
            |value| TypedText::of(&value, ask),
        )
    };
    let twist = Sweep::twist_ask(design);
    [
        length_field(
            fitting_length(camera, design.units, PITCH_SHARE),
            &Sweep::pitch_ask(design),
            design,
        ),
        read("5", &Sweep::turns_ask(design)),
        TypedText::read(
            varde_expr::format(0.0, Some(Unit::Angle(AngleUnit::Deg))),
            &twist,
        ),
    ]
}

impl MotionSession {
    /// Opens the sweep `sweep` of `document` in this session: its
    /// regions, path and options.
    pub(super) fn open_sweep(&mut self, document: &Document, sweep: &Sweep) {
        let design = document.design();
        let setup = &mut self.sweep;
        setup.regions =
            RegionPick::editing(document, sweep.sketch, &sweep.regions, MAX_SWEEP_REGIONS);
        setup.keep_orientation = sweep.orientation == Orientation::Keep;
        setup.operation = OperationKind::of(&sweep.operation);
        setup.targets = BodyTargets::new(sweep.operation.excluded());
        if let Some(twist) = &sweep.twist {
            setup.twist_stored = true;
            self.fields[MotionField::Twist.index()] =
                TypedText::of(twist, &Sweep::twist_ask(&design));
        }
        match &sweep.path {
            PathRef::Chain(parts) => {
                setup.path = SweepPath::Path;
                setup.stored_parts = parts.clone();
                let mut edges = None;
                for part in parts {
                    match part {
                        PathPart::Curves(chain) => setup.chains.push(chain.clone()),
                        PathPart::Edges {
                            edges: picked,
                            tangent,
                        } if edges.is_none() => edges = Some((picked.clone(), *tangent)),
                        part => setup.stored_edges.push(part.clone()),
                    }
                }
                if let Some((picked, tangent)) = edges {
                    self.blend = super::blend::BlendSetup::of(&picked, tangent);
                }
            }
            PathRef::Helix(helix) => {
                setup.path = SweepPath::Helix;
                setup.left_handed = helix.left_handed;
                self.axis = Some(helix.axis);
                self.flip = helix.flip;
                self.fields[MotionField::Pitch.index()] =
                    TypedText::of(&helix.pitch, &Sweep::pitch_ask(&design));
                self.fields[MotionField::Turns.index()] =
                    TypedText::of(&helix.turns, &Sweep::turns_ask(&design));
            }
        }
        self.picking = MotionPick::Nothing;
    }

    /// The path as set up, if it has what it needs: a path's parts (its
    /// chains, its edges as one part, and an edited one's other edge
    /// parts), or a helix's axis and its values as they last read.
    fn sweep_path(&self) -> Option<PathRef> {
        let setup = &self.sweep;
        match setup.path {
            SweepPath::Path => {
                let mut parts: Vec<PathPart> = (setup.chains.iter().cloned())
                    .map(PathPart::Curves)
                    .collect();
                if !self.blend.edges.refs.is_empty() {
                    parts.push(PathPart::Edges {
                        edges: self.blend.edges.refs.clone(),
                        tangent: self.blend.chains,
                    });
                }
                parts.extend(setup.stored_edges.iter().cloned());
                if same_parts(&setup.stored_parts, &parts) {
                    parts.clone_from(&setup.stored_parts);
                }
                (!parts.is_empty()).then_some(PathRef::Chain(parts))
            }
            SweepPath::Helix => Some(PathRef::Helix(Helix {
                axis: self.axis?,
                pitch: self.field(MotionField::Pitch).value.clone()?,
                turns: self.field(MotionField::Turns).value.clone()?,
                left_handed: setup.left_handed,
                flip: self.flip,
            })),
        }
    }

    /// The sweep as set up, if it's whole: a profile's regions and a
    /// path, its twist as it last read (none for a helix, and none for a
    /// twist of nothing unless the edited sweep stored one).
    pub(super) fn sweep(&self) -> Option<Sweep> {
        let setup = &self.sweep;
        let sketch = setup.regions.source?;
        let regions = setup.regions.references();
        if regions.is_empty() {
            return None;
        }
        let path = self.sweep_path()?;
        let helix = matches!(path, PathRef::Helix(_));
        let twist = if helix {
            None
        } else {
            let twist = self.field(MotionField::Twist).value.clone()?;
            (twist.value != 0.0 || setup.twist_stored).then_some(twist)
        };
        Some(Sweep {
            sketch,
            regions: regions.to_vec(),
            path,
            orientation: if setup.keep_orientation && !helix {
                Orientation::Keep
            } else {
                Orientation::FollowPath
            },
            twist,
            operation: setup.targets.operation(setup.operation),
        })
    }

    /// What's still to be done before it can be committed, the words for
    /// the status bar: its profile, then its path.
    pub(super) fn sweep_need(&self) -> Option<&'static str> {
        if self.sweep.regions.picked.is_empty() {
            return Some("pick the regions to sweep");
        }
        if self.sweep_path().is_some() {
            return None;
        }
        Some(match self.sweep.path {
            SweepPath::Path => "pick the path: sketch curves or model edges",
            SweepPath::Helix if self.axis.is_none() => "pick the helix's axis",
            SweepPath::Helix => "enter the helix's pitch and turns",
        })
    }

    /// The words for what it names being gone, if anything is: a part's
    /// sketch or curve, its edges, or its helix's axis.
    pub(super) fn sweep_gone(&self) -> Option<&'static str> {
        if self.sweep.stale_regions.is_some() {
            return Some("The profile's sketch is gone: pick other regions");
        }
        match self.sweep.path {
            SweepPath::Path if self.sweep.chains_gone => {
                Some("A path's sketch or curve is gone: take its part out")
            }
            SweepPath::Path => self.blend_gone(),
            SweepPath::Helix => (self.gone_reference.is_some() && self.axis.is_some())
                .then_some("The axis is gone: pick another"),
        }
    }

    /// Whether the fields its path takes read: a path's twist, or a
    /// helix's pitch and turns.
    pub(super) fn sweep_typed(&self) -> bool {
        let fine = |field: MotionField| self.field(field).error.is_none();
        match self.sweep.path {
            SweepPath::Path => fine(MotionField::Twist),
            SweepPath::Helix => fine(MotionField::Pitch) && fine(MotionField::Turns),
        }
    }

    /// Finds its profile's regions again where their sketch changed (put
    /// by where it's gone, [`SweepSetup::stale_regions`], and picked again
    /// where it's back), and notes whether `document` no longer takes its
    /// edges, its parts' sketches and curves at feature `index` (an undo
    /// took them away).
    pub(super) fn prune_sweep(&mut self, document: &Document, index: usize) {
        if self.kind != MotionKind::Sweep {
            return;
        }
        let setup = &mut self.sweep;
        let is_sketch = |id: FeatureId| sketch_of(document, id).is_some();
        if let Some(source) = setup.regions.source
            && !is_sketch(source)
        {
            setup.stale_regions = Some((source, setup.regions.references().to_vec()));
            setup.regions = RegionPick::new(None, MAX_SWEEP_REGIONS);
        } else if setup.regions.source.is_none()
            && let Some((source, regions)) =
                (setup.stale_regions).take_if(|(source, _)| is_sketch(*source))
        {
            setup.regions = RegionPick::editing(document, source, &regions, MAX_SWEEP_REGIONS);
        }
        setup.regions.refresh(document);
        setup.targets.prune(document);
        setup.chains_gone = chains_gone(document, index, &setup.chains);
        self.prune_blend(document, index);
    }

    /// Picks the region `region` of `sketch` for its profile, or takes it
    /// out: once one is picked with no path yet, clicks go on to the path.
    pub(super) fn sweep_region(&mut self, sketch: FeatureId, region: usize, document: &Document) {
        if !self.takes_sketch(document, sketch) {
            return;
        }
        self.sweep.regions.toggle(sketch, region, false, document);
        if self.sweep.regions.source.is_some() {
            self.sweep.stale_regions = None;
        }
        if self.picking == MotionPick::Regions
            && !self.sweep.regions.picked.is_empty()
            && self.sweep_path().is_none()
        {
            self.picking = match self.sweep.path {
                SweepPath::Path => MotionPick::Path,
                SweepPath::Helix => MotionPick::Reference,
            };
        }
    }

    /// Adds the chain of `sketch`'s curves that `curve` is in as a part
    /// of its path, or takes out the part it's in: of a sketch before the
    /// sweep other than its profile's. Refused, why, if not.
    pub(super) fn sweep_curve(
        &mut self,
        sketch: FeatureId,
        curve: Id,
        document: &Document,
    ) -> Result<(), Cow<'static, str>> {
        if self.sweep.regions.source == Some(sketch) {
            return Err("The profile's own sketch can't be its path: pick another sketch's".into());
        }
        let drawn = sketch_of(document, sketch)
            .filter(|drawn| drawn.curve(curve).is_some())
            .ok_or("That curve isn't in the sketch")?;
        if !self.takes_sketch(document, sketch) {
            return Err("Only a sketch made before the sweep can be its path".into());
        }
        if let Some(at) = chain_at(&self.sweep.chains, sketch, curve) {
            self.sweep.chains.remove(at);
            return Ok(());
        }
        let chain = chain_through(drawn, sketch, curve);
        self.sweep_room(1, chain.curves.len())?;
        self.sweep.chains.push(chain);
        Ok(())
    }

    /// How many parts its path has as set up, and how many curves and
    /// edges they name in all.
    pub(super) fn path_counts(&self) -> (usize, usize) {
        let setup = &self.sweep;
        let edges = self.blend.edges.refs.len();
        let parts = (setup.chains.len())
            .saturating_add(setup.stored_edges.len())
            .saturating_add(usize::from(edges > 0));
        let curves = (setup.chains.iter().map(|chain| chain.curves.len()))
            .chain(setup.stored_edges.iter().map(PathPart::len))
            .fold(edges, usize::saturating_add);
        (parts, curves)
    }

    /// Whether its path has room for `parts` more parts naming `curves`
    /// more curves and edges, as the document takes them
    /// ([`MAX_PATH_PARTS`], [`MAX_PATH_CURVES`]); why not, if it hasn't.
    pub(super) fn sweep_room(&self, parts: usize, curves: usize) -> Result<(), Cow<'static, str>> {
        let (had, named) = self.path_counts();
        if had.saturating_add(parts) > MAX_PATH_PARTS {
            return Err(format!("A sweep's path takes at most {MAX_PATH_PARTS} parts").into());
        }
        if named.saturating_add(curves) > MAX_PATH_CURVES {
            return Err(format!(
                "A sweep's path takes at most {MAX_PATH_CURVES} curves and edges in all"
            )
            .into());
        }
        Ok(())
    }

    /// Takes out the path part of sketch curves (or an edited sweep's
    /// other edge part) listed at `at`.
    pub(super) fn drop_part(&mut self, at: usize) {
        let setup = &mut self.sweep;
        if at < setup.chains.len() {
            setup.chains.remove(at);
        } else if let Some(at) = at.checked_sub(setup.chains.len())
            && at < setup.stored_edges.len()
        {
            setup.stored_edges.remove(at);
        }
    }

    /// Sets what its path is: picking that path next unless it has one.
    pub(super) fn sweep_mode(&mut self, path: SweepPath) {
        self.sweep.path = path;
        if self.sweep.regions.picked.is_empty() {
            self.picking = MotionPick::Regions;
        } else if self.sweep_path().is_none() || self.picking != MotionPick::Nothing {
            self.picking = match path {
                SweepPath::Path => MotionPick::Path,
                SweepPath::Helix => MotionPick::Reference,
            };
        }
    }
}

/// Where in `chains` the one of `sketch`'s holding `curve` is, if one
/// does: a click on it takes it out.
pub(super) fn chain_at(chains: &[CurveChain], sketch: FeatureId, curve: Id) -> Option<usize> {
    (chains.iter())
        .position(|chain| chain.sketch == sketch && chain.curves.binary_search(&curve).is_ok())
}

/// The chain of curves `curve` is in, of `drawn`, the sketch `sketch`'s
/// ([`varde_sketch::Sketch::chain_of`]), sorted: a sweep's path's part or
/// a loft's rail.
pub(super) fn chain_through(drawn: &Sketch, sketch: FeatureId, curve: Id) -> CurveChain {
    let mut curves = drawn.chain_of(curve);
    curves.sort_unstable();
    curves.dedup();
    CurveChain { sketch, curves }
}

/// Whether a chain of `chains` names what `document` no longer takes at
/// feature `index`: a sketch not before it, or a curve its sketch hasn't.
pub(super) fn chains_gone(document: &Document, index: usize, chains: &[CurveChain]) -> bool {
    let features = document.features();
    let before = |sketch: FeatureId| {
        (features[..index.min(features.len())].iter()).any(|feature| feature.id == sketch)
    };
    (chains.iter()).any(|chain| {
        !before(chain.sketch)
            || sketch_of(document, chain.sketch).is_none_or(|sketch| {
                (chain.curves.iter()).any(|&curve| sketch.curve(curve).is_none())
            })
    })
}

/// "1 curve", "3 curves": `n` of `one`, or of `many` if not one.
pub(super) fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// Whether `a` and `b` hold the same parts, each as many times, in
/// whatever order.
fn same_parts(a: &[PathPart], b: &[PathPart]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut used = vec![false; b.len()];
    a.iter().all(|part| {
        let found = (0..b.len()).find(|&k| !used[k] && b[k] == *part);
        found.inspect(|&k| used[k] = true).is_some()
    })
}

impl Doc {
    /// Why the sweep as set up can't be committed, if what it names is
    /// refused at its place: a path sketch that's the profile's or no
    /// sketch before it, a curve not of its sketch, or what its edges or
    /// axis name; its own parts are [`Sweep::check_own`]'s.
    pub(super) fn sweep_refused(&self, session: &MotionSession) -> Option<SweepError> {
        let sweep = session.sweep()?;
        let document = self.editor.document();
        let index = session.index_in(document);
        (document.check_path(index, sweep.sketch, &sweep.path))
            .and_then(|()| sweep.check_curves(|id| sketch_of(document, id)))
            .err()
    }

    /// The sketches a sweep's path's curves are picked from, where
    /// they're placed: every visible sketch before the feature but the
    /// profile's, and each part's own.
    fn sweep_lines<'s>(&'s self, session: &MotionSession) -> Vec<SketchLines<'s>> {
        let setup = &session.sweep;
        self.sketch_lines(session, |feature| {
            (feature.visible || (setup.chains.iter()).any(|chain| chain.sketch == feature.id))
                && Some(feature.id) != setup.regions.source
        })
    }

    /// What's drawn of the sweep being set up, and named in its panel.
    pub(super) fn sweep_view<'s>(&'s self, session: &'s MotionSession) -> SweepView<'s> {
        let document = self.editor.document();
        let setup = &session.sweep;
        let candidates = (setup
            .regions
            .candidates(|id| self.placement(id))
            .into_iter())
        .filter(|candidate| session.takes_sketch(document, candidate.feature))
        .collect();
        let name = |sketch: FeatureId| {
            (document.feature(sketch)).map_or_else(|| "A sketch".to_owned(), |f| f.name.clone())
        };
        let chains = (setup.chains.iter()).map(|chain| SweepPart {
            name: name(chain.sketch),
            meta: Some(count(chain.curves.len(), "curve", "curves")),
        });
        let stored = (setup.stored_edges.iter()).map(|part| {
            let body = match part {
                PathPart::Edges { edges, .. } => edges.first().map(|edge| edge.body),
                PathPart::Curves(_) => None,
            };
            SweepPart {
                name: body.and_then(|body| document.body(body)).map_or_else(
                    || "Edges".to_owned(),
                    |body| format!("Edges of {}", body.name),
                ),
                meta: Some(count(part.len(), "edge", "edges")),
            }
        });
        let picking = matches!(session.picking, MotionPick::Path);
        SweepView {
            path: setup.path,
            candidates,
            source: setup.regions.source,
            picked: &setup.regions.picked,
            missing: setup.regions.missing,
            parts: chains.chain(stored).collect(),
            edges: self.blend_edges(session),
            lines: if picking || !setup.chains.is_empty() {
                self.sweep_lines(session)
            } else {
                Vec::new()
            },
            chains: (setup.chains.iter())
                .map(|chain| (chain.sketch, chain.curves.as_slice()))
                .collect(),
            keep_orientation: setup.keep_orientation,
            left_handed: setup.left_handed,
            operation: setup.operation,
            targets: self.body_targets(setup.operation, session.feature, &setup.targets),
            info: session.sweep().map(|sweep| sweep_info(document, &sweep)),
        }
    }

    /// Takes the body `body` out of what the sweep being set up joins,
    /// cuts or intersects, or puts it back.
    pub(super) fn sweep_target(&mut self, body: BodyId) {
        let revision = self.feed.revision();
        let document = self.editor.document();
        let Some(session) = &mut self.motion else {
            return;
        };
        let feature = session.feature;
        (session.sweep.targets).toggle(body, feature, document, revision);
    }
}
