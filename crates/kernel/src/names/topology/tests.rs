//! User topology on the primitives and on flat sheets of squares: seams
//! and halves of one key inside one region; chains through vertices of
//! two chain edges; touching coplanar faces two regions in one flush
//! group, an overlap one region with an alias; contact edges never
//! transparent, a moved join not a contact edge; a key one region's
//! primary and another's alias; the same sets and names whatever the
//! body's ids; and the derivation charged.

use std::collections::BTreeMap;

use varde_decide::num::decide::Sign;
use varde_decide::num::storage::OpStart;

use super::*;
use crate::build::primitives::{Primitive, build};
use crate::geom::curve::Curve3D;
use crate::geom::form::PlaneKey;
use crate::geom::lattice::Lattice;
use crate::geom::pcurve::{PCurve, Span2};
use crate::geom::surface::Surface;
use crate::infra::budget::{Budget, Work};
use crate::infra::ops::{FeatureId, InstanceId, OpCtx, Status, Tolerance};
use crate::names::keys::FacePart;
use crate::topo::brep::{
    BodyKind, ClusterKind, ClusterOrigin, Contact, ContactCert, ContactKind, ExactRows, SurfId,
};
use crate::topo::build::{Builder, Use};

pub(crate) fn key(part: u32) -> FaceKey {
    FaceKey::new(
        OpCtx {
            feature: FeatureId(3),
            instance: InstanceId(1),
        },
        FacePart::Primitive(part),
    )
}

fn topology(b: &Brep) -> Topology {
    Topology::new(b, &Budget::DEFAULT).expect("within the budget")
}

pub(crate) fn primitive(p: &Primitive) -> Brep {
    let ctx = OpCtx::new(FeatureId(1));
    build(p, ctx, Tolerance::DEFAULT, &mut Work::new(&Budget::DEFAULT))
        .unwrap_or_else(|e| panic!("{p:?}: {e:?}"))
}

fn status() -> Status {
    let start = OpStart {
        max_coord: 8.0,
        rho_op: 0.0,
    };
    Status::new(Tolerance::new(1e-3).expect("in range"), &start)
}

fn line(a: [f64; 3], b: [f64; 3]) -> Curve3D {
    let m = [0, 1, 2].map(|k| (a[k] + b[k]) * 0.5);
    let pt = |p: [f64; 3]| [p[0], p[1], p[2], 1.0];
    Curve3D::new(vec![[pt(a), pt(m), pt(b)]], vec![0.0, 1.0]).expect("a line")
}

fn uv_line(a: [f64; 3], b: [f64; 3]) -> PCurve {
    let m = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    let span: Span2 = [[a[0], a[1], 1.0], [m[0], m[1], 1.0], [b[0], b[1], 1.0]];
    PCurve::new(vec![span], vec![0.0, 1.0]).expect("a UV line")
}

fn plane(z: f64) -> Surface {
    let key = PlaneKey::new([0.0, 0.0, 1.0], [0.0, 0.0, z]).unwrap();
    let lat = Lattice::new(key.into()).unwrap();
    Surface::new(lat.clone(), lat.half(0).unwrap()).unwrap()
}

/// A flat sheet of unit squares: square `(i, j)` of `cols × rows` at
/// `[i, i + 1] × [j, j + 1]`, keyed `keys[j][i]`, its surface the plane
/// at height `forms[j][i]` (squares of one height share one surface; the
/// corners stay at height 0, the user topology reading only forms); the
/// squares added in the order `order` (indices `j·cols + i`), each edge
/// when its first square is.
pub(crate) struct Grid {
    pub(crate) keys: Vec<Vec<FaceKey>>,
    pub(crate) forms: Vec<Vec<f64>>,
    pub(crate) order: Vec<usize>,
}

impl Grid {
    pub(crate) fn new(keys: Vec<Vec<FaceKey>>) -> Grid {
        let (rows, cols) = (keys.len(), keys[0].len());
        Grid {
            forms: vec![vec![0.0; cols]; rows],
            order: (0..rows * cols).collect(),
            keys,
        }
    }

    /// The sheet, and its edges by their two corners (lesser first).
    pub(crate) fn build(&self) -> (Brep, BTreeMap<[[i64; 2]; 2], EdgeId>) {
        let cols = self.keys[0].len();
        let mut b = Builder::new(BodyKind::Sheet, status());
        let mut surfaces: BTreeMap<u64, SurfId> = BTreeMap::new();
        let mut verts = BTreeMap::new();
        let mut edges = BTreeMap::new();
        for &k in &self.order {
            let (i, j) = ((k % cols) as i64, (k / cols) as i64);
            let z = self.forms[j as usize][i as usize];
            let s = *surfaces
                .entry(z.to_bits())
                .or_insert_with(|| b.add_surface(plane(z), None, ExactRows::default()).unwrap());
            let face = b
                .add_face(s, false, self.keys[j as usize][i as usize])
                .unwrap();
            let corners = [[i, j], [i + 1, j], [i + 1, j + 1], [i, j + 1]];
            let uses: Vec<Use> = (0..4)
                .map(|n| {
                    let (from, to) = (corners[n], corners[(n + 1) % 4]);
                    let mut vert = |c: [i64; 2]| {
                        *verts
                            .entry(c)
                            .or_insert_with(|| b.add_vert([c[0] as f64, c[1] as f64, 0.0]).unwrap())
                    };
                    let (vf, vt) = (vert(from), vert(to));
                    let (lo, hi) = if vf < vt { (vf, vt) } else { (vt, vf) };
                    let corner = if from < to { [from, to] } else { [to, from] };
                    let (a, c) = (b.brep().pos(lo), b.brep().pos(hi));
                    let e = *edges.entry(corner).or_insert_with(|| {
                        let curve = b.add_curve(line(a, c)).unwrap();
                        b.add_edge(curve, [0.0, 1.0], [lo, hi]).unwrap()
                    });
                    Use {
                        edge: e,
                        reversed: vf > vt,
                        pcurve: b.add_pcurve(uv_line(a, c)).unwrap(),
                        seam_bound: 0.0,
                    }
                })
                .collect();
            b.add_loop(face, true, &uses).unwrap();
        }
        (b.finish().unwrap().0, edges)
    }
}

/// The edge of `edges` between corners `a` and `b`.
pub(crate) fn between(edges: &BTreeMap<[[i64; 2]; 2], EdgeId>, a: [i64; 2], b: [i64; 2]) -> EdgeId {
    edges[&if a < b { [a, b] } else { [b, a] }]
}

/// What the topology says, read without ids: the regions' names, the
/// chains' names with their edge counts, kinds and strengths, the
/// corners' names and strengths, and the flush groups by their members'
/// primary keys.
type Named = (
    Vec<(FaceKey, Vec<FaceKey>, usize)>,
    Vec<(Between, usize, bool, bool, Strength)>,
    Vec<(Meet, Option<VertTag>, Strength)>,
    Vec<Vec<FaceKey>>,
);

fn named(t: &Topology) -> Named {
    let mut regions: Vec<_> = (t.regions().iter())
        .map(|r| (r.primary, r.aliases.clone(), r.faces.len()))
        .collect();
    regions.sort();
    let mut chains: Vec<_> = (t.chains().iter())
        .map(|c| {
            let n = (t.chain_name(c), c.edges.len(), c.closed, c.boundary);
            (n.0, n.1, n.2, n.3, c.strength)
        })
        .collect();
    chains.sort();
    let mut corners: Vec<_> = (t.corners().iter())
        .map(|c| (t.corner_name(c), c.tag, c.strength))
        .collect();
    corners.sort();
    let mut flush: Vec<Vec<FaceKey>> = (t.flush_groups().iter())
        .map(|g| {
            (g.regions.iter())
                .map(|&r| t.regions()[r as usize].primary)
                .collect()
        })
        .collect();
    flush.sort();
    (regions, chains, corners, flush)
}

#[test]
fn halves_and_quarters_of_one_key_are_one_region_across_their_seams() {
    let sphere = primitive(&Primitive::Sphere {
        centre: [10.0, -20.0, 5.0],
        radius: 7.0,
    });
    let t = topology(&sphere);
    assert_eq!(t.regions().len(), 1);
    assert_eq!(t.regions()[0].faces.len(), 2);
    assert!(t.regions()[0].aliases.is_empty());
    assert!(t.chains().is_empty());
    // Its poles are its corners, strong, though one region meets there.
    assert_eq!(t.corners().len(), 2);
    for c in t.corners() {
        assert!(matches!(c.tag, Some(VertTag::Pole(_))));
        assert_eq!((c.regions.len(), c.strength), (1, Strength::Strong));
    }
    for e in 0..sphere.edge_count() {
        let e = EdgeId(e as u32);
        if sphere.edge_tag(e) == Some(EdgeTag::Seam) {
            assert_eq!(t.edge_class(e), Some(EdgeClass::Transparent));
        }
    }
    // A ring torus's four quarters: one region, no chain, no corner.
    let torus = primitive(&Primitive::Torus {
        centre: [3.0, 1.0, -2.0],
        major: 6.0,
        minor: 2.5,
    });
    let t = topology(&torus);
    assert_eq!(t.regions().len(), 1);
    assert_eq!(t.regions()[0].faces.len(), 4);
    assert!(t.chains().is_empty() && t.corners().is_empty());
}

#[test]
fn a_cylinders_rims_are_closed_chains_through_its_seam_vertices() {
    let b = primitive(&Primitive::Cylinder {
        base: [1.5, -2.0, 3.0],
        radius: 4.0,
        height: 9.5,
    });
    let t = topology(&b);
    // The bottom, the top and the wall's two halves as one.
    assert_eq!(t.regions().len(), 3);
    let wall = (t.regions().iter())
        .position(|r| r.faces.len() == 2)
        .expect("the wall") as u32;
    assert_eq!(t.chains().len(), 2);
    for c in t.chains() {
        assert!(c.closed && c.regions.contains(&wall));
        assert_eq!((c.edges.len(), c.strength), (2, Strength::Strong));
    }
    assert!(t.corners().is_empty());
    assert_eq!(t.flush_groups().len(), 3);
}

#[test]
fn a_boxs_faces_edges_and_corners_are_its_regions_chains_and_corners() {
    let b = primitive(&Primitive::Cuboid {
        min: [1.0, -2.0, 0.5],
        size: [3.0, 5.0, 6.75],
    });
    let t = topology(&b);
    assert_eq!(
        [t.regions().len(), t.chains().len(), t.corners().len()],
        [6, 12, 8]
    );
    assert!(t.chains().iter().all(|c| !c.closed && c.edges.len() == 1));
    for c in t.corners() {
        assert_eq!((c.regions.len(), c.strength), (3, Strength::Strong));
        assert_eq!(t.corner_name(c).keys().len(), 3);
    }
    // Regions in the leaf order of their keys.
    assert!(t.regions().is_sorted_by_key(|r| r.primary));
    for (i, r) in t.regions().iter().enumerate() {
        assert_eq!(
            r.faces.iter().map(|&f| t.region_of(f)).collect::<Vec<_>>(),
            [Some(i as u32)]
        );
        assert_eq!(t.flush_groups()[r.flush as usize].regions, [i as u32]);
    }
}

/// Two squares side by side, keyed `a` and `b`, and their shared edge.
pub(crate) fn pair(a: FaceKey, b: FaceKey) -> (Brep, EdgeId) {
    let (body, edges) = Grid::new(vec![vec![a, b]]).build();
    let e = between(&edges, [1, 0], [1, 1]);
    (body, e)
}

pub(crate) fn edited(body: Brep, f: impl FnOnce(&mut Builder)) -> Brep {
    let mut b = Builder::edit(body);
    f(&mut b);
    b.finish().unwrap().0
}

#[test]
fn touching_coplanar_faces_are_two_regions_in_one_flush_group() {
    let (body, e) = pair(key(4), key(2));
    let body = edited(body, |b| {
        b.set_edge_kind(e, EdgeKind::SameForm { touch: true })
    });
    let t = topology(&body);
    assert_eq!(t.edge_class(e), Some(EdgeClass::Contact));
    assert_eq!(t.regions().len(), 2);
    assert_eq!(t.flush_groups().len(), 1);
    // Members in the leaf order of their primary keys.
    let members: Vec<FaceKey> = (t.flush_groups()[0].regions.iter())
        .map(|&r| t.regions()[r as usize].primary)
        .collect();
    assert_eq!(members, [key(2), key(4)]);
    assert!(t.regions().iter().all(|r| r.flush == 0));
    // The contact edge is a weak chain of its own.
    let c = &t.chains()[t.chain_of(e).expect("a chain") as usize];
    assert_eq!(
        (c.edges.len(), c.contact, c.strength),
        (1, true, Strength::Weak)
    );
    assert_eq!(t.chain_name(c), Between::new(key(2), key(4)));
    // Two regions meet at its ends: no corner. Each region's open
    // boundary is a strong chain.
    assert!(t.corners().is_empty());
    let boundary: Vec<&Chain> = t.chains().iter().filter(|c| c.boundary).collect();
    assert_eq!(boundary.len(), 2);
    assert!(boundary.iter().all(|c| c.strength == Strength::Strong));
}

#[test]
fn a_co_oriented_overlap_is_one_region_its_greater_key_an_alias() {
    let (body, e) = pair(key(4), key(2));
    let body = edited(body, |b| {
        b.set_edge_kind(e, EdgeKind::SameForm { touch: false })
    });
    let t = topology(&body);
    assert_eq!(t.edge_class(e), Some(EdgeClass::Transparent));
    assert_eq!(t.regions().len(), 1);
    let r = &t.regions()[0];
    assert_eq!((r.primary, &r.aliases[..]), (key(2), &[key(4)][..]));
    assert!(r.named(&key(4)) && r.named(&key(2)) && !r.named(&key(3)));
    // Its boundary: one closed chain of six edges.
    assert_eq!(t.chains().len(), 1);
    assert!(t.chains()[0].closed && t.chains()[0].boundary);
    assert_eq!(t.chains()[0].edges.len(), 6);
}

#[test]
fn edges_between_different_keys_stay_chains_even_tangent() {
    for cont in [
        Continuity::Crease(crate::topo::brep::Convexity::Convex),
        Continuity::Tangent { angle: 0.0 },
        Continuity::Curvature,
    ] {
        let (body, e) = pair(key(1), key(2));
        let body = edited(body, |b| b.set_cont(e, cont));
        let t = topology(&body);
        assert_eq!(t.edge_class(e), Some(EdgeClass::Crease), "{cont:?}");
        assert_eq!(t.regions().len(), 2);
        assert_eq!(t.flush_groups().len(), 2);
        let c = &t.chains()[t.chain_of(e).unwrap() as usize];
        assert_eq!(c.strength, Strength::Strong);
    }
}

#[test]
fn one_key_edges_are_transparent_on_one_form_or_smooth_and_else_weak_chains() {
    // One key, one form: transparent whatever the crease.
    let (body, e) = pair(key(1), key(1));
    assert_eq!(topology(&body).edge_class(e), Some(EdgeClass::Transparent));
    // One key on two forms: transparent only if tangent or
    // curvature-continuous, a cluster's rim or tagged a seam.
    let mut grid = Grid::new(vec![vec![key(1), key(1)]]);
    grid.forms = vec![vec![0.0, 1.0]];
    let (other, edges) = grid.build();
    let e = between(&edges, [1, 0], [1, 1]);
    let t = topology(&other);
    assert_eq!(t.edge_class(e), Some(EdgeClass::Crease));
    assert_eq!(t.regions().len(), 2);
    let c = &t.chains()[t.chain_of(e).unwrap() as usize];
    assert_eq!(c.strength, Strength::Weak, "a chain between one key");
    for smooth in [
        |b: &mut Builder, e| b.set_cont(e, Continuity::Tangent { angle: 1e-9 }),
        |b: &mut Builder, e| b.set_cont(e, Continuity::Curvature),
        |b: &mut Builder, e| b.set_origin(e, rim(ClusterKind::Near).into()),
        |b: &mut Builder, e| b.set_edge_tag(e, Some(EdgeTag::Seam)),
    ] as [fn(&mut Builder, EdgeId); 4]
    {
        let body = edited(other.clone(), |b| smooth(b, e));
        let t = topology(&body);
        assert_eq!(t.edge_class(e), Some(EdgeClass::Transparent));
        assert_eq!(t.regions().len(), 1);
    }
}

fn rim(kind: ClusterKind) -> PlainOrigin {
    PlainOrigin::Cluster(ClusterOrigin {
        kind,
        sa: SurfId(0),
        sb: SurfId(0),
        sigma: Sign::Pos,
    })
}

#[test]
fn contact_edges_are_never_transparent_and_a_moved_join_is_none() {
    let same_form = EdgeKind::SameForm { touch: false };
    for kind in [ClusterKind::Near, ClusterKind::Contact] {
        let (body, e) = pair(key(1), key(2));
        let body = edited(body, |b| {
            b.set_edge_kind(e, same_form);
            b.set_origin(e, rim(kind).into());
        });
        let t = topology(&body);
        assert_eq!(t.edge_class(e), Some(EdgeClass::Contact), "{kind:?}");
        // Not touching: two regions, two flush groups.
        assert_eq!(t.flush_groups().len(), 2);
    }
    // A contact record's member edge.
    let (body, e) = pair(key(1), key(2));
    let body = edited(body, |b| {
        b.set_edge_kind(e, same_form);
        b.add_contact(Contact {
            kind: ContactKind::Edge,
            members: vec![[Member::Edge(e), Member::Edge(e)]],
            cert: ContactCert::default(),
        });
    });
    assert_eq!(topology(&body).edge_class(e), Some(EdgeClass::Contact));
    // A moved join's clusters only moved its ends: an ordinary edge.
    let (body, e) = pair(key(1), key(2));
    let cluster = match rim(ClusterKind::Contact) {
        PlainOrigin::Cluster(c) => c,
        _ => unreachable!(),
    };
    let body = edited(body, |b| {
        b.set_edge_kind(e, same_form);
        b.set_origin(e, Origin::from(PlainOrigin::Exact).moved(cluster));
    });
    let t = topology(&body);
    assert_eq!(t.edge_class(e), Some(EdgeClass::Transparent));
    assert_eq!(t.regions().len(), 1);
}

/// A row of four squares: a groove (key 7) cuts a face of key 5 in two;
/// the right half is joined to a coplanar face of key 2 across an
/// ordinary same-form edge.
pub(crate) fn grooved() -> (Brep, BTreeMap<[[i64; 2]; 2], EdgeId>) {
    let (body, edges) = Grid::new(vec![vec![key(5), key(7), key(5), key(2)]]).build();
    let e = between(&edges, [3, 0], [3, 1]);
    let body = edited(body, |b| {
        b.set_edge_kind(e, EdgeKind::SameForm { touch: false })
    });
    (body, edges)
}

#[test]
fn a_key_may_be_one_regions_primary_and_anothers_alias() {
    let (body, _) = grooved();
    let t = topology(&body);
    let names: Vec<(FaceKey, Vec<FaceKey>)> = (t.regions().iter())
        .map(|r| (r.primary, r.aliases.clone()))
        .collect();
    assert_eq!(
        names,
        [(key(2), vec![key(5)]), (key(5), vec![]), (key(7), vec![]),]
    );
}

#[test]
fn chains_run_through_vertices_where_two_chain_edges_meet() {
    // Two rows: the top of key 1, the bottom of key 2, each row's squares
    // joined across same-form edges.
    let (body, edges) = Grid::new(vec![vec![key(2), key(2)], vec![key(1), key(1)]]).build();
    let body = edited(body, |b| {
        for e in [
            between(&edges, [1, 0], [1, 1]),
            between(&edges, [1, 1], [1, 2]),
        ] {
            b.set_edge_kind(e, EdgeKind::SameForm { touch: false });
        }
    });
    let t = topology(&body);
    assert_eq!(t.regions().len(), 2);
    // The middle line, through the middle vertex: one open chain.
    let mid = t.chain_of(between(&edges, [0, 1], [1, 1])).unwrap();
    assert_eq!(t.chain_of(between(&edges, [1, 1], [2, 1])), Some(mid));
    let c = &t.chains()[mid as usize];
    assert_eq!((c.edges.len(), c.closed, c.boundary), (2, false, false));
    // Each region's boundary, from one end of the middle line to the
    // other, through the corners of the sheet.
    assert_eq!(t.chains().len(), 3);
    for c in t.chains().iter().filter(|c| c.boundary) {
        assert_eq!((c.edges.len(), c.closed), (4, false));
    }
}

#[test]
fn the_topology_does_not_depend_on_the_bodys_ids() {
    let keys = vec![
        vec![key(5), key(7), key(5), key(2)],
        vec![key(1), key(1), key(9), key(9)],
    ];
    let mark = |body: Brep, edges: &BTreeMap<[[i64; 2]; 2], EdgeId>| {
        edited(body, |b| {
            b.set_edge_kind(
                between(edges, [3, 0], [3, 1]),
                EdgeKind::SameForm { touch: false },
            );
            b.set_edge_kind(
                between(edges, [1, 1], [1, 2]),
                EdgeKind::SameForm { touch: false },
            );
            b.set_edge_kind(
                between(edges, [3, 1], [3, 2]),
                EdgeKind::SameForm { touch: false },
            );
            b.set_edge_kind(
                between(edges, [2, 1], [3, 1]),
                EdgeKind::SameForm { touch: true },
            );
        })
    };
    let mut grid = Grid::new(keys);
    let (body, edges) = grid.build();
    let first = named(&topology(&mark(body, &edges)));
    assert_eq!(first.0.len(), 5, "{first:?}");
    for order in [
        vec![7, 6, 5, 4, 3, 2, 1, 0],
        vec![3, 0, 6, 1, 7, 2, 4, 5],
        vec![5, 2, 7, 0, 4, 1, 3, 6],
    ] {
        grid.order = order;
        let (body, edges) = grid.build();
        assert_eq!(named(&topology(&mark(body, &edges))), first);
    }
}

#[test]
fn deriving_is_charged() {
    let b = primitive(&Primitive::Cuboid {
        min: [0.0; 3],
        size: [1.0; 3],
    });
    // Six faces, twelve edges, eight vertices, twenty-four coedges.
    let units = 6 + 12 + 8 + 24;
    assert!(Topology::new(&b, &Budget::new(units)).is_ok());
    let short = Topology::new(&b, &Budget::new(units - 1));
    assert!(
        matches!(short, Err(KernelError::OutOfBudget { .. })),
        "{short:?}"
    );
}
