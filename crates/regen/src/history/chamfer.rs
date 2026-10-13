//! Evaluating a chamfer: the body's solid, as the features before it
//! leave it, with the chamfer's edges cut off by the kernel's
//! [`varde_kernel_compat::chamfer`].
//!
//! The edges are found, grown along tangent chains, given their first
//! faces and named as a fillet's are (`super::blend`).
//!
//! The result is cached by the body's key, the chains and how each is
//! cut, the feature and the fit tolerance, and replaces the body's solid
//! (the body keeps its id). The kernel's refusals are worded for the
//! Timeline (`why::blend_refused`), with the edge they're about
//! drawn; its other failures as a boolean's ("chamfering Body 1 ...").
//!
//! The kernel's chamfer isn't built yet: it fails as too complex, which
//! reaches the user as "chamfering Body 1 is too complex to work out",
//! and the rest of the history goes on.

use varde_document::{Chamfer, ChamferSize, Document, FeatureId};
use varde_kernel_compat::{
    BlendError, Budget, ChamferChain, ChamferCut, Solid, Tolerance, Topology,
};

use super::blend::prepare;
use super::{Evaluation, Failed};
use crate::cache::Cache;
use crate::why::{self, Blend};

/// The kernel's chamfer, which tests may replace with a stand-in to
/// check what regeneration does with the result before the kernel's is
/// built.
pub(crate) type Chamferer =
    fn(&Solid, &Topology, &[ChamferChain], u64, &Tolerance, &Budget) -> Result<Solid, BlendError>;

kernel_op! {
    /// The chamfer to run: the kernel's, or the one a test set.
    fn chamferer() -> Chamferer = varde_kernel_compat::chamfer, chamfer;
}

/// Changes the body of `evaluation` as the chamfer `chamfer`, the
/// feature `feature`, says, or says why it fails, changing nothing.
///
/// Its body must have a solid of its own (`own_solids`: one a join or
/// a combine consumed fails it, naming the body holding it). Then the
/// edges, the chains and the kernel's chamfer, as the module's docs say.
pub(super) fn evaluate_chamfer(
    document: &Document,
    feature: FeatureId,
    chamfer: &Chamfer,
    tolerance: &Tolerance,
    evaluation: &mut Evaluation,
    cache: &mut Cache,
) -> Result<(), Failed> {
    let prepared = prepare(
        document,
        chamfer.body(),
        (&chamfer.edges, &chamfer.faces),
        chamfer.chains,
        chamfer.flip,
        evaluation,
        cache,
    )?;
    let (place, planned) = (&prepared.place, &prepared.planned);
    let (solid, topology) = (&*place.solid, &*place.topology);
    let chains: Vec<ChamferChain> = (planned.iter())
        .map(|planned| ChamferChain {
            chain: planned.chain,
            name: planned.name,
            cut: cut(
                &chamfer.distances,
                topology.chains()[planned.chain as usize].regions,
                planned.first,
            ),
        })
        .collect();
    let mut keyer = place.keyer("chamfer", feature, tolerance);
    for chain in &chains {
        keyer.number(u64::from(chain.chain)).number(chain.name);
        match chain.cut {
            ChamferCut::Distances([a, b]) => {
                keyer.number(0).number(a.to_bits()).number(b.to_bits())
            }
            ChamferCut::Angle {
                on,
                distance,
                angle,
            } => keyer
                .number(1)
                .number(on as u64)
                .number(distance.to_bits())
                .number(angle.to_bits()),
        };
    }
    let key = keyer.finish();
    let chamfer_by = chamferer();
    place.replace(
        key,
        evaluation,
        cache,
        |name| why::blend_leaves_nothing(Blend::Chamfer, name),
        |budget| {
            chamfer_by(solid, topology, &chains, feature.get(), tolerance, budget)
                .map_err(|error| prepared.refused(Blend::Chamfer, error, tolerance))
        },
    )
}

/// How a chain with regions `sides` and first face `first` is cut by
/// `size`, in the kernel's terms (its distances in the regions' order).
fn cut(size: &ChamferSize, sides: [u32; 2], first: u32) -> ChamferCut {
    let on = usize::from(sides[0] != first);
    match size {
        ChamferSize::Equal(d) => ChamferCut::Distances([d.value; 2]),
        ChamferSize::Two(a, b) => {
            let mut distances = [a.value, b.value];
            if on == 1 {
                distances.reverse();
            }
            ChamferCut::Distances(distances)
        }
        ChamferSize::Angle(d, a) => ChamferCut::Angle {
            on,
            distance: d.value,
            angle: a.value,
        },
    }
}
