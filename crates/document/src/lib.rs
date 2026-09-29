//! The document model: what the user is editing, independent of any UI.
//!
//! A [`Document`] is plain data, read-only outside this crate. All
//! modifications go through an [`Editor`], which applies [`Command`]s,
//! checks their results and records history for undo/redo.

pub mod codec;
mod editor;
mod feature;
pub mod name;
#[cfg(test)]
mod testing;

pub use codec::DecodeError;
pub use editor::{Command, Editor, Generation, Revision};
pub use feature::{Feature, FeatureId, FeatureKind, OriginPlane, Placement, Plane};

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
// The types and limits the model's API names, so that clients editing a
// document need only this crate.
pub use varde_expr::LengthUnit;
pub use varde_kernel::MAX_COORD;
pub use varde_sketch::{Design, Sketch, SketchError};

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

/// The longest body or feature name, in bytes, a document may hold. The
/// editor only makes short ones, but a file could carry any length, and the
/// side panel lays each name out on the UI thread, where a long enough one
/// stalls it or overflows text shaping.
pub const MAX_NAME_LEN: usize = 1024;

/// A body: a solid the feature history makes. The document holds only
/// its name and whether it's shown, and which feature makes it; its
/// geometry is whatever regenerating the history gives it.
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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Unchecked")]
pub struct Document {
    bodies: Vec<Body>,
    features: Vec<Feature>,
    /// The design's units: what values are shown in, and what a bare
    /// number in a dimension's expression is read in. The model itself is
    /// in millimetres whatever they are.
    units: LengthUnit,
    /// The id the next body or feature gets.
    next_id: u64,
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
    next_id: u64,
}

impl Unchecked {
    /// The document, if it passes [`Document::check`].
    pub fn check(self) -> Result<Document, CheckError> {
        let Unchecked {
            bodies,
            features,
            units,
            next_id,
        } = self;
        let document = Document {
            bodies,
            features,
            units,
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
    /// The document a new design shows off: empty, until there's a
    /// feature that makes a body.
    pub fn example() -> Self {
        Document::default()
    }

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
    /// feature ids, no name is longer than [`MAX_NAME_LEN`], every body
    /// is made by a feature the document holds, and every sketch passes
    /// [`Sketch::check`] against [`MAX_COORD`] and the document's units
    /// ([`Document::design`]), so every dimension's expression gives its
    /// value in them.
    pub fn check(&self) -> Result<(), CheckError> {
        // Features first: a body's creator is found by binary search, which
        // needs them in order.
        if let Some(pair) = self
            .features
            .windows(2)
            .find(|pair| pair[0].id >= pair[1].id)
        {
            return Err(CheckError::FeatureOrder(pair[1].id, pair[0].id));
        }
        for body in &self.bodies {
            let id = body.id;
            if body.name.len() > MAX_NAME_LEN {
                return Err(CheckError::NameLength(id, body.name.len()));
            }
            if self.feature_index(body.created_by).is_none() {
                return Err(CheckError::Creator(id, body.created_by));
            }
        }
        for feature in &self.features {
            let id = feature.id;
            if feature.name.len() > MAX_NAME_LEN {
                return Err(CheckError::FeatureNameLength(id, feature.name.len()));
            }
            match &feature.kind {
                FeatureKind::Sketch { plane: _, sketch } => sketch
                    .check(&self.design())
                    .map_err(|why| CheckError::Sketch(id, why))?,
            }
        }
        if let Some(pair) = self.bodies.windows(2).find(|pair| pair[0].id >= pair[1].id) {
            return Err(CheckError::Order(pair[1].id, pair[0].id));
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
}

/// Why a [`Document`] fails [`Document::check`], and where.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CheckError {
    /// A body's name is this many bytes, over [`MAX_NAME_LEN`].
    NameLength(BodyId, usize),
    /// A body is made by a feature the document doesn't hold.
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
                "body {} is made by feature {}, which isn't there",
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
