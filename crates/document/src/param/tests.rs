use varde_expr::{ErrorKind, Kind, NameError, Quantity, Value};

use super::*;
use crate::testing::{extrude_of, with_body};
use crate::{
    Axis3, AxisRef, BodyId, CheckError, Command, Copies, EditError, Editor, Extent, LengthUnit,
    Pattern, PatternKind, ValueOf,
};

/// The example in an editor with parameters `width = 40 mm` and
/// `half = width / 2`, its extrude's distance `half`: the editor and the
/// extrude's id.
fn with_params() -> (Editor, FeatureId) {
    let example = with_body();
    let extrude = example.features[1].id;
    let mut editor = Editor::new(example);
    for (name, text) in [("width", " 40 mm "), ("half", "width / 2")] {
        editor
            .apply(Command::AddParam {
                name: name.into(),
                text: text.into(),
            })
            .unwrap();
    }
    set_distance(&mut editor, extrude, "half").unwrap();
    (editor, extrude)
}

/// Sets extrude `id`'s distance to `text`, read with the document's
/// parameters.
fn set_distance(editor: &mut Editor, id: FeatureId, text: &str) -> Result<(), EditError> {
    let document = editor.document();
    let value = Value::new(text, &Extent::ask(&document.design())).unwrap();
    let mut extrude = extrude_of(document, id).clone();
    extrude.extent = Extent::OneSide(value);
    editor.apply(Command::SetFeature {
        feature: id,
        kind: Box::new(extrude.into()),
    })
}

fn distance(editor: &Editor, id: FeatureId) -> Value {
    extrude_of(editor.document(), id)
        .extent
        .values()
        .next()
        .unwrap()
        .clone()
}

fn text(editor: &Editor, index: usize) -> &str {
    &editor.document().params()[index].text
}

#[test]
fn values_use_parameters_and_follow_them() {
    let (mut editor, extrude) = with_params();
    let document = editor.document();
    assert_eq!(text(&editor, 0), "40 mm", "kept trimmed");
    let resolved = document.params_resolved();
    assert_eq!(resolved.len(), 2);
    assert_eq!(resolved.get("half").unwrap().as_ref().unwrap().value, 20.0);
    assert_eq!(
        distance(&editor, extrude),
        Value {
            text: "half".into(),
            value: 20.0
        }
    );
    assert_eq!(document.param_uses("half"), [extrude]);
    assert_eq!(document.param_uses("width"), []);
    assert_eq!(document.param_users("width"), [1]);
    assert!(document.param_used("width"));
    let before = document.clone();

    // Through `half`, the extrude follows `width`, in one step.
    editor
        .apply(Command::SetParam {
            index: 0,
            text: "60 mm".into(),
        })
        .unwrap();
    assert_eq!(distance(&editor, extrude).value, 30.0);
    editor.undo();
    assert_eq!(*editor.document(), before);
    editor.redo();

    // Its own text the same changes nothing.
    let revision = editor.revision();
    editor
        .apply(Command::SetParam {
            index: 0,
            text: "60 mm".into(),
        })
        .unwrap();
    editor
        .apply(Command::SetParam {
            index: 5,
            text: "1".into(),
        })
        .unwrap();
    assert_eq!(editor.revision(), revision);
}

#[test]
fn a_parameter_that_would_break_a_value_is_refused() {
    let (mut editor, extrude) = with_params();
    let before = editor.document().clone();
    let set = |editor: &mut Editor, text: &str| {
        editor.apply(Command::SetParam {
            index: 0,
            text: text.into(),
        })
    };
    // A number isn't a length.
    let error = set(&mut editor, "40").unwrap_err();
    let EditError::Value(ValueOf::Feature(id), why) = error else {
        panic!("{error:?}");
    };
    assert_eq!(id, extrude);
    assert_eq!(
        why.kind,
        ErrorKind::Wrong {
            want: Quantity::Length,
            got: Kind::Number
        }
    );
    // Nor may it break, or go past what the extrude takes.
    assert!(set(&mut editor, "widht").is_err());
    assert!(set(&mut editor, "half").is_err());
    assert!(set(&mut editor, "1e9 mm").is_err());
    assert_eq!(*editor.document(), before);

    // One nothing uses may be in error.
    editor
        .apply(Command::AddParam {
            name: "spare".into(),
            text: "nothing + 1".into(),
        })
        .unwrap();
    let resolved = editor.document().params_resolved();
    assert!(resolved.get("spare").unwrap().is_err());
    // And a value can't use it.
    let document = editor.document();
    assert!(Value::new("spare", &Extent::ask(&document.design())).is_err());
}

#[test]
fn names_are_checked() {
    let (mut editor, _) = with_params();
    let add = |editor: &mut Editor, name: &str| {
        editor.apply(Command::AddParam {
            name: name.into(),
            text: "1".into(),
        })
    };
    assert_eq!(
        add(&mut editor, "mm"),
        Err(EditError::Invalid(CheckError::Param(
            2,
            ParamError::Name(NameError::Unit)
        )))
    );
    assert_eq!(
        add(&mut editor, "width"),
        Err(EditError::Invalid(CheckError::Param(
            2,
            ParamError::Duplicate
        )))
    );
    assert_eq!(
        add(&mut editor, "a b"),
        Err(EditError::Invalid(CheckError::Param(
            2,
            ParamError::Name(NameError::NotAWord)
        )))
    );
    assert!(
        editor
            .apply(Command::AddParam {
                name: "long".into(),
                text: "1".repeat(MAX_LEN + 1),
            })
            .is_err()
    );
    assert!(
        editor
            .apply(Command::RenameParam {
                index: 0,
                name: "half".into(),
            })
            .is_err()
    );
    assert_eq!(editor.document().params().len(), 2);
    assert_eq!(editor.document().new_param_name("width"), "width1");
}

#[test]
fn renaming_rewrites_every_use() {
    let (mut editor, extrude) = with_params();
    editor
        .apply(Command::RenameParam {
            index: 0,
            name: "w".into(),
        })
        .unwrap();
    assert_eq!(text(&editor, 1), "w / 2");
    editor
        .apply(Command::RenameParam {
            index: 1,
            name: "h".into(),
        })
        .unwrap();
    assert_eq!(distance(&editor, extrude).text, "h");
    assert_eq!(distance(&editor, extrude).value, 20.0);
    assert_eq!(editor.document().params()[1].name, "h");
    editor.undo();
    assert_eq!(distance(&editor, extrude).text, "half");

    // A rename taking a text past the limit is refused.
    let long = format!("w{}", " + w".repeat(60));
    editor
        .apply(Command::AddParam {
            name: "many".into(),
            text: long,
        })
        .unwrap();
    let error = editor
        .apply(Command::RenameParam {
            index: 0,
            name: "much_longer".into(),
        })
        .unwrap_err();
    assert!(matches!(error, EditError::Value(ValueOf::Param(2), _)));
}

#[test]
fn a_used_parameter_stays() {
    let (mut editor, extrude) = with_params();
    assert_eq!(
        editor.apply(Command::RemoveParam(0)),
        Err(EditError::ParamUsed("width".into()))
    );
    assert_eq!(
        editor.apply(Command::RemoveParam(1)),
        Err(EditError::ParamUsed("half".into()))
    );
    set_distance(&mut editor, extrude, "25").unwrap();
    editor.apply(Command::RemoveParam(1)).unwrap();
    editor.apply(Command::RemoveParam(0)).unwrap();
    assert!(editor.document().params().is_empty());
    assert!(editor.document().params_resolved().is_empty());
}

#[test]
fn changing_units_pins_parameters() {
    let (mut editor, extrude) = with_params();
    for (name, text) in [("n", "2 * 3"), ("more", "width + 1"), ("broken", "x + 1")] {
        editor
            .apply(Command::AddParam {
                name: name.into(),
                text: text.into(),
            })
            .unwrap();
    }
    editor.apply(Command::SetUnits(LengthUnit::In)).unwrap();
    assert_eq!(text(&editor, 0), "40 mm");
    assert_eq!(text(&editor, 1), "width / 2");
    assert_eq!(text(&editor, 2), "2 * 3");
    assert_eq!(text(&editor, 3), "width + 1 mm");
    assert_eq!(text(&editor, 4), "x + 1", "in error, left");
    let resolved = editor.document().params_resolved();
    assert_eq!(resolved.get("more").unwrap().as_ref().unwrap().value, 41.0);
    assert_eq!(distance(&editor, extrude).value, 20.0);
    // A file holds it, and reads it back resolved.
    let document = editor.document();
    assert_eq!(
        Document::from_postcard(&document.to_postcard()).as_ref(),
        Ok(document)
    );
}

#[test]
fn a_pattern_count_from_a_parameter_makes_its_copies() {
    let example = with_body();
    let body = example.bodies[0].id;
    let mut editor = Editor::new(example);
    editor
        .apply(Command::AddParam {
            name: "n".into(),
            text: "3".into(),
        })
        .unwrap();
    let design = editor.document().design();
    let pattern = Pattern {
        bodies: vec![body],
        kind: PatternKind::Linear {
            along: AxisRef::Origin(Axis3::X),
            count: Value::new("n", &Pattern::count_ask(&design)).unwrap(),
            spacing: Value::new("30", &Pattern::spacing_ask(&design)).unwrap(),
        },
        copies: Copies::Separate(Vec::new()),
    };
    editor
        .apply(editor.document().add_feature(pattern.into()))
        .unwrap();
    let copies = |editor: &Editor| -> Vec<BodyId> {
        match &editor.document().features().last().unwrap().kind {
            FeatureKind::Pattern(pattern) => match &pattern.copies {
                Copies::Separate(made) => made.clone(),
                Copies::Joined => Vec::new(),
            },
            _ => panic!("not a pattern"),
        }
    };
    assert_eq!(copies(&editor).len(), 2);
    let bodies = editor.document().bodies().len();
    editor
        .apply(Command::SetParam {
            index: 0,
            text: "5".into(),
        })
        .unwrap();
    assert_eq!(copies(&editor).len(), 4);
    assert_eq!(editor.document().bodies().len(), bodies + 2);
    editor
        .apply(Command::SetParam {
            index: 0,
            text: "2".into(),
        })
        .unwrap();
    assert_eq!(copies(&editor).len(), 1);
}

#[test]
fn values_lists_every_typed_value() {
    let (editor, extrude) = with_params();
    let document = editor.document();
    let kind = &document.feature(extrude).unwrap().kind;
    assert_eq!(kind.values(), [&distance(&editor, extrude)]);
    let mut kind = kind.clone();
    let values = kind.values_mut(&document.design());
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].1, Extent::ask(&document.design()));
}

/// A design with `side = 10 mm` and a sketch of a triangle, its first
/// corner fixed and base horizontal, its sides driven by dimensions
/// `side`, `8` and `6`: the editor, the sketch feature, the corners.
fn with_triangle() -> (Editor, FeatureId, [varde_sketch::Id; 3]) {
    use glam::DVec2;
    use varde_sketch::{Constraint, Curve, Dimension, Measure, Side, Sketch};

    let mut editor = Editor::new(Document::default());
    let plane = crate::Plane::Origin(crate::OriginPlane::XY);
    editor.apply(editor.document().add_sketch(plane)).unwrap();
    let feature = editor.document().features()[0].id;
    editor
        .apply(Command::AddParam {
            name: "side".into(),
            text: "10 mm".into(),
        })
        .unwrap();
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
    sketch
        .add_constraint(Constraint::Horizontal(sides[0]))
        .unwrap();
    let design = editor.document().design();
    for (side, text) in sides.into_iter().zip(["side", "6", "8"]) {
        let measure = Measure::Length(side);
        let value = Value::new(text, &measure.ask(&design)).unwrap();
        sketch
            .add_dimension(Dimension {
                measure,
                value,
                driving: true,
                label: DVec2::ZERO,
                side: Side::Positive,
            })
            .unwrap();
    }
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    (editor, feature, corners)
}

fn sketch_in(editor: &Editor, feature: FeatureId) -> &varde_sketch::Sketch {
    match &editor.document().feature(feature).unwrap().kind {
        FeatureKind::Sketch { sketch, .. } => sketch,
        _ => panic!("not a sketch"),
    }
}

#[test]
fn a_dimension_follows_its_parameter_in_one_step() {
    let (mut editor, feature, corners) = with_triangle();
    assert_eq!(editor.document().param_uses("side"), [feature]);
    let before = editor.document().clone();
    editor
        .apply(Command::SetParam {
            index: 0,
            text: "12 mm".into(),
        })
        .unwrap();
    let sketch = sketch_in(&editor, feature);
    let base = &sketch.dimensions[0].dimension.value;
    assert_eq!((base.text.as_str(), base.value), ("side", 12.0));
    let corner = sketch.point(corners[1]).unwrap().at;
    assert!((corner - glam::DVec2::new(12.0, 0.0)).length() < 1e-9);
    editor.document().check().unwrap();
    // One undo puts the parameter and the sketch back.
    editor.undo();
    assert_eq!(*editor.document(), before);
}

#[test]
fn a_parameter_a_sketch_cannot_solve_with_is_refused() {
    let (mut editor, feature, _) = with_triangle();
    let before = editor.document().clone();
    // No triangle has sides 1, 6 and 8.
    let set = |text: &str| Command::SetParam {
        index: 0,
        text: text.into(),
    };
    assert!(matches!(
        editor.apply(set("1 mm")),
        Err(EditError::Unsolved(id, _)) if id == feature
    ));
    // A number isn't a length.
    assert!(matches!(
        editor.apply(set("10")),
        Err(EditError::Value(ValueOf::Feature(id), _)) if id == feature
    ));
    assert_eq!(*editor.document(), before);
}

#[test]
fn a_sketch_out_of_time_refuses_the_parameter() {
    let (mut editor, feature, _) = with_triangle();
    let before = editor.document().clone();
    let set = Command::SetParam {
        index: 0,
        text: "12 mm".into(),
    };
    let expired = || true;
    let budget = varde_sketch::Budget {
        expired: &expired,
        ..varde_sketch::Budget::default()
    };
    assert!(matches!(
        editor.apply_within(set.clone(), &budget),
        Err(EditError::Unsolved(
            id,
            varde_sketch::Rejected::Unsolved(varde_sketch::Failure::OutOfTime)
        )) if id == feature
    ));
    assert_eq!(*editor.document(), before);
    // With time, it's taken.
    editor
        .apply_within(set, &varde_sketch::Budget::default())
        .unwrap();
}

#[test]
fn renaming_rewrites_dimensions_and_a_used_one_stays() {
    let (mut editor, feature, _) = with_triangle();
    editor
        .apply(Command::RenameParam {
            index: 0,
            name: "base".into(),
        })
        .unwrap();
    let value = &sketch_in(&editor, feature).dimensions[0].dimension.value;
    assert_eq!((value.text.as_str(), value.value), ("base", 10.0));
    editor.document().check().unwrap();
    assert_eq!(
        editor.apply(Command::RemoveParam(0)),
        Err(EditError::ParamUsed("base".into()))
    );
}

#[test]
fn a_dimension_naming_a_parameter_is_read_back_and_checked_with_it() {
    let (editor, _, _) = with_triangle();
    let document = editor.document();
    let read = Document::from_postcard(&document.to_postcard()).unwrap();
    assert_eq!(read, *document);
    // Without the parameter, the dimension names nothing: refused.
    let mut lacking = document.clone();
    lacking.params.clear();
    lacking.resolved = resolve(&lacking.params, lacking.units);
    assert!(matches!(lacking.check(), Err(CheckError::Sketch(..))));
}

#[test]
fn all_uses_agree_with_each_parameter_s() {
    let (mut editor, _, _) = with_triangle();
    for (name, text) in [
        ("double", "side * 2"),
        ("own", "double + side"),
        ("free", "1"),
    ] {
        let command = Command::AddParam {
            name: name.into(),
            text: text.into(),
        };
        editor.apply(command).unwrap();
    }
    let (with_extrude, _) = with_params();
    for document in [editor.document(), with_extrude.document()] {
        let all = document.all_param_uses();
        assert_eq!(all.len(), document.params().len());
        for (param, uses) in document.params().iter().zip(&all) {
            assert_eq!(
                uses.features,
                document.param_uses(&param.name),
                "{}",
                param.name
            );
            assert_eq!(
                uses.params,
                document.param_users(&param.name),
                "{}",
                param.name
            );
        }
    }
}
