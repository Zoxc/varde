//! The platform's file dialogs, where the user picks files of theirs
//! natively.

use varde_document::EXTENSION;
use varde_document::name::{download_name, download_name_with};

use crate::pick::{Download, export_filter, filter};
use crate::three_mf;
use crate::{Chosen, Picked};

/// Shows the Open dialog. `None` if the user backed out.
pub async fn pick_open() -> Option<Chosen> {
    let file = rfd::AsyncFileDialog::new()
        .set_title("Open design")
        .add_filter(filter(), &[EXTENSION])
        .pick_file()
        .await?;
    Some(Chosen::Path(file.path().to_owned()))
}

/// Shows the Save As dialog for the design `name`, suggesting
/// [`download_name`], or nothing for a design with no name (`name` empty).
/// `None` if the user backed out.
pub async fn pick_save(name: &str) -> Option<Chosen> {
    let dialog = rfd::AsyncFileDialog::new()
        .set_title("Save design as")
        .add_filter(filter(), &[EXTENSION]);
    let dialog = if name.is_empty() {
        dialog
    } else {
        dialog.set_file_name(download_name(name))
    };
    let file = dialog.save_file().await?;
    Some(Chosen::Path(file.path().to_owned()))
}

/// Shows the Export dialog for the design `name`, suggesting
/// `name.3mf`, or nothing for a design with no name (`name` empty). `None`
/// if the user backed out.
pub async fn pick_export(name: &str) -> Option<Chosen> {
    let dialog = rfd::AsyncFileDialog::new()
        .set_title("Export 3MF")
        .add_filter(export_filter(), &[three_mf::EXTENSION]);
    let dialog = if name.is_empty() {
        dialog
    } else {
        dialog.set_file_name(download_name_with(name, three_mf::EXTENSION))
    };
    let file = dialog.save_file().await?;
    Some(Chosen::Path(file.path().to_owned()))
}

/// Natively saving writes to the path the design has: nothing is
/// downloaded.
pub fn downloader() -> Option<Download> {
    None
}

/// Natively the platform's dialogs pick files, by path.
pub fn file_system_access() -> bool {
    false
}

/// Natively nothing is picked as a [`Picked`], and a path needs no asking
/// to be written to.
pub async fn writable(_picked: &Picked) -> Result<(), String> {
    Ok(())
}

/// Natively nothing is picked as a [`Picked`], so there's nothing to let
/// go of.
pub fn forget(_picked: &Picked) {}
