//! Random sequences of what the user can do around the combine session,
//! checking after each step what must always hold.

use varde_document::{Command, Extent, Extrude, OriginPlane, Plane};
use varde_view::{OperationKind, Pick};

use super::*;

/// A small deterministic generator: xorshift64*.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

/// The draft the newest request carries, as `(feature, kind)`, if it
/// carries one: what the lane was last asked to preview.
fn sent_draft(request: &Request) -> Option<Option<(Option<FeatureId>, FeatureKind)>> {
    match request {
        Request::Regenerate { draft, .. } => Some(
            draft
                .as_ref()
                .map(|draft| (draft.feature, draft.kind.clone())),
        ),
        _ => None,
    }
}

/// Adds, through the document, a disc of radius 4 about `center` from z
/// 0 up to z 4 with `operation`: a sketch, its drawing and its extrude,
/// each its own undo step.
pub(super) fn add_disc_in(doc: &mut Doc, center: (f64, f64), operation: Operation) {
    let document = doc.editor.document().clone();
    doc.apply(document.add_sketch(Plane::Origin(OriginPlane::XY)));
    let Some(sketch) = (doc.editor.document().features().last())
        .filter(|feature| matches!(feature.kind, FeatureKind::Sketch { .. }))
        .map(|feature| feature.id)
    else {
        return;
    };
    let mut drawn = varde_sketch::Sketch::default();
    let center = drawn
        .add_point(glam::DVec2::new(center.0, center.1))
        .unwrap();
    let circle = varde_sketch::Curve::Circle {
        center,
        radius: 4.0,
    };
    drawn.add_curve(circle, false).unwrap();
    let profiles = drawn.profiles().unwrap();
    let regions = vec![profiles.reference(0).unwrap()];
    doc.apply(Command::SetSketch {
        feature: sketch,
        sketch: Box::new(drawn),
    });
    let ask = Extent::ask(&doc.editor.document().design());
    let extent = Extent::OneSide(varde_expr::Value::new("4", &ask).unwrap());
    let extrude = Extrude {
        taper: None,
        sketch,
        regions,
        extent,
        flip: false,
        operation,
    };
    let add = doc.editor.document().add_feature(extrude.into());
    doc.apply(add);
}

fn check(plates: &Plates, step: usize, what: &str) {
    let doc = &plates.doc;
    let at = || format!("step {step}: {what}");
    // Sessions never overlap, nor run in a read-only document.
    if doc.combine.is_some() {
        assert!(doc.extrude.is_none() && doc.revolve.is_none(), "{}", at());
        assert!(doc.sketch.is_none() && doc.measure.is_none(), "{}", at());
        assert!(doc.picking_plane.is_none(), "{}", at());
        assert!(doc.editable(), "{}", at());
    }
    if doc.picking_plane.is_some() {
        assert!(!doc.operating(), "{}", at());
    }
    // The draft the lane was last asked for is the session's.
    let wanted = (doc.extrude_draft())
        .or_else(|| doc.revolve_draft())
        .or_else(|| doc.combine_draft());
    let requests = plates.requests.borrow();
    if let Some(sent) = requests.iter().rev().find_map(sent_draft) {
        assert_eq!(sent, wanted, "{}", at());
    }
    drop(requests);
    let Some(session) = &doc.combine else {
        return;
    };
    let document = doc.editor.document();
    // The bodies are ones the combine can name, the tools sorted
    // without repeats, the target not among them.
    for body in session.target.iter().chain(&session.tools) {
        assert!(pickable(document, *body, session.feature), "{}", at());
    }
    assert!(session.tools.windows(2).all(|w| w[0] < w[1]), "{}", at());
    assert!(
        session.target.is_none_or(|t| !session.tools.contains(&t)),
        "{}",
        at()
    );
    // The combine edited is there.
    if let Some(feature) = session.feature {
        assert!(
            matches!(
                document.feature(feature).map(|f| &f.kind),
                Some(FeatureKind::Combine(_))
            ),
            "{}",
            at()
        );
    }
    // The panel shows the session.
    let state = doc.combine_state().unwrap();
    assert_eq!(state.target.map(|t| t.body), session.target, "{}", at());
    let tools: Vec<BodyId> = state.tools.iter().map(|t| t.body).collect();
    assert_eq!(tools, session.tools, "{}", at());
    assert_eq!(
        (state.op, state.keep_tools),
        (session.op, session.keep_tools),
        "{}",
        at()
    );
    // OK waits while the preview fails, and Accept error takes only that.
    let failed = doc.feed.draft_error().is_some();
    assert!(
        !(state.ready && failed) && !(state.accept && !failed),
        "{}",
        at()
    );
    if state.ready || state.accept {
        assert!(session.target.is_some() && !session.tools.is_empty());
        assert!(doc.editable(), "{}", at());
    }
}

/// Runs `steps` random steps from `seed` on the plates.
fn run(seed: u64, steps: usize) {
    let mut plates = plates();
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let first = plates.doc.editor.document().clone();
    let mut renderer = varde_view::probe::renderer();
    let mut last_sent: Option<Request> = None;
    let note = |last_sent: &mut Option<Request>, plates: &Plates| {
        let requests = plates.requests.borrow();
        let found =
            (requests.iter().rev()).find(|request| matches!(request, Request::Regenerate { .. }));
        if let Some(found) = found {
            *last_sent = Some(found.clone());
        }
    };
    let mut stale: Option<Pick> = None;
    for step in 0..steps {
        note(&mut last_sent, &plates);
        let document = plates.doc.editor.document().clone();
        let revision = plates.doc.editor.revision();
        let features: Vec<FeatureId> = document.features().iter().map(|f| f.id).collect();
        let combines: Vec<FeatureId> = (document.features().iter())
            .filter(|f| matches!(f.kind, FeatureKind::Combine(_)))
            .map(|f| f.id)
            .collect();
        // Bodies the document holds, and some it doesn't.
        let mut bodies: Vec<BodyId> = document.bodies().iter().map(|b| b.id).collect();
        bodies.extend(plates.bodies);
        let roll = rng.below(56);
        let mut escaped = false;
        let mut committed = false;
        let what = format!("roll {roll}");
        match roll {
            0 => plates.doc.look(Look::StartCombine),
            1 => key_in(&mut plates.doc, character("b")),
            2 if !combines.is_empty() => {
                plates.doc.look(Look::EditFeature(rng.pick(&combines)));
            }
            3..=8 => {
                // A click in the viewport on the model shown, on a face,
                // an edge or nothing.
                let index = plates.doc.feed.pick_index();
                let faces = index.picking().faces().len();
                let pick = (faces > 0 && rng.below(6) != 0).then(|| {
                    let face = rng.below(faces) as u32;
                    let target = Picked::Face(face);
                    Pick {
                        model: index.model(),
                        target,
                        body: index.body(target).unwrap(),
                        at: glam::DVec3::ZERO,
                        snap: None,
                    }
                });
                if let Some(pick) = &pick
                    && rng.below(3) == 0
                {
                    plates.doc.look(Look::Hover(Some(*pick)));
                }
                // Sometimes one picked on a model shown before.
                let pick = match (rng.below(8), &stale) {
                    (0, Some(old)) => Some(*old),
                    _ => pick,
                };
                if pick.is_some() {
                    stale = pick;
                }
                let add = rng.below(4) == 0;
                plates.doc.look(Look::ClickModel {
                    pick,
                    add,
                    double: false,
                });
            }
            9..=12 => {
                // A row of Objects.
                let body = rng.pick(&bodies);
                let add = rng.below(4) == 0;
                plates.doc.look(Look::ClickBody { body, add });
            }
            13 => {
                let picking = rng.pick(&[CombinePick::Target, CombinePick::Tools]);
                plates.combine(CombineLook::Picking(picking));
            }
            14 | 15 => plates.combine(CombineLook::Drop(rng.pick(&bodies))),
            16 => {
                let op = rng.pick(&[BodyOp::Union, BodyOp::Subtract, BodyOp::Intersect]);
                plates.combine(CombineLook::Operation(op));
            }
            17 => plates.combine(CombineLook::KeepTools),
            18 => {
                escaped = plates.doc.combine.is_some();
                plates.combine(CombineLook::Cancel);
            }
            19 => {
                escaped = plates.doc.operating() || plates.doc.picking_plane.is_some();
                escaped &= plates.doc.deleting.is_none() && plates.doc.rail.open.is_none();
                plates.doc.look(Look::Escape);
            }
            20 => {
                committed = true;
                plates.doc.update(Edit::CommitCombine);
            }
            21 => {
                committed = true;
                plates.doc.update(Edit::AcceptError);
            }
            22 => {
                committed = true;
                key_in(&mut plates.doc, enter());
            }
            23 => plates.doc.update(Edit::Undo),
            24 => plates.doc.update(Edit::Redo),
            25 => plates.answer(),
            26 => {
                // Answers newest first: the older answers come late.
                let mut taken = plates.requests.take();
                taken.reverse();
                for request in taken {
                    plates.doc.computed(varde_regen::handle(request));
                }
            }
            27 => {
                // Answers only the oldest.
                let mut requests = plates.requests.borrow_mut();
                let oldest = (!requests.is_empty()).then(|| requests.remove(0));
                drop(requests);
                if let Some(request) = oldest {
                    plates.doc.computed(varde_regen::handle(request));
                }
            }
            28 => {
                plates.doc.read_only = match plates.doc.read_only {
                    Some(_) => None,
                    None => Some("read-only".to_owned()),
                };
                plates.doc.sync();
            }
            29 => plates.doc.look(Look::StartExtrude),
            30 => plates.doc.look(Look::StartRevolve),
            31 => plates.doc.look(Look::StartMeasure),
            32 => plates.doc.look(Look::PickPlane),
            33 if !features.is_empty() => {
                plates.doc.look(Look::EditFeature(rng.pick(&features)));
            }
            34 if !features.is_empty() => {
                plates.doc.look(Look::SelectFeature(rng.pick(&features)));
            }
            35 if !features.is_empty() => {
                // Deleted from the Timeline's menu, mid-session or not.
                plates.doc.update(Edit::RemoveFeature(rng.pick(&features)));
                if plates.doc.deleting.is_some() {
                    plates.doc.update(Edit::ConfirmDelete);
                }
            }
            36 => {
                plates.doc.update(Edit::RemoveBody(rng.pick(&bodies)));
                if plates.doc.deleting.is_some() {
                    if rng.below(2) == 0 {
                        plates.doc.update(Edit::ConfirmDelete);
                    } else {
                        plates.doc.look(Look::CancelDelete);
                    }
                }
            }
            37 => {
                // Keys, as pressed.
                let pressed = rng.pick(&["x", "o", "i", "s", "Delete", "b"]);
                let key = match pressed {
                    "Delete" => keyboard::Key::Named(key::Named::Delete),
                    c => character(c),
                };
                key_in(&mut plates.doc, key);
                if plates.doc.deleting.is_some() {
                    if rng.below(2) == 0 {
                        plates.doc.update(Edit::ConfirmDelete);
                    } else {
                        plates.doc.look(Look::CancelDelete);
                    }
                }
            }
            38 => {
                // A new body, or a join over the plate and a disc.
                let center = rng.pick(&[(20.0, 0.0), (-20.0, 0.0), (0.0, 15.0), (24.0, 0.0)]);
                let operation = match rng.below(3) {
                    0 => Operation::Join(Default::default()),
                    _ => Operation::NewBody(BodyId::NEW),
                };
                plates.doc.sync();
                add_disc_in(&mut plates.doc, center, operation);
                plates.doc.sync();
            }
            39 => {
                // Replaced whole: by the first document, or by itself.
                let by = if rng.below(2) == 0 {
                    first.clone()
                } else {
                    document.clone()
                };
                plates.doc.apply(Command::Replace(Box::new(by)));
                plates.doc.sync();
            }
            40 => {
                // Shown at some size, light or dark.
                let sizes = [(1280.0, 800.0), (640.0, 400.0), (300.0, 900.0)];
                let (w, h) = rng.pick(&sizes);
                let mode = rng.pick(&[Mode::Light, Mode::Dark]);
                plates.doc.look(Look::SelectPanel(Panel::Objects));
                let ui = shown(
                    plates.doc.view_in(mode),
                    iced::Size::new(w, h),
                    &mut renderer,
                );
                drop(ui);
            }
            41..=44 | 48..=55 => {
                // Set up as the user would: started if it isn't, a target
                // and a tool or two picked.
                if plates.doc.sketch.is_some() {
                    plates.doc.look(Look::FinishSketch);
                }
                if plates.doc.read_only.take().is_some() {
                    plates.doc.sync();
                }
                // Out of whatever else is open.
                while plates.doc.combine.is_none()
                    && (plates.doc.operating()
                        || plates.doc.picking_plane.is_some()
                        || plates.doc.measure.is_some()
                        || plates.doc.deleting.is_some()
                        || plates.doc.rail.open.is_some())
                {
                    plates.doc.look(Look::Escape);
                }
                if plates.doc.combine.is_none() {
                    plates.doc.look(Look::StartCombine);
                }
                let held: Vec<BodyId> = document.bodies().iter().map(|b| b.id).collect();
                if !held.is_empty() {
                    for _ in 0..1 + rng.below(3) {
                        let body = rng.pick(&held);
                        plates.doc.look(Look::ClickBody { body, add: false });
                    }
                }
                // Then often committed, answered first or not.
                if rng.below(2) == 0 {
                    if rng.below(2) == 0 {
                        plates.answer();
                    }
                    committed = true;
                    plates.doc.update(Edit::CommitCombine);
                }
            }
            45 => plates.doc.update(crate::tests::an_edit(&plates.doc)),
            46 => {
                // A combine of the session's bodies in place of an earlier
                // one: the extrude panel's operation on a combine's
                // body, refused.
                let kind = rng.pick(&OperationKind::ALL);
                plates.doc.look(Look::StartExtrude);
                plates
                    .doc
                    .look(Look::Extrude(varde_view::ExtrudeLook::Operation(kind)));
            }
            _ => {}
        }
        let after = plates.doc.editor.document();
        if escaped {
            assert_eq!(*after, document, "step {step}: esc changed the document");
            assert_eq!(plates.doc.editor.revision(), revision, "step {step}");
            assert!(plates.doc.combine.is_none(), "step {step}: {what}");
        }
        if committed && plates.doc.editor.revision() != revision {
            // One undo step, and redo puts it back.
            let made = after.clone();
            assert!(plates.doc.combine.is_none(), "step {step}: committed");
            plates.doc.update(Edit::Undo);
            assert_eq!(*plates.doc.editor.document(), document, "step {step}: undo");
            plates.doc.update(Edit::Redo);
            assert_eq!(*plates.doc.editor.document(), made, "step {step}: redo");
        }
        check(&plates, step, &what);
        note(&mut last_sent, &plates);
        // Once all are answered, late ones too, the draft's error is the
        // newest's.
        if matches!(roll, 25..=27)
            && plates.requests.borrow().is_empty()
            && let Some(request) = &last_sent
            && let Request::Regenerate { draft: Some(_), .. } = request
        {
            let varde_regen::Response::Regenerated { draft, .. } =
                varde_regen::handle(request.clone())
            else {
                panic!("a regeneration");
            };
            let error = draft.and_then(|draft| draft.error);
            assert_eq!(
                plates.doc.feed.draft_error(),
                error.as_deref(),
                "step {step}: stale answer shown"
            );
        }
    }
}

/// The steps of the quick run, see [`crate::tests::fuzz_steps`].
const QUICK_STEPS: usize = 15;

#[test]
fn random_sessions_keep_their_invariants() {
    for seed in varde_testing::seeds(1, 4) {
        run(seed, crate::tests::fuzz_steps(QUICK_STEPS, 150));
    }
}
