//! Renaming features, sketches and bodies in the side panel.

use varde_document::Named;

use super::*;

/// The name of `target` in `doc`'s document.
fn name(doc: &Doc, target: Named) -> String {
    doc.editor.document().name_of(target).unwrap().to_owned()
}

/// Renames `target` to `text` through the rename field, as `F2` or the
/// context menu opens it and `Enter` takes it.
fn rename(doc: &mut Doc, target: Named, text: &str) {
    doc.look(Look::StartRename(target));
    assert!(doc.take_rename_focus());
    doc.look(Look::RenameInput(text.to_owned()));
    doc.update(Edit::CommitRename);
}

/// A feature renamed in its row is undone and redone like any edit.
#[test]
fn a_feature_is_renamed_undoably() {
    let (mut doc, feature, _) = with_sketch();
    let target = Named::Feature(feature);
    let old = name(&doc, target);
    doc.look(Look::StartRename(target));
    // The field holds the name to start with.
    assert_eq!(doc.renaming.as_ref().unwrap().text, old);
    doc.look(Look::RenameInput("Outline".into()));
    doc.update(Edit::CommitRename);
    assert!(doc.renaming.is_none());
    assert_eq!(name(&doc, target), "Outline");
    assert_eq!(doc.take_toast(), None);
    doc.update(Edit::Undo);
    assert_eq!(name(&doc, target), old);
    doc.update(Edit::Redo);
    assert_eq!(name(&doc, target), "Outline");
}

/// A name another feature or body has gets a number, and a toast says
/// the name was taken.
#[test]
fn a_name_taken_is_numbered_and_said() {
    let (mut doc, _) = example();
    let body = Named::Body(doc.editor.document().bodies()[0].id);
    let sketch = doc.editor.document().features()[0].name.clone();
    rename(&mut doc, body, &sketch);
    assert_eq!(name(&doc, body), format!("{sketch} (1)"));
    let toast = doc.take_toast().expect("a toast");
    assert!(toast.contains(&sketch) && toast.contains("already exists"));
    assert_eq!(doc.take_toast(), None);
}

/// `Esc` closes the field renaming nothing; anything done but hovering
/// renames, an empty name nothing.
#[test]
fn escape_cancels_and_other_actions_rename() {
    let (mut doc, feature, _) = with_sketch();
    let target = Named::Feature(feature);
    let old = name(&doc, target);
    doc.look(Look::StartRename(target));
    doc.look(Look::RenameInput("Gone".into()));
    doc.look(Look::Escape);
    assert!(doc.renaming.is_none());
    assert_eq!(name(&doc, target), old);

    doc.look(Look::StartRename(target));
    doc.look(Look::RenameInput("Kept".into()));
    doc.look(Look::HoverFeature(Some(feature)));
    assert!(doc.renaming.is_some());
    doc.look(Look::ClearSelection);
    assert!(doc.renaming.is_none());
    assert_eq!(name(&doc, target), "Kept");

    rename(&mut doc, target, "   ");
    assert_eq!(name(&doc, target), "Kept");
}

/// `F2` renames the feature selected in the Timeline, and nothing is
/// renamed in a read-only document.
#[test]
fn f2_renames_the_feature_selected() {
    let (mut doc, feature, _) = with_sketch();
    doc.look(Look::SelectFeature(feature));
    let keys = doc.keys().unwrap();
    assert_eq!(keys.rename, Some(Named::Feature(feature)));
    let f2 = iced::keyboard::Key::Named(iced::keyboard::key::Named::F2);
    let sent = varde_view::pressed(
        varde_view::document_bindings(keys),
        &f2,
        iced::keyboard::Modifiers::empty(),
    );
    assert!(
        matches!(sent, Some(varde_view::Message::Look(Look::StartRename(Named::Feature(f)))) if f == feature),
        "{sent:?}"
    );

    doc.read_only = Some("read-only".into());
    doc.look(Look::StartRename(Named::Feature(feature)));
    assert!(doc.renaming.is_none());
}

/// The field opens in the row in place of its name and note (the
/// status bar still names the feature selected).
#[test]
fn the_rename_field_shows_in_the_row() {
    let (mut doc, feature, _) = with_sketch();
    doc.look(Look::SelectPanel(Panel::Timeline));
    let old = name(&doc, Named::Feature(feature));
    let count = |shown: &[String]| shown.iter().filter(|text| **text == old).count();
    let before = screen_texts(&doc);
    doc.look(Look::StartRename(Named::Feature(feature)));
    let after = screen_texts(&doc);
    // The row is drawn plain and hovered.
    assert_eq!(count(&after) + 2, count(&before), "{before:?} {after:?}");
}

/// Moving the cursor over the viewport, across the rail and the view
/// cube on the way, leaves the rename field open and focused.
#[test]
fn moving_over_the_viewport_keeps_the_field() {
    let (doc, _) = example();
    let feature = doc.editor.document().features()[0].id;
    let body = doc.editor.document().bodies()[0].id;
    move_over_viewport(Panel::Timeline, Named::Feature(feature));
    move_over_viewport(Panel::Objects, Named::Feature(feature));
    move_over_viewport(Panel::Objects, Named::Body(body));
}

/// Renames `target` from `panel` and moves the cursor over the viewport.
fn move_over_viewport(panel: Panel, target: Named) {
    use iced::mouse::{Cursor, Event};
    use iced_runtime::user_interface::UserInterface;
    let (mut doc, _) = example();
    doc.look(Look::SelectPanel(panel));
    doc.look(Look::StartRename(target));
    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();
    let mut cache = iced_runtime::user_interface::Cache::default();
    {
        use iced::advanced::widget::operation::focusable;
        let mut ui = UserInterface::build(doc.view_in(Mode::Light), size, cache, &mut renderer);
        ui.operate(&renderer, &mut focusable::focus(varde_view::RENAME_FIELD));
        cache = ui.into_cache();
    }
    for step in 0..80 {
        let at = iced::Point::new(
            300.0 + (step % 10) as f32 * 95.0,
            40.0 + (step / 10) as f32 * 90.0,
        );
        let mut ui = UserInterface::build(doc.view_in(Mode::Light), size, cache, &mut renderer);
        let mut sent = Vec::new();
        for event in [
            iced::Event::Mouse(Event::CursorMoved { position: at }),
            iced::Event::Window(iced::window::Event::RedrawRequested(Instant::now())),
        ] {
            let _ = ui.update(
                &[event],
                Cursor::Available(at),
                &mut renderer,
                &mut iced::advanced::clipboard::Null,
                &mut sent,
            );
        }
        // Typing still reaches the field.
        let key = iced::keyboard::Key::Character("x".into());
        let typed = iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key.clone(),
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: iced::keyboard::Location::Standard,
            modifiers: iced::keyboard::Modifiers::empty(),
            text: Some("x".into()),
            repeat: false,
        });
        let mut typing = Vec::new();
        let _ = ui.update(
            &[typed],
            Cursor::Available(at),
            &mut renderer,
            &mut iced::advanced::clipboard::Null,
            &mut typing,
        );
        assert!(
            matches!(typing[..], [Ui::Look(Look::RenameInput(_))]),
            "unfocused at {step}: {typing:?}"
        );
        cache = ui.into_cache();
        for message in sent {
            match message {
                Ui::Look(look) => doc.look(look),
                Ui::Edit(edit) => doc.update(edit),
                _ => {}
            }
            assert!(doc.renaming.is_some(), "closed at {step}");
        }
    }
}
