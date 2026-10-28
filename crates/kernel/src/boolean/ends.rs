//! End records: where an edge of one operand passes through a face of
//! the other. So far [`count`]: the crossing count of an edge through
//! each piece of a face's partition, from the piece's share of the
//! column hits at the edge's ends and of the edge pairs' crossing
//! records, and the edge's roots on a face given by an implicit
//! equation, each with its degree, which realise a core piece's count.

pub mod count;
