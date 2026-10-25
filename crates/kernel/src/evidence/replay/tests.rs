//! Full replays: a result's defect (`Internal`) and a check running out
//! reproduce bit for bit on their captured operands and on operands
//! handed back, on any number of threads; operands that aren't the
//! bodies failed on, failures replayed locally or built outside an
//! entry, and an operation passing on replay are refused; digests tell
//! bodies apart by their stored bits.

use super::*;
use crate::build::polyhedron::build;
use crate::evidence::failure::{InternalSite, Site};
use crate::evidence::tests::{CUBE, CUBE_FACES};
use crate::infra::error::{Inconsistency, Internal, KernelError};
use crate::infra::ops::{FeatureId, Tolerance};
#[cfg(not(target_arch = "wasm32"))]
use crate::infra::par::assert_deterministic;
use crate::topo::brep::EdgeId;

/// On the web, with no thread pool: `f` once.
#[cfg(target_arch = "wasm32")]
fn assert_deterministic<R>(f: impl Fn() -> R) -> R {
    f()
}

fn ctx() -> OpCtx {
    OpCtx::new(FeatureId(7))
}

/// The invariant the check leaves out: faces apart (9) are not certified
/// yet on any body.
const APART: Option<Inv> = Some(Inv::Embedding);

/// The unit cube from its twelve triangles: a body passing the check
/// (but for [`APART`]).
fn cube() -> Brep {
    let tris: Vec<[usize; 3]> = CUBE_FACES
        .iter()
        .flat_map(|f| [[f[0], f[1], f[2]], [f[0], f[2], f[3]]].map(|t| t.map(|i| i as usize)))
        .collect();
    let ctx = OpCtx::new(FeatureId(1));
    let mut work = crate::infra::budget::Work::new(&Budget::DEFAULT);
    build(&CUBE, &tris, ctx, Tolerance::DEFAULT, &mut work).unwrap()
}

/// The cube with edge 0's range a step short of its curve's end: a
/// defect of invariant 2, as an operation's bug would leave it.
fn faulty() -> Arc<Brep> {
    let mut b = cube();
    let t = &mut b.edges.t_range[EdgeId(0).index()][1];
    *t = t.next_down();
    Arc::new(b)
}

/// `body`'s check as a result, on the default budget, in `mode`.
fn as_result(body: &Arc<Brep>, mode: EvidenceMode) -> Result<Checked, Failure> {
    check_body(body, Role::Result, APART, ctx(), Budget::DEFAULT, mode)
}

/// `a` and `b` the same to the bit: equal, and their `Debug` texts too
/// (which tell `-0.0` from `0.0`).
fn same(a: &Failure, b: &Failure) {
    assert_eq!(a, b);
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
}

fn full(f: &Failure) -> &FullReplay {
    match &f.evidence.replay {
        Replay::Full(full) => full,
        r => panic!("{r:?}"),
    }
}

#[test]
fn the_valid_cube_passes_as_any_role() {
    let b = Arc::new(cube());
    for role in [Role::Operand, Role::Constructed, Role::Result] {
        let got = check_body(
            &b,
            role,
            APART,
            ctx(),
            Budget::DEFAULT,
            EvidenceMode::Capture,
        );
        assert_eq!(got, Ok(Checked::Passed));
    }
}

/// A result's defect is the kernel's own (`Internal`), replayed in full:
/// captured, it reproduces bit for bit, on one thread and on eight.
#[test]
fn a_results_defect_reproduces_bit_for_bit() {
    let body = faulty();
    let f = as_result(&body, EvidenceMode::Capture).unwrap_err();
    assert_eq!(
        f.error,
        KernelError::Internal(Internal::Inconsistent(Inconsistency::ResultDefect))
    );
    assert_eq!(
        f.evidence.site,
        Site::Internal(InternalSite::ResultDefect { inv: Inv::Joins })
    );
    assert_eq!(f.check(), Ok(()));
    let kept = full(&f);
    let want = Operation {
        ctx: ctx(),
        params: Params::Check {
            role: Role::Result,
            except: APART,
        },
        budget: Budget::DEFAULT,
    };
    assert_eq!(kept.operation.as_ref(), Some(&want));
    assert_eq!(kept.digests, vec![digest(&body)]);
    let held = kept.captured.operands().unwrap();
    assert!(held.len() == 1 && Arc::ptr_eq(&held[0], &body));
    let again = assert_deterministic(|| replay(&f).unwrap());
    same(&again, &f);
    // Replayed twice over: the replay's own failure replays the same.
    same(&replay(&again).unwrap(), &f);
}

/// Without a capture the failure keeps the operation and the digests;
/// the caller hands the operands back, a copy of the same body doing as
/// well as the handle.
#[test]
fn without_a_capture_the_operands_are_handed_back() {
    let body = faulty();
    let f = as_result(&body, EvidenceMode::Digests).unwrap_err();
    let kept = full(&f);
    assert!(kept.captured.operands().is_none());
    assert_eq!(kept.digests, vec![digest(&body)]);
    assert_eq!(replay(&f), Err(ReplayError::NotCaptured));
    let copy = Arc::new(Brep::clone(&body));
    same(&replay_with(&f, &[copy]).unwrap(), &f);
    // Not the body it failed on, or not as many operands.
    let valid = Arc::new(cube());
    assert_eq!(
        replay_with(&f, std::slice::from_ref(&valid)),
        Err(ReplayError::Digest { operand: 0 })
    );
    assert_eq!(
        replay_with(&f, &[body.clone(), body]),
        Err(ReplayError::Operands { given: 2 })
    );
    assert_eq!(
        replay_with(&f, &[]),
        Err(ReplayError::Operands { given: 0 })
    );
}

/// Running out is replayed in full too: the same error, profile and
/// evidence on the same budget.
#[test]
fn a_check_running_out_reproduces_with_its_profile() {
    let body = Arc::new(cube());
    for budget in [3, 40] {
        let f = check_body(
            &body,
            Role::Operand,
            APART,
            ctx(),
            Budget::new(budget),
            EvidenceMode::Capture,
        )
        .unwrap_err();
        assert!(matches!(f.error, KernelError::OutOfBudget { .. }), "{f:?}");
        assert_eq!(
            full(&f).operation.as_ref().unwrap().budget,
            Budget::new(budget)
        );
        same(&assert_deterministic(|| replay(&f).unwrap()), &f);
    }
}

/// An operand's defect is replayed locally, so not in full; a failure
/// built outside an operation's entry names no operation; a recorded
/// operation that passes on its operands is a replay passing.
#[test]
fn replays_not_in_full_or_with_nothing_to_run_are_refused() {
    let body = faulty();
    let f = check_body(
        &body,
        Role::Operand,
        APART,
        ctx(),
        Budget::DEFAULT,
        EvidenceMode::Capture,
    )
    .unwrap_err();
    assert_eq!(f.evidence.replay.kind(), crate::evidence::ReplayKind::Local);
    assert_eq!(replay(&f), Err(ReplayError::NotFull));
    let err = body
        .check_except(Inv::Embedding, &Budget::DEFAULT)
        .unwrap_err();
    let bare = Failure::of_check(&err, Role::Result, &Leaf::new(0, &body), ctx(), 0).unwrap();
    assert_eq!(*full(&bare), FullReplay::default());
    assert_eq!(replay(&bare), Err(ReplayError::NoOperation));
    // An operation recorded on the valid cube passes there.
    let valid = Arc::new(cube());
    let mut f = as_result(&body, EvidenceMode::Capture).unwrap_err();
    if let Replay::Full(kept) = &mut f.evidence.replay {
        kept.digests = vec![digest(&valid)];
        kept.captured = Captured(Some(Arc::new([valid])));
    }
    assert_eq!(replay(&f), Err(ReplayError::Passed));
}

/// Digests are of the stored bits: a copy has its body's, a value one
/// step off or a zero's sign flipped another; a text's digest is never
/// the empty text's.
#[test]
fn digests_tell_bodies_by_their_bits() {
    let b = cube();
    assert_eq!(digest(&b), digest(&b.clone()));
    assert_ne!(digest(&b), digest(&faulty()));
    let mut signed = b.clone();
    let t = &mut signed.edges.t_range[EdgeId(0).index()][0];
    assert_eq!(*t, 0.0);
    *t = -0.0;
    assert_ne!(digest(&b), digest(&signed));
    let text = |s: &str| {
        let mut d = TextDigest::new();
        d.write_str(s).unwrap();
        d.finish()
    };
    let words = |parts: &[&str]| {
        let mut d = TextDigest::new();
        for p in parts {
            d.write_str(p).unwrap();
        }
        d.finish()
    };
    // Split anywhere, a text digests the same; padding is not content.
    assert_eq!(text("abcdefghij"), words(&["abc", "", "defghi", "j"]));
    assert_ne!(text("abc"), text("abc\0"));
    assert_ne!(text(""), text("\0"));
}
