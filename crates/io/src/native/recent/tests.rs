use super::*;
use crate::UnixSeconds;
use crate::tests::TempDir;

fn entry(path: &str, opened: i64) -> RecentFile {
    RecentFile {
        path: path.into(),
        opened: UnixSeconds(opened),
    }
}

#[test]
fn toml_round_trip() {
    let entries = vec![entry("/a/b.vrdp", 1_700_000_000), entry("/c.vrdp", -5)];
    let toml = serialize(&entries);
    assert!(toml.contains("[[file]]"), "{toml}");
    assert_eq!(parse(&toml).unwrap(), entries);
}

#[test]
fn parses_empty_and_rejects_garbage() {
    assert_eq!(parse("").unwrap(), []);
    assert!(parse("file = 3\n").is_err());
    assert!(parse("[[file]\n").is_err());
}

/// One entry gone bad, e.g. edited by hand, loses only itself.
#[test]
fn a_bad_entry_loses_only_itself() {
    let toml = "[[file]]\npath = \"/a.vrdp\"\nopened = 1\n\
                [[file]]\npath = 3\n\
                [[file]]\npath = \"/b.vrdp\"\nopened = \"soon\"\n\
                [[file]]\nopened = 4\n\
                [[file]]\npath = \"/c.vrdp\"\nopened = 2\n";
    assert_eq!(
        parse(toml).unwrap(),
        [entry("/a.vrdp", 1), entry("/c.vrdp", 2)]
    );
}

#[test]
fn parse_trims_to_max() {
    let toml: String = (0..MAX + 10)
        .map(|i| format!("[[file]]\npath = \"/{i}.vrdp\"\nopened = {i}\n"))
        .collect();
    let entries = parse(&toml).unwrap();
    assert_eq!(entries.len(), MAX);
    assert_eq!(entries[0], entry("/0.vrdp", 0));
}

#[test]
fn write_and_load_through_a_file() {
    let dir = TempDir::new("recent");
    let store = dir.0.join("sub").join("recent.toml");
    let kept = dir.design();
    let kept = kept.to_str().unwrap();
    write(&store, &[entry(kept, 42), entry("/gone/x.vrdp", 41)]).unwrap();
    assert_eq!(load(&store), [entry(kept, 42), entry("/gone/x.vrdp", 41)]);
    assert_eq!(load(&dir.0.join("missing.toml")), []);
}

/// A file that can't be found is kept: it may be on a drive that isn't
/// mounted just now, and would otherwise be gone for good with the next
/// write. It's only marked unavailable.
#[test]
fn missing_files_are_kept_and_marked_unavailable() {
    let dir = TempDir::new("recent-missing");
    let store = dir.0.join("recent.toml");
    let kept = dir.design();
    let entries = [
        entry(kept.to_str().unwrap(), 2),
        entry("/unmounted/x.vrdp", 1),
    ];
    write(&store, &entries).unwrap();
    let loaded = load(&store);
    assert_eq!(loaded, entries);
    let available: Vec<_> = listed(loaded)
        .into_iter()
        .map(|listed| listed.available)
        .collect();
    assert_eq!(available, [true, false]);
}

/// Another instance writing the list at the same time has its own
/// temporary file, which this one leaves alone.
#[test]
fn another_instances_temporary_file_is_left_alone() {
    let dir = TempDir::new("recent-temporary");
    let store = dir.0.join("recent.toml");
    let theirs = store.with_extension("toml.tmp");
    std::fs::write(&theirs, "theirs").unwrap();
    write(&store, &[entry("/a.vrdp", 1)]).unwrap();
    write(&store, &[entry("/b.vrdp", 2)]).unwrap();
    assert_eq!(std::fs::read_to_string(&theirs).unwrap(), "theirs");
    assert_eq!(load(&store), [entry("/b.vrdp", 2)]);
    // And leaves none of its own behind.
    let left: Vec<_> = std::fs::read_dir(&dir.0)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(left.len(), 2, "{left:?}");
}
