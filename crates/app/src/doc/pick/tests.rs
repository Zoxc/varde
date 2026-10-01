use glam::{DVec2, Vec3};
use iced::mouse::{Cursor, Event};
use varde_render::{Camera, View};
use varde_view::{Look, Message as Ui, Mode, Picked, Picks};

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
    let pick = index
        .pick(&top(), SIZE, DVec2::new(250.0, 125.0), Picks::FacesAndEdges)
        .unwrap();
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
    assert_eq!(doc.pick.hover(), Some(pick));
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
    assert_eq!(doc.pick.hover(), Some(pick));
    answer(&mut doc, &requests);
    assert_ne!(doc.feed.model(), pick.model);
    assert_eq!(doc.pick.hover(), None);
    assert_eq!(doc.highlight(), None);
    // A pick of the old model, sent before it went, is dropped.
    doc.look(Look::Hover(Some(pick)));
    assert_eq!(doc.pick.hover(), None);
    let pick = on_top(&doc);
    doc.look(Look::Hover(Some(pick)));
    assert_eq!(doc.pick.hover(), Some(pick));
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
    assert_eq!(doc.pick.hover(), None);
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

/// The texts of `doc`'s status bar, left to right, at 1280 × 800.
fn status_bar(doc: &Doc) -> Vec<String> {
    use crate::tests::texts;
    let size = iced::Size::new(1280.0, 800.0);
    let top = size.height - varde_view::STATUS_BAR_ROOM;
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(doc.view(false, Mode::Light, true), size, &mut renderer);
    let mut bar: Vec<_> = texts(&mut ui, &renderer)
        .into_iter()
        .filter(|text| text.bounds.y >= top && text.bounds.x >= varde_view::SIDE_PANEL_WIDTH)
        .collect();
    bar.sort_by(|a, b| a.bounds.x.total_cmp(&b.bounds.x));
    bar.into_iter().map(|text| text.text).collect()
}

/// What a left click at `at` over the viewport of `doc`'s screen, in
/// the app's window, sends, with `modifiers` held.
fn click(doc: &Doc, at: iced::Point, modifiers: iced::keyboard::Modifiers) -> Vec<Ui> {
    use iced::mouse::Button;
    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(doc.view(false, Mode::Light, true), size, &mut renderer);
    let mut sent = Vec::new();
    for event in [
        iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)),
        iced::Event::Mouse(Event::CursorMoved { position: at }),
        iced::Event::Mouse(Event::ButtonPressed(Button::Left)),
        iced::Event::Mouse(Event::ButtonReleased(Button::Left)),
    ] {
        let _ = ui.update(
            &[event],
            Cursor::Available(at),
            &mut renderer,
            &mut iced::advanced::clipboard::Null,
            &mut sent,
        );
    }
    sent
}

/// Over the middle of the viewport, the plate's top shows from Home.
const OVER_TOP: iced::Point = iced::Point::new(780.0, 450.0);

/// Clicks at `at` with `modifiers` held, see [`click`], and takes what
/// that sends.
fn clicking(doc: &mut Doc, at: iced::Point, modifiers: iced::keyboard::Modifiers) {
    let sent = click(doc, at, modifiers);
    take(doc, sent);
}

/// Takes what clicking at `at` sends, as the app would.
fn take(doc: &mut Doc, sent: Vec<Ui>) {
    for message in sent {
        if let Ui::Look(look) = message {
            doc.look(look);
        }
    }
}

#[test]
fn a_click_selects_in_the_model_and_esc_or_space_clears_it() {
    let (mut doc, requests) = example();
    let extrude = doc.editor.document().features()[1].id;
    doc.look(Look::SelectFeature(extrude));
    let none = iced::keyboard::Modifiers::empty();
    let sent = click(&doc, OVER_TOP, none);
    assert!(
        sent.iter().any(|message| matches!(
            message,
            Ui::Look(Look::ClickModel {
                pick: Some(_),
                add: false,
                double: false
            })
        )),
        "{sent:?}"
    );
    take(&mut doc, sent);
    let picked: Vec<_> = doc.pick.selection.targets().collect();
    assert!(matches!(picked[..], [Picked::Face(_)]), "{picked:?}");
    // The feature selected is let go of; the model's selection shows.
    assert_eq!(doc.selected_feature, None);
    let highlight = doc.highlight().unwrap().clone();
    assert!(!highlight.is_empty());
    assert!(requests.borrow().is_empty(), "no regeneration asked for");
    // Selecting a feature lets go of the model's selection.
    doc.look(Look::SelectFeature(extrude));
    assert!(doc.pick.selection.is_empty());
    clicking(&mut doc, OVER_TOP, none);
    assert!(!doc.pick.selection.is_empty());
    doc.look(Look::Escape);
    assert!(doc.pick.selection.is_empty());
    clicking(&mut doc, OVER_TOP, none);
    doc.look(Look::ClearSelection);
    assert!(doc.pick.selection.is_empty());
    // What's hovered still shows.
    assert!(doc.pick.hover().is_some());
    assert_ne!(**doc.highlight().unwrap(), *highlight);
}

#[test]
fn shift_or_ctrl_toggles_and_a_double_click_takes_the_body() {
    let (mut doc, _requests) = example();
    let shift = iced::keyboard::Modifiers::SHIFT;
    let none = iced::keyboard::Modifiers::empty();
    clicking(&mut doc, OVER_TOP, shift);
    assert_eq!(doc.pick.selection.targets().count(), 1);
    clicking(&mut doc, OVER_TOP, iced::keyboard::Modifiers::CTRL);
    assert!(doc.pick.selection.is_empty());
    // Two clicks soon after each other: the body. Each screen is built
    // anew, so the second click is sent as the viewport would.
    clicking(&mut doc, OVER_TOP, none);
    let pick = doc.pick.hover().unwrap();
    doc.look(Look::ClickModel {
        pick: Some(pick),
        add: false,
        double: true,
    });
    let body = doc.editor.document().bodies()[0].id;
    let bodies: Vec<_> = doc.pick.selection.bodies().collect();
    assert_eq!(bodies, [body]);
    assert_eq!(doc.pick.selection.targets().count(), 0);
    assert!(!doc.highlight().unwrap().is_empty());
}

#[test]
fn objects_and_the_viewport_select_bodies_alike() {
    let (mut doc, _requests) = example();
    let body = doc.editor.document().bodies()[0].id;
    doc.look(Look::SelectPanel(varde_view::Panel::Objects));
    // Body 1's row, clicked, says so.
    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(doc.view(false, Mode::Light, true), size, &mut renderer);
    let row = crate::tests::texts(&mut ui, &renderer)
        .into_iter()
        .find(|text| text.text == "Body 1")
        .expect("Body 1's row");
    let sent = crate::tests::clicked(&mut ui, &mut renderer, row.bounds.center());
    drop(ui);
    assert!(
        sent.iter().any(
            |message| matches!(message, Ui::Look(Look::ClickBody { body: b, .. }) if *b == body)
        ),
        "{sent:?}"
    );
    take(&mut doc, sent);
    assert_eq!(doc.pick.selection.bodies().collect::<Vec<_>>(), [body]);
    // Its faces show selected in the viewport.
    let index = doc.feed.pick_index();
    let faces = index
        .body_faces(body)
        .map(|f| (Picked::Face(f), varde_render::Emphasis::Selected));
    assert_eq!(**doc.highlight().unwrap(), index.highlight(faces));
    // And the status bar tells of it.
    let bar = status_bar(&doc);
    assert!(
        bar.starts_with(&[
            "Body 1".into(),
            "Body".into(),
            "Space".into(),
            "Clear".into()
        ]),
        "{bar:?}"
    );
    // Taken out with Ctrl, as the app fills in.
    doc.look(Look::ClickBody { body, add: true });
    assert!(doc.pick.selection.is_empty());
    // A double-click in the viewport selects it there too.
    let pick = on_top(&doc);
    doc.look(Look::ClickModel {
        pick: Some(pick),
        add: false,
        double: true,
    });
    assert_eq!(doc.pick.selection.bodies().collect::<Vec<_>>(), [body]);
}

#[test]
fn the_status_bar_tells_of_the_selection_and_how_to_change_it() {
    let (mut doc, _requests) = example();
    let pick = on_top(&doc);
    doc.look(Look::ClickModel {
        pick: Some(pick),
        add: false,
        double: false,
    });
    let bar = status_bar(&doc);
    assert_eq!(
        bar[..8],
        [
            "Face",
            "Plane",
            "Body 1",
            "Space",
            "Clear",
            "Shift",
            "Add or remove",
            "Body"
        ],
        "{bar:?}"
    );
    doc.look(Look::ClickBody {
        body: pick.body,
        add: true,
    });
    let bar = status_bar(&doc);
    assert_eq!(
        bar[..3],
        ["2 selected", "1 face · 1 body", "Space"],
        "{bar:?}"
    );
}

#[test]
fn a_selection_outlives_regenerations_and_drops_what_s_gone() {
    let (mut doc, requests) = example();
    let pick = on_top(&doc);
    doc.look(Look::ClickModel {
        pick: Some(pick),
        add: false,
        double: false,
    });
    let item = *doc.pick.selection.items().next().unwrap();
    // Another model of the same faces: found again by name.
    let tolerance = varde_document::Tolerance::new(1e-2).unwrap();
    doc.update(varde_view::Edit::SetTolerance(tolerance));
    answer(&mut doc, &requests);
    assert_ne!(doc.feed.model(), pick.model);
    assert_eq!(doc.pick.selection.model(), Some(doc.feed.model()));
    assert_eq!(
        doc.pick.selection.items().copied().collect::<Vec<_>>(),
        [item]
    );
    assert_eq!(doc.pick.selection.targets().count(), 1);
    assert!(!doc.highlight().unwrap().is_empty());
    // Not while a sketch is edited: what's selected isn't drawn then.
    let sketch = doc.editor.document().features()[0].id;
    doc.look(Look::EditFeature(sketch));
    assert!(doc.highlight().is_none());
    doc.look(Look::FinishSketch);
    assert!(doc.highlight().is_some());
    // The body hidden: its face isn't there to find, and goes; the body
    // selected as a body stays, as the document holds it.
    doc.look(Look::ClickBody {
        body: pick.body,
        add: true,
    });
    doc.update(varde_view::Edit::ToggleVisible(pick.body));
    answer(&mut doc, &requests);
    let items: Vec<_> = doc.pick.selection.items().copied().collect();
    assert_eq!(items, [varde_view::Selected::Body(pick.body)]);
    // Undone, the face isn't found again: it was dropped.
    doc.update(varde_view::Edit::Undo);
    answer(&mut doc, &requests);
    assert_eq!(doc.pick.selection.targets().count(), 0);
}

/// Once the extrude being set up ends, its draft's preview shows until
/// the answer without it comes: the cursor doesn't pick it, and what's
/// selected isn't looked for in it, so a face the draft hasn't got (an
/// intersect keeps none of the plate's walls) isn't dropped.
#[test]
fn a_draft_still_shown_after_the_extrude_doesnt_drop_the_selection() {
    use varde_view::{ExtrudeLook, OperationKind};

    let (mut doc, sketch, requests) = crate::tests::example_and_a_hole();
    let index = doc.feed.pick_index();
    // A wall of the plate: a plane facing sideways.
    let (face, body) = (index.picking().faces().iter().enumerate())
        .find_map(|(i, face)| match face.summary {
            varde_regen::Summary::Plane { n, .. } if n[2] == 0.0 => {
                Some((u32::try_from(i).unwrap(), face.body))
            }
            _ => None,
        })
        .unwrap();
    let pick = varde_view::Pick {
        model: index.model(),
        target: Picked::Face(face),
        body,
        at: glam::DVec3::ZERO,
    };
    doc.look(Look::ClickModel {
        pick: Some(pick),
        add: false,
        double: false,
    });
    let selected: Vec<_> = doc.pick.selection.items().copied().collect();
    assert_eq!(selected.len(), 1);

    doc.look(Look::StartExtrude);
    doc.look(Look::Extrude(ExtrudeLook::PickRegion { sketch, region: 0 }));
    doc.look(Look::Extrude(ExtrudeLook::Operation(
        OperationKind::Intersect,
    )));
    answer(&mut doc, &requests);
    assert!(doc.feed.shows_draft());
    let walls = |doc: &Doc| {
        let index = doc.feed.pick_index();
        let Some(varde_view::Selected::Face { key, .. }) = selected.first() else {
            unreachable!()
        };
        index.find_face(body, key, glam::DVec3::NAN).is_some()
    };
    assert!(!walls(&doc), "the draft has the wall");
    doc.look(Look::Escape);
    assert!(doc.extrude.is_none());
    assert!(doc.feed.shows_draft());
    assert!(!doc.picks());
    assert_eq!(doc.highlight(), None);
    let items = |doc: &Doc| doc.pick.selection.items().copied().collect::<Vec<_>>();
    assert_eq!(items(&doc), selected);

    answer(&mut doc, &requests);
    assert!(!doc.feed.shows_draft());
    assert!(walls(&doc));
    assert_eq!(items(&doc), selected);
    assert!(doc.highlight().is_some());
}
