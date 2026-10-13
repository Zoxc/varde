//! The welcome screen, shown when no document is open: buttons to start a
//! design, and the designs to open, laid out as `notes/ui-mock/welcome.html`
//! has it. Natively a grey column with the logo, the Start buttons and a
//! foot of help, beside the recovered and recent designs; on the web, where
//! there are no recent files, a header bar with the logo and the buttons
//! over what's kept in browser storage, kept to the middle of the page.

use std::sync::LazyLock;

use iced::widget::{Space, button, column, container, row, space, stack, svg, text};
use iced::{Alignment, Element, Length, Padding};
use varde_document::APP_NAME;

use crate::chrome::{self, ChipSize};
use crate::icons::{self, Icon, LOGO_SVG};
use crate::shortcut::{Binding, Shortcut, welcome_bindings};
use crate::theme::{self, Emphasis, Tone};
use crate::{Message, SettingsState};
mod cards;
mod prompts;

pub use cards::{CardKey, DesignCard, RecentCard, SampleCard};
use cards::{cards, design_card, drop_zone, hover_button, recent_grid, sample_card};
pub use prompts::{DamagedPrompt, DeleteFromBrowserPrompt, PanicNote};
use prompts::{
    clearing_note, damaged_prompt, delete_from_browser_prompt, panic_card, panic_report,
};

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
    /// On the web, the sample designs built into the app, listed below
    /// browser storage; none where the build has none.
    pub samples: Vec<SampleCard<'a>>,
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
    /// The settings page, while it shows: natively in the recent designs'
    /// place, on the web under the header.
    pub settings: Option<SettingsState<'a>>,
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

/// Natively: the column, with the logo, the Start buttons and the
/// settings' button at its foot, beside the panic recorded, the recovered
/// designs and the `recent` files, each under a band of its own, or the
/// settings page in their place while it's open.
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
    let foot = settings_button(state.settings.is_some(), true);
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

    if let Some(settings) = state.settings {
        let page = crate::settings::page(settings);
        return row![side, chrome::vrule(), page].into();
    }
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
/// and a foot with the settings' button and how much is used. The
/// settings page, while it's open, shows under the header instead.
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
    let stored = cards(
        std::iter::once(drop_zone(state.dragging)).chain(state.stored.into_iter().map(design_card)),
    );
    content = content
        .push(heading("In browser storage", Some(count)))
        .push(stored);
    if !state.samples.is_empty() {
        content = content
            .push(Space::new().height(12))
            .push(heading("Samples", Some(state.samples.len())))
            .push(cards(state.samples.into_iter().map(sample_card)));
    }
    let content = middle(container(content).padding(Padding::from([32, 0]).bottom(28)));

    let used = storage
        .used
        .map(|used| container(text(used).size(11).style(theme::faint_text)).padding([0, 8]));
    let foot = container(
        row![settings_button(state.settings.is_some(), false), used]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .center_x(Length::Fill)
    .padding(Padding::from([14, 16]).bottom(18));

    let content: Element<'a, Message> = match state.settings {
        Some(settings) => crate::settings::page(settings),
        None => container(chrome::scrolled(content, 2.0))
            .height(Length::Fill)
            .into(),
    };
    container(column![
        header,
        chrome::hrule(),
        content,
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
    let chip = (binding.shortcut)
        .map(|shortcut| chrome::key_chip(shortcut, ChipSize::Normal).style(emphasis.key_chip()));
    let content = row![
        action_icon(icon, 18.0, emphasis),
        text(label).font(theme::SEMIBOLD),
        space::horizontal(),
        chip,
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
        (binding.shortcut).map(|shortcut| {
            container(chrome::key_chip(shortcut, ChipSize::Normal).style(emphasis.key_chip()))
                .padding(Padding::ZERO.left(4))
        }),
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

/// The settings' button at the foot, the gear and "Settings", and
/// natively its key: on while the page is `open`, closing it then. Its
/// icon turns to the text colour on hover, as its label does.
fn settings_button<'a>(open: bool, key: bool) -> Element<'a, Message> {
    let content = move |hovered: bool| {
        row![
            icons::tinted(Icon::Gear, icons::INLINE, move |p| {
                if open {
                    p.accent
                } else {
                    theme::flat_content(p, Tone::Muted, true, hovered)
                }
            }),
            text("Settings"),
            key.then(|| chrome::key_chip(Shortcut::SETTINGS, ChipSize::Small)),
        ]
        .spacing(8)
        .height(Length::Fill)
        .padding([0, 8])
        .align_y(Alignment::Center)
    };
    hover_button(content)
        .height(28)
        .style(theme::flat_button(open, Tone::Muted))
        .on_press(Message::ToggleSettings)
        .into()
}

#[cfg(test)]
mod tests;
