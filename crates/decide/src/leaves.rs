//! Leaves: pairs of dyadic boxes, one in each of two faces' parameter
//! domains, each accepted only with a certificate of one kind (Empty,
//! Simple, Near, Contact), else split at ½. So far [`flat`], the
//! certificates on polynomial patches decided exactly, whose final
//! answers are those of flat pieces.

pub mod flat;
