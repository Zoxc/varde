//! `body_topology`: a body's faces, edges and corners from the committed
//! model, measured on its mesh.

use std::collections::HashMap;
use std::ops::Range;

use glam::DVec3;
use serde_json::{Value, json};
use varde_document::{BodyId, EdgeRef, FaceRef, FeatureId, Generation, MAX_COORD};
use varde_kernel_compat::RenderPart;
use varde_mcp::tools::{Direction, EdgeFilter, ToolReply, Topology};
use varde_regen::{Convexity, EdgeFacts, EdgeForm, Summary};

use super::{BUDGET, FAILURES_NOTES, ModelNotes, Numbers, model_state, paged_into};
use crate::doc::{Doc, Shown};
use crate::mcp::key::{Key, Ref};

/// What a listing says of its model: failures out of date as a
/// summary's, and what a rollback leaves out.
const TOPOLOGY_NOTES: ModelNotes = ModelNotes {
    rolled_back: Some(
        "The model is rolled back: the features from that point on aren't built, so their \
         faces and edges aren't listed.",
    ),
    ..FAILURES_NOTES
};

/// How far from the filter's direction a flat face's normal, or a
/// straight edge, may turn and still pass it: half a degree, its cosine.
/// Faces and edges a design means to be square to an axis are within
/// rounding of it; ones turned on purpose are turned by far more.
const FILTER_COS: f64 = 0.999_961_923_064_171;

/// `body_topology {body, faces?, edges?, offset?, limit?}`: the body's
/// faces, edges and corners in the committed model, three listings each
/// paged alone by `offset` and `limit` after its filter, with how many
/// there are and how many were left out.
///
/// Filters: `faces.normal` keeps the flat faces whose outward normal is
/// within [`FILTER_COS`] of the direction, either way along it for an
/// unsigned one (`"z"`). `edges.direction` keeps the straight edges
/// along it, either way whatever its sign: an edge runs whichever way
/// its faces have it, which means nothing to the client. `edges.convex`
/// keeps the convex edges (`true`) or the concave ones (`false`); smooth
/// ones, and those whose convexity wasn't found, pass neither.
///
/// Points, directions and bounding boxes are bare numbers in the
/// design's units; lengths and areas typed text. Areas, lengths,
/// bounding boxes and the keys' points come from the tessellation, so
/// they're approximate, and the answer says so; an edge's ends and form
/// and a corner's point are the solid's.
///
/// The edges' forms, ends and convexity come from regen apart
/// ([`varde_regen::Query::Topology`]), so this only checks the call
/// can be answered: what to wait on ([`Asked`]), or the answer now if
/// it can't. [`listed`] answers once the facts have come.
pub(crate) fn body_topology(doc: &Doc, args: &Topology) -> Result<Asked, ToolReply> {
    let found = locate(doc, args)?;
    Ok(Asked {
        body: found.body,
        generation: found.model.generation,
        until: found.model.until,
    })
}

/// What a `body_topology` call waits on: the edge facts of `body` in
/// the committed model of `generation` and `until`
/// ([`varde_regen::Query::Topology`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Asked {
    pub(crate) body: BodyId,
    pub(crate) generation: Generation,
    pub(crate) until: Option<FeatureId>,
}

/// The body `args` names, its place in the committed model, or why it
/// can't be listed, answered now.
struct Found<'a> {
    body: BodyId,
    name: &'a str,
    model: Shown<'a>,
    part: RenderPart,
}

fn locate<'a>(doc: &'a Doc, args: &Topology) -> Result<Found<'a>, ToolReply> {
    let body = super::super::names::body_reply(doc, &args.body)?;
    let Some(model) = doc.feed.committed() else {
        return Err(ToolReply::error(super::NOT_BUILT));
    };
    let Some(at) = model.parts.iter().position(|&id| id == body.id) else {
        return Err(ToolReply::error(if super::rebuilding(&model) {
            super::REBUILDING.to_owned()
        } else {
            format!(
                "\"{}\" isn't in the model: it's hidden, or wasn't built \
                 (design_summary says what failed)",
                body.name
            )
        }));
    };
    let Some(part) = model.mesh.parts().nth(at) else {
        return Err(ToolReply::error(super::REBUILDING));
    };
    Ok(Found {
        body: body.id,
        name: &body.name,
        model,
        part,
    })
}

/// The answer to `body_topology` `args`, which [`body_topology`] found
/// waits on `asked`, now that the edge facts it asked for have come:
/// `None` if the committed model has moved on from the one asked about
/// meanwhile, or the body isn't the one asked about.
pub(crate) fn listed(
    doc: &Doc,
    args: &Topology,
    asked: Asked,
    facts: Result<&[EdgeFacts], &str>,
) -> Option<ToolReply> {
    let found = match locate(doc, args) {
        Ok(found) => found,
        Err(reply) => return Some(reply),
    };
    let model = &found.model;
    if (found.body, model.generation, model.until) != (asked.body, asked.generation, asked.until) {
        return None;
    }
    let facts = match facts {
        Ok(facts) => facts,
        Err(why) => {
            return Some(ToolReply::error(format!(
                "the edges of \"{}\" couldn't be read: {why}",
                found.name
            )));
        }
    };
    let part = &found.part;
    if let Err(why) = paired(model, part.edges.clone(), facts) {
        return Some(ToolReply::error(format!(
            "internal error: the edges of \"{}\" didn't match the model ({why}); \
             ask again, and report it if it persists",
            found.name
        )));
    }
    let document = doc.editor.document();
    let units = document.units();
    let topology = Lister {
        doc,
        model,
        body: found.body,
        numbers: Numbers { units },
    };
    let faces = topology.faces(part.faces.clone());
    let face_keys: HashMap<usize, String> = (faces.iter())
        .map(|face| (face.id, face.key.clone()))
        .collect();
    let face_items: Vec<Value> = (faces.iter())
        .filter(|face| {
            (args.faces.and_then(|filter| filter.normal))
                .is_none_or(|normal| faces_along(face.summary, normal))
        })
        .map(|face| topology.face(face))
        .collect();
    // Facts pair with the part's first edges, checked above; the rest are
    // creases, inside a face, which aren't listed.
    let edge_items: Vec<Value> = (part.edges.clone().zip(facts))
        .filter_map(|(edge, fact)| {
            topology.edge(edge, fact, &face_keys, args.edges.unwrap_or_default())
        })
        .collect();
    let corner_items: Vec<Value> = (0..model.picking.corners().len())
        .filter_map(|corner| topology.corner(corner))
        .collect();
    let answer = json!({
        "body": found.name,
        "units": units.symbol(),
        "model": model_state(doc, (model.generation, model.until), &TOPOLOGY_NOTES),
        "note": "Areas, lengths, bounding boxes and the points keys carry are measured on the \
                 tessellated model, so they're approximate; edges' forms and ends and corners' \
                 points are exact. Points and boxes are in the design's units. Each listing is \
                 paged alone by offset and limit after its filter.",
    });
    let lists = vec![
        ("faces", face_items),
        ("edges", edge_items),
        ("corners", corner_items),
    ];
    let answer = paged_into(&answer, lists, args.page, BUDGET);
    Some(ToolReply::data(answer))
}

/// Whether `facts` pair with the edges `edges` of `model`'s mesh: the
/// facts at `i` with edge `edges.start + i`, by its two faces' keys, and
/// every edge past them a crease (no faces either side to pair).
fn paired(model: &Shown, edges: Range<usize>, facts: &[EdgeFacts]) -> Result<(), String> {
    if facts.len() > edges.len() {
        return Err(format!(
            "{} edges' facts for {} edges",
            facts.len(),
            edges.len()
        ));
    }
    for (at, edge) in edges.enumerate() {
        let keys =
            (u32::try_from(edge).ok()).and_then(|edge| model.picking.edge_keys(model.mesh, edge));
        match (facts.get(at), keys) {
            (Some(fact), Some(keys)) if fact.faces == keys => {}
            (None, None) => {}
            _ => return Err(format!("edge {at} differs")),
        }
    }
    Ok(())
}

/// Whether a face of `summary` passes the `normal` filter: see
/// [`body_topology`].
fn faces_along(summary: Summary, normal: Direction) -> bool {
    let Summary::Plane { n, .. } = summary else {
        return false;
    };
    let dot = DVec3::from(n).dot(DVec3::from(normal.vector));
    let dot = if normal.signed { dot } else { dot.abs() };
    dot >= FILTER_COS
}

/// A face of the body, measured on the mesh.
struct MeshFace {
    /// Its id in the mesh.
    id: usize,
    key: String,
    summary: Summary,
    /// The sum of its triangles' areas, in mm².
    area: f64,
    bounds: [DVec3; 2],
}

/// What `body_topology` lists from.
struct Lister<'a> {
    doc: &'a Doc,
    model: &'a Shown<'a>,
    body: BodyId,
    numbers: Numbers,
}

impl Lister<'_> {
    fn key(&self, item: Ref) -> String {
        Key::new(self.doc.id, self.model.generation, item).encode()
    }

    /// The faces `ids` of the mesh, measured, with their keys: those the
    /// picking tables name.
    fn faces(&self, ids: Range<usize>) -> Vec<MeshFace> {
        let mesh = &**self.model.mesh;
        let picked = self.model.picking.faces();
        let position = |index: u32| {
            (mesh.positions().get(usize::try_from(index).ok()?))
                .map(|p| DVec3::from(p.map(f64::from)))
        };
        ids.filter_map(|id| {
            let pick = picked.get(id)?;
            let indices = mesh.indices().get(mesh.face_indices(id)?)?;
            let mut area = 0.0;
            let mut largest = (-1.0, DVec3::ZERO);
            let mut bounds = [DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)];
            for triangle in indices.as_chunks::<3>().0 {
                let [a, b, c] = [0, 1, 2].map(|k| position(triangle[k]));
                let (Some(a), Some(b), Some(c)) = (a, b, c) else {
                    continue;
                };
                // Positions are within `RenderMesh::MAX_POSITION`, so
                // this is finite.
                let twice = (b - a).cross(c - a).length();
                area += twice / 2.0;
                if twice > largest.0 {
                    largest = (twice, (a + b + c) / 3.0);
                }
                for p in [a, b, c] {
                    bounds = [bounds[0].min(p), bounds[1].max(p)];
                }
            }
            if largest.0 < 0.0 {
                return None;
            }
            let near = clamped(largest.1);
            let key = self.key(Ref::Face(FaceRef {
                body: self.body,
                key: pick.key,
                near,
            }));
            Some(MeshFace {
                id,
                key,
                summary: pick.summary,
                area,
                bounds,
            })
        })
        .collect()
    }

    fn face(&self, face: &MeshFace) -> Value {
        let mut item = json!({ "key": face.key });
        let (form, facts) = self.summary(face.summary);
        item["form"] = json!(form);
        if let (Value::Object(item), Value::Object(facts)) = (&mut item, facts) {
            item.extend(facts);
        }
        item["area"] = json!(self.numbers.area(face.area));
        item["bounds"] = json!({
            "min": self.numbers.point(face.bounds[0].to_array()),
            "max": self.numbers.point(face.bounds[1].to_array()),
        });
        item
    }

    /// A face's form in words and its facts.
    fn summary(&self, summary: Summary) -> (&'static str, Value) {
        match summary {
            Summary::Plane { n, .. } => ("plane", json!({ "normal": direction(n) })),
            Summary::Cylinder {
                point,
                axis,
                radius,
            } => (
                "cylinder",
                json!({
                    "axis": { "point": self.numbers.point(point), "direction": direction(axis) },
                    "radius": self.numbers.length(radius),
                }),
            ),
            Summary::Cone { apex, axis, .. } => (
                "cone",
                json!({ "apex": self.numbers.point(apex), "axis": direction(axis) }),
            ),
            Summary::Sphere { centre, radius } => (
                "sphere",
                json!({ "centre": self.numbers.point(centre), "radius": self.numbers.length(radius) }),
            ),
            Summary::Torus {
                centre,
                axis,
                major,
                minor,
            } => (
                "torus",
                json!({
                    "centre": self.numbers.point(centre),
                    "axis": direction(axis),
                    "major_radius": self.numbers.length(major),
                    "minor_radius": self.numbers.length(minor),
                }),
            ),
            Summary::ConicCylinder { along } => {
                ("conic cylinder", json!({ "along": direction(along) }))
            }
            Summary::Revolved { origin, axis } => (
                "revolved",
                json!({ "axis": { "point": self.numbers.point(origin), "direction": direction(axis) } }),
            ),
            Summary::Other => ("other", json!({})),
        }
    }

    /// Edge `edge` of the mesh, of `pick`, its facts, if it passes
    /// `filter`.
    fn edge(
        &self,
        edge: usize,
        pick: &EdgeFacts,
        face_keys: &HashMap<usize, String>,
        filter: EdgeFilter,
    ) -> Option<Value> {
        let mesh = &**self.model.mesh;
        let faces = pick.faces;
        if let Some(convex) = filter.convex {
            let wanted = if convex {
                Convexity::Convex
            } else {
                Convexity::Concave
            };
            if pick.convexity != Some(wanted) {
                return None;
            }
        }
        if let Some(along) = filter.direction {
            let EdgeForm::Line { from, to } = pick.form else {
                return None;
            };
            let run = (DVec3::from(to) - DVec3::from(from)).normalize_or_zero();
            if run.dot(DVec3::from(along.vector)).abs() < FILTER_COS {
                return None;
            }
        }
        let points: Vec<DVec3> = (mesh.polyline(edge)?.iter())
            .filter_map(|&v| mesh.positions().get(usize::try_from(v).ok()?))
            .map(|p| DVec3::from(p.map(f64::from)))
            .collect();
        let mut length = 0.0;
        let mut longest = (-1.0, DVec3::ZERO);
        for pair in points.windows(2) {
            let step = pair[0].distance(pair[1]);
            length += step;
            if step > longest.0 {
                longest = (step, pair[0].midpoint(pair[1]));
            }
        }
        if longest.0 < 0.0 {
            return None;
        }
        let key = self.key(Ref::Edge(EdgeRef {
            body: self.body,
            faces,
            near: clamped(longest.1),
        }));
        let [a, b] = *mesh.edge_faces().get(edge)?;
        let mut item = json!({ "key": key });
        match pick.form {
            EdgeForm::Line { from, to } => {
                item["form"] = json!("line");
                item["direction"] = direction(
                    (DVec3::from(to) - DVec3::from(from))
                        .normalize_or_zero()
                        .to_array(),
                );
            }
            EdgeForm::Circle {
                centre,
                axis,
                radius,
            } => {
                item["form"] = json!("circle");
                item["centre"] = self.numbers.point(centre);
                item["axis"] = direction(axis);
                item["radius"] = json!(self.numbers.length(radius));
            }
            EdgeForm::Ellipse {
                centre,
                axis,
                major,
                minor,
            } => {
                item["form"] = json!("ellipse");
                item["centre"] = self.numbers.point(centre);
                item["axis"] = direction(axis);
                item["major_radius"] = json!(self.numbers.length(major));
                item["minor_radius"] = json!(self.numbers.length(minor));
            }
            EdgeForm::Other => item["form"] = json!("other"),
        }
        match pick.ends {
            Some([from, to]) => {
                item["ends"] = json!([self.numbers.point(from), self.numbers.point(to)])
            }
            None => item["closed"] = json!(true),
        }
        item["length"] = json!(self.numbers.length(length));
        item["faces"] = json!([key_of(face_keys, a), key_of(face_keys, b)]);
        match pick.convexity {
            Some(Convexity::Convex) => item["convex"] = json!(true),
            Some(Convexity::Concave) => item["convex"] = json!(false),
            Some(Convexity::Smooth) => item["smooth"] = json!(true),
            None => {}
        }
        Some(item)
    }

    /// Corner `corner` of the model, if it's of the body.
    fn corner(&self, corner: usize) -> Option<Value> {
        let picking = &**self.model.picking;
        let pick = picking.corners().get(corner)?;
        if picking.face_body(self.model.mesh, pick.faces[0]) != Some(self.body) {
            return None;
        }
        let faces = picking.corner_keys(u32::try_from(corner).ok()?)?;
        let key = self.key(Ref::Corner {
            body: self.body,
            faces,
            point: pick.point,
        });
        Some(json!({ "key": key, "point": self.numbers.point(pick.point) }))
    }
}

/// The key `face_keys` gives face `face` of the mesh.
fn key_of(face_keys: &HashMap<usize, String>, face: u32) -> Option<&String> {
    usize::try_from(face)
        .ok()
        .and_then(|face| face_keys.get(&face))
}

/// A unit direction, to six places.
fn direction(v: [f64; 3]) -> Value {
    json!(v.map(|x| {
        varde_expr::format_number(x, None)
            .parse::<f64>()
            .unwrap_or(x)
    }))
}

/// `p` within the bounds a reference's point keeps to.
fn clamped(p: DVec3) -> DVec3 {
    let bound = f64::from(MAX_COORD);
    p.clamp(DVec3::splat(-bound), DVec3::splat(bound))
}
