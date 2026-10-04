//! Why a feature failed, in words for the Timeline's tooltip and the
//! feature's panel: what the user did and what to try, rather than the
//! kernel's terms. Messages start in lower case, to follow a colon; the
//! view capitalises them where they stand alone, in the Timeline's
//! tooltip and the panel (`varde_view`'s `chrome::sentence`).

use varde_kernel::{BooleanError, KernelError, ProfileError};

/// Why a sketch on a face isn't placed: no face of its body as the
/// features before it leave it has the face's name.
pub(crate) const FACE_NOT_FOUND: &str = "its face wasn't found";

/// Why a sketch on a face isn't placed: the face found isn't a plane.
pub(crate) const FACE_NOT_FLAT: &str = "its face isn't flat";

/// Why a sketch on a face isn't placed: the face's body has no solid
/// when the history reaches the sketch (its maker failed or is gone).
pub(crate) const FACE_BODY_GONE: &str = "its face's body is gone";

/// Why a sketch on a face isn't placed: the placement the face gives
/// isn't one ([`Placement::valid`]), its origin past the coordinate
/// limit.
///
/// [`Placement::valid`]: varde_document::Placement::valid
pub(crate) const FACE_TOO_FAR: &str = "its face is too far out to sketch on";

/// Why a feature made from a sketch that isn't placed fails.
pub(crate) const SKETCH_NOT_PLACED: &str = "its sketch isn't placed";

/// Why a revolve fails: the line its axis names is gone or isn't a line
/// any more.
pub(crate) const AXIS_NOT_FOUND: &str = "axis not found";

/// Why a revolve fails: the line its axis names has no length (its ends
/// at one place).
pub(crate) const AXIS_NO_LENGTH: &str = "its axis line has no length";

/// Why a revolve fails: its regions, moved into the axis's frame, are
/// past the coordinate limit.
pub(crate) const AXIS_TOO_FAR: &str = "its regions are too far from the axis to revolve";

/// Why a revolve about a model edge fails: the edge's body has no solid
/// when the history reaches the revolve (its maker failed or is gone).
pub(crate) const EDGE_BODY_GONE: &str = "its axis edge's body is gone";

/// Why a revolve about a model edge fails: no edge of its body as the
/// features before it leave it is between faces of the edge's names.
pub(crate) const EDGE_NOT_FOUND: &str = "its axis edge wasn't found";

/// Why a revolve about a model edge fails: the edge found isn't a line.
pub(crate) const EDGE_NOT_STRAIGHT: &str = "its axis edge isn't straight";

/// Why a revolve about a model edge fails: aliases name both of the
/// edge's faces by both of its keys, so its direction can't be told.
pub(crate) const EDGE_UNDIRECTED: &str = "its axis edge's direction can't be told";

/// Why a revolve about a model edge fails: an end of the edge is further
/// than the resolution from its sketch's plane.
pub(crate) const EDGE_OFF_PLANE: &str = "its axis edge isn't in the sketch's plane";

/// Why a move fails: the round edge or face its axis names is found but
/// gives no direction a turn can be made about.
pub(crate) const AXIS_NO_DIRECTION: &str = "its axis has no direction";

/// Why a move fails: its offsets don't make a shift (never, checked as
/// they are).
pub(crate) const OFFSET_NOT_FINITE: &str = "its offsets are out of range";

/// Why a move about a model edge fails: the edge found is neither a line
/// nor a circle (or an arc of one).
pub(crate) const EDGE_NOT_AN_AXIS: &str = "its axis edge isn't straight or round";

/// Why a move about a round face fails: the face's body has no solid
/// when the history reaches the move.
pub(crate) const AXIS_FACE_BODY_GONE: &str = "its axis face's body is gone";

/// Why a move about a round face fails: no face of its body as the
/// features before it leave it has the face's name.
pub(crate) const AXIS_FACE_NOT_FOUND: &str = "its axis face wasn't found";

/// Why a move about a round face fails: the face found isn't a cylinder,
/// cone, torus or other surface of revolution.
pub(crate) const AXIS_FACE_NOT_ROUND: &str = "its axis face isn't round";

/// Why a mirror in a face fails: the face's body has no solid when the
/// history reaches the mirror.
pub(crate) const MIRROR_FACE_BODY_GONE: &str = "its mirror face's body is gone";

/// Why a mirror in a face fails: no face of its body as the features
/// before it leave it has the face's name.
pub(crate) const MIRROR_FACE_NOT_FOUND: &str = "its mirror face wasn't found";

/// Why a mirror in a face fails: the face found isn't a plane.
pub(crate) const MIRROR_FACE_NOT_FLAT: &str = "its mirror face isn't flat";

/// Why a pattern fails: its count isn't one it takes (never, checked as
/// it is).
pub(crate) const PATTERN_COUNT: &str = "its count is out of range";
/// A pattern whose copies are bodies of their own lists none for a copy:
/// not of a checked document.
pub(crate) const COPY_BODIES: &str = "its copy bodies aren't listed";

/// What a move, a mirror or a pattern does to a body, for its messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Moving {
    Move,
    Mirror,
    Pattern,
    Align,
    Scale,
}

impl Moving {
    /// "moving", "mirroring", "patterning".
    fn doing(self) -> &'static str {
        match self {
            Moving::Move => "moving",
            Moving::Mirror => "mirroring",
            Moving::Pattern => "patterning",
            Moving::Align => "aligning",
            Moving::Scale => "scaling",
        }
    }
}

/// Why `moving` the body named `body` fails before it's tried: it would
/// take the body past the coordinate limit.
pub(crate) fn out_of_range(moving: Moving, body: &str) -> String {
    let limit = varde_kernel::MAX_COORD;
    format!(
        "{} {body} takes it out of range: every part must stay within {limit} mm of the origin",
        moving.doing()
    )
}

/// Why the kernel couldn't move the body named `body` as `moving` does.
pub(crate) fn moving(moving: Moving, body: &str, error: KernelError) -> String {
    let doing = moving.doing();
    match error {
        KernelError::TooComplex => format!("{doing} {body} is too complex to work out"),
        // A scale down can take detail under the resolution, a scale up
        // or down round coordinates as a move does.
        KernelError::Invalid(_) if moving == Moving::Scale => format!(
            "scaling {body} leaves no clean solid: parts of it come too close together, or \
             get too small, for the tolerance; try a finer tolerance"
        ),
        // Rounding brought patches a hair closer, within the resolution.
        KernelError::Invalid(_) => format!(
            "{doing} {body} leaves no clean solid: rounding brings parts of it too close \
             together; try a finer tolerance"
        ),
        KernelError::Patch(_) => format!("{doing} {body} takes it out of range"),
        error => failed(&format!("{doing} {body}"), error),
    }
}

/// Why putting the body named `body` together with its mirror image
/// failed (a mirror keeping the original, the two meeting).
pub(crate) fn with_image(body: &str, error: KernelError) -> String {
    failed(&format!("joining {body} to its mirror image"), error)
}

/// Why putting the body named `body` together with its pattern's copies
/// failed (copies meeting each other or the original).
pub(crate) fn with_copies(body: &str, error: KernelError) -> String {
    failed(&format!("joining {body} to its copies"), error)
}

/// Why a pattern of `count` copies of the body named `body`, of
/// `patches` patches, isn't tried: together they'd be over the most
/// patches a solid may have.
pub(crate) fn too_many_copies(body: &str, count: u32, patches: usize) -> String {
    format!(
        "{count} copies of {body} are too many to work out: it has {patches} patches, and a \
         body may have {} in all; use fewer copies",
        varde_kernel::MAX_PATCHES
    )
}

/// Which of an align's references a message is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AlignRef {
    Point,
    Primary,
    Secondary,
}

/// Which side of an align a reference is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Moved,
    Target,
}

impl Side {
    /// "on the moved body", "on the target".
    fn on(self) -> &'static str {
        match self {
            Side::Moved => "on the moved body",
            Side::Target => "on the target",
        }
    }
}

/// Why an align's reference `which` on `side` isn't there, `why` saying
/// what's wrong with it ("wasn't found", "is a face that isn't flat"):
/// "its first direction on the target is a face that isn't flat".
pub(crate) fn align_ref(which: AlignRef, side: Side, why: &str) -> String {
    let what = match which {
        AlignRef::Point => "its point",
        AlignRef::Primary => "its first direction",
        AlignRef::Secondary => "its second direction",
    };
    format!("{what} {} {why}", side.on())
}

/// Which reference of a feature a message is about: one of an align's,
/// or a scale's point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Whose {
    Align(AlignRef, Side),
    ScaleCentre,
}

/// Why the reference `whose` isn't there, `why` saying what's wrong
/// with it, as [`align_ref`] words an align's: "its point wasn't found"
/// for a scale's.
pub(crate) fn reference(whose: Whose, why: &str) -> String {
    match whose {
        Whose::Align(which, side) => align_ref(which, side, why),
        Whose::ScaleCentre => format!("its point {why}"),
    }
}

/// Why an align's reference on the target fails: its body was merged
/// into the body the align moves.
pub(crate) const ALIGN_ON_MOVED: &str =
    "is in the moved body now: a feature before this one merged them; pick it on another body";

/// Why an align's reference fails: its body has no solid.
pub(crate) const ALIGN_BODY_GONE: &str = "is on a body that's gone";

/// Why an align fails: a secondary direction on `side` is parallel to its
/// primary, so it says nothing of the turn about it.
pub(crate) fn align_parallel(side: Side) -> String {
    format!(
        "its second direction {} is parallel to its first: pick one across it",
        side.on()
    )
}

/// Why an align fails: its point on `side` (a nearly straight arc's
/// centre) is past the coordinate limit.
pub(crate) fn align_too_far(side: Side) -> String {
    align_ref(AlignRef::Point, side, "is too far out to align by")
}

/// Why an align fails where the document should have refused it (its
/// directions don't pair, or its offset or turn is out of range).
pub(crate) const ALIGN_MALFORMED: &str = "its directions, offset or turn can't be used";

/// Why a scale fails: its edge length's edge isn't on its body as the
/// features before it leave it.
pub(crate) const SCALE_EDGE_NOT_FOUND: &str = "its edge wasn't found";

/// Why a scale fails: its edge is no longer than the resolution, so no
/// factor can be told from it.
pub(crate) const SCALE_EDGE_SHORT: &str =
    "its edge is too short to scale by: no longer than the tolerance can tell";

/// Why a scale fails: the typed length over the edge's is a factor out
/// of range.
pub(crate) const SCALE_TOO_FAR: &str =
    "the length is too far from the edge's: more than a thousand times longer or shorter";

/// Why a scale along its edge's axis only fails: the edge isn't
/// straight.
pub(crate) const SCALE_EDGE_NOT_STRAIGHT: &str =
    "its edge isn't straight, so it can't scale along its axis only";

/// Why a scale along its edge's axis only fails: the edge is straight
/// but not along a world axis (an edit before it tilted it).
pub(crate) const SCALE_EDGE_SLANTED: &str =
    "its edge isn't along an axis any more, so it can't scale along it only";

/// Why a scale fails where the document should have refused it: a
/// factor out of range.
pub(crate) const SCALE_FACTOR: &str = "a factor is out of range";

/// Why a scale fails: the kernel gave up measuring its edge.
pub(crate) fn scale_measure(error: varde_kernel::measure::MeasureError) -> String {
    format!("its edge can't be measured: {error}")
}

/// Why a split by a face's plane fails: the face's body has no solid
/// when the history reaches the split.
pub(crate) const SPLIT_PLANE_BODY_GONE: &str = "its plane face's body is gone";

/// Why a split by a face's plane fails: no face of its body as the
/// features before it leave it has the face's name.
pub(crate) const SPLIT_PLANE_NOT_FOUND: &str = "its plane face wasn't found";

/// Why a split by a face's plane fails: the face found isn't a plane.
pub(crate) const SPLIT_PLANE_NOT_FLAT: &str = "its plane face isn't flat";

/// Why a split by a face's surface fails: the face's body has no solid
/// when the history reaches the split.
pub(crate) const SPLIT_FACE_BODY_GONE: &str = "its face's body is gone";

/// Why a split by a face's surface fails: no face of its body as the
/// features before it leave it has the face's name.
pub(crate) const SPLIT_FACE_NOT_FOUND: &str = "its face wasn't found";

/// Why a split by a face's surface fails: the face's form has no surface
/// to continue past the face (a blend traced along its edges, a drafted
/// face).
pub(crate) const SPLIT_CANT_EXTEND: &str = "its face can't be extended to split with";

/// Why a split by a sketch's regions or line fails: the sketch isn't
/// there (not of a checked document).
pub(crate) const SPLIT_SKETCH_GONE: &str = "its sketch isn't there";

/// Why a split by a sketch's line fails, `error` saying how its curves
/// don't make one.
pub(crate) fn split_line(error: crate::profile::ChainError) -> String {
    use crate::profile::ChainError;
    match error {
        ChainError::Missing => "its line's curves weren't found".to_owned(),
        ChainError::Closed => {
            "its line is closed: split with the region it encloses instead".to_owned()
        }
        ChainError::Branches => "its line's curves don't join end to end into one line".to_owned(),
        ChainError::Profile(error) => format!("its line can't be used: {error}"),
    }
}

/// Why making the tool a split cuts the body named `body` with failed.
pub(crate) fn split_tool(body: &str, error: KernelError) -> String {
    match error {
        KernelError::TooComplex => {
            format!("extending its tool past {body} is too complex to work out")
        }
        KernelError::Patch(_) => format!("{body} is too near the edge of the space to split"),
        KernelError::Profile(ProfileError::Touching(_)) => {
            format!("the line doesn't split {body}: it crosses itself once extended")
        }
        error => format!("its tool past {body} can't be made: {error}"),
    }
}

/// Why splitting the body named `body` failed in the kernel.
pub(crate) fn splitting(body: &str, error: KernelError) -> String {
    failed(&format!("splitting {body}"), error)
}

/// Why a split fails though the kernel split the body named `body`: one
/// side holds nothing of it, so it isn't cut in two.
pub(crate) fn split_one_side(body: &str) -> String {
    format!("{body} lies all on one side: the tool doesn't cut it in two")
}

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
        ProfileError::Nesting(_) => "its loops don't nest as outlines and holes".to_owned(),
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
/// joined to each on its own they were fine, merged they're no solid
/// ([`BooleanError::NotManifold`]).
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
        // The kernel found the result touching itself (two vertices at a
        // pinch), so this is said as a fact. Parts closer than the
        // resolution are named so too: the same thing at the kernel's
        // resolution, mended the same way.
        KernelError::Boolean(BooleanError::NotManifold) => format!(
            "{what} leaves no clean solid: the result would touch itself along an edge or at a \
             point, or come too close to itself; move it to overlap more or to clear it"
        ),
        // What else fails the check: zero-angle corners where faces are
        // tangent (a boss tangent to a plate's edge), thin triangles
        // beside a hole's rim. Hedged, so moving it is a suggestion. No
        // tolerance is offered: a finer one doesn't mend those.
        KernelError::Invalid(_) => format!(
            "{what} leaves no clean solid: parts would be too thin or too close together, as \
             where faces are tangent; if so, move it to overlap more or to clear it"
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

/// Why a combine `doing` its tool named `tool` to its target named
/// `target` failed: "cutting Body 2 from Body 1 leaves no clean solid
/// ...", as an extrude's [`boolean`] with the tool body named in place of
/// "it".
///
/// A tool of more than one piece (`pieces`: a pattern's copies apart, a
/// grid of pins to cut as holes) that runs out of work is said so: what
/// tangent faces would also do isn't the likely cause there, and fewer
/// pieces at once is what mends it.
pub(crate) fn combining(
    doing: Doing,
    target: &str,
    tool: &str,
    pieces: usize,
    error: KernelError,
) -> String {
    let step = combine_step(doing, target, tool);
    match error {
        KernelError::TooComplex if pieces > 1 => format!(
            "{step} is too complex to work out at once: {tool} is {pieces} separate pieces; \
             use fewer, or split them over more than one combine"
        ),
        error => failed(&step, error),
    }
}

/// What a combine step does, in words: "joining Body 2 to Body 1",
/// "cutting Body 2 from Body 1", "intersecting Body 1 with Body 2".
fn combine_step(doing: Doing, target: &str, tool: &str) -> String {
    match doing {
        Doing::Cutting => format!("cutting {tool} from {target}"),
        Doing::Intersecting => format!("intersecting {target} with {tool}"),
        Doing::Joining | Doing::Merging | Doing::Touching => format!("joining {tool} to {target}"),
    }
}

/// Why a combine fails though the kernel worked out `doing` its tool
/// named `tool` to its target named `target`: it would leave nothing of
/// the target, as an extrude's [`emptied`].
pub(crate) fn combine_emptied(doing: Doing, target: &str, tool: &str) -> String {
    let step = combine_step(doing, target, tool);
    match doing {
        Doing::Cutting => format!(
            "{step} would leave nothing of {target}: take {tool} out of the tools, or delete \
             {target}"
        ),
        Doing::Intersecting => {
            format!("{step} would leave nothing of {target}: they don't overlap")
        }
        // A union of solids that aren't empty isn't.
        Doing::Joining | Doing::Merging | Doing::Touching => {
            format!("{step} would leave nothing of {target}")
        }
    }
}

/// Why a combine naming the body named `body` fails when an earlier join
/// or combine consumed it into the body named `holder`: it has no solid
/// of its own any more.
pub(crate) fn consumed(body: &str, holder: &str) -> String {
    format!("{body} is in {holder} now: a feature before this one merged it in")
}

/// Why a combine naming the body named `body` fails when that body has
/// no solid: the feature making it failed.
pub(crate) fn no_solid(body: &str) -> String {
    format!("{body} has no solid: the feature making it failed")
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
