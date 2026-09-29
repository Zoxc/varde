//! The document model: what the user is editing, independent of any UI.
//!
//! A [`Document`] is plain data, read-only outside this crate. All
//! modifications go through an [`Editor`], which applies [`Command`]s,
//! checks their results and records history for undo/redo.

pub mod codec;
mod editor;
pub mod name;

pub use codec::DecodeError;
pub use editor::{CUBE_SIZE, Command, Editor, Generation, Revision};

use std::fmt;
use std::sync::Arc;

use glam::Vec3;
use serde::{Deserialize, Serialize};
// The types and limits the model's API names, so that clients editing a
// document need only this crate.
pub use varde_kernel::{MAX_COORD, Shape, ShapeError};
pub use varde_sketch::{Sketch, SketchError};

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

/// The longest body name, in bytes, a document may hold. The editor only
/// makes short ones, but a file could carry any length, and the Objects
/// panel lays each name out on the UI thread, where a long enough one
/// stalls it or overflows text shaping.
pub const MAX_NAME_LEN: usize = 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Body {
    pub id: BodyId,
    pub name: String,
    pub shape: Shape,
    pub position: Vec3,
    pub visible: bool,
}

/// The fields are private so that a document passes [`Document::check`]
/// by construction: the ways to get one are [`Document::default`],
/// [`Document::example`], an [`Editor`] edit, which is checked, and
/// deserializing one, which goes through [`Unchecked::check`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Unchecked")]
pub struct Document {
    bodies: Vec<Body>,
    sketches: Vec<Sketch>,
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
    sketches: Vec<Sketch>,
    next_id: u64,
}

impl Unchecked {
    /// The document, if it passes [`Document::check`].
    pub fn check(self) -> Result<Document, CheckError> {
        let Unchecked {
            bodies,
            sketches,
            next_id,
        } = self;
        let document = Document {
            bodies,
            sketches,
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
    /// A document holding the cube [`add_cube`](Document::add_cube) adds
    /// to a new one: "Cube 1", with a corner at the origin.
    pub fn example() -> Self {
        let mut editor = Editor::new(Document::default());
        editor
            .apply(editor.document().add_cube())
            .expect("a new document takes a cube");
        editor.document().clone()
    }

    /// Adds a body with a new id. Fails, leaving the document as it was,
    /// once the ids have run out: `next_id` may come from a file, so it
    /// can be anything. The rest of [`Document::check`] is up to the
    /// caller, as [`Editor::apply`] does.
    pub(crate) fn add_body(
        &mut self,
        name: impl Into<String>,
        shape: Shape,
        position: Vec3,
    ) -> Result<BodyId, EditError> {
        let id = BodyId(self.next_id);
        self.next_id = self.next_id.checked_add(1).ok_or(EditError::OutOfIds)?;
        self.bodies.push(Body {
            id,
            name: name.into(),
            shape,
            position,
            visible: true,
        });
        Ok(id)
    }

    pub fn bodies(&self) -> &[Body] {
        &self.bodies
    }

    pub fn sketches(&self) -> &[Sketch] {
        &self.sketches
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
    /// ids and come last, where an edit adds them, no
    /// name is longer than [`MAX_NAME_LEN`], every coordinate is within
    /// [`MAX_COORD`], every cuboid size and sketch distance above zero,
    /// and every sketch point id names one of that sketch's points.
    pub fn check(&self) -> Result<(), CheckError> {
        for body in &self.bodies {
            let id = body.id;
            if !varde_kernel::position_in_range(body.position) {
                return Err(CheckError::Position(id, body.position));
            }
            if body.name.len() > MAX_NAME_LEN {
                return Err(CheckError::NameLength(id, body.name.len()));
            }
            body.shape
                .check()
                .map_err(|why| CheckError::Shape(id, why))?;
        }
        for (index, sketch) in self.sketches.iter().enumerate() {
            sketch
                .check(f64::from(MAX_COORD))
                .map_err(|why| CheckError::Sketch(index, why))?;
        }
        if let Some(pair) = self.bodies.windows(2).find(|pair| pair[0].id >= pair[1].id) {
            return Err(CheckError::Order(pair[1].id, pair[0].id));
        }
        match self.bodies.last() {
            Some(last) if last.id.0 >= self.next_id => Err(CheckError::NextId(last.id)),
            _ => Ok(()),
        }
    }
}

/// Why a [`Document`] fails [`Document::check`], and where.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CheckError {
    /// A body's position is past [`MAX_COORD`], or not a number.
    Position(BodyId, Vec3),
    /// A body's name is this many bytes, over [`MAX_NAME_LEN`].
    NameLength(BodyId, usize),
    /// A body's shape fails [`Shape::check`].
    Shape(BodyId, ShapeError),
    /// The sketch at this index fails [`Sketch::check`].
    Sketch(usize, SketchError),
    /// The first body's id doesn't come after the second's, the one
    /// before it.
    Order(BodyId, BodyId),
    /// The last body's id isn't below the document's next id.
    NextId(BodyId),
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckError::Position(id, position) => write!(
                f,
                "body {} is at {position}, outside the limit of {MAX_COORD}",
                id.0
            ),
            CheckError::NameLength(id, len) => write!(
                f,
                "body {} has a name of {len} bytes, over the limit of {MAX_NAME_LEN}",
                id.0
            ),
            CheckError::Shape(id, why) => write!(f, "body {}: {why}", id.0),
            CheckError::Sketch(index, why) => write!(f, "sketch {index}: {why}"),
            CheckError::Order(id, before) => {
                write!(f, "body id {} doesn't come after {}", id.0, before.0)
            }
            CheckError::NextId(id) => write!(f, "body id {} is not below the next id", id.0),
        }
    }
}

impl std::error::Error for CheckError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CheckError::Shape(_, why) => Some(why),
            CheckError::Sketch(_, why) => Some(why),
            _ => None,
        }
    }
}

/// Why an edit was refused. The document is left as it was.
#[derive(Debug, Clone, PartialEq)]
pub enum EditError {
    /// Every body id has been used.
    OutOfIds,
    /// The edit would leave the document failing [`Document::check`], for
    /// the reason given.
    Invalid(CheckError),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::OutOfIds => f.write_str("the document has no body ids left"),
            EditError::Invalid(why) => why.fmt(f),
        }
    }
}

impl std::error::Error for EditError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EditError::OutOfIds => None,
            EditError::Invalid(why) => Some(why),
        }
    }
}
