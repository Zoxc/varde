use super::*;
use crate::infra::budget::{Budget, Pass, Step, Stop, Work};

/// The plan's number of each class: exhaustive, so a sixth class (or a
/// lost one) doesn't build.
fn number(class: Inconsistency) -> u8 {
    match class {
        Inconsistency::ExactTie => 1,
        Inconsistency::Proxy => 2,
        Inconsistency::ResultDefect => 3,
        Inconsistency::SosGuard => 4,
        Inconsistency::ClusterRuleGap => 5,
    }
}

const CLASSES: [Inconsistency; 5] = [
    Inconsistency::ExactTie,
    Inconsistency::Proxy,
    Inconsistency::ResultDefect,
    Inconsistency::SosGuard,
    Inconsistency::ClusterRuleGap,
];

#[test]
fn an_inconsistency_is_one_of_five_classes_two_of_them_rule_gaps() {
    let numbers: Vec<u8> = CLASSES.iter().map(|&c| number(c)).collect();
    assert_eq!(numbers, [1, 2, 3, 4, 5]);
    let gaps: Vec<u8> = CLASSES
        .iter()
        .filter(|c| c.is_rule_gap())
        .map(|&c| number(c))
        .collect();
    assert_eq!(gaps, [2, 5]);
    // Each class says what it is.
    let texts: std::collections::BTreeSet<String> = CLASSES
        .iter()
        .map(|&c| KernelError::Internal(Internal::Inconsistent(c)).to_string())
        .collect();
    assert_eq!(texts.len(), 5);
}

/// A release build has no stage variant: the match below names every
/// variant of [`Unsupported`], so it only builds without one.
#[cfg(not(feature = "staging"))]
#[test]
fn a_release_build_has_no_stage() {
    fn import(u: Unsupported) -> Import {
        match u {
            Unsupported::Import(i) => i,
        }
    }
    assert_eq!(
        import(Unsupported::Import(Import::SurfaceDegree)),
        Import::SurfaceDegree
    );
}

/// A development build has the five stages.
#[cfg(feature = "staging")]
#[test]
fn a_staging_build_has_the_stages() {
    for stage in [
        Stage::CheckStage,
        Stage::Tangency,
        Stage::MultiPair,
        Stage::SpliceRefit,
        Stage::Contact,
    ] {
        let e = KernelError::Unsupported(Unsupported::Stage(stage));
        assert!(e.to_string().contains(&format!("{stage:?}")));
    }
}

#[test]
fn the_storage_causes_an_operation_checks_keep_their_cause() {
    use varde_decide::num::storage::Unrepresentable as U;
    assert_eq!(
        KernelError::from(U::OutOfRange {
            value: f64::NAN,
            magnitude: f64::NAN,
        }),
        KernelError::Unrepresentable(Storage::OutOfRange)
    );
    assert_eq!(
        KernelError::from(U::Tolerance {
            eps: 1e-12,
            eps_min: 1e-9,
        }),
        KernelError::Unrepresentable(Storage::Tolerance)
    );
}

/// Running out of a budget of one unit at a pass's first key.
fn out_of_budget() -> KernelError {
    let pass = Pass {
        stage: "test",
        rung: 0,
    };
    let halt = Work::new(&Budget::new(1))
        .pass(pass, &[()], |_, ledger| {
            ledger.charge(Step::Work, 2)?;
            Ok::<_, Stop<KernelError>>(())
        })
        .unwrap_err();
    KernelError::from(halt)
}

fn failure(key: u32, check: u32, error: KernelError) -> Failure<u32> {
    Failure { key, check, error }
}

const TIE: KernelError = KernelError::Internal(Internal::Inconsistent(Inconsistency::ExactTie));
const GAP: KernelError =
    KernelError::Internal(Internal::Inconsistent(Inconsistency::ClusterRuleGap));
const REPAIR: KernelError = KernelError::Unrepresentable(Storage::Repair);

/// The failure reported: least key first; at one key running out, then
/// storage, then the contradiction; at one key and kind the first check.
/// In any order given.
#[test]
fn a_stage_reports_the_least_key_then_kind_then_check() {
    let cases: Vec<(Vec<Failure<u32>>, Failure<u32>)> = vec![
        // A later key's running out doesn't beat an earlier contradiction.
        (
            vec![failure(5, 0, out_of_budget()), failure(3, 2, TIE)],
            failure(3, 2, TIE),
        ),
        // At one key: out of budget, then unrepresentable, then internal,
        // whatever their checks.
        (
            vec![
                failure(3, 0, TIE),
                failure(3, 1, REPAIR),
                failure(3, 2, out_of_budget()),
            ],
            failure(3, 2, out_of_budget()),
        ),
        (
            vec![failure(3, 0, TIE), failure(3, 4, REPAIR)],
            failure(3, 4, REPAIR),
        ),
        // At one key and kind: the check first in the stage's order.
        (
            vec![failure(3, 2, TIE), failure(3, 1, GAP), failure(4, 0, TIE)],
            failure(3, 1, GAP),
        ),
        // The other kinds come after the three.
        (
            vec![
                failure(3, 0, KernelError::Invalid(Invalid::NotClosed)),
                failure(3, 1, GAP),
            ],
            failure(3, 1, GAP),
        ),
    ];
    for (found, want) in cases {
        let mut reversed = found.clone();
        reversed.reverse();
        assert_eq!(first_failure(found), Some(want.clone()));
        assert_eq!(first_failure(reversed), Some(want));
    }
    assert_eq!(first_failure(Vec::<Failure<u32>>::new()), None);
}
