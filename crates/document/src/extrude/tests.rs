use glam::DVec2;
use varde_expr::Value;
use varde_sketch::RegionRefError;

use super::*;
use crate::testing::{extrude_again, extrude_of, plate, with_body};
use crate::{
    Command, Document, EditError, Editor, FeatureKind, LengthUnit, OriginPlane, Plane, Revision,
    Tolerance,
};

fn distance(text: &str) -> Value {
    Value::new(text, &Extent::ask(&Document::default().design())).unwrap()
}

/// `document` with its extrude `feature` changed by `change`.
fn changed(document: &Document, feature: FeatureId, change: impl FnOnce(&mut Extrude)) -> Document {
    let mut document = document.clone();
    let index = document.feature_index(feature).unwrap();
    let FeatureKind::Extrude(extrude) = &mut document.features[index].kind else {
        panic!("not an extrude");
    };
    change(extrude);
    document
}

/// Whether `document` fails its check with `why` for extrude `feature`,
/// and so isn't read from a file either.
fn refused(document: &Document, feature: FeatureId, why: ExtrudeError) {
    assert_eq!(
        document.check(),
        Err(crate::CheckError::Extrude(feature, why))
    );
    assert!(Document::from_postcard(&document.to_postcard()).is_err());
}

#[test]
fn the_example_is_a_plate_extruded_from_its_sketch() {
    let document = Document::example();
    assert_eq!(document.check(), Ok(()));
    let [sketch, extrude] = &document.features[..] else {
        panic!("a sketch and an extrude");
    };
    assert_eq!(
        (sketch.name.as_str(), extrude.name.as_str()),
        ("Sketch 1", "Extrude 1")
    );
    // Adding the extrude hid its sketch.
    assert!(!sketch.visible && extrude.visible);
    let [body] = &document.bodies[..] else {
        panic!("one body");
    };
    assert_eq!(
        (body.name.as_str(), body.created_by),
        ("Body 1", extrude.id)
    );
    let FeatureKind::Sketch { sketch: drawn, .. } = &sketch.kind else {
        panic!("a sketch first");
    };
    let made = extrude_of(&document, extrude.id);
    assert_eq!(made.operation, Operation::NewBody(body.id));
    assert_eq!(made.span(), Some((0.0, 10.0)));
    // Its region is the rectangle less the hole.
    let profiles = drawn.profiles().unwrap();
    let [Some(region)] = profiles.resolve(&made.regions)[..] else {
        panic!("the region is found");
    };
    assert_eq!(profiles.regions[region].holes.len(), 1);
    assert_eq!(profiles.regions[region].outer.len(), 4);
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document)
    );
}

#[test]
fn spans_follow_the_extent_and_flip() {
    let [a, b] = [distance("4"), distance("6")];
    let span = |extent: Extent, flip: bool| {
        Extrude {
            extent,
            flip,
            ..plate(Operation::NewBody(BodyId::NEW))
        }
        .span()
    };
    assert_eq!(span(Extent::OneSide(a.clone()), false), Some((0.0, 4.0)));
    assert_eq!(span(Extent::OneSide(a.clone()), true), Some((-4.0, 0.0)));
    assert_eq!(span(Extent::Symmetric(b.clone()), false), Some((-3.0, 3.0)));
    assert_eq!(span(Extent::Symmetric(b.clone()), true), Some((-3.0, 3.0)));
    let two = Extent::TwoSides(a, b);
    assert_eq!(span(two.clone(), false), Some((-6.0, 4.0)));
    assert_eq!(span(two, true), Some((-4.0, 6.0)));
    assert_eq!(span(Extent::ThroughAll, false), None);
}

#[test]
fn adding_an_extrude_hides_its_sketch_and_adds_its_body_in_one_step() {
    let mut editor = Editor::new(with_body());
    let before = editor.document().clone();
    let sketch = before.features[0].id;
    editor
        .apply(Command::SetFeatureVisible(sketch, true))
        .unwrap();
    let shown = editor.document().clone();
    let (feature, body) = extrude_again(&mut editor);
    let document = editor.document();
    assert_eq!(document.feature(feature).unwrap().name, "Extrude 2");
    assert!(!document.feature(sketch).unwrap().visible);
    assert_eq!(document.body(body).unwrap().name, "Body 2");
    // The body's id is the one after the extrude's, whatever the command
    // held.
    assert_eq!(body.0, feature.0 + 1);
    assert_eq!(document.next_id, body.0 + 1);
    assert_eq!(
        extrude_of(document, feature).operation,
        Operation::NewBody(body)
    );
    editor.undo();
    assert_eq!(*editor.document(), shown);

    // A cut adds no body.
    let cut = plate(Operation::Cut(Targets::default()));
    editor
        .apply(editor.document().add_feature(cut.into()))
        .unwrap();
    assert_eq!(editor.document().bodies, before.bodies);
    assert_eq!(editor.document().features.len(), 3);

    // Out of ids for the body, nothing is added.
    let mut editor = Editor::new(with_body());
    let mut full = editor.document().clone();
    full.next_id = u64::MAX - 1;
    editor
        .apply(Command::Replace(Box::new(full.clone())))
        .unwrap();
    let extrude = plate(Operation::NewBody(BodyId::NEW));
    assert_eq!(
        editor.apply(editor.document().add_feature(extrude.into())),
        Err(EditError::OutOfIds)
    );
    assert_eq!(*editor.document(), full);
}

#[test]
fn an_extrude_must_use_an_earlier_sketch() {
    let mut editor = Editor::new(with_body());
    let (second, _) = extrude_again(&mut editor);
    let document = editor.document().clone();
    let first = document.features[1].id;
    // Another extrude, a later feature, a missing one.
    for sketch in [first, second, FeatureId(document.next_id)] {
        let wrong = changed(&document, first, |extrude| extrude.sketch = sketch);
        refused(&wrong, first, ExtrudeError::Sketch(sketch));
    }
    // A sketch added after the extrude.
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XZ)))
        .unwrap();
    let later = editor.document().features.last().unwrap().id;
    let wrong = changed(editor.document(), first, |extrude| extrude.sketch = later);
    refused(&wrong, first, ExtrudeError::Sketch(later));
}

#[test]
fn regions_are_bounded_and_checked() {
    let document = with_body();
    let id = document.features[1].id;
    let region = extrude_of(&document, id).regions[0].clone();

    let none = changed(&document, id, |extrude| extrude.regions.clear());
    refused(&none, id, ExtrudeError::Regions(0));
    let most = changed(&document, id, |extrude| {
        extrude.regions = vec![region.clone(); MAX_EXTRUDE_REGIONS];
    });
    assert_eq!(most.check(), Ok(()));
    let too_many = changed(&document, id, |extrude| {
        extrude.regions = vec![region.clone(); MAX_EXTRUDE_REGIONS + 1];
    });
    refused(
        &too_many,
        id,
        ExtrudeError::Regions(MAX_EXTRUDE_REGIONS + 1),
    );

    let far = f64::from(MAX_COORD).next_up();
    for inside in [DVec2::new(far, 0.0), DVec2::new(0.0, f64::NAN)] {
        let outside = changed(&document, id, |extrude| extrude.regions[0].inside = inside);
        assert!(matches!(
            outside.check(),
            Err(crate::CheckError::Extrude(
                feature,
                ExtrudeError::Region(RegionRefError::Inside(_))
            )) if feature == id
        ));
    }
    let unsorted = changed(&document, id, |extrude| extrude.regions[0].curves.reverse());
    refused(
        &unsorted,
        id,
        ExtrudeError::Region(RegionRefError::Unsorted),
    );
}

#[test]
fn distances_are_checked_in_the_design_s_units() {
    let document = with_body();
    let id = document.features[1].id;
    let with = |extent: Extent| changed(&document, id, |extrude| extrude.extent = extent);

    // A value its text doesn't give, or a length out of bounds.
    let mut tampered = distance("10");
    tampered.value = 11.0;
    refused(&with(Extent::OneSide(tampered)), id, ExtrudeError::Distance);
    for text in ["0", "-5", "0.0005 mm", "1001 m", "2 * 3 mm * 1 mm"] {
        let value = Value {
            text: text.to_owned(),
            value: 1.0,
        };
        refused(&with(Extent::Symmetric(value)), id, ExtrudeError::Distance);
    }
    let most = distance("1000 m");
    assert_eq!(with(Extent::OneSide(most.clone())).check(), Ok(()));

    // Two sides together are at most the limit.
    let half = distance("500 m");
    let two = with(Extent::TwoSides(half.clone(), half.clone()));
    assert_eq!(two.check(), Ok(()));
    let over = Extent::TwoSides(half, distance("500 m + 0.001 mm"));
    refused(&with(over), id, ExtrudeError::Length);
    refused(&with(Extent::ThroughAll), id, ExtrudeError::ThroughAll);
}

#[test]
fn only_a_cut_goes_through_all() {
    let mut editor = Editor::new(with_body());
    let through = Extrude {
        extent: Extent::ThroughAll,
        ..plate(Operation::Cut(Targets::default()))
    };
    editor
        .apply(editor.document().add_feature(through.clone().into()))
        .unwrap();
    for operation in [
        Operation::Join(Targets::default()),
        Operation::Intersect(Targets::default()),
        Operation::NewBody(BodyId::NEW),
    ] {
        let extrude = Extrude {
            operation,
            ..through.clone()
        };
        assert!(matches!(
            editor.apply(editor.document().add_feature(extrude.into())),
            Err(EditError::Invalid(crate::CheckError::Extrude(
                _,
                ExtrudeError::ThroughAll
            )))
        ));
    }
    assert_eq!(editor.revision(), Revision::from(1));
}

#[test]
fn excluded_bodies_are_sorted_and_made_earlier() {
    let mut editor = Editor::new(with_body());
    let (_, second) = extrude_again(&mut editor);
    let first = editor.document().bodies[0].id;
    let cut = plate(Operation::Join(Targets {
        excluded: vec![first, second],
    }));
    editor
        .apply(editor.document().add_feature(cut.into()))
        .unwrap();
    let document = editor.document().clone();
    let join = document.features[3].id;
    let excluding = |excluded: Vec<BodyId>| {
        changed(&document, join, |extrude| {
            extrude.operation = Operation::Join(Targets { excluded });
        })
    };
    refused(
        &excluding(vec![second, first]),
        join,
        ExtrudeError::ExcludedOrder,
    );
    refused(
        &excluding(vec![first, first]),
        join,
        ExtrudeError::ExcludedOrder,
    );
    let missing = BodyId(document.next_id);
    refused(
        &excluding(vec![first, missing]),
        join,
        ExtrudeError::Excluded(missing),
    );

    // Made a cut, the second extrude may exclude the first body; the
    // first may not exclude the second's, made after it.
    let mut editor = Editor::new(document.clone());
    let cut = |excluded| {
        Box::new(Extrude {
            operation: Operation::Cut(Targets { excluded }),
            ..plate(Operation::NewBody(BodyId::NEW))
        })
    };
    let [first_extrude, second_extrude] = [1, 2].map(|index| document.features[index].id);
    assert!(matches!(
        editor.apply(Command::SetFeature { feature: first_extrude, kind: Box::new((*cut(vec![second])).into()) }),
        Err(EditError::Invalid(crate::CheckError::Extrude(
            feature,
            ExtrudeError::Excluded(body)
        ))) if feature == first_extrude && body == second
    ));
    editor
        .apply(Command::SetFeature {
            feature: second_extrude,
            kind: Box::new((*cut(vec![first])).into()),
        })
        .unwrap();
}

#[test]
fn set_extrude_keeps_adds_or_removes_its_body() {
    let mut editor = Editor::new(with_body());
    let (second, body) = extrude_again(&mut editor);
    let first_body = editor.document().bodies[0].id;
    // A join after both, excluding the second body.
    let join = plate(Operation::Join(Targets {
        excluded: vec![first_body, body],
    }));
    editor
        .apply(editor.document().add_feature(join.into()))
        .unwrap();
    let join = editor.document().features[3].id;
    let start = editor.document().clone();

    // Changing its distance keeps its body, whatever id the command held.
    let deeper = Extrude {
        extent: Extent::OneSide(distance("25")),
        ..plate(Operation::NewBody(BodyId::NEW))
    };
    editor
        .apply(Command::SetFeature {
            feature: second,
            kind: Box::new(deeper.clone().into()),
        })
        .unwrap();
    let document = editor.document();
    assert_eq!(document.bodies, start.bodies);
    assert_eq!(
        extrude_of(document, second).operation,
        Operation::NewBody(body)
    );
    assert_eq!(extrude_of(document, second).span(), Some((0.0, 25.0)));
    // The same again changes nothing.
    let revision = editor.revision();
    editor
        .apply(Command::SetFeature {
            feature: second,
            kind: Box::new(deeper.into()),
        })
        .unwrap();
    assert_eq!(editor.revision(), revision);

    // Made a cut, it loses its body, and the join no longer excludes it.
    let cut = plate(Operation::Cut(Targets::default()));
    editor
        .apply(Command::SetFeature {
            feature: second,
            kind: Box::new(cut.into()),
        })
        .unwrap();
    let document = editor.document();
    assert!(document.body(body).is_none());
    assert_eq!(
        extrude_of(document, join).operation.excluded(),
        [first_body]
    );

    // Made a new body again, it gets a new one.
    let next = BodyId(document.next_id);
    editor
        .apply(Command::SetFeature {
            feature: second,
            kind: Box::new(plate(Operation::NewBody(first_body)).into()),
        })
        .unwrap();
    let document = editor.document();
    assert_eq!(
        extrude_of(document, second).operation,
        Operation::NewBody(next)
    );
    let made = document.body(next).unwrap();
    assert_eq!((made.name.as_str(), made.created_by), ("Body 2", second));

    // Each one step to undo.
    for _ in 0..3 {
        editor.undo();
    }
    assert_eq!(*editor.document(), start);

    // A sketch isn't set this way, and a missing feature isn't set.
    let cut = || Box::new(plate(Operation::Cut(Targets::default())).into());
    let sketch = start.features[0].id;
    assert_eq!(
        editor.apply(Command::SetFeature {
            feature: sketch,
            kind: cut()
        }),
        Err(EditError::SketchKind)
    );
    let missing = FeatureId(start.next_id);
    editor
        .apply(Command::SetFeature {
            feature: missing,
            kind: cut(),
        })
        .unwrap();
    assert_eq!(*editor.document(), start);
}

#[test]
fn set_units_pins_the_distances() {
    let mut editor = Editor::new(with_body());
    let id = editor.document().features[1].id;
    let two = Extrude {
        extent: Extent::TwoSides(distance("4 + 1"), distance("1 in")),
        ..plate(Operation::NewBody(BodyId::NEW))
    };
    editor
        .apply(Command::SetFeature {
            feature: id,
            kind: Box::new(two.into()),
        })
        .unwrap();
    editor.apply(Command::SetUnits(LengthUnit::In)).unwrap();
    let Extent::TwoSides(a, b) = &extrude_of(editor.document(), id).extent else {
        panic!("two sides");
    };
    assert_eq!((a.text.as_str(), a.value), ("(4 + 1) mm", 5.0));
    assert_eq!((b.text.as_str(), b.value), ("1 in", 25.4));
    assert_eq!(editor.document().check(), Ok(()));
}

#[test]
fn the_tolerance_is_set_and_checked() {
    let mut editor = Editor::new(with_body());
    assert_eq!(editor.document().tolerance(), Tolerance::DEFAULT);
    let fine = Tolerance::new(1e-4).unwrap();
    editor.apply(Command::SetTolerance(fine)).unwrap();
    assert_eq!(editor.document().tolerance(), fine);
    assert_eq!(editor.revision(), Revision::from(1));
    // The same again changes nothing.
    editor.apply(Command::SetTolerance(fine)).unwrap();
    assert_eq!(editor.revision(), Revision::from(1));
    let document = editor.document().clone();
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document.clone())
    );
    editor.undo();
    assert_eq!(editor.document().tolerance(), Tolerance::DEFAULT);

    // A file's tolerance is checked.
    for fit in [0.0, 1e-6, 0.2, f64::NAN, -1e-3] {
        let mut wrong = document.clone();
        wrong.tolerance = fit;
        assert!(matches!(
            wrong.check(),
            Err(crate::CheckError::Tolerance(_))
        ));
        assert!(Document::from_postcard(&wrong.to_postcard()).is_err());
    }
}

#[test]
fn extrude_errors_say_what_is_wrong() {
    let document = with_body();
    let id = document.features[1].id;
    let none = changed(&document, id, |extrude| extrude.regions.clear());
    assert_eq!(
        none.check().unwrap_err().to_string(),
        format!("feature {}: extrudes 0 regions, not 1 to 256", id.0)
    );
}

/// The example's region is found again once its sketch is edited: the
/// hole made larger and a square added beside the plate.
#[test]
fn the_region_is_found_after_its_sketch_is_edited() {
    let mut editor = Editor::new(with_body());
    let document = editor.document().clone();
    let [sketch, extrude] = [0, 1].map(|index| document.features[index].id);
    let FeatureKind::Sketch { sketch: drawn, .. } = &document.features[0].kind else {
        panic!("a sketch first");
    };
    let mut edited = drawn.clone();
    for entry in &mut edited.curves {
        if let varde_sketch::Curve::Circle { radius, .. } = &mut entry.curve {
            *radius = 12.0;
        }
    }
    let corners = [(40.0, 0.0), (50.0, 0.0), (50.0, 10.0), (40.0, 10.0)]
        .map(|(x, y)| edited.add_point(DVec2::new(x, y)).unwrap());
    for (k, &start) in corners.iter().enumerate() {
        let end = corners[(k + 1) % 4];
        let line = varde_sketch::Curve::Line { start, end };
        edited.add_curve(line, false).unwrap();
    }
    let profiles = edited.profiles().unwrap();
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(edited),
        })
        .unwrap();
    let regions = &extrude_of(editor.document(), extrude).regions;
    let [Some(found)] = profiles.resolve(regions)[..] else {
        panic!("the plate is found");
    };
    let region = &profiles.regions[found];
    assert_eq!((region.outer.len(), region.holes.len()), (4, 1));
    let hole = 12.0 * 12.0 * std::f64::consts::PI;
    assert!((region.area - (60.0 * 40.0 - hole)).abs() < 1.0);
}

fn taper(text: &str) -> Value {
    Value::new(text, &Extrude::taper_ask(&Document::default().design())).unwrap()
}

/// A taper is an angle under 90° either way; none is none, and one of
/// zero (as a file may hold, though the panel stores none) is taken as
/// none.
#[test]
fn tapers_are_checked() {
    let document = with_body();
    let id = document.features[1].id;
    let with = |taper: Option<Value>| changed(&document, id, |extrude| extrude.taper = taper);
    for text in ["2", "-2", "89.999", "-89.999", "0.5 rad", "1e-300 rad"] {
        let tapered = with(Some(taper(text)));
        assert_eq!(tapered.check(), Ok(()), "{text}");
        assert!(extrude_of(&tapered, id).tapered().is_some(), "{text}");
        assert_eq!(
            Document::from_postcard(&tapered.to_postcard()),
            Ok(tapered.clone())
        );
    }
    assert_eq!(with(None).check(), Ok(()));
    // Zero, either sign, is untapered.
    let mut zero = taper("3 - 3");
    assert_eq!(zero.value, 0.0);
    for _ in 0..2 {
        let untapered = with(Some(zero.clone()));
        assert_eq!(untapered.check(), Ok(()));
        assert_eq!(extrude_of(&untapered, id).tapered(), None);
        zero = taper("-0");
        assert!(zero.value == 0.0 && zero.value.is_sign_negative());
    }
    // A right angle or more, a value its text doesn't give, a length,
    // not a number.
    for text in ["90", "-90", "100", "2 mm", "1/0", "nan", "", "5 deg deg"] {
        let value = Value {
            text: text.to_owned(),
            value: 0.0,
        };
        refused(&with(Some(value)), id, ExtrudeError::Taper);
    }
    for value in [f64::NAN, f64::INFINITY, 1.6, -1.6, FRAC_PI_2] {
        let value = Value {
            text: format!("{value:e} rad"),
            value,
        };
        refused(&with(Some(value)), id, ExtrudeError::Taper);
    }
    let mut tampered = taper("3");
    tampered.value = 0.1;
    refused(&with(Some(tampered)), id, ExtrudeError::Taper);
    let mut tampered = taper("3");
    tampered.value = 0.0;
    refused(&with(Some(tampered)), id, ExtrudeError::Taper);
    let wrong = with(Some(Value {
        text: "95 - 5".to_owned(),
        value: 90f64.to_radians(),
    }));
    assert_eq!(
        wrong.check().unwrap_err().to_string(),
        format!(
            "feature {}: its taper isn't an angle under 90° either way",
            id.0
        )
    );
}

/// Changing the units keeps a taper's angle as typed.
#[test]
fn set_units_pins_the_taper() {
    let mut editor = Editor::new(with_body());
    let id = editor.document().features[1].id;
    let tapered = Extrude {
        taper: Some(taper("-(1 + 2)")),
        ..plate(Operation::NewBody(BodyId::NEW))
    };
    editor
        .apply(Command::SetFeature {
            feature: id,
            kind: Box::new(tapered.into()),
        })
        .unwrap();
    editor.apply(Command::SetUnits(LengthUnit::In)).unwrap();
    let pinned = extrude_of(editor.document(), id).taper.clone().unwrap();
    assert_eq!(pinned.value, -(3f64.to_radians()));
    assert_eq!(editor.document().check(), Ok(()));
    editor.undo();
    assert_eq!(
        extrude_of(editor.document(), id).taper,
        Some(taper("-(1 + 2)"))
    );
}
