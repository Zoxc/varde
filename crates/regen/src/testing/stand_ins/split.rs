//! The stand-in for the kernel's split that tests run in its place
//! until the kernel's is built (see [`crate::testing`]).

use varde_kernel_compat::{Budget, Failure, Solid, Tolerance};

/// The kernel's split as two booleans, the front `body ∩ tool` and the
/// back `body − tool`: what regeneration does with the pieces is tested
/// with it until the kernel's is built.
pub(crate) fn by_booleans(
    body: &Solid,
    tool: &Solid,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<(Solid, Solid), Failure> {
    use varde_kernel_compat::Op;
    let front = varde_kernel_compat::boolean(body, tool, Op::Intersection, tol, budget)?;
    let back = varde_kernel_compat::boolean(body, tool, Op::Difference, tol, budget)?;
    Ok((front, back))
}
