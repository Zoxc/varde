//! The combine feature: bodies already made united with, subtracted from
//! or intersected with another, the target, as a step of the history.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::bodies::check_bodies;
use crate::{BodiesError, BodyId, MAX_FEATURE_BODIES};

/// A combine: the target's solid, as the features before it leave it,
/// united with, less, or intersected with each tool's, one boolean a
/// tool in the order the tools were made. The target keeps its id and
/// gets the result; the tools are *consumed* (they have no solid of their
/// own after it and live on in the target, as a join merging bodies
/// leaves them) unless `keep_tools` is set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Combine {
    /// A body a feature before it makes.
    pub target: BodyId,
    /// `1..=`[`MAX_FEATURE_BODIES`] bodies features before it make,
    /// sorted without repeats, the target not among them.
    pub tools: Vec<BodyId>,
    pub op: BodyOp,
    /// Leaves the tools as they were, each still a body of its own;
    /// otherwise they're consumed into the target.
    pub keep_tools: bool,
}

/// What a combine does with its tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BodyOp {
    /// The target gets the union of it and the tools.
    Union,
    /// The tools are cut from the target.
    Subtract,
    /// The target keeps only what it shares with every tool.
    Intersect,
}

impl BodyOp {
    /// Its name as the panel and the Timeline show it: "Union",
    /// "Subtract", "Intersect".
    pub fn label(self) -> &'static str {
        match self {
            BodyOp::Union => "Union",
            BodyOp::Subtract => "Subtract",
            BodyOp::Intersect => "Intersect",
        }
    }
}

impl Combine {
    /// The bodies it names: the target, then the tools.
    pub fn bodies(&self) -> impl Iterator<Item = BodyId> + '_ {
        std::iter::once(self.target).chain(self.tools.iter().copied())
    }

    /// Checks what needs only the combine: the tool count, the tools
    /// sorted without repeats, and the target not among them. The bodies
    /// being there and made before it are
    /// [`Document::check`](crate::Document::check)'s. Cheap, for a panel
    /// to run on every view, as [`Extrude::check_own`](crate::Extrude::check_own).
    pub fn check_own(&self) -> Result<(), CombineError> {
        check_bodies(&self.tools).map_err(CombineError::Tools)?;
        if self.tools.binary_search(&self.target).is_ok() {
            return Err(CombineError::TargetIsTool(self.target));
        }
        Ok(())
    }
}

/// What's wrong with a combine, see
/// [`CheckError::Combine`](crate::CheckError::Combine).
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum CombineError {
    /// Its tools are none, over [`MAX_FEATURE_BODIES`], unsorted or
    /// repeated.
    #[error(fmt = tools_message)]
    Tools(#[source] BodiesError),
    /// Its target, this body, is among its tools too.
    #[error("its target, body {}, is one of its tools too", .0.0)]
    TargetIsTool(BodyId),
    /// It names this body, which isn't there or which no feature before
    /// it makes.
    #[error("combines body {}, which isn't there or no earlier feature makes", .0.0)]
    Body(BodyId),
}

/// [`CombineError::Tools`]: a [`BodiesError`] in the tools' words.
fn tools_message(why: &BodiesError, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match why {
        BodiesError::Count(count) => write!(
            f,
            "combines {count} tool bodies, not 1 to {MAX_FEATURE_BODIES}"
        ),
        BodiesError::Order => f.write_str("its tool bodies are out of order or repeated"),
    }
}

#[cfg(test)]
mod tests;
