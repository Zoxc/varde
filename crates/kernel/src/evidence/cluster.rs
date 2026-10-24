//! The evidence of a tangency cluster no rule settles: a
//! [`RuleGap`], the kernel's own known gap
//! ([`Inconsistency::ClusterRuleGap`](crate::Inconsistency::ClusterRuleGap)),
//! at the cluster.
//!
//! It says how the cluster got there: the route that ended it (a pinned
//! placement, the stop, the ladder's last rung), the gap bounds `b_K`
//! tried in order, and each step of its escalation: per attempt the
//! failing certificate's kind and key with its certified quantity, or
//! the later check failing and its piece per rung; for a validity
//! failure, each corner-rule placement and bracket end in its window
//! with its on-radius `R_on = max(b_K, β_x + β_y)`, the term that pinned
//! it. The rim itself is the caller's geometry, named by the keys.
//!
//! Built in the evidence pass from its allowance
//! ([`Allowance`]): the bounds first, then the steps from the last back,
//! the failure that ended the ladder and those it was compared with
//! being the ones that tell; past the allowance, the earlier steps are
//! dropped and counted.

use super::Allowance;
use crate::boolean::snap::corner::r_on;
use crate::boolean::snap::escalate::{Cert, Event, Gap, RuleGap};

/// The evidence of a cluster rule gap ([`ClusterEvidence::build`]).
#[derive(Clone, Debug, PartialEq)]
pub struct ClusterEvidence<K, L> {
    /// The route that ended the ladder.
    pub gap: Gap,
    /// The gap bounds tried, in order: the first attempt's, then each
    /// halving's. Empty if the allowance couldn't afford them.
    pub bounds: Vec<f64>,
    /// The escalation's steps, in order: the last ones, as many as the
    /// allowance afforded after the bounds.
    pub steps: Vec<ClusterStep<K, L>>,
    /// How many items were dropped: the earlier steps, and the bounds as
    /// one.
    pub truncated: u32,
}

/// One step of a cluster's escalation, as its evidence gives it.
#[derive(Clone, Debug, PartialEq)]
pub struct ClusterStep<K, L> {
    /// What happened.
    pub event: Event<K, L>,
    /// For a failed validity certificate, the on-radius `R_on` of each
    /// placement in its window at the attempt's bound, in the order of
    /// its bands; empty otherwise.
    pub r_on: Vec<f64>,
}

impl<K: Clone, L: Clone> ClusterEvidence<K, L> {
    /// The evidence of `gap`, from `allowance`: a unit per bound, then
    /// per step a unit and one per placement, the last step first.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the boolean's snap driver calls it as it lands")
    )]
    pub(crate) fn build(gap: &RuleGap<K, L>, allowance: &mut Allowance) -> ClusterEvidence<K, L> {
        let mut truncated = 0u32;
        let tried = bounds(&gap.events);
        let bounds = if allowance.afford(len(tried.len())) {
            tried
        } else {
            truncated += 1;
            Vec::new()
        };
        let mut steps = Vec::new();
        for event in gap.events.iter().rev() {
            let pins = match event {
                Event::Failed { failed, .. } if failed.cert == Cert::Validity => &failed.pins[..],
                _ => &[],
            };
            if !allowance.afford(len(pins.len()).saturating_add(1)) {
                truncated = truncated.saturating_add(1);
                continue;
            }
            let r_on = match event {
                Event::Failed { b_k, .. } => {
                    pins.iter().map(|&[bx, by]| r_on(*b_k, bx, by)).collect()
                }
                _ => Vec::new(),
            };
            steps.push(ClusterStep {
                event: event.clone(),
                r_on,
            });
        }
        steps.reverse();
        ClusterEvidence {
            gap: gap.gap,
            bounds,
            steps,
            truncated,
        }
    }
}

/// The bounds a cluster was tried at, in order, from its events: the
/// first event's, then each halving's.
fn bounds<K, L>(events: &[Event<K, L>]) -> Vec<f64> {
    let mut out = Vec::new();
    for e in events {
        let b_k = match *e {
            Event::Failed { b_k, .. }
            | Event::Simple { b_k, .. }
            | Event::Passed { b_k }
            | Event::Signal { b_k, .. } => {
                if !out.is_empty() {
                    continue;
                }
                b_k
            }
            Event::Halved { b_k } => b_k,
        };
        out.push(b_k);
    }
    out
}

/// `n` as units.
fn len(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests;
