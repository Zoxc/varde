//! Evaluating a loft's tool: its sections placed in the world in order
//! and lofted by the kernel's [`varde_kernel_compat::loft::loft`]. What's done
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
//! worded for the Timeline (`why::loft_refused`: "rail 1 doesn't
//! pass through section 2", "the loft twists: pick matching start
//! points", "the loft runs into itself"); its other failures as
//! `why::lofting`'s.
//!
//! The kernel's loft isn't built yet: it fails as too complex, which
//! reaches the user as "lofting its sections is too complex to work
//! out", and the rest of the history goes on.
//!
//! [`Profiles::resolve`]: varde_sketch::Profiles::resolve
//! [`on_one_plane`]: varde_kernel_compat::loft::on_one_plane
//! [`loft_corner`]: crate::loft_corner
//! [`profile`]: crate::profile()

use std::sync::Arc;

use varde_document::{Loft, LoftMode, Placement, Section};
use varde_kernel_compat::loft::{self as kernel_loft, LoftError, Rail};
use varde_kernel_compat::{Budget, Solid, Tolerance};

use super::{Failed, Run, SketchOutput};
use crate::cache::{Cache, Key, Keyer};
use crate::profile::{chain, loft_corner, profile_marked};
use crate::why::{self, LoftRefusal};

/// The kernel's loft, which tests may replace with a stand-in to check
/// what regeneration does with the result before the kernel's is built.
pub(crate) type Lofter = fn(
    &[kernel_loft::Section],
    kernel_loft::LoftMode,
    bool,
    &[Rail],
    u64,
    &Tolerance,
    &Budget,
) -> Result<Solid, LoftError>;

kernel_op! {
    /// The loft to run: the kernel's, or the one a test set.
    fn lofter() -> Lofter = kernel_loft::loft, loft;
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
            let (output, placement) = super::placed_sketch(
                self.sketches,
                section.sketch(),
                || why::section_sketch_gone(index),
                || why::section_not_placed(index),
            )?;
            keyer.key(output.key).placement(&placement).value(section);
            placed.push((output, placement));
        }
        let mut rails_placed: Vec<(&SketchOutput, Placement)> =
            Vec::with_capacity(loft.rails.len());
        for (index, rail) in loft.rails.iter().enumerate() {
            let (output, placement) = super::placed_sketch(
                self.sketches,
                rail.sketch,
                || why::rail_sketch_gone(index),
                || why::rail_not_placed(index),
            )?;
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
                        (true, _) => why::point_on_section_plane(i, j),
                        (_, true) => why::point_on_section_plane(j, i),
                        _ => why::sections_on_one_plane(i, j),
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
                .map_err(|error| why::chain(why::ChainOf::Rail(index), error))?;
                let conics = (segments.iter())
                    .map(|segment| super::world_conic(&placement, segment))
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
                    .ok_or_else(|| why::section_not_found(index))?
                    .at;
                Ok(kernel_loft::Section::Point(placement.to_world(at)))
            }
            Section::Region { region, start, .. } => {
                let profiles = match &*output.profiles {
                    Ok(profiles) => profiles,
                    Err(e) => return Err(why::section_unusable(index, e).into()),
                };
                let found = profiles
                    .resolve(std::slice::from_ref(region))
                    .into_iter()
                    .collect::<Option<Vec<usize>>>()
                    .ok_or_else(|| why::section_not_found(index))?;
                let loops = profiles
                    .merge(&found)
                    .map_err(|e| why::section_unusable(index, e))?;
                let (made, firsts) = profile_marked(sketch, profiles, &loops, self.tolerance.fit())
                    .map_err(|e| why::section_unusable(index, e))?;
                let [outline] =
                    <[_; 1]>::try_from(made.loops).map_err(|_| why::section_holes(index))?;
                let start = match start {
                    None => None,
                    Some(id) => {
                        if sketch.point(*id).is_none() {
                            return Err(why::start_not_found(index).into());
                        }
                        // The piece starting at the point, by the rule
                        // the session picks starts by, and its first
                        // segment.
                        let resolution = self.tolerance.resolution();
                        let piece = loft_corner(sketch, profiles, &loops[0], *id, resolution)
                            .ok_or_else(|| why::start_not_corner(index))?;
                        Some(firsts[0][piece])
                    }
                };
                Ok(kernel_loft::Section::Loop {
                    outline,
                    frame: super::frame(&placement),
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
                let words = why::lofting(failure.error, self.tolerance.fit() <= Tolerance::MIN_FIT);
                return Failed::by_value(words, failure, &self.tolerance);
            }
        };
        why::loft_refused(why).into()
    }
}
