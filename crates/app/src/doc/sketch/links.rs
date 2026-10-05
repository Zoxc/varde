//! The links of the sketch being edited in its Sketch tab: their rows
//! (what each comes from by name, whether it's broken and why, whether
//! its curves count for profiles), selecting what a link made from its
//! row, lighting what it comes from in the model while its row is
//! hovered, and its row's menu: removing it, or having its curves count
//! for profiles or not.

use std::sync::Arc;

use varde_document::{Document, FeatureKind, OutsideRef, PointRef};
use varde_sketch::{Id, SketchEdit};
use varde_view::{LinkRow, ModelHighlight, Picked};

use super::super::Doc;
use super::outside::outside_target;

impl Doc {
    /// Lists the links of the sketch being edited for its Sketch tab, as
    /// committed, with why each is broken as the model shown found, and
    /// lights what the link whose row is hovered comes from on the model
    /// shown. Its hover goes with a link that's gone.
    pub(crate) fn refresh_links(&mut self) {
        let Some(feature) = self.sketch.as_ref().map(|session| session.feature) else {
            return;
        };
        let document = self.editor.document();
        let rows = match self.edited_links() {
            Some((sketch, sources)) => (sketch.links.iter())
                .zip(sources)
                .map(|(link, from)| LinkRow {
                    link: link.id,
                    kind: link.kind,
                    source: source_name(document, &from.source),
                    broken: self.feed.broken(feature, link.id).map(capitalized),
                    profiles: link.profiles,
                })
                .collect(),
            None => Vec::new(),
        };
        if let Some(session) = &mut self.sketch {
            session
                .link_hover
                .take_if(|hovered| !rows.iter().any(|row| row.link == *hovered));
            session.links = rows;
        }
        self.pick.link_highlight = self.link_highlight();
    }

    /// Selects what the link `link` made: its points and curves.
    pub(crate) fn click_link(&mut self, link: Id) {
        let Some(made) = self
            .shown_sketch()
            .and_then(|sketch| sketch.link(link))
            .cloned()
        else {
            return;
        };
        if let Some(session) = &mut self.sketch {
            session.selection = made.items().collect();
        }
    }

    /// Notes the link whose row is hovered, lighting what it comes from.
    pub(crate) fn hover_link(&mut self, link: Option<Id>) {
        if let Some(session) = &mut self.sketch {
            session.link_hover = link.filter(|&id| session.links.iter().any(|row| row.link == id));
        }
        self.pick.link_highlight = self.link_highlight();
    }

    /// Deletes the link `link`, with what it made.
    pub(crate) fn remove_link(&mut self, link: Id) {
        self.propose(SketchEdit::Delete(vec![link]));
    }

    /// Has the link `link`'s curves count for profiles, or not.
    pub(crate) fn set_link_profiles(&mut self, link: Id, profiles: bool) {
        self.propose(SketchEdit::SetLinkProfiles { link, profiles });
    }

    /// What the link whose row is hovered comes from, lit on the model
    /// shown, if it's a face, an edge or a corner found there.
    fn link_highlight(&self) -> Arc<ModelHighlight> {
        let Some(session) = &self.sketch else {
            return Arc::default();
        };
        let Some(hovered) = session.link_hover else {
            return Arc::default();
        };
        let source = (self.edited_links())
            .and_then(|(_, sources)| sources.iter().find(|from| from.link == hovered))
            .map(|from| from.source);
        let found: Vec<Picked> = (source.iter())
            .filter_map(|source| outside_target(&self.feed, source))
            .collect();
        if found.is_empty() {
            return Arc::default();
        }
        Arc::new(self.feed.pick_index().highlight(&[], &found))
    }

    /// The model highlight of what the link whose row is hovered comes
    /// from, if there's one to draw.
    pub(crate) fn hovered_link_highlight(&self) -> Option<&Arc<ModelHighlight>> {
        self.sketch.as_ref()?.link_hover?;
        Some(&self.pick.link_highlight).filter(|highlight| !highlight.is_empty())
    }
}

/// `why` with its first letter capitalized, as a row's note.
fn capitalized(why: &str) -> String {
    let mut chars = why.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// What `source` is, by name: "Line 3 of Sketch 2", "Edge of Body 1",
/// "Face of Body 1", "Corner of Body 1"; what's gone said so.
pub(crate) fn source_name(document: &Document, source: &OutsideRef) -> String {
    let body = |id| {
        (document.body(id)).map_or_else(|| "a removed body".to_owned(), |body| body.name.clone())
    };
    match *source {
        OutsideRef::Sketch { sketch, item } => {
            let Some(feature) = document.feature(sketch) else {
                return "Geometry of a removed sketch".to_owned();
            };
            let name = match &feature.kind {
                FeatureKind::Sketch { sketch, .. } => sketch.name(item),
                _ => None,
            };
            match name {
                Some(name) => format!("{name} of {}", feature.name),
                None => format!("Removed geometry of {}", feature.name),
            }
        }
        OutsideRef::Edge(edge) => format!("Edge of {}", body(edge.body)),
        OutsideRef::Face(face) => format!("Face of {}", body(face.body)),
        OutsideRef::Corner(PointRef::Corner { body: id, .. }) => format!("Corner of {}", body(id)),
        OutsideRef::Corner(_) => "A point".to_owned(),
    }
}

#[cfg(test)]
mod tests;
