use std::path::PathBuf;

use iced::advanced::{Layout, Shell, clipboard};
use iced::{Event, Point, Rectangle, mouse};

use super::*;
use crate::probe::Shown;
use crate::testing::Laid;

/// The window the tests lay out in.
const WINDOW: Size = Size::new(1280.0, 800.0);

/// The store entries the tests list.
fn paths() -> [PathBuf; 2] {
    ["designs/a.vrdp", "designs/b.vrdp"].map(PathBuf::from)
}

/// A design kept in the store at `path`, known by `name`, opened from a
/// file if `file`, standing as `downloads` against its downloads.
fn stored<'a>(
    path: &'a Path,
    name: &str,
    file: bool,
    downloads: Option<Downloads>,
) -> DesignCard<'a> {
    DesignCard {
        key: CardKey::Recovered(path),
        name: name.to_owned(),
        file,
        written: Some("2 h ago".to_owned()),
        downloads,
        damaged: false,
        opens: true,
        thumbnail: None,
        note: None,
        deletable: true,
    }
}

/// The welcome screen with `stored` designs and, natively, the recent
/// files `recent`.
fn state<'a>(recent: Option<Vec<RecentCard<'a>>>, stored: Vec<DesignCard<'a>>) -> WelcomeState<'a> {
    WelcomeState {
        error: None,
        recent,
        stored,
        storage: None,
        deleting: None,
        dragging: false,
        damaged: None,
        panic: None,
        mode: theme::Mode::Light,
        theme: ThemeChoice::Auto,
    }
}

/// A recent file at `path`.
fn recent(path: &Path) -> RecentCard<'_> {
    RecentCard {
        path,
        name: "bracket".to_owned(),
        extension: ".vrdp".to_owned(),
        dir: "~/parts".to_owned(),
        opened: "Yesterday".to_owned(),
        available: true,
        thumbnail: None,
    }
}

fn find<'a>(shown: &'a [Shown], text: &str) -> &'a Shown {
    shown
        .iter()
        .find(|shown| shown.text == text)
        .unwrap_or_else(|| panic!("no {text:?} in {shown:?}"))
}

fn shows(shown: &[Shown], text: &str) -> bool {
    shown.iter().any(|shown| shown.text == text)
}

/// What a left click at `at` on `laid` sends.
fn click(laid: &mut Laid<'_>, at: Point) -> Vec<Message> {
    let mut messages = Vec::new();
    for event in [
        mouse::Event::CursorMoved { position: at },
        mouse::Event::ButtonPressed(mouse::Button::Left),
        mouse::Event::ButtonReleased(mouse::Button::Left),
    ] {
        let mut shell = Shell::new(&mut messages);
        laid.element.as_widget_mut().update(
            &mut laid.tree,
            &Event::Mouse(event),
            Layout::new(&laid.node),
            mouse::Cursor::Available(at),
            &laid.renderer,
            &mut clipboard::Null,
            &mut shell,
            &Rectangle::with_size(WINDOW),
        );
    }
    messages
}

#[test]
fn natively_the_start_buttons_are_in_a_column_beside_the_designs() {
    let [a, b] = paths();
    let files = [PathBuf::from("/parts/bracket.vrdp")];
    let screen = welcome(state(
        Some(vec![recent(&files[0])]),
        vec![
            stored(&a, "Untitled", false, None),
            stored(&b, "lid", true, None),
        ],
    ));
    let shown = Laid::new(screen, WINDOW).texts();
    for text in [APP_NAME, "Start", "New design", "Open…", "Shortcuts"] {
        let shown = find(&shown, text);
        assert!(shown.bounds.x < COLUMN_WIDTH, "{shown:?}");
    }
    // Beside it, each under a band with its count.
    for text in ["Recovered", "Recent", "bracket", "Untitled", "lid"] {
        let shown = find(&shown, text);
        assert!(shown.bounds.x > COLUMN_WIDTH, "{shown:?}");
    }
    assert!(find(&shown, "Recovered").bounds.y < find(&shown, "Recent").bounds.y);
    // Each band's count beside its heading: two recovered, one recent.
    for (band, count) in [("Recovered", "2"), ("Recent", "1")] {
        let band = find(&shown, band).bounds;
        assert!(
            shown.iter().any(|shown| shown.text == count
                && shown.bounds.x > band.x + band.width
                && (shown.bounds.center_y() - band.center_y()).abs() < 2.0),
            "no {count} beside {band:?} in {shown:?}"
        );
    }
    assert!(shows(&shown, "Auto-saved"));
    // The web's page isn't.
    assert!(!shows(&shown, "In browser storage"));
}

#[test]
fn on_the_web_the_page_lists_what_is_in_browser_storage_after_the_drop_zone() {
    let [a, b] = paths();
    let screen = welcome(state(
        None,
        vec![
            stored(&a, "Untitled", false, Some(Downloads::Never)),
            stored(
                &b,
                "washer",
                true,
                Some(Downloads::Latest(Some("2 h ago".to_owned()))),
            ),
        ],
    ));
    let shown = Laid::new(screen, WINDOW).texts();
    let header = find(&shown, "New design");
    assert!(header.bounds.y < 64.0, "{header:?}");
    assert!(find(&shown, APP_NAME).bounds.x < header.bounds.x);
    let drop = find(&shown, "Drop a .vrdp file");
    let never = find(&shown, "Never downloaded");
    let latest = find(&shown, "Latest downloaded 2 h ago");
    assert!(drop.bounds.x < never.bounds.x && never.bounds.x < latest.bounds.x);
    assert!(shows(&shown, "In browser storage"));
    assert!(shows(&shown, "washer") && shows(&shown, ".vrdp"));
    assert!(!shows(&shown, "Start") && !shows(&shown, "Recent"));
}

#[test]
fn a_stored_card_opens_on_a_click_but_not_one_on_its_delete_button() {
    let [a, _] = paths();
    let mut laid = Laid::new(
        welcome(state(
            None,
            vec![stored(&a, "Untitled", false, Some(Downloads::Never))],
        )),
        WINDOW,
    );
    let shown = laid.texts();
    let name = find(&shown, "Untitled").bounds;
    let sent = click(&mut laid, name.center());
    assert!(
        matches!(&sent[..], [Message::Welcome(Welcome::OpenStored(path))] if path == &a),
        "{sent:?}"
    );
    // The delete button is at the right of the card's last row, beside
    // when it was written: a click on it deletes the design, and doesn't
    // open it too.
    let delete = delete_button_of(&laid);
    let written = find(&shown, "2 h ago").bounds;
    assert!(delete.x > written.x + written.width, "{delete:?}");
    assert!(
        (delete.center_y() - written.center_y()).abs() < 1.0,
        "{delete:?}"
    );
    let sent = click(&mut laid, delete.center());
    assert!(
        matches!(&sent[..], [Message::Welcome(Welcome::DiscardStored(path))] if path == &a),
        "{sent:?}"
    );
}

/// Where the only stored card's delete button is: the one place laid out
/// at its size, by the button, its tip and what's in it, in both the
/// card's looks.
fn delete_button_of(laid: &Laid<'_>) -> Rectangle {
    fn boxes(layout: Layout<'_>, found: &mut Vec<Rectangle>) {
        if layout.bounds().size() == DELETE_SIZE {
            found.push(layout.bounds());
        }
        for child in layout.children() {
            boxes(child, found);
        }
    }
    let mut found = Vec::new();
    boxes(Layout::new(&laid.node), &mut found);
    found.dedup();
    match found[..] {
        [delete] => delete,
        _ => panic!("not one delete button: {found:?}"),
    }
}

/// A damaged design's card says so in place of where it stands against
/// its downloads, so its last row, when it was written and the delete
/// button, stays inside the card.
#[test]
fn a_damaged_card_keeps_its_last_row_inside() {
    let [a, _] = paths();
    for opens in [true, false] {
        let design = DesignCard {
            damaged: true,
            opens,
            ..stored(&a, "Untitled", false, Some(Downloads::Never))
        };
        let mut laid = Laid::new(welcome(state(None, vec![design])), WINDOW);
        let shown = laid.texts();
        let note = if opens {
            "Damaged"
        } else {
            "Damaged, can't be read"
        };
        assert!(shows(&shown, note) && !shows(&shown, "Never downloaded"));
        // The text under the thumbnail starts with the name, 10 px into
        // it, and is `STORED_META_HEIGHT` tall with its padding.
        let name = find(&shown, "Untitled").bounds;
        let bottom = name.y - 10.0 + STORED_META_HEIGHT;
        let written = find(&shown, "2 h ago").bounds;
        let delete = delete_button_of(&laid);
        assert!(
            written.y + written.height <= bottom,
            "{written:?} past {bottom}"
        );
        assert!(
            delete.y + delete.height <= bottom,
            "{delete:?} past {bottom}"
        );
        assert!(written.y > find(&shown, note).bounds.y, "{shown:?}");
    }
}

#[test]
fn the_drop_zone_opens_the_picker_and_lights_while_a_file_is_dragged() {
    let mut laid = Laid::new(welcome(state(None, Vec::new())), WINDOW);
    let shown = laid.texts();
    let hint = find(&shown, "choose one").bounds;
    let sent = click(&mut laid, hint.center());
    assert!(
        matches!(&sent[..], [Message::Welcome(Welcome::Open)]),
        "{sent:?}"
    );
    let shown = Laid::new(
        welcome(WelcomeState {
            dragging: true,
            ..state(None, Vec::new())
        }),
        WINDOW,
    )
    .texts();
    // Saying to drop it, still offering to choose one.
    assert!(shows(&shown, "Drop to open it"));
    assert!(!shows(&shown, "Drop a .vrdp file"));
    assert!(shows(&shown, "choose one"));
}

#[test]
fn downloads_say_where_a_design_stands() {
    let said = |downloads: Downloads| downloads_said(&downloads).0;
    assert_eq!(said(Downloads::Never), "Never downloaded");
    assert_eq!(
        said(Downloads::Changed(Some("Sep 30".to_owned()))),
        "Changed since downloaded Sep 30"
    );
    assert_eq!(said(Downloads::Latest(None)), "Latest downloaded");
}

/// A design in browser storage, `name.vrdp` there, as the web lists it.
fn saved<'a>(name: &'a str, downloads: Downloads) -> DesignCard<'a> {
    DesignCard {
        key: CardKey::Browser(name),
        name: name.trim_end_matches(".vrdp").to_owned(),
        ..stored(Path::new(""), "", true, Some(downloads))
    }
}

/// A design in browser storage opens on a click, and its buttons download
/// it, as it's saved, and delete it, by its name there.
#[test]
fn a_saved_card_opens_downloads_and_deletes_by_name() {
    let mut laid = Laid::new(
        welcome(state(None, vec![saved("washer.vrdp", Downloads::Never)])),
        WINDOW,
    );
    let shown = laid.texts();
    let sent = click(&mut laid, find(&shown, "washer").bounds.center());
    assert!(
        matches!(&sent[..], [Message::Welcome(Welcome::OpenFromBrowser(name))] if name == "washer.vrdp"),
        "{sent:?}"
    );
    fn boxes(layout: Layout<'_>, found: &mut Vec<Rectangle>) {
        if layout.bounds().size() == DELETE_SIZE {
            found.push(layout.bounds());
        }
        for child in layout.children() {
            boxes(child, found);
        }
    }
    let mut buttons = Vec::new();
    boxes(Layout::new(&laid.node), &mut buttons);
    // In both the card's looks.
    buttons.sort_by(|a, b| a.x.total_cmp(&b.x));
    buttons.dedup();
    let [download, delete] = buttons[..] else {
        panic!("not two buttons: {buttons:?}");
    };
    assert!(download.x < delete.x);
    let sent = click(&mut laid, download.center());
    assert!(
        matches!(&sent[..], [Message::Welcome(Welcome::DownloadFromBrowser(name))] if name == "washer.vrdp"),
        "{sent:?}"
    );
    let sent = click(&mut laid, delete.center());
    assert!(
        matches!(&sent[..], [Message::Welcome(Welcome::DeleteFromBrowser(name))] if name == "washer.vrdp"),
        "{sent:?}"
    );

    // Open in another tab, it can't be deleted: the button does nothing,
    // nor opens the card under it.
    let design = DesignCard {
        deletable: false,
        ..saved("washer.vrdp", Downloads::Never)
    };
    let mut laid = Laid::new(welcome(state(None, vec![design])), WINDOW);
    assert!(click(&mut laid, delete.center()).is_empty());
    assert!(!click(&mut laid, download.center()).is_empty());
}

/// A card says, in place of when it was saved, that a closed tab left
/// changes in it, or another tab has it open.
#[test]
fn a_saved_card_says_what_holds_it() {
    let design = DesignCard {
        note: Some("Changes not saved"),
        ..saved("a.vrdp", Downloads::Changed(None))
    };
    let shown = Laid::new(welcome(state(None, vec![design])), WINDOW).texts();
    assert!(shows(&shown, "Changes not saved") && !shows(&shown, "2 h ago"));
    assert!(shows(&shown, "Changed since downloaded"));
}

/// Where the browser may clear what's kept in it, a note says so over
/// the designs, and to download them; the foot says how much is used,
/// beside the theme chosen, with no Shortcuts.
#[test]
fn the_page_says_the_browser_may_clear_its_storage() {
    let note = "This browser may clear what's kept in it";
    let page = |may_clear, stored: Vec<DesignCard<'static>>| {
        Laid::new(
            welcome(WelcomeState {
                storage: Some(StorageNote {
                    may_clear,
                    used: Some("1.2 MB of 2 GB used".to_owned()),
                }),
                ..state(None, stored)
            }),
            WINDOW,
        )
        .texts()
    };
    let shown = page(true, vec![saved("a.vrdp", Downloads::Never)]);
    assert!(shows(&shown, note));
    assert!(find(&shown, note).bounds.y < find(&shown, "In browser storage").bounds.y);
    assert!(shows(&shown, "1.2 MB of 2 GB used"));
    assert!(shows(&shown, "Theme: System"));
    assert!(!shows(&shown, "Shortcuts"));
    // Nothing to lose, or kept for good: nothing said.
    assert!(!shows(&page(true, Vec::new()), note));
    assert!(!shows(
        &page(false, vec![saved("a.vrdp", Downloads::Never)]),
        note
    ));
}

/// Deleting a design not downloaded as it is asks first, saying why.
#[test]
fn deleting_a_design_not_downloaded_asks_first() {
    for (downloads, why) in [
        (Downloads::Never, "It has never been downloaded"),
        (
            Downloads::Changed(None),
            "It has changed since it was last downloaded",
        ),
    ] {
        let mut laid = Laid::new(
            welcome(WelcomeState {
                deleting: Some(DeleteFromBrowserPrompt {
                    name: "washer.vrdp",
                    downloads,
                }),
                ..state(None, Vec::new())
            }),
            WINDOW,
        );
        let shown = laid.texts();
        assert!(shows(&shown, "Delete washer.vrdp?"));
        assert!(
            shown.iter().any(|shown| shown.text.starts_with(why)),
            "{shown:?}"
        );
        let sent = click(&mut laid, find(&shown, "Delete").bounds.center());
        assert!(
            matches!(&sent[..], [Message::Welcome(Welcome::ConfirmDelete)]),
            "{sent:?}"
        );
        let sent = click(&mut laid, find(&shown, "Cancel").bounds.center());
        assert!(
            matches!(&sent[..], [Message::Welcome(Welcome::CancelDelete)]),
            "{sent:?}"
        );
    }
}
