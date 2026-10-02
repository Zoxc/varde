use std::cell::RefCell;
use std::collections::BTreeSet;
use std::f64::consts::PI;
use std::rc::Rc;

use glam::DVec2;
use iced::keyboard::{self, key};
use varde_document::{
    AxisLine, BodyId, Command, Document, Editor, FeatureId, FeatureKind, Id, OriginPlane, Plane,
    Revolve, RevolveError, Sketch,
};
use varde_regen::Request;
use varde_sketch::Curve;
use varde_view::{Angle, Edit, Look, OperationKind, RevolveLook, RevolvePick, TurnKind};

use super::*;
use crate::tests::{answer, deferred, key_in, press_in};

type Requests = Rc<RefCell<Vec<Request>>>;

/// A lathe profile on XZ: a 10 × 30 rectangle from (10, 0) to (20, 30)
/// in the sketch, a construction line from (0, 0) to (0, 30) along its
/// y axis, and a circle of radius 2 about (40, 15), shown and not
/// revolved.
pub(crate) struct Lathe {
    pub(crate) doc: Doc,
    requests: Requests,
    pub(crate) sketch: FeatureId,
    /// The rectangle's left side, from (10, 30) to (10, 0): against +y.
    left: Id,
    /// The construction line, from (0, 0) to (0, 30).
    pub(crate) construction: Id,
    circle: Id,
}

pub(crate) fn lathe() -> Lathe {
    let mut editor = Editor::new(Document::default());
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XZ)))
        .unwrap();
    let sketch = editor.document().features()[0].id;
    let mut drawn = Sketch::default();
    let corners = [(10.0, 0.0), (20.0, 0.0), (20.0, 30.0), (10.0, 30.0)]
        .map(|(x, y)| drawn.add_point(DVec2::new(x, y)).unwrap());
    let sides: Vec<Id> = (0..4)
        .map(|k| {
            let line = Curve::Line {
                start: corners[k],
                end: corners[(k + 1) % 4],
            };
            drawn.add_curve(line, false).unwrap()
        })
        .collect();
    let [start, end] =
        [(0.0, 0.0), (0.0, 30.0)].map(|(x, y)| drawn.add_point(DVec2::new(x, y)).unwrap());
    let construction = drawn.add_curve(Curve::Line { start, end }, true).unwrap();
    let center = drawn.add_point(DVec2::new(40.0, 15.0)).unwrap();
    let circle = drawn
        .add_curve(
            Curve::Circle {
                center,
                radius: 2.0,
            },
            false,
        )
        .unwrap();
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn),
        })
        .unwrap();
    let (mut doc, requests) = deferred();
    doc.apply(Command::Replace(Box::new(editor.document().clone())));
    doc.sync();
    answer(&mut doc, &requests);
    Lathe {
        doc,
        requests,
        sketch,
        left: sides[3],
        construction,
        circle,
    }
}

impl Lathe {
    /// The rectangle's region, by its index.
    pub(crate) fn rectangle(&self) -> usize {
        let Some(FeatureKind::Sketch { sketch, .. }) = self
            .doc
            .editor
            .document()
            .feature(self.sketch)
            .map(|f| &f.kind)
        else {
            panic!("no sketch");
        };
        let profiles = sketch.profiles().unwrap();
        profiles.region_at(DVec2::new(15.0, 15.0)).unwrap()
    }

    pub(crate) fn revolve(&mut self, message: RevolveLook) {
        self.doc.look(Look::Revolve(message));
    }

    /// Starts a session and picks the rectangle and `axis`.
    pub(crate) fn set_up(&mut self, axis: AxisLine) {
        self.doc.look(Look::StartRevolve);
        let (sketch, region) = (self.sketch, self.rectangle());
        self.revolve(RevolveLook::PickRegion { sketch, region });
        self.revolve(RevolveLook::PickAxis { sketch, axis });
    }

    pub(crate) fn input(&mut self, angle: Angle, text: &str) {
        let text = text.to_owned();
        self.revolve(RevolveLook::Input { angle, text });
    }

    /// The draft the last request waiting carries, if any, and the
    /// revolve it edits.
    fn last_draft(&self) -> Option<(Option<FeatureId>, Revolve)> {
        let requests = self.requests.borrow();
        let Request::Regenerate { draft, .. } = requests.last()? else {
            return None;
        };
        let draft = draft.as_ref()?;
        let FeatureKind::Revolve(revolve) = &draft.kind else {
            panic!("the draft is a revolve's");
        };
        Some((draft.feature, revolve.clone()))
    }

    pub(crate) fn answer(&mut self) {
        answer(&mut self.doc, &self.requests);
    }

    /// The curve `id` of the lathe's sketch.
    fn sketch_curve(&self, id: Id) -> Option<&Curve> {
        let document = self.doc.editor.document();
        match &document.feature(self.sketch)?.kind {
            FeatureKind::Sketch { sketch, .. } => Some(&sketch.curve(id)?.curve),
            _ => None,
        }
    }

    /// The revolves of the document.
    fn revolves(&self) -> Vec<(FeatureId, &Revolve)> {
        (self.doc.editor.document().features().iter())
            .filter_map(|feature| match &feature.kind {
                FeatureKind::Revolve(revolve) => Some((feature.id, revolve)),
                _ => None,
            })
            .collect()
    }
}

fn enter() -> keyboard::Key {
    keyboard::Key::Named(key::Named::Enter)
}

#[test]
fn regions_then_a_sketch_line_picked_are_previewed() {
    let mut lathe = lathe();
    lathe.doc.look(Look::StartRevolve);
    let session = lathe.doc.revolve.as_ref().expect("a session");
    assert_eq!(session.regions.source, None);
    assert_eq!(session.picking, RevolvePick::Regions);
    let state = lathe.doc.revolve_state().unwrap();
    assert_eq!(state.candidates.len(), 1);
    assert!(!state.ready);
    assert!(lathe.last_draft().is_none());

    // A region picked sets the sketch, and the axis is picked next.
    let (sketch, region) = (lathe.sketch, lathe.rectangle());
    lathe.revolve(RevolveLook::PickRegion { sketch, region });
    let session = lathe.doc.revolve.as_ref().unwrap();
    assert_eq!(session.regions.source, Some(sketch));
    assert_eq!(session.regions.picked, BTreeSet::from([region]));
    assert_eq!(session.picking, RevolvePick::Axis);
    // Without an axis there's nothing to preview.
    assert!(!lathe.doc.revolve_state().unwrap().ready);
    assert!(lathe.last_draft().is_none());

    let axis = AxisLine::Curve(lathe.left);
    lathe.revolve(RevolveLook::PickAxis { sketch, axis });
    let state = lathe.doc.revolve_state().unwrap();
    assert_eq!(state.axis, Some(axis));
    assert_eq!(state.axis_name().as_deref(), Some("Line 4"));
    assert_eq!(state.picking, RevolvePick::Regions);
    assert!(state.ready);

    // The preview is the draft applied: a full turn making a new body.
    let (feature, draft) = lathe.last_draft().expect("a draft");
    assert_eq!(feature, None);
    assert_eq!(draft.sketch, sketch);
    assert_eq!(draft.axis, axis);
    assert_eq!(draft.extent, varde_document::Turn::Full);
    assert_eq!(draft.span(), None);
    lathe.answer();
    assert_eq!(lathe.doc.feed.draft_error(), None);
    assert!(lathe.doc.feed.mesh().triangle_count() > 0);
    assert!(lathe.revolves().is_empty());
}

#[test]
fn a_built_in_axis_is_stored_as_the_sketch_s_and_a_construction_line_as_a_curve() {
    let mut lathe = lathe();
    lathe.set_up(AxisLine::SketchY);
    let (_, draft) = lathe.last_draft().unwrap();
    assert_eq!(draft.axis, AxisLine::SketchY);
    let state = lathe.doc.revolve_state().unwrap();
    assert_eq!(state.axis_name().as_deref(), Some("Y axis"));
    lathe.answer();
    assert_eq!(lathe.doc.feed.draft_error(), None);
    assert!(lathe.doc.feed.mesh().triangle_count() > 0);

    // The axis field picks again; a construction line is a line.
    let sketch = lathe.sketch;
    lathe.revolve(RevolveLook::Picking(RevolvePick::Axis));
    let axis = AxisLine::Curve(lathe.construction);
    lathe.revolve(RevolveLook::PickAxis { sketch, axis });
    assert_eq!(lathe.last_draft().unwrap().1.axis, axis);
    lathe.answer();
    assert_eq!(lathe.doc.feed.draft_error(), None);
}

#[test]
fn a_circle_is_no_axis() {
    let mut lathe = lathe();
    lathe.set_up(AxisLine::Curve(lathe.circle));
    let session = lathe.doc.revolve.as_ref().unwrap();
    assert_eq!(session.axis, None);
    assert_eq!(session.picking, RevolvePick::Axis);
    assert!(lathe.last_draft().is_none());
}

/// The lathe's sketch as `edit` changes it, set.
fn edit_sketch(lathe: &mut Lathe, edit: impl FnOnce(&mut Sketch)) {
    let document = lathe.doc.editor.document();
    let Some(FeatureKind::Sketch { sketch, .. }) = document.feature(lathe.sketch).map(|f| &f.kind)
    else {
        panic!("no sketch");
    };
    let mut sketch = sketch.clone();
    edit(&mut sketch);
    let feature = lathe.sketch;
    lathe.doc.apply(Command::SetSketch {
        feature,
        sketch: Box::new(sketch),
    });
    lathe.doc.sync();
}

#[test]
fn a_line_of_no_length_is_no_axis() {
    let mut lathe = lathe();
    // The construction line's end dragged onto its start: no direction.
    let Some(&Curve::Line { start, end }) = lathe.sketch_curve(lathe.construction) else {
        panic!("a line");
    };
    lathe.set_up(AxisLine::Curve(lathe.construction));
    assert!(lathe.doc.revolve_state().unwrap().ready);
    edit_sketch(&mut lathe, |sketch| {
        let at = sketch.point(start).unwrap().at;
        sketch.point_mut(end).unwrap().at = at;
    });
    // Picked, it waits as a deleted line does: no preview, no OK.
    let state = lathe.doc.revolve_state().unwrap();
    assert_eq!(state.axis, None);
    assert!(!state.ready);
    assert!(lathe.last_draft().is_none());
    // Nor can it be picked again.
    let sketch = lathe.sketch;
    lathe.revolve(RevolveLook::Picking(RevolvePick::Axis));
    let axis = AxisLine::Curve(lathe.construction);
    lathe.revolve(RevolveLook::PickAxis { sketch, axis });
    assert!(!lathe.doc.revolve_state().unwrap().ready);
    // Its length back, it's the axis again.
    edit_sketch(&mut lathe, |sketch| {
        sketch.point_mut(end).unwrap().at = DVec2::new(0.0, 30.0);
    });
    let state = lathe.doc.revolve_state().unwrap();
    assert_eq!(state.axis, Some(axis));
    assert!(state.ready);
}

#[test]
fn typed_angles_of_each_kind_reach_the_draft() {
    let mut lathe = lathe();
    lathe.set_up(AxisLine::SketchY);
    let span = |lathe: &Lathe| lathe.last_draft().unwrap().1.span();
    let near = |a: Option<(f64, f64)>, b: (f64, f64)| {
        let (a0, a1) = a.expect("a part turn");
        assert!(
            (a0 - b.0).abs() < 1e-12 && (a1 - b.1).abs() < 1e-12,
            "{a:?} {b:?}"
        );
    };

    // One side starts at half a turn.
    lathe.revolve(RevolveLook::Extent(TurnKind::OneSide));
    near(span(&lathe), (0.0, PI));
    lathe.input(Angle::First, "90");
    near(span(&lathe), (0.0, PI / 2.0));
    lathe.revolve(RevolveLook::Flip);
    near(span(&lathe), (-PI / 2.0, 0.0));

    // Symmetric ignores Flip.
    lathe.revolve(RevolveLook::Extent(TurnKind::Symmetric));
    lathe.input(Angle::First, "120");
    near(span(&lathe), (-PI / 3.0, PI / 3.0));

    // Two sides: the second starts at a quarter, the other way. Still
    // flipped from one side, they're swapped.
    lathe.revolve(RevolveLook::Extent(TurnKind::TwoSides));
    near(span(&lathe), (-2.0 * PI / 3.0, PI / 2.0));
    lathe.input(Angle::Second, "45 deg");
    near(span(&lathe), (-2.0 * PI / 3.0, PI / 4.0));
    lathe.revolve(RevolveLook::Flip);
    near(span(&lathe), (-PI / 4.0, 2.0 * PI / 3.0));
    assert!(lathe.doc.revolve_state().unwrap().ready);

    // Together over a turn, the revolve's own check refuses it: no OK.
    lathe.input(Angle::Second, "300");
    let state = lathe.doc.revolve_state().unwrap();
    assert_eq!(state.refused, Some(RevolveError::Turn));
    assert!(!state.ready);
    assert!(press_in(&lathe.doc, enter()).is_none());

    // A refused text says why and blocks OK; the draft keeps the last.
    lathe.input(Angle::Second, "45");
    let revision = lathe.doc.feed.revision();
    lathe.input(Angle::First, "120 +");
    let state = lathe.doc.revolve_state().unwrap();
    assert!(state.fields[0].error.is_some());
    assert!(!state.ready);
    assert_eq!(lathe.doc.feed.revision(), revision);

    // A full turn has no angles, and no refused field blocks it.
    lathe.revolve(RevolveLook::Extent(TurnKind::Full));
    assert_eq!(span(&lathe), None);
    assert!(lathe.doc.revolve_state().unwrap().ready);
}

#[test]
fn enter_commits_the_revolve_as_one_undo_step() {
    let mut lathe = lathe();
    let before = lathe.doc.editor.document().clone();
    lathe.set_up(AxisLine::Curve(lathe.left));
    lathe.revolve(RevolveLook::Extent(TurnKind::OneSide));
    lathe.input(Angle::First, "90");
    lathe.answer();

    key_in(&mut lathe.doc, enter());
    assert!(lathe.doc.revolve.is_none());
    let [(feature, revolve)] = lathe.revolves()[..] else {
        panic!("one revolve");
    };
    assert_eq!(revolve.axis, AxisLine::Curve(lathe.left));
    let quarter = revolve.span().unwrap();
    assert!((quarter.1 - quarter.0 - PI / 2.0).abs() < 1e-12);
    assert_eq!(lathe.doc.selected_feature, Some(feature));
    let document = lathe.doc.editor.document();
    assert_eq!(document.feature(feature).unwrap().name, "Revolve 1");
    assert_eq!(document.bodies().len(), 1);
    // Its sketch is hidden, as an extrude's.
    assert!(!document.feature(lathe.sketch).unwrap().visible);
    assert!(lathe.last_draft().is_none());
    lathe.answer();
    assert!(lathe.doc.feed.mesh().triangle_count() > 0);
    assert!(lathe.doc.feed.failed_features().is_empty());

    lathe.doc.update(Edit::Undo);
    assert_eq!(*lathe.doc.editor.document(), before);
}

#[test]
fn escape_leaves_no_trace() {
    let mut lathe = lathe();
    let before = lathe.doc.editor.document().clone();
    let revision = lathe.doc.editor.revision();
    lathe.set_up(AxisLine::SketchY);
    lathe.revolve(RevolveLook::Operation(OperationKind::Join));
    lathe.answer();
    assert!(lathe.doc.feed.shown_draft().is_some());

    lathe.doc.look(Look::Escape);
    assert!(lathe.doc.revolve.is_none());
    assert_eq!(*lathe.doc.editor.document(), before);
    assert_eq!(lathe.doc.editor.revision(), revision);
    assert!(lathe.last_draft().is_none());
    lathe.answer();
    assert_eq!(lathe.doc.feed.shown_draft(), None);
    assert_eq!(lathe.doc.feed.mesh().triangle_count(), 0);

    // Cancel does the same, and so does starting it again.
    lathe.set_up(AxisLine::SketchY);
    lathe.revolve(RevolveLook::Cancel);
    assert!(lathe.doc.revolve.is_none());
    lathe.set_up(AxisLine::SketchY);
    lathe.doc.look(Look::StartRevolve);
    assert!(lathe.doc.revolve.is_none());
    assert_eq!(lathe.doc.editor.revision(), revision);
}

#[test]
fn editing_a_revolve_reopens_it_with_its_values_and_sets_it_again() {
    let mut lathe = lathe();
    lathe.set_up(AxisLine::Curve(lathe.construction));
    lathe.revolve(RevolveLook::Extent(TurnKind::TwoSides));
    lathe.input(Angle::First, "100");
    lathe.input(Angle::Second, "20");
    lathe.revolve(RevolveLook::Flip);
    lathe.doc.update(Edit::CommitRevolve);
    lathe.answer();
    let [(feature, committed)] = lathe.revolves()[..] else {
        panic!("one revolve");
    };
    let committed = committed.clone();
    let features = lathe.doc.editor.document().features().len();

    lathe.doc.look(Look::EditFeature(feature));
    let session = lathe.doc.revolve.as_ref().expect("editing it");
    assert_eq!(session.feature, Some(feature));
    assert_eq!(session.regions.source, Some(lathe.sketch));
    assert_eq!(session.regions.picked, BTreeSet::from([lathe.rectangle()]));
    assert_eq!(session.axis, Some(AxisLine::Curve(lathe.construction)));
    assert_eq!(session.extent, TurnKind::TwoSides);
    assert_eq!(session.fields[0].text, "100");
    assert_eq!(session.fields[1].text, "20");
    assert!(session.flip);
    assert_eq!(session.operation, OperationKind::NewBody);
    let state = lathe.doc.revolve_state().unwrap();
    assert_eq!(state.editing, Some("Revolve 1"));
    assert!(state.ready);
    // The preview edits it.
    let (edited, mut draft) = lathe.last_draft().unwrap();
    assert_eq!(edited, Some(feature));
    // A new body that stays one keeps its body, whatever id the draft
    // holds.
    assert_eq!(
        draft.operation,
        varde_document::Operation::NewBody(BodyId::NEW)
    );
    draft.operation = committed.operation.clone();
    assert_eq!(draft, committed);

    // OK with nothing changed writes nothing.
    let revision = lathe.doc.editor.revision();
    lathe.doc.update(Edit::CommitRevolve);
    assert!(lathe.doc.revolve.is_none());
    assert_eq!(lathe.doc.editor.revision(), revision);

    // Changed, it's set again in place, one undo step.
    lathe.doc.look(Look::EditFeature(feature));
    lathe.revolve(RevolveLook::Extent(TurnKind::Full));
    lathe.revolve(RevolveLook::Picking(RevolvePick::Axis));
    let sketch = lathe.sketch;
    lathe.revolve(RevolveLook::PickAxis {
        sketch,
        axis: AxisLine::SketchY,
    });
    lathe.doc.update(Edit::CommitRevolve);
    let [(same, revolve)] = lathe.revolves()[..] else {
        panic!("still one revolve");
    };
    assert_eq!(same, feature);
    assert_eq!(revolve.extent, varde_document::Turn::Full);
    assert_eq!(revolve.axis, AxisLine::SketchY);
    assert_eq!(lathe.doc.editor.document().features().len(), features);
    lathe.doc.update(Edit::Undo);
    let [(_, undone)] = lathe.revolves()[..] else {
        panic!("one revolve");
    };
    assert_eq!(*undone, committed);
}

#[test]
fn a_revolve_whose_axis_line_is_gone_reopens_without_it() {
    let mut lathe = lathe();
    lathe.set_up(AxisLine::Curve(lathe.construction));
    lathe.doc.update(Edit::CommitRevolve);
    let [(feature, _)] = lathe.revolves()[..] else {
        panic!("one revolve");
    };
    // The sketch loses the line; the document keeps the revolve, which
    // regenerating fails.
    let Some(FeatureKind::Sketch { sketch, .. }) = lathe
        .doc
        .editor
        .document()
        .feature(lathe.sketch)
        .map(|f| &f.kind)
    else {
        panic!("no sketch");
    };
    let mut sketch = sketch.clone();
    sketch.delete(&[lathe.construction]);
    lathe.doc.apply(Command::SetSketch {
        feature: lathe.sketch,
        sketch: Box::new(sketch),
    });
    lathe.doc.sync();

    lathe.doc.look(Look::EditFeature(feature));
    let state = lathe.doc.revolve_state().unwrap();
    assert_eq!(state.axis, None);
    assert!(state.axis_missing);
    assert!(!state.ready);
    assert!(lathe.last_draft().is_none());

    let sketch = lathe.sketch;
    lathe.revolve(RevolveLook::PickAxis {
        sketch,
        axis: AxisLine::SketchY,
    });
    let state = lathe.doc.revolve_state().unwrap();
    assert!(!state.axis_missing);
    assert!(state.ready);
}

#[test]
fn the_axis_keeps_its_sketch_and_waits_for_its_line_through_undo() {
    let mut lathe = lathe();
    lathe.set_up(AxisLine::Curve(lathe.construction));
    // Every region taken out, the sketch stays: the axis is on it.
    let (sketch, region) = (lathe.sketch, lathe.rectangle());
    lathe.revolve(RevolveLook::PickRegion { sketch, region });
    let session = lathe.doc.revolve.as_ref().unwrap();
    assert!(session.regions.picked.is_empty());
    assert_eq!(session.regions.source, Some(sketch));
    assert_eq!(lathe.doc.revolve_state().unwrap().candidates.len(), 1);
    lathe.revolve(RevolveLook::PickRegion { sketch, region });
    assert!(lathe.doc.revolve_state().unwrap().ready);

    // The line deleted (in an edit made elsewhere), the revolve isn't
    // whole: no preview, no OK. Undone, the axis is back.
    let Some(FeatureKind::Sketch { sketch: drawn, .. }) =
        lathe.doc.editor.document().feature(sketch).map(|f| &f.kind)
    else {
        panic!("no sketch");
    };
    let mut drawn = drawn.clone();
    drawn.delete(&[lathe.construction]);
    lathe.doc.apply(Command::SetSketch {
        feature: sketch,
        sketch: Box::new(drawn),
    });
    lathe.doc.sync();
    let state = lathe.doc.revolve_state().expect("the session stays");
    assert_eq!(state.axis, None);
    assert!(!state.ready);
    assert!(lathe.last_draft().is_none());
    lathe.doc.update(Edit::Undo);
    let state = lathe.doc.revolve_state().unwrap();
    assert_eq!(state.axis, Some(AxisLine::Curve(lathe.construction)));
    assert!(state.ready);
    assert!(lathe.last_draft().is_some());
}

#[test]
fn while_revolving_other_tools_swap_it() {
    let mut lathe = lathe();
    lathe.doc.look(Look::SelectFeature(lathe.sketch));
    lathe.doc.look(Look::StartRevolve);
    // The selected sketch is the source.
    assert_eq!(
        lathe.doc.revolve.as_ref().unwrap().regions.source,
        Some(lathe.sketch)
    );
    // No edit of the selected feature, nor does the cursor pick the
    // model.
    assert!(press_in(&lathe.doc, enter()).is_none());
    assert!(!lathe.doc.picks());
    // An extrude or a new sketch takes its place, leaving no trace.
    let before = lathe.doc.editor.document().clone();
    let key = |c: &str| keyboard::Key::Character(c.into());
    key_in(&mut lathe.doc, key("x"));
    assert!(lathe.doc.extrude.is_some() && lathe.doc.revolve.is_none());
    lathe.doc.look(Look::StartRevolve);
    assert!(lathe.doc.extrude.is_none() && lathe.doc.revolve.is_some());
    key_in(&mut lathe.doc, key("s"));
    assert!(!lathe.doc.operating() && lathe.doc.picking_plane.is_some());
    assert_eq!(*lathe.doc.editor.document(), before);
    lathe.doc.look(Look::Escape);
    lathe.doc.look(Look::StartRevolve);
    // A read-only document has no session.
    lathe.doc.read_only = Some("read-only".to_owned());
    lathe.doc.sync();
    assert!(lathe.doc.revolve.is_none());
    lathe.doc.look(Look::StartRevolve);
    assert!(lathe.doc.revolve.is_none());
}

/// What clicking each text `text` of `doc`'s screen sends, one click on
/// a fresh screen each, top to bottom.
fn clicking_text(doc: &Doc, text: &str) -> Vec<Vec<varde_view::Message>> {
    use crate::tests::{clicked, shown, texts};

    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(doc.view_in(varde_view::Mode::Light), size, &mut renderer);
    let mut found: Vec<_> = texts(&mut ui, &renderer)
        .into_iter()
        .filter(|shown| shown.text == text && !shown.hidden())
        .collect();
    drop(ui);
    found.sort_by(|a, b| a.bounds.y.total_cmp(&b.bounds.y));
    found
        .iter()
        .map(|shown| {
            let view = doc.view_in(varde_view::Mode::Light);
            let mut ui = crate::tests::shown(view, size, &mut renderer);
            clicked(&mut ui, &mut renderer, shown.bounds.center())
        })
        .collect()
}

#[test]
fn the_toolbar_s_revolve_button_starts_a_session_and_backs_out() {
    use varde_view::Message as Ui;

    let mut lathe = lathe();
    let start = |sent: &Vec<Ui>| matches!(sent[..], [Ui::Look(Look::StartRevolve)]);
    let sent = clicking_text(&lathe.doc, "Revolve");
    assert_eq!(
        sent.iter().filter(|sent| start(sent)).count(),
        1,
        "{sent:?}"
    );
    lathe.doc.look(Look::StartRevolve);
    assert!(lathe.doc.revolve.is_some());
    // While it's set up, the button backs out (the toolbar's tag
    // "Revolve" beside it sends nothing).
    let sent = clicking_text(&lathe.doc, "Revolve");
    assert_eq!(
        sent.iter().filter(|sent| start(sent)).count(),
        1,
        "{sent:?}"
    );
    lathe.doc.look(Look::StartRevolve);
    assert!(lathe.doc.revolve.is_none());
    // An extrude being set up leaves it enabled, swapping the two.
    lathe.doc.look(Look::StartExtrude);
    let sent = clicking_text(&lathe.doc, "Revolve");
    assert!(sent.iter().any(start), "{sent:?}");
    lathe.doc.look(Look::StartRevolve);
    assert!(lathe.doc.extrude.is_none() && lathe.doc.revolve.is_some());
}

#[test]
fn o_starts_a_session_and_backs_out_and_the_rail_s_o_does_too() {
    let mut lathe = lathe();
    let o = || keyboard::Key::Character("o".into());
    key_in(&mut lathe.doc, o());
    assert!(lathe.doc.revolve.is_some());
    key_in(&mut lathe.doc, o());
    assert!(lathe.doc.revolve.is_none());
    // With the Create set's list open, O is its Revolve.
    lathe.doc.look(Look::Rail(varde_view::RailLook::Toggle(0)));
    assert!(matches!(
        press_in(&lathe.doc, o()),
        Some(crate::Message::Ui(varde_view::Message::Look(
            Look::StartRevolve
        )))
    ));
    // In a sketch, O is the Offset tool's.
    lathe.doc.look(Look::Rail(varde_view::RailLook::Close));
    lathe.doc.look(Look::EditFeature(lathe.sketch));
    assert!(matches!(
        press_in(&lathe.doc, o()),
        Some(crate::Message::Ui(varde_view::Message::Look(
            Look::SelectTool(varde_view::Tool::Offset)
        )))
    ));
}

#[test]
fn a_revolve_s_timeline_row_shows_its_turn_and_a_double_click_edits_it() {
    use crate::tests::{clicked, shown, texts};
    use varde_view::Message as Ui;

    let mut lathe = lathe();
    lathe.set_up(AxisLine::SketchY);
    lathe.revolve(RevolveLook::Extent(TurnKind::TwoSides));
    lathe.input(Angle::First, "100");
    lathe.input(Angle::Second, "20");
    lathe.doc.update(Edit::CommitRevolve);
    lathe.answer();
    let [(feature, _)] = lathe.revolves()[..] else {
        panic!("one revolve");
    };
    lathe
        .doc
        .look(Look::SelectPanel(varde_view::Panel::Timeline));

    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(
        lathe.doc.view_in(varde_view::Mode::Light),
        size,
        &mut renderer,
    );
    let shown_texts = texts(&mut ui, &renderer);
    let name = shown_texts
        .iter()
        .find(|text| text.text == "Revolve 1")
        .expect("the revolve's row");
    // Its note, on its row, is the whole turn.
    assert!(
        shown_texts.iter().any(|text| text.text == "120°"
            && (text.bounds.center_y() - name.bounds.center_y()).abs() < 2.0),
        "{shown_texts:?}"
    );
    // Clicked, it's selected; clicked again at once, edited.
    let at = name.bounds.center();
    let mut sent = clicked(&mut ui, &mut renderer, at);
    sent.extend(clicked(&mut ui, &mut renderer, at));
    drop(ui);
    assert!(
        sent.iter()
            .any(|m| matches!(m, Ui::Look(Look::SelectFeature(id)) if *id == feature)),
        "{sent:?}"
    );
    assert!(
        sent.iter()
            .any(|m| matches!(m, Ui::Look(Look::EditFeature(id)) if *id == feature)),
        "{sent:?}"
    );
    for message in sent {
        if let Ui::Look(look) = message {
            lathe.doc.look(look);
        }
    }
    let session = lathe.doc.revolve.as_ref().expect("editing it");
    assert_eq!(session.feature, Some(feature));
    assert_eq!(session.extent, TurnKind::TwoSides);
    assert_eq!(session.axis, Some(AxisLine::SketchY));

    // Selected, its status bar says what it does.
    lathe.revolve(RevolveLook::Cancel);
    lathe.doc.look(Look::SelectFeature(feature));
    let mut ui = shown(
        lathe.doc.view_in(varde_view::Mode::Light),
        size,
        &mut renderer,
    );
    let shown_texts = texts(&mut ui, &renderer);
    assert!(
        shown_texts
            .iter()
            .any(|text| text.text == "Two sides 100° + 20° · New body · about Y axis"),
        "{shown_texts:?}"
    );
}

/// The turn goes the way the viewport's arrow says: right-handed about
/// the axis line from its start to its end, the other way flipped (the
/// arrow on the line's end, or its start, is the view's test). The
/// lathe's sketch is on XZ: its +y is world +Z and the rectangle is on
/// world +X, so a quarter turn about the construction line, along +Z,
/// sweeps it into +Y, and one about the rectangle's left side, along −Z,
/// into −Y.
#[test]
fn the_turn_goes_right_handed_about_the_axis_line_as_the_arrow_points() {
    let mut lathe = lathe();
    // Which side of the XZ plane the preview's mesh is on: the least
    // and the most world y of its vertices.
    let side = |lathe: &mut Lathe| {
        lathe.answer();
        assert_eq!(lathe.doc.feed.draft_error(), None);
        let bounds = lathe.doc.feed.mesh().bounds().expect("a preview");
        (bounds.min.y, bounds.max.y)
    };
    lathe.set_up(AxisLine::Curve(lathe.construction));
    lathe.revolve(RevolveLook::Extent(TurnKind::OneSide));
    lathe.input(Angle::First, "90");
    let (low, high) = side(&mut lathe);
    assert!(low > -1e-3 && high > 19.0, "{low} {high}");
    lathe.revolve(RevolveLook::Flip);
    let (low, high) = side(&mut lathe);
    assert!(low < -19.0 && high < 1e-3, "{low} {high}");

    // About the left side, against +y: the other way round, still
    // flipped, then not.
    let sketch = lathe.sketch;
    lathe.revolve(RevolveLook::Picking(RevolvePick::Axis));
    let axis = AxisLine::Curve(lathe.left);
    lathe.revolve(RevolveLook::PickAxis { sketch, axis });
    let (low, high) = side(&mut lathe);
    assert!(low > -1e-3 && high > 9.0, "{low} {high}");
    lathe.revolve(RevolveLook::Flip);
    let (low, high) = side(&mut lathe);
    assert!(low < -9.0 && high < 1e-3, "{low} {high}");

    // The sketch's Y axis is along +y, as the construction line.
    lathe.revolve(RevolveLook::Picking(RevolvePick::Axis));
    let axis = AxisLine::SketchY;
    lathe.revolve(RevolveLook::PickAxis { sketch, axis });
    let (low, high) = side(&mut lathe);
    assert!(low > -1e-3 && high > 19.0, "{low} {high}");
}

#[test]
fn the_session_ends_when_its_revolve_or_its_document_goes() {
    let mut lathe = lathe();
    lathe.set_up(AxisLine::SketchY);
    lathe.doc.update(Edit::CommitRevolve);
    let [(feature, _)] = lathe.revolves()[..] else {
        panic!("one revolve");
    };
    // Undone away while it's edited, the session ends: OK would write
    // over whatever the id names.
    lathe.doc.look(Look::EditFeature(feature));
    assert!(lathe.doc.revolve.is_some());
    lathe.doc.update(Edit::Undo);
    assert!(lathe.revolves().is_empty());
    assert!(lathe.doc.revolve.is_none());
    assert!(lathe.last_draft().is_none());

    // Replaced whole (restoring recovered changes, say) by one still
    // holding its sketch, a new one's ends too: its ids may name others.
    lathe.set_up(AxisLine::SketchY);
    assert!(lathe.doc.revolve_state().unwrap().ready);
    let mut replaced = Editor::new(lathe.doc.editor.document().clone());
    let coarse = varde_document::Tolerance::new(1e-2).unwrap();
    replaced.apply(Command::SetTolerance(coarse)).unwrap();
    let replaced = replaced.document().clone();
    lathe.doc.apply(Command::Replace(Box::new(replaced)));
    lathe.doc.sync();
    assert!(lathe.doc.revolve.is_none());
    assert!(lathe.last_draft().is_none());
}

mod edges;
mod fuzz;
