//! The snap: where two operands' surfaces run within the gap bound `b_K`
//! of each other (leaves certified Near or Contact), one side is
//! rewritten onto the other in one certified decision that every record
//! then reads. So far [`cluster`]: the clusters of such leaves, merged
//! where their windows share an element, and their geometric order; and
//! [`target`]: the operand each cluster is rewritten onto and the map
//! carrying the other sheet there.

pub mod cluster;
pub mod target;
