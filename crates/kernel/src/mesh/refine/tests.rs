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

/// The cut of a 2 × 2 × 2 box (`[−1, 1]² × [0, 2]`) and a round hole
/// along `y` of radius `r` about `(x, z) = (0, z)`, all through.
fn drilled_box(r: f64, z: f64) -> crate::Solid {
    use crate::patch::Conic2;
    use crate::{Frame, Loop, Op, Profile, Segment, boolean, extrude};
    use glam::DVec2;
    let budget = Budget::DEFAULT;
    let q = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(x, y)| DVec2::new(x, y));
    let square = Loop {
        segments: (0..4)
            .map(|i| Segment::line(q[i], q[(i + 1) % 4], 4 + i as u64).unwrap())
            .collect(),
    };
    let profile = Profile {
        loops: vec![square],
    };
    let cube = extrude(&profile, &Frame::XY, 0.0, 2.0, 1, &TOL, &budget).unwrap();
    let c = DVec2::new(0.0, z);
    let p = [DVec2::X, DVec2::Y, DVec2::NEG_X, DVec2::NEG_Y].map(|d| c + d * r);
    let circle = Loop {
        segments: (0..4)
            .map(|i| Segment {
                conic: Conic2::arc_between(c, r, p[i], p[(i + 1) % 4]).unwrap(),
                curve: 1,
            })
            .collect(),
    };
    let frame = Frame {
        origin: DVec3::new(0.0, -1.0, 0.0),
        x: DVec3::X,
        y: DVec3::Z,
    };
    let profile = Profile {
        loops: vec![circle],
    };
    let hole = extrude(&profile, &frame, -3.6, 0.0, 6, &TOL, &budget).unwrap();
    boolean(&cube, &hole, Op::Difference, &TOL, &budget).unwrap()
}

#[test]
fn a_cap_triangle_whose_bisector_would_leave_it_is_split_red() {
    // A box drilled through along `y` (a user's design): its front cap
    // (`y = −1`) has the triangle from `(1, −1, 2)` to the rim at 44° and
    // the rim's seam vertex at 0°, whose arc side bulges into it, leaving
    // it a corner of 11° at the seam vertex. Its neighbour across the
    // straight side opposite that vertex split, it was bisected by a
    // straight edge from the seam vertex to that side's middle, 1.5°
    // outside the corner: a piece inside out, which repair refused. Now
    // it is split red.
    let (r, z) = (0.8110238395601597, 1.0155982131481562);
    let drilled = drilled_box(r, z);
    let mesh = drilled.mesh();
    let seam = DVec3::new(r, -1.0, z);
    let corner = DVec3::new(1.0, -1.0, 2.0);
    let at = |t: usize, q: DVec3| mesh.patch(t).p.iter().position(|&v| v.distance(q) < 1e-9);
    let t = (0..mesh.tris().len())
        .find(|&t| {
            let patch = mesh.patch(t);
            patch.p.iter().all(|v| v.y == -1.0)
                && at(t, seam).is_some()
                && at(t, corner).is_some()
                && patch.w.iter().any(|&w| w < 1.0)
        })
        .expect("the cap triangle");
    // Its neighbour across the side opposite the seam vertex.
    let corners = mesh.corners(t as u32);
    let o = at(t, seam).unwrap();
    let (x, y) = (corners[(o + 1) % 3], corners[(o + 2) % 3]);
    let n = (0..mesh.tris().len() as u32)
        .find(|&u| {
            let k = mesh.corners(u);
            (0..3).any(|i| k[i] == y && k[(i + 1) % 3] == x)
        })
        .expect("the neighbour");
    let side = (o + 1) % 3;
    assert!(straight_bisection_folds(&mesh.patch(t), side));
    let mut refiner = Refiner::new(mesh, TOL.resolution(), 0.0);
    split(&mut refiner, &[n]);
    assert!(refiner.leaves[t].is_none(), "the cap triangle is split");
    let pieces = refiner.pieces().unwrap();
    let folded: Vec<u32> = (pieces.iter())
        .filter(|p| p.patch.fold_direction().is_none())
        .map(|p| p.leaf)
        .collect();
    assert!(folded.is_empty(), "folded pieces of leaves {folded:?}");
    assert_eq!(refiner.mesh(&pieces).check(&TOL), Ok(()));
}

/// Whether bisecting the plane `patch` from the middle of its side `side`
/// to the opposite corner by a straight edge leaves a half failing the
/// fold check: built here from the patch, not by the refiner.
fn straight_bisection_folds(patch: &Patch, side: usize) -> bool {
    let [h0, h1] = patch.edge(side).split_half().unwrap();
    let (m, o) = (h0.p1, patch.p[(side + 2) % 3]);
    let (next, prev) = (patch.edge((side + 1) % 3), patch.edge((side + 2) % 3));
    let straight = (m + o) * 0.5;
    let first = Patch::new([h0.p0, m, o], [h0.c, straight, prev.c], [h0.w, 1.0, prev.w]);
    let second = Patch::new([m, h1.p1, o], [h1.c, next.c, straight], [h1.w, next.w, 1.0]);
    [first.unwrap(), second.unwrap()]
        .iter()
        .any(|half| half.fold_direction().is_none())
}

/// The tetrahedron on the triangle `base` in `z = 0`, clockwise from
/// above (its first side the arc `arc`, the others straight), and the
/// apex above its centroid, the base on the plane face. The base is
/// triangle 0 and the triangle across its side `i` is `i + 1`.
fn on_a_cap(base: [DVec3; 3], arc: crate::patch::Conic3) -> Mesh {
    let mut builder = MeshBuilder::new();
    let cap = builder.face(super::super::tests::face(
        1,
        Surface::Plane {
            n: DVec3::NEG_Z,
            d: 0.0,
        },
    ));
    let wall = super::super::tests::free(&mut builder);
    let v = base.map(|p| builder.vert(p));
    let apex = builder.vert((base[0] + base[1] + base[2]) / 3.0 + DVec3::Z);
    builder.edge(v[0], v[1], arc.c, arc.w);
    builder.tri(v, cap);
    for i in 0..3 {
        builder.tri([v[(i + 1) % 3], v[i], apex], wall);
    }
    builder.build().unwrap()
}

#[test]
#[allow(clippy::disallowed_methods, reason = "std maths to build inputs")]
fn cap_triangles_are_split_red_where_a_bisector_would_fold() {
    // Seeded triangles on a plane with one side an arc of the unit circle
    // bulging into them (a cap beside a hole's rim), passing the fold
    // check, given a hanging vertex on one side. A straight bisector from
    // an end of the arc leaves the triangle where the opposite side's
    // middle is past the arc's tangent there (about one in nine); the
    // triangle is then split red, and only then, and no piece of it is
    // inside out. One from the arc's middle never leaves it.
    use crate::patch::Conic3;
    use crate::test_rng::Rng;
    let mut rng = Rng::new(7);
    let (mut tried, mut red) = (0, 0);
    while tried < 3000 {
        let angle = rng.range(0.02, std::f64::consts::FRAC_PI_2);
        let o = DVec3::X;
        let a = DVec3::new(angle.cos(), angle.sin(), 0.0);
        let b = DVec3::new(rng.range(-2.0, 3.0), rng.range(-2.0, 3.0), 0.0);
        let arc = Conic3::arc_between(DVec3::ZERO, 1.0, o, a).unwrap();
        // The arc bulges out of the circle, to the right of `o → a`:
        // towards `b`, outside the circle, so `o, a, b` is clockwise.
        let right = |q: DVec3| (a - o).cross(q - o).z < 0.0;
        if b.length() <= 1.05 || !right(b) {
            continue;
        }
        let mesh = on_a_cap([o, a, b], arc);
        let base = mesh.patch(0);
        if base.fold_direction().is_none() {
            continue;
        }
        // Hanging on `a → b` (side 1, triangle 2), bisected from `o`; on
        // `b → o` (side 2, triangle 3), from `a`; or on the arc (side 0,
        // triangle 1), from `b`.
        for (side, from) in [(1, o), (2, a), (0, b)] {
            let folds = straight_bisection_folds(&base, side);
            if side == 0 {
                assert!(!folds, "angle {angle}, b {b}: a bisector from {from} folds");
            } else {
                tried += 1;
            }
            let middle = base.edge(side).split_half().unwrap()[0].p1;
            let mut refiner = Refiner::new(&mesh, TOL.resolution(), 0.0);
            split(&mut refiner, &[side as u32 + 1]);
            let pieces = refiner.pieces().unwrap();
            let mine: Vec<&Piece> = pieces.iter().filter(|p| p.face == 0).collect();
            for piece in &mine {
                assert!(
                    piece.patch.fold_direction().is_some(),
                    "angle {angle}, b {b}, from {from}: piece {:?} folds",
                    piece.patch.p
                );
            }
            assert_eq!(
                refiner.leaves[0].is_none(),
                folds,
                "angle {angle}, b {b}, from {from}"
            );
            let refined = refiner.mesh(&pieces);
            assert_eq!(
                refined.check_embedding(&TOL).err(),
                None,
                "angle {angle}, b {b}, from {from}"
            );
            if folds {
                // Split red: its four children, each a whole leaf.
                red += 1;
                let mut leaves: Vec<u32> = mine.iter().map(|p| p.leaf).collect();
                leaves.dedup();
                assert_eq!(leaves.len(), 4);
                assert_eq!(
                    mine.iter().filter(|p| p.patch.p.contains(&middle)).count(),
                    3
                );
            } else {
                // Bisected straight, from `from` to `middle`.
                assert_eq!(mine.len(), 2);
                assert!(mine.iter().all(|p| p.patch.p.contains(&middle)));
                assert!(mine.iter().all(|p| p.patch.p.contains(&from)));
            }
        }
    }
    println!("{red} of {tried} split red");
    assert!((200..=600).contains(&red), "{red} of {tried} split red");
}
