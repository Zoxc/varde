use super::*;
use crate::tests::TempDir;

#[test]
fn load_and_discard_through_a_file() {
    let dir = TempDir::new("panicked");
    let store = dir.0.join("sub").join("panic.toml");
    assert_eq!(load(&store), None);
    let panic = Panic::new(
        Some("main".to_owned()),
        "on purpose",
        Some("src/lib.rs:1:2".to_owned()),
        Some("0: here\n1: there".to_owned()),
    );
    config::replace(&store, &panic.serialize()).unwrap();
    assert_eq!(load(&store), Some(panic.clone()));
    // Not another one, recorded since.
    let other = Panic::new(None, "another", None, None);
    discard(&store, &other).unwrap();
    assert_eq!(load(&store), Some(panic.clone()));
    discard(&store, &panic).unwrap();
    assert_eq!(load(&store), None);
    // Gone already.
    discard(&store, &panic).unwrap();
}

/// A file too large to be a panic is taken as gone bad.
#[test]
fn a_huge_file_is_no_panic() {
    let dir = TempDir::new("panicked-huge");
    let store = dir.0.join("panic.toml");
    let padding = " ".repeat(usize::try_from(MAX_BYTES).unwrap());
    std::fs::write(&store, format!("message = \"m\"\n{padding}")).unwrap();
    assert_eq!(load(&store), None);
}
