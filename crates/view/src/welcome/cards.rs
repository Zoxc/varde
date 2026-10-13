//! The welcome screen's cards: recent files, stored designs, samples and
//! the web's drop zone, and the grid they're laid out in.

use std::path::Path;

use iced::widget::{
    Button, button, column, container, grid, hover, image, opaque, responsive, row, space, stack,
    svg, text,
};
use iced::{Alignment, ContentFit, Element, Length, Padding, Size, Theme};
use varde_document::EXTENSION;

use crate::chrome::{self};
use crate::downloads::{Downloads, downloads_said};
use crate::icons::{self, Icon};
use crate::shortcut::welcome_bindings;
use crate::theme::{self, Palette, Tone};
use crate::{Message, Welcome};

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

/// A sample design built into the app, which opens as a new design.
#[derive(Debug, Clone)]
pub struct SampleCard<'a> {
    /// Which sample it is, as [`Welcome::OpenSample`] names it.
    pub index: usize,
    pub name: &'a str,
    /// A line on what it shows.
    pub about: &'a str,
    /// The thumbnail its file holds, if it has one.
    pub thumbnail: Option<image::Handle>,
}

/// The widest a design's card gets.
pub(super) const CARD_WIDTH: f32 = 220.0;

/// `cards` in a grid as many to a row as fit.
pub(super) fn cards<'a>(
    cards: impl IntoIterator<Item = Element<'a, Message>>,
) -> Element<'a, Message> {
    grid(cards)
        .fluid(CARD_WIDTH)
        .spacing(14)
        .height(Length::Shrink)
        .into()
}

/// The recently opened files' cards, or a note on where they will show.
pub(super) fn recent_grid(recent: Vec<RecentCard<'_>>) -> Element<'_, Message> {
    if recent.is_empty() {
        text("Files you open show up here")
            .style(theme::muted_text)
            .into()
    } else {
        cards(recent.into_iter().map(recent_card))
    }
}

/// A row of a card's text, clipped to its width rather than wrapped.
pub(super) fn clipped<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content).width(Length::Fill).clip(true).into()
}

/// A design's name, and in faint the extension after it, like
/// "bracket.vrdp".
pub(super) fn file_name<'a>(name: String, extension: String) -> Element<'a, Message> {
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
pub(super) fn recent_card<'a>(file: RecentCard<'a>) -> Element<'a, Message> {
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
pub(super) const THUMBNAIL_HEIGHT: f32 = 150.0;

/// The room around a thumbnail in its card: what's left of the card's
/// width, at its widest, and of [`THUMBNAIL_HEIGHT`] past
/// [`THUMBNAIL_ROOM`](crate::THUMBNAIL_ROOM), split either side.
pub(super) const THUMBNAIL_PADDING: f32 = 12.0;

/// How tall the text under a stored design's thumbnail is, with its
/// padding: three rows, the last with the delete button, which a damaged
/// design's note doesn't add to, taking the place of the second. The drop
/// zone beside them is as tall as the whole card, see
/// [`STORED_CARD_HEIGHT`].
pub(super) const STORED_META_HEIGHT: f32 = 84.0;

/// How tall a stored design's card is: the thumbnail, the rule under it,
/// the text, and the card's border.
pub(super) const STORED_CARD_HEIGHT: f32 = THUMBNAIL_HEIGHT + 1.0 + STORED_META_HEIGHT + 2.0;

/// A design kept in the store: a card like a recent file's, which opens
/// it, saying when it was last written and, on the web, where it stands
/// against its downloads, with a button at its foot deleting it. A
/// damaged one says so instead of where it stands, and one holding
/// changes not saved, or open in another tab, says so instead of when.
pub(super) fn design_card<'a>(design: DesignCard<'a>) -> Element<'a, Message> {
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
                row![crate::downloads::dot(color), small(said),]
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

/// A sample design's card: its thumbnail, if it has one, its name and
/// what it shows. Clicking it opens
/// it as a new design.
pub(super) fn sample_card<'a>(sample: SampleCard<'a>) -> Element<'a, Message> {
    let meta = move || {
        column![
            clipped(
                text(sample.name)
                    .font(theme::SEMIBOLD)
                    .wrapping(text::Wrapping::None)
            ),
            text(sample.about).size(11.5).style(theme::muted_text),
        ]
        .spacing(2)
        .into()
    };
    card(
        sample.thumbnail.clone(),
        meta,
        Some(STORED_META_HEIGHT),
        Some(Message::Welcome(Welcome::OpenSample(sample.index))),
    )
}

/// What a stored design's card says of its entry's damage, if it's
/// damaged.
pub(super) fn damage_note(design: &DesignCard<'_>) -> Option<&'static str> {
    match (design.damaged, design.opens) {
        (false, _) => None,
        (true, true) => Some("Damaged"),
        (true, false) => Some("Damaged, can't be read"),
    }
}

/// How big the delete button at the foot of a stored design's card is.
pub(super) const DELETE_SIZE: Size = Size::new(26.0, 24.0);

/// A button showing `content(hovered)`, hovered anywhere over the button:
/// `hover` swaps it in as the button's own status does its text and wash,
/// so its padding is inside `content`.
pub(super) fn hover_button<'a, E: Into<Element<'a, Message>>>(
    content: impl Fn(bool) -> E,
) -> Button<'a, Message> {
    button(hover(content(false), content(true))).padding(0)
}

/// A [`DELETE_SIZE`] [`hover_button`] at the foot of a card: `icon`
/// centred, coloured `color(palette, hovered)`.
pub(super) fn foot_button<'a>(
    icon: Icon,
    color: impl Fn(&Palette, bool) -> iced::Color + Copy + 'static,
) -> Button<'a, Message> {
    hover_button(move |hovered| -> Element<'a, Message> {
        container(icons::tinted(icon, 15.0, move |p| color(p, hovered)))
            .center(Length::Fill)
            .into()
    })
    .width(DELETE_SIZE.width)
    .height(DELETE_SIZE.height)
}

/// The button at the foot of a card of a design in browser storage
/// downloading it, as it's saved, beside the delete button; none for a
/// new design left behind.
pub(super) fn download_button<'a>(key: CardKey<'_>) -> Option<Element<'a, Message>> {
    let CardKey::Browser(name) = key else {
        return None;
    };
    let download = foot_button(Icon::Download, |p, hovered| {
        theme::flat_content(p, Tone::Muted, true, hovered)
    })
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
pub(super) fn delete_button<'a>(key: CardKey<'_>, enabled: bool) -> Element<'a, Message> {
    let delete = foot_button(Icon::Trash, move |p, hovered| {
        if enabled {
            theme::delete_icon(p, hovered)
        } else {
            p.faint
        }
    })
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
pub(super) fn card<'a>(
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

    hover_button(content)
        .padding(1)
        .width(Length::Fill)
        .style(theme::card)
        .on_press_maybe(on_press)
        .into()
}

/// The web page's first card: where a `.vrdp` file can be dropped to open
/// it, which clicking opens the Open picker for instead. Lit while a file
/// is `dragged` over the page: washed in the accent, saying to drop it.
pub(super) fn drop_zone<'a>(dragged: bool) -> Element<'a, Message> {
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
pub(super) fn dashed_outline<'a>(size: Size, lit: bool) -> Element<'a, Message> {
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
