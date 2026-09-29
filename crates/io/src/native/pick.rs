//! The platform's file dialogs, where the user picks files of theirs
//! natively.

use varde_document::EXTENSION;
use varde_document::name::download_name;

use crate::pick::{Download, filter};
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
/// [`download_name`]. `None` if the user backed out.
pub async fn pick_save(name: &str) -> Option<Chosen> {
    let file = rfd::AsyncFileDialog::new()
        .set_title("Save design as")
        .add_filter(filter(), &[EXTENSION])
        .set_file_name(download_name(name))
        .save_file()
        .await?;
    Some(Chosen::Path(file.path().to_owned()))
}

/// Natively saving writes to the path the design has: nothing is
/// downloaded.
pub fn downloader() -> Option<Download> {
    None
}

/// Natively nothing is picked as a [`Picked`], and a path needs no asking
/// to be written to.
pub async fn writable(_picked: &Picked) -> Result<(), String> {
    Ok(())
}
