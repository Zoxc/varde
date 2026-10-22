use super::*;
use crate::infra::budget::Budget;
use crate::infra::error::Cause;

/// A failing piece's place: its nearest anchor and its side.
type Loc = (u32, bool);

/// `2^e`, exactly, for `e` in the normal range.
fn p2(e: i32) -> f64 {
    f64::from_bits(u64::try_from(e + 1023).unwrap() << 52)
}

fn work() -> Work {
    Work::new(&Budget::DEFAULT)
}

fn fail(cert: Cert, key: u32, quantity: f64) -> Outcome<u32> {
    Outcome::Fail(Failed {
        cert,
        key,
        quantity,
        pins: Vec::new(),
    })
}

/// A cluster whose attempts and clearances are given as functions of the
/// bound (and of whether its abutting leaves are Simple), recording what
/// it was asked.
struct Fake {
    at: Box<dyn Fn(f64, bool) -> Outcome<u32>>,
    clear: Box<dyn Fn(f64) -> Outcome<u32>>,
    x_k: f64,
    simple: bool,
    simples: u32,
    attempts: Vec<f64>,
    clearances: Vec<f64>,
}

impl Fake {
    fn new(at: impl Fn(f64, bool) -> Outcome<u32> + 'static) -> Fake {
        Fake {
            at: Box::new(at),
            clear: Box::new(|_| Outcome::Pass),
            x_k: 100.0,
            simple: false,
            simples: 0,
            attempts: Vec::new(),
            clearances: Vec::new(),
        }
    }
}

impl Resnap for Fake {
    type Key = u32;
    type Error = ();

    fn attempt(&mut self, b_k: f64, _: u8, _: &mut Work) -> Result<Outcome<u32>, ()> {
        self.attempts.push(b_k);
        Ok((self.at)(b_k, self.simple))
    }

    fn simple(&mut self, _: &[Abutment], _: u8, _: &mut Work) -> Result<(), ()> {
        self.simple = true;
        self.simples += 1;
        Ok(())
    }

    fn clearance(&mut self, _: &[u32], b_k: f64, _: u8, _: &mut Work) -> Result<Outcome<u32>, ()> {
        self.clearances.push(b_k);
        Ok((self.clear)(b_k))
    }

    fn magnitude(&self) -> f64 {
        self.x_k
    }
}

fn rung(k: u8, unchanged: bool, near_tie_moved: bool) -> Rung {
    Rung {
        k,
        unchanged,
        near_tie_moved,
    }
}

fn pinned(locator: Loc) -> Signal<Loc> {
    Signal {
        check: Check::CutPoints,
        level_set: Vec::new(),
        locator,
    }
}

fn gap_of<E: Debug>(r: Result<impl Debug, EscalateError<E, u32, Loc>>) -> RuleGap<u32, Loc> {
    match r {
        Err(EscalateError::RuleGap(g)) => *g,
        other => panic!("not a rule gap: {other:?}"),
    }
}

const ABUT: Abutment = Abutment {
    leaf: 7,
    cluster: 0,
};

// ---------------------------------------------------------------------
// The storage floor.

/// The halvings a failure that keeps strictly improving gets, from
/// `b_K = ε/2` at magnitude `x_k`, before the floor.
fn halvings_to_floor(eps: f64, x_k: f64) -> u32 {
    let mut fake = Fake::new(|b, _| fail(Cert::Rim, 1, -b));
    fake.x_k = x_k;
    let mut esc = Escalation::<u32, Loc>::new(eps / 2.0, Vec::new()).unwrap();
    match esc.settle(&mut fake, &mut work()) {
        Err(EscalateError::Floor { b_k, x_k: x }) => {
            assert_eq!((b_k, x), (esc.b_k(), x_k));
            assert!(b_k >= eps_min(x_k).unwrap() && b_k / 2.0 < eps_min(x_k).unwrap());
        }
        other => panic!("not the floor: {other:?}"),
    }
    assert_eq!(fake.attempts.len() as u32, esc.halvings() + 1);
    esc.halvings()
}

#[test]
fn halving_stops_at_the_storage_floor_after_the_storable_halvings() {
    // From `b_K = ε/2`: 29 halvings stay storable at 100 mm and 16 at
    // 10⁶ mm at `ε = 10⁻³`, 22 and 9 at `ε = 10⁻⁵`.
    assert_eq!(halvings_to_floor(1e-3, 100.0), 29);
    assert_eq!(halvings_to_floor(1e-3, 1e6), 16);
    assert_eq!(halvings_to_floor(1e-5, 100.0), 22);
    assert_eq!(halvings_to_floor(1e-5, 1e6), 9);
}

// ---------------------------------------------------------------------
// Halving.

#[test]
fn a_true_tangency_passes_at_a_smaller_bound() {
    // The graph margin grows as the bound shrinks, and passes below
    // `b_K = 2⁻¹³`.
    let threshold = p2(-13);
    let mut fake = Fake::new(move |b, _| {
        if b <= threshold {
            Outcome::Pass
        } else {
            fail(Cert::Graph, 3, threshold - b)
        }
    });
    let mut esc = Escalation::<u32, Loc>::new(p2(-10), Vec::new()).unwrap();
    assert_eq!(esc.settle(&mut fake, &mut work()), Ok(threshold));
    assert_eq!(esc.halvings(), 3);
    assert_eq!(fake.attempts, [-10, -11, -12, -13].map(p2).to_vec());
    assert!(matches!(esc.events().last(), Some(Event::Passed { b_k }) if *b_k == threshold));
}

#[test]
fn the_same_failure_with_no_strict_improvement_is_a_rule_gap() {
    // A clearance that stays the same however small `b_K` gets: the first
    // halving shows it, and the escalation stops there.
    let mut fake = Fake::new(|_, _| fail(Cert::Rim, 5, -1e-4));
    let mut esc = Escalation::<u32, Loc>::new(1e-3, Vec::new()).unwrap();
    let g = gap_of(esc.settle(&mut fake, &mut work()));
    assert_eq!(g.gap, Gap::Stop);
    assert_eq!(g.error(), KernelError::Internal(Internal::Inconsistent));
    assert_eq!(esc.halvings(), 1);
    assert_eq!(fake.attempts, vec![1e-3, 5e-4]);
    // The evidence: each bound with its failure.
    let seq: Vec<f64> = g
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Failed { b_k, failed } => {
                assert_eq!((failed.cert, failed.key), (Cert::Rim, 5));
                Some(*b_k)
            }
            _ => None,
        })
        .collect();
    assert_eq!(seq, vec![1e-3, 5e-4]);
}

#[test]
fn a_failure_that_gets_worse_stops_too() {
    let mut fake = Fake::new(|b, _| fail(Cert::ThinWall, 2, -1.0 / b));
    let mut esc = Escalation::<u32, Loc>::new(1e-3, Vec::new()).unwrap();
    assert_eq!(gap_of(esc.settle(&mut fake, &mut work())).gap, Gap::Stop);
    assert_eq!(esc.halvings(), 1);
}

#[test]
fn a_failure_at_another_certificate_halves_again() {
    // The same quantity each time, but the failing certificate moves (its
    // key, then its kind): never the stop; the third attempt passes.
    let mut fake = Fake::new(|b, _| {
        if b == 1e-3 {
            fail(Cert::Rim, 1, -1.0)
        } else if b == 5e-4 {
            fail(Cert::Rim, 2, -1.0)
        } else if b == 2.5e-4 {
            fail(Cert::Curvature, 2, -1.0)
        } else {
            Outcome::Pass
        }
    });
    let mut esc = Escalation::<u32, Loc>::new(1e-3, Vec::new()).unwrap();
    assert_eq!(esc.settle(&mut fake, &mut work()), Ok(1.25e-4));
    assert_eq!(esc.halvings(), 3);
}

#[test]
fn validity_at_a_pinned_placement_is_a_rule_gap_at_once() {
    // A corner-rule placement whose bands sum above `b_K` sits in the
    // failing window: no halving can move it.
    let mut fake = Fake::new(|_, _| {
        Outcome::Fail(Failed {
            cert: Cert::Validity,
            key: 9,
            quantity: -1.0,
            pins: vec![[1e-4, 1e-4], [6e-4, 6e-4]],
        })
    });
    let mut esc = Escalation::<u32, Loc>::new(1e-3, Vec::new()).unwrap();
    assert_eq!(gap_of(esc.settle(&mut fake, &mut work())).gap, Gap::Pin);
    assert_eq!(esc.halvings(), 0);
    assert_eq!(fake.attempts, vec![1e-3]);
}

#[test]
fn validity_with_only_gap_bound_placements_halves() {
    // Bands summing to at most `b_K` at every bound tried: the on-radius
    // is `b_K`, the displacement moves with it.
    let mut fake = Fake::new(|b, _| {
        if b < 1e-3 {
            Outcome::Pass
        } else {
            Outcome::Fail(Failed {
                cert: Cert::Validity,
                key: 9,
                quantity: -1.0,
                pins: vec![[1e-4, 2e-4]],
            })
        }
    });
    let mut esc = Escalation::<u32, Loc>::new(1e-3, Vec::new()).unwrap();
    assert_eq!(esc.settle(&mut fake, &mut work()), Ok(5e-4));
}

#[test]
fn the_routing_rows_apply_in_order() {
    let at = |q: f64, pins: Vec<[f64; 2]>| Failed {
        cert: Cert::Validity,
        key: 0u32,
        quantity: q,
        pins,
    };
    let prev = at(-1.0, Vec::new());
    let floor = eps_min(100.0).unwrap();
    // The pin row before the stop.
    assert_eq!(
        route(&at(-1.0, vec![[1.0, 0.0]]), Some(&prev), 1e-3, 100.0),
        Ok(Route::Gap(Gap::Pin))
    );
    // The stop before the floor.
    assert_eq!(
        route(&at(-1.0, Vec::new()), Some(&prev), floor, 100.0),
        Ok(Route::Gap(Gap::Stop))
    );
    // The floor before halving; strictly better isn't the stop.
    assert_eq!(
        route(&at(-0.5, Vec::new()), Some(&prev), floor, 100.0),
        Ok(Route::Floor)
    );
    assert_eq!(
        route(&at(-0.5, Vec::new()), Some(&prev), 2.0 * floor, 100.0),
        Ok(Route::Halve)
    );
    // Pins are read for validity only.
    let rim = Failed {
        cert: Cert::Rim,
        ..at(-1.0, vec![[1.0, 0.0]])
    };
    assert_eq!(route(&rim, None, 1e-3, 100.0), Ok(Route::Halve));
    // Unusable inputs.
    assert_eq!(
        route(&at(f64::NAN, Vec::new()), None, 1e-3, 100.0),
        Err(InputError::Quantity)
    );
    assert_eq!(
        route(&at(-1.0, vec![[-1.0, 0.0]]), None, 1e-3, 100.0),
        Err(InputError::Band)
    );
    assert_eq!(
        route(&at(-1.0, Vec::new()), None, 0.0, 100.0),
        Err(InputError::Bound)
    );
    assert_eq!(
        route(&at(-1.0, Vec::new()), None, 1e-3, f64::NAN),
        Err(InputError::Magnitude)
    );
}

#[test]
fn a_certificate_still_open_at_a_limit_is_out_of_budget() {
    // Even where the same failure before would make it the stop.
    let open = charge(&mut Work::new(&Budget::new(0)), 0).unwrap_err();
    assert!(matches!(open, KernelError::OutOfBudget { .. }));
    let given = open.clone();
    let mut fake = Fake::new(move |b, _| {
        if b == 1e-3 {
            fail(Cert::Rim, 1, -1.0)
        } else {
            Outcome::Exhausted(given.clone())
        }
    });
    let mut esc = Escalation::<u32, Loc>::new(1e-3, Vec::new()).unwrap();
    assert_eq!(
        esc.settle(&mut fake, &mut work()),
        Err(EscalateError::OutOfBudget(open))
    );
}

#[test]
fn each_halving_is_charged() {
    let mut fake = Fake::new(|b, _| fail(Cert::Rim, 1, -b));
    let mut esc = Escalation::<u32, Loc>::new(1e-3, Vec::new()).unwrap();
    let mut work = Work::new(&Budget::new(3));
    match esc.settle(&mut fake, &mut work) {
        Err(EscalateError::OutOfBudget(KernelError::OutOfBudget { cause, .. })) => {
            assert_eq!(cause, Cause::Work)
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(esc.halvings(), 3);
    assert_eq!(work.used(), 3);
}

// ---------------------------------------------------------------------
// Abutment first.

#[test]
fn abutting_leaves_go_simple_before_any_halving() {
    // The Near cluster dissolves once the transversal face's leaves are
    // Simple: no halving.
    let mut fake = Fake::new(|_, simple| {
        if simple {
            Outcome::Pass
        } else {
            fail(Cert::Rim, 1, -1.0)
        }
    });
    let mut esc = Escalation::<u32, Loc>::new(1e-3, vec![ABUT]).unwrap();
    assert_eq!(esc.settle(&mut fake, &mut work()), Ok(1e-3));
    assert_eq!((fake.simples, esc.halvings()), (1, 0));
    assert_eq!(fake.attempts, vec![1e-3, 1e-3]);
}

#[test]
fn abutting_leaves_go_simple_once_only() {
    // Simple doesn't help: the retry at the same bound isn't a halving, so
    // its failure isn't compared with the first; halving then finds the
    // stop, and Simple is never tried again.
    let mut fake = Fake::new(|_, _| fail(Cert::Rim, 1, -1.0));
    let mut esc = Escalation::<u32, Loc>::new(1e-3, vec![ABUT]).unwrap();
    assert_eq!(gap_of(esc.settle(&mut fake, &mut work())).gap, Gap::Stop);
    assert_eq!((fake.simples, esc.halvings()), (1, 1));
    assert_eq!(fake.attempts, vec![1e-3, 1e-3, 5e-4]);
    let r = esc.signal(
        &mut fake,
        pinned((0, true)),
        rung(0, false, false),
        &mut work(),
    );
    assert_eq!(r, Ok(Next::Rung));
    assert_eq!(fake.simples, 1);
}

#[test]
fn a_later_check_tries_abutting_leaves_first() {
    let mut fake = Fake::new(|_, _| Outcome::Pass);
    let mut esc = Escalation::<u32, Loc>::new(1e-3, vec![ABUT]).unwrap();
    // The first attempt passes, so Simple isn't tried until a check fails.
    assert_eq!(esc.settle(&mut fake, &mut work()), Ok(1e-3));
    assert_eq!(fake.simples, 0);
    let r = esc.signal(
        &mut fake,
        pinned((0, true)),
        rung(0, false, false),
        &mut work(),
    );
    assert_eq!(r, Ok(Next::Resnapped { b_k: 1e-3 }));
    assert_eq!(fake.simples, 1);
}

// ---------------------------------------------------------------------
// Later checks.

#[test]
fn a_pinned_incidence_never_reaches_a_halving() {
    // At every rung the same pinned piece fails with precision still
    // acting: the ladder goes on, and ends at its last rung, a rule gap,
    // with the cluster never re-run.
    let mut fake = Fake::new(|_, _| Outcome::Pass);
    let mut esc = Escalation::<u32, Loc>::new(1e-3, Vec::new()).unwrap();
    assert_eq!(esc.settle(&mut fake, &mut work()), Ok(1e-3));
    for k in 0..K_MAX {
        let r = esc.signal(
            &mut fake,
            pinned((4, false)),
            rung(k, false, false),
            &mut work(),
        );
        assert_eq!(r, Ok(Next::Rung), "rung {k}");
    }
    let g = gap_of(esc.signal(
        &mut fake,
        pinned((4, false)),
        rung(K_MAX, false, false),
        &mut work(),
    ));
    assert_eq!(g.gap, Gap::Last);
    assert_eq!(esc.halvings(), 0);
    assert_eq!(fake.attempts, vec![1e-3]);
    assert!(fake.clearances.is_empty());
    // The evidence names the failing piece per rung.
    let rungs: Vec<u8> = g
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Signal { rung, signal, .. } => {
                assert_eq!(signal.locator, (4, false));
                Some(*rung)
            }
            _ => None,
        })
        .collect();
    assert_eq!(rungs, vec![0, 1, 2, 3, 4]);
}

#[test]
fn a_pinned_incidence_at_the_last_rung_with_a_near_tie_moved_is_out_of_precision() {
    let mut fake = Fake::new(|_, _| Outcome::Pass);
    let mut esc = Escalation::<u32, Loc>::new(1e-3, Vec::new()).unwrap();
    let r = esc.signal(
        &mut fake,
        pinned((4, false)),
        rung(K_MAX, false, true),
        &mut work(),
    );
    assert_eq!(r, Err(EscalateError::Precision));
}

#[test]
fn the_stop_on_a_pinned_piece_waits_for_rung_2_and_no_key_moved() {
    let mut fake = Fake::new(|_, _| Outcome::Pass);
    let mut esc = Escalation::<u32, Loc>::new(1e-3, Vec::new()).unwrap();
    let mut sig = |esc: &mut Escalation<u32, Loc>, loc, k, unchanged| {
        esc.signal(
            &mut fake,
            pinned(loc),
            rung(k, unchanged, false),
            &mut work(),
        )
    };
    // Rung 1, same piece, nothing moved: too early.
    assert_eq!(sig(&mut esc, (1, true), 0, true), Ok(Next::Rung));
    assert_eq!(sig(&mut esc, (1, true), 1, true), Ok(Next::Rung));
    // Rung 2, a key moved: the ladder goes on.
    assert_eq!(sig(&mut esc, (1, true), 2, false), Ok(Next::Rung));
    // Rung 3, another piece: goes on.
    assert_eq!(sig(&mut esc, (1, false), 3, true), Ok(Next::Rung));
    // Rung 3 again (a second check at that rung), the same piece as last,
    // nothing moved: the stop.
    assert_eq!(gap_of(sig(&mut esc, (1, false), 3, true)).gap, Gap::Stop);
}

#[test]
fn a_level_set_piece_failing_its_clearance_halves_past_the_last_rung() {
    // The named level-set piece's clearance fails at the first bound only:
    // that is a rim failure, which halves (at the ladder's last rung too:
    // halving has no rung limit), and the cluster passes at half.
    let mut fake = Fake::new(|_, _| Outcome::Pass);
    fake.clear = Box::new(|b| {
        if b == 1e-3 {
            fail(Cert::Rim, 6, -1.0)
        } else {
            Outcome::Pass
        }
    });
    let mut esc = Escalation::<u32, Loc>::new(1e-3, Vec::new()).unwrap();
    let s = Signal {
        check: Check::ZeroArea,
        level_set: vec![2, 3],
        locator: (2, true),
    };
    let r = esc.signal(&mut fake, s.clone(), rung(K_MAX, true, false), &mut work());
    assert_eq!(r, Ok(Next::Resnapped { b_k: 5e-4 }));
    assert_eq!(esc.halvings(), 1);
    assert_eq!(fake.clearances, vec![1e-3]);
    assert_eq!(fake.attempts, vec![5e-4]);
    // Clear now: the failure doesn't depend on `b_K`, and takes the pinned
    // route.
    let r = esc.signal(&mut fake, s, rung(1, true, false), &mut work());
    assert_eq!(r, Ok(Next::Rung));
    assert_eq!(fake.clearances, vec![1e-3, 5e-4]);
    assert_eq!(esc.halvings(), 1);
}

#[test]
fn a_level_set_clearance_that_never_improves_stops() {
    let mut fake = Fake::new(|_, _| fail(Cert::Rim, 6, -1.0));
    fake.clear = Box::new(|_| fail(Cert::Rim, 6, -1.0));
    let mut esc = Escalation::<u32, Loc>::new(1e-3, Vec::new()).unwrap();
    let s = Signal {
        check: Check::CutPoints,
        level_set: vec![0],
        locator: (0, true),
    };
    let r = esc.signal(&mut fake, s, rung(1, false, false), &mut work());
    assert_eq!(gap_of(r).gap, Gap::Stop);
    assert_eq!(esc.halvings(), 1);
}

#[test]
fn a_bound_that_isnt_positive_is_refused() {
    assert_eq!(
        Escalation::<u32, Loc>::new(0.0, Vec::new()).err(),
        Some(InputError::Bound)
    );
    assert_eq!(
        Escalation::<u32, Loc>::new(f64::NAN, Vec::new()).err(),
        Some(InputError::Bound)
    );
}
