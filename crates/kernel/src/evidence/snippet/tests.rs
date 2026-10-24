//! Faces cut out of the cube as sheets: copied bit for bit with the leaf
//! ids kept, open where a neighbour stays behind, joined into open
//! shells per part, naming only their own rows; within the face cap and
//! the allowance; refused, not panicking, on a leaf whose links name
//! nothing; and passing the check with their invariant left out.

use super::*;
use crate::evidence::tests::{CUBE_FACES, cube};
use crate::topo::brep::Brep;

/// The cube as leaf 0.
fn add(b: &Brep, faces: &[u32], except: Option<Inv>, units: u64) -> SheetSnippet {
    let leaf = Leaf::new(0, b);
    let mut s = SheetSnippet::default();
    let faces: Vec<FaceId> = faces.iter().map(|&f| FaceId(f)).collect();
    s.add(&leaf, &faces, except, &mut Allowance::new(units));
    s
}

#[test]
fn one_face_is_an_open_square() {
    let b = cube();
    let s = add(&b, &[3], Some(Inv::Seams), u64::MAX);
    assert_eq!((s.pieces.len(), s.truncated), (1, 0));
    let p = &s.pieces[0];
    let sh = &p.sheet;
    assert_eq!(sh.kind(), BodyKind::Sheet);
    assert_eq!((p.leaf, p.except), (0, Some(Inv::Seams)));
    assert_eq!(p.faces, vec![FaceId(3)]);
    assert_eq!(
        [sh.face_count(), sh.loop_count(), sh.coedge_count()],
        [1, 1, 4]
    );
    assert_eq!([sh.edge_count(), sh.vert_count()], [4, 4]);
    assert_eq!(sh.dangling(), None);
    // The leaf's corners, in id order, at the same positions.
    let corners: Vec<VertId> = {
        let mut v: Vec<u32> = CUBE_FACES[3].to_vec();
        v.sort_unstable();
        v.into_iter().map(VertId).collect()
    };
    assert_eq!(p.verts, corners);
    for (i, &v) in p.verts.iter().enumerate() {
        assert_eq!(
            sh.pos(VertId(i as u32)).map(f64::to_bits),
            b.pos(v).map(f64::to_bits)
        );
    }
    for (i, &e) in p.edges.iter().enumerate() {
        let new = EdgeId(i as u32);
        assert_eq!(sh.ends(new).map(|v| p.verts[v.index()]), b.ends(e));
        assert_eq!(sh.edge_curve(new), b.edge_curve(e));
    }
    assert_eq!(*sh.key(FaceId(0)), *b.key(FaceId(3)));
    // Every coedge an open boundary, the loop linked round.
    for c in 0..4 {
        let c = CoedgeId(c);
        assert!(sh.pair(c).is_none());
        assert_eq!(sh.coedge_vert(sh.next(c), false), sh.coedge_vert(c, true));
    }
    // Two open shells, in no region.
    assert_eq!([sh.shell_count(), sh.region_count()], [2, 0]);
    for side in [Side::Out, Side::In] {
        let shell = sh.face_shell(FaceId(0), side);
        assert_eq!(sh.shell_kind(shell), ShellKind::Open);
        assert_eq!(sh.shell_sides(shell), &[(FaceId(0), side)]);
    }
}

#[test]
fn neighbours_stay_paired_and_parts_get_their_own_shells() {
    let b = cube();
    let paired = |sh: &Brep| {
        (0..sh.coedge_count() as u32)
            .map(CoedgeId)
            .filter(|&c| !sh.pair(c).is_none())
            .count()
    };
    // Faces 0 (z = 0) and 2 (y = 0) share the edge from corner 0 to 1:
    // one part.
    let s = add(&b, &[2, 0, 2], None, u64::MAX);
    let p = &s.pieces[0];
    let sh = &p.sheet;
    assert_eq!(p.faces, vec![FaceId(0), FaceId(2)]);
    assert_eq!(sh.dangling(), None);
    assert_eq!(paired(sh), 2);
    assert_eq!(sh.edge_count(), 4 + 4 - 1);
    assert_eq!(sh.shell_count(), 2);
    let shell = |sh: &Brep, f: u32| sh.face_shell(FaceId(f), Side::Out);
    assert_eq!(shell(sh, 0), shell(sh, 1));
    // Faces 0 and 1 (z = 1) are opposite: two parts, each with an `Out`
    // and an `In` shell.
    let s = add(&b, &[0, 1], None, u64::MAX);
    let sh = &s.pieces[0].sheet;
    assert_eq!(paired(sh), 0);
    assert_eq!(sh.shell_count(), 4);
    assert_ne!(shell(sh, 0), shell(sh, 1));
    assert_eq!(sh.dangling(), None);
}

#[test]
fn faces_past_the_cap_or_the_allowance_are_counted() {
    let b = cube();
    let leaf = Leaf::new(0, &b);
    let mut s = SheetSnippet::default();
    let all: Vec<FaceId> = (0..6).map(FaceId).collect();
    let mut a = Allowance::new(u64::MAX);
    for _ in 0..3 {
        s.add(&leaf, &all, None, &mut a);
    }
    assert_eq!(s.face_count(), MAX_SNIPPET);
    assert_eq!(s.truncated as usize, 18 - MAX_SNIPPET);
    // The face, its loop and four coedges: six units.
    let none = add(&b, &[0], None, 5);
    assert!(none.is_empty());
    assert_eq!(none.truncated, 1);
    assert_eq!(add(&b, &[0], None, 6).face_count(), 1);
}

#[test]
fn a_leaf_whose_links_name_nothing_is_refused_not_panicking() {
    let mut b = cube();
    b.coedges.edge[0] = EdgeId(1000);
    b.faces.first_loop[1] = LoopId(77);
    b.verts.coedge[0] = CoedgeId::NONE;
    let face = b.coedges.face[0];
    let s = add(&b, &[face.0], Some(Inv::Topology), u64::MAX);
    assert!(s.is_empty());
    assert_eq!(s.truncated, 1);
    // The others cut, the dangling loop and the vertex's missing coedge
    // carried as they are.
    let s = add(&b, &[1], Some(Inv::Topology), u64::MAX);
    assert_eq!(s.pieces.len(), 1);
    assert!(s.pieces[0].sheet.first_loop(FaceId(0)).is_none());
    // Faces naming nothing are skipped.
    assert!(add(&b, &[40], None, u64::MAX).is_empty());
}

#[test]
fn snippets_compare_by_what_they_cut() {
    let (a, b) = (cube(), cube());
    assert_eq!(
        add(&a, &[1, 2], None, u64::MAX),
        add(&b, &[2, 1], None, u64::MAX)
    );
    assert_ne!(
        add(&a, &[1, 2], None, u64::MAX),
        add(&a, &[1], None, u64::MAX)
    );
    assert_ne!(
        add(&a, &[1], None, u64::MAX),
        add(&a, &[1], Some(Inv::Forms), u64::MAX)
    );
}

/// A snippet carries its failing invariant's defect and nothing else: it
/// passes the check with that invariant left out, and any other snippet
/// the full sheet check.
#[test]
#[ignore = "needs every invariant's hook in the check"]
fn snippets_pass_their_check() {
    let b = cube();
    let budget = Budget::DEFAULT;
    for faces in [&[0][..], &[0, 2], &[0, 1, 2, 3, 4]] {
        assert_eq!(add(&b, faces, None, u64::MAX).check(&budget), Ok(()));
        for inv in Inv::ALL {
            assert_eq!(add(&b, faces, Some(inv), u64::MAX).check(&budget), Ok(()));
        }
    }
}
