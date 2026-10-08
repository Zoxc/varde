//! Deleting a feature or a body, and asking first when more goes with it,
//! or when a join, cut or intersect that stays worked only on bodies that
//! go: see [`Doc::remove`]. When features depend on it, the prompt offers
//! to delete it alone, keeping them to fail until they're given another
//! ([`Doc::confirm_delete`]).

use varde_document::{
    BodyId, Command, Document, FeatureId, Generation, Operation, Removable, Removal,
};
use varde_view::DeletePrompt;

use super::feed::Merges;
use super::{Change, Doc};

/// A removal the user is asked about before it's applied: what was asked
/// to be removed, everything that goes with it, and the generation of
/// the document that was worked out of.
pub(crate) struct Deleting {
    /// One or more, from Objects' selection.
    pub(crate) targets: Vec<Removable>,
    pub(crate) removal: Removal,
    generation: Generation,
}

impl Doc {
    /// Removes `target` and what depends on it, as one undo step: at once
    /// if no other feature goes with it (a feature's own bodies, or a
    /// body's own feature, go without asking, but not a pattern's other
    /// copy bodies with one of them) and no join, cut or
    /// intersect that stays worked only on bodies that go
    /// ([`Doc::worked`]),
    /// or else asks first, listing everything that would go and warning
    /// about those that may then fail. Nothing in a read-only
    /// document. While edits wait on the solver it waits behind them, see
    /// [`Doc::change`], and asks once it's made.
    pub(crate) fn remove(&mut self, target: Removable) {
        self.remove_all(vec![target]);
    }

    /// Removes all of `targets` together, as [`Doc::remove`] does one:
    /// one undo step, asking first if anything else goes with them.
    /// Nothing for none.
    pub(crate) fn remove_all(&mut self, targets: Vec<Removable>) {
        if targets.is_empty() {
            return;
        }
        self.change(Change::Remove {
            targets,
            confirmed: None,
            only: false,
        });
    }

    /// Removes what the delete prompt lists, and closes it: Delete. The
    /// command removes exactly that, as the document hasn't changed since
    /// (the prompt goes if it does, see [`Doc::prune_deleting`]). Asked by
    /// a delete that waited behind edits on the solver, it's made at once,
    /// in its turn, before what waits behind it. Otherwise, while edits
    /// wait on the solver, it waits behind them, and asks again if more
    /// would go by then. With `only`, Delete only, it removes just what
    /// was asked for, keeping the features that depend on it
    /// ([`Document::breaking_removal`]).
    pub(crate) fn confirm_delete(&mut self, only: bool) {
        let Some(deleting) = self.deleting.take() else {
            return;
        };
        if deleting.generation == self.editor.generation() {
            let removal = if only {
                self.editor.document().breaking_removal(&deleting.targets)
            } else {
                deleting.removal
            };
            let change = Change::Remove {
                targets: deleting.targets,
                confirmed: Some(removal),
                only,
            };
            if self.proposals.take_asking() {
                self.make(change);
            } else {
                self.change(change);
            }
        }
    }

    /// Whether the user is being asked about deleting.
    pub(crate) fn delete_asked(&self) -> bool {
        self.deleting
            .as_ref()
            .is_some_and(|deleting| deleting.generation == self.editor.generation())
    }

    /// Removes `target` now, as [`Doc::remove`] says, without asking if
    /// the user said yes to removing all that goes with it, `confirmed`;
    /// with `only`, removing just `targets` and the bodies they make, if
    /// that's still what `confirmed` is, keeping what depends on them.
    pub(super) fn remove_now(
        &mut self,
        targets: Vec<Removable>,
        confirmed: Option<Removal>,
        only: bool,
    ) {
        if !self.editable() || targets.is_empty() {
            return;
        }
        if only {
            let removal = self.editor.document().breaking_removal(&targets);
            if confirmed.as_ref() == Some(&removal) {
                self.apply(Command::RemoveOnly(removal.features));
                return;
            }
        }
        let removal = self.editor.document().removal_of(&targets);
        // A body goes quietly only with the feature making it alone: a
        // copy body takes its pattern, and the pattern's other copy
        // bodies, which the user didn't pick.
        let picked = |body: &BodyId| targets.contains(&Removable::Body(*body));
        let alone = (targets.iter()).all(|target| match target {
            Removable::Body(_) => removal.bodies.iter().all(picked),
            Removable::Feature(_) => true,
        });
        let quiet =
            removal.features.len() <= targets.len() && alone && self.worked(&removal).0.is_empty();
        if quiet || confirmed.as_ref() == Some(&removal) {
            let command = command(self.editor.document(), &targets);
            self.apply(command);
        } else {
            self.file_menu = false;
            self.view_menu = false;
            self.deleting = Some(Deleting {
                targets,
                removal,
                generation: self.editor.generation(),
            });
        }
    }

    /// Drops the delete prompt if the document changed under it
    /// (recovery, undo), so it never deletes a set it didn't show. No
    /// sketch edit commits while it's up, see [`Doc::send_proposal`].
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

    /// The joins, cuts and intersects `removal` leaves whose every body
    /// it touched it takes, as the model shown found them touch bodies,
    /// and those bodies, each in the document's order. With nothing left
    /// to touch they fail ("it doesn't touch any body"), so the delete
    /// prompt warns about them; one that also touched a body that stays
    /// goes on working on that. One the model shown doesn't know (added
    /// since, as a cut committed before its model comes back is, or any
    /// after the document was replaced) is taken to touch every body made
    /// before it that it doesn't take out: warned of if they all go, as
    /// then it surely has nothing to work on. A body that holds bodies
    /// earlier joins (or combines using their tools up) merged into it
    /// counts as those too: one of them staying takes its place once it
    /// goes.
    fn worked(&self, removal: &Removal) -> (Vec<FeatureId>, Vec<BodyId>) {
        let document = self.editor.document();
        let shown = self.feed.touched_features();
        let mut features = Vec::new();
        let mut worked_on = Vec::new();
        // The bodies merged into others by the joins and combines before.
        let mut merges = Merges::default();
        // The bodies made by the features before the one at hand.
        let mut made: Vec<BodyId> = Vec::new();
        for feature in document.features() {
            // A combine using its tools up merges them into its target.
            if let varde_document::FeatureKind::Combine(combine) = &feature.kind
                && removal.features.binary_search(&feature.id).is_err()
                && self.feed.consumes(document, feature.id)
            {
                merges.join(&combine.bodies().collect::<Vec<_>>());
            }
            if let Some(operation) = feature.kind.operation()
                && !matches!(operation, Operation::NewBody(_))
                && removal.features.binary_search(&feature.id).is_err()
            {
                // A checked document's ids go up in its order.
                let goes = |body: &BodyId| removal.bodies.binary_search(body).is_ok();
                let touched = match shown.iter().find(|(id, _)| *id == feature.id) {
                    Some((_, touched)) => {
                        let held = (touched.iter()).flat_map(|&holder| merges.held_by(holder));
                        let worked: Vec<BodyId> = touched.iter().copied().chain(held).collect();
                        if self.feed.merges(document, feature.id) {
                            merges.join(touched);
                        }
                        worked
                    }
                    None => {
                        let excluded = operation.excluded();
                        (made.iter())
                            .filter(|body| !excluded.contains(body))
                            .copied()
                            .collect()
                    }
                };
                if !touched.is_empty() && touched.iter().all(goes) {
                    features.push(feature.id);
                    worked_on.extend(touched);
                }
            }
            made.extend(
                (document.bodies().iter())
                    .filter(|body| body.created_by == feature.id)
                    .map(|body| body.id),
            );
        }
        worked_on.sort_unstable();
        // In the bodies' order, as the prompt lists them.
        let worked_on = (document.bodies().iter())
            .map(|body| body.id)
            .filter(|body| worked_on.binary_search(body).is_ok())
            .collect();
        (features, worked_on)
    }

    /// The delete prompt, if the user is being asked.
    pub(crate) fn delete_prompt(&self) -> Option<DeletePrompt<'_>> {
        let deleting = self.deleting.as_ref()?;
        let document = self.editor.document();
        if deleting.generation != self.editor.generation() {
            return None;
        }
        // Several are named by the first.
        let name = match *deleting.targets.first()? {
            Removable::Feature(id) => &document.feature(id)?.name,
            Removable::Body(id) => &document.body(id)?.name,
        };
        let (worked, worked_on) = self.worked(&deleting.removal);
        let only = document.breaking_removal(&deleting.targets);
        Some(DeletePrompt {
            name,
            body: matches!(deleting.targets[..], [Removable::Body(_), ..]),
            // In the Timeline's order, not the removal's, which is by id.
            features: (document.features().iter())
                .filter(|feature| deleting.removal.features.binary_search(&feature.id).is_ok())
                .collect(),
            bodies: (deleting.removal.bodies.iter())
                .filter_map(|&id| document.body(id))
                .collect(),
            worked: worked
                .iter()
                .filter_map(|&id| document.feature(id))
                .collect(),
            worked_on: worked_on
                .iter()
                .filter_map(|&id| document.body(id))
                .collect(),
            keeping: only.features.len() < deleting.removal.features.len(),
        })
    }
}

/// The command removing `targets`, which removes what
/// [`Document::removal_of`](varde_document::Document::removal_of) says:
/// several by the features making them.
fn command(document: &Document, targets: &[Removable]) -> Command {
    match *targets {
        [Removable::Feature(id)] => Command::RemoveFeature(id),
        [Removable::Body(id)] => Command::RemoveBody(id),
        _ => Command::RemoveFeatures(
            (targets.iter())
                .filter_map(|&target| match target {
                    Removable::Feature(id) => Some(id),
                    // Its feature goes with it, and only what that takes.
                    Removable::Body(id) => document.body(id).map(|body| body.created_by),
                })
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests;
