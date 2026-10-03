//! Exporting the visible bodies as a 3MF file, from the File menu's Export
//! 3MF…: where to, asked first as a save, then the bodies welded by the
//! regeneration lane, then written by the IO lane, or downloaded on the
//! web without the File System Access API. Each step is something the user
//! did or a lane answered, like saving's (see `save.rs`).

use varde_document::name::{download_name_with, with_extension_of};
use varde_io::three_mf::{self, Body};
use varde_io::{Chosen, Request as IoRequest, SaveTo};
use varde_regen::ExportedBody;
use varde_view::MeshStatus;

use super::Doc;
use crate::{Files, Next};

/// Where an export of the document stands, see [`Doc::request_export`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Exporting {
    /// The Export dialog is showing, answered with [`Doc::export_picked`].
    Picking,
    /// The regeneration lane is welding the bodies, answered with
    /// [`Doc::export_welded`] for the request tagged `export`: to be
    /// written where `to` says, or downloaded without one.
    Welding { export: u64, to: Option<SaveTo> },
    /// The IO lane is writing them, answered with [`Doc::export_written`].
    Writing,
}

/// The part of [`Doc`] exporting keeps to itself.
#[derive(Debug, Default)]
pub(crate) struct Export {
    /// The export on its way, if one is: one at a time.
    exporting: Option<Exporting>,
    /// Why the last export failed, until dismissed or another starts.
    error: Option<String>,
}

impl Doc {
    /// Whether the File menu's Export 3MF… goes: the model shown has a
    /// body the document shows, its regeneration hasn't failed, and no
    /// export is on its way. A read-only design can be exported too.
    pub(crate) fn exportable(&self) -> bool {
        self.export.exporting.is_none()
            && !matches!(self.feed.status(&self.editor), MeshStatus::Failed(_))
            && self.feed.shows_a_body(self.editor.document())
    }

    /// Whether an export is being welded or written, which the status bar
    /// says: not while the user is still choosing where to.
    pub(crate) fn exporting(&self) -> bool {
        matches!(
            self.export.exporting,
            Some(Exporting::Welding { .. } | Exporting::Writing)
        )
    }

    /// Where the export on its way stands, if one is.
    #[cfg(test)]
    pub(crate) fn export_state(&self) -> Option<&Exporting> {
        self.export.exporting.as_ref()
    }

    /// Why the last export failed, if it did and isn't dismissed.
    pub(crate) fn export_error(&self) -> Option<&str> {
        self.export.error.as_deref()
    }

    pub(super) fn dismiss_export_error(&mut self) {
        self.export.error = None;
    }

    /// Starts exporting the visible bodies, if they can be (see
    /// [`Doc::exportable`]): asks where to, suggesting the design's name
    /// if it has one,
    /// answered with [`Doc::export_picked`]. On the web without the File
    /// System Access API there's nothing to ask: the bodies are welded at
    /// once and downloaded.
    pub(crate) fn request_export(&mut self, cx: &mut Files) -> Next {
        self.file_menu = false;
        if !self.exportable() {
            return Next::Stay;
        }
        if cx.downloader.is_some() && !cx.file_system_access {
            self.weld(None);
            return Next::Stay;
        }
        self.export.exporting = Some(Exporting::Picking);
        Next::PickExport {
            id: self.id,
            name: self.suggested_name(),
        }
    }

    /// The Export dialog closed, with the file the user `chose` if they
    /// did: the bodies of the document as it is now are welded for it.
    /// Backing out does nothing.
    pub(crate) fn export_picked(&mut self, chosen: Option<Chosen>) {
        if self.export.exporting != Some(Exporting::Picking) {
            return;
        }
        self.export.exporting = None;
        let to = match chosen {
            None => return,
            // The dialog only asked about replacing the file the user
            // named, not one with the extension added.
            Some(Chosen::Path(path)) => {
                let (path, overwrite) = with_extension_of(path, three_mf::EXTENSION);
                SaveTo::Path { path, overwrite }
            }
            Some(Chosen::File(picked)) => SaveTo::Picked(picked),
            // Exports are never kept in browser storage.
            Some(Chosen::Browser(_)) => return,
        };
        self.weld(Some(to));
    }

    /// Asks the regeneration lane to weld the committed document's
    /// visible bodies, to be written where `to` says or downloaded.
    fn weld(&mut self, to: Option<SaveTo>) {
        self.export.error = None;
        match self.feed.request_export(&self.editor) {
            Some(export) => self.export.exporting = Some(Exporting::Welding { export, to }),
            None => self.export.error = Some("the model isn't ready yet: try again".to_owned()),
        }
    }

    /// The regeneration lane's answer to the export tagged `export`: the
    /// bodies welded are handed to the IO lane to write, or downloaded as
    /// `name.3mf`, encoded on the page as a design's download is.
    pub(crate) fn export_welded(
        &mut self,
        cx: &mut Files,
        export: u64,
        result: Result<Vec<ExportedBody>, String>,
    ) {
        let to = match self.export.exporting.take() {
            Some(Exporting::Welding { export: asked, to }) if asked == export => to,
            other => {
                self.export.exporting = other;
                return;
            }
        };
        let bodies: Vec<Body> = match result {
            Ok(bodies) => (bodies.into_iter())
                .map(|body| Body {
                    name: body.name,
                    mesh: body.mesh,
                })
                .collect(),
            Err(error) => {
                self.export.error = Some(error);
                return;
            }
        };
        // Hidden since, or failed: nothing to ask the file system for.
        if bodies.is_empty() {
            self.export.error = Some(three_mf::Error::NoObjects.to_string());
            return;
        }
        let written = match (to, &cx.downloader) {
            (Some(to), _) => {
                cx.io.send(IoRequest::Export {
                    to,
                    title: self.name.clone(),
                    bodies,
                });
                self.export.exporting = Some(Exporting::Writing);
                Ok(())
            }
            (None, Some(download)) => three_mf::package(&self.name, &bodies)
                .map_err(|e| e.to_string())
                .and_then(|bytes| {
                    download(&download_name_with(&self.name, three_mf::EXTENSION), &bytes)
                }),
            (None, None) => Err("there's nowhere to export to".to_owned()),
        };
        if let Err(error) = written {
            self.export.error = Some(error);
        }
    }

    /// The regeneration lane was replaced: an export the old one was
    /// welding won't be answered, so it fails rather than waiting for
    /// good. Not asked again: the document may have changed since.
    pub(crate) fn regen_replaced(&mut self) {
        if let Some(Exporting::Welding { .. }) = self.export.exporting {
            self.export.exporting = None;
            self.export.error =
                Some("regenerating restarted before the bodies were welded: try again".to_owned());
        }
    }

    /// The IO lane's answer to the export it was writing.
    pub(crate) fn export_written(&mut self, result: Result<(), String>) {
        if self.export.exporting != Some(Exporting::Writing) {
            return;
        }
        self.export.exporting = None;
        if let Err(error) = result {
            self.export.error = Some(error);
        }
    }
}
