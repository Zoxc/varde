use glam::{DVec2, Vec3};
use iced::mouse::{Cursor, Event};
use varde_render::{Camera, View};
use varde_view::{Look, Message as Ui, Mode, Picked};

use super::*;
use crate::tests::{answer, example, shown};

/// The viewport the tests pick in, in logical pixels.
const SIZE: [f32; 2] = [400.0, 300.0];

/// From the top, the example's plate filling the view: its top's centre
/// at (20, 5) shows at `(250, 125)`.
fn top() -> Camera {
    let mut camera = Camera::default();
    camera.look_from(View::Top);
    camera.set_target(Vec3::new(0.0, 0.0, 5.0));
    camera.zoom(60.0 / camera.view_height());
    camera
}

/// What the cursor over the example's plate's top picks in `doc`.
fn on_top(doc: &Doc) -> varde_view::Pick {
    let index = doc.feed.pick_index();
    let pick = index.pick(&top(), SIZE, DVec2::new(250.0, 125.0)).unwrap();
    assert!(matches!(pick.target, Picked::Face(_)), "{pick:?}");
    pick
}

#[test]
fn hovering_changes_only_the_highlight() {
    let (mut doc, requests) = example();
    let pick = on_top(&doc);
    let generation = doc.editor.generation();
    let (camera, selected, panel) = (doc.camera, doc.selected_feature, doc.panel);
    let edited = doc.edited();
    assert_eq!(doc.highlight(), None);

    doc.look(Look::Hover(Some(pick)));
    assert_eq!(doc.hover.pick(), Some(pick));
    let highlight = doc.highlight().unwrap().clone();
    assert!(!highlight.is_empty());
    assert!(requests.borrow().is_empty(), "no regeneration asked for");
    assert_eq!(doc.editor.generation(), generation);
    assert_eq!(doc.edited(), edited);
    assert_eq!(
        (doc.camera, doc.selected_feature, doc.panel),
        (camera, selected, panel)
    );
    // Elsewhere on the same face, the same highlight, not built again.
    let moved = varde_view::Pick {
        at: pick.at + glam::DVec3::X,
        ..pick
    };
    doc.look(Look::Hover(Some(moved)));
    assert!(Arc::ptr_eq(doc.highlight().unwrap(), &highlight));
    assert_eq!(doc.model_picking().unwrap().hovered, Some(pick.target));

    doc.look(Look::Hover(None));
    assert_eq!(doc.highlight(), None);
    assert!(requests.borrow().is_empty());
    assert_eq!(doc.editor.generation(), generation);
}

#[test]
fn a_new_model_drops_the_hover_and_picks_of_the_old_one() {
    let (mut doc, requests) = example();
    let pick = on_top(&doc);
    doc.look(Look::Hover(Some(pick)));
    // A coarser tolerance: another mesh.
    let tolerance = varde_document::Tolerance::new(1e-2).unwrap();
    doc.update(varde_view::Edit::SetTolerance(tolerance));
    // Not until the new model shows.
    assert_eq!(doc.hover.pick(), Some(pick));
    answer(&mut doc, &requests);
    assert_ne!(doc.feed.model(), pick.model);
    assert_eq!(doc.hover.pick(), None);
    assert_eq!(doc.highlight(), None);
    // A pick of the old model, sent before it went, is dropped.
    doc.look(Look::Hover(Some(pick)));
    assert_eq!(doc.hover.pick(), None);
    let pick = on_top(&doc);
    doc.look(Look::Hover(Some(pick)));
    assert_eq!(doc.hover.pick(), Some(pick));
}

#[test]
fn the_cursor_doesnt_pick_in_a_sketch() {
    let (mut doc, _requests) = example();
    let pick = on_top(&doc);
    doc.look(Look::Hover(Some(pick)));
    let sketch = doc.editor.document().features()[0].id;
    doc.look(Look::EditFeature(sketch));
    assert!(doc.sketch.is_some());
    assert!(doc.model_picking().is_none());
    assert_eq!(doc.highlight(), None);
    doc.look(Look::Hover(Some(pick)));
    assert_eq!(doc.hover.pick(), None);
    doc.look(Look::FinishSketch);
    assert!(doc.model_picking().is_some());
}

#[test]
fn moving_the_cursor_over_the_model_says_what_it_hovers() {
    let (mut doc, _requests) = example();
    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();
    // Over the middle of the viewport, the plate shows from Home.
    let at = iced::Point::new(780.0, 450.0);
    let mut moved = |doc: &Doc, at: iced::Point| {
        let mut ui = shown(doc.view(false, Mode::Light, true), size, &mut renderer);
        let mut sent = Vec::new();
        let _ = ui.update(
            &[iced::Event::Mouse(Event::CursorMoved { position: at })],
            Cursor::Available(at),
            &mut renderer,
            &mut iced::advanced::clipboard::Null,
            &mut sent,
        );
        sent
    };
    let sent = moved(&doc, at);
    let [Ui::Look(Look::Hover(Some(pick)))] = sent[..] else {
        panic!("{sent:?}");
    };
    assert_eq!(pick.model, doc.feed.model());
    doc.look(Look::Hover(Some(pick)));
    // Moved a pixel, still over it: nothing to say.
    let sent = moved(&doc, iced::Point::new(at.x + 1.0, at.y));
    assert!(sent.is_empty(), "{sent:?}");
}
