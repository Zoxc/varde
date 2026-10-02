//! Random sequences of what the user can do around the extrude and
//! revolve sessions, checking after each step what must always hold.

use varde_document::{Document, FeatureKind, OriginPlane, Plane};
use varde_regen::Request;
use varde_view::{Distance, ExtentKind, ExtrudeLook, Mode};

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
        (self.next() % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

const TEXTS: &[&str] = &[
    "90",
    "90°",
    "1 rad",
    "0",
    "-5",
    "360",
    "361",
    "359.5",
    "0.5",
    "abc",
    "",
    "45+45",
    "10 mm",
    "1e400",
    "pi",
    "180 deg",
    "2*90",
    "1/0",
    "nan",
    "120 +",
    "45 deg",
    "0.000001",
    "720",
    "10 mm / 1 mm",
    "1 turn",
    "200",
    "160",
];

const DISTANCES: &[&str] = &["10", "5 mm", "-3", "0", "abc", "1e300", "2 in", ""];

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

/// `original` with `ids` deleted.
fn without(original: &Sketch, ids: &[Id]) -> Sketch {
    let mut sketch = original.clone();
    sketch.delete(ids);
    sketch
}

fn check(lathe: &Lathe, step: usize, what: &str) {
    let doc = &lathe.doc;
    let at = || format!("step {step}: {what}");
    // Sessions never overlap, nor run in a read-only document.
    assert!(
        !(doc.revolve.is_some() && doc.extrude.is_some()),
        "{}",
        at()
    );
    assert!(!(doc.operating() && doc.sketch.is_some()), "{}", at());
    if !doc.editable() {
        assert!(!doc.operating(), "{}", at());
    }
    // The draft the lane was last asked for is the session's.
    let wanted = doc.extrude_draft().or_else(|| doc.revolve_draft());
    let requests = lathe.requests.borrow();
    if let Some(sent) = requests.iter().rev().find_map(sent_draft) {
        assert_eq!(sent, wanted, "{}", at());
    }
    drop(requests);
    if let Some(state) = doc.revolve_state() {
        // OK waits while the preview fails, and Accept error takes only that.
        let failed = doc.feed.draft_error().is_some();
        assert!(
            !(state.ready && failed) && !(state.accept && !failed),
            "{}",
            at()
        );
        if state.ready || state.accept {
            for angle in state.extent.angles() {
                assert!(state.fields[angle.index()].error.is_none(), "{}", at());
            }
            assert!(state.refused.is_none(), "{}", at());
            assert!(state.axis.is_some() && !state.picked.is_empty(), "{}", at());
            assert!(state.axis_name().is_some(), "{}", at());
        }
        // What the panel shows is what's previewed.
        if let Some((feature, FeatureKind::Revolve(draft))) = doc.revolve_draft() {
            assert_eq!(feature, doc.revolve.as_ref().unwrap().feature, "{}", at());
            assert_eq!(Some(draft.axis), state.axis, "{}", at());
            assert_eq!(draft.regions.len(), state.picked.len(), "{}", at());
            assert_eq!(draft.flip, state.flip, "{}", at());
            assert_eq!(OperationKind::of(&draft.operation), state.operation);
            let values: Vec<f64> = match &draft.extent {
                Turn::Full => vec![],
                Turn::OneSide(a) | Turn::Symmetric(a) => vec![a.value],
                Turn::TwoSides(a, b) => vec![a.value, b.value],
            };
            let kind = match draft.extent {
                Turn::Full => TurnKind::Full,
                Turn::OneSide(_) => TurnKind::OneSide,
                Turn::Symmetric(_) => TurnKind::Symmetric,
                Turn::TwoSides(..) => TurnKind::TwoSides,
            };
            assert_eq!(kind, state.extent, "{}", at());
            let shown: Vec<f64> = (state.extent.angles().iter())
                .map(|angle| state.fields[angle.index()].value.unwrap())
                .collect();
            assert_eq!(values, shown, "{}", at());
        }
        // Each picked region is one of the source's.
        let source = (state.candidates.iter()).find(|c| Some(c.feature) == state.source);
        if let Some(source) = source {
            for &region in state.picked {
                assert!(region < source.profiles.regions.len(), "{}", at());
            }
        }
    }
    if let Some(state) = doc.extrude_state() {
        let failed = doc.feed.draft_error().is_some();
        assert!(
            !(state.ready && failed) && !(state.accept && !failed),
            "{}",
            at()
        );
    }
    if let Some(state) = doc.extrude_state()
        && (state.ready || state.accept)
    {
        for distance in state.extent.distances() {
            assert!(state.fields[distance.index()].error.is_none(), "{}", at());
        }
        assert!(state.refused.is_none(), "{}", at());
    }
}

/// Runs `steps` random steps from `seed` on the lathe.
fn run(seed: u64, steps: usize) {
    let mut lathe = lathe();
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let original = match &lathe
        .doc
        .editor
        .document()
        .feature(lathe.sketch)
        .unwrap()
        .kind
    {
        FeatureKind::Sketch { sketch, .. } => sketch.clone(),
        _ => unreachable!(),
    };
    let first = lathe.doc.editor.document().clone();
    let mut renderer = varde_view::probe::renderer();
    let mut last_sent: Option<Request> = None;
    let note = |last_sent: &mut Option<Request>, lathe: &Lathe| {
        let requests = lathe.requests.borrow();
        let found =
            (requests.iter().rev()).find(|request| matches!(request, Request::Regenerate { .. }));
        if let Some(found) = found {
            *last_sent = Some(found.clone());
        }
    };
    for step in 0..steps {
        note(&mut last_sent, &lathe);
        let document = lathe.doc.editor.document().clone();
        let revision = lathe.doc.editor.revision();
        let sketches: Vec<FeatureId> = (document.features().iter())
            .filter(|f| matches!(f.kind, FeatureKind::Sketch { .. }))
            .map(|f| f.id)
            .collect();
        let features: Vec<FeatureId> = document.features().iter().map(|f| f.id).collect();
        let bodies: Vec<BodyId> = document.bodies().iter().map(|b| b.id).collect();
        let sketch = match sketches.is_empty() {
            true => lathe.sketch,
            false => *rng.pick(&sketches),
        };
        let regions = match document.feature(sketch).map(|f| &f.kind) {
            Some(FeatureKind::Sketch { sketch, .. }) => {
                sketch.profiles().map_or(0, |p| p.regions.len())
            }
            _ => 0,
        };
        let axes = [
            AxisLine::SketchX,
            AxisLine::SketchY,
            AxisLine::Curve(lathe.construction),
            AxisLine::Curve(lathe.left),
            AxisLine::Curve(lathe.circle),
            AxisLine::Curve(Id::X_AXIS),
        ];
        let roll = rng.below(59);
        let mut escaped = false;
        let mut committed = false;
        // While revolving, `X` and `Delete` don't act.
        let mut quiet = false;
        let what = format!("roll {roll}");
        match roll {
            0 => lathe.doc.look(Look::StartRevolve),
            1 | 2 => {
                // An extrude set up: started if it isn't, a region
                // picked.
                if lathe.doc.revolve.is_some() {
                    lathe.doc.look(Look::Escape);
                }
                if lathe.doc.extrude.is_none() {
                    lathe.doc.look(Look::StartExtrude);
                }
                let region = rng.below(regions.max(1));
                lathe
                    .doc
                    .look(Look::Extrude(ExtrudeLook::PickRegion { sketch, region }));
            }
            3 => lathe.doc.look(Look::StartExtrude),
            4..=8 => {
                let region = rng.below(regions + 1);
                lathe.revolve(RevolveLook::PickRegion { sketch, region });
            }
            9..=11 => {
                let axis = *rng.pick(&axes);
                lathe.revolve(RevolveLook::PickAxis { sketch, axis });
            }
            12 => {
                let picking = *rng.pick(&[RevolvePick::Regions, RevolvePick::Axis]);
                lathe.revolve(RevolveLook::Picking(picking));
            }
            13 | 14 => lathe.revolve(RevolveLook::Extent(*rng.pick(&TurnKind::ALL))),
            15..=18 => {
                let angle = *rng.pick(&[Angle::First, Angle::Second]);
                let text = (*rng.pick(TEXTS)).to_owned();
                lathe.revolve(RevolveLook::Input { angle, text });
            }
            19 => lathe.revolve(RevolveLook::Flip),
            20 => lathe.revolve(RevolveLook::Operation(*rng.pick(&OperationKind::ALL))),
            21 if !bodies.is_empty() => lathe.revolve(RevolveLook::Target(*rng.pick(&bodies))),
            22 => {
                escaped = lathe.doc.revolve.is_some();
                lathe.revolve(RevolveLook::Cancel);
            }
            23 => {
                committed = true;
                let accept = rng.below(3) == 0;
                (lathe.doc).update(if accept {
                    Edit::AcceptError
                } else {
                    Edit::CommitRevolve
                });
            }
            24 => {
                let region = rng.below(regions + 1);
                lathe
                    .doc
                    .look(Look::Extrude(ExtrudeLook::PickRegion { sketch, region }));
            }
            25 => {
                let distance = *rng.pick(&[Distance::First, Distance::Second]);
                let text = (*rng.pick(DISTANCES)).to_owned();
                (lathe.doc).look(Look::Extrude(ExtrudeLook::Input { distance, text }));
            }
            26 => {
                let kind = *rng.pick(&ExtentKind::ALL);
                lathe.doc.look(Look::Extrude(ExtrudeLook::Extent(kind)));
            }
            27 => {
                let kind = *rng.pick(&OperationKind::ALL);
                lathe.doc.look(Look::Extrude(ExtrudeLook::Operation(kind)));
            }
            28 => {
                committed = true;
                let accept = rng.below(3) == 0;
                (lathe.doc).update(if accept {
                    Edit::AcceptError
                } else {
                    Edit::CommitExtrude
                });
            }
            29 => lathe.doc.update(Edit::Undo),
            30 => lathe.doc.update(Edit::Redo),
            31 => {
                // A sketch edit: the axis line, a side, or the circle
                // deleted, or all back.
                let ids: &[Id] = match rng.below(4) {
                    0 => &[lathe.construction],
                    1 => &[lathe.left],
                    2 => &[lathe.circle],
                    _ => &[],
                };
                let edited = without(&original, ids);
                lathe.doc.apply(Command::SetSketch {
                    feature: lathe.sketch,
                    sketch: Box::new(edited),
                });
                lathe.doc.sync();
            }
            32 => {
                // Replaced whole: by the first document, or by itself
                // (no change).
                let by = if rng.below(2) == 0 {
                    first.clone()
                } else {
                    document.clone()
                };
                lathe.doc.apply(Command::Replace(Box::new(by)));
                lathe.doc.sync();
            }
            33 => {
                lathe.doc.read_only = match lathe.doc.read_only {
                    Some(_) => None,
                    None => Some("read-only".to_owned()),
                };
                lathe.doc.sync();
            }
            34 if !features.is_empty() => lathe.doc.look(Look::EditFeature(*rng.pick(&features))),
            35 if !features.is_empty() => lathe.doc.look(Look::SelectFeature(*rng.pick(&features))),
            36..=40 => {
                let keys = ["x", "o", "Enter", "Escape", "Delete"];
                let pressed = *rng.pick(&keys);
                let key = match pressed {
                    "Enter" => keyboard::Key::Named(key::Named::Enter),
                    "Escape" => keyboard::Key::Named(key::Named::Escape),
                    "Delete" => keyboard::Key::Named(key::Named::Delete),
                    c => keyboard::Key::Character(c.into()),
                };
                escaped = pressed == "Escape" && lathe.doc.operating();
                quiet = lathe.doc.revolve.is_some() && matches!(pressed, "x" | "Delete");
                committed = pressed == "Enter";
                // `Esc` is the app's, not a binding's.
                if pressed == "Escape" {
                    lathe.doc.look(Look::Escape);
                } else {
                    key_in(&mut lathe.doc, key);
                }
                // A delete prompt asked: confirmed or not.
                if lathe.doc.deleting.is_some() {
                    if rng.below(2) == 0 {
                        lathe.doc.update(Edit::ConfirmDelete);
                    } else {
                        lathe.doc.look(Look::CancelDelete);
                    }
                }
            }
            41 => {
                // Another sketch, on XY, with a square.
                let mut editor = Editor::new(document.clone());
                let add = editor.document().add_sketch(Plane::Origin(OriginPlane::XY));
                if editor.apply(add).is_ok() {
                    let feature = editor.document().features().last().unwrap().id;
                    let mut drawn = Sketch::default();
                    let corners = [(30.0, 30.0), (40.0, 30.0), (40.0, 40.0), (30.0, 40.0)]
                        .map(|(x, y)| drawn.add_point(DVec2::new(x, y)).unwrap());
                    for k in 0..4 {
                        let line = Curve::Line {
                            start: corners[k],
                            end: corners[(k + 1) % 4],
                        };
                        drawn.add_curve(line, false).unwrap();
                    }
                    let set = Command::SetSketch {
                        feature,
                        sketch: Box::new(drawn),
                    };
                    if editor.apply(set).is_ok() {
                        let replaced: Document = editor.document().clone();
                        lathe.doc.apply(Command::Replace(Box::new(replaced)));
                        lathe.doc.sync();
                    }
                }
            }
            42 => {
                // Answers in the order asked.
                lathe.answer();
            }
            43 => {
                // Answers newest first: the older answers come late.
                let mut taken = lathe.requests.take();
                taken.reverse();
                for request in taken {
                    lathe.doc.computed(varde_regen::handle(request));
                }
            }
            44 => {
                // Answers only the oldest.
                let mut requests = lathe.requests.borrow_mut();
                let oldest = (!requests.is_empty()).then(|| requests.remove(0));
                drop(requests);
                if let Some(request) = oldest {
                    lathe.doc.computed(varde_regen::handle(request));
                }
            }
            45 => {
                // Shown at some size, light or dark.
                let sizes = [
                    (1280.0, 800.0),
                    (640.0, 400.0),
                    (300.0, 900.0),
                    (1920.0, 300.0),
                ];
                let (w, h) = *rng.pick(&sizes);
                let mode = *rng.pick(&[Mode::Light, Mode::Dark]);
                let ui = crate::tests::shown(
                    lathe.doc.view_in(mode),
                    iced::Size::new(w, h),
                    &mut renderer,
                );
                drop(ui);
            }
            46..=55 => {
                // Set up as the user would: started if it isn't, a
                // region and an axis picked.
                if lathe.doc.sketch.is_some() {
                    lathe.doc.look(Look::FinishSketch);
                }
                if lathe.doc.extrude.is_some() {
                    lathe.doc.look(Look::Escape);
                }
                if lathe.doc.read_only.take().is_some() {
                    lathe.doc.sync();
                }
                if lathe.doc.revolve.is_none() {
                    lathe.doc.look(Look::StartRevolve);
                }
                let region = rng.below(regions.max(1));
                lathe.revolve(RevolveLook::PickRegion { sketch, region });
                let axis = *rng.pick(&axes[..4]);
                lathe.revolve(RevolveLook::PickAxis { sketch, axis });
            }
            56 => lathe.doc.update(crate::tests::an_edit(&lathe.doc)),
            57 => {
                let tolerance = *rng.pick(&[1e-2, 1e-3]);
                let tolerance = varde_document::Tolerance::new(tolerance).unwrap();
                lathe.doc.update(Edit::SetTolerance(tolerance));
            }
            58 if !features.is_empty() => {
                // Deleted from the Timeline's menu, mid-session or not.
                lathe.doc.update(Edit::RemoveFeature(*rng.pick(&features)));
                if lathe.doc.deleting.is_some() {
                    lathe.doc.update(Edit::ConfirmDelete);
                }
            }
            _ => {}
        }
        let after = lathe.doc.editor.document();
        if escaped {
            assert_eq!(*after, document, "step {step}: esc changed the document");
            assert_eq!(lathe.doc.editor.revision(), revision, "step {step}");
            assert!(!lathe.doc.operating(), "step {step}: {what}");
        }
        if quiet {
            assert_eq!(*after, document, "step {step}: a key acted");
            let revolving = lathe.doc.revolve.is_some() && lathe.doc.extrude.is_none();
            assert!(revolving, "step {step}: {what}");
        }
        if committed && lathe.doc.editor.revision() != revision {
            // One undo step, and redo puts it back.
            let made = after.clone();
            lathe.doc.update(Edit::Undo);
            assert_eq!(*lathe.doc.editor.document(), document, "step {step}: undo");
            lathe.doc.update(Edit::Redo);
            assert_eq!(*lathe.doc.editor.document(), made, "step {step}: redo");
        }
        check(&lathe, step, &what);
        note(&mut last_sent, &lathe);
        // Once all are answered, late ones too, the draft's error is the
        // newest's.
        if matches!(roll, 42..=44)
            && lathe.requests.borrow().is_empty()
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
                lathe.doc.feed.draft_error(),
                error.as_deref(),
                "step {step}: stale answer shown"
            );
        }
    }
}

#[test]
fn random_sessions_keep_their_invariants() {
    let seeds: u64 = std::env::var("VARDE_FUZZ_SEEDS")
        .ok()
        .and_then(|seeds| seeds.parse().ok())
        .unwrap_or(6);
    let from: u64 = std::env::var("VARDE_FUZZ_FROM")
        .ok()
        .and_then(|from| from.parse().ok())
        .unwrap_or(0);
    for seed in from..from.saturating_add(seeds) {
        run(seed, 150);
    }
}
