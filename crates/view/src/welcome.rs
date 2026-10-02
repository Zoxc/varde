//! The welcome screen, shown when no document is open: buttons to start a
//! design, and the recovered, downloaded and recent designs.

use std::path::Path;
use std::sync::LazyLock;

use iced::widget::{Space, button, column, container, grid, hover, row, space, stack, svg, text};
use iced::{Alignment, Element, Length, Padding};
use varde_document::APP_NAME;

use crate::chrome::{self, ChipSize, dialog, dialog_button, key_hint};
use crate::icons::{self, Icon, LOGO_SVG};
use crate::shortcut::{Binding, Shortcut, welcome_bindings};
use crate::status::{Status, status_bar};
use crate::theme::{self, Emphasis};
use crate::{Message, Welcome};

/// Borrowed state needed to build the welcome screen.
pub struct WelcomeState<'a> {
    /// Why the last open failed, if it did.
    pub error: Option<&'a str>,
    /// Recently opened files, newest first; `None` where there's no such
    /// list (the web).
    pub recent: Option<Vec<RecentCard<'a>>>,
    /// New designs left behind by sessions that crashed, newest first.
    pub recovered: Vec<StoredDesign<'a>>,
    /// On the web, designs downloaded and closed since, newest first: kept
    /// in case the download didn't finish, which the page isn't told.
    pub downloaded: Vec<StoredDesign<'a>>,
    /// The damaged file just opened, asked about before its design shows,
    /// if one is.
    pub damaged: Option<DamagedPrompt<'a>>,
    pub mode: theme::Mode,
    /// The theme chosen, which the theme button shows.
    pub theme: theme::ThemeChoice,
}

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

/// A card in the welcome screen's recent files.
pub struct RecentCard<'a> {
    pub path: &'a Path,
    /// The file's name, as the design is known by.
    pub name: String,
    /// The file's extension with its dot, like ".vrdp", or nothing.
    pub extension: String,
    /// The directory holding the file, for display.
    pub dir: String,
    /// When the file was opened, like "2 h ago".
    pub opened: String,
    /// Whether the file is there, as far as is known. One that isn't may
    /// be on a drive that isn't mounted just now; it's still listed.
    pub available: bool,
}

/// A card in the welcome screen's recovered designs, or a row in its
/// downloaded ones.
pub struct StoredDesign<'a> {
    pub path: &'a Path,
    /// The design's name: `Untitled`, or that of the file it was opened
    /// from, as on the web. For a downloaded one, the name it was
    /// downloaded as.
    pub name: String,
    /// When it was last written to the store, like "2 h ago", if that's
    /// known: auto-saved for a recovered design, downloaded for a
    /// downloaded one.
    pub written: Option<String>,
    /// Whether its entry is damaged: marked so.
    pub damaged: bool,
    /// Whether it opens: not if it's damaged so that nothing of it can be
    /// read, which can only be discarded.
    pub opens: bool,
}

/// The screen shown when no document is open: buttons to start a design
/// and the recently opened files.
pub fn welcome<'a>(state: WelcomeState<'a>) -> Element<'a, Message> {
    let [new, open] = welcome_bindings();
    // The prompt takes every key but `Esc`.
    let hints = if state.damaged.is_some() {
        vec![key_hint(Shortcut::ESCAPE, "Cancel")]
    } else {
        vec![
            key_hint(new.shortcut, "New design"),
            key_hint(open.shortcut, "Open"),
        ]
    };
    let new = welcome_button(Icon::Plus, "New design", new, Emphasis::Primary);
    let open = welcome_button(Icon::Folder, "Open…", open, Emphasis::Secondary);
    let mut start = column![row![new, open].spacing(10)].spacing(10);
    if let Some(error) = state.error {
        start = start.push(text(error).style(theme::danger_text));
    }

    let recovered = (!state.recovered.is_empty()).then(|| {
        section(
            "Recovered designs",
            [
                text(format!(
                    "Changes never saved, left when {APP_NAME} last closed unexpectedly"
                ))
                .style(theme::muted_text)
                .into(),
                grid(state.recovered.into_iter().map(recovered_card))
                    .fluid(RECENT_CARD_WIDTH)
                    .spacing(14)
                    .height(Length::Shrink)
                    .into(),
            ],
        )
    });

    let downloaded = (!state.downloaded.is_empty()).then(|| {
        section(
            "Downloaded",
            [
                text("Kept in case a download didn't finish: the browser doesn't say")
                    .style(theme::muted_text)
                    .into(),
                column(state.downloaded.into_iter().map(downloaded_row))
                    .spacing(4)
                    .into(),
            ],
        )
    });

    let recent = state
        .recent
        .map(|recent| section("Recent", [recent_grid(recent)]));
    let content = column![]
        .push(heading())
        .push(section_label("Start"))
        .push(start)
        .push(recovered)
        .push(downloaded)
        .push(recent)
        .spacing(12)
        // Fill up to the maximum, so the page doesn't shift with its content.
        .width(Length::Fill)
        .max_width(920)
        .padding([48, 40]);

    let page = stack![
        container(chrome::scrolled(
            container(content).center_x(Length::Fill),
            2.0
        ))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::welcome),
        container(chrome::app_buttons(state.theme))
            .align_right(Length::Fill)
            .padding(8),
    ];
    let status = status_bar(Status {
        selection: None,
        info: None,
        hints,
        mouse_hints: true,
        view_menu: None,
    });
    let window = chrome::window(page, status);
    match state.damaged {
        Some(prompt) => stack![window, damaged_prompt(prompt)].into(),
        None => window,
    }
}

/// Asks what to do about a damaged file, see [`DamagedPrompt`], as a
/// dialog over the whole screen like the document screen's prompts.
fn damaged_prompt<'a>(prompt: DamagedPrompt<'a>) -> Element<'a, Message> {
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

/// The logo and the app's name over the page, on the web only: there the
/// browser tab is the only other place showing them, where natively the
/// window's title bar does.
fn heading<'a>() -> Option<Element<'a, Message>> {
    cfg!(target_arch = "wasm32").then(|| {
        static LOGO: LazyLock<svg::Handle> =
            LazyLock::new(|| svg::Handle::from_memory(LOGO_SVG.as_bytes()));
        column![
            row![
                svg(LOGO.clone()).width(40).height(40),
                text(APP_NAME).size(30).font(theme::SEMIBOLD),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            Space::new().height(36 - 12),
        ]
        .into()
    })
}

/// The widest a recent file card gets.
const RECENT_CARD_WIDTH: f32 = 220.0;

/// A welcome screen section after the first: a gap above its label, then
/// its `children`.
fn section<'a>(
    label: &'a str,
    children: impl IntoIterator<Item = Element<'a, Message>>,
) -> Element<'a, Message> {
    column![Space::new().height(36 - 12), section_label(label)]
        .extend(children)
        .spacing(12)
        .into()
}

/// A small caps heading over a welcome screen section.
fn section_label<'a>(label: &'a str) -> Element<'a, Message> {
    text(label.to_uppercase())
        .size(11)
        .font(theme::SEMIBOLD)
        .style(theme::faint_text)
        .into()
}

/// A welcome screen button: an icon, a label and the key that presses it,
/// sending what `binding` does.
fn welcome_button<'a>(
    icon: Icon,
    label: &'a str,
    binding: Binding,
    emphasis: Emphasis,
) -> Element<'a, Message> {
    let content = row![
        // On the primary colour in its content's, else in its own.
        if emphasis == Emphasis::Secondary {
            icons::icon(icon, icons::INLINE)
        } else {
            icons::tinted(icon, icons::INLINE, move |p| emphasis.content(p)).into()
        },
        text(label).font(theme::SEMIBOLD),
        container(chrome::key_chip(binding.shortcut, ChipSize::Normal).style(emphasis.key_chip()))
            .padding(Padding::ZERO.left(4)),
    ]
    .spacing(8)
    .height(Length::Fill)
    .align_y(Alignment::Center);
    button(content)
        .height(36)
        .padding(Padding::from([0, 14]).left(12))
        .style(emphasis.button_style())
        .on_press_maybe(binding.sends())
        .into()
}

/// The recently opened files' cards, or a note on where they will show.
fn recent_grid(recent: Vec<RecentCard<'_>>) -> Element<'_, Message> {
    if recent.is_empty() {
        text("Files you open show up here")
            .style(theme::muted_text)
            .into()
    } else {
        // At most four columns fit the page's width.
        grid(recent.into_iter().map(recent_card))
            .fluid(RECENT_CARD_WIDTH)
            .spacing(14)
            .height(Length::Shrink)
            .into()
    }
}

/// A recently opened file: a thumbnail over its name, directory and when it
/// was opened. Clicking it opens the file.
fn recent_card<'a>(file: RecentCard<'a>) -> Element<'a, Message> {
    let meta = || {
        let clipped =
            |content: Element<'a, Message>| container(content).width(Length::Fill).clip(true);
        column![
            clipped(
                row![
                    text(file.name.clone())
                        .font(theme::SEMIBOLD)
                        .wrapping(text::Wrapping::None),
                    text(file.extension.clone())
                        .style(theme::faint_text)
                        .wrapping(text::Wrapping::None),
                ]
                .into()
            ),
            row![
                clipped(
                    text(file.dir.clone())
                        .size(11.5)
                        .wrapping(text::Wrapping::None)
                        .into()
                ),
                text(if file.available {
                    file.opened.clone()
                } else {
                    "Unavailable".to_owned()
                })
                .size(11.5)
                .wrapping(text::Wrapping::None),
            ]
            .spacing(8),
        ]
        .spacing(2)
        .into()
    };
    card(
        56.0,
        150.0,
        meta,
        Some(Message::Welcome(Welcome::OpenPath(file.path.to_owned()))),
    )
}

/// A design's changes left behind by a session that crashed: a card like a
/// recent file's, opening it, and a button under it to delete it.
fn recovered_card<'a>(design: StoredDesign<'a>) -> Element<'a, Message> {
    let saved = design.written.as_ref().map_or_else(
        || "Auto-saved".to_owned(),
        |saved| format!("Auto-saved · {saved}"),
    );
    let damage = damage_note(&design);
    let meta = || {
        column![
            text(design.name.clone()).font(theme::SEMIBOLD),
            text(saved.clone())
                .size(11.5)
                .wrapping(text::Wrapping::None),
            damage.map(|note| {
                text(note)
                    .size(11.5)
                    .style(theme::warning_text)
                    .wrapping(text::Wrapping::None)
            }),
        ]
        .spacing(2)
        .into()
    };
    let card = card(
        40.0,
        96.0,
        meta,
        design
            .opens
            .then(|| Message::Welcome(Welcome::OpenStored(design.path.to_owned()))),
    );
    column![card, discard_button(design.path)].spacing(6).into()
}

/// What a recovered or downloaded design's card or row says of its
/// entry's damage, if it's damaged.
fn damage_note(design: &StoredDesign<'_>) -> Option<&'static str> {
    match (design.damaged, design.opens) {
        (false, _) => None,
        (true, true) => Some("Damaged"),
        (true, false) => Some("Damaged, can't be read"),
    }
}

/// A card on the welcome screen: a thumbnail over the rows `meta` builds.
/// Clicking it sends `on_press`, if there's that.
fn card<'a>(
    icon_size: f32,
    thumbnail_height: f32,
    meta: impl Fn() -> Element<'a, Message>,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    let content = |hovered: bool| {
        // No real thumbnails yet: a large body icon stands in.
        let thumbnail = container(icons::tinted(Icon::Body, icon_size, |p| p.muted))
            .center_x(Length::Fill)
            .center_y(thumbnail_height)
            .style(theme::card_thumbnail);
        column![
            thumbnail,
            chrome::hrule(),
            container(meta())
                .width(Length::Fill)
                .padding(Padding::from([10, 12]).bottom(11))
                .style(move |theme| theme::card_meta(theme, hovered)),
        ]
    };

    // `hover` swaps in the highlighted content anywhere over the card, not
    // just over the text.
    button(hover(content(false), content(true)))
        .padding(1)
        .width(Length::Fill)
        .style(theme::card)
        .on_press_maybe(on_press)
        .into()
}

/// The button deleting a recovered or downloaded design.
fn discard_button<'a>(path: &Path) -> Element<'a, Message> {
    chrome::small_button("Discard", Emphasis::Secondary)
        .on_press(Message::Welcome(Welcome::DiscardStored(path.to_owned())))
        .into()
}

/// A design downloaded on the web and closed since: quieter than a
/// recovered design's card, a row with its name and when it was downloaded,
/// opening it, and a button beside it to delete it.
fn downloaded_row<'a>(design: StoredDesign<'a>) -> Element<'a, Message> {
    let when = design.written.as_ref().map_or_else(
        || "Downloaded".to_owned(),
        |saved| format!("Downloaded · {saved}"),
    );
    let damage = damage_note(&design).map(|note| {
        text(note)
            .size(11.5)
            .style(theme::warning_text)
            .wrapping(text::Wrapping::None)
    });
    let open = button(
        row![
            icons::tinted(Icon::Body, icons::INLINE, |p| p.muted),
            container(text(design.name).wrapping(text::Wrapping::None))
                .width(Length::Fill)
                .clip(true),
            damage,
            text(when)
                .size(11.5)
                .style(theme::muted_text)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding([6, 10])
    .width(Length::Fill)
    .style(theme::file_cell(false))
    .on_press_maybe(
        design
            .opens
            .then(|| Message::Welcome(Welcome::OpenStored(design.path.to_owned()))),
    );
    row![open, discard_button(design.path)]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
}
