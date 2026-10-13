//! The bodies a feature names: a combine's tools, a move's, a mirror's,
//! a pattern's or a scale's bodies, and the checks they share.

use crate::BodyId;

/// The most bodies one feature may name (a combine's tools, a move's,
/// a mirror's, a pattern's or a scale's bodies). Each costs a boolean, so
/// this bounds the work, and what a file can ask for.
pub const MAX_FEATURE_BODIES: usize = 256;

/// Checks a feature's body list: `1..=`[`MAX_FEATURE_BODIES`], sorted
/// without repeats.
pub(crate) fn check_bodies(bodies: &[BodyId]) -> Result<(), BodiesError> {
    let count = bodies.len();
    if !(1..=MAX_FEATURE_BODIES).contains(&count) {
        return Err(BodiesError::Count(count));
    }
    if !bodies.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(BodiesError::Order);
    }
    Ok(())
}

/// What's wrong with a feature's body list, see `check_bodies`.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum BodiesError {
    /// It names this many bodies: none, or over [`MAX_FEATURE_BODIES`].
    #[error("names {0} bodies, not 1 to {MAX_FEATURE_BODIES}")]
    Count(usize),
    /// Its bodies aren't sorted, or one is repeated.
    #[error("its bodies are out of order or repeated")]
    Order,
}
