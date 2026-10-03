//! The welcome screen, shown when no document is open: buttons to start a
//! design, and the designs to open, laid out as `notes/ui-mock/welcome.html`
//! has it. Natively a grey column with the logo, the Start buttons and a
//! foot of help, beside the recovered and recent designs; on the web, where
//! there are no recent files, a header bar with the logo and the buttons
//! over what's kept in browser storage, kept to the middle of the page.

use std::path::Path;
use std::sync::LazyLock;

use iced::widget::{
    Space, button, column, container, grid, hover, image, opaque, responsive, row, space, stack,
    svg, text,
};
use iced::{Alignment, ContentFit, Element, Font, Length, Padding, Size, Theme};
use varde_document::{APP_NAME, EXTENSION};

use crate::chrome::{self, ChipSize, dialog, dialog_button, dialog_of_width};
use crate::icons::{self, Icon, LOGO_SVG};
use crate::shortcut::{Binding, welcome_bindings};
use crate::theme::{self, Emphasis, Palette, Tone};
use crate::{Message, ThemeChoice, Welcome};

/// Borrowed state needed to build the welcome screen.
pub struct WelcomeState<'a> {
    /// Why the last open failed, if it did.
    pub error: Option<&'a str>,
    /// Recently opened files, newest first; `None` where there's no such
    /// list (the web), which lays the screen out as the web's page.
    pub recent: Option<Vec<RecentCard<'a>>>,
    /// The designs kept in the store, newest first: natively new designs
    /// left behind by sessions that crashed, on the web the designs saved
    /// in browser storage among them.
    pub stored: Vec<DesignCard<'a>>,
    /// On the web, what's said of browser storage: whether the browser
    /// keeps it for good, and how much is used.
    pub storage: Option<StorageNote>,
    /// The design in browser storage the user is asked about deleting, if
    /// one is.
    pub deleting: Option<DeleteFromBrowserPrompt<'a>>,
    /// On the web, whether a file is dragged over the page: the drop zone
    /// lights up.
    pub dragging: bool,
    /// The damaged file just opened, asked about before its design shows,
    /// if one is.
    pub damaged: Option<DamagedPrompt<'a>>,
    /// The panic a session recorded, if one did.
    pub panic: Option<PanicNote<'a>>,
    pub mode: theme::Mode,
    /// The theme chosen, which the theme button shows.
    pub theme: ThemeChoice,
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
    /// The design's thumbnail, saved with it, if it has one: rendered at
    /// [`THUMBNAIL_SCALE`](crate::THUMBNAIL_SCALE), fitting
    /// [`THUMBNAIL_ROOM`](crate::THUMBNAIL_ROOM) at it.
    pub thumbnail: Option<image::Handle>,
}

/// What's said of browser storage on the web's welcome screen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageNote {
    /// Whether the browser may clear the designs kept in it, as it hasn't
    /// made the site's storage persistent: a note says so, and to
    /// download what matters.
    pub may_clear: bool,
    /// How much of it is used, like "1.2 MB of 2 GB used", if the browser
    /// says.
    pub used: Option<String>,
}

/// Asks before deleting a design in browser storage whose latest isn't
/// downloaded: it may be the only copy there is.
pub struct DeleteFromBrowserPrompt<'a> {
    /// Its file name there.
    pub name: &'a str,
    /// Where it stands against its downloads.
    pub downloads: Downloads,
}

/// Which design a card of the welcome screen's designs is, for opening or
/// deleting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardKey<'a> {
    /// A new design left behind, by its entry in the store of new designs.
    Recovered(&'a Path),
    /// A design in browser storage, by its file name there.
    Browser(&'a str),
}

/// A card in the welcome screen's designs: new designs left behind, and
/// on the web those in browser storage.
pub struct DesignCard<'a> {
    pub key: CardKey<'a>,
    /// The design's name: `Untitled`, or that of the file it was opened
    /// from or is saved as.
    pub name: String,
    /// Whether it's known by a file's name, shown with the extension
    /// after it, rather than as Untitled.
    pub file: bool,
    /// When it was last written to the store, like "2 h ago", if that's
    /// known.
    pub written: Option<String>,
    /// On the web, where it stands against its downloads; `None`
    /// natively, where nothing is downloaded.
    pub downloads: Option<Downloads>,
    /// Whether its entry is damaged: marked so.
    pub damaged: bool,
    /// Whether it opens: not if it's damaged so that nothing of it can be
    /// read, which can only be discarded.
    pub opens: bool,
    /// The thumbnail its last save wrote, if it has one.
    pub thumbnail: Option<image::Handle>,
    /// What's said of it in place of when it was written, if anything:
    /// that it holds changes not saved, or is open in another tab.
    pub note: Option<&'static str>,
    /// Whether it may be deleted: not while another tab has it open.
    pub deletable: bool,
}

/// Where a design in browser storage stands against its downloads, each
/// with when it was downloaded, like "2 h ago", if that's known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Downloads {
    /// It never was: what's in the browser is all there is.
    Never,
    /// It was, and has changed since.
    Changed(Option<String>),
    /// What's in the browser is what was last downloaded.
    Latest(Option<String>),
}

/// What a card, or the file menu, says of where a design stands against
/// its `downloads`, and the colour of the dot before that.
pub(crate) fn downloads_said(downloads: &Downloads) -> (String, fn(&Palette) -> iced::Color) {
    let at = |what: &str, when: &Option<String>| match when {
        Some(when) => format!("{what} {when}"),
        None => what.to_owned(),
    };
    match downloads {
        Downloads::Never => ("Never downloaded".to_owned(), |_| theme::NEVER_DOWNLOADED),
        Downloads::Changed(when) => (at("Changed since downloaded", when), |p| p.accent),
        Downloads::Latest(when) => (at("Latest downloaded", when), |p| p.ok),
    }
}

/// The screen shown when no document is open: buttons to start a design
/// and the designs to open, natively in a column beside the recent files,
/// on the web as a page.
pub fn welcome<'a>(mut state: WelcomeState<'a>) -> Element<'a, Message> {
    let report = (state.panic.as_mut()).and_then(|panic| panic.report.take());
    let damaged = state.damaged.take();
    let deleting = state.deleting.take();
    let screen = match state.recent.take() {
        Some(recent) => desktop(state, recent),
        None => web(state),
    };
    match (damaged, deleting, report) {
        (Some(prompt), _, _) => stack![screen, damaged_prompt(prompt)].into(),
        (None, Some(prompt), _) => stack![screen, delete_from_browser_prompt(prompt)].into(),
        (None, None, Some(report)) => stack![screen, panic_report(report)].into(),
        (None, None, None) => screen,
    }
}

/// The width of the native screen's column.
const COLUMN_WIDTH: f32 = 320.0;

/// Natively: the column, with the logo, the Start buttons and the help at
/// its foot, beside the panic recorded, the recovered designs and the
/// `recent` files, each under a band of its own.
fn desktop<'a>(state: WelcomeState<'a>, recent: Vec<RecentCard<'a>>) -> Element<'a, Message> {
    let [new, open] = welcome_bindings();
    let actions = column![
        start_action(Icon::Plus, "New design", new, Emphasis::Primary),
        start_action(Icon::Folder, "Open…", open, Emphasis::Secondary),
    ]
    .spacing(2);
    let error = state.error.map(|error| {
        container(text(error).style(theme::danger_text)).padding(Padding::from([6, 8]))
    });
    let foot = column![shortcuts_button(), theme_button(state.theme),].spacing(2);
    let logo = container(lockup(false))
        .center_x(Length::Fill)
        .padding(Padding::from([22, 16]).bottom(18))
        .style(theme::toolbar);
    let side = column![
        logo,
        chrome::hrule(),
        column![
            container(sub_label("Start")).padding(Padding::from([0, 8]).bottom(6)),
            actions,
            error,
            space::vertical(),
            foot,
        ]
        .padding(Padding::from([18, 16]).bottom(14))
        .height(Length::Fill),
    ];
    let side = container(side)
        .width(COLUMN_WIDTH)
        .height(Length::Fill)
        .style(theme::tab_strip);

    let mut main = column![];
    if let Some(note) = state.panic {
        main = main
            .push(band("Internal error", None))
            .push(padded(panic_card(note)));
    }
    if !state.stored.is_empty() {
        let count = state.stored.len();
        let said = text(format!(
            "Changes never saved, left when {APP_NAME} last closed unexpectedly"
        ))
        .style(theme::muted_text);
        let cards = cards(state.stored.into_iter().map(design_card));
        main = (main.push(band("Recovered", Some(count))))
            .push(padded(column![said, cards].spacing(12)));
    }
    let count = recent.len();
    main = (main.push(band("Recent", Some(count)))).push(padded(recent_grid(recent)));
    let main = container(chrome::scrolled(main, 2.0))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::welcome);
    row![side, chrome::vrule(), main].into()
}

/// The widest the web page's content gets, its header's too: four cards
/// across, about as wide as on the desktop's screen at its default size.
const PAGE_WIDTH: f32 = 920.0;

/// On the web: the header bar with the logo and the Start buttons over
/// what's kept in browser storage, the drop zone the first of its cards,
/// and a foot with the theme and how much is used.
fn web<'a>(state: WelcomeState<'a>) -> Element<'a, Message> {
    let [new, open] = welcome_bindings();
    let buttons = row![
        page_button(Icon::Plus, "New design", new, Emphasis::Primary),
        page_button(Icon::Folder, "Open…", open, Emphasis::Secondary),
    ]
    .spacing(10);
    let header = container(middle(
        row![lockup(true), space::horizontal(), buttons]
            .height(Length::Fill)
            .align_y(Alignment::Center),
    ))
    .height(64)
    .style(theme::toolbar);

    let mut content = column![].spacing(12);
    if let Some(error) = state.error {
        content = content.push(text(error).style(theme::danger_text));
    }
    if let Some(note) = state.panic {
        content = content
            .push(heading("Internal error", None))
            .push(panic_card(note))
            .push(Space::new().height(12));
    }
    let count = state.stored.len();
    let storage = state.storage.unwrap_or_default();
    if storage.may_clear && count > 0 {
        content = content.push(clearing_note());
    }
    let cards = cards(
        std::iter::once(drop_zone(state.dragging)).chain(state.stored.into_iter().map(design_card)),
    );
    content = content
        .push(heading("In browser storage", Some(count)))
        .push(cards);
    let content = middle(container(content).padding(Padding::from([32, 0]).bottom(28)));

    let used = storage
        .used
        .map(|used| container(text(used).size(11).style(theme::faint_text)).padding([0, 8]));
    let foot = container(
        row![theme_button(state.theme), used]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .center_x(Length::Fill)
    .padding(Padding::from([14, 16]).bottom(18));

    container(column![
        header,
        chrome::hrule(),
        container(chrome::scrolled(content, 2.0)).height(Length::Fill),
        chrome::hrule(),
        foot,
    ])
    .width(Length::Fill)
    .height(Length::Fill)
    .style(theme::welcome)
    .into()
}

/// `content` as wide as the page's, at most [`PAGE_WIDTH`] with 24 px
/// either side to spare, in the middle.
fn middle<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(container(content).width(Length::Fill).max_width(PAGE_WIDTH))
        .center_x(Length::Fill)
        .padding([0, 24])
        .into()
}

/// The logo: the mark over the app's name, or beside it in a `row`.
fn lockup<'a>(in_row: bool) -> Element<'a, Message> {
    static LOGO: LazyLock<svg::Handle> =
        LazyLock::new(|| svg::Handle::from_memory(LOGO_SVG.as_bytes()));
    let (side, size) = if in_row { (30, 16) } else { (38, 15) };
    let mark = svg(LOGO.clone()).width(side).height(side);
    let name = text(APP_NAME)
        .size(size)
        .font(theme::BOLD)
        .style(theme::muted_text)
        .wrapping(text::Wrapping::None);
    if in_row {
        row![mark, name]
            .spacing(8)
            .align_y(Alignment::Center)
            .into()
    } else {
        column![mark, name]
            .spacing(4)
            .align_x(Alignment::Center)
            .into()
    }
}

/// A small heading in the column, like Start.
fn sub_label<'a>(label: &'a str) -> Element<'a, Message> {
    text(label)
        .size(11)
        .font(theme::BOLD)
        .style(theme::faint_text)
        .into()
}

/// A heading of the native screen's main side, in a band across it in the
/// tab strip's grey, with the `count` of what's under it, if that's
/// counted.
fn band<'a>(label: &'a str, count: Option<usize>) -> Element<'a, Message> {
    column![
        container(heading(label, count))
            .width(Length::Fill)
            .height(29)
            .align_y(Alignment::Center)
            .padding([0, 24])
            .style(theme::tab_strip),
        chrome::hrule(),
    ]
    .into()
}

/// What's under a [`band`], padded.
fn padded<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .padding(Padding::from([18, 24]).bottom(26))
        .into()
}

/// A heading over designs: `label` and their `count`, if they're counted.
fn heading<'a>(label: &'a str, count: Option<usize>) -> Element<'a, Message> {
    let count = count.map(|count| {
        container(text(count.to_string()).size(10.5).font(theme::SEMIBOLD))
            .padding([0, 6])
            .style(theme::count_chip)
    });
    row![
        text(label).font(theme::SEMIBOLD).style(theme::muted_text),
        count
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

/// A Start button in the column: an icon, a label and, at the right, the
/// key that presses it, sending what `binding` does. The primary one is
/// filled with the accent, the others flat.
fn start_action<'a>(
    icon: Icon,
    label: &'a str,
    binding: Binding,
    emphasis: Emphasis,
) -> Element<'a, Message> {
    let content = row![
        action_icon(icon, 18.0, emphasis),
        text(label).font(theme::SEMIBOLD),
        space::horizontal(),
        chrome::key_chip(binding.shortcut, ChipSize::Normal).style(emphasis.key_chip()),
    ]
    .spacing(10)
    .height(Length::Fill)
    .align_y(Alignment::Center);
    let primary = emphasis == Emphasis::Primary;
    button(content)
        .width(Length::Fill)
        .height(38)
        .padding([0, 10])
        .style(move |theme, status| {
            if primary {
                theme::primary_button(theme, status)
            } else {
                theme::flat_button(false, Tone::Text)(theme, status)
            }
        })
        .on_press_maybe(binding.sends())
        .into()
}

/// A Start button in the web page's header: an icon, a label and the key
/// that presses it, sending what `binding` does.
fn page_button<'a>(
    icon: Icon,
    label: &'a str,
    binding: Binding,
    emphasis: Emphasis,
) -> Element<'a, Message> {
    let content = row![
        action_icon(icon, icons::INLINE, emphasis),
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

/// A Start button's icon, `size` square: on the primary colour in its
/// content's, else in its own.
fn action_icon<'a>(icon: Icon, size: f32, emphasis: Emphasis) -> Element<'a, Message> {
    if emphasis == Emphasis::Secondary {
        icons::icon(icon, size)
    } else {
        icons::tinted(icon, size, move |p| emphasis.content(p)).into()
    }
}

/// A quiet button at the foot: an icon and a label, sending `message`, or
/// faint without one. Both turn to the text colour on hover.
fn foot_button<'a>(icon: Icon, label: &'a str, message: Option<Message>) -> Element<'a, Message> {
    let enabled = message.is_some();
    let content = |hovered: bool| {
        row![
            icons::tinted(icon, icons::INLINE, move |p| {
                theme::flat_content(p, Tone::Muted, enabled, hovered)
            }),
            text(label),
        ]
        .spacing(8)
        .height(Length::Fill)
        .padding([0, 8])
        .align_y(Alignment::Center)
    };
    // `hover` swaps in the icon's hover colour anywhere over the button,
    // as the button's own does its text's: its padding is inside.
    button(hover(content(false), content(enabled)))
        .height(28)
        .padding(0)
        .style(theme::flat_button(false, Tone::Muted))
        .on_press_maybe(message)
        .into()
}

/// The shortcut sheet's button, inert till there is one, as the
/// toolbar's help button is.
fn shortcuts_button<'a>() -> Element<'a, Message> {
    // TODO: open the shortcut sheet.
    foot_button(Icon::Help, "Shortcuts", None)
}

/// The theme's button, cycling through the themes, saying which is
/// chosen.
fn theme_button<'a>(theme: ThemeChoice) -> Element<'a, Message> {
    let (icon, label) = chrome::theme_said(theme);
    foot_button(icon, label, Some(Message::CycleTheme))
}

/// The panic a session recorded, as a card: its message's first line and
/// when, beside buttons to show the whole of it and to discard it.
fn panic_card<'a>(note: PanicNote<'a>) -> Element<'a, Message> {
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
fn panic_button<'a>(label: &'a str, message: Welcome) -> Element<'a, Message> {
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
fn panic_report<'a>(report: String) -> Element<'a, Message> {
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

/// The widest a design's card gets.
const CARD_WIDTH: f32 = 220.0;

/// `cards` in a grid as many to a row as fit.
fn cards<'a>(cards: impl IntoIterator<Item = Element<'a, Message>>) -> Element<'a, Message> {
    grid(cards)
        .fluid(CARD_WIDTH)
        .spacing(14)
        .height(Length::Shrink)
        .into()
}

/// The recently opened files' cards, or a note on where they will show.
fn recent_grid(recent: Vec<RecentCard<'_>>) -> Element<'_, Message> {
    if recent.is_empty() {
        text("Files you open show up here")
            .style(theme::muted_text)
            .into()
    } else {
        cards(recent.into_iter().map(recent_card))
    }
}

/// A row of a card's text, clipped to its width rather than wrapped.
fn clipped<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content).width(Length::Fill).clip(true).into()
}

/// A design's name, and in faint the extension after it, like
/// "bracket.vrdp".
fn file_name<'a>(name: String, extension: String) -> Element<'a, Message> {
    clipped(row![
        text(name)
            .font(theme::SEMIBOLD)
            .wrapping(text::Wrapping::None),
        text(extension)
            .style(theme::faint_text)
            .wrapping(text::Wrapping::None),
    ])
}

/// A recently opened file: a thumbnail over its name, directory and when it
/// was opened. Clicking it opens the file.
fn recent_card<'a>(file: RecentCard<'a>) -> Element<'a, Message> {
    let meta = || {
        column![
            file_name(file.name.clone(), file.extension.clone()),
            row![
                clipped(
                    text(file.dir.clone())
                        .size(11.5)
                        .wrapping(text::Wrapping::None)
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
        file.thumbnail.clone(),
        meta,
        None,
        Some(Message::Welcome(Welcome::OpenPath(file.path.to_owned()))),
    )
}

/// How tall a card's thumbnail is, with its padding.
const THUMBNAIL_HEIGHT: f32 = 150.0;

/// The room around a thumbnail in its card: what's left of the card's
/// width, at its widest, and of [`THUMBNAIL_HEIGHT`] past
/// [`THUMBNAIL_ROOM`](crate::THUMBNAIL_ROOM), split either side.
const THUMBNAIL_PADDING: f32 = 12.0;

/// How tall the text under a stored design's thumbnail is, with its
/// padding: three rows, the last with the delete button, which a damaged
/// design's note doesn't add to, taking the place of the second. The drop
/// zone beside them is as tall as the whole card, see
/// [`STORED_CARD_HEIGHT`].
const STORED_META_HEIGHT: f32 = 84.0;

/// How tall a stored design's card is: the thumbnail, the rule under it,
/// the text, and the card's border.
const STORED_CARD_HEIGHT: f32 = THUMBNAIL_HEIGHT + 1.0 + STORED_META_HEIGHT + 2.0;

/// The note over what's in browser storage when the browser may clear it,
/// as it hasn't made the site's storage persistent.
fn clearing_note<'a>() -> Element<'a, Message> {
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
fn delete_from_browser_prompt<'a>(prompt: DeleteFromBrowserPrompt<'a>) -> Element<'a, Message> {
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

/// A design kept in the store: a card like a recent file's, which opens
/// it, saying when it was last written and, on the web, where it stands
/// against its downloads, with a button at its foot deleting it. A
/// damaged one says so instead of where it stands, and one holding
/// changes not saved, or open in another tab, says so instead of when.
fn design_card<'a>(design: DesignCard<'a>) -> Element<'a, Message> {
    let damage = damage_note(&design);
    let extension = if design.file {
        format!(".{EXTENSION}")
    } else {
        String::new()
    };
    let small = |line: String| text(line).size(11.5).wrapping(text::Wrapping::None);
    let key = design.key;
    let meta = move || {
        let status: Element<'a, Message> = match (damage, &design.downloads) {
            (Some(note), _) => small(note.to_owned()).style(theme::warning_text).into(),
            (None, Some(downloads)) => {
                let (said, color) = downloads_said(downloads);
                row![
                    container(Space::new().width(7).height(7)).style(theme::dot(color)),
                    small(said),
                ]
                .spacing(6)
                .align_y(Alignment::Center)
                .into()
            }
            (None, None) => small("Auto-saved".to_owned()).into(),
        };
        let written: Element<'a, Message> = match design.note {
            Some(note) => small(note.to_owned()).style(theme::warning_text).into(),
            None => small(design.written.clone().unwrap_or_default()).into(),
        };
        column![
            file_name(design.name.clone(), extension.clone()),
            clipped(status),
            space::vertical(),
            row![
                clipped(written),
                download_button(key),
                delete_button(key, design.deletable)
            ]
            .spacing(2)
            .align_y(Alignment::Center),
        ]
        .spacing(2)
        .height(Length::Fill)
        .into()
    };
    let open = match key {
        CardKey::Recovered(path) => Welcome::OpenStored(path.to_owned()),
        CardKey::Browser(name) => Welcome::OpenFromBrowser(name.to_owned()),
    };
    card(
        design.thumbnail.clone(),
        meta,
        Some(STORED_META_HEIGHT),
        design.opens.then_some(Message::Welcome(open)),
    )
}

/// What a stored design's card says of its entry's damage, if it's
/// damaged.
fn damage_note(design: &DesignCard<'_>) -> Option<&'static str> {
    match (design.damaged, design.opens) {
        (false, _) => None,
        (true, true) => Some("Damaged"),
        (true, false) => Some("Damaged, can't be read"),
    }
}

/// How big the delete button at the foot of a stored design's card is.
const DELETE_SIZE: Size = Size::new(26.0, 24.0);

/// The button at the foot of a card of a design in browser storage
/// downloading it, as it's saved, beside the delete button; none for a
/// new design left behind.
fn download_button<'a>(key: CardKey<'_>) -> Option<Element<'a, Message>> {
    let CardKey::Browser(name) = key else {
        return None;
    };
    let arrow = |hovered| {
        container(icons::tinted(Icon::Download, 15.0, move |p| {
            theme::flat_content(p, Tone::Muted, true, hovered)
        }))
        .center(Length::Fill)
    };
    let download = button(hover(arrow(false), arrow(true)))
        .width(DELETE_SIZE.width)
        .height(DELETE_SIZE.height)
        .padding(0)
        .style(theme::flat_button(false, Tone::Muted))
        .on_press(Message::Welcome(Welcome::DownloadFromBrowser(
            name.to_owned(),
        )));
    Some(chrome::tip(download, text("Download")))
}

/// The button at the foot of a stored design's card deleting it: a trash
/// can with a hover background of its own over the card's, as it doesn't
/// open the card, and a tip saying what it does. Unless `enabled`, it's
/// faint and does nothing, its tip saying why: the design is open in
/// another tab.
fn delete_button<'a>(key: CardKey<'_>, enabled: bool) -> Element<'a, Message> {
    // `hover` swaps in the icon's hover colour anywhere over the button,
    // as the button's own does its wash: its padding is inside.
    let trash = |hovered| {
        container(icons::tinted(Icon::Trash, 15.0, move |p| {
            if enabled {
                theme::delete_icon(p, hovered)
            } else {
                p.faint
            }
        }))
        .center(Length::Fill)
    };
    let delete = button(hover(trash(false), trash(true)))
        .width(DELETE_SIZE.width)
        .height(DELETE_SIZE.height)
        .padding(0)
        .style(theme::delete_button)
        .on_press_maybe(enabled.then(|| {
            Message::Welcome(match key {
                CardKey::Recovered(path) => Welcome::DiscardStored(path.to_owned()),
                CardKey::Browser(name) => Welcome::DeleteFromBrowser(name.to_owned()),
            })
        }));
    if enabled {
        chrome::tip(delete, text("Delete"))
    } else {
        // Its clicks don't reach the card under it, which would open.
        opaque(chrome::tip(
            delete,
            text("Open in another tab, so it can't be deleted"),
        ))
    }
}

/// A card on the welcome screen: `thumbnail` over the rows `meta` builds,
/// `meta_height` tall if that's given, else as tall as they are. Without
/// a thumbnail, a large body icon stands in for it. Clicking it sends
/// `on_press`, if there's that.
fn card<'a>(
    thumbnail: Option<image::Handle>,
    meta: impl Fn() -> Element<'a, Message>,
    meta_height: Option<f32>,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    let content = |hovered: bool| {
        let picture: Element<'a, Message> = match thumbnail.clone() {
            Some(handle) => container(
                image(handle)
                    .content_fit(ContentFit::Contain)
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .padding(THUMBNAIL_PADDING)
            .into(),
            None => icons::tinted(Icon::Body, 56.0, |p| p.muted).into(),
        };
        let thumbnail = container(picture)
            .center_x(Length::Fill)
            .center_y(THUMBNAIL_HEIGHT)
            .style(theme::card_thumbnail);
        column![
            thumbnail,
            chrome::hrule(),
            container(meta())
                .width(Length::Fill)
                .height(meta_height.map_or(Length::Shrink, Length::Fixed))
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

/// The web page's first card: where a `.vrdp` file can be dropped to open
/// it, which clicking opens the Open picker for instead. Lit while a file
/// is `dragged` over the page: washed in the accent, saying to drop it.
fn drop_zone<'a>(dragged: bool) -> Element<'a, Message> {
    let [_, open] = welcome_bindings();
    let said = if dragged {
        "Drop to open it".to_owned()
    } else {
        format!("Drop a .{EXTENSION} file")
    };
    let content = column![
        icons::tinted(Icon::Folder, 28.0, |p| p.muted),
        text(said).font(theme::SEMIBOLD),
        row![
            text("or ").style(theme::muted_text),
            text("choose one")
                .font(theme::SEMIBOLD)
                .style(theme::accent_text),
        ],
    ]
    .spacing(6)
    .align_x(Alignment::Center);
    let zone = button(container(content).center(Length::Fill))
        .width(Length::Fill)
        .height(STORED_CARD_HEIGHT)
        .padding(14)
        .style(theme::drop_zone(dragged))
        .on_press_maybe(open.sends());
    stack![zone, responsive(move |size| dashed_outline(size, dragged)),].into()
}

/// The drop zone's dashed outline, `size`, drawn over it: iced's borders
/// are only solid. An SVG made for the size, so its dashes aren't scaled,
/// drawn in one colour (see [`theme::drop_zone_line`]); a canvas's
/// geometry inside the page's scrollable was left behind where it was
/// first drawn on the web as the layout moved.
fn dashed_outline<'a>(size: Size, lit: bool) -> Element<'a, Message> {
    const WIDTH: f32 = 2.0;
    let inset = WIDTH / 2.0;
    let outline = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}"><rect x="{inset}" y="{inset}" width="{rw}" height="{rh}" rx="{r}" fill="none" stroke="#000" stroke-width="{WIDTH}" stroke-dasharray="6 4"/></svg>"##,
        w = size.width,
        h = size.height,
        rw = (size.width - WIDTH).max(0.0),
        rh = (size.height - WIDTH).max(0.0),
        r = theme::CARD_RADIUS - inset,
    );
    svg(svg::Handle::from_memory(outline.into_bytes()))
        .width(size.width)
        .height(size.height)
        .content_fit(ContentFit::Fill)
        .style(move |theme: &Theme, status| svg::Style {
            color: Some(theme::drop_zone_line(
                theme::palette(theme),
                lit,
                status == svg::Status::Hovered,
            )),
        })
        .into()
}

#[cfg(test)]
mod tests;
