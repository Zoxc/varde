//! The [`Dir`] contract, as `FsDir` keeps it natively and the Origin
//! Private File System on the web: what the code over a directory relies
//! on.

use std::io::ErrorKind;

use pollster::block_on;

use super::fs::FsDir;
use super::*;
use crate::tests::TempDir;
use crate::vrdp::ReadAt;

fn dir(name: &str) -> (TempDir, FsDir) {
    let temp = TempDir::new(name);
    let dir = FsDir(temp.0.clone());
    (temp, dir)
}

#[test]
fn names_are_plain() {
    for name in ["a.vrdp", ".a.vrdp.autosave", "a b (2).vrdp", "ünïcode"] {
        assert!(is_plain_name(name), "{name}");
    }
    for name in ["", ".", "..", "a/b", "a\\b", "a\0b"] {
        assert!(!is_plain_name(name), "{name:?}");
    }
}

/// A file taken is the taker's alone: nobody else can take it, read it,
/// delete it or rename it till it's let go of.
#[test]
fn a_file_taken_is_held_till_let_go_of() {
    let (_temp, dir) = dir("dir-take");
    let mut file = block_on(dir.take("a", Make::New)).unwrap();
    file.write_at(b"held", 0).unwrap();
    let busy = |result: std::io::Result<()>| result.unwrap_err().kind() == ErrorKind::ResourceBusy;
    assert!(busy(block_on(dir.take("a", Make::IfMissing)).map(drop)));
    assert!(busy(block_on(dir.read("a", 100)).map(drop)));
    assert!(busy(block_on(dir.remove("a"))));
    assert!(busy(block_on(dir.rename("a", "b"))));
    // Removing one held is fine, as it's someone else's then.
    block_on(remove_if_free(&dir, "a")).unwrap();
    drop(file);
    assert_eq!(block_on(dir.read("a", 100)).unwrap(), b"held");
    let again = block_on(dir.take("a", Make::No)).unwrap();
    assert_eq!(again.len().unwrap(), 4);
}

/// Taking finds or makes the file as asked.
#[test]
fn taking_makes_the_file_as_asked() {
    let (_temp, dir) = dir("dir-make");
    let kind = |make| block_on(dir.take("a", make)).map(drop).unwrap_err().kind();
    assert_eq!(kind(Make::No), ErrorKind::NotFound);
    drop(block_on(dir.take("a", Make::IfMissing)).unwrap());
    drop(block_on(dir.take("a", Make::IfMissing)).unwrap());
    assert_eq!(kind(Make::New), ErrorKind::AlreadyExists);
    drop(block_on(dir.take("a", Make::No)).unwrap());
    assert_eq!(block_on(dir.names()).unwrap(), ["a"]);
}

/// Reading is bounded, deleting and renaming say what's missing or there.
#[test]
fn reading_deleting_and_renaming() {
    let (_temp, dir) = dir("dir-rename");
    let mut file = block_on(dir.take("a", Make::New)).unwrap();
    file.write_at(b"0123456789", 0).unwrap();
    drop(file);
    assert_eq!(
        block_on(dir.read("a", 9)).unwrap_err().kind(),
        ErrorKind::FileTooLarge
    );
    assert_eq!(
        block_on(dir.read("b", 9)).unwrap_err().kind(),
        ErrorKind::NotFound
    );
    assert!(block_on(dir.modified("a")).is_some());
    drop(block_on(dir.take("b", Make::New)).unwrap());
    assert_eq!(
        block_on(dir.rename("a", "b")).unwrap_err().kind(),
        ErrorKind::AlreadyExists
    );
    block_on(dir.remove("b")).unwrap();
    assert_eq!(
        block_on(dir.remove("b")).unwrap_err().kind(),
        ErrorKind::NotFound
    );
    block_on(remove_if_free(&dir, "b")).unwrap();
    block_on(dir.rename("a", "b")).unwrap();
    assert_eq!(block_on(dir.read("b", 10)).unwrap(), b"0123456789");
    assert_eq!(block_on(dir.names()).unwrap(), ["b"]);
    assert!(block_on(dir.take("../b", Make::IfMissing)).is_err());
}
