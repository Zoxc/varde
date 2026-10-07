//! The document model: what the user is editing, independent of any UI.
//!
//! A [`Document`] is plain data, read-only outside this crate. All
//! modifications go through an [`Editor`], which applies [`Command`]s,
//! checks their results and records history for undo/redo.

mod align;
mod blend;
mod chamfer;
pub mod codec;
mod combine;
mod edge;
mod editor;
mod example;
mod extrude;
mod face_draft;
mod feature;
mod fillet;
mod loft;
mod motion;
pub mod name;
mod offset_face;
mod opacity;
mod outside;
mod param;
mod pattern;
mod plane;
mod removal;
mod rename;
mod revolve;
mod scale;
mod shell;
mod split;
mod sweep;
#[cfg(test)]
mod testing;
mod tint;

pub use align::{Align, AlignError, AlignRefs, DirRef, PointRef};
pub use blend::{BlendEdgesError, MAX_BLEND_EDGES, check_blend_edges_own};
pub use chamfer::{Chamfer, ChamferError, ChamferSize};
pub use codec::DecodeError;
pub use combine::{BodyOp, Combine, CombineError, MAX_FEATURE_BODIES};
pub use edge::{EdgeError, EdgeRef};
pub use editor::{Command, Editor, Generation, Revision};
pub use extrude::{Extent, Extrude, ExtrudeError, MAX_EXTRUDE_REGIONS, Operation, Targets};
pub use face_draft::{FaceDraft, FaceDraftError, MAX_DRAFT_FACES};
pub use feature::{Feature, FeatureId, FeatureKind};
pub use fillet::{Fillet, FilletError};
pub use loft::{
    Loft, LoftError, LoftMode, MAX_LOFT_RAILS, MAX_LOFT_SECTIONS, MAX_RAIL_CURVES, Section,
};
pub use motion::{Axis3, AxisRef, Mirror, MotionError, Move, PlaneRef};
pub use offset_face::{MAX_OFFSET_FACES, OffsetFace, OffsetFaceError};
pub use opacity::Opacity;
pub use outside::{LinkError, LinkSource, OutsideRef, sketch_face};
pub use param::{Param, ParamError, ParamUses};
pub use pattern::{Copies, MAX_PATTERN_BODIES, MAX_PATTERN_COUNT, Pattern, PatternKind};
pub use plane::{FaceRef, FaceSetError, OriginPlane, Placement, Plane, PlaneError};
pub use removal::{Removable, Removal};
pub use rename::{Named, Rename};
pub use revolve::{AxisLine, MAX_REVOLVE_REGIONS, Revolve, RevolveError, Turn};
pub use scale::{MAX_SCALE_FACTOR, Scale, ScaleError, ScaleFactor};
pub use shell::{MAX_SHELL_FACES, Shell, ShellError};
pub use split::{Keep, MAX_SPLIT_CURVES, Side, Split, SplitError, SplitTool};
pub use sweep::{
    CurveChain, Helix, MAX_HELIX_TURNS, MAX_PATH_CURVES, MAX_PATH_PARTS, MAX_SWEEP_REGIONS,
    MAX_TWIST_TURNS, MIN_HELIX_TURNS, Orientation, PathPart, PathRef, Sweep, SweepError,
};
pub use tint::Tint;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
// The types and limits the model's API names, so that clients editing a
// document need only this crate.
pub use varde_expr::{LengthUnit, Params};
pub use varde_kernel::mesh::{FaceKey, PartKey};
pub use varde_kernel::{MAX_COORD, Tolerance};
pub use varde_sketch::{Design, Id, RegionRef, Sketch, SketchError};

/// Whether `point` is a reference's point a file may hold: finite and
/// within [`MAX_COORD`] of zero in each coordinate.
pub(crate) fn in_bounds(point: glam::DVec3) -> bool {
    point.is_finite() && point.abs().max_element() <= f64::from(MAX_COORD)
}

/// The application's name, as the user sees it.
pub const APP_NAME: &str = "Varde CAD";

/// The extension of the application's design files, `.vrdp`.
pub const EXTENSION: &str = "vrdp";

/// A document as of one editor revision, shared rather than copied: what
/// the regeneration and IO lanes are sent to work on. See [`Editor::snapshot`].
pub type Snapshot = Arc<Document>;

/// A body's handle in one document. It's opaque: ids come from the
/// document's bodies, not from literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BodyId(u64);

impl BodyId {
    /// Stands for the body an extrude or revolve not added yet will make
    /// ([`Operation::NewBody`]), which a command adding it gives a new id.
    /// No document's body has it: ids are below the next id, which is at
    /// most this.
    pub const NEW: BodyId = BodyId(u64::MAX);
}

/// The longest body or feature name, in bytes, a document may hold. The
/// editor only makes short ones, but a file could carry any length, and the
/// side panel lays each name out on the UI thread, where a long enough one
/// stalls it or overflows text shaping.
pub const MAX_NAME_LEN: usize = 1024;

/// A body: a solid the feature history makes. The document holds only
/// its name, whether it's shown and how opaque, and which feature makes
/// it (an extrude, revolve, sweep or loft making a new body, [`Operation::NewBody`],
/// a pattern whose copies are bodies of their own,
/// [`Copies::Separate`], or a split's other piece, [`Split::new_body`]); its geometry is whatever regenerating the history gives it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Body {
    pub id: BodyId,
    pub name: String,
    pub visible: bool,
    /// How opaque it's drawn, in range in a checked document, see
    /// [`Opacity`].
    pub opacity: Opacity,
    /// The colour it's drawn in, if not the theme's: in range in a
    /// checked document, see [`Tint`]. Missing in older files, which read
    /// as none.
    #[serde(default)]
    pub color: Option<Tint>,
    /// The feature that makes the body, listed in the document.
    pub created_by: FeatureId,
}

/// The fields are private so that a document passes [`Document::check`]
/// by construction: the ways to get one are [`Document::default`],
/// [`Document::example`], an [`Editor`] edit, which is checked, and
/// deserializing one, which goes through [`Unchecked::check`].
///
/// Files store it by field and variant names, to be extended as `varde-io`'s
/// `vrdp` module says: a new field goes here and on [`Unchecked`],
/// `#[serde(default)]` on both.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Unchecked")]
pub struct Document {
    bodies: Vec<Body>,
    features: Vec<Feature>,
    /// The design's units: what values are shown in, and what a bare
    /// number in a dimension's expression is read in. The model itself is
    /// in millimetres whatever they are.
    units: LengthUnit,
    /// The design's fit tolerance in millimetres, see
    /// [`Document::tolerance`].
    tolerance: f64,
    /// The id the next body or feature gets.
    next_id: u64,
    /// The feature the Timeline is rolled back to before, if it is: the
    /// model is that of the features before it ([`Document::before`]).
    /// Names a feature of the document. Missing in older files, which
    /// read as none.
    #[serde(default)]
    rollback: Option<FeatureId>,
    /// The design's parameters, see [`Param`]. Missing in older files,
    /// which read as none.
    #[serde(default)]
    params: Vec<Param>,
    /// `params` resolved in `units`, kept in step with both: not stored,
    /// but worked out again as a document is read. Shared, so that copies
    /// of the document and the panels reading values with it
    /// ([`Document::params_shared`]) don't copy it.
    #[serde(skip)]
    resolved: Arc<Params>,
}

/// A new design: no bodies or features, in millimetres, to the default
/// tolerance.
impl Default for Document {
    fn default() -> Self {
        Document {
            bodies: Vec::new(),
            features: Vec::new(),
            units: LengthUnit::default(),
            tolerance: Tolerance::DEFAULT.fit(),
            next_id: 0,
            rollback: None,
            params: Vec::new(),
            resolved: Arc::default(),
        }
    }
}

/// A [`Document`] as deserialized, before [`Document::check`]: the same
/// fields by the same names in the same order, so it reads the same bytes.
///
/// Deserializing a [`Document`] checks it too, but postcard drops the
/// message of an error raised while deserializing, and the [`CheckError`]
/// is lost as a string either way, so decoders that want to say what's
/// wrong decode this and [`check`](Unchecked::check) it, as
/// [`Document::from_postcard`] and `varde-io`'s files do.
#[derive(Debug, Deserialize)]
pub struct Unchecked {
    bodies: Vec<Body>,
    features: Vec<Feature>,
    units: LengthUnit,
    tolerance: f64,
    next_id: u64,
    #[serde(default)]
    rollback: Option<FeatureId>,
    #[serde(default)]
    params: Vec<Param>,
}

impl Unchecked {
    /// The document, if it passes [`Document::check`], its sketches'
    /// handles read without ends given them first
    /// ([`Sketch::add_handle_ends`](varde_sketch::Sketch::add_handle_ends)).
    pub fn check(self) -> Result<Document, CheckError> {
        let Unchecked {
            bodies,
            mut features,
            units,
            tolerance,
            next_id,
            rollback,
            params,
        } = self;
        for feature in &mut features {
            if let FeatureKind::Sketch { sketch, .. } = &mut feature.kind {
                // Out of ids, it's left for the check to refuse.
                let _ = sketch.add_handle_ends();
            }
        }
        // Checked before resolving, which takes whatever it's given but
        // bounds the work by the checked limits.
        param::check_params(&params)?;
        let resolved = param::resolve(&params, units);
        let document = Document {
            bodies,
            features,
            units,
            tolerance,
            next_id,
            rollback,
            params,
            resolved,
        };
        document.check()?;
        Ok(document.with_sketch_faces())
    }
}

impl TryFrom<Unchecked> for Document {
    type Error = CheckError;

    fn try_from(unchecked: Unchecked) -> Result<Document, CheckError> {
        unchecked.check()
    }
}

impl Document {
    /// A new id for a body or a feature. Fails, leaving the document as it
    /// was, once the ids have run out: `next_id` may come from a file, so
    /// it can be anything.
    fn new_id(&mut self) -> Result<u64, EditError> {
        let id = self.next_id;
        self.next_id = id.checked_add(1).ok_or(EditError::OutOfIds)?;
        Ok(id)
    }

    /// Adds a visible feature with a new id, see
    /// [`new_id`](Document::new_id). New ids are the highest, so it goes
    /// last, keeping the features in order. The rest of
    /// [`Document::check`] is up to the caller, as [`Editor::apply`] does.
    pub(crate) fn push_feature(
        &mut self,
        name: impl Into<String>,
        kind: FeatureKind,
    ) -> Result<FeatureId, EditError> {
        let id = FeatureId(self.new_id()?);
        // A new feature goes last, so the Timeline rolls forward to show it.
        self.rollback = None;
        self.features.push(Feature {
            id,
            name: name.into(),
            visible: true,
            kind,
        });
        Ok(id)
    }

    pub fn bodies(&self) -> &[Body] {
        &self.bodies
    }

    /// The features, in the order they were added.
    pub fn features(&self) -> &[Feature] {
        &self.features
    }

    /// The document as it was before feature `until` was added: the
    /// features before it and the bodies they make, as the Timeline's
    /// rollback shows it. Features are in id order, and each refers only
    /// to those before it, so what's kept stands on its own.
    #[must_use]
    pub fn before(&self, until: FeatureId) -> Document {
        let mut document = self.clone();
        document.features.retain(|feature| feature.id < until);
        document.bodies.retain(|body| body.created_by < until);
        document.rollback = None;
        document
    }

    /// The feature the Timeline is rolled back to before, if it is.
    pub fn rollback(&self) -> Option<FeatureId> {
        self.rollback
    }

    pub fn feature(&self, id: FeatureId) -> Option<&Feature> {
        self.feature_index(id).map(|index| &self.features[index])
    }

    /// Where feature `id` is in [`features`](Document::features), found
    /// by binary search, as the features are in increasing id order.
    pub fn feature_index(&self, id: FeatureId) -> Option<usize> {
        self.features
            .binary_search_by_key(&id, |feature| feature.id)
            .ok()
    }

    /// The design's units, see [`Command::SetUnits`].
    pub fn units(&self) -> LengthUnit {
        self.units
    }

    /// The design's tolerance: how far fitted curves and surfaces may be
    /// from the true ones (1 µm unless changed, see
    /// [`Command::SetTolerance`]), and a thousandth of that, the finest
    /// detail the kernel keeps apart.
    pub fn tolerance(&self) -> Tolerance {
        // Checked by `check`, which every document passes.
        Tolerance::new(self.tolerance).unwrap_or_default()
    }

    /// What the document's sketches and values are checked against:
    /// [`MAX_COORD`], its units and its parameters, resolved.
    pub fn design(&self) -> Design<'_> {
        Design {
            max: f64::from(MAX_COORD),
            units: self.units,
            params: &self.resolved,
        }
    }

    pub fn body(&self, id: BodyId) -> Option<&Body> {
        self.body_index(id).map(|index| &self.bodies[index])
    }

    /// Where body `id` is in [`bodies`](Document::bodies), found by binary
    /// search, as [`Document::check`] keeps them in increasing id order.
    pub(crate) fn body_index(&self, id: BodyId) -> Option<usize> {
        self.bodies.binary_search_by_key(&id, |body| body.id).ok()
    }

    /// Checks what a file or a [`Command`] could get wrong, and
    /// [`Editor::apply`] refuses a command that does: the parameters are
    /// at most [`MAX_PARAMS`](varde_expr::MAX_PARAMS), each named as
    /// [`check_name`](varde_expr::check_name) takes, no name twice, each
    /// text at most [`MAX_LEN`](varde_expr::MAX_LEN) bytes (an expression
    /// may be in error: only values using it may not, which their
    /// features' checks find, every value read with the parameters,
    /// [`Document::design`]); body ids are in
    /// increasing order and below `next_id`, so bodies added later get new
    /// ids and come last, where an edit adds them, and the same for
    /// feature ids; no name is longer than [`MAX_NAME_LEN`]; every body's
    /// opacity is one [`Opacity::new`] takes, and its colour, if it has one,
    /// one [`Tint::new`] takes; the tolerance is one
    /// [`Tolerance::new`] takes; every body is made by an extrude,
    /// revolve, sweep, loft or split the document holds that names it as its new
    /// body, and every such body is there, or by a pattern listing it as a copy
    /// body ([`Copies::Separate`]), every such body there too, one per
    /// copy; every id a feature holds for a body it made before
    /// ([`FeatureKind::held_body`]) is below `next_id`, no body's, and
    /// held by that feature alone; every sketch passes [`Sketch::check`]
    /// against [`MAX_COORD`] and the document's units
    /// ([`Document::design`]), so every dimension's expression gives its
    /// value in them, and every sketch on a face names what comes before
    /// it, as [`PlaneError`] lists; and every extrude and revolve uses a
    /// sketch feature before it, has regions, distances or angles and an
    /// operation as [`Extrude`] and [`Revolve`] describe, and excludes
    /// only bodies features before it make; a revolve about a model edge
    /// names a body and faces' makers before it, as a sketch on a face
    /// does; every combine's target and tools are bodies features
    /// before it make, its tools as [`Combine::check_own`] wants them;
    /// and every move's, mirror's and pattern's bodies are bodies
    /// features before it make, as [`Move::check_own`],
    /// [`Mirror::check_own`] and [`Pattern::check_own`] want them with
    /// its values, its axis edge or face, or plane face, named as a
    /// revolve's edge is; and every align's body is one a feature before
    /// it makes, its sides shaped and its values as [`Align::check_own`]
    /// wants them, its moved side's references on that body, its target
    /// side's on others, each named as a move's axis is; and every scale's
    /// bodies are bodies features before it make, its factors or edge
    /// length as [`Scale::check_own`] wants them, its edge on one of
    /// them, its point and edge named as a move's axis is; and every
    /// split's body, tool body and face tool's body are bodies features
    /// before it make, its plane face and faces' makers named as a
    /// mirror's plane is, its sketch a sketch before it, and its new body
    /// there when it keeps both pieces, as [`Split::check_own`]
    /// wants it; and every chamfer's edges are on one body a feature
    /// before it makes, its faces' makers before it (or not there with
    /// ids no later feature can take), its edges and values as
    /// [`Chamfer::check_own`] wants them; and every shell's body is one a
    /// feature before it makes, its open faces' makers before it (or not
    /// there with ids no later feature can take), its faces and
    /// thickness as [`Shell::check_own`] wants them; every fillet's
    /// edges as a chamfer's, its radius as [`Fillet::check_own`] wants
    /// it; every offset face's faces are on one body a feature before it
    /// makes, their makers before it (or not there with ids no later
    /// feature can take), its faces and distance as
    /// [`OffsetFace::check_own`] wants them; every draft's faces
    /// likewise, its neutral face on a body a feature before it makes and
    /// made by a feature before it, its faces and angle as
    /// [`FaceDraft::check_own`] wants them; every sweep is
    /// as an extrude is, its path's sketches sketches before it other than
    /// its profile's, its path's edges on bodies features before it make
    /// (with their faces' makers before it, or not there with ids no
    /// later feature can take), its helix's axis named likewise, its
    /// parts and values as [`Sweep::check_own`] wants them; and every
    /// loft's sections and rails are of sketch features before it, its
    /// sections, mode, rails and operation as [`Loft::check_own`] wants
    /// them and its operation as an extrude's. A revolve's axis line, a
    /// sweep's path's curves and a loft's points and rail curves aren't
    /// checked against their sketches here (see [`Revolve::check_axis`],
    /// [`Sweep::check_curves`] and [`Loft::check_names`]).
    pub fn check(&self) -> Result<(), CheckError> {
        param::check_params(&self.params)?;
        debug_assert!(
            self.resolved == param::resolve(&self.params, self.units),
            "the resolved parameters are kept in step"
        );
        // Orders first: features and bodies are found by binary search.
        if let Some(pair) = self
            .features
            .windows(2)
            .find(|pair| pair[0].id >= pair[1].id)
        {
            return Err(CheckError::FeatureOrder(pair[1].id, pair[0].id));
        }
        if let Some(pair) = self.bodies.windows(2).find(|pair| pair[0].id >= pair[1].id) {
            return Err(CheckError::Order(pair[1].id, pair[0].id));
        }
        if Tolerance::new(self.tolerance).is_none() {
            return Err(CheckError::Tolerance(self.tolerance));
        }
        // How many bodies each pattern makes, which its own check holds
        // to the copy bodies it lists.
        let mut copies: BTreeMap<FeatureId, usize> = BTreeMap::new();
        for body in &self.bodies {
            let id = body.id;
            if body.name.len() > MAX_NAME_LEN {
                return Err(CheckError::NameLength(id, body.name.len()));
            }
            if !body.opacity.in_range() {
                return Err(CheckError::Opacity(id, body.opacity.percent()));
            }
            if let Some(tint) = body.color.filter(|tint| !tint.in_range()) {
                return Err(CheckError::Tint(id, tint.hue(), tint.saturation()));
            }
            let made = match self.feature(body.created_by).map(|feature| &feature.kind) {
                Some(FeatureKind::Pattern(_)) => {
                    *copies.entry(body.created_by).or_default() += 1;
                    true
                }
                Some(kind) => kind.new_body() == Some(id),
                None => false,
            };
            if !made {
                return Err(CheckError::Creator(id, body.created_by));
            }
        }
        let design = self.design();
        for (index, feature) in self.features.iter().enumerate() {
            let id = feature.id;
            if feature.name.len() > MAX_NAME_LEN {
                return Err(CheckError::FeatureNameLength(id, feature.name.len()));
            }
            match &feature.kind {
                FeatureKind::Sketch {
                    plane,
                    sketch,
                    sources,
                } => {
                    sketch
                        .check(&design)
                        .map_err(|why| CheckError::Sketch(id, why))?;
                    self.check_plane(index, plane)
                        .map_err(|why| CheckError::SketchPlane(id, why))?;
                    self.check_sources(index, sketch, sources)
                        .map_err(|why| CheckError::SketchLink(id, why))?;
                }
                FeatureKind::Extrude(extrude) => self
                    .check_extrude(index, extrude)
                    .map_err(|why| CheckError::Extrude(id, why))?,
                FeatureKind::Revolve(revolve) => self
                    .check_revolve(index, revolve)
                    .map_err(|why| CheckError::Revolve(id, why))?,
                FeatureKind::Combine(combine) => self
                    .check_combine(index, combine)
                    .map_err(|why| CheckError::Combine(id, why))?,
                FeatureKind::Move(moved) => {
                    moved
                        .check_own(&design)
                        .map_err(|why| CheckError::Move(id, why))?;
                    self.check_motion(index, &moved.bodies, moved.referred())
                        .map_err(|why| CheckError::Move(id, why))?;
                }
                FeatureKind::Mirror(mirror) => {
                    mirror
                        .check_own()
                        .map_err(|why| CheckError::Mirror(id, why))?;
                    self.check_motion(index, &mirror.bodies, mirror.referred())
                        .map_err(|why| CheckError::Mirror(id, why))?;
                }
                FeatureKind::Pattern(pattern) => {
                    pattern
                        .check_own(&design)
                        .map_err(|why| CheckError::Pattern(id, why))?;
                    self.check_motion(index, &pattern.bodies, pattern.referred())
                        .map_err(|why| CheckError::Pattern(id, why))?;
                    let made = copies.get(&id).copied().unwrap_or(0);
                    self.check_copies(id, pattern, made)
                        .map_err(|why| CheckError::Pattern(id, why))?;
                }
                FeatureKind::Align(align) => {
                    align
                        .check_own(&design)
                        .map_err(|why| CheckError::Align(id, why))?;
                    self.check_align(index, align)
                        .map_err(|why| CheckError::Align(id, why))?;
                }
                FeatureKind::Scale(scale) => {
                    scale
                        .check_own(&design)
                        .map_err(|why| CheckError::Scale(id, why))?;
                    self.check_scale(index, scale)
                        .map_err(|why| CheckError::Scale(id, why))?;
                }
                FeatureKind::Split(split) => {
                    split
                        .check_own()
                        .map_err(|why| CheckError::Split(id, why))?;
                    self.check_split(index, split)
                        .map_err(|why| CheckError::Split(id, why))?;
                }
                FeatureKind::Chamfer(chamfer) => {
                    chamfer
                        .check_own(&design)
                        .map_err(|why| CheckError::Chamfer(id, why))?;
                    self.check_blend_edges(index, &chamfer.edges)
                        .map_err(|why| CheckError::Chamfer(id, ChamferError::Edges(why)))?;
                }
                FeatureKind::Shell(shell) => {
                    shell
                        .check_own(&design)
                        .map_err(|why| CheckError::Shell(id, why))?;
                    self.check_face_set(index, shell.body, &shell.open)
                        .map_err(|why| CheckError::Shell(id, why.into()))?;
                }
                FeatureKind::OffsetFace(offset) => {
                    offset
                        .check_own(&design)
                        .map_err(|why| CheckError::OffsetFace(id, why))?;
                    let body = offset.body().expect("checked: it has faces");
                    self.check_face_set(index, body, &offset.faces)
                        .map_err(|why| CheckError::OffsetFace(id, why.into()))?;
                }
                FeatureKind::Fillet(fillet) => {
                    fillet
                        .check_own(&design)
                        .map_err(|why| CheckError::Fillet(id, why))?;
                    self.check_blend_edges(index, &fillet.edges)
                        .map_err(|why| CheckError::Fillet(id, FilletError::Edges(why)))?;
                }
                FeatureKind::FaceDraft(draft) => {
                    draft
                        .check_own(&design)
                        .map_err(|why| CheckError::FaceDraft(id, why))?;
                    let body = draft.body().expect("checked: it has faces");
                    self.check_face_set(index, body, &draft.faces)
                        .map_err(|why| CheckError::FaceDraft(id, why.into()))?;
                    self.check_neutral_plane(index, &draft.neutral)
                        .map_err(|why| CheckError::FaceDraft(id, why))?;
                }
                FeatureKind::Sweep(sweep) => self
                    .check_sweep(index, sweep)
                    .map_err(|why| CheckError::Sweep(id, why))?,
                FeatureKind::Loft(loft) => self
                    .check_loft(index, loft)
                    .map_err(|why| CheckError::Loft(id, why))?,
            }
        }
        if let Some(last) = self.bodies.last()
            && last.id.0 >= self.next_id
        {
            return Err(CheckError::NextId(last.id));
        }
        // Ids held for bodies made before: given out, no body's, held once.
        let mut held = BTreeSet::new();
        for feature in &self.features {
            if let Some(body) = feature.kind.held_body()
                && (body.0 >= self.next_id || self.body(body).is_some() || !held.insert(body))
            {
                return Err(CheckError::Held(feature.id, body));
            }
        }
        if let Some(rollback) = self.rollback
            && self.feature(rollback).is_none()
        {
            return Err(CheckError::Rollback(rollback));
        }
        match self.features.last() {
            Some(last) if last.id.0 >= self.next_id => Err(CheckError::FeatureNextId(last.id)),
            _ => Ok(()),
        }
    }

    /// Checks `plane`, the plane of sketch feature `index`: a face's point
    /// in bounds, its body, if it's there, made by a feature before the
    /// sketch, and the feature its key names, if it's there, before the
    /// sketch. A body or a feature that isn't there is allowed (removing
    /// the body's maker leaves the sketch, which regenerating then fails,
    /// to be put on another plane), but only with an id below `next_id`,
    /// one no body or feature made later can take: otherwise the edit
    /// that made it would be refused, and every one after it, the ids
    /// being handed out again.
    fn check_plane(&self, index: usize, plane: &Plane) -> Result<(), PlaneError> {
        let Some(face) = plane.face() else {
            return Ok(());
        };
        face.check_own()?;
        if !self.body_before(index, face.body) {
            return Err(PlaneError::Body(face.body));
        }
        if !self.maker_before(index, face.maker()) {
            return Err(PlaneError::Maker(face.maker()));
        }
        Ok(())
    }

    /// Checks the sources of the links of `sketch`, feature `index`: one
    /// for each link, in its order, each of a kind the link takes
    /// ([`OutsideRef::takes`]), its own parts right, and what it names
    /// made before the sketch, or not there with ids no later feature or
    /// body can take, as a sketch's face's: a removed source leaves the
    /// link, which then doesn't find it. Another sketch's item is a
    /// sketch's before it; whether it has the item is regenerating's to
    /// find.
    fn check_sources(
        &self,
        index: usize,
        sketch: &Sketch,
        sources: &[LinkSource],
    ) -> Result<(), LinkError> {
        if let Some(extra) = sources.get(sketch.links.len()) {
            return Err(LinkError::Sources(extra.link));
        }
        for (at, link) in sketch.links.iter().enumerate() {
            let id = link.id;
            let from = sources.get(at).ok_or(LinkError::Sources(id))?;
            if from.link != id {
                return Err(LinkError::Sources(from.link));
            }
            if !from.source.takes(link.kind) {
                return Err(LinkError::Kind(id));
            }
            match &from.source {
                OutsideRef::Sketch { sketch: other, .. } => {
                    let before = match self.feature_index(*other) {
                        Some(at) => {
                            at < index
                                && matches!(self.features[at].kind, FeatureKind::Sketch { .. })
                        }
                        None => other.0 < self.next_id,
                    };
                    if !before {
                        return Err(LinkError::Later(id));
                    }
                }
                OutsideRef::Edge(edge) => {
                    edge.check_own().map_err(|why| LinkError::Edge(id, why))?;
                    let makers_before = edge.makers().iter().all(|&m| self.maker_before(index, m));
                    if !self.body_before(index, edge.body) || !makers_before {
                        return Err(LinkError::Later(id));
                    }
                }
                OutsideRef::Face(face) => {
                    face.check_own().map_err(|why| LinkError::Face(id, why))?;
                    if !self.body_before(index, face.body)
                        || !self.maker_before(index, face.maker())
                    {
                        return Err(LinkError::Later(id));
                    }
                }
                OutsideRef::Corner(corner) => {
                    let (PointRef::Corner { body, .. }, Ok(())) = (corner, corner.check_own())
                    else {
                        return Err(LinkError::Corner(id));
                    };
                    let makers_before =
                        (corner.makers().iter()).all(|&m| self.maker_before(index, m));
                    if !self.body_before(index, *body) || !makers_before {
                        return Err(LinkError::Later(id));
                    }
                }
            }
        }
        Ok(())
    }

    /// Whether `feature`, as a face's maker that a reference of feature
    /// `index` names, comes before it: there and before it, or not there
    /// with an id below `next_id`, one no feature made later can take
    /// (see [`Document::check_plane`]).
    fn maker_before(&self, index: usize, feature: FeatureId) -> bool {
        match self.feature_index(feature) {
            Some(maker) => maker < index,
            None => feature.0 < self.next_id,
        }
    }

    /// Whether `body`, as one that a reference of feature `index` names,
    /// is made before it: there and made by a feature before it, or not
    /// there with an id below `next_id` (see [`Document::check_plane`]).
    fn body_before(&self, index: usize, body: BodyId) -> bool {
        match self.body(body) {
            Some(body) => self.maker_before(index, body.created_by),
            None => body.0 < self.next_id,
        }
    }

    /// Checks `extrude`, feature `index`, see [`Document::check`].
    fn check_extrude(&self, index: usize, extrude: &Extrude) -> Result<(), ExtrudeError> {
        if self.sketch_before(index, extrude.sketch).is_none() {
            return Err(ExtrudeError::Sketch(extrude.sketch));
        }
        extrude.check_own(&self.design())?;
        self.check_uses(index, extrude.sketch, &extrude.operation)
            .map_err(|why| match why {
                Uses::Sketch(sketch) => ExtrudeError::Sketch(sketch),
                Uses::NewBody(body) => ExtrudeError::NewBody(body),
                Uses::Excluded(body) => ExtrudeError::Excluded(body),
                Uses::ExcludedOrder => ExtrudeError::ExcludedOrder,
            })
    }

    /// Checks `revolve`, feature `index`, see [`Document::check`].
    fn check_revolve(&self, index: usize, revolve: &Revolve) -> Result<(), RevolveError> {
        if self.sketch_before(index, revolve.sketch).is_none() {
            return Err(RevolveError::Sketch(revolve.sketch));
        }
        revolve.check_own(&self.design())?;
        if let AxisLine::Edge(edge) = &revolve.axis {
            self.check_edge(index, edge)?;
        }
        self.check_uses(index, revolve.sketch, &revolve.operation)
            .map_err(|why| match why {
                Uses::Sketch(sketch) => RevolveError::Sketch(sketch),
                Uses::NewBody(body) => RevolveError::NewBody(body),
                Uses::Excluded(body) => RevolveError::Excluded(body),
                Uses::ExcludedOrder => RevolveError::ExcludedOrder,
            })
    }

    /// Checks `edge`, the axis of a revolve at feature `index` (the
    /// feature count for one added after the last): its own parts, and its
    /// body and its faces' makers made before it, or not there with ids
    /// no later body or feature can take, as a sketch's face's (as
    /// [`Document::check`] has them). What a revolve being set up checks
    /// its edge by as the document changes under it.
    pub fn check_edge(&self, index: usize, edge: &EdgeRef) -> Result<(), RevolveError> {
        edge.check_own().map_err(RevolveError::Edge)?;
        if !self.body_before(index, edge.body) {
            return Err(RevolveError::EdgeBody(edge.body));
        }
        if let Some(&maker) = (edge.makers().iter()).find(|&&m| !self.maker_before(index, m)) {
            return Err(RevolveError::EdgeMaker(maker));
        }
        Ok(())
    }

    /// Checks `axis` as the axis of a move at feature `index` (at the
    /// end for a new one, the count of features) would name it: its own
    /// parts, and an edge's or face's body and its faces' makers before
    /// it, as [`Document::check`] has them. For a panel keeping what it
    /// sets up one the document takes.
    pub fn check_axis_ref(&self, index: usize, axis: &AxisRef) -> Result<(), MotionError> {
        if let Some(referred) = axis.refers() {
            referred.check_own()?;
        }
        self.check_motion(index, &[], axis.refers())
    }

    /// Checks `plane` as the plane of a mirror at feature `index` would
    /// name it, as [`Document::check_axis_ref`] does a move's axis.
    pub fn check_plane_ref(&self, index: usize, plane: &PlaneRef) -> Result<(), MotionError> {
        if let Some(referred) = plane.refers() {
            referred.check_own()?;
        }
        self.check_motion(index, &[], plane.refers())
    }

    /// Checks `loft`, feature `index`, see [`Document::check`]: every
    /// section's and rail's sketch a sketch feature before it, its own
    /// parts, and its operation as an extrude's.
    fn check_loft(&self, index: usize, loft: &Loft) -> Result<(), LoftError> {
        if let Some(section) =
            (loft.sections.iter()).find(|s| self.sketch_before(index, s.sketch()).is_none())
        {
            return Err(LoftError::Sketch(section.sketch()));
        }
        if let Some(rail) =
            (loft.rails.iter()).find(|r| self.sketch_before(index, r.sketch).is_none())
        {
            return Err(LoftError::RailSketch(rail.sketch));
        }
        loft.check_own()?;
        // Checked above: it has sections, each of a sketch before it.
        let sketch = loft.sections[0].sketch();
        self.check_uses(index, sketch, &loft.operation)
            .map_err(|why| match why {
                Uses::Sketch(sketch) => LoftError::Sketch(sketch),
                Uses::NewBody(body) => LoftError::NewBody(body),
                Uses::Excluded(body) => LoftError::Excluded(body),
                Uses::ExcludedOrder => LoftError::ExcludedOrder,
            })
    }

    /// Checks `combine`, feature `index`, see [`Document::check`]: its own
    /// parts, and every body it names there and made by a feature before
    /// it. A body that isn't there is refused, unlike a sketch's face's:
    /// removing a body's maker removes the combine too
    /// ([`Document::removal`]), and an edit that would leave it naming a
    /// body that's gone otherwise (setting the maker to stop making it)
    /// is refused.
    fn check_combine(&self, index: usize, combine: &Combine) -> Result<(), CombineError> {
        combine.check_own()?;
        for body in combine.bodies() {
            if !self.made_before(index, body) {
                return Err(CombineError::Body(body));
            }
        }
        Ok(())
    }

    /// Checks what a move or mirror, feature `index`, names: every body
    /// in `bodies` there and made by a feature before it, as a combine's
    /// ([`Document::check_combine`]), and the edge or face its axis or
    /// plane names (`referred`) on a body and of makers before it, or not
    /// there with ids no later body or feature can take, as a sketch's
    /// face's ([`Document::check_plane`]).
    fn check_motion(
        &self,
        index: usize,
        bodies: &[BodyId],
        referred: Option<motion::Referred<'_>>,
    ) -> Result<(), MotionError> {
        if let Some(&body) = bodies.iter().find(|&&body| !self.made_before(index, body)) {
            return Err(MotionError::Body(body));
        }
        if let Some(referred) = referred {
            if !self.body_before(index, referred.body()) {
                return Err(MotionError::RefBody(referred.body()));
            }
            if let Some(&maker) =
                (referred.makers().iter()).find(|&&m| !self.maker_before(index, m))
            {
                return Err(MotionError::RefMaker(maker));
            }
        }
        Ok(())
    }

    /// Checks the copy bodies of `pattern`, feature `id`, which makes
    /// `made` bodies: joined, none; else one per copy, as
    /// [`Copies::Separate`] lays them out, each a body it makes, none
    /// repeated, and every body it makes among them.
    fn check_copies(
        &self,
        id: FeatureId,
        pattern: &Pattern,
        made: usize,
    ) -> Result<(), MotionError> {
        let listed: &[BodyId] = match &pattern.copies {
            Copies::Joined => &[],
            Copies::Separate(listed) => listed,
        };
        let wanted = if pattern.joins() {
            Some(0)
        } else {
            pattern.separate_count()
        };
        if wanted != Some(listed.len()) || made != listed.len() {
            return Err(MotionError::CopyBodies);
        }
        let mut sorted = listed.to_vec();
        sorted.sort_unstable();
        let repeated = sorted.windows(2).any(|pair| pair[0] == pair[1]);
        let theirs =
            (listed.iter()).all(|&body| self.body(body).is_some_and(|b| b.created_by == id));
        if repeated || !theirs {
            return Err(MotionError::CopyBodies);
        }
        Ok(())
    }

    /// Checks what an align, feature `index`, names: its body there and
    /// made by a feature before it, as a move's; every reference on the
    /// moved side on that body, every one on the target side on another;
    /// and each named as a move's axis is ([`Document::check_motion`]).
    fn check_align(&self, index: usize, align: &Align) -> Result<(), AlignError> {
        if !self.made_before(index, align.body) {
            return Err(AlignError::Body(align.body));
        }
        for named in align.from.named() {
            if named.body != align.body {
                return Err(AlignError::FromBody(named.body));
            }
        }
        for named in align.to.named() {
            if named.body == align.body {
                return Err(AlignError::OnMoved);
            }
        }
        self.check_align_refs(index, &align.to)?;
        self.check_align_refs(index, &align.from)
    }

    /// Checks `refs` as one side of an align at feature `index` (at the
    /// end for a new one) would name them: each reference's own parts,
    /// and its body and its faces' makers before it, as
    /// [`Document::check`] has them. For a panel keeping what it sets up
    /// one the document takes; which side is which body is the caller's.
    pub fn check_align_refs(&self, index: usize, refs: &AlignRefs) -> Result<(), AlignError> {
        refs.point.check_own()?;
        for direction in refs.directions() {
            direction.check_own()?;
        }
        for named in refs.named() {
            if !self.body_before(index, named.body) {
                return Err(AlignError::RefBody(named.body));
            }
            if let Some(&maker) = (named.makers.iter()).find(|&&m| !self.maker_before(index, m)) {
                return Err(AlignError::RefMaker(maker));
            }
        }
        Ok(())
    }

    /// Checks what a scale, feature `index`, names: its bodies there and
    /// made by features before it, as a move's, and its point and its
    /// edge length's edge named as a move's axis is
    /// ([`Document::check_motion`]).
    fn check_scale(&self, index: usize, scale: &Scale) -> Result<(), ScaleError> {
        if let Some(&body) = (scale.bodies.iter()).find(|&&body| !self.made_before(index, body)) {
            return Err(ScaleError::Body(body));
        }
        self.check_scale_refs(index, scale)
    }

    /// Checks the point and the edge of `scale` as a scale at feature
    /// `index` (at the end for a new one) would name them: each one's
    /// body and its faces' makers before it, as [`Document::check`] has
    /// them. For a panel keeping what it sets up one the document takes;
    /// their own parts are [`Scale::check_own`]'s.
    pub fn check_scale_refs(&self, index: usize, scale: &Scale) -> Result<(), ScaleError> {
        for (body, makers) in scale.named() {
            if !self.body_before(index, body) {
                return Err(ScaleError::RefBody(body));
            }
            if let Some(&maker) = (makers.iter()).find(|&&m| !self.maker_before(index, m)) {
                return Err(ScaleError::RefMaker(maker));
            }
        }
        Ok(())
    }

    /// Checks what a split, feature `index`, names: its body, a tool
    /// body and a face tool's body there and made by features before it,
    /// as a combine's ([`Document::check_combine`]); a plane face's body
    /// and any face's maker as a mirror's plane ([`Document::check_motion`]);
    /// a sketch tool's sketch a sketch feature before it; and its new
    /// body, while it keeps both pieces, a body it makes (the id it holds
    /// otherwise is [`Document::check`]'s, as every held id).
    fn check_split(&self, index: usize, split: &Split) -> Result<(), SplitError> {
        if !self.made_before(index, split.body) {
            return Err(SplitError::Body(split.body));
        }
        self.check_split_tool(index, &split.tool)?;
        let id = self.features[index].id;
        if let Some(body) = split.made_body()
            && self.body(body).is_none_or(|body| body.created_by != id)
        {
            return Err(SplitError::NewBody(body));
        }
        Ok(())
    }

    /// Checks what `tool` names as the tool of a split at feature `index`
    /// (at the end for a new one, the count of features), as
    /// [`Document::check`] has it: a tool body, or a face tool's body,
    /// there and made by a feature before it; a plane face's body and any
    /// face's maker as a mirror's plane; a sketch tool's sketch a sketch
    /// feature before it. For a panel keeping what it sets up one the
    /// document takes; its own parts are [`Split::check_own`]'s.
    pub fn check_split_tool(&self, index: usize, tool: &SplitTool) -> Result<(), SplitError> {
        match tool {
            SplitTool::Body(tool) if !self.made_before(index, *tool) => {
                return Err(SplitError::ToolBody(*tool));
            }
            SplitTool::Face(face) if !self.made_before(index, face.body) => {
                return Err(SplitError::FaceBody(face.body));
            }
            SplitTool::Plane(PlaneRef::Face(face)) if !self.body_before(index, face.body) => {
                return Err(SplitError::RefBody(face.body));
            }
            SplitTool::Regions { sketch, .. } | SplitTool::Chain { sketch, .. }
                if self.sketch_before(index, *sketch).is_none() =>
            {
                return Err(SplitError::Sketch(*sketch));
            }
            _ => {}
        }
        if let SplitTool::Plane(PlaneRef::Face(face)) | SplitTool::Face(face) = tool
            && !self.maker_before(index, face.maker())
        {
            return Err(SplitError::RefMaker(face.maker()));
        }
        Ok(())
    }

    /// Checks what a set of faces of `body` at feature `index` (at the
    /// end for a new one, the count of features) names, as
    /// [`Document::check`] has it for a shell's open faces and an offset
    /// face's faces: the body there and made by a feature before it
    /// (depended on, as a combine's bodies), and the faces' makers
    /// before it, or not there with ids no later feature can take, as a
    /// sketch's face's. For a panel keeping what it sets up one the
    /// document takes; the faces' own parts (on `body`, in order) are
    /// the feature's `check_own`'s.
    pub fn check_face_set(
        &self,
        index: usize,
        body: BodyId,
        faces: &[FaceRef],
    ) -> Result<(), FaceSetError> {
        if !self.made_before(index, body) {
            return Err(FaceSetError::Body(body));
        }
        match (faces.iter()).find(|face| !self.maker_before(index, face.maker())) {
            Some(face) => Err(FaceSetError::RefMaker(face.maker())),
            None => Ok(()),
        }
    }

    /// Checks `neutral` as the neutral plane of a draft at feature
    /// `index` (at the end for a new one, the count of features) would
    /// name it, as [`Document::check`] has it: a face's own parts (as
    /// [`FaceDraft::check_own`] checks them too), its body there and
    /// made by a feature before it (the draft depends on it), its key's
    /// maker before it or not there with an id no later feature can
    /// take. For a panel keeping what it sets up one the document takes.
    pub fn check_neutral_plane(
        &self,
        index: usize,
        neutral: &PlaneRef,
    ) -> Result<(), FaceDraftError> {
        let PlaneRef::Face(face) = neutral else {
            return Ok(());
        };
        face.check_own().map_err(FaceDraftError::Neutral)?;
        if !self.made_before(index, face.body) {
            return Err(FaceDraftError::NeutralBody(face.body));
        }
        if !self.maker_before(index, face.maker()) {
            return Err(FaceDraftError::NeutralMaker(face.maker()));
        }
        Ok(())
    }

    /// Checks `sweep`, feature `index`, see [`Document::check`].
    fn check_sweep(&self, index: usize, sweep: &Sweep) -> Result<(), SweepError> {
        if self.sketch_before(index, sweep.sketch).is_none() {
            return Err(SweepError::Sketch(sweep.sketch));
        }
        sweep.check_own(&self.design())?;
        self.check_path(index, sweep.sketch, &sweep.path)?;
        self.check_uses(index, sweep.sketch, &sweep.operation)
            .map_err(|why| match why {
                Uses::Sketch(sketch) => SweepError::Sketch(sketch),
                Uses::NewBody(body) => SweepError::NewBody(body),
                Uses::Excluded(body) => SweepError::Excluded(body),
                Uses::ExcludedOrder => SweepError::ExcludedOrder,
            })
    }

    /// Checks what `path` names as the path of a sweep at feature `index`
    /// (at the end for a new one, the count of features) whose profile
    /// is sketch `profile`, as [`Document::check`] has it: each part's
    /// sketch a sketch feature before it, not the profile's; each edge
    /// part's body there and made by a feature before it (depended on,
    /// as a chamfer's), its faces' makers before it, or not there with
    /// ids no later feature can take; a helix's axis's body and its
    /// faces' makers likewise. For a panel keeping what it sets up one
    /// the document takes; their own parts are [`Sweep::check_own`]'s.
    pub fn check_path(
        &self,
        index: usize,
        profile: FeatureId,
        path: &PathRef,
    ) -> Result<(), SweepError> {
        match path {
            PathRef::Chain(parts) => {
                for part in parts {
                    match part {
                        PathPart::Curves(chain) => {
                            if chain.sketch == profile {
                                return Err(SweepError::OwnSketch);
                            }
                            if self.sketch_before(index, chain.sketch).is_none() {
                                return Err(SweepError::PathSketch(chain.sketch));
                            }
                        }
                        PathPart::Edges { edges, .. } => {
                            for edge in edges {
                                if !self.made_before(index, edge.body) {
                                    return Err(SweepError::EdgeBody(edge.body));
                                }
                                if let Some(&maker) =
                                    (edge.makers().iter()).find(|&&m| !self.maker_before(index, m))
                                {
                                    return Err(SweepError::EdgeMaker(maker));
                                }
                            }
                        }
                    }
                }
            }
            PathRef::Helix(helix) => {
                if let Some(referred) = helix.axis.refers() {
                    if !self.made_before(index, referred.body()) {
                        return Err(SweepError::AxisBody(referred.body()));
                    }
                    if let Some(&maker) =
                        (referred.makers().iter()).find(|&&m| !self.maker_before(index, m))
                    {
                        return Err(SweepError::AxisMaker(maker));
                    }
                }
            }
        }
        Ok(())
    }

    /// Whether `body` is there and made by a feature before feature
    /// `index`.
    fn made_before(&self, index: usize, body: BodyId) -> bool {
        self.body(body)
            .and_then(|body| self.feature_index(body.created_by))
            .is_some_and(|maker| maker < index)
    }

    /// The sketch feature before feature `index` whose id is `sketch`.
    pub(crate) fn sketch_before(&self, index: usize, sketch: FeatureId) -> Option<&Sketch> {
        let before = &self.features[..index];
        let at = before
            .binary_search_by_key(&sketch, |feature| feature.id)
            .ok()?;
        match &before[at].kind {
            FeatureKind::Sketch { sketch, .. } => Some(sketch),
            _ => None,
        }
    }

    /// Checks what extrudes, revolves and lofts share, of feature `index`: its
    /// sketch `sketch` is a sketch feature before it, and `operation`'s
    /// new body names it as its maker and its excluded bodies are sorted
    /// and made by features before it.
    fn check_uses(
        &self,
        index: usize,
        sketch: FeatureId,
        operation: &Operation,
    ) -> Result<(), Uses> {
        if self.sketch_before(index, sketch).is_none() {
            return Err(Uses::Sketch(sketch));
        }
        let id = self.features[index].id;
        if let Some(body) = operation.new_body()
            && self.body(body).is_none_or(|body| body.created_by != id)
        {
            return Err(Uses::NewBody(body));
        }
        let excluded = operation.excluded();
        if !excluded.windows(2).all(|pair| pair[0] < pair[1]) {
            return Err(Uses::ExcludedOrder);
        }
        for &body in excluded {
            if !self.made_before(index, body) {
                return Err(Uses::Excluded(body));
            }
        }
        Ok(())
    }
}

/// What [`Document::check_uses`] finds wrong, which each kind's error
/// says in its own words.
enum Uses {
    Sketch(FeatureId),
    NewBody(BodyId),
    Excluded(BodyId),
    ExcludedOrder,
}

/// Why a [`Document`] fails [`Document::check`], and where.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CheckError {
    /// A body's name is this many bytes, over [`MAX_NAME_LEN`].
    NameLength(BodyId, usize),
    /// A body's opacity is this percent, out of [`Opacity::MIN`] to
    /// [`Opacity::MAX`].
    Opacity(BodyId, u8),
    /// A body's colour is this hue and saturation, out of what
    /// [`Tint::new`] takes.
    Tint(BodyId, u16, u8),
    /// A body's maker isn't an extrude, revolve, sweep, loft or split the document
    /// holds that makes it as its new body, or a pattern (whose own check
    /// holds it to the copy bodies it lists).
    Creator(BodyId, FeatureId),
    /// The first body's id doesn't come after the second's, the one
    /// before it.
    Order(BodyId, BodyId),
    /// The last body's id isn't below the document's next id.
    NextId(BodyId),
    /// A feature holds this id for a body it made before
    /// ([`FeatureKind::held_body`]), but it's a body's, another feature
    /// holds it too, or it isn't below the document's next id.
    Held(FeatureId, BodyId),
    /// A feature's name is this many bytes, over [`MAX_NAME_LEN`].
    FeatureNameLength(FeatureId, usize),
    /// A sketch feature's sketch fails [`Sketch::check`].
    Sketch(FeatureId, SketchError),
    /// A sketch feature's plane is wrong, see [`PlaneError`].
    SketchPlane(FeatureId, PlaneError),
    /// A sketch feature's links' sources are wrong, see [`LinkError`].
    SketchLink(FeatureId, LinkError),
    /// An extrude feature is wrong, see [`ExtrudeError`].
    Extrude(FeatureId, ExtrudeError),
    /// A revolve feature is wrong, see [`RevolveError`].
    Revolve(FeatureId, RevolveError),
    /// A combine feature is wrong, see [`CombineError`].
    Combine(FeatureId, CombineError),
    /// A move feature is wrong, see [`MotionError`].
    Move(FeatureId, MotionError),
    /// A mirror feature is wrong, see [`MotionError`].
    Mirror(FeatureId, MotionError),
    /// A pattern feature is wrong, see [`MotionError`].
    Pattern(FeatureId, MotionError),
    /// An align feature is wrong, see [`AlignError`].
    Align(FeatureId, AlignError),
    /// A scale feature is wrong, see [`ScaleError`].
    Scale(FeatureId, ScaleError),
    /// A split feature is wrong, see [`SplitError`].
    Split(FeatureId, SplitError),
    /// A chamfer feature is wrong, see [`ChamferError`].
    Chamfer(FeatureId, ChamferError),
    /// A shell feature is wrong, see [`ShellError`].
    Shell(FeatureId, ShellError),
    /// A fillet feature is wrong, see [`FilletError`].
    Fillet(FeatureId, FilletError),
    /// An offset face feature is wrong, see [`OffsetFaceError`].
    OffsetFace(FeatureId, OffsetFaceError),
    /// A draft feature is wrong, see [`FaceDraftError`].
    FaceDraft(FeatureId, FaceDraftError),
    /// A sweep feature is wrong, see [`SweepError`].
    Sweep(FeatureId, SweepError),
    /// A loft feature is wrong, see [`LoftError`].
    Loft(FeatureId, LoftError),
    /// The fit tolerance, in millimetres, isn't one [`Tolerance::new`]
    /// takes.
    Tolerance(f64),
    /// The first feature's id doesn't come after the second's, the one
    /// before it.
    FeatureOrder(FeatureId, FeatureId),
    /// The last feature's id isn't below the document's next id.
    FeatureNextId(FeatureId),
    /// The Timeline is rolled back to before a feature that isn't there.
    Rollback(FeatureId),
    /// The parameter at this index is wrong, see [`ParamError`].
    Param(usize, ParamError),
    /// There are this many parameters, over
    /// [`MAX_PARAMS`](varde_expr::MAX_PARAMS).
    Params(usize),
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckError::NameLength(id, len) => write!(
                f,
                "body {} has a name of {len} bytes, over the limit of {MAX_NAME_LEN}",
                id.0
            ),
            CheckError::Opacity(id, percent) => write!(
                f,
                "body {} has an opacity of {percent} %, not from {} to {}",
                id.0,
                Opacity::MIN,
                Opacity::MAX
            ),
            CheckError::Tint(id, hue, saturation) => write!(
                f,
                "body {} has a colour of hue {hue}° and saturation {saturation} %, not below 360° \
                 and at most {} %",
                id.0,
                Tint::MAX_SATURATION
            ),
            CheckError::Creator(id, feature) => write!(
                f,
                "body {} is made by feature {}, which doesn't make it",
                id.0, feature.0
            ),
            CheckError::Order(id, before) => {
                write!(f, "body id {} doesn't come after {}", id.0, before.0)
            }
            CheckError::NextId(id) => write!(f, "body id {} is not below the next id", id.0),
            CheckError::Held(feature, id) => write!(
                f,
                "feature {} holds body id {} for a body it made, which is taken or not given out yet",
                feature.0, id.0
            ),
            CheckError::FeatureNameLength(id, len) => write!(
                f,
                "feature {} has a name of {len} bytes, over the limit of {MAX_NAME_LEN}",
                id.0
            ),
            CheckError::Sketch(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::SketchPlane(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::SketchLink(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Extrude(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Revolve(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Combine(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Move(id, why)
            | CheckError::Mirror(id, why)
            | CheckError::Pattern(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Align(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Scale(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Split(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Chamfer(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Shell(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Fillet(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::OffsetFace(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::FaceDraft(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Sweep(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Loft(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Tolerance(fit) => write!(
                f,
                "the tolerance {fit} mm isn't from {} to {} mm",
                Tolerance::MIN_FIT,
                Tolerance::MAX_FIT
            ),
            CheckError::FeatureOrder(id, before) => {
                write!(f, "feature id {} doesn't come after {}", id.0, before.0)
            }
            CheckError::FeatureNextId(id) => {
                write!(f, "feature id {} is not below the next id", id.0)
            }
            CheckError::Rollback(id) => {
                write!(
                    f,
                    "the timeline is rolled back to feature id {}, which isn't there",
                    id.0
                )
            }
            CheckError::Param(index, why) => write!(f, "parameter {}: {why}", index + 1),
            CheckError::Params(count) => write!(
                f,
                "there are {count} parameters, over the limit of {}",
                varde_expr::MAX_PARAMS
            ),
        }
    }
}

impl std::error::Error for CheckError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CheckError::Sketch(_, why) => Some(why),
            CheckError::SketchPlane(_, why) => Some(why),
            CheckError::Extrude(_, why) => Some(why),
            CheckError::Revolve(_, why) => Some(why),
            CheckError::Combine(_, why) => Some(why),
            CheckError::Move(_, why) | CheckError::Mirror(_, why) | CheckError::Pattern(_, why) => {
                Some(why)
            }
            CheckError::Align(_, why) => Some(why),
            CheckError::Scale(_, why) => Some(why),
            CheckError::Split(_, why) => Some(why),
            CheckError::Chamfer(_, why) => Some(why),
            CheckError::Shell(_, why) => Some(why),
            CheckError::Fillet(_, why) => Some(why),
            CheckError::OffsetFace(_, why) => Some(why),
            CheckError::FaceDraft(_, why) => Some(why),
            CheckError::Sweep(_, why) => Some(why),
            CheckError::Loft(_, why) => Some(why),
            CheckError::Param(_, why) => Some(why),
            _ => None,
        }
    }
}

/// Why an edit was refused. The document is left as it was.
#[derive(Debug, Clone, PartialEq)]
pub enum EditError {
    /// Every id for a body or a feature has been used.
    OutOfIds,
    /// The edit would leave the document failing [`Document::check`], for
    /// the reason given.
    Invalid(CheckError),
    /// An edit of the sketch of this feature can't be made, see
    /// [`SketchEdit::apply`](varde_sketch::SketchEdit::apply).
    Sketch(FeatureId, varde_sketch::EditError),
    /// [`Command::AddFeature`] or [`Command::SetFeature`] was given a
    /// sketch, or asked to set one: sketches are added by
    /// [`Command::AddSketch`] and set by [`Command::SetSketch`].
    SketchKind,
    /// [`Command::SetSketch`] or [`Command::AddLink`] would take away the
    /// sketch face ([`sketch_face`]) of this sketch feature, which stays
    /// while the sketch is on its face.
    SketchFace(FeatureId),
    /// A parameter command would leave this value in error: a feature's
    /// that uses the parameter (directly or through others) or, renaming
    /// one, a parameter's text the new name takes over
    /// [`MAX_LEN`](varde_expr::MAX_LEN).
    Value(ValueOf, varde_expr::Error),
    /// [`Command::RemoveParam`] was asked to remove this parameter, which
    /// a feature value or another parameter uses.
    ParamUsed(String),
    /// A parameter command changes dimensions of this sketch feature's
    /// sketch, which then doesn't solve, see
    /// [`varde_sketch::revalue`].
    Unsolved(FeatureId, varde_sketch::Rejected),
}

/// Whose value an [`EditError::Value`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueOf {
    /// One of this feature's values.
    Feature(FeatureId),
    /// The text of the parameter at this index.
    Param(usize),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::OutOfIds => f.write_str("the document has no ids left"),
            EditError::Invalid(why) => why.fmt(f),
            EditError::Sketch(id, why) => write!(f, "feature {}: {why}", id.0),
            EditError::SketchKind => {
                f.write_str("sketches are added and set by their own commands")
            }
            EditError::SketchFace(_) => f.write_str("the sketch face can't be removed"),
            EditError::Value(ValueOf::Feature(id), why) => {
                write!(f, "feature {}: a value would be in error: {why}", id.0)
            }
            EditError::Value(ValueOf::Param(index), why) => {
                write!(f, "parameter {}: {why}", index + 1)
            }
            EditError::ParamUsed(name) => write!(f, "the parameter '{name}' is in use"),
            EditError::Unsolved(id, why) => {
                write!(f, "feature {}: the sketch wouldn't solve: {why}", id.0)
            }
        }
    }
}

impl std::error::Error for EditError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EditError::OutOfIds
            | EditError::SketchKind
            | EditError::SketchFace(_)
            | EditError::ParamUsed(_) => None,
            EditError::Invalid(why) => Some(why),
            EditError::Value(_, why) => Some(why),
            EditError::Sketch(_, why) => Some(why),
            EditError::Unsolved(_, why) => Some(why),
        }
    }
}
