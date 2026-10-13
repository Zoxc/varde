//! The settings page's MCP tab: the server's switch, its port, the
//! client connected, the token and the lines adding it to a client.

use std::borrow::Cow;

use iced::widget::{Space, column, container, row, text, text_input};
use iced::{Alignment, Element, Font, Length};

use super::{Setting, SettingRow, SettingsPage, on_off_row, reset, setting, tipped_button};
use crate::chrome;
use crate::icons::Icon;
use crate::mcp::{TOKEN_HIDDEN, bind_failed, client_name};
use crate::mcp_config::{mcp_client_lines, mcp_port_of, mcp_stdio_lines};
use crate::theme::{self, Emphasis};
use crate::{
    MCP_PORT_DEFAULT, MCP_PORT_MAX, MCP_PORT_MIN, McpState, McpStatus, Message, ViewOptions,
};

/// The MCP section's rows: the switch, the port, the client while
/// serving, the token and the lines adding the server to a client.
pub(super) fn rows<'a>(options: ViewOptions, mcp: &McpState<'a>) -> Vec<SettingRow<'a>> {
    let set = |setting| Message::Settings(SettingsPage::Set(setting));
    let page = Message::Settings;
    let d = ViewOptions::default();
    let port = options.mcp_port;
    let serving = match mcp.status {
        McpStatus::Listening(bound) if options.mcp => Some(bound),
        _ => None,
    };
    let status: Cow<'static, str> = match mcp.status {
        McpStatus::Off => "Stopped: Start it in the MCP panel".into(),
        McpStatus::Listening(bound) => {
            format!("Listening at 127.0.0.1:{bound}, one client at a time").into()
        }
        McpStatus::BindFailed(_) => "Not started: see the port".into(),
        McpStatus::Starting => "Starting".into(),
    };
    let typed_bad = mcp
        .port_typed
        .is_some_and(|typed| mcp_port_of(typed).is_none());
    let (port_said, port_bad): (Cow<'static, str>, bool) = match mcp.status {
        _ if typed_bad => (
            format!("A number from {MCP_PORT_MIN} to {MCP_PORT_MAX}").into(),
            true,
        ),
        McpStatus::BindFailed(why) if options.mcp => (bind_failed(port, why).into(), true),
        _ => ("Where it listens, on 127.0.0.1 only".into(), false),
    };
    let port_text = match mcp.port_typed {
        Some(typed) => typed.to_owned(),
        None => port.to_string(),
    };
    let port_field = text_input(&MCP_PORT_DEFAULT.to_string(), &port_text)
        .on_input(|typed| Message::Settings(SettingsPage::TypeMcpPort(typed)))
        .padding([3, 6])
        .size(12.5)
        .width(70)
        .style(theme::field_input(port_bad));
    let mut rows = vec![
        on_off_row(
            "MCP",
            "Shows the MCP panel, where the server is started",
            (options.mcp, d.mcp),
            Setting::Mcp,
        ),
        SettingRow {
            bad: port_bad,
            ..setting(
                "Port",
                port_said,
                port_field.into(),
                reset(
                    port,
                    MCP_PORT_DEFAULT,
                    |port| port.to_string(),
                    |port| set(Setting::McpPort(port)),
                ),
            )
        },
    ];
    if options.mcp {
        rows.insert(1, setting("Server", status, Space::new().into(), None));
    }
    if serving.is_some() {
        let client: Element<'a, Message> = match &mcp.client {
            Some(client) => row![
                text(client_name(client.name).to_owned()).font(theme::SEMIBOLD),
                text(format!("since {}", client.since))
                    .size(12)
                    .style(theme::muted_text),
                chrome::small_button("Disconnect", Emphasis::Secondary)
                    .on_press(page(SettingsPage::DisconnectMcp)),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
            None => text("None").size(12.5).style(theme::muted_text).into(),
        };
        rows.push(setting(
            "Client",
            "Connected now; Disconnect drops it",
            client,
            None,
        ));
    }
    let copy = |what: String| tipped_button(Icon::Copy, "Copy", Message::Copy(what));
    let token: Element<'a, Message> = match &mcp.token {
        Some(token) => {
            let shown = mcp.token_shown;
            row![
                text(if shown {
                    token.clone()
                } else {
                    TOKEN_HIDDEN.to_owned()
                })
                .size(12)
                .font(Font::MONOSPACE),
                tipped_button(
                    if shown { Icon::EyeOff } else { Icon::Eye },
                    if shown { "Hide" } else { "Show" },
                    page(SettingsPage::ShowMcpToken(!shown)),
                ),
                copy(token.clone()),
                chrome::small_button("Regenerate", Emphasis::Secondary)
                    .on_press(page(SettingsPage::RegenerateMcpToken)),
            ]
            .spacing(4)
            .align_y(Alignment::Center)
            .into()
        }
        None => text("Not read yet")
            .size(12.5)
            .style(theme::muted_text)
            .into(),
    };
    rows.push(setting(
        "Token",
        "Clients send it with each request; a new one drops the client",
        token,
        None,
    ));
    // The stdio relay's lines first, holding no token; then, once the
    // token is known, the HTTP ones, shown with it as the Token row shows
    // it and copied with it.
    let stdio = mcp
        .exe
        .and_then(mcp_stdio_lines)
        .map(|lines| lines.clone().into_iter().zip(lines));
    let http = mcp.token.as_ref().map(|token| {
        let shown = if mcp.token_shown {
            token.as_str()
        } else {
            TOKEN_HIDDEN
        };
        mcp_client_lines(port, shown)
            .into_iter()
            .zip(mcp_client_lines(port, token))
    });
    if stdio.is_some() || http.is_some() {
        let carried = match (stdio.is_some(), http.is_some()) {
            (true, true) => {
                "The first three run this Varde as a relay and hold no token; the last three carry it"
            }
            (true, false) => "Each runs this Varde as a relay, which reads the token",
            _ => "Each carries the token",
        };
        // A path the lines can't hold safely is said, not offered.
        let refused = if mcp.exe.is_some() && stdio.is_none() {
            ". This Varde's path has characters a terminal would read differently, so there are no lines running it as a relay"
        } else {
            ""
        };
        let said = format!(
            "Claude Code: paste into a terminal. Codex: add to ~/.codex/config.toml. opencode: add to opencode.json under \"mcp\". {carried}{refused}"
        );
        let lines = (stdio.into_iter().flatten())
            .chain(http.into_iter().flatten())
            .map(|(shown, copied)| {
                row![
                    container(text(shown).size(11.5).font(Font::MONOSPACE))
                        .width(Length::Fill)
                        .padding([4, 8])
                        .style(theme::reset_row),
                    copy(copied),
                ]
                .spacing(6)
                .align_y(Alignment::Center)
                .into()
            });
        rows.push(SettingRow {
            below: Some(column(lines).spacing(6).into()),
            ..setting("Add to a client", said, Space::new().into(), None)
        });
    }
    rows
}
