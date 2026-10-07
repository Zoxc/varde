use super::*;

fn params() -> Params {
    Params::evaluate(
        [
            ("width", "40 mm"),
            ("wall", "2 mm"),
            ("angle", "30 deg"),
            ("count", "4"),
            ("broken", "nope"),
            ("w", "1 mm"),
        ],
        LengthUnit::Mm,
    )
}

fn names(text: &str, params: &Params) -> Option<(usize, Vec<String>)> {
    let found = suggest(
        text,
        ParamsIn {
            params,
            units: LengthUnit::Mm,
        },
    )?;
    let names = found
        .names
        .iter()
        .map(|(name, shown)| format!("{name} {shown}"))
        .collect();
    Some((found.start, names))
}

#[test]
fn names_starting_with_the_last_word_are_offered_with_their_values() {
    let params = params();
    assert_eq!(
        names("2 * w", &params),
        Some((4, vec!["width 40 mm".into(), "wall 2 mm".into()]))
    );
    assert_eq!(names("an", &params), Some((0, vec!["angle 30°".into()])));
    assert_eq!(names("c", &params), Some((0, vec!["count 4".into()])));
    assert_eq!(names("b", &params), Some((0, vec!["broken error".into()])));
    // A whole name, a number with its unit, nothing typed: nothing.
    assert_eq!(names("width", &params), None);
    assert_eq!(names("10mm", &params), None);
    assert_eq!(names("10 ", &params), None);
    assert_eq!(names("", &params), None);
    assert_eq!(names("x", &params), None);
    assert_eq!(names("w", Params::EMPTY), None);
    // Where a unit goes, after a number or a `)`, names aren't offered;
    // after a name or an operator they are.
    assert_eq!(names("10 w", &params), None);
    assert_eq!(names("1.5 w", &params), None);
    assert_eq!(names("(2 + 3) w", &params), None);
    assert_eq!(names("(2 + 3)w", &params), None);
    assert!(names("10 * w", &params).is_some());
    assert!(names("(2 + w", &params).is_some());
    assert!(names("p2 w", &params).is_some());
}

#[test]
fn the_list_is_bounded_and_takes_a_name_in_place_of_the_word() {
    let many: Vec<(String, String)> = (0..100)
        .map(|i| (format!("p{i}"), "1 mm".to_owned()))
        .collect();
    let params = Params::evaluate(
        many.iter().map(|(n, t)| (n.as_str(), t.as_str())),
        LengthUnit::Mm,
    );
    let (_, found) = names("p", &params).unwrap();
    assert_eq!(found.len(), MAX_SUGGESTIONS);
    assert_eq!(take("2 * wi", 4, "width"), "2 * width");
    // A start past the text keeps it whole.
    assert_eq!(take("é", 1, "width"), "éwidth");
}

mod widget {
    use iced::advanced::clipboard;
    use iced::advanced::widget::operation::focusable;
    use iced::keyboard::Modifiers;
    use iced::widget::text_input;

    use super::super::*;
    use super::params;
    use crate::Look;
    use crate::testing::Laid;

    const WINDOW: Size = Size::new(300.0, 300.0);

    fn on_input(text: String) -> Message {
        Message::Look(Look::ValueInput(text))
    }

    /// A field showing `typed`, offering `params`' names, laid out, with
    /// the focus if `focused`.
    fn field<'a>(typed: &'a str, params: &'a Params, focused: bool) -> Laid<'a> {
        let id = iced::widget::Id::new("suggest-test");
        let input = text_input("", typed).id(id.clone()).on_input(on_input);
        let params = ParamsIn {
            params,
            units: LengthUnit::Mm,
        };
        let element = suggesting(input, typed, params, Some(&on_input));
        let mut laid = Laid::new(element, WINDOW);
        if focused {
            let mut focus = focusable::focus::<()>(id);
            laid.element.as_widget_mut().operate(
                &mut laid.tree,
                Layout::new(&laid.node),
                &laid.renderer,
                &mut focus,
            );
        }
        laid
    }

    fn tab() -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(Named::Tab),
            modified_key: keyboard::Key::Named(Named::Tab),
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers: Modifiers::empty(),
            text: None,
            repeat: false,
        })
    }

    fn update(laid: &mut Laid<'_>, event: Event) -> (Vec<Message>, bool) {
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        laid.element.as_widget_mut().update(
            &mut laid.tree,
            &event,
            Layout::new(&laid.node),
            mouse::Cursor::Unavailable,
            &laid.renderer,
            &mut clipboard::Null,
            &mut shell,
            &Rectangle::with_size(WINDOW),
        );
        let captured = shell.is_event_captured();
        (messages, captured)
    }

    fn has_overlay(laid: &mut Laid<'_>) -> bool {
        laid.element
            .as_widget_mut()
            .overlay(
                &mut laid.tree,
                Layout::new(&laid.node),
                &laid.renderer,
                &Rectangle::with_size(WINDOW),
                Vector::ZERO,
            )
            .is_some()
    }

    #[test]
    fn tab_puts_the_first_name_in_while_the_field_has_the_focus() {
        let params = params();
        let mut laid = field("2 * wa", &params, true);
        assert!(has_overlay(&mut laid));
        let (sent, captured) = update(&mut laid, tab());
        assert!(captured);
        assert!(
            matches!(sent.as_slice(), [Message::Look(Look::ValueInput(text))] if text == "2 * wall"),
            "{sent:?}"
        );

        // Without the focus, nothing shows and Tab is the app's.
        let mut laid = field("2 * wa", &params, false);
        assert!(!has_overlay(&mut laid));
        let (sent, captured) = update(&mut laid, tab());
        assert!(sent.is_empty() && !captured);

        // Nor with nothing to offer.
        let mut laid = field("2 * x", &params, true);
        assert!(!has_overlay(&mut laid));
        let (sent, captured) = update(&mut laid, tab());
        assert!(sent.is_empty() && !captured);
    }

    fn focused(laid: &Laid<'_>) -> bool {
        let state: &text_input::State<
            <iced::Renderer as iced::advanced::text::Renderer>::Paragraph,
        > = laid.tree.children[0].state.downcast_ref();
        state.is_focused()
    }

    #[test]
    fn the_field_keeps_its_focus_as_names_come_and_go() {
        let params = params();
        let mut laid = field("2 * x", &params, true);
        assert!(focused(&laid) && !has_overlay(&mut laid));
        let id = iced::widget::Id::new("suggest-test");
        let params_in = ParamsIn {
            params: &params,
            units: LengthUnit::Mm,
        };
        // Typing on: names appear, then go again.
        for typed in ["2 * x + w", "2 * x + wa", "2 * x + wax"] {
            let input = text_input("", typed).id(id.clone()).on_input(on_input);
            laid.replace(suggesting(input, typed, params_in, Some(&on_input)), WINDOW);
            assert!(focused(&laid), "{typed}");
            assert_eq!(has_overlay(&mut laid), typed != "2 * x + wax", "{typed}");
        }
    }

    #[test]
    fn a_click_on_a_name_puts_it_in_and_keeps_the_focus() {
        let params = params();
        let mut laid = field("w", &params, true);
        let mut overlay = laid
            .element
            .as_widget_mut()
            .overlay(
                &mut laid.tree,
                Layout::new(&laid.node),
                &laid.renderer,
                &Rectangle::with_size(WINDOW),
                Vector::ZERO,
            )
            .unwrap();
        let overlay = overlay.as_overlay_mut();
        let node = overlay.layout(&laid.renderer, WINDOW);
        let layout = Layout::new(&node);
        let list = layout.children().next().unwrap().bounds();
        // The second row: "wall", under "width".
        let at = Point::new(list.x + 20.0, list.y + 3.0 + 26.0 + 10.0);
        let mut sent = Vec::new();
        for event in [
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ] {
            let mut shell = Shell::new(&mut sent);
            overlay.update(
                &event,
                layout,
                mouse::Cursor::Available(at),
                &laid.renderer,
                &mut clipboard::Null,
                &mut shell,
            );
            if matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_))) {
                assert!(shell.is_event_captured());
            }
        }
        assert!(
            matches!(sent.as_slice(), [Message::Look(Look::ValueInput(text))] if text == "wall"),
            "{sent:?}"
        );
    }
}
