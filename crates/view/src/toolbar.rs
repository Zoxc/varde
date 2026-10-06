//! The document toolbar and its file menu.

use std::borrow::Cow;

use iced::widget::{
    Button, Container, Space, button, column, container, mouse_area, opaque, row, space, text,
    tooltip,
};
use iced::{Alignment, Element, Length, Padding, mouse};
use varde_document::{Axis3, EXTENSION, FeatureId, FeatureKind, OriginPlane, Tolerance};
use varde_expr::LengthUnit;
use varde_regen::Summary;
use varde_sketch::Curve;

use crate::chrome::{Edge, edged, hrule, icon_button, key_label, vrule};
use crate::icons::{self, Icon};
use crate::shortcut::{
    Binding, Shortcut, align_binding, chamfer_binding, circular_pattern_binding, comb_binding,
    combine_binding, constrain_binding, constraint_binding, draft_binding, extrude_binding,
    file_bindings, fillet_binding, handles_binding, history_bindings, loft_binding,
    measure_binding, mirror_binding, move_binding, offset_face_binding, pattern_binding,
    revolve_binding, scale_binding, shell_binding, sketch_binding, split_binding, sweep_binding,
    switch_binding, tool_binding,
};
use crate::theme::{self, Emphasis, SEMIBOLD, SIDE_PANEL_INNER_WIDTH, Tone};
use crate::{
    ActiveTool, AlignRole, AlignSide, ConstraintKind, DocumentState, Downloads, Edit, File,
    Location, Look, Message, MotionKind, MotionLook, MotionPick, NOT_SAVED, Overlay, Picked,
    SplitMode, Tool,
};

/// Includes the 1 px border.
const TOOLBAR_HEIGHT: f32 = 40.0;

/// The width of the Save button's cell, right of the file cell.
const SAVE_CELL_WIDTH: f32 = 34.0;

/// The bar's spacing between its items.
const BAR_SPACING: f32 = 2.0;

pub fn toolbar<'a>(state: &DocumentState<'a>) -> Element<'a, Message> {
    let operation = operation(state);
    let (context, tag): (Element<'a, Message>, _) = match (&state.sketch, &operation) {
        (Some(sketch), _) => (
            pill(
                Icon::Sketch,
                sketch.name,
                Some(Message::Look(Look::FinishSketch)),
                "Finish sketch (Esc)",
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
                state.measure.as_ref().map(|_| "Measure".to_owned())
            },
        ),
    };
    let tag = tag.map(|tag| {
        container(text(tag).size(11).font(SEMIBOLD))
            .padding([2, 7])
            .style(theme::tag)
    });

    let [undo, redo, _] = history_bindings(state.keys());
    let bar = row![
        // The rule right after the file cell, its wash meeting it.
        row![file_cell(state), vrule()],
        save_cell(state.editable(), state.edited),
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
        row(ops(state, operation.as_ref()))
            .spacing(2)
            .padding([0, 6])
            .align_y(Alignment::Center),
        // What's empty of the bar: a click on it clears the selection.
        mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
            .on_press(crate::panels::CLEAR_SELECTION),
        history_button(Icon::Undo, "Undo", undo),
        history_button(Icon::Redo, "Redo", redo),
        container(vrule()).height(18).padding([0, 4]),
        crate::chrome::theme_button(state.options.theme),
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
fn operation<'a>(state: &DocumentState<'a>) -> Option<Operation<'a>> {
    if let Some(motion) = &state.motion {
        return Some(Operation {
            icon: motion.kind.icon(),
            name: motion.title(),
            ok: motion.ready.then_some(Message::Edit(Edit::CommitMotion)),
            cancel: Message::Look(Look::Motion(MotionLook::Cancel)),
        });
    }
    if let Some(extrude) = &state.extrude {
        return Some(Operation {
            icon: Icon::Extrude,
            name: extrude.editing.unwrap_or("New extrude"),
            ok: extrude.ready.then_some(Message::Edit(Edit::CommitExtrude)),
            cancel: Message::Look(Look::Extrude(crate::ExtrudeLook::Cancel)),
        });
    }
    if let Some(revolve) = &state.revolve {
        return Some(Operation {
            icon: Icon::Revolve,
            name: revolve.editing.unwrap_or("New revolve"),
            ok: revolve.ready.then_some(Message::Edit(Edit::CommitRevolve)),
            cancel: Message::Look(Look::Revolve(crate::RevolveLook::Cancel)),
        });
    }
    let combine = state.combine.as_ref()?;
    Some(Operation {
        icon: Icon::Combine,
        name: combine.editing.unwrap_or("New combine"),
        ok: combine.ready.then_some(Message::Edit(Edit::CommitCombine)),
        cancel: Message::Look(Look::Combine(crate::CombineLook::Cancel)),
    })
}

/// The sketch being edited or the operation being set up, named with
/// `icon` on the soft accent, joined at its right end by the button
/// finishing it, sending `finish` (disabled without), which hovering
/// tells `tip`.
fn pill<'a>(
    icon: Icon,
    name: &'a str,
    finish: Option<Message>,
    tip: &'static str,
) -> Element<'a, Message> {
    let finish = button(
        container(icons::tinted(Icon::Check, icons::INLINE, |p| {
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
        text(format!("Save ({})", save.shortcut.label())),
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
    let dirty =
        (state.edited).then(|| container(Space::new().width(6).height(6)).style(theme::dirty_dot));
    let name: Element<'a, Message> = if state.unnamed {
        container(text(NOT_SAVED).size(11.5).font(SEMIBOLD))
            .padding([1, 8])
            .style(theme::name_pill)
            .into()
    } else {
        row![
            text(state.name).font(SEMIBOLD),
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
    match (state.location, state.path) {
        (Some(location), _) if !open => {
            let (said, kept, why) = location_told(location);
            lines.push(text(said).size(12).font(SEMIBOLD).into());
            lines.push(
                text(format!("{}.{EXTENSION}, {kept}", state.name))
                    .size(12)
                    .into(),
            );
            lines.push(text(why).size(12).into());
        }
        (None, Some(path)) if !open => lines.push(text(path).size(12).into()),
        _ => {}
    }
    if state.edited && !open {
        lines.push(text("Changes not saved").size(12).into());
    }
    if lines.is_empty() {
        return cell.into();
    }
    tooltip(
        cell,
        container(column(lines).spacing(1))
            .padding([4, 7])
            .max_width(theme::SIDE_PANEL_WIDTH * 1.5)
            .style(theme::menu),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .snap_within_viewport(true)
    .into()
}

/// Where a design is kept, on the web, under the file cell: its icon and
/// the words, in the location's colour on the panel's.
pub(crate) fn location_bar<'a>(location: Location) -> Element<'a, Message> {
    let (icon, said) = match location {
        Location::Browser => (Icon::Browser, "In browser storage"),
        Location::Computer => (Icon::Computer, "On your computer"),
    };
    let color = move |p: &theme::Palette| theme::location_ink(p, location == Location::Computer);
    edged(
        container(
            row![
                icons::tinted(icon, 13.0, color),
                text(said)
                    .size(11.5)
                    .font(SEMIBOLD)
                    .style(move |theme: &iced::Theme| {
                        iced::widget::text::Style {
                            color: Some(color(theme::palette(theme))),
                        }
                    }),
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
/// web: where, how it's kept and what that means (a file picked on the
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

/// The icon of the drawing tool `tool`.
pub(crate) fn tool_icon(tool: Tool) -> Icon {
    match tool {
        Tool::Line => Icon::Line,
        Tool::Circle => Icon::Circle,
        Tool::Arc => Icon::Arc,
        Tool::Point => Icon::Point,
        Tool::Rectangle => Icon::Rectangle,
        Tool::Polygon => Icon::Polygon,
        Tool::Spline => Icon::Spline,
        Tool::Dimension => Icon::Dimension,
        Tool::Trim => Icon::Trim,
        Tool::Extend => Icon::Extend,
        Tool::Mirror => Icon::Mirror,
        Tool::Offset => Icon::Offset,
        Tool::Fillet => Icon::Fillet,
        Tool::Chamfer => Icon::Chamfer,
        Tool::Project => Icon::Project,
        Tool::Intersect => Icon::Intersect,
    }
}

/// The operations for what's going on: modelling, picking the plane for a
/// new sketch, or editing a sketch. While an `operation` is set up, only
/// Cancel and what it picks, its OK being in the pill.
fn ops<'a>(
    state: &DocumentState<'a>,
    operation: Option<&Operation<'a>>,
) -> Vec<Element<'a, Message>> {
    let editable = state.editable();
    let keys = state.keys();
    if let Some(sketch) = &state.sketch {
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
        // them, handles and the curvature comb before them.
        if keys.geometry_selected && !keys.drawing {
            let splines = keys.splines_selected.then(|| {
                [
                    tipped_op(Icon::Convert, "Convert", switch_binding(keys), false),
                    tipped_op(Icon::Handles, "Handles", handles_binding(keys), false),
                    tipped_op(Icon::Comb, "Comb", comb_binding(keys), sketch.comb),
                    separator(),
                ]
            });
            return (splines.into_iter().flatten())
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
                tool_icon(tool),
                tool.label(),
                tool_binding(tool, keys),
                active == Some(tool),
            )
        });
        // Constrain last, as the mock's bar has it.
        return tools.into_iter().chain([constrain]).collect();
    }
    let sketch = bound_op(
        Icon::Sketch,
        if keys.face_selected {
            "Sketch on face"
        } else {
            "Sketch"
        },
        sketch_binding(keys),
        state
            .picking_plane
            .is_some_and(|pick| pick.sketch.is_none()),
    );
    let extrude = bound_op(
        Icon::Extrude,
        "Extrude",
        extrude_binding(keys),
        state.extrude.is_some(),
    );
    let revolve = bound_op(
        Icon::Revolve,
        "Revolve",
        revolve_binding(keys),
        state.revolve.is_some(),
    );
    if state.picking_plane.is_some() {
        // Picking a plane in the viewport comes with picking, so the
        // origin planes are offered here.
        let planes = OriginPlane::ALL.map(|plane| {
            op(
                plane_icon(plane),
                plane_label(plane),
                editable.then_some(Message::Edit(Edit::PlanePicked(plane))),
            )
        });
        // Only Sketch, which backs out: Extrude and Revolve take no plane.
        return [sketch, separator()].into_iter().chain(planes).collect();
    }
    // Chamfer after the solids, before Combine, as the mock's model bar
    // orders them (its Hole isn't built). The mock's Fillet, before
    // Chamfer, and Shell, after it, would each leave the bar too wide at
    // 1280 px (Fillet to 1210 px of 1160): the rail's Modify set has
    // them, and Offset face too, and Fillet has its key.
    let moving = state.motion.as_ref().map(|motion| motion.kind);
    let chamfer = bound_op(
        Icon::BChamfer,
        "Chamfer",
        chamfer_binding(keys),
        moving == Some(MotionKind::Chamfer),
    );
    // After the solids, as the mock orders them.
    let combine = bound_op(
        Icon::Combine,
        "Combine",
        combine_binding(keys),
        state.combine.is_some(),
    );
    // Move after Combine, as the mock's model bar orders them. Mirror
    // isn't on it (the mock's model bar has none: its body bar does),
    // which left no room at 1280 px wide: the rail's Transform set has
    // it.
    let move_op = bound_op(
        Icon::Move,
        "Move",
        move_binding(keys),
        moving == Some(MotionKind::Move),
    );
    // The linear pattern after Move, with its key, as the mock's model
    // bar has it: the circular one is on the rail (the mock's model bar
    // has none either).
    let pattern_op = bound_op(
        Icon::LPattern,
        "Pattern",
        pattern_binding(keys),
        moving == Some(MotionKind::LinearPattern),
    );
    // Picking a move's or pattern's axis or a mirror's plane offers the
    // origin ones here, as picking a sketch's plane does: the viewport
    // picks the model's edges and faces.
    let origins: Vec<Element<'a, Message>> = match &state.motion {
        // An align's target may be the origin, or an origin axis.
        Some(motion) if matches!(motion.picking, MotionPick::Align(slot) if slot.side == AlignSide::Target) =>
        {
            let send = |look: MotionLook| {
                (editable && motion.editable).then_some(Message::Look(Look::Motion(look)))
            };
            let buttons: Vec<Element<'a, Message>> = match motion.picking {
                MotionPick::Align(slot) if slot.role == AlignRole::Point => {
                    vec![op(Icon::Point, "Origin", send(MotionLook::OriginPoint))]
                }
                _ => (Axis3::ALL.iter())
                    .map(|&axis| {
                        op(
                            Icon::SeAxis,
                            axis_label(axis),
                            send(MotionLook::OriginAxis(axis)),
                        )
                    })
                    .collect(),
            };
            std::iter::once(separator()).chain(buttons).collect()
        }
        // A scale's point may be the origin.
        Some(motion) if motion.picking == MotionPick::Point => {
            let send = (editable && motion.editable)
                .then_some(Message::Look(Look::Motion(MotionLook::OriginPoint)));
            vec![separator(), op(Icon::Point, "Origin", send)]
        }
        // A split's tool may be an origin plane.
        Some(motion)
            if motion.picking == MotionPick::Tool
                && (motion.split.as_ref()).is_some_and(|split| split.mode == SplitMode::Face) =>
        {
            let buttons = (OriginPlane::ALL.iter()).map(|&plane| {
                let send = (editable && motion.editable)
                    .then_some(Message::Look(Look::Motion(MotionLook::OriginPlane(plane))));
                op(plane_icon(plane), plane_label(plane), send)
            });
            std::iter::once(separator()).chain(buttons).collect()
        }
        Some(motion) if motion.picking == MotionPick::Reference => {
            let send = |look: MotionLook| {
                (editable && motion.editable).then_some(Message::Look(Look::Motion(look)))
            };
            let buttons: Vec<Element<'a, Message>> = match motion.kind {
                // A loft picks no reference.
                MotionKind::Loft => Vec::new(),
                MotionKind::Move
                | MotionKind::LinearPattern
                | MotionKind::CircularPattern
                | MotionKind::Sweep => (Axis3::ALL.iter())
                    .map(|&axis| {
                        op(
                            Icon::SeAxis,
                            axis_label(axis),
                            send(MotionLook::OriginAxis(axis)),
                        )
                    })
                    .collect(),
                MotionKind::Mirror
                | MotionKind::Align
                | MotionKind::Scale
                | MotionKind::Split
                | MotionKind::Chamfer
                | MotionKind::Shell
                | MotionKind::Fillet
                | MotionKind::OffsetFace
                | MotionKind::Draft => (OriginPlane::ALL.iter())
                    .map(|&plane| {
                        op(
                            plane_icon(plane),
                            plane_label(plane),
                            send(MotionLook::OriginPlane(plane)),
                        )
                    })
                    .collect(),
            };
            std::iter::once(separator()).chain(buttons).collect()
        }
        _ => Vec::new(),
    };
    // Measure after a separator, as the mock has it.
    let measure = bound_op(
        Icon::Measure,
        "Measure",
        measure_binding(keys),
        state.measure.is_some(),
    );
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
        return std::iter::once(cancel).chain(origins).collect();
    }
    // What's selected decides the operations offered, each starting
    // with it ([`selection_bar`]).
    if let Some(bar) = selection_bar(state) {
        return (bar.into_iter())
            .map(|kind| bar_op(state, kind))
            .chain([separator(), measure])
            .collect();
    }
    [
        sketch,
        extrude,
        revolve,
        chamfer,
        combine,
        move_op,
        pattern_op,
        separator(),
        measure,
    ]
    .into_iter()
    .chain(origins)
    .collect()
}

/// An operation the toolbar offers for what's selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarOp {
    Sketch,
    Extrude,
    Revolve,
    Sweep,
    Loft,
    Offset,
    Fillet,
    Chamfer,
    Shell,
    Draft,
    Combine,
    Move,
    Mirror,
    Pattern,
    CircularPattern,
    Align,
    Scale,
    Split,
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
fn selection_bar(state: &DocumentState<'_>) -> Option<Vec<BarOp>> {
    use BarOp::*;
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
                vec![Fillet, Chamfer, Sketch, Sweep, CircularPattern, Move]
            }
            [Picked::Edge(_)] => vec![Sketch, Revolve, Fillet, Chamfer, Sweep, Move, Pattern],
            _ if edges().all(straight) => vec![Sketch, Revolve, Fillet, Chamfer, Sweep, Move],
            _ => vec![Sketch, Fillet, Chamfer, Sweep, Move],
        });
    }
    if selection.only_faces() {
        let flat = faces().all(flat);
        let mut bar = Vec::new();
        bar.extend(flat.then_some(Sketch));
        bar.extend([Extrude, Offset, Fillet, Chamfer, Shell, Draft]);
        bar.push(Move);
        bar.extend(flat.then_some(Mirror));
        return Some(bar);
    }
    if selection.edges_and_faces() {
        return Some(vec![Fillet, Chamfer, Sketch, Move]);
    }
    if selection.only_vertices() {
        return Some(vec![Sketch, Align, Scale, Move]);
    }
    if selection.only_bodies() {
        return Some(vec![
            Sketch, Move, Mirror, Pattern, Combine, Split, Scale, Shell,
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
            Extrude, Revolve, Sweep, Loft, Sketch, Chamfer, Combine, Move, Pattern,
        ]
    })
}

/// The operations offered for the items of one sketch selected in the
/// model, as [`selection_bar`] lists them: for curves, those taking the
/// regions they bound or the chain they make (Revolve a line alone as
/// its axis, with no regions to extrude), and the sketch's editing.
fn sketch_items_bar(state: &DocumentState<'_>) -> Option<Vec<BarOp>> {
    use BarOp::*;
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
    let edit = EditSketch(sketch);
    if selection.sketch_items().count() != selection.items().count() {
        return None;
    }
    Some(match curves.as_deref() {
        Some([Curve::Line { .. }]) => vec![Revolve, Sweep, Split, edit],
        Some(_) => vec![Extrude, Revolve, Sweep, Split, edit],
        None => vec![edit],
    })
}

/// The toolbar's button for `kind`, with its key, lit while it's set up.
fn bar_op(state: &DocumentState<'_>, kind: BarOp) -> Element<'static, Message> {
    let keys = state.keys();
    let moving = state.motion.as_ref().map(|motion| motion.kind);
    let motion = |icon, label, binding, kind| bound_op(icon, label, binding, moving == Some(kind));
    match kind {
        BarOp::Sketch => bound_op(
            Icon::Sketch,
            if keys.face_selected {
                "Sketch on face"
            } else {
                "Sketch"
            },
            sketch_binding(keys),
            false,
        ),
        BarOp::Extrude => bound_op(
            Icon::Extrude,
            "Extrude",
            extrude_binding(keys),
            state.extrude.is_some(),
        ),
        BarOp::Revolve => bound_op(
            Icon::Revolve,
            "Revolve",
            revolve_binding(keys),
            state.revolve.is_some(),
        ),
        BarOp::Combine => bound_op(
            Icon::Combine,
            "Combine",
            combine_binding(keys),
            state.combine.is_some(),
        ),
        BarOp::Sweep => motion(Icon::Sweep, "Sweep", sweep_binding(keys), MotionKind::Sweep),
        BarOp::Loft => motion(Icon::Loft, "Loft", loft_binding(keys), MotionKind::Loft),
        BarOp::Offset => motion(
            Icon::OffsetFace,
            "Offset",
            offset_face_binding(keys),
            MotionKind::OffsetFace,
        ),
        BarOp::Fillet => motion(
            Icon::BFillet,
            "Fillet",
            fillet_binding(keys),
            MotionKind::Fillet,
        ),
        BarOp::Chamfer => motion(
            Icon::BChamfer,
            "Chamfer",
            chamfer_binding(keys),
            MotionKind::Chamfer,
        ),
        BarOp::Shell => motion(Icon::Shell, "Shell", shell_binding(keys), MotionKind::Shell),
        BarOp::Draft => motion(Icon::Draft, "Draft", draft_binding(keys), MotionKind::Draft),
        BarOp::Move => motion(Icon::Move, "Move", move_binding(keys), MotionKind::Move),
        BarOp::Mirror => motion(
            MotionKind::Mirror.icon(),
            "Mirror",
            mirror_binding(keys),
            MotionKind::Mirror,
        ),
        BarOp::Pattern => motion(
            Icon::LPattern,
            "Pattern",
            pattern_binding(keys),
            MotionKind::LinearPattern,
        ),
        BarOp::CircularPattern => motion(
            MotionKind::CircularPattern.icon(),
            "Circular pattern",
            circular_pattern_binding(keys),
            MotionKind::CircularPattern,
        ),
        BarOp::Align => motion(Icon::Align, "Align", align_binding(keys), MotionKind::Align),
        BarOp::Scale => motion(Icon::Scale, "Scale", scale_binding(keys), MotionKind::Scale),
        BarOp::Split => motion(Icon::Split, "Split", split_binding(keys), MotionKind::Split),
        BarOp::EditSketch(sketch) => op(
            Icon::Sketch,
            "Edit sketch",
            state
                .editable()
                .then_some(Message::Look(Look::EditFeature(sketch))),
        ),
    }
}

/// Whether the toolbar offers the origin planes now: picking a sketch's
/// plane, a split's tool by face, or a reference that may be a plane. The
/// viewport shows them then, whether Objects has them shown or not.
pub(crate) fn picks_origin_planes(state: &DocumentState<'_>) -> bool {
    let Some(motion) = &state.motion else {
        return state.picking_plane.is_some();
    };
    let split_face = (motion.split.as_ref()).is_some_and(|split| split.mode == SplitMode::Face);
    motion_picks_origin_planes(motion.kind, motion.picking, split_face)
}

/// Whether an operation of `kind` picking `picking` takes an origin
/// plane, offered on the toolbar and clicked in the viewport: a split's
/// tool by face (`split_face`), or a reference that may be a plane.
pub fn motion_picks_origin_planes(kind: MotionKind, picking: MotionPick, split_face: bool) -> bool {
    match picking {
        MotionPick::Tool => split_face,
        MotionPick::Reference => matches!(
            kind,
            MotionKind::Mirror
                | MotionKind::Align
                | MotionKind::Scale
                | MotionKind::Split
                | MotionKind::Chamfer
                | MotionKind::Shell
                | MotionKind::Fillet
                | MotionKind::OffsetFace
                | MotionKind::Draft
        ),
        _ => false,
    }
}

/// The label of the button turning a move, or patterning, about `axis`.
fn axis_label(axis: Axis3) -> &'static str {
    match axis {
        Axis3::X => "X axis",
        Axis3::Y => "Y axis",
        Axis3::Z => "Z axis",
    }
}

/// The icon of `plane`, on its buttons and its row in Objects.
pub(crate) fn plane_icon(plane: OriginPlane) -> Icon {
    match plane {
        OriginPlane::XY => Icon::PlaneXy,
        OriginPlane::XZ => Icon::PlaneXz,
        OriginPlane::YZ => Icon::PlaneYz,
    }
}

/// The label of the button making a sketch on `plane`.
fn plane_label(plane: OriginPlane) -> &'static str {
    match plane {
        OriginPlane::XY => "XY plane",
        OriginPlane::XZ => "XZ plane",
        OriginPlane::YZ => "YZ plane",
    }
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
    let key = Some(binding.shortcut).filter(|shortcut| !shortcut.is_none());
    op_button(icon, label, key, on, message)
}

/// A sketch toolbar's button for `binding`, highlighted while `on`, its
/// key in its tooltip ("Line (L)") rather than beside it.
fn tipped_op(
    icon: Icon,
    label: &'static str,
    binding: Binding,
    on: bool,
) -> Element<'static, Message> {
    let tip = if binding.shortcut.is_none() {
        label.to_owned()
    } else {
        format!("{label} ({})", binding.shortcut.label())
    };
    let button = button(
        row![icons::icon(icon, icons::INLINE), text(label)]
            .spacing(6)
            .height(Length::Fill)
            .align_y(Alignment::Center),
    )
    .height(28)
    // As the mock's narrower buttons, without their keys beside them.
    .padding([0, 7])
    .style(theme::flat_button(on, theme::Tone::Text))
    .on_press_maybe(binding.sends());
    crate::chrome::tip(button, text(tip))
}

fn op_button(
    icon: Icon,
    label: &'static str,
    key: Option<Shortcut>,
    on: bool,
    message: Option<Message>,
) -> Element<'static, Message> {
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
    .padding([0, 8])
    .style(theme::flat_button(on, theme::Tone::Text))
    .on_press_maybe(message)
    .into()
}

/// The design units the file menu offers, with their names.
const UNITS: [(LengthUnit, &str); 2] =
    [(LengthUnit::Mm, "Millimetres"), (LengthUnit::In, "Inches")];

/// The tolerances the file menu offers, by fit tolerance in mm, with
/// their names.
const TOLERANCES: [(f64, &str); 3] = [(1e-4, "0.1 µm"), (1e-3, "1 µm"), (1e-2, "10 µm")];

/// The name of the fit tolerance `fit`, in mm, in micrometres to three
/// decimals without trailing zeros: "0.05 µm", "25 µm".
fn tolerance_label(fit: f64) -> String {
    let mut text = format!("{:.3}", fit * 1000.0);
    let kept = text.trim_end_matches('0').trim_end_matches('.').len();
    text.truncate(kept);
    format!("{text} µm")
}

/// The tolerances the file menu lists for a design of `tolerance`, by
/// fit tolerance in mm, with their names and whether they're ticked: the
/// ones offered, and `tolerance` unticked after them if it isn't one,
/// under a name none of them has.
fn tolerance_choices(tolerance: Tolerance) -> Vec<(f64, String, bool)> {
    let fit = tolerance.fit();
    let listed = TOLERANCES.iter().any(|&(offered, _)| offered == fit);
    let offered = TOLERANCES.map(|(offered, label)| (offered, label.to_owned(), offered == fit));
    let other = (!listed).then(|| {
        // Rounded as it's named, it may look like one offered: then it's
        // named exactly, in millimetres.
        let label = tolerance_label(fit);
        let taken = TOLERANCES.iter().any(|&(_, offered)| offered == label);
        let label = if taken { format!("{fit} mm") } else { label };
        (fit, label, false)
    });
    offered.into_iter().chain(other).collect()
}

/// A menu's item: `icon`, `label` and the `key` that does the same if
/// any, sending `message`; disabled without one.
pub(crate) fn menu_item(
    icon: Icon,
    label: Cow<'static, str>,
    key: Option<Shortcut>,
    message: Option<Message>,
) -> Button<'static, Message> {
    let key = key.map(|key| key_label(key).into());
    menu_row(icon, label, key, message)
}

/// A menu's item that's one of a set of choices with icons of their own:
/// `icon` and `label`, sending `message`, with a tick at its right while
/// `chosen`.
pub(crate) fn choice_item(
    icon: Icon,
    label: &'static str,
    chosen: bool,
    message: Message,
) -> Button<'static, Message> {
    let tick = chosen.then(|| {
        icons::tinted(Icon::Check, icons::INLINE, |p| {
            theme::flat_content(p, Tone::Text, true, false)
        })
        .into()
    });
    menu_row(icon, label.into(), tick, Some(message))
}

/// A menu's item opening a submenu to its left: `icon` and `label`, with a
/// chevron at its right pointing to where it opens, sending `message`;
/// lit while `open`.
pub(crate) fn submenu_item(
    icon: Icon,
    label: &'static str,
    open: bool,
    message: Message,
) -> Button<'static, Message> {
    let chevron = icons::tinted(Icon::ChevLeft, icons::INLINE, |p| {
        theme::flat_content(p, Tone::Muted, true, false)
    })
    .into();
    menu_row(icon, label.into(), Some(chevron), Some(message))
        .style(theme::flat_button(open, theme::Tone::Text))
}

/// A menu's item: `icon`, `label` and what's at its `right` if anything,
/// sending `message`; disabled without one.
fn menu_row(
    icon: Icon,
    label: Cow<'static, str>,
    right: Option<Element<'static, Message>>,
    message: Option<Message>,
) -> Button<'static, Message> {
    let enabled = message.is_some();
    let key = right.map(|right| container(right).align_right(Length::Fill));
    button(
        row![
            // In its own colours, or text-toned, so hovering doesn't change
            // it; faint while disabled.
            if enabled && icon.category().is_some() {
                icons::icon(icon, icons::INLINE)
            } else {
                icons::tinted(icon, icons::INLINE, move |p| {
                    theme::flat_content(p, Tone::Text, enabled, false)
                })
                .into()
            },
            text(label),
            key,
        ]
        .spacing(MENU_ITEM_SPACING)
        .height(Length::Fill)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(MENU_ITEM_HEIGHT)
    .padding([0, 8])
    .style(theme::flat_button(false, theme::Tone::Text))
    .on_press_maybe(message)
}

/// How tall a menu's item is, in pixels.
pub(crate) const MENU_ITEM_HEIGHT: f32 = 28.0;

/// The space between a menu item's icon and its label.
const MENU_ITEM_SPACING: f32 = 10.0;

/// A line between a menu's groups of items.
pub(crate) fn menu_separator<'a>() -> Container<'a, Message> {
    container(hrule()).padding([4, 2])
}

/// The icon left of a menu item that's a choice: a tick while `on`.
pub(crate) fn ticked(on: bool) -> Icon {
    if on { Icon::Check } else { Icon::Blank }
}

/// Undo's or Redo's button, telling its `label` and key while hovered.
fn history_button<'a>(icon: Icon, label: &str, binding: Binding) -> Element<'a, Message> {
    crate::chrome::tip(
        icon_button(icon, Tone::Muted, binding.sends()),
        text(format!("{label} ({})", binding.shortcut.label())),
    )
}

/// What the file menu shows, see [`file_menu`].
pub struct FileMenu {
    pub editable: bool,
    pub edited: bool,
    pub exportable: bool,
    pub units: LengthUnit,
    pub tolerance: Tolerance,
    /// Where it stands against its downloads, for a design in browser
    /// storage: the menu starts with it.
    pub downloads: Option<Downloads>,
    /// Whether it offers Download: on the web.
    pub downloadable: bool,
    /// For a design in browser storage, it offers Rename…: whether the
    /// design may be renamed now.
    pub rename: Option<bool>,
}

/// The file menu, as a layer over the whole screen. Clicking outside the
/// menu closes it. Save, and changing the design's `units` or its
/// `tolerance`, are disabled unless the document is `editable`, and Save
/// unless it's `edited` too. Export 3MF is disabled unless `exportable`.
/// A tolerance the menu doesn't offer, from a file, shows unticked. On
/// the web it offers Download, and for a design in browser storage
/// Rename…, starting with where it stands against its downloads.
pub fn file_menu(state: FileMenu) -> Element<'static, Message> {
    let FileMenu {
        editable,
        edited,
        exportable,
        units,
        tolerance,
        downloads,
        downloadable,
        rename,
    } = state;
    let item = menu_item;
    let separator = menu_separator;

    let bound = |icon, label: &'static str, binding: Binding| {
        let message = binding.sends();
        item(icon, label.into(), Some(binding.shortcut), message)
    };

    let [save, save_as] = file_bindings(editable, edited);
    // No key: exporting is rare, and the tool rail has no tool for it.
    let export = exportable.then_some(Message::File(File::Export));
    let rename = rename.map(|enabled| {
        item(
            Icon::Rename,
            "Rename…".into(),
            None,
            enabled.then_some(Message::File(File::Rename)),
        )
    });
    let download = downloadable.then(|| {
        item(
            Icon::Download,
            "Download".into(),
            None,
            Some(Message::File(File::Download)),
        )
    });
    // Its dot in line with the items' icons.
    let downloads = downloads.as_ref().map(|downloads| {
        let (said, color) = crate::welcome::downloads_said(downloads);
        column![
            container(
                row![
                    container(container(Space::new().width(7).height(7)).style(theme::dot(color)))
                        .center_x(icons::INLINE),
                    text(said).size(11.5).style(theme::muted_text),
                ]
                .spacing(MENU_ITEM_SPACING)
                .align_y(Alignment::Center),
            )
            .padding([6, 8]),
            separator(),
        ]
    });
    let saving = column![
        downloads,
        bound(Icon::Save, "Save", save),
        bound(Icon::Save, "Save As…", save_as),
        rename,
        download,
        separator(),
        item(Icon::Export, "Export 3MF…".into(), None, export),
        separator(),
    ];
    // A new design starts in millimetres; its units are chosen here, and
    // changed here later.
    let heading = |label| {
        container(
            text(label)
                .size(11.5)
                .font(SEMIBOLD)
                .style(theme::muted_text),
        )
        .padding([4, 8])
    };
    let choices = UNITS.map(|(unit, label)| {
        let message = editable.then_some(Message::Edit(Edit::SetUnits(unit)));
        item(ticked(unit == units), label.into(), None, message).into()
    });
    // How closely curved surfaces are fitted, which is rarely changed.
    let tolerances = tolerance_choices(tolerance)
        .into_iter()
        .map(|(fit, label, on)| {
            let message = Tolerance::new(fit)
                .filter(|_| editable)
                .map(|tolerance| Message::Edit(Edit::SetTolerance(tolerance)));
            item(ticked(on), label.into(), None, message).into()
        });
    let menu = container(
        column![
            saving,
            heading("Units"),
            column(choices),
            heading("Tolerance"),
            column(tolerances),
            separator(),
            item(
                Icon::Close,
                "Close document".into(),
                None,
                Some(Message::File(File::CloseDocument))
            ),
        ]
        .width(232),
    )
    .padding(4)
    .style(theme::menu);

    mouse_area(
        container(opaque(menu))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding::ZERO.top(TOOLBAR_HEIGHT + 4.0).left(8)),
    )
    .interaction(mouse::Interaction::Idle)
    .on_press(Message::Look(Look::CloseFileMenu))
    .on_right_press(Message::Look(Look::CloseFileMenu))
    .into()
}

#[cfg(test)]
mod tests {
    use iced::advanced::{Layout, Shell, clipboard};
    use iced::{Event, Point, Rectangle, Size};

    use super::*;
    use crate::testing::Laid;

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

    /// The file menu as natively, `exportable` or not.
    fn native_menu(exportable: bool) -> FileMenu {
        FileMenu {
            editable: true,
            edited: true,
            exportable,
            units: LengthUnit::Mm,
            tolerance: Tolerance::DEFAULT,
            downloads: None,
            downloadable: false,
            rename: None,
        }
    }

    /// What clicking the file menu's item `label` sends, the menu laid out
    /// over a 800 × 600 window, `exportable` or not.
    fn click_file_menu(label: &str, exportable: bool) -> Vec<Message> {
        click_in_menu(native_menu(exportable), label)
    }

    /// What clicking the item `label` of the file menu `menu` sends.
    fn click_in_menu(menu: FileMenu, label: &str) -> Vec<Message> {
        let window = Size::new(800.0, 600.0);
        let menu = file_menu(menu);
        let mut laid = Laid::new(menu, window);
        let shown = laid.texts();
        let item = shown
            .iter()
            .find(|shown| shown.text == label)
            .unwrap_or_else(|| panic!("no {label:?} in {shown:?}"));
        let at = item.bounds.center();
        let mut messages = Vec::new();
        for event in [mouse::Event::ButtonPressed, mouse::Event::ButtonReleased] {
            let mut shell = Shell::new(&mut messages);
            laid.element.as_widget_mut().update(
                &mut laid.tree,
                &Event::Mouse(event(mouse::Button::Left)),
                Layout::new(&laid.node),
                mouse::Cursor::Available(Point::new(at.x, at.y)),
                &laid.renderer,
                &mut clipboard::Null,
                &mut shell,
                &Rectangle::with_size(window),
            );
        }
        messages
    }

    /// On the web the menu offers Download and, for a design in browser
    /// storage, starts with where it stands against its downloads and
    /// offers Rename…; natively none of that. Where the design is kept is
    /// the bar under the file cell's to say, not the menu's.
    #[test]
    fn on_the_web_the_file_menu_says_where_the_design_stands_against_its_downloads() {
        let web = || FileMenu {
            downloads: Some(Downloads::Latest(Some("just now".to_owned()))),
            downloadable: true,
            rename: Some(true),
            ..native_menu(false)
        };
        let shown = crate::testing::Laid::new(file_menu(web()), Size::new(800.0, 600.0)).texts();
        let first = shown
            .iter()
            .min_by(|a, b| a.bounds.y.total_cmp(&b.bounds.y));
        assert_eq!(
            first.map(|shown| shown.text.as_str()),
            Some("Latest downloaded just now"),
            "{shown:?}"
        );
        assert!(!shown.iter().any(|shown| shown.text == "In browser storage"));
        let sent = click_in_menu(web(), "Download");
        assert!(
            matches!(sent[..], [Message::File(File::Download)]),
            "{sent:?}"
        );
        let sent = click_in_menu(web(), "Rename…");
        assert!(
            matches!(sent[..], [Message::File(File::Rename)]),
            "{sent:?}"
        );
        // Not while it may not be renamed, say as it's saved.
        let saving = FileMenu {
            rename: Some(false),
            ..web()
        };
        assert!(click_in_menu(saving, "Rename…").is_empty());
        let computer = FileMenu {
            downloads: None,
            rename: None,
            ..web()
        };
        let shown = crate::testing::Laid::new(file_menu(computer), Size::new(800.0, 600.0)).texts();
        let first = shown
            .iter()
            .min_by(|a, b| a.bounds.y.total_cmp(&b.bounds.y));
        assert_eq!(
            first.map(|shown| shown.text.as_str()),
            Some("Save"),
            "{shown:?}"
        );
        assert!(!shown.iter().any(|shown| shown.text == "Rename…"));
        let shown =
            crate::testing::Laid::new(file_menu(native_menu(true)), Size::new(800.0, 600.0))
                .texts();
        for text in ["Download", "Rename…"] {
            assert!(
                !shown.iter().any(|shown| shown.text == text),
                "{text:?} natively"
            );
        }
    }

    #[test]
    fn export_is_in_the_file_menu_while_there_is_something_to_export() {
        let sent = click_file_menu("Export 3MF…", true);
        assert!(
            matches!(sent[..], [Message::File(File::Export)]),
            "{sent:?}"
        );
        // Disabled, the click lands on the menu, which keeps it.
        let sent = click_file_menu("Export 3MF…", false);
        assert!(sent.is_empty(), "{sent:?}");
        // The items around it still go.
        let sent = click_file_menu("Save As…", false);
        assert!(
            matches!(sent[..], [Message::File(File::SaveAs)]),
            "{sent:?}"
        );
    }

    #[test]
    fn a_tolerance_not_offered_is_listed_unticked_under_its_own_name() {
        let near = f64::from_bits(1e-3_f64.to_bits() + 1);
        for fit in [5e-5, near, 1.0004e-3, 0.0999999] {
            let choices = tolerance_choices(Tolerance::new(fit).unwrap());
            assert_eq!(choices.len(), TOLERANCES.len() + 1, "{fit}");
            assert!(choices.iter().all(|&(.., on)| !on), "{fit}");
            let (last, label, _) = choices.last().unwrap();
            assert_eq!(*last, fit);
            assert!(
                TOLERANCES.iter().all(|&(_, offered)| offered != label),
                "{fit} is named {label}, as an offered one is"
            );
        }
        let choices = tolerance_choices(Tolerance::DEFAULT);
        assert_eq!(choices.len(), TOLERANCES.len());
        let ticked: Vec<f64> = choices.iter().filter(|c| c.2).map(|c| c.0).collect();
        assert_eq!(ticked, [Tolerance::DEFAULT.fit()]);
    }

    #[test]
    fn tolerances_are_named_in_micrometres() {
        for (fit, label) in TOLERANCES {
            assert!(Tolerance::new(fit).is_some(), "{label}");
            assert_eq!(tolerance_label(fit), label);
        }
        assert_eq!(tolerance_label(Tolerance::MIN_FIT), "0.01 µm");
        assert_eq!(tolerance_label(Tolerance::MAX_FIT), "100 µm");
        assert_eq!(tolerance_label(0.025), "25 µm");
        assert_eq!(tolerance_label(5e-5), "0.05 µm");
    }
}
