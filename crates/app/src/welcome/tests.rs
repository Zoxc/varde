use varde_io::storage::Space;
use varde_view::{CardKey, Downloads};

use super::*;

/// A new design left behind, last written at 1000 s, known by `name` if it
/// was opened from a file.
fn design(name: Option<&str>) -> Recovered {
    Recovered {
        path: PathBuf::from("designs/a.vrdp"),
        modified: Some(UnixSeconds(1000)),
        name: name.map(str::to_owned),
        damage: None,
    }
}

/// A design saved in browser storage at 1000 s, standing as `download`
/// against its downloads.
fn saved(name: &str, download: DownloadStatus) -> BrowserDesign {
    BrowserDesign {
        name: name.to_owned(),
        saved: Some(UnixSeconds(1000)),
        sum: None,
        thumbnail: None,
        download,
        unsaved: false,
        in_use: false,
        damage: None,
    }
}

/// Two hours after the designs were last written.
const NOW: UnixSeconds = UnixSeconds(1000 + 2 * 3600);

/// A new design left behind says it's auto-saved, and nothing of
/// downloads: it's never been saved.
#[test]
fn a_recovered_card_says_nothing_of_downloads() {
    let untitled = design(None);
    let card = recovered_card(&untitled, NOW);
    assert_eq!(card.key, CardKey::Recovered(Path::new("designs/a.vrdp")));
    assert_eq!((card.name.as_str(), card.file), ("Untitled", false));
    assert_eq!(card.downloads, None);
    assert_eq!(card.written.as_deref(), Some("2 h ago"));
    let lid = design(Some("lid.vrdp"));
    let card = recovered_card(&lid, NOW);
    assert_eq!((card.name.as_str(), card.file), ("lid", true));
    let unreadable = Recovered {
        damage: Some(ListedDamage::Unreadable),
        ..design(None)
    };
    let card = recovered_card(&unreadable, NOW);
    assert!(card.damaged && !card.opens);
}

/// A design in browser storage says where it stands against its
/// downloads, with when it was last downloaded, and what holds it.
#[test]
fn a_saved_card_says_where_its_design_stands() {
    let files = Files::new(None);
    let card = |design: &BrowserDesign| {
        let card = browser_card(&files, design, NOW);
        (card.name, card.downloads, card.note)
    };
    let never = saved("lid.vrdp", DownloadStatus::Never);
    assert_eq!(
        card(&never),
        ("lid".to_owned(), Some(Downloads::Never), None)
    );
    let latest = saved("lid.vrdp", DownloadStatus::Latest(UnixSeconds(1000)));
    assert_eq!(
        card(&latest).1,
        Some(Downloads::Latest(Some("2 h ago".to_owned())))
    );
    let changed = BrowserDesign {
        unsaved: true,
        ..saved(
            "lid.vrdp",
            DownloadStatus::Changed(UnixSeconds(1000 - 3600)),
        )
    };
    assert_eq!(
        card(&changed),
        (
            "lid".to_owned(),
            Some(Downloads::Changed(Some("3 h ago".to_owned()))),
            Some("Changes not saved")
        )
    );
    let open = BrowserDesign {
        in_use: true,
        unsaved: true,
        ..never.clone()
    };
    assert_eq!(card(&open).2, Some("Open in another tab"));
    let unreadable = BrowserDesign {
        damage: Some(ListedDamage::Unreadable),
        ..never
    };
    let shown = browser_card(&files, &unreadable, NOW);
    assert!(shown.damaged && !shown.opens && shown.downloads.is_none());
    assert_eq!(shown.key, CardKey::Browser("lid.vrdp"));
}

#[test]
fn space_used_reads_as_a_size() {
    let said = |used, quota| space_used(Space { used, quota });
    assert_eq!(said(0, 999), "0 bytes of 999 bytes used");
    assert_eq!(said(1_234_567, 2_000_000_000), "1.2 MB of 2.0 GB used");
    assert_eq!(said(45_600, 10_000), "46 KB of 10 KB used");
    assert_eq!(bytes(u64::MAX), "18446744 TB");
}
