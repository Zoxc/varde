//! How much the tests run, read once from the environment at runtime (so
//! switching never rebuilds anything). A dev-dependency of the crates whose
//! tests have long runs; it depends on nothing.
//!
//! - `VARDE_TESTS` unset or `quick`: the short default.
//! - `VARDE_TESTS=full`: the long runs (all fuzz seeds, the slow bounds
//!   checks).
//! - Anything else panics, so a typo can't quietly run the quick suite.
//!
//! `VARDE_TEST_SEED=<n>` replays the single seed `n` in every fuzz test
//! that takes its seeds from [`seeds`], whatever the mode.

use std::ops::Range;
use std::sync::OnceLock;

/// The suite's mode, from `VARDE_TESTS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Quick,
    Full,
}

/// The mode, parsed once. Panics on a value that isn't `quick` or `full`.
pub fn mode() -> Mode {
    static MODE: OnceLock<Mode> = OnceLock::new();
    *MODE.get_or_init(|| parse_mode(std::env::var("VARDE_TESTS").ok().as_deref()))
}

/// Whether the long runs are on.
pub fn full() -> bool {
    mode() == Mode::Full
}

/// `quick` in quick mode, `full` in full mode.
pub fn pick<T>(quick: T, full: T) -> T {
    match mode() {
        Mode::Quick => quick,
        Mode::Full => full,
    }
}

/// The seeds a fuzz test runs: `0..quick` or `0..full` by mode (quick
/// being a prefix of full), or just the one in `VARDE_TEST_SEED` when it
/// is set.
pub fn seeds(quick: u64, full: u64) -> Range<u64> {
    match replay_seed() {
        Some(seed) => seed..seed.checked_add(1).expect("VARDE_TEST_SEED is too large"),
        None => 0..pick(quick, full),
    }
}

/// The seed in `VARDE_TEST_SEED`, if set. Panics if it isn't a number
/// below `u64::MAX`.
pub fn replay_seed() -> Option<u64> {
    static SEED: OnceLock<Option<u64>> = OnceLock::new();
    *SEED.get_or_init(|| parse_seed(std::env::var("VARDE_TEST_SEED").ok().as_deref()))
}

fn parse_mode(value: Option<&str>) -> Mode {
    match value {
        None | Some("") | Some("quick") => Mode::Quick,
        Some("full") => Mode::Full,
        Some(other) => panic!("VARDE_TESTS={other:?}: expected `quick` or `full`"),
    }
}

fn parse_seed(value: Option<&str>) -> Option<u64> {
    let value = value?.trim();
    match value.parse::<u64>() {
        // One below the top, so `seed + 1` bounds a range.
        Ok(seed) if seed < u64::MAX => Some(seed),
        _ => panic!(
            "VARDE_TEST_SEED={value:?}: expected a number below {}",
            u64::MAX
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_parse() {
        assert_eq!(parse_mode(None), Mode::Quick);
        assert_eq!(parse_mode(Some("")), Mode::Quick);
        assert_eq!(parse_mode(Some("quick")), Mode::Quick);
        assert_eq!(parse_mode(Some("full")), Mode::Full);
    }

    #[test]
    #[should_panic(expected = "VARDE_TESTS")]
    fn a_typo_fails_loudly() {
        parse_mode(Some("ful"));
    }

    #[test]
    fn seeds_parse_bounded() {
        assert_eq!(parse_seed(None), None);
        assert_eq!(parse_seed(Some(" 17 ")), Some(17));
        assert!(std::panic::catch_unwind(|| parse_seed(Some("-1"))).is_err());
        assert!(std::panic::catch_unwind(|| parse_seed(Some("x"))).is_err());
        assert!(std::panic::catch_unwind(|| parse_seed(Some(&u64::MAX.to_string()))).is_err());
    }
}
