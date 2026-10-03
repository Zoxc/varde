use glam::DVec3;
use varde_kernel::mesh::{FaceKey, PartKey};

use super::*;
use crate::testing::{extrude_again, with_body};
use crate::{
    Axis3, CheckError, Command, Document, EdgeError, EdgeRef, EditError, Editor, FaceRef,
    FeatureId, FeatureKind, LengthUnit, MAX_FEATURE_BODIES, Removable,
};

/// The example's body and another plate: the editor, the bodies and
/// their makers.
fn two_bodies() -> (Editor, [BodyId; 2], [FeatureId; 2]) {
    let example = with_body();
    let first = (example.bodies[0].id, example.features[1].id);
    let mut editor = Editor::new(example);
    let (maker, body) = extrude_again(&mut editor);
    (editor, [first.0, body], [first.1, maker])
}

fn count(document: &Document, text: &str) -> Value {
    Value::new(text, &Pattern::count_ask(&document.design())).unwrap()
}

fn spacing(document: &Document, text: &str) -> Value {
    Value::new(text, &Pattern::spacing_ask(&document.design())).unwrap()
}

fn angle(document: &Document, text: &str) -> Value {
    Value::new(text, &Pattern::angle_ask(&document.design())).unwrap()
}

/// A linear pattern of `bodies` along `axis`.
fn linear(document: &Document, bodies: &[BodyId], n: &str, step: &str) -> Pattern {
    Pattern {
        bodies: bodies.to_vec(),
        kind: PatternKind::Linear {
            along: AxisRef::Origin(Axis3::X),
            count: count(document, n),
            spacing: spacing(document, step),
        },
    }
}

/// A circular pattern of `bodies` about Z.
fn circular(document: &Document, bodies: &[BodyId], n: &str, span: &str) -> Pattern {
    Pattern {
        bodies: bodies.to_vec(),
        kind: PatternKind::Circular {
            about: AxisRef::Origin(Axis3::Z),
            count: count(document, n),
            angle: angle(document, span),
        },
    }
}

/// Adds `kind` as one edit: its id.
fn add(editor: &mut Editor, kind: impl Into<FeatureKind>) -> Result<FeatureId, EditError> {
    editor.apply(editor.document().add_feature(kind.into()))?;
    Ok(editor.document().features.last().unwrap().id)
}

#[test]
fn adding_a_pattern_makes_no_body() {
    let (mut editor, [a, b], _) = two_bodies();
    let before = editor.document().clone();
    let pattern = linear(editor.document(), &[a, b], "4", "-25");
    let id = add(&mut editor, pattern.clone()).unwrap();
    let document = editor.document();
    assert_eq!(document.bodies, before.bodies, "no body added");
    let feature = document.feature(id).unwrap();
    assert_eq!(feature.name, "Pattern 1");
    assert_eq!(feature.kind, FeatureKind::Pattern(pattern.clone()));
    assert_eq!(feature.kind.noun(), "Pattern");
    assert_eq!(feature.kind.bodies(), vec![a, b]);
    assert_eq!(feature.kind.uses(), Vec::new());
    assert_eq!(feature.kind.sketch(), None);
    assert_eq!(feature.kind.new_body(), None);
    assert_eq!(pattern.count(), Some(4));
    assert_eq!(pattern.span_steps(), None);
    assert!(!pattern.full_turn());
    let other = add(&mut editor, circular(&before, &[b], "6", "360")).unwrap();
    assert_eq!(editor.document().feature(other).unwrap().name, "Pattern 2");
    editor.undo();
    editor.undo();
    assert_eq!(*editor.document(), before);
}

/// A whole turn shares its ends; a span short of one has a copy at each
/// end.
#[test]
fn circular_spacing() {
    let document = with_body();
    let a = document.bodies[0].id;
    let full = circular(&document, &[a], "4", "360");
    assert!(full.full_turn());
    assert_eq!(full.span_steps(), Some((360.0, 4)));
    assert_eq!(full.step_degrees(), Some(90.0));
    // Two halves typed are a whole turn too.
    let halves = circular(&document, &[a], "3", "180 + 180");
    assert!(halves.full_turn());
    assert_eq!(halves.step_degrees(), Some(120.0));
    let part = circular(&document, &[a], "3", "90");
    assert!(!part.full_turn());
    assert_eq!(part.span_steps(), Some((90.0, 2)));
    assert_eq!(part.step_degrees(), Some(45.0));
    let two = circular(&document, &[a], "2", "359");
    assert_eq!(two.step_degrees(), Some(359.0));
}

#[test]
fn counts_spacings_and_angles_are_checked() {
    let document = with_body();
    let design = document.design();
    let a = document.bodies[0].id;
    assert_eq!(linear(&document, &[a], "2", "1").check_own(&design), Ok(()));
    assert_eq!(
        linear(&document, &[a], "1024", "-1e6").check_own(&design),
        Ok(())
    );
    // Counts: whole numbers from 2 to the limit.
    let ask = Pattern::count_ask(&design);
    for text in ["1", "0", "-3", "1025", "2.5", "3 mm", "90 deg"] {
        assert!(Value::new(text, &ask).is_err(), "{text}");
    }
    assert_eq!(Value::new("2 * 3", &ask).unwrap().value, 6.0);
    let tamper = |pattern: &mut Pattern, value: Value| match &mut pattern.kind {
        PatternKind::Linear { count, .. } | PatternKind::Circular { count, .. } => *count = value,
    };
    let mut wrong = linear(&document, &[a], "3", "10");
    for (text, value) in [
        ("3", 4.0),
        ("1", 1.0),
        ("1e9", 1e9),
        ("nan", f64::NAN),
        ("2.5", 2.5),
    ] {
        let alone = value != 4.0;
        let value = Value {
            text: text.into(),
            value,
        };
        tamper(&mut wrong, value);
        assert_eq!(wrong.check_own(&design), Err(MotionError::Count), "{text}");
        // The count read alone goes by the value.
        assert_eq!(wrong.count().is_none(), alone, "{text}");
    }
    // Spacings: lengths within the limit, not zero.
    let mut wrong = linear(&document, &[a], "3", "0");
    assert_eq!(wrong.check_own(&design), Err(MotionError::Spacing));
    if let PatternKind::Linear { spacing, .. } = &mut wrong.kind {
        *spacing = Value {
            text: "2e6".into(),
            value: 2e6,
        };
    }
    assert_eq!(wrong.check_own(&design), Err(MotionError::Spacing));
    assert!(Value::new("30 deg", &Pattern::spacing_ask(&design)).is_err());
    // Angles: above zero, at most a turn.
    let ask = Pattern::angle_ask(&design);
    for text in ["0", "-90", "361", "5 mm"] {
        assert!(Value::new(text, &ask).is_err(), "{text}");
    }
    let mut wrong = circular(&document, &[a], "3", "90");
    if let PatternKind::Circular { angle, .. } = &mut wrong.kind {
        *angle = Value {
            text: "720".into(),
            value: 720f64.to_radians(),
        };
    }
    assert_eq!(wrong.check_own(&design), Err(MotionError::Angle));
    // Bodies as a move's.
    assert_eq!(
        linear(&document, &[], "2", "1").check_own(&design),
        Err(MotionError::Bodies(0))
    );
    assert_eq!(
        linear(&document, &[a, a], "2", "1").check_own(&design),
        Err(MotionError::BodyOrder)
    );
    let many: Vec<BodyId> = (0..=MAX_FEATURE_BODIES as u64).map(BodyId).collect();
    assert_eq!(
        linear(&document, &many, "2", "1").check_own(&design),
        Err(MotionError::Bodies(MAX_FEATURE_BODIES + 1))
    );
    // An axis edge's own parts.
    let key = FaceKey {
        feature: document.features[1].id.get(),
        part: PartKey::EndCap,
        instance: 0,
    };
    let mut edged = linear(&document, &[a], "2", "1");
    *edged.kind.axis_mut() = AxisRef::Edge(EdgeRef {
        body: a,
        faces: [key, key],
        near: DVec3::ZERO,
    });
    assert_eq!(
        edged.check_own(&design),
        Err(MotionError::Edge(EdgeError::Faces))
    );
}

#[test]
fn bodies_and_axes_name_what_comes_before() {
    let (mut editor, [a, b], [first, second]) = two_bodies();
    let document = editor.document().clone();
    let next = FeatureId(document.next_id);
    let missing = BodyId(document.next_id + 3);
    assert_eq!(
        add(&mut editor, linear(&document, &[a, missing], "2", "5")).unwrap_err(),
        EditError::Invalid(CheckError::Pattern(next, MotionError::Body(missing)))
    );
    // An axis face made later is refused.
    let later = FeatureId(document.next_id + 1);
    let mut wrong = circular(&document, &[a], "3", "360");
    *wrong.kind.axis_mut() = AxisRef::Face(FaceRef {
        body: b,
        key: FaceKey {
            feature: later.get(),
            part: PartKey::EndCap,
            instance: 0,
        },
        near: DVec3::ZERO,
    });
    assert_eq!(
        add(&mut editor, wrong).unwrap_err(),
        EditError::Invalid(CheckError::Pattern(next, MotionError::RefMaker(later)))
    );
    // A pattern set in place of the first extrude can't copy a body
    // made after it.
    let error = editor
        .apply(Command::SetFeature {
            feature: first,
            kind: Box::new(linear(&document, &[b], "2", "5").into()),
        })
        .unwrap_err();
    assert!(
        matches!(error, EditError::Invalid(CheckError::Pattern(_, _))),
        "{error:?}"
    );
    assert_eq!(*editor.document(), document);
    // Removing a patterned body's maker removes the pattern.
    let id = add(&mut editor, linear(&document, &[b], "2", "5")).unwrap();
    let removal = editor.document().removal(Removable::Body(b));
    assert_eq!(removal.features, vec![second, id]);
}

#[test]
fn set_units_pins_the_spacing_and_keeps_the_count() {
    let (mut editor, [a, _], _) = two_bodies();
    let pattern = linear(editor.document(), &[a], "2 + 3", "12.5");
    let id = add(&mut editor, pattern.clone()).unwrap();
    let turning = circular(editor.document(), &[a], "6", "90");
    let other = add(&mut editor, turning.clone()).unwrap();
    editor.apply(Command::SetUnits(LengthUnit::In)).unwrap();
    let document = editor.document();
    let FeatureKind::Pattern(pinned) = &document.feature(id).unwrap().kind else {
        panic!("a pattern");
    };
    let PatternKind::Linear { count, spacing, .. } = &pinned.kind else {
        panic!("linear");
    };
    assert_eq!(count.text, "2 + 3");
    assert_eq!(count.value, 5.0);
    assert_eq!(spacing.text, "12.5 mm");
    assert_eq!(spacing.value, 12.5);
    assert_eq!(
        document.feature(other).unwrap().kind,
        FeatureKind::Pattern(turning)
    );
    document.check().unwrap();
}

#[test]
fn patterns_round_trip_and_wrong_ones_are_refused_when_read() {
    let (mut editor, [a, b], [first, _]) = two_bodies();
    let pattern = linear(editor.document(), &[a, b], "3", "-7");
    let id = add(&mut editor, pattern).unwrap();
    let mut turning = circular(editor.document(), &[b], "5", "120");
    *turning.kind.axis_mut() = AxisRef::Face(FaceRef {
        body: a,
        key: FaceKey {
            feature: first.get(),
            part: PartKey::EndCap,
            instance: 0,
        }
        .copy(id.get(), 2),
        near: DVec3::new(1.0, 2.0, 10.0),
    });
    let other = add(&mut editor, turning).unwrap();
    let document = editor.document().clone();
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document.clone())
    );
    let read = |feature: FeatureId, change: &dyn Fn(&mut Pattern)| {
        let mut document = document.clone();
        let index = document.feature_index(feature).unwrap();
        let FeatureKind::Pattern(pattern) = &mut document.features[index].kind else {
            unreachable!()
        };
        change(pattern);
        Document::from_postcard(&document.to_postcard()).map_err(|e| e.to_string())
    };
    let n = id.get();
    let count_wrong = format!("feature {n}: its count isn't a whole number from 2 to 1024");
    let set_count = |value: f64, text: &'static str| {
        move |pattern: &mut Pattern| {
            if let PatternKind::Linear { count, .. } = &mut pattern.kind {
                *count = Value {
                    text: text.into(),
                    value,
                };
            }
        }
    };
    assert_eq!(read(id, &set_count(1e9, "1e9")), Err(count_wrong.clone()));
    assert_eq!(read(id, &set_count(3.0, "1e9")), Err(count_wrong.clone()));
    assert_eq!(
        read(id, &set_count(f64::INFINITY, "inf")),
        Err(count_wrong.clone())
    );
    assert_eq!(read(id, &set_count(4.0, "3")), Err(count_wrong));
    assert_eq!(
        read(id, &|pattern| {
            if let PatternKind::Linear { spacing, .. } = &mut pattern.kind {
                spacing.value = f64::NAN;
            }
        }),
        Err(format!(
            "feature {n}: its spacing's expression doesn't give its value, or it's zero"
        ))
    );
    let o = other.get();
    assert_eq!(
        read(other, &|pattern| {
            if let PatternKind::Circular { angle, .. } = &mut pattern.kind {
                *angle = Value {
                    text: "0".into(),
                    value: 0.0,
                };
            }
        }),
        Err(format!(
            "feature {o}: its angle's expression doesn't give its value"
        ))
    );
    assert_eq!(
        read(other, &|pattern| pattern.bodies = vec![b, a]),
        Err(format!(
            "feature {o}: its bodies are out of order or repeated"
        ))
    );
    assert_eq!(
        read(other, &|pattern| {
            if let AxisRef::Face(face) = pattern.kind.axis_mut() {
                face.near.z = f64::INFINITY;
            }
        }),
        Err(format!(
            "feature {o}: its face's point [1, 2, inf] is out of bounds"
        ))
    );
}

/// Kinds are stored by name in files, but the workers' postcard keeps
/// the variants' order: the pattern comes after the mirror, and its
/// kinds in the order they're listed.
#[test]
fn a_pattern_is_the_seventh_kind() {
    let value = |text: &str, value: f64| Value {
        text: text.into(),
        value,
    };
    let pattern = FeatureKind::Pattern(Pattern {
        bodies: vec![BodyId(1)],
        kind: PatternKind::Circular {
            about: AxisRef::Origin(Axis3::Z),
            count: value("2", 2.0),
            angle: value("1", 1.0),
        },
    });
    let bytes = postcard::to_stdvec(&pattern).unwrap();
    // The kind, one body, then circular about the origin axis Z, and the
    // count's text.
    assert_eq!(bytes[..8], [6, 1, 1, 1, 0, 2, 1, b'2']);
}
