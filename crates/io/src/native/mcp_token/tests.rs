use super::*;
use crate::tests::TempDir;

#[test]
fn written_and_loaded() {
    let dir = TempDir::new("mcp-token");
    let store = dir.0.join("sub").join("mcp-token.toml");
    assert!(load(&store).is_none());
    let token = generate().unwrap();
    write(&store, &token).unwrap();
    assert_eq!(load(&store), Some(token.clone()));
    let text = std::fs::read_to_string(&store).unwrap();
    assert!(text.contains(&format!("token = \"{token}\"")), "{text}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&store).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // Replaced by another.
    let other = generate().unwrap();
    assert_ne!(other, token);
    write(&store, &other).unwrap();
    assert_eq!(load(&store), Some(other));
}

#[test]
fn what_isnt_a_token_isnt_loaded() {
    let dir = TempDir::new("mcp-token-bad");
    let store = dir.0.join("mcp-token.toml");
    let token = generate().unwrap();
    for bad in [
        String::new(),
        "token = 7\n".to_owned(),
        "token = \"short\"\n".to_owned(),
        format!("token = \"{token}x\"\n"),
        format!("not toml {token}"),
        format!("token = \"{token}\"\n{}", " ".repeat(2000)),
    ] {
        write_mode(&store, &bad, 0o600);
        assert!(load(&store).is_none(), "{bad:?}");
    }
    write_mode(&store, &format!("token = \"{token}\"\n"), 0o600);
    assert_eq!(load(&store), Some(token));
}

/// Writes `text` to `store` at `mode`, in place.
fn write_mode(store: &Path, text: &str, mode: u32) {
    std::fs::write(store, text).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(store, std::fs::Permissions::from_mode(mode)).unwrap();
    }
    #[cfg(not(unix))]
    let _ = mode;
}

#[cfg(unix)]
#[test]
fn others_can_read_it_or_a_symlink_isnt_trusted() {
    let dir = TempDir::new("mcp-token-untrusted");
    let store = dir.0.join("mcp-token.toml");
    let token = generate().unwrap();
    let text = format!("token = \"{token}\"\n");
    for mode in [0o644, 0o640, 0o604, 0o700, 0o400] {
        write_mode(&store, &text, mode);
        assert!(load(&store).is_none(), "{mode:o}");
    }
    let real = dir.0.join("elsewhere.toml");
    write_mode(&real, &text, 0o600);
    assert_eq!(load(&real), Some(token));
    std::fs::remove_file(&store).unwrap();
    std::os::unix::fs::symlink(&real, &store).unwrap();
    assert!(load(&store).is_none());
    // A directory in its place neither.
    let in_place = dir.0.join("a-directory.toml");
    std::fs::create_dir(&in_place).unwrap();
    assert!(load(&in_place).is_none());
}

/// `Stores::read_mcp_token`, the relay's: a token it can trust, else
/// the error, the file never made or rewritten.
#[test]
fn the_relay_reads_without_writing() {
    let dir = TempDir::new("mcp-token-relay");
    let store = dir.0.join("mcp-token.toml");
    let stores = crate::Stores {
        mcp_token: Some(store.clone()),
        ..crate::Stores::default()
    };
    let refused = |stores: &crate::Stores| {
        let error = stores.read_mcp_token().unwrap_err();
        assert_eq!(error, crate::NO_MCP_TOKEN);
    };
    // Missing: still missing after.
    refused(&stores);
    assert!(!store.exists());
    refused(&crate::Stores::default());

    let token = generate().unwrap();
    let good = format!("token = \"{token}\"\n");
    let unchanged = |text: &str| {
        assert_eq!(std::fs::read_to_string(&store).unwrap(), text);
    };
    // Junk.
    write_mode(&store, "token = \"short\"\n", 0o600);
    refused(&stores);
    unchanged("token = \"short\"\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Others can read it.
        write_mode(&store, &good, 0o644);
        refused(&stores);
        unchanged(&good);
        let mode = std::fs::metadata(&store).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o644);
        // A symlink to a good one.
        let real = dir.0.join("real.toml");
        write_mode(&real, &good, 0o600);
        std::fs::remove_file(&store).unwrap();
        std::os::unix::fs::symlink(&real, &store).unwrap();
        refused(&stores);
        assert!(std::fs::symlink_metadata(&store).unwrap().is_symlink());
        std::fs::remove_file(&store).unwrap();
    }
    // A good one, and the error never holds the token.
    write_mode(&store, &good, 0o600);
    assert_eq!(stores.read_mcp_token(), Ok(token.clone()));
    unchanged(&good);
    assert!(!crate::NO_MCP_TOKEN.contains(&token.to_string()));
}

/// A FIFO swapped in is opened without blocking, and isn't read as a
/// token; a symlink isn't followed.
#[cfg(target_os = "linux")]
#[test]
fn a_fifo_or_symlink_doesnt_block_or_get_followed() {
    let dir = TempDir::new("mcp-token-fifo");
    let fifo = dir.0.join("fifo");
    let made = std::process::Command::new("mkfifo").arg(&fifo).status();
    if !made.is_ok_and(|s| s.success()) {
        eprintln!("no mkfifo, skipping");
        return;
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    let opening = fifo.clone();
    std::thread::spawn(move || {
        let opened = open(&opening).map(|file| file.metadata().unwrap().is_file());
        let _ = sender.send(opened.ok());
    });
    let opened = receiver
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("opening a FIFO blocked");
    assert_eq!(opened, Some(false));
    assert!(load(&fifo).is_none());

    let real = dir.0.join("real.toml");
    write_mode(&real, "token = \"x\"\n", 0o600);
    let link = dir.0.join("link.toml");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    assert!(open(&link).is_err());
}
