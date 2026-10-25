//! The check's invariant 8, on a solid's orientation and regions
//! ([`Orientation`]; a sheet's check leaves it out). The empty solid,
//! without shells, passes it vacuously.
//!
//! **Its checks**, in this order:
//! - on the body: its **side graph** is rebuilt from its faces, edges and
//!   contact records ([`regions::side_graph`]): a contact record's glue
//!   needs its orientation class, not built ([`Finding::Unbuilt`]); a
//!   glue across an edge whose coedges run it the same way isn't
//!   balanced; each component of the graph must be one stored shell,
//!   whole;
//! - on each shell: its sides all face its region (`In` sides on a
//!   lump's shell, `Out` sides on an empty region's); its **signed
//!   volume**, its sides oriented (an `Out` side as its face, an `In`
//!   side reversed; [`regions::signed_volume`]), is certified of its
//!   kind's sign: negative on the outer shell of the region it faces,
//!   positive on an island. A sign certified the other way is a defect;
//!   where the volume's bounds hold zero, its sign is the **perturbed
//!   ray's** to decide, not built;
//! - on each region: each island's **placement**. An island outside the
//!   box of its region's outer shell is a defect; the region the box
//!   exclusions leave it ([`regions::place`], among the outer shells of
//!   its kind: a material island the one lump whose box holds it, an
//!   empty island the unbounded region where no void's box does) must be
//!   its region, a material island no lump's box holds is a defect, and
//!   one left more than one region is the nesting query's to place, not
//!   built; then the region's **container**: a lump's is the region of
//!   the shells of its outer shell's faces' `Out` sides (none for the
//!   unbounded region), which must all lie in that one region; a void's
//!   and the unbounded region's is none.
//!
//! **Work.** The body one unit per edge and per side; every shell one
//! unit, one more per side, each face's flux as the measure charges it;
//! every region one unit, one per island, and each box it reads as the
//! measure's box search charges it.
//!
//! **Reading a broken body.** A side naming no face, a region id naming
//! no region, or a face whose loops don't close or whose spans leave its
//! tiles is passed over: the earlier invariants refuse those.

use super::{Cx, Fault, Finding, Invariant, Item, ItemKind};
use crate::infra::budget::{Ledger, Step, Stop};
use crate::measure::bounds::BoundsError;
use crate::topo::brep::{FaceId, RegionId, RegionKind, ShellId, ShellKind, Side};
use crate::topo::regions::{self, Place, RegionError};

#[cfg(test)]
mod tests;

/// The checks of invariant 8, numbered in the hook's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum OrientCheck {
    /// The body has a contact record, whose glue isn't built.
    Graph = 1,
    /// A glue across an edge whose coedges run it the same way.
    Balance,
    /// A component of the side graph not one stored shell, whole.
    Shells,
    /// A shell's side not facing its region.
    Sides,
    /// A shell's signed volume certified against its kind's sign.
    Sign,
    /// A shell's signed volume not certified of a sign: the perturbed
    /// ray's to decide, not built.
    Ray,
    /// An island its region can't hold, or another region's.
    Place,
    /// An island the box exclusions leave more than one region for: the
    /// nesting query's to place, not built.
    Unplaced,
    /// A region's container isn't the region of its empty islands.
    Container,
}

type Checked = Result<(), Stop<Fault>>;

fn fault(check: OrientCheck, finding: Finding) -> Checked {
    Err(Stop::Failed(Fault {
        check: check as u32,
        finding,
    }))
}

/// Charges `n` units of work.
fn charge(ledger: &mut Ledger<'_>, n: usize) -> Checked {
    ledger.charge(Step::Work, u64::try_from(n).unwrap_or(u64::MAX))?;
    Ok(())
}

/// Invariant 8, orientation and regions (module docs).
pub(crate) struct Orientation;

impl Invariant for Orientation {
    fn kinds(&self) -> &[ItemKind] {
        &[ItemKind::Body, ItemKind::Shell, ItemKind::Region]
    }

    fn check(&self, cx: &Cx<'_>, item: Item, ledger: &mut Ledger<'_>) -> Checked {
        charge(ledger, 1)?;
        match item {
            Item::Body => body(cx, ledger),
            Item::Shell(s) => shell(cx, s, ledger),
            Item::Region(r) => region(cx, r, ledger),
            _ => Ok(()),
        }
    }
}

fn body(cx: &Cx<'_>, ledger: &mut Ledger<'_>) -> Checked {
    use OrientCheck as O;
    let b = cx.body();
    let graph = match regions::side_graph(b, ledger) {
        Ok(g) => g,
        Err(Stop::Failed(RegionError::Balance(_))) => return fault(O::Balance, Finding::Defect),
        Err(Stop::Failed(_)) => return fault(O::Graph, Finding::Unbuilt),
        Err(Stop::Exhausted(x)) => return Err(Stop::Exhausted(x)),
        Err(Stop::Cap(c)) => return Err(Stop::Cap(c)),
    };
    for sides in &graph {
        let s = b.face_shell(sides[0].0, sides[0].1);
        let whole = s.index() < b.shell_count()
            && sides.iter().all(|&(f, side)| b.face_shell(f, side) == s)
            && b.shells.sides[s.index()].len() == sides.len();
        if !whole {
            return fault(O::Shells, Finding::Defect);
        }
    }
    Ok(())
}

fn shell(cx: &Cx<'_>, s: ShellId, ledger: &mut Ledger<'_>) -> Checked {
    use OrientCheck as O;
    let b = cx.body();
    let ss = &b.shells;
    let negative = match ss.kind[s.index()] {
        ShellKind::Outer => true,
        ShellKind::Island => false,
        // A solid's shell is closed: invariant 1's.
        ShellKind::Open => return Ok(()),
    };
    let sides = &ss.sides[s.index()];
    if let Some(&kind) = b.regions.kind.get(ss.region[s.index()].index()) {
        let facing = match kind {
            RegionKind::Material => Side::In,
            RegionKind::Empty => Side::Out,
        };
        if sides.iter().any(|&(_, side)| side != facing) {
            return fault(O::Sides, Finding::Defect);
        }
    }
    let Some(volume) = regions::signed_volume(b, sides, &cx.inc.face_edges, ledger)? else {
        return Ok(());
    };
    // The flux is three times the volume: its sign is the volume's.
    if volume.v.abs() > volume.err() {
        if (volume.v < 0.0) == negative {
            Ok(())
        } else {
            fault(O::Sign, Finding::Defect)
        }
    } else {
        fault(O::Ray, Finding::Unbuilt)
    }
}

/// A box search's stop as the check's: a broken face passes (the earlier
/// invariants refuse it).
fn bounded<T>(r: Result<T, Stop<BoundsError>>) -> Result<Option<T>, Stop<Fault>> {
    match r {
        Ok(t) => Ok(Some(t)),
        Err(Stop::Failed(_)) => Ok(None),
        Err(Stop::Exhausted(x)) => Err(Stop::Exhausted(x)),
        Err(Stop::Cap(c)) => Err(Stop::Cap(c)),
    }
}

/// Whether region id `r` names a region of the body.
fn is_region(cx: &Cx<'_>, r: RegionId) -> bool {
    r.index() < cx.body().region_count()
}

/// The sides of shell `s`; none for an id naming no shell.
fn sides_of<'a>(cx: &Cx<'a>, s: ShellId) -> &'a [(FaceId, Side)] {
    cx.body
        .shells
        .sides
        .get(s.index())
        .map_or(&[], Vec::as_slice)
}

fn region(cx: &Cx<'_>, r: RegionId, ledger: &mut Ledger<'_>) -> Checked {
    use OrientCheck as O;
    let b = cx.body();
    let rs = &b.regions;
    let kind = rs.kind[r.index()];
    let outer = rs.outer[r.index()];
    let edges = &cx.inc.face_edges;
    // The outer shells of each kind, by region.
    let outers = |kind: RegionKind| -> Vec<(RegionId, ShellId)> {
        (0..b.region_count())
            .map(|q| RegionId(q as u32))
            .filter(|&q| rs.kind[q.index()] == kind && !rs.outer[q.index()].is_none())
            .map(|q| (q, rs.outer[q.index()]))
            .collect()
    };
    let cands = outers(kind);
    let cand_sides: Vec<&[(FaceId, Side)]> = cands.iter().map(|&(_, s)| sides_of(cx, s)).collect();
    for &island in &rs.islands[r.index()] {
        charge(ledger, 1)?;
        let sides = sides_of(cx, island);
        if sides.is_empty() {
            continue;
        }
        if !outer.is_none() {
            let out = regions::excludes(b, sides_of(cx, outer), sides, edges, ledger);
            if bounded(out)? == Some(true) {
                return fault(O::Place, Finding::Defect);
            }
        }
        let material = kind == RegionKind::Material;
        let place = regions::place(b, sides, material, &cand_sides, edges, ledger);
        let Some(place) = bounded(place)? else {
            continue;
        };
        let right = match place {
            Place::In(k) => cands[k].0 == r,
            Place::Unbounded => outer.is_none(),
            Place::Unenclosed => false,
            Place::Unsettled => return fault(O::Unplaced, Finding::Unbuilt),
        };
        if !right {
            return fault(O::Place, Finding::Defect);
        }
    }
    let want = match (kind, outer.is_none()) {
        (RegionKind::Material, false) => {
            // The regions of the shells of its outer shell's faces' `Out`
            // sides.
            let mut around: Vec<RegionId> = sides_of(cx, outer)
                .iter()
                .filter(|(f, _)| f.index() < b.face_count())
                .map(|&(f, _)| b.face_shell(f, Side::Out))
                .map(|s| {
                    b.shells
                        .region
                        .get(s.index())
                        .copied()
                        .unwrap_or(RegionId::NONE)
                })
                .collect();
            around.sort_unstable();
            around.dedup();
            match around.as_slice() {
                &[e] if is_region(cx, e) => {
                    if rs.outer[e.index()].is_none() {
                        RegionId::NONE
                    } else {
                        e
                    }
                }
                _ => return fault(O::Container, Finding::Defect),
            }
        }
        _ => RegionId::NONE,
    };
    if rs.container[r.index()] == want {
        Ok(())
    } else {
        fault(O::Container, Finding::Defect)
    }
}
