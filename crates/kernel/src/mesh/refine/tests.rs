use glam::DVec3;

use super::super::tests::{TOL, round_octahedron, tetrahedron};
use super::*;
use crate::Budget;

fn split(refiner: &mut Refiner, leaves: &[u32]) {
    refiner
        .split(leaves, &mut Work::new(&Budget::DEFAULT))
        .unwrap();
}

#[test]
fn a_red_split_bisects_its_neighbours() {
    let mesh = tetrahedron(DVec3::ZERO);
    let mut refiner = Refiner::new(&mesh, TOL.resolution(), 0.0);
    split(&mut refiner, &[0]);
    let pieces = refiner.pieces().unwrap();
    // Three green pairs, then the four red children.
    assert_eq!(pieces.len(), 10);
    let kids = mesh.patch(0).split4().unwrap();
    let red: Vec<Patch> = pieces
        .iter()
        .filter(|p| p.leaf >= 4)
        .map(|p| p.patch)
        .collect();
    assert_eq!(red, kids);
    for piece in &pieces {
        assert!(piece.changed);
        if piece.leaf < 4 {
            // Half of a neighbour, bisected at the edge it shares.
            let whole = mesh.patch(piece.leaf as usize);
            let halves: Vec<Patch> = (0..3)
                .filter_map(|i| whole.bisect(i, 0.5).ok())
                .flatten()
                .collect();
            assert!(halves.contains(&piece.patch));
        }
    }
    let refined = refiner.mesh(&pieces);
    assert_eq!(refined.check(&TOL), Ok(()));
    refiner.settle();
    assert!(refiner.pieces().unwrap().iter().all(|p| !p.changed));
}

#[test]
fn splitting_a_green_piece_splits_its_leaf() {
    let mesh = tetrahedron(DVec3::ZERO);
    let mut refiner = Refiner::new(&mesh, TOL.resolution(), 0.0);
    split(&mut refiner, &[0]);
    let green = refiner.pieces().unwrap()[0].clone();
    assert_eq!(green.leaf, 1);
    split(&mut refiner, &[green.leaf]);
    // Leaf 1 is red now, and 2 and 3, left with two hanging vertices
    // each, are too: every piece is a red child.
    let pieces = refiner.pieces().unwrap();
    assert_eq!(pieces.len(), 16);
    assert!(pieces.iter().all(|p| p.leaf >= 4));
    assert_eq!(refiner.mesh(&pieces).check(&TOL), Ok(()));
}

#[test]
fn refinement_stays_graded() {
    // Split, again and again, a piece at the octahedron's +x corner:
    // coarser neighbours are split first, so levels across an edge never
    // differ by more than one, and the pieces around grow in number only
    // with the depth.
    let mesh = round_octahedron(DVec3::ZERO);
    let mut refiner = Refiner::new(&mesh, TOL.resolution(), 0.0);
    let mut counts = Vec::new();
    for _ in 0..12 {
        let pieces = refiner.pieces().unwrap();
        let at_corner = pieces
            .iter()
            .filter(|p| p.corners.contains(&0))
            .map(|p| p.leaf)
            .max()
            .unwrap();
        split(&mut refiner, &[at_corner]);
        let pieces = refiner.pieces().unwrap();
        let refined = refiner.mesh(&pieces);
        assert_eq!(refined.check_topology(), Ok(()));
        assert!(pieces.iter().all(|p| p.patch.fold_direction().is_some()));
        counts.push(pieces.len());
    }
    let steps: Vec<usize> = counts.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(steps.iter().all(|&s| s <= 24), "{counts:?}");
}

#[test]
fn planar_leaves_split_with_straight_inner_edges() {
    let mesh = Mesh::cylinder(DVec3::ZERO, 1.0, 2.0, 1, &TOL).unwrap();
    // Triangle 2 is the first quarter of the bottom cap: centre, then its
    // arc from the rim at 90° to 0°.
    let cap = mesh.patch(2);
    let mut refiner = Refiner::new(&mesh, TOL.resolution(), 0.0);
    split(&mut refiner, &[2]);
    let pieces = refiner.pieces().unwrap();
    let arc = cap.edge(1).split_half().unwrap();
    for piece in pieces.iter().filter(|p| p.leaf >= 16) {
        for i in 0..3 {
            let edge = piece.patch.edge(i);
            if arc.contains(&edge) || arc.contains(&edge.reversed()) {
                continue;
            }
            // The other edges are the cap's straight sides or new straight
            // edges.
            assert_eq!(edge.w, 1.0);
            assert_eq!(edge.c, (edge.p0 + edge.p1) * 0.5);
        }
    }
    let refined = refiner.mesh(&pieces);
    assert_eq!(refined.check(&TOL), Ok(()));
}

#[test]
fn plane_tags_are_tested_before_a_straight_split() {
    // The tetrahedron's one face tagged as the plane z = 0, which only
    // triangle 0 is on. Triangle 0 splits (straight); its neighbours,
    // off the plane, fail when bisected by a straight green edge, naming
    // the first of them, and fail a red split themselves.
    let mut mesh = tetrahedron(DVec3::ZERO);
    mesh.faces[0].surface = Surface::Plane {
        n: DVec3::Z,
        d: 0.0,
    };
    let mut refiner = Refiner::new(&mesh, TOL.resolution(), 0.0);
    split(&mut refiner, &[0]);
    let result = refiner.pieces();
    assert_eq!(
        result.err(),
        Some(KernelError::Invalid(CheckError::Face(1)))
    );
    let result = refiner.split(&[2], &mut Work::new(&Budget::DEFAULT));
    assert_eq!(result, Err(KernelError::Invalid(CheckError::Face(2))));
}

#[test]
fn leaves_too_deep_or_too_small_are_not_split() {
    let mesh = tetrahedron(DVec3::ZERO);
    let mut refiner = Refiner::new(&mesh, TOL.resolution(), 2.0);
    let result = refiner.split(&[0], &mut Work::new(&Budget::DEFAULT));
    assert_eq!(result, Err(KernelError::TooComplex));

    let mut refiner = Refiner::new(&mesh, TOL.resolution(), 0.0);
    let mut leaf = 0;
    for _ in 0..MAX_REFINE_DEPTH {
        split(&mut refiner, &[leaf]);
        // The first red child keeps corner 0.
        leaf = refiner.leaves.len() as u32 - 4;
    }
    let result = refiner.split(&[leaf], &mut Work::new(&Budget::DEFAULT));
    assert_eq!(result, Err(KernelError::TooComplex));
}

#[test]
fn leaves_past_the_patch_limit_are_not_made() {
    // Every leaf is at least one piece, so a round with more leaves than
    // `MAX_PATCHES` fails anyway: the refiner stops as it gets there,
    // before making the rest of the round's leaves and their pieces.
    let mesh = round_octahedron(DVec3::ZERO);
    let mut refiner = Refiner::new(&mesh, TOL.resolution(), 0.0);
    assert_eq!(refiner.max_leaves, crate::MAX_PATCHES);
    refiner.max_leaves = 20;
    let mut work = Work::new(&Budget::DEFAULT);
    // 8 leaves, and 3 more a split: the fifth split would make 23.
    let all: Vec<u32> = (0..8).collect();
    assert_eq!(refiner.split(&all, &mut work), Err(KernelError::TooComplex));
    assert_eq!(refiner.leaves.iter().flatten().count(), 20);
}
