//! The edge certificate on fillets against their supports (tangent,
//! a convex crease, and one curling back across its support), on a
//! cube's and the primitives' edges and on spans of every degree; the
//! vertex certificate at
//! a pyramid's apex and on two germs folded onto one plane.

use std::collections::BTreeMap;

use varde_decide::bezier::Dir;
use varde_decide::num::bounded::Bounded;
use varde_decide::num::storage::OpStart;
use varde_decide::num::trig::{cos, sin};

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

/// The window's threshold at the default tolerance: `ε/8` with its
/// margin.
const DELTA: f64 = 1e-3 * 0.125 * window::MARGIN;

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
            seam: 0.0,
        };
        let pl = Side {
            surface: &self.plane,
            pcurve: &self.on_plane,
            flipped: self.plane_flipped,
            reversed: !self.plane_forward,
            seam: 0.0,
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
            one_surface: false,
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
        alone(Budget::DEFAULT.work(), |l| {
            certify_edge(&edge, &strip, DELTA, l)
        })
        .expect("settled")
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
        EdgeCert::Fails {
            side: s, t, s: at, ..
        } => {
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
        certify_edge(&edge, &strip, DELTA, l)
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
/// sign function of degrees (28, 4) on curved faces: the field 16, the
/// strip 8 (and 4 in `s`, the strip moving both coordinates), the curve 2
/// and its tangent 2; on two planes the field is a constant (each
/// normal its tile's one value), so (12, 4). A straight UV line on the
/// support and an iso line on the fillet, along a straight curve, give
/// lower degrees, each from its operands:
/// the field 4 (the cylinder's normal along its line; the plane's is a
/// constant), the curve 2 and its constant tangent 0, the
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
        seam: 0.0,
    };
    let edge = Edge {
        curve: &curve,
        t_range: [0.0, 1.0],
        sides: [side(false), side(true)],
        one_surface: false,
    };
    let strip = Strip {
        width: [1.0 / 1_048_576.0; 2],
    };
    assert_eq!(sign_degrees(&edge, &strip), [(12, 4); 2]);

    let f = fillet(0.25, true, (1.0, 0.0));
    let (edge, fillet_side) = f.edge();
    let d = sign_degrees(&edge, &f.strip());
    assert_eq!(d[fillet_side], (2 + 2 + 4, 2));
    assert_eq!(d[1 - fillet_side].0, 4 + 2 + 4);
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
    polyhedron_keyed(pos, faces, key)
}

/// [`polyhedron`] with face `j` keyed `key(j)`.
fn polyhedron_keyed(pos: &[[f64; 3]], faces: &[&[usize]], key: impl Fn(usize) -> FaceKey) -> Brep {
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
        let out = alone(Budget::DEFAULT.work(), |l| {
            certify_edge(&edge, &strip, DELTA, l)
        });
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
            let out = alone(Budget::DEFAULT.work(), |l| {
                certify_edge(&edge, &strip, DELTA, l)
            });
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

// ---------------------------------------------------------------------
// The window.

/// The window of `edge`, the faces' other edges `near`, alone on a
/// fresh budget.
fn window_alone(edge: &Edge<'_>, near: &[window::Arc3]) -> Result<window::Window, Halt<Skip>> {
    alone(Budget::DEFAULT.work(), |l| {
        window::certify_window(edge, near, DELTA, l)
    })
}

/// The window of edge `e` of `body`, alone on a fresh budget.
fn body_window(body: &Brep, e: EdgeId) -> Result<window::Window, Halt<Skip>> {
    alone(Budget::DEFAULT.work(), |l| window_of(body, e, l))
}

#[track_caller]
fn tube(w: Result<window::Window, Halt<Skip>>) -> window::Tube {
    match w {
        Ok(window::Window::Tube(t)) => {
            // A power of two at least `ρ_min` and above `r₀`.
            assert_eq!(dyadic_floor(t.rho_adj), t.rho_adj, "{t:?}");
            assert!(t.rho_adj >= t.rho_min && t.rho_adj > t.r0, "{t:?}");
            t
        }
        other => panic!("no tube: {other:?}"),
    }
}

/// An invariant that checks nothing.
struct Nothing;

impl Invariant for Nothing {
    fn kinds(&self) -> &[ItemKind] {
        &[]
    }

    fn check(&self, _: &Cx<'_>, _: Item, _: &mut Ledger<'_>) -> Result<(), Stop<Fault>> {
        Ok(())
    }
}

/// The check of `body` with invariant 9 alone.
fn embedding(body: &Brep) -> Result<(), crate::topo::check::CheckError> {
    use crate::topo::check::{Invariants, Scope, run};
    let mut table: Invariants<'_> = [&Nothing; 10];
    table[8] = &Embedding;
    run(
        body,
        Scope::Full,
        None,
        &table,
        &mut Work::new(&Budget::DEFAULT),
        0,
    )
}

/// Invariant 9 alone fails `body` first past its edges, at a part not
/// built (a vertex's ball, or the leaves of faces apart): every edge's
/// tube and its inside certificate hold.
#[track_caller]
fn edges_pass(body: &Brep) {
    use crate::topo::check::{CheckError, Finding};
    let past = [EmbedCheck::Vertex as u32, EmbedCheck::Apart as u32];
    match embedding(body) {
        Err(CheckError::Failed(f)) if f.finding == Finding::Unbuilt && past.contains(&f.check) => {}
        other => panic!("{other:?}"),
    }
}

/// A fillet tangent to its support has the window of its separation: the
/// cylinder of radius 10 leaves the plane by `r²/20` at `r` from the
/// edge, past `δ` from `√(20δ) ≈ 0.05` mm on; a 30° crease's `ρ_min` is
/// `δ/sin 30°`. The tubes reach as far as the strips (the cylinder's
/// quarter is 7.65 mm across its chord).
#[test]
fn fillets_have_their_windows() {
    let f = fillet(0.25, true, (1.0, 0.0));
    let t = tube(window_alone(&f.edge().0, &[]));
    assert!((0.05..0.2).contains(&t.rho_min), "{t:?}");
    assert_eq!(t.rho_adj, 4.0);
    let half = 0.5f64;
    let f = fillet(0.25, true, ((1.0 - half * half).sqrt(), half));
    let t = tube(window_alone(&f.edge().0, &[]));
    let crease = DELTA / half;
    assert!(
        t.rho_min >= crease && t.rho_min < crease * (1.0 + 1e-9),
        "{t:?}"
    );
    assert_eq!(t.rho_adj, 4.0);
}

/// A cube's edges have their tubes up to their faces' far edges (side 10:
/// `ρ_adj` 8), and invariant 9 passes them all.
#[test]
fn a_cubes_edges_have_their_tubes() {
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
    for e in 0..body.edge_count() {
        let t = tube(body_window(&body, EdgeId(e as u32)));
        assert_eq!(t.rho_adj, 8.0, "edge {e}: {t:?}");
        assert!(t.rho_min < DELTA * (1.0 + 1e-9), "edge {e}: {t:?}");
    }
    edges_pass(&body);
}

/// The prism over a profile with two 0.5 mm segments at 179.99°, ending
/// at 90° corners (r9-cons C1), keyed as an extrusion's walls and caps:
/// face `j` of the profile's segment `j`, then the caps.
fn shallow_prism() -> Brep {
    let a = 0.01f64.to_radians();
    let profile = [
        [0.0, 0.0],
        [0.5, 0.0],
        [0.5 + 0.5 * cos(a), 0.5 * sin(a)],
        [0.5 + 0.5 * cos(a), 1.0],
        [0.0, 1.0],
    ];
    let n = profile.len();
    let mut pos: Vec<[f64; 3]> = profile.iter().map(|p| [p[0], p[1], 0.0]).collect();
    pos.extend(profile.iter().map(|p| [p[0], p[1], 1.0]));
    let walls: Vec<Vec<usize>> = (0..n)
        .map(|i| {
            let j = (i + 1) % n;
            vec![i, j, j + n, i + n]
        })
        .collect();
    let bottom: Vec<usize> = (0..n).rev().collect();
    let top: Vec<usize> = (n..2 * n).collect();
    let mut faces: Vec<&[usize]> = walls.iter().map(Vec::as_slice).collect();
    faces.push(&bottom);
    faces.push(&top);
    polyhedron_keyed(&pos, &faces, |j| FaceKey {
        feature: FeatureId(1),
        part: match j {
            0..5 => FacePart::Side {
                curve: 7,
                segment: j as u32,
            },
            5 => FacePart::StartCap { region: 0 },
            _ => FacePart::EndCap { region: 0 },
        },
        instance: InstanceId::ORIGINAL,
    })
}

/// The edge between faces `f` and `g` of `body`.
fn edge_between(body: &Brep, f: FaceId, g: FaceId) -> EdgeId {
    (0..body.edge_count() as u32)
        .map(EdgeId)
        .find(|&e| {
            let mut fs = edge_faces(body, e);
            fs.sort_unstable();
            fs == [f.min(g), f.max(g)]
        })
        .expect("an edge between them")
}

/// Two walls 0.5 mm wide at 179.99° (r9-cons C1): `ρ_min = δ/sin 0.01° ≈
/// 0.72` mm, but the walls' far edges are 0.5 mm off: the window is
/// certified empty. A constructed body so is a self-intersection of its
/// inputs, naming the two profile segments' walls, never out of budget; a
/// result face so is a rule gap at invariant 9's validity.
#[test]
fn walls_at_a_nearly_straight_joint_have_an_empty_window() {
    use crate::evidence::{Failure, ItemRole, Leaf, Site};
    use crate::infra::error::{Inconsistency, Internal, Invalid, KernelError};
    use crate::names::Name;
    use crate::topo::check::{CheckError, Finding, Inv, Item as CheckItem, Role};
    let body = shallow_prism();
    let e = edge_between(&body, FaceId(0), FaceId(1));
    match body_window(&body, e) {
        Ok(window::Window::Empty { rho_min, rho_graph }) => {
            let crease = DELTA / sin(0.01f64.to_radians());
            assert!((crease * 0.999..=crease).contains(&rho_min), "{rho_min}");
            assert!(
                (0.5..0.5 * (1.0 + 1e-9)).contains(&rho_graph),
                "{rho_graph}"
            );
        }
        other => panic!("{other:?}"),
    }
    // Its other edges have their tubes.
    for g in (0..body.edge_count() as u32)
        .map(EdgeId)
        .filter(|&g| g != e)
    {
        tube(body_window(&body, g));
    }
    let err = embedding(&body).unwrap_err();
    let CheckError::Failed(f) = err.clone() else {
        panic!("{err:?}");
    };
    assert_eq!(
        (f.inv, f.item, f.check, f.finding),
        (
            Inv::Embedding,
            CheckItem::Edge(e),
            EmbedCheck::Edge as u32,
            Finding::EmptyWindow
        )
    );
    let leaf = Leaf::new(0, &body);
    let op = crate::infra::ops::OpCtx::new(FeatureId(1));
    let made = Failure::of_check(&err, Role::Constructed, &leaf, op, 0).expect("a failure");
    assert_eq!(made.error, KernelError::Invalid(Invalid::SelfIntersection));
    let walls: Vec<FacePart> = made
        .evidence
        .items
        .iter()
        .filter(|i| i.role == ItemRole::Partner)
        .filter_map(|i| match i.subject.as_ref()?.name.as_ref()? {
            Name::Face(k) => Some(k.part),
            _ => None,
        })
        .collect();
    assert_eq!(
        walls,
        [0, 1].map(|segment| FacePart::Side { curve: 7, segment })
    );
    let values = made
        .evidence
        .items
        .iter()
        .find(|i| i.role == ItemRole::Value)
        .expect("ρ_min and ρ_graph");
    assert_eq!(values.values.len(), 2);
    assert!(values.values[0].v > values.values[1].v);
    let result = Failure::of_check(&err, Role::Result, &leaf, op, 0).expect("a failure");
    assert_eq!(
        result.error,
        KernelError::Internal(Internal::Inconsistent(Inconsistency::ClusterRuleGap))
    );
    assert_eq!(
        result.evidence.site,
        Site::Validity {
            inv: Inv::Embedding
        }
    );
}

/// Faces on one surface need no window: a cylinder's and a torus's
/// seams, each between two faces of one form (`SameForm`), `ρ_min` 0 and
/// the least power of two above their ribbons, with no subdivision.
#[test]
fn faces_on_one_surface_need_no_window() {
    use crate::build::primitives::{Primitive, build};
    use crate::infra::ops::OpCtx;
    let ctx = OpCtx {
        feature: FeatureId(1),
        instance: InstanceId::ORIGINAL,
    };
    for p in [
        Primitive::Cylinder {
            base: [1.0, 2.0, 3.0],
            radius: 5.0,
            height: 7.0,
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
        let mut seams = 0;
        for e in (0..body.edge_count() as u32).map(EdgeId) {
            if !matches!(body.edge_kind(e), EdgeKind::SameForm { .. }) {
                continue;
            }
            let edge = edge_of(&body, e).expect("two faces");
            assert!(edge.one_surface);
            // No certificate runs: a budget of nothing settles it.
            let t = tube(alone(0, |l| window::certify_window(&edge, &[], DELTA, l)));
            assert_eq!(tube(body_window(&body, e)), t);
            assert_eq!(t.rho_min, 0.0);
            assert_eq!(t.rho_adj, window::dyadic_above(t.r0));
            seams += 1;
        }
        assert!(seams > 0, "{p:?}");
    }
}

/// Invariant 9 passes the edges of every primitive but the sphere's,
/// whose edges end at its poles (the poles' balls aren't built).
#[test]
fn the_primitives_edges_pass_invariant_9() {
    use crate::build::primitives::{Primitive, build};
    use crate::infra::ops::OpCtx;
    use crate::topo::check::{CheckError, Finding};
    let ctx = OpCtx {
        feature: FeatureId(1),
        instance: InstanceId::ORIGINAL,
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
        Primitive::Sphere {
            centre: [1.0, 1.0, 1.0],
            radius: 3.0,
        },
    ] {
        let body = build(
            &p,
            ctx,
            Tolerance::DEFAULT,
            &mut Work::new(&Budget::DEFAULT),
        )
        .expect("built");
        if matches!(p, Primitive::Sphere { .. }) {
            match embedding(&body) {
                Err(CheckError::Failed(f)) => {
                    assert_eq!(
                        (f.check, f.finding),
                        (EmbedCheck::Edge as u32, Finding::Unbuilt)
                    );
                }
                other => panic!("{other:?}"),
            }
        } else {
            edges_pass(&body);
        }
    }
}

/// A span through `a`, `m` and `b` at `0`, `½` and `1`, weights 1.
fn through<const N: usize>(a: [f64; N], m: [f64; N], b: [f64; N]) -> [[f64; N]; 3] {
    let mid = std::array::from_fn(|k| 2.0 * m[k] - 0.5 * (a[k] + b[k]));
    [a, mid, b]
}

/// Whether a face of outward normal `n` at `x` on `s` is flipped against
/// the surface's own normal there.
fn flipped_at(s: &Surface, uv: [f64; 2], n: [f64; 3]) -> bool {
    let p = s.eval_partials(uv).expect("on it");
    dot3(cross3(approx(p.du), approx(p.dv)), n) < 0.0
}

/// The union of a cylinder of radius 10 along `z` and one of radius 6
/// along `x`: their crossing curve where `y ∈ [1, 5]`, `x, z > 0`, fitted
/// by 16 quadratic spans through the exact points (in space and in each
/// cylinder's UV), each ribbon bounded by twice its largest sampled
/// width. The faces outside each other meet at a concave crease of about
/// 80°.
struct Crossing {
    big: Surface,
    small: Surface,
    curve: Curve3D,
    on: [PCurve; 2],
    flipped: [bool; 2],
    forward: [bool; 2],
    seam: [f64; 2],
}

fn crossing() -> Crossing {
    let big_key = CylinderKey::new([0.0; 3], [0.0, 0.0, 1.0], 10.0).expect("a cylinder");
    let small_key = CylinderKey::new([0.0; 3], [1.0, 0.0, 0.0], 6.0).expect("a cylinder");
    let point = |y: f64| [(100.0 - y * y).sqrt(), y, (36.0 - y * y).sqrt()];
    // The 180° surface of each cylinder holding the curve.
    let holding = |key: CylinderKey| {
        [0, 1, 2, 3]
            .into_iter()
            .map(|first| cylinder(key, first).expect("its surface"))
            .find(|s| {
                [1.0, 3.0, 5.0]
                    .iter()
                    .all(|&y| s.inverse(point(y)).expect("finite").dev < 1e-8)
            })
            .expect("a surface holds the curve")
    };
    let (big, small) = (holding(big_key), holding(small_key));
    let n = 16;
    let ys: Vec<f64> = (0..=2 * n)
        .map(|i| 1.0 + 4.0 * i as f64 / (2 * n) as f64)
        .collect();
    let pts: Vec<[f64; 3]> = ys.iter().map(|&y| point(y)).collect();
    let knots: Vec<f64> = (0..=n).map(|i| i as f64 / n as f64).collect();
    let spans: Vec<[[f64; 4]; 3]> = (0..n)
        .map(|i| {
            through(pts[2 * i], pts[2 * i + 1], pts[2 * i + 2]).map(|p| [p[0], p[1], p[2], 1.0])
        })
        .collect();
    let curve = Curve3D::new(spans, knots.clone()).expect("a curve");
    let on = [&big, &small].map(|s| {
        let uv: Vec<[f64; 2]> = pts
            .iter()
            .map(|&p| s.inverse(p).expect("finite").uv)
            .collect();
        let spans = (0..n)
            .map(|i| through(uv[2 * i], uv[2 * i + 1], uv[2 * i + 2]).map(|q| [q[0], q[1], 1.0]))
            .collect();
        PCurve::new(spans, knots.clone()).expect("a p-curve")
    });
    let seam = [0, 1].map(|i| {
        let s = [&big, &small][i];
        let mut w: f64 = 0.0;
        for k in 0..=64 * n {
            let t = k as f64 / (64 * n) as f64;
            let x = approx(s.eval(on[i].point_at(t).expect("on it")).expect("on it"));
            let c = curve.point_at(t).expect("on it");
            let d = dot3(sub(x, c), sub(x, c)).sqrt();
            if d > w {
                w = d;
            }
        }
        2.0 * w + 1e-12
    });
    // The outward normals, the directions into each face (away from the
    // other cylinder) and the curve's direction, at its middle.
    let m = point(3.0);
    let normals = [unit([m[0], m[1], 0.0]), unit([0.0, m[1], m[2]])];
    let away = [
        // Along the big cylinder, away from the small one's axis.
        {
            let g = [0.0, m[1], m[2]];
            let n0 = normals[0];
            sub(g, n0.map(|x| x * dot3(g, n0)))
        },
        {
            let g = [m[0], m[1], 0.0];
            let n1 = normals[1];
            sub(g, n1.map(|x| x * dot3(g, n1)))
        },
    ];
    let along = sub(point(3.1), point(2.9));
    let forward = [0, 1].map(|i| runs_forward(normals[i], along, away[i]));
    assert_ne!(forward[0], forward[1], "one coedge each way");
    let uvm = [&big, &small].map(|s| s.inverse(m).expect("finite").uv);
    let flipped = [
        flipped_at(&big, uvm[0], normals[0]),
        flipped_at(&small, uvm[1], normals[1]),
    ];
    Crossing {
        big,
        small,
        curve,
        on,
        flipped,
        forward,
        seam,
    }
}

impl Crossing {
    fn edge(&self) -> Edge<'_> {
        let side = |i: usize| Side {
            surface: [&self.big, &self.small][i],
            pcurve: &self.on[i],
            flipped: self.flipped[i],
            reversed: !self.forward[i],
            seam: self.seam[i],
        };
        let sides = if self.forward[0] {
            [side(0), side(1)]
        } else {
            [side(1), side(0)]
        };
        Edge {
            curve: &self.curve,
            t_range: [0.0, 1.0],
            sides,
            one_surface: false,
        }
    }
}

/// A fitted crossing of two cylinders (r8-impl I4): its ribbons off the
/// curve, the faces at a crease of about 80°, has its tube.
#[test]
fn a_fitted_crossing_has_its_tube() {
    let c = crossing();
    assert!(
        c.seam.iter().all(|&s| s > 1e-12 && s < 5e-4),
        "{:?}",
        c.seam
    );
    let t = tube(window_alone(&c.edge(), &[]));
    assert!(t.rho_min < 2.0 * DELTA, "{t:?}");
    assert!(t.rho_adj >= 1.0, "{t:?}");
    // The join zone holds the ribbons, as wide as half their bounds.
    let widest = if c.seam[0] > c.seam[1] {
        c.seam[0]
    } else {
        c.seam[1]
    };
    assert!(t.r0 >= 0.5 * widest, "{t:?}");
}

/// Two walls along `z` over profile arcs meeting at `(0, 10)` with
/// curvature `1/10` both (a G² joint of different conics): a quarter of
/// the circle of radius 10 about the origin, and a quarter of the ellipse
/// of semi-axes 20 (along `x`) and 40 (along `y`) about `(0, −30)`, at
/// its vertex. Each wall its own conic-chain cylinder; the edge the line
/// over the joint, 10 mm long; the walls' far edges the lines over the
/// arcs' other ends.
struct Osculating {
    walls: [Surface; 2],
    curve: Curve3D,
    on: [PCurve; 2],
    flipped: [bool; 2],
    far: [Curve3D; 2],
}

fn osculating() -> Osculating {
    use crate::geom::form::ConicCylinderKey;
    use crate::geom::forms::cyl::conic_chain;
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let arcs: [[[f64; 4]; 3]; 2] = [
        [
            [10.0, 0.0, 0.0, 1.0],
            [10.0, 10.0, 0.0, h],
            [0.0, 10.0, 0.0, 1.0],
        ],
        [
            [0.0, 10.0, 0.0, 1.0],
            [-20.0, 10.0, 0.0, h],
            [-20.0, -30.0, 0.0, 1.0],
        ],
    ];
    let walls = arcs.map(|a| {
        let key = ConicCylinderKey::new([0.0, 0.0, 1.0], &[a]).expect("a conic wall");
        conic_chain(key, 0..1).expect("its surface")
    });
    let (a, b) = ([0.0, 10.0, 0.0], [0.0, 10.0, 10.0]);
    let curve = line(a, b);
    // The joint is an end of each arc: a side of its tile, exactly.
    let ends = [0, 1].map(|i| walls[i].inverse(a).expect("finite").uv[0].round());
    let on = [0, 1].map(|i| {
        let (ua, ub) = (
            walls[i].inverse(a).expect("finite"),
            walls[i].inverse(b).expect("finite"),
        );
        assert!(ua.dev < 1e-8 && ub.dev < 1e-8);
        assert!((ua.uv[0] - ends[i]).abs() < 1e-12, "{ua:?}");
        PCurve::from_spans(
            vec![PSpan::Iso {
                fixed: Dir::U,
                at: ends[i],
                m: [[ua.uv[1], 1.0], [ub.uv[1], 1.0]],
            }],
            vec![0.0, 1.0],
        )
        .expect("an iso line")
    });
    // Outward along `+y` at the joint: the walls bound the profile's
    // inside.
    let uv = [0, 1].map(|i| [ends[i], walls[i].inverse(a).expect("finite").uv[1]]);
    let flipped = [0, 1].map(|i| flipped_at(&walls[i], uv[i], [0.0, 1.0, 0.0]));
    let far = [[10.0, 0.0], [-20.0, -30.0]].map(|p| line([p[0], p[1], 0.0], [p[0], p[1], 10.0]));
    Osculating {
        walls,
        curve,
        on,
        flipped,
        far,
    }
}

/// Walls at a G² joint of two conics have no positive bound on their
/// relative curvature: their window comes from the next order, the arcs
/// apart by `x⁴·(1/8000 − 1/32000)` at `x` from the joint, past `δ` from
/// about 1.1 mm on; never empty, never refused (§12 row v14.39 windows).
#[test]
fn walls_at_an_osculating_joint_have_their_tube() {
    let o = osculating();
    // The circle's wall reaches `+x` from the edge running `+z`: its
    // coedge runs the edge's way.
    let side = |i: usize| Side {
        surface: &o.walls[i],
        pcurve: &o.on[i],
        flipped: o.flipped[i],
        reversed: i == 1,
        seam: 0.0,
    };
    let edge = Edge {
        curve: &o.curve,
        t_range: [0.0, 1.0],
        sides: [side(0), side(1)],
        one_surface: false,
    };
    let near: Vec<window::Arc3> = o
        .far
        .iter()
        .flat_map(|c| window::arcs(c, [0.0, 1.0]).expect("its spans"))
        .collect();
    let t = tube(window_alone(&edge, &near));
    assert!((1.0..4.0).contains(&t.rho_min), "{t:?}");
    // The strips reach half way along the arcs (their tiles' halves):
    // 7.65 mm from the edge on the circle.
    assert_eq!(t.rho_adj, 4.0, "{t:?}");
}

/// A fillet leaving its support at a sharp edge of angle `φ` beside it
/// and curling back across it at `2φ` of its arc (radius 10): it crosses
/// the ruled surface between them near the edge. At `φ = 0.001` that is
/// inside `ρ_min = δ/sin φ` (about 0.13 mm): the window is certified
/// empty, from a failing point. At `φ = 0.05` it is far
/// outside: the strips narrow below the failing point and the tube lies
/// inside it.
#[test]
fn a_face_curling_back_narrows_or_empties_the_window() {
    let a = 0.001f64;
    let f = fillet(0.75, false, (cos(a), sin(a)));
    match window_alone(&f.edge().0, &[]) {
        Ok(window::Window::Empty { rho_min, rho_graph }) => {
            // No power of two between them.
            assert!(rho_graph < 2.0 * rho_min, "{rho_graph}");
            assert!(rho_min > DELTA / a * 0.98, "{rho_min}");
        }
        other => panic!("{other:?}"),
    }
    let a = 0.05f64;
    let f = fillet(0.75, false, (cos(a), sin(a)));
    let t = tube(window_alone(&f.edge().0, &[]));
    assert!(t.rho_adj < 10.0 * 2.0 * a, "{t:?}");
}
