//! The MCP panel's Revert, through `Varde::update`: to before an edit
//! (one undo step) and to after it, asking first when later changes go
//! too; rows greyed as Undo and Redo take them back; refused while a
//! client edit is in flight and across a `Replace`; the bound on the
//! rows' snapshots; the client told of a revert; *Applied (client not
//! told)*; Show in Timeline.

use serde_json::json;
use varde_document::Named;
use varde_view::{McpEdit, McpPanel, McpRow};

use super::changes::{document, in_session, summary, user};
use super::*;

fn panel(varde: &mut Varde, message: McpPanel) {
    ui(varde, varde_view::Message::McpPanel(message));
}

fn undo(varde: &mut Varde) {
    ui(varde, varde_view::Message::Edit(varde_view::Edit::Undo));
}

fn redo(varde: &mut Varde) {
    ui(varde, varde_view::Message::Edit(varde_view::Edit::Redo));
}

/// Renames the feature `name` to `to` as the client's edit: its row's
/// serial.
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

/// What the edit row of `serial` offers and shows.
fn edit_row(varde: &Varde, serial: u64) -> McpEdit {
    (varde.mcp_panel_shown().unwrap().rows.into_iter())
        .find_map(|row| match row {
            McpRow::Call(call) => call.edit.filter(|edit| edit.serial == serial),
            _ => None,
        })
        .expect("the edit's row")
}

fn extrude_name(varde: &Varde) -> String {
    feature_name(varde, 1)
}

fn toast(varde: &Varde) -> Option<&str> {
    varde.toast.message()
}

#[test]
fn revert_to_before_an_edit_is_one_undo_step() {
    let (mut varde, requests) = in_session();
    let serial = rename(&mut varde, &requests, "Extrude 1", "Base");
    let row = edit_row(&varde, serial);
    assert!(row.before && !row.here && !row.reverted && !row.not_told);

    // Nothing since: it alone goes, unasked.
    panel(&mut varde, McpPanel::RevertBefore(serial));
    regenerate(&mut varde, &requests);
    assert!(varde.screen.doc().unwrap().mcp.revert_ask.is_none());
    assert_eq!(extrude_name(&varde), "Extrude 1");
    assert!(edit_row(&varde, serial).reverted);
    let rows = varde.mcp_panel_shown().unwrap().rows;
    assert!(
        matches!(rows.last(), Some(McpRow::Revert(revert)) if !revert.reverted
            && revert.to == "before “Renamed feature Extrude 1 to Base”"),
        "{rows:?}"
    );
    panel(&mut varde, McpPanel::Toggle);
    assert!(on_screen(&varde).iter().any(|text| text == "Reverted"));
    // Now Revert to here is offered: its after isn't the design.
    assert!(edit_row(&varde, serial).here);

    // Undo takes the revert back, the edit back in effect.
    undo(&mut varde);
    assert_eq!(extrude_name(&varde), "Base");
    assert!(!edit_row(&varde, serial).reverted);
    let rows = varde.mcp_panel_shown().unwrap().rows;
    assert!(matches!(rows.last(), Some(McpRow::Revert(revert)) if revert.reverted));
    // One more undo takes the edit itself back.
    undo(&mut varde);
    assert_eq!(extrude_name(&varde), "Extrude 1");
    assert!(edit_row(&varde, serial).reverted);
}

#[test]
fn revert_to_here_asks_listing_the_later_changes() {
    let (mut varde, requests) = in_session();
    let first = rename(&mut varde, &requests, "Extrude 1", "Base");
    rename(&mut varde, &requests, "Base", "Top");
    panel(&mut varde, McpPanel::OpenMenu(first));
    assert!(edit_row(&varde, first).menu);
    panel(&mut varde, McpPanel::RevertToHere(first));
    assert!(!edit_row(&varde, first).menu);
    let ask = varde.mcp_panel_shown().unwrap().revert.unwrap();
    assert_eq!(ask.to, "after “Renamed feature Extrude 1 to Base”");
    assert_eq!(ask.differences, ["renamed Top to Base"]);
    let shown = on_screen(&varde);
    assert!(shown.iter().any(|text| text.starts_with("Revert to after")));
    assert!(shown.iter().any(|text| text == "• renamed Top to Base"));
    assert_eq!(extrude_name(&varde), "Top");

    // Cancelled, nothing changes; asked again and confirmed, it goes.
    panel(&mut varde, McpPanel::CancelRevert);
    assert!(varde.mcp_panel_shown().unwrap().revert.is_none());
    assert_eq!(extrude_name(&varde), "Top");
    panel(&mut varde, McpPanel::RevertToHere(first));
    panel(&mut varde, McpPanel::ConfirmRevert);
    regenerate(&mut varde, &requests);
    assert_eq!(extrude_name(&varde), "Base");
    assert!(!edit_row(&varde, first).reverted);
    let second = varde.mcp.log.back().unwrap().serial;
    assert!(edit_row(&varde, second).reverted);
}

#[test]
fn revert_before_an_edit_asks_when_the_user_changed_things_since() {
    let (mut varde, requests) = in_session();
    let serial = rename(&mut varde, &requests, "Extrude 1", "Base");
    let body = document(&varde).bodies()[0].id;
    user(
        &mut varde,
        &requests,
        Command::Rename {
            target: Named::Body(body),
            name: "Plate".to_owned(),
        },
    );
    panel(&mut varde, McpPanel::RevertBefore(serial));
    let ask = varde.mcp_panel_shown().unwrap().revert.unwrap();
    assert_eq!(
        ask.differences,
        ["renamed Base to Extrude 1", "renamed body Plate to Body 1"]
    );
    // Esc cancels it, as the dialog's Cancel.
    let doc = varde.screen.doc().unwrap();
    assert_eq!(doc.dialog(), Some(crate::doc::Dialog::McpRevert));
    panel(&mut varde, McpPanel::ConfirmRevert);
    regenerate(&mut varde, &requests);
    assert_eq!(extrude_name(&varde), "Extrude 1");
    assert_eq!(document(&varde).bodies()[0].name, "Body 1");
}

#[test]
fn rows_grey_as_undo_and_redo_take_them_back() {
    let (mut varde, requests) = in_session();
    ui(&mut varde, varde_view::Message::McpPanel(McpPanel::Toggle));
    let serial = rename(&mut varde, &requests, "Extrude 1", "Base");
    undo(&mut varde);
    assert!(edit_row(&varde, serial).reverted);
    redo(&mut varde);
    assert!(!edit_row(&varde, serial).reverted);

    // The user's own, told with a call, grey the same way.
    let body = document(&varde).bodies()[0].id;
    user(
        &mut varde,
        &requests,
        Command::Rename {
            target: Named::Body(body),
            name: "Plate".to_owned(),
        },
    );
    summary(&mut varde);
    let told = McpRow::User {
        said: "renamed body Body 1 to Plate".into(),
        reverted: false,
    };
    assert!(varde.mcp_panel_shown().unwrap().rows.contains(&told));
    undo(&mut varde);
    let greyed = McpRow::User {
        said: "renamed body Body 1 to Plate".into(),
        reverted: true,
    };
    assert!(varde.mcp_panel_shown().unwrap().rows.contains(&greyed));
    redo(&mut varde);
    assert!(varde.mcp_panel_shown().unwrap().rows.contains(&told));
    // And by a revert past them.
    panel(&mut varde, McpPanel::RevertBefore(serial));
    panel(&mut varde, McpPanel::ConfirmRevert);
    regenerate(&mut varde, &requests);
    let rows = varde.mcp_panel_shown().unwrap().rows;
    assert!(rows.contains(&greyed), "{rows:?}");
    assert!(edit_row(&varde, serial).reverted);
}

#[test]
fn a_revert_is_refused_while_a_client_edit_is_in_flight() {
    let (mut varde, requests) = in_session();
    let serial = rename(&mut varde, &requests, "Extrude 1", "Base");
    // The model behind the design: the next edit waits for it.
    let body = document(&varde).bodies()[0].id;
    let doc = varde.screen.doc_mut().unwrap();
    doc.apply(Command::Rename {
        target: Named::Body(body),
        name: "Plate".to_owned(),
    });
    doc.sync();
    let reply = calling(&mut varde, "a session", 50, rename_feature("Base", "Top"));
    assert!(varde.screen.doc().unwrap().mcp.any_unapplied());
    let revision = varde.screen.doc().unwrap().editor.revision();
    panel(&mut varde, McpPanel::RevertBefore(serial));
    assert_eq!(toast(&varde), Some(super::super::revert::EDIT_IN_FLIGHT));
    let doc = varde.screen.doc().unwrap();
    assert!(doc.mcp.revert_ask.is_none());
    assert_eq!(doc.editor.revision(), revision);

    // Applied and answered, it's allowed.
    regenerate(&mut varde, &requests);
    assert!(!reply.try_recv().unwrap().is_error);
    panel(&mut varde, McpPanel::RevertBefore(serial));
    assert!(varde.screen.doc().unwrap().mcp.revert_ask.is_some());
}

#[test]
fn the_oldest_rows_lose_revert_past_the_snapshot_bound() {
    let (mut varde, requests) = in_session();
    let mut name = "Extrude 1".to_owned();
    let mut serials = Vec::new();
    for at in 0..=super::super::revert::SNAPSHOTS {
        let to = format!("Step {at}");
        serials.push(rename(&mut varde, &requests, &name, &to));
        name = to;
    }
    // 52 documents, 51 edits: the first row lets both go, the second
    // its before (its after is the third's before).
    let first = edit_row(&varde, serials[0]);
    assert!(!first.before && !first.here);
    let second = edit_row(&varde, serials[1]);
    assert!(!second.before && second.here);
    let last = edit_row(&varde, *serials.last().unwrap());
    assert!(last.before);
    // Without its state, the menu's Revert does nothing.
    let revision = varde.screen.doc().unwrap().editor.revision();
    panel(&mut varde, McpPanel::RevertBefore(serials[0]));
    let doc = varde.screen.doc().unwrap();
    assert!(doc.mcp.revert_ask.is_none());
    assert_eq!(doc.editor.revision(), revision);
}

#[test]
fn a_revert_across_a_replace_is_refused() {
    let (mut varde, requests) = in_session();
    let serial = rename(&mut varde, &requests, "Extrude 1", "Base");
    user(
        &mut varde,
        &requests,
        Command::Replace(Box::new(Document::example())),
    );
    let row = edit_row(&varde, serial);
    assert!(!row.before && !row.here);
    let revision = varde.screen.doc().unwrap().editor.revision();
    panel(&mut varde, McpPanel::RevertBefore(serial));
    assert_eq!(toast(&varde), Some(super::super::revert::REPLACED));
    assert_eq!(varde.screen.doc().unwrap().editor.revision(), revision);
}

#[test]
fn the_client_is_told_of_a_revert() {
    let (mut varde, requests) = in_session();
    let serial = rename(&mut varde, &requests, "Extrude 1", "Base");
    panel(&mut varde, McpPanel::RevertBefore(serial));
    regenerate(&mut varde, &requests);
    let changes = &summary(&mut varde)["user_changes"];
    assert_eq!(
        changes["reverted"],
        json!(["before “Renamed feature Extrude 1 to Base”"])
    );
    assert_eq!(
        changes["features"]["changed"][0]["renamed"],
        json!({ "old": "Base", "new": "Extrude 1" })
    );
    // Told once.
    assert_eq!(summary(&mut varde).get("user_changes"), None);
}

#[test]
fn an_edit_whose_reply_reached_no_one_says_so() {
    let (mut varde, requests) = in_session();
    let reply = calling(
        &mut varde,
        "a session",
        7,
        rename_feature("Extrude 1", "Base"),
    );
    drop(reply);
    regenerate(&mut varde, &requests);
    let serial = varde.mcp.log.back().unwrap().serial;
    assert!(edit_row(&varde, serial).not_told);
    ui(&mut varde, varde_view::Message::McpPanel(McpPanel::Toggle));
    assert!(
        on_screen(&varde)
            .iter()
            .any(|text| text == "Applied (client not told)")
    );
}

#[test]
fn show_in_timeline_selects_the_feature() {
    let (mut varde, requests) = in_session();
    let serial = rename(&mut varde, &requests, "Extrude 1", "Base");
    assert!(edit_row(&varde, serial).timeline);
    varde.screen.doc_mut().unwrap().panel = varde_view::Panel::Objects;
    panel(&mut varde, McpPanel::ShowInTimeline(serial));
    let doc = varde.screen.doc().unwrap();
    assert_eq!(doc.panel, varde_view::Panel::Timeline);
    assert_eq!(
        doc.selected_feature,
        Some(document(&varde).features()[1].id)
    );
}

#[test]
fn a_revert_of_a_revert_ungreys_what_it_brought_back() {
    let (mut varde, requests) = in_session();
    let serial = rename(&mut varde, &requests, "Extrude 1", "Base");
    panel(&mut varde, McpPanel::RevertBefore(serial));
    regenerate(&mut varde, &requests);
    assert!(edit_row(&varde, serial).reverted);
    panel(&mut varde, McpPanel::RevertToHere(serial));
    panel(&mut varde, McpPanel::ConfirmRevert);
    regenerate(&mut varde, &requests);
    assert_eq!(extrude_name(&varde), "Base");
    assert!(!edit_row(&varde, serial).reverted);
    let reverts: Vec<_> = (varde.mcp_panel_shown().unwrap().rows.into_iter())
        .filter_map(|row| match row {
            McpRow::Revert(revert) => Some(revert.reverted),
            _ => None,
        })
        .collect();
    // The first taken back by the second.
    assert_eq!(reverts, [true, false]);
}

#[test]
fn an_edit_undone_then_edited_past_stays_grey() {
    let (mut varde, requests) = in_session();
    let serial = rename(&mut varde, &requests, "Extrude 1", "Base");
    undo(&mut varde);
    let body = document(&varde).bodies()[0].id;
    user(
        &mut varde,
        &requests,
        Command::Rename {
            target: Named::Body(body),
            name: "Plate".to_owned(),
        },
    );
    assert!(edit_row(&varde, serial).reverted);
}

#[test]
fn revert_to_here_is_not_offered_once_reverted_there() {
    let (mut varde, requests) = in_session();
    let first = rename(&mut varde, &requests, "Extrude 1", "Base");
    rename(&mut varde, &requests, "Base", "Top");
    panel(&mut varde, McpPanel::RevertToHere(first));
    panel(&mut varde, McpPanel::ConfirmRevert);
    regenerate(&mut varde, &requests);
    assert_eq!(extrude_name(&varde), "Base");
    assert!(!edit_row(&varde, first).here);
    // Asked anyway, it changes nothing and asks nothing.
    let revision = varde.screen.doc().unwrap().editor.revision();
    panel(&mut varde, McpPanel::RevertToHere(first));
    let doc = varde.screen.doc().unwrap();
    assert!(doc.mcp.revert_ask.is_none());
    assert_eq!(doc.editor.revision(), revision);
}

#[test]
fn the_revert_dialog_follows_changes_made_while_it_is_up() {
    let (mut varde, requests) = in_session();
    let first = rename(&mut varde, &requests, "Extrude 1", "Base");
    rename(&mut varde, &requests, "Base", "Top");
    panel(&mut varde, McpPanel::RevertToHere(first));
    let ask = varde.mcp_panel_shown().unwrap().revert.unwrap();
    assert_eq!(ask.differences, ["renamed Top to Base"]);
    // The client edits on while it's up.
    rename(&mut varde, &requests, "Top", "Lid");
    panel(&mut varde, McpPanel::ConfirmRevert);
    // Not reverted unseen: asked again, listing what it undoes now.
    assert_eq!(extrude_name(&varde), "Lid");
    let ask = varde.mcp_panel_shown().unwrap().revert.unwrap();
    assert_eq!(ask.differences, ["renamed Lid to Base"]);
    panel(&mut varde, McpPanel::ConfirmRevert);
    regenerate(&mut varde, &requests);
    assert_eq!(extrude_name(&varde), "Base");
}

#[test]
fn an_edit_reverted_while_it_waits_on_regen_says_undone() {
    let (mut varde, requests) = in_session();
    let first = rename(&mut varde, &requests, "Extrude 1", "Base");
    let reply = calling(&mut varde, "a session", 50, rename_feature("Base", "Top"));
    assert!(!varde.screen.doc().unwrap().mcp.any_unapplied());
    assert_eq!(extrude_name(&varde), "Top");
    panel(&mut varde, McpPanel::RevertBefore(first));
    panel(&mut varde, McpPanel::ConfirmRevert);
    assert_eq!(extrude_name(&varde), "Extrude 1");
    regenerate(&mut varde, &requests);
    let reply = reply.try_recv().unwrap();
    let data = reply.structured.unwrap();
    assert_eq!(data["result"], json!(super::super::edit::UNDONE));
    let second = varde.mcp.log.back().unwrap().serial;
    assert!(edit_row(&varde, second).reverted);
}

/// The same at the bound, regeneration not answering: the revert takes
/// it back as an undo would.
#[test]
fn an_edit_reverted_while_regen_is_late_says_undone() {
    let (mut varde, requests) = in_session();
    let first = rename(&mut varde, &requests, "Extrude 1", "Base");
    let came = iced::time::Instant::now();
    let reply = calling(&mut varde, "a session", 50, rename_feature("Base", "Top"));
    panel(&mut varde, McpPanel::RevertBefore(first));
    panel(&mut varde, McpPanel::ConfirmRevert);
    assert_eq!(extrude_name(&varde), "Extrude 1");
    let late = came + super::super::waiting::BOUND + std::time::Duration::from_secs(1);
    let _ = varde.update(Message::Tick(late));
    let data = reply.try_recv().unwrap().structured.unwrap();
    assert_eq!(data["result"], json!(super::super::edit::UNDONE));
}

/// The user's rows of `varde`'s panel, greyed or not, as their text.
fn user_rows(varde: &Varde) -> Vec<String> {
    (varde.mcp_panel_shown().unwrap().rows.into_iter())
        .filter_map(|row| match row {
            McpRow::User { said, .. } => Some(said.into_owned()),
            _ => None,
        })
        .collect()
}

/// A revert shows as its own row alone, what it changed in its tooltip,
/// while the client is still told of it; the user's edits after still
/// get rows, and Undo greys the revert's row.
#[test]
fn a_revert_is_one_row_its_changes_in_its_tooltip() {
    let (mut varde, requests) = in_session();
    panel(&mut varde, McpPanel::Toggle);
    let serial = rename(&mut varde, &requests, "Extrude 1", "Base");
    panel(&mut varde, McpPanel::RevertBefore(serial));
    regenerate(&mut varde, &requests);
    let reverts = |varde: &Varde| -> Vec<varde_view::McpRevert<'static>> {
        (varde.mcp_panel_shown().unwrap().rows.into_iter())
            .filter_map(|row| match row {
                McpRow::Revert(revert) => Some(varde_view::McpRevert {
                    to: revert.to.into_owned().into(),
                    tip: revert.tip.into_owned().into(),
                    reverted: revert.reverted,
                }),
                _ => None,
            })
            .collect()
    };
    let shown = reverts(&varde);
    assert_eq!(shown.len(), 1);
    assert_eq!(
        shown[0].tip,
        "You: reverted to before “Renamed feature Extrude 1 to Base”\nrenamed Base to Extrude 1"
    );
    assert_eq!(user_rows(&varde), Vec::<String>::new());
    assert!(
        !on_screen(&varde)
            .iter()
            .any(|text| text.contains("renamed Base"))
    );

    // The client is still told, and its call's row adds none either.
    let changes = &summary(&mut varde)["user_changes"];
    assert_eq!(
        changes["reverted"],
        json!(["before “Renamed feature Extrude 1 to Base”"])
    );
    assert_eq!(
        changes["features"]["changed"][0]["renamed"],
        json!({ "old": "Base", "new": "Extrude 1" })
    );
    assert_eq!(reverts(&varde).len(), 1);
    assert_eq!(user_rows(&varde), Vec::<String>::new());

    // A revert and an edit of the user's in one call's changes: the edit
    // alone gets a row.
    let serial = rename(&mut varde, &requests, "Extrude 1", "Top");
    panel(&mut varde, McpPanel::RevertBefore(serial));
    regenerate(&mut varde, &requests);
    let body = document(&varde).bodies()[0].id;
    user(
        &mut varde,
        &requests,
        Command::Rename {
            target: Named::Body(body),
            name: "Plate".to_owned(),
        },
    );
    assert_eq!(user_rows(&varde), ["renamed body Body 1 to Plate"]);
    summary(&mut varde);
    assert_eq!(user_rows(&varde), ["renamed body Body 1 to Plate"]);
    assert_eq!(reverts(&varde).len(), 2);

    // Undo of the body rename, then of the revert: its row greys.
    undo(&mut varde);
    undo(&mut varde);
    assert_eq!(feature_name(&varde, 1), "Top");
    let shown = reverts(&varde);
    assert!(!shown[0].reverted && shown[1].reverted, "{shown:?}");
}

/// A revert's tooltip lists at most ten of its changes, the rest counted.
#[test]
fn a_reverts_tooltip_is_bounded() {
    let (mut varde, requests) = in_session();
    let serial = rename(&mut varde, &requests, "Extrude 1", "Base");
    for at in 0..12 {
        user(
            &mut varde,
            &requests,
            Command::AddParam {
                name: format!("p{at}"),
                text: "1 mm".to_owned(),
            },
        );
    }
    panel(&mut varde, McpPanel::RevertBefore(serial));
    panel(&mut varde, McpPanel::ConfirmRevert);
    regenerate(&mut varde, &requests);
    let tip = (varde.mcp_panel_shown().unwrap().rows.into_iter())
        .find_map(|row| match row {
            McpRow::Revert(revert) => Some(revert.tip.into_owned()),
            _ => None,
        })
        .unwrap();
    let lines: Vec<_> = tip.lines().collect();
    assert_eq!(lines.len(), 12, "{tip}");
    assert_eq!(lines[11], "and 3 more");
}
