//! The settings page: its own state, and the app's steps on it.

use varde_view::{Setting, ViewOptions};

use crate::Varde;
use crate::files::Files;
use crate::keys::PageEscape;

/// The settings page's own state: the app's, not a document's, but
/// closed by any move to another view (see [`Varde::show`]).
#[derive(Debug, Default)]
pub(crate) struct Page {
    pub(crate) open: bool,
    pub(crate) tab: varde_view::SettingsTab,
    /// Whether Reset to defaults is asking first.
    pub(crate) resetting: bool,
    /// The interface scale the slider is dragged to, set once let go.
    pub(crate) scale_dragged: Option<u16>,
    /// The MCP port as typed, while it isn't the port set.
    pub(crate) port_typed: Option<String>,
    /// Whether the MCP token shows.
    pub(crate) token_shown: bool,
}

impl Page {
    /// Closes the page, with what it was asking or being dragged to.
    pub(crate) fn close(&mut self) {
        self.open = false;
        self.resetting = false;
        // A drag not let go on the page sets nothing.
        self.scale_dragged = None;
        // Nor a port typed out of range; the token is hidden again.
        self.port_typed = None;
        self.token_shown = false;
    }
}

impl Varde {
    /// Sets `setting`, if it's a value the page offers (others are
    /// ignored), closing the view options menu, and stores it.
    pub(crate) fn set(&mut self, setting: Setting) {
        if !setting.offered() {
            return;
        }
        setting.apply(&mut self.options);
        self.with_doc(|doc, _| doc.view_menu = false);
        self.options_changed();
        self.files
            .io
            .send_all(self.settings.chose(setting, self.options));
    }

    /// Has the open document take the options as they are now: their
    /// projection, its camera's, and how it's turned; the values shown
    /// as they say; and the recent files kept as many as they say.
    pub(crate) fn options_changed(&mut self) {
        let options = self.options;
        options.shown().set();
        self.with_doc(|doc, _| doc.take_options(&options));
        self.files
            .io
            .send_all(self.files.recent.keep(usize::from(options.recent_designs)));
        self.mcp.configure(options.mcp, options.mcp_port);
    }

    /// Takes `message`, from the settings page itself.
    pub(crate) fn settings_page(&mut self, message: varde_view::SettingsPage) {
        use varde_view::SettingsPage;
        match message {
            SettingsPage::Tab(tab) => {
                self.page.tab = tab;
                self.page.scale_dragged = None;
            }
            // Only the slider's label follows a drag, so the page doesn't
            // rescale under the pointer: the scale is set once let go.
            SettingsPage::DragScale(percent) => {
                self.page.scale_dragged =
                    varde_view::interface_scale_offered(percent).then_some(percent);
            }
            SettingsPage::ReleaseScale => {
                if let Some(percent) = self.page.scale_dragged.take() {
                    self.set(Setting::InterfaceScale(percent));
                }
            }
            SettingsPage::Set(setting) => {
                // A reset or a value chosen replaces what's typed.
                if matches!(setting, Setting::McpPort(_)) {
                    self.page.port_typed = None;
                }
                self.set(setting);
            }
            // Set once it's a port the field takes; kept as typed while
            // it isn't the port set, as an error out of range.
            SettingsPage::TypeMcpPort(typed) => {
                if let Some(port) = varde_view::mcp_port_of(&typed)
                    && port != self.options.mcp_port
                {
                    self.set(Setting::McpPort(port));
                }
                self.page.port_typed =
                    (typed != self.options.mcp_port.to_string()).then_some(typed);
            }
            SettingsPage::ShowMcpToken(shown) => self.page.token_shown = shown,
            SettingsPage::DisconnectMcp => self.mcp.disconnect(),
            // With nothing to reset, the button is off.
            SettingsPage::AskReset => {
                self.page.resetting = !varde_view::changed_settings(self.options).is_empty();
            }
            SettingsPage::CancelReset => self.page.resetting = false,
            SettingsPage::RegenerateMcpToken => self.mcp.regenerate_token(&mut self.files.io),
            SettingsPage::Reset => {
                // Only as asked: a stray Enter resets nothing.
                if std::mem::take(&mut self.page.resetting) {
                    self.page.port_typed = None;
                    self.options = ViewOptions::default();
                    self.options_changed();
                    self.files
                        .io
                        .send_all(self.settings.chose_all(self.options));
                }
            }
        }
    }

    /// What `Esc` does on the settings page, if it's open: cancels Reset
    /// to defaults while it asks, else closes the page.
    pub(crate) fn page_escape(&self) -> Option<PageEscape> {
        match (self.page.open, self.page.resetting) {
            (false, _) => None,
            (true, true) => Some(PageEscape::CancelReset),
            (true, false) => Some(PageEscape::Close),
        }
    }
}

/// Where the settings are kept, as the settings page's foot says it:
/// with the home directory as `~` once the recent `files` say where it
/// is. `None` without a config directory.
pub(crate) fn settings_shown(files: Option<&Files>) -> Option<String> {
    let store = varde_io::settings::store()?;
    Some(match files {
        Some(files) => files.shown_path(&store),
        None => store.display().to_string(),
    })
}
