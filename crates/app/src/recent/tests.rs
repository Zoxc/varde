use super::*;

fn entry(path: &str, opened: i64) -> RecentFile {
    RecentFile {
        path: path.into(),
        opened: UnixSeconds(opened),
    }
}

/// `entry`, there.
fn listed(path: &str, opened: i64) -> Listed {
    Listed {
        entry: entry(path, opened),
        available: true,
    }
}

/// The entries of `recent`, without whether they're available.
fn entries(recent: &Recent) -> Vec<RecentFile> {
    recent
        .entries()
        .iter()
        .map(|listed| listed.entry.clone())
        .collect()
}

#[test]
fn opened_moves_to_front_and_trims() {
    let mut recent = Recent::default();
    let last = MAX as i64 + 1;
    for i in 0..=last {
        recent.remember(format!("/{i}.vrdp").into(), UnixSeconds(i));
    }
    assert_eq!(recent.entries().len(), MAX);
    assert_eq!(entries(&recent)[0], entry(&format!("/{last}.vrdp"), last));
    assert_eq!(entries(&recent)[MAX - 1], entry("/2.vrdp", 2));

    recent.remember("/5.vrdp".into(), UnixSeconds(last + 100));
    assert_eq!(recent.entries().len(), MAX);
    assert_eq!(entries(&recent)[0], entry("/5.vrdp", last + 100));
    assert_eq!(
        entries(&recent)
            .iter()
            .filter(|e| e.path == Path::new("/5.vrdp"))
            .count(),
        1
    );
}

#[test]
fn files_opened_before_loading_stay_in_front() {
    let mut recent = Recent::default();
    // Not written before the stored list has arrived, which would drop it.
    assert!(recent.opened("/b.vrdp".into(), UnixSeconds(20)).is_none());
    assert!(recent.opened("/a.vrdp".into(), UnixSeconds(30)).is_none());
    let stored = vec![listed("/c.vrdp", 10), listed("/b.vrdp", 5)];
    let merged = [
        entry("/a.vrdp", 30),
        entry("/b.vrdp", 20),
        entry("/c.vrdp", 10),
    ];
    let write = recent.loaded(stored, Some("/home/me".into()));
    assert!(matches!(write, Some(IoRequest::WriteRecent { entries }) if entries == merged));
    assert_eq!(entries(&recent), merged);
    assert_eq!(recent.display_dir(Path::new("/home/me/a.vrdp")), "~");
    // Written as opened from now on.
    assert!(recent.opened("/c.vrdp".into(), UnixSeconds(40)).is_some());

    let mut recent = Recent::default();
    assert!(recent.loaded(vec![listed("/c.vrdp", 10)], None).is_none());
    assert_eq!(entries(&recent), [entry("/c.vrdp", 10)]);
}

#[test]
fn dir_abbreviates_home() {
    let home = Some(Path::new("/home/me"));
    assert_eq!(
        display_dir(Path::new("/home/me/parts/a.vrdp"), home),
        "~/parts"
    );
    assert_eq!(display_dir(Path::new("/home/me/a.vrdp"), home), "~");
    assert_eq!(
        display_dir(Path::new("/home/mex/a.vrdp"), home),
        "/home/mex"
    );
    assert_eq!(display_dir(Path::new("/srv/a.vrdp"), None), "/srv");
}
