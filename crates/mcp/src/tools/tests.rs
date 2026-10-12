use super::*;

fn call(name: &str, arguments: Value) -> Result<Call, CallError> {
    parse_call(Some(&json!({ "name": name, "arguments": arguments })))
}

fn refused(name: &str, arguments: Value) -> String {
    match call(name, arguments) {
        Err(error @ CallError::Arguments { .. }) => error.to_string(),
        other => panic!("expected an argument error, got {other:?}"),
    }
}

fn topology(arguments: Value) -> Topology {
    match call("body_topology", arguments) {
        Ok(Call::BodyTopology(t)) => t,
        other => panic!("expected body_topology, got {other:?}"),
    }
}

#[test]
fn lists_the_tools_built() {
    let list = list();
    let tools = list["tools"].as_array().unwrap();
    let names: Vec<_> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "design_summary",
            "feature",
            "body_topology",
            "sketch",
            "measure",
            "render",
            "rename",
            "set_visible",
            "set_color",
            "set_opacity",
            "remove_feature",
            "remove_body",
            "add_parameter",
            "set_parameter",
            "rename_parameter",
            "remove_parameter",
            "set_units",
            "set_tolerance",
            "set_rollback",
            "add_sketch",
            "set_sketch_plane",
            "add_link",
            "edit_sketch",
            "set_feature",
            "extrude",
            "revolve",
            "combine",
            "move_body",
            "mirror",
            "pattern",
            "align",
            "scale",
            "split",
            "shell",
            "offset_face",
            "draft",
            "chamfer",
            "fillet",
            "sweep",
            "loft",
            "undo",
        ]
    );
    let queries = [
        "design_summary",
        "feature",
        "body_topology",
        "sketch",
        "measure",
        "render",
    ];
    let removals = [
        "remove_feature",
        "remove_body",
        "remove_parameter",
        "set_feature",
        "undo",
    ];
    for tool in tools {
        assert_eq!(tool["inputSchema"]["type"], "object");
        let name = tool["name"].as_str().unwrap();
        let edits = !queries.contains(&name);
        assert_eq!(tool["annotations"]["readOnlyHint"], !edits);
        if edits {
            let destructive = removals.contains(&name);
            assert_eq!(
                tool["annotations"]["destructiveHint"], destructive,
                "{name}"
            );
        }
    }
    // Every tool listed is read: `{}` is refused for its arguments, never
    // as no such tool.
    // A shell needs nothing: the model's only body, closed; nor does a
    // render: every visible body from the iso view.
    for name in &names[1..] {
        let empty = call(name, json!({}));
        if *name == "shell" {
            assert!(matches!(empty, Ok(Call::Shell(_))), "{empty:?}");
        } else if *name == "render" {
            assert!(matches!(empty, Ok(Call::Render(_))), "{empty:?}");
        } else if *name == "undo" {
            assert_eq!(empty, Ok(Call::Undo { steps: 1 }));
        } else {
            assert!(matches!(empty, Err(CallError::Arguments { .. })), "{name}");
        }
    }
    assert!(tools[0]["outputSchema"].is_object());
    assert!(tools[1].get("outputSchema").is_none());
    assert!(tools[2]["outputSchema"].is_object());
}

#[test]
fn reads_each_call() {
    assert_eq!(
        call("design_summary", json!({})),
        Ok(Call::DesignSummary(Page::SUMMARY))
    );
    assert_eq!(
        parse_call(Some(&json!({ "name": "design_summary" }))),
        Ok(Call::DesignSummary(Page::SUMMARY))
    );
    assert_eq!(
        call("feature", json!({ "name": "Extrude 1" })),
        Ok(Call::Feature {
            name: "Extrude 1".into(),
            page: Page::FEATURE,
        })
    );
    assert_eq!(
        call(
            "feature",
            json!({ "name": "Chamfer 1", "offset": 50, "limit": 9999 })
        ),
        Ok(Call::Feature {
            name: "Chamfer 1".into(),
            page: Page {
                offset: 50,
                limit: MAX_LIMIT,
            },
        })
    );
    assert_eq!(
        topology(json!({ "body": "Body 1" })),
        Topology {
            body: "Body 1".into(),
            faces: None,
            edges: None,
            offset: 0,
            limit: LIMIT,
        }
    );
}

#[test]
fn unknown_tools_and_bad_params_are_protocol_errors() {
    assert!(matches!(call("nope", json!({})), Err(CallError::Params(_))));
    assert!(matches!(parse_call(None), Err(CallError::Params(_))));
    assert!(matches!(
        parse_call(Some(&json!({ "arguments": {} }))),
        Err(CallError::Params(_))
    ));
    assert!(matches!(
        parse_call(Some(&json!({ "name": "feature", "arguments": [1] }))),
        Err(CallError::Params(_))
    ));
}

#[test]
fn names_are_checked() {
    refused("feature", json!({}));
    refused("feature", json!({ "name": "" }));
    refused("feature", json!({ "name": 3 }));
    let long = "a".repeat(NAME_BYTES + 1);
    assert!(refused("feature", json!({ "name": long })).contains("longer"));
    let longest = "a".repeat(NAME_BYTES);
    assert!(call("feature", json!({ "name": longest })).is_ok());
}

#[test]
fn unknown_arguments_are_refused() {
    assert!(refused("design_summary", json!({ "x": 1 })).contains("\"x\""));
    refused("body_topology", json!({ "body": "B", "face": {} }));
    refused(
        "body_topology",
        json!({ "body": "B", "faces": { "norml": "z" } }),
    );
}

#[test]
fn echoed_names_are_bounded() {
    let long = "é".repeat(100);
    let Err(CallError::Params(message)) = call(&long, json!({})) else {
        panic!("expected a params error");
    };
    assert!(message.len() < 100);
    assert!(refused("design_summary", json!({ long.clone(): 1 })).len() < 120);
}

#[test]
fn reads_directions() {
    let normal = |n: Value| {
        topology(json!({ "body": "B", "faces": { "normal": n } }))
            .faces
            .unwrap()
            .normal
            .unwrap()
    };
    assert_eq!(
        normal(json!("+z")),
        Direction {
            vector: [0.0, 0.0, 1.0],
            signed: true
        }
    );
    assert_eq!(
        normal(json!("-X")),
        Direction {
            vector: [-1.0, 0.0, 0.0],
            signed: true
        }
    );
    assert_eq!(
        normal(json!("y")),
        Direction {
            vector: [0.0, 1.0, 0.0],
            signed: false
        }
    );
    let d = normal(json!([3.0, 0.0, 4.0]));
    assert!(d.signed);
    assert!((d.vector[0] - 0.6).abs() < 1e-12 && (d.vector[2] - 0.8).abs() < 1e-12);
    // Huge and tiny components normalize without overflow or underflow.
    let d = normal(json!([1e308, 1e308, 0.0]));
    assert!((d.vector[0] - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
    let d = normal(json!([0.0, 5e-324, 0.0]));
    assert_eq!(d.vector, [0.0, 1.0, 0.0]);
}

#[test]
fn bad_directions_are_refused() {
    for bad in [
        json!("w"),
        json!("+"),
        json!("++z"),
        json!(""),
        json!([0, 0, 0]),
        json!([1, 2]),
        json!([1, 2, 3, 4]),
        json!([1, "2", 3]),
        json!(true),
    ] {
        refused(
            "body_topology",
            json!({ "body": "B", "edges": { "direction": bad } }),
        );
    }
}

#[test]
fn reads_edge_filters() {
    let t = topology(json!({ "body": "B", "edges": { "direction": "z", "convex": true } }));
    let edges = t.edges.unwrap();
    assert_eq!(edges.convex, Some(true));
    assert_eq!(edges.direction.unwrap().vector, [0.0, 0.0, 1.0]);
    refused(
        "body_topology",
        json!({ "body": "B", "edges": { "convex": 1 } }),
    );
    refused("body_topology", json!({ "body": "B", "edges": "z" }));
}

#[test]
fn offset_and_limit_are_checked() {
    let t = topology(json!({ "body": "B", "offset": 7, "limit": 20 }));
    assert_eq!((t.offset, t.limit), (7, 20));
    assert_eq!(
        topology(json!({ "body": "B", "limit": 1_000_000 })).limit,
        MAX_LIMIT
    );
    assert_eq!(
        topology(json!({ "body": "B", "offset": u64::MAX })).offset,
        usize::try_from(u64::MAX).unwrap()
    );
    for bad in [json!(-1), json!(1.5), json!("3"), json!(1e30)] {
        refused("body_topology", json!({ "body": "B", "offset": bad }));
    }
    refused("body_topology", json!({ "body": "B", "limit": 0 }));
}

#[test]
fn replies_carry_text_and_structure() {
    let reply = ToolReply::data(json!({ "a": 1 }));
    let result = reply.result(ToolReply::CAP);
    assert_eq!(result["structuredContent"], json!({ "a": 1 }));
    assert_eq!(result["content"][0]["type"], "text");
    assert_eq!(result["content"][0]["text"], "{\"a\":1}");
    assert_eq!(result["isError"], false);

    let result = ToolReply::error("no").result(ToolReply::CAP);
    assert_eq!(result["isError"], true);
    assert!(result.get("structuredContent").is_none());
    assert_eq!(result["content"][0]["text"], "no");
}

#[test]
fn long_replies_are_cut_and_say_so() {
    // 'é' is two bytes: a cap of 5 falls inside the third.
    let result = ToolReply::text("éééééé").result(5);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("éé\n[Cut: this reply was 12 bytes"));
    assert_eq!(result["isError"], false);

    let result = ToolReply::data(json!({ "long": "x".repeat(100) })).result(10);
    assert!(result.get("structuredContent").is_none());
    assert_eq!(result["isError"], true);
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("[Cut")
    );

    // At the cap exactly: whole.
    let result = ToolReply::text("abc").result(3);
    assert_eq!(result["content"][0]["text"], "abc");
}

#[test]
fn reads_sketch() {
    assert_eq!(
        call("sketch", json!({ "name": "Sketch 1" })),
        Ok(Call::Sketch(SketchQuery {
            name: "Sketch 1".to_owned(),
            page: Page {
                offset: 0,
                limit: SKETCH_LIMIT
            }
        }))
    );
    assert_eq!(
        call(
            "sketch",
            json!({ "name": "S", "offset": 3, "limit": 10_000 })
        ),
        Ok(Call::Sketch(SketchQuery {
            name: "S".to_owned(),
            page: Page {
                offset: 3,
                limit: MAX_LIMIT
            }
        }))
    );
    assert!(!call("sketch", json!({ "name": "S" })).unwrap().edits());
    assert!(refused("sketch", json!({})).contains("name is required"));
    assert!(refused("sketch", json!({ "name": "" })).contains("empty"));
    let long = "x".repeat(NAME_BYTES + 1);
    assert!(refused("sketch", json!({ "name": long })).contains("longer"));
    assert!(refused("sketch", json!({ "name": "S", "limit": 0 })).contains("at least 1"));
    assert!(refused("sketch", json!({ "name": "S", "plane": "xy" })).contains("\"plane\""));
}

#[test]
fn design_summary_is_paged() {
    assert_eq!(
        call("design_summary", json!({ "offset": 30, "limit": 10_000 })),
        Ok(Call::DesignSummary(Page {
            offset: 30,
            limit: MAX_LIMIT
        }))
    );
    assert!(refused("design_summary", json!({ "limit": 0 })).contains("at least 1"));
    assert!(refused("design_summary", json!({ "offset": -1 })).contains("whole number"));
}

#[test]
fn reads_rename() {
    assert_eq!(
        call("rename", json!({ "feature": "Extrude 1", "to": "Base" })),
        Ok(Call::Rename(Rename {
            target: Target::Feature("Extrude 1".into()),
            to: "Base".into(),
        }))
    );
    assert_eq!(
        call("rename", json!({ "body": "Body 1", "to": "Plate" })),
        Ok(Call::Rename(Rename {
            target: Target::Body("Body 1".into()),
            to: "Plate".into(),
        }))
    );
    assert!(
        call("rename", json!({ "body": "Body 1", "to": "x" }))
            .unwrap()
            .edits()
    );
    for arguments in [
        json!({ "to": "Plate" }),
        json!({ "feature": "Sketch 1", "body": "Body 1", "to": "Plate" }),
        json!({ "body": "Body 1" }),
        json!({ "body": "Body 1", "to": " " }),
        json!({ "body": "Body 1", "to": "x".repeat(NAME_BYTES + 1) }),
        json!({ "body": "Body 1", "to": "Plate", "as": "y" }),
    ] {
        refused("rename", arguments);
    }
}

#[test]
fn reads_the_body_edits() {
    assert_eq!(
        call("set_visible", json!({ "body": "Body 1", "visible": false })),
        Ok(Call::SetVisible {
            body: "Body 1".into(),
            visible: false,
        })
    );
    assert_eq!(
        call(
            "set_color",
            json!({ "body": "Body 1", "color": { "hue": 200, "saturation": 0.2 } })
        ),
        Ok(Call::SetColor {
            body: "Body 1".into(),
            color: Some(Color {
                hue: 200.0,
                saturation: 0.2,
            }),
        })
    );
    assert_eq!(
        call("set_color", json!({ "body": "Body 1", "color": null })),
        Ok(Call::SetColor {
            body: "Body 1".into(),
            color: None,
        })
    );
    assert_eq!(
        call("set_opacity", json!({ "body": "Body 1", "opacity": 0.5 })),
        Ok(Call::SetOpacity {
            body: "Body 1".into(),
            opacity: 0.5,
        })
    );
    for (tool, arguments) in [
        ("set_visible", json!({ "body": "Body 1" })),
        ("set_visible", json!({ "body": "Body 1", "visible": 1 })),
        ("set_color", json!({ "body": "Body 1" })),
        ("set_color", json!({ "body": "Body 1", "color": "red" })),
        (
            "set_color",
            json!({ "body": "Body 1", "color": { "hue": 360, "saturation": 0 } }),
        ),
        (
            "set_color",
            json!({ "body": "Body 1", "color": { "hue": -1, "saturation": 0 } }),
        ),
        (
            "set_color",
            json!({ "body": "Body 1", "color": { "hue": 0, "saturation": 0.31 } }),
        ),
        (
            "set_color",
            json!({ "body": "Body 1", "color": { "hue": 0 } }),
        ),
        (
            "set_color",
            json!({ "body": "Body 1", "color": { "hue": 0, "saturation": 0, "x": 1 } }),
        ),
        ("set_opacity", json!({ "body": "Body 1", "opacity": 0.09 })),
        ("set_opacity", json!({ "body": "Body 1", "opacity": 1.01 })),
        ("set_opacity", json!({ "body": "Body 1", "opacity": "1" })),
        ("set_opacity", json!({ "body": "Body 1", "opacity": 1e300 })),
        ("remove_feature", json!({ "body": "Body 1" })),
        ("remove_body", json!({ "body": "" })),
    ] {
        refused(tool, arguments);
    }
}

#[test]
fn reads_the_parameter_and_design_edits() {
    assert_eq!(
        call("add_parameter", json!({ "name": "w", "value": "5 mm" })),
        Ok(Call::AddParameter {
            name: "w".into(),
            value: "5 mm".into(),
        })
    );
    assert_eq!(
        call("set_parameter", json!({ "name": "w", "value": "6 mm" })),
        Ok(Call::SetParameter {
            name: "w".into(),
            value: "6 mm".into(),
        })
    );
    assert_eq!(
        call("rename_parameter", json!({ "name": "w", "to": "width" })),
        Ok(Call::RenameParameter {
            name: "w".into(),
            to: "width".into(),
        })
    );
    assert_eq!(
        call("set_units", json!({ "units": "in" })),
        Ok(Call::SetUnits(Units::In))
    );
    assert_eq!(
        call("set_rollback", json!({ "before": null })),
        Ok(Call::SetRollback { before: None })
    );
    assert_eq!(
        call("set_rollback", json!({ "before": "Extrude 1" })),
        Ok(Call::SetRollback {
            before: Some("Extrude 1".into())
        })
    );
    for (tool, arguments) in [
        ("add_parameter", json!({ "name": "w" })),
        (
            "add_parameter",
            json!({ "name": "w", "value": "1".repeat(TEXT_BYTES + 1) }),
        ),
        ("set_parameter", json!({ "name": "w", "value": 5 })),
        ("rename_parameter", json!({ "name": "w" })),
        ("remove_parameter", json!({ "name": "w", "value": "1" })),
        ("set_units", json!({ "units": "cm" })),
        ("set_units", json!({ "units": 1 })),
        ("set_tolerance", json!({ "tolerance": "" })),
        (
            "set_tolerance",
            json!({ "tolerance": "1".repeat(TEXT_BYTES + 1) }),
        ),
        ("set_rollback", json!({})),
        ("set_rollback", json!({ "before": 3 })),
    ] {
        refused(tool, arguments);
    }
}

#[test]
fn reads_the_feature_tools() {
    assert_eq!(
        call(
            "extrude",
            json!({ "sketch": "Sketch 1", "profiles": "all" })
        ),
        Ok(Call::Extrude(Extrude {
            naming: Default::default(),
            sketch: "Sketch 1".into(),
            profiles: Profiles::All,
            extent: ExtrudeExtent::OneSide,
            distance: None,
            second_distance: None,
            flip: false,
            taper: None,
            operation: Operation::NewBody,
            exclude: vec![],
        }))
    );
    assert_eq!(
        call(
            "extrude",
            json!({
                "sketch": "Sketch 1", "profiles": [1, ["5 mm", -2.5]], "extent": "two_sides",
                "distance": "5", "second_distance": "w / 2", "flip": true, "taper": "2 deg",
                "operation": "cut", "exclude": ["Body 2"]
            })
        ),
        Ok(Call::Extrude(Extrude {
            naming: Default::default(),
            sketch: "Sketch 1".into(),
            profiles: Profiles::Picked(vec![
                Profile::Index(1),
                Profile::Point(["5 mm".into(), "-2.5".into()]),
            ]),
            extent: ExtrudeExtent::TwoSides,
            distance: Some("5".into()),
            second_distance: Some("w / 2".into()),
            flip: true,
            taper: Some("2 deg".into()),
            operation: Operation::Cut,
            exclude: vec!["Body 2".into()],
        }))
    );
    assert_eq!(
        call(
            "revolve",
            json!({ "sketch": "S", "profiles": [0], "axis": { "line": 7 }, "extent": "one_side",
                    "angle": "90 deg" })
        ),
        Ok(Call::Revolve(Revolve {
            naming: Default::default(),
            sketch: "S".into(),
            profiles: Profiles::Picked(vec![Profile::Index(0)]),
            axis: Axis::Line(7),
            extent: RevolveExtent::OneSide,
            angle: Some("90 deg".into()),
            second_angle: None,
            flip: false,
            operation: Operation::NewBody,
            exclude: vec![],
        }))
    );
    for (axis, read) in [
        (json!("x"), Axis::SketchX),
        (json!("Y"), Axis::SketchY),
        (json!({ "edge": "abc" }), Axis::Edge("abc".into())),
    ] {
        let Ok(Call::Revolve(revolve)) = call(
            "revolve",
            json!({ "sketch": "S", "profiles": "all", "axis": axis }),
        ) else {
            panic!();
        };
        assert_eq!(revolve.axis, read);
    }
    assert_eq!(
        call(
            "combine",
            json!({ "target": "Body 1", "tools": ["Body 2"], "operation": "subtract",
                    "keep_tools": true })
        ),
        Ok(Call::Combine(Combine {
            naming: Default::default(),
            target: "Body 1".into(),
            tools: vec!["Body 2".into()],
            operation: BodyOp::Subtract,
            keep_tools: true,
        }))
    );
    let many: Vec<usize> = (0..=MAX_ITEMS).collect();
    for (tool, arguments) in [
        ("extrude", json!({ "sketch": "S" })),
        ("extrude", json!({ "sketch": "S", "profiles": [] })),
        ("extrude", json!({ "sketch": "S", "profiles": "some" })),
        ("extrude", json!({ "sketch": "S", "profiles": [-1] })),
        ("extrude", json!({ "sketch": "S", "profiles": [1.5] })),
        ("extrude", json!({ "sketch": "S", "profiles": [[1]] })),
        ("extrude", json!({ "sketch": "S", "profiles": [["", "1"]] })),
        ("extrude", json!({ "sketch": "S", "profiles": many })),
        (
            "extrude",
            json!({ "sketch": "S", "profiles": "all", "extent": "far" }),
        ),
        (
            "extrude",
            json!({ "sketch": "S", "profiles": "all", "operation": "glue" }),
        ),
        (
            "extrude",
            json!({ "sketch": "S", "profiles": "all", "distance": 5 }),
        ),
        (
            "extrude",
            json!({ "sketch": "S", "profiles": "all", "distance": "1".repeat(TEXT_BYTES + 1) }),
        ),
        (
            "extrude",
            json!({ "sketch": "S", "profiles": "all", "exclude": "Body 1" }),
        ),
        (
            "extrude",
            json!({ "sketch": "S", "profiles": "all", "depth": "5" }),
        ),
        ("revolve", json!({ "sketch": "S", "profiles": "all" })),
        (
            "revolve",
            json!({ "sketch": "S", "profiles": "all", "axis": "z" }),
        ),
        (
            "revolve",
            json!({ "sketch": "S", "profiles": "all", "axis": { "line": -1 } }),
        ),
        (
            "revolve",
            json!({ "sketch": "S", "profiles": "all", "axis": { "line": 1, "edge": "k" } }),
        ),
        ("combine", json!({ "target": "Body 1" })),
        ("combine", json!({ "target": "Body 1", "tools": [] })),
        (
            "combine",
            json!({ "target": "Body 1", "tools": ["B"], "operation": "join" }),
        ),
    ] {
        refused(tool, arguments);
    }
}

/// A key wrapped in what it names.
fn keyed(kind: ItemKind, key: &str) -> Item {
    Item {
        kind,
        key: key.into(),
    }
}

#[test]
fn reads_the_motion_tools() {
    assert_eq!(
        call("move_body", json!({ "bodies": ["Body 1"] })),
        Ok(Call::MoveBody(MoveBody {
            naming: Default::default(),
            bodies: vec!["Body 1".into()],
            offset: [None, None, None],
            angle: None,
            axis: None,
        }))
    );
    assert_eq!(
        call(
            "move_body",
            json!({ "bodies": ["A", "B"], "x": "5", "z": "-w", "angle": "90 deg",
                    "axis": { "face": "k" } })
        ),
        Ok(Call::MoveBody(MoveBody {
            naming: Default::default(),
            bodies: vec!["A".into(), "B".into()],
            offset: [Some("5".into()), None, Some("-w".into())],
            angle: Some("90 deg".into()),
            axis: Some(Reference::Key(keyed(ItemKind::Face, "k"))),
        }))
    );
    for (axis, read) in [
        (json!("X"), Reference::World(WorldAxis::X)),
        (json!(" y "), Reference::World(WorldAxis::Y)),
        (
            json!({ "edge": "e" }),
            Reference::Key(keyed(ItemKind::Edge, "e")),
        ),
    ] {
        let Ok(Call::MoveBody(moved)) = call("move_body", json!({ "bodies": ["A"], "axis": axis }))
        else {
            panic!();
        };
        assert_eq!(moved.axis, Some(read));
    }
    assert_eq!(
        call("mirror", json!({ "bodies": ["A"], "plane": "XZ" })),
        Ok(Call::Mirror(Mirror {
            naming: Default::default(),
            bodies: vec!["A".into()],
            plane: Reference::World(WorldPlane::Xz),
            keep_original: true,
        }))
    );
    assert_eq!(
        call(
            "mirror",
            json!({ "bodies": ["A"], "plane": { "face": "f" }, "keep_original": false })
        ),
        Ok(Call::Mirror(Mirror {
            naming: Default::default(),
            bodies: vec!["A".into()],
            plane: Reference::Key(keyed(ItemKind::Face, "f")),
            keep_original: false,
        }))
    );
    assert_eq!(
        call("pattern", json!({ "bodies": ["A"] })),
        Ok(Call::Pattern(Pattern {
            naming: Default::default(),
            kind: PatternKind::Linear,
            bodies: vec!["A".into()],
            axis: None,
            count: None,
            mode: None,
            spread: None,
            flip: false,
            join: true,
        }))
    );
    assert_eq!(
        call(
            "pattern",
            json!({ "kind": "circular", "bodies": ["A"], "axis": "y", "count": 6,
                    "mode": "total", "spread": "180 deg", "join": false })
        ),
        Ok(Call::Pattern(Pattern {
            naming: Default::default(),
            kind: PatternKind::Circular,
            bodies: vec!["A".into()],
            axis: Some(Reference::World(WorldAxis::Y)),
            count: Some("6".into()),
            mode: Some(PatternMode::Total),
            spread: Some("180 deg".into()),
            flip: false,
            join: false,
        }))
    );
    let Ok(Call::Pattern(pattern)) = call("pattern", json!({ "bodies": ["A"], "count": "n + 1" }))
    else {
        panic!();
    };
    assert_eq!(pattern.count.as_deref(), Some("n + 1"));
    assert_eq!(
        call(
            "align",
            json!({ "from": { "point": { "corner": "c" } }, "to": { "point": "origin" } })
        ),
        Ok(Call::Align(Box::new(Align {
            naming: Default::default(),
            body: None,
            from: AlignSide {
                point: Reference::Key(keyed(ItemKind::Corner, "c")),
                direction: None,
                second_direction: None,
            },
            to: AlignSide {
                point: Reference::World(()),
                direction: None,
                second_direction: None,
            },
            flip: false,
            distance: None,
            angle: None,
        })))
    );
    assert_eq!(
        call(
            "align",
            json!({
                "body": "Body 2",
                "from": { "point": { "edge": "r" }, "direction": { "face": "f" },
                          "second_direction": { "edge": "s" } },
                "to": { "point": { "edge": "h" }, "direction": "z", "second_direction": "x" },
                "flip": true, "distance": "10", "angle": "5 deg"
            })
        ),
        Ok(Call::Align(Box::new(Align {
            naming: Default::default(),
            body: Some("Body 2".into()),
            from: AlignSide {
                point: Reference::Key(keyed(ItemKind::Edge, "r")),
                direction: Some(Reference::Key(keyed(ItemKind::Face, "f"))),
                second_direction: Some(Reference::Key(keyed(ItemKind::Edge, "s"))),
            },
            to: AlignSide {
                point: Reference::Key(keyed(ItemKind::Edge, "h")),
                direction: Some(Reference::World(WorldAxis::Z)),
                second_direction: Some(Reference::World(WorldAxis::X)),
            },
            flip: true,
            distance: Some("10".into()),
            angle: Some("5 deg".into()),
        })))
    );
    assert_eq!(
        call("scale", json!({ "bodies": ["A"], "factor": "2" })),
        Ok(Call::Scale(Box::new(Scale {
            naming: Default::default(),
            bodies: vec!["A".into()],
            point: None,
            mode: ScaleMode::Uniform,
            factor: Some("2".into()),
            factors: [None, None, None],
            edge: None,
            length: None,
            axis_only: false,
        })))
    );
    assert_eq!(
        call(
            "scale",
            json!({ "bodies": ["A"], "point": { "corner": "c" }, "mode": "edge_length",
                    "edge": "e", "length": "50 mm", "axis_only": true, "y": "3" })
        ),
        Ok(Call::Scale(Box::new(Scale {
            naming: Default::default(),
            bodies: vec!["A".into()],
            point: Some(Reference::Key(keyed(ItemKind::Corner, "c"))),
            mode: ScaleMode::EdgeLength,
            factor: None,
            factors: [None, Some("3".into()), None],
            edge: Some("e".into()),
            length: Some("50 mm".into()),
            axis_only: true,
        })))
    );
    let many: Vec<String> = (0..=MAX_ITEMS).map(|n| format!("Body {n}")).collect();
    for (tool, arguments) in [
        ("move_body", json!({})),
        ("move_body", json!({ "bodies": [] })),
        ("move_body", json!({ "bodies": many })),
        ("move_body", json!({ "bodies": "Body 1" })),
        ("move_body", json!({ "bodies": ["A"], "x": 5 })),
        ("move_body", json!({ "bodies": ["A"], "axis": "w" })),
        (
            "move_body",
            json!({ "bodies": ["A"], "axis": { "corner": "c" } }),
        ),
        (
            "move_body",
            json!({ "bodies": ["A"], "axis": { "edge": "e", "face": "f" } }),
        ),
        (
            "move_body",
            json!({ "bodies": ["A"], "axis": { "edge": "" } }),
        ),
        ("move_body", json!({ "bodies": ["A"], "offset": "5" })),
        ("mirror", json!({ "bodies": ["A"] })),
        ("mirror", json!({ "bodies": ["A"], "plane": "z" })),
        (
            "mirror",
            json!({ "bodies": ["A"], "plane": { "edge": "e" } }),
        ),
        ("pattern", json!({ "bodies": ["A"], "kind": "radial" })),
        ("pattern", json!({ "bodies": ["A"], "mode": "even" })),
        ("pattern", json!({ "bodies": ["A"], "count": -3 })),
        ("pattern", json!({ "bodies": ["A"], "count": 2.5 })),
        ("pattern", json!({ "bodies": ["A"], "count": "" })),
        ("pattern", json!({ "bodies": ["A"], "spread": 5 })),
        ("align", json!({ "from": { "point": "origin" } })),
        ("align", json!({ "to": { "point": "origin" } })),
        ("align", json!({ "from": {}, "to": { "point": "origin" } })),
        (
            "align",
            json!({ "from": { "point": { "face": "f" } }, "to": { "point": "origin" } }),
        ),
        (
            "align",
            json!({ "from": { "point": "x" }, "to": { "point": "origin" } }),
        ),
        (
            "align",
            json!({ "from": { "point": "origin", "direction": { "corner": "c" } },
                    "to": { "point": "origin" } }),
        ),
        (
            "align",
            json!({ "from": { "point": "origin", "normal": "z" }, "to": { "point": "origin" } }),
        ),
        ("scale", json!({ "bodies": ["A"], "mode": "stretch" })),
        ("scale", json!({ "bodies": ["A"], "point": "x" })),
        (
            "scale",
            json!({ "bodies": ["A"], "point": { "face": "f" } }),
        ),
        ("scale", json!({ "bodies": ["A"], "edge": { "edge": "e" } })),
    ] {
        refused(tool, arguments);
    }
    assert!(
        refused("move_body", json!({ "bodies": ["A"], "axis": "w" })).contains(
            "axis is \"x\", \"y\", \"z\", {\"edge\": key} or {\"face\": key} (keys from \
             body_topology)"
        )
    );
}

#[test]
fn reads_the_split_and_face_calls() {
    let keyed = |kind, key: &str| Item {
        kind,
        key: key.into(),
    };
    assert_eq!(
        call("split", json!({ "body": "Body 1", "tool": "XZ" })),
        Ok(Call::Split(Split {
            naming: Default::default(),
            body: "Body 1".into(),
            tool: SplitWith::Surface(Reference::World(WorldPlane::Xz)),
            original: None,
            keep: SplitKeep::Both,
        }))
    );
    assert_eq!(
        call(
            "split",
            json!({ "body": "Body 1", "tool": { "face": "f" }, "original": "back" })
        ),
        Ok(Call::Split(Split {
            naming: Default::default(),
            body: "Body 1".into(),
            tool: SplitWith::Surface(Reference::Key(keyed(ItemKind::Face, "f"))),
            original: Some(SplitSide::Back),
            keep: SplitKeep::Both,
        }))
    );
    assert_eq!(
        call(
            "split",
            json!({ "body": "Body 1", "tool": { "body": "Body 2" }, "keep": "front" })
        ),
        Ok(Call::Split(Split {
            naming: Default::default(),
            body: "Body 1".into(),
            tool: SplitWith::Body("Body 2".into()),
            original: None,
            keep: SplitKeep::Front,
        }))
    );
    assert_eq!(
        call(
            "split",
            json!({ "body": "B", "tool": { "sketch": "Sketch 1", "profiles": [0, ["1", 2]] } })
        ),
        Ok(Call::Split(Split {
            naming: Default::default(),
            body: "B".into(),
            tool: SplitWith::Regions {
                sketch: "Sketch 1".into(),
                profiles: Profiles::Picked(vec![
                    Profile::Index(0),
                    Profile::Point(["1".into(), "2".into()])
                ]),
            },
            original: None,
            keep: SplitKeep::Both,
        }))
    );
    assert_eq!(
        call(
            "split",
            json!({ "body": "B", "tool": { "sketch": "Sketch 1", "curves": [3, 1] } })
        ),
        Ok(Call::Split(Split {
            naming: Default::default(),
            body: "B".into(),
            tool: SplitWith::Line {
                sketch: "Sketch 1".into(),
                curves: vec![3, 1],
            },
            original: None,
            keep: SplitKeep::Both,
        }))
    );
    assert_eq!(
        call("shell", json!({})),
        Ok(Call::Shell(Shell {
            naming: Default::default(),
            body: None,
            faces: Vec::new(),
            thickness: None,
            direction: ShellDirection::Inward,
        }))
    );
    assert_eq!(
        call(
            "shell",
            json!({ "faces": ["a", "b"], "thickness": "1 mm", "direction": "outward" })
        ),
        Ok(Call::Shell(Shell {
            naming: Default::default(),
            body: None,
            faces: vec!["a".into(), "b".into()],
            thickness: Some("1 mm".into()),
            direction: ShellDirection::Outward,
        }))
    );
    assert_eq!(
        call("offset_face", json!({ "faces": ["a"] })),
        Ok(Call::OffsetFace(OffsetFace {
            naming: Default::default(),
            faces: vec!["a".into()],
            distance: None,
            inward: false,
            tangent: true,
        }))
    );
    assert_eq!(
        call(
            "offset_face",
            json!({ "faces": ["a"], "distance": "2", "inward": true, "tangent": false })
        ),
        Ok(Call::OffsetFace(OffsetFace {
            naming: Default::default(),
            faces: vec!["a".into()],
            distance: Some("2".into()),
            inward: true,
            tangent: false,
        }))
    );
    assert_eq!(
        call("draft", json!({ "faces": ["a"] })),
        Ok(Call::Draft(Draft {
            naming: Default::default(),
            faces: vec!["a".into()],
            neutral: None,
            angle: None,
            flip: false,
            tangent: true,
        }))
    );
    assert_eq!(
        call(
            "draft",
            json!({ "faces": ["a"], "neutral": { "face": "n" }, "angle": "5 deg", "flip": true })
        ),
        Ok(Call::Draft(Draft {
            naming: Default::default(),
            faces: vec!["a".into()],
            neutral: Some(Reference::Key(keyed(ItemKind::Face, "n"))),
            angle: Some("5 deg".into()),
            flip: true,
            tangent: true,
        }))
    );
    let many: Vec<String> = (0..=MAX_ITEMS).map(|n| format!("k{n}")).collect();
    let curves: Vec<u32> = (0..=MAX_ITEMS as u32).collect();
    for (tool, arguments) in [
        ("split", json!({ "tool": "xy" })),
        ("split", json!({ "body": "B" })),
        ("split", json!({ "body": "B", "tool": "z" })),
        ("split", json!({ "body": "B", "tool": 3 })),
        ("split", json!({ "body": "B", "tool": { "edge": "e" } })),
        (
            "split",
            json!({ "body": "B", "tool": { "face": "f", "body": "C" } }),
        ),
        ("split", json!({ "body": "B", "tool": { "sketch": "S" } })),
        (
            "split",
            json!({ "body": "B", "tool": { "sketch": "S", "profiles": "all", "curves": [1] } }),
        ),
        (
            "split",
            json!({ "body": "B", "tool": { "sketch": "S", "curves": [] } }),
        ),
        (
            "split",
            json!({ "body": "B", "tool": { "sketch": "S", "curves": [-1] } }),
        ),
        (
            "split",
            json!({ "body": "B", "tool": { "sketch": "S", "curves": curves } }),
        ),
        ("split", json!({ "body": "B", "tool": { "curves": [1] } })),
        (
            "split",
            json!({ "body": "B", "tool": "xy", "keep": "neither" }),
        ),
        (
            "split",
            json!({ "body": "B", "tool": "xy", "original": "both" }),
        ),
        ("shell", json!({ "faces": "a" })),
        ("shell", json!({ "faces": many.clone() })),
        ("shell", json!({ "faces": [""] })),
        ("shell", json!({ "direction": "up" })),
        ("shell", json!({ "body": "" })),
        ("offset_face", json!({ "faces": [] })),
        ("offset_face", json!({ "faces": many.clone() })),
        ("offset_face", json!({ "faces": ["a"], "distance": 1 })),
        ("draft", json!({})),
        ("draft", json!({ "faces": ["a"], "neutral": "z" })),
        (
            "draft",
            json!({ "faces": ["a"], "neutral": { "edge": "e" } }),
        ),
        ("draft", json!({ "faces": ["a"], "plane": "xy" })),
    ] {
        refused(tool, arguments);
    }
}

#[test]
fn reads_the_blend_sweep_and_loft_calls() {
    let keyed = |kind, key: &str| Item {
        kind,
        key: key.into(),
    };
    assert_eq!(
        call("chamfer", json!({ "edges": ["e"] })),
        Ok(Call::Chamfer(Chamfer {
            naming: Default::default(),
            edges: vec!["e".into()],
            faces: Vec::new(),
            kind: ChamferType::Equal,
            distance: None,
            second_distance: None,
            angle: None,
            flip: false,
            tangent: true,
        }))
    );
    assert_eq!(
        call(
            "chamfer",
            json!({ "faces": ["f"], "type": "distance_angle", "distance": "2", "angle": "30 deg",
                    "flip": true, "tangent": false })
        ),
        Ok(Call::Chamfer(Chamfer {
            naming: Default::default(),
            edges: Vec::new(),
            faces: vec!["f".into()],
            kind: ChamferType::DistanceAngle,
            distance: Some("2".into()),
            second_distance: None,
            angle: Some("30 deg".into()),
            flip: true,
            tangent: false,
        }))
    );
    assert_eq!(
        call("fillet", json!({ "edges": ["e"], "radius": "3" })),
        Ok(Call::Fillet(Fillet {
            naming: Default::default(),
            edges: vec!["e".into()],
            faces: Vec::new(),
            radius: Some("3".into()),
            tangent: true,
        }))
    );
    assert_eq!(
        call(
            "sweep",
            json!({ "sketch": "Sketch 1", "profiles": [0],
                    "path": { "curves": [{ "sketch": "Sketch 2", "curve": 4 }], "edges": ["e"] },
                    "twist": "90 deg", "operation": "join", "exclude": ["Body 2"] })
        ),
        Ok(Call::Sweep(Box::new(Sweep {
            naming: Default::default(),
            sketch: "Sketch 1".into(),
            profiles: Profiles::Picked(vec![Profile::Index(0)]),
            along: SweepAlong::Path {
                curves: vec![CurveIn {
                    sketch: "Sketch 2".into(),
                    curve: 4,
                }],
                edges: vec!["e".into()],
                tangent: true,
            },
            keep_orientation: false,
            twist: Some("90 deg".into()),
            operation: Operation::Join,
            exclude: vec!["Body 2".into()],
        })))
    );
    assert_eq!(
        call(
            "sweep",
            json!({ "sketch": "S", "profiles": "all",
                    "helix": { "axis": { "edge": "e" }, "turns": 3, "left_handed": true } })
        ),
        Ok(Call::Sweep(Box::new(Sweep {
            naming: Default::default(),
            sketch: "S".into(),
            profiles: Profiles::All,
            along: SweepAlong::Helix {
                axis: Reference::Key(keyed(ItemKind::Edge, "e")),
                pitch: None,
                turns: Some("3".into()),
                left_handed: true,
                flip: false,
            },
            keep_orientation: false,
            twist: None,
            operation: Operation::NewBody,
            exclude: Vec::new(),
        })))
    );
    // A helix takes part of a turn, given as a number too.
    let helix = |turns: Value| match call(
        "sweep",
        json!({ "sketch": "S", "profiles": "all", "helix": { "axis": "z", "turns": turns } }),
    ) {
        Ok(Call::Sweep(sweep)) => match sweep.along {
            SweepAlong::Helix { turns, .. } => turns,
            along => panic!("not a helix: {along:?}"),
        },
        other => panic!("not a sweep: {other:?}"),
    };
    assert_eq!(helix(json!(2.5)), Some("2.5".into()));
    assert_eq!(helix(json!("2.5")), Some("2.5".into()));
    assert_eq!(
        call(
            "loft",
            json!({ "sections": [
                        { "sketch": "A", "region": 1, "start": 7 },
                        { "sketch": "B", "region": ["5", 2.5] },
                        { "sketch": "C", "point": 3 }
                    ],
                    "mode": "ruled", "rails": [{ "sketch": "D", "curve": 2 }] })
        ),
        Ok(Call::Loft(Loft {
            naming: Default::default(),
            sections: vec![
                Section {
                    sketch: "A".into(),
                    shape: SectionShape::Region(Profile::Index(1)),
                    start: Some(7),
                },
                Section {
                    sketch: "B".into(),
                    shape: SectionShape::Region(Profile::Point(["5".into(), "2.5".into()])),
                    start: None,
                },
                Section {
                    sketch: "C".into(),
                    shape: SectionShape::Point(3),
                    start: None,
                },
            ],
            mode: LoftMode::Ruled,
            closed: false,
            rails: vec![CurveIn {
                sketch: "D".into(),
                curve: 2,
            }],
            operation: Operation::NewBody,
            exclude: Vec::new(),
        }))
    );
    let many: Vec<String> = (0..=MAX_ITEMS).map(|n| format!("k{n}")).collect();
    let rails: Vec<Value> = (0..=MAX_ITEMS)
        .map(|n| json!({ "sketch": "S", "curve": n }))
        .collect();
    let path = json!({ "edges": ["e"] });
    for (tool, arguments) in [
        ("chamfer", json!({})),
        ("chamfer", json!({ "edges": [] })),
        ("chamfer", json!({ "edges": many.clone() })),
        ("chamfer", json!({ "edges": ["e"], "type": "two" })),
        ("chamfer", json!({ "edges": ["e"], "distance": 1 })),
        ("chamfer", json!({ "edges": ["e"], "radius": "1" })),
        ("fillet", json!({ "faces": [] })),
        ("fillet", json!({ "edges": [""] })),
        ("fillet", json!({ "edges": ["e"], "tangent": "yes" })),
        ("sweep", json!({ "sketch": "S", "profiles": [0] })),
        (
            "sweep",
            json!({ "sketch": "S", "profiles": [0], "path": path.clone(), "helix": { "axis": "z" } }),
        ),
        (
            "sweep",
            json!({ "sketch": "S", "profiles": [0], "path": {} }),
        ),
        (
            "sweep",
            json!({ "sketch": "S", "profiles": [0], "path": { "curves": [{ "sketch": "S" }] } }),
        ),
        (
            "sweep",
            json!({ "sketch": "S", "profiles": [0], "path": { "curves": [{ "sketch": "S", "curve": -1 }] } }),
        ),
        (
            "sweep",
            json!({ "sketch": "S", "profiles": [0], "path": { "edges": ["e"], "axis": "z" } }),
        ),
        (
            "sweep",
            json!({ "sketch": "S", "profiles": [0], "helix": {} }),
        ),
        (
            "sweep",
            json!({ "sketch": "S", "profiles": [0], "helix": { "axis": { "corner": "c" } } }),
        ),
        (
            "sweep",
            json!({ "sketch": "S", "profiles": [0], "helix": { "axis": "w" } }),
        ),
        ("sweep", json!({ "profiles": [0], "path": path.clone() })),
        ("loft", json!({})),
        ("loft", json!({ "sections": [] })),
        ("loft", json!({ "sections": [{ "sketch": "A" }] })),
        (
            "loft",
            json!({ "sections": [{ "sketch": "A", "region": 0, "point": 1 }] }),
        ),
        (
            "loft",
            json!({ "sections": [{ "sketch": "A", "point": 1, "start": 2 }] }),
        ),
        (
            "loft",
            json!({ "sections": [{ "sketch": "A", "region": -1 }] }),
        ),
        (
            "loft",
            json!({ "sections": [{ "sketch": "A", "region": 0 }], "mode": "loose" }),
        ),
        (
            "loft",
            json!({ "sections": [{ "sketch": "A", "region": 0 }], "rails": rails }),
        ),
    ] {
        refused(tool, arguments);
    }
}

/// Each of the world's says the word it's read by.
#[test]
fn world_words_round_trip() {
    for (word, axis) in WORLD_AXES {
        assert_eq!(axis.word(), *word);
    }
    for (word, plane) in WORLD_PLANES {
        assert_eq!(plane.word(), *word);
    }
    assert_eq!(().word(), "origin");
}

/// `set_feature` takes its feature's name and the rest as the patch,
/// read later as the feature's tool's arguments: each feature tool's
/// fields are what its arguments are.
#[test]
fn set_feature_keeps_the_patch_for_the_app() {
    let patch = json!({ "distance": "15 mm", "flip": true });
    let mut arguments = patch.as_object().unwrap().clone();
    arguments.insert("name".to_owned(), json!("Extrude 1"));
    let Ok(Call::SetFeature(set)) = call("set_feature", Value::Object(arguments)) else {
        panic!("not a set_feature");
    };
    assert_eq!(set.name, "Extrude 1");
    assert_eq!(Value::Object(set.patch.clone()), patch);
    assert!(matches!(
        call("set_feature", json!({ "name": "Extrude 1" })),
        Err(CallError::Arguments { message: why, .. }) if why.contains("give the fields to change")
    ));
    assert!(matches!(
        call("set_feature", json!({ "distance": "1" })),
        Err(CallError::Arguments { message: why, .. }) if why.contains("name is required")
    ));
    assert_eq!(FeatureTool::named("set_feature"), None);
    assert_eq!(
        FeatureTool::named("combine").map(FeatureTool::fields),
        Some(&["target", "tools", "operation", "keep_tools"][..])
    );
    let mut extrude = set.patch;
    extrude.insert("sketch".to_owned(), json!("Sketch 1"));
    extrude.insert("profiles".to_owned(), json!("all"));
    let read = parse_arguments("extrude", &extrude);
    assert!(matches!(read, Ok(Call::Extrude(_))), "{read:?}");
}

/// Each feature tool's fields are its schema's properties, and an empty
/// patch of its kind reads: every argument it requires has a
/// placeholder.
#[test]
fn feature_tools_fields_match_their_schemas_and_patches_read() {
    let list = list();
    let mut kinds = 0;
    for tool in list["tools"].as_array().unwrap() {
        let name = tool["name"].as_str().unwrap();
        let Some(feature) = FeatureTool::named(name) else {
            continue;
        };
        let fields = feature.fields();
        kinds += 1;
        let mut properties: Vec<&str> = (tool["inputSchema"]["properties"].as_object())
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let mut fields = fields.to_vec();
        // Beside its fields, the names of what it makes, never patched.
        let bodies = feature.new_bodies().key();
        let naming = |key: &str| key == "name" || Some(key) == bodies;
        assert!(properties.contains(&"name"), "{name}");
        assert_eq!(
            bodies.is_some_and(|key| properties.contains(&key)),
            bodies.is_some()
        );
        properties.retain(|key| !naming(key));
        properties.sort_unstable();
        fields.sort_unstable();
        assert_eq!(fields, properties, "{name}");
        let read = parse_patch(feature, &Map::new());
        assert!(read.is_ok(), "{name}: {read:?}");
        // A patch takes no names: they're rename's.
        for key in ["name"].into_iter().chain(bodies) {
            let value = match key {
                "body_names" => json!(["A"]),
                _ => json!("A"),
            };
            let patch = Map::from_iter([(key.to_owned(), value)]);
            let read = parse_patch(feature, &patch);
            assert!(read.is_err(), "{name} {key}: {read:?}");
        }
    }
    assert_eq!(kinds, 16);
}

#[test]
fn reads_the_sketch_edits() {
    assert_eq!(
        call("add_sketch", json!({ "plane": "XZ" })),
        Ok(Call::AddSketch {
            name: None,
            plane: Reference::World(WorldPlane::Xz)
        })
    );
    assert_eq!(
        call("add_sketch", json!({ "plane": { "face": "k" } })),
        Ok(Call::AddSketch {
            name: None,
            plane: Reference::Key(keyed(ItemKind::Face, "k"))
        })
    );
    assert_eq!(
        call(
            "set_sketch_plane",
            json!({ "sketch": "Sketch 1", "plane": "yz" })
        ),
        Ok(Call::SetSketchPlane {
            sketch: "Sketch 1".into(),
            plane: Reference::World(WorldPlane::Yz)
        })
    );
    assert_eq!(
        call(
            "add_link",
            json!({ "sketch": "Sketch 2", "from": { "edge": "e" } })
        ),
        Ok(Call::AddLink(AddLink {
            sketch: "Sketch 2".into(),
            kind: LinkKind::Project,
            from: LinkFrom::Item(keyed(ItemKind::Edge, "e")),
        }))
    );
    assert_eq!(
        call(
            "add_link",
            json!({ "sketch": "S", "kind": "intersect", "from": { "sketch": "T", "curve": 3 } })
        ),
        Ok(Call::AddLink(AddLink {
            sketch: "S".into(),
            kind: LinkKind::Intersect,
            from: LinkFrom::Curve(CurveIn {
                sketch: "T".into(),
                curve: 3
            }),
        }))
    );
    assert_eq!(
        call(
            "add_link",
            json!({ "sketch": "S", "from": { "sketch": "T", "point": 1 } })
        ),
        Ok(Call::AddLink(AddLink {
            sketch: "S".into(),
            kind: LinkKind::Project,
            from: LinkFrom::Point {
                sketch: "T".into(),
                point: 1
            },
        }))
    );
    for (name, arguments) in [
        ("add_sketch", json!({})),
        ("add_sketch", json!({ "plane": "zz" })),
        ("add_sketch", json!({ "plane": { "edge": "k" } })),
        ("set_sketch_plane", json!({ "plane": "xy" })),
        ("add_link", json!({ "sketch": "S" })),
        ("add_link", json!({ "sketch": "S", "from": "xy" })),
        (
            "add_link",
            json!({ "sketch": "S", "from": { "sketch": "T" } }),
        ),
        (
            "add_link",
            json!({ "sketch": "S", "from": { "sketch": "T", "curve": 1, "point": 2 } }),
        ),
        (
            "add_link",
            json!({ "sketch": "S", "kind": "mirror", "from": { "edge": "e" } }),
        ),
    ] {
        assert!(
            matches!(
                call(name, arguments.clone()),
                Err(CallError::Arguments { .. })
            ),
            "{name} {arguments}"
        );
    }
}

/// `edit_sketch`'s operations read into their checked forms: shapes,
/// labels and their parts, typed text and numbers, the defaults.
#[test]
fn edit_sketch_reads_its_operations() {
    let Ok(Call::EditSketch(edit)) = call(
        "edit_sketch",
        json!({ "sketch": "Sketch 2", "operations": [
            { "op": "rectangle", "corner": [0, "1 mm"], "size": [20, 10], "label": "r" },
            { "op": "line", "start": "r.corner3", "end": 7, "construction": true },
            { "op": "polygon", "center": "origin", "sides": 6, "diameter": "d" },
            { "op": "spline", "points": [[0, 0], [1, 1]], "kind": "control", "closed": true },
            { "op": "dimension", "kind": "length", "items": ["r.side1"], "value": "30 mm" },
            { "op": "dimension", "kind": "radius", "items": [3], "driving": false },
            { "op": "move", "point": 4, "to": [1, 2] }
        ]}),
    ) else {
        panic!("not read");
    };
    assert_eq!(edit.sketch, "Sketch 2");
    let ops = &edit.operations;
    assert_eq!(ops.len(), 7);
    assert_eq!(ops[0].label.as_deref(), Some("r"));
    assert_eq!(
        ops[0].op,
        Op::Rectangle {
            from: RectangleFrom::Corner(At::Point(["0".into(), "1 mm".into()])),
            size: ["20".into(), "10".into()],
        }
    );
    assert_eq!(
        ops[1].op,
        Op::Line {
            start: At::Item(ItemRef::Label("r.corner3".into())),
            end: At::Item(ItemRef::Id(7)),
        }
    );
    assert!(ops[1].construction && !ops[0].construction);
    assert_eq!(
        ops[2].op,
        Op::Polygon {
            center: At::Item(ItemRef::Label("origin".into())),
            sides: 6,
            size: PolygonSize::Diameter("d".into()),
        }
    );
    assert!(matches!(
        ops[3].op,
        Op::Spline {
            closed: true,
            control: true,
            ..
        }
    ));
    assert_eq!(
        ops[4].op,
        Op::Dimension {
            kind: MeasureWord::Length,
            items: vec![ItemRef::Label("r.side1".into())],
            value: Some("30 mm".into()),
            driving: true,
        }
    );
    assert!(matches!(
        ops[5].op,
        Op::Dimension {
            driving: false,
            value: None,
            ..
        }
    ));
    assert_eq!(ops[6].op.word(), "move");
}

/// `edit_sketch`'s bounds: the operations' count, labels unique, not
/// reserved, without a dot and bounded, a polygon's sides, a spline's
/// points, the items' counts, a reference's value, and unknown ops and
/// arguments; each naming the operation.
#[test]
fn edit_sketch_is_bounded() {
    let point = json!({ "op": "point", "at": [0, 0] });
    let ops = |ops: Value| json!({ "sketch": "S", "operations": ops });
    let too_many: Vec<Value> = vec![point.clone(); MAX_OPERATIONS + 1];
    assert!(refused("edit_sketch", ops(json!(too_many))).contains("more than 64"));
    let most: Vec<Value> = vec![point.clone(); MAX_OPERATIONS];
    assert!(call("edit_sketch", ops(json!(most))).is_ok());
    let long = "l".repeat(LABEL_BYTES + 1);
    for (operations, why) in [
        (json!([]), "operations is empty"),
        (json!("point"), "operations is a list"),
        (
            json!([point, { "op": "point", "at": [1, 1], "label": "a" }, { "op": "point", "at": [2, 2], "label": "a" }]),
            "edit_sketch: operations[2]: the label \"a\" is given already",
        ),
        (
            json!([{ "op": "point", "at": [1, 1], "label": "a.b" }]),
            "operations[0]: a label has no \".\"",
        ),
        (
            json!([{ "op": "point", "at": [1, 1], "label": "origin" }]),
            "names the sketch's own",
        ),
        (
            json!([{ "op": "point", "at": [1, 1], "label": long }]),
            "label is longer than 64 bytes",
        ),
        (
            json!([{ "op": "polygon", "center": [0, 0], "sides": 2, "diameter": 4 }]),
            "sides is required: a whole number from 3 to 64",
        ),
        (
            json!([{ "op": "polygon", "center": [0, 0], "sides": 65, "diameter": 4 }]),
            "from 3 to 64",
        ),
        (
            json!([{ "op": "polygon", "center": [0, 0], "sides": 6 }]),
            "give corner or diameter",
        ),
        (
            json!([{ "op": "spline", "points": [[0, 0]] }]),
            "points is a list of 2 to 100 points",
        ),
        (
            json!([{ "op": "spline", "points": vec![json!([0, 0]); SPLINE_POINTS + 1] }]),
            "points is a list of 2 to 100 points",
        ),
        (
            json!([{ "op": "delete", "items": vec![json!(1); MAX_ITEMS + 1] }]),
            "items is a list of 1 to 256 items",
        ),
        (
            json!([{ "op": "dimension", "kind": "length", "items": [1, 2, 3] }]),
            "items is a list of 1 to 2 items",
        ),
        (
            json!([{ "op": "dimension", "kind": "length", "items": [1], "driving": false, "value": "3" }]),
            "a reference dimension takes no value",
        ),
        (
            json!([{ "op": "delete", "items": [-1] }]),
            "each an id or a label",
        ),
        (
            json!([{ "op": "delete", "items": [4294967296_u64] }]),
            "each an id or a label",
        ),
        (json!([{ "op": "erase" }]), "no op named \"erase\""),
        (json!([{ "at": [0, 0] }]), "op is required"),
        (
            json!([{ "op": "constraint", "kind": "horizontal", "items": [1], "label": "c" }]),
            "no argument named \"label\"",
        ),
        (
            json!([{ "op": "rectangle", "corner": [0, 0], "center": [1, 1], "size": [1, 1] }]),
            "give corner or center, one of them",
        ),
        (
            json!([{ "op": "line", "start": [0], "end": [1, 1] }]),
            "start is required",
        ),
        (
            json!([{ "op": "move", "point": 1, "to": [1, "x".repeat(TEXT_BYTES + 1)] }]),
            "to is required",
        ),
    ] {
        let message = refused("edit_sketch", ops(operations.clone()));
        assert!(message.contains(why), "{operations}: {message}");
    }
}

#[test]
fn reads_measure() {
    let face = Measured::Item(Item {
        kind: ItemKind::Face,
        key: "k1".to_owned(),
    });
    assert_eq!(
        call("measure", json!({ "a": { "face": "k1" } })),
        Ok(Call::Measure(Measure {
            a: face.clone(),
            b: None
        }))
    );
    let both = call(
        "measure",
        json!({ "a": { "face": "k1" }, "b": { "body": "Body 1" } }),
    );
    assert_eq!(
        both,
        Ok(Call::Measure(Measure {
            a: face,
            b: Some(Measured::Body("Body 1".to_owned())),
        }))
    );
    assert!(!both.unwrap().edits());
    let Ok(Call::Measure(Measure {
        a: Measured::Item(Item { kind, .. }),
        ..
    })) = call("measure", json!({ "a": { "corner": "k" } }))
    else {
        panic!("not a corner");
    };
    assert_eq!(kind, ItemKind::Corner);
    assert!(refused("measure", json!({})).contains("a is required"));
    assert!(refused("measure", json!({ "a": "Body 1" })).contains("{\"body\": name}"));
    assert!(refused("measure", json!({ "a": { "plane": "k" } })).contains("a is"));
    let two_keys = json!({ "a": { "face": "k", "edge": "k" } });
    assert!(refused("measure", two_keys).contains("a is"));
    assert!(refused("measure", json!({ "a": { "body": "" } })).contains("empty"));
    let long = "x".repeat(NAME_BYTES + 1);
    assert!(refused("measure", json!({ "a": { "edge": long } })).contains("longer"));
    let extra = json!({ "a": { "body": "B" }, "c": 1 });
    assert!(refused("measure", extra).contains("\"c\""));
}

#[test]
fn reads_render() {
    assert_eq!(
        call("render", json!({})),
        Ok(Call::Render(Render {
            bodies: Vec::new(),
            from: RenderFrom::View(NamedView::Iso),
            perspective: false,
            zoom: 1.0,
            highlight: Vec::new(),
            size: RENDER_SIZE,
        }))
    );
    assert_eq!(
        call(
            "render",
            json!({
                "bodies": ["Body 1"],
                "from": { "yaw": "30 deg", "pitch": -10 },
                "projection": "perspective",
                "zoom": 2.5,
                "highlight": [{ "face": "k1" }, { "edge": "k2" }],
                "size": 1024
            })
        ),
        Ok(Call::Render(Render {
            bodies: vec!["Body 1".to_owned()],
            from: RenderFrom::Angles {
                yaw: "30 deg".to_owned(),
                pitch: "-10".to_owned(),
            },
            perspective: true,
            zoom: 2.5,
            highlight: vec![
                Item {
                    kind: ItemKind::Face,
                    key: "k1".to_owned(),
                },
                Item {
                    kind: ItemKind::Edge,
                    key: "k2".to_owned(),
                },
            ],
            size: 1024,
        }))
    );
    let Ok(Call::Render(top)) = call("render", json!({ "from": "TOP" })) else {
        panic!();
    };
    assert_eq!(top.from, RenderFrom::View(NamedView::Top));
    assert!(!Call::Render(top).edits());
    for bad in [
        json!({ "from": "sideways" }),
        json!({ "from": { "yaw": "30 deg" } }),
        json!({ "from": { "yaw": 1, "pitch": 2, "roll": 3 } }),
        json!({ "from": 3 }),
        json!({ "projection": "fisheye" }),
        json!({ "zoom": 0.01 }),
        json!({ "zoom": 11 }),
        json!({ "zoom": "2" }),
        json!({ "size": 63 }),
        json!({ "size": 1025 }),
        json!({ "size": 500.5 }),
        json!({ "highlight": [{ "corner": "k" }] }),
        json!({ "highlight": [{ "face": "k", "edge": "j" }] }),
        json!({ "highlight": { "face": "k" } }),
        json!({ "highlight": vec![json!({ "face": "k" }); MAX_ITEMS + 1] }),
        json!({ "bodies": "Body 1" }),
        json!({ "colour": "red" }),
    ] {
        refused("render", bad);
    }
}

#[test]
fn an_image_goes_as_image_content_bounded_apart_from_the_text() {
    use base64::Engine;
    let png = vec![1, 2, 3];
    let reply = ToolReply::data(json!({ "a": 1 })).with_png(png.clone());
    let result = reply.result(ToolReply::CAP);
    let content = result["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "image");
    assert_eq!(content[1]["mimeType"], "image/png");
    let data = content[1]["data"].as_str().unwrap();
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .unwrap(),
        png
    );
    assert_eq!(result["isError"], false);
    // The image doesn't count against the text's cap, however near it.
    let big = vec![0; ToolReply::IMAGE_BYTES];
    let result = ToolReply::text("abc").with_png(big).result(3);
    assert_eq!(result["content"][1]["type"], "image");
    assert_eq!(result["isError"], false);
    // Past its own bound: left out, saying so.
    let result = (ToolReply::text("abc"))
        .with_png(vec![0; ToolReply::IMAGE_BYTES + 1])
        .result(ToolReply::CAP);
    assert_eq!(result["content"].as_array().unwrap().len(), 2);
    assert_eq!(result["content"][1]["type"], "text");
    assert_eq!(result["isError"], true);
}

/// The creating tools read the names asked for what they make: the
/// feature's, and its new bodies' where it can make some.
#[test]
fn creating_tools_read_names() {
    let read = call(
        "extrude",
        json!({ "sketch": "S", "profiles": "all", "name": "Base", "body_name": "Plate" }),
    );
    let Ok(Call::Extrude(extrude)) = read else {
        panic!("{read:?}")
    };
    assert_eq!(extrude.naming.name.as_deref(), Some("Base"));
    assert_eq!(extrude.naming.bodies, ["Plate"]);
    let read = call(
        "pattern",
        json!({ "bodies": ["B"], "body_names": ["One", "Two"], "join": false }),
    );
    let Ok(Call::Pattern(pattern)) = read else {
        panic!("{read:?}")
    };
    assert_eq!(pattern.naming.bodies, ["One", "Two"]);
    let read = call("add_sketch", json!({ "plane": "xy", "name": "Profile" }));
    assert!(
        matches!(&read, Ok(Call::AddSketch { name: Some(name), .. }) if name == "Profile"),
        "{read:?}"
    );
    // A combine makes no body; pattern's are a list; names are names.
    for (tool, arguments) in [
        (
            "combine",
            json!({ "target": "A", "tools": ["B"], "body_name": "C" }),
        ),
        ("pattern", json!({ "bodies": ["B"], "body_name": "C" })),
        (
            "extrude",
            json!({ "sketch": "S", "profiles": "all", "name": " " }),
        ),
        (
            "extrude",
            json!({ "sketch": "S", "profiles": "all", "name": "x".repeat(NAME_BYTES + 1) }),
        ),
        ("add_sketch", json!({ "plane": "xy", "name": "" })),
    ] {
        let read = call(tool, arguments);
        assert!(read.is_err(), "{tool}: {read:?}");
    }
}

#[test]
fn reads_undo() {
    assert_eq!(call("undo", json!({})), Ok(Call::Undo { steps: 1 }));
    assert_eq!(
        call("undo", json!({ "steps": 3 })),
        Ok(Call::Undo { steps: 3 })
    );
    assert_eq!(
        call("undo", json!({ "steps": MAX_UNDO_STEPS })),
        Ok(Call::Undo {
            steps: MAX_UNDO_STEPS
        })
    );
    for steps in [
        json!(0),
        json!(MAX_UNDO_STEPS + 1),
        json!(-1),
        json!(1.5),
        json!("2"),
    ] {
        let refused = call("undo", json!({ "steps": steps }));
        assert!(
            matches!(refused, Err(CallError::Arguments { .. })),
            "{refused:?}"
        );
    }
    let refused = call("undo", json!({ "count": 2 }));
    assert!(
        matches!(refused, Err(CallError::Arguments { .. })),
        "{refused:?}"
    );
    assert!(Call::Undo { steps: 1 }.edits());
    let list = list();
    let undo = (list["tools"].as_array().unwrap().iter())
        .find(|tool| tool["name"] == "undo")
        .unwrap();
    assert_eq!(undo["annotations"]["destructiveHint"], true);
    assert_eq!(undo["annotations"]["idempotentHint"], false);
    assert!(undo.get("outputSchema").is_none());
}

/// `edit_sketch`'s schema offers each op the arguments its parser
/// takes: none of them is refused as unknown, and a point takes no
/// `construction`.
#[test]
fn edit_sketch_schema_matches_its_parser() {
    let tool = super::draw::edit_sketch_tool();
    let schemas = tool["inputSchema"]["properties"]["operations"]["items"]["oneOf"]
        .as_array()
        .expect("a list of operations");
    assert_eq!(schemas.len(), 11);
    for schema in schemas {
        let properties = schema["properties"].as_object().expect("properties");
        let operation: Map<String, Value> = (properties.iter())
            .map(|(key, property)| match key.as_str() {
                "op" => (key.clone(), property["const"].clone()),
                _ => (key.clone(), Value::Null),
            })
            .collect();
        let word = operation["op"].clone();
        let args = json!({ "sketch": "S", "operations": [operation] });
        if let Err(error) = call("edit_sketch", args) {
            assert!(
                !format!("{error:?}").contains("no argument named"),
                "{word}: {error:?}"
            );
        }
        assert_eq!(
            properties.contains_key("construction"),
            word != "point" && properties.contains_key("label")
        );
    }
    let point = json!({ "op": "point", "at": [0, 0], "construction": true });
    assert!(
        refused(
            "edit_sketch",
            json!({ "sketch": "S", "operations": [point] })
        )
        .contains("no argument named \"construction\"")
    );
}

#[test]
fn feature_tools_are_listed_once_and_named_apart() {
    let listed = list();
    let listed: Vec<&str> = (listed["tools"].as_array().unwrap().iter())
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    for (at, feature) in FeatureTool::ALL.iter().enumerate() {
        let name = feature.name();
        assert!(listed.contains(&name), "{name}");
        assert!(
            FeatureTool::ALL[..at]
                .iter()
                .all(|seen| seen.name() != name),
            "{name}"
        );
    }
}

#[test]
fn every_word_is_looked_up_one_way() {
    assert_eq!(
        call("set_units", json!({ "units": " IN " })),
        Ok(Call::SetUnits(Units::In))
    );
    assert!(refused("set_units", json!({})).contains("units is required"));
    assert_eq!(lookup(OPERATIONS, "Cut"), Some(Operation::Cut));
    assert_eq!(lookup(WORLD_PLANES, " XY"), Some(WorldPlane::Xy));
    assert_eq!(lookup(UNITS, "cm"), None);
}

#[test]
fn a_choice_says_its_tables_word() {
    assert_eq!(Units::Mm.word(), "mm");
    assert_eq!(ScaleMode::PerAxis.word(), "per_axis");
    assert_eq!(ChamferType::DistanceAngle.word(), "distance_angle");
    assert_eq!(LinkKind::Intersect.word(), "intersect");
}

/// Every listed tool is read by its name, takes each argument its schema
/// lists and refuses one it doesn't (unless it takes any, as
/// `set_feature`'s patch does): the catalogue and the parser agree.
#[test]
fn every_listed_tool_reads_the_arguments_it_lists() {
    for tool in list()["tools"].as_array().unwrap() {
        let name = tool["name"].as_str().unwrap();
        let schema = &tool["inputSchema"];
        let properties = schema["properties"].as_object().unwrap();
        let closed = schema["additionalProperties"] == json!(false);
        let unknown = closed.then_some("no_such_key");
        for key in properties.keys().map(String::as_str).chain(unknown) {
            let mut arguments = Map::new();
            arguments.insert(key.to_owned(), Value::Null);
            let refused = match parse_arguments(name, &arguments) {
                Ok(_) => false,
                Err(CallError::Params(error)) => panic!("{name}: {error}"),
                Err(CallError::Arguments { message, .. }) => message.contains("no argument named"),
            };
            assert_eq!(refused, key == "no_such_key", "{name}: {key}");
        }
    }
}
