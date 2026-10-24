//! The store on a square sheet face and on single rows: every column of
//! a kind as long as the kind, the links read back as added, the rows
//! refused for what they refer to, and the rules of the edge origins,
//! the tolerances and the touching report.

use varde_decide::num::storage::OpStart;

use super::*;
use crate::geom::form::PlaneKey;
use crate::geom::lattice::Lattice;
use crate::geom::pcurve::Span2;
use crate::infra::ops::{FeatureId, InstanceId};

fn status(eps: f64) -> Status {
    let start = OpStart {
        max_coord: 1.0,
        rho_op: 0.0,
    };
    Status::new(Tolerance::new(eps).expect("in range"), &start)
}

fn key(feature: u64, instance: u32, part: FacePart) -> FaceKey {
    FaceKey {
        feature: FeatureId(feature),
        part,
        instance: InstanceId(instance),
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

fn vert(pos: [f64; 3]) -> VertRow {
    VertRow {
        pos,
        tol: 0.0,
        kind: VertKind::Regular,
        coedge: CoedgeId::NONE,
        tag: None,
        attrs: AttrSetId::NONE,
    }
}

fn edge(curve: CurveId, v_start: VertId, v_end: VertId) -> EdgeRow {
    EdgeRow {
        curve,
        t_range: [0.0, 1.0],
        v_start,
        v_end,
        kind: EdgeKind::Regular,
        cont: Continuity::default(),
        origin: Origin::default(),
        tol: 0.0,
        dev: 0.0,
        embedded_join: false,
        coedge: CoedgeId::NONE,
        tag: None,
        attrs: AttrSetId::NONE,
    }
}

/// Every column of each kind holds a value per entity.
fn assert_aligned(b: &Brep) {
    let s = &b.surfaces;
    assert_eq!([s.fit.len(), s.exact_rows.len()], [b.surface_count(); 2]);
    let v = &b.verts;
    let n = b.vert_count();
    assert_eq!(
        [
            v.tol.len(),
            v.kind.len(),
            v.coedge.len(),
            v.tag.len(),
            v.attrs.len()
        ],
        [n; 5]
    );
    let e = &b.edges;
    let n = b.edge_count();
    assert_eq!(
        [
            e.t_range.len(),
            e.v_start.len(),
            e.v_end.len(),
            e.kind.len(),
            e.cont.len(),
            e.origin.len(),
            e.tol.len(),
            e.dev.len(),
            e.embedded_join.len(),
            e.coedge.len(),
            e.tag.len(),
            e.attrs.len(),
        ],
        [n; 12]
    );
    let c = &b.coedges;
    let n = b.coedge_count();
    assert_eq!(
        [
            c.reversed.len(),
            c.face.len(),
            c.lp.len(),
            c.next.len(),
            c.prev.len(),
            c.pair.len(),
            c.pcurve.len(),
            c.seam_bound.len(),
        ],
        [n; 8]
    );
    let l = &b.loops;
    let n = b.loop_count();
    assert_eq!(
        [l.first_coedge.len(), l.next_loop.len(), l.is_outer.len()],
        [n; 3]
    );
    let f = &b.faces;
    let n = b.face_count();
    assert_eq!(
        [
            f.flipped.len(),
            f.shells.len(),
            f.first_loop.len(),
            f.key.len(),
            f.attrs.len(),
        ],
        [n; 5]
    );
    let s = &b.shells;
    assert_eq!([s.sides.len(), s.region.len()], [b.shell_count(); 2]);
    let r = &b.regions;
    assert_eq!(
        [r.outer.len(), r.islands.len(), r.container.len()],
        [b.region_count(); 3]
    );
}

/// The unit square on `z = 0` as a sheet: one face, one loop of four
/// coedges, the edge from vertex 0 to vertex 3 used against its way, one
/// open shell holding both sides of the face.
struct Square {
    brep: Brep,
    verts: [VertId; 4],
    edges: [EdgeId; 4],
    coedges: [CoedgeId; 4],
    face: FaceId,
    lp: LoopId,
    shell: ShellId,
}

fn square() -> Square {
    let mut b = Brep::new(BodyKind::Sheet, status(1e-3));
    let surf = b
        .push_surface(SurfaceRow {
            surface: plane(),
            fit: None,
            exact_rows: ExactRows::default(),
        })
        .unwrap();
    let p = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let verts = p.map(|p| b.push_vert(vert(p)).unwrap());
    // Canonical: each edge from its lower vertex id to its higher.
    let ends = [(0, 1), (1, 2), (2, 3), (0, 3)];
    let edges = ends.map(|(i, j)| {
        let c = b.push_curve(line(p[i], p[j])).unwrap();
        b.push_edge(edge(c, verts[i], verts[j])).unwrap()
    });
    let uv = p.map(|p| [p[0], p[1]]);
    let first = CoedgeId(0);
    let face = FaceId(0);
    let lp = LoopId(0);
    let shell = ShellId(0);
    let coedges = [0, 1, 2, 3].map(|k| {
        let (i, j) = ends[k];
        let pc = b.push_pcurve(uv_line(uv[i], uv[j])).unwrap();
        b.push_coedge(CoedgeRow {
            edge: edges[k],
            reversed: k == 3,
            face,
            lp,
            next: CoedgeId(((k + 1) % 4) as u32),
            prev: CoedgeId(((k + 3) % 4) as u32),
            pair: CoedgeId::NONE,
            pcurve: pc,
            seam_bound: 0.0,
        })
        .unwrap()
    });
    for (k, &v) in verts.iter().enumerate() {
        b.verts.coedge[v.index()] = coedges[k];
    }
    for (k, &e) in edges.iter().enumerate() {
        b.edges.coedge[e.index()] = coedges[k];
    }
    assert_eq!(
        b.push_loop(LoopRow {
            face,
            first_coedge: first,
            next_loop: LoopId::NONE,
            is_outer: true,
        }),
        Ok(lp)
    );
    let face_row = FaceRow {
        surface: surf,
        flipped: false,
        shells: [shell; 2],
        first_loop: lp,
        key: key(7, 0, FacePart::Primitive(0)),
        attrs: AttrSetId::NONE,
    };
    assert_eq!(b.push_face(face_row), Ok(face));
    let shell_row = ShellRow {
        sides: vec![(face, Side::Out), (face, Side::In)],
        region: RegionId::NONE,
        kind: ShellKind::Open,
    };
    assert_eq!(b.push_shell(shell_row), Ok(shell));
    Square {
        brep: b,
        verts,
        edges,
        coedges,
        face,
        lp,
        shell,
    }
}

#[test]
fn a_square_sheet_reads_back_as_added() {
    let s = square();
    let b = &s.brep;
    assert_aligned(b);
    assert_eq!(b.kind(), BodyKind::Sheet);
    assert_eq!(
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
        ],
        [1, 4, 4, 4, 4, 4, 1, 1, 1, 0]
    );
    assert_eq!(b.loops(s.face).collect::<Vec<_>>(), [s.lp]);
    assert_eq!(b.loop_face(s.lp), s.face);
    assert!(b.is_outer(s.lp));
    assert_eq!(b.loop_coedges(s.lp).collect::<Vec<_>>(), s.coedges);
    // Round the loop, each coedge ends where the next starts, and the
    // loop visits the vertices in order.
    for &c in &s.coedges {
        assert_eq!(b.coedge_vert(c, true), b.coedge_vert(b.next(c), false));
        assert_eq!(b.prev(b.next(c)), c);
        assert_eq!(b.face(c), s.face);
        assert_eq!(b.coedge_loop(c), s.lp);
        assert_eq!(b.pair(c), CoedgeId::NONE);
        assert!(std::ptr::eq(b.coedge_pcurve(c), b.pcurve(b.pcurve_id(c))));
    }
    let starts: Vec<_> = s.coedges.iter().map(|&c| b.coedge_vert(c, false)).collect();
    assert_eq!(starts, s.verts);
    // The edge from 0 to 3 is used against its way.
    assert!(b.reversed(s.coedges[3]));
    assert_eq!(b.ends(s.edges[3]), [s.verts[0], s.verts[3]]);
    for &e in &s.edges {
        assert_eq!(b.edge_spans(e).len(), 1);
        assert!(!b.is_closed(e));
        assert_eq!(b.edge_kind(e), EdgeKind::Regular);
        assert_eq!(b.cont(e), Continuity::Crease(Convexity::Mixed));
        assert_eq!(b.origin(e), &Origin::Plain(PlainOrigin::Exact));
        assert!(!b.embedded_join(e));
    }
    assert_eq!(b.face_shell(s.face, Side::Out), s.shell);
    assert_eq!(b.face_shell(s.face, Side::In), s.shell);
    assert_eq!(
        b.shell_sides(s.shell),
        [(s.face, Side::Out), (s.face, Side::In)]
    );
    assert_eq!(b.shell_kind(s.shell), ShellKind::Open);
    assert_eq!(b.shell_region(s.shell), RegionId::NONE);
    assert_eq!(b.key(s.face), &key(7, 0, FacePart::Primitive(0)));
    assert_eq!(b.fit(b.face_surface_id(s.face)), None);
    assert!(b.contacts().is_empty());
    assert_eq!(b.eps().eps(), Tolerance::DEFAULT);
    assert!(!b.status().op().approximate());
    assert_eq!(b.status().touching().count(), 0);
}

#[test]
fn edges_are_refused_unless_canonical_on_knots_of_their_curve() {
    let mut s = square();
    let b = &mut s.brep;
    let [v0, v1, ..] = s.verts;
    let c = b.edge_curve_id(s.edges[0]);
    let refused = [
        (edge(c, v1, v0), StoreError::NotCanonical),
        (edge(CurveId(4), v0, v1), StoreError::Id),
        (edge(c, v0, VertId(4)), StoreError::Id),
        (edge(c, VertId::NONE, v1), StoreError::Id),
        (
            EdgeRow {
                t_range: [1.0, 0.0],
                ..edge(c, v0, v1)
            },
            StoreError::TRange,
        ),
        (
            EdgeRow {
                t_range: [0.0, 0.5],
                ..edge(c, v0, v1)
            },
            StoreError::TRange,
        ),
        // `−0` isn't the knot `+0`.
        (
            EdgeRow {
                t_range: [-0.0, 1.0],
                ..edge(c, v0, v1)
            },
            StoreError::TRange,
        ),
    ];
    for (row, why) in refused {
        assert_eq!(b.push_edge(row), Err(why));
    }
    assert_eq!(b.edge_count(), 4);
    assert_aligned(b);
    // A closed edge: both ends one vertex.
    let e = b.push_edge(edge(c, v1, v1)).unwrap();
    assert!(b.is_closed(e));
    assert_aligned(b);
}

#[test]
fn rows_are_refused_for_missing_geometry_or_lower_entities() {
    let mut s = square();
    let b = &mut s.brep;
    assert_eq!(
        b.push_vert(vert([0.0, f64::NAN, 0.0])),
        Err(StoreError::NotFinite)
    );
    assert_eq!(
        b.push_vert(vert([f64::INFINITY, 0.0, 0.0])),
        Err(StoreError::NotFinite)
    );
    let row = CoedgeRow {
        edge: s.edges[0],
        reversed: true,
        face: s.face,
        lp: LoopId::NONE,
        next: CoedgeId::NONE,
        prev: CoedgeId::NONE,
        pair: s.coedges[0],
        pcurve: PCurveId(4),
        seam_bound: 0.0,
    };
    assert_eq!(b.push_coedge(row), Err(StoreError::Id));
    let row = CoedgeRow {
        edge: EdgeId(4),
        pcurve: PCurveId(0),
        ..row
    };
    assert_eq!(b.push_coedge(row), Err(StoreError::Id));
    let row = FaceRow {
        surface: SurfId(1),
        flipped: false,
        shells: [ShellId::NONE; 2],
        first_loop: LoopId::NONE,
        key: key(1, 0, FacePart::Primitive(1)),
        attrs: AttrSetId::NONE,
    };
    assert_eq!(b.push_face(row), Err(StoreError::Id));
    assert_eq!(
        [b.vert_count(), b.coedge_count(), b.face_count()],
        [4, 4, 1]
    );
    assert_aligned(b);
}

#[test]
fn ids_stop_below_none() {
    assert_eq!(next_id(0), Ok(0));
    assert_eq!(next_id(NONE as usize - 1), Ok(NONE - 1));
    assert_eq!(next_id(NONE as usize), Err(StoreError::Full));
    assert_eq!(next_id(usize::MAX), Err(StoreError::Full));
}

#[test]
fn a_moved_join_keeps_its_prior_origin_and_each_cluster_once() {
    let near = ClusterOrigin {
        kind: ClusterKind::Near,
        sa: SurfId(2),
        sb: SurfId(5),
        sigma: Sign::Pos,
    };
    let contact = ClusterOrigin {
        kind: ClusterKind::Contact,
        sa: SurfId(1),
        sb: SurfId(3),
        sigma: Sign::Neg,
    };
    let exact = Origin::from(PlainOrigin::Exact);
    assert_eq!(exact.prior(), PlainOrigin::Exact);
    assert!(exact.clusters().is_empty());
    assert_eq!(exact.surfaces().count(), 0);

    let moved = exact.moved(near).moved(contact).moved(near);
    assert_eq!(
        moved,
        Origin::MovedJoin {
            prior: PlainOrigin::Exact,
            clusters: vec![near, contact],
        }
    );
    assert_eq!(
        moved.surfaces().collect::<Vec<_>>(),
        [SurfId(2), SurfId(5), SurfId(1), SurfId(3)]
    );

    // A cluster's own edge moved by another: the prior is the cluster's
    // origin, never a moved join, and its surfaces stay roots.
    let rim = Origin::from(PlainOrigin::Cluster(contact)).moved(near);
    assert_eq!(rim.prior(), PlainOrigin::Cluster(contact));
    assert_eq!(rim.clusters(), [near]);
    assert_eq!(
        rim.surfaces().collect::<Vec<_>>(),
        [SurfId(1), SurfId(3), SurfId(2), SurfId(5)]
    );
}

#[test]
fn max_tol_starts_at_half_eps_and_only_rises() {
    let eps = Tolerance::new(1e-3).unwrap();
    let mut t = Tolerances::new(eps);
    assert_eq!(t.eps(), eps);
    assert_eq!(t.max_tol(), 5e-4);
    assert!(t.admits(5e-4));
    assert!(!t.admits(5e-4_f64.next_up()));
    assert!(!t.is_wide(5e-4));
    assert!(t.is_wide(5e-4_f64.next_up()));
    assert!(!t.raise(1e-4));
    assert!(!t.raise(5e-4));
    assert_eq!(t.max_tol(), 5e-4);
    assert!(t.raise(7e-4));
    assert_eq!(t.max_tol(), 7e-4);
    assert!(t.admits(7e-4));
    // Wide is against `ε`, not the raised `max_tol`.
    assert!(t.is_wide(6e-4));
    // The body takes the tolerances of the operation that made it.
    let b = Brep::new(BodyKind::Solid, status(1e-2));
    assert_eq!(b.eps().max_tol(), 5e-3);
    assert_eq!(b.eps().eps(), b.status().op().eps());
}

#[test]
fn touching_lists_pairs_lesser_key_first_up_to_the_cap() {
    let mut t = Touching::default();
    let a = key(
        1,
        0,
        FacePart::Side {
            curve: 4,
            segment: 0,
        },
    );
    let b = key(2, 0, FacePart::StartCap { region: 0 });
    t.add(b, a);
    t.add(a, b);
    assert_eq!(t.pairs(), [(a, b), (a, b)]);
    for k in 0..2 * MAX_WARNINGS as u32 {
        t.add(a, key(3, k, FacePart::Primitive(0)));
    }
    assert_eq!(t.pairs().len(), MAX_WARNINGS);
    assert_eq!(t.count(), 2 + 2 * MAX_WARNINGS as u64);
    let mut s = BodyStatus::new(status(1e-3));
    s.touching_mut().add(a, b);
    assert_eq!(s.touching().pairs(), [(a, b)]);
}

#[test]
fn only_a_same_form_edge_marked_touch_is_a_touch() {
    assert!(EdgeKind::SameForm { touch: true }.is_touch());
    for k in [
        EdgeKind::SameForm { touch: false },
        EdgeKind::Regular,
        EdgeKind::Singular,
    ] {
        assert!(!k.is_touch());
    }
    assert_eq!(Side::Out.index(), 0);
    assert_eq!(Side::In.index(), 1);
}
