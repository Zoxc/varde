//! `feature`: a feature's settings in its panel's shape, which is its
//! creating tool's: the same argument names and value forms, so a client
//! can read a feature, change a field and send it back with
//! `set_feature`. Values are the text the panel's fields show (its
//! editing session's, `TypedText::of`), choices
//! the tool's words, sketches and bodies by name, regions by index in
//! their sketch's, faces, edges and corners by key of the committed
//! model. A reference that can't be named so (a body or sketch gone, a
//! region its sketch no longer has, a face the committed model doesn't
//! have: rolled past, failed or moved since) is said in its place, as
//! `{"unresolved": why}`, rather than left out. A sketch is summed up by
//! its counts.
//!
//! Each list in the settings (at the top or in an object there: a split's
//! `tool.profiles`, a sweep's `path.edges`) is paged alone by the call's
//! `offset` and `limit`, as `design_summary`'s listings are, and a page
//! is shortened past that (the longest first) until the answer fits
//! [`super::BUDGET`], names of a kilobyte among it. A list given whole
//! stays a plain array, so a small feature reads and round-trips as its
//! tool takes it; one that isn't becomes a page (`items`, `total`,
//! `left_out`), with a `note` naming it, which `set_feature` refuses in
//! place of the list (`set::page_sent`).

use serde_json::{Map, Value, json};
use varde_document::{
    AlignRefs, AxisLine, AxisRef, BodyId, ChamferSize, Copies, DirRef, Document, EdgeRef, FaceRef,
    Feature, FeatureId, FeatureKind, Operation, OriginPlane, PathPart, PatternKind, Plane,
    PlaneRef, PointRef, RegionRef, ScaleFactor, Section, SplitTool,
};
use varde_mcp::tools::{ItemKind, Page, ToolReply, World, WorldAxis, WorldPlane};
use varde_view::MotionField;

use super::failure;
use crate::doc::{CombineSession, Doc, ExtrudeSession, MotionSession, RevolveSession, sketch_of};
use crate::mcp::key::{self, Key, Ref};

/// `feature {name, offset?, limit?}`: see the module's docs.
pub(crate) fn feature(doc: &Doc, name: &str, page: Page) -> ToolReply {
    let feature = match super::named(doc, name) {
        Ok(feature) => feature,
        Err(reply) => return reply,
    };
    let settings = Shape::new(doc).settings(feature);
    let mut answer = json!({
        "name": feature.name,
        "kind": feature.kind.noun(),
        "id": feature.id.get(),
        "visible": feature.visible,
        "settings": settings,
    });
    if let Some(tool) = tool_of(&feature.kind) {
        answer["tool"] = json!(tool);
    }
    let failures = doc.feed.committed().map(|model| model.failures);
    if let Some(failure) = failures.and_then(|failures| failure(failures, feature)) {
        answer["failure"] = json!(failure.message);
    }
    ToolReply::data(paged(answer, page))
}

/// Where a list sits in a feature's `settings`: a key, and a key in the
/// object there.
type At = (String, Option<String>);

/// `answer` with each list in its `settings` paged by `page` and
/// shortened to fit ([`super::pages`]), a list given whole left a plain
/// array: see the module's docs.
fn paged(answer: Value, page: Page) -> Value {
    let Some(settings) = answer["settings"].as_object() else {
        return answer;
    };
    let mut at: Vec<At> = Vec::new();
    let mut lists: Vec<Vec<Value>> = Vec::new();
    for (key, value) in settings {
        match value {
            Value::Array(items) => {
                at.push((key.clone(), None));
                lists.push(items.clone());
            }
            Value::Object(inner) => {
                for (sub, value) in inner {
                    if let Value::Array(items) = value {
                        at.push((key.clone(), Some(sub.clone())));
                        lists.push(items.clone());
                    }
                }
            }
            _ => {}
        }
    }
    if lists.is_empty() {
        return answer;
    }
    super::pages(&lists, page, super::BUDGET, |pages| {
        let mut said = answer.clone();
        let mut partial = Vec::new();
        for ((key, sub), mut value) in at.iter().zip(pages) {
            if page.offset == 0 && value["left_out"] == 0 {
                value = value["items"].take();
            } else {
                partial.push(match sub {
                    Some(sub) => format!("{key}.{sub}"),
                    None => key.clone(),
                });
            }
            match sub {
                Some(sub) => said["settings"][key][sub] = value,
                None => said["settings"][key] = value,
            }
        }
        if !partial.is_empty() {
            said["note"] = json!(format!(
                "{} {} only part of the list, a page (items, total, left_out); ask with \
                 another offset for the rest. A page isn't the list: don't send it to \
                 set_feature in its place (it's refused); leave the field out to keep the \
                 list, or send the whole list it's to be.",
                partial.join(", "),
                if partial.len() == 1 { "is" } else { "are" },
            ));
        }
        said
    })
}

/// The tool that makes a feature of `kind`, whose arguments are its
/// shape: none for a sketch.
pub(crate) fn tool_of(kind: &FeatureKind) -> Option<&'static str> {
    Some(match kind {
        FeatureKind::Sketch { .. } => return None,
        FeatureKind::Extrude(_) => "extrude",
        FeatureKind::Revolve(_) => "revolve",
        FeatureKind::Combine(_) => "combine",
        FeatureKind::Move(_) => "move_body",
        FeatureKind::Mirror(_) => "mirror",
        FeatureKind::Pattern(_) => "pattern",
        FeatureKind::Align(_) => "align",
        FeatureKind::Scale(_) => "scale",
        FeatureKind::Split(_) => "split",
        FeatureKind::Chamfer(_) => "chamfer",
        FeatureKind::Shell(_) => "shell",
        FeatureKind::Fillet(_) => "fillet",
        FeatureKind::OffsetFace(_) => "offset_face",
        FeatureKind::FaceDraft(_) => "draft",
        FeatureKind::Sweep(_) => "sweep",
        FeatureKind::Loft(_) => "loft",
    })
}

/// The word an origin plane is named by.
fn plane_word(plane: OriginPlane) -> &'static str {
    match plane {
        OriginPlane::XY => WorldPlane::Xy,
        OriginPlane::XZ => WorldPlane::Xz,
        OriginPlane::YZ => WorldPlane::Yz,
    }
    .word()
}

/// The word a world axis is named by.
fn axis_word(axis: varde_document::Axis3) -> &'static str {
    match axis {
        varde_document::Axis3::X => WorldAxis::X,
        varde_document::Axis3::Y => WorldAxis::Y,
        varde_document::Axis3::Z => WorldAxis::Z,
    }
    .word()
}

/// The word an extrude's, revolve's, sweep's or loft's operation is.
fn operation_word(operation: &Operation) -> &'static str {
    match operation {
        Operation::NewBody(_) => "new_body",
        Operation::Join(_) => "join",
        Operation::Cut(_) => "cut",
        Operation::Intersect(_) => "intersect",
    }
}

/// What's said in place of a reference that can't be named.
fn unresolved(why: impl Into<String>) -> Value {
    json!({ "unresolved": why.into() })
}

/// Shapes features of `doc`'s design.
pub(crate) struct Shape<'a> {
    doc: &'a Doc,
    /// The design the features shaped are of: `doc`'s as it is, or one
    /// it was ([`Shape::at`]).
    document: &'a Document,
}

impl<'a> Shape<'a> {
    pub(crate) fn new(doc: &'a Doc) -> Self {
        Self::at(doc, doc.editor.document())
    }

    /// Shapes features of `document`, a state `doc`'s design was in:
    /// names, regions and typed text as of it, keys of the committed
    /// model as for any feature.
    pub(crate) fn at(doc: &'a Doc, document: &'a Document) -> Self {
        Self { doc, document }
    }

    /// A sketch's `plane`: an origin plane's word, or its face by key.
    pub(crate) fn sketch_plane(&self, plane: &Plane) -> Value {
        match plane {
            Plane::Origin(plane) => json!(plane_word(*plane)),
            Plane::Face(face) => self.face(face),
        }
    }

    /// `feature`'s settings as `feature` gives them: [`Shape::of`], a
    /// sketch summed up by its plane and counts.
    pub(crate) fn settings(&self, feature: &Feature) -> Value {
        match &feature.kind {
            FeatureKind::Sketch { plane, sketch, .. } => json!({
                "plane": self.sketch_plane(plane),
                "points": sketch.points.len(),
                "curves": sketch.curves.len(),
                "constraints": sketch.constraints.len(),
                "dimensions": sketch.dimensions.len(),
                "links": sketch.links.len(),
            }),
            _ => Value::Object(self.of(feature).unwrap_or_default()),
        }
    }

    /// `feature`'s settings as its tool takes them: none for a sketch.
    pub(crate) fn of(&self, feature: &Feature) -> Option<Map<String, Value>> {
        let document = self.document;
        let id = feature.id;
        let mut map = Map::new();
        let mut put = |key: &str, value: Value| {
            map.insert(key.to_owned(), value);
        };
        match &feature.kind {
            FeatureKind::Sketch { .. } => return None,
            FeatureKind::Extrude(extrude) => {
                let session = ExtrudeSession::editing_for_client(document, id, extrude);
                put("sketch", self.feature_name(extrude.sketch));
                put("profiles", self.regions(extrude.sketch, &extrude.regions));
                let (word, distances) = match &extrude.extent {
                    varde_document::Extent::OneSide(_) => ("one_side", 1),
                    varde_document::Extent::Symmetric(_) => ("symmetric", 1),
                    varde_document::Extent::TwoSides(..) => ("two_sides", 2),
                    varde_document::Extent::ThroughAll => ("through_all", 0),
                };
                put("extent", json!(word));
                if distances >= 1 {
                    put("distance", json!(session.sides.fields[0].text));
                }
                if distances >= 2 {
                    put("second_distance", json!(session.sides.fields[1].text));
                }
                put("flip", json!(extrude.flip));
                put("taper", json!(session.taper.text));
                self.operation(&mut put, &extrude.operation);
            }
            FeatureKind::Revolve(revolve) => {
                let session = RevolveSession::editing(document, id, revolve);
                put("sketch", self.feature_name(revolve.sketch));
                put("profiles", self.regions(revolve.sketch, &revolve.regions));
                put(
                    "axis",
                    match &revolve.axis {
                        AxisLine::SketchX => json!("x"),
                        AxisLine::SketchY => json!("y"),
                        AxisLine::Curve(line) => json!({ "line": line.get() }),
                        AxisLine::Edge(edge) => self.edge(edge),
                    },
                );
                let (word, angles) = match &revolve.extent {
                    varde_document::Turn::Full => ("full", 0),
                    varde_document::Turn::OneSide(_) => ("one_side", 1),
                    varde_document::Turn::Symmetric(_) => ("symmetric", 1),
                    varde_document::Turn::TwoSides(..) => ("two_sides", 2),
                };
                put("extent", json!(word));
                if angles >= 1 {
                    put("angle", json!(session.sides.fields[0].text));
                }
                if angles >= 2 {
                    put("second_angle", json!(session.sides.fields[1].text));
                }
                put("flip", json!(revolve.flip));
                self.operation(&mut put, &revolve.operation);
            }
            FeatureKind::Combine(combine) => {
                let session = CombineSession::editing(id, combine);
                put("target", self.body(combine.target));
                put("tools", self.bodies(&combine.tools));
                let word = match session.op {
                    varde_document::BodyOp::Union => "union",
                    varde_document::BodyOp::Subtract => "subtract",
                    varde_document::BodyOp::Intersect => "intersect",
                };
                put("operation", json!(word));
                put("keep_tools", json!(session.keep_tools));
            }
            kind => {
                let shape = self.doc.pattern_shapes.get(&id);
                let session = MotionSession::editing_for_client(document, id, shape)?;
                let text = |field: MotionField| json!(session.field(field).text);
                match kind {
                    FeatureKind::Move(moved) => {
                        put("bodies", self.bodies(&moved.bodies));
                        for (key, axis) in [("x", 0), ("y", 1), ("z", 2)] {
                            put(
                                key,
                                text(MotionField::Offset(varde_document::Axis3::ALL[axis])),
                            );
                        }
                        if let Some((axis, _)) = &moved.turn {
                            put("angle", text(MotionField::Angle));
                            put("axis", self.axis(axis));
                        }
                    }
                    FeatureKind::Mirror(mirror) => {
                        put("bodies", self.bodies(&mirror.bodies));
                        put("plane", self.plane(&mirror.plane));
                        put("keep_original", json!(mirror.keep_original));
                    }
                    FeatureKind::Pattern(pattern) => {
                        let linear = matches!(pattern.kind, PatternKind::Linear { .. });
                        put("kind", json!(if linear { "linear" } else { "circular" }));
                        put("bodies", self.bodies(&pattern.bodies));
                        put("axis", self.axis(pattern.kind.axis()));
                        put("count", text(MotionField::Count));
                        let mode = match session.mode {
                            varde_view::PatternMode::Spacing => "spacing",
                            varde_view::PatternMode::Total => "total",
                            varde_view::PatternMode::Full => "full",
                        };
                        put("mode", json!(mode));
                        if session.mode != varde_view::PatternMode::Full {
                            put("spread", text(MotionField::Spread));
                        }
                        if linear {
                            put("flip", json!(session.flip));
                        }
                        put("join", json!(pattern.copies == Copies::Joined));
                    }
                    FeatureKind::Align(align) => {
                        put("body", self.body(align.body));
                        put("from", self.align_side(&align.from));
                        put("to", self.align_side(&align.to));
                        put("flip", json!(align.flip));
                        put("distance", text(MotionField::Distance));
                        put("angle", text(MotionField::Angle));
                    }
                    FeatureKind::Scale(scale) => {
                        put("bodies", self.bodies(&scale.bodies));
                        put("point", self.point(&scale.about));
                        match &scale.factor {
                            ScaleFactor::Uniform(_) => {
                                put("mode", json!("uniform"));
                                put("factor", text(MotionField::Factor));
                            }
                            ScaleFactor::PerAxis(_) => {
                                put("mode", json!("per_axis"));
                                for (key, axis) in [("x", 0), ("y", 1), ("z", 2)] {
                                    let axis = varde_document::Axis3::ALL[axis];
                                    put(key, text(MotionField::AxisFactor(axis)));
                                }
                            }
                            ScaleFactor::EdgeLength {
                                edge, axis_only, ..
                            } => {
                                put("mode", json!("edge_length"));
                                put("edge", self.edge_key(edge));
                                put("length", text(MotionField::Length));
                                put("axis_only", json!(axis_only));
                            }
                        }
                    }
                    FeatureKind::Split(split) => {
                        put("body", self.body(split.body));
                        let tool = match &split.tool {
                            SplitTool::Plane(plane) => self.plane(plane),
                            SplitTool::Face(face) => self.face(face),
                            SplitTool::Body(body) => json!({ "body": self.body(*body) }),
                            SplitTool::Regions { sketch, regions } => json!({
                                "sketch": self.feature_name(*sketch),
                                "profiles": self.regions(*sketch, regions),
                            }),
                            SplitTool::Chain { sketch, curves } => json!({
                                "sketch": self.feature_name(*sketch),
                                "curves": curves.iter().map(|id| id.get()).collect::<Vec<_>>(),
                            }),
                        };
                        put("tool", tool);
                        let side = |side: varde_document::Side| match side {
                            varde_document::Side::Front => "front",
                            varde_document::Side::Back => "back",
                        };
                        let keep = match split.keep {
                            varde_document::Keep::Both => "both",
                            varde_document::Keep::Front => "front",
                            varde_document::Keep::Back => "back",
                        };
                        if split.keep == varde_document::Keep::Both {
                            put("original", json!(side(split.original)));
                        }
                        put("keep", json!(keep));
                    }
                    FeatureKind::Shell(shell) => {
                        // A shell's body is its faces' when it has any.
                        if shell.open.is_empty() {
                            put("body", self.body(shell.body));
                        }
                        put("faces", self.face_keys(&shell.open));
                        put("thickness", text(MotionField::Thickness));
                        let direction = if shell.outward { "outward" } else { "inward" };
                        put("direction", json!(direction));
                    }
                    FeatureKind::OffsetFace(offset) => {
                        put("faces", self.face_keys(&offset.faces));
                        put("distance", text(MotionField::Distance));
                        put("inward", json!(offset.inward));
                        put("tangent", json!(offset.tangent));
                    }
                    FeatureKind::FaceDraft(draft) => {
                        put("faces", self.face_keys(&draft.faces));
                        put("neutral", self.plane(&draft.neutral));
                        put("angle", text(MotionField::Angle));
                        put("flip", json!(draft.flip));
                        put("tangent", json!(draft.tangent));
                    }
                    FeatureKind::Chamfer(chamfer) => {
                        put("edges", self.edge_keys(&chamfer.edges));
                        put("faces", self.face_keys(&chamfer.faces));
                        let distance = text(MotionField::ChamferDistance);
                        match &chamfer.distances {
                            ChamferSize::Equal(_) => {
                                put("type", json!("equal"));
                                put("distance", distance);
                            }
                            ChamferSize::Two(..) => {
                                put("type", json!("two_distances"));
                                put("distance", distance);
                                put("second_distance", text(MotionField::ChamferSecond));
                                put("flip", json!(chamfer.flip));
                            }
                            ChamferSize::Angle(..) => {
                                put("type", json!("distance_angle"));
                                put("distance", distance);
                                put("angle", text(MotionField::ChamferAngle));
                                put("flip", json!(chamfer.flip));
                            }
                        }
                        // The panel's tick, on as it opens: the edges are stored each
                        // of its own.
                        put("tangent", json!(session.blend.chains));
                    }
                    FeatureKind::Fillet(fillet) => {
                        put("edges", self.edge_keys(&fillet.edges));
                        put("faces", self.face_keys(&fillet.faces));
                        put("radius", text(MotionField::Radius));
                        put("tangent", json!(session.blend.chains));
                    }
                    FeatureKind::Sweep(sweep) => {
                        put("sketch", self.feature_name(sweep.sketch));
                        put("profiles", self.regions(sweep.sketch, &sweep.regions));
                        match &sweep.path {
                            varde_document::PathRef::Chain(parts) => {
                                let mut curves = Vec::new();
                                let mut edges = Vec::new();
                                let mut tangent = None;
                                for part in parts {
                                    match part {
                                        PathPart::Curves(chain) => curves.push(json!({
                                            "sketch": self.feature_name(chain.sketch),
                                            "curve": chain.curves.first().map(|id| id.get()),
                                        })),
                                        PathPart::Edges {
                                            edges: some,
                                            tangent: tick,
                                        } => {
                                            edges.extend(some.iter().map(|e| self.edge_key(e)));
                                            tangent.get_or_insert(*tick);
                                        }
                                    }
                                }
                                put(
                                    "path",
                                    json!({
                                        "curves": curves,
                                        "edges": edges,
                                        "tangent": tangent.unwrap_or(true),
                                    }),
                                );
                                let keep = sweep.orientation == varde_document::Orientation::Keep;
                                put("keep_orientation", json!(keep));
                                put("twist", text(MotionField::Twist));
                            }
                            varde_document::PathRef::Helix(helix) => put(
                                "helix",
                                json!({
                                    "axis": self.axis(&helix.axis),
                                    "pitch": text(MotionField::Pitch),
                                    "turns": text(MotionField::Turns),
                                    "left_handed": helix.left_handed,
                                    "flip": helix.flip,
                                }),
                            ),
                        }
                        self.operation(&mut put, &sweep.operation);
                    }
                    FeatureKind::Loft(loft) => {
                        let sections: Vec<Value> = (loft.sections.iter())
                            .map(|section| match section {
                                Section::Region {
                                    sketch,
                                    region,
                                    start,
                                } => {
                                    let regions =
                                        self.regions(*sketch, std::slice::from_ref(region));
                                    let mut said = json!({
                                        "sketch": self.feature_name(*sketch),
                                        "region": regions.get(0).cloned().unwrap_or(Value::Null),
                                    });
                                    if let Some(start) = start {
                                        said["start"] = json!(start.get());
                                    }
                                    said
                                }
                                Section::Point { sketch, point } => json!({
                                    "sketch": self.feature_name(*sketch),
                                    "point": point.get(),
                                }),
                            })
                            .collect();
                        put("sections", json!(sections));
                        let mode = match loft.mode {
                            varde_document::LoftMode::Smooth => "smooth",
                            varde_document::LoftMode::Ruled => "ruled",
                        };
                        put("mode", json!(mode));
                        put("closed", json!(loft.closed));
                        let rails: Vec<Value> = (loft.rails.iter())
                            .map(|chain| {
                                json!({
                                    "sketch": self.feature_name(chain.sketch),
                                    "curve": chain.curves.first().map(|id| id.get()),
                                })
                            })
                            .collect();
                        put("rails", json!(rails));
                        self.operation(&mut put, &loft.operation);
                    }
                    _ => return None,
                }
            }
        }
        Some(map)
    }

    /// An operation and, for a join, cut or intersect, the bodies it
    /// leaves out.
    fn operation(&self, put: &mut impl FnMut(&str, Value), operation: &Operation) {
        put("operation", json!(operation_word(operation)));
        if !matches!(operation, Operation::NewBody(_)) {
            put("exclude", self.bodies(operation.excluded()));
        }
    }

    /// The feature `id`'s name.
    fn feature_name(&self, id: FeatureId) -> Value {
        match self.document.feature(id) {
            Some(feature) => json!(feature.name),
            None => unresolved("a sketch the design no longer has"),
        }
    }

    /// The body `id`'s name.
    fn body(&self, id: BodyId) -> Value {
        match self.document.body(id) {
            Some(body) => json!(body.name),
            None => unresolved("a body the design no longer has"),
        }
    }

    fn bodies(&self, ids: &[BodyId]) -> Value {
        Value::Array(ids.iter().map(|&id| self.body(id)).collect())
    }

    /// `regions` of `sketch`, by their indices in its regions as the
    /// tools take them.
    fn regions(&self, sketch: FeatureId, regions: &[RegionRef]) -> Value {
        let Some(drawn) = sketch_of(self.document, sketch) else {
            let gone = unresolved("a region of a sketch the design no longer has");
            return Value::Array(vec![gone; regions.len()]);
        };
        let resolved = match drawn.profiles() {
            Ok(profiles) => profiles.resolve(regions),
            Err(_) => vec![None; regions.len()],
        };
        let found = resolved.into_iter().map(|index| match index {
            Some(index) => json!(index),
            None => unresolved("a region its sketch no longer has"),
        });
        Value::Array(found.collect())
    }

    /// The key of `item`, a `kind`, in the committed model, or why it
    /// has none. A body merged into another since is looked for as the
    /// one holding it, as the model has it.
    fn key(&self, item: Ref, kind: ItemKind) -> Result<String, String> {
        let doc = self.doc;
        let model = super::committed(doc)?;
        let generation = model.generation;
        let text = |item: Ref| Key::new(doc.id, generation, item).encode();
        let first = text(item);
        let why = match key::found(doc, &first, kind) {
            Ok(_) => return Ok(first),
            Err(why) => why,
        };
        let merged = crate::mcp::make::committed_merges(doc, None)?;
        let held = |body: BodyId| merged.holder(body).filter(|&holder| holder != body);
        let moved = match item {
            Ref::Face(face) => held(face.body).map(|body| Ref::Face(FaceRef { body, ..face })),
            Ref::Edge(edge) => held(edge.body).map(|body| Ref::Edge(EdgeRef { body, ..edge })),
            Ref::Corner { body, faces, point } => {
                held(body).map(|body| Ref::Corner { body, faces, point })
            }
        };
        if let Some(moved) = moved {
            let second = text(moved);
            if key::found(doc, &second, kind).is_ok() {
                return Ok(second);
            }
        }
        Err(format!(
            "not in the committed model (rolled past, failed or changed since): {why}"
        ))
    }

    /// A face's key alone, as a face tool lists them.
    fn face_key(&self, face: &FaceRef) -> Value {
        match self.key(Ref::Face(*face), ItemKind::Face) {
            Ok(key) => json!(key),
            Err(why) => unresolved(why),
        }
    }

    fn face_keys(&self, faces: &[FaceRef]) -> Value {
        Value::Array(faces.iter().map(|face| self.face_key(face)).collect())
    }

    /// An edge's key alone.
    fn edge_key(&self, edge: &EdgeRef) -> Value {
        match self.key(Ref::Edge(*edge), ItemKind::Edge) {
            Ok(key) => json!(key),
            Err(why) => unresolved(why),
        }
    }

    fn edge_keys(&self, edges: &[EdgeRef]) -> Value {
        Value::Array(edges.iter().map(|edge| self.edge_key(edge)).collect())
    }

    /// A face as a reference: `{"face": key}`.
    pub(crate) fn face(&self, face: &FaceRef) -> Value {
        match self.key(Ref::Face(*face), ItemKind::Face) {
            Ok(key) => json!({ "face": key }),
            Err(why) => unresolved(why),
        }
    }

    /// An edge as a reference: `{"edge": key}`.
    fn edge(&self, edge: &EdgeRef) -> Value {
        match self.key(Ref::Edge(*edge), ItemKind::Edge) {
            Ok(key) => json!({ "edge": key }),
            Err(why) => unresolved(why),
        }
    }

    fn axis(&self, axis: &AxisRef) -> Value {
        match axis {
            AxisRef::Origin(axis) => json!(axis_word(*axis)),
            AxisRef::Edge(edge) => self.edge(edge),
            AxisRef::Face(face) => self.face(face),
        }
    }

    fn plane(&self, plane: &PlaneRef) -> Value {
        match plane {
            PlaneRef::Origin(plane) => json!(plane_word(*plane)),
            PlaneRef::Face(face) => self.face(face),
        }
    }

    fn point(&self, point: &PointRef) -> Value {
        match point {
            PointRef::Origin => json!("origin"),
            PointRef::Corner { body, faces, near } => {
                let corner = Ref::Corner {
                    body: *body,
                    faces: *faces,
                    point: near.to_array(),
                };
                match self.key(corner, ItemKind::Corner) {
                    Ok(key) => json!({ "corner": key }),
                    Err(why) => unresolved(why),
                }
            }
            PointRef::Middle(edge) | PointRef::Centre(edge) => self.edge(edge),
        }
    }

    fn direction(&self, direction: &DirRef) -> Value {
        match direction {
            DirRef::Origin(axis) => json!(axis_word(*axis)),
            DirRef::Normal(face) => self.face(face),
            DirRef::Axis(axis) => self.axis(axis),
        }
    }

    fn align_side(&self, side: &AlignRefs) -> Value {
        let mut said = json!({ "point": self.point(&side.point) });
        if let Some(primary) = &side.primary {
            said["direction"] = self.direction(primary);
        }
        if let Some(secondary) = &side.secondary {
            said["second_direction"] = self.direction(secondary);
        }
        said
    }
}
