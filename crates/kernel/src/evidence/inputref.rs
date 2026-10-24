//! What evidence names input entities by ([`InputRef`]), and the items a
//! failure's evidence lists ([`Item`]).
//!
//! **An input entity** is named for whoever reads the evidence outside
//! the operation: by the leaf operand it is of (its place among the
//! operation's operands, [`Leaf`]) and the copy of its feature, by its
//! name (a face's key, an edge's or a vertex's derived name), which stays
//! the same when the model is rebuilt, and, for the operation's own
//! replay, by its id in that leaf and a point of it (its fingerprint).
//! The name and the point are read from the leaf's columns, guarded: an
//! entity of a body whose topology is broken (an operand failing its
//! check) is named as far as its links allow, never with a panic.
//!
//! The entities a record key holds ([`Named`]: an operand and an id) are
//! named so through the operation's leaves ([`ItemName::inputs`]), the
//! budget profile's exhausting key among them. Entities an operation
//! made inside itself (split or snapped edges, rim vertices, a batch
//! node's results) map to their leaves through the operation's own maps;
//! they are named here once mapped.

use varde_decide::ids::Entity;
use varde_decide::num::bounded::Bounded;
use varde_decide::records::keys::{Named, Operand};

use crate::evidence::budget_profile::ItemName;
use crate::infra::ops::InstanceId;
use crate::names::{Between, EdgeName, FaceKey, Meet, Name, VertName};
use crate::topo::brep::{Brep, CoedgeId, EdgeId, FaceId, VertId};

/// One leaf operand of an operation: a body as the caller gave it, its
/// place among the operation's operands (a boolean's first operand is 0,
/// its second 1; a batch's leaves in their order) and the copy of its
/// feature it is.
#[derive(Clone, Copy, Debug)]
pub struct Leaf<'a> {
    /// Its place among the operands.
    pub index: u32,
    /// The body.
    pub body: &'a Brep,
    /// The copy of its feature.
    pub instance: InstanceId,
}

impl<'a> Leaf<'a> {
    /// Leaf `index`, the original of its feature.
    pub fn new(index: u32, body: &'a Brep) -> Leaf<'a> {
        Leaf {
            index,
            body,
            instance: InstanceId::ORIGINAL,
        }
    }

    /// The leaf a record key's operand is of in a boolean: the first
    /// operand leaf 0, the second leaf 1.
    pub fn of_operand(operand: Operand) -> u32 {
        match operand {
            Operand::A => 0,
            Operand::B => 1,
        }
    }
}

/// An input entity, named (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct InputRef {
    /// The leaf operand it is of, by its place among the operands.
    pub leaf: u32,
    /// The copy of the leaf's feature.
    pub instance: InstanceId,
    /// Its name: a face's key; an edge's derived name (a coedge's is its
    /// edge's); a vertex's; a loop's is its face's key. `None` for a
    /// surface or a curve, which have none, and for an entity whose links
    /// name nothing.
    pub name: Option<Name>,
    /// Its id in the leaf.
    pub id: Entity,
    /// A point of it, in the leaf's coordinates: a vertex's position, an
    /// edge's (a coedge's) curve at the middle of its parameter range, a
    /// loop's first vertex, a face's (a surface's) surface at the centre
    /// of its lattice rectangle, a curve's middle; `None` where the body
    /// has none.
    pub point: Option<[f64; 3]>,
}

impl InputRef {
    /// Entity `id` of `leaf`, named (module docs).
    pub fn of(leaf: &Leaf<'_>, id: Entity) -> InputRef {
        let b = leaf.body;
        InputRef {
            leaf: leaf.index,
            instance: leaf.instance,
            name: name(b, id),
            id,
            point: point(b, id),
        }
    }

    /// Entity `id` of the leaf at `index` among `leaves`; unnamed and
    /// without a point when no leaf is there.
    pub fn among(leaves: &[Leaf<'_>], index: u32, id: Entity) -> InputRef {
        match leaves.iter().find(|l| l.index == index) {
            Some(leaf) => InputRef::of(leaf, id),
            None => InputRef {
                leaf: index,
                instance: InstanceId::ORIGINAL,
                name: None,
                id,
                point: None,
            },
        }
    }
}

impl ItemName {
    /// The input entities the item is about, named through the
    /// operation's `leaves` (a boolean's operands as leaves 0 and 1): the
    /// entities its record's key holds, the first operand's first. The
    /// budget profile's exhausting key is named so.
    pub fn inputs(&self, leaves: &[Leaf<'_>]) -> Vec<InputRef> {
        self.entities()
            .into_iter()
            .map(|Named { operand, entity }| {
                InputRef::among(leaves, Leaf::of_operand(operand), entity)
            })
            .collect()
    }
}

/// What an [`Item`] is to its failure.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemRole {
    /// The entity the failure is at: the failing check's item, the
    /// failing record's first entity.
    Failing,
    /// Another entity of the failing pair or record.
    Partner,
    /// An entity showing the failure: a witness of a broken invariant, a
    /// member of a failing shell or contact.
    Witness,
    /// A query point's entity.
    Query,
    /// An entity of the key at which the budget ran out.
    Exhausting,
    /// An input of a construction (a profile curve, a path), by name.
    Construction,
    /// A value given, with no entity (the tolerance refused).
    Value,
}

/// One item of a failure's evidence: the input entity it is about (none
/// for a value given, such as a tolerance), what it is to the failure,
/// the certified values read there and the key's text.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// The entity, by name; `None` for a value with no entity.
    pub subject: Option<InputRef>,
    /// What it is to the failure.
    pub role: ItemRole,
    /// The values read there (a row's value and error against its
    /// target, a gap's bounds): empty once the item is degraded to its
    /// name.
    pub values: Vec<Bounded<f64>>,
    /// The key or check it was found by, as text.
    pub key_text: String,
}

impl Item {
    /// An item about `subject` in `role`, found by `key_text`, without
    /// values.
    pub fn new(subject: Option<InputRef>, role: ItemRole, key_text: impl Into<String>) -> Item {
        Item {
            subject,
            role,
            values: Vec::new(),
            key_text: key_text.into(),
        }
    }

    /// The item with `values`.
    pub fn with_values(mut self, values: impl IntoIterator<Item = Bounded<f64>>) -> Item {
        self.values = values.into_iter().collect();
        self
    }
}

/// `v[i]`, if there is one.
fn get<T: Copy>(v: &[T], i: u32) -> Option<T> {
    v.get(i as usize).copied()
}

/// The key of face `f`, if it is one of `b`'s.
fn face_key(b: &Brep, f: FaceId) -> Option<FaceKey> {
    get(&b.faces.key, f.0)
}

/// The face of coedge `c`'s loop, if both are `b`'s.
fn coedge_face(b: &Brep, c: CoedgeId) -> Option<FaceId> {
    get(&b.coedges.face, c.0).filter(|f| f.index() < b.face_count())
}

/// The vertex coedge `c` starts at, if its edge is `b`'s.
fn coedge_start(b: &Brep, c: CoedgeId) -> Option<VertId> {
    let e = get(&b.coedges.edge, c.0)?;
    let reversed = get(&b.coedges.reversed, c.0)?;
    let ends = (b.edges.v_start.get(e.index()), b.edges.v_end.get(e.index()));
    match ends {
        (Some(&s), Some(&t)) => Some(if reversed { t } else { s }),
        _ => None,
    }
}

/// The name of entity `id` of `b` (module docs), guarded.
fn name(b: &Brep, id: Entity) -> Option<Name> {
    match id {
        Entity::Face(f) => face_key(b, f).map(Name::Face),
        Entity::Loop(l) => face_key(b, get(&b.loops.face, l.0)?).map(Name::Face),
        Entity::Edge(e) => edge_name(b, e).map(Name::Edge),
        Entity::Coedge(c) => edge_name(b, get(&b.coedges.edge, c.0)?).map(Name::Edge),
        Entity::Vert(v) => vert_name(b, v).map(Name::Vert),
        Entity::Surf(_) | Entity::Curve(_) => None,
    }
}

/// Edge `e`'s name: between the keys of its held coedge's face and its
/// pair's (one key twice without a pair), as
/// [`edge_name`](crate::names::edge_name), guarded.
fn edge_name(b: &Brep, e: EdgeId) -> Option<EdgeName> {
    let c = get(&b.edges.coedge, e.0)?;
    let k = face_key(b, coedge_face(b, c)?)?;
    let l = get(&b.coedges.pair, c.0)
        .filter(|p| !p.is_none())
        .and_then(|p| face_key(b, coedge_face(b, p)?))
        .unwrap_or(k);
    Some(EdgeName {
        between: Between::new(k, l),
        tag: get(&b.edges.tag, e.0)?,
    })
}

/// Vertex `v`'s name: the meet of the keys of the faces whose coedges
/// start there, as [`vert_names`](crate::names::vert_names), guarded:
/// one pass over the coedges.
fn vert_name(b: &Brep, v: VertId) -> Option<VertName> {
    let tag = get(&b.verts.tag, v.0)?;
    let keys = (0..b.coedge_count() as u32)
        .map(CoedgeId)
        .filter(|&c| coedge_start(b, c) == Some(v))
        .filter_map(|c| face_key(b, coedge_face(b, c)?));
    Some(VertName {
        meet: Meet::new(keys),
        tag,
    })
}

/// A point of entity `id` of `b` (module docs), guarded.
fn point(b: &Brep, id: Entity) -> Option<[f64; 3]> {
    let finite = |p: [f64; 3]| p.iter().all(|x| x.is_finite()).then_some(p);
    match id {
        Entity::Vert(v) => get(&b.verts.pos, v.0).and_then(finite),
        Entity::Edge(e) => edge_mid(b, e).and_then(finite),
        Entity::Coedge(c) => edge_mid(b, get(&b.coedges.edge, c.0)?).and_then(finite),
        Entity::Loop(l) => {
            let c = get(&b.loops.first_coedge, l.0)?;
            get(&b.verts.pos, coedge_start(b, c)?.0).and_then(finite)
        }
        Entity::Face(f) => surface_centre(b, get(&b.faces.surface, f.0)?.0).and_then(finite),
        Entity::Surf(s) => surface_centre(b, s.0).and_then(finite),
        Entity::Curve(c) => {
            let curve = b.curves3.get(c.index())?;
            let k = curve.knots();
            let (t0, t1) = (k.first()?, k.last()?);
            curve.point_at((t0 + t1) * 0.5).and_then(finite)
        }
    }
}

/// Edge `e`'s curve at the middle of its parameter range.
fn edge_mid(b: &Brep, e: EdgeId) -> Option<[f64; 3]> {
    let curve = b.curves3.get(get(&b.edges.curve, e.0)?.index())?;
    let [t0, t1] = get(&b.edges.t_range, e.0)?;
    curve.point_at((t0 + t1) * 0.5)
}

/// Surface `s` at the centre of its lattice rectangle.
fn surface_centre(b: &Brep, s: u32) -> Option<[f64; 3]> {
    let surface = b.surfaces.surface.get(s as usize)?;
    let r = surface.rect();
    let mid = |r: &std::ops::Range<i64>| (r.start as f64 + r.end as f64) * 0.5;
    surface.eval([mid(&r.u), mid(&r.v)]).map(|p| p.map(|x| x.v))
}

#[cfg(test)]
mod tests;
