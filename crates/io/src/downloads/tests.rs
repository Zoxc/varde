use pollster::block_on;

use super::*;
use crate::dir::fs::FsDir;
use crate::tests::TempDir;

const SUM: u128 = 0x0123_4567_89ab_cdef_0011_2233_4455_6677;

/// A download recorded reads back as written, the newest per design.
#[test]
fn downloads_read_back_as_written() {
    let mut downloads = Downloads::default();
    downloads.record("a.vrdp", UnixSeconds(10), Some(SUM));
    downloads.record("b.vrdp", UnixSeconds(-3), None);
    downloads.record("a.vrdp", UnixSeconds(12), Some(SUM + 1));
    let toml = downloads.serialize();
    assert!(
        toml.contains("record = \"0123456789abcdef0011223344556678\""),
        "{toml}"
    );
    let read = Downloads::parse(&toml);
    assert_eq!(read, downloads);
    assert_eq!(read.entries.len(), 2);
    assert_eq!(
        read.last("a.vrdp", SUM + 1),
        Some(LastDownload {
            time: UnixSeconds(12),
            latest: true,
        })
    );
    assert_eq!(
        read.last("a.vrdp", SUM).map(|last| last.latest),
        Some(false)
    );
    assert_eq!(
        read.last("b.vrdp", SUM).map(|last| last.latest),
        Some(false)
    );
    assert_eq!(read.last("c.vrdp", SUM), None);
}

/// The three statuses: never downloaded, the latest as long as the save
/// downloaded is the newest with nothing changed since, and changed since
/// otherwise.
#[test]
fn a_design_stands_against_its_downloads() {
    let status =
        |downloads: &Downloads, newest, unsaved| downloads.status("a.vrdp", newest, unsaved);
    let mut downloads = Downloads::default();
    assert_eq!(status(&downloads, Some(SUM), false), DownloadStatus::Never);
    downloads.record("a.vrdp", UnixSeconds(10), Some(SUM));
    let at = UnixSeconds(10);
    assert_eq!(
        status(&downloads, Some(SUM), false),
        DownloadStatus::Latest(at)
    );
    // Changes not saved, in the sidecar.
    assert_eq!(
        status(&downloads, Some(SUM), true),
        DownloadStatus::Changed(at)
    );
    // Saved since.
    assert_eq!(
        status(&downloads, Some(SUM + 1), false),
        DownloadStatus::Changed(at)
    );
    // Can't be read.
    assert_eq!(status(&downloads, None, false), DownloadStatus::Changed(at));
    // Downloaded with changes not saved then.
    downloads.record("a.vrdp", UnixSeconds(11), None);
    assert_eq!(
        status(&downloads, Some(SUM), false),
        DownloadStatus::Changed(UnixSeconds(11))
    );
    assert_eq!(
        downloads.status("b.vrdp", Some(SUM), false),
        DownloadStatus::Never
    );
}

/// Renaming a design takes its downloads with it, in place of any of the
/// name it takes; deleting it takes them away.
#[test]
fn downloads_follow_their_design() {
    let mut downloads = Downloads::default();
    downloads.record("a.vrdp", UnixSeconds(10), Some(SUM));
    downloads.record("b.vrdp", UnixSeconds(11), None);
    downloads.renamed("a.vrdp", "b.vrdp");
    assert_eq!(
        downloads.status("a.vrdp", Some(SUM), false),
        DownloadStatus::Never
    );
    assert_eq!(
        downloads.status("b.vrdp", Some(SUM), false),
        DownloadStatus::Latest(UnixSeconds(10))
    );
    downloads.removed("b.vrdp");
    assert_eq!(downloads, Downloads::default());
}

/// Each entry is read on its own: one gone bad costs only itself.
#[test]
fn an_entry_gone_bad_costs_only_itself() {
    let toml = r#"
        [[download]]
        name = "a.vrdp"
        time = 1

        [[download]]
        name = "../escape.vrdp"
        time = 2

        [[download]]
        name = "b.vrdp"
        time = "yesterday"

        [[download]]
        name = "c.vrdp"
        time = 3
        record = "not hex"

        [[download]]
        name = "d.vrdp"
        time = 4
        record = "0123456789abcdef0011223344556677ff"

        [[download]]
        name = "e.vrdp"
        time = -9223372036854775808
        record = "0123456789abcdef0011223344556677"
        unknown = true
    "#;
    let downloads = Downloads::parse(toml);
    let names: Vec<_> = (downloads.entries.iter())
        .map(|download| download.name.as_str())
        .collect();
    assert_eq!(names, ["a.vrdp", "e.vrdp"]);
    assert_eq!(
        downloads.status("e.vrdp", Some(SUM), false),
        DownloadStatus::Latest(UnixSeconds(i64::MIN))
    );
    for bad in ["", "not toml [", "download = 3", "[[download]]\nname = 3"] {
        assert_eq!(Downloads::parse(bad), Downloads::default(), "{bad}");
    }
}

/// A tab closed as it rewrote the file leaves the new entries up to where
/// it stopped, then the old ones after: each whole entry is still read,
/// only the torn one lost, and of two of one design the newer counts.
#[test]
fn a_torn_rewrite_loses_at_most_the_entry_torn() {
    let mut old = Downloads::default();
    for (n, name) in ["a", "a long name", "c", "d"].into_iter().enumerate() {
        old.record(&format!("{name}.vrdp"), UnixSeconds(n as i64), None);
    }
    let mut new = old.clone();
    new.removed("a long name.vrdp");
    new.record("e.vrdp", UnixSeconds(101), Some(SUM));
    let (old, new) = (old.serialize(), new.serialize());
    // New: a, c, d, e. Torn in d, the third, the rest as it was.
    let cut = new.match_indices("[[download]]").nth(2).unwrap().0 + 20;
    let torn = format!("{}{}", &new[..cut], &old[cut..]);
    assert!(torn.parse::<toml::Table>().is_err(), "{torn}");
    let read = Downloads::parse(&torn);
    let status = |name| read.status(name, Some(SUM), false);
    assert_eq!(status("a.vrdp"), DownloadStatus::Changed(UnixSeconds(0)));
    assert_eq!(status("c.vrdp"), DownloadStatus::Changed(UnixSeconds(2)));
    // The old entry of d, whole after the torn one.
    assert_eq!(status("d.vrdp"), DownloadStatus::Changed(UnixSeconds(3)));
    assert_eq!(status("e.vrdp"), DownloadStatus::Never);

    // Of two of one design, the newer, wherever it is.
    let twice = r#"
        [[download]]
        name = "a.vrdp"
        time = 9
        record = "0123456789abcdef0011223344556677"

        [[download]]
        name = "a.vrdp"
        time = 2
    "#;
    assert_eq!(
        Downloads::parse(twice).status("a.vrdp", Some(SUM), false),
        DownloadStatus::Latest(UnixSeconds(9))
    );
}

/// Another tab recording a download holds the file a moment: one waits
/// for it, and records its own after.
#[test]
fn a_file_held_a_moment_is_waited_for() {
    let temp = TempDir::new("downloads-wait");
    let dir = FsDir(temp.0.clone());
    let held = std::cell::RefCell::new(Some(block_on(dir.take(FILE, Make::IfMissing)).unwrap()));
    let waiting = Waiting {
        dir: &dir,
        held: &held,
    };
    block_on(update(&waiting, |downloads| {
        downloads.record("a.vrdp", UnixSeconds(1), None);
    }))
    .unwrap();
    assert!(held.borrow().is_none());
    assert_ne!(
        block_on(load(&dir)).status("a.vrdp", None, false),
        DownloadStatus::Never
    );
}

/// [`FsDir`] with a file another tab holds, let go of as one pauses.
struct Waiting<'a> {
    dir: &'a FsDir,
    held: &'a std::cell::RefCell<Option<std::fs::File>>,
}

impl Dir for Waiting<'_> {
    type File = std::fs::File;
    type Reader = std::fs::File;

    async fn names(&self) -> io::Result<Vec<String>> {
        self.dir.names().await
    }

    async fn take(&self, name: &str, make: Make) -> io::Result<std::fs::File> {
        self.dir.take(name, make).await
    }

    async fn read(&self, name: &str, max: usize) -> io::Result<Vec<u8>> {
        self.dir.read(name, max).await
    }

    async fn reader(&self, name: &str) -> io::Result<std::fs::File> {
        self.dir.reader(name).await
    }

    async fn pause(&self, _millis: u32) {
        self.held.borrow_mut().take();
    }

    async fn modified(&self, name: &str) -> Option<UnixSeconds> {
        self.dir.modified(name).await
    }

    async fn remove(&self, name: &str) -> io::Result<()> {
        self.dir.remove(name).await
    }

    async fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        self.dir.rename(from, to).await
    }
}

/// At most [`MAX`] designs' downloads are kept: the oldest go.
#[test]
fn the_oldest_downloads_go_past_the_most_kept() {
    let mut downloads = Downloads::default();
    for n in 0..=MAX {
        // The first is the newest.
        let time = if n == 0 { i64::MAX } else { n as i64 };
        downloads.record(&format!("{n}.vrdp"), UnixSeconds(time), None);
    }
    assert_eq!(downloads.entries.len(), MAX);
    assert_ne!(
        downloads.status("0.vrdp", None, false),
        DownloadStatus::Never
    );
    assert_eq!(
        downloads.status("1.vrdp", None, false),
        DownloadStatus::Never
    );
}

/// Through a directory: recorded while the file is held, read back, and a
/// file gone bad or too large starts over.
#[test]
fn downloads_are_kept_in_their_file() {
    let temp = TempDir::new("downloads-file");
    let dir = FsDir(temp.0.clone());
    assert_eq!(block_on(load(&dir)), Downloads::default());
    let answer = block_on(update(&dir, |downloads| {
        downloads.record("a.vrdp", UnixSeconds(10), Some(SUM));
        7
    }))
    .unwrap();
    assert_eq!(answer, 7);
    block_on(update(&dir, |downloads| {
        downloads.record("b.vrdp", UnixSeconds(9), None);
    }))
    .unwrap();
    let loaded = block_on(load(&dir));
    assert_eq!(
        loaded.status("a.vrdp", Some(SUM), false),
        DownloadStatus::Latest(UnixSeconds(10))
    );
    assert_eq!(
        loaded.status("b.vrdp", Some(SUM), false),
        DownloadStatus::Changed(UnixSeconds(9))
    );

    // Held by another tab for longer than a moment: refused.
    let held = block_on(dir.take(FILE, Make::No)).unwrap();
    assert!(block_on(update(&dir, |_| ())).is_err());
    drop(held);

    std::fs::write(temp.0.join(FILE), vec![b'#'; MAX_BYTES + 1]).unwrap();
    assert_eq!(block_on(load(&dir)), Downloads::default());
    block_on(update(&dir, |downloads| {
        downloads.record("c.vrdp", UnixSeconds(1), None);
    }))
    .unwrap();
    let loaded = block_on(load(&dir));
    assert_eq!(loaded.entries.len(), 1);
    assert!(std::fs::metadata(temp.0.join(FILE)).unwrap().len() < 200);
}
