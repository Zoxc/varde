//! The directories the IO worker keeps designs in, in the Origin Private
//! File System, and the downloads recorded at its root.

use std::io;
use std::path::{Path, PathBuf};

use web_sys::FileSystemDirectoryHandle;

use super::opfs::{self, OpfsDir};
use crate::browser::{self, LastDownload};
use crate::store::NO_STORE;
use crate::vrdp::Tail;
use crate::web_support::downloads::{self, Downloads};

/// The directories the worker keeps designs in, each found and made on
/// first use and kept from then on.
#[derive(Debug)]
pub(super) struct Dirs {
    /// Where new designs are kept, if anywhere, see [`Stores::designs`](crate::Stores::designs).
    pub(super) designs: Option<PathBuf>,
    /// The directory `designs` names, once found.
    pub(super) store: Option<OpfsDir>,
    /// Browser storage's directory, `saved`, once found.
    pub(super) browser: Option<OpfsDir>,
    /// The root of the Origin Private File System, where `downloads.toml`
    /// is, once found.
    pub(super) root: Option<OpfsDir>,
}

impl Dirs {
    /// The directory in `slot`, found by `find` and kept there if it's
    /// not yet.
    async fn cached(
        slot: &mut Option<OpfsDir>,
        find: impl Future<Output = io::Result<FileSystemDirectoryHandle>>,
    ) -> io::Result<OpfsDir> {
        if let Some(dir) = slot {
            return Ok(dir.clone());
        }
        let dir = OpfsDir(find.await?);
        *slot = Some(dir.clone());
        Ok(dir)
    }

    /// The store's directory, made if needed.
    pub(super) async fn store(&mut self) -> io::Result<OpfsDir> {
        let designs = self
            .designs
            .as_deref()
            .ok_or_else(|| io::Error::other(NO_STORE))?;
        Self::cached(&mut self.store, opfs::dir(designs)).await
    }

    /// Browser storage's directory, made if needed.
    pub(super) async fn browser(&mut self) -> io::Result<OpfsDir> {
        Self::cached(&mut self.browser, opfs::dir(Path::new(browser::DIR))).await
    }

    /// The root, where `downloads.toml` is.
    pub(super) async fn root(&mut self) -> io::Result<OpfsDir> {
        Self::cached(&mut self.root, opfs::root()).await
    }

    /// The last download recorded of the design `name`, whose file ends at
    /// `tail`.
    pub(super) async fn last_download(&mut self, name: &str, tail: Tail) -> Option<LastDownload> {
        self.downloads().await.last(name, tail.sum())
    }

    /// The downloads recorded, none if they can't be read, see
    /// [`downloads::load`].
    pub(super) async fn downloads(&mut self) -> Downloads {
        match self.root().await {
            Ok(root) => downloads::load(&root).await,
            Err(_) => Downloads::default(),
        }
    }

    /// Changes the downloads recorded as `change` does, see
    /// [`downloads::update`].
    pub(super) async fn update_downloads<T>(
        &mut self,
        change: impl FnOnce(&mut Downloads) -> T,
    ) -> io::Result<T> {
        downloads::update(&self.root().await?, change).await
    }
}
