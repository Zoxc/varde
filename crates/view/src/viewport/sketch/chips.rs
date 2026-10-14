//! The widgets over the sketch being edited: the constraints' glyphs, the
//! snap's, the dimensions' label chips and the drawing tools' fields.

use glam::DVec2;
use iced::widget::{MouseArea, column, container, mouse_area, row, text};
use iced::{Alignment, Color, Element, Padding, mouse};
use varde_sketch::Id;

use super::draw::tool_items;
use super::{Sketching, fade, glyphs};
use crate::snap::Snap;
use crate::theme::{self, SketchColors};
use crate::typed::{self, Field};
use crate::{
    ConstraintKind, Look, Message, Tool, ValueField, ValueTarget, dimension, icons, panels,
};

/// The side of a constraint's glyph's icon, and the room around it on its
/// chip, in pixels.
const GLYPH_ICON: f32 = 16.0;
const GLYPH_PADDING: f32 = 2.0;
/// The side of a snap's glyph's icon, in pixels.
const SNAP_ICON: f32 = 12.0;
/// The size of a dimension's label's text, and the room around it on its
/// chip, in pixels.
const LABEL_SIZE: f32 = 12.0;
const LABEL_PADDING: [u16; 2] = [1, 4];
/// How wide a field's name is beside it, in pixels.
const FIELD_NAME_WIDTH: f32 = 56.0;

impl<'a> Sketching<'a> {
    /// The glyphs of the sketch's constraints, if they're shown, each
    /// where [`glyphs::anchors`] puts it: hovering one highlights what it
    /// ties together, clicking selects it. In red while in a conflict, in
    /// the selection's colour while selected, faded while waiting on the
    /// solver.
    pub(crate) fn glyphs(&self) -> Vec<(DVec2, Element<'a, Message>)> {
        if !self.glyphs {
            return Vec::new();
        }
        let mut glyphs = Vec::new();
        for entry in &self.sketch.constraints {
            let id = entry.id;
            let kind = ConstraintKind::of(&entry.constraint);
            let look = self.look(id);
            for at in glyphs::anchors(self.sketch, &entry.constraint) {
                glyphs.push((at, glyph(id, kind, look)));
            }
        }
        glyphs
    }

    /// The glyph of where the drawing tool's next click snaps to, if it
    /// does, by the cursor: what the point is on, then how the shape
    /// runs, as their constraints' icons, as far as the values typed leave
    /// them (see [`typed::hold`]).
    pub(crate) fn snap_glyph(&self) -> Option<(DVec2, Element<'a, Message>)> {
        let tool = self.tool.filter(|tool| tool.tool.draws())?;
        let snap = self.snap?;
        // What the values typed leave of it, where it snapped.
        let snap = Snap {
            at: snap.at,
            ..typed::hold(tool, snap)?
        };
        if !snap.snapped() {
            return None;
        }
        // On the Spline tool's first point, once that closes it, Close's
        // icon rather than Coincident's.
        let closes =
            tool.tool == Tool::Spline && tool.can_close() && tool.placed.first() == Some(&snap.at);
        let kinds = if closes { Vec::new() } else { snap.kinds() };
        let close = closes.then_some(icons::Icon::CloseCurve);
        let icons = (close.into_iter())
            .chain(kinds.into_iter().map(|kind| kind.icon()))
            .map(|icon| icons::tinted(icon, SNAP_ICON, |palette| palette.sketching.guide).into());
        let chip = container(iced::widget::Row::with_children(icons).spacing(2))
            .padding(GLYPH_PADDING)
            .style(|theme| theme::glyph(theme, false));
        Some((snap.at, chip.into()))
    }

    /// How the constraint or dimension `id` looks, by its state.
    pub(super) fn look(&self, id: Id) -> GlyphLook {
        if self.states.conflicts.contains(&id) {
            GlyphLook::Conflict
        } else if self.selected(id) {
            GlyphLook::Selected
        } else if self.states.pending.contains(&id) {
            GlyphLook::Pending
        } else {
            GlyphLook::Free
        }
    }

    /// The labels of the sketch's dimensions, each at its place: its
    /// value, coloured by its state; a reference's after an equals sign
    /// in a green pill, as the Geometry list notes a driven size, outlined
    /// as a driving one's chip while selected or in a conflict. Pressing
    /// one selects it and grabs it to drag, double-clicking a driving one
    /// opens the value field in its place, hovering it highlights what it
    /// measures. The one the field is open on over the viewport is left
    /// out: the field takes its place.
    pub(crate) fn labels(&self) -> Vec<(DVec2, Element<'a, Message>)> {
        let edited =
            self.value
                .filter(|field| !field.in_list)
                .and_then(|field| match field.target {
                    ValueTarget::Dimension(id) => Some(*id),
                    ValueTarget::New { .. } | ValueTarget::Field(_) => None,
                });
        let shown = self
            .sketch
            .dimensions
            .iter()
            .filter(|entry| Some(entry.id) != edited);
        shown
            .filter_map(|entry| {
                let at = self.label_at(entry)?;
                let dimension = &entry.dimension;
                // A reference's after an equals sign in its pill.
                let value = dimension::shown(self.sketch, dimension, self.units);
                let chip = label_chip(
                    entry.id,
                    value,
                    self.look(entry.id),
                    dimension.driving,
                    self.editable,
                );
                Some((at, chip))
            })
            .collect()
    }

    /// The value field over the viewport, if it's open there: at the label
    /// of the dimension it edits, or of the one it places, with why the
    /// value typed was refused under it if it was.
    pub(crate) fn field(&self) -> Option<(DVec2, Element<'a, Message>)> {
        let field = self.value.filter(|field| !field.in_list)?;
        let at = match field.target {
            ValueTarget::New { measure, label, .. } => self.sketch.anchor(measure)? + *label,
            ValueTarget::Dimension(id) => self.label_at(self.sketch.dimension(*id)?)?,
            // By the cursor, with the tool's other fields.
            ValueTarget::Field(_) => return None,
        };
        let content = column![
            panels::dimension_value_field("", field.text, field.params),
            field_error(field)
        ]
        .spacing(2);
        let chip = container(content)
            .padding(2)
            .style(|theme| theme::glyph(theme, false));
        Some((at, chip.into()))
    }

    /// The drawing tool's fields, while its shape has any, by where its
    /// click would go (see [`typed`]): each named, showing the value typed
    /// in it, or what it measures of the shape until one is, and the one
    /// the value field is open on taking it, with why the value typed was
    /// refused under it if it was.
    pub(crate) fn fields(&self) -> Option<(DVec2, Element<'a, Message>)> {
        let tool = self.tool?;
        let fields = tool.fields();
        let at = self.aim.filter(|_| !fields.is_empty())?;
        let outline =
            typed::outline(tool, at).or_else(|| typed::corner_outline(tool, self.sketch, at));
        let open = self
            .value
            .filter(|field| matches!(field.target, ValueTarget::Field(_)));
        let rows = fields.iter().map(|&field| {
            let typed = tool.value_in(field);
            let measured = match field {
                Field::Sides => Some(f64::from(tool.sides)),
                Field::Distance if tool.tool == Tool::Offset => self
                    .sketch
                    .offset_side(&tool_items(tool), at)
                    .map(|(reach, _)| reach),
                _ => outline.as_ref().and_then(|outline| outline.value(field)),
            };
            let format = varde_expr::Shown::current();
            let unit = format.unit(field.quantity(), self.units);
            let shown = match typed {
                Some(value) => format.format(value.value, unit),
                None => measured.map_or_else(String::new, |value| format.format(value, unit)),
            };
            let value: Element<'a, Message> = match open {
                Some(open) if *open.target == ValueTarget::Field(field) => column![
                    panels::dimension_value_field(&shown, open.text, open.params),
                    field_error(open)
                ]
                .spacing(2)
                .into(),
                _ => text(shown)
                    .size(LABEL_SIZE)
                    .style(if typed.is_some() {
                        theme::accent_text
                    } else {
                        theme::muted_text
                    })
                    .into(),
            };
            row![text(field.label()).size(11).width(FIELD_NAME_WIDTH), value]
                .spacing(4)
                .align_y(Alignment::Center)
                .into()
        });
        let chip = container(iced::widget::Column::with_children(rows).spacing(2))
            .padding(LABEL_PADDING)
            .style(|theme| theme::glyph(theme, false));
        Some((at, chip.into()))
    }
}

/// How a constraint's glyph, or a dimension, looks, by its state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GlyphLook {
    Free,
    Selected,
    Conflict,
    Pending,
}

impl GlyphLook {
    /// Its colour among `colors`: free in the sketch colour, or a
    /// `reference` dimension in the construction colour, and that faded
    /// while waiting on the solver.
    fn color(self, colors: SketchColors, reference: bool) -> Color {
        let free = if reference {
            colors.construction
        } else {
            colors.curve
        };
        self.over(free, colors)
    }

    /// Its colour as a dimension's among `colors`: free in the Dimension
    /// tools' colour if `driving`, else in the driven colour, as its
    /// label's pill, and that faded while waiting on the solver.
    pub(super) fn dimension_color(self, colors: SketchColors, driving: bool) -> Color {
        let free = if driving {
            colors.dimension
        } else {
            colors.driven
        };
        self.over(free, colors)
    }

    /// Its colour among `colors`, free in `free`.
    fn over(self, free: Color, colors: SketchColors) -> Color {
        match self {
            GlyphLook::Free => free,
            GlyphLook::Selected => colors.selected,
            GlyphLook::Conflict => colors.conflict,
            GlyphLook::Pending => fade(free),
        }
    }
}

/// The glyph of the constraint `id`, of `kind`, looking `look`: its icon on
/// a chip, in its category's colours while free, else in one colour by
/// its look, which selects it when clicked and highlights what it ties
/// together while hovered.
fn glyph<'a>(id: Id, kind: ConstraintKind, look: GlyphLook) -> Element<'a, Message> {
    let icon: Element<'a, Message> = if look == GlyphLook::Free {
        icons::icon(kind.icon(), GLYPH_ICON)
    } else {
        icons::tinted(kind.icon(), GLYPH_ICON, move |palette| {
            look.color(palette.sketching, false)
        })
        .into()
    };
    chip(id, icon, GLYPH_PADDING, look)
        .on_press(Message::Look(Look::ClickRow(id.into())))
        .interaction(mouse::Interaction::Pointer)
        .into()
}

/// The label of the dimension `id`, showing `value`, looking `look`, in
/// the construction colour unless `driving`: pressing it selects it and,
/// if `editable`, grabs it to drag, double-clicking a driving one opens the
/// value field on it, and hovering it highlights what it measures.
fn label_chip<'a>(
    id: Id,
    value: String,
    look: GlyphLook,
    driving: bool,
    editable: bool,
) -> Element<'a, Message> {
    // A reference's measures a driven size: an equals sign before it, in
    // a green pill, as the Geometry list notes one. Selected or in a
    // conflict, it's outlined and coloured as a driving one's chip is.
    let value = text(value)
        .size(LABEL_SIZE)
        .style(move |theme| text::Style {
            color: Some(if driving {
                look.dimension_color(theme::palette(theme).sketching, driving)
            } else {
                look.over(theme::palette(theme).text, theme::palette(theme).sketching)
            }),
        });
    let area = if !driving {
        let mark = text("=").size(LABEL_SIZE).style(move |theme| text::Style {
            color: Some(look.dimension_color(theme::palette(theme).sketching, false)),
        });
        let content = row![mark, value].spacing(3).align_y(Alignment::Center);
        let pill = container(content)
            .padding(LABEL_PADDING)
            .style(move |theme| {
                let pill = theme::driven_pill(theme, true);
                match look {
                    GlyphLook::Selected => iced::widget::container::Style {
                        border: theme::selected_glyph(theme)
                            .border
                            .rounded(pill.border.radius),
                        ..pill
                    },
                    GlyphLook::Conflict => iced::widget::container::Style {
                        border: theme::glyph(theme, true).border.rounded(pill.border.radius),
                        ..pill
                    },
                    GlyphLook::Free | GlyphLook::Pending => pill,
                }
            });
        mouse_area(pill)
            .on_enter(Message::Look(Look::HoverItem(Some(id.into()))))
            .on_exit(Message::Look(Look::LeaveItem(id.into())))
    } else {
        chip(id, value, LABEL_PADDING, look)
    };
    let area = area
        .on_press(Message::Look(Look::PressLabel { id, add: false }))
        .interaction(if editable {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::Pointer
        });
    if driving && editable {
        area.on_double_click(Message::Look(Look::EditDimension { id, in_list: false }))
            .into()
    } else {
        area.into()
    }
}

/// `content` on a chip over the viewport, as a glyph or a label of the
/// constraint or dimension `id` looking `look`, highlighting what `id`
/// ties together while hovered.
fn chip<'a>(
    id: Id,
    content: impl Into<Element<'a, Message>>,
    padding: impl Into<Padding>,
    look: GlyphLook,
) -> MouseArea<'a, Message> {
    let chip = container(content)
        .padding(padding)
        .style(move |theme| match look {
            GlyphLook::Selected => theme::selected_glyph(theme),
            _ => theme::glyph(theme, look == GlyphLook::Conflict),
        });
    mouse_area(chip)
        .on_enter(Message::Look(Look::HoverItem(Some(id.into()))))
        .on_exit(Message::Look(Look::LeaveItem(id.into())))
}

/// Why the value typed in `field` was refused, under it, if it was.
fn field_error<'a>(field: ValueField<'_>) -> Option<Element<'a, Message>> {
    field.error.map(|error| {
        text(crate::chrome::sentence(&error.to_string()).into_owned())
            .size(11)
            .style(theme::danger_text)
            .into()
    })
}
