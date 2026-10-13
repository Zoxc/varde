//! Records: every fact the counting reads, decided once per canonical key
//! and read by every consumer, which reads the stored answer from its own
//! side where it asks the other way and never decides it again. [`keys`] holds the keys
//! (which entities, of which operand, by id), [`table`] the sorted store
//! a stage's charged passes fill.

pub mod keys;
pub mod table;
