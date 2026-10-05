//! Sketches' links following what they come from: what regenerating
//! found of them folded into the document.
//!
//! A regeneration finds each link's source where its sketch is in the
//! history, and answers each sketch whose links found other geometry
//! than they hold, relinked and solved (`varde_regen`'s `link.rs`). Taken
//! only from the answer of the editor's generation as it is (anything
//! newer would be relinked from a document that's gone; the next answer
//! is of the new one), and only for a document that can be changed, it's
//! folded into the change it follows from ([`Editor::amend`]): moving a
//! source and the links following are one undo step, undo takes both
//! back (the sketch then holds what it held before, which was in step
//! with the model before), redo gives both again, and following never
//! makes a step of its own, so it never fights undo. The next answer,
//! of the document relinked, finds the links in step, or follows on (a
//! link of a link). A link whose shape changed keeps what ids it can
//! (`Sketch::relink`); the constraints and dimensions on what went, gone
//! with it, are said in the status bar. A relinked sketch the document refuses is left as it
//! was, logged.
//!
//! A sketch face added as the document was read (`varde_document::
//! sketch_face`) makes nothing until it's relinked: filling only such
//! empty links in a document as it was opened or saved keeps it so
//! (`Doc::keep_clean`), neither unsaved nor auto-saved for it, as the file
//! read again gives the same.
//!
//! [`Editor::amend`]: varde_document::Editor::amend

use std::sync::Arc;

use varde_document::{Command, FeatureKind, Sketch};
use varde_sketch::Id;

use super::Doc;

impl Doc {
    /// Folds the sketches the model shown relinked into the change they
    /// follow from, see the module's docs.
    pub(crate) fn relink(&mut self) {
        let relinked = self.feed.take_relinked(self.editor.generation());
        if relinked.is_empty() || !self.editable() {
            return;
        }
        let mut lost = Vec::new();
        let clean = (!self.edited()).then(|| self.editor.revision());
        let mut only_filled = true;
        for (feature, sketch) in relinked {
            let document = self.editor.document();
            let Some((name, before)) =
                (document.feature(feature)).and_then(|held| match &held.kind {
                    FeatureKind::Sketch { sketch, .. } => Some((held.name.clone(), sketch.clone())),
                    _ => None,
                })
            else {
                continue;
            };
            let gone = went_with(&before, &sketch);
            let stale = stale_links(&before, &sketch);
            only_filled &= (before.links.iter())
                .filter(|link| stale.contains(&link.id))
                .all(|link| link.items().next().is_none());
            let command = Command::SetSketch {
                feature,
                sketch: Box::new(Arc::unwrap_or_clone(sketch)),
            };
            match self.editor.amend(command) {
                Ok(()) if gone != (0, 0) => lost.push((name, gone)),
                Ok(()) => {}
                Err(error) => {
                    let why = format!("{REFUSED}: {error}");
                    for link in stale {
                        self.feed.mark_broken(feature, link, why.clone());
                    }
                }
            }
        }
        if let Some(clean) = clean.filter(|_| only_filled) {
            self.keep_clean(clean);
        }
        if let Some(said) = (lost.iter())
            .map(|(name, (constraints, dimensions))| lost_note(name, *constraints, *dimensions))
            .reduce(|a, b| format!("{a}; {b}"))
        {
            self.notice = Some(said);
        }
        self.sync();
    }
}

/// What a link whose relinked sketch the document refused is broken
/// with, before the refusal.
pub(crate) const REFUSED: &str = "what it found was refused";

/// The links of `before` that `after`, the sketch relinked, changed, or
/// all of them if it changed none: those a refusal of it leaves stale.
fn stale_links(before: &Sketch, after: &Sketch) -> Vec<Id> {
    let changed: Vec<Id> = (before.links.iter())
        .filter(|link| {
            let now = after.link(link.id);
            now.is_none_or(|now| after.link_shape(now) != before.link_shape(link))
        })
        .map(|link| link.id)
        .collect();
    if changed.is_empty() {
        before.links.iter().map(|link| link.id).collect()
    } else {
        changed
    }
}

/// How many of `before`'s constraints and dimensions `after` hasn't:
/// those on a link's items that went as it changed shape.
fn went_with(before: &Sketch, after: &Sketch) -> (usize, usize) {
    let constraints = (before.constraints.iter())
        .filter(|entry| after.constraint(entry.id).is_none())
        .count();
    let dimensions = (before.dimensions.iter())
        .filter(|entry| after.dimension(entry.id).is_none())
        .count();
    (constraints, dimensions)
}

/// What the status bar says of the sketch `name`'s links taking
/// `constraints` constraints and `dimensions` dimensions with what of
/// theirs went: "Sketch 2's links changed shape, removing 1 constraint
/// on what they no longer make".
fn lost_note(name: &str, constraints: usize, dimensions: usize) -> String {
    let count = |n: usize, one: &str, many: &str| match n {
        0 => None,
        1 => Some(format!("1 {one}")),
        n => Some(format!("{n} {many}")),
    };
    let parts: Vec<String> = [
        count(constraints, "constraint", "constraints"),
        count(dimensions, "dimension", "dimensions"),
    ]
    .into_iter()
    .flatten()
    .collect();
    format!(
        "{name}'s links changed shape, removing {} on what they no longer make",
        parts.join(" and ")
    )
}

#[cfg(test)]
mod tests;
