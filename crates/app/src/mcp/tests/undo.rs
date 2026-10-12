//! The `undo` tool, through `Varde::update`: the session's newest edit
//! taken back (one history step, its row greyed, the next result telling
//! no change of the user's), `steps` of 3, refused when an edit of the
//! user's sits within the steps, after the user's Undo, after a revert,
//! past another session's edit, with nothing of its own and past the
//! history's reach; queued behind a waiting edit and failing with it,
//! behind the user's sketch proposal, the popup with a sketch open, the
//! user's Redo of it told, and the 30 s bound; sent right behind its
//! edit, refused failing the edit behind it, an amended edit, and the
//! user's Undo or Redo while it waits on regeneration.

use serde_json::json;
use varde_document::Named;
use varde_view::{McpEdit, McpPanel, McpRow};

use super::changes::{document, in_session, summary, user};
use super::*;

fn undo_call(steps: usize) -> Call {
    Call::Undo { steps }
}

/// Renames the feature `name` to `to` as the client's edit in "a
/// session", answered: its row's serial.
fn rename(varde: &mut Varde, requests: &Lane, name: &str, to: &str) -> u64 {
    let (reply, state) = edit(
        varde,
        requests,
        "rename",
        json!({ "feature": name, "to": to }),
    );
    assert!(!reply.is_error, "{}", reply.text);
    assert_eq!(state, McpCallState::Applied);
    varde.mcp.log.back().unwrap().serial
}

/// Calls `undo` of `steps` in `session` and answers regeneration: its
/// reply.
fn undo_in(varde: &mut Varde, requests: &Lane, session: &str, steps: usize) -> ToolReply {
    let id = (varde.mcp.log.back()).map_or(1, |logged| i64::try_from(logged.serial).unwrap() + 1);
    let reply = calling(varde, session, id, undo_call(steps));
    regenerate(varde, requests);
    reply.try_recv().unwrap()
}

fn undo(varde: &mut Varde, requests: &Lane, steps: usize) -> ToolReply {
    undo_in(varde, requests, "a session", steps)
}

/// An undo refused, saying `why`, the design unchanged.
fn refused(varde: &mut Varde, requests: &Lane, session: &str, steps: usize, why: &str) {
    let generation = varde.screen.doc().unwrap().editor.generation();
    let reply = undo_in(varde, requests, session, steps);
    assert!(reply.is_error, "{}", reply.text);
    assert!(reply.text.starts_with("Not applied: "), "{}", reply.text);
    assert!(reply.text.contains(why), "{}", reply.text);
    assert_eq!(
        varde.mcp.log.back().unwrap().state,
        McpCallState::NotApplied
    );
    assert_eq!(varde.screen.doc().unwrap().editor.generation(), generation);
}

/// The edit row of `serial`.
fn edit_row(varde: &Varde, serial: u64) -> McpEdit {
    (varde.mcp_panel_shown().unwrap().rows.into_iter())
        .find_map(|row| match row {
            McpRow::Call(call) => call.edit.filter(|edit| edit.serial == serial),
            _ => None,
        })
        .expect("the edit's row")
}

fn redo(varde: &mut Varde) {
    ui(varde, varde_view::Message::Edit(varde_view::Edit::Redo));
}

fn user_undo(varde: &mut Varde) {
    ui(varde, varde_view::Message::Edit(varde_view::Edit::Undo));
}

#[test]
fn undo_takes_back_the_sessions_newest_edit() {
    let (mut varde, requests) = in_session();
    let before = varde.screen.doc().unwrap().editor.revision();
    let renamed = rename(&mut varde, &requests, "Extrude 1", "Base");
    summary(&mut varde);

    let reply = calling(&mut varde, "a session", 50, undo_call(1));
    // One history step, back to before the rename, answered once
    // regeneration has.
    let doc = varde.screen.doc().unwrap();
    assert_eq!(doc.editor.revision(), before);
    assert!(doc.editor.can_redo());
    assert_eq!(feature_name(&varde, 1), "Extrude 1");
    assert!(reply.try_recv().is_err());
    regenerate(&mut varde, &requests);
    let reply = reply.try_recv().unwrap();
    assert_eq!(reply.structured.as_ref().unwrap().get("user_changes"), None);
    let data = edit_data(reply);
    assert_eq!(data["result"], json!("applied"));
    assert_eq!(data["undone"], json!(["rename feature Extrude 1 to Base"]));

    // The rename's row greyed; the undo's says what it undid, and offers
    // Show in Timeline only.
    assert!(edit_row(&varde, renamed).reverted);
    let logged = varde.mcp.log.back().unwrap();
    assert_eq!(logged.tool, McpTool::Undo);
    assert_eq!(logged.args, "renamed feature Extrude 1 to Base");
    assert_eq!(logged.state, McpCallState::Applied);
    assert_eq!(
        logged.result.as_deref(),
        Some("Undid: Renamed feature Extrude 1 to Base")
    );
    let serial = logged.serial;
    let row = edit_row(&varde, serial);
    assert!(row.undo && row.timeline && !row.here && !row.before && !row.reverted);
    panel(&mut varde, McpPanel::Toggle);
    let shown = on_screen(&varde);
    assert!(shown.iter().any(|text| text == "Undid:"), "{shown:?}");
    assert!(
        shown
            .iter()
            .any(|text| text == "renamed feature Extrude 1 to Base")
    );
    panel(&mut varde, McpPanel::OpenMenu(serial));
    let shown = on_screen(&varde);
    assert!(shown.iter().any(|text| text == "Show in Timeline"));
    assert!(!shown.iter().any(|text| text.starts_with("Revert to")));
    panel(&mut varde, McpPanel::CloseMenu);

    // The client's own undo isn't the user's to tell of.
    assert_eq!(summary(&mut varde).get("user_changes"), None);
    // Nothing of its own left: refused.
    refused(
        &mut varde,
        &requests,
        "a session",
        1,
        crate::mcp::undo::NOTHING_OF_YOURS,
    );

    // The user's Redo of it is theirs, and told; it takes the undo back.
    redo(&mut varde);
    regenerate(&mut varde, &requests);
    assert_eq!(feature_name(&varde, 1), "Base");
    assert!(edit_row(&varde, serial).reverted);
    assert!(!edit_row(&varde, renamed).reverted);
    let changes = summary(&mut varde)["user_changes"].clone();
    assert_eq!(
        changes,
        json!({ "redid_your_edits": ["rename feature Extrude 1 to Base"] })
    );
    // Redone by the user, it's no longer the client's to undo.
    refused(
        &mut varde,
        &requests,
        "a session",
        1,
        crate::mcp::undo::NOTHING_OF_YOURS,
    );
}

#[test]
fn undo_takes_back_steps_newest_first() {
    let (mut varde, requests) = in_session();
    rename(&mut varde, &requests, "Extrude 1", "Base");
    rename(&mut varde, &requests, "Sketch 1", "Profile");
    rename(&mut varde, &requests, "Base", "Plate");
    let data = edit_data(undo(&mut varde, &requests, 3));
    assert_eq!(
        data["undone"],
        json!([
            "rename feature Base to Plate",
            "rename feature Sketch 1 to Profile",
            "rename feature Extrude 1 to Base",
        ])
    );
    assert_eq!(feature_name(&varde, 0), "Sketch 1");
    assert_eq!(feature_name(&varde, 1), "Extrude 1");
    let logged = varde.mcp.log.back().unwrap();
    assert_eq!(logged.tool, McpTool::Undo);
    assert_eq!(logged.args, "3 edits");
    assert_eq!(
        logged.result.as_deref(),
        Some(
            "Undid: Renamed feature Base to Plate; Renamed feature Sketch 1 to Profile; \
             Renamed feature Extrude 1 to Base"
        )
    );
    assert_eq!(summary(&mut varde).get("user_changes"), None);
}

#[test]
fn undo_is_refused_past_an_edit_not_its_own() {
    // An edit of the user's within the steps.
    let (mut varde, requests) = in_session();
    rename(&mut varde, &requests, "Extrude 1", "Base");
    let body = document(&varde).bodies()[0].id;
    let plate = Command::Rename {
        target: Named::Body(body),
        name: "Plate".to_owned(),
    };
    user(&mut varde, &requests, plate);
    rename(&mut varde, &requests, "Sketch 1", "Profile");
    refused(
        &mut varde,
        &requests,
        "a session",
        2,
        "only 1 of the design's newest steps is",
    );
    assert_eq!(feature_name(&varde, 0), "Profile");
    // On top of the user's edit: the design changed since.
    let body = document(&varde).bodies()[0].id;
    let top = Command::Rename {
        target: Named::Body(body),
        name: "Top".to_owned(),
    };
    user(&mut varde, &requests, top);
    refused(
        &mut varde,
        &requests,
        "a session",
        1,
        crate::mcp::undo::CHANGED_SINCE,
    );

    // After the user's own Undo of the session's edit.
    let (mut varde, requests) = in_session();
    rename(&mut varde, &requests, "Extrude 1", "Base");
    rename(&mut varde, &requests, "Sketch 1", "Profile");
    user_undo(&mut varde);
    regenerate(&mut varde, &requests);
    refused(
        &mut varde,
        &requests,
        "a session",
        1,
        crate::mcp::undo::CHANGED_SINCE,
    );
    assert_eq!(feature_name(&varde, 1), "Base");

    // After a revert from the panel.
    let (mut varde, requests) = in_session();
    rename(&mut varde, &requests, "Extrude 1", "Base");
    let serial = rename(&mut varde, &requests, "Sketch 1", "Profile");
    panel(&mut varde, McpPanel::RevertBefore(serial));
    regenerate(&mut varde, &requests);
    assert_eq!(feature_name(&varde, 0), "Sketch 1");
    refused(
        &mut varde,
        &requests,
        "a session",
        1,
        crate::mcp::undo::CHANGED_SINCE,
    );

    // Another session's edit: never its own, before it or on top.
    let (mut varde, requests) = in_session();
    rename(&mut varde, &requests, "Extrude 1", "Base");
    started(&mut varde, "b session", None);
    refused(
        &mut varde,
        &requests,
        "b session",
        1,
        crate::mcp::undo::NOTHING_OF_YOURS,
    );
    let id = 90;
    let reply = calling(
        &mut varde,
        "b session",
        id,
        rename_feature("Sketch 1", "Profile"),
    );
    regenerate(&mut varde, &requests);
    assert!(!reply.try_recv().unwrap().is_error);
    refused(
        &mut varde,
        &requests,
        "b session",
        2,
        "only 1 of the design's newest steps is",
    );
    refused(
        &mut varde,
        &requests,
        "a session",
        1,
        crate::mcp::undo::CHANGED_SINCE,
    );
    // Its own alone goes.
    let data = edit_data(undo_in(&mut varde, &requests, "b session", 1));
    assert_eq!(
        data["undone"],
        json!(["rename feature Sketch 1 to Profile"])
    );
    assert_eq!(feature_name(&varde, 1), "Base");
}

/// The history reaches back only as far as it does: a design whose every
/// step is the session's can't be undone past its start.
#[test]
fn undo_is_refused_past_the_historys_reach() {
    let (mut doc, requests) = crate::tests::deferred();
    doc.editor = varde_document::Editor::new(Document::example());
    doc.sync();
    crate::tests::answer(&mut doc, &requests);
    let mut varde = showing(doc);
    varde.options.mcp = true;
    varde.options.mcp_port = 7311;
    varde.mcp = serving_on(7311);
    varde.mcp.status = Status::Listening("127.0.0.1:7311".parse().unwrap());
    started(&mut varde, "a session", None);
    rename(&mut varde, &requests, "Extrude 1", "Base");
    rename(&mut varde, &requests, "Sketch 1", "Profile");
    refused(
        &mut varde,
        &requests,
        "a session",
        3,
        "reaches back only 2 steps",
    );
    assert_eq!(feature_name(&varde, 0), "Profile");
    let data = edit_data(undo(&mut varde, &requests, 2));
    assert_eq!(data["undone"].as_array().unwrap().len(), 2);
    assert!(!varde.screen.doc().unwrap().editor.can_undo());
}

/// An undo queued behind the session's edit waiting on the popup waits
/// with it, and fails with it.
#[test]
fn undo_behind_a_waiting_edit_fails_with_it() {
    let (mut varde, requests) = in_session();
    rename(&mut varde, &requests, "Extrude 1", "Base");
    edit_extrude(&mut varde);
    extrude_distance(&mut varde, "7 mm");
    let first = calling(
        &mut varde,
        "a session",
        10,
        rename_feature("Sketch 1", "Profile"),
    );
    let second = calling(&mut varde, "a session", 11, undo_call(1));
    assert!(second.try_recv().is_err());
    panel(&mut varde, McpPanel::KeepEditing);
    assert!(first.try_recv().unwrap().is_error);
    let second = second.try_recv().unwrap();
    assert!(
        second
            .text
            .starts_with(&failed_behind("the user is editing Base")),
        "{}",
        second.text
    );
    assert_eq!(feature_name(&varde, 1), "Base");
}

/// Behind the user's sketch edit waiting on the solver, the undo is
/// checked at its turn, once it's committed: the user's point is then the
/// newest step.
#[test]
fn undo_behind_the_users_sketch_proposal_is_checked_at_its_turn() {
    let (mut varde, requests, mut lane) = sketching();
    rename(&mut varde, &requests, "Extrude 1", "Base");
    enter_last_sketch(&mut varde);
    place_point(&mut varde, 1.0, 2.0);
    assert!(varde.screen.doc().unwrap().proposing());
    let reply = calling(&mut varde, "a session", 20, undo_call(1));
    assert!(reply.try_recv().is_err());
    assert_eq!(feature_name(&varde, 1), "Base");
    solve(&mut varde, &mut lane);
    regenerate(&mut varde, &requests);
    let reply = reply.try_recv().unwrap();
    assert!(
        reply.text.contains(crate::mcp::undo::CHANGED_SINCE),
        "{}",
        reply.text
    );
    assert_eq!(points(&varde), 1);
    assert_eq!(feature_name(&varde, 1), "Base");
}

/// A sketch open with its tool started asks first, saying the client
/// wants to undo its last edit; Apply yours lets it go ahead.
#[test]
fn undo_with_a_sketch_open_asks_first() {
    let (mut varde, requests, mut lane) = sketching();
    rename(&mut varde, &requests, "Extrude 1", "Base");
    enter_last_sketch(&mut varde);
    look(
        &mut varde,
        varde_view::Look::SelectTool(varde_view::Tool::Line),
    );
    place_point(&mut varde, 1.0, 2.0);
    solve(&mut varde, &mut lane);
    let reply = calling(&mut varde, "a session", 30, undo_call(1));
    let asked = ask(&varde).unwrap();
    assert_eq!(asked.tool, McpTool::Undo);
    let sketch = feature_name(&varde, 2);
    let shown = on_screen(&varde);
    let title = format!("A client wants to undo its last edit while you're editing {sketch}.");
    assert!(shown.contains(&title), "{shown:?}");
    // Revert waits for it too.
    let renamed = varde.mcp.log.iter().rev().nth(1).unwrap().serial;
    panel(&mut varde, McpPanel::RevertBefore(renamed));
    assert_eq!(
        varde.toast.message(),
        Some(crate::mcp::revert::EDIT_IN_FLIGHT)
    );
    panel(&mut varde, McpPanel::ApplyYours);
    assert!(varde.screen.doc().unwrap().sketch.is_none());
    assert_eq!(feature_name(&varde, 1), "Extrude 1");
    regenerate(&mut varde, &requests);
    let data = edit_data(reply.try_recv().unwrap());
    assert_eq!(data["undone"], json!(["rename feature Extrude 1 to Base"]));
}

/// Past 30 s waiting on the popup, the undo answers not applied, the
/// user editing; the popup says the undo was cancelled.
#[test]
fn undo_waiting_30_s_is_not_applied() {
    let (mut varde, requests) = in_session();
    rename(&mut varde, &requests, "Extrude 1", "Base");
    rename(&mut varde, &requests, "Sketch 1", "Profile");
    edit_extrude(&mut varde);
    extrude_distance(&mut varde, "7 mm");
    let came = iced::time::Instant::now();
    let reply = calling(&mut varde, "a session", 40, undo_call(2));
    assert!(on_screen(&varde).contains(
        &"Claude Code wants to undo its last 2 edits while you're editing Base.".to_owned()
    ));
    let late = came + waiting::BOUND + std::time::Duration::from_secs(1);
    let _ = varde.update(Message::Tick(late));
    let reply = reply.try_recv().unwrap();
    assert!(
        reply.text.contains("the user is editing Base"),
        "{}",
        reply.text
    );
    let logged = varde.mcp.log.back().unwrap();
    assert_eq!(logged.state, McpCallState::NotApplied);
    assert_eq!(logged.args, "its last 2 edits");
    assert!(ask(&varde).unwrap().cancelled);
    let shown = on_screen(&varde);
    assert!(
        shown
            .iter()
            .any(|text| text == "Claude Code's undo was cancelled")
    );
    assert_eq!(feature_name(&varde, 1), "Base");
}

/// An undo sent right behind the session's edit, before that one's
/// regeneration answered, waits its turn and takes it back too; the edit
/// behind the undo goes after it.
#[test]
fn undo_right_behind_its_edit_takes_it_back() {
    let (mut varde, requests) = in_session();
    rename(&mut varde, &requests, "Extrude 1", "Base");
    let first = calling(
        &mut varde,
        "a session",
        60,
        rename_feature("Sketch 1", "Profile"),
    );
    let second = calling(&mut varde, "a session", 61, undo_call(2));
    let third = calling(
        &mut varde,
        "a session",
        62,
        rename_feature("Extrude 1", "Plate"),
    );
    for _ in 0..3 {
        regenerate(&mut varde, &requests);
    }
    assert!(!first.try_recv().unwrap().is_error);
    let data = edit_data(second.try_recv().unwrap());
    assert_eq!(
        data["undone"],
        json!([
            "rename feature Sketch 1 to Profile",
            "rename feature Extrude 1 to Base",
        ])
    );
    assert!(!third.try_recv().unwrap().is_error);
    assert_eq!(feature_name(&varde, 0), "Sketch 1");
    assert_eq!(feature_name(&varde, 1), "Plate");
    assert_eq!(summary(&mut varde).get("user_changes"), None);
}

/// An undo queued behind another session's edit's regeneration, refused
/// at its turn, fails the session's edit queued behind it.
#[test]
fn an_edit_behind_a_refused_undo_fails_with_it() {
    let (mut varde, requests) = in_session();
    started(&mut varde, "b session", None);
    let other = calling(
        &mut varde,
        "b session",
        59,
        rename_feature("Sketch 1", "Profile"),
    );
    let undo = calling(&mut varde, "a session", 60, undo_call(1));
    let next = calling(
        &mut varde,
        "a session",
        61,
        rename_feature("Extrude 1", "Plate"),
    );
    regenerate(&mut varde, &requests);
    assert!(!other.try_recv().unwrap().is_error);
    let undo = undo.try_recv().unwrap();
    assert!(
        undo.text.contains(crate::mcp::undo::NOTHING_OF_YOURS),
        "{}",
        undo.text
    );
    let next = next.try_recv().unwrap();
    assert!(
        next.text
            .starts_with(&failed_behind(crate::mcp::undo::NOTHING_OF_YOURS)),
        "{}",
        next.text
    );
    assert_eq!(feature_name(&varde, 1), "Extrude 1");
}

/// The user's Undo of another step while the undo waits on regeneration
/// doesn't take the undo back: it's applied, not "undone by the user".
#[test]
fn undo_followed_by_the_users_undo_is_applied() {
    let (mut varde, requests) = in_session();
    rename(&mut varde, &requests, "Extrude 1", "Base");
    rename(&mut varde, &requests, "Sketch 1", "Profile");
    let reply = calling(&mut varde, "a session", 70, undo_call(1));
    user_undo(&mut varde);
    regenerate(&mut varde, &requests);
    let reply = reply.try_recv().unwrap();
    let changes = reply.structured.as_ref().unwrap()["user_changes"].clone();
    assert_eq!(
        changes["undid_your_edits"],
        json!(["rename feature Extrude 1 to Base"])
    );
    let data = edit_data(reply);
    assert_eq!(data["result"], json!("applied"));
    let serial = varde.mcp.log.back().unwrap().serial;
    assert!(!edit_row(&varde, serial).reverted);
}

/// Answered at the bound before regeneration, an undo the user redid
/// meanwhile says so, as one answered by regeneration does.
#[test]
fn undo_redone_before_the_bound_says_so() {
    let (mut varde, requests) = in_session();
    rename(&mut varde, &requests, "Extrude 1", "Base");
    let came = iced::time::Instant::now();
    let reply = calling(&mut varde, "a session", 70, undo_call(1));
    redo(&mut varde);
    let late = came + waiting::BOUND + std::time::Duration::from_secs(1);
    let _ = varde.update(Message::Tick(late));
    let data = edit_data(reply.try_recv().unwrap());
    assert_eq!(data["result"], json!(crate::mcp::undo::REDONE));
    assert_eq!(feature_name(&varde, 1), "Base");
    regenerate(&mut varde, &requests);
}

/// An edit the links' amend moved along is undone with its amend, one
/// step of the user's Undo.
#[test]
fn undo_takes_an_amended_edit_back_with_its_amend() {
    let (mut varde, requests) = in_session();
    let before = varde.screen.doc().unwrap().editor.revision();
    rename(&mut varde, &requests, "Extrude 1", "Base");
    // What `Doc::computed` does as the links are found again.
    let doc = varde.screen.doc_mut().unwrap();
    let after = doc.editor.revision();
    let sketch = doc.editor.document().features()[0].id;
    doc.editor
        .amend(Command::Rename {
            target: Named::Feature(sketch),
            name: "Relinked".to_owned(),
        })
        .unwrap();
    let now = doc.editor.revision();
    doc.mcp.amended(after, now);
    doc.sync();
    regenerate(&mut varde, &requests);
    let data = edit_data(undo(&mut varde, &requests, 1));
    assert_eq!(data["undone"], json!(["rename feature Extrude 1 to Base"]));
    assert_eq!(varde.screen.doc().unwrap().editor.revision(), before);
    assert_eq!(feature_name(&varde, 0), "Sketch 1");
    assert_eq!(feature_name(&varde, 1), "Extrude 1");
    assert_eq!(summary(&mut varde).get("user_changes"), None);
}
