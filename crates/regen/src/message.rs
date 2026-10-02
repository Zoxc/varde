//! Why a feature failed, in words for the Timeline's tooltip and the
//! feature's panel: what the user did and what to try, rather than the
//! kernel's terms. Messages start in lower case, to follow a colon; the
//! view capitalises them where they stand alone, in the Timeline's
//! tooltip and the panel (`varde_view`'s `chrome::sentence`).

use varde_kernel::{BooleanError, KernelError, ProfileError};

/// Why a sketch on a face isn't drawn or used: placing sketches on faces
/// isn't done yet.
pub(crate) const FACE_NOT_PLACED: &str = "sketches on faces can't be placed yet";

/// Why a feature made from a sketch that isn't placed fails.
pub(crate) const SKETCH_NOT_PLACED: &str = "its sketch isn't placed";

/// What a feature was doing with a body when the kernel gave up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Doing {
    /// Finding whether it touches the body.
    Touching,
    Joining,
    Cutting,
    Intersecting,
    /// Uniting a body a join touches with the others it touches (the key's
    /// name only: [`merging`] words its errors).
    Merging,
}

impl Doing {
    /// Its name, which also keys what it works out.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Doing::Touching => "finding where it meets",
            Doing::Joining => "joining it to",
            Doing::Cutting => "cutting it from",
            Doing::Intersecting => "intersecting it with",
            Doing::Merging => "merging",
        }
    }
}

/// What a feature makes its tool solid by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Making {
    Extrude,
    Revolve,
}

impl Making {
    /// The verb: "extrude", "revolve".
    fn verb(self) -> &'static str {
        match self {
            Making::Extrude => "extrude",
            Making::Revolve => "revolve",
        }
    }

    /// Its past participle: "extruded", "revolved".
    fn done(self) -> &'static str {
        match self {
            Making::Extrude => "extruded",
            Making::Revolve => "revolved",
        }
    }
}

/// Why making a feature's tool from its regions (`making` them) failed,
/// at the finest tolerance there is if `finest` (where a finer one can't
/// be suggested).
pub(crate) fn tool(making: Making, error: KernelError, finest: bool) -> String {
    let verb = making.verb();
    match error {
        // Out of budget or past a limit: a coarser tolerance changes
        // neither, so it isn't offered.
        KernelError::TooComplex => {
            format!("its regions are too complex to {verb}: try fewer or simpler curves")
        }
        KernelError::Invalid(_) if finest => format!(
            "its regions have parts too thin or too close together to {verb}, even at the \
             finest tolerance"
        ),
        KernelError::Invalid(_) => "its regions have parts too thin or too close together for \
             this tolerance: try a finer tolerance"
            .to_owned(),
        KernelError::Patch(_) => format!("its regions are too far out or too large to {verb}"),
        KernelError::Profile(error) => profile(making, error, finest),
        // Making a tool makes no boolean.
        KernelError::Boolean(error) => format!("it couldn't be {}: {error}", making.done()),
    }
}

/// Why a profile can't be made into a tool (`making` it), at the finest
/// tolerance if `finest`.
fn profile(making: Making, error: ProfileError, finest: bool) -> String {
    let verb = making.verb();
    match error {
        ProfileError::Touching(_) => {
            "its outline touches or crosses itself, or comes too close to itself".to_owned()
        }
        ProfileError::Nesting => "its loops don't nest as outlines and holes".to_owned(),
        ProfileError::Cusp(..) => "its outline turns back on itself in a sharp point".to_owned(),
        ProfileError::Area(_) => "a loop of its outline encloses no area".to_owned(),
        ProfileError::TooManySegments(_) => "its outline has too many curves".to_owned(),
        ProfileError::TooFine(..) if finest => {
            format!("its outline has detail too small to {verb}, even at the finest tolerance")
        }
        ProfileError::TooFine(..) => {
            "its outline has detail too small for this tolerance: try a finer tolerance".to_owned()
        }
        // Separation leaves every vertex clear of the chords it doesn't
        // end, so spade or the builder refusing isn't something a finer
        // tolerance is known to mend: none is named.
        ProfileError::Triangulation => "its end faces couldn't be made".to_owned(),
        // Only a revolve refuses these.
        ProfileError::CrossesAxis(..) => "its outline crosses the axis".to_owned(),
        ProfileError::TouchesAxis(..) => {
            "its outline touches the axis at a single point".to_owned()
        }
        ProfileError::NearlyFullTurn => {
            "its turn is so nearly full that its ends touch: make it a full turn".to_owned()
        }
        ProfileError::Empty
        | ProfileError::Short(_)
        | ProfileError::Segment(..)
        | ProfileError::Degenerate(..)
        | ProfileError::Open(..) => format!("its outline can't be {}: {error}", making.done()),
    }
}

/// Why `doing` the feature's tool and the body named `body` failed.
pub(crate) fn boolean(doing: Doing, body: &str, error: KernelError) -> String {
    failed(&format!("{} {body}", doing.name()), error)
}

/// Why merging the body named `other`, which a join touches, into the
/// body named `into`, which it touches too and which already holds the
/// feature's tool, failed, and that unticking `other` keeps it apart. Mostly
/// the two meet along an edge or at a corner the tool doesn't cover:
/// joined to each on its own they were fine, merged they're no solid.
pub(crate) fn merging(into: &str, other: &str, error: KernelError) -> String {
    let why = failed(&format!("merging {other} into {into}"), error);
    format!("{why}; or untick {other} under Bodies to keep it apart")
}

/// Why `what` (doing something with two solids) failed.
fn failed(what: &str, error: KernelError) -> String {
    match error {
        KernelError::TooComplex => format!(
            "{what} is too complex to work out: they may meet on faces that are tangent or \
             nearly flush"
        ),
        // Hedged: not every boolean `Invalid` is such a contact (thin cap
        // triangles left next to a hole's rim fail too), so the cause is
        // a guess and moving it a suggestion. No tolerance is offered: a
        // finer one doesn't mend those either.
        KernelError::Invalid(_) => format!(
            "{what} leaves no clean solid: they may meet only along an edge, at a point, or on \
             tangent faces; if so, move it to overlap more or to clear it"
        ),
        KernelError::Boolean(BooleanError::Inconsistent) => format!(
            "{what} can't be worked out: they meet on faces too nearly flush or tangent to \
             tell apart; move it a little"
        ),
        KernelError::Boolean(BooleanError::Degenerate) => {
            format!("{what} leaves a face that can't be made: parts are too thin")
        }
        KernelError::Patch(_) => format!("{what} goes out of bounds"),
        KernelError::Profile(error) => format!("{what} failed: {error}"),
    }
}

/// `message`, why a feature failed with the body named `body`, with
/// the way past it when the feature works on other bodies too: leaving
/// that one out (unticked, it stays as it is and the others are still
/// worked on).
pub(crate) fn leave_out(message: String, body: &str) -> String {
    format!("{message}; untick {body} under Bodies to leave it out")
}

/// Why `doing` the feature's tool and the body named `body` fails though the
/// kernel worked it out: it would leave nothing of the body. Bodies are
/// the document's, so an emptied one would stay listed with no geometry.
pub(crate) fn emptied(doing: Doing, body: &str) -> String {
    let name = doing.name();
    match doing {
        Doing::Cutting => format!(
            "{name} {body} would leave nothing of it: untick it under Bodies to keep it as it \
             is, or delete the body"
        ),
        // The tool touches the body (`touches` counts flush faces, an edge
        // on a face and a corner) without overlapping it: most likely
        // drawn on a face and extruded away from the body.
        Doing::Intersecting => format!(
            "{name} {body} would leave nothing of it: they touch but don't overlap; flip it \
             or move it to overlap"
        ),
        // A union of two solids that aren't empty isn't empty, and
        // finding where they meet makes no solid.
        Doing::Joining | Doing::Touching | Doing::Merging => {
            format!("{name} {body} would leave nothing of it")
        }
    }
}

#[cfg(test)]
mod tests;
