#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::collections::BTreeMap;
use std::f64::consts::{FRAC_1_SQRT_2, PI, TAU};

use glam::{DVec2, DVec3};

use super::*;
use crate::mesh::{CheckError, Face, FaceName, FacePart, Mesh, MeshBuilder, Quadric, Surface};
use crate::par::assert_deterministic;
use crate::patch::{Conic2, Conic3, Patch};
use crate::sweep::tests::inside_out;
use crate::sweep::{cone_strip, revolution_strip};
use crate::test_rng::Rng;
use crate::{Budget, KernelError, Solid, Tolerance};

/// An axis: a point on it, the unit axis, and the unit `x` across it
/// that station 0 lies along.
#[derive(Debug, Clone, Copy)]
struct Frame {
    origin: DVec3,
    axis: DVec3,
    x: DVec3,
}

impl Frame {
    const Z: Frame = Frame {
        origin: DVec3::ZERO,
        axis: DVec3::Z,
        x: DVec3::X,
    };

    fn random(rng: &mut Rng, extent: f64) -> Frame {
        let axis = rng.direction();
        Frame {
            origin: rng.point(extent),
            axis,
            x: axis.any_orthonormal_vector(),
        }
    }

    /// The point `rho` across the axis (along `x`) and `h` along it.
    fn at(&self, rho: f64, h: f64) -> DVec3 {
        self.origin + self.x * rho + self.axis * h
    }

    fn lathe(&self, pieces: usize) -> Lathe {
        Lathe::new(self.origin, self.axis, None, pieces).unwrap()
    }

    /// Where the tests' bounds are relative to: the size plus the
    /// coordinates.
    fn scale(&self, size: f64) -> f64 {
        size + self.origin.abs().max_element()
    }
}

/// Builds a closed solid of revolution on a lathe: rings found by the
/// bits of their point at station 0, so faces built apart share them (a
/// point on the axis is one vertex at every station).
struct Assembly<'a> {
    lathe: &'a Lathe,
    builder: MeshBuilder,
    rings: BTreeMap<[u64; 3], Vec<u32>>,
    faces: u64,
}

impl<'a> Assembly<'a> {
    fn new(lathe: &'a Lathe) -> Self {
        Assembly {
            lathe,
            builder: MeshBuilder::new(),
            rings: BTreeMap::new(),
            faces: 0,
        }
    }

    fn ring(&mut self, p: DVec3) -> Vec<u32> {
        let key = p.to_array().map(f64::to_bits);
        if let Some(ring) = self.rings.get(&key) {
            return ring.clone();
        }
        let ring: Vec<u32> = if self.lathe.on_axis(p) {
            vec![self.builder.vert(p); self.lathe.stations()]
        } else {
            self.lathe
                .ring(p)
                .into_iter()
                .map(|q| self.builder.vert(q))
                .collect()
        };
        self.rings.insert(key, ring.clone());
        ring
    }

    /// A face of its own name, and a copy claiming no surface.
    fn face(&mut self, surface: Surface, form: Form) -> (u32, u32) {
        self.faces += 1;
        let name = FaceName::new(
            1,
            FacePart::Side {
                curve: self.faces,
                segment: 0,
            },
        );
        let face = self.builder.face(Face {
            name,
            surface,
            form,
        });
        let free = self.builder.face(Face {
            name,
            surface: Surface::Free,
            form,
        });
        (face, free)
    }

    fn strips(&mut self, piece: &Conic3, strips: &[[Patch; 2]], face: u32) {
        let (a, b) = (self.ring(piece.p0), self.ring(piece.p1));
        assert_eq!(strips.len(), self.lathe.pieces());
        for (k, patches) in strips.iter().enumerate() {
            // A full turn's last strip ends at station 0.
            let next = (k + 1) % a.len();
            self.builder
                .strip([a[k], a[next]], [b[k], b[next]], patches, face);
        }
    }

    /// Strips made exactly on each station by `make(bottom, top, left,
    /// right)`.
    fn exact(
        &mut self,
        piece: &Conic3,
        face: u32,
        make: impl Fn(&Conic3, &Conic3, &Conic3, &Conic3) -> [Patch; 2],
    ) {
        let lathe = self.lathe;
        let strips: Vec<[Patch; 2]> = (0..lathe.pieces())
            .map(|k| {
                make(
                    &lathe.parallel(piece.p0, k).unwrap(),
                    &lathe.parallel(piece.p1, k).unwrap(),
                    &lathe.meridian(piece, k).unwrap(),
                    &lathe.meridian(piece, k + 1).unwrap(),
                )
            })
            .collect();
        self.strips(piece, &strips, face);
    }

    fn band(&mut self, band: &Band, face: u32) {
        let n = self.lathe.pieces();
        for (j, piece) in band.pieces.iter().enumerate() {
            self.strips(piece, &band.strips[j * n..(j + 1) * n], face);
        }
    }

    fn cap(&mut self, cap: &Cap, pole: Pole, face: u32) {
        let (tip, rim) = match pole {
            Pole::Start => (cap.meridian.p0, cap.meridian.p1),
            Pole::End => (cap.meridian.p1, cap.meridian.p0),
        };
        let tip = self.ring(tip)[0];
        let rim = self.ring(rim);
        let n = rim.len();
        for (k, patch) in cap.patches.iter().enumerate() {
            let next = (k + 1) % n;
            let corners = match pole {
                Pole::Start => [tip, rim[next], rim[k]],
                Pole::End => [rim[k], rim[next], tip],
            };
            for i in 0..3 {
                self.builder
                    .edge(corners[i], corners[(i + 1) % 3], patch.c[i], patch.w[i]);
            }
            self.builder.tri(corners, face);
        }
    }

    /// The flat disc in `rim`'s parallel, its normal along the axis if
    /// `up`.
    fn disc(&mut self, rim: DVec3, up: bool) {
        assert!(self.lathe.is_full());
        let axis = self.lathe.axis();
        let n = if up { axis } else { -axis };
        let d = n.dot(rim);
        let face = self.builder.face(Face {
            name: FaceName::new(
                1,
                if up {
                    FacePart::EndCap
                } else {
                    FacePart::StartCap
                },
            ),
            surface: Surface::Plane { n, d },
            form: Form::plane(n, d),
        });
        let v = rim - self.lathe.origin();
        let centre = self.builder.vert(self.lathe.origin() + axis * v.dot(axis));
        let ring = self.ring(rim);
        for k in 0..ring.len() {
            let next = (k + 1) % ring.len();
            let arc = self.lathe.parallel(rim, k).unwrap();
            self.builder.edge(ring[k], ring[next], arc.c, arc.w);
            let corners = if up {
                [centre, ring[k], ring[next]]
            } else {
                [centre, ring[next], ring[k]]
            };
            self.builder.tri(corners, face);
        }
    }

    /// The flat end of a part turn at `station` (0 or the last), facing
    /// back at station 0 and on at the last: a fan from `centre` (at
    /// station 0) over `chain`, the meridian's pieces in order round it,
    /// counter-clockwise in `(ρ, h)`.
    fn end(&mut self, station: usize, centre: DVec3, chain: &[Conic3]) {
        let lathe = self.lathe;
        assert!(!lathe.is_full());
        let back = station == 0;
        // The end's plane holds the axis and the station's half-plane.
        let off = chain
            .iter()
            .map(|c| c.p1)
            .find(|&p| !lathe.on_axis(p))
            .unwrap();
        let point = lathe.turned(off, station);
        let foot = lathe.origin() + lathe.axis() * (point - lathe.origin()).dot(lathe.axis());
        let mut n = lathe.axis().cross(point - foot).normalize();
        if back {
            n = -n;
        }
        let d = n.dot(lathe.origin());
        let face = self.builder.face(Face {
            name: FaceName::new(
                1,
                if back {
                    FacePart::StartCap
                } else {
                    FacePart::EndCap
                },
            ),
            surface: Surface::Plane { n, d },
            form: Form::plane(n, d),
        });
        let centre = if lathe.on_axis(centre) {
            self.ring(centre)[0]
        } else {
            self.builder.vert(lathe.turned(centre, station))
        };
        for piece in chain {
            let (a, b) = (self.ring(piece.p0)[station], self.ring(piece.p1)[station]);
            let curve = lathe.meridian(piece, station).unwrap();
            self.builder.edge(a, b, curve.c, curve.w);
            let corners = if back { [centre, a, b] } else { [centre, b, a] };
            self.builder.tri(corners, face);
        }
    }

    fn build(self) -> Mesh {
        self.builder.build().unwrap()
    }
}

/// Checks a solid of revolution: it passes `check` (orientation and face
/// tags included) at `tol`, its volume is within `volume_slack` of
/// `volume` and its area within `area_slack` of `area` (if given), and
/// turned inside out it is refused. Gives the volume's and area's errors.
fn measure(
    mesh: Mesh,
    tol: &Tolerance,
    volume: f64,
    volume_slack: f64,
    area: Option<(f64, f64)>,
) -> (f64, f64) {
    let turned = inside_out(&mesh);
    let solid = Solid::new(mesh, tol).unwrap();
    let (v, a) = (solid.volume(), solid.area());
    assert!(
        (v - volume).abs() <= volume_slack,
        "volume {v} for {volume} (slack {volume_slack:e})"
    );
    let mut area_error = 0.0;
    if let Some((area, slack)) = area {
        area_error = (a - area).abs();
        assert!(area_error <= slack, "area {a} for {area} (slack {slack:e})");
    }
    assert!(matches!(
        Solid::new(turned, tol),
        Err(KernelError::Invalid(CheckError::InsideOut(_)))
    ));
    ((v - volume).abs(), area_error)
}

/// The largest distance from `form` on a barycentric grid of `n` steps a
/// side: a lower bound on a patch's true error, and dense enough to
/// trust [`deviation`] against.
fn dense(patch: &Patch, form: &Form, n: usize) -> f64 {
    let mut worst: f64 = 0.0;
    for i in 0..=n {
        for j in 0..=n - i {
            let u = DVec3::new(i as f64, j as f64, (n - i - j) as f64) / n as f64;
            worst = worst.max(form.distance(patch.eval(u)));
        }
    }
    worst
}

/// The four quarter points of the circle of `minor` about `major` out
/// along `x`, in the half-plane at station 0, counter-clockwise (going up
/// on the outside), from the one `start` eighths of a turn round from
/// the outside: exact cosines and sines, so at 45° the points opposite
/// across the axis's direction are at one height to the bit.
fn quarters(frame: &Frame, major: f64, minor: f64, start: usize) -> [DVec3; 4] {
    let r = FRAC_1_SQRT_2;
    let eighths = [
        (1.0, 0.0),
        (r, r),
        (0.0, 1.0),
        (-r, r),
        (-1.0, 0.0),
        (-r, -r),
        (0.0, -1.0),
        (r, -r),
    ];
    [0, 2, 4, 6].map(|i| {
        let (c, s) = eighths[(i + start) % 8];
        frame.at(major + minor * c, minor * s)
    })
}

/// A torus fitted at `tol`: its tube as four quarter arcs, the lathe
/// halved until every band fits. Gives the mesh, the lathe and the
/// bands.
fn torus(
    frame: &Frame,
    major: f64,
    minor: f64,
    tol: &Tolerance,
) -> Result<(Mesh, Lathe, Vec<Band>), KernelError> {
    let centre = frame.at(major, 0.0);
    // Off the top and bottom of the tube: no ring may sit where the
    // surface touches its parallel's plane.
    let q = quarters(frame, major, minor, 1);
    let arcs: Vec<Conic3> = (0..4)
        .map(|i| Conic3::arc_between(centre, minor, q[i], q[(i + 1) % 4]).unwrap())
        .collect();
    let form = Form::Torus {
        centre: frame.origin,
        axis: frame.axis,
        major,
        minor,
    };
    let (lathe, bands) = fit_all(frame.lathe(4), &arcs, &form, tol)?;
    let mut assembly = Assembly::new(&lathe);
    let (_, free) = assembly.face(Surface::Free, form);
    for band in &bands {
        assembly.band(band, free);
    }
    Ok((assembly.build(), lathe, bands))
}

/// Every meridian's band on `lathe`, halved round the axis until all fit.
fn fit_all(
    mut lathe: Lathe,
    meridians: &[Conic3],
    form: &Form,
    tol: &Tolerance,
) -> Result<(Lathe, Vec<Band>), KernelError> {
    'lathe: loop {
        let mut bands = Vec::new();
        for meridian in meridians {
            match fitted_band(&lathe, meridian, form, tol, &Budget::DEFAULT)? {
                Some(band) => bands.push(band),
                None => {
                    lathe = lathe.halved()?;
                    continue 'lathe;
                }
            }
        }
        return Ok((lathe, bands));
    }
}

fn patches(bands: &[Band]) -> usize {
    bands.iter().map(|b| 2 * b.strips.len()).sum()
}

#[test]
fn stations_are_exact_at_quarter_turns() {
    let frame = Frame::random(&mut Rng::new(20), 1e3);
    let lathe = frame.lathe(8);
    let p = frame.at(3.0, 1.5);
    assert_eq!(lathe.stations(), 8);
    assert_eq!(lathe.turned(p, 0), p);
    assert_eq!(lathe.turned(p, 8), p);
    // A quarter turn takes `x` to `axis × x`, to a rounding.
    let quarter = lathe.turned(p, 2);
    let expected = frame.at(0.0, 1.5) + frame.axis.cross(frame.x) * 3.0;
    assert!((quarter - expected).length() < 1e-12 * frame.scale(3.0));
    for k in 0..8 {
        let q = lathe.turned(p, k);
        let angle = TAU * k as f64 / 8.0;
        let expected = frame.at(0.0, 1.5)
            + (frame.x * angle.cos() + frame.axis.cross(frame.x) * angle.sin()) * 3.0;
        assert!((q - expected).length() < 1e-12 * frame.scale(3.0), "{k}");
        // The parallel is the exact arc between the stations.
        let arc = lathe.parallel(p, k).unwrap();
        assert_eq!((arc.p0, arc.p1), (q, lathe.turned(p, (k + 1) % 8)));
    }
    // A point on the axis stays put, and a meridian keeps its weight.
    let pole = frame.at(0.0, 4.0);
    assert!(lathe.on_axis(pole));
    assert!(!lathe.on_axis(p));
    let meridian = Conic3::arc_between(frame.at(0.0, 1.5), 2.5, frame.at(2.5, 1.5), pole).unwrap();
    let turned = lathe.meridian(&meridian, 3).unwrap();
    assert_eq!(turned.w, meridian.w);
    assert_eq!(turned.p0, lathe.turned(meridian.p0, 3));
    // A part turn: its last station at the sweep.
    let part = Lathe::new(frame.origin, frame.axis, Some(PI), 2).unwrap();
    assert_eq!(part.stations(), 3);
    let back = part.turned(p, 2);
    assert!((back - (frame.at(-3.0, 1.5))).length() < 1e-12 * frame.scale(3.0));
    assert_eq!(part.halved().unwrap().pieces(), 4);
    // Quarter stations exact whatever the pieces: `TAU · 11 / 44` rounds
    // off a quarter turn, and so does a part turn's `270° / 3`.
    let p = DVec3::new(3.0, 0.0, 1.5);
    let lathe = Frame::Z.lathe(44);
    let quarters = [11, 22, 33].map(|k| lathe.turned(p, k));
    assert_eq!(
        quarters,
        [
            DVec3::new(0.0, 3.0, 1.5),
            DVec3::new(-3.0, 0.0, 1.5),
            DVec3::new(0.0, -3.0, 1.5)
        ]
    );
    let part = Lathe::new(DVec3::ZERO, DVec3::Z, Some(1.5 * PI), 3).unwrap();
    let quarters = [1, 2, 3].map(|k| part.turned(p, k));
    assert_eq!(quarters, [11, 22, 33].map(|k| lathe.turned(p, k)));
    // A part turn's last station is never station 0.
    let nearly = Lathe::new(DVec3::ZERO, DVec3::Z, Some(TAU * (1.0 - 1e-16)), 4).unwrap();
    assert_ne!(nearly.turned(p, 4), p);
}

#[test]
fn bad_lathes_are_refused() {
    let new = |axis: DVec3, sweep: Option<f64>, pieces: usize| {
        Lathe::new(DVec3::ZERO, axis, sweep, pieces)
    };
    for (axis, sweep, pieces) in [
        (DVec3::ZERO, None, 4),
        (DVec3::NAN, None, 4),
        (DVec3::Z, None, 0),
        (DVec3::Z, None, 3),
        (DVec3::Z, Some(0.0), 4),
        (DVec3::Z, Some(TAU), 4),
        (DVec3::Z, Some(-1.0), 4),
        (DVec3::Z, Some(f64::NAN), 4),
        (DVec3::Z, Some(PI), 1),
        (DVec3::Z, None, Lathe::MAX_PIECES * 2),
    ] {
        assert!(
            matches!(new(axis, sweep, pieces), Err(PatchError::Parameter(_))),
            "{axis} {sweep:?} {pieces}"
        );
    }
    assert!(Lathe::new(DVec3::splat(1e300), DVec3::Z, None, 4).is_err());
    let most = new(DVec3::Z, None, Lathe::MAX_PIECES).unwrap();
    assert!(most.halved().is_err());
}

#[test]
fn torus_strips_are_measured_at_their_farthest() {
    // `deviation` climbs to the maxima: never under a far denser grid,
    // and over it by no more than the grid's spacing allows.
    let mut rng = Rng::new(21);
    for i in 0..40 {
        let frame = if i == 0 {
            Frame::Z
        } else {
            Frame::random(&mut rng, 1e3)
        };
        let major = rng.log_range(1.0, 100.0);
        let minor = major * rng.range(0.05, 0.6);
        let form = Form::Torus {
            centre: frame.origin,
            axis: frame.axis,
            major,
            minor,
        };
        let lathe = frame.lathe([16, 32, 64, 128][i % 4]);
        let centre = frame.at(major, 0.0);
        let q = quarters(&frame, major, minor, i % 2);
        let arc = Conic3::arc_between(centre, minor, q[i % 4], q[(i + 1) % 4]).unwrap();
        let quarter = arc.split_half().unwrap()[i % 2];
        let piece = quarter.split_half().unwrap()[(i / 2) % 2];
        let k = i % lathe.pieces();
        let (strip, _) = lathe.strip(&piece, k, &form, 1e-9).unwrap().unwrap();
        for patch in &strip {
            let measured = deviation(patch, &form);
            let grid = dense(patch, &form, 300);
            let floor = 1e-15 * frame.scale(major);
            assert!(measured >= grid - floor, "{measured:e} under {grid:e}");
            assert!(
                measured <= grid * 1.001 + floor,
                "{measured:e} over {grid:e}"
            );
        }
    }
}

#[test]
fn fitted_strips_find_exact_ones_where_they_exist() {
    // On a sphere and a cone an exact diagonal exists: the fit finds one
    // as close as the exact strips are.
    let mut rng = Rng::new(22);
    for i in 0..40 {
        let frame = Frame::random(&mut rng, if i % 2 == 0 { 0.0 } else { 1e3 });
        let radius = rng.log_range(0.1, 1e2);
        let lathe = frame.lathe([4, 8, 16, 64][i % 4]);
        let (ha, hb) = (rng.range(-0.9, 0.0), rng.range(0.1, 0.9));
        let ends = [ha, hb].map(|h| frame.at(radius * (1.0 - h * h).sqrt(), radius * h));
        let meridian = Conic3::arc_between(frame.origin, radius, ends[0], ends[1]).unwrap();
        let sphere = Form::Sphere {
            centre: frame.origin,
            radius,
        };
        let error = lathe.strip_error(&meridian, 0, &sphere, 1e-12).unwrap();
        assert!(error <= 1e-12 * frame.scale(radius), "{error:e}");
        // A cone through the apex on the axis, its meridian straight.
        let apex = frame.at(0.0, -radius);
        let meridian = Conic3::line(frame.at(radius, 0.0), frame.at(2.0 * radius, radius)).unwrap();
        let cone = Form::Cone {
            apex,
            axis: frame.axis,
            cos: FRAC_1_SQRT_2,
            sin: FRAC_1_SQRT_2,
        };
        // A cone has a diagonal on it in every plane through `a0` and
        // `b1`: the fit lands on one, which may fold for wide pieces.
        let meridian = crate::sweep::cone_ruling(meridian.p0, meridian.p1, apex).unwrap();
        let fitted = fitted_strip(
            &lathe.parallel(meridian.p0, 0).unwrap(),
            &lathe.parallel(meridian.p1, 0).unwrap(),
            &lathe.meridian(&meridian, 0).unwrap(),
            &lathe.meridian(&meridian, 1).unwrap(),
            &cone,
        )
        .unwrap();
        assert!(
            fitted.error <= 1e-12 * frame.scale(radius),
            "{:e}",
            fitted.error
        );
    }
}

#[test]
fn bad_fitted_strips_are_refused() {
    let lathe = Frame::Z.lathe(8);
    let q = quarters(&Frame::Z, 5.0, 1.0, 0);
    let arc = Conic3::arc_between(DVec3::X * 5.0, 1.0, q[0], q[1]).unwrap();
    let [bottom, top, left, right] = [
        lathe.parallel(arc.p0, 0).unwrap(),
        lathe.parallel(arc.p1, 0).unwrap(),
        lathe.meridian(&arc, 0).unwrap(),
        lathe.meridian(&arc, 1).unwrap(),
    ];
    let torus = Form::Torus {
        centre: DVec3::ZERO,
        axis: DVec3::Z,
        major: 5.0,
        minor: 1.0,
    };
    assert!(fitted_strip(&bottom, &top, &left, &right, &torus).is_ok());
    assert_eq!(
        fitted_strip(&bottom, &top, &right, &left, &torus),
        Err(PatchError::Mismatch)
    );
    assert_eq!(
        fitted_strip(&bottom, &top, &left, &right, &Form::Unknown),
        Err(PatchError::Degenerate)
    );
}

/// The cap triangle at station 0 of the piece of `meridian` from its
/// pole at the start to its parameter `size`, and its error.
fn cap_error(lathe: &Lathe, meridian: &Conic3, size: f64, form: &Form) -> f64 {
    let cap = if size == 1.0 {
        *meridian
    } else {
        meridian.piece(0.0, size).unwrap()
    };
    let patch = cap_triangle(lathe, &cap, Pole::Start, 0).unwrap();
    assert!(patch.fold_direction().is_some());
    deviation(&patch, form)
}

#[test]
fn pole_errors_fall_with_the_cap() {
    // A sphere's: about R·δ²·φ²/64, a quarter each halving of the cap; a
    // cone's in proportion to the cap's length.
    let mut rng = Rng::new(23);
    for i in 0..12 {
        let frame = Frame::random(&mut rng, 1e3);
        let radius = rng.log_range(1.0, 100.0);
        let pieces = [4, 8, 16, 32][i % 4];
        let lathe = frame.lathe(pieces);
        let pole = frame.at(0.0, radius);
        // From the pole down to 45° from it.
        let rim = frame.at(radius * FRAC_1_SQRT_2, radius * FRAC_1_SQRT_2);
        let meridian = Conic3::arc_between(frame.origin, radius, pole, rim).unwrap();
        let sphere = Form::Sphere {
            centre: frame.origin,
            radius,
        };
        let phi = TAU / pieces as f64;
        let mut last = f64::INFINITY;
        for halving in 0..8 {
            let size = 0.5f64.powi(halving);
            let error = cap_error(&lathe, &meridian, size, &sphere);
            let h = meridian.blossom(size, size);
            let chord = (h.truncate() / h.w - pole).length();
            let delta = 2.0 * (chord / (2.0 * radius)).asin();
            let estimate = radius * delta * delta * phi * phi / 64.0;
            if error > 1e-9 * frame.scale(radius) {
                assert!(
                    (error / estimate - 1.0).abs() < 0.25,
                    "{error:e} for {estimate:e}"
                );
            }
            if halving > 0 && last > 1e-8 * frame.scale(radius) {
                let ratio = last / error;
                assert!((3.6..4.4).contains(&ratio), "{ratio}");
            }
            last = error;
        }
        // A cone of half-angle 30° to 60°, its apex on the axis.
        let apex = frame.at(0.0, radius);
        let slope = rng.range(0.6, 1.7);
        let rim = frame.at(radius * slope, 0.0);
        let line = Conic3::line(apex, rim).unwrap();
        let length = (1.0 + slope * slope).sqrt();
        let cone = Form::Cone {
            apex,
            axis: -frame.axis,
            cos: 1.0 / length,
            sin: slope / length,
        };
        let mut last = f64::INFINITY;
        for halving in 0..10 {
            let error = cap_error(&lathe, &line, 0.5f64.powi(halving), &cone);
            if halving > 0 && last > 1e-8 * frame.scale(radius) {
                let ratio = last / error;
                assert!((1.9..2.1).contains(&ratio), "{ratio}");
            }
            last = error;
        }
    }
}

/// A sphere of `radius` about the frame's origin at `tol`: capped poles,
/// exact strips between them.
fn sphere(frame: &Frame, radius: f64, pieces: usize, tol: &Tolerance) -> (Mesh, [Cap; 2]) {
    let lathe = frame.lathe(pieces);
    let [south, equator, north] =
        [-radius, 0.0, radius].map(|h| frame.at(if h == 0.0 { radius } else { 0.0 }, h));
    let lower = Conic3::arc_between(frame.origin, radius, south, equator).unwrap();
    let upper = Conic3::arc_between(frame.origin, radius, equator, north).unwrap();
    let form = Form::Sphere {
        centre: frame.origin,
        radius,
    };
    let budget = Budget::DEFAULT;
    let caps = [
        pole_cap(&lathe, &lower, Pole::Start, &form, tol, &budget).unwrap(),
        pole_cap(&lathe, &upper, Pole::End, &form, tol, &budget).unwrap(),
    ];
    let mut assembly = Assembly::new(&lathe);
    let (face, free) = assembly.face(
        Surface::Quadric(Quadric::sphere(frame.origin, radius)),
        form,
    );
    let rest: Vec<Conic3> = caps.iter().flat_map(|c| c.rest.clone()).collect();
    for piece in &rest {
        assembly.exact(piece, face, |b, t, l, r| {
            revolution_strip(b, t, l, r, frame.origin, frame.axis).unwrap()
        });
    }
    assembly.cap(&caps[0], Pole::Start, free);
    assembly.cap(&caps[1], Pole::End, free);
    (assembly.build(), caps)
}

#[test]
fn spheres_with_capped_poles_are_solids_of_their_volume_and_area() {
    let mut rng = Rng::new(24);
    for i in 0..24 {
        let frame = if i == 0 {
            Frame::Z
        } else {
            Frame::random(&mut rng, 1e3)
        };
        let radius = rng.log_range(0.5, 100.0);
        let tol = Tolerance::new([1e-2, 1e-3, 1e-4][i % 3]).unwrap();
        let pieces = [4, 8, 16, 32][i % 4];
        let (mesh, caps) = sphere(&frame, radius, pieces, &tol);
        let limit = tol.fit() / 2.0;
        for cap in &caps {
            assert!(cap.error <= limit, "{:e}", cap.error);
            for patch in &cap.patches {
                assert!(dense(patch, &mesh_form(&mesh), 60) <= limit);
            }
            // The rest grows from the rim by at most 16 times a piece.
            assert!(cap.rest.len() <= 1 + 40 / 4);
        }
        let area = 4.0 * PI * radius * radius;
        // The caps are within fit/2 of the sphere over their area; the
        // rest is exact.
        let slack = area * limit + 1e-9 * radius.powi(3);
        let volume = 4.0 / 3.0 * PI * radius.powi(3);
        let area_slack = 4.0 * area * limit / radius + 1e-9 * area;
        measure(mesh, &tol, volume, slack, Some((area, area_slack)));
    }
}

/// The form of the mesh's first face, which every test solid's curved
/// face is.
fn mesh_form(mesh: &Mesh) -> Form {
    mesh.faces()
        .iter()
        .find(|f| !matches!(f.form, Form::Plane { .. }))
        .unwrap()
        .form
}

#[test]
fn cones_with_capped_apexes_are_solids_of_their_volume_and_area() {
    let mut rng = Rng::new(25);
    for i in 0..24 {
        let frame = if i == 0 {
            Frame::Z
        } else {
            Frame::random(&mut rng, 1e3)
        };
        let height = rng.log_range(0.5, 100.0);
        let rho = height * rng.range(0.2, 3.0);
        let tol = Tolerance::new([1e-2, 1e-3, 1e-4][i % 3]).unwrap();
        let lathe = frame.lathe([4, 8, 16, 32][i % 4]);
        let apex = frame.at(0.0, height);
        let base = frame.at(rho, 0.0);
        let slant = (rho * rho + height * height).sqrt();
        let form = Form::Cone {
            apex,
            axis: -frame.axis,
            cos: height / slant,
            sin: rho / slant,
        };
        let line = Conic3::line(base, apex).unwrap();
        let cap = pole_cap(&lathe, &line, Pole::End, &form, &tol, &Budget::DEFAULT).unwrap();
        let limit = tol.fit() / 2.0;
        assert!(cap.error <= limit);
        for patch in &cap.patches {
            assert!(dense(patch, &form, 60) <= limit);
        }
        let mut assembly = Assembly::new(&lathe);
        let quadric = Quadric::cone(apex, -frame.axis, height / slant, rho / slant).unwrap();
        let (face, free) = assembly.face(Surface::Quadric(quadric), form);
        for piece in &cap.rest {
            assembly.exact(piece, face, |b, t, _, _| cone_strip(b, t, apex).unwrap());
        }
        assembly.cap(&cap, Pole::End, free);
        assembly.disc(base, false);
        let volume = PI * rho * rho * height / 3.0;
        let area = PI * rho * slant + PI * rho * rho;
        let cap_length = (cap.meridian.p1 - cap.meridian.p0).length();
        let cap_area = PI * cap_length * cap_length;
        let slack = cap_area * limit + 1e-9 * frame.scale(slant).powi(3);
        let area_slack = cap_area + 1e-9 * frame.scale(slant).powi(2);
        measure(
            assembly.build(),
            &tol,
            volume,
            slack,
            Some((area, area_slack)),
        );
    }
}

#[test]
fn caps_are_refused_off_the_axis_and_past_their_budget() {
    let lathe = Frame::Z.lathe(4);
    let form = Form::Sphere {
        centre: DVec3::ZERO,
        radius: 1.0,
    };
    let arc = Conic3::arc_between(DVec3::ZERO, 1.0, DVec3::X, DVec3::Z).unwrap();
    let tol = Tolerance::DEFAULT;
    assert_eq!(
        pole_cap(&lathe, &arc, Pole::Start, &form, &tol, &Budget::DEFAULT),
        Err(KernelError::Patch(PatchError::Parameter(0.0)))
    );
    assert!(pole_cap(&lathe, &arc, Pole::End, &form, &tol, &Budget::DEFAULT).is_ok());
    // A band can't reach the pole: that is the cap's.
    assert!(matches!(
        fitted_band(&lathe, &arc, &form, &tol, &Budget::DEFAULT),
        Err(KernelError::Patch(_))
    ));
    assert_eq!(
        pole_cap(&lathe, &arc, Pole::End, &form, &tol, &Budget::new(100)),
        Err(KernelError::TooComplex)
    );
    let q = quarters(&Frame::Z, 5.0, 1.0, 0);
    let arc = Conic3::arc_between(DVec3::X * 5.0, 1.0, q[0], q[1]).unwrap();
    let torus = Form::Torus {
        centre: DVec3::ZERO,
        axis: DVec3::Z,
        major: 5.0,
        minor: 1.0,
    };
    assert_eq!(
        fitted_band(&lathe, &arc, &torus, &tol, &Budget::new(1000)),
        Err(KernelError::TooComplex)
    );
}

#[test]
fn tori_are_fitted_within_half_the_fit_tolerance() {
    // The measured torus (`R = 20`, `r = 2`) at three fit tolerances,
    // with the lathe and the meridian's pieces the halving settles on.
    // Against the measured best fits (32 × 16 strips, 1 024 patches,
    // 3.6e-4 off; 64 × 32, 4 096, 3.6e-5): about 1 000 patches at the
    // default tolerance and 4 000 at 0.1 µm.
    let expected = [
        // fit, pieces round the axis, pieces along the tube, patches
        (1e-2, 32, 4, 256),
        (1e-3, 64, 6, 768),
        (1e-4, 128, 10, 2560),
    ];
    let (major, minor) = (20.0, 2.0);
    let mut found = Vec::new();
    for (fit, ..) in expected {
        let tol = Tolerance::new(fit).unwrap();
        let (mesh, lathe, bands) = torus(&Frame::Z, major, minor, &tol).unwrap();
        let along: usize = bands.iter().map(|b| b.pieces.len()).sum();
        let error = bands.iter().map(|b| b.error).fold(0.0, f64::max);
        assert!(error <= fit / 2.0, "{error:e} at {fit:e}");
        found.push((fit, lathe.pieces(), along, patches(&bands)));
        eprintln!(
            "fit {fit:e}: {} x {along} strips, {} patches, error {error:.2e}",
            lathe.pieces(),
            patches(&bands)
        );
        let form = mesh_form(&mesh);
        // Spot checks against a dense grid, a strip of each piece.
        for band in &bands {
            for strip in band.strips.iter().step_by(lathe.pieces()) {
                for patch in strip {
                    assert!(dense(patch, &form, 80) <= fit / 2.0);
                }
            }
        }
        let volume = 2.0 * PI * PI * major * minor * minor;
        let area = 4.0 * PI * PI * major * minor;
        let (dv, da) = measure(
            mesh,
            &tol,
            volume,
            area * fit / 2.0,
            Some((area, 4.0 * area * fit / 2.0 / minor)),
        );
        eprintln!("  volume off {dv:.2e}, area off {da:.2e}");
    }
    assert_eq!(found, expected);
}

#[test]
fn tori_anywhere_are_solids_of_their_volume_and_area() {
    let mut rng = Rng::new(26);
    for i in 0..6 {
        let frame = Frame::random(&mut rng, 1e3);
        let major = rng.log_range(1.0, 50.0);
        let minor = major * rng.range(0.1, 0.7);
        let tol = Tolerance::new([1e-2, 1e-3][i % 2]).unwrap();
        let (mesh, _, bands) = torus(&frame, major, minor, &tol).unwrap();
        let error = bands.iter().map(|b| b.error).fold(0.0, f64::max);
        assert!(error <= tol.fit() / 2.0);
        let volume = 2.0 * PI * PI * major * minor * minor;
        let area = 4.0 * PI * PI * major * minor;
        let limit = tol.fit() / 2.0;
        let floor = 1e-9 * frame.scale(major + minor).powi(3);
        measure(
            mesh,
            &tol,
            volume,
            area * limit + floor,
            Some((area, 4.0 * area * limit / minor + floor)),
        );
    }
}

#[test]
fn elliptic_tori_are_fitted_to_their_meridian() {
    // An ellipse (semi-axes `a` across, `b` along the axis) about the axis
    // at `R`: a revolved conic, fitted; its volume by Pappus, its area by
    // a fine quadrature of `2π·ρ·ds` along the ellipse.
    let mut rng = Rng::new(27);
    for i in 0..6 {
        let frame = if i == 0 {
            Frame::Z
        } else {
            Frame::random(&mut rng, 1e3)
        };
        let big = rng.log_range(2.0, 50.0);
        let (a, b) = (big * rng.range(0.1, 0.6), big * rng.range(0.1, 0.6));
        let tol = Tolerance::new([1e-2, 1e-3][i % 2]).unwrap();
        // Quarters from 45°, off the top and bottom: the control point of
        // the quarter from `α` is the unit circle's at `α + 45°`, `√2` out.
        let r = FRAC_1_SQRT_2;
        let unit = [(r, r), (-r, r), (-r, -r), (r, -r)];
        let control = [
            (0.0, 2.0 * r),
            (-2.0 * r, 0.0),
            (0.0, -2.0 * r),
            (2.0 * r, 0.0),
        ];
        let point = |(c, s): (f64, f64)| DVec2::new(big + a * c, b * s);
        let place = |v: DVec2| frame.at(v.x, v.y);
        let meridians: Vec<Conic3> = (0..4)
            .map(|i| {
                Conic3::new(
                    place(point(unit[i])),
                    place(point(control[i])),
                    FRAC_1_SQRT_2,
                    place(point(unit[(i + 1) % 4])),
                )
                .unwrap()
            })
            .collect();
        let form = Form::Revolved {
            origin: frame.origin,
            axis: frame.axis,
            meridian: Conic2::new(
                point(unit[0]),
                point(control[0]),
                FRAC_1_SQRT_2,
                point(unit[1]),
            )
            .unwrap(),
        };
        let (lathe, bands) = fit_all(frame.lathe(4), &meridians, &form, &tol).unwrap();
        let mut assembly = Assembly::new(&lathe);
        let (_, free) = assembly.face(Surface::Free, form);
        for band in &bands {
            assert!(band.error <= tol.fit() / 2.0);
            assembly.band(band, free);
        }
        let volume = 2.0 * PI * big * PI * a * b;
        let steps = 20_000;
        let area: f64 = (0..steps)
            .map(|k| {
                let t = TAU * (k as f64 + 0.5) / steps as f64;
                let ds = (a * a * t.sin().powi(2) + b * b * t.cos().powi(2)).sqrt();
                TAU * (big + a * t.cos()) * ds * TAU / steps as f64
            })
            .sum();
        let limit = tol.fit() / 2.0;
        let floor = 1e-9 * frame.scale(big + a).powi(3);
        measure(
            assembly.build(),
            &tol,
            volume,
            area * limit + floor,
            Some((
                area,
                4.0 * area * limit / a.min(b) * (a.max(b) / a.min(b)) + floor,
            )),
        );
    }
}

#[test]
fn fitted_solids_are_the_same_on_any_thread_count() {
    let frame = Frame::random(&mut Rng::new(28), 1e3);
    assert_deterministic(|| {
        let tol = Tolerance::DEFAULT;
        let (torus, ..) = torus(&frame, 10.0, 3.0, &tol).unwrap();
        let (sphere, _) = sphere(&frame, 5.0, 16, &tol);
        let [torus, sphere] = [torus, sphere].map(|mesh| Solid::new(mesh, &tol).unwrap());
        (torus.volume(), sphere.volume(), torus, sphere)
    });
}

#[test]
fn tori_split_anywhere_off_their_turns_are_solids() {
    // A tube in arcs from any angle: the arcs over the top and bottom are
    // cut where they reach their other end's height, so the strip over
    // each turn ends at one height and every ring passes the hull rule.
    let mut rng = Rng::new(29);
    for i in 0..8 {
        let frame = if i == 0 {
            Frame::Z
        } else {
            Frame::random(&mut rng, 1e3)
        };
        let (major, minor) = (rng.log_range(2.0, 40.0), 0.0);
        let minor = minor + major * rng.range(0.1, 0.5);
        let start = rng.range(0.05, 1.5);
        let angles: Vec<f64> = (0..=4).map(|k| start + PI / 2.0 * k as f64).collect();
        let points: Vec<DVec3> = angles[..4]
            .iter()
            .map(|a| frame.at(major + minor * a.cos(), minor * a.sin()))
            .collect();
        let centre = frame.at(major, 0.0);
        let arcs: Vec<Conic3> = (0..4)
            .map(|k| Conic3::arc_between(centre, minor, points[k], points[(k + 1) % 4]).unwrap())
            .collect();
        let form = Form::Torus {
            centre: frame.origin,
            axis: frame.axis,
            major,
            minor,
        };
        let tol = Tolerance::new(1e-2).unwrap();
        let (lathe, bands) = fit_all(frame.lathe(4), &arcs, &form, &tol).unwrap();
        // The pieces over a turn end at one height.
        let height = |p: DVec3| (p - frame.origin).dot(frame.axis);
        let over: Vec<&Conic3> = bands
            .iter()
            .flat_map(|b| &b.pieces)
            .filter(|p| !lathe.turns(p).is_empty())
            .collect();
        assert_eq!(over.len(), 2);
        for piece in over {
            assert!((height(piece.p0) - height(piece.p1)).abs() <= tol.resolution() / 4.0);
        }
        let mut assembly = Assembly::new(&lathe);
        let (_, free) = assembly.face(Surface::Free, form);
        for band in &bands {
            assembly.band(band, free);
        }
        let volume = 2.0 * PI * PI * major * minor * minor;
        let area = 4.0 * PI * PI * major * minor;
        let limit = tol.fit() / 2.0;
        let floor = 1e-9 * frame.scale(major + minor).powi(3);
        measure(
            assembly.build(),
            &tol,
            volume,
            area * limit + floor,
            Some((area, 4.0 * area * limit / minor + floor)),
        );
    }
}

/// `π·∫ρ² dh` along a meridian in `(ρ, h)`, by the trapezoid rule on
/// `ρ²` over 100 000 steps of its parameter: the volume it sweeps, signed.
fn swept_volume(meridian: &Conic2) -> f64 {
    let at = |t: f64| {
        let h = meridian.blossom(t, t);
        DVec2::new(h.x, h.y) / h.z
    };
    let steps = 100_000;
    let mut last = at(0.0);
    let mut volume = 0.0;
    for k in 1..=steps {
        let p = at(k as f64 / steps as f64);
        volume += PI * (p.x * p.x + p.x * last.x + last.x * last.x) / 3.0 * (p.y - last.y);
        last = p;
    }
    volume
}

/// A solid of revolution of `meridians` (in `(ρ, h)`, end to end, each
/// its own face of the revolved conic), from a pole to a pole: capped
/// at both, fitted bands between, the lathe halved until every band
/// fits. Gives the mesh, the lathe and the caps.
fn revolved(
    frame: &Frame,
    meridians: &[Conic2],
    tol: &Tolerance,
) -> Result<(Mesh, Lathe, [Cap; 2]), KernelError> {
    let place = |v: DVec2| frame.at(v.x, v.y);
    let curves: Vec<Conic3> = meridians
        .iter()
        .map(|m| Conic3::new(place(m.p0), place(m.c), m.w, place(m.p1)))
        .collect::<Result<_, _>>()?;
    let forms: Vec<Form> = meridians
        .iter()
        .map(|&meridian| Form::Revolved {
            origin: frame.origin,
            axis: frame.axis,
            meridian,
        })
        .collect();
    let last = curves.len() - 1;
    let mut lathe = frame.lathe(4);
    'lathe: loop {
        let budget = Budget::DEFAULT;
        let caps = [
            pole_cap(&lathe, &curves[0], Pole::Start, &forms[0], tol, &budget)?,
            pole_cap(&lathe, &curves[last], Pole::End, &forms[last], tol, &budget)?,
        ];
        let mut assembly = Assembly::new(&lathe);
        for (i, (curve, form)) in curves.iter().zip(&forms).enumerate() {
            let (_, free) = assembly.face(Surface::Free, *form);
            let mut pieces = vec![*curve];
            if i == 0 {
                assembly.cap(&caps[0], Pole::Start, free);
                pieces = caps[0].rest.clone();
            }
            if i == last {
                // Both caps on one meridian would need it split.
                assert!(last > 0);
                assembly.cap(&caps[1], Pole::End, free);
                pieces = caps[1].rest.clone();
            }
            for piece in &pieces {
                match fitted_band(&lathe, piece, form, tol, &budget)? {
                    Some(band) => assembly.band(&band, free),
                    None => {
                        lathe = lathe.halved()?;
                        continue 'lathe;
                    }
                }
            }
        }
        return Ok((assembly.build(), lathe, caps));
    }
}

/// The meridian of an apple: the ellipse about `(c, 0)` of semi-axes `a`
/// across the axis and `b` along it (`c < a`), from where it crosses the
/// axis below round the outside to where it crosses above, in three
/// conics. Its height turns at its bottom and top, between its poles and
/// its widest point.
fn apple(c: f64, a: f64, b: f64) -> [Conic2; 3] {
    let r = FRAC_1_SQRT_2;
    let x = -c / a;
    let y = (1.0 - x * x).sqrt();
    let unit = [
        DVec2::new(x, -y),
        DVec2::new(r, -r),
        DVec2::new(r, r),
        DVec2::new(x, y),
    ];
    let map = |p: DVec2| DVec2::new(c + a * p.x, b * p.y);
    [0, 1, 2].map(|k| {
        let arc = Conic2::arc_between(DVec2::ZERO, 1.0, unit[k], unit[k + 1]).unwrap();
        Conic2::new(map(arc.p0), map(arc.c), arc.w, map(arc.p1)).unwrap()
    })
}

#[test]
fn caps_stop_short_of_a_turn() {
    // An apple's meridian dips below its poles before it widens: its
    // height turns between the pole and the widest point. A cap over the
    // turn lies on both sides of its rim's plane, with the strips beyond
    // it on one, and was refused (`EdgeNeighbours`) at coarse
    // tolerances; caps now stop half way to the turn and the bands
    // balance the rest over it.
    let mut rng = Rng::new(30);
    for i in 0..8 {
        let frame = if i == 0 {
            Frame::Z
        } else {
            Frame::random(&mut rng, 1e3)
        };
        let (c, a, b) = [
            (0.5, 1.0, 1.0),
            (0.9, 1.0, 2.0),
            (0.99, 1.0, 1.0),
            (0.2, 3.0, 0.5),
        ][i % 4];
        let size = rng.log_range(0.5, 20.0);
        let meridians =
            apple(c, a, b).map(|m| Conic2::new(m.p0 * size, m.c * size, m.w, m.p1 * size).unwrap());
        let tol = Tolerance::new([1e-1, 1e-2][i % 2]).unwrap();
        let (mesh, lathe, caps) = revolved(&frame, &meridians, &tol).unwrap();
        for cap in &caps {
            assert!(lathe.turns(&cap.meridian).is_empty());
            assert!(cap.error <= tol.fit() / 2.0);
        }
        let volume: f64 = meridians.iter().map(swept_volume).sum();
        let solid = Solid::new(mesh.clone(), &tol).unwrap();
        let floor = 1e-9 * frame.scale(size).powi(3);
        measure(
            mesh,
            &tol,
            volume,
            solid.area() * tol.fit() / 2.0 + floor,
            None,
        );
    }
}

#[test]
fn caps_whose_rim_reaches_the_axis_are_too_complex() {
    // A sphere's cap 4.5e-5 across at `1e3` out, at a resolution of
    // `1e-4`: no pair of its triangles passes the hull rule, and halving
    // brought the rim to the axis, whose parallel is no arc (a NaN
    // control point). Now it stops there.
    let frame = Frame::random(&mut Rng::new(77), 1e3);
    let radius = 0.01;
    let z = -0.99999 * radius;
    let lathe = frame.lathe(4);
    let south = frame.at(0.0, -radius);
    let rim = frame.at((radius * radius - z * z).sqrt(), z);
    let meridian = Conic3::arc_between(frame.origin, radius, south, rim).unwrap();
    let form = Form::Sphere {
        centre: frame.origin,
        radius,
    };
    let tol = Tolerance::new(1e-1).unwrap();
    assert_eq!(
        pole_cap(
            &lathe,
            &meridian,
            Pole::Start,
            &form,
            &tol,
            &Budget::DEFAULT
        ),
        Err(KernelError::TooComplex)
    );
    // A point on the axis has no parallel.
    assert_eq!(
        Frame::Z.lathe(4).parallel(DVec3::Z, 0),
        Err(PatchError::Parameter(0.0))
    );
}

#[test]
fn part_turns_are_closed_by_flat_ends() {
    // Tori and spheres over part turns, closed by flat ends through the
    // axis: the fitted or exact surfaces meet the flat ends along curved
    // edges, the spheres' caps and both ends meeting at the poles. Their
    // volumes by Pappus, and each refused turned inside out.
    let mut rng = Rng::new(32);
    for i in 0..6 {
        let frame = if i == 0 {
            Frame::Z
        } else {
            Frame::random(&mut rng, 1e3)
        };
        let sweep = [0.3, PI / 2.0, 2.5, PI, 270f64.to_radians(), 6.0][i];
        let pieces = (sweep / FRAC_PI_2).ceil() as usize;
        let tol = Tolerance::new([1e-2, 1e-3][i % 2]).unwrap();
        let floor = 1e-9 * frame.scale(50.0).powi(3);
        // A tube in quarters from 45°, the lathe halved until it fits.
        let (major, minor) = (rng.log_range(2.0, 40.0), 0.0);
        let minor = minor + major * rng.range(0.1, 0.5);
        let centre = frame.at(major, 0.0);
        let q = quarters(&frame, major, minor, 1);
        let arcs: Vec<Conic3> = (0..4)
            .map(|k| Conic3::arc_between(centre, minor, q[k], q[(k + 1) % 4]).unwrap())
            .collect();
        let form = Form::Torus {
            centre: frame.origin,
            axis: frame.axis,
            major,
            minor,
        };
        let part = Lathe::new(frame.origin, frame.axis, Some(sweep), pieces).unwrap();
        let (lathe, bands) = fit_all(part, &arcs, &form, &tol).unwrap();
        let mut assembly = Assembly::new(&lathe);
        let (_, free) = assembly.face(Surface::Free, form);
        for band in &bands {
            assembly.band(band, free);
        }
        let chain: Vec<Conic3> = bands.iter().flat_map(|b| b.pieces.clone()).collect();
        assembly.end(0, centre, &chain);
        assembly.end(lathe.pieces(), centre, &chain);
        let mesh = assembly.build();
        let area = Solid::new(mesh.clone(), &tol).unwrap().area();
        let volume = sweep * major * PI * minor * minor;
        measure(mesh, &tol, volume, area * tol.fit() / 2.0 + floor, None);
        // An orange's wedge: both poles capped, exact strips between,
        // the two ends meeting along the axis.
        let radius = rng.log_range(0.5, 50.0);
        let lathe = Lathe::new(frame.origin, frame.axis, Some(sweep), 2 * pieces).unwrap();
        let [south, equator, north] =
            [-radius, 0.0, radius].map(|h| frame.at(if h == 0.0 { radius } else { 0.0 }, h));
        let form = Form::Sphere {
            centre: frame.origin,
            radius,
        };
        let budget = Budget::DEFAULT;
        let caps = [
            Conic3::arc_between(frame.origin, radius, south, equator)
                .map(|m| pole_cap(&lathe, &m, Pole::Start, &form, &tol, &budget)),
            Conic3::arc_between(frame.origin, radius, equator, north)
                .map(|m| pole_cap(&lathe, &m, Pole::End, &form, &tol, &budget)),
        ]
        .map(|cap| cap.unwrap().unwrap());
        let mut assembly = Assembly::new(&lathe);
        let (face, free) = assembly.face(
            Surface::Quadric(Quadric::sphere(frame.origin, radius)),
            form,
        );
        let mut chain = vec![caps[0].meridian];
        chain.extend(caps.iter().flat_map(|c| c.rest.clone()));
        chain.push(caps[1].meridian);
        for piece in &chain[1..chain.len() - 1] {
            assembly.exact(piece, face, |b, t, l, r| {
                revolution_strip(b, t, l, r, frame.origin, frame.axis).unwrap()
            });
        }
        assembly.cap(&caps[0], Pole::Start, free);
        assembly.cap(&caps[1], Pole::End, free);
        assembly.end(0, frame.origin, &chain);
        assembly.end(lathe.pieces(), frame.origin, &chain);
        // The coarse caps' hulls cross the ends' near the poles: refined
        // until they don't, exactly.
        let mesh = assembly.build().repair(&tol, &budget).unwrap();
        let area = Solid::new(mesh.clone(), &tol).unwrap().area();
        let volume = 2.0 / 3.0 * sweep * radius.powi(3);
        measure(mesh, &tol, volume, area * tol.fit() / 2.0 + floor, None);
    }
}

#[test]
fn caps_and_part_turns_are_the_same_on_any_thread_count() {
    let frame = Frame::random(&mut Rng::new(33), 1e3);
    let tol = Tolerance::new(1e-2).unwrap();
    assert_deterministic(|| {
        let (mesh, ..) = revolved(&frame, &apple(0.9, 1.0, 2.0), &tol).unwrap();
        let solid = Solid::new(mesh, &tol).unwrap();
        (solid.volume(), solid)
    });
}

mod turns;
