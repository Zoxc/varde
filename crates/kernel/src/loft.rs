//! Lofts: a solid through two or more sections in order, each a closed
//! loop on a plane of its own, or (first or last only) a point.
//!
//! **Not built yet**: [`loft`] is a stand-in with the planned signature
//! that fails with [`KernelError::TooComplex`], so the loft feature
//! above the kernel (its document, regeneration and messages) is built
//! against it. The real implementation replaces this file (and adds
//! what it needs beside it, under `loft/`):
//!
//! - **Correspondence**: each loop runs from its start vertex,
//!   counter-clockwise seen along the loft (a loop whose plane faces back
//!   along the loft runs the other way); a section with no start given
//!   starts at its vertex nearest the previous section's start (a choice
//!   by distance, not a merge). With equal segment counts vertex `i`
//!   meets vertex `i`; otherwise every vertex goes to its fraction of its
//!   loop's length from the start, and each section is split at the
//!   others' fractions (lines at a point, arcs at an angle through
//!   [`trig`](crate::trig), other conics at a parameter by Newton on
//!   length), so pieces correspond one to one, each still an exact conic.
//!   The merged fractions stay within
//!   [`MAX_PROFILE_SEGMENTS`](crate::MAX_PROFILE_SEGMENTS), and sections
//!   times pieces (by `checked_mul`) within a quarter of
//!   [`MAX_PATCHES`](crate::MAX_PATCHES): past either, too complex.
//! - **Ruled** ([`LoftMode::Ruled`], and any loft of two sections): a
//!   strip per pair of corresponding pieces of consecutive sections,
//!   its rulings straight. Two lines span a plane when parallel and a
//!   hyperbolic paraboloid otherwise (linear rulings): exact. Two
//!   conics one the image of the other by a homothety or a translation
//!   (any two circles' arcs on parallel planes with their ends in the
//!   same directions from their centres) span a cone or a cylinder over
//!   the conic, the rulings through the apex by the geometric-mean rule:
//!   exact. To a point section: planes from lines, cones from arcs, the
//!   apex a small fitted cap. Anything else (a line and an arc, arcs
//!   turned against each other, a square to a circle) is a fitted ruled
//!   strip. A hyperbolic-paraboloid strip sharing a ruling with a cone
//!   strip takes the cone's ruling and is fitted.
//! - **Smooth** ([`LoftMode::Smooth`], three or more sections): the
//!   surface meant is the cubic Hermite interpolation of corresponding
//!   points through the sections (Catmull–Rom tangents, natural at open
//!   ends), the strips between sections fitted to it within half the fit
//!   tolerance, the sections exact.
//! - **Rails** (up to [`MAX_RAILS`]): each passes through the matching
//!   vertex of every section within the resolution (a decision stated as
//!   one); the vertex paths follow the rails and the sections are blended
//!   between them; fitted.
//! - **Closed** (three or more sections, no points): the last section
//!   lofts back to the first, no caps.
//! - Caps: the first and last sections (not points) triangulated as
//!   extrude's. Names: the walls are named for `feature` by the first
//!   section's curve each piece belongs to and the section pair (a ruled
//!   loft's spans meet at creases; a smooth loft names every span 0).
//!
//! The sections and rails are in the world, built by the caller
//! (regeneration) from what the feature names: each loop with the frame
//! of its sketch's placement, a point placed by its sketch, a rail a
//! sketch's open chain of curves placed likewise. The kernel knows
//! nothing of sketches.

use glam::DVec3;

use crate::patch::Conic3;
use crate::{Budget, Failure, Frame, KernelError, Loop, Solid, Tolerance};

/// The most sections a loft takes.
pub const MAX_SECTIONS: usize = 64;

/// The most rails a loft takes.
pub const MAX_RAILS: usize = 4;

/// One section of a loft, in the world.
#[derive(Debug, Clone, PartialEq)]
pub enum Section {
    /// A closed loop, one outline with no holes, in the 2D coordinates
    /// of `frame`'s plane; its segments' curve ids name the walls. It
    /// starts at the start of segment `start` (a vertex), or with `None`
    /// at the vertex nearest the previous section's start (the first
    /// section's first segment for the first).
    Loop {
        outline: Loop,
        frame: Frame,
        start: Option<usize>,
    },
    /// A point: only the first or the last section.
    Point(DVec3),
}

/// How a loft runs between its sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoftMode {
    /// Straight rulings between consecutive sections.
    Ruled,
    /// Through the sections smoothly, by the cubic interpolation. A loft
    /// of two sections is ruled whatever its mode.
    Smooth,
}

/// A curve the loft's vertices follow: its conics end to end in the
/// world, an open chain through the matching vertex of every section.
#[derive(Debug, Clone, PartialEq)]
pub struct Rail {
    pub conics: Vec<Conic3>,
}

/// Why a loft gives no solid. Sections and rails are named by their
/// index in what the caller gave.
#[derive(Debug, Clone, PartialEq)]
pub enum LoftError {
    /// Sections `section` and the next one (the first, after the last of
    /// a closed loft) lie on one plane ([`on_one_plane`]).
    OnePlane { section: u32 },
    /// Rail `rail` doesn't pass within the resolution of its vertex of
    /// section `section`.
    RailMisses { rail: u32, section: u32 },
    /// The strips fold over: the sections' starts don't match.
    Twists,
    /// The loft runs into itself, or sections cross (the result fails
    /// the hull rules).
    IntoItself,
    /// The kernel failed otherwise: out of budget, past a limit, its
    /// checks, or sections not shaped as [`loft`] takes them.
    Failed(Failure),
}

impl std::fmt::Display for LoftError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoftError::OnePlane { section } => {
                write!(f, "sections {section} and the next are on one plane")
            }
            LoftError::RailMisses { rail, section } => {
                write!(f, "rail {rail} misses section {section}")
            }
            LoftError::Twists => f.write_str("the loft twists"),
            LoftError::IntoItself => f.write_str("the loft runs into itself"),
            LoftError::Failed(failure) => write!(f, "{}", failure.error),
        }
    }
}

impl std::error::Error for LoftError {}

impl From<Failure> for LoftError {
    fn from(failure: Failure) -> Self {
        LoftError::Failed(failure)
    }
}

impl Section {
    /// Its vertices in the world: a loop's segments' starts placed on its
    /// frame, or the point.
    pub fn vertices(&self) -> Vec<DVec3> {
        match self {
            Section::Loop { outline, frame, .. } => (outline.segments.iter())
                .map(|segment| frame.point(segment.conic.p0, 0.0))
                .collect(),
            Section::Point(point) => vec![*point],
        }
    }
}

/// Whether sections `a` and `b` lie on one plane, which a loft can't run
/// between: every vertex of each within `resolution` of the other's
/// plane (a point section has no plane of its own: a point on the other
/// loop's plane is on one plane with it, two points never are). A
/// decision on geometry stated as one, by `+ − ×` only, so regeneration
/// refuses what the kernel would, to the bit.
pub fn on_one_plane(a: &Section, b: &Section, resolution: f64) -> bool {
    let near = |frame: &Frame, points: &[DVec3]| {
        let normal = frame.normal();
        (points.iter()).all(|&p| (p - frame.origin).dot(normal).abs() <= resolution)
    };
    let (va, vb) = (a.vertices(), b.vertices());
    match (a, b) {
        (Section::Point(_), Section::Point(_)) => false,
        (Section::Loop { frame, .. }, Section::Point(_)) => near(frame, &vb),
        (Section::Point(_), Section::Loop { frame, .. }) => near(frame, &va),
        (Section::Loop { frame: fa, .. }, Section::Loop { frame: fb, .. }) => {
            near(fa, &vb) && near(fb, &va)
        }
    }
}

/// The solid through `sections` in order (`2..=`[`MAX_SECTIONS`], a
/// point only first or last, not all points; with `closed` at least
/// three and no points), `mode` as [`LoftMode`] says, following `rails`
/// (at most [`MAX_RAILS`], none for a closed loft), the faces named for
/// `feature`, as the module's docs describe. The refusals are
/// [`LoftError`]'s. Not built yet: always [`LoftError::Failed`] with
/// [`KernelError::TooComplex`].
pub fn loft(
    _sections: &[Section],
    _mode: LoftMode,
    _closed: bool,
    _rails: &[Rail],
    _feature: u64,
    _tol: &Tolerance,
    _budget: &Budget,
) -> Result<Solid, LoftError> {
    Err(LoftError::Failed(KernelError::TooComplex.into()))
}

#[cfg(test)]
mod tests;
