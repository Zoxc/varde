//! `render`: a picture of the committed model, PNG, as MCP's image
//! content. Only the viewport has the GPU, so the picture is drawn by its
//! next frame ([`PreviewRequest`](varde_view::PreviewRequest), beside the save's thumbnail), and the
//! PNG encoded in the IO lane (`varde_io::Request::EncodePng`); the call
//! waits for both (`Wait::Render`, `Wait::Encoding`), within the bound
//! every call has.

use std::sync::Arc;

use glam::Vec3;
use serde_json::{Value, json};
use varde_document::BodyId;
use varde_mcp::tools::{ItemKind, NamedView, Render as Args, RenderFrom, ToolReply};
use varde_render::{Camera, PreviewShot, Projection, View};
use varde_view::Picked;

use crate::doc::Doc;
use crate::mcp::key;

/// The margin left around the bodies, in pixels: room for the outline's
/// edges, as a thumbnail has, at the picture's larger scale.
const MARGIN: u32 = 8;

/// What a `render` call draws: everything a [`PreviewRequest`] takes but
/// what takes its pixels, and the answer's data, the image to follow.
///
/// [`PreviewRequest`]: varde_view::PreviewRequest
pub(crate) struct Shot {
    pub(crate) mesh: Arc<varde_kernel::RenderMesh>,
    pub(crate) opacity: Arc<[f32]>,
    pub(crate) tints: Arc<[Option<varde_render::BodyTint>]>,
    pub(crate) shot: PreviewShot,
    pub(crate) selected_faces: Vec<u32>,
    pub(crate) highlights: Arc<varde_render::Highlights>,
    pub(crate) data: Value,
}

/// What `args` asks to draw of `doc`'s committed model, or the answer now
/// if it can't be: a body not named so or not in the model, an angle that
/// doesn't evaluate, a highlight key not found ([`key::found`]) or on a
/// body not drawn.
pub(crate) fn render(doc: &Doc, args: &Args) -> Result<Shot, ToolReply> {
    let model = super::committed(doc).map_err(ToolReply::error)?;
    let document = doc.editor.document();
    // The bodies drawn, in the order asked, or the model's.
    let bodies: Vec<BodyId> = if args.bodies.is_empty() {
        let mut bodies: Vec<BodyId> = Vec::new();
        for &body in model.parts {
            if !bodies.contains(&body) {
                bodies.push(body);
            }
        }
        bodies
    } else {
        let mut bodies = Vec::new();
        for name in &args.bodies {
            let shown = varde_mcp::shown(name);
            let body = (document.bodies().iter())
                .find(|body| body.name == *name)
                .ok_or_else(|| ToolReply::error(format!("no body is named \"{shown}\"")))?;
            if !model.parts.contains(&body.id) {
                return Err(ToolReply::error(format!(
                    "{shown} isn't in the model shown: it's hidden (set_visible shows it), \
                     not built, or a join merged it into another"
                )));
            }
            if !bodies.contains(&body.id) {
                bodies.push(body.id);
            }
        }
        bodies
    };
    let parts: Vec<usize> = (model.parts.iter().enumerate())
        .filter(|(_, body)| bodies.contains(body))
        .map(|(part, _)| part)
        .collect();
    if parts.is_empty() {
        return Err(ToolReply::error(
            "there's nothing to draw: no body is visible",
        ));
    }
    let camera = camera(doc, args)?;
    let side = args.size;
    let mut shot = varde_render::frame(
        model.mesh,
        &Default::default(),
        Some(&parts),
        &camera,
        [side, side],
        MARGIN,
    )
    .ok_or_else(|| ToolReply::error("there's nothing to draw: the bodies have no faces"))?;
    // Bounded by the arguments' check, from 0.1 to 10. A factor on the
    // framing: the view narrows, the camera stays outside the bodies.
    shot.zoom(args.zoom as f32);

    // The highlight, by the mesh's ids, as a selection is drawn.
    let mut picked = Vec::new();
    let mut index = None;
    for (at, item) in args.highlight.iter().enumerate() {
        let found = key::found(doc, &item.key, item.kind)
            .map_err(|why| ToolReply::error(format!("highlight {at}: {why}")))?;
        if !bodies.contains(&found.pick.body) {
            let name = document
                .body(found.pick.body)
                .map_or("a body", |body| body.name.as_str());
            return Err(ToolReply::error(format!(
                "highlight {at}: that {} is on {name}, which isn't drawn",
                item.kind.word()
            )));
        }
        match (item.kind, found.pick.target) {
            (ItemKind::Face, target @ Picked::Face(_))
            | (ItemKind::Edge, target @ Picked::Edge(_)) => picked.push(target),
            _ => {
                return Err(ToolReply::error(
                    "internal error: a key found something else",
                ));
            }
        }
        index = Some(found.index);
    }
    let highlight = index.map(|index| index.highlight(&[], &picked));
    let (selected_faces, highlights) = match highlight {
        Some(highlight) => (highlight.selected_faces, highlight.highlights),
        None => Default::default(),
    };

    let opacity = (model.parts.iter())
        .map(|&body| match bodies.contains(&body) {
            true => document.body(body).map_or(1.0, |body| body.opacity.alpha()),
            // Not drawn at all.
            false => 0.0,
        })
        .collect();
    let tints = (model.parts.iter())
        .map(|&body| (document.body(body)?.color).map(varde_view::body_tint))
        .collect();
    let names: Vec<&str> = (bodies.iter())
        .filter_map(|&body| document.body(body))
        .map(|body| body.name.as_str())
        .collect();
    let data = json!({
        "bodies": listed(&names),
        "body_count": names.len(),
        "projection": if args.perspective { "perspective" } else { "orthographic" },
        "size": shot.size,
        "highlighted": picked.len(),
        "model": super::model_state(doc, (model.generation, model.until)),
    });
    Ok(Shot {
        mesh: model.mesh.clone(),
        opacity,
        tints,
        shot,
        selected_faces,
        highlights,
        data,
    })
}

/// How many of the bodies drawn the answer names, and how much of each
/// name: the answer stays well within the reply's cap
/// ([`ToolReply::CAP`]), its image apart, however many bodies there are
/// and however long their names (up to 1 KiB each).
const LISTED: usize = 32;
const LISTED_BYTES: usize = 64;

/// `names`, the first [`LISTED`] of them, each cut to [`LISTED_BYTES`].
fn listed(names: &[&str]) -> Vec<String> {
    (names.iter().take(LISTED))
        .map(|name| varde_mcp::cut(name, LISTED_BYTES).to_owned())
        .collect()
}

/// Where `args` looks from, in its projection: a named view, the iso one
/// as Home looks, or yaw and pitch as typed angles in `doc`'s units and
/// parameters, the pitch from -90 to 90 degrees.
fn camera(doc: &Doc, args: &Args) -> Result<Camera, ToolReply> {
    let mut camera = Camera::default();
    match &args.from {
        RenderFrom::View(view) => {
            let side = match view {
                // Home's, the camera's own.
                NamedView::Iso => None,
                NamedView::Front => Some(View::Front),
                NamedView::Back => Some(View::Back),
                NamedView::Left => Some(View::Left),
                NamedView::Right => Some(View::Right),
                NamedView::Top => Some(View::Top),
                NamedView::Bottom => Some(View::Bottom),
            };
            if let Some(side) = side {
                camera.look_from(side);
            }
        }
        RenderFrom::Angles { yaw, pitch } => {
            let document = doc.editor.document();
            let angle = |text: &str, said: &str, max: f64| {
                let ask = varde_expr::Ask::angle(document.units(), max)
                    .with_params(document.params_resolved());
                varde_expr::Value::new(text.trim(), &ask)
                    .map(|value| value.value)
                    .map_err(|error| ToolReply::error(format!("from.{said}: {error}")))
            };
            // Any number of turns round; up or down at most.
            let yaw = angle(yaw, "yaw", 1e6)?;
            let pitch = angle(pitch, "pitch", std::f64::consts::FRAC_PI_2)?;
            let (yaw, pitch) = (yaw.rem_euclid(std::f64::consts::TAU) as f32, pitch as f32);
            let normal = Vec3::new(
                pitch.cos() * yaw.cos(),
                pitch.cos() * yaw.sin(),
                pitch.sin(),
            );
            // Up on screen as the camera has it at that yaw and pitch,
            // which says the way to turn straight down or up.
            let up = Vec3::new(
                -pitch.sin() * yaw.cos(),
                -pitch.sin() * yaw.sin(),
                pitch.cos(),
            );
            camera.face(normal, up);
        }
    }
    camera.set_projection(match args.perspective {
        true => Projection::Perspective,
        false => Projection::Orthographic,
    });
    Ok(camera)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bodies_named_stay_within_the_cap() {
        // As many bodies as a model may show, each named as long as a
        // name may be: listed whole, the answer was cut past the cap
        // and became an error, its picture with it.
        let long = "b".repeat(varde_document::MAX_NAME_LEN);
        let names = vec![long.as_str(); 256];
        let text = serde_json::to_string(&listed(&names)).unwrap();
        assert!(text.len() < ToolReply::CAP / 4, "{}", text.len());
        assert_eq!(listed(&["Body 1"]), ["Body 1"]);
    }
}
