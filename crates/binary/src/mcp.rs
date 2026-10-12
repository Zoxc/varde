//! `varde mcp [--port <n>]`: the stdio relay to the running app's MCP
//! server (`varde_mcp::relay`), for clients that speak stdio alone, so
//! their configs needn't hold the token. No window, iced or wgpu: stdin
//! and stdout are the protocol's, anything else goes to stderr.
//!
//! The port is `--port`'s, else `settings.toml`'s `mcp_port`; the token
//! is `mcp-token.toml`'s, read by `varde_io` without ever being written,
//! when first needed and again after a 401. Both are the user's files,
//! read on this thread rather than through the IO lane: this is a process
//! of its own, holding no design.
//!
//! Exits 0 when stdin ends, 1 if stdin or stdout fail, 2 on bad
//! arguments (with the usage on stderr).

use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use varde_io::Stores;
use varde_io::settings::McpPort;
use varde_mcp::relay::{self, Target};

const USAGE: &str = "usage: varde mcp [--port <n>]\n\
\n\
Relays MCP's stdio transport to the running Varde's MCP server: start\n\
it in Varde's MCP panel. --port is the server's (1024 to 65535), by\n\
default the one in Varde's settings.";

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    /// The app: no `mcp` first.
    App,
    /// The relay, to this port if given.
    Relay { port: Option<McpPort> },
    /// The usage asked for.
    Help,
    /// Arguments it doesn't take: what's wrong.
    Bad(String),
}

/// Reads the arguments after the program's name.
fn parse(args: impl IntoIterator<Item = OsString>) -> Command {
    let mut args = args.into_iter();
    if args.next().as_deref() != Some("mcp".as_ref()) {
        return Command::App;
    }
    let mut port = None;
    while let Some(arg) = args.next() {
        let Some(arg) = arg.to_str() else {
            return Command::Bad(format!("unexpected argument {arg:?}"));
        };
        let value = match arg {
            "-h" | "--help" => return Command::Help,
            "--port" => match args.next() {
                Some(value) => value,
                None => return Command::Bad("--port needs a value".to_owned()),
            },
            _ => match arg.strip_prefix("--port=") {
                Some(value) => value.into(),
                None => return Command::Bad(format!("unexpected argument {arg:?}")),
            },
        };
        if port.is_some() {
            return Command::Bad("--port given twice".to_owned());
        }
        let parsed = value.to_str().and_then(McpPort::parse);
        match parsed {
            Some(parsed) => port = Some(parsed),
            None => {
                return Command::Bad(format!(
                    "--port takes a number from {} to {}, not {value:?}",
                    McpPort::MIN,
                    McpPort::MAX
                ));
            }
        }
    }
    Command::Relay { port }
}

/// Runs `varde mcp` if the arguments ask for it, with the process's
/// stdio and the user's files, giving the exit code; `None` for the app.
pub(crate) fn main(args: impl IntoIterator<Item = OsString>) -> Option<ExitCode> {
    match parse(args) {
        Command::App => None,
        Command::Help => {
            eprintln!("{USAGE}");
            Some(ExitCode::SUCCESS)
        }
        Command::Bad(why) => {
            eprintln!("varde mcp: {why}\n\n{USAGE}");
            Some(ExitCode::from(2))
        }
        Command::Relay { port } => Some(serve(
            port,
            &Stores::user(),
            io::stdin().lock(),
            io::stdout().lock(),
        )),
    }
}

/// Relays `stdin` to the server at `port`, else the one in `stores`'
/// settings, with `stores`' token, until `stdin` ends.
fn serve(
    port: Option<McpPort>,
    stores: &Stores,
    stdin: impl BufRead,
    stdout: impl Write,
) -> ExitCode {
    let port = port.unwrap_or_else(|| stores.read_settings_now().mcp_port);
    let target = Target {
        port: port.into(),
        token_source: || stores.read_mcp_token(),
    };
    match relay::run(stdin, stdout, target) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("varde mcp: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests;
