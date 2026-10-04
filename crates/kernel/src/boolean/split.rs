//! Splitting a solid in two by a tool, and the tools a split cuts with.
//!
//! **Not built yet**: every entry point here is a stand-in with the
//! planned signature that fails with [`KernelError::TooComplex`], so the
//! split feature above the kernel (its document, regeneration and
//! messages) is built against it. The real implementation replaces this
//! file whole: `split` as `body ∩ tool` and `body − tool` from one
//! arrangement; `half_space`, the box on one side of a plane past a
//! body's box; `surface_tool`, the closed solid a face's surface bounds,
//! continued past the body; `chain_tool`, an open chain of sketch curves
//! closed round the body's shadow and extruded through it.

use glam::DVec3;

use crate::mesh::Form;
use crate::patch::Bounds3;
use crate::profile::Segment;
use crate::{Budget, Failure, Frame, KernelError, Solid, Tolerance};

/// `body` split by `tool`: its part inside the tool (the front) and the
/// rest (the back). Not built yet: always [`KernelError::TooComplex`].
pub fn split(
    _body: &Solid,
    _tool: &Solid,
    _tol: &Tolerance,
    _budget: &Budget,
) -> Result<(Solid, Solid), Failure> {
    Err(KernelError::TooComplex.into())
}

/// Why [`surface_tool`] gives no tool.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolError {
    /// The form has no surface the kernel continues past the face.
    CantExtend,
    /// Building the tool failed.
    Failed(Failure),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolError::CantExtend => f.write_str("the face can't be extended to split with"),
            ToolError::Failed(failure) => write!(f, "{}", failure.error),
        }
    }
}

impl std::error::Error for ToolError {}

/// The half-space on the side of the plane `n·x = d` that `n` points
/// to, as far as a body in `bounds` reaches, its face on the plane
/// `Split(0)` of `feature`. Not built yet: always
/// [`KernelError::TooComplex`].
pub fn half_space(
    _n: DVec3,
    _d: f64,
    _bounds: &Bounds3,
    _feature: u64,
    _tol: &Tolerance,
    _budget: &Budget,
) -> Result<Solid, Failure> {
    Err(KernelError::TooComplex.into())
}

/// The closed solid bounded by the surface of `form` (the form of a face
/// through `on`), continued past a body in `bounds`, its faces on the
/// surface `Split(0)` of `feature`. Not built yet: always
/// [`ToolError::Failed`] with [`KernelError::TooComplex`].
pub fn surface_tool(
    _form: &Form,
    _on: DVec3,
    _bounds: &Bounds3,
    _feature: u64,
    _tol: &Tolerance,
    _budget: &Budget,
) -> Result<Solid, ToolError> {
    Err(ToolError::Failed(KernelError::TooComplex.into()))
}

/// The tool an open chain of sketch curves on `frame` splits a body in
/// `bounds` with: the chain continued along its end tangents to a
/// rectangle round the body's shadow (its sides named as profile curve
/// `rim`), closed on the chain's left and extruded through the body both
/// ways, named for `feature`. Not built yet: always
/// [`KernelError::TooComplex`].
pub fn chain_tool(
    _chain: &[Segment],
    _frame: &Frame,
    _bounds: &Bounds3,
    _rim: u64,
    _feature: u64,
    _tol: &Tolerance,
    _budget: &Budget,
) -> Result<Solid, Failure> {
    Err(KernelError::TooComplex.into())
}
