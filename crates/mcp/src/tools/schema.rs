//! The `tools/list` result: each tool's input and output JSON schemas,
//! and the schema pieces they share.

use super::*;

/// The `tools/list` result: the tools built so far, built once.
pub fn list() -> &'static Value {
    static LIST: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    LIST.get_or_init(build_list)
}

/// Builds [`list`]'s catalogue.
fn build_list() -> Value {
    let parameter = json!({ "type": "string", "description": "The parameter's name" });
    let expression = json!({
        "type": "string",
        "maxLength": TEXT_BYTES,
        "description": "An expression as typed in the Parameters popup: \"20 mm\", \"width / 2\", \
                        \"90 deg\"; bare numbers in the design's units"
    });
    let direction = json!({
        "description": "An axis such as \"+z\", \"-x\" or \"y\" (either way along it), \
                        or three numbers [x, y, z]",
        "oneOf": [
            { "type": "string" },
            { "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3 }
        ]
    });
    // The queries and the design's edits.
    let design = json!([
        {
            "name": "design_summary",
            "title": "Design summary",
            "description": "The open design at a glance: its units and tolerance, its \
                bodies (name, shown or hidden), the Timeline's features in order (kind, \
                name, id, and why one fails), the rollback marker, the parameters and \
                the revision. Call it first, and again when a result says the user \
                changed the design. Each listing is paged alone by offset and limit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "offset": offset("How many to skip in each listing"),
                    "limit": limit("How many to list at most in each")
                },
                "additionalProperties": false
            },
            "outputSchema": design_summary_output(),
            "annotations": read_only(),
        },
        {
            "name": "feature",
            "title": "Feature settings",
            "description": "One feature's settings, by its name in the Timeline: values \
                as typed text in the design's units, and what it refers to (sketches, \
                bodies, faces, edges) by name or key. Each list in them (edges, faces, \
                bodies, profiles, sections, rails, ...) is paged alone by offset and \
                limit: a list given whole is a plain array, one that isn't is a page \
                {items, total, left_out} with a note. A page is part of the list: \
                never send it back to set_feature as the list (it's refused); leave \
                the field out to keep the list, or send the whole list you want.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": feature_name(),
                    "offset": offset("How many to skip in each list"),
                    "limit": limit("How many to give at most of each")
                },
                "required": ["name"],
                "additionalProperties": false
            },
            "annotations": read_only(),
        },
        {
            "name": "body_topology",
            "title": "Body faces, edges and corners",
            "description": "A body's faces, edges and corners, each with a key to refer \
                to it by in other tools (to chamfer an edge, sketch on a face). Faces \
                carry their form, normal if flat, area and bounds; edges their form, \
                ends or axis, length, two faces and whether convex; corners their \
                point. Measures are approximate, from the display mesh. Filter to keep \
                it short, page with offset and limit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "body": body(),
                    "faces": {
                        "type": "object",
                        "description": "List only the flat faces facing this way",
                        "properties": { "normal": direction },
                        "additionalProperties": false
                    },
                    "edges": {
                        "type": "object",
                        "description": "List only these edges",
                        "properties": {
                            "direction": {
                                "description": "List only the straight edges along this axis, \
                                                either way: an edge's sense means nothing, so \
                                                \"+z\", \"-z\" and \"z\" are the same here",
                                "oneOf": direction["oneOf"].clone()
                            },
                            "convex": { "type": "boolean", "description": "Only convex (outside) edges, or only concave (inside) ones" }
                        },
                        "additionalProperties": false
                    },
                    "offset": offset("How many to skip in each listing"),
                    "limit": limit("How many to list at most in each")
                },
                "required": ["body"],
                "additionalProperties": false
            },
            "outputSchema": body_topology_output(),
            "annotations": read_only(),
        },
        {
            "name": "sketch",
            "title": "Sketch geometry and regions",
            "description": "A sketch's points and curves with their ids, its constraints \
                and dimensions (values as typed, in the design's units), its plane, its \
                regions and its degrees of freedom (0: fully constrained). Each region \
                has the index extrude, revolve, split and sweep take in profiles, its \
                area, a point inside it and its outer curves and holes by id. Any sketch, \
                open or not. Points are bare numbers in the design's units. Each listing \
                is paged alone by offset and limit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "The sketch's name, as the Timeline shows it" },
                    "offset": offset("How many to skip in each listing"),
                    "limit": limit("How many to list at most in each")
                },
                "required": ["name"],
                "additionalProperties": false
            },
            "outputSchema": sketch_output(),
            "annotations": read_only(),
        },
        {
            "name": "measure",
            "title": "Measure",
            "description": "What the measure tool shows, on the model as it is: of one \
                item, a face's area (with its radius if round, width and height if a \
                rectangle, normal if flat), an edge's length (radius and centre if \
                round), a corner's point, a body's volume, area, centre and box; of \
                two, the minimum distance between them with its X, Y and Z parts, and \
                the angle between their directions where both have one. Exact, from \
                the solid; values as typed text in the design's units. Says which \
                generation of the model it measured.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "a": measured_schema("What to measure"),
                    "b": measured_schema("A second item, to measure the distance and angle between the two")
                },
                "required": ["a"],
                "additionalProperties": false
            },
            "annotations": read_only(),
        },
        {
            "name": "render",
            "title": "Render",
            "description": format!("A picture of the model as a PNG image, to check by \
                eye: the bodies asked for (every visible one by default) on nothing, \
                framed and cropped to them, the others and the sketches not drawn, in \
                the app's theme. from is a named view (\"iso\", the default, as Home \
                looks; \"front\", \"back\", \"left\", \"right\", \"top\", \
                \"bottom\") or yaw and pitch: the direction looked from, yaw about Z \
                from +X (front is -90 deg, right 0), pitch up from the XY plane \
                (iso is yaw -60 deg, pitch 30 deg). highlight draws faces and edges \
                selected, to check keys before an edit. size is the long side in \
                pixels, {MIN_RENDER_SIZE} to {MAX_RENDER_SIZE}, default {RENDER_SIZE}; \
                the PNG is at most {} KiB. Waits for the app's next frame: it fails while \
                the window is minimized or there's no GPU.", ToolReply::IMAGE_BYTES / 1024),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "bodies": {
                        "type": "array",
                        "items": body(),
                        "maxItems": MAX_ITEMS,
                        "description": "The bodies to draw: every visible one if left out"
                    },
                    "from": {
                        "description": "A named view, or {yaw, pitch} as typed angles (\"30 deg\") or numbers of degrees",
                        "oneOf": [
                            { "type": "string", "enum": words(NAMED_VIEWS) },
                            {
                                "type": "object",
                                "properties": {
                                    "yaw": { "type": ["string", "number"] },
                                    "pitch": { "type": ["string", "number"] }
                                },
                                "required": ["yaw", "pitch"],
                                "additionalProperties": false
                            }
                        ]
                    },
                    "projection": { "type": "string", "enum": words(PROJECTIONS), "description": "Default orthographic" },
                    "zoom": { "type": "number", "minimum": MIN_ZOOM, "maximum": MAX_ZOOM, "description": "A factor on the framing, default 1; above 1 closes in on the bodies' centre, below 1 shows more around them, the camera staying outside them in either projection" },
                    "highlight": {
                        "type": "array",
                        "items": item(&[ItemKind::Face, ItemKind::Edge]),
                        "maxItems": MAX_ITEMS,
                        "description": "Faces and edges to draw selected, by their keys from body_topology"
                    },
                    "size": { "type": "integer", "minimum": MIN_RENDER_SIZE, "maximum": MAX_RENDER_SIZE }
                },
                "additionalProperties": false
            },
            "annotations": read_only(),
        },
        {
            "name": "rename",
            "title": "Rename a feature or body",
            "description": "Renames a feature (by its Timeline name) or a body (by its \
                Objects name): give feature or body, and to. A name another feature or \
                body has gets a number after it, and the result says the name it got. \
                Applied to the design as the user's edit, one undo step; the result \
                waits for regeneration and says what failed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "feature": feature_name(),
                    "body": body(),
                    "to": { "type": "string", "description": "The new name" }
                },
                "required": ["to"],
                "additionalProperties": false
            },
            "annotations": edit(),
        },
        {
            "name": "set_visible",
            "title": "Show or hide a body",
            "description": "Shows or hides a body, as its eye in Objects does. One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": { "body": body(), "visible": { "type": "boolean" } },
                "required": ["body", "visible"],
                "additionalProperties": false
            },
            "annotations": edit(),
        },
        {
            "name": "set_color",
            "title": "Colour a body",
            "description": "Gives a body a colour of its own, a hue and a saturation (the \
                lightness stays the theme's shading), or with color null the theme's back. \
                One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "body": body(),
                    "color": {
                        "description": "The colour, or null for the theme's",
                        "oneOf": [
                            {
                                "type": "object",
                                "properties": {
                                    "hue": { "type": "number", "minimum": 0, "exclusiveMaximum": 360, "description": "Degrees round the colour wheel: 0 red, 120 green, 240 blue" },
                                    "saturation": { "type": "number", "minimum": 0, "maximum": MAX_SATURATION, "description": "From grey (0) to the most a body's colour may be" }
                                },
                                "required": ["hue", "saturation"],
                                "additionalProperties": false
                            },
                            { "type": "null" }
                        ]
                    }
                },
                "required": ["body", "color"],
                "additionalProperties": false
            },
            "annotations": edit(),
        },
        {
            "name": "set_opacity",
            "title": "Set a body's opacity",
            "description": format!("Sets how opaque a body is drawn, from {MIN_OPACITY} to 1 \
                (opaque), in whole percent. To hide a body use set_visible. One undo step."),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "body": body(),
                    "opacity": { "type": "number", "minimum": MIN_OPACITY, "maximum": 1 }
                },
                "required": ["body", "opacity"],
                "additionalProperties": false
            },
            "annotations": edit(),
        },
        {
            "name": "remove_feature",
            "title": "Remove a feature",
            "description": "Removes a feature by its Timeline name, with the bodies it makes. \
                One that later features use is refused, naming them: remove those first \
                (last first), or change them. One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "feature": feature_name()
                },
                "required": ["feature"],
                "additionalProperties": false
            },
            "annotations": destructive(),
        },
        {
            "name": "remove_body",
            "title": "Remove a body",
            "description": "Removes a body with the feature that makes it (and so that \
                feature's other bodies, as a pattern's copies: the result lists them). One \
                that later features use is refused, naming them. One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": { "body": body() },
                "required": ["body"],
                "additionalProperties": false
            },
            "annotations": destructive(),
        },
        {
            "name": "add_parameter",
            "title": "Add a parameter",
            "description": "Adds a parameter after the others, for feature values and \
                sketch dimensions to use by name. Its name is a word (letters, digits, _; \
                not starting with a digit, not a unit) no other parameter has. One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": { "name": parameter, "value": expression },
                "required": ["name", "value"],
                "additionalProperties": false
            },
            "annotations": edit_once(),
        },
        {
            "name": "set_parameter",
            "title": "Set a parameter",
            "description": "Sets a parameter's expression; everything using it follows. \
                Refused if a value using it would be in error or a sketch wouldn't solve. \
                One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": { "name": parameter, "value": expression },
                "required": ["name", "value"],
                "additionalProperties": false
            },
            "annotations": edit(),
        },
        {
            "name": "rename_parameter",
            "title": "Rename a parameter",
            "description": "Renames a parameter, rewriting every use of it. One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": parameter,
                    "to": { "type": "string", "description": "The new name" }
                },
                "required": ["name", "to"],
                "additionalProperties": false
            },
            "annotations": edit_once(),
        },
        {
            "name": "remove_parameter",
            "title": "Remove a parameter",
            "description": "Removes a parameter. One in use is refused, naming what uses \
                it. One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": { "name": parameter },
                "required": ["name"],
                "additionalProperties": false
            },
            "annotations": destructive(),
        },
        {
            "name": "set_units",
            "title": "Set the design's units",
            "description": "Changes the design's units, which bare numbers are in. Nothing \
                changes size: expressions get the old units written in. One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": { "units": { "type": "string", "enum": words(UNITS) } },
                "required": ["units"],
                "additionalProperties": false
            },
            "annotations": edit(),
        },
        {
            "name": "set_tolerance",
            "title": "Set the design's tolerance",
            "description": "Changes how closely curved surfaces are fitted: a length from \
                0.01 µm to 0.1 mm, such as \"1 µm\" (the default) or \"0.01 mm\"; finer is \
                slower. One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": { "tolerance": { "type": "string", "maxLength": TEXT_BYTES } },
                "required": ["tolerance"],
                "additionalProperties": false
            },
            "annotations": edit(),
        },
        {
            "name": "set_rollback",
            "title": "Move the rollback marker",
            "description": "Rolls the Timeline back to before a feature, so it and those \
                after aren't built (and new features go there), or forward to the end with \
                before null. One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "before": {
                        "description": "The feature to roll back to before, by its Timeline name, or null for the end",
                        "type": ["string", "null"]
                    }
                },
                "required": ["before"],
                "additionalProperties": false
            },
            "annotations": edit(),
        },
    ]);
    let Value::Array(mut tools) = design else {
        unreachable!("a literal list")
    };
    // After the queries and the design's edits: the sketch tools, the
    // patch the feature tools share, the feature tools, and undo last.
    tools.extend([
        add_sketch_tool(),
        set_sketch_plane_tool(),
        add_link_tool(),
        draw::edit_sketch_tool(),
        set_feature_tool(),
        extrude_tool(),
        revolve_tool(),
        combine_tool(),
        move_body_tool(),
        mirror_tool(),
        pattern_tool(),
        align_tool(),
        scale_tool(),
        split_tool(),
        shell_tool(),
        offset_face_tool(),
        draft_tool(),
        chamfer_tool(),
        fillet_tool(),
        sweep_tool(),
        loft_tool(),
        undo_tool(),
    ]);
    json!({ "tools": tools })
}

/// A creating tool's schema, `tool` with the `name` of the feature it
/// adds and the `body_name` or `body_names` of the `bodies` it makes.
fn creating(mut tool: Value, bodies: NewBodies) -> Value {
    let properties = &mut tool["inputSchema"]["properties"];
    properties["name"] = json!({
        "type": "string",
        "maxLength": NAME_BYTES,
        "description": "The feature's name in the Timeline; a default like 'Extrude 2' if left out — give a descriptive one"
    });
    match bodies {
        NewBodies::None => {}
        NewBodies::One => {
            properties["body_name"] = json!({
                "type": "string",
                "maxLength": NAME_BYTES,
                "description": "The name of the body it makes, in Objects; a default like 'Body 3' if left out. Refused when it makes none"
            });
        }
        NewBodies::Many => {
            properties["body_names"] = json!({
                "type": "array",
                "items": { "type": "string", "maxLength": NAME_BYTES },
                "maxItems": MAX_ITEMS,
                "description": "Names for the bodies it makes (copies with join false), in the order the result lists them; those left out keep defaults like 'Body 3'. Refused when it makes fewer"
            });
        }
    }
    tool
}

/// The `extrude` tool's schema.
fn extrude_tool() -> Value {
    creating(
        json!({
            "name": "extrude",
            "title": "Extrude",
            "description": "Adds an extrude of regions of a sketch, as the Extrude panel's OK \
                does, at the rollback marker (the end unless rolled back). Values are typed \
                text in the design's units. Defaults are the panel's: one side, 10 mm (the \
                panel's own default follows the view, so give distance), no taper, a new \
                body. through_all goes with operation cut only. One undo step; the result \
                names the feature and its new bodies, and waits for regeneration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "sketch": sketch(),
                    "profiles": profiles(),
                    "extent": { "enum": words(EXTRUDE_EXTENTS), "description": "How far it goes: default one_side; symmetric's distance is the whole, split either side" },
                    "distance": typed("A length, \"20 mm\" or a parameter expression; default 10 mm. Two sides' first"),
                    "second_distance": typed("Two sides' second length; default the first's"),
                    "flip": { "type": "boolean", "description": "One side the other way along the sketch's normal, or two sides swapped; default false" },
                    "taper": typed("A taper angle, \"5 deg\"; default none"),
                    "operation": operation(),
                    "exclude": exclude(),
                },
                "required": ["sketch", "profiles"],
                "additionalProperties": false
            },
            "annotations": edit_once(),
        }),
        FeatureTool::Extrude.new_bodies(),
    )
}

/// The `revolve` tool's schema.
fn revolve_tool() -> Value {
    creating(
        json!({
            "name": "revolve",
            "title": "Revolve",
            "description": "Adds a revolve of regions of a sketch about an axis, as the \
                Revolve panel's OK does, at the rollback marker. Defaults are the panel's: a \
                full turn, 180 deg for one side or symmetric, 90 deg for two sides' second, a \
                new body. One undo step; the result names the feature and its new bodies, \
                and waits for regeneration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "sketch": sketch(),
                    "profiles": profiles(),
                    "axis": {
                        "description": "\"x\" or \"y\", the sketch's own axes; {\"line\": id}, a line of the sketch by id; or {\"edge\": key}, a straight edge in the sketch's plane by its body_topology key",
                        "oneOf": [
                            { "enum": ["x", "y"] },
                            { "type": "object", "properties": { "line": { "type": "integer", "minimum": 0 } }, "required": ["line"], "additionalProperties": false },
                            { "type": "object", "properties": { "edge": { "type": "string" } }, "required": ["edge"], "additionalProperties": false }
                        ]
                    },
                    "extent": { "enum": words(REVOLVE_EXTENTS), "description": "How far it turns: default full" },
                    "angle": typed("An angle, \"90 deg\": default 180 deg. Two sides' first"),
                    "second_angle": typed("Two sides' second angle: default 90 deg"),
                    "flip": { "type": "boolean", "description": "One side the other way round, or two sides swapped; default false" },
                    "operation": operation(),
                    "exclude": exclude(),
                },
                "required": ["sketch", "profiles", "axis"],
                "additionalProperties": false
            },
            "annotations": edit_once(),
        }),
        FeatureTool::Revolve.new_bodies(),
    )
}

/// The `combine` tool's schema.
fn combine_tool() -> Value {
    creating(
        json!({
            "name": "combine",
            "title": "Combine bodies",
            "description": "Adds a combine, as the Combine panel's OK does, at the rollback \
                marker: the target gets the union of it and the tools (default), the tools \
                cut from it (subtract), or only what it shares with every tool (intersect). \
                The tools are used up unless keep_tools. One undo step; the result waits \
                for regeneration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "target": { "type": "string", "description": "The body kept, by its Objects name" },
                    "tools": { "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": MAX_ITEMS, "description": "The bodies combined into the target, by name" },
                    "operation": { "enum": words(BODY_OPS), "description": "Default union" },
                    "keep_tools": { "type": "boolean", "description": "Keep the tools as bodies of their own; default false" }
                },
                "required": ["target", "tools"],
                "additionalProperties": false
            },
            "annotations": edit_once(),
        }),
        FeatureTool::Combine.new_bodies(),
    )
}

/// A sketch's plane's schema.
pub(super) fn sketch_plane() -> Value {
    plane(
        "\"xy\", \"xz\" or \"yz\", an origin plane; or {\"face\": key}, a flat face by its body_topology key",
    )
}

/// An origin plane or a face by key, described as `description`.
fn plane(description: &str) -> Value {
    json!({
        "description": description,
        "oneOf": [
            { "enum": words(WORLD_PLANES) },
            item(&[ItemKind::Face])
        ]
    })
}

/// The `add_sketch` tool's schema.
fn add_sketch_tool() -> Value {
    creating(
        json!({
            "name": "add_sketch",
            "title": "Add a sketch",
            "description": "Adds an empty sketch on an origin plane or a flat face, as New sketch \
                does, at the rollback marker (the end unless rolled back). It isn't opened for \
                the user. Draw in it with the sketch tools. One undo step; the result names the \
                sketch and its id, and waits for regeneration.",
            "inputSchema": {
                "type": "object",
                "properties": { "plane": sketch_plane() },
                "required": ["plane"],
                "additionalProperties": false
            },
            "annotations": edit_once(),
        }),
        NewBodies::None,
    )
}

/// The `set_sketch_plane` tool's schema.
fn set_sketch_plane_tool() -> Value {
    json!({
        "name": "set_sketch_plane",
        "title": "Change a sketch's plane",
        "description": "Puts a sketch on another origin plane or flat face, as Change plane \
            does, its drawing kept as it is in its own coordinates. A face must be made \
            before the sketch. One undo step; the result waits for regeneration.",
        "inputSchema": {
            "type": "object",
            "properties": { "sketch": sketch(), "plane": sketch_plane() },
            "required": ["sketch", "plane"],
            "additionalProperties": false
        },
        "annotations": edit(),
    })
}

/// The `add_link` tool's schema.
fn add_link_tool() -> Value {
    let id = |what: &str| json!({ "type": "integer", "minimum": 0, "description": what });
    json!({
        "name": "add_link",
        "title": "Link into a sketch",
        "description": "Adds a link to a sketch, as a click of its Project or Intersect tool \
            does: project takes a model edge, a face (its outline), a corner, or another \
            sketch's curve or point, onto the sketch's plane; intersect cuts a model face or \
            edge with the plane. Only what's made before the sketch. The linked geometry \
            follows its source as the design changes. One undo step, solved first; the \
            result names the link and the ids of the points and curves it made, as sketch \
            lists them.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "sketch": sketch(),
                "kind": { "enum": words(LINK_KINDS), "description": "default project" },
                "from": {
                    "description": "What it links: a model face, edge or corner by its \
                        body_topology key, or another sketch's curve or point by its id there",
                    "oneOf": [
                        item(&[ItemKind::Face, ItemKind::Edge, ItemKind::Corner]),
                        {
                            "type": "object",
                            "properties": { "sketch": sketch(), "curve": id("The curve's id in that sketch") },
                            "required": ["sketch", "curve"],
                            "additionalProperties": false
                        },
                        {
                            "type": "object",
                            "properties": { "sketch": sketch(), "point": id("The point's id in that sketch") },
                            "required": ["sketch", "point"],
                            "additionalProperties": false
                        }
                    ]
                }
            },
            "required": ["sketch", "from"],
            "additionalProperties": false
        },
        "annotations": edit_once(),
    })
}

/// The `set_feature` tool's schema.
fn set_feature_tool() -> Value {
    json!({
        "name": "set_feature",
        "title": "Change a feature",
        "description": "Changes a feature's settings, as its panel's OK on the edited \
            feature does: name is the feature, and the other arguments a patch in the \
            shape feature shows it, its creating tool's arguments (extrude's for an \
            extrude, ...), each given replacing the feature's, the rest kept. Read it \
            with feature first, change fields and send them back; a field sent as \
            feature shows it changes nothing. Fields its kind or mode doesn't have \
            are refused. A list feature gave as a page (part of it) is refused: \
            leave it out to keep the list, or send it whole. One undo step; the \
            result waits for regeneration.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "name": feature_name()
            },
            "required": ["name"],
            "additionalProperties": true
        },
        "annotations": edit_hints(true, true),
    })
}

/// The `move_body` tool's schema.
fn move_body_tool() -> Value {
    creating(
        json!({
            "name": "move_body",
            "title": "Move bodies",
            "description": "Adds a move, as the Move panel's OK does, at the rollback marker: \
                each body turned by angle about axis, then shifted by x, y and z along the \
                world axes. Values are typed text in the design's units. Defaults are the \
                panel's: 0 mm, 0 deg about the Z axis; one that moves nothing is refused. \
                One undo step; the result waits for regeneration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "bodies": bodies(),
                    "x": typed("The shift along world X, a length: default 0 mm"),
                    "y": typed("The shift along world Y: default 0 mm"),
                    "z": typed("The shift along world Z: default 0 mm"),
                    "angle": typed("The turn about axis, right-handed, \"90 deg\": default 0 deg"),
                    "axis": motion_axis("What it turns about: default \"z\""),
                },
                "required": ["bodies"],
                "additionalProperties": false
            },
            "annotations": edit_once(),
        }),
        FeatureTool::MoveBody.new_bodies(),
    )
}

/// The `mirror` tool's schema.
fn mirror_tool() -> Value {
    creating(
        json!({
            "name": "mirror",
            "title": "Mirror bodies",
            "description": "Adds a mirror, as the Mirror panel's OK does, at the rollback \
                marker: each body reflected in plane, keeping the original too (default) \
                or becoming its image. One undo step; the result waits for regeneration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "bodies": bodies(),
                    "plane": sketch_plane(),
                    "keep_original": { "type": "boolean", "description": "The panel's Create copy: keep each body and its image together; default true" }
                },
                "required": ["bodies", "plane"],
                "additionalProperties": false
            },
            "annotations": edit_once(),
        }),
        FeatureTool::Mirror.new_bodies(),
    )
}

/// The `pattern` tool's schema.
fn pattern_tool() -> Value {
    creating(
        json!({
            "name": "pattern",
            "title": "Pattern bodies",
            "description": "Adds a linear or circular pattern, as its panel's OK does, at the \
                rollback marker: count copies of each body, the original among them, along \
                or about axis. Defaults are the panel's: linear along X, 3, spaced 10 mm \
                (the panel's own spacing follows the view, so give spread); circular about Z, \
                4 evenly round a full turn. Copies join their body unless join is false, \
                when each is a body of its own. One undo step; the result waits for \
                regeneration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": { "enum": words(PATTERN_KINDS), "description": "Default linear" },
                    "bodies": bodies(),
                    "axis": motion_axis("The direction (linear: default \"x\") or the axis turned about (circular: default \"z\")"),
                    "count": {
                        "type": ["string", "integer"],
                        "maxLength": TEXT_BYTES,
                        "description": format!("How many, the original among them, 2 to {MAX_PATTERN_COUNT}: a whole number or an expression; default 3 (linear) or 4 (circular)")
                    },
                    "mode": { "enum": words(PATTERN_MODES), "description": "What spread is: spacing, one step (linear's default); total, first to last; full, a circular one's copies round a full turn with no spread (circular's default)" },
                    "spread": typed("The spacing or total: a positive length for linear (default 10 mm), an angle for circular (default 90 deg)"),
                    "flip": { "type": "boolean", "description": "A linear one's copies the other way along axis; default false" },
                    "join": { "type": "boolean", "description": "Join to original: the copies in their body (default true), else each copy a new body" }
                },
                "required": ["bodies"],
                "additionalProperties": false
            },
            "annotations": edit_once(),
        }),
        FeatureTool::Pattern.new_bodies(),
    )
}

/// The `align` tool's schema.
fn align_tool() -> Value {
    creating(
        json!({
            "name": "align",
            "title": "Align a body",
            "description": "Adds an align, as the Align panel's OK does, at the rollback \
                marker: the body moved so from's point meets to's, and with directions on \
                both sides, from's direction turned onto to's (a second pair fixes the turn \
                about it). Points are corners or edges (a straight one's middle, a round \
                one's centre; a round edge's centre with no direction also gives its side \
                the rim's axis) by body_topology key, or to's \"origin\"; directions are \
                faces or edges by key, or to's \"x\", \"y\" or \"z\". One undo step; the \
                result waits for regeneration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "body": { "type": "string", "description": "The body moved, by name: default the one from's point is on" },
                    "from": align_side("On the body moved"),
                    "to": align_side("What it's aligned to, on another body or the origin"),
                    "flip": { "type": "boolean", "description": "The directions meet the other way round (with directions only); default false" },
                    "distance": typed("An offset along to's direction (with directions only): default 0 mm"),
                    "angle": typed("A turn about to's direction (with directions only): default 0 deg")
                },
                "required": ["from", "to"],
                "additionalProperties": false
            },
            "annotations": edit_once(),
        }),
        FeatureTool::Align.new_bodies(),
    )
}

/// The `scale` tool's schema.
fn scale_tool() -> Value {
    creating(
        json!({
            "name": "scale",
            "title": "Scale bodies",
            "description": "Adds a scale, as the Scale panel's OK does, at the rollback \
                marker, about point: uniform by factor, per_axis by x, y and z factors, or \
                edge_length, the factor that gives edge (an edge of a scaled body) length. \
                Factors are from 0.001 to 1000; one scaling nothing is refused. One undo \
                step; the result waits for regeneration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "bodies": bodies(),
                    "point": {
                        "description": "What it scales about: \"origin\" (default), or {\"corner\": key} or {\"edge\": key} (a straight edge's middle, a round one's centre) by body_topology key",
                        "oneOf": [
                            { "const": "origin" },
                            item(&[ItemKind::Corner, ItemKind::Edge])
                        ]
                    },
                    "mode": { "enum": words(SCALE_MODES), "description": "Default uniform" },
                    "factor": typed("Uniform's factor: default 1"),
                    "x": typed("Per axis's factor along world X: default 1"),
                    "y": typed("Per axis's factor along world Y: default 1"),
                    "z": typed("Per axis's factor along world Z: default 1"),
                    "edge": { "type": "string", "description": "Edge length's edge, by its body_topology key" },
                    "length": typed("Edge length's length, \"50 mm\""),
                    "axis_only": { "type": "boolean", "description": "Edge length's Along its axis only, for a straight edge along a world axis: scale along that axis alone; default false" }
                },
                "required": ["bodies"],
                "additionalProperties": false
            },
            "annotations": edit_once(),
        }),
        FeatureTool::Scale.new_bodies(),
    )
}

/// The `split` tool's schema.
pub(super) fn split_tool() -> Value {
    creating(
        json!({
            "name": "split",
            "title": "Split a body",
            "description": "Adds a split, as the Split panel's OK does, at the rollback \
                marker: body cut in two by tool. The front is the part inside the tool (on \
                the side a plane's normal points to, out of a face's body, inside a tool \
                body or the sketch's regions, left of a line); the back the rest. With keep \
                both, the piece original names (default front) keeps the body's name and id \
                and the other becomes a new body; keeping one side trims the other away. \
                One undo step; the result waits for regeneration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "body": body(),
                    "tool": {
                        "description": "\"xy\", \"xz\" or \"yz\", an origin plane; {\"face\": key}, a face by its body_topology key (a flat one's plane, a curved one's surface continued); {\"body\": name}, another body, kept; {\"sketch\": name, \"profiles\": ...}, regions of a sketch made before, through the body both ways; or {\"sketch\": name, \"curves\": [id, ...]}, the curves of one open line of a sketch",
                        "oneOf": [
                            { "enum": words(WORLD_PLANES) },
                            item(&[ItemKind::Face]),
                            {
                                "type": "object",
                                "properties": { "body": { "type": "string" } },
                                "required": ["body"],
                                "additionalProperties": false
                            },
                            {
                                "type": "object",
                                "properties": { "sketch": sketch(), "profiles": profiles() },
                                "required": ["sketch", "profiles"],
                                "additionalProperties": false
                            },
                            {
                                "type": "object",
                                "properties": {
                                    "sketch": sketch(),
                                    "curves": {
                                        "type": "array",
                                        "items": { "type": "integer", "minimum": 0 },
                                        "minItems": 1,
                                        "maxItems": MAX_ITEMS
                                    }
                                },
                                "required": ["sketch", "curves"],
                                "additionalProperties": false
                            }
                        ]
                    },
                    "original": { "enum": words(SPLIT_SIDES), "description": "With keep both: the piece that keeps the body's name and id; default front" },
                    "keep": { "enum": words(SPLIT_KEEPS), "description": "The pieces kept: both (default), or front or back alone, trimming the other away" }
                },
                "required": ["body", "tool"],
                "additionalProperties": false
            },
            "annotations": edit_once(),
        }),
        FeatureTool::Split.new_bodies(),
    )
}

/// The `shell` tool's schema.
fn shell_tool() -> Value {
    creating(
        json!({
            "name": "shell",
            "title": "Shell a body",
            "description": "Adds a shell, as the Shell panel's OK does, at the rollback \
                marker: a body hollowed to walls thickness thick, faces opened (none, a \
                closed hollow body). Its body is its faces'; with no faces give body, or the \
                model's only body is taken. Defaults: 2 mm (the panel's own follows the \
                view), inward. One undo step; the result waits for regeneration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "body": { "type": "string", "description": "The body hollowed closed, by its Objects name: only with no faces" },
                    "faces": keys("The faces removed, opening the shell, all on one body", 0),
                    "thickness": typed("The walls' thickness, a positive length: default 2 mm"),
                    "direction": { "enum": words(SHELL_DIRECTIONS), "description": "inward (default), the walls inside the body's faces; or outward, outside them" }
                },
                "additionalProperties": false
            },
            "annotations": edit_once(),
        }),
        FeatureTool::Shell.new_bodies(),
    )
}

/// The `offset_face` tool's schema.
fn offset_face_tool() -> Value {
    creating(
        json!({
            "name": "offset_face",
            "title": "Offset faces",
            "description": "Adds an offset face, as the Offset face panel's OK does, at the \
                rollback marker: faces of one body moved along their normals by distance, \
                out of the body or inward into it. Defaults: 1 mm (the panel's own follows \
                the view), outward, tangent faces taken in. One undo step; the result waits \
                for regeneration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "faces": keys("The faces moved, all on one body", 1),
                    "distance": typed("How far, a positive length: default 1 mm"),
                    "inward": { "type": "boolean", "description": "Into the body, shrinking it; default false" },
                    "tangent": { "type": "boolean", "description": "Take in faces that run on smoothly; default true" }
                },
                "required": ["faces"],
                "additionalProperties": false
            },
            "annotations": edit_once(),
        }),
        FeatureTool::OffsetFace.new_bodies(),
    )
}

/// The `draft` tool's schema.
fn draft_tool() -> Value {
    creating(
        json!({
            "name": "draft",
            "title": "Draft faces",
            "description": "Adds a draft, as the Draft panel's OK does, at the rollback \
                marker: faces of one body turned by angle about where they meet the neutral \
                plane, so the part narrows along the pull (the plane's normal, reversed by \
                flip). Defaults: XY, 3 deg, tangent faces taken in. One undo step; the result \
                waits for regeneration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "faces": keys("The faces drafted, all on one body", 1),
                    "neutral": plane("The neutral plane: \"xy\" (default), \"xz\" or \"yz\", an origin plane; or {\"face\": key}, a flat face not drafted, by its body_topology key"),
                    "angle": typed("The draft angle, above 0 and under 90 deg: default 3 deg"),
                    "flip": { "type": "boolean", "description": "Pull against the neutral plane's normal; default false" },
                    "tangent": { "type": "boolean", "description": "Take in faces that run on smoothly; default true" }
                },
                "required": ["faces"],
                "additionalProperties": false
            },
            "annotations": edit_once(),
        }),
        FeatureTool::Draft.new_bodies(),
    )
}

/// The `chamfer` tool's schema.
fn chamfer_tool() -> Value {
    creating(
        json!({
            "name": "chamfer",
            "title": "Chamfer edges",
            "description": "Adds a chamfer, as the Chamfer panel's OK does, at the rollback \
                marker: edges of one body cut off flat, a face standing for the edges round it \
                (an edge named round a face named is left out). type equal cuts distance along \
                both faces; two_distances distance along the first face and second_distance \
                along the other; distance_angle distance along the first face at angle to it. \
                An edge's first face is its key's first, the other with flip (not for equal). \
                Defaults: equal, 1 mm, second 2 mm, 45 deg (the panel's distances follow the \
                view), tangent chains taken in. The kernel doesn't chamfer yet: it applies and \
                fails in regeneration, as for the user. One undo step; the result waits for \
                regeneration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "edges": keys("The edges cut off, all on one body", 0),
                    "faces": keys("Faces whose edges are cut off, on the edges' body", 0),
                    "type": { "enum": words(CHAMFER_TYPES), "description": "How it's sized: default equal" },
                    "distance": typed("A positive length: default 1 mm"),
                    "second_distance": typed("With two_distances, the length along the other face: default 2 mm"),
                    "angle": typed("With distance_angle, the cut's angle to the first face, above 0 and under 90 deg: default 45 deg"),
                    "flip": { "type": "boolean", "description": "With two_distances or distance_angle: the first face is the other; default false" },
                    "tangent": tangent()
                },
                "additionalProperties": false
            },
            "annotations": edit_once(),
        }),
        FeatureTool::Chamfer.new_bodies(),
    )
}

/// The `fillet` tool's schema.
fn fillet_tool() -> Value {
    creating(
        json!({
            "name": "fillet",
            "title": "Fillet edges",
            "description": "Adds a fillet, as the Fillet panel's OK does, at the rollback \
                marker: edges of one body rounded to radius, a face standing for the edges round \
                it (an edge named round a face named is left out). Defaults: 2 mm (the panel's \
                follows the view), tangent chains taken in. The kernel doesn't fillet yet: it \
                applies and fails in regeneration, as for the user. One undo step; the result \
                waits for regeneration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "edges": keys("The edges rounded, all on one body", 0),
                    "faces": keys("Faces whose edges are rounded, on the edges' body", 0),
                    "radius": typed("A positive length: default 2 mm"),
                    "tangent": tangent()
                },
                "additionalProperties": false
            },
            "annotations": edit_once(),
        }),
        FeatureTool::Fillet.new_bodies(),
    )
}

/// The `sweep` tool's schema.
fn sweep_tool() -> Value {
    creating(
        json!({
            "name": "sweep",
            "title": "Sweep",
            "description": "Adds a sweep of regions of a sketch, as the Sweep panel's OK does, \
                at the rollback marker: along a path (chains of curves of sketches made before \
                but the profile's, each named by one of its curves, and model edges of one body) \
                or a helix about an axis. keep_orientation and twist go with a path only. \
                Defaults: follow the path, no twist, a helix's pitch 10 mm (the panel's follows \
                the view) and 5 turns, a new body. One undo step; the result waits for \
                regeneration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "sketch": sketch(),
                    "profiles": profiles(),
                    "path": {
                        "type": "object",
                        "description": "Along a path: give this or helix",
                        "properties": {
                            "curves": curves_in("Chains of sketch curves, each the chain the curve named is in"),
                            "edges": keys("Model edges, all on one body", 0),
                            "tangent": tangent()
                        },
                        "additionalProperties": false
                    },
                    "helix": {
                        "type": "object",
                        "description": "Along a helix: give this or path",
                        "properties": {
                            "axis": motion_axis("The helix's axis: \"x\", \"y\" or \"z\", or {\"edge\": key} (straight or round) or {\"face\": key} (round)"),
                            "pitch": typed("The rise per turn, a positive length: default 10 mm"),
                            "turns": { "type": ["string", "number"], "maxLength": TEXT_BYTES, "description": "How many turns, part of one too, typed text or a number: default 5" },
                            "left_handed": { "type": "boolean", "description": "Turning the other way; default false" },
                            "flip": { "type": "boolean", "description": "Rising the other way along the axis; default false" }
                        },
                        "required": ["axis"],
                        "additionalProperties": false
                    },
                    "keep_orientation": { "type": "boolean", "description": "A path's profile keeps its orientation rather than turning with the path; default false" },
                    "twist": typed("A path's twist from end to end, an angle: default 0 deg"),
                    "operation": operation(),
                    "exclude": exclude()
                },
                "required": ["sketch", "profiles"],
                "additionalProperties": false
            },
            "annotations": edit_once(),
        }),
        FeatureTool::Sweep.new_bodies(),
    )
}

/// The `loft` tool's schema.
fn loft_tool() -> Value {
    creating(
        json!({
            "name": "loft",
            "title": "Loft",
            "description": "Adds a loft, as the Loft panel's OK does, at the rollback marker: a \
                solid through its sections in order, each a region without holes of a sketch \
                made before, or first or last a sketch point on its own. A region's start is a \
                sketch point at one of its corners (default the corner nearest the previous \
                section's start). Rails, open chains of sketch curves each named by one of its \
                curves, guide it, not with closed. Defaults: smooth, open, a new body. One undo \
                step; the result waits for regeneration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "sections": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": MAX_ITEMS,
                        "description": "The sections in order, at least two for a loft",
                        "items": {
                            "type": "object",
                            "properties": {
                                "sketch": sketch(),
                                "region": {
                                    "description": "A region of the sketch: its index (as sketch lists them) or a point [x, y] inside it",
                                    "oneOf": region()
                                },
                                "point": { "type": "integer", "minimum": 0, "description": "A point of the sketch on its own, by id: the first or last section only" },
                                "start": { "type": "integer", "minimum": 0, "description": "A region's start: a point of the sketch at one of its corners, by id" }
                            },
                            "required": ["sketch"],
                            "additionalProperties": false
                        }
                    },
                    "mode": { "enum": words(LOFT_MODES), "description": "Between sections: smooth (default) or ruled; two sections are ruled either way" },
                    "closed": { "type": "boolean", "description": "The last section lofted back to the first; no rails then; default false" },
                    "rails": curves_in("Rails guiding it from the first section to the last, each the open chain the curve named is in"),
                    "operation": operation(),
                    "exclude": exclude()
                },
                "required": ["sections"],
                "additionalProperties": false
            },
            "annotations": edit_once(),
        }),
        FeatureTool::Loft.new_bodies(),
    )
}

/// `undo`'s schema: the session's own newest edits taken back.
fn undo_tool() -> Value {
    json!({
        "name": "undo",
        "title": "Undo your own edits",
        "description": "Takes back your own newest edits, as the user's Undo would: steps \
            steps of the design's history (default 1), each one of your edits in this \
            session. Refused, nothing undone, if any of them isn't yours (the user edited, \
            undid or redid since, or reverted) or the history doesn't reach that far: \
            then edit again instead. There's no redo: edit again. The result lists what \
            was undone, newest first, and waits for regeneration.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "steps": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_UNDO_STEPS,
                    "description": "How many of your newest edits to take back; default 1"
                }
            },
            "additionalProperties": false
        },
        "annotations": destructive(),
    })
}

/// A query's `offset` argument, described as `description`.
fn offset(description: &str) -> Value {
    json!({ "type": "integer", "minimum": 0, "description": description })
}

/// A query's `limit` argument, described as `description`.
fn limit(description: &str) -> Value {
    json!({ "type": "integer", "minimum": 1, "maximum": MAX_LIMIT, "description": description })
}

/// A read-only tool's annotations.
fn read_only() -> Value {
    json!({ "readOnlyHint": true, "openWorldHint": false })
}

/// An edit tool's annotations, asked twice the second changing nothing.
fn edit() -> Value {
    edit_hints(false, true)
}

/// An edit tool's annotations, asked twice the second failing or adding another.
pub(super) fn edit_once() -> Value {
    edit_hints(false, false)
}

/// The annotations of an edit tool that takes away what was there: a
/// removal, undo.
fn destructive() -> Value {
    edit_hints(true, false)
}

/// An edit tool's annotations, by whether it's `destructive` and
/// `idempotent`.
fn edit_hints(destructive: bool, idempotent: bool) -> Value {
    json!({
        "readOnlyHint": false,
        "destructiveHint": destructive,
        "idempotentHint": idempotent,
        "openWorldHint": false
    })
}

/// A feature's schema, by name.
fn feature_name() -> Value {
    json!({ "type": "string", "description": "The feature's name, as the Timeline shows it" })
}

/// A body's schema, by name.
fn body() -> Value {
    json!({ "type": "string", "description": "The body's name, as Objects shows it" })
}

/// A sketch feature's schema, by name.
pub(super) fn sketch() -> Value {
    json!({ "type": "string", "description": "The sketch, by its Timeline name" })
}

/// A feature's profiles' schema: a sketch's regions.
pub(super) fn profiles() -> Value {
    json!({
        "description": "The sketch's regions: \"all\", or a list of region indices (in the \
                        order the sketch's regions are found, as sketch lists them) and points \
                        [x, y] inside a region, in the sketch's own coordinates as lengths \
                        (\"5 mm\", or bare numbers in the design's units)",
        "oneOf": [
            { "const": "all" },
            {
                "type": "array",
                "minItems": 1,
                "maxItems": MAX_ITEMS,
                "items": { "oneOf": region() }
            }
        ]
    })
}

/// A sketch's region's ways: its index, or a point [x, y] inside it.
fn region() -> Value {
    json!([
        { "type": "integer", "minimum": 0 },
        { "type": "array", "items": { "type": ["string", "number"] }, "minItems": 2, "maxItems": 2 }
    ])
}

/// A feature's operation's schema.
fn operation() -> Value {
    json!({
        "enum": words(OPERATIONS),
        "description": "What it does with its solid: default new_body"
    })
}

/// The schema of the bodies a join, cut or intersect leaves out.
fn exclude() -> Value {
    json!({
        "type": "array",
        "items": { "type": "string" },
        "maxItems": MAX_ITEMS,
        "description": "Bodies a join, cut or intersect leaves alone, by name; it takes every \
                        other body it touches"
    })
}

/// A motion's bodies' schema.
pub(super) fn bodies() -> Value {
    json!({
        "type": "array",
        "items": { "type": "string" },
        "minItems": 1,
        "maxItems": MAX_ITEMS,
        "description": "The bodies, by their Objects names"
    })
}

/// A blend's or face feature's Tangent chain schema.
fn tangent() -> Value {
    json!({ "type": "boolean", "description": "Tangent chain: each edge with the edges running on smoothly from it; default true" })
}

/// A feature's value's schema, as typed text.
fn typed(description: &str) -> Value {
    json!({ "type": "string", "maxLength": TEXT_BYTES, "description": description })
}

/// The schema of a list of chains of sketch curves.
pub(super) fn curves_in(description: &str) -> Value {
    json!({
        "type": "array",
        "maxItems": MAX_ITEMS,
        "items": {
            "type": "object",
            "properties": {
                "sketch": sketch(),
                "curve": { "type": "integer", "minimum": 0, "description": "A curve of the chain, by its id in the sketch" }
            },
            "required": ["sketch", "curve"],
            "additionalProperties": false
        },
        "description": description
    })
}

/// A list of faces or edges by their body_topology keys, at least `least`.
pub(super) fn keys(description: &str, least: usize) -> Value {
    json!({
        "type": "array",
        "items": { "type": "string" },
        "minItems": least,
        "maxItems": MAX_ITEMS,
        "description": format!("{description}, by their body_topology keys")
    })
}

/// `measure`'s `a` or `b` schema: a body by name, or a face, edge or
/// corner by key.
fn measured_schema(description: &str) -> Value {
    let body = json!({
        "type": "object",
        "properties": { "body": body() },
        "required": ["body"],
        "additionalProperties": false
    });
    let mut one_of = vec![body];
    one_of.extend(item_variants(&[
        ItemKind::Face,
        ItemKind::Edge,
        ItemKind::Corner,
    ]));
    json!({
        "description": format!("{description}: {{\"face\": key}}, {{\"edge\": key}} or \
                                {{\"corner\": key}} by body_topology key, or {{\"body\": name}}"),
        "oneOf": one_of
    })
}

/// A schema for a model item by key, of one of `kinds`.
fn item(kinds: &[ItemKind]) -> Value {
    match item_variants(kinds).as_slice() {
        [one] => one.clone(),
        variants => json!({ "oneOf": variants }),
    }
}

/// The schema of a model item by key of each of `kinds`, in turn.
fn item_variants(kinds: &[ItemKind]) -> Vec<Value> {
    kinds
        .iter()
        .map(|kind| {
            json!({
                "type": "object",
                "properties": { kind.word(): { "type": "string" } },
                "required": [kind.word()],
                "additionalProperties": false
            })
        })
        .collect()
}

/// A move's or pattern's axis's schema, `description` saying what it's
/// for.
fn motion_axis(description: &str) -> Value {
    json!({
        "description": format!("{description}. \"x\", \"y\" or \"z\", a world axis; or \
                                {{\"edge\": key}}, a straight or round edge, or {{\"face\": key}}, \
                                a round face, by body_topology key"),
        "oneOf": [{ "enum": words(WORLD_AXES) }, item(AXIS_ITEMS)]
    })
}

/// An align's side's schema.
pub(super) fn align_side(description: &str) -> Value {
    let direction = |what: &str| {
        json!({
            "description": format!("{what}: a face (a flat one's normal, a round one's axis) or an \
                                    edge (a straight one's direction, a round one's axis) by \
                                    body_topology key; on to also \"x\", \"y\" or \"z\""),
            "oneOf": [{ "enum": words(WORLD_AXES) }, item(AXIS_ITEMS)]
        })
    };
    json!({
        "type": "object",
        "description": description,
        "properties": {
            "point": {
                "description": "A corner, or an edge (a straight one's middle, a round one's centre), by body_topology key; on to also \"origin\"",
                "oneOf": [{ "const": "origin" }, item(POINT_ITEMS)]
            },
            "direction": direction("The direction"),
            "second_direction": direction("A second direction, fixing the turn about the first")
        },
        "required": ["point"],
        "additionalProperties": false
    })
}

fn design_summary_output() -> Value {
    json!({
        "type": "object",
        "properties": {
            "units": { "type": "string" },
            "tolerance": { "type": "string" },
            "revision": { "type": "integer" },
            "pending_edit": { "type": "string" },
            "user_changes": { "type": "object" },
            "bodies": listing(json!({ "type": "object", "properties": {
                "name": { "type": "string" },
                "visible": { "type": "boolean" }
            }})),
            "timeline": listing(json!({ "type": "object", "properties": {
                "kind": { "type": "string" },
                "name": { "type": "string" },
                "id": { "type": "integer" },
                "failure": { "type": "string" }
            }})),
            "rollback": { "type": ["string", "null"] },
            "parameters": listing(json!({ "type": "object", "properties": {
                "name": { "type": "string" },
                "value": { "type": "string" }
            }}))
        }
    })
}

/// A paged listing's schema, its items `item`.
fn listing(item: Value) -> Value {
    json!({
        "type": "object",
        "properties": {
            "items": { "type": "array", "items": item },
            "total": { "type": "integer" },
            "left_out": { "type": "integer" }
        }
    })
}

fn sketch_output() -> Value {
    let ids = json!({ "type": "array", "items": { "type": "integer" } });
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "id": { "type": "integer" },
            "units": { "type": "string" },
            "plane": {},
            "revision": { "type": "integer" },
            "degrees_of_freedom": { "type": "integer" },
            "pending_edit": { "type": "string" },
            "user_changes": { "type": "object" },
            "points": listing(json!({ "type": "object", "properties": {
                "id": { "type": "integer" },
                "at": { "type": "array", "items": { "type": "number" } }
            }})),
            "curves": listing(json!({ "type": "object", "properties": {
                "id": { "type": "integer" },
                "kind": { "type": "string" },
                "construction": { "type": "boolean" }
            }})),
            "constraints": listing(json!({ "type": "object", "properties": {
                "id": { "type": "integer" },
                "kind": { "type": "string" }
            }})),
            "dimensions": listing(json!({ "type": "object", "properties": {
                "id": { "type": "integer" },
                "measure": { "type": "string" },
                "value": { "type": "string" },
                "driving": { "type": "boolean" }
            }})),
            "regions": listing(json!({ "type": "object", "properties": {
                "index": { "type": "integer" },
                "area": { "type": "string" },
                "inside": { "type": "array", "items": { "type": "number" } },
                "outer": ids,
                "outer_left_out": { "type": "integer" },
                "holes": { "type": "array", "items": ids },
                "holes_left_out": { "type": "integer" },
                "hole_curves_left_out": { "type": "integer" }
            }}))
        }
    })
}

fn body_topology_output() -> Value {
    json!({
        "type": "object",
        "properties": {
            "body": { "type": "string" },
            "revision": { "type": "integer" },
            "pending_edit": { "type": "string" },
            "user_changes": { "type": "object" },
            "faces": listing(json!({ "type": "object", "properties": {
                "key": { "type": "string" },
                "form": { "type": "string" }
            }})),
            "edges": listing(json!({ "type": "object", "properties": {
                "key": { "type": "string" },
                "form": { "type": "string" },
                "convex": { "type": "boolean" }
            }})),
            "corners": listing(json!({ "type": "object", "properties": {
                "key": { "type": "string" }
            }}))
        }
    })
}
