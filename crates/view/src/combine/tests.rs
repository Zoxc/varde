use super::*;
use crate::probe::Shown;
use crate::testing::Laid;

fn state_of<'a>(target: Option<CombineBody<'a>>, tools: Vec<CombineBody<'a>>) -> CombineState<'a> {
    CombineState {
        editing: None,
        target,
        tools,
        picking: CombinePick::Tools,
        op: BodyOp::Union,
        keep_tools: false,
        enough: true,
        error: None,
        checking: false,
        ready: false,
        editable: true,
    }
}

/// A body named `name`: the panel only sends ids, so they're all the
/// example's.
fn body(name: &str) -> CombineBody<'_> {
    let example = varde_document::Document::example();
    CombineBody {
        body: example.bodies()[0].id,
        name,
    }
}

/// Each text of `state`'s panel and where it's laid out.
fn texts_of(state: &CombineState<'_>) -> Vec<Shown> {
    Laid::new(panel(state), iced::Size::new(400.0, 800.0)).texts()
}

fn found<'s>(shown: &'s [Shown], text: &str) -> &'s Shown {
    shown
        .iter()
        .find(|shown| shown.text == text)
        .unwrap_or_else(|| panic!("no {text:?} in {shown:?}"))
}

fn has(shown: &[Shown], text: &str) -> bool {
    shown.iter().any(|shown| shown.text == text)
}

#[test]
fn the_panel_shows_the_bodies_as_chips_and_what_to_click() {
    let mut state = state_of(None, Vec::new());
    state.picking = CombinePick::Target;
    let shown = texts_of(&state);
    for text in [
        "New combine",
        "Target",
        "Click a body",
        "Tools",
        "Click bodies",
        "Operation",
        "Union",
        "Subtract",
        "Intersect",
        "Keep tool bodies",
        "Otherwise the tools are used up",
    ] {
        found(&shown, text);
    }

    // Picked: the chips in their fields, the tools one under another and
    // "+ Click bodies" after them while they're picked.
    let state = state_of(Some(body("Body 1")), vec![body("Body 2"), body("Body 3")]);
    let shown = texts_of(&state);
    let target = found(&shown, "Body 1");
    let (two, three) = (found(&shown, "Body 2"), found(&shown, "Body 3"));
    let more = found(&shown, "+ Click bodies");
    assert!(target.bounds.y < two.bounds.y);
    assert!(two.bounds.y < three.bounds.y && three.bounds.y < more.bounds.y);
    assert!((two.bounds.x - three.bounds.x).abs() < 0.5);
    assert!(!has(&shown, "Click a body"));
    found(&shown, "2 tools");

    // Picking the target, the tools' field asks for nothing more.
    let state = CombineState {
        picking: CombinePick::Target,
        ..state
    };
    assert!(!has(&texts_of(&state), "+ Click bodies"));
}

#[test]
fn the_footer_says_why_it_can_t_be_done() {
    let mut state = state_of(Some(body("Body 1")), Vec::new());
    state.enough = false;
    found(
        &texts_of(&state),
        "There’s only one body: make another to combine with",
    );
    state.enough = true;
    state.tools = vec![body("Body 2")];
    state.error = Some("cutting Body 2 from Body 1 would leave nothing of Body 1");
    found(
        &texts_of(&state),
        "Cutting Body 2 from Body 1 would leave nothing of Body 1",
    );
}
