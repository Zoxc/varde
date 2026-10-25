//! The side graph and the regions derived from it: a primitive's two
//! shells, an unbalanced glue, a contact record's glue not built; the
//! regions a box with a cavity derives.

use super::*;
use crate::build::polyhedron;
use crate::build::primitives::{Primitive, build};
use crate::infra::budget::{Budget, Work};
use crate::infra::ops::{FeatureId, OpCtx, Tolerance};
use crate::topo::brep::{Contact, ContactCert, ContactKind, Member, VertId};

fn cuboid() -> Brep {
    let p = Primitive::Cuboid {
        min: [1.0, -2.0, 0.5],
        size: [3.0, 5.0, 6.75],
    };
    build(
        &p,
        OpCtx::new(FeatureId(1)),
        Tolerance::DEFAULT,
        &mut Work::new(&Budget::DEFAULT),
    )
    .expect("a box")
}

/// The side graph of `b` in one pass.
fn graph(b: &Brep) -> Result<Vec<Vec<(FaceId, Side)>>, Halt<RegionError>> {
    let pass = Pass {
        stage: "test",
        rung: 0,
    };
    let mut work = Work::new(&Budget::DEFAULT);
    Ok(work
        .pass(pass, &[()], |(), ledger| side_graph(b, ledger))?
        .remove(0))
}

/// A mesh: vertices and triangles.
type Mesh = (Vec<[f64; 3]>, Vec<[usize; 3]>);

/// The axis box `[lo, hi]` as a closed mesh, its triangles turning
/// counter-clockwise seen from outside, or from inside if `inward`.
fn box_mesh(lo: f64, hi: f64, inward: bool) -> Mesh {
    let verts = (0..8)
        .map(|i| [0, 1, 2].map(|k| if i >> k & 1 == 1 { hi } else { lo }))
        .collect();
    let quads = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    let tris = quads
        .iter()
        .flat_map(|&[a, b, c, d]| [[a, b, c], [a, c, d]])
        .map(|t: [usize; 3]| if inward { [t[0], t[2], t[1]] } else { t })
        .collect();
    (verts, tris)
}

/// The meshes as one, in pieces.
fn join(meshes: &[Mesh]) -> Mesh {
    let (mut verts, mut tris) = (Vec::new(), Vec::new());
    for (v, t) in meshes {
        let n = verts.len();
        verts.extend(v);
        tris.extend(t.iter().map(|t| t.map(|i| i + n)));
    }
    (verts, tris)
}

/// A 10 mm box with a 2 mm cavity in its middle.
pub(crate) fn cavity() -> Brep {
    let (verts, tris) = join(&[box_mesh(0.0, 10.0, false), box_mesh(4.0, 6.0, true)]);
    let ctx = OpCtx::new(FeatureId(1));
    let mut work = Work::new(&Budget::DEFAULT);
    polyhedron::build(&verts, &tris, ctx, Tolerance::DEFAULT, &mut work)
        .expect("a box with a cavity")
}

/// A box's sides make two shells: its faces' `Out` sides and their `In`
/// sides, in face order.
#[test]
fn a_box_has_two_side_shells() {
    let b = cuboid();
    let faces: Vec<FaceId> = (0..6).map(FaceId).collect();
    let side = |s| faces.iter().map(|&f| (f, s)).collect::<Vec<_>>();
    assert_eq!(graph(&b), Ok(vec![side(Side::Out), side(Side::In)]));
    let d = derive(&b, &mut Work::new(&Budget::DEFAULT)).expect("derived");
    assert_eq!(
        d.shells,
        [
            (side(Side::Out), ShellKind::Island),
            (side(Side::In), ShellKind::Outer)
        ]
    );
    assert_eq!(
        d.regions,
        [
            DerivedRegion {
                kind: RegionKind::Empty,
                outer: None,
                islands: vec![0],
                container: None,
            },
            DerivedRegion {
                kind: RegionKind::Material,
                outer: Some(1),
                islands: Vec::new(),
                container: None,
            },
        ]
    );
}

/// An edge whose two coedges run it the same way: the glue across it
/// isn't balanced.
#[test]
fn an_unbalanced_glue_is_found() {
    let mut b = cuboid();
    let e = EdgeId(3);
    let c = b.edge_coedge(e);
    let p = b.pair(c);
    b.coedges.reversed[p.index()] = b.reversed(c);
    assert_eq!(
        graph(&b),
        Err(Halt::Failed {
            item: 0,
            error: RegionError::Balance(e)
        })
    );
}

/// A contact record's sides are glued by its class: not built.
#[test]
fn a_contact_record_is_not_glued_yet() {
    let mut b = cuboid();
    b.contacts.push(Contact {
        kind: ContactKind::Vertex,
        members: vec![[Member::Vert(VertId(0)), Member::Vert(VertId(1))]],
        cert: ContactCert::default(),
    });
    assert_eq!(
        graph(&b),
        Err(Halt::Failed {
            item: 0,
            error: RegionError::Contact
        })
    );
}

/// The box with a cavity: the cavity's material side is an island of the
/// lump (one lump's box holds it), the box's outer sides an island of the
/// outside (the cavity's box leaves them out).
#[test]
fn a_cavity_is_a_void_inside_the_lump() {
    let b = cavity();
    let d = derive(&b, &mut Work::new(&Budget::DEFAULT)).expect("derived");
    let kinds: Vec<ShellKind> = d.shells.iter().map(|(_, k)| *k).collect();
    assert_eq!(
        kinds,
        [
            ShellKind::Island,
            ShellKind::Outer,
            ShellKind::Outer,
            ShellKind::Island
        ]
    );
    let region = |kind, outer, islands: &[usize]| DerivedRegion {
        kind,
        outer,
        islands: islands.to_vec(),
        container: None,
    };
    assert_eq!(
        d.regions,
        [
            region(RegionKind::Empty, None, &[0]),
            region(RegionKind::Material, Some(1), &[3]),
            region(RegionKind::Empty, Some(2), &[]),
        ]
    );
}
