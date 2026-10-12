//! The web's stand-in for `mcp.rs`: the MCP server is native only, so
//! this [`Mcp`] has its hooks as no-ops, and `lib.rs` calls them on
//! either target without a `cfg` at each.

use iced::Subscription;

use crate::doc::Doc;
use crate::message::Message;

/// No server, no session, no panel.
#[derive(Debug, Default)]
pub(crate) struct Mcp {}

impl Mcp {
    /// No server to run.
    pub(crate) fn subscription(&self, _on: bool) -> Subscription<Message> {
        Subscription::none()
    }

    pub(crate) fn configure(&mut self, _on: bool, _port: u16) {}

    pub(crate) fn design_closed(&mut self) {}

    /// No panel: the document screen shows none.
    pub(crate) fn panel<'a>(
        &'a self,
        _on: bool,
        _port: u16,
        _now: varde_io::UnixSeconds,
        _doc: Option<&'a Doc>,
    ) -> Option<varde_view::McpShown<'a>> {
        None
    }

    pub(crate) fn shown(&self, _now: varde_io::UnixSeconds) -> varde_view::McpState<'_> {
        varde_view::McpState::default()
    }

    pub(crate) fn disconnect(&self) {}

    pub(crate) fn tick(&mut self, _now: iced::time::Instant, _doc: Option<&mut Doc>) {}

    pub(crate) fn closing(&mut self, _doc: Option<&mut Doc>) {}

    pub(crate) fn quitting(&mut self, _doc: Option<&mut Doc>) {}

    pub(crate) fn sync(&mut self, _doc: Option<&mut Doc>) {}
}
