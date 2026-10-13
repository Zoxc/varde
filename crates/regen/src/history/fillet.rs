//! Evaluating a fillet: the body's solid, as the features before it
//! leave it, with the fillet's edges rounded off by the kernel's
//! [`varde_kernel_compat::fillet()`].
//!
//! The edges are found, grown along tangent chains and named as a
//! chamfer's are (`super::blend`; a fillet, of one radius, has no use
//! for the chains' first faces).
//!
//! The result is cached by the body's key, the chains, the radius's
//! bits, the feature and the fit tolerance, and replaces the body's
//! solid (the body keeps its id). The kernel's refusals are worded for
//! the Timeline (`why::blend_refused`), with the edge they're about
//! drawn; its other failures as a boolean's ("filleting Body 1 ...").
//!
//! The kernel's fillet isn't built yet: it fails as too complex, which
//! reaches the user as "filleting Body 1 is too complex to work out",
//! and the rest of the history goes on.

use varde_document::Fillet;
use varde_kernel_compat::{BlendError, Budget, FilletChain, Solid, Tolerance, Topology};

use super::blend::prepare;
use super::{Evaluation, Failed, Step};
use crate::cache::Cache;
use crate::why::{self, Blend};

/// The kernel's fillet, which tests may replace with a stand-in to check
/// what regeneration does with the result before the kernel's is built.
pub(crate) type Filleter = fn(
    &Solid,
    &Topology,
    &[FilletChain],
    f64,
    u64,
    &Tolerance,
    &Budget,
) -> Result<Solid, BlendError>;

kernel_op! {
    /// The fillet to run: the kernel's, or the one a test set.
    fn filleter() -> Filleter = varde_kernel_compat::fillet, fillet;
}

/// Changes the body of `evaluation` as the fillet `fillet`, the feature
/// `feature`, says, or says why it fails, changing nothing.
///
/// Its body must have a solid of its own (a join or a combine consumed
/// fails it, naming the body holding it). Then the edges, the chains and
/// the kernel's fillet, as the module's docs say.
pub(super) fn evaluate_fillet(
    step: &Step,
    fillet: &Fillet,
    evaluation: &mut Evaluation,
    cache: &mut Cache,
) -> Result<(), Failed> {
    let Step {
        document,
        feature,
        tolerance,
        ..
    } = *step;
    let prepared = prepare(
        document,
        fillet.body(),
        (&fillet.edges, &fillet.faces),
        fillet.chains,
        false,
        evaluation,
        cache,
    )?;
    let (place, planned) = (&prepared.place, &prepared.planned);
    let (solid, topology) = (&*place.solid, &*place.topology);
    let chains: Vec<FilletChain> = (planned.iter())
        .map(|planned| FilletChain {
            chain: planned.chain,
            name: planned.name,
        })
        .collect();
    let radius = fillet.radius.value;
    let mut keyer = place.keyer("fillet", feature, tolerance);
    keyer.number(radius.to_bits()).number(chains.len() as u64);
    for chain in &chains {
        keyer.number(u64::from(chain.chain)).number(chain.name);
    }
    let key = keyer.finish();
    let fillet_by = filleter();
    place.replace(
        key,
        evaluation,
        cache,
        |name| why::blend_leaves_nothing(Blend::Fillet, name),
        |budget| {
            fillet_by(
                solid,
                topology,
                &chains,
                radius,
                feature.get(),
                tolerance,
                budget,
            )
            .map_err(|error| prepared.refused(Blend::Fillet, error, tolerance))
        },
    )
}
