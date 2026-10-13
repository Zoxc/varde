//! Numerics: bounded evaluation, exact predicates ([`exact`], their
//! expansion arithmetic in [`expansion`]), the precision levels (double-double in
//! [`dd`], big-float in [`bigf`]), the sign decision of every record
//! ([`decide`]) and the `libm` wrapper ([`trig`]).

pub mod bigf;
pub mod bounded;
pub mod dd;
pub mod decide;
pub mod exact;
pub mod expansion;
pub mod trig;
