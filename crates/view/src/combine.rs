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
    Footer, ListRow, OperationKind, PanelHover, Parts, Verdict, field, message_text,
    operation_panel, pick_field, tile, tiles, toggle,
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
    /// Its title, OK and foot.
    pub verdict: Verdict<'a>,
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
    /// Whether the document can be changed.
    pub editable: bool,
    /// The row of the panel the cursor is over, if any: the viewport
    /// lights it up too.
    pub hover: Option<PanelHover>,
}

impl<'a> CombineState<'a> {
    /// The panel's title: the feature edited's name, or "New combine".
    pub(crate) fn title(&self) -> &'a str {
        self.verdict.title("New combine")
    }

    /// What the picking asks for, for the hint bar.
    pub(crate) fn pick_hint(&self) -> &'static str {
        match self.picking {
            CombinePick::Target => "Pick the target",
            CombinePick::Tools => "Pick tools",
        }
    }

    /// What the status bar says of it: "Body 1 with 2 tools · Union", or
    /// what's still to pick.
    pub(crate) fn status_info(&self) -> String {
        let Some(target) = self.target else {
            return "pick the target body".to_owned();
        };
        match self.tools.len() {
            0 => format!("{} · pick the tool bodies", target.name),
            n => format!(
                "{} with {} · {}",
                target.name,
                crate::words::counted(n, "tool", "tools"),
                self.op.label()
            ),
        }
    }
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
                ListRow::new(Icon::Body, body.name, PanelHover::Body(body.body))
                    .remove(drop(body.body))
                    .press(message.clone())
                    .view(state.hover)
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
            OperationKind::from(op).icon(),
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
        state.verdict.footer("Combine")
    } else {
        Some(Footer::Text(message_text(
            "There’s only one body: make another to combine with",
            theme::danger_text,
        )))
    };
    let body = column![target, tools, field("Operation", tiles(ops)), keep].spacing(10);
    operation_panel(Parts {
        icon: Icon::Combine,
        title: state.title(),
        body: body.into(),
        message,
        ok: state.verdict.ok(Message::Edit(Edit::CommitCombine)),
        cancel: Message::Look(Look::Combine(CombineLook::Cancel)),
        close: false,
    })
}

#[cfg(test)]
mod tests;
