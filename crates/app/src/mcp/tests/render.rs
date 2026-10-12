//! The tool `render`, through `Varde::update`: the picture asked of the
//! viewport (framed, the highlight drawn selected), drawn by a stand-in
//! for its frame, encoded by the IO lane and answered as image content;
//! the bodies and keys checked; a picture that can't be drawn, one past
//! the bound, and one no frame comes for.

use std::rc::Rc;
use std::sync::Arc;

use serde_json::{Value, json};
use varde_view::PreviewRequest;

use super::motion::face_facing;
use super::*;
use crate::message::ForDoc;

/// [`listening_with`] in "a session", its IO lane's requests kept.
fn ready() -> (Varde, Lane, Rc<RefCell<Vec<varde_io::Request>>>) {
    let (mut varde, requests) = listening_with();
    let io = Rc::default();
    varde.files.io.ready(Box::new(Deferred(Rc::clone(&io))));
    started(&mut varde, "a session", None);
    (varde, requests, io)
}

/// Calls `render` with `arguments` in "a session": where its reply comes.
fn render(varde: &mut Varde, arguments: Value) -> mpsc::Receiver<ToolReply> {
    let id = (varde.mcp.log.back()).map_or(1, |logged| i64::try_from(logged.serial).unwrap() + 1);
    calling(varde, "a session", id, tool_call("render", arguments))
}

/// The one picture waiting to be drawn, under its call's tag.
fn drawing(varde: &Varde) -> (u64, Arc<PreviewRequest>) {
    let mut rendering = varde.screen.doc().unwrap().mcp.rendering();
    assert_eq!(rendering.len(), 1);
    rendering.remove(0)
}

/// A picture `width` by `height`, all one colour.
fn picture(width: u32, height: u32) -> varde_io::thumbnail::Image {
    let rgba = [30, 60, 90, 255].repeat((width * height) as usize);
    varde_io::thumbnail::Image::new(width, height, rgba).unwrap()
}

/// Draws the picture under `tag` as `image`, as the viewport's frame and
/// the app's task waiting on it would hand it back.
fn drawn(varde: &mut Varde, tag: u64, image: Option<varde_io::thumbnail::Image>) {
    let id = varde.screen.doc().unwrap().id;
    let _ = varde.update(Message::Doc(id, ForDoc::McpRendered(tag, image)));
}

/// The PNG the IO lane was asked to encode, answered as it answers.
fn encode(varde: &mut Varde, io: &Rc<RefCell<Vec<varde_io::Request>>>) {
    let sent = io.take();
    let [varde_io::Request::EncodePng { owner, tag, image }] = &sent[..] else {
        panic!("expected one PNG to encode, not {sent:?}");
    };
    let png = varde_io::thumbnail::encode_png(image).map_err(|e| e.to_string());
    let (owner, tag) = (*owner, *tag);
    super::io(
        varde,
        varde_io::Response::PngEncoded {
            owner,
            tag,
            result: png,
        },
    );
}

#[test]
fn a_render_is_drawn_encoded_and_answered_as_an_image() {
    let (mut varde, _requests, io) = ready();
    let reply = render(&mut varde, json!({}));
    assert!(reply.try_recv().is_err());
    let doc = varde.screen.doc().unwrap();
    assert!(doc.drawing_for_mcp() && doc.ticking());
    let (tag, request) = drawing(&varde);
    // The committed model, every body drawn, framed to the default size
    // on its long side from where Home looks.
    let doc = varde.screen.doc().unwrap();
    assert!(Arc::ptr_eq(&request.mesh, doc.feed.mesh()));
    assert!(request.opacity.iter().all(|&alpha| alpha == 1.0));
    let size = varde_mcp::tools::RENDER_SIZE;
    assert_eq!(request.shot.size[0], size);
    assert!(request.shot.size[1] < size);
    let home = varde_render::Camera::default();
    assert!(request.shot.camera.backward().distance(home.backward()) < 1e-6);
    assert!(request.selected_faces.is_empty());
    // For the viewport's next frame.
    let state = doc.state(
        false,
        varde_view::Mode::Light,
        varde_view::ViewOptions::default(),
        crate::Offers::default(),
    );
    assert!(
        state
            .renders
            .iter()
            .any(|shown| Arc::ptr_eq(shown, &request))
    );

    drawn(&mut varde, tag, Some(picture(4, 3)));
    let doc = varde.screen.doc().unwrap();
    assert!(doc.mcp.encoding(tag));
    assert!(!doc.drawing_for_mcp());
    assert!(reply.try_recv().is_err());
    encode(&mut varde, &io);
    let reply = reply.try_recv().unwrap();
    assert!(!reply.is_error, "{}", reply.text);
    let png = reply.png.clone().unwrap();
    assert_eq!(
        png,
        varde_io::thumbnail::encode_png(&picture(4, 3)).unwrap()
    );
    let data = bare(reply).structured.unwrap();
    assert_eq!(data["bodies"], json!(["Body 1"]), "{data}");
    assert_eq!(data["projection"], json!("orthographic"));
    assert_eq!(data["model"]["current"], json!(true));
    let row = varde.mcp.log.back().unwrap();
    assert_eq!(row.state, McpCallState::Answered);
    assert_eq!(row.tool, McpTool::Render);
    assert!(row.result.as_ref().unwrap().ends_with("PNG"), "{row:?}");
    assert!(!varde.screen.doc().unwrap().mcp_waiting());
}

#[test]
fn a_render_frames_as_asked() {
    let (mut varde, _requests, _io) = ready();
    let _reply = render(
        &mut varde,
        json!({ "from": "front", "projection": "perspective", "size": 200, "zoom": 2 }),
    );
    let (_, front) = drawing(&varde);
    let camera = front.shot.camera;
    assert_eq!(camera.projection(), varde_render::Projection::Perspective);
    assert!(camera.backward().distance(glam::Vec3::NEG_Y) < 1e-5);
    assert!(front.shot.size[0] <= 200 && front.shot.size[1] <= 200);
    // Yaw and pitch as typed angles: front is yaw -90 degrees.
    let _reply = render(
        &mut varde,
        json!({ "from": { "yaw": "-90 deg", "pitch": 0 } }),
    );
    let rendering = varde.screen.doc().unwrap().mcp.rendering();
    let angles = &rendering[1].1;
    let backward = angles.shot.camera.backward();
    assert!(backward.distance(glam::Vec3::NEG_Y) < 1e-5, "{backward}");
    // Zoomed in by 2: half the view across, the picture as large.
    let _reply = render(
        &mut varde,
        json!({ "from": "front", "projection": "perspective", "size": 200 }),
    );
    let rendering = varde.screen.doc().unwrap().mcp.rendering();
    let plain = &rendering[2].1;
    assert_eq!(plain.shot.size, front.shot.size);
    let ratio = front.shot.camera.view_height() / plain.shot.camera.view_height();
    assert!((ratio - 0.5).abs() < 1e-4, "{ratio}");
}

/// `render` with `arguments` refused at once, saying `why`.
fn refused_render(varde: &mut Varde, arguments: Value, why: &str) {
    let reply = render(varde, arguments).try_recv().unwrap();
    assert!(reply.is_error, "{}", reply.text);
    assert!(reply.text.contains(why), "{}", reply.text);
    assert!(varde.screen.doc().unwrap().mcp.rendering().is_empty());
}

#[test]
fn bodies_angles_and_keys_are_checked() {
    let (mut varde, requests, _io) = ready();
    refused_render(
        &mut varde,
        json!({ "bodies": ["Body 9"] }),
        "no body is named",
    );
    refused_render(
        &mut varde,
        json!({ "from": { "yaw": 0, "pitch": "100 deg" } }),
        "from.pitch",
    );
    refused_render(
        &mut varde,
        json!({ "from": { "yaw": "3 mm", "pitch": 0 } }),
        "from.yaw",
    );
    refused_render(
        &mut varde,
        json!({ "highlight": [{ "face": "nonsense" }] }),
        "highlight 0: that isn't a key",
    );
    // A highlight's key drawn selected.
    let top = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    let _reply = render(&mut varde, json!({ "highlight": [{ "face": top }] }));
    let (tag, request) = drawing(&varde);
    assert_eq!(request.selected_faces.len(), 1);
    drawn(&mut varde, tag, None);
    // A hidden body isn't drawn: named, it's refused; with none visible,
    // there's nothing to draw.
    let (hidden, _) = edit(
        &mut varde,
        &requests,
        "set_visible",
        json!({ "body": "Body 1", "visible": false }),
    );
    assert!(!hidden.is_error, "{}", hidden.text);
    let reply = render(&mut varde, json!({ "bodies": ["Body 1"] }))
        .try_recv()
        .unwrap();
    assert!(
        reply.text.contains("isn't in the model shown"),
        "{}",
        reply.text
    );
    let reply = render(&mut varde, json!({})).try_recv().unwrap();
    assert!(reply.text.contains("nothing to draw"), "{}", reply.text);
}

#[test]
fn a_picture_not_drawn_or_too_large_fails() {
    let (mut varde, _requests, io) = ready();
    let reply = render(&mut varde, json!({}));
    let (tag, _) = drawing(&varde);
    drawn(&mut varde, tag, None);
    assert_eq!(
        reply.try_recv().unwrap(),
        ToolReply::error(waiting::DRAW_FAILED)
    );
    assert!(io.borrow().is_empty());
    // A PNG past the bound.
    let reply = render(&mut varde, json!({}));
    let (tag, _) = drawing(&varde);
    drawn(&mut varde, tag, Some(picture(2, 2)));
    let owner = varde.screen.doc().unwrap().id.number();
    io.take();
    let big = vec![0; ToolReply::IMAGE_BYTES + 1];
    super::io(
        &mut varde,
        varde_io::Response::PngEncoded {
            owner,
            tag,
            result: Ok(big),
        },
    );
    let reply = reply.try_recv().unwrap();
    assert!(reply.is_error && reply.png.is_none());
    assert!(reply.text.contains("smaller size"), "{}", reply.text);
    // Another design's answer, or a late one, is dropped.
    super::io(
        &mut varde,
        varde_io::Response::PngEncoded {
            owner: owner + 1,
            tag,
            result: Ok(vec![1]),
        },
    );
    assert!(!varde.screen.doc().unwrap().mcp_waiting());
}

#[test]
fn a_render_no_frame_comes_for_times_out() {
    let (mut varde, _requests, _io) = ready();
    let came = iced::time::Instant::now();
    let reply = render(&mut varde, json!({}));
    let (tag, request) = drawing(&varde);
    let late = came + waiting::BOUND + std::time::Duration::from_secs(1);
    let _ = varde.update(Message::Tick(late));
    assert_eq!(
        reply.try_recv().unwrap(),
        ToolReply::error(waiting::NOT_DRAWN)
    );
    let doc = varde.screen.doc().unwrap();
    assert!(!doc.drawing_for_mcp() && !doc.mcp_waiting());
    assert_eq!(varde.mcp.log.back().unwrap().state, McpCallState::Failed);
    // Drawn after all, too late: dropped.
    request.answer_for_tests(None);
    drawn(&mut varde, tag, Some(picture(2, 2)));
    assert!(varde.screen.doc().unwrap().mcp.rendering().is_empty());
}
