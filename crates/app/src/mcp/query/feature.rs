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
    PointRef, RegionRef, ScaleFactor, Section, SplitTool,
};
use varde_mcp::tools::{
    self, FeatureTool, ItemKind, Page, ToolReply, Worded, WorldAxis, WorldPlane,
};
use varde_view::MotionField;

use super::{ListAt, failure};
use crate::doc::{CombineSession, Doc, ExtrudeSession, MotionSession, RevolveSession, sketch_of};
use crate::mcp::args;
use crate::mcp::key::{self, Key, Ref};

/// `feature {name, offset?, limit?}`: see the module's docs.
pub(crate) fn feature(doc: &Doc, name: &str, page: Page) -> ToolReply {
    let feature = match super::super::names::feature_reply(doc, name) {
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
        answer["tool"] = json!(tool.name());
    }
    let failures = doc.feed.committed().map(|model| model.failures);
    if let Some(failure) = failures.and_then(|failures| failure(failures, feature)) {
        answer["failure"] = json!(failure.message);
    }
    ToolReply::data(paged(answer, page))
}

/// `answer` with each list in its `settings` paged by `page` and
/// shortened to fit ([`super::pages`]), a list given whole left a plain
/// array: see the module's docs.
fn paged(answer: Value, page: Page) -> Value {
    let Some(settings) = answer["settings"].as_object() else {
        return answer;
    };
    let (at, lists): (Vec<ListAt>, Vec<Vec<Value>>) = super::lists_in(settings, Value::is_array)
        .into_iter()
        .map(|(at, list)| (at, list.as_array().cloned().unwrap_or_default()))
        .unzip();
    if lists.is_empty() {
        return answer;
    }
    super::pages(&lists, page, super::BUDGET, |pages| {
        let mut said = answer.clone();
        let mut partial = Vec::new();
        for (list, mut value) in at.iter().zip(pages) {
            if page.offset == 0 && value["left_out"] == 0 {
                value = value["items"].take();
            } else {
                partial.push(list.name());
            }
            let ListAt { key, sub } = list;
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
pub(crate) fn tool_of(kind: &FeatureKind) -> Option<FeatureTool> {
    Some(match kind {
        FeatureKind::Sketch { .. } => return None,
        FeatureKind::Extrude(_) => FeatureTool::Extrude,
        FeatureKind::Revolve(_) => FeatureTool::Revolve,
        FeatureKind::Combine(_) => FeatureTool::Combine,
        FeatureKind::Move(_) => FeatureTool::MoveBody,
        FeatureKind::Mirror(_) => FeatureTool::Mirror,
        FeatureKind::Pattern(_) => FeatureTool::Pattern,
        FeatureKind::Align(_) => FeatureTool::Align,
        FeatureKind::Scale(_) => FeatureTool::Scale,
        FeatureKind::Split(_) => FeatureTool::Split,
        FeatureKind::Chamfer(_) => FeatureTool::Chamfer,
        FeatureKind::Shell(_) => FeatureTool::Shell,
        FeatureKind::Fillet(_) => FeatureTool::Fillet,
        FeatureKind::OffsetFace(_) => FeatureTool::OffsetFace,
        FeatureKind::FaceDraft(_) => FeatureTool::Draft,
        FeatureKind::Sweep(_) => FeatureTool::Sweep,
        FeatureKind::Loft(_) => FeatureTool::Loft,
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
        Operation::NewBody(_) => tools::Operation::NewBody,
        Operation::Join(_) => tools::Operation::Join,
        Operation::Cut(_) => tools::Operation::Cut,
        Operation::Intersect(_) => tools::Operation::Intersect,
    }
    .word()
}

/// What's said in place of a reference that can't be named.
fn unresolved(why: impl Into<String>) -> Value {
    json!({ "unresolved": why.into() })
}

/// A feature's settings as they're shaped, by key.
#[derive(Default)]
struct Said(Map<String, Value>);

impl Said {
    fn put(&mut self, key: &str, value: Value) {
        self.0.insert(key.to_owned(), value);
    }
}

/// A motion panel's field as its text shows.
fn text(session: &MotionSession, field: MotionField) -> Value {
    json!(session.field(field).text)
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

    /// `feature`'s settings as `feature` gives them: [`Shape::of`], a
    /// sketch summed up by its plane and counts.
    pub(crate) fn settings(&self, feature: &Feature) -> Value {
        match &feature.kind {
            FeatureKind::Sketch { plane, sketch, .. } => json!({
                "plane": self.plane(plane),
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
    /// Each kind's are shaped by its own method, the mirror of its setter.
    pub(crate) fn of(&self, feature: &Feature) -> Option<Map<String, Value>> {
        let id = feature.id;
        let mut said = Said::default();
        match &feature.kind {
            FeatureKind::Sketch { .. } => return None,
            FeatureKind::Extrude(extrude) => self.extrude(&mut said, id, extrude),
            FeatureKind::Revolve(revolve) => self.revolve(&mut said, id, revolve),
            FeatureKind::Combine(combine) => self.combine(&mut said, id, combine),
            kind => {
                let shape = self.doc.pattern_shapes.get(&id);
                let session = args::editing_for_client(self.document, id, shape)?;
                let session = &session;
                match kind {
                    FeatureKind::Move(moved) => self.move_body(&mut said, session, moved),
                    FeatureKind::Mirror(mirror) => self.mirror(&mut said, mirror),
                    FeatureKind::Pattern(pattern) => self.pattern(&mut said, session, pattern),
                    FeatureKind::Align(align) => self.align(&mut said, session, align),
                    FeatureKind::Scale(scale) => self.scale(&mut said, session, scale),
                    FeatureKind::Split(split) => self.split(&mut said, split),
                    FeatureKind::Shell(shell) => self.shell(&mut said, session, shell),
                    FeatureKind::OffsetFace(offset) => self.offset_face(&mut said, session, offset),
                    FeatureKind::FaceDraft(draft) => self.draft(&mut said, session, draft),
                    FeatureKind::Chamfer(chamfer) => self.chamfer(&mut said, session, chamfer),
                    FeatureKind::Fillet(fillet) => self.fillet(&mut said, session, fillet),
                    FeatureKind::Sweep(sweep) => self.sweep(&mut said, session, sweep),
                    FeatureKind::Loft(loft) => self.loft(&mut said, loft),
                    _ => return None,
                }
            }
        }
        Some(said.0)
    }

    fn extrude(&self, said: &mut Said, id: FeatureId, extrude: &varde_document::Extrude) {
        let session = ExtrudeSession::editing_for_client(self.document, id, extrude);
        said.put("sketch", self.feature_name(extrude.sketch));
        said.put("profiles", self.regions(extrude.sketch, &extrude.regions));
        let (word, distances) = match &extrude.extent {
            varde_document::Extent::OneSide(_) => (tools::ExtrudeExtent::OneSide, 1),
            varde_document::Extent::Symmetric(_) => (tools::ExtrudeExtent::Symmetric, 1),
            varde_document::Extent::TwoSides(..) => (tools::ExtrudeExtent::TwoSides, 2),
            varde_document::Extent::ThroughAll => (tools::ExtrudeExtent::ThroughAll, 0),
        };
        said.put("extent", json!(word.word()));
        if distances >= 1 {
            said.put("distance", json!(session.sides.fields[0].text));
        }
        if distances >= 2 {
            said.put("second_distance", json!(session.sides.fields[1].text));
        }
        said.put("flip", json!(extrude.flip));
        said.put("taper", json!(session.taper.text));
        self.operation(said, &extrude.operation);
    }

    fn revolve(&self, said: &mut Said, id: FeatureId, revolve: &varde_document::Revolve) {
        let session = RevolveSession::editing(self.document, id, revolve);
        said.put("sketch", self.feature_name(revolve.sketch));
        said.put("profiles", self.regions(revolve.sketch, &revolve.regions));
        let axis = match &revolve.axis {
            AxisLine::SketchX => json!("x"),
            AxisLine::SketchY => json!("y"),
            AxisLine::Curve(line) => json!({ "line": line.get() }),
            AxisLine::Edge(edge) => self.edge(edge),
        };
        said.put("axis", axis);
        let (word, angles) = match &revolve.extent {
            varde_document::Turn::Full => (tools::RevolveExtent::Full, 0),
            varde_document::Turn::OneSide(_) => (tools::RevolveExtent::OneSide, 1),
            varde_document::Turn::Symmetric(_) => (tools::RevolveExtent::Symmetric, 1),
            varde_document::Turn::TwoSides(..) => (tools::RevolveExtent::TwoSides, 2),
        };
        said.put("extent", json!(word.word()));
        if angles >= 1 {
            said.put("angle", json!(session.sides.fields[0].text));
        }
        if angles >= 2 {
            said.put("second_angle", json!(session.sides.fields[1].text));
        }
        said.put("flip", json!(revolve.flip));
        self.operation(said, &revolve.operation);
    }

    fn combine(&self, said: &mut Said, id: FeatureId, combine: &varde_document::Combine) {
        let session = CombineSession::editing(id, combine);
        said.put("target", self.body(combine.target));
        said.put("tools", self.bodies(&combine.tools));
        let op = match session.op {
            varde_document::BodyOp::Union => tools::BodyOp::Union,
            varde_document::BodyOp::Subtract => tools::BodyOp::Subtract,
            varde_document::BodyOp::Intersect => tools::BodyOp::Intersect,
        };
        said.put("operation", json!(op.word()));
        said.put("keep_tools", json!(session.keep_tools));
    }

    fn move_body(&self, said: &mut Said, session: &MotionSession, moved: &varde_document::Move) {
        said.put("bodies", self.bodies(&moved.bodies));
        for (key, axis) in ["x", "y", "z"].into_iter().zip(varde_document::Axis3::ALL) {
            said.put(key, text(session, MotionField::Offset(axis)));
        }
        if let Some((axis, _)) = &moved.turn {
            said.put("angle", text(session, MotionField::Angle));
            said.put("axis", self.axis(axis));
        }
    }

    fn mirror(&self, said: &mut Said, mirror: &varde_document::Mirror) {
        said.put("bodies", self.bodies(&mirror.bodies));
        said.put("plane", self.plane(&mirror.plane));
        said.put("keep_original", json!(mirror.keep_original));
    }

    fn pattern(&self, said: &mut Said, session: &MotionSession, pattern: &varde_document::Pattern) {
        let linear = matches!(pattern.kind, PatternKind::Linear { .. });
        let kind = if linear {
            tools::PatternKind::Linear
        } else {
            tools::PatternKind::Circular
        };
        said.put("kind", json!(kind.word()));
        said.put("bodies", self.bodies(&pattern.bodies));
        said.put("axis", self.axis(pattern.kind.axis()));
        said.put("count", text(session, MotionField::Count));
        let mode = match session.mode {
            varde_view::PatternMode::Spacing => tools::PatternMode::Spacing,
            varde_view::PatternMode::Total => tools::PatternMode::Total,
            varde_view::PatternMode::Full => tools::PatternMode::Full,
        };
        said.put("mode", json!(mode.word()));
        if session.mode != varde_view::PatternMode::Full {
            said.put("spread", text(session, MotionField::Spread));
        }
        if linear {
            said.put("flip", json!(session.flip));
        }
        said.put("join", json!(pattern.copies == Copies::Joined));
    }

    fn align(&self, said: &mut Said, session: &MotionSession, align: &varde_document::Align) {
        said.put("body", self.body(align.body));
        said.put("from", self.align_side(&align.from));
        said.put("to", self.align_side(&align.to));
        said.put("flip", json!(align.flip));
        said.put("distance", text(session, MotionField::Distance));
        said.put("angle", text(session, MotionField::Angle));
    }

    fn scale(&self, said: &mut Said, session: &MotionSession, scale: &varde_document::Scale) {
        said.put("bodies", self.bodies(&scale.bodies));
        said.put("point", self.point(&scale.about));
        match &scale.factor {
            ScaleFactor::Uniform(_) => {
                said.put("mode", json!(tools::ScaleMode::Uniform.word()));
                said.put("factor", text(session, MotionField::Factor));
            }
            ScaleFactor::PerAxis(_) => {
                said.put("mode", json!(tools::ScaleMode::PerAxis.word()));
                for (key, axis) in ["x", "y", "z"].into_iter().zip(varde_document::Axis3::ALL) {
                    said.put(key, text(session, MotionField::AxisFactor(axis)));
                }
            }
            ScaleFactor::EdgeLength {
                edge, axis_only, ..
            } => {
                said.put("mode", json!(tools::ScaleMode::EdgeLength.word()));
                said.put("edge", self.edge_key(edge));
                said.put("length", text(session, MotionField::Length));
                said.put("axis_only", json!(axis_only));
            }
        }
    }

    fn split(&self, said: &mut Said, split: &varde_document::Split) {
        said.put("body", self.body(split.body));
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
        said.put("tool", tool);
        let side = |side: varde_document::Side| match side {
            varde_document::Side::Front => tools::SplitSide::Front.word(),
            varde_document::Side::Back => tools::SplitSide::Back.word(),
        };
        let keep = match split.keep {
            varde_document::Keep::Both => tools::SplitKeep::Both,
            varde_document::Keep::Front => tools::SplitKeep::Front,
            varde_document::Keep::Back => tools::SplitKeep::Back,
        }
        .word();
        if split.keep == varde_document::Keep::Both {
            said.put("original", json!(side(split.original)));
        }
        said.put("keep", json!(keep));
    }

    fn shell(&self, said: &mut Said, session: &MotionSession, shell: &varde_document::Shell) {
        // A shell's body is its faces' when it has any.
        if shell.open.is_empty() {
            said.put("body", self.body(shell.body));
        }
        said.put("faces", self.face_keys(&shell.open));
        said.put("thickness", text(session, MotionField::Thickness));
        let direction = if shell.outward {
            tools::ShellDirection::Outward
        } else {
            tools::ShellDirection::Inward
        };
        said.put("direction", json!(direction.word()));
    }

    fn offset_face(
        &self,
        said: &mut Said,
        session: &MotionSession,
        offset: &varde_document::OffsetFace,
    ) {
        said.put("faces", self.face_keys(&offset.faces));
        said.put("distance", text(session, MotionField::Distance));
        said.put("inward", json!(offset.inward));
        said.put("tangent", json!(offset.tangent));
    }

    fn draft(&self, said: &mut Said, session: &MotionSession, draft: &varde_document::FaceDraft) {
        said.put("faces", self.face_keys(&draft.faces));
        said.put("neutral", self.plane(&draft.neutral));
        said.put("angle", text(session, MotionField::Angle));
        said.put("flip", json!(draft.flip));
        said.put("tangent", json!(draft.tangent));
    }

    fn chamfer(&self, said: &mut Said, session: &MotionSession, chamfer: &varde_document::Chamfer) {
        said.put("edges", self.edge_keys(&chamfer.edges));
        said.put("faces", self.face_keys(&chamfer.faces));
        let distance = text(session, MotionField::ChamferDistance);
        match &chamfer.distances {
            ChamferSize::Equal(_) => {
                said.put("type", json!(tools::ChamferType::Equal.word()));
                said.put("distance", distance);
            }
            ChamferSize::Two(..) => {
                said.put("type", json!(tools::ChamferType::TwoDistances.word()));
                said.put("distance", distance);
                said.put("second_distance", text(session, MotionField::ChamferSecond));
                said.put("flip", json!(chamfer.flip));
            }
            ChamferSize::Angle(..) => {
                said.put("type", json!(tools::ChamferType::DistanceAngle.word()));
                said.put("distance", distance);
                said.put("angle", text(session, MotionField::ChamferAngle));
                said.put("flip", json!(chamfer.flip));
            }
        }
        // The panel's tick, on as it opens: the edges are stored each
        // of its own.
        said.put("tangent", json!(session.blend.chains));
    }

    fn fillet(&self, said: &mut Said, session: &MotionSession, fillet: &varde_document::Fillet) {
        said.put("edges", self.edge_keys(&fillet.edges));
        said.put("faces", self.face_keys(&fillet.faces));
        said.put("radius", text(session, MotionField::Radius));
        said.put("tangent", json!(session.blend.chains));
    }

    fn sweep(&self, said: &mut Said, session: &MotionSession, sweep: &varde_document::Sweep) {
        said.put("sketch", self.feature_name(sweep.sketch));
        said.put("profiles", self.regions(sweep.sketch, &sweep.regions));
        match &sweep.path {
            varde_document::PathRef::Chain(parts) => {
                said.put("path", self.sweep_path(parts));
                let keep = sweep.orientation == varde_document::Orientation::Keep;
                said.put("keep_orientation", json!(keep));
                said.put("twist", text(session, MotionField::Twist));
            }
            varde_document::PathRef::Helix(helix) => said.put(
                "helix",
                json!({
                    "axis": self.axis(&helix.axis),
                    "pitch": text(session, MotionField::Pitch),
                    "turns": text(session, MotionField::Turns),
                    "left_handed": helix.left_handed,
                    "flip": helix.flip,
                }),
            ),
        }
        self.operation(said, &sweep.operation);
    }

    /// A sweep's chained path: its sketch curves and model edges.
    fn sweep_path(&self, parts: &[PathPart]) -> Value {
        let mut curves = Vec::new();
        let mut edges = Vec::new();
        let mut tangent = None;
        for part in parts {
            match part {
                PathPart::Curves(chain) => curves.push(self.chain_start(chain)),
                PathPart::Edges {
                    edges: some,
                    tangent: tick,
                } => {
                    edges.extend(some.iter().map(|e| self.edge_key(e)));
                    tangent.get_or_insert(*tick);
                }
            }
        }
        json!({
            "curves": curves,
            "edges": edges,
            "tangent": tangent.unwrap_or(true),
        })
    }

    fn loft(&self, said: &mut Said, loft: &varde_document::Loft) {
        let sections: Vec<Value> = loft.sections.iter().map(|s| self.loft_section(s)).collect();
        said.put("sections", json!(sections));
        let mode = match loft.mode {
            varde_document::LoftMode::Smooth => tools::LoftMode::Smooth,
            varde_document::LoftMode::Ruled => tools::LoftMode::Ruled,
        };
        said.put("mode", json!(mode.word()));
        said.put("closed", json!(loft.closed));
        let rails: Vec<Value> = loft.rails.iter().map(|c| self.chain_start(c)).collect();
        said.put("rails", json!(rails));
        self.operation(said, &loft.operation);
    }

    /// A loft's section: a sketch's region (and its start point if
    /// set) or point.
    fn loft_section(&self, section: &Section) -> Value {
        match section {
            Section::Region {
                sketch,
                region,
                start,
            } => {
                let regions = self.regions(*sketch, std::slice::from_ref(region));
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
        }
    }

    /// A chain of sketch curves (a sweep path's part, a loft's rail) by
    /// its sketch and first curve.
    fn chain_start(&self, chain: &varde_document::CurveChain) -> Value {
        json!({
            "sketch": self.feature_name(chain.sketch),
            "curve": chain.curves.first().map(|id| id.get()),
        })
    }

    /// An operation and, for a join, cut or intersect, the bodies it
    /// leaves out.
    fn operation(&self, said: &mut Said, operation: &Operation) {
        said.put("operation", json!(operation_word(operation)));
        if !matches!(operation, Operation::NewBody(_)) {
            said.put("exclude", self.bodies(operation.excluded()));
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
        let merged = crate::mcp::args::committed_merges(doc, None)?;
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

    /// `item`'s key alone, or why it has none.
    fn bare_key(&self, item: Ref, kind: ItemKind) -> Value {
        match self.key(item, kind) {
            Ok(key) => json!(key),
            Err(why) => unresolved(why),
        }
    }

    /// `item` as a reference, its key wrapped in its kind's word
    /// (`{"face": key}`), or why it has none.
    fn wrapped_key(&self, item: Ref, kind: ItemKind) -> Value {
        match self.key(item, kind) {
            Ok(key) => json!({ kind.word(): key }),
            Err(why) => unresolved(why),
        }
    }

    /// The faces' keys alone, as a face tool lists them.
    fn face_keys(&self, faces: &[FaceRef]) -> Value {
        let keys = faces
            .iter()
            .map(|face| self.bare_key(Ref::Face(*face), ItemKind::Face));
        Value::Array(keys.collect())
    }

    /// An edge's key alone.
    fn edge_key(&self, edge: &EdgeRef) -> Value {
        self.bare_key(Ref::Edge(*edge), ItemKind::Edge)
    }

    fn edge_keys(&self, edges: &[EdgeRef]) -> Value {
        Value::Array(edges.iter().map(|edge| self.edge_key(edge)).collect())
    }

    /// A face as a reference: `{"face": key}`.
    pub(crate) fn face(&self, face: &FaceRef) -> Value {
        self.wrapped_key(Ref::Face(*face), ItemKind::Face)
    }

    /// An edge as a reference: `{"edge": key}`.
    fn edge(&self, edge: &EdgeRef) -> Value {
        self.wrapped_key(Ref::Edge(*edge), ItemKind::Edge)
    }

    fn axis(&self, axis: &AxisRef) -> Value {
        match axis {
            AxisRef::Origin(axis) => json!(axis_word(*axis)),
            AxisRef::Edge(edge) => self.edge(edge),
            AxisRef::Face(face) => self.face(face),
        }
    }

    /// A `plane`: an origin plane's word, or its face by key.
    pub(crate) fn plane(&self, plane: &Plane) -> Value {
        match plane {
            Plane::Origin(plane) => json!(plane_word(*plane)),
            Plane::Face(face) => self.face(face),
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
                self.wrapped_key(corner, ItemKind::Corner)
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
