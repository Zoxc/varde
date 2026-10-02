//! The measure tool: what the app hands the view of it, the messages
//! changing it, and its floating panel over the right of the viewport,
//! each value in the design's units with a button copying it. What the
//! viewport draws of it (the snap dots, the points picked, the distance's
//! segment and its label) is in `viewport/measure.rs`.
//!
//! Measuring writes nothing to the document: the picks and what's shown
//! of them are the app's session's, and the values come from the
//! regeneration lane's answer (`varde_regen::Inspected`), in millimetres
//! and radians, shown here in the design's units and degrees.

use glam::DVec3;
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, space, text};
use iced::{Alignment, Element, Length};
use varde_expr::{AngleUnit, LengthUnit, Power, Unit};
use varde_regen::{Between, EdgeForm, Gap, Measure, Summary};

use crate::chrome::{hrule, icon_button, sentence};
use crate::icons::Icon;
use crate::operation_panel::{FIELD_INDENT, Parts, message_text, operation_panel};
use crate::pick::{Pick, PickIndex};
use crate::theme::{self, SEMIBOLD, Tone};
use crate::{Look, Message};

/// One of the measure tool's two picks: the first click picks A, the
/// second B, a third starts again from A.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MeasureSlot {
    A,
    B,
}

impl MeasureSlot {
    /// Where it's kept in an array of both.
    pub fn index(self) -> usize {
        match self {
            MeasureSlot::A => 0,
            MeasureSlot::B => 1,
        }
    }

    fn label(self) -> &'static str {
        match self {
            MeasureSlot::A => "A",
            MeasureSlot::B => "B",
        }
    }
}

/// A change to the measure tool, see [`Look::Measure`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasureLook {
    /// Shows a pick's own values, or folds them away: with two picks
    /// they're folded under what's between them.
    Fold(MeasureSlot),
    /// Leaves the measure tool, changing nothing: Close, or `Esc`.
    Close,
}

/// How a pick stands in the newest answer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outcome<'a> {
    /// The answer for the picks as they are hasn't come yet.
    Waiting,
    /// The model shown doesn't have it any more, and why ("face not
    /// found"): an edit took it away, say. It's kept, and found again
    /// should the model have it again.
    Missing(&'a str),
    /// What it measures, or why it can't be measured.
    Measured(Result<&'a Measure, &'a str>),
}

/// A pick, as the panel shows it.
#[derive(Debug, Clone)]
pub struct Picked<'a> {
    /// What it is ("Face", "Edge", "Point") and of which body, or the
    /// body's name for a body.
    pub name: String,
    pub outcome: Outcome<'a>,
}

/// The measure tool, and how it's shown.
#[derive(Debug, Clone)]
pub struct MeasureState<'a> {
    /// A and B, as picked.
    pub picks: [Option<Picked<'a>>; 2],
    /// What's between them, once both are measured in the newest answer.
    pub between: Option<&'a Between>,
    /// The design's units, which lengths, areas and volumes show in.
    pub units: LengthUnit,
    /// Whether each pick's own values are folded away, with two picks.
    pub folded: [bool; 2],
    /// The model shown, whose snap points the cursor takes.
    pub index: &'a PickIndex,
    /// What the cursor is over, whose snap points show.
    pub hover: Option<Pick>,
    /// Where each pick that's a point is, as the newest answer measured
    /// it: drawn as a dot in its colour.
    pub points: [Option<DVec3>; 2],
}

impl MeasureState<'_> {
    /// The minimum distance between the picks and where it's reached, as
    /// the newest answer has it.
    pub(crate) fn gap(&self) -> Option<&Gap> {
        self.between?.distance.as_ref().ok()
    }
}

/// A value shown in the panel: its label, its text in the design's
/// units, and what copying it gives, at full precision.
#[derive(Debug, Clone, PartialEq)]
pub struct Value {
    pub label: &'static str,
    pub shown: String,
    pub copied: String,
}

impl Value {
    /// A length, an area or a volume, in model units.
    fn size(label: &'static str, value: f64, units: LengthUnit, power: Power) -> Value {
        Value {
            label,
            shown: varde_expr::format_power(value, units, power),
            copied: varde_expr::full_power(value, units, power),
        }
    }

    fn length(label: &'static str, value: f64, units: LengthUnit) -> Value {
        Value::size(label, value, units, Power::Length)
    }

    /// An angle, in radians, shown in degrees.
    fn angle(label: &'static str, radians: f64) -> Value {
        let degrees = Some(Unit::Angle(AngleUnit::Deg));
        Value {
            label,
            shown: varde_expr::format(radians, degrees),
            copied: varde_expr::full(radians, degrees),
        }
    }

    /// A point or a vector: its parts in `unit`, the unit's symbol once
    /// after them, or none for a direction.
    fn triple(label: &'static str, parts: [f64; 3], unit: Option<LengthUnit>) -> Value {
        let unit = unit.map(Unit::Length);
        let join = |each: &dyn Fn(f64) -> String| {
            let numbers = parts.map(each).join(", ");
            match unit {
                Some(unit) => format!("{numbers} {}", unit.symbol()),
                None => numbers,
            }
        };
        Value {
            label,
            shown: join(&|x| varde_expr::format_number(x, unit)),
            copied: join(&|x| varde_expr::full_number(x, unit)),
        }
    }
}

/// What kind of face `summary` is, from its form.
pub fn face_kind(summary: &Summary) -> &'static str {
    match summary {
        Summary::Plane { .. } => "Planar face",
        Summary::Cylinder { .. } => "Cylindrical face",
        Summary::Cone { .. } => "Conical face",
        Summary::Sphere { .. } => "Spherical face",
        Summary::Torus { .. } => "Toroidal face",
        Summary::Revolved { .. } => "Revolved face",
        Summary::ConicCylinder { .. } | Summary::Other => "Curved face",
    }
}

/// What kind of edge `form` is.
fn edge_kind(form: &EdgeForm) -> &'static str {
    match form {
        EdgeForm::Line { .. } => "Straight edge",
        EdgeForm::Circle { .. } => "Circular edge",
        EdgeForm::Ellipse { .. } => "Elliptic edge",
        EdgeForm::Other => "Curved edge",
    }
}

/// The values `measure` shows in a design in `units`.
pub fn values(measure: &Measure, units: LengthUnit) -> Vec<Value> {
    let length = |label, value| Value::length(label, value, units);
    let point = |label, at| Value::triple(label, at, Some(units));
    match *measure {
        Measure::Point(at) => {
            let [x, y, z] = at;
            vec![length("X", x), length("Y", y), length("Z", z)]
        }
        Measure::Edge {
            length: l, shape, ..
        } => {
            let mut values = vec![length("Length", l)];
            match shape {
                EdgeForm::Line { from, to } => {
                    let along = DVec3::from(to) - DVec3::from(from);
                    if let Some(along) = along.try_normalize() {
                        values.push(Value::triple("Direction", along.to_array(), None));
                    }
                }
                EdgeForm::Circle { centre, radius, .. } => {
                    values.push(length("Radius", radius));
                    values.push(length("Diameter", 2.0 * radius));
                    values.push(point("Centre", centre));
                }
                EdgeForm::Ellipse {
                    centre,
                    major,
                    minor,
                    ..
                } => {
                    values.push(length("Semi-major", major));
                    values.push(length("Semi-minor", minor));
                    values.push(point("Centre", centre));
                }
                EdgeForm::Other => {}
            }
            values
        }
        Measure::Face {
            area,
            summary,
            half_angle,
        } => {
            let mut values = vec![Value::size("Area", area, units, Power::Area)];
            match summary {
                Summary::Plane { n, .. } => values.push(Value::triple("Normal", n, None)),
                Summary::Cylinder { radius, .. } | Summary::Sphere { radius, .. } => {
                    values.push(length("Radius", radius));
                    values.push(length("Diameter", 2.0 * radius));
                }
                Summary::Torus { major, minor, .. } => {
                    values.push(length("Major radius", major));
                    values.push(length("Minor radius", minor));
                }
                _ => {}
            }
            values.extend(half_angle.map(|angle| Value::angle("Half-angle", angle)));
            values
        }
        Measure::Body {
            volume,
            area,
            centre,
            bounds,
        } => {
            let mut values = vec![
                Value::size("Volume", volume, units, Power::Volume),
                Value::size("Area", area, units, Power::Area),
            ];
            values.extend(centre.map(|centre| point("Centre", centre)));
            if let Some([min, max]) = bounds {
                let size = std::array::from_fn(|i| max[i] - min[i]);
                values.push(point("Box from", min));
                values.push(point("Box to", max));
                values.push(point("Box size", size));
            }
            values
        }
    }
}

/// What's shown between two picks in a design in `units`: the minimum
/// distance and its X, Y and Z parts (from A's point to B's), and the
/// angle between their directions where both have one.
pub fn between_values(between: &Between, units: LengthUnit) -> Vec<Value> {
    let mut values = Vec::new();
    if let Ok(gap) = &between.distance {
        let [a, b] = gap.points.map(DVec3::from);
        let apart = b - a;
        values.push(Value::length("Distance", gap.distance, units));
        values.push(Value::length("ΔX", apart.x, units));
        values.push(Value::length("ΔY", apart.y, units));
        values.push(Value::length("ΔZ", apart.z, units));
    }
    values.extend(between.angle.map(|angle| Value::angle("Angle", angle)));
    values
}

/// The name a pick shows with in the panel once measured: its kind from
/// its form ("Planar face", "Circular edge") in place of the bare
/// "Face" or "Edge", with the body it's of.
fn shown_name<'a>(picked: &'a Picked<'_>) -> std::borrow::Cow<'a, str> {
    let kind = match picked.outcome {
        Outcome::Measured(Ok(Measure::Face { summary, .. })) => Some(face_kind(summary)),
        Outcome::Measured(Ok(Measure::Edge { shape, .. })) => Some(edge_kind(shape)),
        _ => None,
    };
    match (kind, picked.name.split_once(" of ")) {
        (Some(kind), Some((_, body))) => format!("{kind} of {body}").into(),
        _ => picked.name.as_str().into(),
    }
}

/// The floating panel of the measure tool.
pub(crate) fn panel<'a>(state: &MeasureState<'a>) -> Element<'a, Message> {
    let units = state.units;
    let mut body = column![
        pick_row(state, MeasureSlot::A),
        pick_row(state, MeasureSlot::B)
    ]
    .spacing(6);
    match &state.picks {
        [Some(only), None] | [None, Some(only)] => {
            body = body.push(hrule()).push(own_values(only, units));
        }
        [Some(_), Some(_)] => {
            body = body.push(hrule());
            let between = match state.between {
                Some(between) => {
                    let values = between_values(between, units);
                    let error = between.distance.as_ref().err().map(|error| note(error));
                    column(values.into_iter().map(value_row))
                        .push(error)
                        .spacing(4)
                        .into()
                }
                None if waiting(state) => message_text("Measuring…", theme::muted_text),
                None => message_text("Nothing to measure between them", theme::muted_text),
            };
            body = body.push(between);
            for slot in [MeasureSlot::A, MeasureSlot::B] {
                let Some(picked) = &state.picks[slot.index()] else {
                    continue;
                };
                let folded = state.folded[slot.index()];
                body = body.push(hrule()).push(fold_header(picked, slot, folded));
                if !folded {
                    body = body.push(own_values(picked, units));
                }
            }
        }
        [None, None] => {}
    }
    operation_panel(Parts {
        title: "Measure",
        summary: None,
        body: body.into(),
        message: None,
        ok: None,
        accept: None,
        cancel: Message::Look(Look::Measure(MeasureLook::Close)),
        close: true,
    })
}

/// Whether the answer for the picks as they are hasn't come yet.
fn waiting(state: &MeasureState<'_>) -> bool {
    state
        .picks
        .iter()
        .flatten()
        .any(|picked| picked.outcome == Outcome::Waiting)
}

/// The row of `slot`'s pick: its tag and its name, or what to click.
fn pick_row<'a>(state: &MeasureState<'a>, slot: MeasureSlot) -> Element<'a, Message> {
    let tag = container(text(slot.label()).size(11).font(SEMIBOLD))
        .padding([1, 6])
        .style(theme::pick_tag(slot == MeasureSlot::B));
    let content: Element<'a, Message> = match &state.picks[slot.index()] {
        Some(picked) => {
            let name = text(shown_name(picked).into_owned())
                .size(12)
                .wrapping(Wrapping::WordOrGlyph);
            let note = match picked.outcome {
                Outcome::Missing(why) => Some(note(why)),
                _ => None,
            };
            column![name, note].spacing(2).into()
        }
        None => {
            let ask = match slot {
                MeasureSlot::A => "Click a face, edge, point or body",
                MeasureSlot::B => "Click another to measure between",
            };
            text(ask)
                .size(12)
                .wrapping(Wrapping::WordOrGlyph)
                .style(theme::muted_text)
                .into()
        }
    };
    row![tag, content]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
}

/// A pick's own values, or why there are none.
fn own_values<'a>(picked: &Picked<'a>, units: LengthUnit) -> Element<'a, Message> {
    match picked.outcome {
        Outcome::Waiting => message_text("Measuring…", theme::muted_text),
        Outcome::Missing(_) => space::vertical().height(0).into(),
        Outcome::Measured(Err(why)) => note(why),
        Outcome::Measured(Ok(measure)) => column(values(measure, units).into_iter().map(value_row))
            .spacing(4)
            .into(),
    }
}

/// The header folding `slot`'s values away or showing them.
fn fold_header<'a>(picked: &Picked<'a>, slot: MeasureSlot, folded: bool) -> Element<'a, Message> {
    let arrow = if folded { "▸" } else { "▾" };
    let label = format!("{arrow} {}  {}", slot.label(), shown_name(picked));
    button(
        text(label)
            .size(12)
            .font(SEMIBOLD)
            .wrapping(Wrapping::WordOrGlyph)
            .width(Length::Fill),
    )
    .padding([2, 0])
    .width(Length::Fill)
    .style(theme::flat_button(false))
    .on_press(Message::Look(Look::Measure(MeasureLook::Fold(slot))))
    .into()
}

/// A value's row: its label as wide as a typed value's, its text, and a
/// button copying it with its unit at full precision.
fn value_row<'a>(value: Value) -> Element<'a, Message> {
    let label = text(value.label)
        .size(12)
        .style(theme::muted_text)
        .width(FIELD_INDENT - 6.0);
    let shown = text(value.shown)
        .size(12)
        .wrapping(Wrapping::WordOrGlyph)
        .width(Length::Fill);
    let copy = icon_button(Icon::Copy, Tone::Faint, Some(Message::Copy(value.copied)));
    row![label, shown, copy]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
}

/// Why something wasn't found or measured, in the danger colour.
fn note<'a>(why: &str) -> Element<'a, Message> {
    message_text(sentence(why).into_owned(), theme::danger_text)
}

#[cfg(test)]
mod tests;
