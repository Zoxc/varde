use std::path::{Path, PathBuf};

use super::*;

#[test]
fn only_entries_in_the_store_are_taken() {
    let dir = Path::new("designs");
    assert_eq!(
        entry_in(dir, Path::new("designs/18c-5-0.vrdp")),
        Some("18c-5-0.vrdp")
    );
    for path in [
        "designs/.vrdp",
        "designs/a.txt",
        "designs/a.vrdp/b.vrdp",
        "other/a.vrdp",
        "a.vrdp",
        "designs/../a.vrdp",
        "designs",
        "",
    ] {
        assert_eq!(entry_in(dir, Path::new(path)), None, "{path}");
    }
    assert!(is_entry_name("a.vrdp"));
    assert!(!is_entry_name("a\\b.vrdp"));
    assert!(!is_entry_name("avrdp"));
}

/// Both lanes list recovered designs newest first, those of unknown times
/// last.
#[test]
fn recovered_designs_are_listed_newest_first() {
    let design = |modified: Option<i64>| Recovered {
        path: PathBuf::from("designs/a.vrdp"),
        modified: modified.map(UnixSeconds),
        name: None,
        damage: None,
    };
    let mut found = [
        design(None),
        design(Some(1)),
        design(Some(-5)),
        design(Some(7)),
    ];
    newest_first(&mut found);
    let times: Vec<_> = found
        .iter()
        .map(|design| design.modified.map(|seconds| seconds.0))
        .collect();
    assert_eq!(times, [Some(7), Some(1), Some(-5), None]);
}

#[test]
fn only_entries_are_listed_to_open() {
    let dir = Path::new("designs");
    assert_eq!(listed_entry(dir, Path::new("designs/a.vrdp")), Ok("a.vrdp"));
    assert_eq!(
        listed_entry(dir, Path::new("elsewhere/a.vrdp")),
        Err("elsewhere/a.vrdp isn't a recovered design".to_owned())
    );
}

/// Both lanes list an entry by what reading it found: intact, or damaged
/// with an intact auto-save that opens, or with none that can be read,
/// listed; empty, to delete; anything else skipped.
#[test]
fn entries_are_listed_by_what_reading_found() {
    use std::sync::Arc;

    use varde_document::Document;

    use crate::autosave::Held;
    use crate::tests::TempDir;

    let dir = TempDir::new("store-listing");
    let path = dir.0.join("a.vrdp");
    let file = std::fs::File::options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    let mut held = Held::new(file);
    let listed = |held: &mut Held<std::fs::File>| {
        listing(path.clone(), Some(UnixSeconds(3)), held.read_with_report())
    };
    assert!(matches!(listed(&mut held), Listing::Empty));
    let document = Arc::new(Document::example());
    held.append(None, Some("a.vrdp".to_owned()), &document)
        .unwrap();
    let second = std::fs::metadata(&path).unwrap().len();
    held.append(None, Some("b.vrdp".to_owned()), &document)
        .unwrap();
    let Listing::Listed(design) = listed(&mut held) else {
        panic!("not listed");
    };
    assert_eq!(
        (design.name.as_deref(), design.damage),
        (Some("b.vrdp"), None)
    );

    let flip = |at: u64| {
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[at as usize] ^= 0xff;
        std::fs::write(&path, bytes).unwrap();
    };
    // Into the newest auto-save's payload.
    flip(second + 60);
    let Listing::Listed(design) = listed(&mut held) else {
        panic!("not listed");
    };
    assert_eq!(
        (design.name.as_deref(), design.damage),
        (Some("a.vrdp"), Some(ListedDamage::Opens))
    );
    flip(crate::vrdp::FILE_HEADER_LEN as u64 + 60);
    let Listing::Listed(design) = listed(&mut held) else {
        panic!("not listed");
    };
    assert_eq!(
        design,
        Recovered {
            path: path.clone(),
            modified: Some(UnixSeconds(3)),
            name: None,
            damage: Some(ListedDamage::Unreadable),
        }
    );
    std::fs::write(&path, "not an entry").unwrap();
    assert!(matches!(listed(&mut held), Listing::Skipped));
}
