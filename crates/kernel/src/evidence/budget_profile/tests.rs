use varde_decide::ids::{Entity, VertId};
use varde_decide::num::storage::OpStart;
use varde_decide::records::keys::{Operand, Point, VvKey};

use super::*;
use crate::evidence::EVIDENCE_WORK;
use crate::infra::budget::{Budget, Cap, Halt, Ledger, Pass, Stop, Work};
use crate::infra::error::{Cause, KernelError};
use crate::infra::ops::{Tolerance, WarningKind};

const W: Step = Step::Work;

fn p(level: u8) -> Step {
    Step::Precision { level }
}

/// The steps of item `i` of the pass of stage `s` at rung `r`: a pure
/// function of them, of few distinct costs (so that many items tie),
/// mixing work with precision steps of several levels.
fn steps(s: usize, r: u8, i: u64) -> Vec<(Step, u64)> {
    let h = (i ^ (s as u64) << 20 ^ u64::from(r) << 30).wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 40;
    (0..h % 3 + 1)
        .map(|k| {
            let step = match (h >> (3 * k)) % 4 {
                0 | 1 => W,
                l => p(l as u8 - 1),
            };
            (step, (h >> (2 * k + 7)) % 5 + 1)
        })
        .collect()
}

const STAGES: [&str; 2] = ["first", "second"];
const ITEMS: u64 = 300;
/// The item of stage 1 that fails at rung 0, sending the operation to
/// rung 1.
const FAILS: u64 = 211;

/// Whether item `i` of stage `s` at rung `r` fails after its steps.
fn fails(s: usize, r: u8, i: u64) -> bool {
    s == 1 && r == 0 && i == FAILS
}

/// Runs the operation: per rung, each stage's pass over [`ITEMS`] items,
/// until a pass fails (then the next rung) or every pass of a rung
/// passes, at most three rungs; the profile of its running out, if it
/// does.
fn run(budget: u64) -> Option<(Cause, BudgetProfile)> {
    let items: Vec<u64> = (0..ITEMS).collect();
    let mut work = Work::new(&Budget::new(budget));
    for rung in 0..3u8 {
        let mut failed = false;
        for (s, &stage) in STAGES.iter().enumerate() {
            let out = work.pass(Pass { stage, rung }, &items, |&i, ledger: &mut Ledger| {
                for (step, units) in steps(s, rung, i) {
                    ledger.charge(step, units)?;
                }
                if fails(s, rung, i) {
                    return Err(Stop::Failed(()));
                }
                Ok(())
            });
            match out {
                Ok(_) => {}
                Err(Halt::Failed { .. }) => {
                    failed = true;
                    break;
                }
                Err(Halt::OutOfBudget(KernelError::OutOfBudget { cause, profile })) => {
                    return Some((cause, *profile));
                }
                Err(Halt::OutOfBudget(other)) => panic!("{other:?}"),
            }
        }
        if !failed {
            return None;
        }
    }
    None
}

/// An item counted: its key (pass, position), stage, rung, steps and
/// the units counted of them.
type Counted = ((u64, usize), &'static str, u8, Vec<(Step, u64)>, u64);

/// The same operation counted one item after another, the profile
/// assembled from every counted item at the end (hottest by sorting).
fn reference(budget: u64) -> Option<(Cause, BudgetProfile)> {
    let mut used = 0u64;
    let mut counted: Vec<Counted> = Vec::new();
    let mut pass = 0u64;
    let mut end = None;
    'rungs: for rung in 0..3u8 {
        let mut failed = false;
        for (s, &stage) in STAGES.iter().enumerate() {
            let mut sum = 0;
            for i in 0..ITEMS {
                let st = steps(s, rung, i);
                let total: u64 = st.iter().map(|&(_, n)| n).sum();
                let room = budget - used - sum;
                let key = (pass, i as usize);
                if total > room {
                    // The step holding unit `room`.
                    let mut start = 0;
                    let mut at = None;
                    for &(step, n) in &st {
                        if room < start + n {
                            at = Some(step);
                            break;
                        }
                        start += n;
                    }
                    counted.push((key, stage, rung, st, room));
                    used = budget;
                    let cause = match at {
                        Some(Step::Precision { .. }) => Cause::Precision,
                        _ => Cause::Work,
                    };
                    end = Some((cause, key, stage, rung, at));
                    break 'rungs;
                }
                sum += total;
                counted.push((key, stage, rung, st, total));
                if fails(s, rung, i) {
                    failed = true;
                    break;
                }
            }
            used += sum;
            pass += 1;
            if failed {
                break;
            }
        }
        if !failed {
            break;
        }
    }
    let (cause, (pass, item), stage, rung, step) = end?;
    let mut work = 0;
    let mut precision: Vec<u64> = Vec::new();
    let mut stages: Vec<(&'static str, u64)> = Vec::new();
    let mut rungs: Vec<u64> = Vec::new();
    for (_, stage, rung, st, units) in &counted {
        let mut left = *units;
        for &(step, n) in st {
            let take = n.min(left);
            left -= take;
            match step {
                _ if take == 0 => {}
                Step::Work => work += take,
                Step::Precision { level } => {
                    let l = usize::from(level);
                    if precision.len() <= l {
                        precision.resize(l + 1, 0);
                    }
                    precision[l] += take;
                }
            }
        }
        if *units > 0 {
            match stages.iter_mut().find(|(s, _)| s == stage) {
                Some((_, n)) => *n += units,
                None => stages.push((stage, *units)),
            }
            let r = usize::from(*rung);
            if rungs.len() <= r {
                rungs.resize(r + 1, 0);
            }
            rungs[r] += units;
        }
    }
    let mut hot: Vec<((u64, usize), u64)> = counted
        .iter()
        .filter(|c| c.4 > 0)
        .map(|c| (c.0, c.4))
        .collect();
    hot.sort_by_key(|&(key, n)| (std::cmp::Reverse(n), key));
    let at = |(pass, item)| ItemAt {
        pass,
        item,
        name: ItemName::default(),
    };
    Some((
        cause,
        BudgetProfile {
            limit: Limit::Budget(budget),
            used,
            work,
            precision,
            stages,
            rungs,
            stopped: Stopped {
                at: at((pass, item)),
                stage,
                rung,
                step,
            },
            hottest: hot
                .into_iter()
                .take(HOTTEST)
                .map(|(k, n)| (at(k), n))
                .collect(),
        },
    ))
}

/// Running out in each stage and rung (the first rung's second stage
/// failing at an item sends the operation on to the next), the profile
/// is the one counted item by item, its hottest items sorted most first
/// with ties in key order; the same on 1 and 8 threads, with the items
/// after the deciding key stopped early or run on; and its counts agree.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_profile_is_its_prefix_counted_item_by_item_on_any_number_of_threads() {
    use crate::infra::budget::assert_early_stop_changes_nothing;
    let mut seen = std::collections::BTreeSet::new();
    let mut ties = false;
    for budget in [1, 7, 300, 1_500, 2_000, 2_700, 3_500, 4_100, 4_700, 6_000] {
        let got = assert_early_stop_changes_nothing(|| run(budget));
        assert_eq!(got, reference(budget), "budget {budget}");
        if let Some((_, profile)) = got {
            assert_eq!(profile.check(), Ok(()));
            seen.insert((profile.stopped.stage, profile.stopped.rung));
            ties |= profile.hottest.windows(2).any(|w| w[0].1 == w[1].1);
        }
    }
    // Both stages of both rungs, and hottest items tied.
    assert_eq!(seen.len(), 4, "{seen:?}");
    assert!(ties);
}

/// Of many items of as many units, the hottest are the first eight in
/// key order; a later, costlier item goes before them all.
#[test]
fn hottest_items_tie_in_key_order() {
    let mut work = Work::new(&Budget::new(100));
    let items: Vec<u64> = (0..20).map(|i| if i == 15 { 3 } else { 2 }).collect();
    let halt = work
        .pass(
            Pass {
                stage: "ties",
                rung: 0,
            },
            &items,
            |&n, ledger| -> Result<(), Stop<()>> {
                ledger.charge(W, n)?;
                Ok(())
            },
        )
        .and_then(|_| {
            work.pass(
                Pass {
                    stage: "ties",
                    rung: 0,
                },
                &[1000u64],
                |&n, ledger| -> Result<(), Stop<()>> {
                    ledger.charge(W, n)?;
                    Ok(())
                },
            )
        })
        .unwrap_err();
    let Halt::OutOfBudget(KernelError::OutOfBudget { profile, .. }) = halt else {
        panic!("{halt:?}");
    };
    let hottest: Vec<(u64, usize, u64)> = profile
        .hottest
        .iter()
        .map(|(at, n)| (at.pass, at.item, *n))
        .collect();
    // The second pass's item: the 59 units left, past every first-pass item.
    let mut want = vec![(1, 0, 59), (0, 15, 3)];
    want.extend((0..6).map(|i| (0, i, 2)));
    assert_eq!(hottest, want);
    assert_eq!(profile.check(), Ok(()));
}

/// The level of the step an operation stopped at: a precision step's;
/// none at a work step, or before any step.
#[test]
fn the_exhausting_key_s_level_is_its_step_s() {
    let level_at = |steps: &[(Step, u64)]| {
        let mut work = Work::new(&Budget::new(5));
        let halt = work
            .pass(
                Pass {
                    stage: "level",
                    rung: 0,
                },
                &[()],
                |_, ledger| -> Result<(), Stop<()>> {
                    for &(step, n) in steps {
                        ledger.charge(step, n)?;
                    }
                    Err(Stop::Cap(Cap::SubdivisionDepth))
                },
            )
            .unwrap_err();
        match halt {
            Halt::OutOfBudget(KernelError::OutOfBudget { profile, .. }) => profile.stopped.level(),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(level_at(&[(W, 3), (p(2), 4)]), Some(2));
    assert_eq!(level_at(&[(p(3), 3), (W, 4)]), None);
    // Within the budget, the cap names its last step's level.
    assert_eq!(level_at(&[(W, 1), (p(1), 1)]), Some(1));
    assert_eq!(level_at(&[]), None);
}

/// A key's entities by name: those its record's key holds; none without
/// a record.
#[test]
fn an_item_is_named_by_its_record_s_entities() {
    let key = VvKey::of(Point::vert(Operand::A, VertId(4)), VertId(9)).key;
    let name = ItemName {
        record: Some(RecordKey::Vv(key)),
        world: None,
    };
    let named = |operand, v| Named {
        operand,
        entity: Entity::Vert(VertId(v)),
    };
    assert_eq!(
        name.entities(),
        vec![named(Operand::A, 4), named(Operand::B, 9)]
    );
    assert!(ItemName::default().entities().is_empty());
}

/// A profile whose counts disagree fails its check, for each way.
#[test]
fn a_profile_s_check_finds_counts_that_disagree() {
    let at = |pass, item| ItemAt {
        pass,
        item,
        name: ItemName::default(),
    };
    let good = BudgetProfile {
        limit: Limit::Budget(10),
        used: 10,
        work: 4,
        precision: vec![0, 6],
        stages: vec![("a", 7), ("b", 3)],
        rungs: vec![8, 2],
        stopped: Stopped {
            at: at(1, 0),
            stage: "b",
            rung: 1,
            step: Some(p(1)),
        },
        hottest: vec![(at(0, 1), 4), (at(0, 0), 3), (at(1, 0), 3)],
    };
    assert_eq!(good.check(), Ok(()));
    let bad = |f: &dyn Fn(&mut BudgetProfile)| {
        let mut p = good.clone();
        f(&mut p);
        p.check().unwrap_err()
    };
    assert_eq!(bad(&|p| p.work = 5), ProfileDefect::Steps);
    assert_eq!(bad(&|p| p.stages[1].1 = 4), ProfileDefect::Stages);
    assert_eq!(bad(&|p| p.rungs = vec![10, 1]), ProfileDefect::Rungs);
    assert_eq!(
        bad(&|p| p.stages = vec![("a", 7), ("a", 3)]),
        ProfileDefect::Stage
    );
    assert_eq!(bad(&|p| p.hottest[0].1 = 11), ProfileDefect::Hottest);
    assert_eq!(bad(&|p| p.hottest[2].1 = 0), ProfileDefect::Hottest);
    assert_eq!(
        bad(&|p| p.hottest.extend(vec![(at(2, 0), 1); 6])),
        ProfileDefect::Hottest
    );
    assert_eq!(bad(&|p| p.hottest.swap(1, 2)), ProfileDefect::Order);
    assert_eq!(bad(&|p| p.hottest.swap(0, 1)), ProfileDefect::Order);
    assert_eq!(bad(&|p| p.hottest[2].0 = at(0, 0)), ProfileDefect::Twice);
}

/// A status of 100 warnings (64 listed, all counted).
fn status() -> Status {
    let start = OpStart {
        max_coord: 100.0,
        rho_op: 1e-12,
    };
    let mut status = Status::new(Tolerance::DEFAULT, &start);
    for i in 0..100u32 {
        let kind = match i % 3 {
            0 => WarningKind::Accuracy,
            _ => WarningKind::StorageRounding,
        };
        status.warn(Warning::new(kind, f64::from(i % 17) * 1e-4).unwrap());
    }
    status
}

/// The warnings' evidence at allowances from none to unlimited: the
/// warnings, their bounds and count, the approximate flag and the
/// largest deviation are the same at every one; the listed warnings get
/// their evidence in order while the allowance lasts, then are only
/// named and counted, each later one too.
#[test]
fn warnings_evidence_degrades_with_its_allowance_and_the_warnings_do_not() {
    let status = status();
    let before = status.clone();
    // Item `i`'s evidence takes `i % 4` units.
    let cost = |i: usize, _: &Warning| (i % 4) as u64;
    let costs: Vec<u64> = (0..64).map(|i| (i % 4) as u64).collect();
    let mut last = None;
    for units in [0, 1, 5, 40, 95, 96, 1000, EVIDENCE_WORK, u64::MAX] {
        let mut allowance = Allowance::new(units);
        let ev = WarningEvidence::build(&status, &mut allowance, cost, |i, w| (i, w.bound()));
        assert_eq!(status, before);
        assert_eq!(status.warning_count(), 100);
        assert_eq!(ev.items.len(), status.warnings().len());
        // Built: the longest prefix whose costs fit.
        let mut sum = 0;
        let built = costs
            .iter()
            .take_while(|&&c| {
                sum += c;
                sum <= units
            })
            .count();
        for (i, item) in ev.items.iter().enumerate() {
            let want = (i < built).then(|| (i, status.warnings()[i].bound()));
            assert_eq!(*item, want, "allowance {units}, item {i}");
        }
        assert_eq!(ev.truncated as usize, 64 - built);
        last = Some(ev);
    }
    // 96 units cover every listed warning's evidence.
    assert_eq!(last.map(|e| e.truncated), Some(0));
}
