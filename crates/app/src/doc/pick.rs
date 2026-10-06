//! Picking the model shown with the cursor, outside sketches and
//! sessions: what's hovered, what's selected, and the highlight drawn for
//! them. The pick index is the feed's, built the first time it's needed
//! for a model (see [`MeshFeed::pick_index`](super::feed::MeshFeed::pick_index)).
//! The selection keeps faces and edges by name and finds them again in
//! each new model (see [`Selection`]); Objects shows the bodies selected
//! and selects them too. Outside the sessions the curves and points of
//! the sketches shown are hovered and selected with the model
//! ([`Doc::selectable_sketches`]).

use std::sync::Arc;

use varde_document::{BodyId, Document, FeatureId, FeatureKind};
use varde_view::{
    AlignRole, ModelHighlight, ModelPicking, MotionKind, MotionPick, ObjectRow, PanelHover, Pick,
    Picked, Picks, Selection, SketchItem, SketchLines,
};

use super::{Doc, MotionSession};

/// What's hovered and selected in the model shown, and its highlight.
#[derive(Debug, Default)]
pub(crate) struct ModelPick {
    /// Of the model [`Pick::model`] names.
    hover: Option<Pick>,
    /// The curve or point of a sketch shown hovered, in place of `hover`.
    sketch_hover: Option<SketchItem>,
    pub(crate) selection: Selection,
    /// Built when what it's of changes, so the renderer uploads it only
    /// then.
    highlight: Arc<ModelHighlight>,
    /// The body whose row in Objects is hovered, if one is: its faces
    /// are lit hovered with the selection, in place of the cursor's
    /// hover.
    pub(crate) row_hover: Option<BodyId>,
    /// What `highlight` was built of: the model, the target hovered, the
    /// selection and the body row hovered.
    built: Option<(u64, Option<Picked>, Selection, Option<BodyId>)>,
    /// While an extrude or revolve is set up, the body whose row in its
    /// panel is hovered, lit in the model shown (its preview).
    panel_highlight: Arc<ModelHighlight>,
    /// What `panel_highlight` was built of: the model and the body.
    panel_built: Option<(u64, BodyId)>,
    /// While Project or Intersect picks outside the sketch being edited,
    /// what it has picked of the model and what's hovered, built for the
    /// model shown ([`Doc::outside_highlight`]).
    outside_highlight: Arc<ModelHighlight>,
    /// What the link whose row in the Sketch tab is hovered comes from,
    /// lit on the model shown ([`Doc::refresh_links`]).
    pub(crate) link_highlight: Arc<ModelHighlight>,
}

impl ModelPick {
    /// The face, edge or vertex hovered, if any.
    pub(crate) fn hover(&self) -> Option<Pick> {
        self.hover
    }

    /// The curve or point of a sketch hovered, if any.
    pub(crate) fn sketch_hover(&self) -> Option<SketchItem> {
        self.sketch_hover
    }
}

impl Doc {
    /// Whether the cursor picks the model: in a sketch only for its
    /// Project or Intersect tool ([`Doc::picks_outside`]), else outside
    /// sketches and the extrude or revolve being set up, which pick what
    /// they need themselves, and not while a draft's preview is still
    /// shown after it, where what's selected would be looked for in a
    /// model that isn't the document's, nor while the model shown is of a
    /// document since replaced whole, whose bodies' ids may name others
    /// now. While a combine is set up it picks bodies, its preview's too:
    /// a combine's draft makes no body, so the bodies its model has are
    /// the document's.
    pub(crate) fn picks(&self) -> bool {
        let combining = self.combine.is_some() || self.motion.is_some();
        (self.picks_outside()
            || (self.sketch.is_none()
                && (combining || (!self.operating() && !self.feed.shows_draft()))))
            && !self.feed.predates_replacement()
    }

    /// The row of the operation's panel the cursor is over, if one is set
    /// up and the row is still there: a region still picked, a combine's
    /// body still named.
    pub(crate) fn panel_hover(&self) -> Option<PanelHover> {
        let region = |regions: &super::regions::RegionPick, hover: PanelHover| match hover {
            PanelHover::Region { sketch, region } => {
                regions.source == Some(sketch) && regions.picked.contains(&region)
            }
            _ => true,
        };
        if let Some(session) = &self.extrude {
            return session
                .hover
                .filter(|&hover| region(&session.regions, hover));
        }
        if let Some(session) = &self.revolve {
            return session
                .hover
                .filter(|&hover| region(&session.regions, hover));
        }
        if let Some(session) = &self.motion {
            return session.hover.filter(|&hover| match hover {
                PanelHover::Body(body) => session.bodies.contains(&body),
                PanelHover::Axis => true,
                PanelHover::Edge(at) => at < session.blend.edges.refs.len(),
                PanelHover::Face(at) => at < session.faces.refs.len(),
                PanelHover::Part(at) if session.kind == MotionKind::Loft => {
                    at < session.loft.rails.len()
                }
                PanelHover::Part(at) => at < session.sweep.chains.len(),
                PanelHover::Section(at) => {
                    session.kind == MotionKind::Loft && at < session.loft.sections.len()
                }
                PanelHover::Region { .. } => {
                    session.kind == MotionKind::Sweep && region(&session.sweep.regions, hover)
                }
            });
        }
        let session = self.combine.as_ref()?;
        session.hover.filter(|&hover| match hover {
            PanelHover::Body(body) => session.target == Some(body) || session.tools.contains(&body),
            _ => false,
        })
    }

    /// Hovers the row `hover` of the operation's panel, or none: the
    /// viewport lights it up too.
    pub(crate) fn hover_panel(&mut self, hover: Option<PanelHover>) {
        if let Some(session) = &mut self.extrude {
            session.hover = hover;
        }
        if let Some(session) = &mut self.revolve {
            session.hover = hover;
        }
        if let Some(session) = &mut self.combine {
            session.hover = hover;
        }
        if let Some(session) = &mut self.motion {
            session.hover = hover;
        }
        self.refresh_highlight();
    }

    /// Takes the cursor leaving the panel's row `left`: nothing's hovered,
    /// unless another row already is, entered before this one was left.
    pub(crate) fn leave_panel(&mut self, left: PanelHover) {
        let hovered = (self.extrude.as_ref().map(|session| session.hover))
            .or_else(|| self.revolve.as_ref().map(|session| session.hover))
            .or_else(|| self.combine.as_ref().map(|session| session.hover))
            .or_else(|| self.motion.as_ref().map(|session| session.hover))
            .flatten();
        if hovered == Some(left) {
            self.hover_panel(None);
        }
    }

    /// Hovers `pick`, from the viewport as the cursor moves, or nothing:
    /// only the highlight changes. A pick of another model than the one
    /// shown, or while the cursor doesn't pick, is dropped.
    pub(crate) fn hover(&mut self, pick: Option<Pick>) {
        let pick = pick.filter(|pick| pick.model == self.feed.model() && self.picks());
        self.pick.hover = pick;
        self.pick.sketch_hover = None;
        self.refresh_highlight();
    }

    /// Hovers `item`, a sketch's curve or point, from the viewport as the
    /// cursor moves, or nothing: nothing of the model is then. One the
    /// cursor doesn't pick ([`Doc::selectable_sketch_item`]) is dropped.
    pub(crate) fn hover_sketch(&mut self, item: Option<SketchItem>) {
        self.pick.hover = None;
        self.pick.sketch_hover = item.filter(|&item| self.selectable_sketch_item(item));
        self.refresh_highlight();
    }

    /// Takes a click on a sketch's curve or point `item` in the model,
    /// see [`Selection::click_sketch`], letting go of the feature
    /// selected in the Timeline as a click on the model does. One the
    /// cursor doesn't pick is ignored.
    pub(crate) fn click_sketch(&mut self, item: SketchItem, add: bool) {
        if !self.selectable_sketch_item(item) || self.sketch.is_some() {
            return;
        }
        self.pick.selection.click_sketch(item, add);
        if !add || !self.pick.selection.is_empty() {
            self.selected_feature = None;
        }
        self.refresh_highlight();
    }

    /// Whether the curves and points of the sketches shown are picked
    /// with the model: while the cursor picks it outside the sessions,
    /// and not picking a plane.
    fn picks_sketch_items(&self) -> bool {
        self.picks()
            && self.sketch.is_none()
            && !self.operating()
            && self.measure.is_none()
            && self.combine.is_none()
            && self.motion.is_none()
            && self.picking_plane.is_none()
            && self.pick.selection.mode().takes_sketch_items()
    }

    /// The sketches whose curves and points the cursor picks with the
    /// model, where they're placed: the visible ones, while it picks
    /// them ([`Doc::picks_sketch_items`]).
    pub(crate) fn selectable_sketches(&self) -> Vec<SketchLines<'_>> {
        if !self.picks_sketch_items() {
            return Vec::new();
        }
        self.placed_sketches(|feature| feature.visible)
    }

    /// Whether `item` is a curve or point the cursor picks with the model
    /// now.
    fn selectable_sketch_item(&self, item: SketchItem) -> bool {
        let edited = self.sketch.as_ref().map(|session| session.feature);
        (self.picks_sketch_items() || self.picks_outside())
            && Some(item.sketch) != edited
            && shown_sketch_item(self.editor.document(), item)
    }

    /// Takes a click on the model, on `pick` or on nothing, see
    /// [`Selection::click`]. Selecting in the model lets go of the
    /// feature selected in the Timeline.
    pub(crate) fn click_model(&mut self, pick: Option<Pick>, add: bool, double: bool) {
        // Picking a plane, a click on a flat face asks for a sketch on it
        // instead (`Edit::FacePicked`), and one elsewhere does nothing.
        // In a sketch, the tool picking outside it has the click.
        if !self.picks()
            || self.sketch.is_some()
            || self.picking_plane.is_some()
            || pick.is_some_and(|pick| pick.model != self.feed.model())
        {
            return;
        }
        let index = self.feed.pick_index();
        self.pick.selection.click(index, pick, add, double);
        if !add || !self.pick.selection.is_empty() {
            self.selected_feature = None;
        }
        self.refresh_highlight();
    }

    /// Takes a click on `body`'s row in Objects, see
    /// [`Selection::click_body`], letting go of the feature selected in
    /// the Timeline. Not in a sketch, where Objects' rows don't select.
    /// A body a join merged into another is drawn as that one, so
    /// selects it.
    pub(crate) fn click_body(&mut self, body: BodyId, add: bool) {
        if self.sketch.is_some() || self.editor.document().body(body).is_none() {
            return;
        }
        if !add {
            self.objects_selected.clear();
        }
        let body = self.feed.shown_body(body);
        self.pick.selection.click_body(body, add);
        self.selected_feature = None;
        self.refresh_highlight();
    }

    /// Takes a click on `row`, an origin object's or a sketch's in
    /// Objects: selects it alone, the model's selection and the
    /// Timeline's let go, or with `add` adds it or takes it out. Not in a
    /// sketch.
    pub(crate) fn click_object(&mut self, row: ObjectRow, add: bool) {
        if self.sketch.is_some() {
            return;
        }
        if let ObjectRow::Sketch(id) = row
            && self.editor.document().feature(id).is_none()
        {
            return;
        }
        if add {
            match self
                .objects_selected
                .iter()
                .position(|&selected| selected == row)
            {
                Some(at) => {
                    self.objects_selected.remove(at);
                }
                None => self.objects_selected.push(row),
            }
        } else {
            self.objects_selected = vec![row];
            self.selected_feature = None;
            self.pick.selection.clear();
        }
        self.refresh_highlight();
    }

    /// The curves selected in the model, if curves of one sketch are all
    /// that's selected: the sketch and the curves, in the order they
    /// were.
    pub(crate) fn selected_curves(&self) -> Option<(FeatureId, Vec<varde_sketch::Id>)> {
        let document = self.editor.document();
        let mut items = self.pick.selection.sketch_items().peekable();
        let sketch = items.peek()?.sketch;
        let Some(FeatureKind::Sketch { sketch: drawn, .. }) =
            document.feature(sketch).map(|feature| &feature.kind)
        else {
            return None;
        };
        let curves: Option<Vec<varde_sketch::Id>> = items
            .map(|item| {
                (item.sketch == sketch && drawn.curve(item.item).is_some()).then_some(item.item)
            })
            .collect();
        let curves = curves?;
        (curves.len() == self.pick.selection.items().count()).then_some((sketch, curves))
    }

    /// The sketches selected in Objects, in the order they were.
    pub(crate) fn selected_sketches(&self) -> impl Iterator<Item = FeatureId> + '_ {
        (self.objects_selected.iter()).filter_map(|row| match row {
            ObjectRow::Sketch(id) => Some(*id),
            ObjectRow::Origin(_) => None,
        })
    }

    /// The origin plane selected in Objects, if it's the one origin
    /// object selected there.
    pub(crate) fn selected_origin_plane(&self) -> Option<varde_document::OriginPlane> {
        let mut origins = (self.objects_selected.iter()).filter_map(|row| match row {
            ObjectRow::Origin(object) => Some(*object),
            ObjectRow::Sketch(_) => None,
        });
        match (origins.next(), origins.next()) {
            (Some(varde_view::OriginObject::Plane(plane)), None) => Some(plane),
            _ => None,
        }
    }

    /// Drops the sketches selected in Objects the document no longer
    /// has, or all of them if it was `replaced` whole, when their ids may
    /// name others.
    pub(crate) fn prune_objects(&mut self, replaced: bool) {
        let document = self.editor.document();
        self.objects_selected.retain(|row| match row {
            ObjectRow::Sketch(id) => !replaced && document.feature(*id).is_some(),
            ObjectRow::Origin(_) => true,
        });
    }

    /// Selects nothing in the model.
    pub(crate) fn clear_model_selection(&mut self) {
        self.objects_selected.clear();
        self.pick.selection.clear();
        self.refresh_highlight();
    }

    /// Forgets what's hovered and selected in the model: the document was
    /// replaced whole, and the bodies they name by id may be others now.
    pub(crate) fn forget_picks(&mut self) {
        self.pick.hover = None;
        self.pick.selection.clear();
        self.refresh_highlight();
    }

    /// Drops what's hovered once the model it's of isn't shown, or the
    /// cursor doesn't pick, finds what's selected again in a new model
    /// while the cursor picks (what isn't there, bodies the document
    /// doesn't hold or a join merged into another, stops being selected
    /// but is looked for in later models; a face or edge of a merged body
    /// in the body holding it), and rebuilds the highlight if what it's
    /// of changed. The list of the model's overlaps is found again on a
    /// new model ([`Doc::follow_overlaps`]).
    pub(crate) fn prune_picks(&mut self) {
        self.follow_overlaps();
        let stale = |pick: Pick| pick.model != self.feed.model() || !self.picks();
        if self.pick.hover.is_some_and(stale) {
            self.pick.hover = None;
        }
        if (self.pick.sketch_hover).is_some_and(|item| !self.selectable_sketch_item(item)) {
            self.pick.sketch_hover = None;
        }
        // A sketch's item selected goes with its sketch, or once hidden.
        let document = self.editor.document();
        (self.pick.selection).retain_sketch_items(|item| shown_sketch_item(document, item));
        // Not in a combine's preview, which isn't the document's model.
        if self.picks()
            && self.sketch.is_none()
            && self.combine.is_none()
            && self.motion.is_none()
            && !self.pick.selection.holds_nothing()
        {
            let document = self.editor.document();
            let drawn = |body| document.body(body).map(|_| self.feed.shown_body(body));
            self.pick.selection.resolve(self.feed.pick_index(), drawn);
        }
        self.refresh_highlight();
    }

    /// Rebuilds the highlight if the model, the target hovered or the
    /// selection changed since it was built. Nothing is drawn while the
    /// cursor doesn't pick, so it isn't built then.
    pub(crate) fn refresh_highlight(&mut self) {
        self.refresh_panel_highlight();
        if !self.picks() {
            return;
        }
        // The tool picking outside the sketch has its own.
        if self.picks_outside() {
            self.pick.outside_highlight = self.outside_highlight();
            return;
        }
        // The measure tool's own, leaving the selection's as it was, and
        // likewise the combine's.
        if self.measure.is_some() {
            self.refresh_measure_highlight();
            return;
        }
        if self.combine.is_some() {
            self.refresh_combine_highlight();
            return;
        }
        if self.motion.is_some() {
            self.refresh_motion_highlight();
            return;
        }
        let hover = self.shown_hover();
        let row = self.pick.row_hover;
        let key = (
            self.feed.model(),
            hover.map(|pick| pick.target),
            self.pick.selection.clone(),
            row,
        );
        if self.pick.built.as_ref() == Some(&key) {
            return;
        }
        let highlight = if let Some(body) = row {
            let index = self.feed.pick_index();
            // A body selected shows so already.
            let selected = self
                .pick
                .selection
                .bodies()
                .any(|selected| selected == body);
            let faces = (index.body_faces(body).filter(|_| !selected))
                .map(Picked::Face)
                .collect();
            self.pick.selection.highlight_hovering(index, faces)
        } else if hover.is_none() && self.pick.selection.is_empty() {
            ModelHighlight::default()
        } else {
            let index = self.feed.pick_index();
            self.pick.selection.highlight(index, hover)
        };
        self.pick.highlight = Arc::new(highlight);
        self.pick.built = Some(key);
    }

    /// Rebuilds the extrude's or revolve's highlight, the body whose row in
    /// its panel is hovered lit, if the model or the body changed since it
    /// was built. The preview keeps the bodies' ids, so the body is found
    /// in the model shown, preview or not.
    fn refresh_panel_highlight(&mut self) {
        let operating = self.extrude.is_some() || self.revolve.is_some();
        let body = (self.panel_hover())
            .filter(|_| operating)
            .and_then(PanelHover::body);
        let key = body.map(|body| (self.feed.model(), body));
        if self.pick.panel_built == key {
            return;
        }
        self.pick.panel_highlight = Arc::new(match body {
            Some(body) => {
                let index = self.feed.pick_index();
                let faces: Vec<Picked> = index.body_faces(body).map(Picked::Face).collect();
                index.highlight_with(&faces, &[], &[])
            }
            None => ModelHighlight::default(),
        });
        self.pick.panel_built = key;
    }

    /// What's hovered as it's highlighted: while picking a plane, only a
    /// face that can take the sketch, which a click puts it on (the
    /// status bar says why another isn't): flat, and for a sketch whose
    /// plane is changed, of a body made before it.
    fn shown_hover(&self) -> Option<Pick> {
        let picking = self.model_picking()?;
        (self.pick.hover).filter(|pick| picking.takes(pick.target))
    }

    /// Picking for the viewport, if the cursor picks the model: faces
    /// and edges and the snap points of what it's over while measuring,
    /// only faces while picking a plane or a combine's bodies (a click
    /// anywhere on a body picks it, whatever the selection's mode), else
    /// what the selection's mode takes.
    pub(crate) fn model_picking(&self) -> Option<ModelPicking<'_>> {
        let measuring = self.measure.is_some();
        // An align's point is picked as the measure tool picks points: on
        // what the cursor is over, at its snap points.
        let pointing = (self.motion.as_ref()).is_some_and(|session| match session.picking {
            MotionPick::Align(slot) => slot.role == AlignRole::Point,
            MotionPick::Point => true,
            _ => false,
        });
        // A split's regions or line are picked on its sketches, not on
        // the model.
        if (self.motion.as_ref()).is_some_and(MotionSession::picks_sketches) {
            return None;
        }
        self.picks().then(|| ModelPicking {
            index: self.feed.pick_index(),
            hovered: self.pick.hover().map(|pick| pick.target),
            hovered_snap: self.pick.hover().and_then(|pick| pick.snap),
            sketches: if self.picks_outside() {
                self.outside_sketches()
            } else {
                self.selectable_sketches()
            },
            hovered_sketch: self.pick.sketch_hover(),
            marked: if self.picks_outside() {
                self.outside_marked()
            } else if self.picks_sketch_items() {
                self.pick.selection.sketch_items().collect()
            } else {
                Vec::new()
            },
            picks: if measuring || pointing || self.picks_outside() {
                Picks::All
            } else if let Some(session) = &self.motion {
                match session.picking {
                    MotionPick::Reference if session.kind.takes_axis() => Picks::EdgesAndFaces,
                    MotionPick::Align(_) => Picks::EdgesAndFaces,
                    MotionPick::Edge | MotionPick::Edges | MotionPick::Path => Picks::Edges,
                    _ => Picks::Faces,
                }
            } else if self.picking_plane.is_some() || self.combine.is_some() {
                Picks::Faces
            } else {
                self.pick.selection.mode().picks()
            },
            snaps: measuring || pointing,
            hovered_origin: self.hovered_plane(),
            origin_planes: !measuring && self.picks_origin_planes(),
            whole: {
                let selected: Vec<_> = self.selected_sketches().collect();
                self.placed_sketches(|feature| feature.visible && selected.contains(&feature.id))
            },
            planes: (self.picking_plane.as_ref())
                .filter(|_| !measuring)
                .map(|picking| &picking.pick),
        })
    }

    /// The origin plane hovered in the viewport, while a plane is picked.
    pub(crate) fn hovered_plane(&self) -> Option<varde_document::OriginPlane> {
        self.plane_hover.filter(|_| self.picks_origin_planes())
    }

    /// Whether an origin plane can be picked: a sketch's plane, or an
    /// operation's plane its toolbar offers.
    pub(crate) fn picks_origin_planes(&self) -> bool {
        match &self.motion {
            Some(session) => varde_view::motion_picks_origin_planes(
                session.kind,
                session.picking,
                session.split.mode == varde_view::SplitMode::Face,
            ),
            None => self.picking_plane.is_some(),
        }
    }

    /// What the viewport draws over the model, if anything: nothing while
    /// the cursor doesn't pick it.
    pub(crate) fn highlight(&self) -> Option<&Arc<ModelHighlight>> {
        // A body's Opacity or Colour hovered or slid shows it as drawn.
        if self.body_look_hovered || self.sliding() {
            return None;
        }
        // A link's row hovered lights what it comes from, whatever's
        // picked.
        if let Some(highlight) = self.hovered_link_highlight() {
            return Some(highlight);
        }
        if self.picks_outside() {
            return Some(&self.pick.outside_highlight).filter(|highlight| !highlight.is_empty());
        }
        // While measuring, the measure tool's, and not the selection's.
        if self.measure.is_some() {
            return self.measure_highlight().filter(|_| self.picks());
        }
        if self.combine.is_some() {
            return self.combine_highlight().filter(|_| self.picks());
        }
        if self.motion.is_some() {
            return self.motion_highlight().filter(|_| self.picks());
        }
        // An extrude's or revolve's: the body hovered in its panel.
        if self.extrude.is_some() || self.revolve.is_some() {
            let current =
                (self.pick.panel_built).is_some_and(|(model, _)| model == self.feed.model());
            return current.then_some(&self.pick.panel_highlight);
        }
        // Only of the model shown: one built for an earlier model would be
        // drawn over another.
        let current = (self.pick.built.as_ref()).is_some_and(|built| built.0 == self.feed.model());
        Some(&self.pick.highlight)
            .filter(|highlight| self.picks() && current && !highlight.is_empty())
    }
}

/// Whether `item` is a curve or point of a sketch `document` shows, one
/// the cursor may hover or keep selected.
fn shown_sketch_item(document: &Document, item: SketchItem) -> bool {
    document
        .feature(item.sketch)
        .is_some_and(|feature| feature.visible)
        && sketch_holds(document, item)
}

/// Whether `item.sketch` is a sketch of `document` holding the curve or
/// point `item.item`.
pub(crate) fn sketch_holds(document: &Document, item: SketchItem) -> bool {
    document.feature(item.sketch).is_some_and(|feature| {
        matches!(&feature.kind, FeatureKind::Sketch { sketch, .. }
            if sketch.point(item.item).is_some() || sketch.curve(item.item).is_some())
    })
}

#[cfg(test)]
mod tests;
