//! `sketch` through `Varde::update` on the example: its points, curves
//! and regions, its degrees of freedom from the solve lane's client
//! analysis, which it waits for (after the model), the regions' indices
//! those `extrude` takes, paging, and its ends: answered beside the
//! user's own analysis, timed out, failed, without a lane.

use varde_solve::{Request as SolveRequest, Tag};

use super::*;
use crate::tests::SolveLane;

/// `sketch` of `name` with `arguments` besides.
fn sketch_call(name: &str, mut arguments: Value) -> Call {
    arguments["name"] = json!(name);
    tool_call("sketch", arguments)
}

/// The example listening in "a session", its solve lane deferred.
fn with_solver() -> (Varde, Lane, SolveLane) {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let lane = SolveLane::connect(varde.screen.doc_mut().unwrap());
    (varde, requests, lane)
}

/// The client's analyses waiting on the lane, by id.
fn queries(lane: &SolveLane) -> Vec<u64> {
    (lane.waiting().iter())
        .filter_map(|request| match request.tag() {
            Tag::Query(id) => Some(id),
            _ => None,
        })
        .collect()
}

/// `sketch` waits for the solve lane's analysis, then lists the
/// example's sketch: its points, curves, regions, plane and degrees of
/// freedom, its row saying so.
#[test]
fn sketch_lists_the_example_with_its_degrees_of_freedom() {
    let (mut varde, _requests, mut lane) = with_solver();
    let reply = calling(
        &mut varde,
        "a session",
        1,
        sketch_call("Sketch 1", json!({})),
    );
    assert!(reply.try_recv().is_err());
    assert_eq!(varde.mcp.log[0].state, McpCallState::Waiting);
    assert_eq!(queries(&lane).len(), 1);
    solve(&mut varde, &mut lane);
    let data = edit_data(reply.try_recv().unwrap());
    assert_eq!(data["name"], json!("Sketch 1"));
    assert_eq!(data["units"], json!("mm"));
    assert!(data["plane"].is_string(), "{}", data["plane"]);
    // A rectangle's four corners and lines, a circle and its centre.
    assert_eq!(data["points"]["total"], json!(5));
    assert_eq!(data["curves"]["total"], json!(5));
    assert_eq!(data["constraints"]["total"], json!(0));
    assert_eq!(data["dimensions"]["total"], json!(0));
    let kinds: Vec<&str> = (data["curves"]["items"].as_array().unwrap().iter())
        .map(|curve| curve["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["line", "line", "line", "line", "circle"]);
    let circle = &data["curves"]["items"][4];
    assert!(
        circle["radius"].as_str().unwrap().ends_with("mm"),
        "{circle}"
    );
    let centre = (data["points"]["items"].as_array().unwrap().iter())
        .find(|point| point["id"] == circle["center"])
        .unwrap();
    assert_eq!(centre["at"], json!([0.0, 0.0]));
    // Unconstrained: two per point and the circle's radius.
    assert_eq!(data["degrees_of_freedom"], json!(11));
    assert_eq!(data["solved"], json!(true));
    let regions = data["regions"]["items"].as_array().unwrap();
    assert_eq!(regions.len(), 2);
    for region in regions {
        assert!(
            region["area"].as_str().unwrap().ends_with("mm²"),
            "{region}"
        );
        assert_eq!(region["inside"].as_array().unwrap().len(), 2);
    }
    let log = &varde.mcp.log[0];
    assert_eq!(log.state, McpCallState::Answered);
    assert_eq!(log.args, "Sketch 1");
    assert_eq!(
        log.result.as_deref(),
        Some("5 points · 5 curves · 2 regions · 11 degrees of freedom")
    );
    assert!(!varde.screen.doc().unwrap().mcp_waiting());
}

/// The regions are listed in the order `extrude` takes their indices:
/// the plate's index from `sketch`, its point inside and its curves
/// pick the plate there.
#[test]
fn a_regions_index_from_sketch_picks_it_in_extrude() {
    let (mut varde, requests, mut lane) = with_solver();
    let reply = calling(
        &mut varde,
        "a session",
        1,
        sketch_call("Sketch 1", json!({})),
    );
    solve(&mut varde, &mut lane);
    let data = edit_data(reply.try_recv().unwrap());
    let regions = data["regions"]["items"].as_array().unwrap();
    let plate = (regions.iter())
        .find(|region| region["holes"].as_array().unwrap().len() == 1)
        .unwrap();
    let index = usize::try_from(plate["index"].as_u64().unwrap()).unwrap();
    assert_eq!(index, disc_and_plate(&varde).1);
    let profiles = profiles_of(&varde, "Sketch 1");
    // Its outer loop is the rectangle's four lines, its hole the circle.
    let ids = |value: &Value| -> Vec<u64> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_u64().unwrap())
            .collect()
    };
    let lines: Vec<u64> = (data["curves"]["items"].as_array().unwrap()[..4].iter())
        .map(|curve| curve["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids(&plate["outer"]), lines);
    let inside = plate["inside"].as_array().unwrap();
    let at = glam::DVec2::new(inside[0].as_f64().unwrap(), inside[1].as_f64().unwrap());
    assert_eq!(profiles.region_at(at), Some(index));
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "extrude",
        json!({ "sketch": "Sketch 1", "profiles": [index] }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let extrude = extrude_of(&feature_at(&varde, 2));
    assert_eq!(extrude.regions, vec![profiles.reference(index).unwrap()]);
}

/// The client's analysis queues beside the user's: entering the sketch
/// asks for theirs, the call for its own, neither replacing the other,
/// and each is answered.
#[test]
fn the_clients_analysis_and_the_users_are_both_answered() {
    let (mut varde, _requests, mut lane) = with_solver();
    let id = varde.screen.doc().unwrap().editor.document().features()[0].id;
    ui(
        &mut varde,
        varde_view::Message::Look(varde_view::Look::EditFeature(id)),
    );
    let reply = calling(
        &mut varde,
        "a session",
        1,
        sketch_call("Sketch 1", json!({})),
    );
    let waiting = lane.waiting();
    assert!(
        (waiting.iter()).any(|request| matches!(request, SolveRequest::Analyse { .. })),
        "{waiting:?}"
    );
    assert_eq!(queries(&lane).len(), 1);
    solve(&mut varde, &mut lane);
    let data = edit_data(reply.try_recv().unwrap());
    assert_eq!(data["degrees_of_freedom"], json!(11));
    let doc = varde.screen.doc().unwrap();
    let session = doc.mode.sketch().unwrap();
    assert_eq!(
        session.analysis(doc.editor.revision()).map(|a| a.freedom),
        Some(11)
    );
}

/// A call coming while the model trails the design waits for it first,
/// then for the analysis.
#[test]
fn sketch_waits_for_the_model_then_the_analysis() {
    let (mut varde, requests, mut lane) = with_solver();
    let doc = varde.screen.doc_mut().unwrap();
    let plane = varde_document::Plane::Origin(varde_document::OriginPlane::XZ);
    doc.apply(doc.editor.document().add_sketch(plane));
    doc.sync();
    let reply = calling(
        &mut varde,
        "a session",
        1,
        sketch_call("Sketch 2", json!({})),
    );
    assert!(queries(&lane).is_empty());
    regenerate(&mut varde, &requests);
    assert_eq!(queries(&lane).len(), 1);
    assert!(reply.try_recv().is_err());
    solve(&mut varde, &mut lane);
    let data = edit_data(reply.try_recv().unwrap());
    assert_eq!(data["points"]["total"], json!(0));
    assert_eq!(data["regions"]["total"], json!(0));
    assert_eq!(data["degrees_of_freedom"], json!(0));
    assert_eq!(
        varde.mcp.log[0].result.as_deref(),
        Some("0 points · 0 curves · 0 regions · fully constrained")
    );
}

/// Each listing is paged alone; a page at the default limit fits the
/// cap.
#[test]
fn sketch_pages_each_listing() {
    let (mut varde, _requests, mut lane) = with_solver();
    let call = sketch_call("Sketch 1", json!({ "offset": 1, "limit": 2 }));
    let reply = calling(&mut varde, "a session", 1, call);
    solve(&mut varde, &mut lane);
    let data = edit_data(reply.try_recv().unwrap());
    assert_eq!(data["points"]["items"].as_array().unwrap().len(), 2);
    assert_eq!(data["points"]["left_out"], json!(3));
    assert_eq!(data["regions"]["items"][0]["index"], json!(1));
    assert_eq!(data["regions"]["left_out"], json!(1));
    assert_eq!(varde.mcp.log[0].args, "Sketch 1, from 1, 2 at most");
}

/// A sketch of many items at the default limit fits the cap.
#[test]
fn a_page_of_a_big_sketch_fits_the_cap() {
    let (mut varde, requests, mut lane) = with_solver();
    let doc = varde.screen.doc_mut().unwrap();
    let plane = varde_document::Plane::Origin(varde_document::OriginPlane::XY);
    doc.apply(doc.editor.document().add_sketch(plane));
    let feature = doc.editor.document().features().last().unwrap().id;
    let mut drawn = varde_sketch::Sketch::default();
    let design = doc.editor.document().design();
    for k in 0..40 {
        let x = f64::from(k) * 30.0 - 600.0;
        let corners = [(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)]
            .map(|(dx, dy)| drawn.add_point(glam::DVec2::new(x + dx, dy)).unwrap());
        let mut lines = Vec::new();
        for (i, &start) in corners.iter().enumerate() {
            let end = corners[(i + 1) % 4];
            let line = varde_sketch::Curve::Line { start, end };
            lines.push(drawn.add_curve(line, false).unwrap());
        }
        for &line in &lines[..2] {
            let constraint = match lines.iter().position(|&l| l == line) {
                Some(0) => varde_sketch::Constraint::Horizontal(line),
                _ => varde_sketch::Constraint::Vertical(line),
            };
            drawn.add_constraint(constraint).unwrap();
        }
        let ask = varde_sketch::Measure::Length(lines[0]).ask(&design);
        let value = varde_expr::Value::new("20 mm", &ask).unwrap();
        drawn
            .add_dimension(varde_sketch::Dimension {
                measure: varde_sketch::Measure::Length(lines[0]),
                value,
                driving: true,
                label: glam::DVec2::new(x + 10.0, -5.0),
                side: varde_sketch::Side::Positive,
            })
            .unwrap();
    }
    doc.apply(Command::SetSketch {
        feature,
        sketch: Box::new(drawn),
    });
    doc.sync();
    regenerate(&mut varde, &requests);
    let name = feature_at(&varde, 2).name;
    let reply = calling(&mut varde, "a session", 1, sketch_call(&name, json!({})));
    solve(&mut varde, &mut lane);
    let reply = reply.try_recv().unwrap();
    assert!(!reply.is_error, "{}", reply.text);
    let data = reply.structured.as_ref().unwrap();
    for listing in ["points", "curves", "constraints", "dimensions", "regions"] {
        assert_eq!(
            data[listing]["items"].as_array().unwrap().len(),
            varde_mcp::tools::SKETCH_LIMIT,
            "{listing}"
        );
    }
    assert!(
        reply.text.len() <= ToolReply::CAP,
        "{} bytes",
        reply.text.len()
    );
}

/// A region whose loops have many curves, and many holes, still fits
/// the cap a page of one: each loop's curves and the holes are listed
/// so far, with how many more.
#[test]
fn a_region_of_many_curves_and_holes_fits_the_cap() {
    let (mut varde, requests, mut lane) = with_solver();
    let doc = varde.screen.doc_mut().unwrap();
    let plane = varde_document::Plane::Origin(varde_document::OriginPlane::XY);
    doc.apply(doc.editor.document().add_sketch(plane));
    let feature = doc.editor.document().features().last().unwrap().id;
    let mut drawn = varde_sketch::Sketch::default();
    let sides = 3000;
    let corners: Vec<_> = (0..sides)
        .map(|k| {
            let angle = std::f64::consts::TAU * f64::from(k) / f64::from(sides);
            let at = glam::DVec2::new(angle.cos(), angle.sin()) * 900.0;
            drawn.add_point(at).unwrap()
        })
        .collect();
    for (i, &start) in corners.iter().enumerate() {
        let end = corners[(i + 1) % corners.len()];
        (drawn.add_curve(varde_sketch::Curve::Line { start, end }, false)).unwrap();
    }
    for k in 0..300 {
        let center = glam::DVec2::new(
            f64::from(k % 20) * 40.0 - 400.0,
            f64::from(k / 20) * 40.0 - 300.0,
        );
        let center = drawn.add_point(center).unwrap();
        let circle = varde_sketch::Curve::Circle {
            center,
            radius: 5.0,
        };
        drawn.add_curve(circle, false).unwrap();
    }
    let profiles = drawn.profiles().unwrap();
    let big = (profiles.regions.iter())
        .position(|region| region.holes.len() == 300)
        .unwrap();
    doc.apply(Command::SetSketch {
        feature,
        sketch: Box::new(drawn),
    });
    doc.sync();
    regenerate(&mut varde, &requests);
    let name = feature_at(&varde, 2).name;
    let call = sketch_call(&name, json!({ "offset": big, "limit": 1 }));
    let reply = calling(&mut varde, "a session", 1, call);
    solve(&mut varde, &mut lane);
    let reply = reply.try_recv().unwrap();
    assert!(!reply.is_error, "{}", reply.text);
    assert!(
        reply.text.len() <= ToolReply::CAP,
        "{} bytes",
        reply.text.len()
    );
    let data = reply.structured.as_ref().unwrap();
    let region = &data["regions"]["items"][0];
    assert_eq!(data["regions"]["total"], json!(301));
    assert_eq!(region["outer_left_out"], json!(sides as usize - 50));
    assert_eq!(region["holes"].as_array().unwrap().len(), 50);
    assert_eq!(region["holes_left_out"], json!(250));
}

/// A call waiting 30 s for its analysis times out; the analysis coming
/// after is dropped.
#[test]
fn a_sketch_call_waiting_30_s_times_out() {
    let (mut varde, _requests, mut lane) = with_solver();
    let came = iced::time::Instant::now();
    let reply = calling(
        &mut varde,
        "a session",
        1,
        sketch_call("Sketch 1", json!({})),
    );
    let late = came + waiting::BOUND + std::time::Duration::from_secs(1);
    let _ = varde.update(Message::Tick(late));
    assert_eq!(
        reply.try_recv().unwrap(),
        ToolReply::error(waiting::TOOK_TOO_LONG)
    );
    assert_eq!(varde.mcp.log[0].state, McpCallState::Failed);
    solve(&mut varde, &mut lane);
    assert_eq!(varde.mcp.log[0].state, McpCallState::Failed);
    assert!(!varde.screen.doc().unwrap().mcp_waiting());
}

/// A failed analysis fails the call; a lane replaced ends the calls
/// waiting on it; without a lane the call says so; a name not of a
/// sketch is refused.
#[test]
fn sketch_fails_saying_why() {
    let (mut varde, _requests, mut lane) = with_solver();
    let reply = calling(
        &mut varde,
        "a session",
        1,
        sketch_call("Sketch 1", json!({})),
    );
    let id = queries(&lane)[0];
    let doc_id = varde.screen.doc().unwrap().id;
    let failed = varde_solve::Response::Failed {
        tag: Tag::Query(id),
        error: "on purpose".to_owned(),
    };
    let _ = varde.update(Message::Doc(doc_id, crate::message::ForDoc::Solved(failed)));
    let reply = reply.try_recv().unwrap();
    assert!(
        reply.is_error && reply.text.contains("on purpose"),
        "{}",
        reply.text
    );
    let _ = lane.respond();

    let reply = calling(
        &mut varde,
        "a session",
        2,
        sketch_call("Sketch 1", json!({})),
    );
    let _replaced = SolveLane::connect(varde.screen.doc_mut().unwrap());
    let _ = varde.update(Message::Tick(iced::time::Instant::now()));
    assert_eq!(
        reply.try_recv().unwrap(),
        ToolReply::error(waiting::SOLVER_RESTARTED)
    );

    let reply = called(
        &mut varde,
        "a session",
        3,
        sketch_call("Extrude 1", json!({})),
    );
    assert_eq!(
        reply,
        ToolReply::error("\"Extrude 1\" is an extrude, not a sketch")
    );
    let reply = called(&mut varde, "a session", 4, sketch_call("Nope", json!({})));
    assert_eq!(reply, ToolReply::error("no feature is named \"Nope\""));

    let (mut varde, _requests) = listening_with();
    started(&mut varde, "a session", None);
    varde.screen.doc_mut().unwrap().solver = None;
    let reply = called(
        &mut varde,
        "a session",
        1,
        sketch_call("Sketch 1", json!({})),
    );
    assert_eq!(reply, ToolReply::error(waiting::NO_SOLVER));
}
