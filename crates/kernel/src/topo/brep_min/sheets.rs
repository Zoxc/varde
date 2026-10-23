//! The snap's sheet fixtures ([`varde_testkit::sheets`]) as stores: the
//! flat pairs built by [`Brep::from_polygons`], the smooth pair's
//! cylinder as a one-face sheet on the cylinder's lattice and its
//! biquadratic sheet as a [`Tile`] (the store holds no free-form
//! surface yet). Every body passes the planar check, relabelled too;
//! each fault the check names is found on a store broken for it.

use varde_decide::num::exact::Runs;
use varde_decide::num::schedule::{PredKind, UNIT_PARTS, UNITS};
use varde_decide::num::trig;
use varde_testkit::rng::Rng;
use varde_testkit::sheets::{self, Diagonals, OFFSET, Pair, Polygons, SmoothPair, Wavy};

use super::planar::{CheckFault, FaceError, PolygonError, line, plane_uv, uv_line};
use super::*;
use crate::geom::curve::{Span2, Span3};
use crate::geom::form::{CylinderKey, FormKey};
use crate::geom::lattice::{CELL, Lattice};
use crate::geom::tile::Tile;
use crate::infra::budget::{Budget, Work};

/// The fixtures' seam target (mm): a quarter of a tolerance of `10⁻⁵`.
pub(crate) const TARGET: f64 = 2.5e-6;

const ROOT_HALF: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// The store of a fixture body: a solid if it is closed, else a sheet.
pub(crate) fn store(p: &Polygons) -> Brep {
    let kind = if p.closed {
        BodyKind::Solid
    } else {
        BodyKind::Sheet
    };
    Brep::from_polygons(
        kind,
        &p.verts,
        &p.faces,
        TARGET,
        &mut Work::new(&Budget::DEFAULT),
    )
    .unwrap_or_else(|e| panic!("{e:?}"))
}

/// The smooth pair built: the cylinder's upper half over the pair's
/// axial range as a one-face sheet facing out, and the biquadratic
/// sheet's tile.
pub(crate) struct Smooth {
    pub(crate) data: SmoothPair,
    pub(crate) cylinder: Brep,
    pub(crate) sheet: Tile,
}

/// The smooth pair with the least gap `gap` (mm), built.
pub(crate) fn smooth(gap: f64) -> Smooth {
    let data = sheets::smooth(gap);
    let key = CylinderKey::new(data.axis_point, data.axis, data.radius).expect("a cylinder");
    let lattice = Lattice::new(key.into()).expect("a lattice");
    let [e1, e2, a] = lattice.axes();
    // Angle cells −1 and 0: from −e₂ through e₁ to e₂.
    let rect = lattice.half(-1).expect("a rectangle");
    let foot = key.foot();
    let r = key.radius();
    let at = |dir: [f64; 3], z: f64| [0, 1, 2].map(|k| foot[k] + r * dir[k] + a[k] * z);
    let m2 = e2.map(|x| -x);
    let [z0, z1] = data.along;
    let mut b = Brep::with_kind(BodyKind::Sheet);
    let s = b
        .add_surface(Surface::new(lattice, rect).expect("a surface"))
        .expect("a surface id");
    let pts = [at(m2, z0), at(e2, z0), at(m2, z1), at(e2, z1)];
    let vs = pts.map(|p| b.add_vert(p).expect("a vertex"));
    let pt = |p: [f64; 3], w: f64| [p[0], p[1], p[2], w];
    // Two quarter circles at axial height `z`: −e₂ to e₁ to e₂.
    let arc = |z: f64| {
        let quarter = |from: [f64; 3], to: [f64; 3]| -> Span3 {
            let corner = [0, 1, 2].map(|k| foot[k] + r * (from[k] + to[k]) + a[k] * z);
            [
                pt(at(from, z), 1.0),
                pt(corner, ROOT_HALF),
                pt(at(to, z), 1.0),
            ]
        };
        Curve3D::new(vec![quarter(m2, e1), quarter(e1, e2)], vec![0.0, 1.0, 2.0]).expect("an arc")
    };
    let ruling = |p: [f64; 3], q: [f64; 3]| {
        let m = [0, 1, 2].map(|k| 0.5 * (p[k] + q[k]));
        Curve3D::new(vec![[pt(p, 1.0), pt(m, 1.0), pt(q, 1.0)]], vec![0.0, 1.0]).expect("a line")
    };
    let curves = [
        (arc(z0), 0, 1),
        (arc(z1), 2, 3),
        (ruling(pts[0], pts[2]), 0, 2),
        (ruling(pts[1], pts[3]), 1, 3),
    ];
    let edges = curves.map(|(c, s, e)| {
        let t1 = c.knots()[c.knots().len() - 1];
        let id = b.add_curve(c).expect("a curve id");
        b.add_edge(id, [0.0, t1], vs[s], vs[e]).expect("an edge")
    });
    // The axial cell 0 is centred on the foot point: `v = z/CELL + ½`.
    let (lo, hi) = (0.5 + z0 / CELL, 0.5 + z1 / CELL);
    let uv = |a: [f64; 2], b: [f64; 2]| -> Span2 {
        let m = [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1])];
        [[a[0], a[1], 1.0], [m[0], m[1], 1.0], [b[0], b[1], 1.0]]
    };
    let arc_uv = |v: f64| {
        PCurve::new(
            vec![uv([-1.0, v], [0.0, v]), uv([0.0, v], [1.0, v])],
            vec![0.0, 1.0, 2.0],
        )
        .expect("a p-curve")
    };
    let ruling_uv =
        |u: f64| PCurve::new(vec![uv([u, lo], [u, hi])], vec![0.0, 1.0]).expect("a p-curve");
    let [arc0, arc1, rul0, rul1] = edges;
    let face = b.add_face(s, false).expect("a face");
    // Counter-clockwise in UV: the cylinder's outward normal.
    b.add_loop(
        face,
        true,
        vec![
            (arc0, false, arc_uv(lo)),
            (rul1, false, ruling_uv(1.0)),
            (arc1, true, arc_uv(hi)),
            (rul0, true, ruling_uv(-1.0)),
        ],
        TARGET,
        &mut Work::new(&Budget::DEFAULT),
    )
    .expect("the cylinder's loop");
    let sheet = Tile::new(data.net).expect("the sheet's tile");
    Smooth {
        data,
        cylinder: b,
        sheet,
    }
}

/// The flat pairs, named.
fn flat() -> Vec<(&'static str, Pair)> {
    let inset = Wavy {
        inset: 2,
        diagonals_a: Diagonals::Alternating,
        ..Wavy::DEFAULT
    };
    vec![
        ("wavy", sheets::wavy(&Wavy::DEFAULT)),
        ("wavy inset", sheets::wavy(&inset)),
        ("box-edge fold", sheets::box_edge_fold(OFFSET)),
        ("L-corner", sheets::l_corner(OFFSET)),
        ("L-corner quad", sheets::l_corner_quad(OFFSET)),
    ]
}

/// Builds and checks `p`: its kind, its counts and the planar check,
/// every face's trims checked.
fn built_and_checked(name: &str, p: &Polygons) -> Brep {
    let b = store(p);
    assert_eq!(b.kind() == BodyKind::Solid, p.closed, "{name}");
    assert_eq!(b.vert_count(), p.verts.len(), "{name}");
    assert_eq!(b.face_count(), p.faces.len(), "{name}");
    assert_eq!(b.edge_count(), p.edges().len(), "{name}");
    let checked = b.check_planar().unwrap_or_else(|f| panic!("{name}: {f:?}"));
    assert!(checked.unchecked_trims.is_empty(), "{name}: {checked:?}");
    b
}

#[test]
fn flat_fixtures_pass_the_planar_check() {
    for (name, pair) in flat() {
        for p in [&pair.a, &pair.b] {
            let b = built_and_checked(name, p);
            // Open boundary coedges are exactly the edges one face uses.
            let open = (0..b.coedge_count())
                .filter(|&c| b.pair(CoedgeId(c as u32)) == CoedgeId::NONE)
                .count();
            let once = p.edges().iter().filter(|(_, n)| *n == 1).count();
            assert_eq!(open, once, "{name}");
            // Every seam bound reaches the target: the planes' and the
            // UVs' rounding only.
            for c in 0..b.coedge_count() {
                let s = b.seam_bound(CoedgeId(c as u32));
                assert!(s <= TARGET, "{name}: coedge {c} seam {s:e}");
            }
        }
    }
}

#[test]
fn relabelled_fixtures_pass_the_planar_check() {
    let mut rng = Rng::new(11);
    for (name, pair) in flat() {
        for p in [&pair.a, &pair.b] {
            for _ in 0..varde_testing::pick(2, 8) {
                built_and_checked(name, &p.relabelled(&mut rng));
            }
        }
    }
}

/// The fixtures' faces lie where the testkit says: the faces of a pair
/// face the same way where they run within the offset (a wavy pair's,
/// the L-corner quad's), each face's outward side from its loop.
#[test]
fn faces_face_out() {
    let up = |b: &Brep, f: usize| {
        let FormKey::Plane(k) = b.surface(FaceId(f as u32)).lattice().key() else {
            panic!("a plane")
        };
        let n = k.normal();
        if b.flipped(FaceId(f as u32)) {
            -n[2]
        } else {
            n[2]
        }
    };
    for (name, pair) in [
        ("wavy", sheets::wavy(&Wavy::DEFAULT)),
        ("L-corner quad", sheets::l_corner_quad(OFFSET)),
    ] {
        let (a, b) = (store(&pair.a), store(&pair.b));
        for s in [&a, &b] {
            for f in 0..s.face_count() {
                assert!(up(s, f) > 0.0, "{name}: face {f}");
            }
        }
    }
}

#[test]
fn the_smooth_pair_passes_the_planar_check() {
    let s = smooth(1e-4);
    let checked = s
        .cylinder
        .check_planar()
        .unwrap_or_else(|f| panic!("{f:?}"));
    // The cylinder's trims are straight in UV: checked too.
    assert!(checked.unchecked_trims.is_empty(), "{checked:?}");
    assert_eq!(s.cylinder.kind(), BodyKind::Sheet);
    for c in 0..s.cylinder.coedge_count() {
        assert_eq!(s.cylinder.pair(CoedgeId(c as u32)), CoedgeId::NONE);
        assert!(s.cylinder.seam_bound(CoedgeId(c as u32)) <= TARGET);
    }
    // The lattice's frame puts e₁ up, so the face is the upper half.
    let axes = s.cylinder.surface(FaceId(0)).lattice().axes();
    assert_eq!(axes[0], [0.0, 0.0, 1.0]);
    // The sheet is regular, its normals up, and lies outside the
    // cylinder at its corners and middle, `gap` above it there.
    let cone = s.sheet.normal_cone().expect("a regular sheet");
    assert!(cone.axis[2] > 0.0, "{cone:?}");
    let key = s.cylinder.surface(FaceId(0)).lattice().key().clone();
    for (u, v) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (0.5, 0.5)] {
        let p = s.sheet.eval(u, v).map(|b| b.v);
        let exact = p.map(|x| malachite::Rational::try_from(x).expect("finite"));
        assert!(key.implicit(&exact).expect("an implicit") > 0u32, "{p:?}");
    }
    let mid = s.sheet.eval(0.5, 0.5);
    assert!(
        (mid[2].v - (s.data.radius + s.data.gap)).abs() <= 1e-12,
        "{mid:?}"
    );
}

/// The first vertex and face of a sheet whose two triangles meet at one
/// vertex only.
fn bow_tie() -> Polygons {
    Polygons {
        closed: false,
        verts: vec![
            [0.0, 0.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
            [-1.0, -1.0, 0.0],
        ],
        faces: vec![vec![0, 1, 2], vec![0, 3, 4]],
    }
}

#[test]
fn faults_are_found_with_their_entities() {
    let quad = sheets::l_corner_quad(OFFSET).b;
    // A sheet's faces stored as a solid: open coedges.
    let mut solid = store(&quad);
    solid.kind = BodyKind::Solid;
    assert_eq!(solid.check_planar(), Err(CheckFault::Open(CoedgeId(0))));
    // Two fans at a vertex.
    assert_eq!(
        store(&bow_tie()).check_planar(),
        Err(CheckFault::Fan(VertId(0)))
    );
    // A pair link broken.
    let mut b = store(&sheets::box_edge_fold(OFFSET).a);
    let p = b.pair(CoedgeId(0));
    b.coedges.pair[p.index()] = CoedgeId::NONE;
    assert_eq!(b.check_planar(), Err(CheckFault::Pair(CoedgeId(0))));
    // An edge no face uses.
    let mut b = store(&quad);
    let curve = b.curves[0].clone();
    let curve = b.add_curve(curve).unwrap();
    let [v0, v1] = b.ends(EdgeId(0));
    let e = b.add_edge(curve, [0.0, 1.0], v0, v1).unwrap();
    assert_eq!(b.check_planar(), Err(CheckFault::Unused(e)));
    // `prev` broken.
    let mut b = store(&quad);
    b.coedges.prev[1] = CoedgeId(2);
    assert_eq!(b.check_planar(), Err(CheckFault::NextPrev(CoedgeId(0))));
    // A face whose normal is turned against its loop.
    let mut b = store(&quad);
    b.faces.flipped[0] = !b.faces.flipped[0];
    assert_eq!(b.check_planar(), Err(CheckFault::Orientation(LoopId(0))));
    // A loop moved off its face's tile, joins kept.
    let mut b = store(&quad);
    for p in &mut b.coedges.pcurve {
        let spans = p
            .spans()
            .iter()
            .map(|s| s.map(|q| [q[0] + 1.0, q[1], q[2]]))
            .collect();
        *p = PCurve::new(spans, p.knots().to_vec()).unwrap();
    }
    assert_eq!(b.check_planar(), Err(CheckFault::Domain(CoedgeId(0))));
    // A seam bound above its edge's tolerance.
    let mut b = store(&quad);
    b.coedges.seam_bound[2] = 1.0;
    assert_eq!(b.check_planar(), Err(CheckFault::Seam(CoedgeId(2))));
    // A self-crossing (bow-tie) quad, planar.
    let twisted = Polygons {
        closed: false,
        verts: vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
        ],
        faces: vec![vec![0, 1, 2, 3]],
    };
    // Its Newell normal is zero: no plane.
    assert!(matches!(
        Brep::from_polygons(
            BodyKind::Sheet,
            &twisted.verts,
            &twisted.faces,
            TARGET,
            &mut Work::new(&Budget::DEFAULT)
        ),
        Err(PolygonError::Face {
            face: 0,
            why: FaceError::Plane(_)
        })
    ));
    // Unbalanced, it has a plane, and crosses itself.
    let twisted = Polygons {
        verts: vec![
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
        ],
        ..twisted
    };
    assert_eq!(
        store(&twisted).check_planar(),
        Err(CheckFault::Crossing(FaceId(0)))
    );
    // A spike folding back on itself.
    let spike = Polygons {
        closed: false,
        verts: vec![
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        ],
        faces: vec![vec![0, 1, 2, 3]],
    };
    assert_eq!(
        store(&spike).check_planar(),
        Err(CheckFault::Crossing(FaceId(0)))
    );
}

/// The planes' pass charges its faces' planes in parts of a unit,
/// summed and rounded up once: a cube's six planes take
/// `⌈6·rational/64⌉` units, the room for them builds every plane and the
/// first seam bound runs out, one unit less runs out at the last plane.
#[test]
fn planes_are_charged_their_parts_rounded_up_once() {
    let verts: Vec<[f64; 3]> = (0..8)
        .map(|k| [k & 1, (k >> 1) & 1, (k >> 2) & 1].map(f64::from))
        .collect();
    let faces = vec![
        vec![0, 2, 3, 1],
        vec![4, 5, 7, 6],
        vec![0, 1, 5, 4],
        vec![2, 6, 7, 3],
        vec![0, 4, 6, 2],
        vec![1, 3, 7, 5],
    ];
    let planes = (6 * UNITS.record.rational).div_ceil(UNIT_PARTS);
    assert!(planes < 6 * UNITS.record.rational.div_ceil(UNIT_PARTS));
    let build = |units| {
        Brep::from_polygons(
            BodyKind::Solid,
            &verts,
            &faces,
            TARGET,
            &mut Work::new(&Budget::new(units)),
        )
    };
    let built = Brep::from_polygons(
        BodyKind::Solid,
        &verts,
        &faces,
        TARGET,
        &mut Work::new(&Budget::DEFAULT),
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    let checked = built.check_planar().unwrap_or_else(|f| panic!("{f:?}"));
    assert!(checked.unchecked_trims.is_empty(), "{checked:?}");
    match build(planes) {
        Err(PolygonError::Face {
            face: 0,
            why:
                FaceError::Store(StoreError::Coedge {
                    index: 0,
                    why: CoedgeError::OutOfBudget(KernelError::OutOfBudget { profile, .. }),
                }),
        }) => assert_eq!(profile.stages[0], ("planes", planes)),
        other => panic!("{other:?}"),
    }
    match build(planes - 1) {
        Err(PolygonError::OutOfBudget(KernelError::OutOfBudget { profile, .. })) => {
            assert_eq!(profile.stopped.stage, "planes");
            assert_eq!(profile.stopped.at.item, 5);
            assert_eq!(profile.stages, vec![("planes", planes - 1)]);
        }
        other => panic!("{other:?}"),
    }
}

/// The planar check charges its exact predicates by kind, a loop's
/// signed area per term: on a convex `n`-gon sheet, 3 to 9 corners, the
/// area is `n` `Orient2` filter runs, beside one per consecutive pair of
/// sides (the fold test) and four per other pair (the segments' test);
/// nothing reaches the expansions or another kind. The units are those
/// runs' parts rounded up once.
#[test]
fn the_planar_check_charges_a_polygons_area_per_term() {
    for n in 3..=9u32 {
        let verts: Vec<[f64; 3]> = (0..n)
            .map(|k| {
                let a = std::f64::consts::TAU * f64::from(k) / f64::from(n);
                [10.0 * trig::cos(a), 10.0 * trig::sin(a), 0.0]
            })
            .collect();
        let gon = Polygons {
            closed: false,
            verts,
            faces: vec![(0..n).collect()],
        };
        let checked = store(&gon)
            .check_planar()
            .unwrap_or_else(|f| panic!("{n}-gon: {f:?}"));
        assert!(checked.unchecked_trims.is_empty(), "{n}-gon");
        let (area, folds, others) = (n, n, n * (n - 3) / 2);
        let want = Runs {
            filters: area + folds + 4 * others,
            ..Runs::default()
        };
        let orient2 = checked.work.of(PredKind::Orient2);
        assert_eq!(orient2, want, "{n}-gon");
        assert_eq!(checked.work.total(), orient2, "{n}-gon");
        let parts = u64::from(want.filters) * UNITS.record.of(PredKind::Orient2).filter;
        assert_eq!(checked.units(), parts.div_ceil(UNIT_PARTS), "{n}-gon");
    }
}

/// The builder refuses faces it can't take, naming them.
#[test]
fn bad_faces_are_refused() {
    let verts = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let build = |faces: &[Vec<u32>]| {
        Brep::from_polygons(
            BodyKind::Sheet,
            &verts,
            faces,
            TARGET,
            &mut Work::new(&Budget::DEFAULT),
        )
        .map(|_| ())
    };
    let face = |why| Err(PolygonError::Face { face: 1, why });
    let tri = vec![0, 1, 2];
    assert_eq!(build(&[tri.clone(), vec![0, 1]]), face(FaceError::Corners));
    assert_eq!(build(&[tri.clone(), vec![0, 1, 3]]), face(FaceError::Index));
    assert_eq!(
        build(&[tri.clone(), vec![0, 1, 0]]),
        face(FaceError::Repeated)
    );
    // The same triangle twice, the same way: its edges taken twice one
    // way.
    assert!(matches!(
        build(&[tri.clone(), tri.clone()]),
        Err(PolygonError::Face {
            face: 1,
            why: FaceError::Store(StoreError::Coedge {
                index: 0,
                why: CoedgeError::SameWay
            })
        })
    ));
    assert_eq!(
        Brep::from_polygons(
            BodyKind::Sheet,
            &[[f64::NAN; 3]],
            &[],
            TARGET,
            &mut Work::new(&Budget::DEFAULT)
        )
        .map(|_| ()),
        Err(PolygonError::Vert {
            index: 0,
            why: StoreError::NotFinite
        })
    );
}

/// A square sheet `[0, 4]²` facing up with a square hole `corners` (in
/// `z = 0`, in the order given) added as a second loop of its face.
fn holed(corners: [[f64; 2]; 4]) -> Brep {
    let outer = Polygons {
        closed: false,
        verts: vec![
            [0.0, 0.0, 0.0],
            [4.0, 0.0, 0.0],
            [4.0, 4.0, 0.0],
            [0.0, 4.0, 0.0],
        ],
        faces: vec![vec![0, 1, 2, 3]],
    };
    let mut b = store(&outer);
    let lattice = b.surface(FaceId(0)).lattice().clone();
    let pts = corners.map(|c| [c[0], c[1], 0.0]);
    let vs = pts.map(|p| b.add_vert(p).unwrap());
    let mut uses = Vec::new();
    for k in 0..4 {
        let (i, j) = (k, (k + 1) % 4);
        let reversed = vs[i] > vs[j];
        let (lo, hi) = if reversed { (j, i) } else { (i, j) };
        let c = b
            .add_curve(Curve3D::new(vec![line(pts[lo], pts[hi])], vec![0.0, 1.0]).unwrap())
            .unwrap();
        let e = b.add_edge(c, [0.0, 1.0], vs[lo], vs[hi]).unwrap();
        let (a, z) = (plane_uv(&lattice, pts[lo]), plane_uv(&lattice, pts[hi]));
        uses.push((
            e,
            reversed,
            PCurve::new(vec![uv_line(a, z)], vec![0.0, 1.0]).unwrap(),
        ));
    }
    b.add_loop(
        FaceId(0),
        false,
        uses,
        TARGET,
        &mut Work::new(&Budget::DEFAULT),
    )
    .unwrap();
    b
}

/// Holes run clockwise in the face's orientation, inside the outer loop.
#[test]
fn holes_are_checked() {
    let cw = [[1.0, 1.0], [1.0, 3.0], [3.0, 3.0], [3.0, 1.0]];
    let checked = holed(cw).check_planar().unwrap_or_else(|f| panic!("{f:?}"));
    assert!(checked.unchecked_trims.is_empty());
    let ccw = [[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]];
    assert_eq!(
        holed(ccw).check_planar(),
        Err(CheckFault::Orientation(LoopId(1)))
    );
    let outside = cw.map(|p| [p[0] + 5.0, p[1]]);
    assert_eq!(
        holed(outside).check_planar(),
        Err(CheckFault::Nesting(LoopId(1)))
    );
    let across = cw.map(|p| [p[0] + 2.0, p[1]]);
    assert_eq!(
        holed(across).check_planar(),
        Err(CheckFault::Crossing(FaceId(0)))
    );
}

/// The flat fixture bodies built on budgets running out anywhere along
/// their planes' and seams' passes, and the whole: the same store, or
/// the same error, cause and profile, with the items after the deciding
/// one stopped early or run on.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn building_on_any_budget_is_the_same_without_early_stop() {
    use crate::infra::budget::assert_early_stop_changes_nothing;
    let pairs = [sheets::l_corner(OFFSET), sheets::box_edge_fold(OFFSET)];
    for p in pairs.iter().flat_map(|pair| [&pair.a, &pair.b]) {
        let kind = if p.closed {
            BodyKind::Solid
        } else {
            BodyKind::Sheet
        };
        let build = |units| {
            let mut work = Work::new(&Budget::new(units));
            let out = Brep::from_polygons(kind, &p.verts, &p.faces, TARGET, &mut work);
            (out, work.used())
        };
        let (out, used) = build(crate::MAX_WORK);
        assert!(out.is_ok(), "{out:?}");
        for k in 0..=12 {
            let (out, _) = assert_early_stop_changes_nothing(|| build(used * k / 12));
            assert_eq!(out.is_ok(), k == 12, "{k}");
        }
    }
}
