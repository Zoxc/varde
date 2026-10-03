//! What the public operations give back when they fail: the
//! [`KernelError`] and the geometry it is about.
//!
//! Inside the kernel, steps return the small `Copy` [`KernelError`] and
//! match on it (retries, the boolean's naming of a pinch); the operations
//! that make or combine solids ([`extrude`](crate::extrude()),
//! [`revolve`](crate::revolve()), [`boolean`](crate::boolean()),
//! [`touches`](crate::touches), [`assemble`](crate::assemble),
//! [`Solid::transformed`](crate::Solid::transformed)) return a
//! [`Failure`], whose [`Evidence`] only the steps holding both the error
//! and its geometry fill.
//!
//! Evidence never changes an outcome: no result becomes an error or the
//! reverse, and the error is the one the operation would return without
//! it. It is bounded ([`MAX_EVIDENCE`] items of each kind, then
//! [`Evidence::truncated`]), gathered from its own allowance
//! ([`EVIDENCE_WORK`]) rather than the operation's budget, so finding it
//! can't turn an error into [`KernelError::TooComplex`], deterministic as
//! everything the kernel returns, and goes with the error returned: where
//! an operation retries and returns an earlier try's error, it returns
//! that try's evidence.

use glam::DVec3;

use crate::budget::Work;
use crate::mesh::FaceKey;

mod check;
use crate::patch::{Conic, Patch};
use crate::{Budget, KernelError};

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

/// The work, in the units of [`Budget`], gathering one failure's
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

    /// Takes `units` of `work` (an allowance from [`evidence_work`]) for
    /// gathering more, or marks the evidence truncated and says no once
    /// it has run out.
    pub(crate) fn afford(&mut self, work: &mut Work, units: usize) -> bool {
        let ok = work.spend(units).is_ok();
        if !ok {
            self.truncated = true;
        }
        ok
    }
}

/// A fresh allowance of [`EVIDENCE_WORK`] for gathering one failure's
/// evidence.
pub(crate) fn evidence_work() -> Work {
    Work::new(&Budget::new(EVIDENCE_WORK))
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

#[cfg(test)]
mod tests {
    use glam::DVec2;

    use super::*;
    use crate::boolean::BooleanError;
    use crate::mesh::tests::TOL;
    use crate::patch::PatchError;
    use crate::profile::tests::rect;
    use crate::{
        Frame, Motion, Op, Profile, ProfileError, Solid, Sweep, assemble, boolean, extrude,
        revolve, touches,
    };

    /// The error of a failed operation, its evidence checked empty (none
    /// is gathered for these errors).
    fn bare<T: std::fmt::Debug>(result: Result<T, Failure>) -> KernelError {
        let failure = result.unwrap_err();
        assert!(failure.evidence.is_empty(), "{failure:?}");
        failure.error
    }

    /// Each public operation fails with the error it gave before
    /// failures carried evidence.
    #[test]
    fn operations_fail_with_their_errors() {
        let square = Profile {
            loops: vec![rect(DVec2::ZERO, DVec2::ONE, 1)],
        };
        let budget = Budget::DEFAULT;
        assert_eq!(
            bare(extrude(&square, &Frame::XY, 1.0, 1.0, 1, &TOL, &budget)),
            KernelError::Patch(PatchError::Parameter(0.0))
        );
        let across = Profile {
            loops: vec![rect(DVec2::new(-1.0, 0.0), DVec2::new(3.0, 1.0), 1)],
        };
        // Profile errors come with their segments (see
        // `profile/evidence/tests.rs`).
        let crossing = revolve(&across, &Frame::XY, Sweep::Full, 1, &TOL, &budget).unwrap_err();
        assert_eq!(
            crossing.error,
            KernelError::Profile(ProfileError::CrossesAxis(0, 0))
        );
        assert!(!crossing.evidence.is_empty());
        let cube =
            |at: f64, feature| Solid::cuboid(DVec3::splat(at), DVec3::ONE, feature, &TOL).unwrap();
        let (a, b) = (cube(0.0, 1), cube(0.5, 2));
        assert_eq!(
            bare(boolean(&a, &b, Op::Union, &TOL, &Budget::new(10))),
            KernelError::TooComplex
        );
        assert_eq!(
            bare(touches(&a, &b, &TOL, &Budget::new(0))),
            KernelError::TooComplex
        );
        assert_eq!(
            bare(assemble(&[a.clone(), b], &TOL, &Budget::new(10))),
            KernelError::TooComplex
        );
        let far = Motion::translation(DVec3::X * f64::from(crate::MAX_COORD)).unwrap();
        assert!(matches!(
            bare(a.transformed(&far, None, &TOL, &budget)),
            KernelError::Patch(PatchError::Coordinate(_))
        ));
        // A pinch: a hole whose wall comes within the resolution of the
        // tube's. Named so from repair's `Hull`, whose two triangles it
        // keeps (see `failure/check.rs`).
        let half = 0.5 * TOL.resolution();
        let tube = Solid::cylinder(DVec3::ZERO, 1.0, 10.0, 1, &TOL).unwrap();
        let hole = Solid::cylinder(DVec3::Z * 2.0, 1.0 - half, 6.0, 2, &TOL).unwrap();
        let pinch = boolean(&tube, &hole, Op::Difference, &TOL, &Budget::new(100_000)).unwrap_err();
        assert_eq!(pinch.error, KernelError::Boolean(BooleanError::NotManifold));
        assert_eq!(pinch.evidence.patches.len(), 2);
    }

    #[test]
    fn from_error_is_empty() {
        let f = Failure::from(KernelError::TooComplex);
        assert_eq!(f.error, KernelError::TooComplex);
        assert!(f.evidence.is_empty());
        assert_eq!(f.to_string(), KernelError::TooComplex.to_string());
    }

    #[test]
    fn caps_truncate() {
        let mut e = Evidence::default();
        e.add_points((0..MAX_EVIDENCE.points).map(|i| DVec3::splat(i as f64)));
        assert!(!e.truncated);
        assert!(e.within_caps());
        e.add_points([DVec3::ZERO]);
        assert!(e.truncated);
        assert_eq!(e.points.len(), MAX_EVIDENCE.points);
        assert_eq!(e.points[1], DVec3::ONE);

        let mut e = Evidence::default();
        e.add_sketch_curves(0..u64::MAX);
        assert_eq!(e.sketch_curves.len(), MAX_EVIDENCE.sketch_curves);
        assert!(e.truncated && e.within_caps());

        let mut e = Evidence::default();
        e.add_faces(std::iter::empty());
        e.add_curves(std::iter::empty());
        e.add_patches(std::iter::empty());
        assert!(e.is_empty());
    }

    #[test]
    fn allowance_runs_out() {
        let mut e = Evidence::default();
        let mut work = evidence_work();
        assert!(e.afford(&mut work, usize::try_from(EVIDENCE_WORK).unwrap()));
        assert!(!e.truncated);
        assert!(!e.afford(&mut work, 1));
        assert!(e.truncated);
    }
}
