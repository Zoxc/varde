//! The builder on polygon bodies (a square sheet, a tetrahedron): loops
//! linked and paired as added, tolerances raised, shells and regions
//! joined; each refusal leaving the body as it was; removals unpairing
//! and compacting with ids mapped in order; geometry collected from its
//! roots; and every id the finished store holds naming a row.

use std::collections::BTreeMap;

use varde_decide::num::storage::OpStart;

use super::*;
use crate::geom::form::PlaneKey;
use crate::geom::lattice::{CELL, Lattice};
use crate::geom::pcurve::Span2;
use crate::infra::budget::Budget;
use crate::infra::ops::{FeatureId, InstanceId, Tolerance};
use crate::topo::brep::{ClusterKind, ClusterOrigin, ContactCert, ContactKind, FacePart, PoleEnd};
use varde_decide::num::decide::Sign;

const EPS: f64 = 1e-3;

fn status() -> Status {
    let start = OpStart {
        max_coord: 1.0,
        rho_op: 0.0,
    };
    Status::new(Tolerance::new(EPS).expect("in range"), &start)
}

fn key(part: u32) -> FaceKey {
    FaceKey {
        feature: FeatureId(1),
        part: FacePart::Primitive(part),
        instance: InstanceId(0),
    }
}

fn line(a: [f64; 3], b: [f64; 3]) -> Curve3D {
    let m = [0, 1, 2].map(|k| (a[k] + b[k]) * 0.5);
    let pt = |p: [f64; 3]| [p[0], p[1], p[2], 1.0];
    Curve3D::new(vec![[pt(a), pt(m), pt(b)]], vec![0.0, 1.0]).expect("a line")
}

fn uv_line(a: [f64; 2], b: [f64; 2]) -> PCurve {
    let m = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    let span: Span2 = [[a[0], a[1], 1.0], [m[0], m[1], 1.0], [b[0], b[1], 1.0]];
    PCurve::new(vec![span], vec![0.0, 1.0]).expect("a UV line")
}

fn plane() -> Surface {
    let lat = Lattice::new(PlaneKey::new([0.0, 0.0, 1.0], [0.0; 3]).unwrap().into()).unwrap();
    Surface::new(lat.clone(), lat.half(0).unwrap()).unwrap()
}

/// A body of polygon faces, as built: the topology only (every face on
/// one plane, each p-curve the line between its corners' `x, y`), each
/// edge from its lower vertex to its higher, coedge `k` of the body given
/// seam bound `k·10⁻⁹`.
struct Poly {
    b: Builder,
    verts: Vec<VertId>,
    edges: BTreeMap<[u32; 2], EdgeId>,
    faces: Vec<FaceId>,
    loops: Vec<LoopId>,
    surface: SurfId,
}

impl Poly {
    fn new(kind: BodyKind, pos: &[[f64; 3]]) -> Poly {
        let mut b = Builder::new(kind, status());
        let surface = b.add_surface(plane(), None, ExactRows::default()).unwrap();
        let verts = pos.iter().map(|&p| b.add_vert(p).unwrap()).collect();
        Poly {
            b,
            verts,
            edges: BTreeMap::new(),
            faces: Vec::new(),
            loops: Vec::new(),
            surface,
        }
    }

    /// The uses of a loop round the corners `f`, adding the edges and
    /// p-curves it needs.
    fn uses(&mut self, f: &[u32]) -> Vec<Use> {
        let n = f.len();
        (0..n)
            .map(|k| {
                let (from, to) = (f[k], f[(k + 1) % n]);
                let reversed = from > to;
                let (lo, hi) = if reversed { (to, from) } else { (from, to) };
                let a = self.b.brep().pos(self.verts[lo as usize]);
                let z = self.b.brep().pos(self.verts[hi as usize]);
                let e = match self.edges.get(&[lo, hi]) {
                    Some(&e) => e,
                    None => {
                        let c = self.b.add_curve(line(a, z)).unwrap();
                        let ends = [self.verts[lo as usize], self.verts[hi as usize]];
                        let e = self.b.add_edge(c, [0.0, 1.0], ends).unwrap();
                        self.edges.insert([lo, hi], e);
                        e
                    }
                };
                let pcurve = self
                    .b
                    .add_pcurve(uv_line([a[0], a[1]], [z[0], z[1]]))
                    .unwrap();
                let at = self.b.brep().coedge_count() + k;
                Use {
                    edge: e,
                    reversed,
                    pcurve,
                    seam_bound: at as f64 * 1e-9,
                }
            })
            .collect()
    }

    fn face(&mut self, f: &[u32]) -> FaceId {
        let face = self
            .b
            .add_face(self.surface, false, key(self.faces.len() as u32))
            .unwrap();
        let uses = self.uses(f);
        let lp = self.b.add_loop(face, true, &uses).unwrap();
        self.faces.push(face);
        self.loops.push(lp);
        face
    }

    fn edge(&self, a: u32, z: u32) -> EdgeId {
        self.edges[&[a.min(z), a.max(z)]]
    }
}

const SQUARE: [[f64; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [1.0, 1.0, 0.0],
    [0.0, 1.0, 0.0],
];

fn square() -> Poly {
    let mut p = Poly::new(BodyKind::Sheet, &SQUARE);
    p.face(&[0, 1, 2, 3]);
    p
}

const TET: [[f64; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
];

/// Each edge used once each way.
const TET_FACES: [[u32; 3]; 4] = [[0, 2, 1], [0, 1, 3], [1, 2, 3], [0, 3, 2]];

/// The tetrahedron with its shells and regions: the unbounded region
/// (its island the `Out` shell) and the lump (its outer shell the `In`
/// one).
fn tet() -> (Poly, [ShellId; 2], [RegionId; 2]) {
    let mut p = Poly::new(BodyKind::Solid, &TET);
    for f in TET_FACES {
        p.face(&f);
    }
    let sides = |side| p.faces.iter().map(|&f| (f, side)).collect::<Vec<_>>();
    let (out, inn) = (sides(Side::Out), sides(Side::In));
    let so = p.b.add_shell(ShellKind::Island, out).unwrap();
    let si = p.b.add_shell(ShellKind::Outer, inn).unwrap();
    let r0 =
        p.b.add_region(RegionKind::Empty, ShellId::NONE, vec![so], RegionId::NONE)
            .unwrap();
    let r1 =
        p.b.add_region(RegionKind::Material, si, Vec::new(), RegionId::NONE)
            .unwrap();
    (p, [so, si], [r0, r1])
}

/// The rows' counts, in `Brep::dangling`'s kind order, geometry first.
fn counts(b: &Brep) -> [usize; 11] {
    [
        b.surface_count(),
        b.curve_count(),
        b.pcurve_count(),
        b.vert_count(),
        b.edge_count(),
        b.coedge_count(),
        b.loop_count(),
        b.face_count(),
        b.shell_count(),
        b.region_count(),
        b.contacts().len(),
    ]
}

#[test]
fn a_loop_is_linked_round_its_face() {
    let p = square();
    let b = p.b.brep();
    let lp = p.loops[0];
    let cs: Vec<CoedgeId> = b.loop_coedges(lp).collect();
    assert_eq!(cs, (0..4).map(CoedgeId).collect::<Vec<_>>());
    for (k, &c) in cs.iter().enumerate() {
        assert_eq!(b.next(c), cs[(k + 1) % 4]);
        assert_eq!(b.prev(c), cs[(k + 3) % 4]);
        assert_eq!((b.face(c), b.coedge_loop(c)), (p.faces[0], lp));
        assert_eq!(b.pair(c), CoedgeId::NONE, "a sheet's boundary");
        // Each coedge ends where the next starts.
        assert_eq!(
            b.coedge_vert(c, true),
            b.coedge_vert(cs[(k + 1) % 4], false)
        );
        // The vertex it starts at holds it, the edge it uses too.
        assert_eq!(b.vert_coedge(b.coedge_vert(c, false)), c);
        assert_eq!(b.edge_coedge(b.edge(c)), c);
    }
    // The edge from 0 to 3 is used against its way.
    assert!(b.reversed(cs[3]));
    assert_eq!(b.first_loop(p.faces[0]), lp);
    assert!(b.is_outer(lp));
    assert_eq!(b.dangling(), None);
}

#[test]
fn tolerances_rise_to_the_seam_bounds() {
    let p = square();
    let b = p.b.brep();
    for c in 0..4 {
        let c = CoedgeId(c);
        assert_eq!(b.seam_bound(c), c.0 as f64 * 1e-9);
        assert_eq!(b.edge_tol(b.edge(c)), b.seam_bound(c));
    }
    // A vertex takes its edges' greatest tolerance: vertex 0 the edges
    // to 1 (0) and to 3 (3e-9).
    let tols: Vec<f64> = p.verts.iter().map(|&v| b.vert_tol(v)).collect();
    assert_eq!(tols, [3, 1, 2, 3].map(|k| k as f64 * 1e-9));
    // Within `ε/2`: `max_tol` stays there.
    assert_eq!(b.eps().max_tol(), EPS * 0.5);

    let mut p = Poly::new(BodyKind::Sheet, &SQUARE);
    let face = p.b.add_face(p.surface, false, key(0)).unwrap();
    let mut uses = p.uses(&[0, 1, 2, 3]);
    uses[2].seam_bound = EPS;
    p.b.add_loop(face, true, &uses).unwrap();
    assert_eq!(p.b.brep().eps().max_tol(), EPS, "raised by a wide edge");
}

#[test]
fn a_closed_body_pairs_each_edge_once_each_way() {
    let (p, [so, si], [r0, r1]) = tet();
    let b = p.b.brep();
    assert_eq!(counts(b), [1, 6, 12, 4, 6, 12, 4, 4, 2, 2, 0]);
    for c in 0..12 {
        let c = CoedgeId(c);
        let q = b.pair(c);
        assert!(!q.is_none());
        assert_eq!(b.pair(q), c);
        assert_eq!(b.edge(q), b.edge(c));
        assert_ne!(b.reversed(q), b.reversed(c));
    }
    for &f in &p.faces {
        assert_eq!(
            [b.face_shell(f, Side::Out), b.face_shell(f, Side::In)],
            [so, si]
        );
    }
    assert_eq!([b.shell_region(so), b.shell_region(si)], [r0, r1]);
    assert_eq!(b.island_shells(r0), [so]);
    assert_eq!(b.outer_shell(r1), si);
    assert_eq!(b.dangling(), None);
    let (done, map) = p.b.finish().unwrap();
    assert_eq!(counts(&done), [1, 6, 12, 4, 6, 12, 4, 4, 2, 2, 0]);
    assert_eq!(map.coedges(CoedgeId(7)), CoedgeId(7));
}

#[test]
fn a_hole_goes_after_the_outer_loop() {
    let mut p = Poly::new(
        BodyKind::Sheet,
        &[
            [0.0, 0.0, 0.0],
            [4.0, 0.0, 0.0],
            [4.0, 4.0, 0.0],
            [0.0, 4.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 2.0, 0.0],
            [2.0, 2.0, 0.0],
        ],
    );
    let face = p.face(&[0, 1, 2, 3]);
    let uses = p.uses(&[4, 5, 6]);
    let hole = p.b.add_loop(face, false, &uses).unwrap();
    let b = p.b.brep();
    assert_eq!(b.loops(face).collect::<Vec<_>>(), [p.loops[0], hole]);
    assert!(!b.is_outer(hole));
    assert_eq!(b.loop_coedges(hole).count(), 3);
}

#[test]
fn refused_loops_add_nothing() {
    let mut p = Poly::new(BodyKind::Solid, &TET);
    p.face(&TET_FACES[0]);
    let face = p.b.add_face(p.surface, false, key(1)).unwrap();
    let good = p.uses(&TET_FACES[1]);
    let before = counts(p.b.brep());
    let refused = |p: &mut Poly, uses: &[Use]| {
        let e = p.b.add_loop(face, true, uses).unwrap_err();
        assert_eq!(counts(p.b.brep()), before, "{e:?}");
        e
    };
    let at = |index, why| BuildError::Use { index, why };

    assert_eq!(refused(&mut p, &[]), BuildError::EmptyLoop);
    // Corners 0 → 1 → 3 with the last two swapped: 1 → 3 is followed by
    // 3 → 0 only if in order.
    let swapped = [good[0], good[2], good[1]];
    assert_eq!(refused(&mut p, &swapped), at(1, UseError::Chain));
    // Face 0 runs the edge 0–1 against its way: a second use that way
    // is refused.
    let mut same = good.clone();
    same[0].reversed = true;
    assert_eq!(refused(&mut p, &same), at(0, UseError::SameWay));
    let mut nan = good.clone();
    nan[1].seam_bound = f64::NAN;
    assert_eq!(refused(&mut p, &nan), at(1, UseError::SeamBound));
    let mut wrong = good.clone();
    // A p-curve over [0, 2] on an edge over [0, 1].
    let long = PCurve::new(
        vec![
            [[0.0, 0.0, 1.0], [0.5, 0.0, 1.0], [1.0, 0.0, 1.0]],
            [[1.0, 0.0, 1.0], [1.5, 0.0, 1.0], [2.0, 0.0, 1.0]],
        ],
        vec![0.0, 1.0, 2.0],
    )
    .unwrap();
    wrong[2].pcurve = p.b.add_pcurve(long).unwrap();
    let before = counts(p.b.brep());
    let e = p.b.add_loop(face, true, &wrong).unwrap_err();
    assert_eq!(e, at(2, UseError::SameT));
    assert_eq!(counts(p.b.brep()), before);
    let mut gone = good.clone();
    gone[0].pcurve = PCurveId(9999);
    let e = p.b.add_loop(face, true, &gone).unwrap_err();
    assert_eq!(e, at(0, UseError::Id));
    let e = p.b.add_loop(FaceId(9), true, &good).unwrap_err();
    assert_eq!(e, BuildError::Store(StoreError::Id));

    p.b.add_loop(face, true, &good).unwrap();
    // A third use of the edge 0–1.
    let third = p.b.add_face(p.surface, false, key(2)).unwrap();
    let again = p.uses(&[0, 1, 3]);
    let before = counts(p.b.brep());
    let e = p.b.add_loop(third, true, &again).unwrap_err();
    assert_eq!(e, at(0, UseError::Third));
    assert_eq!(counts(p.b.brep()), before);
}

#[test]
fn a_singular_edge_takes_one_coedge() {
    let mut b = Builder::new(BodyKind::Solid, status());
    let s = b.add_surface(plane(), None, ExactRows::default()).unwrap();
    let pole = b
        .add_pole([0.0, 0.0, 1.0], VertTag::Pole(PoleEnd::N))
        .unwrap();
    assert_eq!(b.brep().vert_kind(pole), VertKind::Pole);
    let p = [0.0, 0.0, 1.0];
    let c = b.add_curve(line(p, p)).unwrap();
    let e = b.add_edge(c, [0.0, 1.0], [pole, pole]).unwrap();
    b.set_edge_kind(e, EdgeKind::Singular);
    let f = b.add_face(s, false, key(0)).unwrap();
    let use_ = |b: &mut Builder, reversed| Use {
        edge: e,
        reversed,
        pcurve: b.add_pcurve(uv_line([0.0, 1.0], [1.0, 1.0])).unwrap(),
        seam_bound: 0.0,
    };
    let u = use_(&mut b, false);
    b.add_loop(f, true, &[u]).unwrap();
    let g = b.add_face(s, false, key(1)).unwrap();
    let u = use_(&mut b, true);
    let e = b.add_loop(g, true, &[u]).unwrap_err();
    assert_eq!(
        e,
        BuildError::Use {
            index: 0,
            why: UseError::Third
        }
    );
}

#[test]
fn sides_and_shells_join_once() {
    let (mut p, [so, _], _) = tet();
    let f = p.faces[0];
    assert_eq!(
        p.b.add_shell(ShellKind::Open, vec![(f, Side::Out)]),
        Err(BuildError::Side(f, Side::Out))
    );
    p.b.clear_shells();
    assert_eq!(p.b.brep().face_shell(f, Side::Out), ShellId::NONE);
    assert_eq!(
        p.b.add_shell(ShellKind::Open, vec![(f, Side::In), (f, Side::In)]),
        Err(BuildError::Side(f, Side::In))
    );
    let s = p.b.add_shell(ShellKind::Open, vec![(f, Side::In)]).unwrap();
    assert_eq!(
        p.b.add_region(RegionKind::Empty, s, vec![s], RegionId::NONE),
        Err(BuildError::Shell(s))
    );
    // A removed shell can't be named.
    assert_eq!(
        p.b.add_region(RegionKind::Empty, so, vec![], RegionId::NONE),
        Err(BuildError::Store(StoreError::Id))
    );
    // The removed shells and regions go at the finish; the new one stays.
    let (b, map) = p.b.finish().unwrap();
    assert_eq!([b.shell_count(), b.region_count()], [1, 0]);
    assert_eq!(map.shells(s), ShellId(0));
    assert_eq!(map.shells(so), ShellId::NONE);
    assert_eq!(b.face_shell(map.faces(f), Side::In), ShellId(0));
    assert_eq!(b.shell_sides(ShellId(0)), [(FaceId(0), Side::In)]);
}

#[test]
fn removing_a_face_unpairs_and_compacts_in_order() {
    let (mut p, [so, si], _) = tet();
    // Face 1 (0 → 1 → 3) holds coedges 3..6, loop 1.
    let gone = p.faces[1];
    p.b.remove_face(gone).unwrap();
    {
        let b = p.b.brep();
        // Each edge of the removed face keeps its other coedge, unpaired.
        for (a, z) in [(0, 1), (1, 3), (0, 3)] {
            let e = p.edge(a, z);
            let c = b.edge_coedge(e);
            assert!(c.0 < 3 || c.0 >= 6, "{c:?} remains");
            assert_eq!(b.pair(c), CoedgeId::NONE);
        }
        assert!(!b.shell_sides(so).contains(&(gone, Side::Out)));
        assert!(!b.shell_sides(si).contains(&(gone, Side::In)));
    }
    assert_eq!(
        p.b.remove_face(gone),
        Err(BuildError::Store(StoreError::Id))
    );
    let old = p.b.brep().clone();
    let (b, map) = p.b.finish().unwrap();
    assert_eq!(counts(&b), [1, 6, 12, 4, 6, 9, 3, 3, 2, 2, 0]);
    assert_eq!(b.dangling(), None);
    // Ids move down in order; the removed face's map to none.
    let faces: Vec<FaceId> = (0..4).map(|f| map.faces(FaceId(f))).collect();
    assert_eq!(faces, [FaceId(0), FaceId::NONE, FaceId(1), FaceId(2)]);
    let coedges: Vec<u32> = (0..12).map(|c| map.coedges(CoedgeId(c)).0).collect();
    assert_eq!(coedges, [0, 1, 2, NONE, NONE, NONE, 3, 4, 5, 6, 7, 8]);
    // Every kept row reads as before, its ids mapped.
    for c in (0..12).map(CoedgeId).filter(|&c| !map.coedges(c).is_none()) {
        let n = map.coedges(c);
        assert_eq!(b.next(n), map.coedges(old.next(c)));
        assert_eq!(b.pair(n), map.coedges(old.pair(c)));
        assert_eq!(b.face(n), map.faces(old.face(c)));
        assert_eq!(b.coedge_loop(n), map.loops(old.coedge_loop(c)));
        assert_eq!(b.edge(n), old.edge(c));
        assert_eq!(b.seam_bound(n), old.seam_bound(c));
    }
    // Every vertex and edge still holds a coedge leaving (using) it.
    for v in 0..4 {
        let c = b.vert_coedge(VertId(v));
        assert_eq!(b.coedge_vert(c, false), VertId(v));
    }
    for e in 0..6 {
        assert_eq!(b.edge(b.edge_coedge(EdgeId(e))), EdgeId(e));
    }
    // The p-curves of the removed coedges stay until collected.
    assert_eq!(b.pcurve_count(), 12);
}

#[test]
fn a_loop_removed_and_added_again_retrims_its_face() {
    let mut p = square();
    let face = p.faces[0];
    p.b.remove_loop(p.loops[0]).unwrap();
    assert_eq!(p.b.brep().first_loop(face), LoopId::NONE);
    assert_eq!(
        p.b.remove_loop(p.loops[0]),
        Err(BuildError::Store(StoreError::Id))
    );
    let uses = p.uses(&[0, 1, 2, 3]);
    let lp = p.b.add_loop(face, true, &uses).unwrap();
    assert_eq!(p.b.brep().first_loop(face), lp);
    p.b.collect_geometry();
    let (b, map) = p.b.finish().unwrap();
    assert_eq!(counts(&b), [1, 4, 4, 4, 4, 4, 1, 1, 0, 0, 0]);
    assert_eq!(map.loops(lp), LoopId(0));
    assert_eq!(map.pcurves(PCurveId(0)), PCurveId::NONE);
    for v in 0..4 {
        assert_eq!(b.vert_coedge(VertId(v)).0, v);
    }
    assert_eq!(b.dangling(), None);
}

#[test]
fn a_removed_row_still_named_is_dangling() {
    let mut p = square();
    p.b.remove_vert(p.verts[2]).unwrap();
    assert_eq!(
        p.b.finish().unwrap_err(),
        BuildError::Dangling(Dangling {
            row: Row::Edge(EdgeId(1)),
            field: "v_end"
        })
    );

    let mut p = square();
    let e = p.edge(1, 2);
    p.b.remove_edge(e).unwrap();
    assert_eq!(
        p.b.finish().unwrap_err(),
        BuildError::Dangling(Dangling {
            row: Row::Coedge(CoedgeId(1)),
            field: "edge"
        })
    );

    // A vertex removed with its edges and their coedges' loop: nothing
    // names it.
    let mut p = square();
    p.b.remove_face(p.faces[0]).unwrap();
    for (a, z) in [(1, 2), (2, 3)] {
        let e = p.edge(a, z);
        p.b.remove_edge(e).unwrap();
    }
    p.b.remove_vert(p.verts[2]).unwrap();
    p.b.collect_geometry();
    let (b, map) = p.b.finish().unwrap();
    assert_eq!(counts(&b), [0, 2, 0, 3, 2, 0, 0, 0, 0, 0, 0]);
    assert_eq!(b.ends(map.edges(EdgeId(3))), [VertId(0), VertId(2)]);
    assert_eq!(b.vert_coedge(VertId(0)), CoedgeId::NONE);
}

#[test]
fn contacts_follow_their_members() {
    let (mut p, _, _) = tet();
    let e03 = p.edge(0, 3);
    let contact = |members| Contact {
        kind: ContactKind::Vertex,
        members,
        cert: ContactCert::default(),
    };
    p.b.add_contact(contact(vec![[
        Member::Vert(p.verts[3]),
        Member::EdgeAt { edge: e03, t: 0.5 },
    ]]));
    p.b.add_contact(contact(vec![[
        Member::Vert(p.verts[0]),
        Member::FaceAt {
            face: p.faces[1],
            uv: [0.25, 0.25],
        },
    ]]));
    p.b.remove_face(p.faces[0]).unwrap();
    // The second contact names face 1, now face 0.
    let (b, _) = p.b.clone().finish().unwrap();
    assert_eq!(
        b.contacts()[1].members[0][1],
        Member::FaceAt {
            face: FaceId(0),
            uv: [0.25, 0.25]
        }
    );
    // Its face removed, it dangles until dropped.
    p.b.remove_face(p.faces[1]).unwrap();
    assert_eq!(
        p.b.clone().finish().unwrap_err(),
        BuildError::Dangling(Dangling {
            row: Row::Contact(1),
            field: "members"
        })
    );
    p.b.retain_contacts(|c| {
        !c.members
            .iter()
            .flatten()
            .any(|m| matches!(m, Member::FaceAt { .. }))
    });
    let (b, _) = p.b.finish().unwrap();
    assert_eq!(b.contacts().len(), 1);
    assert_eq!(b.dangling(), None);
}

#[test]
fn geometry_is_collected_from_its_roots() {
    let mut p = square();
    let spare = |b: &mut Builder| b.add_surface(plane(), None, ExactRows::default()).unwrap();
    let (s1, s2, s3) = (spare(&mut p.b), spare(&mut p.b), spare(&mut p.b));
    let lone = p.b.add_curve(line([0.0; 3], [1.0; 3])).unwrap();
    let pc = p.b.add_pcurve(uv_line([0.0; 2], [1.0; 2])).unwrap();
    // An edge made by a cluster on s2 and s3 keeps them.
    let e = p.edge(0, 1);
    let cluster = ClusterOrigin {
        kind: ClusterKind::Near,
        sa: s2,
        sb: s3,
        sigma: Sign::Pos,
    };
    p.b.set_origin(e, Origin::default().moved(cluster));
    p.b.collect_geometry();
    let (b, map) = p.b.finish().unwrap();
    assert_eq!(counts(&b)[..3], [3, 4, 4]);
    assert_eq!(map.surfaces(s1), SurfId::NONE);
    assert_eq!([map.surfaces(s2), map.surfaces(s3)], [SurfId(1), SurfId(2)]);
    assert_eq!(map.curves(lone), CurveId::NONE);
    assert_eq!(map.pcurves(pc), PCurveId::NONE);
    let moved = ClusterOrigin {
        sa: SurfId(1),
        sb: SurfId(2),
        ..cluster
    };
    assert_eq!(b.origin(EdgeId(0)).clusters(), [moved]);
    assert_eq!(b.dangling(), None);
}

#[test]
fn seam_bounds_are_certified_in_one_pass() {
    let mut p = Poly::new(BodyKind::Sheet, &SQUARE);
    let face = p.b.add_face(p.surface, false, key(0)).unwrap();
    let mut uses = p.uses(&[0, 1, 2, 3]);
    // The p-curves in the plane's UV: `d·axis/CELL + 1/2` from its origin.
    let lat = Lattice::new(PlaneKey::new([0.0, 0.0, 1.0], [0.0; 3]).unwrap().into()).unwrap();
    let (o, [au, av, _]) = (lat.origin(), lat.axes());
    let uv = |x: [f64; 3]| {
        let along = |a: [f64; 3]| ((0..3).map(|k| (x[k] - o[k]) * a[k]).sum::<f64>()) / CELL + 0.5;
        [along(au), along(av)]
    };
    for u in &mut uses {
        let [a, z] = p.b.brep().ends(u.edge).map(|v| p.b.brep().pos(v));
        u.pcurve = p.b.add_pcurve(uv_line(uv(a), uv(z))).unwrap();
    }
    let items: Vec<(EdgeId, PCurveId)> = uses.iter().map(|u| (u.edge, u.pcurve)).collect();
    let mut work = Work::new(&Budget::new(1 << 20));
    let bounds = p.b.seam_bounds(p.surface, &items, 1e-6, &mut work).unwrap();
    assert_eq!(bounds.len(), 4);
    for (u, b) in uses.iter_mut().zip(bounds) {
        // A line in its plane.
        assert!((0.0..=1e-6).contains(&b), "{b}");
        u.seam_bound = b;
    }
    p.b.add_loop(face, true, &uses).unwrap();
    let mut none = Work::new(&Budget::new(0));
    assert!(matches!(
        p.b.seam_bounds(p.surface, &items, 1e-6, &mut none),
        Err(BuildError::OutOfBudget(_))
    ));
}

#[test]
fn dangling_ids_are_found_by_row_and_field() {
    let (p, _, _) = tet();
    let (mut b, _) = p.b.finish().unwrap();
    assert_eq!(b.dangling(), None);
    let found = |b: &Brep| b.dangling().map(|d| (d.row, d.field));
    let mut c = b.clone();
    c.coedges.next[5] = CoedgeId(12);
    assert_eq!(found(&c), Some((Row::Coedge(CoedgeId(5)), "next")));
    let mut c = b.clone();
    c.coedges.pcurve[2] = PCurveId::NONE;
    assert_eq!(found(&c), Some((Row::Coedge(CoedgeId(2)), "pcurve")));
    let mut c = b.clone();
    c.faces.shells[3][1] = ShellId(2);
    assert_eq!(found(&c), Some((Row::Face(FaceId(3)), "shells")));
    let mut c = b.clone();
    c.regions.islands[0].push(ShellId::NONE);
    assert_eq!(found(&c), Some((Row::Region(RegionId(0)), "islands")));
    // `NONE` is allowed where a field may name nothing.
    b.coedges.pair[0] = CoedgeId::NONE;
    b.verts.coedge[0] = CoedgeId::NONE;
    b.regions.container[1] = RegionId::NONE;
    assert_eq!(found(&b), None);
}
