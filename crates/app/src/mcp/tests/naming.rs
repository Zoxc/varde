//! Naming what the creating tools make, through `Varde::update`: a
//! sketch's, a feature's and its new bodies' names given in the same
//! undo step, a clash numbered as `rename` numbers it, names refused,
//! and `set_feature` leaving renaming to `rename`.

use serde_json::{Value, json};
use varde_mcp::tools::{Call, SetFeature};

use super::*;

/// The names of `varde`'s design's features.
fn feature_names(varde: &Varde) -> Vec<String> {
    let document = varde.screen.doc().unwrap().editor.document();
    (document.features().iter())
        .map(|feature| feature.name.clone())
        .collect()
}

/// The names of `varde`'s design's bodies.
fn body_names(varde: &Varde) -> Vec<String> {
    let document = varde.screen.doc().unwrap().editor.document();
    (document.bodies().iter())
        .map(|body| body.name.clone())
        .collect()
}

/// `call`, already read, in "a session" with regeneration answered: its
/// reply and its row's state.
fn sent(varde: &mut Varde, requests: &Lane, call: Call) -> (ToolReply, McpCallState) {
    let id = (varde.mcp.log.back()).map_or(1, |logged| i64::try_from(logged.serial).unwrap() + 1);
    let reply = calling(varde, "a session", id, call);
    regenerate(varde, requests);
    let reply = reply.try_recv().unwrap();
    (reply, varde.mcp.log.back().unwrap().state)
}

/// The example's disc extruded, with `more` arguments.
fn disc(varde: &Varde, more: Value) -> Value {
    let (disc, _) = disc_and_plate(varde);
    let mut arguments = json!({ "sketch": "Sketch 1", "profiles": [disc], "distance": "20" });
    for (key, value) in more.as_object().unwrap() {
        arguments[key] = value.clone();
    }
    arguments
}

/// `add_sketch` names the sketch as asked, a name taken numbered with a
/// note; its row shows the name.
#[test]
fn add_sketch_takes_a_name() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "add_sketch",
        json!({ "plane": "xz", "name": "  Profile " }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let data = edit_data(reply);
    assert_eq!(data["sketch"], json!("Profile"));
    assert!(data.get("note").is_none(), "{data}");
    assert_eq!(feature_names(&varde)[2], "Profile");
    assert!(
        varde
            .mcp
            .log
            .back()
            .unwrap()
            .args
            .starts_with("  Profile : ")
    );

    let (reply, _) = edit(
        &mut varde,
        &requests,
        "add_sketch",
        json!({ "plane": "xz", "name": "Profile" }),
    );
    let data = edit_data(reply);
    assert_eq!(data["sketch"], json!("Profile (1)"));
    let note = data["note"].as_str().unwrap();
    assert!(note.contains("\"Profile\" was taken"), "{note}");
}

/// `extrude` names the feature and its new body in one undo step; undo
/// takes both back.
#[test]
fn extrude_takes_its_and_its_bodys_names_in_one_step() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let arguments = disc(&varde, json!({ "name": "Peg", "body_name": "Peg body" }));
    let (reply, state) = edit(&mut varde, &requests, "extrude", arguments);
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let data = edit_data(reply);
    assert_eq!(data["feature"], json!("Peg"));
    assert_eq!(data["bodies"], json!(["Peg body"]));
    assert!(varde.mcp.log.back().unwrap().args.starts_with("Peg: "));
    assert!(
        varde
            .mcp
            .log
            .back()
            .unwrap()
            .args
            .contains("making Peg body")
    );

    undo(&mut varde);
    regenerate(&mut varde, &requests);
    assert_eq!(feature_names(&varde), ["Sketch 1", "Extrude 1"]);
    assert_eq!(body_names(&varde), ["Body 1"]);
}

/// Names another feature or body has are numbered as `rename` numbers
/// them, the result saying so; feature and body names are kept apart
/// together.
#[test]
fn names_taken_are_numbered() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let arguments = disc(
        &varde,
        json!({ "name": "Body 1", "body_name": "Extrude 1" }),
    );
    let (reply, _) = edit(&mut varde, &requests, "extrude", arguments);
    let data = edit_data(reply);
    assert_eq!(data["feature"], json!("Body 1 (1)"));
    assert_eq!(data["bodies"], json!(["Extrude 1 (1)"]));
    let note = data["note"].as_str().unwrap();
    assert!(note.contains("\"Body 1\" was taken"), "{note}");
    assert!(note.contains("\"Extrude 1\" was taken"), "{note}");
}

/// A motion kind takes a name; a pattern's separate copies take theirs
/// in the result's order, those left out keeping defaults.
#[test]
fn motion_kinds_take_names() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "mirror",
        json!({ "bodies": ["Body 1"], "plane": "yz", "name": "Mirrored plate" }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    assert_eq!(edit_data(reply)["feature"], json!("Mirrored plate"));
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    let (reply, state) = edit(
        &mut varde,
        &requests,
        "pattern",
        json!({
            "bodies": ["Body 1"], "count": "4", "join": false,
            "name": "Row", "body_names": ["Second", "Third"]
        }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let data = edit_data(reply);
    assert_eq!(data["feature"], json!("Row"));
    let bodies = data["bodies"].as_array().unwrap();
    assert_eq!(bodies[..2], [json!("Second"), json!("Third")]);
    assert!(bodies.len() > 2, "{data}");
    assert!(bodies[2].as_str().unwrap().starts_with("Body "), "{data}");
}

/// Body names a feature makes no body for, or more than it makes, are
/// refused saying why; so is a name empty once trimmed, and one past
/// the bound never reaches the app.
#[test]
fn names_not_taken_are_refused() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let arguments = disc(&varde, json!({ "operation": "join", "body_name": "Peg" }));
    wrong_edit(
        &mut varde,
        &requests,
        "extrude",
        arguments,
        "makes no new body",
    );
    let arguments = json!({ "bodies": ["Body 1"], "body_names": ["A"] });
    wrong_edit(
        &mut varde,
        &requests,
        "pattern",
        arguments,
        "only join false",
    );
    let arguments =
        json!({ "bodies": ["Body 1"], "count": "2", "join": false, "body_names": ["A", "B"] });
    wrong_edit(
        &mut varde,
        &requests,
        "pattern",
        arguments,
        "2 names for the 1 bodies",
    );

    // Empty, as the server would never send it: the app refuses it too.
    let Call::Extrude(mut extrude) = tool_call("extrude", disc(&varde, json!({}))) else {
        unreachable!()
    };
    extrude.naming.name = Some(" ".to_owned());
    let generation = varde.screen.doc().unwrap().editor.generation();
    let (reply, state) = sent(&mut varde, &requests, Call::Extrude(extrude));
    assert!(reply.text.contains("name is empty"), "{}", reply.text);
    assert_eq!(state, McpCallState::Failed);
    assert_eq!(varde.screen.doc().unwrap().editor.generation(), generation);

    for name in [
        json!(""),
        json!("x".repeat(varde_mcp::tools::NAME_BYTES + 1)),
    ] {
        let params =
            json!({ "name": "extrude", "arguments": disc(&varde, json!({ "name": name })) });
        assert!(varde_mcp::tools::parse_call(Some(&params)).is_err());
        let params =
            json!({ "name": "add_sketch", "arguments": { "plane": "xy", "body_name": "A" } });
        assert!(varde_mcp::tools::parse_call(Some(&params)).is_err());
    }
}

/// `set_feature` doesn't rename: names in a patch are refused, pointing
/// to `rename`.
#[test]
fn set_feature_leaves_names_to_rename() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    for key in ["body_name", "name"] {
        let patch = serde_json::Map::from_iter([(key.to_owned(), json!("Base"))]);
        let call = Call::SetFeature(SetFeature {
            name: "Extrude 1".to_owned(),
            patch,
        });
        let generation = varde.screen.doc().unwrap().editor.generation();
        let (reply, _) = sent(&mut varde, &requests, call);
        assert!(reply.is_error, "{}", reply.text);
        assert!(reply.text.contains("rename"), "{}", reply.text);
        assert_eq!(varde.screen.doc().unwrap().editor.generation(), generation);
    }
    let params =
        json!({ "name": "set_feature", "arguments": { "name": "Extrude 1", "body_names": ["A"] } });
    let error = varde_mcp::tools::parse_call(Some(&params)).unwrap_err();
    assert!(format!("{error:?}").contains("rename"), "{error:?}");
    assert_eq!(feature_names(&varde), ["Sketch 1", "Extrude 1"]);
}
