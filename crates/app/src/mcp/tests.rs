use crate::doc::OperationSlot;
use std::cell::RefCell;
use std::sync::mpsc;
use varde_view::MotionKind;

use serde_json::{Value, json};
use varde_document::{
    Command, Document, EdgeRef, FaceKey, FaceRef, FeatureId, Generation, PartKey,
};
use varde_mcp::guard::Token;
use varde_mcp::rpc::Id;
use varde_mcp::server::Reply;
use varde_mcp::session::SessionId;
use varde_mcp::tools::{Call, Direction, Page, ToolReply};
use varde_regen::{Regenerated, Request, Response, handle};

use super::key::{self, BadKey, Key, Ref};
use super::*;
use crate::doc::DocId;
use crate::tests::settings::set;
use crate::tests::{Deferred, deferred, example};
use crate::{Screen, Varde};

/// An app showing `doc`, its IO lane's requests dropped.
fn showing(doc: Doc) -> Varde {
    let mut varde = Varde::new();
    varde.files.io.ready(Box::new(Deferred(Default::default())));
    varde.show(Screen::Document(Box::new(doc)));
    varde
}

/// An app showing the example, its feed on a deferred regen lane,
/// answered.
fn with_example() -> Varde {
    showing(example().0)
}

/// Answers the requests waiting as `crate::tests::answer` does, each
/// model with `failing` listed as failing.
fn answer_failing(doc: &mut Doc, requests: &RefCell<Vec<Request>>, failing: FeatureId) {
    while !requests.borrow().is_empty() {
        for request in requests.take() {
            let mut response = handle(request);
            if let Response::Regenerated(Regenerated { failed, .. }) = &mut response {
                failed.push(varde_regen::FeatureFailure {
                    feature: failing,
                    message: "the profile is gone".to_owned(),
                    geometry: None,
                });
            }
            doc.computed(response);
        }
    }
}

/// The answer to `call` from `doc`, as the app gives it: a
/// `body_topology` waiting on its edge facts is answered with them
/// asked of `varde_regen::handle` for the design as it is.
fn answer(doc: Option<&Doc>, call: &Call) -> ToolReply {
    let Some(doc) = doc else {
        return ToolReply::error(query::NO_DESIGN);
    };
    // Those that wait, but for `body_topology`, are only checked.
    let waits = |checked: Option<ToolReply>| {
        checked.unwrap_or_else(|| ToolReply::error("internal error: the call waits"))
    };
    let args = match call {
        Call::BodyTopology(args) => args,
        Call::Sketch(args) => return waits(query::sketch(doc, args).err()),
        Call::Measure(args) => return waits(query::measure(doc, args).err()),
        Call::Render(args) => return waits(query::render(doc, args).err()),
        _ => return waiting::answer_now(doc, call),
    };
    let asked = match query::body_topology(doc, args) {
        Ok(asked) => asked,
        Err(reply) => return reply,
    };
    assert_eq!(asked.generation, doc.editor.generation());
    let Response::Answered {
        answer: varde_regen::Answer::Topology { result, .. },
        ..
    } = handle(Request::Query {
        tag: 0,
        generation: asked.generation,
        document: doc.editor.snapshot(),
        until: asked.until,
        query: varde_regen::Query::Topology(asked.body),
    })
    else {
        panic!();
    };
    let facts = result.as_deref().map_err(String::as_str);
    query::listed(doc, args, asked, facts).unwrap()
}

/// The answer to `call` in `varde`, as its data.
fn data(varde: &Varde, call: &Call) -> Value {
    let reply = answer(varde.screen.doc(), call);
    assert!(!reply.is_error, "{}", reply.text);
    // The text is the same JSON.
    let structured = reply.structured.as_ref().unwrap();
    assert_eq!(reply.text, serde_json::to_string(structured).unwrap());
    reply.structured.unwrap()
}

fn feature(name: &str) -> Call {
    Call::Feature {
        name: name.to_owned(),
        page: Page::FEATURE,
    }
}

#[test]
fn every_call_says_no_design_is_open() {
    let mut varde = Varde::new();
    varde.files.io.ready(Box::new(Deferred(Default::default())));
    assert!(varde.screen.doc().is_none());
    let topology = Call::BodyTopology(topology("Body 1"));
    for call in [
        Call::DesignSummary(Page::SUMMARY),
        feature("Extrude 1"),
        topology,
    ] {
        assert_eq!(answer(None, &call), ToolReply::error("no design is open"));
    }
}

#[test]
fn design_summary_lists_the_example() {
    let varde = with_example();
    let doc = varde.screen.doc().unwrap();
    let document = doc.editor.document();
    let [sketch, extrude] = [0, 1].map(|at| document.features()[at].id.get());
    let summary = data(&varde, &Call::DesignSummary(Page::SUMMARY));
    let generation = serde_json::to_value(doc.editor.generation()).unwrap();
    let revision = serde_json::to_value(doc.editor.revision()).unwrap();
    assert_eq!(
        summary,
        json!({
            "units": "mm",
            "tolerance": "1 µm",
            "revision": revision,
            "bodies": {
                "items": [{ "name": "Body 1", "visible": true }],
                "total": 1,
                "left_out": 0,
            },
            "timeline": {
                "items": [
                    { "kind": "Sketch", "name": "Sketch 1", "id": sketch },
                    { "kind": "Extrude", "name": "Extrude 1", "id": extrude },
                ],
                "total": 2,
                "left_out": 0,
            },
            "rollback": null,
            "parameters": { "items": [], "total": 0, "left_out": 0 },
            "model": {
                "generation": generation,
                "current": true,
                "built_up_to": "the end of the Timeline",
            },
        })
    );
}

#[test]
fn design_summary_gives_parameters_rollback_and_a_model_behind() {
    let (mut doc, requests) = example();
    doc.apply(Command::AddParam {
        name: "width".to_owned(),
        text: "2 cm".to_owned(),
    });
    let extrude = doc.editor.document().features()[1].id;
    doc.apply(Command::SetRollback(Some(extrude)));
    doc.sync();
    // The regeneration not answered yet: the model is the one before.
    let behind = doc.feed.committed().unwrap().generation;
    let varde = showing(doc);
    let summary = data(&varde, &Call::DesignSummary(Page::SUMMARY));
    assert_eq!(
        summary["parameters"]["items"],
        json!([{ "name": "width", "expression": "2 cm", "value": "20 mm" }])
    );
    assert_eq!(summary["rollback"], json!("Extrude 1"));
    let model = &summary["model"];
    assert_eq!(model["generation"], serde_json::to_value(behind).unwrap());
    assert_eq!(model["current"], json!(false));
    assert!(model["note"].as_str().unwrap().contains("still running"));
    assert!(!requests.borrow().is_empty());
}

#[test]
fn design_summary_says_what_failed_and_how_far_the_model_got() {
    let (mut doc, requests) = deferred();
    doc.apply(Command::Replace(Box::new(Document::example())));
    let extrude = doc.editor.document().features()[1].id;
    doc.sync();
    answer_failing(&mut doc, &requests, extrude);
    let varde = showing(doc);
    let summary = data(&varde, &Call::DesignSummary(Page::SUMMARY));
    let timeline = summary["timeline"]["items"].as_array().unwrap();
    assert_eq!(timeline[0].get("failure"), None);
    assert_eq!(timeline[1]["failure"], json!("the profile is gone"));
    // As the feature says too.
    let listed = data(&varde, &feature("Extrude 1"));
    assert_eq!(listed["failure"], json!("the profile is gone"));
}

#[test]
fn a_model_rolled_back_says_what_it_left_out() {
    let (mut doc, requests) = example();
    let extrude = doc.editor.document().features()[1].id;
    doc.apply(Command::SetRollback(Some(extrude)));
    doc.sync();
    crate::tests::answer(&mut doc, &requests);
    let varde = showing(doc);
    let summary = data(&varde, &Call::DesignSummary(Page::SUMMARY));
    assert_eq!(summary["model"]["built_up_to"], json!("before Extrude 1"));
    assert_eq!(summary["timeline"]["items"][1]["built"], json!(false));
    assert_eq!(summary["timeline"]["items"][0].get("built"), None);
}

#[test]
fn feature_lists_the_extrude_as_typed() {
    let varde = with_example();
    let listed = data(&varde, &feature("Extrude 1"));
    assert_eq!(listed["kind"], json!("Extrude"));
    assert_eq!(listed["tool"], json!("extrude"));
    assert_eq!(listed["name"], json!("Extrude 1"));
    let plate = disc_and_plate(&varde).1;
    // As the extrude tool takes it, values as the panel shows them.
    assert_eq!(
        listed["settings"],
        json!({
            "sketch": "Sketch 1",
            "profiles": [plate],
            "extent": "one_side",
            "distance": "10 mm",
            "flip": false,
            "taper": "0°",
            "operation": "new_body",
        })
    );
    assert_eq!(listed.get("failure"), None);
}

#[test]
fn feature_sums_a_sketch_up() {
    let varde = with_example();
    let listed = data(&varde, &feature("Sketch 1"));
    assert_eq!(listed.get("tool"), None);
    assert_eq!(
        listed["settings"],
        json!({
            "plane": "xy",
            "points": 5,
            "curves": 5,
            "constraints": 0,
            "dimensions": 0,
            "links": 0,
        })
    );
}

#[test]
fn feature_names_faces_by_key() {
    let (mut doc, _requests) = example();
    let body = doc.editor.document().bodies()[0].id;
    let extrude = doc.editor.document().features()[1].id;
    let face = FaceRef {
        body,
        key: FaceKey {
            feature: extrude.get(),
            part: PartKey::EndCap,
            instance: 0,
        },
        near: glam::DVec3::new(0.0, 15.0, 10.0),
    };
    let plane = varde_document::Plane::Face(face);
    doc.apply(doc.editor.document().add_sketch(plane));
    let id = doc.id;
    let generation = doc.feed.committed().unwrap().generation;
    let varde = showing(doc);
    let listed = data(&varde, &feature("Sketch 2"));
    // A key of the committed model.
    let shown = &listed["settings"]["plane"]["face"];
    let key = key::decode(shown.as_str().unwrap()).unwrap();
    assert_eq!(key, Key::new(id, generation, Ref::Face(face)));
    // A face the committed model doesn't have is said to be unresolved.
    let (mut doc, _requests) = example();
    let gone = FaceRef {
        near: glam::DVec3::new(500.0, 500.0, 500.0),
        ..face
    };
    let gone = FaceRef {
        body: doc.editor.document().bodies()[0].id,
        ..gone
    };
    doc.apply((doc.editor.document()).add_sketch(varde_document::Plane::Face(gone)));
    let varde = showing(doc);
    let listed = data(&varde, &feature("Sketch 2"));
    let why = listed["settings"]["plane"]["unresolved"].as_str().unwrap();
    assert!(why.starts_with("not in the committed model"), "{why}");
}

#[test]
fn an_unknown_feature_is_named() {
    let varde = with_example();
    let reply = answer(varde.screen.doc(), &feature("Extrude 9"));
    assert_eq!(reply, ToolReply::error("no feature is named \"Extrude 9\""));
}

#[test]
fn keys_round_trip_and_are_bounded() {
    let doc = DocId::unique();
    let body = varde_document::Document::example().bodies()[0].id;
    let face_key = |instance| FaceKey {
        feature: 3,
        part: PartKey::Side { curve: 7 },
        instance,
    };
    let near = glam::DVec3::new(1.5, -2.0, 1e9);
    let refs = [
        Ref::Face(FaceRef {
            body,
            key: face_key(0),
            near,
        }),
        Ref::Edge(EdgeRef {
            body,
            faces: [face_key(0), face_key(1)],
            near,
        }),
        Ref::Corner {
            body,
            faces: [face_key(0), face_key(1), face_key(2)],
            point: [f64::MAX, -0.0, 3.25],
        },
    ];
    for item in refs {
        let key = Key::new(doc, Generation::from(u64::MAX), item);
        let text = key.encode();
        assert!(text.len() <= key::MAX_TEXT, "{}", text.len());
        assert!(
            (text.bytes()).all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "{text}"
        );
        let back = key::decode(&text).unwrap();
        assert_eq!(back, key);
        assert!(back.is_of(doc));
        assert!(!back.is_of(DocId::unique()));
        // Cut short, or with more after it, it isn't one.
        assert_eq!(key::decode(&text[..text.len() - 2]), Err(BadKey::Malformed));
        assert_eq!(key::decode(&format!("{text}AA")), Err(BadKey::Malformed));
    }
    assert_eq!(key::decode("not base64!"), Err(BadKey::Malformed));
    assert_eq!(key::decode(""), Err(BadKey::Malformed));
    let long = "A".repeat(key::MAX_TEXT + 1);
    assert_eq!(key::decode(&long), Err(BadKey::Long));
}

#[test]
fn a_call_is_answered_through_update() {
    let mut varde = with_example();
    varde.mcp = serving_on(8000);
    let (sender, receiver) = mpsc::sync_channel(1);
    let event = Event::Call {
        session: SessionId::for_tests("a session"),
        id: Id::Number(1),
        call: Call::DesignSummary(Page::SUMMARY),
        reply: Reply::for_tests(sender, iced::time::Instant::now()),
    };
    let _ = varde.update(Message::Mcp(start(&varde.mcp), event));
    let reply = receiver.try_recv().unwrap();
    assert!(!reply.is_error);
    assert_eq!(
        reply.structured.unwrap()["bodies"]["items"][0]["name"],
        json!("Body 1")
    );
}

fn topology(body: &str) -> varde_mcp::tools::Topology {
    varde_mcp::tools::Topology {
        body: body.to_owned(),
        faces: None,
        edges: None,
        page: varde_mcp::tools::Page::TOPOLOGY,
    }
}

/// `body_topology` called as a client would, its arguments parsed.
fn topology_call(arguments: Value) -> Call {
    let params = json!({ "name": "body_topology", "arguments": arguments });
    varde_mcp::tools::parse_call(Some(&params)).unwrap()
}

/// The keys of a listing's items.
fn keys(listing: &Value) -> Vec<Key> {
    (listing["items"].as_array().unwrap().iter())
        .map(|item| key::decode(item["key"].as_str().unwrap()).unwrap())
        .collect()
}

/// The number at `value`.
fn number(value: &Value) -> f64 {
    value.as_f64().unwrap()
}

#[test]
fn body_topology_lists_the_example_plate() {
    let varde = with_example();
    let doc = varde.screen.doc().unwrap();
    let body = doc.editor.document().bodies()[0].id;
    let listed = data(&varde, &Call::BodyTopology(topology("Body 1")));
    // A plate with a hole: 4 sides, top, bottom and the hole's wall; 12
    // straight edges and the hole's two rims; 8 corners.
    for (list, total) in [("faces", 7), ("edges", 14), ("corners", 8)] {
        assert_eq!(listed[list]["total"], json!(total), "{list}");
        assert_eq!(listed[list]["left_out"], json!(0), "{list}");
        assert_eq!(listed[list]["items"].as_array().unwrap().len(), total);
    }
    assert_eq!(listed["body"], json!("Body 1"));
    assert_eq!(listed["units"], json!("mm"));
    assert!(listed["note"].as_str().unwrap().contains("approximate"));
    assert_eq!(
        listed["model"]["built_up_to"],
        json!("the end of the Timeline")
    );
    for key in [&listed["faces"], &listed["edges"], &listed["corners"]]
        .map(keys)
        .concat()
    {
        assert!(key.is_of(doc.id));
        assert_eq!(key.generation, doc.editor.generation());
    }
    // Each corner's key names its point, on the body.
    for (item, key) in listed["corners"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .zip(keys(&listed["corners"]))
    {
        let Ref::Corner {
            body: of, point, ..
        } = key.item
        else {
            panic!("{key:?}");
        };
        assert_eq!(of, body);
        assert_eq!(json!(point), item["point"]);
        assert_eq!(point[0].abs(), 30.0);
        assert_eq!(point[1].abs(), 20.0);
    }
    let hole = (listed["faces"]["items"].as_array().unwrap().iter())
        .find(|face| face["form"] == json!("cylinder"))
        .unwrap();
    assert_eq!(hole["radius"], json!("8 mm"));
    let rims = (listed["edges"]["items"].as_array().unwrap().iter())
        .filter(|edge| edge["form"] == json!("circle") && edge["closed"] == json!(true))
        .count();
    assert_eq!(rims, 2);
}

#[test]
fn body_topology_finds_the_top_face_by_its_normal() {
    let varde = with_example();
    let doc = varde.screen.doc().unwrap();
    let extrude = doc.editor.document().features()[1].id;
    let listed = data(
        &varde,
        &topology_call(json!({ "body": "Body 1", "faces": { "normal": "+z" } })),
    );
    let faces = &listed["faces"];
    assert_eq!(faces["total"], json!(1));
    let top = &faces["items"][0];
    assert_eq!(top["form"], json!("plane"));
    assert_eq!(top["normal"], json!([0.0, 0.0, 1.0]));
    // 60 × 40 less the hole of radius 8, its tessellation a little less
    // round.
    let area: f64 = top["area"]
        .as_str()
        .unwrap()
        .trim_end_matches(" mm²")
        .parse()
        .unwrap();
    let exact = 2400.0 - std::f64::consts::PI * 64.0;
    assert!((area - exact).abs() < 1.0, "{area}");
    assert_eq!(
        top["bounds"],
        json!({ "min": [-30.0, -20.0, 10.0], "max": [30.0, 20.0, 10.0] })
    );
    // Its key names the extrude's end cap, near a point on it: off the
    // hole, as a centroid wouldn't be.
    let Ref::Face(face) = keys(faces)[0].item else {
        panic!();
    };
    assert_eq!(face.key.feature, extrude.get());
    assert_eq!(face.key.part, PartKey::EndCap);
    assert!((face.near.z - 10.0).abs() < 1e-4, "{}", face.near);
    assert!(face.near.x.abs() <= 30.0 && face.near.y.abs() <= 20.0);
    assert!(face.near.truncate().length() > 8.0, "{}", face.near);
    // Unsigned, it's the bottom too; the edges and corners aren't
    // filtered.
    let either = data(
        &varde,
        &topology_call(json!({ "body": "Body 1", "faces": { "normal": "z" } })),
    );
    assert_eq!(either["faces"]["total"], json!(2));
    assert_eq!(either["edges"]["total"], json!(14));
    assert_eq!(either["corners"]["total"], json!(8));
    // A slanted direction finds none.
    let none = data(
        &varde,
        &topology_call(json!({ "body": "Body 1", "faces": { "normal": [1, 0, 1] } })),
    );
    assert_eq!(none["faces"]["total"], json!(0));
}

#[test]
fn body_topology_filters_edges_by_direction_and_convexity() {
    let varde = with_example();
    let listed = data(
        &varde,
        &topology_call(json!({ "body": "Body 1", "edges": { "direction": "z" } })),
    );
    let edges = &listed["edges"];
    assert_eq!(edges["total"], json!(4));
    let face_keys: Vec<Key> = keys(&listed["faces"]);
    for (item, key) in edges["items"].as_array().unwrap().iter().zip(keys(edges)) {
        assert_eq!(item["form"], json!("line"));
        assert_eq!(item["length"], json!("10 mm"));
        assert_eq!(item["convex"], json!(true));
        let Ref::Edge(edge) = key.item else {
            panic!("{key:?}");
        };
        // Near a point along it, at a corner's x and y.
        assert!((edge.near.x.abs() - 30.0).abs() < 1e-4, "{}", edge.near);
        assert!((edge.near.y.abs() - 20.0).abs() < 1e-4, "{}", edge.near);
        assert!((0.0..=10.0).contains(&edge.near.z));
        // Its faces are the two sides it's between, listed by key.
        let sides: Vec<FaceKey> = (item["faces"].as_array().unwrap().iter())
            .map(|face| {
                let key = key::decode(face.as_str().unwrap()).unwrap();
                assert!(face_keys.contains(&key));
                let Ref::Face(face) = key.item else { panic!() };
                face.key
            })
            .collect();
        let mut sorted = sides.clone();
        sorted.sort_unstable();
        assert_eq!(edge.faces.to_vec(), sorted);
        assert!(
            sides
                .iter()
                .all(|side| matches!(side.part, PartKey::Side { .. }))
        );
    }
    // "+z" takes them either way along too.
    let signed = data(
        &varde,
        &topology_call(json!({ "body": "Body 1", "edges": { "direction": "+z" } })),
    );
    assert_eq!(signed["edges"]["total"], json!(4));
    let convex = data(
        &varde,
        &topology_call(json!({ "body": "Body 1", "edges": { "convex": true } })),
    );
    assert_eq!(convex["edges"]["total"], json!(14));
    let concave = data(
        &varde,
        &topology_call(json!({ "body": "Body 1", "edges": { "convex": false } })),
    );
    assert_eq!(concave["edges"]["total"], json!(0));
    assert_eq!(concave["edges"]["left_out"], json!(0));
}

#[test]
fn body_topology_pages_each_listing() {
    let varde = with_example();
    let all = data(&varde, &Call::BodyTopology(topology("Body 1")));
    let paged = data(
        &varde,
        &topology_call(json!({ "body": "Body 1", "offset": 2, "limit": 3 })),
    );
    for (list, total) in [("faces", 7), ("edges", 14), ("corners", 8)] {
        let items = paged[list]["items"].as_array().unwrap();
        assert_eq!(
            items[..],
            all[list]["items"].as_array().unwrap()[2..5],
            "{list}"
        );
        assert_eq!(paged[list]["total"], json!(total));
        assert_eq!(paged[list]["left_out"], json!(total - 3));
    }
    // The last page is short.
    let last = data(
        &varde,
        &topology_call(json!({ "body": "Body 1", "offset": 6, "limit": 3 })),
    );
    assert_eq!(last["faces"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(last["faces"]["left_out"], json!(6));
    // Past the end: none, said so; and an offset at the bound adds
    // nothing that overflows.
    for offset in [9, usize::MAX] {
        let mut call = topology("Body 1");
        call.page.offset = offset;
        call.page.limit = varde_mcp::tools::MAX_LIMIT;
        let past = data(&varde, &Call::BodyTopology(call));
        assert_eq!(past["corners"]["items"], json!([]));
        assert_eq!(past["corners"]["left_out"], json!(8));
        assert!(
            past["corners"]["note"]
                .as_str()
                .unwrap()
                .contains("past the end")
        );
    }
}

#[test]
fn body_topology_clamps_the_limit() {
    let Call::BodyTopology(clamped) = topology_call(json!({ "body": "Body 1", "limit": 100000 }))
    else {
        panic!();
    };
    assert_eq!(clamped.page.limit, varde_mcp::tools::MAX_LIMIT);
    let varde = with_example();
    let listed = data(&varde, &Call::BodyTopology(clamped));
    assert_eq!(listed["edges"]["left_out"], json!(0));
    assert_eq!(number(&listed["edges"]["total"]), 14.0);
}

#[test]
fn body_topology_names_an_unknown_body() {
    let varde = with_example();
    let reply = answer(varde.screen.doc(), &Call::BodyTopology(topology("Body 9")));
    assert_eq!(reply, ToolReply::error("no body is named \"Body 9\""));
}

#[test]
fn body_topology_says_a_body_not_built() {
    let (mut doc, requests) = example();
    let extrude = doc.editor.document().features()[1].id;
    doc.apply(Command::SetRollback(Some(extrude)));
    doc.sync();
    crate::tests::answer(&mut doc, &requests);
    let varde = showing(doc);
    let reply = answer(varde.screen.doc(), &Call::BodyTopology(topology("Body 1")));
    assert!(reply.is_error);
    assert!(reply.text.contains("isn't in the model"), "{}", reply.text);
}

#[test]
fn body_topology_says_how_far_a_rolled_back_model_got() {
    let (mut doc, requests) = example();
    let plane = varde_document::Plane::Origin(varde_document::OriginPlane::XZ);
    doc.apply(doc.editor.document().add_sketch(plane));
    let sketch = doc.editor.document().features()[2].id;
    doc.apply(Command::SetRollback(Some(sketch)));
    doc.sync();
    crate::tests::answer(&mut doc, &requests);
    let varde = showing(doc);
    let listed = data(&varde, &Call::BodyTopology(topology("Body 1")));
    assert_eq!(listed["model"]["built_up_to"], json!("before Sketch 2"));
    assert!(
        listed["model"]["rolled_back"]
            .as_str()
            .unwrap()
            .contains("aren't built")
    );
    assert_eq!(listed["faces"]["total"], json!(7));
}

#[test]
fn design_summary_pages_a_big_design() {
    let (mut doc, _requests) = example();
    for at in 0..varde_expr::MAX_PARAMS {
        doc.apply(Command::AddParam {
            name: format!("length_{at}"),
            text: format!("{at} mm + 1 mm"),
        });
    }
    assert_eq!(doc.editor.document().params().len(), varde_expr::MAX_PARAMS);
    let varde = showing(doc);
    let reply = answer(varde.screen.doc(), &Call::DesignSummary(Page::SUMMARY));
    assert!(!reply.is_error);
    // Within the cap, so not cut.
    assert!(!reply.result(ToolReply::CAP)["isError"].as_bool().unwrap());
    let parameters = &reply.structured.unwrap()["parameters"];
    assert_eq!(parameters["total"], json!(varde_expr::MAX_PARAMS));
    assert_eq!(
        parameters["left_out"],
        json!(varde_expr::MAX_PARAMS - varde_mcp::tools::SUMMARY_LIMIT)
    );
    // The last of them, paged to.
    let page = Page {
        offset: varde_expr::MAX_PARAMS - 1,
        limit: 10,
    };
    let last = data(&varde, &Call::DesignSummary(page));
    assert_eq!(
        last["parameters"]["items"][0]["name"],
        json!(format!("length_{}", varde_expr::MAX_PARAMS - 1))
    );
}

#[test]
fn a_feature_with_the_longest_name_is_found() {
    let (mut doc, _requests) = example();
    let extrude = doc.editor.document().features()[1].id;
    let long = "e".repeat(varde_document::MAX_NAME_LEN);
    doc.apply(Command::Rename {
        target: varde_document::Named::Feature(extrude),
        name: long.clone(),
    });
    let varde = showing(doc);
    let params = json!({ "name": "feature", "arguments": { "name": long } });
    let call = varde_mcp::tools::parse_call(Some(&params)).unwrap();
    assert_eq!(data(&varde, &call)["name"], json!(long));
}

/// An `mcp` serving on `port`, its token made, started.
fn serving_on(port: u16) -> Mcp {
    let mut mcp = Mcp::default();
    mcp.token_loaded(Token::generate().unwrap(), false, true, 0, None);
    mcp.start(true, port);
    mcp
}

/// The start of the server `mcp` serves with.
fn start(mcp: &Mcp) -> u64 {
    mcp.serve.as_ref().unwrap().start
}

#[test]
fn a_new_server_forgets_the_old_ones_session() {
    let mut mcp = serving_on(8000);
    let old = start(&mcp);
    let started = session::Event::Started {
        session: SessionId::for_tests("the old server's"),
        client: Some("a client".to_owned()),
    };
    mcp.event(old, Event::Session(started), None);
    assert!(mcp.session.is_some());
    // The server restarts on another port: the old server's stream is
    // gone, and no `Ended` comes for its session.
    mcp.configure(true, 8001);
    assert_eq!(mcp.session, None);
    mcp.event(start(&mcp), Event::BindFailed("in use".to_owned()), None);
    assert_eq!(mcp.status, Status::BindFailed("in use".to_owned()));
    // Configured the same again, it isn't restarted.
    let new = start(&mcp);
    mcp.configure(true, 8001);
    assert_eq!(start(&mcp), new);
}

#[test]
fn body_topology_at_its_default_limit_fits_the_cap() {
    // A full page of each listing, of items the size of the plate's
    // largest and with room for longer numbers, isn't cut: a body of a
    // few dozen faces (a plate with a pattern of holes) is answered at
    // the default without paging.
    let varde = with_example();
    let listed = data(&varde, &Call::BodyTopology(topology("Body 1")));
    let mut page = 0;
    for listing in ["faces", "edges", "corners"] {
        let items = listed[listing]["items"].as_array().unwrap();
        let largest = items.iter().map(|item| item.to_string().len()).max();
        // Each point's three numbers as long as `12.345678`.
        page += (largest.unwrap() + 3 * 3 * 9) * varde_mcp::tools::TOPOLOGY_LIMIT;
    }
    assert!(page < ToolReply::CAP, "{page} bytes");
}

/// The settings page's message `page`.
fn page(varde: &mut Varde, page: varde_view::SettingsPage) {
    let _ = varde.update(crate::message::Message::Ui(varde_view::Message::Settings(
        page,
    )));
}

fn io(varde: &mut Varde, response: varde_io::Response) {
    let _ = varde.update(crate::message::Message::Io(response));
}

fn token_answer(token: &Token, regenerated: bool) -> varde_io::Response {
    varde_io::Response::McpToken {
        regenerated,
        result: Ok(token.clone()),
    }
}

/// Where the app serves: the port and token.
fn serving(varde: &Varde) -> Option<(u16, Token)> {
    (varde.mcp.serve.clone()).map(|serve| (serve.port, serve.token))
}

/// The panel's Start or Stop.
fn panel_says(varde: &mut Varde, message: varde_view::McpPanel) {
    ui(varde, varde_view::Message::McpPanel(message));
}

/// The switch turns the feature on, not the server: launched with it on,
/// the server is stopped till Start, on the port set once the token has
/// come; a port outside 1024 to 65535 is ignored, a new one restarts it,
/// Stop stops it, and the feature off stops it, as Reset to defaults
/// does.
#[test]
fn the_switch_port_and_token_say_where_to_serve() {
    use varde_view::{McpPanel, Setting, SettingsPage};
    let (mut varde, requests) = crate::tests::with_files();
    assert!(
        requests
            .borrow()
            .iter()
            .any(|request| matches!(request, varde_io::Request::LoadMcpToken))
    );
    io(
        &mut varde,
        varde_io::Response::SettingsLoaded {
            settings: varde_io::Settings {
                mcp: true,
                mcp_port: varde_io::settings::McpPort::new(8123).unwrap(),
                ..varde_io::Settings::default()
            },
        },
    );
    let token = Token::generate().unwrap();
    io(&mut varde, token_answer(&token, false));
    // Launched with the feature on, it starts stopped.
    assert_eq!(serving(&varde), None);
    assert!(!varde.mcp.running);
    panel_says(&mut varde, McpPanel::Start);
    assert_eq!(serving(&varde), Some((8123, token.clone())));
    panel_says(&mut varde, McpPanel::Stop);
    assert_eq!(serving(&varde), None);
    assert!(varde.options.mcp);
    panel_says(&mut varde, McpPanel::Start);

    requests.take();
    let before = start(&varde.mcp);
    set(&mut varde, Setting::McpPort(9000));
    assert_eq!(serving(&varde), Some((9000, token.clone())));
    assert_ne!(start(&varde.mcp), before);
    assert!(matches!(
        &requests.take()[..],
        [varde_io::Request::WriteSettings { settings }]
            if settings.mcp && settings.mcp_port.get() == 9000
    ));
    for bad in [0, 80, 1023] {
        set(&mut varde, Setting::McpPort(bad));
    }
    assert_eq!(serving(&varde), Some((9000, token.clone())));
    assert!(requests.take().is_empty());

    // A bind failure is kept for the row to show, till the server stops.
    let started = start(&varde.mcp);
    (varde.mcp).event(started, Event::BindFailed("port in use".to_owned()), None);
    assert_eq!(
        varde.mcp.status,
        Status::BindFailed("port in use".to_owned())
    );
    set(&mut varde, Setting::Mcp(false));
    assert_eq!(serving(&varde), None);
    assert_eq!(varde.mcp.status, Status::Off);
    // On again, the server stays stopped till Start.
    set(&mut varde, Setting::Mcp(true));
    assert_eq!(serving(&varde), None);
    panel_says(&mut varde, McpPanel::Start);
    assert_eq!(serving(&varde), Some((9000, token.clone())));

    let changed: Vec<_> = varde_view::changed_settings(varde.options)
        .into_iter()
        .map(|change| change.name)
        .collect();
    assert_eq!(changed, ["MCP", "MCP port"]);
    page(&mut varde, SettingsPage::AskReset);
    page(&mut varde, SettingsPage::Reset);
    assert_eq!(serving(&varde), None);
    assert!(!varde.options.mcp && !varde.mcp.running);
    assert_eq!(varde.options.mcp_port, varde_view::MCP_PORT_DEFAULT);
}

/// Regenerate asks the IO lane for a new token, which the server then
/// serves with; one that couldn't be made leaves the old.
#[test]
fn regenerating_the_token_serves_with_the_new_one() {
    let (mut varde, requests) = crate::tests::with_files();
    io(
        &mut varde,
        varde_io::Response::SettingsLoaded {
            settings: varde_io::Settings {
                mcp: true,
                ..varde_io::Settings::default()
            },
        },
    );
    let old = Token::generate().unwrap();
    io(&mut varde, token_answer(&old, false));
    assert_eq!(serving(&varde), None);
    panel_says(&mut varde, varde_view::McpPanel::Start);
    assert_eq!(serving(&varde), Some((7311, old.clone())));
    requests.take();
    page(&mut varde, varde_view::SettingsPage::RegenerateMcpToken);
    assert!(matches!(
        &requests.take()[..],
        [varde_io::Request::RegenerateMcpToken]
    ));
    io(
        &mut varde,
        varde_io::Response::McpToken {
            regenerated: true,
            result: Err("disk full".to_owned()),
        },
    );
    assert_eq!(serving(&varde), Some((7311, old.clone())));
    let new = Token::generate().unwrap();
    io(&mut varde, token_answer(&new, true));
    assert_eq!(serving(&varde), Some((7311, new)));
}

/// Stopped, a token coming or regenerated leaves the server stopped,
/// and Start before the token comes waits for it: starting, then
/// serving with the token that came last.
#[test]
fn a_token_while_stopped_serves_only_once_started() {
    use varde_view::{McpPanel, McpStatus};
    let (mut varde, _requests) = crate::tests::with_files();
    set(&mut varde, varde_view::Setting::Mcp(true));
    panel_says(&mut varde, McpPanel::Start);
    assert_eq!(serving(&varde), None);
    assert_eq!(varde.mcp_shown().status, McpStatus::Starting);
    panel_says(&mut varde, McpPanel::Stop);
    assert_eq!(varde.mcp_shown().status, McpStatus::Off);
    let old = Token::generate().unwrap();
    io(&mut varde, token_answer(&old, false));
    let new = Token::generate().unwrap();
    io(&mut varde, token_answer(&new, true));
    assert_eq!(serving(&varde), None);
    assert_eq!(varde.mcp_shown().status, McpStatus::Off);
    panel_says(&mut varde, McpPanel::Start);
    assert_eq!(serving(&varde), Some((7311, new)));
}

/// Through the real IO lane, its token kept in a temporary directory:
/// made at start, kept for the next session, replaced on Regenerate.
#[test]
fn the_token_through_the_lane_and_its_file() {
    use iced::futures::StreamExt;
    use iced::futures::executor::block_on;

    let dir = std::env::temp_dir().join(format!("varde-app-mcp-token-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = dir.join("mcp-token.toml");
    let session = |regenerate: bool| {
        let (lane, mut responses) = varde_io::lane::spawn_at(varde_io::Stores {
            mcp_token: Some(store.clone()),
            ..varde_io::Stores::default()
        });
        let mut varde = Varde::new();
        let _ = varde.update(crate::message::Message::IoReady(lane));
        set(&mut varde, varde_view::Setting::Mcp(true));
        panel_says(&mut varde, varde_view::McpPanel::Start);
        if regenerate {
            page(&mut varde, varde_view::SettingsPage::RegenerateMcpToken);
        }
        let mut answers = if regenerate { 2 } else { 1 };
        while answers > 0 {
            let response = block_on(responses.next()).unwrap();
            if matches!(response, varde_io::Response::McpToken { .. }) {
                answers -= 1;
            }
            io(&mut varde, response);
        }
        serving(&varde).unwrap().1
    };
    let in_file = || {
        let text = std::fs::read_to_string(&store).unwrap();
        text.split('"').nth(1).unwrap().to_owned()
    };
    let made = session(false);
    assert_eq!(in_file(), made.to_string());
    assert_eq!(session(false), made);
    let regenerated = session(true);
    assert_ne!(regenerated, made);
    assert_eq!(in_file(), regenerated.to_string());
    assert_eq!(session(false), regenerated);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Events a server had queued as it stopped, or was replaced, come after
/// and are dropped: they don't say the server is in a state it isn't.
#[test]
fn a_stopped_servers_late_events_are_dropped() {
    let mut mcp = serving_on(8000);
    let old = start(&mcp);
    mcp.configure(false, 8000);
    mcp.event(old, Event::BindFailed("in use".to_owned()), None);
    assert_eq!(mcp.status, Status::Off);

    mcp.start(true, 8000);
    mcp.configure(true, 8001);
    let replaced = old + 1;
    assert_ne!(start(&mcp), replaced);
    let started = session::Event::Started {
        session: SessionId::for_tests("the replaced server's"),
        client: None,
    };
    mcp.event(replaced, Event::Session(started), None);
    assert_eq!(mcp.session, None);
    // A call from it is answered, with an error.
    let (sender, receiver) = mpsc::sync_channel(1);
    let call = Event::Call {
        session: SessionId::for_tests("the replaced server's"),
        id: Id::Number(1),
        call: Call::DesignSummary(Page::SUMMARY),
        reply: Reply::for_tests(sender, iced::time::Instant::now()),
    };
    mcp.event(replaced, call, None);
    assert!(receiver.try_recv().unwrap().is_error);
}

/// The port's field sets a port once it's in range, keeps what's typed
/// out of range for the row to say, and a row reset or closing the page
/// drops it; Show shows the token, closing the page hides it again.
#[test]
fn the_port_field_and_the_token_shown() {
    use varde_view::{McpStatus, Setting, SettingsPage};
    let (mut varde, requests) = crate::tests::with_files();
    io(
        &mut varde,
        varde_io::Response::SettingsLoaded {
            settings: varde_io::Settings::default(),
        },
    );
    let token = Token::generate().unwrap();
    io(&mut varde, token_answer(&token, false));
    let _ = varde.update(crate::message::Message::Ui(
        varde_view::Message::ToggleSettings,
    ));
    let type_port = |varde: &mut Varde, typed: &str| {
        page(varde, SettingsPage::TypeMcpPort(typed.to_owned()));
    };

    type_port(&mut varde, "80");
    assert_eq!(varde.options.mcp_port, 7311);
    assert_eq!(varde.mcp_shown().port_typed, Some("80"));
    type_port(&mut varde, "8000");
    assert_eq!(varde.options.mcp_port, 8000);
    assert_eq!(varde.mcp_shown().port_typed, None);
    assert!(requests.take().iter().any(|request| matches!(
        request,
        varde_io::Request::WriteSettings { settings } if settings.mcp_port.get() == 8000
    )));
    type_port(&mut varde, "800");
    page(&mut varde, SettingsPage::Set(Setting::McpPort(7311)));
    assert_eq!(varde.options.mcp_port, 7311);
    assert_eq!(varde.mcp_shown().port_typed, None);

    let shown = varde.mcp_shown();
    assert_eq!(shown.status, McpStatus::Off);
    assert_eq!(shown.token, Some(token.to_string()));
    assert!(!shown.token_shown);
    page(&mut varde, SettingsPage::ShowMcpToken(true));
    assert!(varde.mcp_shown().token_shown);
    type_port(&mut varde, "1");
    let _ = varde.update(crate::message::Message::Ui(
        varde_view::Message::ToggleSettings,
    ));
    let shown = varde.mcp_shown();
    assert!(!shown.token_shown);
    assert_eq!(shown.port_typed, None);

    set(&mut varde, Setting::Mcp(true));
    assert_eq!(varde.mcp_shown().status, McpStatus::Off);
    panel_says(&mut varde, varde_view::McpPanel::Start);
    assert_eq!(varde.mcp_shown().status, McpStatus::Starting);
    (varde.mcp).event(
        start(&varde.mcp),
        Event::BindFailed("in use".to_owned()),
        None,
    );
    assert_eq!(varde.mcp_shown().status, McpStatus::BindFailed("in use"));
}

/// Disconnect ends the session on a real server; the client shown with
/// it goes once its `Ended` comes.
#[test]
fn disconnect_ends_the_session() {
    use std::io::{BufRead, BufReader, Read, Write};

    use iced::futures::executor::block_on_stream;

    let mut varde = Varde::new();
    varde.files.io.ready(Box::new(Deferred(Default::default())));
    let token = Token::generate().unwrap();
    varde.mcp.token_loaded(token.clone(), false, true, 0, None);
    varde.options.mcp = true;
    varde.mcp.start(true, 0);
    let started = start(&varde.mcp);
    let mut events = block_on_stream(varde_mcp::server::start(0, token.clone()));
    let mut next = |varde: &mut Varde| {
        let event = events.next().unwrap();
        let _ = varde.update(crate::message::Message::Mcp(started, event));
    };
    next(&mut varde);
    let varde_view::McpStatus::Listening(port) = varde.mcp_shown().status else {
        panic!("not listening: {:?}", varde.mcp_shown().status);
    };

    let stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2025-06-18", "clientInfo": { "name": "claude-code" } },
    })
    .to_string();
    let head = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    (&stream).write_all(head.as_bytes()).unwrap();
    (&stream).write_all(body.as_bytes()).unwrap();
    let mut reader = BufReader::new(&stream);
    let mut length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap();
        }
        if line.trim().is_empty() {
            break;
        }
    }
    reader.read_exact(&mut vec![0; length]).unwrap();

    next(&mut varde);
    let client = varde.mcp_shown().client.unwrap();
    assert_eq!(client.name, Some("claude-code"));
    assert_eq!(client.since, "just now");
    page(&mut varde, varde_view::SettingsPage::DisconnectMcp);
    next(&mut varde);
    assert_eq!(varde.mcp_shown().client, None);
    assert_eq!(varde.mcp.session, None);
}

/// A server listening, restarted on another port, isn't shown listening
/// on the old one while the new one starts.
#[test]
fn a_restart_drops_the_old_address() {
    let mut mcp = serving_on(8000);
    mcp.status = Status::Listening(([127, 0, 0, 1], 8000).into());
    mcp.configure(true, 8001);
    assert_eq!(mcp.status, Status::Off);
    assert_eq!(
        mcp.shown(varde_io::UnixSeconds(0)).status,
        varde_view::McpStatus::Starting
    );
}

/// The example shown, serving on 7311 and listening, switched on.
fn listening() -> Varde {
    listening_with().0
}

/// The requests a design's deferred regen lane holds.
type Lane = std::rc::Rc<RefCell<Vec<Request>>>;

/// [`listening`], with the requests its design's regen lane holds.
fn listening_with() -> (Varde, Lane) {
    let (doc, requests) = example();
    let mut varde = showing(doc);
    varde.options.mcp = true;
    varde.options.mcp_port = 7311;
    varde.mcp = serving_on(7311);
    varde.mcp.status = Status::Listening("127.0.0.1:7311".parse().unwrap());
    (varde, requests)
}

/// Answers the requests waiting that `which` picks, through
/// `varde_regen::handle` and `Varde::update`, in order, and those
/// answering them sends, until none it picks is left.
fn respond(varde: &mut Varde, requests: &Lane, which: impl Fn(&Request) -> bool) {
    loop {
        let (picked, kept) = requests.take().into_iter().partition(|r| which(r));
        *requests.borrow_mut() = kept;
        let picked: Vec<Request> = picked;
        if picked.is_empty() {
            return;
        }
        for request in picked {
            let id = varde.screen.doc().unwrap().id;
            let message = crate::message::ForDoc::Computed(handle(request));
            let _ = varde.update(Message::Doc(id, message));
        }
    }
}

fn is_topology(request: &Request) -> bool {
    matches!(
        request,
        Request::Query {
            query: varde_regen::Query::Topology(_),
            ..
        }
    )
}

/// Calls `call` in `session`, as request `id`: where its reply comes.
fn calling(varde: &mut Varde, session: &str, id: i64, call: Call) -> mpsc::Receiver<ToolReply> {
    let (sender, receiver) = mpsc::sync_channel(1);
    let event = Event::Call {
        session: SessionId::for_tests(session),
        id: Id::Number(id),
        call,
        reply: Reply::for_tests(sender, iced::time::Instant::now()),
    };
    feed(varde, event);
    receiver
}

/// Feeds `event` to `varde` from the server it serves with.
fn feed(varde: &mut Varde, event: Event) {
    let _ = varde.update(Message::Mcp(start(&varde.mcp), event));
}

fn started(varde: &mut Varde, session: &str, client: Option<&str>) {
    let event = session::Event::Started {
        session: SessionId::for_tests(session),
        client: client.map(str::to_owned),
    };
    feed(varde, Event::Session(event));
}

/// Calls `call` in `session`, as request `id`: its reply, answered at
/// once.
fn called(varde: &mut Varde, session: &str, id: i64, call: Call) -> ToolReply {
    calling(varde, session, id, call).try_recv().unwrap()
}

/// The texts the app shows, in a window 1400 × 800.
fn on_screen(varde: &Varde) -> Vec<String> {
    let mut renderer = varde_view::probe::renderer();
    let view = varde.screen.doc().unwrap().view_with(
        false,
        varde_view::Mode::Light,
        varde.options,
        varde.files.offers(),
        None,
        varde.mcp_panel_shown(),
    );
    let mut ui = crate::tests::shown(view, iced::Size::new(1400.0, 800.0), &mut renderer);
    (crate::tests::texts(&mut ui, &renderer).into_iter())
        .map(|shown| shown.text)
        .collect()
}

fn ui(varde: &mut Varde, message: varde_view::Message) {
    let _ = varde.update(Message::Ui(message));
}

/// The client the status bar's chip names, while the feature is on: the
/// name it gave, `None` shown as "A client".
fn chip<'a>(shown: &varde_view::McpShown<'a>) -> Option<Option<&'a str>> {
    shown.client.filter(|_| shown.on)
}

/// The status bar's chip shows only while a client is connected, the
/// toolbar's toggle only while the feature is on, and the chip opens the
/// panel.
#[test]
fn the_chip_shows_with_a_client_and_the_toggle_while_on() {
    use varde_view::{McpPanel, Setting, SettingsPage};
    let varde = with_example();
    let shown = varde.mcp_panel_shown().unwrap();
    assert!(!shown.on);
    assert_eq!(chip(&shown), None);

    let mut varde = listening();
    let shown = varde.mcp_panel_shown().unwrap();
    assert!(shown.on && !shown.open);
    assert_eq!(chip(&shown), None);
    assert!(!on_screen(&varde).iter().any(|text| text.starts_with("MCP")));

    started(&mut varde, "a session", Some("Claude Code"));
    assert_eq!(
        chip(&varde.mcp_panel_shown().unwrap()),
        Some(Some("Claude Code"))
    );
    assert!(
        on_screen(&varde)
            .iter()
            .any(|text| text == "MCP · Claude Code")
    );
    // The chip's click, as the toggle's, opens the panel.
    ui(&mut varde, varde_view::Message::McpPanel(McpPanel::Toggle));
    assert!(varde.mcp.open);
    assert!(on_screen(&varde).iter().any(|text| text == "Disconnect"));

    // The feature off: the server stopped, no chip, no toggle, no panel.
    ui(
        &mut varde,
        varde_view::Message::Settings(SettingsPage::Set(Setting::Mcp(false))),
    );
    let shown = varde.mcp_panel_shown().unwrap();
    assert_eq!(chip(&shown), None);
    assert!(!shown.on && !shown.open);
    assert!(varde.mcp.serve.is_none() && !varde.mcp.running);
    assert!(!on_screen(&varde).iter().any(|text| text == "Stopped"));
    // The toggle can't open it while off.
    ui(&mut varde, varde_view::Message::McpPanel(McpPanel::Toggle));
    assert!(!varde.mcp.open);
}

/// The feature on shows the toggle, and the panel says the server is
/// stopped, with Start, until it's started.
#[test]
fn the_feature_on_shows_the_toggle_and_a_stopped_panel() {
    use varde_view::{McpPanel, McpStatus, Setting};
    let mut varde = with_example();
    varde
        .mcp
        .token_loaded(Token::generate().unwrap(), false, true, 0, None);
    set(&mut varde, Setting::Mcp(true));
    let shown = varde.mcp_panel_shown().unwrap();
    assert!(shown.on);
    assert_eq!(shown.status, McpStatus::Off);
    assert!(varde.mcp.serve.is_none());
    panel_says(&mut varde, McpPanel::Toggle);
    let texts = on_screen(&varde);
    assert!(texts.iter().any(|text| text == "Stopped"), "{texts:?}");
    assert!(texts.iter().any(|text| text == "Start"), "{texts:?}");
    panel_says(&mut varde, McpPanel::Start);
    assert!(varde.mcp.serve.is_some());
    assert_eq!(varde.mcp_panel_shown().unwrap().status, McpStatus::Starting);
    assert!(on_screen(&varde).iter().any(|text| text == "Stop"));
}

/// Each call is a row: in the user's words, its arguments as typed, its
/// state and what came of it.
#[test]
fn the_panel_lists_the_calls() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", Some("Claude Code"));
    ui(
        &mut varde,
        varde_view::Message::McpPanel(varde_view::McpPanel::Toggle),
    );
    assert!(
        !called(
            &mut varde,
            "a session",
            1,
            Call::DesignSummary(Page::SUMMARY)
        )
        .is_error
    );
    assert!(!called(&mut varde, "a session", 2, feature("Extrude 1")).is_error);
    assert!(called(&mut varde, "a session", 3, feature("No such")).is_error);
    let mut topology = topology("Body 1");
    topology.faces = Some(varde_mcp::tools::FaceFilter {
        normal: Some(Direction {
            vector: [0.0, 0.0, 1.0],
            signed: true,
        }),
    });
    topology.edges = Some(varde_mcp::tools::EdgeFilter {
        direction: None,
        convex: Some(true),
    });
    topology.page.limit = 5;
    let reply = calling(&mut varde, "a session", 4, Call::BodyTopology(topology));
    respond(&mut varde, &requests, is_topology);
    assert!(!reply.try_recv().unwrap().is_error);

    let rows: Vec<_> = (varde.mcp.log.iter())
        .map(|logged| (logged.tool, logged.args.as_str(), logged.state))
        .collect();
    assert_eq!(
        rows,
        [
            (McpTool::DesignSummary, "", McpCallState::Answered),
            (McpTool::Feature, "Extrude 1", McpCallState::Answered),
            (McpTool::Feature, "No such", McpCallState::Failed),
            (
                McpTool::BodyTopology,
                "Body 1, faces facing +z, convex edges, 5 at most",
                McpCallState::Answered
            ),
        ]
    );
    let result = |at: usize| varde.mcp.log[at].result.clone().unwrap();
    assert_eq!(result(0), "1 body · 2 features · 0 parameters");
    assert_eq!(result(1), "Extrude");
    assert!(result(2).contains("No such"), "{}", result(2));
    assert!(result(3).ends_with("corners"), "{}", result(3));

    let texts = on_screen(&varde);
    for text in [
        "Read the design summary",
        "Looked at",
        "Extrude 1",
        "Listed the faces and edges of",
        "Body 1, faces facing +z, convex edges, 5 at most",
        "Just now",
    ] {
        assert!(
            texts.iter().any(|shown| shown == text),
            "no {text:?} in {texts:?}"
        );
    }
}

/// The log keeps the newest `LOG_CAP` calls, counting those let go.
#[test]
fn the_log_is_bounded() {
    let mut varde = listening();
    started(&mut varde, "a session", Some("Claude Code"));
    let extra = 5;
    for id in 0..LOG_CAP + extra {
        let name = format!("Feature {id}");
        called(&mut varde, "a session", id as i64, feature(&name));
    }
    assert_eq!(varde.mcp.log.len(), LOG_CAP);
    assert_eq!(varde.mcp.dropped, extra);
    assert_eq!(varde.mcp.log[0].args, format!("Feature {extra}"));
    let shown = varde.mcp_panel_shown().unwrap();
    assert_eq!(shown.rows.len(), LOG_CAP);
    assert_eq!(shown.dropped, extra);
}

/// The log is the session's and the design's: kept, with a note, as the
/// session ends; cleared as the next starts, the design closes or the
/// server stops.
#[test]
fn the_log_goes_with_the_session_the_design_and_the_server() {
    use varde_view::{McpPanel, McpStatus};
    let mut varde = listening();
    started(&mut varde, "first", Some("Claude Code"));
    called(&mut varde, "first", 1, Call::DesignSummary(Page::SUMMARY));
    let ended = session::Event::Ended {
        session: SessionId::for_tests("first"),
        end: session::End::Deleted,
    };
    feed(&mut varde, Event::Session(ended));
    let shown = varde.mcp_panel_shown().unwrap();
    assert_eq!(shown.rows.len(), 1);
    assert_eq!(shown.gone, Some(Some("Claude Code")));
    assert_eq!(shown.client, None);

    started(&mut varde, "second", Some("Codex"));
    let shown = varde.mcp_panel_shown().unwrap();
    assert!(shown.rows.is_empty() && shown.gone.is_none());

    called(&mut varde, "second", 1, Call::DesignSummary(Page::SUMMARY));
    varde.show(Screen::Document(Box::new(example().0)));
    assert!(varde.mcp.log.is_empty());

    called(&mut varde, "second", 2, Call::DesignSummary(Page::SUMMARY));
    ui(&mut varde, varde_view::Message::McpPanel(McpPanel::Toggle));
    ui(&mut varde, varde_view::Message::McpPanel(McpPanel::Stop));
    assert!(varde.options.mcp);
    let shown = varde.mcp_panel_shown().unwrap();
    assert!(shown.rows.is_empty());
    assert_eq!(shown.status, McpStatus::Off);
    assert!(shown.open && shown.on);
    let texts = on_screen(&varde);
    assert!(texts.iter().any(|text| text == "Stopped"), "{texts:?}");
    assert!(texts.iter().any(|text| text == "Start"), "{texts:?}");

    // Start starts it again, the settings untouched.
    ui(&mut varde, varde_view::Message::McpPanel(McpPanel::Start));
    assert!(varde.options.mcp);
    assert!(varde.mcp.serve.is_some());
}

/// Start after the port couldn't be bound starts the server again.
#[test]
fn start_tries_a_port_not_bound_again() {
    let mut varde = listening();
    feed(&mut varde, Event::BindFailed("address in use".to_owned()));
    let failed = start(&varde.mcp);
    ui(
        &mut varde,
        varde_view::Message::McpPanel(varde_view::McpPanel::Start),
    );
    assert_ne!(start(&varde.mcp), failed);
    assert_eq!(varde.mcp.status, Status::Off);
    // Bound, Start does nothing more.
    varde.mcp.status = Status::Listening("127.0.0.1:7311".parse().unwrap());
    let bound = start(&varde.mcp);
    ui(
        &mut varde,
        varde_view::Message::McpPanel(varde_view::McpPanel::Start),
    );
    assert_eq!(start(&varde.mcp), bound);
}

/// A client's name and the names in its calls come off the network:
/// line breaks, other controls and bidi overrides would break the chip's
/// and the rows' lines or turn the text around them, so they show as
/// spaces or a placeholder; a blank name is no name.
#[test]
fn untrusted_text_is_shown_on_one_line() {
    let mut varde = listening();
    started(&mut varde, "a session", Some("Evil\nClient\u{202E}\u{7}"));
    assert_eq!(
        chip(&varde.mcp_panel_shown().unwrap()),
        Some(Some("Evil Client\u{FFFD}\u{FFFD}"))
    );
    let _ = called(&mut varde, "a session", 1, feature("Ext\r\nrude\u{2066}1"));
    let shown = varde.mcp_panel_shown().unwrap();
    let varde_view::McpRow::Call(call) = &shown.rows[0] else {
        panic!("a call row")
    };
    assert_eq!(call.args, "Ext  rude\u{FFFD}1");
    assert!(!call.result.unwrap().contains(['\n', '\u{2066}']));

    started(&mut varde, "another", Some(" \t "));
    assert_eq!(chip(&varde.mcp_panel_shown().unwrap()), Some(None));
}

/// `body_topology` waits for its edge facts, its row waiting, and is
/// answered as they come.
#[test]
fn body_topology_waits_for_its_edge_facts() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", Some("Claude Code"));
    let reply = calling(
        &mut varde,
        "a session",
        1,
        Call::BodyTopology(topology("Body 1")),
    );
    assert!(reply.try_recv().is_err());
    assert!(varde.screen.doc().unwrap().mcp_waiting());
    assert_eq!(varde.mcp.log[0].state, McpCallState::Waiting);
    let doc = varde.screen.doc().unwrap();
    let generation = doc.editor.generation();
    let body = doc.editor.document().bodies()[0].id;
    let asked: Vec<_> = (requests.borrow().iter())
        .filter_map(|request| match request {
            Request::Query {
                generation,
                until,
                query: varde_regen::Query::Topology(body),
                ..
            } => Some((*generation, *until, *body)),
            _ => None,
        })
        .collect();
    assert_eq!(asked, [(generation, None, body)]);
    respond(&mut varde, &requests, is_topology);
    let reply = reply.try_recv().unwrap();
    assert!(!reply.is_error, "{}", reply.text);
    let listed = reply.structured.unwrap();
    let rims = (listed["edges"]["items"].as_array().unwrap().iter())
        .filter(|edge| edge["form"] == json!("circle"))
        .count();
    assert_eq!(rims, 2);
    assert!(!varde.screen.doc().unwrap().mcp_waiting());
    assert_eq!(varde.mcp.log[0].state, McpCallState::Answered);
    assert!(
        varde.mcp.log[0]
            .result
            .as_ref()
            .unwrap()
            .ends_with("corners")
    );
}

/// A call waiting 30 s from its arrival is answered that the model took
/// too long; an answer coming after is dropped.
#[test]
fn a_call_waiting_30_s_times_out() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let came = iced::time::Instant::now();
    let reply = calling(
        &mut varde,
        "a session",
        1,
        Call::BodyTopology(topology("Body 1")),
    );
    let _ = varde.update(Message::Tick(came));
    assert!(reply.try_recv().is_err());
    let late = came + waiting::BOUND + std::time::Duration::from_secs(1);
    let _ = varde.update(Message::Tick(late));
    assert_eq!(
        reply.try_recv().unwrap(),
        ToolReply::error(waiting::TOOK_TOO_LONG)
    );
    assert!(!varde.screen.doc().unwrap().mcp_waiting());
    assert_eq!(varde.mcp.log[0].state, McpCallState::Failed);
    respond(&mut varde, &requests, is_topology);
    assert_eq!(varde.mcp.log[0].state, McpCallState::Failed);
}

/// Calls waiting fail as the design closes, is cancelled, the session
/// ends or the server stops.
#[test]
fn calls_waiting_fail_as_they_end() {
    let (mut varde, _requests) = listening_with();
    started(&mut varde, "a session", None);
    let call = || Call::BodyTopology(topology("Body 1"));
    let first = calling(&mut varde, "a session", 1, call());
    let second = calling(&mut varde, "a session", 2, call());
    let cancelled = session::Event::Cancelled {
        session: SessionId::for_tests("a session"),
        request: Id::Number(1),
    };
    feed(&mut varde, Event::Session(cancelled));
    assert_eq!(
        first.try_recv().unwrap(),
        ToolReply::error("The call was cancelled")
    );
    assert!(second.try_recv().is_err());
    assert_eq!(varde.mcp.log[0].state, McpCallState::Failed);
    assert_eq!(varde.mcp.log[1].state, McpCallState::Waiting);
    let ended = session::Event::Ended {
        session: SessionId::for_tests("a session"),
        end: session::End::Deleted,
    };
    feed(&mut varde, Event::Session(ended));
    assert_eq!(
        second.try_recv().unwrap(),
        ToolReply::error("The session ended")
    );
    assert_eq!(varde.mcp.log[1].state, McpCallState::Failed);

    started(&mut varde, "next", None);
    let closed = calling(&mut varde, "next", 1, call());
    varde.show(Screen::Document(Box::new(example().0)));
    assert_eq!(
        closed.try_recv().unwrap(),
        ToolReply::error(waiting::DESIGN_CLOSED)
    );

    started(&mut varde, "last", None);
    let stopped = calling(&mut varde, "last", 1, call());
    ui(
        &mut varde,
        varde_view::Message::McpPanel(varde_view::McpPanel::Stop),
    );
    assert_eq!(
        stopped.try_recv().unwrap(),
        ToolReply::error("The server stopped")
    );
    assert!(!varde.screen.doc().unwrap().mcp_waiting());
}

/// Regenerating the token ends the session at once: its calls waiting
/// are answered as at a session's end, not as the server stopping, and
/// the panel still lists them, its client shown gone.
#[test]
fn regenerating_the_token_ends_the_session() {
    let (mut varde, _requests) = listening_with();
    started(&mut varde, "a session", Some("Claude Code"));
    let waiting = calling(
        &mut varde,
        "a session",
        1,
        Call::BodyTopology(topology("Body 1")),
    );
    let old = start(&varde.mcp);
    io(&mut varde, token_answer(&Token::generate().unwrap(), true));
    assert_ne!(start(&varde.mcp), old);
    assert_eq!(
        waiting.try_recv().unwrap(),
        ToolReply::error("The session ended")
    );
    assert_eq!(varde.mcp.session, None);
    assert_eq!(varde.mcp.gone, Some(Some("Claude Code".to_owned())));
    assert_eq!(varde.mcp.log.len(), 1);
    assert_eq!(varde.mcp.log[0].state, McpCallState::Failed);
}

/// Moves the committed model of `varde`'s design on a generation.
fn move_on(varde: &mut Varde, requests: &Lane, name: &str) {
    let doc = varde.screen.doc_mut().unwrap();
    doc.apply(Command::AddParam {
        name: name.to_owned(),
        text: "1 mm".to_owned(),
    });
    doc.sync();
    respond(varde, requests, |request| !is_topology(request));
}

/// Facts of a model since moved on from are asked for again, once, of
/// the model now; moved on again, the call says so.
#[test]
fn body_topology_asks_again_once_for_a_model_moved_on() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let call = || Call::BodyTopology(topology("Body 1"));
    let reply = calling(&mut varde, "a session", 1, call());
    move_on(&mut varde, &requests, "a");
    let now = varde.screen.doc().unwrap().editor.generation();
    // The first answer is of the model before: asked again, of now.
    respond(&mut varde, &requests, is_topology);
    let reply = reply.try_recv().unwrap();
    assert!(!reply.is_error, "{}", reply.text);
    let model = &reply.structured.unwrap()["model"];
    assert_eq!(model["generation"], serde_json::to_value(now).unwrap());

    let reply = calling(&mut varde, "a session", 2, call());
    move_on(&mut varde, &requests, "b");
    // Answered once: asked again, and moved on before that answer too.
    let first = requests.take().into_iter().filter(is_topology);
    for request in first.collect::<Vec<_>>() {
        let id = varde.screen.doc().unwrap().id;
        let _ = varde.update(Message::Doc(
            id,
            crate::message::ForDoc::Computed(handle(request)),
        ));
    }
    assert!(reply.try_recv().is_err());
    move_on(&mut varde, &requests, "c");
    respond(&mut varde, &requests, is_topology);
    assert_eq!(
        bare(reply.try_recv().unwrap()),
        ToolReply::error(waiting::MOVED_ON)
    );
}

/// Facts that don't pair with the model are an internal error, and a
/// failure to read them a tool error.
#[test]
fn body_topology_checks_the_facts() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let call = || Call::BodyTopology(topology("Body 1"));
    for (spoil, said) in [
        (true, "internal error"),
        (false, "couldn't be read: no solid"),
    ] {
        let reply = calling(&mut varde, "a session", 1, call());
        let request = requests.take().into_iter().find(is_topology).unwrap();
        let mut response = handle(request);
        let Response::Answered {
            answer: varde_regen::Answer::Topology { result, .. },
            ..
        } = &mut response
        else {
            panic!();
        };
        if spoil {
            let facts = result.as_mut().unwrap();
            facts.swap(0, 1);
            assert_ne!(facts[0].faces, facts[1].faces);
        } else {
            *result = Err("no solid".to_owned());
        }
        let id = varde.screen.doc().unwrap().id;
        let _ = varde.update(Message::Doc(id, crate::message::ForDoc::Computed(response)));
        let reply = reply.try_recv().unwrap();
        assert!(reply.is_error);
        assert!(reply.text.contains(said), "{}", reply.text);
    }
}

/// The 30 s run from the call's arrival at the server, the time it
/// queued before the app took it included: one that queued past them is
/// answered at the first tick, ahead of the server's backstop.
#[test]
fn the_bound_runs_from_the_calls_arrival_at_the_server() {
    let (mut varde, _requests) = listening_with();
    started(&mut varde, "a session", None);
    let now = std::time::Instant::now();
    let Some(came) = now.checked_sub(waiting::BOUND + std::time::Duration::from_secs(1)) else {
        return;
    };
    let (sender, reply) = mpsc::sync_channel(1);
    let event = Event::Call {
        session: SessionId::for_tests("a session"),
        id: Id::Number(1),
        call: Call::BodyTopology(topology("Body 1")),
        reply: Reply::for_tests(sender, came),
    };
    feed(&mut varde, event);
    let _ = varde.update(Message::Tick(now));
    assert_eq!(
        reply.try_recv().unwrap(),
        ToolReply::error(waiting::TOOK_TOO_LONG)
    );
}

/// Quitting answers the calls waiting that Varde is closing, rather than
/// leaving them to the server's backstop: their answers and ticks no
/// longer act.
#[test]
fn quitting_answers_the_calls_waiting() {
    let (mut varde, _requests) = listening_with();
    started(&mut varde, "a session", None);
    let reply = calling(
        &mut varde,
        "a session",
        1,
        Call::BodyTopology(topology("Body 1")),
    );
    assert!(reply.try_recv().is_err());
    let _ = varde.quit(iced::window::Id::unique());
    assert_eq!(
        reply.try_recv().unwrap(),
        ToolReply::error(varde_mcp::server::CLOSING)
    );
    assert!(!varde.screen.doc().unwrap().mcp_waiting());
}

fn rename_feature(name: &str, to: &str) -> Call {
    Call::Rename(varde_mcp::tools::Rename {
        target: varde_mcp::tools::Target::Feature(name.to_owned()),
        to: to.to_owned(),
    })
}

/// The name of `varde`'s design's feature at `at`.
fn feature_name(varde: &Varde, at: usize) -> String {
    let document = varde.screen.doc().unwrap().editor.document();
    document.features()[at].name.clone()
}

/// Answers the design's regeneration requests (not edge facts) through
/// `Varde::update`.
fn regenerate(varde: &mut Varde, requests: &Lane) {
    respond(varde, requests, |request| !is_topology(request));
}

/// An edit's reply, as its data, checked not to be an error.
fn edit_data(reply: ToolReply) -> Value {
    assert!(!reply.is_error, "{}", reply.text);
    bare(reply).structured.unwrap()
}

/// `reply` without the user's changes and the revision it carries
/// (`changes::attach`), checked to be in their place and form, for the
/// tests of what else it says.
fn bare(reply: ToolReply) -> ToolReply {
    match reply.structured {
        Some(mut data) if !reply.is_error => {
            let object = data.as_object_mut().unwrap();
            // Neither for a call of a session ended or replaced.
            let revision = object.remove("revision");
            assert!(revision.as_ref().is_none_or(Value::is_u64), "{data}");
            if let Some(changes) = object.remove("user_changes") {
                assert!(revision.is_some() && changes.is_object(), "{changes}");
            }
            ToolReply {
                png: reply.png,
                ..ToolReply::data(data)
            }
        }
        _ => ToolReply {
            text: match reply.text.split_once("\n\nuser_changes: ") {
                Some((text, tail)) => {
                    // Exactly the changes' JSON and the revision.
                    let (changes, revision) = tail.split_once("\nrevision: ").unwrap();
                    assert!(serde_json::from_str::<Value>(changes).unwrap().is_object());
                    revision.parse::<u64>().unwrap();
                    text.to_owned()
                }
                None => reply.text,
            },
            ..reply
        },
    }
}

/// [`listening_with`] in a session, in a new sketch on XY with its
/// solver lane deferred and the Point tool picked.
fn sketching() -> (Varde, Lane, crate::tests::SolveLane) {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    ui(
        &mut varde,
        varde_view::Message::Edit(varde_view::Edit::PlanePicked(
            varde_document::OriginPlane::XY,
        )),
    );
    let mut lane = crate::tests::SolveLane::new();
    (varde.screen.doc_mut().unwrap()).solver_ready(lane.transport());
    solve(&mut varde, &mut lane);
    ui(
        &mut varde,
        varde_view::Message::Look(varde_view::Look::SelectTool(varde_view::Tool::Point)),
    );
    regenerate(&mut varde, &requests);
    (varde, requests, lane)
}

/// Edits the sketch made last again, with the Point tool picked.
fn enter_last_sketch(varde: &mut Varde) {
    let document = varde.screen.doc().unwrap().editor.document();
    let id = (document.features().iter().rev())
        .find(|feature| matches!(feature.kind, varde_document::FeatureKind::Sketch { .. }))
        .unwrap()
        .id;
    ui(
        varde,
        varde_view::Message::Look(varde_view::Look::EditFeature(id)),
    );
    ui(
        varde,
        varde_view::Message::Look(varde_view::Look::SelectTool(varde_view::Tool::Point)),
    );
}

/// Answers the solver's requests through `Varde::update`.
fn solve(varde: &mut Varde, lane: &mut crate::tests::SolveLane) {
    let id = varde.screen.doc().unwrap().id;
    while let Some(response) = lane.respond() {
        let _ = varde.update(Message::Doc(id, crate::message::ForDoc::Solved(response)));
    }
}

/// Places a point at `x`, `y` in the sketch, which waits on the solver.
fn place_point(varde: &mut Varde, x: f64, y: f64) {
    ui(
        varde,
        varde_view::Message::Edit(varde_view::Edit::ToolClick(varde_view::ToolClick {
            at: glam::DVec2::new(x, y),
            target: None,
            inference: None,
            pixel: 0.1,
            double: false,
            hit: None,
            reference: false,
        })),
    );
}

/// The points of the sketch made last, committed: the one being edited,
/// or left as a client's edit closed it.
fn points(varde: &Varde) -> usize {
    let doc = varde.screen.doc().unwrap();
    (doc.editor.document().features().iter().rev())
        .find_map(|feature| match &feature.kind {
            varde_document::FeatureKind::Sketch { sketch, .. } => Some(sketch.points.len()),
            _ => None,
        })
        .unwrap()
}

fn undo(varde: &mut Varde) {
    ui(varde, varde_view::Message::Edit(varde_view::Edit::Undo));
}

/// An edit applies at once, one undo step, and answers once regeneration
/// has: what it made, its row applied.
#[test]
fn an_edit_applies_and_answers_after_regeneration() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let reply = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Base"),
    );
    assert_eq!(feature_name(&varde, 1), "Base");
    assert!(reply.try_recv().is_err());
    assert_eq!(varde.mcp.log[0].state, McpCallState::Waiting);
    assert_eq!(varde.mcp.log[0].args, "feature Extrude 1 to Base");
    regenerate(&mut varde, &requests);
    let data = edit_data(reply.try_recv().unwrap());
    let id = varde.screen.doc().unwrap().editor.document().features()[1].id;
    assert_eq!(
        data,
        json!({ "feature": "Base", "id": id.get(), "result": "applied" })
    );
    assert_eq!(varde.mcp.log[0].state, McpCallState::Applied);
    assert!(!varde.screen.doc().unwrap().mcp_waiting());
    // Its row says so, in words.
    varde.mcp.open = true;
    let shown = on_screen(&varde);
    assert!(shown.iter().any(|text| text == "Renamed"), "{shown:?}");
    assert!(shown.iter().any(|text| text == "Applied"), "{shown:?}");
    undo(&mut varde);
    assert_eq!(feature_name(&varde, 1), "Extrude 1");
    assert!(varde.screen.doc().unwrap().edit_error.is_none());
}

/// An edit changing nothing says so at once; a read-only design refuses
/// one, saying so; a name not found fails.
#[test]
fn an_edit_changing_nothing_or_on_a_read_only_design_says_so() {
    let (mut varde, _requests) = listening_with();
    started(&mut varde, "a session", None);
    let generation = varde.screen.doc().unwrap().editor.generation();
    let reply = called(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Extrude 1"),
    );
    assert_eq!(reply, ToolReply::text(edit::NOTHING_TO_CHANGE));
    assert_eq!(varde.screen.doc().unwrap().editor.generation(), generation);
    let reply = called(
        &mut varde,
        "a session",
        2,
        rename_feature("Extrude 9", "Base"),
    );
    assert!(reply.is_error && reply.text.contains("no feature is named"));
    assert_eq!(varde.mcp.log[1].state, McpCallState::Failed);

    varde.screen.doc_mut().unwrap().read_only = Some("it's open elsewhere".to_owned());
    let reply = called(
        &mut varde,
        "a session",
        3,
        rename_feature("Extrude 1", "Base"),
    );
    assert!(reply.is_error);
    assert!(
        reply.text.contains("read-only (it's open elsewhere)"),
        "{}",
        reply.text
    );
    assert_eq!(varde.mcp.log[2].state, McpCallState::NotApplied);
    assert_eq!(varde.screen.doc().unwrap().editor.generation(), generation);
}

/// An edit made while the user's sketch edit waits on the solver queues
/// behind it, so the undo order is the order things happened.
#[test]
fn an_edit_queues_behind_the_users_waiting_proposal() {
    let (mut varde, requests, mut lane) = sketching();
    place_point(&mut varde, 1.0, 2.0);
    assert!(varde.screen.doc().unwrap().proposing());
    let reply = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Base"),
    );
    assert_eq!(feature_name(&varde, 1), "Extrude 1");
    assert!(reply.try_recv().is_err());
    solve(&mut varde, &mut lane);
    assert_eq!(points(&varde), 1);
    // Its names are resolved on the model of the point committed.
    assert_eq!(feature_name(&varde, 1), "Extrude 1");
    regenerate(&mut varde, &requests);
    assert_eq!(feature_name(&varde, 1), "Base");
    assert_eq!(
        edit_data(reply.try_recv().unwrap())["result"],
        json!("applied")
    );
    // The rename came last: undone first.
    undo(&mut varde);
    assert_eq!(
        (feature_name(&varde, 1).as_str(), points(&varde)),
        ("Extrude 1", 1)
    );
    undo(&mut varde);
    assert_eq!(points(&varde), 0);
}

/// An edit still queued at 30 s is taken out of the queue and answered
/// not applied: it never applies after.
#[test]
fn an_edit_queued_30_s_is_not_applied() {
    let (mut varde, _requests, mut lane) = sketching();
    place_point(&mut varde, 1.0, 2.0);
    let came = iced::time::Instant::now();
    let reply = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Base"),
    );
    let late = came + waiting::BOUND + std::time::Duration::from_secs(1);
    let _ = varde.update(Message::Tick(late));
    assert_eq!(
        bare(reply.try_recv().unwrap()),
        edit::not_applied(waiting::NOT_APPLIED_BUSY)
    );
    assert_eq!(varde.mcp.log[0].state, McpCallState::NotApplied);
    assert!(!varde.screen.doc().unwrap().mcp_waiting());
    solve(&mut varde, &mut lane);
    assert_eq!(points(&varde), 1);
    assert_eq!(feature_name(&varde, 1), "Extrude 1");
    assert!(!varde.screen.doc().unwrap().proposing());
}

/// Once applied an edit's answer is applied: regeneration not answering
/// within 30 s says it's still running.
#[test]
fn an_edit_applied_with_regeneration_slow_says_so() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let came = iced::time::Instant::now();
    let reply = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Base"),
    );
    let late = came + waiting::BOUND + std::time::Duration::from_secs(1);
    let _ = varde.update(Message::Tick(late));
    let data = edit_data(reply.try_recv().unwrap());
    assert_eq!(data["result"], json!(waiting::STILL_RUNNING));
    assert_eq!(data["feature"], json!("Base"));
    assert_eq!(varde.mcp.log[0].state, McpCallState::Applied);
    regenerate(&mut varde, &requests);
    assert_eq!(feature_name(&varde, 1), "Base");
}

/// Undo passes over the client's edit waiting: it drops the user's own
/// newest, and the client's applies in its turn.
#[test]
fn undo_skips_the_clients_queued_edit() {
    let (mut varde, requests, mut lane) = sketching();
    place_point(&mut varde, 1.0, 2.0);
    let reply = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Base"),
    );
    place_point(&mut varde, 5.0, 6.0);
    // The second point, queued after the rename, goes.
    undo(&mut varde);
    assert_eq!(feature_name(&varde, 1), "Extrude 1");
    solve(&mut varde, &mut lane);
    assert_eq!(points(&varde), 1);
    regenerate(&mut varde, &requests);
    assert_eq!(feature_name(&varde, 1), "Base");
    assert_eq!(
        edit_data(reply.try_recv().unwrap())["result"],
        json!("applied")
    );

    // With only the one in flight, Undo drops it and the rename applies.
    // (The rename closed the sketch, clean: back into it.)
    assert!(varde.screen.doc().unwrap().mode.sketch().is_none());
    enter_last_sketch(&mut varde);
    place_point(&mut varde, 8.0, 1.0);
    let reply = calling(&mut varde, "a session", 2, rename_feature("Base", "Plate"));
    undo(&mut varde);
    assert_eq!(feature_name(&varde, 1), "Plate");
    solve(&mut varde, &mut lane);
    assert_eq!(points(&varde), 1);
    regenerate(&mut varde, &requests);
    assert_eq!(
        edit_data(reply.try_recv().unwrap())["result"],
        json!("applied")
    );
}

/// An edit the user undid before regeneration answered says so.
#[test]
fn an_edit_undone_before_regeneration_says_so() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let reply = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Base"),
    );
    undo(&mut varde);
    regenerate(&mut varde, &requests);
    let data = edit_data(reply.try_recv().unwrap());
    assert_eq!(data["result"], json!(edit::UNDONE));
}

/// A rename to a name taken comes back with the name it got; a body is
/// renamed by its name; a failing feature is reported.
#[test]
fn a_rename_clashing_says_the_name_it_got() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let reply = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Sketch 1"),
    );
    regenerate(&mut varde, &requests);
    let data = edit_data(reply.try_recv().unwrap());
    assert_eq!(data["feature"], json!("Sketch 1 (1)"));
    assert!(
        data["note"]
            .as_str()
            .unwrap()
            .contains("\"Sketch 1\" was taken")
    );

    let body = Call::Rename(varde_mcp::tools::Rename {
        target: varde_mcp::tools::Target::Body("Body 1".to_owned()),
        to: "Plate".to_owned(),
    });
    let reply = calling(&mut varde, "a session", 2, body);
    regenerate(&mut varde, &requests);
    assert_eq!(
        edit_data(reply.try_recv().unwrap()),
        json!({ "body": "Plate", "result": "applied" })
    );
}

/// The failures of the regeneration answering an edit come with its
/// result.
#[test]
fn an_edits_result_says_what_failed() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let reply = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Base"),
    );
    let extrude = varde.screen.doc().unwrap().editor.document().features()[1].id;
    let doc = varde.screen.doc_mut().unwrap();
    answer_failing(doc, &requests, extrude);
    let _ = varde.update(Message::Tick(iced::time::Instant::now()));
    let data = edit_data(reply.try_recv().unwrap());
    assert_eq!(
        data["failures"],
        json!([{ "feature": "Base", "failure": "the profile is gone" }])
    );
}

/// The call to `tool` with `arguments`, read as the server reads it.
fn tool_call(tool: &str, arguments: Value) -> Call {
    let params = json!({ "name": tool, "arguments": arguments });
    varde_mcp::tools::parse_call(Some(&params)).unwrap()
}

/// Calls `tool` with `arguments` in "a session" and answers regeneration:
/// its reply, and its row's state.
fn edit(
    varde: &mut Varde,
    requests: &Lane,
    tool: &str,
    arguments: Value,
) -> (ToolReply, McpCallState) {
    let id = (varde.mcp.log.back()).map_or(1, |logged| i64::try_from(logged.serial).unwrap() + 1);
    let reply = calling(varde, "a session", id, tool_call(tool, arguments));
    regenerate(varde, requests);
    let reply = reply.try_recv().unwrap();
    let state = varde.mcp.log.back().unwrap().state;
    (reply, state)
}

/// An edit's reply refused: not applied, saying `why`, the design
/// unchanged and the user's `edit_error` untouched.
fn refused_edit(varde: &mut Varde, requests: &Lane, tool: &str, arguments: Value, why: &str) {
    let generation = varde.screen.doc().unwrap().editor.generation();
    let (reply, state) = edit(varde, requests, tool, arguments);
    assert!(reply.is_error, "{}", reply.text);
    assert!(reply.text.starts_with("Not applied: "), "{}", reply.text);
    assert!(reply.text.contains(why), "{}", reply.text);
    assert_eq!(state, McpCallState::NotApplied);
    let doc = varde.screen.doc().unwrap();
    assert_eq!(doc.editor.generation(), generation);
    assert!(doc.edit_error.is_none());
}

/// A body is shown or hidden, coloured and made fainter as the Objects
/// panel does it, each one undo step, its result how it's drawn now.
#[test]
fn the_body_edits_apply() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "set_visible",
        json!({ "body": "Body 1", "visible": false }),
    );
    assert_eq!(state, McpCallState::Applied);
    assert_eq!(
        edit_data(reply),
        json!({
            "body": "Body 1", "visible": false, "color": null, "opacity": 1.0,
            "result": "applied"
        })
    );
    assert!(!varde.screen.doc().unwrap().editor.document().bodies()[0].visible);
    assert_eq!(varde.mcp.log[0].args, "Body 1 hidden");
    // Again: as it is already.
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_visible",
        json!({ "body": "Body 1", "visible": false }),
    );
    assert_eq!(reply, ToolReply::text(edit::NOTHING_TO_CHANGE));

    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_color",
        json!({ "body": "Body 1", "color": { "hue": 199.6, "saturation": 0.2 } }),
    );
    assert_eq!(
        edit_data(reply)["color"],
        json!({ "hue": 200, "saturation": 0.2 })
    );
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_opacity",
        json!({ "body": "Body 1", "opacity": 0.456 }),
    );
    assert_eq!(edit_data(reply)["opacity"], json!(0.46));
    let body = &varde.screen.doc().unwrap().editor.document().bodies()[0];
    assert_eq!(body.opacity, varde_document::Opacity::new(46).unwrap());
    assert_eq!(body.color, varde_document::Tint::new(200, 20));
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_color",
        json!({ "body": "Body 1", "color": null }),
    );
    assert_eq!(edit_data(reply)["color"], Value::Null);
    // Each was a step of its own.
    undo(&mut varde);
    undo(&mut varde);
    let body = &varde.screen.doc().unwrap().editor.document().bodies()[0];
    assert_eq!(
        (body.color, body.opacity.is_opaque(), body.visible),
        (varde_document::Tint::new(200, 20), true, false)
    );

    let (reply, state) = edit(
        &mut varde,
        &requests,
        "set_opacity",
        json!({ "body": "Body 9", "opacity": 0.5 }),
    );
    assert!(reply.text.contains("no body is named \"Body 9\""));
    assert_eq!(state, McpCallState::Failed);
}

/// A feature or a body others depend on is refused, naming them; one
/// nothing depends on goes with what it makes, without the delete
/// prompt.
#[test]
fn the_removals_go_without_the_cascade() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    refused_edit(
        &mut varde,
        &requests,
        "remove_feature",
        json!({ "feature": "Sketch 1" }),
        "later features use it: \"Extrude 1\"",
    );
    assert!(!varde.screen.doc().unwrap().delete_asked());
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "remove_body",
        json!({ "body": "Body 1" }),
    );
    assert_eq!(state, McpCallState::Applied);
    assert_eq!(
        edit_data(reply),
        json!({
            "removed": { "features": ["Extrude 1"], "bodies": ["Body 1"] },
            "result": "applied"
        })
    );
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "remove_feature",
        json!({ "feature": "Sketch 1" }),
    );
    assert_eq!(
        edit_data(reply)["removed"],
        json!({ "features": ["Sketch 1"], "bodies": [] })
    );
    let document = varde.screen.doc().unwrap().editor.document();
    assert!(document.features().is_empty() && document.bodies().is_empty());
    assert!(!varde.screen.doc().unwrap().delete_asked());
    // Two steps: undone one at a time.
    undo(&mut varde);
    assert_eq!(feature_name(&varde, 0), "Sketch 1");
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "remove_body",
        json!({ "body": "Body 1" }),
    );
    assert!(reply.text.contains("no body is named"));
    assert_eq!(state, McpCallState::Failed);
}

/// Parameters are added, set, renamed and removed as the popup does it;
/// the editor's refusals come back to the call, worded as the popup
/// words them.
#[test]
fn the_parameter_edits_apply_or_are_refused() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "add_parameter",
        json!({ "name": "w", "value": " 2 cm " }),
    );
    assert_eq!(state, McpCallState::Applied);
    assert_eq!(
        edit_data(reply),
        json!({ "parameter": "w", "expression": "2 cm", "value": "20 mm", "result": "applied" })
    );
    // The editor refuses another of the name.
    refused_edit(
        &mut varde,
        &requests,
        "add_parameter",
        json!({ "name": "w", "value": "1 mm" }),
        "Another parameter has this name",
    );
    // And a name that isn't a word.
    refused_edit(
        &mut varde,
        &requests,
        "add_parameter",
        json!({ "name": "2w", "value": "1 mm" }),
        "starting with a letter",
    );
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "add_parameter",
        json!({ "name": "v", "value": "w * 2" }),
    );
    assert_eq!(edit_data(reply)["value"], json!("40 mm"));
    // One left in error is taken, as the popup takes it: nothing uses it.
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "add_parameter",
        json!({ "name": "u", "value": "w +" }),
    );
    let data = edit_data(reply);
    assert!(
        data["error"].is_string() && data.get("value").is_none(),
        "{data}"
    );

    refused_edit(
        &mut varde,
        &requests,
        "remove_parameter",
        json!({ "name": "w" }),
        "it's used by \"v\"",
    );
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_parameter",
        json!({ "name": "w", "value": "3 cm" }),
    );
    assert_eq!(edit_data(reply)["value"], json!("30 mm"));
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_parameter",
        json!({ "name": "w", "value": "3 cm" }),
    );
    assert_eq!(reply, ToolReply::text(edit::NOTHING_TO_CHANGE));
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "rename_parameter",
        json!({ "name": "w", "to": "width" }),
    );
    assert_eq!(edit_data(reply)["parameter"], json!("width"));
    let params = varde.screen.doc().unwrap().editor.document().params();
    assert_eq!(params[1].text, "width * 2");
    refused_edit(
        &mut varde,
        &requests,
        "rename_parameter",
        json!({ "name": "width", "to": "v" }),
        "Another parameter has this name",
    );
    for name in ["u", "v"] {
        let (reply, _) = edit(
            &mut varde,
            &requests,
            "remove_parameter",
            json!({ "name": name }),
        );
        assert_eq!(
            edit_data(reply),
            json!({ "removed": { "parameters": [name] }, "result": "applied" })
        );
    }
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "set_parameter",
        json!({ "name": "v", "value": "1" }),
    );
    assert!(reply.text.contains("no parameter is named \"v\""));
    assert_eq!(state, McpCallState::Failed);
}

/// A parameter a feature's value uses can't be set to what would leave
/// that value in error: the editor refuses it, naming the feature.
#[test]
fn a_parameter_set_into_error_is_refused() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    edit(
        &mut varde,
        &requests,
        "add_parameter",
        json!({ "name": "w", "value": "5 mm" }),
    );
    let doc = varde.screen.doc_mut().unwrap();
    let extrude = doc.editor.document().features()[1].clone();
    let varde_document::FeatureKind::Extrude(mut extrude_kind) = extrude.kind else {
        panic!("the example's second feature is an extrude");
    };
    let ask = varde_expr::Ask::length(varde_expr::LengthUnit::Mm, 1e6)
        .with_params(doc.editor.document().params_resolved());
    extrude_kind.extent =
        varde_document::Extent::OneSide(varde_expr::Value::new("w", &ask).unwrap());
    doc.apply(Command::SetFeature {
        feature: extrude.id,
        kind: Box::new(varde_document::FeatureKind::Extrude(extrude_kind)),
    });
    assert!(doc.edit_error.is_none());
    regenerate(&mut varde, &requests);
    refused_edit(
        &mut varde,
        &requests,
        "set_parameter",
        json!({ "name": "w", "value": "5 deg" }),
        "Extrude 1 would be in error",
    );
}

/// The design's units, tolerance and rollback marker, each checked as
/// the file menu and the Timeline bound them.
#[test]
fn the_design_edits_apply_within_bounds() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let (reply, _) = edit(&mut varde, &requests, "set_units", json!({ "units": "in" }));
    assert_eq!(edit_data(reply)["units"], json!("in"));
    assert_eq!(
        varde.screen.doc().unwrap().editor.document().units(),
        varde_expr::LengthUnit::In
    );
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_tolerance",
        json!({ "tolerance": "10 µm" }),
    );
    assert_eq!(edit_data(reply)["tolerance"], json!("10 µm"));
    // A length as a field reads it, bare numbers in the design's units.
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_tolerance",
        json!({ "tolerance": "0.05 mm" }),
    );
    assert_eq!(edit_data(reply)["tolerance"], json!("50 µm"));
    let tolerance = varde.screen.doc().unwrap().editor.document().tolerance();
    for text in [
        "1 mm", "0.001 um", "0", "-1 µm", "1 deg", "width", "1e400 um", "0.01",
    ] {
        let (reply, state) = edit(
            &mut varde,
            &requests,
            "set_tolerance",
            json!({ "tolerance": text }),
        );
        assert!(
            reply.is_error && reply.text.starts_with("set_tolerance: "),
            "{text}: {}",
            reply.text
        );
        assert_eq!(state, McpCallState::Failed);
    }
    assert_eq!(
        varde.screen.doc().unwrap().editor.document().tolerance(),
        tolerance
    );

    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_rollback",
        json!({ "before": "Extrude 1" }),
    );
    let data = edit_data(reply);
    assert_eq!(data["rollback"], json!("Extrude 1"));
    assert_eq!(data["not_built"], json!(["Extrude 1"]));
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_rollback",
        json!({ "before": null }),
    );
    let data = edit_data(reply);
    assert_eq!(data["rollback"], Value::Null);
    assert!(data.get("not_built").is_none());
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "set_rollback",
        json!({ "before": "Extrude 9" }),
    );
    assert!(reply.text.contains("no feature is named"));
    assert_eq!(state, McpCallState::Failed);
}

/// An edit the user undid before the 30 s bound ended its wait says so,
/// rather than that regeneration is still running.
#[test]
fn an_edit_undone_then_timed_out_says_so() {
    let (mut varde, _requests) = listening_with();
    started(&mut varde, "a session", None);
    let came = iced::time::Instant::now();
    let reply = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Base"),
    );
    undo(&mut varde);
    let late = came + waiting::BOUND + std::time::Duration::from_secs(1);
    let _ = varde.update(Message::Tick(late));
    let data = edit_data(reply.try_recv().unwrap());
    assert_eq!(data["result"], json!(edit::UNDONE));
}

/// The bounds `varde-mcp` mirrors (it depends on no other varde crate)
/// are their sources': the longest name a design holds, the expression
/// fields evaluate, the faintest opacity and the most saturated colour.
#[test]
fn the_mirrored_bounds_are_their_sources() {
    assert_eq!(varde_mcp::tools::NAME_BYTES, varde_document::MAX_NAME_LEN);
    assert_eq!(varde_mcp::tools::TEXT_BYTES, varde_expr::MAX_LEN);
    assert_eq!(
        varde_mcp::tools::MIN_OPACITY,
        f64::from(varde_document::Opacity::MIN.percent()) / 100.0
    );
    assert_eq!(
        varde_mcp::tools::MAX_SATURATION,
        f64::from(varde_document::Tint::MAX_SATURATION) / 100.0
    );
}

/// An edit queued is made from its call in its turn, on the design as it
/// is then: one queued before it that shifts the parameters doesn't send
/// it to another parameter, and a target gone meanwhile is said so.
#[test]
fn a_queued_edit_is_made_on_the_design_of_its_turn() {
    let (mut varde, requests, mut lane) = sketching();
    for (name, value) in [("a", "1 mm"), ("b", "2 mm")] {
        let (reply, _) = edit(
            &mut varde,
            &requests,
            "add_parameter",
            json!({ "name": name, "value": value }),
        );
        assert!(reply.structured.is_some(), "{}", reply.text);
    }
    place_point(&mut varde, 1.0, 2.0);
    let removed = calling(
        &mut varde,
        "a session",
        10,
        tool_call("remove_parameter", json!({ "name": "a" })),
    );
    let set = calling(
        &mut varde,
        "a session",
        11,
        tool_call("set_parameter", json!({ "name": "b", "value": "5 mm" })),
    );
    let renamed = calling(
        &mut varde,
        "a session",
        12,
        tool_call("rename_parameter", json!({ "name": "a", "to": "c" })),
    );
    solve(&mut varde, &mut lane);
    regenerate(&mut varde, &requests);
    edit_data(removed.try_recv().unwrap());
    let data = edit_data(set.try_recv().unwrap());
    assert_eq!(data["parameter"], json!("b"));
    assert_eq!(data["expression"], json!("5 mm"));
    let params = varde.screen.doc().unwrap().editor.document().params();
    assert_eq!(params.len(), 1);
    assert_eq!(
        (params[0].name.as_str(), params[0].text.as_str()),
        ("b", "5 mm")
    );
    let reply = renamed.try_recv().unwrap();
    assert!(
        reply.is_error && reply.text.contains("no parameter is named \"a\""),
        "{}",
        reply.text
    );
}

/// An edit whose call has waited 30 s by its turn isn't applied, though
/// the tick that would have ended it hasn't come yet.
#[test]
fn an_edit_reaching_its_turn_past_the_bound_is_not_applied() {
    let (mut varde, _requests, mut lane) = sketching();
    place_point(&mut varde, 1.0, 2.0);
    // Taken up just inside the bound, its turn coming just past it.
    let margin = std::time::Duration::from_millis(200);
    let Some(came) = std::time::Instant::now().checked_sub(waiting::BOUND - margin) else {
        return;
    };
    let (sender, reply) = mpsc::sync_channel(1);
    feed(
        &mut varde,
        Event::Call {
            session: SessionId::for_tests("a session"),
            id: Id::Number(1),
            call: rename_feature("Extrude 1", "Base"),
            reply: Reply::for_tests(sender, came),
        },
    );
    std::thread::sleep(margin);
    solve(&mut varde, &mut lane);
    assert_eq!(points(&varde), 1);
    assert_eq!(feature_name(&varde, 1), "Extrude 1");
    // At its turn, the point's model not in: it was holding the queue
    // for that.
    assert_eq!(
        bare(reply.try_recv().unwrap()),
        edit::not_applied(waiting::NOT_APPLIED_MODEL)
    );
    assert!(!varde.screen.doc().unwrap().mcp_waiting());
}

/// An edit the app takes up past the bound, its time queued before
/// counted, isn't applied.
#[test]
fn an_edit_taken_up_past_the_bound_is_not_applied() {
    let (mut varde, _requests) = listening_with();
    started(&mut varde, "a session", None);
    let Some(came) = std::time::Instant::now().checked_sub(waiting::BOUND) else {
        return;
    };
    let (sender, reply) = mpsc::sync_channel(1);
    feed(
        &mut varde,
        Event::Call {
            session: SessionId::for_tests("a session"),
            id: Id::Number(1),
            call: rename_feature("Extrude 1", "Base"),
            reply: Reply::for_tests(sender, came),
        },
    );
    assert_eq!(feature_name(&varde, 1), "Extrude 1");
    assert_eq!(
        reply.try_recv().unwrap(),
        edit::not_applied(waiting::NOT_APPLIED_LATE)
    );
}

/// The tolerance's ends are taken, in micrometres and as a length.
#[test]
fn the_tolerances_ends_are_taken() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    for (text, shown) in [
        ("0.01 µm", "0.01 µm"),
        ("100 um", "100 µm"),
        ("0.1 mm", "100 µm"),
        ("0.00001 mm", "0.01 µm"),
    ] {
        let (reply, _) = edit(
            &mut varde,
            &requests,
            "set_tolerance",
            json!({ "tolerance": text }),
        );
        let tolerance = (reply.structured.as_ref()).map(|data| data["tolerance"].clone());
        assert!(
            tolerance == Some(json!(shown)) || reply == ToolReply::text(edit::NOTHING_TO_CHANGE),
            "{text}: {}",
            reply.text
        );
    }
}

fn look(varde: &mut Varde, message: varde_view::Look) {
    ui(varde, varde_view::Message::Look(message));
}

fn panel(varde: &mut Varde, message: varde_view::McpPanel) {
    ui(varde, varde_view::Message::McpPanel(message));
}

/// The design's feature `at`'s id.
fn feature_id(varde: &Varde, at: usize) -> FeatureId {
    varde.screen.doc().unwrap().editor.document().features()[at].id
}

/// The popup up, if it is.
fn ask(varde: &Varde) -> Option<&ask::Ask> {
    varde.screen.doc().unwrap().mcp.ask.as_ref()
}

/// Opens the example's extrude.
fn edit_extrude(varde: &mut Varde) {
    let id = feature_id(varde, 1);
    look(varde, varde_view::Look::EditFeature(id));
}

/// Types `text` in the extrude panel's first distance.
fn extrude_distance(varde: &mut Varde, text: &str) {
    look(
        varde,
        varde_view::Look::Extrude(varde_view::ExtrudeLook::Input {
            distance: varde_view::Distance::First,
            text: text.to_owned(),
        }),
    );
}

/// The example's extrude's first distance, as committed.
fn committed_distance(varde: &Varde) -> String {
    let document = varde.screen.doc().unwrap().editor.document();
    match &document.features()[1].kind {
        varde_document::FeatureKind::Extrude(extrude) => match &extrude.extent {
            varde_document::Extent::OneSide(value) => value.text.clone(),
            extent => panic!("{extent:?}"),
        },
        kind => panic!("{kind:?}"),
    }
}

/// Whether any session, plane pick, marker drag or measure is open.
fn anything_open(varde: &Varde) -> bool {
    let doc = varde.screen.doc().unwrap();
    doc.mode.sketch().is_some()
        || doc.operating()
        || doc.mode.picking_plane().is_some()
        || doc.rolling.is_some()
        || doc.mode.measure().is_some()
}

/// A client edit closes each session, new or on a feature, with nothing
/// unfinished in it, as Esc would, and applies: and the plane being
/// picked, the marker's drag and the measure tool.
#[test]
fn a_client_edit_closes_what_is_open_and_clean() {
    use varde_view::Look;
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let extrude = feature_id(&varde, 1);
    let sketch = feature_id(&varde, 0);
    let opens: Vec<(&str, Vec<Look>)> = vec![
        ("a new extrude", vec![Look::StartExtrude]),
        ("the extrude", vec![Look::EditFeature(extrude)]),
        ("a new revolve", vec![Look::StartRevolve]),
        ("a new combine", vec![Look::StartCombine]),
        ("a new move", vec![Look::StartMotion(MotionKind::Move)]),
        ("a new shell", vec![Look::StartMotion(MotionKind::Shell)]),
        ("the sketch", vec![Look::EditFeature(sketch)]),
        (
            "the sketch with a tool",
            vec![
                Look::EditFeature(sketch),
                Look::SelectTool(varde_view::Tool::Line),
            ],
        ),
        ("a plane pick", vec![Look::PickPlane]),
        ("the measure tool", vec![Look::StartMeasure]),
        ("the marker's drag", vec![Look::DragRollback(Some(extrude))]),
    ];
    for (at, (what, messages)) in opens.into_iter().enumerate() {
        for message in messages {
            look(&mut varde, message);
        }
        assert!(anything_open(&varde), "{what} isn't open");
        let to = format!("Profile {at}");
        let call = rename_feature(&feature_name(&varde, 0), &to);
        let reply = calling(&mut varde, "a session", i64::try_from(at).unwrap(), call);
        assert!(ask(&varde).is_none(), "{what} asked");
        assert!(!anything_open(&varde), "{what} is still open");
        assert_eq!(feature_name(&varde, 0), to, "{what}");
        regenerate(&mut varde, &requests);
        assert_eq!(
            edit_data(reply.try_recv().unwrap())["result"],
            json!("applied")
        );
    }
}

/// An extrude changed since it opened asks; Apply yours commits it as
/// its OK would, then the client's edit applies, and an edit coming
/// while the popup is up waits behind it, in order.
#[test]
fn an_unfinished_panel_asks_and_apply_yours_commits_it_first() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", Some("Claude Code"));
    let extrude = feature_id(&varde, 1);
    look(&mut varde, varde_view::Look::EditFeature(extrude));
    extrude_distance(&mut varde, "7 mm");
    let first = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Sketch 1", "Profile"),
    );
    let second = calling(
        &mut varde,
        "a session",
        2,
        rename_feature("Extrude 1", "Base"),
    );
    assert!(first.try_recv().is_err() && second.try_recv().is_err());
    assert_eq!(feature_name(&varde, 0), "Sketch 1");
    assert!(varde.screen.doc().unwrap().mode.extrude().is_some());
    assert_eq!(ask(&varde).unwrap().editing, "editing Extrude 1");
    let shown = on_screen(&varde);
    assert!(
        shown
            .iter()
            .any(|text| text
                == "Claude Code wants to change the design while you're editing Extrude 1."),
        "{shown:?}"
    );
    assert!(shown.iter().any(|text| text == "Apply yours"));
    assert!(varde.screen.doc().unwrap().mcp_waiting());

    panel(&mut varde, varde_view::McpPanel::ApplyYours);
    assert!(ask(&varde).is_none());
    assert!(varde.screen.doc().unwrap().mode.extrude().is_none());
    assert_eq!(committed_distance(&varde), "7 mm");
    // Applied once the model of the user's extrude is in, the second,
    // behind it, after.
    assert_eq!(feature_name(&varde, 0), "Sketch 1");
    regenerate(&mut varde, &requests);
    assert_eq!(feature_name(&varde, 0), "Profile");
    assert_eq!(feature_name(&varde, 1), "Base");
    assert_eq!(
        edit_data(first.try_recv().unwrap())["result"],
        json!("applied")
    );
    assert_eq!(
        edit_data(second.try_recv().unwrap())["result"],
        json!("applied")
    );
    // Undo takes them back newest first: the user's extrude last.
    undo(&mut varde);
    assert_eq!(feature_name(&varde, 1), "Extrude 1");
    assert_eq!(feature_name(&varde, 0), "Profile");
    undo(&mut varde);
    assert_eq!(feature_name(&varde, 0), "Sketch 1");
    assert_eq!(committed_distance(&varde), "7 mm");
    undo(&mut varde);
    assert_ne!(committed_distance(&varde), "7 mm");
}

/// Apply yours with the panel's OK refused leaves it open and the popup
/// saying why, the call waiting; Keep editing then refuses the call.
#[test]
fn apply_yours_refused_says_why_and_keep_editing_refuses_the_call() {
    let (mut varde, _requests) = listening_with();
    started(&mut varde, "a session", None);
    edit_extrude(&mut varde);
    // A field refused keeps the value it read last, which differs.
    extrude_distance(&mut varde, "7 mm");
    extrude_distance(&mut varde, "seven");
    let reply = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Sketch 1", "Profile"),
    );
    panel(&mut varde, varde_view::McpPanel::ApplyYours);
    let refused = ask(&varde).unwrap().refused.clone().unwrap();
    assert!(
        refused.starts_with("Your Extrude 1 wasn't applied"),
        "{refused}"
    );
    assert!(varde.screen.doc().unwrap().mode.extrude().is_some());
    assert!(reply.try_recv().is_err());
    assert!(on_screen(&varde).contains(&refused));

    panel(&mut varde, varde_view::McpPanel::KeepEditing);
    let reply = reply.try_recv().unwrap();
    assert!(reply.is_error);
    assert!(
        reply.text.contains("the user is editing Extrude 1"),
        "{}",
        reply.text
    );
    assert_eq!(varde.mcp.log[0].state, McpCallState::NotApplied);
    assert!(ask(&varde).is_none());
    assert!(varde.screen.doc().unwrap().mode.extrude().is_some());
    assert_eq!(feature_name(&varde, 0), "Sketch 1");
}

/// A sketch with its tool started asks; Apply yours leaves it, then the
/// edit applies.
#[test]
fn a_started_tool_asks_and_apply_yours_leaves_the_sketch() {
    let (mut varde, requests, mut lane) = sketching();
    look(
        &mut varde,
        varde_view::Look::SelectTool(varde_view::Tool::Line),
    );
    place_point(&mut varde, 1.0, 2.0);
    solve(&mut varde, &mut lane);
    let reply = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Base"),
    );
    let name = feature_name(&varde, 2);
    assert_eq!(ask(&varde).unwrap().editing, format!("editing {name}"));
    panel(&mut varde, varde_view::McpPanel::ApplyYours);
    assert!(varde.screen.doc().unwrap().mode.sketch().is_none());
    assert_eq!(feature_name(&varde, 1), "Base");
    regenerate(&mut varde, &requests);
    assert_eq!(
        edit_data(reply.try_recv().unwrap())["result"],
        json!("applied")
    );
}

/// Past 30 s the call asked about answers not applied, the user editing;
/// the popup stays, saying it was cancelled, with only Close. Apply yours
/// then finishes the user's work alone.
#[test]
fn a_popup_past_30_s_says_the_edit_was_cancelled() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", Some("Claude Code"));
    edit_extrude(&mut varde);
    extrude_distance(&mut varde, "7 mm");
    let came = iced::time::Instant::now();
    let reply = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Sketch 1", "Profile"),
    );
    let late = came + waiting::BOUND + std::time::Duration::from_secs(1);
    let _ = varde.update(Message::Tick(late));
    let reply = reply.try_recv().unwrap();
    assert!(
        reply.text.contains("the user is editing Extrude 1"),
        "{}",
        reply.text
    );
    assert_eq!(varde.mcp.log[0].state, McpCallState::NotApplied);
    assert!(!varde.screen.doc().unwrap().mcp_waiting());
    assert!(ask(&varde).unwrap().cancelled);
    let shown = on_screen(&varde);
    assert!(
        shown
            .iter()
            .any(|text| text == "Claude Code's edit was cancelled")
    );
    assert!(shown.iter().any(|text| text == "Close"));
    assert!(!shown.iter().any(|text| text == "Apply yours"));

    panel(&mut varde, varde_view::McpPanel::ApplyYours);
    assert!(ask(&varde).is_none());
    assert_eq!(committed_distance(&varde), "7 mm");
    assert_eq!(feature_name(&varde, 0), "Sketch 1");
    regenerate(&mut varde, &requests);

    // Cancelled by the client: the same; Close closes it.
    edit_extrude(&mut varde);
    extrude_distance(&mut varde, "8 mm");
    let reply = calling(
        &mut varde,
        "a session",
        2,
        rename_feature("Sketch 1", "Profile"),
    );
    let cancelled = session::Event::Cancelled {
        session: SessionId::for_tests("a session"),
        request: Id::Number(2),
    };
    feed(&mut varde, Event::Session(cancelled));
    assert!(
        reply
            .try_recv()
            .unwrap()
            .text
            .contains("the user is editing")
    );
    assert!(ask(&varde).unwrap().cancelled);
    panel(&mut varde, varde_view::McpPanel::KeepEditing);
    assert!(ask(&varde).is_none());
    assert!(varde.screen.doc().unwrap().mode.extrude().is_some());
    // The design closing closes the popup with it.
    let _reply = calling(
        &mut varde,
        "a session",
        3,
        rename_feature("Sketch 1", "Profile"),
    );
    assert!(ask(&varde).is_some());
    varde.show(Screen::Document(Box::new(example().0)));
    assert!(ask(&varde).is_none());
}

/// The rename field open and a parameter being typed ask; Apply yours
/// renames as typed first.
#[test]
fn the_rename_field_and_a_parameter_typed_ask() {
    let (mut doc, requests) = example();
    doc.apply(Command::AddParam {
        name: "width".to_owned(),
        text: "2 cm".to_owned(),
    });
    doc.sync();
    let mut varde = showing(doc);
    varde.options.mcp = true;
    varde.options.mcp_port = 7311;
    varde.mcp = serving_on(7311);
    started(&mut varde, "a session", None);
    regenerate(&mut varde, &requests);
    let extrude = feature_id(&varde, 1);
    look(
        &mut varde,
        varde_view::Look::StartRename(varde_document::Named::Feature(extrude)),
    );
    look(&mut varde, varde_view::Look::RenameInput("Base".to_owned()));
    let reply = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Sketch 1", "Profile"),
    );
    assert_eq!(ask(&varde).unwrap().editing, "renaming Extrude 1");
    panel(&mut varde, varde_view::McpPanel::ApplyYours);
    assert_eq!(feature_name(&varde, 1), "Base");
    regenerate(&mut varde, &requests);
    assert_eq!(feature_name(&varde, 0), "Profile");
    assert_eq!(
        edit_data(reply.try_recv().unwrap())["result"],
        json!("applied")
    );

    look(
        &mut varde,
        varde_view::Look::Params(varde_view::ParamsLook::Input {
            index: 0,
            field: varde_view::ParamField::Expression,
            text: "3 cm".to_owned(),
        }),
    );
    let reply = calling(
        &mut varde,
        "a session",
        2,
        rename_feature("Profile", "Outline"),
    );
    assert_eq!(ask(&varde).unwrap().editing, "editing the parameter width");
    panel(&mut varde, varde_view::McpPanel::KeepEditing);
    assert!(
        reply
            .try_recv()
            .unwrap()
            .text
            .contains("the user is editing the parameter width")
    );
    assert_eq!(feature_name(&varde, 0), "Profile");
}

/// Queries never ask.
#[test]
fn queries_never_ask() {
    let (mut varde, _requests) = listening_with();
    started(&mut varde, "a session", None);
    edit_extrude(&mut varde);
    extrude_distance(&mut varde, "7 mm");
    let reply = called(
        &mut varde,
        "a session",
        1,
        Call::DesignSummary(Page::SUMMARY),
    );
    assert!(!reply.is_error);
    assert!(ask(&varde).is_none());
    assert!(varde.screen.doc().unwrap().mode.extrude().is_some());
}

/// An opener refused while a panel is open (the measure tool can't
/// start over an extrude) leaves that panel as it was, and what's unfinished in it
/// with it: the client's edit asks rather than closing it.
#[test]
fn an_opener_refused_keeps_the_panels_baseline() {
    let (mut varde, _requests) = listening_with();
    started(&mut varde, "a session", None);
    edit_extrude(&mut varde);
    extrude_distance(&mut varde, "7 mm");
    look(&mut varde, varde_view::Look::StartMeasure);
    assert!(varde.screen.doc().unwrap().mode.extrude().is_some());
    let reply = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Sketch 1", "Profile"),
    );
    assert!(reply.try_recv().is_err());
    assert_eq!(ask(&varde).unwrap().editing, "editing Extrude 1");
    assert!(varde.screen.doc().unwrap().mode.extrude().is_some());
    assert_eq!(feature_name(&varde, 0), "Sketch 1");
}

/// A panel opened, and changed, while the client's edit waited behind
/// the user's sketch edits on the solver asks at the edit's turn; the
/// sketch's edits, left with the sketch, are still committed.
#[test]
fn a_panel_opened_while_the_edit_queued_asks_at_its_turn() {
    let (mut varde, requests, mut lane) = sketching();
    let before = points(&varde);
    place_point(&mut varde, 1.0, 2.0);
    look(&mut varde, varde_view::Look::FinishSketch);
    edit_extrude(&mut varde);
    extrude_distance(&mut varde, "7 mm");
    let reply = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Sketch 1", "Profile"),
    );
    assert!(reply.try_recv().is_err());
    solve(&mut varde, &mut lane);
    assert_eq!(points(&varde), before + 1);
    // Its turn waits for the model of the point committed.
    assert!(ask(&varde).is_none());
    regenerate(&mut varde, &requests);
    assert_eq!(ask(&varde).unwrap().editing, "editing Extrude 1");
    assert_eq!(feature_name(&varde, 0), "Sketch 1");
    assert!(varde.screen.doc().unwrap().mode.extrude().is_some());
    panel(&mut varde, varde_view::McpPanel::ApplyYours);
    assert_eq!(committed_distance(&varde), "7 mm");
    regenerate(&mut varde, &requests);
    assert_eq!(feature_name(&varde, 0), "Profile");
    assert_eq!(
        edit_data(reply.try_recv().unwrap())["result"],
        json!("applied")
    );
}

/// An edit coming while the popup is up, with the user's sketch edits
/// still on the solver, waits behind the one asked about: Apply yours
/// keeps their order.
#[test]
fn an_edit_behind_the_popup_keeps_its_place_behind_the_solver() {
    let (mut varde, requests, mut lane) = sketching();
    place_point(&mut varde, 5.0, 5.0);
    let first = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Base"),
    );
    look(
        &mut varde,
        varde_view::Look::SelectTool(varde_view::Tool::Line),
    );
    place_point(&mut varde, 1.0, 2.0);
    // The point committed and its model in, the first edit's turn asks
    // (the line is started).
    solve(&mut varde, &mut lane);
    regenerate(&mut varde, &requests);
    assert!(ask(&varde).is_some());
    // The user ends the line meanwhile: its proposal goes to the solver.
    place_point(&mut varde, 3.0, 2.0);
    assert!(varde.screen.doc().unwrap().proposing());
    let second = calling(
        &mut varde,
        "a session",
        2,
        rename_feature("Sketch 1", "Profile"),
    );
    assert!(second.try_recv().is_err());
    // Apply yours: the first queues behind the line, the second behind
    // it; once the line is committed and its model in, both apply.
    panel(&mut varde, varde_view::McpPanel::ApplyYours);
    assert!(first.try_recv().is_err());
    solve(&mut varde, &mut lane);
    regenerate(&mut varde, &requests);
    regenerate(&mut varde, &requests);
    assert_eq!(
        edit_data(first.try_recv().unwrap())["result"],
        json!("applied")
    );
    assert_eq!(
        edit_data(second.try_recv().unwrap())["result"],
        json!("applied")
    );
    // Undo takes back the second first.
    undo(&mut varde);
    assert_eq!(feature_name(&varde, 0), "Sketch 1");
    assert_eq!(feature_name(&varde, 1), "Base");
}

/// The feature of `varde`'s design at `at`.
fn feature_at(varde: &Varde, at: usize) -> varde_document::Feature {
    let document = varde.screen.doc().unwrap().editor.document();
    document.features()[at].clone()
}

/// The profiles of the sketch named `name` in `varde`'s design.
fn profiles_of(varde: &Varde, name: &str) -> varde_sketch::Profiles {
    let document = varde.screen.doc().unwrap().editor.document();
    let feature = (document.features().iter())
        .find(|f| f.name == name)
        .unwrap();
    let varde_document::FeatureKind::Sketch { sketch, .. } = &feature.kind else {
        panic!("{name} isn't a sketch");
    };
    sketch.profiles().unwrap()
}

/// The index of the example's disc, the hole's region, in its sketch's
/// regions, and of the plate less it.
fn disc_and_plate(varde: &Varde) -> (usize, usize) {
    let profiles = profiles_of(varde, "Sketch 1");
    let disc = (profiles.regions.iter()).position(|region| region.holes.is_empty());
    let plate = (profiles.regions.iter()).position(|region| region.holes.len() == 1);
    (disc.unwrap(), plate.unwrap())
}

/// An argument refused: the row failed, the design unchanged, the
/// reply saying `why`.
fn wrong_edit(varde: &mut Varde, requests: &Lane, tool: &str, arguments: Value, why: &str) {
    let generation = varde.screen.doc().unwrap().editor.generation();
    let (reply, state) = edit(varde, requests, tool, arguments);
    assert!(reply.is_error, "{}", reply.text);
    assert!(reply.text.contains(why), "{}", reply.text);
    assert_eq!(state, McpCallState::Failed, "{}", reply.text);
    assert_eq!(varde.screen.doc().unwrap().editor.generation(), generation);
}

/// The extrude of `feature`.
fn extrude_of(feature: &varde_document::Feature) -> varde_document::Extrude {
    match &feature.kind {
        varde_document::FeatureKind::Extrude(extrude) => extrude.clone(),
        other => panic!("not an extrude: {other:?}"),
    }
}

#[test]
fn the_feature_tools_take_what_the_document_does() {
    assert_eq!(
        varde_mcp::tools::MAX_ITEMS,
        varde_document::MAX_EXTRUDE_REGIONS
    );
    assert_eq!(
        varde_mcp::tools::MAX_ITEMS,
        varde_document::MAX_REVOLVE_REGIONS
    );
    assert_eq!(
        varde_mcp::tools::MAX_ITEMS,
        varde_document::MAX_FEATURE_BODIES
    );
}

/// `extrude` with only its sketch and profiles takes the panel's
/// defaults, but a fixed distance: one side, 10 mm, no taper, a new
/// body; one undo step, its result naming the feature and its body.
#[test]
fn extrude_adds_with_the_panels_defaults() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let (disc, _) = disc_and_plate(&varde);
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "extrude",
        json!({ "sketch": "Sketch 1", "profiles": [disc] }),
    );
    assert_eq!(state, McpCallState::Applied);
    let feature = feature_at(&varde, 2);
    assert_eq!(
        edit_data(reply),
        json!({
            "feature": "Extrude 2", "id": feature.id.get(), "bodies": ["Body 2"],
            "result": "applied"
        })
    );
    assert_eq!(varde.mcp.log[0].args, "Sketch 1, 1 region");
    let extrude = extrude_of(&feature);
    let profiles = profiles_of(&varde, "Sketch 1");
    assert_eq!(extrude.regions, vec![profiles.reference(disc).unwrap()]);
    let varde_document::Extent::OneSide(distance) = &extrude.extent else {
        panic!("{:?}", extrude.extent);
    };
    assert_eq!(distance.value, 10.0);
    assert!(!extrude.flip);
    assert!(extrude.taper.is_none());
    assert!(matches!(
        extrude.operation,
        varde_document::Operation::NewBody(_)
    ));
    let doc = varde.screen.doc().unwrap();
    assert!(doc.edit_error.is_none());
    assert_eq!(doc.editor.document().bodies().len(), 2);
    undo(&mut varde);
    let document = varde.screen.doc().unwrap().editor.document();
    assert_eq!(document.features().len(), 2);
}

/// `extrude` with every value given: the profiles by index, `"all"` or
/// a point inside, each extent and operation, typed text read as the
/// panel's fields read it.
#[test]
fn extrude_takes_explicit_values_and_profiles() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let (disc, plate) = disc_and_plate(&varde);
    let profiles = profiles_of(&varde, "Sketch 1");
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "extrude",
        json!({
            "sketch": "Sketch 1", "profiles": "all", "extent": "two_sides",
            "distance": "5 mm", "second_distance": "2", "flip": true, "taper": "3 deg",
            "operation": "join"
        }),
    );
    assert_eq!(edit_data(reply)["bodies"], json!([]));
    let extrude = extrude_of(&feature_at(&varde, 2));
    let mut all = [disc, plate];
    all.sort_unstable();
    let references: Vec<_> = all
        .iter()
        .map(|&i| profiles.reference(i).unwrap())
        .collect();
    assert_eq!(extrude.regions, references);
    let varde_document::Extent::TwoSides(first, second) = &extrude.extent else {
        panic!("{:?}", extrude.extent);
    };
    assert_eq!((first.value, second.value), (5.0, 2.0));
    assert!(extrude.flip);
    let taper = extrude.taper.as_ref().unwrap();
    assert!((taper.value - 3f64.to_radians()).abs() < 1e-12);
    assert!(matches!(
        extrude.operation,
        varde_document::Operation::Join(_)
    ));

    // A point inside picks the region it's in; symmetric; a cut through
    // all leaving Body 1 out (the only body it could cut).
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "extrude",
        json!({ "sketch": "Sketch 1", "profiles": [["0", "1 mm"]], "extent": "symmetric",
                "distance": "4" }),
    );
    edit_data(reply);
    let extrude = extrude_of(&feature_at(&varde, 3));
    assert_eq!(extrude.regions, vec![profiles.reference(disc).unwrap()]);
    assert!(matches!(
        extrude.extent,
        varde_document::Extent::Symmetric(_)
    ));
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "extrude",
        json!({ "sketch": "Sketch 1", "profiles": [plate, ["25", "15"]],
                "extent": "through_all", "operation": "cut", "exclude": ["Body 1"] }),
    );
    edit_data(reply);
    let extrude = extrude_of(&feature_at(&varde, 4));
    assert_eq!(extrude.regions, vec![profiles.reference(plate).unwrap()]);
    assert_eq!(extrude.extent, varde_document::Extent::ThroughAll);
    let body = varde.screen.doc().unwrap().editor.document().bodies()[0].id;
    assert_eq!(extrude.operation.excluded(), &[body]);
}

/// What the panel wouldn't take, `extrude` refuses, the design
/// unchanged: values out of the fields' bounds, profiles not in the
/// sketch, an extent or exclusion the operation doesn't go with.
#[test]
fn extrude_refuses_what_the_panel_would_not_take() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    for (arguments, why) in [
        (
            json!({ "sketch": "Sketch 1", "profiles": "all", "distance": "1e9 mm" }),
            "distance: ",
        ),
        (
            json!({ "sketch": "Sketch 1", "profiles": "all", "distance": "0" }),
            "distance: ",
        ),
        (
            json!({ "sketch": "Sketch 1", "profiles": "all", "distance": "90 deg" }),
            "distance: ",
        ),
        (
            json!({ "sketch": "Sketch 1", "profiles": "all", "extent": "two_sides",
                    "second_distance": "-2" }),
            "second_distance: ",
        ),
        (
            json!({ "sketch": "Sketch 1", "profiles": "all", "taper": "90 deg" }),
            "taper: ",
        ),
        (
            json!({ "sketch": "Sketch 1", "profiles": [2] }),
            "the sketch has 2 regions: index 2 is past them",
        ),
        (
            json!({ "sketch": "Sketch 1", "profiles": [["100", "100"]] }),
            "no region of the sketch has the point (100, 100) inside",
        ),
        (
            json!({ "sketch": "Sketch 1", "profiles": [["1e12", "0"]] }),
            "the point's x: ",
        ),
        (
            json!({ "sketch": "Sketch 1", "profiles": "all", "extent": "through_all" }),
            "through_all goes with operation cut only",
        ),
        (
            json!({ "sketch": "Sketch 1", "profiles": "all", "exclude": ["Body 1"] }),
            "exclude goes with operation join, cut or intersect",
        ),
        (
            json!({ "sketch": "Sketch 1", "profiles": "all", "operation": "cut",
                    "exclude": ["Body 9"] }),
            "no body is named \"Body 9\"",
        ),
        (
            json!({ "sketch": "Extrude 1", "profiles": "all" }),
            "\"Extrude 1\" isn't a sketch",
        ),
        (
            json!({ "sketch": "Sketch 9", "profiles": "all" }),
            "no feature is named \"Sketch 9\"",
        ),
    ] {
        wrong_edit(&mut varde, &requests, "extrude", arguments, why);
    }
    assert_eq!(
        varde
            .screen
            .doc()
            .unwrap()
            .editor
            .document()
            .features()
            .len(),
        2
    );
}

/// A new feature goes where the user's would: at the rollback marker.
#[test]
fn a_new_feature_goes_at_the_rollback_marker() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_rollback",
        json!({ "before": "Extrude 1" }),
    );
    edit_data(reply);
    let (disc, _) = disc_and_plate(&varde);
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "extrude",
        json!({ "sketch": "Sketch 1", "profiles": [disc], "distance": "3 mm" }),
    );
    assert_eq!(state, McpCallState::Applied);
    let data = edit_data(reply);
    assert_eq!(data["feature"], "Extrude 2");
    // Built: it's before the marker; Extrude 1 isn't.
    assert_eq!(data["not_built"], json!(["Extrude 1"]));
    assert_eq!(feature_name(&varde, 1), "Extrude 2");
    assert_eq!(feature_name(&varde, 2), "Extrude 1");
    let document = varde.screen.doc().unwrap().editor.document();
    assert_eq!(document.rollback(), Some(document.features()[2].id));
}

/// A sketch on XY after the example: a rectangle from (10, 0) to
/// (20, 10) and a line from (0, 0) to (0, 10), the ids of the line and
/// of the rectangle's first point.
fn with_revolve_sketch(varde: &mut Varde, requests: &Lane) -> (u32, u32) {
    let doc = varde.screen.doc_mut().unwrap();
    let plane = varde_document::Plane::Origin(varde_document::OriginPlane::XY);
    doc.apply(doc.editor.document().add_sketch(plane));
    let sketch = doc.editor.document().features().last().unwrap().id;
    let mut drawn = varde_sketch::Sketch::default();
    let corners = [(10.0, 0.0), (20.0, 0.0), (20.0, 10.0), (10.0, 10.0)]
        .map(|(x, y)| drawn.add_point(glam::DVec2::new(x, y)).unwrap());
    for k in 0..4 {
        let line = varde_sketch::Curve::Line {
            start: corners[k],
            end: corners[(k + 1) % 4],
        };
        drawn.add_curve(line, false).unwrap();
    }
    let [a, b] =
        [(0.0, 0.0), (0.0, 10.0)].map(|(x, y)| drawn.add_point(glam::DVec2::new(x, y)).unwrap());
    let axis = drawn
        .add_curve(varde_sketch::Curve::Line { start: a, end: b }, false)
        .unwrap();
    doc.apply(Command::SetSketch {
        feature: sketch,
        sketch: Box::new(drawn),
    });
    doc.sync();
    regenerate(varde, requests);
    (axis.get(), corners[0].get())
}

fn revolve_of(feature: &varde_document::Feature) -> varde_document::Revolve {
    match &feature.kind {
        varde_document::FeatureKind::Revolve(revolve) => revolve.clone(),
        other => panic!("not a revolve: {other:?}"),
    }
}

/// `revolve` with the panel's defaults (a full turn, a new body) about
/// the sketch's axis, and with its values given about a line of the
/// sketch; what the panel wouldn't take refused.
#[test]
fn revolve_adds_about_an_axis_or_a_line() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let (line, point) = with_revolve_sketch(&mut varde, &requests);
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "revolve",
        json!({ "sketch": "Sketch 2", "profiles": "all", "axis": "y" }),
    );
    assert_eq!(state, McpCallState::Applied);
    let feature = feature_at(&varde, 3);
    assert_eq!(
        edit_data(reply),
        json!({
            "feature": "Revolve 1", "id": feature.id.get(), "bodies": ["Body 2"],
            "result": "applied"
        })
    );
    assert_eq!(varde.mcp.log[0].args, "Sketch 2, every region, about y");
    let revolve = revolve_of(&feature);
    assert_eq!(revolve.axis, varde_document::AxisLine::SketchY);
    assert_eq!(revolve.extent, varde_document::Turn::Full);
    assert_eq!(revolve.regions.len(), 1);

    let (reply, _) = edit(
        &mut varde,
        &requests,
        "revolve",
        json!({ "sketch": "Sketch 2", "profiles": [["15", "5"]], "axis": { "line": line },
                "extent": "two_sides", "angle": "30 deg", "flip": true, "operation": "join" }),
    );
    edit_data(reply);
    let revolve = revolve_of(&feature_at(&varde, 4));
    let varde_document::AxisLine::Curve(id) = revolve.axis else {
        panic!("{:?}", revolve.axis);
    };
    assert_eq!(id.get(), line);
    let varde_document::Turn::TwoSides(first, second) = &revolve.extent else {
        panic!("{:?}", revolve.extent);
    };
    assert!((first.value - 30f64.to_radians()).abs() < 1e-12);
    // The panel's second angle to begin with: a quarter turn.
    assert!((second.value - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert!(revolve.flip);

    for (arguments, why) in [
        (
            json!({ "sketch": "Sketch 2", "profiles": "all", "axis": "y",
                    "extent": "one_side", "angle": "400 deg" }),
            "angle: ",
        ),
        (
            json!({ "sketch": "Sketch 2", "profiles": "all", "axis": "y",
                    "extent": "one_side", "angle": "5 mm" }),
            "angle: ",
        ),
        (
            json!({ "sketch": "Sketch 2", "profiles": "all", "axis": { "line": 9999 } }),
            "the sketch has no curve 9999",
        ),
        (
            json!({ "sketch": "Sketch 2", "profiles": "all", "axis": { "line": point } }),
            "the sketch has no curve",
        ),
    ] {
        wrong_edit(&mut varde, &requests, "revolve", arguments, why);
    }
}

/// The key of a straight edge of the example's plate along x at z = 0,
/// in the sketch plane, from `body_topology`.
fn bottom_edge_key(varde: &Varde) -> String {
    let listing = data(
        varde,
        &topology_call(json!({ "body": "Body 1", "edges": { "direction": "x" }, "limit": 50 })),
    );
    let edge = (listing["edges"]["items"].as_array().unwrap().iter())
        .find(|edge| (edge["ends"].as_array().unwrap().iter()).all(|end| number(&end[2]) == 0.0))
        .unwrap();
    edge["key"].as_str().unwrap().to_owned()
}

/// A revolve about a model edge by its key: taken while the edge is
/// there where the key says, at its generation or after; a key that
/// isn't one, or whose edge moved, refused saying to ask again.
#[test]
fn revolve_takes_an_edge_by_key_and_refuses_a_bad_or_stale_one() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let (disc, _) = disc_and_plate(&varde);
    let key = bottom_edge_key(&varde);
    wrong_edit(
        &mut varde,
        &requests,
        "revolve",
        json!({ "sketch": "Sketch 1", "profiles": [disc], "axis": { "edge": "not a key" } }),
        "that isn't a key Varde gave: ask body_topology again",
    );
    // Another generation, the edge where it was: taken.
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_opacity",
        json!({ "body": "Body 1", "opacity": 0.5 }),
    );
    edit_data(reply);
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "revolve",
        json!({ "sketch": "Sketch 1", "profiles": [disc], "axis": { "edge": key },
                "extent": "one_side", "angle": "90 deg" }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let revolve = revolve_of(&feature_at(&varde, 2));
    let varde_document::AxisLine::Edge(edge) = revolve.axis else {
        panic!("{:?}", revolve.axis);
    };
    assert_eq!(
        Some(edge),
        key::decode(&key).ok().and_then(|key| match key.item {
            Ref::Edge(edge) => Some(edge),
            _ => None,
        })
    );
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    // The plate made symmetric: its bottom edge moved down, the key's
    // point no longer on it.
    let doc = varde.screen.doc_mut().unwrap();
    let plate = doc.editor.document().features()[1].clone();
    let mut extrude = extrude_of(&plate);
    let varde_document::Extent::OneSide(distance) = extrude.extent.clone() else {
        panic!();
    };
    extrude.extent = varde_document::Extent::Symmetric(distance);
    doc.apply(Command::SetFeature {
        feature: plate.id,
        kind: Box::new(extrude.into()),
    });
    doc.sync();
    regenerate(&mut varde, &requests);
    wrong_edit(
        &mut varde,
        &requests,
        "revolve",
        json!({ "sketch": "Sketch 1", "profiles": [disc], "axis": { "edge": key } }),
        "that edge isn't in the model any more: ask body_topology again",
    );
    // A face's key isn't an edge's.
    let doc = varde.screen.doc().unwrap();
    let face = Key::new(
        doc.id,
        doc.editor.generation(),
        Ref::Face(FaceRef {
            body: doc.editor.document().bodies()[0].id,
            key: FaceKey {
                feature: plate.id.get(),
                part: PartKey::EndCap,
                instance: 0,
            },
            near: glam::DVec3::ZERO,
        }),
    )
    .encode();
    wrong_edit(
        &mut varde,
        &requests,
        "revolve",
        json!({ "sketch": "Sketch 1", "profiles": [disc], "axis": { "edge": face } }),
        "not an edge",
    );
}

/// `combine` with the panel's defaults, a union using the tools up, and
/// with its values given; bodies it can't name refused.
#[test]
fn combine_adds_with_defaults_or_given_values() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let (disc, _) = disc_and_plate(&varde);
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "extrude",
        json!({ "sketch": "Sketch 1", "profiles": [disc], "distance": "20" }),
    );
    edit_data(reply);
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "combine",
        json!({ "target": "Body 1", "tools": ["Body 2"] }),
    );
    assert_eq!(state, McpCallState::Applied);
    let feature = feature_at(&varde, 3);
    assert_eq!(
        edit_data(reply),
        json!({ "feature": "Combine 1", "id": feature.id.get(), "bodies": [], "result": "applied" })
    );
    assert_eq!(varde.mcp.log[1].args, "Body 1 with Body 2");
    let bodies: Vec<_> = (varde
        .screen
        .doc()
        .unwrap()
        .editor
        .document()
        .bodies()
        .iter())
    .map(|body| body.id)
    .collect();
    assert_eq!(
        feature.kind,
        varde_document::FeatureKind::Combine(varde_document::Combine {
            target: bodies[0],
            tools: vec![bodies[1]],
            op: varde_document::BodyOp::Union,
            keep_tools: false,
        })
    );
    undo(&mut varde);
    regenerate(&mut varde, &requests);
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "combine",
        json!({ "target": "Body 2", "tools": ["Body 1"], "operation": "subtract",
                "keep_tools": true }),
    );
    edit_data(reply);
    assert_eq!(
        feature_at(&varde, 3).kind,
        varde_document::FeatureKind::Combine(varde_document::Combine {
            target: bodies[1],
            tools: vec![bodies[0]],
            op: varde_document::BodyOp::Subtract,
            keep_tools: true,
        })
    );
    for (arguments, why) in [
        (
            json!({ "target": "Body 1", "tools": ["Body 9"] }),
            "no body is named \"Body 9\"",
        ),
        (
            json!({ "target": "Body 1", "tools": ["Body 1", "Body 2"] }),
            "the target can't be one of the tools",
        ),
    ] {
        wrong_edit(&mut varde, &requests, "combine", arguments, why);
    }
    // Rolled back before Extrude 2, Body 2 can't be named.
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_rollback",
        json!({ "before": "Extrude 2" }),
    );
    edit_data(reply);
    wrong_edit(
        &mut varde,
        &requests,
        "combine",
        json!({ "target": "Body 1", "tools": ["Body 2"] }),
        "\"Body 2\" is made after where the combine goes",
    );
}

/// A key re-encoded with its edge's point `near`, at `varde`'s design's
/// generation now.
fn key_with_near(varde: &Varde, key: &str, near: glam::DVec3) -> String {
    let doc = varde.screen.doc().unwrap();
    let Ref::Edge(mut edge) = key::decode(key).unwrap().item else {
        panic!("not an edge's key");
    };
    edge.near = near;
    Key::new(doc.id, doc.editor.generation(), Ref::Edge(edge)).encode()
}

/// A key's point is checked on its edge whatever its generation: one
/// not a number, or off the edge, isn't stored in the revolve.
#[test]
fn an_edge_key_whose_point_is_off_its_edge_is_refused() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let (disc, _) = disc_and_plate(&varde);
    let key = bottom_edge_key(&varde);
    for near in [
        glam::DVec3::NAN,
        glam::DVec3::new(f64::INFINITY, 0.0, 0.0),
        glam::DVec3::new(1e30, 0.0, 0.0),
        glam::DVec3::new(0.0, 500.0, 0.0),
    ] {
        let bad = key_with_near(&varde, &key, near);
        wrong_edit(
            &mut varde,
            &requests,
            "revolve",
            json!({ "sketch": "Sketch 1", "profiles": [disc], "axis": { "edge": bad } }),
            "that edge isn't in the model any more",
        );
    }
}

/// A sketch made after where the feature goes (the rollback marker)
/// can't be its profile.
#[test]
fn a_sketch_past_the_marker_is_refused() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_rollback",
        json!({ "before": "Sketch 1" }),
    );
    edit_data(reply);
    for (tool, arguments) in [
        (
            "extrude",
            json!({ "sketch": "Sketch 1", "profiles": "all" }),
        ),
        (
            "revolve",
            json!({ "sketch": "Sketch 1", "profiles": "all", "axis": "y" }),
        ),
    ] {
        wrong_edit(
            &mut varde,
            &requests,
            tool,
            arguments,
            "\"Sketch 1\" is made after where the",
        );
    }
}

/// A tool named twice, or two naming one body, is refused, not dropped.
#[test]
fn combine_refuses_a_tool_named_twice() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let (disc, _) = disc_and_plate(&varde);
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "extrude",
        json!({ "sketch": "Sketch 1", "profiles": [disc], "distance": "20" }),
    );
    edit_data(reply);
    wrong_edit(
        &mut varde,
        &requests,
        "combine",
        json!({ "target": "Body 1", "tools": ["Body 2", "Body 2"] }),
        "\"Body 2\" is named twice",
    );
}

/// A client edit coming while the popup shows cancelled doesn't wait
/// on it: the popup asks about that edit, its buttons back, and Apply
/// yours applies it after the user's work.
#[test]
fn an_edit_while_the_popup_shows_cancelled_takes_it_over() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", Some("Claude Code"));
    edit_extrude(&mut varde);
    extrude_distance(&mut varde, "8 mm");
    let first = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Sketch 1", "Profile"),
    );
    let cancelled = session::Event::Cancelled {
        session: SessionId::for_tests("a session"),
        request: Id::Number(1),
    };
    feed(&mut varde, Event::Session(cancelled));
    assert!(
        first
            .try_recv()
            .unwrap()
            .text
            .contains("the user is editing")
    );
    assert!(ask(&varde).unwrap().cancelled);

    let second = calling(
        &mut varde,
        "a session",
        2,
        rename_feature("Sketch 1", "Outline"),
    );
    let shown = ask(&varde).unwrap();
    assert!(!shown.cancelled);
    assert!(shown.args.contains("Outline"), "{}", shown.args);
    let shown = on_screen(&varde);
    assert!(shown.iter().any(|text| text == "Apply yours"));
    assert!(shown.iter().any(|text| text.contains("Outline")));
    assert!(second.try_recv().is_err());

    panel(&mut varde, varde_view::McpPanel::ApplyYours);
    assert!(ask(&varde).is_none());
    assert_eq!(committed_distance(&varde), "8 mm");
    regenerate(&mut varde, &requests);
    assert_eq!(feature_name(&varde, 0), "Outline");
}

/// Changes the design as the user would, through the editor: a
/// parameter added, its regeneration asked for and not answered.
fn users_change(varde: &mut Varde, name: &str) {
    let doc = varde.screen.doc_mut().unwrap();
    doc.apply(Command::AddParam {
        name: name.to_owned(),
        text: "1 mm".to_owned(),
    });
    doc.sync();
}

/// The name of the sketch made last.
fn last_sketch(varde: &Varde) -> String {
    let document = varde.screen.doc().unwrap().editor.document();
    (document.features().iter().rev())
        .find(|feature| matches!(feature.kind, varde_document::FeatureKind::Sketch { .. }))
        .unwrap()
        .name
        .clone()
}

/// A query coming while the user's sketch edit waits on the solver
/// waits for it to be committed and for its model, then sees it.
#[test]
fn a_query_waits_for_the_users_sketch_edit_and_its_model() {
    let (mut varde, requests, mut lane) = sketching();
    let sketch = last_sketch(&varde);
    place_point(&mut varde, 1.0, 2.0);
    let reply = calling(&mut varde, "a session", 1, feature(&sketch));
    assert!(reply.try_recv().is_err());
    assert_eq!(varde.mcp.log[0].state, McpCallState::Waiting);
    solve(&mut varde, &mut lane);
    assert_eq!(points(&varde), 1);
    // Committed, its model not in yet.
    assert!(reply.try_recv().is_err());
    regenerate(&mut varde, &requests);
    let listed = edit_data(reply.try_recv().unwrap());
    assert_eq!(listed["settings"]["points"], json!(1));
    assert_eq!(varde.mcp.log[0].state, McpCallState::Answered);
    assert!(!varde.screen.doc().unwrap().mcp_waiting());
}

/// A query coming while regeneration runs waits for its answer, and
/// answers with that model's failures; the design summary's model is
/// current.
#[test]
fn a_query_waits_for_regeneration_and_says_its_failures() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    users_change(&mut varde, "width");
    let reply = calling(
        &mut varde,
        "a session",
        1,
        Call::DesignSummary(Page::SUMMARY),
    );
    assert!(reply.try_recv().is_err());
    let extrude = feature_id(&varde, 1);
    let id = varde.screen.doc().unwrap().id;
    for request in requests.take() {
        let mut response = handle(request);
        if let Response::Regenerated(Regenerated { failed, .. }) = &mut response {
            failed.push(varde_regen::FeatureFailure {
                feature: extrude,
                message: "the profile is gone".to_owned(),
                geometry: None,
            });
        }
        let _ = varde.update(Message::Doc(id, crate::message::ForDoc::Computed(response)));
    }
    let summary = edit_data(reply.try_recv().unwrap());
    assert_eq!(
        summary["timeline"]["items"][1]["failure"],
        json!("the profile is gone")
    );
    assert_eq!(summary["model"]["current"], json!(true));
    assert_eq!(summary["parameters"]["items"][0]["name"], json!("width"));
}

/// A query coming after a client's edit still waiting for the model
/// waits for that edit to apply, and sees it.
#[test]
fn a_query_after_a_waiting_edit_sees_it() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    users_change(&mut varde, "width");
    let edit = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Base"),
    );
    let query = calling(
        &mut varde,
        "a session",
        2,
        Call::DesignSummary(Page::SUMMARY),
    );
    assert!(edit.try_recv().is_err() && query.try_recv().is_err());
    // Its names wait for the model too: nothing applied yet.
    assert_eq!(feature_name(&varde, 1), "Extrude 1");
    regenerate(&mut varde, &requests);
    assert_eq!(
        edit_data(edit.try_recv().unwrap())["result"],
        json!("applied")
    );
    let summary = edit_data(query.try_recv().unwrap());
    assert_eq!(summary["timeline"]["items"][1]["name"], json!("Base"));
    assert_eq!(summary["model"]["current"], json!(true));
}

/// A query waits only for its own session's earlier edits: another
/// session's, applied as the query is taken up, doesn't hold it for that
/// edit's model.
#[test]
fn a_query_waits_only_for_its_own_sessions_edits() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    started(&mut varde, "another", None);
    users_change(&mut varde, "width");
    let edit = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Base"),
    );
    let query = calling(&mut varde, "another", 1, Call::DesignSummary(Page::SUMMARY));
    regenerate(&mut varde, &requests);
    assert_eq!(
        edit_data(edit.try_recv().unwrap())["result"],
        json!("applied")
    );
    // Answered on the model of the user's change, the edit's still to
    // come.
    let summary = edit_data(query.try_recv().unwrap());
    assert_eq!(summary["model"]["current"], json!(false));
}

/// A query and an edit waiting 30 s for the model are answered that it
/// took too long, the edit not applied.
#[test]
fn calls_waiting_30_s_for_the_model_time_out() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    users_change(&mut varde, "width");
    let came = iced::time::Instant::now();
    let query = calling(
        &mut varde,
        "a session",
        1,
        Call::DesignSummary(Page::SUMMARY),
    );
    let edit = calling(
        &mut varde,
        "a session",
        2,
        rename_feature("Extrude 1", "Base"),
    );
    let late = came + waiting::BOUND + std::time::Duration::from_secs(1);
    let _ = varde.update(Message::Tick(late));
    assert_eq!(
        bare(query.try_recv().unwrap()),
        ToolReply::error(waiting::TOOK_TOO_LONG)
    );
    assert_eq!(
        edit.try_recv().unwrap(),
        edit::not_applied(waiting::NOT_APPLIED_MODEL)
    );
    assert_eq!(varde.mcp.log[0].state, McpCallState::Failed);
    assert_eq!(varde.mcp.log[1].state, McpCallState::NotApplied);
    assert!(!varde.screen.doc().unwrap().mcp_waiting());
    regenerate(&mut varde, &requests);
    assert_eq!(feature_name(&varde, 1), "Extrude 1");
}

/// An edit's keys are resolved once the model has caught up with the
/// design: it waits, then applies.
#[test]
fn an_edits_key_is_resolved_once_the_model_caught_up() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let plate = varde.screen.doc().unwrap().editor.document().bodies()[0].id;
    let top = motion::face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    users_change(&mut varde, "width");
    let generation = varde.screen.doc().unwrap().editor.generation();
    let reply = calling(
        &mut varde,
        "a session",
        1,
        tool_call("offset_face", json!({ "faces": [top] })),
    );
    assert!(reply.try_recv().is_err());
    assert_eq!(varde.screen.doc().unwrap().editor.generation(), generation);
    regenerate(&mut varde, &requests);
    edit_data(reply.try_recv().unwrap());
    let document = varde.screen.doc().unwrap().editor.document();
    let varde_document::FeatureKind::OffsetFace(offset) = &document.features().last().unwrap().kind
    else {
        panic!("not an offset face");
    };
    assert_eq!(offset.faces[0].body, plate);
}

/// The user editing on while a query waits doesn't hold it: it answers
/// once the model of the design it asked about is in, saying a newer
/// one is still to come.
#[test]
fn a_query_answers_the_design_it_asked_about_as_the_user_edits_on() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    users_change(&mut varde, "width");
    let asked = varde.screen.doc().unwrap().editor.generation();
    let reply = calling(
        &mut varde,
        "a session",
        1,
        Call::DesignSummary(Page::SUMMARY),
    );
    users_change(&mut varde, "depth");
    let id = varde.screen.doc().unwrap().id;
    // The answer for the generation asked about alone.
    let first = requests.borrow_mut().remove(0);
    let _ = varde.update(Message::Doc(
        id,
        crate::message::ForDoc::Computed(handle(first)),
    ));
    let summary = edit_data(reply.try_recv().unwrap());
    let model = &summary["model"];
    assert_eq!(model["generation"], serde_json::to_value(asked).unwrap());
    assert_eq!(model["current"], json!(false));
    // The parameters are the design's as it is now.
    assert_eq!(summary["parameters"]["total"], json!(2));
    regenerate(&mut varde, &requests);
}

/// `body_topology` coming while regeneration runs waits for the model,
/// then asks for its edge facts of that model.
#[test]
fn body_topology_waits_for_the_model_then_its_edge_facts() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    users_change(&mut varde, "width");
    let reply = calling(
        &mut varde,
        "a session",
        1,
        Call::BodyTopology(topology("Body 1")),
    );
    assert!(!requests.borrow().iter().any(is_topology));
    regenerate(&mut varde, &requests);
    assert!(reply.try_recv().is_err());
    let generation = varde.screen.doc().unwrap().editor.generation();
    assert!((requests.borrow().iter()).any(|request| matches!(
        request,
        Request::Query { generation: asked, .. } if *asked == generation
    )));
    respond(&mut varde, &requests, is_topology);
    let listed = edit_data(reply.try_recv().unwrap());
    assert_eq!(listed["model"]["current"], json!(true));
}

/// An edit taken up once the model is in, which closes the clean sketch
/// open and is then refused, has the model asked for again without the
/// sketch left out: the sketch closed is drawn in the model.
#[test]
fn an_edit_ready_then_refused_asks_for_the_model_of_what_it_closed() {
    use varde_view::Look;
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    edit(
        &mut varde,
        &requests,
        "add_parameter",
        json!({ "name": "w", "value": "5 mm" }),
    );
    let doc = varde.screen.doc_mut().unwrap();
    let extrude = doc.editor.document().features()[1].clone();
    let varde_document::FeatureKind::Extrude(mut extrude_kind) = extrude.kind else {
        panic!("the example's second feature is an extrude");
    };
    let ask = varde_expr::Ask::length(varde_expr::LengthUnit::Mm, 1e6)
        .with_params(doc.editor.document().params_resolved());
    extrude_kind.extent =
        varde_document::Extent::OneSide(varde_expr::Value::new("w", &ask).unwrap());
    doc.apply(Command::SetFeature {
        feature: extrude.id,
        kind: Box::new(varde_document::FeatureKind::Extrude(extrude_kind)),
    });
    doc.sync();
    regenerate(&mut varde, &requests);
    let sketch = feature_id(&varde, 0);
    look(&mut varde, Look::EditFeature(sketch));
    regenerate(&mut varde, &requests);
    users_change(&mut varde, "depth");
    let reply = calling(
        &mut varde,
        "a session",
        1,
        tool_call("set_parameter", json!({ "name": "w", "value": "5 deg" })),
    );
    assert!(reply.try_recv().is_err());
    assert!(anything_open(&varde));
    // The model of the user's change alone.
    let id = varde.screen.doc().unwrap().id;
    for request in requests.take() {
        let _ = varde.update(Message::Doc(
            id,
            crate::message::ForDoc::Computed(handle(request)),
        ));
    }
    let refused = reply.try_recv().unwrap();
    assert!(refused.is_error, "{}", refused.text);
    assert!(!anything_open(&varde));
    assert!(
        (requests.borrow().iter())
            .any(|request| matches!(request, Request::Regenerate { exclude: None, .. })),
        "the model wasn't asked for again"
    );
    regenerate(&mut varde, &requests);
}

/// An edit queued between two of the user's sketch edits applies
/// between them, though its model comes only after the first is
/// committed: the undo order is the order they came in.
#[test]
fn an_edit_queued_between_the_users_sketch_edits_keeps_its_place() {
    let (mut varde, requests, mut lane) = sketching();
    place_point(&mut varde, 1.0, 2.0);
    let reply = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Base"),
    );
    place_point(&mut varde, 5.0, 6.0);
    for _ in 0..4 {
        solve(&mut varde, &mut lane);
        regenerate(&mut varde, &requests);
    }
    assert_eq!(
        edit_data(reply.try_recv().unwrap())["result"],
        json!("applied")
    );
    assert_eq!(
        (feature_name(&varde, 1).as_str(), points(&varde)),
        ("Base", 2)
    );
    // The second point came last: undone first.
    undo(&mut varde);
    assert_eq!(
        (feature_name(&varde, 1).as_str(), points(&varde)),
        ("Base", 1)
    );
    undo(&mut varde);
    assert_eq!(
        (feature_name(&varde, 1).as_str(), points(&varde)),
        ("Extrude 1", 1)
    );
}

/// An edit at its turn waiting for the model isn't pushed behind the
/// user's sketch edits queued after it, as it would be at each turn
/// while they keep sketching: it applies once its model is in, theirs
/// waiting for it.
#[test]
fn an_edit_is_not_pushed_back_by_the_user_sketching_on() {
    let (mut varde, requests, mut lane) = sketching();
    place_point(&mut varde, 1.0, 2.0);
    let reply = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Base"),
    );
    place_point(&mut varde, 5.0, 6.0);
    let id = varde.screen.doc().unwrap().id;
    // The first point alone is answered and committed.
    let response = lane.respond().unwrap();
    let _ = varde.update(Message::Doc(id, crate::message::ForDoc::Solved(response)));
    assert_eq!(points(&varde), 1);
    regenerate(&mut varde, &requests);
    assert_eq!(feature_name(&varde, 1), "Base");
    assert_eq!(points(&varde), 1);
    solve(&mut varde, &mut lane);
    regenerate(&mut varde, &requests);
    assert_eq!(points(&varde), 2);
    assert_eq!(
        edit_data(reply.try_recv().unwrap())["result"],
        json!("applied")
    );
}

/// An edit holding the queue for its model that times out lets the
/// user's sketch edits behind it go on.
#[test]
fn an_edit_holding_the_queue_timed_out_lets_it_go_on() {
    let (mut varde, _requests, mut lane) = sketching();
    place_point(&mut varde, 1.0, 2.0);
    let came = iced::time::Instant::now();
    let reply = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Base"),
    );
    place_point(&mut varde, 5.0, 6.0);
    solve(&mut varde, &mut lane);
    // Held for the model of the first point: the second waits.
    assert_eq!(points(&varde), 1);
    let late = came + waiting::BOUND + std::time::Duration::from_secs(1);
    let _ = varde.update(Message::Tick(late));
    // It waited for the model, not behind the user's sketch edits.
    assert_eq!(
        bare(reply.try_recv().unwrap()),
        edit::not_applied(waiting::NOT_APPLIED_MODEL)
    );
    solve(&mut varde, &mut lane);
    assert_eq!(points(&varde), 2);
    assert_eq!(feature_name(&varde, 1), "Extrude 1");
    assert!(!varde.screen.doc().unwrap().proposing());
}

/// An edit holding the queue for its model doesn't hold the user's own
/// change: it's made at once, before the client's in the undo history.
#[test]
fn an_edit_holding_the_queue_lets_the_users_change_go_first() {
    let (mut varde, requests, mut lane) = sketching();
    place_point(&mut varde, 1.0, 2.0);
    let reply = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Base"),
    );
    solve(&mut varde, &mut lane);
    let doc = varde.screen.doc().unwrap();
    assert!(doc.proposing() && !doc.users_proposing());
    let body = doc.editor.document().bodies()[0].id;
    let visible = |varde: &Varde| {
        let document = varde.screen.doc().unwrap().editor.document();
        document.body(body).unwrap().visible
    };
    ui(
        &mut varde,
        varde_view::Message::Edit(varde_view::Edit::ToggleVisible(body)),
    );
    assert!(!visible(&varde));
    assert_eq!(feature_name(&varde, 1), "Extrude 1");
    regenerate(&mut varde, &requests);
    assert_eq!(feature_name(&varde, 1), "Base");
    assert_eq!(
        edit_data(reply.try_recv().unwrap())["result"],
        json!("applied")
    );
    // The client's came last: undone first.
    undo(&mut varde);
    assert_eq!(
        (feature_name(&varde, 1).as_str(), visible(&varde)),
        ("Extrude 1", false)
    );
    undo(&mut varde);
    assert!(visible(&varde));
    assert_eq!(points(&varde), 1);
}

/// Calls `call` in `session`, as request `id`, come at `came`: where its
/// reply comes.
fn calling_at(
    varde: &mut Varde,
    session: &str,
    id: i64,
    call: Call,
    came: iced::time::Instant,
) -> mpsc::Receiver<ToolReply> {
    let (sender, receiver) = mpsc::sync_channel(1);
    let event = Event::Call {
        session: SessionId::for_tests(session),
        id: Id::Number(id),
        call,
        reply: Reply::for_tests(sender, came),
    };
    feed(varde, event);
    receiver
}

/// The extrude panel open with a distance typed: an edit asks.
fn asking(session: &str) -> (Varde, Lane) {
    let (mut varde, requests) = listening_with();
    started(&mut varde, session, None);
    edit_extrude(&mut varde);
    extrude_distance(&mut varde, "7 mm");
    (varde, requests)
}

/// The text an edit failed as an earlier one of its session wasn't
/// applied, `why`, starts with.
fn failed_behind(why: &str) -> String {
    format!("Not applied: an earlier edit of this session wasn't: {why}")
}

/// A session's edits past the one in flight and [`waiting::QUEUED_EDITS`]
/// behind it are refused at once; another session's aren't.
#[test]
fn edits_past_the_bound_are_refused() {
    let (mut varde, _requests) = asking("a session");
    let mut replies = Vec::new();
    for id in 0..=waiting::QUEUED_EDITS {
        let id = i64::try_from(id).unwrap() + 1;
        let to = format!("Name {id}");
        replies.push(calling(
            &mut varde,
            "a session",
            id,
            rename_feature("Sketch 1", &to),
        ));
    }
    assert!(replies.iter().all(|reply| reply.try_recv().is_err()));
    let refused = called(
        &mut varde,
        "a session",
        100,
        rename_feature("Sketch 1", "Late"),
    );
    assert_eq!(refused, edit::not_applied(waiting::TOO_MANY_QUEUED));
    assert_eq!(
        varde.mcp.log.back().unwrap().state,
        McpCallState::NotApplied
    );
    // Those queued wait on: the refusal fails none.
    assert!(replies.iter().all(|reply| reply.try_recv().is_err()));
    let other = calling(
        &mut varde,
        "b session",
        1,
        rename_feature("Sketch 1", "Other"),
    );
    assert!(other.try_recv().is_err());
}

/// An edit the editor refuses at its turn fails its session's edits
/// queued behind it with its reason; another session's go on.
#[test]
fn an_edit_refused_fails_its_sessions_edits_behind_it() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    edit(
        &mut varde,
        &requests,
        "add_parameter",
        json!({ "name": "w", "value": "5 mm" }),
    );
    // The extrude's distance is w.
    let doc = varde.screen.doc_mut().unwrap();
    let extrude = doc.editor.document().features()[1].clone();
    let varde_document::FeatureKind::Extrude(mut extrude_kind) = extrude.kind else {
        panic!("the example's second feature is an extrude");
    };
    let ask_w = varde_expr::Ask::length(varde_expr::LengthUnit::Mm, 1e6)
        .with_params(doc.editor.document().params_resolved());
    extrude_kind.extent =
        varde_document::Extent::OneSide(varde_expr::Value::new("w", &ask_w).unwrap());
    doc.apply(Command::SetFeature {
        feature: extrude.id,
        kind: Box::new(varde_document::FeatureKind::Extrude(extrude_kind)),
    });
    doc.sync();
    regenerate(&mut varde, &requests);
    // The rename field open: the edits wait on the popup.
    let sketch = feature_id(&varde, 0);
    look(
        &mut varde,
        varde_view::Look::StartRename(varde_document::Named::Feature(sketch)),
    );
    look(
        &mut varde,
        varde_view::Look::RenameInput("Outline".to_owned()),
    );
    let set = tool_call("set_parameter", json!({ "name": "w", "value": "5 deg" }));
    let first = calling(&mut varde, "a session", 11, set);
    let second = calling(
        &mut varde,
        "a session",
        12,
        rename_feature("Extrude 1", "Base"),
    );
    let other = calling(
        &mut varde,
        "b session",
        1,
        rename_feature("Extrude 1", "Top"),
    );
    assert!(ask(&varde).is_some());
    panel(&mut varde, varde_view::McpPanel::ApplyYours);
    regenerate(&mut varde, &requests);
    let first = bare(first.try_recv().unwrap());
    assert!(
        first.text.contains("Extrude 1 would be in error"),
        "{}",
        first.text
    );
    let why = first.text["Not applied: ".len()..]
        .strip_suffix(". The design is unchanged")
        .unwrap();
    let second = second.try_recv().unwrap();
    assert!(second.is_error);
    assert!(
        second.text.starts_with(&failed_behind(why)),
        "{}",
        second.text
    );
    assert_eq!(feature_name(&varde, 0), "Outline");
    assert_eq!(
        edit_data(other.try_recv().unwrap())["result"],
        json!("applied")
    );
    assert_eq!(feature_name(&varde, 1), "Top");
}

/// Keep editing fails the session's edits behind the one it refuses.
#[test]
fn keep_editing_fails_the_sessions_edits_behind() {
    let (mut varde, _requests) = asking("a session");
    let first = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Sketch 1", "Profile"),
    );
    let second = calling(
        &mut varde,
        "a session",
        2,
        rename_feature("Extrude 1", "Base"),
    );
    panel(&mut varde, varde_view::McpPanel::KeepEditing);
    assert!(
        first
            .try_recv()
            .unwrap()
            .text
            .contains("the user is editing Extrude 1")
    );
    let second = second.try_recv().unwrap();
    assert!(
        second
            .text
            .starts_with(&failed_behind("the user is editing Extrude 1")),
        "{}",
        second.text
    );
    assert!(!varde.screen.doc().unwrap().mcp_waiting());
    assert_eq!(feature_name(&varde, 1), "Extrude 1");
}

/// An edit timing out behind the user's sketch edits fails its
/// session's edits behind it, not yet at the bound themselves, and
/// they leave the proposals' queue.
#[test]
fn an_edit_timed_out_fails_its_sessions_edits_behind() {
    let (mut varde, _requests, mut lane) = sketching();
    place_point(&mut varde, 1.0, 2.0);
    let came = iced::time::Instant::now();
    let first = calling_at(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Base"),
        came,
    );
    let later = came + std::time::Duration::from_secs(10);
    let second = calling_at(
        &mut varde,
        "a session",
        2,
        rename_feature("Sketch 1", "P"),
        later,
    );
    let late = came + waiting::BOUND + std::time::Duration::from_secs(1);
    let _ = varde.update(Message::Tick(late));
    assert_eq!(
        bare(first.try_recv().unwrap()),
        edit::not_applied(waiting::NOT_APPLIED_BUSY)
    );
    let second = second.try_recv().unwrap();
    let why = "the design was busy with the user's sketch edits for over 30 s";
    assert!(
        second.text.starts_with(&failed_behind(why)),
        "{}",
        second.text
    );
    assert!(!varde.screen.doc().unwrap().mcp_waiting());
    solve(&mut varde, &mut lane);
    assert!(!varde.screen.doc().unwrap().proposing());
    assert_eq!(feature_name(&varde, 1), "Extrude 1");
}

/// An edit applied, its regeneration failing, fails none behind it.
#[test]
fn an_applied_edit_failing_fails_none_behind() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let first = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Base"),
    );
    let second = calling(
        &mut varde,
        "a session",
        2,
        rename_feature("Sketch 1", "Profile"),
    );
    let extrude = feature_id(&varde, 1);
    answer_failing(varde.screen.doc_mut().unwrap(), &requests, extrude);
    let _ = varde.update(Message::Tick(iced::time::Instant::now()));
    let first = edit_data(first.try_recv().unwrap());
    assert!(first["failures"].is_array(), "{first}");
    regenerate(&mut varde, &requests);
    assert_eq!(
        edit_data(second.try_recv().unwrap())["result"],
        json!("applied")
    );
    assert_eq!(feature_name(&varde, 0), "Profile");
}

/// A query answered while an edit of its session waits on the popup
/// says what the edit is; another session's doesn't.
#[test]
fn a_query_during_the_popup_says_its_sessions_edit_waits() {
    let (mut varde, _requests) = asking("a session");
    let edit = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Sketch 1", "Profile"),
    );
    let summary = Call::DesignSummary(Page::SUMMARY);
    let reply = called(&mut varde, "a session", 2, summary.clone());
    let pending = reply.structured.as_ref().unwrap()["pending_edit"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(pending.starts_with("rename "), "{pending}");
    assert!(pending.contains("Profile"), "{pending}");
    assert!(pending.contains("editing Extrude 1"), "{pending}");
    assert!(reply.text.contains("pending_edit"), "{}", reply.text);
    let reply = called(&mut varde, "b session", 1, summary);
    assert!(reply.structured.unwrap().get("pending_edit").is_none());
    assert!(edit.try_recv().is_err());
}

/// An edit reaching its turn in the proposals' queue as a cancelled
/// popup gives way keeps its place there, before the user's sketch
/// edits queued after it.
#[test]
fn an_edit_at_its_turn_past_a_cancelled_popup_keeps_its_place() {
    let (mut varde, requests, mut lane) = sketching();
    place_point(&mut varde, 5.0, 5.0);
    let first = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Base"),
    );
    look(
        &mut varde,
        varde_view::Look::SelectTool(varde_view::Tool::Line),
    );
    // A line, the next one started from its end.
    place_point(&mut varde, 1.0, 2.0);
    place_point(&mut varde, 3.0, 2.0);
    let held = calling(
        &mut varde,
        "c session",
        1,
        rename_feature("Sketch 1", "Outline"),
    );
    place_point(&mut varde, 3.0, 4.0);
    // The point committed and its model in: the first asks, the line
    // goes to the solver.
    solve(&mut varde, &mut lane);
    regenerate(&mut varde, &requests);
    assert!(ask(&varde).is_some());
    let behind = calling(
        &mut varde,
        "b session",
        1,
        rename_feature("Extrude 1", "Top"),
    );
    let cancelled = session::Event::Cancelled {
        session: SessionId::for_tests("a session"),
        request: Id::Number(1),
    };
    feed(&mut varde, Event::Session(cancelled));
    assert!(first.try_recv().unwrap().is_error);
    assert!(ask(&varde).unwrap().cancelled);
    // The line committed, then its model: the held edit's turn, the
    // cancelled popup giving way to the one behind it, which queues
    // behind the user's next line. The held one keeps its place, ahead
    // of it, and asks in turn (the next line is started).
    solve(&mut varde, &mut lane);
    regenerate(&mut varde, &requests);
    let shown = ask(&varde).unwrap();
    assert!(!shown.cancelled);
    assert!(shown.args.contains("Outline"), "{}", shown.args);
    assert!(behind.try_recv().is_err());
    panel(&mut varde, varde_view::McpPanel::ApplyYours);
    for _ in 0..3 {
        solve(&mut varde, &mut lane);
        regenerate(&mut varde, &requests);
    }
    assert_eq!(
        edit_data(held.try_recv().unwrap())["result"],
        json!("applied")
    );
    assert_eq!(
        edit_data(behind.try_recv().unwrap())["result"],
        json!("applied")
    );
    assert_eq!(feature_name(&varde, 0), "Outline");
    assert_eq!(feature_name(&varde, 1), "Top");
}

/// An edit let go by the popup and refused at once fails its
/// session's edits let go with it, not only those still in the table.
#[test]
fn an_edit_let_go_by_the_popup_and_refused_fails_those_let_go_behind() {
    let (mut varde, _requests) = asking("b session");
    let asked = calling(
        &mut varde,
        "b session",
        1,
        rename_feature("Sketch 1", "Profile"),
    );
    let first = calling(&mut varde, "a session", 1, rename_feature("Nope", "X"));
    let second = calling(
        &mut varde,
        "a session",
        2,
        rename_feature("Extrude 1", "Base"),
    );
    assert!(first.try_recv().is_err(), "it waits behind the popup");
    panel(&mut varde, varde_view::McpPanel::KeepEditing);
    assert!(asked.try_recv().unwrap().is_error);
    let first = first.try_recv().unwrap();
    assert!(first.is_error, "{}", first.text);
    let second = second.try_recv().unwrap();
    assert!(
        second.text.starts_with(&failed_behind(&first.text)),
        "{}",
        second.text
    );
    assert!(ask(&varde).is_none());
    assert_eq!(feature_name(&varde, 1), "Extrude 1");
}

/// An edit naming what its session's edit waiting on the popup makes
/// waits for it, rather than being refused on the design before it.
#[test]
fn an_edit_behind_the_popup_is_made_after_the_one_asked() {
    let (mut varde, requests) = asking("a session");
    let first = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Sketch 1", "Profile"),
    );
    let second = calling(
        &mut varde,
        "a session",
        2,
        rename_feature("Profile", "Outline"),
    );
    assert!(second.try_recv().is_err());
    panel(&mut varde, varde_view::McpPanel::ApplyYours);
    for _ in 0..3 {
        regenerate(&mut varde, &requests);
    }
    assert_eq!(
        edit_data(first.try_recv().unwrap())["result"],
        json!("applied")
    );
    assert_eq!(
        edit_data(second.try_recv().unwrap())["result"],
        json!("applied")
    );
    assert_eq!(feature_name(&varde, 0), "Outline");
}

/// Edits let go by Keep editing: the first asks again, and the one
/// behind it, naming what the first makes, waits on the popup again
/// rather than being made on the design before it.
#[test]
fn an_edit_let_go_behind_one_asking_again_waits_for_it() {
    let (mut varde, requests) = asking("b session");
    let asked = calling(
        &mut varde,
        "b session",
        1,
        rename_feature("Extrude 1", "Base"),
    );
    let first = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Sketch 1", "Profile"),
    );
    let second = calling(
        &mut varde,
        "a session",
        2,
        rename_feature("Profile", "Outline"),
    );
    panel(&mut varde, varde_view::McpPanel::KeepEditing);
    assert!(asked.try_recv().unwrap().is_error);
    assert!(ask(&varde).unwrap().args.contains("Profile"));
    assert!(second.try_recv().is_err(), "{:?}", second.try_recv());
    panel(&mut varde, varde_view::McpPanel::ApplyYours);
    for _ in 0..3 {
        regenerate(&mut varde, &requests);
    }
    assert_eq!(
        edit_data(first.try_recv().unwrap())["result"],
        json!("applied")
    );
    assert_eq!(
        edit_data(second.try_recv().unwrap())["result"],
        json!("applied")
    );
    assert_eq!(feature_name(&varde, 0), "Outline");
}

/// An edit naming what its session's edit queued behind the user's
/// sketch edits makes isn't refused on arrival: it waits for it.
#[test]
fn an_edit_behind_a_queued_one_is_not_checked_on_arrival() {
    let (mut varde, requests, mut lane) = sketching();
    place_point(&mut varde, 1.0, 2.0);
    let first = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Base"),
    );
    let second = calling(&mut varde, "a session", 2, rename_feature("Base", "Top"));
    assert!(second.try_recv().is_err(), "{:?}", second.try_recv());
    for _ in 0..3 {
        solve(&mut varde, &mut lane);
        regenerate(&mut varde, &requests);
    }
    assert_eq!(
        edit_data(first.try_recv().unwrap())["result"],
        json!("applied")
    );
    assert_eq!(
        edit_data(second.try_recv().unwrap())["result"],
        json!("applied")
    );
    assert_eq!(feature_name(&varde, 1), "Top");
}

/// Edits of a session timing out in the same tick: the first says why,
/// those behind it that it failed say it was the earlier one.
#[test]
fn edits_timed_out_together_fail_behind_the_first() {
    let (mut varde, _requests, _lane) = sketching();
    place_point(&mut varde, 1.0, 2.0);
    let came = iced::time::Instant::now();
    let first = calling_at(
        &mut varde,
        "a session",
        1,
        rename_feature("Extrude 1", "Base"),
        came,
    );
    let second = calling_at(
        &mut varde,
        "a session",
        2,
        rename_feature("Sketch 1", "P"),
        came,
    );
    let late = came + waiting::BOUND + std::time::Duration::from_secs(1);
    let _ = varde.update(Message::Tick(late));
    assert_eq!(
        bare(first.try_recv().unwrap()),
        edit::not_applied(waiting::NOT_APPLIED_BUSY)
    );
    let second = second.try_recv().unwrap();
    assert!(
        second
            .text
            .starts_with(&failed_behind(waiting::NOT_APPLIED_BUSY)),
        "{}",
        second.text
    );
}

mod blend;
mod changes;
mod draw;
mod measure;
mod modify;
mod motion;
mod naming;
mod render;
mod revert;
mod set;
mod sketch;
mod sketches;
mod sweep;
mod undo;

/// A query's `pending_edit` stays within [`waiting::PENDING_BYTES`]
/// however long the edit's arguments, so a query's answer at its
/// budget still fits the cap with it.
#[test]
fn a_long_pending_edit_is_bounded() {
    let (mut varde, _requests) = asking("a session");
    let long = "\"".repeat(varde_mcp::tools::NAME_BYTES);
    let _edit = calling(
        &mut varde,
        "a session",
        1,
        rename_feature("Sketch 1", &long),
    );
    let reply = called(
        &mut varde,
        "a session",
        2,
        Call::DesignSummary(Page::SUMMARY),
    );
    let pending = reply.structured.as_ref().unwrap()["pending_edit"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(pending.starts_with("rename "), "{pending}");
    assert!(pending.contains("editing Extrude 1"), "{pending}");
    let json = serde_json::to_string(&pending).unwrap();
    assert!(json.len() <= waiting::PENDING_BYTES, "{}", json.len());
}

/// The settings' relay lines start a program a client can run later: not
/// one replaced while running, nor a Flatpak's sandboxed path, and an
/// AppImage by its own file rather than its mount.
#[test]
fn the_relay_path_is_one_that_runs_later() {
    use std::path::PathBuf;
    let path = |text: &str| PathBuf::from(text);
    assert_eq!(
        relay_exe(path("/usr/bin/varde"), true, None, false).as_deref(),
        Some("/usr/bin/varde")
    );
    assert_eq!(
        relay_exe(path("/usr/bin/varde (deleted)"), false, None, false),
        None
    );
    assert_eq!(relay_exe(path("/usr/bin/varde"), false, None, false), None);
    assert_eq!(relay_exe(path("/app/bin/varde"), true, None, true), None);
    assert_eq!(
        relay_exe(
            path("/tmp/.mount_vardeX/usr/bin/varde"),
            true,
            Some(path("/home/a/Varde.AppImage")),
            false
        )
        .as_deref(),
        Some("/home/a/Varde.AppImage")
    );
}

#[test]
fn the_mirrored_fractions_match_their_sources() {
    use varde_mcp::tools::{MAX_SATURATION, MIN_OPACITY};
    assert_eq!(
        MIN_OPACITY,
        f64::from(varde_document::Opacity::MIN.percent()) / 100.0
    );
    assert_eq!(
        MAX_SATURATION,
        f64::from(varde_document::Tint::MAX_SATURATION) / 100.0
    );
}

/// The answers that name [`waiting::BOUND`] or [`waiting::QUEUED_EDITS`]
/// say the number they are.
#[test]
fn waiting_texts_name_their_bounds() {
    let bound = format!("{} s", waiting::BOUND.as_secs());
    for text in [
        waiting::TOOK_TOO_LONG,
        waiting::NOT_APPLIED_BUSY,
        waiting::NOT_APPLIED_LATE,
        waiting::NOT_APPLIED_MODEL,
        waiting::NOT_APPLIED_SOLVER,
        waiting::NOT_DRAWN,
    ] {
        assert!(text.contains(&bound), "{text:?} doesn't say {bound}");
    }
    let queued = format!("{} edits", waiting::QUEUED_EDITS);
    assert!(waiting::TOO_MANY_QUEUED.starts_with(&queued));
}
