//! `feature` in its panel's shape and `set_feature`, through
//! `Varde::update`: each kind read and sent back changing nothing, its
//! shape read by its own tool; a field patched per kind, one undo step;
//! a list replaced; refusals, a mode's among them, and a stale key in a
//! patch, a revolve's and a move's.

use serde_json::{Map, Value, json};
use varde_document::FeatureKind;

use super::motion::{edge_between, items, key_of, with_disc};
use super::sweep::with_sections;
use super::*;

/// The settings `feature` gives for `name`, and the tool they're of.
fn shape(varde: &Varde, name: &str) -> (Map<String, Value>, String) {
    let listed = data(varde, &feature(name));
    let settings = listed["settings"].as_object().unwrap().clone();
    (settings, listed["tool"].as_str().unwrap().to_owned())
}

/// `set_feature` of `name` with `patch`.
fn set(varde: &mut Varde, requests: &Lane, name: &str, patch: Value) -> (ToolReply, McpCallState) {
    let mut arguments = patch.as_object().unwrap().clone();
    arguments.insert("name".to_owned(), json!(name));
    edit(varde, requests, "set_feature", Value::Object(arguments))
}

/// Whether `value` holds a reference said to be unresolved.
fn unresolved(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.contains_key("unresolved") || map.values().any(unresolved),
        Value::Array(items) => items.iter().any(unresolved),
        _ => false,
    }
}

/// The name of the feature added last.
fn last_name(varde: &Varde) -> String {
    let document = varde.screen.doc().unwrap().editor.document();
    document.features().last().unwrap().name.clone()
}

/// The key of the first of `body`'s `list`.
fn first_key(varde: &Varde, body: &str, list: &str) -> String {
    items(varde, body, list)[0]["key"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Each of the sixteen kinds, added by its tool, reads in its tool's
/// shape (its own parser takes it, references resolved) and sent back
/// whole changes nothing.
#[test]
fn every_kind_reads_in_its_tools_shape_and_round_trips() {
    let (mut varde, requests) = with_sections();
    type Arguments = Box<dyn Fn(&Varde) -> Value>;
    let fixed = |value: Value| -> Arguments { Box::new(move |_| value.clone()) };
    let calls: Vec<(&str, Arguments)> = vec![
        (
            "extrude",
            fixed(
                json!({ "sketch": "Sketch 2", "profiles": [0], "distance": "10",
                          "extent": "two_sides", "taper": "2 deg" }),
            ),
        ),
        (
            "revolve",
            fixed(
                json!({ "sketch": "Sketch 3", "profiles": "all", "axis": "y",
                          "extent": "one_side", "angle": "90 deg" }),
            ),
        ),
        (
            "combine",
            fixed(json!({ "target": "Body 1", "tools": ["Body 2"], "keep_tools": true })),
        ),
        (
            "move_body",
            fixed(json!({ "bodies": ["Body 2"], "x": "5", "angle": "10 deg" })),
        ),
        (
            "mirror",
            fixed(json!({ "bodies": ["Body 2"], "plane": "yz" })),
        ),
        (
            "pattern",
            fixed(json!({ "bodies": ["Body 3"], "count": 2 })),
        ),
        (
            "pattern",
            fixed(
                json!({ "kind": "circular", "bodies": ["Body 3"], "mode": "total",
                          "spread": "90 deg" }),
            ),
        ),
        (
            "align",
            Box::new(|varde: &Varde| {
                let corner = first_key(varde, "Body 1", "corners");
                json!({ "body": "Body 1", "from": { "point": { "corner": corner } },
                        "to": { "point": "origin" } })
            }),
        ),
        (
            "scale",
            fixed(json!({ "bodies": ["Body 2"], "factor": "2" })),
        ),
        (
            "split",
            fixed(json!({ "body": "Body 1", "tool": "xz", "original": "back" })),
        ),
        (
            "shell",
            fixed(json!({ "body": "Body 2", "thickness": "1" })),
        ),
        (
            "offset_face",
            Box::new(|varde: &Varde| json!({ "faces": [first_key(varde, "Body 1", "faces")] })),
        ),
        (
            "draft",
            Box::new(
                |varde: &Varde| json!({ "faces": [first_key(varde, "Body 1", "faces")], "neutral": "xz" }),
            ),
        ),
        (
            "chamfer",
            Box::new(|varde: &Varde| {
                json!({ "edges": [first_key(varde, "Body 1", "edges")],
                        "type": "two_distances", "flip": true })
            }),
        ),
        (
            "fillet",
            Box::new(|varde: &Varde| json!({ "edges": [first_key(varde, "Body 1", "edges")] })),
        ),
        (
            "sweep",
            fixed(json!({ "sketch": "Sketch 2", "profiles": [0], "helix": { "axis": "z" } })),
        ),
        (
            "loft",
            fixed(json!({ "sections": [{ "sketch": "Sketch 2", "region": 0 },
                                       { "sketch": "Sketch 3", "region": 0 }],
                          "mode": "ruled" })),
        ),
    ];
    let mut tools = std::collections::BTreeSet::new();
    for (tool, arguments) in calls {
        let arguments = arguments(&varde);
        let (reply, state) = edit(&mut varde, &requests, tool, arguments);
        assert_eq!(state, McpCallState::Applied, "{tool}: {}", reply.text);
        let name = last_name(&varde);
        let (settings, shown) = shape(&varde, &name);
        assert_eq!(shown, tool);
        tools.insert(tool);
        // Read by its own tool's parser as a call of it.
        if !unresolved(&Value::Object(settings.clone())) {
            let call = varde_mcp::tools::parse_arguments(tool, &settings);
            assert!(call.is_ok(), "{tool}: {call:?} from {settings:?}");
        }
        let generation = varde.screen.doc().unwrap().editor.generation();
        let (reply, _) = set(&mut varde, &requests, &name, Value::Object(settings));
        assert_eq!(reply, ToolReply::text(edit::NOTHING_TO_CHANGE), "{tool}");
        assert_eq!(varde.screen.doc().unwrap().editor.generation(), generation);
    }
    assert_eq!(tools.len(), 16);
}

/// The example with an extrude of the square ("Extrude 2", Body 2), a
/// revolve of Sketch 3 about its y axis ("Revolve 1", Body 3) and a
/// combine of Body 2 into Body 1 ("Combine 1").
fn with_three() -> (Varde, Lane) {
    let (mut varde, requests) = with_sections();
    for (tool, arguments) in [
        (
            "extrude",
            json!({ "sketch": "Sketch 2", "profiles": [0], "distance": "10" }),
        ),
        (
            "revolve",
            json!({ "sketch": "Sketch 3", "profiles": "all", "axis": "y",
                    "extent": "one_side", "angle": "90 deg" }),
        ),
        (
            "combine",
            json!({ "target": "Body 1", "tools": ["Body 2"], "keep_tools": true }),
        ),
    ] {
        let (reply, state) = edit(&mut varde, &requests, tool, arguments);
        assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    }
    (varde, requests)
}

/// The feature named `name`.
fn named(varde: &Varde, name: &str) -> varde_document::Feature {
    let document = varde.screen.doc().unwrap().editor.document();
    (document.features().iter())
        .find(|feature| feature.name == name)
        .unwrap()
        .clone()
}

/// A field patched per kind: only it changes, as the panel's OK on the
/// edited feature sets it, one undo step, the result naming the feature.
#[test]
fn set_feature_patches_one_field_per_kind() {
    let (mut varde, requests) = with_three();
    let before = named(&varde, "Extrude 2");
    let (reply, state) = set(
        &mut varde,
        &requests,
        "Extrude 2",
        json!({ "distance": "15 mm" }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    assert_eq!(
        edit_data(reply),
        json!({ "feature": "Extrude 2", "id": before.id.get(), "bodies": ["Body 2"],
                "result": "applied" })
    );
    assert_eq!(varde.mcp.log.back().unwrap().args, "Extrude 2: distance");
    let after = extrude_of(&named(&varde, "Extrude 2"));
    let varde_document::Extent::OneSide(distance) = &after.extent else {
        panic!("{:?}", after.extent);
    };
    assert_eq!((distance.value, distance.text.as_str()), (15.0, "15 mm"));
    let old = extrude_of(&before);
    assert_eq!(
        (&after.regions, &after.operation, after.flip),
        (&old.regions, &old.operation, old.flip)
    );
    assert_eq!(shape(&varde, "Extrude 2").0["distance"], json!("15 mm"));
    undo(&mut varde);
    assert_eq!(named(&varde, "Extrude 2"), before);
    regenerate(&mut varde, &requests);

    // A revolve's extent and angles; its mode's fields only.
    let (reply, _) = set(
        &mut varde,
        &requests,
        "Revolve 1",
        json!({ "extent": "two_sides", "second_angle": "45 deg" }),
    );
    edit_data(reply);
    let revolve = revolve_of(&named(&varde, "Revolve 1"));
    let varde_document::Turn::TwoSides(first, second) = &revolve.extent else {
        panic!("{:?}", revolve.extent);
    };
    assert!((first.value - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert!((second.value - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
    assert_eq!(revolve.axis, varde_document::AxisLine::SketchY);

    // A combine's Keep tools.
    let (reply, _) = set(
        &mut varde,
        &requests,
        "Combine 1",
        json!({ "keep_tools": false }),
    );
    edit_data(reply);
    let FeatureKind::Combine(combine) = named(&varde, "Combine 1").kind else {
        panic!("not a combine");
    };
    assert!(!combine.keep_tools);
    assert_eq!(combine.tools.len(), 1);

    // An extrude's operation and extent, the bodies a cut leaves out
    // (Body 2 no longer one the combine could use).
    let (mut varde, requests) = with_sections();
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "extrude",
        json!({ "sketch": "Sketch 2", "profiles": [0], "distance": "10" }),
    );
    edit_data(reply);
    let (reply, state) = set(
        &mut varde,
        &requests,
        "Extrude 2",
        json!({ "operation": "cut", "extent": "through_all", "exclude": ["Body 1"] }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let extrude = extrude_of(&named(&varde, "Extrude 2"));
    assert_eq!(extrude.extent, varde_document::Extent::ThroughAll);
    let body = varde.screen.doc().unwrap().editor.document().bodies()[0].id;
    assert_eq!(extrude.operation.excluded(), &[body]);
}

/// What `set_feature` refuses, in the tools' words: a field its kind
/// hasn't, one of another mode (a scale's too), null, a sketch, a value
/// its field refuses, a name not found.
#[test]
fn set_feature_refuses_what_the_panel_would_not_take() {
    let (mut varde, requests) = with_three();
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "scale",
        json!({ "bodies": ["Body 3"] , "factor": "2" }),
    );
    edit_data(reply);
    let scale = last_name(&varde);
    let set_call = |name: &str, patch: Value| {
        let mut arguments = patch.as_object().unwrap().clone();
        arguments.insert("name".to_owned(), json!(name));
        Value::Object(arguments)
    };
    for (name, patch, why) in [
        (
            "Extrude 2",
            json!({ "keep_tools": true }),
            "an extrude has no field \"keep_tools\"",
        ),
        (
            "Extrude 2",
            json!({ "second_distance": "4" }),
            "second_distance goes with extent two_sides",
        ),
        (
            "Revolve 1",
            json!({ "extent": "full", "angle": "30 deg" }),
            "angle goes with extent one_side, symmetric or two_sides",
        ),
        ("Extrude 2", json!({ "distance": null }), "distance is null"),
        (
            "Extrude 2",
            json!({ "distance": "5 deg" }),
            "set_feature: distance: ",
        ),
        (
            "Extrude 2",
            json!({ "extent": "far" }),
            "set_feature: extent is",
        ),
        (
            "Extrude 2",
            json!({ "extent": "through_all" }),
            "through_all goes with operation cut only",
        ),
        (
            "Extrude 2",
            json!({ "sketch": "Sketch 3" }),
            "sketch goes with profiles",
        ),
        (
            "Extrude 2",
            json!({ "exclude": ["Body 1"] }),
            "exclude goes with operation join, cut or intersect",
        ),
        (
            "Combine 1",
            json!({ "tools": ["Body 1"] }),
            "the target can't be one of the tools",
        ),
        ("Sketch 2", json!({ "plane": "xz" }), "a sketch's settings"),
        (
            scale.as_str(),
            json!({ "x": "3" }),
            "set_feature: x goes with mode per_axis",
        ),
        (
            scale.as_str(),
            json!({ "mode": "per_axis", "factor": "3" }),
            "set_feature: factor goes with mode uniform",
        ),
        (
            "Extrude 9",
            json!({ "distance": "1" }),
            "no feature is named \"Extrude 9\"",
        ),
    ] {
        wrong_edit(
            &mut varde,
            &requests,
            "set_feature",
            set_call(name, patch),
            why,
        );
    }
}

/// A revolve's axis patched with an edge's key: taken while the edge is
/// where the key says, refused saying to ask again once it moved.
#[test]
fn set_feature_takes_a_key_and_refuses_a_stale_one() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let (disc, _) = disc_and_plate(&varde);
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "revolve",
        json!({ "sketch": "Sketch 1", "profiles": [disc], "axis": "x",
                "extent": "one_side", "angle": "90 deg" }),
    );
    edit_data(reply);
    let key = bottom_edge_key(&varde);
    let (reply, state) = set(
        &mut varde,
        &requests,
        "Revolve 1",
        json!({ "axis": { "edge": key } }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let revolve = revolve_of(&named(&varde, "Revolve 1"));
    assert!(matches!(revolve.axis, varde_document::AxisLine::Edge(_)));
    // Read back, it's the key of the committed model.
    let shown = shape(&varde, "Revolve 1").0;
    assert!(shown["axis"]["edge"].is_string(), "{shown:?}");
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    // The plate made symmetric: its bottom edge moved, the key stale.
    let doc = varde.screen.doc_mut().unwrap();
    let plate = doc.editor.document().features()[1].clone();
    let mut extrude = extrude_of(&plate);
    let varde_document::Extent::OneSide(distance) = extrude.extent.clone() else {
        panic!();
    };
    extrude.extent = varde_document::Extent::Symmetric(distance);
    doc.apply(Command::SetFeature {
        feature: plate.id,
        kind: Box::new(extrude.into()),
    });
    doc.sync();
    regenerate(&mut varde, &requests);
    let mut arguments = Map::new();
    arguments.insert("name".to_owned(), json!("Revolve 1"));
    arguments.insert("axis".to_owned(), json!({ "edge": key }));
    wrong_edit(
        &mut varde,
        &requests,
        "set_feature",
        Value::Object(arguments),
        "that edge isn't in the model any more: ask body_topology again",
    );
}

/// Each motion-panel kind, added by its tool, then a field patched:
/// applied, read back as given, the rest as it was.
#[test]
fn set_feature_patches_one_field_per_motion_kind() {
    let (mut varde, requests) = with_three();
    type Arguments = Box<dyn Fn(&Varde) -> Value>;
    let fixed = |value: Value| -> Arguments { Box::new(move |_| value.clone()) };
    let face = |varde: &Varde| first_key(varde, "Body 1", "faces");
    let edge = |varde: &Varde| first_key(varde, "Body 1", "edges");
    let calls: Vec<(&str, Arguments, Value)> = vec![
        (
            "move_body",
            fixed(json!({ "bodies": ["Body 2"], "x": "5" })),
            json!({ "y": "3 mm" }),
        ),
        (
            "mirror",
            fixed(json!({ "bodies": ["Body 2"], "plane": "yz" })),
            json!({ "keep_original": false }),
        ),
        (
            "pattern",
            fixed(json!({ "bodies": ["Body 3"], "count": 2 })),
            json!({ "count": "4" }),
        ),
        (
            "pattern",
            fixed(
                json!({ "kind": "circular", "bodies": ["Body 3"], "mode": "total",
                        "spread": "90 deg" }),
            ),
            json!({ "spread": "120 deg" }),
        ),
        (
            "align",
            Box::new(|varde: &Varde| {
                let corner = first_key(varde, "Body 1", "corners");
                let edge = first_key(varde, "Body 1", "edges");
                json!({ "body": "Body 1",
                        "from": { "point": { "corner": corner }, "direction": { "edge": edge } },
                        "to": { "point": "origin", "direction": "z" } })
            }),
            json!({ "distance": "2 mm" }),
        ),
        (
            "scale",
            fixed(json!({ "bodies": ["Body 2"], "factor": "2" })),
            json!({ "factor": "3" }),
        ),
        (
            "split",
            fixed(json!({ "body": "Body 1", "tool": "xz" })),
            json!({ "keep": "front" }),
        ),
        (
            "shell",
            fixed(json!({ "body": "Body 2", "thickness": "1" })),
            json!({ "direction": "outward" }),
        ),
        (
            "offset_face",
            Box::new(move |varde: &Varde| json!({ "faces": [face(varde)] })),
            json!({ "distance": "2 mm" }),
        ),
        (
            "draft",
            Box::new(move |varde: &Varde| json!({ "faces": [face(varde)], "neutral": "xz" })),
            json!({ "angle": "5 deg" }),
        ),
        (
            "chamfer",
            Box::new(
                move |varde: &Varde| json!({ "edges": [edge(varde)], "type": "two_distances" }),
            ),
            json!({ "second_distance": "2.5 mm" }),
        ),
        (
            "fillet",
            Box::new(move |varde: &Varde| json!({ "edges": [edge(varde)] })),
            json!({ "radius": "2.5 mm" }),
        ),
        (
            "sweep",
            fixed(json!({ "sketch": "Sketch 2", "profiles": [0], "helix": { "axis": "z" } })),
            json!({ "helix": { "axis": "x", "turns": "3" } }),
        ),
        (
            "loft",
            fixed(json!({ "sections": [{ "sketch": "Sketch 2", "region": 0 },
                                       { "sketch": "Sketch 3", "region": 0 }],
                          "mode": "ruled" })),
            json!({ "mode": "smooth" }),
        ),
    ];
    for (tool, arguments, patch) in calls {
        let arguments = arguments(&varde);
        let (reply, state) = edit(&mut varde, &requests, tool, arguments);
        assert_eq!(state, McpCallState::Applied, "{tool}: {}", reply.text);
        let name = last_name(&varde);
        let (before, _) = shape(&varde, &name);
        let (reply, state) = set(&mut varde, &requests, &name, patch.clone());
        assert_eq!(state, McpCallState::Applied, "{tool}: {}", reply.text);
        let (after, _) = shape(&varde, &name);
        for (key, value) in patch.as_object().unwrap() {
            match (value, &after[key]) {
                // An object given whole reads back with its defaults.
                (Value::Object(given), Value::Object(shown)) => {
                    for (inner, value) in given {
                        assert_eq!(&shown[inner], value, "{tool}: {key}.{inner}");
                    }
                }
                (value, shown) => assert_eq!(shown, value, "{tool}: {key}"),
            }
        }
        // The rest as it was (a split's original shows with keep both
        // only; keys carry the model's generation, so their count).
        for (key, value) in &before {
            if ["faces", "edges"].contains(&key.as_str()) {
                let count = |value: &Value| value.as_array().map(Vec::len);
                assert_eq!(count(&after[key]), count(value), "{tool}: {key}");
            } else if !patch.as_object().unwrap().contains_key(key)
                && !(tool == "split" && key == "original")
            {
                assert_eq!(&after[key], value, "{tool}: {key}");
            }
        }
    }
}

/// A list a patch gives replaces the feature's own, rather than toggling
/// what it names as clicks on it would: a fillet's edges, a move's
/// bodies.
#[test]
fn set_feature_replaces_a_list() {
    let (mut varde, requests) = with_sections();
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "extrude",
        json!({ "sketch": "Sketch 2", "profiles": [0], "distance": "10" }),
    );
    edit_data(reply);
    let edges = items(&varde, "Body 1", "edges");
    let key = |at: usize| edges[at]["key"].as_str().unwrap().to_owned();
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "fillet",
        json!({ "edges": [key(0), key(1)] }),
    );
    edit_data(reply);
    let fillet = last_name(&varde);
    let FeatureKind::Fillet(before) = named(&varde, &fillet).kind else {
        panic!("not a fillet");
    };
    assert_eq!(before.edges.len(), 2);
    // Edge 1 again with another: had the clicks toggled, it would go.
    let (reply, state) = set(
        &mut varde,
        &requests,
        &fillet,
        json!({ "edges": [key(1), key(2)] }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let FeatureKind::Fillet(after) = named(&varde, &fillet).kind else {
        panic!("not a fillet");
    };
    assert_eq!(after.edges.len(), 2);
    assert_ne!(after.edges, before.edges);
    let kept = (before.edges.iter()).filter(|edge| after.edges.contains(edge));
    assert_eq!(kept.count(), 1, "the edge given again is kept");

    let (reply, _) = edit(
        &mut varde,
        &requests,
        "move_body",
        json!({ "bodies": ["Body 1", "Body 2"], "x": "5" }),
    );
    edit_data(reply);
    let moved = last_name(&varde);
    let (reply, state) = set(
        &mut varde,
        &requests,
        &moved,
        json!({ "bodies": ["Body 2"] }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let FeatureKind::Move(after) = named(&varde, &moved).kind else {
        panic!("not a move");
    };
    let document = varde.screen.doc().unwrap().editor.document();
    let body = |name: &str| {
        (document.bodies().iter())
            .find(|b| b.name == name)
            .unwrap()
            .id
    };
    assert_eq!(after.bodies, vec![body("Body 2")]);
}

/// A motion's patch refused for a mode the result doesn't show, in the
/// panel's words: a chamfer's Flip with equal distances, a helix's twist,
/// a circular pattern's Flip, a pattern's kind.
#[test]
fn set_feature_refuses_a_motion_field_its_mode_does_not_show() {
    let (mut varde, requests) = with_sections();
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "extrude",
        json!({ "sketch": "Sketch 2", "profiles": [0], "distance": "10" }),
    );
    edit_data(reply);
    let edge = first_key(&varde, "Body 1", "edges");
    for (tool, arguments) in [
        ("chamfer", json!({ "edges": [edge] })),
        (
            "sweep",
            json!({ "sketch": "Sketch 3", "profiles": [0], "helix": { "axis": "z" } }),
        ),
        (
            "pattern",
            json!({ "kind": "circular", "bodies": ["Body 1"] }),
        ),
    ] {
        let (reply, state) = edit(&mut varde, &requests, tool, arguments);
        assert_eq!(state, McpCallState::Applied, "{tool}: {}", reply.text);
    }
    let names: Vec<String> = {
        let document = varde.screen.doc().unwrap().editor.document();
        let features = document.features();
        features[features.len() - 3..]
            .iter()
            .map(|f| f.name.clone())
            .collect()
    };
    for (name, patch, why) in [
        (
            &names[0],
            json!({ "flip": true }),
            "set_feature: flip goes with type two_distances or distance_angle",
        ),
        (
            &names[0],
            json!({ "angle": "30 deg" }),
            "set_feature: angle goes with type distance_angle",
        ),
        (
            &names[1],
            json!({ "twist": "10 deg" }),
            "set_feature: twist goes with a path",
        ),
        (
            &names[2],
            json!({ "flip": true }),
            "set_feature: flip goes with kind linear only",
        ),
        (
            &names[2],
            json!({ "mode": "full", "spread": "30 deg" }),
            "set_feature: spread goes with mode spacing or total",
        ),
        (
            &names[2],
            json!({ "kind": "linear" }),
            "set_feature: a pattern's kind stays as it is",
        ),
    ] {
        let mut arguments = patch.as_object().unwrap().clone();
        arguments.insert("name".to_owned(), json!(name));
        wrong_edit(
            &mut varde,
            &requests,
            "set_feature",
            Value::Object(arguments),
            why,
        );
    }
    // Its type changed with it, a chamfer's Flip is taken.
    let (reply, state) = set(
        &mut varde,
        &requests,
        &names[0],
        json!({ "type": "two_distances", "flip": true }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
}

/// A move's axis patched with an edge's key: taken while the edge is
/// where the key says, refused saying to ask again once it moved.
#[test]
fn set_feature_refuses_a_stale_key_in_a_motion_patch() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "move_body",
        json!({ "bodies": ["Body 1"], "x": "1" }),
    );
    edit_data(reply);
    let moved = last_name(&varde);
    let key = bottom_edge_key(&varde);
    let patch = json!({ "axis": { "edge": key }, "angle": "10 deg" });
    let (reply, state) = set(&mut varde, &requests, &moved, patch.clone());
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let FeatureKind::Move(after) = named(&varde, &moved).kind else {
        panic!("not a move");
    };
    assert!(matches!(
        after.turn,
        Some((varde_document::AxisRef::Edge(_), _))
    ));
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    // The plate made symmetric: its bottom edge moved, the key stale.
    let doc = varde.screen.doc_mut().unwrap();
    let plate = doc.editor.document().features()[1].clone();
    let mut extrude = extrude_of(&plate);
    let varde_document::Extent::OneSide(distance) = extrude.extent.clone() else {
        panic!();
    };
    extrude.extent = varde_document::Extent::Symmetric(distance);
    doc.apply(Command::SetFeature {
        feature: plate.id,
        kind: Box::new(extrude.into()),
    });
    doc.sync();
    regenerate(&mut varde, &requests);
    let mut arguments = patch.as_object().unwrap().clone();
    arguments.insert("name".to_owned(), json!(moved));
    wrong_edit(
        &mut varde,
        &requests,
        "set_feature",
        Value::Object(arguments),
        "that edge isn't in the model any more: ask body_topology again",
    );
}

/// A field sent as `feature` shows it but going with one that changes
/// is taken with it: an extrude's profiles with its new sketch.
#[test]
fn set_feature_takes_an_unchanged_field_going_with_a_changed_one() {
    let (mut varde, requests) = with_three();
    assert_eq!(shape(&varde, "Extrude 2").0["profiles"], json!([0]));
    let (reply, state) = set(
        &mut varde,
        &requests,
        "Extrude 2",
        json!({ "sketch": "Sketch 3", "profiles": [0] }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    assert_eq!(shape(&varde, "Extrude 2").0["sketch"], json!("Sketch 3"));
}

/// A value the feature had none for, its mode changed, is the client's
/// default as a new feature's is, not one following the user's view: a
/// through-all extrude's distance, an equal chamfer's second distance.
#[test]
fn set_feature_fills_a_new_modes_value_with_the_clients_default() {
    let (mut varde, requests) = with_three();
    // The user's view far out, which a panel's fresh lengths follow.
    let doc = varde.screen.doc_mut().unwrap();
    let height = doc.camera.view_height();
    doc.camera.set_view_height(height * 1000.0);
    let (reply, state) = set(
        &mut varde,
        &requests,
        "Extrude 2",
        json!({ "operation": "cut", "extent": "through_all" }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let (reply, state) = set(
        &mut varde,
        &requests,
        "Extrude 2",
        json!({ "operation": "cut", "extent": "one_side" }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    assert_eq!(shape(&varde, "Extrude 2").0["distance"], json!("10 mm"));

    let edge = first_key(&varde, "Body 1", "edges");
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "chamfer",
        json!({ "edges": [edge.clone()], "type": "two_distances" }),
    );
    edit_data(reply);
    let fresh = shape(&varde, &last_name(&varde)).0["second_distance"].clone();
    undo(&mut varde);
    regenerate(&mut varde, &requests);
    let (reply, _) = edit(&mut varde, &requests, "chamfer", json!({ "edges": [edge] }));
    edit_data(reply);
    let chamfer = last_name(&varde);
    let (reply, state) = set(
        &mut varde,
        &requests,
        &chamfer,
        json!({ "type": "two_distances" }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    assert_eq!(shape(&varde, &chamfer).0["second_distance"], fresh);
}

/// What a patch would change only where it's ignored is refused: an
/// extrude's Flip with symmetric, a cut through all made a join (which
/// would go one side unsaid), a scale's Along its axis only kept for a
/// round edge given.
#[test]
fn set_feature_refuses_what_its_mode_would_ignore() {
    let (mut varde, requests) = with_three();
    let (reply, state) = set(
        &mut varde,
        &requests,
        "Extrude 2",
        json!({ "extent": "symmetric" }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let call = |name: &str, patch: Value| {
        let mut arguments = patch.as_object().unwrap().clone();
        arguments.insert("name".to_owned(), json!(name));
        Value::Object(arguments)
    };
    wrong_edit(
        &mut varde,
        &requests,
        "set_feature",
        call("Extrude 2", json!({ "flip": true })),
        "set_feature: flip goes with extent one_side or two_sides",
    );
    let (reply, state) = set(
        &mut varde,
        &requests,
        "Extrude 2",
        json!({ "operation": "cut", "extent": "through_all" }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    wrong_edit(
        &mut varde,
        &requests,
        "set_feature",
        call("Extrude 2", json!({ "operation": "join" })),
        "set_feature: through_all goes with operation cut only",
    );

    let (mut varde, requests) = with_disc();
    let long = edge_between(&varde, "Body 1", [-30.0, -20.0, 0.0], [30.0, -20.0, 0.0]);
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "scale",
        json!({ "bodies": ["Body 1"], "mode": "edge_length", "edge": long,
                "length": "120", "axis_only": true }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let scale = last_name(&varde);
    // A round edge of the plate's hole, in the model the scale made.
    let rim = key_of(&varde, "Body 1", "edges", |edge| {
        edge["form"] != json!("line")
    });
    wrong_edit(
        &mut varde,
        &requests,
        "set_feature",
        call(&scale, json!({ "edge": rim })),
        "set_feature: axis_only goes with a straight edge along a world axis",
    );
}

/// `feature {name, offset?, limit?}` through `Varde::update`.
/// The regeneration it may wait for answered from `requests`.
fn read(varde: &mut Varde, requests: &Lane, name: &str, page: Option<(usize, usize)>) -> Value {
    let page = page.map_or(Page::FEATURE, |(offset, limit)| Page { offset, limit });
    let id = (varde.mcp.log.back()).map_or(1, |logged| i64::try_from(logged.serial).unwrap() + 1);
    let call = Call::Feature {
        name: name.to_owned(),
        page,
    };
    let reply = calling(varde, "a session", id, call);
    regenerate(varde, requests);
    let reply = reply.try_recv().unwrap();
    assert!(!reply.is_error, "{}", reply.text);
    assert!(
        reply.text.len() <= ToolReply::CAP,
        "{} bytes",
        reply.text.len()
    );
    reply.structured.unwrap()
}

/// The example with a chamfer added on its body naming `edges` edges
/// (made up, so each is said to be unresolved: a long item).
fn with_chamfer(edges: usize) -> (Varde, Lane) {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let doc = varde.screen.doc_mut().unwrap();
    let document = doc.editor.document();
    let body = document.bodies()[0].id;
    let extrude = document.features()[1].id.get();
    let key = |part| FaceKey {
        feature: extrude,
        part,
        instance: 0,
    };
    let edges = (0..edges)
        .map(|at| EdgeRef {
            body,
            faces: [key(PartKey::EndCap), key(PartKey::Side { curve: 1 })],
            near: glam::DVec3::new(at as f64, 0.0, 10.0),
        })
        .collect();
    let chamfer = varde_document::Chamfer {
        edges,
        faces: Vec::new(),
        distances: varde_document::ChamferSize::Equal(crate::tests::length(document, "1")),
        chains: false,
        flip: false,
    };
    doc.apply(document.add_feature(FeatureKind::Chamfer(chamfer)));
    doc.sync();
    (varde, requests)
}

/// A chamfer naming 256 edges reads a page at a time, each within the
/// cap, the page saying it's part of the list and a note naming it; a
/// page sent back to `set_feature` is refused, the list left out isn't.
#[test]
fn a_long_list_is_paged() {
    let (mut varde, requests) = with_chamfer(256);
    let name = last_name(&varde);
    let first = read(&mut varde, &requests, &name, None);
    let edges = &first["settings"]["edges"];
    assert_eq!(edges["total"], json!(256));
    let shown = edges["items"].as_array().unwrap().len();
    assert_eq!(shown, varde_mcp::tools::FEATURE_LIMIT);
    assert_eq!(edges["left_out"], json!(256 - shown));
    // The empty face list is whole, a plain array.
    assert_eq!(first["settings"]["faces"], json!([]));
    let note = first["note"].as_str().unwrap();
    assert!(note.starts_with("edges is only part of the list"), "{note}");
    assert!(note.contains("set_feature"), "{note}");
    // Page by page to the end, every edge once.
    let mut seen = shown;
    while seen < 256 {
        let next = read(&mut varde, &requests, &name, Some((seen, 100)));
        let items = next["settings"]["edges"]["items"].as_array().unwrap();
        assert!(!items.is_empty());
        seen += items.len();
    }
    assert_eq!(seen, 256);
    // Past the end: an empty page saying so.
    let past = read(&mut varde, &requests, &name, Some((300, 10)));
    assert_eq!(past["settings"]["edges"]["items"], json!([]));
    assert!(past["settings"]["edges"]["note"].is_string());
    // Sent back whole, the page is refused, naming it.
    let mut patch = first["settings"].as_object().unwrap().clone();
    patch.insert("distance".to_owned(), json!("2"));
    let generation = varde.screen.doc().unwrap().editor.generation();
    let (reply, state) = set(&mut varde, &requests, &name, Value::Object(patch));
    assert!(reply.is_error, "{}", reply.text);
    assert!(
        reply.text.contains("edges is a page of its list"),
        "{}",
        reply.text
    );
    assert_ne!(state, McpCallState::Applied);
    assert_eq!(varde.screen.doc().unwrap().editor.generation(), generation);
}

/// A list of names at their longest is shortened until the reply fits
/// the cap, the user's changes beside it, at least one item still given.
#[test]
fn a_page_of_long_names_fits_the_cap() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    for _ in 0..17 {
        let (reply, state) = edit(
            &mut varde,
            &requests,
            "extrude",
            json!({ "sketch": "Sketch 1", "profiles": [0], "distance": "5" }),
        );
        assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    }
    let doc = varde.screen.doc_mut().unwrap();
    let bodies: Vec<_> = (doc.editor.document().bodies().iter())
        .map(|body| body.id)
        .collect();
    assert_eq!(bodies.len(), 18);
    for (at, &body) in bodies.iter().enumerate() {
        let long = format!("{at:02}{}", "B".repeat(varde_document::MAX_NAME_LEN - 2));
        let document = doc.editor.document();
        let rename = document
            .rename(varde_document::Named::Body(body), &long)
            .unwrap();
        doc.apply(rename.command);
    }
    let document = doc.editor.document();
    let combine = varde_document::Combine {
        target: bodies[0],
        tools: bodies[1..].to_vec(),
        op: varde_document::BodyOp::Union,
        keep_tools: false,
    };
    doc.apply(document.add_feature(FeatureKind::Combine(combine)));
    doc.sync();
    let name = last_name(&varde);
    assert!(name.starts_with("Combine"), "{name}");
    let listed = read(&mut varde, &requests, &name, None);
    let tools = &listed["settings"]["tools"];
    let shown = tools["items"].as_array().unwrap().len();
    assert!((1..17).contains(&shown), "{shown}");
    assert_eq!(tools["left_out"], json!(17 - shown));
    let note = listed["note"].as_str().unwrap();
    assert!(note.starts_with("tools is only part"), "{note}");
    // The rest, from where that page ended, each within the cap too.
    let rest = read(&mut varde, &requests, &name, Some((shown, 50)));
    let more = rest["settings"]["tools"]["items"].as_array().unwrap().len();
    assert!(more >= 1);
}

/// A short list is a plain array, no note: read whole.
#[test]
fn a_short_list_reads_whole() {
    let (mut varde, requests) = with_chamfer(3);
    let name = last_name(&varde);
    let listed = read(&mut varde, &requests, &name, None);
    assert_eq!(listed["settings"]["edges"].as_array().unwrap().len(), 3);
    assert_eq!(listed.get("note"), None);
    // A limit under the list's length pages it.
    let paged = read(&mut varde, &requests, &name, Some((1, 1)));
    assert_eq!(paged["settings"]["edges"]["total"], json!(3));
    assert_eq!(paged["settings"]["edges"]["left_out"], json!(2));
}
