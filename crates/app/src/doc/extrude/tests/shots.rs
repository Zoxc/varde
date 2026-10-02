//! Screenshots of the document screen, to look at: the extrude, revolve
//! and combine sessions, their panels and the extrude's handle, the measure
//! tool, the Timeline, the delete prompt and the file menu, drawn offscreen by iced's headless wgpu renderer, which draws the
//! viewport's scene too. Every test is `#[ignore]`d and writes nothing
//! unless `VARDE_SHOTS` names the directory for the PNGs, see
//! `agents/viewport.md`. Pixels differ by GPU and driver, so nothing is
//! compared: the one check is that the viewport was drawn.

use std::path::PathBuf;
use std::sync::Mutex;

use iced::advanced::renderer::Headless as _;
use iced::advanced::widget::operation::scrollable::{RelativeOffset, snap_to};
use iced::time::Instant;
use iced::{Point, Size};
use varde_render::{Projection, View};

use super::*;
use crate::tests::{Headless, shown, texts};

/// The variable naming the directory the shots are written to.
const SHOTS: &str = "VARDE_SHOTS";

/// The app's window size, `lib.rs`'s default.
const WINDOW: Size = Size::new(1280.0, 800.0);

/// What the window is cleared to in a first frame: nothing of the screen
/// is this colour, so where it shows, nothing was drawn but what's
/// translucent.
const UNDRAWN: iced::Color = iced::Color::from_rgb(1.0, 0.0, 1.0);

/// One renderer at a time: the Vulkan loader isn't thread safe across
/// instances (see `varde-view`'s viewport tests), and each renderer makes
/// its own.
static GPU: Mutex<()> = Mutex::new(());

/// Runs `shots` with a headless wgpu renderer if `VARDE_SHOTS` is set and
/// there's an adapter, with the directory to write to.
fn shooting(shots: impl FnOnce(&mut Shooter)) {
    let Some(dir) = std::env::var_os(SHOTS).map(PathBuf::from) else {
        eprintln!("{SHOTS} not set, skipping");
        return;
    };
    let _gpu = GPU.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let renderer = iced::futures::executor::block_on(iced::Renderer::new(
        iced::Font::DEFAULT,
        iced::Pixels(13.0),
        Some("wgpu"),
    ));
    let Some(renderer) = renderer else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    shots(&mut Shooter { renderer, dir });
}

/// Takes shots, see [`Shooter::take`].
struct Shooter {
    renderer: iced::Renderer,
    dir: PathBuf,
}

/// Where the cursor is in a shot.
#[derive(Debug, Clone, Copy)]
enum Pointer {
    /// Off the window.
    Away,
    At(Point),
    /// Over the middle of the first text that reads this, held there long
    /// enough for tooltips to show.
    Over(&'static str),
    /// The same over the last text that reads this.
    OverLast(&'static str),
}

/// How a shot is taken.
#[derive(Debug, Clone, Copy)]
struct Shot {
    size: Size,
    scale: f32,
    mode: Mode,
    pointer: Pointer,
    /// Whether the extrude panel's body is scrolled to its end.
    scrolled: bool,
}

impl Shot {
    /// The app's default window, at scale 1, light, the cursor away.
    fn new() -> Self {
        Self {
            size: WINDOW,
            scale: 1.0,
            mode: Mode::Light,
            pointer: Pointer::Away,
            scrolled: false,
        }
    }

    fn size(self, width: f32, height: f32) -> Self {
        Self {
            size: Size::new(width, height),
            ..self
        }
    }

    fn scale(self, scale: f32) -> Self {
        Self { scale, ..self }
    }

    fn dark(self) -> Self {
        Self {
            mode: Mode::Dark,
            ..self
        }
    }

    fn pointer(self, pointer: Pointer) -> Self {
        Self { pointer, ..self }
    }

    fn scrolled(self) -> Self {
        Self {
            scrolled: true,
            ..self
        }
    }
}

impl Shooter {
    /// Draws `doc`'s screen as the app would with `shot`, and writes it to
    /// `<name>.png`. Fails if anything of the window is left undrawn: the
    /// viewport's scene among it.
    fn take(&mut self, doc: &Doc, name: &str, shot: Shot) {
        use iced::advanced::Renderer as _;
        use iced::mouse::{Cursor, Event};
        use iced::theme::Base;

        let renderer = &mut self.renderer;
        let mut ui: Headless<'_> = shown(doc.view_in(shot.mode), shot.size, renderer);
        if shot.scrolled {
            let end = RelativeOffset {
                x: None,
                y: Some(1.0),
            };
            ui.operate(renderer, &mut snap_to(varde_view::PANEL_BODY, end));
        }
        let at = match shot.pointer {
            Pointer::Away => None,
            Pointer::At(at) => Some(at),
            Pointer::Over(text) => Some(over(&mut ui, renderer, text, false)),
            Pointer::OverLast(text) => Some(over(&mut ui, renderer, text, true)),
        };
        let cursor = at.map_or(Cursor::Unavailable, Cursor::Available);
        if let Some(at) = at {
            // Moved there, and a redraw a while later, which tooltips wait
            // for.
            let later = Instant::now() + std::time::Duration::from_secs(2);
            for event in [
                iced::Event::Mouse(Event::CursorMoved { position: at }),
                iced::Event::Window(iced::window::Event::RedrawRequested(later)),
            ] {
                let mut sent = Vec::new();
                let _ = ui.update(
                    &[event],
                    cursor,
                    renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut sent,
                );
            }
        }
        let theme = varde_view::iced_theme(shot.mode);
        let style = iced::advanced::renderer::Style {
            text_color: theme.base().text_color,
        };
        let physical = Size::new(
            (shot.size.width * shot.scale).round() as u32,
            (shot.size.height * shot.scale).round() as u32,
        );
        // Twice: the first frame lays out what the second draws on (the
        // scene's cached layers, the knobs anchored to it), and is cleared
        // to a colour nothing on the screen has, to see what's drawn. The
        // second is cleared as the app clears the window.
        let mut undrawn = 0;
        let mut rgba = Vec::new();
        for clear in [UNDRAWN, theme.base().background_color] {
            renderer.reset(iced::Rectangle::with_size(shot.size));
            ui.draw(renderer, &theme, &style, cursor);
            rgba = renderer.screenshot(physical, shot.scale, clear);
            if clear == UNDRAWN {
                undrawn = rgba
                    .chunks(4)
                    .filter(|p| p[0] > p[1].saturating_add(60) && p[2] > p[1].saturating_add(60))
                    .count();
            }
        }
        let path = self.dir.join(format!("{name}.png"));
        write_png(&path, physical, &rgba);
        // The separators are translucent over the window's clear colour,
        // a pixel wide; an undrawn viewport would be most of the window.
        let pixels = rgba.len() / 4;
        assert!(
            undrawn < pixels / 50,
            "{name}: {undrawn} of {pixels} pixels undrawn"
        );
    }
}

/// The middle of the first text on `ui` reading `text`, or the `last`.
fn over(ui: &mut Headless<'_>, renderer: &iced::Renderer, text: &str, last: bool) -> Point {
    let found = texts(ui, renderer);
    let mut reading = found.iter().filter(|shown| shown.text == text);
    let shown = if last {
        reading.next_back()
    } else {
        reading.next()
    };
    let shown = shown.unwrap_or_else(|| panic!("no {text:?} in {found:?}"));
    shown.seen().center()
}

fn write_png(path: &std::path::Path, size: Size<u32>, rgba: &[u8]) {
    let file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut encoder = png::Encoder::new(file, size.width, size.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(rgba).unwrap();
}

/// Turns the camera Home, then orbits it by `yaw` and `pitch` and zooms by
/// `zoom`, through the messages the controls and the mouse send, the turn
/// finished.
fn aim(doc: &mut Doc, yaw: f32, pitch: f32, zoom: f32) {
    doc.look(Look::ResetCamera);
    doc.animation_frame(Instant::now() + 2 * crate::doc::CAMERA_ANIMATION);
    doc.look(Look::Orbit { yaw, pitch });
    doc.look(Look::Zoom {
        factor: zoom,
        x: 0.0,
        y: 0.0,
    });
}

/// How far out the camera is zoomed from Home's to frame the example's
/// 60 × 40 mm plate: Home shows about 7.5 mm of height (there's no zoom to
/// fit yet).
const PLATE_ZOOM: f32 = 12.0;

/// Home's direction, zoomed out to frame the example's plate.
fn framed(doc: &mut Doc) {
    aim(doc, 0.0, 0.0, PLATE_ZOOM);
}

/// Seen from below the plate, tilted.
fn from_below(doc: &mut Doc) {
    aim(doc, 0.0, -0.9, PLATE_ZOOM);
}

fn type_in(doc: &mut Doc, distance: Distance, text: &str) {
    let text = text.to_owned();
    extrude(doc, ExtrudeLook::Input { distance, text });
}

/// The plate, `X`, its region picked, answered.
fn plate_picked() -> (Doc, FeatureId, Requests) {
    let (mut doc, sketch, requests) = plate();
    key_in(&mut doc, key("x"));
    let region = plate_region(&doc, sketch);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region });
    answer(&mut doc, &requests);
    (doc, sketch, requests)
}

/// Scenario 1: `X` with nothing selected, every candidate's regions
/// filled; one hovered.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_01_candidates() {
    shooting(|camera| {
        let (mut doc, _, _) = plate();
        key_in(&mut doc, key("x"));
        camera.take(&doc, "01-candidates-home", Shot::new());
        framed(&mut doc);
        camera.take(&doc, "01-candidates", Shot::new());
        let centre = Point::new(WINDOW.width * 0.55, WINDOW.height * 0.5);
        let hovered = Shot::new().pointer(Pointer::At(centre));
        camera.take(&doc, "01-candidates-hovered", hovered);
        camera.take(&doc, "01-candidates-dark", hovered.dark());
    });
}

/// Scenario 2, with #34's 3.2: a region picked before the answer
/// ("Regenerating…"), then answered: the new body's preview, the picked
/// fill and outline, the shaft and the knob.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_02_picked() {
    shooting(|camera| {
        let (mut doc, sketch, requests) = plate();
        framed(&mut doc);
        key_in(&mut doc, key("x"));
        let region = plate_region(&doc, sketch);
        extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region });
        camera.take(&doc, "02-picked-waiting", Shot::new());
        answer(&mut doc, &requests);
        camera.take(&doc, "02-picked", Shot::new());
        camera.take(&doc, "02-picked-scale2", Shot::new().scale(2.0));
        camera.take(&doc, "02-picked-dark", Shot::new().dark());
        camera.take(&doc, "02-picked-dark-scale2", Shot::new().dark().scale(2.0));
        type_in(&mut doc, Distance::First, "30");
        answer(&mut doc, &requests);
        camera.take(&doc, "02-picked-30", Shot::new());
    });
}

/// Scenario 3: flipped; symmetric; two sides with both knobs; a refused
/// distance; a draft the document refuses.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_03_extents() {
    shooting(|camera| {
        let (mut doc, _, requests) = plate_picked();
        framed(&mut doc);
        type_in(&mut doc, Distance::First, "20");
        extrude(&mut doc, ExtrudeLook::Flip);
        answer(&mut doc, &requests);
        camera.take(&doc, "03-flipped", Shot::new());
        extrude(&mut doc, ExtrudeLook::Extent(ExtentKind::Symmetric));
        answer(&mut doc, &requests);
        camera.take(&doc, "03-symmetric", Shot::new());
        extrude(&mut doc, ExtrudeLook::Extent(ExtentKind::TwoSides));
        type_in(&mut doc, Distance::First, "20");
        type_in(&mut doc, Distance::Second, "8");
        answer(&mut doc, &requests);
        camera.take(&doc, "03-two-sides", Shot::new());
        camera.take(&doc, "03-two-sides-dark", Shot::new().dark());
        type_in(&mut doc, Distance::Second, "12 parsecs");
        answer(&mut doc, &requests);
        camera.take(&doc, "03-refused-distance", Shot::new());
        type_in(&mut doc, Distance::First, "600000");
        type_in(&mut doc, Distance::Second, "600000");
        answer(&mut doc, &requests);
        camera.take(&doc, "03-refused-draft", Shot::new());
    });
}

/// The example and a hole sketched on its plate, `X`, the hole picked.
fn hole_picked() -> (Doc, FeatureId, Requests) {
    let (mut doc, sketch, requests) = example_and_a_hole();
    framed(&mut doc);
    doc.look(Look::StartExtrude);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region: 0 });
    (doc, sketch, requests)
}

/// Scenario 4, with #34's 3.1 and 3.2: a cut, through all, before and
/// after the answer; the body taken out, with the draft's error.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_04_cut() {
    shooting(|camera| {
        let (mut doc, _, requests) = hole_picked();
        let body = doc.editor.document().bodies()[0].id;
        extrude(&mut doc, ExtrudeLook::Operation(OperationKind::Cut));
        camera.take(&doc, "04-cut-waiting", Shot::new());
        answer(&mut doc, &requests);
        camera.take(&doc, "04-cut", Shot::new());
        extrude(&mut doc, ExtrudeLook::Extent(ExtentKind::ThroughAll));
        answer(&mut doc, &requests);
        camera.take(&doc, "04-cut-through-all", Shot::new());
        camera.take(&doc, "04-cut-through-all-scale2", Shot::new().scale(2.0));
        camera.take(&doc, "04-cut-through-all-dark", Shot::new().dark());
        let dark2 = Shot::new().dark().scale(2.0);
        camera.take(&doc, "04-cut-through-all-dark-scale2", dark2);
        // #34 3.3: the cursor over the body's row.
        let row = Shot::new().pointer(Pointer::OverLast("Body 1"));
        camera.take(&doc, "04-cut-row-hovered", row);
        extrude(&mut doc, ExtrudeLook::Target(body));
        camera.take(&doc, "04-cut-untick-waiting", Shot::new());
        answer(&mut doc, &requests);
        camera.take(&doc, "04-cut-untick", Shot::new());
        camera.take(&doc, "04-cut-untick-dark", Shot::new().dark());
        camera.take(&doc, "04-cut-untick-scale2", Shot::new().scale(2.0));
    });
}

/// Scenario 5, with #34's 3.1 and 3.3: join and intersect, from below
/// the plate, so the overlays are seen through the body; "Through all"
/// disabled, its tip shown.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_05_join_intersect() {
    shooting(|camera| {
        let (mut doc, _, requests) = hole_picked();
        let body = doc.editor.document().bodies()[0].id;
        extrude(&mut doc, ExtrudeLook::Operation(OperationKind::Join));
        camera.take(&doc, "05-join-waiting", Shot::new());
        answer(&mut doc, &requests);
        camera.take(&doc, "05-join", Shot::new());
        camera.take(&doc, "05-join-dark-scale2", Shot::new().dark().scale(2.0));
        let tip = Shot::new().pointer(Pointer::Over("Through all"));
        camera.take(&doc, "05-join-through-all-tip", tip);
        from_below(&mut doc);
        camera.take(&doc, "05-join-below", Shot::new());
        extrude(&mut doc, ExtrudeLook::Operation(OperationKind::Intersect));
        answer(&mut doc, &requests);
        camera.take(&doc, "05-intersect-below", Shot::new());
        framed(&mut doc);
        camera.take(&doc, "05-intersect", Shot::new());
        camera.take(&doc, "05-intersect-dark", Shot::new().dark());
        extrude(&mut doc, ExtrudeLook::Target(body));
        answer(&mut doc, &requests);
        camera.take(&doc, "05-intersect-untick", Shot::new());
        // A cut seen from below: through the body.
        extrude(&mut doc, ExtrudeLook::Target(body));
        extrude(&mut doc, ExtrudeLook::Operation(OperationKind::Cut));
        answer(&mut doc, &requests);
        from_below(&mut doc);
        camera.take(&doc, "05-cut-below", Shot::new());
    });
}

/// The example, a hole, `more` bodies made like the plate's, `X` on the
/// hole, a cut of every body, answered: every shot of a session is taken
/// with its answers in, unless it's of the wait for one.
fn a_cut_of_many(more: usize) -> (Doc, Requests) {
    let (mut doc, sketch, requests) = example_and_a_hole();
    framed(&mut doc);
    let FeatureKind::Extrude(plate) = doc.editor.document().features()[1].kind.clone() else {
        panic!("the example's second feature is its extrude");
    };
    for _ in 0..more {
        let mut extrude = plate.clone();
        extrude.operation = Operation::NewBody(varde_document::BodyId::NEW);
        doc.apply(doc.editor.document().add_feature(extrude.into()));
    }
    doc.sync();
    answer(&mut doc, &requests);
    doc.look(Look::SelectFeature(sketch));
    doc.look(Look::StartExtrude);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region: 0 });
    extrude(&mut doc, ExtrudeLook::Operation(OperationKind::Cut));
    answer(&mut doc, &requests);
    assert_eq!(doc.extrude_state().unwrap().targets.len(), more + 1);
    (doc, requests)
}

/// Takes every body in or out of the extrude being set up.
fn toggle_every_body(doc: &mut Doc) {
    let bodies: Vec<_> = (doc.editor.document().bodies().iter())
        .map(|body| body.id)
        .collect();
    for body in bodies {
        extrude(doc, ExtrudeLook::Target(body));
    }
}

/// Scenario 6, with #34's 3.4: 20 and 30 bodies a cut touches: the
/// panel's height, OK on screen, the body's scrollbar.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_06_many_bodies() {
    shooting(|camera| {
        let (mut doc, requests) = a_cut_of_many(19);
        camera.take(&doc, "06-20-bodies", Shot::new());
        camera.take(&doc, "06-20-bodies-scrolled", Shot::new().scrolled());
        toggle_every_body(&mut doc);
        answer(&mut doc, &requests);
        camera.take(&doc, "06-20-bodies-untick", Shot::new());
        // All ticked again, before the answer: what the list shows while
        // the touch test of the new targets is on its way.
        toggle_every_body(&mut doc);
        camera.take(&doc, "06-20-bodies-retick-waiting", Shot::new());
        let (doc, _) = a_cut_of_many(29);
        camera.take(&doc, "06-30-bodies", Shot::new());
        camera.take(&doc, "06-30-bodies-scrolled", Shot::new().scrolled());
        camera.take(
            &doc,
            "06-30-bodies-dark-scale2",
            Shot::new().dark().scale(2.0),
        );
        let small = Shot::new().size(1024.0, 600.0);
        camera.take(&doc, "06-30-bodies-1024x600", small);
        camera.take(&doc, "06-30-bodies-1024x600-scrolled", small.scrolled());
    });
}

/// Scenario 7: the panel in a small window, two sides with both fields
/// refused and six bodies.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_07_small_window() {
    shooting(|camera| {
        let (mut doc, _, _) = plate_picked();
        framed(&mut doc);
        let small = Shot::new().size(1024.0, 600.0);
        camera.take(&doc, "07-small", small);
        let (mut doc, requests) = a_cut_of_many(5);
        extrude(&mut doc, ExtrudeLook::Extent(ExtentKind::TwoSides));
        answer(&mut doc, &requests);
        type_in(&mut doc, Distance::First, "12 parsecs");
        type_in(&mut doc, Distance::Second, "12 parsecs");
        camera.take(&doc, "07-small-errors", small);
        camera.take(&doc, "07-small-errors-scrolled", small.scrolled());
        camera.take(&doc, "07-small-errors-dark", small.dark());
    });
}

/// Scenario 8: an extrude edited whose region its sketch no longer has:
/// the note.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_08_missing_region() {
    shooting(|camera| {
        let (mut doc, requests) = example();
        framed(&mut doc);
        let features = doc.editor.document().features();
        let (sketch, feature) = (features[0].id, features[1].id);
        let mut drawn = varde_sketch::Sketch::default();
        let center = drawn.add_point(glam::DVec2::new(0.0, 0.0)).unwrap();
        drawn
            .add_curve(
                varde_sketch::Curve::Circle {
                    center,
                    radius: 15.0,
                },
                false,
            )
            .unwrap();
        doc.apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn),
        });
        doc.sync();
        answer(&mut doc, &requests);
        doc.look(Look::EditFeature(feature));
        assert!(doc.extrude.as_ref().unwrap().regions.missing > 0);
        answer(&mut doc, &requests);
        camera.take(&doc, "08-missing-region", Shot::new());
        camera.take(&doc, "08-missing-region-dark", Shot::new().dark());
    });
}

/// Adds an extrude of `sketch`'s region `region` through the panel,
/// `set` choosing what it is, and commits it.
fn extruded(
    doc: &mut Doc,
    requests: &Requests,
    sketch: FeatureId,
    region: usize,
    set: impl FnOnce(&mut Doc),
) {
    doc.look(Look::SelectFeature(sketch));
    doc.look(Look::StartExtrude);
    extrude(doc, ExtrudeLook::PickRegion { sketch, region });
    set(doc);
    doc.update(Edit::CommitExtrude);
    assert_eq!(doc.edit_error, None);
    assert!(doc.extrude.is_none());
    answer(doc, requests);
}

/// Scenario 9: the Timeline with an extrude row of each extent and
/// operation, and a failed row with its tip shown.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_09_timeline() {
    shooting(|camera| {
        let (mut doc, hole, requests) = example_and_a_hole();
        framed(&mut doc);
        let plate = doc.editor.document().features()[0].id;
        let region = plate_region(&doc, plate);
        extruded(&mut doc, &requests, plate, region, |doc| {
            extrude(doc, ExtrudeLook::Extent(ExtentKind::Symmetric));
            type_in(doc, Distance::First, "4");
        });
        extruded(&mut doc, &requests, plate, region, |doc| {
            extrude(doc, ExtrudeLook::Extent(ExtentKind::TwoSides));
            extrude(doc, ExtrudeLook::Operation(OperationKind::Join));
            type_in(doc, Distance::First, "12");
            type_in(doc, Distance::Second, "3");
        });
        extruded(&mut doc, &requests, hole, 0, |doc| {
            extrude(doc, ExtrudeLook::Operation(OperationKind::Cut));
            extrude(doc, ExtrudeLook::Extent(ExtentKind::ThroughAll));
        });
        // Of the plate's region, so something is left of the bodies to
        // see: the hole's would empty them, the hole being cut through.
        extruded(&mut doc, &requests, plate, region, |doc| {
            extrude(doc, ExtrudeLook::Operation(OperationKind::Intersect));
        });
        // A cut of nothing: every body taken out.
        extruded(&mut doc, &requests, hole, 0, |doc| {
            extrude(doc, ExtrudeLook::Operation(OperationKind::Cut));
            toggle_every_body(doc);
        });
        assert!(!doc.feed.failed_features().is_empty());
        doc.look(Look::SelectPanel(varde_view::Panel::Timeline));
        camera.take(&doc, "09-timeline", Shot::new());
        camera.take(&doc, "09-timeline-dark", Shot::new().dark());
        let failed = doc.feed.failed_features()[0].0;
        let name = doc.editor.document().feature(failed).unwrap().name.clone();
        let name: &'static str = Box::leak(name.into_boxed_str());
        let tip = Shot::new().pointer(Pointer::Over(name));
        camera.take(&doc, "09-timeline-failed-tip", tip);
        camera.take(&doc, "09-timeline-failed-tip-scale2", tip.scale(2.0));
    });
}

/// Scenario 10: the delete prompt for a sketch twelve extrudes use, and
/// for the example's sketch.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_10_delete_prompts() {
    shooting(|camera| {
        let (mut doc, requests) = a_cut_of_many(11);
        doc.look(Look::Escape);
        assert!(doc.extrude.is_none());
        answer(&mut doc, &requests);
        let plate = doc.editor.document().features()[0].id;
        doc.update(Edit::RemoveFeature(plate));
        assert!(doc.delete_prompt().is_some());
        camera.take(&doc, "10-delete-sketch", Shot::new());
        camera.take(&doc, "10-delete-sketch-dark", Shot::new().dark());
        camera.take(
            &doc,
            "10-delete-sketch-small",
            Shot::new().size(1024.0, 600.0),
        );

        // A body goes with the feature making it, which no later feature
        // uses (extrudes use sketches only), so deleting one asks only to
        // warn of a cut left with nothing to work on: scenario 14. The
        // short prompt instead: the example's sketch, its extrude and body.
        let (mut doc, _) = example();
        framed(&mut doc);
        let body = doc.editor.document().bodies()[0].id;
        let removal = doc
            .editor
            .document()
            .removal(varde_document::Removable::Body(body));
        assert_eq!(removal.features.len(), 1);
        let sketch = doc.editor.document().features()[0].id;
        doc.update(Edit::RemoveFeature(sketch));
        assert!(doc.delete_prompt().is_some());
        camera.take(&doc, "10-delete-short", Shot::new());
        camera.take(
            &doc,
            "10-delete-short-dark-scale2",
            Shot::new().dark().scale(2.0),
        );
    });
}

/// Scenario 14: the banner over the viewport saying a sketch edit was
/// refused after its sketch was left, a short reason and a long one in a
/// small window; and the body prompt warning of a cut left with nothing
/// to work on.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_14_refused_edit_and_delete_warning() {
    use crate::doc::sketch::Refusal;

    shooting(|camera| {
        let (mut doc, hole, requests) = example_and_a_hole();
        framed(&mut doc);
        extruded(&mut doc, &requests, hole, 0, |doc| {
            extrude(doc, ExtrudeLook::Operation(OperationKind::Cut));
            extrude(doc, ExtrudeLook::Extent(ExtentKind::ThroughAll));
        });
        let plate = doc.editor.document().features()[0].id;
        let redundant = varde_sketch::Rejected::Redundant {
            involved: Default::default(),
        };
        doc.refused_edit = Some((plate, Refusal::Rejected(redundant)));
        let texts = crate::tests::screen_texts(&doc);
        assert!(
            texts
                .iter()
                .any(|text| text == "An edit of Sketch 1 wasn't kept"),
            "{texts:?}"
        );
        camera.take(&doc, "14-refused-edit", Shot::new());
        camera.take(&doc, "14-refused-edit-dark", Shot::new().dark());
        let failed = "the solver's worker stopped while it was checking the edit, and \
                      was started again";
        doc.refused_edit = Some((plate, Refusal::Failed(failed.to_owned())));
        camera.take(
            &doc,
            "14-refused-edit-long-small",
            Shot::new().size(1024.0, 600.0),
        );

        doc.update(Edit::DismissRefusedEdit);
        let body = doc.editor.document().bodies()[0].id;
        doc.update(Edit::RemoveBody(body));
        let prompt = doc.delete_prompt().expect("the prompt shows");
        assert_eq!(prompt.worked.len(), 1);
        camera.take(&doc, "14-delete-body-warning", Shot::new());
        camera.take(
            &doc,
            "14-delete-body-warning-dark-scale2",
            Shot::new().dark().scale(2.0),
        );
    });
}

/// Scenario 11: the file menu, its tolerances, the one set ticked; and a
/// value the menu doesn't offer.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_11_file_menu() {
    shooting(|camera| {
        let (mut doc, _) = example();
        framed(&mut doc);
        doc.update(Edit::ToggleFileMenu);
        let hovered = Shot::new().pointer(Pointer::Over("10 µm"));
        camera.take(&doc, "11-file-menu", hovered);
        camera.take(&doc, "11-file-menu-dark-scale2", hovered.dark().scale(2.0));
        doc.update(Edit::SetTolerance(Tolerance::new(5e-5).unwrap()));
        if !doc.file_menu {
            doc.update(Edit::ToggleFileMenu);
        }
        camera.take(&doc, "11-file-menu-odd-tolerance", Shot::new());
    });
}

/// Looks from `view` in `projection`, the turn finished, zoomed by `zoom`.
fn look_from(doc: &mut Doc, view: View, projection: Projection, zoom: f32) {
    doc.look(Look::SetProjection(projection));
    for look in [Look::ResetCamera, Look::LookFrom(view)] {
        doc.look(look);
        doc.animation_frame(Instant::now() + 2 * crate::doc::CAMERA_ANIMATION);
    }
    doc.look(Look::Zoom {
        factor: zoom,
        x: 0.0,
        y: 0.0,
    });
}

/// Scenario 12, the bug hunt's odd cameras: looking along the handle's
/// axis (no shaft), perspective close up with the handle behind the eye,
/// grazing the plate, the knob panned off the screen, a 100 m extrude at
/// the plate's zoom, a knob dragged while the draft fails, and one `Doc`
/// drawn at several window sizes in turn.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_12_odd_cameras() {
    shooting(|camera| {
        let (mut doc, _, requests) = plate_picked();
        look_from(&mut doc, View::Top, Projection::Orthographic, PLATE_ZOOM);
        camera.take(&doc, "12-along-the-axis", Shot::new());
        look_from(&mut doc, View::Top, Projection::Perspective, PLATE_ZOOM);
        camera.take(&doc, "12-along-the-axis-perspective", Shot::new());
        // Close up from the top in perspective: the knob 10 mm up, the
        // eye a little above it.
        look_from(&mut doc, View::Top, Projection::Perspective, 0.3);
        camera.take(&doc, "12-close-perspective", Shot::new());
        // Grazing the plate's top from the front.
        look_from(&mut doc, View::Front, Projection::Perspective, PLATE_ZOOM);
        doc.look(Look::Orbit {
            yaw: 0.3,
            pitch: 0.05,
        });
        camera.take(&doc, "12-grazing-perspective", Shot::new());
        doc.look(Look::SetProjection(Projection::Orthographic));
        camera.take(&doc, "12-grazing", Shot::new());
        // Panned until the knob is off the screen.
        framed(&mut doc);
        doc.look(Look::Pan { dx: 0.9, dy: 0.0 });
        camera.take(&doc, "12-knob-off-screen", Shot::new());
        // 100 m up, at the plate's zoom, and zoomed out to see it.
        framed(&mut doc);
        type_in(&mut doc, Distance::First, "100000");
        answer(&mut doc, &requests);
        camera.take(&doc, "12-huge", Shot::new());
        aim(&mut doc, 0.0, 0.0, PLATE_ZOOM * 2000.0);
        camera.take(&doc, "12-huge-zoomed-out", Shot::new());
        // Two sides, the knob dragged past what the document takes.
        framed(&mut doc);
        extrude(&mut doc, ExtrudeLook::Extent(ExtentKind::TwoSides));
        type_in(&mut doc, Distance::First, "20");
        type_in(&mut doc, Distance::Second, "8");
        answer(&mut doc, &requests);
        extrude(&mut doc, ExtrudeLook::GrabHandle(Distance::First));
        let to = 1_500_000.0;
        let first = Distance::First;
        extrude(
            &mut doc,
            ExtrudeLook::DragHandle {
                distance: first,
                to,
            },
        );
        answer(&mut doc, &requests);
        camera.take(&doc, "12-dragged-too-far", Shot::new());
        extrude(&mut doc, ExtrudeLook::DropHandle);
        // One `Doc`, resized between shots.
        type_in(&mut doc, Distance::First, "20");
        answer(&mut doc, &requests);
        camera.take(&doc, "12-resized-1280", Shot::new());
        camera.take(&doc, "12-resized-800", Shot::new().size(800.0, 500.0));
        camera.take(&doc, "12-resized-1600", Shot::new().size(1600.0, 1000.0));
    });
}

/// Scenario 13: the status bar with a feature selected and the model
/// failing with a long message, in the default and a small window.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_13_status_bar() {
    shooting(|camera| {
        let (mut doc, requests) = example();
        framed(&mut doc);
        let extrude = doc.editor.document().features()[1].id;
        doc.look(Look::SelectFeature(extrude));
        doc.update(Edit::SetTolerance(Tolerance::new(1e-2).unwrap()));
        for request in requests.take() {
            doc.computed(varde_regen::Response::Failed {
                draft: None,
                inspect: None,
                generation: request.generation().unwrap(),
                exclude: request.exclude(),
                error: "the kernel ran out of room splitting the faces of a body with \
                        very many curved faces; try a coarser tolerance"
                    .to_owned(),
            });
        }
        camera.take(&doc, "13-status-failed", Shot::new());
        camera.take(
            &doc,
            "13-status-failed-small",
            Shot::new().size(1024.0, 600.0),
        );
    });
}

/// Scenario 15: two plates merged by a join bridging them: the Objects
/// list with the merged body in its holder (hovered: no eye, the bin
/// stays), and the join edited, its panel saying which body it joins
/// into.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_15_merged_bodies() {
    shooting(|camera| {
        let (editor, _, join) = crate::tests::merged_plates();
        let (mut doc, requests) = crate::tests::holding(editor.document().clone());
        framed(&mut doc);
        doc.look(Look::SelectPanel(varde_view::Panel::Objects));
        camera.take(&doc, "15-merged-objects", Shot::new());
        let hovered = Shot::new().pointer(Pointer::Over("in Body 1"));
        camera.take(&doc, "15-merged-objects-hovered", hovered);
        camera.take(&doc, "15-merged-objects-dark", hovered.dark());
        doc.look(Look::EditFeature(join));
        answer(&mut doc, &requests);
        camera.take(&doc, "15-merged-join", Shot::new());
        camera.take(&doc, "15-merged-join-scale2", Shot::new().scale(2.0));
    });
}

/// Scenario 16: the point the camera orbits, picked on the example's
/// plate, panned to the middle and orbited about, its marker whole, half
/// faded, and on the origin, where it isn't drawn.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_16_pivot() {
    shooting(|camera| {
        let (mut doc, _requests) = example();
        framed(&mut doc);
        let corner = doc.feed.mesh().bounds().unwrap().max;
        let start = Instant::now();
        doc.set_pivot(Some(corner), start);
        // Panned to the middle, the marker still whole.
        doc.animation_frame(start + crate::doc::CAMERA_ANIMATION);
        doc.look(Look::Orbit {
            yaw: 0.6,
            pitch: 0.3,
        });
        camera.take(&doc, "16-pivot", Shot::new());
        camera.take(&doc, "16-pivot-dark-scale2", Shot::new().dark().scale(2.0));
        doc.animation_frame(start + crate::doc::PIVOT_SHOWN + crate::doc::PIVOT_FADE / 2);
        camera.take(&doc, "16-pivot-fading", Shot::new());
        doc.set_pivot(Some(glam::Vec3::ZERO), start);
        doc.animation_frame(start + crate::doc::CAMERA_ANIMATION);
        camera.take(&doc, "16-pivot-on-origin", Shot::new());
    });
}

/// Scenario 17: the tool rail, outside a sketch and in one, its cards
/// showing as many tools as fit in a tall and a short window, with a
/// set's list open, a tool's tooltip, and a list taller than a short
/// window.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_17_rail() {
    shooting(|camera| {
        let (mut doc, _requests) = example();
        framed(&mut doc);
        camera.take(&doc, "17-rail-model", Shot::new());
        doc.look(Look::Rail(varde_view::RailLook::Open(0)));
        camera.take(&doc, "17-rail-model-create", Shot::new());
        camera.take(&doc, "17-rail-model-create-dark", Shot::new().dark());

        let sketch = doc.editor.document().features()[0].id;
        doc.look(Look::EditFeature(sketch));
        doc.animation_frame(Instant::now() + crate::doc::CAMERA_ANIMATION);
        doc.look(Look::SelectTool(varde_view::Tool::Line));
        camera.take(&doc, "17-rail-sketch", Shot::new());
        // As many tools as fit, shared evenly between the sets.
        camera.take(
            &doc,
            "17-rail-sketch-tall",
            Shot::new().size(1280.0, 1100.0),
        );
        camera.take(
            &doc,
            "17-rail-sketch-short",
            Shot::new().size(1024.0, 420.0),
        );
        // Over the first tool of the first card, Line.
        let line = Point::new(varde_view::SIDE_PANEL_WIDTH + 6.0 + 24.0, 40.0 + 6.0 + 50.0);
        camera.take(
            &doc,
            "17-rail-sketch-tip",
            Shot::new().pointer(Pointer::At(line)),
        );
        doc.look(Look::Rail(varde_view::RailLook::Open(2)));
        camera.take(&doc, "17-rail-sketch-constraints", Shot::new());
        // The keys moved down two rows.
        doc.look(Look::Rail(varde_view::RailLook::Down));
        doc.look(Look::Rail(varde_view::RailLook::Down));
        camera.take(&doc, "17-rail-sketch-constraints-row", Shot::new());
        camera.take(
            &doc,
            "17-rail-sketch-constraints-short",
            Shot::new().size(1024.0, 420.0),
        );
        camera.take(
            &doc,
            "17-rail-sketch-constraints-scale2-dark",
            Shot::new().scale(2.0).dark(),
        );
    });
}

impl Shooter {
    /// Moves the cursor to `at` over `doc`'s screen in the app's window,
    /// and takes what the viewport says it hovers: the target hovered, if
    /// anything.
    fn hover(&mut self, doc: &mut Doc, at: Point) -> Option<varde_view::Picked> {
        use iced::mouse::{Cursor, Event};
        let mut ui: Headless<'_> = shown(doc.view_in(Mode::Light), WINDOW, &mut self.renderer);
        let mut sent = Vec::new();
        let _ = ui.update(
            &[iced::Event::Mouse(Event::CursorMoved { position: at })],
            Cursor::Available(at),
            &mut self.renderer,
            &mut iced::advanced::clipboard::Null,
            &mut sent,
        );
        drop(ui);
        for message in sent {
            if let varde_view::Message::Look(look @ Look::Hover(_)) = message {
                doc.look(look);
            }
        }
        doc.pick.hover().map(|pick| pick.target)
    }
}

/// Scenario 18: picking the model, a face hovered and an edge hovered,
/// light and dark.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_18_hover() {
    shooting(|camera| {
        let (mut doc, _requests) = example();
        framed(&mut doc);
        let face = Point::new(WINDOW.width * 0.66, WINDOW.height * 0.5);
        let hovered = camera.hover(&mut doc, face);
        assert!(
            matches!(hovered, Some(varde_view::Picked::Face(_))),
            "{hovered:?}"
        );
        let shot = Shot::new().pointer(Pointer::At(face));
        camera.take(&doc, "18-hover-face", shot);
        camera.take(&doc, "18-hover-face-dark", shot.dark());
        camera.take(&doc, "18-hover-face-scale2", shot.scale(2.0));
        // Down from there to the first edge.
        let edge = (0..300)
            .map(|dy| Point::new(face.x, face.y + dy as f32))
            .find(|&at| {
                matches!(
                    camera.hover(&mut doc, at),
                    Some(varde_view::Picked::Edge(_))
                )
            })
            .expect("an edge below");
        let shot = Shot::new().pointer(Pointer::At(edge));
        camera.take(&doc, "18-hover-edge", shot);
        camera.take(&doc, "18-hover-edge-dark", shot.dark());
        // From below, the bottom face.
        from_below(&mut doc);
        let below = Point::new(WINDOW.width * 0.6, WINDOW.height * 0.45);
        camera.hover(&mut doc, below);
        camera.take(
            &doc,
            "18-hover-below",
            Shot::new().pointer(Pointer::At(below)),
        );
    });
}

/// Scenario 19: selecting in the model: the top selected and an edge
/// hovered, the status bar's box telling of the face; the top and an edge
/// with Shift; the body double-clicked, marked in Objects too.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_19_select() {
    shooting(|camera| {
        let (mut doc, _requests) = example();
        framed(&mut doc);
        let face = Point::new(WINDOW.width * 0.66, WINDOW.height * 0.5);
        camera.hover(&mut doc, face);
        let pick = doc.pick.hover().expect("the top under the cursor");
        doc.look(Look::ClickModel {
            pick: Some(pick),
            add: false,
            double: false,
        });
        // Down from there to the first edge.
        let edge = (0..300)
            .map(|dy| Point::new(face.x, face.y + dy as f32))
            .find(|&at| {
                matches!(
                    camera.hover(&mut doc, at),
                    Some(varde_view::Picked::Edge(_))
                )
            })
            .expect("an edge below");
        let shot = Shot::new().pointer(Pointer::At(edge));
        camera.take(&doc, "19-select-face", shot);
        camera.take(&doc, "19-select-face-dark", shot.dark());
        let pick = doc.pick.hover().expect("the edge under the cursor");
        doc.look(Look::ClickModel {
            pick: Some(pick),
            add: true,
            double: false,
        });
        camera.take(&doc, "19-select-two", shot);
        // The body, double-clicked, in Objects too.
        doc.look(Look::SelectPanel(varde_view::Panel::Objects));
        doc.look(Look::ClickModel {
            pick: Some(pick),
            add: false,
            double: true,
        });
        let away = Shot::new();
        camera.take(&doc, "19-select-body", away);
        camera.take(&doc, "19-select-body-dark", away.dark());
    });
}

/// Scenario 20: the revolve: `O` with the lathe's regions offered, the
/// rectangle picked and the axis asked for, a full turn about the
/// construction line previewed, one side of 270° flipped, two sides,
/// then the revolve in the Timeline with the rail's Create list open.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_20_revolve() {
    use varde_document::AxisLine;
    use varde_view::{Angle, RevolveLook, TurnKind};

    shooting(|camera| {
        let mut lathe = crate::doc::revolve::tests::lathe();
        aim(&mut lathe.doc, -0.3, -0.2, 15.0);
        key_in(&mut lathe.doc, key("o"));
        camera.take(&lathe.doc, "20-revolve-regions", Shot::new());
        let (sketch, region) = (lathe.sketch, lathe.rectangle());
        lathe.revolve(RevolveLook::PickRegion { sketch, region });
        lathe.answer();
        camera.take(&lathe.doc, "20-revolve-axis", Shot::new());
        let axis = AxisLine::Curve(lathe.construction);
        lathe.revolve(RevolveLook::PickAxis { sketch, axis });
        lathe.answer();
        camera.take(&lathe.doc, "20-revolve-full", Shot::new());
        camera.take(&lathe.doc, "20-revolve-full-dark", Shot::new().dark());
        lathe.revolve(RevolveLook::Extent(TurnKind::OneSide));
        lathe.input(Angle::First, "270");
        lathe.revolve(RevolveLook::Flip);
        lathe.answer();
        camera.take(&lathe.doc, "20-revolve-one-side", Shot::new());
        lathe.revolve(RevolveLook::Extent(TurnKind::TwoSides));
        lathe.input(Angle::First, "100");
        lathe.input(Angle::Second, "45");
        lathe.answer();
        camera.take(&lathe.doc, "20-revolve-two-sides", Shot::new());
        camera.take(
            &lathe.doc,
            "20-revolve-two-sides-scale2-dark",
            Shot::new().scale(2.0).dark(),
        );
        lathe.doc.update(Edit::CommitRevolve);
        lathe.answer();
        assert!(lathe.doc.revolve.is_none());
        lathe
            .doc
            .look(Look::SelectPanel(varde_view::Panel::Timeline));
        let feature = lathe.doc.editor.document().features()[1].id;
        lathe.doc.look(Look::SelectFeature(feature));
        camera.take(&lathe.doc, "20-revolve-timeline", Shot::new());
        lathe.doc.look(Look::Rail(varde_view::RailLook::Open(0)));
        camera.take(&lathe.doc, "20-revolve-rail", Shot::new());
        camera.take(&lathe.doc, "20-revolve-rail-dark", Shot::new().dark());
    });
}

/// A ball and a torus turned about one line, close up: their patches are
/// curved both ways, so the tessellation refines inside them.
#[test]
#[ignore = "writes screenshots where VARDE_SHOTS says"]
fn shots_21_round_solids() {
    use glam::DVec2;
    use varde_document::{AxisLine, OriginPlane, Plane, Sketch};
    use varde_sketch::Curve;
    use varde_view::RevolveLook;

    shooting(|camera| {
        let mut editor = Editor::new(Document::default());
        let add = editor.document().add_sketch(Plane::Origin(OriginPlane::XZ));
        editor.apply(add).unwrap();
        let sketch = editor.document().features()[0].id;
        let mut drawn = Sketch::default();
        let mut point = |x, y| drawn.add_point(DVec2::new(x, y)).unwrap();
        let [low, high, top, centre, ring] = [
            (0.0, -10.0),
            (0.0, 10.0),
            (0.0, 30.0),
            (0.0, 0.0),
            (25.0, 18.0),
        ]
        .map(|(x, y)| point(x, y));
        let axis = (drawn.add_curve(
            Curve::Line {
                start: low,
                end: top,
            },
            true,
        ))
        .unwrap();
        // A half disc against the axis, and a circle away from it.
        let arc = Curve::Arc {
            center: centre,
            start: low,
            end: high,
        };
        drawn.add_curve(arc, false).unwrap();
        let side = Curve::Line {
            start: high,
            end: low,
        };
        drawn.add_curve(side, false).unwrap();
        let circle = Curve::Circle {
            center: ring,
            radius: 6.0,
        };
        drawn.add_curve(circle, false).unwrap();
        let profiles = drawn.profiles().unwrap();
        let regions = [DVec2::new(5.0, 0.0), DVec2::new(25.0, 18.0)]
            .map(|at| profiles.region_at(at).unwrap());
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

        doc.look(Look::StartRevolve);
        for region in regions {
            doc.look(Look::Revolve(RevolveLook::PickRegion { sketch, region }));
        }
        let axis = AxisLine::Curve(axis);
        doc.look(Look::Revolve(RevolveLook::PickAxis { sketch, axis }));
        answer(&mut doc, &requests);
        doc.update(Edit::CommitRevolve);
        answer(&mut doc, &requests);
        assert!(doc.revolve.is_none());
        aim(&mut doc, -0.3, -0.25, 9.0);
        camera.take(&doc, "21-round-solids", Shot::new());
        camera.take(&doc, "21-round-solids-scale2", Shot::new().scale(2.0));
        aim(&mut doc, 0.6, 0.5, 5.0);
        camera.take(&doc, "21-round-solids-close-dark", Shot::new().dark());
    });
}

/// The window's pixels, every 2 of the viewport (the window right of the
/// side panel and under the toolbar), where `accept` takes what `doc`'s
/// pick index picks there, as the viewport picks: the index, the pick,
/// and the viewport's size and the pixel in it.
fn scan(
    doc: &Doc,
    accept: impl Fn(&varde_view::PickIndex, varde_view::Pick, [f32; 2], glam::DVec2) -> bool,
) -> Vec<Point> {
    use varde_view::Picks;
    let origin = Point::new(varde_view::SIDE_PANEL_WIDTH, 40.0);
    let size = [WINDOW.width - origin.x, WINDOW.height - origin.y];
    let index = doc.feed.pick_index();
    let mut found = Vec::new();
    for y in (0..size[1] as u32).step_by(2) {
        for x in (0..size[0] as u32).step_by(2) {
            let at = glam::DVec2::new(f64::from(x), f64::from(y));
            let Some(pick) = index.pick(&doc.camera, size, at, Picks::All) else {
                continue;
            };
            if accept(index, pick, size, at) {
                found.push(Point::new(origin.x + x as f32, origin.y + y as f32));
            }
        }
    }
    found
}

/// Where in the window the cursor over what the model shows there takes
/// the snap point at `point` (to within a micrometre) of it, or of `held`
/// hovered if given: the middle of the pixels that do.
fn snapping_to(doc: &Doc, point: glam::DVec3, held: Option<varde_view::Picked>) -> Point {
    let camera = doc.camera;
    let found = scan(doc, |index, pick, size, at| {
        let target = held.unwrap_or(pick.target);
        let snap = index.snap(&camera, size, at, target);
        snap.is_some_and(|(_, at)| at.distance(point) < 1e-6)
    });
    assert!(!found.is_empty(), "nothing snaps to {point}");
    let (x, y) = found
        .iter()
        .fold((0.0, 0.0), |(x, y), p| (x + p.x, y + p.y));
    let n = found.len() as f32;
    Point::new(x / n, y / n)
}

/// A pixel of the window over the edge whose point is `point`.
fn over_edge_of(doc: &Doc, point: glam::DVec3) -> Point {
    let found = scan(doc, |index, pick, _, _| {
        matches!(pick.target, varde_view::Picked::Edge(_))
            && (index.snaps(pick.target).iter()).any(|&(snapped, at)| {
                matches!(snapped, varde_view::Snapped::EdgePoint(_)) && at.distance(point) < 1e-6
            })
    });
    *found.get(found.len() / 2).expect("the edge shows")
}

/// Scenario 22: the measure tool: `I`, the top hovered by its front right
/// corner, its corners' dots shown and the corner taken; the corner
/// picked and the hole's rim hovered by its centre; the distance from
/// the corner to the centre, its segment and label, in light and dark;
/// the top and the rim, both highlighted, the rim in the second colour;
/// the body double-clicked, in inches.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_22_measure() {
    shooting(|camera| {
        let (mut doc, requests) = example();
        framed(&mut doc);
        key_in(&mut doc, key("i"));
        camera.take(&doc, "22-measure-start", Shot::new());
        let corner = glam::DVec3::new(30.0, -20.0, 10.0);
        let at = snapping_to(&doc, corner, None);
        camera.hover(&mut doc, at);
        let hovered = doc.pick.hover().expect("the corner under the cursor");
        assert!(hovered.snap.is_some(), "{hovered:?}");
        let shot = Shot::new().pointer(Pointer::At(at));
        camera.take(&doc, "22-measure-hover-corner", shot);
        let click = |doc: &mut Doc, add: bool, double: bool| {
            let pick = doc.pick.hover();
            doc.look(Look::ClickModel { pick, add, double });
        };
        click(&mut doc, false, false);
        answer(&mut doc, &requests);
        // Over the rim first, then off it to its centre.
        let centre = glam::DVec3::new(0.0, 0.0, 10.0);
        let over_rim = over_edge_of(&doc, centre);
        camera.hover(&mut doc, over_rim);
        let rim_target = doc.pick.hover().expect("the rim").target;
        let at = snapping_to(&doc, centre, Some(rim_target));
        camera.hover(&mut doc, at);
        assert!(doc.pick.hover().is_some_and(|pick| pick.snap.is_some()));
        let shot = Shot::new().pointer(Pointer::At(at));
        camera.take(&doc, "22-measure-hover-centre", shot);
        click(&mut doc, false, false);
        answer(&mut doc, &requests);
        camera.take(&doc, "22-measure-corner-centre", Shot::new());
        camera.take(&doc, "22-measure-corner-centre-dark", Shot::new().dark());
        // The top, then the rim: faces and edges highlighted.
        let top = Point::new(WINDOW.width * 0.66, WINDOW.height * 0.5);
        camera.hover(&mut doc, top);
        click(&mut doc, false, false);
        let rim = over_rim;
        camera.hover(&mut doc, rim);
        click(&mut doc, false, false);
        answer(&mut doc, &requests);
        doc.look(Look::Measure(varde_view::MeasureLook::Fold(
            varde_view::MeasureSlot::B,
        )));
        camera.take(
            &doc,
            "22-measure-top-rim",
            Shot::new().pointer(Pointer::At(rim)),
        );
        camera.take(&doc, "22-measure-top-rim-dark", Shot::new().dark());
        // The body, double-clicked, in inches.
        doc.update(Edit::SetUnits(varde_document::LengthUnit::In));
        answer(&mut doc, &requests);
        camera.hover(&mut doc, top);
        click(&mut doc, false, false);
        click(&mut doc, false, true);
        answer(&mut doc, &requests);
        camera.take(&doc, "22-measure-body-in", Shot::new());
    });
}

/// Scenario 23: the example's plate and two discs through it made as
/// bodies of their own, combined: `B` with nothing picked, the plate the
/// target and the right disc a kept tool (their highlights, the left disc
/// hovered), a union using both discs up (Objects listing them faint),
/// a subtract, dark, and the right disc's extrude edited into a join,
/// its panel saying why it stays a new body.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_23_combine() {
    shooting(|camera| {
        let mut editor = varde_document::Editor::new(varde_document::Document::example());
        for center in [(20.0, 0.0), (-20.0, 0.0)] {
            let extent = crate::tests::two_sides(editor.document(), "15", "5");
            let new = varde_document::Operation::NewBody(varde_document::BodyId::NEW);
            crate::tests::add_disc(&mut editor, center, extent, new);
        }
        let (mut doc, requests) = crate::tests::holding(editor.document().clone());
        let bodies: Vec<varde_document::BodyId> = doc
            .editor
            .document()
            .bodies()
            .iter()
            .map(|b| b.id)
            .collect();
        aim(&mut doc, 0.0, 0.3, PLATE_ZOOM);
        key_in(&mut doc, key("b"));
        camera.take(&doc, "23-combine-start", Shot::new());
        doc.look(Look::ClickBody {
            body: bodies[0],
            add: false,
        });
        doc.look(Look::ClickBody {
            body: bodies[1],
            add: false,
        });
        doc.look(Look::Combine(varde_view::CombineLook::KeepTools));
        answer(&mut doc, &requests);
        // The left disc hovered.
        let left = Point::new(620.0, 285.0);
        camera.hover(&mut doc, left);
        camera.take(
            &doc,
            "23-combine-kept",
            Shot::new().pointer(Pointer::At(left)),
        );
        doc.look(Look::Combine(varde_view::CombineLook::KeepTools));
        doc.look(Look::ClickBody {
            body: bodies[2],
            add: false,
        });
        answer(&mut doc, &requests);
        doc.look(Look::SelectPanel(varde_view::Panel::Objects));
        camera.take(&doc, "23-combine-union", Shot::new());
        doc.look(Look::Combine(varde_view::CombineLook::Operation(
            varde_document::BodyOp::Subtract,
        )));
        answer(&mut doc, &requests);
        camera.take(&doc, "23-combine-subtract", Shot::new());
        camera.take(&doc, "23-combine-subtract-dark", Shot::new().dark());
        doc.update(Edit::CommitCombine);
        answer(&mut doc, &requests);
        doc.look(Look::SelectPanel(varde_view::Panel::Timeline));
        camera.take(&doc, "23-combine-timeline", Shot::new());
        let maker = doc.editor.document().body(bodies[1]).unwrap().created_by;
        doc.look(Look::EditFeature(maker));
        extrude(&mut doc, ExtrudeLook::Operation(OperationKind::Join));
        answer(&mut doc, &requests);
        camera.take(&doc, "23-combine-held-extrude", Shot::new());
    });
}

/// Scenario 24: sketches on faces: `S` with the plate's top hovered; a
/// circle in a sketch on the top, the Sketch tab naming the face; its
/// Timeline row's menu with Change plane; changing the example's first
/// sketch's plane with the top hovered, which came after it; the top's
/// sketch failing once the plate is gone, and entering it asking for a
/// plane.
#[test]
#[ignore = "writes screenshots, see the module"]
fn shots_24_sketch_on_face() {
    use varde_document::Plane;
    use varde_view::{Panel, Picked, RowMenu};

    shooting(|camera| {
        let (mut doc, requests) = example();
        framed(&mut doc);
        let top = Point::new(WINDOW.width * 0.66, WINDOW.height * 0.5);
        doc.look(Look::PickPlane);
        let hovered = camera.hover(&mut doc, top);
        assert!(matches!(hovered, Some(Picked::Face(_))), "{hovered:?}");
        let shot = Shot::new().pointer(Pointer::At(top));
        camera.take(&doc, "24-pick-top", shot);
        camera.take(&doc, "24-pick-top-dark", shot.dark());

        let pick = doc.pick.hover().expect("the top under the cursor");
        let Picked::Face(face) = pick.target else {
            unreachable!()
        };
        let face = doc.feed.pick_index().face_ref(face, pick.at).unwrap();
        doc.update(Edit::FacePicked(face));
        let id = doc.sketch.as_ref().expect("in the sketch").feature;
        let mut drawn = varde_sketch::Sketch::default();
        let center = drawn.add_point(glam::DVec2::new(20.0, 10.0)).unwrap();
        drawn
            .add_curve(
                varde_sketch::Curve::Circle {
                    center,
                    radius: 6.0,
                },
                false,
            )
            .unwrap();
        doc.apply(Command::SetSketch {
            feature: id,
            sketch: Box::new(drawn),
        });
        doc.sync();
        answer(&mut doc, &requests);
        doc.animation_frame(Instant::now() + 2 * crate::doc::CAMERA_ANIMATION);
        doc.look(Look::SelectPanel(Panel::Sketch));
        // Home frames the circle drawn since.
        doc.look(Look::ResetCamera);
        doc.animation_frame(Instant::now() + 2 * crate::doc::CAMERA_ANIMATION);
        doc.look(Look::Zoom {
            factor: 3.0,
            x: 0.0,
            y: 0.0,
        });
        camera.take(&doc, "24-sketch-on-top", Shot::new());

        doc.look(Look::FinishSketch);
        answer(&mut doc, &requests);
        framed(&mut doc);
        doc.look(Look::SelectPanel(Panel::Timeline));
        doc.look(Look::OpenMenu(RowMenu::Feature(id)));
        camera.take(&doc, "24-timeline-menu", Shot::new());
        doc.look(Look::Escape);

        // The first sketch, before the plate: its top can't take it.
        let first = doc.editor.document().features()[0].id;
        doc.look(Look::ChangePlane(first));
        camera.hover(&mut doc, top);
        camera.take(&doc, "24-change-later-face", shot);
        doc.look(Look::Escape);

        // The plate gone, the top's sketch fails.
        let extrude = doc.editor.document().features()[1].id;
        doc.update(Edit::RemoveFeature(extrude));
        doc.update(Edit::ConfirmDelete);
        answer(&mut doc, &requests);
        assert!(matches!(
            doc.editor.document().feature(id).map(|f| &f.kind),
            Some(FeatureKind::Sketch {
                plane: Plane::Face(_),
                ..
            })
        ));
        camera.take(
            &doc,
            "24-face-gone-timeline",
            Shot::new().pointer(Pointer::Over("on a face")),
        );
        doc.look(Look::EditFeature(id));
        camera.take(&doc, "24-face-gone-pick", Shot::new());
    });
}
