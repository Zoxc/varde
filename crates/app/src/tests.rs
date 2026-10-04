use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use glam::Vec3;
use iced::futures::StreamExt;
use iced::futures::executor::block_on;
use iced::time::Instant;
use varde_document::{
    Command, Document, Editor, FeatureId, Generation, LengthUnit, OriginPlane, Plane, Revision,
};
use varde_io::{
    Access, Closing, FileId, Offer, OpenId, Opened, PickedFrom, ReadOnly, SaveError, SaveTo,
};
use varde_regen::{Request, Response, Transport, handle, lane};
use varde_render::Camera;
use varde_solve::{Request as SolveRequest, Response as SolveResponse, Solver};
use varde_view::{Edit, MeshStatus, Panel, RowMenu, Welcome as WelcomeUi};

use super::*;
use crate::doc::{
    AutoSave, CAMERA_ANIMATION, CameraAnimation, Origin, PIVOT_FADE, PIVOT_SHOWN, Picking,
    Recovery, Refusal, Target,
};

#[test]
fn camera_animation_eases_out_and_ends() {
    let from = Camera::default();
    let mut to = from;
    to.zoom(8.0);
    let start = Instant::now();
    let animation = CameraAnimation { from, to, start };

    assert_eq!(animation.at(start), Some(from));
    // Ease-out covers 7/8 of the way at the halfway point: 8^(7/8).
    let mid = animation.at(start + CAMERA_ANIMATION / 2).unwrap();
    assert!((mid.distance() / from.distance() - 8f32.powf(0.875)).abs() < 1e-3);
    assert_eq!(animation.at(start + CAMERA_ANIMATION), None);
}

#[test]
fn the_camera_orbits_the_pivot_picked_until_home() {
    let mut doc = untitled();
    let pivot = Vec3::new(4.0, -2.0, 3.0);
    let screen = |camera: &Camera| {
        let offset = pivot - camera.target();
        (offset.dot(camera.right()), offset.dot(camera.up()))
    };
    doc.look(Look::SetPivot(Some(pivot)));
    // It pans to bring the pivot to the middle; past the marker's fade.
    doc.animation_frame(Instant::now() + 10 * PIVOT_SHOWN);
    assert!(!doc.animating());
    let before = screen(&doc.camera);
    assert!(before.0.abs() < 1e-4 && before.1.abs() < 1e-4, "{before:?}");
    doc.look(Look::Orbit {
        yaw: 0.4,
        pitch: -0.3,
    });
    let after = screen(&doc.camera);
    assert!((after.0 - before.0).abs() < 1e-4 && (after.1 - before.1).abs() < 1e-4);
    // A click off the model orbits the target again, and doesn't pan.
    let target = doc.camera.target();
    doc.look(Look::SetPivot(None));
    assert!(!doc.animating());
    assert_eq!(doc.camera.target(), target);
    doc.look(Look::Orbit {
        yaw: 0.4,
        pitch: -0.3,
    });
    assert_eq!(doc.camera.target(), target);
    // And so does Home.
    doc.look(Look::SetPivot(Some(pivot)));
    doc.look(Look::ResetCamera);
    assert!(doc.pivot_marker().is_none());
    settle_camera(&mut doc);
    let target = doc.camera.target();
    doc.look(Look::Orbit {
        yaw: 0.4,
        pitch: -0.3,
    });
    assert_eq!(doc.camera.target(), target);
}

/// A document holding a disc of `radius` about (25, 20) on XY, extruded
/// 4 mm up into a new body.
fn a_disc_off_the_origin(radius: f64) -> Document {
    let mut editor = Editor::new(Document::default());
    let up = varde_document::Extent::OneSide(length(editor.document(), "4"));
    let new_body = varde_document::Operation::NewBody(varde_document::BodyId::NEW);
    add_disc_of(&mut editor, (25.0, 20.0), radius, up, new_body);
    editor.document().clone()
}

/// Whether `camera` frames the box from `low` to `high`: looking at its
/// middle, the view taller than the box's diagonal but not much more.
fn frames(camera: &Camera, low: Vec3, high: Vec3) -> bool {
    let diagonal = (high - low).length();
    camera.target().abs_diff_eq((low + high) / 2.0, 1e-2)
        && (diagonal..2.0 * diagonal).contains(&camera.view_height())
}

#[test]
fn an_opened_document_frames_its_first_model_unless_the_camera_moved() {
    let opened = || {
        let requests = Rc::default();
        let target = Target::Entry { file: FileId(0) };
        let origin = Origin::new(target, Access::Edit, "part".to_owned());
        let mut doc = Doc::new(a_disc_off_the_origin(5.0), origin);
        doc.feed.connect(Deferred(Rc::clone(&requests)));
        doc.sync();
        (doc, requests)
    };
    let (mut doc, requests) = opened();
    let home = doc.camera;
    answer(&mut doc, &requests);
    let (low, high) = (Vec3::new(20.0, 15.0, 0.0), Vec3::new(30.0, 25.0, 4.0));
    assert!(frames(&doc.camera, low, high), "{:?}", doc.camera);
    assert_eq!(doc.camera.backward(), home.backward());
    // Once only: a later model leaves the camera where the user put it.
    doc.look(Look::Zoom {
        factor: 3.0,
        x: 0.0,
        y: 0.0,
    });
    let zoomed = doc.camera;
    doc.apply(Command::Replace(Box::new(a_disc_off_the_origin(8.0))));
    doc.sync();
    answer(&mut doc, &requests);
    assert_eq!(doc.camera, zoomed);

    // Nor does it move the camera the user moved before the model came.
    let (mut doc, requests) = opened();
    doc.look(Look::Orbit {
        yaw: 0.4,
        pitch: -0.3,
    });
    let moved = doc.camera;
    answer(&mut doc, &requests);
    assert_eq!(doc.camera, moved);

    // A new design starts on the origin, as before.
    let (mut doc, requests) = deferred();
    doc.apply(Command::Replace(Box::new(a_disc_off_the_origin(5.0))));
    doc.sync();
    answer(&mut doc, &requests);
    assert_eq!(doc.camera, home);
}

#[test]
fn home_frames_the_model() {
    let (mut doc, requests) = deferred();
    doc.look(Look::ResetCamera);
    settle_camera(&mut doc);
    let home = doc.camera;
    // A part of 2 × 2 × 4 mm away from the origin, as a first extrude.
    doc.apply(Command::Replace(Box::new(a_disc_off_the_origin(1.0))));
    doc.sync();
    answer(&mut doc, &requests);
    doc.look(Look::ResetCamera);
    settle_camera(&mut doc);
    let (low, high) = (Vec3::new(24.0, 19.0, 0.0), Vec3::new(26.0, 21.0, 4.0));
    assert!(frames(&doc.camera, low, high), "{:?}", doc.camera);
    assert!(doc.camera.view_height() < home.view_height());
    assert_eq!(doc.camera.backward(), home.backward());
}

/// A view cube face looks from its side at what Home looks at, the
/// model's middle, rather than jumping back to the origin; the zoom stays.
#[test]
fn a_view_cube_face_looks_at_the_model_as_home_does() {
    let (mut doc, requests) = deferred();
    doc.apply(Command::Replace(Box::new(a_disc_off_the_origin(1.0))));
    doc.sync();
    answer(&mut doc, &requests);
    doc.look(Look::ResetCamera);
    settle_camera(&mut doc);
    let home = doc.camera;
    doc.look(Look::LookFrom(varde_render::View::Front));
    settle_camera(&mut doc);
    assert!(
        doc.camera.target().abs_diff_eq(home.target(), 1e-4),
        "{:?}",
        doc.camera
    );
    assert!(doc.camera.target().length() > 20.0);
    assert_eq!(doc.camera.view_height(), home.view_height());
    assert_ne!(doc.camera.backward(), home.backward());
}

#[test]
fn the_pivot_marker_fades_after_it_is_picked_and_shows_over_the_cube() {
    let mut doc = untitled();
    assert!(doc.pivot_marker().is_none());
    let start = Instant::now();
    doc.set_pivot(Some(Vec3::ONE), start);
    assert_eq!(doc.pivot_marker().map(|marker| marker.opacity), Some(1.0));
    assert!(doc.animating());
    doc.animation_frame(start + PIVOT_SHOWN);
    assert_eq!(doc.pivot_marker().map(|marker| marker.opacity), Some(1.0));
    doc.animation_frame(start + PIVOT_SHOWN + PIVOT_FADE / 2);
    let half = doc.pivot_marker().unwrap().opacity;
    assert!((half - 0.5).abs() < 0.01, "{half}");
    doc.animation_frame(start + PIVOT_SHOWN + PIVOT_FADE);
    assert!(doc.pivot_marker().is_none());
    assert!(!doc.animating());

    // Over the cube it shows, without frames, for as long as the cursor
    // is there; off it, it fades at once.
    let later = start + 10 * PIVOT_SHOWN;
    doc.hover_cube(true, later);
    assert_eq!(doc.pivot_marker().map(|marker| marker.opacity), Some(1.0));
    assert!(!doc.animating());
    doc.animation_frame(later + 10 * PIVOT_SHOWN);
    assert_eq!(doc.pivot_marker().map(|marker| marker.opacity), Some(1.0));
    let off = later + 20 * PIVOT_SHOWN;
    doc.hover_cube(false, off);
    assert!(doc.animating());
    doc.animation_frame(off + PIVOT_FADE / 2);
    let half = doc.pivot_marker().unwrap().opacity;
    assert!((half - 0.5).abs() < 0.01, "{half}");
    doc.animation_frame(off + PIVOT_FADE);
    assert!(doc.pivot_marker().is_none());

    // With no pivot picked, nothing shows over the cube.
    doc.set_pivot(None, off);
    doc.hover_cube(true, off);
    assert!(doc.pivot_marker().is_none());
    assert!(!doc.animating());
}

#[test]
fn window_icon_renders() {
    assert!(platform::window_icon().is_some());
}

pub(crate) fn untitled() -> Doc {
    Doc::new(
        Document::default(),
        Origin::new(Target::None, Access::Edit, "Untitled".to_owned()),
    )
}

/// Stands in for a lane: hands requests to the test through a shared
/// list, to answer later, when and in what order it likes.
pub(crate) struct Deferred<R>(pub(crate) Rc<RefCell<Vec<R>>>);

impl<R> Transport<R> for Deferred<R> {
    fn send(&mut self, request: R) {
        self.0.borrow_mut().push(request);
    }
}

/// A document whose regeneration requests wait for the test to answer
/// them, and the list they wait in.
pub(crate) fn deferred() -> (Doc, Rc<RefCell<Vec<Request>>>) {
    let requests = Rc::default();
    let mut doc = untitled();
    doc.feed.connect(Deferred(Rc::clone(&requests)));
    doc.sync();
    (doc, requests)
}

/// A document holding the example, "Sketch 1" and "Extrude 1" making
/// "Body 1", answered by the regeneration lane, whose requests wait for
/// the test, and the list they wait in.
pub(crate) fn example() -> (Doc, Rc<RefCell<Vec<Request>>>) {
    let (mut doc, requests) = deferred();
    doc.apply(Command::Replace(Box::new(Document::example())));
    doc.sync();
    answer(&mut doc, &requests);
    (doc, requests)
}

/// The example, and a sketch on XY after it holding a circle of radius 3
/// about (-20, 10), on the plate, selected: its id.
pub(crate) fn example_and_a_hole() -> (Doc, FeatureId, Rc<RefCell<Vec<Request>>>) {
    let (mut doc, requests) = example();
    let plane = varde_document::Plane::Origin(varde_document::OriginPlane::XY);
    doc.apply(doc.editor.document().add_sketch(plane));
    let sketch = doc.editor.document().features().last().unwrap().id;
    let mut drawn = varde_sketch::Sketch::default();
    let center = drawn.add_point(glam::DVec2::new(-20.0, 10.0)).unwrap();
    drawn
        .add_curve(
            varde_sketch::Curve::Circle {
                center,
                radius: 3.0,
            },
            false,
        )
        .unwrap();
    doc.apply(varde_document::Command::SetSketch {
        feature: sketch,
        sketch: Box::new(drawn),
    });
    doc.sync();
    answer(&mut doc, &requests);
    doc.look(Look::SelectFeature(sketch));
    (doc, sketch, requests)
}

/// A document holding `document`, answered by the regeneration lane,
/// whose requests wait for the test, and the list they wait in.
pub(crate) fn holding(document: Document) -> (Doc, Rc<RefCell<Vec<Request>>>) {
    let (mut doc, requests) = deferred();
    doc.apply(Command::Replace(Box::new(document)));
    doc.sync();
    answer(&mut doc, &requests);
    (doc, requests)
}

/// The example's plate, "Body 1", and a 3 mm plate of the same sketch
/// under it, "Body 2" of "Extrude 2": the editor and the two bodies.
pub(crate) fn two_plates() -> (Editor, [varde_document::BodyId; 2]) {
    let mut editor = Editor::new(Document::example());
    let top = editor.document().bodies()[0].id;
    let varde_document::FeatureKind::Extrude(plate) = &editor.document().features()[1].kind else {
        panic!("the example's second feature is its extrude");
    };
    let below = varde_document::Extrude {
        flip: true,
        extent: varde_document::Extent::OneSide(length(editor.document(), "3")),
        ..plate.clone()
    };
    editor
        .apply(editor.document().add_feature(below.into()))
        .unwrap();
    let below = editor.document().bodies()[1].id;
    (editor, [top, below])
}

/// The plates of [`two_plates`], then a sketch with a disc of radius 5
/// about (20, 0) joined 15 mm up and 5 mm down through both, which merges
/// Body 2 into Body 1: the editor, the two bodies and the join.
pub(crate) fn merged_plates() -> (Editor, [varde_document::BodyId; 2], FeatureId) {
    let (mut editor, bodies) = two_plates();
    let join = add_join(&mut editor);
    (editor, bodies, join)
}

/// Adds [`merged_plates`]'s join.
pub(crate) fn add_join(editor: &mut Editor) -> FeatureId {
    let up_and_down = two_sides(editor.document(), "15", "5");
    let join = varde_document::Operation::Join(varde_document::Targets::default());
    add_disc(editor, (20.0, 0.0), up_and_down, join)
}

/// `a` and `b` in `document`'s units, as the extent of two sides.
pub(crate) fn two_sides(document: &Document, a: &str, b: &str) -> varde_document::Extent {
    varde_document::Extent::TwoSides(length(document, a), length(document, b))
}

/// A length of `text` in `document`'s units.
pub(crate) fn length(document: &Document, text: &str) -> varde_expr::Value {
    let ask = varde_document::Extent::ask(&document.design());
    varde_expr::Value::new(text, &ask).unwrap()
}

/// Adds a sketch on XY holding a disc of radius 5 about `center`, and an
/// extrude of it over `extent` with `operation`. The extrude's id.
pub(crate) fn add_disc(
    editor: &mut Editor,
    center: (f64, f64),
    extent: varde_document::Extent,
    operation: varde_document::Operation,
) -> FeatureId {
    add_disc_of(editor, center, 5.0, extent, operation)
}

/// [`add_disc`] of a disc of `radius`.
pub(crate) fn add_disc_of(
    editor: &mut Editor,
    center: (f64, f64),
    radius: f64,
    extent: varde_document::Extent,
    operation: varde_document::Operation,
) -> FeatureId {
    let plane = Plane::Origin(OriginPlane::XY);
    editor.apply(editor.document().add_sketch(plane)).unwrap();
    let feature = editor.document().features().last().unwrap().id;
    let mut sketch = varde_sketch::Sketch::default();
    let center = (sketch.add_point(glam::DVec2::new(center.0, center.1))).unwrap();
    let circle = varde_sketch::Curve::Circle { center, radius };
    sketch.add_curve(circle, false).unwrap();
    let profiles = sketch.profiles().unwrap();
    let regions = (0..profiles.regions.len())
        .map(|index| profiles.reference(index).unwrap())
        .collect();
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    let extrude = varde_document::Extrude {
        sketch: feature,
        regions,
        extent,
        flip: false,
        operation,
    };
    editor
        .apply(editor.document().add_feature(extrude.into()))
        .unwrap();
    editor.document().features().last().unwrap().id
}

/// The texts `doc`'s screen shows at 1280 × 800, light.
pub(crate) fn screen_texts(doc: &Doc) -> Vec<String> {
    let mut renderer = varde_view::probe::renderer();
    let size = iced::Size::new(1280.0, 800.0);
    let mut ui = shown(doc.view_in(Mode::Light), size, &mut renderer);
    (texts(&mut ui, &renderer).into_iter())
        .map(|text| text.text)
        .collect()
}

/// The texts of `doc`'s status bar, left to right, at 1280 × 800, light,
/// with its hints of the mouse if `mouse_hints`.
pub(crate) fn status_bar_texts(doc: &Doc, mouse_hints: bool) -> Vec<String> {
    let size = iced::Size::new(1280.0, 800.0);
    let top = size.height - varde_view::STATUS_BAR_ROOM;
    let mut renderer = varde_view::probe::renderer();
    let options = varde_view::ViewOptions {
        mouse_hints,
        ..Default::default()
    };
    let view = doc.view(false, Mode::Light, options, crate::Offers::default());
    let mut ui = shown(view, size, &mut renderer);
    // It floats over the viewport, right of the side panel.
    let mut bar: Vec<_> = texts(&mut ui, &renderer)
        .into_iter()
        .filter(|text| text.bounds.y >= top && text.bounds.x >= varde_view::SIDE_PANEL_WIDTH)
        .collect();
    bar.sort_by(|a, b| a.bounds.x.total_cmp(&b.bounds.x));
    bar.into_iter().map(|text| text.text).collect()
}

/// Answers the requests waiting, as the lane's messages would.
pub(crate) fn answer(doc: &mut Doc, requests: &RefCell<Vec<Request>>) {
    for request in requests.take() {
        doc.computed(handle(request));
    }
}

/// A test's stand-in for a document's solver lane: requests wait until
/// the test answers them, with a [`Solver`] of its own as the lane would,
/// which keeps the drag in progress.
pub(crate) struct SolveLane {
    requests: Rc<RefCell<Vec<SolveRequest>>>,
    solver: Solver,
}

impl SolveLane {
    /// Starts `doc`'s solver lane, as the lane's first message does.
    pub(crate) fn connect(doc: &mut Doc) -> Self {
        let lane = Self::new();
        doc.solver_ready(lane.transport());
        lane
    }

    /// A lane not connected to a document yet: see
    /// [`SolveLane::transport`].
    pub(crate) fn new() -> Self {
        Self {
            requests: Rc::default(),
            solver: Solver::default(),
        }
    }

    /// What a document sends the lane through.
    pub(crate) fn transport(&self) -> impl Transport<SolveRequest> + 'static {
        Deferred(Rc::clone(&self.requests))
    }

    /// The requests waiting, which stay waiting.
    pub(crate) fn waiting(&self) -> Vec<SolveRequest> {
        self.requests.borrow().clone()
    }

    /// The answer to the first request waiting, if it has one, taking it.
    pub(crate) fn respond(&mut self) -> Option<SolveResponse> {
        let request = {
            let mut requests = self.requests.borrow_mut();
            if requests.is_empty() {
                return None;
            }
            requests.remove(0)
        };
        self.solver.handle(request)
    }

    /// Answers the requests waiting, and those answering them sends, until
    /// none wait.
    pub(crate) fn answer(&mut self, doc: &mut Doc) {
        while !self.requests.borrow().is_empty() {
            self.answer_first(doc);
        }
    }

    /// Answers the first request waiting, if one is.
    pub(crate) fn answer_first(&mut self, doc: &mut Doc) {
        if let Some(response) = self.respond() {
            doc.solved(response);
        }
    }
}

/// An edit through the UI that always changes the document: its units,
/// switched to the next ones.
pub(crate) fn an_edit(doc: &Doc) -> Edit {
    let units = doc.editor.document().units();
    let at = LengthUnit::ALL.iter().position(|&u| u == units).unwrap();
    Edit::SetUnits(LengthUnit::ALL[(at + 1) % LengthUnit::ALL.len()])
}

fn document(varde: &Varde) -> &Doc {
    match &varde.screen {
        Screen::Document(doc) => doc,
        Screen::Welcome(_) => panic!("no document open"),
    }
}

/// A design holding a sketch on XY with one line in it.
fn with_a_line() -> Document {
    let mut editor = Editor::new(Document::default());
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XY)))
        .unwrap();
    let feature = editor.document().features()[0].id;
    let mut sketch = varde_document::Sketch::default();
    let start = sketch.add_point(glam::DVec2::ZERO).unwrap();
    let end = sketch.add_point(glam::DVec2::new(3.0, 1.0)).unwrap();
    sketch
        .add_curve(varde_sketch::Curve::Line { start, end }, false)
        .unwrap();
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    editor.document().clone()
}

/// Draws a line in `doc`, which regenerating shows: one edit, replacing
/// it with [`with_a_line`].
fn draw_line(doc: &mut Doc) {
    doc.apply(Command::Replace(Box::new(with_a_line())));
    doc.sync();
}

/// The model of [`with_a_line`], for generation 1.
fn one_line() -> Response {
    handle(Request::Regenerate {
        generation: Generation::from(1),
        document: Arc::new(with_a_line()),
        exclude: None,
        draft: None,
        inspect: None,
    })
}

#[test]
fn edit_ends_with_the_new_model_shown() {
    let (mut doc, requests) = deferred();
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.generation(), Some(Generation::from(0)));
    assert_eq!(doc.feed.sketches().segment_count(), 0);

    draw_line(&mut doc);
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.status(&doc.editor), MeshStatus::Current);
    assert_eq!(doc.feed.sketches().segment_count(), 1);

    doc.update(Edit::Undo);
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.status(&doc.editor), MeshStatus::Current);
    assert_eq!(doc.feed.sketches().segment_count(), 0);
}

#[test]
fn computed_message_shows_a_late_model() {
    let (mut doc, requests) = deferred();
    answer(&mut doc, &requests);

    draw_line(&mut doc);
    doc.look(Look::Orbit {
        yaw: 0.1,
        pitch: 0.0,
    });
    assert_eq!(doc.feed.status(&doc.editor), MeshStatus::Regenerating);
    assert_eq!(doc.feed.sketches().segment_count(), 0);
    let request = requests.borrow_mut().pop().unwrap();
    assert!(requests.borrow().is_empty());

    doc.computed(handle(request));
    assert_eq!(doc.feed.status(&doc.editor), MeshStatus::Current);
    assert_eq!(doc.feed.sketches().segment_count(), 1);
}

#[test]
fn nothing_is_requested_until_the_lane_is_ready() {
    let mut doc = untitled();
    draw_line(&mut doc);
    assert_eq!(doc.feed.generation(), None);
    assert_eq!(doc.feed.status(&doc.editor), MeshStatus::Regenerating);

    let (lane, mut responses) = lane::spawn();
    doc.lane_ready(lane);
    // Through the regeneration's progress, to its answer.
    let mut answered = |doc: &mut Doc| loop {
        let response = block_on(responses.next()).unwrap();
        let progress = matches!(response, varde_regen::Response::Progress(_));
        doc.computed(response);
        if !progress {
            break;
        }
    };
    answered(&mut doc);
    assert_eq!(doc.feed.status(&doc.editor), MeshStatus::Current);
    assert_eq!(doc.feed.sketches().segment_count(), 1);

    doc.update(Edit::Undo);
    answered(&mut doc);
    assert_eq!(doc.feed.status(&doc.editor), MeshStatus::Current);
    assert_eq!(doc.feed.sketches().segment_count(), 0);
}

#[test]
fn work_for_a_closed_document_is_dropped() {
    let mut varde = Varde::new();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let closed = document(&varde).id;
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let open = document(&varde).id;
    assert_ne!(closed, open);

    let (lane, _responses) = lane::spawn();
    let _ = varde.update(Message::Doc(closed, ForDoc::RegenReady(lane)));
    let _ = varde.update(Message::Doc(closed, ForDoc::Computed(one_line())));
    assert!(!document(&varde).feed.connected());
    assert_eq!(document(&varde).feed.generation(), None);

    let _ = varde.update(Message::Doc(open, ForDoc::Computed(one_line())));
    assert_eq!(
        document(&varde).feed.generation(),
        Some(Generation::from(1))
    );
}

#[test]
fn the_solver_lane_goes_to_its_document() {
    let mut varde = Varde::new();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let closed = document(&varde).id;
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let open = document(&varde).id;

    let (lane, _responses) = varde_solve::lane::spawn();
    let _ = varde.update(Message::Doc(closed, ForDoc::SolveReady(lane.clone())));
    assert!(document(&varde).solver.is_none());
    let _ = varde.update(Message::Doc(open, ForDoc::SolveReady(lane)));
    assert!(document(&varde).solver.is_some());
}

/// An app with its IO lane replaced by a list of the requests sent.
fn with_files() -> (Varde, Rc<RefCell<Vec<IoRequest>>>) {
    let requests = Rc::default();
    let mut varde = Varde::new();
    varde
        .files
        .io
        .ready(Box::new(Deferred(Rc::clone(&requests))));
    (varde, requests)
}

/// The open the app asked for last.
fn last_open(requests: &RefCell<Vec<IoRequest>>) -> (OpenId, PathBuf) {
    requests
        .borrow()
        .iter()
        .rev()
        .find_map(|request| match request {
            IoRequest::Open {
                id,
                from: Chosen::Path(path),
            } => Some((*id, path.clone())),
            _ => None,
        })
        .expect("nothing opened")
}

fn opened(id: OpenId, path: PathBuf, file: u64, access: Access) -> Message {
    Message::Io(IoResponse::Opened {
        id,
        path: Some(path),
        result: Ok(Opened {
            file: FileId(file),
            document: with_a_line(),
            access,
            recovered: Ok(None),
            browser: None,
            not_copied: None,
            download: None,
            damage: None,
        }),
    })
}

fn is_welcome(varde: &Varde) -> bool {
    matches!(varde.screen, Screen::Welcome(_))
}

#[test]
fn recent_files_are_loaded_first() {
    let (_varde, requests) = with_files();
    assert!(matches!(
        requests.borrow()[..],
        [
            IoRequest::LoadRecent,
            IoRequest::ListRecovered,
            IoRequest::LoadSettings,
            IoRequest::LoadPanic
        ]
    ));
}

#[test]
fn an_open_answered_shows_the_document() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Io(IoResponse::RecentLoaded {
        entries: vec![],
        home: None,
    }));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/part.vrdp".into(),
    ))));
    let (id, path) = last_open(&requests);
    let _ = varde.update(opened(id, path.clone(), 0, Access::Edit));
    let doc = document(&varde);
    assert_eq!(doc.name, "part");
    assert_eq!(doc.target().design_file(), Some(FileId(0)));
    assert!(doc.read_only.is_none());
    assert_eq!(varde.files.recent.entries()[0].entry.path, path);
    assert!(matches!(
        requests.borrow().last(),
        Some(IoRequest::WriteRecent { entries }) if entries[0].path == path
    ));

    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    assert!(is_welcome(&varde));
    // Then the thumbnails again, the design maybe saved with another.
    let requests = requests.borrow();
    assert!(matches!(
        requests[requests.len() - 2],
        IoRequest::Close { file, closing: Closing::Clean } if file == FileId(0)
    ));
    assert!(matches!(
        requests.last(),
        Some(IoRequest::LoadThumbnails { paths }) if *paths == [path]
    ));
}

#[test]
fn a_late_open_does_not_replace_a_new_design() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/late.vrdp".into(),
    ))));
    let (id, path) = last_open(&requests);
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    // The open is given up on, so its lock doesn't keep others out, and
    // the new design gets a store entry to auto-save to.
    assert!(matches!(
        requests.borrow()[..],
        [.., IoRequest::Abandon { id: abandoned }, IoRequest::New { .. }] if abandoned == id
    ));
    let sent = requests.borrow().len();
    let _ = varde.update(opened(id, path, 0, Access::Edit));
    assert_eq!(document(&varde).name, "Untitled");
    // The lane closes it by itself.
    assert_eq!(requests.borrow().len(), sent);
}

#[test]
fn only_the_last_open_is_shown() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/first.vrdp".into(),
    ))));
    let (first, first_path) = last_open(&requests);
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/second.vrdp".into(),
    ))));
    let (second, second_path) = last_open(&requests);
    assert_ne!(first, second);

    let sent = requests.borrow().len();
    let _ = varde.update(opened(first, first_path.clone(), 0, Access::Edit));
    assert!(is_welcome(&varde));
    assert_eq!(requests.borrow().len(), sent);

    // A stale failure isn't shown either.
    let _ = varde.update(Message::Io(IoResponse::Opened {
        id: first,
        path: Some(first_path),
        result: Err("gone".to_owned()),
    }));
    let Screen::Welcome(welcome) = &varde.screen else {
        panic!("not on the welcome screen");
    };
    assert!(welcome.error().is_none());

    let _ = varde.update(opened(second, second_path, 1, Access::Edit));
    assert_eq!(document(&varde).name, "second");
}

/// Opening A, then B, then A again before any of them is answered: the
/// first open of A would still hold its lock when the second is handled,
/// making it read-only, unless it's given up on first.
#[test]
fn an_open_given_up_on_is_abandoned_before_the_next() {
    let mut varde = Varde::new();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/a.vrdp".into(),
    ))));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/b.vrdp".into(),
    ))));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/a.vrdp".into(),
    ))));
    let requests = Rc::default();
    varde
        .files
        .io
        .ready(Box::new(Deferred(Rc::clone(&requests))));
    let sent: Vec<String> = requests
        .borrow()
        .iter()
        .map(|request| match request {
            IoRequest::Open {
                id,
                from: Chosen::Path(path),
            } => format!("open {} {}", id.0, path.display()),
            IoRequest::Abandon { id } => format!("abandon {}", id.0),
            request => format!("{request:?}"),
        })
        .collect();
    assert_eq!(
        sent,
        [
            "LoadRecent",
            "ListRecovered",
            "LoadSettings",
            "LoadPanic",
            "open 0 /d/a.vrdp",
            "abandon 0",
            "open 1 /d/b.vrdp",
            "abandon 1",
            "open 2 /d/a.vrdp",
        ]
    );
}

/// A New design that was on its way as the document opened doesn't
/// replace it, which would keep its lock until quitting.
#[test]
fn new_design_is_only_made_from_the_welcome_screen() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/a.vrdp".into(),
    ))));
    let (id, path) = last_open(&requests);
    let _ = varde.update(opened(id, path, 0, Access::Edit));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    assert_eq!(document(&varde).name, "a");
}

#[test]
fn opening_the_same_file_twice_asks_once() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/a.vrdp".into(),
    ))));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/a.vrdp".into(),
    ))));
    let opens = requests
        .borrow()
        .iter()
        .filter(|request| matches!(request, IoRequest::Open { .. }))
        .count();
    assert_eq!(opens, 1);
}

#[test]
fn a_failed_open_shows_why() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/a.vrdp".into(),
    ))));
    let (id, path) = last_open(&requests);
    let _ = varde.update(Message::Io(IoResponse::Opened {
        id,
        path: Some(path),
        result: Err("not a varde file".to_owned()),
    }));
    let Screen::Welcome(welcome) = &varde.screen else {
        panic!("not on the welcome screen");
    };
    assert_eq!(
        welcome.error(),
        Some("Couldn't open /d/a.vrdp: not a varde file")
    );
}

#[test]
fn requests_before_the_lane_starts_wait_for_it() {
    let mut varde = Varde::new();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/a.vrdp".into(),
    ))));
    let requests = Rc::default();
    varde
        .files
        .io
        .ready(Box::new(Deferred(Rc::clone(&requests))));
    assert!(matches!(
        requests.borrow()[..],
        [
            IoRequest::LoadRecent,
            IoRequest::ListRecovered,
            IoRequest::LoadSettings,
            IoRequest::LoadPanic,
            IoRequest::Open { .. }
        ]
    ));
}

#[test]
fn recent_files_are_not_written_before_they_are_loaded() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/a.vrdp".into(),
    ))));
    let (id, path) = last_open(&requests);
    let _ = varde.update(opened(id, path.clone(), 0, Access::Edit));
    let writes = || {
        requests
            .borrow()
            .iter()
            .filter_map(|request| match request {
                IoRequest::WriteRecent { entries } => Some(entries.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert!(writes().is_empty());

    let stored = varde_io::RecentFile {
        path: "/d/b.vrdp".into(),
        opened: varde_io::UnixSeconds(1),
    };
    let _ = varde.update(Message::Io(IoResponse::RecentLoaded {
        entries: vec![varde_io::recent::Listed {
            entry: stored.clone(),
            available: true,
        }],
        home: None,
    }));
    let written = writes();
    assert_eq!(written.len(), 1);
    assert_eq!(written[0][0].path, path);
    assert_eq!(written[0][1], stored);
}

/// The stored theme is taken once it arrives, and one chosen after is
/// stored; Auto follows the system, light when it doesn't say.
#[test]
fn the_theme_is_stored_and_auto_follows_the_system() {
    use varde_io::settings::{Settings as Stored, Theme};
    use varde_view::ThemeChoice;

    let (mut varde, requests) = with_files();
    let _ = sent(&requests);
    assert_eq!(varde.options.theme, ThemeChoice::Auto);
    assert_eq!(varde.mode(), Mode::Light);
    let _ = varde.update(Message::SystemTheme(iced::theme::Mode::Dark));
    assert_eq!(varde.mode(), Mode::Dark);

    let _ = varde.update(Message::Io(IoResponse::SettingsLoaded {
        settings: Stored {
            theme: Theme::Light,
            ..Stored::default()
        },
    }));
    assert_eq!(varde.options.theme, ThemeChoice::Light);
    assert_eq!(varde.mode(), Mode::Light);
    assert!(
        sent(&requests).is_empty(),
        "the stored theme isn't written back"
    );

    let _ = varde.update(Message::Ui(Ui::CycleTheme));
    assert_eq!(varde.mode(), Mode::Dark);
    let _ = varde.update(Message::Ui(Ui::CycleTheme));
    assert_eq!(varde.options.theme, ThemeChoice::Auto);
    let _ = varde.update(Message::SystemTheme(iced::theme::Mode::None));
    assert_eq!(varde.mode(), Mode::Light);
    let themes: Vec<_> = sent(&requests)
        .into_iter()
        .map(|request| match request {
            IoRequest::WriteSettings { settings } => settings.theme,
            request => panic!("not a settings write: {request:?}"),
        })
        .collect();
    assert_eq!(themes, [Theme::Dark, Theme::Auto]);
}

/// The mouse's hints turned off are stored, and come back off.
#[test]
fn the_mouse_hints_are_stored() {
    use varde_io::settings::Settings as Stored;

    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Io(IoResponse::SettingsLoaded {
        settings: Stored {
            mouse_hints: false,
            ..Stored::default()
        },
    }));
    assert!(!varde.options.mouse_hints);
    let _ = sent(&requests);
    let _ = varde.update(Message::Ui(Ui::ToggleMouseHints));
    assert!(varde.options.mouse_hints);
    let stored: Vec<_> = sent(&requests)
        .into_iter()
        .map(|request| match request {
            IoRequest::WriteSettings { settings } => settings.mouse_hints,
            request => panic!("not a settings write: {request:?}"),
        })
        .collect();
    assert_eq!(stored, [true]);
}

#[test]
fn a_read_only_document_refuses_edits_but_moves_the_camera() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/a.vrdp".into(),
    ))));
    let (id, path) = last_open(&requests);
    let _ = varde.update(opened(id, path, 0, Access::ReadOnly(ReadOnly::InUse)));
    assert_eq!(
        document(&varde).read_only.as_deref(),
        Some("the design is open elsewhere")
    );
    let sketch = document(&varde).editor.document().features()[0].id;
    for message in [
        Message::Ui(Ui::Edit(an_edit(document(&varde)))),
        Message::Ui(Ui::Edit(Edit::PlanePicked(OriginPlane::XY))),
        Message::Ui(Ui::Edit(Edit::RemoveFeature(sketch))),
        Message::Ui(Ui::Edit(Edit::ToggleFeatureVisible(sketch))),
        Message::Ui(Ui::Edit(Edit::Undo)),
        Message::Ui(Ui::Edit(Edit::Redo)),
    ] {
        let _ = varde.update(message);
    }
    assert_eq!(document(&varde).editor.revision(), Revision::from(0));

    let camera = document(&varde).camera;
    let _ = varde.update(Message::Ui(Ui::Look(Look::Orbit {
        yaw: 0.3,
        pitch: 0.1,
    })));
    assert_ne!(document(&varde).camera, camera);
}

#[test]
fn a_refused_edit_is_shown() {
    // A document from a file that has used up its ids: no bodies, no
    // features, millimetres, the default tolerance and `next_id` at
    // `u64::MAX` as a postcard varint.
    let mut bytes = vec![0, 0, 0];
    bytes.extend(varde_document::Tolerance::DEFAULT.fit().to_le_bytes());
    bytes.extend([0xff; 9]);
    bytes.push(0x01);
    let full = Document::from_postcard(&bytes).unwrap();
    let mut doc = Doc::new(
        full,
        Origin::new(Target::None, Access::Edit, "Full".to_owned()),
    );
    doc.update(Edit::PlanePicked(OriginPlane::XY));
    assert!(doc.editor.document().features().is_empty());
    assert_eq!(
        doc.edit_error.as_ref().map(ToString::to_string).as_deref(),
        Some("the document has no ids left")
    );
    doc.look(Look::Orbit {
        yaw: 0.1,
        pitch: 0.0,
    });
    assert!(doc.edit_error.is_some());
    doc.update(Edit::Undo);
    assert!(doc.edit_error.is_none());
}

#[test]
fn a_read_only_document_keeps_offering_what_was_recovered() {
    let read_only = Access::ReadOnly(ReadOnly::InUse);
    let origin = Origin {
        recovered: Some(Recovery::Offered(Offer {
            document: Document::default(),
            design_changed: false,
            damage: None,
            newer_base: false,
        })),
        ..Origin::new(Target::None, read_only, "Design".to_owned())
    };
    let mut doc = Doc::new(Document::default(), origin);
    let _ = doc.restore_recovered(&mut Files::new(None));
    assert!(doc.recovered().is_some());
    assert_eq!(doc.editor.revision(), Revision::from(0));
}

/// An app showing `/d/part.vrdp`, opened for editing as file 0, with the
/// requests sent so far forgotten.
fn with_open_file() -> (Varde, Rc<RefCell<Vec<IoRequest>>>) {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Io(IoResponse::RecentLoaded {
        entries: vec![],
        home: None,
    }));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/part.vrdp".into(),
    ))));
    let (id, path) = last_open(&requests);
    let _ = varde.update(opened(id, path, 0, Access::Edit));
    requests.borrow_mut().clear();
    (varde, requests)
}

/// The requests sent since the last call, and forgets them.
fn sent(requests: &RefCell<Vec<IoRequest>>) -> Vec<IoRequest> {
    std::mem::take(&mut *requests.borrow_mut())
}

/// The revision the only request in `sent`, a Save, is of.
fn saving(sent: &[IoRequest]) -> Revision {
    match sent {
        [IoRequest::Save { file, revision, .. }] => {
            assert_eq!(*file, FileId(0));
            *revision
        }
        sent => panic!("not one save: {sent:?}"),
    }
}

fn saved(revision: Revision, result: Result<(), SaveError>) -> Message {
    Message::Io(IoResponse::Saved {
        file: FileId(0),
        revision,
        result,
    })
}

fn window_id() -> window::Id {
    window::Id::unique()
}

/// An app showing `/d/part.vrdp` as [`with_open_file`] does, in a new
/// sketch on XY, with a Point tool in use and a solver lane the test
/// answers, the requests sent so far forgotten.
fn sketching_in_open_file() -> (Varde, Rc<RefCell<Vec<IoRequest>>>, SolveLane) {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::PlanePicked(OriginPlane::XY))));
    let mut lane = SolveLane::new();
    varde
        .screen
        .doc_mut()
        .unwrap()
        .solver_ready(lane.transport());
    solve(&mut varde, &mut lane);
    let _ = varde.update(Message::Ui(Ui::Look(Look::SelectTool(
        varde_view::Tool::Point,
    ))));
    requests.borrow_mut().clear();
    (varde, requests, lane)
}

/// Answers the solver's requests waiting, and those that sends, as its
/// lane's messages would.
fn solve(varde: &mut Varde, lane: &mut SolveLane) {
    let id = document(varde).id;
    while let Some(response) = lane.respond() {
        let _ = varde.update(Message::Doc(id, ForDoc::Solved(response)));
    }
}

/// Places a point with the Point tool, which waits on the solver.
fn place_point(varde: &mut Varde) {
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::ToolClick(
        varde_view::ToolClick {
            at: glam::DVec2::new(1.0, 2.0),
            target: None,
            inference: None,
            pixel: 0.1,
            double: false,
            hit: None,
            reference: false,
        },
    ))));
}

#[test]
fn save_waits_for_the_edits_on_the_solver() {
    let (mut varde, requests, mut lane) = sketching_in_open_file();
    place_point(&mut varde);
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    // Saving, but nothing sent until the point is committed or not.
    assert!(sent(&requests).is_empty());
    assert!(document(&varde).saving());
    assert!(varde.title().contains("Saving…"));
    solve(&mut varde, &mut lane);
    let doc = document(&varde);
    assert_eq!(saving(&sent(&requests)), doc.editor.revision());
    assert_eq!(doc.edited_sketch().unwrap().1.points.len(), 1);
    // Saving again while one waits sends one save.
    place_point(&mut varde);
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    solve(&mut varde, &mut lane);
    assert_eq!(saving(&sent(&requests)), document(&varde).editor.revision());
}

#[test]
fn a_save_waiting_for_an_edit_and_a_delete_behind_it_saves_both() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::PlanePicked(OriginPlane::XY))));
    let feature = document(&varde).sketch.as_ref().unwrap().feature;
    let _ = varde.update(Message::Ui(Ui::Look(Look::SelectTool(
        varde_view::Tool::Point,
    ))));
    // No lane yet: the point waits in the app, the save for it, and the
    // sketch's deletion behind it.
    place_point(&mut varde);
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::RemoveFeature(feature))));
    assert!(
        document(&varde)
            .editor
            .document()
            .feature(feature)
            .is_some()
    );
    assert!(document(&varde).proposing());
    assert!(sent(&requests).is_empty());
    // The lane starts: the point is committed, then the sketch deleted,
    // and then it's saved.
    let mut lane = SolveLane::new();
    varde
        .screen
        .doc_mut()
        .unwrap()
        .solver_ready(lane.transport());
    assert!(sent(&requests).is_empty());
    solve(&mut varde, &mut lane);
    let doc = document(&varde);
    assert!(!doc.proposing());
    assert!(doc.editor.document().feature(feature).is_none());
    assert_eq!(saving(&sent(&requests)), doc.editor.revision());
    // Undoing the deletion brings the sketch back with its point.
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::Undo)));
    let doc = document(&varde);
    let points = match &doc.editor.document().feature(feature).unwrap().kind {
        varde_document::FeatureKind::Sketch { sketch, .. } => sketch.points.len(),
        _ => panic!("a sketch"),
    };
    assert_eq!(points, 1);
}

#[test]
fn undoing_the_edits_a_save_waits_for_saves_at_once() {
    let (mut varde, requests, _lane) = sketching_in_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let edited = document(&varde).editor.revision();
    place_point(&mut varde);
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    assert!(sent(&requests).is_empty());
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::Undo)));
    let doc = document(&varde);
    assert!(!doc.proposing());
    // Only the point is dropped.
    assert_eq!(doc.editor.revision(), edited);
    assert_eq!(saving(&sent(&requests)), edited);
}

#[test]
fn closing_waits_for_the_edits_on_the_solver_then_asks() {
    let (mut varde, requests, mut lane) = sketching_in_open_file();
    // Saved, so only the edit waiting would be lost.
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let revision = saving(&sent(&requests));
    let _ = varde.update(saved(revision, Ok(())));
    place_point(&mut varde);
    assert!(document(&varde).at_stake());
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    assert!(matches!(varde.screen, Screen::Document(_)));
    assert_eq!(document(&varde).prompt(), None);
    solve(&mut varde, &mut lane);
    assert_eq!(document(&varde).prompt(), Some(Leave::Close));
    assert!(sent(&requests).is_empty());
}

#[test]
fn save_sends_the_document_and_marks_it_saved_when_answered() {
    let (mut varde, requests) = with_open_file();
    // Nothing to save yet.
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    assert!(sent(&requests).is_empty());

    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let sent = sent(&requests);
    let revision = saving(&sent);
    let IoRequest::Save { document: sent, .. } = &sent[0] else {
        unreachable!()
    };
    assert_eq!(**sent, *document(&varde).editor.document());
    assert!(varde.title().contains("Saving…"));

    let _ = varde.update(saved(revision, Ok(())));
    assert!(!document(&varde).edited());
    assert_eq!(varde.title(), "part.vrdp — Varde CAD");
}

#[test]
fn edits_made_while_saving_keep_the_document_edited() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let revision = saving(&sent(&requests));
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(saved(revision, Ok(())));
    let doc = document(&varde);
    assert_eq!(doc.saved_revision(), Some(revision));
    assert!(doc.edited());
    assert!(!doc.saves().any());
    assert!(varde.title().contains("Edited"));
}

#[test]
fn undoing_back_to_the_saved_state_leaves_nothing_to_save() {
    let (mut varde, requests) = with_open_file();
    let edit = |varde: &mut Varde, edit| {
        let _ = varde.update(Message::Ui(Ui::Edit(edit)));
    };
    let change = an_edit(document(&varde));
    edit(&mut varde, change);
    assert!(varde.title().contains("Edited"));
    edit(&mut varde, Edit::Undo);
    assert!(!document(&varde).edited());
    assert_eq!(varde.title(), "part.vrdp — Varde CAD");
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    assert!(sent(&requests).is_empty());

    // Likewise back to a state saved since, whichever way.
    edit(&mut varde, Edit::Redo);
    assert!(document(&varde).edited());
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let revision = saving(&sent(&requests));
    let _ = varde.update(saved(revision, Ok(())));
    edit(&mut varde, Edit::Undo);
    assert!(document(&varde).edited());
    edit(&mut varde, Edit::Redo);
    assert!(!document(&varde).edited());
}

/// A save of an older state, undone back to, is the newest save once it
/// lands, though its revision was made first.
#[test]
fn the_save_landing_last_is_what_is_saved() {
    let (mut varde, requests) = with_open_file();
    let edit = |varde: &mut Varde, edit| {
        let _ = varde.update(Message::Ui(Ui::Edit(edit)));
    };
    let change = an_edit(document(&varde));
    edit(&mut varde, change);
    let one_edit = document(&varde).editor.revision();
    let change = an_edit(document(&varde));
    edit(&mut varde, change);
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let two_edits = saving(&sent(&requests));
    edit(&mut varde, Edit::Undo);
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let back = saving(&sent(&requests));
    assert_eq!(back, one_edit);

    let _ = varde.update(saved(two_edits, Ok(())));
    assert!(document(&varde).edited());
    let _ = varde.update(saved(back, Ok(())));
    assert!(!document(&varde).edited());
    assert_eq!(document(&varde).saved_revision(), Some(one_edit));
}

/// Two saves in flight: the lane may drop the first for the second, and
/// "Saving…" lasts until the newest is answered.
#[test]
fn saving_lasts_until_the_newest_save_is_answered() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let first = saving(&sent(&requests));
    // The same revision again isn't sent twice.
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    assert!(sent(&requests).is_empty());
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let second = saving(&sent(&requests));

    let _ = varde.update(saved(first, Ok(())));
    assert!(document(&varde).saves().any());
    assert!(document(&varde).edited());
    let _ = varde.update(saved(second, Ok(())));
    assert!(!document(&varde).saves().any());
    assert!(!document(&varde).edited());
}

#[test]
fn a_conflict_is_shown_with_save_as() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let revision = saving(&sent(&requests));
    let _ = varde.update(saved(revision, Err(SaveError::Conflict)));
    let doc = document(&varde);
    assert_eq!(doc.save_error(), Some(&SaveError::Conflict));
    assert!(doc.edited());
    // The document is kept as it is.
    assert_eq!(doc.editor.revision(), revision);

    // Save As from the banner opens the dialog, and saving there clears it.
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    assert!(document(&varde).picking().is_some());
    let id = document(&varde).id;
    let _ = varde.update(Message::Doc(
        id,
        ForDoc::SaveAsPicked(Some(Chosen::Path("/d/copy".into()))),
    ));
    let sent = sent(&requests);
    let [
        IoRequest::SaveAs {
            file: Some(FileId(0)),
            to: SaveTo::Path {
                path,
                overwrite: false,
            },
            ..
        },
    ] = &sent[..]
    else {
        panic!("not a save as: {sent:?}");
    };
    assert_eq!(path, Path::new("/d/copy.vrdp"));
    assert!(document(&varde).save_error().is_none());

    let _ = varde.update(Message::Ui(Ui::Edit(Edit::DismissSaveError)));
}

#[test]
fn another_save_error_is_shown_and_can_be_dismissed() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let revision = saving(&sent(&requests));
    let _ = varde.update(saved(
        revision,
        Err(SaveError::Failed("disk full".to_owned())),
    ));
    assert_eq!(
        document(&varde).save_error(),
        Some(&SaveError::Failed("disk full".to_owned()))
    );
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::DismissSaveError)));
    assert!(document(&varde).save_error().is_none());
    assert!(document(&varde).edited());
}

#[test]
fn saving_an_untitled_design_asks_where() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Io(IoResponse::RecentLoaded {
        entries: vec![],
        home: None,
    }));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    sent(&requests);
    // Never saved, it has no name: "Not saved" in its place, and none
    // suggested to save it as.
    assert!(document(&varde).unnamed());
    assert_eq!(document(&varde).suggested_name(), "");
    assert_eq!(varde.title(), "Not saved — Edited — Varde CAD");
    let shown = screen_texts(document(&varde));
    assert!(shown.iter().any(|text| text == "Not saved"), "{shown:?}");
    assert!(!shown.iter().any(|text| text == "Untitled"), "{shown:?}");
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    assert!(document(&varde).picking().is_some());
    // The dialog is only shown once.
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    let id = document(&varde).id;
    let _ = varde.update(Message::Doc(
        id,
        ForDoc::SaveAsPicked(Some(Chosen::Path("/d/new.vrdp".into()))),
    ));
    let sent_now = sent(&requests);
    let [
        IoRequest::SaveAs {
            file: None,
            to: SaveTo::Path {
                path,
                overwrite: true,
            },
            revision,
            ..
        },
    ] = &sent_now[..]
    else {
        panic!("not a save as: {sent_now:?}");
    };
    assert_eq!(path, Path::new("/d/new.vrdp"));
    // Saving again waits for it rather than asking again.
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    assert!(document(&varde).picking().is_none());
    assert!(sent(&requests).is_empty());

    let _ = varde.update(Message::Io(IoResponse::SavedAs {
        file: None,
        to: Chosen::Path(path.clone()),
        revision: *revision,
        result: Ok(varde_io::SavedAs {
            file: FileId(4),
            access: Access::Edit,
            offered: false,
        }),
    }));
    let doc = document(&varde);
    assert_eq!(doc.target().design_file(), Some(FileId(4)));
    assert_eq!(doc.name, "new");
    assert!(!doc.unnamed());
    assert_eq!(doc.suggested_name(), "new");
    // Pointing at the file cell shows where it is.
    assert_eq!(
        doc.path.as_deref(),
        Some(Path::new("/d/new.vrdp").to_str().unwrap())
    );
    assert_eq!(varde.title(), "new.vrdp — Varde CAD");
    assert!(!doc.edited());
    assert_eq!(varde.files.recent.entries()[0].entry.path, *path);
    assert!(matches!(
        sent(&requests)[..],
        [IoRequest::WriteRecent { .. }]
    ));
}

#[test]
fn a_read_only_design_is_saved_as_and_becomes_editable() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/a.vrdp".into(),
    ))));
    let (id, path) = last_open(&requests);
    let _ = varde.update(opened(id, path, 2, Access::ReadOnly(ReadOnly::InUse)));
    sent(&requests);
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    assert!(sent(&requests).is_empty());
    assert!(document(&varde).picking().is_none());

    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    let doc_id = document(&varde).id;
    let _ = varde.update(Message::Doc(
        doc_id,
        ForDoc::SaveAsPicked(Some(Chosen::Path("/d/b.vrdp".into()))),
    ));
    assert!(matches!(
        sent(&requests)[..],
        [IoRequest::SaveAs {
            file: Some(FileId(2)),
            ..
        }]
    ));
    let _ = varde.update(Message::Io(IoResponse::SavedAs {
        file: Some(FileId(2)),
        to: Chosen::Path("/d/b.vrdp".into()),
        revision: 0.into(),
        result: Ok(varde_io::SavedAs {
            file: FileId(2),
            access: Access::Edit,
            offered: false,
        }),
    }));
    assert!(document(&varde).read_only.is_none());
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    assert_eq!(document(&varde).editor.revision(), Revision::from(1));
}

#[test]
fn a_save_as_leaving_the_design_read_only_puts_the_sketch_tool_down() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::PlanePicked(OriginPlane::XY))));
    let _ = varde.update(Message::Ui(Ui::Look(Look::SelectTool(
        varde_view::Tool::Line,
    ))));
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::ToolClick(
        varde_view::ToolClick {
            at: glam::DVec2::ZERO,
            target: None,
            inference: None,
            pixel: 0.1,
            double: false,
            hit: None,
            reference: false,
        },
    ))));
    assert!(document(&varde).keys().unwrap().drawing);
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    let doc_id = document(&varde).id;
    let _ = varde.update(Message::Doc(
        doc_id,
        ForDoc::SaveAsPicked(Some(Chosen::Path("/d/b.vrdp".into()))),
    ));
    let [IoRequest::SaveAs { revision, .. }] = sent(&requests)[..] else {
        panic!("not a save as");
    };
    let _ = varde.update(Message::Io(IoResponse::SavedAs {
        file: Some(FileId(0)),
        to: Chosen::Path("/d/b.vrdp".into()),
        revision,
        result: Ok(varde_io::SavedAs {
            file: FileId(0),
            access: Access::ReadOnly(ReadOnly::InUse),
            offered: false,
        }),
    }));
    let doc = document(&varde);
    assert!(doc.read_only.is_some());
    // Still in the sketch, to look at, but without the tool.
    let session = doc.sketch.as_ref().unwrap();
    assert!(session.tool.is_none());
    assert!(!doc.keys().unwrap().drawing);
}

#[test]
fn a_cancelled_save_as_sends_nothing() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    let id = document(&varde).id;
    let _ = varde.update(Message::Doc(id, ForDoc::SaveAsPicked(None)));
    assert!(document(&varde).picking().is_none());
    assert!(sent(&requests).is_empty());
    // One for a document closed since is ignored.
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    let _ = varde.update(Message::Doc(
        DocId::unique(),
        ForDoc::SaveAsPicked(Some(Chosen::Path("/d/x.vrdp".into()))),
    ));
    assert!(document(&varde).picking().is_some());
    assert!(sent(&requests).is_empty());
}

#[test]
fn closing_waits_for_a_save_in_flight() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let revision = saving(&sent(&requests));
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    // Nothing to ask: the edit is on its way to the file.
    assert!(document(&varde).prompt().is_none());
    assert!(sent(&requests).is_empty());
    assert!(varde.title().contains("Saving…"));

    let _ = varde.update(saved(revision, Ok(())));
    assert!(is_welcome(&varde));
    assert!(matches!(
        sent(&requests)[..],
        [
            IoRequest::Close {
                file: FileId(0),
                closing: Closing::Clean
            },
            IoRequest::LoadThumbnails { .. }
        ]
    ));
}

#[test]
fn closing_is_cancelled_if_the_save_it_waits_for_fails() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let revision = saving(&sent(&requests));
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = varde.update(saved(revision, Err(SaveError::Conflict)));
    let doc = document(&varde);
    assert!(doc.leaving().is_none());
    assert_eq!(doc.save_error(), Some(&SaveError::Conflict));
    assert!(sent(&requests).is_empty());
    // Closing again asks about the changes.
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    assert_eq!(document(&varde).prompt(), Some(Leave::Close));
}

/// A save failing while a newer one is in flight is superseded: the newer
/// one's outcome decides, so it neither shows nor cancels leaving.
#[test]
fn a_failed_save_superseded_by_a_newer_one_is_not_shown() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let first = saving(&sent(&requests));
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let second = saving(&sent(&requests));
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = varde.update(saved(first, Err(SaveError::Failed("disk full".to_owned()))));
    assert_eq!(save_error(&varde), None);
    assert!(document(&varde).leaving().is_some());

    let _ = varde.update(saved(second, Ok(())));
    assert!(is_welcome(&varde));
}

/// Then the newest save failing too is shown, and cancels leaving.
#[test]
fn the_newest_save_failing_is_shown_and_cancels_leaving() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let first = saving(&sent(&requests));
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let second = saving(&sent(&requests));
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = varde.update(saved(first, Err(SaveError::Conflict)));
    let _ = varde.update(saved(second, Err(SaveError::Conflict)));
    let doc = document(&varde);
    assert_eq!(doc.save_error(), Some(&SaveError::Conflict));
    assert!(doc.leaving().is_none());
    assert!(sent(&requests).is_empty());
}

/// A Save As in flight supersedes a Save sent before it, like a Save does.
#[test]
fn a_save_as_supersedes_a_failed_save() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let revision = saving(&sent(&requests));
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    let id = document(&varde).id;
    let _ = varde.update(Message::Doc(
        id,
        ForDoc::SaveAsPicked(Some(Chosen::Path("/d/copy.vrdp".into()))),
    ));
    sent(&requests);
    let _ = varde.update(saved(revision, Err(SaveError::Conflict)));
    assert_eq!(save_error(&varde), None);
    let _ = varde.update(Message::Io(IoResponse::SavedAs {
        file: Some(FileId(0)),
        to: Chosen::Path("/d/copy.vrdp".into()),
        revision,
        result: Ok(varde_io::SavedAs {
            file: FileId(0),
            access: Access::Edit,
            offered: false,
        }),
    }));
    assert_eq!(save_error(&varde), None);
    assert!(!document(&varde).edited());
}

/// A Save sent while a Save As is in flight writes the design's own file,
/// not the one the Save As was to write: it doesn't supersede the Save As
/// failing, which is shown, cancels leaving, and stays once the Save is
/// done.
#[test]
fn a_save_does_not_supersede_a_failed_save_as() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    let id = document(&varde).id;
    let _ = varde.update(Message::Doc(
        id,
        ForDoc::SaveAsPicked(Some(Chosen::Path("/d/copy.vrdp".into()))),
    ));
    let [IoRequest::SaveAs { revision, .. }] = sent(&requests)[..] else {
        panic!("no Save As");
    };
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    assert_eq!(saving(&sent(&requests)), revision);
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = varde.update(Message::Io(IoResponse::SavedAs {
        file: Some(FileId(0)),
        to: Chosen::Path("/d/copy.vrdp".into()),
        revision,
        result: Err(SaveError::Failed("copy.vrdp already exists".to_owned())),
    }));
    assert_eq!(save_error(&varde), Some("copy.vrdp already exists"));
    assert!(document(&varde).leaving().is_none());

    let _ = varde.update(saved(revision, Ok(())));
    assert_eq!(save_error(&varde), Some("copy.vrdp already exists"));
    assert!(!document(&varde).edited());
    assert_eq!(document(&varde).name, "part");
}

/// A save answered after an auto-save failed writes all there was to
/// protect: the banner goes, and comes back should auto-saving fail again.
#[test]
fn a_successful_save_clears_an_auto_save_error() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let revision = saving(&sent(&requests));
    let _ = varde.update(Message::Io(IoResponse::AutoSaved {
        file: FileId(0),
        revision,
        result: Err("no room".to_owned()),
    }));
    assert!(save_error(&varde).is_some_and(|error| error.contains("no room")));
    let _ = varde.update(saved(revision, Ok(())));
    assert_eq!(save_error(&varde), None);
}

/// A later auto-save succeeding clears an auto-save error, but not a save
/// error: the design's own file still isn't saved.
#[test]
fn a_successful_auto_save_clears_only_an_auto_save_error() {
    let (mut varde, requests) = with_open_file();
    let auto_saved = |result| {
        Message::Io(IoResponse::AutoSaved {
            file: FileId(0),
            revision: 1.into(),
            result,
        })
    };
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(auto_saved(Err("no room".to_owned())));
    assert!(save_error(&varde).is_some());
    let _ = varde.update(auto_saved(Ok(())));
    assert_eq!(save_error(&varde), None);

    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let revision = saving(&sent(&requests));
    let _ = varde.update(saved(revision, Err(SaveError::Conflict)));
    let _ = varde.update(auto_saved(Ok(())));
    assert_eq!(document(&varde).save_error(), Some(&SaveError::Conflict));
}

#[test]
fn closing_unsaved_changes_asks_and_cancel_stays() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    assert_eq!(document(&varde).prompt(), Some(Leave::Close));
    let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Cancel))));
    let doc = document(&varde);
    assert!(doc.prompt().is_none());
    assert!(doc.leaving().is_none());
    assert!(doc.edited());
    assert!(sent(&requests).is_empty());
}

/// Under the unsaved changes prompt, the status bar's hints are only that
/// `Esc` cancels it: no other key reaches the document behind it.
#[test]
fn the_unsaved_changes_prompt_hints_only_its_keys() {
    let (mut varde, _requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let texts = screen_texts(document(&varde));
    assert!(!texts.contains(&"Select".to_owned()), "{texts:?}");
    assert!(texts.contains(&"Esc".to_owned()), "{texts:?}");
    // The prompt's button and the hint.
    let cancels = texts.iter().filter(|text| *text == "Cancel").count();
    assert_eq!(cancels, 2, "{texts:?}");
}

/// Leaving is stepped through on the document alone, the app only doing
/// what each step says to next.
#[test]
fn a_document_says_when_it_is_left() {
    let mut cx = Files::new(None);
    let file = FileId(4);
    let mut doc = Doc::new(
        Document::default(),
        Origin::new(
            Target::File { file, picked: None },
            Access::Edit,
            "Part".to_owned(),
        ),
    );
    doc.update(an_edit(&doc));

    assert!(matches!(doc.leave(&mut cx, Leave::Close), Next::Stay));
    assert_eq!(doc.prompt(), Some(Leave::Close));
    let quit = Leave::Quit(window_id());
    assert!(matches!(doc.leave(&mut cx, quit), Next::Stay));
    assert_eq!(doc.prompt(), Some(quit));
    assert!(matches!(
        doc.answer_unsaved(&mut cx, Unsaved::Discard),
        Next::Left(left) if left == quit
    ));
    assert!(matches!(
        cx.io.waiting().last(),
        Some(IoRequest::Close { file: closed, closing: Closing::Clean }) if *closed == file
    ));
}

#[test]
fn closing_unsaved_changes_without_saving() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Discard))));
    assert!(is_welcome(&varde));
    assert!(matches!(
        sent(&requests)[..],
        [
            IoRequest::Close {
                file: FileId(0),
                closing: Closing::Clean
            },
            IoRequest::LoadThumbnails { .. }
        ]
    ));
}

#[test]
fn closing_unsaved_changes_saves_first() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Save))));
    let revision = saving(&sent(&requests));
    assert!(!is_welcome(&varde));
    let _ = varde.update(saved(revision, Ok(())));
    assert!(is_welcome(&varde));
    assert!(matches!(
        sent(&requests)[..],
        [
            IoRequest::Close {
                file: FileId(0),
                closing: Closing::Clean
            },
            IoRequest::LoadThumbnails { .. }
        ]
    ));
}

#[test]
fn closing_an_untitled_design_saves_as_first_or_stays() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Save))));
    assert!(document(&varde).picking().is_some());
    // Backing out of the dialog stays.
    let id = document(&varde).id;
    let _ = varde.update(Message::Doc(id, ForDoc::SaveAsPicked(None)));
    assert!(document(&varde).leaving().is_none());
    assert!(!is_welcome(&varde));

    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Save))));
    let _ = varde.update(Message::Doc(
        id,
        ForDoc::SaveAsPicked(Some(Chosen::Path("/d/n.vrdp".into()))),
    ));
    let revision = document(&varde).editor.revision();
    sent(&requests);
    let _ = varde.update(Message::Io(IoResponse::SavedAs {
        file: None,
        to: Chosen::Path("/d/n.vrdp".into()),
        revision,
        result: Ok(varde_io::SavedAs {
            file: FileId(3),
            access: Access::Edit,
            offered: false,
        }),
    }));
    assert!(is_welcome(&varde));
    let sent = sent(&requests);
    assert!(
        sent.iter().any(|request| matches!(
            request,
            IoRequest::Close {
                file: FileId(3),
                closing: Closing::Clean
            }
        )),
        "{sent:?}"
    );
}

/// Quitting: unsaved changes are asked about, then the lane closes the
/// file and flushes before the window closes.
#[test]
fn quitting_closes_the_file_and_waits_for_the_lane() {
    let (mut varde, requests) = with_open_file();
    let window = window_id();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::CloseRequested(window));
    assert_eq!(document(&varde).prompt(), Some(Leave::Quit(window)));
    assert!(varde.quitting.is_none());

    let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Save))));
    let revision = saving(&sent(&requests));
    // Asking again while waiting changes nothing.
    let _ = varde.update(Message::CloseRequested(window));
    assert!(sent(&requests).is_empty());
    let _ = varde.update(saved(revision, Ok(())));
    assert_eq!(varde.quitting, Some(window));
    assert!(matches!(
        sent(&requests)[..],
        [
            IoRequest::Close {
                file: FileId(0),
                closing: Closing::Clean
            },
            IoRequest::Flush
        ]
    ));
    // Nothing more is asked of the lane.
    let _ = varde.update(Message::CloseRequested(window));
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    assert!(sent(&requests).is_empty());

    let _ = varde.update(Message::Io(IoResponse::Closed {
        file: FileId(0),
        result: Ok(()),
    }));
    let _ = varde.update(Message::Io(IoResponse::Flushed));
}

#[test]
fn quitting_from_the_welcome_screen_flushes_first() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/a.vrdp".into(),
    ))));
    let (id, _) = last_open(&requests);
    sent(&requests);
    let window = window_id();
    let _ = varde.update(Message::CloseRequested(window));
    assert_eq!(varde.quitting, Some(window));
    let sent = sent(&requests);
    assert!(
        matches!(&sent[..], [IoRequest::Abandon { id: abandoned }, IoRequest::Flush] if *abandoned == id),
        "{sent:?}"
    );
}

/// Asked to quit while asking about closing: the answer then quits.
#[test]
fn quitting_while_asked_about_closing_quits() {
    let (mut varde, requests) = with_open_file();
    let window = window_id();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = varde.update(Message::CloseRequested(window));
    assert_eq!(document(&varde).prompt(), Some(Leave::Quit(window)));
    let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Discard))));
    assert_eq!(varde.quitting, Some(window));
    assert!(matches!(
        sent(&requests)[..],
        [
            IoRequest::Close {
                file: FileId(0),
                closing: Closing::Clean
            },
            IoRequest::Flush
        ]
    ));
}

/// Writes a design file holding `document` at `path`.
fn write_design(path: &std::path::Path, document: &Document) {
    let (bytes, _) = varde_io::vrdp::to_bytes(document, &[]).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// The whole save round trip through the real lane: open, edit, save,
/// close, and the reopened design has the edit.
#[test]
fn a_saved_edit_is_there_on_reopening() {
    let dir = std::env::temp_dir().join(format!("varde-app-save-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("part.vrdp");
    write_design(&path, &with_a_line());

    // Not the user's recent files.
    let (lane, mut responses) = varde_io::lane::spawn_at(varde_io::Stores::default());
    let mut varde = Varde::new();
    let _ = varde.update(Message::IoReady(lane));
    let mut answer = |varde: &mut Varde, until: fn(&IoResponse) -> bool| loop {
        let response = block_on(responses.next()).unwrap();
        let done = until(&response);
        let _ = varde.update(Message::Io(response));
        if done {
            break;
        }
    };
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(path.clone()))));
    answer(&mut varde, |r| matches!(r, IoResponse::Opened { .. }));
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let edited = document(&varde).editor.document().clone();
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    answer(&mut varde, |r| matches!(r, IoResponse::Saved { .. }));
    assert!(!document(&varde).edited());
    assert!(document(&varde).save_error().is_none());
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    assert!(is_welcome(&varde));
    answer(&mut varde, |r| matches!(r, IoResponse::Closed { .. }));

    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(path.clone()))));
    answer(&mut varde, |r| matches!(r, IoResponse::Opened { .. }));
    assert_eq!(*document(&varde).editor.document(), edited);
    assert!(document(&varde).read_only.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The document screen builds with the save error banner, the file menu
/// and the prompt about unsaved changes showing.
#[test]
fn saving_state_is_shown() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let revision = saving(&sent(&requests));
    let _ = varde.view();
    let _ = varde.update(saved(revision, Err(SaveError::Conflict)));
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::ToggleFileMenu)));
    let _ = varde.view();
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    assert!(!document(&varde).file_menu);
    let _ = varde.view();
}

/// A Save As of a design never saved is on its way: another Save As waits
/// for it, like Save does. Otherwise both would make a new file, and the
/// lane would keep the first one open and locked for good, its saves'
/// answers ignored and "Saving…" stuck.
#[test]
fn a_new_design_is_not_saved_as_twice_at_once() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    let id = document(&varde).id;
    let _ = varde.update(Message::Doc(
        id,
        ForDoc::SaveAsPicked(Some(Chosen::Path("/d/a.vrdp".into()))),
    ));
    sent(&requests);
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    assert!(document(&varde).picking().is_none());
    assert!(sent(&requests).is_empty());

    // Once it has a file, Save As is offered again.
    let revision = document(&varde).editor.revision();
    let _ = varde.update(Message::Io(IoResponse::SavedAs {
        file: None,
        to: Chosen::Path("/d/a.vrdp".into()),
        revision,
        result: Ok(varde_io::SavedAs {
            file: FileId(5),
            access: Access::Edit,
            offered: false,
        }),
    }));
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    assert!(document(&varde).picking().is_some());
}

/// Backing out of a Save As dialog the close wasn't waiting for doesn't
/// cancel the close, which goes on once the save it waits for is done.
#[test]
fn backing_out_of_save_as_keeps_closing_for_a_save_in_flight() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let revision = saving(&sent(&requests));
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let id = document(&varde).id;
    let _ = varde.update(Message::Doc(id, ForDoc::SaveAsPicked(None)));
    assert!(document(&varde).leaving().is_some());
    let _ = varde.update(saved(revision, Ok(())));
    assert!(is_welcome(&varde));
}

/// A document that became read-only with edits made meanwhile, e.g. saved
/// as to where no lock file can be made: Save in the prompt about them
/// asks where to save, since the file can't be saved to.
#[test]
fn closing_a_read_only_design_with_edits_saves_as() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    let id = document(&varde).id;
    let _ = varde.update(Message::Doc(
        id,
        ForDoc::SaveAsPicked(Some(Chosen::Path("/r/b.vrdp".into()))),
    ));
    let revision = document(&varde).editor.revision();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    sent(&requests);
    let _ = varde.update(Message::Io(IoResponse::SavedAs {
        file: Some(FileId(0)),
        to: Chosen::Path("/r/b.vrdp".into()),
        revision,
        result: Ok(varde_io::SavedAs {
            file: FileId(0),
            access: Access::ReadOnly(ReadOnly::NoLock("no lock".to_owned())),
            offered: false,
        }),
    }));
    assert!(document(&varde).edited());

    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Save))));
    let doc = document(&varde);
    assert!(doc.picking().is_some());
    assert!(doc.prompt().is_none());
    // Backing out of it stays.
    let _ = varde.update(Message::Doc(id, ForDoc::SaveAsPicked(None)));
    assert!(document(&varde).leaving().is_none());
    assert!(
        sent(&requests)
            .iter()
            .all(|request| matches!(request, IoRequest::WriteRecent { .. }))
    );
}

/// Once quitting, nothing more is sent to the lane: it would come after
/// the flush the window closes on, and be cut off.
#[test]
fn nothing_is_saved_once_quitting() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    let window = window_id();
    let _ = varde.update(Message::CloseRequested(window));
    assert_eq!(varde.quitting, Some(window));
    sent(&requests);
    let id = document(&varde).id;
    let _ = varde.update(Message::Doc(
        id,
        ForDoc::SaveAsPicked(Some(Chosen::Path("/d/late.vrdp".into()))),
    ));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    let _ = varde.update(Message::Ui(Ui::File(File::DiscardChanges)));
    let _ = varde.update(Message::PageLeaving);
    assert!(sent(&requests).is_empty());
}

/// Nor from the welcome screen, which opens or makes nothing more.
#[test]
fn nothing_is_opened_once_quitting() {
    let (mut varde, requests) = with_files();
    let window = window_id();
    let _ = varde.update(Message::CloseRequested(window));
    assert_eq!(varde.quitting, Some(window));
    sent(&requests);
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/a.vrdp".into(),
    ))));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenStored(
        "/d/b.vrdp".into(),
    ))));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::DiscardStored(
        "/d/b.vrdp".into(),
    ))));
    assert!(sent(&requests).is_empty());
    assert!(matches!(varde.screen, Screen::Welcome(_)));
}

const SECOND: Duration = Duration::from_secs(1);

#[test]
fn auto_save_waits_for_a_pause_in_editing() {
    let start = Instant::now();
    let at = |seconds: u64| start + SECOND * seconds as u32;
    let mut auto_save = AutoSave {
        sent: Some(Revision::from(0)),
        ..AutoSave::default()
    };
    // Nothing edited.
    assert!(!auto_save.due(Revision::from(0), true, at(0)));
    assert!(!auto_save.due(Revision::from(0), true, at(100)));
    // Edited at 101 and 102, then left alone.
    assert!(!auto_save.due(Revision::from(1), true, at(101)));
    assert!(!auto_save.due(Revision::from(2), true, at(102)));
    assert!(!auto_save.due(Revision::from(2), true, at(104)));
    assert!(auto_save.due(Revision::from(2), true, at(105)));
    // Sent: not again.
    assert!(!auto_save.due(Revision::from(2), true, at(200)));
    // Not wanted, e.g. saved meanwhile, or read-only.
    assert!(!auto_save.due(Revision::from(3), false, at(201)));
    assert!(!auto_save.due(Revision::from(3), false, at(300)));
    // Wanted again: the pause counts from when it was first seen wanted.
    assert!(!auto_save.due(Revision::from(3), true, at(301)));
    assert!(auto_save.due(Revision::from(3), true, at(304)));
}

#[test]
fn auto_save_comes_every_two_minutes_while_edits_keep_coming() {
    let start = Instant::now();
    let mut auto_save = AutoSave {
        sent: Some(Revision::from(0)),
        ..AutoSave::default()
    };
    let mut saved = Vec::new();
    // An edit every second for five minutes.
    for second in 1..=300u64 {
        if auto_save.due(Revision::from(second), true, start + SECOND * second as u32) {
            saved.push(second);
        }
    }
    // The first edit after one auto-save is seen a second later.
    assert_eq!(saved, [121, 242]);
}

/// A tick of the auto-save timer, `seconds` after `start`.
fn tick(varde: &mut Varde, start: Instant, seconds: u32) {
    let _ = varde.update(Message::AutoSaveTick(start + SECOND * seconds));
}

/// The auto-saves in `sent`, as `(file, revision)`.
fn auto_saves(sent: &[IoRequest]) -> Vec<(u64, u64)> {
    sent.iter()
        .filter_map(|request| match request {
            IoRequest::AutoSave {
                file,
                revision,
                document,
            } => {
                assert_eq!(document.check(), Ok(()));
                Some((file.0, u64::from(*revision)))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn an_edited_document_is_auto_saved_once_idle() {
    let (mut varde, requests) = with_open_file();
    let start = Instant::now();
    tick(&mut varde, start, 0);
    tick(&mut varde, start, 10);
    assert!(sent(&requests).is_empty());

    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    tick(&mut varde, start, 11);
    tick(&mut varde, start, 13);
    assert!(sent(&requests).is_empty());
    tick(&mut varde, start, 14);
    let sent_now = sent(&requests);
    assert_eq!(auto_saves(&sent_now), [(0, 1)]);
    let IoRequest::AutoSave {
        document: snapshot, ..
    } = &sent_now[0]
    else {
        unreachable!()
    };
    assert_eq!(**snapshot, *document(&varde).editor.document());
    // Auto-saving isn't saving: the document is still edited.
    assert!(document(&varde).edited());
    assert!(!varde.title().contains("Saving"));
    tick(&mut varde, start, 30);
    assert!(sent(&requests).is_empty());

    // A saved document isn't auto-saved, nor one on its way to its file.
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let revision = saving(&sent(&requests));
    tick(&mut varde, start, 31);
    tick(&mut varde, start, 40);
    assert!(sent(&requests).is_empty());
    let _ = varde.update(saved(revision, Ok(())));
    tick(&mut varde, start, 50);
    assert!(sent(&requests).is_empty());

    // Nor once quitting, as it would come after the flush.
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    tick(&mut varde, start, 51);
    let _ = varde.update(Message::CloseRequested(window_id()));
    let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Discard))));
    sent(&requests);
    tick(&mut varde, start, 60);
    assert!(sent(&requests).is_empty());
}

/// Undoing back to the saved state takes back what was auto-saved of the
/// edits undone, so a crash doesn't offer them, and redoing auto-saves
/// again. Nothing to take back once a save emptied it, unless it failed.
#[test]
fn undoing_back_to_the_saved_state_empties_what_was_auto_saved() {
    let (mut varde, requests) = with_open_file();
    let start = Instant::now();
    let edit = |varde: &mut Varde, edit| {
        let _ = varde.update(Message::Ui(Ui::Edit(edit)));
    };
    let change = an_edit(document(&varde));
    edit(&mut varde, change);
    tick(&mut varde, start, 0);
    tick(&mut varde, start, 3);
    assert_eq!(auto_saves(&sent(&requests)), [(0, 1)]);
    edit(&mut varde, Edit::Undo);
    tick(&mut varde, start, 4);
    assert!(matches!(
        sent(&requests)[..],
        [IoRequest::DiscardRecovery { file: FileId(0) }]
    ));
    tick(&mut varde, start, 10);
    assert!(sent(&requests).is_empty());
    edit(&mut varde, Edit::Redo);
    tick(&mut varde, start, 11);
    tick(&mut varde, start, 14);
    assert_eq!(auto_saves(&sent(&requests)), [(0, 1)]);

    // Saved, then undone and redone back to it before an auto-save.
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let revision = saving(&sent(&requests));
    let _ = varde.update(saved(revision, Ok(())));
    edit(&mut varde, Edit::Undo);
    edit(&mut varde, Edit::Redo);
    tick(&mut varde, start, 20);
    tick(&mut varde, start, 30);
    assert!(sent(&requests).is_empty());

    // A save failing leaves what was auto-saved: the state it was of is
    // auto-saved in its place.
    let change = an_edit(document(&varde));
    edit(&mut varde, change);
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let revision = saving(&sent(&requests));
    let _ = varde.update(saved(revision, Err(SaveError::Failed("full".into()))));
    tick(&mut varde, start, 31);
    tick(&mut varde, start, 34);
    assert_eq!(auto_saves(&sent(&requests)), [(0, u64::from(revision))]);
}

#[test]
fn a_read_only_design_is_never_auto_saved() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/a.vrdp".into(),
    ))));
    let (id, path) = last_open(&requests);
    let _ = varde.update(opened(id, path, 0, Access::ReadOnly(ReadOnly::InUse)));
    assert!(!varde.auto_saving());
    // Even if it had changes, e.g. edited before a Save As made it so.
    let Screen::Document(doc) = &mut varde.screen else {
        unreachable!()
    };
    doc.editor
        .apply(Command::RemoveFeature(
            doc.editor.document().features()[0].id,
        ))
        .unwrap();
    sent(&requests);
    let start = Instant::now();
    tick(&mut varde, start, 0);
    tick(&mut varde, start, 10);
    assert!(sent(&requests).is_empty());
}

/// `/d/part.vrdp` opened as file 0, with `recovered` left by a crash.
fn with_recovered(recovered: Document) -> (Varde, Rc<RefCell<Vec<IoRequest>>>) {
    with_recovered_changed(recovered, false)
}

/// [`with_recovered`], the design changed since if `changed`.
fn with_recovered_changed(
    recovered: Document,
    changed: bool,
) -> (Varde, Rc<RefCell<Vec<IoRequest>>>) {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/d/part.vrdp".into(),
    ))));
    let (id, path) = last_open(&requests);
    let _ = varde.update(Message::Io(IoResponse::Opened {
        id,
        path: Some(path),
        result: Ok(Opened {
            file: FileId(0),
            document: with_a_line(),
            access: Access::Edit,
            recovered: Ok(Some(Offer {
                document: recovered,
                design_changed: changed,
                damage: None,
                newer_base: false,
            })),
            browser: None,
            not_copied: None,
            download: None,
            damage: None,
        }),
    }));
    requests.borrow_mut().clear();
    (varde, requests)
}

#[test]
fn recovered_changes_are_restored_as_one_edit() {
    let (mut varde, requests) = with_recovered(Document::default());
    assert!(document(&varde).recovered().is_some());
    assert!(!document(&varde).edited());
    let _ = varde.view();

    // Auto-saves wait for the answer, so as not to replace them.
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let start = Instant::now();
    tick(&mut varde, start, 0);
    tick(&mut varde, start, 10);
    assert!(sent(&requests).is_empty());

    let _ = varde.update(Message::Ui(Ui::File(File::RestoreChanges)));
    let doc = document(&varde);
    assert!(doc.recovered().is_none());
    assert_eq!(*doc.editor.document(), Document::default());
    assert!(doc.edited());
    assert_eq!(doc.editor.revision(), Revision::from(2));
    tick(&mut varde, start, 11);
    tick(&mut varde, start, 14);
    assert_eq!(auto_saves(&sent(&requests)), [(0, 2)]);
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::Undo)));
    assert_eq!(document(&varde).editor.document().features().len(), 1);

    // Closing now discards it all.
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Discard))));
    assert!(matches!(
        sent(&requests)[..],
        [
            IoRequest::Close {
                file: FileId(0),
                closing: Closing::Clean
            },
            IoRequest::LoadThumbnails { .. }
        ]
    ));
}

#[test]
fn restoring_recovered_changes_drops_the_edits_waiting() {
    // What was recovered has a sketch of its own where the one drawn in
    // now is.
    let mut editor = Editor::new(with_a_line());
    editor
        .apply(
            editor
                .document()
                .add_sketch(varde_document::Plane::Origin(OriginPlane::XY)),
        )
        .unwrap();
    let feature = editor.document().features().last().unwrap().id;
    let mut sketch = varde_sketch::Sketch::default();
    sketch.add_point(glam::DVec2::new(5.0, 5.0)).unwrap();
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    let recovered = editor.document().clone();
    let (mut varde, requests) = with_recovered(recovered.clone());
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::PlanePicked(OriginPlane::XY))));
    assert_eq!(document(&varde).sketch.as_ref().unwrap().feature, feature);
    let mut lane = SolveLane::new();
    varde
        .screen
        .doc_mut()
        .unwrap()
        .solver_ready(lane.transport());
    solve(&mut varde, &mut lane);
    let _ = varde.update(Message::Ui(Ui::Look(Look::SelectTool(
        varde_view::Tool::Point,
    ))));
    place_point(&mut varde);
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    requests.borrow_mut().clear();

    // The point was drawn in the document replaced, not in this one.
    let _ = varde.update(Message::Ui(Ui::File(File::RestoreChanges)));
    assert!(!document(&varde).proposing());
    solve(&mut varde, &mut lane);
    assert_eq!(*document(&varde).editor.document(), recovered);
    // The save that waited for it goes.
    let sent = sent(&requests);
    assert!(
        sent.iter()
            .any(|request| matches!(request, IoRequest::Save { .. })),
        "{sent:?}"
    );
}

#[test]
fn recovered_changes_can_be_discarded() {
    let (mut varde, requests) = with_recovered(Document::default());
    let _ = varde.update(Message::Ui(Ui::File(File::DiscardChanges)));
    assert!(document(&varde).recovered().is_none());
    assert_eq!(*document(&varde).editor.document(), with_a_line());
    assert!(!document(&varde).edited());
    assert!(matches!(
        sent(&requests)[..],
        [IoRequest::DiscardRecovery { file: FileId(0) }]
    ));
    // Once only.
    let _ = varde.update(Message::Ui(Ui::File(File::DiscardChanges)));
    let _ = varde.update(Message::Ui(Ui::File(File::RestoreChanges)));
    assert!(sent(&requests).is_empty());
    assert_eq!(document(&varde).editor.revision(), Revision::from(0));
}

/// Restoring recovered changes ends an extrude being set up, whose ids
/// may name other things in the document restored.
#[test]
fn restoring_recovered_changes_ends_the_extrude_session() {
    let (mut varde, _) = with_recovered(Document::example());
    let sketch = document(&varde).editor.document().features()[0].id;
    let _ = varde.update(Message::Ui(Ui::Look(Look::SelectFeature(sketch))));
    let _ = varde.update(Message::Ui(Ui::Look(Look::StartExtrude)));
    assert!(document(&varde).extrude.is_some());
    let _ = varde.update(Message::Ui(Ui::File(File::RestoreChanges)));
    assert_eq!(*document(&varde).editor.document(), Document::example());
    assert!(document(&varde).extrude.is_none());
}

/// [`with_a_line`] with a second sketch, holding a point, and that
/// sketch's id.
fn with_a_line_and_a_sketch() -> (Document, FeatureId) {
    let mut editor = Editor::new(with_a_line());
    editor
        .apply(
            editor
                .document()
                .add_sketch(varde_document::Plane::Origin(OriginPlane::XY)),
        )
        .unwrap();
    let feature = editor.document().features().last().unwrap().id;
    let mut sketch = varde_sketch::Sketch::default();
    sketch.add_point(glam::DVec2::new(5.0, 5.0)).unwrap();
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    (editor.document().clone(), feature)
}

/// The feature selected in the Timeline goes when the document is
/// replaced whole, by restoring recovered changes or undoing or redoing
/// that: its id may name another feature there, which Delete would take.
#[test]
fn restoring_recovered_changes_lets_go_of_the_selected_feature() {
    let (recovered, theirs) = with_a_line_and_a_sketch();
    let (mut varde, _) = with_recovered(recovered.clone());
    // A sketch added after opening gets the id the recovered one has.
    let doc = varde.screen.doc_mut().unwrap();
    doc.apply(
        doc.editor
            .document()
            .add_sketch(varde_document::Plane::Origin(OriginPlane::XY)),
    );
    doc.sync();
    let ours = doc.editor.document().features().last().unwrap().id;
    assert_eq!(ours, theirs);
    let _ = varde.update(Message::Ui(Ui::Look(Look::SelectFeature(ours))));
    assert_eq!(document(&varde).selected_feature, Some(ours));

    let _ = varde.update(Message::Ui(Ui::File(File::RestoreChanges)));
    assert_eq!(*document(&varde).editor.document(), recovered);
    assert_eq!(document(&varde).selected_feature, None);

    // Undoing and redoing the restore cross the replacement too.
    let _ = varde.update(Message::Ui(Ui::Look(Look::SelectFeature(theirs))));
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::Undo)));
    assert_ne!(*document(&varde).editor.document(), recovered);
    assert_eq!(document(&varde).selected_feature, None);
    let _ = varde.update(Message::Ui(Ui::Look(Look::SelectFeature(ours))));
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::Redo)));
    assert_eq!(*document(&varde).editor.document(), recovered);
    assert_eq!(document(&varde).selected_feature, None);

    // An edit within one line of edits keeps it.
    let _ = varde.update(Message::Ui(Ui::Look(Look::SelectFeature(theirs))));
    let doc = varde.screen.doc_mut().unwrap();
    doc.apply(
        doc.editor
            .document()
            .add_sketch(varde_document::Plane::Origin(OriginPlane::XY)),
    );
    doc.sync();
    assert_eq!(document(&varde).selected_feature, Some(theirs));
}

/// The sketch being edited goes on across a replacement with the sketch
/// its id names now, and stays selected in the Timeline.
#[test]
fn restoring_recovered_changes_keeps_the_sketch_edited_selected() {
    let (recovered, theirs) = with_a_line_and_a_sketch();
    let (mut varde, _) = with_recovered(recovered.clone());
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::PlanePicked(OriginPlane::XY))));
    assert_eq!(document(&varde).sketch.as_ref().unwrap().feature, theirs);
    assert_eq!(document(&varde).selected_feature, Some(theirs));

    let _ = varde.update(Message::Ui(Ui::File(File::RestoreChanges)));
    assert_eq!(*document(&varde).editor.document(), recovered);
    assert_eq!(document(&varde).sketch.as_ref().unwrap().feature, theirs);
    assert_eq!(document(&varde).selected_feature, Some(theirs));
}

/// The sketch being edited ends across a replacement where its id names
/// a feature that isn't a sketch, and that feature isn't selected.
#[test]
fn restoring_recovered_changes_lets_go_of_the_sketch_edited_if_not_a_sketch() {
    let recovered = Document::example();
    let extrude = recovered.features()[1].id;
    assert!(matches!(
        recovered.features()[1].kind,
        varde_document::FeatureKind::Extrude(_)
    ));
    let (mut varde, _) = with_recovered(recovered.clone());
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::PlanePicked(OriginPlane::XY))));
    assert_eq!(document(&varde).sketch.as_ref().unwrap().feature, extrude);

    let _ = varde.update(Message::Ui(Ui::File(File::RestoreChanges)));
    assert_eq!(*document(&varde).editor.document(), recovered);
    assert!(document(&varde).sketch.is_none());
    assert_eq!(document(&varde).selected_feature, None);
}

/// The failure markers of a model of the document from before it was
/// replaced whole name features by ids that may name others now: none
/// show until a model of the document as it is arrives, not even from a
/// model of before arriving late, and none across undoing the
/// replacement.
#[test]
fn failure_marks_of_before_a_replacement_mark_nothing() {
    let (mut doc, requests) = deferred();
    // Answers marking `id` unsolved and failed, as if the model found so.
    let answer_marking = |doc: &mut Doc, requests: Vec<Request>, id: FeatureId| {
        for request in requests {
            let response = match handle(request) {
                Response::Regenerated {
                    generation,
                    exclude,
                    draft,
                    mesh,
                    picking,
                    sketches,
                    bodies,
                    ..
                } => Response::Regenerated {
                    generation,
                    exclude,
                    draft,
                    mesh,
                    picking,
                    sketches,
                    unsolved: vec![id],
                    failed: vec![varde_regen::FeatureFailure {
                        feature: id,
                        message: "failed".to_owned(),
                        geometry: None,
                    }],
                    touched: vec![(id, Vec::new())],
                    merged: Vec::new(),
                    placements: Vec::new(),
                    bodies,
                    inspected: None,
                },
                failed => failed,
            };
            doc.computed(response);
        }
    };
    let add_sketch = |doc: &mut Doc| {
        doc.apply(
            doc.editor
                .document()
                .add_sketch(varde_document::Plane::Origin(OriginPlane::XY)),
        );
        doc.sync();
    };
    add_sketch(&mut doc);
    let ours = doc.editor.document().features()[0].id;
    answer_marking(&mut doc, requests.take(), ours);
    assert_eq!(doc.feed.unsolved(), [ours]);
    assert_eq!(doc.feed.failed_features().len(), 1);

    // A change whose model is late.
    add_sketch(&mut doc);
    let late = requests.take();
    assert_eq!(late.len(), 1);

    // Replaced by a design whose first feature has the same id.
    let theirs = Document::example();
    assert_eq!(theirs.features()[0].id, ours);
    doc.apply(Command::Replace(Box::new(theirs)));
    doc.sync();
    assert!(doc.feed.unsolved().is_empty());
    assert!(doc.feed.failed_features().is_empty());
    answer_marking(&mut doc, late, ours);
    assert!(doc.feed.unsolved().is_empty());
    assert!(doc.feed.failed_features().is_empty());

    // The model of the design as it is marks what it finds.
    answer_marking(&mut doc, requests.take(), ours);
    assert_eq!(doc.feed.unsolved(), [ours]);
    assert_eq!(doc.feed.failed_features().len(), 1);

    // Undoing the replacement crosses it too.
    doc.update(Edit::Undo);
    assert!(doc.feed.unsolved().is_empty());
    assert!(doc.feed.failed_features().is_empty());
    answer_marking(&mut doc, requests.take(), ours);
    assert_eq!(doc.feed.unsolved(), [ours]);
}

/// Restoring recovered changes while the sketch is edited lets go of
/// why the solver refused the last edit and the item hovered: their ids
/// are of the sketch replaced.
#[test]
fn restoring_recovered_changes_lets_go_of_the_refusal_and_hover() {
    let (recovered, theirs) = with_a_line_and_a_sketch();
    let (mut varde, _) = with_recovered(recovered.clone());
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::PlanePicked(OriginPlane::XY))));
    // An item of the sketch recovered, with the id of none edited yet.
    let varde_document::FeatureKind::Sketch { sketch, .. } =
        &recovered.feature(theirs).unwrap().kind
    else {
        panic!("a sketch");
    };
    let point = sketch.points[0].id;
    let session = varde.screen.doc_mut().unwrap().sketch.as_mut().unwrap();
    assert_eq!(session.feature, theirs);
    session.hovered = Some(point);
    session.refusal = Some(Refusal::Failed("worker".to_owned()));

    let _ = varde.update(Message::Ui(Ui::File(File::RestoreChanges)));
    assert_eq!(*document(&varde).editor.document(), recovered);
    let session = document(&varde).sketch.as_ref().unwrap();
    assert_eq!(session.hovered, None);
    assert!(session.refusal.is_none());
}

/// Saved as another file, recovered changes not answered yet stay with
/// the design they're of, which offers them when it's next opened: the
/// offer goes, rather than answering it for the new file. Saved over the
/// design itself, it stays.
#[test]
fn saving_as_another_file_leaves_the_recovered_changes_behind() {
    let saved_as = |varde: &mut Varde, requests: &Rc<RefCell<Vec<IoRequest>>>, offered| {
        let id = document(varde).id;
        let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
        let _ = varde.update(Message::Doc(
            id,
            ForDoc::SaveAsPicked(Some(Chosen::Path("/d/other.vrdp".into()))),
        ));
        let sent_now = sent(requests);
        let [IoRequest::SaveAs { revision, .. }] = &sent_now[..] else {
            panic!("not a save as: {sent_now:?}");
        };
        let _ = varde.update(Message::Io(IoResponse::SavedAs {
            file: Some(FileId(0)),
            to: Chosen::Path("/d/other.vrdp".into()),
            revision: *revision,
            result: Ok(varde_io::SavedAs {
                file: FileId(0),
                access: Access::Edit,
                offered,
            }),
        }));
        sent(requests);
    };

    let (mut varde, requests) = with_recovered_changed(Document::default(), true);
    saved_as(&mut varde, &requests, true);
    assert!(document(&varde).recovered().unwrap().design_changed);

    saved_as(&mut varde, &requests, false);
    assert!(document(&varde).recovered().is_none());
    let _ = varde.update(Message::Ui(Ui::File(File::DiscardChanges)));
    let _ = varde.update(Message::Ui(Ui::File(File::RestoreChanges)));
    assert!(sent(&requests).is_empty());
    assert_eq!(*document(&varde).editor.document(), with_a_line());
    // Auto-saves no longer wait for an answer.
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let start = Instant::now();
    tick(&mut varde, start, 0);
    tick(&mut varde, start, 10);
    assert_eq!(auto_saves(&sent(&requests)), [(0, 1)]);
}

/// Restoring auto-saves at once, before anything sent after it: until
/// then the lane keeps what was recovered from being emptied by a save,
/// the offer not being answered as far as it knows.
#[test]
fn restoring_recovered_changes_auto_saves_them_at_once() {
    let (mut varde, requests) = with_recovered(Document::default());
    let _ = varde.update(Message::Ui(Ui::File(File::RestoreChanges)));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let restored = sent(&requests);
    assert_eq!(auto_saves(&restored), [(0, 1)]);
    assert!(matches!(
        restored[..],
        [IoRequest::AutoSave { .. }, IoRequest::Save { .. }]
    ));
    // Not again at the next ticks.
    let start = Instant::now();
    tick(&mut varde, start, 0);
    tick(&mut varde, start, 10);
    assert!(auto_saves(&sent(&requests)).is_empty());
}

/// The page going away on the web auto-saves at once, rather than at a
/// tick that may never come, and the browser asks first while changes
/// would be lost.
#[test]
fn the_page_leaving_auto_saves_at_once() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::PageLeaving);
    assert!(sent(&requests).is_empty());
    assert!(!varde.at_stake());

    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    assert!(varde.at_stake());
    let _ = varde.update(Message::PageLeaving);
    assert_eq!(auto_saves(&sent(&requests)), [(0, 1)]);
    // Auto-saved isn't saved.
    assert!(varde.at_stake());
    // Not again, at the next tick or the page leaving again.
    let _ = varde.update(Message::PageLeaving);
    tick(&mut varde, Instant::now(), 0);
    assert!(sent(&requests).is_empty());

    // Saving is at stake until it's answered.
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let revision = saving(&sent(&requests));
    assert!(varde.at_stake());
    let _ = varde.update(saved(revision, Ok(())));
    assert!(!varde.at_stake());

    // Changes offered to be restored aren't replaced before the answer.
    let (mut varde, requests) = with_recovered(Document::default());
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::PageLeaving);
    assert!(sent(&requests).is_empty());
}

/// Changes auto-saved from an older version of the design say so.
#[test]
fn recovered_changes_of_a_changed_design_say_so() {
    let (varde, _) = with_recovered_changed(Document::default(), true);
    assert!(document(&varde).recovered().unwrap().design_changed);
    let _ = varde.view();
    let (varde, _) = with_recovered(Document::default());
    assert!(!document(&varde).recovered().unwrap().design_changed);
}

/// Closing without an answer keeps them, to be offered again.
#[test]
fn recovered_changes_not_answered_are_kept_on_closing() {
    let (mut varde, requests) = with_recovered(Document::default());
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    assert!(is_welcome(&varde));
    assert!(matches!(
        sent(&requests)[..],
        [
            IoRequest::Close {
                file: FileId(0),
                closing: Closing::Keep
            },
            IoRequest::LoadThumbnails { .. }
        ]
    ));
}

/// A new design, with its store entry made as file 9.
fn with_new_design() -> (Varde, Rc<RefCell<Vec<IoRequest>>>) {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Io(IoResponse::RecentLoaded {
        entries: vec![],
        home: None,
    }));
    sent(&requests);
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let [IoRequest::New { id }] = sent(&requests)[..] else {
        panic!("no store entry asked for");
    };
    let _ = varde.update(Message::Io(IoResponse::Created {
        id,
        result: Ok(FileId(9)),
    }));
    assert_eq!(
        *document(&varde).target(),
        Target::Entry { file: FileId(9) }
    );
    (varde, requests)
}

#[test]
fn a_new_design_auto_saves_to_its_entry_until_saved_as() {
    let (mut varde, requests) = with_new_design();
    let start = Instant::now();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    tick(&mut varde, start, 0);
    tick(&mut varde, start, 3);
    assert_eq!(auto_saves(&sent(&requests)), [(9, 1)]);

    // Save asks where, and the Save As takes the entry along.
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    assert!(document(&varde).picking().is_some());
    let id = document(&varde).id;
    let _ = varde.update(Message::Doc(
        id,
        ForDoc::SaveAsPicked(Some(Chosen::Path("/d/new.vrdp".into()))),
    ));
    let sent_now = sent(&requests);
    let [
        IoRequest::SaveAs {
            file: Some(FileId(9)),
            revision,
            ..
        },
    ] = sent_now[..]
    else {
        panic!("not a save as: {sent_now:?}");
    };
    let _ = varde.update(Message::Io(IoResponse::SavedAs {
        file: Some(FileId(9)),
        to: Chosen::Path("/d/new.vrdp".into()),
        revision,
        result: Ok(varde_io::SavedAs {
            file: FileId(9),
            access: Access::Edit,
            offered: false,
        }),
    }));
    assert_eq!(document(&varde).target().design_file(), Some(FileId(9)));
    sent(&requests);
    // From now on Save saves.
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    assert!(matches!(
        sent(&requests)[..],
        [IoRequest::Save {
            file: FileId(9),
            ..
        }]
    ));
}

#[test]
fn a_new_design_not_saved_is_discarded_with_its_entry() {
    let (mut varde, requests) = with_new_design();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    assert_eq!(document(&varde).prompt(), Some(Leave::Close));
    let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Discard))));
    assert!(matches!(
        sent(&requests)[..],
        [IoRequest::Close {
            file: FileId(9),
            closing: Closing::Clean
        }]
    ));
}

/// The shown save error of the open document: a save's, else an
/// auto-save's.
fn save_error(varde: &Varde) -> Option<&str> {
    let doc = document(varde);
    match doc.save_error() {
        Some(SaveError::Failed(error)) => Some(error),
        Some(SaveError::Conflict) => Some("conflict"),
        Some(SaveError::Taken) => Some("taken"),
        Some(SaveError::OpenedDamaged | SaveError::Damaged) => Some("damaged"),
        None => doc.auto_save_error(),
    }
}

/// A new design the lane can't make an entry for is never auto-saved,
/// which means it's lost on a crash or reload: the user is told.
#[test]
fn a_new_design_without_an_entry_says_it_is_not_auto_saved() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let [.., IoRequest::New { id }] = sent(&requests)[..] else {
        panic!("no store entry asked for");
    };
    let _ = varde.update(Message::Io(IoResponse::Created {
        id,
        result: Err("the file worker didn't load".to_owned()),
    }));
    assert_eq!(*document(&varde).target(), Target::None);
    let error = save_error(&varde).expect("no error shown");
    assert!(error.contains("the file worker didn't load"), "{error}");
}

/// Not needed once a Save As is on its way, which gives the design a file
/// and a sidecar of its own: nothing to tell.
#[test]
fn an_entry_failing_after_a_save_as_is_not_shown() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let [.., IoRequest::New { id }] = sent(&requests)[..] else {
        panic!("no store entry asked for");
    };
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    let doc_id = document(&varde).id;
    let _ = varde.update(Message::Doc(
        doc_id,
        ForDoc::SaveAsPicked(Some(Chosen::Path("/d/a.vrdp".into()))),
    ));
    let _ = varde.update(Message::Io(IoResponse::Created {
        id,
        result: Err("no room".to_owned()),
    }));
    assert_eq!(save_error(&varde), None);
}

#[test]
fn a_failed_auto_save_is_shown() {
    let (mut varde, requests) = with_new_design();
    let start = Instant::now();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    tick(&mut varde, start, 0);
    tick(&mut varde, start, 3);
    assert_eq!(auto_saves(&sent(&requests)), [(9, 1)]);
    // Another document's, answered late: not this one's.
    let _ = varde.update(Message::Io(IoResponse::AutoSaved {
        file: FileId(3),
        revision: 1.into(),
        result: Err("gone".to_owned()),
    }));
    assert_eq!(save_error(&varde), None);
    let _ = varde.update(Message::Io(IoResponse::AutoSaved {
        file: FileId(9),
        revision: 1.into(),
        result: Err("the file worker stopped".to_owned()),
    }));
    let error = save_error(&varde).expect("no error shown");
    assert!(error.contains("the file worker stopped"), "{error}");
}

/// Left before the lane has made its entry: the lane is told to close it,
/// and the late answer is ignored.
#[test]
fn a_new_design_left_before_its_entry_is_made_abandons_it() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let [.., IoRequest::New { id }] = sent(&requests)[..] else {
        panic!("no store entry asked for");
    };
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    assert!(matches!(
        sent(&requests)[..],
        [IoRequest::Abandon { id: abandoned }] if abandoned == id
    ));
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    sent(&requests);
    let _ = varde.update(Message::Io(IoResponse::Created {
        id,
        result: Ok(FileId(1)),
    }));
    assert_eq!(*document(&varde).target(), Target::None);
    assert!(sent(&requests).is_empty());
}

/// Saved as before its entry was made: the entry isn't needed, and is
/// closed as it arrives.
#[test]
fn an_entry_made_after_a_save_as_is_closed() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let [.., IoRequest::New { id }] = sent(&requests)[..] else {
        panic!("no store entry asked for");
    };
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    let doc_id = document(&varde).id;
    let _ = varde.update(Message::Doc(
        doc_id,
        ForDoc::SaveAsPicked(Some(Chosen::Path("/d/a.vrdp".into()))),
    ));
    assert!(matches!(
        sent(&requests)[..],
        [IoRequest::SaveAs { file: None, .. }]
    ));
    let _ = varde.update(Message::Io(IoResponse::Created {
        id,
        result: Ok(FileId(1)),
    }));
    assert_eq!(*document(&varde).target(), Target::None);
    assert!(matches!(
        sent(&requests)[..],
        [IoRequest::Close {
            file: FileId(1),
            closing: Closing::Clean
        }]
    ));
}

/// A Save As answered once no document is open: the file it made has
/// nothing to write it, and is closed.
#[test]
fn a_save_as_answered_on_the_welcome_screen_closes_its_file() {
    let (mut varde, requests) = with_files();
    sent(&requests);
    let _ = varde.update(Message::Io(IoResponse::SavedAs {
        file: None,
        to: Chosen::Path("/d/a.vrdp".into()),
        revision: 0.into(),
        result: Ok(varde_io::SavedAs {
            file: FileId(3),
            access: Access::Edit,
            offered: false,
        }),
    }));
    assert!(matches!(
        sent(&requests)[..],
        [IoRequest::Close {
            file: FileId(3),
            closing: Closing::Clean
        }]
    ));
}

/// The Save As the entry was closed for failing: the design would never be
/// auto-saved, so another entry is asked for.
#[test]
fn a_failed_save_as_an_entry_was_closed_for_asks_for_another() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let [.., IoRequest::New { id }] = sent(&requests)[..] else {
        panic!("no store entry asked for");
    };
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    let doc_id = document(&varde).id;
    let _ = varde.update(Message::Doc(
        doc_id,
        ForDoc::SaveAsPicked(Some(Chosen::Path("/d/a.vrdp".into()))),
    ));
    let [IoRequest::SaveAs { revision, .. }] = sent(&requests)[..] else {
        panic!("no save as");
    };
    let _ = varde.update(Message::Io(IoResponse::Created {
        id,
        result: Ok(FileId(1)),
    }));
    sent(&requests);
    let _ = varde.update(Message::Io(IoResponse::SavedAs {
        file: None,
        to: Chosen::Path("/d/a.vrdp".into()),
        revision,
        result: Err(SaveError::Failed("permission denied".to_owned())),
    }));
    let [IoRequest::New { id }] = sent(&requests)[..] else {
        panic!("no store entry asked for again");
    };
    let _ = varde.update(Message::Io(IoResponse::Created {
        id,
        result: Ok(FileId(2)),
    }));
    assert_eq!(
        *document(&varde).target(),
        Target::Entry { file: FileId(2) }
    );
    let start = Instant::now();
    tick(&mut varde, start, 0);
    tick(&mut varde, start, 3);
    assert_eq!(auto_saves(&sent(&requests)), [(2, 1)]);
}

#[test]
fn recovered_designs_are_offered_on_the_welcome_screen() {
    let (mut varde, requests) = with_files();
    let designs = vec![
        Recovered {
            path: "/data/designs/a.vrdp".into(),
            modified: Some(varde_io::UnixSeconds(1_790_424_000)),
            name: None,
            damage: None,
        },
        Recovered {
            path: "/data/designs/b.vrdp".into(),
            modified: None,
            name: None,
            damage: None,
        },
    ];
    let _ = varde.update(Message::Io(IoResponse::RecoveredListed { designs }));
    let _ = varde.view();
    sent(&requests);

    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::DiscardStored(
        "/data/designs/b.vrdp".into(),
    ))));
    assert_eq!(varde.files.recovered.len(), 1);
    // The lane lists what's left after.
    assert!(matches!(
        &sent(&requests)[..],
        [IoRequest::DiscardRecovered { path }]
            if path == Path::new("/data/designs/b.vrdp")
    ));
    // Should that fail, it says so.
    let _ = varde.update(Message::Io(IoResponse::RecoveredDiscarded {
        path: "/data/designs/b.vrdp".into(),
        result: Err("in use".to_owned()),
    }));
    let Screen::Welcome(welcome) = &varde.screen else {
        panic!("not on the welcome screen");
    };
    assert!(welcome.error().unwrap().contains("in use"));
    assert!(sent(&requests).is_empty());

    let path = PathBuf::from("/data/designs/a.vrdp");
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenStored(
        path.clone(),
    ))));
    let sent_now = sent(&requests);
    let [IoRequest::OpenRecovered { id, path: asked }] = &sent_now[..] else {
        panic!("not opened: {sent_now:?}");
    };
    assert_eq!(*asked, path);
    let mut editor = Editor::new(with_a_line());
    let sketch = editor.document().features()[0].id;
    editor.apply(Command::RemoveFeature(sketch)).unwrap();
    let left = editor.document().clone();
    let _ = varde.update(Message::Io(IoResponse::Opened {
        id: *id,
        path: Some(path.clone()),
        result: Ok(Opened {
            file: FileId(4),
            document: left.clone(),
            access: Access::Edit,
            recovered: Ok(None),
            browser: None,
            not_copied: None,
            download: None,
            damage: None,
        }),
    }));
    let doc = document(&varde);
    assert_eq!(*doc.editor.document(), left);
    assert_eq!(doc.name, "Untitled");
    assert!(matches!(doc.target(), Target::Entry { .. }));
    // Never saved: closing asks, and it's not a recent file.
    assert!(doc.edited());
    assert!(varde.files.recovered.is_empty());
    assert!(varde.files.recent.entries().is_empty());
    // Nothing to auto-save until it's edited: it's in its entry already.
    let start = Instant::now();
    tick(&mut varde, start, 0);
    tick(&mut varde, start, 10);
    assert!(sent(&requests).is_empty());
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    assert_eq!(document(&varde).prompt(), Some(Leave::Close));
}

/// A file the recent list has that isn't there just now is shown, as
/// unavailable, until it's opened again.
#[test]
fn unavailable_recent_files_are_kept() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Io(IoResponse::RecentLoaded {
        entries: vec![
            varde_io::recent::Listed {
                entry: varde_io::RecentFile {
                    path: "/mnt/a.vrdp".into(),
                    opened: varde_io::UnixSeconds(2),
                },
                available: false,
            },
            varde_io::recent::Listed {
                entry: varde_io::RecentFile {
                    path: "/d/b.vrdp".into(),
                    opened: varde_io::UnixSeconds(1),
                },
                available: true,
            },
        ],
        home: None,
    }));
    let available = |varde: &Varde| -> Vec<bool> {
        let entries = varde.files.recent.entries();
        entries.iter().map(|listed| listed.available).collect()
    };
    assert_eq!(available(&varde), [false, true]);
    let _ = varde.view();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(
        "/mnt/a.vrdp".into(),
    ))));
    let (id, path) = last_open(&requests);
    let _ = varde.update(opened(id, path, 0, Access::Edit));
    assert_eq!(available(&varde), [true, true]);
    assert_eq!(
        varde.files.recent.entries()[0].entry.path,
        Path::new("/mnt/a.vrdp")
    );
}

/// Waits until nobody holds the lock on `path`, as the lane of a session
/// that "crashed" lets go of it once its thread ends.
fn wait_unlocked(path: &Path) {
    let start = std::time::Instant::now();
    loop {
        let file = std::fs::File::open(path).unwrap();
        if file.try_lock().is_ok() {
            return;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "still locked");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// An app on a real lane with its own files in `dir`, answering requests
/// until one `until` wants arrives.
struct Session {
    varde: Varde,
    responses: varde_io::lane::Responses,
}

impl Session {
    fn new(dir: &Path) -> Self {
        let (lane, responses) = varde_io::lane::spawn_at(varde_io::Stores {
            recent: None,
            settings: None,
            designs: Some(dir.join("designs")),
            panic: None,
        });
        let mut varde = Varde::new();
        let _ = varde.update(Message::IoReady(lane));
        Self { varde, responses }
    }

    fn answer(&mut self, until: fn(&IoResponse) -> bool) {
        loop {
            let response = block_on(self.responses.next()).unwrap();
            let done = until(&response);
            let _ = self.varde.update(Message::Io(response));
            if done {
                break;
            }
        }
    }

    /// Opens the store entry at `entry` from the welcome screen, and waits
    /// for it to open and for the list the lane follows that with.
    fn open_in_browser(&mut self, entry: &Path) {
        let _ = self
            .varde
            .update(Message::Ui(Ui::Welcome(WelcomeUi::OpenStored(
                entry.to_owned(),
            ))));
        self.answer(|r| matches!(r, IoResponse::Opened { .. }));
        self.answer(|r| matches!(r, IoResponse::RecoveredListed { .. }));
    }

    /// Edits and waits for the edit to be auto-saved.
    fn edit_and_auto_save(&mut self) {
        let _ = self
            .varde
            .update(Message::Ui(Ui::Edit(an_edit(document(&self.varde)))));
        let start = Instant::now();
        tick(&mut self.varde, start, 0);
        tick(&mut self.varde, start, 3);
        self.answer(|r| matches!(r, IoResponse::AutoSaved { result: Ok(()), .. }));
    }
}

/// Through the real lane: edits auto-saved, then undone back to the
/// saved state, aren't offered after a crash.
#[test]
fn auto_saved_edits_undone_are_not_recovered_after_a_crash() {
    let dir = std::env::temp_dir().join(format!("varde-app-undone-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("part.vrdp");
    write_design(&path, &with_a_line());

    let mut crashed = Session::new(&dir);
    let _ = crashed
        .varde
        .update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(path.clone()))));
    crashed.answer(|r| matches!(r, IoResponse::Opened { .. }));
    crashed.edit_and_auto_save();
    let _ = crashed.varde.update(Message::Ui(Ui::Edit(Edit::Undo)));
    tick(&mut crashed.varde, Instant::now(), 0);
    crashed.varde.files.io.send(IoRequest::Flush);
    crashed.answer(|r| matches!(r, IoResponse::Flushed));
    drop(crashed);
    // Empty, the sidecar is deleted as the lane lets go of it.
    let sidecar = dir.join(".part.vrdp.autosave");
    let start = std::time::Instant::now();
    while sidecar.exists() {
        assert!(start.elapsed() < Duration::from_secs(10), "still there");
        std::thread::sleep(Duration::from_millis(5));
    }

    let mut session = Session::new(&dir);
    let _ = session
        .varde
        .update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(path.clone()))));
    session.answer(|r| matches!(r, IoResponse::Opened { .. }));
    let doc = document(&session.varde);
    assert_eq!(*doc.editor.document(), with_a_line());
    assert!(doc.recovered().is_none());
    drop(session);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The whole crash round trip through the real lane: auto-saved edits of
/// a design and of a new one outlive a session that ends without closing
/// them, and are offered back by the next.
#[test]
fn auto_saved_edits_are_recovered_after_a_crash() {
    let dir = std::env::temp_dir().join(format!("varde-app-recover-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("part.vrdp");
    write_design(&path, &with_a_line());

    // A new design, edited and auto-saved.
    let mut crashed = Session::new(&dir);
    crashed.answer(|r| matches!(r, IoResponse::RecoveredListed { .. }));
    let _ = crashed
        .varde
        .update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    crashed.answer(|r| matches!(r, IoResponse::Created { .. }));
    crashed.edit_and_auto_save();
    let new_design = document(&crashed.varde).editor.document().clone();
    // Its lane ends without the design being closed, as in a crash.
    drop(crashed);

    // A design, opened, edited and auto-saved.
    let mut crashed = Session::new(&dir);
    let _ = crashed
        .varde
        .update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(path.clone()))));
    crashed.answer(|r| matches!(r, IoResponse::Opened { .. }));
    crashed.edit_and_auto_save();
    let edited = document(&crashed.varde).editor.document().clone();
    drop(crashed);
    wait_unlocked(&dir.join(".part.vrdp.autosave"));
    for entry in std::fs::read_dir(dir.join("designs")).unwrap() {
        wait_unlocked(&entry.unwrap().path());
    }

    let mut session = Session::new(&dir);
    session.answer(|r| matches!(r, IoResponse::RecoveredListed { .. }));
    assert_eq!(session.varde.files.recovered.len(), 1);
    let _ = session.varde.view();
    let entry = session.varde.files.recovered[0].path.clone();
    session.open_in_browser(&entry);
    assert_eq!(*document(&session.varde).editor.document(), new_design);
    // Not saved: closing asks, and not saving deletes it.
    let _ = session
        .varde
        .update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = session
        .varde
        .update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Discard))));
    session.answer(|r| matches!(r, IoResponse::Closed { .. }));
    assert!(!entry.exists());

    let _ = session
        .varde
        .update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(path.clone()))));
    session.answer(|r| matches!(r, IoResponse::Opened { .. }));
    let doc = document(&session.varde);
    assert_eq!(*doc.editor.document(), with_a_line());
    assert_eq!(doc.recovered().map(|offer| &offer.document), Some(&edited));
    let _ = session
        .varde
        .update(Message::Ui(Ui::File(File::RestoreChanges)));
    assert_eq!(*document(&session.varde).editor.document(), edited);
    // Saving empties the sidecar, and closing deletes it.
    let _ = session.varde.update(Message::Ui(Ui::File(File::Save)));
    session.answer(|r| matches!(r, IoResponse::Saved { .. }));
    assert_eq!(
        std::fs::metadata(dir.join(".part.vrdp.autosave"))
            .unwrap()
            .len(),
        0
    );
    let _ = session
        .varde
        .update(Message::Ui(Ui::File(File::CloseDocument)));
    session.answer(|r| matches!(r, IoResponse::Closed { .. }));
    assert!(!dir.join(".part.vrdp.autosave").exists());
    let (saved, _) = varde_io::vrdp::from_bytes(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved, edited);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Saving a design without answering the offer of what a crash left keeps
/// it, to be offered again.
#[test]
fn recovered_changes_not_answered_outlive_a_save() {
    let dir = std::env::temp_dir().join(format!("varde-app-unanswered-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("part.vrdp");
    write_design(&path, &with_a_line());

    let mut crashed = Session::new(&dir);
    let _ = crashed
        .varde
        .update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(path.clone()))));
    crashed.answer(|r| matches!(r, IoResponse::Opened { .. }));
    crashed.edit_and_auto_save();
    let edited = document(&crashed.varde).editor.document().clone();
    drop(crashed);
    wait_unlocked(&dir.join(".part.vrdp.autosave"));

    let mut session = Session::new(&dir);
    let _ = session
        .varde
        .update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(path.clone()))));
    session.answer(|r| matches!(r, IoResponse::Opened { .. }));
    assert_eq!(
        document(&session.varde)
            .recovered()
            .map(|offer| &offer.document),
        Some(&edited)
    );
    let id = document(&session.varde).editor.document().features()[0].id;
    let _ = session
        .varde
        .update(Message::Ui(Ui::Edit(Edit::ToggleFeatureVisible(id))));
    let _ = session.varde.update(Message::Ui(Ui::File(File::Save)));
    session.answer(|r| matches!(r, IoResponse::Saved { result: Ok(()), .. }));
    let _ = session
        .varde
        .update(Message::Ui(Ui::File(File::CloseDocument)));
    session.answer(|r| matches!(r, IoResponse::Closed { .. }));

    let _ = session
        .varde
        .update(Message::Ui(Ui::Welcome(WelcomeUi::OpenPath(path.clone()))));
    session.answer(|r| matches!(r, IoResponse::Opened { .. }));
    assert_eq!(
        document(&session.varde)
            .recovered()
            .map(|offer| &offer.document),
        Some(&edited)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file picked on the web, `bracket.vrdp`.
fn picked(id: u64, from: PickedFrom) -> Picked {
    Picked {
        id,
        name: "bracket.vrdp".to_owned(),
        from,
    }
}

/// An app showing the file `picked` opened as file 0 on the web, with the
/// requests sent so far forgotten.
fn with_picked_file(picked: Picked) -> (Varde, Rc<RefCell<Vec<IoRequest>>>) {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Picked(Some(Chosen::File(picked.clone()))));
    let sent = sent(&requests);
    let [
        IoRequest::LoadRecent,
        IoRequest::ListRecovered,
        IoRequest::LoadSettings,
        IoRequest::LoadPanic,
        IoRequest::Open {
            id,
            from: Chosen::File(asked),
        },
    ] = &sent[..]
    else {
        panic!("not opened: {sent:?}");
    };
    assert_eq!(*asked, picked);
    let _ = varde.update(opened(*id, "bracket.vrdp".into(), 0, Access::Edit));
    (varde, requests)
}

/// On the web, a file dragged over the welcome screen's page lights the
/// drop zone, and dropped, it opens as one picked with Open… would.
#[test]
fn a_file_dropped_on_the_welcome_screen_opens() {
    let (mut varde, requests) = with_files();
    sent(&requests);
    let dragging = |varde: &Varde| match &varde.screen {
        Screen::Welcome(welcome) => welcome.dragging(),
        Screen::Document(_) => panic!("not on the welcome screen"),
    };
    let _ = varde.update(Message::FileDragged(true));
    assert!(dragging(&varde));
    let _ = varde.update(Message::FileDragged(true));
    assert!(dragging(&varde));
    let _ = varde.update(Message::FileDragged(false));
    assert!(!dragging(&varde));

    // Something that isn't a file opens nothing.
    let _ = varde.update(Message::FileDragged(true));
    let _ = varde.update(Message::FileDropped(None));
    assert!(!dragging(&varde));
    assert!(sent(&requests).is_empty());

    let dropped = picked(4, PickedFrom::Input);
    let _ = varde.update(Message::FileDropped(Some(Chosen::File(dropped.clone()))));
    assert!(matches!(
        &sent(&requests)[..],
        [IoRequest::Open { from: Chosen::File(asked), .. }] if *asked == dropped
    ));
    // Opened, so not let go of.
    assert!(crate::welcome::FORGOTTEN.with_borrow(Vec::is_empty));
}

/// With a document open, a file dropped on the page opens nothing, and is
/// let go of: the browser doesn't take it either, see `platform::drops`.
#[test]
fn a_file_dropped_with_a_document_open_is_ignored() {
    let (mut varde, requests) = with_picked_file(picked(3, PickedFrom::Input));
    sent(&requests);
    let _ = varde.update(Message::FileDragged(true));
    let dropped = picked(4, PickedFrom::Input);
    let _ = varde.update(Message::FileDropped(Some(Chosen::File(dropped.clone()))));
    assert!(sent(&requests).is_empty());
    assert!(varde.screen.doc().is_some());
    assert_eq!(
        crate::welcome::FORGOTTEN.with_borrow(Clone::clone),
        [dropped]
    );
}

/// A file picked through the File System Access API goes on from that
/// file, and isn't a recent file: there's no keeping its handle.
#[test]
fn a_writable_picked_file_opens_as_the_design() {
    let (varde, _requests) = with_picked_file(picked(3, PickedFrom::Handle));
    let doc = document(&varde);
    assert_eq!(doc.name, "bracket");
    assert_eq!(
        *doc.target(),
        Target::File {
            file: FileId(0),
            picked: Some(picked(3, PickedFrom::Handle))
        }
    );
    assert!(!doc.edited());
    assert!(varde.files.recent.entries().is_empty());
}

/// A file from a file input can't be written back: the web's lane copies
/// it into browser storage (see `storage.rs`); answered without that, it
/// opens as a copy, by the file's name, which Save saves as, and which is
/// unedited so far.
#[test]
fn a_file_only_read_opens_as_an_untitled_copy() {
    let (mut varde, requests) = with_picked_file(picked(3, PickedFrom::Input));
    let doc = document(&varde);
    assert_eq!(doc.name, "bracket");
    assert_eq!(*doc.target(), Target::Entry { file: FileId(0) });
    assert!(!doc.edited());
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    assert!(is_welcome(&varde));
    assert!(matches!(
        sent(&requests)[..],
        [IoRequest::Close {
            file: FileId(0),
            closing: Closing::Clean
        }]
    ));
}

/// Saving to a picked file asks the browser whether it may first, and
/// only then sends the save; if not, it says why and sends nothing.
#[test]
fn saving_to_a_picked_file_asks_whether_it_may_first() {
    let (mut varde, requests) = with_picked_file(picked(3, PickedFrom::Handle));
    let id = document(&varde).id;
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    assert!(sent(&requests).is_empty());
    assert!(document(&varde).picking().is_some());
    // Asked once at a time.
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    assert!(sent(&requests).is_empty());

    let _ = varde.update(Message::Doc(
        id,
        ForDoc::Writable(Err("not allowed".to_owned())),
    ));
    let doc = document(&varde);
    assert!(doc.picking().is_none());
    assert_eq!(
        doc.banner_error().as_deref(),
        Some("not allowed"),
        "{:?}",
        doc.save_error()
    );
    assert!(sent(&requests).is_empty());
    // An answer nobody waits for does nothing.
    let _ = varde.update(Message::Doc(id, ForDoc::Writable(Ok(()))));
    assert!(sent(&requests).is_empty());

    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let _ = varde.update(Message::Doc(id, ForDoc::Writable(Ok(()))));
    let revision = saving(&sent(&requests));
    assert!(document(&varde).save_error().is_none());
    let _ = varde.update(saved(revision, Ok(())));
    assert!(!document(&varde).edited());
    // Nothing to save: nothing asked.
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    assert!(document(&varde).picking().is_none());
}

/// Each answer is taken only while it's the one waited for: the Save As
/// dialog closing while the browser is asked whether the file may be
/// written to, or the browser answering while the Save As dialog shows,
/// does nothing.
#[test]
fn only_the_answer_waited_for_is_taken() {
    let (mut varde, requests) = with_picked_file(picked(3, PickedFrom::Handle));
    let id = document(&varde).id;
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let other = Chosen::Path("/d/other.vrdp".into());
    let _ = varde.update(Message::Doc(id, ForDoc::SaveAsPicked(Some(other))));
    assert_eq!(document(&varde).picking(), Some(Picking::Writable));
    assert!(sent(&requests).is_empty());

    let (mut varde, requests) = with_open_file();
    let id = document(&varde).id;
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    let _ = varde.update(Message::Doc(id, ForDoc::Writable(Ok(()))));
    assert_eq!(document(&varde).picking(), Some(Picking::SaveAs));
    assert!(sent(&requests).is_empty());
}

/// Closing a picked file with unsaved changes and saving them waits for
/// the browser to allow it, then for the save, and then closes.
#[test]
fn closing_a_picked_file_saves_once_allowed() {
    let (mut varde, requests) = with_picked_file(picked(3, PickedFrom::Handle));
    let id = document(&varde).id;
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Save))));
    assert!(sent(&requests).is_empty());
    let _ = varde.update(Message::Doc(id, ForDoc::Writable(Ok(()))));
    let revision = saving(&sent(&requests));
    assert!(!is_welcome(&varde));
    let _ = varde.update(saved(revision, Ok(())));
    assert!(is_welcome(&varde));

    // Not allowed: it stays.
    let (mut varde, requests) = with_picked_file(picked(4, PickedFrom::Handle));
    let id = document(&varde).id;
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Save))));
    let _ = varde.update(Message::Doc(id, ForDoc::Writable(Err("no".to_owned()))));
    assert!(sent(&requests).is_empty());
    assert!(!is_welcome(&varde));
    assert_eq!(document(&varde).leaving(), None);
}

/// Leaving to discard changes goes on once the saves it waits for are
/// answered, even if one is answered while asking whether a Save may write
/// the file, and the browser then refuses.
#[test]
fn discarding_goes_on_after_a_save_is_refused() {
    let (mut varde, requests) = with_picked_file(picked(3, PickedFrom::Handle));
    let id = document(&varde).id;
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let _ = varde.update(Message::Doc(id, ForDoc::Writable(Ok(()))));
    let revision = saving(&sent(&requests));
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Discard))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    assert!(document(&varde).picking().is_some());
    let _ = varde.update(saved(revision, Ok(())));
    assert!(!is_welcome(&varde));
    let _ = varde.update(Message::Doc(id, ForDoc::Writable(Err("no".to_owned()))));
    assert!(is_welcome(&varde));
}

/// Leaving to discard changes goes on once the saves it waits for are
/// answered, even if one is answered while asking where to Save As, and
/// the user then backs out of it.
#[test]
fn discarding_goes_on_after_backing_out_of_save_as() {
    let (mut varde, requests) = with_open_file();
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let revision = saving(&sent(&requests));
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Discard))));
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    assert!(document(&varde).picking().is_some());
    let _ = varde.update(saved(revision, Ok(())));
    assert!(!is_welcome(&varde));
    let id = document(&varde).id;
    let _ = varde.update(Message::Doc(id, ForDoc::SaveAsPicked(None)));
    assert!(is_welcome(&varde));
}

/// A Save As to a file picked on the web goes on from that file.
#[test]
fn a_design_saved_as_a_picked_file_goes_on_from_it() {
    let (mut varde, requests) = with_files();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let created = sent(&requests);
    let Some(IoRequest::New { id: new }) = created.last() else {
        panic!("no entry asked for: {created:?}");
    };
    let _ = varde.update(Message::Io(IoResponse::Created {
        id: *new,
        result: Ok(FileId(5)),
    }));
    let doc_id = document(&varde).id;
    let _ = varde.update(Message::Ui(Ui::Edit(an_edit(document(&varde)))));
    let _ = varde.update(Message::Ui(Ui::File(File::SaveAs)));
    assert!(document(&varde).picking().is_some());
    // Backing out sends nothing.
    let _ = varde.update(Message::Doc(doc_id, ForDoc::SaveAsPicked(None)));
    assert!(document(&varde).picking().is_none());
    assert!(sent(&requests).is_empty());

    let _ = varde.update(Message::Ui(Ui::File(File::Save)));
    let _ = varde.update(Message::Doc(
        doc_id,
        ForDoc::SaveAsPicked(Some(Chosen::File(picked(6, PickedFrom::Handle)))),
    ));
    let sent_now = sent(&requests);
    let [
        IoRequest::SaveAs {
            file: Some(FileId(5)),
            to: SaveTo::Picked(asked),
            revision,
            ..
        },
    ] = &sent_now[..]
    else {
        panic!("not a Save As: {sent_now:?}");
    };
    assert_eq!(*asked, picked(6, PickedFrom::Handle));
    assert!(document(&varde).saves().any());
    let _ = varde.update(Message::Io(IoResponse::SavedAs {
        file: Some(FileId(5)),
        to: Chosen::File(picked(6, PickedFrom::Handle)),
        revision: *revision,
        result: Ok(varde_io::SavedAs {
            file: FileId(5),
            access: Access::Edit,
            offered: false,
        }),
    }));
    let doc = document(&varde);
    assert_eq!(doc.name, "bracket");
    assert!(!doc.edited());
    assert_eq!(
        *doc.target(),
        Target::File {
            file: FileId(5),
            picked: Some(picked(6, PickedFrom::Handle))
        }
    );
    assert!(varde.files.recent.entries().is_empty());
}

/// A design recovered on the web that was opened from a file is known by
/// the file's name, on the welcome screen and once opened.
#[test]
fn a_recovered_design_is_known_by_its_name() {
    let (mut varde, requests) = with_files();
    let path = PathBuf::from("designs/a.vrdp");
    let _ = varde.update(Message::Io(IoResponse::RecoveredListed {
        designs: vec![Recovered {
            path: path.clone(),
            modified: None,
            name: Some("bracket.vrdp".to_owned()),
            damage: None,
        }],
    }));
    let _ = varde.view();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::OpenStored(
        path.clone(),
    ))));
    let sent_now = sent(&requests);
    let Some(IoRequest::OpenRecovered { id, .. }) = sent_now.last() else {
        panic!("not opened: {sent_now:?}");
    };
    let _ = varde.update(opened(*id, path, 1, Access::Edit));
    let doc = document(&varde);
    assert_eq!(doc.name, "bracket");
    assert!(matches!(doc.target(), Target::Entry { .. }));
    assert!(doc.edited());
}

#[test]
fn a_tab_clicked_while_peeking_is_shown() {
    let (mut varde, _) = with_open_file();
    assert_eq!(document(&varde).panel, Panel::Objects);
    let _ = varde.update(Message::PeekPanel(true));
    let _ = varde.update(Message::Ui(Ui::Look(Look::SelectPanel(Panel::Timeline))));
    assert_eq!(document(&varde).panel, Panel::Timeline);
    assert!(!varde.peeking);
}

#[test]
fn the_peek_lets_go_of_the_timeline_row_hovered() {
    let (mut varde, _) = with_open_file();
    let feature = document(&varde).editor.document().features()[0].id;
    let hover = Look::HoverFeature(Some(feature));
    let _ = varde.update(Message::Ui(Ui::Look(Look::SelectPanel(Panel::Timeline))));
    let _ = varde.update(Message::Ui(Ui::Look(hover.clone())));
    assert_eq!(document(&varde).hovered_feature, Some(feature));
    // The Timeline peeked away goes without an exit.
    let _ = varde.update(Message::PeekPanel(true));
    assert_eq!(document(&varde).hovered_feature, None);
    // Peeking at it, and back: the same.
    let _ = varde.update(Message::Ui(Ui::Look(hover)));
    let _ = varde.update(Message::PeekPanel(false));
    assert_eq!(document(&varde).hovered_feature, None);
}

/// Pressing `key` with `modifiers`, as iced reports it.
pub(crate) fn press(key: keyboard::Key, modifiers: keyboard::Modifiers) -> keyboard::Event {
    keyboard::Event::KeyPressed {
        modified_key: key.clone(),
        key,
        physical_key: key::Physical::Unidentified(key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers,
        text: None,
        repeat: false,
    }
}

#[test]
fn escape_cancels_a_prompt_or_closes_the_file_menu() {
    let escape = || press(keyboard::Key::Named(key::Named::Escape), Default::default());
    assert!(matches!(
        escape_key(((Some(Dialog::Unsaved), false), escape())),
        Some(Message::Ui(Ui::File(File::Unsaved(Unsaved::Cancel))))
    ));
    assert!(matches!(
        escape_key(((Some(Dialog::Delete), false), escape())),
        Some(Message::Ui(Ui::Look(Look::CancelDelete)))
    ));
    assert!(matches!(
        escape_key(((None, false), escape())),
        Some(Message::Ui(Ui::Look(Look::Escape)))
    ));
}

/// Tab alone backs out as Escape does, prompts included, but not where a
/// shortcut takes it: a drawing tool's fields, the Dimension tool's
/// switch between radius and diameter.
#[test]
fn tab_backs_out_as_escape_where_nothing_else_takes_it() {
    let tab = |modifiers| press(keyboard::Key::Named(key::Named::Tab), modifiers);
    let none = keyboard::Modifiers::empty();
    assert!(matches!(
        escape_key(((None, false), tab(none))),
        Some(Message::Ui(Ui::Look(Look::Escape)))
    ));
    assert!(matches!(
        escape_key(((Some(Dialog::Delete), false), tab(none))),
        Some(Message::Ui(Ui::Look(Look::CancelDelete)))
    ));
    assert!(escape_key(((None, true), tab(none))).is_none());
    assert!(escape_key(((None, false), tab(keyboard::Modifiers::SHIFT))).is_none());
    // Escape is taken whatever Tab does.
    let escape = press(keyboard::Key::Named(key::Named::Escape), none);
    assert!(escape_key(((None, true), escape)).is_some());
}

#[test]
fn tab_is_taken_by_a_line_s_fields_once_it_starts() {
    let (mut varde, _, _) = sketching_in_open_file();
    assert!(!varde.tab_taken());
    let _ = varde.update(Message::Ui(Ui::Look(Look::SelectTool(
        varde_view::Tool::Line,
    ))));
    assert!(!varde.tab_taken());
    place_point(&mut varde);
    assert!(varde.tab_taken());
}

/// The sketches of `doc` by name, each with whether it's visible.
fn sketches(doc: &Doc) -> Vec<(&str, bool)> {
    let features = doc.editor.document().features();
    features
        .iter()
        .map(|feature| (feature.name.as_str(), feature.visible))
        .collect()
}

/// The sketch being edited in `doc`, if one is.
fn edited(doc: &Doc) -> Option<FeatureId> {
    doc.sketch.as_ref().map(|session| session.feature)
}

/// Lets the camera's animation run to its end.
fn settle_camera(doc: &mut Doc) {
    doc.animation_frame(Instant::now() + 2 * CAMERA_ANIMATION);
    assert!(!doc.animating());
}

#[test]
fn a_new_sketch_is_made_on_the_plane_picked_and_entered() {
    let mut doc = untitled();
    doc.look(Look::SelectPanel(Panel::Timeline));
    doc.look(Look::PickPlane);
    assert!(doc.picking_plane.is_some());
    // Again backs out, and so does Escape.
    doc.look(Look::PickPlane);
    assert!(doc.picking_plane.is_none());
    doc.look(Look::PickPlane);
    doc.look(Look::Escape);
    assert!(doc.picking_plane.is_none());
    assert_eq!(edited(&doc), None);

    doc.look(Look::PickPlane);
    doc.update(Edit::PlanePicked(OriginPlane::XZ));
    assert!(doc.picking_plane.is_none());
    assert_eq!(sketches(&doc), [("Sketch 1", true)]);
    let feature = doc.editor.document().features()[0].id;
    assert_eq!(edited(&doc), Some(feature));
    assert_eq!(doc.panel, Panel::Sketch);
    assert_eq!(doc.selected_feature, Some(feature));

    // The camera turns to face the plane, from the side its normal points
    // to, centred on it.
    assert!(doc.animating());
    settle_camera(&mut doc);
    assert!(doc.camera.backward().abs_diff_eq(-Vec3::Y, 1e-5));
    assert!(doc.camera.up().abs_diff_eq(Vec3::Z, 1e-5));
    assert_eq!(doc.camera.target().y, 0.0);

    // One undoable edit.
    doc.update(Edit::Undo);
    assert!(!doc.editor.can_undo());
}

#[test]
fn a_sketch_is_not_made_in_a_read_only_document() {
    let mut editor = Editor::new(Document::default());
    editor
        .apply(
            editor
                .document()
                .add_sketch(varde_document::Plane::Origin(OriginPlane::XY)),
        )
        .unwrap();
    let mut doc = Doc::new(
        editor.document().clone(),
        Origin::new(
            Target::None,
            Access::ReadOnly(ReadOnly::InUse),
            "a".to_owned(),
        ),
    );
    doc.look(Look::PickPlane);
    assert!(doc.picking_plane.is_none());
    doc.update(Edit::PlanePicked(OriginPlane::XY));
    assert_eq!(sketches(&doc).len(), 1);
    assert_eq!(edited(&doc), None);

    // Its sketches can still be looked into, but not changed.
    let feature = doc.editor.document().features()[0].id;
    doc.look(Look::EditFeature(feature));
    assert_eq!(edited(&doc), Some(feature));
    doc.update(Edit::ToggleFeatureVisible(feature));
    assert_eq!(sketches(&doc), [("Sketch 1", true)]);
}

/// A document with a sketch on XY, not entered, whose regeneration
/// requests wait for the test, and the list they wait in.
pub(crate) fn with_sketch() -> (Doc, FeatureId, Rc<RefCell<Vec<Request>>>) {
    let (mut doc, requests) = deferred();
    doc.update(Edit::PlanePicked(OriginPlane::XY));
    let feature = edited(&doc).unwrap();
    doc.look(Look::FinishSketch);
    answer(&mut doc, &requests);
    (doc, feature, requests)
}

/// What the requests waiting asked to leave out, answering them.
fn left_out(doc: &mut Doc, requests: &RefCell<Vec<Request>>) -> Vec<Option<FeatureId>> {
    let asked = requests.borrow().iter().map(Request::exclude).collect();
    answer(doc, requests);
    asked
}

#[test]
fn a_sketch_is_left_out_of_the_model_while_it_is_edited() {
    let (mut doc, feature, requests) = with_sketch();
    let generation = doc.editor.generation();
    assert_eq!(doc.feed.left_out(), Some(None));

    // Entering asks again for the same generation, without the sketch.
    doc.look(Look::EditFeature(feature));
    assert_eq!(left_out(&mut doc, &requests), [Some(feature)]);
    assert_eq!(doc.feed.generation(), Some(generation));
    assert_eq!(doc.feed.left_out(), Some(Some(feature)));
    assert_eq!(doc.feed.status(&doc.editor), MeshStatus::Current);

    // Edits in it keep it out.
    doc.update(an_edit(&doc));
    assert_eq!(left_out(&mut doc, &requests), [Some(feature)]);

    // Leaving asks for it back.
    doc.look(Look::Escape);
    assert_eq!(edited(&doc), None);
    assert_eq!(left_out(&mut doc, &requests), [None]);
    assert_eq!(doc.feed.left_out(), Some(None));
}

#[test]
fn the_sketch_tab_takes_the_timeline_s_place_in_a_sketch() {
    let (mut doc, feature, _) = with_sketch();
    doc.look(Look::SelectPanel(Panel::Timeline));

    doc.look(Look::EditFeature(feature));
    assert_eq!(doc.panel, Panel::Sketch);
    assert_eq!(doc.panel.other(true), Panel::Objects);
    // The Timeline isn't there to pick.
    doc.look(Look::SelectPanel(Panel::Timeline));
    assert_eq!(doc.panel, Panel::Sketch);
    doc.look(Look::SelectPanel(Panel::Objects));
    assert_eq!(doc.panel.other(true), Panel::Sketch);
    // Objects stays chosen after leaving.
    doc.look(Look::FinishSketch);
    assert_eq!(doc.panel, Panel::Objects);
    assert_eq!(doc.panel.other(false), Panel::Timeline);

    doc.look(Look::SelectPanel(Panel::Sketch));
    assert_eq!(doc.panel, Panel::Timeline);
    doc.look(Look::EditFeature(feature));
    doc.look(Look::FinishSketch);
    assert_eq!(doc.panel, Panel::Timeline);
}

#[test]
fn escape_backs_out_of_one_thing_at_a_time() {
    let (mut doc, feature, _) = with_sketch();
    doc.look(Look::EditFeature(feature));
    doc.update(Edit::ToggleFileMenu);
    doc.look(Look::Escape);
    assert!(!doc.file_menu);
    assert_eq!(edited(&doc), Some(feature));
    doc.look(Look::Escape);
    assert_eq!(edited(&doc), None);
    // The sketch stays selected in the Timeline, until the next Escape.
    assert_eq!(doc.selected_feature, Some(feature));
    doc.look(Look::Escape);
    assert_eq!(doc.selected_feature, None);
}

/// Right-clicking a feature in the Timeline selects it and opens its
/// context menu, which `Esc` or a press off it closes alone, and which
/// edits or deletes the feature.
#[test]
fn a_feature_s_context_menu_edits_or_deletes_it() {
    let (mut doc, feature, _) = with_sketch();
    doc.look(Look::SelectPanel(Panel::Timeline));
    doc.look(Look::OpenMenu(RowMenu::Feature(feature)));
    assert_eq!(doc.row_menu, Some(RowMenu::Feature(feature)));
    assert_eq!(doc.selected_feature, Some(feature));
    doc.look(Look::Escape);
    assert_eq!(doc.row_menu, None);
    assert_eq!(doc.selected_feature, Some(feature));
    doc.look(Look::OpenMenu(RowMenu::Feature(feature)));
    doc.look(Look::CloseMenu);
    assert_eq!(doc.row_menu, None);
    assert_eq!(doc.selected_feature, Some(feature));

    doc.look(Look::OpenMenu(RowMenu::Feature(feature)));
    doc.look(Look::EditFeature(feature));
    assert_eq!(doc.row_menu, None);
    assert_eq!(edited(&doc), Some(feature));
    // Not opened in a sketch, where the Timeline isn't.
    doc.look(Look::OpenMenu(RowMenu::Feature(feature)));
    assert_eq!(doc.row_menu, None);
    doc.look(Look::FinishSketch);

    doc.look(Look::OpenMenu(RowMenu::Feature(feature)));
    doc.update(Edit::RemoveFeature(feature));
    assert_eq!(doc.row_menu, None);
    assert!(sketches(&doc).is_empty());
    assert_eq!(doc.selected_feature, None);
}

/// A sketch's menu in the Timeline hides it, and then shows it.
#[test]
fn a_sketch_s_timeline_menu_hides_and_shows_it() {
    use iced_runtime::user_interface::UserInterface;
    let (mut doc, feature, _) = with_sketch();
    doc.look(Look::SelectPanel(Panel::Timeline));
    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();
    for (item, visible) in [("Hide", false), ("Show", true)] {
        doc.look(Look::OpenMenu(RowMenu::Feature(feature)));
        let cache = iced_runtime::user_interface::Cache::default();
        let mut ui = UserInterface::build(doc.view_in(Mode::Light), size, cache, &mut renderer);
        let shown = texts(&mut ui, &renderer);
        let at = (shown.iter()).find(|t| t.text == item);
        let at = at.unwrap_or_else(|| panic!("no {item}: {shown:?}"));
        let sent = clicked(&mut ui, &mut renderer, at.bounds.center());
        drop(ui);
        let toggle = sent
            .iter()
            .find(|m| matches!(m, Ui::Edit(Edit::ToggleFeatureVisible(f)) if *f == feature));
        assert!(toggle.is_some(), "{sent:?}");
        doc.update(Edit::ToggleFeatureVisible(feature));
        assert_eq!(sketches(&doc)[0].1, visible);
        assert_eq!(doc.row_menu, None);
    }
}

/// The context menu, headless: right-clicking the sketch's row opens it
/// where it was clicked, and its Delete deletes the sketch.
#[test]
fn a_right_click_on_a_timeline_row_opens_its_menu_there() {
    use iced::mouse::{Button, Cursor, Event};
    use iced_runtime::user_interface::UserInterface;
    let (mut doc, feature, _) = with_sketch();
    doc.look(Look::SelectPanel(Panel::Timeline));
    let name = doc.editor.document().feature(feature).unwrap().name.clone();
    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();

    let mut ui = shown(doc.view_in(Mode::Light), size, &mut renderer);
    let row = texts(&mut ui, &renderer);
    let row = row.iter().find(|t| t.text == name).unwrap();
    let at = row.seen().center();
    let mut sent = Vec::new();
    for event in [
        Event::CursorMoved { position: at },
        Event::ButtonPressed(Button::Right),
    ] {
        let _ = ui.update(
            &[iced::Event::Mouse(event)],
            Cursor::Available(at),
            &mut renderer,
            &mut iced::advanced::clipboard::Null,
            &mut sent,
        );
    }
    let cache = ui.into_cache();
    // Moving onto the row hovers it first.
    let [
        Ui::Look(Look::HoverFeature(Some(_))),
        Ui::Look(Look::OpenMenu(menu)),
    ] = sent[..]
    else {
        panic!("{sent:?}");
    };
    assert_eq!(menu, RowMenu::Feature(feature));
    doc.look(Look::OpenMenu(menu));

    let mut ui = UserInterface::build(doc.view_in(Mode::Light), size, cache, &mut renderer);
    let shown = texts(&mut ui, &renderer);
    let edit = shown.iter().find(|t| t.text == "Edit sketch").unwrap();
    // The menu's top left is at the click: the item's text is right of
    // its padding and icon, and a little down.
    let (dx, dy) = (edit.bounds.x - at.x, edit.bounds.y - at.y);
    assert!(
        (0.0..50.0).contains(&dx) && (0.0..20.0).contains(&dy),
        "{edit:?} {at:?}"
    );
    let delete = (shown.iter())
        .find(|t| t.text == "Delete" && t.bounds.x == edit.bounds.x)
        .unwrap();
    let sent = clicked(&mut ui, &mut renderer, delete.bounds.center());
    drop(ui);
    // Moving onto the menu leaves the row.
    let [
        Ui::Look(Look::LeaveFeature(_)),
        Ui::Edit(Edit::RemoveFeature(id)),
    ] = sent[..]
    else {
        panic!("{sent:?}");
    };
    doc.update(Edit::RemoveFeature(id));
    assert!(sketches(&doc).is_empty());
    assert_eq!(doc.row_menu, None);
}

/// A body's and a sketch's context menus in Objects: Hide, then Show,
/// leave the menu closed, and an undo that takes the body away takes its
/// menu too.
#[test]
fn objects_have_context_menus() {
    let (mut doc, _) = example();
    doc.look(Look::SelectPanel(Panel::Objects));
    let body = doc.editor.document().bodies()[0].id;
    let sketch = doc.editor.document().features()[0].id;

    doc.look(Look::OpenMenu(RowMenu::Body(body)));
    assert_eq!(doc.row_menu, Some(RowMenu::Body(body)));
    // Not selected in the Timeline: the Objects' rows aren't.
    assert_eq!(doc.selected_feature, None);
    doc.update(Edit::ToggleVisible(body));
    assert_eq!(doc.row_menu, None);
    assert!(!doc.editor.document().body(body).unwrap().visible);

    doc.look(Look::OpenMenu(RowMenu::Sketch(sketch)));
    doc.look(Look::Escape);
    assert_eq!(doc.row_menu, None);
    doc.look(Look::OpenMenu(RowMenu::Sketch(sketch)));
    doc.look(Look::EditFeature(sketch));
    assert_eq!(doc.row_menu, None);
    assert_eq!(edited(&doc), Some(sketch));
    doc.look(Look::FinishSketch);

    // Gone with the body.
    doc.update(Edit::RemoveBody(body));
    doc.look(Look::OpenMenu(RowMenu::Body(body)));
    assert_eq!(doc.row_menu, None);
}

/// Headless: right-clicking a body in Objects asks for its menu, which
/// offers hiding and deleting it.
#[test]
fn a_right_click_on_a_body_opens_its_menu() {
    use iced::mouse::{Button, Cursor, Event};
    use iced_runtime::user_interface::UserInterface;
    let (mut doc, _) = example();
    doc.look(Look::SelectPanel(Panel::Objects));
    let body = doc.editor.document().bodies()[0].clone();
    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();

    let mut ui = shown(doc.view_in(Mode::Light), size, &mut renderer);
    let row = texts(&mut ui, &renderer);
    let at = row
        .iter()
        .find(|t| t.text == body.name)
        .unwrap()
        .seen()
        .center();
    let mut sent = Vec::new();
    for event in [
        Event::CursorMoved { position: at },
        Event::ButtonPressed(Button::Right),
    ] {
        let _ = ui.update(
            &[iced::Event::Mouse(event)],
            Cursor::Available(at),
            &mut renderer,
            &mut iced::advanced::clipboard::Null,
            &mut sent,
        );
    }
    let cache = ui.into_cache();
    let [Ui::Look(Look::OpenMenu(menu))] = sent[..] else {
        panic!("{sent:?}");
    };
    assert_eq!(menu, RowMenu::Body(body.id));
    doc.look(Look::OpenMenu(menu));

    let view = doc.view_in(Mode::Light);
    let mut ui = UserInterface::build(view, size, cache, &mut renderer);
    let shown = texts(&mut ui, &renderer);
    assert!(
        !shown.iter().any(|t| t.text.starts_with("Edit")),
        "{shown:?}"
    );
    let hide = shown.iter().find(|t| t.text == "Hide").unwrap();
    let sent = clicked(&mut ui, &mut renderer, hide.bounds.center());
    let [Ui::Edit(Edit::ToggleVisible(id))] = sent[..] else {
        panic!("{sent:?}");
    };
    assert_eq!(id, body.id);
    assert!(shown.iter().any(|t| t.text == "Delete"));
}

/// How opaque the viewport draws each part of `doc`'s mesh.
fn part_opacity(doc: &Doc) -> Vec<f32> {
    let state = doc.state(
        false,
        Mode::Light,
        ViewOptions::default(),
        Offers::default(),
    );
    state.part_opacity().to_vec()
}

/// The example's body and how opaque the document has it.
fn body_opacity(doc: &Doc) -> (varde_document::BodyId, varde_document::Opacity) {
    let body = &doc.editor.document().bodies()[0];
    (body.id, body.opacity)
}

fn percent(percent: u8) -> varde_document::Opacity {
    varde_document::Opacity::new(percent).unwrap()
}

/// Dragging the Opacity slider previews without editing the document,
/// shortcuts off; letting go sets it as one undo step, the menu left open.
#[test]
fn the_opacity_slider_previews_then_commits_one_step() {
    let (mut doc, _) = example();
    doc.look(Look::SelectPanel(Panel::Objects));
    let (body, before) = body_opacity(&doc);
    assert!(before.is_opaque());
    assert!(!part_opacity(&doc).is_empty());
    assert!(part_opacity(&doc).iter().all(|&alpha| alpha == 1.0));
    let generation = doc.editor.generation();

    // Not without the body's menu open.
    doc.look(Look::PreviewOpacity(body, percent(40)));
    assert_eq!(doc.opacity_preview, None);

    doc.look(Look::OpenMenu(RowMenu::Body(body)));
    for step in [30, 55, 40] {
        doc.look(Look::PreviewOpacity(body, percent(step)));
    }
    assert_eq!(doc.row_menu, Some(RowMenu::Body(body)));
    assert_eq!(doc.editor.generation(), generation);
    assert_eq!(body_opacity(&doc).1, before);
    assert!(part_opacity(&doc).iter().all(|&alpha| alpha == 0.4));
    assert!(doc.keys().is_none());

    doc.update(Edit::CommitOpacity);
    assert_eq!(doc.opacity_preview, None);
    assert_eq!(doc.row_menu, Some(RowMenu::Body(body)));
    assert_eq!(body_opacity(&doc).1, percent(40));
    assert!(part_opacity(&doc).iter().all(|&alpha| alpha == 0.4));
    assert!(doc.keys().is_some());

    doc.update(Edit::Undo);
    assert_eq!(body_opacity(&doc).1, before);
    assert!(part_opacity(&doc).iter().all(|&alpha| alpha == 1.0));
    doc.update(Edit::Redo);
    assert_eq!(body_opacity(&doc).1, percent(40));
}

/// `Esc` mid-drag, or the menu closing, goes back to the body's own
/// opacity, and letting go after changes nothing.
#[test]
fn escape_or_closing_the_menu_drops_the_opacity_preview() {
    let (mut doc, _) = example();
    doc.look(Look::SelectPanel(Panel::Objects));
    let (body, before) = body_opacity(&doc);
    let generation = doc.editor.generation();
    for close in [
        Look::Escape,
        Look::CloseMenu,
        Look::SelectPanel(Panel::Timeline),
    ] {
        doc.look(Look::SelectPanel(Panel::Objects));
        doc.look(Look::OpenMenu(RowMenu::Body(body)));
        doc.look(Look::PreviewOpacity(body, percent(25)));
        assert!(part_opacity(&doc).iter().all(|&alpha| alpha == 0.25));
        doc.look(close.clone());
        assert_eq!(doc.row_menu, None, "{close:?}");
        assert_eq!(doc.opacity_preview, None, "{close:?}");
        assert!(part_opacity(&doc).iter().all(|&alpha| alpha == 1.0));
        doc.update(Edit::CommitOpacity);
        assert_eq!(doc.editor.generation(), generation, "{close:?}");
        assert_eq!(body_opacity(&doc).1, before);
    }
    // An edit closes the menu too.
    doc.look(Look::OpenMenu(RowMenu::Body(body)));
    doc.look(Look::PreviewOpacity(body, percent(25)));
    doc.update(Edit::SetUnits(LengthUnit::In));
    assert_eq!(doc.opacity_preview, None);
}

/// Letting go of the slider where it started, or with nothing previewed,
/// adds no history.
#[test]
fn a_release_changing_nothing_adds_no_undo_step() {
    let (mut doc, _) = example();
    doc.look(Look::SelectPanel(Panel::Objects));
    let (body, before) = body_opacity(&doc);
    let generation = doc.editor.generation();
    doc.look(Look::OpenMenu(RowMenu::Body(body)));
    doc.update(Edit::CommitOpacity);
    doc.look(Look::PreviewOpacity(body, percent(60)));
    doc.look(Look::PreviewOpacity(body, before));
    doc.update(Edit::CommitOpacity);
    assert_eq!(doc.editor.generation(), generation);
    assert_eq!(doc.opacity_preview, None);
    // Undo still takes back the example.
    doc.update(Edit::Undo);
    assert!(doc.editor.document().bodies().is_empty());
}

/// The body menu's Opacity slider, dragged, previews without closing the
/// menu and commits once on release; it takes no keys.
#[test]
fn the_body_menu_s_opacity_slider_is_dragged() {
    use iced::mouse::{Button, Cursor, Event};
    let (mut doc, _) = example();
    doc.look(Look::SelectPanel(Panel::Objects));
    let (body, _) = body_opacity(&doc);
    doc.look(Look::OpenMenu(RowMenu::Body(body)));
    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();

    let mut ui = shown(doc.view_in(Mode::Light), size, &mut renderer);
    let labels = texts(&mut ui, &renderer);
    let heading = labels.iter().find(|t| t.text == "Opacity").unwrap();
    let value = labels.iter().find(|t| t.text == "100 %").unwrap();
    // The slider runs under the heading up to the value, on its line.
    let y = value.bounds.center_y();
    assert!(value.bounds.y > heading.bounds.y, "{heading:?} {value:?}");
    let (left, right) = (heading.bounds.x, value.bounds.x - 10.0);
    let middle = iced::Point::new((left + right) / 2.0, y);
    let mut sent = Vec::new();
    let mut update = |ui: &mut Headless<'_>, event, at| {
        let (_, statuses) = ui.update(
            &[event],
            Cursor::Available(at),
            &mut renderer,
            &mut iced::advanced::clipboard::Null,
            &mut sent,
        );
        statuses
    };
    // An arrow key over it is left for the shortcuts.
    let up = iced::Event::Keyboard(press(
        keyboard::Key::Named(key::Named::ArrowUp),
        Default::default(),
    ));
    let statuses = update(&mut ui, up, middle);
    assert_eq!(statuses, [iced::event::Status::Ignored]);
    let start = iced::Point::new(left + 1.0, y);
    for (event, at) in [
        (Event::CursorMoved { position: start }, start),
        (Event::ButtonPressed(Button::Left), start),
        (Event::CursorMoved { position: middle }, middle),
        (Event::ButtonReleased(Button::Left), middle),
    ] {
        update(&mut ui, iced::Event::Mouse(event), at);
    }
    drop(ui);
    let [
        Ui::Look(Look::PreviewOpacity(first, low)),
        Ui::Look(Look::PreviewOpacity(_, dragged)),
        Ui::Edit(Edit::CommitOpacity),
    ] = sent[..]
    else {
        panic!("{sent:?}");
    };
    assert_eq!((first, low), (body, varde_document::Opacity::MIN));
    // Halfway along, on a step of 5.
    assert!((50..=60).contains(&dragged.percent()), "{dragged}");
    assert_eq!(dragged.percent() % 5, 0);
    for message in sent {
        match message {
            Ui::Look(look) => doc.look(look),
            Ui::Edit(edit) => doc.update(edit),
            _ => unreachable!(),
        }
    }
    assert_eq!(doc.row_menu, Some(RowMenu::Body(body)));
    assert_eq!(body_opacity(&doc).1, dragged);
    let percent = dragged.to_string();
    let mut ui = shown(doc.view_in(Mode::Light), size, &mut renderer);
    assert!(texts(&mut ui, &renderer).iter().any(|t| t.text == percent));
}

/// The peek key mid-drag doesn't swap the Objects tab, and the slider with
/// it, for the Timeline: the slider would never see its release, leaving
/// the preview uncommitted and the shortcuts off.
#[test]
fn the_peek_key_leaves_the_opacity_slider_being_dragged() {
    let (mut doc, _) = example();
    doc.look(Look::SelectPanel(Panel::Objects));
    let (body, _) = body_opacity(&doc);
    doc.look(Look::OpenMenu(RowMenu::Body(body)));
    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();
    let peeking = |doc: &Doc, renderer: &mut iced::Renderer| {
        let view = doc.view(true, Mode::Light, ViewOptions::default(), Offers::default());
        let mut ui = shown(view, size, renderer);
        let labels = texts(&mut ui, renderer);
        labels.iter().any(|t| t.text == "Opacity")
    };
    // Not dragged, the peek shows the other tab, as it does any menu.
    assert!(!peeking(&doc, &mut renderer));
    doc.look(Look::PreviewOpacity(body, percent(40)));
    assert!(peeking(&doc, &mut renderer));
    doc.update(Edit::CommitOpacity);
    assert!(!peeking(&doc, &mut renderer));
}

/// `Ctrl Z` and `Ctrl Shift Z` undo and redo while there's something to.
#[test]
fn undo_and_redo_keys_follow_the_history() {
    let command = keyboard::Modifiers::COMMAND;
    let z = keyboard::Key::Character("z".into());
    let (mut doc, _, _) = with_sketch();
    let pressed = |doc: &Doc, modifiers| {
        varde_view::pressed(
            varde_view::document_bindings(doc.keys().unwrap()),
            &z,
            modifiers,
        )
    };
    assert!(matches!(pressed(&doc, command), Some(Ui::Edit(Edit::Undo))));
    assert!(pressed(&doc, command | keyboard::Modifiers::SHIFT).is_none());
    doc.update(Edit::Undo);
    assert!(sketches(&doc).is_empty());
    assert!(pressed(&doc, command).is_none());
    assert!(matches!(
        pressed(&doc, command | keyboard::Modifiers::SHIFT),
        Some(Ui::Edit(Edit::Redo))
    ));
}

/// Leaving a sketch turns the camera back to the view it had before,
/// in the projection picked meanwhile; one sketch entered from another
/// comes back to the view before the first.
#[test]
fn leaving_a_sketch_turns_the_camera_back() {
    let (mut doc, feature, _) = with_sketch();
    doc.look(Look::Orbit {
        yaw: 0.4,
        pitch: 0.3,
    });
    let before = doc.camera;
    doc.look(Look::EditFeature(feature));
    settle_camera(&mut doc);
    assert_ne!(doc.camera, before);
    doc.look(Look::SetProjection(varde_render::Projection::Perspective));
    doc.look(Look::FinishSketch);
    settle_camera(&mut doc);
    let mut expected = before;
    expected.set_projection(varde_render::Projection::Perspective);
    assert_eq!(doc.camera, expected);

    // Entered while still turning: back to where it was turning to.
    doc.look(Look::EditFeature(feature));
    doc.look(Look::FinishSketch);
    doc.look(Look::EditFeature(feature));
    settle_camera(&mut doc);
    doc.look(Look::FinishSketch);
    settle_camera(&mut doc);
    assert_eq!(doc.camera, expected);
}

#[test]
fn undoing_a_sketch_s_creation_leaves_it() {
    let (mut doc, requests) = deferred();
    doc.look(Look::SelectPanel(Panel::Timeline));
    doc.update(Edit::PlanePicked(OriginPlane::YZ));
    answer(&mut doc, &requests);
    assert!(edited(&doc).is_some());

    doc.update(Edit::Undo);
    assert_eq!(edited(&doc), None);
    assert_eq!(doc.selected_feature, None);
    assert_eq!(doc.panel, Panel::Timeline);
    assert_eq!(left_out(&mut doc, &requests), [None]);

    // Redo brings the sketch back, but doesn't enter it.
    doc.update(Edit::Redo);
    assert_eq!(sketches(&doc).len(), 1);
    assert_eq!(edited(&doc), None);
}

#[test]
fn sketches_are_hidden_and_shown_from_objects() {
    let (mut doc, feature, requests) = with_sketch();
    doc.update(Edit::ToggleFeatureVisible(feature));
    assert_eq!(sketches(&doc), [("Sketch 1", false)]);
    answer(&mut doc, &requests);
    doc.update(Edit::ToggleFeatureVisible(feature));
    assert_eq!(sketches(&doc), [("Sketch 1", true)]);
    doc.update(Edit::Undo);
    assert_eq!(sketches(&doc), [("Sketch 1", false)]);
}

/// Pressing `key` on the document screen of `doc`, as the app's
/// subscription hears it.
pub(crate) fn press_in(doc: &Doc, key: keyboard::Key) -> Option<Message> {
    crate::keys::document_key((doc.keys(), press(key, Default::default())))
}

/// Sends what pressing `key` in `doc` sends to it, if anything.
pub(crate) fn key_in(doc: &mut Doc, key: keyboard::Key) {
    match press_in(doc, key) {
        Some(Message::Ui(Ui::Edit(edit))) => doc.update(edit),
        Some(Message::Ui(Ui::Look(look))) => doc.look(look),
        Some(other) => panic!("unexpected {other:?}"),
        None => {}
    }
}

#[test]
fn the_selected_feature_is_edited_with_enter_and_deleted_with_delete() {
    let (mut doc, feature, _) = with_sketch();
    let enter = || keyboard::Key::Named(key::Named::Enter);
    let delete = || keyboard::Key::Named(key::Named::Delete);
    doc.look(Look::Escape);
    assert!(press_in(&doc, enter()).is_none());
    assert!(press_in(&doc, delete()).is_none());

    doc.look(Look::SelectFeature(feature));
    key_in(&mut doc, enter());
    assert_eq!(edited(&doc), Some(feature));
    // In the sketch, they're the sketch's, which has nothing selected.
    assert!(press_in(&doc, delete()).is_none());
    doc.look(Look::FinishSketch);

    key_in(&mut doc, delete());
    assert!(sketches(&doc).is_empty());
    assert_eq!(doc.selected_feature, None);
    doc.update(Edit::Undo);
    assert_eq!(sketches(&doc).len(), 1);
}

#[test]
fn s_starts_a_sketch_outside_sketches() {
    let (mut doc, feature, _) = with_sketch();
    let s = || keyboard::Key::Character("s".into());
    key_in(&mut doc, s());
    assert!(doc.picking_plane.is_some());
    doc.look(Look::EditFeature(feature));
    assert!(doc.picking_plane.is_none());
    assert!(press_in(&doc, s()).is_none());
    doc.look(Look::PickPlane);
    assert!(doc.picking_plane.is_none());
}

#[test]
fn no_shortcut_acts_under_the_unsaved_changes_prompt() {
    let mut cx = Files::new(None);
    let file = FileId(4);
    let mut doc = Doc::new(
        Document::default(),
        Origin::new(
            Target::File { file, picked: None },
            Access::Edit,
            "Part".to_owned(),
        ),
    );
    doc.update(Edit::PlanePicked(OriginPlane::XY));
    let feature = edited(&doc).unwrap();
    doc.look(Look::FinishSketch);
    doc.look(Look::SelectFeature(feature));
    let delete = || keyboard::Key::Named(key::Named::Delete);
    assert!(press_in(&doc, delete()).is_some());

    assert!(matches!(doc.leave(&mut cx, Leave::Close), Next::Stay));
    assert_eq!(doc.prompt(), Some(Leave::Close));
    // Only the prompt's buttons and Escape answer it: the keys don't edit
    // the document behind it.
    assert!(press_in(&doc, delete()).is_none());
    assert!(press_in(&doc, keyboard::Key::Character("s".into())).is_none());
    assert!(press_in(&doc, keyboard::Key::Named(key::Named::Space)).is_none());
}

#[test]
fn geometry_selected_is_what_the_sketch_holds() {
    use glam::DVec2;

    let (mut doc, feature, _) = with_sketch();
    let mut sketch = varde_document::Sketch::default();
    let a = sketch.add_point(DVec2::ZERO).unwrap();
    let b = sketch.add_point(DVec2::new(40.0, 20.0)).unwrap();
    let line = sketch
        .add_curve(varde_sketch::Curve::Line { start: a, end: b }, false)
        .unwrap();
    doc.editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch.clone()),
        })
        .unwrap();
    doc.look(Look::EditFeature(feature));

    // Home frames what's drawn, facing the plane from above.
    doc.look(Look::Orbit {
        yaw: 1.0,
        pitch: -0.5,
    });
    doc.look(Look::ResetCamera);
    settle_camera(&mut doc);
    assert!(doc.camera.backward().abs_diff_eq(Vec3::Z, 1e-5));
    assert!(doc.camera.up().abs_diff_eq(Vec3::Y, 1e-5));
    assert!(
        doc.camera
            .target()
            .abs_diff_eq(Vec3::new(20.0, 10.0, 0.0), 1e-4)
    );
    assert!(doc.camera.view_height() >= 40.0);

    let selection = |doc: &Doc| doc.sketch.as_ref().unwrap().selection.clone();
    doc.look(Look::ClickGeometry {
        hit: Some(line),
        add: false,
    });
    doc.look(Look::ClickGeometry {
        hit: Some(b),
        add: false,
    });
    assert_eq!(selection(&doc).into_iter().collect::<Vec<_>>(), [b]);

    // What an edit takes away is no longer selected.
    sketch.delete(&[line]);
    doc.editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    doc.sync();
    assert!(selection(&doc).is_empty());
    doc.look(Look::ClickGeometry {
        hit: Some(b),
        add: false,
    });
    assert!(selection(&doc).is_empty());
}

#[test]
fn a_sketch_is_made_and_left_through_the_app() {
    let mut varde = Varde::new();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    let _ = varde.update(Message::Ui(Ui::Look(Look::PickPlane)));
    assert!(document(&varde).picking_plane.is_some());
    let _ = varde.view();
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::PlanePicked(OriginPlane::XY))));
    assert!(edited(document(&varde)).is_some());
    let _ = varde.view();
    // Peeking in a sketch shows Objects, and the Sketch tab from it.
    let _ = varde.update(Message::PeekPanel(true));
    let _ = varde.view();
    let _ = varde.update(Message::Ui(Ui::Look(Look::SelectPanel(Panel::Objects))));
    let _ = varde.update(Message::PeekPanel(true));
    let _ = varde.view();
    let _ = varde.update(Message::Ui(Ui::Look(Look::Escape)));
    assert!(edited(document(&varde)).is_none());
    let _ = varde.view();
}

/// Pressing `key`, which types `text` if any.
pub(crate) fn typing(key: keyboard::Key, text: Option<&str>) -> iced::Event {
    let mut event = press(key, keyboard::Modifiers::empty());
    if let keyboard::Event::KeyPressed { text: typed, .. } = &mut event {
        *typed = text.map(Into::into);
    }
    iced::Event::Keyboard(event)
}

/// A screen shown headless, see [`shown`].
pub(crate) type Headless<'a> =
    iced_runtime::user_interface::UserInterface<'a, Ui, iced::Theme, iced::Renderer>;

/// `view` shown headless in a window of `size`, after a first redraw,
/// which sets the widgets' status as the app has it.
pub(crate) fn shown<'a>(
    view: iced::Element<'a, Ui>,
    size: iced::Size,
    renderer: &mut iced::Renderer,
) -> Headless<'a> {
    use iced_runtime::user_interface::{Cache, UserInterface};
    let mut ui = UserInterface::build(view, size, Cache::default(), renderer);
    let redraw = iced::Event::Window(iced::window::Event::RedrawRequested(Instant::now()));
    let mut sent = Vec::new();
    let _ = ui.update(
        &[redraw],
        iced::mouse::Cursor::Unavailable,
        renderer,
        &mut iced::advanced::clipboard::Null,
        &mut sent,
    );
    ui
}

/// Each text `ui` shows, see [`varde_view::probe::Texts`].
pub(crate) fn texts(
    ui: &mut Headless<'_>,
    renderer: &iced::Renderer,
) -> Vec<varde_view::probe::Shown> {
    let mut find = varde_view::probe::Texts::default();
    ui.operate(renderer, &mut find);
    find.shown
}

/// What a left click at `at` on `ui` sends.
pub(crate) fn clicked(
    ui: &mut Headless<'_>,
    renderer: &mut iced::Renderer,
    at: iced::Point,
) -> Vec<Ui> {
    use iced::mouse::{Button, Cursor, Event};
    let mut sent = Vec::new();
    for event in [
        Event::CursorMoved { position: at },
        Event::ButtonPressed(Button::Left),
        Event::ButtonReleased(Button::Left),
    ] {
        let _ = ui.update(
            &[iced::Event::Mouse(event)],
            Cursor::Available(at),
            renderer,
            &mut iced::advanced::clipboard::Null,
            &mut sent,
        );
    }
    sent
}

/// What pressing each of `keys` does to the document screen of `doc`
/// shown headless: the messages its widgets send, and those the app's
/// shortcuts send for what the widgets leave, as `keyboard::listen` hands
/// the app only the events no widget captured. With the value field
/// focused first, if `focused`.
pub(crate) fn pressed(doc: &Doc, keys: &[iced::Event], focused: bool) -> (Vec<Ui>, Vec<Message>) {
    use iced::advanced::widget::operation::{self, focusable};
    use iced_runtime::user_interface::{Cache, UserInterface};

    let mut renderer = varde_view::probe::renderer();
    let view = doc.view_in(Mode::Light);
    let mut ui = UserInterface::build(
        view,
        iced::Size::new(1280.0, 800.0),
        Cache::default(),
        &mut renderer,
    );
    // As the app has it when the field opens.
    if focused {
        let mut focus = focusable::focus(varde_view::VALUE_FIELD);
        ui.operate(&renderer, &mut focus);
        let mut select = operation::text_input::select_all(varde_view::VALUE_FIELD);
        ui.operate(&renderer, &mut select);
    }
    let mut sent = Vec::new();
    let mut shortcuts = Vec::new();
    for key in keys {
        let (_, statuses) = ui.update(
            std::slice::from_ref(key),
            iced::mouse::Cursor::Unavailable,
            &mut renderer,
            &mut iced::advanced::clipboard::Null,
            &mut sent,
        );
        if let (iced::Event::Keyboard(event), [iced::event::Status::Ignored]) =
            (key, statuses.as_slice())
        {
            shortcuts.extend(crate::keys::document_key((doc.keys(), event.clone())));
        }
    }
    (sent, shortcuts)
}

#[test]
fn closing_waits_for_a_delete_behind_sketch_edits_to_be_asked_and_answered() {
    let (mut varde, _requests) = with_open_file();
    let doc = varde.screen.doc_mut().unwrap();
    doc.apply(Command::Replace(Box::new(Document::example())));
    doc.sync();
    let example = doc.editor.document().features()[0].id;
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::PlanePicked(OriginPlane::XY))));
    let mut lane = SolveLane::new();
    varde
        .screen
        .doc_mut()
        .unwrap()
        .solver_ready(lane.transport());
    solve(&mut varde, &mut lane);
    let _ = varde.update(Message::Ui(Ui::Look(Look::SelectTool(
        varde_view::Tool::Point,
    ))));
    place_point(&mut varde);
    // The example's sketch takes its extrude with it: it asks, once made.
    let _ = varde.update(Message::Ui(Ui::Edit(Edit::RemoveFeature(example))));
    let _ = varde.update(Message::Ui(Ui::File(File::CloseDocument)));
    solve(&mut varde, &mut lane);
    assert!(!is_welcome(&varde));
    assert!(document(&varde).delete_prompt().is_some());
    assert_eq!(document(&varde).dialog(), Some(crate::doc::Dialog::Delete));
    // Cancelled, closing goes on, asking about the unsaved changes.
    let _ = varde.update(Message::Ui(Ui::Look(Look::CancelDelete)));
    let doc = document(&varde);
    assert!(doc.editor.document().feature(example).is_some());
    assert_eq!(doc.prompt(), Some(Leave::Close));
    let _ = varde.update(Message::Ui(Ui::File(File::Unsaved(Unsaved::Discard))));
    assert!(is_welcome(&varde));
}

#[test]
fn a_click_on_an_empty_part_of_the_panel_or_toolbar_clears_the_selection() {
    let mut doc = untitled();
    doc.look(Look::PickPlane);
    doc.update(Edit::PlanePicked(OriginPlane::XZ));
    doc.look(Look::FinishSketch);
    doc.look(Look::SelectPanel(Panel::Timeline));
    let feature = doc.editor.document().features()[0].id;
    assert_eq!(doc.selected_feature, Some(feature));

    // Wide enough for the toolbar's operations to leave some of it
    // empty: at 1280 px they take nearly all of it.
    let size = iced::Size::new(1440.0, 800.0);
    let mut renderer = varde_view::probe::renderer();
    let view = doc.view(
        false,
        Mode::Light,
        ViewOptions::default(),
        Offers::default(),
    );
    let mut ui = shown(view, size, &mut renderer);
    let on_screen = texts(&mut ui, &renderer);
    drop(ui);
    let row = on_screen
        .iter()
        .find(|t| t.text == "Sketch 1")
        .unwrap()
        .bounds;
    let tab = on_screen
        .iter()
        .find(|t| t.text == "Objects")
        .unwrap()
        .bounds;
    let mut click = |doc: &Doc, at: iced::Point| {
        let view = doc.view(
            false,
            Mode::Light,
            ViewOptions::default(),
            Offers::default(),
        );
        let mut ui = shown(view, size, &mut renderer);
        clicked(&mut ui, &mut renderer, at)
    };
    let clears = |sent: &[Ui]| matches!(sent, [Ui::Look(Look::ClearSelection)]);

    // The row takes its own click; below it, the strip right of the tabs
    // and the toolbar's empty middle clear.
    assert!(!clears(&click(&doc, row.center())));
    let below = iced::Point::new(row.center_x(), row.y + 200.0);
    let beside_tabs = iced::Point::new(varde_view::SIDE_PANEL_WIDTH - 10.0, tab.center_y());
    // Past the last operation, Measure, and its key.
    let measure = on_screen
        .iter()
        .find(|t| t.text == "Measure" && t.bounds.y < 40.0)
        .unwrap()
        .bounds;
    let toolbar = iced::Point::new(measure.x + measure.width + 40.0, 20.0);
    for at in [below, beside_tabs, toolbar] {
        let sent = click(&doc, at);
        assert!(clears(&sent), "{at:?}: {sent:?}");
    }
    doc.look(Look::ClearSelection);
    assert_eq!(doc.selected_feature, None);
}

#[test]
fn the_view_options_menu_picks_the_projection_and_the_options() {
    fn view(varde: &Varde) -> iced::Element<'_, Ui> {
        document(varde).view(false, Mode::Light, varde.options, varde.files.offers())
    }

    use varde_render::Projection;

    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();
    let mut varde = Varde::new();
    let _ = varde.update(Message::Ui(Ui::Welcome(WelcomeUi::NewDesign)));
    // Regenerated, so the status bar has nothing to say and the mouse's
    // hints show.
    let requests = Rc::default();
    let doc = varde.screen.doc_mut().unwrap();
    doc.feed.connect(Deferred(Rc::clone(&requests)));
    doc.sync();
    answer(doc, &requests);
    let shown = |varde: &Varde, renderer: &mut iced::Renderer| {
        let mut ui = crate::tests::shown(view(varde), size, renderer);
        texts(&mut ui, renderer)
    };
    let menu = [
        "Shading",
        "Edges",
        "Orthographic",
        "Perspective",
        "Mouse hints",
        "Hidden edges",
    ];
    let has = |shown: &[varde_view::probe::Shown], text: &str| shown.iter().any(|t| t.text == text);
    assert!(!has(&shown(&varde, &mut renderer), menu[0]));

    // Its button opens it, at the right above the status bar.
    let _ = varde.update(Message::Ui(Ui::Look(Look::ToggleViewMenu)));
    let open = shown(&varde, &mut renderer);
    for item in menu {
        let item = open.iter().find(|t| t.text == item).unwrap();
        assert!(item.bounds.x > size.width / 2.0, "{item:?}");
        assert!(
            item.bounds.y + item.bounds.height < size.height - varde_view::STATUS_BAR_ROOM,
            "{item:?}"
        );
    }
    assert!(has(&open, "Drag to orbit"));

    // A projection picked closes it.
    let perspective = open.iter().find(|t| t.text == "Perspective").unwrap();
    let mut ui = iced_runtime::user_interface::UserInterface::build(
        view(&varde),
        size,
        Default::default(),
        &mut renderer,
    );
    let sent = clicked(&mut ui, &mut renderer, perspective.bounds.center());
    drop(ui);
    let [Ui::Look(Look::SetProjection(Projection::Perspective))] = sent[..] else {
        panic!("{sent:?}");
    };
    let _ = varde.update(Message::Ui(Ui::Look(Look::SetProjection(
        Projection::Perspective,
    ))));
    assert!(!document(&varde).view_menu);
    assert_eq!(
        document(&varde).camera.projection(),
        Projection::Perspective
    );

    // Mouse hints, off, leave the hints of the keys, and close it too.
    let _ = varde.update(Message::Ui(Ui::Look(Look::ToggleViewMenu)));
    let _ = varde.update(Message::Ui(Ui::ToggleMouseHints));
    assert!(!document(&varde).view_menu);
    let off = shown(&varde, &mut renderer);
    assert!(!has(&off, "Drag to orbit") && !has(&off, "Zoom"), "{off:?}");

    // Hidden edges, on by default, clicked, turn off, leaving the mouse
    // hints be, and close it too.
    assert!(varde.options.hidden_edges);
    let _ = varde.update(Message::Ui(Ui::Look(Look::ToggleViewMenu)));
    let open = shown(&varde, &mut renderer);
    let hidden_edges = open.iter().find(|t| t.text == "Hidden edges").unwrap();
    let mut ui = iced_runtime::user_interface::UserInterface::build(
        view(&varde),
        size,
        Default::default(),
        &mut renderer,
    );
    let sent = clicked(&mut ui, &mut renderer, hidden_edges.bounds.center());
    drop(ui);
    let [Ui::ToggleHiddenEdges] = sent[..] else {
        panic!("{sent:?}");
    };
    let _ = varde.update(Message::Ui(Ui::ToggleHiddenEdges));
    assert!(!varde.options.hidden_edges && !varde.options.mouse_hints);
    assert!(!document(&varde).view_menu);

    // A submenu's item hovered opens it to its left, its first choice
    // beside the item; a choice picked closes both.
    let hover = |varde: &mut Varde, renderer: &mut iced::Renderer, label: &str| {
        let open = shown(varde, renderer);
        let at = open
            .iter()
            .find(|t| t.text == label)
            .unwrap()
            .bounds
            .center();
        let mut ui = crate::tests::shown(view(varde), size, renderer);
        let mut sent = Vec::new();
        let _ = ui.update(
            &[iced::Event::Mouse(iced::mouse::Event::CursorMoved {
                position: at,
            })],
            iced::mouse::Cursor::Available(at),
            renderer,
            &mut iced::advanced::clipboard::Null,
            &mut sent,
        );
        drop(ui);
        for message in sent {
            let _ = varde.update(Message::Ui(message));
        }
    };
    let pick = |varde: &mut Varde, renderer: &mut iced::Renderer, submenu, label: &str| {
        let _ = varde.update(Message::Ui(Ui::Look(Look::ToggleViewMenu)));
        hover(varde, renderer, submenu);
        let open = shown(varde, renderer);
        let opener = open.iter().find(|t| t.text == submenu).unwrap().bounds;
        let item = open.iter().find(|t| t.text == label).unwrap().bounds;
        assert!(item.x + item.width < opener.x, "{item:?} {opener:?}");
        let mut ui = crate::tests::shown(view(varde), size, renderer);
        let sent = clicked(&mut ui, renderer, item.center());
        drop(ui);
        let [message] = &sent[..] else {
            panic!("{sent:?}");
        };
        let _ = varde.update(Message::Ui(message.clone()));
        assert!(!document(varde).view_menu);
    };
    use varde_render::Shading;
    use varde_view::Edges;
    assert_eq!(varde.options.edges, Edges::Default);
    pick(&mut varde, &mut renderer, "Edges", "Wireframe");
    assert_eq!(varde.options.edges, Edges::Wireframe);
    pick(&mut varde, &mut renderer, "Edges", "Tessellation");
    assert_eq!(varde.options.edges, Edges::Tessellation);
    assert_eq!(varde.options.shading, Shading::Regular);
    pick(&mut varde, &mut renderer, "Shading", "Metal");
    assert_eq!(varde.options.shading, Shading::Metal);
    pick(&mut varde, &mut renderer, "Shading", "Flat metal");
    assert_eq!(varde.options.shading, Shading::FlatMetal);
    assert!(!varde.options.hidden_edges);

    // The first choices line up with their items; another item hovered
    // closes the submenu.
    let _ = varde.update(Message::Ui(Ui::Look(Look::ToggleViewMenu)));
    hover(&mut varde, &mut renderer, "Edges");
    let open = shown(&varde, &mut renderer);
    let y = |label: &str| {
        open.iter()
            .find(|t| t.text == label)
            .unwrap()
            .bounds
            .center_y()
    };
    assert!((y("Default") - y("Edges")).abs() < 1.0);
    assert!(!open.iter().any(|t| t.text == "Shaded"));
    hover(&mut varde, &mut renderer, "Perspective");
    assert!(
        !shown(&varde, &mut renderer)
            .iter()
            .any(|t| t.text == "Default")
    );
    hover(&mut varde, &mut renderer, "Shading");
    let open = shown(&varde, &mut renderer);
    let y = |label: &str| {
        open.iter()
            .find(|t| t.text == label)
            .unwrap()
            .bounds
            .center_y()
    };
    assert!((y("Shaded") - y("Shading")).abs() < 1.0);
    let _ = varde.update(Message::Ui(Ui::Look(Look::CloseViewMenu)));

    // `Esc` closes it, and a click off it.
    let _ = varde.update(Message::Ui(Ui::Look(Look::ToggleViewMenu)));
    let _ = varde.update(Message::Ui(Ui::Look(Look::Escape)));
    assert!(!document(&varde).view_menu);
    let _ = varde.update(Message::Ui(Ui::Look(Look::ToggleViewMenu)));
    let mut ui = iced_runtime::user_interface::UserInterface::build(
        view(&varde),
        size,
        Default::default(),
        &mut renderer,
    );
    let sent = clicked(&mut ui, &mut renderer, iced::Point::new(640.0, 300.0));
    assert!(
        matches!(sent[..], [Ui::Look(Look::CloseViewMenu)]),
        "{sent:?}"
    );
}

#[test]
fn a_long_status_leaves_the_key_hints_on_the_screen() {
    // A feature selected and the model failing with a long message, in a
    // small window: the status is cut where the hints start, which all
    // show whole.
    let (mut doc, requests) = example();
    let extrude = doc.editor.document().features()[1].id;
    doc.look(varde_view::Look::SelectFeature(extrude));
    doc.update(Edit::SetTolerance(
        varde_document::Tolerance::new(1e-2).unwrap(),
    ));
    for request in requests.take() {
        doc.computed(Response::Failed {
            draft: None,
            inspect: None,
            generation: request.generation().unwrap(),
            exclude: request.exclude(),
            error: "the kernel ran out of room splitting the faces of a body with very \
                    many curved faces; try a coarser tolerance"
                .to_owned(),
        });
    }
    let size = iced::Size::new(1024.0, 600.0);
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(doc.view_in(varde_view::Mode::Light), size, &mut renderer);
    let shown = texts(&mut ui, &renderer);
    let status_top = size.height - varde_view::STATUS_BAR_ROOM;
    let in_bar: Vec<_> = shown.iter().filter(|t| t.bounds.y >= status_top).collect();
    // The mouse's hints make room for the status.
    for hint in ["Drag to orbit", "Zoom", "Click to set pivot"] {
        assert!(!in_bar.iter().any(|t| t.text == hint), "{hint:?}");
    }
    for hint in ["Edit", "Delete"] {
        let text = in_bar
            .iter()
            .find(|t| t.text == hint)
            .unwrap_or_else(|| panic!("no {hint:?} in {in_bar:?}"));
        let seen = text.seen();
        assert!(
            seen.x >= 0.0 && seen.x + seen.width <= size.width,
            "{text:?}"
        );
        assert!(seen.y + seen.height <= size.height, "{text:?}");
    }
    // The status is on one line, within the bar.
    let status = in_bar
        .iter()
        .find(|t| t.text.contains("Couldn't regenerate"))
        .expect("the status");
    assert!(status.bounds.height < 20.0, "{status:?}");
}

mod damaged;
mod export;
mod panicked;
mod storage;
mod thumbnail;
