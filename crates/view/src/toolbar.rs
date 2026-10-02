//! The document toolbar and its file menu.

use std::borrow::Cow;

use iced::widget::{
    Button, Container, Space, button, column, container, mouse_area, opaque, row, space, text,
};
use iced::{Alignment, Element, Length, Padding, mouse};
use varde_document::{EXTENSION, OriginPlane, Tolerance};
use varde_expr::LengthUnit;

use crate::chrome::{Edge, edged, hrule, icon_button, key_label, vrule};
use crate::icons::{self, Icon};
use crate::shortcut::{
    Binding, Shortcut, comb_binding, combine_binding, constrain_binding, constraint_binding,
    extrude_binding, file_bindings, handles_binding, history_bindings, measure_binding,
    revolve_binding, sketch_binding, switch_binding, tool_binding,
};
use crate::theme::{self, Emphasis, SEMIBOLD, SIDE_PANEL_INNER_WIDTH, Tone};
use crate::{ActiveTool, ConstraintKind, DocumentState, Edit, File, Look, Message, Overlay, Tool};

/// Includes the 1 px border.
const TOOLBAR_HEIGHT: f32 = 40.0;

/// The width of the Save button's cell, right of the file cell.
const SAVE_CELL_WIDTH: f32 = 34.0;

/// The bar's spacing between its items.
const BAR_SPACING: f32 = 2.0;

pub fn toolbar<'a>(state: &DocumentState<'a>) -> Element<'a, Message> {
    let (context, tag): (Element<'a, Message>, _) = match &state.sketch {
        Some(sketch) => (
            sketch_pill(sketch.name),
            match sketch.tool {
                Some(tool) => Some(tool_tag(&tool)),
                None if sketch.constraining => Some("Constrain".to_owned()),
                None => None,
            },
        ),
        None => (
            text("Model").font(SEMIBOLD).into(),
            if let Some(pick) = state.picking_plane {
                Some(match &pick.sketch {
                    None => "New sketch".to_owned(),
                    Some((_, name)) => format!("{name}'s plane"),
                })
            } else {
                let editing = |editing: Option<&str>, noun: &str| {
                    editing.map_or_else(|| noun.to_owned(), |name| format!("Editing {name}"))
                };
                (state.extrude.as_ref())
                    .map(|extrude| editing(extrude.editing, "Extrude"))
                    .or_else(|| {
                        let revolve = state.revolve.as_ref()?;
                        Some(editing(revolve.editing, "Revolve"))
                    })
                    .or_else(|| {
                        let combine = state.combine.as_ref()?;
                        Some(editing(combine.editing, "Combine"))
                    })
                    .or_else(|| state.measure.as_ref().map(|_| "Measure".to_owned()))
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
        file_cell(
            state.name,
            state.edited,
            state.overlay == Some(Overlay::FileMenu),
        ),
        vrule(),
        save_cell(state.editable(), state.edited),
        vrule(),
        container(row![context, tag].spacing(6).align_y(Alignment::Center))
            // The sketch's pill starts left of where the text would, so its
            // name lines up with "Model".
            .padding(Padding::from([0, 12]).left(if state.sketch.is_some() { 4 } else { 12 })),
        vrule(),
        row(ops(state))
            .spacing(2)
            .padding([0, 6])
            .align_y(Alignment::Center),
        space::horizontal(),
        history_button(Icon::Undo, "Undo", undo),
        history_button(Icon::Redo, "Redo", redo),
        container(vrule()).height(18).padding([0, 4]),
        // TODO: open the command palette.
        icon_button(Icon::Search, Tone::Muted, None),
        crate::chrome::app_buttons(state.mode),
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

/// The sketch being edited, named with its icon on the soft accent, joined
/// at its right end by the button finishing it.
fn sketch_pill<'a>(name: &'a str) -> Element<'a, Message> {
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
    .on_press(Message::Look(Look::FinishSketch));
    container(
        row![
            container(
                row![
                    icons::icon(Icon::Sketch, icons::INLINE),
                    text(name).font(SEMIBOLD)
                ]
                .spacing(6)
                .align_y(Alignment::Center)
            )
            .padding([0, 8]),
            crate::chrome::tip(finish, text("Finish sketch (Esc)")),
        ]
        .height(Length::Fill)
        .align_y(Alignment::Center),
    )
    .height(24)
    .style(theme::pill)
    .into()
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
            .style(theme::flat_button(false))
            .on_press_maybe(message),
        text(format!("Save ({})", save.shortcut.label())),
    );
    container(button)
        .center_x(SAVE_CELL_WIDTH)
        .height(Length::Fill)
        .align_y(Alignment::Center)
        .into()
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

    // With the rule after it and the Save cell, as wide as the side panel
    // under them, so the rule after the Save cell is in line with the
    // panel's edge.
    button(content)
        .width(SIDE_PANEL_INNER_WIDTH - SAVE_CELL_WIDTH - 1.0 - 2.0 * BAR_SPACING)
        .height(Length::Fill)
        .padding(Padding::from([0, 10]).left(12))
        .style(theme::file_cell(open))
        .on_press(Message::Edit(Edit::ToggleFileMenu))
        .into()
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
    }
}

/// The operations for what's going on: modelling, picking the plane for a
/// new sketch, or editing a sketch.
fn ops<'a>(state: &DocumentState<'a>) -> Vec<Element<'a, Message>> {
    let editable = state.editable();
    let keys = state.keys();
    if let Some(sketch) = &state.sketch {
        let active = sketch.tool.map(|tool| tool.tool);
        let tools = Tool::ALL.map(|tool| {
            let icon = tool_icon(tool);
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
        return tools
            .into_iter()
            .chain([separator(), constrain])
            .chain(constraints)
            .chain(spline_ops)
            .collect();
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
                Icon::Plane,
                plane_label(plane),
                editable.then_some(Message::Edit(Edit::PlanePicked(plane))),
            )
        });
        return [sketch, extrude, revolve, separator()]
            .into_iter()
            .chain(planes)
            .collect();
    }
    // After the solids, as the mock orders them.
    let combine = bound_op(
        Icon::Combine,
        "Combine",
        combine_binding(keys),
        state.combine.is_some(),
    );
    // Measure after a separator, as the mock has it.
    let measure = bound_op(
        Icon::Measure,
        "Measure",
        measure_binding(keys),
        state.measure.is_some(),
    );
    vec![sketch, extrude, revolve, combine, separator(), measure]
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
    let enabled = message.is_some();
    let key = key.map(|key| container(key_label(key)).align_right(Length::Fill));
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
        .spacing(10)
        .height(Length::Fill)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(28)
    .padding([0, 8])
    .style(theme::flat_button(false))
    .on_press_maybe(message)
}

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

/// The file menu, as a layer over the whole screen. Clicking outside the
/// menu closes it. Save, and changing the design's `units` or its
/// `tolerance`, are disabled unless the document is `editable`, and Save
/// unless it's `edited` too. Export 3MF is disabled unless `exportable`.
/// A tolerance the menu doesn't offer, from a file, shows unticked.
pub fn file_menu(
    editable: bool,
    edited: bool,
    exportable: bool,
    units: LengthUnit,
    tolerance: Tolerance,
) -> Element<'static, Message> {
    let item = menu_item;
    let separator = menu_separator;

    let bound = |icon, label: &'static str, binding: Binding| {
        let message = binding.sends();
        item(icon, label.into(), Some(binding.shortcut), message)
    };

    let [save, save_as] = file_bindings(editable, edited);
    // No key: exporting is rare, and the tool rail has no tool for it.
    let export = exportable.then_some(Message::File(File::Export));
    let saving = column![
        bound(Icon::Save, "Save", save),
        bound(Icon::Save, "Save As…", save_as),
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

    /// What clicking the file menu's item `label` sends, the menu laid out
    /// over a 800 × 600 window, `exportable` or not.
    fn click_file_menu(label: &str, exportable: bool) -> Vec<Message> {
        let window = Size::new(800.0, 600.0);
        let menu = file_menu(true, true, exportable, LengthUnit::Mm, Tolerance::DEFAULT);
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
