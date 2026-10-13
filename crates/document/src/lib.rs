//! The document model: what the user is editing, independent of any UI.
//!
//! A [`Document`] is plain data, read-only outside this crate. All
//! modifications go through an [`Editor`], which applies [`Command`]s,
//! checks their results and records history for undo/redo.

mod align;
mod blend;
mod bodies;
mod chamfer;
mod check;
pub mod codec;
mod combine;
mod command;
mod edge;
mod editor;
#[cfg(any(test, feature = "testing"))]
mod example;
mod extrude;
mod face_draft;
mod feature;
mod fillet;
mod loft;
mod motion;
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

pub use align::{Align, AlignError, AlignRefs, DirRef, PointRef, PointRefError};
pub use blend::{
    BlendEdgesError, MAX_BLEND_EDGES, blend_body, blend_excludes, check_blend_edges_own,
};
pub use bodies::{BodiesError, MAX_FEATURE_BODIES};
pub use chamfer::{Chamfer, ChamferError, ChamferSize};
pub use check::CheckError;
pub use codec::DecodeError;
pub use combine::{BodyOp, Combine, CombineError};
pub use command::{Command, EditError, ValueOf};
pub use edge::{EdgeError, EdgeRef};
pub use editor::{Editor, EditorState, Generation, MAX_UNDO, RestoreError, Revision};
pub use extrude::{
    Extent, Extrude, ExtrudeError, MAX_EXTRUDE_REGIONS, Operation, OperationError, Targets,
};
pub use face_draft::{FaceDraft, FaceDraftError, MAX_DRAFT_FACES};
pub use feature::{Feature, FeatureId, FeatureKind};
pub use fillet::{Fillet, FilletError};
pub use loft::{
    Loft, LoftError, LoftMode, MAX_LOFT_RAILS, MAX_LOFT_SECTIONS, MAX_RAIL_CURVES, Section,
};
pub use motion::{Axis3, AxisRef, Mirror, MotionError, Move, RefError};
pub use offset_face::{MAX_OFFSET_FACES, OffsetFace, OffsetFaceError};
pub use opacity::Opacity;
pub use outside::{LinkError, LinkSource, OutsideRef, sketch_face};
pub use param::{Param, ParamError, ParamUses};
pub use pattern::{
    Copies, MAX_PATTERN_BODIES, MAX_PATTERN_COUNT, Pattern, PatternError, PatternKind,
};
pub use plane::{FaceListError, FaceRef, FaceSetError, OriginPlane, Placement, Plane, PlaneError};
pub use removal::{Removable, Removal};
pub use rename::{Named, NewName, Rename};
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
use std::sync::Arc;

use serde::{Deserialize, Serialize};
// The types and limits the model's API names, so that clients editing a
// document need only this crate.
pub use varde_expr::{LengthUnit, Params};
pub use varde_kernel_compat::mesh::{FaceKey, PartKey};
pub use varde_kernel_compat::{MAX_COORD, Tolerance};
pub use varde_sketch::{Design, Id, RegionRef, Sketch, SketchError};

/// Whether `point` is a reference's point a file may hold: finite and
/// within [`MAX_COORD`] of zero in each coordinate.
pub(crate) fn in_bounds(point: glam::DVec3) -> bool {
    point.is_finite() && point.abs().max_element() <= f64::from(MAX_COORD)
}

/// `items` sorted, without repeats: what the lists of ids a feature names
/// or uses promise.
pub(crate) fn sorted_unique<T: Ord>(items: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut items: Vec<T> = items.into_iter().collect();
    items.sort_unstable();
    items.dedup();
    items
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
/// `Document::example` (for tests), an [`Editor`] edit, which is checked, and
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
    /// Where each feature is in `features`, kept in step with it
    /// ([`Document::reindex`]): not stored, but worked out again as a
    /// document is read. Shared, as `resolved` is.
    #[serde(skip)]
    index: Arc<BTreeMap<FeatureId, usize>>,
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
            index: Arc::default(),
        }
    }
}

impl Document {
    /// A new design in `units`: as [`Document::default`], but for them.
    pub fn in_units(units: LengthUnit) -> Self {
        Document {
            units,
            resolved: param::resolve(&[], units),
            ..Document::default()
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
        let mut document = Document {
            bodies,
            features,
            units,
            tolerance,
            next_id,
            rollback,
            params,
            resolved,
            index: Arc::default(),
        };
        document.reindex();
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
    /// [`new_id`](Document::new_id), where the Timeline is rolled back to:
    /// before [`rollback`](Document::rollback)'s feature, which stays the
    /// one rolled back to before so the new feature is the last shown, or
    /// last. The rest of [`Document::check`] is up to the caller, as
    /// [`Editor::apply`] does.
    pub(crate) fn push_feature(
        &mut self,
        name: impl Into<String>,
        kind: FeatureKind,
    ) -> Result<FeatureId, EditError> {
        let id = FeatureId(self.new_id()?);
        let at = self.insert_at();
        self.features.insert(
            at,
            Feature {
                id,
                name: name.into(),
                visible: true,
                kind,
            },
        );
        self.reindex();
        Ok(id)
    }

    /// Works out `index` again after `features` changed.
    pub(crate) fn reindex(&mut self) {
        let index = (self.features.iter().enumerate())
            .map(|(at, feature)| (feature.id, at))
            .collect();
        self.index = Arc::new(index);
    }

    pub fn bodies(&self) -> &[Body] {
        &self.bodies
    }

    /// The features, in the Timeline's order: each uses only those before
    /// it. Ids don't follow it, as a feature added while rolled back goes
    /// in before later ones.
    pub fn features(&self) -> &[Feature] {
        &self.features
    }

    /// The document as it was before feature `until`: the features before
    /// it in the Timeline and the bodies they make, as the Timeline's
    /// rollback shows it. Each feature refers only to those before it, so
    /// what's kept stands on its own. All of it if `until` isn't there.
    #[must_use]
    pub fn before(&self, until: FeatureId) -> Document {
        let mut document = self.clone();
        if let Some(at) = self.feature_index(until) {
            document.features.truncate(at);
            document.reindex();
            let kept: BTreeSet<FeatureId> = document.features.iter().map(|f| f.id).collect();
            document
                .bodies
                .retain(|body| kept.contains(&body.created_by));
        }
        document.rollback = None;
        document
    }

    /// The feature the Timeline is rolled back to before, if it is.
    pub fn rollback(&self) -> Option<FeatureId> {
        self.rollback
    }

    /// Where a new feature goes in [`features`](Document::features): before
    /// the feature rolled back to, or last.
    pub fn insert_at(&self) -> usize {
        (self.rollback)
            .and_then(|rollback| self.feature_index(rollback))
            .unwrap_or(self.features.len())
    }

    pub fn feature(&self, id: FeatureId) -> Option<&Feature> {
        self.feature_index(id).map(|index| &self.features[index])
    }

    /// Where feature `id` is in [`features`](Document::features).
    pub fn feature_index(&self, id: FeatureId) -> Option<usize> {
        self.index.get(&id).copied()
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
}
