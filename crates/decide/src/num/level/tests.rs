//! One generic computation at every level, and the runner's order and
//! stopping rule.

use super::*;

/// `3·(1/3) − 1 + d`, written once for every level: `d` exactly, plus
/// the level's rounding of a third.
fn third<S: LevelScalar>(d: f64, like: &S) -> S {
    let one = S::int(1, like);
    let three = S::int(3, like);
    one.div(&three)
        .mul(&three)
        .sub(&one)
        .add(&S::from_f64(d, like))
}

/// Records the level and type each run sees, and settles once the sign
/// of [`third`] at `d` is certified.
struct Third {
    d: f64,
    seen: Vec<Level>,
}

impl AtLevel<u32> for Third {
    type Out = (Level, Option<std::cmp::Ordering>);

    fn at<S: LevelScalar>(&mut self, like: &S, runs: &mut u32) -> Self::Out {
        *runs += 1;
        self.seen.push(like.level());
        (like.level(), third(self.d, like).sign())
    }

    fn escalates(out: &Self::Out) -> bool {
        out.1.is_none()
    }
}

/// Each level runs in its own type: the value a step is handed says
/// which level it is, and the big-float levels map to their widths.
#[test]
fn each_level_runs_in_its_own_type() {
    for level in Level::ALL {
        let mut t = Third {
            d: 0.0,
            seen: Vec::new(),
        };
        let mut runs = 0;
        let (at, _) = at_level(&mut t, level, &mut runs);
        assert_eq!((at, t.seen.as_slice(), runs), (level, &[level][..], 1));
        if let Some(w) = level.width() {
            assert_eq!(Level::of_width(w), level);
        }
    }
}

/// The one code path is certified at every level, and each level up
/// bounds it tighter: the enclosure holds the true value (0) and its
/// estimate, and the error shrinks strictly from `f64` to 848 bits.
#[test]
fn one_computation_tightens_level_by_level() {
    struct Err;
    impl AtLevel<()> for Err {
        type Out = ([f64; 2], f64, f64);

        fn at<S: LevelScalar>(&mut self, like: &S, _: &mut ()) -> Self::Out {
            let x = third(0.0, like);
            (x.enclose(), x.approx(), Coeff::err(&x))
        }

        fn escalates(_: &Self::Out) -> bool {
            true
        }
    }
    let mut last = f64::INFINITY;
    for level in Level::ALL {
        let ([lo, hi], approx, err) = at_level(&mut Err, level, &mut ());
        assert!(lo <= 0.0 && 0.0 <= hi, "{level:?}: [{lo}, {hi}]");
        assert!(lo <= approx && approx <= hi, "{level:?}: {approx}");
        assert!(err > 0.0 && err < last, "{level:?}: {err} after {last}");
        last = err;
    }
}

/// The runner goes up from its first level until a level settles, each
/// level's work in its own slot: `3·(1/3) − 1 + 2⁻⁶⁰` is below `f64`'s
/// rounding of a third but certified by double-double; `2⁻¹⁵⁰` needs the
/// first big-float width.
#[test]
fn the_ladder_stops_at_the_first_level_that_settles() {
    let cases = [
        (f64::from_bits((1023 - 60) << 52), Level::L0, Level::L1),
        (f64::from_bits((1023 - 150) << 52), Level::L0, Level::L2),
        (f64::from_bits((1023 - 150) << 52), Level::L2, Level::L2),
        (1.0, Level::L3, Level::L3),
    ];
    for (d, from, settles) in cases {
        let mut t = Third {
            d,
            seen: Vec::new(),
        };
        let mut runs = [0u32; LEVELS];
        let (at, sign) = ladder(&mut t, from, &mut runs);
        assert_eq!((at, sign), (settles, Some(std::cmp::Ordering::Greater)));
        let ran: Vec<Level> = Level::ALL
            .into_iter()
            .filter(|l| (from..=settles).contains(l))
            .collect();
        assert_eq!(t.seen, ran);
        for l in Level::ALL {
            assert_eq!(runs[l.index()], u32::from(ran.contains(&l)), "{l:?}");
        }
    }
}

/// At the top level the answer stands, escalating or not: an exact zero
/// has no certified sign at any level.
#[test]
fn the_top_level_answer_stands() {
    let mut t = Third {
        d: 0.0,
        seen: Vec::new(),
    };
    let mut runs = [0u32; LEVELS];
    assert_eq!(ladder(&mut t, Level::L0, &mut runs), (Level::TOP, None));
    assert_eq!(t.seen, Level::ALL);
    assert_eq!(runs, [1; LEVELS]);
}
