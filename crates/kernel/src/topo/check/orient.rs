//! The check's invariant 8, on a solid's orientation and regions
//! ([`Orientation`]; a sheet's check leaves it out). The empty solid,
//! without shells, passes it vacuously.
//!
//! **Its checks**, on the shells, in this order:
//! - a shell's sides all face its region: `In` sides on a lump's shell,
//!   `Out` sides on an empty region's;
//! - its **signed volume**, its sides oriented (an `Out` side as its face,
//!   an `In` side reversed), is certified of its kind's sign: negative on
//!   the outer shell of the region it faces, positive on an island. A
//!   sign certified the other way is a defect;
//! - where the volume's bounds hold zero, its sign is the **perturbed
//!   ray's** to decide (the winding of a point of the shell's region,
//!   counted along a ray whose ties are perturbed away), not built here
//!   ([`Finding::Unbuilt`]).
//!
//! **The signed volume.** By the divergence theorem a closed shell's
//! volume is `⅓·Σ ∮ (x − o)·n dA` over its faces, each face's flux the
//! body's measure computes ([`face_integrals`]: certified, its fits',
//! quadratures' and ribbons' errors in its bounds), an `In` side's
//! counted negative. The origin `o` is the centre of the box of the
//! control points of the shell's faces' edges: the flux doesn't depend on
//! it, its bounds do, and this keeps them small and the shell's own. The
//! faces are summed in order of their keys (their sides, then their ids,
//! breaking exact ties), so the sum's rounding doesn't depend on the
//! order the shell lists them in.
//!
//! A shell's side-graph, its regions and their islands are not compared
//! with the body's faces, edges and contact records here, nor its islands'
//! placement asked again.
//!
//! **Work.** Every shell is charged one unit, one more per side, each
//! face's flux as the measure charges it.
//!
//! **Reading a broken body.** A side naming no face, a region id naming
//! no region, or a face whose loops don't close or whose spans leave its
//! tiles is passed over: the earlier invariants refuse those.

use varde_decide::num::bounded::Bounded;

use super::{Cx, Fault, Finding, Invariant, Item, ItemKind};
use crate::infra::budget::{Ledger, Step, Stop};
use crate::measure::mass::{Integrand, face_integrals};
use crate::topo::brep::{Brep, FaceId, RegionKind, ShellId, ShellKind, Side};

#[cfg(test)]
mod tests;

/// The checks of invariant 8, numbered in the hook's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum OrientCheck {
    /// A shell's side not facing its region.
    Sides = 1,
    /// A shell's signed volume certified against its kind's sign.
    Sign,
    /// A shell's signed volume not certified of a sign: the perturbed
    /// ray's to decide, not built.
    Ray,
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
        &[ItemKind::Shell]
    }

    fn check(&self, cx: &Cx<'_>, item: Item, ledger: &mut Ledger<'_>) -> Checked {
        charge(ledger, 1)?;
        match item {
            Item::Shell(s) => shell(cx, s, ledger),
            _ => Ok(()),
        }
    }
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
    charge(ledger, sides.len())?;
    let mut sides: Vec<(FaceId, Side)> = sides
        .iter()
        .copied()
        .filter(|(f, _)| f.index() < b.face_count())
        .collect();
    if let Some(&kind) = b.regions.kind.get(ss.region[s.index()].index()) {
        let facing = match kind {
            RegionKind::Material => Side::In,
            RegionKind::Empty => Side::Out,
        };
        if sides.iter().any(|&(_, side)| side != facing) {
            return fault(O::Sides, Finding::Defect);
        }
    }
    sides.sort_unstable_by(|&(f, a), &(g, c)| (b.key(f), a, f).cmp(&(b.key(g), c, g)));
    let Some(volume) = signed_volume(cx, &sides, ledger)? else {
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

/// Three times the signed volume of the shell of `sides` (module docs),
/// certified; `None` where a face can't be measured (a broken store).
fn signed_volume(
    cx: &Cx<'_>,
    sides: &[(FaceId, Side)],
    ledger: &mut Ledger<'_>,
) -> Result<Option<Bounded<f64>>, Stop<Fault>> {
    let b = cx.body();
    let o = centre(cx, sides);
    let mut sum = Bounded::exact(0.0);
    for &(f, side) in sides {
        let flux = match face_integrals(b, f, o, &[Integrand::Volume], ledger) {
            Ok(v) => v[0],
            Err(Stop::Failed(_)) => return Ok(None),
            Err(Stop::Exhausted(x)) => return Err(Stop::Exhausted(x)),
            Err(Stop::Cap(cap)) => return Err(Stop::Cap(cap)),
        };
        sum = match side {
            Side::Out => sum + flux,
            Side::In => sum + -flux,
        };
    }
    Ok(Some(sum))
}

/// The centre of the box of the control points of the faces' edges; the
/// world origin where they have none.
fn centre(cx: &Cx<'_>, sides: &[(FaceId, Side)]) -> [f64; 3] {
    let b: &Brep = cx.body();
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for &(f, _) in sides {
        for &e in &cx.inc.face_edges[f.index()] {
            for p in b.edge_spans(e).iter().flat_map(|span| span.iter()) {
                for k in 0..3 {
                    // Compare and select: no `min`/`max`, a NaN left out.
                    if p[k] < lo[k] {
                        lo[k] = p[k];
                    }
                    if p[k] > hi[k] {
                        hi[k] = p[k];
                    }
                }
            }
        }
    }
    std::array::from_fn(|k| {
        if lo[k] <= hi[k] {
            lo[k] * 0.5 + hi[k] * 0.5
        } else {
            0.0
        }
    })
}
