//! The document toolbar.

use std::borrow::Cow;

use iced::widget::{
    Button, Space, button, column, container, mouse_area, row, space, text, tooltip,
};
use iced::{Alignment, Element, Length, Padding};
use varde_document::{Axis3, EXTENSION, FeatureId, FeatureKind, OriginPlane};
use varde_regen::Summary;
use varde_sketch::Curve;

use crate::chrome::{Edge, edged, icon_button, key_label, vrule};
use crate::icons::{self, Icon};
use crate::motion::{axis_label, plane_label};
use crate::rail::{Entry, Using};
use crate::shortcut::{
    Binding, DocumentKeys, Shortcut, close_binding, comb_binding, commit_binding,
    constrain_binding, constraint_binding, file_bindings, handles_binding, history_bindings,
    insert_point_binding, plane_binding, remove_point_binding, switch_binding, tool_binding,
};
use crate::theme::{self, Emphasis, SEMIBOLD, SIDE_PANEL_INNER_WIDTH, Tone};
use crate::{
    ActiveTool, AlignRole, AlignSide, ConstraintKind, DocumentState, Edit, Location, Look, Message,
    MotionKind, MotionLook, MotionPick, NOT_SAVED, Overlay, Picked, SketchState, SplitMode, Tool,
};

/// Includes the 1 px border.
pub(crate) const TOOLBAR_HEIGHT: f32 = 40.0;

/// The width of the Save button's cell, right of the file cell.
const SAVE_CELL_WIDTH: f32 = 34.0;

/// The bar's spacing between its items.
const BAR_SPACING: f32 = 2.0;

pub fn toolbar<'a>(state: &DocumentState<'a>) -> Element<'a, Message> {
    // Worked out once: every button shows the same snapshot.
    let keys = state.keys();
    let operation = operation(state, keys);
    let (context, tag): (Element<'a, Message>, _) = match (&state.sketch, &operation) {
        (Some(sketch), _) => (
            pill(
                Icon::Sketch,
                sketch.name,
                Some(Message::Look(Look::FinishSketch)),
                if sketch.modified {
                    "Finish sketch (Esc)"
                } else {
                    "Leave sketch, unchanged (Esc)"
                },
                if sketch.modified {
                    Icon::Check
                } else {
                    Icon::Close
                },
            ),
            match sketch.tool {
                Some(tool) => Some(tool_tag(&tool)),
                None if sketch.constraining => Some("Constrain".to_owned()),
                None => None,
            },
        ),
        // An operation being set up shows as a sketch being edited does:
        // its name, with OK joined to it.
        (None, Some(operation)) => (
            pill(
                operation.icon,
                operation.name,
                operation.ok.clone(),
                "OK (Enter)",
                Icon::Check,
            ),
            None,
        ),
        (None, None) => (
            text("Model").font(SEMIBOLD).into(),
            if let Some(pick) = state.picking_plane {
                Some(match &pick.sketch {
                    None => "New sketch".to_owned(),
                    Some((_, name)) => format!("{name}'s plane"),
                })
            } else {
                state.measure().map(|_| "Measure".to_owned())
            },
        ),
    };
    let tag = tag.map(|tag| {
        container(text(tag).size(11).font(SEMIBOLD))
            .padding([2, 7])
            .style(theme::tag)
    });

    let [undo, redo, _] = history_bindings(keys);
    let bar = row![
        // The rule right after the file cell, its wash meeting it.
        row![file_cell(state), vrule()],
        save_cell(state.editable(), state.file.edited),
        vrule(),
        container(row![context, tag].spacing(6).align_y(Alignment::Center))
            // A pill starts left of where the text would, so its name
            // lines up with "Model".
            .padding(Padding::from([0, 12]).left(
                if state.sketch.is_some() || operation.is_some() {
                    4
                } else {
                    12
                },
            )),
        vrule(),
        // The ops and what's empty of the bar after them, cut off where
        // they would run under Undo, Redo and the settings gear, which
        // are laid out first and so always show.
        container(
            row![
                row(ops(state, keys, operation.as_ref()))
                    .spacing(2)
                    .padding([0, 6])
                    .align_y(Alignment::Center),
                // What's empty of the bar: a click on it clears the selection.
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                    .on_press(crate::panels::CLEAR_SELECTION),
            ]
            .align_y(Alignment::Center)
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(Alignment::Center)
        .clip(true),
        history_button(Icon::Undo, "Undo", undo),
        history_button(Icon::Redo, "Redo", redo),
        separator(),
        crate::mcp::toggle(state.mcp.as_ref()),
        crate::settings::settings_button(state.settings.is_some()),
        Space::new().width(8),
    ]
    .spacing(BAR_SPACING)
    .height(Length::Fill)
    .align_y(Alignment::Center);

    edged(
        container(bar).width(Length::Fill).style(theme::toolbar),
        Edge::Bottom,
        TOOLBAR_HEIGHT,
    )
}

/// The operation being set up, as the toolbar shows it.
struct Operation<'a> {
    icon: Icon,
    /// "New revolve", or the feature edited.
    name: &'a str,
    /// What OK sends, or nothing while it can't be pressed.
    ok: Option<Message>,
    /// What Cancel sends.
    cancel: Message,
}

/// The operation being set up, if one is.
fn operation<'a>(state: &DocumentState<'a>, keys: DocumentKeys) -> Option<Operation<'a>> {
    // OK sends what `Enter` does, when it does.
    let ok = commit_binding(keys).and_then(|binding| binding.sends());
    let (icon, name, cancel) = state.operation.as_ref()?.heading()?;
    Some(Operation {
        icon,
        name,
        ok,
        cancel,
    })
}

/// The sketch being edited or the operation being set up, named with
/// `icon` on the soft accent, joined at its right end by the button
/// finishing it, showing `mark` and sending `finish` (disabled without),
/// which hovering tells `tip`.
fn pill<'a>(
    icon: Icon,
    name: &'a str,
    finish: Option<Message>,
    tip: &'static str,
    mark: Icon,
) -> Element<'a, Message> {
    let finish = button(
        container(icons::tinted(mark, icons::INLINE, |p| {
            Emphasis::Primary.content(p)
        }))
        .center(Length::Fill),
    )
    .width(26)
    .height(Length::Fill)
    .padding(0)
    .style(theme::pill_end_button)
    .on_press_maybe(finish);
    container(
        row![
            container(
                row![
                    icons::icon(icon, icons::INLINE),
                    text(shortened(name, PILL_NAME_CHARS)).font(SEMIBOLD)
                ]
                .spacing(6)
                .align_y(Alignment::Center)
            )
            .padding([0, 8]),
            crate::chrome::tip(finish, text(tip)),
        ]
        .height(Length::Fill)
        .align_y(Alignment::Center),
    )
    .height(24)
    .style(theme::pill)
    .into()
}

/// The most characters of a name a pill shows: a longer one (a file's;
/// the app's own are short) is cut short to fit the bar at 1280 px.
const PILL_NAME_CHARS: usize = 32;

/// `name`, cut to its first `max − 1` characters and "…" if it's longer
/// than `max`.
fn shortened(name: &str, max: usize) -> Cow<'_, str> {
    match name.char_indices().nth(max) {
        None => Cow::Borrowed(name),
        Some(_) => {
            let end = name
                .char_indices()
                .nth(max.saturating_sub(1))
                .map_or(0, |(at, _)| at);
            Cow::Owned(format!("{}…", name[..end].trim_end()))
        }
    }
}

/// What the toolbar's tag says of `tool`: its name, and how it draws: the
/// Rectangle tool from the centre, the Polygon tool's sides, the Spline
/// tool by control points, construction geometry.
fn tool_tag(tool: &ActiveTool<'_>) -> String {
    let how = match tool.tool {
        Tool::Rectangle if tool.centered => Some("From center".to_owned()),
        Tool::Polygon => Some(format!("{} sides", tool.sides)),
        Tool::Spline if tool.control => Some("Control points".to_owned()),
        _ => None,
    };
    let construction = tool.construction.then(|| "Construction".to_owned());
    std::iter::once(tool.tool.label().to_owned())
        .chain(how)
        .chain(construction)
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The Save button, telling its key while hovered: disabled unless the
/// document is `editable` and `edited`, as the file menu's Save is.
fn save_cell<'a>(editable: bool, edited: bool) -> Element<'a, Message> {
    let [save, _] = file_bindings(editable, edited);
    let message = save.sends();
    let content = icons::button_content(Icon::Save, message.is_some(), |p, hovered| {
        theme::flat_content(p, Tone::Muted, true, hovered)
    });
    let button = crate::chrome::tip(
        button(content)
            .padding(0)
            .style(theme::flat_button(false, theme::Tone::Text))
            .on_press_maybe(message),
        text(save.tip("Save")),
    );
    container(button)
        .center_x(SAVE_CELL_WIDTH)
        .height(Length::Fill)
        .align_y(Alignment::Center)
        .into()
}

/// The document name with a dirty dot and a chevron, opening the file
/// menu; [`NOT_SAVED`] on a grey pill in place of a name the design hasn't.
/// Pointing at it says where the design is kept, on the web, or natively
/// its path, and whether it has changes not saved.
fn file_cell<'a>(state: &DocumentState<'a>) -> Element<'a, Message> {
    let open = state.overlay == Some(Overlay::FileMenu);
    let dirty = (state.file.edited)
        .then(|| container(Space::new().width(6).height(6)).style(theme::dirty_dot));
    let name: Element<'a, Message> = if state.file.unnamed {
        container(text(NOT_SAVED).size(11.5).font(SEMIBOLD))
            .padding([1, 8])
            .style(theme::name_pill)
            .into()
    } else {
        row![
            text(state.file.name).font(SEMIBOLD),
            text(format!(".{EXTENSION}")).style(theme::faint_text)
        ]
        .into()
    };

    let content = row![
        name,
        dirty,
        space::horizontal(),
        icons::tinted(Icon::Chev, icons::INLINE, |p| p.muted),
    ]
    .spacing(8)
    .height(Length::Fill)
    .align_y(Alignment::Center);

    // With the rule after it and the Save cell, as wide as the side panel
    // under them, so the rule after the Save cell is in line with the
    // panel's edge.
    let cell = button(content)
        .width(SIDE_PANEL_INNER_WIDTH - SAVE_CELL_WIDTH - 1.0 - BAR_SPACING)
        .height(Length::Fill)
        .padding(Padding::from([0, 10]).left(12))
        .style(theme::file_cell(open))
        .on_press(Message::Edit(Edit::ToggleFileMenu));

    // Not while the menu it opens shows under it.
    let mut lines: Vec<Element<'a, Message>> = Vec::new();
    match (state.file.location, state.file.path) {
        (Some(location), _) if !open => {
            let (said, kept, why) = location_told(location);
            lines.push(text(said).size(12).font(SEMIBOLD).into());
            lines.push(
                text(format!("{}.{EXTENSION}, {kept}", state.file.name))
                    .size(12)
                    .into(),
            );
            lines.push(text(why).size(12).into());
        }
        (None, Some(path)) if !open => lines.push(text(path).size(12).into()),
        _ => {}
    }
    if state.file.edited && !open {
        lines.push(text("Changes not saved").size(12).into());
    }
    if lines.is_empty() {
        return cell.into();
    }
    tooltip(
        cell,
        crate::chrome::tip_box(column(lines).spacing(1)).padding([4, 7]),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .snap_within_viewport(true)
    .into()
}

/// Where a design is kept, on the web, under the file cell: its icon and
/// the words, in the location's colour on the panel's.
pub(crate) fn location_bar<'a>(location: Location) -> Element<'a, Message> {
    let icon = match location {
        Location::Browser => Icon::Browser,
        Location::Computer => Icon::Computer,
    };
    let (said, _, _) = location_told(location);
    let color = move |p: &theme::Palette| theme::location_ink(p, location == Location::Computer);
    edged(
        container(
            row![
                icons::tinted(icon, 13.0, color),
                text(said)
                    .size(11.5)
                    .font(SEMIBOLD)
                    .style(theme::text_in(color)),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .padding([0, 12])
        .align_y(Alignment::Center)
        .style(theme::toolbar),
        Edge::Bottom,
        LOCATION_BAR_HEIGHT,
    )
}

/// The location bar's height, with its 1 px border.
const LOCATION_BAR_HEIGHT: f32 = 26.0;

/// What pointing at the file cell says of where a design is kept, on the
/// web, the first as the location bar says it: where, how it's kept and what that means (a file picked on the
/// computer gives no folder).
fn location_told(location: Location) -> (&'static str, &'static str, &'static str) {
    match location {
        Location::Browser => (
            "In browser storage",
            "kept by this browser",
            "Cleared with the site's data: download a copy to keep it",
        ),
        Location::Computer => (
            "On your computer",
            "picked from your files",
            "Save writes back to that file; the browser doesn't say which folder",
        ),
    }
}

/// The button closing the shape drawn or the curves selected, or with
/// `close` false opening them.
fn close_op(close: bool, binding: Binding) -> Element<'static, Message> {
    let (icon, label) = if close {
        (Icon::CloseCurve, "Close")
    } else {
        (Icon::OpenCurve, "Open")
    };
    tipped_op(icon, label, binding, false)
}

/// Remove point, taking the spline points selected out of their splines,
/// or disabled saying `why` it can't.
fn remove_op(why: Option<String>, binding: Binding) -> Element<'static, Message> {
    match why {
        None => tipped_op(Icon::RemovePoint, "Remove point", binding, false),
        // The binding is disabled here, so the button is too.
        Some(why) => tipped_op_with(
            Icon::RemovePoint,
            "Remove point",
            binding,
            false,
            format!("Remove point: {why}"),
        ),
    }
}

/// The operations for what's going on: modelling, picking the plane for a
/// new sketch, or editing a sketch. While an `operation` is set up, only
/// Cancel and what it picks, its OK being in the pill.
fn ops<'a>(
    state: &DocumentState<'a>,
    keys: DocumentKeys,
    operation: Option<&Operation<'a>>,
) -> Vec<Element<'a, Message>> {
    if let Some(sketch) = &state.sketch {
        return sketch_ops(sketch, keys);
    }
    if state.picking_plane.is_some() {
        return plane_pick_ops(state, keys);
    }
    // While an operation is set up, the bar is Cancel and what the
    // operation picks, as the mock's: the operations would leave no room
    // for the origins at 1280 px wide, and their keys still work.
    if let Some(operation) = operation {
        let cancel = op_button(
            Icon::Close,
            "Cancel",
            Some(Shortcut::ESCAPE),
            false,
            Some(operation.cancel.clone()),
        );
        return std::iter::once(cancel)
            .chain(motion_origins(state))
            .collect();
    }
    // Measure after a separator, as the mock has it.
    let measure = bar_op(state, keys, Entry::Measure);
    // What's selected decides the operations offered, each starting
    // with it ([`selection_bar`]).
    if let Some(bar) = selection_bar(state) {
        return (bar.into_iter())
            .map(|item| match item {
                BarItem::Op(entry) => bar_op(state, keys, entry),
                BarItem::EditSketch(sketch) => op(
                    Icon::Sketch,
                    "Edit sketch",
                    (state.editable()).then_some(Message::Look(Look::EditFeature(sketch))),
                ),
            })
            .chain([separator(), measure])
            .collect();
    }
    // The model bar: Chamfer after the solids, before Combine, as the
    // mock's model bar orders them (its Hole isn't built). The mock's
    // Fillet, before Chamfer, and Shell, after it, would each leave the
    // bar too wide at 1280 px (Fillet to 1210 px of 1160): the rail's
    // Modify set has them, and Offset face too, and Fillet has its key.
    // Move after Combine, as the mock's model bar orders them. Mirror
    // isn't on it (the mock's model bar has none: its body bar does),
    // which left no room at 1280 px wide: the rail's Transform set has
    // it. The linear pattern after Move, with its key, as the mock's
    // model bar has it: the circular one is on the rail (the mock's
    // model bar has none either). Then, by Measure, the design's
    // parameters, lit while their popup is open: as the rail's tool, it
    // opens and closes it.
    let model = [
        Entry::Extrude,
        Entry::Revolve,
        Entry::Motion(MotionKind::Chamfer),
        Entry::Combine,
        Entry::Motion(MotionKind::Move),
        Entry::Motion(MotionKind::LinearPattern),
    ];
    std::iter::once(bar_op(state, keys, Entry::Sketch))
        .chain(model.map(|entry| bar_op(state, keys, entry)))
        .chain([separator(), measure, bar_op(state, keys, Entry::Params)])
        .chain(motion_origins(state))
        .collect()
}

/// The sketch's bar: the constraints fitting the selection while
/// constraining, or with something selected and no drawing tool, those
/// and the curves' edits; else the drawing tools.
fn sketch_ops<'a>(sketch: &SketchState<'a>, keys: DocumentKeys) -> Vec<Element<'a, Message>> {
    let constrain = tipped_op(
        Icon::Constrain,
        "Constrain",
        constrain_binding(keys),
        sketch.constraining,
    );
    // The Constrain tool offers the constraints that fit the
    // selection, the most likely first, in place of the drawing
    // tools: as the mock's model bar changes with what's selected,
    // and there's no room for both at 1280 px wide. The tools are on
    // the rail, their keys still work.
    let fitting = || {
        let fitting = ConstraintKind::fitting(sketch.sketch, sketch.selection);
        fitting.into_iter().filter_map(move |kind| {
            let binding = constraint_binding(kind, keys)?;
            Some(tipped_op(kind.icon(), kind.label(), binding, false))
        })
    };
    if sketch.constraining {
        return std::iter::once(constrain)
            .chain(std::iter::once(separator()))
            .chain(fitting())
            .collect();
    }
    // With anything selected, and no drawing tool, likewise the
    // constraints that fit it, and with splines among it switching
    // them, handles, the curvature comb and adding points before
    // them; while adding points, Add point stays, to stop it.
    if (keys.geometry_selected || keys.inserting) && !keys.drawing {
        let insert = || {
            let binding = insert_point_binding(keys);
            tipped_op(Icon::InsertPoint, "Add point", binding, keys.inserting)
        };
        let splines = keys.splines_selected.then(|| {
            [
                tipped_op(Icon::Convert, "Convert", switch_binding(keys), false),
                tipped_op(Icon::Handles, "Handles", handles_binding(keys), false),
                tipped_op(Icon::Comb, "Comb", comb_binding(keys), sketch.comb),
            ]
        });
        let insert = (keys.splines_selected || keys.inserting).then(insert);
        // Spline points selected: taking them out, or why it can't.
        let remove = (keys.removing)
            .zip(crate::spline::removals(sketch.sketch, sketch.selection))
            .map(|(_, found)| remove_op(found.err(), remove_point_binding(keys)));
        // Closing or opening, for arcs as for splines.
        let close = keys
            .closing
            .map(|close| close_op(close, close_binding(keys)));
        let curves = (splines.into_iter().flatten())
            .chain(insert)
            .chain(remove)
            .chain(close)
            .collect::<Vec<_>>();
        let gap = (!curves.is_empty()).then(separator);
        return curves
            .into_iter()
            .chain(gap)
            .chain([constrain, separator()])
            .chain(fitting())
            .collect();
    }
    // The tools the mock's sketch bar has, in its order; the rest are
    // on the rail (all of them, with their keys), which left no room
    // at 1280 px wide. Their keys are in their tooltips, as the
    // mock's bar has them at that width, not beside them: with a
    // tool's tag beside the sketch's name they'd run past the bar.
    let active = sketch.tool.map(|tool| tool.tool);
    let tools = Tool::BAR.map(|tool| {
        tipped_op(
            tool.icon(),
            tool.label(),
            tool_binding(tool, keys),
            active == Some(tool),
        )
    });
    // Constrain last, as the mock's bar has it, and Close after it
    // while the Spline or Line tool's shape can close.
    let close = keys
        .tool_closes
        .then(|| [separator(), close_op(true, close_binding(keys))]);
    (tools.into_iter().chain([constrain]))
        .chain(close.into_iter().flatten())
        .collect()
}

/// The bar while picking a new sketch's plane: Sketch, which backs out,
/// and the origin planes.
fn plane_pick_ops<'a>(state: &DocumentState<'a>, keys: DocumentKeys) -> Vec<Element<'a, Message>> {
    let sketch = bar_op(state, keys, Entry::Sketch);
    // Picking a plane in the viewport comes with picking, so the
    // origin planes are offered here.
    let planes = OriginPlane::ALL.map(|plane| {
        bound_op(
            icons::plane_icon(plane),
            plane_label(plane),
            plane_binding(plane, keys),
            false,
        )
    });
    // Only Sketch, which backs out: Extrude and Revolve take no plane.
    [sketch, separator()].into_iter().chain(planes).collect()
}

/// The origin buttons while picking a move's or pattern's axis or a
/// mirror's plane, after a separator: the viewport picks the model's
/// edges and faces, so the origin ones are offered here, as picking a
/// sketch's plane does.
fn motion_origins<'a>(state: &DocumentState<'a>) -> Vec<Element<'a, Message>> {
    let editable = state.editable();
    match state.motion() {
        Some(motion) => {
            let send = |look: MotionLook| {
                (editable && motion.editable).then_some(Message::Look(Look::Motion(look)))
            };
            let origin = || vec![op(Icon::Point, "Origin", send(MotionLook::OriginPoint))];
            let axes = || {
                (Axis3::ALL.iter())
                    .map(|&axis| {
                        op(
                            Icon::SeAxis,
                            axis_label(axis),
                            send(MotionLook::OriginAxis(axis)),
                        )
                    })
                    .collect()
            };
            let split_face = motion
                .split()
                .is_some_and(|split| split.mode == SplitMode::Face);
            let buttons: Vec<Element<'a, Message>> = match motion.picking {
                // A scale's point may be the origin.
                MotionPick::Point => origin(),
                // An align's target may be the origin, or an origin axis.
                MotionPick::Align(slot) if slot.side == AlignSide::Target => {
                    if slot.role == AlignRole::Point {
                        origin()
                    } else {
                        axes()
                    }
                }
                // A split's tool by face, or a reference that may be a plane.
                picking if motion.kind.picks_origin_planes(picking, split_face) => {
                    (OriginPlane::ALL.iter())
                        .map(|&plane| {
                            op(
                                icons::plane_icon(plane),
                                plane_label(plane),
                                send(MotionLook::OriginPlane(plane)),
                            )
                        })
                        .collect()
                }
                // A loft picks no reference.
                MotionPick::Reference if motion.kind.takes_axis() => axes(),
                _ => Vec::new(),
            };
            if buttons.is_empty() {
                buttons
            } else {
                std::iter::once(separator()).chain(buttons).collect()
            }
        }
        None => Vec::new(),
    }
}

/// The operations of [`BarItem`] by short names, for the lists of
/// [`selection_bar`].
mod bar_items {
    use super::{BarItem, Entry, MotionKind};

    pub(super) const SKETCH: BarItem = BarItem::Op(Entry::Sketch);
    pub(super) const EXTRUDE: BarItem = BarItem::Op(Entry::Extrude);
    pub(super) const REVOLVE: BarItem = BarItem::Op(Entry::Revolve);
    pub(super) const COMBINE: BarItem = BarItem::Op(Entry::Combine);
    pub(super) const SWEEP: BarItem = BarItem::Op(Entry::Motion(MotionKind::Sweep));
    pub(super) const LOFT: BarItem = BarItem::Op(Entry::Motion(MotionKind::Loft));
    pub(super) const OFFSET: BarItem = BarItem::Op(Entry::Motion(MotionKind::OffsetFace));
    pub(super) const FILLET: BarItem = BarItem::Op(Entry::Motion(MotionKind::Fillet));
    pub(super) const CHAMFER: BarItem = BarItem::Op(Entry::Motion(MotionKind::Chamfer));
    pub(super) const SHELL: BarItem = BarItem::Op(Entry::Motion(MotionKind::Shell));
    pub(super) const DRAFT: BarItem = BarItem::Op(Entry::Motion(MotionKind::Draft));
    pub(super) const MOVE: BarItem = BarItem::Op(Entry::Motion(MotionKind::Move));
    pub(super) const MIRROR: BarItem = BarItem::Op(Entry::Motion(MotionKind::Mirror));
    pub(super) const PATTERN: BarItem = BarItem::Op(Entry::Motion(MotionKind::LinearPattern));
    pub(super) const CIRCULAR_PATTERN: BarItem =
        BarItem::Op(Entry::Motion(MotionKind::CircularPattern));
    pub(super) const ALIGN: BarItem = BarItem::Op(Entry::Motion(MotionKind::Align));
    pub(super) const SCALE: BarItem = BarItem::Op(Entry::Motion(MotionKind::Scale));
    pub(super) const SPLIT: BarItem = BarItem::Op(Entry::Motion(MotionKind::Split));
}

/// What the toolbar offers for what's selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarItem {
    /// An operation, as the rail has it.
    Op(Entry),
    /// Editing the sketch whose items are selected.
    EditSketch(FeatureId),
}

/// The operations offered for what's selected, in place of the model
/// bar's, outside sketches and sessions; none for nothing selected, or
/// a selection of mixed kinds that takes none of them.
///
/// - Edges alone: Revolve while they're all straight (an axis), Pattern
///   for one straight edge (a direction), Fillet and Chamfer first and
///   Circular pattern for one round edge (a rim), with Sweep (a path).
/// - Faces alone: Sketch on face and Mirror while they're all flat,
///   with Offset, Fillet, Chamfer, Shell and Draft.
/// - Edges and faces: Fillet and Chamfer, taking both, first.
/// - Vertices: Align and Scale, which take a point.
/// - Bodies: the operations on bodies.
/// - A sketch selected in the Timeline: those making a solid of it
///   first.
fn selection_bar(state: &DocumentState<'_>) -> Option<Vec<BarItem>> {
    use bar_items::*;
    let picking = state.picking.as_ref()?;
    let selection = state.model_selection;
    let index = picking.index;
    let targets: Vec<Picked> = selection.targets().collect();
    let straight = |edge: u32| {
        (index.chain_keys(edge)).is_some_and(|keys| index.edge_ends(edge, &keys).is_some())
    };
    let summary = |face: u32| (index.picking().faces().get(face as usize)).map(|f| f.summary);
    let flat = |face: u32| matches!(summary(face), Some(Summary::Plane { .. }));
    let edges = || {
        targets.iter().filter_map(|target| match *target {
            Picked::Edge(edge) => Some(edge),
            _ => None,
        })
    };
    let faces = || {
        targets.iter().filter_map(|target| match *target {
            Picked::Face(face) => Some(face),
            _ => None,
        })
    };
    if selection.only_edges() {
        return Some(match targets[..] {
            [Picked::Edge(edge)] if !straight(edge) => {
                vec![FILLET, CHAMFER, SKETCH, SWEEP, CIRCULAR_PATTERN, MOVE]
            }
            [Picked::Edge(_)] => vec![SKETCH, REVOLVE, FILLET, CHAMFER, SWEEP, MOVE, PATTERN],
            _ if edges().all(straight) => vec![SKETCH, REVOLVE, FILLET, CHAMFER, SWEEP, MOVE],
            _ => vec![SKETCH, FILLET, CHAMFER, SWEEP, MOVE],
        });
    }
    if selection.only_faces() {
        let flat = faces().all(flat);
        let mut bar = Vec::new();
        bar.extend(flat.then_some(SKETCH));
        bar.extend([EXTRUDE, OFFSET, FILLET, CHAMFER, SHELL, DRAFT]);
        bar.push(MOVE);
        bar.extend(flat.then_some(MIRROR));
        return Some(bar);
    }
    if selection.edges_and_faces() {
        return Some(vec![FILLET, CHAMFER, SKETCH, MOVE]);
    }
    if selection.only_vertices() {
        return Some(vec![SKETCH, ALIGN, SCALE, MOVE]);
    }
    if selection.only_bodies() {
        return Some(vec![
            SKETCH, MOVE, MIRROR, PATTERN, COMBINE, SPLIT, SCALE, SHELL,
        ]);
    }
    let document = state.editor.document();
    if let Some(bar) = sketch_items_bar(state) {
        return Some(bar);
    }
    let sketch_selected = selection.is_empty()
        && (state.selected_feature.and_then(|id| document.feature(id)))
            .is_some_and(|feature| matches!(feature.kind, FeatureKind::Sketch { .. }));
    sketch_selected.then(|| {
        vec![
            EXTRUDE, REVOLVE, SWEEP, LOFT, SKETCH, CHAMFER, COMBINE, MOVE, PATTERN,
        ]
    })
}

/// The operations offered for the items of one sketch selected in the
/// model, as [`selection_bar`] lists them: for curves, those taking the
/// regions they bound or the chain they make (Revolve a line alone as
/// its axis, with no regions to extrude), and the sketch's editing.
fn sketch_items_bar(state: &DocumentState<'_>) -> Option<Vec<BarItem>> {
    use bar_items::*;
    let selection = state.model_selection;
    let mut items = selection.sketch_items().peekable();
    let sketch = items.peek()?.sketch;
    let Some(FeatureKind::Sketch { sketch: drawn, .. }) =
        (state.editor.document().feature(sketch)).map(|feature| &feature.kind)
    else {
        return None;
    };
    let curves: Option<Vec<&Curve>> = items
        .map(|item| {
            (item.sketch == sketch)
                .then(|| drawn.curve(item.item))
                .flatten()
                .map(|entry| &entry.curve)
        })
        .collect();
    let edit = BarItem::EditSketch(sketch);
    if selection.sketch_items().count() != selection.items().count() {
        return None;
    }
    Some(match curves.as_deref() {
        Some([Curve::Line { .. }]) => vec![REVOLVE, SWEEP, SPLIT, edit],
        Some(_) => vec![EXTRUDE, REVOLVE, SWEEP, SPLIT, edit],
        None => vec![edit],
    })
}

/// The toolbar's button for `entry`, with its key, lit while it's in
/// use, as the rail's.
fn bar_op(
    state: &DocumentState<'_>,
    keys: DocumentKeys,
    entry: Entry,
) -> Element<'static, Message> {
    bound_op(
        entry.icon(),
        entry.bar_label(keys),
        entry.binding(keys),
        entry.on(Using::of(state)),
    )
}

/// A short vertical separator between groups of operations.
fn separator<'a>() -> Element<'a, Message> {
    container(vrule()).height(18).padding([0, 4]).into()
}

/// A toolbar operation's button, sending `message`; disabled without one.
fn op(icon: Icon, label: &'static str, message: Option<Message>) -> Element<'static, Message> {
    op_button(icon, label, None, false, message)
}

/// A toolbar operation's button for `binding`, showing its key, and
/// highlighted while `on`.
fn bound_op(
    icon: Icon,
    label: &'static str,
    binding: Binding,
    on: bool,
) -> Element<'static, Message> {
    let message = binding.sends();
    op_button(icon, label, binding.shortcut, on, message)
}

/// A sketch toolbar's button for `binding`, highlighted while `on`, its
/// key in its tooltip ("Line (L)") rather than beside it.
fn tipped_op(
    icon: Icon,
    label: &'static str,
    binding: Binding,
    on: bool,
) -> Element<'static, Message> {
    let tip = binding.tip(label);
    tipped_op_with(icon, label, binding, on, tip)
}

/// [`tipped_op`] with `tip` for its tooltip.
fn tipped_op_with(
    icon: Icon,
    label: &'static str,
    binding: Binding,
    on: bool,
    tip: String,
) -> Element<'static, Message> {
    // As the mock's narrower buttons, without their keys beside them.
    let button = op_body(icon, label, None, on, binding.sends(), 7);
    crate::chrome::tip(button, text(tip))
}

fn op_button(
    icon: Icon,
    label: &'static str,
    key: Option<Shortcut>,
    on: bool,
    message: Option<Message>,
) -> Element<'static, Message> {
    op_body(icon, label, key, on, message, 8).into()
}

/// A toolbar button: `icon`, `label` and `key` in a row, `padding` either side.
fn op_body(
    icon: Icon,
    label: &'static str,
    key: Option<Shortcut>,
    on: bool,
    message: Option<Message>,
    padding: u16,
) -> Button<'static, Message> {
    button(
        row![
            icons::icon(icon, icons::INLINE),
            text(label),
            key.map(key_label)
        ]
        .spacing(6)
        .height(Length::Fill)
        .align_y(Alignment::Center),
    )
    .height(28)
    .padding([0, padding])
    .style(theme::flat_button(on, theme::Tone::Text))
    .on_press_maybe(message)
}

/// Undo's or Redo's button, telling its `label` and key while hovered.
fn history_button<'a>(icon: Icon, label: &str, binding: Binding) -> Element<'a, Message> {
    crate::chrome::tip(
        icon_button(icon, Tone::Muted, binding.sends()),
        text(binding.tip(label)),
    )
}

#[cfg(test)]
mod tests {

    use super::*;

    /// A name past the limit is cut on a character's edge, with "…";
    /// one at it is kept whole.
    #[test]
    fn long_names_are_cut_short() {
        assert_eq!(shortened("Chamfer 1", 4), "Cha…");
        assert_eq!(shortened("Chamfer", 7), "Chamfer");
        assert_eq!(shortened("Chamfer 12", 9), "Chamfer…");
        assert_eq!(shortened("ÆØÅæøå", 3), "ÆØ…");
        assert_eq!(shortened("", 3), "");
        assert_eq!(shortened("abc", 0), "…");
    }
}
