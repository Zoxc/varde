//! The combine being set up: what the app hands the view of it, the
//! messages changing it, and its floating panel over the right of the
//! viewport. Its bodies are picked in the viewport, where the cursor picks
//! the model as outside the sessions and a click picks the body of what
//! it's on, or in Objects; the panel shows them as chips, the target in
//! one field and the tools in another, each with a button taking it out.

use iced::Element;
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, space, text};
use iced::{Alignment, Length};
use varde_document::{BodyId, BodyOp};

use crate::chrome::{heading, hrule};
use crate::icons::{self, Icon};
use crate::operation_panel::{
    FIELD_INDENT, Parts, choice, footer_message, message_text, operation_panel, tick,
};
use crate::theme;
use crate::{Edit, Look, Message};

/// What a click on a body picks: the target, or a tool. A click on one
/// of the panel's two fields makes it the one picking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CombinePick {
    #[default]
    Target,
    Tools,
}

/// A change to the combine being set up, see [`Look::Combine`].
#[derive(Debug, Clone, PartialEq)]
pub enum CombineLook {
    /// Whether clicks pick the target or tools: the panel's Target and
    /// Tools fields clicked.
    Picking(CombinePick),
    /// Takes `body` out, the target or a tool: its chip's button.
    Drop(BodyId),
    Operation(BodyOp),
    /// Keeps the tools as bodies of their own, or uses them up.
    KeepTools,
    /// Drops the combine being set up, changing nothing: Cancel, or `Esc`.
    Cancel,
}

/// A body the combine names, and its name.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CombineBody<'a> {
    pub body: BodyId,
    pub name: &'a str,
}

/// The combine being set up, and how it's shown.
#[derive(Debug, Clone)]
pub struct CombineState<'a> {
    /// The name of the combine edited, or none for a new one.
    pub editing: Option<&'a str>,
    pub target: Option<CombineBody<'a>>,
    /// The tools, in the order they were made, which is the order they're
    /// combined in.
    pub tools: Vec<CombineBody<'a>>,
    /// What a click on a body picks.
    pub picking: CombinePick,
    pub op: BodyOp,
    pub keep_tools: bool,
    /// Whether the document has two bodies or more: with one there's
    /// nothing to combine it with, which the panel says.
    pub enough: bool,
    /// Why the preview failed, if it did.
    pub error: Option<&'a str>,
    /// Whether sketch edits have waited on the solver long enough to say
    /// so: OK waits for them, and the panel says why.
    pub checking: bool,
    /// Whether OK can be pressed.
    pub ready: bool,
    /// Whether the document can be changed.
    pub editable: bool,
}

/// The operations a combine does, as the panel offers them.
const OPS: [BodyOp; 3] = [BodyOp::Union, BodyOp::Subtract, BodyOp::Intersect];

/// The floating panel of the combine being set up.
pub(crate) fn panel<'a>(state: &CombineState<'a>) -> Element<'a, Message> {
    let editable = state.editable;
    let send = |look: CombineLook| editable.then_some(Message::Look(Look::Combine(look)));
    let drop = |body: BodyId| send(CombineLook::Drop(body));

    let target = pick_row(
        "Target",
        pick_field(
            state
                .target
                .iter()
                .map(|body| chip(body.name, drop(body.body))),
            false,
            "Click a body",
            state.picking == CombinePick::Target,
            send(CombineLook::Picking(CombinePick::Target)),
        ),
    );
    let tools = pick_row(
        "Tools",
        pick_field(
            state
                .tools
                .iter()
                .map(|body| chip(body.name, drop(body.body))),
            true,
            "Click bodies",
            state.picking == CombinePick::Tools,
            send(CombineLook::Picking(CombinePick::Tools)),
        ),
    );
    let ops = OPS.map(|op| {
        Element::from(
            container(choice(
                op.label(),
                state.op == op,
                send(CombineLook::Operation(op)),
            ))
            .width(Length::Fill),
        )
    });
    let keep = column![
        tick(
            "Keep tool bodies",
            state.keep_tools,
            send(CombineLook::KeepTools),
        ),
        // Under the label, as the mock's sub-line: in from the box.
        container(
            text("Otherwise the tools are used up")
                .size(11)
                .wrapping(Wrapping::WordOrGlyph)
                .style(theme::faint_text),
        )
        .padding(iced::Padding::ZERO.left(22.0)),
    ]
    .spacing(1);
    let message = if state.enough {
        footer_message(None, state.error, state.checking)
    } else {
        Some(message_text(
            "There’s only one body: make another to combine with",
            theme::danger_text,
        ))
    };
    let summary = match state.tools.len() {
        0 => None,
        1 => Some("1 tool".to_owned()),
        n => Some(format!("{n} tools")),
    }
    .map(|summary| {
        text(summary)
            .size(12)
            .wrapping(Wrapping::None)
            .style(theme::muted_text)
            .into()
    });
    let body = column![
        target,
        tools,
        hrule(),
        heading("Operation"),
        row(ops).spacing(4),
        keep,
    ]
    .spacing(6);
    operation_panel(Parts {
        title: state.editing.unwrap_or("New combine"),
        summary,
        body: body.into(),
        message,
        ok: state.ready.then_some(Message::Edit(Edit::CommitCombine)),
        cancel: Message::Look(Look::Combine(CombineLook::Cancel)),
        close: false,
    })
}

/// A row of the panel with `label` at its top left, as wide as a typed
/// value's, and `field` right of it.
fn pick_row<'a>(label: &'a str, field: Element<'a, Message>) -> Element<'a, Message> {
    row![
        container(text(label).size(12))
            .width(FIELD_INDENT - 6.0)
            .padding(iced::Padding::ZERO.top(5.0)),
        field,
    ]
    .spacing(6)
    .align_y(Alignment::Start)
    .into()
}

/// A field picked into by clicks on bodies, holding `chips`, one under
/// another, and what to click while it has none (and, while it's the one
/// picking, after them if it takes several, `list`). Outlined while it's
/// the one picking (`on`); a click on it makes it so, sending `message`.
fn pick_field<'a>(
    chips: impl Iterator<Item = Element<'a, Message>>,
    list: bool,
    placeholder: &'a str,
    on: bool,
    message: Option<Message>,
) -> Element<'a, Message> {
    let chips: Vec<Element<'a, Message>> = chips.collect();
    let prompt = (chips.is_empty() || (list && on)).then(|| {
        let shown = if chips.is_empty() {
            placeholder.to_owned()
        } else {
            format!("+ {placeholder}")
        };
        container(text(shown).size(12).wrapping(Wrapping::WordOrGlyph).style(
            move |theme: &iced::Theme| text::Style {
                color: Some(if on {
                    theme::palette(theme).accent
                } else {
                    theme::palette(theme).faint
                }),
            },
        ))
        .padding([2, 5])
    });
    button(column(chips).push(prompt).spacing(2).width(Length::Fill))
        .width(Length::Fill)
        .padding(2)
        .style(theme::pick_field(on))
        .on_press_maybe(message)
        .into()
}

/// A body's chip: its name, and a button taking it out sending `drop`, if
/// the document can be changed.
fn chip<'a>(name: &'a str, drop: Option<Message>) -> Element<'a, Message> {
    let close = button(icons::tinted(Icon::Close, 11.0, |palette| palette.faint))
        .padding(2)
        .style(theme::flat_button(false))
        .on_press_maybe(drop);
    container(
        row![
            text(name)
                .size(12)
                .wrapping(Wrapping::None)
                .width(Length::Fill),
            space::horizontal().width(4),
            close,
        ]
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(20)
    .padding(iced::Padding::from([0.0, 2.0]).left(7.0))
    .align_y(Alignment::Center)
    .clip(true)
    .style(theme::chip)
    .into()
}

#[cfg(test)]
mod tests;
