//! Edges' and vertices' names, derived from the keys of the faces they
//! bound, never stored: an edge is [`Between`] the keys of its two faces,
//! a vertex the [`Meet`] of the keys of the faces round it, each with its
//! stored tag if any ([`EdgeName`], [`VertName`]).
//!
//! Several entities may share a name (the two edges a groove leaves
//! between one pair of faces); references tell them apart, the name
//! never does. Names read only keys, so they don't change when an
//! operation numbers its entities differently.

use crate::topo::brep::{Brep, EdgeId, VertId};

use super::keys::{EdgeTag, FaceKey, VertTag};

/// An edge's name: the keys of its two faces, lesser first in the leaf
/// order. Both may be one key (a seam, or an edge only one face uses).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Between {
    keys: [FaceKey; 2],
}

impl Between {
    /// The name of an edge between faces keyed `a` and `b`, either order.
    pub fn new(a: FaceKey, b: FaceKey) -> Between {
        Between {
            keys: if b < a { [b, a] } else { [a, b] },
        }
    }

    /// The two keys, lesser first.
    pub fn keys(&self) -> [FaceKey; 2] {
        self.keys
    }

    /// Whether both sides have one key.
    pub fn is_one_key(&self) -> bool {
        self.keys[0] == self.keys[1]
    }

    /// Whether `key` is one of the two.
    pub fn has(&self, key: &FaceKey) -> bool {
        self.keys.contains(key)
    }
}

/// A vertex's name: the keys of the faces round it, ascending in the
/// leaf order, each once.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Meet {
    keys: Vec<FaceKey>,
}

impl Meet {
    /// The name of a vertex round which faces keyed `keys` lie, any
    /// order, repeats counted once.
    pub fn new(keys: impl IntoIterator<Item = FaceKey>) -> Meet {
        let mut keys: Vec<FaceKey> = keys.into_iter().collect();
        keys.sort_unstable();
        keys.dedup();
        Meet { keys }
    }

    /// The name of a vertex where an edge named `edge` meets a face keyed
    /// `face` (an edge's end on a face): the edge's keys and the face's.
    pub fn edge_on_face(edge: Between, face: FaceKey) -> Meet {
        let [a, b] = edge.keys();
        Meet::new([a, b, face])
    }

    /// The keys, ascending, each once.
    pub fn keys(&self) -> &[FaceKey] {
        &self.keys
    }

    /// Whether `key` is one of them.
    pub fn has(&self, key: &FaceKey) -> bool {
        self.keys.binary_search(key).is_ok()
    }
}

/// An edge's full name: [`Between`] its faces' keys, and its tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeName {
    /// The keys of its faces.
    pub between: Between,
    /// Its tag.
    pub tag: Option<EdgeTag>,
}

/// A vertex's full name: the [`Meet`] of its faces' keys, and its tag.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VertName {
    /// The keys of the faces round it.
    pub meet: Meet,
    /// Its tag.
    pub tag: Option<VertTag>,
}

/// A name of a face, an edge or a vertex. Made only of keys, digests and
/// tags: no name holds a face's, an edge's or a vertex's id.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Name {
    /// A face's key.
    Face(FaceKey),
    /// An edge's name.
    Edge(EdgeName),
    /// A vertex's name.
    Vert(VertName),
}

/// The name of edge `e` of `body`: between the keys of the faces of its
/// coedges (one key twice for an edge one face uses: a pole's or apex's
/// side, a sheet's open boundary); `None` for an edge no coedge uses.
pub fn edge_name(body: &Brep, e: EdgeId) -> Option<EdgeName> {
    let c = body.edge_coedge(e);
    if c.is_none() {
        return None;
    }
    let p = body.pair(c);
    let k = *body.key(body.face(c));
    let l = if p.is_none() {
        k
    } else {
        *body.key(body.face(p))
    };
    Some(EdgeName {
        between: Between::new(k, l),
        tag: body.edge_tag(e),
    })
}

/// The names of all of `body`'s edges, by id ([`edge_name`]).
pub fn edge_names(body: &Brep) -> Vec<Option<EdgeName>> {
    (0..body.edge_count())
        .map(|e| edge_name(body, EdgeId(e as u32)))
        .collect()
}

/// The names of all of `body`'s vertices, by id: each the meet of the
/// keys of the faces whose loops pass through it, read from the coedges
/// starting there in one pass over them (a vertex no loop passes through
/// meets no keys), with its tag.
pub fn vert_names(body: &Brep) -> Vec<VertName> {
    let mut keys: Vec<Vec<FaceKey>> = vec![Vec::new(); body.vert_count()];
    for c in 0..body.coedge_count() {
        let c = crate::topo::brep::CoedgeId(c as u32);
        let v = body.coedge_vert(c, false);
        keys[v.index()].push(*body.key(body.face(c)));
    }
    keys.into_iter()
        .enumerate()
        .map(|(v, k)| VertName {
            meet: Meet::new(k),
            tag: body.vert_tag(VertId(v as u32)),
        })
        .collect()
}

#[cfg(test)]
mod tests;
