//! The edge certificate on fillets against their supports (tangent,
//! a convex crease, and one curling back across its support), on a
//! cube's and the primitives' edges and on spans of every degree; the
//! vertex certificate at
//! a pyramid's apex and on two germs folded onto one plane.

use std::collections::BTreeMap;

use varde_decide::bezier::Dir;
use varde_decide::num::bounded::Bounded;
use varde_decide::num::storage::OpStart;

use super::*;
use crate::geom::form::{CylinderKey, PlaneKey};
use crate::geom::forms::cyl::cylinder;
use crate::geom::lattice::{CELL, Lattice};
use crate::infra::budget::{Budget, Halt, Pass, Work};
use crate::infra::ops::{FeatureId, InstanceId, Status, Tolerance};
use crate::topo::brep::{BodyKind, ExactRows, FaceKey, FacePart};
use crate::topo::build::{Builder, Use};

/// `f` alone in a pass on a fresh budget of `units`.
fn alone<R: Send>(
    units: u64,
    f: impl Fn(&mut Ledger<'_>) -> Result<R, Stop<Skip>> + Sync,
) -> Result<R, Halt<Skip>> {
    let mut work = Work::new(&Budget::new(units));
    let pass = Pass {
        stage: "adjacent",
        rung: 0,
    };
    work.pass(pass, &[()], |_, ledger| f(ledger))
        .map(|mut r| r.remove(0))
}

fn approx(x: [Bounded<f64>; 3]) -> [f64; 3] {
    x.map(|b| b.v)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(a: [f64; 3]) -> [f64; 3] {
    let l = dot3(a, a).sqrt();
    a.map(|x| x / l)
}

fn comb(a: f64, x: [f64; 3], b: f64, y: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|k| a * x[k] + b * y[k])
}

/// A straight segment as a quadratic span, weights 1.
fn line(a: [f64; 3], b: [f64; 3]) -> Curve3D {
    let m = [0, 1, 2].map(|k| (a[k] + b[k]) * 0.5);
    let pt = |p: [f64; 3]| [p[0], p[1], p[2], 1.0];
    Curve3D::new(vec![[pt(a), pt(m), pt(b)]], vec![0.0, 1.0]).expect("a line")
}

/// A plane through `p` with outward normal `n`, on its one tile, and
/// whether the face is flipped against the key's normal.
fn plane(n: [f64; 3], p: [f64; 3]) -> (Surface, bool) {
    let key = PlaneKey::new(n, p).expect("a plane");
    let flipped = dot3(key.normal(), n) < 0.0;
    let lattice = Lattice::new(key.into()).expect("a lattice");
    let rect = lattice.half(0).expect("its tile");
    (Surface::new(lattice, rect).expect("a surface"), flipped)
}

/// A point's UV on a plane surface.
fn plane_uv(s: &Surface, x: [f64; 3]) -> [f64; 2] {
    let lat = s.lattice();
    let (o, [u, v, _]) = (lat.origin(), lat.axes());
    let d = sub(x, o);
    [u, v].map(|a| dot3(d, a) / CELL + 0.5)
}

/// Whether a face of outward normal `n` reaching from an edge of
/// direction `e` towards `w` lies left of it: its coedge runs the edge's
/// way.
fn runs_forward(n: [f64; 3], e: [f64; 3], w: [f64; 3]) -> bool {
    dot3(cross3(n, e), w) > 0.0
}

/// A fillet of radius 10 against its support plane along a generator
/// of the cylinder (the edge, 4 mm long, at angle parameter `u0`): the
/// fillet face reaching from it to growing `u` (`forward`) or falling,
/// the support at the angle `(cos φ, sin φ)` from the tangent plane
/// (below).
struct Fillet {
    cyl: Surface,
    plane: Surface,
    plane_flipped: bool,
    curve: Curve3D,
    on_cyl: PCurve,
    on_plane: PCurve,
    cyl_forward: bool,
    plane_forward: bool,
}

/// The fillet (module docs of [`Fillet`]). With `n` the cylinder's
/// outward normal at the edge and `d` its direction of growing `u`:
/// forward, the support's normal is `cos φ·n − sin φ·d` and it reaches
/// along `−sin φ·n − cos φ·d` (`φ = 0`: tangent; else a convex crease);
/// backward, its normal is `−cos φ·n + sin φ·d` and it reaches the same
/// way, so the fillet leaves the edge at `φ` beside the support and curls
/// back across it at `2φ` of its arc.
fn fillet(u0: f64, forward: bool, (c, s): (f64, f64)) -> Fillet {
    let key = CylinderKey::new([0.0; 3], [1.0, 0.0, 0.0], 10.0).expect("a cylinder");
    let cyl = cylinder(key, 0).expect("its surface");
    let (va, vb) = (0.5, 0.5 + 1.0 / 1_048_576.0);
    let a = approx(cyl.eval([u0, va]).expect("on it"));
    let b = approx(cyl.eval([u0, vb]).expect("on it"));
    let pa = cyl.eval_partials([u0, va]).expect("on it");
    let d = unit(approx(pa.du));
    let n = unit(cross3(approx(pa.du), approx(pa.dv)));
    let (pn, reach) = if forward {
        (comb(c, n, -s, d), comb(-s, n, -c, d))
    } else {
        (comb(-c, n, s, d), comb(-s, n, -c, d))
    };
    let (plane, plane_flipped) = plane(pn, a);
    let curve = line(a, b);
    let on_cyl = PCurve::from_spans(
        vec![PSpan::Iso {
            fixed: Dir::U,
            at: u0,
            m: [[va, 1.0], [vb, 1.0]],
        }],
        vec![0.0, 1.0],
    )
    .expect("an iso line");
    let (ua, ub) = (plane_uv(&plane, a), plane_uv(&plane, b));
    let on_plane = PCurve::from_spans(
        vec![PSpan::Linear([[ua[0], ua[1], 1.0], [ub[0], ub[1], 1.0]])],
        vec![0.0, 1.0],
    )
    .expect("a UV line");
    let e = sub(b, a);
    let cyl_forward = runs_forward(n, e, if forward { d } else { d.map(|x| -x) });
    let plane_forward = runs_forward(pn, e, reach);
    assert_ne!(cyl_forward, plane_forward, "one coedge each way");
    Fillet {
        cyl,
        plane,
        plane_flipped,
        curve,
        on_cyl,
        on_plane,
        cyl_forward,
        plane_forward,
    }
}

impl Fillet {
    /// The edge, the face running its way first; and which side is the
    /// fillet's.
    fn edge(&self) -> (Edge<'_>, usize) {
        let cyl = Side {
            surface: &self.cyl,
            pcurve: &self.on_cyl,
            flipped: false,
            reversed: !self.cyl_forward,
        };
        let pl = Side {
            surface: &self.plane,
            pcurve: &self.on_plane,
            flipped: self.plane_flipped,
            reversed: !self.plane_forward,
        };
        let (sides, fillet) = if self.cyl_forward {
            ([cyl, pl], 0)
        } else {
            ([pl, cyl], 1)
        };
        let edge = Edge {
            curve: &self.curve,
            t_range: [0.0, 1.0],
            sides,
        };
        (edge, fillet)
    }

    /// The strips: half a cell (45° of arc) on the cylinder, 2⁻¹⁸ of
    /// the plane's cell (16 mm) on the plane.
    fn strip(&self) -> Strip {
        let w = |cyl: bool| if cyl { 0.5 } else { 1.0 / 262_144.0 };
        Strip {
            width: if self.cyl_forward {
                [w(true), w(false)]
            } else {
                [w(false), w(true)]
            },
        }
    }

    fn certify(&self) -> EdgeCert {
        let (edge, _) = self.edge();
        let strip = self.strip();
        alone(Budget::DEFAULT.work(), |l| certify_edge(&edge, &strip, l)).expect("settled")
    }
}

/// A fillet tangent to its support holds: each face on its side of the
/// ruled surface beyond a join zone of rounding's size, over the whole
/// strips.
#[test]
fn a_tangent_fillet_holds() {
    let f = fillet(0.25, true, (1.0, 0.0));
    let (_, side) = f.edge();
    match f.certify() {
        EdgeCert::Held(h) => {
            assert_eq!(h.reach, f.strip().width);
            // The join zones: far below a micrometre on either face.
            assert!(h.s0[side] < 1e-6, "{h:?}");
            assert!(h.s0[1 - side] * CELL < 1e-3, "{h:?}");
        }
        other => panic!("not held: {other:?}"),
    }
}

/// A fillet meeting its support at a convex crease of 30° holds too.
#[test]
fn a_crease_holds() {
    let half = 0.5f64;
    let f = fillet(0.25, true, ((1.0 - half * half).sqrt(), half));
    assert!(matches!(f.certify(), EdgeCert::Held(_)));
}

/// A fillet leaving its support at 30° beside it and curling back across
/// it within its strip is refused: certified on the support's side of the
/// ruled surface (the bisector of the two faces), on the fillet.
#[test]
fn a_fillet_crossing_its_support_fails() {
    let half = 0.5f64;
    let f = fillet(0.75, false, ((1.0 - half * half).sqrt(), half));
    let (_, side) = f.edge();
    match f.certify() {
        EdgeCert::Fails { side: s, t, s: at } => {
            assert_eq!(s, side, "the fillet's side");
            assert!((0.0..=1.0).contains(&t));
            assert!(at > 0.0);
        }
        other => panic!("not refused: {other:?}"),
    }
}

/// Every cell and box is charged before it runs: on a budget too small
/// for one, the certificate runs out, whatever its outcome would be.
#[test]
fn the_certificate_is_charged() {
    let f = fillet(0.25, true, (1.0, 0.0));
    let (edge, _) = f.edge();
    let strip = f.strip();
    let r = alone(UNITS.adjacent.field_low[0] - 1, |l| {
        certify_edge(&edge, &strip, l)
    });
    assert!(matches!(r, Err(Halt::OutOfBudget(_))), "{r:?}");
}

/// The sign functions' degrees on the whole piece at the first level:
/// the field's, the strip's and the curve's added (module docs).
fn sign_degrees(edge: &Edge<'_>, strip: &Strip) -> [(usize, usize); 2] {
    let pieces = edge.pieces().expect("pieces");
    let piece = &pieces[0];
    let cell = TCell {
        k: 0,
        depth: 0,
        level: 0,
    };
    let like = Bounded::exact(0.0);
    let FieldOut::Ready(frame) = field_at(edge, piece, cell, strip.width, &like) else {
        panic!("no field at the first level");
    };
    [0, 1].map(|side| {
        let f = EdgeStrip {
            edge,
            piece,
            cell,
            frame: &frame,
            side,
            plane: false,
        };
        let (g, _) = f.forms(&SBox::WHOLE, &like).expect("formed");
        g.degrees()
    })
}

/// Quadratic p-curve spans on both faces and a conic curve give the
/// sign function of degrees (28, 4): the field 16, the strip 8 (and 4 in
/// `s`, the strip moving both coordinates), the curve 2 and its tangent
/// 2. A straight UV line on the support and an iso line on the fillet,
/// along a straight curve, give lower degrees, each from its operands:
/// the field 8 (the line's), the curve 2 and its constant tangent 0, the
/// strip 4 on the plane and 2 on the cylinder (whose strip moves `u`
/// alone, so 2 in `s`).
#[test]
fn degrees_are_computed_from_the_spans() {
    // A quarter circle of radius 50 on the plane `z = 0`, both faces on
    // the plane, one on each side.
    let (s, flipped) = plane([0.0, 0.0, 1.0], [0.0; 3]);
    let (r, mid) = (50.0, std::f64::consts::FRAC_1_SQRT_2);
    let curve = Curve3D::new(
        vec![[[r, 0.0, 0.0, 1.0], [r, r, 0.0, mid], [0.0, r, 0.0, 1.0]]],
        vec![0.0, 1.0],
    )
    .expect("an arc");
    let pc = {
        let span = curve.spans()[0].map(|p| {
            let uv = plane_uv(&s, [p[0], p[1], p[2]]);
            [uv[0], uv[1], p[3]]
        });
        PCurve::new(vec![span], vec![0.0, 1.0]).expect("a Q span")
    };
    let side = |reversed| Side {
        surface: &s,
        pcurve: &pc,
        flipped,
        reversed,
    };
    let edge = Edge {
        curve: &curve,
        t_range: [0.0, 1.0],
        sides: [side(false), side(true)],
    };
    let strip = Strip {
        width: [1.0 / 1_048_576.0; 2],
    };
    assert_eq!(sign_degrees(&edge, &strip), [(28, 4); 2]);

    let f = fillet(0.25, true, (1.0, 0.0));
    let (edge, fillet_side) = f.edge();
    let d = sign_degrees(&edge, &f.strip());
    assert_eq!(d[fillet_side], (2 + 2 + 8, 2));
    assert_eq!(d[1 - fillet_side].0, 4 + 2 + 8);
}

// ---------------------------------------------------------------------
// Bodies.

fn status() -> Status {
    let start = OpStart {
        max_coord: 4.0,
        rho_op: 0.0,
    };
    Status::new(Tolerance::new(1e-3).expect("in range"), &start)
}

fn key(j: usize) -> FaceKey {
    FaceKey {
        feature: FeatureId(1),
        part: FacePart::Primitive(j as u32),
        instance: InstanceId::ORIGINAL,
    }
}

/// The planar polyhedron on `pos` with the faces `faces` (vertex indices
/// counter-clockwise seen from outside) as a sheet: a plane per face, an
/// edge per vertex pair, lines for curves and p-curves.
fn polyhedron(pos: &[[f64; 3]], faces: &[&[usize]]) -> Brep {
    let mut b = Builder::new(BodyKind::Sheet, status());
    let verts: Vec<VertId> = pos
        .iter()
        .map(|&x| b.add_vert(x).expect("a vertex"))
        .collect();
    let mut edges: BTreeMap<(usize, usize), EdgeId> = BTreeMap::new();
    for (j, f) in faces.iter().enumerate() {
        let p: Vec<[f64; 3]> = f.iter().map(|&i| pos[i]).collect();
        let n = cross3(sub(p[1], p[0]), sub(p[2], p[0]));
        let (surface, flipped) = plane(n, p[0]);
        let uv: Vec<[f64; 2]> = p.iter().map(|&x| plane_uv(&surface, x)).collect();
        let s = b
            .add_surface(surface, None, ExactRows::default())
            .expect("a surface");
        let face = b.add_face(s, flipped, key(j)).expect("a face");
        let uses: Vec<Use> = (0..f.len())
            .map(|i| {
                let (from, to) = (f[i], f[(i + 1) % f.len()]);
                let (lo, hi) = (from.min(to), from.max(to));
                let e = *edges.entry((lo, hi)).or_insert_with(|| {
                    let c = b.add_curve(line(pos[lo], pos[hi])).expect("a curve");
                    b.add_edge(c, [0.0, 1.0], [verts[lo], verts[hi]])
                        .expect("an edge")
                });
                let (ul, uh) = if from < to {
                    (uv[i], uv[(i + 1) % f.len()])
                } else {
                    (uv[(i + 1) % f.len()], uv[i])
                };
                let pc = PCurve::from_spans(
                    vec![PSpan::Linear([[ul[0], ul[1], 1.0], [uh[0], uh[1], 1.0]])],
                    vec![0.0, 1.0],
                )
                .expect("a UV line");
                Use {
                    edge: e,
                    reversed: from > to,
                    pcurve: b.add_pcurve(pc).expect("a p-curve"),
                    seam_bound: 0.0,
                }
            })
            .collect();
        b.add_loop(face, true, &uses).expect("a loop");
    }
    b.finish().expect("finished").0
}

/// A cube's twelve edges hold, each on strips of 2⁻²⁰ of the plane's cell
/// (4 mm) on both faces.
#[test]
fn a_cubes_edges_hold() {
    let pos: Vec<[f64; 3]> = (0..8)
        .map(|i| [(i & 1) as f64, ((i >> 1) & 1) as f64, ((i >> 2) & 1) as f64].map(|x| x * 10.0))
        .collect();
    let body = polyhedron(
        &pos,
        &[
            &[0, 2, 3, 1],
            &[4, 5, 7, 6],
            &[0, 1, 5, 4],
            &[2, 6, 7, 3],
            &[0, 4, 6, 2],
            &[1, 3, 7, 5],
        ],
    );
    let strip = Strip {
        width: [1.0 / 1_048_576.0; 2],
    };
    for e in 0..body.edge_count() {
        let edge = edge_of(&body, EdgeId(e as u32)).expect("two faces");
        let out = alone(Budget::DEFAULT.work(), |l| certify_edge(&edge, &strip, l));
        assert!(matches!(out, Ok(EdgeCert::Held(_))), "edge {e}: {out:?}");
    }
}

/// The square pyramid of base 20 and height 10: its apex (vertex 4)
/// joins four faces, the opposite two sharing only the apex.
fn pyramid() -> Brep {
    let pos = [
        [-10.0, -10.0, 0.0],
        [10.0, -10.0, 0.0],
        [10.0, 10.0, 0.0],
        [-10.0, 10.0, 0.0],
        [0.0, 0.0, 10.0],
    ];
    polyhedron(
        &pos,
        &[
            &[0, 3, 2, 1],
            &[0, 1, 4],
            &[1, 2, 4],
            &[2, 3, 4],
            &[3, 0, 4],
        ],
    )
}

/// At a pyramid's apex the opposite faces are separated by a plane
/// through it.
#[test]
fn a_pyramids_opposite_faces_are_separated_at_its_apex() {
    let body = pyramid();
    let apex = VertId(4);
    let v = body.pos(apex);
    for pair in [[FaceId(1), FaceId(3)], [FaceId(2), FaceId(4)]] {
        let g = pair.map(|f| germ_of(&body, f, apex).expect("a germ"));
        assert_eq!(g[0].corners.len(), 1);
        let out = alone(Budget::DEFAULT.work(), |l| {
            certify_vertex(v, [&g[0], &g[1]], [1.0 / 1_048_576.0; 2], l)
        });
        match out {
            Ok(VertCert::Held { normal, rho0 }) => {
                // The germs on either side, the join zones of rounding's
                // size.
                assert!(normal.iter().any(|&x| x != 0.0));
                assert!(rho0.iter().all(|&r| r * CELL < 1e-2), "{rho0:?}");
            }
            other => panic!("{pair:?}: {other:?}"),
        }
    }
}

/// Two germs folded onto one plane, overlapping in their sectors, have
/// no separating plane.
#[test]
fn germs_folded_onto_one_plane_are_not_separated() {
    let (s, _) = plane([0.0, 0.0, 1.0], [0.0; 3]);
    let o = plane_uv(&s, [0.0; 3]);
    let ray = |d: [f64; 2]| {
        PSpan::Linear([
            [o[0], o[1], 1.0],
            [o[0] + d[0] * 1e-6, o[1] + d[1] * 1e-6, 1.0],
        ])
    };
    let germ = |flipped: bool, a: [f64; 2], b: [f64; 2]| Germ {
        surface: &s,
        flipped,
        corners: vec![[ray(a), ray(b)]],
    };
    // A quarter on top, and an overlapping quarter turned over.
    let g0 = germ(false, [1.0, 0.0], [0.0, 1.0]);
    let g1 = germ(true, [1.0, 1.0], [1.0, -1.0]);
    let out = alone(Budget::DEFAULT.work(), |l| {
        certify_vertex([0.0; 3], [&g0, &g1], [1.0 / 1_048_576.0; 2], l)
    });
    assert_eq!(out, Ok(VertCert::Open));
}

/// The primitives' edges hold on strips of 2⁻²⁰ of a cell on both faces:
/// a box's creases, a cylinder's and a frustum's rims and seams, a
/// torus's seams. A sphere's meridians end at its poles, whose balls
/// this build doesn't certify.
#[test]
fn the_primitives_edges_hold() {
    use crate::build::primitives::{Primitive, build};
    use crate::infra::ops::OpCtx;
    let ctx = OpCtx {
        feature: FeatureId(1),
        instance: InstanceId::ORIGINAL,
    };
    let strip = Strip {
        width: [1.0 / 1_048_576.0; 2],
    };
    for p in [
        Primitive::Cuboid {
            min: [-1.0, 2.0, 3.0],
            size: [10.0, 20.0, 5.0],
        },
        Primitive::Cylinder {
            base: [1.0, 2.0, 3.0],
            radius: 5.0,
            height: 7.0,
        },
        Primitive::Cone {
            base: [0.0; 3],
            radii: [5.0, 2.0],
            height: 4.0,
        },
        Primitive::Torus {
            centre: [0.0; 3],
            major: 10.0,
            minor: 3.0,
        },
    ] {
        let body = build(
            &p,
            ctx,
            Tolerance::DEFAULT,
            &mut Work::new(&Budget::DEFAULT),
        )
        .expect("built");
        let mut certified = 0;
        for e in 0..body.edge_count() {
            let Ok(edge) = edge_of(&body, EdgeId(e as u32)) else {
                continue;
            };
            let out = alone(Budget::DEFAULT.work(), |l| certify_edge(&edge, &strip, l));
            assert!(
                matches!(out, Ok(EdgeCert::Held(_))),
                "{p:?} edge {e}: {out:?}"
            );
            certified += 1;
        }
        assert!(certified > 0, "{p:?}");
    }
    let sphere = Primitive::Sphere {
        centre: [1.0, 1.0, 1.0],
        radius: 3.0,
    };
    let body = build(
        &sphere,
        ctx,
        Tolerance::DEFAULT,
        &mut Work::new(&Budget::DEFAULT),
    )
    .expect("built");
    for e in 0..body.edge_count() {
        let r = edge_of(&body, EdgeId(e as u32)).map(|_| ());
        assert!(matches!(r, Err(Skip::Staged | Skip::NoPair)), "edge {e}");
    }
}
