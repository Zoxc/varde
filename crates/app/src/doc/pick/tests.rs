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
        .pick(&top(), SIZE, DVec2::new(250.0, 125.0), Picks::All)
        .unwrap();
    assert!(matches!(pick.target, Picked::Face(_)), "{pick:?}");
    pick
}

/// Makes the example's plate 12 mm thick: another mesh of the same
/// faces. (A coarser tolerance draws this plate alike.)
fn thicken(doc: &mut Doc) {
    let document = doc.editor.document();
    let feature = document.features()[1].id;
    let Some(varde_document::FeatureKind::Extrude(extrude)) =
        document.feature(feature).map(|f| f.kind.clone())
    else {
        panic!("the example's second feature is its extrude");
    };
    let ask = varde_document::Extent::ask(&document.design());
    let distance = varde_expr::Value::new("12", &ask).unwrap();
    let extrude = varde_document::Extrude {
        extent: varde_document::Extent::OneSide(distance),
        ..extrude
    };
    doc.apply(varde_document::Command::SetFeature {
        feature,
        kind: Box::new(extrude.into()),
    });
    doc.sync();
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
    // A thicker plate: another mesh.
    thicken(&mut doc);
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
fn a_vertex_hovered_and_clicked_is_drawn_and_told_of() {
    let (mut doc, _requests) = example();
    // The plate's top front left corner, at (-30, -20), 5 pixels a
    // millimetre from (0, 0) in the middle: 3 pixels off it, on the top.
    let index = doc.feed.pick_index();
    let at = DVec2::new(200.0 - 150.0 + 3.0, 150.0 + 100.0 - 3.0);
    let pick = index.pick(&top(), SIZE, at, Picks::All).unwrap();
    let Picked::Vertex(corner) = pick.target else {
        panic!("{pick:?}");
    };
    doc.look(Look::Hover(Some(pick)));
    let vertices = &doc.highlight().unwrap().highlights.vertices;
    assert_eq!(vertices.len(), 1);
    assert!(vertices[0].corner == corner && vertices[0].hovered && !vertices[0].selected);
    doc.look(Look::ClickModel {
        pick: Some(pick),
        add: false,
        double: false,
    });
    let vertices = &doc.highlight().unwrap().highlights.vertices;
    assert!(vertices[0].hovered && vertices[0].selected);
    let bar = status_bar(&doc);
    assert_eq!(bar[..2], ["Vertex", "Body 1"], "{bar:?}");
}

#[test]
fn a_face_listed_while_picking_a_plane_is_picked_for_the_sketch() {
    let (mut doc, _requests) = example();
    doc.look(Look::PickPlane);
    let index = doc.feed.pick_index();
    let picks = index.overlaps(&top(), SIZE, DVec2::new(250.0, 125.0), Picks::All, 8.0, 12);
    // The top, then the bottom under it.
    assert_eq!(picks.len(), 2);
    doc.look(Look::OpenOverlaps(varde_view::Overlaps {
        held: DVec2::ZERO,
        at: DVec2::ZERO,
        items: varde_view::OverlapItems::Model(picks),
    }));
    assert!(doc.overlaps.is_some());
    // Each row picks alone: no ticks, and Ctrl held picks all the same.
    let ticks = doc.overlap_ticks().unwrap();
    assert!(
        ticks.iter().all(|tick| tick.only && !tick.ticked),
        "{ticks:?}"
    );
    doc.look(Look::ChooseOverlap {
        index: 1,
        add: true,
    });
    assert!(doc.overlaps.is_none());
    assert!(doc.picking_plane.is_none());
    let session = doc
        .sketch
        .as_ref()
        .expect("a sketch on the bottom is entered");
    let document = doc.editor.document();
    let plane = match &document.feature(session.feature).unwrap().kind {
        varde_document::FeatureKind::Sketch { plane, .. } => plane,
        kind => panic!("{kind:?}"),
    };
    assert!(
        matches!(plane, varde_document::Plane::Face(face) if face.near.z.abs() < 1e-9),
        "{plane:?}"
    );
}

#[test]
fn an_origin_plane_listed_while_picking_a_plane_is_hovered_and_picked() {
    let (mut doc, _requests) = example();
    doc.look(Look::PickPlane);
    let index = doc.feed.pick_index();
    let picks = index.overlaps(&top(), SIZE, DVec2::new(250.0, 125.0), Picks::All, 8.0, 12);
    let mut items = vec![varde_view::OverlapItem::Origin(
        varde_document::OriginPlane::XY,
    )];
    items.extend(picks.into_iter().map(varde_view::OverlapItem::Model));
    doc.look(Look::OpenOverlaps(varde_view::Overlaps {
        held: DVec2::ZERO,
        at: DVec2::ZERO,
        items: varde_view::OverlapItems::Mixed(items),
    }));
    doc.look(Look::HoverOverlap(Some(0)));
    assert_eq!(doc.plane_hover, Some(varde_document::OriginPlane::XY));
    doc.look(Look::HoverOverlap(None));
    assert_eq!(doc.plane_hover, None);
    doc.look(Look::ChooseOverlap {
        index: 0,
        add: false,
    });
    assert!(doc.overlaps.is_none());
    let session = doc.sketch.as_ref().expect("a sketch on XY is entered");
    let document = doc.editor.document();
    let plane = match &document.feature(session.feature).unwrap().kind {
        varde_document::FeatureKind::Sketch { plane, .. } => plane,
        kind => panic!("{kind:?}"),
    };
    assert_eq!(
        *plane,
        varde_document::Plane::Origin(varde_document::OriginPlane::XY)
    );
}

#[test]
fn a_face_listed_where_faces_overlap_is_hovered_and_chosen() {
    let (mut doc, _requests) = example();
    let index = doc.feed.pick_index();
    let picks = index.overlaps(&top(), SIZE, DVec2::new(250.0, 125.0), Picks::All, 8.0, 12);
    // The top, then the bottom under it.
    assert_eq!(picks.len(), 2);
    let list = varde_view::Overlaps {
        held: DVec2::ZERO,
        at: DVec2::ZERO,
        items: varde_view::OverlapItems::Model(picks.clone()),
    };
    doc.look(Look::OpenOverlaps(list.clone()));
    assert!(!doc.hovers_through());
    // Hovered, the bottom shows through the top.
    doc.look(Look::HoverOverlap(Some(1)));
    assert_eq!(doc.pick.hover(), Some(picks[1]));
    assert!(doc.hovers_through());
    // The viewport's hover waits while the list is open.
    doc.look(Look::Hover(None));
    assert_eq!(doc.pick.hover(), Some(picks[1]));
    doc.look(Look::ChooseOverlap {
        index: 1,
        add: false,
    });
    assert_eq!(
        doc.pick.selection.targets().collect::<Vec<_>>(),
        [picks[1].target]
    );
    assert_eq!(doc.pick.hover(), None);
    assert!(!doc.hovers_through());
    // Its tick adds the top, the list kept open.
    doc.look(Look::OpenOverlaps(list.clone()));
    doc.look(Look::ToggleOverlap(0));
    assert!(doc.overlaps.is_some());
    assert_eq!(
        doc.pick.selection.targets().collect::<Vec<_>>(),
        [picks[1].target, picks[0].target]
    );
    doc.look(Look::ToggleOverlap(0));
    // A click away keeps it.
    doc.look(Look::CloseOverlaps);
    assert_eq!(
        doc.pick.selection.targets().collect::<Vec<_>>(),
        [picks[1].target]
    );
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
        let mut ui = shown(doc.view_in(Mode::Light), size, &mut renderer);
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
    let mut ui = shown(doc.view_in(Mode::Light), size, &mut renderer);
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
    let mut ui = shown(doc.view_in(Mode::Light), size, &mut renderer);
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
    // The model is asked for again only to measure the face.
    let asked = requests.borrow();
    assert!(
        matches!(&asked[..], [varde_regen::Request::Regenerate { inspect: Some(inspect), .. }]
            if inspect.second.is_none()),
        "{asked:?}"
    );
    drop(asked);
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
    let mut ui = shown(doc.view_in(Mode::Light), size, &mut renderer);
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
    let faces: Vec<_> = index.body_faces(body).map(Picked::Face).collect();
    assert_eq!(**doc.highlight().unwrap(), index.highlight(&[], &faces));
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
    // The mouse's hints make room for the selection.
    let bar = status_bar(&doc);
    assert_eq!(bar, ["Face", "Plane", "Body 1", "Space", "Clear"]);
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
    thicken(&mut doc);
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
    // Undone, the face is found again: it was looked for in each model
    // since, as the selection didn't change.
    doc.update(varde_view::Edit::Undo);
    answer(&mut doc, &requests);
    let items: Vec<_> = doc.pick.selection.items().copied().collect();
    assert_eq!(items, [item, varde_view::Selected::Body(pick.body)]);
    assert_eq!(doc.pick.selection.targets().count(), 1);
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
                let face = u32::try_from(i).unwrap();
                Some((face, index.face_body(face).unwrap()))
            }
            _ => None,
        })
        .unwrap();
    let pick = varde_view::Pick {
        model: index.model(),
        target: Picked::Face(face),
        body,
        at: glam::DVec3::ZERO,
        snap: None,
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

/// The face picked over the plate's top in `doc`, clicked alone.
fn select_top(doc: &mut Doc) -> varde_view::Selected {
    let pick = on_top(doc);
    doc.look(Look::ClickModel {
        pick: Some(pick),
        add: false,
        double: false,
    });
    *doc.pick.selection.items().next().unwrap()
}

#[test]
fn what_an_edit_removes_comes_back_with_its_undo_until_the_selection_changes() {
    use varde_view::Edit;

    let (mut doc, requests) = example();
    let top = select_top(&mut doc);
    let extrude = doc.editor.document().features()[1].id;
    doc.update(Edit::RemoveFeature(extrude));
    answer(&mut doc, &requests);
    assert!(doc.pick.selection.is_empty());
    assert_eq!(doc.highlight(), None);
    assert!(!status_bar(&doc).iter().any(|text| text == "Face"));
    doc.update(Edit::Undo);
    answer(&mut doc, &requests);
    assert_eq!(
        doc.pick.selection.items().copied().collect::<Vec<_>>(),
        [top]
    );
    assert_eq!(doc.pick.selection.targets().count(), 1);
    assert!(doc.highlight().is_some());
    // Redone, gone again; then a click elsewhere (off the model) changes
    // the selection, and the undo doesn't bring the face back.
    doc.update(Edit::Redo);
    answer(&mut doc, &requests);
    assert!(doc.pick.selection.is_empty());
    let model = doc.feed.model();
    doc.look(Look::ClickModel {
        pick: None,
        add: false,
        double: false,
    });
    assert!(doc.pick.selection.holds_nothing());
    doc.update(Edit::Undo);
    answer(&mut doc, &requests);
    assert_ne!(doc.feed.model(), model);
    assert!(doc.pick.selection.is_empty());
}

#[test]
fn replacing_the_document_forgets_the_selection() {
    let (mut doc, requests) = example();
    select_top(&mut doc);
    doc.look(Look::Hover(Some(on_top(&doc))));
    let model = doc.feed.model();
    // Another design restored whole: its ids might name other things.
    doc.drop_proposals();
    let (replacement, _) = crate::tests::two_plates();
    doc.apply(varde_document::Command::Replace(Box::new(
        replacement.document().clone(),
    )));
    doc.sync();
    assert!(doc.pick.selection.holds_nothing());
    assert_eq!(doc.pick.hover(), None);
    // Until the new model shows, the old one's picks don't count.
    assert_eq!(doc.feed.model(), model);
    assert!(!doc.picks());
    assert!(doc.model_picking().is_none());
    let pick = on_top(&doc);
    doc.look(Look::ClickModel {
        pick: Some(pick),
        add: false,
        double: false,
    });
    doc.look(Look::Hover(Some(pick)));
    assert!(doc.pick.selection.is_empty());
    assert_eq!(doc.pick.hover(), None);
    answer(&mut doc, &requests);
    assert!(doc.picks());
    select_top(&mut doc);
    // And undoing the replacement forgets it too.
    doc.update(varde_view::Edit::Undo);
    assert!(doc.pick.selection.holds_nothing());
}

/// A face and an edge of Body 2 are looked for in Body 1 once a join
/// merges it in, and come back to Body 2 when the join is undone; Body 2
/// selected as a body stops being selected while it's merged, and its
/// row in Objects selects Body 1, which draws it.
#[test]
fn a_join_merging_a_body_keeps_its_faces_selected_in_the_holder() {
    use varde_regen::Summary;
    use varde_view::{Edit, Selected};

    let (editor, [top, below]) = crate::tests::two_plates();
    let (mut doc, requests) = crate::tests::holding(editor.document().clone());
    // Body 2's bottom, at z -3, facing down.
    let index = doc.feed.pick_index();
    let bottom = (index.picking().faces().iter().enumerate())
        .find_map(|(i, face)| match face.summary {
            Summary::Plane { n, d } if n[2] < -0.5 && (d - 3.0).abs() < 1e-9 => {
                let face = u32::try_from(i).unwrap();
                assert_eq!(index.face_body(face), Some(below));
                Some(face)
            }
            _ => None,
        })
        .unwrap();
    let pick = varde_view::Pick {
        model: index.model(),
        target: Picked::Face(bottom),
        body: below,
        at: glam::DVec3::new(-20.0, -10.0, -3.0),
        snap: None,
    };
    doc.look(Look::ClickModel {
        pick: Some(pick),
        add: false,
        double: false,
    });
    doc.look(Look::ClickBody {
        body: below,
        add: true,
    });
    let selected: Vec<Selected> = doc.pick.selection.items().copied().collect();
    assert_eq!(selected.len(), 2);

    // Three steps: the sketch, its disc and the join.
    crate::tests::add_join(&mut doc.editor);
    doc.sync();
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.merged_bodies(), [(below, top)]);
    // The bottom, of Body 1 now, is still selected; Body 2 isn't.
    let items: Vec<Selected> = doc.pick.selection.items().copied().collect();
    assert_eq!(items, selected[..1]);
    let index = doc.feed.pick_index();
    let targets: Vec<_> = doc.pick.selection.targets().collect();
    let [Picked::Face(face)] = targets[..] else {
        panic!("{targets:?}");
    };
    assert_eq!(index.face_body(face), Some(top));
    let face = &index.picking().faces()[face as usize];
    assert!(
        matches!(face.summary, Summary::Plane { n, d } if n[2] < -0.5 && (d - 3.0).abs() < 1e-9)
    );
    // The status bar names the body it's in now, once the face is
    // measured alone.
    answer(&mut doc, &requests);
    let bar = status_bar(&doc);
    assert_eq!(bar[..2], ["Face", "Plane"], "{bar:?}");
    assert!(bar.contains(&"Body 1".to_string()), "{bar:?}");
    // The join undone: the bottom is Body 2's again.
    for _ in 0..3 {
        doc.update(Edit::Undo);
    }
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.merged_bodies(), []);
    let index = doc.feed.pick_index();
    let targets: Vec<_> = doc.pick.selection.targets().collect();
    let [Picked::Face(face)] = targets[..] else {
        panic!("{targets:?}");
    };
    assert_eq!(index.face_body(face), Some(below));
    // Redone, Body 2's row selects Body 1, which holds it.
    for _ in 0..3 {
        doc.update(Edit::Redo);
    }
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.merged_bodies(), [(below, top)]);
    doc.look(Look::ClickBody {
        body: below,
        add: false,
    });
    assert_eq!(doc.pick.selection.bodies().collect::<Vec<_>>(), [top]);
}

/// With several bodies, Objects marks those selected as bodies, whether
/// selected there or double-clicked in the viewport, and the viewport
/// draws all their faces selected.
#[test]
fn objects_and_the_viewport_agree_on_many_bodies() {
    use varde_view::{Edit, Panel, Selected};

    let (mut editor, _) = crate::tests::two_plates();
    let up = crate::tests::two_sides(editor.document(), "15", "1");
    for x in [-24.0, 0.0, 24.0] {
        let new = varde_document::Operation::NewBody(varde_document::BodyId::NEW);
        crate::tests::add_disc(&mut editor, (x, 30.0), up.clone(), new);
    }
    let (mut doc, requests) = crate::tests::holding(editor.document().clone());
    let bodies: Vec<_> = (doc.editor.document().bodies().iter())
        .map(|body| body.id)
        .collect();
    assert_eq!(bodies.len(), 5);
    doc.look(Look::SelectPanel(Panel::Objects));
    doc.look(Look::ClickBody {
        body: bodies[1],
        add: false,
    });
    doc.look(Look::ClickBody {
        body: bodies[3],
        add: true,
    });
    // Body 5 double-clicked in the viewport with Shift.
    let index = doc.feed.pick_index();
    let face = index.body_faces(bodies[4]).next().unwrap();
    let pick = varde_view::Pick {
        model: index.model(),
        target: Picked::Face(face),
        body: bodies[4],
        at: glam::DVec3::new(24.0, 30.0, 15.0),
        snap: None,
    };
    doc.look(Look::ClickModel {
        pick: Some(pick),
        add: true,
        double: false,
    });
    doc.look(Look::ClickModel {
        pick: Some(pick),
        add: true,
        double: true,
    });
    let selected: Vec<_> = doc.pick.selection.bodies().collect();
    assert_eq!(selected, [bodies[1], bodies[3], bodies[4]]);
    let index = doc.feed.pick_index();
    let faces = selected
        .iter()
        .flat_map(|&body| index.body_faces(body).collect::<Vec<_>>());
    let mut faces: Vec<_> = faces.map(Picked::Face).collect();
    faces.sort_unstable();
    let expected = index.highlight(&[], &faces);
    assert_eq!(**doc.highlight().unwrap(), expected);
    // Body 4 hidden: it stays selected, as the document holds it.
    doc.update(Edit::ToggleVisible(bodies[3]));
    answer(&mut doc, &requests);
    assert_eq!(doc.pick.selection.bodies().count(), 3);
    assert!(matches!(
        doc.pick.selection.items().next(),
        Some(Selected::Body(_))
    ));
}

/// The example and a sketch on XY after it holding a line from
/// (-25, 30) to (25, 30), off the plate: the sketch and the line.
fn example_and_a_line() -> (
    Doc,
    varde_document::FeatureId,
    varde_sketch::Id,
    std::rc::Rc<std::cell::RefCell<Vec<varde_regen::Request>>>,
) {
    let (mut doc, requests) = example();
    let plane = varde_document::Plane::Origin(varde_document::OriginPlane::XY);
    doc.apply(doc.editor.document().add_sketch(plane));
    let sketch = doc.editor.document().features().last().unwrap().id;
    let mut drawn = varde_sketch::Sketch::default();
    let a = drawn.add_point(DVec2::new(-25.0, 30.0)).unwrap();
    let b = drawn.add_point(DVec2::new(25.0, 30.0)).unwrap();
    let line = drawn
        .add_curve(varde_sketch::Curve::Line { start: a, end: b }, false)
        .unwrap();
    doc.apply(varde_document::Command::SetSketch {
        feature: sketch,
        sketch: Box::new(drawn),
    });
    doc.sync();
    answer(&mut doc, &requests);
    (doc, sketch, line, requests)
}

/// Outside the sessions a sketch's curve is hovered and selected with the
/// model, drawn as selected, kept across a new model, and dropped once
/// it's hidden or its sketch is deleted; a click on the model alone
/// selects that instead.
#[test]
fn a_sketch_s_curve_is_selected_in_the_model_until_its_sketch_goes() {
    use varde_view::{Edit, Selected, SketchItem};

    let (mut doc, sketch, line, requests) = example_and_a_line();
    let item = SketchItem { sketch, item: line };
    let picking = doc.model_picking().unwrap();
    assert!(picking.sketches.iter().any(|lines| lines.feature == sketch));
    doc.look(Look::Hover(Some(on_top(&doc))));
    doc.look(Look::HoverSketch(Some(item)));
    assert_eq!(doc.pick.sketch_hover(), Some(item));
    assert_eq!(doc.pick.hover(), None, "one or the other is hovered");
    doc.look(Look::ClickSketch { item, add: false });
    let items = |doc: &Doc| doc.pick.selection.items().copied().collect::<Vec<_>>();
    assert_eq!(items(&doc), [Selected::SketchItem { sketch, item: line }]);
    assert_eq!(doc.model_picking().unwrap().marked, [item]);
    // A click on the model with Shift adds to it; alone, it's that.
    let face = on_top(&doc);
    doc.look(Look::ClickModel {
        pick: Some(face),
        add: true,
        double: false,
    });
    assert_eq!(items(&doc).len(), 2);
    assert!(doc.selection_inspect().is_none(), "a curve isn't measured");
    doc.look(Look::ClickSketch { item, add: true });
    assert!(matches!(items(&doc)[..], [Selected::Face { .. }]));
    doc.look(Look::ClickSketch { item, add: false });
    // Another model keeps it.
    thicken(&mut doc);
    answer(&mut doc, &requests);
    assert_eq!(doc.model_picking().unwrap().marked, [item]);
    // Hidden, it goes; shown again, it's not back.
    doc.update(Edit::ToggleFeatureVisible(sketch));
    doc.sync();
    assert!(items(&doc).is_empty());
    assert!(
        doc.model_picking()
            .unwrap()
            .sketches
            .iter()
            .all(|l| l.feature != sketch)
    );
    doc.update(Edit::ToggleFeatureVisible(sketch));
    doc.sync();
    doc.look(Look::ClickSketch { item, add: false });
    assert_eq!(items(&doc).len(), 1);
    // Deleted, it goes, and a hover of it is dropped.
    doc.update(Edit::RemoveFeature(sketch));
    doc.sync();
    answer(&mut doc, &requests);
    assert!(doc.editor.document().feature(sketch).is_none());
    assert!(items(&doc).is_empty());
    doc.look(Look::HoverSketch(Some(item)));
    assert_eq!(doc.pick.sketch_hover(), None);
}

/// A sketch's curve listed with the model where the button was held:
/// hovered from its row, ticked as it's selected, its tick adding it as
/// a click with Ctrl would, and another row chosen selecting that alone.
#[test]
fn a_sketch_s_curve_listed_with_the_model_is_hovered_ticked_and_chosen() {
    use varde_view::{OverlapItem, OverlapItems, Overlaps, Selected, SketchItem};

    let (mut doc, sketch, line, _requests) = example_and_a_line();
    let item = SketchItem { sketch, item: line };
    let face = on_top(&doc);
    let list = Overlaps {
        held: DVec2::ZERO,
        at: DVec2::ZERO,
        items: OverlapItems::Mixed(vec![OverlapItem::Sketch(item), OverlapItem::Model(face)]),
    };
    doc.look(Look::OpenOverlaps(list));
    assert!(doc.overlaps.is_some());
    doc.look(Look::HoverOverlap(Some(0)));
    assert_eq!(doc.pick.sketch_hover(), Some(item));
    doc.look(Look::HoverOverlap(Some(1)));
    assert_eq!(doc.pick.hover(), Some(face));
    assert_eq!(doc.pick.sketch_hover(), None);
    assert_eq!(doc.overlap_ticked(), Some(vec![false, false]));
    doc.look(Look::ToggleOverlap(0));
    assert!(doc.overlaps.is_some());
    let items = |doc: &Doc| doc.pick.selection.items().copied().collect::<Vec<_>>();
    assert_eq!(items(&doc), [Selected::SketchItem { sketch, item: line }]);
    assert_eq!(doc.overlap_ticked(), Some(vec![true, false]));
    doc.look(Look::ChooseOverlap {
        index: 1,
        add: false,
    });
    assert!(doc.overlaps.is_none());
    assert_eq!(
        doc.pick.selection.targets().collect::<Vec<_>>(),
        [face.target]
    );
    assert!(items(&doc).len() == 1);
}
