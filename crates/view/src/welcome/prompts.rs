//! The welcome screen's prompts and notes: the damaged file, deleting
//! from browser storage, the panic recorded and browser storage's clearing.

use iced::widget::{Space, button, column, container, row, space, text};
use iced::{Alignment, Element, Font, Length, Padding};
use varde_document::APP_NAME;

use crate::chrome::{self, dialog, dialog_button, dialog_of_width};
use crate::downloads::Downloads;
use crate::icons::{self, Icon};
use crate::theme::{self};
use crate::{Message, Welcome};

/// Asks what to do about a file found damaged as it was opened, before
/// its design shows: open the newest save that can be read, or the one
/// found after the damage, or leave it.
pub struct DamagedPrompt<'a> {
    /// What the file is called: "part.vrdp", or for the store entry of a
    /// new design, "the recovered design".
    pub name: String,
    /// How much of it can't be read, like "12 KB".
    pub unreadable: String,
    /// When the newest save that can be read was saved, like "2 h ago",
    /// lower case, to go in a sentence.
    pub from: String,
    /// When the save found after the damage was saved, likewise, if one
    /// was found.
    pub found: Option<String>,
    /// Whether the file is the store entry of a new design, holding
    /// auto-saves, whose damage the next auto-save cuts off, rather than
    /// a design's own file, which opening leaves as it is.
    pub auto_saves: bool,
    /// Whether the save found is being opened.
    pub opening: bool,
    /// Why opening the save found failed, if it did.
    pub error: Option<&'a str>,
}

/// The panic a session recorded, kept till the user discards it: a note
/// on the welcome screen, and the whole report in a dialog over it while
/// they look at it.
pub struct PanicNote<'a> {
    /// When it happened, like "2 h ago", if that's known.
    pub when: Option<String>,
    /// Its message.
    pub message: &'a str,
    /// The whole of it, while it's shown, to copy, e.g. for a bug report.
    pub report: Option<String>,
}

/// Asks before deleting a design in browser storage whose latest isn't
/// downloaded: it may be the only copy there is.
pub struct DeleteFromBrowserPrompt<'a> {
    /// Its file name there.
    pub name: &'a str,
    /// Where it stands against its downloads.
    pub downloads: Downloads,
}

/// The panic a session recorded, as a card: its message's first line and
/// when, beside buttons to show the whole of it and to discard it.
pub(super) fn panic_card<'a>(note: PanicNote<'a>) -> Element<'a, Message> {
    let said = match &note.when {
        Some(when) => format!("{APP_NAME} ran into an internal error · {when}"),
        None => format!("{APP_NAME} ran into an internal error"),
    };
    let first = note.message.lines().next().unwrap_or_default();
    let clipped = |line: text::Text<'a>| {
        container(line.wrapping(text::Wrapping::None))
            .width(Length::Fill)
            .clip(true)
    };
    container(
        row![
            icons::tinted(Icon::Alert, 20.0, |p| p.warning),
            column![
                clipped(text(first).font(theme::SEMIBOLD)),
                clipped(text(said).size(11.5).style(theme::muted_text)),
            ]
            .spacing(2)
            .width(Length::Fill),
            panic_button("Details…", Welcome::ShowPanic),
            panic_button("Discard", Welcome::DiscardPanic),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([10, 12]).left(14))
    .width(Length::Fill)
    .style(theme::note_card)
    .into()
}

/// A button of the panic's card, as tall as the web page's Start buttons.
pub(super) fn panic_button<'a>(label: &'a str, message: Welcome) -> Element<'a, Message> {
    button(
        container(text(label).font(theme::SEMIBOLD))
            .height(Length::Fill)
            .align_y(Alignment::Center),
    )
    .height(36)
    .padding([0, 14])
    .style(theme::secondary_button)
    .on_press(Message::Welcome(message))
    .into()
}

/// The whole of the panic recorded, `report`, in a dialog over the whole
/// screen, to read and copy.
pub(super) fn panic_report<'a>(report: String) -> Element<'a, Message> {
    let body = container(
        chrome::scrolled(
            container(text(report.clone()).size(12).font(Font::MONOSPACE)).padding(10),
            2.0,
        )
        .height(Length::Shrink),
    )
    .max_height(360)
    .width(Length::Fill)
    .style(theme::text_well);
    let copy = dialog_button("Copy", theme::secondary_button, Some(Message::Copy(report)));
    let close = dialog_button(
        "Close",
        theme::primary_button,
        Some(Message::Welcome(Welcome::ClosePanic)),
    );
    dialog_of_width(
        column![
            text("Internal error").size(14).font(theme::SEMIBOLD),
            text("Copy it into a bug report to help get it fixed.").style(theme::muted_text),
            body,
            Space::new().height(4),
            row![space::horizontal(), copy, close].spacing(8),
        ]
        .spacing(8),
        640.0,
    )
}

/// Asks what to do about a damaged file, see [`DamagedPrompt`], as a
/// dialog over the whole screen like the document screen's prompts.
pub(super) fn damaged_prompt<'a>(prompt: DamagedPrompt<'a>) -> Element<'a, Message> {
    let line = |line: String| text(line).style(theme::muted_text);
    let found = prompt.found.as_ref().map(|found| {
        line(format!(
            "A newer save, from {found}, was found after the damage."
        ))
    });
    let leaves = line(
        if prompt.auto_saves {
            "Opening it cuts off what can't be read at the next auto-save."
        } else {
            "Opening it leaves the file as it is: saving it saves another file."
        }
        .to_owned(),
    );
    let error = prompt.error.map(|error| {
        text(format!("Couldn't open the save found: {error}")).style(theme::danger_text)
    });
    // Nothing else while the save found opens, but leaving it.
    let idle = |message| (!prompt.opening).then_some(message);
    let cancel = dialog_button(
        "Cancel",
        theme::secondary_button,
        Some(Message::Welcome(Welcome::CancelDamaged)),
    );
    let open_found = prompt.found.is_some().then(|| {
        dialog_button(
            if prompt.opening {
                "Opening…"
            } else {
                "Open found save"
            },
            theme::secondary_button,
            idle(Message::Welcome(Welcome::OpenFound)),
        )
    });
    let open = dialog_button(
        "Open",
        theme::primary_button,
        idle(Message::Welcome(Welcome::OpenDamaged)),
    );
    dialog(
        column![
            text("This file is damaged.").size(14).font(theme::SEMIBOLD),
            line(format!(
                "{} of {} can't be read. The newest save that can is from {}.",
                prompt.unreadable, prompt.name, prompt.from
            )),
            found,
            leaves,
            error,
            Space::new().height(4),
            row![space::horizontal(), cancel, open_found, open].spacing(8),
        ]
        .spacing(8),
    )
}

/// The note over what's in browser storage when the browser may clear it,
/// as it hasn't made the site's storage persistent.
pub(super) fn clearing_note<'a>() -> Element<'a, Message> {
    container(
        row![
            icons::tinted(Icon::Alert, 20.0, |p| p.warning),
            column![
                text("This browser may clear what's kept in it").font(theme::SEMIBOLD),
                text(
                    "When it runs short of space, or its site data is cleared. Download the \
                     designs that matter to keep them."
                )
                .size(11.5)
                .style(theme::muted_text),
            ]
            .spacing(2)
            .width(Length::Fill),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([10, 12]).left(14))
    .width(Length::Fill)
    .style(theme::note_card)
    .into()
}

/// Asks before deleting a design in browser storage, see
/// [`DeleteFromBrowserPrompt`].
pub(super) fn delete_from_browser_prompt<'a>(
    prompt: DeleteFromBrowserPrompt<'a>,
) -> Element<'a, Message> {
    let why = match prompt.downloads {
        Downloads::Never => "It has never been downloaded: this browser has the only copy.",
        _ => {
            "It has changed since it was last downloaded: this browser has the only copy of the changes."
        }
    };
    dialog(
        column![
            text(format!("Delete {}?", prompt.name))
                .size(14)
                .font(theme::SEMIBOLD),
            text(why).style(theme::muted_text),
            Space::new().height(4),
            row![
                space::horizontal(),
                dialog_button(
                    "Cancel",
                    theme::secondary_button,
                    Some(Message::Welcome(Welcome::CancelDelete)),
                ),
                dialog_button(
                    "Delete",
                    theme::danger_button,
                    Some(Message::Welcome(Welcome::ConfirmDelete)),
                ),
            ]
            .spacing(8),
        ]
        .spacing(8),
    )
}
