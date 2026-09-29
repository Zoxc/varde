use std::path::PathBuf;

use super::*;
use crate::store::{entry_in, is_entry_name};

#[test]
fn the_store_is_plain_names() {
    assert_eq!(dir_names(Path::new("designs")), Some(vec!["designs"]));
    assert_eq!(dir_names(Path::new("a/b")), Some(vec!["a", "b"]));
    assert_eq!(dir_names(Path::new("")), None);
    assert_eq!(dir_names(Path::new("/designs")), None);
    assert_eq!(dir_names(Path::new("../designs")), None);
    assert_eq!(dir_names(Path::new("a/./b")), Some(vec!["a", "b"]));
}

#[test]
fn new_names_are_entries_whatever_the_numbers() {
    let name = new_name(1_700_000_000_123.0, 0.5, 7);
    assert!(is_entry_name(&name), "{name}");
    assert_eq!(name, "18bcfe5687b-7fffffff-7.vrdp");
    assert_ne!(new_name(1.0, 0.5, 7), new_name(1.0, 0.5, 8));
    for (millis, random) in [
        (f64::NAN, f64::NAN),
        (f64::INFINITY, f64::INFINITY),
        (-1.0, -1.0),
        (f64::MAX, 2.0),
    ] {
        let name = new_name(millis, random, u64::MAX);
        assert!(is_entry_name(&name), "{name}");
        let path = PathBuf::from("designs").join(&name);
        assert_eq!(entry_in(Path::new("designs"), &path), Some(name.as_str()));
    }
}

#[test]
fn a_read_or_write_never_does_more_than_asked() {
    assert_eq!(done(0.0, 10).unwrap(), 0);
    assert_eq!(done(10.0, 10).unwrap(), 10);
    assert!(done(11.0, 10).is_err());
    assert!(done(-1.0, 10).is_err());
    assert!(done(f64::NAN, 10).is_err());
}

#[test]
fn a_new_entry_lost_to_another_tab_is_a_name_to_skip() {
    // Another tab holds it, or listed it as empty and deleted it between
    // its making and its taking.
    assert!(lost_to_another_tab(io::ErrorKind::ResourceBusy));
    assert!(lost_to_another_tab(io::ErrorKind::NotFound));
    assert!(!lost_to_another_tab(io::ErrorKind::StorageFull));
    assert!(!lost_to_another_tab(io::ErrorKind::PermissionDenied));
    assert!(!lost_to_another_tab(io::ErrorKind::Other));
}
