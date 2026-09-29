//! The document model: what the user is editing, independent of any UI.
//!
//! A [`Document`] is plain data, read-only outside this crate. All
//! modifications go through an [`Editor`], which applies [`Command`]s,
//! checks their results and records history for undo/redo.

pub mod codec;
mod editor;
mod example;
mod extrude;
mod feature;
pub mod name;
mod removal;
#[cfg(test)]
mod testing;

pub use codec::DecodeError;
pub use editor::{Command, Editor, Generation, Revision};
pub use extrude::{Extent, Extrude, ExtrudeError, MAX_EXTRUDE_REGIONS, Operation, Targets};
pub use feature::{Feature, FeatureId, FeatureKind, OriginPlane, Placement, Plane};
pub use removal::{Removable, Removal};

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
// The types and limits the model's API names, so that clients editing a
// document need only this crate.
pub use varde_expr::LengthUnit;
pub use varde_kernel::{MAX_COORD, Tolerance};
pub use varde_sketch::{Design, RegionRef, Sketch, SketchError};

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
    /// Stands for the body an extrude not added yet will make
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
/// its name and whether it's shown, and which feature makes it (an
/// extrude making a new body, [`Operation::NewBody`]); its geometry is
/// whatever regenerating the history gives it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Body {
    pub id: BodyId,
    pub name: String,
    pub visible: bool,
    /// The feature that makes the body, listed in the document.
    pub created_by: FeatureId,
}

/// The fields are private so that a document passes [`Document::check`]
/// by construction: the ways to get one are [`Document::default`],
/// [`Document::example`], an [`Editor`] edit, which is checked, and
/// deserializing one, which goes through [`Unchecked::check`].
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
        }
    }
}

/// A [`Document`] as deserialized, before [`Document::check`]: the same
/// fields in the same order, so it reads the same bytes.
///
/// Deserializing a [`Document`] checks it too, but postcard drops the
/// message of an error raised while deserializing, so decoders that want
/// to say what's wrong decode this and [`check`](Unchecked::check) it, as
/// [`Document::from_postcard`] does.
#[derive(Debug, Deserialize)]
pub struct Unchecked {
    bodies: Vec<Body>,
    features: Vec<Feature>,
    units: LengthUnit,
    tolerance: f64,
    next_id: u64,
}

impl Unchecked {
    /// The document, if it passes [`Document::check`].
    pub fn check(self) -> Result<Document, CheckError> {
        let Unchecked {
            bodies,
            features,
            units,
            tolerance,
            next_id,
        } = self;
        let document = Document {
            bodies,
            features,
            units,
            tolerance,
            next_id,
        };
        document.check()?;
        Ok(document)
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
    pub(crate) fn add_feature(
        &mut self,
        name: impl Into<String>,
        kind: FeatureKind,
    ) -> Result<FeatureId, EditError> {
        let id = FeatureId(self.new_id()?);
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

    pub fn feature(&self, id: FeatureId) -> Option<&Feature> {
        self.feature_index(id).map(|index| &self.features[index])
    }

    /// Where feature `id` is in [`features`](Document::features), like
    /// [`body_index`](Document::body_index).
    pub(crate) fn feature_index(&self, id: FeatureId) -> Option<usize> {
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

    /// What the document's sketches are checked against: [`MAX_COORD`]
    /// and its units.
    pub fn design(&self) -> Design {
        Design {
            max: f64::from(MAX_COORD),
            units: self.units,
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
    /// [`Editor::apply`] refuses a command that does: body ids are in
    /// increasing order and below `next_id`, so bodies added later get new
    /// ids and come last, where an edit adds them, and the same for
    /// feature ids; no name is longer than [`MAX_NAME_LEN`]; the tolerance
    /// is one [`Tolerance::new`] takes; every body is made by an extrude
    /// the document holds that names it as its new body, and every such
    /// body is there; every sketch passes [`Sketch::check`] against
    /// [`MAX_COORD`] and the document's units ([`Document::design`]), so
    /// every dimension's expression gives its value in them; and every
    /// extrude uses a sketch feature before it, has regions, distances and
    /// an operation as [`Extrude`] describes, and excludes only bodies
    /// features before it make.
    pub fn check(&self) -> Result<(), CheckError> {
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
        for body in &self.bodies {
            let id = body.id;
            if body.name.len() > MAX_NAME_LEN {
                return Err(CheckError::NameLength(id, body.name.len()));
            }
            let made = self.feature(body.created_by).is_some_and(|feature| {
                matches!(&feature.kind, FeatureKind::Extrude(extrude)
                    if extrude.operation.new_body() == Some(id))
            });
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
                FeatureKind::Sketch { plane: _, sketch } => sketch
                    .check(&design)
                    .map_err(|why| CheckError::Sketch(id, why))?,
                FeatureKind::Extrude(extrude) => self
                    .check_extrude(index, extrude)
                    .map_err(|why| CheckError::Extrude(id, why))?,
            }
        }
        if let Some(last) = self.bodies.last()
            && last.id.0 >= self.next_id
        {
            return Err(CheckError::NextId(last.id));
        }
        match self.features.last() {
            Some(last) if last.id.0 >= self.next_id => Err(CheckError::FeatureNextId(last.id)),
            _ => Ok(()),
        }
    }

    /// Checks `extrude`, feature `index`, see [`Document::check`].
    fn check_extrude(&self, index: usize, extrude: &Extrude) -> Result<(), ExtrudeError> {
        let before = &self.features[..index];
        let sketch = before
            .binary_search_by_key(&extrude.sketch, |feature| feature.id)
            .ok()
            .filter(|&at| matches!(before[at].kind, FeatureKind::Sketch { .. }));
        if sketch.is_none() {
            return Err(ExtrudeError::Sketch(extrude.sketch));
        }
        extrude.check_own(&self.design())?;
        let id = self.features[index].id;
        if let Some(body) = extrude.operation.new_body()
            && self.body(body).is_none_or(|body| body.created_by != id)
        {
            return Err(ExtrudeError::NewBody(body));
        }
        let excluded = extrude.operation.excluded();
        if !excluded.windows(2).all(|pair| pair[0] < pair[1]) {
            return Err(ExtrudeError::ExcludedOrder);
        }
        for &body in excluded {
            let earlier = self
                .body(body)
                .and_then(|body| self.feature_index(body.created_by))
                .is_some_and(|maker| maker < index);
            if !earlier {
                return Err(ExtrudeError::Excluded(body));
            }
        }
        Ok(())
    }
}

/// Why a [`Document`] fails [`Document::check`], and where.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CheckError {
    /// A body's name is this many bytes, over [`MAX_NAME_LEN`].
    NameLength(BodyId, usize),
    /// A body's maker isn't an extrude the document holds that makes it
    /// as its new body.
    Creator(BodyId, FeatureId),
    /// The first body's id doesn't come after the second's, the one
    /// before it.
    Order(BodyId, BodyId),
    /// The last body's id isn't below the document's next id.
    NextId(BodyId),
    /// A feature's name is this many bytes, over [`MAX_NAME_LEN`].
    FeatureNameLength(FeatureId, usize),
    /// A sketch feature's sketch fails [`Sketch::check`].
    Sketch(FeatureId, SketchError),
    /// An extrude feature is wrong, see [`ExtrudeError`].
    Extrude(FeatureId, ExtrudeError),
    /// The fit tolerance, in millimetres, isn't one [`Tolerance::new`]
    /// takes.
    Tolerance(f64),
    /// The first feature's id doesn't come after the second's, the one
    /// before it.
    FeatureOrder(FeatureId, FeatureId),
    /// The last feature's id isn't below the document's next id.
    FeatureNextId(FeatureId),
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckError::NameLength(id, len) => write!(
                f,
                "body {} has a name of {len} bytes, over the limit of {MAX_NAME_LEN}",
                id.0
            ),
            CheckError::Creator(id, feature) => write!(
                f,
                "body {} is made by feature {}, which isn't an extrude making it",
                id.0, feature.0
            ),
            CheckError::Order(id, before) => {
                write!(f, "body id {} doesn't come after {}", id.0, before.0)
            }
            CheckError::NextId(id) => write!(f, "body id {} is not below the next id", id.0),
            CheckError::FeatureNameLength(id, len) => write!(
                f,
                "feature {} has a name of {len} bytes, over the limit of {MAX_NAME_LEN}",
                id.0
            ),
            CheckError::Sketch(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Extrude(id, why) => write!(f, "feature {}: {why}", id.0),
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
        }
    }
}

impl std::error::Error for CheckError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CheckError::Sketch(_, why) => Some(why),
            CheckError::Extrude(_, why) => Some(why),
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
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::OutOfIds => f.write_str("the document has no ids left"),
            EditError::Invalid(why) => why.fmt(f),
            EditError::Sketch(id, why) => write!(f, "feature {}: {why}", id.0),
        }
    }
}

impl std::error::Error for EditError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EditError::OutOfIds => None,
            EditError::Invalid(why) => Some(why),
            EditError::Sketch(_, why) => Some(why),
        }
    }
}
