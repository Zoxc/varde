use super::*;
use crate::tests::TempDir;

fn new() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    options
}

#[test]
fn names_differ() {
    assert_ne!(name(), name());
}

#[test]
fn creates_a_new_file() {
    let dir = TempDir::new("unique-new");
    let (path, _file) = create(&new(), 4, |name| dir.0.join(name)).unwrap();
    assert!(path.exists());
}

/// A path that's always taken is tried `attempts` more times, then given
/// up on.
#[test]
fn gives_up_on_taken_names() {
    let dir = TempDir::new("unique-taken");
    let taken = dir.0.join("taken");
    std::fs::write(&taken, "").unwrap();
    let tries = std::cell::Cell::new(0);
    let error = create(&new(), 4, |_| {
        tries.set(tries.get() + 1);
        taken.clone()
    })
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(tries.get(), 5);
}
