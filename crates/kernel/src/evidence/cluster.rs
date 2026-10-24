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
//!
//! As a [`Failure`] ([`ClusterEvidence::failure`]) it is the detail of
//! the site [`InternalSite::Cluster`], the caller's keys and locators
//! given as text.

use super::Allowance;
use super::failure::{Detail, Evidence, Failure, InternalSite, Site};
use crate::boolean::snap::corner::r_on;
use crate::boolean::snap::escalate::{Cert, Event, Failed, Gap, RuleGap, Signal};
use crate::infra::ops::OpCtx;

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

impl<K: Clone + std::fmt::Debug, L: Clone + std::fmt::Debug> ClusterEvidence<K, L> {
    /// The failure of the rule gap `gap` in operation `op`, at rung
    /// `rung` with tie band `tau`: the kernel's own known gap at the
    /// cluster ([`Site::Internal`] of [`InternalSite::Cluster`]), its
    /// evidence built from `allowance` ([`ClusterEvidence::build`]), the
    /// caller's keys and locators as their text. Replayed in full.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the boolean's snap driver calls it as it lands")
    )]
    pub(crate) fn failure(
        gap: &RuleGap<K, L>,
        op: OpCtx,
        rung: u8,
        tau: f64,
        allowance: &mut Allowance,
    ) -> Failure {
        let ev = ClusterEvidence::build(gap, allowance);
        let mut evidence = Evidence::new(op, Site::Internal(InternalSite::Cluster));
        evidence.rung = rung;
        evidence.tau = tau;
        evidence.truncated = ev.truncated;
        evidence.detail = Some(Detail::Cluster(Box::new(ev.texts())));
        Failure::new(gap.error(), evidence, "snap")
    }

    /// The evidence with the keys and locators as their text.
    fn texts(&self) -> ClusterEvidence<String, String> {
        let text = |x: &dyn std::fmt::Debug| format!("{x:?}");
        let event = |e: &Event<K, L>| match e {
            Event::Failed { b_k, failed } => Event::Failed {
                b_k: *b_k,
                failed: Failed {
                    cert: failed.cert,
                    key: text(&failed.key),
                    quantity: failed.quantity,
                    pins: failed.pins.clone(),
                },
            },
            Event::Simple { b_k, leaves } => Event::Simple {
                b_k: *b_k,
                leaves: *leaves,
            },
            Event::Halved { b_k } => Event::Halved { b_k: *b_k },
            Event::Passed { b_k } => Event::Passed { b_k: *b_k },
            Event::Signal { b_k, rung, signal } => Event::Signal {
                b_k: *b_k,
                rung: *rung,
                signal: Signal {
                    check: signal.check,
                    level_set: signal.level_set.clone(),
                    locator: text(&signal.locator),
                },
            },
        };
        ClusterEvidence {
            gap: self.gap,
            bounds: self.bounds.clone(),
            steps: (self.steps.iter())
                .map(|s| ClusterStep {
                    event: event(&s.event),
                    r_on: s.r_on.clone(),
                })
                .collect(),
            truncated: self.truncated,
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
