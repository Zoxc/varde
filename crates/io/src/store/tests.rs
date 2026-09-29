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
        downloaded: false,
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
