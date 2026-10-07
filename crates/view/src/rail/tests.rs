use iced::keyboard::{Key as KeyPress, Modifiers};

use super::*;
use crate::shortcut::{document_bindings, pressed};
use crate::{ConstraintSet, Edit};

fn key(c: &str) -> KeyPress {
    KeyPress::Character(c.into())
}

fn sketching() -> DocumentKeys {
    DocumentKeys {
        editable: true,
        sketching: true,
        ..DocumentKeys::default()
    }
}

#[test]
fn every_entry_has_a_letter_of_its_own_and_its_key_s_where_it_s_a_letter_alone() {
    for (sets, sketching) in [(&SKETCH[..], true), (&MODEL[..], false)] {
        let set_keys = &SET_KEYS[..sets.len()];
        for set in sets {
            let letters = letters(set.entries, sketching);
            assert_eq!(letters.len(), set.entries.len());
            for (i, (entry, letter)) in set.entries.iter().zip(&letters).enumerate() {
                let letter = letter.unwrap_or_else(|| panic!("{} has no letter", entry.label()));
                assert!(letter.is_ascii_lowercase(), "{letter:?}");
                assert!(
                    !letters[..i].contains(&Some(letter)),
                    "{letter} twice in {}",
                    set.name
                );
                // Another set opens from an open list.
                assert!(!set_keys.contains(&letter), "{letter} opens a set");
                let shortcut = entry.binding(DocumentKeys::default()).shortcut;
                if shortcut.is_plain() && !shortcut.is_none() {
                    assert_eq!(shortcut.letter_key(), Some(letter), "{}", entry.label());
                }
            }
        }
    }
    // Line is L, as it is everywhere; Parallel, Shift P elsewhere, is P;
    // Perpendicular and Equal, whose keys' letters open sets, N and U.
    let constraints: String = letters(SKETCH[2].entries, true)
        .into_iter()
        .flatten()
        .collect();
    assert_eq!(constraints, "kihvpntsucmyf");
    let draw: String = letters(SKETCH[0].entries, true)
        .into_iter()
        .flatten()
        .collect();
    assert_eq!(draw, "lbcagnp");
    let create: String = letters(MODEL[0].entries, false)
        .into_iter()
        .flatten()
        .collect();
    // Sweep and Loft, with no key, take the first free letter of their
    // names, as the mock's list has it.
    assert_eq!(create, "sxopl");
    let modify: String = letters(MODEL[1].entries, false)
        .into_iter()
        .flatten()
        .collect();
    // Fillet is F, Chamfer C; Shell, Draft, Scale, Offset face, Split
    // body and Parameters, with no key, take the first free letter of
    // their names.
    assert_eq!(modify, "fcsdabopm");
    // Move is M, Linear pattern P; Mirror, Circular pattern and Align,
    // with no key, take the first free letter of their names.
    let transform: String = letters(MODEL[2].entries, false)
        .into_iter()
        .flatten()
        .collect();
    assert_eq!(transform, "mipca");
}

#[test]
fn letters_fall_back_to_the_label_s_first_free_one() {
    // P is Point's own; Parallel takes A.
    let entries = [
        Entry::Tool(Tool::Point),
        Entry::Constraint(ConstraintKind::Parallel),
    ];
    assert_eq!(letters(&entries, true), [Some('p'), Some('a')]);
}

#[test]
fn the_sets_hold_every_tool_the_app_has_and_only_those() {
    let sketch: Vec<Entry> = SKETCH.iter().flat_map(|set| set.entries).copied().collect();
    for tool in Tool::ALL {
        assert_eq!(
            sketch.iter().filter(|&&e| e == Entry::Tool(tool)).count(),
            1,
            "{tool:?}"
        );
    }
    for kind in ConstraintKind::ALL {
        let on = sketch.contains(&Entry::Constraint(kind));
        assert_eq!(on, kind.shortcut().is_some(), "{kind:?}");
    }
    assert!(sketch.contains(&Entry::Constrain));
    let model: Vec<Entry> = MODEL.iter().flat_map(|set| set.entries).copied().collect();
    assert_eq!(
        model,
        [
            Entry::Sketch,
            Entry::Extrude,
            Entry::Revolve,
            Entry::Sweep,
            Entry::Loft,
            Entry::Fillet,
            Entry::Chamfer,
            Entry::Shell,
            Entry::Draft,
            Entry::Scale,
            Entry::Combine,
            Entry::OffsetFace,
            Entry::Split,
            Entry::Params,
            Entry::Move,
            Entry::Mirror,
            Entry::LinearPattern,
            Entry::CircularPattern,
            Entry::Align,
            Entry::Measure
        ]
    );
    // I is Measure's own, in the Inspect set.
    assert_eq!(letters(MODEL[3].entries, false), [Some('i')]);
    for set in SKETCH.iter().chain(&MODEL) {
        // No set without tools, and a card shows its first ones.
        assert!(!set.entries.is_empty(), "{}", set.name);
        assert!(set.icon.category().is_some(), "{}", set.name);
    }
    // Each set has a number key.
    assert!(rail_sets(true) <= 9 && rail_sets(false) <= 9);
}

#[test]
fn the_top_row_s_letters_open_the_mode_s_sets() {
    let none = Modifiers::empty();
    for keys in [sketching(), DocumentKeys::default()] {
        let sets = rail_sets(keys.sketching);
        for (i, letter) in SET_KEYS[..sets].iter().enumerate() {
            let open = |keys| pressed(document_bindings(keys), &key(&letter.to_string()), none);
            assert!(matches!(
                open(keys),
                Some(Message::Look(Look::Rail(RailLook::Toggle(set)))) if set == i
            ));
            // From another set's open list too.
            let other = DocumentKeys {
                rail: Some(RailOpen {
                    set: (i + 1) % sets,
                    row: 0,
                    held: true,
                }),
                ..keys
            };
            assert!(matches!(
                open(other),
                Some(Message::Look(Look::Rail(RailLook::Toggle(set)))) if set == i
            ));
        }
    }
    // Outside a sketch, with three sets, E opens the third and Extrude
    // is X.
    let keys = DocumentKeys {
        editable: true,
        ..DocumentKeys::default()
    };
    assert!(matches!(
        pressed(document_bindings(keys), &key("e"), none),
        Some(Message::Look(Look::Rail(RailLook::Toggle(2))))
    ));
    assert!(matches!(
        pressed(document_bindings(keys), &key("x"), none),
        Some(Message::Look(Look::StartExtrude))
    ));
}

#[test]
fn an_open_set_s_letters_come_before_the_document_s_keys() {
    let none = Modifiers::empty();
    // Closed, P takes up the Point tool.
    assert!(matches!(
        pressed(document_bindings(sketching()), &key("p"), none),
        Some(Message::Look(Look::SelectTool(Tool::Point)))
    ));
    // With Constraints open, P is Parallel, once it fits the selection.
    let open = DocumentKeys {
        rail: Some(RailOpen {
            set: 2,
            row: 0,
            held: true,
        }),
        constraints: [ConstraintKind::Parallel].into_iter().collect(),
        ..sketching()
    };
    assert!(matches!(
        pressed(document_bindings(open), &key("p"), none),
        Some(Message::Edit(Edit::ToggleConstraint(
            ConstraintKind::Parallel
        )))
    ));
    // Not fitting, P does nothing, rather than take up Point.
    let unfit = DocumentKeys {
        constraints: ConstraintSet::default(),
        ..open
    };
    assert!(pressed(document_bindings(unfit), &key("p"), none).is_none());
    // Letters the set doesn't have still do what they do elsewhere.
    assert!(matches!(
        pressed(document_bindings(open), &key("l"), none),
        Some(Message::Look(Look::SelectTool(Tool::Line)))
    ));
    // Shift P is Parallel's own key either way.
    assert!(matches!(
        pressed(document_bindings(open), &key("P"), Modifiers::SHIFT),
        Some(Message::Edit(Edit::ToggleConstraint(
            ConstraintKind::Parallel
        )))
    ));
}

#[test]
fn a_set_index_past_the_mode_s_sets_has_no_letters() {
    let keys = DocumentKeys {
        rail: Some(RailOpen {
            set: 4,
            row: 0,
            held: true,
        }),
        ..DocumentKeys::default()
    };
    assert!(letter_bindings(keys).is_empty());
    let keys = DocumentKeys {
        rail: Some(RailOpen {
            set: 0,
            row: 0,
            held: true,
        }),
        editable: true,
        ..DocumentKeys::default()
    };
    assert!(matches!(
        pressed(letter_bindings(keys), &key("x"), Modifiers::empty()),
        Some(Message::Look(Look::StartExtrude))
    ));
    // Revolve's letter is its own key, O.
    assert!(matches!(
        pressed(letter_bindings(keys), &key("o"), Modifiers::empty()),
        Some(Message::Look(Look::StartRevolve))
    ));
}

/// How tall the rail is with `shown` tools on each card.
fn rail_height(shown: &[usize]) -> f32 {
    shown.iter().map(|&n| card_height(n)).sum::<f32>() + GAP * (shown.len() - 1) as f32
}

#[test]
fn cards_show_as_many_tools_as_fit_shared_evenly() {
    // Three tools on a card, as the mock's, is 140 px.
    assert_eq!(card_height(3), 140.0);
    assert_eq!(card_height(0), 38.0);
    for room in (0..1200).step_by(7).map(|room| room as f32) {
        let shown = fitting(&SKETCH, room);
        let fits = rail_height(&shown) <= room;
        // Only heads, too many for the room, run past it.
        assert!(fits || shown.iter().all(|&n| n == 0), "{room}: {shown:?}");
        // Even: no set with more to show shows two fewer than another.
        let fewest = (0..SKETCH.len())
            .filter(|&i| shown[i] < SKETCH[i].entries.len())
            .map(|i| shown[i])
            .min();
        if let Some(fewest) = fewest {
            assert!(shown.iter().all(|&n| n <= fewest + 1), "{room}: {shown:?}");
            // And the next tool wouldn't have fitted.
            let next = (0..SKETCH.len())
                .find(|&i| shown[i] == fewest && shown[i] < SKETCH[i].entries.len())
                .unwrap();
            let mut more = shown.clone();
            more[next] += 1;
            assert!(rail_height(&more) > room, "{room}: {shown:?}");
        }
    }
    // Room for everything shows everything.
    let all: Vec<usize> = SKETCH.iter().map(|set| set.entries.len()).collect();
    assert_eq!(fitting(&SKETCH, 10_000.0), all);
    // The first sets get the odd ones out.
    let room = rail_height(&[2, 2, 1, 1]);
    assert_eq!(fitting(&SKETCH, room), [2, 2, 1, 1]);
    // A set out of tools leaves the room to the rest: Dimension has one.
    let room = rail_height(&[5, 5, 5, 1]);
    assert_eq!(fitting(&SKETCH, room), [5, 5, 5, 1]);
}

#[test]
fn the_arrows_move_along_the_open_list_and_enter_picks_its_row() {
    let none = Modifiers::empty();
    let up = KeyPress::Named(iced::keyboard::key::Named::ArrowUp);
    let down = KeyPress::Named(iced::keyboard::key::Named::ArrowDown);
    let enter = KeyPress::Named(iced::keyboard::key::Named::Enter);
    // Closed, the arrows do nothing.
    assert!(pressed(document_bindings(sketching()), &down, none).is_none());
    let draw = |row| DocumentKeys {
        rail: Some(RailOpen {
            set: 0,
            row,
            held: true,
        }),
        ..sketching()
    };
    assert!(matches!(
        pressed(document_bindings(draw(0)), &down, none),
        Some(Message::Look(Look::Rail(RailLook::Down)))
    ));
    assert!(matches!(
        pressed(document_bindings(draw(0)), &up, none),
        Some(Message::Look(Look::Rail(RailLook::Up)))
    ));
    // Enter picks the row the keys are on: Circle is Draw's third.
    assert!(matches!(
        pressed(document_bindings(draw(2)), &enter, none),
        Some(Message::Look(Look::SelectTool(Tool::Circle)))
    ));
    // On a row whose tool can't be used, Enter does nothing, rather than
    // edit the feature selected or commit an extrude.
    let read_only = DocumentKeys {
        editable: false,
        ..draw(2)
    };
    assert!(pressed(document_bindings(read_only), &enter, none).is_none());
    assert_eq!(Shortcut::DOWN.label(), "↓");
}
