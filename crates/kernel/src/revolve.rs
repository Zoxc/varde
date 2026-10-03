//! Revolving a profile into a solid.
//!
//! [`revolve`] turns a [`Profile`], drawn on a [`Frame`] whose `y` is the
//! axis and whose `x` points towards the profile, about the axis, all the
//! way round or from one angle to another. Every profile segment turns
//! into a face of its own: a segment along the axis into none, one square
//! to it into a flat ring, disc or sector, other lines into cones (and
//! cylinders), arcs centred on the axis into spheres, all exact, and
//! everything else (arcs off the axis: tori; other conics) into fitted
//! faces within half the fit tolerance. Where a face meets the axis it is
//! closed by a fitted cap round the pole. A part turn is closed by its two
//! ends, the profile's region triangulated as an extrude's caps.
//!
//! One angular split serves the whole solid: the turn in `4·2^k` equal
//! pieces (a part turn in `⌈θ/90°⌉·2^k`), `k` growing until every fitted
//! band fits and every flat face's caps take the ring arcs as they are.
//! Faces and ends share their edges, so the solid is closed by
//! construction; repair then splits whatever breaks the fold or hull
//! rules, faces of one surface are named as one, and the result passes
//! [`Mesh::check`](crate::mesh::Mesh::check). The rules are written down
//! in `agents/kernel.md`.

use std::collections::BTreeMap;
use std::f64::consts::{FRAC_PI_2, TAU};

use glam::{DVec2, DVec3};

use crate::budget::{Budget, Work};
use crate::extrude::Frame;
use crate::extrude::cap::{self, Mode, Rounds};
use crate::extrude::chain::Chain;
use crate::mesh::{BuildError, Face, FaceName, FacePart, Form, MeshBuilder, Surface};
use crate::patch::{Conic2, Conic3, Patch, PatchError};
use crate::profile::evidence::{AXIS_WORK, Gather, nearest_axis};
use crate::profile::{Loop, Profile, ProfileError, Segment};
use crate::sweep::{Cap, Lathe, Pole, fitted_band_with, pole_cap_with, revolution_strip};
use crate::{Failure, KernelError, MAX_PATCHES, Solid, Tolerance, in_range, trig};

mod kind;

use kind::Kind;

/// How far a revolve turns: all the way round, or from the angle `from`
/// to `to` (radians, turning the frame's `x` towards `x × y`, `0` at `x`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sweep {
    Full,
    Part { from: f64, to: f64 },
}

/// The farthest a part turn may start or end from `x`, either way: keeps
/// the angles' cosines and sines accurate.
const MAX_ANGLE: f64 = 4.0 * TAU;

/// The most rounds of halving the turn's pieces or the profile's for the
/// ends.
const MAX_ROUNDS: usize = 64;

/// Work units per vertex of a region checked for nesting, as the caps
/// count a triangulation.
const NESTING_WORK: usize = 8;

/// The solid `profile`, drawn on `frame` (`y` along the axis, `x` towards
/// the profile, whose points have `x ≥ 0`), turned about the axis through
/// `sweep`, as the revolve feature `feature` names its faces: the face of
/// the `n`-th segment (in profile order) of curve `c` is
/// [`FacePart::Side`]` { curve: c, segment: n }`, and a part turn's ends
/// are [`FacePart::StartCap`] (at `from`, facing back) and
/// [`FacePart::EndCap`]. Faces are tagged with their planes, cones and
/// spheres where they are exact (their caps round the axis on copies
/// claiming none), and fitted faces claim no surface; every face has its
/// [`Form`].
///
/// The profile must pass [`Profile::check`], and its segments must
/// neither touch nor cross within `tol`'s resolution, nest as outer loops
/// and holes and meet at no cusp, as an extrude's. Vertices within the
/// resolution of the axis are put on it (a decision by distance, as is
/// a segment coming that close inside); a region reaching across the
/// axis is [`ProfileError::CrossesAxis`], one touching it at a point
/// [`ProfileError::TouchesAxis`] (a vertex in a full turn, the inside of
/// a segment coming within the resolution of it in any), and a part turn
/// whose ends come within the resolution of each other
/// [`ProfileError::NearlyFullTurn`]. Segments along the axis make no
/// face. A part turn's angles are finite, within `8π` of `x`
/// and `0 < to − from < 2π` ([`PatchError::Parameter`] otherwise).
///
/// A solid too thin or too fine for the resolution fails with
/// [`KernelError::Invalid`] or [`ProfileError::TooFine`], running out of
/// `budget` or past a limit with [`KernelError::TooComplex`]; it never
/// gives an invalid solid. A profile's error comes with the segments and
/// points it is about, placed on `frame`, and their sketch curves, as
/// [`Failure::evidence`]; the axis's errors with the axis or the turn's
/// ends too.
pub fn revolve(
    profile: &Profile,
    frame: &Frame,
    sweep: Sweep,
    feature: u64,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Solid, Failure> {
    revolved(profile, frame, sweep, feature, tol, budget)
        .map_err(|error| revolve_failure(error, profile, frame, sweep, tol))
}

/// `error`, revolving `profile` on `frame` through `sweep`, with its
/// evidence: a profile's as an extrude's (see
/// [`Gather`]), placed at the frame's
/// own angle, and for the axis's errors the axis too: a segment crossing
/// it with the axis across the profile's extent along it, a segment
/// touching it inside with the point nearest it (a vertex on it as a
/// cusp is given), a turn nearly full with the profile at both its
/// ends.
fn revolve_failure(
    error: KernelError,
    profile: &Profile,
    frame: &Frame,
    sweep: Sweep,
    tol: &Tolerance,
) -> Failure {
    let KernelError::Profile(e) = error else {
        return error.into();
    };
    let mut gather = Gather::new(profile, frame);
    match e {
        ProfileError::CrossesAxis(l, s) => {
            gather.segment(l, s);
            if let Some((lo, hi)) = gather.extent_along_y() {
                gather.line(DVec2::new(0.0, lo), DVec2::new(0.0, hi));
            }
        }
        // Inside the segment: both its ends are off the axis by more than
        // the resolution (within it they are put on it).
        ProfileError::TouchesAxis(l, s) => match gather.get(l, s) {
            Some(seg) if seg.conic.p0.x.abs() > tol.resolution() => {
                if gather.afford(AXIS_WORK) {
                    gather.points([nearest_axis(&seg.conic)]);
                }
                gather.segment(l, s);
            }
            _ => gather.vertex(l, s),
        },
        ProfileError::NearlyFullTurn => {
            let ends = gather.frame().and_then(|_| Turn::new(frame, sweep).ok());
            match ends.and_then(|turn| turn.ends()) {
                Some([start, end]) => {
                    gather.all_on(Some(&start));
                    gather.all_on(Some(&end));
                }
                None => gather.all(),
            }
        }
        e => gather.error(e),
    }
    gather.failure(error)
}

/// [`revolve`], failing with the error alone.
fn revolved(
    profile: &Profile,
    frame: &Frame,
    sweep: Sweep,
    feature: u64,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Solid, KernelError> {
    profile.check().map_err(KernelError::Profile)?;
    frame.check()?;
    let turn = Turn::new(frame, sweep)?;
    let margin = tol.resolution();
    let profile = onto_axis(profile, margin);
    profile.check().map_err(KernelError::Profile)?;
    axis_rules(&profile, turn.is_full(), margin)?;
    let mut work = Work::new(budget);
    let mut chain = Chain::new(&profile, margin)?;
    chain.separate(&mut work)?;
    // The loops must nest as outer loops and holes: the caps' winding
    // rule, which a full turn would otherwise never run.
    let (segs, starts) = chain.flat();
    work.spend(segs.len().saturating_mul(NESTING_WORK))?;
    cap::nests(&segs, &starts)?;
    turn.apart(&segs, margin)?;
    let kinds: Vec<Kind> = chain
        .sides
        .iter()
        .map(|side| Kind::of(side, &turn, margin))
        .collect();
    let build = Build {
        chain: &chain,
        kinds: &kinds,
        turn: &turn,
        feature,
        tol,
    };
    // As an extrude's: caps with slivers along short segments meeting
    // nearly straight fail the hull rules, and Steiner points moved in
    // from those corners mend most, so they are the second try, made
    // only where the first found such a corner.
    let mut flat = false;
    match build.solid(false, &mut flat, &mut work) {
        Err(
            first @ (KernelError::Invalid(_)
            | KernelError::TooComplex
            | KernelError::Profile(ProfileError::TooFine(..))),
        ) if flat && work.left() > 0 => build.solid(true, &mut false, &mut work).map_err(|_| first),
        result => result,
    }
}

/// The frame a revolve turns on: unit and square, with `x` turned to
/// where the sweep starts.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Turn {
    origin: DVec3,
    /// Where station 0 lies, from the axis.
    x: DVec3,
    /// The axis.
    y: DVec3,
    /// `x × y`: the way the stations turn.
    n: DVec3,
    /// The angle of a part turn; `None` for a full one.
    sweep: Option<f64>,
}

impl Turn {
    fn new(frame: &Frame, sweep: Sweep) -> Result<Turn, KernelError> {
        // Made square to the last bit: forms and tags are measured
        // against the profile's own lengths.
        let y = frame.y.normalize();
        let x = (frame.x - y * frame.x.dot(y)).normalize();
        let (from, sweep) = match sweep {
            Sweep::Full => (0.0, None),
            Sweep::Part { from, to } => {
                for a in [from, to] {
                    if a.is_nan() || a.abs() > MAX_ANGLE {
                        return Err(PatchError::Parameter(a).into());
                    }
                }
                let s = to - from;
                if !(s > 0.0 && s < TAU) {
                    return Err(PatchError::Parameter(s).into());
                }
                (from, Some(s))
            }
        };
        let start = unit_turn(from);
        let x = x * start.x + x.cross(y) * start.y;
        Ok(Turn {
            origin: frame.origin,
            x,
            y,
            n: x.cross(y),
            sweep,
        })
    }

    fn is_full(&self) -> bool {
        self.sweep.is_none()
    }

    /// The frames a part turn's start and end lie on: `x` turned to each,
    /// `y` the axis. `None` for a full turn.
    fn ends(&self) -> Option<[Frame; 2]> {
        let sweep = self.sweep?;
        let at = |x: DVec3| Frame {
            origin: self.origin,
            x,
            y: self.y,
        };
        let turned = unit_turn(sweep);
        Some([at(self.x), at(self.x * turned.x + self.n * turned.y)])
    }

    /// The profile's point `p` (`x` from the axis, `y` along it) at
    /// station 0.
    pub(crate) fn place(&self, p: DVec2) -> DVec3 {
        self.origin + self.x * p.x + self.y * p.y
    }

    fn place_conic(&self, c: &Conic2) -> Result<Conic3, PatchError> {
        Conic3::new(self.place(c.p0), self.place(c.c), c.w, self.place(c.p1))
    }

    /// The axis.
    pub(crate) fn axis(&self) -> DVec3 {
        self.y
    }

    /// A point on the axis.
    pub(crate) fn origin(&self) -> DVec3 {
        self.origin
    }

    /// The lathe in `pieces`: its axis `−y`, so its stations turn `x`
    /// towards `x × y`.
    fn lathe(&self, pieces: usize) -> Result<Lathe, PatchError> {
        Lathe::new(self.origin, -self.y, self.sweep, pieces)
    }

    /// The fewest pieces of at most a quarter turn.
    fn first_pieces(&self) -> usize {
        match self.sweep {
            None => 4,
            // A sweep of a whole number of quarters, give or take its
            // rounding, in that many.
            Some(s) => ((s / FRAC_PI_2) * (1.0 - 1e-12)).ceil().max(1.0) as usize,
        }
    }

    /// Refuses a part turn whose two ends come within `margin` of each
    /// other at the profile's farthest point from the axis (its control
    /// points', which hold it): no solid has them that close.
    fn apart(&self, segs: &[crate::extrude::chain::Seg], margin: f64) -> Result<(), ProfileError> {
        let Some(sweep) = self.sweep else {
            return Ok(());
        };
        let far = segs
            .iter()
            .flat_map(|s| [s.conic.p0.x, s.conic.c.x, s.conic.p1.x])
            .fold(0.0, f64::max);
        let gap = 2.0 * far * trig::sin(0.5 * (TAU - sweep));
        if gap > margin {
            Ok(())
        } else {
            Err(ProfileError::NearlyFullTurn)
        }
    }

    /// The unit normal of the plane of station `k` of `lathe`, the way
    /// the stations turn.
    fn normal_at(&self, lathe: &Lathe, k: usize) -> DVec3 {
        (lathe.turned(self.origin + self.n, k) - self.origin).normalize()
    }
}

/// The cosine and sine of `angle`, exact at whole quarter turns (within a
/// few roundings of one: an angle given as 90° is meant to be one).
fn unit_turn(angle: f64) -> DVec2 {
    let q = angle / FRAC_PI_2;
    let whole = q.round();
    if (q - whole).abs() <= 8.0 * f64::EPSILON * whole.abs().max(1.0) {
        match (whole as i64).rem_euclid(4) {
            0 => DVec2::new(1.0, 0.0),
            1 => DVec2::new(0.0, 1.0),
            2 => DVec2::new(-1.0, 0.0),
            _ => DVec2::new(0.0, -1.0),
        }
    } else {
        trig::unit(angle)
    }
}

/// `profile` with every segment end within `margin` of the axis put on
/// it, and the control point of a segment with both ends there too if it
/// is that close: a decision by distance, with the axis rule on
/// segments coming that close inside (see [`axis_rules`]). Ends
/// shared to the bit stay shared.
fn onto_axis(profile: &Profile, margin: f64) -> Profile {
    let snap = |p: DVec2| {
        if p.x.abs() <= margin {
            DVec2::new(0.0, p.y)
        } else {
            p
        }
    };
    let loops = profile
        .loops
        .iter()
        .map(|lp| Loop {
            segments: lp
                .segments
                .iter()
                .map(|seg| {
                    let c = seg.conic;
                    let (p0, p1) = (snap(c.p0), snap(c.p1));
                    let mut ctrl = c.c;
                    if p0.x == 0.0 && p1.x == 0.0 && ctrl.x.abs() <= margin {
                        ctrl.x = 0.0;
                    }
                    Segment {
                        conic: Conic2 {
                            p0,
                            c: ctrl,
                            w: c.w,
                            p1,
                        },
                        curve: seg.curve,
                    }
                })
                .collect(),
        })
        .collect();
    Profile { loops }
}

/// The axis rules on a profile already put onto the axis: no segment
/// reaching across it ([`ProfileError::CrossesAxis`]), none with both ends
/// off it coming within `margin` of it inside, and in a full turn no
/// vertex on it alone, without a segment along the axis on either side
/// ([`ProfileError::TouchesAxis`]).
///
/// A segment's distance from the axis is `x(t) = N(t)/D(t)` with `D > 0`
/// and `N` the quadratic of Bernstein coefficients `x0`, `w·cx`, `x1`, so
/// exact signs decide: it reaches below 0 where an end does or where the
/// middle coefficient is negative and its square beats `x0·x1`. It comes
/// within `margin` where `N − margin·D`, of coefficients `x0 − margin`,
/// `w·(cx − margin)`, `x1 − margin`, reaches 0 that way: the one place the
/// rules take a distance, as the ends put onto the axis do (a segment
/// touching the axis inside comes within rounding of it, never exactly
/// onto it, and its face would pinch there, far too thin to pass).
fn axis_rules(profile: &Profile, full: bool, margin: f64) -> Result<(), ProfileError> {
    let along = |c: &Conic2| c.p0.x == 0.0 && c.c.x == 0.0 && c.p1.x == 0.0;
    // Whether `N` of coefficients `b0, b1, b2`, its ends above 0, reaches
    // 0 inside.
    let dips = |b0: f64, b1: f64, b2: f64| b1 < 0.0 && b1 * b1 >= b0 * b2;
    let segments = || {
        profile.loops.iter().enumerate().flat_map(|(l, lp)| {
            lp.segments
                .iter()
                .enumerate()
                .map(move |(s, seg)| (l, s, &seg.conic))
        })
    };
    // Crossing first: a segment across the axis also leaves a vertex on
    // it alone.
    for (l, s, c) in segments() {
        let (b0, b1, b2) = (c.p0.x, c.w * c.c.x, c.p1.x);
        if b0 < 0.0 || b2 < 0.0 || (b1 < 0.0 && b1 * b1 > b0 * b2) {
            return Err(ProfileError::CrossesAxis(l, s));
        }
    }
    for (l, s, c) in segments() {
        // Ends off the axis are beyond `margin`: within it they were put
        // on it.
        let off = c.p0.x > 0.0 && c.p1.x > 0.0;
        if off && dips(c.p0.x - margin, c.w * (c.c.x - margin), c.p1.x - margin) {
            return Err(ProfileError::TouchesAxis(l, s));
        }
        let lp = &profile.loops[l].segments;
        let before = &lp[(s + lp.len() - 1) % lp.len()].conic;
        if full && c.p0.x == 0.0 && !along(c) && !along(before) {
            return Err(ProfileError::TouchesAxis(l, s));
        }
    }
    Ok(())
}

/// A piece of the profile at station 0, in the profile's direction, and
/// the input segment (the chain's side) it is part of.
#[derive(Debug, Clone, Copy)]
struct Piece {
    side: u32,
    curve: Conic3,
}

/// A piece's face: its meridian's pieces and the strips and caps made of
/// them. Meridians run the profile's pieces backwards: the lathe turns
/// about `−y`, so a profile counter-clockwise in `(x, y)` is
/// counter-clockwise in the lathe's `(ρ, h)` run backwards, and its
/// strips then face out.
#[derive(Debug, Clone)]
struct Wall {
    side: u32,
    /// The meridian's pieces at station 0, end to end in the meridian's
    /// direction: the edges a part turn's ends share.
    meridians: Vec<Conic3>,
    rows: Vec<Row>,
    caps: Vec<(Cap, Pole)>,
    /// A flat face, made from its rings once the walls are.
    flat: bool,
}

/// A piece's strips, station by station.
#[derive(Debug, Clone)]
struct Row {
    piece: Conic3,
    strips: Vec<[Patch; 2]>,
    /// On the face's surface; otherwise on its copy claiming none.
    exact: bool,
}

/// A vertex of a face triangulated in a plane: a ring's point at a
/// station (its point at station 0), or one of the face's own.
#[derive(Debug, Clone, Copy)]
enum Corner {
    Ring(DVec3, usize),
    Own(usize),
}

/// Which face a [`Patchwork`] is.
#[derive(Debug, Clone, Copy)]
enum Which {
    /// Input segment `side`'s flat face.
    Side(u32),
    /// A part turn's end: its name and plane.
    End(FacePart, DVec3, f64),
}

/// A flat face's triangles: a flat ring, disc or sector, or a part
/// turn's end.
#[derive(Debug, Clone)]
struct Patchwork {
    face: Which,
    /// Rings whose parallels bound it, at station 0.
    rings: Vec<DVec3>,
    /// Its own vertices (Steiner points).
    own: Vec<DVec3>,
    tris: Vec<[Corner; 3]>,
}

/// One revolve: the separated chain, its sides' kinds and the frame.
struct Build<'a> {
    chain: &'a Chain,
    kinds: &'a [Kind],
    turn: &'a Turn,
    feature: u64,
    tol: &'a Tolerance,
}

/// What a round of building asks for when it can't finish.
enum Again {
    /// Every face again on twice the pieces round the axis.
    Halve,
    /// The ends want the profile's pieces halved: these, by loop.
    Split(Vec<Vec<Piece>>),
}

impl Build<'_> {
    /// The solid, with Steiner points moved in from flat corners if
    /// `flat_corners` (`flat_found` set when a triangulation without
    /// found one).
    fn solid(
        &self,
        flat_corners: bool,
        flat_found: &mut bool,
        work: &mut Work,
    ) -> Result<Solid, KernelError> {
        let mut pieces: Vec<Vec<Piece>> = self
            .chain
            .loops
            .iter()
            .map(|lp| {
                lp.iter()
                    .map(|seg| {
                        Ok(Piece {
                            side: seg.side,
                            curve: self.turn.place_conic(&seg.conic)?,
                        })
                    })
                    .collect::<Result<_, PatchError>>()
            })
            .collect::<Result<_, _>>()?;
        let mut lathe = self.turn.lathe(self.turn.first_pieces())?;
        for _ in 0..MAX_ROUNDS {
            match self.round(&pieces, &lathe, flat_corners, flat_found, work)? {
                Ok(solid) => return Ok(solid),
                Err(Again::Halve) => {
                    lathe = lathe.halved().map_err(|_| KernelError::TooComplex)?;
                }
                Err(Again::Split(finer)) => pieces = finer,
            }
        }
        Err(KernelError::TooComplex)
    }

    /// One try at the solid on `lathe` from `pieces`.
    fn round(
        &self,
        pieces: &[Vec<Piece>],
        lathe: &Lathe,
        flat_corners: bool,
        flat_found: &mut bool,
        work: &mut Work,
    ) -> Result<Result<Solid, Again>, KernelError> {
        let count: usize = pieces.iter().map(Vec::len).sum();
        if count.saturating_mul(lathe.pieces()).saturating_mul(2) > MAX_PATCHES {
            return Err(KernelError::TooComplex);
        }
        let mut walls: Vec<Vec<Wall>> = Vec::with_capacity(pieces.len());
        for lp in pieces {
            let mut row = Vec::with_capacity(lp.len());
            for piece in lp {
                match self.wall(piece, lathe, work)? {
                    Some(wall) => row.push(wall),
                    None => return Ok(Err(Again::Halve)),
                }
            }
            walls.push(row);
        }
        let mut patchworks = Vec::new();
        for wall in walls.iter().flatten().filter(|w| w.flat) {
            match self.flat_face(wall, lathe, flat_corners, flat_found, work)? {
                Some(face) => patchworks.push(face),
                None => return Ok(Err(Again::Halve)),
            }
        }
        if !self.turn.is_full() {
            match self.ends(&walls, lathe, flat_corners, flat_found, work)? {
                Ok(ends) => patchworks.extend(ends),
                Err(finer) => return Ok(Err(Again::Split(finer))),
            }
        }
        let mut assembly = Assembly {
            build: self,
            lathe,
            builder: MeshBuilder::new(),
            rings: BTreeMap::new(),
            faces: BTreeMap::new(),
        };
        for wall in walls.iter().flatten() {
            for row in &wall.rows {
                let face = assembly.face(wall.side, !row.exact);
                assembly.strips(&row.piece, &row.strips, face)?;
            }
            for (cap, pole) in &wall.caps {
                let face = assembly.face(wall.side, true);
                assembly.cap(cap, *pole, face)?;
            }
        }
        for patchwork in &patchworks {
            assembly.patchwork(patchwork)?;
        }
        let mesh = assembly.builder.build().map_err(|e| match e {
            BuildError::TooManyPatches(_) => KernelError::TooComplex,
            _ => KernelError::Profile(ProfileError::Triangulation),
        })?;
        work.spend(mesh.tris().len())?;
        // A face per surface: collinear segments' faces are one.
        Ok(Ok(Solid::finished(mesh, self.tol, work)?))
    }

    /// The face of `piece` on `lathe`, or `None` if a fitted band wants
    /// more pieces round the axis.
    fn wall(
        &self,
        piece: &Piece,
        lathe: &Lathe,
        work: &mut Work,
    ) -> Result<Option<Wall>, KernelError> {
        let kind = &self.kinds[piece.side as usize];
        let meridian = piece.curve.reversed();
        let mut wall = Wall {
            side: piece.side,
            meridians: Vec::new(),
            rows: Vec::new(),
            caps: Vec::new(),
            flat: false,
        };
        match kind {
            Kind::Axis => wall.meridians.push(meridian),
            Kind::Flat { .. } => {
                wall.meridians.push(meridian);
                wall.flat = true;
            }
            Kind::Exact { .. } | Kind::Fitted { .. } => {
                if !self.revolved(&mut wall, meridian, kind, lathe, work)? {
                    return Ok(None);
                }
            }
        }
        Ok(Some(wall))
    }

    /// The strips and pole caps of `meridian`'s curved face into `wall`:
    /// exact strips for exact kinds, fitted bands otherwise, and a fitted
    /// cap where the meridian ends on the axis (halved first if both
    /// ends do). False if a band wants more pieces round the axis.
    fn revolved(
        &self,
        wall: &mut Wall,
        meridian: Conic3,
        kind: &Kind,
        lathe: &Lathe,
        work: &mut Work,
    ) -> Result<bool, KernelError> {
        let form = kind.form();
        let straight = matches!(kind, Kind::Exact { straight: true, .. });
        // A cone's cap is fitted with linear rulings: the exact ones
        // would have their control points on the apex.
        let cap_curve = |c: &Conic3| {
            if straight {
                Conic3::line(c.p0, c.p1)
            } else {
                Ok(*c)
            }
        };
        let (start, end) = (lathe.on_axis(meridian.p0), lathe.on_axis(meridian.p1));
        let halves = if start && end {
            meridian.split_half()?.to_vec()
        } else {
            vec![meridian]
        };
        let last = halves.len() - 1;
        let mut body = Vec::new();
        let (mut head, mut tail) = (None, None);
        for (i, half) in halves.iter().enumerate() {
            if i == 0 && start {
                let cap =
                    pole_cap_with(lathe, &cap_curve(half)?, Pole::Start, &form, self.tol, work)?;
                body.extend_from_slice(&cap.rest);
                head = Some(cap);
            } else if i == last && end {
                let cap =
                    pole_cap_with(lathe, &cap_curve(half)?, Pole::End, &form, self.tol, work)?;
                body.extend_from_slice(&cap.rest);
                tail = Some(cap);
            } else {
                body.push(*half);
            }
        }
        if let Some(cap) = &head {
            wall.meridians.push(cap.meridian);
        }
        let n = lathe.pieces();
        for piece in body {
            match kind {
                Kind::Fitted { form } => {
                    let Some(band) = fitted_band_with(lathe, &piece, form, self.tol, work)? else {
                        return Ok(false);
                    };
                    for (j, piece) in band.pieces.iter().enumerate() {
                        wall.meridians.push(*piece);
                        wall.rows.push(Row {
                            piece: *piece,
                            strips: band.strips[j * n..(j + 1) * n].to_vec(),
                            exact: false,
                        });
                    }
                }
                _ => {
                    let piece = if straight {
                        ruling(&piece, self.turn)?
                    } else {
                        piece
                    };
                    work.spend(n)?;
                    let strips = (0..n)
                        .map(|k| {
                            revolution_strip(
                                &lathe.parallel(piece.p0, k)?,
                                &lathe.parallel(piece.p1, k)?,
                                &lathe.meridian(&piece, k)?,
                                &lathe.meridian(&piece, k + 1)?,
                                lathe.origin(),
                                lathe.axis(),
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    wall.meridians.push(piece);
                    wall.rows.push(Row {
                        piece,
                        strips,
                        exact: true,
                    });
                }
            }
        }
        if let Some(cap) = &tail {
            wall.meridians.push(cap.meridian);
        }
        wall.caps.extend(head.map(|c| (c, Pole::Start)));
        wall.caps.extend(tail.map(|c| (c, Pole::End)));
        Ok(true)
    }

    /// A flat face's triangles: the region between its two rings (one
    /// may be a point on the axis), and for a part turn the piece itself
    /// at both ends, triangulated as an extrude's caps. `None` if the
    /// caps want the rings' arcs halved: more pieces round the axis.
    fn flat_face(
        &self,
        wall: &Wall,
        lathe: &Lathe,
        flat_corners: bool,
        flat_found: &mut bool,
        work: &mut Work,
    ) -> Result<Option<Patchwork>, KernelError> {
        let Kind::Flat { height, .. } = self.kinds[wall.side as usize] else {
            unreachable!("only flat faces");
        };
        let turn = self.turn;
        let piece = wall.meridians[0];
        let (outer, inner) = if lathe.radius(piece.p0) > lathe.radius(piece.p1) {
            (piece.p0, piece.p1)
        } else {
            (piece.p1, piece.p0)
        };
        let centre = lathe.on_axis(inner);
        // In the plane: `x` and `n`, counter-clockwise the way the
        // stations turn, so facing `x × n = −y`.
        let foot = turn.origin + turn.y * height;
        let flat = |q: DVec3| {
            let d = q - foot;
            DVec2::new(d.dot(turn.x), d.dot(turn.n))
        };
        let arc = |p: DVec3, k: usize| -> Result<Conic2, KernelError> {
            let a = lathe.parallel(p, k)?;
            Ok(Conic2::new(flat(a.p0), flat(a.c), a.w, flat(a.p1))?)
        };
        let line = |p: DVec3, q: DVec3| Conic2::line(flat(p), flat(q));
        let n = lathe.pieces();
        let mut loops: Vec<Vec<(Corner, Conic2)>> = Vec::new();
        let mut around = Vec::with_capacity(n);
        for k in 0..n {
            around.push((Corner::Ring(outer, k), arc(outer, k)?));
        }
        let mut rings = vec![outer];
        if !centre {
            rings.push(inner);
        }
        if turn.is_full() {
            loops.push(around);
            if !centre {
                let mut hole = Vec::with_capacity(n);
                for k in (0..n).rev() {
                    hole.push((Corner::Ring(inner, k + 1), arc(inner, k)?.reversed()));
                }
                loops.push(hole);
            }
        } else {
            let at = |p: DVec3, k: usize| lathe.turned(p, k);
            around.push((Corner::Ring(outer, n), line(at(outer, n), at(inner, n))?));
            if !centre {
                for k in (0..n).rev() {
                    around.push((Corner::Ring(inner, k + 1), arc(inner, k)?.reversed()));
                }
            }
            around.push((Corner::Ring(inner, 0), line(inner, outer)?));
            loops.push(around);
        }
        let conics: Vec<Vec<Conic2>> = loops
            .iter()
            .map(|lp| lp.iter().map(|&(_, c)| c).collect())
            .collect();
        let at = self.chain.sides[wall.side as usize].at;
        let (depths, caps) = match triangulated(&conics, self.tol, flat_corners, flat_found, work) {
            Ok(done) => done,
            // Detail too small for the resolution: the face's own.
            Err(KernelError::Profile(_)) => {
                return Err(ProfileError::TooFine(at.0, at.1).into());
            }
            Err(e) => return Err(e),
        };
        if depths.iter().any(|d| d[..] != [0]) {
            return Ok(None);
        }
        let corners: Vec<Corner> = loops.iter().flatten().map(|&(c, _)| c).collect();
        let own: Vec<DVec3> = caps
            .steiner
            .iter()
            .map(|s| foot + turn.x * s.x + turn.n * s.y)
            .collect();
        let Kind::Flat {
            surface: Surface::Plane { n: normal, .. },
            ..
        } = self.kinds[wall.side as usize]
        else {
            unreachable!("flat faces are planes");
        };
        let keep = normal.dot(turn.y) < 0.0;
        Ok(Some(Patchwork {
            face: Which::Side(wall.side),
            rings,
            own,
            tris: patchwork_tris(&caps.tris, &corners, keep),
        }))
    }

    /// A part turn's two ends: the profile's region at station 0 and at
    /// the last, triangulated as an extrude's caps on the walls' meridian
    /// pieces. If the caps want pieces halved, the profile's pieces with
    /// those halved, for another round.
    fn ends(
        &self,
        walls: &[Vec<Wall>],
        lathe: &Lathe,
        flat_corners: bool,
        flat_found: &mut bool,
        work: &mut Work,
    ) -> Result<Result<[Patchwork; 2], Vec<Vec<Piece>>>, KernelError> {
        let turn = self.turn;
        let flat = |q: DVec3| {
            let d = q - turn.origin;
            DVec2::new(d.dot(turn.x), d.dot(turn.y))
        };
        // The pieces in the profile's direction, by loop.
        let pieces: Vec<Vec<Piece>> = walls
            .iter()
            .map(|lp| {
                lp.iter()
                    .flat_map(|wall| {
                        wall.meridians.iter().rev().map(|m| Piece {
                            side: wall.side,
                            curve: m.reversed(),
                        })
                    })
                    .collect()
            })
            .collect();
        let conics = pieces
            .iter()
            .map(|lp| {
                lp.iter()
                    .map(|p| {
                        let c = &p.curve;
                        Conic2::new(flat(c.p0), flat(c.c), c.w, flat(c.p1))
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect::<Result<Vec<_>, _>>()?;
        let flat_pieces: Vec<&Piece> = pieces.iter().flatten().collect();
        let (depths, caps) = triangulated(&conics, self.tol, flat_corners, flat_found, work)
            .map_err(|e| match e {
                KernelError::Profile(e) => {
                    let at = |l: usize, s: usize| {
                        let i = pieces[..l].iter().map(Vec::len).sum::<usize>() + s;
                        flat_pieces
                            .get(i)
                            .map_or((l, s), |p| self.chain.sides[p.side as usize].at)
                    };
                    KernelError::Profile(remap(e, at))
                }
                e => e,
            })?;
        if depths.iter().any(|d| d[..] != [0]) {
            let mut i = 0;
            let mut finer = Vec::with_capacity(pieces.len());
            for lp in &pieces {
                let mut out = Vec::with_capacity(lp.len());
                for piece in lp {
                    for curve in split_like(&piece.curve, &depths[i])? {
                        out.push(Piece {
                            side: piece.side,
                            curve,
                        });
                    }
                    i += 1;
                }
                finer.push(out);
            }
            return Ok(Err(finer));
        }
        let last = lathe.pieces();
        let starts: Vec<DVec3> = flat_pieces.iter().map(|p| p.curve.p0).collect();
        let own: Vec<DVec3> = caps.steiner.iter().map(|s| turn.place(*s)).collect();
        let back = -turn.n;
        let start = Patchwork {
            face: Which::End(FacePart::StartCap, back, back.dot(turn.origin)),
            rings: Vec::new(),
            own: own.clone(),
            // Counter-clockwise in `(x, y)` faces `x × y`, on: the start
            // faces back.
            tris: patchwork_tris(
                &caps.tris,
                &starts
                    .iter()
                    .map(|&p| Corner::Ring(p, 0))
                    .collect::<Vec<_>>(),
                false,
            ),
        };
        let on = turn.normal_at(lathe, last);
        let end = Patchwork {
            face: Which::End(FacePart::EndCap, on, on.dot(turn.origin)),
            rings: Vec::new(),
            own: own.iter().map(|&p| lathe.turned(p, last)).collect(),
            tris: patchwork_tris(
                &caps.tris,
                &starts
                    .iter()
                    .map(|&p| Corner::Ring(p, last))
                    .collect::<Vec<_>>(),
                true,
            ),
        };
        Ok(Ok([start, end]))
    }
}

/// The straight piece as a cone's ruling: the control point at the
/// geometric mean of its ends' distances from the apex, which on a cone
/// are as their distances from the axis, `ρ0` and `ρ1`: `(p0·√ρ1 +
/// p1·√ρ0)/(√ρ0 + √ρ1)` (see [`crate::sweep::cone_ruling`]), with no apex
/// to lose precision on (a cylinder's is far away), and the midpoint for
/// equal radii.
fn ruling(piece: &Conic3, turn: &Turn) -> Result<Conic3, PatchError> {
    let radius = |p: DVec3| {
        let v = p - turn.origin;
        (v - turn.y * v.dot(turn.y)).length()
    };
    let (r0, r1) = (radius(piece.p0).sqrt(), radius(piece.p1).sqrt());
    Conic3::new(
        piece.p0,
        (piece.p0 * r1 + piece.p1 * r0) / (r0 + r1),
        1.0,
        piece.p1,
    )
}

/// The region bounded by `loops` (outer loops counter-clockwise, holes
/// clockwise), triangulated as an extrude's caps: for each input segment
/// (through the loops in order) the halving depths of the pieces the caps
/// made of it (`[0]` if none), and the caps. Profile errors name the
/// segments by these loops.
fn triangulated(
    loops: &[Vec<Conic2>],
    tol: &Tolerance,
    flat_corners: bool,
    flat_found: &mut bool,
    work: &mut Work,
) -> Result<(Vec<Vec<u8>>, cap::Cap), KernelError> {
    let margin = tol.resolution();
    let mut curve = 0u64;
    let profile = Profile {
        loops: loops
            .iter()
            .map(|lp| Loop {
                segments: lp
                    .iter()
                    .map(|&conic| {
                        curve += 1;
                        Segment {
                            conic,
                            curve: curve - 1,
                        }
                    })
                    .collect(),
            })
            .collect(),
    };
    profile.check()?;
    let mut chain = Chain::new(&profile, margin)?;
    chain.separate(work)?;
    // The plain caps: a face's or an end's region isn't refined for
    // quality here.
    let mode = if flat_corners {
        Mode::FLAT_CORNERS_PLAIN
    } else {
        Mode::PLAIN
    };
    let mut fork = None;
    let (chain, caps) = cap::triangulate(
        Rounds::new(chain),
        margin,
        mode,
        &mut fork,
        &mut false,
        work,
    )?;
    if fork.is_some() {
        *flat_found = true;
    }
    let mut depths = vec![Vec::new(); curve as usize];
    for seg in chain.flat().0 {
        depths[seg.side as usize].push(seg.depth);
    }
    Ok((depths, caps))
}

/// `curve` halved as a chain's segment was, into pieces of halving
/// depths `depths` in order (the leaves of the halving, left to right).
fn split_like(curve: &Conic3, depths: &[u8]) -> Result<Vec<Conic3>, KernelError> {
    fn leaves(
        curve: Conic3,
        depth: u8,
        depths: &mut std::slice::Iter<'_, u8>,
        out: &mut Vec<Conic3>,
    ) -> Result<(), KernelError> {
        match depths.as_slice().first() {
            Some(&d) if d == depth => {
                depths.next();
                out.push(curve);
                Ok(())
            }
            Some(&d) if d > depth => {
                let [a, b] = curve.split_half()?;
                leaves(a, depth + 1, depths, out)?;
                leaves(b, depth + 1, depths, out)
            }
            _ => Err(KernelError::Profile(ProfileError::Triangulation)),
        }
    }
    let mut out = Vec::new();
    let mut iter = depths.iter();
    leaves(*curve, 0, &mut iter, &mut out)?;
    if iter.next().is_some() {
        return Err(KernelError::Profile(ProfileError::Triangulation));
    }
    Ok(out)
}

/// The caps' triangles (counter-clockwise, vertices numbered through
/// `corners` and then the caps' own points) as corners, turned round
/// unless `keep`.
fn patchwork_tris(tris: &[[u32; 3]], corners: &[Corner], keep: bool) -> Vec<[Corner; 3]> {
    let corner = |v: u32| {
        let v = v as usize;
        if v < corners.len() {
            corners[v]
        } else {
            Corner::Own(v - corners.len())
        }
    };
    tris.iter()
        .map(|&[a, b, c]| {
            if keep {
                [corner(a), corner(b), corner(c)]
            } else {
                [corner(a), corner(c), corner(b)]
            }
        })
        .collect()
}

/// `error` with the `(loop, segment)` pairs it names mapped by `at`. A
/// loop named alone (`Short`, `Area`, `Nesting`) keeps its index: the
/// ends' loops are the profile's, in order.
fn remap(error: ProfileError, at: impl Fn(usize, usize) -> (usize, usize)) -> ProfileError {
    match error {
        ProfileError::Segment(l, s, e) => {
            let (l, s) = at(l, s);
            ProfileError::Segment(l, s, e)
        }
        ProfileError::Degenerate(l, s) => {
            let (l, s) = at(l, s);
            ProfileError::Degenerate(l, s)
        }
        ProfileError::Open(l, s) => {
            let (l, s) = at(l, s);
            ProfileError::Open(l, s)
        }
        ProfileError::Cusp(l, s) => {
            let (l, s) = at(l, s);
            ProfileError::Cusp(l, s)
        }
        ProfileError::TooFine(l, s) => {
            let (l, s) = at(l, s);
            ProfileError::TooFine(l, s)
        }
        ProfileError::Touching([(la, sa), (lb, sb)]) => {
            ProfileError::Touching([at(la, sa), at(lb, sb)])
        }
        e => e,
    }
}

/// Builds the mesh: rings found by the bits of their point at station 0,
/// so faces built apart share them (a point on the axis is one vertex at
/// every station), and each side's face and its copy claiming no surface
/// made when first used.
struct Assembly<'a> {
    build: &'a Build<'a>,
    lathe: &'a Lathe,
    builder: MeshBuilder,
    rings: BTreeMap<[u64; 3], Vec<u32>>,
    faces: BTreeMap<(u32, bool), u32>,
}

impl Assembly<'_> {
    fn ring(&mut self, p: DVec3) -> Result<Vec<u32>, KernelError> {
        let key = p.to_array().map(f64::to_bits);
        if let Some(ring) = self.rings.get(&key) {
            return Ok(ring.clone());
        }
        let ring: Vec<u32> = if self.lathe.on_axis(p) {
            in_range(p)?;
            vec![self.builder.vert(p); self.lathe.stations()]
        } else {
            let mut ring = Vec::with_capacity(self.lathe.stations());
            for q in self.lathe.ring(p) {
                in_range(q)?;
                ring.push(self.builder.vert(q));
            }
            ring
        };
        self.rings.insert(key, ring.clone());
        Ok(ring)
    }

    /// Input segment `side`'s face, or its copy claiming no surface.
    fn face(&mut self, side: u32, free: bool) -> u32 {
        if let Some(&face) = self.faces.get(&(side, free)) {
            return face;
        }
        let kind = &self.build.kinds[side as usize];
        let s = &self.build.chain.sides[side as usize];
        let face = self.builder.face(Face {
            name: FaceName::new(
                self.build.feature,
                FacePart::Side {
                    curve: s.curve,
                    segment: s.segment,
                },
            ),
            surface: if free { Surface::Free } else { kind.surface() },
            form: kind.form(),
            slack: 1.0,
        });
        self.faces.insert((side, free), face);
        face
    }

    fn strips(
        &mut self,
        piece: &Conic3,
        strips: &[[Patch; 2]],
        face: u32,
    ) -> Result<(), KernelError> {
        let (a, b) = (self.ring(piece.p0)?, self.ring(piece.p1)?);
        for (k, patches) in strips.iter().enumerate() {
            // A full turn's last strip ends at station 0.
            let next = (k + 1) % a.len();
            self.builder
                .strip([a[k], a[next]], [b[k], b[next]], patches, face);
        }
        Ok(())
    }

    fn cap(&mut self, cap: &Cap, pole: Pole, face: u32) -> Result<(), KernelError> {
        let (tip, rim) = match pole {
            Pole::Start => (cap.meridian.p0, cap.meridian.p1),
            Pole::End => (cap.meridian.p1, cap.meridian.p0),
        };
        let tip = self.ring(tip)?[0];
        let rim = self.ring(rim)?;
        for (k, patch) in cap.patches.iter().enumerate() {
            let next = (k + 1) % rim.len();
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
        Ok(())
    }

    fn patchwork(&mut self, patchwork: &Patchwork) -> Result<(), KernelError> {
        let face = match patchwork.face {
            Which::Side(side) => self.face(side, false),
            Which::End(part, n, d) => self.builder.face(Face {
                name: FaceName::new(self.build.feature, part),
                surface: Surface::Plane { n, d },
                form: Form::plane(n, d),
                slack: 1.0,
            }),
        };
        for &p in &patchwork.rings {
            let ring = self.ring(p)?;
            for k in 0..self.lathe.pieces() {
                let arc = self.lathe.parallel(p, k)?;
                self.builder
                    .edge(ring[k], ring[(k + 1) % ring.len()], arc.c, arc.w);
            }
        }
        let mut own = Vec::with_capacity(patchwork.own.len());
        for &p in &patchwork.own {
            in_range(p)?;
            own.push(self.builder.vert(p));
        }
        for tri in &patchwork.tris {
            let mut corners = [0; 3];
            for (i, corner) in tri.iter().enumerate() {
                corners[i] = match *corner {
                    Corner::Ring(p, k) => {
                        let ring = self.ring(p)?;
                        ring[k % ring.len()]
                    }
                    Corner::Own(i) => own[i],
                };
            }
            self.builder.tri(corners, face);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
