//! Failures and their evidence: sites fitting their errors, replays by
//! the error, the caps and the allowance; a body's check failing by role
//! on the cube; a refused tolerance; a cluster rule gap; and warnings'
//! evidence kept on the status without changing it.

use super::*;
use crate::boolean::snap::escalate::{Cert, Event, Failed, Gap, RuleGap};
use crate::evidence::budget_profile::WarningEvidence;
use crate::evidence::tests::{cube, key};
use crate::infra::budget::Budget;
use crate::infra::ops::{FeatureId, Status, Tolerance, Warning, WarningKind};
use crate::names::Name;
use crate::topo::brep::{FaceId, ShellId};
use crate::topo::check::CheckFailure;
use varde_decide::num::storage::OpStart;

fn op() -> OpCtx {
    OpCtx::new(FeatureId(9))
}

fn internal(c: Inconsistency) -> KernelError {
    KernelError::Internal(Internal::Inconsistent(c))
}

/// A check run out of budget on the cube: while forming the order.
fn ran_out() -> CheckError {
    cube().check(&Budget::new(3)).unwrap_err()
}

fn out_of_budget() -> KernelError {
    match ran_out() {
        CheckError::OutOfBudget { error, .. } => error,
        e => panic!("{e:?}"),
    }
}

/// `error` with its cause set to `cause`.
fn with_cause(error: &KernelError, cause: Cause) -> KernelError {
    match error.clone() {
        KernelError::OutOfBudget { profile, .. } => KernelError::OutOfBudget { cause, profile },
        e => e,
    }
}

#[test]
fn sites_fit_their_errors_only() {
    let oob = out_of_budget();
    let [work, precision, size] =
        [Cause::Work, Cause::Precision, Cause::SizeLimit].map(|c| with_cause(&oob, c));
    let invalid = KernelError::Invalid(Invalid::Check { inv: 3 });
    let budget = Site::Budget {
        stage: "broad",
        row: BudgetRow::Broad,
    };
    let validity = |inv| Site::Validity { inv };
    let rows: [(Site, &[&KernelError]); 9] = [
        (
            Site::None,
            &[&invalid, &KernelError::Unrepresentable(Storage::Repair)],
        ),
        (budget, &[&work, &precision, &size]),
        (Site::LoopIn, &[&precision]),
        (
            Site::Snap {
                kind: SnapSite::RimGap,
            },
            &[&precision],
        ),
        (validity(Inv::Trims), &[&precision]),
        (
            validity(Inv::Embedding),
            &[&precision, &internal(Inconsistency::ClusterRuleGap)],
        ),
        (Site::LabelAudit(AuditSite::Uncertifiable), &[&size]),
        (
            Site::Internal(InternalSite::Cluster),
            &[&internal(Inconsistency::ClusterRuleGap)],
        ),
        (
            Site::Internal(InternalSite::ResultDefect { inv: Inv::Joins }),
            &[&internal(Inconsistency::ResultDefect)],
        ),
    ];
    let errors = [
        invalid.clone(),
        work.clone(),
        precision.clone(),
        size.clone(),
        internal(Inconsistency::ClusterRuleGap),
        internal(Inconsistency::ResultDefect),
        internal(Inconsistency::ExactTie),
        KernelError::Unrepresentable(Storage::Repair),
    ];
    for (site, fit) in rows {
        for e in &errors {
            assert_eq!(site.fits(e), fit.contains(&e), "{site:?} {e:?}");
        }
    }
    for s in [
        InternalSite::ExactTie,
        InternalSite::Proxy,
        InternalSite::SosGuard,
    ] {
        assert!(Site::Internal(s).fits(&internal(s.class())));
        assert!(!Site::None.fits(&internal(s.class())));
    }
}

#[test]
fn replays_follow_the_error() {
    let precision = with_cause(&out_of_budget(), Cause::Precision);
    let cases = [
        (
            KernelError::Invalid(Invalid::Check { inv: 1 }),
            Site::None,
            ReplayKind::Local,
        ),
        (
            KernelError::Invalid(Invalid::SelfIntersection),
            Site::None,
            ReplayKind::Local,
        ),
        (
            KernelError::Invalid(Invalid::NotClosed),
            Site::None,
            ReplayKind::None,
        ),
        (
            KernelError::Invalid(Invalid::Tolerance),
            Site::None,
            ReplayKind::None,
        ),
        (precision.clone(), Site::CurvedSign, ReplayKind::Local),
        (precision.clone(), Site::Leaf, ReplayKind::Local),
        (precision.clone(), Site::LoopIn, ReplayKind::Full),
        (out_of_budget(), Site::None, ReplayKind::Full),
        (internal(Inconsistency::Proxy), Site::None, ReplayKind::Full),
        (
            KernelError::Unrepresentable(Storage::OutOfRange),
            Site::None,
            ReplayKind::None,
        ),
        (
            KernelError::Unrepresentable(Storage::Repair),
            Site::None,
            ReplayKind::Full,
        ),
        (
            KernelError::NotImplemented("x"),
            Site::None,
            ReplayKind::None,
        ),
    ];
    for (e, site, want) in cases {
        assert_eq!(ReplayKind::of(&e, &site), want, "{e:?} {site:?}");
    }
}

#[test]
fn evidence_holds_together_or_says_how_not() {
    let ok = || {
        let mut ev = Evidence::new(op(), Site::None);
        ev.push_item(
            Item::new(None, ItemRole::Value, "v"),
            &mut Allowance::default(),
        );
        Failure::new(KernelError::Invalid(Invalid::NotClosed), ev, "x")
    };
    assert_eq!(ok().check(), Ok(()));
    assert_eq!(ok().evidence.replay, Replay::None);
    let mut f = ok();
    f.evidence.site = Site::Leaf;
    assert_eq!(f.check(), Err(EvidenceDefect::Site));
    let mut f = ok();
    f.evidence.items = vec![Item::new(None, ItemRole::Value, ""); MAX_EVIDENCE_ITEMS + 1];
    assert_eq!(f.check(), Err(EvidenceDefect::Items));
    let mut f = ok();
    f.evidence.replay = Replay::Local { stage: "x" };
    assert_eq!(f.check(), Err(EvidenceDefect::Replay));
    let step = |rung| LadderStep {
        rung,
        tau: 0.0,
        key_text: String::new(),
    };
    let mut f = ok();
    f.evidence.ladder = vec![step(1), step(1)];
    assert_eq!(f.check(), Err(EvidenceDefect::Ladder));
    f.evidence.ladder = (0..=K_MAX).map(step).collect();
    assert_eq!(f.check(), Err(EvidenceDefect::Ladder));
    f.evidence.ladder.pop();
    assert_eq!(f.check(), Ok(()));
}

#[test]
fn items_past_the_cap_are_counted_and_past_the_allowance_named_only() {
    let value = Item::new(None, ItemRole::Value, "v").with_values([Bounded::exact(1.0); 3]);
    let mut ev = Evidence::new(op(), Site::None);
    // Four units an item: three items' worth.
    let mut a = Allowance::new(12);
    for _ in 0..MAX_EVIDENCE_ITEMS + 2 {
        ev.push_item(value.clone(), &mut a);
    }
    assert_eq!(ev.items.len(), MAX_EVIDENCE_ITEMS);
    assert!(ev.items[..3].iter().all(|i| i.values.len() == 3));
    assert!(ev.items[3..].iter().all(|i| i.values.is_empty()));
    assert!(ev.items.iter().all(|i| i.key_text == "v"));
    assert_eq!(ev.truncated as usize, MAX_EVIDENCE_ITEMS - 3 + 2);
}

/// A check failure at the cube's edge `e`, invariant `inv`.
fn failed(inv: Inv, finding: Finding, item: CheckItem) -> CheckError {
    CheckError::Failed(CheckFailure {
        inv,
        item,
        check: 2,
        finding,
    })
}

#[test]
fn an_operands_defect_is_invalid_with_its_entity_and_faces_cut_out() {
    let b = cube();
    let leaf = Leaf::new(1, &b);
    let e = EdgeId(0);
    let err = failed(Inv::Joins, Finding::Defect, CheckItem::Edge(e));
    let f = Failure::of_check(&err, Role::Operand, &leaf, op(), 0).unwrap();
    assert_eq!(f.error, KernelError::Invalid(Invalid::Check { inv: 2 }));
    assert_eq!(f.check(), Ok(()));
    let ev = &f.evidence;
    assert_eq!((ev.op, ev.site, ev.rung), (op(), Site::None, 0));
    assert_eq!(ev.replay, Replay::Local { stage: "check" });
    assert_eq!(ev.items.len(), 1);
    let subject = ev.items[0].subject.as_ref().unwrap();
    assert_eq!((subject.leaf, subject.id), (1, Entity::Edge(e)));
    assert!(matches!(subject.name, Some(Name::Edge(_))));
    assert_eq!(ev.items[0].role, ItemRole::Failing);
    assert!(ev.items[0].key_text.contains("invariant 2"));
    // The edge's two faces, as one sheet, for invariant 2.
    let pieces = &ev.geometry.pieces;
    assert_eq!(pieces.len(), 1);
    assert_eq!((pieces[0].leaf, pieces[0].except), (1, Some(Inv::Joins)));
    assert_eq!(pieces[0].faces.len(), 2);
    for &face in &pieces[0].faces {
        assert!((b.loop_coedges(b.first_loop(face))).any(|c| b.edge(c) == e));
    }
    // A crossing on an operand is a self-intersection.
    let err = failed(
        Inv::Embedding,
        Finding::Crossing,
        CheckItem::Face(FaceId(4)),
    );
    let f = Failure::of_check(&err, Role::Operand, &leaf, op(), 0).unwrap();
    assert_eq!(f.error, KernelError::Invalid(Invalid::SelfIntersection));
    assert_eq!(f.evidence.geometry.pieces[0].faces, vec![FaceId(4)]);
}

#[test]
fn a_constructed_or_result_defect_is_the_kernels_with_its_site() {
    let b = cube();
    let leaf = Leaf::new(0, &b);
    let v = crate::topo::brep::VertId(5);
    let err = failed(Inv::Forms, Finding::Defect, CheckItem::Vert(v));
    for role in [Role::Constructed, Role::Result] {
        let f = Failure::of_check(&err, role, &leaf, op(), 2).unwrap();
        assert_eq!(f.error, internal(Inconsistency::ResultDefect));
        assert_eq!(
            f.evidence.site,
            Site::Internal(InternalSite::ResultDefect { inv: Inv::Forms })
        );
        assert_eq!(f.evidence.rung, 2);
        assert_eq!(f.evidence.replay, Replay::Full(FullReplay::default()));
        // The corner's three faces.
        assert_eq!(f.evidence.geometry.face_count(), 3);
        assert_eq!(f.check(), Ok(()));
    }
    // A result face too tight to embed: a cluster rule gap at
    // invariant 9's validity; a constructed one, the construction's
    // self-intersection.
    let err = failed(
        Inv::Embedding,
        Finding::EmptyWindow,
        CheckItem::Edge(EdgeId(3)),
    );
    let f = Failure::of_check(&err, Role::Result, &leaf, op(), 0).unwrap();
    assert_eq!(f.error, internal(Inconsistency::ClusterRuleGap));
    assert_eq!(
        f.evidence.site,
        Site::Validity {
            inv: Inv::Embedding
        }
    );
    let f = Failure::of_check(&err, Role::Constructed, &leaf, op(), 0).unwrap();
    assert_eq!(f.error, KernelError::Invalid(Invalid::SelfIntersection));
    // A seam over its target and a result's orientation are no failure.
    let seam = failed(Inv::Seams, Finding::Defect, CheckItem::Edge(EdgeId(0)));
    let turned = failed(
        Inv::Orientation,
        Finding::Defect,
        CheckItem::Shell(ShellId(0)),
    );
    assert!(Failure::of_check(&seam, Role::Result, &leaf, op(), 0).is_none());
    assert!(Failure::of_check(&turned, Role::Result, &leaf, op(), 0).is_none());
}

#[test]
fn a_failing_shell_is_shown_by_its_faces_and_an_unbuilt_part_by_none() {
    let b = cube();
    let leaf = Leaf::new(0, &b);
    let err = failed(Inv::Topology, Finding::Defect, CheckItem::Shell(ShellId(1)));
    let f = Failure::of_check(&err, Role::Operand, &leaf, op(), 0).unwrap();
    let ev = &f.evidence;
    assert_eq!(ev.items.len(), 6);
    let names: Vec<_> = (ev.items.iter())
        .map(|i| i.subject.as_ref().unwrap().name.clone().unwrap())
        .collect();
    let want: Vec<_> = (0..6).map(|j| Name::Face(key(j))).collect();
    assert_eq!(names, want);
    assert!(ev.items.iter().all(|i| i.role == ItemRole::Witness));
    assert_eq!(ev.geometry.face_count(), 6);
    let err = failed(Inv::Forms, Finding::Unbuilt, CheckItem::Face(FaceId(0)));
    let f = Failure::of_check(&err, Role::Operand, &leaf, op(), 0).unwrap();
    assert!(f.evidence.geometry.is_empty());
    assert_eq!(f.check(), Ok(()));
}

#[test]
fn a_check_running_out_names_its_stage() {
    let b = cube();
    let leaf = Leaf::new(0, &b);
    let err = ran_out();
    let f = Failure::of_check(&err, Role::Operand, &leaf, op(), 0).unwrap();
    assert!(matches!(f.error, KernelError::OutOfBudget { .. }));
    let KernelError::OutOfBudget { profile, .. } = &f.error else {
        unreachable!()
    };
    assert_eq!(
        f.evidence.site,
        Site::Budget {
            stage: profile.stopped.stage,
            row: BudgetRow::Check
        }
    );
    assert_eq!(f.evidence.replay, Replay::Full(FullReplay::default()));
    assert_eq!(f.check(), Ok(()));
    let g = Failure::out_of_budget(f.error.clone(), op(), BudgetRow::Check, &[leaf]).unwrap();
    assert_eq!(g, f);
    assert!(
        Failure::out_of_budget(internal(Inconsistency::Proxy), op(), BudgetRow::Check, &[])
            .is_none()
    );
}

/// The allowance changes the evidence only: the error, the site and the
/// items' subjects are the same at none and at no limit.
#[test]
fn the_allowance_changes_only_the_evidence() {
    let b = cube();
    let leaf = Leaf::new(0, &b);
    let err = failed(Inv::Domain, Finding::Defect, CheckItem::Face(FaceId(2)));
    let at = |units| {
        Failure::of_check_in(
            &err,
            Role::Operand,
            &leaf,
            op(),
            0,
            &mut Allowance::new(units),
        )
        .unwrap()
    };
    let (none, all) = (at(0), at(u64::MAX));
    assert_eq!(none.error, all.error);
    assert_eq!(none.evidence.site, all.evidence.site);
    let subjects =
        |f: &Failure| -> Vec<_> { f.evidence.items.iter().map(|i| i.subject.clone()).collect() };
    assert_eq!(subjects(&none), subjects(&all));
    assert!(none.evidence.geometry.is_empty());
    assert_eq!(all.evidence.geometry.face_count(), 1);
    // The item degraded to its name, the face left out of the snippet.
    assert_eq!(none.evidence.truncated, 1);
    assert_eq!(none.evidence.geometry.truncated, 1);
    assert_eq!(all.evidence.truncated + all.evidence.geometry.truncated, 0);
}

#[test]
fn a_refused_tolerance_gives_its_value_and_range() {
    let refused = Tolerance::new(2.0).unwrap_err();
    let f = Failure::of_tolerance(refused, op());
    assert_eq!(f.error, KernelError::Invalid(Invalid::Tolerance));
    assert_eq!(f.check(), Ok(()));
    let values: Vec<f64> = f.evidence.items[0].values.iter().map(|b| b.v).collect();
    assert_eq!(values, vec![2.0, Tolerance::MIN, Tolerance::MAX]);
    assert_eq!(f.to_string(), f.error.to_string());
}

#[test]
fn a_cluster_rule_gap_is_the_clusters_site_with_its_steps_as_text() {
    let fail = |b_k, quantity| Event::Failed {
        b_k,
        failed: Failed {
            cert: Cert::Rim,
            key: (3u32, 'r'),
            quantity,
            pins: Vec::new(),
        },
    };
    let gap: RuleGap<(u32, char), u8> = RuleGap {
        gap: Gap::Stop,
        events: vec![
            fail(0.5, -1.0),
            Event::Halved { b_k: 0.25 },
            fail(0.25, -1.0),
        ],
    };
    let f = ClusterEvidence::failure(&gap, op(), 1, 1e-6, &mut Allowance::default());
    assert_eq!(f.error, internal(Inconsistency::ClusterRuleGap));
    assert_eq!(f.evidence.site, Site::Internal(InternalSite::Cluster));
    assert_eq!((f.evidence.rung, f.evidence.tau), (1, 1e-6));
    assert_eq!(f.check(), Ok(()));
    let Some(Detail::Cluster(c)) = &f.evidence.detail else {
        panic!("{:?}", f.evidence.detail)
    };
    assert_eq!(c.gap, Gap::Stop);
    assert_eq!(c.bounds, vec![0.5, 0.25]);
    assert_eq!(c.steps.len(), 3);
    let Event::Failed { failed, .. } = &c.steps[2].event else {
        panic!()
    };
    assert_eq!(failed.key, "(3, 'r')");
    // Past the allowance, steps dropped and counted on the evidence too.
    let f = ClusterEvidence::failure(&gap, op(), 1, 0.0, &mut Allowance::new(0));
    assert_eq!(f.evidence.truncated, 4);
}

#[test]
fn warnings_evidence_is_kept_on_the_status_which_compares_the_same() {
    let start = OpStart {
        max_coord: 1.0,
        rho_op: 0.0,
    };
    let mut status = Status::new(Tolerance::DEFAULT, &start);
    for i in 0..3 {
        status.warn(Warning::new(WarningKind::StorageRounding, f64::from(i) * 1e-9).unwrap());
    }
    let mut kept = Vec::new();
    for units in [0, 2, u64::MAX] {
        let mut s = status.clone();
        let mut a = Allowance::new(units);
        let ev = WarningEvidence::build(
            &s,
            &mut a,
            |_, _| 1,
            |i, w| {
                let mut e = Evidence::new(op(), Site::None);
                let item = Item::new(None, ItemRole::Value, format!("warning {i}"))
                    .with_values([Bounded::exact(w.bound())]);
                e.push_item(item, &mut Allowance::default());
                e
            },
        );
        s.attach_evidence(ev);
        assert!(s.same_outcome(&status));
        assert_eq!(s.warnings(), status.warnings());
        let ev = s.warning_evidence().unwrap();
        assert_eq!(ev.items.len(), 3);
        kept.push(ev.items.iter().filter(|i| i.is_some()).count());
    }
    assert_eq!(kept, vec![0, 2, 3]);
    assert!(status.warning_evidence().is_none());
}

/// A repair's evidence names its site's entities (the result's under the
/// result's leaf, the operands') with the repair's detail and no site;
/// as a failure it is a storage refusal replayed in full; the allowance
/// only degrades its items, and a `Repair` warning's evidence rides on
/// the status without changing it.
#[test]
fn a_repair_names_its_site() {
    let b = cube();
    let leaf = Leaf::new(0, &b);
    let result = InputRef {
        leaf: Leaf::RESULT,
        ..InputRef::of(&leaf, Entity::Vert(crate::topo::brep::VertId(2)))
    };
    let site = vec![
        result,
        InputRef::of(&leaf, Entity::Edge(crate::topo::brep::EdgeId(1))),
    ];
    let detail = RepairDetail {
        kind: Repair::Reorder,
        order: "x(v2) < x(e1) unrounded".into(),
        chosen: vec![1.0, 1.0 + f64::EPSILON],
        bound: 2e-16,
        rounds: None,
    };
    let ev = Evidence::of_repair(
        op(),
        site.clone(),
        detail.clone(),
        &mut Allowance::default(),
    );
    assert_eq!(ev.site, Site::None);
    assert_eq!(ev.items.len(), 2);
    assert!(ev.items.iter().all(|i| i.role == ItemRole::Repaired));
    assert_eq!(
        ev.items[0].subject.as_ref().map(|s| s.leaf),
        Some(Leaf::RESULT)
    );
    assert_eq!(ev.detail, Some(Detail::Repair(Box::new(detail.clone()))));
    let failed = Failure::of_repair(
        op(),
        site.clone(),
        RepairDetail {
            rounds: Some(8),
            ..detail.clone()
        },
        &mut Allowance::default(),
    );
    assert_eq!(failed.error, KernelError::Unrepresentable(Storage::Repair));
    assert_eq!(failed.check(), Ok(()));
    assert_eq!(failed.evidence.replay.kind(), ReplayKind::Full);
    let poor = Evidence::of_repair(op(), site, detail.clone(), &mut Allowance::new(0));
    assert_eq!(poor.truncated, 2);
    let start = OpStart {
        max_coord: 2.0,
        rho_op: 0.0,
    };
    let mut status = Status::new(Tolerance::DEFAULT, &start);
    status.warn(Warning::new(WarningKind::Repair(Repair::Reorder), detail.bound).unwrap());
    let mut s = status.clone();
    let ev = WarningEvidence::build(
        &s,
        &mut Allowance::default(),
        |_, _| 1,
        |_, _| Evidence::of_repair(op(), Vec::new(), detail.clone(), &mut Allowance::default()),
    );
    s.attach_evidence(ev);
    assert!(s.same_outcome(&status));
}
