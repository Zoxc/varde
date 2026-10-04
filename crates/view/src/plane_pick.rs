//! Picking a plane for a sketch: an origin plane from the toolbar, or a
//! flat face of the model in the viewport, for a new sketch or for one
//! whose plane is changed.

use std::borrow::Cow;

use glam::DVec3;
use varde_document::{
    BodyId, Document, EdgeRef, FaceRef, FeatureId, FeatureKind, Operation, Plane, PointRef,
};
use varde_kernel::mesh::{FaceKey, PartKey};

use crate::document::CURVED_FACE;
use crate::pick::PickIndex;

/// What a plane is being picked for, and so which faces can take it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlanePick {
    /// The sketch whose plane is changed and its name; none for a new
    /// sketch.
    pub sketch: Option<(FeatureId, String)>,
    /// Why the sketch has to be put on another plane, if it does: why
    /// regenerating couldn't place it ("its face wasn't found").
    pub failed: Option<String>,
    /// How the faces that can take it are named: those of the document
    /// before the sketch if its plane is changed.
    naming: Naming,
}

/// Naming the faces and edges of the model shown as a feature of the
/// document stores them, with the history stopped at the feature: which
/// can be named (those of bodies made before it, named by features
/// before it), and by which body (see [`Naming::face_ref`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Naming {
    /// The bodies whose faces can be named, ascending: the document's
    /// made by a feature before the feature.
    bodies: Vec<BodyId>,
    /// The numbers of the features whose faces can be named, ascending:
    /// the document's before the feature.
    features: Vec<u64>,
    /// Each body a join merged into another and the body holding it, as
    /// the model shown has it, whose faces it shows as the holder's.
    merged: Vec<(BodyId, BodyId)>,
    /// The same with the history stopped at the feature: replayed from
    /// the joins and the combines using their tools up before it that
    /// worked, as regenerating merges.
    merged_before: Vec<(BodyId, BodyId)>,
    /// The body each extrude or revolve made, by the feature's number,
    /// ascending (a pattern's copy bodies are in `separated`).
    made: Vec<(u64, BodyId)>,
    /// The copies faces can be named as, ascending by [`FaceKey`]'s
    /// `instance`: none (0), and those the mirrors keeping their
    /// originals and the patterns before the feature make, of each
    /// other's too, with how each is made; `None` if
    /// they're past [`MAX_INSTANCES`], when any is taken. A later
    /// mirror's image or pattern's copy is shown when the feature isn't
    /// the last, but isn't there at the feature.
    instances: Option<Vec<Copied>>,
    /// The patterns before the feature whose copies are bodies of their
    /// own, in the document's order: a face copied by one is on a copy
    /// body, not on the body it was made on.
    separated: Vec<Separated>,
    /// The bodies a face each join, cut or intersect made may be on, as
    /// the model shown found, by the feature's number, ascending: those
    /// it touched, as they were then, or for a join only the first, which
    /// it merges the others into.
    touched: Vec<(u64, Vec<BodyId>)>,
    /// The new body of each split at or after the feature keeping both
    /// sides, and the body it splits, in the document's order: the model
    /// shown has a face of that body on the new body, which at the
    /// feature is still on the body.
    split_from: Vec<(BodyId, BodyId)>,
}

/// A copy faces can be named as: its instance ([`FaceKey`]'s), and how
/// it's made, the copy `index` the feature numbered `feature` makes of
/// faces of instance `parent`; none for the original's, instance 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Copied {
    instance: u64,
    from: Option<(u64, u64, u64)>,
}

/// A pattern whose copies are bodies of their own, as [`Naming`] follows
/// faces on to them: the feature's number, the body each body whose
/// faces it copies holds them in, by its index among the pattern's
/// bodies (its own, and those merged into it before the pattern), sorted
/// by body, and the copy bodies as the pattern lists them.
#[derive(Debug, Clone, PartialEq)]
struct Separated {
    feature: u64,
    sources: usize,
    holders: Vec<(BodyId, usize)>,
    copies: Vec<BodyId>,
}

impl Separated {
    /// The body copy `index` of the faces of `body` is on, if `body`'s
    /// faces are among those the pattern copies.
    fn copy(&self, body: BodyId, index: u64) -> Option<BodyId> {
        let at = (self.holders)
            .binary_search_by_key(&body, |&(body, _)| body)
            .ok()?;
        let source = self.holders[at].1;
        let at = usize::try_from(index.checked_sub(1)?)
            .ok()?
            .checked_mul(self.sources)?
            .checked_add(source)?;
        self.copies.get(at).copied()
    }
}

/// How many copies [`Naming`] tells apart at most: past it, faces of
/// any copy are taken (the mirrors' images, each doubling them, and the
/// patterns' copies, each multiplying them by their count, are bounded
/// by the document's features, not by this).
const MAX_INSTANCES: usize = 4096;

/// The copies (see `Naming::instances`) faces of `document`'s model can
/// be in with the history stopped at feature `before`: none's, each
/// image a mirror keeping its original makes of those before it, and
/// each copy a pattern makes of them. `None` past [`MAX_INSTANCES`].
fn instances_before(document: &Document, before: usize) -> Option<Vec<Copied>> {
    let mut instances = vec![Copied {
        instance: 0,
        from: None,
    }];
    for feature in &document.features()[..before] {
        // The copies it makes of each, by index.
        let copies = match &feature.kind {
            FeatureKind::Mirror(mirror) if mirror.keep_original => 1..2,
            // A checked pattern's count is in range.
            FeatureKind::Pattern(pattern) => 1..u64::from(pattern.count().unwrap_or(1)),
            _ => continue,
        };
        let copy = |instance: u64, index: u64| {
            let key = FaceKey {
                feature: 0,
                part: PartKey::StartCap,
                instance,
            };
            key.copy(feature.id.get(), index).instance
        };
        let mut images = Vec::new();
        for index in copies {
            images.extend(instances.iter().map(|copied| Copied {
                instance: copy(copied.instance, index),
                from: Some((copied.instance, feature.id.get(), index)),
            }));
            if instances.len().saturating_add(images.len()) > MAX_INSTANCES {
                return None;
            }
        }
        instances.extend(images);
    }
    // Stable: of two ways to one instance (which mixing makes all but
    // impossible), the first made is kept.
    instances.sort_by_key(|copied| copied.instance);
    instances.dedup_by_key(|copied| copied.instance);
    Some(instances)
}

/// Why a face or an edge of the model shown can't be named
/// ([`Naming::edge_ref`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unnamed {
    /// It isn't in the model shown's tables, or isn't between two faces.
    Missing,
    /// A face of it is made by the feature or a later one, or is on a
    /// body made by one.
    Later,
    /// Which body it's on where the history stops can't be told: its
    /// faces may be on more than one body there.
    Unclear,
}

/// What the model shown found that naming its faces needs, as
/// the app keeps it: each body a join merged into
/// another and the body holding it, the bodies each join, cut or
/// intersect touched (holders as they were then, in the order they were
/// made), and the features that failed.
#[derive(Debug, Clone, Copy, Default)]
pub struct Shown<'a> {
    pub merged: &'a [(BodyId, BodyId)],
    pub touched: &'a [(FeatureId, Vec<BodyId>)],
    pub failed: &'a [varde_regen::FeatureFailure],
}

impl PlanePick {
    /// Picking the plane for a new sketch of `document`, whose model
    /// shown found `shown`: any flat face of it takes it.
    pub fn new_sketch(document: &Document, shown: Shown) -> Self {
        Self {
            naming: Naming::before(document, document.features().len(), shown),
            ..Self::default()
        }
    }

    /// Picking another plane for the sketch feature `sketch` of
    /// `document`, whose model shown found `shown`, which
    /// `failed` to be placed if it says why: only faces of bodies made
    /// before it, named by features before it, as [`Document::check`]
    /// has a sketch's face, which also lets a face name a body or
    /// feature that's gone, but such a face is of a model shown from
    /// before an edit, so it's refused here. None if it isn't a sketch
    /// of `document`.
    pub fn change(
        document: &Document,
        sketch: FeatureId,
        failed: Option<String>,
        shown: Shown,
    ) -> Option<Self> {
        let index = (document.features().iter()).position(|feature| feature.id == sketch)?;
        let feature = &document.features()[index];
        if !matches!(feature.kind, varde_document::FeatureKind::Sketch { .. }) {
            return None;
        }
        Some(Self {
            sketch: Some((sketch, feature.name.clone())),
            failed,
            naming: Naming::before(document, index, shown),
        })
    }

    /// The reference to face `face` of `index`'s model picked at `near`,
    /// as a sketch on it stores it: see [`Naming::face_ref`].
    pub fn face_ref(&self, index: &PickIndex, face: u32, near: DVec3) -> Option<FaceRef> {
        self.naming.face_ref(index, face, near)
    }

    /// Why face `face` of `index`'s model can't take the sketch, if it
    /// can't: it isn't flat, or it came after the sketch, or it isn't
    /// the document's (a model shown from before an undo).
    pub fn refusal(&self, index: &PickIndex, face: u32) -> Option<Cow<'static, str>> {
        if index.face_placement(face).is_none() {
            return Some(CURVED_FACE.into());
        }
        let naming = &self.naming;
        let after =
            (index.face_ref(face, DVec3::ZERO)).is_some_and(|raw| !naming.takes_key(&raw.key));
        let taken = (!after)
            .then(|| naming.face_ref(index, face, DVec3::ZERO))
            .flatten()
            .map(|found| naming.takes_body(found.body));
        match (&self.sketch, taken) {
            (_, Some(true)) => None,
            (Some((_, name)), None) if !after && index.face_ref(face, DVec3::ZERO).is_some() => {
                Some(
                    format!("Which body that face is on at {name} can't be told: pick another")
                        .into(),
                )
            }
            (Some((_, name)), _) => {
                Some(format!("{name} can only go on a face made before it").into())
            }
            (None, _) => Some("That face isn't in the model".into()),
        }
    }

    /// Whether face `face` of `index`'s model can take the sketch.
    pub fn takes(&self, index: &PickIndex, face: u32) -> bool {
        self.refusal(index, face).is_none()
    }

    /// What the status bar asks for while picking.
    pub(crate) fn asking(&self) -> String {
        match (&self.sketch, &self.failed) {
            (None, _) => "Pick a plane or a flat face for the new sketch".to_owned(),
            (Some((_, name)), Some(why)) => format!("{name}: {why}. Pick a plane for it"),
            (Some((_, name)), None) => format!("Pick a plane or a flat face for {name}"),
        }
    }
}

impl Naming {
    /// Naming the faces and edges of `document`'s model shown, which
    /// found `shown`, as its feature `before` (or a feature added after
    /// its last, at its count) stores them: those named by its first
    /// `before` features, of bodies not made by a later one.
    pub fn before(document: &Document, before: usize, shown: Shown) -> Self {
        let later = &document.features()[before..];
        let mut features: Vec<u64> = (document.features()[..before].iter())
            .map(|feature| feature.id.get())
            .collect();
        features.sort_unstable();
        let mut bodies: Vec<BodyId> = (document.bodies().iter())
            .filter(|body| !later.iter().any(|feature| feature.id == body.created_by))
            .map(|body| body.id)
            .collect();
        bodies.sort_unstable();
        let join = |feature: FeatureId| {
            document
                .feature(feature)
                .is_some_and(|feature| matches!(feature.kind.operation(), Some(Operation::Join(_))))
        };
        let worked = |feature: FeatureId| !(shown.failed.iter()).any(|f| f.feature == feature);
        // Replayed in the document's order, as regenerating merges: a
        // working join merges the bodies the model shown found it touch,
        // a working combine using its tools up merges them into its
        // target.
        let mut merged_before = Vec::new();
        let mut separated = Vec::new();
        for feature in &document.features()[..before] {
            if let FeatureKind::Pattern(pattern) = &feature.kind
                && !pattern.joins()
            {
                // Its sources hold the faces of the bodies merged into
                // them so far.
                let mut holders: Vec<(BodyId, usize)> = (pattern.bodies.iter().enumerate())
                    .map(|(source, &body)| (body, source))
                    .collect();
                for &(consumed, holder) in &merged_before {
                    if let Ok(source) = pattern.bodies.binary_search(&holder) {
                        holders.push((consumed, source));
                    }
                }
                holders.sort_unstable();
                holders.dedup_by_key(|(body, _)| *body);
                let copies = pattern.copy_bodies().map(|(_, _, body)| body).collect();
                separated.push(Separated {
                    feature: feature.id.get(),
                    sources: pattern.bodies.len(),
                    holders,
                    copies,
                });
            }
            if !worked(feature.id) {
                continue;
            }
            match &feature.kind {
                FeatureKind::Combine(combine) if !combine.keep_tools => {
                    // The tools in the order they were made, as regen
                    // has them: only the target, first, holds.
                    let bodies: Vec<BodyId> = combine.bodies().collect();
                    varde_regen::note_merge(&mut merged_before, &bodies);
                }
                _ if join(feature.id) => {
                    let touched = (shown.touched.iter()).find(|(id, _)| *id == feature.id);
                    if let Some((_, bodies)) = touched {
                        varde_regen::note_merge(&mut merged_before, bodies);
                    }
                }
                _ => {}
            }
        }
        let mut touched: Vec<(u64, Vec<BodyId>)> = (shown.touched.iter())
            .map(|(feature, bodies)| {
                let on = if join(*feature) {
                    bodies.iter().copied().take(1).collect()
                } else {
                    bodies.clone()
                };
                (feature.get(), on)
            })
            .collect();
        touched.sort_unstable();
        let made_new = |body: &&varde_document::Body| {
            (document.feature(body.created_by))
                .is_some_and(|maker| maker.kind.new_body() == Some(body.id))
        };
        let mut made: Vec<(u64, BodyId)> = (document.bodies().iter())
            .filter(made_new)
            .map(|body| (body.created_by.get(), body.id))
            .collect();
        made.sort_unstable();
        let split_from = (later.iter())
            .filter_map(|feature| match &feature.kind {
                FeatureKind::Split(split) => Some((split.new_body?, split.body)),
                _ => None,
            })
            .collect();
        Self {
            split_from,
            bodies,
            features,
            merged: shown.merged.to_vec(),
            merged_before,
            made,
            touched,
            instances: instances_before(document, before),
            separated,
        }
    }

    /// Whether faces keyed `key` can be named: made by a feature before
    /// the feature ([`Naming::takes_maker`]), and none's copy or the copy
    /// of a mirror before it.
    pub(crate) fn takes_key(&self, key: &FaceKey) -> bool {
        self.takes_maker(key.feature) && self.copied(key.instance).is_some()
    }

    /// How faces of copy `instance` are made, if they can be named: a
    /// copy made before the feature, or any past [`MAX_INSTANCES`]
    /// (`Some(None)` then).
    fn copied(&self, instance: u64) -> Option<Option<&Copied>> {
        let Some(instances) = &self.instances else {
            return Some(None);
        };
        let at = (instances.binary_search_by_key(&instance, |copied| copied.instance)).ok()?;
        Some(Some(&instances[at]))
    }

    /// The body faces of copy `instance` first made on `body` are on,
    /// where the history stops: `body` itself, or, where a pattern with
    /// copies of their own copied them, its copy body, and so on through
    /// each such pattern the copy comes from. None where that can't be
    /// told (copies past [`MAX_INSTANCES`]) or the faces weren't on a
    /// body such a pattern copies.
    fn copy_body(&self, body: BodyId, instance: u64) -> Option<BodyId> {
        if self.separated.is_empty() || instance == 0 {
            return Some(body);
        }
        // The copies it comes from, the first made first.
        let mut steps = Vec::new();
        let mut at = instance;
        while at != 0 {
            let copied = self.copied(at)??;
            let (parent, feature, index) = copied.from?;
            steps.push((feature, index));
            at = parent;
            // Each step is to an instance made before: at most as many
            // steps as instances.
            if steps.len() > MAX_INSTANCES {
                return None;
            }
        }
        let mut body = body;
        for &(feature, index) in steps.iter().rev() {
            if let Some(pattern) = self.separated.iter().find(|p| p.feature == feature) {
                body = pattern.copy(body, index)?;
            }
        }
        Some(body)
    }

    /// Whether faces of `body` can be named: it's made before the
    /// feature.
    pub(crate) fn takes_body(&self, body: BodyId) -> bool {
        self.bodies.binary_search(&body).is_ok()
    }

    /// Whether faces the feature numbered `feature` made can be named:
    /// it comes before the feature.
    pub(crate) fn takes_maker(&self, feature: u64) -> bool {
        self.features.binary_search(&feature).is_ok()
    }

    /// The reference to face `face` of `index`'s model picked at `near`,
    /// as a feature stores it. The model shows it on the body holding it
    /// at the end of the history, but a join after the feature may have
    /// merged the body it's on there into that one: it's named by the
    /// body it was made on, which regenerating follows on to the body
    /// holding it wherever the feature is (see `Naming::body_of`). None
    /// if there's no such face, or if which body it's on there can't be
    /// told.
    pub fn face_ref(&self, index: &PickIndex, face: u32, near: DVec3) -> Option<FaceRef> {
        let mut found = index.face_ref(face, near)?;
        found.body = self.body_of(found.body, &found.key)?;
        Some(found)
    }

    /// The reference to face `face` of `index`'s model picked at `near`,
    /// as a feature stores it ([`Naming::face_ref`]), refused as
    /// [`Naming::edge_ref`] refuses an edge: not in the model's tables
    /// ([`Unnamed::Missing`]), named by a feature that isn't before the
    /// history's stop or on a body that isn't ([`Unnamed::Later`]), or
    /// which body it's on there can't be told ([`Unnamed::Unclear`]).
    pub fn checked_face_ref(
        &self,
        index: &PickIndex,
        face: u32,
        near: DVec3,
    ) -> Result<FaceRef, Unnamed> {
        let raw = index.face_ref(face, near).ok_or(Unnamed::Missing)?;
        if !self.takes_key(&raw.key) {
            return Err(Unnamed::Later);
        }
        let found = self.face_ref(index, face, near).ok_or(Unnamed::Unclear)?;
        if !self.takes_body(found.body) {
            return Err(Unnamed::Later);
        }
        Ok(found)
    }

    /// The body a face shown on `shown` and named by the feature numbered
    /// `maker` is on where the history stops: the body that feature made,
    /// or one the join, cut or intersect touched that's merged into the
    /// body shown; `shown` itself where neither is known. None if those
    /// are more than one body where the history stops, so which it's on
    /// there can't be told.
    fn body_of(&self, shown: BodyId, key: &FaceKey) -> Option<BodyId> {
        let shown = self.unsplit(shown);
        let maker = key.feature;
        let lookup = |list: &[(u64, BodyId)]| {
            (list.binary_search_by_key(&maker, |(feature, _)| *feature)).map(|at| list[at].1)
        };
        let made: Vec<BodyId> = match lookup(&self.made) {
            Ok(body) => vec![body],
            Err(_) => (self.touched)
                .binary_search_by_key(&maker, |(feature, _)| *feature)
                .map_or_else(|_| Vec::new(), |at| self.touched[at].1.clone()),
        };
        // On to the copy bodies of patterns that copied it: a body such
        // a pattern doesn't copy can't hold this copy.
        let mut on: Vec<BodyId> = if made.is_empty() || key.instance == 0 {
            made
        } else {
            let hopped: Vec<BodyId> = (made.iter())
                .filter_map(|&body| self.copy_body(body, key.instance))
                .collect();
            if hopped.is_empty() && self.copied(key.instance).is_some_and(|c| c.is_none()) {
                // Past the cap: which copy body can't be told.
                if !self.separated.is_empty() {
                    return None;
                }
            }
            hopped
        };
        on.retain(|&body| holder(&self.merged, body) == shown);
        let Some(&first) = on.first() else {
            return Some(shown);
        };
        let there = holder(&self.merged_before, first);
        if on
            .iter()
            .any(|&body| holder(&self.merged_before, body) != there)
        {
            return None;
        }
        Some(first)
    }

    /// The body `body` is part of where the history stops: the body a
    /// split at or after the feature cut it from, if it's that split's
    /// new body (and so on back through splits of splits), else itself.
    pub fn unsplit(&self, body: BodyId) -> BodyId {
        let mut body = body;
        // Each step goes to an earlier split: at most one per split.
        for _ in 0..self.split_from.len() {
            match self.split_from.iter().find(|(new, _)| *new == body) {
                Some(&(_, split)) => body = split,
                None => break,
            }
        }
        body
    }

    /// The reference to edge `edge` of `index`'s model picked at `near`,
    /// as a feature stores it ([`EdgeRef`]): the keys of its two faces,
    /// sorted, and the body its first face is on where the history stops
    /// (`Naming::body_of`). Refused if it isn't an edge between two
    /// faces of the model ([`Unnamed::Missing`]), a face of it is named
    /// by a feature that isn't before the history's stop or on a body
    /// that isn't ([`Unnamed::Later`]), or which body either face is on
    /// there can't be told, or they're on different bodies there, which
    /// a later join merged ([`Unnamed::Unclear`]).
    pub fn edge_ref(&self, index: &PickIndex, edge: u32, near: DVec3) -> Result<EdgeRef, Unnamed> {
        let faces = index.edge_faces(edge).ok_or(Unnamed::Missing)?;
        let shown = index.face_body(faces[0]).ok_or(Unnamed::Missing)?;
        let keys = index.chain_keys(edge).ok_or(Unnamed::Missing)?;
        if !keys.iter().all(|key| self.takes_key(key)) {
            return Err(Unnamed::Later);
        }
        let [a, b] = keys.map(|key| self.body_of(shown, &key));
        let (Some(a), Some(b)) = (a, b) else {
            return Err(Unnamed::Unclear);
        };
        if holder(&self.merged_before, a) != holder(&self.merged_before, b) {
            return Err(Unnamed::Unclear);
        }
        if !self.takes_body(a) {
            return Err(Unnamed::Later);
        }
        Ok(EdgeRef {
            body: a,
            faces: keys,
            near,
        })
    }
}

impl Naming {
    /// The reference to corner `corner` of `index`'s model (an index into
    /// its [`varde_regen::Picking::corners`]) as an align stores it: the
    /// keys of three faces meeting there, sorted, the body the first is
    /// on where the history stops (`Naming::body_of`), and the corner's
    /// point. Refused as [`Naming::edge_ref`] refuses an edge, and as
    /// [`Unnamed::Missing`] where two of the keys are the same, which
    /// doesn't name one corner.
    pub fn corner_ref(&self, index: &PickIndex, corner: u32) -> Result<PointRef, Unnamed> {
        let picking = index.picking();
        let found = picking
            .corners()
            .get(corner as usize)
            .ok_or(Unnamed::Missing)?;
        let shown = index.face_body(found.faces[0]).ok_or(Unnamed::Missing)?;
        let keys = picking.corner_keys(corner);
        if keys[0] == keys[1] || keys[1] == keys[2] {
            return Err(Unnamed::Missing);
        }
        if !keys.iter().all(|key| self.takes_key(key)) {
            return Err(Unnamed::Later);
        }
        let bodies = keys.map(|key| self.body_of(shown, &key));
        let [Some(body), Some(b), Some(c)] = bodies else {
            return Err(Unnamed::Unclear);
        };
        let there = holder(&self.merged_before, body);
        if holder(&self.merged_before, b) != there || holder(&self.merged_before, c) != there {
            return Err(Unnamed::Unclear);
        }
        if !self.takes_body(body) {
            return Err(Unnamed::Later);
        }
        Ok(PointRef::Corner {
            body,
            faces: keys,
            near: DVec3::from(found.point),
        })
    }
}

/// The body holding `body` by `merged`, a list like
/// [`varde_regen::Evaluation::merged`]: the one it was merged into, or
/// itself.
fn holder(merged: &[(BodyId, BodyId)], body: BodyId) -> BodyId {
    (merged.iter())
        .find(|(consumed, _)| *consumed == body)
        .map_or(body, |&(_, holder)| holder)
}

/// Where a sketch on `plane` is, as the Timeline notes it: "XY", or the
/// face by the feature that made it, "on Extrude 1's end", or by its body
/// if that feature is gone, "on Body 1".
pub fn plane_note(document: &Document, plane: &Plane) -> String {
    match plane.face() {
        Some(face) => format!("on {}", face_name(document, face)),
        None => plane.name().to_owned(),
    }
}

/// A face of the model as its reference `face` names it, for notes:
/// by the feature that made it, "Extrude 1's end", "a face of Revolve
/// 1", or by its body if that feature is gone, "Body 1", or "a face".
pub fn face_name(document: &Document, face: &FaceRef) -> String {
    if let Some(maker) = document.feature(face.maker()) {
        let part = match face.key.part {
            PartKey::StartCap => "start",
            PartKey::EndCap => "end",
            PartKey::Side { .. } => "side",
            _ => return format!("a face of {}", maker.name),
        };
        return format!("{}'s {part}", maker.name);
    }
    match document.body(face.body) {
        Some(body) => body.name.clone(),
        None => "a face".to_owned(),
    }
}

/// Where a sketch on `plane` is, for the status bar: "on XY", "on
/// Extrude 1's end".
pub(crate) fn on_plane(document: &Document, plane: &Plane) -> String {
    match plane {
        Plane::Origin(plane) => format!("on {}", plane.name()),
        Plane::Face(_) => plane_note(document, plane),
    }
}

#[cfg(test)]
mod tests {
    use varde_document::{
        Axis3, AxisRef, Copies, Editor, Mirror, OriginPlane, Pattern, PatternKind, PlaneRef,
        Targets,
    };
    use varde_expr::Value;

    use super::*;

    /// The image of a mirror keeping its original is named only with the
    /// history stopped after the mirror: before it, the model shown
    /// shows a face that isn't there yet. Images of images too.
    #[test]
    fn a_later_mirror_s_image_is_not_named_before_it() {
        let mut editor = Editor::new(Document::example());
        let plate = editor.document().bodies()[0].id;
        let mirror = Mirror {
            bodies: vec![plate],
            plane: PlaneRef::Origin(OriginPlane::XY),
            keep_original: true,
        };
        for _ in 0..2 {
            let add = editor.document().add_feature(mirror.clone().into());
            editor.apply(add).unwrap();
        }
        let document = editor.document();
        let [_, extrude, first, second] = [0, 1, 2, 3].map(|k| document.features()[k].id.get());
        let shown = Shown {
            merged: &[],
            touched: &[],
            failed: &[],
        };
        let top = FaceKey {
            feature: extrude,
            part: PartKey::EndCap,
            instance: 0,
        };
        let image = top.copy(first, 1);
        let twice = image.copy(second, 1);
        let naming = |before: usize| Naming::before(document, before, shown);
        assert!(naming(2).takes_key(&top));
        assert!(!naming(2).takes_key(&image));
        assert!(naming(3).takes_key(&image));
        assert!(!naming(3).takes_key(&twice));
        assert!(naming(4).takes_key(&twice));
        assert!(naming(4).takes_key(&top.copy(second, 1)));
        // A copy no mirror makes.
        assert!(!naming(4).takes_key(&top.copy(second, 2)));
    }

    /// A linear pattern of `body` along `axis`, `count` copies.
    fn row(document: &Document, body: varde_document::BodyId, axis: Axis3, count: &str) -> Pattern {
        let design = document.design();
        Pattern {
            bodies: vec![body],
            kind: PatternKind::Linear {
                along: AxisRef::Origin(axis),
                count: Value::new(count, &Pattern::count_ask(&design)).unwrap(),
                spacing: Value::new("100", &Pattern::spacing_ask(&design)).unwrap(),
            },
            copies: Default::default(),
        }
    }

    /// Copies of copies are named up to the cap: a pattern of a pattern
    /// 64 by 64 is 4096 copies, each named only after both, its last
    /// copy too; a mirror's image patterned, and a pattern mirrored, are
    /// named copy by copy. Past the cap any copy is taken, which
    /// regenerating then doesn't find if it isn't there yet.
    #[test]
    fn copies_of_copies_are_named_up_to_the_cap() {
        let shown = Shown {
            merged: &[],
            touched: &[],
            failed: &[],
        };
        let grid = |second: &str| {
            let mut editor = Editor::new(Document::example());
            let plate = editor.document().bodies()[0].id;
            for (axis, count) in [(Axis3::X, "64"), (Axis3::Y, second)] {
                let add = (editor.document())
                    .add_feature(row(editor.document(), plate, axis, count).into());
                editor.apply(add).unwrap();
            }
            editor.document().clone()
        };
        let document = grid("64");
        let [_, extrude, first, second] = [0, 1, 2, 3].map(|k| document.features()[k].id.get());
        let top = FaceKey {
            feature: extrude,
            part: PartKey::EndCap,
            instance: 0,
        };
        let last = top.copy(first, 63).copy(second, 63);
        let naming = |before: usize| Naming::before(&document, before, shown);
        assert_eq!(
            instances_before(&document, 4).map(|all| all.len()),
            Some(4096)
        );
        assert!(naming(4).takes_key(&last));
        assert!(naming(4).takes_key(&top.copy(second, 63)));
        assert!(!naming(4).takes_key(&top.copy(second, 64)));
        assert!(!naming(4).takes_key(&top.copy(first, 64)));
        assert!(naming(3).takes_key(&top.copy(first, 63)));
        assert!(!naming(3).takes_key(&last));
        assert!(!naming(3).takes_key(&top.copy(second, 1)));
        // One more row is past the cap.
        let past = grid("65");
        assert_eq!(instances_before(&past, 4), None);
        assert!(Naming::before(&past, 4, shown).takes_key(&top.copy(99, 1)));
        assert!(instances_before(&past, 3).is_some());

        // A mirror's image patterned, then the whole mirrored.
        let mut editor = Editor::new(Document::example());
        let plate = editor.document().bodies()[0].id;
        let mirror = Mirror {
            bodies: vec![plate],
            plane: PlaneRef::Origin(OriginPlane::XY),
            keep_original: true,
        };
        let add = editor.document().add_feature(mirror.clone().into());
        editor.apply(add).unwrap();
        let add =
            (editor.document()).add_feature(row(editor.document(), plate, Axis3::X, "1024").into());
        editor.apply(add).unwrap();
        for _ in 0..3 {
            let add = editor.document().add_feature(mirror.clone().into());
            editor.apply(add).unwrap();
        }
        let document = editor.document();
        let ids: Vec<u64> = document.features().iter().map(|f| f.id.get()).collect();
        let image = top.copy(ids[2], 1);
        let copied = image.copy(ids[3], 1023);
        let naming = |before: usize| Naming::before(document, before, shown);
        assert_eq!(
            instances_before(document, 4).map(|all| all.len()),
            Some(2048)
        );
        assert!(naming(4).takes_key(&copied));
        assert!(!naming(3).takes_key(&copied));
        assert!(!naming(4).takes_key(&copied.copy(ids[4], 1)));
        assert_eq!(
            instances_before(document, 5).map(|all| all.len()),
            Some(4096)
        );
        assert!(naming(5).takes_key(&copied.copy(ids[4], 1)));
        assert_eq!(instances_before(document, 6), None);
    }

    /// Faces copied by a pattern whose copies are bodies of their own are
    /// named on the copy body, even where a later join merged it into
    /// the original; copies of copies on the copy's copy body; a body
    /// joined into the original before the pattern has its faces copied
    /// on to the copy bodies too.
    #[test]
    fn faces_of_copy_bodies_are_named_on_them() {
        let mut editor = Editor::new(Document::example());
        let document = editor.document().clone();
        let plate = document.bodies()[0].id;
        let FeatureKind::Extrude(extrude) = document.features()[1].kind.clone() else {
            unreachable!()
        };
        let add = |editor: &mut Editor, kind: FeatureKind| {
            editor.apply(editor.document().add_feature(kind)).unwrap();
            editor.document().features().last().unwrap().id
        };
        // Another plate, joined into the first by a join touching both.
        let mut other = extrude.clone();
        other.operation = Operation::NewBody(varde_document::BodyId::NEW);
        let second = add(&mut editor, other.into());
        let other_body = editor.document().bodies()[1].id;
        let mut join = extrude.clone();
        join.operation = Operation::Join(Targets::default());
        let joined = add(&mut editor, join.into());
        let mut pattern = row(editor.document(), plate, Axis3::X, "3");
        pattern.copies = Copies::Separate(Vec::new());
        let first = add(&mut editor, pattern.into());
        let copies = |document: &Document, id: FeatureId| match &document.feature(id).unwrap().kind
        {
            FeatureKind::Pattern(pattern) => pattern.copy_bodies().map(|(_, _, b)| b).collect(),
            _ => Vec::<varde_document::BodyId>::new(),
        };
        let made = copies(editor.document(), first);
        assert_eq!(made.len(), 2);
        // The first copy body patterned again, unjoined.
        let mut again = row(editor.document(), made[0], Axis3::Y, "2");
        again.copies = Copies::Separate(Vec::new());
        let next = add(&mut editor, again.into());
        let twice = copies(editor.document(), next);
        let document = editor.document().clone();
        let touched = [(joined, vec![plate, other_body])];
        let shown = Shown {
            merged: &[(other_body, plate)],
            touched: &touched,
            failed: &[],
        };
        let top = |feature: FeatureId| FaceKey {
            feature: feature.get(),
            part: PartKey::EndCap,
            instance: 0,
        };
        let naming = Naming::before(&document, document.features().len(), shown);
        let original = top(document.features()[1].id);
        assert_eq!(naming.body_of(plate, &original), Some(plate));
        assert_eq!(
            naming.body_of(made[1], &original.copy(first.get(), 2)),
            Some(made[1])
        );
        // The other plate's faces, joined in before the pattern.
        let theirs = top(second).copy(first.get(), 1);
        assert_eq!(naming.body_of(made[0], &theirs), Some(made[0]));
        // Copies of copies.
        let both = original.copy(first.get(), 1).copy(next.get(), 1);
        assert_eq!(naming.body_of(twice[0], &both), Some(twice[0]));
        // A copy body merged into the plate at the end: named on the copy
        // body all the same, where the history stops before the merge.
        let merged = [(other_body, plate), (made[1], plate)];
        let shown = Shown {
            merged: &merged,
            ..shown
        };
        let naming = Naming::before(&document, document.features().len(), shown);
        assert_eq!(
            naming.body_of(plate, &original.copy(first.get(), 2)),
            Some(made[1])
        );
        assert!(naming.takes_body(made[1]));
        // Before the pattern, its copy bodies can't be named.
        let at = document
            .features()
            .iter()
            .position(|f| f.id == first)
            .unwrap();
        let naming = Naming::before(&document, at, shown);
        assert!(!naming.takes_body(made[0]));
        assert!(!naming.takes_key(&original.copy(first.get(), 1)));
    }

    /// Copies of a copy body, patterned on by joined patterns, its copy
    /// later merged into the plate: named on the copy body up to the cap;
    /// past it, which body a copy's face is on can't be told (none is
    /// guessed), while the plate's own faces are still named.
    #[test]
    fn copies_of_a_copy_body_are_named_up_to_the_cap_and_unclear_past_it() {
        let grid = |last: &str| {
            let mut editor = Editor::new(Document::example());
            let plate = editor.document().bodies()[0].id;
            let mut pattern = row(editor.document(), plate, Axis3::X, "2");
            pattern.copies = Copies::Separate(Vec::new());
            editor
                .apply(editor.document().add_feature(pattern.into()))
                .unwrap();
            let copy = editor.document().bodies()[1].id;
            for (axis, count) in [(Axis3::Y, "64"), (Axis3::Z, last)] {
                let add = (editor.document())
                    .add_feature(row(editor.document(), copy, axis, count).into());
                editor.apply(add).unwrap();
            }
            (editor.document().clone(), plate, copy)
        };
        for (last, clear) in [("32", true), ("33", false)] {
            let (document, plate, copy) = grid(last);
            let ids: Vec<u64> = document.features().iter().map(|f| f.id.get()).collect();
            let top = FaceKey {
                feature: ids[1],
                part: PartKey::EndCap,
                instance: 0,
            };
            let far = top.copy(ids[2], 1).copy(ids[3], 63).copy(ids[4], 1);
            // The copy body merged into the plate by a later join.
            let merged = [(copy, plate)];
            let shown = Shown {
                merged: &merged,
                touched: &[],
                failed: &[],
            };
            let naming = Naming::before(&document, document.features().len(), shown);
            assert_eq!(
                instances_before(&document, document.features().len()).is_some(),
                clear,
                "{last}"
            );
            assert!(naming.takes_key(&far), "{last}");
            let wanted = clear.then_some(copy);
            assert_eq!(naming.body_of(plate, &far), wanted, "{last}");
            assert_eq!(naming.body_of(plate, &top), Some(plate), "{last}");
        }
    }
}
