//! The snap: where two operands' surfaces run within the gap bound `b_K`
//! of each other (leaves certified Near or Contact), one side is
//! rewritten onto the other in one certified decision that every record
//! then reads. So far [`cluster`]: the clusters of such leaves, merged
//! where their windows share an element, and their geometric order;
//! [`target`]: the operand each cluster is rewritten onto and the map
//! carrying the other sheet there; [`rim`]: where a cluster ends on
//! the snapped side, the gap's sign along it, its branch vertices and
//! its window; [`corner`]: where the vertices at the rim and in its
//! window land, the root clusters the target's vertices carry, decided
//! once by the corner rule; and [`rewrite`]: the snapped side's faces
//! split along the rim, the inner pieces onto the target, with canonical
//! keys from the operands' ids and the map back to the operands; and
//! [`along`]: per pair of a snapped and a target edge, the brackets where
//! the snapped edge runs along the target edge and reuses it, and its
//! ordinary crossings of it.

pub mod along;
pub mod cluster;
pub mod corner;
pub mod rewrite;
pub mod rim;
pub mod target;
