//! The combine being set up: what the app hands the view of it, the
//! messages changing it, and its floating panel over the right of the
//! viewport. Its bodies are picked in the viewport, where the cursor picks
//! the model as outside the sessions and a click picks the body of what
//! it's on, or in Objects; the panel shows them as rows, the target in
//! one field and the tools in another, each with a cross taking it out.

use iced::Element;
use iced::widget::column;
use varde_document::{BodyId, BodyOp};

use crate::icons::Icon;
use crate::operation_panel::{
    Footer, Framing, PanelHover, Parts, field, footer_message, message_text, operation_panel,
    pick_field, picked_row, tile, tiles, toggle,
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
    /// The tools, sorted by id as the document keeps them.
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
    /// The button framing the camera on the geometry of why the preview
    /// failed, or going back from it, if that geometry has a box: beside
    /// [`CombineState::error`]'s Add anyway.
    pub show_error: Option<Framing>,
    /// Whether sketch edits have waited on the solver long enough to say
    /// so: OK waits for them, and the panel says why.
    pub checking: bool,
    /// Whether OK (and `Enter`) can be pressed: not while the preview
    /// failed.
    pub ready: bool,
    /// Whether Accept error can be pressed: the preview failed
    /// ([`CombineState::error`]), and the operation could be committed otherwise.
    pub accept: bool,
    /// Whether the document can be changed.
    pub editable: bool,
    /// The row of the panel the cursor is over, if any: the viewport
    /// lights it up too.
    pub hover: Option<PanelHover>,
}

/// The operations a combine does, as the panel offers them.
const OPS: [BodyOp; 3] = [BodyOp::Union, BodyOp::Subtract, BodyOp::Intersect];

/// The floating panel of the combine being set up.
pub(crate) fn panel<'a>(state: &CombineState<'a>) -> Element<'a, Message> {
    let editable = state.editable;
    let send = |look: CombineLook| editable.then_some(Message::Look(Look::Combine(look)));
    let drop = |body: BodyId| send(CombineLook::Drop(body));

    // Each field's bodies as rows, and what to click while it has none
    // (and, while it's the one picking, after them if it takes several,
    // `list`).
    let bodies_field = |picked: &[CombineBody<'a>], list: bool, place: &str, pick: CombinePick| {
        let on = state.picking == pick;
        let message = send(CombineLook::Picking(pick));
        let rows: Vec<_> = (picked.iter())
            .map(|body| {
                picked_row(
                    Icon::Body,
                    body.name,
                    None,
                    drop(body.body),
                    message.clone(),
                    PanelHover::Body(body.body),
                    state.hover,
                )
            })
            .collect();
        let place = (rows.is_empty() || (list && on)).then(|| place.to_owned());
        pick_field(rows, place, on, message)
    };
    let target = field(
        "Target",
        bodies_field(
            state.target.as_slice(),
            false,
            "Click a body",
            CombinePick::Target,
        ),
    );
    let tools = field(
        "Tools",
        bodies_field(&state.tools, true, "Click bodies", CombinePick::Tools),
    );
    let ops = OPS.map(|op| {
        tile(
            op_icon(op),
            op.label(),
            state.op == op,
            send(CombineLook::Operation(op)),
        )
    });
    let keep = toggle(
        Icon::TkKeep,
        "Keep tool bodies",
        state.keep_tools,
        send(CombineLook::KeepTools),
        Some("Otherwise the tools are used up"),
    );
    let message = if state.enough {
        footer_message(
            "Combine",
            None,
            state.error,
            state.show_error,
            state.accept.then_some(Message::Edit(Edit::AcceptError)),
            state.checking,
        )
    } else {
        Some(Footer::Text(message_text(
            "There’s only one body: make another to combine with",
            theme::danger_text,
        )))
    };
    let body = column![target, tools, field("Operation", tiles(ops)), keep].spacing(10);
    operation_panel(Parts {
        icon: Icon::Combine,
        title: state.editing.unwrap_or("New combine"),
        body: body.into(),
        message,
        ok: state.ready.then_some(Message::Edit(Edit::CommitCombine)),
        cancel: Message::Look(Look::Combine(CombineLook::Cancel)),
        close: false,
    })
}

/// The icon of `op`'s choice: the booleans as circles.
fn op_icon(op: BodyOp) -> Icon {
    match op {
        BodyOp::Union => Icon::BoJoin,
        BodyOp::Subtract => Icon::BoCut,
        BodyOp::Intersect => Icon::BoInt,
    }
}

#[cfg(test)]
mod tests;
