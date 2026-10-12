//! The user's changes since a client's previous call, through
//! `Varde::update`: each kind reported once with old and new values, not
//! again in the next result; the client's own edits left out; the user's
//! undo and redo of them, and an id made again after an undo; a design
//! replaced; the bound; a new session starting clean; the panel's rows.

use serde_json::{Value, json};
use varde_document::{FeatureKind, Named, Opacity, OriginPlane, Plane, Tint, Tolerance};
use varde_expr::LengthUnit;

use super::*;

/// Applies `command` as the user's own edit, and lets regeneration
/// answer.
pub(super) fn user(varde: &mut Varde, requests: &Lane, command: Command) {
    let doc = varde.screen.doc_mut().unwrap();
    doc.apply(command);
    assert!(doc.edit_error.is_none(), "{:?}", doc.edit_error);
    doc.sync();
    regenerate(varde, requests);
}

/// The id the next design_summary call in `session` takes.
pub(super) fn next_id(varde: &Varde) -> i64 {
    (varde.mcp.log.back()).map_or(1, |logged| i64::try_from(logged.serial).unwrap() + 100)
}

/// A design_summary call in `session`: its data.
pub(super) fn summary_in(varde: &mut Varde, session: &str) -> Value {
    let id = next_id(varde);
    let reply = called(varde, session, id, Call::DesignSummary(Page::SUMMARY));
    assert!(!reply.is_error, "{}", reply.text);
    reply.structured.unwrap()
}

pub(super) fn summary(varde: &mut Varde) -> Value {
    summary_in(varde, "a session")
}

pub(super) fn document(varde: &Varde) -> &Document {
    varde.screen.doc().unwrap().editor.document()
}

fn revision(varde: &Varde) -> Value {
    serde_json::to_value(varde.screen.doc().unwrap().editor.revision()).unwrap()
}

/// [`listening_with`] in a session.
pub(super) fn in_session() -> (Varde, Lane) {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", Some("Claude Code"));
    (varde, requests)
}

#[test]
fn each_kind_of_change_is_reported_once() {
    let (mut varde, requests) = in_session();
    let first = summary(&mut varde);
    assert_eq!(first.get("user_changes"), None);
    assert_eq!(first["revision"], revision(&varde));

    let extrude = document(&varde).features()[1].clone();
    let body = document(&varde).bodies()[0].id;
    user(
        &mut varde,
        &requests,
        Command::Rename {
            target: Named::Feature(extrude.id),
            name: "Base".to_owned(),
        },
    );
    let FeatureKind::Extrude(mut flipped) = extrude.kind.clone() else {
        panic!("the example's extrude");
    };
    flipped.flip = !flipped.flip;
    user(
        &mut varde,
        &requests,
        Command::SetFeature {
            feature: extrude.id,
            kind: Box::new(FeatureKind::Extrude(flipped)),
        },
    );
    user(&mut varde, &requests, Command::SetVisible(body, false));
    user(
        &mut varde,
        &requests,
        Command::SetColor(body, Some(Tint::new(200, 20).unwrap())),
    );
    user(
        &mut varde,
        &requests,
        Command::SetOpacity(body, Opacity::new(50).unwrap()),
    );
    user(
        &mut varde,
        &requests,
        Command::AddParam {
            name: "w".to_owned(),
            text: "2 cm".to_owned(),
        },
    );
    let sketch = document(&varde).add_sketch(Plane::Origin(OriginPlane::XY));
    user(&mut varde, &requests, sketch);
    user(&mut varde, &requests, Command::SetUnits(LengthUnit::Cm));
    user(
        &mut varde,
        &requests,
        Command::SetTolerance(Tolerance::new(1e-2).unwrap()),
    );
    user(
        &mut varde,
        &requests,
        Command::SetRollback(Some(extrude.id)),
    );

    let told = summary(&mut varde);
    assert_eq!(told["revision"], revision(&varde));
    let changes = &told["user_changes"];
    // One run of the user's steps: a feature's changes together.
    assert_eq!(
        changes["features"]["changed"],
        json!([{
            "name": "Base",
            "id": extrude.id.get(),
            "renamed": { "old": "Extrude 1", "new": "Base" },
            "settings": { "flip": { "old": false, "new": true } },
        }])
    );
    let added = &changes["features"]["added"];
    assert_eq!(added[0]["name"], json!("Sketch 2"));
    assert_eq!(added[0]["settings"]["plane"], json!("xy"));
    assert_eq!(
        changes["bodies"]["changed"],
        json!([{
            "name": "Body 1",
            "visible": false,
            "color": { "old": null, "new": { "hue": 200, "saturation": 0.2 } },
            "opacity": { "old": 1.0, "new": 0.5 },
        }])
    );
    assert_eq!(changes["parameters"]["added"][0]["name"], json!("w"));
    assert_eq!(changes["units"], json!({ "old": "mm", "new": "cm" }));
    assert_eq!(
        changes["tolerance"],
        json!({ "old": "1 µm", "new": "10 µm" })
    );
    assert_eq!(changes["rollback"], json!({ "old": null, "new": "Base" }));

    // Told once: the next result has none.
    assert_eq!(summary(&mut varde).get("user_changes"), None);

    // Removals, a parameter renamed and set.
    let sketch = document(&varde).features()[2].id;
    user(&mut varde, &requests, Command::RemoveFeature(sketch));
    user(
        &mut varde,
        &requests,
        Command::RenameParam {
            index: 0,
            name: "width".to_owned(),
        },
    );
    user(
        &mut varde,
        &requests,
        Command::SetParam {
            index: 0,
            text: "3 cm".to_owned(),
        },
    );
    let changes = summary(&mut varde)["user_changes"].clone();
    assert_eq!(
        changes["features"]["removed"],
        json!([{ "kind": "Sketch", "name": "Sketch 2", "id": sketch.get() }])
    );
    assert_eq!(
        changes["parameters"]["changed"],
        json!([{
            "name": "width",
            "renamed": { "old": "w", "new": "width" },
            "expression": { "old": "2 cm", "new": "3 cm" },
        }])
    );
}

#[test]
fn the_clients_own_edits_are_not_reported() {
    let (mut varde, requests) = in_session();
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "rename",
        json!({ "feature": "Extrude 1", "to": "Base" }),
    );
    let data = reply.structured.unwrap();
    assert_eq!(data.get("user_changes"), None, "{data}");
    assert_eq!(data["revision"], revision(&varde));

    // The user's edit between two of the client's: only it is told.
    let body = document(&varde).bodies()[0].id;
    user(&mut varde, &requests, Command::SetVisible(body, false));
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "rename",
        json!({ "feature": "Base", "to": "Top" }),
    );
    let changes = reply.structured.unwrap()["user_changes"].clone();
    assert_eq!(
        changes,
        json!({ "bodies": { "changed": [{ "name": "Body 1", "visible": false }] } })
    );
    assert_eq!(summary(&mut varde).get("user_changes"), None);
}

#[test]
fn the_users_undo_and_redo_of_a_client_edit_are_reported() {
    let (mut varde, requests) = in_session();
    edit(
        &mut varde,
        &requests,
        "rename",
        json!({ "feature": "Extrude 1", "to": "Base" }),
    );
    undo(&mut varde);
    regenerate(&mut varde, &requests);
    let changes = summary(&mut varde)["user_changes"].clone();
    assert_eq!(
        changes["undid_your_edits"],
        json!(["rename feature Extrude 1 to Base"])
    );
    // The panel says it as the edit's row does.
    assert_eq!(
        varde.mcp.log.back().unwrap().told[0],
        "undid its edit: renamed feature Extrude 1 to Base"
    );
    assert_eq!(
        changes["features"]["changed"][0]["renamed"],
        json!({ "old": "Base", "new": "Extrude 1" })
    );
    ui(
        &mut varde,
        varde_view::Message::Edit(varde_view::Edit::Redo),
    );
    regenerate(&mut varde, &requests);
    let changes = summary(&mut varde)["user_changes"].clone();
    assert_eq!(
        changes["redid_your_edits"],
        json!(["rename feature Extrude 1 to Base"])
    );
    assert_eq!(changes.get("undid_your_edits"), None);
}

/// Undone past the client's edit, then edited: said first, and an id
/// made again names something new, removed and added.
#[test]
fn an_id_made_again_after_an_undo_is_removed_and_added() {
    let (mut varde, requests) = in_session();
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "add_sketch",
        json!({ "plane": "xy" }),
    );
    assert!(!reply.is_error, "{}", reply.text);
    let theirs = document(&varde).features()[2].id;
    undo(&mut varde);
    let sketch = document(&varde).add_sketch(Plane::Origin(OriginPlane::YZ));
    user(&mut varde, &requests, sketch);
    let ours = document(&varde).features()[2].id;
    assert_eq!(ours, theirs, "undo winds the ids back");

    let changes = summary(&mut varde)["user_changes"].clone();
    assert!(
        changes["note"]
            .as_str()
            .unwrap()
            .contains("undid your edits"),
        "{changes}"
    );
    assert_eq!(
        changes["undid_your_edits"],
        json!(["add_sketch xy"]),
        "{changes}"
    );
    let features = &changes["features"];
    assert_eq!(features["removed"][0]["id"], json!(theirs.get()));
    assert_eq!(features["added"][0]["id"], json!(ours.get()));
    assert_eq!(features["added"][0]["settings"]["plane"], json!("yz"));
    assert_eq!(features.get("changed"), None, "{changes}");
}

#[test]
fn a_replaced_design_is_said_so() {
    let (mut varde, requests) = in_session();
    summary(&mut varde);
    user(&mut varde, &requests, Command::Replace(Box::default()));
    let changes = summary(&mut varde)["user_changes"].clone();
    assert_eq!(
        changes,
        json!({ "replaced": "the design was replaced: query it again" })
    );
}

#[test]
fn many_changes_name_only_the_features() {
    let (mut varde, requests) = in_session();
    for _ in 0..40 {
        let sketch = document(&varde).add_sketch(Plane::Origin(OriginPlane::XY));
        user(&mut varde, &requests, sketch);
    }
    let changes = summary(&mut varde)["user_changes"].clone();
    assert!(changes.get("features").is_none(), "{changes}");
    assert!(changes["bounded"].as_str().is_some());
    let names = changes["features_changed"]["names"].as_array().unwrap();
    assert_eq!(names.len(), 40);
    assert_eq!(names[0], json!("Sketch 2"));
    assert!(serde_json::to_string(&changes).unwrap().len() <= super::super::changes::CHANGES_BYTES);
}

#[test]
fn a_new_session_starts_from_the_design_at_its_initialize() {
    let (mut varde, requests) = in_session();
    let body = document(&varde).bodies()[0].id;
    user(&mut varde, &requests, Command::SetVisible(body, false));
    started(&mut varde, "b session", None);
    assert_eq!(
        summary_in(&mut varde, "b session").get("user_changes"),
        None
    );
    // Its changes from then on are told.
    user(&mut varde, &requests, Command::SetVisible(body, true));
    let changes = summary_in(&mut varde, "b session")["user_changes"].clone();
    assert_eq!(changes["bodies"]["changed"][0]["visible"], json!(true));
}

#[test]
fn the_panel_shows_the_users_edits_and_what_was_told() {
    let (mut varde, requests) = in_session();
    ui(
        &mut varde,
        varde_view::Message::McpPanel(varde_view::McpPanel::Toggle),
    );
    summary(&mut varde);
    let extrude = document(&varde).features()[1].id;
    user(
        &mut varde,
        &requests,
        Command::Rename {
            target: Named::Feature(extrude),
            name: "Bolt hole".to_owned(),
        },
    );
    // Shown as it's made, after the last call.
    let rows = varde.mcp_panel_shown().unwrap().rows;
    assert_eq!(
        rows.last(),
        Some(&varde_view::McpRow::User {
            said: "renamed Extrude 1 to Bolt hole".into(),
            reverted: false
        })
    );
    assert!(on_screen(&varde).iter().any(|text| text == "You:"));

    summary(&mut varde);
    let rows = varde.mcp_panel_shown().unwrap().rows;
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert_eq!(
        rows[1],
        varde_view::McpRow::User {
            said: "renamed Extrude 1 to Bolt hole".into(),
            reverted: false
        }
    );
    let varde_view::McpRow::Call(call) = &rows[2] else {
        panic!("a call row");
    };
    let told = "Told: since its last call, you renamed Extrude 1 to Bolt hole";
    assert_eq!(call.told.as_deref(), Some(told));
    assert!(on_screen(&varde).iter().any(|text| text == told));
    // The client's own edits make no rows of the user's.
    edit(
        &mut varde,
        &requests,
        "rename",
        json!({ "feature": "Bolt hole", "to": "Base" }),
    );
    let rows = varde.mcp_panel_shown().unwrap().rows;
    assert!(matches!(rows.last(), Some(varde_view::McpRow::Call(_))));
}

/// Changed in two runs of the user's, the client's edit between: one
/// entry, from the first old to the last new, and a value back where it
/// was left out.
#[test]
fn a_feature_changed_in_two_runs_is_listed_once() {
    let (mut varde, requests) = in_session();
    summary(&mut varde);
    let extrude = document(&varde).features()[1].id;
    let body = document(&varde).bodies()[0].id;
    let rename = |name: &str| Command::Rename {
        target: Named::Feature(extrude),
        name: name.to_owned(),
    };
    user(&mut varde, &requests, rename("Base"));
    user(&mut varde, &requests, Command::SetVisible(body, false));
    // A client's step between, its reply not yet sent (as with edits
    // queued together): taken as one, it splits the runs.
    user(&mut varde, &requests, Command::SetUnits(LengthUnit::In));
    let doc = varde.screen.doc_mut().unwrap();
    doc.mcp.steps.push_back(super::super::waiting::ClientStep {
        start: 0,
        session: SessionId::for_tests("a session"),
        after: doc.editor.revision(),
        label: "set_units in".to_owned(),
        shown: "Set the units to in".to_owned(),
        told_done: true,
        undone: false,
        feature: None,
    });
    user(&mut varde, &requests, rename("Top"));
    user(&mut varde, &requests, Command::SetVisible(body, true));
    let changes = summary(&mut varde)["user_changes"].clone();
    assert_eq!(
        changes,
        json!({ "features": { "changed": [{
            "name": "Top",
            "id": extrude.get(),
            "renamed": { "old": "Extrude 1", "new": "Top" },
        }] } })
    );
}

/// Pushes a client's step at the design's revision now, as an edit
/// queued with others, its reply not yet sent.
fn client_step(varde: &mut Varde) {
    let doc = varde.screen.doc_mut().unwrap();
    doc.mcp.steps.push_back(super::super::waiting::ClientStep {
        start: 0,
        session: SessionId::for_tests("a session"),
        after: doc.editor.revision(),
        label: "an edit".to_owned(),
        shown: "An edit".to_owned(),
        told_done: true,
        undone: false,
        feature: None,
    });
}

/// Moved between two runs of the user's by a client's step, a value the
/// user set back to where it first was is a change from where the
/// client's step left it, not dropped.
#[test]
fn a_change_after_a_clients_step_on_the_same_thing_is_kept() {
    let (mut varde, requests) = in_session();
    summary(&mut varde);
    let extrude = document(&varde).features()[1].id;
    let body = document(&varde).bodies()[0].id;
    let rename = |name: &str| Command::Rename {
        target: Named::Feature(extrude),
        name: name.to_owned(),
    };
    user(&mut varde, &requests, rename("Base"));
    user(&mut varde, &requests, Command::SetVisible(body, false));
    // The client's: renamed on, shown again.
    user(&mut varde, &requests, rename("Top"));
    client_step(&mut varde);
    user(&mut varde, &requests, Command::SetVisible(body, true));
    client_step(&mut varde);
    user(&mut varde, &requests, rename("Extrude 1"));
    user(&mut varde, &requests, Command::SetVisible(body, false));
    let changes = summary(&mut varde)["user_changes"].clone();
    assert_eq!(
        changes,
        json!({
            "features": { "changed": [{
                "name": "Extrude 1",
                "id": extrude.get(),
                "renamed": { "old": "Top", "new": "Extrude 1" },
            }] },
            "bodies": { "changed": [{ "name": "Body 1", "visible": false }] },
        })
    );
}

/// A client's edit made after the user undid its earlier one isn't
/// reported back as the user's.
#[test]
fn a_clients_edit_after_an_undo_of_its_last_is_not_reported() {
    let (mut varde, requests) = in_session();
    edit(
        &mut varde,
        &requests,
        "rename",
        json!({ "feature": "Extrude 1", "to": "Base" }),
    );
    undo(&mut varde);
    regenerate(&mut varde, &requests);
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "add_sketch",
        json!({ "plane": "xy" }),
    );
    assert!(!reply.is_error, "{}", reply.text);
    let changes = reply.structured.unwrap()["user_changes"].clone();
    assert_eq!(
        changes["undid_your_edits"],
        json!(["rename feature Extrude 1 to Base"])
    );
    assert_eq!(changes.get("note"), None, "{changes}");
    assert_eq!(changes["features"].get("added"), None, "{changes}");
    assert_eq!(
        changes["features"]["changed"][0]["renamed"],
        json!({ "old": "Base", "new": "Extrude 1" })
    );
}

/// A reply sent before the regeneration whose links' amend replaced the
/// state it saw: the state is followed, not taken as undone past.
#[test]
fn a_state_seen_then_amended_is_followed() {
    let (mut varde, requests) = in_session();
    let sketch = document(&varde).add_sketch(Plane::Origin(OriginPlane::XY));
    user(&mut varde, &requests, sketch);
    summary(&mut varde);
    // What `Doc::computed` does as the links are found again.
    let doc = varde.screen.doc_mut().unwrap();
    let before = doc.editor.revision();
    // Standing for the sketch its links found again.
    let sketch = doc.editor.document().features()[2].id;
    doc.editor
        .amend(Command::Rename {
            target: Named::Feature(sketch),
            name: "Relinked".to_owned(),
        })
        .unwrap();
    let now = doc.editor.revision();
    assert_ne!(before, now);
    doc.mcp.amended(before, now);
    doc.sync();
    regenerate(&mut varde, &requests);
    assert_eq!(summary(&mut varde).get("user_changes"), None);
}

/// Names of the longest kind still keep the bounded form in its bound.
#[test]
fn the_bounded_form_keeps_its_bound_with_long_names() {
    let (mut varde, requests) = in_session();
    for at in 0..40 {
        let command = match document(&varde).add_sketch(Plane::Origin(OriginPlane::XY)) {
            Command::AddSketch { plane, .. } => Command::AddSketch {
                name: format!("{at:03}{}", "n".repeat(200)),
                plane,
            },
            _ => unreachable!("a sketch added"),
        };
        user(&mut varde, &requests, command);
    }
    let changes = summary(&mut varde)["user_changes"].clone();
    let bytes = serde_json::to_string(&changes).unwrap().len();
    assert!(bytes <= super::super::changes::CHANGES_BYTES, "{bytes}");
    let named = &changes["features_changed"];
    assert!(named.is_object(), "{changes}");
    let names = named["names"].as_array().unwrap().len();
    assert!(names > 0 && names < 40, "{names}");
    assert_eq!(named["left_out"], json!(40 - names));
}

/// A reply too long to carry the changes carries the brief form; too
/// long for even that, none, and they're told with the next.
#[test]
fn a_reply_carries_its_changes_only_within_the_cap() {
    use super::super::changes::{self, Changes};
    let cap = ToolReply::CAP;
    let filler = |bytes: usize| ToolReply::data(json!({ "filler": "x".repeat(bytes) }));
    let changes = Changes {
        value: Some(json!({ "features": "y".repeat(3000) })),
        brief: Some(json!({ "bounded": "query" })),
        lines: Vec::new(),
        panel: Vec::new(),
    };
    let revision = varde_document::Revision::from(7);
    let (reply, told) = changes::fitted(filler(100), &changes, revision);
    assert!(told);
    assert_eq!(
        reply.structured.unwrap()["user_changes"],
        changes.value.clone().unwrap()
    );
    let (reply, told) = changes::fitted(filler(cap - 2000), &changes, revision);
    assert!(told);
    assert!(reply.text.len() <= cap);
    assert_eq!(
        reply.structured.unwrap()["user_changes"],
        json!({ "bounded": "query" })
    );
    let (reply, told) = changes::fitted(filler(cap), &changes, revision);
    assert!(!told);
    assert_eq!(reply.structured.unwrap().get("user_changes"), None);
}

/// A reply's data of `ROOM` under the cap carries the brief changes and
/// the largest revision within it.
#[test]
fn room_holds_the_brief_changes_and_revision() {
    use super::super::changes::{self, Changes, ROOM};
    let cap = ToolReply::CAP;
    for replaced in [false, true] {
        let brief = changes::briefest(replaced);
        let changes = Changes {
            value: Some(brief.clone()),
            brief: Some(brief),
            lines: Vec::new(),
            panel: Vec::new(),
        };
        // `{"filler":"..."}`: 13 bytes besides the filler.
        let data = ToolReply::data(json!({ "filler": "x".repeat(cap - ROOM - 13) }));
        assert_eq!(data.text.len(), cap - ROOM);
        let revision = varde_document::Revision::from(u64::MAX);
        let (reply, told) = changes::fitted(data, &changes, revision);
        assert!(told);
        assert!(reply.text.len() <= cap, "{}", reply.text.len());
    }
}

/// A call cancelled, or whose reply isn't taken, tells nothing: the
/// user's changes come in the next result.
#[test]
fn a_reply_not_read_leaves_the_changes_for_the_next() {
    let (mut varde, requests) = in_session();
    summary(&mut varde);
    let extrude = document(&varde).features()[1].id;
    let rename = |name: &str| Command::Rename {
        target: Named::Feature(extrude),
        name: name.to_owned(),
    };
    user(&mut varde, &requests, rename("Base"));
    let call = || Call::BodyTopology(topology("Body 1"));
    let cancelled = calling(&mut varde, "a session", 50, call());
    let event = session::Event::Cancelled {
        session: SessionId::for_tests("a session"),
        request: Id::Number(50),
    };
    feed(&mut varde, Event::Session(event));
    let reply = cancelled.try_recv().unwrap();
    assert!(reply.structured.is_none(), "{reply:?}");
    assert_eq!(varde.mcp.log.back().unwrap().told, Vec::<String>::new());
    assert!(summary(&mut varde).get("user_changes").is_some());

    user(&mut varde, &requests, rename("Plate"));
    drop(calling(&mut varde, "a session", 51, call()));
    respond(&mut varde, &requests, is_topology);
    assert!(varde.mcp.log.back().unwrap().result.is_some());
    assert!(summary(&mut varde).get("user_changes").is_some());
}
