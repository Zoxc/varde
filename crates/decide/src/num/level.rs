//! The precision levels as one numeric type family, and the runner that
//! steps a computation through them.
//!
//! **One code path, every level.** A curved computation is written once,
//! generic over [`LevelScalar`]: a [`Bounded`] value at one of the
//! schedule's levels ([`Level`]), `f64` at [`Level::L0`], double-double
//! at [`Level::L1`], big-float at the widths above. Its operations are
//! the Bernstein forms' ([`Scalar`]: `+ − × ÷`, the midpoint, the
//! enclosure, the certified sign), the curved rows' ([`Curved`]: the
//! square roots, widening) and [`decide`]'s reading of a coefficient
//! ([`Coeff`]); the value knows its own level ([`LevelScalar::level`]).
//! The Bernstein forms themselves are generic over [`Scalar`] alone, which
//! exact rationals also implement, so the same code also runs exactly.
//!
//! **The runner.** A computation hands one step to the runner as an
//! [`AtLevel`]: [`at_level`] runs it at a given level, with a value of
//! that level's type to build its numbers from (`like`, zero, the width
//! for big-float); [`ladder`] runs it at each level from a given one in
//! turn, in the schedule's order, until a level's answer doesn't ask for
//! the next (the precision levels of one decision, not the rungs of the
//! robustness ladder, whose targets [`decide`]'s bands carry). The type
//! a level computes in is chosen here and nowhere else.
//!
//! **Promotion is exact.** A step at a higher level builds its numbers
//! from the same exact inputs (an `f64` is exact at every level), so it
//! never starts from a worse value than the one below; a value carried
//! up keeps its value and error parts (`promote` from `f64` to
//! double-double and on to the first width, [`Bounded::widen`] between
//! widths).
//!
//! [`decide`]: super::decide::decide
//! [`Scalar`]: crate::bezier::Scalar

use super::bigf::{Big, BigF, Width};
use super::bounded::Bounded;
use super::dd::Dd;
use super::decide::Coeff;
use super::schedule::{LEVELS, Level};
use crate::perturb::Curved;

/// A value at one of the schedule's precision levels (module docs): a
/// [`Bounded`] `f64`, double-double or big-float, with the operations
/// every curved computation is written in.
pub trait LevelScalar: Curved + Coeff {
    /// The level the value is at.
    fn level(&self) -> Level;

    /// The value rounded to an `f64`: an estimate inside the error, not a
    /// bound ([`enclose`](crate::bezier::Scalar::enclose) bounds it).
    fn approx(&self) -> f64;
}

impl LevelScalar for Bounded<f64> {
    #[inline]
    fn level(&self) -> Level {
        Level::L0
    }

    #[inline]
    fn approx(&self) -> f64 {
        self.v
    }
}

impl LevelScalar for Bounded<Dd> {
    #[inline]
    fn level(&self) -> Level {
        Level::L1
    }

    #[inline]
    fn approx(&self) -> f64 {
        self.v.hi()
    }
}

impl<B: BigF> LevelScalar for Bounded<B> {
    #[inline]
    fn level(&self) -> Level {
        Level::of_width(self.v.width())
    }

    #[inline]
    fn approx(&self) -> f64 {
        self.v.to_f64()
    }
}

/// One step of a computation, run at a precision level by [`at_level`]
/// or [`ladder`]; `W` the work it records at that level.
pub trait AtLevel<W> {
    /// The step's answer.
    type Out;

    /// The step at the level of `like` (a value of that level's type,
    /// [`LevelScalar::level`]), recording its work in `work`.
    fn at<S: LevelScalar>(&mut self, like: &S, work: &mut W) -> Self::Out;

    /// Whether `out` asks for the next level.
    fn escalates(out: &Self::Out) -> bool;
}

/// Runs `f` at `level`, in that level's type.
pub fn at_level<W, F: AtLevel<W>>(f: &mut F, level: Level, work: &mut W) -> F::Out {
    match level.width() {
        None if level == Level::L0 => f.at(&Bounded::exact(0.0), work),
        None => f.at(&Bounded::exact(Dd::ZERO), work),
        Some(w) => f.at(&big_zero(w), work),
    }
}

/// Runs `f` at each level from `from` up in turn until one doesn't
/// escalate ([`AtLevel::escalates`]), each recording its work in its own
/// slot of `work`; at the top level its answer stands, whatever it is.
pub fn ladder<W, F: AtLevel<W>>(f: &mut F, from: Level, work: &mut [W; LEVELS]) -> F::Out {
    let mut level = from;
    loop {
        let out = at_level(f, level, &mut work[level.index()]);
        match level.next() {
            Some(next) if F::escalates(&out) => level = next,
            _ => return out,
        }
    }
}

/// Zero at width `w`, exactly.
fn big_zero(w: Width) -> Bounded<Big> {
    Bounded::exact(Big::from_f64(0.0, w))
}

#[cfg(test)]
mod tests;
