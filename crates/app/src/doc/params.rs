//! The design's parameters' popup: whether it's open, and what's typed in
//! its fields and not yet in the document.
//!
//! What's typed in a name or an expression is a draft until it's
//! committed: `Enter` in its field, typing in another field, or anything
//! else done but hovering, scrolling and moving the camera (as the rename
//! field commits). Each commit is one command, so one undo step, and
//! regeneration follows as for any edit. One the document refuses stays
//! in its field with why, changing nothing, until it's typed again or
//! `Esc` puts the field back. The drafts are of the list as it was when
//! they were typed: an undo, a redo or a replaced document that changes
//! the list drops them.

use varde_document::{
    CheckError, Command, Document, EditError, Generation, Param, ParamError, ParamUses, ValueOf,
};
use varde_view::{Look, ParamDraft, ParamEdit, ParamField, ParamsLook, ParamsState};

use super::{Change, Doc};

/// The text a new parameter starts with.
pub(crate) const NEW_PARAM_TEXT: &str = "10 mm";

/// The popup, see the module's docs.
#[derive(Debug, Clone, Default)]
pub(crate) struct Params {
    /// Whether it's open: only its tool and its Close open and close it.
    pub(crate) open: bool,
    drafts: Vec<ParamDraft>,
    /// The document's parameters as `drafts` and `refused` were made
    /// against.
    basis: Vec<Param>,
    /// The field typed in last, while its draft hasn't been committed.
    typing: Option<(usize, ParamField)>,
    /// A parameter whose removal was refused, and why.
    refused: Option<(usize, String)>,
    /// The parameter whose name is to take the focus, as it was just
    /// added.
    focus: Option<usize>,
    /// What uses each parameter ([`Document::all_param_uses`]) while
    /// it's open, for the document's generation it was found for.
    uses: Option<(Generation, Vec<ParamUses>)>,
}

impl Params {
    /// Whether the drafts are of `document`'s list as it is.
    fn current(&self, document: &Document) -> bool {
        self.basis == document.params()
    }

    /// Drops the drafts and what was refused if the list changed since
    /// they were made, and makes them of the list as it is.
    fn rebase(&mut self, document: &Document) {
        if !self.current(document) {
            self.drafts.clear();
            self.typing = None;
            self.refused = None;
            self.basis = document.params().to_vec();
        }
    }

    fn draft(&self, index: usize, field: ParamField) -> Option<&ParamDraft> {
        (self.drafts.iter()).find(|draft| draft.index == index && draft.field == field)
    }

    fn drop_draft(&mut self, index: usize, field: ParamField) {
        (self.drafts).retain(|draft| !(draft.index == index && draft.field == field));
        self.typing.take_if(|typing| *typing == (index, field));
    }
}

impl Doc {
    /// Opens the popup, or closes it, committing what's typed there.
    pub(crate) fn toggle_params(&mut self) {
        self.commit_param_typing();
        self.rail.close();
        self.params.open = !self.params.open;
        if !self.params.open {
            self.params.drafts.clear();
            self.params.typing = None;
            self.params.refused = None;
        }
        self.refresh_param_uses();
    }

    /// Finds what uses each parameter again if the popup is open and the
    /// document changed since: once a change, not each frame. Let go of
    /// while it's closed.
    pub(crate) fn refresh_param_uses(&mut self) {
        if !self.params.open {
            self.params.uses = None;
            return;
        }
        let generation = self.editor.generation();
        if self
            .params
            .uses
            .as_ref()
            .is_none_or(|(at, _)| *at != generation)
        {
            let uses = self.editor.document().all_param_uses();
            self.params.uses = Some((generation, uses));
        }
    }

    /// Takes `message`, typing in the popup.
    pub(crate) fn params_look(&mut self, message: ParamsLook) {
        self.params.rebase(self.editor.document());
        self.params.refused = None;
        match message {
            ParamsLook::Input { index, field, text } => {
                // Typing in another field leaves the one before.
                if self
                    .params
                    .typing
                    .is_some_and(|typing| typing != (index, field))
                {
                    self.commit_param_typing();
                    self.params.rebase(self.editor.document());
                }
                if index >= self.editor.document().params().len() {
                    return;
                }
                self.params.drop_draft(index, field);
                self.params.drafts.push(ParamDraft {
                    index,
                    field,
                    text,
                    error: None,
                });
                self.params.typing = Some((index, field));
            }
            ParamsLook::Revert { index, field } => self.params.drop_draft(index, field),
        }
    }

    /// Takes `message`, changing the design's parameters.
    pub(crate) fn params_edit(&mut self, message: ParamEdit) {
        self.params.rebase(self.editor.document());
        self.params.refused = None;
        match message {
            ParamEdit::Commit { index, field } => self.commit_param(index, field),
            ParamEdit::Add => {
                self.commit_param_typing();
                let name = self.editor.document().new_param_name("param");
                self.params.open = true;
                self.refresh_param_uses();
                self.make_param(Command::AddParam {
                    name,
                    text: NEW_PARAM_TEXT.to_owned(),
                });
            }
            ParamEdit::Remove(index) => {
                self.commit_param_typing();
                let document = self.editor.document();
                let Some(param) = document.params().get(index) else {
                    return;
                };
                let uses = uses(document, &param.name);
                if !uses.is_empty() {
                    let why = format!("Used by {}: change those first", uses.join(", "));
                    self.params.refused = Some((index, why));
                    return;
                }
                self.make_param(Command::RemoveParam(index));
            }
        }
    }

    /// Commits the draft of the field typed in last, if there is one: as
    /// it's left.
    pub(crate) fn commit_param_typing(&mut self) {
        if let Some((index, field)) = self.params.typing.take() {
            self.commit_param(index, field);
        }
    }

    /// Applies the draft of `index`'s `field`, if it has one: dropped if
    /// it's as the document has it or once the document takes it, else
    /// kept with why (see [`Doc::param_answered`]); kept while it waits
    /// behind the edits on the solver.
    fn commit_param(&mut self, index: usize, field: ParamField) {
        self.params.rebase(self.editor.document());
        self.params
            .typing
            .take_if(|typing| *typing == (index, field));
        let Some(draft) = self.params.draft(index, field) else {
            return;
        };
        let Some(param) = self.editor.document().params().get(index) else {
            return;
        };
        let text = draft.text.trim();
        let command = match field {
            ParamField::Name if text == param.name => None,
            ParamField::Expression if text == param.text => None,
            ParamField::Name => Some(Command::RenameParam {
                index,
                name: text.to_owned(),
            }),
            ParamField::Expression => Some(Command::SetParam {
                index,
                text: text.to_owned(),
            }),
        };
        let Some(command) = command else {
            self.params.drop_draft(index, field);
            return;
        };
        self.make_param(command);
    }

    /// Makes `command`, or has it wait behind the edits waiting on the
    /// solver, as other changes do: the popup follows once it's made
    /// ([`Doc::param_answered`]).
    fn make_param(&mut self, command: Command) {
        if !self.editable() {
            let why = "The design can't be changed".to_owned();
            self.param_answered(&command, Err(why));
            return;
        }
        if self.proposing() {
            self.change(Change::Param(command));
            return;
        }
        self.make_param_now(command);
    }

    /// Makes `command` now, on the document as it is, then has the popup
    /// follow: a refusal, worded for the user, is said there in place of
    /// the status bar, if the popup's drafts are of the list as it was.
    pub(crate) fn make_param_now(&mut self, command: Command) {
        let current = self.params.current(self.editor.document());
        self.apply(command.clone());
        if !current {
            return;
        }
        let outcome = match self.edit_error.take() {
            None => Ok(()),
            Some(error) => Err(refusal(self.editor.document(), &error)),
        };
        self.param_answered(&command, outcome);
    }

    /// Has the popup follow `command`, made or refused with why: a draft
    /// it came from is dropped once made, or kept with why; a row removed
    /// takes its drafts, and those after it move up with their rows; an
    /// added parameter's name takes the focus. Only a draft whose text is
    /// still the command's is answered, as one typed while it waited is
    /// newer.
    fn param_answered(&mut self, command: &Command, outcome: Result<(), String>) {
        let made = outcome.is_ok();
        match (command, outcome) {
            (Command::AddParam { .. }, Ok(())) => {
                let added = self.editor.document().params().len().checked_sub(1);
                self.params.focus = added;
            }
            (Command::AddParam { .. }, Err(why)) => self.toast = Some(why),
            (&Command::RemoveParam(index), Ok(())) => {
                self.params.drafts.retain(|draft| draft.index != index);
                for draft in &mut self.params.drafts {
                    if draft.index > index {
                        draft.index -= 1;
                    }
                }
                self.params.typing = None;
            }
            (&Command::RemoveParam(index), Err(why)) => self.params.refused = Some((index, why)),
            (
                &Command::RenameParam {
                    index,
                    name: ref text,
                }
                | &Command::SetParam { index, ref text },
                outcome,
            ) => {
                let field = match command {
                    Command::RenameParam { .. } => ParamField::Name,
                    _ => ParamField::Expression,
                };
                let Some(draft) = (self.params.drafts.iter_mut())
                    .find(|draft| draft.index == index && draft.field == field)
                    .filter(|draft| draft.text.trim() == text)
                else {
                    return self.rebase_after(made);
                };
                match outcome {
                    Ok(()) => self.params.drop_draft(index, field),
                    Err(why) => draft.error = Some(why),
                }
            }
            _ => {}
        }
        self.rebase_after(made);
    }

    /// Makes the drafts of the list as it is, once a command of the
    /// popup's was made on the list they were of.
    fn rebase_after(&mut self, made: bool) {
        if made {
            self.params.basis = self.editor.document().params().to_vec();
        }
    }

    /// The design's parameters as typed fields offer their names.
    pub(crate) fn params_in(&self) -> varde_view::ParamsIn<'_> {
        let document = self.editor.document();
        varde_view::ParamsIn {
            params: document.params_resolved(),
            units: document.units(),
        }
    }

    /// The popup as the view shows it, if it's open.
    pub(crate) fn params_state(&self) -> Option<ParamsState<'_>> {
        let document = self.editor.document();
        let current = self.params.current(document);
        self.params.open.then(|| ParamsState {
            document,
            drafts: if current { &self.params.drafts } else { &[] },
            refused: (self.params.refused.as_ref())
                .filter(|_| current)
                .map(|(index, why)| (*index, why.as_str())),
            uses: (self.params.uses.as_ref())
                .filter(|(at, _)| *at == self.editor.generation())
                .map_or(&[], |(_, uses)| uses.as_slice()),
            selected: self.selected_feature,
            editable: self.editable(),
        })
    }

    /// The parameter whose name is to take the focus, as it was just
    /// added: asked once.
    pub(crate) fn take_param_focus(&mut self) -> Option<usize> {
        self.params.focus.take()
    }

    /// Takes `message` if it's about the popup, returning whether it was;
    /// otherwise commits what's typed there first unless `message` only
    /// hovers, scrolls or moves the camera.
    pub(crate) fn params_message(&mut self, message: &Look) -> bool {
        match message {
            Look::ToggleParams => self.toggle_params(),
            Look::Params(message) => self.params_look(message.clone()),
            _ => {
                if self.params.typing.is_some() && !super::rename::passive(message) {
                    self.commit_param_typing();
                    self.sync();
                }
                return false;
            }
        }
        true
    }
}

/// What uses the parameter `name` in `document`, by name: features, then
/// other parameters.
fn uses(document: &Document, name: &str) -> Vec<String> {
    let features = (document.param_uses(name).into_iter())
        .filter_map(|id| document.feature(id))
        .map(|feature| feature.name.clone());
    let params = (document.param_users(name).into_iter())
        .filter_map(|index| document.params().get(index))
        .map(|param| param.name.clone());
    features.chain(params).collect()
}

/// Why `document` refused a parameter's command, as the popup says it.
fn refusal(document: &Document, error: &EditError) -> String {
    match error {
        EditError::Value(ValueOf::Feature(id), why) => {
            let name = document
                .feature(*id)
                .map_or("A feature", |f| f.name.as_str());
            format!("{name} would be in error: {why}")
        }
        EditError::Value(ValueOf::Param(index), why) => {
            let name = (document.params().get(*index)).map_or("A parameter", |p| p.name.as_str());
            format!("{name} would be in error: {why}")
        }
        EditError::Invalid(CheckError::Param(_, ParamError::Duplicate)) => {
            "Another parameter has this name".to_owned()
        }
        EditError::Invalid(CheckError::Param(_, ParamError::Name(why))) => why.to_string(),
        EditError::ParamUsed(name) => format!("\"{name}\" is in use"),
        EditError::Unsolved(id, why) => {
            let name = document
                .feature(*id)
                .map_or("A sketch", |f| f.name.as_str());
            format!("{name} wouldn't solve: {why}")
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests;
