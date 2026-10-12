//! The tools `sweep` and `loft`, through `Varde::update`: the panel's
//! defaults, values given, regions, curves, points and keys taken or
//! refused, values past their bounds, and what only the panel refuses,
//! said. The kernel sweeps and lofts nothing yet, so each applies and
//! fails in regeneration, as for the user.

use glam::DVec2;
use serde_json::{Value, json};
use varde_document::{
    AxisRef, FeatureKind, LoftMode, Operation, Orientation, OriginPlane, PathPart, PathRef, Section,
};
use varde_sketch::{Curve, Sketch};

use super::blend::sketched;
use super::motion::{applied_last, bodies, edge_between, face_facing, face_of_form, with_disc};
use super::*;

/// Takes the last feature back, and regenerates.
fn back(varde: &mut Varde, requests: &Lane) {
    undo(varde);
    regenerate(varde, requests);
}

/// Draws lines through `points` in turn, an open polyline.
fn polyline(points: &[(f64, f64)]) -> impl FnOnce(&mut Sketch) + '_ {
    move |sketch| {
        let ids: Vec<_> = (points.iter())
            .map(|&(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap())
            .collect();
        for pair in ids.windows(2) {
            let line = Curve::Line {
                start: pair[0],
                end: pair[1],
            };
            sketch.add_curve(line, false).unwrap();
        }
    }
}

/// Draws the square from `min` to `max`, and a point on its own at
/// `lone`.
fn square(min: (f64, f64), max: (f64, f64), lone: (f64, f64)) -> impl FnOnce(&mut Sketch) {
    move |sketch| {
        let corners = [
            (min.0, min.1),
            (max.0, min.1),
            (max.0, max.1),
            (min.0, max.1),
        ]
        .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
        for (k, &start) in corners.iter().enumerate() {
            let end = corners[(k + 1) % corners.len()];
            sketch.add_curve(Curve::Line { start, end }, false).unwrap();
        }
        sketch.add_point(DVec2::new(lone.0, lone.1)).unwrap();
    }
}

/// The sketch named `name`.
fn sketch_named<'v>(varde: &'v Varde, name: &str) -> &'v Sketch {
    let document = varde.screen.doc().unwrap().editor.document();
    let feature = (document.features().iter())
        .find(|feature| feature.name == name)
        .unwrap();
    crate::doc::sketch_of(document, feature.id).unwrap()
}

/// The id of the point of sketch `name` at `at`.
fn point_at(varde: &Varde, name: &str, at: (f64, f64)) -> u32 {
    let sketch = sketch_named(varde, name);
    let point = (sketch.points.iter())
        .find(|point| point.at == DVec2::new(at.0, at.1))
        .unwrap();
    point.id.get()
}

/// The id of curve `at` of sketch `name`, in its order.
fn curve_at(varde: &Varde, name: &str, at: usize) -> u32 {
    sketch_named(varde, name).curves[at].id.get()
}

/// The example with an open path of two lines on XZ, "Sketch 2".
fn with_path() -> (Varde, Lane) {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let path = [(0.0, 0.0), (0.0, 20.0), (10.0, 40.0)];
    let name = sketched(&mut varde, &requests, OriginPlane::XZ, polyline(&path));
    assert_eq!(name, "Sketch 2");
    (varde, requests)
}

/// `sweep` of the disc's region along a chain of sketch curves follows
/// the path with no twist, a new body, as the panel starts; along model
/// edges as a blend picks them, Tangent chain stored; with Keep
/// orientation, a twist, an operation and a body left out given.
#[test]
fn sweep_runs_along_curves_or_edges() {
    let (mut varde, requests) = with_path();
    let (disc, _) = disc_and_plate(&varde);
    let line = curve_at(&varde, "Sketch 2", 0);
    let FeatureKind::Sweep(sweep) = applied_last(
        &mut varde,
        &requests,
        "sweep",
        json!({ "sketch": "Sketch 1", "profiles": [disc],
                "path": { "curves": [{ "sketch": "Sketch 2", "curve": line }] } }),
    ) else {
        panic!("not a sweep");
    };
    assert_eq!(sweep.regions.len(), 1);
    let PathRef::Chain(parts) = &sweep.path else {
        panic!("not a path");
    };
    // The chain the line is in: both lines.
    assert!(matches!(&parts[..], [PathPart::Curves(chain)] if chain.curves.len() == 2));
    assert_eq!(sweep.orientation, Orientation::FollowPath);
    assert!(sweep.twist.is_none());
    assert!(matches!(sweep.operation, Operation::NewBody(_)));
    assert_eq!(
        varde.mcp.log.back().unwrap().args,
        "Sketch 1 1 region along a path"
    );
    back(&mut varde, &requests);

    let plate = bodies(&varde)[0];
    let edge = edge_between(&varde, "Body 1", [30.0, 20.0, 0.0], [30.0, 20.0, 10.0]);
    let FeatureKind::Sweep(sweep) = applied_last(
        &mut varde,
        &requests,
        "sweep",
        json!({ "sketch": "Sketch 1", "profiles": [disc],
                "path": { "edges": [edge.clone()], "tangent": false },
                "keep_orientation": true, "twist": "90", "operation": "join",
                "exclude": ["Body 1"] }),
    ) else {
        panic!("not a sweep");
    };
    let PathRef::Chain(parts) = &sweep.path else {
        panic!("not a path");
    };
    assert!(
        matches!(&parts[..], [PathPart::Edges { edges, tangent: false }]
        if edges.len() == 1 && edges[0].body == plate)
    );
    assert_eq!(sweep.orientation, Orientation::Keep);
    assert!((sweep.twist.unwrap().value - 90f64.to_radians()).abs() < 1e-12);
    assert!(matches!(&sweep.operation, Operation::Join(_)));
    assert_eq!(sweep.operation.excluded(), [plate]);
}

/// `sweep` along a helix about the toolbar's Z rises 10 mm a turn (the
/// panel's pitch follows the view) for 5 turns, right-handed; about a
/// round face by key with its values, hand and Flip given.
#[test]
fn sweep_climbs_a_helix() {
    let (mut varde, requests) = with_path();
    let (disc, _) = disc_and_plate(&varde);
    let FeatureKind::Sweep(sweep) = applied_last(
        &mut varde,
        &requests,
        "sweep",
        json!({ "sketch": "Sketch 1", "profiles": [disc], "helix": { "axis": "z" } }),
    ) else {
        panic!("not a sweep");
    };
    let PathRef::Helix(helix) = &sweep.path else {
        panic!("not a helix");
    };
    assert_eq!(helix.axis, AxisRef::Origin(varde_document::Axis3::Z));
    assert_eq!((helix.pitch.value, helix.turns.value), (10.0, 5.0));
    assert!(!helix.left_handed && !helix.flip);
    assert!(sweep.twist.is_none());
    assert_eq!(
        varde.mcp.log.back().unwrap().args,
        "Sketch 1 1 region along a helix"
    );
    back(&mut varde, &requests);

    let hole = face_of_form(&varde, "Body 1", "cylinder");
    let FeatureKind::Sweep(sweep) = applied_last(
        &mut varde,
        &requests,
        "sweep",
        json!({ "sketch": "Sketch 1", "profiles": [disc],
                "helix": { "axis": { "face": hole }, "pitch": "4", "turns": 2,
                           "left_handed": true, "flip": true } }),
    ) else {
        panic!("not a sweep");
    };
    let PathRef::Helix(helix) = &sweep.path else {
        panic!("not a helix");
    };
    assert!(matches!(helix.axis, AxisRef::Face(_)));
    assert_eq!((helix.pitch.value, helix.turns.value), (4.0, 2.0));
    assert!(helix.left_handed && helix.flip);
    back(&mut varde, &requests);

    // Part of a turn, as a number, as the panel's field takes it typed.
    let FeatureKind::Sweep(sweep) = applied_last(
        &mut varde,
        &requests,
        "sweep",
        json!({ "sketch": "Sketch 1", "profiles": [disc],
                "helix": { "axis": "z", "turns": 2.5 } }),
    ) else {
        panic!("not a sweep");
    };
    let PathRef::Helix(helix) = &sweep.path else {
        panic!("not a helix");
    };
    assert_eq!(helix.turns.value, 2.5);
}

/// What the Sweep panel refuses, the call refuses saying why: the
/// profile's own sketch as the path, a curve of a chain named, a path's
/// options with a helix, a flat face as the axis, values past their
/// bounds, edges of two bodies, regions past the sketch's, bodies left
/// out of a new body, a sketch made after the rollback marker.
#[test]
fn sweep_refuses_what_the_panel_would_not_take() {
    let (mut varde, requests) = with_disc();
    let path = [(0.0, 0.0), (0.0, 20.0), (10.0, 40.0)];
    sketched(&mut varde, &requests, OriginPlane::XZ, polyline(&path));
    let (disc, _) = disc_and_plate(&varde);
    let own = curve_at(&varde, "Sketch 1", 0);
    let [first, second] = [0, 1].map(|at| curve_at(&varde, "Sketch 2", at));
    let top = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    let plate_edge = edge_between(&varde, "Body 1", [30.0, 20.0, 0.0], [30.0, 20.0, 10.0]);
    let disc_rim = super::motion::key_of(&varde, "Body 2", "edges", |edge| {
        edge["form"] == json!("circle")
    });
    let along = |path: Value| json!({ "sketch": "Sketch 1", "profiles": [disc], "path": path });
    let helix = |helix: Value| json!({ "sketch": "Sketch 1", "profiles": [disc], "helix": helix });
    let with = |base: Value, extra: Value| {
        let mut base = base;
        for (key, value) in extra.as_object().unwrap() {
            base[key] = value.clone();
        }
        base
    };
    for (arguments, why) in [
        (
            along(json!({ "curves": [{ "sketch": "Sketch 1", "curve": own }] })),
            "sweep: path.curves[0]: The profile's own sketch can't be its path".to_owned(),
        ),
        (
            along(json!({ "curves": [{ "sketch": "Sketch 2", "curve": first },
                                     { "sketch": "Sketch 2", "curve": second }] })),
            "sweep: path.curves[1]: that curve is in the chain of one named before".to_owned(),
        ),
        (
            along(json!({ "curves": [{ "sketch": "Sketch 2", "curve": 9999 }] })),
            "sweep: path.curves[0]: the sketch has no curve 9999".to_owned(),
        ),
        (
            along(json!({ "edges": [plate_edge.clone(), disc_rim] })),
            "sweep: path.edges[1]: A sweep's edges are all on one body".to_owned(),
        ),
        (
            along(json!({ "edges": [top.clone()] })),
            "sweep: path.edges[0]: that key names a face, not an edge".to_owned(),
        ),
        (
            with(helix(json!({ "axis": "z" })), json!({ "twist": "10" })),
            "sweep: twist goes with a path".to_owned(),
        ),
        (
            with(
                helix(json!({ "axis": "z" })),
                json!({ "keep_orientation": true }),
            ),
            "sweep: keep_orientation goes with a path".to_owned(),
        ),
        (
            helix(json!({ "axis": { "face": top.clone() } })),
            "sweep: helix.axis: Only a straight or round edge, or a round face, can be the axis"
                .to_owned(),
        ),
        (
            helix(json!({ "axis": "z", "turns": "0" })),
            "sweep: helix.turns: ".to_owned(),
        ),
        (
            helix(json!({ "axis": "z", "pitch": "-2" })),
            "sweep: helix.pitch: ".to_owned(),
        ),
        (
            with(
                along(json!({ "edges": [plate_edge.clone()] })),
                json!({ "twist": "1000 turn" }),
            ),
            "sweep: twist: ".to_owned(),
        ),
        (
            json!({ "sketch": "Sketch 1", "profiles": [99], "helix": { "axis": "z" } }),
            "sweep: profiles: the sketch has 2 regions: index 99 is past them".to_owned(),
        ),
        (
            with(
                along(json!({ "edges": [plate_edge.clone()] })),
                json!({ "exclude": ["Body 1"] }),
            ),
            "sweep: exclude goes with operation join, cut or intersect".to_owned(),
        ),
    ] {
        wrong_edit(&mut varde, &requests, "sweep", arguments, &why);
    }
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_rollback",
        json!({ "before": "Sketch 2" }),
    );
    edit_data(reply);
    wrong_edit(
        &mut varde,
        &requests,
        "sweep",
        along(json!({ "curves": [{ "sketch": "Sketch 2", "curve": first }] })),
        "\"Sketch 2\" is made after where the sweep goes",
    );
}

/// The example with a square on XY ("Sketch 2", a point on its own
/// beside it), one on XZ ("Sketch 3") and one on YZ ("Sketch 4"), and on
/// YZ a point on its own with two lines end to end ("Sketch 5").
pub(super) fn with_sections() -> (Varde, Lane) {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    for (plane, min, max, lone) in [
        (OriginPlane::XY, (15.0, -5.0), (25.0, 5.0), (-40.0, 30.0)),
        (OriginPlane::XZ, (15.0, 20.0), (25.0, 30.0), (-40.0, 30.0)),
        (OriginPlane::YZ, (-5.0, 40.0), (5.0, 50.0), (-40.0, 30.0)),
    ] {
        sketched(&mut varde, &requests, plane, square(min, max, lone));
    }
    let rail = |sketch: &mut Sketch| {
        polyline(&[(-30.0, 0.0), (-30.0, 20.0), (-20.0, 40.0)])(sketch);
        sketch.add_point(DVec2::new(0.0, 60.0)).unwrap();
    };
    let name = sketched(&mut varde, &requests, OriginPlane::YZ, rail);
    assert_eq!(name, "Sketch 5");
    (varde, requests)
}

/// `loft` through two regions is smooth and open, a new body, each
/// region starting at its corner the panel picks; a start given, a point
/// on its own last, and the order given kept; Ruled, Closed and rails
/// given are taken.
#[test]
fn loft_takes_sections_in_order() {
    let (mut varde, requests) = with_sections();
    let region = |sketch: &str| json!({ "sketch": sketch, "region": 0 });
    let FeatureKind::Loft(loft) = applied_last(
        &mut varde,
        &requests,
        "loft",
        json!({ "sections": [region("Sketch 2"), region("Sketch 3")] }),
    ) else {
        panic!("not a loft");
    };
    assert_eq!(loft.sections.len(), 2);
    assert!(
        (loft.sections.iter())
            .all(|section| matches!(section, Section::Region { start: Some(_), .. }))
    );
    assert_eq!((loft.mode, loft.closed), (LoftMode::Smooth, false));
    assert!(loft.rails.is_empty());
    assert!(matches!(loft.operation, Operation::NewBody(_)));
    assert_eq!(varde.mcp.log.back().unwrap().args, "2 sections");
    back(&mut varde, &requests);

    let corner = point_at(&varde, "Sketch 2", (25.0, 5.0));
    let apex = point_at(&varde, "Sketch 5", (0.0, 60.0));
    let FeatureKind::Loft(loft) = applied_last(
        &mut varde,
        &requests,
        "loft",
        json!({ "sections": [
                    { "sketch": "Sketch 5", "point": apex },
                    { "sketch": "Sketch 3", "region": 0 },
                    { "sketch": "Sketch 2", "region": ["20", "0"], "start": corner }
                ], "mode": "ruled" }),
    ) else {
        panic!("not a loft");
    };
    let names: Vec<_> = loft.sections.iter().map(Section::sketch).collect();
    let document = varde.screen.doc().unwrap().editor.document();
    let id = |name: &str| {
        (document.features().iter())
            .find(|f| f.name == name)
            .unwrap()
            .id
    };
    assert_eq!(names, [id("Sketch 5"), id("Sketch 3"), id("Sketch 2")]);
    assert!(matches!(loft.sections[0], Section::Point { .. }));
    assert!(
        matches!(loft.sections[2], Section::Region { start: Some(start), .. }
        if start.get() == corner)
    );
    assert_eq!(loft.mode, LoftMode::Ruled);
    back(&mut varde, &requests);

    let rail = curve_at(&varde, "Sketch 5", 0);
    let FeatureKind::Loft(loft) = applied_last(
        &mut varde,
        &requests,
        "loft",
        json!({ "sections": [region("Sketch 2"), region("Sketch 3")],
                "rails": [{ "sketch": "Sketch 5", "curve": rail }] }),
    ) else {
        panic!("not a loft");
    };
    // The chain the curve is in: both lines.
    assert!(matches!(&loft.rails[..], [rail] if rail.curves.len() == 2));
    assert_eq!(varde.mcp.log.back().unwrap().args, "2 sections, 1 rail");
    back(&mut varde, &requests);

    let FeatureKind::Loft(loft) = applied_last(
        &mut varde,
        &requests,
        "loft",
        json!({ "sections": [region("Sketch 2"), region("Sketch 3"), region("Sketch 4")],
                "closed": true }),
    ) else {
        panic!("not a loft");
    };
    assert!(loft.closed);
    assert_eq!(loft.sections.len(), 3);
}

/// What the Loft panel refuses, the call refuses saying why: one section
/// alone, a section named twice, a region with holes, a point in the
/// middle (the panel's foot), a point as a section between points, a
/// start off the region's corners, a point or curve not in the sketch,
/// rails with Closed, a closed line as a rail, a rail named twice.
#[test]
fn loft_refuses_what_the_panel_would_not_take() {
    let (mut varde, requests) = with_sections();
    let (_, plate) = disc_and_plate(&varde);
    let region = |sketch: &str| json!({ "sketch": sketch, "region": 0 });
    let apex = point_at(&varde, "Sketch 5", (0.0, 60.0));
    let lone = point_at(&varde, "Sketch 2", (-40.0, 30.0));
    let rail = curve_at(&varde, "Sketch 5", 0);
    let [first, second] = [0, 1].map(|at| curve_at(&varde, "Sketch 5", at));
    let edge_of_square = curve_at(&varde, "Sketch 2", 0);
    let point = |sketch: &str, id: u32| json!({ "sketch": sketch, "point": id });
    for (arguments, why) in [
        (
            json!({ "sections": [region("Sketch 2")] }),
            "loft: pick the next section".to_owned(),
        ),
        (
            json!({ "sections": [region("Sketch 2"), region("Sketch 2")] }),
            "loft: sections[1]: that section is named twice".to_owned(),
        ),
        (
            json!({ "sections": [{ "sketch": "Sketch 1", "region": plate }, region("Sketch 2")] }),
            "loft: sections[0]: A section is one loop: pick a region without holes".to_owned(),
        ),
        (
            json!({ "sections": [
                region("Sketch 2"), point("Sketch 5", apex), region("Sketch 3")
            ] }),
            "loft: its section 2 is a point, which only the first or last may be".to_owned(),
        ),
        (
            json!({ "sections": [
                point("Sketch 5", apex), point("Sketch 2", lone), region("Sketch 3")
            ] }),
            "loft: sections[1]: A loft takes a point only as its first or last section".to_owned(),
        ),
        (
            json!({ "sections": [{ "sketch": "Sketch 2", "region": 0, "start": lone },
                                 region("Sketch 3")] }),
            format!("loft: sections[0].start: point {lone} isn't at one of the region's corners"),
        ),
        (
            json!({ "sections": [region("Sketch 2"), point("Sketch 5", 9999)] }),
            "loft: sections[1].point: the sketch has no point 9999".to_owned(),
        ),
        (
            json!({ "sections": [{ "sketch": "Sketch 2", "region": 5 }, region("Sketch 3")] }),
            "loft: sections[0].region: the sketch has 1 region: index 5 is past them".to_owned(),
        ),
        (
            json!({ "sections": [region("Sketch 2"), region("Sketch 3"), region("Sketch 4")],
                    "closed": true, "rails": [{ "sketch": "Sketch 5", "curve": rail }] }),
            "loft: rails[0]: A closed loft takes no rails".to_owned(),
        ),
        (
            json!({ "sections": [region("Sketch 2"), region("Sketch 3")],
                    "rails": [{ "sketch": "Sketch 2", "curve": edge_of_square }] }),
            "loft: rails[0]: That line is closed".to_owned(),
        ),
        (
            json!({ "sections": [region("Sketch 2"), region("Sketch 3")],
                    "rails": [{ "sketch": "Sketch 5", "curve": first },
                              { "sketch": "Sketch 5", "curve": second }] }),
            "loft: rails[1]: that curve is in a rail named before".to_owned(),
        ),
        (
            json!({ "sections": [region("Sketch 2"), region("Sketch 3")],
                    "rails": [{ "sketch": "Sketch 5", "curve": 9999 }] }),
            "loft: rails[0]: the sketch has no curve 9999".to_owned(),
        ),
        (
            json!({ "sections": [region("Sketch 2"), region("Sketch 9")] }),
            "no feature is named \"Sketch 9\"".to_owned(),
        ),
    ] {
        wrong_edit(&mut varde, &requests, "loft", arguments, &why);
    }
}
