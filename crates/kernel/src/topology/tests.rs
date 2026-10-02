use glam::DVec3;

use super::*;
use crate::mesh::tests::TOL;
use crate::mesh::{CheckError, FaceName, FacePart, PartKey};
use crate::par::assert_deterministic;
use crate::{Budget, KernelError, Op, Tolerance, boolean};

fn key(feature: u64, part: PartKey) -> FaceKey {
    FaceKey {
        feature,
        part,
        instance: 0,
    }
}

const TOP: PartKey = PartKey::EndCap;
const BOTTOM: PartKey = PartKey::StartCap;

/// A box's side: 0 at `-y`, 1 at `+x`, 2 at `+y`, 3 at `-x`.
fn side(curve: u64) -> PartKey {
    PartKey::Side { curve }
}

fn cuboid(min: [f64; 3], size: [f64; 3], feature: u64, tol: &Tolerance) -> Solid {
    Solid::cuboid(DVec3::from(min), DVec3::from(size), feature, tol).unwrap()
}

fn op(a: &Solid, b: &Solid, op: Op, tol: &Tolerance) -> Solid {
    boolean(a, b, op, tol, &Budget::DEFAULT).unwrap()
}

/// Whether the patches of `region` come within `1e-9` of `x`.
fn reaches(solid: &Solid, topology: &Topology, region: u32, x: DVec3) -> bool {
    let tris = &topology.regions()[region as usize].tris;
    let patches = tris.iter().map(|&t| solid.mesh().patch(t as usize));
    distance::to_patches(x, patches, f64::INFINITY, &mut distance::Allowance::new()) <= 1e-9
}

/// The keys of chain `c`'s regions, sorted.
fn chain_keys(topology: &Topology, c: u32) -> [FaceKey; 2] {
    let [a, b] = topology.chains()[c as usize]
        .regions
        .map(|r| topology.regions()[r as usize].key);
    [a.min(b), a.max(b)]
}

/// What names a topology has: its regions' keys, its chains' pairs of
/// keys with whether they close, and its corners' keys, each sorted.
type Signature = (Vec<FaceKey>, Vec<([FaceKey; 2], bool)>, Vec<Vec<FaceKey>>);

fn signature(topology: &Topology) -> Signature {
    let mut regions: Vec<FaceKey> = topology.regions().iter().map(|r| r.key).collect();
    regions.sort();
    let mut chains: Vec<([FaceKey; 2], bool)> = (0..topology.chains().len() as u32)
        .map(|c| {
            (
                chain_keys(topology, c),
                topology.chains()[c as usize].closed,
            )
        })
        .collect();
    chains.sort();
    let mut corners: Vec<Vec<FaceKey>> = topology
        .corners()
        .iter()
        .map(|c| {
            let mut keys: Vec<FaceKey> = c
                .regions
                .iter()
                .map(|&r| topology.regions()[r as usize].key)
                .collect();
            keys.sort();
            keys
        })
        .collect();
    corners.sort();
    (regions, chains, corners)
}

#[test]
fn the_mix_is_fixed() {
    // Stored in files: these values must never change. Worked out
    // independently of this code, from the definition.
    assert_eq!(mix(&[]), 0);
    assert_eq!(mix(&[0]), 0x910a_2dec_8902_5cc1);
    assert_eq!(mix(&[1, 2, 3]), 0x25a1_5d91_607b_47d2);
    assert_eq!(mix(&[3, 2, 1]), 0x4f02_9006_f812_af6c);
    assert_eq!(mix(&[0, 0]), 0x6468_4c4f_0fd7_84b4);
    assert_eq!(key(7, side(3)).mixed(), 0x90bb_bddc_bee9_3480);
    // Order and length count.
    assert_ne!(mix(&[1, 2]), mix(&[2, 1]));
    assert_ne!(mix(&[0]), mix(&[0, 0]));
}

#[test]
fn derived_names_follow_their_parts() {
    let (a, b) = (key(1, TOP), key(2, side(5)));
    assert_eq!(blend_edge([a, b], 0), blend_edge([b, a], 0));
    assert_ne!(blend_edge([a, b], 0), blend_edge([a, b], 1));
    // A copy of a copy is unique and stable.
    let name = FaceName::new(1, FacePart::EndCap);
    let copy = name.copy(9, 2);
    assert_eq!(copy, name.copy(9, 2));
    assert_ne!(copy.instance, 0);
    assert_ne!(copy.copy(9, 2).instance, copy.instance);
    assert_ne!(name.copy(9, 1).instance, copy.instance);
    assert_eq!(copy.key().instance, copy.instance);
    // Pieces of one wall share a key; a loft's spans don't.
    let piece = |segment| FaceName::new(1, FacePart::Side { curve: 4, segment });
    assert_eq!(piece(0).key(), piece(3).key());
    let span = |span| {
        FaceName::new(
            1,
            FacePart::Lofted {
                curve: 4,
                span,
                segment: 0,
            },
        )
    };
    assert_ne!(span(0).key(), span(1).key());
}

#[test]
fn a_box_has_six_faces_twelve_edges_and_eight_corners() {
    let solid = cuboid([0.0; 3], [1.0, 2.0, 3.0], 1, &TOL);
    let topology = solid.topology();
    assert_eq!(topology.regions().len(), 6);
    assert_eq!(topology.chains().len(), 12);
    assert!(
        topology
            .chains()
            .iter()
            .all(|c| !c.closed && c.halfedges.len() == 1)
    );
    assert_eq!(topology.corners().len(), 8);
    assert!(topology.corners().iter().all(|c| c.regions.len() == 3));
    // The edge between the top and the front, and the corner at the top
    // front left.
    let edge = topology
        .edge(&solid, [key(1, side(0)), key(1, TOP)], DVec3::ZERO)
        .unwrap();
    let h = topology.chains()[edge as usize].halfedges[0];
    let ends = [solid.mesh().halfedge(h).start, solid.mesh().end(h)];
    let mut ends = ends.map(|v| solid.mesh().verts()[v as usize].to_array());
    ends.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(ends, [[0.0, 0.0, 3.0], [1.0, 0.0, 3.0]]);
    let corner = topology
        .corner(
            &solid,
            [key(1, TOP), key(1, side(0)), key(1, side(3))],
            DVec3::ZERO,
        )
        .unwrap();
    let vertex = topology.corners()[corner as usize].vertex;
    assert_eq!(
        solid.mesh().verts()[vertex as usize],
        DVec3::new(0.0, 0.0, 3.0)
    );
}

#[test]
fn a_cylinder_is_three_faces_with_closed_rims() {
    let solid = Solid::cylinder(DVec3::ZERO, 2.0, 1.0, 4, &TOL).unwrap();
    let topology = solid.topology();
    let mut keys: Vec<FaceKey> = topology.regions().iter().map(|r| r.key).collect();
    keys.sort();
    assert_eq!(keys, [key(4, BOTTOM), key(4, TOP), key(4, side(0))]);
    assert_eq!(topology.chains().len(), 2);
    for chain in topology.chains() {
        assert!(chain.closed);
        assert_eq!(chain.halfedges.len(), 4);
        // End to end, round the rim.
        let mesh = solid.mesh();
        for (i, &h) in chain.halfedges.iter().enumerate() {
            let next = chain.halfedges[(i + 1) % 4];
            assert_eq!(mesh.end(h), mesh.halfedge(next).start);
        }
    }
    assert!(topology.corners().is_empty());
    let rim = topology
        .edge(
            &solid,
            [key(4, side(0)), key(4, TOP)],
            DVec3::new(0.0, 0.0, 1.0),
        )
        .unwrap();
    assert_eq!(chain_keys(&topology, rim), [key(4, TOP), key(4, side(0))]);
}

/// A plate with a hole through it and a pocket cut in from its top
/// edge, `scale` times as large.
fn drilled(scale: f64, tol: &Tolerance) -> Solid {
    let s = |v: [f64; 3]| v.map(|x| x * scale);
    let plate = cuboid(s([0.0, 0.0, 0.0]), s([10.0, 6.0, 2.0]), 1, tol);
    let hole = Solid::cylinder(
        DVec3::from(s([3.0, 3.0, -1.0])),
        1.5 * scale,
        4.0 * scale,
        2,
        tol,
    )
    .unwrap();
    let pocket = cuboid(s([6.0, -1.0, 1.0]), s([2.0, 4.0, 2.0]), 3, tol);
    let drilled = op(&plate, &hole, Op::Difference, tol);
    op(&drilled, &pocket, Op::Difference, tol)
}

#[test]
fn names_are_the_same_whatever_the_dimensions_and_tolerance() {
    let fine = Tolerance::new(1e-4).unwrap();
    let solids = [
        drilled(1.0, &TOL),
        drilled(1.37, &TOL),
        drilled(1.0, &fine),
        drilled(0.61, &fine),
    ];
    let topologies: Vec<Topology> = solids.iter().map(Solid::topology).collect();
    let first = signature(&topologies[0]);
    // The plate's six faces (its front split by nothing: the pocket
    // starts at its front edge but stays inside it), the hole's wall and
    // the pocket's floor and three walls.
    assert_eq!(first.0.len(), 11, "{:?}", first.0);
    // The hole's rims close; the rest end at corners.
    let closed: Vec<[FaceKey; 2]> = first.1.iter().filter(|c| c.1).map(|c| c.0).collect();
    assert_eq!(
        closed,
        [
            [key(1, BOTTOM), key(2, side(0))],
            [key(1, TOP), key(2, side(0))]
        ]
    );
    assert!(first.1.iter().filter(|c| !c.1).count() >= 12);
    assert!(!first.2.is_empty());
    for topology in &topologies[1..] {
        assert_eq!(signature(topology), first);
    }
    // Every reference made on the first resolves on the others to the
    // same names, its point scaled with the part.
    let scales = [1.0, 1.37, 1.0, 0.61];
    for c in 0..topologies[0].chains().len() as u32 {
        let h = topologies[0].chains()[c as usize].halfedges[0];
        let near = solids[0].mesh().verts()[solids[0].mesh().halfedge(h).start as usize];
        let keys = chain_keys(&topologies[0], c);
        for (i, topology) in topologies.iter().enumerate() {
            let found = topology.edge(&solids[i], keys, near * scales[i]).unwrap();
            assert_eq!(chain_keys(topology, found), keys);
        }
    }
}

/// Two boxes of feature 1 apart, as copies would be, and a groove of
/// feature 2 across the first one's top: the top's key names three
/// regions, the edge between the top and the front three chains, and the
/// corner of the top, front and left two corners.
fn two_boxes() -> Solid {
    let left = cuboid([0.0, 0.0, 0.0], [10.0, 4.0, 2.0], 1, &TOL);
    let right = cuboid([20.0, 0.0, 0.0], [10.0, 4.0, 2.0], 1, &TOL);
    let groove = cuboid([4.0, -1.0, 1.0], [2.0, 6.0, 2.0], 2, &TOL);
    let both = op(&left, &right, Op::Union, &TOL);
    op(&both, &groove, Op::Difference, &TOL)
}

#[test]
fn several_regions_or_chains_of_one_name_go_by_the_nearest() {
    let solid = two_boxes();
    let topology = solid.topology();
    let tops: Vec<u32> = (0..topology.regions().len() as u32)
        .filter(|&r| topology.regions()[r as usize].key == key(1, TOP))
        .collect();
    assert_eq!(tops.len(), 3);
    for near in [
        [1.0, 2.0, 2.5],
        [8.0, 1.0, 2.0],
        [25.0, 3.0, 2.1],
        [14.0, 2.0, 2.0],
    ] {
        let near = DVec3::from(near);
        let found = topology.face(&solid, &key(1, TOP), near).unwrap();
        // The nearest: the box's own, and from the gap the left box's
        // right part (4 from it, 6 from the other box's).
        let x = near.x.clamp(0.0, 30.0);
        let on = DVec3::new(if x > 10.0 && x < 20.0 { 10.0 } else { x }, near.y, 2.0);
        assert!(reaches(&solid, &topology, found, on), "{near}");
    }
    let edges: Vec<u32> = (0..topology.chains().len() as u32)
        .filter(|&c| chain_keys(&topology, c) == [key(1, TOP), key(1, side(0))])
        .collect();
    assert_eq!(edges.len(), 3);
    for (near, x) in [(1.0, 2.0), (9.0, 8.0), (29.0, 25.0)] {
        let found = topology
            .edge(
                &solid,
                [key(1, TOP), key(1, side(0))],
                DVec3::new(near, -1.0, 3.0),
            )
            .unwrap();
        let curves = topology.chains()[found as usize]
            .halfedges
            .iter()
            .map(|&h| solid.mesh().curve(h));
        let d = distance::to_curves(
            DVec3::new(x, 0.0, 2.0),
            curves,
            f64::INFINITY,
            &mut distance::Allowance::new(),
        );
        assert!(d <= 1e-12, "{near}: {d}");
    }
    let corner = [key(1, TOP), key(1, side(0)), key(1, side(3))];
    for x in [0.0, 20.0] {
        let found = topology
            .corner(&solid, corner, DVec3::new(x + 1.0, 0.0, 2.0))
            .unwrap();
        let vertex = topology.corners()[found as usize].vertex;
        assert_eq!(
            solid.mesh().verts()[vertex as usize],
            DVec3::new(x, 0.0, 2.0)
        );
    }
}

#[test]
fn one_region_or_chain_of_a_name_is_taken_wherever_the_point_is() {
    let solid = two_boxes();
    let topology = solid.topology();
    // The groove's floor is one region, its walls one each.
    let far = DVec3::splat(1e6);
    let floor = topology.face(&solid, &key(2, BOTTOM), far).unwrap();
    assert!(reaches(&solid, &topology, floor, DVec3::new(5.0, 2.0, 1.0)));
    let nan = DVec3::splat(f64::NAN);
    assert_eq!(topology.face(&solid, &key(2, BOTTOM), nan), Ok(floor));
    let wall = topology
        .edge(&solid, [key(2, BOTTOM), key(2, side(1))], far)
        .unwrap();
    assert_eq!(
        chain_keys(&topology, wall),
        [key(2, BOTTOM), key(2, side(1))]
    );
    // The groove's wall at `+x` ends where it meets the floor and the
    // front.
    let corner = [key(2, BOTTOM), key(2, side(1)), key(1, side(0))];
    let found = topology.corner(&solid, corner, far).unwrap();
    let vertex = topology.corners()[found as usize].vertex;
    assert_eq!(
        solid.mesh().verts()[vertex as usize],
        DVec3::new(6.0, 0.0, 1.0)
    );
}

#[test]
fn names_that_aren_t_there_aren_t_found() {
    let solid = two_boxes();
    let topology = solid.topology();
    let near = DVec3::ZERO;
    assert_eq!(
        topology.face(&solid, &key(3, TOP), near),
        Err(NotFound::Face)
    );
    // Faces that don't meet.
    let apart = [key(1, TOP), key(1, BOTTOM)];
    assert_eq!(topology.edge(&solid, apart, near), Err(NotFound::Edge));
    let corner = [key(1, TOP), key(1, BOTTOM), key(1, side(0))];
    assert_eq!(topology.corner(&solid, corner, near), Err(NotFound::Corner));
    // A key twice, or three times: every corner of the top has it, but
    // it names one face of the three.
    for corner in [
        [key(1, TOP), key(1, TOP), key(1, side(0))],
        [key(1, side(0)), key(1, TOP), key(1, TOP)],
        [key(1, TOP); 3],
    ] {
        assert_eq!(topology.corner(&solid, corner, near), Err(NotFound::Corner));
    }
}

#[test]
fn a_key_merged_into_another_face_resolves_through_its_alias() {
    // A boss standing in a plate over one span, its top flush with the
    // plate's: the clean-up mends the seam along its rim and merges the
    // two tops onto the first operand's, which takes the other's key as
    // an alias. Either order.
    let plate = cuboid([0.0; 3], [10.0, 8.0, 2.0], 1, &TOL);
    let boss = Solid::cylinder(DVec3::new(5.0, 4.0, -1.0), 2.0, 3.0, 2, &TOL).unwrap();
    for (first, second, kept, merged) in [
        (&plate, &boss, key(1, TOP), key(2, TOP)),
        (&boss, &plate, key(2, TOP), key(1, TOP)),
    ] {
        let solid = op(first, second, Op::Union, &TOL);
        let topology = solid.topology();
        let regions = topology.regions();
        assert!(regions.iter().all(|r| r.key != merged));
        let top = regions.iter().position(|r| r.key == kept).unwrap() as u32;
        assert_eq!(regions[top as usize].aliases, [merged]);
        // Wherever it was picked, the merged key finds the one top.
        for near in [
            DVec3::new(5.0, 4.0, 2.0),
            DVec3::new(9.0, 1.0, 2.0),
            DVec3::new(5.0, 4.0, -1.0),
        ] {
            assert_eq!(topology.face(&solid, &merged, near), Ok(top));
        }
        // And edges and corners named by it: the top's edge at `-y`, and
        // its corner at the origin's side.
        let edge = topology
            .edge(&solid, [merged, key(1, side(0))], DVec3::new(5.0, 0.0, 2.0))
            .unwrap();
        let mut keys = [kept, key(1, side(0))];
        keys.sort();
        assert_eq!(chain_keys(&topology, edge), keys);
        let corner = [merged, key(1, side(0)), key(1, side(3))];
        let found = topology.corner(&solid, corner, DVec3::ZERO).unwrap();
        let vertex = topology.corners()[found as usize].vertex;
        assert_eq!(
            solid.mesh().verts()[vertex as usize],
            DVec3::new(0.0, 0.0, 2.0)
        );
    }
}

#[test]
fn only_a_flush_face_covered_whole_is_an_alias() {
    // A box inside another: its faces go, and nothing names them.
    let outer = cuboid([0.0; 3], [2.0; 3], 1, &TOL);
    let inner = cuboid([0.5; 3], [1.0; 3], 2, &TOL);
    for (a, b) in [(&outer, &inner), (&inner, &outer)] {
        let solid = op(a, b, Op::Union, &TOL);
        assert!(solid.mesh().aliases().is_empty());
    }
    // Flush with the top: its top is the outer box's (the first operand
    // keeps its cap, either way), its sides nothing.
    let inner = cuboid([0.5; 3], [1.0, 1.0, 1.5], 2, &TOL);
    for operation in [Op::Union, Op::Intersection] {
        let solid = op(&outer, &inner, operation, &TOL);
        let mesh = solid.mesh();
        let names: Vec<(FaceKey, FaceKey)> = mesh
            .aliases()
            .iter()
            .map(|&(f, alias)| (mesh.faces()[f as usize].name.key(), alias))
            .collect();
        assert_eq!(names, [(key(1, TOP), key(2, TOP))], "{operation:?}");
    }
    // A difference makes no aliases: the tool's faces are walls of its own.
    let solid = op(&outer, &inner, Op::Difference, &TOL);
    assert!(solid.mesh().aliases().is_empty());
}

#[test]
fn a_key_only_an_alias_has_resolves() {
    // As a merge leaves it when the whole face went into another: the
    // top's region holds the old key as an alias only.
    let old = key(5, TOP);
    let mesh = cuboid([0.0; 3], [1.0; 3], 1, &TOL).into_mesh();
    let top = mesh
        .faces()
        .iter()
        .position(|f| f.name.key() == key(1, TOP))
        .unwrap() as u32;
    let mesh = mesh.with_aliases(vec![(top, old), (top, old), (top, key(1, TOP))]);
    assert_eq!(mesh.aliases(), [(top, old)]);
    let solid = Solid::new(mesh, &TOL).unwrap();
    let topology = solid.topology();
    let found = topology.face(&solid, &old, DVec3::splat(9.0)).unwrap();
    assert_eq!(topology.regions()[found as usize].key, key(1, TOP));
    let edge = topology
        .edge(&solid, [key(1, side(1)), old], DVec3::ZERO)
        .unwrap();
    assert_eq!(chain_keys(&topology, edge), [key(1, TOP), key(1, side(1))]);
    let corner = [old, key(1, side(1)), key(1, side(2))];
    let found = topology.corner(&solid, corner, DVec3::ZERO).unwrap();
    let vertex = topology.corners()[found as usize].vertex;
    assert_eq!(solid.mesh().verts()[vertex as usize], DVec3::ONE);
}

#[test]
fn aliases_go_with_their_faces_through_booleans() {
    let old = key(5, TOP);
    let aliased = |solid: Solid| {
        let mesh = solid.into_mesh();
        let top = mesh
            .faces()
            .iter()
            .position(|f| f.name.key() == key(1, TOP))
            .unwrap() as u32;
        Solid::new(mesh.with_aliases(vec![(top, old)]), &TOL).unwrap()
    };
    let has = |solid: &Solid| -> Vec<FaceKey> {
        let mesh = solid.mesh();
        mesh.aliases()
            .iter()
            .map(|&(f, alias)| {
                assert_eq!(alias, old);
                mesh.faces()[f as usize].name.key()
            })
            .collect()
    };
    let a = aliased(cuboid([0.0; 3], [2.0; 3], 1, &TOL));
    let b = cuboid([1.0; 3], [2.0; 3], 2, &TOL);
    // Kept wherever the top is: as the first operand or the second, cut
    // or whole, and on the copies of it that claim no surface.
    for (op_, first) in [
        (Op::Union, true),
        (Op::Difference, true),
        (Op::Intersection, true),
        (Op::Union, false),
        (Op::Intersection, false),
    ] {
        let result = if first {
            op(&a, &b, op_, &TOL)
        } else {
            op(&b, &a, op_, &TOL)
        };
        let tops = has(&result);
        assert!(!tops.is_empty(), "{op_:?} {first}");
        assert!(tops.iter().all(|&k| k == key(1, TOP)), "{op_:?} {first}");
        let topology = result.topology();
        assert!(topology.face(&result, &old, DVec3::ZERO).is_ok());
    }
    // Gone with the face: the top cut away.
    let lid = cuboid([-1.0, -1.0, 1.5], [4.0, 4.0, 1.0], 3, &TOL);
    let result = op(&a, &lid, Op::Difference, &TOL);
    assert!(has(&result).is_empty());
    assert_eq!(
        result.topology().face(&result, &old, DVec3::ZERO),
        Err(NotFound::Face)
    );
    // An empty operand gives the other back, aliases and all.
    assert_eq!(op(&a, &Solid::empty(), Op::Union, &TOL), a);
}

#[test]
fn aliases_of_faces_that_aren_t_there_fail_the_check() {
    let mesh = cuboid([0.0; 3], [1.0; 3], 1, &TOL).into_mesh();
    let faces = mesh.faces().len() as u32;
    let bad = mesh.with_aliases(vec![(faces, key(5, TOP))]);
    assert_eq!(
        Solid::new(bad, &TOL),
        Err(KernelError::Invalid(CheckError::Alias(0)))
    );
}

#[test]
fn topology_and_resolving_are_deterministic() {
    let (_, face, edge) = assert_deterministic(|| {
        let solid = two_boxes();
        let topology = solid.topology();
        let near = DVec3::new(7.0, 1.0, 2.0);
        let face = topology.face(&solid, &key(1, TOP), near);
        let edge = topology.edge(&solid, [key(1, TOP), key(1, side(0))], near);
        (topology, face, edge)
    });
    assert!(face.is_ok() && edge.is_ok());
    let solid = drilled(1.0, &TOL);
    assert_eq!(solid.topology(), solid.topology());
}

/// The region of `solid` whose patches reach `x`, the lowest of several.
fn region_at(solid: &Solid, topology: &Topology, x: DVec3) -> u32 {
    (0..topology.regions().len() as u32)
        .find(|&r| reaches(solid, topology, r, x))
        .expect("a region there")
}

#[test]
fn a_flush_cap_dropped_either_way_round_is_an_alias() {
    // A boss whose top is flush with a plate's, inside it, over its edge
    // or round its corner, standing on its middle or through it: whichever
    // operand's top the perturbation drops, both tops' keys name a face of
    // the result's top plane, and the one under the boss's middle where
    // that is one face (the boss inside the plate, or an intersection).
    // Only the dropped top's first triangle was tried, and the plate's
    // middle lies outside the boss, so with the boss first an
    // intersection lost the plate's top.
    let plate = cuboid([0.0; 3], [10.0, 8.0, 2.0], 1, &TOL);
    for (c, r, from, inside) in [
        ([9.0, 4.0], 2.0, 1.0, false),
        ([5.0, 4.0], 2.0, 1.0, true),
        ([0.5, 0.5], 3.0, 1.0, false),
        ([9.0, 4.0], 2.0, -1.0, false),
    ] {
        let boss = Solid::cylinder(DVec3::new(c[0], c[1], from), r, 2.0 - from, 2, &TOL).unwrap();
        let on = DVec3::new(c[0], c[1], 2.0);
        for operation in [Op::Union, Op::Intersection] {
            for (a, b) in [(&plate, &boss), (&boss, &plate)] {
                let solid = op(a, b, operation, &TOL);
                let topology = solid.topology();
                let top = region_at(&solid, &topology, on);
                for k in [key(1, TOP), key(2, TOP)] {
                    let what = format!("{c:?} {from} {operation:?} {k:?}");
                    let found = topology.face(&solid, &k, on).expect(&what);
                    let tris = &topology.regions()[found as usize].tris;
                    let flat = tris.iter().all(|&t| {
                        let p = solid.mesh().patch(t as usize);
                        p.p.iter().chain(&p.c).all(|q| q.z == 2.0)
                    });
                    assert!(flat, "{what}");
                    if inside || operation == Op::Intersection {
                        assert_eq!(found, top, "{what}");
                    }
                }
            }
        }
    }
    let boss = Solid::cylinder(DVec3::new(5.0, 4.0, 1.0), 2.0, 1.0, 2, &TOL).unwrap();
    let aliases = assert_deterministic(|| {
        let solid = op(&boss, &plate, Op::Intersection, &TOL);
        solid.mesh().aliases().to_vec()
    });
    assert_eq!(aliases.len(), 1);
}

#[test]
fn a_covered_face_takes_its_aliases_along() {
    // A boss flush with a plate's top merges into it (the boss's top an
    // alias of the plate's); a lid whose top is flush with both, first in
    // an intersection, then keeps its own top: the plate's top and the
    // alias it held both name the lid's.
    let plate = cuboid([0.0; 3], [10.0, 8.0, 2.0], 1, &TOL);
    let boss = Solid::cylinder(DVec3::new(5.0, 4.0, -1.0), 2.0, 3.0, 2, &TOL).unwrap();
    let joined = op(&plate, &boss, Op::Union, &TOL);
    let lid = Solid::cylinder(DVec3::new(5.0, 4.0, 1.0), 3.0, 1.0, 3, &TOL).unwrap();
    let solid = op(&lid, &joined, Op::Intersection, &TOL);
    let topology = solid.topology();
    let on = DVec3::new(5.0, 4.0, 2.0);
    let top = region_at(&solid, &topology, on);
    for k in [key(1, TOP), key(2, TOP), key(3, TOP)] {
        assert_eq!(topology.face(&solid, &k, on), Ok(top), "{k:?}");
    }
}

#[test]
fn a_covered_face_s_aliases_go_along_though_its_key_lives_on() {
    // Two blocks of one feature, the first's top holding an alias; a lid
    // flush with that top, first in a union, keeps its own: the alias
    // names the lid's top, though the top's own key still names the
    // second block's. It was looked for only where the key was gone.
    let old = key(5, TOP);
    let blocks = op(
        &cuboid([0.0; 3], [4.0, 4.0, 2.0], 1, &TOL),
        &cuboid([10.0, 0.0, 0.0], [4.0, 4.0, 2.0], 1, &TOL),
        Op::Union,
        &TOL,
    );
    let mesh = blocks.into_mesh();
    let first = (0..mesh.tris().len())
        .find(|&t| {
            let p = mesh.patch(t);
            p.p.iter().all(|q| q.z == 2.0 && q.x < 5.0)
        })
        .unwrap();
    let top = mesh.tris()[first].face;
    let blocks = Solid::new(mesh.with_aliases(vec![(top, old)]), &TOL).unwrap();
    let lid = cuboid([-1.0, -1.0, 1.0], [6.0, 6.0, 1.0], 3, &TOL);
    let solid = op(&lid, &blocks, Op::Union, &TOL);
    let topology = solid.topology();
    let on = DVec3::new(2.0, 2.0, 2.0);
    let lid_top = region_at(&solid, &topology, on);
    assert_eq!(topology.regions()[lid_top as usize].key, key(3, TOP));
    assert_eq!(topology.face(&solid, &old, on), Ok(lid_top));
    let second = region_at(&solid, &topology, DVec3::new(12.0, 2.0, 2.0));
    assert_eq!(topology.face(&solid, &key(1, TOP), on), Ok(second));
}

#[test]
fn a_key_both_live_and_aliased_goes_by_the_point() {
    // One feature's two bosses (as one extrude of two circles makes them):
    // the first flush with the plate's top, so its top's key is the
    // plate's alias, the second standing above it. The key names both;
    // the point chooses.
    let plate = cuboid([0.0; 3], [20.0, 8.0, 2.0], 1, &TOL);
    let flush = Solid::cylinder(DVec3::new(5.0, 4.0, -1.0), 2.0, 3.0, 2, &TOL).unwrap();
    let tall = Solid::cylinder(DVec3::new(15.0, 4.0, 1.0), 2.0, 3.0, 2, &TOL).unwrap();
    let solid = op(&op(&plate, &flush, Op::Union, &TOL), &tall, Op::Union, &TOL);
    let topology = solid.topology();
    let regions = topology.regions();
    assert!(regions.iter().any(|r| r.key == key(2, TOP)));
    assert!(regions.iter().any(|r| r.aliases.contains(&key(2, TOP))));
    for on in [DVec3::new(5.0, 4.0, 2.0), DVec3::new(15.0, 4.0, 4.0)] {
        let found = topology.face(&solid, &key(2, TOP), on + DVec3::Z * 0.1);
        assert_eq!(found, Ok(region_at(&solid, &topology, on)), "{on}");
    }
}

#[test]
fn far_huge_and_odd_points_still_resolve() {
    // Never a panic, and the same answer every time: the nearest where
    // the distances can be told apart (a far point still sees which box
    // is nearer), the lowest where they can't (past the squares' range
    // every distance is infinite) or the point isn't a number.
    let solid = two_boxes();
    let topology = solid.topology();
    let top = key(1, TOP);
    let lowest = topology.face(&solid, &top, DVec3::splat(f64::NAN)).unwrap();
    let right = region_at(&solid, &topology, DVec3::new(25.0, 2.0, 2.0));
    for (near, want) in [
        (DVec3::new(1e9, 2.0, 2.0), Some(right)),
        (DVec3::new(1e15, 1e15, 1e15), Some(right)),
        (DVec3::new(-1e15, 2.0, 2.0), None),
        (DVec3::splat(1e200), Some(lowest)),
        (DVec3::splat(-f64::MAX), Some(lowest)),
        (DVec3::new(f64::INFINITY, 0.0, 0.0), Some(lowest)),
        (DVec3::new(1e-310, -0.0, 0.0), None),
    ] {
        let found = topology.face(&solid, &top, near).unwrap();
        if let Some(want) = want {
            assert_eq!(found, want, "{near}");
        } else {
            assert_ne!(found, right, "{near}");
        }
        assert_eq!(topology.face(&solid, &top, near), Ok(found));
        let edge = [key(1, TOP), key(1, side(0))];
        assert!(topology.edge(&solid, edge, near).is_ok());
        let corner = [key(1, TOP), key(1, side(0)), key(1, side(3))];
        assert!(topology.corner(&solid, corner, near).is_ok());
    }
}

#[test]
fn a_tie_goes_to_the_lowest() {
    // Halfway between the boxes: as near the left box's top past the
    // groove as the right box's.
    let solid = two_boxes();
    let topology = solid.topology();
    let tops: Vec<u32> = (0..topology.regions().len() as u32)
        .filter(|&r| topology.regions()[r as usize].key == key(1, TOP))
        .collect();
    let at = |x: f64| region_at(&solid, &topology, DVec3::new(x, 2.0, 2.0));
    let (left, right) = (at(8.0), at(25.0));
    let lowest = left.min(right);
    assert!(tops.contains(&left) && tops.contains(&right));
    let found = topology.face(&solid, &key(1, TOP), DVec3::new(15.0, 2.0, 2.0));
    assert_eq!(found, Ok(lowest));
}

#[test]
fn corners_where_four_faces_meet_resolve_by_any_three() {
    // A step: a block on the left half of a slab, their fronts in one
    // plane meeting along a straight edge, so they are one face, the
    // block's front key an alias of it, and three regions meet where the
    // step's riser meets the front; by any three keys, aliases too.
    let slab = cuboid([0.0; 3], [4.0, 2.0, 1.0], 1, &TOL);
    let block = cuboid([0.0, 0.0, 0.0], [2.0, 2.0, 2.0], 2, &TOL);
    let solid = op(&slab, &block, Op::Union, &TOL);
    let topology = solid.topology();
    let vertex = DVec3::new(2.0, 0.0, 1.0);
    let at = topology
        .corners()
        .iter()
        .position(|c| solid.mesh().verts()[c.vertex as usize] == vertex)
        .expect("a corner at the step") as u32;
    let keys: Vec<FaceKey> = topology.corners()[at as usize]
        .regions
        .iter()
        .map(|&r| topology.regions()[r as usize].key)
        .collect();
    assert_eq!(keys.len(), 3, "{keys:?}");
    // The block's front, merged into the slab's.
    let front = key(2, side(0));
    let mut keys = keys;
    assert!(!keys.contains(&front));
    keys.push(front);
    for i in 0..keys.len() {
        for j in i + 1..keys.len() {
            for k in j + 1..keys.len() {
                let three = [keys[i], keys[j], keys[k]];
                let found = topology.corner(&solid, three, vertex + DVec3::splat(0.1));
                assert_eq!(found, Ok(at), "{three:?}");
            }
        }
    }
}

#[test]
fn a_chain_of_flush_joins_names_alike_at_any_size_and_tolerance() {
    // A drilled plate and bosses joined one after another, the body first
    // as the app joins them: through it filling a hole, flush with both
    // its caps, standing on it, through it flush with the last one's top,
    // a block over its edge flush with both caps, and a taller one at its
    // other edge. Each step's faces, as the sets of keys naming each
    // region, are the same at another size and a finer tolerance.
    use std::collections::BTreeSet;

    use glam::DVec2;

    use crate::profile::tests::{circle, rect};
    use crate::{Frame, Profile, extrude};
    let names = |scale: f64, tol: &Tolerance| -> Vec<Vec<BTreeSet<FaceKey>>> {
        let v = |x: f64, y: f64| DVec2::new(x, y) * scale;
        let up = |loops, from: f64, to: f64, feature| {
            let profile = Profile { loops };
            let (from, to) = (from * scale, to * scale);
            extrude(
                &profile,
                &Frame::XY,
                from,
                to,
                feature,
                tol,
                &Budget::DEFAULT,
            )
            .unwrap()
        };
        let plate = up(
            vec![
                rect(v(-30.0, -20.0), v(30.0, 20.0), 0),
                circle(v(-15.0, 1.0), 3.0 * scale, 4, true),
                circle(v(20.0, 12.0), 3.0 * scale, 5, true),
            ],
            0.0,
            10.0,
            1,
        );
        let bosses = [
            up(
                vec![circle(v(-15.0, 0.0), 8.0 * scale, 0, false)],
                0.0,
                20.0,
                2,
            ),
            up(
                vec![circle(v(-5.0, 2.0), 6.0 * scale, 0, false)],
                0.0,
                10.0,
                3,
            ),
            up(
                vec![circle(v(4.0, -1.0), 5.0 * scale, 0, false)],
                10.0,
                20.0,
                4,
            ),
            up(
                vec![circle(v(11.0, 0.0), 4.0 * scale, 0, false)],
                0.0,
                20.0,
                5,
            ),
            up(vec![rect(v(-40.0, -5.0), v(-25.0, 5.0), 10)], 0.0, 10.0, 6),
            up(vec![rect(v(25.0, -25.0), v(35.0, 25.0), 10)], 0.0, 20.0, 7),
        ];
        let mut body = plate;
        let mut steps = Vec::new();
        for boss in &bosses {
            body = crate::boolean(&body, boss, Op::Union, tol, &Budget::DEFAULT).unwrap();
            let topology = body.topology();
            let mut regions: Vec<BTreeSet<FaceKey>> = topology
                .regions()
                .iter()
                .map(|r| {
                    std::iter::once(r.key)
                        .chain(r.aliases.iter().copied())
                        .collect()
                })
                .collect();
            regions.sort();
            steps.push(regions);
        }
        steps
    };
    let first = names(1.0, &TOL);
    // The flush tops of the last two bosses are one face, named by both.
    let both: BTreeSet<FaceKey> = [key(4, TOP), key(5, TOP)].into();
    assert!(first[3].contains(&both), "{:?}", first[3]);
    assert_eq!(names(1.37, &TOL), first);
    assert_eq!(names(1.0, &Tolerance::new(1e-4).unwrap()), first);
}

/// `loops` extruded up `z` from 0 to 1 as feature 1.
fn prism(loops: Vec<crate::profile::Loop>, tol: &Tolerance) -> Solid {
    crate::extrude(
        &crate::Profile { loops },
        &crate::Frame::XY,
        0.0,
        1.0,
        1,
        tol,
        &Budget::DEFAULT,
    )
    .unwrap()
}

/// The tangent chains of `solid` as sets of its chains' keys, each set
/// and the sets sorted.
fn tangent_sets(solid: &Solid) -> Vec<Vec<[FaceKey; 2]>> {
    let topology = solid.topology();
    let first = topology.tangent_chains(solid);
    assert_eq!(first.len(), topology.chains().len());
    let mut sets: std::collections::BTreeMap<u32, Vec<[FaceKey; 2]>> = Default::default();
    for (c, &f) in first.iter().enumerate() {
        // The lowest of its set, which is its own first.
        assert!(f <= c as u32 && first[f as usize] == f);
        sets.entry(f)
            .or_default()
            .push(chain_keys(&topology, c as u32));
    }
    let mut sets: Vec<_> = sets.into_values().collect();
    sets.iter_mut().for_each(|set| set.sort());
    sets.sort();
    sets
}

#[test]
fn a_rounded_plate_s_rims_are_tangent_chains() {
    use crate::profile::tests::arc;
    use crate::profile::{Loop, Segment};
    let p = glam::DVec2::new;
    let line = |a, b, curve| Segment::line(a, b, curve).unwrap();
    // Rounded at each corner, radius 2: lines 0, 2, 4, 6 and arcs 1, 3,
    // 5, 7.
    let outline = Loop {
        segments: vec![
            line(p(2.0, 0.0), p(8.0, 0.0), 0),
            arc(p(8.0, 2.0), p(8.0, 0.0), p(10.0, 2.0), 1),
            line(p(10.0, 2.0), p(10.0, 4.0), 2),
            arc(p(8.0, 4.0), p(10.0, 4.0), p(8.0, 6.0), 3),
            line(p(8.0, 6.0), p(2.0, 6.0), 4),
            arc(p(2.0, 4.0), p(2.0, 6.0), p(0.0, 4.0), 5),
            line(p(0.0, 4.0), p(0.0, 2.0), 6),
            arc(p(2.0, 2.0), p(0.0, 2.0), p(2.0, 0.0), 7),
        ],
    };
    for tol in [TOL, Tolerance::new(1e-2).unwrap()] {
        let solid = prism(vec![outline.clone()], &tol);
        let sets = tangent_sets(&solid);
        let rim = |cap: PartKey| {
            let mut set: Vec<[FaceKey; 2]> = (0..8)
                .map(|curve| {
                    let (a, b) = (key(1, cap), key(1, side(curve)));
                    [a.min(b), a.max(b)]
                })
                .collect();
            set.sort();
            set
        };
        // The two rims, and each upright edge where a wall meets the next
        // on its own: they run on into nothing.
        assert_eq!(sets.len(), 2 + 8, "{tol:?}");
        assert!(sets.contains(&rim(TOP)), "{tol:?}");
        assert!(sets.contains(&rim(BOTTOM)), "{tol:?}");
        assert_eq!(sets.iter().filter(|set| set.len() == 1).count(), 8);
    }
}

#[test]
fn edges_turning_by_a_degree_or_more_aren_t_tangent() {
    use crate::profile::tests::polygon;
    // A turn of `degrees` at (10, 0): the top's edges along walls 0 and 1
    // meet there.
    let turned = |degrees: f64| {
        let rise = 10.0 * libm::tan(degrees.to_radians());
        let p = glam::DVec2::new;
        let points = [
            p(0.0, 0.0),
            p(10.0, 0.0),
            p(20.0, rise),
            p(20.0, 10.0),
            p(0.0, 10.0),
        ];
        prism(vec![polygon(&points, 0)], &TOL)
    };
    let joined = |solid: &Solid| {
        let pair = |curve| {
            let (a, b) = (key(1, TOP), key(1, side(curve)));
            [a.min(b), a.max(b)]
        };
        let sets = tangent_sets(solid);
        sets.iter()
            .any(|set| set.contains(&pair(0)) && set.contains(&pair(1)))
    };
    assert!(joined(&turned(0.5)));
    assert!(joined(&turned(0.99)));
    assert!(!joined(&turned(1.01)));
    assert!(!joined(&turned(2.0)));
    // A box's edges all meet square.
    let solid = cuboid([0.0; 3], [1.0, 2.0, 3.0], 1, &TOL);
    assert!(tangent_sets(&solid).iter().all(|set| set.len() == 1));
    // A cylinder's rims close on themselves.
    let solid = Solid::cylinder(DVec3::ZERO, 2.0, 1.0, 4, &TOL).unwrap();
    let topology = solid.topology();
    assert_eq!(topology.tangent_chains(&solid), [0, 1]);
}

/// Rims through arcs that aren't quarters, a slot's ends of two quarters
/// each, and a hole's: each rim one tangent chain, at either tolerance,
/// with the corners where lines meet square left apart.
#[test]
fn rims_through_part_arcs_and_slot_ends_are_tangent_chains() {
    use crate::profile::tests::arc;
    use crate::profile::{Loop, Segment};
    let p = glam::DVec2::new;
    let line = |a, b, curve| Segment::line(a, b, curve).unwrap();
    // Along x, a 60° arc of radius 5 up to the left, a line on along it,
    // and square corners back: lines 0, 2, 3, 4, arc 1.
    let (s, c) = (libm::sin(60f64.to_radians()), libm::cos(60f64.to_radians()));
    let bend = p(10.0 + 5.0 * s, 5.0 - 5.0 * c);
    let end = bend + p(c, s) * 6.0;
    let bent = Loop {
        segments: vec![
            line(p(0.0, 0.0), p(10.0, 0.0), 0),
            arc(p(10.0, 5.0), p(10.0, 0.0), bend, 1),
            line(bend, end, 2),
            line(end, p(0.0, end.y), 3),
            line(p(0.0, end.y), p(0.0, 0.0), 4),
        ],
    };
    // A slot hole through it, clockwise: ends of two quarter arcs each.
    let slot = Loop {
        segments: vec![
            arc(p(3.0, 4.0), p(3.0, 3.0), p(2.0, 4.0), 6),
            arc(p(3.0, 4.0), p(2.0, 4.0), p(3.0, 5.0), 6),
            line(p(3.0, 5.0), p(7.0, 5.0), 7),
            arc(p(7.0, 4.0), p(7.0, 5.0), p(8.0, 4.0), 8),
            arc(p(7.0, 4.0), p(8.0, 4.0), p(7.0, 3.0), 8),
            line(p(7.0, 3.0), p(3.0, 3.0), 9),
        ],
    };
    for tol in [TOL, Tolerance::new(1e-2).unwrap()] {
        let solid = prism(vec![bent.clone(), slot.clone()], &tol);
        let sets = tangent_sets(&solid);
        let rim = |cap: PartKey, curves: &[u64]| {
            let mut set: Vec<[FaceKey; 2]> = (curves.iter())
                .map(|&curve| {
                    let (a, b) = (key(1, cap), key(1, side(curve)));
                    [a.min(b), a.max(b)]
                })
                .collect();
            set.sort();
            set
        };
        for cap in [TOP, BOTTOM] {
            assert!(sets.contains(&rim(cap, &[0, 1, 2])), "{tol:?}: {sets:?}");
            for curve in [3, 4] {
                assert!(sets.contains(&rim(cap, &[curve])), "{tol:?}: {sets:?}");
            }
            assert!(sets.contains(&rim(cap, &[6, 7, 8, 9])), "{tol:?}: {sets:?}");
        }
    }
}
