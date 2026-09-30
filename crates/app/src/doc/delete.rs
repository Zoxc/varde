//! Deleting a feature or a body, and asking first when more goes with it:
//! see [`Doc::remove`].

use varde_document::{Command, Generation, Removable, Removal};
use varde_view::DeletePrompt;

use super::Doc;

/// A removal the user is asked about before it's applied: what was asked
/// to be removed, everything that goes with it, and the generation of
/// the document that was worked out of.
pub(crate) struct Deleting {
    pub(crate) target: Removable,
    pub(crate) removal: Removal,
    generation: Generation,
}

impl Doc {
    /// Removes `target` and what depends on it, as one undo step: at once
    /// if no other feature goes with it (a feature's own bodies, or a
    /// body's own feature, go without asking), or else asks first,
    /// listing everything that would go. Nothing in a read-only
    /// document.
    pub(crate) fn remove(&mut self, target: Removable) {
        if !self.editable() {
            return;
        }
        let removal = self.editor.document().removal(target);
        if removal.features.len() <= 1 {
            self.apply(command(target));
        } else {
            self.file_menu = false;
            self.deleting = Some(Deleting {
                target,
                removal,
                generation: self.editor.generation(),
            });
        }
    }

    /// Removes what the delete prompt lists, and closes it: Delete. The
    /// command removes exactly that, as the document hasn't changed since
    /// (the prompt goes if it does, see [`Doc::prune_deleting`]).
    pub(crate) fn confirm_delete(&mut self) {
        let Some(deleting) = self.deleting.take() else {
            return;
        };
        if deleting.generation == self.editor.generation() {
            self.apply(command(deleting.target));
        }
    }

    /// Drops the delete prompt if the document changed under it (a
    /// proposal committing, recovery, undo), so it never deletes a set it
    /// didn't show.
    pub(crate) fn prune_deleting(&mut self) {
        let generation = self.editor.generation();
        if self
            .deleting
            .as_ref()
            .is_some_and(|deleting| deleting.generation != generation)
        {
            self.deleting = None;
        }
    }

    /// The delete prompt, if the user is being asked.
    pub(crate) fn delete_prompt(&self) -> Option<DeletePrompt<'_>> {
        let deleting = self.deleting.as_ref()?;
        let document = self.editor.document();
        if deleting.generation != self.editor.generation() {
            return None;
        }
        let name = match deleting.target {
            Removable::Feature(id) => &document.feature(id)?.name,
            Removable::Body(id) => &document.body(id)?.name,
        };
        Some(DeletePrompt {
            name,
            features: (deleting.removal.features.iter())
                .filter_map(|&id| document.feature(id))
                .collect(),
            bodies: (deleting.removal.bodies.iter())
                .filter_map(|&id| document.body(id))
                .collect(),
        })
    }
}

/// The command removing `target`, which removes what
/// [`Document::removal`](varde_document::Document::removal) says.
fn command(target: Removable) -> Command {
    match target {
        Removable::Feature(id) => Command::RemoveFeature(id),
        Removable::Body(id) => Command::RemoveBody(id),
    }
}

#[cfg(test)]
mod tests;
