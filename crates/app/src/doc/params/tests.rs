use iced::keyboard;
use varde_document::{Extent, FeatureKind};
use varde_expr::Value;
use varde_regen::Request;
use varde_view::{Edit, Look, RailLook};

use super::*;
use crate::tests::{example, key_in, screen_texts};

fn send(doc: &mut Doc, message: ParamEdit) {
    doc.update(Edit::Param(message));
}

fn type_in(doc: &mut Doc, index: usize, field: ParamField, text: &str) {
    doc.look(Look::Params(ParamsLook::Input {
        index,
        field,
        text: text.to_owned(),
    }));
}

fn params(doc: &Doc) -> Vec<(String, String)> {
    (doc.editor.document().params().iter())
        .map(|param| (param.name.clone(), param.text.clone()))
        .collect()
}

/// Sets the example's extrude's distance to `text`: its id.
fn extrude_by(doc: &mut Doc, text: &str) -> varde_document::FeatureId {
    let document = doc.editor.document();
    let (id, mut extrude) = (document.features().iter())
        .find_map(|feature| match &feature.kind {
            FeatureKind::Extrude(extrude) => Some((feature.id, extrude.clone())),
            _ => None,
        })
        .unwrap();
    let value = Value::new(text, &Extent::ask(&document.design())).unwrap();
    extrude.extent = Extent::OneSide(value);
    doc.apply(Command::SetFeature {
        feature: id,
        kind: Box::new(extrude.into()),
    });
    assert!(doc.edit_error.is_none(), "{:?}", doc.edit_error);
    doc.sync();
    id
}

fn regenerates(requests: &std::cell::RefCell<Vec<Request>>) -> bool {
    (requests.take().iter()).any(|request| matches!(request, Request::Regenerate { .. }))
}

#[test]
fn the_tool_opens_the_popup_and_closes_it() {
    let (mut doc, _) = example();
    assert!(doc.params_state().is_none());
    let popup =
        |doc: &Doc| (screen_texts(doc).iter()).any(|text| text.starts_with("No parameters yet."));
    assert!(!popup(&doc));
    // The rail's Modify set: its list, then Enter on Parameters, its last.
    doc.look(Look::Rail(RailLook::Open(1)));
    doc.look(Look::Rail(RailLook::Row(8)));
    key_in(&mut doc, keyboard::Key::Named(keyboard::key::Named::Enter));
    assert!(doc.params_state().is_some());
    assert_eq!(doc.rail.open, None);
    let texts = screen_texts(&doc);
    assert!(texts.contains(&"Parameters".to_owned()), "{texts:?}");
    assert!(
        texts
            .iter()
            .any(|text| text.starts_with("No parameters yet.")),
        "{texts:?}"
    );
    // Esc leaves it open: only the tool and Close close it.
    key_in(&mut doc, keyboard::Key::Named(keyboard::key::Named::Escape));
    assert!(doc.params_state().is_some());
    doc.look(Look::ToggleParams);
    assert!(doc.params_state().is_none());
}

#[test]
fn add_makes_a_parameter_whose_name_takes_the_focus() {
    let (mut doc, _) = example();
    doc.look(Look::ToggleParams);
    send(&mut doc, ParamEdit::Add);
    send(&mut doc, ParamEdit::Add);
    assert_eq!(
        params(&doc),
        [
            ("param1".into(), "10 mm".into()),
            ("param2".into(), "10 mm".into())
        ]
    );
    assert_eq!(doc.take_param_focus(), Some(1));
    assert_eq!(doc.take_param_focus(), None);
    let texts = screen_texts(&doc);
    for text in [
        "Name",
        "Expression",
        "Value",
        "Used by",
        "10 mm",
        "Not used",
        "2",
    ] {
        assert!(texts.contains(&text.to_owned()), "{text}: {texts:?}");
    }
    // Each one undo step.
    doc.update(Edit::Undo);
    assert_eq!(params(&doc).len(), 1);
}

#[test]
fn an_expression_commits_on_enter_and_regenerates() {
    let (mut doc, requests) = example();
    doc.look(Look::ToggleParams);
    send(&mut doc, ParamEdit::Add);
    let extrude = extrude_by(&mut doc, "param1");
    requests.take();
    type_in(&mut doc, 0, ParamField::Expression, "12 mm");
    // Typing alone changes nothing.
    assert_eq!(params(&doc)[0].1, "10 mm");
    let field = ParamField::Expression;
    send(&mut doc, ParamEdit::Commit { index: 0, field });
    assert_eq!(params(&doc)[0].1, "12 mm");
    assert!(regenerates(&requests));
    let state = doc.params_state().unwrap();
    assert!(state.drafts.is_empty());
    let Some(FeatureKind::Extrude(extruded)) =
        (doc.editor.document().feature(extrude)).map(|feature| &feature.kind)
    else {
        panic!()
    };
    let Extent::OneSide(value) = &extruded.extent else {
        panic!()
    };
    assert_eq!(value.value, 12.0);
    doc.update(Edit::Undo);
    assert_eq!(params(&doc)[0].1, "10 mm");
}

#[test]
fn a_refused_expression_stays_in_its_field_with_why() {
    let (mut doc, _) = example();
    doc.look(Look::ToggleParams);
    send(&mut doc, ParamEdit::Add);
    extrude_by(&mut doc, "param1");
    let undo = doc.editor.can_undo();
    type_in(&mut doc, 0, ParamField::Expression, "-5 mm");
    let field = ParamField::Expression;
    send(&mut doc, ParamEdit::Commit { index: 0, field });
    assert_eq!(params(&doc)[0].1, "10 mm");
    assert!(doc.edit_error.is_none());
    assert_eq!(doc.editor.can_undo(), undo);
    let state = doc.params_state().unwrap();
    let draft = &state.drafts[0];
    assert_eq!(draft.text, "-5 mm");
    let why = draft.error.clone().unwrap();
    assert!(why.starts_with("Extrude"), "{why}");
    assert!(screen_texts(&doc).contains(&why));
    // Esc in the field puts it back.
    doc.look(Look::Params(ParamsLook::Revert { index: 0, field }));
    assert!(doc.params_state().unwrap().drafts.is_empty());
}

#[test]
fn a_name_commits_as_its_field_is_left_and_its_uses_follow() {
    let (mut doc, _) = example();
    doc.look(Look::ToggleParams);
    send(&mut doc, ParamEdit::Add);
    send(&mut doc, ParamEdit::Add);
    let extrude = extrude_by(&mut doc, "param1 / 2");
    type_in(&mut doc, 0, ParamField::Name, "height");
    // Typing in another field leaves the name's.
    type_in(&mut doc, 1, ParamField::Expression, "height");
    assert_eq!(params(&doc)[0].0, "height");
    let Some(FeatureKind::Extrude(extruded)) =
        (doc.editor.document().feature(extrude)).map(|feature| &feature.kind)
    else {
        panic!()
    };
    let Extent::OneSide(value) = &extruded.extent else {
        panic!()
    };
    assert_eq!(value.text, "height / 2");
    // Acting elsewhere leaves the expression's.
    doc.look(Look::ClearSelection);
    assert_eq!(params(&doc)[1].1, "height");
    // A name another has is refused.
    type_in(&mut doc, 1, ParamField::Name, "height");
    let field = ParamField::Name;
    send(&mut doc, ParamEdit::Commit { index: 1, field });
    assert_eq!(params(&doc)[1].0, "param2");
    let state = doc.params_state().unwrap();
    assert_eq!(
        state.drafts[0].error.as_deref(),
        Some("Another parameter has this name")
    );
    // An undo drops the drafts of the list as it was.
    doc.update(Edit::Undo);
    assert!(doc.params_state().unwrap().drafts.is_empty());
}

#[test]
fn a_parameter_in_use_isn_t_deleted() {
    let (mut doc, _) = example();
    doc.look(Look::ToggleParams);
    send(&mut doc, ParamEdit::Add);
    send(&mut doc, ParamEdit::Add);
    send(&mut doc, ParamEdit::Add);
    extrude_by(&mut doc, "param1");
    let extrude_name = doc.editor.document().features()[1].name.clone();
    send(&mut doc, ParamEdit::Remove(0));
    assert_eq!(params(&doc).len(), 3);
    let state = doc.params_state().unwrap();
    let (index, why) = state.refused.unwrap();
    assert_eq!(index, 0);
    assert!(why.contains(&extrude_name), "{why}");
    assert!(screen_texts(&doc).iter().any(|text| text == why));
    // One in error that nothing uses is taken, and goes up a row as one
    // before it is removed.
    type_in(&mut doc, 2, ParamField::Expression, "nope +");
    let field = ParamField::Expression;
    send(&mut doc, ParamEdit::Commit { index: 2, field });
    send(&mut doc, ParamEdit::Remove(1));
    assert_eq!(params(&doc)[1].0, "param3");
    assert!(doc.params_state().unwrap().refused.is_none());
    assert_eq!(params(&doc)[1].1, "nope +");
}

/// Adds a sketch of a triangle to `doc`, its first corner fixed and base
/// horizontal, its sides driven by dimensions `base_text`, `6` and `8`
/// (10, 6, 8 as drawn): the sketch's id and the corners.
fn triangle(doc: &mut Doc, base_text: &str) -> (varde_document::FeatureId, [varde_sketch::Id; 3]) {
    use glam::DVec2;
    use varde_document::{OriginPlane, Plane};
    use varde_sketch::{Constraint, Curve, Dimension, Measure, Side, Sketch};

    doc.apply((doc.editor.document()).add_sketch(Plane::Origin(OriginPlane::XY)));
    let feature = doc.editor.document().features().last().unwrap().id;
    let mut sketch = Sketch::default();
    let corners = [(0.0, 0.0), (10.0, 0.0), (6.4, 4.8)]
        .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
    let sides = [0, 1, 2].map(|i| {
        let line = Curve::Line {
            start: corners[i],
            end: corners[(i + 1) % 3],
        };
        sketch.add_curve(line, false).unwrap()
    });
    sketch.add_constraint(Constraint::Fix(corners[0])).unwrap();
    (sketch.add_constraint(Constraint::Horizontal(sides[0]))).unwrap();
    let design = doc.editor.document().design();
    for (side, text) in sides.into_iter().zip([base_text, "6", "8"]) {
        let measure = Measure::Length(side);
        let value = Value::new(text, &measure.ask(&design)).unwrap();
        let dimension = Dimension {
            measure,
            value,
            driving: true,
            label: DVec2::ZERO,
            side: Side::Positive,
        };
        sketch.add_dimension(dimension).unwrap();
    }
    doc.apply(Command::SetSketch {
        feature,
        sketch: Box::new(sketch),
    });
    assert!(doc.edit_error.is_none(), "{:?}", doc.edit_error);
    doc.sync();
    (feature, corners)
}

fn sketch_of(doc: &Doc, feature: varde_document::FeatureId) -> &varde_sketch::Sketch {
    match &doc.editor.document().feature(feature).unwrap().kind {
        FeatureKind::Sketch { sketch, .. } => sketch,
        _ => panic!("not a sketch"),
    }
}

#[test]
fn a_dimension_naming_a_parameter_is_solved_again_as_it_changes() {
    let (mut doc, requests) = example();
    doc.look(Look::ToggleParams);
    send(&mut doc, ParamEdit::Add);
    type_in(&mut doc, 0, ParamField::Name, "width");
    send(
        &mut doc,
        ParamEdit::Commit {
            index: 0,
            field: ParamField::Name,
        },
    );
    type_in(&mut doc, 0, ParamField::Expression, "10 mm");
    let field = ParamField::Expression;
    send(&mut doc, ParamEdit::Commit { index: 0, field });
    let (sketch, corners) = triangle(&mut doc, "width");
    let sketch_name = doc.editor.document().feature(sketch).unwrap().name.clone();
    let before = doc.editor.document().clone();
    requests.take();

    type_in(&mut doc, 0, field, "12 mm");
    send(&mut doc, ParamEdit::Commit { index: 0, field });
    assert_eq!(params(&doc)[0].1, "12 mm");
    let solved = sketch_of(&doc, sketch);
    assert_eq!(solved.dimensions[0].dimension.value.value, 12.0);
    let corner = solved.point(corners[1]).unwrap().at;
    assert!((corner - glam::DVec2::new(12.0, 0.0)).length() < 1e-9);
    assert!(regenerates(&requests));
    // "Used by" names the sketch.
    send(&mut doc, ParamEdit::Remove(0));
    let (_, why) = doc.params_state().unwrap().refused.unwrap();
    assert!(why.contains(&sketch_name), "{why}");
    // One undo puts back the parameter and the sketch.
    doc.update(Edit::Undo);
    assert_eq!(*doc.editor.document(), before);

    // A value the sketch can't solve with is refused, saying which.
    type_in(&mut doc, 0, field, "1 mm");
    send(&mut doc, ParamEdit::Commit { index: 0, field });
    assert_eq!(*doc.editor.document(), before);
    let why = doc.params_state().unwrap().drafts[0].error.clone().unwrap();
    assert!(
        why.starts_with(&format!("{sketch_name} wouldn't solve")),
        "{why}"
    );
}

/// Answers what waits on `lane`, as its messages would.
fn answer_solver(doc: &mut Doc, lane: &mut crate::tests::SolveLane) {
    while let Some(response) = lane.respond() {
        doc.solved(response);
    }
    doc.sync();
}

#[test]
fn commits_waiting_on_the_solver_keep_their_drafts_until_made() {
    use varde_document::OriginPlane;
    let (mut doc, _) = example();
    doc.look(Look::ToggleParams);
    for _ in 0..3 {
        send(&mut doc, ParamEdit::Add);
    }
    extrude_by(&mut doc, "param1");
    let mut lane = crate::tests::SolveLane::connect(&mut doc);
    doc.update(Edit::PlanePicked(OriginPlane::XY));
    answer_solver(&mut doc, &mut lane);
    doc.look(Look::SelectTool(varde_view::Tool::Point));
    doc.update(Edit::ToolClick(varde_view::ToolClick {
        at: glam::DVec2::new(1.0, 2.0),
        target: None,
        inference: None,
        pixel: 0.1,
        double: false,
        hit: None,
        reference: false,
    }));
    assert!(doc.proposing());

    // While the point waits: a change to the third, the second's removal
    // and a change the extrude refuses all wait behind it.
    let field = ParamField::Expression;
    type_in(&mut doc, 2, field, "3 mm");
    send(&mut doc, ParamEdit::Commit { index: 2, field });
    send(&mut doc, ParamEdit::Remove(1));
    type_in(&mut doc, 0, field, "-5 mm");
    send(&mut doc, ParamEdit::Commit { index: 0, field });
    assert_eq!(params(&doc).len(), 3);
    let drafts = |doc: &Doc| {
        let state = doc.params_state().unwrap();
        (state.drafts.iter())
            .map(|draft| (draft.index, draft.text.clone(), draft.error.clone()))
            .collect::<Vec<_>>()
    };
    // Their drafts stay as typed, on their rows, with no error yet.
    assert_eq!(
        drafts(&doc),
        [(2, "3 mm".to_owned(), None), (0, "-5 mm".to_owned(), None)]
    );

    // Once the point is in, each is made in turn: the change taken, the
    // row removed, and the refusal said in its field.
    answer_solver(&mut doc, &mut lane);
    assert!(!doc.proposing());
    assert_eq!(
        params(&doc),
        [
            ("param1".to_owned(), "10 mm".to_owned()),
            ("param3".to_owned(), "3 mm".to_owned())
        ]
    );
    let after = drafts(&doc);
    assert_eq!(after.len(), 1, "{after:?}");
    let (index, text, error) = &after[0];
    assert_eq!((*index, text.as_str()), (0, "-5 mm"));
    assert!(error.as_ref().unwrap().starts_with("Extrude"), "{error:?}");
}
