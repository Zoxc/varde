//! The shell feature: a body already made hollowed out to walls of one
//! thickness, opened through the faces picked to remove.

use std::fmt;

use serde::{Deserialize, Serialize};
use varde_expr::{Ask, Value};

use crate::plane::check_face_list_own;
use crate::{BodyId, Design, Extent, FaceListError, FaceRef, FaceSetError};

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
    pub fn thickness_ask<'p>(design: &Design<'p>) -> Ask<'p> {
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
        check_face_list_own(&self.open, 0..=MAX_SHELL_FACES, Some(self.body))
            .map_err(ShellError::Faces)?;
        (self.thickness)
            .check(&Shell::thickness_ask(design))
            .map_err(|_| ShellError::Thickness)
    }

    /// Its typed values and what each is checked against in `design`.
    pub(crate) fn values_mut<'p>(&mut self, design: &Design<'p>) -> Vec<(&mut Value, Ask<'p>)> {
        vec![(&mut self.thickness, Shell::thickness_ask(design))]
    }
}

/// What's wrong with a shell, see
/// [`CheckError::Shell`](crate::CheckError::Shell).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShellError {
    /// Its faces fail their own check ([`FaceListError`]).
    Faces(FaceListError),
    /// Its thickness's expression doesn't give its value, or the value
    /// isn't a length [`Shell::thickness_ask`] takes.
    Thickness,
    /// What its faces name ([`Document::check_face_set`](crate::Document::check_face_set)).
    Set(FaceSetError),
}

impl fmt::Display for ShellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShellError::Faces(why) => write!(f, "the faces it opens: {why}"),
            ShellError::Thickness => f.write_str(
                "its thickness's expression doesn't give its value, or it isn't a length it takes",
            ),
            ShellError::Set(why) => why.fmt(f),
        }
    }
}

impl std::error::Error for ShellError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ShellError::Faces(why) => Some(why),
            ShellError::Set(why) => Some(why),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
