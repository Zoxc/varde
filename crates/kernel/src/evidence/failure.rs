//! What an operation gives back when it fails: its error and the
//! evidence of where ([`Failure`], [`Evidence`]).
//!
//! **The error decides; the evidence tells.** Deciding carries only the
//! failing key. The evidence is built after the outcome is fixed, from
//! its own allowance ([`Allowance`]): running out of it degrades items to
//! their names and leaves geometry out, counted in
//! [`Evidence::truncated`]. The error, its kind and its key are the same
//! whatever the allowance.
//!
//! **The site** ([`Site`]) refines the error's one cause: where the
//! budget ran out (the stage and the loop of the budget's table), which
//! certificate reached the highest precision still open, which class of
//! the kernel's own failure it is. It fits its error ([`Site::fits`]):
//! no site names a cause the error doesn't have.
//!
//! **Contents.** The operation's context, the attempt (rung) and its tie
//! band `τ`, the precision level reached, at most
//! [`MAX_EVIDENCE_ITEMS`] items (the input entities, by name, least keys
//! first; [`Item`]), the geometry cut out of the leaves
//! ([`SheetSnippet`]) and that of entities the operation made itself
//! (`derived`, labelled), how to replay it ([`Replay`], by the error:
//! [`ReplayKind::of`]), the ladder's attempts, and the details of a
//! cluster no rule settles ([`Detail`]).
//!
//! **A body's check** gives its failure's evidence by role
//! ([`Failure::of_check`]): the failing item by name, the faces round it
//! cut out with its invariant left out, and the site of the kernel's own
//! defects.

use super::Allowance;
use super::cluster::ClusterEvidence;
use super::inputref::{InputRef, Item, ItemRole, Leaf};
use super::snippet::{MAX_SNIPPET, SheetSnippet};
use crate::boolean::snap::escalate::K_MAX;
use crate::evidence::budget_profile::BudgetProfile;
use crate::infra::budget::{Cap, Limit};
use crate::infra::error::{
    Cause, Inconsistency, Internal, Invalid, KernelError, Storage, Unsupported,
};
use crate::infra::ops::{OpCtx, Repair, ToleranceOutOfRange};
use crate::topo::brep::{Brep, CoedgeId, EdgeId, FaceId, Member};
use crate::topo::check::{CheckError, Finding, Inv, Item as CheckItem, Outcome, Role};
use varde_decide::ids::Entity;
use varde_decide::num::bounded::Bounded;

/// The most items an [`Evidence`] lists; past them, items are counted in
/// [`Evidence::truncated`].
pub const MAX_EVIDENCE_ITEMS: usize = 8;

/// Why an operation gives no result, and where (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    /// Why.
    pub error: KernelError,
    /// Where. Boxed, so a `Result` carrying a failure stays small.
    pub evidence: Box<Evidence>,
}

/// What a [`Failure`] (or a warning) is about (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Evidence {
    /// The operation's context (a batch's own).
    pub op: OpCtx,
    /// The attempt (rung of the escalation ladder) it failed at.
    pub rung: u8,
    /// The tie band `τ` of that attempt (mm).
    pub tau: f64,
    /// Where, refining the error's cause.
    pub site: Site,
    /// The precision level reached, if a precision step was the last.
    pub level: Option<u8>,
    /// The items, at most [`MAX_EVIDENCE_ITEMS`], least keys first.
    pub items: Vec<Item>,
    /// The leaves' faces involved, cut out.
    pub geometry: SheetSnippet,
    /// The entities the operation made itself (a batch node's, a failing
    /// derived arc), labelled.
    pub derived: Option<SheetSnippet>,
    /// How to replay it.
    pub replay: Replay,
    /// The ladder's attempts before it: rung, `τ` and failing key.
    pub ladder: Vec<LadderStep>,
    /// What the site holds beyond items and geometry.
    pub detail: Option<Detail>,
    /// How many items were dropped or degraded to their names: past
    /// [`MAX_EVIDENCE_ITEMS`] or past the allowance.
    pub truncated: u32,
}

/// Where a failure is (module docs): one per error cause, open for the
/// sites operations add as they land.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Site {
    /// No site: for errors whose kind says it all (an operand invalid, a
    /// value not stored, a kind not taken, a feature not built).
    None,
    /// The budget ran out (any cause), or an item reached a fixed size
    /// limit: the stage and the loop of the budget's table it was in.
    Budget {
        /// The stage, as the profile names it.
        stage: &'static str,
        /// The loop.
        row: BudgetRow,
    },
    /// A curved sign still open at the highest precision.
    CurvedSign,
    /// A leaf certificate still open at the highest precision.
    Leaf,
    /// An end's placement still open at the highest precision.
    EndPlacement,
    /// A thin box still open at the highest precision.
    ThinBox,
    /// A knife edge still open at the highest precision.
    KnifeEdge,
    /// A loop's nesting (`LoopInRec`) still open at the highest
    /// precision: only for a span kind without exact polynomial data.
    LoopIn,
    /// A snap certificate still open at the highest precision.
    Snap {
        /// Which.
        kind: SnapSite,
    },
    /// A validity certificate still open at the highest precision
    /// (invariant 4's disjointness, nesting or sector; invariant 9's
    /// embedding or window; splice clearance; a snapped edge's fit; a join
    /// refit), by its invariant; or, under a cluster rule gap, a result
    /// face certified too tight to embed (invariant 9).
    Validity {
        /// The invariant.
        inv: Inv,
    },
    /// The label audit's query no precision or depth certifies.
    LabelAudit(AuditSite),
    /// The class of the kernel's own failure.
    Internal(InternalSite),
}

/// Which snap certificate a [`Site::Snap`] is.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SnapSite {
    /// A `SnapAlong` cell.
    Along,
    /// A corner rule's distance.
    Corner,
    /// A rim piece's gap sign.
    RimGap,
    /// A branch's root cluster.
    Branch,
}

/// Why a label audit failed ([`Site::LabelAudit`]).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AuditSite {
    /// Every centre query of the region still open at the highest
    /// precision, in every direction, at every far box within the depth.
    Uncertifiable,
}

/// The class of the kernel's own failure ([`Site::Internal`]), as its
/// [`Inconsistency`].
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InternalSite {
    /// A ladder's end at exact ties.
    ExactTie,
    /// A ladder's end at a proxy.
    Proxy,
    /// A result failing invariant `inv`, or a crossing or defect
    /// certified on it.
    ResultDefect {
        /// The invariant.
        inv: Inv,
    },
    /// The symbolic perturbation's guard reached.
    SosGuard,
    /// A tangency cluster no rule settles.
    Cluster,
}

impl InternalSite {
    /// The class it is.
    pub fn class(self) -> Inconsistency {
        match self {
            InternalSite::ExactTie => Inconsistency::ExactTie,
            InternalSite::Proxy => Inconsistency::Proxy,
            InternalSite::ResultDefect { .. } => Inconsistency::ResultDefect,
            InternalSite::SosGuard => Inconsistency::SosGuard,
            InternalSite::Cluster => Inconsistency::ClusterRuleGap,
        }
    }
}

/// A loop of the budget's table: what was being charged where it ran
/// out.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BudgetRow {
    /// The operands' scan at the start: ranges and finite values.
    OperandScan,
    /// The broad phase, leaves, snaps and escalations.
    Broad,
    /// Stale leaves recertified after a snap, cluster merges, the join
    /// pass.
    Recertify,
    /// Records, counting, the seed clearance search, the tie band's sort.
    Records,
    /// Precision steps.
    Precision,
    /// Flat predicates' stages.
    FlatPredicates,
    /// The second tier's end matching.
    T2Ends,
    /// Fits, seams, splices and refit rounds.
    Fits,
    /// The repair of rounded results.
    Repair,
    /// The label audit.
    LabelAudit,
    /// A feature operation's surgery.
    Surgery,
    /// Tidying and resealing.
    Tidy,
    /// A body's check.
    Check,
    /// The ladder's attempts.
    Ladder,
    /// Tessellation: its triangles and refinement.
    Tessellation,
}

impl Site {
    /// Whether it fits `error`: no site but [`Site::None`] for an error
    /// whose kind says it all; a budget site for any running out; a
    /// precision site only at the highest precision; the label audit's
    /// only at a size limit; a class only for its own class (and a result
    /// face too tight to embed, [`Site::Validity`] of invariant 9, for a
    /// cluster rule gap).
    pub fn fits(&self, error: &KernelError) -> bool {
        use KernelError as E;
        let cause = match error {
            E::OutOfBudget { cause, .. } => Some(*cause),
            _ => None,
        };
        match (self, error) {
            (Site::None, E::OutOfBudget { .. } | E::Internal(_)) => false,
            (Site::None, _) => true,
            (Site::Budget { .. }, E::OutOfBudget { .. }) => true,
            (Site::LabelAudit(_), _) => cause == Some(Cause::SizeLimit),
            (
                Site::CurvedSign
                | Site::Leaf
                | Site::EndPlacement
                | Site::ThinBox
                | Site::KnifeEdge
                | Site::LoopIn
                | Site::Snap { .. },
                _,
            ) => cause == Some(Cause::Precision),
            (Site::Validity { inv }, E::Internal(Internal::Inconsistent(c))) => {
                *inv == Inv::Embedding && *c == Inconsistency::ClusterRuleGap
            }
            (Site::Validity { .. }, _) => cause == Some(Cause::Precision),
            (Site::Internal(s), E::Internal(Internal::Inconsistent(c))) => s.class() == *c,
            _ => false,
        }
    }
}

/// How a failure is replayed: the stage again on the snippet (local),
/// the whole operation on its operands (full), or not at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReplayKind {
    /// Not replayed: the error and its values say it all.
    None,
    /// The stage's entry point on the snippet.
    Local,
    /// The operation again on its operands and parameters.
    Full,
}

impl ReplayKind {
    /// The replay of a failure with `error` at `site`: local for an
    /// operand's check and self-intersection, a curved sign or leaf at
    /// the highest precision, and a check left out at this stage; none
    /// for an open sheet, a refused tolerance, a value out of range or a
    /// tolerance below storage's, an import refused and the cases not
    /// implemented; full for the rest (every running out, every failure
    /// of the kernel's own, a repair failing, a feature operation's).
    pub fn of(error: &KernelError, site: &Site) -> ReplayKind {
        use KernelError as E;
        match error {
            E::OutOfBudget {
                cause: Cause::Precision,
                ..
            } if matches!(site, Site::CurvedSign | Site::Leaf) => ReplayKind::Local,
            E::OutOfBudget { .. } | E::Internal(_) | E::Feature(_) => ReplayKind::Full,
            E::Invalid(Invalid::Check { .. } | Invalid::SelfIntersection) => ReplayKind::Local,
            E::Invalid(Invalid::NotClosed | Invalid::Tolerance) => ReplayKind::None,
            E::Unrepresentable(Storage::Repair) => ReplayKind::Full,
            E::Unrepresentable(_) => ReplayKind::None,
            E::Unsupported(Unsupported::Import(_)) => ReplayKind::None,
            #[cfg(feature = "staging")]
            E::Unsupported(Unsupported::Stage(crate::infra::error::Stage::CheckStage)) => {
                ReplayKind::Local
            }
            #[cfg(feature = "staging")]
            E::Unsupported(Unsupported::Stage(_)) => ReplayKind::Full,
            E::NotImplemented(_) => ReplayKind::None,
        }
    }
}

/// How a failure is replayed ([`ReplayKind`]), with what the replay
/// needs.
#[derive(Clone, Debug, PartialEq)]
pub enum Replay {
    /// Not replayed.
    None,
    /// The stage's entry point on the snippet, which keeps the leaves'
    /// ids.
    Local {
        /// The stage.
        stage: &'static str,
    },
    /// The operation again on its operands.
    Full(FullReplay),
}

impl Replay {
    /// Its kind.
    pub fn kind(&self) -> ReplayKind {
        match self {
            Replay::None => ReplayKind::None,
            Replay::Local { .. } => ReplayKind::Local,
            Replay::Full(_) => ReplayKind::Full,
        }
    }
}

/// What a full replay needs: the operands' digests and the operation's
/// parameters, and in-process the operands themselves. The place is
/// kept; its content comes with the replay.
#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FullReplay {}

/// One attempt of the escalation ladder before the failure.
#[derive(Clone, Debug, PartialEq)]
pub struct LadderStep {
    /// The rung.
    pub rung: u8,
    /// Its tie band `τ` (mm).
    pub tau: f64,
    /// The key failing there, as text.
    pub key_text: String,
}

/// What a site holds beyond items and geometry.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum Detail {
    /// A tangency cluster no rule settles: its route, bounds and steps,
    /// keys and locators as text.
    Cluster(Box<ClusterEvidence<String, String>>),
    /// A repair of a rounded result: a `Repair` warning's, or a repair
    /// failing ([`Storage::Repair`]).
    Repair(Box<RepairDetail>),
}

/// What a repair's evidence holds beyond its items, which name the
/// site's entities (the result's and the operands', role
/// [`ItemRole::Repaired`]): the repair, the certified unrounded order or
/// the failing certificate, the values chosen and the achieved bound;
/// for a repair failing, the rounds tried.
#[derive(Clone, Debug, PartialEq)]
pub struct RepairDetail {
    /// The repair.
    pub kind: Repair,
    /// The certified unrounded order the rounding broke, or the
    /// certificate failing on the stored values, as text.
    pub order: String,
    /// The values chosen (stored), in the site's order.
    pub chosen: Vec<f64>,
    /// The achieved deviation bound (mm).
    pub bound: f64,
    /// The rounds tried, for a repair failing.
    pub rounds: Option<u32>,
}

/// How an [`Evidence`] doesn't hold together ([`Failure::check`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceDefect {
    /// The site doesn't fit the error.
    Site,
    /// More than [`MAX_EVIDENCE_ITEMS`] items.
    Items,
    /// A snippet of more than [`MAX_SNIPPET`] faces.
    Snippet,
    /// The replay isn't the error's.
    Replay,
    /// More ladder steps than rungs, or rungs not increasing.
    Ladder,
}

impl Evidence {
    /// The evidence of operation `op` at `site`, at rung 0 with `τ` 0,
    /// holding nothing yet.
    pub fn new(op: OpCtx, site: Site) -> Evidence {
        Evidence {
            op,
            rung: 0,
            tau: 0.0,
            site,
            level: None,
            items: Vec::new(),
            geometry: SheetSnippet::default(),
            derived: None,
            replay: Replay::None,
            ladder: Vec::new(),
            detail: None,
            truncated: 0,
        }
    }

    /// Adds `item` after the others, if fewer than
    /// [`MAX_EVIDENCE_ITEMS`] are listed: with its values if `allowance`
    /// affords a unit for it and one per value, else degraded to its name
    /// (counted). Past the cap it is only counted. Whether it was listed.
    pub(crate) fn push_item(&mut self, mut item: Item, allowance: &mut Allowance) -> bool {
        if self.items.len() >= MAX_EVIDENCE_ITEMS {
            self.truncated = self.truncated.saturating_add(1);
            return false;
        }
        let units = u64::try_from(item.values.len())
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        if !allowance.afford(units) {
            item.values.clear();
            self.truncated = self.truncated.saturating_add(1);
        }
        self.items.push(item);
        true
    }
}

impl Evidence {
    /// The evidence of a repair (a `Repair` warning, or with its rounds a
    /// repair failing) of operation `op`: no site, the entities `site`
    /// (the result's under [`Leaf::RESULT`], the operands') as items in
    /// their order, from `allowance`, and `detail`.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the repair of rounded results calls it as it lands"
        )
    )]
    pub(crate) fn of_repair(
        op: OpCtx,
        site: Vec<InputRef>,
        detail: RepairDetail,
        allowance: &mut Allowance,
    ) -> Evidence {
        let mut ev = Evidence::new(op, Site::None);
        let text = format!("repair {:?}", detail.kind);
        for subject in site {
            let item = Item::new(Some(subject), ItemRole::Repaired, text.clone());
            ev.push_item(item, allowance);
        }
        ev.detail = Some(Detail::Repair(Box::new(detail)));
        ev
    }
}

impl Failure {
    /// A repair failing after `detail.rounds` rounds
    /// ([`Storage::Repair`], cause 3): its evidence as a repair's
    /// ([`Evidence::of_repair`]), replayed in full.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the repair of rounded results calls it as it lands"
        )
    )]
    pub(crate) fn of_repair(
        op: OpCtx,
        site: Vec<InputRef>,
        detail: RepairDetail,
        allowance: &mut Allowance,
    ) -> Failure {
        let ev = Evidence::of_repair(op, site, detail, allowance);
        Failure::new(KernelError::Unrepresentable(Storage::Repair), ev, "repair")
    }

    /// The failure of `error` with `evidence`, replayed as the error says
    /// ([`ReplayKind::of`]): a local replay at `stage`.
    pub fn new(error: KernelError, mut evidence: Evidence, stage: &'static str) -> Failure {
        evidence.replay = match ReplayKind::of(&error, &evidence.site) {
            ReplayKind::None => Replay::None,
            ReplayKind::Local => Replay::Local { stage },
            ReplayKind::Full => Replay::Full(FullReplay::default()),
        };
        let f = Failure {
            error,
            evidence: Box::new(evidence),
        };
        debug_assert_eq!(f.check(), Ok(()), "{f:?}");
        f
    }

    /// Whether its evidence holds together: the site fits the error, the
    /// items and snippets within their caps, the replay the error's, the
    /// ladder's rungs increasing and at most [`K_MAX`] before the last.
    pub fn check(&self) -> Result<(), EvidenceDefect> {
        let ev = &self.evidence;
        if !ev.site.fits(&self.error) {
            return Err(EvidenceDefect::Site);
        }
        if ev.items.len() > MAX_EVIDENCE_ITEMS {
            return Err(EvidenceDefect::Items);
        }
        let snippets = std::iter::once(&ev.geometry).chain(ev.derived.as_ref());
        if snippets.into_iter().any(|s| s.face_count() > MAX_SNIPPET) {
            return Err(EvidenceDefect::Snippet);
        }
        if ev.replay.kind() != ReplayKind::of(&self.error, &ev.site) {
            return Err(EvidenceDefect::Replay);
        }
        let increasing = ev.ladder.windows(2).all(|w| w[0].rung < w[1].rung);
        if ev.ladder.len() > usize::from(K_MAX) || !increasing {
            return Err(EvidenceDefect::Ladder);
        }
        Ok(())
    }

    /// A tolerance refused where it was formed: the value given and the
    /// range, no entity.
    pub fn of_tolerance(refused: ToleranceOutOfRange, op: OpCtx) -> Failure {
        let range = refused.range();
        let mut ev = Evidence::new(op, Site::None);
        let item = Item::new(
            None,
            ItemRole::Value,
            "the tolerance given and its range (mm)",
        )
        .with_values(
            [refused.given(), *range.start(), *range.end()]
                .into_iter()
                .map(Bounded::exact),
        );
        ev.push_item(item, &mut Allowance::default());
        Failure::new(refused.into(), ev, "tolerance")
    }

    /// `error`, the operation having run out ([`KernelError::OutOfBudget`]
    /// with its profile): at loop `row` of the profile's stage (a budget
    /// site), the exhausting key's entities named through `leaves`, the
    /// level it stopped at. `None` for another error.
    pub fn out_of_budget(
        error: KernelError,
        op: OpCtx,
        row: BudgetRow,
        leaves: &[Leaf<'_>],
    ) -> Option<Failure> {
        let KernelError::OutOfBudget { profile, .. } = &error else {
            return None;
        };
        let stage = profile.stopped.stage;
        let site = Site::Budget { stage, row };
        let ev = budget_evidence(profile, op, site, leaves, &mut Allowance::default());
        Some(Failure::new(error, ev, stage))
    }

    /// The failure a check's error `err` is for a body of role `role`, as
    /// leaf `leaf` of operation `op` at rung `rung`; `None` where it is
    /// no failure (an accuracy item, a signal to the ladder). Its
    /// evidence: the failing item, by name (or the faces of a failing
    /// shell, region or contact record); the faces round it cut out with
    /// its invariant left out; the site of a defect of the kernel's own
    /// (a result's defect by its invariant, a result face too tight to
    /// embed as invariant 9's validity); when the check ran out, its
    /// stage and the check's loop, or the invariant whose certificate
    /// reached the highest precision.
    pub fn of_check(
        err: &CheckError,
        role: Role,
        leaf: &Leaf<'_>,
        op: OpCtx,
        rung: u8,
    ) -> Option<Failure> {
        Failure::of_check_in(err, role, leaf, op, rung, &mut Allowance::default())
    }

    /// [`Failure::of_check`] from `allowance`.
    pub(crate) fn of_check_in(
        err: &CheckError,
        role: Role,
        leaf: &Leaf<'_>,
        op: OpCtx,
        rung: u8,
        allowance: &mut Allowance,
    ) -> Option<Failure> {
        let error = match err.outcome(role) {
            Outcome::Error(e) => e,
            Outcome::Accuracy | Outcome::LadderSignal => return None,
        };
        let f = match err {
            CheckError::OutOfBudget {
                inv,
                error: KernelError::OutOfBudget { profile, .. },
            } => {
                let stage = profile.stopped.stage;
                let site = match (profile.limit, inv) {
                    (Limit::Cap(Cap::PrecisionBits), Some(inv)) => Site::Validity { inv: *inv },
                    _ => Site::Budget {
                        stage,
                        row: BudgetRow::Check,
                    },
                };
                let ev = budget_evidence(profile, op, site, &[*leaf], allowance);
                return Some(Failure::new(error, ev, stage));
            }
            // Running out is always out of budget; any other error is
            // passed on bare.
            CheckError::OutOfBudget { .. } => {
                return Some(Failure::new(error, Evidence::new(op, Site::None), "check"));
            }
            CheckError::Failed(f) => f,
        };
        let site = match &error {
            KernelError::Internal(Internal::Inconsistent(Inconsistency::ResultDefect)) => {
                Site::Internal(InternalSite::ResultDefect { inv: f.inv })
            }
            KernelError::Internal(Internal::Inconsistent(Inconsistency::ClusterRuleGap)) => {
                Site::Validity { inv: f.inv }
            }
            _ => Site::None,
        };
        let mut ev = Evidence::new(op, site);
        ev.rung = rung;
        let text = format!(
            "invariant {}, check {}: {:?}",
            f.inv.number(),
            f.check,
            f.finding
        );
        let body = leaf.body;
        let faces = item_faces(body, f.item);
        match entity(f.item) {
            Some(id) => {
                let item = Item::new(Some(InputRef::of(leaf, id)), ItemRole::Failing, text);
                ev.push_item(item, allowance);
            }
            None => {
                for &face in &faces {
                    let subject = Some(InputRef::of(leaf, Entity::Face(face)));
                    ev.push_item(Item::new(subject, ItemRole::Witness, &*text), allowance);
                }
            }
        }
        if let (Finding::EmptyWindow, CheckItem::Edge(e)) = (f.finding, f.item) {
            window_evidence(&mut ev, leaf, e, &faces, allowance);
        }
        if f.finding != Finding::Unbuilt {
            ev.geometry.add(leaf, &faces, Some(f.inv), allowance);
        }
        Some(Failure::new(error, ev, "check"))
    }
}

/// The units an empty window's evidence may spend recomputing it, of the
/// pass's allowance.
const WINDOW_EVIDENCE: u64 = super::EVIDENCE_WORK / 2;

/// The evidence of edge `e`'s window certified empty: its two faces by
/// name (a constructed body's name the construction's inputs, the
/// profile curves or the path whose faces meet there), and the window
/// recomputed on [`WINDOW_EVIDENCE`] of the allowance: a certified lower
/// bound of `ρ_min` and an upper bound of `ρ_graph` (left out where it
/// doesn't settle within them).
fn window_evidence(
    ev: &mut Evidence,
    leaf: &Leaf<'_>,
    e: EdgeId,
    faces: &[FaceId],
    allowance: &mut Allowance,
) {
    for &face in faces {
        let subject = Some(InputRef::of(leaf, Entity::Face(face)));
        let item = Item::new(subject, ItemRole::Partner, "a face across the edge");
        ev.push_item(item, allowance);
    }
    if !allowance.afford(WINDOW_EVIDENCE) {
        return;
    }
    if let Some((_, [rho_min, rho_graph])) =
        crate::topo::adjacent::empty_window(leaf.body, e, WINDOW_EVIDENCE)
    {
        let item = Item::new(
            None,
            ItemRole::Value,
            "the edge's window: a lower bound of rho_min, an upper bound of rho_graph (mm)",
        )
        .with_values([rho_min, rho_graph].map(Bounded::exact));
        ev.push_item(item, allowance);
    }
}

/// The evidence of running out per `profile` at `site`: the exhausting
/// key's entities named through `leaves`, the level it stopped at.
fn budget_evidence(
    profile: &BudgetProfile,
    op: OpCtx,
    site: Site,
    leaves: &[Leaf<'_>],
    allowance: &mut Allowance,
) -> Evidence {
    let stopped = &profile.stopped;
    let mut ev = Evidence::new(op, site);
    ev.rung = stopped.rung;
    ev.level = stopped.level();
    let text = format!(
        "pass {}, item {} of stage {}",
        stopped.at.pass, stopped.at.item, stopped.stage
    );
    for input in stopped.at.name.inputs(leaves) {
        ev.push_item(
            Item::new(Some(input), ItemRole::Exhausting, &*text),
            allowance,
        );
    }
    ev
}

/// The entity a check's item is, if it is one an [`InputRef`] names.
fn entity(item: CheckItem) -> Option<Entity> {
    match item {
        CheckItem::Vert(v) => Some(Entity::Vert(v)),
        CheckItem::Edge(e) => Some(Entity::Edge(e)),
        CheckItem::Coedge(c) => Some(Entity::Coedge(c)),
        CheckItem::Loop(l) => Some(Entity::Loop(l)),
        CheckItem::Face(f) => Some(Entity::Face(f)),
        CheckItem::Body | CheckItem::Shell(_) | CheckItem::Region(_) | CheckItem::Contact(_) => {
            None
        }
    }
}

/// The faces round a check's item, read guarded: a face itself; a loop's
/// face; a coedge's face and its pair's; the faces using an edge or a
/// vertex; a shell's faces, a region's shells'; the faces of a contact
/// record's members. None for the body.
fn item_faces(b: &Brep, item: CheckItem) -> Vec<FaceId> {
    let coedges = 0..b.coedge_count() as u32;
    let face_of = |c: CoedgeId| b.coedges.face.get(c.index()).copied();
    let edge_faces = |e: EdgeId| -> Vec<FaceId> {
        coedges
            .clone()
            .map(CoedgeId)
            .filter(|c| b.coedges.edge[c.index()] == e)
            .filter_map(face_of)
            .collect()
    };
    let vert_faces = |v| -> Vec<FaceId> {
        coedges
            .clone()
            .map(CoedgeId)
            .filter(|c| {
                let e = b.coedges.edge[c.index()].index();
                (b.edges.v_start.get(e) == Some(&v)) || (b.edges.v_end.get(e) == Some(&v))
            })
            .filter_map(face_of)
            .collect()
    };
    let shell_faces = |s: crate::topo::brep::ShellId| -> Vec<FaceId> {
        (b.shells.sides.get(s.index()))
            .map(|sides| sides.iter().map(|&(f, _)| f).collect())
            .unwrap_or_default()
    };
    match item {
        CheckItem::Body => Vec::new(),
        CheckItem::Face(f) => vec![f],
        CheckItem::Loop(l) => b.loops.face.get(l.index()).copied().into_iter().collect(),
        CheckItem::Coedge(c) => {
            let pair = b.coedges.pair.get(c.index()).copied();
            [Some(c), pair]
                .into_iter()
                .flatten()
                .filter_map(face_of)
                .collect()
        }
        CheckItem::Edge(e) => edge_faces(e),
        CheckItem::Vert(v) => vert_faces(v),
        CheckItem::Shell(s) => shell_faces(s),
        CheckItem::Region(r) => {
            let outer = b.regions.outer.get(r.index()).copied();
            let islands = b
                .regions
                .islands
                .get(r.index())
                .cloned()
                .unwrap_or_default();
            (outer.into_iter().chain(islands))
                .flat_map(shell_faces)
                .collect()
        }
        CheckItem::Contact(i) => {
            let Some(c) = b.contacts.get(i as usize) else {
                return Vec::new();
            };
            c.members
                .iter()
                .flatten()
                .flat_map(|m| match *m {
                    Member::Vert(v) => vert_faces(v),
                    Member::Edge(e) | Member::EdgeAt { edge: e, .. } => edge_faces(e),
                    Member::FaceAt { face, .. } => vec![face],
                })
                .collect()
        }
    }
}

impl std::fmt::Display for Failure {
    /// The error's words; the evidence is for drawing and replaying.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for Failure {}

#[cfg(test)]
mod tests;
