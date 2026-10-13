use super::*;
use crate::mcp::TOKEN_HIDDEN;
use crate::mcp_config::{mcp_client_lines, mcp_stdio_lines};
use crate::{McpClient, McpStatus};

#[test]
fn nothing_changed_by_default() {
    assert!(changed_settings(ViewOptions::default()).is_empty());
}

/// Each setting not its default is listed, in the page's order, from
/// what it is to the default.
#[test]
fn changes_are_listed_from_what_they_are_to_the_defaults() {
    let options = ViewOptions {
        theme: ThemeChoice::Dark,
        names: true,
        shading: Shading::FlatMetal,
        length_unit: LengthUnit::In,
        ..ViewOptions::default()
    };
    let change = |name, from: &'static str, to: &'static str| SettingChange {
        name,
        from: from.into(),
        to: to.into(),
    };
    assert_eq!(
        changed_settings(options),
        [
            change("Theme", "Dark", "System"),
            change("Shading", "Flat metal", "Regular"),
            change("Names", "On", "Off"),
            change("Length unit", "in", "mm"),
        ]
    );
}

/// The newer settings are listed too, said as the page says them.
#[test]
fn mouse_units_and_files_changes_are_listed() {
    let options = ViewOptions {
        interface_scale: 135,
        angle_unit: AngleUnit::Rad,
        decimals: Some(0),
        mouse: crate::MouseOptions {
            left: DragAction::Nothing,
            rotation: Rotation::OverTheTop,
            rotation_point: RotationPoint::Pointer,
            pivot_click: PivotClick::ShiftMiddle,
            wheel_down_zooms_in: true,
            zoom_to_pointer: false,
            sketch_snap: false,
            ..crate::MouseOptions::default()
        },
        auto_save: 10,
        recent_designs: 25,
        mcp: true,
        mcp_port: 8123,
        ..ViewOptions::default()
    };
    let change = |name, from: &'static str, to: &'static str| SettingChange {
        name,
        from: from.into(),
        to: to.into(),
    };
    assert_eq!(
        changed_settings(options),
        [
            change("Interface scale", "135%", "100%"),
            change("Angle unit", "Radians", "Degrees"),
            change("Decimal places", "0", "Auto"),
            change("Left drag", "Nothing", "Orbit"),
            change("Rotation", "Turntable, over the top", "Turntable"),
            change("Rotation point", "Under the pointer", "Clicked"),
            change(
                "Set rotation point with",
                "Shift + middle click",
                "Middle click"
            ),
            change("Zoom direction", "Wheel down", "Wheel up"),
            change("Zoom to pointer", "Off", "On"),
            change("Snapping", "Off", "On"),
            change("Auto-save", "10 min", "1 min"),
            change("Recent designs", "25", "100"),
            change("MCP", "On", "Off"),
            change("MCP port", "8123", "7311"),
        ]
    );
}

/// Every tab shows, its rows laid out, and the Units tab's sample shows
/// values as the settings do; set rotation point shows only for a
/// clicked one.
#[test]
fn every_tab_shows_its_rows() {
    let texts = |options: ViewOptions, tab| {
        let state = SettingsState {
            options,
            tab,
            resetting: false,
            scale_dragged: None,
            kept_in: KeptIn::Browser,
            mcp: Default::default(),
        };
        let mut laid = crate::testing::Laid::new(page(state), iced::Size::new(1100.0, 900.0));
        laid.texts()
            .into_iter()
            .map(|shown| shown.text)
            .collect::<Vec<_>>()
    };
    let has = |texts: &[String], text: &str| texts.iter().any(|shown| shown == text);
    let defaults = ViewOptions::default();
    for (tab, rows) in [
        (
            SettingsTab::Appearance,
            &["Theme", "Interface scale", "100%"][..],
        ),
        (
            SettingsTab::Units,
            &["Length", "Angle", "Decimal places", "Radians", "Auto"],
        ),
        (
            SettingsTab::Mouse,
            &[
                "Left drag",
                "Middle drag",
                "Right drag",
                "Rotation",
                "Over the top",
                "Rotation point",
                "Set rotation point with",
                "Zoom direction",
                "Zoom to pointer",
                "Snapping",
            ],
        ),
        (
            SettingsTab::Files,
            &[
                "Auto-save",
                "Recent designs",
                "Off",
                "1 min",
                "5 min",
                "10 min",
                "100",
            ],
        ),
    ] {
        let shown = texts(defaults, tab);
        for row in rows {
            assert!(has(&shown, row), "{tab:?} {row}: {shown:?}");
        }
    }
    let note = |options| {
        texts(options, SettingsTab::Units)
            .into_iter()
            .find(|text| text.starts_with("Length for new designs"))
            .unwrap()
    };
    assert!(
        note(defaults).ends_with("Shown as 12.346 mm · 22.5°."),
        "{}",
        note(defaults)
    );
    let set = ViewOptions {
        angle_unit: AngleUnit::Rad,
        decimals: Some(1),
        length_unit: LengthUnit::Cm,
        ..defaults
    };
    assert!(
        note(set).ends_with("Shown as 1.2 cm · 0.4 rad."),
        "{}",
        note(set)
    );
    let centre = ViewOptions {
        mouse: crate::MouseOptions {
            rotation_point: RotationPoint::Centre,
            ..crate::MouseOptions::default()
        },
        ..defaults
    };
    assert!(!has(
        &texts(centre, SettingsTab::Mouse),
        "Set rotation point with"
    ));
}

/// Every scale offered is said as its percentage, and nothing else is.
#[test]
fn scales_are_said_as_their_percentages() {
    for percent in 0..=u16::MAX {
        let said = scale_said(percent);
        if crate::interface_scale_offered(percent) {
            assert_eq!(said, format!("{percent}%"));
        } else {
            assert_eq!(said, "Other", "{percent}");
        }
    }
    assert!(!crate::interface_scale_offered(70) && crate::interface_scale_offered(75));
    assert!(crate::interface_scale_offered(200) && !crate::interface_scale_offered(205));
    assert!(!crate::interface_scale_offered(112));
}

/// The scale's slider sends where it's dragged to, in steps of 5 from 75
/// to 200, and only on letting go asks for it to be set; dragged, the
/// label shows where it is, not the scale set.
#[test]
fn the_scale_slider_sets_only_when_let_go() {
    use iced::advanced::{Layout, Shell, clipboard};
    use iced::{Event, Point, Rectangle, Size, mouse};

    let window = Size::new(1100.0, 900.0);
    let state = |scale_dragged| SettingsState {
        options: ViewOptions::default(),
        tab: SettingsTab::Appearance,
        resetting: false,
        scale_dragged,
        kept_in: KeptIn::Browser,
        mcp: Default::default(),
    };
    let mut laid = crate::testing::Laid::new(page(state(None)), window);
    let shown = laid.texts();
    let label = shown
        .iter()
        .find(|shown| shown.text == "100%")
        .unwrap_or_else(|| panic!("no 100% in {shown:?}"));
    // The slider ends 10 px left of its label, 180 px wide.
    let (right, y) = (label.bounds.x - 12.0, label.bounds.center_y());
    let left = right - 176.0;
    let mut messages = Vec::new();
    let mut send = |laid: &mut crate::testing::Laid, event: Event, x: f32| {
        let mut shell = Shell::new(&mut messages);
        laid.element.as_widget_mut().update(
            &mut laid.tree,
            &event,
            Layout::new(&laid.node),
            mouse::Cursor::Available(Point::new(x, y)),
            &laid.renderer,
            &mut clipboard::Null,
            &mut shell,
            &Rectangle::with_size(window),
        );
    };
    let moved = |x| {
        Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(x, y),
        })
    };
    send(
        &mut laid,
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        left,
    );
    send(&mut laid, moved(right + 50.0), right + 50.0);
    send(&mut laid, moved((left + right) / 2.0), (left + right) / 2.0);
    send(
        &mut laid,
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        (left + right) / 2.0,
    );
    let scale = |message: &Message| match message {
        Message::Settings(SettingsPage::DragScale(percent)) => Some(*percent),
        _ => None,
    };
    let dragged: Vec<u16> = messages.iter().filter_map(scale).collect();
    assert_eq!(dragged.first(), Some(&75), "{messages:?}");
    assert_eq!(dragged.get(1), Some(&200), "{messages:?}");
    assert!(
        dragged
            .iter()
            .all(|&percent| crate::interface_scale_offered(percent)),
        "{dragged:?}"
    );
    assert!(
        matches!(
            messages.last(),
            Some(Message::Settings(SettingsPage::ReleaseScale))
        ),
        "{messages:?}"
    );
    assert_eq!(
        messages
            .iter()
            .filter(|message| matches!(message, Message::Settings(SettingsPage::ReleaseScale)))
            .count(),
        1
    );
    assert!(
        !messages
            .iter()
            .any(|message| matches!(message, Message::Settings(SettingsPage::Set(_))))
    );

    let mut dragged = crate::testing::Laid::new(page(state(Some(135))), window);
    let texts: Vec<String> = dragged
        .texts()
        .into_iter()
        .map(|shown| shown.text)
        .collect();
    assert!(texts.iter().any(|text| text == "135%"), "{texts:?}");
    assert!(!texts.iter().any(|text| text == "100%"), "{texts:?}");
}

/// Every row of every tab, by name: whether it's changed, as its reset
/// says, the default said and what it sends.
fn resets(options: ViewOptions) -> Vec<(&'static str, Option<(String, String)>)> {
    SettingsTab::ALL
        .into_iter()
        .flat_map(|tab| section(tab, options, None, &McpState::default()).2)
        .map(|row| {
            let reset = row
                .reset
                .map(|(said, message)| (said.into_owned(), format!("{message:?}")));
            (row.name, reset)
        })
        .collect()
}

/// With every setting its default, no row is marked changed nor has a
/// reset; with one changed, only its row does, its reset saying the
/// default as Reset to defaults' popup does and sending what choosing the
/// default on the page sends.
#[test]
fn only_a_changed_settings_row_has_a_reset() {
    let defaults = ViewOptions::default();
    for (name, reset) in resets(defaults) {
        assert!(reset.is_none(), "{name}");
    }
    let set = |setting| format!("{:?}", Message::Settings(SettingsPage::Set(setting)));
    let mouse = crate::MouseOptions::default();
    let cases: Vec<(ViewOptions, &str, &str, String)> = vec![
        (
            ViewOptions {
                theme: ThemeChoice::Dark,
                ..defaults
            },
            "Theme",
            "System",
            set(Setting::Theme(defaults.theme)),
        ),
        (
            ViewOptions {
                interface_scale: 150,
                ..defaults
            },
            "Interface scale",
            "100%",
            set(Setting::InterfaceScale(100)),
        ),
        (
            ViewOptions {
                projection: Projection::Perspective,
                ..defaults
            },
            "Projection",
            defaults.projection.said(),
            set(Setting::Projection(defaults.projection)),
        ),
        (
            ViewOptions {
                shading: Shading::Metal,
                ..defaults
            },
            "Shading",
            "Regular",
            set(Setting::Shading(Shading::Regular)),
        ),
        (
            ViewOptions {
                edges: Edges::Tessellation,
                ..defaults
            },
            "Edges",
            "Features",
            set(Setting::Edges(Edges::Default)),
        ),
        (
            ViewOptions {
                hidden_edges: !defaults.hidden_edges,
                ..defaults
            },
            "Hidden edges",
            on_off(defaults.hidden_edges),
            set(Setting::HiddenEdges(defaults.hidden_edges)),
        ),
        (
            ViewOptions {
                names: !defaults.names,
                ..defaults
            },
            "Names",
            on_off(defaults.names),
            set(Setting::Names(defaults.names)),
        ),
        (
            ViewOptions {
                mouse_hints: !defaults.mouse_hints,
                ..defaults
            },
            "Mouse hints",
            on_off(defaults.mouse_hints),
            set(Setting::MouseHints(defaults.mouse_hints)),
        ),
        (
            ViewOptions {
                length_unit: LengthUnit::Ft,
                ..defaults
            },
            "Length",
            "mm",
            format!(
                "{:?}",
                Message::Settings(SettingsPage::Set(Setting::LengthUnit(LengthUnit::Mm)))
            ),
        ),
        (
            ViewOptions {
                angle_unit: AngleUnit::Rad,
                ..defaults
            },
            "Angle",
            "Degrees",
            set(Setting::AngleUnit(AngleUnit::Deg)),
        ),
        (
            ViewOptions {
                decimals: Some(2),
                ..defaults
            },
            "Decimal places",
            "Auto",
            set(Setting::Decimals(None)),
        ),
        (
            ViewOptions {
                mouse: crate::MouseOptions {
                    left: DragAction::Zoom,
                    ..mouse
                },
                ..defaults
            },
            "Left drag",
            "Orbit",
            set(Setting::LeftDrag(DragAction::Orbit)),
        ),
        (
            ViewOptions {
                mouse: crate::MouseOptions {
                    middle: DragAction::Nothing,
                    ..mouse
                },
                ..defaults
            },
            "Middle drag",
            "Orbit",
            set(Setting::MiddleDrag(DragAction::Orbit)),
        ),
        (
            ViewOptions {
                mouse: crate::MouseOptions {
                    right: DragAction::Orbit,
                    ..mouse
                },
                ..defaults
            },
            "Right drag",
            "Pan",
            set(Setting::RightDrag(DragAction::Pan)),
        ),
        (
            ViewOptions {
                mouse: crate::MouseOptions {
                    rotation: Rotation::Trackball,
                    ..mouse
                },
                ..defaults
            },
            "Rotation",
            "Turntable",
            set(Setting::Rotation(Rotation::Turntable)),
        ),
        (
            ViewOptions {
                mouse: crate::MouseOptions {
                    rotation_point: RotationPoint::Centre,
                    ..mouse
                },
                ..defaults
            },
            "Rotation point",
            "Clicked",
            set(Setting::RotationPoint(RotationPoint::Clicked)),
        ),
        (
            ViewOptions {
                mouse: crate::MouseOptions {
                    pivot_click: PivotClick::ShiftMiddle,
                    ..mouse
                },
                ..defaults
            },
            "Set rotation point with",
            "Middle click",
            set(Setting::PivotClick(PivotClick::Middle)),
        ),
        (
            ViewOptions {
                mouse: crate::MouseOptions {
                    wheel_down_zooms_in: true,
                    ..mouse
                },
                ..defaults
            },
            "Zoom direction",
            "Wheel up",
            set(Setting::WheelDownZoomsIn(false)),
        ),
        (
            ViewOptions {
                mouse: crate::MouseOptions {
                    zoom_to_pointer: false,
                    ..mouse
                },
                ..defaults
            },
            "Zoom to pointer",
            "On",
            set(Setting::ZoomToPointer(true)),
        ),
        (
            ViewOptions {
                mouse: crate::MouseOptions {
                    sketch_snap: false,
                    ..mouse
                },
                ..defaults
            },
            "Snapping",
            "On",
            set(Setting::SketchSnap(true)),
        ),
        (
            ViewOptions {
                auto_save: 0,
                ..defaults
            },
            "Auto-save",
            "1 min",
            set(Setting::AutoSave(1)),
        ),
        (
            ViewOptions {
                recent_designs: 10,
                ..defaults
            },
            "Recent designs",
            "100",
            set(Setting::RecentDesigns(100)),
        ),
        (
            ViewOptions {
                mcp: true,
                ..defaults
            },
            "MCP",
            "Off",
            set(Setting::Mcp(false)),
        ),
        (
            ViewOptions {
                mcp_port: 8000,
                ..defaults
            },
            "Port",
            "7311",
            set(Setting::McpPort(7311)),
        ),
    ];
    let mut named = std::collections::HashSet::new();
    for (options, changed, said, message) in cases {
        named.insert(changed);
        // Said as Reset to defaults' popup says the default.
        assert!(
            changed_settings(options)
                .iter()
                .any(|change| change.to == said),
            "{changed}"
        );
        let rows = resets(options);
        assert!(rows.iter().any(|(name, _)| *name == changed), "{changed}");
        for (name, reset) in rows {
            if name == changed {
                assert_eq!(reset, Some((said.to_owned(), message.clone())), "{name}");
            } else {
                assert!(reset.is_none(), "{changed}: {name}");
            }
        }
    }
    // Every row is covered, but the token's, which isn't a setting.
    for (name, _) in resets(defaults) {
        assert!(named.contains(name) || name == "Token", "{name}");
    }
}

/// A changed row lays out with its reset left of its control, which
/// clicked sends what choosing the default does; the row's name and
/// control's line stay where they are unchanged, the bar taking the
/// padding's room and the row as high.
#[test]
fn a_changed_row_shows_its_reset() {
    use iced::advanced::{Shell, clipboard, layout::Layout, mouse};
    use iced::{Event, Point, Rectangle};
    let window = iced::Size::new(1100.0, 900.0);
    let laid = |angle_unit| {
        let state = SettingsState {
            options: ViewOptions {
                angle_unit,
                ..ViewOptions::default()
            },
            tab: SettingsTab::Units,
            resetting: false,
            scale_dragged: None,
            kept_in: KeptIn::Browser,
            mcp: Default::default(),
        };
        crate::testing::Laid::new(page(state), window)
    };
    let find = |shown: &[crate::probe::Shown], text: &str| {
        let found = shown.iter().find(|shown| shown.text == text);
        found
            .unwrap_or_else(|| panic!("no {text:?} in {shown:?}"))
            .bounds
    };
    let click = |laid: &mut crate::testing::Laid<'_>, at: Point| {
        let mut messages = Vec::new();
        for event in [mouse::Event::ButtonPressed, mouse::Event::ButtonReleased] {
            let mut shell = Shell::new(&mut messages);
            laid.element.as_widget_mut().update(
                &mut laid.tree,
                &Event::Mouse(event(mouse::Button::Left)),
                Layout::new(&laid.node),
                mouse::Cursor::Available(at),
                &laid.renderer,
                &mut clipboard::Null,
                &mut shell,
                &Rectangle::with_size(window),
            );
        }
        messages
    };
    let mut changed = laid(AngleUnit::Rad);
    let mut unchanged = laid(AngleUnit::Deg);
    let (shown, before) = (changed.texts(), unchanged.texts());
    // The chosen segment's label is set apart, so only rows are compared.
    assert_eq!(find(&shown, "Angle"), find(&before, "Angle"));
    assert_eq!(find(&shown, "Degrees").y, find(&before, "Degrees").y);
    // Left of the control, only the changed row has something to click.
    let (name, degrees) = (find(&shown, "Angle"), find(&shown, "Degrees"));
    let y = name.y + name.height / 2.0;
    let reset = (0..)
        .map(|step| name.x + name.width + step as f32)
        .take_while(|x| *x < degrees.x)
        .find(|x| !click(&mut changed, Point::new(*x, y)).is_empty())
        .expect("a reset left of the control");
    assert!(click(&mut unchanged, Point::new(reset, y)).is_empty());
    assert_eq!(
        format!("{:?}", click(&mut changed, Point::new(reset, y))),
        format!(
            "{:?}",
            [Message::Settings(SettingsPage::Set(Setting::AngleUnit(
                AngleUnit::Deg
            )))]
        ),
    );
}

/// The MCP tab's texts with `options` and `mcp`.
fn mcp_texts(options: ViewOptions, mcp: McpState<'_>) -> Vec<String> {
    let state = SettingsState {
        options,
        tab: SettingsTab::Mcp,
        resetting: false,
        scale_dragged: None,
        kept_in: KeptIn::Browser,
        mcp,
    };
    let mut laid = crate::testing::Laid::new(page(state), iced::Size::new(1100.0, 900.0));
    laid.texts().into_iter().map(|shown| shown.text).collect()
}

/// Off, the tab has the switch, port and token, the token hidden, and
/// the client lines with it hidden too; serving, the client and
/// Disconnect; shown, the token in the lines.
#[test]
fn the_mcp_tab_shows_its_rows() {
    let has = |texts: &[String], text: &str| texts.iter().any(|shown| shown == text);
    let token = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQ".to_owned();
    let off = mcp_texts(
        ViewOptions::default(),
        McpState {
            token: Some(token.clone()),
            ..McpState::default()
        },
    );
    // The port's field shows no text of its own here.
    for text in [
        "MCP",
        "Shows the MCP panel, where the server is started",
        "Port",
        "Token",
        "Regenerate",
        "Add to a client",
    ] {
        assert!(has(&off, text), "{text} in {off:?}");
    }
    assert!(!has(&off, "Client") && !has(&off, "Server"), "{off:?}");
    assert!(!off.iter().any(|shown| shown.contains(&token)), "{off:?}");
    let [claude, ..] = mcp_client_lines(7311, TOKEN_HIDDEN);
    assert!(has(&off, &claude), "{off:?}");

    // With the program's path, the stdio lines come first, and without
    // the token they're still there.
    let [stdio, ..] = mcp_stdio_lines("/opt/varde").unwrap();
    let both = mcp_texts(
        ViewOptions::default(),
        McpState {
            token: Some(token.clone()),
            exe: Some("/opt/varde"),
            ..McpState::default()
        },
    );
    let at = |text: &str| both.iter().position(|shown| shown == text);
    assert!(at(&stdio).unwrap() < at(&claude).unwrap(), "{both:?}");
    let tokenless = mcp_texts(
        ViewOptions::default(),
        McpState {
            exe: Some("/opt/varde"),
            ..McpState::default()
        },
    );
    assert!(
        has(&tokenless, "Add to a client") && has(&tokenless, &stdio),
        "{tokenless:?}"
    );
    // A path the lines can't hold: no relay lines, and the tab says why.
    let refused = mcp_texts(
        ViewOptions::default(),
        McpState {
            token: Some(token.clone()),
            exe: Some("/opt/a\nb/varde"),
            ..McpState::default()
        },
    );
    assert!(has(&refused, &claude), "{refused:?}");
    assert!(
        !refused
            .iter()
            .any(|shown| shown.contains("mcp add --scope user varde --"))
            && refused
                .iter()
                .any(|shown| shown.contains("no lines running it as a relay")),
        "{refused:?}"
    );
    assert!(!has(&tokenless, &claude), "{tokenless:?}");

    let on = ViewOptions {
        mcp: true,
        ..ViewOptions::default()
    };
    let serving = mcp_texts(
        on,
        McpState {
            status: McpStatus::Listening(7311),
            client: Some(McpClient {
                name: Some("claude-code"),
                since: "5 min ago".to_owned(),
            }),
            token: Some(token.clone()),
            token_shown: true,
            port_typed: None,
            exe: None,
        },
    );
    for text in [
        "Server",
        "Listening at 127.0.0.1:7311, one client at a time",
        "Client",
        "claude-code",
        "since 5 min ago",
        "Disconnect",
        token.as_str(),
    ] {
        assert!(has(&serving, text), "{text} in {serving:?}");
    }
    let [claude, ..] = mcp_client_lines(7311, &token);
    assert!(has(&serving, &claude), "{serving:?}");

    let waiting = mcp_texts(
        on,
        McpState {
            status: McpStatus::Listening(7311),
            ..McpState::default()
        },
    );
    assert!(
        has(&waiting, "None") && has(&waiting, "Not read yet"),
        "{waiting:?}"
    );
    assert!(!has(&waiting, "Add to a client"), "{waiting:?}");

    let stopped = mcp_texts(on, McpState::default());
    assert!(
        has(&stopped, "Stopped: Start it in the MCP panel"),
        "{stopped:?}"
    );
}

/// The port row says a port out of range, as typed, or one that
/// couldn't be bound.
#[test]
fn the_port_row_says_its_error() {
    let has = |texts: &[String], text: &str| texts.iter().any(|shown| shown == text);
    let typed = mcp_texts(
        ViewOptions::default(),
        McpState {
            port_typed: Some("80"),
            ..McpState::default()
        },
    );
    assert!(has(&typed, "A number from 1024 to 65535"), "{typed:?}");

    let on = ViewOptions {
        mcp: true,
        ..ViewOptions::default()
    };
    let failed = mcp_texts(
        on,
        McpState {
            status: McpStatus::BindFailed("Address already in use"),
            ..McpState::default()
        },
    );
    assert!(has(&failed, "Not started: see the port"), "{failed:?}");
    assert!(
        has(
            &failed,
            "Port 7311 can't be used: Address already in use. Choose another"
        ),
        "{failed:?}"
    );
    // Switched off, the failure is past.
    let off = mcp_texts(
        ViewOptions::default(),
        McpState {
            status: McpStatus::BindFailed("Address already in use"),
            ..McpState::default()
        },
    );
    assert!(has(&off, "Where it listens, on 127.0.0.1 only"), "{off:?}");
}

/// The MCP state's `Debug`, and so the settings page's, doesn't show the
/// token, shown on the tab or not.
#[test]
fn mcp_debug_hides_the_token() {
    let token = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG".to_owned();
    let state = McpState {
        token: Some(token.clone()),
        token_shown: true,
        ..McpState::default()
    };
    assert!(!format!("{state:?}").contains(&token));
}
