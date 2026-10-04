//! Evaluating a loft's tool: its sections placed in the world in order
//! and lofted by the kernel's [`varde_kernel::loft::loft`]. What's done
//! with the tool (a new body, or touches and booleans) is an extrude's
//! (`Run::evaluate`).
//!
//! **The sections**, each found as the features before the loft leave
//! its sketch:
//!
//! - **A region**: found again in its sketch's profiles as an extrude's
//!   regions are ([`Profiles::resolve`]; gone: "section 2 not found"),
//!   made into a kernel profile ([`profile`]); one with holes fails
//!   ("section 2 has holes: only sections with one loop can be
//!   lofted"). Its start, a sketch point, is the first segment of the
//!   outline's piece whose start vertex is within the resolution of the
//!   point ([`loft_corner`], the rule the session picks starts by; an
//!   arc's split or a spline's fitted joint is no corner) (gone:
//!   "section 2's start point wasn't found"; on no corner: "section 2's
//!   start point isn't one of its corners"); with none, the kernel's
//!   default. Its frame is its sketch's placement.
//! - **A point**: the sketch point placed by its sketch's placement
//!   (gone: "section 1 not found").
//!
//! A section whose sketch isn't placed fails it ("section 2's sketch
//! isn't placed"). Two consecutive sections on one plane (the last and
//! the first too, for a closed loft) fail it before the kernel is asked
//! ("sections 1 and 2 are on one plane"; a point on the plane of the
//! section next to it, "section 3 is a point on section 2's plane: move
//! it off the plane"), by the kernel's own rule ([`on_one_plane`]).
//!
//! **The rails**: each a sketch's curves ordered into one open chain
//! ([`chain`], as a split's line: lines, arcs of at most 90° and
//! splines' fitted conics; "rail 1 not found", "rail 1 is closed ...",
//! "rail 1's curves don't join end to end into one line"), mapped into
//! the world by its sketch's placement.
//!
//! The tool is cached by every section's and rail's sketch key and
//! placement and what the feature names of them, the mode, whether it's
//! closed, the feature and the fit tolerance. The kernel's refusals are
//! worded for the Timeline (`message::loft_refused`: "rail 1 doesn't
//! pass through section 2", "the loft twists: pick matching start
//! points", "the loft runs into itself"); its other failures as
//! `message::lofting`'s.
//!
//! The kernel's loft isn't built yet: it fails as too complex, which
//! reaches the user as "lofting its sections is too complex to work
//! out", and the rest of the history goes on.
//!
//! [`Profiles::resolve`]: varde_sketch::Profiles::resolve
//! [`on_one_plane`]: varde_kernel::loft::on_one_plane
//! [`loft_corner`]: crate::loft_corner
//! [`profile`]: crate::profile()

use std::sync::Arc;

use varde_document::{Loft, LoftMode, Placement, Section};
use varde_kernel::loft::{self as kernel_loft, LoftError, Rail};
use varde_kernel::patch::Conic3;
use varde_kernel::{Budget, Frame, Solid, Tolerance};

use super::{Failed, Run, SketchOutput};
use crate::cache::{Cache, Key, Keyer};
use crate::error_geometry::KernelFailure;
use crate::message::{self, LoftRefusal};
use crate::profile::{chain, loft_corner, profile_marked};

/// The kernel's loft, which tests may replace with a stand-in to check
/// what regeneration does with the result before the kernel's is built.
type Lofter = fn(
    &[kernel_loft::Section],
    kernel_loft::LoftMode,
    bool,
    &[Rail],
    u64,
    &Tolerance,
    &Budget,
) -> Result<Solid, LoftError>;

#[cfg(any(test, feature = "testing"))]
thread_local! {
    /// The loft a test asks for in place of the kernel's, on its own
    /// thread.
    pub(crate) static LOFTER: std::cell::Cell<Option<Lofter>> =
        const { std::cell::Cell::new(None) };
}

/// The loft to run: the kernel's, or the one a test set.
fn lofter() -> Lofter {
    #[cfg(any(test, feature = "testing"))]
    if let Some(lofter) = LOFTER.get() {
        return lofter;
    }
    kernel_loft::loft
}

/// Lofts on this thread by [`by_extrude`] from now on.
#[cfg(any(test, feature = "testing"))]
pub(crate) fn loft_by_extrude() {
    LOFTER.set(Some(by_extrude));
}

/// A stand-in for the kernel's loft, for tests: two open sections, no
/// rails, each a loop of straight segments, the second the first moved
/// along its plane's normal (vertex for vertex within the resolution,
/// either way round) is the first extruded to the second, its faces
/// named as an extrude's. The second's start (its given one, or the
/// vertex nearest the first's start) must be the first's moved: another
/// is [`LoftError::Twists`], as the kernel's would twist. Anything else
/// is [`KernelError::TooComplex`].
///
/// [`KernelError::TooComplex`]: varde_kernel::KernelError::TooComplex
#[cfg(any(test, feature = "testing"))]
pub(crate) fn by_extrude(
    sections: &[kernel_loft::Section],
    _mode: kernel_loft::LoftMode,
    closed: bool,
    rails: &[Rail],
    feature: u64,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Solid, LoftError> {
    use glam::DVec3;
    use varde_kernel::{KernelError, Profile};
    let too_complex = || LoftError::Failed(KernelError::TooComplex.into());
    let resolution = tol.resolution();
    let [first, second] = sections else {
        return Err(too_complex());
    };
    let (
        kernel_loft::Section::Loop {
            outline,
            frame,
            start: a_start,
        },
        kernel_loft::Section::Loop { start: b_start, .. },
    ) = (first, second)
    else {
        return Err(too_complex());
    };
    if closed || !rails.is_empty() {
        return Err(too_complex());
    }
    let straight = |section: &kernel_loft::Section| match section {
        kernel_loft::Section::Loop { outline, .. } => outline.segments.iter().all(|segment| {
            let c = segment.conic;
            let chord = c.p1 - c.p0;
            (c.c - c.p0).perp_dot(chord).abs() <= resolution * chord.length()
        }),
        kernel_loft::Section::Point(_) => false,
    };
    let (a, b) = (first.vertices(), second.vertices());
    let n = a.len();
    if !straight(first) || !straight(second) || b.len() != n || n < 3 {
        return Err(too_complex());
    }
    let normal = frame.normal();
    let height = (b[0] - frame.origin).dot(normal);
    if (b.iter()).any(|&p| ((p - frame.origin).dot(normal) - height).abs() > resolution) {
        return Err(too_complex());
    }
    let shift = normal * height;
    let a_start = a_start.unwrap_or(0) % n;
    let nearest = |to: DVec3| {
        (0..n)
            .min_by(|&i, &j| {
                b[i].distance_squared(to)
                    .total_cmp(&b[j].distance_squared(to))
            })
            .unwrap_or(0)
    };
    let b_start = b_start.map_or_else(|| nearest(a[a_start]), |start| start % n);
    // Whether `b` from `at`, `step` (1 or n − 1) a vertex, is `a` from
    // its start moved.
    let matches = |at: usize, step: usize| {
        (0..n).all(|i| {
            let moved = a[(a_start + i) % n] + shift;
            b[(at + i * step) % n].distance(moved) <= resolution
        })
    };
    if !(matches(b_start, 1) || matches(b_start, n - 1)) {
        let elsewhere = (0..n).any(|at| matches(at, 1) || matches(at, n - 1));
        return Err(if elsewhere {
            LoftError::Twists
        } else {
            too_complex()
        });
    }
    let profile = Profile {
        loops: vec![outline.clone()],
    };
    let (from, to) = if height > 0.0 {
        (0.0, height)
    } else {
        (height, 0.0)
    };
    Ok(varde_kernel::extrude(
        &profile, frame, from, to, feature, tol, budget,
    )?)
}

impl Run<'_> {
    /// A loft's tool solid and the key it's filed under: its sections
    /// placed and lofted, as the module's docs say.
    pub(super) fn lofted(
        &self,
        loft: &Loft,
        cache: &mut Cache,
    ) -> Result<(Arc<Solid>, Key), Failed> {
        let mut keyer = Keyer::new("loft");
        keyer
            .number(self.feature.id.get())
            .number(self.tolerance.fit().to_bits())
            .value(&loft.mode)
            .number(u64::from(loft.closed));
        let mut placed: Vec<(&SketchOutput, Placement)> = Vec::with_capacity(loft.sections.len());
        for (index, section) in loft.sections.iter().enumerate() {
            let output = self
                .sketch_output(section.sketch())
                .ok_or_else(|| message::section_sketch_gone(index))?;
            let placement = output
                .placement
                .ok_or_else(|| message::section_not_placed(index))?;
            keyer.key(output.key).placement(&placement).value(section);
            placed.push((output, placement));
        }
        let mut rails_placed: Vec<(&SketchOutput, Placement)> =
            Vec::with_capacity(loft.rails.len());
        for (index, rail) in loft.rails.iter().enumerate() {
            let output = self
                .sketch_output(rail.sketch)
                .ok_or_else(|| message::rail_sketch_gone(index))?;
            let placement = output
                .placement
                .ok_or_else(|| message::rail_not_placed(index))?;
            keyer.key(output.key).placement(&placement).value(rail);
            rails_placed.push((output, placement));
        }
        let key = keyer.finish();
        let loft_by = lofter();
        let solid = cache.solid(key, || {
            let mut sections = Vec::with_capacity(placed.len());
            for (index, (section, &(output, placement))) in
                loft.sections.iter().zip(&placed).enumerate()
            {
                sections.push(self.section(index, section, output, placement)?);
            }
            let count = sections.len();
            let pairs = if loft.closed {
                count
            } else {
                count.saturating_sub(1)
            };
            let resolution = self.tolerance.resolution();
            for i in 0..pairs {
                let j = (i + 1) % count;
                if kernel_loft::on_one_plane(&sections[i], &sections[j], resolution) {
                    let point = |at: usize| matches!(sections[at], kernel_loft::Section::Point(_));
                    return Err(match (point(i), point(j)) {
                        (true, _) => message::point_on_section_plane(i, j),
                        (_, true) => message::point_on_section_plane(j, i),
                        _ => message::sections_on_one_plane(i, j),
                    }
                    .into());
                }
            }
            let mut rails = Vec::with_capacity(rails_placed.len());
            for (index, (rail, &(output, placement))) in
                loft.rails.iter().zip(&rails_placed).enumerate()
            {
                let segments = chain(
                    output.sketch,
                    &rail.curves,
                    resolution,
                    self.tolerance.fit(),
                )
                .map_err(|error| message::rail_chain(index, error))?;
                let world = |p| placement.to_world(p);
                let conics = (segments.iter())
                    .map(|segment| Conic3 {
                        p0: world(segment.conic.p0),
                        c: world(segment.conic.c),
                        w: segment.conic.w,
                        p1: world(segment.conic.p1),
                    })
                    .collect();
                rails.push(Rail { conics });
            }
            let mode = match loft.mode {
                LoftMode::Smooth => kernel_loft::LoftMode::Smooth,
                LoftMode::Ruled => kernel_loft::LoftMode::Ruled,
            };
            loft_by(
                &sections,
                mode,
                loft.closed,
                &rails,
                self.feature.id.get(),
                &self.tolerance,
                &Budget::DEFAULT,
            )
            .map_err(|error| self.loft_refused(error, count))
        })?;
        Ok((solid, key))
    }

    /// The sketch before the loft whose id is `sketch`.
    fn sketch_output(&self, sketch: varde_document::FeatureId) -> Option<&SketchOutput<'_>> {
        self.sketches.iter().find(|output| output.id == sketch)
    }

    /// Section `index` of the loft, `section`, as the kernel takes it,
    /// found in its sketch `output` placed at `placement`.
    fn section(
        &self,
        index: usize,
        section: &Section,
        output: &SketchOutput,
        placement: Placement,
    ) -> Result<kernel_loft::Section, Failed> {
        let sketch = output.sketch;
        match section {
            Section::Point { point, .. } => {
                let at = sketch
                    .point(*point)
                    .ok_or_else(|| message::section_not_found(index))?
                    .at;
                Ok(kernel_loft::Section::Point(placement.to_world(at)))
            }
            Section::Region { region, start, .. } => {
                let profiles = match &*output.profiles {
                    Ok(profiles) => profiles,
                    Err(e) => return Err(message::section_unusable(index, e).into()),
                };
                let found = profiles
                    .resolve(std::slice::from_ref(region))
                    .into_iter()
                    .collect::<Option<Vec<usize>>>()
                    .ok_or_else(|| message::section_not_found(index))?;
                let loops = profiles
                    .merge(&found)
                    .map_err(|e| message::section_unusable(index, e))?;
                let (made, firsts) = profile_marked(sketch, profiles, &loops, self.tolerance.fit())
                    .map_err(|e| message::section_unusable(index, e))?;
                let [outline] =
                    <[_; 1]>::try_from(made.loops).map_err(|_| message::section_holes(index))?;
                let start = match start {
                    None => None,
                    Some(id) => {
                        if sketch.point(*id).is_none() {
                            return Err(message::start_not_found(index).into());
                        }
                        // The piece starting at the point, by the rule
                        // the session picks starts by, and its first
                        // segment.
                        let resolution = self.tolerance.resolution();
                        let piece = loft_corner(sketch, profiles, &loops[0], *id, resolution)
                            .ok_or_else(|| message::start_not_corner(index))?;
                        Some(firsts[0][piece])
                    }
                };
                Ok(kernel_loft::Section::Loop {
                    outline,
                    frame: Frame {
                        origin: placement.origin,
                        x: placement.x,
                        y: placement.y,
                    },
                    start,
                })
            }
        }
    }

    /// Why the kernel's loft of `count` sections gave no solid, in words.
    fn loft_refused(&self, error: LoftError, count: usize) -> Failed {
        let index = |at: u32| usize::try_from(at).unwrap_or(usize::MAX);
        let why = match error {
            LoftError::OnePlane { section } => {
                let a = index(section);
                LoftRefusal::OnePlane(a, a.saturating_add(1) % count.max(1))
            }
            LoftError::RailMisses { rail, section } => LoftRefusal::RailMisses {
                rail: index(rail),
                section: index(section),
            },
            LoftError::Twists => LoftRefusal::Twists,
            LoftError::IntoItself => LoftRefusal::IntoItself,
            LoftError::Failed(failure) => {
                let words =
                    message::lofting(failure.error, self.tolerance.fit() <= Tolerance::MIN_FIT);
                let failure = KernelFailure::new(failure, &self.tolerance);
                return Failed::kernel(words, &failure, [&[], &[]]);
            }
        };
        message::loft_refused(why).into()
    }
}
