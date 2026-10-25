//! The check's invariant 10, on names ([`Names`]): face keys, tags,
//! aliases and attributes.
//!
//! **Its checks**, in this order:
//! - on the faces: the digests a face's key holds resolve in the body's
//!   term table, each to a term of its part's kind (an offset's source
//!   face, a blend corner's edges; a blend chain's name is the caller's
//!   and needs none); its attribute set is valid;
//! - on the edges: no edge with one key, one surface and one winding on
//!   both sides is left, unless tagged or a contact record's member (an
//!   artefact the operation's tidying removes); an edge is tagged a seam
//!   exactly where it is of the same form between two faces of one key;
//!   its attribute set is valid;
//! - on the vertices: a pole (its kind) is tagged a pole or an apex, and
//!   ends a singular edge; a tag on a regular vertex is allowed only
//!   where its history or snap parent was a pole, which this build doesn't
//!   keep ([`Finding::Unbuilt`]); its attribute set is valid;
//! - on the body: its user topology ([`Topology`]) derived, every alias
//!   of a region differs from the region's primary and is the key of one
//!   of its faces. A key may still be one region's primary and another's
//!   alias.
//!
//! **Not checked here.** A tag's pole end (towards the key's axis
//! direction or away) is not compared with the form. The body keeps no
//! attribute sets yet, so the only valid set is none, and no provenance
//! classes, whose check against the edges' origins runs inside an
//! operation.
//!
//! **Work.** Every item is charged one unit, an edge one more per
//! coedge, a pole one more per edge ending there; the user topology as
//! its derivation charges it, then one unit per region's face and alias.
//!
//! **Reading a broken body.** A link naming no entity is passed over:
//! the earlier invariants refuse those.

use super::{Cx, Fault, Finding, Invariant, Item, ItemKind};
use crate::infra::budget::{Ledger, Step, Stop};
use crate::names::keys::{EdgeTag, FacePart, Term};
use crate::names::topology::{Region, Topology};
use crate::topo::brep::{AttrSetId, Brep, EdgeId, EdgeKind, FaceId, VertId, VertKind};

#[cfg(test)]
mod tests;

/// The checks of invariant 10, numbered in the hook's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum NameCheck {
    /// A digest of a face's key not resolving to a term of its kind.
    Digest = 1,
    /// A face's attribute set not valid.
    FaceAttrs,
    /// An edge of one key, one surface and one winding, untagged and no
    /// contact member.
    Artefact,
    /// A seam tag where the edge is not of the same form between faces
    /// of one key, or none where it is.
    Seam,
    /// An edge's attribute set not valid.
    EdgeAttrs,
    /// A pole without a pole's or an apex's tag.
    PoleTag,
    /// A pole ending no singular edge.
    PoleEdge,
    /// A tag on a regular vertex: its history's to allow, not kept.
    TagHistory,
    /// A vertex's attribute set not valid.
    VertAttrs,
    /// An alias equal to its region's primary.
    AliasPrimary,
    /// An alias that is the key of none of its region's faces.
    AliasFace,
}

type Checked = Result<(), Stop<Fault>>;

fn fault(check: NameCheck, finding: Finding) -> Checked {
    Err(Stop::Failed(Fault {
        check: check as u32,
        finding,
    }))
}

fn need(ok: bool, check: NameCheck) -> Checked {
    if ok {
        Ok(())
    } else {
        fault(check, Finding::Defect)
    }
}

/// Charges `n` units of work.
fn charge(ledger: &mut Ledger<'_>, n: usize) -> Checked {
    ledger.charge(Step::Work, u64::try_from(n).unwrap_or(u64::MAX))?;
    Ok(())
}

/// Whether `a` is a valid attribute set: none, the body keeping no sets
/// yet.
fn valid_attrs(a: AttrSetId) -> bool {
    a == AttrSetId::NONE
}

/// Invariant 10, names (module docs).
pub(crate) struct Names;

impl Invariant for Names {
    fn kinds(&self) -> &[ItemKind] {
        &[
            ItemKind::Face,
            ItemKind::Edge,
            ItemKind::Vert,
            ItemKind::Body,
        ]
    }

    fn check(&self, cx: &Cx<'_>, item: Item, ledger: &mut Ledger<'_>) -> Checked {
        charge(ledger, 1)?;
        match item {
            Item::Face(f) => face(cx.body(), f),
            Item::Edge(e) => edge(cx, e, ledger),
            Item::Vert(v) => vert(cx, v, ledger),
            Item::Body => body(cx, ledger),
            _ => Ok(()),
        }
    }
}

fn face(b: &Brep, f: FaceId) -> Checked {
    let terms = b.names().terms();
    let resolves = match b.key(f).part {
        FacePart::Offset { of } => matches!(terms.get(of), Some(Term::Face(_))),
        FacePart::BlendCorner { edges, .. } => matches!(terms.get(edges), Some(Term::Edges(_))),
        _ => true,
    };
    need(resolves, NameCheck::Digest)?;
    need(valid_attrs(b.face_attrs(f)), NameCheck::FaceAttrs)
}

fn edge(cx: &Cx<'_>, e: EdgeId, ledger: &mut Ledger<'_>) -> Checked {
    let b = cx.body();
    let coedges = &cx.inc.edge_coedges[e.index()];
    charge(ledger, coedges.len())?;
    let faces: Vec<FaceId> = coedges
        .iter()
        .map(|&c| b.face(c))
        .filter(|f| f.index() < b.face_count())
        .collect();
    let tagged = b.edge_tag(e).is_some();
    let one_key = match faces[..] {
        [f, g] if coedges.len() == 2 => b.key(f) == b.key(g),
        _ => false,
    };
    if let [f, g] = faces[..]
        && one_key
    {
        let artefact = b.face_surface_id(f) == b.face_surface_id(g)
            && b.flipped(f) == b.flipped(g)
            && !tagged
            && !cx.is_contact_member(e);
        need(!artefact, NameCheck::Artefact)?;
    }
    let same_form = matches!(b.edge_kind(e), EdgeKind::SameForm { .. });
    let seam = b.edge_tag(e) == Some(EdgeTag::Seam);
    need(seam == (same_form && one_key), NameCheck::Seam)?;
    need(valid_attrs(b.edge_attrs(e)), NameCheck::EdgeAttrs)
}

fn vert(cx: &Cx<'_>, v: VertId, ledger: &mut Ledger<'_>) -> Checked {
    let b = cx.body();
    let tagged = b.vert_tag(v).is_some();
    match b.vert_kind(v) {
        VertKind::Pole => {
            need(tagged, NameCheck::PoleTag)?;
            let edges = cx.vert_edges(v);
            charge(ledger, edges.len())?;
            let singular = edges.iter().any(|&e| b.edge_kind(e) == EdgeKind::Singular);
            need(singular, NameCheck::PoleEdge)?;
        }
        VertKind::Regular if tagged => return fault(NameCheck::TagHistory, Finding::Unbuilt),
        VertKind::Regular => {}
    }
    need(valid_attrs(b.vert_attrs(v)), NameCheck::VertAttrs)
}

fn body(cx: &Cx<'_>, ledger: &mut Ledger<'_>) -> Checked {
    let b = cx.body();
    let t = Topology::derive(b, &cx.inc.contact_edges, ledger)?;
    for r in t.regions() {
        charge(ledger, r.faces.len().saturating_add(r.aliases.len()))?;
        aliases(b, r)?;
    }
    Ok(())
}

/// Every alias of region `r` differs from its primary and is the key of
/// one of its faces.
pub(super) fn aliases(b: &Brep, r: &Region) -> Checked {
    need(
        r.aliases.iter().all(|a| *a != r.primary),
        NameCheck::AliasPrimary,
    )?;
    let mut keys: Vec<_> = r.faces.iter().map(|&f| *b.key(f)).collect();
    keys.sort_unstable();
    need(
        r.aliases.iter().all(|a| keys.binary_search(a).is_ok()),
        NameCheck::AliasFace,
    )
}
