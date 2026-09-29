//! The page's side of the web build, see the parent module: never in the
//! IO worker, which has no `window`.

pub(crate) mod lane;
pub(crate) mod pick;
