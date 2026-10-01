//! Why a feature failed, in words for the Timeline's tooltip and the
//! extrude panel: what the user did and what to try, rather than the
//! kernel's terms. Messages start in lower case, as the Timeline shows
//! them after the feature's name.

use varde_kernel::{BooleanError, KernelError, ProfileError};

/// What an extrude was doing with a body when the kernel gave up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Doing {
    /// Finding whether it touches the body.
    Touching,
    Joining,
    Cutting,
    Intersecting,
}

impl Doing {
    /// Its name, which also keys what it works out.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Doing::Touching => "finding where it meets",
            Doing::Joining => "joining it to",
            Doing::Cutting => "cutting it from",
            Doing::Intersecting => "intersecting it with",
        }
    }
}

/// Why sweeping an extrude's regions failed.
pub(crate) fn extrude(error: KernelError) -> String {
    match error {
        // Out of budget or past a limit: a coarser tolerance changes
        // neither, so it isn't offered.
        KernelError::TooComplex => {
            "its regions are too complex to extrude: try fewer or simpler curves".to_owned()
        }
        KernelError::Invalid(_) => "its regions have parts too thin or too close together for \
             this tolerance: try a finer tolerance"
            .to_owned(),
        KernelError::Patch(_) => "its regions are too far out or too large to extrude".to_owned(),
        KernelError::Profile(error) => profile(error),
        // An extrude makes no boolean.
        KernelError::Boolean(error) => format!("it couldn't be extruded: {error}"),
    }
}

/// Why a profile can't be extruded.
fn profile(error: ProfileError) -> String {
    match error {
        ProfileError::Touching(_) => {
            "its outline touches or crosses itself, or comes too close to itself".to_owned()
        }
        ProfileError::Nesting => "its loops don't nest as outlines and holes".to_owned(),
        ProfileError::Cusp(..) => "its outline turns back on itself in a sharp point".to_owned(),
        ProfileError::Area(_) => "a loop of its outline encloses no area".to_owned(),
        ProfileError::TooManySegments(_) => "its outline has too many curves".to_owned(),
        ProfileError::TooFine(..) => {
            "its outline has detail too small for this tolerance: try a finer tolerance".to_owned()
        }
        ProfileError::Triangulation => {
            "its end faces couldn't be made: parts are too thin at this tolerance".to_owned()
        }
        ProfileError::Empty
        | ProfileError::Short(_)
        | ProfileError::Segment(..)
        | ProfileError::Degenerate(..)
        | ProfileError::Open(..) => format!("its outline can't be extruded: {error}"),
    }
}

/// Why `doing` the extrude and the body named `body` failed.
pub(crate) fn boolean(doing: Doing, body: &str, error: KernelError) -> String {
    let doing = doing.name();
    match error {
        KernelError::TooComplex => format!(
            "{doing} {body} is too complex to work out: they may meet on faces that are \
             tangent or nearly flush"
        ),
        // Hedged: not every boolean `Invalid` is such a contact (a tall
        // part cut through can fail at every tolerance), so the cause is
        // a guess and moving it a suggestion. No tolerance is offered: a
        // finer one doesn't mend those either.
        KernelError::Invalid(_) => format!(
            "{doing} {body} leaves no clean solid: they may meet only along an edge, at a \
             point, or on tangent faces; if so, move it to overlap more or to clear it"
        ),
        KernelError::Boolean(BooleanError::Inconsistent) => format!(
            "{doing} {body} can't be worked out: they meet on faces too nearly flush or \
             tangent to tell apart; move it a little"
        ),
        KernelError::Boolean(BooleanError::Degenerate) => {
            format!("{doing} {body} leaves a face that can't be made: parts are too thin")
        }
        KernelError::Patch(_) => format!("{doing} {body} goes out of bounds"),
        KernelError::Profile(error) => format!("{doing} {body} failed: {error}"),
    }
}

#[cfg(test)]
mod tests;
