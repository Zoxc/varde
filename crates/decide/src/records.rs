//! Records: every fact the counting reads, decided once per canonical key
//! and read by every consumer, which reads the stored answer from its own
//! side where it asks the other way and never decides it again. [`keys`] holds the keys
//! (which entities, of which operand, by id), [`table`] the sorted store
//! a stage's charged passes fill, [`shadow`] the flat shadow records (a
//! point against a vertex of the other operand, a point's ray against a
//! straight edge), [`ray`] the curved ones (a point's ray against a span
//! of a curved edge, and against the whole edge), [`feet`] the stretches
//! of an edge near a point, [`cusp`] the certificate of a sharp corner
//! (how its two curves come apart, where its spike is cut), [`side`] a point's ray against a box side on a
//! tile line the counting can't certify off the point, [`height`] the
//! height keys a point reads against a vertex, an edge or a face's layer,
//! the layers themselves and the height records (and, in
//! [`height::model`], the geometry they are decided from), [`near`] the
//! operation's bands and near radii they read, and [`xrec`] the count of
//! an edge pair's shadow crossings from four ray records and the degrees
//! of the boxes of their parameter square.

pub mod cusp;
pub mod feet;
pub mod height;
pub mod keys;
pub mod near;
pub mod ray;
pub mod shadow;
pub mod side;
pub mod table;
pub mod xrec;
