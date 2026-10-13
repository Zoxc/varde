//! The floating panel an operation is set up in, over the right of the
//! viewport, as the tool rail's cards are (the UI mock's `.opp`): an
//! accent line along its top, a head with the operation's icon and title
//! and its Cancel and OK as small square buttons, and a recessed well
//! under it holding the body with the options and, at its foot, the
//! message (why OK waits, or the draft failing, with Add anyway). The
//! head and the foot always show; the body scrolls when the panel would
//! run past the room it has, so OK and Cancel stay on screen however many
//! options there are or however short the window is.
//!
//! The extrude (`extrude::panel`), the revolve (`revolve::panel`) and the
//! combine (`combine::panel`) are set up in it, and the measure tool
//! shows its values in it (`measure::panel`, with only Close), from the
//! parts here they share: what a session hands the view of the sketches
//! whose regions it picks, the labels, choices as tiles, options as icon
//! toggles, fields picked into by clicks in the viewport, typed fields,
//! the Bodies list of a join, cut or intersect, and the foot's message.
//! Its controls are the Timeline's rows' size: 28 px tall, 12.5 px words,
//! 8 px in.

use std::borrow::Cow;

use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, opaque, row, text};
use iced::{Alignment, Element, Length, Padding};

use crate::chrome::{scrolled, sentence};
use crate::icons::{self, Icon};
use crate::theme::{self, SEMIBOLD};
use crate::{Edit, Message};

mod card;
mod controls;
mod model;

pub(crate) use card::*;
pub(crate) use controls::*;
pub use model::*;

/// The scrollable holding the panel's body.
pub const PANEL_BODY: iced::widget::Id = iced::widget::Id::new("operation-panel-body");

/// The head's buttons, which have no words of their own: what each is
/// called, which the tests' probe reports as their text.
pub(crate) const OK_BUTTON: iced::widget::Id = iced::widget::Id::new("OK");
pub(crate) const CANCEL_BUTTON: iced::widget::Id = iced::widget::Id::new("Cancel");
pub(crate) const CLOSE_BUTTON: iced::widget::Id = iced::widget::Id::new("Close");
/// The parameters popup's close button, its own id as the popup can
/// show beside a panel with [`CLOSE_BUTTON`].
pub(crate) const PARAMS_CLOSE: iced::widget::Id = iced::widget::Id::new("Close parameters");

/// The head's buttons and what each is called, see [`OK_BUTTON`].
#[cfg(any(test, feature = "probe"))]
pub(crate) const BUTTON_NAMES: [(iced::widget::Id, &str); 4] = [
    (OK_BUTTON, "OK"),
    (CANCEL_BUTTON, "Cancel"),
    (CLOSE_BUTTON, "Close"),
    (PARAMS_CLOSE, "Close"),
];

/// How tall a message at the foot gets at most, in pixels: about five
/// lines. A longer one scrolls, so it can't push the body away and none
/// of it is lost.
const MESSAGE_HEIGHT: f32 = 80.0;

/// What an operation's panel shows.
pub(crate) struct Parts<'a> {
    /// The operation's icon, before its title.
    pub icon: Icon,
    /// The operation's name, or the feature edited: one line, clipped.
    pub title: &'a str,
    /// The options, scrolled when they don't fit.
    pub body: Element<'a, Message>,
    /// At the well's foot: why OK can't be pressed, say, or the draft
    /// failing.
    pub message: Option<Footer<'a>>,
    /// What OK sends, or nothing while it can't be pressed.
    pub ok: Option<Message>,
    /// What Cancel sends.
    pub cancel: Message,
    /// Whether the head has only a Close button, sending `cancel`, in
    /// place of Cancel and OK: for a tool that changes nothing, as the
    /// measure tool.
    pub close: bool,
}

/// The message at the panel's foot.
pub(crate) enum Footer<'a> {
    /// Words (see [`message_text`]), scrolled past about five lines.
    Text(Element<'a, Message>),
    /// The draft failing: "Extrude fails" (`noun`) over why, with
    /// `show`'s button, if its geometry has a box, and Add anyway sending
    /// `accept`, if it can be pressed, which keeps the operation with its
    /// error (marked failed in the Timeline, to fix later). OK waits
    /// meanwhile.
    Fails {
        noun: &'a str,
        error: Cow<'a, str>,
        show: Option<Framing>,
        accept: Option<Message>,
        /// Whether a feature already there is edited: Edit anyway then.
        editing: bool,
    },
}

/// The panel showing `parts`. It's `opaque`: clicks and the wheel over it
/// don't reach the scene under it.
pub(crate) fn operation_panel(parts: Parts<'_>) -> Element<'_, Message> {
    let Parts {
        icon,
        title,
        body,
        message,
        ok,
        cancel,
        close,
    } = parts;
    let title = container(text(title).size(13).font(SEMIBOLD).wrapping(Wrapping::None))
        .width(Length::Fill)
        .clip(true);
    let buttons: Element<'_, Message> = if close {
        head_button(
            Icon::Cancel,
            CLOSE_BUTTON,
            false,
            Some(cancel),
            "Close (Esc)",
        )
    } else {
        row![
            head_button(
                Icon::Cancel,
                CANCEL_BUTTON,
                false,
                Some(cancel),
                "Cancel (Esc)"
            ),
            head_button(Icon::Confirm, OK_BUTTON, true, ok, "OK (Enter)"),
        ]
        .spacing(6)
        .into()
    };
    let header = container(
        row![icons::icon(icon, icons::INLINE), title, buttons]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .padding(Padding::from([7.0, 6.0]).left(8.0));
    // Both scrollbars float in the right padding, clear of the text.
    let margin = (SIDE - theme::SCROLLBAR_WIDTH) / 2.0;
    let body = scrolled(
        container(body)
            .width(Length::Fill)
            .padding(Padding::from([6.0, SIDE]).bottom(10.0)),
        margin,
    )
    .id(PANEL_BODY)
    .width(Length::Fill);
    let footer: Element<'_, Message> = match message {
        None => Space::new().into(),
        Some(Footer::Text(message)) => container(
            container(
                scrolled(
                    container(message).width(Length::Fill).padding([0.0, SIDE]),
                    margin,
                )
                .width(Length::Fill),
            )
            .max_height(MESSAGE_HEIGHT),
        )
        .padding(Padding::ZERO.top(2.0).bottom(8.0))
        .into(),
        Some(Footer::Fails {
            noun,
            error,
            show,
            accept,
            editing,
        }) => container(fail_box(noun, error, show, accept, editing))
            .padding(Padding::from([0.0, SIDE]).top(2.0).bottom(8.0))
            .into(),
    };
    let sections = Sections {
        width: PANEL_WIDTH,
        parts: [header.into(), body.into(), footer],
        well: true,
    };
    opaque(sections)
}

/// The box of a draft that fails, at the foot: a title, "Extrude fails"
/// (`noun`), with the alert in the danger colour on its wash, `error`
/// under it in muted words, scrolled past about five lines, and under
/// that `show`'s button at the left, if there's geometry to frame, and
/// Add anyway at the right sending `accept`, if it can be pressed: Edit
/// anyway while `editing` a feature already there.
fn fail_box<'a>(
    noun: &'a str,
    error: Cow<'a, str>,
    show: Option<Framing>,
    accept: Option<Message>,
    editing: bool,
) -> Element<'a, Message> {
    let title = container(
        row![
            icons::tinted(Icon::Alert, 14.0, |p| p.danger),
            container(
                text(format!("{noun} fails"))
                    .size(CONTROL_TEXT)
                    .font(SEMIBOLD)
                    .wrapping(Wrapping::WordOrGlyph),
            )
            .width(Length::Fill),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([5, 8])
    .style(theme::fail_title);
    let words = container(
        scrolled(
            container(
                text(error)
                    .size(CONTROL_TEXT)
                    .wrapping(Wrapping::WordOrGlyph)
                    .style(theme::muted_text),
            )
            .width(Length::Fill)
            .padding(Padding::from([5.0, 8.0]).bottom(7.0)),
            2.0,
        )
        .width(Length::Fill),
    )
    .max_height(MESSAGE_HEIGHT);
    let show = show.map(|show| {
        let (icon, words, message) = show.button();
        let content = row![
            icons::tinted(icon, 14.0, |p| p.text),
            text(words).size(CONTROL_TEXT).font(SEMIBOLD),
        ]
        .spacing(5)
        .align_y(Alignment::Center);
        fail_button(content.into(), false, message)
    });
    let add = accept.map(|accept| {
        fail_button(
            text(if editing { "Edit anyway" } else { "Add anyway" })
                .size(CONTROL_TEXT)
                .font(SEMIBOLD)
                .into(),
            true,
            accept,
        )
    });
    let buttons = (show.is_some() || add.is_some()).then(|| {
        container(row![show, Space::new().width(Length::Fill), add].spacing(6))
            .width(Length::Fill)
            .padding(Padding::from([0.0, 8.0]).bottom(8.0))
    });
    container(column![title, words, buttons])
        .width(Length::Fill)
        .style(theme::fail_box)
        .into()
}

/// A button in a [`fail_box`] showing `content`, its words in the danger
/// colour if `danger`, sending `message`.
fn fail_button(
    content: Element<'_, Message>,
    danger: bool,
    message: Message,
) -> Element<'_, Message> {
    button(content)
        .padding([5, 12])
        .style(theme::fail_button(danger))
        .on_press(message)
        .into()
}

/// How an operation being set up stands, as its panel's title, OK and
/// foot show it: the fields every operation's state shares.
#[derive(Debug, Clone, Default)]
pub struct Verdict<'a> {
    /// The name of the feature edited, or none for a new one.
    pub editing: Option<&'a str>,
    /// Why the operation as set up can't be committed, if its own check
    /// refuses it (an extrude's two sides over the limit together, say):
    /// shown in place of [`Verdict::error`].
    pub refused: Option<String>,
    /// Why the preview failed, if it did.
    pub error: Option<&'a str>,
    /// The button framing the camera on the geometry of why the preview
    /// failed, or going back from it, if that geometry has a box: beside
    /// [`Verdict::error`]'s Add anyway.
    pub show_error: Option<Framing>,
    /// What the panel warns of where nothing else is said, if anything:
    /// a cut whose preview works but takes nothing from a body ("Body 2:
    /// nothing to cut"), a pattern's copies overlapping.
    pub warning: Option<String>,
    /// Whether sketch edits have waited on the solver long enough to say
    /// so: OK waits for them, and the panel says why.
    pub checking: bool,
    /// Whether OK (and `Enter`) can be pressed: not while the preview
    /// failed.
    pub ready: bool,
    /// Whether Add anyway can be pressed: the preview failed
    /// ([`Verdict::error`]), and the operation could be committed otherwise.
    pub accept: bool,
}

impl<'a> Verdict<'a> {
    /// The foot's message: why OK can't be pressed ([`Verdict::refused`]),
    /// else the draft failing, as "Extrude fails" (`noun`) with the
    /// framing button and Add anyway (Edit anyway while editing a feature
    /// already there), else, if checking, that OK waits on the solver,
    /// else the warning.
    pub(crate) fn footer(&self, noun: &'a str) -> Option<Footer<'a>> {
        let editing = self.editing.is_some();
        match (&self.refused, self.error) {
            // As the kernel's, but with nothing to add: the document would
            // refuse it.
            (Some(refused), _) => Some(Footer::Fails {
                noun,
                error: sentence(refused).into_owned().into(),
                show: None,
                accept: None,
                editing,
            }),
            (None, Some(error)) => Some(Footer::Fails {
                noun,
                error: sentence(error),
                show: self.show_error,
                accept: self.accept.then_some(Message::Edit(Edit::AcceptError)),
                editing,
            }),
            (None, None) if self.checking => Some(Footer::Text(message_text(
                "Checking the sketch…",
                theme::muted_text,
            ))),
            (None, None) => (self.warning.clone())
                .map(|warning| Footer::Text(message_text(warning, theme::warning_text))),
        }
    }

    /// The panel's title: the feature edited's name, else `new`.
    pub(crate) fn title(&self, new: &'a str) -> &'a str {
        self.editing.unwrap_or(new)
    }

    /// OK's message, `commit`, if it can be pressed.
    pub(crate) fn ok(&self, commit: Message) -> Option<Message> {
        self.ready.then_some(commit)
    }
}

#[cfg(test)]
mod tests;
