use glam::{DVec2, DVec3};
use varde_kernel::mesh::{FaceKey, PartKey};
use varde_sketch::Curve;

use super::*;
use crate::testing::{extrude_again, extrude_of, with_body};
use crate::{
    Axis3, CheckError, Command, Document, EditError, Editor, FeatureKind, LengthUnit, OriginPlane,
    Plane, Removable, Targets,
};

/// The sketch id numbered `n`: ids are opaque, so made as the workers'
/// bytes would hold it.
fn id(n: u8) -> Id {
    postcard::from_bytes(&[n]).unwrap()
}

/// The example with "Sketch 2" on XZ added, two lines up from the
/// origin to (0, 30) and on to (10, 30): the editor and the sketch's id
/// and its two lines' ids.
fn with_path() -> (Editor, FeatureId, [Id; 2]) {
    let mut editor = Editor::new(with_body());
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XZ)))
        .unwrap();
    let feature = editor.document().features.last().unwrap().id;
    let mut sketch = Sketch::default();
    let [a, b, c] = [(0.0, 0.0), (0.0, 30.0), (10.0, 30.0)]
        .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
    let up = sketch
        .add_curve(Curve::Line { start: a, end: b }, false)
        .unwrap();
    let across = sketch
        .add_curve(Curve::Line { start: b, end: c }, false)
        .unwrap();
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    (editor, feature, [up, across])
}

/// The example's plate region swept along curves `curves` of sketch
/// `path`, making a new body.
fn along(document: &Document, path: FeatureId, curves: &[Id]) -> Sweep {
    let extrude = extrude_of(document, document.features[1].id);
    Sweep {
        sketch: extrude.sketch,
        regions: extrude.regions.clone(),
        path: PathRef::Chain(vec![PathPart::Curves(CurveChain {
            sketch: path,
            curves: curves.to_vec(),
        })]),
        orientation: Orientation::FollowPath,
        twist: None,
        operation: Operation::NewBody(BodyId::NEW),
    }
}

fn length(document: &Document, text: &str) -> Value {
    Value::new(text, &Sweep::pitch_ask(&document.design())).unwrap()
}

fn turns(document: &Document, text: &str) -> Value {
    Value::new(text, &Sweep::turns_ask(&document.design())).unwrap()
}

fn twist(document: &Document, text: &str) -> Value {
    Value::new(text, &Sweep::twist_ask(&document.design())).unwrap()
}

/// The example's plate region carried round a helix about `axis`.
fn helical(document: &Document, axis: AxisRef) -> Sweep {
    Sweep {
        path: PathRef::Helix(Helix {
            axis,
            pitch: length(document, "4"),
            turns: turns(document, "10"),
            left_handed: false,
            flip: false,
        }),
        ..along(document, FeatureId(0), &[])
    }
}

/// An edge of `body` between faces `maker` made as `parts`, at `near`.
fn edge(body: BodyId, maker: FeatureId, parts: [PartKey; 2], near: DVec3) -> EdgeRef {
    let mut faces = parts.map(|part| FaceKey {
        feature: maker.get(),
        part,
        instance: 0,
    });
    faces.sort();
    EdgeRef { body, faces, near }
}

/// The example plate's top front edge.
fn top_edge(document: &Document) -> EdgeRef {
    edge(
        document.bodies[0].id,
        document.features[1].id,
        [PartKey::EndCap, PartKey::Side { curve: 1 }],
        DVec3::new(0.0, -20.0, 10.0),
    )
}

fn add(editor: &mut Editor, kind: impl Into<FeatureKind>) -> Result<FeatureId, EditError> {
    editor.apply(editor.document().add_feature(kind.into()))?;
    Ok(editor.document().features().last().unwrap().id)
}

fn sweep_of(document: &Document, id: FeatureId) -> &Sweep {
    match &document.feature(id).unwrap().kind {
        FeatureKind::Sweep(sweep) => sweep,
        other => panic!("{other:?}"),
    }
}

/// `sweep` refused as the next feature of `editor`'s document, for
/// `why`, leaving the document as it was.
fn refused(editor: &mut Editor, sweep: Sweep, why: SweepError) {
    let next = FeatureId(editor.document().next_id);
    let before = editor.document().clone();
    assert_eq!(
        add(editor, sweep),
        Err(EditError::Invalid(CheckError::Sweep(next, why)))
    );
    assert_eq!(*editor.document(), before);
}

/// A sweep is "Sweep 1", makes "Body 2", hides its profile's sketch but
/// not its path's, and uses both; undo takes it away and redo puts it
/// back.
#[test]
fn a_sweep_is_added_and_undone() {
    let (mut editor, path, lines) = with_path();
    let before = editor.document().clone();
    let sweep = along(&before, path, &lines);
    let id = add(&mut editor, sweep.clone()).unwrap();
    let document = editor.document();
    let feature = document.feature(id).unwrap();
    assert_eq!(feature.name, "Sweep 1");
    assert_eq!(feature.kind.noun(), "Sweep");
    let body = document.bodies.last().unwrap();
    assert_eq!((body.name.as_str(), body.created_by), ("Body 2", id));
    assert_eq!(feature.kind.new_body(), Some(body.id));
    let profile = before.features[0].id;
    assert_eq!(feature.kind.sketch(), Some(profile));
    assert_eq!(feature.kind.uses(), [profile, path]);
    assert_eq!(feature.kind.bodies(), Vec::new());
    assert!(!document.feature(profile).unwrap().visible);
    assert!(document.feature(path).unwrap().visible);
    editor.undo();
    assert_eq!(*editor.document(), before);
    editor.redo();
    assert_eq!(
        sweep_of(editor.document(), id).path,
        sweep.path,
        "the path as added"
    );
}

/// Editing a sweep keeps its id, name and body while it makes one;
/// making it a join drops its body, a helix sweep drops its path's
/// sketch from what it uses.
#[test]
fn a_sweep_is_edited() {
    let (mut editor, path, lines) = with_path();
    let document = editor.document().clone();
    let id = add(&mut editor, along(&document, path, &lines)).unwrap();
    let body = editor.document().bodies.last().unwrap().id;
    let set = |editor: &mut Editor, sweep: Sweep| {
        editor.apply(Command::SetFeature {
            feature: id,
            kind: Box::new(sweep.into()),
        })
    };
    let generation = editor.generation();
    set(&mut editor, along(&document, path, &lines)).unwrap();
    assert_eq!(editor.generation(), generation);
    let kept = Sweep {
        orientation: Orientation::Keep,
        twist: Some(twist(&document, "90")),
        ..along(&document, path, &lines[..1])
    };
    set(&mut editor, kept.clone()).unwrap();
    assert_eq!(
        *sweep_of(editor.document(), id),
        Sweep {
            operation: Operation::NewBody(body),
            ..kept
        }
    );
    let joined = Sweep {
        operation: Operation::Join(Targets::default()),
        ..helical(&document, AxisRef::Origin(Axis3::Z))
    };
    set(&mut editor, joined.clone()).unwrap();
    let now = editor.document();
    assert!(now.body(body).is_none());
    assert_eq!(now.feature(id).unwrap().name, "Sweep 1");
    assert_eq!(
        now.feature(id).unwrap().kind.uses(),
        [document.features[0].id]
    );
    editor.undo();
    editor.undo();
    assert_eq!(
        *sweep_of(editor.document(), id),
        Sweep {
            operation: Operation::NewBody(body),
            ..along(&document, path, &lines)
        }
    );
}

/// Its own parts: regions, parts, curves and edges and their order, a
/// helix's options and values, and the twist.
#[test]
fn its_own_parts_are_checked() {
    let (editor, path, lines) = with_path();
    let document = editor.document();
    let design = document.design();
    let good = along(document, path, &lines);
    good.check_own(&design).unwrap();
    let check = |change: &dyn Fn(&mut Sweep)| {
        let mut sweep = good.clone();
        change(&mut sweep);
        sweep.check_own(&design)
    };
    fn parts(sweep: &mut Sweep) -> &mut Vec<PathPart> {
        match &mut sweep.path {
            PathRef::Chain(parts) => parts,
            PathRef::Helix(_) => unreachable!(),
        }
    }
    assert_eq!(check(&|s| s.regions.clear()), Err(SweepError::Regions(0)));
    let region = good.regions[0].clone();
    assert_eq!(
        check(&|s| s.regions = vec![region.clone(); MAX_SWEEP_REGIONS + 1]),
        Err(SweepError::Regions(MAX_SWEEP_REGIONS + 1))
    );
    assert_eq!(check(&|s| parts(s).clear()), Err(SweepError::Parts(0)));
    let part = parts(&mut good.clone())[0].clone();
    assert_eq!(
        check(&|s| *parts(s) = vec![part.clone(); MAX_PATH_PARTS + 1]),
        Err(SweepError::Parts(MAX_PATH_PARTS + 1))
    );
    // Several parts of one sketch are the document's to allow, and
    // regenerating's to join.
    assert_eq!(check(&|s| *parts(s) = vec![part.clone(); 3]), Ok(()));
    fn curves(sweep: &mut Sweep) -> &mut Vec<Id> {
        match &mut parts(sweep)[0] {
            PathPart::Curves(chain) => &mut chain.curves,
            PathPart::Edges { .. } => unreachable!(),
        }
    }
    assert_eq!(check(&|s| curves(s).clear()), Err(SweepError::EmptyPart));
    assert_eq!(check(&|s| curves(s).reverse()), Err(SweepError::CurveOrder));
    assert_eq!(
        check(&|s| curves(s)[1] = lines[0]),
        Err(SweepError::CurveOrder)
    );
    // 64 parts of 17 curves are 1 088, over the limit; the sum is
    // checked as it goes.
    let many: Vec<Id> = (1..=17).map(id).collect();
    let big = PathPart::Curves(CurveChain {
        sketch: path,
        curves: many,
    });
    assert_eq!(
        check(&|s| *parts(s) = vec![big.clone(); MAX_PATH_PARTS]),
        Err(SweepError::PathCurves(1037))
    );
    let edge = top_edge(document);
    let other = EdgeRef {
        near: edge.near + DVec3::X,
        ..edge
    };
    let edges = |list: Vec<EdgeRef>| PathPart::Edges {
        edges: list,
        tangent: true,
    };
    assert_eq!(
        check(&|s| *parts(s) = vec![edges(vec![edge, other])]),
        Ok(())
    );
    assert_eq!(
        check(&|s| *parts(s) = vec![edges(vec![other, edge])]),
        Err(SweepError::EdgeOrder)
    );
    assert_eq!(
        check(&|s| *parts(s) = vec![edges(vec![edge, edge])]),
        Err(SweepError::EdgeOrder)
    );
    let elsewhere = EdgeRef {
        body: BodyId(999),
        ..edge
    };
    assert_eq!(
        check(&|s| *parts(s) = vec![edges(vec![edge, elsewhere])]),
        Err(SweepError::EdgeBodies)
    );
    let swapped = EdgeRef {
        faces: [edge.faces[1], edge.faces[0]],
        ..edge
    };
    assert_eq!(
        check(&|s| *parts(s) = vec![edges(vec![swapped])]),
        Err(SweepError::Edge(EdgeError::Faces))
    );
    let far = EdgeRef {
        near: DVec3::new(f64::NAN, 0.0, 0.0),
        ..edge
    };
    assert!(matches!(
        check(&|s| *parts(s) = vec![edges(vec![far])]),
        Err(SweepError::Edge(EdgeError::Near(_)))
    ));
    // The twist: within 8 turns either way.
    assert_eq!(check(&|s| s.twist = Some(twist(document, "-2880"))), Ok(()));
    let mut over = twist(document, "2880");
    over.value = 8.5 * std::f64::consts::TAU;
    assert_eq!(
        check(&|s| s.twist = Some(over.clone())),
        Err(SweepError::Twist)
    );
    // A helix: follows its path, no twist, a pitch, turns in range.
    let helix = helical(document, AxisRef::Origin(Axis3::Z));
    helix.check_own(&design).unwrap();
    let check_helix = |change: &dyn Fn(&mut Sweep)| {
        let mut sweep = helix.clone();
        change(&mut sweep);
        sweep.check_own(&design)
    };
    fn helix_of(sweep: &mut Sweep) -> &mut Helix {
        match &mut sweep.path {
            PathRef::Helix(helix) => helix,
            PathRef::Chain(_) => unreachable!(),
        }
    }
    assert_eq!(
        check_helix(&|s| s.orientation = Orientation::Keep),
        Err(SweepError::HelixOptions)
    );
    assert_eq!(
        check_helix(&|s| s.twist = Some(twist(document, "10"))),
        Err(SweepError::HelixOptions)
    );
    assert_eq!(
        check_helix(&|s| helix_of(s).pitch = Value {
            text: "0".into(),
            value: 0.0
        }),
        Err(SweepError::Pitch)
    );
    for text in ["0", "0.0001", "1001", "-3"] {
        assert_eq!(
            check_helix(&|s| helix_of(s).turns = Value {
                text: text.into(),
                value: text.parse().unwrap()
            }),
            Err(SweepError::Turns),
            "{text}"
        );
    }
    for text in ["0.001", "1000", "2.5"] {
        assert_eq!(
            check_helix(&|s| helix_of(s).turns = turns(document, text)),
            Ok(()),
            "{text}"
        );
    }
    assert!(matches!(
        check_helix(&|s| helix_of(s).axis = AxisRef::Edge(swapped)),
        Err(SweepError::Axis(MotionError::Edge(EdgeError::Faces)))
    ));
}

/// What a sweep names: its profile's sketch and its path's sketches
/// before it (not the profile's own), its path's edges' and its helix
/// axis's bodies and makers before it, and on adding, its path's curves
/// in their sketches.
#[test]
fn what_it_names_is_checked() {
    let (mut editor, path, lines) = with_path();
    let document = editor.document().clone();
    let profile = document.features[0].id;
    let extrude = document.features[1].id;
    let good = along(&document, path, &lines);
    let with_part = |part: PathPart| Sweep {
        path: PathRef::Chain(vec![part]),
        ..good.clone()
    };
    let curves =
        |sketch: FeatureId, curves: Vec<Id>| PathPart::Curves(CurveChain { sketch, curves });
    let FeatureKind::Sketch { sketch: drawn, .. } = &document.features[0].kind else {
        unreachable!()
    };
    let outline = drawn.curves[0].id;
    refused(
        &mut editor,
        with_part(curves(profile, vec![outline])),
        SweepError::OwnSketch,
    );
    refused(
        &mut editor,
        with_part(curves(extrude, vec![id(1)])),
        SweepError::PathSketch(extrude),
    );
    refused(
        &mut editor,
        with_part(curves(path, vec![lines[0], id(127)])),
        SweepError::Curve(id(127)),
    );
    let edge = top_edge(&document);
    let edges = |edge: EdgeRef| PathPart::Edges {
        edges: vec![edge],
        tangent: false,
    };
    add(&mut editor, with_part(edges(edge))).unwrap();
    editor.undo();
    let gone = EdgeRef {
        body: BodyId(999),
        ..edge
    };
    refused(
        &mut editor,
        with_part(edges(gone)),
        SweepError::EdgeBody(gone.body),
    );
    let next = FeatureId(editor.document().next_id);
    let later = EdgeRef {
        faces: [
            edge.faces[0],
            FaceKey {
                feature: next.get(),
                ..edge.faces[1]
            },
        ],
        ..edge
    };
    refused(
        &mut editor,
        with_part(edges(later)),
        SweepError::EdgeMaker(next),
    );
    // A helix about the plate's edge, then about one of a body that isn't
    // there, or of faces made later.
    add(&mut editor, helical(&document, AxisRef::Edge(edge))).unwrap();
    editor.undo();
    refused(
        &mut editor,
        helical(&document, AxisRef::Edge(gone)),
        SweepError::AxisBody(gone.body),
    );
    refused(
        &mut editor,
        helical(&document, AxisRef::Edge(later)),
        SweepError::AxisMaker(next),
    );
    refused(
        &mut editor,
        Sweep {
            sketch: extrude,
            ..good.clone()
        },
        SweepError::Sketch(extrude),
    );
    // A path's curve deleted later is the regeneration's to report: the
    // document keeps the sweep.
    let id = add(&mut editor, good).unwrap();
    let FeatureKind::Sketch { sketch, .. } = &editor.document().feature(path).unwrap().kind else {
        unreachable!()
    };
    let mut sketch = sketch.clone();
    sketch.curves.retain(|entry| entry.id != lines[1]);
    editor
        .apply(Command::SetSketch {
            feature: path,
            sketch: Box::new(sketch),
        })
        .unwrap();
    assert!(editor.document().feature(id).is_some());
    editor.document().check().unwrap();
}

/// Removing its path's sketch, its path's edges' body or its helix
/// axis's body removes the sweep (and the body it makes); removing a
/// body it only excludes doesn't.
#[test]
fn removal_follows_its_sketches_and_bodies() {
    let (mut editor, path, lines) = with_path();
    let document = editor.document().clone();
    let id = add(&mut editor, along(&document, path, &lines)).unwrap();
    let made = editor.document().bodies.last().unwrap().id;
    let removal = editor.document().removal(Removable::Feature(path));
    assert_eq!(removal.features, [path, id]);
    assert_eq!(removal.bodies, [made]);
    editor.apply(Command::RemoveFeature(path)).unwrap();
    assert!(editor.document().feature(id).is_none());
    editor.undo();

    let (mut editor, plate, maker) = {
        let mut editor = Editor::new(with_body());
        let (maker, plate) = extrude_again(&mut editor);
        (editor, plate, maker)
    };
    let document = editor.document().clone();
    let on_plate = EdgeRef {
        body: plate,
        ..edge(
            plate,
            maker,
            [PartKey::EndCap, PartKey::Side { curve: 1 }],
            DVec3::ZERO,
        )
    };
    let helix = add(&mut editor, helical(&document, AxisRef::Edge(on_plate))).unwrap();
    let along_edges = Sweep {
        path: PathRef::Chain(vec![PathPart::Edges {
            edges: vec![on_plate],
            tangent: true,
        }]),
        operation: Operation::Cut(Targets {
            excluded: vec![document.bodies[0].id],
        }),
        ..helical(&document, AxisRef::Origin(Axis3::Z))
    };
    let edges = add(&mut editor, along_edges).unwrap();
    assert_eq!(
        editor.document().feature(edges).unwrap().kind.bodies(),
        [plate]
    );
    let removal = editor.document().removal(Removable::Body(plate));
    assert!(removal.features.contains(&helix));
    assert!(removal.features.contains(&edges));
    // The example's body is only excluded from the cut.
    let removal = editor
        .document()
        .removal(Removable::Body(document.bodies[0].id));
    assert!(!removal.features.contains(&edges));
    editor
        .apply(Command::RemoveBody(document.bodies[0].id))
        .unwrap();
    let FeatureKind::Sweep(cut) = &editor.document().feature(edges).unwrap().kind else {
        unreachable!()
    };
    assert_eq!(cut.operation, Operation::Cut(Targets::default()));
}

/// Changing the units pins a twist and a pitch in the units they were
/// typed in; turns stay as typed.
#[test]
fn set_units_pins_its_values() {
    let (mut editor, path, lines) = with_path();
    let document = editor.document().clone();
    let twisted = Sweep {
        twist: Some(twist(&document, "90 + 0")),
        ..along(&document, path, &lines)
    };
    let one = add(&mut editor, twisted).unwrap();
    let two = add(&mut editor, helical(&document, AxisRef::Origin(Axis3::Z))).unwrap();
    editor.apply(Command::SetUnits(LengthUnit::In)).unwrap();
    let now = editor.document();
    let twist = sweep_of(now, one).twist.clone().unwrap();
    assert_eq!(twist.value, std::f64::consts::FRAC_PI_2);
    let PathRef::Helix(helix) = &sweep_of(now, two).path else {
        unreachable!()
    };
    assert_eq!(
        (helix.pitch.text.as_str(), helix.pitch.value),
        ("4 mm", 4.0)
    );
    assert_eq!((helix.turns.text.as_str(), helix.turns.value), ("10", 10.0));
    now.check().unwrap();
}

#[test]
fn sweeps_round_trip() {
    let (mut editor, path, lines) = with_path();
    let document = editor.document().clone();
    add(&mut editor, along(&document, path, &lines)).unwrap();
    let edge = top_edge(&document);
    let mixed = Sweep {
        path: PathRef::Chain(vec![
            PathPart::Edges {
                edges: vec![edge],
                tangent: true,
            },
            PathPart::Curves(CurveChain {
                sketch: path,
                curves: vec![lines[1]],
            }),
        ]),
        orientation: Orientation::Keep,
        twist: Some(twist(&document, "-45")),
        operation: Operation::Join(Targets::default()),
        ..along(&document, path, &lines)
    };
    add(&mut editor, mixed).unwrap();
    add(&mut editor, helical(&document, AxisRef::Edge(edge))).unwrap();
    let document = editor.document().clone();
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document)
    );
}

/// Bytes whose sweep is wrong are refused as they're read.
#[test]
fn wrong_sweeps_are_refused_when_read() {
    let (mut editor, path, lines) = with_path();
    let document = editor.document().clone();
    let sweep = add(&mut editor, along(&document, path, &lines)).unwrap();
    let read = |change: &dyn Fn(&mut Sweep)| {
        let mut document = editor.document().clone();
        let index = document.feature_index(sweep).unwrap();
        let FeatureKind::Sweep(sweep) = &mut document.features[index].kind else {
            unreachable!()
        };
        change(sweep);
        Document::from_postcard(&document.to_postcard()).map_err(|e| e.to_string())
    };
    let n = sweep.get();
    assert_eq!(read(&|_| {}).as_ref(), Ok(editor.document()));
    assert_eq!(
        read(&|s| s.path = PathRef::Chain(Vec::new())),
        Err(format!("feature {n}: its path has 0 parts, not 1 to 64"))
    );
    assert_eq!(
        read(&|s| s.orientation = Orientation::Keep)
            .as_ref()
            .map(|_| ()),
        Ok(())
    );
    // A curve gone from its sketch reads: regenerating says it's gone.
    assert!(
        read(&|s| {
            let PathRef::Chain(parts) = &mut s.path else {
                unreachable!()
            };
            parts[0] = PathPart::Curves(CurveChain {
                sketch: path,
                curves: vec![id(127)],
            });
        })
        .is_ok()
    );
    assert_eq!(
        read(&|s| s.sketch = path),
        Err(format!(
            "feature {n}: its path runs along curves of its profile's own sketch"
        ))
    );
    assert_eq!(
        read(&|s| s.twist = Some(Value {
            text: "1e9".into(),
            value: 1e9
        })),
        Err(format!(
            "feature {n}: its twist's expression doesn't give its value, or it isn't an \
             angle within 8 turns"
        ))
    );
}

/// Kinds are stored by name in files, but the workers' postcard keeps
/// the variants' order: the sweep comes after the draft. (This is the
/// one place its index is written down.)
#[test]
fn a_sweep_is_the_sixteenth_kind() {
    let (editor, path, lines) = with_path();
    let sweep = along(editor.document(), path, &lines);
    let bytes = postcard::to_stdvec(&FeatureKind::from(sweep)).unwrap();
    // The kind, then the profile's sketch.
    assert_eq!(bytes[..2], [15, 0]);
}

#[test]
fn errors_say_what_is_wrong() {
    assert_eq!(
        CheckError::Sweep(FeatureId(3), SweepError::PathSketch(FeatureId(1))).to_string(),
        "feature 3: its path runs along feature 1, which isn't a sketch before it"
    );
    assert_eq!(
        SweepError::Turns.to_string(),
        "its turns' expression doesn't give its value, or it isn't from 0.001 to 1000"
    );
    assert_eq!(
        SweepError::PathCurves(1025).to_string(),
        "its path names 1025 curves and edges, more than 1024"
    );
    assert_eq!(
        SweepError::Axis(MotionError::Edge(EdgeError::Faces)).to_string(),
        "its helix: its axis: its edge's faces are out of order or the same"
    );
}
