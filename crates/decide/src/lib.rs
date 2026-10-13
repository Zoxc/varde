//! The Varde kernel's decision layer: numerics, Bernstein forms, records,
//! counting and the geometric order, generic over the patch type. So far
//! [`num::bounded`], values with certified error bounds, [`num::dd`],
//! double-double with them, [`num::bigf`], big-float with them,
//! [`num::expansion`], the exact floating-point expansions,
//! [`num::exact`], the exact flat predicates over them,
//! [`num::decide`], every record's sign with ties broken by symbolic
//! perturbation,
//! [`num::trig`], the deterministic trigonometry, [`bezier`], the
//! Bernstein forms over those numbers, in one and two parameters, and
//! [`perturb`], the perturbation rows `decide` reads for each kind of
//! record, and the directions the moving operand's elements move along.

pub mod bezier;
pub mod num;
pub mod perturb;
