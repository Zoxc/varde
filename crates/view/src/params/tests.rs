use iced::Size;
use varde_document::{Command, Document, Editor, Extent, FeatureKind};
use varde_expr::Value;

use super::*;
use crate::operation_panel::{PANEL_TOP, placed};
use crate::probe::Shown;
use crate::testing::Laid;

/// The example with parameters `height = 10 mm`, `wall = height / 5` and
/// `bad = nope`, its extrude's distance `wall`: the editor and the
/// extrude's id.
fn with_params() -> (Editor, FeatureId) {
    let mut editor = Editor::new(Document::example());
    for (name, text) in [("height", "10 mm"), ("wall", "height / 5"), ("bad", "nope")] {
        let command = Command::AddParam {
            name: name.into(),
            text: text.into(),
        };
        editor.apply(command).unwrap();
    }
    let document = editor.document();
    let (id, mut extrude) = (document.features().iter())
        .find_map(|feature| match &feature.kind {
            FeatureKind::Extrude(extrude) => Some((feature.id, extrude.clone())),
            _ => None,
        })
        .unwrap();
    let value = Value::new("wall", &Extent::ask(&document.design())).unwrap();
    extrude.extent = Extent::OneSide(value);
    let command = Command::SetFeature {
        feature: id,
        kind: Box::new(extrude.into()),
    };
    editor.apply(command).unwrap();
    (editor, id)
}

fn state(document: &Document) -> ParamsState<'_> {
    ParamsState {
        document,
        // Kept by the app for the document; for the test, for good.
        uses: Box::leak(document.all_param_uses().into_boxed_slice()),
        drafts: &[],
        refused: None,
        selected: None,
        editable: true,
    }
}

fn find<'s>(shown: &'s [Shown], text: &str) -> &'s Shown {
    shown
        .iter()
        .find(|shown| shown.text == text)
        .unwrap_or_else(|| panic!("no {text:?} in {shown:?}"))
}

#[test]
fn each_row_shows_its_value_its_uses_and_what_s_wrong() {
    let (editor, extrude) = with_params();
    let document = editor.document();
    let rows = ParamsState {
        selected: Some(extrude),
        ..state(document)
    }
    .rows();
    let extrude_name = &document.feature(extrude).unwrap().name;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].value, "10 mm");
    assert_eq!(rows[0].uses, "wall");
    assert!(!rows[0].lit && rows[0].error.is_none());
    assert_eq!(rows[1].value, "2 mm");
    assert_eq!(&rows[1].uses, extrude_name);
    assert!(rows[1].lit);
    assert_eq!(rows[2].value, "\u{2014}");
    assert_eq!(rows[2].uses, "Not used");
    assert!(rows[2].bad_text);
    let error = rows[2].error.as_deref().unwrap();
    assert!(error.starts_with(char::is_uppercase), "{error}");
}

#[test]
fn a_draft_shows_in_its_field_with_why_it_was_refused() {
    let (editor, _) = with_params();
    let drafts = [
        ParamDraft {
            index: 0,
            field: ParamField::Expression,
            text: "1 mm".into(),
            error: Some("wall would be in error".into()),
        },
        ParamDraft {
            index: 1,
            field: ParamField::Name,
            text: "height".into(),
            error: None,
        },
    ];
    let rows = ParamsState {
        drafts: &drafts,
        ..state(editor.document())
    }
    .rows();
    assert_eq!(rows[0].text, "1 mm");
    assert_eq!(rows[0].error.as_deref(), Some("Wall would be in error"));
    assert!(rows[0].bad_text);
    // Its value is still the document's.
    assert_eq!(rows[0].value, "10 mm");
    assert_eq!(rows[1].name, "height");
    assert!(rows[1].error.is_none());
}

#[test]
fn the_card_lists_the_table_under_its_head() {
    let (editor, _) = with_params();
    let mut laid = Laid::new(card(state(editor.document())), Size::new(800.0, 800.0));
    assert_eq!(laid.node.size().width, PARAMS_WIDTH);
    let shown = laid.texts();
    let title = find(&shown, "Parameters");
    let count = find(&shown, "3");
    assert!(count.bounds.x > title.bounds.x);
    let name = find(&shown, "Name");
    let expression = find(&shown, "Expression");
    let value = find(&shown, "Value");
    let used = find(&shown, "Used by");
    assert!(name.bounds.y > title.bounds.y);
    // The columns, 12 px in from the card's border.
    assert_eq!(name.bounds.x, 1.0 + 12.0);
    assert_eq!(expression.bounds.x, name.bounds.x + NAME_WIDTH + COLUMN_GAP);
    assert_eq!(
        used.bounds.x,
        PARAMS_WIDTH - 1.0 - 8.0 - DELETE_WIDTH - COLUMN_GAP - USES_WIDTH
    );
    assert_eq!(value.bounds.x, used.bounds.x - COLUMN_GAP - VALUE_WIDTH);
    let ten = find(&shown, "10 mm");
    assert_eq!(ten.bounds.x, value.bounds.x);
    assert_eq!(find(&shown, "Not used").bounds.x, used.bounds.x);
    // The add row last.
    let add = find(&shown, "Parameter");
    assert!(add.bounds.y > ten.bounds.y);
}

#[test]
fn with_none_the_card_says_how_to_start() {
    let document = Document::example();
    let mut laid = Laid::new(card(state(&document)), Size::new(800.0, 800.0));
    let shown = laid.texts();
    find(&shown, "0");
    assert!(
        shown
            .iter()
            .any(|shown| shown.text.starts_with("No parameters yet."))
    );
    find(&shown, "Parameter");
    assert!(!shown.iter().any(|shown| shown.text == "Name"));
}

#[test]
fn the_popup_goes_under_the_view_cube_or_left_of_an_operation_s_panel() {
    let (editor, _) = with_params();
    let max = Size::new(1200.0, 800.0);
    let mut laid = Laid::new(popup(state(editor.document()), false), max);
    let alone = laid.node.children()[0].bounds();
    assert_eq!(alone.y, PANEL_TOP);
    assert_eq!(alone.width, PARAMS_WIDTH);
    assert_eq!(alone.x + alone.width, max.width - PANEL_MARGIN);
    let _ = laid.texts();

    // Beside an operation's panel, both laid over the viewport as the
    // viewport stacks them.
    let panel = crate::operation_panel::operation_panel(crate::operation_panel::Parts {
        icon: Icon::Extrude,
        title: "Extrude",
        body: text("Body").into(),
        message: None,
        ok: None,
        cancel: Message::Look(Look::Escape),
        close: false,
    });
    let both = stack![placed(panel), popup(state(editor.document()), true)];
    let mut laid = Laid::new(both, max);
    let panel = laid.node.children()[0].children()[0].bounds();
    let beside = laid.node.children()[1].children()[0].bounds();
    assert_eq!(beside.y, PANEL_TOP);
    assert_eq!(beside.x + beside.width, panel.x - PANEL_MARGIN);
    assert_eq!(beside.width, PARAMS_WIDTH);
    let _ = laid.texts();

    // Narrower where the viewport is, clear of its left.
    let narrow = Size::new(700.0, 800.0);
    let mut laid = Laid::new(popup(state(editor.document()), true), narrow);
    let squeezed = laid.node.children()[0].bounds();
    assert_eq!(squeezed.x, PANEL_MARGIN);
    assert!(squeezed.width < PARAMS_WIDTH, "{squeezed:?}");
    let _ = laid.texts();
}

#[test]
fn a_long_list_scrolls_above_the_status_bar() {
    let mut editor = Editor::new(Document::example());
    for k in 0..40 {
        let command = Command::AddParam {
            name: format!("p{k}"),
            text: format!("{k} mm"),
        };
        editor.apply(command).unwrap();
    }
    let max = Size::new(1200.0, 600.0);
    let mut laid = Laid::new(popup(state(editor.document()), false), max);
    let card = laid.node.children()[0].bounds();
    assert_eq!(
        card.y + card.height,
        max.height - crate::operation_panel::PANEL_BOTTOM
    );
    let shown = laid.texts();
    assert!(find(&shown, "Parameters").whole());
    assert!(find(&shown, "39 mm").hidden());
}

#[test]
fn it_draws_in_both_themes() {
    let (editor, extrude) = with_params();
    for mode in [crate::Mode::Light, crate::Mode::Dark] {
        let shown = ParamsState {
            selected: Some(extrude),
            ..state(editor.document())
        };
        let mut laid = Laid::new(card(shown), Size::new(600.0, 400.0));
        let pixels = laid.pixels_in(Size::new(600, 400), mode);
        assert!(pixels.chunks(4).any(|pixel| pixel[3] > 0));
    }
}
