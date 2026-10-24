use super::*;
use crate::boolean::snap::escalate::{Check, Failed, Signal};

/// A failing piece's place: its nearest anchor and its side.
type Loc = (u32, bool);

fn failed(b_k: f64, cert: Cert, key: u32, quantity: f64, pins: Vec<[f64; 2]>) -> Event<u32, Loc> {
    Event::Failed {
        b_k,
        failed: Failed {
            cert,
            key,
            quantity,
            pins,
        },
    }
}

/// A cluster tried at `1/4`, recertified Simple, halved twice on a rim
/// failure improving, then failing its validity at a pinned placement;
/// before that a later check failed on it at rung 1.
fn pinned() -> RuleGap<u32, Loc> {
    RuleGap {
        gap: Gap::Pin,
        events: vec![
            failed(0.25, Cert::Rim, 3, -0.5, Vec::new()),
            Event::Simple {
                b_k: 0.25,
                leaves: 2,
            },
            failed(0.25, Cert::Rim, 3, -0.4, Vec::new()),
            Event::Halved { b_k: 0.125 },
            Event::Passed { b_k: 0.125 },
            Event::Signal {
                b_k: 0.125,
                rung: 1,
                signal: Signal {
                    check: Check::CutPoints,
                    level_set: vec![4],
                    locator: (7, true),
                },
            },
            failed(0.125, Cert::Rim, 3, -0.2, Vec::new()),
            Event::Halved { b_k: 0.0625 },
            // One placement within `b_K`, one whose bands sum past it.
            failed(
                0.0625,
                Cert::Validity,
                9,
                -1.0,
                vec![[0.01, 0.02], [0.05, 0.03]],
            ),
        ],
    }
}

/// With room for all of it, the evidence holds the route, the bounds
/// tried in order and every step, the validity failure with its
/// placements' on-radii: `b_K` for the one within it, the bands' sum for
/// the one that pins it.
#[test]
fn a_rule_gap_s_evidence_holds_its_route_bounds_and_steps() {
    let gap = pinned();
    let ev = ClusterEvidence::build(&gap, &mut Allowance::new(u64::MAX));
    assert_eq!(ev.gap, Gap::Pin);
    assert_eq!(ev.bounds, vec![0.25, 0.125, 0.0625]);
    assert_eq!(ev.truncated, 0);
    let events: Vec<_> = ev.steps.iter().map(|s| s.event.clone()).collect();
    assert_eq!(events, gap.events);
    let last = ev.steps.last().unwrap();
    assert_eq!(last.r_on[0], 0.0625);
    // The sum rounded up.
    assert!(last.r_on[1] >= 0.05 + 0.03 && last.r_on[1] < 0.081);
    assert!(ev.steps[..8].iter().all(|s| s.r_on.is_empty()));
    // The bounds, one unit each, then a unit a step and one a placement.
    let mut exact = Allowance::new(3 + 9 + 2);
    assert_eq!(ClusterEvidence::build(&gap, &mut exact), ev);
}

/// Short of room, the last steps are kept, in order, and the earlier
/// ones dropped and counted; without room for the bounds nothing is
/// kept but the route. The route and the error never change.
#[test]
fn a_short_allowance_keeps_the_last_steps() {
    let gap = pinned();
    // The bounds (3), the validity failure (3), the halving before it (1).
    let ev = ClusterEvidence::build(&gap, &mut Allowance::new(7));
    assert_eq!(ev.bounds.len(), 3);
    let events: Vec<_> = ev.steps.iter().map(|s| s.event.clone()).collect();
    assert_eq!(events, gap.events[7..]);
    assert_eq!(ev.truncated, 7);
    // One unit short of the validity failure: no step, though the cheap
    // ones before it would fit.
    let ev = ClusterEvidence::build(&gap, &mut Allowance::new(5));
    assert!(ev.steps.is_empty());
    assert_eq!(ev.truncated, 9);
    let ev = ClusterEvidence::build(&gap, &mut Allowance::new(2));
    assert_eq!((ev.gap, ev.bounds.len(), ev.steps.len()), (Gap::Pin, 0, 0));
    assert_eq!(ev.truncated, 10);
    assert_eq!(
        gap.error(),
        crate::KernelError::Internal(crate::Internal::Inconsistent(
            crate::Inconsistency::ClusterRuleGap
        ))
    );
}

/// A cluster that failed at its first bound with no halving has that
/// bound alone.
#[test]
fn a_cluster_never_halved_has_one_bound() {
    let gap = RuleGap::<u32, Loc> {
        gap: Gap::Stop,
        events: vec![
            failed(0.5, Cert::ThinWall, 1, -1.0, Vec::new()),
            failed(0.5, Cert::ThinWall, 1, -1.0, Vec::new()),
        ],
    };
    let ev = ClusterEvidence::build(&gap, &mut Allowance::default());
    assert_eq!((ev.bounds, ev.steps.len()), (vec![0.5], 2));
}
