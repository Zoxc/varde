//! The tools: the static `tools/list` of what's built, a `tools/call`'s
//! params read into a checked [`Call`], and the [`ToolReply`] a call is
//! answered with.
//!
//! A call naming no tool of ours is a JSON-RPC error (-32602); arguments
//! that don't fit its tool are a tool result with `isError`, as MCP has
//! input validation errors, so the model reads why and tries again.
//! Everything in the arguments comes off the network: names bounded,
//! directions finite, not zero and normalized, offsets and limits
//! converted with `usize::try_from` and limits capped.

use crate::shown;
use serde_json::{Map, Value, json};

mod args;
mod call;
mod draw;
mod reply;
mod schema;

use args::*;
pub use call::*;
pub use draw::{
    At, ConstraintWord, EditSketch, ItemRef, LABEL_BYTES, MAX_OPERATIONS, MAX_SIDES, MIN_SIDES,
    MeasureWord, Op, PolygonSize, RESERVED, RectangleFrom, SPLINE_POINTS, SketchOp,
};
pub use reply::ToolReply;
pub use schema::list;

/// The longest name taken (a feature's, a body's), in bytes: as long as
/// the longest a design may hold (`varde_document::MAX_NAME_LEN`), so
/// every feature and body can be named.
///
/// This and the other limits mirrored from other crates are held to
/// their sources by the app (`varde_app`'s `mcp::mirrored`).
pub const NAME_BYTES: usize = 1024;

/// `design_summary`'s default `limit`, per listing: a page of each of
/// its three listings fits under [`ToolReply::CAP`] even with parameters
/// at their longest (about 400 bytes each), names of usual length.
pub const SUMMARY_LIMIT: usize = 30;

/// `body_topology`'s default `limit`, per listing: a page of each of its
/// three listings, at about 850 bytes for a face, an edge and a corner
/// together, fits well under [`ToolReply::CAP`].
pub const TOPOLOGY_LIMIT: usize = 16;

/// `feature`'s default `limit`, per list in a feature's settings (a
/// chamfer's edges and faces, a loft's sections and rails, ...): at
/// about 120 bytes for an edge's key, a page of one fits well under
/// [`ToolReply::CAP`]; the app shortens the pages past that (several
/// long lists, names at their longest) rather than cut the reply.
pub const FEATURE_LIMIT: usize = 50;

/// `sketch`'s default `limit`, per listing: a page of each of its five
/// listings (points, curves, constraints, dimensions, regions), at about
/// 500 bytes for one of each, fits under [`ToolReply::CAP`].
pub const SKETCH_LIMIT: usize = 20;

/// The largest `limit` taken; more is cut down to it.
pub const MAX_LIMIT: usize = 500;

/// Why `tools/call` params weren't a call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallError {
    /// No such tool, or params not shaped as `tools/call`'s: -32602.
    Params(String),
    /// The tool's arguments don't fit it: a tool result with `isError`,
    /// worded `"{at}: {message}"`.
    Arguments {
        /// Whose arguments: the tool's name, or the path to an item of
        /// them, `"edit_sketch: operations[2]"`.
        at: String,
        /// What doesn't fit.
        message: String,
    },
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Params(message) => f.write_str(message),
            CallError::Arguments { at, message } => write!(f, "{at}: {message}"),
        }
    }
}

/// Reads `tools/call`'s params: `{name, arguments?}`.
pub fn parse_call(params: Option<&Value>) -> Result<Call, CallError> {
    let bad = |m: &str| CallError::Params(m.to_owned());
    let Some(Value::Object(params)) = params else {
        return Err(bad("tools/call needs params with the tool's name"));
    };
    let Some(Value::String(name)) = params.get("name") else {
        return Err(bad("tools/call needs the tool's name"));
    };
    let empty = Map::new();
    let arguments = match params.get("arguments") {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(arguments)) => arguments,
        Some(_) => return Err(bad("A tool's arguments are an object")),
    };
    parse_arguments(name, arguments)
}

/// Reads `arguments` as the tool `tool`'s, as `tools/call` does (a
/// `set_feature` patch: [`parse_patch`]).
pub fn parse_arguments(tool: &str, arguments: &Map<String, Value>) -> Result<Call, CallError> {
    let name = tool;
    let mut args = Arguments {
        tool: name,
        map: arguments,
    };
    // The names of new bodies a feature tool takes, and its arguments
    // checked here once for every feature tool.
    let feature = FeatureTool::named(name);
    let new_bodies = feature.map_or(NewBodies::None, FeatureTool::new_bodies);
    if let Some(tool) = feature {
        args.only_making(tool.fields(), new_bodies)?;
    }
    let call = match name {
        "design_summary" => {
            args.only(&["offset", "limit"])?;
            Call::DesignSummary(page(&args, SUMMARY_LIMIT)?)
        }
        "feature" => {
            args.only(&["name", "offset", "limit"])?;
            Call::Feature {
                name: args.name("name")?,
                page: page(&args, FEATURE_LIMIT)?,
            }
        }
        "body_topology" => {
            args.only(&["body", "faces", "edges", "offset", "limit"])?;
            Call::BodyTopology(topology(&mut args)?)
        }
        "measure" => {
            args.only(&["a", "b"])?;
            let a = args.measured("a")?;
            let a = a.ok_or_else(|| args.error(format_args!("a is required: {MEASURED}")))?;
            Call::Measure(Measure {
                a,
                b: args.measured("b")?,
            })
        }
        "render" => {
            args.only(&["bodies", "from", "projection", "zoom", "highlight", "size"])?;
            Call::Render(render(&args)?)
        }
        "sketch" => {
            args.only(&["name", "offset", "limit"])?;
            Call::Sketch(SketchQuery {
                name: args.name("name")?,
                page: page(&args, SKETCH_LIMIT)?,
            })
        }
        "rename" => {
            args.only(&["feature", "body", "to"])?;
            let target = match (
                args.map.contains_key("feature"),
                args.map.contains_key("body"),
            ) {
                (true, false) => Target::Feature(args.name("feature")?),
                (false, true) => Target::Body(args.name("body")?),
                _ => return Err(args.error("give either feature or body, not both")),
            };
            Call::Rename(Rename {
                target,
                to: args.name("to")?,
            })
        }
        "set_visible" => {
            args.only(&["body", "visible"])?;
            Call::SetVisible {
                body: args.name("body")?,
                visible: args.required_boolean("visible")?,
            }
        }
        "set_color" => {
            args.only(&["body", "color"])?;
            let body = args.name("body")?;
            let color = match args.nullable("color", "the theme's")? {
                None => None,
                Some(Value::Object(map)) => {
                    let color = args.nested(map);
                    color.only(&["hue", "saturation"])?;
                    Some(Color {
                        hue: color.number_in(
                            "hue",
                            0.0..360.0,
                            "in degrees, from 0 to under 360",
                        )?,
                        saturation: color.number_in(
                            "saturation",
                            0.0..=MAX_SATURATION,
                            format_args!("from 0 to {MAX_SATURATION}"),
                        )?,
                    })
                }
                Some(_) => return Err(args.error("color is {hue, saturation} or null")),
            };
            Call::SetColor { body, color }
        }
        "set_opacity" => {
            args.only(&["body", "opacity"])?;
            Call::SetOpacity {
                body: args.name("body")?,
                opacity: args.number_in(
                    "opacity",
                    MIN_OPACITY..=1.0,
                    format_args!("from {MIN_OPACITY} to 1"),
                )?,
            }
        }
        "remove_feature" => Call::RemoveFeature {
            feature: args.sole_name("feature")?,
        },
        "remove_body" => Call::RemoveBody {
            body: args.sole_name("body")?,
        },
        tool @ ("add_parameter" | "set_parameter") => {
            args.only(&["name", "value"])?;
            let name = args.name("name")?;
            let value = args.text("value")?;
            match tool {
                "add_parameter" => Call::AddParameter { name, value },
                _ => Call::SetParameter { name, value },
            }
        }
        "rename_parameter" => {
            args.only(&["name", "to"])?;
            Call::RenameParameter {
                name: args.name("name")?,
                to: args.name("to")?,
            }
        }
        "remove_parameter" => Call::RemoveParameter {
            name: args.sole_name("name")?,
        },
        "set_units" => {
            args.only(&["units"])?;
            Call::SetUnits(args.required_choice("units", UNITS)?)
        }
        "set_tolerance" => {
            args.only(&["tolerance"])?;
            Call::SetTolerance {
                tolerance: args.text("tolerance")?,
            }
        }
        "undo" => {
            args.only(&["steps"])?;
            let steps = args.count("steps")?.unwrap_or(1);
            if !(1..=MAX_UNDO_STEPS).contains(&steps) {
                return Err(args.error(format_args!(
                    "steps is a whole number from 1 to {MAX_UNDO_STEPS}"
                )));
            }
            Call::Undo { steps }
        }
        "set_rollback" => {
            args.only(&["before"])?;
            let before = match args.nullable("before", "the end")? {
                None => None,
                Some(_) => Some(args.name("before")?),
            };
            Call::SetRollback { before }
        }
        "add_sketch" => {
            args.only(&["plane", "name"])?;
            Call::AddSketch {
                plane: args.sketch_plane()?,
                name: args.optional_name("name")?,
            }
        }
        "set_sketch_plane" => {
            args.only(&["sketch", "plane"])?;
            Call::SetSketchPlane {
                sketch: args.name("sketch")?,
                plane: args.sketch_plane()?,
            }
        }
        "add_link" => {
            args.only(&["sketch", "kind", "from"])?;
            Call::AddLink(AddLink {
                sketch: args.name("sketch")?,
                kind: args.choice("kind", LINK_KINDS)?,
                from: args.link_from()?,
            })
        }
        "edit_sketch" => {
            args.only(&["sketch", "operations"])?;
            Call::EditSketch(EditSketch {
                sketch: args.name("sketch")?,
                operations: args.operations()?,
            })
        }
        "set_feature" => {
            let name = args.name("name")?;
            let patch: Map<String, Value> = (args.map.iter())
                .filter(|(key, _)| *key != "name")
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            if let Some(why) = renaming(&patch) {
                return Err(args.error(why));
            }
            if patch.is_empty() {
                return Err(args.error(
                    "give the fields to change beside name, as feature shows them (to \
                     rename the feature, use rename)",
                ));
            }
            Call::SetFeature(SetFeature { name, patch })
        }
        "extrude" => Call::Extrude(Extrude {
            naming: Box::new(args.naming(new_bodies)?),
            sketch: args.name("sketch")?,
            profiles: args.profiles()?,
            extent: args.choice("extent", EXTRUDE_EXTENTS)?,
            distance: args.optional_text("distance")?,
            second_distance: args.optional_text("second_distance")?,
            flip: args.flag("flip", false)?,
            taper: args.optional_text("taper")?,
            operation: args.choice("operation", OPERATIONS)?,
            exclude: args.names("exclude")?,
        }),
        "revolve" => Call::Revolve(Revolve {
            naming: Box::new(args.naming(new_bodies)?),
            sketch: args.name("sketch")?,
            profiles: args.profiles()?,
            axis: args.axis()?,
            extent: args.choice("extent", REVOLVE_EXTENTS)?,
            angle: args.optional_text("angle")?,
            second_angle: args.optional_text("second_angle")?,
            flip: args.flag("flip", false)?,
            operation: args.choice("operation", OPERATIONS)?,
            exclude: args.names("exclude")?,
        }),
        "combine" => {
            let operation = args.choice("operation", BODY_OPS)?;
            let tools = args.names("tools")?;
            if tools.is_empty() {
                return Err(args.error("tools is required: at least one body"));
            }
            Call::Combine(Combine {
                naming: Box::new(args.naming(new_bodies)?),
                target: args.name("target")?,
                tools,
                operation,
                keep_tools: args.flag("keep_tools", false)?,
            })
        }
        "move_body" => Call::MoveBody(MoveBody {
            naming: Box::new(args.naming(new_bodies)?),
            bodies: args.bodies()?,
            offset: [
                args.optional_text("x")?,
                args.optional_text("y")?,
                args.optional_text("z")?,
            ],
            angle: args.optional_text("angle")?,
            axis: args.reference("axis", WORLD_AXES, AXIS_ITEMS)?,
        }),
        "mirror" => {
            let bodies = args.bodies()?;
            let plane = args.reference("plane", WORLD_PLANES, &[ItemKind::Face])?;
            Call::Mirror(Mirror {
                naming: Box::new(args.naming(new_bodies)?),
                bodies,
                plane: plane.ok_or_else(|| args.error("plane is required"))?,
                keep_original: args.flag("keep_original", true)?,
            })
        }
        "pattern" => Call::Pattern(Pattern {
            naming: Box::new(args.naming(new_bodies)?),
            kind: args.choice("kind", PATTERN_KINDS)?,
            bodies: args.bodies()?,
            axis: args.reference("axis", WORLD_AXES, AXIS_ITEMS)?,
            count: args.count_text("count")?,
            mode: args.optional_choice("mode", PATTERN_MODES)?,
            spread: args.optional_text("spread")?,
            flip: args.flag("flip", false)?,
            join: args.flag("join", true)?,
        }),
        "align" => Call::Align(Box::new(Align {
            naming: Box::new(args.naming(new_bodies)?),
            body: args.optional_name("body")?,
            from: args.align_side("from")?,
            to: args.align_side("to")?,
            flip: args.flag("flip", false)?,
            distance: args.optional_text("distance")?,
            angle: args.optional_text("angle")?,
        })),
        "scale" => Call::Scale(Box::new(Scale {
            naming: Box::new(args.naming(new_bodies)?),
            bodies: args.bodies()?,
            point: args.reference("point", ORIGIN, POINT_ITEMS)?,
            mode: args.choice("mode", SCALE_MODES)?,
            factor: args.optional_text("factor")?,
            factors: [
                args.optional_text("x")?,
                args.optional_text("y")?,
                args.optional_text("z")?,
            ],
            edge: args.optional_name("edge")?,
            length: args.optional_text("length")?,
            axis_only: args.flag("axis_only", false)?,
        })),
        "split" => Call::Split(Split {
            naming: Box::new(args.naming(new_bodies)?),
            body: args.name("body")?,
            tool: args.split_tool()?,
            original: args.optional_choice("original", SPLIT_SIDES)?,
            keep: args.choice("keep", SPLIT_KEEPS)?,
        }),
        "shell" => Call::Shell(Shell {
            naming: Box::new(args.naming(new_bodies)?),
            body: args.optional_name("body")?,
            faces: args.names("faces")?,
            thickness: args.optional_text("thickness")?,
            direction: args.choice("direction", SHELL_DIRECTIONS)?,
        }),
        "offset_face" => Call::OffsetFace(OffsetFace {
            naming: Box::new(args.naming(new_bodies)?),
            faces: args.faces()?,
            distance: args.optional_text("distance")?,
            inward: args.flag("inward", false)?,
            tangent: args.flag("tangent", true)?,
        }),
        "draft" => Call::Draft(Draft {
            naming: Box::new(args.naming(new_bodies)?),
            faces: args.faces()?,
            neutral: args.reference("neutral", WORLD_PLANES, &[ItemKind::Face])?,
            angle: args.optional_text("angle")?,
            flip: args.flag("flip", false)?,
            tangent: args.flag("tangent", true)?,
        }),
        "chamfer" => {
            let (edges, faces) = args.edges_and_faces()?;
            Call::Chamfer(Chamfer {
                naming: Box::new(args.naming(new_bodies)?),
                edges,
                faces,
                kind: args.choice("type", CHAMFER_TYPES)?,
                distance: args.optional_text("distance")?,
                second_distance: args.optional_text("second_distance")?,
                angle: args.optional_text("angle")?,
                flip: args.flag("flip", false)?,
                tangent: args.flag("tangent", true)?,
            })
        }
        "fillet" => {
            let (edges, faces) = args.edges_and_faces()?;
            Call::Fillet(Fillet {
                naming: Box::new(args.naming(new_bodies)?),
                edges,
                faces,
                radius: args.optional_text("radius")?,
                tangent: args.flag("tangent", true)?,
            })
        }
        "sweep" => Call::Sweep(Box::new(Sweep {
            naming: Box::new(args.naming(new_bodies)?),
            sketch: args.name("sketch")?,
            profiles: args.profiles()?,
            along: args.sweep_along()?,
            keep_orientation: args.flag("keep_orientation", false)?,
            twist: args.optional_text("twist")?,
            operation: args.choice("operation", OPERATIONS)?,
            exclude: args.names("exclude")?,
        })),
        "loft" => Call::Loft(Loft {
            naming: Box::new(args.naming(new_bodies)?),
            sections: args.sections()?,
            mode: args.choice("mode", LOFT_MODES)?,
            closed: args.flag("closed", false)?,
            rails: args.curves_in("rails")?,
            operation: args.choice("operation", OPERATIONS)?,
            exclude: args.names("exclude")?,
        }),
        _ => {
            return Err(CallError::Params(format!(
                "No tool named {:?}",
                shown(name)
            )));
        }
    };
    Ok(call)
}

/// The creating tools' arguments naming new bodies, which a
/// `set_feature` patch doesn't take: renaming is `rename`'s.
const RENAMES: &[&str] = &["body_name", "body_names"];

/// The arguments of each feature tool, which `set_feature` patches a
/// feature of its kind with.
const EXTRUDE_FIELDS: &[&str] = &[
    "sketch",
    "profiles",
    "extent",
    "distance",
    "second_distance",
    "flip",
    "taper",
    "operation",
    "exclude",
];
const REVOLVE_FIELDS: &[&str] = &[
    "sketch",
    "profiles",
    "axis",
    "extent",
    "angle",
    "second_angle",
    "flip",
    "operation",
    "exclude",
];
const COMBINE_FIELDS: &[&str] = &["target", "tools", "operation", "keep_tools"];
const MOVE_BODY_FIELDS: &[&str] = &["bodies", "x", "y", "z", "angle", "axis"];
const MIRROR_FIELDS: &[&str] = &["bodies", "plane", "keep_original"];
const PATTERN_FIELDS: &[&str] = &[
    "kind", "bodies", "axis", "count", "mode", "spread", "flip", "join",
];
const ALIGN_FIELDS: &[&str] = &["body", "from", "to", "flip", "distance", "angle"];
const SCALE_FIELDS: &[&str] = &[
    "bodies",
    "point",
    "mode",
    "factor",
    "x",
    "y",
    "z",
    "edge",
    "length",
    "axis_only",
];
const SPLIT_FIELDS: &[&str] = &["body", "tool", "original", "keep"];
const SHELL_FIELDS: &[&str] = &["body", "faces", "thickness", "direction"];
const OFFSET_FACE_FIELDS: &[&str] = &["faces", "distance", "inward", "tangent"];
const DRAFT_FIELDS: &[&str] = &["faces", "neutral", "angle", "flip", "tangent"];
const CHAMFER_FIELDS: &[&str] = &[
    "edges",
    "faces",
    "type",
    "distance",
    "second_distance",
    "angle",
    "flip",
    "tangent",
];
const FILLET_FIELDS: &[&str] = &["edges", "faces", "radius", "tangent"];
const SWEEP_FIELDS: &[&str] = &[
    "sketch",
    "profiles",
    "path",
    "helix",
    "keep_orientation",
    "twist",
    "operation",
    "exclude",
];
const LOFT_FIELDS: &[&str] = &[
    "sections",
    "mode",
    "closed",
    "rails",
    "operation",
    "exclude",
];

/// A feature tool: one of the tools that add a feature, whose
/// arguments are the fields `feature` shows a feature of its kind by
/// and `set_feature` patches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeatureTool {
    Extrude,
    Revolve,
    Combine,
    MoveBody,
    Mirror,
    Pattern,
    Align,
    Scale,
    Split,
    Shell,
    OffsetFace,
    Draft,
    Chamfer,
    Fillet,
    Sweep,
    Loft,
}

impl FeatureTool {
    /// Every feature tool, the one place a tool is marked as one.
    pub const ALL: [FeatureTool; 16] = [
        FeatureTool::Extrude,
        FeatureTool::Revolve,
        FeatureTool::Combine,
        FeatureTool::MoveBody,
        FeatureTool::Mirror,
        FeatureTool::Pattern,
        FeatureTool::Align,
        FeatureTool::Scale,
        FeatureTool::Split,
        FeatureTool::Shell,
        FeatureTool::OffsetFace,
        FeatureTool::Draft,
        FeatureTool::Chamfer,
        FeatureTool::Fillet,
        FeatureTool::Sweep,
        FeatureTool::Loft,
    ];

    /// The feature tool named `tool`, if it is one.
    pub fn named(tool: &str) -> Option<FeatureTool> {
        FeatureTool::ALL
            .into_iter()
            .find(|feature| feature.name() == tool)
    }

    /// Its name, as listed.
    pub fn name(self) -> &'static str {
        self.spec().0
    }

    /// Its arguments, the fields `feature` shows a feature of its kind
    /// by and `set_feature` patches.
    pub fn fields(self) -> &'static [&'static str] {
        self.spec().1
    }

    /// The names of new bodies it takes.
    fn new_bodies(self) -> NewBodies {
        self.spec().2
    }

    /// Its name, fields and names of new bodies.
    fn spec(self) -> (&'static str, &'static [&'static str], NewBodies) {
        match self {
            FeatureTool::Extrude => ("extrude", EXTRUDE_FIELDS, NewBodies::One),
            FeatureTool::Revolve => ("revolve", REVOLVE_FIELDS, NewBodies::One),
            FeatureTool::Combine => ("combine", COMBINE_FIELDS, NewBodies::None),
            FeatureTool::MoveBody => ("move_body", MOVE_BODY_FIELDS, NewBodies::None),
            FeatureTool::Mirror => ("mirror", MIRROR_FIELDS, NewBodies::None),
            FeatureTool::Pattern => ("pattern", PATTERN_FIELDS, NewBodies::Many),
            FeatureTool::Align => ("align", ALIGN_FIELDS, NewBodies::None),
            FeatureTool::Scale => ("scale", SCALE_FIELDS, NewBodies::None),
            FeatureTool::Split => ("split", SPLIT_FIELDS, NewBodies::One),
            FeatureTool::Shell => ("shell", SHELL_FIELDS, NewBodies::None),
            FeatureTool::OffsetFace => ("offset_face", OFFSET_FACE_FIELDS, NewBodies::None),
            FeatureTool::Draft => ("draft", DRAFT_FIELDS, NewBodies::None),
            FeatureTool::Chamfer => ("chamfer", CHAMFER_FIELDS, NewBodies::None),
            FeatureTool::Fillet => ("fillet", FILLET_FIELDS, NewBodies::None),
            FeatureTool::Sweep => ("sweep", SWEEP_FIELDS, NewBodies::One),
            FeatureTool::Loft => ("loft", LOFT_FIELDS, NewBodies::One),
        }
    }
}

/// Why `patch`, a `set_feature` patch, is refused if it names what a
/// feature makes (`name`, `body_name`, `body_names`): renaming is
/// `rename`'s.
pub fn renaming(patch: &Map<String, Value>) -> Option<String> {
    let key = (RENAMES.iter().chain(&["name"])).find(|key| patch.contains_key(**key))?;
    Some(format!(
        "{key} isn't a feature's setting: rename renames features and bodies"
    ))
}

/// Reads `patch`, a `set_feature` patch of a feature made by `tool`, as
/// that tool's arguments all optional: those the tool requires and the
/// patch doesn't give are filled in with placeholders for its parser,
/// which the app never takes (it takes only the fields the patch gives).
pub fn parse_patch(tool: FeatureTool, patch: &Map<String, Value>) -> Result<Call, CallError> {
    if let Some(why) = renaming(patch) {
        return Err(CallError::Arguments {
            at: "set_feature".to_owned(),
            message: why,
        });
    }
    let fields = tool.fields();
    let needs = |key: &str| fields.contains(&key) && !patch.contains_key(key);
    let mut filled = patch.clone();
    let required = [
        ("sketch", json!("-")),
        ("profiles", json!("all")),
        ("axis", json!("x")),
        ("target", json!("-")),
        ("tools", json!(["-"])),
        ("bodies", json!(["-"])),
        ("plane", json!("xy")),
        ("from", json!({ "point": "origin" })),
        ("to", json!({ "point": "origin" })),
        ("body", json!("-")),
        ("tool", json!("xy")),
        ("sections", json!([{ "sketch": "-", "point": 1 }])),
    ];
    for (key, value) in required {
        if needs(key) {
            filled.insert(key.to_owned(), value);
        }
    }
    // One of two: a face feature's faces, a blend's edges or faces, a
    // sweep's path or helix.
    match tool {
        FeatureTool::OffsetFace | FeatureTool::Draft if needs("faces") => {
            filled.insert("faces".to_owned(), json!(["-"]));
        }
        FeatureTool::Chamfer | FeatureTool::Fillet if needs("edges") && needs("faces") => {
            filled.insert("edges".to_owned(), json!(["-"]));
        }
        FeatureTool::Sweep if needs("path") && needs("helix") => {
            filled.insert("path".to_owned(), json!({ "edges": ["-"] }));
        }
        _ => {}
    }
    parse_arguments(tool.name(), &filled)
}

#[cfg(test)]
mod tests;
