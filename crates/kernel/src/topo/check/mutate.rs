//! The mutation suite: each defect of an invariant this build checks,
//! seeded into a valid body, is rejected by the check with the right
//! first failure (its invariant, the item the defect was seeded at, the
//! check of the invariant naming it, what it found) and evidence naming
//! that item (or, for a shell, its faces); the same on one thread and on
//! several. The valid bodies the defects are seeded into pass, and so
//! does a variant an invariant admits (a mixed crease, which claims
//! nothing).
//!
//! **The check as far as this build certifies.** The suite runs the
//! check's own hooks with the parts this build doesn't certify yet passed
//! over ([`Staged`]): a curved trim (4) or a pair of faces apart (9)
//! fails the full check of every primitive with a face, as not built,
//! before or after the seeded defect. Where the defect's invariant comes
//! before the first part not built on the valid body, the full check
//! ([`Brep::check`]) reports the same failure; elsewhere it refuses the
//! body too, and with that part's invariant left out
//! ([`Brep::check_except`]) reports the same failure where the valid body
//! then passes.
//!
//! **Defects by invariant**, each seeded where the invariant certifies
//! it in this build:
//! - 1: a pair not paired back, a coedge turned round;
//! - 2: an edge's parameter range off its curve's end;
//! - 3: a face's trims moved off its surface's lattice rectangle;
//! - 4: trims crossing, an outer loop running the wrong way;
//! - 5: a seam bound below the deviation it bounds, an edge's tolerance
//!   over its origin's target, a vertex's tolerance below an edge's;
//! - 6: a wrong continuity (a crease labelled tangent, a crease's sign
//!   flipped), an edge of the same form between faces of different
//!   forms, a seam tag on an edge of different forms;
//! - 8: a solid turned inside out, a face's side in another side-shell's
//!   shell, a lump's container not its empty islands' region;
//! - 10: a seam edge untagged, a pole untagged, a face key's digest the
//!   body's terms don't hold, an attribute set the body doesn't keep, an
//!   alias equal to its region's primary and one naming no key of its
//!   region (nor of the body: an alias naming a live key is valid, one
//!   region's primary may be another's alias).
//!
//! Not seeded: invariant 2's shared tile rows (held by the store's
//! types, not re-checked) and exact rows (no primitive has one; their
//! own tests seed them), invariant 7's fitted deviation (a face on a
//! fitted surface is not built), invariant 9's adjacent pairs (an empty
//! window needs geometry made for it, as their own tests build: no field
//! within the earlier invariants' bounds narrows one) and contact records
//! (not built), and defects of later stages (a fillet crossing its
//! support, a self-touching surface).
//!
//! The body keeps no aliases: they are derived, so the alias defects are
//! seeded into the derived user topology the check reads
//! ([`SeededAlias`]); the check reports them at the body, whose evidence
//! names no entity.

use super::names::{Names, aliases};
use super::{
    BUILT, CheckError, CheckFailure, Cx, Fault, Finding, Inv, Invariant, Invariants, Item,
    ItemKind, Scope, run,
};
use crate::build::primitives::Primitive;
use varde_decide::ids::Entity;

use crate::evidence::{Failure, ItemRole, Leaf};
use crate::geom::pcurve::{PCurve, PSpan};
use crate::infra::budget::{Budget, Ledger, Stop, Work};
use crate::infra::error::{Invalid, KernelError};
use crate::infra::ops::{FeatureId, OpCtx};
use crate::names::keys::{Digest, FaceKey};
use crate::names::topology::Topology;
use crate::names::topology::tests::primitive;
use crate::topo::brep::{
    AttrSetId, Brep, CoedgeId, Continuity, Convexity, EdgeId, EdgeKind, FaceId, FacePart, Side,
    VertId,
};
use crate::topo::check::geom::{RegularityCheck, SeamCheck};
use crate::topo::check::names::NameCheck;
use crate::topo::check::orient::OrientCheck;
use crate::topo::check::topo::{DomainCheck, JoinCheck, TopoCheck};
use crate::topo::check::trims::TrimCheck;

/// A hook with the parts this build doesn't certify passed over: an item
/// failing as not built passes.
struct Staged<'a>(&'a dyn Invariant);

impl Invariant for Staged<'_> {
    fn kinds(&self) -> &[ItemKind] {
        self.0.kinds()
    }

    fn check(&self, cx: &Cx<'_>, item: Item, ledger: &mut Ledger<'_>) -> Result<(), Stop<Fault>> {
        match self.0.check(cx, item, ledger) {
            Err(Stop::Failed(f)) if f.finding == Finding::Unbuilt => Ok(()),
            r => r,
        }
    }
}

/// Invariant 10 with an alias seeded into the first region of the
/// derived user topology: the alias `seed` makes of that region.
struct SeededAlias(fn(&Brep, &Topology) -> FaceKey);

impl Invariant for SeededAlias {
    fn kinds(&self) -> &[ItemKind] {
        Names.kinds()
    }

    fn check(&self, cx: &Cx<'_>, item: Item, ledger: &mut Ledger<'_>) -> Result<(), Stop<Fault>> {
        if item != Item::Body {
            return Names.check(cx, item, ledger);
        }
        let b = cx.body();
        let t = Topology::derive(b, &cx.inc.contact_edges, ledger)?;
        let seeded = (self.0)(b, &t);
        for (k, r) in t.regions().iter().enumerate() {
            let mut r = r.clone();
            if k == 0 {
                r.aliases.push(seeded);
                r.aliases.sort_unstable();
            }
            aliases(b, &r)?;
        }
        Ok(())
    }
}

/// The check of `b` as far as this build certifies (module docs), with
/// `names` for invariant 10, the same on one thread and on several.
fn staged_with(b: &Brep, names: &dyn Invariant) -> Result<(), CheckError> {
    let staged: Vec<Staged<'_>> = BUILT.iter().map(|&h| Staged(h)).collect();
    let names = Staged(names);
    let mut table: Invariants<'_> = BUILT;
    for (slot, hook) in table.iter_mut().zip(&staged) {
        *slot = hook;
    }
    table[9] = &names;
    let go = || {
        run(
            b,
            Scope::Full,
            None,
            &table,
            &mut Work::new(&Budget::DEFAULT),
            0,
        )
    };
    #[cfg(not(target_arch = "wasm32"))]
    return crate::infra::par::assert_deterministic(go);
    #[cfg(target_arch = "wasm32")]
    return go();
}

fn staged(b: &Brep) -> Result<(), CheckError> {
    staged_with(b, &Names)
}

/// A seeded defect: what it is, and the failure it must give.
struct Mutant {
    what: &'static str,
    inv: Inv,
    check: u32,
    finding: Finding,
}

fn mutant(what: &'static str, inv: Inv, check: u32) -> Mutant {
    Mutant {
        what,
        inv,
        check,
        finding: Finding::Defect,
    }
}

/// The entity a failure at `item` names in its evidence.
fn entity(item: Item) -> Option<Entity> {
    match item {
        Item::Vert(v) => Some(Entity::Vert(v)),
        Item::Edge(e) => Some(Entity::Edge(e)),
        Item::Coedge(c) => Some(Entity::Coedge(c)),
        Item::Loop(l) => Some(Entity::Loop(l)),
        Item::Face(f) => Some(Entity::Face(f)),
        _ => None,
    }
}

/// `valid` with a defect seeded (`m`, at one of `at`, its items) is
/// rejected with `m`'s failure at one of `at`, by the check `names`
/// stands in for invariant 10 of, and its evidence names that item;
/// `full`: the defect is the body's, so the full check sees it too.
#[track_caller]
fn rejects_with(
    valid: &Brep,
    b: &Brep,
    m: &Mutant,
    at: &[Item],
    names: &dyn Invariant,
    full: bool,
) {
    let what = m.what;
    let err = staged_with(b, names).expect_err(what);
    let CheckError::Failed(f) = err.clone() else {
        panic!("{what}: {err:?}");
    };
    let CheckFailure {
        inv,
        item,
        check,
        finding,
    } = f;
    assert_eq!((inv, check, finding), (m.inv, m.check, m.finding), "{what}");
    assert!(at.contains(&item), "{what}: at {item:?}, not {at:?}");
    // The full check: the same failure before the valid body's first part
    // not built; a refusal after it.
    let budget = Budget::DEFAULT;
    if full {
        let full = b.check(&budget);
        match valid.check(&budget) {
            Err(CheckError::Failed(u)) if u.finding == Finding::Unbuilt && u.inv <= inv => {
                assert!(full.is_err(), "{what}");
                if u.inv < inv && valid.check_except(u.inv, &budget) == Ok(()) {
                    assert_eq!(b.check_except(u.inv, &budget), Err(err.clone()), "{what}");
                }
            }
            _ => assert_eq!(full, Err(err.clone()), "{what}"),
        }
    }
    // The evidence: an operand's defect is invalid at its invariant (a
    // crossing a self-intersection), naming the item, or a shell's faces.
    let leaf = Leaf::new(0, b);
    let op = OpCtx::new(FeatureId(1));
    let failure = Failure::of_check(&err, super::Role::Operand, &leaf, op, 0).expect(what);
    let error = match finding {
        Finding::Crossing | Finding::EmptyWindow => Invalid::SelfIntersection,
        _ => Invalid::Check { inv: inv.number() },
    };
    assert_eq!(failure.error, KernelError::Invalid(error), "{what}");
    assert_eq!(failure.check(), Ok(()), "{what}");
    if let Some(id) = entity(item) {
        let failing = (failure.evidence.items.iter())
            .find(|i| i.role == ItemRole::Failing)
            .and_then(|i| i.subject.as_ref())
            .expect(what);
        assert_eq!(failing.id, id, "{what}");
        assert!(failing.name.is_some(), "{what}: unnamed");
    }
    if let Item::Shell(s) = item {
        let mut faces: Vec<Entity> = (failure.evidence.items.iter())
            .filter(|i| i.role == ItemRole::Witness)
            .filter_map(|i| Some(i.subject.as_ref()?.id))
            .collect();
        faces.sort_unstable();
        let mut sides: Vec<Entity> = b.shells.sides[s.index()]
            .iter()
            .map(|&(f, _)| Entity::Face(f))
            .collect();
        sides.sort_unstable();
        assert_eq!(faces, sides, "{what}");
    }
}

#[track_caller]
fn rejects(valid: &Brep, b: &Brep, m: Mutant, at: &[Item]) {
    rejects_with(valid, b, &m, at, &Names, true);
}

fn cuboid() -> Brep {
    primitive(&Primitive::Cuboid {
        min: [1.0, -2.0, 0.5],
        size: [3.0, 5.0, 6.75],
    })
}

fn cylinder() -> Brep {
    primitive(&Primitive::Cylinder {
        base: [1.5, -2.0, 3.0],
        radius: 4.0,
        height: 9.5,
    })
}

fn cone() -> Brep {
    primitive(&Primitive::Cone {
        base: [1.0, 2.0, -3.0],
        radii: [5.0, 0.0],
        height: 7.0,
    })
}

/// `b` with `seed` applied, and the items the defect was seeded at.
fn seeded(b: &Brep, seed: impl FnOnce(&mut Brep) -> Vec<Item>) -> (Brep, Vec<Item>) {
    let mut b = b.clone();
    let at = seed(&mut b);
    (b, at)
}

/// The first edge of `b` for which `f` holds.
fn edge_where(b: &Brep, f: impl Fn(EdgeId) -> bool) -> EdgeId {
    (0..b.edge_count() as u32)
        .map(EdgeId)
        .find(|&e| f(e))
        .expect("an edge")
}

/// The coedges of face `f`.
fn face_coedges(b: &Brep, f: FaceId) -> Vec<CoedgeId> {
    (0..b.coedge_count() as u32)
        .map(CoedgeId)
        .filter(|c| b.coedges.face[c.index()] == f)
        .collect()
}

/// `(u, v, w)` mapped by `map` in affine UV, its weight kept.
fn map_hom(q: [f64; 3], map: &impl Fn([f64; 2]) -> [f64; 2]) -> [f64; 3] {
    let [u, v] = map([q[0] / q[2], q[1] / q[2]]);
    [u * q[2], v * q[2], q[2]]
}

/// Every p-curve of face `f`, its spans linear or rational quadratic,
/// mapped point by point by `map`.
fn map_face_uv(b: &mut Brep, f: FaceId, map: impl Fn([f64; 2]) -> [f64; 2]) {
    for c in face_coedges(b, f) {
        let id = b.coedges.pcurve[c.index()].index();
        let p = &b.pcurves[id];
        let spans = (p.spans().iter())
            .map(|s| match *s {
                PSpan::Linear(ends) => PSpan::Linear(ends.map(|q| map_hom(q, &map))),
                PSpan::Q(q) => PSpan::Q(q.map(|q| map_hom(q, &map))),
                other => panic!("not a linear or Q span: {other:?}"),
            })
            .collect();
        b.pcurves[id] = PCurve::from_spans(spans, p.knots().to_vec()).expect("a p-curve");
    }
}

/// The face's corners in UV: the start of each of its coedges' p-curves.
fn face_uvs(b: &Brep, f: FaceId) -> Vec<[f64; 2]> {
    face_coedges(b, f)
        .into_iter()
        .map(|c| b.pcurves[b.coedges.pcurve[c.index()].index()].spans()[0].start())
        .collect()
}

#[test]
fn the_valid_bodies_and_variants_pass() {
    for b in [cuboid(), cylinder(), cone()] {
        assert_eq!(staged(&b), Ok(()));
    }
    // A mixed crease claims nothing.
    let mut b = cuboid();
    b.edges.cont[0] = Continuity::Crease(Convexity::Mixed);
    assert_eq!(staged(&b), Ok(()));
}

#[test]
fn topology_defects_are_rejected() {
    let valid = cuboid();
    let c = CoedgeId(0);
    let partner = valid.coedges.pair[0];
    let other = (0..valid.coedge_count() as u32)
        .map(CoedgeId)
        .find(|&o| o != c && o != partner && valid.edge(o) != valid.edge(c))
        .expect("another coedge");
    let (b, at) = seeded(&valid, |b| {
        b.coedges.pair[c.index()] = other;
        vec![Item::Coedge(c), Item::Coedge(partner), Item::Coedge(other)]
    });
    let m = mutant(
        "a pair not paired back",
        Inv::Topology,
        TopoCheck::Pair as u32,
    );
    rejects(&valid, &b, m, &at);
    let (b, at) = seeded(&valid, |b| {
        b.coedges.reversed[c.index()] ^= true;
        vec![Item::Coedge(c), Item::Coedge(b.coedges.next[c.index()])]
    });
    let m = mutant(
        "a coedge turned round",
        Inv::Topology,
        TopoCheck::Corner as u32,
    );
    rejects(&valid, &b, m, &at);
}

#[test]
fn join_and_domain_defects_are_rejected() {
    let valid = cuboid();
    let e = EdgeId(0);
    let (b, at) = seeded(&valid, |b| {
        let t = &mut b.edges.t_range[e.index()][1];
        *t = t.next_down();
        vec![Item::Edge(e)]
    });
    let m = mutant(
        "an edge's range not its curve's",
        Inv::Joins,
        JoinCheck::Range as u32,
    );
    rejects(&valid, &b, m, &at);
    let f = FaceId(1);
    let (b, at) = seeded(&valid, |b| {
        // A whole number of tiles: each span stays inside one.
        let r = b.surfaces.surface[b.faces.surface[f.index()].index()].rect();
        let far = (r.u.end - r.u.start) as f64;
        map_face_uv(b, f, |[u, v]| [u + far, v]);
        let mut at: Vec<Item> = face_coedges(b, f).into_iter().map(Item::Coedge).collect();
        at.push(Item::Face(f));
        at
    });
    let m = mutant(
        "trims off the lattice rectangle",
        Inv::Domain,
        DomainCheck::Inside as u32,
    );
    rejects(&valid, &b, m, &at);
}

#[test]
fn trim_defects_are_rejected() {
    let valid = cuboid();
    let f = FaceId(1);
    let uvs = face_uvs(&valid, f);
    let (a, z) = (uvs[0], uvs[1]);
    // Two corners' places swapped: a bow tie, its trims crossing.
    let (b, at) = seeded(&valid, |b| {
        map_face_uv(b, f, |p| {
            if p == a {
                z
            } else if p == z {
                a
            } else {
                p
            }
        });
        vec![Item::Face(f)]
    });
    let mut m = mutant("trims crossing", Inv::Trims, TrimCheck::Crossing as u32);
    m.finding = Finding::Crossing;
    rejects(&valid, &b, m, &at);
    // Mirrored about the face's centre line: the outer loop runs the
    // wrong way.
    let mid = uvs.iter().map(|p| p[0]).sum::<f64>() / uvs.len() as f64;
    let (b, at) = seeded(&valid, |b| {
        map_face_uv(b, f, |[u, v]| [2.0 * mid - u, v]);
        vec![Item::Face(f), Item::Loop(b.faces.first_loop[f.index()])]
    });
    let m = mutant(
        "an outer loop the wrong way",
        Inv::Trims,
        TrimCheck::Orientation as u32,
    );
    rejects(&valid, &b, m, &at);
}

#[test]
fn seam_defects_are_rejected() {
    let valid = cone();
    let c = CoedgeId(0);
    let (b, at) = seeded(&valid, |b| {
        b.coedges.seam_bound[c.index()] = 0.0;
        vec![Item::Coedge(c)]
    });
    let m = mutant(
        "a seam bound below the deviation",
        Inv::Seams,
        SeamCheck::SeamBound as u32,
    );
    rejects(&valid, &b, m, &at);
    let valid = cuboid();
    let e = EdgeId(0);
    let (b, at) = seeded(&valid, |b| {
        b.edges.tol[e.index()] = 0.375 * b.eps.eps().eps();
        vec![Item::Edge(e)]
    });
    let m = mutant(
        "an edge's tolerance over its target",
        Inv::Seams,
        SeamCheck::Target as u32,
    );
    rejects(&valid, &b, m, &at);
    let v = VertId(0);
    let (b, at) = seeded(&valid, |b| {
        b.verts.tol[v.index()] = 0.0;
        vec![Item::Vert(v)]
    });
    let m = mutant(
        "a vertex's tolerance below an edge's",
        Inv::Seams,
        SeamCheck::VertTol as u32,
    );
    rejects(&valid, &b, m, &at);
}

#[test]
fn regularity_defects_are_rejected() {
    let valid = cuboid();
    let e = EdgeId(0);
    let Continuity::Crease(sign) = valid.edges.cont[e.index()] else {
        panic!("a crease");
    };
    let at = [Item::Edge(e)];
    let (b, _) = seeded(&valid, |b| {
        b.edges.cont[e.index()] = Continuity::Tangent { angle: 0.0 };
        vec![]
    });
    let m = mutant(
        "a crease labelled tangent",
        Inv::Regularity,
        RegularityCheck::Tangent as u32,
    );
    rejects(&valid, &b, m, &at);
    let flipped = match sign {
        Convexity::Convex => Convexity::Concave,
        _ => Convexity::Convex,
    };
    let (b, _) = seeded(&valid, |b| {
        b.edges.cont[e.index()] = Continuity::Crease(flipped);
        vec![]
    });
    let m = mutant(
        "a crease's sign flipped",
        Inv::Regularity,
        RegularityCheck::Crease as u32,
    );
    rejects(&valid, &b, m, &at);
    let (b, _) = seeded(&valid, |b| {
        b.edges.kind[e.index()] = EdgeKind::SameForm { touch: false };
        vec![]
    });
    let m = mutant(
        "same form across two forms",
        Inv::Regularity,
        RegularityCheck::SameForm as u32,
    );
    rejects(&valid, &b, m, &at);
    let (b, _) = seeded(&valid, |b| {
        b.edges.tag[e.index()] = Some(crate::names::keys::EdgeTag::Seam);
        vec![]
    });
    let m = mutant(
        "a seam tag across two forms",
        Inv::Regularity,
        RegularityCheck::SeamTag as u32,
    );
    rejects(&valid, &b, m, &at);
}

#[test]
fn a_solid_turned_inside_out_is_rejected() {
    let valid = cuboid();
    let (b, at) = seeded(&valid, |b| {
        // Every face's other side outward: its loops run the other way,
        // each vertex starting the coedge that ended there.
        let prev = b.coedges.prev.clone();
        for v in 0..b.vert_count() {
            let c = b.verts.coedge[v];
            b.verts.coedge[v] = prev[c.index()];
        }
        for c in 0..b.coedge_count() {
            b.coedges.reversed[c] ^= true;
        }
        std::mem::swap(&mut b.coedges.next, &mut b.coedges.prev);
        for f in &mut b.faces.flipped {
            *f ^= true;
        }
        // Its creases the other way.
        for c in &mut b.edges.cont {
            if let Continuity::Crease(sign) = c {
                *sign = match sign {
                    Convexity::Convex => Convexity::Concave,
                    Convexity::Concave => Convexity::Convex,
                    Convexity::Mixed => Convexity::Mixed,
                };
            }
        }
        (0..b.shell_count() as u32)
            .map(|s| Item::Shell(crate::topo::brep::ShellId(s)))
            .collect()
    });
    let m = mutant(
        "a solid inside out",
        Inv::Orientation,
        OrientCheck::Sign as u32,
    );
    rejects(&valid, &b, m, &at);
}

#[test]
fn region_defects_are_rejected() {
    let valid = cuboid();
    let f = FaceId(2);
    let island = valid.face_shell(f, Side::Out);
    let outer = valid.face_shell(f, Side::In);
    let (b, at) = seeded(&valid, |b| {
        // A face's `Out` side linked to the lump's shell, both lists kept
        // in step with the face.
        b.shells.sides[island.index()].retain(|&(g, _)| g != f);
        b.shells.sides[outer.index()].push((f, Side::Out));
        b.faces.shells[f.index()][Side::Out.index()] = outer;
        vec![Item::Body]
    });
    let m = mutant(
        "a side in another side-shell's shell",
        Inv::Orientation,
        OrientCheck::Shells as u32,
    );
    rejects(&valid, &b, m, &at);
    let lump = valid.shell_region(outer);
    let (b, at) = seeded(&valid, |b| {
        b.regions.container[lump.index()] = b.shells.region[island.index()];
        vec![Item::Region(lump)]
    });
    let m = mutant(
        "a lump's container not its empty islands' region",
        Inv::Orientation,
        OrientCheck::Container as u32,
    );
    rejects(&valid, &b, m, &at);
}

#[test]
fn name_defects_are_rejected() {
    let valid = cylinder();
    let e = edge_where(&valid, |e| valid.edge_tag(e).is_some());
    let (b, at) = seeded(&valid, |b| {
        b.edges.tag[e.index()] = None;
        vec![Item::Edge(e)]
    });
    let m = mutant("a seam edge untagged", Inv::Names, NameCheck::Seam as u32);
    rejects(&valid, &b, m, &at);
    let valid = cone();
    let v = (0..valid.vert_count() as u32)
        .map(VertId)
        .find(|&v| valid.vert_tag(v).is_some())
        .expect("the apex");
    let (b, at) = seeded(&valid, |b| {
        b.verts.tag[v.index()] = None;
        vec![Item::Vert(v)]
    });
    let m = mutant("a pole untagged", Inv::Names, NameCheck::PoleTag as u32);
    rejects(&valid, &b, m, &at);
    let valid = cuboid();
    let f = FaceId(2);
    let (b, at) = seeded(&valid, |b| {
        let key = b.faces.key[f.index()];
        b.faces.key[f.index()] = FaceKey {
            part: FacePart::Offset {
                of: Digest::from_u64(0x5eed),
            },
            ..key
        };
        vec![Item::Face(f)]
    });
    let m = mutant(
        "a digest no term holds",
        Inv::Names,
        NameCheck::Digest as u32,
    );
    rejects(&valid, &b, m, &at);
    let (b, at) = seeded(&valid, |b| {
        b.faces.attrs[f.index()] = AttrSetId(0);
        vec![Item::Face(f)]
    });
    let m = mutant(
        "an attribute set not kept",
        Inv::Names,
        NameCheck::FaceAttrs as u32,
    );
    rejects(&valid, &b, m, &at);
}

/// An alias equal to its region's primary, and one naming no key of its
/// region, are rejected; one naming a key of another region passes.
#[test]
fn alias_defects_are_rejected() {
    let valid = cuboid();
    let at = [Item::Body];
    let primary = SeededAlias(|_, t| t.regions()[0].primary);
    let m = mutant(
        "an alias equal to its primary",
        Inv::Names,
        NameCheck::AliasPrimary as u32,
    );
    rejects_with(&valid, &valid, &m, &at, &primary, false);
    let foreign = SeededAlias(|b, t| {
        let mut key = *b.key(t.regions()[0].faces[0]);
        key.feature = FeatureId(u64::MAX);
        key
    });
    let m = mutant(
        "an alias naming no key of its region",
        Inv::Names,
        NameCheck::AliasFace as u32,
    );
    rejects_with(&valid, &valid, &m, &at, &foreign, false);
}
