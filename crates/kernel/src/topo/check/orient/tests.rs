//! Invariant 8 on the primitives and a box with a cavity: each passes; a
//! shell of the wrong sign, a side facing away from its region, a volume
//! whose sign isn't certified, a side in another side-shell's shell, an
//! island in a region that can't hold it and a wrong container are
//! caught where they are; the empty solid passes the whole check;
//! running out names the invariant.

use varde_decide::num::storage::OpStart;

use super::*;
use crate::build::primitives::{Primitive, build};
use crate::infra::budget::{Budget, Work};
use crate::infra::ops::{FeatureId, OpCtx, Status, Tolerance};
use crate::topo::brep::{BodyKind, Brep, RegionRow};
use crate::topo::check::topo::tests::{Passing, failure};
use crate::topo::check::{CheckError, Inv, Invariants, Scope, run};
use crate::topo::regions::tests::cavity;

/// Invariant 8, the others passing.
fn table() -> Invariants<'static> {
    let p: &dyn Invariant = &Passing;
    [p, p, p, p, p, p, p, &Orientation, p, p]
}

fn check_on(b: &Brep, budget: &Budget) -> Result<(), CheckError> {
    let work = &mut Work::new(budget);
    run(b, Scope::Full, None, &table(), work, 0)
}

/// Invariant 8 on `b`, the same on one thread and on several.
fn check(b: &Brep) -> Result<(), CheckError> {
    #[cfg(not(target_arch = "wasm32"))]
    return crate::infra::par::assert_deterministic(|| check_on(b, &Budget::DEFAULT));
    #[cfg(target_arch = "wasm32")]
    return check_on(b, &Budget::DEFAULT);
}

/// The failure of `b`'s check: its item, check and finding.
fn fails(b: &Brep) -> (Item, u32, Finding) {
    let f = failure(check(b));
    assert_eq!(f.inv, Inv::Orientation);
    (f.item, f.check, f.finding)
}

fn built(p: &Primitive) -> Brep {
    let ctx = OpCtx::new(FeatureId(1));
    build(p, ctx, Tolerance::DEFAULT, &mut Work::new(&Budget::DEFAULT))
        .unwrap_or_else(|e| panic!("{p:?}: {e:?}"))
}

fn cuboid() -> Brep {
    built(&Primitive::Cuboid {
        min: [1.0, -2.0, 0.5],
        size: [3.0, 5.0, 6.75],
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
    [
        Primitive::Cuboid {
            min: [1.0, -2.0, 0.5],
            size: [3.0, 5.0, 6.75],
        },
        Primitive::Cylinder {
            base: [1.5, -2.0, 3.0],
            radius: 4.0,
            height: 9.5,
        },
        Primitive::Sphere {
            centre: [10.0, -20.0, 5.0],
            radius: 7.0,
        },
        cone([1.0, 2.0, -3.0], [5.0, 0.0], 7.0),
        cone([1.0, 2.0, -3.0], [0.0, 5.0], 7.0),
        cone([-4.0, 0.25, 2.0], [3.0, 1.5], 2.5),
        cone([-4.0, 0.25, 2.0], [1.5, 3.0], 2.5),
        torus(6.0, 2.5),
        torus(2.0, 3.0),
    ]
    .iter()
    .map(built)
    .collect()
}

/// A primitive's island (its faces' `Out` sides, in the unbounded
/// region) and its lump's outer shell (their `In` sides).
fn shells(b: &Brep) -> [ShellId; 2] {
    let f = FaceId(0);
    [b.face_shell(f, Side::Out), b.face_shell(f, Side::In)]
}

/// Each passes; the box and the sphere the same on one thread and on
/// several.
#[test]
fn the_primitives_pass() {
    for b in all() {
        assert_eq!(check_on(&b, &Budget::DEFAULT), Ok(()));
    }
    let sphere = Primitive::Sphere {
        centre: [10.0, -20.0, 5.0],
        radius: 7.0,
    };
    for b in [cuboid(), built(&sphere)] {
        assert_eq!(check(&b), Ok(()));
    }
}

/// The island made the outer shell of the unbounded region and the outer
/// shell an island of the lump: their volumes have the other signs. The
/// island's sides come first.
#[test]
fn a_shell_of_the_wrong_sign_is_caught() {
    let sphere = Primitive::Sphere {
        centre: [10.0, -20.0, 5.0],
        radius: 7.0,
    };
    for mut b in [cuboid(), built(&sphere)] {
        let [island, outer] = shells(&b);
        b.shells.kind[island.index()] = ShellKind::Outer;
        b.shells.kind[outer.index()] = ShellKind::Island;
        assert_eq!(
            fails(&b),
            (
                Item::Shell(island),
                OrientCheck::Sign as u32,
                Finding::Defect
            )
        );
        // The lump's shell alone of the wrong sign.
        b.shells.kind[island.index()] = ShellKind::Island;
        assert_eq!(
            fails(&b),
            (
                Item::Shell(outer),
                OrientCheck::Sign as u32,
                Finding::Defect
            )
        );
    }
}

/// The lump and the unbounded region trade kinds: each shell's sides
/// face away from its region.
#[test]
fn a_side_facing_away_from_its_region_is_caught() {
    let mut b = cuboid();
    let [island, outer] = shells(&b);
    let [r0, r1] = [island, outer].map(|s| b.shell_region(s).index());
    b.regions.kind.swap(r0, r1);
    assert_eq!(
        fails(&b),
        (
            Item::Shell(island),
            OrientCheck::Sides as u32,
            Finding::Defect
        )
    );
}

/// Ribbons as wide as the box leave the volume's sign open: the perturbed
/// ray would decide it.
#[test]
fn a_volume_of_no_certified_sign_is_left_to_the_ray() {
    let mut b = cuboid();
    b.coedges.seam_bound.iter_mut().for_each(|w| *w = 10.0);
    let [island, _] = shells(&b);
    assert_eq!(
        fails(&b),
        (
            Item::Shell(island),
            OrientCheck::Ray as u32,
            Finding::Unbuilt
        )
    );
}

/// The empty solid, with no regions or with the unbounded one alone,
/// passes the whole check.
#[test]
fn the_empty_solid_passes() {
    let start = OpStart {
        max_coord: 2.0,
        rho_op: 0.0,
    };
    let status = Status::new(Tolerance::DEFAULT, &start);
    let mut b = Brep::new(BodyKind::Solid, status);
    assert_eq!(b.check(&Budget::DEFAULT), Ok(()));
    b.push_region(RegionRow {
        kind: RegionKind::Empty,
        outer: ShellId::NONE,
        islands: Vec::new(),
        container: RegionId::NONE,
    })
    .unwrap();
    assert_eq!(b.check(&Budget::DEFAULT), Ok(()));
}

#[test]
fn running_out_names_the_invariant() {
    let b = cuboid();
    match check_on(&b, &Budget::new(400)) {
        Err(CheckError::OutOfBudget { inv, .. }) => assert_eq!(inv, Some(Inv::Orientation)),
        other => panic!("{other:?}"),
    }
}

/// The box with a cavity passes: its four shells are its side graph's
/// components, each island where the boxes place it, no container.
#[test]
fn a_box_with_a_cavity_passes() {
    assert_eq!(check(&cavity()), Ok(()));
}

/// A box face's `Out` side moved into the lump's shell (the links kept
/// consistent): the side graph's component of `Out` sides is split
/// across two shells.
#[test]
fn a_side_in_another_shell_is_caught() {
    let mut b = cuboid();
    let [island, outer] = shells(&b);
    let f = FaceId(2);
    b.shells.sides[island.index()].retain(|&(g, _)| g != f);
    b.shells.sides[outer.index()].push((f, Side::Out));
    b.faces.shells[f.index()][Side::Out.index()] = outer;
    assert_eq!(
        fails(&b),
        (Item::Body, OrientCheck::Shells as u32, Finding::Defect)
    );
}

/// The box's outer sides moved from the outside into the cavity's void
/// (the lump's container with them): the cavity's box leaves them out.
#[test]
fn an_island_its_region_cannot_hold_is_caught() {
    let mut b = cavity();
    let outside = RegionId(0);
    let void = RegionId(2);
    let island = b.regions.islands[outside.index()].remove(0);
    b.regions.islands[void.index()].push(island);
    b.shells.region[island.index()] = void;
    // The lump's container follows its empty islands.
    b.regions.container[1] = void;
    assert_eq!(
        fails(&b),
        (
            Item::Region(void),
            OrientCheck::Place as u32,
            Finding::Defect
        )
    );
}

/// The lump's container set to the void (another empty region, as
/// invariant 1 asks), or on the box to the outside: its empty islands lie
/// in the outside, so it has none.
#[test]
fn a_wrong_container_is_caught() {
    let mut b = cavity();
    let lump = RegionId(1);
    b.regions.container[lump.index()] = RegionId(2);
    assert_eq!(
        fails(&b),
        (
            Item::Region(lump),
            OrientCheck::Container as u32,
            Finding::Defect
        )
    );
    let mut b = cuboid();
    let [island, outer] = shells(&b);
    let lump = b.shell_region(outer);
    b.regions.container[lump.index()] = b.shell_region(island);
    assert_eq!(
        fails(&b),
        (
            Item::Region(lump),
            OrientCheck::Container as u32,
            Finding::Defect
        )
    );
}
