//! [`Document::check`]: what a file or a [`Command`](crate::Command) could
//! get wrong, its per-reference helpers, and [`CheckError`].

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use varde_kernel_compat::Tolerance;
use varde_sketch::{Design, Sketch, SketchError};

use crate::{
    Align, AlignError, AlignRefs, AxisLine, AxisRef, BodyId, ChamferError, Combine, CombineError,
    Copies, Document, EdgeRef, Extrude, ExtrudeError, FaceDraftError, FaceRef, FaceSetError,
    FeatureId, FeatureKind, FilletError, LinkError, LinkSource, Loft, LoftError, MAX_NAME_LEN,
    MotionError, OffsetFaceError, Opacity, Operation, OperationError, OutsideRef, ParamError,
    PathPart, PathRef, Pattern, Plane, PlaneError, PlaneRef, PointRef, Revolve, RevolveError,
    Scale, ScaleError, ShellError, Split, SplitError, SplitTool, Sweep, SweepError, Tint, motion,
    param,
};

impl Document {
    /// Checks what a file or a [`Command`](crate::Command) could get wrong, and
    /// [`Editor::apply`](crate::Editor::apply) refuses a command that does: the parameters are
    /// at most [`MAX_PARAMS`](varde_expr::MAX_PARAMS), each named as
    /// [`check_name`](varde_expr::check_name) takes, no name twice, each
    /// text at most [`MAX_LEN`](varde_expr::MAX_LEN) bytes (an expression
    /// may be in error: only values using it may not, which their
    /// features' checks find, every value read with the parameters,
    /// [`Document::design`]); body ids are in
    /// increasing order and below `next_id`, so bodies added later get new
    /// ids and come last, where an edit adds them, and the same for
    /// feature ids; no name is longer than [`MAX_NAME_LEN`]; every body's
    /// opacity is one [`Opacity::new`] takes, and its colour, if it has one,
    /// one [`Tint::new`] takes; the tolerance is one
    /// [`Tolerance::new`] takes; every body is made by a feature the
    /// document holds that names it as its new body
    /// ([`FeatureKind::new_body`]), and every such body is there, or by a
    /// pattern listing it as a copy body ([`Copies::Separate`]), every such
    /// body there too, one per copy; every id a feature holds for a body it
    /// made before ([`FeatureKind::held_body`]) is below `next_id`, no
    /// body's, and held by that feature alone; the rollback, if set, names
    /// a feature there; and every feature is as its kind wants it, see
    /// `Document::check_feature`.
    pub fn check(&self) -> Result<(), CheckError> {
        param::check_params(&self.params)?;
        debug_assert!(
            self.resolved == param::resolve(&self.params, self.units),
            "the resolved parameters are kept in step"
        );
        // Ids first: features are found by id, bodies by binary search.
        debug_assert!(
            self.index.len() <= self.features.len()
                && (self.index.iter()).all(|(&id, &at)| self.features[at].id == id),
            "the index is kept in step"
        );
        if self.index.len() < self.features.len() {
            let mut ids = BTreeSet::new();
            let twice = (self.features.iter()).find(|feature| !ids.insert(feature.id));
            let twice = twice.expect("an id is there twice").id;
            return Err(CheckError::FeatureTwice(twice));
        }
        if let Some(pair) = self.bodies.windows(2).find(|pair| pair[0].id >= pair[1].id) {
            return Err(CheckError::Order(pair[1].id, pair[0].id));
        }
        if Tolerance::new(self.tolerance).is_none() {
            return Err(CheckError::Tolerance(self.tolerance));
        }
        // How many bodies each pattern makes, which its own check holds
        // to the copy bodies it lists.
        let mut copies: BTreeMap<FeatureId, usize> = BTreeMap::new();
        for body in &self.bodies {
            let id = body.id;
            if body.name.len() > MAX_NAME_LEN {
                return Err(CheckError::NameLength(id, body.name.len()));
            }
            if !body.opacity.in_range() {
                return Err(CheckError::Opacity(id, body.opacity.percent()));
            }
            if let Some(tint) = body.color.filter(|tint| !tint.in_range()) {
                return Err(CheckError::Tint(id, tint.hue(), tint.saturation()));
            }
            let made = match self.feature(body.created_by).map(|feature| &feature.kind) {
                Some(FeatureKind::Pattern(_)) => {
                    *copies.entry(body.created_by).or_default() += 1;
                    true
                }
                Some(kind) => kind.new_body() == Some(id),
                None => false,
            };
            if !made {
                return Err(CheckError::Creator(id, body.created_by));
            }
        }
        let design = self.design();
        for (index, feature) in self.features.iter().enumerate() {
            let id = feature.id;
            if feature.name.len() > MAX_NAME_LEN {
                return Err(CheckError::FeatureNameLength(id, feature.name.len()));
            }
            self.check_feature(index, &design, &copies)?;
        }
        if let Some(last) = self.bodies.last()
            && last.id.0 >= self.next_id
        {
            return Err(CheckError::NextId(last.id));
        }
        // Ids held for bodies made before: given out, no body's, held once.
        let mut held = BTreeSet::new();
        for feature in &self.features {
            if let Some(body) = feature.kind.held_body()
                && (body.0 >= self.next_id || self.body(body).is_some() || !held.insert(body))
            {
                return Err(CheckError::Held(feature.id, body));
            }
        }
        if let Some(rollback) = self.rollback
            && self.feature(rollback).is_none()
        {
            return Err(CheckError::Rollback(rollback));
        }
        match self.index.last_key_value() {
            Some((&last, _)) if last.0 >= self.next_id => Err(CheckError::FeatureNextId(last)),
            _ => Ok(()),
        }
    }

    /// Checks feature `index` as its kind wants it, `copies` the number of
    /// bodies each pattern makes: a sketch passes [`Sketch::check`]
    /// against [`MAX_COORD`](crate::MAX_COORD) and the document's units
    /// ([`Document::design`]) and names what comes before it, as
    /// [`PlaneError`] lists; every other kind passes its own `check_own`
    /// (such as [`Move::check_own`](crate::Move::check_own)) and, against the features before it,
    /// its `Document::check_<kind>` (for the bodies, sketches, edges, faces
    /// and references it names). A revolve's axis line, a sweep's path's
    /// curves and a loft's points and rail curves aren't checked against
    /// their sketches here (see [`Revolve::check_axis`],
    /// [`Sweep::check_curves`] and [`Loft::check_names`]).
    fn check_feature(
        &self,
        index: usize,
        design: &Design<'_>,
        copies: &BTreeMap<FeatureId, usize>,
    ) -> Result<(), CheckError> {
        let id = self.features[index].id;
        match &self.features[index].kind {
            FeatureKind::Sketch {
                plane,
                sketch,
                sources,
            } => {
                sketch
                    .check(design)
                    .map_err(|why| CheckError::Sketch(id, why))?;
                self.check_plane(index, plane)
                    .map_err(|why| CheckError::SketchPlane(id, why))?;
                self.check_sources(index, sketch, sources)
                    .map_err(|why| CheckError::SketchLink(id, why))?;
            }
            FeatureKind::Extrude(extrude) => self
                .check_extrude(index, extrude)
                .map_err(|why| CheckError::Extrude(id, why))?,
            FeatureKind::Revolve(revolve) => self
                .check_revolve(index, revolve)
                .map_err(|why| CheckError::Revolve(id, why))?,
            FeatureKind::Combine(combine) => self
                .check_combine(index, combine)
                .map_err(|why| CheckError::Combine(id, why))?,
            FeatureKind::Move(moved) => {
                moved
                    .check_own(design)
                    .map_err(|why| CheckError::Move(id, why))?;
                self.check_motion(index, &moved.bodies, moved.referred())
                    .map_err(|why| CheckError::Move(id, why))?;
            }
            FeatureKind::Mirror(mirror) => {
                mirror
                    .check_own()
                    .map_err(|why| CheckError::Mirror(id, why))?;
                self.check_motion(index, &mirror.bodies, mirror.referred())
                    .map_err(|why| CheckError::Mirror(id, why))?;
            }
            FeatureKind::Pattern(pattern) => {
                pattern
                    .check_own(design)
                    .map_err(|why| CheckError::Pattern(id, why))?;
                self.check_motion(index, &pattern.bodies, pattern.referred())
                    .map_err(|why| CheckError::Pattern(id, why))?;
                let made = copies.get(&id).copied().unwrap_or(0);
                self.check_copies(id, pattern, made)
                    .map_err(|why| CheckError::Pattern(id, why))?;
            }
            FeatureKind::Align(align) => {
                align
                    .check_own(design)
                    .map_err(|why| CheckError::Align(id, why))?;
                self.check_align(index, align)
                    .map_err(|why| CheckError::Align(id, why))?;
            }
            FeatureKind::Scale(scale) => {
                scale
                    .check_own(design)
                    .map_err(|why| CheckError::Scale(id, why))?;
                self.check_scale(index, scale)
                    .map_err(|why| CheckError::Scale(id, why))?;
            }
            FeatureKind::Split(split) => {
                split
                    .check_own()
                    .map_err(|why| CheckError::Split(id, why))?;
                self.check_split(index, split)
                    .map_err(|why| CheckError::Split(id, why))?;
            }
            FeatureKind::Chamfer(chamfer) => {
                chamfer
                    .check_own(design)
                    .map_err(|why| CheckError::Chamfer(id, why))?;
                self.check_blend_edges(index, &chamfer.edges, &chamfer.faces)
                    .map_err(|why| CheckError::Chamfer(id, ChamferError::Edges(why)))?;
            }
            FeatureKind::Shell(shell) => {
                shell
                    .check_own(design)
                    .map_err(|why| CheckError::Shell(id, why))?;
                self.check_face_set(index, shell.body, &shell.open)
                    .map_err(|why| CheckError::Shell(id, ShellError::Set(why)))?;
            }
            FeatureKind::OffsetFace(offset) => {
                offset
                    .check_own(design)
                    .map_err(|why| CheckError::OffsetFace(id, why))?;
                let body = offset.body().expect("checked: it has faces");
                self.check_face_set(index, body, &offset.faces)
                    .map_err(|why| CheckError::OffsetFace(id, OffsetFaceError::Set(why)))?;
            }
            FeatureKind::Fillet(fillet) => {
                fillet
                    .check_own(design)
                    .map_err(|why| CheckError::Fillet(id, why))?;
                self.check_blend_edges(index, &fillet.edges, &fillet.faces)
                    .map_err(|why| CheckError::Fillet(id, FilletError::Edges(why)))?;
            }
            FeatureKind::FaceDraft(draft) => {
                draft
                    .check_own(design)
                    .map_err(|why| CheckError::FaceDraft(id, why))?;
                let body = draft.body().expect("checked: it has faces");
                self.check_face_set(index, body, &draft.faces)
                    .map_err(|why| CheckError::FaceDraft(id, FaceDraftError::Set(why)))?;
                self.check_neutral_plane(index, &draft.neutral)
                    .map_err(|why| CheckError::FaceDraft(id, why))?;
            }
            FeatureKind::Sweep(sweep) => self
                .check_sweep(index, sweep)
                .map_err(|why| CheckError::Sweep(id, why))?,
            FeatureKind::Loft(loft) => self
                .check_loft(index, loft)
                .map_err(|why| CheckError::Loft(id, why))?,
        }
        Ok(())
    }

    /// Checks `plane`, the plane of sketch feature `index`: a face's point
    /// in bounds, its body, if it's there, made by a feature before the
    /// sketch, and the feature its key names, if it's there, before the
    /// sketch. A body or a feature that isn't there is allowed (removing
    /// the body's maker leaves the sketch, which regenerating then fails,
    /// to be put on another plane), but only with an id below `next_id`,
    /// one no body or feature made later can take: otherwise the edit
    /// that made it would be refused, and every one after it, the ids
    /// being handed out again.
    fn check_plane(&self, index: usize, plane: &Plane) -> Result<(), PlaneError> {
        let Some(face) = plane.face() else {
            return Ok(());
        };
        face.check_own()?;
        self.refers_before(index, face.body, &[face.maker()])
            .map_err(|late| late.or(PlaneError::Body, PlaneError::Maker))
    }

    /// Checks the sources of the links of `sketch`, feature `index`: one
    /// for each link, in its order, each of a kind the link takes
    /// ([`OutsideRef::takes`]), its own parts right, and what it names
    /// made before the sketch, or not there with ids no later feature or
    /// body can take, as a sketch's face's: a removed source leaves the
    /// link, which then doesn't find it. Another sketch's item is a
    /// sketch's before it; whether it has the item is regenerating's to
    /// find.
    fn check_sources(
        &self,
        index: usize,
        sketch: &Sketch,
        sources: &[LinkSource],
    ) -> Result<(), LinkError> {
        if let Some(extra) = sources.get(sketch.links.len()) {
            return Err(LinkError::Sources(extra.link));
        }
        for (at, link) in sketch.links.iter().enumerate() {
            let id = link.id;
            let from = sources.get(at).ok_or(LinkError::Sources(id))?;
            if from.link != id {
                return Err(LinkError::Sources(from.link));
            }
            if !from.source.takes(link.kind) {
                return Err(LinkError::Kind(id));
            }
            match &from.source {
                OutsideRef::Sketch { sketch: other, .. } => {
                    let before = match self.feature_index(*other) {
                        Some(at) => {
                            at < index
                                && matches!(self.features[at].kind, FeatureKind::Sketch { .. })
                        }
                        None => other.0 < self.next_id,
                    };
                    if !before {
                        return Err(LinkError::Later(id));
                    }
                }
                OutsideRef::Edge(edge) => {
                    edge.check_own().map_err(|why| LinkError::Edge(id, why))?;
                    (self.refers_before(index, edge.body, &edge.makers()))
                        .map_err(|_| LinkError::Later(id))?;
                }
                OutsideRef::Face(face) => {
                    face.check_own().map_err(|why| LinkError::Face(id, why))?;
                    (self.refers_before(index, face.body, &[face.maker()]))
                        .map_err(|_| LinkError::Later(id))?;
                }
                OutsideRef::Corner(corner) => {
                    let (PointRef::Corner { body, .. }, Ok(())) = (corner, corner.check_own())
                    else {
                        return Err(LinkError::Corner(id));
                    };
                    (self.refers_before(index, *body, &corner.makers()))
                        .map_err(|_| LinkError::Later(id))?;
                }
            }
        }
        Ok(())
    }

    /// Whether `feature`, as a face's maker that a reference of feature
    /// `index` names, comes before it: there and before it, or not there
    /// with an id below `next_id`, one no feature made later can take
    /// (see [`Document::check_plane`]).
    fn maker_before(&self, index: usize, feature: FeatureId) -> bool {
        match self.feature_index(feature) {
            Some(maker) => maker < index,
            None => feature.0 < self.next_id,
        }
    }

    /// Checks that `body` and `makers`, as what a reference of feature
    /// `index` names (a face's or edge's body and the makers of its
    /// faces), come before it ([`Document::body_before`],
    /// [`Document::maker_before`]): the body first, then the makers in
    /// order, the first that doesn't the error.
    pub(crate) fn refers_before(
        &self,
        index: usize,
        body: BodyId,
        makers: &[FeatureId],
    ) -> Result<(), Late> {
        if !self.body_before(index, body) {
            return Err(Late::Body(body));
        }
        match makers.iter().find(|&&m| !self.maker_before(index, m)) {
            Some(&maker) => Err(Late::Maker(maker)),
            None => Ok(()),
        }
    }

    /// Whether `body`, as one that a reference of feature `index` names,
    /// is made before it: there and made by a feature before it, or not
    /// there with an id below `next_id` (see [`Document::check_plane`]).
    fn body_before(&self, index: usize, body: BodyId) -> bool {
        match self.body(body) {
            Some(body) => self.maker_before(index, body.created_by),
            None => body.0 < self.next_id,
        }
    }

    /// Checks `extrude`, feature `index`, see [`Document::check`].
    fn check_extrude(&self, index: usize, extrude: &Extrude) -> Result<(), ExtrudeError> {
        if !self.sketch_ref_before(index, extrude.sketch) {
            return Err(ExtrudeError::Sketch(extrude.sketch));
        }
        extrude.check_own(&self.design())?;
        self.check_uses(index, &extrude.operation)
            .map_err(ExtrudeError::Operation)
    }

    /// Checks `revolve`, feature `index`, see [`Document::check`].
    fn check_revolve(&self, index: usize, revolve: &Revolve) -> Result<(), RevolveError> {
        if !self.sketch_ref_before(index, revolve.sketch) {
            return Err(RevolveError::Sketch(revolve.sketch));
        }
        revolve.check_own(&self.design())?;
        if let AxisLine::Edge(edge) = &revolve.axis {
            self.check_edge(index, edge)?;
        }
        self.check_uses(index, &revolve.operation)
            .map_err(RevolveError::Operation)
    }

    /// Checks `edge`, the axis of a revolve at feature `index` (the
    /// feature count for one added after the last): its own parts, and its
    /// body and its faces' makers made before it, or not there with ids
    /// no later body or feature can take, as a sketch's face's (as
    /// [`Document::check`] has them). What a revolve being set up checks
    /// its edge by as the document changes under it.
    pub fn check_edge(&self, index: usize, edge: &EdgeRef) -> Result<(), RevolveError> {
        edge.check_own().map_err(RevolveError::Edge)?;
        (self.refers_before(index, edge.body, &edge.makers()))
            .map_err(|late| late.or(RevolveError::EdgeBody, RevolveError::EdgeMaker))
    }

    /// Checks `axis` as the axis of a move at feature `index` (at the
    /// end for a new one, the count of features) would name it: its own
    /// parts, and an edge's or face's body and its faces' makers before
    /// it, as [`Document::check`] has them. For a panel keeping what it
    /// sets up one the document takes.
    pub fn check_axis_ref(&self, index: usize, axis: &AxisRef) -> Result<(), MotionError> {
        if let Some(referred) = axis.refers() {
            referred.check_own().map_err(MotionError::Ref)?;
        }
        self.check_motion(index, &[], axis.refers())
    }

    /// Checks `plane` as the plane of a mirror at feature `index` would
    /// name it, as [`Document::check_axis_ref`] does a move's axis.
    pub fn check_plane_ref(&self, index: usize, plane: &PlaneRef) -> Result<(), MotionError> {
        if let Some(referred) = plane.refers() {
            referred.check_own().map_err(MotionError::Ref)?;
        }
        self.check_motion(index, &[], plane.refers())
    }

    /// Checks `loft`, feature `index`, see [`Document::check`]: every
    /// section's and rail's sketch a sketch feature before it, its own
    /// parts, and its operation as an extrude's.
    fn check_loft(&self, index: usize, loft: &Loft) -> Result<(), LoftError> {
        if let Some(section) =
            (loft.sections.iter()).find(|s| !self.sketch_ref_before(index, s.sketch()))
        {
            return Err(LoftError::Sketch(section.sketch()));
        }
        if let Some(rail) = (loft.rails.iter()).find(|r| !self.sketch_ref_before(index, r.sketch)) {
            return Err(LoftError::RailSketch(rail.sketch));
        }
        loft.check_own()?;
        self.check_uses(index, &loft.operation)
            .map_err(LoftError::Operation)
    }

    /// Checks `combine`, feature `index`, see [`Document::check`]: its own
    /// parts, and every body it names made by a feature before it, or
    /// not there with an id no later body can take, as a sketch's face's:
    /// removing a body's maker removes the combine too
    /// ([`Document::removal`]), unless the user chose to keep it
    /// ([`Document::breaking_removal`]), and an edit can stop a body
    /// being made (setting its maker to join); regenerating then fails
    /// the combine until it's given another. Edits are never refused for
    /// what later features name.
    fn check_combine(&self, index: usize, combine: &Combine) -> Result<(), CombineError> {
        combine.check_own()?;
        for body in combine.bodies() {
            if !self.body_before(index, body) {
                return Err(CombineError::Body(body));
            }
        }
        Ok(())
    }

    /// Checks what a move or mirror, feature `index`, names: every body
    /// in `bodies` there and made by a feature before it, as a combine's
    /// ([`Document::check_combine`]), and the edge or face its axis or
    /// plane names (`referred`) on a body and of makers before it, or not
    /// there with ids no later body or feature can take, as a sketch's
    /// face's ([`Document::check_plane`]).
    fn check_motion(
        &self,
        index: usize,
        bodies: &[BodyId],
        referred: Option<motion::Referred<'_>>,
    ) -> Result<(), MotionError> {
        if let Some(&body) = bodies.iter().find(|&&body| !self.body_before(index, body)) {
            return Err(MotionError::Body(body));
        }
        if let Some(referred) = referred {
            (self.refers_before(index, referred.body(), &referred.makers()))
                .map_err(|late| late.or(MotionError::RefBody, MotionError::RefMaker))?;
        }
        Ok(())
    }

    /// Checks the copy bodies of `pattern`, feature `id`, which makes
    /// `made` bodies: joined, none; else one per copy, as
    /// [`Copies::Separate`] lays them out, each a body it makes, none
    /// repeated, and every body it makes among them.
    fn check_copies(
        &self,
        id: FeatureId,
        pattern: &Pattern,
        made: usize,
    ) -> Result<(), MotionError> {
        let listed: &[BodyId] = match &pattern.copies {
            Copies::Joined => &[],
            Copies::Separate(listed) => listed,
        };
        let wanted = if pattern.joins() {
            Some(0)
        } else {
            pattern.separate_count()
        };
        if wanted != Some(listed.len()) || made != listed.len() {
            return Err(MotionError::CopyBodies);
        }
        let mut sorted = listed.to_vec();
        sorted.sort_unstable();
        let repeated = sorted.windows(2).any(|pair| pair[0] == pair[1]);
        let theirs =
            (listed.iter()).all(|&body| self.body(body).is_some_and(|b| b.created_by == id));
        if repeated || !theirs {
            return Err(MotionError::CopyBodies);
        }
        Ok(())
    }

    /// Checks what an align, feature `index`, names: its body there and
    /// made by a feature before it, as a move's; every reference on the
    /// moved side on that body, every one on the target side on another;
    /// and each named as a move's axis is ([`Document::check_motion`]).
    fn check_align(&self, index: usize, align: &Align) -> Result<(), AlignError> {
        if !self.body_before(index, align.body) {
            return Err(AlignError::Body(align.body));
        }
        for named in align.from.named() {
            if named.body != align.body {
                return Err(AlignError::FromBody(named.body));
            }
        }
        for named in align.to.named() {
            if named.body == align.body {
                return Err(AlignError::OnMoved);
            }
        }
        self.check_align_refs(index, &align.to)?;
        self.check_align_refs(index, &align.from)
    }

    /// Checks `refs` as one side of an align at feature `index` (at the
    /// end for a new one) would name them: each reference's own parts,
    /// and its body and its faces' makers before it, as
    /// [`Document::check`] has them. For a panel keeping what it sets up
    /// one the document takes; which side is which body is the caller's.
    pub fn check_align_refs(&self, index: usize, refs: &AlignRefs) -> Result<(), AlignError> {
        refs.point.check_own()?;
        for direction in refs.directions() {
            direction.check_own()?;
        }
        for named in refs.named() {
            (self.refers_before(index, named.body, &named.makers))
                .map_err(|late| late.or(AlignError::RefBody, AlignError::RefMaker))?;
        }
        Ok(())
    }

    /// Checks what a scale, feature `index`, names: its bodies there and
    /// made by features before it, as a move's, and its point and its
    /// edge length's edge named as a move's axis is
    /// ([`Document::check_motion`]).
    fn check_scale(&self, index: usize, scale: &Scale) -> Result<(), ScaleError> {
        if let Some(&body) = (scale.bodies.iter()).find(|&&body| !self.body_before(index, body)) {
            return Err(ScaleError::Body(body));
        }
        self.check_scale_refs(index, scale)
    }

    /// Checks the point and the edge of `scale` as a scale at feature
    /// `index` (at the end for a new one) would name them: each one's
    /// body and its faces' makers before it, as [`Document::check`] has
    /// them. For a panel keeping what it sets up one the document takes;
    /// their own parts are [`Scale::check_own`]'s.
    pub fn check_scale_refs(&self, index: usize, scale: &Scale) -> Result<(), ScaleError> {
        for (body, makers) in scale.named() {
            (self.refers_before(index, body, &makers))
                .map_err(|late| late.or(ScaleError::RefBody, ScaleError::RefMaker))?;
        }
        Ok(())
    }

    /// Whether the point `point`, as feature `index` (a scale) would
    /// name it, is taken there: its body and its faces' makers before
    /// it, as [`Document::check_scale_refs`] has them.
    pub fn point_before(&self, index: usize, point: &PointRef) -> bool {
        (point.body()).is_none_or(|body| self.refers_before(index, body, &point.makers()).is_ok())
    }

    /// Whether the edge `edge`, as feature `index` (a scale) would name
    /// it, is taken there: its body and its faces' makers before it, as
    /// [`Document::check_scale_refs`] has them.
    pub fn edge_before(&self, index: usize, edge: &EdgeRef) -> bool {
        self.refers_before(index, edge.body, &edge.makers()).is_ok()
    }

    /// Checks what a split, feature `index`, names: its body, a tool
    /// body and a face tool's body there and made by features before it,
    /// as a combine's ([`Document::check_combine`]); a plane face's body
    /// and any face's maker as a mirror's plane ([`Document::check_motion`]);
    /// a sketch tool's sketch a sketch feature before it; and its new
    /// body, while it keeps both pieces, a body it makes (the id it holds
    /// otherwise is [`Document::check`]'s, as every held id).
    fn check_split(&self, index: usize, split: &Split) -> Result<(), SplitError> {
        if !self.body_before(index, split.body) {
            return Err(SplitError::Body(split.body));
        }
        self.check_split_tool(index, &split.tool)?;
        let id = self.features[index].id;
        if let Some(body) = split.made_body()
            && self.body(body).is_none_or(|body| body.created_by != id)
        {
            return Err(SplitError::NewBody(body));
        }
        Ok(())
    }

    /// Checks what `tool` names as the tool of a split at feature `index`
    /// (at the end for a new one, the count of features), as
    /// [`Document::check`] has it: a tool body, or a face tool's body,
    /// there and made by a feature before it; a plane face's body and any
    /// face's maker as a mirror's plane; a sketch tool's sketch a sketch
    /// feature before it. For a panel keeping what it sets up one the
    /// document takes; its own parts are [`Split::check_own`]'s.
    pub fn check_split_tool(&self, index: usize, tool: &SplitTool) -> Result<(), SplitError> {
        match tool {
            SplitTool::Body(tool) if !self.body_before(index, *tool) => {
                return Err(SplitError::ToolBody(*tool));
            }
            SplitTool::Face(face) if !self.body_before(index, face.body) => {
                return Err(SplitError::FaceBody(face.body));
            }
            SplitTool::Plane(PlaneRef::Face(face)) if !self.body_before(index, face.body) => {
                return Err(SplitError::RefBody(face.body));
            }
            SplitTool::Regions { sketch, .. } | SplitTool::Chain { sketch, .. }
                if !self.sketch_ref_before(index, *sketch) =>
            {
                return Err(SplitError::Sketch(*sketch));
            }
            _ => {}
        }
        if let SplitTool::Plane(PlaneRef::Face(face)) | SplitTool::Face(face) = tool
            && !self.maker_before(index, face.maker())
        {
            return Err(SplitError::RefMaker(face.maker()));
        }
        Ok(())
    }

    /// Checks what a set of faces of `body` at feature `index` (at the
    /// end for a new one, the count of features) names, as
    /// [`Document::check`] has it for a shell's open faces and an offset
    /// face's faces: the body there and made by a feature before it
    /// (depended on, as a combine's bodies), and the faces' makers
    /// before it, or not there with ids no later feature can take, as a
    /// sketch's face's. For a panel keeping what it sets up one the
    /// document takes; the faces' own parts (on `body`, in order) are
    /// the feature's `check_own`'s.
    pub fn check_face_set(
        &self,
        index: usize,
        body: BodyId,
        faces: &[FaceRef],
    ) -> Result<(), FaceSetError> {
        let makers: Vec<_> = faces.iter().map(FaceRef::maker).collect();
        (self.refers_before(index, body, &makers))
            .map_err(|late| late.or(FaceSetError::Body, FaceSetError::RefMaker))
    }

    /// Checks `neutral` as the neutral plane of a draft at feature
    /// `index` (at the end for a new one, the count of features) would
    /// name it, as [`Document::check`] has it: a face's own parts (as
    /// [`FaceDraft::check_own`](crate::FaceDraft::check_own) checks them too), its body there and
    /// made by a feature before it (the draft depends on it), its key's
    /// maker before it or not there with an id no later feature can
    /// take. For a panel keeping what it sets up one the document takes.
    pub fn check_neutral_plane(
        &self,
        index: usize,
        neutral: &PlaneRef,
    ) -> Result<(), FaceDraftError> {
        let PlaneRef::Face(face) = neutral else {
            return Ok(());
        };
        face.check_own().map_err(FaceDraftError::Neutral)?;
        (self.refers_before(index, face.body, &[face.maker()]))
            .map_err(|late| late.or(FaceDraftError::NeutralBody, FaceDraftError::NeutralMaker))
    }

    /// Checks `sweep`, feature `index`, see [`Document::check`].
    fn check_sweep(&self, index: usize, sweep: &Sweep) -> Result<(), SweepError> {
        if !self.sketch_ref_before(index, sweep.sketch) {
            return Err(SweepError::Sketch(sweep.sketch));
        }
        sweep.check_own(&self.design())?;
        self.check_path(index, sweep.sketch, &sweep.path)?;
        self.check_uses(index, &sweep.operation)
            .map_err(SweepError::Operation)
    }

    /// Checks what `path` names as the path of a sweep at feature `index`
    /// (at the end for a new one, the count of features) whose profile
    /// is sketch `profile`, as [`Document::check`] has it: each part's
    /// sketch a sketch feature before it, not the profile's; each edge
    /// part's body there and made by a feature before it (depended on,
    /// as a chamfer's), its faces' makers before it, or not there with
    /// ids no later feature can take; a helix's axis's body and its
    /// faces' makers likewise. For a panel keeping what it sets up one
    /// the document takes; their own parts are [`Sweep::check_own`]'s.
    pub fn check_path(
        &self,
        index: usize,
        profile: FeatureId,
        path: &PathRef,
    ) -> Result<(), SweepError> {
        match path {
            PathRef::Chain(parts) => {
                for part in parts {
                    match part {
                        PathPart::Curves(chain) => {
                            if chain.sketch == profile {
                                return Err(SweepError::OwnSketch);
                            }
                            if !self.sketch_ref_before(index, chain.sketch) {
                                return Err(SweepError::PathSketch(chain.sketch));
                            }
                        }
                        PathPart::Edges { edges, .. } => {
                            for edge in edges {
                                (self.refers_before(index, edge.body, &edge.makers())).map_err(
                                    |late| late.or(SweepError::EdgeBody, SweepError::EdgeMaker),
                                )?;
                            }
                        }
                    }
                }
            }
            PathRef::Helix(helix) => {
                if let Some(referred) = helix.axis.refers() {
                    (self.refers_before(index, referred.body(), &referred.makers()))
                        .map_err(|late| late.or(SweepError::AxisBody, SweepError::AxisMaker))?;
                }
            }
        }
        Ok(())
    }

    /// Whether `sketch`, which feature `index` names as a sketch, is a
    /// sketch feature before it, or isn't there with an id below
    /// `next_id`, as a body it depends on is ([`Document::body_before`]): deleting a
    /// feature can leave those naming it, which regenerating then fails,
    /// to be given another ([`Document::breaking_removal`]).
    fn sketch_ref_before(&self, index: usize, sketch: FeatureId) -> bool {
        match self.feature_index(sketch) {
            Some(_) => self.sketch_before(index, sketch).is_some(),
            None => sketch.0 < self.next_id,
        }
    }

    /// The sketch feature before feature `index` whose id is `sketch`.
    pub(crate) fn sketch_before(&self, index: usize, sketch: FeatureId) -> Option<&Sketch> {
        let at = self.feature_index(sketch).filter(|&at| at < index)?;
        match &self.features[at].kind {
            FeatureKind::Sketch { sketch, .. } => Some(sketch),
            _ => None,
        }
    }

    /// Checks what extrudes, revolves, sweeps and lofts share, of feature
    /// `index`: `operation`'s new body names it as its maker and its
    /// excluded bodies are sorted and made by features before it. Each
    /// caller checks its sketch before its own parts, so that comes first.
    fn check_uses(&self, index: usize, operation: &Operation) -> Result<(), OperationError> {
        let id = self.features[index].id;
        if let Some(body) = operation.new_body()
            && self.body(body).is_none_or(|body| body.created_by != id)
        {
            return Err(OperationError::NewBody(body));
        }
        let excluded = operation.excluded();
        if !excluded.windows(2).all(|pair| pair[0] < pair[1]) {
            return Err(OperationError::ExcludedOrder);
        }
        // Removing a body drops it from the lists, so they never name one
        // that isn't there.
        for &body in excluded {
            if !self
                .body(body)
                .is_some_and(|body| self.maker_before(index, body.created_by))
            {
                return Err(OperationError::Excluded(body));
            }
        }
        Ok(())
    }
}

/// What of a reference comes after the feature naming it, see
/// [`Document::refers_before`].
pub(crate) enum Late {
    /// Its body.
    Body(BodyId),
    /// One of its faces' makers.
    Maker(FeatureId),
}

impl Late {
    /// As the error `body` or `maker` makes of it.
    pub(crate) fn or<E>(
        self,
        body: impl FnOnce(BodyId) -> E,
        maker: impl FnOnce(FeatureId) -> E,
    ) -> E {
        match self {
            Late::Body(id) => body(id),
            Late::Maker(id) => maker(id),
        }
    }
}

/// Why a [`Document`] fails [`Document::check`], and where.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CheckError {
    /// A body's name is this many bytes, over [`MAX_NAME_LEN`].
    NameLength(BodyId, usize),
    /// A body's opacity is this percent, out of [`Opacity::MIN`] to
    /// [`Opacity::MAX`].
    Opacity(BodyId, u8),
    /// A body's colour is this hue and saturation, out of what
    /// [`Tint::new`] takes.
    Tint(BodyId, u16, u8),
    /// A body's maker isn't an extrude, revolve, sweep, loft or split the document
    /// holds that makes it as its new body, or a pattern (whose own check
    /// holds it to the copy bodies it lists).
    Creator(BodyId, FeatureId),
    /// The first body's id doesn't come after the second's, the one
    /// before it.
    Order(BodyId, BodyId),
    /// The last body's id isn't below the document's next id.
    NextId(BodyId),
    /// A feature holds this id for a body it made before
    /// ([`FeatureKind::held_body`]), but it's a body's, another feature
    /// holds it too, or it isn't below the document's next id.
    Held(FeatureId, BodyId),
    /// A feature's name is this many bytes, over [`MAX_NAME_LEN`].
    FeatureNameLength(FeatureId, usize),
    /// A sketch feature's sketch fails [`Sketch::check`].
    Sketch(FeatureId, SketchError),
    /// A sketch feature's plane is wrong, see [`PlaneError`].
    SketchPlane(FeatureId, PlaneError),
    /// A sketch feature's links' sources are wrong, see [`LinkError`].
    SketchLink(FeatureId, LinkError),
    /// An extrude feature is wrong, see [`ExtrudeError`].
    Extrude(FeatureId, ExtrudeError),
    /// A revolve feature is wrong, see [`RevolveError`].
    Revolve(FeatureId, RevolveError),
    /// A combine feature is wrong, see [`CombineError`].
    Combine(FeatureId, CombineError),
    /// A move feature is wrong, see [`MotionError`].
    Move(FeatureId, MotionError),
    /// A mirror feature is wrong, see [`MotionError`].
    Mirror(FeatureId, MotionError),
    /// A pattern feature is wrong, see [`MotionError`].
    Pattern(FeatureId, MotionError),
    /// An align feature is wrong, see [`AlignError`].
    Align(FeatureId, AlignError),
    /// A scale feature is wrong, see [`ScaleError`].
    Scale(FeatureId, ScaleError),
    /// A split feature is wrong, see [`SplitError`].
    Split(FeatureId, SplitError),
    /// A chamfer feature is wrong, see [`ChamferError`].
    Chamfer(FeatureId, ChamferError),
    /// A shell feature is wrong, see [`ShellError`].
    Shell(FeatureId, ShellError),
    /// A fillet feature is wrong, see [`FilletError`].
    Fillet(FeatureId, FilletError),
    /// An offset face feature is wrong, see [`OffsetFaceError`].
    OffsetFace(FeatureId, OffsetFaceError),
    /// A draft feature is wrong, see [`FaceDraftError`].
    FaceDraft(FeatureId, FaceDraftError),
    /// A sweep feature is wrong, see [`SweepError`].
    Sweep(FeatureId, SweepError),
    /// A loft feature is wrong, see [`LoftError`].
    Loft(FeatureId, LoftError),
    /// The fit tolerance, in millimetres, isn't one [`Tolerance::new`]
    /// takes.
    Tolerance(f64),
    /// Two features have this id.
    FeatureTwice(FeatureId),
    /// The last feature's id isn't below the document's next id.
    FeatureNextId(FeatureId),
    /// The Timeline is rolled back to before a feature that isn't there.
    Rollback(FeatureId),
    /// The parameter at this index is wrong, see [`ParamError`].
    Param(usize, ParamError),
    /// There are this many parameters, over
    /// [`MAX_PARAMS`](varde_expr::MAX_PARAMS).
    Params(usize),
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckError::NameLength(id, len) => write!(
                f,
                "body {} has a name of {len} bytes, over the limit of {MAX_NAME_LEN}",
                id.0
            ),
            CheckError::Opacity(id, percent) => write!(
                f,
                "body {} has an opacity of {percent} %, not from {} to {}",
                id.0,
                Opacity::MIN,
                Opacity::MAX
            ),
            CheckError::Tint(id, hue, saturation) => write!(
                f,
                "body {} has a colour of hue {hue}° and saturation {saturation} %, not below 360° \
                 and at most {} %",
                id.0,
                Tint::MAX_SATURATION
            ),
            CheckError::Creator(id, feature) => write!(
                f,
                "body {} is made by feature {}, which doesn't make it",
                id.0, feature.0
            ),
            CheckError::Order(id, before) => {
                write!(f, "body id {} doesn't come after {}", id.0, before.0)
            }
            CheckError::NextId(id) => write!(f, "body id {} is not below the next id", id.0),
            CheckError::Held(feature, id) => write!(
                f,
                "feature {} holds body id {} for a body it made, which is taken or not given out yet",
                feature.0, id.0
            ),
            CheckError::FeatureNameLength(id, len) => write!(
                f,
                "feature {} has a name of {len} bytes, over the limit of {MAX_NAME_LEN}",
                id.0
            ),
            CheckError::Sketch(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::SketchPlane(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::SketchLink(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Extrude(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Revolve(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Combine(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Move(id, why)
            | CheckError::Mirror(id, why)
            | CheckError::Pattern(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Align(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Scale(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Split(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Chamfer(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Shell(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Fillet(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::OffsetFace(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::FaceDraft(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Sweep(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Loft(id, why) => write!(f, "feature {}: {why}", id.0),
            CheckError::Tolerance(fit) => write!(
                f,
                "the tolerance {fit} mm isn't from {} to {} mm",
                Tolerance::MIN_FIT,
                Tolerance::MAX_FIT
            ),
            CheckError::FeatureTwice(id) => write!(f, "two features have id {}", id.0),
            CheckError::FeatureNextId(id) => {
                write!(f, "feature id {} is not below the next id", id.0)
            }
            CheckError::Rollback(id) => {
                write!(
                    f,
                    "the timeline is rolled back to feature id {}, which isn't there",
                    id.0
                )
            }
            CheckError::Param(index, why) => write!(f, "parameter {}: {why}", index + 1),
            CheckError::Params(count) => write!(
                f,
                "there are {count} parameters, over the limit of {}",
                varde_expr::MAX_PARAMS
            ),
        }
    }
}

impl std::error::Error for CheckError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CheckError::Sketch(_, why) => Some(why),
            CheckError::SketchPlane(_, why) => Some(why),
            CheckError::SketchLink(_, why) => Some(why),
            CheckError::Extrude(_, why) => Some(why),
            CheckError::Revolve(_, why) => Some(why),
            CheckError::Combine(_, why) => Some(why),
            CheckError::Move(_, why) | CheckError::Mirror(_, why) | CheckError::Pattern(_, why) => {
                Some(why)
            }
            CheckError::Align(_, why) => Some(why),
            CheckError::Scale(_, why) => Some(why),
            CheckError::Split(_, why) => Some(why),
            CheckError::Chamfer(_, why) => Some(why),
            CheckError::Shell(_, why) => Some(why),
            CheckError::Fillet(_, why) => Some(why),
            CheckError::OffsetFace(_, why) => Some(why),
            CheckError::FaceDraft(_, why) => Some(why),
            CheckError::Sweep(_, why) => Some(why),
            CheckError::Loft(_, why) => Some(why),
            CheckError::Param(_, why) => Some(why),
            CheckError::NameLength(..)
            | CheckError::Opacity(..)
            | CheckError::Tint(..)
            | CheckError::Creator(..)
            | CheckError::Order(..)
            | CheckError::NextId(..)
            | CheckError::Held(..)
            | CheckError::FeatureNameLength(..)
            | CheckError::Tolerance(..)
            | CheckError::FeatureTwice(..)
            | CheckError::FeatureNextId(..)
            | CheckError::Rollback(..)
            | CheckError::Params(..) => None,
        }
    }
}
