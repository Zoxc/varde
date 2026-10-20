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
//! keys from the operands' ids and the map back to the operands, then
//! where its vertices land and its edges' curves, reusing the target's
//! edges bit for bit where they run along them; and
//! [`along`]: per pair of a snapped and a target edge, the brackets where
//! the snapped edge runs along the target edge and reuses it, and its
//! ordinary crossings of it; and [`join`]: once every cluster is snapped,
//! the edges outside the clusters at the vertices they moved refit once,
//! split where the refit meets the untouched rest; and [`stale`]: which
//! elements of a rewrite lie on the target, and the order the snaps run
//! in, the leaves each snap and the join pass leave stale recertified,
//! merged into an earlier cluster where they join it, which then re-runs;
//! and [`escalate`]: where a cluster's snap fails, the one routing of the
//! failure, halving the gap bound only where it depends on it, and the
//! rule gaps where halving stops helping; and [`decide`]: once snapped,
//! the zone's one-surface decision, the rim's records by the gap's sign
//! with the corner records at its ties, the elements on the target read
//! at their places there, and the band of records between two of them.

pub mod along;
pub mod cluster;
pub mod corner;
pub mod decide;
pub mod escalate;
pub mod join;
pub mod rewrite;
pub mod rim;
pub mod stale;
pub mod target;
