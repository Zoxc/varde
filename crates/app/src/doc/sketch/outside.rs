//! The Project and Intersect tools of the sketch being edited, picking
//! outside it (`Tool::picks_outside`): the cursor picks the model (its
//! vertices, edges and faces) and the curves and points of the other
//! visible sketches, the sketch's own geometry not hit. Each accepted
//! click, named as a feature at the sketch's place names it
//! (`Naming::before`), proposes a new link of the tool's kind from it
//! ([`Doc::propose_link`]), committed with its source as one undoable
//! change, its geometry found by the next regeneration and folded into
//! that change (see `relink`); a click on what a link of the kind
//! already comes from proposes deleting that link. A click on something
//! the tool can't use says why in the status bar (`Doc::notice`).
//! Project takes edges, faces (their outlines), corners and other
//! sketches' curves and points; Intersect faces and edges. Only what's made before the sketch is
//! taken.

use std::borrow::Cow;
use std::sync::Arc;

use varde_document::{OutsideRef, PointRef};
use varde_sketch::{Id, LinkKind, SketchEdit};
use varde_view::{ModelHighlight, Pick, Picked, SketchItem, SketchLines, Tool, Unnamed};

use super::super::Doc;
use super::super::OUT_OF_DATE;
use super::super::feed::MeshFeed;
use super::super::pick::sketch_holds;

/// What a click of Project or Intersect is on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum OutsideClick {
    /// The model, at `pick`, or nothing.
    Model(Option<Pick>),
    /// Another sketch's curve or point.
    Sketch(SketchItem),
}

impl Doc {
    /// The tool in use in the sketch being edited, if it picks outside
    /// the sketch ([`Tool::picks_outside`]).
    pub(crate) fn outside_tool(&self) -> Option<Tool> {
        let tool = self.sketch.as_ref()?.tool.as_ref()?.tool;
        tool.picks_outside().then_some(tool)
    }

    /// Whether the cursor picks outside the sketch being edited, for its
    /// Project or Intersect tool.
    pub(crate) fn picks_outside(&self) -> bool {
        self.outside_tool().is_some()
    }

    /// The links of the sketch being edited of the kind the tool in use
    /// makes, with what each comes from, in the links' order: as it's
    /// worked on, with the edits waiting on the solver applied, so a
    /// second click on what a link waiting to be added comes from takes
    /// it back, as edits of the other tools go on from those waiting.
    fn outside_links(&self) -> Vec<(Id, OutsideRef)> {
        let (Some(kind), Some((_, committed)), Some(working)) = (
            self.outside_tool().map(link_kind),
            self.edited_links(),
            self.working_sketch(),
        ) else {
            return Vec::new();
        };
        let waiting = (self.sketch.as_ref())
            .and_then(|session| session.waiting.as_ref())
            .map_or(&[][..], |waiting| &waiting.sources[..]);
        let source = |link: Id| {
            (committed.iter())
                .map(|from| (from.link, from.source))
                .chain(waiting.iter().copied())
                .find(|&(id, _)| id == link)
                .map(|(_, source)| source)
        };
        (working.links.iter())
            .filter(|link| link.kind == kind)
            .filter_map(|link| Some((link.id, source(link.id)?)))
            .collect()
    }

    /// What the links of the tool in use come from.
    fn outside_picks(&self) -> Vec<OutsideRef> {
        (self.outside_links().into_iter())
            .map(|(_, source)| source)
            .collect()
    }

    /// The other sketches whose curves and points the tool picking
    /// outside picks, where they're placed: the visible ones but the
    /// sketch being edited, later ones too, which a click is refused on.
    pub(crate) fn outside_sketches(&self) -> Vec<SketchLines<'_>> {
        let Some(edited) = self.sketch.as_ref().map(|session| session.feature) else {
            return Vec::new();
        };
        self.placed_sketches(|feature| feature.visible && feature.id != edited)
    }

    /// The sketches' items the tool picking outside has picked, drawn as
    /// selected.
    pub(crate) fn outside_marked(&self) -> Vec<SketchItem> {
        (self.outside_picks().iter())
            .filter_map(sketch_item)
            .collect()
    }

    /// Takes a click of the tool picking outside on `click`: picks what
    /// it's on, as a feature at the sketch's place names it, or takes it
    /// out if it's picked; or says why it can't be picked.
    pub(crate) fn outside_click(&mut self, click: OutsideClick) {
        let Some(tool) = self.outside_tool() else {
            return;
        };
        let picked = match click {
            OutsideClick::Model(None) => return,
            OutsideClick::Model(Some(pick)) => self.outside_model(tool, pick),
            OutsideClick::Sketch(item) => self.outside_sketch_item(tool, item),
        };
        let found = match picked {
            Ok(found) => found,
            Err(why) => {
                self.notice = Some(why.into_owned());
                return;
            }
        };
        // The model's by what it finds on the model shown, as another
        // click on the same edge names it elsewhere along it.
        let same = |other: &OutsideRef| match click {
            OutsideClick::Model(Some(pick)) => {
                outside_target(&self.feed, other) == Some(pick.target)
            }
            _ => *other == found,
        };
        let linked = (self.outside_links().into_iter()).find(|(_, source)| same(source));
        match linked {
            Some((link, _)) if self.sketch_face() == Some(link) => {
                self.notice = Some(super::links::SKETCH_FACE_STAYS.to_owned());
                return;
            }
            Some((link, _)) => {
                self.propose(SketchEdit::Delete(vec![link]));
            }
            None => {
                self.propose_link(link_kind(tool), found);
            }
        }
        self.refresh_highlight();
    }

    /// The name of the sketch being edited and where it is in the
    /// document's features (their count if it isn't there).
    fn sketch_place(&self) -> (&str, usize) {
        let document = self.editor.document();
        let edited = self.sketch.as_ref().map(|session| session.feature);
        let at = edited.and_then(|id| document.feature_index(id));
        let name = at.map_or("", |at| document.features()[at].name.as_str());
        (name, at.unwrap_or(document.insert_at()))
    }

    /// Whether the tool picking outside has picked what `pick` is on, of
    /// the model shown.
    pub(crate) fn outside_has(&self, pick: Pick) -> bool {
        pick.model == self.feed.model()
            && (self.outside_picks().iter())
                .any(|picked| outside_target(&self.feed, picked) == Some(pick.target))
    }

    /// What `tool` makes of a click on the model at `pick`.
    fn outside_model(&self, tool: Tool, pick: Pick) -> Result<OutsideRef, Cow<'static, str>> {
        if pick.model != self.feed.model() || self.feed.predates_replacement() {
            return Err(OUT_OF_DATE.into());
        }
        let (name, _) = self.sketch_place();
        let naming = self.naming_at(self.sketch.as_ref().map(|session| session.feature));
        let index = self.feed.pick_index();
        let refused = |why: Unnamed| -> Cow<'static, str> {
            match why {
                Unnamed::Missing => "That isn't in the model shown".into(),
                Unnamed::Later => later(tool, name).into(),
                Unnamed::Unclear => {
                    format!("Which body that's on at {name} can't be told: pick another").into()
                }
            }
        };
        match (tool, pick.target) {
            (_, Picked::Edge(edge)) => {
                let edge = naming.edge_ref(index, edge, pick.at).map_err(refused)?;
                Ok(OutsideRef::Edge(edge))
            }
            (_, Picked::Face(face)) => {
                let face = (naming.checked_face_ref(index, face, pick.at)).map_err(refused)?;
                Ok(OutsideRef::Face(face))
            }
            (Tool::Project, Picked::Vertex(vertex)) => {
                let corner =
                    (index.vertex_corner(vertex)).ok_or("That corner isn't in the model shown")?;
                let corner = naming.corner_ref(index, corner).map_err(refused)?;
                Ok(OutsideRef::Corner(corner))
            }
            _ => Err(INTERSECT_TAKES.into()),
        }
    }

    /// What `tool` makes of a click on another sketch's curve or point
    /// `item`.
    fn outside_sketch_item(
        &self,
        tool: Tool,
        item: SketchItem,
    ) -> Result<OutsideRef, Cow<'static, str>> {
        if tool != Tool::Project {
            return Err(INTERSECT_TAKES.into());
        }
        let document = self.editor.document();
        if !sketch_holds(document, item) {
            return Err("That isn't in the sketch any more".into());
        }
        let (name, before) = self.sketch_place();
        if document
            .feature_index(item.sketch)
            .is_none_or(|at| at >= before)
        {
            return Err(later(tool, name).into());
        }
        Ok(OutsideRef::Sketch {
            sketch: item.sketch,
            item: item.item,
        })
    }

    /// What's drawn over the model while the tool picking outside is in
    /// use: what it has picked of the model as selected, and what's
    /// hovered if the tool takes its kind.
    pub(crate) fn outside_highlight(&self) -> Arc<ModelHighlight> {
        let Some(tool) = self.outside_tool() else {
            return Arc::default();
        };
        let index = self.feed.pick_index();
        let selected: Vec<Picked> = (self.outside_picks().iter())
            .filter_map(|picked| outside_target(&self.feed, picked))
            .collect();
        let hovered: Vec<Picked> = (self.pick.hover())
            .filter(|pick| pick.model == index.model() && takes_kind(tool, pick.target))
            .map(|pick| pick.target)
            .into_iter()
            .collect();
        Arc::new(index.highlight(&hovered, &selected))
    }
}

/// The kind of link `tool`, Project or Intersect, makes.
pub(crate) fn link_kind(tool: Tool) -> LinkKind {
    if tool == Tool::Intersect {
        LinkKind::Intersect
    } else {
        LinkKind::Project
    }
}

/// What Intersect takes, said when it's clicked on something else.
const INTERSECT_TAKES: &str = "Intersect takes faces and edges, cut with the sketch's plane";

/// Why `tool` refuses what's made at or after the sketch `name`.
fn later(tool: Tool, name: &str) -> String {
    let done = if tool == Tool::Intersect {
        "intersected"
    } else {
        "projected"
    };
    format!("Only what's made before {name} can be {done}")
}

/// Whether `tool` takes the model's `target`'s kind: Project faces,
/// edges and vertices, Intersect faces and edges.
fn takes_kind(tool: Tool, target: Picked) -> bool {
    matches!(
        (tool, target),
        (_, Picked::Edge(_) | Picked::Face(_)) | (Tool::Project, Picked::Vertex(_))
    )
}

/// The other sketch's curve or point `picked` is, if it's one.
pub(super) fn sketch_item(picked: &OutsideRef) -> Option<SketchItem> {
    match *picked {
        OutsideRef::Sketch { sketch, item } => Some(SketchItem { sketch, item }),
        _ => None,
    }
}

/// The model's face, edge or vertex `picked` names on `feed`'s model
/// shown, on the body drawing its body there
/// ([`MeshFeed::shown_body`]), if it's there.
pub(super) fn outside_target(feed: &MeshFeed, picked: &OutsideRef) -> Option<Picked> {
    let index = feed.pick_index();
    match *picked {
        OutsideRef::Sketch { .. } => None,
        OutsideRef::Edge(edge) => {
            let found = index.find_edge(feed.shown_body(edge.body), edge.faces, edge.near)?;
            Some(Picked::Edge(found))
        }
        OutsideRef::Face(face) => {
            let found = index.find_face(feed.shown_body(face.body), &face.key, face.near)?;
            Some(Picked::Face(found))
        }
        OutsideRef::Corner(PointRef::Corner { body, faces, near }) => Some(Picked::Vertex(
            index.find_vertex(feed.shown_body(body), faces, near)?,
        )),
        OutsideRef::Corner(_) => None,
    }
}
