//! The document toolbar and its file menu.

use iced::widget::{Space, button, column, container, mouse_area, opaque, row, space, text};
use iced::{Alignment, Element, Length, Padding, mouse};
use varde_document::{EXTENSION, OriginPlane};
use varde_expr::LengthUnit;

use crate::chrome::{Edge, edged, hrule, icon_button, key_label, vrule};
use crate::icons::{self, Icon};
use crate::shortcut::{
    Binding, Shortcut, comb_binding, constrain_binding, constraint_binding, file_bindings,
    handles_binding, sketch_binding, switch_binding, tool_binding,
};
use crate::theme::{self, Emphasis, SEMIBOLD, SIDE_PANEL_INNER_WIDTH, Tone};
use crate::{ActiveTool, ConstraintKind, DocumentState, Edit, File, Look, Message, Overlay, Tool};

/// Includes the 1 px border.
const TOOLBAR_HEIGHT: f32 = 40.0;

pub fn toolbar<'a>(state: &DocumentState<'a>) -> Element<'a, Message> {
    let editor = state.editor;

    let editable = state.editable();
    let (context, tag) = match &state.sketch {
        Some(sketch) => (
            sketch.name,
            Some(match sketch.tool {
                Some(tool) => tool_tag(&tool),
                None if sketch.constraining => "Constrain".to_owned(),
                None => "Editing sketch".to_owned(),
            }),
        ),
        None => (
            "Model",
            state.picking_plane.then(|| "New sketch".to_owned()),
        ),
    };
    let tag = tag.map(|tag| {
        container(text(tag).size(11).font(SEMIBOLD))
            .padding([2, 7])
            .style(theme::tag)
    });

    let bar = row![
        file_cell(
            state.name,
            state.edited,
            state.overlay == Some(Overlay::FileMenu),
        ),
        vrule(),
        container(
            row![text(context).font(SEMIBOLD), tag]
                .spacing(6)
                .align_y(Alignment::Center)
        )
        .padding([0, 12]),
        vrule(),
        row(ops(state))
            .spacing(2)
            .padding([0, 6])
            .align_y(Alignment::Center),
        space::horizontal(),
        icon_button(
            Icon::Undo,
            Tone::Muted,
            (editable && (editor.can_undo() || state.proposing))
                .then_some(Message::Edit(Edit::Undo))
        ),
        icon_button(
            Icon::Redo,
            Tone::Muted,
            // Not while edits wait on the solver, which come after.
            (editable && editor.can_redo() && !state.proposing)
                .then_some(Message::Edit(Edit::Redo))
        ),
        container(vrule()).height(18).padding([0, 4]),
        // TODO: open the command palette.
        icon_button(Icon::Search, Tone::Muted, None),
        crate::chrome::app_buttons(state.mode),
        Space::new().width(8),
    ]
    .spacing(2)
    .height(Length::Fill)
    .align_y(Alignment::Center);

    edged(
        container(bar).width(Length::Fill).style(theme::toolbar),
        Edge::Bottom,
        TOOLBAR_HEIGHT,
    )
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

/// The document name with a dirty dot and a chevron, opening the file menu.
fn file_cell<'a>(name: &'a str, edited: bool, open: bool) -> Element<'a, Message> {
    let dirty = edited.then(|| container(Space::new().width(6).height(6)).style(theme::dirty_dot));

    let content = row![
        row![
            text(name).font(SEMIBOLD),
            text(format!(".{EXTENSION}")).style(theme::faint_text)
        ],
        dirty,
        space::horizontal(),
        icons::tinted(Icon::Chev, icons::INLINE, |p| p.muted),
    ]
    .spacing(8)
    .height(Length::Fill)
    .align_y(Alignment::Center);

    button(content)
        .width(SIDE_PANEL_INNER_WIDTH)
        .height(Length::Fill)
        .padding(Padding::from([0, 10]).left(12))
        .style(theme::file_cell(open))
        .on_press(Message::Edit(Edit::ToggleFileMenu))
        .into()
}

/// The operations for what's going on: modelling, picking the plane for a
/// new sketch, or editing a sketch.
fn ops<'a>(state: &DocumentState<'a>) -> Vec<Element<'a, Message>> {
    let editable = state.editable();
    let keys = state.keys();
    if let Some(sketch) = &state.sketch {
        let active = sketch.tool.map(|tool| tool.tool);
        let tools = Tool::ALL.map(|tool| {
            let icon = match tool {
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
            };
            bound_op(
                icon,
                tool.label(),
                tool_binding(tool, keys),
                active == Some(tool),
            )
        });
        let constrain = bound_op(
            Icon::Constrain,
            "Constrain",
            constrain_binding(keys),
            sketch.constraining,
        );
        // The Constrain tool offers the constraints that fit the
        // selection, the most likely first.
        let constraints = sketch
            .constraining
            .then(|| {
                let fitting = ConstraintKind::fitting(sketch.sketch, sketch.selection);
                let bound = fitting.into_iter().filter_map(|kind| {
                    let binding = constraint_binding(kind, keys)?;
                    Some(bound_op(kind.icon(), kind.label(), binding, false))
                });
                std::iter::once(separator()).chain(bound)
            })
            .into_iter()
            .flatten();
        // With splines selected, and no drawing tool: switching them,
        // handles and the curvature comb.
        let spline_ops = keys
            .splines_selected
            .then(|| {
                [
                    separator(),
                    bound_op(Icon::Convert, "Convert", switch_binding(keys), false),
                    bound_op(Icon::Handles, "Handles", handles_binding(keys), false),
                    bound_op(Icon::Comb, "Comb", comb_binding(keys), sketch.comb),
                ]
            })
            .into_iter()
            .flatten();
        let finish = button(
            row![
                icons::tinted(Icon::Check, icons::INLINE, |p| Emphasis::Primary.content(p)),
                text("Finish sketch").font(SEMIBOLD),
            ]
            .spacing(6)
            .height(Length::Fill)
            .align_y(Alignment::Center),
        )
        .height(28)
        .padding([0, 10])
        .style(theme::primary_button)
        .on_press(Message::Look(Look::FinishSketch));
        return tools
            .into_iter()
            .chain([separator(), constrain])
            .chain(constraints)
            .chain(spline_ops)
            .chain([separator(), finish.into()])
            .collect();
    }
    let sketch = bound_op(
        Icon::Sketch,
        "Sketch",
        sketch_binding(keys),
        state.picking_plane,
    );
    if state.picking_plane {
        // Picking a plane in the viewport comes with picking, so the
        // origin planes are offered here.
        let planes = OriginPlane::ALL.map(|plane| {
            op(
                Icon::Plane,
                plane_label(plane),
                editable.then_some(Message::Edit(Edit::NewSketch(plane))),
            )
        });
        return [sketch, separator()].into_iter().chain(planes).collect();
    }
    vec![sketch]
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
    op_button(icon, label, Some(binding.shortcut), on, message)
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
    .style(theme::flat_button(on))
    .on_press_maybe(message)
    .into()
}

/// The design units the file menu offers, with their names.
const UNITS: [(LengthUnit, &str); 2] =
    [(LengthUnit::Mm, "Millimetres"), (LengthUnit::In, "Inches")];

/// The file menu, as a layer over the whole screen. Clicking outside the
/// menu closes it. Save, and changing the design's `units`, are disabled
/// unless the document is `editable`.
pub fn file_menu(editable: bool, units: LengthUnit) -> Element<'static, Message> {
    let item = |icon, label, key: Option<Shortcut>, message: Option<Message>| {
        let enabled = message.is_some();
        let key = key.map(|key| container(key_label(key)).align_right(Length::Fill));
        button(
            row![
                // Text-toned, so hovering doesn't change it.
                icons::tinted(icon, icons::INLINE, move |p| {
                    theme::flat_content(p, Tone::Text, enabled, false)
                }),
                text(label),
                key,
            ]
            .spacing(10)
            .height(Length::Fill)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(28)
        .padding([0, 8])
        .style(theme::flat_button(false))
        .on_press_maybe(message)
    };
    let separator = || container(hrule()).padding([4, 2]);

    let bound = |icon, label, binding: Binding| {
        let message = binding.sends();
        item(icon, label, Some(binding.shortcut), message)
    };

    // Export and the rest join Save once they exist.
    let [save, save_as] = file_bindings(editable);
    let saving = column![
        bound(Icon::Save, "Save", save),
        bound(Icon::Save, "Save As…", save_as),
        separator(),
    ];
    // A new design starts in millimetres; its units are chosen here, and
    // changed here later.
    let heading = container(
        text("Units")
            .size(11.5)
            .font(SEMIBOLD)
            .style(theme::muted_text),
    )
    .padding([4, 8]);
    let choices = UNITS.map(|(unit, label)| {
        let message = editable.then_some(Message::Edit(Edit::SetUnits(unit)));
        let icon = if unit == units {
            Icon::Check
        } else {
            Icon::Blank
        };
        item(icon, label, None, message).into()
    });
    let menu = container(
        column![
            saving,
            heading,
            column(choices),
            separator(),
            item(
                Icon::Close,
                "Close document",
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
