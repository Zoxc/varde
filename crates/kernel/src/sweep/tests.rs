#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::f64::consts::{PI, TAU};

use glam::DVec3;

use super::*;
use crate::mesh::tests::TOL;
use crate::mesh::{
    CheckError, Face, FaceName, FacePart, Form, Mesh, MeshBuilder, Quadric, Surface,
};
use crate::par::assert_deterministic;
use crate::test_rng::Rng;
use crate::{KernelError, Solid};

/// An axis: a point on it, and square unit axes `u`, `v` across it and
/// `z` along it.
#[derive(Debug, Clone, Copy)]
struct Axis {
    origin: DVec3,
    u: DVec3,
    v: DVec3,
    z: DVec3,
}

impl Axis {
    const Z: Axis = Axis {
        origin: DVec3::ZERO,
        u: DVec3::X,
        v: DVec3::Y,
        z: DVec3::Z,
    };

    fn random(rng: &mut Rng, extent: f64) -> Axis {
        let z = rng.direction();
        let u = z.any_orthonormal_vector();
        Axis {
            origin: rng.point(extent),
            u,
            v: z.cross(u),
            z,
        }
    }

    /// The point `rho` from the axis at height `h`, at angle `phi` from
    /// `u` towards `v`.
    fn at(&self, rho: f64, h: f64, phi: f64) -> DVec3 {
        self.origin + self.z * h + (self.u * phi.cos() + self.v * phi.sin()) * rho
    }

    fn on_axis(&self, h: f64) -> DVec3 {
        self.origin + self.z * h
    }
}

/// A quadric of revolution about an axis, `ρ² = r0 + r1·h + r2·h²`, with
/// how its meridians are built.
#[derive(Debug, Clone, Copy)]
enum Meridians {
    /// Arcs of the circle of this radius around the axis's origin.
    Sphere(f64),
    /// Straight, through the apex at this height, the nappe above it if
    /// `true`: rulings by the geometric mean.
    Cone(f64, bool),
    /// Straight and parallel to the axis.
    Cylinder,
    /// Conics through two points with the surface's tangents, and the
    /// point mirrored across the axis.
    Conic,
}

#[derive(Debug, Clone, Copy)]
struct Revolution {
    axis: Axis,
    r: [f64; 3],
    meridians: Meridians,
}

impl Revolution {
    fn sphere(axis: Axis, radius: f64) -> Self {
        Revolution {
            axis,
            r: [radius * radius, 0.0, -1.0],
            meridians: Meridians::Sphere(radius),
        }
    }

    /// The cone through the apex at height `h0` with `ρ = t·|h − h0|`, its
    /// nappe above the apex if `up`.
    fn cone(axis: Axis, h0: f64, t: f64, up: bool) -> Self {
        let t2 = t * t;
        Revolution {
            axis,
            r: [t2 * h0 * h0, -2.0 * t2 * h0, t2],
            meridians: Meridians::Cone(h0, up),
        }
    }

    fn cylinder(axis: Axis, radius: f64) -> Self {
        Revolution {
            axis,
            r: [radius * radius, 0.0, 0.0],
            meridians: Meridians::Cylinder,
        }
    }

    fn conic(axis: Axis, r: [f64; 3]) -> Self {
        Revolution {
            axis,
            r,
            meridians: Meridians::Conic,
        }
    }

    fn rho(&self, h: f64) -> f64 {
        let [r0, r1, r2] = self.r;
        (r0 + r1 * h + r2 * h * h).sqrt()
    }

    fn quadric(&self) -> Quadric {
        let [r0, r1, r2] = self.r;
        Quadric::revolution(self.axis.origin, self.axis.z, r0, r1, r2).unwrap()
    }

    /// The meridian from height `ha` to `hb` at angle `phi`, between the
    /// points `p` and `q` there.
    fn meridian(&self, p: DVec3, q: DVec3, ha: f64, hb: f64, phi: f64) -> Conic3 {
        let a = &self.axis;
        match self.meridians {
            Meridians::Sphere(radius) => Conic3::arc_between(a.origin, radius, p, q).unwrap(),
            Meridians::Cone(h0, _) => cone_ruling(p, q, a.on_axis(h0)).unwrap(),
            Meridians::Cylinder => Conic3::line(p, q).unwrap(),
            Meridians::Conic => {
                let [_, r1, r2] = self.r;
                let radial = a.u * phi.cos() + a.v * phi.sin();
                // d(ρ²) = 2ρ·dρ = (r1 + 2·r2·h)·dh.
                let tangent = |h: f64| radial * (r1 + 2.0 * r2 * h) + a.z * (2.0 * self.rho(h));
                let mirrored = a.at(-self.rho(ha), ha, phi);
                conic_touching(p, tangent(ha), q, tangent(hb), mirrored).unwrap()
            }
        }
    }

    /// The face this surface's strips are on.
    fn form(&self) -> Form {
        let a = &self.axis;
        match self.meridians {
            Meridians::Sphere(radius) => Form::Sphere {
                centre: a.origin,
                radius,
            },
            Meridians::Cone(h0, up) => {
                let t = self.r[2].sqrt();
                let length = (1.0 + t * t).sqrt();
                Form::Cone {
                    apex: a.on_axis(h0),
                    axis: if up { a.z } else { -a.z },
                    cos: 1.0 / length,
                    sin: t / length,
                }
            }
            Meridians::Cylinder => Form::Cylinder {
                point: a.origin,
                axis: a.z,
                radius: self.rho(0.0),
            },
            Meridians::Conic => Form::Unknown,
        }
    }
}

/// The strip of `surface` between heights `ha < hb` and angles `phi`
/// and `phi + sweep`, by [`revolution_strip`].
fn revolution(surface: &Revolution, ha: f64, hb: f64, phi: f64, sweep: f64) -> [Patch; 2] {
    let a = &surface.axis;
    let (ra, rb) = (surface.rho(ha), surface.rho(hb));
    let [a0, a1] = [phi, phi + sweep].map(|f| a.at(ra, ha, f));
    let [b0, b1] = [phi, phi + sweep].map(|f| a.at(rb, hb, f));
    let bottom = Conic3::arc_between(a.on_axis(ha), ra, a0, a1).unwrap();
    let top = Conic3::arc_between(a.on_axis(hb), rb, b0, b1).unwrap();
    let left = surface.meridian(a0, b0, ha, hb, phi);
    let right = surface.meridian(a1, b1, ha, hb, phi + sweep);
    revolution_strip(&bottom, &top, &left, &right, a.origin, a.z).unwrap()
}

/// Where the property tests put the axis: at the origin for even `i`,
/// so their bounds are relative to the size alone, and up to `1e3` out
/// for odd, where they are relative to the coordinates too.
fn far(i: usize) -> f64 {
    if i.is_multiple_of(2) { 0.0 } else { 1e3 }
}

/// An angle for a strip, from 90° down to a third of a degree (a full
/// turn in 1 024 pieces), on a log scale.
fn sweep(rng: &mut Rng) -> f64 {
    rng.log_range(0.003, 1.0) * PI / 2.0
}

/// The farthest a dense grid of points of `patch` is from `distance`'s
/// zero, and that the patch passes the fold check.
fn farthest(patch: &Patch, distance: impl Fn(DVec3) -> f64) -> f64 {
    assert!(patch.fold_direction().is_some(), "{patch:?}");
    let n = 12;
    let mut worst: f64 = 0.0;
    for i in 0..=n {
        for j in 0..=n - i {
            let u = DVec3::new(i as f64, j as f64, (n - i - j) as f64) / n as f64;
            worst = worst.max(distance(patch.eval(u)));
        }
    }
    worst
}

#[test]
fn sphere_strips_lie_on_their_spheres() {
    let mut rng = Rng::new(1);
    for i in 0..400 {
        let radius = rng.log_range(0.1, 1e3);
        let surface = Revolution::sphere(Axis::random(&mut rng, far(i)), radius);
        let ha = rng.range(-0.9, 0.8) * radius;
        let hb = rng.range(ha / radius + 0.05, 0.95) * radius;
        let sweep = sweep(&mut rng);
        let phi = rng.range(0.0, TAU);
        let centre = surface.axis.origin;
        let scale = radius + centre.abs().max_element();
        for patch in revolution(&surface, ha, hb, phi, sweep) {
            let off = farthest(&patch, |x| ((x - centre).length() - radius).abs());
            assert!(off <= 1e-12 * scale, "{off:e} on {radius} at {centre}");
        }
    }
}

#[test]
fn cone_strips_lie_on_their_cones_for_any_diagonal_plane() {
    let mut rng = Rng::new(2);
    for i in 0..400 {
        let axis = Axis::random(&mut rng, far(i));
        let size = rng.log_range(0.1, 1e3);
        let t = rng.log_range(0.05, 20.0);
        let surface = Revolution::cone(axis, 0.0, t, true);
        let ha = rng.range(0.05, 1.0) * size;
        let hb = ha + rng.range(0.05, 1.0) * size;
        let sweep = sweep(&mut rng);
        let phi = rng.range(0.0, TAU);
        let form = surface.form();
        let reach = hb * (1.0 + t) + axis.origin.abs().max_element();
        let check = |patches: [Patch; 2]| {
            for patch in patches {
                let off = farthest(&patch, |x| form.distance(x));
                assert!(off <= 1e-12 * reach, "{off:e} on {reach}");
            }
        };
        check(revolution(&surface, ha, hb, phi, sweep));
        let (ra, rb) = (surface.rho(ha), surface.rho(hb));
        let [a0, a1] = [phi, phi + sweep].map(|f| axis.at(ra, ha, f));
        let [b0, b1] = [phi, phi + sweep].map(|f| axis.at(rb, hb, f));
        let bottom = Conic3::arc_between(axis.on_axis(ha), ra, a0, a1).unwrap();
        let top = Conic3::arc_between(axis.on_axis(hb), rb, b0, b1).unwrap();
        check(cone_strip(&bottom, &top, axis.origin).unwrap());
        // Any plane through `a0` and `b1` clear of the apex will do.
        for _ in 0..4 {
            let along = rng.direction();
            if let Ok(patches) = cone_strip_along(&bottom, &top, axis.origin, along)
                && patches.iter().all(|p| p.fold_direction().is_some())
            {
                check(patches);
            }
        }
    }
}

#[test]
fn oblique_and_elliptic_cone_strips_are_exact() {
    // A cone over an ellipse or a circle on a plane, its apex anywhere off
    // the plane: a point is on it when its projection from the apex onto
    // the plane is on the bottom's conic.
    let mut rng = Rng::new(3);
    for i in 0..400 {
        let frame = Axis::random(&mut rng, far(i));
        let size = rng.log_range(0.1, 1e3);
        let stretch = rng.range(0.3, 3.0);
        let start = rng.range(0.0, TAU);
        let sweep = sweep(&mut rng);
        let bottom =
            Conic3::arc(frame.origin, frame.u, frame.v * stretch, size, start, sweep).unwrap();
        let apex = frame.origin
            + (frame.u * rng.range(-1.0, 1.0) + frame.v * rng.range(-1.0, 1.0)) * size
            + frame.z * size * rng.range(0.3, 3.0) * if rng.unit() < 0.5 { 1.0 } else { -1.0 };
        let scale = rng.range(0.2, 0.9);
        let shrink = |x: DVec3| apex + (x - apex) * scale;
        let top = Conic3 {
            p0: shrink(bottom.p0),
            c: shrink(bottom.c),
            w: bottom.w,
            p1: shrink(bottom.p1),
        };
        // The bottom's conic is `λ1² = 4w²·λ0·λ2` on its control triangle.
        let on_bottom = |x: DVec3| {
            let k = (frame.origin - apex).dot(frame.z) / (x - apex).dot(frame.z);
            let y = apex + (x - apex) * k;
            let form = Form::ConicCylinder {
                conic: bottom,
                along: frame.z,
            };
            form.distance(y) * scale.min(1.0)
        };
        for patch in cone_strip(&bottom, &top, apex).unwrap() {
            let off = farthest(&patch, on_bottom);
            let scale = size + frame.origin.abs().max_element();
            assert!(off <= 1e-12 * scale, "{off:e} on {size}");
        }
    }
}

#[test]
fn linear_rulings_miss_the_cone() {
    // The geometric mean is what makes them exact: the midpoint rule (a
    // cylinder's) puts a quarter turn of a 45° cone from 1 to 2 high
    // 3e-3 off.
    let surface = Revolution::cone(Axis::Z, 0.0, 1.0, true);
    let (ha, hb, sweep) = (1.0, 2.0, PI / 2.0);
    let [a0, a1] = [0.0, sweep].map(|f| Axis::Z.at(ha, ha, f));
    let [b0, b1] = [0.0, sweep].map(|f| Axis::Z.at(hb, hb, f));
    let bottom = Conic3::arc_between(Axis::Z.on_axis(ha), ha, a0, a1).unwrap();
    let top = Conic3::arc_between(Axis::Z.on_axis(hb), hb, b0, b1).unwrap();
    let [left, right] = [(a0, b0), (a1, b1)].map(|(p, q)| Conic3::line(p, q).unwrap());
    let form = surface.form();
    let off = revolution_strip(&bottom, &top, &left, &right, DVec3::ZERO, DVec3::Z)
        .unwrap()
        .iter()
        .map(|p| farthest(p, |x| form.distance(x)))
        .fold(0.0, f64::max);
    assert!(off > 2e-3, "{off:e}");
    let left = cone_ruling(a0, b0, DVec3::ZERO).unwrap();
    assert!((left.c.length() - 2f64.sqrt() * 2f64.sqrt()).abs() < 1e-15);
}

#[test]
fn rulings_are_the_same_either_way_round() {
    let mut rng = Rng::new(4);
    for _ in 0..100 {
        let apex = rng.point(1e3);
        let d = rng.direction();
        let (p, q) = (
            apex + d * rng.log_range(1e-3, 1e3),
            apex + d * rng.log_range(1e-3, 1e3),
        );
        let (there, back) = (
            cone_ruling(p, q, apex).unwrap(),
            cone_ruling(q, p, apex).unwrap(),
        );
        assert_eq!(there.reversed(), back);
        let mean = ((p - apex).length() * (q - apex).length()).sqrt();
        assert!(((there.c - apex).length() - mean).abs() <= 1e-13 * mean.max(apex.length()));
    }
    // At the apex, or either side of it: no ruling.
    let x = DVec3::X;
    assert_eq!(
        cone_ruling(DVec3::ZERO, x, DVec3::ZERO),
        Err(PatchError::Degenerate)
    );
    assert_eq!(cone_ruling(-x, x, DVec3::ZERO), Err(PatchError::Degenerate));
}

#[test]
fn quadrics_of_revolution_strips_lie_on_them() {
    let mut rng = Rng::new(5);
    let mut kinds = [0; 4];
    for i in 0..400 {
        let axis = Axis::random(&mut rng, far(i / 4));
        let size = rng.log_range(0.1, 1e2);
        let s2 = size * size;
        // Ellipsoids, paraboloids, hyperboloids of one and two sheets,
        // and the heights where each has a radius.
        let kind = i % 4;
        kinds[kind] += 1;
        let (r, lo, hi) = match kind {
            0 => {
                let k = rng.range(0.2, 5.0);
                ([s2, 0.0, -k * k], -0.9 * size / k, 0.9 * size / k)
            }
            1 => ([s2 * 0.1, rng.range(0.2, 5.0) * size, 0.0], 0.0, 2.0 * size),
            2 => ([s2, 0.0, rng.range(0.1, 5.0)], -2.0 * size, 2.0 * size),
            _ => {
                let k = rng.range(0.2, 5.0);
                ([-s2, 0.0, k * k], 1.2 * size / k, 3.0 * size / k)
            }
        };
        let surface = Revolution::conic(axis, r);
        let ha = rng.range(lo, hi - 0.1 * (hi - lo));
        let hb = rng.range(ha + 0.05 * (hi - lo), hi);
        let sweep = sweep(&mut rng);
        let phi = rng.range(0.0, TAU);
        let quadric = surface.quadric();
        let reach = surface
            .rho(ha)
            .max(surface.rho(hb))
            .max(ha.abs())
            .max(hb.abs())
            + axis.origin.abs().max_element();
        for patch in revolution(&surface, ha, hb, phi, sweep) {
            let off = farthest(&patch, |x| quadric.distance(x));
            assert!(off <= 1e-12 * reach, "kind {kind}: {off:e} on {reach}");
        }
    }
    assert_eq!(kinds, [100; 4]);
}

#[test]
fn bad_strips_are_refused() {
    let surface = Revolution::sphere(Axis::Z, 1.0);
    let a = &surface.axis;
    let [a0, a1] = [0.0, 1.0].map(|f| a.at(surface.rho(0.0), 0.0, f));
    let [b0, b1] = [0.0, 1.0].map(|f| a.at(surface.rho(0.5), 0.5, f));
    let bottom = Conic3::arc_between(a.on_axis(0.0), 1.0, a0, a1).unwrap();
    let top = Conic3::arc_between(a.on_axis(0.5), surface.rho(0.5), b0, b1).unwrap();
    let left = surface.meridian(a0, b0, 0.0, 0.5, 0.0);
    let right = surface.meridian(a1, b1, 0.0, 0.5, 1.0);
    let strip = |bottom: &Conic3, left: &Conic3, right: &Conic3, axis: DVec3| {
        revolution_strip(bottom, &top, left, right, DVec3::ZERO, axis)
    };
    assert!(strip(&bottom, &left, &right, DVec3::Z).is_ok());
    // Edges that don't meet.
    assert_eq!(
        strip(&bottom, &right, &left, DVec3::Z),
        Err(PatchError::Mismatch)
    );
    // No axis, or `a1` on it.
    assert_eq!(
        strip(&bottom, &left, &right, DVec3::ZERO),
        Err(PatchError::Degenerate)
    );
    assert_eq!(
        strip(&bottom, &left, &right, a1),
        Err(PatchError::Degenerate)
    );
    // A straight bottom.
    let line = Conic3::line(a0, a1).unwrap();
    assert_eq!(
        cone_strip(&line, &top, DVec3::Z * 9.0),
        Err(PatchError::Degenerate)
    );
}

/// A solid of revolution of `stations` (radius, height pairs with the
/// heights rising, every radius above zero) about `axis`, in `pieces`
/// angular pieces: a face per segment, `surfaces[j]` between stations `j`
/// and `j + 1`, and flat caps at both ends. Strips by
/// [`revolution_strip`], or for cones by [`cone_strip`] if `cones` says
/// so.
fn solid_of_revolution(
    axis: Axis,
    stations: &[(f64, f64)],
    surfaces: &[Revolution],
    pieces: usize,
    cones: bool,
) -> Mesh {
    let mut builder = MeshBuilder::new();
    let mut pos = Vec::new();
    let mut vert = |builder: &mut MeshBuilder, p: DVec3| {
        pos.push(p);
        builder.vert(p)
    };
    let angle = |k: usize| TAU * (k % pieces) as f64 / pieces as f64;
    let rings: Vec<Vec<u32>> = stations
        .iter()
        .map(|&(rho, h)| {
            (0..pieces)
                .map(|k| vert(&mut builder, axis.at(rho, h, angle(k))))
                .collect()
        })
        .collect();
    let last = stations.len() - 1;
    let centres = [stations[0].1, stations[last].1].map(|h| vert(&mut builder, axis.on_axis(h)));
    let name = |part| FaceName::new(1, part);
    let mut cap = |part, n: DVec3, h: f64| {
        let d = n.dot(axis.on_axis(h));
        builder.face(Face {
            name: name(part),
            surface: Surface::Plane { n, d },
            form: Form::plane(n, d),
            slack: 1.0,
        })
    };
    let start = cap(FacePart::StartCap, -axis.z, stations[0].1);
    let end = cap(FacePart::EndCap, axis.z, stations[last].1);
    let parallel = |j: usize, k: usize| {
        let (rho, h) = stations[j];
        let [p, q] = [k, k + 1].map(|k| pos[rings[j][k % pieces] as usize]);
        Conic3::arc_between(axis.on_axis(h), rho, p, q).unwrap()
    };
    for (j, surface) in surfaces.iter().enumerate() {
        let face = builder.face(Face {
            name: name(FacePart::Side {
                curve: j as u64,
                segment: 0,
            }),
            surface: Surface::Quadric(surface.quadric()),
            form: surface.form(),
            slack: 1.0,
        });
        // Heights from the surface's own origin on the axis.
        let shift = (surface.axis.origin - axis.origin).dot(axis.z);
        let (ha, hb) = (stations[j].1 - shift, stations[j + 1].1 - shift);
        let meridians: Vec<Conic3> = (0..pieces)
            .map(|k| {
                let [p, q] = [j, j + 1].map(|i| pos[rings[i][k] as usize]);
                surface.meridian(p, q, ha, hb, angle(k))
            })
            .collect();
        for k in 0..pieces {
            let next = (k + 1) % pieces;
            let (bottom, top) = (parallel(j, k), parallel(j + 1, k));
            let patches = match surface.meridians {
                Meridians::Cone(h0, _) if cones => {
                    cone_strip(&bottom, &top, surface.axis.on_axis(h0)).unwrap()
                }
                _ => revolution_strip(
                    &bottom,
                    &top,
                    &meridians[k],
                    &meridians[next],
                    axis.origin,
                    axis.z,
                )
                .unwrap(),
            };
            let a = [rings[j][k], rings[j][next]];
            let b = [rings[j + 1][k], rings[j + 1][next]];
            builder.strip(a, b, &patches, face);
        }
    }
    for k in 0..pieces {
        let next = (k + 1) % pieces;
        for (i, j, face) in [(0, 0, start), (1, last, end)] {
            let arc = parallel(j, k);
            let (p, q) = (rings[j][k], rings[j][next]);
            builder.edge(p, q, arc.c, arc.w);
            let corners = if i == 0 {
                [centres[0], q, p]
            } else {
                [centres[1], p, q]
            };
            builder.tri(corners, face);
        }
    }
    builder.build().unwrap()
}

/// `mesh` turned inside out: every triangle reversed, plane forms and
/// tags turned round.
pub(crate) fn inside_out(mesh: &Mesh) -> Mesh {
    let mut builder = MeshBuilder::new();
    for &p in mesh.verts() {
        builder.vert(p);
    }
    for &face in mesh.faces() {
        let surface = match face.surface {
            Surface::Plane { n, d } => Surface::Plane { n: -n, d: -d },
            s => s,
        };
        builder.face(Face {
            surface,
            form: face.form.flipped(),
            ..face
        });
    }
    for (t, tri) in mesh.tris().iter().enumerate() {
        let [a, b, c] = tri.halfedges.map(|h| h.start);
        let patch = mesh.patch(t);
        for (i, (u, v)) in [(a, b), (b, c), (c, a)].into_iter().enumerate() {
            builder.edge(u, v, patch.c[i], patch.w[i]);
        }
        builder.tri([a, c, b], tri.face);
    }
    builder.build().unwrap()
}

/// Checks the solid of revolution: it passes `check` (orientation and
/// face tags included), has `volume` and `area` within `1e-10`
/// relative, and turned inside out is refused.
fn measure(mesh: Mesh, volume: f64, area: f64) {
    let turned = inside_out(&mesh);
    let solid = Solid::new(mesh, &TOL).unwrap();
    let (v, a) = (solid.volume(), solid.area());
    assert!(
        (v - volume).abs() <= 1e-10 * volume,
        "volume {v} for {volume}"
    );
    if area > 0.0 {
        assert!((a - area).abs() <= 1e-10 * area, "area {a} for {area}");
    }
    assert!(matches!(
        Solid::new(turned, &TOL),
        Err(KernelError::Invalid(CheckError::InsideOut(_)))
    ));
}

#[test]
fn frustums_are_solids_of_their_volume_and_area() {
    let mut rng = Rng::new(6);
    for i in 0..40 {
        let axis = if i == 0 {
            Axis::Z
        } else {
            Axis::random(&mut rng, 1e3)
        };
        let size = rng.log_range(0.1, 1e2);
        let t = rng.log_range(0.1, 10.0);
        let h0 = rng.range(0.1, 2.0) * size;
        let h1 = h0 + rng.range(0.05, 2.0) * size;
        let surface = Revolution::cone(axis, 0.0, t, true);
        let (r0, r1) = (t * h0, t * h1);
        let stations = [(r0, h0), (r1, h1)];
        let h = h1 - h0;
        let volume = PI * h * (r0 * r0 + r0 * r1 + r1 * r1) / 3.0;
        let slant = (h * h + (r1 - r0) * (r1 - r0)).sqrt();
        let area = PI * (r0 + r1) * slant + PI * (r0 * r0 + r1 * r1);
        let pieces = [4, 8, 16, 256][i % 4];
        for cones in [false, true] {
            let mesh = solid_of_revolution(axis, &stations, &[surface], pieces, cones);
            measure(mesh, volume, area);
        }
        // Narrowing upwards: the cone below its apex.
        let surface = Revolution::cone(axis, h1 + h0, t, false);
        let stations = [(r1, h0), (r0, h1)];
        let mesh = solid_of_revolution(axis, &stations, &[surface], pieces, false);
        measure(mesh, volume, area);
    }
}

#[test]
fn sphere_zones_are_solids_of_their_volume_and_area() {
    let mut rng = Rng::new(7);
    for i in 0..40 {
        let axis = if i == 0 {
            Axis::Z
        } else {
            Axis::random(&mut rng, 1e3)
        };
        let radius = rng.log_range(0.1, 1e2);
        let bands = 1 + i % 4;
        let mut heights: Vec<f64> = (0..=bands)
            .map(|_| rng.range(-0.95, 0.95) * radius)
            .collect();
        heights.sort_by(f64::total_cmp);
        if heights.windows(2).any(|w| w[1] - w[0] < 0.02 * radius) {
            continue;
        }
        let surface = Revolution::sphere(axis, radius);
        let stations: Vec<(f64, f64)> = heights.iter().map(|&h| (surface.rho(h), h)).collect();
        let (lo, hi) = (heights[0], heights[bands]);
        let cube = |h: f64| radius * radius * h - h * h * h / 3.0;
        let volume = PI * (cube(hi) - cube(lo));
        let area =
            TAU * radius * (hi - lo) + PI * (stations[0].0.powi(2) + stations[bands].0.powi(2));
        let pieces = [4, 8, 16, 32, 128][i % 5];
        let mesh = solid_of_revolution(axis, &stations, &vec![surface; bands], pieces, false);
        measure(mesh, volume, area);
    }
}

#[test]
fn lathe_profiles_are_solids_of_their_volume_and_area() {
    // A cone, a cylinder, a sphere and an ellipsoid stacked, meeting at
    // parallels, on tilted axes far out.
    let mut rng = Rng::new(8);
    for i in 0..20 {
        let axis = if i == 0 {
            Axis::Z
        } else {
            Axis::random(&mut rng, 3e5)
        };
        // Frustum r 2 → 3 over h 0..1 (apex at h = −2), cylinder r 3 over
        // 1..2, sphere R 3 about h = 2 up to r = 2 (h = 2 + √5), then an
        // ellipsoid ρ² = 4 − 4(h − h2)² up to h2 + 0.6.
        let s5 = 5f64.sqrt();
        let h2 = 2.0 + s5;
        let sphere_axis = Axis {
            origin: axis.on_axis(2.0),
            ..axis
        };
        let ellipsoid = Axis {
            origin: axis.on_axis(h2),
            ..axis
        };
        let surfaces = [
            Revolution::cone(axis, -2.0, 1.0, true),
            Revolution::cylinder(axis, 3.0),
            Revolution::sphere(sphere_axis, 3.0),
            Revolution::conic(ellipsoid, [4.0, 0.0, -4.0]),
        ];
        let top = (4.0 - 4.0 * 0.36f64).sqrt();
        let stations = [
            (2.0, 0.0),
            (3.0, 1.0),
            (3.0, 2.0),
            (2.0, h2),
            (top, h2 + 0.6),
        ];
        let volume = PI * (4.0 + 6.0 + 9.0) / 3.0
            + PI * 9.0
            + PI * (9.0 * s5 - s5 * s5 * s5 / 3.0)
            + PI * (4.0 * 0.6 - 4.0 * 0.216 / 3.0);
        let mesh = solid_of_revolution(axis, &stations, &surfaces, [4, 8, 16][i % 3], i % 2 == 1);
        measure(mesh, volume, 0.0);
    }
}

#[test]
fn shared_rulings_are_one_record_on_both_sides() {
    // Two cone strips side by side make the same ruling, to the bit, and
    // with it as one record both patches beside it are on the cone.
    let surface = Revolution::cone(Axis::Z, 0.0, 0.7, true);
    let form = surface.form();
    let (ha, hb) = (1.0, 2.5);
    let (ra, rb) = (surface.rho(ha), surface.rho(hb));
    let phis = [0.0, 0.6, 1.3];
    let a: Vec<DVec3> = phis.iter().map(|&f| Axis::Z.at(ra, ha, f)).collect();
    let b: Vec<DVec3> = phis.iter().map(|&f| Axis::Z.at(rb, hb, f)).collect();
    let strip = |k: usize| {
        let bottom = Conic3::arc_between(Axis::Z.on_axis(ha), ra, a[k], a[k + 1]).unwrap();
        let top = Conic3::arc_between(Axis::Z.on_axis(hb), rb, b[k], b[k + 1]).unwrap();
        cone_strip(&bottom, &top, DVec3::ZERO).unwrap()
    };
    let (first, second) = (strip(0), strip(1));
    // The first's right ruling (edge 1 of its first patch) is the
    // second's left one (edge 2 of its second patch, reversed).
    assert_eq!(first[0].edge(1), second[1].edge(2).reversed());
    for patch in first.iter().chain(&second) {
        let off = farthest(patch, |x| form.distance(x));
        assert!(off <= 1e-14 * hb * 2.0, "{off:e}");
    }
}

#[test]
fn strips_and_their_solids_are_the_same_on_any_thread_count() {
    let axis = Axis::random(&mut Rng::new(9), 1e3);
    let surfaces = [Revolution::sphere(axis, 5.0); 3];
    let stations: Vec<(f64, f64)> = [-4.0, -1.0, 2.0, 4.5]
        .iter()
        .map(|&h| (surfaces[0].rho(h), h))
        .collect();
    assert_deterministic(|| {
        let mesh = solid_of_revolution(axis, &stations, &surfaces, 16, false);
        let solid = Solid::new(mesh, &TOL).unwrap();
        (solid.volume(), solid.area(), solid)
    });
}
