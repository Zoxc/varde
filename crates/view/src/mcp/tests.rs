use iced::Size;

use super::*;
use crate::testing::Laid;

/// Serving on 7311 with Claude Code connected, the panel open, with no
/// calls.
fn serving<'a>() -> McpShown<'a> {
    McpShown {
        on: true,
        open: true,
        status: McpStatus::Listening(7311),
        port: 7311,
        client: Some(Some("Claude Code")),
        gone: None,
        rows: Vec::new(),
        dropped: 0,
        ask: None,
        revert: None,
    }
}

fn call<'a>(tool: McpTool, args: &'a str, state: McpCallState) -> McpRow<'a> {
    McpRow::Call(McpCall {
        tool,
        args: args.into(),
        ago: "2 min ago".to_owned(),
        state,
        result: None,
        told: None,
        edit: None,
    })
}

/// The texts `mcp`'s panel shows, in a window 1000 × 600.
fn panel_texts(mcp: &McpShown<'_>) -> Vec<String> {
    let panel = panel(mcp).expect("the panel is open");
    let mut laid = Laid::new(panel, Size::new(1000.0, 600.0));
    laid.texts().into_iter().map(|shown| shown.text).collect()
}

#[test]
fn the_toggle_shows_while_on_and_the_chip_with_a_client() {
    let mut mcp = serving();
    assert!(mcp.toggle_shown());
    assert_eq!(mcp.chip(), Some("Claude Code"));
    // A client that gave no name.
    mcp.client = Some(None);
    assert_eq!(mcp.chip(), Some("A client"));
    mcp.client = None;
    assert_eq!(mcp.chip(), None);
    assert!(status_chip(Some(&mcp)).is_none());
    // The feature off: no toggle, chip or panel, the panel open or not.
    mcp.on = false;
    mcp.client = Some(Some("Claude Code"));
    assert!(!mcp.toggle_shown());
    assert_eq!(mcp.chip(), None);
    assert!(toggle(Some(&mcp)).is_none());
    assert!(panel(&mcp).is_none());
    // None on the web.
    assert!(toggle(None).is_none() && status_chip(None).is_none());
}

#[test]
fn the_panel_lists_the_calls_in_the_users_words() {
    let mut mcp = serving();
    mcp.rows = vec![
        call(McpTool::DesignSummary, "", McpCallState::Answered),
        call(McpTool::Feature, "Extrude 2", McpCallState::Failed),
        call(McpTool::BodyTopology, "Body 1", McpCallState::Waiting),
    ];
    mcp.dropped = 3;
    let texts = panel_texts(&mcp);
    let at = |text: &str| {
        (texts.iter())
            .position(|shown| shown == text)
            .unwrap_or_else(|| panic!("no {text:?} in {texts:?}"))
    };
    // The head: the chip, the client, Disconnect, Stop.
    for text in ["MCP", "Claude Code", "Disconnect", "Stop"] {
        at(text);
    }
    // Oldest first, the note of those let go above them.
    assert!(at("3 earlier calls aren't kept") < at("Read the design summary"));
    assert!(at("Read the design summary") < at("Looked at"));
    assert!(at("Looked at") < at("Extrude 2"));
    assert!(at("Extrude 2") < at("Listed the faces and edges of"));
    at("Body 1");
    at("2 min ago");
    // Its session ended, the calls stay with a note.
    mcp.client = None;
    mcp.gone = Some(Some("Claude Code"));
    let texts = panel_texts(&mcp);
    assert!(texts.iter().any(|text| text == "Claude Code disconnected"));
    assert!(texts.iter().any(|text| text == "No client"));
    assert!(!texts.iter().any(|text| text == "Disconnect"));
}

#[test]
fn stopped_the_panel_says_so_with_start() {
    let mut mcp = serving();
    mcp.client = None;
    mcp.status = McpStatus::Off;
    let texts = panel_texts(&mcp);
    assert!(texts.iter().any(|text| text == "Start"), "{texts:?}");
    assert!(texts.iter().any(|text| text == "Stopped"), "{texts:?}");
    assert!(
        texts
            .iter()
            .any(|text| text.starts_with("The MCP server is stopped")
                && text.contains("127.0.0.1:7311/mcp"))
    );
    // Started, but the port couldn't be bound: said, with Start to try
    // again.
    mcp.status = McpStatus::BindFailed("address in use");
    let texts = panel_texts(&mcp);
    assert!(texts.iter().any(|text| text == "Start"), "{texts:?}");
    assert!(
        texts.iter().any(|text| text
            == "Port 7311 can't be used: address in use. Choose another in the settings"),
        "{texts:?}"
    );
    // Starting.
    mcp.status = McpStatus::Starting;
    let texts = panel_texts(&mcp);
    assert!(texts.iter().any(|text| text == "Stop"), "{texts:?}");
    assert!(texts.iter().any(|text| text == "Starting…"), "{texts:?}");
}

#[test]
fn the_popup_asks_then_says_the_edit_was_cancelled() {
    let mut mcp = serving();
    assert!(ask_popup(&mcp).is_none());
    mcp.ask = Some(McpAsk {
        client: Some("Claude Code"),
        editing: "editing Sketch 2",
        tool: McpTool::Rename,
        args: "feature Sketch 2 to Slot",
        refused: None,
        cancelled: false,
    });
    let texts = |mcp: &McpShown<'_>| -> Vec<String> {
        let popup = ask_popup(mcp).expect("the popup is up");
        let mut laid = Laid::new(popup, Size::new(1000.0, 600.0));
        laid.texts().into_iter().map(|shown| shown.text).collect()
    };
    let shown = texts(&mcp);
    assert!(shown.iter().any(
        |text| text == "Claude Code wants to change the design while you're editing Sketch 2."
    ));
    assert!((shown.iter()).any(|text| text.contains("Renamed feature Sketch 2 to Slot")));
    for button in ["Keep editing", "Apply yours"] {
        assert!(shown.iter().any(|text| text == button), "{shown:?}");
    }
    // Apply yours refused: why, under the question.
    if let Some(ask) = &mut mcp.ask {
        ask.refused = Some("Your Extrude 2 wasn't applied: it isn't complete");
    }
    assert!((texts(&mcp).iter()).any(|text| text.starts_with("Your Extrude 2 wasn't")));
    // Cancelled: said so, with only Close.
    if let Some(ask) = &mut mcp.ask {
        ask.refused = None;
        ask.cancelled = true;
    }
    let shown = texts(&mcp);
    assert!(
        shown
            .iter()
            .any(|text| text == "Claude Code's edit was cancelled")
    );
    assert!(shown.iter().any(|text| text == "Close"));
    assert!(
        !shown
            .iter()
            .any(|text| text == "Apply yours" || text == "Keep editing")
    );
}
