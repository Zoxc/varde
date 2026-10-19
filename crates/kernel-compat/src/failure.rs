//! What the public operations give back when they fail: the
//! [`KernelError`] and the geometry it is about.
//!
//! The operations that make or combine solids
//! ([`extrude`](crate::extrude()), [`revolve`](crate::revolve()),
//! [`boolean`](crate::boolean()), [`touches`](crate::touches),
//! [`assemble`](crate::assemble),
//! [`Solid::transformed`](crate::Solid::transformed)) return a
//! [`Failure`]: the error, and the [`Evidence`] it is about. The facade's
//! stubs give none (a [`Failure`] from a bare [`KernelError`]); the
//! regeneration lane fills its own for the errors it finds itself.
//!
//! Evidence never changes an outcome: no result becomes an error or the
//! reverse, and the error is the one the operation would return without
//! it. It is bounded ([`MAX_EVIDENCE`] items of each kind, then
//! [`Evidence::truncated`]), gathered from its own allowance
//! ([`EVIDENCE_WORK`]) rather than the operation's budget, so finding it
//! can't turn an error into [`KernelError::TooComplex`], and is
//! deterministic as everything the kernel returns.

use glam::DVec3;

use crate::KernelError;
use crate::mesh::FaceKey;
use crate::patch::{Conic, Patch};

/// Why a public kernel operation gives no result, and where.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    /// Why.
    pub error: KernelError,
    /// What the error is about; empty for [`KernelError::TooComplex`] and
    /// wherever it isn't known. Boxed, so a `Result` carrying a failure
    /// stays small.
    pub evidence: Box<Evidence>,
}

impl From<KernelError> for Failure {
    /// The failure with no evidence.
    fn from(error: KernelError) -> Self {
        Failure {
            error,
            evidence: Box::default(),
        }
    }
}

impl std::fmt::Display for Failure {
    /// The error's words; the evidence is for drawing.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for Failure {}

/// An operand of a [`boolean`](crate::boolean()) or
/// [`touches`](crate::touches): its first argument `a` or its second `b`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Operand {
    A,
    B,
}

/// The most items of each kind an [`Evidence`] holds; see
/// [`MAX_EVIDENCE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvidenceCaps {
    pub patches: usize,
    pub curves: usize,
    pub points: usize,
    pub sketch_curves: usize,
    pub faces: usize,
}

/// The most items of each kind an [`Evidence`] holds: past them, the
/// first ones in a fixed order and [`Evidence::truncated`]. Enough to
/// show where any failure is (an inside-out shell of a million triangles
/// gives its first 4 096), and small enough to draw at once.
pub const MAX_EVIDENCE: EvidenceCaps = EvidenceCaps {
    patches: 4096,
    curves: 4096,
    points: 256,
    sketch_curves: 4096,
    faces: 256,
};

/// The work, in the units of [`Budget`](crate::Budget), gathering one failure's
/// [`Evidence`] may do, apart from the operation's budget: about 30 ms on
/// one thread. Past it the evidence is what was gathered so far, with
/// [`Evidence::truncated`] set.
pub const EVIDENCE_WORK: u64 = 1 << 16;

/// The geometry a [`Failure`] is about, by value (a failed result is
/// never a [`Solid`](crate::Solid), so has no ids anyone else knows),
/// plus the names that do mean something outside: sketch curves and
/// operand faces. Coordinates are the operation's world coordinates
/// (extrude and revolve place the profile by their frame).
///
/// The fields are open; the methods adding items
/// ([`Evidence::add_points`] and the others) keep to [`MAX_EVIDENCE`],
/// and whoever receives one from elsewhere checks it
/// ([`Evidence::within_caps`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Evidence {
    /// Triangles of a failed result, or of an operand.
    pub patches: Vec<Patch>,
    /// Rational quadratic curves: profile segments placed by the frame,
    /// cut loops, an edge whose winding numbers disagree.
    pub curves: Vec<Conic<DVec3>>,
    /// Points: a pinch, a cusp, a gap's ends, where two segments touch.
    pub points: Vec<DVec3>,
    /// Profile segments by the sketch curve each came from
    /// ([`Segment::curve`](crate::Segment::curve)).
    pub sketch_curves: Vec<u64>,
    /// Operand faces by name, each with its operand.
    pub faces: Vec<(Operand, FaceKey)>,
    /// Some was left out, past [`MAX_EVIDENCE`] or [`EVIDENCE_WORK`].
    pub truncated: bool,
}

impl Evidence {
    /// Whether it holds nothing and left nothing out.
    pub fn is_empty(&self) -> bool {
        self.patches.is_empty()
            && self.curves.is_empty()
            && self.points.is_empty()
            && self.sketch_curves.is_empty()
            && self.faces.is_empty()
            && !self.truncated
    }

    /// Whether every kind is within [`MAX_EVIDENCE`].
    pub fn within_caps(&self) -> bool {
        let c = MAX_EVIDENCE;
        self.patches.len() <= c.patches
            && self.curves.len() <= c.curves
            && self.points.len() <= c.points
            && self.sketch_curves.len() <= c.sketch_curves
            && self.faces.len() <= c.faces
    }

    /// Adds `patches` in order, up to the cap.
    pub fn add_patches(&mut self, patches: impl IntoIterator<Item = Patch>) {
        capped(
            &mut self.patches,
            MAX_EVIDENCE.patches,
            patches,
            &mut self.truncated,
        );
    }

    /// Adds `curves` in order, up to the cap.
    pub fn add_curves(&mut self, curves: impl IntoIterator<Item = Conic<DVec3>>) {
        capped(
            &mut self.curves,
            MAX_EVIDENCE.curves,
            curves,
            &mut self.truncated,
        );
    }

    /// Adds `points` in order, up to the cap.
    pub fn add_points(&mut self, points: impl IntoIterator<Item = DVec3>) {
        capped(
            &mut self.points,
            MAX_EVIDENCE.points,
            points,
            &mut self.truncated,
        );
    }

    /// Adds sketch curve ids in order, up to the cap.
    pub fn add_sketch_curves(&mut self, curves: impl IntoIterator<Item = u64>) {
        capped(
            &mut self.sketch_curves,
            MAX_EVIDENCE.sketch_curves,
            curves,
            &mut self.truncated,
        );
    }

    /// Adds operand faces in order, up to the cap.
    pub fn add_faces(&mut self, faces: impl IntoIterator<Item = (Operand, FaceKey)>) {
        capped(
            &mut self.faces,
            MAX_EVIDENCE.faces,
            faces,
            &mut self.truncated,
        );
    }
}

/// Pushes `items` onto `to` while it holds fewer than `cap`, and sets
/// `truncated` if any are left over.
fn capped<T>(
    to: &mut Vec<T>,
    cap: usize,
    items: impl IntoIterator<Item = T>,
    truncated: &mut bool,
) {
    let mut items = items.into_iter();
    let room = cap.saturating_sub(to.len());
    to.extend(items.by_ref().take(room));
    if items.next().is_some() {
        *truncated = true;
    }
}
