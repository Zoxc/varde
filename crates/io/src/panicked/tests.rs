use super::*;

#[test]
fn round_trips() {
    let panic = Panic::new(
        Some("main".to_owned()),
        "it \"broke\"\non two lines",
        Some("src/lib.rs:1:2".to_owned()),
        Some("0: here\n1: there".to_owned()),
    );
    assert_eq!(Panic::parse(&panic.serialize()), Some(panic.clone()));
    let bare = Panic {
        time: None,
        version: None,
        thread: None,
        message: "m".to_owned(),
        location: None,
        backtrace: None,
    };
    assert_eq!(bare.serialize(), "message = \"m\"\n");
    assert_eq!(Panic::parse(&bare.serialize()), Some(bare));
}

#[test]
fn keys_gone_bad_are_left_out() {
    assert_eq!(Panic::parse("not toml ="), None);
    assert_eq!(Panic::parse("time = 3"), None);
    assert_eq!(Panic::parse("message = 3"), None);
    let panic =
        Panic::parse("message = \"m\"\ntime = \"soon\"\nthread = 1\nlocation = \"a:1:2\"").unwrap();
    assert_eq!(panic.time, None);
    assert_eq!(panic.thread, None);
    assert_eq!(panic.location.as_deref(), Some("a:1:2"));
}

#[test]
fn long_keys_are_cut_at_a_character() {
    assert_eq!(bounded("abc".to_owned(), 3), "abc");
    assert_eq!(bounded("abc".to_owned(), 2), "ab");
    // "é" is two bytes.
    assert_eq!(bounded("aé".to_owned(), 2), "a");
    let long = "é".repeat(MAX_MESSAGE);
    let panic = Panic::parse(&format!("message = \"{long}\"")).unwrap();
    assert_eq!(panic.message.len(), MAX_MESSAGE);
}

#[test]
fn reports_what_is_known() {
    let mut panic = Panic::new(
        Some("main".to_owned()),
        "on purpose",
        Some("src/lib.rs:1:2".to_owned()),
        Some("0: here".to_owned()),
    );
    panic.version = Some("1.2.3".to_owned());
    assert_eq!(
        panic.report(),
        "Varde CAD 1.2.3 panicked on thread 'main' at src/lib.rs:1:2:\non purpose\n\n\
         Backtrace:\n0: here\n"
    );
    let bare = Panic::parse("message = \"m\"").unwrap();
    assert_eq!(bare.report(), "Varde CAD panicked:\nm\n");
}
