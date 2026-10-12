use std::path::PathBuf;

use futures::executor::block_on_stream;
use serde_json::{Value, json};
use varde_mcp::guard::Token;
use varde_mcp::server::{self, Event};

use super::*;

fn args(args: &[&str]) -> Command {
    parse(args.iter().map(OsString::from))
}

fn port(port: u16) -> Command {
    Command::Relay {
        port: Some(McpPort::new(port).unwrap()),
    }
}

#[test]
fn the_arguments() {
    assert_eq!(args(&[]), Command::App);
    assert_eq!(args(&["design.vrdp"]), Command::App);
    assert_eq!(args(&["--port", "8000"]), Command::App);
    assert_eq!(args(&["mcp"]), Command::Relay { port: None });
    assert_eq!(args(&["mcp", "--port", "8000"]), port(8000));
    assert_eq!(args(&["mcp", "--port=1024"]), port(1024));
    assert_eq!(args(&["mcp", "--port", "65535"]), port(65535));
    assert_eq!(args(&["mcp", "--help"]), Command::Help);
    assert_eq!(args(&["mcp", "--port", "8000", "-h"]), Command::Help);
    for bad in [
        &["mcp", "--port"][..],
        &["mcp", "--port", "1023"],
        &["mcp", "--port", "65536"],
        &["mcp", "--port", "0"],
        &["mcp", "--port", "+8000"],
        &["mcp", "--port", "99999999999999999999"],
        &["mcp", "--port", ""],
        &["mcp", "--port=x"],
        &["mcp", "--port", "8000", "--port", "8001"],
        &["mcp", "8000"],
        &["mcp", "--verbose"],
    ] {
        assert!(matches!(args(bad), Command::Bad(_)), "{bad:?}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let odd = [OsString::from("mcp"), OsString::from_vec(vec![0xff])];
        assert!(matches!(parse(odd), Command::Bad(_)));
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("varde-bin-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn lines(lines: &[Value]) -> io::Cursor<Vec<u8>> {
    let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
    io::Cursor::new(text.into_bytes())
}

fn initialize() -> Value {
    json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2025-06-18", "clientInfo": { "name": "test" } },
    })
}

/// The relay's entry, against a server on port 0, the port from the
/// settings and the token from its file; without the file, the error
/// saying to turn MCP on.
#[cfg(unix)]
#[test]
fn relays_with_the_users_files() {
    use std::os::unix::fs::PermissionsExt;

    let token = Token::from_bytes([7; Token::BYTES]);
    let mut events = block_on_stream(server::start(0, token.clone()));
    let handle = match events.next() {
        Some(Event::Ready(handle)) => handle,
        other => panic!("expected Ready, got {other:?}"),
    };
    let listening = handle.address().port();
    let dir = TempDir::new("relay");
    let stores = Stores {
        settings: Some(dir.0.join("settings.toml")),
        mcp_token: Some(dir.0.join("mcp-token.toml")),
        ..Stores::default()
    };
    std::fs::write(
        stores.settings.as_ref().unwrap(),
        format!("mcp_port = {listening}\n"),
    )
    .unwrap();
    let ping = json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" });
    let input = [
        initialize(),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        ping.clone(),
    ];

    // No token yet.
    let mut out = Vec::new();
    assert_eq!(
        serve(None, &stores, lines(&input), &mut out),
        ExitCode::SUCCESS
    );
    let out: Vec<Value> = (out.split(|&b| b == b'\n'))
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert_eq!(out.len(), 2, "{out:?}");
    assert_eq!(out[0]["error"]["code"], -32000);
    assert_eq!(out[0]["error"]["message"], varde_io::NO_MCP_TOKEN);

    let store = stores.mcp_token.as_ref().unwrap();
    std::fs::write(store, format!("token = \"{token}\"\n")).unwrap();
    std::fs::set_permissions(store, std::fs::Permissions::from_mode(0o600)).unwrap();
    let mut out = Vec::new();
    // Given the port, the settings' isn't read: a wrong one there.
    std::fs::write(stores.settings.as_ref().unwrap(), "mcp_port = 1024\n").unwrap();
    let given = McpPort::new(listening);
    assert_eq!(
        serve(given, &stores, lines(&input), &mut out),
        ExitCode::SUCCESS
    );
    let text = String::from_utf8(out).unwrap();
    assert!(!text.contains(&token.to_string()), "{text}");
    let out: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(out.len(), 2, "{out:?}");
    assert_eq!(out[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(out[1], json!({ "jsonrpc": "2.0", "id": 2, "result": {} }));
}

/// Nothing listening: answered, and the relay runs on to stdin's end.
#[test]
fn not_running_is_an_answer() {
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = McpPort::new(free.local_addr().unwrap().port());
    drop(free);
    let stores = Stores::default();
    let mut out = Vec::new();
    assert_eq!(
        serve(port, &stores, lines(&[initialize()]), &mut out),
        ExitCode::SUCCESS
    );
    let out: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(out["error"]["code"], -32000);
}

/// A stdout that fails is exit code 1.
#[test]
fn a_broken_stdout_fails() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
    }
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = McpPort::new(free.local_addr().unwrap().port());
    drop(free);
    let code = serve(port, &Stores::default(), lines(&[initialize()]), Broken);
    assert_eq!(code, ExitCode::FAILURE);
}
