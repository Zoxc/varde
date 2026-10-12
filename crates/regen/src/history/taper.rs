//! A tapered extrude's tool: its regions extruded as an untapered
//! extrude's are, its walls leaning by the taper, by the kernel's
//! [`varde_kernel_compat::extrude_tapered`]. What's done with the tool (a new
//! body, or touches and booleans) is an extrude's (`Run::evaluate`).
//!
//! The tool is cached as the untapered extrude's, with the taper's bits
//! (an extrude with no taper keeps the key it always had). The kernel's
//! refusals are worded for the Timeline (`why::taper_refused`); its
//! running out of budget as "tapering its walls is too complex to work
//! out"; its other failures as an untapered tool's.
//!
//! The kernel's tapered extrude isn't built yet: any taper fails as too
//! complex, which reaches the user as that message, and the rest of the
//! history goes on.

use varde_kernel_compat::{Budget, Frame, KernelError, Profile, Solid, TaperError, Tolerance};

use super::{Failed, Run};
use crate::why::{self, TaperRefusal};

#[cfg(any(test, feature = "testing"))]
pub(crate) mod stand_in;

/// The kernel's tapered extrude, which tests may replace with a
/// stand-in to check what regeneration does with the result before the
/// kernel's is built.
pub(crate) type Taperer =
    fn(&Profile, &Frame, f64, f64, f64, u64, &Tolerance, &Budget) -> Result<Solid, TaperError>;

kernel_op! {
    /// The tapered extrude to run: the kernel's, or the one a test set.
    pub(super) fn taperer() -> Taperer = varde_kernel_compat::extrude_tapered, taper;
}

impl Run<'_> {
    /// Why the kernel's tapered extrude gave no tool, in words, with its
    /// failure's evidence where it has one.
    pub(super) fn taper_refused(&self, error: TaperError) -> Failed {
        let why = match error {
            TaperError::Closes => TaperRefusal::Closes,
            TaperError::TooSteep => TaperRefusal::TooSteep,
            TaperError::OutOfRange => TaperRefusal::OutOfRange,
            TaperError::Failed(failure) if failure.error == KernelError::TooComplex => {
                return why::TAPER_TOO_COMPLEX.into();
            }
            TaperError::Failed(failure) => return self.kernel_error(failure),
        };
        why::taper_refused(why).into()
    }
}
