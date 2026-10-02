//! Picking a plane for a sketch: an origin plane from the toolbar, or a
//! flat face of the model in the viewport, for a new sketch or for one
//! whose plane is changed.

use std::borrow::Cow;

use glam::DVec3;
use varde_document::{BodyId, Document, EdgeRef, FaceRef, FeatureId, Operation, Plane};
use varde_kernel::mesh::PartKey;

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
    /// The same with the history stopped at the sketch: replayed from
    /// the joins before it that worked, as regenerating merges.
    merged_before: Vec<(BodyId, BodyId)>,
    /// The body each feature made, by the feature's number, ascending.
    made: Vec<(u64, BodyId)>,
    /// The bodies a face each join, cut or intersect made may be on, as
    /// the model shown found, by the feature's number, ascending: those
    /// it touched, as they were then, or for a join only the first, which
    /// it merges the others into.
    touched: Vec<(u64, Vec<BodyId>)>,
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
        let after = (index.face_ref(face, DVec3::ZERO))
            .is_some_and(|raw| !naming.takes_maker(raw.key.feature));
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
        let mut merged_before = Vec::new();
        let mut touched = Vec::new();
        for (feature, bodies) in shown.touched {
            let at = document.features().iter().position(|f| f.id == *feature);
            let worked = !(shown.failed.iter()).any(|failed| failed.feature == *feature);
            if join(*feature) {
                if worked && at.is_some_and(|at| at < before) {
                    varde_regen::note_merge(&mut merged_before, bodies);
                }
                touched.push((feature.get(), bodies.iter().copied().take(1).collect()));
            } else {
                touched.push((feature.get(), bodies.clone()));
            }
        }
        touched.sort_unstable();
        let mut made: Vec<(u64, BodyId)> = (document.bodies().iter())
            .map(|body| (body.created_by.get(), body.id))
            .collect();
        made.sort_unstable();
        Self {
            bodies,
            features,
            merged: shown.merged.to_vec(),
            merged_before,
            made,
            touched,
        }
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
        found.body = self.body_of(found.body, found.key.feature)?;
        Some(found)
    }

    /// The body a face shown on `shown` and named by the feature numbered
    /// `maker` is on where the history stops: the body that feature made,
    /// or one the join, cut or intersect touched that's merged into the
    /// body shown; `shown` itself where neither is known. None if those
    /// are more than one body where the history stops, so which it's on
    /// there can't be told.
    fn body_of(&self, shown: BodyId, maker: u64) -> Option<BodyId> {
        let lookup = |list: &[(u64, BodyId)]| {
            (list.binary_search_by_key(&maker, |(feature, _)| *feature)).map(|at| list[at].1)
        };
        let mut on: Vec<BodyId> = match lookup(&self.made) {
            Ok(body) => vec![body],
            Err(_) => (self.touched)
                .binary_search_by_key(&maker, |(feature, _)| *feature)
                .map_or_else(|_| Vec::new(), |at| self.touched[at].1.clone()),
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
        if !keys.iter().all(|key| self.takes_maker(key.feature)) {
            return Err(Unnamed::Later);
        }
        let [a, b] = keys.map(|key| self.body_of(shown, key.feature));
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
    let Some(face) = plane.face() else {
        return plane.name().to_owned();
    };
    if let Some(maker) = document.feature(face.maker()) {
        let part = match face.key.part {
            PartKey::StartCap => "start",
            PartKey::EndCap => "end",
            PartKey::Side { .. } => "side",
            _ => return format!("on a face of {}", maker.name),
        };
        return format!("on {}'s {part}", maker.name);
    }
    match document.body(face.body) {
        Some(body) => format!("on {}", body.name),
        None => "on a face".to_owned(),
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
