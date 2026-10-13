//! The Varde kernel's decision layer: numerics, Bernstein forms, records,
//! counting and the geometric order, generic over the patch type. So far
//! [`num::bounded`], values with certified error bounds, [`num::dd`],
//! double-double with them, [`num::bigf`], big-float with them,
//! [`num::expansion`], the exact floating-point expansions,
//! [`num::exact`], the exact flat predicates over them,
//! [`num::trig`], the deterministic trigonometry, and [`bezier`], the
//! Bernstein forms over those numbers, in one and two parameters.

pub mod bezier;
pub mod num;
