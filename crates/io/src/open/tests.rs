use super::*;

#[test]
fn each_file_gets_its_own_id() {
    let mut open = OpenFiles::new();
    let first = open.add(Some(OpenId(1)), ());
    let second = open.add(None, ());
    assert_ne!(first, second);
    assert!(open.remove(first).is_ok());
    // Never reused, so a stale id can't reach another file.
    let third = open.add(None, ());
    assert_ne!(third, first);
    assert_ne!(third, second);
}

#[test]
fn a_file_not_open_says_so() {
    let mut open = OpenFiles::new();
    let file = open.add(None, ());
    open.remove(file).unwrap();
    assert_eq!(open.get_mut(file).err(), Some(NotOpen));
    assert_eq!(open.remove(file).err(), Some(NotOpen));
    assert_eq!(String::from(NotOpen), "the file isn't open");
    assert_eq!(
        SaveError::from(NotOpen),
        SaveError::Failed("the file isn't open".to_owned())
    );
}

#[test]
fn files_are_found_by_what_opened_them() {
    let mut open = OpenFiles::new();
    open.add(None, ());
    let file = open.add(Some(OpenId(3)), ());
    open.add(Some(OpenId(4)), ());
    assert_eq!(open.opened_by(OpenId(3)), Some(file));
    assert_eq!(open.opened_by(OpenId(5)), None);
}

/// Recovered designs are listed again after opening or discarding one, and
/// after a close or abandon of a file with a store entry, the lane says
/// which.
#[test]
fn what_may_change_the_recovered_designs_relists_them() {
    let mut open = OpenFiles::new();
    let entry = open.add(Some(OpenId(1)), true);
    let sidecar = open.add(Some(OpenId(2)), false);
    let relists = |request: Request| open.relists(&request, |&entry: &bool| entry);
    let close = |file| Request::Close {
        file,
        closing: crate::Closing::Clean,
    };
    assert!(relists(close(entry)));
    assert!(relists(Request::Abandon { id: OpenId(1) }));
    assert!(!relists(close(sidecar)));
    assert!(!relists(Request::Abandon { id: OpenId(2) }));
    assert!(!relists(Request::Abandon { id: OpenId(9) }));
    assert!(!relists(close(FileId(99))));
    assert!(relists(Request::DiscardRecovered {
        path: "designs/a.vrdp".into()
    }));
    assert!(relists(Request::OpenRecovered {
        id: OpenId(3),
        path: "designs/a.vrdp".into()
    }));
    assert!(!relists(Request::ListRecovered));
}
