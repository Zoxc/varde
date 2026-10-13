//! The stand-in for the kernel's chamfer that tests run in its place
//! until the kernel's is built (see [`crate::testing`]).

use glam::DVec3;
use varde_kernel_compat::{
    BlendError, Budget, ChamferChain, ChamferCut, Frame, KernelError, Loop, Op, Profile, Segment,
    Solid, Tolerance, Topology,
};

/// A stand-in for the kernel's chamfer, for tests: each chain must be a
/// straight, open, convex edge between two planar faces, and is cut off
/// by a triangular prism (the corner between the edge and the two rails,
/// widened outwards) running past its ends, one boolean each. Right
/// where the faces at the chain's ends are square to it (a block's
/// edges); anything else is [`KernelError::TooComplex`], a rail past
/// its face is [`BlendError::TooBig`] only as far as the stand-in can
/// tell (an angled cut that never meets the other face).
///
/// [`KernelError::TooComplex`]: varde_kernel_compat::KernelError::TooComplex
pub(crate) fn by_wedges(
    solid: &Solid,
    topology: &Topology,
    chains: &[ChamferChain],
    feature: u64,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Solid, BlendError> {
    let too_complex = || BlendError::Failed(KernelError::TooComplex.into());
    let mut result = solid.clone();
    for chamfered in chains {
        let edge = super::convex_edge(solid, topology, chamfered.chain).ok_or_else(too_complex)?;
        let super::ConvexEdge {
            a, t, length, into, ..
        } = edge;
        let [d0, d1] = match chamfered.cut {
            ChamferCut::Distances(distances) => distances,
            ChamferCut::Angle {
                on,
                distance,
                angle,
            } => {
                // The cut from `distance` along face `on`, at `angle` to it
                // inside the material, meets the other face this far
                // along: in the section's frame of `into[on]` and the
                // unit `m` square to it towards the other face.
                let other = into[1 - on];
                let c1 = other.dot(into[on]);
                let m = (other - into[on] * c1).normalize();
                let c2 = other.dot(m);
                let (sin, cos) = varde_kernel_compat::trig::sin_cos(angle);
                let k = distance * sin / (c1 * sin + c2 * cos);
                if !(k.is_finite() && k > 0.0) {
                    return Err(BlendError::TooBig {
                        chain: chamfered.chain,
                    });
                }
                if on == 0 {
                    [distance, k]
                } else {
                    [k, distance]
                }
            }
        };
        let rails = [a + into[0] * d0, a + into[1] * d1];
        let apex = a - (into[0] + into[1]) * (d0 + d1);
        let x = into[0];
        let y = t.cross(x);
        let frame = Frame { origin: a, x, y };
        let flat = |p: DVec3| glam::DVec2::new((p - a).dot(x), (p - a).dot(y));
        let mut corners = [flat(rails[0]), flat(rails[1]), flat(apex)];
        let [p, q, r] = corners;
        if (q - p).perp_dot(r - p) < 0.0 {
            corners.swap(0, 1);
        }
        let segments = (0..3)
            .map(|i| Segment::line(corners[i], corners[(i + 1) % 3], i as u64))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| too_complex())?;
        let profile = Profile {
            loops: vec![Loop { segments }],
        };
        let margin = d0 + d1 + 1.0;
        let prism = varde_kernel_compat::extrude(
            &profile,
            &frame,
            -margin,
            length + margin,
            feature,
            tol,
            budget,
        )?;
        result = varde_kernel_compat::boolean(&result, &prism, Op::Difference, tol, budget)?;
    }
    Ok(result)
}
