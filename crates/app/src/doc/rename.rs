//! Renaming features, sketches and bodies: `F2` on what's selected, or
//! Rename in a row's context menu, opens the rename field in its row of
//! the side panel; `Enter` renames, `Esc` closes it, and anything else
//! done there but hovering, scrolling and moving the camera renames too.
//! A name another feature or body has gets a number after it
//! ([`Document::rename`](varde_document::Document::rename)), and a toast
//! says the name was taken.

use varde_document::Named;
use varde_view::Look;

use super::{Change, Doc};

/// The rename field open on a feature, sketch or body.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Renaming {
    pub(crate) target: Named,
    /// The name as typed.
    pub(crate) text: String,
}

impl Doc {
    /// What `F2` renames: the feature selected in the Timeline, else the
    /// one sketch selected in Objects with nothing else selected, else
    /// the one body what's selected in the model is of, with nothing
    /// selected in Objects.
    pub(crate) fn rename_target(&self) -> Option<Named> {
        if let Some(id) = self.selected_feature {
            return Some(Named::Feature(id));
        }
        let sketches: Vec<_> = self.selected_sketches().collect();
        let bodies = self.selected_bodies();
        match (self.objects_selected.len(), &sketches[..], &bodies[..]) {
            (1, [id], []) => Some(Named::Feature(*id)),
            (0, [], [body]) => Some(Named::Body(*body)),
            _ => None,
        }
    }

    /// Opens the rename field on `target`, holding its name, in a
    /// document that can be changed, renaming what it was open on first.
    pub(crate) fn start_rename(&mut self, target: Named) {
        if !self.editable() {
            return;
        }
        self.commit_rename();
        if let Some(name) = self.editor.document().name_of(target) {
            self.renaming = Some(Renaming {
                target,
                text: name.to_owned(),
            });
            self.rename_focus = true;
        }
    }

    /// The text in the rename field changed.
    pub(crate) fn rename_input(&mut self, text: String) {
        if let Some(renaming) = &mut self.renaming {
            renaming.text = text;
        }
    }

    /// Closes the rename field, renaming what it was open on to what was
    /// typed, if that's another name: one another has gets a number after
    /// it, and a toast says so. An empty name renames nothing.
    pub(crate) fn commit_rename(&mut self) {
        let Some(Renaming { target, text }) = self.renaming.take() else {
            return;
        };
        let Some(rename) = self.editor.document().rename(target, &text) else {
            return;
        };
        if let Some(taken) = rename.taken {
            self.toast = Some(format!("The name \"{taken}\" already exists"));
        }
        self.change(Change::Rename(rename.command));
    }

    /// Takes `message` if it's about the rename field, returning whether
    /// it was; otherwise renames first unless `message` only hovers,
    /// scrolls or moves the camera.
    pub(crate) fn rename_look(&mut self, message: &Look) -> bool {
        match message {
            Look::StartRename(target) => self.start_rename(*target),
            Look::RenameInput(text) => self.rename_input(text.clone()),
            Look::CancelRename => self.renaming = None,
            Look::Escape if self.renaming.is_some() => self.renaming = None,
            _ => {
                if self.renaming.is_some() && !passive(message) {
                    self.commit_rename();
                }
                return false;
            }
        }
        true
    }

    /// Whether the rename field is to take the focus, as it just opened:
    /// asked once.
    pub(crate) fn take_rename_focus(&mut self) -> bool {
        std::mem::take(&mut self.rename_focus)
    }

    /// What the document has to say as a toast, if anything: asked once.
    pub(crate) fn take_toast(&mut self) -> Option<String> {
        self.toast.take()
    }

    /// Closes the rename field if what it's open on is gone, as with an
    /// undo, or the document was `replaced`, whose ids may name others.
    pub(crate) fn prune_renaming(&mut self, replaced: bool) {
        let document = self.editor.document();
        self.renaming.take_if(|renaming| {
            replaced || document.name_of(renaming.target).is_none()
        });
    }
}

/// Whether `message` only hovers, scrolls or moves the camera, which
/// leaves the rename field open.
fn passive(message: &Look) -> bool {
    matches!(
        message,
        Look::HoverItem(_)
            | Look::HoverLink(_)
            | Look::HoverFeature(_)
            | Look::LeaveFeature(_)
            | Look::HoverOrigin(_)
            | Look::HoverPlane(_)
            | Look::LeaveOrigin(_)
            | Look::HoverBodyRow(_)
            | Look::LeaveBodyRow(_)
            | Look::HoverPanel(_)
            | Look::LeavePanel(_)
            | Look::Hover(_)
            | Look::HoverSketch(_)
            | Look::HoverCube(_)
            | Look::Snap(_)
            | Look::Aim(_)
            | Look::ScrollGeometry(_)
            | Look::ScrollConstraints(_)
            | Look::Orbit { .. }
            | Look::Pan { .. }
            | Look::Zoom { .. }
            | Look::SetPivot(_)
    )
}
