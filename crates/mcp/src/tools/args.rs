//! Reading a call's arguments: [`Arguments`] and its readers, and the
//! pieces of a call read alike by several tools.

use super::*;

/// What a move's or pattern's axis, or an align's direction, may be by
/// key.
pub(super) const AXIS_ITEMS: &[ItemKind] = &[ItemKind::Edge, ItemKind::Face];

/// What an align's or scale's point may be by key.
pub(super) const POINT_ITEMS: &[ItemKind] = &[ItemKind::Corner, ItemKind::Edge];

pub(super) fn topology(args: &mut Arguments) -> Result<Topology, CallError> {
    let body = args.name("body")?;
    let faces = match args.object("faces")? {
        None => None,
        Some(faces) => {
            faces.only(&["normal"])?;
            Some(FaceFilter {
                normal: faces.direction("normal")?,
            })
        }
    };
    let edges = match args.object("edges")? {
        None => None,
        Some(edges) => {
            edges.only(&["direction", "convex"])?;
            Some(EdgeFilter {
                direction: edges.direction("direction")?,
                convex: edges.boolean("convex")?,
            })
        }
    };
    let Page { offset, limit } = page(args, LIMIT)?;
    Ok(Topology {
        body,
        faces,
        edges,
        offset,
        limit,
    })
}

/// `render`'s arguments, all optional.
pub(super) fn render(args: &Arguments) -> Result<Render, CallError> {
    let from = match args.map.get("from") {
        None | Some(Value::Null) => RenderFrom::View(NamedView::Iso),
        Some(Value::String(word)) => lookup(NAMED_VIEWS, word)
            .map(RenderFrom::View)
            .ok_or_else(|| args.error(RENDER_FROM))?,
        Some(Value::Object(map)) => {
            let from = args.nested(map);
            from.only(&["yaw", "pitch"])?;
            let angle = |key: &str| {
                (from.map.get(key))
                    .and_then(coordinate)
                    .ok_or_else(|| args.error(RENDER_FROM))
            };
            RenderFrom::Angles {
                yaw: angle("yaw")?,
                pitch: angle("pitch")?,
            }
        }
        Some(_) => return Err(args.error(RENDER_FROM)),
    };
    let perspective = args.choice("projection", PROJECTIONS)?;
    let zoom = match args.map.get("zoom") {
        None | Some(Value::Null) => 1.0,
        Some(_) => args.number_in(
            "zoom",
            MIN_ZOOM..=MAX_ZOOM,
            format_args!("from {MIN_ZOOM} to {MAX_ZOOM}"),
        )?,
    };
    let size = match args.map.get("size") {
        None | Some(Value::Null) => RENDER_SIZE,
        Some(value) => (value.as_u64())
            .and_then(|size| u32::try_from(size).ok())
            .filter(|size| (MIN_RENDER_SIZE..=MAX_RENDER_SIZE).contains(size))
            .ok_or_else(|| {
                args.error(format_args!(
                    "size is a whole number of pixels from {MIN_RENDER_SIZE} to {MAX_RENDER_SIZE}"
                ))
            })?,
    };
    const HIGHLIGHT: &str = "highlight is a list of {\"face\": key} and {\"edge\": key} (keys \
                             from body_topology)";
    let highlight = (args.list("highlight", 0, MAX_ITEMS, "faces and edges")?)
        .unwrap_or_default()
        .iter()
        .map(|item| match item {
            Value::Object(map) if map.len() == 1 => args
                .keyed(map, &[ItemKind::Face, ItemKind::Edge])?
                .ok_or_else(|| args.error(HIGHLIGHT)),
            _ => Err(args.error(HIGHLIGHT)),
        })
        .collect::<Result<_, _>>()?;
    Ok(Render {
        bodies: args.names("bodies")?,
        from,
        perspective,
        zoom,
        highlight,
        size,
    })
}

/// The words `render`'s `projection` takes, the default first: whether
/// it's perspective.
pub(super) const PROJECTIONS: &[(&str, bool)] = &[("orthographic", false), ("perspective", true)];

/// What `render`'s `from` takes, in words.
const RENDER_FROM: &str = "from is \"iso\", \"front\", \"back\", \"left\", \"right\", \"top\" \
                           or \"bottom\", or {\"yaw\": angle, \"pitch\": angle}, each typed text \
                           (\"30 deg\") or a number of degrees";

/// `offset` and `limit`, `limit` defaulting to `default` and cut down to
/// [`MAX_LIMIT`].
pub(super) fn page(args: &Arguments, default: usize) -> Result<Page, CallError> {
    let offset = args.count("offset")?.unwrap_or(0);
    let limit = args.count("limit")?.unwrap_or(default).min(MAX_LIMIT);
    if limit == 0 {
        return Err(args.error("limit must be at least 1"));
    }
    Ok(Page { offset, limit })
}

/// One region as `profiles` lists it: an index, or a point `[x, y]`,
/// each coordinate typed text of at most [`TEXT_BYTES`] or a finite
/// number.
fn profile(item: &Value) -> Option<Profile> {
    match item {
        Value::Number(_) => item
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .map(Profile::Index),
        Value::Array(point) if point.len() == 2 => Some(Profile::Point([
            coordinate(&point[0])?,
            coordinate(&point[1])?,
        ])),
        _ => None,
    }
}

/// An id: a whole number that fits a `u32`.
pub(super) fn id(value: &Value) -> Option<u32> {
    value.as_u64().and_then(|n| u32::try_from(n).ok())
}

/// A coordinate or a length: typed text of at most [`TEXT_BYTES`], not
/// empty, or a finite number, taken as its text.
pub(super) fn coordinate(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.trim().is_empty() && text.len() <= TEXT_BYTES => {
            Some(text.clone())
        }
        Value::Number(number) => number
            .as_f64()
            .filter(|n| n.is_finite())
            .map(|_| number.to_string()),
        _ => None,
    }
}

/// A tool's arguments (or an object within them), read field by field.
pub(super) struct Arguments<'a> {
    pub(super) tool: &'a str,
    pub(super) map: &'a Map<String, Value>,
}

impl<'a> Arguments<'a> {
    pub(super) fn error(&self, message: impl std::fmt::Display) -> CallError {
        CallError::Arguments {
            at: self.tool.to_owned(),
            message: message.to_string(),
        }
    }

    /// Refuses keys not in `known`, so a misspelt filter isn't ignored.
    pub(super) fn only(&self, known: &[&str]) -> Result<(), CallError> {
        self.only_where(|key| known.contains(&key))
    }

    /// Refuses keys that aren't `known`.
    pub(super) fn only_where(&self, known: impl Fn(&str) -> bool) -> Result<(), CallError> {
        match self.map.keys().find(|k| !known(k)) {
            None => Ok(()),
            Some(key) => Err(self.error(format_args!("no argument named {:?}", shown(key)))),
        }
    }

    /// Refuses keys not in `fields`, a creating tool's, nor its `name`
    /// and the names of its new `bodies`.
    pub(super) fn only_making(&self, fields: &[&str], bodies: NewBodies) -> Result<(), CallError> {
        self.only_where(|key| fields.contains(&key) || key == "name" || bodies.key() == Some(key))
    }

    /// A creating tool's `name` and the names of its new `bodies`.
    pub(super) fn naming(&self, bodies: NewBodies) -> Result<Naming, CallError> {
        let names = match bodies {
            NewBodies::None => Vec::new(),
            NewBodies::One => self.optional_name("body_name")?.into_iter().collect(),
            NewBodies::Many => self.names("body_names")?,
        };
        Ok(Naming {
            name: self.optional_name("name")?,
            bodies: names,
        })
    }

    /// The one argument `key`, a name, refusing any other.
    pub(super) fn sole_name(&self, key: &str) -> Result<String, CallError> {
        self.only(&[key])?;
        self.name(key)
    }

    /// A name: a string, not empty, at most [`NAME_BYTES`].
    pub(super) fn name(&self, key: &str) -> Result<String, CallError> {
        match self.map.get(key) {
            None => Err(self.error(format_args!("{key} is required"))),
            Some(value) => self.check_name(key, value),
        }
    }

    /// [`Arguments::name`] of `value`, sent as `key`.
    fn check_name(&self, key: &str, value: &Value) -> Result<String, CallError> {
        match value {
            Value::String(s) if s.trim().is_empty() => {
                Err(self.error(format_args!("{key} is empty")))
            }
            Value::String(s) if s.len() > NAME_BYTES => {
                Err(self.error(format_args!("{key} is longer than {NAME_BYTES} bytes")))
            }
            Value::String(s) => Ok(s.clone()),
            _ => Err(self.error(format_args!("{key} is a string"))),
        }
    }

    /// An expression: a string, not empty, at most [`TEXT_BYTES`].
    pub(super) fn text(&self, key: &str) -> Result<String, CallError> {
        match self.map.get(key) {
            Some(Value::String(s)) if s.len() > TEXT_BYTES => {
                Err(self.error(format_args!("{key} is longer than {TEXT_BYTES} bytes")))
            }
            _ => self.name(key),
        }
    }

    /// A number, required: finite (JSON has no others, but checked all
    /// the same).
    fn number(&self, key: &str) -> Result<f64, CallError> {
        match self.map.get(key) {
            None => Err(self.error(format_args!("{key} is required"))),
            Some(value) => (value.as_f64())
                .filter(|v| v.is_finite())
                .ok_or_else(|| self.error(format_args!("{key} is a number"))),
        }
    }

    /// A number, required, within `range`, which `said` says.
    pub(super) fn number_in(
        &self,
        key: &str,
        range: impl std::ops::RangeBounds<f64>,
        said: impl std::fmt::Display,
    ) -> Result<f64, CallError> {
        let number = self.number(key)?;
        match range.contains(&number) {
            true => Ok(number),
            false => Err(self.error(format_args!("{key} is {said}"))),
        }
    }

    /// A value that must be sent but may be null, `None` for null:
    /// `null_is` says what null asks for.
    pub(super) fn nullable(
        &self,
        key: &str,
        null_is: &str,
    ) -> Result<Option<&'a Value>, CallError> {
        match self.map.get(key) {
            None => Err(self.error(format_args!("{key} is required (null for {null_is})"))),
            Some(Value::Null) => Ok(None),
            Some(value) => Ok(Some(value)),
        }
    }

    /// The arguments of the object `map` sent among these, of the same
    /// tool.
    pub(super) fn nested(&self, map: &'a Map<String, Value>) -> Arguments<'a> {
        Arguments {
            tool: self.tool,
            map,
        }
    }

    /// A boolean, `default` if not sent.
    pub(super) fn flag(&self, key: &str, default: bool) -> Result<bool, CallError> {
        Ok(self.boolean(key)?.unwrap_or(default))
    }

    /// A name, if sent: as [`Arguments::name`].
    pub(super) fn optional_name(&self, key: &str) -> Result<Option<String>, CallError> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(_) => self.name(key).map(Some),
        }
    }

    /// One of `choices` by its word, if sent: as [`Arguments::choice`].
    pub(super) fn optional_choice<T: Copy>(
        &self,
        key: &str,
        choices: &[(&str, T)],
    ) -> Result<Option<T>, CallError> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(_) => self.choice(key, choices).map(Some),
        }
    }

    pub(super) fn required_boolean(&self, key: &str) -> Result<bool, CallError> {
        self.boolean(key)?
            .ok_or_else(|| self.error(format_args!("{key} is required")))
    }

    fn object(&self, key: &'a str) -> Result<Option<Arguments<'a>>, CallError> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Object(map)) => Ok(Some(self.nested(map))),
            Some(_) => Err(self.error(format_args!("{key} is an object"))),
        }
    }

    fn boolean(&self, key: &str) -> Result<Option<bool>, CallError> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(_) => Err(self.error(format_args!("{key} is true or false"))),
        }
    }

    /// A whole number, 0 or more, that fits a `usize`.
    pub(super) fn count(&self, key: &str) -> Result<Option<usize>, CallError> {
        let value = match self.map.get(key) {
            None | Some(Value::Null) => return Ok(None),
            Some(value) => value,
        };
        value
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| self.error(format_args!("{key} is a whole number, 0 or more")))
    }

    /// One of `choices` by its word, required.
    pub(super) fn required_choice<T: Copy>(
        &self,
        key: &str,
        choices: &[(&str, T)],
    ) -> Result<T, CallError> {
        if matches!(self.map.get(key), None | Some(Value::Null)) {
            return Err(self.error(format_args!("{key} is required")));
        }
        self.choice(key, choices)
    }

    /// One of `choices` by its word, the first if not sent.
    pub(super) fn choice<T: Copy>(&self, key: &str, choices: &[(&str, T)]) -> Result<T, CallError> {
        let word = match self.map.get(key) {
            None | Some(Value::Null) => return Ok(choices[0].1),
            Some(Value::String(word)) => Some(word.as_str()),
            Some(_) => None,
        };
        word.and_then(|word| lookup(choices, word)).ok_or_else(|| {
            let words: Vec<String> = choices
                .iter()
                .map(|(said, _)| format!("{said:?}"))
                .collect();
            let (last, rest) = words.split_last().expect("choices aren't empty");
            self.error(format_args!("{key} is {} or {last}", rest.join(", ")))
        })
    }

    /// An expression, if sent: as [`Arguments::text`].
    pub(super) fn optional_text(&self, key: &str) -> Result<Option<String>, CallError> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(_) => self.text(key).map(Some),
        }
    }

    /// A list of names, at most [`MAX_ITEMS`]; none if not sent.
    pub(super) fn names(&self, key: &str) -> Result<Vec<String>, CallError> {
        (self.list(key, 0, MAX_ITEMS, "names")?)
            .unwrap_or_default()
            .iter()
            .map(|item| self.check_name(key, item))
            .collect()
    }

    /// `key`, a list of `least` to `most` `what`; none if not sent.
    fn list(
        &self,
        key: &str,
        least: usize,
        most: usize,
        what: &str,
    ) -> Result<Option<&'a [Value]>, CallError> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Array(items)) if (least..=most).contains(&items.len()) => Ok(Some(items)),
            Some(_) if least == 0 => {
                Err(self.error(format_args!("{key} is a list of at most {most} {what}")))
            }
            Some(_) => Err(self.error(format_args!("{key} is a list of {least} to {most} {what}"))),
        }
    }

    /// [`Arguments::list`], required.
    pub(super) fn required_list(
        &self,
        key: &str,
        least: usize,
        most: usize,
        what: &str,
    ) -> Result<&'a [Value], CallError> {
        (self.list(key, least, most, what)?)
            .ok_or_else(|| self.error(format_args!("{key} is required")))
    }

    /// `profiles`, required: `"all"`, or a list of region indices and
    /// points `[x, y]` (each a length as typed text, or a number), at
    /// least one and at most [`MAX_ITEMS`].
    pub(super) fn profiles(&self) -> Result<Profiles, CallError> {
        const SAID: &str = "profiles is \"all\" or a list of region indices (as sketch lists \
                            them) and points [x, y] inside regions";
        let items = match self.map.get("profiles") {
            Some(Value::String(all)) if all == "all" => return Ok(Profiles::All),
            Some(Value::String(_)) => return Err(self.error(SAID)),
            _ => self.required_list("profiles", 1, MAX_ITEMS, "regions, or \"all\"")?,
        };
        items
            .iter()
            .map(|item| profile(item).ok_or_else(|| self.error(SAID)))
            .collect::<Result<_, _>>()
            .map(Profiles::Picked)
    }

    /// A revolve's `axis`, required.
    pub(super) fn axis(&self) -> Result<Axis, CallError> {
        const SAID: &str = "axis is \"x\" or \"y\" (the sketch's axes), {\"line\": id} (a line \
                            of the sketch) or {\"edge\": key} (a straight edge from body_topology)";
        match self.map.get("axis") {
            None => Err(self.error("axis is required")),
            Some(Value::String(axis)) if axis.eq_ignore_ascii_case("x") => Ok(Axis::SketchX),
            Some(Value::String(axis)) if axis.eq_ignore_ascii_case("y") => Ok(Axis::SketchY),
            Some(Value::Object(map)) => {
                let axis = self.nested(map);
                axis.only(&["line", "edge"])?;
                match (map.get("line"), map.contains_key("edge")) {
                    (Some(line), false) => id(line)
                        .map(Axis::Line)
                        .ok_or_else(|| self.error("axis.line is a line's id, a whole number")),
                    (None, true) => axis.name("edge").map(Axis::Edge),
                    _ => Err(self.error(SAID)),
                }
            }
            Some(_) => Err(self.error(SAID)),
        }
    }

    /// A blend's `edges` and `faces`, keys, each at most [`MAX_ITEMS`]:
    /// at least one between them.
    pub(super) fn edges_and_faces(&self) -> Result<(Vec<String>, Vec<String>), CallError> {
        let (edges, faces) = (self.names("edges")?, self.names("faces")?);
        if edges.is_empty() && faces.is_empty() {
            return Err(self.error("edges or faces is required: at least one edge's or face's key"));
        }
        Ok((edges, faces))
    }

    /// A list of chains of sketch curves `key`, each `{"sketch": name,
    /// "curve": id}`, at most [`MAX_ITEMS`]; none if not sent.
    pub(super) fn curves_in(&self, key: &str) -> Result<Vec<CurveIn>, CallError> {
        const WHAT: &str = "{\"sketch\": name, \"curve\": id}";
        let said = || self.error(format_args!("{key} is a list of {WHAT}"));
        (self.list(key, 0, MAX_ITEMS, WHAT)?)
            .unwrap_or_default()
            .iter()
            .map(|item| {
                let Value::Object(map) = item else {
                    return Err(said());
                };
                let one = self.nested(map);
                one.only(&["sketch", "curve"])?;
                let curve = map.get("curve").and_then(id).ok_or_else(said)?;
                Ok(CurveIn {
                    sketch: one.name("sketch")?,
                    curve,
                })
            })
            .collect()
    }

    /// A sweep's `path` or `helix`, one of them.
    pub(super) fn sweep_along(&self) -> Result<SweepAlong, CallError> {
        match (self.object("path")?, self.object("helix")?) {
            (Some(path), None) => {
                path.only(&["curves", "edges", "tangent"])?;
                let curves = path.curves_in("curves")?;
                let edges = path.names("edges")?;
                if curves.is_empty() && edges.is_empty() {
                    return Err(self.error(
                        "path takes at least one of curves (chains of sketch curves) and \
                         edges (model edges' keys)",
                    ));
                }
                Ok(SweepAlong::Path {
                    curves,
                    edges,
                    tangent: path.flag("tangent", true)?,
                })
            }
            (None, Some(helix)) => {
                helix.only(&["axis", "pitch", "turns", "left_handed", "flip"])?;
                let axis = helix.reference("axis", WORLD_AXES, AXIS_ITEMS)?;
                Ok(SweepAlong::Helix {
                    axis: axis.ok_or_else(|| self.error("helix.axis is required"))?,
                    pitch: helix.optional_text("pitch")?,
                    turns: helix.number_text("turns")?,
                    left_handed: helix.flag("left_handed", false)?,
                    flip: helix.flag("flip", false)?,
                })
            }
            _ => {
                Err(self.error("path or helix is required, one of them: what the sweep runs along"))
            }
        }
    }

    /// A loft's `sections`, required: 1 to [`MAX_ITEMS`], each
    /// `{"sketch": name, "region": index or [x, y], "start"?: id}` or
    /// `{"sketch": name, "point": id}`.
    pub(super) fn sections(&self) -> Result<Vec<Section>, CallError> {
        const WHAT: &str = "sections, each {\"sketch\": name, \"region\": index or [x, y], \
                            \"start\"?: point id} or {\"sketch\": name, \"point\": id}";
        let said = || self.error(format_args!("sections is a list of {WHAT}"));
        (self.required_list("sections", 1, MAX_ITEMS, WHAT)?)
            .iter()
            .map(|item| {
                let Value::Object(map) = item else {
                    return Err(said());
                };
                let one = self.nested(map);
                one.only(&["sketch", "region", "point", "start"])?;
                let shape = match (map.get("region"), map.get("point")) {
                    (Some(region), None) => SectionShape::Region(profile(region).ok_or_else(said)?),
                    (None, Some(point)) if !map.contains_key("start") => {
                        SectionShape::Point(id(point).ok_or_else(said)?)
                    }
                    (None, Some(_)) => {
                        return Err(self.error("a section's start goes with a region"));
                    }
                    _ => return Err(said()),
                };
                let start = match map.get("start") {
                    None | Some(Value::Null) => None,
                    Some(start) => Some(id(start).ok_or_else(said)?),
                };
                Ok(Section {
                    sketch: one.name("sketch")?,
                    shape,
                    start,
                })
            })
            .collect()
    }

    /// A face feature's faces, required: at least one key, at most
    /// [`MAX_ITEMS`].
    pub(super) fn faces(&self) -> Result<Vec<String>, CallError> {
        let faces = self.names("faces")?;
        if faces.is_empty() {
            return Err(self.error("faces is required: at least one face's key"));
        }
        Ok(faces)
    }

    /// A split's `tool`, required: an origin plane's word, or one of
    /// `{"face": key}`, `{"body": name}`, `{"sketch": name, "profiles":
    /// ...}` and `{"sketch": name, "curves": [id, ...]}`.
    pub(super) fn split_tool(&self) -> Result<SplitWith, CallError> {
        const SAID: &str = "tool is \"xy\", \"xz\" or \"yz\" (an origin plane), {\"face\": key} \
                            (a face from body_topology), {\"body\": name}, {\"sketch\": name, \
                            \"profiles\": ...} or {\"sketch\": name, \"curves\": [id, ...]}";
        let map = match self.map.get("tool") {
            None | Some(Value::Null) => return Err(self.error("tool is required")),
            Some(Value::String(word)) => {
                return lookup(WORLD_PLANES, word)
                    .map(|plane| SplitWith::Surface(Reference::World(plane)))
                    .ok_or_else(|| self.error(SAID));
            }
            Some(Value::Object(map)) => map,
            Some(_) => return Err(self.error(SAID)),
        };
        let tool = self.nested(map);
        let has = |key: &str| map.contains_key(key);
        if has("face") || has("body") {
            if map.len() != 1 {
                return Err(self.error(SAID));
            }
            if has("body") {
                return tool.name("body").map(SplitWith::Body);
            }
            let face = self.reference("tool", WORLD_PLANES, &[ItemKind::Face])?;
            return Ok(SplitWith::Surface(face.expect("tool was sent")));
        }
        match (has("profiles"), has("curves")) {
            (true, false) => {
                tool.only(&["sketch", "profiles"])?;
                Ok(SplitWith::Regions {
                    sketch: tool.name("sketch")?,
                    profiles: tool.profiles()?,
                })
            }
            (false, true) => {
                tool.only(&["sketch", "curves"])?;
                const WHAT: &str = "curve ids, whole numbers";
                let curves = (tool.required_list("curves", 1, MAX_ITEMS, WHAT)?)
                    .iter()
                    .map(id)
                    .collect::<Option<Vec<u32>>>()
                    .ok_or_else(|| tool.error(format_args!("curves is a list of {WHAT}")))?;
                Ok(SplitWith::Line {
                    sketch: tool.name("sketch")?,
                    curves,
                })
            }
            _ => Err(self.error(SAID)),
        }
    }

    /// A motion's bodies, required: at least one name, at most
    /// [`MAX_ITEMS`].
    pub(super) fn bodies(&self) -> Result<Vec<String>, CallError> {
        let bodies = self.names("bodies")?;
        if bodies.is_empty() {
            return Err(self.error("bodies is required: at least one body"));
        }
        Ok(bodies)
    }

    /// A count, if sent: typed text as [`Arguments::text`], or a whole
    /// number, taken as its text for the panel's field to read.
    pub(super) fn count_text(&self, key: &str) -> Result<Option<String>, CallError> {
        match self.map.get(key) {
            Some(Value::Number(_)) => self
                .count(key)
                .map(|count| count.map(|count| count.to_string())),
            _ => self.optional_text(key),
        }
    }

    /// A value, if sent: typed text as [`Arguments::text`], or a number
    /// (JSON's are finite), taken as its text for the panel's field to
    /// read and bound.
    fn number_text(&self, key: &str) -> Result<Option<String>, CallError> {
        match self.map.get(key) {
            Some(Value::Number(number)) => Ok(Some(number.to_string())),
            _ => self.optional_text(key),
        }
    }

    /// One of the world's, by a word of `worlds` (in any case), or a
    /// model item by key, `{"edge": key}` and the like, of `kinds`; none
    /// if not sent.
    pub(super) fn reference<W: Copy>(
        &self,
        key: &str,
        worlds: &[(&str, W)],
        kinds: &[ItemKind],
    ) -> Result<Option<Reference<W>>, CallError> {
        let said = || {
            let mut choices: Vec<String> = worlds.iter().map(|(w, _)| format!("{w:?}")).collect();
            choices.extend(
                kinds
                    .iter()
                    .map(|kind| format!("{{\"{}\": key}}", kind.word())),
            );
            let (last, rest) = choices.split_last().expect("choices aren't empty");
            self.error(format_args!(
                "{key} is {} or {last} (keys from body_topology)",
                rest.join(", ")
            ))
        };
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(word)) => lookup(worlds, word)
                .map(|world| Some(Reference::World(world)))
                .ok_or_else(said),
            Some(Value::Object(map)) if map.len() == 1 => {
                let item = self.keyed(map, kinds)?.ok_or_else(said)?;
                Ok(Some(Reference::Key(item)))
            }
            Some(_) => Err(said()),
        }
    }

    /// The model item `map` names by key, of one of `kinds`: `None` if
    /// it names none of them.
    fn keyed(
        &self,
        map: &'a Map<String, Value>,
        kinds: &[ItemKind],
    ) -> Result<Option<Item>, CallError> {
        let Some(kind) = kinds.iter().find(|kind| map.contains_key(kind.word())) else {
            return Ok(None);
        };
        let key = self.nested(map).name(kind.word())?;
        Ok(Some(Item { kind: *kind, key }))
    }

    /// What `measure` takes as `key`, if sent: `{"body": name}`, or a
    /// face, edge or corner by key.
    pub(super) fn measured(&self, key: &'a str) -> Result<Option<Measured>, CallError> {
        let said = || self.error(format_args!("{key} is {MEASURED}"));
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Object(map)) if map.len() == 1 && map.contains_key("body") => {
                Ok(Some(Measured::Body(self.nested(map).name("body")?)))
            }
            Some(Value::Object(map)) if map.len() == 1 => {
                let kinds = [ItemKind::Face, ItemKind::Edge, ItemKind::Corner];
                let item = self.keyed(map, &kinds)?.ok_or_else(said)?;
                Ok(Some(Measured::Item(item)))
            }
            Some(_) => Err(said()),
        }
    }

    /// A sketch's `plane`, required: an origin plane or a face.
    pub(super) fn sketch_plane(&self) -> Result<Reference<WorldPlane>, CallError> {
        self.reference("plane", WORLD_PLANES, &[ItemKind::Face])?
            .ok_or_else(|| {
                self.error("plane is required: \"xy\", \"xz\", \"yz\" or {\"face\": key}")
            })
    }

    /// What a link comes from, `from`, required: a model item by key, or
    /// another sketch's curve or point by id.
    pub(super) fn link_from(&self) -> Result<LinkFrom, CallError> {
        const SAID: &str = "from is required: {\"edge\": key}, {\"face\": key}, \
                            {\"corner\": key} (keys from body_topology), or {\"sketch\": name, \
                            \"curve\": id} or {\"sketch\": name, \"point\": id}";
        let Some(from) = self.object("from")? else {
            return Err(self.error(SAID));
        };
        let id_of = |key: &str| {
            (from.map.get(key))
                .and_then(id)
                .ok_or_else(|| self.error(SAID))
        };
        if from.map.contains_key("sketch") {
            from.only(&["sketch", "curve", "point"])?;
            let sketch = from.name("sketch")?;
            return match (
                from.map.contains_key("curve"),
                from.map.contains_key("point"),
            ) {
                (true, false) => Ok(LinkFrom::Curve(CurveIn {
                    sketch,
                    curve: id_of("curve")?,
                })),
                (false, true) => Ok(LinkFrom::Point {
                    sketch,
                    point: id_of("point")?,
                }),
                _ => Err(self.error(SAID)),
            };
        }
        let kinds = [ItemKind::Face, ItemKind::Edge, ItemKind::Corner];
        match self.reference("from", &[] as &[(&str, ())], &kinds) {
            Ok(Some(Reference::Key(item))) => Ok(LinkFrom::Item(item)),
            _ => Err(self.error(SAID)),
        }
    }

    /// An align's side `key`, required: `{point, direction?,
    /// second_direction?}`.
    pub(super) fn align_side(&self, key: &'a str) -> Result<AlignSide, CallError> {
        let side = self
            .object(key)?
            .ok_or_else(|| self.error(format_args!("{key} is required: {{point, direction?}}")))?;
        side.only(&["point", "direction", "second_direction"])?;
        let point = side
            .reference("point", ORIGIN, POINT_ITEMS)?
            .ok_or_else(|| self.error(format_args!("{key}.point is required")))?;
        Ok(AlignSide {
            point,
            direction: side.reference("direction", WORLD_AXES, AXIS_ITEMS)?,
            second_direction: side.reference("second_direction", WORLD_AXES, AXIS_ITEMS)?,
        })
    }

    fn direction(&self, key: &str) -> Result<Option<Direction>, CallError> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => direction(value).map(Some).ok_or_else(|| {
                self.error(format_args!(
                    "{key} is an axis such as \"+z\", \"-x\" or \"y\", or three numbers \
                     not all zero"
                ))
            }),
        }
    }
}

/// What `measure`'s `a` and `b` take, in words.
pub(super) const MEASURED: &str = "{\"face\": key}, {\"edge\": key}, {\"corner\": key} (keys from \
                        body_topology) or {\"body\": name}";

/// `"+z"`, `"-x"`, `"z"` (in any case), or `[x, y, z]`: finite, not all
/// zero, then scaled to unit length.
fn direction(value: &Value) -> Option<Direction> {
    match value {
        Value::String(text) => {
            let text = text.trim();
            let (sign, axis, signed) = match text.as_bytes() {
                [b'+', axis] => (1.0, *axis, true),
                [b'-', axis] => (-1.0, *axis, true),
                [axis] => (1.0, *axis, false),
                _ => return None,
            };
            let mut vector = [0.0; 3];
            let index = match axis.to_ascii_lowercase() {
                b'x' => 0,
                b'y' => 1,
                b'z' => 2,
                _ => return None,
            };
            vector[index] = sign;
            Some(Direction { vector, signed })
        }
        Value::Array(items) if items.len() == 3 => {
            let mut vector = [0.0; 3];
            for (v, item) in vector.iter_mut().zip(items) {
                *v = item.as_f64().filter(|v| v.is_finite())?;
            }
            normalize(vector).map(|vector| Direction {
                vector,
                signed: true,
            })
        }
        _ => None,
    }
}

/// `v` at unit length, `None` if it's zero. Divided by its largest
/// component first, so squaring can neither overflow nor underflow.
fn normalize(v: [f64; 3]) -> Option<[f64; 3]> {
    let largest = v.iter().fold(0.0f64, |m, c| m.max(c.abs()));
    if largest == 0.0 || !largest.is_finite() {
        return None;
    }
    let scaled = v.map(|c| c / largest);
    let length = scaled.iter().map(|c| c * c).sum::<f64>().sqrt();
    Some(scaled.map(|c| c / length))
}
