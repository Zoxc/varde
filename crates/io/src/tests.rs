//! Helpers shared by the crate's tests.

use std::path::{Path, PathBuf};

use varde_document::{Command, Document, Editor};

use crate::autosave::AutoSaved;
use crate::vrdp::{HeldFile, to_bytes};

/// A fresh directory for one test, deleted when dropped.
pub(crate) struct TempDir(pub(crate) PathBuf);

impl TempDir {
    pub(crate) fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("varde-io-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// A design in the directory, `doc.vrdp`.
    pub(crate) fn design(&self) -> PathBuf {
        let path = self.0.join("doc.vrdp");
        if !path.exists() {
            std::fs::write(&path, to_bytes(&Document::example()).unwrap().0).unwrap();
        }
        path
    }

    /// The sidecar of `doc.vrdp`.
    pub(crate) fn sidecar(&self) -> PathBuf {
        self.0.join(".doc.vrdp.autosave")
    }
}

/// The newest auto-save in the sidecar or store entry at `path`, read
/// without locking it; `None` if it's missing or empty.
pub(crate) fn auto_saved_at(path: &Path) -> Option<AutoSaved> {
    let file = std::fs::File::open(path).ok()?;
    HeldFile::<_, AutoSaved>::new(file).read().unwrap()
}

/// The example design with `bodies` of its bodies.
pub(crate) fn with_bodies(bodies: usize) -> Document {
    let mut editor = Editor::new(Document::example());
    let removed: Vec<_> = editor.document().bodies()[bodies..]
        .iter()
        .map(|body| body.id)
        .collect();
    for id in removed {
        editor.apply(Command::RemoveBody(id)).unwrap();
    }
    editor.document().clone()
}

/// The example design with a copy of its cube named `name`, to tell it
/// apart by.
pub(crate) fn with_cube_named(name: &str) -> Document {
    let mut editor = Editor::new(Document::example());
    let cube = editor.document().bodies()[0].clone();
    editor
        .apply(Command::AddBody {
            name: name.to_owned(),
            shape: cube.shape,
            position: cube.position,
        })
        .unwrap();
    editor.document().clone()
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
