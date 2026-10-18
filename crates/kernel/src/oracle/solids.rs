//! The solids the runs count: a box, a half cylinder and a half sphere
//! zone, each a closed body in the minimal store on plane, cylinder and
//! sphere tiles, at any rigid placement and lattice level, with every
//! p-curve's middle control point perturbed (the ribbons then have
//! width).
//!
//! - **Box**: a corner and three orthogonal edge vectors; six plane
//!   faces, twelve straight edges.
//! - **Half cylinder**: radius `r` about an axis, axial range `z` from the
//!   axis's foot point, the half from angle cell `first` of its lattice
//!   (`first·90°` from the frame's `e₁`) through 180°; the cylinder's
//!   face, the plane through the axis and two half-disc caps. Arcs are
//!   the cylinder's iso-lines, so the face's parametrisation and the
//!   edges' agree.
//! - **Sphere zone**: the sphere between latitudes ±45° over 180° of
//!   longitude from angle cell `first`, the sphere face at latitude level
//!   1 (its tile rows at ±45° are the zone's circles), two caps and the
//!   plane through the axis. The circles and meridian arcs are the
//!   tiles' boundary rows, bit for bit.
//!
//! Plane faces' p-curves are their edges' control points mapped by the
//! plane's frame (at its level); a face's loop runs round its outward
//! normal, so on a plane whose stored normal is the other way it runs
//! clockwise in UV and the face is flipped.

use glam::DVec3;
use varde_testkit::rng::Rng;

use crate::geom::curve::{Curve3D, PCurve, Span2, Span3};
use crate::geom::form::{CylinderKey, PlaneKey, SphereKey};
use crate::geom::lattice::{CELL, Lattice, Level, Rect};
use crate::geom::surface::Surface;
use crate::infra::budget::{Budget, Work};
use crate::topo::brep_min::{Brep, EdgeId, FaceId, VertId};

const ROOT_HALF: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// The seam target of the solids' coedges (mm): a quarter of a
/// tolerance of `10⁻⁵`.
pub(super) const SEAM_TARGET: f64 = 2.5e-6;

/// A solid's shape and placement.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Shape {
    /// Corner `o`, edges `axes` (orthogonal, right-handed).
    Box { o: [f64; 3], axes: [[f64; 3]; 3] },
    /// A polyhedron: vertices, and faces as loops of vertex indices round
    /// their outward normals (planar, convex or not).
    Poly {
        verts: Vec<[f64; 3]>,
        faces: Vec<Vec<usize>>,
    },
    /// See the module docs; `z` along the stored axis from `point`'s
    /// foot on it.
    HalfCylinder {
        point: [f64; 3],
        axis: [f64; 3],
        radius: f64,
        z: [f64; 2],
        first: i64,
    },
    /// The region between a quarter of a cylinder (angle cell `first`)
    /// and its two tangent planes at the quarter's ends, which meet at
    /// the corner `foot + r·(d₀ + d₁)`, over the axial range `z` as for
    /// [`Shape::HalfCylinder`]: its caps have a cusp at each end of the
    /// arc, and the cylinder's face meets each plane tangentially.
    Spandrel {
        point: [f64; 3],
        axis: [f64; 3],
        radius: f64,
        z: [f64; 2],
        first: i64,
    },
    /// See the module docs.
    SphereZone {
        centre: [f64; 3],
        axis: [f64; 3],
        radius: f64,
        first: i64,
    },
}

/// How a shape is built: its faces' extra lattice levels, the
/// p-curves' perturbation and the vertex ids' order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Make {
    /// Levels added to the curved faces' lattice level, in `u` and `v`.
    pub levels: [u32; 2],
    /// The plane faces' lattice level, in `u` and `v` (base: one tile).
    pub plane: [u32; 2],
    /// The largest p-curve perturbation, in mm along the surface.
    pub pert: f64,
    /// The perturbation's seed.
    pub pert_seed: u64,
    /// The vertex ids' shuffle seed (`None`: in build order).
    pub ids: Option<u64>,
}

impl Make {
    pub(super) const PLAIN: Make = Make {
        levels: [0, 0],
        plane: [0, 0],
        pert: 0.0,
        pert_seed: 0,
        ids: None,
    };
}

/// A solid built: its store, which edges are straight (one line span),
/// and per face its UV unit lengths (what a split compares).
#[derive(Clone, Debug)]
pub(super) struct Solid {
    pub brep: Brep,
    pub straight: Vec<bool>,
    pub scale: Vec<[f64; 2]>,
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
    a.map(|x| x * k)
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn pt(p: [f64; 3], w: f64) -> [f64; 4] {
    [p[0], p[1], p[2], w]
}

/// A line span from `a` to `b`, its middle control point the rounded
/// midpoint: on the chord exactly where `a + b` is exact in each
/// coordinate (as [`snap`] makes it), else a hair off it, a curve
/// ([`is_line`]).
pub(super) fn line(a: [f64; 3], b: [f64; 3]) -> Span3 {
    let m = [0, 1, 2].map(|k| 0.5 * (a[k] + b[k]));
    [pt(a, 1.0), pt(m, 1.0), pt(b, 1.0)]
}

fn uv_line(a: [f64; 2], b: [f64; 2]) -> Span2 {
    let m = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    [[a[0], a[1], 1.0], [m[0], m[1], 1.0], [b[0], b[1], 1.0]]
}

/// A straight p-curve from `a` (at `t0`) to `b` (at `t1`), linear in
/// `t`, split where it crosses the tile lines (integer `u` or `v`): each
/// cut's `t` rounded once and its point placed on the line exactly, so
/// every span lies in one tile.
fn uv_path(a: [f64; 2], b: [f64; 2], t0: f64, t1: f64) -> (Vec<Span2>, Vec<f64>) {
    let mut cuts: Vec<(f64, [f64; 2])> = Vec::new();
    for k in 0..2 {
        let (lo, hi) = if a[k] < b[k] {
            (a[k], b[k])
        } else {
            (b[k], a[k])
        };
        let mut x = lo.floor() + 1.0;
        while x < hi {
            let f = (x - a[k]) / (b[k] - a[k]);
            let mut p = [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f];
            p[k] = x;
            cuts.push((t0 + (t1 - t0) * f, p));
            x += 1.0;
        }
    }
    cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
    cuts.dedup_by(|x, y| x.0 == y.0);
    let mut pts = vec![(t0, a)];
    pts.extend(cuts.into_iter().filter(|c| c.0 > t0 && c.0 < t1));
    pts.push((t1, b));
    let spans = pts.windows(2).map(|w| uv_line(w[0].1, w[1].1)).collect();
    let knots = pts.iter().map(|p| p.0).collect();
    (spans, knots)
}

/// A p-curve of straight pieces: `(from, to)` in UV per edge span, the
/// spans over `t` `[k, k + 1]`, each split at the tile lines.
fn uv_chain(pieces: &[([f64; 2], [f64; 2])]) -> PCurve {
    let (mut spans, mut knots) = (Vec::new(), vec![0.0]);
    for (k, &(a, b)) in pieces.iter().enumerate() {
        let (s, t) = uv_path(a, b, k as f64, k as f64 + 1.0);
        spans.extend(s);
        knots.extend_from_slice(&t[1..]);
    }
    PCurve::new(spans, knots).expect("a p-curve")
}

/// Whether a chain is one line span, its middle control point on the
/// chord exactly (the midpoint, `a + b` exact): the straight edge the
/// counting reads by its end points. A rounded midpoint bends the stored
/// curve off the chord (by up to half an ulp), and the counting surface
/// ends on the stored curve.
pub(super) fn is_line(spans: &[Span3]) -> bool {
    let [s] = spans else { return false };
    s.iter().all(|p| p[3] == 1.0)
        && (0..3).all(|k| {
            let (a, b) = (s[0][k], s[2][k]);
            let sum = a + b;
            // The rounding error of `a + b` (Knuth's two-sum).
            let bb = sum - a;
            let err = (a - (sum - bb)) + (b - bb);
            err == 0.0 && (0.5 * sum).to_bits() == s[1][k].to_bits() && 2.0 * s[1][k] == sum
        })
}

/// `verts` with every coordinate rounded to a multiple of `2^(E − 52)`,
/// `2^E` above the largest magnitude among them (at most one ulp of the
/// largest binade): so the sum of any two coordinates is exact and a
/// [`line`] between two of them is straight exactly.
pub(super) fn snap(verts: &[[f64; 3]]) -> Vec<[f64; 3]> {
    let m = verts
        .iter()
        .flatten()
        .fold(0.0f64, |m, x| if x.abs() > m { x.abs() } else { m });
    if m == 0.0 || !m.is_finite() {
        return verts.to_vec();
    }
    // `2^E > m`, from the exponent bits of `m`.
    let e = ((m.to_bits() >> 52) & 0x7ff) as i64 - 1023 + 1;
    let q = pow2_signed(e - 52);
    verts
        .iter()
        .map(|p| p.map(|x| (x / q).round() * q))
        .collect()
}

/// `2^k`, exact, for `k` within the normal range.
fn pow2_signed(k: i64) -> f64 {
    f64::from_bits(((k + 1023) as u64) << 52)
}

/// The p-curve on a plane of a chain's spans at the plane surface's
/// level `level`: each control point mapped by the frame,
/// `((x − o)·axis / CELL + ½)·2^level`, its weight kept (the same point
/// maps to the same bits on every curve through it).
fn on_plane(lat: &Lattice, level: Level, spans: &[Span3], knots: &[f64]) -> PCurve {
    let (o, [u, v, _]) = (lat.origin(), lat.axes());
    let (su, sv) = (pow2(level.u), pow2(level.v));
    let spans = spans
        .iter()
        .map(|s| {
            s.map(|h| {
                let d = [h[0] - o[0], h[1] - o[1], h[2] - o[2]];
                let along = |a: [f64; 3]| (d[0] * a[0] + d[1] * a[1] + d[2] * a[2]) / CELL + 0.5;
                [along(u) * su, along(v) * sv, h[3]]
            })
        })
        .collect();
    PCurve::new(spans, knots.to_vec()).expect("a plane p-curve")
}

/// `2^k`, exact (`k < 64`).
fn pow2(k: u32) -> f64 {
    (1u64 << k) as f64
}

/// A plane face to build: its outward normal, a point on it, and its
/// loop as `(edge, reversed)` per coedge.
type PlaneFace = ([f64; 3], [f64; 3], Vec<(usize, bool)>);

/// The loops of one face as the builder takes them: `(edge, reversed,
/// p-curve)` per coedge.
type Uses = Vec<(usize, bool, PCurve)>;

/// Builds a solid from its vertices, curves (`(spans, knots, ends)` per
/// edge, ends by build index, canonicalised here) and faces (surface,
/// its UV unit lengths, its one loop), the vertex ids shuffled and the
/// p-curves perturbed as `make` says.
struct Parts {
    verts: Vec<[f64; 3]>,
    edges: Vec<(Vec<Span3>, Vec<f64>, [usize; 2])>,
    faces: Vec<(Surface, [f64; 2], bool, Uses)>,
}

impl Parts {
    fn build(self, make: &Make) -> Solid {
        let n = self.verts.len();
        // Vertex `k` of the build gets id `id[k]`.
        let mut id: Vec<usize> = (0..n).collect();
        if let Some(seed) = make.ids {
            let mut rng = Rng::new(seed);
            for k in (1..n).rev() {
                let j = (rng.next_u64() % (k as u64 + 1)) as usize;
                id.swap(k, j);
            }
        }
        let mut b = Brep::new();
        let mut by_id = vec![[0.0; 3]; n];
        for (k, p) in self.verts.iter().enumerate() {
            by_id[id[k]] = *p;
        }
        let vs: Vec<VertId> = by_id
            .iter()
            .map(|&p| b.add_vert(p).expect("a vertex"))
            .collect();
        let mut rng = Rng::new(make.pert_seed);
        let mut edges = Vec::new();
        // Per build edge: whether its curve was reversed to run from the
        // lower id.
        let mut flip = Vec::new();
        let mut straight = Vec::new();
        for (spans, knots, [s, e]) in &self.edges {
            let (lo, hi) = (id[*s], id[*e]);
            let reversed = lo > hi;
            let (spans, knots) = if reversed {
                reverse(spans, knots)
            } else {
                (spans.clone(), knots.clone())
            };
            straight.push(is_line(&spans));
            let t1 = knots[knots.len() - 1];
            let c = b
                .add_curve(Curve3D::new(spans, knots.clone()).expect("a curve"))
                .expect("a curve id");
            let (v0, v1) = if reversed { (hi, lo) } else { (lo, hi) };
            edges.push(
                b.add_edge(c, [knots[0], t1], vs[v0], vs[v1])
                    .expect("an edge"),
            );
            flip.push(reversed);
        }
        let mut scales = Vec::new();
        for (surface, unit, flipped, uses) in self.faces {
            let rect = surface.rect().clone();
            let s = b.add_surface(surface).expect("a surface");
            let f = b.add_face(s, flipped).expect("a face");
            let uses = uses
                .into_iter()
                .map(|(e, rev, p)| {
                    let p = if flip[e] { reverse_uv(&p) } else { p };
                    let p = perturb(&p, &rect, unit, make.pert, &mut rng);
                    (edges[e], rev != flip[e], p)
                })
                .collect::<Vec<(EdgeId, bool, PCurve)>>();
            let shown = format!(
                "{rect:?} {:?}",
                uses.iter()
                    .map(|u| u.2.spans().to_vec())
                    .collect::<Vec<_>>()
            );
            b.add_loop(f, true, uses, SEAM_TARGET, &mut Work::new(&Budget::DEFAULT))
                .unwrap_or_else(|e| panic!("a loop: {e:?} {shown}"));
            scales.push(unit);
        }
        debug_assert_eq!(b.check_ribbons(), Ok(()));
        Solid {
            brep: b,
            straight,
            scale: scales,
        }
    }
}

/// A chain run the other way, over the same knots reflected.
fn reverse(spans: &[Span3], knots: &[f64]) -> (Vec<Span3>, Vec<f64>) {
    let (t0, t1) = (knots[0], knots[knots.len() - 1]);
    let spans = spans.iter().rev().map(|s| [s[2], s[1], s[0]]).collect();
    let knots = knots.iter().rev().map(|&t| t0 + (t1 - t)).collect();
    (spans, knots)
}

fn reverse_uv(p: &PCurve) -> PCurve {
    let k = p.knots();
    let (t0, t1) = (k[0], k[k.len() - 1]);
    let spans = p.spans().iter().rev().map(|s| [s[2], s[1], s[0]]).collect();
    let knots = k.iter().rev().map(|&t| t0 + (t1 - t)).collect();
    PCurve::new(spans, knots).expect("a reversed p-curve")
}

/// `p` with each span's middle control point moved by up to `pert` mm
/// (`unit` the face's UV unit lengths), kept inside the face's
/// rectangle (a p-curve on a tile line bulges into the face).
fn perturb(p: &PCurve, rect: &Rect, unit: [f64; 2], pert: f64, rng: &mut Rng) -> PCurve {
    if pert == 0.0 {
        return p.clone();
    }
    let lo = [rect.u.start as f64, rect.v.start as f64];
    let hi = [rect.u.end as f64, rect.v.end as f64];
    let offsets: Vec<[f64; 2]> = p
        .spans()
        .iter()
        .map(|s| {
            let m = s[1];
            std::array::from_fn(|k| {
                // The span's tile (its ends' middle's), within the face.
                let c = (0.5 * (s[0][k] + s[2][k])).floor();
                let c = if c > hi[k] - 1.0 { hi[k] - 1.0 } else { c };
                let l = if c > lo[k] { c } else { lo[k] };
                let h = if c + 1.0 < hi[k] { c + 1.0 } else { hi[k] };
                let d = rng.range(-pert, pert) / unit[k];
                let at = m[k] + d;
                if at < l || at > h { -d } else { d }
            })
        })
        .collect();
    p.perturbed(&offsets).expect("a perturbed p-curve")
}

impl Shape {
    /// The solid of this shape, built as `make` says.
    pub(super) fn build(&self, make: &Make) -> Solid {
        match self {
            Shape::Box { o, axes } => cuboid(*o, *axes, make),
            Shape::Poly { verts, faces } => polyhedron(verts, faces, make),
            Shape::Spandrel {
                point,
                axis,
                radius,
                z,
                first,
            } => spandrel(*point, *axis, *radius, *z, *first, make),
            Shape::HalfCylinder {
                point,
                axis,
                radius,
                z,
                first,
            } => half_cylinder(*point, *axis, *radius, *z, *first, make),
            Shape::SphereZone {
                centre,
                axis,
                radius,
                first,
            } => sphere_zone(*centre, *axis, *radius, *first, make),
        }
    }

    /// A random placement of a shape of kind `kind` (0 box, 1 half
    /// cylinder, 2 sphere zone) of size about `size` near `at`.
    pub(super) fn random(rng: &mut Rng, kind: u32, at: DVec3, size: f64) -> Shape {
        let r = rotation(rng);
        let [x, y, z] = [DVec3::X, DVec3::Y, DVec3::Z].map(|a| r * a);
        match kind {
            0 => {
                let l = [0, 1, 2].map(|_| size * rng.range(0.5, 1.5));
                let o = at - (x * l[0] + y * l[1] + z * l[2]) * 0.5;
                Shape::Box {
                    o: o.to_array(),
                    axes: [
                        (x * l[0]).to_array(),
                        (y * l[1]).to_array(),
                        (z * l[2]).to_array(),
                    ],
                }
            }
            1 => {
                let h = size * rng.range(0.8, 2.0);
                Shape::HalfCylinder {
                    point: (at - z * (0.5 * h)).to_array(),
                    axis: z.to_array(),
                    radius: size * rng.range(0.4, 0.9),
                    z: [0.0, h],
                    first: (rng.next_u64() % 4) as i64,
                }
            }
            _ => Shape::SphereZone {
                centre: at.to_array(),
                axis: z.to_array(),
                radius: size * rng.range(0.5, 1.0),
                first: (rng.next_u64() % 4) as i64,
            },
        }
    }

    /// The shape turned by the exact rotation `rot` (a signed permutation
    /// of the coordinates), the half's angle cell found again on the
    /// turned key's frame.
    pub(super) fn turned(&self, rot: &Turn) -> Shape {
        match self {
            Shape::Box { o, axes } => Shape::Box {
                o: rot.apply(*o),
                axes: axes.map(|a| rot.apply(a)),
            },
            Shape::Poly { verts, faces } => Shape::Poly {
                verts: verts.iter().map(|&p| rot.apply(p)).collect(),
                faces: faces.clone(),
            },
            Shape::HalfCylinder {
                point,
                axis,
                radius,
                z,
                first,
            } => {
                let key = CylinderKey::new(*point, *axis, *radius).expect("a cylinder");
                let lat = Lattice::new(key.into()).expect("a lattice");
                let d0 = rot.apply(quarter(&lat, *first));
                let (point, axis) = (rot.apply(*point), rot.apply(*axis));
                let key = CylinderKey::new(point, axis, *radius).expect("a cylinder");
                let lat2 = Lattice::new(key.into()).expect("a lattice");
                // The turned axis may be stored the other way: then the
                // axial range is mirrored about the foot, and the half
                // from `d0` runs the other way round.
                let same = dot(lat2.axes()[2], rot.apply(lat.axes()[2])) > 0.0;
                let (z, first) = if same {
                    (*z, cell_of(&lat2, d0))
                } else {
                    ([-z[1], -z[0]], cell_of(&lat2, d0) - 2)
                };
                Shape::HalfCylinder {
                    point,
                    axis,
                    radius: *radius,
                    z,
                    first: first.rem_euclid(4),
                }
            }
            Shape::Spandrel {
                point,
                axis,
                radius,
                z,
                first,
            } => {
                let lat = Lattice::new(
                    CylinderKey::new(*point, *axis, *radius)
                        .expect("a cylinder")
                        .into(),
                )
                .expect("a lattice");
                let turned = Shape::HalfCylinder {
                    point: *point,
                    axis: *axis,
                    radius: *radius,
                    z: *z,
                    first: *first,
                }
                .turned(rot);
                let Shape::HalfCylinder {
                    point,
                    axis,
                    radius,
                    z,
                    first,
                } = turned
                else {
                    unreachable!("a half cylinder turns into one")
                };
                // A half from `d0` running the other way round starts a
                // quarter later for the quarter.
                let key = CylinderKey::new(point, axis, radius).expect("a cylinder");
                let lat2 = Lattice::new(key.into()).expect("a lattice");
                let same = dot(lat2.axes()[2], rot.apply(lat.axes()[2])) > 0.0;
                Shape::Spandrel {
                    point,
                    axis,
                    radius,
                    z,
                    first: if same {
                        first
                    } else {
                        (first + 1).rem_euclid(4)
                    },
                }
            }
            Shape::SphereZone {
                centre,
                axis,
                radius,
                first,
            } => {
                let key = SphereKey::new(*centre, *radius, *axis).expect("a sphere");
                let lat = Lattice::new(key.into()).expect("a lattice");
                let d0 = rot.apply(quarter(&lat, *first));
                let (centre, axis) = (rot.apply(*centre), rot.apply(*axis));
                let key = SphereKey::new(centre, *radius, axis).expect("a sphere");
                let lat2 = Lattice::new(key.into()).expect("a lattice");
                let same = dot(lat2.axes()[2], rot.apply(lat.axes()[2])) > 0.0;
                let first = if same {
                    cell_of(&lat2, d0)
                } else {
                    cell_of(&lat2, d0) - 2
                };
                Shape::SphereZone {
                    centre,
                    axis,
                    radius: *radius,
                    first: first.rem_euclid(4),
                }
            }
        }
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// An exact rotation: a signed permutation of the coordinates,
/// determinant `+1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Turn {
    /// Output coordinate `k` is `sign[k]·p[perm[k]]`.
    pub perm: [usize; 3],
    pub sign: [i8; 3],
}

impl Turn {
    pub(super) fn apply(&self, p: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|k| f64::from(self.sign[k]) * p[self.perm[k]])
    }

    pub(super) fn apply_v(&self, p: DVec3) -> DVec3 {
        DVec3::from_array(self.apply(p.to_array()))
    }

    /// The 90° turns about the world axes, `x`, `y` and `z`.
    pub(super) const QUARTERS: [Turn; 3] = [
        // About x: (x, y, z) → (x, −z, y).
        Turn {
            perm: [0, 2, 1],
            sign: [1, -1, 1],
        },
        // About y: (x, y, z) → (z, y, −x).
        Turn {
            perm: [2, 1, 0],
            sign: [1, 1, -1],
        },
        // About z: (x, y, z) → (−y, x, z).
        Turn {
            perm: [1, 0, 2],
            sign: [-1, 1, 1],
        },
    ];
}

/// The radius direction at angle cell `k`'s start: `e₁, e₂, −e₁, −e₂`.
fn quarter(lat: &Lattice, k: i64) -> [f64; 3] {
    let [e1, e2, _] = lat.axes();
    match k.rem_euclid(4) {
        0 => e1,
        1 => e2,
        2 => scale(e1, -1.0),
        _ => scale(e2, -1.0),
    }
}

/// The angle cell whose start direction is nearest `d`.
fn cell_of(lat: &Lattice, d: [f64; 3]) -> i64 {
    (0..4)
        .max_by(|&a, &b| dot(quarter(lat, a), d).total_cmp(&dot(quarter(lat, b), d)))
        .expect("four cells")
}

/// A random rotation, from a random unit quaternion.
pub(super) fn rotation(rng: &mut Rng) -> glam::DQuat {
    loop {
        let q = glam::DQuat::from_xyzw(
            rng.range(-1.0, 1.0),
            rng.range(-1.0, 1.0),
            rng.range(-1.0, 1.0),
            rng.range(-1.0, 1.0),
        );
        let l = q.length();
        if l > 0.1 && l <= 1.0 {
            return q / l;
        }
    }
}

/// A plane surface through `p` with normal `n`, at its level plus
/// `levels`, its rectangle the tiles holding the points `pts`; and its
/// UV unit lengths.
fn plane_surface(
    n: [f64; 3],
    p: [f64; 3],
    levels: [u32; 2],
    pts: &[[f64; 3]],
) -> (Surface, [f64; 2], bool) {
    let lat = Lattice::new(PlaneKey::new(n, p).expect("a plane").into()).expect("a lattice");
    let level = Level {
        u: levels[0],
        v: levels[1],
    };
    let (o, [u, v, _]) = (lat.origin(), lat.axes());
    let range = |a: [f64; 3], s: f64| {
        let along = |x: [f64; 3]| (dot(sub(x, o), a) / CELL + 0.5) * s;
        let lo = pts
            .iter()
            .map(|&x| along(x))
            .fold(f64::INFINITY, |m, x| if x < m { x } else { m });
        let hi = pts
            .iter()
            .map(|&x| along(x))
            .fold(f64::NEG_INFINITY, |m, x| if x > m { x } else { m });
        // A margin of a tile each side: perturbed p-curves bulge.
        (lo.floor() as i64 - 1)..(hi.floor() as i64 + 2)
    };
    let (su, sv) = (pow2(level.u), pow2(level.v));
    let rect = Rect {
        level,
        u: range(u, su),
        v: range(v, sv),
    };
    let rect = if levels == [0, 0] {
        lat.half(0).expect("the plane's tile")
    } else {
        rect
    };
    // The stored normal has its first non-zero component positive: the
    // face is flipped where the outward normal is the other way.
    let flipped = dot(lat.axes()[2], n) < 0.0;
    (
        Surface::new(lat, rect).expect("a plane surface"),
        [CELL / su, CELL / sv],
        flipped,
    )
}

fn cuboid(o: [f64; 3], axes: [[f64; 3]; 3], make: &Make) -> Solid {
    let corner = |c: usize| {
        let mut p = o;
        for (k, a) in axes.iter().enumerate() {
            if c >> k & 1 == 1 {
                p = add(p, *a);
            }
        }
        p
    };
    let verts: Vec<[f64; 3]> = (0..8).map(corner).collect();
    // Faces by corner bits: origin, `a`, `b` with `a × b` outward.
    const FACES: [(usize, usize, usize); 6] = [
        (0, 2, 1),
        (4, 1, 2),
        (0, 4, 2),
        (1, 2, 4),
        (0, 1, 4),
        (2, 4, 1),
    ];
    let faces: Vec<Vec<usize>> = FACES
        .iter()
        .map(|&(c, a, b)| vec![c, c | a, c | a | b, c | b])
        .collect();
    polyhedron(&verts, &faces, make)
}

/// A polyhedron: its vertices (snapped, [`snap`], so its edges are
/// straight exactly) and its faces as loops of vertex indices running
/// round their outward normals, each face planar.
fn polyhedron(verts: &[[f64; 3]], loops: &[Vec<usize>], make: &Make) -> Solid {
    let verts = &snap(verts)[..];
    let mut edges: Vec<(Vec<Span3>, Vec<f64>, [usize; 2])> = Vec::new();
    let mut faces = Vec::new();
    for l in loops {
        let pts: Vec<[f64; 3]> = l.iter().map(|&k| verts[k]).collect();
        // Newell's normal: the loop's area vector.
        let mut n = [0.0; 3];
        for k in 0..pts.len() {
            n = add(n, cross(pts[k], pts[(k + 1) % pts.len()]));
        }
        let (s, unit, flipped) = plane_surface(n, pts[0], make.plane, &pts);
        let (lat, level) = (s.lattice().clone(), s.rect().level);
        let mut uses = Vec::new();
        for k in 0..l.len() {
            let (a, b) = (l[k], l[(k + 1) % l.len()]);
            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
            let e = match edges.iter().position(|e| e.2 == [lo, hi]) {
                Some(e) => e,
                None => {
                    edges.push((vec![line(verts[lo], verts[hi])], vec![0.0, 1.0], [lo, hi]));
                    edges.len() - 1
                }
            };
            let (spans, knots, _) = &edges[e];
            uses.push((e, a != lo, on_plane(&lat, level, spans, knots)));
        }
        faces.push((s, unit, flipped, uses));
    }
    Parts {
        verts: verts.to_vec(),
        edges,
        faces,
    }
    .build(make)
}

fn half_cylinder(
    point: [f64; 3],
    axis: [f64; 3],
    r: f64,
    z: [f64; 2],
    first: i64,
    make: &Make,
) -> Solid {
    let key = CylinderKey::new(point, axis, r).expect("a cylinder");
    let lat = Lattice::new(key.into()).expect("a lattice");
    let [lu, lv] = make.levels;
    let (su, sv) = (pow2(lu), pow2(lv));
    let (foot, a) = (lat.origin(), lat.axes()[2]);
    // `z` from `point` along the stored axis.
    let off = dot(sub(point, foot), a);
    let z = [z[0] + off, z[1] + off];
    let d = [0, 1, 2].map(|k| quarter(&lat, first + k));
    let at = |dir: [f64; 3], h: f64| add(add(foot, scale(dir, r)), scale(a, h));
    // Snapped ([`snap`]): the lines between them straight exactly, the
    // arcs ending on them.
    let verts = snap(&[
        at(d[0], z[0]),
        at(d[2], z[0]),
        at(d[0], z[1]),
        at(d[2], z[1]),
    ]);
    // A half circle at height h from vertex `v0` to `v1`: two quarters,
    // `d0` to `d1` to `d2`.
    let arc = |h: f64, v0: [f64; 3], v1: [f64; 3]| -> Vec<Span3> {
        let quarter = |from: [f64; 3], to: [f64; 3]| -> Span3 {
            [
                pt(at(from, h), 1.0),
                pt(add(at(add(from, to), 0.0), scale(a, h)), ROOT_HALF),
                pt(at(to, h), 1.0),
            ]
        };
        let (mut q0, mut q1) = (quarter(d[0], d[1]), quarter(d[1], d[2]));
        q0[0] = pt(v0, 1.0);
        q1[2] = pt(v1, 1.0);
        vec![q0, q1]
    };
    let edges = vec![
        (arc(z[0], verts[0], verts[1]), vec![0.0, 1.0, 2.0], [0, 1]),
        (arc(z[1], verts[2], verts[3]), vec![0.0, 1.0, 2.0], [2, 3]),
        (vec![line(verts[0], verts[2])], vec![0.0, 1.0], [0, 2]),
        (vec![line(verts[1], verts[3])], vec![0.0, 1.0], [1, 3]),
        (vec![line(verts[0], verts[1])], vec![0.0, 1.0], [0, 1]),
        (vec![line(verts[2], verts[3])], vec![0.0, 1.0], [2, 3]),
    ];
    // The cylinder: angle cells `first..first + 2` at level `lu`, the
    // axial cell 0 at level `lv` (`v = (h/CELL + ½)·2^lv`).
    let vh = |h: f64| (h / CELL + 0.5) * sv;
    let (v0, v1) = (vh(z[0]), vh(z[1]));
    let u0 = first as f64 * su;
    let (u1, u2) = (u0 + su, u0 + 2.0 * su);
    let rect = Rect {
        level: Level { u: lu, v: lv },
        u: (first << lu)..((first + 2) << lu),
        v: (v0.floor() as i64)..(v1.floor() as i64 + 1),
    };
    let cyl = Surface::new(lat, rect).expect("a cylinder surface");
    let arc_p = |v: f64| uv_chain(&[([u0, v], [u1, v]), ([u1, v], [u2, v])]);
    let ruling = |u: f64| uv_chain(&[([u, v0], [u, v1])]);
    let unit = [r * std::f64::consts::FRAC_PI_2 / su, CELL / sv];
    let mut faces = vec![(
        cyl,
        unit,
        false,
        vec![
            (0, false, arc_p(v0)),
            (3, false, ruling(u2)),
            (1, true, arc_p(v1)),
            (2, true, ruling(u0)),
        ],
    )];
    // The plane through the axis (outward `−d1`) and the caps.
    let planes: [PlaneFace; 3] = [
        (
            scale(d[1], -1.0),
            verts[0],
            vec![(2, false), (5, false), (3, true), (4, true)],
        ),
        (scale(a, -1.0), verts[0], vec![(0, true), (4, false)]),
        (a, verts[2], vec![(1, false), (5, true)]),
    ];
    for (n, p, uses) in planes {
        let pts: Vec<[f64; 3]> = uses
            .iter()
            .flat_map(|&(e, _)| {
                edges[e]
                    .0
                    .iter()
                    .flat_map(|s| s.iter().map(|c| [c[0], c[1], c[2]]))
            })
            .collect();
        let (s, unit, flipped) = plane_surface(n, p, make.plane, &pts);
        let (lat, level) = (s.lattice().clone(), s.rect().level);
        let uses = uses
            .into_iter()
            .map(|(e, rev)| (e, rev, on_plane(&lat, level, &edges[e].0, &edges[e].1)))
            .collect();
        faces.push((s, unit, flipped, uses));
    }
    Parts {
        verts,
        edges,
        faces,
    }
    .build(make)
}

fn spandrel(
    point: [f64; 3],
    axis: [f64; 3],
    r: f64,
    z: [f64; 2],
    first: i64,
    make: &Make,
) -> Solid {
    let key = CylinderKey::new(point, axis, r).expect("a cylinder");
    let lat = Lattice::new(key.into()).expect("a lattice");
    let [lu, lv] = make.levels;
    let (su, sv) = (pow2(lu), pow2(lv));
    let (foot, a) = (lat.origin(), lat.axes()[2]);
    let off = dot(sub(point, foot), a);
    let z = [z[0] + off, z[1] + off];
    let (d0, d1) = (quarter(&lat, first), quarter(&lat, first + 1));
    let at = |dir: [f64; 3], h: f64| add(add(foot, scale(dir, r)), scale(a, h));
    let corner = add(d0, d1);
    // Vertices: `P0`, `Q`, `P1` at `z0`, then at `z1`.
    let verts = vec![
        at(d0, z[0]),
        at(corner, z[0]),
        at(d1, z[0]),
        at(d0, z[1]),
        at(corner, z[1]),
        at(d1, z[1]),
    ];
    let arc = |h: f64| -> Vec<Span3> {
        vec![[
            pt(at(d0, h), 1.0),
            pt(at(corner, h), ROOT_HALF),
            pt(at(d1, h), 1.0),
        ]]
    };
    let ln = |i: usize, j: usize| (vec![line(verts[i], verts[j])], vec![0.0, 1.0], [i, j]);
    let edges = vec![
        (arc(z[0]), vec![0.0, 1.0], [0, 2]),
        (arc(z[1]), vec![0.0, 1.0], [3, 5]),
        ln(0, 3),
        ln(2, 5),
        ln(1, 4),
        ln(0, 1),
        ln(1, 2),
        ln(3, 4),
        ln(4, 5),
    ];
    let vh = |h: f64| (h / CELL + 0.5) * sv;
    let (v0, v1) = (vh(z[0]), vh(z[1]));
    let (u0, u1) = (first as f64 * su, first as f64 * su + su);
    let rect = Rect {
        level: Level { u: lu, v: lv },
        u: (first << lu)..((first + 1) << lu),
        v: (v0.floor() as i64)..(v1.floor() as i64 + 1),
    };
    let cyl = Surface::new(lat, rect).expect("a cylinder surface");
    let arc_p = |v: f64| uv_chain(&[([u0, v], [u1, v])]);
    let ruling = |u: f64| uv_chain(&[([u, v0], [u, v1])]);
    let unit = [r * std::f64::consts::FRAC_PI_2 / su, CELL / sv];
    // The solid lies outside the cylinder: the face is flipped and its
    // loop runs clockwise in UV.
    let mut faces = vec![(
        cyl,
        unit,
        true,
        vec![
            (2, false, ruling(u0)),
            (1, false, arc_p(v1)),
            (3, true, ruling(u1)),
            (0, true, arc_p(v0)),
        ],
    )];
    let planes: [PlaneFace; 4] = [
        (
            d0,
            verts[0],
            vec![(5, false), (4, false), (7, true), (2, true)],
        ),
        (
            d1,
            verts[2],
            vec![(3, false), (8, true), (4, true), (6, false)],
        ),
        (
            scale(a, -1.0),
            verts[0],
            vec![(0, false), (6, true), (5, true)],
        ),
        (a, verts[3], vec![(7, false), (8, false), (1, true)]),
    ];
    for (n, p, uses) in planes {
        let pts: Vec<[f64; 3]> = uses
            .iter()
            .flat_map(|&(e, _)| {
                edges[e]
                    .0
                    .iter()
                    .flat_map(|s| s.iter().map(|c| [c[0], c[1], c[2]]))
            })
            .collect();
        let (s, unit, flipped) = plane_surface(n, p, make.plane, &pts);
        let (lat, level) = (s.lattice().clone(), s.rect().level);
        let uses = uses
            .into_iter()
            .map(|(e, rev)| (e, rev, on_plane(&lat, level, &edges[e].0, &edges[e].1)))
            .collect();
        faces.push((s, unit, flipped, uses));
    }
    Parts {
        verts,
        edges,
        faces,
    }
    .build(make)
}

fn sphere_zone(centre: [f64; 3], axis: [f64; 3], r: f64, first: i64, make: &Make) -> Solid {
    let key = SphereKey::new(centre, r, axis).expect("a sphere");
    let lat = Lattice::new(key.into()).expect("a lattice");
    let [lu, lv] = make.levels;
    let lv = lv + 1;
    let (su, sv) = (1i64 << lu, 1i64 << lv.min(62));
    let a = lat.axes()[2];
    let d1 = quarter(&lat, first + 1);
    // Latitudes ±45°: global `v` from `−sv/2` to `sv/2`.
    let rect = Rect {
        level: Level { u: lu, v: lv },
        u: (first << lu)..((first + 2) << lu),
        v: (-sv / 2)..(sv / 2),
    };
    let sph = Surface::new(lat, rect.clone()).expect("a sphere surface");
    let net = |i: i64, j: i64| *sph.tile(i, j).expect("a tile").net();
    // Rows and columns of the boundary, span by span along `u` or `v`.
    let row = |j: i64, at: usize| -> Vec<Span3> {
        rect.u
            .clone()
            .map(|i| {
                let n = net(i, j);
                [n[at], n[3 + at], n[6 + at]]
            })
            .collect()
    };
    let column = |i: i64, at: usize| -> Vec<Span3> {
        rect.v
            .clone()
            .map(|j| {
                let n = net(i, j);
                [n[3 * at], n[3 * at + 1], n[3 * at + 2]]
            })
            .collect()
    };
    let (ju, jv) = (rect.u.end - 1, rect.v.end - 1);
    let lat0 = row(rect.v.start, 0);
    let lat1 = row(jv, 2);
    let mer0 = column(rect.u.start, 0);
    let mer1 = column(ju, 2);
    let p3 = |s: &[f64; 4]| [s[0], s[1], s[2]];
    let verts = vec![
        p3(&lat0[0][0]),
        p3(&lat0[lat0.len() - 1][2]),
        p3(&lat1[0][0]),
        p3(&lat1[lat1.len() - 1][2]),
    ];
    let knots = |n: usize| (0..=n).map(|k| k as f64).collect::<Vec<f64>>();
    let (nu, nv) = (lat0.len(), mer0.len());
    let edges = vec![
        (lat0, knots(nu), [0, 1]),
        (lat1, knots(nu), [2, 3]),
        (mer0, knots(nv), [0, 2]),
        (mer1, knots(nv), [1, 3]),
        (vec![line(verts[0], verts[1])], vec![0.0, 1.0], [0, 1]),
        (vec![line(verts[2], verts[3])], vec![0.0, 1.0], [2, 3]),
    ];
    // The sphere's p-curves: along tile lines, one span per tile.
    let (u0, u1) = (rect.u.start as f64, rect.u.end as f64);
    let (v0, v1) = (rect.v.start as f64, rect.v.end as f64);
    let along_u = |v: f64| {
        let spans = (0..nu)
            .map(|k| uv_line([u0 + k as f64, v], [u0 + k as f64 + 1.0, v]))
            .collect();
        PCurve::new(spans, knots(nu)).expect("a row p-curve")
    };
    let along_v = |u: f64| {
        let spans = (0..nv)
            .map(|k| uv_line([u, v0 + k as f64], [u, v0 + k as f64 + 1.0]))
            .collect();
        PCurve::new(spans, knots(nv)).expect("a column p-curve")
    };
    let quarter_len = r * std::f64::consts::FRAC_PI_2;
    let unit = [quarter_len / su as f64, quarter_len / sv as f64];
    let mut faces = vec![(
        sph,
        unit,
        false,
        vec![
            (0, false, along_u(v0)),
            (3, false, along_v(u1)),
            (1, true, along_u(v1)),
            (2, true, along_v(u0)),
        ],
    )];
    let planes: [PlaneFace; 3] = [
        (
            scale(d1, -1.0),
            centre,
            vec![(2, false), (5, false), (3, true), (4, true)],
        ),
        (scale(a, -1.0), verts[0], vec![(0, true), (4, false)]),
        (a, verts[2], vec![(1, false), (5, true)]),
    ];
    for (n, p, uses) in planes {
        let pts: Vec<[f64; 3]> = uses
            .iter()
            .flat_map(|&(e, _)| {
                edges[e]
                    .0
                    .iter()
                    .flat_map(|s| s.iter().map(|c| [c[0], c[1], c[2]]))
            })
            .collect();
        let (s, unit, flipped) = plane_surface(n, p, make.plane, &pts);
        let (lat, level) = (s.lattice().clone(), s.rect().level);
        let uses = uses
            .into_iter()
            .map(|(e, rev)| (e, rev, on_plane(&lat, level, &edges[e].0, &edges[e].1)))
            .collect();
        faces.push((s, unit, flipped, uses));
    }
    Parts {
        verts,
        edges,
        faces,
    }
    .build(make)
}

impl Solid {
    /// The faces' ids.
    pub(super) fn faces(&self) -> Vec<FaceId> {
        (0..self.brep.face_count() as u32).map(FaceId).collect()
    }

    /// The edges' ids.
    pub(super) fn edges(&self) -> Vec<EdgeId> {
        (0..self.brep.edge_count() as u32).map(EdgeId).collect()
    }

    /// The vertices' ids.
    pub(super) fn verts(&self) -> Vec<VertId> {
        (0..self.brep.vert_count() as u32).map(VertId).collect()
    }
}

impl Shape {
    /// Whether `p` lies inside the ideal shape (exact forms, no
    /// rounding), `None` within `margin` of its boundary: a cheap check
    /// of the windings away from every tie.
    pub(super) fn inside(&self, p: [f64; 3], margin: f64) -> Option<bool> {
        let clear = |x: f64| (x.abs() >= margin).then_some(x > 0.0);
        let all = |xs: &[f64]| -> Option<bool> {
            let mut inside = true;
            for &x in xs {
                inside &= clear(x)?;
            }
            Some(inside)
        };
        match self {
            Shape::Poly { verts, faces } => {
                // Convex polyhedra only.
                let mut xs = Vec::new();
                for l in faces {
                    let mut n = [0.0; 3];
                    for k in 0..l.len() {
                        n = add(n, cross(verts[l[k]], verts[l[(k + 1) % l.len()]]));
                    }
                    let len = dot(n, n).sqrt();
                    xs.push(-dot(sub(p, verts[l[0]]), n) / len);
                }
                all(&xs)
            }
            Shape::Box { o, axes } => {
                let d = sub(p, *o);
                let mut xs = Vec::new();
                for a in axes {
                    let l = dot(*a, *a).sqrt();
                    let t = dot(d, *a) / l;
                    xs.extend([t, l - t]);
                }
                all(&xs)
            }
            Shape::HalfCylinder {
                point,
                axis,
                radius,
                z,
                first,
            } => {
                let key = CylinderKey::new(*point, *axis, *radius).expect("a cylinder");
                let lat = Lattice::new(key.into()).expect("a lattice");
                let (f, a) = (lat.origin(), lat.axes()[2]);
                let d1 = quarter(&lat, first + 1);
                let r = sub(p, f);
                let rad = sub(r, scale(a, dot(r, a)));
                let h = dot(r, a) - dot(sub(*point, f), a);
                let rho = dot(rad, rad).sqrt();
                all(&[radius - rho, h - z[0], z[1] - h, dot(r, d1)])
            }
            Shape::Spandrel {
                point,
                axis,
                radius,
                z,
                first,
            } => {
                let key = CylinderKey::new(*point, *axis, *radius).expect("a cylinder");
                let lat = Lattice::new(key.into()).expect("a lattice");
                let (f, a) = (lat.origin(), lat.axes()[2]);
                let (d0, d1) = (quarter(&lat, *first), quarter(&lat, first + 1));
                let r = sub(p, f);
                let rad = sub(r, scale(a, dot(r, a)));
                let rho = dot(rad, rad).sqrt();
                let h = dot(r, a) - dot(sub(*point, f), a);
                all(&[
                    rho - radius,
                    radius - dot(r, d0),
                    radius - dot(r, d1),
                    h - z[0],
                    z[1] - h,
                ])
            }
            Shape::SphereZone {
                centre,
                axis,
                radius,
                first,
            } => {
                let key = SphereKey::new(*centre, *radius, *axis).expect("a sphere");
                let lat = Lattice::new(key.into()).expect("a lattice");
                let a = lat.axes()[2];
                let d1 = quarter(&lat, first + 1);
                let r = sub(p, *centre);
                let h = dot(r, a);
                let top = radius * ROOT_HALF;
                all(&[radius - dot(r, r).sqrt(), top - h, h + top, dot(r, d1)])
            }
        }
    }
}
