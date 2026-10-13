//! The text adding the MCP server to a client (Claude Code, Codex,
//! opencode), with the program's path quoted for the shell it's pasted
//! into, and the port typed for it: no UI here.

use crate::mcp_port_offered;

/// The port typed into the MCP tab's field, if it's one it takes: digits
/// alone, one [`mcp_port_offered`] takes.
pub fn mcp_port_of(typed: &str) -> Option<u16> {
    let typed = typed.trim();
    if typed.is_empty() || typed.len() > 5 || !typed.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    typed
        .parse::<u16>()
        .ok()
        .filter(|&port| mcp_port_offered(port))
}

/// The lines adding the server at `port` to a client, carrying `token`:
/// Claude Code's command, Codex's `~/.codex/config.toml` table (its
/// `mcp add` takes no header) and opencode's `opencode.json` entry,
/// under `"mcp"` (no OAuth, which it would otherwise try).
pub fn mcp_client_lines(port: u16, token: &str) -> [String; 3] {
    let url = format!("http://127.0.0.1:{port}/mcp");
    [
        format!(
            "claude mcp add --transport http varde {url} --header \"Authorization: Bearer {token}\""
        ),
        format!(
            "[mcp_servers.varde]\nurl = \"{url}\"\nhttp_headers = {{ Authorization = \"Bearer {token}\" }}"
        ),
        format!(
            "\"varde\": {{ \"type\": \"remote\", \"url\": \"{url}\", \"oauth\": false, \"headers\": {{ \"Authorization\": \"Bearer {token}\" }} }}"
        ),
    ]
}

/// The lines adding the server to a client through `varde mcp`, the
/// stdio relay, run from `exe`: Claude Code's command, Codex's
/// `~/.codex/config.toml` table and opencode's `opencode.json` entry,
/// as [`mcp_client_lines`] has them, but with no token: the relay reads
/// it. The path quoted for the shell the command is pasted into (on
/// Windows double quotes, which cmd and PowerShell both read, else
/// POSIX's single). `None` if the path can't be quoted safely: a control
/// character in it (a newline would run half the command when pasted),
/// or on Windows something cmd or PowerShell expands or ends a string at
/// even in double quotes (`%`, `$`, `` ` ``, `!`, `"`, `“`, `”`, `„`).
pub fn mcp_stdio_lines(exe: &str) -> Option<[String; 3]> {
    mcp_stdio_lines_for(exe, cfg!(windows))
}

fn mcp_stdio_lines_for(exe: &str, windows: bool) -> Option<[String; 3]> {
    if exe.chars().any(char::is_control) {
        return None;
    }
    let shell = if windows {
        windows_quoted(exe)?
    } else {
        posix_quoted(exe)
    };
    let string = json_string(exe);
    Some([
        format!("claude mcp add varde -- {shell} mcp"),
        format!("[mcp_servers.varde]\ncommand = {string}\nargs = [\"mcp\"]"),
        format!("\"varde\": {{ \"type\": \"local\", \"command\": [{string}, \"mcp\"] }}"),
    ])
}

/// `word` as one POSIX shell word: as it is if nothing in it is special,
/// else in single quotes, each `'` closed, escaped and reopened.
fn posix_quoted(word: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "/._-+:@%,=".contains(c);
    if !word.is_empty() && word.chars().all(plain) {
        return word.to_owned();
    }
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// `word` as one word for cmd and PowerShell: as it is if nothing in it
/// is special to either (PowerShell splits at `,`, splats at `@`), else
/// in double quotes; `None` if it holds what either expands or ends a
/// string at inside them.
fn windows_quoted(word: &str) -> Option<String> {
    if word
        .chars()
        .any(|c| "%$`!\"\u{201C}\u{201D}\u{201E}".contains(c))
    {
        return None;
    }
    let plain = |c: char| c.is_ascii_alphanumeric() || "/\\._-+:".contains(c);
    if !word.is_empty() && word.chars().all(plain) {
        return Some(word.to_owned());
    }
    Some(format!("\"{word}\""))
}

/// `text` as a string both JSON and TOML read: double quotes, with `"`,
/// `\\` and control characters escaped.
fn json_string(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            c if c.is_control() => quoted.push_str(&format!("\\u{:04x}", c as u32)),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::{json_string, mcp_client_lines, mcp_port_of, mcp_stdio_lines_for};

    /// The lines adding the server to a client, as the plan has them.
    #[test]
    fn the_client_lines_carry_the_port_and_token() {
        let [claude, codex, opencode] = mcp_client_lines(8123, "TOKEN");
        assert_eq!(
            claude,
            r#"claude mcp add --transport http varde http://127.0.0.1:8123/mcp --header "Authorization: Bearer TOKEN""#
        );
        assert_eq!(
            codex,
            "[mcp_servers.varde]\nurl = \"http://127.0.0.1:8123/mcp\"\nhttp_headers = { Authorization = \"Bearer TOKEN\" }"
        );
        assert_eq!(
            opencode,
            r#""varde": { "type": "remote", "url": "http://127.0.0.1:8123/mcp", "oauth": false, "headers": { "Authorization": "Bearer TOKEN" } }"#
        );
    }

    /// The stdio lines: the path quoted for the shell and escaped for TOML
    /// and JSON, no token.
    #[test]
    fn the_stdio_lines_quote_the_path() {
        let [claude, codex, opencode] = mcp_stdio_lines_for("/usr/bin/varde", false).unwrap();
        assert_eq!(claude, "claude mcp add varde -- /usr/bin/varde mcp");
        assert_eq!(
            codex,
            "[mcp_servers.varde]\ncommand = \"/usr/bin/varde\"\nargs = [\"mcp\"]"
        );
        assert_eq!(
            opencode,
            r#""varde": { "type": "local", "command": ["/usr/bin/varde", "mcp"] }"#
        );

        let odd = r#"/home/a b/it's "x"\y/varde"#;
        let [claude, codex, opencode] = mcp_stdio_lines_for(odd, false).unwrap();
        assert_eq!(
            claude,
            r#"claude mcp add varde -- '/home/a b/it'\''s "x"\y/varde' mcp"#
        );
        let escaped = r#""/home/a b/it's \"x\"\\y/varde""#;
        assert_eq!(
            codex,
            format!("[mcp_servers.varde]\ncommand = {escaped}\nargs = [\"mcp\"]")
        );
        let table: toml::Table = codex.parse().unwrap();
        assert_eq!(table["mcp_servers"]["varde"]["command"].as_str(), Some(odd));
        assert_eq!(
            opencode,
            format!(r#""varde": {{ "type": "local", "command": [{escaped}, "mcp"] }}"#)
        );

        let [claude, ..] = mcp_stdio_lines_for(r"C:\Program Files\Varde\varde.exe", true).unwrap();
        assert_eq!(
            claude,
            r#"claude mcp add varde -- "C:\Program Files\Varde\varde.exe" mcp"#
        );
        let [claude, ..] = mcp_stdio_lines_for(r"C:\Varde\varde.exe", true).unwrap();
        assert_eq!(claude, r"claude mcp add varde -- C:\Varde\varde.exe mcp");
        // A control character escaped for TOML and JSON.
        assert_eq!(json_string("/a\nb\u{7f}"), r#""/a\u000ab\u007f""#);
    }

    /// A path a shell would read differently from the program's path gets no
    /// lines: a newline anywhere (pasted, it runs half the command), and on
    /// Windows what cmd or PowerShell expands or ends at in double quotes.
    /// Unquoted, Windows words hold nothing PowerShell splits or splats at.
    #[test]
    fn the_stdio_lines_refuse_paths_a_shell_misreads() {
        assert_eq!(mcp_stdio_lines_for("/a\nb/varde", false), None);
        assert_eq!(mcp_stdio_lines_for("C:\\a\rb\\varde.exe", true), None);
        for path in [
            r"C:\%USERPROFILE%\varde.exe",
            r"C:\$env:HOME\varde.exe",
            r"C:\a`b\varde.exe",
            r"C:\a!b!\varde.exe",
            "C:\\a\u{201C} b\\varde.exe",
            "C:\\a\u{201D}\\varde.exe",
            "C:\\a\u{201E}\\varde.exe",
        ] {
            assert_eq!(mcp_stdio_lines_for(path, true), None, "{path}");
        }
        // POSIX single quotes hold `$`, backticks, `!` and `\` as they are.
        let [claude, ..] = mcp_stdio_lines_for("/a$b`c`!d\\e/varde", false).unwrap();
        assert_eq!(claude, r"claude mcp add varde -- '/a$b`c`!d\e/varde' mcp");
        for (path, quoted) in [
            (r"C:\a,b\varde.exe", r#""C:\a,b\varde.exe""#),
            (r"C:\@a\varde.exe", r#""C:\@a\varde.exe""#),
            (r"C:\a=b\varde.exe", r#""C:\a=b\varde.exe""#),
            ("C:\\Vård\\varde.exe", "\"C:\\Vård\\varde.exe\""),
        ] {
            let [claude, ..] = mcp_stdio_lines_for(path, true).unwrap();
            assert_eq!(claude, format!("claude mcp add varde -- {quoted} mcp"));
        }
    }

    #[test]
    fn the_port_field_takes_digits_in_range() {
        for (typed, port) in [
            ("7311", Some(7311)),
            (" 1024 ", Some(1024)),
            ("65535", Some(65535)),
            ("01024", Some(1024)),
            ("1023", None),
            ("65536", None),
            ("99999999999", None),
            ("+8000", None),
            ("80a", None),
            ("", None),
        ] {
            assert_eq!(mcp_port_of(typed), port, "{typed:?}");
        }
    }
}
