//! The snap: where two operands' surfaces run within the gap bound `b_K`
//! of each other (leaves certified Near or Contact), one side is
//! rewritten onto the other in one certified decision that every record
//! then reads. So far [`cluster`]: the clusters of such leaves, merged
//! where their windows share an element, and their geometric order;
//! [`target`]: the operand each cluster is rewritten onto and the map
//! carrying the other sheet there; and [`rim`]: where a cluster ends on
//! the snapped side, the gap's sign along it, its branch vertices and
//! its window.

pub mod cluster;
pub mod rim;
pub mod target;
