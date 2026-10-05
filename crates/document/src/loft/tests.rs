use glam::DVec2;
use varde_sketch::Curve;

use super::*;
use crate::testing::with_body;
use crate::{
    CheckError, Command, Document, EditError, Editor, FeatureKind, LengthUnit, OriginPlane, Plane,
    Removable, Targets,
};

/// The sketches a loft is made from in [`lofted`]'s document.
struct Sketches {
    /// On XY: a square, its corner point first.
    low: FeatureId,
    low_region: RegionRef,
    low_corner: Id,
    /// On XZ: a smaller square, and a point apart.
    high: FeatureId,
    high_region: RegionRef,
    apex: Id,
    /// On YZ: two lines end to end, a rail.
    rail: FeatureId,
    rail_curves: Vec<Id>,
}

/// Draws the square of side `side` about the origin: its corners' ids.
fn square(sketch: &mut Sketch, side: f64) -> [Id; 4] {
    let h = side / 2.0;
    let corners = [(-h, -h), (h, -h), (h, h), (-h, h)]
        .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
    for (k, &start) in corners.iter().enumerate() {
        let end = corners[(k + 1) % 4];
        sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    }
    corners
}

/// Adds a sketch on `plane` drawn by `draw`: its id and what `draw`
/// gave.
fn add_sketch<T>(
    editor: &mut Editor,
    plane: OriginPlane,
    draw: impl FnOnce(&mut Sketch) -> T,
) -> (FeatureId, T) {
    editor
        .apply(editor.document().add_sketch(Plane::Origin(plane)))
        .unwrap();
    let feature = editor.document().features.last().unwrap().id;
    let mut sketch = Sketch::default();
    let drawn = draw(&mut sketch);
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    (feature, drawn)
}

/// The region of `sketch`'s only region.
fn only_region(document: &Document, sketch: FeatureId) -> RegionRef {
    let FeatureKind::Sketch { sketch, .. } = &document.feature(sketch).unwrap().kind else {
        panic!("a sketch");
    };
    let profiles = sketch.profiles().unwrap();
    assert_eq!(profiles.regions.len(), 1);
    profiles.reference(0).unwrap()
}

/// The example with three sketches to loft from.
fn sketches() -> (Editor, Sketches) {
    let mut editor = Editor::new(with_body());
    let (low, corners) = add_sketch(&mut editor, OriginPlane::XY, |s| square(s, 20.0));
    let (high, apex) = add_sketch(&mut editor, OriginPlane::XZ, |s| {
        square(s, 10.0);
        s.add_point(DVec2::new(0.0, 30.0)).unwrap()
    });
    let (rail, rail_curves) = add_sketch(&mut editor, OriginPlane::YZ, |s| {
        let [a, b, c] = [(0.0, 0.0), (5.0, 5.0), (5.0, 10.0)]
            .map(|(x, y)| s.add_point(DVec2::new(x, y)).unwrap());
        let one = s
            .add_curve(Curve::Line { start: a, end: b }, false)
            .unwrap();
        let two = s
            .add_curve(Curve::Line { start: b, end: c }, false)
            .unwrap();
        vec![one, two]
    });
    let document = editor.document();
    let sketches = Sketches {
        low,
        low_region: only_region(document, low),
        low_corner: corners[0],
        high,
        high_region: only_region(document, high),
        apex,
        rail,
        rail_curves,
    };
    (editor, sketches)
}

/// A smooth loft from the low square, started at its corner, to the high
/// one, along the rail, making a new body.
fn loft(s: &Sketches) -> Loft {
    Loft {
        sections: vec![
            Section::Region {
                sketch: s.low,
                region: s.low_region.clone(),
                start: Some(s.low_corner),
            },
            Section::Region {
                sketch: s.high,
                region: s.high_region.clone(),
                start: None,
            },
        ],
        mode: LoftMode::Smooth,
        closed: false,
        rails: vec![CurveChain {
            sketch: s.rail,
            curves: s.rail_curves.clone(),
        }],
        operation: Operation::NewBody(BodyId::NEW),
    }
}

fn add(editor: &mut Editor, kind: impl Into<FeatureKind>) -> Result<FeatureId, EditError> {
    editor.apply(editor.document().add_feature(kind.into()))?;
    Ok(editor.document().features().last().unwrap().id)
}

fn loft_of(document: &Document, id: FeatureId) -> &Loft {
    match &document.feature(id).unwrap().kind {
        FeatureKind::Loft(loft) => loft,
        other => panic!("{other:?}"),
    }
}

/// `loft` refused as the next feature of `editor`'s document, for `why`,
/// leaving the document as it was.
fn refused(editor: &mut Editor, loft: Loft, why: LoftError) {
    let next = FeatureId(editor.document().next_id);
    let before = editor.document().clone();
    assert_eq!(
        add(editor, loft),
        Err(EditError::Invalid(CheckError::Loft(next, why)))
    );
    assert_eq!(*editor.document(), before);
}

/// A loft is "Loft 1", makes "Body 2", hides its sections' sketches but
/// not its rail's, and uses all three; undo takes it away and redo puts
/// it back.
#[test]
fn a_loft_is_added_and_undone() {
    let (mut editor, s) = sketches();
    let before = editor.document().clone();
    let id = add(&mut editor, loft(&s)).unwrap();
    let document = editor.document();
    let feature = document.feature(id).unwrap();
    assert_eq!(feature.name, "Loft 1");
    assert_eq!(feature.kind.noun(), "Loft");
    let body = document.bodies.last().unwrap();
    assert_eq!((body.name.as_str(), body.created_by), ("Body 2", id));
    let made = loft_of(document, id).clone();
    assert_eq!(made.operation, Operation::NewBody(body.id));
    assert_eq!(
        Loft {
            operation: Operation::NewBody(BodyId::NEW),
            ..made.clone()
        },
        loft(&s)
    );
    assert_eq!(feature.kind.new_body(), Some(body.id));
    assert_eq!(feature.kind.operation(), Some(&made.operation));
    assert_eq!(feature.kind.uses(), [s.low, s.high, s.rail]);
    assert_eq!(feature.kind.profile_sketches(), [s.low, s.high]);
    assert_eq!(feature.kind.sketch(), None);
    assert_eq!(feature.kind.bodies(), Vec::new());
    let visible = |id| document.feature(id).unwrap().visible;
    assert!(!visible(s.low) && !visible(s.high) && visible(s.rail));
    // A second is "Loft 2", from a point.
    let to_point = Loft {
        sections: vec![
            loft(&s).sections[0].clone(),
            Section::Point {
                sketch: s.high,
                point: s.apex,
            },
        ],
        mode: LoftMode::Ruled,
        rails: Vec::new(),
        ..loft(&s)
    };
    let again = add(&mut editor, to_point).unwrap();
    assert_eq!(editor.document().feature(again).unwrap().name, "Loft 2");
    editor.undo();
    editor.undo();
    assert_eq!(*editor.document(), before);
    editor.redo();
    assert_eq!(loft_of(editor.document(), id), &made);
}

/// Editing a loft keeps its body while it makes one, removes it once it
/// joins, and keeps the other features' visibility as it was.
#[test]
fn a_loft_is_edited() {
    let (mut editor, s) = sketches();
    let id = add(&mut editor, loft(&s)).unwrap();
    let body = editor.document().bodies.last().unwrap().id;
    let reversed = Loft {
        sections: loft(&s).sections.into_iter().rev().collect(),
        mode: LoftMode::Ruled,
        ..loft(&s)
    };
    editor
        .apply(Command::SetFeature {
            feature: id,
            kind: Box::new(reversed.clone().into()),
        })
        .unwrap();
    let document = editor.document();
    assert_eq!(document.bodies.last().unwrap().id, body);
    assert_eq!(
        loft_of(document, id),
        &Loft {
            operation: Operation::NewBody(body),
            ..reversed.clone()
        }
    );
    let joined = Loft {
        operation: Operation::Join(Targets::default()),
        ..reversed
    };
    editor
        .apply(Command::SetFeature {
            feature: id,
            kind: Box::new(joined.clone().into()),
        })
        .unwrap();
    assert!(editor.document().body(body).is_none());
    // It holds the body's id, for a new body again.
    let held = Loft {
        operation: Operation::Join(Targets {
            excluded: Vec::new(),
            held: Some(body),
        }),
        ..joined
    };
    assert_eq!(loft_of(editor.document(), id), &held);
}

/// What needs only the loft, checked by `check_own` and as it's added.
#[test]
fn its_own_parts_are_checked() {
    let (mut editor, s) = sketches();
    let good = loft(&s);
    assert_eq!(good.check_own(), Ok(()));
    let with = |change: &dyn Fn(&mut Loft)| {
        let mut loft = good.clone();
        change(&mut loft);
        loft
    };
    let point = Section::Point {
        sketch: s.high,
        point: s.apex,
    };
    let cases: Vec<(Loft, LoftError)> = vec![
        (with(&|l| l.sections.truncate(1)), LoftError::Sections(1)),
        (
            with(&|l| {
                let first = l.sections[0].clone();
                l.sections = vec![first; MAX_LOFT_SECTIONS + 1];
            }),
            LoftError::Sections(MAX_LOFT_SECTIONS + 1),
        ),
        (
            with(&|l| {
                if let Section::Region { region, .. } = &mut l.sections[1] {
                    region.curves.reverse();
                }
            }),
            LoftError::Region(RegionRefError::Unsorted),
        ),
        (
            with(&|l| {
                if let Section::Region { region, .. } = &mut l.sections[1] {
                    region.holes = vec![s.low_region.curves.clone()];
                }
            }),
            LoftError::Holes(1),
        ),
        (
            with(&|l| l.sections.insert(1, point.clone())),
            LoftError::PointInside(1),
        ),
        (
            with(&|l| l.sections = vec![point.clone(), point.clone()]),
            LoftError::Points,
        ),
        (with(&|l| l.closed = true), LoftError::ClosedSections(2)),
        (
            with(&|l| {
                l.closed = true;
                l.rails.clear();
                l.sections.push(point.clone());
            }),
            LoftError::ClosedPoint,
        ),
        (
            with(&|l| {
                l.closed = true;
                let first = l.sections[0].clone();
                l.sections.push(first);
            }),
            LoftError::ClosedRails,
        ),
        (
            with(&|l| {
                l.rails = (0..=MAX_LOFT_RAILS)
                    .map(|k| CurveChain {
                        sketch: s.rail,
                        curves: vec![s.rail_curves[k % 2]],
                    })
                    .collect();
            }),
            LoftError::Rails(MAX_LOFT_RAILS + 1),
        ),
        (
            with(&|l| l.rails[0].curves.clear()),
            LoftError::RailCurves(0),
        ),
        (
            with(&|l| {
                let id = s.rail_curves[0];
                l.rails[0].curves = vec![id; MAX_RAIL_CURVES + 1];
            }),
            LoftError::RailCurves(MAX_RAIL_CURVES + 1),
        ),
        (with(&|l| l.rails[0].curves.reverse()), LoftError::RailOrder),
        (
            with(&|l| {
                let rail = l.rails[0].clone();
                l.rails.push(rail);
            }),
            LoftError::RailRepeated,
        ),
    ];
    for (loft, why) in cases {
        assert_eq!(loft.check_own(), Err(why), "{loft:?}");
        refused(&mut editor, loft, why);
    }
    // Closed with three sections, ruled, no rails: taken; a point first
    // or last, open: taken.
    let closed = with(&|l| {
        l.closed = true;
        l.rails.clear();
        let first = l.sections[0].clone();
        l.sections.push(first);
    });
    assert_eq!(closed.check_own(), Ok(()));
    add(&mut editor, closed).unwrap();
    let from_point = with(&|l| l.sections.insert(0, point.clone()));
    add(&mut editor, from_point).unwrap();
}

/// What the sections and rails name of other features, checked by the
/// document; what they name in their sketches, only as the loft is added
/// or edited: a sketch edit taking a start point away later is taken,
/// and regeneration reports it.
#[test]
fn what_it_names_is_checked() {
    let (mut editor, s) = sketches();
    let extrude = editor.document().features[1].id;
    let later = FeatureId(editor.document().next_id + 5);
    let with = |change: &dyn Fn(&mut Loft)| {
        let mut made = loft(&s);
        change(&mut made);
        made
    };
    let section_sketch = |l: &mut Loft, sketch| {
        if let Section::Region { sketch: on, .. } = &mut l.sections[1] {
            *on = sketch;
        }
    };
    let cases = [
        (
            with(&|l| section_sketch(l, extrude)),
            LoftError::Sketch(extrude),
        ),
        (
            with(&|l| section_sketch(l, later)),
            LoftError::Sketch(later),
        ),
        (
            with(&|l| l.rails[0].sketch = extrude),
            LoftError::RailSketch(extrude),
        ),
        (
            with(&|l| {
                if let Section::Region { start, .. } = &mut l.sections[0] {
                    *start = Some(s.apex);
                }
            }),
            LoftError::Start(s.apex),
        ),
        (
            with(&|l| {
                l.sections[1] = Section::Point {
                    sketch: s.low,
                    point: s.apex,
                };
            }),
            LoftError::Point(s.apex),
        ),
        (
            with(&|l| l.rails[0].sketch = s.low),
            LoftError::RailCurve(s.rail_curves[0]),
        ),
        (
            with(&|l| {
                l.operation = Operation::Join(Targets {
                    excluded: vec![BodyId(999)],
                    held: None,
                });
            }),
            LoftError::Excluded(BodyId(999)),
        ),
    ];
    for (loft, why) in cases {
        refused(&mut editor, loft, why);
    }
    // The start corner taken out of its sketch after: taken.
    let id = add(&mut editor, loft(&s)).unwrap();
    let FeatureKind::Sketch { sketch, .. } = &editor.document().feature(s.low).unwrap().kind else {
        unreachable!()
    };
    let mut sketch = sketch.clone();
    let kept: Vec<Id> = (sketch.curves.iter())
        .filter(|entry| !entry.curve.points().any(|p| p == s.low_corner))
        .map(|entry| entry.id)
        .collect();
    sketch.curves.retain(|entry| kept.contains(&entry.id));
    sketch.points.retain(|point| point.id != s.low_corner);
    editor
        .apply(Command::SetSketch {
            feature: s.low,
            sketch: Box::new(sketch),
        })
        .unwrap();
    assert!(editor.document().feature(id).is_some());
    // A body it makes that isn't its own: refused as read.
    let mut document = editor.document().clone();
    let index = document.feature_index(id).unwrap();
    let FeatureKind::Loft(made) = &mut document.features[index].kind else {
        unreachable!()
    };
    let first = document.bodies[0].id;
    made.operation = Operation::NewBody(first);
    assert_eq!(
        document.check(),
        Err(CheckError::Creator(document.bodies.last().unwrap().id, id))
    );
}

/// Removing a section's sketch or the rail's removes the loft and its
/// body; removing another sketch leaves it.
#[test]
fn removal_follows_every_sketch_it_uses() {
    let (mut editor, s) = sketches();
    let id = add(&mut editor, loft(&s)).unwrap();
    let body = editor.document().bodies.last().unwrap().id;
    for sketch in [s.low, s.high, s.rail] {
        let removal = editor.document().removal(Removable::Feature(sketch));
        assert_eq!(removal.features, [sketch, id]);
        assert_eq!(removal.bodies, [body]);
    }
    let example = editor.document().features[0].id;
    let removal = editor.document().removal(Removable::Feature(example));
    assert!(!removal.features.contains(&id));
    editor.apply(Command::RemoveFeature(s.rail)).unwrap();
    assert!(editor.document().feature(id).is_none());
    assert!(editor.document().body(body).is_none());
}

/// A loft has no values: changing the units leaves it as it was.
#[test]
fn set_units_leaves_it() {
    let (mut editor, s) = sketches();
    let id = add(&mut editor, loft(&s)).unwrap();
    let before = loft_of(editor.document(), id).clone();
    editor.apply(Command::SetUnits(LengthUnit::In)).unwrap();
    assert_eq!(*loft_of(editor.document(), id), before);
}

#[test]
fn lofts_round_trip() {
    let (mut editor, s) = sketches();
    add(&mut editor, loft(&s)).unwrap();
    let ruled = Loft {
        sections: vec![
            Section::Point {
                sketch: s.high,
                point: s.apex,
            },
            loft(&s).sections[0].clone(),
        ],
        mode: LoftMode::Ruled,
        rails: Vec::new(),
        operation: Operation::Cut(Targets::default()),
        ..loft(&s)
    };
    add(&mut editor, ruled).unwrap();
    let document = editor.document().clone();
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document)
    );
}

/// Bytes whose loft is wrong are refused as they're read.
#[test]
fn wrong_lofts_are_refused_when_read() {
    let (mut editor, s) = sketches();
    let id = add(&mut editor, loft(&s)).unwrap();
    let read = |change: &dyn Fn(&mut Loft)| {
        let mut document = editor.document().clone();
        let index = document.feature_index(id).unwrap();
        let FeatureKind::Loft(loft) = &mut document.features[index].kind else {
            unreachable!()
        };
        change(loft);
        Document::from_postcard(&document.to_postcard()).map_err(|e| e.to_string())
    };
    let n = id.get();
    assert_eq!(read(&|_| {}).as_ref(), Ok(editor.document()));
    assert_eq!(
        read(&|l| l.sections.truncate(1)),
        Err(format!("feature {n}: lofts 1 sections, not 2 to 64"))
    );
    assert_eq!(
        read(&|l| l.rails[0].sketch = FeatureId(n)),
        Err(format!(
            "feature {n}: a rail is on feature {n}, which isn't a sketch before it"
        ))
    );
    assert_eq!(
        read(&|l| l.closed = true),
        Err(format!(
            "feature {n}: it's closed with 2 sections, fewer than 3"
        ))
    );
    // Its start point or rail's curves gone from their sketches: read,
    // as a sketch edit may leave them.
    assert!(
        read(&|l| {
            if let Section::Region { start, .. } = &mut l.sections[0] {
                *start = Some(s.apex);
            }
        })
        .is_ok()
    );
}

/// Kinds are stored by name in files, but the workers' postcard keeps
/// the variants' order: the loft comes after the sweep. (This is the
/// one place its index is written down.)
#[test]
fn a_loft_is_the_seventeenth_kind() {
    let (_, s) = sketches();
    let bytes = postcard::to_stdvec(&FeatureKind::from(loft(&s))).unwrap();
    // The kind, the section count, the first section's kind.
    assert_eq!(bytes[..3], [16, 2, 0]);
}

#[test]
fn errors_say_what_is_wrong() {
    assert_eq!(
        CheckError::Loft(FeatureId(3), LoftError::Sketch(FeatureId(2))).to_string(),
        "feature 3: lofts feature 2, which isn't a sketch before it"
    );
    assert_eq!(
        LoftError::PointInside(1).to_string(),
        "its section 2 is a point, which only the first or last may be"
    );
    assert_eq!(
        LoftError::Holes(0).to_string(),
        "its section 1 has holes, but a section is one loop"
    );
    assert_eq!(
        LoftError::Rails(5).to_string(),
        "it has 5 rails, more than 4"
    );
    assert_eq!(
        LoftError::ClosedRails.to_string(),
        "it's closed, but has rails"
    );
    assert_eq!(LoftError::Points.to_string(), "all its sections are points");
}
