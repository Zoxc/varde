//! The shell feature: a body already made hollowed out to walls of one
//! thickness, opened through the faces picked to remove.

use std::fmt;

use serde::{Deserialize, Serialize};
use varde_expr::{Ask, Value};

use crate::{BodyId, Design, Extent, FaceRef, FaceSetError, FeatureId, PlaneError};

/// The most faces a shell may open.
pub const MAX_SHELL_FACES: usize = 256;

/// A shell: the body `body` hollowed to walls `thickness` thick, inside
/// its faces or with `outward` outside them, opened through the faces
/// `open` (none: a closed hollow body, its void inside). The body keeps
/// its id; the new faces are named after the shell and the face each
/// is offset from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shell {
    /// A body a feature before it makes.
    pub body: BodyId,
    /// `0..=`[`MAX_SHELL_FACES`] faces, all on `body`, in
    /// [`FaceRef::order`] without repeats. Each is found on the body as
    /// the features before the shell leave it.
    pub open: Vec<FaceRef>,
    /// A length as an extrude's distance ([`Shell::thickness_ask`]).
    pub thickness: Value,
    /// Whether the walls are grown outside the body's faces (the body
    /// becomes the hollow) rather than inside them.
    #[serde(default)]
    pub outward: bool,
}

impl Shell {
    /// What its thickness is checked against in `design`: a length as an
    /// extrude's distance ([`Extent::ask`]).
    pub fn thickness_ask(design: &Design) -> Ask {
        Extent::ask(design)
    }

    /// The bodies it names, which it depends on: its body.
    pub fn bodies(&self) -> Vec<BodyId> {
        vec![self.body]
    }

    /// Checks what needs only the shell and `design`: the face count,
    /// order and body, each face's own parts, and its thickness. What
    /// the body and the faces name is
    /// [`Document::check`](crate::Document::check)'s. Cheap, for a panel
    /// to run on every view.
    pub fn check_own(&self, design: &Design) -> Result<(), ShellError> {
        let count = self.open.len();
        if count > MAX_SHELL_FACES {
            return Err(ShellError::Faces(count));
        }
        for face in &self.open {
            face.check_own().map_err(ShellError::Face)?;
        }
        if !(self.open.windows(2)).all(|pair| pair[0].order(&pair[1]).is_lt()) {
            return Err(ShellError::FaceOrder);
        }
        if self.open.iter().any(|face| face.body != self.body) {
            return Err(ShellError::Bodies);
        }
        (self.thickness)
            .check(&Shell::thickness_ask(design))
            .map_err(|_| ShellError::Thickness)
    }

    /// Its typed values and what each is checked against in `design`.
    pub(crate) fn values_mut(&mut self, design: &Design) -> Vec<(&mut Value, Ask)> {
        vec![(&mut self.thickness, Shell::thickness_ask(design))]
    }
}

/// What's wrong with a shell, see
/// [`CheckError::Shell`](crate::CheckError::Shell).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShellError {
    /// It opens this many faces, over [`MAX_SHELL_FACES`].
    Faces(usize),
    /// Its faces aren't in [`FaceRef::order`], or one is repeated.
    FaceOrder,
    /// A face fails its own check ([`FaceRef::check_own`]).
    Face(PlaneError),
    /// A face is on another body than the shell's.
    Bodies,
    /// Its thickness's expression doesn't give its value, or the value
    /// isn't a length [`Shell::thickness_ask`] takes.
    Thickness,
    /// Its body, this one, isn't there or no feature before it makes it.
    Body(BodyId),
    /// A face's key names this feature, which is the shell itself or
    /// comes after it, or isn't there and has an id a feature made later
    /// could take.
    RefMaker(FeatureId),
}

impl fmt::Display for ShellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShellError::Faces(count) => {
                write!(f, "opens {count} faces, more than {MAX_SHELL_FACES}")
            }
            ShellError::FaceOrder => f.write_str("its faces are out of order or repeated"),
            ShellError::Face(why) => why.fmt(f),
            ShellError::Bodies => f.write_str("a face it opens is on another body"),
            ShellError::Thickness => f.write_str(
                "its thickness's expression doesn't give its value, or it isn't a length it takes",
            ),
            ShellError::Body(body) => write!(
                f,
                "shells body {}, which isn't there or no earlier feature makes",
                body.0
            ),
            ShellError::RefMaker(feature) => write!(
                f,
                "a face it opens was made by feature {}, which doesn't come before it",
                feature.0
            ),
        }
    }
}

impl From<FaceSetError> for ShellError {
    fn from(why: FaceSetError) -> Self {
        match why {
            FaceSetError::Body(body) => ShellError::Body(body),
            FaceSetError::RefMaker(feature) => ShellError::RefMaker(feature),
        }
    }
}

impl std::error::Error for ShellError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ShellError::Face(why) => Some(why),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
