//! The welcome screen, shown when no document is open: buttons to start a
//! design, and the recovered, downloaded and recent designs.

use std::path::Path;

use iced::widget::{Space, button, column, container, grid, hover, row, stack, text};
use iced::{Alignment, Element, Length, Padding};
use varde_document::APP_NAME;

use crate::chrome::{self, ChipSize, key_hint};
use crate::icons::{self, Icon};
use crate::shortcut::{Binding, welcome_bindings};
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
    pub mode: theme::Mode,
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
}

/// The screen shown when no document is open: buttons to start a design
/// and the recently opened files.
pub fn welcome<'a>(state: WelcomeState<'a>) -> Element<'a, Message> {
    let [new, open] = welcome_bindings();
    let hints = [
        key_hint(new.shortcut, "New design"),
        key_hint(open.shortcut, "Open"),
    ];
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
    let content = column![section_label("Start"), start,]
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
        container(chrome::app_buttons(state.mode))
            .align_right(Length::Fill)
            .padding(8),
    ];
    let status = status_bar(Status {
        selection: None,
        info: None,
        hints: hints.into(),
        mouse_hints: true,
        view_menu: None,
    });
    chrome::window(page, status)
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
        icons::tinted(icon, icons::INLINE, move |p| emphasis.content(p)),
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
        Message::Welcome(Welcome::OpenPath(file.path.to_owned())),
    )
}

/// A design's changes left behind by a session that crashed: a card like a
/// recent file's, opening it, and a button under it to delete it.
fn recovered_card<'a>(design: StoredDesign<'a>) -> Element<'a, Message> {
    let saved = design.written.map_or_else(
        || "Auto-saved".to_owned(),
        |saved| format!("Auto-saved · {saved}"),
    );
    let meta = || {
        column![
            text(design.name.clone()).font(theme::SEMIBOLD),
            text(saved.clone())
                .size(11.5)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(2)
        .into()
    };
    let card = card(
        40.0,
        96.0,
        meta,
        Message::Welcome(Welcome::OpenStored(design.path.to_owned())),
    );
    column![card, discard_button(design.path)].spacing(6).into()
}

/// A card on the welcome screen: a thumbnail over the rows `meta` builds.
/// Clicking it sends `on_press`.
fn card<'a>(
    icon_size: f32,
    thumbnail_height: f32,
    meta: impl Fn() -> Element<'a, Message>,
    on_press: Message,
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
        .on_press(on_press)
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
    let when = design.written.map_or_else(
        || "Downloaded".to_owned(),
        |saved| format!("Downloaded · {saved}"),
    );
    let open = button(
        row![
            icons::tinted(Icon::Body, icons::INLINE, |p| p.muted),
            container(text(design.name).wrapping(text::Wrapping::None))
                .width(Length::Fill)
                .clip(true),
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
    .on_press(Message::Welcome(Welcome::OpenStored(
        design.path.to_owned(),
    )));
    row![open, discard_button(design.path)]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
}
