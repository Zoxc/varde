use std::f64::consts::{PI, TAU};

use varde_expr::Value;
use varde_sketch::{Curve, RegionRefError};

use glam::DVec3;
use varde_kernel::mesh::{FaceKey, PartKey};

use super::*;
use crate::testing::{extrude_of, plate, with_body};
use crate::{
    CheckError, Command, Document, EditError, Editor, Extent, FeatureKind, LengthUnit, Removable,
    Targets,
};

fn angle(text: &str) -> Value {
    Value::new(text, &Turn::ask(&Document::default().design())).unwrap()
}

/// The example's sketch and the ids of one of its lines and of its circle.
fn sketch_of(document: &Document) -> (FeatureId, &Sketch, Id, Id) {
    let feature = &document.features[0];
    let FeatureKind::Sketch { sketch, .. } = &feature.kind else {
        panic!("a sketch first");
    };
    let line = sketch
        .curves
        .iter()
        .find(|entry| matches!(entry.curve, Curve::Line { .. }))
        .unwrap()
        .id;
    let circle = sketch
        .curves
        .iter()
        .find(|entry| matches!(entry.curve, Curve::Circle { .. }))
        .unwrap()
        .id;
    (feature.id, sketch, line, circle)
}

/// A revolve of the example's plate region about its sketch's y axis, a
/// whole turn, doing `operation`.
fn ring(operation: Operation) -> Revolve {
    let example = Document::example();
    let extrude = extrude_of(&example, example.features[1].id);
    Revolve {
        sketch: extrude.sketch,
        regions: extrude.regions.clone(),
        axis: AxisLine::SketchY,
        extent: Turn::Full,
        flip: false,
        operation,
    }
}

/// Revolve `feature` of `document`.
fn revolve_of(document: &Document, feature: FeatureId) -> &Revolve {
    match &document.feature(feature).unwrap().kind {
        FeatureKind::Revolve(revolve) => revolve,
        _ => panic!("feature {} isn't a revolve", feature.0),
    }
}

/// The example with `revolve` added: the editor, the revolve's id.
fn added(revolve: Revolve) -> (Editor, FeatureId) {
    let mut editor = Editor::new(with_body());
    editor
        .apply(editor.document().add_feature(revolve.into()))
        .unwrap();
    let id = editor.document().features.last().unwrap().id;
    (editor, id)
}

/// `document` with its revolve `feature` changed by `change`.
fn changed(document: &Document, feature: FeatureId, change: impl FnOnce(&mut Revolve)) -> Document {
    let mut document = document.clone();
    let index = document.feature_index(feature).unwrap();
    let FeatureKind::Revolve(revolve) = &mut document.features[index].kind else {
        panic!("not a revolve");
    };
    change(revolve);
    document
}

/// Whether `document` fails its check with `why` for revolve `feature`,
/// and so isn't read from a file either.
fn refused(document: &Document, feature: FeatureId, why: RevolveError) {
    assert_eq!(document.check(), Err(CheckError::Revolve(feature, why)));
    assert!(Document::from_postcard(&document.to_postcard()).is_err());
}

#[test]
fn adding_a_revolve_hides_its_sketch_and_adds_its_body_in_one_step() {
    let mut editor = Editor::new(with_body());
    let sketch = editor.document().features[0].id;
    editor
        .apply(Command::SetFeatureVisible(sketch, true))
        .unwrap();
    let before = editor.document().clone();
    let command = before.add_feature(ring(Operation::NewBody(BodyId::NEW)).into());
    let Command::AddFeature { name, .. } = &command else {
        panic!("adds a feature");
    };
    assert_eq!(name, "Revolve 1");
    editor.apply(command).unwrap();
    let document = editor.document().clone();
    let feature = document.features.last().unwrap();
    assert_eq!(feature.name, "Revolve 1");
    assert!(feature.visible);
    assert!(!document.feature(sketch).unwrap().visible);
    let body = document.bodies.last().unwrap();
    assert_eq!(
        (body.name.as_str(), body.created_by),
        ("Body 2", feature.id)
    );
    assert_eq!(body.id.0, feature.id.0 + 1);
    assert_eq!(
        revolve_of(&document, feature.id).operation,
        Operation::NewBody(body.id)
    );
    assert_eq!(document.check(), Ok(()));

    // One step to undo and redo.
    editor.undo();
    assert_eq!(*editor.document(), before);
    editor.redo();
    assert_eq!(*editor.document(), document);

    // The next is "Revolve 2"; a cut adds no body.
    let cut = ring(Operation::Cut(Targets::default()));
    editor
        .apply(editor.document().add_feature(cut.into()))
        .unwrap();
    assert_eq!(editor.document().features.last().unwrap().name, "Revolve 2");
    assert_eq!(editor.document().bodies, document.bodies);

    // Sketches have their own command.
    let plane = crate::Plane::Origin(crate::OriginPlane::XY);
    let sketch = FeatureKind::Sketch {
        plane,
        sketch: Sketch::default(),
    };
    let revision = editor.revision();
    assert_eq!(
        editor.apply(editor.document().add_feature(sketch)),
        Err(EditError::SketchKind)
    );
    assert_eq!(editor.revision(), revision);
}

#[test]
fn spans_follow_the_turn_and_flip() {
    let span = |extent: Turn, flip: bool| {
        Revolve {
            extent,
            flip,
            ..ring(Operation::NewBody(BodyId::NEW))
        }
        .span()
    };
    let [a, b] = [angle("90"), angle("30")];
    let (quarter, sixth) = (PI / 2.0, PI / 6.0);
    assert_eq!(span(Turn::Full, false), None);
    assert_eq!(span(Turn::Full, true), None);
    assert_eq!(span(Turn::OneSide(a.clone()), false), Some((0.0, quarter)));
    assert_eq!(span(Turn::OneSide(a.clone()), true), Some((-quarter, 0.0)));
    let half = Some((-quarter / 2.0, quarter / 2.0));
    assert_eq!(span(Turn::Symmetric(a.clone()), false), half);
    assert_eq!(span(Turn::Symmetric(a.clone()), true), half);
    let two = Turn::TwoSides(a.clone(), b.clone());
    assert_eq!(span(two.clone(), false), Some((-sixth, quarter)));
    assert_eq!(span(two, true), Some((-quarter, sixth)));
    // What comes to a turn is a whole turn.
    let turn = angle("360");
    assert_eq!(turn.value, TAU);
    assert_eq!(span(Turn::OneSide(turn.clone()), true), None);
    assert_eq!(span(Turn::Symmetric(turn), false), None);
    let [c, d] = [angle("200"), angle("160")];
    assert_eq!(span(Turn::TwoSides(c, d), false), None);
}

/// Two sides typed in degrees that add up to 360 are a whole turn, though
/// in radians they come a rounding over (0.5 and 359.5) or under (1.1
/// and 358.9) it.
#[test]
fn two_sides_adding_up_to_a_turn_in_degrees_are_a_whole_turn() {
    let (editor, id) = added(ring(Operation::NewBody(BodyId::NEW)));
    let document = editor.document().clone();
    for (a, b) in [
        ("0.5", "359.5"),
        ("2.2", "357.8"),
        ("1.1", "358.9"),
        ("180", "180"),
    ] {
        let (a, b) = (angle(a), angle(b));
        let sum = a.value + b.value;
        let two = Turn::TwoSides(a, b);
        let set = changed(&document, id, |revolve| revolve.extent = two.clone());
        assert_eq!(set.check(), Ok(()), "{two:?} ({sum} vs {TAU})");
        for flip in [false, true] {
            let revolve = Revolve {
                extent: two.clone(),
                flip,
                ..ring(Operation::NewBody(BodyId::NEW))
            };
            assert_eq!(revolve.span(), None, "{two:?}");
        }
    }
    // Over or under by more than rounding isn't.
    let over = changed(&document, id, |revolve| {
        revolve.extent = Turn::TwoSides(angle("0.5"), angle("359.5000001"));
    });
    assert!(over.check().is_err());
    let under = Revolve {
        extent: Turn::TwoSides(angle("0.5"), angle("359.4999999")),
        ..ring(Operation::NewBody(BodyId::NEW))
    };
    assert!(under.span().is_some());
}

#[test]
fn angles_are_above_zero_and_at_most_a_turn() {
    let (editor, id) = added(ring(Operation::NewBody(BodyId::NEW)));
    let document = editor.document().clone();
    let with = |extent: Turn| changed(&document, id, |revolve| revolve.extent = extent);

    for good in ["1e-6", "45", "360", "720 / 2", "1 rad"] {
        let document = with(Turn::OneSide(angle(good)));
        assert_eq!(document.check(), Ok(()), "{good}");
        assert_eq!(
            Document::from_postcard(&document.to_postcard()),
            Ok(document)
        );
    }
    let raw = |text: &str, value: f64| Value {
        text: text.to_owned(),
        value,
    };
    for bad in [
        raw("0", 0.0),
        raw("-10", -10f64.to_radians()),
        raw("361", 361f64.to_radians()),
        raw("10 mm", 10.0),
        raw("90", 1.0),
        raw("90", f64::NAN),
        raw("90", f64::INFINITY),
        raw("(", 0.5),
    ] {
        for extent in [
            Turn::OneSide(bad.clone()),
            Turn::Symmetric(bad.clone()),
            Turn::TwoSides(angle("10"), bad.clone()),
            Turn::TwoSides(bad.clone(), angle("10")),
        ] {
            refused(&with(extent), id, RevolveError::Angle);
        }
    }

    // Two sides at most a turn together.
    let at_most = with(Turn::TwoSides(angle("180"), angle("180")));
    assert_eq!(at_most.check(), Ok(()));
    let over = with(Turn::TwoSides(angle("180"), angle("180.001")));
    refused(&over, id, RevolveError::Turn);
    let over = with(Turn::TwoSides(angle("360"), angle("360")));
    refused(&over, id, RevolveError::Turn);
}

#[test]
fn regions_are_bounded_and_checked() {
    let (editor, id) = added(ring(Operation::NewBody(BodyId::NEW)));
    let document = editor.document().clone();
    let none = changed(&document, id, |revolve| revolve.regions.clear());
    refused(&none, id, RevolveError::Regions(0));
    let many = changed(&document, id, |revolve| {
        let region = revolve.regions[0].clone();
        revolve.regions = vec![region; MAX_REVOLVE_REGIONS + 1];
    });
    refused(&many, id, RevolveError::Regions(MAX_REVOLVE_REGIONS + 1));
    let most = changed(&document, id, |revolve| {
        let region = revolve.regions[0].clone();
        revolve.regions = vec![region; MAX_REVOLVE_REGIONS];
    });
    assert_eq!(most.check(), Ok(()));
    let far = changed(&document, id, |revolve| {
        revolve.regions[0].inside = glam::DVec2::new(f64::from(MAX_COORD) * 2.0, 0.0);
    });
    assert!(matches!(
        far.check(),
        Err(CheckError::Revolve(feature, RevolveError::Region(RegionRefError::Inside(_))))
            if feature == id
    ));
}

#[test]
fn a_revolve_must_use_an_earlier_sketch_and_own_its_body() {
    let (editor, id) = added(ring(Operation::NewBody(BodyId::NEW)));
    let document = editor.document().clone();
    let extrude = document.features[1].id;
    // Its sketch an extrude.
    let wrong = changed(&document, id, |revolve| revolve.sketch = extrude);
    refused(&wrong, id, RevolveError::Sketch(extrude));
    // Its sketch after it.
    let later = FeatureId(document.next_id);
    let wrong = changed(&document, id, |revolve| revolve.sketch = later);
    refused(&wrong, id, RevolveError::Sketch(later));

    // Its body another's.
    let first = document.bodies[0].id;
    let mut wrong = changed(&document, id, |revolve| {
        revolve.operation = Operation::NewBody(first);
    });
    wrong.bodies.pop();
    assert!(matches!(
        wrong.check(),
        Err(CheckError::Revolve(feature, RevolveError::NewBody(body)))
            if feature == id && body == first
    ));
    // A body it no longer makes names it as its maker.
    let made = document.bodies[1].id;
    let wrong = changed(&document, id, |revolve| {
        revolve.operation = Operation::Join(Targets::default());
    });
    assert_eq!(wrong.check(), Err(CheckError::Creator(made, id)));

    // Excluded bodies sorted and made earlier.
    let join = |excluded: Vec<BodyId>| {
        changed(&document, id, |revolve| {
            revolve.operation = Operation::Join(Targets { excluded });
        })
    };
    let mut ok = join(vec![first]);
    ok.bodies.pop();
    assert_eq!(ok.check(), Ok(()));
    let mut own = join(vec![made]);
    own.bodies.pop();
    refused(&own, id, RevolveError::Excluded(made));
    let mut twice = join(vec![first, first]);
    twice.bodies.pop();
    refused(&twice, id, RevolveError::ExcludedOrder);
}

#[test]
fn the_axis_is_a_line_of_its_sketch_when_added_or_set() {
    let document = with_body();
    let (sketch, drawn, line, circle) = sketch_of(&document);
    let mut editor = Editor::new(document.clone());
    let point = drawn.points[0].id;
    // The sketch's own axes are `SketchX` and `SketchY`, not curves.
    for axis in [circle, point, Id::ORIGIN, Id::X_AXIS, Id::Y_AXIS] {
        let revolve = Revolve {
            axis: AxisLine::Curve(axis),
            ..ring(Operation::NewBody(BodyId::NEW))
        };
        let next = FeatureId(document.next_id);
        assert_eq!(
            editor.apply(editor.document().add_feature(revolve.into())),
            Err(EditError::Invalid(CheckError::Revolve(
                next,
                RevolveError::Axis(axis)
            )))
        );
        assert_eq!(*editor.document(), document);
    }
    for axis in [AxisLine::Curve(line), AxisLine::SketchX, AxisLine::SketchY] {
        let revolve = Revolve {
            axis,
            ..ring(Operation::Cut(Targets::default()))
        };
        editor
            .apply(editor.document().add_feature(revolve.into()))
            .unwrap();
    }
    let about_line = editor.document().features[2].id;

    // The sketch may lose the line afterwards: the document still checks,
    // and regenerating says the axis is gone.
    let mut edited = drawn.clone();
    edited.curves.retain(|entry| entry.id != line);
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(edited),
        })
        .unwrap();
    assert_eq!(
        revolve_of(editor.document(), about_line).axis,
        AxisLine::Curve(line)
    );
    let document = editor.document().clone();
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document.clone())
    );

    // Set again, it needs a line, but other edits of it are fine.
    let stale = Revolve {
        flip: true,
        ..revolve_of(&document, about_line).clone()
    };
    assert_eq!(
        editor.apply(Command::SetFeature {
            feature: about_line,
            kind: Box::new(stale.clone().into()),
        }),
        Err(EditError::Invalid(CheckError::Revolve(
            about_line,
            RevolveError::Axis(line)
        )))
    );
    assert_eq!(*editor.document(), document);
    let fixed = Revolve {
        axis: AxisLine::SketchX,
        ..stale
    };
    editor
        .apply(Command::SetFeature {
            feature: about_line,
            kind: Box::new(fixed.clone().into()),
        })
        .unwrap();
    assert_eq!(*revolve_of(editor.document(), about_line), fixed);
}

#[test]
fn set_feature_keeps_adds_or_removes_a_revolve_s_body() {
    let (mut editor, id) = added(ring(Operation::NewBody(BodyId::NEW)));
    let start = editor.document().clone();
    let body = start.bodies[1].id;
    let first = start.bodies[0].id;

    // A part turn keeps the body, whatever the command held.
    let part = Revolve {
        extent: Turn::OneSide(angle("90")),
        ..ring(Operation::NewBody(first))
    };
    editor
        .apply(Command::SetFeature {
            feature: id,
            kind: Box::new(part.clone().into()),
        })
        .unwrap();
    let document = editor.document().clone();
    assert_eq!(document.bodies, start.bodies);
    let set = revolve_of(&document, id);
    assert_eq!(set.operation, Operation::NewBody(body));
    assert_eq!(set.extent, part.extent);
    let revision = editor.revision();
    editor
        .apply(Command::SetFeature {
            feature: id,
            kind: Box::new(part.into()),
        })
        .unwrap();
    assert_eq!(editor.revision(), revision);

    // A cut loses it; undo brings it back.
    let cut = ring(Operation::Cut(Targets::default()));
    editor
        .apply(Command::SetFeature {
            feature: id,
            kind: Box::new(cut.into()),
        })
        .unwrap();
    assert!(editor.document().body(body).is_none());
    editor.undo();
    assert_eq!(*editor.document(), document);
    editor.redo();
    assert!(editor.document().body(body).is_none());

    // A new body again gets a new id.
    let next = BodyId(editor.document().next_id);
    editor
        .apply(Command::SetFeature {
            feature: id,
            kind: Box::new(ring(Operation::NewBody(BodyId::NEW)).into()),
        })
        .unwrap();
    assert_eq!(
        revolve_of(editor.document(), id).operation,
        Operation::NewBody(next)
    );
    assert_eq!(editor.document().body(next).unwrap().created_by, id);

    // An extrude may become a revolve and keep its body.
    let extrude = start.features[1].id;
    editor
        .apply(Command::SetFeature {
            feature: extrude,
            kind: Box::new(ring(Operation::NewBody(BodyId::NEW)).into()),
        })
        .unwrap();
    assert_eq!(
        revolve_of(editor.document(), extrude).operation,
        Operation::NewBody(first)
    );
    assert_eq!(editor.document().check(), Ok(()));
}

#[test]
fn removing_the_sketch_removes_the_revolve_and_its_body() {
    let (mut editor, id) = added(ring(Operation::NewBody(BodyId::NEW)));
    let document = editor.document().clone();
    let sketch = document.features[0].id;
    assert_eq!(
        FeatureKind::Revolve(revolve_of(&document, id).clone()).uses(),
        [sketch]
    );
    let removal = document.removal(Removable::Feature(sketch));
    assert_eq!(removal.features, [sketch, document.features[1].id, id]);
    assert_eq!(
        removal.bodies,
        document
            .bodies
            .iter()
            .map(|body| body.id)
            .collect::<Vec<_>>()
    );
    // Removing the revolve's body takes only the revolve.
    let body = document.bodies[1].id;
    let removal = document.removal(Removable::Body(body));
    assert_eq!(
        (&removal.features[..], &removal.bodies[..]),
        (&[id][..], &[body][..])
    );

    // A revolve excluding the extrude's body lets it go.
    let first = document.bodies[0].id;
    let join = ring(Operation::Join(Targets {
        excluded: vec![first],
    }));
    editor
        .apply(editor.document().add_feature(join.into()))
        .unwrap();
    let join = editor.document().features.last().unwrap().id;
    editor.apply(Command::RemoveBody(first)).unwrap();
    assert_eq!(
        revolve_of(editor.document(), join).operation,
        Operation::Join(Targets::default())
    );
    editor.apply(Command::RemoveFeature(sketch)).unwrap();
    assert!(editor.document().features.is_empty());
    assert!(editor.document().bodies.is_empty());
}

#[test]
fn set_units_pins_the_angles() {
    let revolve = Revolve {
        extent: Turn::TwoSides(angle("45 + 0"), angle("1 rad")),
        ..ring(Operation::NewBody(BodyId::NEW))
    };
    let (mut editor, id) = added(revolve);
    editor.apply(Command::SetUnits(LengthUnit::In)).unwrap();
    let Turn::TwoSides(a, b) = &revolve_of(editor.document(), id).extent else {
        panic!("two sides");
    };
    assert_eq!(a.value, 45f64.to_radians());
    assert_eq!(b.value, 1.0);
    assert_eq!(editor.document().check(), Ok(()));
    let document = editor.document().clone();
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document)
    );
}

#[test]
fn revolves_round_trip() {
    let (mut editor, _) = added(ring(Operation::NewBody(BodyId::NEW)));
    let (_, _, line, _) = sketch_of(editor.document());
    let first = editor.document().bodies[0].id;
    let extents = [
        Turn::OneSide(angle("30")),
        Turn::Symmetric(angle("120")),
        Turn::TwoSides(angle("10"), angle("1 rad")),
    ];
    let axes = [AxisLine::Curve(line), AxisLine::SketchX, AxisLine::SketchY];
    let operations = [
        Operation::Join(Targets::default()),
        Operation::Cut(Targets {
            excluded: vec![first],
        }),
        Operation::Intersect(Targets::default()),
    ];
    for ((extent, axis), operation) in extents.into_iter().zip(axes).zip(operations) {
        let revolve = Revolve {
            extent,
            axis,
            flip: true,
            operation,
            ..ring(Operation::NewBody(BodyId::NEW))
        };
        editor
            .apply(editor.document().add_feature(revolve.into()))
            .unwrap();
    }
    let document = editor.document().clone();
    assert_eq!(document.features.len(), 6);
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document)
    );
}

/// A file whose revolve holds an axis, turn or flip of no known value is
/// refused, as one with a kind past the revolve.
#[test]
fn unknown_revolve_bytes_are_refused() {
    let (editor, id) = added(ring(Operation::NewBody(BodyId::NEW)));
    let document = editor.document();
    let bytes = document.to_postcard();
    let kind = &document.feature(id).unwrap().kind;
    let FeatureKind::Revolve(revolve) = kind else {
        unreachable!()
    };
    // Where the revolve's bytes are in the file's, and its axis, turn
    // (a whole turn, one byte) and flip in them.
    let encoded = postcard::to_stdvec(kind).unwrap();
    let at = (0..bytes.len() - encoded.len())
        .find(|&k| bytes[k..].starts_with(&encoded))
        .unwrap();
    let sketch = postcard::to_stdvec(&revolve.sketch).unwrap().len();
    let regions = postcard::to_stdvec(&revolve.regions).unwrap().len();
    let axis = at + 1 + sketch + regions;
    assert_eq!(bytes[axis], 2, "the sketch's y axis");
    assert_eq!(bytes[axis + 1], 0, "a whole turn");
    assert_eq!(bytes[axis + 2], 0, "not flipped");
    for (offset, byte) in [(0, 4), (1, 4), (2, 2)] {
        let mut bad = bytes.clone();
        bad[axis + offset] = byte;
        assert!(Document::from_postcard(&bad).is_err(), "{offset}: {byte}");
    }
    let mut bad = bytes.clone();
    bad[at] = 3;
    assert!(Document::from_postcard(&bad).is_err());
}

/// Kinds are appended, so the kinds before keep their bytes, and so the
/// files holding them.
#[test]
fn a_revolve_is_the_third_kind() {
    let revolve = FeatureKind::Revolve(ring(Operation::NewBody(BodyId::NEW)));
    assert_eq!(postcard::to_stdvec(&revolve).unwrap()[0], 2);
    let extrude = FeatureKind::Extrude(plate(Operation::NewBody(BodyId::NEW)));
    assert_eq!(postcard::to_stdvec(&extrude).unwrap()[0], 1);
    let axes = [AxisLine::SketchX, AxisLine::SketchY];
    assert_eq!(
        axes.map(|axis| postcard::to_stdvec(&axis).unwrap()),
        [vec![1], vec![2]]
    );
    // A model edge is appended after them.
    let edge = AxisLine::Edge(plate_edge(&with_body()));
    assert_eq!(postcard::to_stdvec(&edge).unwrap()[0], 3);
    assert_eq!(postcard::to_stdvec(&Turn::Full).unwrap(), [0]);
    let _ = Extent::ThroughAll;
}

#[test]
fn revolve_errors_say_what_is_wrong() {
    let (editor, id) = added(ring(Operation::NewBody(BodyId::NEW)));
    let document = editor.document().clone();
    let none = changed(&document, id, |revolve| revolve.regions.clear());
    assert_eq!(
        none.check().unwrap_err().to_string(),
        format!("feature {}: revolves 0 regions, not 1 to 256", id.0)
    );
    let over = changed(&document, id, |revolve| {
        revolve.extent = Turn::TwoSides(angle("300"), angle("100"));
    });
    assert_eq!(
        over.check().unwrap_err().to_string(),
        format!("feature {}: its two sides come to over a turn", id.0)
    );
    let (_, _, _, circle) = sketch_of(&document);
    assert_eq!(
        RevolveError::Axis(circle).to_string(),
        format!("its axis, curve {circle}, isn't a line of its sketch")
    );
}

/// The example plate's edge between its top (its extrude's end cap) and
/// the side its sketch's first line makes, picked at its middle.
fn plate_edge(document: &Document) -> EdgeRef {
    let maker = document.features[1].id.get();
    let (_, drawn, _, _) = sketch_of(document);
    let top = FaceKey {
        feature: maker,
        part: PartKey::EndCap,
        instance: 0,
    };
    let side = FaceKey {
        feature: maker,
        part: PartKey::Side {
            curve: u64::from(drawn.curves[0].id.get()),
        },
        instance: 0,
    };
    EdgeRef {
        body: document.bodies[0].id,
        faces: [top.min(side), top.max(side)],
        near: DVec3::new(0.0, -20.0, 10.0),
    }
}

/// A revolve about a model edge names a body and faces' makers before
/// it, as a sketch on a face does: one that isn't there is allowed with
/// an id no later one can take. Its keys are sorted and different, its
/// point in bounds. What it names isn't removed with it, nor it with
/// them, and it round trips.
#[test]
fn an_axis_edge_names_only_what_comes_before_its_revolve() {
    let document = with_body();
    let edge = plate_edge(&document);
    let about = |edge: EdgeRef| Revolve {
        axis: AxisLine::Edge(edge),
        ..ring(Operation::NewBody(BodyId::NEW))
    };
    let (mut editor, id) = added(about(edge));
    let document = editor.document().clone();
    assert_eq!(revolve_of(&document, id).axis, AxisLine::Edge(edge));
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document.clone())
    );

    // Its own body and itself come after it; ids at or past the next a
    // later body or feature would take.
    let own = document.body(document.bodies[1].id).unwrap().id;
    let next = document.next_id;
    let [a, b] = edge.faces;
    let made_by = |feature: u64| {
        let key = FaceKey { feature, ..b };
        [a.min(key), a.max(key)]
    };
    let cases = [
        (EdgeRef { body: own, ..edge }, RevolveError::EdgeBody(own)),
        (
            EdgeRef {
                body: BodyId(next),
                ..edge
            },
            RevolveError::EdgeBody(BodyId(next)),
        ),
        (
            EdgeRef {
                faces: made_by(id.get()),
                ..edge
            },
            RevolveError::EdgeMaker(id),
        ),
        (
            EdgeRef {
                faces: made_by(next + 3),
                ..edge
            },
            RevolveError::EdgeMaker(FeatureId(next + 3)),
        ),
        (
            EdgeRef {
                faces: [b, a],
                ..edge
            },
            RevolveError::Edge(EdgeError::Faces),
        ),
        (
            EdgeRef {
                faces: [a, a],
                ..edge
            },
            RevolveError::Edge(EdgeError::Faces),
        ),
    ];
    for (bad, why) in cases {
        let changed = changed(&document, id, |revolve| revolve.axis = AxisLine::Edge(bad));
        refused(&changed, id, why);
        assert_eq!(
            editor.apply(Command::SetFeature {
                feature: id,
                kind: Box::new(about(bad).into()),
            }),
            Err(EditError::Invalid(CheckError::Revolve(id, why)))
        );
    }
    for near in [
        DVec3::new(f64::NAN, 0.0, 0.0),
        DVec3::new(0.0, f64::INFINITY, 0.0),
        DVec3::new(0.0, 0.0, f64::from(MAX_COORD) * 2.0),
    ] {
        let bad = EdgeRef { near, ..edge };
        let changed = changed(&document, id, |revolve| revolve.axis = AxisLine::Edge(bad));
        assert!(matches!(
            changed.check(),
            Err(CheckError::Revolve(
                _,
                RevolveError::Edge(EdgeError::Near(_))
            ))
        ));
        assert!(Document::from_postcard(&changed.to_postcard()).is_err());
    }

    // Removing the plate's extrude takes its body, not the revolve, which
    // still checks naming them, as its ids stay below the next.
    let plate_maker = document.features[1].id;
    editor.apply(Command::RemoveFeature(plate_maker)).unwrap();
    let removed = editor.document().clone();
    assert!(removed.body(edge.body).is_none());
    assert_eq!(revolve_of(&removed, id).axis, AxisLine::Edge(edge));
    assert_eq!(removed.check(), Ok(()));
    // An edge of a body or a feature that's gone, set again, is allowed.
    let flipped = Revolve {
        flip: true,
        ..revolve_of(&removed, id).clone()
    };
    editor
        .apply(Command::SetFeature {
            feature: id,
            kind: Box::new(flipped.into()),
        })
        .unwrap();

    assert_eq!(
        RevolveError::EdgeBody(own).to_string(),
        format!(
            "its axis is an edge of body {}, which isn't made before it",
            own.0
        )
    );
    assert_eq!(
        RevolveError::Edge(EdgeError::Faces).to_string(),
        "its axis: its edge's faces are out of order or the same"
    );
}
