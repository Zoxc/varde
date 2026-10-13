//! Numerics: bounded evaluation, exact predicates (their expansion
//! arithmetic in [`expansion`]), the precision levels (double-double in
//! [`dd`], big-float in [`bigf`]) and the `libm` wrapper ([`trig`]).

pub mod bigf;
pub mod bounded;
pub mod dd;
pub mod expansion;
pub mod trig;
