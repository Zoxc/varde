//! Solids of revolution whose profiles have rings at turns of their
//! height: rounded edges meeting flat faces, tubes split at their top and
//! bottom, arcs tangent at a top. The rings there are parted by the
//! cylinder over them (see `mesh/hull.rs`).

use super::*;
use crate::budget::Work;
use crate::mesh::Refiner;

/// A piece of a profile in `(ρ, h)`, counter-clockwise round the region
/// it bounds with the axis.
#[derive(Debug, Clone, Copy)]
enum Seg {
    /// A line along or across the axis.
    Line(DVec2, DVec2),
    /// An arc: centre, radius, from, to (the shorter way).
    Arc(DVec2, f64, DVec2, DVec2),
}

fn v(rho: f64, h: f64) -> DVec2 {
    DVec2::new(rho, h)
}

/// The profiles, by name.
fn profiles() -> Vec<(&'static str, Vec<Seg>)> {
    use Seg::{Arc, Line};
    let tube = |c: DVec2, r: f64, from: f64| {
        let at = |k: usize| {
            let a = from + PI / 2.0 * k as f64;
            c + DVec2::new(a.cos(), a.sin()) * r
        };
        (0..4)
            .map(|k| Arc(c, r, at(k), at((k + 1) % 4)))
            .collect::<Vec<_>>()
    };
    // Its quarters from the outside, at exact points.
    let quarters = |c: DVec2, r: f64| {
        let at = [
            v(c.x + r, c.y),
            v(c.x, c.y + r),
            v(c.x - r, c.y),
            v(c.x, c.y - r),
        ];
        (0..4)
            .map(|k| Arc(c, r, at[k], at[(k + 1) % 4]))
            .collect::<Vec<_>>()
    };
    let over = {
        let (c, r) = (v(8.0, 3.0), 2.0);
        c + DVec2::new(2.3f64.cos(), 2.3f64.sin()) * r
    };
    vec![
        (
            "puck R10 r2",
            vec![
                Line(v(0.0, 0.0), v(10.0, 0.0)),
                Line(v(10.0, 0.0), v(10.0, 2.0)),
                Arc(v(8.0, 2.0), 2.0, v(10.0, 2.0), v(8.0, 4.0)),
                Line(v(8.0, 4.0), v(0.0, 4.0)),
            ],
        ),
        (
            "puck rounded top and bottom",
            vec![
                Line(v(0.0, 0.0), v(8.0, 0.0)),
                Arc(v(8.0, 2.0), 2.0, v(8.0, 0.0), v(10.0, 2.0)),
                Line(v(10.0, 2.0), v(10.0, 3.0)),
                Arc(v(8.0, 3.0), 2.0, v(10.0, 3.0), v(8.0, 5.0)),
                Line(v(8.0, 5.0), v(0.0, 5.0)),
            ],
        ),
        // Split at the outside, top, inside and bottom.
        ("torus R20 r2 at its turns", quarters(v(20.0, 0.0), 2.0)),
        ("torus R10 r3 at its turns", quarters(v(10.0, 0.0), 3.0)),
        (
            "two arcs tangent at a top",
            vec![
                Line(v(0.0, 0.0), v(10.0, 0.0)),
                Line(v(10.0, 0.0), v(10.0, 3.0)),
                Arc(v(7.0, 3.0), 3.0, v(10.0, 3.0), v(7.0, 6.0)),
                Arc(v(7.0, 4.0), 2.0, v(7.0, 6.0), v(5.0, 4.0)),
                Line(v(5.0, 4.0), v(5.0, 2.0)),
                Line(v(5.0, 2.0), v(0.0, 2.0)),
            ],
        ),
        (
            "S with a turn at the joint",
            vec![
                Line(v(0.0, 0.0), v(10.0, 0.0)),
                Line(v(10.0, 0.0), v(10.0, 3.0)),
                Arc(v(7.0, 3.0), 3.0, v(10.0, 3.0), v(7.0, 6.0)),
                Arc(v(7.0, 8.0), 2.0, v(7.0, 6.0), v(5.0, 8.0)),
                Line(v(5.0, 8.0), v(0.0, 8.0)),
            ],
        ),
        (
            "boss with a concave fillet onto a plate",
            vec![
                Line(v(0.0, -5.0), v(12.0, -5.0)),
                Line(v(12.0, -5.0), v(12.0, 0.0)),
                Line(v(12.0, 0.0), v(6.0, 0.0)),
                Arc(v(6.0, 2.0), 2.0, v(6.0, 0.0), v(4.0, 2.0)),
                Line(v(4.0, 2.0), v(4.0, 8.0)),
                Line(v(4.0, 8.0), v(0.0, 8.0)),
            ],
        ),
        (
            "lip: a half torus on a cup's wall",
            vec![
                Line(v(0.0, 0.0), v(10.0, 0.0)),
                Line(v(10.0, 0.0), v(10.0, 5.0)),
                Arc(v(9.0, 5.0), 1.0, v(10.0, 5.0), v(9.0, 6.0)),
                Arc(v(9.0, 5.0), 1.0, v(9.0, 6.0), v(8.0, 5.0)),
                Line(v(8.0, 5.0), v(8.0, 1.0)),
                Line(v(8.0, 1.0), v(0.0, 1.0)),
            ],
        ),
        (
            "thin round R50 r0.5",
            vec![
                Line(v(0.0, 0.0), v(50.0, 0.0)),
                Line(v(50.0, 0.0), v(50.0, 1.0)),
                Arc(v(49.5, 1.0), 0.5, v(50.0, 1.0), v(49.5, 1.5)),
                Line(v(49.5, 1.5), v(0.0, 1.5)),
            ],
        ),
        (
            "rounded hole edge in a ring",
            vec![
                Line(v(4.0, 0.0), v(10.0, 0.0)),
                Line(v(10.0, 0.0), v(10.0, 3.0)),
                Line(v(10.0, 3.0), v(5.0, 3.0)),
                Arc(v(5.0, 2.0), 1.0, v(5.0, 3.0), v(4.0, 2.0)),
                Line(v(4.0, 2.0), v(4.0, 0.0)),
            ],
        ),
        // Off its turns, for comparison: the bands keep rings off them.
        ("torus split off its turns", tube(v(15.0, 0.0), 2.5, 0.3)),
        (
            "round over its top in one arc",
            vec![
                Line(v(0.0, 0.0), v(10.0, 0.0)),
                Line(v(10.0, 0.0), v(10.0, 3.0)),
                Arc(v(8.0, 3.0), 2.0, v(10.0, 3.0), over),
                Line(over, v(over.x, 0.5)),
                Line(v(over.x, 0.5), v(0.0, 0.5)),
            ],
        ),
    ]
}

/// The point of `seg` at `t` in `[0, 1]`, and its derivative.
fn along(seg: Seg, t: f64) -> (DVec2, DVec2) {
    match seg {
        Seg::Line(a, b) => (a + (b - a) * t, b - a),
        Seg::Arc(c, r, a, b) => {
            let t0 = (a - c).y.atan2((a - c).x);
            let mut t1 = (b - c).y.atan2((b - c).x);
            if t1 - t0 > PI {
                t1 -= TAU;
            } else if t1 - t0 < -PI {
                t1 += TAU;
            }
            let (s, co) = (t0 + (t1 - t0) * t).sin_cos();
            (
                c + DVec2::new(co, s) * r,
                DVec2::new(-s, co) * r * (t1 - t0),
            )
        }
    }
}

/// The volume `π∮ρ² dh` and area `2π∮ρ ds` the profile sweeps, by
/// Simpson's rule (exact on lines, 20 000 steps on arcs).
fn closed_forms(segs: &[Seg]) -> (f64, f64) {
    let (mut volume, mut area) = (0.0, 0.0);
    for &seg in segs {
        let steps = match seg {
            Seg::Line(..) => 2,
            Seg::Arc(..) => 20_000,
        };
        let (mut sv, mut sa) = (0.0, 0.0);
        for i in 0..=steps {
            let weight = if i == 0 || i == steps {
                1.0
            } else if i % 2 == 1 {
                4.0
            } else {
                2.0
            };
            let (p, d) = along(seg, i as f64 / steps as f64);
            sv += weight * p.x * p.x * d.y;
            sa += weight * p.x * d.length();
        }
        volume += PI * sv / (3.0 * steps as f64);
        area += TAU * sa / (3.0 * steps as f64);
    }
    (volume, area)
}

impl Assembly<'_> {
    /// The flat annulus `piece` (across the axis at one height) on
    /// `face`: straight meridians and diagonals.
    fn annulus(&mut self, piece: &Conic3, face: u32) {
        let strips: Vec<[Patch; 2]> = (0..self.lathe.pieces())
            .map(|k| annulus_strip(self.lathe, piece, k))
            .collect();
        self.strips(piece, &strips, face);
    }
}

fn annulus_strip(lathe: &Lathe, piece: &Conic3, k: usize) -> [Patch; 2] {
    let bottom = lathe.parallel(piece.p0, k).unwrap();
    let top = lathe.parallel(piece.p1, k).unwrap();
    let (a0, a1, b0, b1) = (bottom.p0, bottom.p1, top.p0, top.p1);
    let mid = |x: DVec3, y: DVec3| (x + y) / 2.0;
    [
        Patch {
            p: [a0, a1, b1],
            c: [bottom.c, mid(a1, b1), mid(b1, a0)],
            w: [bottom.w, 1.0, 1.0],
        },
        Patch {
            p: [a0, b1, b0],
            c: [mid(a0, b1), top.c, mid(b0, a0)],
            w: [1.0, top.w, 1.0],
        },
    ]
}

/// A solid of revolution of the profile.
struct Built {
    mesh: Mesh,
    pieces: usize,
    volume: f64,
    area: f64,
    /// The smallest arc's radius.
    smallest: f64,
}

/// The profile turned about `frame`'s axis: arcs as fitted torus bands
/// (claiming no surface), lines along the axis as exact cylinder strips,
/// across it as discs or annuli (straight diagonals). The lathe starts at
/// 4 pieces and is halved until every band fits and every annulus strip
/// passes the fold check.
fn build(frame: &Frame, segs: &[Seg], tol: &Tolerance) -> Result<Built, KernelError> {
    let place = |q: DVec2| frame.at(q.x, q.y);
    let arcs: Vec<(Conic3, Form)> = segs
        .iter()
        .filter_map(|&s| match s {
            Seg::Arc(c, r, a, b) => Some((
                Conic3::arc_between(place(c), r, place(a), place(b)).unwrap(),
                Form::Torus {
                    centre: frame.origin + frame.axis * c.y,
                    axis: frame.axis,
                    major: c.x,
                    minor: r,
                },
            )),
            Seg::Line(..) => None,
        })
        .collect();
    let annuli: Vec<Conic3> = segs
        .iter()
        .filter_map(|&s| match s {
            Seg::Line(a, b) if a.y == b.y && a.x != 0.0 && b.x != 0.0 => {
                Some(Conic3::line(place(a), place(b)).unwrap())
            }
            _ => None,
        })
        .collect();
    let mut lathe = frame.lathe(4);
    let bands = 'lathe: loop {
        let mut bands = Vec::new();
        for (meridian, form) in &arcs {
            match fitted_band(&lathe, meridian, form, tol, &Budget::DEFAULT)? {
                Some(band) => bands.push(band),
                None => {
                    lathe = lathe.halved()?;
                    continue 'lathe;
                }
            }
        }
        let folds = annuli.iter().any(|piece| {
            annulus_strip(&lathe, piece, 0)
                .iter()
                .any(|p| p.fold_direction().is_none())
        });
        if folds {
            lathe = lathe.halved()?;
            continue;
        }
        break bands;
    };
    let mut assembly = Assembly::new(&lathe);
    let mut bands = bands.iter().zip(&arcs);
    for &seg in segs {
        match seg {
            Seg::Arc(..) => {
                let (band, (_, form)) = bands.next().unwrap();
                let (_, free) = assembly.face(Surface::Free, *form);
                assembly.band(band, free);
            }
            Seg::Line(a, b) if a.y == b.y => {
                // Facing up the axis where the profile runs towards it.
                let up = b.x < a.x;
                if b.x == 0.0 {
                    assembly.disc(place(a), up);
                } else if a.x == 0.0 {
                    assembly.disc(place(b), up);
                } else {
                    let n = if up { frame.axis } else { -frame.axis };
                    let d = n.dot(place(a));
                    let (face, _) = assembly.face(Surface::Plane { n, d }, Form::plane(n, d));
                    assembly.annulus(&Conic3::line(place(a), place(b)).unwrap(), face);
                }
            }
            Seg::Line(a, b) => {
                assert_eq!(a.x, b.x);
                let quadric = Quadric::cylinder(frame.origin, frame.axis, a.x).unwrap();
                let form = Form::Cylinder {
                    point: frame.origin,
                    axis: frame.axis,
                    radius: a.x,
                };
                let (face, _) = assembly.face(Surface::Quadric(quadric), form);
                let piece = Conic3::line(place(a), place(b)).unwrap();
                assembly.exact(&piece, face, |bottom, top, left, right| {
                    revolution_strip(bottom, top, left, right, frame.origin, frame.axis).unwrap()
                });
            }
        }
    }
    let (volume, area) = closed_forms(segs);
    let smallest = segs
        .iter()
        .filter_map(|&s| match s {
            Seg::Arc(_, r, ..) => Some(r),
            Seg::Line(..) => None,
        })
        .fold(f64::INFINITY, f64::min);
    Ok(Built {
        mesh: assembly.build(),
        pieces: lathe.pieces(),
        volume,
        area,
        smallest,
    })
}

/// Every profile at `fit` on `Frame::Z` and two random frames (from
/// `seed`, up to `1e3` out), measured: `check` with no repair, volume
/// within the area times half the fit tolerance, area within `4·A·fit/2`
/// over the smallest radius, refused inside out. The thin round's random
/// frames stop at `1e-4`: at `1e-5` its disc's 2 048 sectors, 50 long and
/// 0.15 wide, fail the hull rule against the wall far out (the plane
/// through a ring arc of bulge `1.2e-4` tilts by rounding), no ring at a
/// turn. Gives each profile's lathe pieces and patches on `Frame::Z`.
fn sweep(fit: f64, seed: u64) -> Vec<(usize, usize)> {
    let mut rng = Rng::new(seed);
    let frames = [
        Frame::Z,
        Frame::random(&mut rng, 1e3),
        Frame::random(&mut rng, 1e3),
    ];
    let tol = Tolerance::new(fit).unwrap();
    let mut counts = Vec::new();
    for (name, segs) in profiles() {
        for (i, frame) in frames.iter().enumerate() {
            if i > 0 && fit < 1e-4 && name.starts_with("thin") {
                continue;
            }
            let built = build(frame, &segs, &tol).unwrap();
            if i == 0 {
                counts.push((built.pieces, built.mesh.tris().len()));
            }
            let limit = fit / 2.0;
            let floor = 1e-9 * frame.scale(60.0).powi(3);
            let area = built.area;
            let (volume_error, _) = measure(
                built.mesh,
                &tol,
                built.volume,
                area * limit + floor,
                Some((area, 4.0 * area * limit / built.smallest + floor)),
            );
            assert!(volume_error <= area * limit + floor, "{name} at {fit:e}");
        }
    }
    counts
}

#[test]
fn rings_at_turns_are_solids() {
    // A tube split at its outside, top, inside and bottom: at the top and
    // bottom the torus touches the parallel's plane, both strips beside
    // the ring lie under (or over) it and no plane through it parts them,
    // but they lie either side of the cylinder over it.
    let (major, minor) = (20.0, 2.0);
    let q = quarters(&Frame::Z, major, minor, 0);
    let centre = Frame::Z.at(major, 0.0);
    let arcs: Vec<Conic3> = (0..4)
        .map(|i| Conic3::arc_between(centre, minor, q[i], q[(i + 1) % 4]).unwrap())
        .collect();
    let form = Form::Torus {
        centre: DVec3::ZERO,
        axis: DVec3::Z,
        major,
        minor,
    };
    let tol = Tolerance::new(1e-2).unwrap();
    let (lathe, bands) = fit_all(Frame::Z.lathe(4), &arcs, &form, &tol).unwrap();
    let mut assembly = Assembly::new(&lathe);
    let (_, free) = assembly.face(Surface::Free, form);
    for band in &bands {
        assembly.band(band, free);
    }
    let volume = 2.0 * PI * PI * major * minor * minor;
    let area = 4.0 * PI * PI * major * minor;
    let limit = tol.fit() / 2.0;
    let floor = 1e-9 * Frame::Z.scale(major + minor).powi(3);
    measure(
        assembly.build(),
        &tol,
        volume,
        area * limit + floor,
        Some((area, 4.0 * area * limit / minor + floor)),
    );
}

#[test]
fn profiles_with_rings_at_turns_are_solids_coarse() {
    sweep(1e-2, 83);
}

#[test]
fn profiles_with_rings_at_turns_are_solids_by_default() {
    let counts = sweep(1e-3, 84);
    // The puck's, and the torus split at its turns (as many patches as
    // one split off them; its diagonals leaving the turns' rings in their
    // planes fit in half the pieces round the axis, twice as many along).
    assert_eq!(counts[0], (16, 192));
    assert_eq!(counts[2], (64, 1024));
}

#[test]
fn profiles_with_rings_at_turns_are_solids_fine() {
    let counts = sweep(1e-4, 85);
    assert_eq!(counts[2], (128, 2048));
}

/// One to six seconds a profile in release; not run in debug builds.
#[cfg(not(debug_assertions))]
#[test]
fn profiles_with_rings_at_turns_are_solids_finest() {
    sweep(1e-5, 86);
}

/// Profiles with a turn `eps` (radians of its arc) past a ring: a tube
/// split that far past its outside, top, inside and bottom; a puck whose
/// round goes that far over its top before the flat; an S whose joint is
/// that far past the turn.
fn near_turns(eps: f64) -> Vec<(&'static str, Vec<Seg>)> {
    use Seg::{Arc, Line};
    let pt = |c: DVec2, r: f64, a: f64| c + DVec2::new(a.cos(), a.sin()) * r;
    let (c, r) = (v(20.0, 0.0), 2.0);
    let at = |k: usize| pt(c, r, eps + PI / 2.0 * k as f64);
    let over = pt(v(8.0, 2.0), 2.0, PI / 2.0 + eps);
    let c1 = v(7.0, 3.0);
    let joint = pt(c1, 3.0, PI / 2.0 + eps);
    let c2 = joint + (joint - c1).normalize() * 2.0;
    let end = v(c2.x - 2.0, c2.y);
    vec![
        (
            "tube",
            (0..4).map(|k| Arc(c, r, at(k), at((k + 1) % 4))).collect(),
        ),
        (
            "puck",
            vec![
                Line(v(0.0, 0.0), v(10.0, 0.0)),
                Line(v(10.0, 0.0), v(10.0, 2.0)),
                Arc(v(8.0, 2.0), 2.0, v(10.0, 2.0), over),
                Line(over, v(0.0, over.y)),
            ],
        ),
        (
            "S",
            vec![
                Line(v(0.0, 0.0), v(10.0, 0.0)),
                Line(v(10.0, 0.0), v(10.0, 3.0)),
                Arc(c1, 3.0, v(10.0, 3.0), joint),
                Arc(c2, 2.0, joint, end),
                Line(end, v(0.0, end.y)),
            ],
        ),
    ]
}

#[test]
fn rings_just_past_turns_are_solids() {
    // A turn this near a band piece's end is left to the ring there,
    // which the cylinder parts. Balanced instead, the piece over the turn
    // was a sliver the band halved round the axis until `TooComplex`.
    let mut rng = Rng::new(89);
    let frames = [Frame::Z, Frame::random(&mut rng, 1e3)];
    for fit in [1e-2, 1e-3] {
        let tol = Tolerance::new(fit).unwrap();
        for eps in [1e-5, 1e-4, 1e-3] {
            for (name, segs) in near_turns(eps) {
                for frame in &frames {
                    let built = build(frame, &segs, &tol)
                        .unwrap_or_else(|e| panic!("{name} {eps:e} at {fit:e}: {e:?}"));
                    let limit = fit / 2.0;
                    let floor = 1e-9 * frame.scale(60.0).powi(3);
                    let area = built.area;
                    measure(
                        built.mesh,
                        &tol,
                        built.volume,
                        area * limit + floor,
                        Some((area, 4.0 * area * limit / built.smallest + floor)),
                    );
                }
            }
        }
    }
}

#[test]
fn refined_rings_at_turns_stay_solids() {
    // Every triangle split once: the pieces at the rings keep their signs
    // against the cylinder over them, and every pair passes `check`.
    let mut rng = Rng::new(87);
    let tol = Tolerance::new(1e-2).unwrap();
    for (name, segs) in profiles() {
        let frame = Frame::random(&mut rng, 1e3);
        let built = build(&frame, &segs, &tol).unwrap();
        let mesh = built.mesh;
        let all: Vec<u32> = (0..mesh.tris().len() as u32).collect();
        let mut refiner = Refiner::new(&mesh, tol.resolution(), 0.0);
        refiner
            .split(&all, &mut Work::new(&Budget::DEFAULT))
            .unwrap();
        let refined = refiner.mesh(&refiner.pieces().unwrap());
        assert_eq!(refined.tris().len(), 4 * mesh.tris().len(), "{name}");
        let floor = 1e-9 * frame.scale(60.0).powi(3);
        let limit = tol.fit() / 2.0;
        measure(
            refined,
            &tol,
            built.volume,
            built.area * limit + floor,
            None,
        );
    }
}

#[test]
fn rings_at_turns_are_the_same_on_any_thread_count() {
    let frame = Frame::random(&mut Rng::new(88), 1e3);
    let tol = Tolerance::DEFAULT;
    for (_, segs) in profiles() {
        assert_deterministic(|| {
            let solid = Solid::new(build(&frame, &segs, &tol).unwrap().mesh, &tol).unwrap();
            (solid.volume(), solid)
        });
    }
}
