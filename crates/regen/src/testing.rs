//! What tests set up of regeneration: stand-ins run in place of the
//! kernel operations that aren't built yet, per thread, so tests (this
//! crate's and, by the `testing` feature, other crates') can see what's
//! done with their results. Each operation's stand-in lives beside it,
//! in its `history` module's `stand_in`; `crate::history` asks for the
//! operation through `overrides`.

use std::cell::Cell;

use crate::history::stand_ins::{self, *};

/// The kernel operations this thread runs in place of the kernel's;
/// `None` runs the kernel's.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Overrides {
    pub(crate) chamfer: Option<Chamferer>,
    pub(crate) draft: Option<Drafter>,
    pub(crate) fillet: Option<Filleter>,
    pub(crate) loft: Option<Lofter>,
    pub(crate) offset: Option<Offsetter>,
    pub(crate) shell: Option<Sheller>,
    pub(crate) split: Option<Splitter>,
    pub(crate) sweep: Option<Sweeper>,
    pub(crate) taper: Option<Taperer>,
}

thread_local! {
    /// What this thread's tests set.
    static OVERRIDES: Cell<Overrides> = Cell::new(Overrides::default());
}

/// The operations this thread runs.
pub(crate) fn overrides() -> Overrides {
    OVERRIDES.get()
}

/// Changes what this thread runs from now on.
pub(crate) fn set(change: impl FnOnce(&mut Overrides)) {
    let mut overrides = OVERRIDES.get();
    change(&mut overrides);
    OVERRIDES.set(overrides);
}

/// Splits on this thread by two booleans from now on (the front, in
/// the tool, its intersection with the body, the back the body less
/// it) in place of the kernel's split, which isn't built yet: so
/// other crates' tests can see what's done with the pieces.
pub fn split_by_booleans() {
    set(|o| o.split = Some(stand_ins::split));
}

/// Chamfers on this thread by a stand-in in place of the kernel's
/// chamfer, which isn't built yet: each edge a straight convex one
/// between two flat faces, cut off by a triangular prism past its
/// ends (right for a block's edges); anything else is too complex.
pub fn chamfer_by_wedges() {
    set(|o| o.chamfer = Some(stand_ins::chamfer));
}

/// Shells on this thread by a stand-in in place of the kernel's
/// shell, which isn't built yet: a box along the world's axes
/// hollowed by one boolean with another box; anything else is too
/// complex.
pub fn shell_by_boxes() {
    set(|o| o.shell = Some(stand_ins::shell));
}

/// Fillets on this thread by a stand-in in place of the kernel's
/// fillet, which isn't built yet: each edge a straight convex one
/// between two flat faces, rounded off by cutting away a prism past
/// its ends whose section is the corner less the round (right for a
/// block's edges); anything else is too complex.
pub fn fillet_by_arcs() {
    set(|o| o.fillet = Some(stand_ins::fillet));
}

/// Offsets faces on this thread by a stand-in in place of the
/// kernel's offset face, which isn't built yet: a box along the
/// world's axes with its picked faces moved by a scale along each
/// axis and a move, so every face keeps its name; anything else is
/// too complex.
pub fn offset_by_boxes() {
    set(|o| o.offset = Some(stand_ins::offset));
}

/// Drafts faces on this thread by a stand-in in place of the
/// kernel's draft, which isn't built yet: a box along the world's
/// axes, the pull along one, its sides along the pull turned about
/// their hinges on the neutral plane by moving their corners, so
/// every face keeps its name; anything else is too complex.
pub fn draft_by_boxes() {
    set(|o| o.draft = Some(stand_ins::draft));
}

/// Sweeps on this thread by a stand-in in place of the kernel's
/// sweep, which isn't built yet: a path of straight pieces all along
/// one line is the profile extruded along it; anything else is too
/// complex.
pub fn sweep_by_extrude() {
    set(|o| o.sweep = Some(stand_ins::sweep));
}

/// Lofts on this thread by a stand-in in place of the kernel's loft,
/// which isn't built yet: two parallel sections of straight sides,
/// the second the first moved along its normal, make the first
/// extruded to the second; anything else is too complex.
pub fn loft_by_extrude() {
    set(|o| o.loft = Some(stand_ins::loft));
}

/// Tapers extrudes on this thread by a stand-in in place of the
/// kernel's tapered extrude, which isn't built yet: a rectangle along
/// the sketch's axes on one side of its plane, its box's corners
/// moved in (out for a negative taper) into a frustum of a pyramid;
/// anything else is too complex.
pub fn taper_by_frustum() {
    set(|o| o.taper = Some(stand_ins::taper));
}
