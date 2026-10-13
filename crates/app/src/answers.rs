//! The IO lane's answers, as the app takes them.

use iced::{Task, window};
use varde_document::Revision;
use varde_io::{Chosen, Response as IoResponse, SaveError};

use crate::Varde;
use crate::files::ThumbnailHandles;
use crate::message::Message;
use crate::page::settings_shown;

impl Varde {
    pub(crate) fn io_response(&mut self, response: IoResponse) -> Task<Message> {
        match response {
            IoResponse::Opened { id, path, result } => {
                return self.welcome(|welcome, files| welcome.opened(files, id, path, result));
            }
            // One not the open document's was given up on, and the lane
            // closes it, see `Io::abandon`.
            IoResponse::Created { id, result } => {
                if let Some(doc) = self.screen.doc_mut() {
                    doc.created(&mut self.files, id, &result);
                }
            }
            IoResponse::AutoSaved { file, result, .. } => {
                self.with_doc(|doc, _| doc.auto_saved(file, result))
            }
            IoResponse::RecoveryDiscarded { result, .. } => {
                report_failure("discard what was auto-saved", result);
            }
            IoResponse::RecoveredListed { designs } => self.files.recovered = designs,
            IoResponse::BrowserListed { designs } => self.files.browser_listed(designs),
            IoResponse::DeletedFromBrowser { name, result } => {
                self.files.list_browser();
                if let Err(error) = result {
                    return self.welcome(|welcome, _| {
                        welcome.failed(&format!("Couldn't delete {name}: {error}"))
                    });
                }
            }
            IoResponse::DownloadedFromBrowser {
                name,
                result,
                not_recorded,
            } => {
                let error = match result {
                    Ok(bytes) => self.files.download(&name, &bytes, not_recorded),
                    Err(error) => Some(format!("Couldn't download {name}: {error}")),
                };
                if let Some(error) = error {
                    return self.welcome(|welcome, _| welcome.failed(&error));
                }
            }
            IoResponse::Renamed { file, name, result } => {
                // Another name taken, or another freed.
                self.files.list_browser();
                self.with_doc(|doc, _| doc.renamed(file, name, result));
            }
            IoResponse::DownloadRecorded { file, result } => {
                self.with_doc(|doc, _| doc.download_recorded(file, result));
            }
            IoResponse::RecoveredDiscarded { result, .. } => {
                // The lane lists what's left after it either way.
                if let Err(error) = result {
                    return self.welcome(|welcome, _| welcome.discard_failed(&error));
                }
            }
            IoResponse::Closed { result, .. } | IoResponse::Abandoned { result, .. } => {
                report_failure("close a file", result);
            }
            IoResponse::RecentLoaded { entries, home } => {
                self.files
                    .io
                    .send_all(self.files.recent.loaded(entries, home));
                // The home directory as `~`, now it's known.
                self.settings_shown = settings_shown(Some(&self.files));
                self.files.load_thumbnails();
            }
            IoResponse::ThumbnailsLoaded { thumbnails } => {
                self.files.thumbnails = (thumbnails.into_iter())
                    .map(|(path, thumbnail)| (path, ThumbnailHandles::new(thumbnail)))
                    .collect();
            }
            IoResponse::RecentWritten { result } => {
                report_failure("save the recent files", result);
            }
            IoResponse::SettingsLoaded { settings } => {
                self.files
                    .io
                    .send_all(self.settings.loaded(settings, &mut self.options));
                self.options_changed();
            }
            IoResponse::SettingsWritten { result } => {
                report_failure("save the settings", result);
            }
            IoResponse::PanicLoaded { panic } => self.files.panic = panic,
            #[cfg(not(target_arch = "wasm32"))]
            IoResponse::PngEncoded { owner, tag, result } => {
                if let Some(doc) = self.screen.doc_mut()
                    && doc.id.number() == owner
                {
                    doc.mcp_encoded(tag, result);
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            IoResponse::McpToken {
                regenerated,
                result,
            } => match result {
                Ok(token) => {
                    let (on, port) = (self.options.mcp, self.options.mcp_port);
                    let doc = self.screen.doc_mut();
                    self.mcp.token_loaded(token, regenerated, on, port, doc);
                }
                Err(error) => {
                    let what = if regenerated {
                        "make a new MCP token"
                    } else {
                        "read the MCP token"
                    };
                    report_failure(what, Err(error));
                }
            },
            IoResponse::PanicDiscarded { result } => {
                report_failure("discard the internal error", result);
            }
            IoResponse::Saved {
                file,
                revision,
                result,
            } => {
                return self.step(|doc, cx| doc.saved_to(cx, file, revision, result));
            }
            IoResponse::SavedAs {
                to,
                revision,
                result,
                ..
            } => {
                // A name taken, or replaced: what the Save As dialog asks
                // about replacing follows.
                if matches!(to, Chosen::Browser(_)) {
                    self.files.list_browser();
                }
                return self.saved_as(to, revision, result);
            }
            IoResponse::Exported { to, result } => match self.screen.doc_mut() {
                Some(doc) => doc.export_written(&to, result),
                // Closed while it was written: the file is written or not
                // all the same.
                None => report_failure("export", result),
            },
            IoResponse::Flushed => {
                if let Some(window) = self.quitting {
                    return window::close(window);
                }
            }
        }
        Task::none()
    }

    /// Takes the answer to a Save As to the file the user `chose`, see
    /// [`Doc::saved_as`](crate::doc::Doc::saved_as). Leaving waits for saves, so the document that
    /// asked is the one open, if any.
    fn saved_as(
        &mut self,
        chose: Chosen,
        revision: Revision,
        result: Result<varde_io::SavedAs, SaveError>,
    ) -> Task<Message> {
        if self.screen.doc().is_none() {
            self.files.settle_saved_as(result);
            return Task::none();
        }
        self.step(|doc, cx| doc.saved_as(cx, chose, revision, result))
    }
}

/// Reports `result` if it failed to do `what`, for the IO lane's answers
/// that are only reported, not shown.
fn report_failure(what: &str, result: Result<(), String>) {
    if let Err(error) = result {
        log::error!("Couldn't {what}: {error}");
    }
}
