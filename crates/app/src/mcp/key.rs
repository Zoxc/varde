//! The opaque keys the MCP tools name faces, edges and corners by: the
//! postcard of `(DocId, Generation, the reference)` in base64url without
//! padding, so a key from another design, or read at another
//! generation, tells so. Keys come back from the network: [`decode`]
//! bounds the text before decoding it and takes nothing trailing; what
//! the reference names is for its user to check against the model,
//! as [`found`] does.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use glam::DVec3;
use serde::{Deserialize, Serialize};
use varde_document::{BodyId, EdgeRef, FaceKey, FaceRef, Generation};
use varde_mcp::tools::ItemKind;
use varde_view::{Pick, PickIndex, Picked, Snapped};

use crate::doc::{Doc, DocId};
use crate::words::with_article;

/// What a refused key's answer says after why.
const ASK_AGAIN: &str = "ask body_topology again for its keys";

/// The longest key text taken, in bytes: a corner's, the longest, is
/// about 110.
pub(crate) const MAX_TEXT: usize = 256;

/// What a key names.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) enum Ref {
    Face(FaceRef),
    Edge(EdgeRef),
    /// A corner, which no document type names: its body (its first
    /// face's), the keys of three of the faces meeting there, sorted, as
    /// `Picking::corner_keys` gives them, and its point.
    Corner {
        body: BodyId,
        faces: [FaceKey; 3],
        point: [f64; 3],
    },
}

/// A key, decoded.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) struct Key {
    /// The [`DocId`]'s number: see [`Key::is_of`].
    doc: u64,
    /// The generation of the model it was read from.
    pub(crate) generation: Generation,
    pub(crate) item: Ref,
}

impl Key {
    pub(crate) fn new(doc: DocId, generation: Generation, item: Ref) -> Self {
        Self {
            doc: doc.number(),
            generation,
            item,
        }
    }

    /// Whether it was given for the document `doc`.
    pub(crate) fn is_of(&self, doc: DocId) -> bool {
        self.doc == doc.number()
    }

    /// The key's text.
    pub(crate) fn encode(&self) -> String {
        // Of plain data into a vector: it can't fail.
        let bytes = postcard::to_allocvec(self).expect("a key serializes");
        URL_SAFE_NO_PAD.encode(bytes)
    }
}

/// Why a key's text wasn't one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BadKey {
    /// Longer than [`MAX_TEXT`].
    Long,
    /// Not base64url, or not a key, or with bytes after one.
    Malformed,
}

/// The key `text` is, bounded: see the module's docs.
pub(crate) fn decode(text: &str) -> Result<Key, BadKey> {
    if text.len() > MAX_TEXT {
        return Err(BadKey::Long);
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(text)
        .map_err(|_| BadKey::Malformed)?;
    match postcard::take_from_bytes::<Key>(&bytes) {
        Ok((key, [])) => Ok(key),
        _ => Err(BadKey::Malformed),
    }
}

/// A face, edge or corner a key named, found in the committed model.
pub(crate) struct Found {
    /// The committed model, ready for picking.
    pub(crate) index: PickIndex,
    /// It as a click on it picks it in `index`: a face or an edge at the
    /// key's point, a corner as its vertex with its snap point.
    pub(crate) pick: Pick,
    /// The reference the key carried.
    pub(crate) item: Ref,
}

/// An edge a key named, found in the committed model.
pub(crate) struct FoundEdge {
    /// The committed model, ready for picking.
    pub(crate) index: PickIndex,
    /// The edge's chain in `index`.
    pub(crate) edge: u32,
    /// The reference the key carried.
    pub(crate) reference: EdgeRef,
}

/// The edge the key `text` names in `doc`'s committed model, or why
/// not: see [`found`].
pub(crate) fn edge(doc: &Doc, text: &str) -> Result<FoundEdge, String> {
    let found = found(doc, text, ItemKind::Edge)?;
    match (found.item, found.pick.target) {
        (Ref::Edge(reference), Picked::Edge(edge)) => Ok(FoundEdge {
            index: found.index,
            edge,
            reference,
        }),
        _ => Err("internal error: an edge's key found something else".to_owned()),
    }
}

impl Ref {
    /// The kind of thing this names.
    fn kind(&self) -> ItemKind {
        match self {
            Ref::Face(_) => ItemKind::Face,
            Ref::Edge(_) => ItemKind::Edge,
            Ref::Corner { .. } => ItemKind::Corner,
        }
    }
}

/// The face, edge or corner (`kind`) the key `text` names in `doc`'s
/// committed model, or why not, worded for the client. A key from
/// another design, or naming another kind of thing, is refused.
/// Whatever generation it was read at, it's taken only if what it names
/// is in the model (found by its names, as regenerating finds a
/// reference) and passes through the point the key carries (within the
/// mesh's rounding and the design's resolution), as that point is
/// stored: a face, edge or corner a later edit moved, or whose faces are
/// gone, or a point off it (not a number, say: the key's text is the
/// client's) is refused, saying to ask again.
pub(crate) fn found(doc: &Doc, text: &str, kind: ItemKind) -> Result<Found, String> {
    let key = decode(text.trim()).map_err(|bad| match bad {
        BadKey::Long => format!("that key is too long to be one: {ASK_AGAIN}"),
        BadKey::Malformed => format!("that isn't a key Varde gave: {ASK_AGAIN}"),
    })?;
    if !key.is_of(doc.id) {
        return Err(format!("that key is of another design: {ASK_AGAIN}"));
    }
    if key.item.kind() != kind {
        let (names, not) = (
            with_article(key.item.kind().word()),
            with_article(kind.word()),
        );
        return Err(format!("that key names {names}, not {not}"));
    }
    let model = super::query::committed(doc)?;
    let index = PickIndex::new(model.mesh.clone(), model.picking.clone(), 0);
    let stale = || {
        format!(
            "that {} isn't in the model any more: {ASK_AGAIN}",
            kind.word()
        )
    };
    // The point goes into what the edit stores: checked on what it names
    // whatever the key's generation, as a key's text is the client's.
    let near = match key.item {
        Ref::Face(face) => face.near,
        Ref::Edge(edge) => edge.near,
        Ref::Corner { point, .. } => DVec3::from(point),
    };
    if !near.is_finite() || near.abs().max_element() > f64::from(varde_document::MAX_COORD) {
        return Err(stale());
    }
    let resolution = doc.editor.document().tolerance().resolution();
    let scale = near.abs().max_element().max(1.0);
    let slack = resolution + 16.0 * f64::from(f32::EPSILON) * scale;
    let (target, snap) = match key.item {
        Ref::Face(face) => {
            let found = (index.find_face(face.body, &face.key, near)).ok_or_else(stale)?;
            if index.face_distance(found, near) > slack {
                return Err(stale());
            }
            (Picked::Face(found), None)
        }
        Ref::Edge(edge) => {
            let found = (index.find_edge(edge.body, edge.faces, near)).ok_or_else(stale)?;
            if index.edge_distance(found, near) > slack {
                return Err(stale());
            }
            (Picked::Edge(found), None)
        }
        Ref::Corner { body, faces, .. } => {
            let vertex = (index.find_vertex(body, faces, near)).ok_or_else(stale)?;
            let corner = index.vertex_corner(vertex).ok_or_else(stale)?;
            let at = index
                .snap_point(Snapped::Corner(corner))
                .ok_or_else(stale)?;
            if at.distance(near) > slack {
                return Err(stale());
            }
            (Picked::Vertex(vertex), Some(Snapped::Corner(corner)))
        }
    };
    let body = index.body(target).ok_or_else(stale)?;
    let pick = Pick {
        model: index.model(),
        target,
        body,
        at: near,
        snap,
    };
    Ok(Found {
        index,
        pick,
        item: key.item,
    })
}
