//! Invariant 10 on the primitives and on flat sheets: each passes, a key
//! that is one region's primary and another's alias included, and each
//! defect seeded into one is caught by the check that names it (a digest
//! the term table doesn't hold or holds as another kind of term, a seam
//! tag missing or misplaced, an edge left of one key, surface and
//! winding, a pole untagged or ending no singular edge, an attribute set
//! the body doesn't keep, an alias equal to its primary or naming no face
//! of its region), or refused as not built (a tag on a regular vertex).

use super::*;
use crate::build::primitives::Primitive;
use crate::infra::budget::{Budget, Work};
use crate::names::derive::Between;
use crate::names::keys::{Digest, FaceKey, VertTag};
use crate::names::topology::tests::{between, edited, grooved, key, pair, primitive};
use crate::topo::brep::{Contact, ContactCert, ContactKind, Member};
use crate::topo::check::topo::tests::{Passing, failure};
use crate::topo::check::{CheckError, CheckFailure, Inv, Invariants, Scope, run};

/// Invariant 10, the others passing.
fn table() -> Invariants<'static> {
    let p: &dyn Invariant = &Passing;
    [p, p, p, p, p, p, p, p, p, &Names]
}

fn check_with(b: &Brep) -> Result<(), CheckError> {
    let work = &mut Work::new(&Budget::DEFAULT);
    run(b, Scope::Full, None, &table(), work, 0)
}

/// Invariant 10 on `b`, the same on one thread and on several.
fn check(b: &Brep) -> Result<(), CheckError> {
    #[cfg(not(target_arch = "wasm32"))]
    return crate::infra::par::assert_deterministic(|| check_with(b));
    #[cfg(target_arch = "wasm32")]
    return check_with(b);
}

/// The failure of `b`'s check: its kind of item, check and finding.
fn fails(b: &Brep) -> (ItemKind, NameCheck, Finding) {
    let f: CheckFailure = failure(check(b));
    assert_eq!(f.inv, Inv::Names);
    let check = [
        NameCheck::Digest,
        NameCheck::FaceAttrs,
        NameCheck::Artefact,
        NameCheck::Seam,
        NameCheck::EdgeAttrs,
        NameCheck::PoleTag,
        NameCheck::PoleEdge,
        NameCheck::TagHistory,
        NameCheck::VertAttrs,
        NameCheck::AliasPrimary,
        NameCheck::AliasFace,
    ]
    .into_iter()
    .find(|c| *c as u32 == f.check)
    .expect("a check of the hook");
    (f.item.kind(), check, f.finding)
}

fn cuboid() -> Brep {
    primitive(&Primitive::Cuboid {
        min: [1.0, -2.0, 0.5],
        size: [3.0, 5.0, 6.75],
    })
}

fn sphere() -> Brep {
    primitive(&Primitive::Sphere {
        centre: [10.0, -20.0, 5.0],
        radius: 7.0,
    })
}

/// One of each kind.
fn all() -> Vec<Brep> {
    let cone = |base, radii, height| Primitive::Cone {
        base,
        radii,
        height,
    };
    let torus = |major, minor| Primitive::Torus {
        centre: [3.0, 1.0, -2.0],
        major,
        minor,
    };
    let mut out = vec![
        cuboid(),
        sphere(),
        primitive(&Primitive::Cylinder {
            base: [1.5, -2.0, 3.0],
            radius: 4.0,
            height: 9.5,
        }),
    ];
    for p in [
        cone([1.0, 2.0, -3.0], [5.0, 0.0], 7.0),
        cone([1.0, 2.0, -3.0], [0.0, 5.0], 7.0),
        cone([-4.0, 0.25, 2.0], [3.0, 1.5], 2.5),
        torus(6.0, 2.5),
        torus(2.0, 3.0),
    ] {
        out.push(primitive(&p));
    }
    out
}

fn first_edge(b: &Brep, f: impl Fn(EdgeId) -> bool) -> EdgeId {
    (0..b.edge_count() as u32)
        .map(EdgeId)
        .find(|&e| f(e))
        .expect("an edge")
}

#[test]
fn the_primitives_pass() {
    for b in all() {
        assert_eq!(check(&b), Ok(()));
    }
}

#[test]
fn a_key_may_be_one_regions_primary_and_anothers_alias() {
    let (body, _) = grooved();
    assert_eq!(check(&body), Ok(()));
}

#[test]
fn digests_resolve_to_terms_of_their_kind() {
    let mut b = cuboid();
    let source = *b.key(FaceId(0));
    let wall = |of| FaceKey {
        part: FacePart::Offset { of },
        ..source
    };
    b.faces.key[1] = wall(source.digest());
    assert_eq!(
        fails(&b),
        (ItemKind::Face, NameCheck::Digest, Finding::Defect)
    );
    let of = b.names.terms.face(source).unwrap();
    assert_eq!(check(&b), Ok(()));
    // A corner's digest must stand for edges, not a face.
    b.faces.key[1] = FaceKey {
        part: FacePart::BlendCorner { edges: of, k: 0 },
        ..source
    };
    assert_eq!(
        fails(&b),
        (ItemKind::Face, NameCheck::Digest, Finding::Defect)
    );
    let edges = (b.names.terms)
        .corner([Between::new(source, wall(of))])
        .unwrap();
    b.faces.key[1] = FaceKey {
        part: FacePart::BlendCorner { edges, k: 0 },
        ..source
    };
    assert_eq!(check(&b), Ok(()));
    // A blend chain's name is the caller's: it needs no term.
    b.faces.key[1] = FaceKey {
        part: FacePart::Blend {
            chain: Digest::from_u64(77),
        },
        ..source
    };
    assert_eq!(check(&b), Ok(()));
}

#[test]
fn seam_tags_lie_exactly_on_same_form_edges_of_one_key() {
    let mut b = sphere();
    let seam = first_edge(&b, |e| b.edge_tag(e) == Some(EdgeTag::Seam));
    b.edges.tag[seam.index()] = None;
    assert_eq!(
        fails(&b),
        (ItemKind::Edge, NameCheck::Seam, Finding::Defect)
    );
    let mut b = cuboid();
    b.edges.tag[0] = Some(EdgeTag::Seam);
    assert_eq!(
        fails(&b),
        (ItemKind::Edge, NameCheck::Seam, Finding::Defect)
    );
    // Same form across two keys (an overlap) takes no tag.
    let (body, e) = pair(key(1), key(2));
    let body = edited(body, |b| {
        b.set_edge_kind(e, EdgeKind::SameForm { touch: false })
    });
    assert_eq!(check(&body), Ok(()));
}

#[test]
fn an_edge_of_one_key_surface_and_winding_is_left_only_tagged_or_in_contact() {
    let (body, e) = pair(key(1), key(1));
    assert_eq!(
        fails(&body),
        (ItemKind::Edge, NameCheck::Artefact, Finding::Defect)
    );
    let tagged = edited(body.clone(), |b| {
        b.set_edge_kind(e, EdgeKind::SameForm { touch: false });
        b.set_edge_tag(e, Some(EdgeTag::Seam));
    });
    assert_eq!(check(&tagged), Ok(()));
    let member = edited(body.clone(), |b| {
        b.add_contact(Contact {
            kind: ContactKind::Edge,
            members: vec![[Member::Edge(e), Member::Edge(e)]],
            cert: ContactCert::default(),
        });
    });
    assert_eq!(check(&member), Ok(()));
    // Flipped one way on one side, the other on the other: not one
    // winding.
    let mut flipped = body;
    let f = flipped.face(flipped.edge_coedge(e));
    flipped.faces.flipped[f.index()] = true;
    assert_eq!(check(&flipped), Ok(()));
}

#[test]
fn poles_are_tagged_and_end_singular_edges() {
    let mut b = sphere();
    let pole = VertId(
        (0..b.vert_count() as u32)
            .find(|&v| b.vert_kind(VertId(v)) == VertKind::Pole)
            .expect("a pole"),
    );
    let tag = b.verts.tag[pole.index()].take();
    assert_eq!(
        fails(&b),
        (ItemKind::Vert, NameCheck::PoleTag, Finding::Defect)
    );
    b.verts.tag[pole.index()] = tag;
    for e in 0..b.edge_count() {
        if b.edges.kind[e] == EdgeKind::Singular && b.ends(EdgeId(e as u32))[0] == pole {
            b.edges.kind[e] = EdgeKind::Regular;
        }
    }
    assert_eq!(
        fails(&b),
        (ItemKind::Vert, NameCheck::PoleEdge, Finding::Defect)
    );
}

#[test]
fn a_tag_on_a_regular_vertex_is_its_historys_to_allow() {
    let mut b = cuboid();
    b.verts.tag[3] = Some(VertTag::Apex);
    assert_eq!(
        fails(&b),
        (ItemKind::Vert, NameCheck::TagHistory, Finding::Unbuilt)
    );
}

#[test]
fn attribute_sets_the_body_does_not_keep_are_caught() {
    let mut b = cuboid();
    b.faces.attrs[2] = AttrSetId(0);
    assert_eq!(
        fails(&b),
        (ItemKind::Face, NameCheck::FaceAttrs, Finding::Defect)
    );
    let mut b = cuboid();
    b.edges.attrs[2] = AttrSetId(0);
    assert_eq!(
        fails(&b),
        (ItemKind::Edge, NameCheck::EdgeAttrs, Finding::Defect)
    );
    let mut b = cuboid();
    b.verts.attrs[2] = AttrSetId(0);
    assert_eq!(
        fails(&b),
        (ItemKind::Vert, NameCheck::VertAttrs, Finding::Defect)
    );
}

#[test]
fn an_alias_differs_from_its_primary_and_keys_a_face_of_its_region() {
    let (body, edges) = grooved();
    let half = between(&edges, [3, 0], [3, 1]);
    let faces = {
        let c = body.edge_coedge(half);
        let mut f = vec![body.face(c), body.face(body.pair(c))];
        f.sort_unstable();
        f
    };
    let region = |aliases: Vec<FaceKey>| Region {
        primary: key(2),
        aliases,
        faces: faces.clone(),
        flush: 0,
    };
    assert!(aliases(&body, &region(vec![key(5)])).is_ok());
    let failed = |r: Checked| match r {
        Err(Stop::Failed(f)) => f.check,
        _ => panic!("{r:?}"),
    };
    assert_eq!(
        failed(aliases(&body, &region(vec![key(2)]))),
        NameCheck::AliasPrimary as u32
    );
    assert_eq!(
        failed(aliases(&body, &region(vec![key(7)]))),
        NameCheck::AliasFace as u32
    );
}
