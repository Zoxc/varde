//! Records: every fact the counting reads, decided once per canonical key
//! and read by every consumer, which reads the stored answer from its own
//! side where it asks the other way and never decides it again. [`keys`] holds the keys
//! (which entities, of which operand, by id), [`table`] the sorted store
//! a stage's charged passes fill, [`shadow`] the flat shadow records (a
//! point against a vertex of the other operand, a point's ray against a
//! straight edge).

pub mod keys;
pub mod shadow;
pub mod table;
